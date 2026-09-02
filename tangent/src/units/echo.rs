//! The RE-201 tape echo as a per-lane insert: the fixed chain's machine with
//! a time dial of its own instead of the session's tempo.
//!
//! # What this is and is not
//!
//! The DSP is `effects::TapeEcho`, embedded whole — the mono tape loop, the
//! measured 1 : 1.94 : 2.85 head spacing, the `tanh` record amplifier, wow,
//! flutter, head bump and hiss. Nothing is reimplemented; the machine's own
//! constants stay next to it in `effects.rs`. This file owns the dials, the
//! time-in-seconds mapping, and the blend.
//!
//! # Free-running only, for now — and the parameter space knows it
//!
//! The fixed chain's echo follows the session tempo through [`Division`];
//! an insert cannot, yet, because the bay protocol does not carry the host's
//! bpm to a unit. So v1 is the machine's own truth — a capstan speed, free of
//! the clock — across [`ECHO_TIME_S`]. When the protocol grows a tempo, a
//! `sync` field will be APPENDED after `mix` (additive keys go at the end, so
//! every serialised order that exists keeps meaning what it meant), and
//! `time` will keep exactly this mapping for the unsynced position.
//!
//! [`Division`]: crate::effects::Division
//!
//! # The mix is the old send knob
//!
//! In the chain the echo was a send: wet ADDED to dry, the dry never
//! attenuated. `mix` here keeps that arithmetic — `dry + wet · mix` — so 1.0
//! is the chain at full send and 0 is a wire. A crossfade would thin the note
//! that is being repeated, which no echo box does.

// Nothing reaches this module yet; the suite's engine lane wires it later.
#![allow(dead_code)]

use crate::effects;

// ── what the dials map to ───────────────────────────────────────────────────

/// Where the time dial sweeps, in seconds at head one, mapped geometrically.
///
/// 30 ms is a slapback tighter than the machine's shortest useful setting;
/// 1.5 s is one repeat per breath — and head three, at 2.85 times head one,
/// still fits the six-second tape the embedded machine allocates.
pub const ECHO_TIME_S: (f32, f32) = (0.030, 1.5);

/// The time dial's resting position: the position that lands on **375 ms**,
/// which is what the fixed chain's free-running echo has always defaulted to
/// (`FREE_SECS` in `effects.rs`) — and, not by accident, the length the
/// embedded transport starts at, so the default never slews in from
/// somewhere else.
pub const ECHO_TIME_DEFAULT: f32 = 0.645_634;

/// Below this a 0..=1 dial is at its resting position and means OFF — the
/// same figure the fixed chain uses (`OFF` in `effects.rs`).
const ECHO_OFF: f32 = 0.0005;

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. `feedback` and `tone` pass straight through
/// to the embedded machine, whose mappings (`TAPE_FEEDBACK`, `TAPE_TONE_HZ`)
/// are documented next to the DSP in `effects.rs`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EchoParams {
    /// Head one's time across [`ECHO_TIME_S`], geometric.
    pub time: f32,
    /// Intensity: how much of each repeat comes back for the next. The top
    /// of the travel passes unity and self-oscillates, because the machine's
    /// does — the `tanh` in the loop is what keeps that bounded.
    pub feedback: f32,
    /// How much darker each repeat is than the one before it.
    pub tone: f32,
    /// Wet level, ADDED to the dry — see the module header. 0 is a wire.
    /// **Last on purpose**: a `sync` field will be appended after it.
    pub mix: f32,
}

/// The shipped sound: the fixed chain's free-running default — 375 ms at
/// head one, repeats that decay, a middling darkness — with the send full up.
impl Default for EchoParams {
    fn default() -> Self {
        Self {
            time: ECHO_TIME_DEFAULT,
            feedback: 0.42,
            tone: 0.55,
            mix: 1.0,
        }
    }
}

// ── the echo ────────────────────────────────────────────────────────────────

/// The insert. See the module header; see [`Echo::process`] for the contract.
pub struct Echo {
    /// The machine itself, unchanged from the fixed chain.
    machine: effects::TapeEcho,
    /// The chain's parameter block, the machine's native dial format. Only
    /// `delay_feedback` and `delay_tone` are ever written; `echo_mode` stays
    /// at head one and `delay_width` at the shipped 0.60, which is exactly
    /// the fixed chain's default voice.
    chain: effects::Params,
    sample_rate: f32,
    /// Whether the last call did nothing, so the off-clear runs once.
    idle: bool,
}

impl Echo {
    pub fn new(sample_rate: f32) -> Self {
        let sr = sample_rate.max(8_000.0);
        Self {
            machine: effects::TapeEcho::new(sr),
            chain: effects::Params::default(),
            sample_rate: sr,
            idle: true,
        }
    }

    /// Apply the echo in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is left
    /// alone and fewer than two is returned untouched.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &EchoParams,
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
        if mix < ECHO_OFF {
            // Off is a wire AND a reset, once: six seconds of tape must not
            // still be playing when the mix comes back up an hour later.
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

        // Design math in f64, once per block. The time dial is geometric so
        // equal turns are equal ratios — a delay dial that is linear in
        // seconds spends half its travel between 0.75 s and 1.5 s.
        let t = f64::from(dial(params.time, ECHO_TIME_DEFAULT));
        let secs = f64::from(ECHO_TIME_S.0)
            * f64::from(ECHO_TIME_S.1 / ECHO_TIME_S.0).powf(t);
        // The machine slews toward this like the capstan it is — handing it a
        // target rather than a length is what keeps the pitch-slide on a
        // moving dial.
        let target = (secs * f64::from(self.sample_rate)).max(1.0) as f32;

        self.chain.delay_feedback = dial(params.feedback, 0.42);
        self.chain.delay_tone = dial(params.tone, 0.55);

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            let dry = [frame[0], frame[1]];
            let wet = self.machine.process(dry, target, &self.chain);
            frame[0] = dry[0] + wet[0] * mix;
            frame[1] = dry[1] + wet[1] * mix;
        }
    }

    /// Wipe the tape and the loop filters. Called when the unit leaves the
    /// circuit, and on a stop.
    pub fn clear(&mut self) {
        self.machine.clear();
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

    /// **Mix at zero is a wire whatever the other dials say** — bit-exact.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_the_other_dials_say() {
        let frames = 2_048;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..8_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = EchoParams {
                time: d(0),
                feedback: d(1),
                tone: d(2),
                mix: 0.0,
            };
            let mut unit = Echo::new(SR);
            let mut buf = sine(440.0, 0.8, frames, SR);
            let before = buf.clone();
            unit.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "mix at zero touched the signal (dials {bits:03b})");
        }
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes against a signal past full scale, twice over so the loop has
    /// been round the tape: finite and bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 9_600;
        let extremes = [0.0_f32, 1.0];
        for bits in 0..16_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = EchoParams {
                time: d(0),
                feedback: d(1),
                tone: d(2),
                mix: d(3),
            };
            let mut unit = Echo::new(SR);
            for _ in 0..4 {
                let mut buf = sine(220.0, 2.0, frames, SR);
                unit.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:04b} produced a non-finite sample"
                );
                assert!(
                    peak(&buf) < 8.0,
                    "dials {bits:04b} reached {}",
                    peak(&buf)
                );
            }
        }
    }

    /// **The time dial tells the truth at every sample rate.** An impulse
    /// through the default 375 ms comes back as a repeat 375 ms later — give
    /// or take the wow the transport is supposed to have — at 44.1k, 48k and
    /// 96k alike, because every length in the machine is scaled from seconds.
    #[test]
    fn the_repeat_lands_on_the_dialled_time_at_every_sample_rate() {
        let arrival_at = |sr: f32| {
            let p = EchoParams {
                feedback: 0.0,
                ..EchoParams::default()
            };
            let frames = (sr * 0.6) as usize;
            let mut unit = Echo::new(sr);
            let mut buf = vec![0.0_f32; frames * 2];
            buf[0] = 1.0;
            buf[1] = 1.0;
            unit.process(&mut buf, frames, 2, &p);
            // The loudest left-channel sample after the strike itself is the
            // head-one repeat; the skip clears the strike and its immediate
            // smear.
            let skip = (sr * 0.05) as usize;
            let (mut at, mut best) = (0usize, 0.0_f32);
            for (i, f) in buf.chunks_exact(2).enumerate().skip(skip) {
                if f[0].abs() > best {
                    best = f[0].abs();
                    at = i;
                }
            }
            at as f32 / sr
        };
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            let t = arrival_at(sr);
            assert!(
                (t - 0.375).abs() < 0.375 * 0.03,
                "at {sr} Hz the repeat landed at {t:.4} s against a 375 ms dial"
            );
        }
    }

    /// **The character pin: Intensity at the top is bounded by the tape, not
    /// by luck.** The dial deliberately passes unity — the machine's howl —
    /// and the `tanh` in the inherited loop is what catches it: fourteen
    /// seconds after one strike the loop is louder than any decaying delay
    /// could still be (so the gain really is past unity) and under full
    /// scale (so the saturation really is the ceiling).
    ///
    /// Fourteen seconds because of where the howl comes FROM: the strike
    /// itself is smeared apart by the wow and flutter — each lap re-times it
    /// a fraction of a percent, which is exactly what stops the seed
    /// recirculating coherently — and what actually builds is the loop's own
    /// hiss at whichever frequency the loop favours, doubling roughly every
    /// second. The fixed chain's own self-oscillation test waits eleven
    /// seconds for the same reason.
    #[test]
    fn the_feedback_at_the_top_howls_without_diverging() {
        let p = EchoParams {
            feedback: 1.0,
            ..EchoParams::default()
        };
        let mut unit = Echo::new(SR);
        let block = 4_800;
        let mut worst = 0.0_f32;
        let mut last = 0.0_f32;
        for b in 0..140 {
            let mut buf = vec![0.0_f32; block * 2];
            if b == 0 {
                // One strike, then silence: whatever follows is the loop's.
                buf[0] = 0.9;
                buf[1] = 0.9;
            }
            unit.process(&mut buf, block, 2, &p);
            assert!(
                buf.iter().all(|s| s.is_finite()),
                "block {b} went non-finite"
            );
            last = peak(&buf);
            worst = worst.max(last);
        }
        assert!(
            worst < 6.0,
            "the loop climbed to {worst}: the saturation is not catching it"
        );
        assert!(
            last > 0.05,
            "fourteen seconds on the loop is down to {last:e}: the top of the \
             dial is a long delay, not the machine's howl"
        );
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut unit = Echo::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        unit.process(&mut buf, 256, 1, &EchoParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// The default dial position really is the chain's 375 ms — the mapping
    /// arithmetic inverted, held to a tenth of a millisecond.
    #[test]
    fn the_default_time_dial_lands_on_the_chains_375_ms() {
        let secs = f64::from(ECHO_TIME_S.0)
            * f64::from(ECHO_TIME_S.1 / ECHO_TIME_S.0).powf(f64::from(ECHO_TIME_DEFAULT));
        assert!(
            (secs - 0.375).abs() < 1.0e-4,
            "the resting time dial maps to {secs:.5} s, not 375 ms"
        );
    }
}
