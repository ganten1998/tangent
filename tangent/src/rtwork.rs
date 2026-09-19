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
//! Off macOS there is no workgroup, [`RtReady`] never rises, and the engine
//! renders exactly as it did. Windows has `AvSetMmThreadCharacteristics` and
//! Linux has `SCHED_FIFO`; neither is this change's to add, and a pool
//! running without either is the thing the measurement forbids.

// **Not wired to the engine yet, and that is deliberate.** The pool, the
// policy and the join are one self-contained piece with its own tests; the
// lane pass that will fork onto it is the commit after this one. Landing
// them together would have put a worker pool and a rewrite of `mix_channels`
// in the same diff, which is the one shape a change to the audio thread must
// not have.
#![allow(dead_code)]

use std::mem::{size_of, transmute};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
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
pub use mac::{realtime, Workgroup};

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
pub use elsewhere::{realtime, Workgroup};

#[cfg(not(target_os = "macos"))]
mod elsewhere {
    use super::RtPlan;
    use std::sync::Arc;

    /// No workgroup off macOS, so nothing joins one and [`super::RtReady`]
    /// never rises: the engine renders serially, exactly as it did.
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

    pub fn realtime(_plan: RtPlan) -> bool {
        false
    }
}

/// One job for the pool: a lane number and the pointer to the work.
///
/// The pointer is the whole of the unsafe in this file, and the argument for
/// it is in [`LanePool::run`].
#[derive(Clone, Copy)]
struct Job {
    /// The `&(dyn Fn(usize) + Sync)` the caller handed `run`, as the two
    /// words a fat pointer is, so that no lifetime rides along with it into
    /// the pool. Valid only between a dispatch and its barrier.
    work: (*const (), *const ()),
    /// The lanes to do, as a bitmask. Bit `ch` set means lane `ch` is owed.
    lanes: u32,
}

/// The two halves of a `&dyn Fn` and the way back.
///
/// `transmute` rather than a cast, because a fat pointer is a pair and Rust
/// has no stable way to take it apart otherwise. The sizes are asserted so
/// that a future representation change is a build error and not a Tuesday.
const _: () = assert!(
    size_of::<*const (dyn Fn(usize) + Sync)>() == size_of::<(*const (), *const ())>()
);

// SAFETY: `Job` is only ever read by a worker between the dispatch that
// published it and the barrier that ends it, and the caller of
// [`LanePool::run`] is blocked for the whole of that window. See `run`.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Default for Job {
    fn default() -> Self {
        Self { work: (std::ptr::null(), std::ptr::null()), lanes: 0 }
    }
}

struct Gate {
    /// Bumped once per dispatch. A worker that wakes with an older number
    /// has nothing to do and parks again, which is what makes a spurious
    /// wake harmless.
    round: Mutex<u64>,
    wake: Condvar,
    /// Lanes not yet claimed by any worker, as a bitmask.
    owed: AtomicU32,
    /// Lanes finished. The caller waits for this to equal what it asked for.
    done: AtomicU32,
    /// Set once at teardown; a parked worker wakes, sees it and returns.
    stop: AtomicBool,
    /// How many workers are real-time and joined. See [`RtReady`].
    ready: RtReady,
    /// The current job. Written by the caller before the round is bumped and
    /// read by workers after it, so the mutex orders the two.
    job: Mutex<Job>,
}

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
    pub fn new(width: usize, wg: Option<Arc<Workgroup>>, plan: RtPlan) -> Self {
        let width = width.clamp(1, 16);
        let gate = Arc::new(Gate {
            round: Mutex::new(0),
            wake: Condvar::new(),
            owed: AtomicU32::new(0),
            done: AtomicU32::new(0),
            stop: AtomicBool::new(false),
            ready: RtReady::default(),
            job: Mutex::new(Job::default()),
        });
        let mut hands = Vec::with_capacity(width);
        for i in 0..width {
            let gate = Arc::clone(&gate);
            let wg = wg.clone();
            let hand = std::thread::Builder::new()
                .name(format!("tangent-lane-{i}"))
                .stack_size(512 * 1024)
                .spawn(move || worker(&gate, wg.as_ref(), plan))
                .ok();
            if let Some(h) = hand {
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

    /// **Run `work(ch)` for every lane in `lanes`, and return when all of
    /// them have finished.**
    ///
    /// # Safety of the pointer
    ///
    /// `work` crosses to the workers as a raw fat pointer, and the argument
    /// that this is sound has three parts, all of them enforced here rather
    /// than promised:
    ///
    /// 1. **It cannot outlive the borrow.** This function does not return
    ///    until `done == lanes`, and no worker touches the pointer after it
    ///    has marked its lane done. The caller is blocked for the whole of
    ///    the window, so the reference it lent is live throughout.
    /// 2. **No two workers ever get the same lane.** A lane is claimed by one
    ///    `fetch_and` that clears exactly one bit; the worker that observed
    ///    the bit set is the only one that can have cleared it.
    /// 3. **The caller's own work is disjoint from the workers'.** That is
    ///    the caller's to keep, and in this engine it is kept by seat
    ///    arithmetic: see `lane_seats` and
    ///    `the_seats_a_lane_owns_never_meet_another_lanes`.
    ///
    /// `work` is `Sync` because several threads call it at once.
    pub fn run(&self, lanes: u32, work: &(dyn Fn(usize) + Sync)) {
        if lanes == 0 {
            return;
        }
        if self.hands.is_empty() {
            for ch in lane_iter(lanes) {
                work(ch);
            }
            return;
        }
        self.gate.owed.store(lanes, Ordering::Release);
        self.gate.done.store(0, Ordering::Release);
        {
            let mut job = match self.gate.job.lock() {
                Ok(j) => j,
                // A poisoned gate means a worker panicked mid-block. Do the
                // work here rather than hand it to a pool in that state.
                Err(_) => {
                    for ch in lane_iter(lanes) {
                        work(ch);
                    }
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
                transmute::<*const (dyn Fn(usize) + Sync), (*const (), *const ())>(
                    std::ptr::from_ref(work),
                )
            };
            job.lanes = lanes;
        }
        if let Ok(mut round) = self.gate.round.lock() {
            *round = round.wrapping_add(1);
        }
        self.gate.wake.notify_all();
        // **The caller works too**, rather than waiting on four threads to do
        // what five could: the real-time thread is one of the cores.
        while let Some(ch) = claim(&self.gate.owed) {
            work(ch);
            self.gate.done.fetch_or(1u32 << ch, Ordering::AcqRel);
        }
        // ── the barrier, and why it is not a bare spin ──────────────────
        //
        // What is left at this point is a worker finishing a lane it already
        // holds, which is microseconds, so a few hundred spins is the right
        // first answer and a condvar would cost more than it saves.
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
        while self.gate.done.load(Ordering::Acquire) != lanes {
            spins = spins.saturating_add(1);
            if spins < SPINS_BEFORE_YIELD {
                std::hint::spin_loop();
            } else {
                std::thread::yield_now();
            }
        }
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

/// How long the barrier spins before it starts yielding the core.
///
/// A few microseconds on this machine, which covers a worker that is nearly
/// done, and short enough that a worker which has not been scheduled at all
/// gets the core rather than the spinner. See `LanePool::run`.
const SPINS_BEFORE_YIELD: u32 = 2_000;

/// The set bits of `lanes`, low to high.
fn lane_iter(lanes: u32) -> impl Iterator<Item = usize> {
    (0..u32::BITS as usize).filter(move |ch| lanes & (1u32 << ch) != 0)
}

/// Take one lane out of `owed`, or `None` when it is empty.
///
/// The `fetch_and` that clears the bit is what makes a lane one worker's:
/// whoever observed it set and cleared it owns it, and nobody else can
/// observe the same transition.
fn claim(owed: &AtomicU32) -> Option<usize> {
    loop {
        let now = owed.load(Ordering::Acquire);
        if now == 0 {
            return None;
        }
        let ch = now.trailing_zeros() as usize;
        let bit = 1u32 << ch;
        if owed
            .compare_exchange_weak(now, now & !bit, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            return Some(ch);
        }
    }
}

/// One worker's whole life: become real-time, join, then park and work.
///
/// The order matters and is argued in the module header: the policy first,
/// because joining a workgroup from a timeshare thread succeeds and buys
/// nothing. The membership is a local so that it leaves on THIS thread when
/// the body returns, which is what the API requires.
fn worker(gate: &Arc<Gate>, wg: Option<&Arc<Workgroup>>, plan: RtPlan) {
    let rt = realtime(plan);
    let joined = match wg.map(Workgroup::join) {
        Some(Ok(m)) => Some(m),
        // EALREADY (37) is this thread already in another device's
        // workgroup; EINVAL (22) is a workgroup cancelled under us. Either
        // way this worker does not count as ready.
        Some(Err(_)) | None => None,
    };
    let counted = rt && joined.is_some();
    if counted {
        gate.ready.0.fetch_add(1, Ordering::Relaxed);
    }
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
        // The pointer is read under the same mutex the dispatch wrote it
        // under, so this worker cannot see a half-published job.
        let job = {
            let Ok(job) = gate.job.lock() else { break };
            *job
        };
        if job.lanes == 0 || job.work.0.is_null() {
            continue;
        }
        // SAFETY: the two words the dispatch published, put back together as
        // the reference they came from. The caller of `LanePool::run` is
        // blocked on its barrier until every claimed lane below is marked
        // done, so that reference is live for the whole of this loop. See
        // `run` for the three parts of the argument.
        let work: &(dyn Fn(usize) + Sync) = unsafe {
            &*transmute::<(*const (), *const ()), *const (dyn Fn(usize) + Sync)>(job.work)
        };
        while let Some(ch) = claim(&gate.owed) {
            work(ch);
            gate.done.fetch_or(1u32 << ch, Ordering::AcqRel);
        }
    }
    if counted {
        gate.ready.0.fetch_sub(1, Ordering::Relaxed);
    }
    // `joined` drops here, on this thread, leaving the workgroup.
    drop(joined);
}

#[cfg(test)]
mod tests {
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
        let pool = LanePool::new(4, None, plan);
        let counts: Vec<AtomicUsize> = (0..32).map(|_| AtomicUsize::new(0)).collect();
        let lanes = 0b1010_1111_0000_1111_0101_0101_1111_0000u32;
        for _ in 0..200 {
            pool.run(lanes, &|ch| {
                counts[ch].fetch_add(1, Ordering::Relaxed);
            });
        }
        for ch in 0..32 {
            let want = if lanes & (1u32 << ch) != 0 { 200 } else { 0 };
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
        let pool = LanePool::new(1, None, plan);
        let seen = AtomicUsize::new(0);
        pool.run(0, &|_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(seen.load(Ordering::Relaxed), 0, "an empty ask ran something");
        pool.run(0b101, &|_| {
            seen.fetch_add(1, Ordering::Relaxed);
        });
        assert_eq!(seen.load(Ordering::Relaxed), 2);
    }

    /// Off macOS, and on any machine whose device will not answer, nothing is
    /// ready and the caller is told so rather than left to find out.
    #[test]
    fn a_pool_with_no_workgroup_is_never_ready() {
        let plan = RtPlan::for_block(128, 48_000).expect("plan");
        let pool = LanePool::new(4, None, plan);
        // No workgroup was handed in, so no worker can be both real-time and
        // joined, whatever the scheduler said.
        assert_eq!(pool.ready(), 0);
        assert!(pool.width() >= 1);
    }
}
