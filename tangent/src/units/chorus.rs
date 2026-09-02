//! The CE-1 chorus as a per-lane insert: the fixed chain's pedal, given its
//! own dials and a dry/wet of its own.
//!
//! # What this is and is not
//!
//! The DSP is `effects::Chorus`, embedded whole — one bucket-brigade line,
//! two outputs, the right one inverted, the noise gate, the hiss. Nothing is
//! reimplemented here, because a second copy of a voiced circuit is a second
//! thing to keep in tune. The mapping constants therefore live next to that
//! DSP (`CHORUS_RATE_HZ`, `CHORUS_FLOOR_MS`, `CHORUS_SPAN_MS`,
//! `CHORUS_LP_HZ` in `effects.rs`); this file only owns the dials and the
//! blend.
//!
//! # The mix is the old send knob
//!
//! In the fixed chain the chorus was a send: the wet was ADDED to the dry at
//! the knob's strength and the dry never attenuated, which is also what the
//! pedal itself does — a CE-1's output is the instrument plus the delayed
//! copy. So `mix` here is additive, `dry + wet · mix`, not an equal-power
//! crossfade: at 1.0 it is the pedal (and the chain at full send), at 0 it is
//! a wire, and no position thins the dry. A crossfade would hand back a bare
//! vibrato at the top of the travel, which no chorus box ever did.

// Nothing reaches this module yet; the suite's engine lane wires it later.
#![allow(dead_code)]

use crate::effects;

/// Below this a 0..=1 dial is at its resting position and means OFF — the
/// same figure the fixed chain uses (`OFF` in `effects.rs`).
const CHORUS_OFF: f32 = 0.0005;

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1, passed straight through to the embedded
/// pedal — the Hz/ms these land on are documented next to the DSP in
/// `effects.rs`, where they were voiced.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ChorusParams {
    /// Sweep speed across `CHORUS_RATE_HZ` (0.12..3.2 Hz).
    pub rate: f32,
    /// How far the sweep opens the delay above its fixed 3 ms floor,
    /// across `CHORUS_SPAN_MS` (2..4 ms).
    pub depth: f32,
    /// How much of the wet is inverted into the right channel — the CE-1's
    /// own stereo trick. 0 is mono-safe, 1 is the pedal.
    pub width: f32,
    /// The bucket brigade's bandwidth across `CHORUS_LP_HZ` (2.6..9 kHz).
    /// Down is darker and more like the hardware.
    pub tone: f32,
    /// Wet level, ADDED to the dry — see the module header. 0 is a wire.
    pub mix: f32,
}

/// The shipped sound: exactly the fixed chain's defaults with the send full
/// up — a slow, wide, slightly dark chorus.
impl Default for ChorusParams {
    fn default() -> Self {
        Self {
            rate: 0.28,
            depth: 0.55,
            width: 0.85,
            tone: 0.45,
            mix: 1.0,
        }
    }
}

// ── the chorus ──────────────────────────────────────────────────────────────

/// The insert. See the module header; see [`Chorus::process`] for the
/// contract.
pub struct Chorus {
    /// The pedal itself, unchanged from the fixed chain.
    pedal: effects::Chorus,
    /// The chain's parameter block, kept as the pedal's native dial format.
    /// Only the four chorus fields are ever written; everything else stays at
    /// its default and the pedal never reads it.
    chain: effects::Params,
    /// Whether the last call did nothing, so the off-clear runs once.
    idle: bool,
}

impl Chorus {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            pedal: effects::Chorus::new(sample_rate.max(8_000.0)),
            chain: effects::Params::default(),
            idle: true,
        }
    }

    /// Apply the chorus in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is left
    /// alone and fewer than two is returned untouched, as every unit here
    /// does.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &ChorusParams,
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
        if mix < CHORUS_OFF {
            // Off is a wire AND a reset, once: the delay line must not hold
            // half a second of yesterday when the mix comes back up.
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

        // The pedal reads its dials per sample, so they are written once per
        // block — clamped HERE because the embedded process trusts its
        // caller, exactly as the fixed chain's caller clamps for it.
        self.chain.chorus_rate = dial(params.rate, 0.28);
        self.chain.chorus_depth = dial(params.depth, 0.55);
        self.chain.chorus_width = dial(params.width, 0.85);
        self.chain.chorus_tone = dial(params.tone, 0.45);

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let dry = [frame[0], frame[1]];
            let wet = self.pedal.process(dry, &self.chain);
            frame[0] = dry[0] + wet[0] * mix;
            frame[1] = dry[1] + wet[1] * mix;
        }
    }

    /// Drop everything in the delay line, the gate and the filters. Called
    /// when the unit leaves the circuit, and on a stop.
    pub fn clear(&mut self) {
        self.pedal.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// An interleaved stereo sine, both channels in phase.
    fn sine(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            let v = amp * (i as f32 * hz * std::f32::consts::TAU / sr).sin();
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

    /// **Mix at zero is a wire whatever the other dials say** — bit-exact,
    /// because "this insert is off" must survive a null test.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_the_other_dials_say() {
        let frames = 2_048;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..16_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = ChorusParams {
                rate: d(0),
                depth: d(1),
                width: d(2),
                tone: d(3),
                mix: 0.0,
            };
            let mut unit = Chorus::new(SR);
            let mut buf = sine(440.0, 0.8, frames, SR);
            let before = buf.clone();
            unit.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "mix at zero touched the signal (dials {bits:04b})");
        }
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes against a signal past full scale: finite and bounded. The wet
    /// is a delayed unity copy plus hiss, so the honest ceiling is low.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 9_600;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..32_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = ChorusParams {
                rate: d(0),
                depth: d(1),
                width: d(2),
                tone: d(3),
                mix: d(4),
            };
            let mut unit = Chorus::new(SR);
            for _ in 0..2 {
                let mut buf = sine(220.0, 2.0, frames, SR);
                unit.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:05b} produced a non-finite sample"
                );
                assert!(
                    peak(&buf) < 8.0,
                    "dials {bits:05b} reached {}",
                    peak(&buf)
                );
            }
        }
    }

    /// **The same shimmer at every sample rate.** The pedal's line is scaled
    /// in milliseconds and its filters in hertz, so the settled output level
    /// through the defaults must agree at 44.1k, 48k and 96k.
    #[test]
    fn the_chorus_is_the_same_shimmer_at_every_sample_rate() {
        let level_at = |sr: f32| {
            let p = ChorusParams::default();
            // Two seconds: a couple of full sweeps at the default rate, so
            // the comb's moving cancellations average out the same way
            // everywhere.
            let frames = (sr * 2.0) as usize;
            let mut unit = Chorus::new(sr);
            let mut buf = sine(1_000.0, 0.25, frames, sr);
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

    /// **The character pin: one delay line, and the width dial inverts it
    /// into the right channel.** Identical input on both channels must come
    /// out identical at width 0 (mono-safe) and pushed apart at width 1 (the
    /// CE-1's polarity trick) — the same measurement the fixed chain's own
    /// test makes, now through the insert.
    #[test]
    fn the_width_dial_inverts_the_wet_into_the_right_channel() {
        let difference = |width: f32| {
            let p = ChorusParams {
                width,
                mix: 1.0,
                ..ChorusParams::default()
            };
            let frames = 24_000;
            let mut unit = Chorus::new(SR);
            let mut buf = sine(220.0, 0.5, frames, SR);
            unit.process(&mut buf, frames, 2, &p);
            // The input was identical in both channels, so any spread is the
            // effect's own.
            buf.chunks_exact(2)
                .skip(12_000)
                .fold(0.0_f32, |m, f| m.max((f[0] - f[1]).abs()))
        };
        let (narrow, wide) = (difference(0.0), difference(1.0));
        assert!(
            narrow < 1.0e-6,
            "at zero width the channels differ by {narrow}: not mono-safe"
        );
        assert!(
            wide > 0.05,
            "at full width the channels differ by only {wide}: the inversion is gone"
        );
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut unit = Chorus::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        unit.process(&mut buf, 256, 1, &ChorusParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }
}
