//! The oscillator bank, the noise source and the LFO.
//!
//! **Anti-aliasing is by band-limited step, tabulated.** Every waveform
//! here is a polynomial in the phase, so the only places it has energy
//! above Nyquist are its discontinuities: a saw's reset, a pulse's two
//! edges, a triangle's two corners. Each of those is patched with the
//! residual of a band-limited step (BLEP) or a band-limited ramp (BLAMP,
//! the integral of the former) — the difference between the ideal jump and
//! the jump a low-pass kernel would have drawn through it.
//!
//! **Why the residual is a table and not the polynomial the brief named.**
//! The brief asked for polyBLEP, and polyBLEP was the first thing written
//! here — a four-point kernel, the integrated cubic B-spline. It measured
//! −47 dB on the brief's own gate, and the shortfall is arithmetic rather
//! than a bug: a cubic B-spline's transform is sinc⁴, which at 27.2 kHz —
//! where the 13th harmonic of a saw at C7 folds back to 20.8 kHz — is only
//! 21 dB down, and no polynomial kernel a few samples wide is sharper than
//! that near the fold. The gate (60 dB) is what the method was chosen to
//! serve, so the method moved: the residual is now that of a 64-sample
//! Kaiser-windowed sinc, designed by the SAME designer the suite's 4×
//! oversampler uses (`units::drive::windowed_sinc`), integrated once for
//! the step and twice for the corner, held 64× oversampled and read with
//! linear interpolation. Its cutoff sits at [`BLEP_CUTOFF`] of the rate so
//! its −80 dB stopband begins at Nyquist; everything that folds arrives
//! through the stopband. Cost: one lookup per kernel sample per
//! discontinuity, thousands of discontinuities a second at the top of the
//! keyboard, which is nothing beside the filter.
//!
//! The kernel is one table ([`BlepTable`]) and every event goes through one
//! path ([`Osc::event`]): a hard-sync reset is a step of height
//! `wave(0) − wave(p)` plus a slope change of `slope(0) − slope(p)`, exactly
//! as a wrap is, so it gets the same correction rather than a special case
//! that aliases worse than the unsynced wave. That is the failure mode the
//! brief names.
//!
//! **Free-running phase.** Nothing here resets on a note. The oscillator
//! keeps counting while its voice is silent, which is what a hardware VCO
//! does and why two voices on one note do not sound like one voice twice as
//! loud. `Osc::new` seeds the phase from the voice and oscillator index so a
//! render is repeatable bit for bit.
//!
//! The correction reaches [`BLEP_HALF`] samples INTO THE FUTURE, so the
//! oscillator delays its output by that much ([`Osc::LATENCY`]). All three
//! oscillators and the noise share the delay, so nothing needs aligning,
//! and the instrument reports it.

use crate::units::drive::windowed_sinc;
use std::sync::OnceLock;

/// The six range positions, in switch order. Octaves relative to 8′.
///
/// **LO is three octaves under 32′**, the sub-audio position: with the
/// keyboard switch off, middle C at LO is 8.2 Hz and the ±7 semitone tune
/// knob walks it from 5.5 to 12.2 Hz — the vibrato range — while the same
/// switch at 32′ or 8′ gives a fixed-frequency oscillator for audio-rate
/// modulation, which is the other half of the 1970 machine's trick.
pub const RANGE_OCTAVES: [f32; 6] = [-5.0, -2.0, -1.0, 0.0, 1.0, 2.0];
/// The same six positions as the panel engraves them.
pub const RANGE_LABELS: [&str; 6] = ["LO", "32'", "16'", "8'", "4'", "2'"];

/// The six waveform positions, in switch order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Wave {
    Triangle,
    /// The 1970 machine's second position: the triangle and the saw summed,
    /// a shark's tooth. Its own shape rather than a mix of two oscillators
    /// because it was one position on one switch.
    TriSaw,
    #[default]
    Saw,
    /// Pulse at half width.
    Square,
    /// Pulse at [`PULSE_WIDE`].
    Wide,
    /// Pulse at [`PULSE_NARROW`].
    Narrow,
}

impl Wave {
    pub const ALL: [Wave; 6] = [
        Wave::Triangle,
        Wave::TriSaw,
        Wave::Saw,
        Wave::Square,
        Wave::Wide,
        Wave::Narrow,
    ];

    /// The switch position, 0..6, from a normalised dial.
    pub fn from_norm(v: f32) -> Wave {
        let last = (Wave::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(Wave::ALL.len() - 1);
        Wave::ALL[at]
    }

    pub fn label(self) -> &'static str {
        match self {
            Wave::Triangle => "tri",
            Wave::TriSaw => "tri-saw",
            Wave::Saw => "saw",
            Wave::Square => "square",
            Wave::Wide => "wide",
            Wave::Narrow => "narrow",
        }
    }

    /// The resting pulse width for the three pulse positions; `None` for
    /// the three that have no edge to move.
    fn pulse_width(self) -> Option<f32> {
        match self {
            Wave::Square => Some(0.5),
            Wave::Wide => Some(PULSE_WIDE),
            Wave::Narrow => Some(PULSE_NARROW),
            _ => None,
        }
    }
}

/// The wide and narrow pulse positions, as a fraction of the period high.
/// The original's are about a third and a tenth.
pub const PULSE_WIDE: f32 = 0.30;
pub const PULSE_NARROW: f32 = 0.10;

/// How far PWM at full may push a pulse edge from its resting width, as a
/// fraction of the period. The edges are then held off the ends by
/// [`PULSE_MIN`], because a width of exactly zero is silence with a click
/// in it.
pub const PWM_SWING: f32 = 0.45;
pub const PULSE_MIN: f32 = 0.02;

/// The triangle-saw's scale: the raw sum of a ±1 triangle and a ±1 saw
/// spans −2..+1, and this brings its swing near the other waves'.
const TRI_SAW_SCALE: f32 = 0.6;

/// The highest phase increment an oscillator will run at: 0.45 cycles per
/// sample, 21.6 kHz at 48 kHz. Above half the rate a discontinuity
/// happens more than once per sample and the correction stops being
/// well-defined; the 2′ range at the top of a bent keyboard can ask for
/// it, and gets this instead.
pub const MAX_INC: f32 = 0.45;

/// Half the kernel's width in samples: the correction reaches this far
/// either side of a discontinuity, and the oscillator's latency is this.
pub const BLEP_HALF: usize = 32;
const BLEP_WIDTH: usize = 2 * BLEP_HALF;
/// How finely the residual is tabulated between samples.
const BLEP_OS: usize = 64;
const BLEP_LEN: usize = BLEP_WIDTH * BLEP_OS;
/// The kernel's cutoff as a fraction of the sample rate. With 64 taps at
/// β = 8 the Kaiser transition is about 0.08 of the rate wide, so this
/// puts the stopband's start at Nyquist: 22 kHz in a 48 kHz session,
/// 20.3 kHz at 44.1. The legitimate harmonics in the last two kilohertz
/// under Nyquist are shaded by the transition; nothing that folds is.
pub const BLEP_CUTOFF: f64 = 0.46;
/// Kaiser β for the kernel — the same figure the oversampler uses, and
/// about 80 dB of stopband.
const BLEP_BETA: f64 = 8.0;

/// The tabulated kernel integrals, `(t + BLEP_HALF) · BLEP_OS` for `t`
/// samples after the event, with one guard entry past the end for the
/// interpolator.
///
/// **The step table holds the band-limited step itself, not the
/// residual.** The residual `S(t) − u(t)` jumps by one at `t = 0`, and the
/// first version tabulated it: whenever a discontinuity landed in the last
/// 1/64 of a sample, the interpolator straddled that jump and handed the
/// slot at the discontinuity a value from halfway across it — a click on
/// one event in sixty-four, which read as a −53 dB alias line. `S` is
/// smooth, so it interpolates; the unit step is subtracted at read time
/// from the exact side the slot is on.
pub struct BlepTable {
    /// `S(t)`, the kernel's running integral, 0 → 1.
    step: Vec<f32>,
    /// `∫ (S − u)`, the band-limited ramp's residual, in sample units.
    ramp: Vec<f32>,
}

/// The kernel is designed one entry longer than the table so its centre
/// falls on an integer index — `t = 0` is entry `BLEP_HALF · BLEP_OS`
/// exactly, and `S` there is exactly one half.
const BLEP_TAPS: usize = BLEP_LEN + 1;

impl BlepTable {
    fn build() -> Self {
        // The kernel at the oversampled rate, cutoff scaled to match.
        let h: Box<[f32; BLEP_TAPS]> =
            Box::new(windowed_sinc(BLEP_BETA, BLEP_CUTOFF / BLEP_OS as f64));
        let total: f64 = h.iter().map(|v| f64::from(*v)).sum();
        // The band-limited step: the kernel's running integral, normalised
        // to end at exactly one, sampled at the trailing edge of each
        // entry's own tap so the centre entry reads one half.
        let mut step = Vec::with_capacity(BLEP_TAPS + 1);
        let mut acc = 0.0_f64;
        for (i, v) in h.iter().enumerate() {
            let half = f64::from(*v) / total * 0.5;
            acc += half;
            step.push(acc as f32);
            acc += half;
            let _ = i;
        }
        step.push(1.0);
        // The band-limited ramp's residual: ∫ (S − u), in sample units, by
        // the trapezoid rule with the centre sample — where the residual
        // jumps from +½ to −½ — counted as zero. That keeps the integral
        // of an antisymmetric residual exactly symmetric; a rectangle rule
        // put a half-entry timing skew in it.
        let centre = BLEP_LEN / 2;
        let resid = |i: usize| -> f64 {
            let u = match i.cmp(&centre) {
                std::cmp::Ordering::Less => 0.0,
                std::cmp::Ordering::Equal => 0.5,
                std::cmp::Ordering::Greater => 1.0,
            };
            f64::from(step[i]) - u
        };
        let mut ramp = Vec::with_capacity(BLEP_TAPS + 1);
        let mut acc = 0.0_f64;
        ramp.push(0.0);
        for i in 1..BLEP_TAPS {
            acc += 0.5 * (resid(i - 1) + resid(i)) / BLEP_OS as f64;
            ramp.push(acc as f32);
        }
        ramp.push(0.0);
        Self { step, ramp }
    }

    /// The residual at `t` samples after a step, interpolated.
    pub fn blep(&self, t: f32) -> f32 {
        let u = if t >= 0.0 { 1.0 } else { 0.0 };
        if t <= -(BLEP_HALF as f32) || t >= BLEP_HALF as f32 {
            return 0.0;
        }
        Self::read(&self.step, t) - u
    }

    /// The residual at `t` samples after a corner, interpolated.
    pub fn blamp(&self, t: f32) -> f32 {
        Self::read(&self.ramp, t)
    }

    fn read(tab: &[f32], t: f32) -> f32 {
        let pos = (t + BLEP_HALF as f32) * BLEP_OS as f32;
        if pos <= 0.0 || pos >= BLEP_LEN as f32 {
            return 0.0;
        }
        let i = pos as usize;
        let f = pos - i as f32;
        tab[i] + f * (tab[i + 1] - tab[i])
    }
}

static BLEP_TABLE: OnceLock<BlepTable> = OnceLock::new();

/// The one table, built on first call. **Call it from `new`, off the
/// audio thread**: the first call allocates, every later one is an atomic
/// load.
pub fn blep_table() -> &'static BlepTable {
    BLEP_TABLE.get_or_init(BlepTable::build)
}

/// The naive waveform and its slope (per unit phase) at `p` in `0..1`.
/// Evaluated exactly at a discontinuity, this is the value on the side
/// the phase is APPROACHING FROM: callers asking for the far side pass
/// the far phase (see [`Osc::event`]).
#[inline]
fn wave_at(wave: Wave, p: f32, pw: f32) -> (f32, f32) {
    match wave {
        Wave::Saw => (2.0 * p - 1.0, 2.0),
        Wave::Triangle => tri_at(p),
        Wave::TriSaw => {
            let (t, ts) = tri_at(p);
            (
                (t + 2.0 * p - 1.0) * TRI_SAW_SCALE,
                (ts + 2.0) * TRI_SAW_SCALE,
            )
        }
        Wave::Square | Wave::Wide | Wave::Narrow => {
            if p < pw {
                (1.0, 0.0)
            } else {
                (-1.0, 0.0)
            }
        }
    }
}

#[inline]
fn tri_at(p: f32) -> (f32, f32) {
    if p < 0.5 {
        (4.0 * p - 1.0, 4.0)
    } else {
        (3.0 - 4.0 * p, -4.0)
    }
}

/// One oscillator.
#[derive(Clone, Copy, Debug)]
pub struct Osc {
    /// Phase in cycles, `0..1`.
    phase: f32,
    /// The current pulse width, set per sample by the caller.
    pw: f32,
    /// A ring of output samples under correction: the naive value is
    /// written at `n`, corrections land at `n−BLEP_HALF ..< n+BLEP_HALF`,
    /// and the slot `BLEP_HALF` behind is read out.
    ring: [f32; BLEP_WIDTH],
    /// Index of the slot for sample `n`.
    head: usize,
    /// True when the phase wrapped on the last advance, at the fractional
    /// position `wrap_at` samples before the current sample — the master's
    /// signal to a synced slave.
    wrapped: bool,
    wrap_at: f32,
}

impl Osc {
    /// Samples between an event and its appearance at the output.
    pub const LATENCY: usize = BLEP_HALF;

    /// An oscillator with its phase seeded from `seed` so two renders of
    /// the same score agree bit for bit — and two oscillators in one
    /// voice, or one oscillator in two voices, start apart.
    pub fn new(seed: u32) -> Self {
        let mut s = seed.wrapping_mul(2_654_435_761).wrapping_add(0x9E37_79B9);
        s ^= s >> 16;
        s = s.wrapping_mul(0x85EB_CA6B);
        s ^= s >> 13;
        Self {
            phase: (s & 0x00FF_FFFF) as f32 / 16_777_216.0,
            pw: 0.5,
            ring: [0.0; BLEP_WIDTH],
            head: 0,
            wrapped: false,
            wrap_at: 0.0,
        }
    }

    /// Whether the phase wrapped on the last [`Osc::tick`], and how many
    /// samples before that sample it did: what a slave reads.
    #[inline]
    pub fn wrap(&self) -> Option<f32> {
        if self.wrapped {
            Some(self.wrap_at)
        } else {
            None
        }
    }

    /// Add the corrections for one discontinuity at `at` samples before the
    /// current sample: a step of `step` and a slope change of `dslope`
    /// (both in output units, the slope per sample).
    #[inline]
    fn event(&mut self, at: f32, step: f32, dslope: f32) {
        let tab = blep_table();
        // Slot n−HALF+j sits at t = at − HALF + j after the discontinuity,
        // so its table position is (at + j)·OS: one fractional part for
        // the whole event, and a stride of OS between slots.
        let base = at.clamp(0.0, 0.999_99) * BLEP_OS as f32;
        let i0 = base as usize;
        let frac = base - i0 as f32;
        let mut idx = i0;
        let mut slot = (self.head + BLEP_HALF) & (BLEP_WIDTH - 1);
        for j in 0..BLEP_WIDTH {
            // Slots from the discontinuity onward are on the far side of
            // the unit step the naive waveform already took.
            let u = if j >= BLEP_HALF { 1.0 } else { 0.0 };
            let mut c = 0.0;
            if step != 0.0 {
                let t = &tab.step;
                c += step * (t[idx] + frac * (t[idx + 1] - t[idx]) - u);
            }
            if dslope != 0.0 {
                let t = &tab.ramp;
                c += dslope * (t[idx] + frac * (t[idx + 1] - t[idx]));
            }
            self.ring[slot] += c;
            idx += BLEP_OS;
            slot = (slot + 1) & (BLEP_WIDTH - 1);
        }
    }

    /// Events inside one phase segment `[from, to)` of an UNWRAPPED phase
    /// run (so `to` may exceed 1): the wrap at 1, the pulse edge at `pw`,
    /// the triangle corner at ½ — each located in samples before the
    /// current sample given the increment.
    #[inline]
    fn segment(&mut self, wave: Wave, from: f32, to: f32, end: f32, inc: f32) {
        // `end` is the phase value at the current sample in the same
        // unwrapped frame, so an event at phase x sits (end − x)/inc
        // samples back.
        let pw = self.pw;
        match wave {
            Wave::Saw => {
                if from < 1.0 && to >= 1.0 {
                    self.event((end - 1.0) / inc, -2.0, 0.0);
                }
            }
            Wave::Triangle => {
                if from < 0.5 && to >= 0.5 {
                    self.event((end - 0.5) / inc, 0.0, -8.0 * inc);
                }
                if from < 1.0 && to >= 1.0 {
                    self.event((end - 1.0) / inc, 0.0, 8.0 * inc);
                }
            }
            Wave::TriSaw => {
                if from < 0.5 && to >= 0.5 {
                    self.event((end - 0.5) / inc, 0.0, -8.0 * inc * TRI_SAW_SCALE);
                }
                if from < 1.0 && to >= 1.0 {
                    self.event(
                        (end - 1.0) / inc,
                        -2.0 * TRI_SAW_SCALE,
                        8.0 * inc * TRI_SAW_SCALE,
                    );
                }
            }
            Wave::Square | Wave::Wide | Wave::Narrow => {
                if from < pw && to >= pw {
                    self.event((end - pw) / inc, -2.0, 0.0);
                }
                if from < 1.0 && to >= 1.0 {
                    self.event((end - 1.0) / inc, 2.0, 0.0);
                }
            }
        }
    }

    /// Advance one sample at `inc` cycles per sample and return the output
    /// sample (two samples late, see [`Osc::LATENCY`]).
    ///
    /// `sync` is the master's wrap, if it wrapped this sample: the slave
    /// resets its phase at that instant, with the same step-and-corner
    /// correction any other discontinuity gets.
    #[inline]
    pub fn tick(&mut self, wave: Wave, inc: f32, pw_mod: f32, sync: Option<f32>) -> f32 {
        let inc = inc.clamp(1.0e-9, MAX_INC);
        // The pulse width for this sample, held off the ends.
        self.pw = match wave.pulse_width() {
            Some(w) => (w + pw_mod).clamp(PULSE_MIN, 1.0 - PULSE_MIN),
            None => 0.5,
        };
        let p0 = self.phase;
        let p1 = p0 + inc;
        self.wrapped = false;

        match sync {
            None => {
                self.segment(wave, p0, p1, p1, inc);
                if p1 >= 1.0 {
                    self.wrapped = true;
                    self.wrap_at = (p1 - 1.0) / inc;
                }
                self.phase = p1 - p1.floor();
            }
            Some(at) => {
                // The slave's own phase at the master's wrap, in the
                // unwrapped frame: events before it are ordinary.
                let pw_phase = p1 - at * inc;
                self.segment(wave, p0, pw_phase, p1, inc);
                let before = pw_phase - pw_phase.floor();
                let (v0, s0) = wave_at(wave, before, self.pw);
                let (v1, s1) = wave_at(wave, 0.0, self.pw);
                self.event(at, v1 - v0, (s1 - s0) * inc);
                // Then from zero to where the reset phase lands now.
                let p_new = at * inc;
                self.segment(wave, 0.0, p_new, p_new, inc);
                self.phase = p_new - p_new.floor();
                // A slave reports the master's wrap as its own, so a third
                // oscillator could chain — nothing does, but the report is
                // true either way.
                self.wrapped = true;
                self.wrap_at = at;
            }
        }

        // The naive value at the new phase, into slot n.
        let (v, _) = wave_at(wave, self.phase, self.pw);
        self.ring[self.head] += v;
        // Read slot n−HALF, clear it (it becomes n+HALF next), advance.
        let out_idx = (self.head + BLEP_HALF) & (BLEP_WIDTH - 1);
        let out = self.ring[out_idx];
        self.ring[out_idx] = 0.0;
        self.head = (self.head + 1) & (BLEP_WIDTH - 1);
        out
    }

    /// Drop the pending corrections and the wrap flag but keep the phase:
    /// a voice being reused after a steal keeps its oscillators running.
    pub fn flush(&mut self) {
        self.ring = [0.0; BLEP_WIDTH];
        self.wrapped = false;
    }
}

/// A repeatable pseudo-random source: xorshift32, seeded per user so no
/// two noise sources in the instrument are the same stream and no run of
/// the instrument differs from the last.
#[derive(Clone, Copy, Debug)]
pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        // Zero is xorshift's fixed point; nudge it off.
        Self(seed.wrapping_mul(0x9E37_79B9) | 1)
    }
    #[inline]
    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }
    /// Uniform in −1..1.
    #[inline]
    pub fn bipolar(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 8_388_608.0 - 1.0
    }
    /// Uniform in 0..1.
    #[inline]
    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
}

/// The noise source: white, or pink through the three-pole economy
/// approximation (Kellet), which is within a quarter decibel of −3 dB per
/// octave over the audio band and costs three multiplies.
#[derive(Clone, Copy, Debug)]
pub struct Noise {
    rng: Rng,
    b: [f32; 3],
}

impl Noise {
    pub fn new(seed: u32) -> Self {
        Self {
            rng: Rng::new(seed),
            b: [0.0; 3],
        }
    }
    #[inline]
    pub fn tick(&mut self, pink: bool) -> f32 {
        let w = self.rng.bipolar();
        if !pink {
            return w;
        }
        self.b[0] = 0.997_65 * self.b[0] + w * 0.099_046_0;
        self.b[1] = 0.963_00 * self.b[1] + w * 0.296_396_0;
        self.b[2] = 0.570_00 * self.b[2] + w * 1.052_648_0;
        // The sum runs about 4× hotter than white; brought back near it.
        (self.b[0] + self.b[1] + self.b[2] + w * 0.184_800_0) * 0.25
    }
    pub fn clear(&mut self) {
        self.b = [0.0; 3];
    }
}

/// The five LFO shapes, in switch order.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum LfoWave {
    #[default]
    Triangle,
    Sine,
    Square,
    Saw,
    /// Sample-and-hold: a new random level every cycle.
    Hold,
}

impl LfoWave {
    pub const ALL: [LfoWave; 5] = [
        LfoWave::Triangle,
        LfoWave::Sine,
        LfoWave::Square,
        LfoWave::Saw,
        LfoWave::Hold,
    ];
    pub fn from_norm(v: f32) -> LfoWave {
        let last = (LfoWave::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(LfoWave::ALL.len() - 1);
        LfoWave::ALL[at]
    }
    pub fn label(self) -> &'static str {
        match self {
            LfoWave::Triangle => "tri",
            LfoWave::Sine => "sine",
            LfoWave::Square => "square",
            LfoWave::Saw => "saw",
            LfoWave::Hold => "S&H",
        }
    }
}

/// The one dedicated low-frequency oscillator: bipolar ±1, not
/// band-limited (it never reaches the audio path directly).
#[derive(Clone, Copy, Debug)]
pub struct Lfo {
    phase: f32,
    rng: Rng,
    held: f32,
}

impl Lfo {
    pub fn new(seed: u32) -> Self {
        let mut rng = Rng::new(seed);
        let held = rng.bipolar();
        Self {
            phase: 0.0,
            rng,
            held,
        }
    }
    #[inline]
    pub fn tick(&mut self, wave: LfoWave, inc: f32) -> f32 {
        self.phase += inc;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
            self.held = self.rng.bipolar();
        }
        let p = self.phase;
        match wave {
            LfoWave::Triangle => {
                if p < 0.5 {
                    4.0 * p - 1.0
                } else {
                    3.0 - 4.0 * p
                }
            }
            LfoWave::Sine => (p * std::f32::consts::TAU).sin(),
            LfoWave::Square => {
                if p < 0.5 {
                    1.0
                } else {
                    -1.0
                }
            }
            LfoWave::Saw => 2.0 * p - 1.0,
            LfoWave::Hold => self.held,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::measure::{db, goertzel_hann, measure_hz};

    const SR: f32 = 48_000.0;

    fn render(wave: Wave, hz: f32, secs: f32, pw_mod: f32) -> Vec<f32> {
        let mut o = Osc::new(7);
        let inc = hz / SR;
        (0..(secs * SR) as usize)
            .map(|_| o.tick(wave, inc, pw_mod, None))
            .collect()
    }

    /// The worst component off the harmonic grid, in dB against the
    /// fundamental, scanning a 10 Hz grid over the audio band and skipping
    /// anything within `skirt` Hz of a true harmonic.
    fn worst_alias_db(x: &[f32], f0: f32, skirt: f32) -> (f32, f32) {
        let fund = goertzel_hann(x, f0, SR);
        let mut worst = (0.0_f32, 0.0_f32);
        let mut hz = 20.0_f32;
        while hz < SR * 0.5 - 100.0 {
            let near = (hz / f0).round() * f0;
            if (hz - near).abs() > skirt {
                let a = goertzel_hann(x, hz, SR);
                if a > worst.0 {
                    worst = (a, hz);
                }
            }
            hz += 10.0;
        }
        (db(worst.0 / fund), worst.1)
    }

    /// **The brief's alias floor**: a saw at C7 keeps every component off
    /// its harmonic grid 60 dB under its fundamental. The polyBLEP's whole
    /// justification, measured rather than cited.
    #[test]
    fn a_saw_at_c7_keeps_aliases_sixty_db_down() {
        let f0 = 440.0 * 2.0_f32.powf((96.0 - 69.0) / 12.0);
        let x = render(Wave::Saw, f0, 1.0, 0.0);
        let (worst, at) = worst_alias_db(&x[4096..], f0, 40.0);
        assert!(
            worst <= -60.0,
            "worst alias {worst:.1} dB at {at:.0} Hz (saw at C7 = {f0:.1} Hz)"
        );
    }

    /// The same floor for the shapes with corners and the narrowest pulse:
    /// the BLAMP and the two-edge path earn their keep too, to 60 dB at C6
    /// — a narrow pulse's spectrum is nearly flat, so its aliases start
    /// from a higher shelf than a saw's.
    #[test]
    fn every_waveform_at_c6_keeps_aliases_sixty_db_down() {
        let f0 = 440.0 * 2.0_f32.powf((84.0 - 69.0) / 12.0);
        for wave in Wave::ALL {
            let x = render(wave, f0, 1.0, 0.0);
            let (worst, at) = worst_alias_db(&x[4096..], f0, 40.0);
            assert!(
                worst <= -60.0,
                "{}: worst alias {worst:.1} dB at {at:.0} Hz",
                wave.label()
            );
        }
    }

    /// **Hard sync gets the same correction as a wrap.** A saw synced to a
    /// master a fifth below it is a wave whose every reset is a partial
    /// jump; the aliases of that reset must sit as low as the plain saw's
    /// — 55 dB here, because the sync tone's harmonic grid is the master's
    /// and the grid is denser, so the skirt is narrower.
    #[test]
    fn a_synced_saw_aliases_no_worse_than_a_plain_one() {
        let master = 440.0 * 2.0_f32.powf((72.0 - 69.0) / 12.0);
        let slave = master * 1.5;
        let mut m = Osc::new(1);
        let mut s = Osc::new(2);
        let x: Vec<f32> = (0..SR as usize)
            .map(|_| {
                m.tick(Wave::Saw, master / SR, 0.0, None);
                s.tick(Wave::Saw, slave / SR, 0.0, m.wrap())
            })
            .collect();
        let (worst, at) = worst_alias_db(&x[4096..], master, 25.0);
        assert!(
            worst <= -55.0,
            "synced saw: worst alias {worst:.1} dB at {at:.0} Hz"
        );
    }

    /// **Tuning, every range, every note**: the fundamental of a saw sits
    /// on equal temperament to a cent across 32′..2′ and the whole
    /// keyboard the oscillator can reach below the increment ceiling.
    /// The LO range is the same arithmetic five octaves down and too slow
    /// to read from a one-second window; its constant is pinned instead.
    #[test]
    fn every_range_and_note_is_in_tune_to_a_cent() {
        let mut worst = 0.0_f32;
        for &oct in &RANGE_OCTAVES[1..] {
            for key in (24..=108).step_by(1) {
                let hz = 440.0 * 2.0_f32.powf((key as f32 - 69.0) / 12.0 + oct);
                if hz / SR > MAX_INC || hz < 30.0 {
                    continue;
                }
                let x = render(Wave::Saw, hz, 0.8, 0.0);
                let got = measure_hz(&x[64..], hz, SR);
                let cents = 1200.0 * (got / hz).log2();
                worst = worst.max(cents.abs());
                assert!(
                    cents.abs() <= 1.0,
                    "range {oct:+} key {key}: {hz:.2} Hz read {got:.3} ({cents:+.2} cents)"
                );
            }
        }
        assert_eq!(RANGE_OCTAVES[0], -5.0, "LO moved from three octaves under 32′");
        assert!(worst > 0.0, "the ruler read every note as perfect, which means it read nothing");
    }

    /// The tabulated residuals are what the derivation says: the step's
    /// is antisymmetric with a unit jump at zero and nothing at the ends;
    /// the corner's is symmetric, peaks at zero, and vanishes at the ends.
    #[test]
    fn the_blep_table_is_a_unit_step_and_a_unit_corner() {
        let tab = blep_table();
        assert!((tab.blep(-0.001) - 0.5).abs() < 0.02, "{}", tab.blep(-0.001));
        assert!((tab.blep(0.001) + 0.5).abs() < 0.02, "{}", tab.blep(0.001));
        assert_eq!(tab.blep(-32.0), 0.0);
        assert_eq!(tab.blep(32.0), 0.0);
        for t in [0.3_f32, 1.7, 5.25, 20.0] {
            assert!((tab.blep(t) + tab.blep(-t)).abs() < 1e-4, "blep not antisymmetric at {t}");
            assert!(
                (tab.blamp(t) - tab.blamp(-t)).abs() < 1e-4,
                "blamp not symmetric at {t}: {} vs {}",
                tab.blamp(t),
                tab.blamp(-t)
            );
            assert!(tab.blamp(t) < tab.blamp(0.0), "blamp does not peak at zero");
        }
        assert!(tab.blamp(31.9).abs() < 1e-3, "the corner residual does not vanish: {}", tab.blamp(31.9));
    }

    /// PWM moves the edge and the duty follows: a square driven to its
    /// widest and narrowest reads the mean the width predicts.
    #[test]
    fn pulse_width_modulation_moves_the_duty_cycle() {
        for pw_mod in [-PWM_SWING, 0.0, PWM_SWING] {
            let x = render(Wave::Square, 100.0, 1.0, pw_mod);
            let mean: f32 = x[4800..].iter().sum::<f32>() / (x.len() - 4800) as f32;
            let want = 2.0 * (0.5 + pw_mod).clamp(PULSE_MIN, 1.0 - PULSE_MIN) - 1.0;
            assert!((mean - want).abs() < 0.02, "pw_mod {pw_mod}: mean {mean:.3}, want {want:.3}");
        }
    }

    /// Two oscillators seeded differently start at different phases; the
    /// same seed starts at the same one. The repeatability the brief asks
    /// for begins here.
    #[test]
    fn seeds_set_the_phase_repeatably() {
        let a = Osc::new(3);
        let b = Osc::new(3);
        let c = Osc::new(4);
        assert_eq!(a.phase, b.phase);
        assert_ne!(a.phase, c.phase);
    }

    /// Pink noise carries equal energy per octave and white does not: the
    /// octave 200–400 Hz against the octave 1.6–3.2 kHz reads level for
    /// pink and nine decibels down for white. The Kellet filter is doing
    /// its job, and the switch is a real switch.
    #[test]
    fn pink_noise_holds_equal_energy_per_octave_and_white_does_not() {
        let octave_ratio_db = |pink: bool| {
            let mut n = Noise::new(11);
            let x: Vec<f32> = (0..SR as usize * 4).map(|_| n.tick(pink)).collect();
            let band = |lo: f32, hi: f32| {
                let mut e = 0.0_f32;
                let mut f = lo;
                while f < hi {
                    let m = goertzel_hann(&x, f, SR);
                    e += m * m;
                    f += 10.0;
                }
                e
            };
            db((band(200.0, 400.0) / band(1600.0, 3200.0)).sqrt())
        };
        let pink = octave_ratio_db(true);
        let white = octave_ratio_db(false);
        assert!(pink.abs() < 2.0, "pink: low octave vs high octave {pink:.1} dB (want ~0)");
        assert!((white + 9.0).abs() < 2.0, "white: low octave vs high octave {white:.1} dB (want ~-9)");
    }
}
