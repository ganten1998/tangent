//! The EMT 140 plate as a per-lane insert: the fixed chain's tank with its
//! own four dials and a dry/wet of its own.
//!
//! # What this is and is not
//!
//! The DSP is `effects::Plate`, embedded whole — the figure-of-eight tank,
//! the dispersion chirp, the seven taps, the swept allpass pair. Nothing is
//! reimplemented, and the tank's tables stay next to it in `effects.rs`.
//! The damp, dispersion and mode stay at the chain's shipped voice (0.55,
//! 0.75, plate): four dials is the insert's face, and the ones left off are
//! left at the sound people already know.
//!
//! # Decay changes how long, not how loud
//!
//! The energy normalisation is the chain's own, reused rather than
//! re-derived: the tank's impulse energy grows as `1/(1 − g²)`, so the wet is
//! trimmed by the square root of that, referenced to the decay the send was
//! voiced at (`PLATE_TRIM`, `PLATE_TRIM_AT` in `effects.rs`). Without it a
//! longer plate is a louder plate and the decay dial fights the mix dial —
//! the pinned test below is what holds this.
//!
//! # The mix is the old send knob
//!
//! Wet ADDED to dry at the knob's strength, exactly the chain's sum; 1.0 is
//! the chain at full send, 0 is a wire, and no position thins the dry.

// Nothing reaches this module yet; the suite's engine lane wires it later.
#![allow(dead_code)]

use crate::effects;

/// Below this a 0..=1 dial is at its resting position and means OFF — the
/// same figure the fixed chain uses (`OFF` in `effects.rs`).
const PLATE_OFF: f32 = 0.0005;

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1, passed through to the embedded tank — the
/// seconds and milliseconds these land on are documented next to the DSP in
/// `effects.rs` (`PLATE_DECAY_S`, `PLATE_PREDELAY_MS`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlateParams {
    /// How long the plate rings, across `PLATE_DECAY_S` (0.5..5.5 s).
    pub decay: f32,
    /// Silence in front of the plate, across `PLATE_PREDELAY_MS` (0..150 ms)
    /// — the tape machine every studio patched ahead of theirs.
    pub predelay: f32,
    /// How much of the two pickups' difference survives. 0 is the 1957
    /// single-pickup plate, 1 is the 140ST.
    pub width: f32,
    /// Wet level, ADDED to the dry — see the module header. 0 is a wire.
    pub mix: f32,
}

/// The shipped sound: the fixed chain's defaults with the send full up — a
/// 2.2-second plate, no predelay, wide.
impl Default for PlateParams {
    fn default() -> Self {
        Self {
            decay: 0.34,
            predelay: 0.0,
            width: 0.80,
            mix: 1.0,
        }
    }
}

// ── the plate ───────────────────────────────────────────────────────────────

/// The insert. See the module header; see [`Plate::process`] for the
/// contract.
pub struct Plate {
    /// The tank itself, unchanged from the fixed chain.
    tank: effects::Plate,
    /// The chain's parameter block, the tank's native dial format. Only
    /// `reverb_size` and `reverb_width` are ever written; damp, dispersion
    /// and mode hold the shipped voice.
    chain: effects::Params,
    /// Whether the last call did nothing, so the off-clear runs once.
    idle: bool,
}

impl Plate {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            tank: effects::Plate::new(sample_rate.max(8_000.0)),
            chain: effects::Params::default(),
            idle: true,
        }
    }

    /// Apply the plate in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is left
    /// alone and fewer than two is returned untouched.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &PlateParams,
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
        if mix < PLATE_OFF {
            // Off is a wire AND a reset, once: five seconds of tail must not
            // still be ringing in the tank when the mix comes back up.
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

        self.chain.reverb_size = dial(params.decay, 0.34);
        self.chain.reverb_width = dial(params.width, 0.80);
        let predelay = dial(params.predelay, 0.0);

        // The chain's energy normalisation, once per block: longer must not
        // mean louder. See the module header for the physics; the arithmetic
        // is copied from the send bus (`Effects::process`) rather than
        // re-derived, so the insert and the chain can never disagree.
        let g = self.tank.feedback(effects::Plate::decay_secs(self.chain.reverb_size));
        let g0 = self.tank.feedback(effects::Plate::decay_secs(effects::PLATE_TRIM_AT));
        let energy = |g: f32| (1.0 - g * g).max(1.0e-6).sqrt();
        let trim = effects::PLATE_TRIM * energy(g) / energy(g0);
        let wet_gain = trim * mix;

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let dry = [frame[0], frame[1]];
            let wet = self.tank.process(dry, &self.chain, predelay);
            frame[0] = dry[0] + wet[0] * wet_gain;
            frame[1] = dry[1] + wet[1] * wet_gain;
        }
    }

    /// Empty the tank, the predelay and every diffuser. Called when the unit
    /// leaves the circuit, and on a stop.
    pub fn clear(&mut self) {
        self.tank.clear();
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

    /// One strike into a fresh unit at this decay, and the whole output back.
    /// The strike is a single frame, so everything after frame zero is tail.
    fn strike(decay: f32, secs: f32, sr: f32) -> Vec<f32> {
        let p = PlateParams {
            decay,
            ..PlateParams::default()
        };
        let frames = (sr * secs) as usize;
        let mut unit = Plate::new(sr);
        let mut buf = vec![0.0_f32; frames * 2];
        buf[0] = 0.8;
        buf[1] = 0.8;
        unit.process(&mut buf, frames, 2, &p);
        buf
    }

    /// **Mix at zero is a wire whatever the other dials say** — bit-exact.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_the_other_dials_say() {
        let frames = 2_048;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..8_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = PlateParams {
                decay: d(0),
                predelay: d(1),
                width: d(2),
                mix: 0.0,
            };
            let mut unit = Plate::new(SR);
            let mut buf = sine(440.0, 0.8, frames, SR);
            let before = buf.clone();
            unit.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "mix at zero touched the signal (dials {bits:03b})");
        }
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes against a steady signal past full scale, long enough for the
    /// tank to fill: finite and bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 9_600;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..16_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = PlateParams {
                decay: d(0),
                predelay: d(1),
                width: d(2),
                mix: d(3),
            };
            let mut unit = Plate::new(SR);
            for _ in 0..4 {
                let mut buf = sine(220.0, 2.0, frames, SR);
                unit.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:04b} produced a non-finite sample"
                );
                // The cap catches a runaway, not a hot setting: a steady tone
                // sitting on a tank mode at full decay legitimately rings up
                // a few times over the input.
                assert!(
                    peak(&buf) < 16.0,
                    "dials {bits:04b} reached {}",
                    peak(&buf)
                );
            }
        }
    }

    /// **The same room at every sample rate.** Every length in the tank is
    /// scaled from the design rate, so a strike's tail must last the same
    /// number of SECONDS at 44.1k, 48k and 96k — the same measurement the
    /// chain's own rate test makes, because the length is what the ear pins
    /// a room by. (The tail's raw RMS is NOT rate-invariant: a discrete
    /// echo's amplitude survives resampling while the echoes-per-sample
    /// density halves at a doubled rate, so a level comparison here would be
    /// asking the network for something no delay network does.)
    #[test]
    fn the_plate_is_the_same_room_at_every_sample_rate() {
        let tail_secs = |sr: f32| {
            let out = strike(0.34, 2.0, sr);
            let left: Vec<f32> = out.chunks_exact(2).map(|f| f[0].abs()).collect();
            let peak = left.iter().skip(1_000).fold(0.0_f32, |m, v| m.max(*v));
            // Where it last stood above a thousandth of its loudest.
            left.iter().rposition(|v| *v > peak * 0.001).unwrap_or(0) as f32 / sr
        };
        let (a, b, c) = (tail_secs(44_100.0), tail_secs(48_000.0), tail_secs(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            assert!(
                (x - y).abs() < x.max(y) * 0.2,
                "tail lengths across rates disagree: {a:.2} / {b:.2} / {c:.2} s"
            );
        }
    }

    /// **The character pin: a longer decay is not a louder one.** The tail's
    /// total energy at decay 0.3 and 0.9 must sit within a factor of two —
    /// the inherited `1/(1 − g²)` trim at work — while the late tail proves
    /// the dial still changes how LONG it rings.
    #[test]
    fn a_longer_decay_rings_longer_but_not_louder() {
        let energy = |decay: f32| {
            let out = strike(decay, 2.0, SR);
            // From frame 1: everything but the dry strike itself.
            out.iter()
                .skip(2)
                .map(|s| f64::from(*s) * f64::from(*s))
                .sum::<f64>()
                .sqrt()
        };
        let (short, long) = (energy(0.3), energy(0.9));
        assert!(
            long < short * 2.0 && short < long * 2.0,
            "decay 0.3 carries {short:e} against 0.9's {long:e}: the decay \
             dial is a volume control"
        );

        // And it still rings longer: a second and a half in, the long plate
        // must be well above the short one.
        let late = |decay: f32| {
            let out = strike(decay, 2.0, SR);
            peak(&out[(SR as usize + SR as usize / 2) * 2..])
        };
        let (s, l) = (late(0.3), late(0.9));
        assert!(
            l > s * 2.0,
            "at 1.5 s the long decay holds {l:e} against the short's {s:e}: \
             the dial is not changing the length"
        );
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut unit = Plate::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        unit.process(&mut buf, 256, 1, &PlateParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }
}
