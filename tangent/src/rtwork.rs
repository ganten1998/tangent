//! **Threads that are allowed to miss the deadline as little as the audio
//! thread is.**
//!
//! Tangent renders every lane on the one CoreAudio callback, so a block costs
//! the SUM of what the desk holds while the other fourteen cores do nothing.
//! REAPER on the same machine runs fifteen workers. Closing that is the one
//! largest number in `docs/PERF-REAPER-2026-09-18.md`, and this module is the
//! part of it that is not arithmetic: the scheduling.
//!
//! # A plain thread is worse than no thread
//!
//! This is the whole reason the module exists and it is measured, not argued.
//! Fifteen ordinary `std::thread`s rendering the same plugin concurrently on
//! this M5 Pro split into two populations by core kind (medians 37 and 52 us)
//! with per-block maxima from 380 us to **2634 us** - a whole 128-frame
//! deadline, in one worker, on a machine with nothing else running. A
//! fork-join is defined by its slowest worker, so a pool containing one
//! timeshare thread loses whole blocks that the serial walk would have made.
//!
//! Two calls fix that and they are not interchangeable:
//!
//! * `thread_policy_set` with `THREAD_TIME_CONSTRAINT_POLICY` puts the thread
//!   on the real-time scheduler, with a period, a computation budget and a
//!   constraint derived from the block the device is actually granting.
//! * `os_workgroup_join` puts it in the same workgroup as the device's own IO
//!   thread, so the kernel accounts all of them against one IO cycle and can
//!   schedule them as the one unit of work they are. `AudioWorkInterval.h`
//!   names this case exactly: work "triggered by code known to be running on
//!   the device's I/O thread" with "a deadline within the current I/O cycle".
//!
//! The policy goes on FIRST. `os_workgroup_join` succeeds from a timeshare
//! thread and buys nothing there: a workgroup coordinates threads that are
//! already meeting a deadline, it does not create one, and joining without
//! the policy misinforms the accounting for every other client of the device.
//!
//! # And when either fails, there is no pool
//!
//! Every failure here is reported, never logged and carried. [`RtReady`]
//! counts the workers that are BOTH real-time and joined; the fork reads it
//! once a block and takes the serial walk whenever it is short. That is not
//! defensive programming, it is the measurement above: a fan-out that cannot
//! be scheduled is slower than not forking at all, so the honest answer to a
//! refused policy is to stop forking.
//!
//! # And on the other three platforms
//!
//! The workgroup is macOS's alone. Windows has the Multimedia Class Scheduler
//! Service, where a thread declares itself "Pro Audio" and is given the same
//! treatment the system gives its own audio path; Linux and FreeBSD have
//! `SCHED_FIFO`, which is the real-time scheduler by another name. Both are
//! wired up here, and both are REQUIRED in the same way the mach policy is:
//! a worker that cannot get one does not count toward [`RtReady`], and a pool
//! short of its width does not fork.
//!
//! What differs is only the second half. The workgroup test is applied where
//! a workgroup exists, which is macOS; elsewhere the real-time policy is the
//! whole of the bar, because there is nothing else to join. That is not a
//! relaxation of the measurement above, it is the measurement applied to a
//! platform that has one mechanism instead of two.
//!
//! On Linux and FreeBSD the policy needs permission the desktop may not
//! grant: without `RLIMIT_RTPRIO` or a `rtkit` grant, `pthread_setschedparam`
//! returns `EPERM` and the honest answer is the one macOS gives to a refused
//! join, which is to stop forking and render serially.
//!
//! # Granting it, which is an install problem and not a code one
//!
//! `EPERM` here is not a bug to work around, and there is nothing this
//! module can do about it: the fix is administrative and belongs to the
//! installer.
//!
//! **FreeBSD is not root-only**, which is the opposite of what a bare
//! `rtprio(1)` test suggests. The mechanism is `mac_priority(4)`, which
//! grants `PRIV_SCHED_RTPRIO` and `PRIV_SCHED_SETPOLICY` to members of the
//! `realtime` group, gid 47 - the direct counterpart of Linux's audio
//! group. On a stock GhostBSD install the group is already in `/etc/group`
//! and `mac_priority.ko` is already on disk; what is missing is
//! `mac_priority_load="YES"` in `loader.conf` and the membership. The
//! policy matches the GID NUMBER, so a same-named group at another gid
//! looks right and grants nothing.
//!
//! Linux's counterpart is the audio group with an `RLIMIT_RTPRIO` line in
//! `limits.d`, or rtkit where a session bus has one. Group membership on
//! its own is usually not enough; the limits line is what grants it, and
//! it is distro-dependent.
//!
//! **Neither installer does this yet.** Measured 2026-09-20 on the tester's
//! GhostBSD box: `rtprio 10 /usr/bin/true` returns EPERM for an ordinary
//! uid and succeeds once the module is loaded and the user is in `realtime`.
//! `installer/linux/install.sh` greps zero for `rtprio`, `RLIMIT_RTPRIO`,
//! `limits.d`, `rtkit`, `SCHED_FIFO` and the audio group alike, so Linux has
//! the same silent failure.
//!
//! The reason to care which: an ungranted desktop does not fail loudly. It
//! renders every lane on the callback thread and sounds exactly like a slow
//! computer, and nothing in the interface can tell the user which one they
//! have. The grant has to be something the install does rather than
//! something the user is expected to discover.
//!
//! One caution before anyone writes the test: this module takes the policy
//! through `pthread_setschedparam(SCHED_FIFO)`, not the `rtprio(2)` syscall.
//! They share `PRIV_SCHED_RTPRIO` so the privilege conclusion carries, but a
//! green `rtprio(1)` is a proxy for this code path and not a test of it.

use std::mem::{size_of, transmute};
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// The time-constraint parameters for one worker, in nanoseconds.
///
/// Derived from the block the DEVICE granted rather than the one the engine
/// asked for: `Shared::out_block_seen` is the only trustworthy figure, and a
/// plan built from a wrong one asks the kernel to schedule for a cycle that
/// does not exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RtPlan {
    pub period_ns: u64,
    pub computation_ns: u64,
    pub constraint_ns: u64,
}

impl RtPlan {
    /// The kernel refuses a computation above this and leaves the thread
    /// timeshare. Measured by bisection on this machine at 1,200,000 mach
    /// ticks; a 4096-frame buffer at 48 kHz is 85 ms and reaches it.
    const MAX_COMPUTATION_NS: u64 = 50_000_000;

    /// The plan for a device running `frames` per IO cycle at `rate` Hz.
    ///
    /// `computation` is half the period, because the kernel raises anything
    /// below `constraint / 2` to `constraint / 2` anyway, so half is the
    /// smallest ask that is honoured as written. `constraint` is the whole
    /// cycle: a worker that has not finished by then has cost the block.
    pub fn for_block(frames: u32, rate: u32) -> Option<Self> {
        if frames == 0 || rate == 0 {
            return None;
        }
        let period_ns = u64::from(frames) * 1_000_000_000 / u64::from(rate);
        let constraint_ns = period_ns;
        let computation_ns = (period_ns / 2).min(Self::MAX_COMPUTATION_NS);
        // `computation > constraint` is KERN_INVALID_ARGUMENT, which leaves
        // the thread timeshare, so the invariant is kept here rather than
        // discovered at the call.
        (computation_ns <= constraint_ns && constraint_ns > 0).then_some(Self {
            period_ns,
            computation_ns,
            constraint_ns,
        })
    }
}

/// How many workers are real-time AND in the device's workgroup right now.
#[derive(Default)]
pub struct RtReady(AtomicU32);

impl RtReady {
    pub fn count(&self) -> u32 {
        self.0.load(Ordering::Relaxed)
    }
}

#[cfg(target_os = "macos")]
pub use mac::{realtime, release_realtime, Workgroup, HAS_WORKGROUP};

#[cfg(target_os = "macos")]
mod mac {
    use super::RtPlan;
    use std::ffi::c_void;
    use std::mem::size_of;
    use std::sync::Arc;

    type AudioObjectID = u32;
    type OSStatus = i32;

    const fn fourcc(s: &[u8; 4]) -> u32 {
        u32::from_be_bytes(*s)
    }

    /// `kAudioDevicePropertyIOThreadOSWorkgroup`: "An os_workgroup_t that
    /// represents the thread workgroup the AudioDevice's IO thread belongs
    /// to. The caller is responsible for releasing the returned object."
    const IO_WORKGROUP: u32 = fourcc(b"oswg");
    const SCOPE_GLOBAL: u32 = fourcc(b"glob");

    #[repr(C)]
    struct PropertyAddress {
        selector: u32,
        scope: u32,
        element: u32,
    }

    #[link(name = "CoreAudio", kind = "framework")]
    extern "C" {
        fn AudioObjectGetPropertyData(
            id: AudioObjectID,
            address: *const PropertyAddress,
            qualifier_size: u32,
            qualifier: *const c_void,
            io_size: *mut u32,
            out_data: *mut c_void,
        ) -> OSStatus;
    }

    /// `os_workgroup_join_token_s`: a `uint32_t sig` and `char opaque[36]` on
    /// LP64, so forty bytes at alignment four. A malformed token aborts the
    /// process, which is why this is spelled out rather than boxed.
    #[repr(C)]
    #[derive(Clone, Copy)]
    struct JoinToken {
        sig: u32,
        opaque: [u8; 36],
    }

    const _: () = assert!(size_of::<JoinToken>() == 40);

    // libsystem, which every Rust binary already links, so no `#[link]`.
    extern "C" {
        fn os_workgroup_join(wg: *mut c_void, token_out: *mut JoinToken) -> i32;
        fn os_workgroup_leave(wg: *mut c_void, token: *mut JoinToken);
        fn os_workgroup_testcancel(wg: *mut c_void) -> bool;
        fn os_release(object: *mut c_void);
        fn mach_timebase_info(info: *mut MachTimebase) -> i32;
    }

    #[repr(C)]
    #[derive(Clone, Copy, Default)]
    struct MachTimebase {
        numer: u32,
        denom: u32,
    }

    /// The device's workgroup, held at +1 for as long as this value lives.
    pub struct Workgroup(*mut c_void);

    // SAFETY: an `os_workgroup_t` is a reference-counted `os_object`. Join,
    // leave, retain and release are documented thread safe, join and leave
    // are documented real-time safe, the pointer is never written through,
    // and the +1 this value holds is released exactly once in `Drop`.
    unsafe impl Send for Workgroup {}
    unsafe impl Sync for Workgroup {}

    impl Workgroup {
        /// The workgroup of the device the monitor plays into, by the device
        /// id `outpath` already resolves for the overload watch.
        ///
        /// `None` where the device will not answer, which is not
        /// hypothetical: an AudioServerPlugin need not publish the property.
        pub fn for_device(id: u32) -> Option<Self> {
            let address = PropertyAddress {
                selector: IO_WORKGROUP,
                scope: SCOPE_GLOBAL,
                element: 0,
            };
            let mut wg: *mut c_void = std::ptr::null_mut();
            let want = size_of::<*mut c_void>();
            let mut size = u32::try_from(want).ok()?;
            // SAFETY: `size` describes `wg` exactly and the HAL writes at
            // most that many bytes into it. A non-zero status leaves it null.
            let status = unsafe {
                AudioObjectGetPropertyData(
                    id,
                    &address,
                    0,
                    std::ptr::null(),
                    &mut size,
                    std::ptr::addr_of_mut!(wg).cast(),
                )
            };
            if status != 0 || wg.is_null() || size as usize != want {
                return None;
            }
            let held = Self(wg);
            // A cancelled workgroup refuses every join with EINVAL, so it is
            // no better than no workgroup and the caller hears that answer.
            // SAFETY: a live +1 reference.
            if unsafe { os_workgroup_testcancel(held.0) } {
                return None;
            }
            Some(held)
        }

        /// Join the CALLING thread, giving back a membership that leaves when
        /// it drops. Real-time safe, so a worker does this in its own body.
        pub fn join(self: &Arc<Self>) -> Result<Membership, i32> {
            let mut token = JoinToken { sig: 0, opaque: [0; 36] };
            // SAFETY: a live +1 reference, and a token this thread owns for
            // the whole life of the membership returned below.
            let rc = unsafe { os_workgroup_join(self.0, &mut token) };
            if rc != 0 {
                return Err(rc);
            }
            Ok(Membership { wg: Arc::clone(self), token })
        }
    }

    impl Drop for Workgroup {
        fn drop(&mut self) {
            // SAFETY: the +1 the HAL handed us, released exactly once.
            // Nothing reads the pointer afterwards.
            unsafe { os_release(self.0) };
        }
    }

    /// One thread's membership of one workgroup.
    ///
    /// **Dropped on the thread that made it**, which holding it as a local in
    /// the worker body guarantees: a thread that exits a workgroup it never
    /// left is undefined behaviour.
    pub struct Membership {
        wg: Arc<Workgroup>,
        token: JoinToken,
    }

    impl Drop for Membership {
        fn drop(&mut self) {
            // SAFETY: the token this same thread's join wrote, handed back
            // exactly once, while the `Arc` still holds the workgroup alive.
            unsafe { os_workgroup_leave(self.wg.0, &mut self.token) };
        }
    }

    /// **macOS has a workgroup as well as a policy**, and a worker counts
    /// as ready only with both. See the module header.
    pub const HAS_WORKGROUP: bool = true;

    /// Nothing to hand back here: the [`Membership`]'s own `Drop` leaves the
    /// workgroup, on the thread that joined it. Windows has a handle that
    /// does need returning, which is why this exists at all.
    pub fn release_realtime() {}

    /// Put the CALLING thread on the real-time scheduler. `true` only on
    /// `KERN_SUCCESS`; anything else leaves it timeshare, which the pool
    /// treats as "there is no pool".
    pub fn realtime(plan: RtPlan) -> bool {
        let mut tb = MachTimebase::default();
        // SAFETY: a plain out parameter of exactly the declared width.
        if unsafe { mach_timebase_info(&mut tb) } != 0 || tb.numer == 0 || tb.denom == 0 {
            return false;
        }
        // **The policy is in MACH TICKS, not nanoseconds.** The timebase here
        // is 125/3, so a tick is 41.667 ns and a plan written in nanoseconds
        // would ask for twenty-four times what it meant. On an Intel Mac the
        // timebase is 1/1 and the same mistake is invisible.
        let ticks = |ns: u64| -> u32 {
            let t = u128::from(ns) * u128::from(tb.denom) / u128::from(tb.numer);
            u32::try_from(t).unwrap_or(u32::MAX)
        };
        let policy = libc::thread_time_constraint_policy {
            period: ticks(plan.period_ns),
            computation: ticks(plan.computation_ns),
            constraint: ticks(plan.constraint_ns),
            // Documented ignored. Zero is what every other host writes.
            preemptible: 0,
        };
        if policy.computation > policy.constraint || policy.constraint == 0 {
            return false;
        }
        // `pthread_mach_thread_np` and not `mach_thread_self`: the latter
        // returns a send right at +1 that leaks a mach port per worker
        // unless it is handed back, and libc marks it deprecated besides.
        // SAFETY: both take no arguments and the port names this thread.
        let port = unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) };
        // SAFETY: `policy` is a live, correctly typed local, and the count is
        // the one libc derives from that same struct, so the two agree by
        // construction.
        let kr = unsafe {
            libc::thread_policy_set(
                port,
                libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
                std::ptr::addr_of!(policy) as libc::thread_policy_t,
                libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
            )
        };
        kr == 0
    }
}

#[cfg(not(target_os = "macos"))]
pub use elsewhere::{realtime, release_realtime, Workgroup, HAS_WORKGROUP};

#[cfg(not(target_os = "macos"))]
mod elsewhere {
    use super::RtPlan;
    use std::sync::Arc;

    /// **There is no workgroup to join off macOS**, so the readiness test is
    /// the real-time policy alone. See the module header: this is the same
    /// bar applied to a platform with one mechanism rather than two, not a
    /// lower one.
    pub const HAS_WORKGROUP: bool = false;

    pub struct Workgroup;
    pub struct Membership;

    impl Workgroup {
        pub fn for_device(_id: u32) -> Option<Self> {
            None
        }
        pub fn join(self: &Arc<Self>) -> Result<Membership, i32> {
            Err(0)
        }
    }

    /// **Windows: the Multimedia Class Scheduler Service.** A thread that
    /// registers as "Pro Audio" is scheduled the way the system schedules its
    /// own audio path, which is what every Windows host does and the only
    /// supported way to ask. `AvSetMmThreadPriority` at CRITICAL puts it at
    /// the top of that class.
    ///
    /// The two symbols are declared here rather than pulled in as another
    /// `windows-sys` feature, which is the same choice the macOS half makes
    /// for the six workgroup symbols: one link attribute against a system
    /// library that has shipped since Vista.
    ///
    /// The handle has to be handed back ON THIS THREAD, so it is parked in a
    /// thread-local and returned by [`release_realtime`] when the worker
    /// leaves, exactly as the workgroup membership is.
    #[cfg(target_os = "windows")]
    pub fn realtime(_plan: RtPlan) -> bool {
        use std::ffi::c_void;
        const AVRT_PRIORITY_CRITICAL: i32 = 2;
        #[link(name = "avrt")]
        extern "system" {
            fn AvSetMmThreadCharacteristicsW(task: *const u16, index: *mut u32) -> *mut c_void;
            fn AvSetMmThreadPriority(handle: *mut c_void, priority: i32) -> i32;
        }
        // "Pro Audio", NUL-terminated, as UTF-16.
        const TASK: [u16; 10] = [
            b'P' as u16, b'r' as u16, b'o' as u16, b' ' as u16, b'A' as u16, b'u' as u16,
            b'd' as u16, b'i' as u16, b'o' as u16, 0,
        ];
        let mut index: u32 = 0;
        // SAFETY: a NUL-terminated wide literal and a live out parameter. A
        // failure returns null, which is checked before anything uses it.
        let handle = unsafe { AvSetMmThreadCharacteristicsW(TASK.as_ptr(), &mut index) };
        if handle.is_null() {
            return false;
        }
        // SAFETY: the handle this call just returned, on the thread it was
        // made for.
        if unsafe { AvSetMmThreadPriority(handle, AVRT_PRIORITY_CRITICAL) } == 0 {
            // Registered but not promoted: hand it back and count as no pool
            // rather than fork onto an ordinary thread.
            release_realtime();
            return false;
        }
        HANDLE.with(|h| h.set(handle as usize));
        true
    }

    #[cfg(target_os = "windows")]
    thread_local! {
        static HANDLE: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }

    /// Give back whatever [`realtime`] took, on the thread that took it.
    #[cfg(target_os = "windows")]
    pub fn release_realtime() {
        use std::ffi::c_void;
        #[link(name = "avrt")]
        extern "system" {
            fn AvRevertMmThreadCharacteristics(handle: *mut c_void) -> i32;
        }
        let handle = HANDLE.with(|h| h.replace(0));
        if handle != 0 {
            // SAFETY: a handle this thread took and has not yet returned.
            unsafe { AvRevertMmThreadCharacteristics(handle as *mut c_void) };
        }
    }

    /// **Linux and FreeBSD: `SCHED_FIFO`.** The real-time scheduler by
    /// another name, and the one every audio client on these systems asks
    /// for.
    ///
    /// The priority is deliberately LOW within the class. A worker sits
    /// below the audio callback it is serving, so that a pool which somehow
    /// spun could never starve the thread holding the device; the barrier
    /// yields after a bounded spin for the same reason. `EPERM` is the
    /// ordinary answer on a desktop with no `RLIMIT_RTPRIO` and no rtkit
    /// grant, and it means there is no pool, which is the honest outcome.
    #[cfg(not(target_os = "windows"))]
    pub fn realtime(_plan: RtPlan) -> bool {
        // SAFETY: the two priority queries take a policy and return a bound
        // or -1; nothing is dereferenced.
        let (lo, hi) = unsafe {
            (
                libc::sched_get_priority_min(libc::SCHED_FIFO),
                libc::sched_get_priority_max(libc::SCHED_FIFO),
            )
        };
        if lo < 0 || hi < lo {
            return false;
        }
        // A quarter of the way up the class: above everything timeshare,
        // comfortably below where a driver or the callback itself would sit.
        let param = libc::sched_param {
            sched_priority: lo + (hi - lo) / 4,
        };
        // SAFETY: a live, correctly typed parameter for this thread.
        let kr = unsafe {
            libc::pthread_setschedparam(libc::pthread_self(), libc::SCHED_FIFO, &param)
        };
        kr == 0
    }

    #[cfg(not(target_os = "windows"))]
    pub fn release_realtime() {}
}


/// One dispatch for the pool: the work, the list it walks, and how many
/// hands share the walk.
///
/// The two pointers are the whole of the unsafe in this file, and the
/// argument for them is in [`LanePool::run_list`].
#[derive(Clone, Copy)]
struct Job {
    /// The `&(dyn Fn(usize, usize) + Sync)` the caller handed `run_list`, as
    /// the two words a fat pointer is, so that no lifetime rides along with
    /// it into the pool. Valid only between a dispatch and its barrier.
    work: (*const (), *const ()),
    /// The caller's lane list, by its first element. The same window as
    /// `work`, and never turned into a slice until a claim has proved the
    /// round is still the caller's.
    list: *const u32,
    /// How long `list` is. A `u32` because [`Gate::owed`] carries the
    /// cursor in thirty-two bits; `run_list` walks a longer list serially
    /// rather than truncate one.
    len: u32,
    /// The hands sharing this round, the caller included. The batch size is
    /// read from it, see [`guided`].
    hands: u32,
}

/// The two halves of a `&dyn Fn` and the way back.
///
/// `transmute` rather than a cast, because a fat pointer is a pair and Rust
/// has no stable way to take it apart otherwise. The sizes are asserted so
/// that a future representation change is a build error and not a Tuesday.
const _: () = assert!(
    size_of::<*const (dyn Fn(usize, usize) + Sync)>() == size_of::<(*const (), *const ())>()
);

// SAFETY: `Job` is only ever dereferenced by a worker between the dispatch
// that published it and the barrier that ends it, and the caller of
// [`LanePool::run_list`] is blocked for the whole of that window. See there.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Default for Job {
    fn default() -> Self {
        Self {
            work: (std::ptr::null(), std::ptr::null()),
            list: std::ptr::null(),
            len: 0,
            hands: 1,
        }
    }
}

struct Gate {
    /// Bumped once per dispatch. A worker that wakes with an older number
    /// has nothing to do and parks again, which is what makes a spurious
    /// wake harmless.
    round: Mutex<u64>,
    wake: Condvar,
    /// **The round in the high half, the cursor into the list in the low
    /// half.** Everything before the cursor has been claimed by some hand.
    ///
    /// The round is not decoration. A worker reads the job once per round
    /// and then loops on this word; if it is descheduled between finishing
    /// its last batch and its next look, the caller can meanwhile have
    /// completed its barrier, RETURNED, and dispatched the next chunk, which
    /// stores a fresh cursor here. The worker would then wake, claim a batch
    /// of that, and call it through the previous round's pointers, which are
    /// a `mix_channels` stack frame that has gone. One callback is several
    /// chunks and therefore several rounds back to back, so the two are not
    /// far apart in time.
    ///
    /// Carrying the round makes that unrepresentable rather than unlikely: a
    /// claim only succeeds for the round the claimant read, and a worker
    /// holding a stale round finds nothing to do and parks. That holds for a
    /// cursor part-way down a list exactly as for one at zero, which is the
    /// case a mask never had: a stale worker looking at someone else's
    /// half-claimed list sees the same refusal as at its start.
    owed: AtomicU64,
    /// **Lanes finished, as a count.** The caller waits for this to reach the
    /// list's length. A count rather than a mask because a mask is one bit
    /// per lane and stops at the word's width, and because a batch is marked
    /// with one add however many lanes it held. See [`MarkDone`].
    done: AtomicU32,
    /// Set once at teardown; a parked worker wakes, sees it and returns.
    stop: AtomicBool,
    /// How many workers are real-time and joined. See [`RtReady`].
    ready: RtReady,
    /// The current job. Written by the caller before the round is bumped and
    /// read by workers after it, so the mutex orders the two.
    job: Mutex<Job>,
}

/// **The most workers one pool starts**, whatever the machine offers.
///
/// Reconsidered for unlimited tracks (W7-scale-and-budgets) and KEPT, on a
/// measurement and a risk. The number was born beside a thirty-two-bit lane
/// mask, and nothing here depends on it any more: the claim is a cursor and
/// `done` a count. What it still decides:
///
/// * **No machine it has been measured on reaches it.** The live pool is
///   the performance cores less one (four on the M5 Pro this was measured
///   on) and a bounce's is every core less one (fourteen); the tester boxes
///   the fan-out was swept on are four threads.
/// * **A bounce would use more on a bigger machine.** 1,024 lanes, each a
///   plate, export pool of plain threads, 128-frame blocks: serial 6,922
///   us a block, then 3,759 / 2,710 / 1,948 / 1,587 / 1,285 us at 1, 2, 4,
///   8 and 14 peers, still 1.23x from eight to fourteen with the last six
///   on efficiency cores (`docs/PERF-UNLIMITED.md`). A 32-core machine is
///   capped at half its cores for a bounce, unmeasured.
/// * **A wider LIVE pool is a bet that can turn the fork off.** The fork
///   needs every worker real-time and in the device's workgroup
///   (`ready() >= width()`), and the workgroup names a width of its own
///   (`os_workgroup_max_parallel_threads`, ten here). Whether a join past
///   it is refused, which would leave the pool never ready and every block
///   on one thread, has not been tried. Any raise belongs to bounces
///   alone, measured on a machine that has the cores.
const MAX_WORKERS: usize = 16;

/// **A pool of real-time threads that render lanes.**
///
/// Persistent, because a thread spawned per block would allocate a stack on
/// the audio thread and could not carry a workgroup membership worth having.
/// Parked on a condvar between blocks, which measures about 5 us to wake and
/// is the only cost the pool has when the desk is small enough not to need
/// it - and the fork asks [`Self::ready`] before paying even that.
pub struct LanePool {
    gate: Arc<Gate>,
    hands: Vec<std::thread::JoinHandle<()>>,
    width: usize,
}

impl LanePool {
    /// Start `width` workers on `plan`, joined to `wg` where there is one.
    ///
    /// Workers that cannot be made real-time, or cannot join, simply do not
    /// count toward [`Self::ready`], and a caller that checks it will never
    /// fork onto them.
    ///
    /// **`plan` is `None` for a bounce.** An offline render has no deadline
    /// to miss, and making its workers real-time would be actively wrong:
    /// they would outrank the user interface and any device that happens to
    /// be open, to finish a file nobody is waiting on a millisecond for.
    /// Plain threads running flat out are what it wants. They count toward
    /// [`Self::ready`] unconditionally, because there is no grant that can
    /// fail - which is also why an offline fan-out works on the platforms
    /// where the live one cannot get a real-time grant at all.
    pub fn new(width: usize, wg: Option<Arc<Workgroup>>, plan: Option<RtPlan>) -> Self {
        let width = width.clamp(1, MAX_WORKERS);
        let gate = Arc::new(Gate {
            round: Mutex::new(0),
            wake: Condvar::new(),
            owed: AtomicU64::new(0),
            done: AtomicU32::new(0),
            stop: AtomicBool::new(false),
            ready: RtReady::default(),
            job: Mutex::new(Job::default()),
        });
        let mut hands = Vec::with_capacity(width);
        for i in 0..width {
            let gate = Arc::clone(&gate);
            let wg = wg.clone();
            // **A hand is numbered by the workers that STARTED, not by the
            // attempt.** The caller is hand 0, so the first worker is 1; a
            // spawn that fails takes no number, so the hands are always
            // exactly `0..hands()` and a caller sizing per-hand scratch by
            // [`Self::hands`] never sees an index past its end.
            let hand = hands.len() + 1;
            let spawned = std::thread::Builder::new()
                .name(format!("tangent-lane-{i}"))
                .stack_size(512 * 1024)
                .spawn(move || worker(&gate, wg.as_ref(), plan, hand))
                .ok();
            if let Some(h) = spawned {
                hands.push(h);
            }
        }
        Self { gate, hands, width }
    }

    /// How many workers are real-time AND in the device's workgroup.
    ///
    /// The fork reads this once a block. Anything below [`Self::width`] means
    /// at least one worker is an ordinary timeshare thread, and a fan-out
    /// with one of those in it is slower than the serial walk, so the block
    /// should take the serial road.
    pub fn ready(&self) -> usize {
        self.gate.ready.count() as usize
    }

    pub fn width(&self) -> usize {
        self.hands.len().min(self.width)
    }

    /// **How many hands a [`Self::run_list`] can call its work with**, the
    /// caller included: the work only ever sees `hand < hands()`.
    ///
    /// A hand is one thread for the life of the pool, so state indexed by it
    /// (a lane's scratch buffers, which today are per lane and become per
    /// hand) is touched by one thread at a time without a lock. The caller
    /// is always hand 0 and worker `n` is always hand `n + 1`; a worker that
    /// dies keeps its number unused rather than handing it on.
    pub fn hands(&self) -> usize {
        self.hands.len() + 1
    }

    /// **Run `work(lane, hand)` for every entry of `list`, and return when
    /// all of them have finished.**
    ///
    /// `lane` is the list's own entry, whatever the caller means by it (a
    /// slot, a rank); `hand` says which thread is running it, see
    /// [`Self::hands`]. The list is the caller's and is only read.
    ///
    /// # Safety of the pointers
    ///
    /// `work` and `list` cross to the workers as raw pointers, and the
    /// argument that this is sound has three parts, all of them enforced
    /// here rather than promised:
    ///
    /// 1. **They cannot outlive the borrow.** This function does not return,
    ///    and does not finish unwinding either, until `done` has counted
    ///    every entry, and no worker touches either pointer after the add
    ///    that marks its batch. The caller is blocked for the whole of the
    ///    window, so what it lent is live throughout. See [`Barrier`].
    /// 2. **No two hands ever get the same index.** An index is claimed by
    ///    the compare-and-swap that moves the cursor past it; the hand whose
    ///    swap succeeded is the only one that can have moved it.
    /// 3. **The caller's own work is disjoint from the workers'.** That is
    ///    the caller's to keep, and it includes not listing a lane twice:
    ///    the pool hands out INDICES once each, and two equal entries are two
    ///    indices. In this engine it is kept by seat arithmetic: see
    ///    `lane_seats` and `the_seats_a_lane_owns_never_meet_another_lanes`.
    ///
    /// `work` is `Sync` because several threads call it at once.
    ///
    /// # Forks only for two
    ///
    /// A list of one is run on the caller. Waking a worker and joining it
    /// measures about 6 us, which is pure loss for one lane: the caller would
    /// have finished it in the time the worker took to wake.
    pub fn run_list(&self, list: &[u32], work: &(dyn Fn(usize, usize) + Sync)) {
        let serial = || {
            for &lane in list {
                work(lane as usize, 0);
            }
        };
        // A list longer than the cursor can count is not a desk anyone can
        // build (rows stop at 65,536), but walking it is still right where
        // truncating it would be silence.
        let Ok(len) = u32::try_from(list.len()) else {
            serial();
            return;
        };
        if len < 2 || self.hands.is_empty() {
            serial();
            return;
        }
        // **The round is decided here and PUBLISHED LAST.** The cursor is
        // stamped with it below, the workers read it back off the condvar,
        // and a claim is refused unless the two agree. The bump itself stays
        // after the job is written, because that is what orders the two: a
        // worker that sees this round is guaranteed to see the job that goes
        // with it, and there is exactly one caller of this method.
        let round = {
            let Ok(round) = self.gate.round.lock() else {
                serial();
                return;
            };
            round.wrapping_add(1)
        };
        let hands = u32::try_from(self.hands()).unwrap_or(u32::MAX);
        self.gate.owed.store(owed_word(round, 0), Ordering::Release);
        self.gate.done.store(0, Ordering::Release);
        {
            let mut job = match self.gate.job.lock() {
                Ok(j) => j,
                // A poisoned gate means a worker panicked mid-block. Do the
                // work here rather than hand it to a pool in that state.
                Err(_) => {
                    serial();
                    return;
                }
            };
            // The lifetime is erased here and restored by the barrier
            // below, which is the whole of the argument in this method's
            // doc comment.
            //
            // SAFETY: a `&dyn Fn` and a pair of pointers are the same two
            // words, asserted above. Nothing dereferences either half except
            // the worker, through the inverse transmute, inside the window
            // this call blocks for.
            job.work = unsafe {
                transmute::<*const (dyn Fn(usize, usize) + Sync), (*const (), *const ())>(
                    std::ptr::from_ref(work),
                )
            };
            job.list = list.as_ptr();
            job.len = len;
            job.hands = hands;
        }
        if let Ok(mut r) = self.gate.round.lock() {
            *r = round;
        }
        self.gate.wake.notify_all();
        // Armed before the caller's first lane, so that the wait below runs
        // on an unwind out of `work` as well as on the way out. See
        // `Barrier`.
        let barrier = Barrier { gate: &self.gate, round, len, hands };
        // **The caller works too**, rather than waiting on four threads to do
        // what five could: the real-time thread is one of the cores.
        while let Some(batch) = claim(&self.gate.owed, round, len, hands) {
            let _mark = MarkDone { done: &self.gate.done, count: batch.end - batch.start };
            for i in batch {
                work(list[i as usize] as usize, 0);
            }
        }
        drop(barrier);
    }
}

impl Drop for LanePool {
    fn drop(&mut self) {
        self.gate.stop.store(true, Ordering::Release);
        if let Ok(mut round) = self.gate.round.lock() {
            *round = round.wrapping_add(1);
        }
        self.gate.wake.notify_all();
        for h in self.hands.drain(..) {
            let _ = h.join();
        }
    }
}

/// **The caller's wait for the round, on every path out of `run_list`.**
///
/// The plain path reaches it with every index claimed and only waits. The
/// other path is the caller's own lane panicking: before this guard existed
/// that unwind left `run_list` straight past the barrier, and the frame it tore
/// down was the very closure and lane list the workers were still running
/// through. Waiting here, in a destructor, holds the frame until the last
/// worker has let go of it.
///
/// On that path the caller also stops working, so whatever it had not yet
/// claimed is claimed here and marked done UNRUN rather than left for
/// workers that may all be gone. A lane whose block is abandoned folds
/// nothing, which is the same answer a lane that panicked itself gets.
struct Barrier<'a> {
    gate: &'a Gate,
    round: u64,
    len: u32,
    hands: u32,
}

impl Drop for Barrier<'_> {
    fn drop(&mut self) {
        // Finds nothing on the plain path: the caller's loop only ends when
        // the cursor has passed the end.
        while let Some(batch) = claim(&self.gate.owed, self.round, self.len, self.hands) {
            self.gate.done.fetch_add(batch.end - batch.start, Ordering::AcqRel);
        }
        // ── the wait, and why it is not a bare spin ─────────────────────
        //
        // What is left at this point is a worker finishing a batch it
        // already holds, which is microseconds, so a few hundred spins is
        // the right first answer and a condvar would cost more than it
        // saves.
        //
        // **But a real-time thread that only spins can starve the very
        // worker it is waiting for.** That is not hypothetical: the first
        // version of this loop was a bare `spin_loop`, and under the test
        // suite - many processes, more real-time threads than cores - it
        // wedged a whole test binary at 99% of one core for nine minutes,
        // with its four workers parked at priority 97 and unable to get the
        // CPU to finish. On a machine with a game or a browser on it, that
        // is the same shape as a hang.
        //
        // So the spin is bounded and then the thread yields. Yielding from
        // an audio callback is ordinarily forbidden; here it is the
        // opposite, because everything this thread is waiting for is another
        // thread in the same workgroup with the same deadline, and giving
        // one of them the core IS the progress this thread wants.
        let mut spins = 0u32;
        while self.gate.done.load(Ordering::Acquire) < self.len {
            spins = spins.saturating_add(1);
            if spins < SPINS_BEFORE_YIELD {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
    }
}

/// **Marks a whole batch done however the work leaves.**
///
/// The barrier waits for `done` to reach the list's length, with no timeout
/// and no exit, and the thread waiting is the audio callback. A worker holds
/// no lock while it calls the work closure, so a panic in there poisons
/// nothing and is simply an unwind: it skips the line that marks the batch,
/// and the callback then spins and yields for ever. That is not a dropout, it
/// is the audio thread never returning again.
///
/// A plugin is the obvious way in, but so is any `unwrap` on the render path,
/// and this file's own note says a panic crossing the cpal boundary is
/// undefined behaviour, so the engine must not be relying on there never
/// being one. Marking from a destructor makes the barrier complete on every
/// path out, including that one.
///
/// **The WHOLE batch**, not the lanes that ran. The lanes after the panicking
/// one were claimed by this hand and no other hand can ever claim them, so
/// counting only what ran would leave the barrier one batch short for good.
/// They do not run: a lane that did not render folds nothing that block.
struct MarkDone<'a> {
    done: &'a AtomicU32,
    count: u32,
}

impl Drop for MarkDone<'_> {
    fn drop(&mut self) {
        self.done.fetch_add(self.count, Ordering::AcqRel);
    }
}

/// The same for the readiness count, so a worker that unwinds out of its
/// whole body does not leave the pool believing it is wider than it is.
struct MarkGone<'a> {
    ready: &'a RtReady,
    counted: bool,
}

impl Drop for MarkGone<'_> {
    fn drop(&mut self) {
        if self.counted {
            self.ready.0.fetch_sub(1, Ordering::Relaxed);
        }
    }
}

/// How long the barrier spins before it starts yielding the core.
///
/// A few microseconds on this machine, which covers a worker that is nearly
/// done, and short enough that a worker which has not been scheduled at all
/// gets the core rather than the spinner. See [`Barrier`].
const SPINS_BEFORE_YIELD: u32 = 2_000;

/// **How many indices one claim takes: half of an even share of what is
/// left**, and never fewer than one.
///
/// One index per claim is one compare-and-swap per lane on a single cache
/// line that every hand is hammering, which at a thousand lanes is the
/// fork's whole cost. An even share up front is the opposite mistake: lanes
/// are not equal (a plate next to an empty lane), and the hand that drew
/// the heavy share is the block. Guided batches take both: large while the
/// list is long, so the line is paid once per batch, and single lanes at
/// the tail, so the last hand to finish is never sitting on a big batch the
/// others could have shared. The factor two leaves at least half of what is
/// left for the other hands at every claim.
fn guided(remaining: u32, hands: u32) -> u32 {
    (remaining / hands.max(1).saturating_mul(2)).max(1)
}

/// Take the next batch of indices out of `owed`, or `None` when the list is
/// spent or the round is not the claimant's.
///
/// The compare-and-swap that moves the cursor is what makes a batch one
/// hand's: whoever's swap moved it from `c` to `c + n` owns `c..c + n`, and
/// nobody else can observe the same transition.
fn claim(owed: &AtomicU64, round: u64, len: u32, hands: u32) -> Option<std::ops::Range<u32>> {
    loop {
        let now = owed.load(Ordering::Acquire);
        // **Not my round, not my work.** See the field: this is the whole of
        // what stops a late worker running the previous round's closure.
        if now >> CURSOR_BITS != round & ROUND_MASK {
            return None;
        }
        let cursor = now as u32;
        if cursor >= len {
            return None;
        }
        let next = cursor + guided(len - cursor, hands);
        // Moving the cursor leaves the round where it is.
        if owed
            .compare_exchange_weak(
                now,
                owed_word(round, next),
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
        {
            return Some(cursor..next);
        }
    }
}

/// How many low bits of [`Gate::owed`] are the cursor; the rest is the
/// round. Thirty-two, which bounds a list at four billion lanes and leaves
/// the round four billion dispatches before it wraps: weeks of callbacks,
/// against a stale worker that is microseconds late.
const CURSOR_BITS: u32 = 32;
const ROUND_MASK: u64 = u64::MAX >> CURSOR_BITS;

/// The word a dispatch stores: this round, with the cursor at `cursor`.
fn owed_word(round: u64, cursor: u32) -> u64 {
    ((round & ROUND_MASK) << CURSOR_BITS) | u64::from(cursor)
}

/// One worker's whole life: become real-time, join, then park and work.
///
/// The order matters and is argued in the module header: the policy first,
/// because joining a workgroup from a timeshare thread succeeds and buys
/// nothing. The membership is a local so that it leaves on THIS thread when
/// the body returns, which is what the API requires.
///
/// `hand` is this worker's number for the life of the pool. See
/// [`LanePool::hands`].
fn worker(gate: &Arc<Gate>, wg: Option<&Arc<Workgroup>>, plan: Option<RtPlan>, hand: usize) {
    // **No plan is a bounce.** Nothing to ask the kernel for, so nothing
    // that can be refused: the thread is ordinary and it is ready.
    let rt = plan.is_none_or(realtime);
    let joined = match wg.map(Workgroup::join) {
        Some(Ok(m)) => Some(m),
        // EALREADY (37) is this thread already in another device's
        // workgroup; EINVAL (22) is a workgroup cancelled under us. Either
        // way this worker does not count as ready.
        Some(Err(_)) | None => None,
    };
    // **Both where there are both, the policy alone where there is only
    // one.** The workgroup is macOS's; Windows and the unixes have the
    // scheduler and nothing to join, so requiring a join there would mean
    // never forking at all. See the module header.
    // A plan-less worker is counted on its own: there is no workgroup for a
    // bounce to join and no policy for it to have been granted.
    let counted = rt && (plan.is_none() || joined.is_some() || !HAS_WORKGROUP);
    if counted {
        gate.ready.0.fetch_add(1, Ordering::Relaxed);
    }
    let _gone = MarkGone {
        ready: &gate.ready,
        counted,
    };
    let mut seen = 0u64;
    loop {
        let round = {
            let Ok(guard) = gate.round.lock() else { break };
            let Ok(guard) = gate.wake.wait_while(guard, |r| *r == seen && !gate.stop.load(Ordering::Acquire))
            else {
                break;
            };
            *guard
        };
        if gate.stop.load(Ordering::Acquire) {
            break;
        }
        seen = round;
        // The job is read under the same mutex the dispatch wrote it under,
        // so this worker cannot see a half-published one. It may be a LATER
        // round's job than `round`; then every claim below is refused by the
        // stamp before either pointer is touched.
        let job = {
            let Ok(job) = gate.job.lock() else { break };
            *job
        };
        if job.len == 0 || job.work.0.is_null() || job.list.is_null() {
            continue;
        }
        while let Some(batch) = claim(&gate.owed, round, job.len, job.hands) {
            // The batch is marked when this guard drops, which is on every
            // path out of `work` including an unwind. See `MarkDone`.
            let _mark = MarkDone { done: &gate.done, count: batch.end - batch.start };
            // **Made into references only now**, inside a claim that proved
            // the round is the caller's and that the caller cannot finish
            // its barrier before `_mark` drops. Built any earlier, they
            // could name a frame that a stale round has already left.
            //
            // SAFETY: the words the dispatch published, put back together as
            // the reference and the slice they came from. The caller of
            // `LanePool::run_list` is blocked on its barrier until this
            // batch is counted, so both are live for the whole of this
            // body. See `run_list` for the three parts of the argument.
            let work: &(dyn Fn(usize, usize) + Sync) = unsafe {
                &*transmute::<(*const (), *const ()), *const (dyn Fn(usize, usize) + Sync)>(
                    job.work,
                )
            };
            // SAFETY: the same window, and `len` is the length the caller's
            // slice had when it published this pointer.
            let list = unsafe { std::slice::from_raw_parts(job.list, job.len as usize) };
            for i in batch {
                work(list[i as usize] as usize, hand);
            }
        }
    }
    // `_gone` decrements the readiness count, and `joined` drops here, on
    // this thread, leaving the workgroup.
    drop(joined);
    // And whatever the platform's policy took, on the thread that took it.
    // A no-op everywhere but Windows, where the scheduler hands back a handle
    // that must be returned from here and nowhere else.
    release_realtime();
}

#[cfg(test)]
mod tests {
    /// **A worker that is late may not take the next round's work.**
    ///
    /// The bug this pins shipped. A worker reads the job once per round and
    /// then loops on the claim word. Between finishing its last batch and
    /// looking again it can be descheduled, and in that window the caller
    /// can complete its barrier, RETURN, and dispatch the next chunk, which
    /// stores a fresh cursor. The worker would wake, claim a batch of that,
    /// and call it through a pointer to a stack frame that has gone. One
    /// callback is several chunks, so the two rounds are microseconds apart,
    /// not milliseconds.
    ///
    /// It cannot be caught by running the pool, because it needs one exact
    /// interleaving and nothing in this repo can force one. What CAN be
    /// tested is the word: a claim is refused unless the round matches, so
    /// the interleaving stops being representable. And with a cursor rather
    /// than a mask, the case that matters is a list PART-WAY claimed: the
    /// stale worker finds work plainly left in the word and still takes
    /// none of it.
    #[test]
    fn a_stale_worker_claims_nothing() {
        // Round 7, five of ten indices already taken, two hands: a guided
        // batch of the five left is one.
        let owed = AtomicU64::new(owed_word(7, 5));
        assert_eq!(claim(&owed, 6, 10, 2), None, "a stale round claimed the next round's lanes");
        assert_eq!(claim(&owed, 8, 10, 2), None, "a round that has not happened claimed a lane");
        assert_eq!(owed.load(Ordering::Relaxed), owed_word(7, 5), "a refused claim moved the cursor");
        // And the right round still gets them, from where the cursor stood.
        assert_eq!(claim(&owed, 7, 10, 2), Some(5..6));
        assert_eq!(claim(&owed, 7, 10, 2), Some(6..7));
        assert_eq!(claim(&owed, 6, 10, 2), None, "a stale round claimed mid-list");
        assert_eq!(claim(&owed, 7, 10, 2), Some(7..8));
        assert_eq!(claim(&owed, 7, 10, 2), Some(8..9));
        assert_eq!(claim(&owed, 7, 10, 2), Some(9..10));
        assert_eq!(claim(&owed, 7, 10, 2), None, "a spent list handed out a lane");

        // The round is carried whole, and wraps with the counter rather than
        // colliding with the cursor.
        let wrapped = AtomicU64::new(owed_word(u64::MAX, 0));
        assert_eq!(claim(&wrapped, u64::MAX, 1, 1), Some(0..1));
        let deep = AtomicU64::new(owed_word(3, u32::MAX - 1));
        assert_eq!(claim(&deep, 3, u32::MAX, 1), Some(u32::MAX - 1..u32::MAX), "the top index was lost to the round");
        assert_eq!(deep.load(Ordering::Relaxed) >> CURSOR_BITS, 3, "the cursor carried into the round");
    }

    /// **A panic in one lane must not hang the audio thread for ever.**
    ///
    /// The barrier has no timeout and no exit, and the thread waiting on it
    /// is the callback. A worker holds no lock while it calls the work
    /// closure, so an unwind poisons nothing; it used to simply skip the
    /// line that marks the lane, leaving the callback spinning and yielding
    /// with nothing that could ever satisfy it.
    ///
    /// The guard now marks a BATCH, and the whole of it: the lanes after the
    /// one that panicked belong to this hand and no other can claim them.
    #[test]
    fn a_panic_in_one_lane_still_finishes_the_block() {
        // Asserted on the guard here, and through a running pool in
        // `a_panic_mid_batch_still_releases_the_barrier`.
        let marked = AtomicU32::new(0);
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _mark = MarkDone { done: &marked, count: 5 };
            panic!("inside the work");
        }));
        assert!(unwound.is_err());
        assert_eq!(
            marked.load(Ordering::Relaxed),
            5,
            "a batch that panicked was not counted whole, so the callback \
             would spin on the barrier for ever"
        );
    }

    /// **The readiness bar is the same everywhere; only the mechanisms
    /// differ.** macOS has a policy AND a workgroup and a worker needs both.
    /// Windows and the unixes have the scheduler and nothing to join, so
    /// requiring a join there would mean the pool never forms and the whole
    /// fan-out stayed macOS-only, which is what this commit is about.
    ///
    /// Asserted rather than left to a comment, because the rule is one
    /// boolean in one line and the failure is silent on three platforms this
    /// machine cannot run.
    #[test]
    fn a_worker_counts_as_ready_on_the_platforms_terms() {
        let counted = |rt: bool, joined: bool, has_wg: bool| rt && (joined || !has_wg);
        // macOS: both, or nothing.
        assert!(counted(true, true, true));
        assert!(!counted(true, false, true), "a macOS worker counted without its workgroup");
        assert!(!counted(false, true, true), "a worker counted without the policy");
        // Elsewhere: the policy is the whole bar, and it is still required.
        assert!(counted(true, false, false));
        assert!(!counted(false, false, false), "a worker counted with no policy at all");
        // And the constant says which world this build is in.
        assert_eq!(HAS_WORKGROUP, cfg!(target_os = "macos"));
    }

    use super::*;
    use std::sync::atomic::AtomicUsize;

    /// A plan for the block sizes this app opens at, and the two ways a plan
    /// can be refused.
    #[test]
    fn a_plan_is_the_cycle_the_device_granted() {
        let p = RtPlan::for_block(128, 48_000).expect("128 at 48k is a cycle");
        assert_eq!(p.period_ns, 2_666_666);
        assert_eq!(p.constraint_ns, 2_666_666);
        assert_eq!(p.computation_ns, 1_333_333);
        assert!(p.computation_ns <= p.constraint_ns);
        // The kernel's ceiling, which a very large buffer reaches.
        let big = RtPlan::for_block(8192, 48_000).expect("a big buffer still plans");
        assert_eq!(big.computation_ns, RtPlan::MAX_COMPUTATION_NS);
        assert!(big.computation_ns <= big.constraint_ns);
        assert_eq!(RtPlan::for_block(0, 48_000), None);
        assert_eq!(RtPlan::for_block(128, 0), None);
    }

    /// **Every lane is done exactly once**, whoever picks it up. The claim is
    /// the whole of the disjointness argument, so it is asserted directly
    /// rather than inferred from a result that happens to look right.
    #[test]
    fn every_lane_is_claimed_by_exactly_one_hand() {
        let plan = RtPlan::for_block(128, 48_000).expect("plan");
        let pool = LanePool::new(4, None, Some(plan));
        let counts: Vec<AtomicUsize> = (0..32).map(|_| AtomicUsize::new(0)).collect();
        // A scattered half of thirty-two lanes, as the list the engine walks.
        let lanes: Vec<u32> = [4u32, 5, 6, 7, 8, 10, 12, 14, 16, 17, 18, 19, 24, 25, 26, 27, 29, 31].into();
        for _ in 0..200 {
            pool.run_list(&lanes, &|ch, _hand| {
                counts[ch].fetch_add(1, Ordering::Relaxed);
            });
        }
        for ch in 0..32 {
            let want = if lanes.contains(&(ch as u32)) { 200 } else { 0 };
            assert_eq!(
                counts[ch].load(Ordering::Relaxed),
                want,
                "lane {ch} was done the wrong number of times"
            );
        }
    }

    /// An empty ask costs nothing and a pool with no threads still does the
    /// work, which is what makes the serial fallback one code path.
    #[test]
    fn a_pool_that_cannot_start_still_renders() {
        let plan = RtPlan::for_block(128, 48_000).expect("plan");
        let pool = LanePool::new(1, None, Some(plan));
        let seen = AtomicUsize::new(0);
        pool.run_list(&[], &|_, _| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(seen.load(Ordering::Relaxed), 0, "an empty ask ran something");
        pool.run_list(&[0, 2], &|_, _| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(seen.load(Ordering::Relaxed), 2);
    }

    /// Off macOS, and on any machine whose device will not answer, nothing is
    /// ready and the caller is told so rather than left to find out.
    #[test]
    fn a_pool_with_no_workgroup_is_never_ready() {
        let plan = RtPlan::for_block(128, 48_000).expect("plan");
        let pool = LanePool::new(4, None, Some(plan));
        // **On macOS.** No workgroup was handed in, so no worker can be both
        // real-time and joined, whatever the scheduler said.
        //
        // Off macOS this is deliberately NOT true and must not be asserted:
        // there is no workgroup to join on those platforms, so the real-time
        // policy is the whole bar and a worker that gets it counts. Asserting
        // zero here would make the suite red on Windows, Linux and FreeBSD
        // for doing exactly what it was changed to do.
        #[cfg(target_os = "macos")]
        assert_eq!(pool.ready(), 0);
        // What holds everywhere: readiness never exceeds the pool, and a
        // worker counts only if it was given a policy.
        assert!(pool.ready() <= pool.width());
        assert!(pool.width() >= 1);
    }

    /// A list that is not its own indices: a permutation of `0..n`, shifted,
    /// so a pool that ran the INDEX instead of the entry is caught as surely
    /// as one that ran an entry twice. 37 is prime to every length used.
    fn scrambled(n: u32, shift: u32) -> Vec<u32> {
        (0..n).map(|i| (i * 37) % n + shift).collect()
    }

    /// Busy for a stretch that differs per lane, so that hands finish out of
    /// step and race each other for the cursor rather than taking turns.
    fn uneven(lane: usize) {
        for i in 0..(lane % 7) * 40 {
            std::hint::black_box(i);
        }
    }

    /// **Every listed lane is claimed exactly once, at widths past what a
    /// mask could carry, with every hand fighting for the cursor.**
    ///
    /// 33 is one lane past the old thirty-two-bit word, 1000 a big desk, and
    /// 4096 enough lanes that guided batches run many claims deep. Plain
    /// threads (no plan), so the hands really are concurrent wherever this
    /// runs instead of being refused a policy and parked.
    #[test]
    fn every_listed_lane_is_claimed_exactly_once_under_contention() {
        let pool = LanePool::new(8, None, None);
        assert!(pool.hands() >= 2, "no worker started, so nothing contended");
        for (n, rounds) in [(33u32, 300usize), (1000, 60), (4096, 20)] {
            let shift = 5;
            let list = scrambled(n, shift);
            let counts: Vec<AtomicUsize> = (0..n + shift).map(|_| AtomicUsize::new(0)).collect();
            let bad_hand = AtomicBool::new(false);
            let hands = pool.hands();
            for _ in 0..rounds {
                pool.run_list(&list, &|lane, hand| {
                    if hand >= hands {
                        bad_hand.store(true, Ordering::Relaxed);
                    }
                    uneven(lane);
                    counts[lane].fetch_add(1, Ordering::Relaxed);
                });
            }
            assert!(!bad_hand.load(Ordering::Relaxed), "a hand past hands() at {n} lanes");
            for (lane, c) in counts.iter().enumerate() {
                let want = if lane < shift as usize { 0 } else { rounds };
                assert_eq!(
                    c.load(Ordering::Relaxed),
                    want,
                    "lane {lane} of {n} was done the wrong number of times"
                );
            }
        }
    }

    /// **Guided: half an even share of what is left, never below one, and
    /// the batches tile the list with no gap and no overlap.**
    #[test]
    fn batches_are_guided_and_tile_the_list() {
        assert_eq!(guided(100, 4), 12);
        assert_eq!(guided(7, 4), 1, "a tail smaller than the hands must still move");
        assert_eq!(guided(1, 17), 1);
        assert_eq!(guided(10, 0), 5, "no hands is read as one rather than dividing by zero");

        let (len, hands) = (100u32, 4u32);
        let owed = AtomicU64::new(owed_word(1, 0));
        let mut next = 0;
        let mut sizes = Vec::new();
        while let Some(b) = claim(&owed, 1, len, hands) {
            assert_eq!(b.start, next, "a gap or an overlap at {next}");
            assert_eq!(b.end - b.start, guided(len - b.start, hands));
            sizes.push(b.end - b.start);
            next = b.end;
        }
        assert_eq!(next, len, "the list was not spent");
        assert_eq!(sizes[0], 12, "the first claim was not a guided batch");
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]), "batches grew: {sizes:?}");
        assert_eq!(sizes.last(), Some(&1), "the tail was not single lanes: {sizes:?}");
        assert!(sizes.len() < len as usize / 2, "one lane per claim is not guided: {sizes:?}");
    }

    /// **A panic part-way through a batch still releases the barrier**, on a
    /// running pool, wherever the panic lands: on a worker, whose batch the
    /// guard counts whole, or on the caller, whose unwind must wait for the
    /// workers before leaving the frame they are running through.
    ///
    /// Run on its own thread with a deadline, so that a regression fails
    /// this test instead of hanging the suite.
    #[test]
    fn a_panic_mid_batch_still_releases_the_barrier() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pool = LanePool::new(4, None, None);
            let list = scrambled(64, 0);
            for attempt in 0..20usize {
                let inside = AtomicUsize::new(0);
                let ran: Vec<AtomicUsize> = (0..64).map(|_| AtomicUsize::new(0)).collect();
                // A different lane each attempt, so the panic lands on the
                // caller on some attempts and on a worker on others, and
                // part-way into a batch rather than at its start.
                let fatal = (attempt * 13 + 3) % 64;
                let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    pool.run_list(&list, &|lane, hand| {
                        inside.fetch_add(1, Ordering::SeqCst);
                        uneven(lane + 3);
                        ran[lane].fetch_add(1, Ordering::Relaxed);
                        let leave = (lane == fatal || (hand == 0 && attempt % 2 == 1)).then_some(());
                        inside.fetch_sub(1, Ordering::SeqCst);
                        if leave.is_some() {
                            panic!("lane {lane} on hand {hand}");
                        }
                    });
                }));
                // However it left, nobody is still inside the work: the
                // frame the closure lives in is about to be reused.
                assert_eq!(
                    inside.load(Ordering::SeqCst),
                    0,
                    "run_list left (unwound: {}) while a worker was still in the work",
                    unwound.is_err()
                );
                for (lane, r) in ran.iter().enumerate() {
                    assert!(r.load(Ordering::Relaxed) <= 1, "lane {lane} ran twice");
                }
            }
            // **Now with no workers left at all.** Every worker panics until
            // the pool counts none ready (a plan-less worker is counted until
            // it dies), so nothing but the caller can claim. Then the caller
            // itself panics on its first lane: the rest of the list has no
            // hand to claim it, and only the barrier's own drain finishes
            // the block.
            for _ in 0..1_000 {
                if pool.ready() == 0 {
                    break;
                }
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    pool.run_list(&list, &|lane, hand| {
                        uneven(lane + 3);
                        assert_eq!(hand, 0, "a worker is told to die");
                    });
                }));
                std::thread::yield_now();
            }
            let none_left = pool.ready() == 0;
            let first = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                pool.run_list(&list, &|_, _| panic!("the caller's first lane"));
            }));
            // And the pool still works afterwards, short however many
            // workers the panics took, because the caller always drains.
            let all = AtomicUsize::new(0);
            pool.run_list(&list, &|_, _| {
                all.fetch_add(1, Ordering::Relaxed);
            });
            let _ = tx.send((none_left, first.is_err(), all.load(Ordering::Relaxed)));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok((none_left, unwound, all)) => {
                assert!(none_left, "the workers would not die, so the drain was never needed");
                assert!(unwound, "the caller's panic did not reach its caller");
                assert_eq!(all, 64, "the pool lost lanes after a panic");
            }
            Err(_) => panic!("a panic mid-batch left the barrier waiting for ever"),
        }
    }

    /// **The caller's own panic waits for the workers before it leaves.**
    ///
    /// The closure and the list live in the caller's frame, so an unwind
    /// that left `run_list` while a worker was still in a lane would hand
    /// that worker a frame being torn down. The caller here dies only once a
    /// worker is provably inside a lane that takes milliseconds, which is
    /// the one moment the drain cannot hide a missing wait.
    #[test]
    fn a_callers_panic_waits_for_the_workers_before_it_leaves() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let pool = LanePool::new(4, None, None);
            let list = scrambled(64, 0);
            let mut caught_mid = 0usize;
            let mut left_early = 0usize;
            for _ in 0..10 {
                let inside = AtomicUsize::new(0);
                let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    pool.run_list(&list, &|_, hand| {
                        if hand == 0 {
                            let t = std::time::Instant::now();
                            while inside.load(Ordering::SeqCst) == 0
                                && t.elapsed() < std::time::Duration::from_secs(1)
                            {
                                std::hint::spin_loop();
                            }
                            panic!("the caller, with a worker mid-lane");
                        }
                        inside.fetch_add(1, Ordering::SeqCst);
                        let t = std::time::Instant::now();
                        while t.elapsed() < std::time::Duration::from_millis(3) {
                            std::hint::spin_loop();
                        }
                        inside.fetch_sub(1, Ordering::SeqCst);
                    });
                }));
                if unwound.is_err() {
                    caught_mid += 1;
                }
                if inside.load(Ordering::SeqCst) != 0 {
                    left_early += 1;
                }
            }
            let _ = tx.send((caught_mid, left_early));
        });
        match rx.recv_timeout(std::time::Duration::from_secs(60)) {
            Ok((caught_mid, left_early)) => {
                assert!(caught_mid > 0, "the caller never died mid-round, so nothing was tested");
                assert_eq!(left_early, 0, "run_list unwound while a worker was still in the work");
            }
            Err(_) => panic!("the caller's panic left the barrier waiting for ever"),
        }
    }

    /// **Hand indices are distinct and stable**: one hand is one thread for
    /// the life of the pool, never two at once, the caller is hand 0, and
    /// every index is below `hands()`. Per-hand scratch is sound exactly
    /// when this holds.
    #[test]
    fn hand_indices_are_distinct_and_stable() {
        let pool = LanePool::new(4, None, None);
        let hands = pool.hands();
        assert_eq!(hands, pool.width() + 1, "the caller is a hand beside the workers");
        let busy: Vec<AtomicBool> = (0..hands).map(|_| AtomicBool::new(false)).collect();
        let owner: Vec<Mutex<Option<std::thread::ThreadId>>> =
            (0..hands).map(|_| Mutex::new(None)).collect();
        // Recorded rather than asserted in the work: an assert on a worker
        // only kills that worker, and the test would never hear of it.
        let (past, shared, moved, misplaced) = (
            AtomicBool::new(false),
            AtomicBool::new(false),
            AtomicBool::new(false),
            AtomicBool::new(false),
        );
        let caller = std::thread::current().id();
        let list = scrambled(64, 0);
        for _ in 0..200 {
            pool.run_list(&list, &|lane, hand| {
                if hand >= hands {
                    past.store(true, Ordering::Relaxed);
                    return;
                }
                if busy[hand].swap(true, Ordering::AcqRel) {
                    shared.store(true, Ordering::Relaxed);
                }
                let me = std::thread::current().id();
                if let Ok(mut o) = owner[hand].lock() {
                    match *o {
                        None => *o = Some(me),
                        Some(t) if t != me => moved.store(true, Ordering::Relaxed),
                        Some(_) => {}
                    }
                }
                if (hand == 0) != (me == caller) {
                    misplaced.store(true, Ordering::Relaxed);
                }
                uneven(lane);
                busy[hand].store(false, Ordering::Release);
            });
        }
        assert!(!past.load(Ordering::Relaxed), "a hand at or past hands() {hands}");
        assert!(!shared.load(Ordering::Relaxed), "two threads ran as one hand at once");
        assert!(!moved.load(Ordering::Relaxed), "a hand moved to another thread");
        assert!(!misplaced.load(Ordering::Relaxed), "hand 0 is the caller and only the caller");
        let seen: Vec<_> = owner.iter().filter_map(|o| *o.lock().expect("owner")).collect();
        let mut distinct = seen.clone();
        distinct.sort_by_key(|t| format!("{t:?}"));
        distinct.dedup();
        assert_eq!(distinct.len(), seen.len(), "one thread ran under two hand numbers");
    }

    /// **The pool forks only for two or more lanes**: a list of one runs on
    /// the caller as hand 0 and does not wake a worker (no round is spent).
    #[test]
    fn a_list_of_one_stays_on_the_caller() {
        let pool = LanePool::new(4, None, None);
        let before = *pool.gate.round.lock().expect("round");
        let caller = std::thread::current().id();
        let seen = Mutex::new(Vec::new());
        pool.run_list(&[9], &|lane, hand| {
            seen.lock().expect("seen").push((lane, hand, std::thread::current().id()));
        });
        assert_eq!(*seen.lock().expect("seen"), vec![(9, 0, caller)]);
        assert_eq!(*pool.gate.round.lock().expect("round"), before, "one lane woke the pool");
        pool.run_list(&[], &|_, _| panic!("an empty list ran something"));
        pool.run_list(&[1, 2], &|_, _| {});
        assert_eq!(*pool.gate.round.lock().expect("round"), before + 1, "two lanes did not fork");
    }
}
