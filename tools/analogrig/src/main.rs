//! The Tangent Analog bench.
//!
//! Renders a scripted performance through the REAL module and writes a WAV.
//! No audio device, no DAW, no GUI: a change to the model is audible and
//! measurable about a second after it is typed.
//!
//!   cargo run --release -- <performance> [out.wav]
//!   performances: single velramp chord glide arp unison sweep restrike
//!                 steal
//!   cargo run --release -- report      every gate in the brief's §9, as
//!                                      numbers, plus what each stage costs
//!   cargo run --release -- perf        the per-stage costs alone
//!
//! `ANALOGRIG_PARAMS="31=0.5,32=0.9"` turns dials by id without a UI.

// Relative to THIS FILE, which is how `#[path]` resolves: the bench lives
// in tools/analogrig and reaches over to the one true copy of the DSP.
#[path = "../../../tangent/src/analog/mod.rs"]
#[allow(dead_code)]
mod analog;
mod units;
mod wav;

use analog::arp::Transport;
use analog::measure;
use analog::{Analog, Event};

const RATE: f32 = 48_000.0;
/// The same block size the audio callback uses, so a bug that only happens
/// at a block boundary happens here too.
const BLOCK: usize = 128;

#[derive(Clone, Copy)]
enum Ev {
    On(u8, u8),
    Off(u8),
    Pedal(bool),
    Param(u32, f32),
}

struct Score {
    /// (seconds, event)
    events: Vec<(f32, Ev)>,
    /// How long to keep rendering after the last event.
    tail: f32,
    /// Dials set before the first note, by id.
    setup: Vec<(u32, f32)>,
}

impl Score {
    fn len_secs(&self) -> f32 {
        self.events.iter().map(|e| e.0).fold(0.0, f32::max) + self.tail
    }
}

fn note(ev: &mut Vec<(f32, Ev)>, t: f32, dur: f32, pitch: u8, vel: u8) {
    ev.push((t, Ev::On(pitch, vel)));
    ev.push((t + dur, Ev::Off(pitch)));
}

fn chord(ev: &mut Vec<(f32, Ev)>, t: f32, dur: f32, pitches: &[u8], vel: u8) {
    // Real hands are not simultaneous.
    for (i, &p) in pitches.iter().enumerate() {
        note(ev, t + i as f32 * 0.009, dur, p, vel);
    }
}

use analog::*;

fn build(name: &str) -> Score {
    let mut ev = Vec::new();
    let mut setup = Vec::new();
    match name {
        // One note, held: the shipped voicing, for measuring.
        "single" => {
            note(&mut ev, 0.0, 2.0, 60, 100);
            Score { events: ev, tail: 2.0, setup }
        }
        // The velocity ladder: the same note, twelve times, soft to hard.
        "velramp" => {
            let mut t = 0.0;
            for i in 0..12 {
                let v = (8 + i * 11).min(127) as u8;
                note(&mut ev, t, 0.6, 64, v);
                t += 0.8;
            }
            Score { events: ev, tail: 1.5, setup }
        }
        // An eight-voice forte chord: the level gate.
        "chord" => {
            chord(&mut ev, 0.0, 1.5, &[48, 55, 60, 64, 67, 71, 74, 79], 127);
            Score { events: ev, tail: 1.5, setup }
        }
        // A mono line with the glide up.
        "glide" => {
            setup.push((P_VOICE_MODE as u32, 0.0));
            setup.push((P_GLIDE as u32, 0.5));
            let line = [48u8, 55, 60, 67, 72, 67, 60, 55];
            for (i, &p) in line.iter().enumerate() {
                note(&mut ev, i as f32 * 0.4, 0.45, p, 100);
            }
            Score { events: ev, tail: 1.5, setup }
        }
        // A held chord through the arpeggiator, up, two octaves.
        "arp" => {
            setup.push((P_ARP_MODE as u32, 0.2));
            setup.push((P_ARP_OCT as u32, 1.0 / 3.0));
            setup.push((P_A_SUSTAIN as u32, 0.2));
            setup.push((P_A_DECAY as u32, 0.3));
            chord(&mut ev, 0.0, 3.0, &[48, 52, 55, 59], 100);
            Score { events: ev, tail: 1.0, setup }
        }
        // Eight voices on one key, detuned and spread: the unison side.
        "unison" => {
            setup.push((P_VOICE_MODE as u32, 0.5));
            setup.push((P_DETUNE as u32, 0.4));
            note(&mut ev, 0.0, 2.0, 48, 100);
            note(&mut ev, 2.2, 2.0, 55, 100);
            Score { events: ev, tail: 1.5, setup }
        }
        // A filter sweep, drawn as automation, at several emphases.
        "sweep" => {
            setup.push((P_CONTOUR as u32, 0.0));
            setup.push((P_CUTOFF as u32, 0.1));
            for (k, emph) in [0.0_f32, 0.5, 0.8, 0.97].iter().enumerate() {
                let t0 = k as f32 * 2.5;
                ev.push((t0, Ev::Param(P_EMPHASIS as u32, *emph)));
                note(&mut ev, t0, 2.2, 36, 100);
                for i in 0..=40 {
                    let t = t0 + 0.1 + 2.0 * i as f32 / 40.0;
                    ev.push((t, Ev::Param(P_CUTOFF as u32, 0.1 + 0.85 * i as f32 / 40.0)));
                }
            }
            Score { events: ev, tail: 1.0, setup }
        }
        // A note restruck under the pedal while its release rings.
        "restrike" => {
            setup.push((P_A_RELEASE as u32, 0.6));
            ev.push((0.0, Ev::Pedal(true)));
            for i in 0..6 {
                note(&mut ev, i as f32 * 0.25, 0.1, 60, 100);
            }
            Score { events: ev, tail: 2.0, setup }
        }
        // Nine keys into eight voices: the steal and its corpse.
        "steal" => {
            ev.push((0.0, Ev::Pedal(true)));
            chord(&mut ev, 0.0, 1.5, &[48, 50, 52, 53, 55, 57, 59, 60], 100);
            note(&mut ev, 0.4, 1.0, 72, 110);
            Score { events: ev, tail: 1.5, setup }
        }
        other => panic!("no performance called {other}"),
    }
}

/// Render a score through a fresh instrument. Returns interleaved stereo
/// and the wall-clock time the render took.
fn render(score: &Score) -> (Vec<f32>, std::time::Duration) {
    let mut events = score.events.clone();
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut synth = Analog::new(RATE);
    synth.set_all(&score.setup);
    if let Ok(spec) = std::env::var("ANALOGRIG_PARAMS") {
        let mut extra = Vec::new();
        for part in spec.split(',') {
            if let Some((i, v)) = part.split_once('=') {
                if let (Ok(i), Ok(v)) = (i.trim().parse::<u32>(), v.trim().parse::<f32>()) {
                    extra.push((i, v));
                }
            }
        }
        synth.set_all(&extra);
    }

    let total = ((score.len_secs() * RATE) as usize / BLOCK + 1) * BLOCK;
    let mut out = vec![0.0_f32; total * 2];
    let mut block = vec![0.0_f32; BLOCK * 2];
    let mut block_events: Vec<Event> = Vec::new();

    let mut next = 0usize;
    let mut frame = 0usize;
    let t0 = std::time::Instant::now();
    while frame < total {
        block_events.clear();
        let block_end = (frame + BLOCK) as f32 / RATE;
        while next < events.len() && events[next].0 < block_end {
            let at = ((events[next].0 * RATE) as usize).saturating_sub(frame).min(BLOCK - 1);
            block_events.push(match events[next].1 {
                Ev::On(p, v) => Event::NoteOn { at, key: p, vel: f32::from(v) / 127.0 },
                Ev::Off(p) => Event::NoteOff { at, key: p },
                Ev::Pedal(d) => Event::Sustain { at, down: d },
                Ev::Param(id, v) => Event::Param { at, id, value: v },
            });
            next += 1;
        }
        synth.render(&mut block, BLOCK, 2, &block_events, Transport::default());
        out[frame * 2..(frame + BLOCK) * 2].copy_from_slice(&block);
        frame += BLOCK;
    }
    (out, t0.elapsed())
}

/// The per-stage costs: what one voice-sample of each thing takes, so the
/// CPU number can be argued with rather than about. Inputs are
/// precomputed so the clock reads the stage and not a sine call.
fn perf() {
    use analog::filter::{tpt_g, Cutoff, Ladder, Oversampler, Sem, Taps};
    use analog::osc::{Osc, Wave};
    use analog::voice::Voice;
    let n = 1_000_000usize;
    let sig: Vec<f32> = (0..n).map(|i| (i as f32 * 0.131).sin() * 0.5).collect();
    let sig4: Vec<f32> = (0..n * 4).map(|i| (i as f32 * 0.0327).sin() * 0.5).collect();
    let time = |label: &str, mut f: Box<dyn FnMut(usize) -> f32>| {
        let t0 = std::time::Instant::now();
        let mut acc = 0.0_f32;
        for i in 0..n {
            acc += f(i);
        }
        let ns = t0.elapsed().as_secs_f64() * 1e9 / n as f64;
        println!("  {label:<38} {ns:7.1} ns   (acc {acc:.3e})");
    };
    println!("per-sample costs, one voice's worth of each stage:");
    let mut o = Osc::new(1);
    time("saw oscillator, C4", Box::new(move |_| o.tick(Wave::Saw, 261.6 / RATE, 0.0, None)));
    let mut o = Osc::new(2);
    time("narrow pulse, C7", Box::new(move |_| o.tick(Wave::Narrow, 2093.0 / RATE, 0.0, None)));
    let taps = Taps::new();
    let mut os = Oversampler::new();
    let s1 = sig.clone();
    time(
        "4x pair, up and down",
        Box::new(move |i| {
            let u = os.up(&taps, s1[i]);
            os.down(&taps, u)
        }),
    );
    let g = Cutoff::new(2_000.0, 4.0 * RATE, 0.5);
    for (label, k) in [("ladder, 4 substeps, emphasis 0.5", 2.2_f32), ("ladder, 4 substeps, emphasis 1.0", 4.4)] {
        let mut l = Ladder::new();
        let s4 = sig4.clone();
        time(label, Box::new(move |i| {
            let mut y = 0.0;
            for j in 0..4 {
                y = l.tick(s4[i * 4 + j], &g, k);
            }
            y
        }));
    }
    let mut sm = Sem::new();
    let s4 = sig4.clone();
    time("SEM, 4 substeps", Box::new(move |i| {
        let mut y = 0.0;
        for j in 0..4 {
            y = sm.tick(s4[i * 4 + j], &g, 0.5, 0.0);
        }
        y
    }));
    // The filter across sixteen voices: scalar, interleaved as the frame
    // does it, against lanes of 4, 8 and 16 — the same arithmetic in a
    // data layout the compiler vectorises. Per voice-sample, 4 substeps.
    {
        use analog::filter::{CutoffLanes, LadderLanes, SemLanes};
        let n = 200_000usize;
        let s4: Vec<f32> = (0..n * 4).map(|i| (i as f32 * 0.0327).sin() * 0.5).collect();
        let cuts: Vec<Cutoff> = (0..16).map(|v| Cutoff::new(400.0 + 300.0 * v as f32, 4.0 * RATE, 0.5)).collect();
        let ks: Vec<f32> = (0..16).map(|v| 1.0 + 0.2 * v as f32).collect();
        let report = |label: &str, ns: f64, acc: f32| println!("  {label:<38} {ns:7.1} ns   (acc {acc:.3e})");
        // Scalar, sixteen voices interleaved per substep.
        {
            let mut ls: Vec<Ladder> = (0..16).map(|_| Ladder::new()).collect();
            let mut acc = 0.0_f32;
            let t0 = std::time::Instant::now();
            for i in 0..n {
                for j in 0..4 {
                    let x = s4[i * 4 + j];
                    for v in 0..16 {
                        acc += ls[v].tick(x, &cuts[v], ks[v]);
                    }
                }
            }
            report("ladder x16 scalar, interleaved", t0.elapsed().as_secs_f64() * 1e9 / (n * 16) as f64, acc);
        }
        macro_rules! lanes {
            ($L:expr, $label:expr) => {{
                const L: usize = $L;
                let groups = 16 / L;
                let mut lanes: Vec<LadderLanes<L>> = (0..groups).map(|_| LadderLanes::default()).collect();
                let mut cls: Vec<CutoffLanes<L>> = (0..groups).map(|_| CutoffLanes::default()).collect();
                let mut kl = vec![[0.0_f32; L]; groups];
                for v in 0..16 {
                    cls[v / L].set(v % L, &cuts[v]);
                    kl[v / L][v % L] = ks[v];
                }
                let mut acc = 0.0_f32;
                let mut x = [0.0_f32; L];
                let mut out = [0.0_f32; L];
                let t0 = std::time::Instant::now();
                for i in 0..n {
                    for j in 0..4 {
                        x.fill(s4[i * 4 + j]);
                        for g in 0..groups {
                            lanes[g].tick(&x, &cls[g], &kl[g], &mut out);
                            acc += out[0] + out[L - 1];
                        }
                    }
                }
                report($label, t0.elapsed().as_secs_f64() * 1e9 / (n * 16) as f64, acc);
            }};
        }
        lanes!(4, "ladder x16 in lanes of 4");
        lanes!(8, "ladder x16 in lanes of 8");
        lanes!(16, "ladder x16 in lanes of 16");
        {
            const L: usize = 8;
            let mut lanes: Vec<SemLanes<L>> = (0..2).map(|_| SemLanes::default()).collect();
            let mut cls: Vec<CutoffLanes<L>> = (0..2).map(|_| CutoffLanes::default()).collect();
            let mut dl = [[0.5_f32; L]; 2];
            for v in 0..16 {
                cls[v / L].set(v % L, &cuts[v]);
                dl[v / L][v % L] = 0.5;
            }
            let mut acc = 0.0_f32;
            let mut x = [0.0_f32; L];
            let mut out = [0.0_f32; L];
            let t0 = std::time::Instant::now();
            for i in 0..n {
                for j in 0..4 {
                    x.fill(s4[i * 4 + j]);
                    for g in 0..2 {
                        lanes[g].tick(&x, &cls[g], &dl[g], 0.0, &mut out);
                        acc += out[0];
                    }
                }
            }
            report("SEM x16 in lanes of 8", t0.elapsed().as_secs_f64() * 1e9 / (n * 16) as f64, acc);
        }
    }
    time("tpt_g (tan)", Box::new(move |i| tpt_g(100.0 + (i % 1000) as f32, 4.0 * RATE)));
    time("exp2", Box::new(move |i| ((i % 100) as f32 * 0.01).exp2()));
    time("fast_tanh", Box::new(move |i| analog::filter::fast_tanh((i % 100) as f32 * 0.03 - 1.5)));

    // One voice alone, through its own render: the stages plus the glue.
    {
        let mut synth = Analog::new(RATE);
        synth.set_all(&[(P_VOICE_MODE as u32, 0.0)]);
        let vp = synth.voice_params_for_bench();
        let taps = Taps::new();
        let mut v = Voice::new(0, RATE);
        v.note_on(48, 0.9, 1, true, false);
        v.prepare(&vp, RATE);
        let lfo = [0.0_f32; 32];
        let mut out = [0.0_f32; 64];
        let t0 = std::time::Instant::now();
        let reps = 40_000;
        for _ in 0..reps {
            out.fill(0.0);
            v.render(&vp, &taps, &lfo, RATE, &mut out, 32, 1.0, 0.0);
        }
        let ns = t0.elapsed().as_secs_f64() * 1e9 / (reps * 32) as f64;
        println!("  {:<38} {ns:7.1} ns", "one voice, Voice::render");
    }

    // The whole instrument, both roads.
    for laned in [false, true] {
        let mut synth = Analog::new(RATE);
        synth.set_laned(laned);
        synth.set_all(&[(P_VOICES as u32, 1.0), (P_ENSEMBLE as u32, 0.67), (P_MIX_O2 as u32, 0.6)]);
        for k in [36u8, 40, 43, 47, 48, 52, 55, 59, 60, 64, 67, 71, 72, 76, 79, 83] {
            synth.note_on(k, 0.9);
        }
        let mut out = vec![0.0_f32; BLOCK * 2];
        for _ in 0..20 {
            synth.render(&mut out, BLOCK, 2, &[], Transport::default());
        }
        let blocks = (RATE as usize) / BLOCK;
        let t0 = std::time::Instant::now();
        for _ in 0..blocks {
            synth.render(&mut out, BLOCK, 2, &[], Transport::default());
        }
        let secs = (blocks * BLOCK) as f32 / RATE;
        let el = t0.elapsed().as_secs_f32();
        let factor = secs / el;
        println!(
            "  {:<38} {:7.1} ns per voice-sample;  {factor:.1}x realtime — the brief's gate is 40x: {}",
            if laned { "16 voices + ensemble, filters LANED" } else { "16 voices + ensemble, filters scalar" },
            el * 1e9 / (blocks * BLOCK * 16) as f32,
            if factor >= 40.0 { "MET" } else { "NOT MET" }
        );
    }
    // The cost against notes sounding, laned road: what a polyphony dial
    // buys, and whether an idle lane is free.
    {
        println!("  cost against notes sounding (laned road), per sample and as a share of one core at 48 kHz:");
        for n in [1usize, 2, 3, 4, 8, 9, 12, 16] {
            let mut synth = Analog::new(RATE);
            synth.set_all(&[(P_VOICES as u32, 1.0), (P_ENSEMBLE as u32, 0.0)]);
            for k in 0..n {
                synth.note_on(36 + 3 * k as u8, 0.9);
            }
            let mut out = vec![0.0_f32; BLOCK * 2];
            for _ in 0..20 {
                synth.render(&mut out, BLOCK, 2, &[], Transport::default());
            }
            let blocks = (RATE as usize) / BLOCK / 2;
            let t0 = std::time::Instant::now();
            for _ in 0..blocks {
                synth.render(&mut out, BLOCK, 2, &[], Transport::default());
            }
            let el = t0.elapsed().as_secs_f64();
            let per_sample = el * 1e9 / (blocks * BLOCK) as f64;
            let share = el / ((blocks * BLOCK) as f64 / f64::from(RATE)) * 100.0;
            println!("    {n:2} voices: {per_sample:7.0} ns per sample   {share:5.2} % of a core");
        }
    }
    // Where a voice-sample goes, sixteen voices interleaved as the frame
    // does it: the three phases timed apart.
    {
        use analog::voice::TickCtx;
        let synth = Analog::new(RATE);
        let vp = synth.voice_params_for_bench();
        let ctx = TickCtx::new(&vp, RATE);
        let taps = Taps::new();
        let mut vs: Vec<Voice> = (0..16).map(|i| Voice::new(i, RATE)).collect();
        for (i, v) in vs.iter_mut().enumerate() {
            v.note_on(36 + 3 * i as u8, 0.9, i as u32, true, false);
            v.prepare(&vp, RATE);
        }
        let reps = 20_000;
        let (mut t_pre, mut t_sub, mut t_post) = (0.0_f64, 0.0_f64, 0.0_f64);
        let mut acc = 0.0_f32;
        for r in 0..reps {
            let lfo = (r as f32 * 0.001).sin();
            let t0 = std::time::Instant::now();
            for v in vs.iter_mut() {
                v.pre(&vp, &taps, &ctx, lfo);
            }
            let t1 = std::time::Instant::now();
            for j in 0..4 {
                for v in vs.iter_mut() {
                    v.substep(&vp, &ctx, j);
                }
            }
            let t2 = std::time::Instant::now();
            for v in vs.iter_mut() {
                let (l, r) = v.post(&taps);
                acc += l + r;
            }
            let t3 = std::time::Instant::now();
            t_pre += (t1 - t0).as_secs_f64();
            t_sub += (t2 - t1).as_secs_f64();
            t_post += (t3 - t2).as_secs_f64();
        }
        let per = |t: f64| t * 1e9 / (reps * 16) as f64;
        println!(
            "  {:<38} pre {:5.1}  substeps {:5.1}  post {:5.1} ns   (acc {acc:.2e})",
            "per voice-sample, 16 voices, scalar", per(t_pre), per(t_sub), per(t_post)
        );
    }
}

/// A one-voice render with the dials set, `secs` long, mono: the report's
/// workhorse.
fn render_mono(setup: &[(u32, f32)], events: &[(f32, Ev)], secs: f32) -> Vec<f32> {
    let score = Score { events: events.to_vec(), tail: 0.0, setup: setup.to_vec() };
    let score = Score { tail: secs - score.len_secs(), ..score };
    let (out, _) = render(&score);
    out.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
}

/// The clean voicing every measurement starts from: drift off, one saw,
/// filter open, no contour, spread off.
const CLEAN: &[(u32, f32)] = &[
    (P_DRIFT as u32, 0.0),
    (P_OSC_DRIFT as u32, 0.0),
    (P_CUTOFF as u32, 1.0),
    (P_CONTOUR as u32, 0.0),
    (P_KEYTRACK as u32, 0.0),
    (P_VEL_FILTER as u32, 0.0),
    (P_SPREAD as u32, 0.0),
    (P_A_RELEASE as u32, 0.3),
];

fn gate(name: &str, value: String, ok: bool, rule: &str) {
    println!("  {name:<26} {value:<28} {:<8} ({rule})", if ok { "ok" } else { "FAIL" });
}

/// **Every gate in the brief's §9, as a number.** The `#[test]`s in the
/// module assert these; this prints them, so a change to the model can be
/// argued with in numbers a second after it is typed.
fn report() {
    use analog::filter::{ladder_k, sem_damp, Cutoff, Ladder, Sem};
    use analog::osc::{Osc, Wave};
    println!("Tangent Analog — the brief's gates (§9), at {RATE} Hz");

    // Alias floor: saw at 8′ C7, worst bin off the harmonic grid.
    {
        let f0 = 440.0 * 2.0_f32.powf((96.0 - 69.0) / 12.0);
        let mut o = Osc::new(7);
        let x: Vec<f32> = (0..RATE as usize).map(|_| o.tick(Wave::Saw, f0 / RATE, 0.0, None)).collect();
        let x = &x[4096..];
        let fund = measure::goertzel_hann(x, f0, RATE);
        let mut worst = (0.0_f32, 0.0_f32);
        let mut hz = 20.0_f32;
        while hz < RATE * 0.5 - 100.0 {
            let near = (hz / f0).round() * f0;
            if (hz - near).abs() > 40.0 {
                let a = measure::goertzel_hann(x, hz, RATE);
                if a > worst.0 {
                    worst = (a, hz);
                }
            }
            hz += 10.0;
        }
        let db = measure::db(worst.0 / fund);
        gate("alias floor", format!("{db:.1} dB at {:.0} Hz", worst.1), db <= -60.0, "saw at C7, <= -60 dB");
    }

    // Ladder self-oscillation pitch against the mapping.
    {
        let sr4 = 4.0 * RATE;
        let mut worst = 0.0_f32;
        for hz in [80.0_f32, 220.0, 1_000.0, 3_000.0, 8_000.0] {
            let mut f = Ladder::new();
            let c = Cutoff::new(hz, sr4, 1.0);
            let k = ladder_k(1.0);
            let n = (sr4 * 1.5) as usize;
            let period = sr4 / hz;
            let x: Vec<f32> = (0..n)
                .map(|i| {
                    let kick = if (i as f32) < period { 0.5 * (i as f32 / period * std::f32::consts::TAU).sin() } else { 0.0 };
                    f.tick(kick, &c, k)
                })
                .collect();
            let got = measure::measure_hz(&x[n * 2 / 3..], hz, sr4);
            let cents = 1200.0 * (got / hz).log2();
            worst = if cents.abs() > worst.abs() { cents } else { worst };
        }
        gate("ladder self-oscillation", format!("worst {worst:+.1} cents"), worst.abs() <= 15.0, "80 Hz..8 kHz, <= ±15 cents");
    }

    // Ladder bass loss: 100 Hz under a 1 kHz cutoff, emphasis 0 vs 0.9.
    {
        let sr4 = 4.0 * RATE;
        let level = |e: f32| {
            let mut f = Ladder::new();
            let c = Cutoff::new(1_000.0, sr4, 1.0);
            let n = (sr4 * 0.5) as usize;
            let x: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f64 / f64::from(sr4);
                    f.tick((0.1 * (t * 100.0 * std::f64::consts::TAU).sin()) as f32, &c, ladder_k(e))
                })
                .collect();
            measure::db(measure::goertzel_hann(&x[n / 2..], 100.0, sr4) / 0.1)
        };
        let (a, b) = (level(0.0), level(0.9));
        let mut mono = true;
        let mut last = f32::MAX;
        for step in 0..=9 {
            let l = level(step as f32 / 10.0);
            mono &= l < last;
            last = l;
        }
        gate(
            "ladder bass loss",
            format!("{:+.1} dB at 0.9 (0: {a:+.1} dB)", b),
            mono && b < a - 3.0,
            "monotone, uncompensated, 1/(1+k)",
        );
    }

    // SEM stability: resonance 1.0, cutoff swept under a sine.
    {
        let sr4 = 4.0 * RATE;
        let mut worst = 0.0_f32;
        let mut finite = true;
        for mode in [0.0_f32, 0.5, 1.0] {
            let mut f = Sem::new();
            let damp = sem_damp(1.0);
            let n = (sr4 * 2.0) as usize;
            for i in 0..n {
                let t = i as f64 / f64::from(sr4);
                let hz = 20.0 * 1000.0_f32.powf(i as f32 / n as f32);
                let x = (0.5 * (t * 1_000.0 * std::f64::consts::TAU).sin()) as f32;
                let y = f.tick(x, &Cutoff::new(hz, sr4, damp), damp, mode);
                finite &= y.is_finite();
                worst = worst.max(y.abs());
            }
        }
        let over = measure::db(worst / 0.5);
        gate("SEM stability", format!("peak {over:+.1} dB over input"), finite && over <= 12.0, "res 1.0, 20 Hz..20 kHz, <= +12 dB");
    }

    // Envelope timing: attack rise and release T60 against the mapping.
    {
        use analog::env::{attack_rise_10_90, release_t60, Env, EnvCoeffs};
        let mut worst = 0.0_f32;
        for dial in [0.3_f32, 0.6] {
            let c = EnvCoeffs::new(RATE, dial, 0.5, 1.0, dial, 1.0);
            let want_rise = attack_rise_10_90(dial);
            let want_t60 = release_t60(dial);
            let on = ((want_rise * 4.0 + 0.2) * RATE) as usize;
            let total = on + (want_t60 * 1.5 * RATE) as usize;
            let mut e = Env::new();
            e.gate_on();
            let x: Vec<f32> = (0..total)
                .map(|i| {
                    if i == on {
                        e.gate_off();
                    }
                    e.tick(&c)
                })
                .collect();
            let rise = measure::rise_10_90(&x[..on], RATE).unwrap_or(0.0);
            let t60 = measure::decay_t60(&x[on..], RATE).unwrap_or(0.0);
            worst = worst.max((rise / want_rise - 1.0).abs() * 100.0).max((t60 / want_t60 - 1.0).abs() * 100.0);
        }
        gate("envelope timing", format!("worst {worst:.1} % off the mapping"), worst < 5.0, "10-90 % rise and T60, within 5 %");
    }

    // Restrike and steal: the shape of what the event added.
    {
        let fresh = |setup: &[(u32, f32)], key: u8| {
            let x = render_mono(setup, &[(0.01, Ev::On(key, 100))], 0.1);
            (measure::d1(&x, 0, x.len()), measure::d2(&x, 0, x.len()))
        };
        let added = |setup: &[(u32, f32)], base: &[(f32, Ev)], event: (f32, Ev), secs: f32| {
            let bed = render_mono(setup, base, secs);
            let mut with = base.to_vec();
            with.push(event);
            let got = render_mono(setup, &with, secs);
            let diff: Vec<f32> = got.iter().zip(&bed).map(|(a, b)| a - b).collect();
            let at = (event.0 * RATE) as usize / BLOCK * BLOCK;
            let (a, b) = (at.saturating_sub((0.002 * RATE) as usize), at + (0.02 * RATE) as usize);
            (measure::d1(&diff, a, b), measure::d2(&diff, a, b))
        };
        let shape = |added: (f32, f32), fresh: (f32, f32)| {
            let r1 = added.0 / fresh.0.max(1e-12);
            let r2 = added.1 / fresh.1.max(1e-12);
            (r1, r2 / r1.max(1e-12))
        };
        let mut setup = CLEAN.to_vec();
        setup.push((P_A_RELEASE as u32, 0.6));
        let f = fresh(&setup, 60);
        let base = [(0.0, Ev::Pedal(true)), (0.01, Ev::On(60, 100)), (0.12, Ev::Off(60))];
        let (r1, ratio) = shape(added(&setup, &base, (0.3, Ev::On(60, 100)), 0.6), f);
        gate("restrike", format!("added {r1:.2}x, shape {ratio:.2}"), ratio <= 2.5, "shape near 1 is a hammer, not a step");

        // The steal: how fast the stolen note leaves the difference. A
        // saw is a step every period, so the shape test cannot tell a cut
        // from a fade here; the fade's onset can (see the module's test).
        let mut setup = CLEAN.to_vec();
        setup.push((P_A_ATTACK as u32, 0.35));
        let mut base = vec![(0.0, Ev::Pedal(true))];
        for (i, k) in [48u8, 50, 52, 53, 55, 57, 59, 60].iter().enumerate() {
            base.push((0.01 + 0.009 * i as f32, Ev::On(*k, 100)));
        }
        let event = (0.2, Ev::On(72, 100));
        let bed = render_mono(&setup, &base, 0.5);
        let mut with = base.to_vec();
        with.push(event);
        let got = render_mono(&setup, &with, 0.5);
        let diff: Vec<f32> = got.iter().zip(&bed).map(|(a, b)| a - b).collect();
        let at = (event.0 * RATE) as usize / BLOCK * BLOCK;
        let ms = |m: f32| (m * 0.001 * RATE) as usize;
        let early = measure::rms(&diff[at..at + ms(1.0)]);
        let late = measure::rms(&diff[at + ms(8.0)..at + ms(12.0)]);
        let onset = measure::db(early / late.max(1e-9));
        gate("steal", format!("first ms {onset:+.1} dB vs 8-12 ms"), onset < -8.0, "a fade is ~-20 dB, a cut is level");
    }

    // Drift determinism.
    {
        let setup = [(P_DRIFT as u32, 1.0), (P_OSC_DRIFT as u32, 1.0), (P_ENSEMBLE as u32, 0.67), (P_MIX_NOISE as u32, 0.3)];
        let ev = [(0.0, Ev::On(60, 100)), (0.009, Ev::On(64, 90)), (0.6, Ev::Off(60)), (0.6, Ev::Off(64))];
        let a = render_mono(&setup, &ev, 1.0);
        let b = render_mono(&setup, &ev, 1.0);
        gate("drift determinism", if a == b { "bit for bit".into() } else { "DIFFERS".into() }, a == b, "same seed, same audio");
    }

    // Tuning: every range and note through the oscillator.
    {
        let mut worst = 0.0_f32;
        for &oct in &analog::osc::RANGE_OCTAVES[1..] {
            for key in (24..=108).step_by(4) {
                let hz = 440.0 * 2.0_f32.powf((key as f32 - 69.0) / 12.0 + oct);
                if hz / RATE > analog::osc::MAX_INC || hz < 30.0 {
                    continue;
                }
                let mut o = Osc::new(7);
                let x: Vec<f32> = (0..(0.8 * RATE) as usize).map(|_| o.tick(Wave::Saw, hz / RATE, 0.0, None)).collect();
                let got = measure::measure_hz(&x[64..], hz, RATE);
                let cents = 1200.0 * (got / hz).log2();
                if cents.abs() > worst.abs() {
                    worst = cents;
                }
            }
        }
        let x = render_mono(CLEAN, &[(0.0, Ev::On(60, 100))], 1.0);
        let got = measure::measure_hz(&x[(0.2 * RATE) as usize..], 261.6256, RATE);
        let synth_cents = 1200.0 * (got / 261.6256).log2();
        gate(
            "tuning",
            format!("osc worst {worst:+.2} c, synth C4 {synth_cents:+.2} c"),
            worst.abs() <= 1.0 && synth_cents.abs() <= 1.0,
            "32'..2' x every 4th note, <= ±1 cent",
        );
    }

    // Level: eight-voice forte chord.
    {
        let mut ev = Vec::new();
        for (i, k) in [48u8, 55, 60, 64, 67, 71, 74, 79].iter().enumerate() {
            ev.push((0.009 * i as f32, Ev::On(*k, 127)));
        }
        let score = Score { events: ev, tail: 1.5, setup: Vec::new() };
        let (out, _) = render(&score);
        let pk = measure::db(measure::peak(&out));
        let clipped = out.iter().filter(|s| s.abs() >= 1.0).count();
        gate("level", format!("peak {pk:+.1} dBFS, clipped {clipped}"), pk <= -6.0 && clipped == 0, "8-voice forte chord, <= -6 dBFS");
    }

    println!();
    perf();
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let perf_name = args.get(1).cloned().unwrap_or_else(|| "single".into());
    if perf_name == "perf" {
        perf();
        return;
    }
    if perf_name == "report" {
        report();
        return;
    }
    let out_path = args.get(2).cloned().unwrap_or_else(|| format!("{perf_name}.wav"));
    let score = build(&perf_name);
    let (out, cpu) = render(&score);

    let peak = measure::peak(&out);
    let rms = measure::rms(&out);
    let clipped = out.iter().filter(|s| s.abs() >= 1.0).count();
    let bad = out.iter().filter(|s| !s.is_finite()).count();
    wav::write24(&out_path, RATE as u32, 2, &out).expect("write wav");
    let secs = out.len() as f32 / 2.0 / RATE;
    eprintln!(
        "{out_path}: {:.1}s  peak {:.3} ({:+.1} dBFS)  rms {:+.1} dBFS  clipped {clipped}  nonfinite {bad}",
        secs,
        peak,
        measure::db(peak),
        measure::db(rms),
    );
    eprintln!(
        "  render {:.0} ms for {:.1} s audio  =  {:.0}x real time",
        cpu.as_secs_f32() * 1000.0,
        secs,
        secs / cpu.as_secs_f32().max(1e-9)
    );
}
