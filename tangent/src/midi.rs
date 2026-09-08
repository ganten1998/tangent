//! MIDI input: midir callback thread -> mpsc channel (spec §10), plus the
//! recorder's raw tap.
//!
//! - Auto-connect priority: "USB-MIDI" -> "Scarlett" OR ("USB" AND "MIDI") -> first.
//! - note_on vel>0 = on; note_off or note_on vel==0 = off; CC64 >= 64 = sustain down.
//! - Channels merged (status high nibble only). Everything else ignored.
//! - No reconnect logic (parity): if the port dies, events just stop.
//!
//! # Two consumers, one callback
//!
//! The display path above is deliberately lossy and must stay that way — its
//! three-variant alphabet is the chord engine's contract. The recorder needs the
//! opposite: channel, release velocity, continuous CC64, pitch bend, both
//! aftertouches, program change and SysEx, each with the timestamp midir has
//! always supplied and this file used to discard as `_stamp`.
//!
//! So the callback feeds both, and the raw side is teed **before**
//! `parse_message` runs rather than reconstructed after it.

/// **FreeBSD reads its own MIDI, and everything below this line is unchanged.**
///
/// midir 0.11 has no FreeBSD backend — the crate does not compile there — and
/// its only OS-agnostic arm is JACK, which would mean no keyboard works until
/// the user installs and starts a JACK server. `crate::umidi` opens
/// `/dev/umidi*` directly and offers the same five calls this file makes, so
/// the whole platform difference is this alias: `midir::MidiInput`,
/// `midir::Ignore` and `midir::MidiInputConnection` below resolve to it.
///
/// A local `use` shadows the extern crate name inside this module only, which
/// is exactly the blast radius wanted — nothing else in the app can reach the
/// FreeBSD backend by accident.
#[cfg(target_os = "freebsd")]
use crate::umidi as midir;

use tangent_ui::midi_event::parse_message;
pub use tangent_ui::midi_event::MidiEvent;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::time::Instant;

/// Raw timestamped MIDI, for the recorder.
///
/// # Why this is always on
///
/// midir takes its callback **by value** (`F: FnMut(u64, &[u8], &mut T) + Send +
/// 'static`) and `MidiInputConnection` exposes exactly one method, `close(self)
/// -> (MidiInput, T)`. There is no accessor for the closure or its data while
/// the connection is open, and on CoreMIDI both are sealed inside a private
/// `Arc<Mutex<HandlerData<T>>>`. So an `Option<Tap>` captured by a `move`
/// closure is **fixed for the life of the connection** and could never be
/// switched on when Record is pressed. Arming has to happen on the far side.
///
/// That constraint turns out to be what the file needs anyway. A `.mid` must
/// restate, at tick 0, the program and pedal state that was already true when
/// Record was pressed — and a tap armed at Record has by definition never seen
/// the messages that set it. See `docs/RECORDER-PLAN.md` §7 rule 8.
///
/// # Why a `Mutex` and not a lock-free ring
///
/// This is not an audio callback. It is midir's own thread, delivering a handful
/// of three-byte messages per second, and the only contender for the lock is a
/// drain on the UI thread. A lock-free SPSC ring buys nothing here and would
/// need a second allocation strategy for variable-length SysEx. The audio path
/// in step 4 is a different matter and does use one.
/// # Why each message carries an arrival `Instant` as well as its device stamp
///
/// The device stamp is what PLACES the event — see the comment in
/// `connect_by_name` for why arrival time must never be used for that. But
/// mapping the device's clock onto the host's needs *pairs*:
/// `SourceClock::observe(device_stamp, host_time)` anchors on the smallest
/// delay it has seen, and the reading has to be taken **at the same moment** as
/// the stamp for that minimum to mean anything. Taken later, on the UI thread
/// at drain time, it carries up to a frame of scheduling delay — and since the
/// anchor is a running minimum, that delay is exactly what it would converge to.
///
/// `Instant` rather than the recorder's `Nanos`, deliberately: `push` is
/// compiled into a Minimal build, where `tangent-record` is not linked at all and
/// `Timebase` does not exist. `std::time::Instant` always does, and the session
/// converts on the way out.
#[derive(Debug)]
pub struct RawMidiTap {
    queue: Mutex<VecDeque<(u64, Instant, Vec<u8>)>>,
    capacity: usize,
    dropped: AtomicU64,
}

impl RawMidiTap {
    /// `capacity` messages are retained; older ones are shed. A dense piano take
    /// is a few thousand messages a minute, so a few tens of thousands is
    /// minutes of headroom against a UI that has stopped draining.
    pub fn new(capacity: usize) -> Self {
        Self {
            queue: Mutex::new(VecDeque::with_capacity(capacity.min(4096))),
            capacity,
            dropped: AtomicU64::new(0),
        }
    }

    /// Called from the midir callback thread. Never blocks for long and never
    /// fails: a poisoned lock is recovered from rather than propagated, because
    /// losing MIDI is worse than losing whatever invariant a panicking drainer
    /// broke.
    fn push(&self, stamp_us: u64, bytes: &[u8]) {
        // BEFORE the lock. Whatever this costs, it must not include however
        // long another thread held the queue — that would put the drain's
        // contention into the clock pairing this reading exists to make.
        let arrived = Instant::now();
        let mut q = match self.queue.lock() {
            Ok(q) => q,
            Err(poisoned) => poisoned.into_inner(),
        };
        // Shed the OLDEST, not the newest. A tap that stops accepting new
        // messages when it fills would silently record the first minute of a
        // take and nothing after it.
        while q.len() >= self.capacity {
            q.pop_front();
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
        q.push_back((stamp_us, arrived, bytes.to_vec()));
    }

    /// Take everything captured so far.
    ///
    /// Gated on `recorder` along with `dropped`, because these two are the
    /// recorder's readers and nothing else calls them. `push` stays
    /// unconditional so `connect_by_name` keeps ONE signature across both build
    /// variants — a Minimal build fills a small ring that nobody drains, which
    /// costs a few hundred bytes and is worth more than a second code path
    /// through the MIDI callback.
    #[cfg(feature = "recorder")]
    pub fn drain(&self) -> Vec<(u64, Instant, Vec<u8>)> {
        let mut q = match self.queue.lock() {
            Ok(q) => q,
            Err(poisoned) => poisoned.into_inner(),
        };
        q.drain(..).collect()
    }

    /// How many messages were shed because nothing drained in time. Belongs in
    /// the take's report: a non-zero value means the `.mid` is incomplete, and
    /// silently shipping an incomplete file is worse than saying so.
    #[cfg(feature = "recorder")]
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }
}

/// The HARDWARE FAST LANE: the midir callback's direct line to the audio
/// callback, with no UI frame in between.
///
/// For every release before this one, a hardware note took the scenic route:
/// midir thread -> [`RawMidiTap`] -> a once-per-UI-frame drain at the END of
/// the egui pass -> `Engine::send_midi` -> the audio callback. Typical cost
/// 3-10 ms, worst a whole repaint interval — stacked on top of the audio
/// buffer, and the reason a keyboard felt slower than the audio round trip.
/// The engine's own module docs named this defect and this fix.
///
/// The lane is created ONCE PER APP (like the tap, and for the same reason:
/// midir seals its callback's captured state when the port opens, so the
/// closure must hold something app-lifetime it can look through). Each engine
/// seats its ring's write end here at start via [`MidiFastLane::install`];
/// a dead engine's producer just goes nowhere until the next one replaces it.
///
/// Events are pushed with **stamp 0 — the far past — on purpose**: `place()`
/// puts a past stamp at the block's first frame, which for live input is the
/// soonest anything can sound. A device stamp converted naively could land in
/// the future and be HELD for a block; the recording path still gets real
/// stamps from the tap, which is unchanged.
///
/// The pads' channel-10 mirror lives here too (it used to ride the UI drain):
/// while the drumpad is up with its MIDI chip lit, a note inside the pads'
/// sixteen also strikes its pad, as a copy on channel 10. A note already on
/// channel 10 reached the pads with the original push and is not doubled.
pub struct MidiFastLane {
    /// A `Mutex`, not a lock-free cell, for the tap's own reason: this is
    /// midir's thread, not an audio callback, and the only contention is the
    /// desktop seating a new producer at engine start. `try_lock` on the push
    /// side so a mid-install collision drops one event instead of blocking.
    #[cfg(feature = "recorder")]
    slot: Mutex<Option<rtrb::Producer<crate::instrument::MidiEvent>>>,
    /// Whether the drumpad is up with MIDI live — pushed by the desktop each
    /// frame. Frame-stale by nature; so was the UI-drain mirror it replaces.
    pads_live: std::sync::atomic::AtomicBool,
}

impl MidiFastLane {
    pub fn new() -> Self {
        Self {
            #[cfg(feature = "recorder")]
            slot: Mutex::new(None),
            pads_live: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// Seat a fresh engine's ring. The old producer is dropped here, on the
    /// UI thread, never in a callback.
    #[cfg(feature = "recorder")]
    pub fn install(&self, tx: rtrb::Producer<crate::instrument::MidiEvent>) {
        if let Ok(mut g) = self.slot.lock() {
            *g = Some(tx);
        }
    }

    pub fn set_pads_live(&self, live: bool) {
        self.pads_live.store(live, Ordering::Relaxed);
    }

    /// From the midir callback thread: sound this message NOW.
    ///
    /// Channel messages only — SysEx and realtime have nothing to sound and
    /// would bloat the ring. A full ring or a mid-install collision sheds the
    /// event silently: the tap still has it, so the RECORD is whole either
    /// way, and a shed here only costs the monitor one note.
    pub fn play(&self, bytes: &[u8]) {
        #[cfg(feature = "recorder")]
        {
            let Some(&status) = bytes.first() else {
                return;
            };
            if status >= 0xF0 {
                return;
            }
            let event = crate::instrument::MidiEvent {
                stamp: 0,
                status,
                data1: bytes.get(1).copied().unwrap_or(0),
                data2: bytes.get(2).copied().unwrap_or(0),
            };
            let Ok(mut g) = self.slot.try_lock() else {
                return;
            };
            let Some(tx) = g.as_mut() else {
                return;
            };
            let _ = tx.push(event);
            // The pads' mirror. Bounds and channel checks exactly as the UI
            // drain did them; DRUM_PAD_BASE..+DRUM_PADS is keys 36..52.
            let kind = status & 0xF0;
            let base = tangent_ui::recorder::DRUM_PAD_BASE;
            let top = base + tangent_ui::recorder::DRUM_PADS as u8;
            if self.pads_live.load(Ordering::Relaxed)
                && (kind == 0x90 || kind == 0x80)
                && status & 0x0F != 9
                && (base..top).contains(&(event.data1 & 0x7F))
            {
                let _ = tx.push(crate::instrument::MidiEvent {
                    stamp: 0,
                    status: kind | 9,
                    data1: event.data1,
                    data2: event.data2,
                });
            }
        }
        #[cfg(not(feature = "recorder"))]
        let _ = bytes;
    }
}

impl Default for MidiFastLane {
    fn default() -> Self {
        Self::new()
    }
}

/// Holds the open connection; dropping it closes the port.
pub struct MidiConnection {
    _conn: midir::MidiInputConnection<()>,
    pub port_name: String,
}

fn new_input(client_name: &str) -> Option<midir::MidiInput> {
    midir::MidiInput::new(client_name).ok()
}

/// A port opened for listening, with the noise filtered at the source.
///
/// **midir's default is to ignore nothing** (`Ignore::None`, `common.rs:68-69`)
/// and this file never called `.ignore()`. That was invisible while the only
/// consumer was `parse_message`, which returns `None` for a heartbeat — but the
/// recorder keeps everything it is given, and many keyboards send Active Sensing
/// (`0xFE`) every ~300 ms forever. Left in, a twenty-minute take carries roughly
/// four thousand meaningless messages and the file grows about tenfold.
///
/// `TimeAndActiveSense` drops MIDI Clock and Active Sensing and **keeps SysEx**,
/// which the recorder does want. Filtering here rather than in the tap means the
/// bytes are never allocated.
fn listening_input(client_name: &str) -> Option<midir::MidiInput> {
    let mut input = new_input(client_name)?;
    input.ignore(midir::Ignore::TimeAndActiveSense);
    Some(input)
}

/// Names of all available MIDI input ports.
pub fn list_port_names() -> Vec<String> {
    let Some(input) = new_input("ivory-scan") else {
        return Vec::new();
    };
    input
        .ports()
        .iter()
        .filter_map(|p| input.port_name(p).ok())
        .collect()
}

/// Print the `--list` output with the parity strings (spec §2.1).
pub fn print_port_list() {
    let names = list_port_names();
    println!("Available MIDI Input Ports:");
    if names.is_empty() {
        println!("  No MIDI input ports found!");
    } else {
        for (i, name) in names.iter().enumerate() {
            println!("  {i}: {name}");
        }
    }
}

/// Open the port with this exact name. The egui context is woken on every event
/// so repaints are event-driven rather than busy-looped (D-UI-3).
///
/// `tap` is cloned into the callback and receives every message with its device
/// timestamp, always. Switching ports drops this connection and builds a new
/// closure holding a clone of the **same** `Arc`, so a port switch mid-session
/// keeps both the pre-roll and the controller-state history.
pub fn connect_by_name(
    name: &str,
    tx: mpsc::Sender<MidiEvent>,
    ctx: egui::Context,
    tap: Arc<RawMidiTap>,
    lane: Arc<MidiFastLane>,
) -> Result<MidiConnection, String> {
    let input = listening_input("ivory").ok_or_else(|| "MIDI system unavailable".to_string())?;
    let port = input
        .ports()
        .into_iter()
        .find(|p| input.port_name(p).as_deref() == Ok(name))
        .ok_or_else(|| format!("no port named '{name}'"))?;
    let port_name = name.to_owned();
    let conn = input
        .connect(
            &port,
            "ivory-in",
            move |stamp_us, message, _| {
                // The raw tee comes FIRST and unconditionally. `stamp_us` is
                // MICROSECONDS on every midir backend — the CoreMIDI one is
                // literally `AudioConvertHostTimeToNanos(t) / 1000` — so it is
                // handed on unscaled and `tangent_record::clock::SourceClock`
                // applies MIDIR_SCALE_NS. Scaling here would put the unit in two
                // places and they would disagree.
                //
                // Use the DEVICE stamp, never `Instant::now()`. `now()` in a
                // callback is arrival time, which carries OS delivery latency
                // and its jitter, and on ALSA the sequencer queue's own
                // scheduling on top.
                // SOUND before bookkeeping: the fast lane is why the note is
                // audible this block instead of after the next UI frame.
                lane.play(message);
                tap.push(stamp_us, message);
                if let Some(event) = parse_message(message) {
                    let _ = tx.send(event);
                    ctx.request_repaint_of(egui::ViewportId::ROOT);
                }
            },
            (),
        )
        .map_err(|e| e.to_string())?;
    Ok(MidiConnection {
        _conn: conn,
        port_name,
    })
}

/// Startup auto-connect (spec §10 priority chain). Returns None when there are
/// no ports or opening fails — the app runs without MIDI, silently.
pub fn auto_connect(
    tx: mpsc::Sender<MidiEvent>,
    ctx: egui::Context,
    tap: Arc<RawMidiTap>,
    lane: Arc<MidiFastLane>,
) -> Option<MidiConnection> {
    let names = list_port_names();
    let chosen = pick_auto_port(&names)?;
    connect_by_name(&chosen, tx, ctx, tap, lane).ok()
}

/// Developer probe: open a port and print the raw tap until interrupted.
///
/// Gated on `recorder`: it reads `tangent_record::clock`, which a Minimal build
/// does not link. The tap itself is NOT gated — it is a few hundred bytes and
/// keeping it unconditional means `connect_by_name` has one signature rather
/// than two, which is worth more than the bytes.
///
/// The unit tests prove the arithmetic; only a real keyboard proves the wire.
/// What this is for, specifically — each of these is a claim in
/// `docs/RECORDER-PLAN.md` that deserves checking against hardware rather than
/// against a document:
///
/// * the stamp really is **microseconds** and really is monotonic
/// * the pedal really does send a continuous sweep rather than 0 and 127
/// * note-offs really do carry release velocity on this instrument
/// * Active Sensing really is filtered before it reaches the tap
/// * the anchor really does converge (watch `t=` settle against `+`)
///
/// `t=` is the device stamp converted into the timebase and expressed relative
/// to the first message; `+` is the raw delta from the previous message.
#[cfg(feature = "recorder")]
pub fn dump_raw(port: Option<String>) {
    let names = list_port_names();
    let Some(chosen) = port.or_else(|| pick_auto_port(&names)) else {
        eprintln!("no MIDI input ports found");
        return;
    };

    let tap = Arc::new(RawMidiTap::new(100_000));
    let (tx, _rx) = mpsc::channel();
    let lane = Arc::new(MidiFastLane::new());
    let conn = match connect_by_name(
        &chosen,
        tx,
        egui::Context::default(),
        Arc::clone(&tap),
        lane,
    ) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("could not open {chosen:?}: {e}");
            return;
        }
    };

    println!(
        "listening on {:?} - play something; Ctrl-C to stop",
        conn.port_name
    );
    println!(
        "{:>12}  {:>10}  {:>9}  bytes",
        "t (ms)", "delta (ms)", "stamp"
    );

    let epoch = Instant::now();
    let mut clock =
        tangent_record::clock::SourceClock::new(tangent_record::clock::MIDIR_SCALE_NS, 2_000_000_000);
    let mut first: Option<i64> = None;
    let mut prev_stamp: Option<u64> = None;

    loop {
        for (stamp, arrived, bytes) in tap.drain() {
            // The tap's own arrival reading, not `epoch.elapsed()` here. This
            // loop runs on a sleep, so a reading taken now would be up to a
            // whole poll interval late — and `SourceClock` anchors on the
            // SMALLEST delay it sees, so a uniformly late reading is exactly
            // the thing it cannot see through.
            let arrived_ns = arrived.duration_since(epoch).as_nanos() as i64;
            clock.observe(stamp, arrived_ns);
            let t = clock.to_timebase(stamp).unwrap_or(arrived_ns);
            let base = *first.get_or_insert(t);
            let delta_ms = prev_stamp.map_or(0.0, |p| (stamp - p) as f64 / 1_000.0);
            prev_stamp = Some(stamp);
            println!(
                "{:>12.3}  {:>10.3}  {:>9}  {}",
                (t - base) as f64 / 1e6,
                delta_ms,
                stamp,
                bytes
                    .iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            );
        }
        if tap.dropped() > 0 {
            eprintln!("WARNING: shed {} messages", tap.dropped());
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// The parity priority chain, split out for testing.
pub fn pick_auto_port(names: &[String]) -> Option<String> {
    if names.is_empty() {
        return None;
    }
    if let Some(n) = names.iter().find(|n| n.contains("USB-MIDI")) {
        return Some(n.clone());
    }
    if let Some(n) = names
        .iter()
        .find(|n| n.contains("Scarlett") || (n.contains("USB") && n.contains("MIDI")))
    {
        return Some(n.clone());
    }
    names.first().cloned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(names: &[&str]) -> Vec<String> {
        names.iter().map(|s| s.to_string()).collect()
    }

    /// The tap has to be reachable from a callback thread and drained from the
    /// UI thread, so it must be `Send + Sync` behind an `Arc`. Stated as a test
    /// because the failure otherwise appears as a wall of trait errors at the
    /// `connect` call site, which reads as a midir problem.
    #[test]
    fn the_tap_can_cross_threads() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Arc<RawMidiTap>>();
    }

    #[cfg(feature = "recorder")]
    #[test]
    fn the_tap_keeps_everything_the_display_path_throws_away() {
        let tap = RawMidiTap::new(16);
        // Pitch bend, poly aftertouch and a non-64 CC all parse to None on the
        // display path (`midi_event.rs:36`), and CC64 is collapsed to a bool at
        // 64. The recorder must see all of it, verbatim, with the timestamp.
        tap.push(10, &[0xE5, 0x00, 0x40]);
        tap.push(20, &[0xA0, 60, 70]);
        tap.push(30, &[0xB3, 1, 64]);
        tap.push(40, &[0xB0, 64, 63]);
        tap.push(50, &[0x83, 60, 33]);

        let got: Vec<(u64, Vec<u8>)> = tap
            .drain()
            .into_iter()
            .map(|(stamp, _arrived, bytes)| (stamp, bytes))
            .collect();
        assert_eq!(
            got,
            vec![
                (10, vec![0xE5, 0x00, 0x40]),
                (20, vec![0xA0, 60, 70]),
                (30, vec![0xB3, 1, 64]),
                (40, vec![0xB0, 64, 63]),
                (50, vec![0x83, 60, 33]),
            ]
        );
        assert!(tap.drain().is_empty(), "draining twice must not repeat");
    }

    #[cfg(feature = "recorder")]
    #[test]
    fn a_full_tap_sheds_the_oldest_and_says_how_many() {
        let tap = RawMidiTap::new(3);
        for i in 0..6u64 {
            tap.push(i, &[0x90, 60, 100]);
        }
        let got = tap.drain();
        assert_eq!(
            got.iter().map(|(t, ..)| *t).collect::<Vec<_>>(),
            vec![3, 4, 5],
            "a tap that shed the NEWEST would record the start of a take and \
             silently nothing after it"
        );
        assert_eq!(tap.dropped(), 3, "the loss has to be reportable");
    }

    #[cfg(feature = "recorder")]
    #[test]
    fn a_fresh_tap_has_dropped_nothing() {
        let tap = RawMidiTap::new(4);
        tap.push(1, &[0x90, 60, 1]);
        assert_eq!(tap.dropped(), 0);
        assert_eq!(tap.drain().len(), 1);
    }

    /// **The fast lane delivers, mirrors the pads, and never panics bare.**
    ///
    /// The seated ring gets every channel message; with the drumpad live, a
    /// pad-range note also arrives as its channel-10 copy (but a note already
    /// on channel 10 is not doubled); realtime/SysEx stay out; and playing
    /// into a lane no engine has seated is a quiet no-op.
    #[cfg(feature = "recorder")]
    #[test]
    fn the_fast_lane_delivers_and_mirrors_the_pads() {
        let lane = MidiFastLane::new();
        // Bare: no producer seated. Must simply do nothing.
        lane.play(&[0x90, 60, 100]);

        let (tx, mut rx) = rtrb::RingBuffer::<crate::instrument::MidiEvent>::new(16);
        lane.install(tx);

        // A plain note, pads down: one event, stamp 0 (the far past).
        lane.play(&[0x90, 60, 100]);
        let e = rx.pop().expect("the note was not delivered");
        assert_eq!((e.stamp, e.status, e.data1, e.data2), (0, 0x90, 60, 100));
        assert!(rx.pop().is_err(), "a note was doubled with pads down");

        // Realtime never sounds.
        lane.play(&[0xF8]);
        assert!(rx.pop().is_err(), "a realtime byte reached the ring");

        // Pads live: a note inside 36..52 arrives twice — itself, then its
        // channel-10 copy.
        lane.set_pads_live(true);
        lane.play(&[0x90, 40, 90]);
        let a = rx.pop().expect("the original vanished");
        let b = rx.pop().expect("the pad mirror vanished");
        assert_eq!(a.status, 0x90);
        assert_eq!(b.status, 0x99, "the mirror is not on channel 10");
        assert_eq!((b.data1, b.data2), (40, 90));

        // Already channel 10: no double.
        lane.play(&[0x99, 40, 90]);
        assert!(rx.pop().is_ok());
        assert!(
            rx.pop().is_err(),
            "a channel-10 note was mirrored onto itself"
        );

        // EVERY key is a pad now: high notes mirror too.
        lane.play(&[0x90, 72, 90]);
        assert!(rx.pop().is_ok());
        assert!(
            rx.pop().is_ok_and(|e| e.status == 0x99),
            "a high note stopped mirroring - the pads cover the keyboard now"
        );
    }

    #[test]
    fn auto_connect_priority_chain() {
        assert_eq!(pick_auto_port(&v(&[])), None);
        assert_eq!(
            pick_auto_port(&v(&["Foo", "USB-MIDI 1", "Scarlett 2i2"])),
            Some("USB-MIDI 1".into())
        );
        assert_eq!(
            pick_auto_port(&v(&["Foo", "Scarlett 2i2", "Bar USB MIDI"])),
            Some("Scarlett 2i2".into())
        );
        assert_eq!(
            pick_auto_port(&v(&["Foo", "Bar USB MIDI thing"])),
            Some("Bar USB MIDI thing".into())
        );
        assert_eq!(pick_auto_port(&v(&["Alpha", "Beta"])), Some("Alpha".into()));
    }
}
