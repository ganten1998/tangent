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

/// **A grain render is the same audio, arriving earlier.**
///
/// The one property that stands between the grain render and wrong sound.
/// A lane eligible for it is rendered in 512-frame grains staggered across
/// the desk instead of once per callback, so on most callbacks its plugins
/// are not called at all and the fold reads a span out of a ring that was
/// filled some blocks ago. Every part of that is new: the ring's key, the
/// span gather across a grain boundary, the stagger's phase, the catch-up
/// after a locate, and `run_lane_pdc` clocking a delay line over a staging
/// pair rather than over the render's own buffer.
///
/// **Asserted BIT-EXACT**, which is available here and would not be for
/// every desk: these shapes are sample-by-sample arithmetic - a clip pull,
/// a fader, a biquad, a compressor - so the block size they are cut into
/// cannot change the answer. It is not true of a plugin that blocks
/// internally, which is why the shapes are chosen and not arbitrary: a
/// hosted convolver partitions its own input and a denoiser windows it, and
/// both would differ for a reason that is not a defect.
///
/// A locate is driven half way through, because the catch-up path is the
/// one a cold ring takes and it would otherwise never run here.
///
/// Not `#[ignore]`d: it is a second, and it is the test that would catch a
/// grain served for the wrong position, which is the failure this whole
/// change risks.
#[test]
fn a_grained_lane_renders_what_the_chunked_lane_rendered() {
    let shapes: [(&str, fn() -> Engine); 3] = [
        ("16 lanes, clips only", || audio_desk(16, &[])),
        ("16 lanes x 3 EQ", || audio_desk(16, &["@tangent-eq", "@tangent-eq", "@tangent-eq"])),
        ("16 lanes x [EQ, comp]", || audio_desk(16, &["@tangent-eq", "@tangent-comp"])),
    ];
    for (name, build) in shapes {
        let mut chunked = build();
        let mut grained = build();
        // 512 frames at 48 kHz, against a 128-frame block: eight callbacks
        // to a grain, so every lane spends most of them rendering nothing.
        grained.set_grain_ms(1000.0 * 512.0 / f64::from(RATE));
        let n = BLOCK * 2;
        let mut a = vec![0.0f32; n];
        let mut b = vec![0.0f32; n];
        for block in 0..200 {
            // Half way through, a locate: the ring goes cold and both
            // engines must come back on the same frame.
            chunked.render_offline(&mut a);
            grained.render_offline(&mut b);
            for (i, (x, y)) in a.iter().zip(&b).enumerate() {
                assert!(
                    x.to_bits() == y.to_bits(),
                    "{name}: block {block} sample {i}: chunked gave {x}, grained gave {y}"
                );
            }
        }
        // ── and now a locate, which is the one irreducible discard ───────
        //
        // A grained lane has rendered up to a grain PAST the playhead, and a
        // locate abandons that audio. The ring is emptied, but the stateful
        // units in the lane's rack cannot be: a compressor's envelope and a
        // reverb's delay lines have processed samples nobody will hear, and
        // there is no rewind for either. This is the cost the design admits,
        // and it lands at a locate, where a discontinuity already lives.
        //
        // The numbers below are MEASURED, not chosen. On this desk the
        // difference starts at -53.7 dBFS and is down to -72.6 dBFS a fifth
        // of a second later. It decays toward zero without reaching it: the
        // two envelopes differ by a factor that shrinks exponentially, and
        // in f32 that approaches equality asymptotically rather than landing
        // on it. The bounds are set outside the measurements so this is a
        // regression net and not a tripwire.
        let at = 7 * BLOCK as u64 + 13;
        for e in [&mut chunked, &mut grained] {
            e.set_transport(2, at);
        }
        let (mut worst, mut tail) = (0.0f32, 0.0f32);
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
        }
        let db = |v: f32| 20.0 * v.max(1e-12).log10();
        assert!(
            db(worst) < -40.0,
            "{name}: a locate cost {:.1} dBFS, which is audible against the \
             discontinuity a locate already has",
            db(worst)
        );
        assert!(
            db(tail) < -65.0,
            "{name}: a fifth of a second after the locate the two renders \
             still differ by {:.1} dBFS, so the discarded grain is not \
             decaying out of the lane the way a released envelope should",
            db(tail)
        );
        let peak = a.iter().fold(0.0f32, |m, s| m.max(s.abs()));
        assert!(
            peak > 0.001,
            "{name}: the reference render was silent, so the test proved nothing"
        );
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
