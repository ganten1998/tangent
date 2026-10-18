//! **Counting the callback's allocations, and the lanes it might confuse.**
//!
//! Two guard rails for the engine rebuild, measured BEFORE anything moves so
//! every later wave is compared against today rather than against a hope:
//!
//! * a counting `#[global_allocator]` for this test binary, ported from
//!   `tangent-host/src/instance.rs` and widened to see the `tangent-lane-*`
//!   workers a forked render runs on, with a baseline of what
//!   `render_offline` allocates today;
//! * an alias harness: a switch thrown on one lane changes that lane and no
//!   other, at the lanes where a fixed-width mask wraps or a clamp folds
//!   (0, 1, 30, 31 today; W3 widens it to 100 and 300 lanes).
//!
//! Tests only: the allocator is this binary's, and a production build has the
//! system's untouched.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// A global allocator that counts, for one armed window at a time.
///
/// `cargo test` runs tests in parallel in one process, so a global counter
/// would be measuring the whole suite. The counter is therefore keyed: the
/// thread inside [`count_allocs`] counts in a thread-local, and while it is
/// armed, any thread NAMED `tangent-lane-*` (an `rtwork` hand) counts into a
/// shared figure, because a forked render allocates on the hands as surely as
/// on the callback. Every other thread pays one relaxed load per allocation.
///
/// **Frees are counted too.** The hazard is the allocator's lock and its slow
/// paths, and `free` takes the same lock.
///
/// It sees only Rust's allocator. A plugin calling `malloc` directly is
/// invisible here, which is the plugin's contract to keep.
struct Counting;

thread_local! {
    /// `None` when this thread is not the counting one.
    static TOUCHES: Cell<Option<usize>> = const { Cell::new(None) };
    /// Bytes the counting thread asked for inside its window (see
    /// [`count_bytes`]): what a build costs, where `TOUCHES` is how often.
    static ASKED: Cell<usize> = const { Cell::new(0) };
    /// Bytes the counting thread gave back inside its window (see
    /// [`count_freed`]): what letting something go returns.
    static FREED: Cell<usize> = const { Cell::new(0) };
}

/// Add `size` to the counting thread's bytes; nothing on any other thread.
fn ask(size: usize) {
    let _ = TOUCHES.try_with(|t| {
        if t.get().is_some() {
            let _ = ASKED.try_with(|a| a.set(a.get().saturating_add(size)));
        }
    });
}

/// Add `size` to the counting thread's freed bytes; nothing on any other
/// thread.
fn give(size: usize) {
    let _ = TOUCHES.try_with(|t| {
        if t.get().is_some() {
            let _ = FREED.try_with(|f| f.set(f.get().saturating_add(size)));
        }
    });
}

/// Whether a window is open: lane hands count only then.
static ARMED: AtomicBool = AtomicBool::new(false);
/// What the lane hands did inside the open window.
static HANDS: AtomicUsize = AtomicUsize::new(0);
/// One window at a time: two tests counting at once would each see the
/// other's hands.
static WINDOW: Mutex<()> = Mutex::new(());

/// Whether this thread is an `rtwork` hand, asked of the OS by name.
///
/// `pthread_getname_np`, not `std::thread::current()`: this runs inside the
/// allocator, and `current()` can itself allocate on a thread that has never
/// asked, or panic on one whose thread-locals are being torn down, either of
/// which here would be an abort in an unrelated test. The C call reads a
/// fixed buffer and touches neither. Where there is no such call (Windows),
/// the hands are not counted and the callback thread still is.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
fn on_a_lane_hand() -> bool {
    const PREFIX: &[u8] = b"tangent-lane-";
    let mut buf = [0u8; 64];
    // SAFETY: `pthread_self` is always valid for the calling thread, and the
    // buffer is ours, of the length passed.
    let rc = unsafe {
        libc::pthread_getname_np(libc::pthread_self(), buf.as_mut_ptr().cast(), buf.len())
    };
    rc == 0 && buf.starts_with(PREFIX)
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "freebsd")))]
fn on_a_lane_hand() -> bool {
    false
}

fn touch() {
    // `try_with`, not `with`: this runs inside the allocator, and a panic
    // there during thread teardown would be a very confusing crash.
    let mine = TOUCHES
        .try_with(|c| match c.get() {
            Some(n) => {
                c.set(Some(n + 1));
                true
            }
            None => false,
        })
        .unwrap_or(false);
    if !mine && ARMED.load(Ordering::Relaxed) && on_a_lane_hand() {
        HANDS.fetch_add(1, Ordering::Relaxed);
    }
}

// SAFETY: every method forwards to `System`, unchanged, with the same
// arguments. The bookkeeping is a const thread-local `Cell` (no destructor,
// no lazy allocation) and two statics, none of which allocate through this
// allocator, so there is no re-entrancy.
unsafe impl std::alloc::GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: std::alloc::Layout) -> *mut u8 {
        touch();
        ask(layout.size());
        // SAFETY: forwarding the caller's own layout.
        unsafe { std::alloc::System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: std::alloc::Layout) {
        touch();
        give(layout.size());
        // SAFETY: forwarding the caller's own pointer and layout.
        unsafe { std::alloc::System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: std::alloc::Layout, new_size: usize) -> *mut u8 {
        touch();
        give(layout.size());
        ask(new_size);
        // SAFETY: forwarding the caller's own pointer, layout and size.
        unsafe { std::alloc::System.realloc(ptr, layout, new_size) }
    }
    unsafe fn alloc_zeroed(&self, layout: std::alloc::Layout) -> *mut u8 {
        touch();
        ask(layout.size());
        // SAFETY: forwarding the caller's own layout.
        unsafe { std::alloc::System.alloc_zeroed(layout) }
    }
}

#[global_allocator]
static COUNTING: Counting = Counting;

/// What one window counted: on the counting thread, and on the lane hands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counted {
    pub here: usize,
    pub hands: usize,
}

impl Counted {
    pub fn total(self) -> usize {
        self.here + self.hands
    }
}

/// Run `f` and say how many times it, and any lane hand meanwhile, went to
/// the allocator. Windows are serialised by a module mutex.
pub fn count_allocs<T>(f: impl FnOnce() -> T) -> (T, Counted) {
    let _window = WINDOW
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Touch the thread-local first, so the platform's own lazy setup for it
    // is not the thing being measured.
    TOUCHES.with(|c| c.set(None));
    HANDS.store(0, Ordering::SeqCst);
    TOUCHES.with(|c| c.set(Some(0)));
    ARMED.store(true, Ordering::SeqCst);
    let out = f();
    ARMED.store(false, Ordering::SeqCst);
    let here = TOUCHES.with(Cell::take).unwrap_or(0);
    let hands = HANDS.load(Ordering::SeqCst);
    (out, Counted { here, hands })
}

/// **How many bytes `f` asked the allocator for, on this thread**: every
/// allocation's size and every reallocation's new size, frees not
/// subtracted. A ceiling on what a build costs (design 7.12 pins an idle
/// lane at 256 KB), counted rather than estimated from capacities.
pub fn count_bytes<T>(f: impl FnOnce() -> T) -> (T, usize) {
    ASKED.with(|a| a.set(0));
    let (out, _) = count_allocs(f);
    (out, ASKED.with(Cell::get))
}

/// **How many bytes `f` gave back to the allocator, on this thread**: every
/// free's size and every reallocation's old size. What letting a thing go
/// returns (design 7.12: memory sized by use goes when the use does),
/// counted rather than read off a capacity that a dropped handle no longer
/// reports while the other end still holds the memory.
pub fn count_freed<T>(f: impl FnOnce() -> T) -> (T, usize) {
    FREED.with(|a| a.set(0));
    let (out, _) = count_allocs(f);
    (out, FREED.with(Cell::get))
}

/// The smallest of `tries` windows over `f`, element by element.
///
/// The hands' figure is keyed by a thread NAME, and another test's pool has
/// hands of the same names, so a window can catch a neighbour's allocation.
/// That only ever ADDS, and a real allocation in the measured path happens
/// every time; the minimum of a few windows is therefore the path's own
/// figure and never hides one.
pub fn least_of<T>(tries: usize, mut f: impl FnMut() -> T) -> (T, Counted) {
    let (mut out, mut best) = count_allocs(&mut f);
    for _ in 1..tries {
        if best.total() == 0 {
            break;
        }
        let (o, c) = count_allocs(&mut f);
        if c.total() < best.total() {
            out = o;
            best = c;
        }
    }
    (out, best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instrument::{Engine, Placed, LEGACY_LANES};
    use crate::render_golden::{noise, offline};
    use std::path::Path;
    use std::sync::Arc;

    /// A counter that is silently broken would make every assertion below
    /// pass, which is the worst possible failure for this kind of test. So:
    /// it sees an allocation here, it sees one on a lane hand, and it does
    /// not see one on any other thread.
    #[test]
    fn the_counter_sees_the_callback_and_the_lane_hands_and_nobody_else() {
        let (_, c) = count_allocs(|| drop(vec![0u8; 4096]));
        assert_eq!(c.here, 2, "one allocation and one free");
        // Nothing in the window is nothing counted, hands included; through
        // `least_of`, because a neighbour test's pool shares the hands' names
        // and a single window can catch its thread starting or ending.
        let (_, quiet) = least_of(5, || ());
        assert_eq!(quiet.total(), 0, "{quiet:?}");

        let spawn = |name: &str| {
            std::thread::Builder::new()
                .name(name.to_owned())
                .spawn(|| {
                    // Inside the window, on the spawned thread.
                    let v = std::hint::black_box(vec![1u8; 64]);
                    drop(v);
                })
                .expect("a thread")
        };
        let (_, hand) = count_allocs(|| spawn("tangent-lane-9").join().expect("joined"));
        assert!(
            hand.hands >= 2,
            "a lane hand's allocation went unseen: {hand:?}"
        );
        let (_, other) = least_of(5, || spawn("someone-else").join().expect("joined"));
        assert_eq!(
            other.hands, 0,
            "a thread that is not a lane hand was counted: {other:?}"
        );
    }

    /// **No built-in effect goes to the allocator to process a block**:
    /// automation applied, the knobs gliding, mono and stereo, a block both
    /// smaller and larger than the planar adapter's scratch. Walked over the
    /// whole table so a unit added tomorrow is measured the day it lands;
    /// Tangent Bitcrush is the one this was written for, and it holds no
    /// scratch at all. A unit that keeps its delay while bypassed (the
    /// hardware insert, the tape) runs a block of its own out of circuit, so
    /// that block is measured too, with the way out and back in: the tape's
    /// fade, its stopped kernel and its restart from a clean tape.
    #[test]
    fn no_built_in_effect_allocates_to_process_a_block() {
        for spec in crate::builtin_fx::UNITS.iter().filter(|u| !u.voice) {
            let mut unit = spec.create(48_000.0, &spec.defaults());
            for channels in [2usize, 1] {
                for frames in [64usize, 1_500] {
                    let input: Vec<Vec<f32>> = (0..channels)
                        .map(|c| (0..frames).map(|i| ((i + c) as f32 * 0.01).sin() * 0.5).collect())
                        .collect();
                    let mut out = vec![vec![0.0f32; frames]; channels];
                    let points: Vec<tangent_host::ParamPoint> = spec
                        .params
                        .iter()
                        .map(|p| tangent_host::ParamPoint { offset: 0, id: p.id, value: 0.3 })
                        .collect();
                    // One warm block outside the window: a kernel that
                    // sizes something lazily on its first call is a
                    // load-time cost, not a per-block one.
                    unit.process(&input, frames, &mut out);
                    let ((), c) = least_of(3, || {
                        unit.apply(&points);
                        unit.process(&input, frames, &mut out);
                        if unit.keeps_its_delay_when_bypassed() {
                            unit.bypassed(&input, frames, &mut out);
                            unit.bypassed(&input, frames, &mut out);
                            unit.process(&input, frames, &mut out);
                        }
                    });
                    assert_eq!(
                        c,
                        Counted::default(),
                        "{} allocated processing {channels} channel(s) of {frames}",
                        spec.path
                    );
                }
            }
        }
    }

    /// The 32-lane desk the baseline renders: noise on every lane, an EQ in
    /// every first bay, the fork on so the hands do real work.
    fn baseline_desk() -> Engine {
        let mut e = offline();
        let clip = noise(4.0, 0x51ed);
        let len = clip.frames() as u64;
        e.set_clips(
            (0..LEGACY_LANES)
                .map(|lane| Placed {
                    clip: Arc::clone(&clip),
                    strip: lane,
                    start: 0,
                    offset: 0,
                    len,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 0.5,
                })
                .collect(),
        );
        for lane in 0..LEGACY_LANES {
            e.load_insert(lane, 0, Some(Path::new(crate::builtin_fx::EQ_PATH)), None)
                .expect("the EQ loads")
                .expect("the bay reports it");
        }
        e.set_offline_fanout(true);
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }

    fn monitor_ring() -> rtrb::Consumer<f32> {
        let (mut tx, rx) = rtrb::RingBuffer::<f32>::new(4096);
        for i in 0..2048 {
            let _ = tx.push((i as f32 * 0.01).sin() * 0.1);
        }
        rx
    }

    /// **The allocation baseline of today's callback, at 32 lanes.**
    ///
    /// A steady render allocates nothing, on the callback or on the hands. A
    /// monitor ring arriving into an empty seat allocates nothing, and nor
    /// does a layout arriving for it. There was one KNOWN site, a second
    /// ring arriving: the `pending_monitor` handoff assigned
    /// `self.monitor = Some(ring)` and so freed the old `Consumer` on the
    /// audio thread (two frees). W3-slot-engine removed it: the ring and its
    /// layout arrive as one Monitor op, the ring being let go of is swapped
    /// into that plan and leaves with it on the retire ring, and this test
    /// pins zero.
    ///
    /// Asserted as "never grows": each window is held to the figure recorded
    /// here, so a new allocation anywhere in the path fails it.
    #[test]
    fn the_callback_allocates_only_where_it_is_known_to() {
        /// The KNOWN list is empty (design W3, done when).
        const KNOWN_RING_SWAP: usize = 0;
        let mut e = baseline_desk();
        let mut out = vec![0.0f32; 512 * 2];
        // The hand-off rings drain, the delay lines fill, the pool starts.
        for _ in 0..16 {
            e.render_offline(&mut out);
        }
        let (_, steady) = least_of(5, || {
            for _ in 0..8 {
                e.render_offline(&mut out);
            }
        });
        assert_eq!(
            steady,
            Counted::default(),
            "a steady 32-lane render allocated"
        );
        assert!(
            out.iter().any(|s| *s != 0.0),
            "the baseline desk rendered silence"
        );

        // A ring arriving where there was none.
        e.set_monitor_on(true);
        let picks = tangent_record::audio::Picks::EVERYTHING;
        e.set_monitor(Some((monitor_ring(), 2, vec![2; 32].into_boxed_slice(), 512, picks.clone())));
        // One-shot windows (each event happens once), so only the callback's
        // own figure is held exactly: the hands' figure is name-keyed and a
        // neighbour's pool could add to a single window. The hands are held
        // to zero by the repeated windows either side.
        let (_, first) = count_allocs(|| e.render_offline(&mut out));
        assert_eq!(
            first.here, 0,
            "a first monitor ring arriving allocated: {first:?}"
        );
        // A layout arriving for the ring already held.
        e.set_monitor_layout(&[1; 32], picks.clone());
        let (_, layout) = count_allocs(|| e.render_offline(&mut out));
        assert_eq!(
            layout.here, 0,
            "a monitor layout arriving allocated: {layout:?}"
        );
        // A second ring displacing the first: no longer a free here.
        e.set_monitor(Some((monitor_ring(), 2, vec![2; 32].into_boxed_slice(), 512, picks)));
        let (_, swap) = count_allocs(|| e.render_offline(&mut out));
        assert_eq!(
            swap.here, KNOWN_RING_SWAP,
            "a monitor ring swap allocated or freed on the callback: {swap:?}"
        );
        // And then quiet again.
        let (_, after) = least_of(5, || {
            for _ in 0..4 {
                e.render_offline(&mut out);
            }
        });
        assert_eq!(
            after,
            Counted::default(),
            "the render after a ring swap allocated"
        );
    }

    /// **Several instruments on one track allocate nothing either** (see
    /// `instrument::Layer`): a DX7 at the head, an analog layered over it,
    /// the keyboard entering at the third bay, a sampler in the extra rack,
    /// and a kit with an analog beside it, every one of them playing a note
    /// on the fork. Their alignment lines, the keyboard's inlet and the
    /// kit's pour buffers were all built on the host; the callback only
    /// steps through them, and wakes a rested line with a fill.
    ///
    /// **What this can and cannot catch.** The engine before layering
    /// allocated nothing on this desk either: it simply played one
    /// instrument a track. So this is not a regression test against that
    /// engine; it pins the NEW roads (`layer_in`, `pour_inlets`,
    /// `pour_kits`, `DelayRing::wake`) to zero, and fails the day one of
    /// them builds a line or a buffer lazily on the callback. That they are
    /// the roads this desk takes is proven on the same shapes by the
    /// `slot_engine::layers` tests (the sums only fit through them).
    #[test]
    fn layered_instruments_allocate_nothing_on_the_callback() {
        use crate::desk_plan::Slot;
        use crate::instrument::{ClipNote, PadKit, PlacedMidi, SamplerSample, SAMPLER_ROOT_KEY};
        const LANE: usize = 3;
        const KIT: usize = 6;
        let mut e = offline();
        e.set_builtin_strips(1 << LANE);
        e.set_keys_strips(1 << LANE);
        e.sync_instrument_lanes(1 << LANE, 1 << LANE);
        e.set_inlets(Slot(LANE as u32), 0, 2);
        let seat = |e: &mut Engine, seat: usize, bay: usize, unit: &str| {
            e.load_insert(seat, bay, Some(Path::new(unit)), None)
                .expect("the unit loads")
                .expect("the bay reports it");
        };
        seat(&mut e, LANE, 1, crate::builtin_fx::ANALOG_PATH);
        seat(&mut e, tangent_ui::desk_legacy::folder_extra_seat(LANE), 0, crate::builtin_fx::SAMPLER_PATH);
        assert!(e.set_sampler_sample(
            Slot(LANE as u32),
            Some(SamplerSample {
                clip: noise(0.5, 0x5a5c),
                name: "alloc.wav".to_owned(),
                source: std::path::PathBuf::from("alloc.wav"),
                root_key: SAMPLER_ROOT_KEY,
            })
        ));
        let mut kit = PadKit::default();
        kit.pads[0] = Some(noise(0.1, 0xd00e));
        assert!(e.set_pad_kit(KIT, kit));
        e.set_drum_strips(1 << KIT);
        seat(&mut e, KIT, 1, crate::builtin_fx::ANALOG_PATH);
        let notes = |strip: usize, key: u8| PlacedMidi {
            strip,
            start: 0,
            len: 48_000 * 4,
            notes: Arc::new((0..16).map(|i| ClipNote { at: i * 6_000, dur: 4_000, key, vel: 100 }).collect()),
            pedal: Arc::new(Vec::new()),
        };
        e.set_midi_clips(vec![notes(LANE, SAMPLER_ROOT_KEY), notes(KIT, tangent_ui::recorder::DRUM_PAD_BASE)]);
        e.set_offline_fanout(true);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 512 * 2];
        for _ in 0..16 {
            e.render_offline(&mut out);
        }
        let (_, steady) = least_of(5, || {
            for _ in 0..8 {
                e.render_offline(&mut out);
            }
        });
        assert_eq!(steady, Counted::default(), "a layered desk allocated on the callback");
        assert!(out.iter().any(|s| *s != 0.0), "the layered desk rendered silence");
    }

    /// One render of the alias desk: two lanes playing two different
    /// signals, a loud trim on each (so freezing a lane audibly changes it),
    /// stems on, and `switch` applied to the engine before the roll. Returns
    /// each lane's stem, interleaved stereo.
    fn alias_render(a: usize, b: usize, switch: &dyn Fn(&Engine, usize)) -> (Vec<f32>, Vec<f32>) {
        let mut e = offline();
        let mut clips = Vec::new();
        for (lane, seed) in [(a, 0x1111u32), (b, 0x2222)] {
            let clip = noise(0.5, seed);
            let len = clip.frames() as u64;
            clips.push(Placed {
                clip,
                strip: lane,
                start: 0,
                offset: 0,
                len,
                fade_in: 0,
                fade_out: 0,
                gain: 0.5,
            });
            let spec = crate::builtin_fx::spec_for(crate::builtin_fx::TRIM_PATH).expect("trim");
            let loud = crate::builtin_fx::state_text(spec, &[0.8], &[]).into_bytes();
            e.load_insert(
                lane,
                0,
                Some(Path::new(crate::builtin_fx::TRIM_PATH)),
                Some(&loud),
            )
            .expect("the trim loads")
            .expect("the bay reports it");
        }
        e.set_clips(clips);
        e.set_stems(true);
        switch(&e, a);
        e.set_transport(1, 0);
        e.set_rolling(true);
        let mut out = vec![0.0f32; 512 * 2];
        let mut stems = Vec::new();
        for _ in 0..8 {
            e.render_offline(&mut out);
            e.drain_stems(&mut stems);
        }
        let wide = LEGACY_LANES * 2;
        let lane = |l: usize| -> Vec<f32> {
            stems
                .chunks_exact(wide)
                .flat_map(|f| [f[l * 2], f[l * 2 + 1]])
                .collect()
        };
        (lane(a), lane(b))
    }

    /// **A switch thrown on one lane moves that lane and no other**, for
    /// every pair of the lanes a 32-wide mask is most likely to confuse: the
    /// first two, and the last two, where `1 << n` and `.min(31)` live.
    /// Through today's public setters only, so it holds the engine as it is,
    /// and W2/W3 must keep it green as the masks become sets.
    #[test]
    fn a_switch_on_one_lane_reaches_no_other_lane() {
        type Switch = (&'static str, fn(&Engine, usize));
        let switches: [Switch; 5] = [
            ("mute", |e, l| {
                let mut d = crate::render_golden::quiet_desk();
                d.muted[l] = true;
                e.set_desk(&d);
            }),
            ("phase", |e, l| e.set_strip_phase(1u64 << l)),
            ("frozen", |e, l| e.set_frozen(1u32 << l)),
            ("gain", |e, l| e.set_channel_gain(l, 0.25)),
            ("pan", |e, l| e.set_channel_pan(l, -1.0)),
        ];
        let lanes = [0usize, 1, 30, 31];
        for &a in &lanes {
            for &b in &lanes {
                if a == b {
                    continue;
                }
                let (a_ref, b_ref) = alias_render(a, b, &|e, _| {
                    e.set_desk(&crate::render_golden::quiet_desk());
                });
                assert!(
                    a_ref.iter().any(|v| *v != 0.0),
                    "lane {a}'s stem was silent"
                );
                assert!(
                    b_ref.iter().any(|v| *v != 0.0),
                    "lane {b}'s stem was silent"
                );
                for (name, switch) in switches {
                    let (a_on, b_on) = alias_render(a, b, &|e, l| {
                        e.set_desk(&crate::render_golden::quiet_desk());
                        switch(e, l);
                    });
                    assert!(a_on != a_ref, "{name} on lane {a} did not change lane {a}");
                    assert!(
                        b_on.iter()
                            .zip(&b_ref)
                            .all(|(x, y)| x.to_bits() == y.to_bits()),
                        "{name} on lane {a} changed lane {b}"
                    );
                }
            }
        }
    }
}
