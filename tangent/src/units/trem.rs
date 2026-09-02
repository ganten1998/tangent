//! The opto tremolo kernel: a neon lamp flashing at a photoresistor, not a
//! sine wave multiplying a buffer.
//!
//! # Why not sine AM
//!
//! The classic amp tremolo circuit (Fender AA769/AB763 family — factual
//! reference, no affiliation) is a triode phase-shift oscillator feeding a
//! light-proof package holding a NEON lamp glued to an LDR. Two pieces of
//! physics stand between the oscillator's near-sine and the gain the guitar
//! hears, and they are the whole sound:
//!
//! 1. **The neon has a strike threshold.** A neon lamp conducts nothing at
//!    all until the voltage across it crosses breakdown, then snaps on; it
//!    holds on until the swing falls well below where it struck. So the
//!    lamp's light is a hard-edged pulse — lit for part of the cycle, dark
//!    for the rest — however clean the oscillator is.
//! 2. **The LDR is slow, and lopsided about it.** Cadmium-sulfide cells drop
//!    their resistance within milliseconds of light arriving and take tens
//!    of milliseconds to climb back in the dark. The hard-edged pulse comes
//!    out the other side as a rounded, asymmetric dip: a fast dive when the
//!    lamp strikes, a lazy recovery after it lets go. At high speeds the
//!    recovery never finishes, which is why hardware tremolo gets shallower
//!    as it gets faster — modelled here for free by the same time constants.
//!
//! The LDR then sits in a shunt divider across the signal: series source
//! impedance against (intensity resistance + LDR). Gain follows the divider
//! law, so INTENSITY is not a crossfade — turning it up moves the divider's
//! knee, which both deepens the dip and reshapes it. Deeper reaches further
//! down, exactly as the hardware does.
//!
//! The core lives in [`OptoTrem`] so the amp unit can embed the identical
//! circuit for its vibrato channel; [`Trem`] wraps it as a standalone unit
//! with the house kernel contract.
//!
//! # Sample rate
//!
//! Every coefficient derives from hertz or milliseconds at the top of each
//! block; nothing is a length table. The rate-invariance test holds the
//! wobble to the same speed and depth at 44.1k, 48k and 96k.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// Where the speed dial sweeps, in Hz, mapped geometrically. ~2.5 Hz is the
/// measured floor of the amp circuit's oscillator (its cathode cap sets it);
/// 10 Hz is the top builders report for the same board. Geometric because a
/// speed pot in that circuit is a reverse-audio taper: each degree of
/// rotation multiplies the rate by the same factor, which is also how ears
/// hear tempo.
pub const TREM_SPEED_HZ: (f32, f32) = (2.5, 10.0);

/// Where on the oscillator's swing (normalized −1..1) the neon strikes.
/// Above zero, so the lamp is lit slightly UNDER half of each cycle — the
/// dip is narrower than the recovery, which is the lopsided pulse everyone
/// recognises.
pub const NEON_STRIKE: f32 = 0.25;

/// Where the swing must fall before the struck lamp lets go. Lower than
/// [`NEON_STRIKE`] because a conducting neon holds on below its breakdown —
/// the hysteresis keeps the lamp from chattering near the threshold.
pub const NEON_EXTINGUISH: f32 = 0.05;

/// The LDR's time constant while the lamp is lit, in ms. Light frees
/// carriers fast: a few milliseconds to most of the resistance drop.
pub const LDR_LIT_MS: f32 = 6.0;

/// The LDR's time constant in the dark, in ms. Recombination is the slow
/// direction — an order of magnitude lazier than the strike — and this
/// asymmetry, not the LFO shape, is what rounds the pulse.
pub const LDR_DARK_MS: f32 = 55.0;

/// The LDR's resistance floor (fully lit) and ceiling (fully dark), in
/// ohms. A cadmium-sulfide roach spans decades; the model interpolates the
/// EXPONENT between these, because photoconductance moves geometrically.
pub const LDR_LIT_OHMS: f32 = 20.0e3;
pub const LDR_DARK_OHMS: f32 = 2.0e6;

/// The source impedance the shunt divider works against, in ohms — the
/// plate-side network feeding the tremolo node in the reference circuit.
pub const TREM_SOURCE_OHMS: f32 = 100.0e3;

/// What the intensity dial puts in series with the LDR, in ohms, geometric
/// from ceiling (barely reaches the signal) down to floor (LDR hard across
/// the node). This is the intensity pot: it does not scale the wobble, it
/// moves the divider's knee.
pub const TREM_INTENSITY_OHMS: (f32, f32) = (1.0e3, 5.0e6);

/// Below this a 0..=1 dial is at its resting position and means OFF — the
/// figure the fixed chain uses, duplicated so the kernel stands alone.
const TREM_OFF: f32 = 0.0005;

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

/// A dial is a number out of a settings file somebody may have edited:
/// clamp, and treat anything non-finite as the given resting place.
#[inline]
fn dial(v: f32, rest: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        rest
    }
}

// ── the core the amp embeds ─────────────────────────────────────────────────

/// The oscillator, the lamp and the photoresistor, per channel. See the
/// module header for the physics. The amp unit embeds one of these for its
/// vibrato channel; [`Trem`] wraps one as the standalone pedal.
pub struct OptoTrem {
    sample_rate: f32,
    /// LFO phase in cycles, 0..1. One accumulator for both channels; the
    /// stereo dial offsets the right channel's readout, not its clock.
    phase: f64,
    /// Whether each channel's neon is conducting — the hysteresis state.
    lit: [bool; 2],
    /// Each channel's LDR position, 0 = fully dark, 1 = fully lit. This is
    /// the exponent of the resistance interpolation, which makes the
    /// one-pole relaxation a straight line in log-resistance — the domain a
    /// photocell actually relaxes in.
    ldr: [f32; 2],
}

impl OptoTrem {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            phase: 0.0,
            lit: [false; 2],
            ldr: [0.0; 2],
        }
    }

    /// Apply the tremolo in place to an interleaved buffer. Dials are
    /// normalized 0..=1: `speed` across [`TREM_SPEED_HZ`], `depth` across
    /// [`TREM_INTENSITY_OHMS`] (0 is a bit-exact wire), `stereo` from mono
    /// (0) through a continuous phase offset to anti-phase L/R (1).
    ///
    /// `channels` is the stride; the first two channels are processed and
    /// fewer than two is returned untouched, as the house contract says.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        speed: f32,
        depth: f32,
        stereo: f32,
    ) {
        if channels < 2 {
            return;
        }
        let speed = dial(speed, 0.35);
        let depth = dial(depth, 0.0);
        let stereo = dial(stereo, 0.0);

        // Design math in f64, once per block; the audio path stays f32.
        let sr = f64::from(self.sample_rate);
        let hz = f64::from(TREM_SPEED_HZ.0)
            * f64::from(TREM_SPEED_HZ.1 / TREM_SPEED_HZ.0).powf(f64::from(speed));
        let dphase = hz / sr;

        // Depth at rest is a WIRE — the intensity pot at zero takes the LDR
        // out of the divider entirely. The oscillator keeps running (it does
        // in the amp too; the intensity pot does not stop the lamp), so
        // easing the depth back in picks up mid-wobble, not from a reset.
        if depth < TREM_OFF {
            self.phase = (self.phase + dphase * frames as f64).fract();
            // The lamp still flashes on the idle LDRs: relax them toward
            // dark so re-engaging starts from the hardware's resting state.
            let a_dark = 1.0 - (-1.0 / (f64::from(LDR_DARK_MS) * 0.001 * sr)).exp();
            let fall = (1.0 - a_dark).powi(frames as i32) as f32;
            self.ldr = [flush(self.ldr[0] * fall), flush(self.ldr[1] * fall)];
            return;
        }

        // The LDR's two one-pole coefficients — the asymmetry itself.
        let a_lit = (1.0 - (-1.0 / (f64::from(LDR_LIT_MS) * 0.001 * sr)).exp()) as f32;
        let a_dark = (1.0 - (-1.0 / (f64::from(LDR_DARK_MS) * 0.001 * sr)).exp()) as f32;

        // The divider's fixed legs for this block. Intensity maps
        // geometrically: the knee of a shunt divider moves in ratios.
        let (r_floor, r_ceil) = (
            f64::from(TREM_INTENSITY_OHMS.0),
            f64::from(TREM_INTENSITY_OHMS.1),
        );
        let r_int = r_floor * (r_ceil / r_floor).powf(f64::from(1.0 - depth));
        let r_src = f64::from(TREM_SOURCE_OHMS);
        // Normalise so a fully-dark LDR is unity gain: the envelope's TOP
        // sits at 1 and intensity reads as depth, not as a volume dial.
        // (The real amp does lose a fraction of a dB of standing level
        // through the divider; the shape is what matters here.)
        let g_dark = (r_int + f64::from(LDR_DARK_OHMS)) / (r_src + r_int + f64::from(LDR_DARK_OHMS));
        let ln_lit = f64::from(LDR_LIT_OHMS).ln() as f32;
        let ln_dark = f64::from(LDR_DARK_OHMS).ln() as f32;
        let (r_int, r_src, inv_g_dark) = (r_int as f32, r_src as f32, (1.0 / g_dark) as f32);

        // The right channel reads the same oscillator half a cycle later at
        // full stereo — a continuous phase offset 0..π across the dial.
        let offset = [0.0_f64, f64::from(stereo) * 0.5];

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            self.phase += dphase;
            if self.phase >= 1.0 {
                self.phase -= 1.0;
            }
            for (ch, sample) in frame.iter_mut().take(2).enumerate() {
                let swing = (std::f64::consts::TAU * (self.phase + offset[ch])).sin() as f32;
                // The neon: strike above breakdown, hold until the swing
                // falls well below it. No intermediate brightness exists.
                if !self.lit[ch] && swing > NEON_STRIKE {
                    self.lit[ch] = true;
                } else if self.lit[ch] && swing < NEON_EXTINGUISH {
                    self.lit[ch] = false;
                }
                // The LDR drags behind the lamp, fast toward lit and slow
                // toward dark.
                let (target, a) = if self.lit[ch] { (1.0, a_lit) } else { (0.0, a_dark) };
                let b = flush(self.ldr[ch] + a * (target - self.ldr[ch]));
                self.ldr[ch] = b;
                // The shunt-divider law: geometric resistance under the
                // exponent, then the plain resistive divide.
                let r_ldr = (ln_dark + (ln_lit - ln_dark) * b).exp();
                let shunt = r_int + r_ldr;
                let gain = shunt / (r_src + shunt) * inv_g_dark;
                *sample *= gain;
            }
        }
    }

    /// Drop the lamp, the cell and the phase back to power-on.
    pub fn clear(&mut self) {
        self.phase = 0.0;
        self.lit = [false; 2];
        self.ldr = [0.0; 2];
    }
}

// ── the standalone unit ─────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TremParams {
    /// Across [`TREM_SPEED_HZ`], geometric.
    pub speed: f32,
    /// Across [`TREM_INTENSITY_OHMS`], reversed and geometric; 0 is a wire.
    pub depth: f32,
    /// 0 mono, 1 anti-phase L/R, continuous phase offset between.
    pub stereo: f32,
}

/// The shipped sound: about 4 Hz, a medium throb, mono — the setting the
/// reference amp spends its life on.
impl Default for TremParams {
    fn default() -> Self {
        Self {
            speed: 0.35,
            depth: 0.6,
            stereo: 0.0,
        }
    }
}

/// The standalone tremolo unit: [`OptoTrem`] under the house kernel
/// contract, nothing more — the pedal IS the amp circuit in a box.
pub struct Trem {
    opto: OptoTrem,
}

impl Trem {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            opto: OptoTrem::new(sample_rate),
        }
    }

    /// Apply the tremolo in place; see [`OptoTrem::process`] for the
    /// contract.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, params: &TremParams) {
        self.opto
            .process(buf, frames, channels, params.speed, params.depth, params.stereo);
    }

    pub fn clear(&mut self) {
        self.opto.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// A block of DC ones: through a pure gain effect, the output IS the
    /// gain envelope, which is the thing every test here wants to read.
    fn envelope(sr: f32, seconds: f32, p: &TremParams) -> Vec<f32> {
        let frames = (sr * seconds) as usize;
        let mut trem = Trem::new(sr);
        let mut buf = vec![1.0_f32; frames * 2];
        trem.process(&mut buf, frames, 2, p);
        buf
    }

    /// One channel of an interleaved buffer, decimated by plain averaging —
    /// the envelope is band-limited to a few tens of hertz, so a coarse
    /// grid loses nothing and the autocorrelation below stops costing a
    /// billion multiplies.
    fn channel_decimated(buf: &[f32], ch: usize, decim: usize) -> Vec<f32> {
        buf.chunks_exact(2)
            .map(|f| f[ch])
            .collect::<Vec<f32>>()
            .chunks(decim)
            .map(|c| c.iter().sum::<f32>() / c.len() as f32)
            .collect()
    }

    /// The envelope's period via autocorrelation: the lag in 0.7..1.3 of
    /// the expected period with the highest mean-removed self-product.
    fn measured_hz(env: &[f32], env_rate: f32, expect_hz: f32) -> f32 {
        let mean = env.iter().sum::<f32>() / env.len() as f32;
        let e: Vec<f32> = env.iter().map(|v| v - mean).collect();
        let period = env_rate / expect_hz;
        let (lo, hi) = ((period * 0.7) as usize, (period * 1.3) as usize);
        let mut best = (0.0_f32, lo);
        for lag in lo..=hi {
            let r: f32 = e[..e.len() - lag]
                .iter()
                .zip(&e[lag..])
                .map(|(a, b)| a * b)
                .sum();
            if r > best.0 {
                best = (r, lag);
            }
        }
        env_rate / best.1 as f32
    }

    /// **The speed dial tells the truth**: the envelope's autocorrelation
    /// period lands within 5% of the mapped rate, at the middle and the top
    /// of the dial.
    #[test]
    fn the_speed_dial_lands_within_five_percent_by_autocorrelation() {
        for (speed, want_hz) in [(0.5_f32, 5.0_f32), (1.0, 10.0)] {
            let p = TremParams {
                speed,
                depth: 1.0,
                stereo: 0.0,
            };
            let buf = envelope(SR, 5.0, &p);
            // Skip the first second: the LDR is still finding its swing.
            let env = channel_decimated(&buf[(SR as usize) * 2..], 0, 32);
            let got = measured_hz(&env, SR / 32.0, want_hz);
            assert!(
                (got - want_hz).abs() / want_hz < 0.05,
                "dialled {want_hz} Hz, measured {got:.2}"
            );
        }
    }

    /// **The pulse is lopsided the way the hardware is lopsided**: the dive
    /// (lamp strikes, resistance collapses in milliseconds) is measurably
    /// faster than the recovery (dark, the cell crawls back), by at least
    /// a factor of two between the envelope's 10% and 90% lines.
    #[test]
    fn the_ldr_dives_fast_and_recovers_slow() {
        let p = TremParams {
            speed: 0.0, // 2.5 Hz: the cycle is long enough to settle both ways
            depth: 1.0,
            stereo: 0.0,
        };
        let buf = envelope(SR, 4.0, &p);
        let env: Vec<f32> = buf.chunks_exact(2).skip(SR as usize).map(|f| f[0]).collect();
        let (min, max) = env
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let (lo, hi) = (min + 0.1 * (max - min), max - 0.1 * (max - min));

        // Walk the envelope collecting hi→lo (fall) and lo→hi (rise) times.
        let (mut falls, mut rises) = (Vec::new(), Vec::new());
        let (mut fall_at, mut rise_at) = (None, None);
        let mut prev = env[0];
        for (i, &v) in env.iter().enumerate() {
            if prev >= hi && v < hi {
                fall_at = Some(i);
            }
            if prev >= lo && v < lo {
                if let Some(t) = fall_at.take() {
                    falls.push(i - t);
                }
                rise_at = None;
            }
            if prev <= lo && v > lo {
                rise_at = Some(i);
            }
            if prev <= hi && v > hi {
                if let Some(t) = rise_at.take() {
                    rises.push(i - t);
                }
                fall_at = None;
            }
            prev = v;
        }
        assert!(falls.len() >= 3 && rises.len() >= 3, "too few cycles to measure");
        let fall = falls.iter().sum::<usize>() as f32 / falls.len() as f32;
        let rise = rises.iter().sum::<usize>() as f32 / rises.len() as f32;
        assert!(
            rise > 2.0 * fall,
            "rise {:.1} ms vs fall {:.1} ms — the asymmetry is missing",
            rise * 1000.0 / SR,
            fall * 1000.0 / SR
        );
    }

    /// **Deeper reaches further down.** Intensity moves the divider's knee,
    /// so full depth dips at least 6 dB below what a mid dial reaches — it
    /// is not the same wobble scaled.
    #[test]
    fn deeper_intensity_reaches_further_down() {
        let floor_at = |depth: f32| {
            let p = TremParams {
                speed: 0.0,
                depth,
                stereo: 0.0,
            };
            let buf = envelope(SR, 3.0, &p);
            buf.chunks_exact(2)
                .skip(SR as usize)
                .map(|f| f[0])
                .fold(f32::MAX, f32::min)
        };
        let (mid, full) = (floor_at(0.4), floor_at(1.0));
        assert!(
            20.0 * (mid / full).log10() >= 6.0,
            "depth 0.4 dips to {mid:.3}, depth 1.0 to {full:.3} — the knee is not moving"
        );
    }

    /// **Full stereo is anti-phase**: with the dial at 1 the two channels'
    /// envelopes are strongly anti-correlated; at 0 they are the same
    /// samples to the bit.
    #[test]
    fn full_stereo_puts_the_channels_in_opposite_phase() {
        let p = TremParams {
            speed: 0.5,
            depth: 1.0,
            stereo: 1.0,
        };
        let buf = envelope(SR, 3.0, &p);
        let skip = (SR as usize) * 2;
        let l = channel_decimated(&buf[skip..], 0, 32);
        let r = channel_decimated(&buf[skip..], 1, 32);
        let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len() as f32;
        let (ml, mr) = (mean(&l), mean(&r));
        let (mut num, mut dl, mut dr) = (0.0_f32, 0.0_f32, 0.0_f32);
        for (a, b) in l.iter().zip(&r) {
            num += (a - ml) * (b - mr);
            dl += (a - ml) * (a - ml);
            dr += (b - mr) * (b - mr);
        }
        let corr = num / (dl * dr).sqrt();
        assert!(
            corr < -0.5,
            "anti-phase envelopes correlate at {corr:.2}, wanted strongly negative"
        );

        let mono = envelope(
            SR,
            1.0,
            &TremParams {
                stereo: 0.0,
                ..p
            },
        );
        for f in mono.chunks_exact(2) {
            assert_eq!(f[0], f[1], "stereo at 0 split the channels");
        }
    }

    /// **Depth at rest is a wire** — bit-exact, whatever the other dials
    /// say, because the intensity pot at zero takes the LDR out of the
    /// circuit entirely.
    #[test]
    fn zero_depth_is_a_wire_whatever_the_other_dials_say() {
        let frames = 4_800;
        for bits in 0..4_u32 {
            let d = |bit: u32| [0.0_f32, 1.0][(bits >> bit & 1) as usize];
            let p = TremParams {
                speed: d(0),
                depth: 0.0,
                stereo: d(1),
            };
            let mut trem = Trem::new(SR);
            let mut buf: Vec<f32> = (0..frames * 2)
                .map(|i| (i as f32 * 0.013).sin() * 0.9)
                .collect();
            let before = buf.clone();
            trem.process(&mut buf, frames, 2, &p);
            assert_eq!(buf, before, "zero depth touched the signal (dials {bits:02b})");
        }
    }

    /// **No setting the panel can reach may blow up**: a tremolo is a gain
    /// between 0 and 1, so a hot input comes back finite and no louder.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 9_600;
        for bits in 0..8_u32 {
            let d = |bit: u32| [0.0_f32, 1.0][(bits >> bit & 1) as usize];
            let p = TremParams {
                speed: d(0),
                depth: d(1),
                stereo: d(2),
            };
            let mut trem = Trem::new(SR);
            for _ in 0..2 {
                let mut buf: Vec<f32> = (0..frames * 2)
                    .map(|i| (i as f32 * 0.007).sin() * 2.0)
                    .collect();
                trem.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:03b} produced a non-finite sample"
                );
                let peak = buf.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(peak <= 2.001, "dials {bits:03b} reached {peak}");
            }
        }
    }

    /// A mono buffer is left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut trem = Trem::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        trem.process(&mut buf, 256, 1, &TremParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **The same wobble at every sample rate**: rate within 5% of the map
    /// and the dip within a dB, at 44.1k, 48k and 96k — nothing in here is
    /// a length table.
    #[test]
    fn the_same_wobble_at_every_sample_rate() {
        let p = TremParams {
            speed: 0.5,
            depth: 1.0,
            stereo: 0.0,
        };
        for sr in [44_100.0_f32, 48_000.0, 96_000.0] {
            let buf = envelope(sr, 4.0, &p);
            let tail = &buf[(sr as usize) * 2..];
            let decim = (sr / 1_500.0) as usize;
            let env = channel_decimated(tail, 0, decim);
            let got = measured_hz(&env, sr / decim as f32, 5.0);
            assert!(
                (got - 5.0).abs() / 5.0 < 0.05,
                "at {sr} Hz the rate came out {got:.2} instead of 5"
            );
            let floor = tail.chunks_exact(2).map(|f| f[0]).fold(f32::MAX, f32::min);
            let want = {
                let buf48 = envelope(48_000.0, 4.0, &p);
                buf48
                    .chunks_exact(2)
                    .skip(48_000)
                    .map(|f| f[0])
                    .fold(f32::MAX, f32::min)
            };
            assert!(
                (20.0 * (floor / want).log10()).abs() < 1.0,
                "at {sr} Hz the dip reaches {floor:.3} vs 48k's {want:.3}"
            );
        }
    }

    /// **Clear forgets the wobble**: phase, lamp and cell all return to
    /// power-on, so the next note starts at the top of the envelope.
    #[test]
    fn clear_forgets_the_wobble() {
        let p = TremParams {
            depth: 1.0,
            ..TremParams::default()
        };
        let mut trem = Trem::new(SR);
        let mut buf = vec![1.0_f32; 48_000 * 2];
        trem.process(&mut buf, 48_000, 2, &p);
        assert!(trem.opto.ldr[0] > 0.0, "a second of full depth never lit the lamp");
        trem.clear();
        assert_eq!(trem.opto.phase, 0.0);
        assert_eq!(trem.opto.ldr, [0.0; 2]);
        assert_eq!(trem.opto.lit, [false; 2]);
    }
}
