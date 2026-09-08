//! The compressor kernel for Tangent Comp: stereo-linked, feed-forward, with
//! an external key input.
//!
//! # The shape
//!
//! One detector for both channels, because a compressor that squeezes left
//! and right separately drags the image toward whichever side is quieter —
//! a snare panned ten degrees walks to the middle on every hit. So the
//! detector reads `max(|L|, |R|)` and the same gain lands on both channels.
//!
//! The detector is a blend of a short RMS window and the instantaneous peak.
//! RMS alone answers "how loud is this passage" and misses the transient
//! that clips the next stage; peak alone chatters on anything dense. A fixed
//! blend, mostly RMS with the peak reaching through it, gets the musical
//! reading with enough speed left for the attack dial to mean something.
//! The RMS side carries a +3 dB sine calibration so a steady tone reads its
//! peak amplitude and one threshold number serves both halves of the blend.
//!
//! Everything after the detector happens in decibels. The static curve is
//! the limiter's quadratic soft knee (`Limiter::wanted` in `effects.rs`)
//! with its infinite slope generalised to the dialled ratio, and the
//! attack/release smoothing is a branching one-pole on the gain-reduction
//! figure itself — dB in, dB out — because a one-pole in the linear domain
//! releases fast when the reduction is deep and slow when it is shallow,
//! which is backwards from what the dial promises.
//!
//! # The key
//!
//! `process` takes an optional side-chain: when a key buffer is handed in,
//! the detector listens to the key and the gain still lands on the main
//! signal — ducking, de-essing from another lane, kick-under-bass. The key
//! path has a one-pole high-pass so a kick's fundamental can be kept out of
//! the detector; the filter also applies to the internal detector when no
//! key is present, because "stop pumping on the bass" is the same request
//! either way.
//!
//! # Sample rate
//!
//! Taken once, in `new`. There are no length tables here to scale from a
//! design rate: every coefficient is derived from milliseconds or hertz at
//! the top of each block, so the kernel is rate-invariant by construction —
//! and a test holds it to that at 44.1k, 48k and 96k.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point. Until then every
// item here is dead to rustc.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// Where the threshold dial sweeps, in dBFS. Linear in dB across the travel,
/// like the limiter's, and off at the TOP — fully clockwise it sits at 0 dB
/// above everything and catches nothing, which is what a threshold is on
/// hardware.
pub const COMP_THRESHOLD_DB: (f32, f32) = (-60.0, 0.0);

/// The top of the ratio dial. The mapping is geometric — `20^t` — so the
/// bottom is exactly 1 : 1 (a wire), the top is 20 : 1 (a limiter in all but
/// name), and 4 : 1, the ratio everybody means by "compression", lands just
/// under the centre of the travel at t ≈ 0.46. A linear map would spend half
/// the dial between 10 : 1 and 20 : 1, which nobody can hear apart.
pub const COMP_RATIO_MAX: f32 = 20.0;

/// What the attack dial spans, in milliseconds, mapped geometrically. 0.1 ms
/// is effectively instant at any rate this app opens; 100 ms lets every
/// transient through untouched and squeezes only the body.
pub const COMP_ATTACK_MS: (f32, f32) = (0.1, 100.0);

/// What the release dial spans, in milliseconds, mapped geometrically. 20 ms
/// is pumping as an effect; 1200 ms is one breath per phrase.
pub const COMP_RELEASE_MS: (f32, f32) = (20.0, 1200.0);

/// What the makeup dial spans, in dB, linear, unity at noon. The very bottom
/// of the travel lets go to silence rather than stopping at −24: a makeup
/// that reaches "off" means a parallel blend can be trimmed all the way out
/// without touching the mix control.
pub const COMP_MAKEUP_DB: (f32, f32) = (-24.0, 24.0);

/// Where the key high-pass sweeps, in Hz, geometric. The bottom of the dial
/// is OFF — the detector hears everything — and the first hair of travel
/// starts at 20 Hz, which hears everything anyway; 500 Hz at the top is
/// enough to blind the detector to any kick or bass fundamental while it
/// still sees the note's own body.
pub const COMP_KEY_HPF_HZ: (f32, f32) = (20.0, 500.0);

/// Full width of the soft knee, in dB, centred on the threshold.
///
/// Hidden rather than dialled, to start: 6 dB is wide enough that a level
/// drifting across the threshold is bent rather than grabbed, and narrow
/// enough that the threshold number still means what it says.
const COMP_KNEE_DB: f32 = 6.0;

/// The detector's RMS window, and the peak side's release, in milliseconds.
///
/// About 5 ms: long enough to average over a full cycle of anything above
/// 200 Hz so a bass note reads as a level rather than as a wobble, short
/// enough that the reading arrives inside the fastest attack anybody dials.
const COMP_WINDOW_MS: f32 = 5.0;

/// How far the detector leans toward the instantaneous peak, 0 (all RMS) to
/// 1 (all peak). Mostly RMS, with enough peak reaching through that a lone
/// transient moves the gain before the window has finished averaging it away.
const COMP_PEAK_BLEND: f32 = 0.2;

/// Below this a 0..=1 dial is at its resting position and means OFF.
/// The same figure the fixed chain uses (`OFF` in `effects.rs`), duplicated
/// because this kernel must stand alone — the plugin suite will not link
/// that file.
const COMP_OFF: f32 = 0.0005;

/// Below this a sample is on its way to a denormal and is worth nothing;
/// see `TINY` in `effects.rs` for the arithmetic.
const TINY: f32 = 1.0e-20;

/// Zero anything too small to hear, before it becomes expensive to add.
#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompParams {
    /// Across [`COMP_THRESHOLD_DB`]; off at the top of the travel.
    pub threshold: f32,
    /// `COMP_RATIO_MAX^t` — 1 : 1 at the bottom, 4 : 1 just under noon.
    pub ratio: f32,
    /// Across [`COMP_ATTACK_MS`], geometric.
    pub attack: f32,
    /// Across [`COMP_RELEASE_MS`], geometric.
    pub release: f32,
    /// Across [`COMP_MAKEUP_DB`]; unity at noon, silence at the very bottom.
    pub makeup: f32,
    /// Dry/wet, for parallel compression. 1 is the compressor, 0 is a wire.
    pub mix: f32,
    /// The detector's high-pass across [`COMP_KEY_HPF_HZ`]; 0 is off.
    pub key_hpf: f32,
}

/// The shipped sound: threshold at −18 dB, 4 : 1, about 6 ms onto the note
/// and 150 ms off it, no makeup, fully wet, detector hearing everything.
/// A vocal or a drum bus through the defaults should just sound "held".
impl Default for CompParams {
    fn default() -> Self {
        Self {
            threshold: 0.7,
            ratio: 0.463,
            attack: 0.6,
            release: 0.5,
            makeup: 0.5,
            mix: 1.0,
            key_hpf: 0.0,
        }
    }
}

// ── the compressor ──────────────────────────────────────────────────────────

/// A stereo-linked feed-forward compressor with an external key input. See
/// the module header for the design; see [`Comp::process`] for the contract.
pub struct Comp {
    sample_rate: f32,
    /// One-pole low-pass state per channel; the key high-pass is the input
    /// minus this. Kept primed at the input while the filter is off, so
    /// switching it in opens from silence instead of thumping the detector.
    key_lp: [f32; 2],
    /// The RMS window: a one-pole average of the rectified detector input,
    /// squared.
    rms_sq: f32,
    /// The peak side: instant attack, [`COMP_WINDOW_MS`] release.
    penv: f32,
    /// The smoothed gain reduction, in dB, zero or below. This is the value
    /// the attack and release dials move.
    gr_db: f32,
    /// The meter's max-hold since the last read, in positive dB.
    hold_db: f32,
    /// Whether the last call did nothing, so the off-clear runs once rather
    /// than every block.
    idle: bool,
}

impl Comp {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            key_lp: [0.0; 2],
            rms_sq: 0.0,
            penv: 0.0,
            gr_db: 0.0,
            hold_db: 0.0,
            idle: true,
        }
    }

    /// The static curve: the gain a detector level demands, in dB of
    /// reduction (zero or below), given how far over the threshold it is.
    ///
    /// This is the limiter's quadratic soft knee (`Limiter::wanted`,
    /// `effects.rs`) with the ∞ : 1 slope generalised: `slope` is
    /// `1/ratio − 1`, the knee spans [`COMP_KNEE_DB`] centred on the
    /// threshold, and the quadratic meets both straight segments tangent —
    /// no corner for a level to sit on and buzz.
    #[inline]
    fn wanted_db(over_db: f32, slope: f32) -> f32 {
        let w = COMP_KNEE_DB;
        if 2.0 * over_db <= -w {
            0.0
        } else if 2.0 * over_db < w {
            let t = over_db + 0.5 * w;
            slope * t * t / (2.0 * w)
        } else {
            slope * over_db
        }
    }

    /// Apply the compressor in place to an interleaved buffer.
    ///
    /// `channels` is the stride; anything past the first two channels is
    /// left alone and fewer than two is returned untouched, as the fixed
    /// chain does — a third channel is not a thing to guess about.
    ///
    /// `key`, when present, is what the DETECTOR hears while the gain still
    /// lands on `buf`: interleaved stereo, stride 2, at least `frames`
    /// frames. A key too short to be that is ignored rather than trusted.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &CompParams,
        key: Option<&[f32]>,
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
        if mix < COMP_OFF {
            // Off is a wire AND a reset, once: yesterday's gain reduction
            // must not still be releasing when the mix comes back up.
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

        // Design math in f64, once per block; the audio path below stays f32.
        let sr = f64::from(self.sample_rate);
        let thr_db = f64::from(COMP_THRESHOLD_DB.0)
            + f64::from(COMP_THRESHOLD_DB.1 - COMP_THRESHOLD_DB.0)
                * f64::from(dial(params.threshold, 1.0));
        let ratio = f64::from(COMP_RATIO_MAX).powf(f64::from(dial(params.ratio, 0.0)));
        let slope = (1.0 / ratio - 1.0) as f32;
        // One-pole smoothing factors: 63% of the way in the dialled time,
        // which is what a time-constant dial has always meant.
        let smooth = |ms: f64| (1.0 - (-1.0 / (ms * 0.001 * sr)).exp()) as f32;
        let geo = |range: (f32, f32), t: f32| {
            f64::from(range.0) * f64::from(range.1 / range.0).powf(f64::from(t))
        };
        let a_att = smooth(geo(COMP_ATTACK_MS, dial(params.attack, 0.6)));
        let a_rel = smooth(geo(COMP_RELEASE_MS, dial(params.release, 0.5)));
        let a_win = smooth(f64::from(COMP_WINDOW_MS));
        let peak_fall = (-1.0 / (f64::from(COMP_WINDOW_MS) * 0.001 * sr)).exp() as f32;
        let makeup = dial(params.makeup, 0.5);
        // The bottom of the makeup travel is silence, not −24 dB: see
        // COMP_MAKEUP_DB.
        let muted = makeup < COMP_OFF;
        let makeup_db = (f64::from(COMP_MAKEUP_DB.0)
            + f64::from(COMP_MAKEUP_DB.1 - COMP_MAKEUP_DB.0) * f64::from(makeup))
            as f32;
        let hpf = dial(params.key_hpf, 0.0);
        let hpf_on = hpf >= COMP_OFF;
        let a_hp = if hpf_on {
            let hz = geo(COMP_KEY_HPF_HZ, hpf);
            (1.0 - (-std::f64::consts::TAU * hz / sr).exp()) as f32
        } else {
            0.0
        };
        let thr_db = thr_db as f32;

        let key = key.filter(|k| k.len() >= frames * 2);
        for (f, frame) in buf.chunks_exact_mut(channels).take(frames).enumerate() {
            let dry = [frame[0], frame[1]];
            let side = match key {
                Some(k) => [k[2 * f], k[2 * f + 1]],
                None => dry,
            };

            // The detector: high-pass, rectify with the channels LINKED,
            // then the RMS window and the peak envelope in parallel.
            let mut rect = 0.0_f32;
            for (ch, &x) in side.iter().enumerate() {
                let heard = if hpf_on {
                    self.key_lp[ch] = flush(self.key_lp[ch] + a_hp * (x - self.key_lp[ch]));
                    x - self.key_lp[ch]
                } else {
                    // Primed, so engaging the filter later starts from "no
                    // bass removed yet" rather than from a stale average.
                    self.key_lp[ch] = x;
                    x
                };
                rect = rect.max(heard.abs());
            }
            self.rms_sq = flush(self.rms_sq + a_win * (rect * rect - self.rms_sq));
            self.penv = flush(rect.max(self.penv * peak_fall));
            // The ×2 under the root is the sine calibration: a steady tone's
            // mean square is half its peak squared, so this reads amplitude.
            let level = COMP_PEAK_BLEND * self.penv
                + (1.0 - COMP_PEAK_BLEND) * (2.0 * self.rms_sq).sqrt();
            // −120 dB floor: silence reads as "far below any threshold"
            // rather than as a logarithm of zero.
            let level_db = 20.0 * level.max(1.0e-6).log10();

            // The gain computer and its ballistics, all in dB. More
            // reduction is the attack dial's business, less is the
            // release's — branching on the direction is what makes the two
            // dials independent.
            let target = Self::wanted_db(level_db - thr_db, slope);
            let a = if target < self.gr_db { a_att } else { a_rel };
            self.gr_db = flush(self.gr_db + a * (target - self.gr_db));
            self.hold_db = self.hold_db.max(-self.gr_db);

            let gain = if muted {
                0.0
            } else {
                10.0_f32.powf((self.gr_db + makeup_db) * 0.05)
            };
            // Written as dry + (wet − dry) · mix so that a gain of exactly
            // one is a bit-exact wire: the correction term is zero, not a
            // rounded product.
            frame[0] = dry[0] + (dry[0] * gain - dry[0]) * mix;
            frame[1] = dry[1] + (dry[1] * gain - dry[1]) * mix;
        }
    }

    /// How hard the compressor has been working since this was last called,
    /// in **decibels of reduction** — a positive number, 0.0 for none, before
    /// makeup. Max-hold, and resets on read, like the limiter's meter.
    pub fn gain_reduction_db(&mut self) -> f32 {
        std::mem::replace(&mut self.hold_db, 0.0)
    }

    /// Drop every trace of what was playing: detector, gain, meter. Called
    /// when the unit leaves the circuit, and on a stop.
    pub fn clear(&mut self) {
        self.key_lp = [0.0; 2];
        self.rms_sq = 0.0;
        self.penv = 0.0;
        self.gr_db = 0.0;
        self.hold_db = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// An interleaved stereo sine, both channels in phase.
    fn sine(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        let mut phase = 0.0_f32;
        for f in buf.chunks_exact_mut(2) {
            phase += hz * std::f32::consts::TAU / sr;
            let v = amp * phase.sin();
            f[0] = v;
            f[1] = v;
        }
        buf
    }

    /// The left channel's level from `from` frames on, in dB, with the same
    /// sine calibration the detector uses: RMS times √2 reads amplitude.
    fn level_db(buf: &[f32], from: usize) -> f32 {
        let left: Vec<f32> = buf.chunks_exact(2).skip(from).map(|f| f[0]).collect();
        let ms = left.iter().map(|v| v * v).sum::<f32>() / left.len() as f32;
        20.0 * (2.0 * ms).sqrt().max(1.0e-9).log10()
    }

    fn peak(v: &[f32]) -> f32 {
        v.iter().fold(0.0_f32, |m, x| m.max(x.abs()))
    }

    // The dials, inverted: what 0..=1 value lands the DSP on a wanted figure.
    fn thr_t(db: f32) -> f32 {
        (db - COMP_THRESHOLD_DB.0) / (COMP_THRESHOLD_DB.1 - COMP_THRESHOLD_DB.0)
    }
    fn ratio_t(ratio: f32) -> f32 {
        ratio.ln() / COMP_RATIO_MAX.ln()
    }
    fn attack_t(ms: f32) -> f32 {
        (ms / COMP_ATTACK_MS.0).ln() / (COMP_ATTACK_MS.1 / COMP_ATTACK_MS.0).ln()
    }
    fn release_t(ms: f32) -> f32 {
        (ms / COMP_RELEASE_MS.0).ln() / (COMP_RELEASE_MS.1 / COMP_RELEASE_MS.0).ln()
    }

    /// The analytic soft-knee output level, in dB, for a level in dB — the
    /// same arithmetic as `wanted_db`, in f64, written independently so the
    /// test is a second opinion rather than a mirror.
    fn soft_knee_out_db(x_db: f64, thr_db: f64, ratio: f64) -> f64 {
        let w = f64::from(COMP_KNEE_DB);
        let over = x_db - thr_db;
        let slope = 1.0 / ratio - 1.0;
        let gr = if 2.0 * over <= -w {
            0.0
        } else if 2.0 * over < w {
            let t = over + 0.5 * w;
            slope * t * t / (2.0 * w)
        } else {
            slope * over
        };
        x_db + gr
    }

    /// A working setup every test starts from: threshold −30 dB, 4 : 1.
    fn working() -> CompParams {
        CompParams {
            threshold: thr_t(-30.0),
            ratio: ratio_t(4.0),
            ..CompParams::default()
        }
    }

    /// Feed a constant level in 16-frame blocks and sample the smoothed gain
    /// reduction once per block. DC on purpose: a sine's rectified ripple
    /// would blur the very timing being measured.
    fn gr_trace(comp: &mut Comp, p: &CompParams, amp: f32, frames: usize) -> Vec<f32> {
        let mut trace = Vec::new();
        let mut done = 0;
        while done < frames {
            let n = 16.min(frames - done);
            let mut buf = vec![amp; n * 2];
            comp.process(&mut buf, n, 2, p, None);
            trace.push(comp.gr_db);
            done += n;
        }
        trace
    }

    /// **A threshold above everything, or a ratio of one, is a wire** — not
    /// nearly a wire: bit-exact, because "this effect is not doing anything"
    /// must survive a null test against the dry track.
    #[test]
    fn a_threshold_above_everything_or_a_ratio_of_one_is_a_wire() {
        let frames = 4_800;
        for p in [
            CompParams {
                threshold: 1.0,
                ..CompParams::default()
            },
            CompParams {
                ratio: 0.0,
                ..CompParams::default()
            },
        ] {
            let mut comp = Comp::new(SR);
            let mut buf = sine(1_000.0, 0.5, frames, SR);
            let before = buf.clone();
            comp.process(&mut buf, frames, 2, &p, None);
            assert_eq!(buf, before, "a compressor with nothing to do changed the signal");
        }
    }

    /// **The static curve lands where the arithmetic says.** A steady tone at
    /// three levels through −30 dB / 4 : 1 comes out within half a dB of the
    /// analytic soft-knee curve.
    #[test]
    fn the_static_curve_lands_on_the_soft_knee_arithmetic() {
        // Instant attack, so the settled gain sits at the curve rather than
        // somewhere on the way to it.
        let p = CompParams {
            attack: 0.0,
            ..working()
        };
        for in_db in [-6.0_f32, -12.0, -20.0] {
            let frames = SR as usize;
            let mut comp = Comp::new(SR);
            let mut buf = sine(1_000.0, 10.0_f32.powf(in_db / 20.0), frames, SR);
            comp.process(&mut buf, frames, 2, &p, None);
            let got = level_db(&buf, frames * 3 / 4);
            let want = soft_knee_out_db(f64::from(in_db), -30.0, 4.0) as f32;
            assert!(
                (got - want).abs() < 0.5,
                "{in_db} dB in came out at {got:.2} dB, the curve says {want:.2}"
            );
        }
    }

    /// **The attack and release dials tell the truth**: the time to 63% of
    /// the final gain reduction, on a level step, is the dialled number —
    /// within the slack the detector's own window adds.
    #[test]
    fn the_attack_and_release_dials_tell_the_truth() {
        let p = CompParams {
            attack: attack_t(50.0),
            release: release_t(300.0),
            ..working()
        };
        let quiet = 10.0_f32.powf(-30.0 / 20.0);
        let loud = 10.0_f32.powf(-10.0 / 20.0);
        let block_ms = 16.0 * 1000.0 / SR;
        let mut comp = Comp::new(SR);

        // A second at the threshold to settle, then the upward step.
        gr_trace(&mut comp, &p, quiet, SR as usize);
        let start = comp.gr_db;
        let up = gr_trace(&mut comp, &p, loud, SR as usize);
        let settled = *up.last().unwrap();
        let want = start + 0.63 * (settled - start);
        let hit = up.iter().position(|g| *g <= want).unwrap();
        let ms = (hit + 1) as f32 * block_ms;
        assert!(
            (35.0..=65.0).contains(&ms),
            "dialled 50 ms of attack, measured {ms:.1} ms"
        );

        // And back down.
        let start = comp.gr_db;
        let down = gr_trace(&mut comp, &p, quiet, 2 * SR as usize);
        let settled = *down.last().unwrap();
        let want = start + 0.63 * (settled - start);
        let hit = down.iter().position(|g| *g >= want).unwrap();
        let ms = (hit + 1) as f32 * block_ms;
        assert!(
            (210.0..=390.0).contains(&ms),
            "dialled 300 ms of release, measured {ms:.1} ms"
        );
    }

    /// **The key is what the detector hears.** A loud key ducks a main signal
    /// far too quiet to trip the threshold on its own.
    #[test]
    fn a_loud_key_ducks_a_main_signal_the_threshold_cannot_see() {
        let p = CompParams {
            ratio: 1.0,
            ..working()
        };
        let frames = SR as usize;
        let key = sine(1_000.0, 1.0, frames, SR);
        let quarter = frames * 3 / 4;

        let mut comp = Comp::new(SR);
        let mut ducked = sine(1_000.0, 0.01, frames, SR);
        comp.process(&mut ducked, frames, 2, &p, Some(&key));

        let mut comp = Comp::new(SR);
        let mut alone = sine(1_000.0, 0.01, frames, SR);
        comp.process(&mut alone, frames, 2, &p, None);

        let (ducked, alone) = (level_db(&ducked, quarter), level_db(&alone, quarter));
        assert!(
            ducked < alone - 10.0,
            "a 0 dB key only moved a −40 dB main from {alone:.1} to {ducked:.1} dB"
        );
    }

    /// **The key filter deafens the detector to bass.** With the high-pass at
    /// the top, a 60 Hz kick fundamental keys markedly less reduction than a
    /// 1 kHz tone of equal RMS.
    #[test]
    fn the_key_high_pass_makes_a_kick_key_less_than_a_tone() {
        let p = CompParams {
            ratio: 1.0,
            key_hpf: 1.0,
            ..working()
        };
        let frames = SR as usize;
        let meter_for = |hz: f32| {
            let mut comp = Comp::new(SR);
            let key = sine(hz, 1.0, frames, SR);
            let mut main = sine(1_000.0, 0.01, frames, SR);
            let settle = frames * 3 / 4;
            let tail = frames - settle;
            comp.process(&mut main[..settle * 2], settle, 2, &p, Some(&key));
            // Reset the hold so the figure below is the settled reduction,
            // not the onset transient.
            comp.gain_reduction_db();
            comp.process(&mut main[settle * 2..], tail, 2, &p, Some(&key[settle * 2..]));
            comp.gain_reduction_db()
        };
        let (kick, tone) = (meter_for(60.0), meter_for(1_000.0));
        assert!(
            tone > 6.0,
            "the 1 kHz key should compress hard through the filter, got {tone:.1} dB"
        );
        assert!(
            kick < tone - 6.0,
            "60 Hz keyed {kick:.1} dB against 1 kHz's {tone:.1} — the filter is not filtering"
        );
    }

    /// **Mix at zero is a wire whatever the other dials say**, including a
    /// key input and dials at their extremes.
    #[test]
    fn mix_at_zero_is_a_wire_whatever_the_other_dials_say() {
        let frames = 512;
        let key = sine(200.0, 1.0, frames, SR);
        let extremes = [0.0_f32, 1.0];
        for bits in 0..64_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = CompParams {
                threshold: d(0),
                ratio: d(1),
                attack: d(2),
                release: d(3),
                makeup: d(4),
                key_hpf: d(5),
                mix: 0.0,
            };
            let mut comp = Comp::new(SR);
            let mut buf = sine(1_000.0, 0.9, frames, SR);
            let before = buf.clone();
            comp.process(&mut buf, frames, 2, &p, Some(&key));
            assert_eq!(buf, before, "mix at zero touched the signal (dials {bits:06b})");
        }
    }

    /// **No setting the panel can reach may blow up.** Every dial at both
    /// extremes, a key handed in, against a signal well past full scale: the
    /// output stays finite and bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 4_800;
        let key = sine(1_000.0, 2.0, frames, SR);
        let extremes = [0.0_f32, 1.0];
        for bits in 0..128_u32 {
            let d = |bit: u32| extremes[(bits >> bit & 1) as usize];
            let p = CompParams {
                threshold: d(0),
                ratio: d(1),
                attack: d(2),
                release: d(3),
                makeup: d(4),
                mix: d(5),
                key_hpf: d(6),
            };
            let mut comp = Comp::new(SR);
            // Fresh input each pass, as the renderer hands it over.
            for _ in 0..2 {
                let mut buf = sine(55.0, 2.0, frames, SR);
                comp.process(&mut buf, frames, 2, &p, Some(&key));
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:07b} produced a non-finite sample"
                );
                // +24 dB of makeup on a double-full-scale input is 31.7;
                // the cap catches a runaway, not a hot setting.
                assert!(
                    peak(&buf) < 64.0,
                    "dials {bits:07b} reached {}",
                    peak(&buf)
                );
            }
        }
    }

    /// **The meter reports the reduction actually applied**: input level
    /// minus output level, makeup put back, within half a dB.
    #[test]
    fn the_meter_reports_the_reduction_actually_applied() {
        let makeup_db = 6.0_f32;
        let p = CompParams {
            attack: 0.0,
            makeup: (makeup_db - COMP_MAKEUP_DB.0) / (COMP_MAKEUP_DB.1 - COMP_MAKEUP_DB.0),
            ..working()
        };
        let frames = SR as usize + SR as usize / 2;
        let settle = SR as usize;
        let mut comp = Comp::new(SR);
        let mut buf = sine(1_000.0, 0.5, frames, SR);
        let in_db = level_db(&buf, settle);
        comp.process(&mut buf[..settle * 2], settle, 2, &p, None);
        comp.gain_reduction_db();
        let tail = frames - settle;
        comp.process(&mut buf[settle * 2..], tail, 2, &p, None);
        let meter = comp.gain_reduction_db();
        let measured = in_db - level_db(&buf, settle) + makeup_db;
        assert!(
            (meter - measured).abs() < 0.5,
            "the meter says {meter:.2} dB, the audio lost {measured:.2}"
        );
    }

    /// **The same squeeze at every sample rate.** Nothing here is a length
    /// table, so the settled output level through a working setup must agree
    /// at 44.1k, 48k and 96k.
    #[test]
    fn the_compressor_is_the_same_squeeze_at_every_sample_rate() {
        let out_at = |sr: f32| {
            let p = working();
            let frames = (sr * 1.5) as usize;
            let mut comp = Comp::new(sr);
            let mut buf = sine(1_000.0, 10.0_f32.powf(-12.0 / 20.0), frames, sr);
            comp.process(&mut buf, frames, 2, &p, None);
            level_db(&buf, frames * 2 / 3)
        };
        let (a, b, c) = (out_at(44_100.0), out_at(48_000.0), out_at(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            assert!(
                (x - y).abs() < 0.25,
                "levels across rates disagree: {a:.2} / {b:.2} / {c:.2} dB"
            );
        }
    }

    /// Mono and odd channel counts are left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut comp = Comp::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        comp.process(&mut buf, 256, 1, &working(), None);
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **Clear forgets the last session.** After a clear, no gain reduction,
    /// no meter reading and no detector level survive from before it.
    #[test]
    fn clear_forgets_what_was_playing() {
        let mut comp = Comp::new(SR);
        let frames = 4_800;
        let mut buf = sine(1_000.0, 1.0, frames, SR);
        comp.process(&mut buf, frames, 2, &working(), None);
        assert!(comp.gr_db < -5.0, "the setup never compressed: {}", comp.gr_db);
        comp.clear();
        assert_eq!(comp.gr_db, 0.0);
        assert_eq!(comp.rms_sq, 0.0);
        assert_eq!(comp.penv, 0.0);
        assert_eq!(comp.gain_reduction_db(), 0.0);
    }
}
