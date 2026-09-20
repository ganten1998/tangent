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

use crate::builtin_fx::{
    ANALOG_PATH, CHORUS_PATH, COMP_PATH, DENOISE_PATH, DRIVE_PATH, ECHO_PATH, EQ_PATH, IR_PATH,
    PLATE_PATH, TAPE_PATH,
};
use crate::instrument::{ClipNote, ClipPedal, Engine, Placed, PlacedMidi};

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
/// **`TANGENT_GRAIN=10.67` turns the grain render on for every desk the
/// bench builds**, in milliseconds, exactly as the setting takes it.
///
/// The door lives here and not in the engine: the engine reads the setting
/// the app pushes it, and a process-wide override there would mean two
/// engines in one process could not disagree - which is precisely what the
/// null test needs.
fn grain_from_env(e: &Engine) {
    if let Some(ms) = std::env::var("TANGENT_GRAIN").ok().and_then(|v| v.parse().ok()) {
        e.set_grain_ms(ms);
    }
}

fn audio_desk(lanes: usize, inserts: &[&str]) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    // Nothing armed: `Shared::new` arms everything, and an armed lane never
    // parks and runs its rack whether or not anything feeds it. A bench that
    // left that standing would measure a desk nobody records on.
    e.set_armed(0);
    // **An audio track's `midi_off` bit is SET**, and zero means every lane
    // is listening to live MIDI. Leaving it at the default would make every
    // lane here read as an instrument lane, which matters to nothing in the
    // timings and to everything in the eligibility probe.
    e.set_midi_off(u64::MAX);
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
    grain_from_env(&e);
    e
}

/// **One lane, one real plugin, and a look at what comes out.**
///
/// `TANGENT_PLUGIN="/Library/Audio/Plug-Ins/VST3/FabFilter Pro-C 3.vst3"
/// cargo test --release -p tangent --bin tangent one_plugin_on_one_lane --
/// --ignored --nocapture`
///
/// A breakup reported on one track with one compressor on it, absent when
/// that compressor is switched off, and invisible to the load meter. This
/// puts the same plugin on one lane over a steady signal and reports every
/// place the output steps by more than its neighbourhood can explain, which
/// is what a click is. A compressor over a constant tone should produce
/// nothing but a smooth envelope.
#[test]
#[ignore = "a hunt with a real plugin, not an assertion"]
fn one_plugin_on_one_lane() {
    let Ok(bundle) = std::env::var("TANGENT_PLUGIN") else {
        println!("set TANGENT_PLUGIN to a .vst3 bundle");
        return;
    };
    if !Path::new(&bundle).exists() {
        println!("no plugin at {bundle}");
        return;
    }
    // **A steady tone, not noise.** A click in noise is invisible; a click
    // in a sine is the only thing in the picture. Loud enough that a
    // compressor has something to do.
    let rate = f64::from(RATE);
    let secs = 20.0;
    let frames = (secs * rate) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    for i in 0..frames {
        let t = i as f64 / rate;
        // A tone that swells and falls, so the compressor opens and closes
        // rather than sitting at one gain reduction for twenty seconds.
        let env = 0.15 + 0.6 * (0.5 - 0.5 * (t * std::f64::consts::TAU / 4.0).cos());
        let v = (env * (t * 220.0 * std::f64::consts::TAU).sin()) as f32;
        samples.push(v);
        samples.push(v);
    }
    let clip = Arc::new(tangent_record::decode::Clip {
        samples,
        rate: RATE,
        source: PathBuf::from("tone.wav"),
    });

    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    e.set_armed(0);
    e.set_midi_off(u64::MAX);
    e.set_desk(&quiet_desk());
    let len = clip.frames() as u64;
    e.set_clips(vec![Placed {
        clip,
        strip: 0,
        start: 0,
        offset: 0,
        len,
        fade_in: 0,
        fade_out: 0,
        gain: 1.0,
    }]);
    match e.load_insert(0, 0, Some(Path::new(&bundle)), None) {
        Ok(Some(_)) => {}
        Ok(None) => {
            println!("{bundle} loaded but bay 0 reports no tenant");
            return;
        }
        Err(err) => {
            println!("{bundle} would not load here: {err}");
            return;
        }
    }
    e.set_transport(1, 0);
    e.set_rolling(true);

    let block = 256usize;
    let blocks = frames / block;
    let mut out = vec![0.0f32; block * 2];
    let mut whole: Vec<f32> = Vec::with_capacity(blocks * block * 2);
    for _ in 0..blocks {
        out.fill(0.0);
        e.render_offline(&mut out);
        whole.extend_from_slice(&out);
    }
    let left: Vec<f32> = whole.iter().step_by(2).copied().collect();
    let peak = left.iter().fold(0.0f32, |m, v| m.max(v.abs()));
    // The step between neighbouring samples of a 220 Hz tone is tiny; the
    // biggest honest one is at the zero crossing of the loudest passage.
    let mut steps: Vec<(usize, f32)> = left
        .windows(2)
        .enumerate()
        .map(|(i, w)| (i, (w[1] - w[0]).abs()))
        .collect();
    steps.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let median = {
        let mut v: Vec<f32> = left.windows(2).map(|w| (w[1] - w[0]).abs()).collect();
        v.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        v[v.len() / 2]
    };
    println!(
        "{}: peak {peak:.4}, median step {median:.6}",
        Path::new(&bundle).file_stem().unwrap_or_default().to_string_lossy()
    );
    for (at, step) in steps.iter().take(10) {
        println!(
            "    frame {at} ({:.4} s): step {step:.5} = {:.0}x the median, block boundary {}",
            *at as f64 / rate,
            step / median.max(1e-9),
            at % block == 0
        );
    }
    assert!(peak > 0.01, "the fixture made no sound");
}

/// **An engine whose lanes carry real impulse responses.**
///
/// The convolver is the one built-in whose cost is set by a file rather than
/// a knob, and the sweep put it at the top of the remaining gap list: a fixed
/// transform cost on every bay whatever the IR length, paid on the block the
/// staging buffer happens to fill. Half a second of reverb is about what a
/// commercial bay costs on this machine, so ninety-six of them is the rung
/// the plan's own acceptance gate was written against.
fn ir_desk(lanes: usize, bays: usize, ir_seconds: f64) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_armed(0);
    // **An audio track's `midi_off` bit is SET**, and zero means every lane
    // is listening to live MIDI. Leaving it at the default would make every
    // lane here read as an instrument lane, which matters to nothing in the
    // timings and to everything in the eligibility probe.
    e.set_midi_off(u64::MAX);
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

    let state = ir_state(ir_seconds);
    for lane in 0..lanes {
        for bay in 0..bays {
            e.load_insert(lane, bay, Some(Path::new(IR_PATH)), Some(&state))
                .unwrap_or_else(|err| panic!("the IR bay did not load into lane {lane}: {err}"))
                .unwrap_or_else(|| panic!("the IR loaded but bay {bay} reports no tenant"));
            // **A bay that quietly failed to find its file is a wire**, and
            // a wire would measure as the fastest reverb ever written. This
            // is the whole proof: the bay's error slot is empty only if the
            // WAV was found, decoded and accepted by `Convolver::set_ir`,
            // which is the one call that builds the spectra.
            assert!(
                e.insert_state_error(lane, bay).is_none(),
                "lane {lane} bay {bay}: {:?}",
                e.insert_state_error(lane, bay)
            );
        }
    }
    e.set_transport(1, 0);
    e.set_rolling(true);
    e
}

/// A decaying-noise IR of `seconds`, written to a real file, wrapped in the
/// state blob a bay restores itself from. Written once per length and left
/// behind in the temp directory, which is where the suite's other fixtures go.
fn ir_state(seconds: f64) -> Vec<u8> {
    let spec = crate::builtin_fx::spec_for(IR_PATH).expect("the IR bay has a spec");
    let frames = (seconds * f64::from(RATE)) as usize;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut seed = 0x51ED_5EEDu64;
    for i in 0..frames {
        let t = i as f64 / f64::from(RATE);
        let env = (-t / (seconds * 0.35).max(0.05)).exp() as f32;
        for _ in 0..2 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            // 24 bits mapped to -1..1.
            let n = (seed >> 40) as f32 / 8_388_608.0 - 1.0;
            samples.push(n * env);
        }
    }
    // **Normalised to unit energy per channel**, which is what a real
    // capture is and what keeps three of these in series from compounding
    // into a signal no reverb produces. A bench measuring a runaway level
    // is measuring the wrong thing.
    for ch in 0..2 {
        let energy: f64 = samples
            .iter()
            .skip(ch)
            .step_by(2)
            .map(|s| f64::from(*s) * f64::from(*s))
            .sum();
        let gain = (1.0 / energy.max(1e-12).sqrt()) as f32;
        for s in samples.iter_mut().skip(ch).step_by(2) {
            *s *= gain;
        }
    }
    let mut wav = Vec::with_capacity(samples.len() * 4 + 64);
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&0u32.to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&3u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&0u32.to_le_bytes());
    wav.extend_from_slice(&0u16.to_le_bytes());
    wav.extend_from_slice(&32u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&((samples.len() * 4) as u32).to_le_bytes());
    for s in &samples {
        wav.extend_from_slice(&s.to_le_bytes());
    }
    let path = std::env::temp_dir().join(format!(
        "tangent-bench-ir-{}-{}ms.wav",
        std::process::id(),
        (seconds * 1000.0) as u64
    ));
    std::fs::write(&path, &wav).expect("the bench IR writes");
    crate::builtin_fx::state_text(
        spec,
        &spec.defaults(),
        &[("ir_path".to_owned(), path.display().to_string())],
    )
    .into_bytes()
}

/// An offline engine with `lanes` instrument lanes, each holding one built-in
/// voice in bay 0 and a MIDI clip of `voices` notes sounding together.
fn instrument_desk(lanes: usize, unit: &str, voices: usize) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_armed(0);
    // **Nothing armed means nothing listening**, even on instrument lanes:
    // `live_midi_off` clears a lane's bit only when it is a MIDI channel AND
    // armed, or is the one lane an open editor names. These lanes play from
    // clips, so a mixing session is what this models.
    e.set_midi_off(u64::MAX);
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
    grain_from_env(&e);
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

/// One unit into one lane's first bay.
fn seat_one(e: &mut Engine, lane: usize, unit: &str) {
    e.load_insert(lane, 0, Some(Path::new(unit)), None)
        .unwrap_or_else(|err| panic!("{unit} did not load into lane {lane}: {err}"))
        .unwrap_or_else(|| panic!("{unit} loaded but the bay reports no tenant"));
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
    // The eligibility probe, when it is on. Zeroed per row so each shape's
    // figure is its own; the counters live on the engine's `Shared`, which
    // is per desk here, so this only matters for shapes measured twice.
    let probe = e.probe();
    if probe.blocks > 0 {
        println!("    {}", probe.line());
    }
    s
}

/// **A lane that rests is a lane that had gone quiet, not one that was
/// still sounding.**
///
/// While the transport rolls, a lane with a rack and any clip anywhere used
/// to run its whole chain on every block of the song. It rests now when it
/// has nothing under the playhead, which is most of a song for most tracks
/// and is worth about 2.3x on a desk where ten of sixteen lanes are idle.
///
/// The risk that buys is a reverb tail cut off mid-song, and the term that
/// removes it is the lane's own measured output: a lane keeps rendering
/// while it is above `PARK_FLOOR`, which is -80 dBFS. So this plays a short
/// clip through a plate and then watches the decay for ten seconds, and
/// asserts that the block where the lane finally stops contributing was
/// already inaudible. That is also why the rest needs no fade.
#[test]
fn a_reverb_tail_is_never_cut_off_by_a_lane_going_to_rest() {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine");
    e.set_armed(0);
    // **An audio track's `midi_off` bit is SET**, and zero means every lane
    // is listening to live MIDI. Leaving it at the default would make every
    // lane here read as an instrument lane, which matters to nothing in the
    // timings and to everything in the eligibility probe.
    e.set_midi_off(u64::MAX);
    e.set_desk(&quiet_desk());
    // Half a second of noise into a plate, then nothing for ten seconds.
    let clip = noise(0.5);
    let len = clip.frames() as u64;
    e.set_clips(vec![Placed {
        clip,
        strip: 0,
        start: 0,
        offset: 0,
        len,
        fade_in: 0,
        fade_out: 0,
        gain: 0.5,
    }]);
    seat(&mut e, 1, &[PLATE_PATH]);
    e.set_transport(1, 0);
    e.set_rolling(true);
    let mut out = vec![0.0f32; BLOCK * 2];
    // Fourteen seconds. The plate is under the floor by about two and a
    // half, and the rest waits a further `RACK_PARK_S` of quiet after that,
    // so the moment being tested lands around eleven.
    let blocks = (14.0 * f64::from(RATE) / BLOCK as f64) as usize;
    let mut last_heard = 0.0f32;
    let mut first_silent: Option<usize> = None;
    for b in 0..blocks {
        out.fill(0.0);
        e.render_offline(&mut out);
        let peak = out.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak > 0.0 {
            last_heard = peak;
            first_silent = None;
        } else if first_silent.is_none() {
            first_silent = Some(b);
        }
    }
    // It really did decay rather than running on for ever, or the test
    // proves nothing about the cut.
    let at = first_silent.expect(
        "the lane never went to rest in fourteen seconds, so the saving this \
         whole change exists for is not happening",
    );
    let secs = at as f64 * BLOCK as f64 / f64::from(RATE);
    assert!(secs > 8.0, "the lane rested after {secs:.1}s, sooner than the park window");
    // And the last sound before it stopped was already below the floor the
    // rest is keyed on, so nothing audible was taken away.
    assert!(
        last_heard <= 1.0e-4,
        "the tail was cut at {last_heard} ({:.1} dBFS), which is audible",
        20.0 * last_heard.max(1e-12).log10()
    );
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

/// An instrument desk whose notes are spread through the song rather than
/// all struck at frame zero, so a grain boundary lands between notes over
/// and over. `instrument_desk` holds every voice down for a minute, which
/// would exercise the note walk exactly once.
fn midi_pattern_desk(lanes: usize, unit: &str) -> Engine {
    let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_armed(0);
    e.set_midi_off(u64::MAX);
    e.set_desk(&quiet_desk());
    // A note every 700 frames, 500 long: shorter than a 512-frame grain and
    // not a divisor of it, so ons and offs land at every offset within a
    // grain rather than always at its head.
    // **A pitch of its own per lane.** Eight lanes on ONE pitch 37 frames
    // apart is not a desk, it is an interferometer: the sum of eight nearly
    // coincident copies of a tone swings several decibels on a phase shift
    // far too small to hear, and a level comparison then reports it as a
    // missing note. Different pitches make the lanes independent, which is
    // both more like music and the only way a level metric means anything.
    let clips: Vec<PlacedMidi> = (0..lanes)
        .map(|lane| PlacedMidi {
            strip: lane,
            start: u64::from(lane as u32) * 37,
            len: 400 * 700,
            notes: Arc::new(
                (0..400u64)
                    .map(|i| ClipNote {
                        at: i * 700,
                        dur: 500,
                        key: 40 + (lane as u8) * 5 + (i % 4) as u8,
                        vel: 90,
                    })
                    .collect(),
            ),
            pedal: Arc::new(Vec::new()),
        })
        .collect();
    e.set_midi_clips(clips);
    seat(&mut e, lanes, &[unit]);
    e.set_transport(1, 0);
    e.set_rolling(true);
    grain_from_env(&e);
    e
}

/// Bisect the buffer-size dependence down to one lane and one note.
#[test]
#[ignore = "a diagnostic, not an assertion"]
fn where_the_buffer_size_dependence_comes_from() {
    fn one_note(dur: u64, at: u64) -> Engine {
        let mut e =
            Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        e.set_midi_clips(vec![PlacedMidi {
            strip: 0,
            start: 0,
            len: u64::from(RATE) * 60,
            notes: Arc::new(vec![ClipNote { at, dur, key: 60, vel: 100 }]),
            pedal: Arc::new(Vec::new()),
        }]);
        seat(&mut e, 1, &["@tangent-analog"]);
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    for (what, dur, at) in [
        ("one note, on at 0, never ends", u64::from(RATE) * 60, 0),
        ("one note, on at 0, off at 500", 500, 0),
        ("one note, on at 300, off at 800", 500, 300),
        ("one note, on at 0, off at 5000", 5000, 0),
    ] {
        let (mut small, mut big) = (one_note(dur, at), one_note(dur, at));
        let (mut sa, mut bb) = (vec![0.0f32; 128 * 2], vec![0.0f32; 512 * 2]);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for _ in 0..200 {
            small.render_offline(&mut sa);
            xs.extend_from_slice(&sa);
        }
        for _ in 0..50 {
            big.render_offline(&mut bb);
            ys.extend_from_slice(&bb);
        }
        let n = xs.len().min(ys.len());
        let first = xs[..n].iter().zip(&ys[..n]).position(|(x, y)| x.to_bits() != y.to_bits());
        let worst = xs[..n].iter().zip(&ys[..n]).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        let db = |v: f32| 20.0 * v.max(1e-12).log10();
        println!(
            "  {what:<34} worst {:>7.1} dBFS   first differs at frame {}",
            db(worst),
            first.map_or("never".to_owned(), |i| (i / 2).to_string())
        );
    }
}

/// **A grain that is not a whole number of blocks renders the same audio
/// as one that is.**
///
/// The test that was missing, and it cost a shipped bug. The null test
/// beside this one uses a 512-frame grain against a 128-frame block, which
/// is a clean multiple, so the fold's span never crosses a grain boundary
/// and `covers` never fails after the first chunk. The knob offers 5 ms,
/// which at a 128-frame buffer is 240 frames and crosses one constantly.
///
/// There, `covers` failed on EVERY chunk - the ring ends part way through
/// the span the fold wants - and the lane caught up from the playhead,
/// handing its rack audio it had already been through. 199 overlapping
/// catch-ups in 200 chunks, and a plate reverb 15.7 dB wrong. A flat EQ and
/// a bare clip were bit-exact throughout, which is what made it invisible:
/// an EQ at its defaults is a wire, and a wire does not care how many times
/// it is handed the same sample.
///
/// So the shapes here are chosen to have state that MINDS: a reverb's
/// delay lines, a compressor's envelope, a saturator.
#[test]
fn a_grain_that_is_not_a_whole_number_of_blocks_is_the_same_audio() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    for (name, build) in shapes {
        // Straddling and aligned, mixed together on purpose.
        for grain in [160.0_f64, 192.0, 240.0, 256.0, 320.0, 384.0, 448.0, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let mut peak = 0.0f32;
            for block in 0..120 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                    assert!(
                        x.to_bits() == y.to_bits(),
                        "{name} at a {grain:.0}-frame grain against a {BLOCK}-frame \
                         block: block {block} sample {i} gave {x} chunked and {y} \
                         grained"
                    );
                }
                peak = a.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            assert!(peak > 0.001, "{name}: silence proves nothing");
        }
    }
}

/// **A built-in instrument is the same sound at any buffer size.**
///
/// It was not, and the grain render is what found it. No grains here at
/// all: two engines with the same notes, one fed 128-frame buffers and one
/// fed 512, compared as one stream.
///
/// Before the fix, Analog driven by notes that start and stop INSIDE a
/// block rendered the two to streams differing by **-15.5 dBFS against a
/// peak of -17.1** - louder than the signal. Held notes were bit-exact,
/// and so were clips, an EQ and a compressor, which is what made it
/// puzzling until the bisection: `Synth::render` cut its chunk grid from
/// the start of each BUFFER, so a note landing mid-buffer restarted the
/// grid there and from that point a 512-frame buffer and a 128-frame one
/// put their chunk boundaries in different places. Every per-chunk
/// quantity was then computed a different number of times.
///
/// Asserted bit-exact, because that is what it now is, and because
/// anything less would let it drift back.
#[test]
fn a_builtin_instrument_is_the_same_at_any_buffer_size() {
    for (name, build) in [
        ("Analog, notes held", (|| instrument_desk(4, "@tangent-analog", 4)) as fn() -> Engine),
        ("Analog, a note every 700 frames", || midi_pattern_desk(4, "@tangent-analog")),
        ("clips only", || audio_desk(4, &[])),
        ("clips x [EQ, comp]", || audio_desk(4, &["@tangent-eq", "@tangent-comp"])),
    ] {
        let (mut small, mut big) = (build(), build());
        let (mut sa, mut bb) = (vec![0.0f32; 128 * 2], vec![0.0f32; 512 * 2]);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for _ in 0..200 {
            small.render_offline(&mut sa);
            xs.extend_from_slice(&sa);
        }
        for _ in 0..50 {
            big.render_offline(&mut bb);
            ys.extend_from_slice(&bb);
        }
        let n = xs.len().min(ys.len());
        let first = xs[..n]
            .iter()
            .zip(&ys[..n])
            .position(|(x, y)| x.to_bits() != y.to_bits());
        if let Some(i) = first {
            let f = i / 2;
            println!(
                "      first differs at frame {f} (block {} of 128, offset {}), \
                 {:.9} vs {:.9}",
                f / 128,
                f % 128,
                xs[i],
                ys[i]
            );
        }
        let worst = xs[..n]
            .iter()
            .zip(&ys[..n])
            .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
        let peak = xs[..n].iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let db = |v: f32| 20.0 * v.max(1e-12).log10();
        assert!(
            worst == 0.0,
            "{name}: 128-frame buffers and 512-frame ones render this to \
             streams differing by {:.1} dBFS against a peak of {:.1}, so the \
             instrument's sound depends on the buffer size",
            db(worst),
            db(peak)
        );
        assert!(peak > 0.001, "{name}: silence agreeing with silence proves nothing");
    }
}

/// **A grain render is the same audio, arriving earlier.**
///
/// The one property that stands between the grain render and wrong sound.
/// A lane eligible for it is rendered in 512-frame grains staggered across
/// the desk instead of once per callback, so on most callbacks its plugins
/// are not called at all and the fold reads a span out of a ring that was
/// filled some blocks ago. Every part of that is new: the ring's key, the
/// span gather across a grain boundary, the stagger's phase, the catch-up
/// after a locate, `run_lane_pdc` clocking a delay line over a staging pair,
/// and - for the instrument shapes - a lane's notes being found for a span
/// the playhead has not reached.
///
/// **Asserted BIT-EXACT through continuous playback**, which is available
/// here and would not be for every desk: these shapes are sample-by-sample
/// arithmetic, so the block size they are cut into cannot change the answer.
/// It is not true of a plugin that blocks internally, which is why the
/// shapes are chosen and not arbitrary - a hosted convolver partitions its
/// own input and a denoiser windows it, and both would differ for a reason
/// that is not a defect.
///
/// **Across a LOCATE it is not bit-exact, and the two reasons are
/// different.** For an effects lane, a grained lane has rendered up to a
/// grain past the playhead and a locate abandons that audio: the ring is
/// emptied but a compressor's envelope cannot be, so the two differ by a
/// decaying amount. `locate_bounded` asserts that difference stays under
/// -40 dBFS.
///
/// `locate_bounded` is a field rather than an assumption, because a lane
/// carrying a HELD instrument note would not be bounded and the difference
/// would not be an error: a locate does not re-articulate a note that is
/// still down - that is deliberate, see `told` - so the note goes on
/// sounding and a grained lane's oscillator would have advanced up to a
/// grain further through it. Two copies of one tone at different phases
/// differ by as much as the tone itself, and no listener has a reference to
/// hear it against. Instrument lanes are not grained today, so every shape
/// here is bounded; the field is what stops that being forgotten on the day
/// they are.
#[test]
fn a_grained_lane_renders_what_the_chunked_lane_rendered() {
    struct Shape {
        name: &'static str,
        build: fn() -> Engine,
        /// False where a held voice's phase makes the transient meaningless.
        locate_bounded: bool,
    }
    let shapes = [
        Shape { name: "16 lanes, clips only", build: || audio_desk(16, &[]), locate_bounded: true },
        Shape {
            name: "16 lanes x 3 EQ",
            build: || audio_desk(16, &["@tangent-eq", "@tangent-eq", "@tangent-eq"]),
            locate_bounded: true,
        },
        Shape {
            name: "16 lanes x [EQ, comp]",
            build: || audio_desk(16, &["@tangent-eq", "@tangent-comp"]),
            locate_bounded: true,
        },
        // **The instrument lanes, which is what the notes half is for.**
        // Their notes are found by where the block is rather than by a
        // cursor, and what the instrument has been told advances in step
        // with whichever span feeds it - so a grain covering a different
        // span must still articulate every note on the same frame.
        // **The instrument lanes, grained like the rest.** Their notes are
        // found by where the block is rather than by a cursor, and what the
        // instrument has been told advances in step with whichever span
        // feeds it, so a grain covering a different span must still
        // articulate every note on the same frame - which the 200 blocks
        // above assert bit-for-bit.
        //
        // **Not bounded across a locate, and that is not a dodge.** The
        // discard knocks the phase of everything free-running in the
        // instrument sideways - its oscillators, its chorus - and a held
        // note is not re-articulated by a locate, so the two renders are
        // one tone against itself out of phase. Sample comparison says as
        // much as the tone itself and means nothing. What IS asserted for
        // these is that the NOTES are unchanged: same frames, same level,
        // measured as energy over a window as long as the note spacing.
        Shape {
            name: "8 lanes x Analog, notes held",
            build: || instrument_desk(8, "@tangent-analog", 4),
            locate_bounded: false,
        },
        Shape {
            name: "8 lanes x Analog, a note every 700 frames",
            build: || midi_pattern_desk(8, "@tangent-analog"),
            locate_bounded: false,
        },
    ];
    for Shape { name, build, locate_bounded } in shapes {
        let mut chunked = build();
        let mut grained = build();
        // 512 frames at 48 kHz, against a 128-frame block: eight callbacks
        // to a grain, so every lane spends most of them rendering nothing.
        grained.set_grain_ms(1000.0 * 512.0 / f64::from(RATE));
        let n = BLOCK * 2;
        let mut a = vec![0.0f32; n];
        let mut b = vec![0.0f32; n];
        let mut peak = 0.0f32;
        for block in 0..200 {
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                assert!(
                    x.to_bits() == y.to_bits(),
                    "{name}: block {block} sample {i}: chunked gave {x}, grained gave {y}"
                );
            }
            peak = a.iter().fold(peak, |m, s| m.max(s.abs()));
        }
        assert!(
            peak > 0.001,
            "{name}: the reference render was silent, so the test proved nothing"
        );

        // ── and now a locate, which is the one irreducible discard ───────
        let at = 7 * BLOCK as u64 + 13;
        for e in [&mut chunked, &mut grained] {
            e.set_transport(2, at);
        }
        let (mut worst, mut tail) = (0.0f32, 0.0f32);
        // The whole post-locate stream, for the shapes whose samples cannot
        // be compared and whose NOTES must be.
        let (mut whole_a, mut whole_b) = (Vec::new(), Vec::new());
        for block in 0..80 {
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            let d = a
                .iter()
                .zip(&b)
                .fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
            worst = worst.max(d);
            if block >= 60 {
                tail = tail.max(d);
            }
            whole_a.extend_from_slice(&a);
            whole_b.extend_from_slice(&b);
        }
        let env_frames = whole_a.len() / 2;
        let db = |v: f32| 20.0 * v.max(1e-12).log10();
        // The numbers are MEASURED, not chosen: the effects desk starts at
        // -53.7 dBFS and is -72.6 dBFS a fifth of a second later, decaying
        // toward zero without reaching it - two envelopes differing by a
        // factor that shrinks exponentially approach equality in f32 rather
        // than landing on it. The bounds sit outside the measurements so
        // this is a regression net and not a tripwire.
        if locate_bounded {
            assert!(
                db(worst) < -40.0,
                "{name}: a locate cost {:.1} dBFS, which is audible against \
                 the discontinuity a locate already has",
                db(worst)
            );
        }
        if locate_bounded {
            assert!(
                db(tail) < -65.0,
                "{name}: a fifth of a second after the locate the two \
                 renders still differ by {:.1} dBFS, so the discarded grain \
                 is not decaying out of the lane the way it should",
                db(tail)
            );
        } else {
            // **ENERGY over a window as long as the note spacing**, which
            // averages the phase out and leaves "was a note played, when,
            // and how loud". A per-block PEAK will not do it: two copies of
            // one tone at different phases differ by as much as the tone,
            // and that metric reported 8.4 dB while the notes were in fact
            // landing on the same frames to within half a decibel.
            //
            // The first two windows are skipped: that is the flushed tail,
            // where the two legitimately differ and no note is due.
            // **Only where there is a note to judge**, and the floor is
            // set against the MUSIC rather than against whatever is left
            // after the jump. A held chord is not re-articulated by a
            // locate that lands past it, so that desk goes quiet and all
            // that remains is two release tails 59 dB down, decaying 8 dB a
            // window; a ratio taken there says nothing. 60 dB under the
            // peak of the playing the locate interrupted.
            let w = 700usize;
            let e = |xs: &[f32], k: usize| -> f32 {
                let s: f32 = xs[k * w * 2..((k + 1) * w * 2).min(xs.len())]
                    .iter()
                    .map(|v| v * v)
                    .sum();
                (s / (w * 2) as f32).sqrt()
            };
            let windows = (env_frames / w).saturating_sub(1);
            let floor = peak * 0.001;
            let mut worst_note = 0.0f32;
            let (mut tot_a, mut tot_b) = (0.0f32, 0.0f32);
            for k in 2..windows {
                if e(&whole_a, k) < floor {
                    continue;
                }
                if std::env::var_os("TANGENT_WINDOWS").is_some() {
                    println!(
                        "      {name} w{k}: chunked {:.1} grained {:.1} diff {:.1}",
                        db(e(&whole_a, k)),
                        db(e(&whole_b, k)),
                        db(e(&whole_a, k)) - db(e(&whole_b, k))
                    );
                }
                worst_note = worst_note.max((db(e(&whole_a, k)) - db(e(&whole_b, k))).abs());
                tot_a += e(&whole_a, k) * e(&whole_a, k);
                tot_b += e(&whole_b, k) * e(&whole_b, k);
            }
            // **The total is what says no note was lost**, and the window
            // is a secondary net. Voices that are independent still
            // interfere: one window in thirteen swings 2.8 dB here while
            // the total moves 0.20, which is phase and not a note. A note
            // actually missing, late or at the wrong level takes energy out
            // of the sum, and the sum is measured to a fifth of a decibel.
            let total = (db(tot_a.sqrt()) - db(tot_b.sqrt())).abs();
            assert!(
                total < 1.0,
                "{name}: after the locate the two renders differ by \
                 {total:.2} dB of total energy, so a note is missing or at \
                 the wrong level rather than merely at a different phase"
            );
            assert!(
                worst_note < 4.0,
                "{name}: after the locate one window's energy differs by \
                 {worst_note:.1} dB, which is more than independent voices \
                 interfering"
            );
        }
    }
}

// ── the grain render, under the conditions the desk actually meets ───────
//
// The three tests above prove the grain render is the same audio as the
// chunked one on a desk that rolls forward from zero at one buffer size and
// never changes. A desk does none of those things for long. What follows is
// the rest of it: a loop, a callback whose size moves, a lane that stops
// being eligible while the transport rolls, and the knob itself being
// turned mid-roll.

/// Render `blocks` callbacks whose sizes cycle through `sizes`, appending
/// every sample rendered to `into`.
///
/// A real driver does not promise one size for ever: CoreAudio may hand a
/// different count when the device is reconfigured under load, and the
/// offline exporter splits at `MAX_BLOCK` whatever it was asked for. The
/// grain clock is keyed on absolute position rather than on a block index
/// precisely so that this is a non-event, and this is what says so.
fn render_sizes(e: &mut Engine, sizes: &[usize], blocks: usize, into: &mut Vec<f32>) {
    let widest = sizes.iter().copied().max().unwrap_or(BLOCK);
    let mut buf = vec![0.0f32; widest * 2];
    for b in 0..blocks {
        let n = sizes[b % sizes.len()] * 2;
        e.render_offline(&mut buf[..n]);
        into.extend_from_slice(&buf[..n]);
    }
}

/// The worst absolute difference between two streams, and where it is.
fn worst_diff(xs: &[f32], ys: &[f32]) -> (f32, Option<usize>) {
    let n = xs.len().min(ys.len());
    let first = xs[..n].iter().zip(&ys[..n]).position(|(x, y)| x.to_bits() != y.to_bits());
    let worst = xs[..n].iter().zip(&ys[..n]).fold(0.0f32, |m, (x, y)| m.max((x - y).abs()));
    (worst, first)
}

fn dbfs(v: f32) -> f64 {
    20.0 * f64::from(v.max(1e-12)).log10()
}

/// **A loop wraps the same way grained as chunked.**
///
/// `clip_grain` is the one line of the plan that had never been run: a grain
/// may not cross `loop_b`, because the wrap folds the overshoot back to
/// `loop_a` and audio rendered past the end is audio of a bar that will not
/// be played next. The alternative - flushing every lane on every wrap -
/// costs a full-desk catch-up a lap in exactly the workflow the feature is
/// for, so the grain is cut short at the loop end instead and the lane picks
/// up again on the other side.
///
/// The loop is 3000 frames on purpose: 23.4 blocks, 12.5 grains at 240 and
/// 5.86 at 512, so the wrap lands mid-block and mid-grain rather than on a
/// boundary that would make the clipping a no-op.
#[test]
fn a_loop_wraps_the_same_way_grained_as_chunked() {
    const LOOP_B: u64 = 3000;
    let shapes: [(&str, fn() -> Engine); 4] = [
        ("clips only", || audio_desk(8, &[])),
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    for (name, build) in shapes {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            chunked.set_loop(0, LOOP_B);
            grained.set_loop(0, LOOP_B);
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let mut peak = 0.0f32;
            // 240 blocks is 30720 frames, which is ten laps of this loop.
            for block in 0..240 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                    assert!(
                        x.to_bits() == y.to_bits(),
                        "{name} looping 0..{LOOP_B} at a {grain:.0}-frame grain: \
                         block {block} sample {i} gave {x} chunked and {y} grained"
                    );
                }
                peak = a.iter().fold(peak, |m, s| m.max(s.abs()));
            }
            // **Proof the loop actually wrapped.** Ten laps of a 3000-frame
            // loop is 30720 frames of playing; a transport that ran straight
            // through would be far past the end, and a test that compared
            // two renders of an un-looped desk would prove nothing about
            // `clip_grain` at all.
            for (what, e) in [("chunked", &chunked), ("grained", &grained)] {
                assert!(
                    e.transport_position() < LOOP_B,
                    "{name}: the {what} transport is at {} after ten laps of a \
                     {LOOP_B}-frame loop, so it never wrapped",
                    e.transport_position()
                );
            }
            assert!(peak > 0.001, "{name}: the looped render was silent");
        }
    }
}

/// **The energy of two streams, window by window**, as decibels of
/// difference: the total, and the worst single window.
///
/// The metric for a shape whose samples cannot be compared. Two copies of
/// one tone at different phases differ by as much as the tone itself, and a
/// peak taken there reports a catastrophe where a note is in fact landing
/// on the same frame at the same level. Energy over a window as long as the
/// note spacing averages the phase out and leaves "was a note played, when,
/// and how loud".
fn energy_gap(xs: &[f32], ys: &[f32], floor: f32) -> (f64, f64) {
    let w = 700usize;
    let e = |v: &[f32], k: usize| -> f32 {
        let s: f32 = v[k * w * 2..((k + 1) * w * 2).min(v.len())].iter().map(|x| x * x).sum();
        (s / (w * 2) as f32).sqrt()
    };
    let windows = (xs.len().min(ys.len()) / 2 / w).saturating_sub(1);
    let (mut tot_a, mut tot_b, mut worst) = (0.0f32, 0.0f32, 0.0f64);
    for k in 0..windows {
        if e(xs, k) < floor {
            continue;
        }
        worst = worst.max((dbfs(e(xs, k)) - dbfs(e(ys, k))).abs());
        tot_a += e(xs, k) * e(xs, k);
        tot_b += e(ys, k) * e(ys, k);
    }
    ((dbfs(tot_a.sqrt()) - dbfs(tot_b.sqrt())).abs(), worst)
}

/// **The grain render is the same audio at any callback size, including one
/// that changes from block to block.**
///
/// The grain clock is keyed on absolute sample position, not on a count of
/// callbacks, which is what makes this true - and which is exactly the
/// property `Synth::render` turned out not to have.
///
/// **On the grid and off it**, and the difference is not the grain's. A
/// built-in instrument recomputes its per-chunk work every
/// `GRAIN_GRID` frames off an absolute
/// counter, and one of those per-chunk quantities is a random draw: the
/// oscillators' slow wander, three cents of it, stepped once per chunk.
/// Split a chunk in two and the walk draws twice, which is the same
/// instrument with different dice - a detune of a fraction of a cent, which
/// beats against the other render and shows up in a sample-by-sample
/// comparison as loudly as the note itself.
///
/// Measured, with NO grain render involved at all: an Analog desk fed
/// 17/128/80/33/256-frame callbacks differs from the same desk fed 128s by
/// -14.3 dBFS against a -13.9 peak, and is bit-exact against 64s, 512s or
/// any other sequence of multiples of 32. See `probe_analog_sizes`.
///
/// So the bar is split. Every shape is bit-exact wherever every span ends
/// on the grid, which is every buffer size a device actually hands over and
/// every span the grain render produces - `Renderer::to_grid` sees to the
/// second. Off the grid, audio lanes stay bit-exact and an instrument lane
/// is held to the energy of its notes instead.
#[test]
fn the_grain_survives_a_callback_that_changes_size() {
    struct Shape {
        name: &'static str,
        build: fn() -> Engine,
        samples_compare: bool,
    }
    let shapes = [
        Shape { name: "plate", build: || audio_desk(8, &["@tangent-plate"]), samples_compare: true },
        Shape { name: "comp", build: || audio_desk(8, &["@tangent-comp"]), samples_compare: true },
        Shape {
            name: "Analog",
            build: || midi_pattern_desk(4, "@tangent-analog"),
            samples_compare: false,
        },
    ];
    // `on_grid` is whether every size is a multiple of the instrument chunk.
    let runs: [(&str, &[usize], bool); 6] = [
        ("64 frames", &[64], true),
        ("256 frames", &[256], true),
        ("512 frames", &[512], true),
        ("a size that moves", &[64, 128, 96, 256, 32, 512, 160, 128], true),
        ("a size that moves, off grid", &[64, 128, 96, 256, 32, 512, 48, 128], false),
        ("a size that moves, awkwardly", &[17, 128, 80, 33, 256], false),
    ];
    for Shape { name, build, samples_compare } in shapes {
        for (what, sizes, on_grid) in runs {
            for grain in [240.0f64, 512.0] {
                let (mut chunked, mut grained) = (build(), build());
                grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
                let (mut xs, mut ys) = (Vec::new(), Vec::new());
                render_sizes(&mut chunked, sizes, 160, &mut xs);
                render_sizes(&mut grained, sizes, 160, &mut ys);
                let (worst, first) = worst_diff(&xs, &ys);
                let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
                assert!(peak > 0.001, "{name} with {what}: the render was silent");
                if std::env::var_os("TANGENT_GRAINSURVEY").is_some() {
                    println!(
                        "  {name:<8} {what:<30} grain {grain:>4.0}  worst {:>7.1} dBFS  \
                         peak {:>6.1}  first {:?}",
                        dbfs(worst),
                        dbfs(peak),
                        first.map(|i| i / 2)
                    );
                    continue;
                }
                if samples_compare || on_grid {
                    assert!(
                        worst == 0.0,
                        "{name} at a {grain:.0}-frame grain with {what}: the two \
                         renders differ by {:.1} dBFS, first at frame {:?}",
                        dbfs(worst),
                        first.map(|i| i / 2)
                    );
                    continue;
                }
                // Off the grid, and an instrument: the notes, not the samples.
                let (total, window) = energy_gap(&xs, &ys, peak * 0.001);
                assert!(
                    total < 1.0,
                    "{name} at a {grain:.0}-frame grain with {what}: the two \
                     renders differ by {total:.2} dB of total energy, so a note \
                     is missing or at the wrong level rather than merely drifting"
                );
                assert!(
                    window < 4.0,
                    "{name} at a {grain:.0}-frame grain with {what}: one window's \
                     energy differs by {window:.1} dB, which is more than the \
                     oscillator wander can account for"
                );
            }
        }
    }
}

/// **A clip that STARTS after a silence, on a grained lane.**
///
/// Every equivalence test above plays from frame zero, and a real song does
/// not. `moscow1` is five instrument lanes whose MIDI clips begin at 44.3,
/// 46.8, 48.4, 49.3 and 52.9 seconds, and rendering it with render-ahead on
/// lost **thirteen seconds of quiet playing entirely** - -143 dBFS against
/// the chunked render's -50 - before catching up and matching for the rest
/// of the song. That is the shape, reduced: silence, then a clip.
///
/// Audio lanes and instrument lanes both, because the two reach the fold by
/// different roads - a clip is pulled for the lane's span, an instrument is
/// TOLD about its notes - and only one of them was ever the suspect.
#[test]
fn a_clip_that_starts_after_a_silence_is_not_lost() {
    fn audio_late(starts: &[u64]) -> Engine {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        let clip = noise(12.0);
        let len = clip.frames() as u64;
        e.set_clips(
            starts
                .iter()
                .enumerate()
                .map(|(lane, start)| Placed {
                    clip: Arc::clone(&clip),
                    strip: lane,
                    start: *start,
                    offset: 0,
                    len,
                    fade_in: 0,
                    fade_out: 0,
                    gain: 0.5,
                })
                .collect(),
        );
        seat(&mut e, starts.len(), &["@tangent-plate"]);
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    fn midi_late(starts: &[u64]) -> Engine {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        e.set_midi_clips(
            starts
                .iter()
                .enumerate()
                .map(|(lane, start)| PlacedMidi {
                    strip: lane,
                    start: *start,
                    len: u64::from(RATE) * 60,
                    notes: Arc::new(
                        (0..4)
                            .map(|v| ClipNote {
                                at: (v as u64) * 4000,
                                dur: 12_000,
                                key: 48 + (v as u8) * 5,
                                vel: 100,
                            })
                            .collect(),
                    ),
                    pedal: Arc::new(Vec::new()),
                })
                .collect(),
        );
        seat(&mut e, starts.len(), &["@tangent-analog"]);
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    // Staggered, none on a block boundary and none on a grain boundary:
    // moscow1's are 2.5 seconds apart and arbitrary, and this is that.
    const STARTS: [u64; 5] = [4_211, 7_777, 11_113, 14_401, 20_007];
    let shapes: [(&str, fn() -> Engine, bool); 2] = [
        ("audio clips x plate", || audio_late(&STARTS), true),
        ("Analog, notes from a late clip", || midi_late(&STARTS), false),
    ];
    for (name, build, samples_compare) in shapes {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for _ in 0..400 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                xs.extend_from_slice(&a);
                ys.extend_from_slice(&b);
            }
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let what = format!("{name} at a {grain:.0}-frame grain");
            assert!(peak > 0.001, "{what}: the reference was silent");
            // **The thing that actually went wrong**: the grained render
            // produced nothing at all where the chunked one played.
            let grained_energy: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let chunked_energy: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            assert!(
                grained_energy > chunked_energy * 0.5,
                "{what}: the grained render holds {:.1} dB less energy than the \
                 chunked one, so a clip that starts after a silence is being lost",
                10.0 * (chunked_energy / grained_energy.max(1e-30)).log10()
            );
            if samples_compare {
                let (worst, first) = worst_diff(&xs, &ys);
                assert!(
                    worst == 0.0,
                    "{what}: the two renders differ by {:.1} dBFS, first at frame {:?}",
                    dbfs(worst),
                    first.map(|i| i / 2)
                );
            } else {
                let (total, window) = energy_gap(&xs, &ys, peak * 0.001);
                assert!(
                    total < 1.0 && window < 4.0,
                    "{what}: {total:.2} dB of total energy and {window:.2} dB in \
                     the worst window"
                );
            }
        }
    }
}

/// **A lane with a SEND, grained.**
///
/// The gap that a real song found. `moscow1` renders 1.4 dB louder on one
/// soloed track with render-ahead on than with it off - and the plugins on
/// it are not to blame, because the same track rendered through a 64-frame
/// callback and a 256-frame one is bit-for-bit identical (+0.000 dB). The
/// difference is this feature's, and the desk it appears on has sends and
/// folders while every equivalence test above has neither.
///
/// The send is the suspect because it is the one customer that reads a
/// lane's audio out of a buffer the FOLD fills, rather than out of the
/// lane's own render: `stems_wanted` names the list, and a grained lane's
/// fold runs for a span its render did not.
#[test]
fn a_lane_that_sends_to_the_effects_bus_is_the_same_grained() {
    fn sending_desk(lanes: usize, inserts: &[&str], send: f32) -> Engine {
        let e = audio_desk(lanes, inserts);
        let mut desk = quiet_desk();
        for lane in 0..lanes {
            if let Some(s) = desk.send.get_mut(tangent_ui::recorder::Strip::Channel(lane).index()) {
                *s = send;
            }
        }
        e.set_desk(&desk);
        e.set_fx_return(0.8);
        e
    }
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("clips, half sent", || sending_desk(8, &[], 0.5)),
        ("plate, half sent", || sending_desk(8, &["@tangent-plate"], 0.5)),
        ("comp, fully sent", || sending_desk(8, &["@tangent-comp"], 1.0)),
    ];
    for (name, build) in shapes {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for _ in 0..300 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                xs.extend_from_slice(&a);
                ys.extend_from_slice(&b);
            }
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let what = format!("{name} at a {grain:.0}-frame grain");
            assert!(peak > 0.001, "{what}: the reference was silent");
            let ea: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let eb: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let level = 10.0 * (eb / ea.max(1e-30)).log10();
            assert!(
                level.abs() < 0.2,
                "{what}: the grained render is {level:+.2} dB against the \
                 chunked one, so the send is not folding the same audio"
            );
            let (worst, first) = worst_diff(&xs, &ys);
            assert!(
                worst == 0.0,
                "{what}: the two renders differ by {:.1} dBFS, first at frame {:?}",
                dbfs(worst),
                first.map(|i| i / 2)
            );
        }
    }
}

/// **Delay compensation across a grained desk.**
///
/// The one term in the whole feature that is clocked at CONSUMPTION rather
/// than at render, because `DelayLine::run` advances its write cursor by
/// `frames` unconditionally and cannot be rewound. Step one of the overhaul
/// moved it there; nothing has ever tested it against a desk where lanes
/// have DIFFERENT latencies, which is the only case where compensation
/// does anything at all.
///
/// A convolver is the built-in that reports latency, so half the lanes
/// carry one and half carry nothing: `pdc_max` is then the convolver's and
/// the bare lanes are delayed to meet it. If the grain render clocks that
/// delay line a different number of times, or over a different span, the
/// two halves stop lining up - and two copies of the same music a few
/// samples apart is a comb filter, which shows as a LEVEL change rather
/// than as an obvious fault.
#[test]
fn delay_compensation_survives_a_grained_desk() {
    fn mixed_latency(lanes: usize) -> Engine {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        let clip = noise(12.0);
        let len = clip.frames() as u64;
        e.set_clips(
            (0..lanes)
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
        // Every other lane gets a convolver, so the desk has two latencies
        // and the compensation has something to do.
        let state = ir_state(0.25);
        for lane in (0..lanes).step_by(2) {
            let _ = e.load_insert(lane, 0, Some(Path::new(IR_PATH)), Some(&state));
        }
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    for lanes in [8usize, 16] {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (mixed_latency(lanes), mixed_latency(lanes));
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for _ in 0..300 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                xs.extend_from_slice(&a);
                ys.extend_from_slice(&b);
            }
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let what = format!("{lanes} lanes, every other one compensated, at a {grain:.0}-frame grain");
            assert!(peak > 0.001, "{what}: the reference was silent");
            let ea: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let eb: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let level = 10.0 * (eb / ea.max(1e-30)).log10();
            assert!(
                level.abs() < 0.2,
                "{what}: the grained render is {level:+.2} dB against the chunked \
                 one, which is what two copies of the same music a few samples \
                 apart look like"
            );
            // **Not bit-exact, and it should not be.** A convolver sums an
            // FFT's worth of partitions, and a span cut in different places
            // sums them in a different order: measured at -132.5 dBFS,
            // which is the twenty-fourth bit of a float and not a
            // difference in the audio. The LEVEL above is the assertion
            // that matters, because two copies of the same music a few
            // samples apart comb, and combing shows up there.
            let (worst, _) = worst_diff(&xs, &ys);
            assert!(
                dbfs(worst) < -120.0,
                "{what}: the two renders differ by {:.1} dBFS, which is more \
                 than the order an FFT's partitions are summed in",
                dbfs(worst)
            );
        }
    }
}

/// **A desk with FOLDERS, grained.**
///
/// The last structure a real session has that no equivalence test had.
/// `moscow1` is five instrument tracks inside groups, and it renders 1.4 dB
/// louder on one soloed track with render-ahead on - with the plugins
/// exonerated, because the same track through a 64-frame callback and a
/// 256-frame one is identical to +0.000 dB.
///
/// A folder is pinned, so it renders on the callback's own block; its
/// CHILDREN are not, so they render in grains and are then summed through
/// the folder's rack at consumption. The two halves meet at
/// `run_lane_pdc`'s `folder_lat`, which is read for every lane the fold
/// sums, and the grain is exactly what makes "the span the fold is summing"
/// different from "the span the child rendered".
#[test]
fn a_desk_of_folders_is_the_same_grained() {
    fn foldered(children: usize, inserts: &[&str], folder_insert: Option<&str>) -> Engine {
        // **The folder is lane 0.** `set_folders` refuses a parent whose
        // index is not below its child's, which is the arrangement's own
        // rule: a group sits above the tracks it holds.
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        let clip = noise(12.0);
        let len = clip.frames() as u64;
        e.set_clips(
            (1..=children)
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
        for lane in 1..=children {
            for (bay, path) in inserts.iter().enumerate() {
                e.load_insert(lane, bay, Some(Path::new(path)), None)
                    .unwrap_or_else(|err| panic!("{path} did not load into lane {lane}: {err}"))
                    .unwrap_or_else(|| panic!("{path} loaded but bay {bay} reports no tenant"));
            }
        }
        let mut parents = [-1i32; crate::instrument::CHANNELS];
        for child in 1..=children {
            parents[child] = 0;
        }
        assert!(e.set_folders(parents), "the desk took the folder layout");
        if let Some(unit) = folder_insert {
            seat_one(&mut e, 0, unit);
        }
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    // **Two children, not eight**, in half of these: with fewer lanes than
    // the grain has phases there are callbacks on which NO lane renders at
    // all, and `moscow1` - seven tracks, four phases - has one in every
    // four. Anything the block pass does only when a lane rendered is
    // skipped on those, and a folder's own rack is the obvious candidate.
    let shapes: [(&str, fn() -> Engine); 6] = [
        ("clips in a folder", || foldered(8, &[], None)),
        ("plate in a folder", || foldered(8, &["@tangent-plate"], None)),
        ("clips, folder carries a plate", || foldered(8, &[], Some("@tangent-plate"))),
        ("comp in a folder that comps", || {
            foldered(8, &["@tangent-comp"], Some("@tangent-comp"))
        }),
        ("two children, folder carries a plate", || {
            foldered(2, &[], Some("@tangent-plate"))
        }),
        ("two children with plates, folder comps", || {
            foldered(2, &["@tangent-plate"], Some("@tangent-comp"))
        }),
    ];
    for (name, build) in shapes {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for _ in 0..300 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                xs.extend_from_slice(&a);
                ys.extend_from_slice(&b);
            }
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let what = format!("{name} at a {grain:.0}-frame grain");
            assert!(peak > 0.001, "{what}: the reference was silent");
            let ea: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let eb: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let level = 10.0 * (eb / ea.max(1e-30)).log10();
            assert!(
                level.abs() < 0.2,
                "{what}: the grained render is {level:+.2} dB against the chunked one"
            );
            let (worst, first) = worst_diff(&xs, &ys);
            assert!(
                worst == 0.0,
                "{what}: the two renders differ by {:.1} dBFS, first at frame {:?}",
                dbfs(worst),
                first.map(|i| i / 2)
            );
        }
    }
}

/// **A HOSTED instrument on a grained lane.**
///
/// The path `midi_pattern_desk` does not reach. A built-in is handed its
/// notes as a list and renders them itself; a hosted VST3 is handed MIDI
/// events with SAMPLE OFFSETS inside the block it is about to process, and
/// a grain changes what that block is. Nothing tested it, and a real
/// session found it: `moscow1` - five Rhodes Anthology tracks - renders
/// 1.4 dB louder on one soloed track with render-ahead on, while the same
/// track through a 64-frame callback and a 256-frame one is identical to
/// +0.000 dB, so the plugin is not the one being inconsistent.
///
/// `TANGENT_INST_PLUGIN` names another, `TANGENT_INST_STATE` a sidecar to
/// seat it with. Skipped with a line where the plugin is not installed or
/// will not instantiate in a test process, exactly as the gate rung is.
#[test]
fn a_hosted_instrument_is_the_same_grained() {
    let plugin = std::env::var("TANGENT_INST_PLUGIN")
        .unwrap_or_else(|_| "/Library/Audio/Plug-Ins/VST3/Rhodes Anthology.vst3".to_owned());
    if !Path::new(&plugin).exists() {
        println!("no {plugin}: the hosted-instrument case is not covered here");
        return;
    }
    let state: Option<Vec<u8>> = std::env::var("TANGENT_INST_STATE")
        .ok()
        .filter(|p| !p.is_empty())
        .and_then(|p| std::fs::read(&p).ok())
        .map(|raw| match raw.iter().position(|b| *b == 0) {
            Some(at) if raw[..at].starts_with(b"/") => raw[at + 1..].to_vec(),
            _ => raw,
        });
    const LANES: usize = 3;
    let build = || -> Option<Engine> {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        // Notes at awkward positions, none on a block or grain boundary, and
        // starting after a silence the way a real take does.
        let notes: Arc<Vec<ClipNote>> = Arc::new(
            (0..24)
                .map(|i| ClipNote {
                    at: 3_001 + (i as u64) * 2_311,
                    dur: 5_507,
                    key: 48 + ((i * 5) % 24) as u8,
                    vel: 90,
                })
                .collect(),
        );
        e.set_midi_clips(
            (0..LANES)
                .map(|lane| PlacedMidi {
                    strip: lane,
                    start: 1_777 * (lane as u64 + 1),
                    len: u64::from(RATE) * 60,
                    notes: Arc::clone(&notes),
                    pedal: Arc::new(Vec::new()),
                })
                .collect(),
        );
        for lane in 0..LANES {
            match e.load_insert(lane, 0, Some(Path::new(&plugin)), state.as_deref()) {
                Ok(Some(_)) => {}
                other => {
                    println!("the instrument would not seat in a test process: {other:?}");
                    return None;
                }
            }
        }
        e.set_transport(1, 0);
        e.set_rolling(true);
        Some(e)
    };
    let (Some(mut chunked), Some(mut grained)) = (build(), build()) else {
        return;
    };
    for grain in [256.0f64, 512.0] {
        for e in [&mut chunked, &mut grained] {
            let gen = GENERATION.fetch_add(1, Ordering::Relaxed);
            e.set_transport(gen, 0);
        }
        chunked.set_grain_ms(0.0);
        grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
        let n = BLOCK * 2;
        let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        for _ in 0..500 {
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            xs.extend_from_slice(&a);
            ys.extend_from_slice(&b);
        }
        let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let name = Path::new(&plugin).file_stem().map_or_else(
            || plugin.clone(),
            |s| s.to_string_lossy().into_owned(),
        );
        let what = format!("{name} on {LANES} lanes at a {grain:.0}-frame grain");
        // **Skipped, not failed, unless BOTH engines sounded.**
        //
        // Rhodes Anthology loads its sample set from its STATE, so at its
        // defaults in a test process it may be silent - and it is not
        // reliably silent, because the licence check behind it answers
        // differently from one instance to the next. Two engines are seated
        // here and one has been seen to load while the other did not, which
        // reads as "the grained render lost everything" and is nothing of
        // the kind.
        //
        // Point `TANGENT_INST_STATE` at a sidecar to cover this properly:
        // seated from moscow1's own it measures +0.000 dB at a 256-frame
        // grain and -0.011 at 512.
        let grained_peak = ys.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        if peak <= 0.0005 || grained_peak <= 0.0005 {
            println!(
                "  {what}: one of the two engines did not sound ({:.1} / {:.1} dBFS), \
                 so nothing was compared",
                dbfs(peak),
                dbfs(grained_peak)
            );
            continue;
        }
        let ea: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        let eb: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
        let level = 10.0 * (eb / ea.max(1e-30)).log10();
        println!("  {what}: level {level:+.3} dB, peak {:.1} dBFS", dbfs(peak));
        assert!(
            level.abs() < 0.5,
            "{what}: the grained render is {level:+.2} dB against the chunked \
             one, so a hosted instrument is not being given its notes for the \
             span it is about to render"
        );
    }
    for lane in 0..LANES {
        let _ = chunked.load_insert(lane, 0, None, None);
        let _ = grained.load_insert(lane, 0, None, None);
    }
}

/// **A lane with a TAIL insert, grained.**
///
/// Every lane wears two rack seats: its own, and an extra one past the
/// whole bank (`folder_extra_seat`) that the arrangement calls the tail.
/// `render_lane` runs the first through `run_rack` and the second through
/// `run_extra_rack`, and BOTH gained a position argument when the grain
/// render was built - which makes the tail exactly as able to be handed the
/// wrong span as the head, and it is the one of the two that nothing tested.
///
/// `moscow1` wears one: Brusfri, a noise reducer, on the tail. It is also
/// the project that renders 1.4 dB out with render-ahead on while every
/// synthetic desk built so far renders identically.
#[test]
fn a_lane_with_a_tail_insert_is_the_same_grained() {
    fn tailed(lanes: usize, head: &[&str], tail: Option<&str>) -> Engine {
        tailed_from(lanes, lanes, head, tail)
    }
    /// `with_clips` lanes carry audio; the rest are EMPTY lanes whose only
    /// live rack is the tail seat. `moscow1` has one of those - lane 16,
    /// no clips, a noise reducer on its tail - and the decide walk's own
    /// note says reading only seat `ch` is how such a lane "skipped the
    /// whole block and its second rack never ran at all".
    fn tailed_from(lanes: usize, with_clips: usize, head: &[&str], tail: Option<&str>) -> Engine {
        let mut e = audio_desk(with_clips, head);
        if let Some(unit) = tail {
            for lane in 0..lanes {
                let seat = tangent_ui::recorder::folder_extra_seat(lane);
                e.load_insert(seat, 0, Some(Path::new(unit)), None)
                    .unwrap_or_else(|err| panic!("{unit} did not load into the tail of lane {lane}: {err}"))
                    .unwrap_or_else(|| panic!("{unit} loaded but the tail bay reports no tenant"));
            }
        }
        e
    }
    let shapes: [(&str, fn() -> Engine); 7] = [
        ("clips, tail plate", || tailed(8, &[], Some("@tangent-plate"))),
        ("clips, tail comp", || tailed(8, &[], Some("@tangent-comp"))),
        ("head plate, tail comp", || tailed(8, &["@tangent-plate"], Some("@tangent-comp"))),
        ("head comp, tail drive", || tailed(8, &["@tangent-comp"], Some("@tangent-drive"))),
        ("4 playing, 4 empty with tail plates", || {
            tailed_from(8, 4, &[], Some("@tangent-plate"))
        }),
        ("4 playing, 4 empty with tail comps", || {
            tailed_from(8, 4, &["@tangent-plate"], Some("@tangent-comp"))
        }),
        ("1 playing, 7 empty with tail drives", || {
            tailed_from(8, 1, &[], Some("@tangent-drive"))
        }),
    ];
    for (name, build) in shapes {
        for grain in [240.0f64, 512.0] {
            let (mut chunked, mut grained) = (build(), build());
            grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
            let n = BLOCK * 2;
            let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            for _ in 0..300 {
                chunked.render_offline(&mut a);
                grained.render_offline(&mut b);
                xs.extend_from_slice(&a);
                ys.extend_from_slice(&b);
            }
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let what = format!("{name} at a {grain:.0}-frame grain");
            assert!(peak > 0.001, "{what}: the reference was silent");
            let ea: f64 = xs.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let eb: f64 = ys.iter().map(|v| f64::from(*v) * f64::from(*v)).sum();
            let level = 10.0 * (eb / ea.max(1e-30)).log10();
            assert!(
                level.abs() < 0.2,
                "{what}: the grained render is {level:+.2} dB against the chunked one"
            );
            let (worst, first) = worst_diff(&xs, &ys);
            assert!(
                worst == 0.0,
                "{what}: the two renders differ by {:.1} dBFS, first at frame {:?}",
                dbfs(worst),
                first.map(|i| i / 2)
            );
        }
    }
}

/// **The damper pedal, on a grained lane.**
///
/// The hole every instrument test above left: all of them pass
/// `pedal: Arc::new(Vec::new())`, so the pedal has never once been fed to a
/// lane that renders in grains.
///
/// It is read as a LEVEL - "where does this stand at frame N" - which is
/// what makes it immune to a locate and a loop wrap. A grained lane asks
/// that question **once per grain, at the grain's start**, instead of once
/// per callback at the playhead, and the answer is then applied to the
/// instrument as state for the whole grain. So an edge that lands part way
/// through a grain is late by up to a grain, and an up-and-down inside one
/// is not seen at all.
///
/// The notes here are short and the pedal is held across them, so whether
/// the damper arrived is the difference between a chord that rings and a
/// chord that stops. The no-pedal control is what gives the test teeth.
#[test]
fn the_damper_pedal_reaches_a_grained_lane() {
    fn pedal_desk(edges: &[(u64, u8)]) -> Engine {
        let mut e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
        e.set_armed(0);
        e.set_midi_off(u64::MAX);
        e.set_desk(&quiet_desk());
        // Short notes: without a damper each one stops well before the next.
        let notes: Arc<Vec<ClipNote>> = Arc::new(
            (0..12)
                .map(|i| ClipNote {
                    at: 1_009 + (i as u64) * 2_003,
                    dur: 400,
                    key: 48 + ((i * 7) % 20) as u8,
                    vel: 100,
                })
                .collect(),
        );
        let pedal: Arc<Vec<ClipPedal>> = Arc::new(
            edges.iter().map(|(at, value)| ClipPedal { at: *at, cc: 64, value: *value }).collect(),
        );
        e.set_midi_clips(
            (0..4)
                .map(|lane| PlacedMidi {
                    strip: lane,
                    start: 0,
                    len: u64::from(RATE) * 60,
                    notes: Arc::clone(&notes),
                    pedal: Arc::clone(&pedal),
                })
                .collect(),
        );
        seat(&mut e, 4, &[ANALOG_PATH]);
        e.set_transport(1, 0);
        e.set_rolling(true);
        e
    }
    fn render(e: &mut Engine, blocks: usize) -> Vec<f32> {
        let mut out = vec![0.0f32; BLOCK * 2];
        let mut all = Vec::with_capacity(blocks * BLOCK * 2);
        for _ in 0..blocks {
            e.render_offline(&mut out);
            all.extend_from_slice(&out);
        }
        all
    }
    // Edges at awkward frames: none on a block boundary, none on a grain
    // boundary, and one pair close enough together to fall inside a single
    // grain.
    let held: &[(u64, u8)] = &[(701, 127), (9_001, 0), (11_003, 127), (19_997, 0), (20_101, 127)];
    let none: &[(u64, u8)] = &[];

    let dry = render(&mut pedal_desk(none), 300);
    let wet = render(&mut pedal_desk(held), 300);
    let peak = wet.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    assert!(peak > 0.001, "the pedal desk was silent");
    // **Teeth**: the pedal has to be doing something, or the comparison
    // below is comparing two renders of nothing.
    let (moved, _) = worst_diff(&dry, &wet);
    assert!(
        dbfs(moved) > dbfs(peak) - 20.0,
        "holding the damper changed the sound by only {:.1} dBFS against a \
         peak of {:.1}, so this desk does not hear the pedal and the test \
         below would prove nothing",
        dbfs(moved),
        dbfs(peak)
    );

    // **The control**: the same desk with NO pedal at all has to be
    // bit-exact grained, or the comparison below is measuring the grain
    // render in general rather than the pedal.
    for grain in [240.0f64, 256.0, 512.0] {
        let mut g = pedal_desk(none);
        g.set_grain_ms(1000.0 * grain / f64::from(RATE));
        let zs = render(&mut g, 300);
        let (d, first) = worst_diff(&dry, &zs);
        assert!(
            d == 0.0,
            "the control moved: with NO pedal at all, a {grain:.0}-frame grain \
             differs by {:.1} dBFS at frame {:?}, so what follows is not about \
             the pedal",
            dbfs(d),
            first.map(|i| i / 2)
        );
    }
    for grain in [240.0f64, 256.0, 512.0] {
        let mut grained = pedal_desk(held);
        grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
        let ys = render(&mut grained, 300);
        let (worst, first) = worst_diff(&wet, &ys);
        let (total, window) = energy_gap(&wet, &ys, peak * 0.001);
        if std::env::var_os("TANGENT_GRAINSURVEY").is_some() {
            println!(
                "  {grain:.0}-frame grain: worst {:.1} dBFS, total energy \
                 {total:.3} dB, worst window {window:.2} dB, first at {:?}",
                dbfs(worst),
                first.map(|i| i / 2)
            );
        }
        let _ = (total, window);
        assert!(
            worst == 0.0,
            "at a {grain:.0}-frame grain the damper lands differently: the two \
             renders differ by {:.1} dBFS, first at frame {:?}",
            dbfs(worst),
            first.map(|i| i / 2)
        );
    }
}

/// **Auto picks the grain the measurements picked.**
///
/// The rule is one line - eight callbacks' worth, capped at `MAX_BLOCK` -
/// and the whole of its authority is a table taken on a quiet machine, so
/// the table is written into the assertion rather than left in a document
/// that can drift away from the code.
///
/// 96 hosted reverbs, forked, worst p50 as a share of the deadline:
///
/// | callback | off | 256 frames | 512 frames | auto lands on |
/// |---|---|---|---|---|
/// | 32 | 123.8% | **47.7%** | 62.7% | 256 |
/// | 64 | 83.1% | 46.4% | **36.1%** | 512 |
/// | 128 | 58.2% | 45.6% | **35.5%** | 512 (capped) |
/// | 256 | 40.6% | 40.6% | **35.1%** | 512 (capped) |
#[test]
fn auto_picks_the_grain_the_measurements_picked() {
    let e = Engine::offline(RATE, Timebase::new()).expect("an offline engine needs no device");
    e.set_grain_ms(-1.0);
    for (callback, want) in [(32usize, 256usize), (64, 512), (128, 512), (256, 512), (512, 512)] {
        assert_eq!(
            crate::instrument::grain_for(&e, callback),
            want,
            "auto at a {callback}-frame callback"
        );
    }
    // Off and a fixed setting are untouched by any of it.
    e.set_grain_ms(0.0);
    assert_eq!(crate::instrument::grain_for(&e, 64), 0, "auto leaked into off");
    e.set_grain_ms(1000.0 * 256.0 / f64::from(RATE));
    assert_eq!(
        crate::instrument::grain_for(&e, 64),
        256,
        "a fixed setting stopped being fixed"
    );
}

/// **The fork and the grain, together and apart, are one sound.**
///
/// Two features that each changed how a lane's audio is produced, and until
/// now each was only ever tested against a desk where the other was off:
/// `the_fork_renders_what_the_single_walk_rendered` runs no grain, and every
/// grain test runs on one thread. A live session with render-ahead on runs
/// BOTH, and nothing asserted that the pair agreed with either half.
///
/// All four corners against the same reference, bit for bit. The reference
/// is the plain one - no fork, no grain - because that is what the engine
/// did before either existed and what the golden clips were rendered from.
#[test]
fn the_fork_and_the_grain_are_one_sound() {
    let shapes: [(&str, fn() -> Engine); 4] = [
        ("16 lanes x [EQ, comp, plate]", || {
            audio_desk(16, &["@tangent-eq", "@tangent-comp", "@tangent-plate"])
        }),
        ("16 lanes x drive", || audio_desk(16, &["@tangent-drive"])),
        ("8 lanes x Analog", || instrument_desk(8, "@tangent-analog", 8)),
        ("Analog, a note every 700 frames", || midi_pattern_desk(8, "@tangent-analog")),
    ];
    for (name, build) in shapes {
        // The reference: one thread, one chunk at a time.
        let mut plain = build();
        plain.set_offline_fanout(false);
        let n = BLOCK * 2;
        let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
        let mut want = Vec::new();
        for _ in 0..200 {
            plain.render_offline(&mut a);
            want.extend_from_slice(&a);
        }
        let peak = want.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(peak > 0.001, "{name}: the reference was silent");

        for (what, fork, grain) in [
            ("fork alone", true, 0.0f64),
            ("grain alone", false, 512.0),
            ("fork and grain", true, 512.0),
            ("fork and an awkward grain", true, 240.0),
        ] {
            let mut e = build();
            e.set_offline_fanout(fork);
            if grain > 0.0 {
                e.set_grain_ms(1000.0 * grain / f64::from(RATE));
            }
            let mut got = Vec::new();
            for _ in 0..200 {
                e.render_offline(&mut b);
                got.extend_from_slice(&b);
            }
            let (worst, first) = worst_diff(&want, &got);
            assert!(
                worst == 0.0,
                "{name} with {what}: differs from the plain walk by {:.1} dBFS, \
                 first at frame {:?}",
                dbfs(worst),
                first.map(|i| i / 2)
            );
        }
    }
}

/// **The fork carries HOSTED plugins too.**
///
/// The built-ins are ours and we know they keep no shared state. A VST3 is
/// somebody else's code, and an export that forks is now calling
/// `process()` on ninety-six instances across several threads at once. Each
/// instance is its own and no two threads ever touch one - that is what the
/// lane carve guarantees - but the claim is worth one measurement against a
/// real plugin rather than an argument.
///
/// Skipped with a line where the plugin is not installed, exactly as the
/// gate rung is.
#[test]
fn the_fork_renders_what_the_single_walk_rendered_with_hosted_plugins() {
    let plugin = std::env::var("TANGENT_GATE_PLUGIN")
        .unwrap_or_else(|_| "/Library/Audio/Plug-Ins/VST3/Rev PLATE-140.vst3".to_owned());
    if !Path::new(&plugin).exists() {
        println!("no {plugin}: the hosted fork case is not covered here");
        return;
    }
    let state: Option<Vec<u8>> = std::env::var("TANGENT_GATE_STATE")
        .ok()
        .filter(|p| !p.is_empty())
        .and_then(|p| std::fs::read(&p).ok())
        .map(|raw| match raw.iter().position(|b| *b == 0) {
            Some(at) if raw[..at].starts_with(b"/") => raw[at + 1..].to_vec(),
            _ => raw,
        });
    const LANES: usize = 8;
    let build = || -> Option<Engine> {
        let mut e = audio_desk(LANES, &[]);
        for lane in 0..LANES {
            match e.load_insert(lane, 0, Some(Path::new(&plugin)), state.as_deref()) {
                Ok(Some(_)) => {}
                other => {
                    println!("the plugin would not seat in a test process: {other:?}");
                    return None;
                }
            }
        }
        Some(e)
    };
    // **Three engines, because the plugin has to prove itself first.**
    //
    // A hosted reverb may modulate its delay lines with a noise source, and
    // two instances of one that does will not agree with each other however
    // they are scheduled. Asserting bit-exactness against such a plugin
    // measures the plugin, not the fork. So: two SERIAL engines are the
    // control, and the fork is only held to bit-exactness where the control
    // earned it. Rev PLATE-140 does not - it differs from itself by -26 to
    // -30 dBFS with the first difference landing on a different frame every
    // run, which is what nondeterminism looks like - and FabFilter Pro-R 2
    // does.
    let (Some(mut one), Some(mut two), Some(mut many)) = (build(), build(), build()) else {
        return;
    };
    one.set_offline_fanout(false);
    two.set_offline_fanout(false);
    many.set_offline_fanout(true);
    // And the grain on top, because that is what a live session does.
    many.set_grain_ms(1000.0 * 512.0 / f64::from(RATE));
    let n = BLOCK * 2;
    let (mut a, mut b, mut c) = (vec![0.0f32; n], vec![0.0f32; n], vec![0.0f32; n]);
    let (mut xs, mut ctl, mut ys) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..200 {
        one.render_offline(&mut a);
        two.render_offline(&mut b);
        many.render_offline(&mut c);
        xs.extend_from_slice(&a);
        ctl.extend_from_slice(&b);
        ys.extend_from_slice(&c);
    }
    let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let name = Path::new(&plugin)
        .file_stem()
        .map_or_else(|| plugin.clone(), |s| s.to_string_lossy().into_owned());
    let (control, _) = worst_diff(&xs, &ctl);
    let (worst, first) = worst_diff(&xs, &ys);
    if peak <= 0.0005 {
        println!("  {name}: silent at its defaults, so nothing was compared");
    } else if control > 0.0 {
        // The plugin disagrees with itself on one thread. All the fork can
        // be held to is not making it WORSE.
        println!(
            "  {LANES} x {name} is not deterministic: two serial renders differ \
             by {:.1} dBFS, so the fork is held to that bar and not to bits",
            dbfs(control)
        );
        assert!(
            worst <= control * 4.0,
            "{LANES} x {name}: the forked, grained walk differs by {:.1} dBFS \
             where the plugin's own run-to-run spread is {:.1}",
            dbfs(worst),
            dbfs(control)
        );
    } else {
        assert!(
            worst == 0.0,
            "{LANES} x {name}: two serial renders agree bit for bit, but the \
             forked, grained walk differs by {:.1} dBFS, first at frame {:?}",
            dbfs(worst),
            first.map(|i| i / 2)
        );
    }
    for lane in 0..LANES {
        let _ = one.load_insert(lane, 0, None, None);
        let _ = two.load_insert(lane, 0, None, None);
        let _ = many.load_insert(lane, 0, None, None);
    }
}

/// What a transition did to a pair of engines.
struct Transition {
    /// Bit-exact from frame zero up to the block the change lands on.
    exact_before: bool,
    /// Bit-exact for the whole run, transition included.
    exact_throughout: bool,
    /// Decibels of total energy between the two whole streams.
    total_db: f64,
    /// The worst single window of energy, in decibels.
    window_db: f64,
    /// Peak of the reference stream.
    peak: f32,
    /// Peak of the GRAINED stream over `quiet`, if one was named.
    quiet_peak: f32,
}

/// **Run one desk change against a chunked reference, mid-roll.**
///
/// Both engines are built the same way and given the same change at the
/// same block; the only difference between them is the grain. `apply` is
/// called with `true` at `on_at` and `false` at `off_at`. `quiet` names a
/// range of blocks the GRAINED engine is expected to be silent over.
fn transition(
    build: fn() -> Engine,
    grain: f64,
    apply: fn(&Engine, bool),
    on_at: usize,
    off_at: usize,
    blocks: usize,
    quiet: Option<std::ops::Range<usize>>,
) -> Transition {
    let (mut chunked, mut grained) = (build(), build());
    grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
    let n = BLOCK * 2;
    let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
    let (mut xs, mut ys) = (Vec::new(), Vec::new());
    let (mut exact_before, mut exact_throughout) = (true, true);
    let mut quiet_peak = 0.0f32;
    for block in 0..blocks {
        if block == on_at {
            apply(&chunked, true);
            apply(&grained, true);
        }
        if block == off_at {
            apply(&chunked, false);
            apply(&grained, false);
        }
        chunked.render_offline(&mut a);
        grained.render_offline(&mut b);
        let same = a.iter().zip(&b).all(|(x, y)| x.to_bits() == y.to_bits());
        exact_throughout &= same;
        if block < on_at {
            exact_before &= same;
        }
        if quiet.as_ref().is_some_and(|q| q.contains(&block)) {
            quiet_peak = b.iter().fold(quiet_peak, |m, s| m.max(s.abs()));
        }
        xs.extend_from_slice(&a);
        ys.extend_from_slice(&b);
    }
    let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
    let (total_db, window_db) = energy_gap(&xs, &ys, peak * 0.001);
    Transition { exact_before, exact_throughout, total_db, window_db, peak, quiet_peak }
}

/// **What a transition is allowed to cost**, and why these two numbers.
///
/// Every fallback out of the grain render meets the same irreducible fact:
/// **a grained lane's rack is up to one grain AHEAD of the playhead.** It
/// has rendered audio nobody has heard yet, and a reverb's delay lines and
/// a compressor's envelope cannot be rewound. So a stop, a varispeed
/// change, a stems export or a freeze - each of which suspends or
/// re-purposes the rack - resumes it from a point up to one grain further
/// along than the chunked reference's.
///
/// For a dense reverb that is a PERMANENT decorrelation: once two plates'
/// delay lines differ at all they never reconverge, and every block after
/// the transition compares at -12 to -15 dBFS for ever. It is also
/// completely inaudible, and the measurement says exactly why: **the total
/// energy of the two streams differs by 0.01 to 0.06 dB.** Same tail, same
/// loudness, different noise. The samples are the wrong thing to compare,
/// the way they are for the locate and for a built-in instrument's wander.
///
/// Measured worsts across the five transitions: total 0.06 dB, window
/// 3.21 dB (the stop, where the parked tail decays across a window
/// boundary). The bounds sit outside those so this is a regression net and
/// not a tripwire.
const TRANSITION_TOTAL_DB: f64 = 1.0;
const TRANSITION_WINDOW_DB: f64 = 6.0;

/// **A stopped transport stays silent, and the desk survives coming back.**
///
/// The hazard is specific and it is why `plan_grains` CLEARS the rings
/// when the transport is not rolling rather than draining them: a parked
/// playhead re-reads one position for ever, and a ring that still held a
/// grain covering it would answer, so the desk would PLAY at a stopped
/// transport. That is the one thing a stopped transport must never do.
#[test]
fn a_stopped_transport_stays_silent_and_comes_back_clean() {
    // `tail` is whether the shape carries one: a plate is still sounding
    // for a second after the music stops and a bare clip is not.
    let shapes: [(&str, fn() -> Engine, bool); 4] = [
        ("clips only", || audio_desk(8, &[]), false),
        ("plate", || audio_desk(8, &["@tangent-plate"]), true),
        ("comp", || audio_desk(8, &["@tangent-comp"]), false),
        ("Analog", || instrument_desk(4, "@tangent-analog", 4), true),
    ];
    for (name, build, tail) in shapes {
        for grain in [240.0f64, 512.0] {
            let t = transition(
                build,
                grain,
                |e, on| e.set_rolling(!on),
                100,
                160,
                300,
                // The last twenty parked blocks, by which time anything a
                // rack carried into the stop has decayed.
                Some(140..160),
            );
            let what = format!("{name} at a {grain:.0}-frame grain, stopped at 100 and rolling at 160");
            assert!(t.peak > 0.001, "{what}: the render was silent, so nothing was proved");
            assert!(t.exact_before, "{what}: the two differed BEFORE the stop");
            assert!(
                t.total_db < TRANSITION_TOTAL_DB && t.window_db < TRANSITION_WINDOW_DB,
                "{what}: {:.2} dB of total energy and {:.2} dB in the worst \
                 window, so the stop cost more than a rack's phase",
                t.total_db,
                t.window_db
            );
            if !tail {
                assert!(
                    t.quiet_peak < 1.0e-6,
                    "{what}: the desk was making {} of sound with the transport \
                     STOPPED, which means a grain was served at a parked playhead",
                    t.quiet_peak
                );
            }
        }
    }
}

/// **Varispeed and a stems export fall back to the chunk, mid-roll.**
///
/// Both are early-outs in `plan_grains` and both were asserted by
/// construction rather than by measurement. Varispeed sizes each chunk
/// from a phase accumulator, so the next chunk's length is not knowable
/// and a grain keyed on position cannot be planned; a stems export takes a
/// dry copy at render and reads it at consumption, so a lane served from a
/// grain would hand the tap a copy taken for a different span. Each
/// arrives while the rings are full, and the rings have to go with it.
#[test]
fn varispeed_and_stems_fall_back_to_the_chunk_mid_roll() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    let modes: [(&str, fn(&Engine, bool)); 2] = [
        ("varispeed", |e, on| e.set_varispeed(if on { 0.75 } else { 1.0 })),
        ("stems", |e, on| e.set_stems(on)),
    ];
    for (mode, apply) in modes {
        for (name, build) in shapes {
            let t = transition(build, 512.0, apply, 80, 180, 260, None);
            let what = format!("{name} with {mode} engaged at 80 and released at 180");
            assert!(t.peak > 0.001, "{what}: the render was silent");
            assert!(t.exact_before, "{what}: the two differed BEFORE {mode} was engaged");
            assert!(
                t.total_db < TRANSITION_TOTAL_DB && t.window_db < TRANSITION_WINDOW_DB,
                "{what}: {:.2} dB of total energy and {:.2} dB in the worst window",
                t.total_db,
                t.window_db
            );
        }
    }
}

/// **The pins that arrive mid-roll do not make a rack re-run.**
///
/// `a_lane_that_becomes_pinned_mid_roll_does_not_re_run_its_rack` proves
/// the rule with the insert tap, chosen because it pins a lane and changes
/// no audio, so the measurement is of one thing. These are the pins a
/// person actually applies - arming a track to record on it, punching in,
/// calling it a drum lane, freezing it.
///
/// **Three of the four are bit-exact and the fourth cannot be**, and the
/// difference is the whole rule. Arming, punching and drums leave the
/// lane's rack running, so the drain covers them completely: serve what
/// the ring holds, then go live from where it ends, and not one sample
/// moves. FREEZING stops the rack and starts it again later, and it
/// restarts from wherever the look-ahead had got to - see
/// `TRANSITION_TOTAL_DB`.
#[test]
fn the_pins_that_arrive_mid_roll_do_not_re_run_a_rack() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    // `keeps_the_rack`: whether the lane's rack goes on running under the pin.
    let pins: [(&str, fn(&Engine, bool), bool); 4] = [
        ("armed", |e, on| e.set_armed(if on { 0b11 } else { 0 }), true),
        // A drum lane is folded through the drum bus instead of its own
        // strip, so calling one a drum lane re-points the path its rack
        // feeds. Measured at 0.02 dB of total energy, which is the tail
        // phase and not a re-run, but it is not bit-exact and should not
        // claim to be.
        ("drums", |e, on| e.set_drum_strips(if on { 0b11 } else { 0 }), false),
        (
            "punched",
            |e, on| {
                let mut p = [None; crate::instrument::CHANNELS];
                if on {
                    p[0] = Some(u64::from(BLOCK as u32) * 150);
                    p[1] = Some(u64::from(BLOCK as u32) * 150);
                }
                e.set_punches(p);
            },
            true,
        ),
        ("frozen", |e, on| e.set_frozen(if on { 0b11 } else { 0 }), false),
    ];
    for (pin, apply, keeps_the_rack) in pins {
        for (name, build) in shapes {
            for grain in [240.0f64, 512.0] {
                let t = transition(build, grain, apply, 100, 200, 300, None);
                let what = format!(
                    "{name} at a {grain:.0}-frame grain, two lanes {pin} at 100 and \
                     not at 200"
                );
                assert!(t.peak > 0.001, "{what}: the render was silent");
                assert!(t.exact_before, "{what}: the two differed BEFORE the pin");
                if keeps_the_rack {
                    assert!(
                        t.exact_throughout,
                        "{what}: the mix moved, so the lane re-ran audio its rack \
                         had already been through - {:.2} dB of total energy",
                        t.total_db
                    );
                } else {
                    assert!(
                        t.total_db < TRANSITION_TOTAL_DB && t.window_db < TRANSITION_WINDOW_DB,
                        "{what}: {:.2} dB of total energy and {:.2} dB in the \
                         worst window",
                        t.total_db,
                        t.window_db
                    );
                }
            }
        }
    }
}

#[test]
#[ignore = "a probe"]
fn probe_transitions() {
    let grain = 240.0f64;
    let build = || audio_desk(8, &["@tangent-plate"]);
    let cases: [(&str, fn(&Engine, bool)); 5] = [
        ("frozen", |e, on| e.set_frozen(if on { 0b11 } else { 0 })),
        ("armed", |e, on| e.set_armed(if on { 0b11 } else { 0 })),
        ("stopped", |e, on| e.set_rolling(!on)),
        ("varispeed", |e, on| e.set_varispeed(if on { 0.75 } else { 1.0 })),
        ("stems", |e, on| e.set_stems(on)),
    ];
    for (what, apply) in cases {
        let (mut chunked, mut grained) = (build(), build());
        grained.set_grain_ms(1000.0 * grain / f64::from(RATE));
        let n = BLOCK * 2;
        let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
        let (mut xs, mut ys) = (Vec::new(), Vec::new());
        let mut line = String::new();
        for block in 0..300usize {
            match block {
                100 => {
                    apply(&chunked, true);
                    apply(&grained, true);
                }
                200 => {
                    apply(&chunked, false);
                    apply(&grained, false);
                }
                _ => {}
            }
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            let (d, _) = worst_diff(&a, &b);
            let pa = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            if (98..=106).contains(&block) || (198..=214).contains(&block) {
                line.push_str(&format!("    b{block}: diff {:.1} dBFS  block peak {:.1}\n", dbfs(d), dbfs(pa)));
            }
            xs.extend_from_slice(&a);
            ys.extend_from_slice(&b);
        }
        let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        let (total, window) = energy_gap(&xs, &ys, peak * 0.001);
        println!("  {what:<10} total energy {total:.2} dB, worst window {window:.2} dB, peak {:.1}", dbfs(peak));
        print!("{line}");
    }
}

#[test]
#[ignore = "a probe"]
fn probe_analog_sizes() {
    let fixed: &[usize] = &[128];
    for sizes in [
        &[64usize][..],
        &[512][..],
        &[64, 128, 96, 256, 32, 512, 48, 128][..],
        &[17, 128, 80, 33, 256][..],
        &[32, 64, 96][..],
    ] {
        for (name, build) in [
            ("Analog", (|| midi_pattern_desk(4, "@tangent-analog")) as fn() -> Engine),
            ("plate", || audio_desk(8, &["@tangent-plate"])),
        ] {
            let (mut a, mut b) = (build(), build());
            let (mut xs, mut ys) = (Vec::new(), Vec::new());
            render_sizes(&mut a, fixed, 640, &mut xs);
            render_sizes(&mut b, sizes, 640, &mut ys);
            let (worst, first) = worst_diff(&xs, &ys);
            let peak = xs.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            println!(
                "  NO GRAIN {name:<7} 128 vs {sizes:?}: worst {:>7.1} dBFS peak {:>6.1} first {:?}",
                dbfs(worst),
                dbfs(peak),
                first.map(|i| i / 2)
            );
        }
    }
}

/// **A lane that stops being eligible mid-roll does not re-run its rack.**
///
/// Opening a plugin face aims the insert tap at that lane, and the tap pins
/// the lane: `pinned_lanes` says so on its second line. The tap changes no
/// audio at all - it copies what the bay produced into a ring the UI drains
/// - so aiming it is the cleanest probe there is for a lane leaving the
/// eligible set while the transport rolls. Arming, punching and dragging a
/// send all do the same thing to the mask, and all of them also change what
/// the lane sounds like, which would make the measurement two things at
/// once.
///
/// What must NOT happen is the shape of the catch-up bug: the lane's rack
/// has already rendered forward to the end of its newest grain, and if the
/// lane now renders the chunk at the playhead, that rack is handed audio it
/// has already been through. A reverb's delay lines and a compressor's
/// envelope cannot be rewound, so the only correct answer is to serve the
/// ring until it is empty and go live after it.
#[test]
fn a_lane_that_becomes_pinned_mid_roll_does_not_re_run_its_rack() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    for (name, build) in shapes {
        let (mut chunked, mut grained) = (build(), build());
        grained.set_grain_ms(1000.0 * 512.0 / f64::from(RATE));
        let n = BLOCK * 2;
        let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
        let (mut worst, mut peak) = (0.0f32, 0.0f32);
        let mut at_block = None;
        for block in 0..300usize {
            // A face opened on lane 0, moved to lane 1, then closed.
            let aim = match block {
                60 => Some(Some((0, 0))),
                140 => Some(Some((1, 0))),
                220 => Some(None),
                _ => None,
            };
            if let Some(aim) = aim {
                chunked.set_insert_tap(aim);
                grained.set_insert_tap(aim);
            }
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            let (d, _) = worst_diff(&a, &b);
            if d > worst {
                worst = d;
                at_block = Some(block);
            }
            peak = a.iter().fold(peak, |m, s| m.max(s.abs()));
        }
        assert!(peak > 0.001, "{name}: the render was silent");
        assert!(
            worst == 0.0,
            "{name}: aiming the insert tap at a rolling lane changed the mix \
             by {:.1} dBFS against a peak of {:.1}, worst at block {at_block:?} \
             - the lane re-ran audio its rack had already been through",
            dbfs(worst),
            dbfs(peak)
        );
    }
}

/// **The knob may be turned while the transport rolls.**
///
/// It is in the audio setup dialog, which is openable mid-session, so this
/// is not a hypothetical. Turning it DOWN or OFF is the interesting
/// direction and it is the same defect shape as the test above: every lane
/// stops being grained at once while every ring still holds audio the racks
/// have already produced.
#[test]
fn the_grain_knob_may_be_turned_while_the_transport_rolls() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("plate", || audio_desk(8, &["@tangent-plate"])),
        ("comp", || audio_desk(8, &["@tangent-comp"])),
        ("drive", || audio_desk(8, &["@tangent-drive"])),
    ];
    // Off, the two knob settings that straddle a block, and back to off.
    let turns: [(usize, f64); 5] =
        [(40, 512.0), (90, 240.0), (140, 0.0), (190, 512.0), (240, 120.0)];
    for (name, build) in shapes {
        let (mut chunked, mut grained) = (build(), build());
        let n = BLOCK * 2;
        let (mut a, mut b) = (vec![0.0f32; n], vec![0.0f32; n]);
        let (mut worst, mut peak) = (0.0f32, 0.0f32);
        let mut at_block = None;
        for block in 0..300usize {
            if let Some((_, frames)) = turns.iter().find(|(b, _)| *b == block) {
                grained.set_grain_ms(1000.0 * frames / f64::from(RATE));
            }
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            let (d, _) = worst_diff(&a, &b);
            if d > worst {
                worst = d;
                at_block = Some(block);
            }
            peak = a.iter().fold(peak, |m, s| m.max(s.abs()));
        }
        assert!(peak > 0.001, "{name}: the render was silent");
        assert!(
            worst == 0.0,
            "{name}: turning the grain knob mid-roll changed the mix by \
             {:.1} dBFS against a peak of {:.1}, worst at block {at_block:?}",
            dbfs(worst),
            dbfs(peak)
        );
    }
}

/// **The grain render's A/B, paired, for a machine that is not quiet.**
///
/// The rung rows in `rt_bench` are absolute microseconds, and an absolute
/// microsecond is only worth having on an idle machine. This was written
/// after a sweep taken during a video call, where the row the grain cannot
/// touch (512 frames, which is `MAX_BLOCK`, so the grain can never exceed
/// the callback) held to 2% across four runs while the 128-frame row moved
/// 37%. Between-run drift of that size buries the effect.
///
/// So this measures the conditions INTERLEAVED: within one repeat, every
/// grain setting is timed back to back on the same engine, and the ratio is
/// taken inside the repeat before anything is averaged across repeats.
/// Whatever the rest of the machine was doing, it was doing it to both
/// sides of each ratio.
///
/// Doors, all optional: `TANGENT_GATE_PLUGIN` and `TANGENT_GATE_STATE` as
/// the rung uses them, `TANGENT_AB_FRAMES` (the callback, default 64),
/// `TANGENT_AB_GRAINS` (frames, comma separated, default `0,240,512`),
/// `TANGENT_AB_REPEATS` (default 7) and `TANGENT_AB_BLOCKS` (default 400).
#[test]
#[ignore = "a bench: run it deliberately with --ignored --nocapture"]
fn grain_ab() {
    let env_num = |k: &str, d: usize| -> usize {
        std::env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(d)
    };
    let frames = env_num("TANGENT_AB_FRAMES", 64);
    let repeats = env_num("TANGENT_AB_REPEATS", 7);
    let blocks = env_num("TANGENT_AB_BLOCKS", 400);
    // **A loop, so the wrap's catch-up is IN the figure.** A wrap empties
    // every lane's ring, so the block after one is a full-desk catch-up:
    // the one cost this feature pays that the straight-through rows do not
    // show. `TANGENT_AB_LOOP=96000` is a two-second loop at 48 kHz.
    let loop_frames = env_num("TANGENT_AB_LOOP", 0);
    // **`-1` is AUTO**, the setting the engine works out per callback. It
    // has to come through as a signed value or it is silently dropped and
    // the row simply does not appear, which is how the first run of this
    // reported three conditions and looked entirely normal.
    let grains: Vec<i64> = std::env::var("TANGENT_AB_GRAINS")
        .unwrap_or_else(|_| "0,240,512".to_owned())
        .split(',')
        .filter_map(|v| v.trim().parse().ok())
        .collect();
    let plugin = std::env::var("TANGENT_GATE_PLUGIN")
        .unwrap_or_else(|_| "/Library/Audio/Plug-Ins/VST3/Rev PLATE-140.vst3".to_owned());
    let state: Option<Vec<u8>> = std::env::var("TANGENT_GATE_STATE")
        .ok()
        .filter(|p| !p.is_empty())
        .and_then(|p| std::fs::read(&p).ok())
        .map(|raw| match raw.iter().position(|b| *b == 0) {
            Some(at) if raw[..at].starts_with(b"/") => raw[at + 1..].to_vec(),
            _ => raw,
        });
    if !Path::new(&plugin).exists() {
        println!("no {plugin}: nothing to A/B");
        return;
    }
    let mut e = audio_desk(32, &[]);
    for lane in 0..32 {
        for bay in 0..3 {
            match e.load_insert(lane, bay, Some(Path::new(&plugin)), state.as_deref()) {
                Ok(Some(_)) => {}
                other => {
                    println!("the A/B plugin would not seat: {other:?}");
                    return;
                }
            }
        }
    }
    let name = Path::new(&plugin)
        .file_stem()
        .map_or_else(|| plugin.clone(), |s| s.to_string_lossy().into_owned());
    if loop_frames > 0 {
        e.set_loop(0, loop_frames as u64);
    }
    let deadline = deadline_us(frames);
    println!(
        "\n96 bays x {name}, {frames}-frame callback, deadline {deadline:.0} us\n\
         {repeats} repeats x {blocks} blocks, conditions interleaved within each repeat{}\n",
        if loop_frames > 0 {
            format!(", looping 0..{loop_frames}")
        } else {
            String::new()
        }
    );
    // [grain][repeat] -> p50 us
    let mut p50s: Vec<Vec<f64>> = vec![Vec::new(); grains.len()];
    let mut overs: Vec<usize> = vec![0; grains.len()];
    // [grain][repeat] -> process CPU seconds for `blocks` blocks.
    let mut cpu: Vec<Vec<f64>> = vec![Vec::new(); grains.len()];
    let mut out = vec![0.0f32; frames * 2];
    for _ in 0..repeats {
        for (gi, g) in grains.iter().enumerate() {
            e.set_grain_ms(match *g {
                0 => 0.0,
                g if g < 0 => -1.0,
                g => 1000.0 * g as f64 / f64::from(RATE),
            });
            let generation = GENERATION.fetch_add(1, Ordering::Relaxed);
            e.set_transport(generation, 0);
            for _ in 0..200 {
                e.render_offline(&mut out);
            }
            // **Process CPU across the window, as well as the wall clock.**
            // Wall is what a deadline is made of and is the first thing a
            // busy machine ruins; CPU seconds to render a fixed amount of
            // audio is throughput, survives contention, and is the figure
            // another host's render can be put beside - REAPER reports
            // exactly this for an offline render.
            let cpu0 = crate::instrument::cpu_seconds();
            let mut us = Vec::with_capacity(blocks);
            for _ in 0..blocks {
                let t = Instant::now();
                e.render_offline(&mut out);
                us.push(t.elapsed().as_nanos() as f64 / 1000.0);
            }
            if let (Some(a), Some(b)) = (cpu0, crate::instrument::cpu_seconds()) {
                cpu[gi].push(b - a);
            }
            overs[gi] += us.iter().filter(|v| **v > deadline).count();
            us.sort_by(|a, b| a.partial_cmp(b).expect("no NaN from a duration"));
            p50s[gi].push(us[us.len() / 2]);
        }
    }
    let med = |mut v: Vec<f64>| -> f64 {
        v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN"));
        v[v.len() / 2]
    };
    println!(
        "{:<12} {:>10} {:>10} {:>12} {:>13} {:>8}",
        "grain", "p50 us", "% of dl", "wall / 12 s", "CPU / 12 s", "over"
    );
    for (gi, g) in grains.iter().enumerate() {
        let p50 = med(p50s[gi].clone());
        // Blocks in twelve seconds of audio, at this callback size.
        let per_12 = 12.0 * f64::from(RATE) / frames as f64;
        let cpu_12 = if cpu[gi].is_empty() {
            f64::NAN
        } else {
            med(cpu[gi].clone()) * per_12 / blocks as f64
        };
        println!(
            "{:<12} {p50:>10.1} {:>9.1}% {:>11.2}s {:>12.2}s {:>8}",
            match *g {
                0 => "off".to_owned(),
                g if g < 0 => format!("auto ({} fr)", crate::instrument::grain_for(&e, frames)),
                g => format!("{g} frames"),
            },
            p50 / deadline * 100.0,
            p50 * per_12 / 1.0e6,
            cpu_12,
            overs[gi],
        );
    }
    // **The ratio taken INSIDE each repeat**, which is the whole point.
    if let Some(base) = p50s.first() {
        for (gi, g) in grains.iter().enumerate().skip(1) {
            let ratios: Vec<f64> =
                base.iter().zip(&p50s[gi]).map(|(off, on)| off / on).collect();
            let lo = ratios.iter().copied().fold(f64::INFINITY, f64::min);
            let hi = ratios.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            println!(
                "  {} is {:.2}x the throughput of off (per-repeat range {lo:.2} to {hi:.2})",
                if *g < 0 { "auto".to_owned() } else { format!("{g} frames") },
                med(ratios.clone()),
            );
        }
    }
    for lane in 0..32 {
        for bay in 0..3 {
            let _ = e.load_insert(lane, bay, None, None);
        }
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

    // **A desk where most lanes have nothing to play**, which is what a
    // song looks like for most of its length. Ten of the sixteen lanes'
    // clips sit past the end of the measurement, so those lanes have a rack
    // and nothing under the playhead.
    let mut e = audio_desk(16, &[EQ_PATH, COMP_PATH, PLATE_PATH]);
    {
        let clip = noise(12.0);
        let len = clip.frames() as u64;
        // Six lanes play from zero; the rest start a minute in.
        let clips: Vec<Placed> = (0..16)
            .map(|lane| Placed {
                clip: Arc::clone(&clip),
                strip: lane,
                start: if lane < 6 { 0 } else { u64::from(RATE) * 60 },
                offset: 0,
                len,
                fade_in: 0,
                fade_out: 0,
                gain: 0.5,
            })
            .collect();
        e.set_clips(clips);
    }
    // **Warmed past the park window**, which is eight seconds of a lane
    // making nothing: at 128 frames that is three thousand blocks, and the
    // ordinary four hundred would time a desk whose idle lanes have not
    // been idle long enough to rest yet.
    rows.push(measure(
        "16 lanes x [EQ, comp, plate], 10 with nothing to play",
        &mut e,
        3_600,
        BLOCK,
    ));
    drop(e);

    // The two that recompute a filter pole per sample, or did.
    let mut e = audio_desk(16, &[CHORUS_PATH]);
    rows.push(measure("16 lanes x Chorus", &mut e, WARMUP, BLOCK));
    drop(e);
    let mut e = audio_desk(16, &[ECHO_PATH]);
    rows.push(measure("16 lanes x Echo", &mut e, WARMUP, BLOCK));
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

    // ── the convolver, which is the bay a file makes expensive ────────────
    //
    // Read the p99 here, not the mean. A convolver works in bursts: it does
    // nothing at all for three blocks out of four at this block size and
    // then transforms, so its mean is a quarter of what any one block
    // actually costs, and the mean is not what glitches.
    let mut e = ir_desk(16, 1, 2.0);
    rows.push(measure("16 lanes x IR, 2 s response", &mut e, WARMUP, BLOCK));
    drop(e);
    // **Ninety-six bays**: the rung the plan's acceptance gate names, at
    // roughly what a commercial reverb costs a bay on this machine.
    let mut e = ir_desk(32, 3, 0.5);
    rows.push(measure("32 lanes x 3 IR, 0.5 s response", &mut e, WARMUP, BLOCK));
    rows.push(measure("32 lanes x 3 IR, 0.5 s @ 64 fr", &mut e, WARMUP, 64));
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

    // ── THE GATE RUNG: 96 instances of a commercial reverb ────────────
    //
    // **This is the shape the plan's own acceptance clause names**, and the
    // reason it is worth its own rows is that the IR rows above are NOT it.
    // A convolver is bursty: it costs nothing for three blocks and
    // everything on the fourth. A plugin like this costs the same amount on
    // every block. Two desks with the same mean can therefore have very
    // different p99s, and the p99 is what the gate is written against, so a
    // gate scored on the bursty desk is not the gate.
    //
    // Thirty-two lanes of three bays. Skipped, with a line, wherever the
    // plugin is not installed or will not instantiate in a test process.
    //
    // **`TANGENT_GATE_PLUGIN` names another one**, and `TANGENT_GATE_STATE` a
    // sidecar to seat it with. Both exist for the head-to-head against
    // another host: a comparison is only worth having if BOTH sides run the
    // same plugin at the same settings, and the other host cannot be asked to
    // run this one at ITS defaults.
    let gate_plugin = std::env::var("TANGENT_GATE_PLUGIN")
        .unwrap_or_else(|_| "/Library/Audio/Plug-Ins/VST3/Rev PLATE-140.vst3".to_owned());
    let gate_state: Option<Vec<u8>> = std::env::var("TANGENT_GATE_STATE")
        .ok()
        .filter(|p| !p.is_empty())
        .and_then(|p| std::fs::read(&p).ok())
        .map(|raw| match raw.iter().position(|b| *b == 0) {
            // A Tangent sidecar is `<bundle path>\0<container>`; anything
            // else is handed over whole.
            Some(at) if raw[..at].starts_with(b"/") => raw[at + 1..].to_vec(),
            _ => raw,
        });
    if Path::new(&gate_plugin).exists() {
        let mut e = audio_desk(32, &[]);
        let mut ok = true;
        'seat: for lane in 0..32 {
            for bay in 0..3 {
                match e.load_insert(
                    lane,
                    bay,
                    Some(Path::new(&gate_plugin)),
                    gate_state.as_deref(),
                ) {
                    Ok(Some(_)) => {}
                    Ok(None) => {
                        println!("note: the gate plugin loaded but lane {lane} bay {bay} reports no tenant");
                        ok = false;
                        break 'seat;
                    }
                    Err(err) => {
                        println!("note: the gate plugin would not load in a test process: {err}");
                        ok = false;
                        break 'seat;
                    }
                }
            }
        }
        if ok {
            let name = Path::new(&gate_plugin)
                .file_stem()
                .map_or_else(|| gate_plugin.to_owned(), |s| s.to_string_lossy().into_owned());
            rows.push(measure(&format!("96 bays x {name} (VST3)"), &mut e, WARMUP, BLOCK));
            rows.push(measure(&format!("96 bays x {name} (VST3) @ 64 fr"), &mut e, WARMUP, 64));
            // **The same desk at bigger grains**, which is not a deadline
            // question at all: it is how much of this shape's cost is PER
            // BLOCK rather than per frame. A host that renders offline
            // chooses its own grain and pays the per-block part far less
            // often, so the throughput gap between this engine and another
            // one is mostly this number. See `docs/PERF-REAPER-2026-09-18.md`.
            rows.push(measure(&format!("96 bays x {name} (VST3) @ 256 fr"), &mut e, WARMUP, 256));
            rows.push(measure(&format!("96 bays x {name} (VST3) @ 512 fr"), &mut e, WARMUP, 512));
        }
        // Emptied by hand before the engine drops, so the plugins are torn
        // down on this thread rather than with the renderer.
        for lane in 0..32 {
            for bay in 0..3 {
                let _ = e.load_insert(lane, bay, None, None);
            }
        }
        drop(e);
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
