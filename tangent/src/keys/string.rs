//! The piano: a struck string, and the box it is strung on.
//!
//! **The model, in one paragraph.** A string is a delay line with a filter in
//! the feedback loop. Sound travels along it, reflects off the bridge and the
//! agraffe, and comes back slightly quieter and slightly slower at the top —
//! which is the whole of why a piano note is bright at the start and mellow at
//! the end. The delay's LENGTH is the pitch. The filter's shape is the decay,
//! and it has to be frequency-dependent, because a real string loses its top
//! octave in a second and its fundamental in half a minute. An extra all-pass
//! in the loop makes the high partials travel FASTER than the low ones, which
//! is stiffness, which is why a piano's partials are stretched sharp and why
//! two pianos tuned to the same fundamental still beat against each other.
//!
//! **What the hammer is.** Not a click. Felt compresses, and the harder it is
//! driven the stiffer it gets, so a loud note is not a quiet note turned up:
//! the contact lasts a couple of milliseconds at pianissimo and under one at
//! fortissimo, and a shorter contact injects energy further up the series.
//! That is the single most important thing in this file, because it is what
//! makes the instrument respond to a player rather than to a volume knob —
//! and the pulse's SHAPE matters as much as its width; see `strike` for the
//! two shapes that failed before the Gaussian.

use super::Patch;

/// Longest delay any string needs: A0 at 27.5 Hz, at 96 kHz, with room for the
/// fractional part and the tuning correction. Fixed and allocated once, so no
/// note can ever cause an allocation.
const MAX_DELAY: usize = 4096;

/// How many strings a unison has. Real pianos use one in the low bass, two
/// through the tenor and three above it; three everywhere costs little and the
/// bass ones are simply detuned less.
const UNISON: usize = 3;

/// A one-pole lowpass, which is the loss in the loop.
#[derive(Clone, Copy, Default)]
struct Lp {
    z: f32,
    a: f32,
}

impl Lp {
    fn tick(&mut self, x: f32) -> f32 {
        self.z += self.a * (x - self.z);
        self.z
    }
}

/// A first-order all-pass. Cascade them and you have dispersion: a frequency
/// dependent delay, which is stiffness.
#[derive(Clone, Copy, Default)]
struct Ap {
    a: f32,
    x1: f32,
    y1: f32,
}

impl Ap {
    fn tick(&mut self, x: f32) -> f32 {
        let y = self.a * (x - self.y1) + self.x1;
        self.x1 = x;
        self.y1 = y;
        y
    }
}

/// One string of a unison.
struct Str {
    buf: [f32; MAX_DELAY],
    write: usize,
    /// Delay in samples, fractional.
    len: f32,
    loss: Lp,
    disp: [Ap; 4],
    /// Loop gain, just under one.
    gain: f32,
}

impl Str {
    fn new() -> Self {
        Str {
            buf: [0.0; MAX_DELAY],
            write: 0,
            len: 100.0,
            loss: Lp::default(),
            disp: [Ap::default(); 4],
            gain: 0.999,
        }
    }

    fn clear(&mut self) {
        self.buf.fill(0.0);
        self.loss = Lp { z: 0.0, a: self.loss.a };
        for d in &mut self.disp {
            d.x1 = 0.0;
            d.y1 = 0.0;
        }
    }

    /// Read the delay line `d` samples back, linearly interpolated.
    fn read(&self, d: f32) -> f32 {
        let d = d.clamp(1.0, (MAX_DELAY - 2) as f32);
        let i = d.floor();
        let frac = d - i;
        let back = i as usize;
        let a = self.buf[(self.write + MAX_DELAY - back) % MAX_DELAY];
        let b = self.buf[(self.write + MAX_DELAY - back - 1) % MAX_DELAY];
        a + (b - a) * frac
    }

    /// Advance one sample. `inject` is this sample of the hammer pulse, and
    /// it is a PARAMETER for a reason that cost a day of measurement: it is
    /// superposed onto the value being written at the head. The first
    /// version poured it in through `excite(0, ..)` — into the very slot
    /// this function then overwrites — so the positive half of the hammer
    /// pair never entered the string at all. What circulated was the lone
    /// negative tap: a one-sided blob — waveform asymmetry 1.000 at low
    /// velocity on the bench, a thump for a soft note, and no strike comb
    /// anywhere because the comb IS the pair.
    fn tick(&mut self, inject: f32) -> f32 {
        let out = self.read(self.len);
        let mut v = self.loss.tick(out) * self.gain;
        for d in &mut self.disp {
            v = d.tick(v);
        }
        // Guard the loop. A filter coefficient that drifts, a denormal that
        // stalls, a NaN from anywhere — all of them live forever in a feedback
        // path, and a NaN escaping here would poison the master bus for the
        // rest of the session rather than for the rest of the note.
        if !v.is_finite() {
            v = 0.0;
        }
        self.buf[self.write] = v + inject;
        self.write = (self.write + 1) % MAX_DELAY;
        out
    }

    /// Put energy in `at` samples behind the write head — the hammer pair's
    /// far tap. Adds, never assigns: the slot already holds circulating
    /// string, and a hammer lands ON a string, it does not replace it.
    fn excite(&mut self, at: usize, v: f32) {
        let i = (self.write + MAX_DELAY - at.min(MAX_DELAY - 1)) % MAX_DELAY;
        self.buf[i] += v;
    }
}

/// Phase delay, in samples, of everything in the loop besides the delay line
/// itself: the loss one-pole (pole `p`) and the four dispersion all-passes
/// (coefficient `ap`), evaluated AT the note's own frequency `w`.
///
/// The first version used the DC formulas — (1-a)/(1+a) and so on — which
/// happened to land within a cent at C4 and would have drifted further up
/// the keyboard, and worse, would have quietly broken every time the loss
/// design below changes its pole. Evaluating the actual phase at the actual
/// frequency keeps "0 cents at the fundamental" true by construction rather
/// than by luck.
fn loop_phase_delay(w: f32, p: f32, ap: f32) -> f32 {
    let (sw, cw) = w.sin_cos();
    // One-pole H = g(1-p)/(1 - p e^{-jw}): real numerator, so the phase is
    // minus the denominator's argument.
    let lp = (p * sw).atan2(1.0 - p * cw);
    // All-pass H = (a + e^{-jw})/(1 + a e^{-jw}).
    let ap_ph = (-sw).atan2(ap + cw) - (-ap * sw).atan2(1.0 + ap * cw);
    (lp - 4.0 * ap_ph) / w.max(1e-6)
}

/// Solve the loop's loss filter so the decay is a designed quantity.
///
/// Two targets, two knobs. The loop's per-pass magnitude at the fundamental
/// sets the note's t60; its magnitude at one reference frequency up the
/// series sets how much faster the top dies. A one-pole with a gain in
/// front has exactly two degrees of freedom, so both are hit exactly. The
/// first version set the coefficient from a hand schedule instead, and once
/// the bench's leakage bug was fixed the schedule stood revealed: C4's
/// third partial gone in 1.0 s, the fourth in 0.72 s, nothing above the
/// fourth measurable at ANY velocity — which is why the velocity table was
/// flat. A string that cannot sustain its top cannot brighten.
///
/// For |H(w)| = g(1-p)/|1 - p e^{-jw}| with per-pass targets rho1 at w1 and
/// rho2 at w2, the ratio eliminates g and leaves a quadratic in the pole p;
/// g then follows from the first target. Between and beyond the two points
/// the one-pole's magnitude falls monotonically with frequency, so every
/// partial's t60 decreases strictly with n — the property the whole fix was
/// for — without being individually placed.
fn design_loss(w1: f32, w2: f32, rho1: f32, rho2: f32) -> (f32, f32) {
    let (c1, c2) = (w1.cos(), w2.cos());
    let r = (rho1 / rho2) * (rho1 / rho2);
    let q = (c2 - r * c1) / (1.0 - r);
    let p = if r > 1.0 && q > 1.0 {
        q - (q * q - 1.0).sqrt()
    } else {
        // A degenerate request — flat or rising t60 with frequency — gets a
        // nearly transparent filter rather than a wrong one. Reachable only
        // at the extreme treble where both targets clamp.
        0.02
    }
    .clamp(0.02, 0.95);
    let g = rho1 * (1.0 - 2.0 * p * c1 + p * p).sqrt() / (1.0 - p);
    (p, g.min(0.999_9))
}

/// One key of the piano.
pub struct PianoVoice {
    rate: f32,
    strings: [Str; UNISON],
    /// The hammer pulse still being poured in, in samples remaining.
    pulse: u32,
    /// Gaussian-pulse state: amplitude, the running value g, the running
    /// step ratio r, and r's own constant per-sample ratio. Two multiplies a
    /// sample generate the exact curve. See `strike` for why a Gaussian.
    pulse_amp: f32,
    pulse_g: f32,
    pulse_r: f32,
    pulse_rstep: f32,
    /// Where along the string the hammer lands, in samples — the comb notch.
    strike: usize,
    /// Damper: how fast the note is being killed. 1.0 is free.
    damp: f32,
    damp_to: f32,
    /// A peak follower, for the stealing policy and for `finished`.
    env: f32,
    done: bool,
}

impl PianoVoice {
    pub fn new(sample_rate: f32) -> Self {
        PianoVoice {
            rate: sample_rate,
            strings: [Str::new(), Str::new(), Str::new()],
            pulse: 0,
            pulse_amp: 0.0,
            pulse_g: 0.0,
            pulse_r: 0.0,
            pulse_rstep: 0.0,
            strike: 8,
            damp: 1.0,
            damp_to: 1.0,
            env: 0.0,
            done: true,
        }
    }

    pub fn strike(&mut self, pitch: u8, velocity: f32) {
        self.strike_full(pitch, velocity, 1.0, 0.5);
    }

    /// `strike` with the panel's VOICING knob: 0 mellow, 1 bright, 0.5 the
    /// bench tuning exactly. See `strike_full` for what the knob touches.
    pub fn strike_voiced(&mut self, pitch: u8, velocity: f32, voicing: f32) {
        self.strike_full(pitch, velocity, 1.0, voicing);
    }

    /// `strike`, with the unison detune scaled: 1.0 is the instrument, 0.0
    /// collapses the three strings onto one frequency.
    ///
    /// **Exists for the bench, and only for the bench.** Three live detuned
    /// strings start in phase and drift apart, and the drift sculpts the
    /// ensemble envelope by 15 dB either way on a period of seconds — the
    /// two-stage decay and wandering sustain a real unison has, and exactly
    /// what the detune is FOR. But a t60 fitted to that curve measures the
    /// beat geometry, not the loss filter: the bench's per-partial table once
    /// showed n2 outliving n1 through nothing but a null in n1's beat. The
    /// t60 column is therefore measured with the unison collapsed, where the
    /// decay is the design and nothing else. `mod.rs` never calls this.
    pub fn strike_spread(&mut self, pitch: u8, velocity: f32, spread: f32) {
        self.strike_full(pitch, velocity, spread, 0.5);
    }

    fn strike_full(&mut self, pitch: u8, velocity: f32, spread: f32, voicing: f32) {
        let f0 = 440.0 * 2.0_f32.powf((f32::from(pitch) - 69.0) / 12.0);
        let total = (self.rate / f0).clamp(4.0, (MAX_DELAY - 4) as f32);

        // **Inharmonicity.** B is the stiffness coefficient in
        // f_n = n*f0*sqrt(1 + B*n^2). It is smallest in the tenor and rises
        // both ways: short thick bass strings and very short treble ones are
        // both stiff for their length. These are the shape of a measured
        // grand, not a constant.
        let k = f32::from(pitch);
        let b = if k < 48.0 {
            // Bass: climbs steeply downward from about 2e-4 to 4e-3.
            2.0e-4 * 2.0_f32.powf((48.0 - k) / 9.0)
        } else {
            // Tenor up: from 2e-4 to about 1e-3 at the top.
            2.0e-4 * 2.0_f32.powf((k - 48.0) / 22.0)
        };

        // Decay, as two designed numbers instead of one guessed coefficient.
        // The fundamental's t60 runs from ~18 s at A0 down to about a second
        // at the top — a solo voicing: singing, not a concert hall's half
        // minute. The reference point up the series (partial 6, or as high as
        // the sample rate allows) gets just over a quarter of that, and
        // `design_loss` hands every partial in between a strictly decreasing
        // t60. Both numbers were tuned against the bench's per-partial table;
        // this pair puts C4 at n1 4.8 s falling steadily to n7 0.8 s, which
        // decays like felt-damped wire rather than either an organ (flat
        // t60) or a clavinet (no top at all).
        let t60_f = (18.0 * (-(k - 21.0) / 30.0).exp()).clamp(1.0, 18.0);
        let f_hi = (6.0 * f0).min(0.40 * self.rate);
        let t60_h = (t60_f * 0.28).max(0.08);
        let w1 = std::f32::consts::TAU * f0 / self.rate;
        let w2 = std::f32::consts::TAU * f_hi / self.rate;
        // Per-pass magnitudes: the loop is traversed f0 times a second, so
        // 60 dB in t60 seconds means 10^(-3/(t60*f0)) per pass.
        let rho1 = 10.0_f32.powf(-3.0 / (t60_f * f0));
        let rho2 = 10.0_f32.powf(-3.0 / (t60_h * f0));
        let (p, gain) = design_loss(w1, w2, rho1, rho2);

        // Dispersion, as a cascade of first-order all-passes. Each one adds a
        // delay that shrinks with frequency; the coefficient below is the
        // standard closed-form fit against B, and four sections is what fits
        // the budget. Fewer sections still stretches the low partials, which
        // is the audible part; what is lost is accuracy at the top, where the
        // ear is measuring beats and not pitch.
        let disp_a = -(1.0 - (b * 1400.0).min(0.92)).sqrt() * 0.92;
        for s in &mut self.strings {
            for d in &mut s.disp {
                d.a = disp_a;
            }
            s.loss.a = 1.0 - p;
        }
        // The loop must be SHORTENED by the delay the filters already
        // contribute, or every note plays flat — the single most common bug
        // in a waveguide, and it worsens as pitch rises because the filters'
        // delay is a fixed number of samples while the period shrinks.
        // Evaluated at f0 itself, not approximated at DC: see
        // `loop_phase_delay` for what the DC shortcut would have cost.
        let phase_delay = loop_phase_delay(w1, p, disp_a);

        // The unison. Two or three strings a few cents apart is where the
        // beating comes from, and the beating is why a real piano's sustain
        // wanders instead of sitting still. Only the delay LINE detunes; the
        // filters' phase delay is common to all three.
        const CENTS: [f32; UNISON] = [0.0, 0.9, -1.1];
        for (i, s) in self.strings.iter_mut().enumerate() {
            let det = 2.0_f32.powf(CENTS[i] * spread / 1200.0);
            s.len = (total / det - phase_delay).max(4.0);
            s.gain = gain;
            s.clear();
        }

        // Strike position: an eighth of the way along through the mid
        // keyboard, a seventh in the treble. The pair of hammer taps a
        // `strike` apart is a comb, |2 sin(pi f strike/rate)|, with a notch
        // wherever the strike divides the string evenly — the reason a piano
        // has no audible 8th partial and no minor-seventh clash from its own
        // spectrum. ROUNDED, not truncated: C4's loop is 183.5 samples, and
        // truncation drags the notch a third of the way toward n9, leaving
        // n8 dented instead of gone.
        let ratio = if k < 84.0 { 8.0 } else { 7.0 };
        self.strike = ((total / ratio).round() as usize).clamp(1, MAX_DELAY - 2);

        // **The hammer: a Gaussian force pulse, and the shape is most of the
        // instrument.** Two shapes failed on the bench before this one. The
        // raised cosine reads as the gentle choice and is a trap: its
        // spectrum has NULLS at multiples of 2/contact, and at mezzo-forte
        // contact times the first null lands square on partials 5-6 of the
        // middle keys — C4's 5th partial measured as injected at -97 dB, not
        // because the loop lost it but because the hammer never put it in.
        // A gamma pulse (t*e^(-t/tau)) fixed the spectrum — monotone, no
        // nulls — but what the tap pair injects is the pulse's DIFFERENCE,
        // and a gamma's rise is seven times steeper than its fall: waveform
        // asymmetry came back at 0.44, a thump on every quiet note. The
        // Gaussian is the shape with both virtues at once: symmetric in
        // time, so the pair's difference is balanced and asymmetry stays
        // under 0.13 at every velocity the bench plays, and a spectrum that
        // is itself a Gaussian — monotone, no nulls anywhere,
        // exp(-2 pi^2 sigma^2 f^2).
        //
        // Velocity narrows sigma: felt compresses and stiffens, contact
        // shortens, and because the rolloff in dB goes as (sigma*f)^2 a
        // modest width change swings the upper series by tens of decibels
        // while barely touching the fundamental. That is the
        // brightness-with-force the whole instrument turns on, with no
        // velocity layer anywhere in sight.
        // The endpoints are voicing, measured on the bench: 0.14 ms at full
        // velocity keeps the upper series a few dB shy of the fundamental —
        // fierce without turning to glass — and the soft end holds the
        // partials-over-f0 ratio near -38 dB, which is where an intimate
        // instrument should whisper. Between them the ratio rises about
        // 30 dB, every step monotone.
        let v = velocity.clamp(0.0, 1.0);
        // The VOICING knob rides the same physics: it scales the contact
        // width the way harder or softer hammer felt would, 1.35x at full
        // mellow to 0.74x at full bright. The exponent form makes 0.5 land
        // on 1.0 EXACTLY, so the shipped default is the bench tuning to the
        // last bit and the knob cannot drift it.
        let felt = 1.35_f32.powf(1.0 - 2.0 * voicing.clamp(0.0, 1.0));
        let sigma_ms = (0.31 - 0.17 * v.powf(0.85)) * felt;
        let sigma_s = (sigma_ms * 0.001 * self.rate).max(1.0);
        // Truncated at 3.5 sigma each side, where the curve is 53 dB down;
        // the tap pair cancels the step that remains.
        let half = 3.5 * sigma_s;
        self.pulse = (2.0 * half) as u32;
        // Exact incremental Gaussian: g(n) = exp(-(n-half)^2 / (2 sigma^2))
        // via g *= r, r *= rstep — two multiplies a sample, no per-sample
        // transcendentals on the audio thread.
        self.pulse_g = (-half * half / (2.0 * sigma_s * sigma_s)).exp();
        self.pulse_r = ((2.0 * half - 1.0) / (2.0 * sigma_s * sigma_s)).exp();
        self.pulse_rstep = (-1.0 / (sigma_s * sigma_s)).exp();
        // Felt is nonlinear — force goes as compression to a power above
        // one — so the amplitude curve is steeper than velocity. The scale
        // is set by the loudest thing the bench plays: at 0.5 the `comp`
        // figure's forte chords clipped the frame's unity bus by a tenth of
        // a decibel, and at 0.35 they peak 3 dB under it with the single-key
        // ceiling near -8 dBFS.
        self.pulse_amp = 0.35 * v.powf(1.35);
        self.damp = 1.0;
        self.damp_to = 1.0;
        self.env = self.pulse_amp;
        self.done = false;
    }

    pub fn release(&mut self, pedal: bool) {
        if !pedal {
            self.damp_to = 0.0;
        }
    }

    pub fn damp(&mut self) {
        self.damp_to = 0.0;
    }

    pub fn finished(&self) -> bool {
        self.done
    }

    pub fn level(&self) -> f32 {
        self.env
    }

    pub fn tick(&mut self) -> f32 {
        if self.done {
            return 0.0;
        }
        // Pour the hammer pulse in over its contact time: the incremental
        // Gaussian, two multiplies a sample.
        let mut inject = 0.0_f32;
        if self.pulse > 0 {
            inject = self.pulse_amp * self.pulse_g;
            self.pulse_g *= self.pulse_r;
            self.pulse_r *= self.pulse_rstep;
            self.pulse -= 1;
        }

        let mut sum = 0.0;
        for s in &mut self.strings {
            // Two injection points, opposite signs, one strike position
            // apart: their difference IS the strike comb, and — because the
            // pair sums to zero — it is also what keeps the excitation
            // DC-free. The positive half rides in through `tick` so it lands
            // in the head slot ADDITIVELY; pushing it through `excite(0, ..)`
            // put it in a slot the loop overwrote the same sample, which
            // silently deleted it and left the one-sided thump the bench
            // measured as asymmetry 1.000.
            if inject != 0.0 {
                s.excite(self.strike, -inject);
            }
            sum += s.tick(inject);
        }
        sum *= 1.0 / UNISON as f32;

        // The damper is a fast fade, not a gate: felt takes tens of
        // milliseconds to stop a string and a hard cut sounds like an edit.
        let coeff = 1.0 - (-1.0 / (0.055 * self.rate)).exp();
        self.damp += (self.damp_to - self.damp) * coeff;
        let out = sum * self.damp;

        let a = out.abs();
        self.env += (a - self.env) * if a > self.env { 0.4 } else { 0.0006 };
        if self.env < 2.0e-5 {
            self.done = true;
            for s in &mut self.strings {
                s.clear();
            }
        }
        out
    }
}

/// The soundboard, and the strings nobody struck.
///
/// **One per instrument, because there is one soundboard.** A string on its own
/// moves almost no air; what is heard is the board it is coupled to through the
/// bridge. Modelled here as a short feedback delay network — a handful of
/// delays cross-mixed so their echoes never line up — which is enough to give
/// the instrument a body and a stereo image without a reverb's cost.
///
/// With the pedal down it does a second job. Every damper is off the strings,
/// so the whole harp is free to answer whatever is being played; that is the
/// wash under a pedalled chord, and it is the reason a piano with the pedal
/// down sounds like a different instrument rather than a longer one.
pub struct Body {
    lines: [Vec<f32>; 4],
    idx: [usize; 4],
    fb: f32,
    damp: [Lp; 4],
    pedal: bool,
    energy: f32,
    patch: Patch,
}

impl Body {
    pub fn new(rate: f32) -> Self {
        // Mutually prime lengths, so no two echoes coincide and the board has
        // no ring of its own.
        let ms = [19.7_f32, 27.3, 34.1, 41.9];
        let lines = ms.map(|m| vec![0.0; ((m * 0.001 * rate) as usize).max(16)]);
        Body {
            lines,
            idx: [0; 4],
            fb: 0.34,
            damp: [Lp { z: 0.0, a: 0.42 }; 4],
            pedal: false,
            energy: 0.0,
            patch: Patch::Piano1,
        }
    }

    pub fn set_patch(&mut self, patch: Patch) {
        self.patch = patch;
        self.reset();
    }

    pub fn set_pedal(&mut self, down: bool) {
        self.pedal = down;
    }

    pub fn reset(&mut self) {
        for l in &mut self.lines {
            l.fill(0.0);
        }
        self.energy = 0.0;
    }

    pub fn ringing(&self) -> bool {
        self.energy > 1.0e-5
    }

    /// Drive the board and take its two outputs.
    pub fn tick(&mut self, drive: f32) -> (f32, f32) {
        // The electric piano has no soundboard. Its pickup is electromagnetic
        // and its body does nothing — which is exactly why a Rhodes taken dry
        // is so dry.
        if matches!(self.patch, Patch::EpMk1) {
            return (0.0, 0.0);
        }
        // Sympathetic strings only answer when the dampers are off.
        let send = if self.pedal { 0.22 } else { 0.075 };
        let x = drive * send;
        let mut outs = [0.0_f32; 4];
        for i in 0..4 {
            let n = self.lines[i].len();
            let v = self.lines[i][self.idx[i]];
            outs[i] = v;
            let _ = n;
        }
        // A Hadamard mix: every line feeds every other, which is what stops
        // the network sounding like four separate echoes.
        let m = [
            outs[0] + outs[1] + outs[2] + outs[3],
            outs[0] - outs[1] + outs[2] - outs[3],
            outs[0] + outs[1] - outs[2] - outs[3],
            outs[0] - outs[1] - outs[2] + outs[3],
        ];
        let mut e = 0.0;
        for i in 0..4 {
            let mut v = x + m[i] * 0.5 * self.fb;
            v = self.damp[i].tick(v);
            if !v.is_finite() {
                v = 0.0;
            }
            let n = self.lines[i].len();
            self.lines[i][self.idx[i]] = v;
            self.idx[i] = (self.idx[i] + 1) % n;
            e += v.abs();
        }
        self.energy += (e * 0.25 - self.energy) * 0.001;
        (
            (outs[0] + outs[2]) * 0.5,
            (outs[1] + outs[3]) * 0.5,
        )
    }
}
