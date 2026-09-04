//! The contour generators: two ADSRs, shaped the way a capacitor charges.
//!
//! **Analogue-shaped, not linear.** An attack here is an RC charging toward
//! a target ABOVE the rail ([`ATTACK_TARGET`]) and switching to decay the
//! instant it crosses one: convex, fast off the mark, slowing into the
//! peak — the punch of every transistor contour generator. Decay and
//! release are true exponentials toward the sustain and toward zero. A
//! linear ramp is the one shape no capacitor draws, and the ear knows.
//!
//! **What the knobs mean, in seconds.** Attack is the time from zero to
//! the peak. Decay and release are T60 — the time to fall sixty decibels,
//! the number a room's decay is quoted in — so a fifteen-second release
//! means what a fifteen-second reverb means. With the sustain up a decay's
//! exponential only travels part of the way, at the same rate.
//!
//! **A restrike attacks from where the envelope IS.** `gate_on` on a
//! sounding envelope resets nothing: the attack charges from the current
//! level toward the target, so a repeated note under a long release gets a
//! hammer, not a hole. Zeroing a live envelope is the pop the brief and the
//! keys work both name, and this module cannot be asked to do it.

/// Where the attack charges toward, as a multiple of the peak. Reaching
/// 1.0 on the way to 1.3 takes `ln(1.3/0.3) ≈ 1.47` time constants, and
/// the curve at the peak still has a third of its initial slope — the
/// convex shape a discrete contour generator draws.
pub const ATTACK_TARGET: f32 = 1.3;

/// The dial ranges, in seconds, logarithmic across the dial.
pub const ATTACK_SECS: (f32, f32) = (0.001, 10.0);
pub const DECAY_SECS: (f32, f32) = (0.002, 15.0);
pub const RELEASE_SECS: (f32, f32) = (0.002, 15.0);

/// `ln(1000)`: sixty decibels as a ratio of natural logs.
const T60_LN: f32 = 6.907_755_3;

/// Below this the envelope is over and the voice may go quiet.
pub const SILENT: f32 = 1.0e-4;

/// A dial position to seconds, geometric across `range`.
#[inline]
pub fn secs(dial: f32, range: (f32, f32)) -> f32 {
    range.0 * (range.1 / range.0).powf(dial.clamp(0.0, 1.0))
}

/// The attack's time constant for a dial, in seconds: the time to the peak
/// divided by the number of time constants the climb to it takes.
#[inline]
pub fn attack_tau(dial: f32) -> f32 {
    secs(dial, ATTACK_SECS) / (ATTACK_TARGET / (ATTACK_TARGET - 1.0)).ln()
}

/// The 10–90% rise the attack dial promises, in seconds — what the bench
/// reads against. Derived from the same curve: the time from 0.1 to 0.9 on
/// the way to [`ATTACK_TARGET`].
pub fn attack_rise_10_90(dial: f32) -> f32 {
    let tau = attack_tau(dial);
    let t = ATTACK_TARGET;
    tau * ((t - 0.1) / (t - 0.9)).ln()
}

/// The decay dial's T60, seconds.
#[inline]
pub fn decay_t60(dial: f32) -> f32 {
    secs(dial, DECAY_SECS)
}

/// The release dial's T60, seconds.
#[inline]
pub fn release_t60(dial: f32) -> f32 {
    secs(dial, RELEASE_SECS)
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Idle,
    Attack,
    Decay,
    Release,
}

/// Per-sample coefficients for one ADSR, recomputed when a dial moves —
/// the exp() lives here, not in the tick.
#[derive(Clone, Copy, Debug)]
pub struct EnvCoeffs {
    /// `1 − e^{−1/(τ·rate)}` for each stage.
    attack: f32,
    decay: f32,
    release: f32,
    sustain: f32,
}

impl EnvCoeffs {
    /// From the four dials. `time_scale` is the voice's component
    /// tolerance on its timing (1.0 exact): a stage takes that much longer.
    pub fn new(rate: f32, attack: f32, decay: f32, sustain: f32, release: f32, time_scale: f32) -> Self {
        let scale = time_scale.max(0.01);
        let coeff = |tau_secs: f32| -> f32 {
            let tau = (tau_secs * scale * rate).max(1.0);
            1.0 - (-1.0 / tau).exp()
        };
        Self {
            attack: coeff(attack_tau(attack)),
            decay: coeff(decay_t60(decay) / T60_LN),
            release: coeff(release_t60(release) / T60_LN),
            sustain: sustain.clamp(0.0, 1.0),
        }
    }
}

/// One contour generator.
#[derive(Clone, Copy, Debug)]
pub struct Env {
    level: f32,
    stage: Stage,
}

impl Env {
    pub const fn new() -> Self {
        Self {
            level: 0.0,
            stage: Stage::Idle,
        }
    }

    #[inline]
    pub fn level(&self) -> f32 {
        self.level
    }

    #[inline]
    pub fn stage(&self) -> Stage {
        self.stage
    }

    /// Sounding at all: anything but idle.
    #[inline]
    pub fn active(&self) -> bool {
        self.stage != Stage::Idle
    }

    /// Start (or restart) the attack — from the current level, whatever
    /// it is. See the module header.
    #[inline]
    pub fn gate_on(&mut self) {
        self.stage = Stage::Attack;
    }

    /// Into the release, from wherever the envelope is.
    #[inline]
    pub fn gate_off(&mut self) {
        if self.stage != Stage::Idle {
            self.stage = Stage::Release;
        }
    }

    /// Straight to silence: only for a voice being reset, never mid-note.
    pub fn kill(&mut self) {
        self.level = 0.0;
        self.stage = Stage::Idle;
    }

    #[inline]
    pub fn tick(&mut self, c: &EnvCoeffs) -> f32 {
        match self.stage {
            Stage::Idle => {}
            Stage::Attack => {
                self.level += c.attack * (ATTACK_TARGET - self.level);
                if self.level >= 1.0 {
                    self.level = 1.0;
                    self.stage = Stage::Decay;
                }
            }
            Stage::Decay => {
                self.level += c.decay * (c.sustain - self.level);
            }
            Stage::Release => {
                self.level += c.release * (0.0 - self.level);
                if self.level < SILENT {
                    self.level = 0.0;
                    self.stage = Stage::Idle;
                }
            }
        }
        self.level
    }
}

impl Default for Env {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::measure::{decay_t60 as measure_t60, rise_10_90};

    const SR: f32 = 48_000.0;

    fn run(c: &EnvCoeffs, on_secs: f32, total_secs: f32) -> Vec<f32> {
        let mut e = Env::new();
        e.gate_on();
        let on = (on_secs * SR) as usize;
        (0..(total_secs * SR) as usize)
            .map(|i| {
                if i == on {
                    e.gate_off();
                }
                e.tick(c)
            })
            .collect()
    }

    /// **The brief's envelope gate**: the 10–90% rise and the decay both
    /// read within 5% of what the dial mapping promises, across the dials.
    #[test]
    fn attack_and_release_times_match_the_mapping_within_five_percent() {
        for dial in [0.2_f32, 0.4, 0.6, 0.8] {
            let c = EnvCoeffs::new(SR, dial, 0.5, 1.0, dial, 1.0);
            let want_rise = attack_rise_10_90(dial);
            let want_t60 = release_t60(dial);
            let x = run(&c, want_rise * 4.0 + 0.2, want_rise * 4.0 + 0.2 + want_t60 * 1.5);
            let on = ((want_rise * 4.0 + 0.2) * SR) as usize;
            let rise = rise_10_90(&x[..on], SR).expect("rose");
            assert!(
                (rise / want_rise - 1.0).abs() < 0.05,
                "attack dial {dial}: 10-90% rise {rise:.4} s, mapping says {want_rise:.4} s"
            );
            // The release, read by Schroeder integration on the envelope
            // itself (the ruler is written for audio; an envelope is the
            // envelope of a DC signal, and integrates the same way).
            let t60 = measure_t60(&x[on..], SR).expect("fell 35 dB");
            assert!(
                (t60 / want_t60 - 1.0).abs() < 0.05,
                "release dial {dial}: T60 {t60:.4} s, mapping says {want_t60:.4} s"
            );
        }
    }

    /// The decay lands on the sustain at the promised rate: with sustain
    /// at zero it is a release-shaped fall, T60 by the dial.
    #[test]
    fn the_decay_falls_at_the_dials_rate() {
        for dial in [0.3_f32, 0.6] {
            let c = EnvCoeffs::new(SR, 0.0, dial, 0.0, 0.5, 1.0);
            let want = decay_t60(dial);
            let x = run(&c, want * 1.5, want * 1.5);
            let peak_at = x.iter().position(|&v| v >= 1.0).unwrap();
            let t60 = measure_t60(&x[peak_at..], SR).expect("fell 35 dB");
            assert!(
                (t60 / want - 1.0).abs() < 0.05,
                "decay dial {dial}: T60 {t60:.4} s, mapping says {want:.4} s"
            );
        }
    }

    /// The attack is convex — a capacitor's curve, not a ramp: at half the
    /// rise time it is past half way.
    #[test]
    fn the_attack_is_convex() {
        let c = EnvCoeffs::new(SR, 0.5, 0.5, 1.0, 0.5, 1.0);
        let x = run(&c, 10.0, 10.0);
        let peak_at = x.iter().position(|&v| v >= 1.0).unwrap();
        let half = x[peak_at / 2];
        assert!(half > 0.6, "at half the attack the level is {half:.3}: a ramp, not a charge");
    }

    /// **A restrike attacks from the current level.** Gate on during the
    /// release leaves no sample lower than the one before it.
    #[test]
    fn a_gate_during_the_release_attacks_from_where_it_is() {
        let c = EnvCoeffs::new(SR, 0.3, 0.5, 0.5, 0.6, 1.0);
        let mut e = Env::new();
        e.gate_on();
        for _ in 0..4800 {
            e.tick(&c);
        }
        e.gate_off();
        for _ in 0..2400 {
            e.tick(&c);
        }
        let before = e.level();
        assert!(before > 0.05 && before < 0.5, "release level {before}");
        e.gate_on();
        let mut prev = before;
        let mut peaked = false;
        for i in 0..2000 {
            let v = e.tick(&c);
            if e.stage() != Stage::Attack {
                peaked = true;
                break;
            }
            assert!(v >= prev - 1e-7, "sample {i} fell from {prev} to {v} on the restrike");
            prev = v;
        }
        assert!(peaked && prev > before, "the restrike went nowhere: {before} -> {prev}");
    }

    /// The timing tolerance scales every stage: a voice at 1.05 takes 5%
    /// longer to rise.
    #[test]
    fn the_time_scale_stretches_the_attack() {
        let a = EnvCoeffs::new(SR, 0.5, 0.5, 1.0, 0.5, 1.0);
        let b = EnvCoeffs::new(SR, 0.5, 0.5, 1.0, 0.5, 1.05);
        let ra = rise_10_90(&run(&a, 10.0, 10.0), SR).unwrap();
        let rb = rise_10_90(&run(&b, 10.0, 10.0), SR).unwrap();
        assert!((rb / ra - 1.05).abs() < 0.01, "{ra} vs {rb}");
    }
}
