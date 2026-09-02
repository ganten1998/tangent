//! The Tangent Keys bench.
//!
//! Renders a scripted performance through the REAL module and writes a WAV.
//! No audio device, no DAW, no GUI: a change to the physics is audible and
//! measurable about a second after it is typed, which is the only way anyone
//! has ever tuned an instrument model.
//!
//!   cargo run --release -- <patch> <performance> <out.wav>
//!   patches: piano piano2 ep suitcase clav clavwah wurli wurli140

// Relative to THIS FILE, which is how `#[path]` resolves: the bench lives
// in tools/keysrig and reaches over to the one true copy of the DSP.
#[path = "../../../tangent/src/keys/mod.rs"]
mod keys;
mod analyze;
mod wav;

use keys::{Keys, Patch};

const RATE: f32 = 48_000.0;
/// The same block size the audio callback uses, so a bug that only happens at
/// a block boundary happens here too.
const BLOCK: usize = 128;

#[derive(Clone, Copy)]
enum Ev {
    On(u8, u8),
    Off(u8),
    Pedal(bool),
}

struct Score {
    /// (seconds, event)
    events: Vec<(f32, Ev)>,
    /// How long to keep rendering after the last event.
    tail: f32,
}

impl Score {
    fn len_secs(&self) -> f32 {
        self.events.iter().map(|e| e.0).fold(0.0, f32::max) + self.tail
    }
}

/// A note: on at `t`, off at `t + dur`.
fn note(ev: &mut Vec<(f32, Ev)>, t: f32, dur: f32, pitch: u8, vel: u8) {
    ev.push((t, Ev::On(pitch, vel)));
    ev.push((t + dur, Ev::Off(pitch)));
}

fn chord(ev: &mut Vec<(f32, Ev)>, t: f32, dur: f32, pitches: &[u8], vel: u8) {
    // Real hands are not simultaneous. A few milliseconds of spread is the
    // difference between a chord and a sample trigger.
    for (i, &p) in pitches.iter().enumerate() {
        note(ev, t + i as f32 * 0.009, dur, p, vel);
    }
}

fn build(name: &str) -> Score {
    let mut ev = Vec::new();
    match name {
        // One note, held, undamped: for measuring decay and partials.
        "single" => {
            note(&mut ev, 0.0, 6.0, 60, 90);
            Score { events: ev, tail: 6.0 }
        }
        // The velocity ladder. The same note, twelve times, from as soft as
        // MIDI goes to as hard. This is the test the whole brief turns on:
        // if these twelve only differ in loudness, the model has failed.
        "velramp" => {
            let mut t = 0.0;
            for i in 0..12 {
                let v = (8 + i * 11).min(127) as u8;
                note(&mut ev, t, 1.4, 64, v);
                t += 1.7;
            }
            Score { events: ev, tail: 3.0 }
        }
        // Every octave, so tuning and register balance are checkable at a
        // glance on a spectrogram.
        "chrom" => {
            let mut t = 0.0;
            for p in (21..=108).step_by(3) {
                note(&mut ev, t, 0.55, p as u8, 84);
                t += 0.62;
            }
            Score { events: ev, tail: 4.0 }
        }
        // Rootless voicings, pedalled, in the register the man actually lived
        // in. Tests the pedal, the sympathetic wash, and whether close
        // intervals in the tenor turn to mud.
        "evans" => {
            ev.push((0.0, Ev::Pedal(true)));
            // | Cmaj7  | Am7 D7 | Gmaj7 | ...
            chord(&mut ev, 0.10, 1.6, &[52, 59, 64, 67, 71], 62);
            note(&mut ev, 0.10, 1.6, 36, 70);
            ev.push((1.72, Ev::Pedal(false)));
            ev.push((1.78, Ev::Pedal(true)));
            chord(&mut ev, 1.80, 0.8, &[52, 57, 62, 67], 58);
            note(&mut ev, 1.80, 0.8, 33, 66);
            chord(&mut ev, 2.70, 0.8, &[52, 57, 60, 66], 64);
            note(&mut ev, 2.70, 0.8, 38, 68);
            ev.push((3.55, Ev::Pedal(false)));
            ev.push((3.62, Ev::Pedal(true)));
            chord(&mut ev, 3.65, 2.4, &[50, 57, 59, 66], 55);
            note(&mut ev, 3.65, 2.4, 31, 64);
            // A single melodic line over the top, quiet, where the model's
            // dynamic floor shows.
            note(&mut ev, 4.10, 0.30, 74, 44);
            note(&mut ev, 4.45, 0.28, 76, 40);
            note(&mut ev, 4.80, 0.55, 79, 48);
            ev.push((6.4, Ev::Pedal(false)));
            Score { events: ev, tail: 5.0 }
        }
        // A comp figure that starts feathered and ends hard, so the bark
        // arrives inside one phrase rather than between two renders.
        "comp" => {
            let voicing = [53u8, 57, 60, 65];
            let mut t = 0.0;
            for i in 0..8 {
                let v = (34 + i * 12).min(127) as u8;
                chord(&mut ev, t, 0.42, &voicing, v);
                note(&mut ev, t, 0.42, 41, v.saturating_sub(6));
                t += 0.55;
            }
            // and the bell, alone, quiet, at the end
            note(&mut ev, t + 0.4, 3.0, 72, 30);
            note(&mut ev, t + 1.0, 3.0, 79, 26);
            Score { events: ev, tail: 5.0 }
        }
        _ => {
            note(&mut ev, 0.0, 2.0, 60, 100);
            Score { events: ev, tail: 4.0 }
        }
    }
}

/// Render a score. Returns interleaved stereo and the wall-clock cost.
fn render(patch: Patch, score: &Score) -> (Vec<f32>, std::time::Duration) {
    let mut events = score.events.clone();
    events.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));

    let mut synth = Keys::new(RATE);
    synth.set_patch(patch);
    // KEYSRIG_PARAMS="5=1.0,0=0.2" turns knobs without a UI: the bench's
    // whole point is measuring what a knob does before anyone hears it.
    if let Ok(spec) = std::env::var("KEYSRIG_PARAMS") {
        for part in spec.split(',') {
            if let Some((i, v)) = part.split_once('=') {
                if let (Ok(i), Ok(v)) = (i.trim().parse::<usize>(), v.trim().parse::<f32>()) {
                    synth.set_param(i, v);
                }
            }
        }
    }

    let total = ((score.len_secs() * RATE) as usize / BLOCK + 1) * BLOCK;
    let mut out = vec![0.0_f32; total * 2];
    let mut scratch = vec![0.0_f32; BLOCK * 2];

    let mut next = 0usize;
    let mut frame = 0usize;
    let t0 = std::time::Instant::now();
    while frame < total {
        // Events land on the block they fall in, which is exactly the timing
        // resolution the engine gives a real note.
        let block_end = (frame + BLOCK) as f32 / RATE;
        while next < events.len() && events[next].0 < block_end {
            match events[next].1 {
                Ev::On(p, v) => synth.note_on(p, f32::from(v) / 127.0),
                Ev::Off(p) => synth.note_off(p),
                Ev::Pedal(d) => synth.set_pedal(d),
            }
            next += 1;
        }
        scratch.fill(0.0);
        synth.render(&mut scratch, BLOCK, 2);
        out[frame * 2..(frame + BLOCK) * 2].copy_from_slice(&scratch);
        frame += BLOCK;
    }
    (out, t0.elapsed())
}

/// One struck string, unison collapsed, held for `secs` — straight off the
/// voice, no Body, no pan. The per-partial t60 column measures THIS: the
/// ensemble's envelope is sculpted 15 dB either way by the unison's beat
/// geometry (run `piano track` to watch it), which is the instrument being
/// alive, not the loss design — and the t60 column exists to check the
/// design. See `strike_spread` in string.rs. The base picks WHICH grand.
fn solo_string(pitch: u8, vel: f32, secs: f32, base: &keys::string::PianoBase) -> Vec<f32> {
    let mut v = keys::string::PianoVoice::new(RATE);
    let mut k = keys::PianoKnobs::default();
    k.unison = 0.0;
    v.strike_based(pitch, vel, &k, base);
    (0..(secs * RATE) as usize).map(|_| v.tick()).collect()
}

/// The label, the inharmonicity scale, and the piano base (where one
/// applies) for every patch the bench knows.
fn patch_label(patch: Patch) -> &'static str {
    match patch {
        Patch::Piano1 => "Piano 1",
        Patch::Piano2 => "Piano 2",
        Patch::EpMk1 => "EP Mk.1",
        Patch::EpSuitcase => "EP Suitcase",
        Patch::Clav => "Clav",
        Patch::ClavWah => "Clav Wah",
        Patch::Wurli200 => "Wurli 200",
        Patch::Wurli140 => "Wurli 140",
    }
}

/// The strongest magnitude within 5% of `around` — how every partial probe
/// must look for its target, because dispersion moves the real partial
/// sharp of the ideal series and a pinned probe measures the space between.
fn search_peak(x: &[f32], around: f32) -> f32 {
    let mut best = 0.0_f32;
    let mut f = around * 0.95;
    while f < around * 1.05 {
        best = best.max(analyze::goertzel_hann(x, f, RATE));
        f += around * 0.001;
    }
    best
}

/// Mono sum of a window of the interleaved buffer, in seconds.
fn window(out: &[f32], from: f32, len: f32) -> Vec<f32> {
    let a = ((from * RATE) as usize).min(out.len() / 2);
    let b = (((from + len) * RATE) as usize).min(out.len() / 2);
    (a..b).map(|i| (out[i * 2] + out[i * 2 + 1]) * 0.5).collect()
}

/// **The velocity report.** The one measurement this whole instrument is
/// judged on: twelve strikes of one key from as soft as MIDI goes to as hard,
/// and what changed besides the level. If HF ratio is flat, the model has no
/// dynamic timbre and every note is the same note turned up.
fn report_velocity(patch: Patch) {
    let score = build("velramp");
    let (out, cpu) = render(patch, &score);
    println!(
        "\n  {} - velocity response on E4, 12 strikes",
        patch_label(patch)
    );
    println!("  vel   peak dB   bell/f0    HF dB    asym");
    println!("  ---   -------   -------   ------   -----");
    let f0 = 440.0 * 2.0_f32.powf((64.0 - 69.0) / 12.0);
    // Where the brightness lives, per mechanism: the bell partial on the tine,
    // the upper half of the series on the string.
    let bright: Vec<f32> = match patch {
        Patch::EpMk1 | Patch::EpSuitcase => {
            let b = (6.6_f32 - 0.030 * (64.0 - 28.0)).clamp(3.6, 6.8);
            vec![f0 * b, f0 * b * 1.62, f0 * b * 2.71]
        }
        Patch::Piano1 | Patch::Piano2 => {
            let scale = if matches!(patch, Patch::Piano2) { 0.85 } else { 1.0 };
            let bb = scale * 2.0e-4 * 2.0_f32.powf((64.0 - 48.0) / 22.0);
            (5..=10)
                .map(|n| n as f32 * f0 * (1.0 + bb * (n * n) as f32).sqrt())
                .collect()
        }
        Patch::Clav | Patch::ClavWah => {
            let bb = 1.2e-4 * 2.0_f32.powf((64.0 - 52.0) / 26.0);
            (4..=10)
                .map(|n| n as f32 * f0 * (1.0 + bb * (n * n) as f32).sqrt())
                .collect()
        }
        // For the reed piano the brightness IS the growl: the plate's
        // second and third harmonics, absent feathered, arriving with
        // force. The bar's own overtones are colour, not the story.
        Patch::Wurli200 | Patch::Wurli140 => vec![f0 * 2.0, f0 * 3.0],
    };
    let mut first_hf = 0.0_f32;
    for i in 0..12 {
        let v = (8 + i * 11).min(127);
        let t = i as f32 * 1.7;
        // The first 250 ms: the attack, where the timbre lives.
        let w = window(&out, t + 0.004, 0.25);
        let hf = analyze::hf_ratio(&w);
        if i == 0 {
            first_hf = hf;
        }
        // **The measurement the brief turns on.** How loud the bright partials
        // are RELATIVE to the fundamental. A plain HF ratio will not show this:
        // the fundamental carries twenty-odd decibels more power than anything
        // above it, so it dominates the sum and a huge change in the bell
        // barely moves the number.
        // Hann-windowed, not rectangular: at pianissimo the true upper
        // partials sit 50-odd dB under the fundamental, which is exactly
        // where a rectangular window's leakage floor lands, and a
        // measurement that bottoms out on its own leakage can never show
        // the soft end of the velocity curve. And SEARCHED, not sampled: the
        // waveguide's all-pass dispersion stretches the top of the series
        // further than the B formula predicts, so a probe pinned to the
        // ideal frequency lands between partials and reads the floor.
        let fund = analyze::goertzel_hann(&w, f0, RATE);
        let hi: f32 = bright.iter().map(|&f| search_peak(&w, f)).sum();
        println!(
            "  {v:3}   {:+7.1}   {:+7.1}   {:+6.1}   {:5.3}",
            analyze::db(analyze::peak(&w)),
            analyze::db(hi / fund.max(1e-12)),
            analyze::db(hf / first_hf.max(1e-9)),
            analyze::asymmetry(&w),
        );
    }
    println!(
        "  rendered {:.1} s in {:.0} ms  ({:.0}x real time)",
        score.len_secs(),
        cpu.as_secs_f32() * 1000.0,
        score.len_secs() / cpu.as_secs_f32().max(1e-9)
    );
}

/// **The partial report.** Where the overtones actually landed, against where
/// the physics says they should be. For the piano that is the stretched series
/// f_n = n*f0*sqrt(1 + B n^2); for the electric piano it is the bar's modes,
/// which are not a series at all.
fn report_partials(patch: Patch) {
    let score = build("single");
    let (out, _) = render(patch, &score);
    let f0 = 440.0 * 2.0_f32.powf((60.0 - 69.0) / 12.0);
    let w = window(&out, 0.02, 2.0);
    // **What "ideal" means depends on the mechanism.** A string's partials are
    // a stretched harmonic series; a stiff bar's are not a series at all, so
    // scoring the electric piano against n*f0 would report its bell partial as
    // a hundred cents flat when it is exactly where a tine puts it.
    let bell = (6.6_f32 - 0.030 * (60.0 - 28.0)).clamp(3.6, 6.8);
    let ideal_of = |n: usize| -> f32 {
        match patch {
            Patch::Piano1 | Patch::Piano2 => {
                // B for C4 on the schedule in string.rs, scaled per base.
                let scale = if matches!(patch, Patch::Piano2) { 0.85 } else { 1.0 };
                let b = scale * 2.0e-4 * 2.0_f32.powf((60.0 - 48.0) / 22.0);
                let n = n as f32;
                n * f0 * (1.0 + b * n * n).sqrt()
            }
            Patch::EpMk1 | Patch::EpSuitcase => {
                let r = [1.0, 2.0, 3.0, bell, bell * 1.62, bell * 2.71];
                f0 * r.get(n - 1).copied().unwrap_or(n as f32)
            }
            Patch::Clav | Patch::ClavWah => {
                let b = 1.2e-4 * 2.0_f32.powf((60.0 - 52.0) / 26.0);
                let n = n as f32;
                n * f0 * (1.0 + b * n * n).sqrt()
            }
            // The reed's bar modes, solder-dropped as reed.rs computes
            // them at C4, then the plate's own harmonics of the
            // fundamental — the growl products sit ON the series.
            Patch::Wurli200 | Patch::Wurli140 => {
                let drop = 1.0 - 0.06 * ((60.0_f32 - 33.0) / 40.0).clamp(0.0, 1.0);
                let r = [1.0, 2.0, 3.0, 4.4 * drop, 14.6 * drop];
                f0 * r.get(n - 1).copied().unwrap_or(n as f32)
            }
        }
    };
    let count = match patch {
        Patch::Piano1 | Patch::Piano2 => 10,
        Patch::EpMk1 | Patch::EpSuitcase => 6,
        Patch::Clav | Patch::ClavWah => 10,
        Patch::Wurli200 | Patch::Wurli140 => 5,
    };
    // t60 source: one string, one strike, no unison, no body — the decay
    // DESIGN, cleanly. Everything else in the table is the full instrument.
    let solo = match patch {
        Patch::Piano1 => {
            solo_string(60, 90.0 / 127.0, 5.8, &keys::string::PianoBase::JAZZ)
        }
        Patch::Piano2 => {
            solo_string(60, 90.0 / 127.0, 5.8, &keys::string::PianoBase::CONCERT)
        }
        _ => window(&out, 0.02, 5.8),
    };
    println!("\n  partials of C4 (f0 = {f0:.2} Hz)");
    println!("  n     ideal Hz   found Hz   cents   level dB    t60");
    println!("  ---   --------   --------   -----   --------   ------");
    for n in 1..=count {
        let ideal = ideal_of(n);
        // Search a semitone either side for where the partial really is.
        let mut best = (0.0_f32, 0.0_f32);
        let lo = ideal * 0.94;
        let hi = ideal * 1.10;
        let mut f = lo;
        while f < hi {
            // Hann again: with a rectangle, the "found" level of a weak
            // partial is the fundamental's sidelobe, and n9/n10 report as
            // present when nothing is there.
            let m = analyze::goertzel_hann(&w, f, RATE);
            if m > best.1 {
                best = (f, m);
            }
            f += ideal * 0.0008;
        }
        // The find threshold sits at -107 dB: with Hann windows the leakage
        // floor is below -120, so this is still 13 dB of daylight — the old
        // 1e-5 was calibrated for the rectangular window's far higher floor
        // and started swallowing real partials once levels were honest.
        if best.1 < 4.5e-6 {
            println!("  {n:3}   {ideal:8.1}          -       -          -        -");
            continue;
        }
        let cents = 1200.0 * (best.0 / ideal).log2();
        println!(
            "  {n:3}   {ideal:8.1}   {:8.1}   {:+5.0}   {:+8.1}   {:5.2} s",
            best.0,
            cents,
            analyze::db(best.1),
            analyze::partial_t60(&solo, best.0, RATE),
        );
    }
    // The top of the series lives fast: by design the 8th is notched out by
    // the strike position and the 9th and 10th decay in well under a second,
    // so a 2 s window integrates them into the floor. Measure the attack
    // instead, so "not found" above can be told apart from "never injected".
    if matches!(patch, Patch::Piano1 | Patch::Piano2) {
        let atk = window(&out, 0.02, 0.30);
        let rel = analyze::goertzel_hann(&atk, f0, RATE).max(1e-12);
        print!("  attack, first 300 ms, dB re f0:");
        for n in [8usize, 9, 10] {
            let m = search_peak(&atk, ideal_of(n));
            print!("   n{n} {:+.0}", analyze::db(m / rel));
        }
        println!();
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let patch = match args.get(1).map(String::as_str) {
        Some("ep") => Patch::EpMk1,
        Some("suitcase") => Patch::EpSuitcase,
        Some("piano2") => Patch::Piano2,
        Some("clav") => Patch::Clav,
        Some("clavwah") => Patch::ClavWah,
        Some("wurli") => Patch::Wurli200,
        Some("wurli140") => Patch::Wurli140,
        _ => Patch::Piano1,
    };
    let perf = args.get(2).cloned().unwrap_or_else(|| "single".into());

    if perf == "report" {
        report_velocity(patch);
        report_partials(patch);
        return;
    }

    // Diagnostic: the raw decay curve of chosen partials, 4 Hz frames, in dB.
    // For telling a measurement artifact from real physics — a beat wobbles
    // around a straight line, a wrong loss IS the line's slope.
    if perf == "track" {
        let score = build("single");
        let (out, _) = render(patch, &score);
        let f0 = 440.0 * 2.0_f32.powf((60.0 - 69.0) / 12.0);
        let b = 2.0e-4 * 2.0_f32.powf((60.0 - 48.0) / 22.0);
        // The ENSEMBLE, beats and all. Set `solo` here instead to watch the
        // bare design: the difference between the two curves is the unison.
        let mono = window(&out, 0.0, 5.9);
        println!("  t      n1 dB    n2 dB    n4 dB    n6 dB");
        let mut t = 0.05;
        while t < 5.6 {
            let i0 = (t * RATE) as usize;
            let seg = &mono[i0..(i0 + 4096).min(mono.len())];
            print!("  {t:4.2}");
            for n in [1usize, 2, 4, 6] {
                let f = n as f32 * f0 * (1.0 + b * (n * n) as f32).sqrt();
                let m = analyze::goertzel_hann(seg, f, RATE);
                print!("   {:+7.1}", analyze::db(m));
            }
            println!();
            t += 0.25;
        }
        return;
    }

    let out_path = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| format!("{perf}.wav"));
    let score = build(&perf);
    let (out, cpu) = render(patch, &score);

    let peak = analyze::peak(&out);
    let rms = (out.iter().map(|s| s * s).sum::<f32>() / out.len() as f32).sqrt();
    let clipped = out.iter().filter(|s| s.abs() >= 1.0).count();
    let bad = out.iter().filter(|s| !s.is_finite()).count();

    wav::write24(&out_path, RATE as u32, 2, &out).expect("write wav");

    let secs = out.len() as f32 / 2.0 / RATE;
    eprintln!(
        "{out_path}: {:.1}s  peak {:.3} ({:+.1} dBFS)  rms {:+.1} dBFS  clipped {clipped}  nonfinite {bad}",
        secs,
        peak,
        analyze::db(peak),
        analyze::db(rms),
    );
    eprintln!(
        "  render {:.0} ms for {:.1} s audio  =  {:.0}x real time",
        cpu.as_secs_f32() * 1000.0,
        secs,
        secs / cpu.as_secs_f32().max(1e-9)
    );
}
