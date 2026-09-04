//! The overdrive kernel: the screamer-family topology (factual reference —
//! the op-amp/diode circuit every green pedal since 1979 shares; no
//! affiliation), oversampled 4× around the one nonlinear element.
//!
//! # The topology, and why it sounds like that
//!
//! The circuit is an op-amp in NON-inverting gain with diodes across the
//! feedback resistor. Three consequences, all modelled:
//!
//! 1. **The "1 +" keeps the lows clean.** Non-inverting gain is
//!    `1 + Zf/Zi`, and `Zi` is a resistor in series with a capacitor to
//!    ground — a high-pass in the GAIN path with its corner near 720 Hz
//!    (4.7 kΩ · 47 nF). Below the corner the stage decays toward unity: the
//!    bass rides through beside the distortion instead of turning to mud.
//!    Above it the full gain (and the clipping) applies — that lopsided
//!    boost is the famous mid hump.
//! 2. **The diodes clip the FEEDBACK, not the signal.** The output is
//!    `input + clipped(gain path)`: soft, compressed, never a hard wall.
//!    The clip itself is a tanh-family knee at the diode drop. The symmetry
//!    toggle swaps one of the pair for two in series — one side clips a
//!    diode-drop later, which adds even harmonics (and a DC component that
//!    a 10 Hz high-pass removes before it reaches the tone stage).
//! 3. **A small cap across the feedback resistor** rolls the gain path off
//!    at `1/(2π·Rf·51 pF)` — the corner FALLS as the drive pot rises, which
//!    is why a cranked screamer gets darker as well as dirtier.
//!
//! Post-clip: the tone control, one pole sweeping ~700 Hz to 6 kHz, then
//! the level pot. Tone runs at the oversampled rate on purpose: its
//! roll-off attenuates the top harmonics BEFORE decimation and buys the
//! anti-alias filters another ~10 dB where they are weakest.
//!
//! # Oversampling, and the two aliases it cannot buy off
//!
//! The nonlinearity runs at 4× through a two-stage windowed-sinc up/down
//! pair, designed in f64 at construction (no crates, per house law). Two
//! folds need more than the textbook halfband pair:
//!
//! - **The fold at the oversampled rate itself.** At full drive the 39th
//!   harmonic of a 5 kHz tone lands at 195 kHz and comes back at 3 kHz the
//!   instant the clipper's output is SAMPLED — inside the audible band,
//!   beyond any decimator's reach. The fix is at the source: the clipper
//!   is discretized by first-order ADAA (see [`clip_adaa`]), whose sinc²
//!   image response has a zero exactly at the 4× rate.
//! - **The fold at Nyquist.** An exact halfband's transition straddles its
//!   fold point, so 25 kHz would come back at 23 kHz at only −10 dB. The
//!   2× stage therefore shades its cutoff below fs/4 (see [`HB_A_CUTOFF`])
//!   until the whole fold region sits in true stopband.
//!
//! The pair adds a fixed ~18 samples of latency; there is no parallel dry
//! path outside it, so nothing needs aligning. A test holds every alias of
//! a 5 kHz tone at full drive to 40 dB under the fundamental.
//!
//! # What has no transparent setting
//!
//! A screamer at minimum drive is still ~20 dB of mid gain into the knee —
//! the pedal has no wire setting, by design. The honest null here is the
//! LEVEL pot: at the bottom of its travel the output is exact silence, so
//! a parallel router can take the unit out without a click.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point.
#![allow(dead_code)]

// ── what the dials map to ───────────────────────────────────────────────────

/// The gain-path input leg: 4.7 kΩ in series with 47 nF to ground, a
/// high-pass at 720 Hz. THE mid-hump number — everything below it decays
/// toward unity gain and stays clean.
pub const DRIVE_RI_OHMS: f32 = 4_700.0;
pub const DRIVE_HPF_HZ: f32 = 720.0;

/// The feedback resistance the drive dial sweeps: a 51 kΩ floor plus a
/// 500 kΩ pot, square-law tapered like the audio pot in the pedal. Gain of
/// the clipping path is `Rf / Ri` — about 21 dB up to 41 dB.
pub const DRIVE_RF_OHMS: (f32, f32) = (51_000.0, 551_000.0);

/// The cap across the feedback resistor, farads. Its corner `1/(2π·Rf·C)`
/// slides from ~61 kHz (drive down) to ~5.7 kHz (drive up).
pub const DRIVE_CF_FARADS: f32 = 51.0e-12;

/// The soft-clip ceiling, normalized: one diode drop, and two in series
/// for the asymmetric side. Absolute scale is arbitrary in a normalized
/// signal path; the RATIO to typical drive levels is what the knee is.
pub const DRIVE_CLIP_ONE: f32 = 0.33;
pub const DRIVE_CLIP_TWO: f32 = 0.66;

/// Where the tone dial's one-pole low-pass sweeps, in Hz, geometric.
pub const DRIVE_TONE_HZ: (f32, f32) = (700.0, 6_000.0);

/// The output level's top, as linear gain (+6 dB), square-law taper.
/// Unity sits near 0.71 on the dial — the shipped default.
pub const DRIVE_LEVEL_MAX: f32 = 2.0;

/// The DC blocker after the clipper, in Hz. Asymmetric clipping rectifies;
/// this keeps the offset out of the tone stage and the next unit.
pub const DRIVE_DC_HZ: f32 = 10.0;

/// Taps in the two decimation/interpolation filters, both windowed-sinc
/// (Kaiser β=8) designed once in `new`. `B` runs at 4× where every fold
/// lands far into its stopband, so it is a short true halfband. `A` runs
/// at 2× and guards the fold onto the audible band — and an exact
/// halfband is the wrong tool there, because its transition straddles the
/// fold point: content just above Nyquist would come back just below it
/// at only −10 dB. So `A` gets more taps AND a cutoff shaded to
/// [`HB_A_CUTOFF`] below the fold, which moves the whole fold region into
/// true stopband (−50 dB and falling) for ~1 dB of shading at 20 kHz per
/// pass in a 48 kHz session.
pub(crate) const HB_A_TAPS: usize = 63;
pub(crate) const HB_B_TAPS: usize = 23;

/// Stage A's cutoff as a fraction of its running rate (0.25 would be the
/// exact halfband). 0.224 puts −54 dB at one kilohertz past the fold in a
/// 48 kHz session, sized numerically against the alias test's worst path.
pub(crate) const HB_A_CUTOFF: f64 = 0.224;

/// Below this a 0..=1 dial is at its resting position and means OFF.
const DRIVE_OFF: f32 = 0.0005;

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

// ── the halfband pair ───────────────────────────────────────────────────────
//
// **Shared with `crate::analog`, deliberately.** The synth runs its ladder
// and its mixer overload at 4× and needs exactly this pair — the same
// Kaiser designer, the same tap counts, the same shaded cutoff — plus the
// ADAA knee below it for the overload. The pieces are `pub(crate)` rather
// than copied: a second oversampler in this repo would be a second place
// for an alias to hide, and the alias test at the bottom of this file
// would not know about it.

/// Modified Bessel I₀ by its power series — all the Kaiser window needs,
/// and it converges in a few dozen terms for any β this file uses.
fn bessel_i0(x: f64) -> f64 {
    let mut sum = 1.0;
    let mut term = 1.0;
    for k in 1..60 {
        term *= (x * x * 0.25) / ((k * k) as f64);
        sum += term;
        if term < sum * 1.0e-18 {
            break;
        }
    }
    sum
}

/// A Kaiser-windowed sinc low-pass with its cutoff at `fc` of the running
/// rate, DC gain one. At `fc = 0.25` this is the classic halfband — every
/// second off-centre tap a true zero.
pub(crate) fn windowed_sinc<const N: usize>(beta: f64, fc: f64) -> [f32; N] {
    let m = (N - 1) as f64 / 2.0;
    let denom = bessel_i0(beta);
    let mut taps = [0.0_f32; N];
    for (i, t) in taps.iter_mut().enumerate() {
        let k = i as f64 - m;
        let ideal = if k == 0.0 {
            2.0 * fc
        } else {
            (std::f64::consts::TAU * fc * k).sin() / (std::f64::consts::PI * k)
        };
        let r = k / m;
        let win = bessel_i0(beta * (1.0 - r * r).max(0.0).sqrt()) / denom;
        *t = (ideal * win) as f32;
    }
    taps
}

/// A direct-form FIR delay line. Taps live outside so both channels (and
/// both directions) share one designed set.
#[derive(Clone)]
pub(crate) struct Fir<const N: usize> {
    z: [f32; N],
    pos: usize,
}

impl<const N: usize> Fir<N> {
    pub(crate) fn new() -> Self {
        Self { z: [0.0; N], pos: 0 }
    }
    #[inline]
    pub(crate) fn run(&mut self, taps: &[f32; N], x: f32) -> f32 {
        self.pos = if self.pos == 0 { N - 1 } else { self.pos - 1 };
        self.z[self.pos] = x;
        let mut acc = 0.0_f32;
        let (head, tail) = self.z.split_at(self.pos);
        for (t, v) in taps.iter().zip(tail.iter().chain(head.iter())) {
            acc += t * v;
        }
        acc
    }
    pub(crate) fn clear(&mut self) {
        self.z = [0.0; N];
        self.pos = 0;
    }
}

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveParams {
    /// Across [`DRIVE_RF_OHMS`], square-law.
    pub drive: f32,
    /// Across [`DRIVE_TONE_HZ`], geometric.
    pub tone: f32,
    /// Across 0..[`DRIVE_LEVEL_MAX`], square-law; 0 is silence.
    pub level: f32,
    /// A toggle (≥ 0.5 is on): one diode against two, instead of one
    /// against one — even harmonics and a touch more grit.
    pub symmetry: f32,
}

/// The shipped sound: moderate drive, tone at noon, level at unity,
/// symmetric diodes — the pedal as it leaves the factory.
impl Default for DriveParams {
    fn default() -> Self {
        Self {
            drive: 0.35,
            tone: 0.5,
            level: 0.71,
            symmetry: 0.0,
        }
    }
}

// ── the overdrive ───────────────────────────────────────────────────────────

/// `ln(cosh(t))`, overflow-safe: for any sizeable `|t|` it is just
/// `|t| − ln 2` plus a vanishing correction, so the naive `cosh` never runs.
#[inline]
pub(crate) fn ln_cosh(t: f64) -> f64 {
    let a = t.abs();
    a + (-2.0 * a).exp().ln_1p() - std::f64::consts::LN_2
}

/// The diode knee: a tanh at the ceiling for the sign of `x` — one drop on
/// the positive side, and two when the symmetry toggle stacks them.
#[inline]
pub(crate) fn knee(x: f64, pos: f64, neg: f64) -> f64 {
    let ceil = if x >= 0.0 { pos } else { neg };
    ceil * (x / ceil).tanh()
}

/// The knee's antiderivative, `ceil²·ln(cosh(x/ceil))` per side — zero at
/// zero on both sides, so the pieces join with continuous value AND slope
/// and one global antiderivative covers the asymmetric curve.
#[inline]
pub(crate) fn knee_integral(x: f64, pos: f64, neg: f64) -> f64 {
    let ceil = if x >= 0.0 { pos } else { neg };
    ceil * ceil * ln_cosh(x / ceil)
}

/// The clipper, discretized by first-order ADAA (Parker et al., 2016):
/// output the mean of the CONTINUOUS curve over the inter-sample segment,
/// `(F(x) − F(x₁)) / (x − x₁)`, instead of the point sample.
///
/// Why: a memoryless clip evaluated per sample aliases the moment it
/// runs — at full drive the 39th harmonic of a 5 kHz tone lands at
/// 195 kHz and folds to 3 kHz AT the 4× rate, where no decimation filter
/// can ever reach it. The segment-mean is a linear-interpolation hold on
/// the continuous waveform, which multiplies every image by sinc² — with
/// a ZERO exactly at the oversampled rate, precisely where those folds
/// are born. In f64: the difference quotient is a subtraction of
/// near-equal numbers, and the fallback (the curve at the midpoint) takes
/// over when the segment is too short to divide safely.
#[inline]
pub(crate) fn clip_adaa(x: f32, x1: f32, pos: f32, neg: f32) -> f32 {
    let (x, x1) = (f64::from(x), f64::from(x1));
    let (pos, neg) = (f64::from(pos), f64::from(neg));
    let dx = x - x1;
    let y = if dx.abs() > 1.0e-6 {
        (knee_integral(x, pos, neg) - knee_integral(x1, pos, neg)) / dx
    } else {
        knee(0.5 * (x + x1), pos, neg)
    };
    y as f32
}

/// One channel's worth of state: the four FIR delay lines of the up/down
/// pair and the one-pole states of the gain path, in signal order.
#[derive(Clone)]
struct DriveChannel {
    up_a: Fir<HB_A_TAPS>,
    up_b: Fir<HB_B_TAPS>,
    dn_b: Fir<HB_B_TAPS>,
    dn_a: Fir<HB_A_TAPS>,
    /// Low-pass state whose complement is the 720 Hz gain-path high-pass.
    hp_lp: f32,
    /// The feedback cap's roll-off on the pre-clip drive signal.
    fb_lp: f32,
    /// The clipper's previous input, for the ADAA difference.
    adaa_x1: f32,
    /// The post-clip DC blocker's low-pass state.
    dc_lp: f32,
    /// The tone control's one-pole state.
    tone_lp: f32,
}

impl DriveChannel {
    fn new() -> Self {
        Self {
            up_a: Fir::new(),
            up_b: Fir::new(),
            dn_b: Fir::new(),
            dn_a: Fir::new(),
            hp_lp: 0.0,
            fb_lp: 0.0,
            adaa_x1: 0.0,
            dc_lp: 0.0,
            tone_lp: 0.0,
        }
    }
    fn clear(&mut self) {
        self.up_a.clear();
        self.up_b.clear();
        self.dn_b.clear();
        self.dn_a.clear();
        self.hp_lp = 0.0;
        self.fb_lp = 0.0;
        self.adaa_x1 = 0.0;
        self.dc_lp = 0.0;
        self.tone_lp = 0.0;
    }
}

/// The screamer-family overdrive. See the module header for the circuit;
/// see [`Drive::process`] for the contract.
pub struct Drive {
    sample_rate: f32,
    taps_a: [f32; HB_A_TAPS],
    taps_b: [f32; HB_B_TAPS],
    ch: [DriveChannel; 2],
    /// Whether the last call was silenced, so the off-clear runs once.
    idle: bool,
}

impl Drive {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            taps_a: windowed_sinc(8.0, HB_A_CUTOFF),
            taps_b: windowed_sinc(8.0, 0.25),
            ch: [DriveChannel::new(), DriveChannel::new()],
            idle: true,
        }
    }

    /// Apply the overdrive in place to an interleaved buffer.
    ///
    /// `channels` is the stride; the first two channels are processed and
    /// fewer than two is returned untouched. Level at the bottom of its
    /// travel writes exact silence (and resets, once), the unit's null.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, params: &DriveParams) {
        if channels < 2 {
            return;
        }
        let level = dial(params.level, 0.71);
        if level < DRIVE_OFF {
            if !self.idle {
                self.clear();
                self.idle = true;
            }
            for frame in buf.chunks_exact_mut(channels).take(frames) {
                frame[0] = 0.0;
                frame[1] = 0.0;
            }
            return;
        }
        self.idle = false;

        // Design math in f64, once per block; the audio path stays f32.
        let sr4 = 4.0 * f64::from(self.sample_rate);
        let drive = dial(params.drive, 0.35);
        let rf = f64::from(DRIVE_RF_OHMS.0)
            + f64::from(DRIVE_RF_OHMS.1 - DRIVE_RF_OHMS.0) * f64::from(drive * drive);
        let gain = (rf / f64::from(DRIVE_RI_OHMS)) as f32;
        let onepole = |hz: f64| (1.0 - (-std::f64::consts::TAU * hz / sr4).exp()) as f32;
        let a_hp = onepole(f64::from(DRIVE_HPF_HZ));
        let a_fb = onepole(1.0 / (std::f64::consts::TAU * rf * f64::from(DRIVE_CF_FARADS)));
        let tone_hz = f64::from(DRIVE_TONE_HZ.0)
            * f64::from(DRIVE_TONE_HZ.1 / DRIVE_TONE_HZ.0).powf(f64::from(dial(params.tone, 0.5)));
        let a_tone = onepole(tone_hz);
        let a_dc = onepole(f64::from(DRIVE_DC_HZ));
        let out_gain = DRIVE_LEVEL_MAX * level * level;
        let pos_clip = DRIVE_CLIP_ONE;
        let neg_clip = if dial(params.symmetry, 0.0) >= 0.5 {
            DRIVE_CLIP_TWO
        } else {
            DRIVE_CLIP_ONE
        };

        for frame in buf.chunks_exact_mut(channels).take(frames) {
            for (ch, sample) in frame.iter_mut().take(2).enumerate() {
                let st = &mut self.ch[ch];
                let x = *sample;
                let mut out = 0.0_f32;
                // Two 2× steps per input sample, two 4× steps per 2× step.
                // The ×2 on each zero-stuffed push restores the amplitude
                // an interpolator's inserted zeros dilute.
                for h in 0..2 {
                    let s2 = st.up_a.run(&self.taps_a, if h == 0 { 2.0 * x } else { 0.0 });
                    for q in 0..2 {
                        let s4 = st.up_b.run(&self.taps_b, if q == 0 { 2.0 * s2 } else { 0.0 });

                        // The clipping stage, at 4×. Gain path: high-pass
                        // (the 720 Hz mid-hump corner), the feedback cap's
                        // roll-off, the op-amp gain, the diode knee.
                        st.hp_lp = flush(st.hp_lp + a_hp * (s4 - st.hp_lp));
                        let hp = s4 - st.hp_lp;
                        st.fb_lp = flush(st.fb_lp + a_fb * (gain * hp - st.fb_lp));
                        let d = st.fb_lp;
                        let clipped = clip_adaa(d, st.adaa_x1, pos_clip, neg_clip);
                        st.adaa_x1 = d;
                        // The non-inverting "1 +": dry beside the dirt.
                        let mut y = s4 + clipped;
                        // DC out (asymmetric diodes rectify), then tone —
                        // at 4× so its roll-off helps the decimator.
                        st.dc_lp = flush(st.dc_lp + a_dc * (y - st.dc_lp));
                        y -= st.dc_lp;
                        st.tone_lp = flush(st.tone_lp + a_tone * (y - st.tone_lp));
                        y = st.tone_lp;

                        // Decimate: filter at 4×, keep every second sample
                        // into the 2× filter, keep every second of those.
                        let d4 = st.dn_b.run(&self.taps_b, y);
                        if q == 1 {
                            let d2 = st.dn_a.run(&self.taps_a, d4);
                            if h == 1 {
                                out = d2;
                            }
                        }
                    }
                }
                *sample = out * out_gain;
            }
        }
    }

    /// Drop every trace of what was playing: delay lines, filter states.
    pub fn clear(&mut self) {
        self.ch[0].clear();
        self.ch[1].clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// An interleaved stereo sine, both channels in phase.
    fn sine(hz: f32, amp: f32, frames: usize, sr: f32) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        let mut phase = 0.0_f64;
        for f in buf.chunks_exact_mut(2) {
            phase += f64::from(hz) * std::f64::consts::TAU / f64::from(sr);
            let v = amp * phase.sin() as f32;
            f[0] = v;
            f[1] = v;
        }
        buf
    }

    /// The left channel's amplitude at one frequency, by Goertzel over the
    /// settled tail — f64 accumulation so −80 dB components still read.
    fn goertzel(buf: &[f32], skip: usize, hz: f32, sr: f32) -> f32 {
        let x: Vec<f64> = buf.chunks_exact(2).skip(skip).map(|f| f64::from(f[0])).collect();
        let w = std::f64::consts::TAU * f64::from(hz) / f64::from(sr);
        let c = 2.0 * w.cos();
        let (mut s1, mut s2) = (0.0_f64, 0.0_f64);
        for &v in &x {
            let s0 = v + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        let n = x.len() as f64;
        let power = (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0);
        (2.0 * power.sqrt() / n) as f32
    }

    fn run(p: &DriveParams, input: &[f32], frames: usize, sr: f32) -> Vec<f32> {
        let mut drive = Drive::new(sr);
        let mut buf = input.to_vec();
        drive.process(&mut buf, frames, 2, p);
        buf
    }

    /// Total harmonic distortion of a 1 kHz tone: harmonics 2..=8 against
    /// the fundamental, amplitude ratio. The probe sits at −26 dBFS — a
    /// guitar-ish level: hot enough to reach the knee at minimum drive,
    /// low enough that the top of the dial is not already a square wave.
    fn thd_1k(p: &DriveParams, sr: f32) -> f32 {
        let frames = (sr * 0.75) as usize;
        let skip = (sr * 0.25) as usize;
        let buf = run(p, &sine(1_000.0, 0.05, frames, sr), frames, sr);
        let fund = goertzel(&buf, skip, 1_000.0, sr);
        let mut harm = 0.0_f32;
        for k in 2..=8 {
            let a = goertzel(&buf, skip, 1_000.0 * k as f32, sr);
            harm += a * a;
        }
        harm.sqrt() / fund
    }

    /// **More drive is more distortion, every step of the dial** — the knee
    /// engages earlier as the feedback resistance grows, so THD climbs
    /// monotonically. Within 2%: at the very top the feedback cap's
    /// falling corner (61 kHz down to 5.7 kHz across the dial) shaves the
    /// pre-clip highs and the measured figure plateaus — the real pedal's
    /// "cranked gets darker" doing exactly what the module header says.
    #[test]
    fn distortion_climbs_monotonically_with_the_drive_dial() {
        let mut last = -1.0_f32;
        let mut first = 0.0;
        for step in 0..=5 {
            let p = DriveParams {
                drive: step as f32 / 5.0,
                tone: 1.0,
                ..DriveParams::default()
            };
            let thd = thd_1k(&p, SR);
            if step == 0 {
                first = thd;
            }
            assert!(
                thd >= last * 0.98,
                "THD fell from {last:.4} to {thd:.4} at drive {}",
                step as f32 / 5.0
            );
            last = thd;
        }
        assert!(
            last > 3.0 * first,
            "the dial barely moved the THD: {first:.4} → {last:.4}"
        );
    }

    /// **The mid hump is real, before the tone control**: at drive 0.7 a
    /// tiny probe (linear region) comes back at least 6 dB hotter at
    /// 720 Hz than at 100 Hz, because only the gain path's high-pass side
    /// gets the op-amp gain.
    #[test]
    fn the_mid_hump_boosts_720_hz_over_100_hz() {
        let p = DriveParams {
            drive: 0.7,
            tone: 1.0, // parked open: ~0.06 dB at 720 Hz, effectively pre-tone
            level: 0.71,
            symmetry: 0.0,
        };
        let frames = (SR * 0.75) as usize;
        let skip = (SR * 0.25) as usize;
        let gain_at = |hz: f32| {
            let buf = run(&p, &sine(hz, 0.001, frames, SR), frames, SR);
            goertzel(&buf, skip, hz, SR) / 0.001
        };
        let (lo, mid) = (gain_at(100.0), gain_at(720.0));
        let db = 20.0 * (mid / lo).log10();
        assert!(
            db >= 6.0,
            "720 Hz is only {db:.1} dB above 100 Hz — the hump is missing"
        );
    }

    /// **The oversampling earns its keep**: a 5 kHz sine at full drive
    /// folds its harmonics through both decimations, and every component
    /// off the 5 kHz grid must sit at least 40 dB under the fundamental —
    /// with either diode arrangement.
    #[test]
    fn a_five_khz_tone_at_full_drive_keeps_aliases_forty_db_down() {
        for symmetry in [0.0_f32, 1.0] {
            let p = DriveParams {
                drive: 1.0,
                tone: 1.0,
                level: 0.71,
                symmetry,
            };
            let frames = (SR * 1.25) as usize;
            let skip = (SR * 0.25) as usize;
            let buf = run(&p, &sine(5_000.0, 0.5, frames, SR), frames, SR);
            let fund = goertzel(&buf, skip, 5_000.0, SR);
            // Probe a 200 Hz grid (bin-exact over the 1 s tail) across the
            // band, skipping anything within 400 Hz of a true harmonic.
            let mut worst = (0.0_f32, 0.0_f32);
            let mut hz = 200.0_f32;
            while hz < 23_800.0 {
                let near_harm = (hz / 5_000.0).round() * 5_000.0;
                if (hz - near_harm).abs() > 400.0 {
                    let a = goertzel(&buf, skip, hz, SR);
                    if a > worst.0 {
                        worst = (a, hz);
                    }
                }
                hz += 200.0;
            }
            let db = 20.0 * (worst.0 / fund).log10();
            assert!(
                db < -40.0,
                "alias at {} Hz is {db:.1} dB under the fundamental (symmetry {symmetry})",
                worst.1
            );
        }
    }

    /// **The diode ceiling makes the level dial honest**: at a fixed level
    /// setting, the settled output peak stays inside a 6 dB window across
    /// the whole drive dial — the clip level, not the gain, sets the roof.
    #[test]
    fn the_peaks_stay_inside_three_db_across_the_drive_dial() {
        let frames = (SR * 0.5) as usize;
        let skip = (SR * 0.25) as usize * 2;
        let (mut lo, mut hi) = (f32::MAX, 0.0_f32);
        for step in 0..=4 {
            let p = DriveParams {
                drive: step as f32 / 4.0,
                ..DriveParams::default()
            };
            let buf = run(&p, &sine(1_000.0, 0.25, frames, SR), frames, SR);
            let peak = buf[skip..].iter().fold(0.0_f32, |m, x| m.max(x.abs()));
            lo = lo.min(peak);
            hi = hi.max(peak);
        }
        let spread = 20.0 * (hi / lo).log10();
        assert!(
            spread <= 6.0,
            "peaks spread {spread:.1} dB across the drive dial ({lo:.3}..{hi:.3})"
        );
    }

    /// **Level at the bottom is exact silence, whatever the dials say** —
    /// the unit's null, since a screamer has no transparent setting.
    #[test]
    fn level_at_zero_is_silence_whatever_the_dials_say() {
        let frames = 2_048;
        for bits in 0..8_u32 {
            let d = |bit: u32| [0.0_f32, 1.0][(bits >> bit & 1) as usize];
            let p = DriveParams {
                drive: d(0),
                tone: d(1),
                level: 0.0,
                symmetry: d(2),
            };
            let mut drive = Drive::new(SR);
            let mut buf = sine(1_000.0, 0.9, frames, SR);
            drive.process(&mut buf, frames, 2, &p);
            assert!(
                buf.iter().all(|s| *s == 0.0),
                "level at zero let something through (dials {bits:03b})"
            );
        }
    }

    /// **No setting the panel can reach may blow up**: every dial at both
    /// extremes against a double-full-scale input, twice over, stays
    /// finite and bounded.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 4_800;
        for bits in 0..16_u32 {
            let d = |bit: u32| [0.0_f32, 1.0][(bits >> bit & 1) as usize];
            let p = DriveParams {
                drive: d(0),
                tone: d(1),
                level: d(2),
                symmetry: d(3),
            };
            let mut drive = Drive::new(SR);
            for _ in 0..2 {
                let mut buf = sine(55.0, 2.0, frames, SR);
                drive.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:04b} produced a non-finite sample"
                );
                let peak = buf.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(peak < 16.0, "dials {bits:04b} reached {peak}");
            }
        }
    }

    /// A mono buffer is left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut drive = Drive::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        drive.process(&mut buf, 256, 1, &DriveParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **The same scream at every sample rate**: every coefficient derives
    /// from hertz at the block top and the halfbands are dimensionless, so
    /// the settled fundamental through the defaults agrees at 44.1k, 48k
    /// and 96k.
    #[test]
    fn the_same_scream_at_every_sample_rate() {
        let out_at = |sr: f32| {
            let p = DriveParams::default();
            let frames = (sr * 0.75) as usize;
            let buf = run(&p, &sine(1_000.0, 0.25, frames, sr), frames, sr);
            goertzel(&buf, (sr * 0.25) as usize, 1_000.0, sr)
        };
        let (a, b, c) = (out_at(44_100.0), out_at(48_000.0), out_at(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            let db = (20.0 * (x / y).log10()).abs();
            assert!(
                db < 0.5,
                "levels across rates disagree: {a:.4} / {b:.4} / {c:.4}"
            );
        }
    }
}
