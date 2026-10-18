//! The Tangent Keys bench.
//!
//! Renders a scripted performance through the REAL module and writes a WAV.
//! No audio device, no DAW, no GUI: a change to the physics is audible and
//! measurable about a second after it is typed, which is the only way anyone
//! has ever tuned an instrument model.
//!
//!   cargo run --release -- <patch> <performance> <out.wav>
//!   patches: piano piano2 ep suitcase clav clavwah reed reedamp egrand guitar
//!   (wurli and wurli140 still name the two reed patches)
//!
//!   cargo run --release -- restrike     the consecutive-strike report, every
//!                                       patch and the DX7 (see `report_restrike`)
//!   cargo run --release -- pin          one FNV per first-strike render, so a
//!                                       change meant for restrikes can PROVE it
//!                                       left a first strike alone

// Relative to THIS FILE, which is how `#[path]` resolves: the bench lives
// in tools/keysrig and reaches over to the one true copy of the DSP.
#[path = "../../../tangent/src/keys/mod.rs"]
mod keys;
mod analyze;
mod wav;

mod dx7;

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
        // The ceiling. Sixty-four keys, pedal down, struck twenty
        // milliseconds apart and let go — the pedal holds every one — so by
        // the end of the run the whole pool is sounding at once and stays
        // that way for the tail. The realtime factor this prints IS the
        // instrument's CPU budget: the brief measured the piano at 78x here
        // before the physics rework and set 25x as the floor with the
        // sympathetic harp on top. Sixty-four is `keys::VOICES`; the
        // sixty-fifth strike would steal, which is a different measurement.
        "stress" => {
            ev.push((0.0, Ev::Pedal(true)));
            let mut t = 0.05;
            for p in 30u8..94 {
                note(&mut ev, t, 0.10, p, 96);
                t += 0.02;
            }
            Score { events: ev, tail: 6.0 }
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
        Patch::Wurli200 => "Reed 200",
        Patch::Wurli140 => "Reed Amp",
        Patch::ElectricGrand => "Electric Grand",
        Patch::Guitar => "Nylon Guitar",
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
        // The electric grand's B at E4, near 4.1e-4 (`egrand::EGrandBase`).
        Patch::ElectricGrand => (5..=10)
            .map(|n| n as f32 * f0 * (1.0 + 4.1e-4 * (n * n) as f32).sqrt())
            .collect(),
        // The plucked string's upper half, on its own measured stiffness
        // (E4 is the top string open).
        Patch::Guitar => {
            let bb = keys::guitar::Place::home(64).map_or(0.0, keys::guitar::inharmonicity) as f32;
            (5..=10)
                .map(|n| n as f32 * f0 * (1.0 + bb * (n * n) as f32).sqrt() / (1.0 + bb).sqrt())
                .collect()
        }
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
            // The electric grand's B at C4 (`egrand::EGrandBase`).
            Patch::ElectricGrand => {
                let n = n as f32;
                n * f0 * (1.0 + 3.5e-4 * n * n).sqrt()
            }
            // C4 is the B string at the first fret, partial 1 on f0 exactly.
            Patch::Guitar => {
                let b = keys::guitar::Place::home(60).map_or(0.0, keys::guitar::inharmonicity) as f32;
                let n = n as f32;
                n * f0 * (1.0 + b * n * n).sqrt() / (1.0 + b).sqrt()
            }
        }
    };
    let count = match patch {
        Patch::Piano1 | Patch::Piano2 => 10,
        Patch::EpMk1 | Patch::EpSuitcase => 6,
        Patch::Clav | Patch::ClavWah => 10,
        Patch::Wurli200 | Patch::Wurli140 => 5,
        Patch::ElectricGrand => 10,
        Patch::Guitar => 10,
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

// ── consecutive strikes ─────────────────────────────────────────────────────
//
// The owner's report, verbatim: "the midi instruments all of them - don't
// handle consecutive strikes very well - they audible pop which gets
// noticeable degraded by any pedal activation". A pop is a discontinuity, and
// a discontinuity is a number: the largest one-sample step in the output.
//
// The first version of this report measured that step at the moment of a
// restrike and held it to a LINEAR ceiling: the fresh strike's own edge plus
// the bed's, a sum superposition can never exceed. That ceiling accused three
// scenarios that do not pop, and it was wrong for a reason worth writing
// down. It takes its fresh reference at the worst possible operating point.
// A fresh strike lands in silence, where the reed's plate is flat and the wah
// is parked shut; a restrike lands with the reed already deep in the gap and
// the filter wide open, so the same hammer legitimately makes a steeper edge.
// The tine's magnet, the reed's plate and the wah are DELIBERATELY nonlinear,
// and a ceiling that assumes they are not will keep accusing them. Raising it
// until the print went green was refused, rightly: a threshold relaxed until
// the output changes proves nothing and blinds the bench to real faults of
// the same size.
//
// So the verdict is the measurement the instruments' own tests already make
// and pass. Render the performance twice, once WITH the event under test and
// once without, subtract, and judge what the strike ADDED. That difference is
// immune to the operating point, because a state-dependent gain scales the
// whole addition — first difference and second by the SAME factor — while a
// state cut and re-seeded is a step, and a step is broadband, so it lands in
// the second difference many times harder than in the first. See `Shape`.
// Everything the old ceiling printed still prints, because the owner reads
// those numbers and they are worth reading; they simply no longer decide.

/// The two instruments behind one set of calls, so every scenario below is
/// written once and played on both.
trait Rig {
    fn on(&mut self, pitch: u8, vel: f32);
    fn off(&mut self, pitch: u8);
    fn pedal(&mut self, down: bool);
    fn render(&mut self, out: &mut [f32], frames: usize, channels: usize);
}

impl Rig for Keys {
    fn on(&mut self, pitch: u8, vel: f32) {
        self.note_on(pitch, vel);
    }
    fn off(&mut self, pitch: u8) {
        self.note_off(pitch);
    }
    fn pedal(&mut self, down: bool) {
        self.set_pedal(down);
    }
    fn render(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        Keys::render(self, out, frames, channels);
    }
}

impl Rig for dx7::synth::Dx7 {
    fn on(&mut self, pitch: u8, vel: f32) {
        self.note_on(pitch, vel);
    }
    fn off(&mut self, pitch: u8) {
        self.note_off(pitch);
    }
    fn pedal(&mut self, down: bool) {
        self.set_pedal(down);
    }
    fn render(&mut self, out: &mut [f32], frames: usize, channels: usize) {
        dx7::synth::Dx7::render(self, out, frames, channels);
    }
}

/// Every instrument the report plays, with a fresh instance per scenario so
/// no scenario inherits another's ringing.
fn rigs() -> Vec<(String, Box<dyn Fn() -> Box<dyn Rig>>)> {
    let mut v: Vec<(String, Box<dyn Fn() -> Box<dyn Rig>>)> = Vec::new();
    for &patch in &Patch::ALL {
        v.push((
            patch_label(patch).to_owned(),
            Box::new(move || {
                let mut k = Keys::new(RATE);
                k.set_patch(patch);
                Box::new(k)
            }),
        ));
    }
    for (name, voice) in dx7_patches() {
        v.push((
            format!("DX7 {name}"),
            Box::new(move || {
                let mut d = dx7::synth::Dx7::new(RATE);
                d.set_voice(voice);
                Box::new(d)
            }),
        ));
    }
    v
}

/// Three DX7 patches with nothing in common but the engine: the shipped
/// electric piano (oscillator sync on, a modulator that dies in a fifth of a
/// second), a percussive sync patch with feedback at the top of the stack,
/// and a free-running pad that sustains. A restrike bug lives in the voice
/// LIFECYCLE, so what matters is that the three take the three different
/// paths through note-on: sync, feedback, and a held sustain stage.
fn dx7_patches() -> Vec<(&'static str, dx7::voice::Voice)> {
    use dx7::voice::Voice;
    let ep = Voice::default();

    let mut bell = Voice::default();
    bell.osc_sync = true;
    bell.feedback = 6;
    for op in &mut bell.ops {
        op.rate = [99, 72, 50, 80];
        op.level = [99, 60, 0, 0];
    }
    bell.ops[1].coarse = 3;
    bell.ops[1].fine = 50;
    bell.ops[1].output_level = 86;
    bell.ops[3].coarse = 7;
    bell.ops[3].output_level = 70;
    bell.ops[5].coarse = 11;
    bell.ops[5].output_level = 75;
    bell.set_name("SYNC BELL");

    let mut pad = Voice::default();
    pad.osc_sync = false;
    pad.feedback = 0;
    pad.lfo_amp_depth = 25;
    pad.lfo_speed = 30;
    for op in &mut pad.ops {
        op.rate = [48, 60, 40, 55];
        op.level = [99, 95, 90, 0];
        op.amp_mod_sens = 1;
    }
    pad.ops[1].coarse = 2;
    pad.ops[1].output_level = 62;
    pad.ops[3].coarse = 1;
    pad.ops[3].fine = 2;
    pad.ops[3].output_level = 55;
    pad.set_name("FREE PAD");

    vec![("E.PIANO 1", ep), ("SYNC BELL", bell), ("FREE PAD", pad)]
}

/// Play `events` — (frame, event), frames on block boundaries so the moment
/// is exact — for `frames` samples and return the mono sum. The block loop
/// is the engine's, so a strike lands where the engine would land it.
fn run(rig: &mut dyn Rig, events: &[(usize, Ev)], frames: usize) -> Vec<f32> {
    let mut events = events.to_vec();
    events.sort_by_key(|e| e.0);
    let total = frames.div_ceil(BLOCK) * BLOCK;
    let mut out = Vec::with_capacity(total);
    let mut scratch = vec![0.0_f32; BLOCK * 2];
    let mut next = 0usize;
    let mut frame = 0usize;
    while frame < total {
        while next < events.len() && events[next].0 < frame + BLOCK {
            match events[next].1 {
                Ev::On(p, v) => rig.on(p, f32::from(v) / 127.0),
                Ev::Off(p) => rig.off(p),
                Ev::Pedal(d) => rig.pedal(d),
            }
            next += 1;
        }
        scratch.fill(0.0);
        rig.render(&mut scratch, BLOCK, 2);
        out.extend(scratch.chunks_exact(2).map(|f| (f[0] + f[1]) * 0.5));
        frame += BLOCK;
    }
    out
}

/// Seconds to a block boundary at or before that instant — where `run`
/// will actually apply an event, and therefore where a measurement window
/// has to be centred.
fn at(secs: f32) -> usize {
    (secs * RATE) as usize / BLOCK * BLOCK
}

fn ms(m: f32) -> usize {
    (m * 0.001 * RATE) as usize
}

/// Largest one-sample step in `x[a..b]`: the first difference, the plain
/// measure of an edge.
fn d1(x: &[f32], a: usize, b: usize) -> f32 {
    let b = b.min(x.len());
    let mut m = 0.0_f32;
    for n in a.max(1)..b {
        m = m.max((x[n] - x[n - 1]).abs());
    }
    m
}

/// Largest second difference in `x[a..b]`. A sinusoid's second difference is
/// its first scaled by the tiny angle w, so a step — whose second difference
/// is the whole step, twice — stands 30 dB clear of a loud low bed that the
/// first difference alone would let it hide under. The chord tests need it.
fn d2(x: &[f32], a: usize, b: usize) -> f32 {
    let b = b.min(x.len());
    let mut m = 0.0_f32;
    for n in a.max(2)..b {
        m = m.max((x[n] - 2.0 * x[n - 1] + x[n - 2]).abs());
    }
    m
}

fn rms_in(x: &[f32], a: usize, b: usize) -> f32 {
    let b = b.min(x.len());
    if b <= a {
        return 0.0;
    }
    let e: f64 = x[a..b].iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (e / (b - a) as f64).sqrt() as f32
}

fn peak_in(x: &[f32], a: usize, b: usize) -> f32 {
    analyze::peak(&x[a.min(x.len())..b.min(x.len())])
}

/// What a fresh strike of `pitch` looks like on this rig: the steepest first
/// and second differences anywhere in the note — the attack, for everything
/// struck; wherever the slope of a slow envelope peaks, for a pad whose
/// first twenty milliseconds are silence — and its peak. These are the
/// ceilings the restrikes are held to.
struct Fresh {
    d1: f32,
    d2: f32,
    peak: f32,
}

/// Where the fresh strike lands in its own render: after a few blocks of
/// silence, so the step from nothing to its first sample is in the window
/// — the first version struck at frame zero and the ceiling never saw the
/// edge every restrike was then measured against.
const FRESH_AT: f32 = 0.01;

fn fresh(make: &dyn Fn() -> Box<dyn Rig>, pitch: u8, vel: u8, secs: f32) -> (Fresh, Vec<f32>) {
    let mut rig = make();
    let x = run(
        rig.as_mut(),
        &[(0, Ev::Pedal(true)), (at(FRESH_AT), Ev::On(pitch, vel))],
        (secs * RATE) as usize,
    );
    let f = Fresh {
        d1: d1(&x, 0, x.len()),
        d2: d2(&x, 0, x.len()),
        peak: analyze::peak(&x),
    };
    (f, x)
}

/// One restrike, measured. `onset` is where the event landed. The window
/// runs from 2 ms before it — where a leaked step would already have to be
/// absent — to 20 ms after, where the waveguide's delayed edge appears.
///
/// `over` is the old linear ceiling: this edge against the fresh strike's own
/// edge PLUS the bed's edge in the 20 ms before. For a linear mechanism that
/// sum cannot be exceeded, so anything above it would be energy the strike
/// did not put in — but the pickups are not linear and the sum is not their
/// ceiling, so it is printed and not judged. `dip` still judges, because it
/// asks a question no edge test can answer.
struct Edge {
    d1: f32,
    d2: f32,
    over_d1: f32,
    over_d2: f32,
    /// The quietest four milliseconds — a period of the lowest note this
    /// plays, so a level and not a phase — in the 12 ms after the strike,
    /// against the four before it, in dB. A hole — a voice cut to silence
    /// and re-attacked — reads as a deep negative here; a landing hammer
    /// mostly adds. `None` when there was no bed to dip from: a bed under
    /// -60 dBFS is a ratio of two noises.
    dip_db: Option<f32>,
}

/// How far a restrike may sit under the bed it landed on and still be a
/// hammer adding energy rather than a voice cut and re-attacked. A hammer
/// landing against the motion takes energy out — a real string restruck
/// in anti-phase gets quieter, and pianists know it — so this is not
/// zero; a cut is tens of decibels.
///
/// **Six decibels until the physics rework, eight after it, and the four
/// scenarios that pushed it are the four instruments that grew a pickup.**
/// Before the rework the two electric pianos carried modes at fixed
/// multiples of the fundamental — oscillators of their own, which a
/// restrike cancelling the fundamental left ringing to fill the hole.
/// Their harmonics are now made by the transducer out of the fundamental's
/// own motion, which is where a real instrument's come from, so when the
/// fundamental cancels they cancel with it. The Rhodes pair dipped 6.3 and
/// 7.1 dB on the rapid-eight scenario and the Wurlitzer pair 7.0 and 6.8
/// on the steal, while every one of the four reported a shape of 0.94 to
/// 1.01 and added 0.99 to 1.14 times what a fresh strike adds — that is a
/// strike doing its whole job, not a voice cut and re-attacked, which is
/// the fault this floor exists to catch and which shows up as a shape
/// nowhere near one. The pianos and the clavinets, whose harmonics do not
/// come from a transducer this way, stayed inside the old six.
///
/// The same number and the same reasoning sit on
/// `keys::tests::eight_pedalled_repeats_neither_stack_nor_leave_a_hole`;
/// if one moves, the other has to move with it, and only ever with a
/// measurement written next to it.
const DIP_FLOOR_DB: f32 = -8.0;

fn edge(x: &[f32], onset: usize, f: &Fresh) -> Edge {
    let bed1 = d1(x, onset.saturating_sub(ms(20.0)), onset);
    let bed2 = d2(x, onset.saturating_sub(ms(20.0)), onset);
    let e1 = d1(x, onset.saturating_sub(ms(2.0)), onset + ms(20.0));
    let e2 = d2(x, onset.saturating_sub(ms(2.0)), onset + ms(20.0));
    let before = rms_in(x, onset.saturating_sub(ms(4.0)), onset);
    let mut worst = f32::MAX;
    let mut a = onset;
    while a + ms(4.0) <= onset + ms(12.0) {
        worst = worst.min(rms_in(x, a, a + ms(4.0)));
        a += ms(1.0);
    }
    Edge {
        d1: e1,
        d2: e2,
        over_d1: analyze::db(e1 / (f.d1 + bed1).max(1e-12)),
        over_d2: analyze::db(e2 / (f.d2 + bed2).max(1e-12)),
        dip_db: (before > 1.0e-3).then(|| analyze::db(worst / before)),
    }
}

impl Edge {
    /// **Did the note go away?** The half of the verdict no edge test can
    /// make: `over` and the shape test both answer "did something step", and
    /// a voice cut to silence and re-attacked can leave a hole without
    /// stepping at all. This is the check that caught the biggest real fault
    /// of the previous pass, and it is deliberately unchanged.
    fn dip_ok(&self) -> bool {
        self.dip_db.is_none_or(|d| d >= DIP_FLOOR_DB)
    }

    fn dip(&self) -> String {
        match self.dip_db {
            Some(d) => format!("{d:+6.1} dB"),
            None => "   (no bed)".to_owned(),
        }
    }
}

/// The first and second difference of `got - bed` in one strike's window:
/// 2 ms before the onset, where a leaked step would already have to be
/// absent, to 20 ms after, where a waveguide's delayed edge appears.
fn diff_edge(got: &[f32], bed: &[f32], onset: usize) -> (f32, f32) {
    let d: Vec<f32> = got.iter().zip(bed).map(|(a, b)| a - b).collect();
    let (a, b) = (onset.saturating_sub(ms(2.0)), onset + ms(20.0));
    (d1(&d, a, b), d2(&d, a, b))
}

/// **What one event ADDED to a take, by rendering it again without that
/// event.** The bed is the same performance on a fresh instrument minus the
/// one thing under test, so the difference is exactly what that event
/// contributed through whatever the mechanism and the amp pass were doing at
/// the time — a magnet leaned on, a plate the reed is deep inside, a wah
/// wide open. `got` is the take the rest of the row describes, passed in
/// rather than rendered again, so the diagnostics and the verdict are two
/// readings of ONE render and can never disagree about which one they mean.
///
/// This is the module's own `added` helper (`tangent/src/keys/mod.rs`, and
/// the DX7 file keeps its own copy) brought over so the bench and the tests
/// ask a single question instead of two.
fn added(
    make: &dyn Fn() -> Box<dyn Rig>,
    base: &[(usize, Ev)],
    got: &[f32],
    onset: usize,
) -> (f32, f32) {
    let bed = run(make().as_mut(), base, got.len());
    diff_edge(got, &bed, onset)
}

/// **A hammer is not a step, and the difference is the SHAPE.**
///
/// What a landing strike added, as a multiple of what the same strike adds
/// from silence: `r1` for the first difference, `r2` for the second. Each of
/// the nonlinear pickups is a gain the strike arrives through, and a gain
/// multiplies both differences by the SAME number — so however far `r1` and
/// `r2` travel, and they travel a long way, the RATIO between them stays put.
/// A state cut and re-seeded does not behave that way: a step is broadband,
/// so it lands in `r2` many times harder than in `r1`.
///
/// The property to hold a landing strike to is therefore not "no louder than
/// a fresh one", which no nonlinear pickup can promise, but "the same shape
/// as a fresh one", which every honest superposition can keep whatever gain
/// it is read through.
struct Shape {
    r1: f32,
    r2: f32,
}

/// How far `ratio` may drift from one before a landing strike is called a
/// step rather than a hammer.
///
/// **Measured over all thirty-three scenarios, not chosen to make a print
/// go green.** With every instrument behaving, the ratio spans 0.42 to 1.67.
/// The three highest — 1.67 on the wah'd clav, 1.62 on the DX7 electric
/// piano, 1.56 on the pad — are all the RAPID scenario, and all three are
/// the same effect: the landing strike also moves a state the BED is read
/// through (the wah's filter, the DX7's LFO and feedback), so the difference
/// of two takes carries a modulation of the old note as well as the new one.
/// That is not a discontinuity and it is repeatable; the wah's seven
/// landings read 1.29 to 1.67 every run. Two and a half leaves half again as
/// much room above the worst of them, which the wah needs: a knob that opens
/// the filter further moves this number.
///
/// **What it costs, stated plainly.** For any signal the second difference
/// is at most twice the first, so `ratio` can never exceed twice a fresh
/// strike's own `d1/d2`. On the tine and the reed a fresh strike's onset IS
/// a one-sample step — their `fresh` row prints d1 and d2 equal — so their
/// ceiling is exactly 2.0 and NO tolerance at or above it can ever fire
/// there. That is not this number's fault and lowering it would not help:
/// with the historical re-seed put back, those instruments read 1.00, dead
/// centre, because a step measured against a step has the same shape. The
/// dip is what catches them, and it does, at 6.8 to 8.0 dB down. Where the
/// fresh attack is bandlimited the check has real teeth: the piano's
/// waveguide runs 7.5 to 1 between its own differences, and the same re-seed
/// fault reads 4.5, 6.9 and 7.9 there.
const SHAPE_TOL: f32 = 2.5;

/// Below this, the strike put nothing into the window worth reading and the
/// ratio is two noises divided — the same reason `dip_db` is `None` under a
/// silent bed. The pad's first five rapid landings are here: sixty
/// milliseconds into a slow attack a repeat has added less than half a
/// percent of a fresh strike's edge, and there is no shape in that. The
/// smallest honest reading anywhere is 0.43, so the floor sits in open
/// space. A strike that adds nothing AND leaves a hole is still a fault:
/// the dip sees it, and the dip is judged separately.
const SHAPE_FLOOR: f32 = 0.01;

impl Shape {
    fn of((a1, a2): (f32, f32), f: &Fresh) -> Self {
        Shape {
            r1: a1 / f.d1.max(1e-12),
            r2: a2 / f.d2.max(1e-12),
        }
    }

    /// One for a strike that arrives through any gain whatsoever, however
    /// large or small that gain is; far above one for a step.
    fn ratio(&self) -> f32 {
        self.r2 / self.r1.max(1e-12)
    }

    /// Whether there is a shape here at all: see `SHAPE_FLOOR`.
    fn readable(&self) -> bool {
        self.r1 >= SHAPE_FLOOR
    }

    fn ok(&self) -> bool {
        !self.readable() || self.ratio() <= SHAPE_TOL
    }
}

/// The second line of every scenario: what the strike added, and the verdict.
/// The columns line up under the diagnostics above them on purpose — `added`
/// d1 beside measured d1 is the comparison the reader is making anyway.
fn print_added(s: &Shape, ok: bool) {
    println!(
        "      added    d1 {:7.2}x  d2 {:7.2}x  shape {:6.2}  {}",
        s.r1,
        s.r2,
        s.ratio(),
        verdict(ok)
    );
}

/// How far above one fresh strike's peak eight of them at sixty milliseconds
/// may stack.
///
/// **Derived, and taken from the module's own contract rather than invented
/// here.** The strike's energy cap holds a re-struck mode to a quarter over
/// one strike's own throw — 1.9 dB of DISPLACEMENT — and the pickups are
/// expansive, so at the output that reads as more than it does on the metal.
/// The worst measured is the reed piano at 3.2 dB, which is the plate's curve
/// doing exactly what the growl is, and `STACK_MARGIN` in `keys/mod.rs` is
/// the round number above it. This figure was 3.0: BELOW the instrument's own
/// documented worst case, which made it a second false alarm of precisely the
/// kind this pass came to remove, firing on the same reed piano.
const STACK_MARGIN_DB: f32 = 4.0;

fn verdict(ok: bool) -> &'static str {
    if ok {
        "ok"
    } else {
        "POP"
    }
}

/// **The consecutive-strike report.** For every patch and the DX7: a note
/// restruck under the pedal, a voice stolen out of a full pool, and eight
/// strikes at 60 ms. Each scenario is rendered twice — with the event under
/// test and without — so the verdict is the SHAPE of what the strike added
/// and not the height of an edge read at an operating point the fresh
/// reference never visits. Returns how many popped, so a script can fail
/// on it.
///
/// Two lines per scenario. The first is diagnostic: the edge as measured,
/// how it sits against the old linear ceiling, and the dip. The second is
/// the verdict: what the strike added, as a multiple of a fresh strike's
/// own first and second differences, and the ratio between those two
/// multiples. A pop is `shape` far above one, or a dip through the floor.
fn report_restrike() -> usize {
    const PITCH: u8 = 60;
    const VEL: u8 = 102;
    let mut pops = 0usize;
    println!("consecutive strikes: the SHAPE of what each strike added, against a fresh strike");
    println!("  shape = (added d2 / fresh d2) / (added d1 / fresh d1); one is a hammer through");
    println!("  any gain, large is a step. over = the old LINEAR ceiling, printed but not judged;");
    println!("  the pickups are nonlinear and it accuses them. dip still judges: a hole is real.");
    for (label, make) in rigs() {
        let (f, _) = fresh(make.as_ref(), PITCH, VEL, 1.0);
        println!("\n  {label}");
        println!(
            "    fresh      d1 {:8.5}  d2 {:8.5}  peak {:+6.1} dBFS",
            f.d1,
            f.d2,
            analyze::db(f.peak)
        );

        // Restrike: key up at 0.25 s under the pedal, down again at 0.5 s.
        let onset = at(0.5);
        let frames = (1.0 * RATE) as usize;
        let base = [
            (0usize, Ev::Pedal(true)),
            (0, Ev::On(PITCH, VEL)),
            (at(0.25), Ev::Off(PITCH)),
        ];
        let mut ev = base.to_vec();
        ev.push((onset, Ev::On(PITCH, VEL)));
        let x = run(make().as_mut(), &ev, frames);
        let e = edge(&x, onset, &f);
        let s = Shape::of(added(make.as_ref(), &base, &x, onset), &f);
        let ok = s.ok() && e.dip_ok();
        pops += usize::from(!ok);
        println!(
            "    restrike   d1 {:8.5}  d2 {:8.5}  over {:+5.1} {:+5.1} dB  dip {}",
            e.d1,
            e.d2,
            e.over_d1,
            e.over_d2,
            e.dip()
        );
        print_added(&s, ok);

        // Steal: fill the pool with held keys, release ONE under the pedal
        // (the stealing policy takes released voices first, so it is the
        // one that goes, and it is still at full level), then a pitch the
        // pool does not hold. On the DX7 the oldest keyed voice goes; the
        // first note struck is the oldest, and it is the same pitch.
        //
        // The bed here is the same crowded performance with the newcomer
        // left out, so the difference carries BOTH halves of a steal: what
        // the new note added, and what the corpse stopped contributing. A
        // slot handed over cleanly reads as a hammer; a voice cut mid-cycle
        // is a step, and it is the step this scenario exists to find.
        let is_dx7 = label.starts_with("DX7");
        let pool = if is_dx7 { dx7::synth::VOICES } else { keys::VOICES };
        let victim = 30u8;
        let newcomer = 100u8;
        let mut base = vec![(0usize, Ev::Pedal(true))];
        for i in 0..pool {
            base.push((0, Ev::On(24 + i as u8, VEL)));
        }
        base.push((at(0.1), Ev::Off(victim)));
        let mut ev = base.clone();
        ev.push((onset, Ev::On(newcomer, VEL)));
        let (fn_, _) = fresh(make.as_ref(), newcomer, VEL, 0.6);
        let x = run(make().as_mut(), &ev, (0.8 * RATE) as usize);
        let e = edge(&x, onset, &fn_);
        let s = Shape::of(added(make.as_ref(), &base, &x, onset), &fn_);
        let ok = s.ok() && e.dip_ok();
        pops += usize::from(!ok);
        println!(
            "    steal      d1 {:8.5}  d2 {:8.5}  over {:+5.1} {:+5.1} dB  dip {}",
            e.d1,
            e.d2,
            e.over_d1,
            e.over_d2,
            e.dip()
        );
        print_added(&s, ok);

        // Rapid repeat: eight strikes 60 ms apart, key up 30 ms after each,
        // pedal down throughout. Every landing is judged on its shape, none
        // may leave a hole, and the whole thing is held to the stacking
        // margin — superposition that piles into clipping is the other way
        // to be wrong, and no shape test can see it.
        let mut ev = vec![(0usize, Ev::Pedal(true))];
        let mut onsets = Vec::new();
        for k in 0..8 {
            let t = at(k as f32 * 0.060);
            onsets.push(t);
            ev.push((t, Ev::On(PITCH, VEL)));
            ev.push((at(k as f32 * 0.060 + 0.030), Ev::Off(PITCH)));
        }
        let x = run(make().as_mut(), &ev, (1.0 * RATE) as usize);

        // Seven landings need seven beds, and each bed is the next take's
        // performance one strike shorter — so render the eight prefixes once
        // and difference them in consecutive pairs rather than rendering
        // fourteen takes. The eighth prefix is `x` itself over this span.
        let cut_frames = onsets[7] + ms(25.0);
        let cuts: Vec<Vec<f32>> = (1..=8)
            .map(|upto| {
                let mut e = vec![(0usize, Ev::Pedal(true))];
                for j in 0..upto {
                    e.push((onsets[j], Ev::On(PITCH, VEL)));
                    e.push((at(j as f32 * 0.060 + 0.030), Ev::Off(PITCH)));
                }
                run(make().as_mut(), &e, cut_frames)
            })
            .collect();

        let mut worst1 = f32::MIN;
        let mut worst2 = f32::MIN;
        let mut worst_dip = f32::MAX;
        let mut dips_ok = true;
        // `None` until a landing puts something readable into its window:
        // the pad's early repeats add nothing measurable, and a ratio taken
        // there would be noise over noise deciding the verdict.
        let mut worst: Option<Shape> = None;
        for (k, &o) in onsets.iter().enumerate().skip(1) {
            let e = edge(&x, o, &f);
            worst1 = worst1.max(e.over_d1);
            worst2 = worst2.max(e.over_d2);
            worst_dip = worst_dip.min(e.dip_db.unwrap_or(0.0));
            dips_ok &= e.dip_ok();
            let s = Shape::of(diff_edge(&cuts[k], &cuts[k - 1], o), &f);
            if s.readable() && worst.as_ref().is_none_or(|w| s.ratio() > w.ratio()) {
                worst = Some(s);
            }
        }
        let over_peak = analyze::db(peak_in(&x, 0, x.len()) / f.peak.max(1e-12));
        let ok = worst.as_ref().is_none_or(Shape::ok) && dips_ok && over_peak <= STACK_MARGIN_DB;
        pops += usize::from(!ok);
        println!(
            "    rapid x8   worst over {:+5.1} {:+5.1} dB  dip {:+6.1} dB  peak {:+5.1} dB vs fresh",
            worst1, worst2, worst_dip, over_peak,
        );
        match &worst {
            Some(w) => print_added(w, ok),
            None => println!(
                "      added    nothing measurable in any of the seven windows      {}",
                verdict(ok)
            ),
        }
    }
    println!("\n  {pops} scenario(s) pop");
    pops
}

/// **The first-strike pin.** One FNV hash per render of a first strike from
/// silence — every patch at its defaults and at both ends of every knob, and
/// the DX7 patches — so a change to restrikes can prove, by diff of two
/// printouts, that it changed nothing a first strike does. A hash rather
/// than a file: the point is a before/after that fits on one screen.
fn report_pin() {
    fn fnv(x: &[f32]) -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325_u64;
        for s in x {
            for b in s.to_bits().to_le_bytes() {
                h ^= u64::from(b);
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
            }
        }
        h
    }
    let ev = [(0usize, Ev::Pedal(true)), (0, Ev::On(60, 102)), (at(0.3), Ev::Off(60))];
    let frames = (0.5 * RATE) as usize;
    for &patch in &Patch::ALL {
        let mut line = format!("  {:<12}", patch_label(patch));
        for knob in 0..=keys::MAX_PARAMS {
            for end in [0.0_f32, 1.0] {
                if knob == keys::MAX_PARAMS && end > 0.0 {
                    continue;
                }
                let mut k = Keys::new(RATE);
                k.set_patch(patch);
                if knob < keys::MAX_PARAMS {
                    k.set_param(knob, end);
                }
                let x = run(&mut k, &ev, frames);
                line.push_str(&format!(" {:016x}", fnv(&x)));
            }
        }
        println!("{line}");
    }
    for (name, voice) in dx7_patches() {
        let mut d = dx7::synth::Dx7::new(RATE);
        d.set_voice(voice);
        let x = run(&mut d, &ev, frames);
        println!("  {:<12} {:016x}", format!("DX7 {name}"), fnv(&x));
    }
}

/// Print the samples around a restrike, the bed alone, and their
/// difference beside a fresh strike: what superposition SHOULD look like
/// next to what the mechanism did. A debugging aid, kept because the next
/// person to touch a strike path will want it.
fn probe(which: &str) {
    const PITCH: u8 = 60;
    const VEL: u8 = 102;
    let Some((label, make)) = rigs().into_iter().find(|(l, _)| l.starts_with(which)) else {
        eprintln!("no rig starts with {which}");
        return;
    };
    let onset = at(0.5);
    let base = [
        (0usize, Ev::Pedal(true)),
        (0, Ev::On(PITCH, VEL)),
        (at(0.25), Ev::Off(PITCH)),
    ];
    let mut with = base.to_vec();
    with.push((onset, Ev::On(PITCH, VEL)));
    let mut rig = make();
    let bed = run(rig.as_mut(), &base, (0.6 * RATE) as usize);
    let mut rig = make();
    let re = run(rig.as_mut(), &with, (0.6 * RATE) as usize);
    let (_, fr) = fresh(make.as_ref(), PITCH, VEL, 0.1);
    println!("{label}: n   bed        restrike   diff       fresh");
    for n in onset.saturating_sub(3)..onset + 24 {
        let d = re[n] - bed[n];
        let f = if n >= onset { fr[n - onset] } else { 0.0 };
        println!(
            "  {:>4}  {:+.6}  {:+.6}  {:+.6}  {:+.6}",
            n as isize - onset as isize,
            bed[n],
            re[n],
            d,
            f
        );
    }
}

/// Print, for every strike of the rapid repeat, what the level did across
/// the event and what each of the first three partials did with it.
///
/// **The dip needs a different instrument from the edge.** `over` answers
/// "did something step"; a dip answers "did something go away", and the two
/// have opposite cures — so this prints the millisecond-by-millisecond level
/// either side of each onset AND the magnitude of the fundamental, its
/// octave and its twelfth before and after, because a fundamental that
/// halves while the octave grows is superposition landing out of phase, and
/// every partial dropping at once is a gain that moved.
fn probe_rapid(which: &str) {
    const PITCH: u8 = 60;
    const VEL: u8 = 102;
    let Some((label, make)) = rigs().into_iter().find(|(l, _)| l.starts_with(which)) else {
        eprintln!("no rig starts with {which}");
        return;
    };
    let mut ev = vec![(0usize, Ev::Pedal(true))];
    let mut onsets = Vec::new();
    for k in 0..8 {
        let t = at(k as f32 * 0.060);
        onsets.push(t);
        ev.push((t, Ev::On(PITCH, VEL)));
        ev.push((at(k as f32 * 0.060 + 0.030), Ev::Off(PITCH)));
    }
    let mut rig = make();
    let x = run(rig.as_mut(), &ev, (1.0 * RATE) as usize);
    let (f, _) = fresh(make.as_ref(), PITCH, VEL, 1.0);
    let f0 = 440.0 * 2.0_f32.powf((f32::from(PITCH) - 69.0) / 12.0);
    println!("{label}: rapid x8, level in 1 ms steps around each onset, then partials");
    for (k, &o) in onsets.iter().enumerate().skip(1) {
        let e = edge(&x, o, &f);
        print!("  #{k} over {:+5.1} {:+5.1}  ", e.over_d1, e.over_d2);
        for j in -4_isize..16 {
            let a = (o as isize + j * ms(1.0) as isize).max(0) as usize;
            print!("{:+5.0}", analyze::db(rms_in(&x, a, a + ms(1.0))));
        }
        let win = ms(20.0);
        let before = &x[o.saturating_sub(win)..o];
        let after = &x[o..(o + win).min(x.len())];
        print!("   |");
        for n in [1.0_f32, 2.0, 3.0] {
            let b = analyze::goertzel_hann(before, f0 * n, RATE);
            let a = analyze::goertzel_hann(after, f0 * n, RATE);
            print!(" {:+5.1}", analyze::db(a / b.max(1e-12)));
        }
        println!();
    }
    // **What each strike ADDED, against what the same strike adds from
    // silence.** The bed is the run stopped one strike short, so the
    // difference is that strike's own contribution through whatever the amp
    // pass was doing at the time. A mechanism that superposes gives a ratio
    // of one. A mechanism that steps gives a d2 ratio far above its d1
    // ratio, because a step is broadband and a gain is not; a state-dependent
    // gain downstream — a pickup leaned on, a wah wide open — moves BOTH by
    // the same factor, which is how to tell the two apart.
    // Through the same `Shape` the report's verdict is made of, so the
    // number the probe shows a reader and the number the bench judged are
    // the same number and cannot drift apart.
    println!("  strike  d1 diff/fresh   d2 diff/fresh    shape  (equal ratios = a gain, not a step)");
    for k in 1..8 {
        let cut = |upto: usize| {
            let mut e = vec![(0usize, Ev::Pedal(true))];
            for j in 0..upto {
                e.push((onsets[j], Ev::On(PITCH, VEL)));
                e.push((at(j as f32 * 0.060 + 0.030), Ev::Off(PITCH)));
            }
            run(make().as_mut(), &e, onsets[k] + ms(25.0))
        };
        let bed = cut(k);
        let with = cut(k + 1);
        let s = Shape::of(diff_edge(&with, &bed, onsets[k]), &f);
        println!(
            "    #{k}    {:8.2}        {:8.2}     {:6.2}{}",
            s.r1,
            s.r2,
            s.ratio(),
            if s.readable() {
                ""
            } else {
                "   (nothing measurable added; not judged)"
            }
        );
    }
    // Where the worst second difference of the whole run actually sits, and
    // the samples around it: an `over` that is not at the onset is not the
    // strike's edge at all.
    let mut worst = (0.0_f32, 0usize);
    for n in 2..x.len() {
        let d = (x[n] - 2.0 * x[n - 1] + x[n - 2]).abs();
        if d > worst.0 {
            worst = (d, n);
        }
    }
    let (_, fr) = fresh(make.as_ref(), PITCH, VEL, 1.0);
    let mut wf = (0.0_f32, 0usize);
    for n in 2..fr.len() {
        let d = (fr[n] - 2.0 * fr[n - 1] + fr[n - 2]).abs();
        if d > wf.0 {
            wf = (d, n);
        }
    }
    println!(
        "  worst d2 {:.6} at frame {} ({:+.1} ms from onset {}); fresh worst {:.6} at {:.1} ms",
        worst.0,
        worst.1,
        (worst.1 as f32 - onsets.iter().rev().find(|&&o| o <= worst.1).copied().unwrap_or(0) as f32)
            / RATE
            * 1000.0,
        onsets.iter().rev().find(|&&o| o <= worst.1).copied().unwrap_or(0),
        wf.0,
        wf.1 as f32 / RATE * 1000.0,
    );
    for n in worst.1.saturating_sub(6)..(worst.1 + 6).min(x.len()) {
        println!("    {:>7}  {:+.7}", n, x[n]);
    }
}

/// The steal, sample by sample: the pool full, one voice buried and faded,
/// and a pitch the pool does not hold arriving on the slot it vacated. The
/// bed here is the same performance with the newcomer left out, so the
/// difference is exactly what the new note added through whatever the amp
/// pass was doing at that moment.
fn probe_steal(which: &str) {
    const VEL: u8 = 102;
    let Some((label, make)) = rigs().into_iter().find(|(l, _)| l.starts_with(which)) else {
        eprintln!("no rig starts with {which}");
        return;
    };
    let is_dx7 = label.starts_with("DX7");
    let pool = if is_dx7 { dx7::synth::VOICES } else { keys::VOICES };
    let onset = at(0.5);
    let mut base = vec![(0usize, Ev::Pedal(true))];
    for i in 0..pool {
        base.push((0, Ev::On(24 + i as u8, VEL)));
    }
    base.push((at(0.1), Ev::Off(30)));
    let mut with = base.clone();
    with.push((onset, Ev::On(100, VEL)));
    let mut rig = make();
    let bed = run(rig.as_mut(), &base, (0.6 * RATE) as usize);
    let mut rig = make();
    let st = run(rig.as_mut(), &with, (0.6 * RATE) as usize);
    let (_, fr) = fresh(make.as_ref(), 100, VEL, 0.1);
    println!("{label} steal: n   bed        steal      diff       fresh");
    for n in onset.saturating_sub(3)..onset + 40 {
        let f = if n >= onset { fr[n - onset] } else { 0.0 };
        println!(
            "  {:>4}  {:+.6}  {:+.6}  {:+.6}  {:+.6}",
            n as isize - onset as isize,
            bed[n],
            st[n],
            st[n] - bed[n],
            f
        );
    }
    println!(
        "  bed d1 {:.5} d2 {:.5} | diff d1 {:.5} d2 {:.5} | fresh d1 {:.5} d2 {:.5}",
        d1(&bed, onset - ms(2.0), onset + ms(20.0)),
        d2(&bed, onset - ms(2.0), onset + ms(20.0)),
        d1(
            &st.iter().zip(&bed).map(|(a, b)| a - b).collect::<Vec<_>>(),
            onset - ms(2.0),
            onset + ms(20.0)
        ),
        d2(
            &st.iter().zip(&bed).map(|(a, b)| a - b).collect::<Vec<_>>(),
            onset - ms(2.0),
            onset + ms(20.0)
        ),
        d1(&fr, 0, fr.len()),
        d2(&fr, 0, fr.len()),
    );
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if let (Some("probe"), Some(which)) = (args.get(1).map(String::as_str), args.get(2)) {
        probe(which);
        return;
    }
    if let (Some("steal"), Some(which)) = (args.get(1).map(String::as_str), args.get(2)) {
        probe_steal(which);
        return;
    }
    if let (Some("rapid"), Some(which)) = (args.get(1).map(String::as_str), args.get(2)) {
        probe_rapid(which);
        return;
    }
    match args.get(1).map(String::as_str) {
        Some("restrike") => {
            let pops = report_restrike();
            std::process::exit(i32::from(pops > 0));
        }
        Some("pin") => {
            report_pin();
            return;
        }
        _ => {}
    }
    let patch = match args.get(1).map(String::as_str) {
        Some("ep") => Patch::EpMk1,
        Some("suitcase") => Patch::EpSuitcase,
        Some("piano2") => Patch::Piano2,
        Some("clav") => Patch::Clav,
        Some("clavwah") => Patch::ClavWah,
        Some("reed" | "wurli") => Patch::Wurli200,
        Some("reedamp" | "wurli140") => Patch::Wurli140,
        Some("egrand") => Patch::ElectricGrand,
        Some("guitar") => Patch::Guitar,
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
