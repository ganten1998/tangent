//! The touch wah as a per-lane insert: the clav patch's envelope filter,
//! lifted out of the keys frame so anything can be played through it.
//!
//! # Where the sound comes from
//!
//! This is a fresh implementation, not an embedding — the original is thirty
//! lines inside the keys renderer's amp pass (`Patch::ClavWah` in
//! `keys/mod.rs`), welded to that instrument's frame. Every constant here is
//! that arm's, copied and named: the 3 ms attack and 140 ms release, the
//! ×5.5 envelope-to-drive gain, the 2.6-octave sweep, the 180..4800 Hz
//! corner clamp, the Chamberlin state-variable filter with its 0.7 frequency
//! ceiling, and the 1.15 : 0.30 band-to-low output blend. The two new dials
//! (`base`, `q`) default to exactly the values the keys arm hard-codes, so
//! the insert at rest IS the clav's pedal.
//!
//! # The quack is the asymmetry
//!
//! One envelope follower rides both channels' level — fast up, slow down —
//! and drags a resonant corner up the spectrum with it. The attack being
//! fifty times quicker than the release is what makes a note QUACK rather
//! than swell: the filter is already open when the transient lands and still
//! closing when the next one arrives.
//!
//! # The mix is a crossfade, unlike the other legacy inserts
//!
//! The chorus, echo and plate were sends — wet added, dry untouched. The wah
//! was never a send: the keys arm REPLACES the signal with the filtered one.
//! So `mix` here fades dry against wet, and 1.0 — the default — is the keys
//! arm exactly.

// Nothing reaches this module yet; the suite's engine lane wires it later.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// Where the resting corner sits, in Hz, geometric across the dial. The
/// midpoint is **240 Hz — the value the keys arm hard-codes** — so the
/// resting dial is the clav's pedal; the ends are a parked-down growl and a
/// parked-open honk.
pub const WAH_BASE_HZ: (f32, f32) = (80.0, 720.0);

/// The state-variable filter's damping across the q dial, linear, INVERTED —
/// up is sharper. The midpoint is **0.40, the keys arm's value** (Q = 2.5);
/// the bottom stops short of no resonance at all because a wah with no peak
/// is a tone control, and the top stops short of self-oscillation.
pub const WAH_DAMP: (f32, f32) = (0.7, 0.1);

/// The follower's ballistics, in milliseconds — the keys arm's own. Fast up,
/// slow down: the QUACK is the asymmetry.
pub const WAH_ATTACK_MS: f32 = 3.0;
pub const WAH_RELEASE_MS: f32 = 140.0;

/// Envelope level to sweep drive: the follower's output times this, capped
/// at one, is how far up the sweep sits. 5.5 means a signal at about −15
/// dBFS already opens the filter fully, which is what makes it respond to
/// TOUCH rather than to peaks.
pub const WAH_DRIVE: f32 = 5.5;

/// How many octaves the sens dial adds to the sweep at full travel, above
/// the one octave the sweep always has.
pub const WAH_OCTAVES: f32 = 2.6;

/// Where the swept corner is allowed to sit, in Hz — the keys arm's clamp.
/// The floor keeps a silent input from parking the filter under the note;
/// the ceiling keeps a hot one from running the resonance into the top.
pub const WAH_FC_HZ: (f32, f32) = (180.0, 4_800.0);

/// The Chamberlin integrator coefficient's ceiling. Past 0.7 the structure's
/// own stability limit is close and the keys arm never let it nearer.
const WAH_FCOEF_MAX: f32 = 0.7;

/// The output blend: mostly the band-pass (the vowel), a little of the
/// low-pass under it (the body). The keys arm's numbers.
pub const WAH_BAND_MIX: f32 = 1.15;
pub const WAH_LOW_MIX: f32 = 0.30;

/// Below this a 0..=1 dial is at its resting position and means OFF — the
/// same figure the fixed chain uses (`OFF` in `effects.rs`).
const WAH_OFF: f32 = 0.0005;

/// Below this a sample is on its way to a denormal and is worth nothing.
const TINY: f32 = 1.0e-20;

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to Hz and damping lives in the
/// consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WahParams {
    /// How hard the filter chases the touch: octaves of sweep above the
    /// base, `1 + [`WAH_OCTAVES`] · sens`.
    pub sens: f32,
    /// The resting corner across [`WAH_BASE_HZ`], geometric. Noon is the
    /// keys arm's 240 Hz.
    pub base: f32,
    /// Resonance across [`WAH_DAMP`], inverted (up is sharper). Noon is the
    /// keys arm's 0.40.
    pub q: f32,
    /// Dry/wet crossfade. 1 is the keys arm (the wah replaces the signal),
    /// 0 is a wire.
    pub mix: f32,
}

/// The shipped sound: the clav patch's own — sens at the patch's 0.55
/// default, base and q at the values the keys arm hard-codes, fully wet.
impl Default for WahParams {
    fn default() -> Self {
        Self {
            sens: 0.55,
            base: 0.5,
            q: 0.5,
            mix: 1.0,
        }
    }
}

// ── the wah ─────────────────────────────────────────────────────────────────

/// A touch-controlled resonant sweep: envelope follower into a Chamberlin
/// state-variable filter. See the module header; see [`Wah::process`] for
/// the contract.
pub struct Wah {
    sample_rate: f32,
    /// The follower — ONE for both channels, as the keys arm has it, so the
    /// two channels always hear the same corner and the image holds still.
    env: f32,
    /// The state-variable filter's two integrators, per channel.
    low: [f32; 2],
    band: [f32; 2],
    /// Whether the last call did nothing, so the off-clear runs once.
    idle: bool,
}

impl Wah {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            env: 0.0,
            low: [0.0; 2],
            band: [0.0; 2],
            idle: true,
        }
    }

    /// Apply the wah in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is left
    /// alone and fewer than two is returned untouched.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &WahParams,
    ) {
        // A dial is a number out of a settings file somebody may have edited:
        // clamp, and treat anything non-finite as the dial's resting place.
        let dial = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        let mix = dial(params.mix, 1.0);
        if mix < WAH_OFF {
            // Off is a wire AND a reset, once: a follower still holding the
            // last phrase would open the filter on the first note back.
            if !self.idle {
                self.clear();
                self.idle = true;
            }
            return;
        }
        self.idle = false;
        if channels < 2 {
            return;
        }

        // Design math in f64, once per block; the audio path stays f32,
        // exactly as the keys arm runs it.
        let sr = f64::from(self.sample_rate);
        let atk = (1.0 - (-1.0 / (f64::from(WAH_ATTACK_MS) * 0.001 * sr)).exp()) as f32;
        let rel = (1.0 - (-1.0 / (f64::from(WAH_RELEASE_MS) * 0.001 * sr)).exp()) as f32;
        let base = (f64::from(WAH_BASE_HZ.0)
            * f64::from(WAH_BASE_HZ.1 / WAH_BASE_HZ.0).powf(f64::from(dial(params.base, 0.5))))
            as f32;
        let damp = WAH_DAMP.0 + (WAH_DAMP.1 - WAH_DAMP.0) * dial(params.q, 0.5);
        let oct = 1.0 + WAH_OCTAVES * dial(params.sens, 0.55);

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let dry = [frame[0], frame[1]];

            // The follower rides the mean rectified level of the pair — the
            // keys arm's detector, kept mono so the corner cannot wander
            // between the channels.
            let lvl = (dry[0].abs() + dry[1].abs()) * 0.5;
            let coeff = if lvl > self.env { atk } else { rel };
            self.env = flush(self.env + (lvl - self.env) * coeff);

            // The sweep: level to drive, drive to octaves, octaves to a
            // corner — clamped where the keys arm clamps it.
            let drive = (self.env * WAH_DRIVE).clamp(0.0, 1.0);
            let fc = (base * 2.0_f32.powf(oct * drive)).clamp(WAH_FC_HZ.0, WAH_FC_HZ.1);
            let fcoef =
                (2.0 * (std::f32::consts::PI * fc / self.sample_rate).sin()).clamp(0.0, WAH_FCOEF_MAX);

            for (ch, &x) in dry.iter().enumerate() {
                self.low[ch] = flush(self.low[ch] + fcoef * self.band[ch]);
                let high = x - self.low[ch] - damp * self.band[ch];
                self.band[ch] = flush(self.band[ch] + fcoef * high);
                // The keys arm's safety net, kept: a state gone non-finite
                // resets rather than latching a screech.
                if !self.low[ch].is_finite() || !self.band[ch].is_finite() {
                    self.low[ch] = 0.0;
                    self.band[ch] = 0.0;
                }
                let wet = self.band[ch] * WAH_BAND_MIX + self.low[ch] * WAH_LOW_MIX;
                // dry + (wet − dry) · mix, so mix at one is the wet exactly
                // — the keys arm's replacement — with no rounding residue.
                frame[ch] = dry[ch] + (wet - dry[ch]) * mix;
            }
        }
    }

    /// Drop the follower and the filter states. Called when the unit leaves
    /// the circuit, and on a stop.
    pub fn clear(&mut self) {
        self.env = 0.0;
        self.low = [0.0; 2];
        self.band = [0.0; 2];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// An interleaved stereo square wave — harmonically rich, so the moving
    /// corner has a spectrum to move across, and with a flat rectified level,
    /// so the follower settles to the amplitude exactly.
    fn square(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            let ph = (i as f32 * hz / sr).fract();
            let v = if ph < 0.5 { amp } else { -amp };
            f[0] = v;
            f[1] = v;
        }
        buf
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0_f32, |m, x| m.max(x.abs()))
    }

    /// Left-channel RMS from `from` frames on.
    fn rms(buf: &[f32], from: usize) -> f32 {
        let left: Vec<f32> = buf.chunks_exact(2).skip(from).map(|f| f[0]).collect();
        (left.iter().map(|v| v * v).sum::<f32>() / left.len() as f32).sqrt()
    }

    /// How much of the left channel's energy sits above `cut`, 0..1, split
    /// with a one-pole each way — the same broad-band question the plate's
    /// dispersion test asks, asked of a spectrum instead of a delay.
    fn high_fraction(buf: &[f32], from: usize, cut: f32, sr: f32) -> f64 {
        let a = (-std::f32::consts::TAU * cut / sr).exp();
        let mut z = 0.0_f32;
        let (mut hi, mut lo) = (0.0_f64, 0.0_f64);
        for f in buf.chunks_exact(2).skip(from) {
            z = f[0] * (1.0 - a) + z * a;
            let h = f[0] - z;
            hi += f64::from(h) * f64::from(h);
            lo += f64::from(z) * f64::from(z);
        }
        hi / (hi + lo).max(1.0e-30)
    }

    /// **Mix at zero is a wire whatever the other dials say** — bit-exact.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_the_other_dials_say() {
        let frames = 2_048;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..8_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = WahParams {
                sens: d(0),
                base: d(1),
                q: d(2),
                mix: 0.0,
            };
            let mut unit = Wah::new(SR);
            let mut buf = square(220.0, 0.8, frames, SR);
            let before = buf.clone();
            unit.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "mix at zero touched the signal (dials {bits:03b})");
        }
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes against a signal past full scale: finite and bounded. The
    /// cap catches a runaway, not a resonant peak — the sharpest q on a note
    /// sitting under the corner legitimately rings well over the input.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 9_600;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..16_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = WahParams {
                sens: d(0),
                base: d(1),
                q: d(2),
                mix: d(3),
            };
            let mut unit = Wah::new(SR);
            for _ in 0..2 {
                let mut buf = square(220.0, 2.0, frames, SR);
                unit.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:04b} produced a non-finite sample"
                );
                assert!(
                    peak(&buf) < 64.0,
                    "dials {bits:04b} reached {}",
                    peak(&buf)
                );
            }
        }
    }

    /// **The same quack at every sample rate.** Every coefficient is derived
    /// from milliseconds or hertz, so a settled note through the defaults
    /// must come out at the same level at 44.1k, 48k and 96k.
    #[test]
    fn the_wah_is_the_same_quack_at_every_sample_rate() {
        let level_at = |sr: f32| {
            let p = WahParams::default();
            let frames = sr as usize;
            let mut unit = Wah::new(sr);
            // Loud enough that the drive is pinned at one: the corner sits
            // still and the measurement is of the filter, not the follower.
            let mut buf = square(110.0, 0.4, frames, sr);
            unit.process(&mut buf, frames, 2, &p);
            20.0 * rms(&buf, (sr * 0.5) as usize).log10()
        };
        let (a, b, c) = (level_at(44_100.0), level_at(48_000.0), level_at(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            assert!(
                (x - y).abs() < 1.0,
                "levels across rates disagree: {a:.2} / {b:.2} / {c:.2} dB"
            );
        }
    }

    /// **The character pin: playing harder opens the filter.** The corner
    /// follows the envelope, so the same waveform louder must come out
    /// BRIGHTER — a rising share of its energy above the split — which is the
    /// measurement that separates a touch wah from a fixed filter. The same
    /// approach that verified the keys arm: spectral balance against level.
    #[test]
    fn playing_harder_moves_the_energy_up_the_spectrum() {
        let brightness = |amp: f32| {
            let p = WahParams::default();
            let frames = SR as usize;
            let mut unit = Wah::new(SR);
            let mut buf = square(110.0, amp, frames, SR);
            unit.process(&mut buf, frames, 2, &p);
            // The back half: the follower has long settled either way.
            high_fraction(&buf, frames / 2, 600.0, SR)
        };
        // Quiet: drive ≈ 0.17, corner near 320 Hz. Loud: drive pinned at
        // one, corner past 1.2 kHz.
        let (quiet, loud) = (brightness(0.03), brightness(0.9));
        assert!(
            loud > quiet * 1.5,
            "loud playing put {loud:.3} of its energy up high against quiet's \
             {quiet:.3}: the corner is not following the touch"
        );
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut unit = Wah::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        unit.process(&mut buf, 256, 1, &WahParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **Clear forgets the last phrase.** A follower left holding a loud
    /// note would open the filter on the first quiet note after a stop.
    #[test]
    fn clear_forgets_what_was_playing() {
        let mut unit = Wah::new(SR);
        let frames = 4_800;
        let mut buf = square(220.0, 0.9, frames, SR);
        unit.process(&mut buf, frames, 2, &WahParams::default());
        assert!(unit.env > 0.1, "the follower never rose: {}", unit.env);
        unit.clear();
        assert_eq!(unit.env, 0.0);
        assert_eq!(unit.low, [0.0; 2]);
        assert_eq!(unit.band, [0.0; 2]);
    }
}
