//! **What one block of the audio callback costs, measured without a device.**
//!
//! Real-time audio is deadline bound, not average bound: at 48 kHz a 128
//! frame block has 2.667 ms of wall clock to be finished in, every time, and
//! the block that misses is the one the room hears. An average CPU figure
//! hides exactly that, so everything here is reported as a distribution — min,
//! mean, p99 and max microseconds per `Renderer::render` call — against the
//! deadline for the block size being rendered.
//!
//! It runs on [`Engine::offline`], which is the same [`crate::instrument::Renderer`]
//! the CoreAudio callback drives, pulled by hand instead of by the device. So
//! the numbers are the engine's own arithmetic: the same `run_rack`, the same
//! folds, the same master chain, the same taps. What they are NOT is the
//! scheduling: this runs on an ordinary test thread at ordinary priority, not
//! on a time-constraint audio thread pinned to a performance core, so treat
//! the max as pessimistic and the mean and p99 as the honest figures.
//!
//! Run it:
//!
//! ```text
//! cargo test --release -p tangent --bin tangent -- rt_bench --ignored --nocapture
//! ```

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Instant;

use tangent_record::audio::Timebase;
use tangent_ui::recorder::{Desk, STRIPS};

use crate::builtin_fx::{ANALOG_PATH, COMP_PATH, DENOISE_PATH, DRIVE_PATH, EQ_PATH, PLATE_PATH, TAPE_PATH};
use crate::instrument::{ClipNote, Engine, Placed, PlacedMidi};

/// The block every shape is measured at: the middle of the three sizes the
/// article names, and the one REAPER is configured for on this machine.
const BLOCK: usize = 128;
const RATE: u32 = 48_000;

/// The wall clock one block has, in microseconds. 128 / 48000 = 2.667 ms.
const DEADLINE_US: f64 = BLOCK as f64 * 1.0e6 / RATE as f64;

/// The deadline for any block size, in microseconds.
fn deadline_us(block: usize) -> f64 {
    block as f64 * 1.0e6 / f64::from(RATE)
}

/// Blocks timed per shape. 2000 blocks is 5.3 seconds of audio, which is long
/// enough for a p99 to mean something and short enough that ten shapes fit in
/// a coffee break.
const MEASURED: usize = 2000;

/// Blocks rendered and thrown away first: the insert hand-off rings drain on
/// the first render, the PDC delay lines fill, the fader smoothers settle and
/// the caches warm.
const WARMUP: usize = 400;

/// One shape's distribution, all in microseconds per block.
struct Stats {
    name: String,
    min: f64,
    mean: f64,
    p50: f64,
    p99: f64,
    max: f64,
    /// The deadline this shape was measured against.
    deadline: f64,
    /// Blocks that took longer than the deadline.
    over: usize,
}

impl Stats {
    fn from(name: impl Into<String>, block: usize, mut us: Vec<f64>) -> Self {
        let deadline = deadline_us(block);
        let over = us.iter().filter(|v| **v > deadline).count();
        let mean = us.iter().sum::<f64>() / us.len() as f64;
        us.sort_by(|a, b| a.partial_cmp(b).expect("no NaN from a duration"));
        let at = |q: f64| us[((us.len() - 1) as f64 * q) as usize];
        Self {
            name: name.into(),
            min: us[0],
            mean,
            p50: at(0.50),
            p99: at(0.99),
            max: us[us.len() - 1],
            deadline,
            over,
        }
    }

    fn line(&self) -> String {
        format!(
            "{:<44} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>8.1} {:>7.1}% {:>6}",
            self.name,
            self.min,
            self.mean,
            self.p50,
            self.p99,
            self.max,
            self.p99 / self.deadline * 100.0,
            self.over,
        )
    }
}

/// A few seconds of stereo noise, shared by every lane.
///
/// Noise rather than a sine on purpose: a compressor, a denoiser and a tape
/// unit all take branches that depend on the signal, and a pure tone would
/// measure the cheap side of several of them. Arc'd once, so thirty two lanes
/// playing the same file cost one buffer of memory and not thirty two.
fn noise(seconds: f64) -> Arc<tangent_record::decode::Clip> {
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut x: u32 = 0x1234_5678;
    for _ in 0..frames {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        let n = (x as f32 / u32::MAX as f32).mul_add(2.0, -1.0);
        samples.push(n * 0.25);
        samples.push(n * 0.22);
    }
    Arc::new(tangent_record::decode::Clip {
        samples,
        rate: RATE,
        source: PathBuf::from("rt_bench-noise.wav"),
    })
}

/// Nothing sends, nothing is muted, nothing is soloed — the desk every shape
/// starts from, so no shape is quietly paying for the effects bus.
fn quiet_desk() -> Desk {
    Desk {
        send: [0.0; STRIPS],
        muted: [false; STRIPS],
        soloed: [false; STRIPS],
    }
}

/// An offline engine with `lanes` audio lanes playing noise, each carrying
/// `inserts` in its bays, rolling from frame zero.
fn audio_desk(lanes: usize, inserts: &[&str]) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    // Nothing armed: `Shared::new` arms everything, and an armed lane never
    // parks and runs its rack whether or not anything feeds it. A bench that
    // left that standing would measure a desk nobody records on.
    e.set_armed(0);
    e.set_desk(&quiet_desk());
    let clip = noise(12.0);
    let len = clip.frames() as u64;
    let clips: Vec<Placed> = (0..lanes)
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
        .collect();
    e.set_clips(clips);
    seat(&mut e, lanes, inserts);
    e.set_transport(1, 0);
    e.set_rolling(true);
    e
}

/// An offline engine with `lanes` instrument lanes, each holding one built-in
/// voice in bay 0 and a MIDI clip of `voices` notes sounding together.
fn instrument_desk(lanes: usize, unit: &str, voices: usize) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_armed(0);
    e.set_desk(&quiet_desk());
    let notes: Arc<Vec<ClipNote>> = Arc::new(
        (0..voices)
            .map(|v| ClipNote {
                at: 0,
                // Longer than the whole bench, so every voice is sounding for
                // every measured block rather than decaying out of the figure.
                dur: u64::from(RATE) * 60,
                key: 48 + (v as u8) * 3,
                vel: 100,
            })
            .collect(),
    );
    let clips: Vec<PlacedMidi> = (0..lanes)
        .map(|lane| PlacedMidi {
            strip: lane,
            start: 0,
            len: u64::from(RATE) * 60,
            notes: Arc::clone(&notes),
            pedal: Arc::new(Vec::new()),
        })
        .collect();
    e.set_midi_clips(clips);
    seat(&mut e, lanes, &[unit]);
    e.set_transport(1, 0);
    e.set_rolling(true);
    e
}

/// Seat one identity in each lane's bays. Slow on purpose: `hand_off_exchange`
/// waits `RETIRE_TIMEOUT` for a callback that is not running, so every seat
/// costs a quarter second of the test's wall clock and none of the engine's.
fn seat(e: &mut Engine, lanes: usize, inserts: &[&str]) {
    for lane in 0..lanes {
        for (bay, path) in inserts.iter().enumerate() {
            e.load_insert(lane, bay, Some(Path::new(path)), None)
                .unwrap_or_else(|err| panic!("{path} did not load into lane {lane}: {err}"))
                .unwrap_or_else(|| panic!("{path} loaded but bay {bay} reports no tenant"));
        }
    }
}

/// The transport generation each reading claims, so a shape measured second
/// on the same engine starts from frame zero like the first.
static GENERATION: AtomicU64 = AtomicU64::new(2);

/// Render `warmup` blocks untimed, then `MEASURED` blocks one at a time with
/// a clock either side of each.
fn measure(name: &str, e: &mut Engine, warmup: usize, block: usize) -> Stats {
    // **Back to frame zero for every reading.** Shapes that share an engine
    // would otherwise run the playhead off the end of the noise file, and the
    // shape after that would be timing a desk with no signal on it at all —
    // which is how the FROZEN row first read as free.
    let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
    e.set_transport(generation, 0);
    let mut out = vec![0.0f32; block * 2];
    for _ in 0..warmup {
        e.render_offline(&mut out);
    }
    let mut us = Vec::with_capacity(MEASURED);
    for _ in 0..MEASURED {
        let t = Instant::now();
        e.render_offline(&mut out);
        us.push(t.elapsed().as_nanos() as f64 / 1000.0);
    }
    // Proof the shape actually made a sound: a bench of a silent desk would
    // be measuring the early-outs and nothing else.
    let peak = e
        .strip_peaks()
        .iter()
        .fold(0.0f32, |m, p| m.max(p[0]).max(p[1]));
    let s = Stats::from(name, block, us);
    println!("{}   [peak {peak:.4}]", s.line());
    s
}

/// **The fork renders what the single walk rendered.**
///
/// The one property that matters more than any microsecond: a desk rendered
/// on four threads must come out as a desk rendered on one. Two engines with
/// the same lanes, the same clips and the same inserts, one with a pool and
/// one without, rendered block for block and compared sample for sample.
///
/// Asserted BIT-EXACT rather than to a tolerance, which is a stronger claim
/// than the fan-out strictly needs and is available because the fold was
/// kept ascending: `mix` and `aux` are accumulated with `+=`, f32 addition
/// is not associative, and ascending order is the single walk's own order.
/// The day that stops holding, this test says so rather than a listener.
///
/// Not `#[ignore]`d: it is seconds, and it is the test that would catch a
/// missed private buffer, which is the failure this whole change risks.
#[test]
fn the_fork_renders_what_the_single_walk_rendered() {
    let shapes: [(&str, fn() -> Engine); 4] = [
        ("16 lanes x 3 EQ", || audio_desk(16, &["@tangent-eq", "@tangent-eq", "@tangent-eq"])),
        ("16 lanes x [EQ, comp, plate]", || {
            audio_desk(16, &["@tangent-eq", "@tangent-comp", "@tangent-plate"])
        }),
        ("16 lanes x denoise", || audio_desk(16, &["@tangent-denoise"])),
        ("8 lanes x Analog", || instrument_desk(8, "@tangent-analog", 8)),
    ];
    for (name, build) in shapes {
        let mut one = build();
        let mut many = build();
        one.set_offline_fanout(false);
        many.set_offline_fanout(true);
        let n = BLOCK * 2;
        let mut a = vec![0.0f32; n];
        let mut b = vec![0.0f32; n];
        // Enough blocks for every delay line, envelope and reverb tail in
        // the chain to be carrying state, so a buffer shared between two
        // lanes has had every chance to show. A shared buffer shows in the
        // first block, not the fiftieth; the rest is for the stateful
        // chains. Two hundred costs a tenth of a second now that an
        // offline engine no longer waits on a callback it does not have.
        for block in 0..200 {
            one.render_offline(&mut a);
            many.render_offline(&mut b);
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                assert!(
                    x.to_bits() == y.to_bits(),
                    "{name}: block {block} sample {i}: one walk gave {x}, four gave {y}"
                );
            }
        }
        // And it was not silence agreeing with silence.
        let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.001, "{name}: the reference render was silent, so the test proved nothing");
    }
}

#[test]
#[ignore = "a bench: run it deliberately with --ignored --nocapture"]
fn rt_bench() {
    println!(
        "\nTangent real-time bench: {BLOCK} frames at {RATE} Hz, deadline {DEADLINE_US:.0} us, \
         {MEASURED} blocks measured after {WARMUP} warm\n"
    );
    println!(
        "{:<44} {:>8} {:>8} {:>8} {:>8} {:>8} {:>8} {:>6}",
        "shape", "min us", "mean", "p50", "p99", "max", "p99/dl", "over"
    );

    let mut rows: Vec<Stats> = Vec::new();

    // ── the empty desk, to price a lane ────────────────────────────────────
    let mut e = audio_desk(1, &[]);
    rows.push(measure("1 lane, no inserts", &mut e, WARMUP, BLOCK));
    drop(e);

    let mut e = audio_desk(16, &[]);
    rows.push(measure("16 lanes, no inserts", &mut e, WARMUP, BLOCK));
    drop(e);

    let mut e = audio_desk(32, &[]);
    rows.push(measure("32 lanes, no inserts", &mut e, WARMUP, BLOCK));
    drop(e);

    // ── the same desk under built-in inserts ──────────────────────────────
    let mut e = audio_desk(16, &[EQ_PATH, EQ_PATH, EQ_PATH]);
    rows.push(measure("16 lanes x 3 EQ", &mut e, WARMUP, BLOCK));

    // The three variants share this engine: what is being asked is what the
    // SAME desk costs muted, frozen and stopped, and rebuilding it would put
    // a fresh set of allocations between the readings.
    let mut all_muted = quiet_desk();
    for m in all_muted.muted.iter_mut().take(16) {
        *m = true;
    }
    e.set_desk(&all_muted);
    rows.push(measure("16 lanes x 3 EQ, every lane MUTED", &mut e, WARMUP, BLOCK));
    e.set_desk(&quiet_desk());

    e.set_frozen(0xFFFF);
    rows.push(measure("16 lanes x 3 EQ, every lane FROZEN", &mut e, WARMUP, BLOCK));
    e.set_frozen(0);

    // A stopped transport has to wait out `RACK_PARK_S` (8 s) before the
    // racks stand down, so the warm-up here is nine seconds of blocks.
    e.set_rolling(false);
    let park = (9.0 * f64::from(RATE) / BLOCK as f64) as usize;
    rows.push(measure("16 lanes x 3 EQ, transport STOPPED (parked)", &mut e, park, BLOCK));
    drop(e);

    let mut e = audio_desk(16, &[EQ_PATH, COMP_PATH, PLATE_PATH]);
    rows.push(measure("16 lanes x [EQ, comp, plate]", &mut e, WARMUP, BLOCK));
    // **The same work against the other two deadlines.** A smaller block does
    // not cost proportionally less: the per-block overhead a desk pays once
    // (the fold, the master chain, the taps, every rack's entry) is the same
    // whatever the block is, so halving the buffer more than halves the
    // headroom. This is the shape of the article's deadline table.
    rows.push(measure("16 lanes x [EQ, comp, plate] @ 64 fr", &mut e, WARMUP, 64));
    rows.push(measure("16 lanes x [EQ, comp, plate] @ 256 fr", &mut e, WARMUP, 256));
    drop(e);

    let mut e = audio_desk(16, &[DENOISE_PATH]);
    rows.push(measure("16 lanes x denoise (spectral studio)", &mut e, WARMUP, BLOCK));
    drop(e);

    // **Our own two most expensive units**, which the sweep measured above
    // every commercial plugin on this machine. One per lane, because at
    // three per lane an unfixed Drive desk does not fit the deadline at all
    // and the row would say only that.
    let mut e = audio_desk(16, &[DRIVE_PATH]);
    rows.push(measure("16 lanes x Drive", &mut e, WARMUP, BLOCK));
    drop(e);
    let mut e = audio_desk(16, &[TAPE_PATH]);
    rows.push(measure("16 lanes x Tape", &mut e, WARMUP, BLOCK));
    drop(e);

    let mut e = audio_desk(32, &[EQ_PATH, EQ_PATH, EQ_PATH]);
    rows.push(measure("32 lanes x 3 EQ", &mut e, WARMUP, BLOCK));
    drop(e);

    // ── an instrument, which is what a lane costs when it GENERATES ───────
    let mut e = instrument_desk(8, ANALOG_PATH, 8);
    rows.push(measure("8 lanes x Analog, 8 voices each", &mut e, WARMUP, BLOCK));
    drop(e);

    // ── a third-party VST3, if one is installed and will instantiate here ──
    for bundle in [
        "/Library/Audio/Plug-Ins/VST3/FabFilter Pro-Q 4.vst3",
        "/Library/Audio/Plug-Ins/VST3/FabFilter Pro-Q 3.vst3",
    ] {
        if !Path::new(bundle).exists() {
            continue;
        }
        let mut e = audio_desk(16, &[]);
        let mut ok = true;
        for lane in 0..16 {
            match e.load_insert(lane, 0, Some(Path::new(bundle)), None) {
                Ok(Some(_)) => {}
                Ok(None) => {
                    println!("note: {bundle} loaded but bay 0 of lane {lane} reports no tenant");
                    ok = false;
                    break;
                }
                Err(err) => {
                    println!("note: {bundle} would not load in a test process: {err}");
                    ok = false;
                    break;
                }
            }
        }
        if ok {
            let name = Path::new(bundle)
                .file_stem()
                .map_or_else(|| bundle.to_owned(), |s| s.to_string_lossy().into_owned());
            rows.push(measure(&format!("16 lanes x {name} (VST3)"), &mut e, WARMUP, BLOCK));
        }
        // Emptied by hand before the engine drops, so the plugin is torn down
        // on this thread rather than with the renderer.
        for lane in 0..16 {
            let _ = e.load_insert(lane, 0, None, None);
        }
        drop(e);
        break;
    }

    println!("\n── what the shapes imply ──");
    let get = |n: &str| rows.iter().find(|r| r.name == n).map(|r| r.mean);
    if let (Some(one), Some(sixteen), Some(thirty_two)) = (
        get("1 lane, no inserts"),
        get("16 lanes, no inserts"),
        get("32 lanes, no inserts"),
    ) {
        println!("bare lane, marginal:        {:.2} us/block (1 -> 16)", (sixteen - one) / 15.0);
        println!("bare lane, marginal:        {:.2} us/block (16 -> 32)", (thirty_two - sixteen) / 16.0);
    }
    if let (Some(bare), Some(eq3)) = (get("16 lanes, no inserts"), get("16 lanes x 3 EQ")) {
        println!("built-in EQ, marginal:      {:.2} us/block per instance", (eq3 - bare) / 48.0);
    }
    if let (Some(bare), Some(mix)) = (get("16 lanes, no inserts"), get("16 lanes x [EQ, comp, plate]")) {
        println!("EQ+comp+plate, marginal:    {:.2} us/block per lane", (mix - bare) / 16.0);
    }
    if let (Some(bare), Some(dn)) = (
        get("16 lanes, no inserts"),
        get("16 lanes x denoise (spectral studio)"),
    ) {
        println!("denoise, marginal:          {:.2} us/block per instance", (dn - bare) / 16.0);
    }
    println!("\ndeadline at 128 frames / 48 kHz: {DEADLINE_US:.1} us per block");
    println!("(64 frames = {:.1} us, 256 frames = {:.1} us)", DEADLINE_US / 2.0, DEADLINE_US * 2.0);
}
