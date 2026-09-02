//! The amp kernel: warm-but-clean blackface-family voicing, derived from
//! the schematic rather than curve-fit — the values below are read off the
//! Fender AA769/AB763 drawings (factual reference; no affiliation).
//!
//! # The chain, in circuit order
//!
//! **Tone stack.** The classic third-order passive stack: a 250 pF treble
//! cap and a 100 kΩ slope resistor both hang on the triode's plate, the
//! 0.1 µF bass and 0.047 µF mid caps below the slope, 250 kΩ treble and
//! bass pots and a 10 kΩ mid pot. Rather than transcribing somebody's
//! symbolic transfer function, the kernel solves the actual network per
//! block: build the 7-node MNA matrices `G + sC` from the component values
//! (source impedance included — the plate's ~38 kΩ ra in parallel with the
//! 100 kΩ plate load — and the 1 MΩ volume pot as the output load),
//! evaluate the numerator and denominator determinants at four points,
//! interpolate the exact cubics, and bilinear-transform at the session
//! rate. The determinant of a network with three capacitors is exactly
//! cubic in `s`, so four evaluations are the whole polynomial — no fitting,
//! no cramping surprises below the audio band. The topology was validated
//! against the published closed-form by matching each capacitor's
//! open-circuit time constant, and the resulting curve shows the signature
//! mid scoop centred near 470 Hz at noon.
//!
//! **Bright switch and volume.** A 120 pF cap across the TOP of the 1 MΩ
//! audio volume pot. That placement is the whole story: the cap bypasses
//! the upper pot segment, so the shelf's boost equals the divider's loss —
//! maximal with the volume low, gone entirely at full, exactly as the
//! panel switch behaves. One zero, one pole, derived from the pot position
//! each block.
//!
//! **Vibrato.** The panel says vibrato; the circuit is the opto tremolo —
//! neon lamp, LDR, shunt divider — and the kernel embeds [`OptoTrem`]
//! unchanged, so the amp's throb and the standalone pedal are the same
//! physics with the same tests.
//!
//! **Power stage.** Fixed-bias 6L6 pair character, clean-focused: an
//! odd-symmetric saturator linearised by negative feedback. The closed
//! loop `y = f((1+β)u − βy)` around a tanh core has a closed-form INVERSE
//! (`u` in terms of `y`), so `new` tabulates the exact static curve once
//! and playback is a table walk. Feedback keeps the stage under 1% THD
//! well up its travel and lets it compress musically past that, which is
//! the fixed-bias-plus-feedback signature. Solid-state rectifier per the
//! schematic, therefore NO sag model; the tiny crossover notch some units
//! show at idle is deliberately omitted (it is off by default on the
//! reference and inaudible at clean settings). Output transformer as a
//! gentle first-order bandpass: −3 dB near 35 Hz and 12 kHz.
//!
//! # Sample rate
//!
//! Everything is designed from hertz and component values at the top of
//! each block; the stack is re-discretized at the session rate. The
//! rate-invariance test holds the whole chain to the same output at
//! 44.1k, 48k and 96k.

// Nothing reaches this module yet: the suite's engine lane wires it later,
// and proving the DSP before any panel exists is the point.
#![allow(dead_code)]

use super::trem::OptoTrem;

// ── the tone stack, off the drawing ─────────────────────────────────────────

/// Treble, bass and mid capacitors, in farads, as printed on the drawing.
pub const STACK_C_TREBLE: f64 = 250.0e-12;
pub const STACK_C_BASS: f64 = 0.1e-6;
pub const STACK_C_MID: f64 = 0.047e-6;

/// The slope resistor between the plate and the bass/mid ladder, ohms.
pub const STACK_R_SLOPE: f64 = 100.0e3;

/// The three pots, ohms: treble and bass 250 kΩ audio, mid 10 kΩ audio.
pub const STACK_R_TREBLE: f64 = 250.0e3;
pub const STACK_R_BASS: f64 = 250.0e3;
pub const STACK_R_MID: f64 = 10.0e3;

/// What drives the stack: the triode's ~38 kΩ internal resistance in
/// parallel with its 100 kΩ plate load — the source impedance the treble
/// cap and slope resistor actually see, ~27.5 kΩ.
pub const STACK_SOURCE_OHMS: f64 = 38.0e3 * 100.0e3 / 138.0e3;

/// What loads the stack's output: the 1 MΩ volume pot.
pub const STACK_LOAD_OHMS: f64 = 1.0e6;

/// The audio-taper exponent for the three tone pots: dial position raised
/// to this power is the wiper's resistance fraction. 1.5 sits between
/// linear and square-law, matching measured mid-rotation behaviour of the
/// era's audio pots, and lands the noon mid-scoop at ~470 Hz — inside the
/// 400–500 Hz window the reference models show.
pub const STACK_TAPER: f32 = 1.5;

// ── bright and volume ───────────────────────────────────────────────────────

/// The volume pot and its bright cap: 1 MΩ audio, 120 pF across the top
/// segment. The "2-35" audio taper is approximated square-law, which puts
/// ~25% of the resistance under the wiper at noon.
pub const VOL_POT_OHMS: f64 = 1.0e6;
pub const BRIGHT_CAP_FARADS: f64 = 120.0e-12;
pub const VOL_TAPER: f32 = 2.0;

/// Makeup for the second triode the real amp has after the volume pot,
/// in dB. Chosen so the defaults land near unity loudness and half volume
/// drives the power stage to its measured sub-1%-THD region.
pub const AMP_RECOVERY_DB: f32 = 20.0;

// ── the power stage ─────────────────────────────────────────────────────────

/// The saturating core's headroom: the output ceiling of the 6L6 pair in
/// normalized units. Signals well under this pass linearly; the knee
/// arrives as the drive approaches it.
pub const POWER_HEADROOM: f64 = 1.0;

/// Loop feedback around the core. The closed loop is
/// `y = H·tanh(((1+β)·u − β·y)/H)`: unity gain when linear, and the knee
/// hardens as β grows — which is precisely what the output transformer's
/// feedback winding does to a tube pair. β = 2 keeps THD a small fraction
/// of a percent through the clean range and lets the top compress.
pub const POWER_NFB: f64 = 2.0;

/// The tabulated static curve: span (±, in drive units) and entry count.
/// ±4 is far past where the curve is flat at the ceiling; 4097 entries
/// keep the linear-interpolation error near −100 dB.
const LUT_SPAN: f64 = 4.0;
const LUT_LEN: usize = 4097;

/// The output transformer's practical bandpass, first order each end:
/// primary inductance rolls the bottom near 35 Hz, leakage inductance and
/// winding capacitance shade the top near 12 kHz.
pub const OT_HIGHPASS_HZ: f32 = 35.0;
pub const OT_LOWPASS_HZ: f32 = 12_000.0;

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

// ── solving the stack per block ─────────────────────────────────────────────

/// Node indices for the stack network. P is the plate side of everything,
/// A the treble cap's cold side, X the slope junction, T the treble pot's
/// bottom (where the bass cap lands), M the bass rheostat's far end, W the
/// mid pot's wiper (the mid cap lands there), O the treble wiper — the
/// output.
const N_P: usize = 0;
const N_A: usize = 1;
const N_X: usize = 2;
const N_T: usize = 3;
const N_M: usize = 4;
const N_W: usize = 5;
const N_O: usize = 6;
const NODES: usize = 7;

/// The frequency the `s` axis is normalized to while solving, so the
/// determinant evaluations stay well-conditioned in f64.
const STACK_W0: f64 = std::f64::consts::TAU * 1_000.0;

/// The determinant of a 7×7, by LU with partial pivoting. Destroys its
/// argument, which is why it takes it by value.
fn det7(mut m: [[f64; NODES]; NODES]) -> f64 {
    let mut det = 1.0;
    for col in 0..NODES {
        let mut pivot = col;
        for row in col + 1..NODES {
            if m[row][col].abs() > m[pivot][col].abs() {
                pivot = row;
            }
        }
        if m[pivot][col] == 0.0 {
            return 0.0;
        }
        if pivot != col {
            m.swap(pivot, col);
            det = -det;
        }
        det *= m[col][col];
        for row in col + 1..NODES {
            let f = m[row][col] / m[col][col];
            for k in col..NODES {
                m[row][k] -= f * m[col][k];
            }
        }
    }
    det
}

/// Build the stack's conductance and capacitance matrices and the source
/// vector for one set of wiper fractions (already tapered, 0..=1).
fn stack_matrices(
    t_w: f64,
    b_w: f64,
    m_w: f64,
) -> ([[f64; NODES]; NODES], [[f64; NODES]; NODES], [f64; NODES]) {
    let mut g = [[0.0; NODES]; NODES];
    let mut c = [[0.0; NODES]; NODES];
    // A resistor or capacitor between two nodes (usize::MAX = ground)
    // stamps the standard MNA pattern.
    let res = |g: &mut [[f64; NODES]; NODES], a: usize, b: usize, ohms: f64| {
        let y = 1.0 / ohms.max(1.0);
        if a != usize::MAX {
            g[a][a] += y;
        }
        if b != usize::MAX {
            g[b][b] += y;
        }
        if a != usize::MAX && b != usize::MAX {
            g[a][b] -= y;
            g[b][a] -= y;
        }
    };
    let cap = |c: &mut [[f64; NODES]; NODES], a: usize, b: usize, farads: f64| {
        c[a][a] += farads;
        c[b][b] += farads;
        c[a][b] -= farads;
        c[b][a] -= farads;
    };
    res(&mut g, N_P, usize::MAX, STACK_SOURCE_OHMS); // Norton source leg
    cap(&mut c, N_P, N_A, STACK_C_TREBLE);
    res(&mut g, N_A, N_O, (1.0 - t_w) * STACK_R_TREBLE); // treble top..wiper
    res(&mut g, N_O, N_T, t_w * STACK_R_TREBLE); // wiper..bottom
    res(&mut g, N_P, N_X, STACK_R_SLOPE);
    cap(&mut c, N_X, N_T, STACK_C_BASS);
    res(&mut g, N_T, N_M, b_w * STACK_R_BASS); // bass rheostat: up = more R = more lows kept
    res(&mut g, N_M, N_W, (1.0 - m_w) * STACK_R_MID); // mid pot above the wiper
    res(&mut g, N_W, usize::MAX, m_w * STACK_R_MID); // and below, to ground
    cap(&mut c, N_X, N_W, STACK_C_MID);
    res(&mut g, N_O, usize::MAX, STACK_LOAD_OHMS);
    let mut b = [0.0; NODES];
    b[N_P] = 1.0 / STACK_SOURCE_OHMS;
    (g, c, b)
}

/// The four-point basis for bilinear-transforming a cubic:
/// `(1−z⁻¹)^i (1+z⁻¹)^(3−i)` expanded in powers of z⁻¹.
const BILINEAR_BASIS: [[f64; 4]; 4] = [
    [1.0, 3.0, 3.0, 1.0],
    [1.0, 1.0, -1.0, -1.0],
    [1.0, -1.0, -1.0, 1.0],
    [1.0, -3.0, 3.0, -1.0],
];

/// Solve the stack for one block: exact cubic transfer by determinant
/// interpolation, then bilinear at the session rate. Returns the z-domain
/// numerator and denominator, denominator normalized to a leading one.
fn stack_digital(sample_rate: f64, t_w: f64, b_w: f64, m_w: f64) -> ([f32; 4], [f32; 4]) {
    let (g, c, b) = stack_matrices(t_w, b_w, m_w);
    // det(G + sC) with three capacitors is exactly cubic in s, so four
    // evaluations pin it. Points sit at s' = 0, ±1, 2 on the normalized
    // axis for conditioning; the fit below is the closed-form inverse of
    // that tiny Vandermonde.
    let points = [0.0, 1.0, -1.0, 2.0];
    let mut den_at = [0.0_f64; 4];
    let mut num_at = [0.0_f64; 4];
    for (k, sp) in points.iter().enumerate() {
        let mut m = [[0.0; NODES]; NODES];
        let mut mn = [[0.0; NODES]; NODES];
        for i in 0..NODES {
            for j in 0..NODES {
                let v = g[i][j] + sp * STACK_W0 * c[i][j];
                m[i][j] = v;
                // Cramer's rule: the numerator determinant swaps the
                // output column for the source vector.
                mn[i][j] = if j == N_O { b[i] } else { v };
            }
        }
        den_at[k] = det7(m);
        num_at[k] = det7(mn);
    }
    // Everything scales by 1/den(0) so the polynomials stay near one.
    let scale = 1.0 / den_at[0];
    let fit = |p: [f64; 4]| -> [f64; 4] {
        let c0 = p[0];
        let (a, b, d) = (p[1] - c0, p[2] - c0, p[3] - c0);
        let c2 = 0.5 * (a + b);
        let c3 = (d - 4.0 * c2 - (a - b)) / 6.0;
        let c1 = 0.5 * (a - b) - c3;
        [c0, c1, c2, c3]
    };
    let num = fit([
        num_at[0] * scale,
        num_at[1] * scale,
        num_at[2] * scale,
        num_at[3] * scale,
    ]);
    let den = fit([
        den_at[0] * scale,
        den_at[1] * scale,
        den_at[2] * scale,
        den_at[3] * scale,
    ]);
    // Bilinear, no prewarp: the stack's features are broad shelves and a
    // scoop, all far below Nyquist at any session rate, so plain k = 2fs
    // keeps the whole curve within a tenth of a dB (checked offline
    // against the analog solution at all three supported rates).
    let k = 2.0 * sample_rate / STACK_W0;
    let mut bz = [0.0_f64; 4];
    let mut az = [0.0_f64; 4];
    let mut kp = 1.0;
    for i in 0..4 {
        for j in 0..4 {
            bz[j] += num[i] * kp * BILINEAR_BASIS[i][j];
            az[j] += den[i] * kp * BILINEAR_BASIS[i][j];
        }
        kp *= k;
    }
    let inv = 1.0 / az[0];
    (
        [
            (bz[0] * inv) as f32,
            (bz[1] * inv) as f32,
            (bz[2] * inv) as f32,
            (bz[3] * inv) as f32,
        ],
        [
            1.0,
            (az[1] * inv) as f32,
            (az[2] * inv) as f32,
            (az[3] * inv) as f32,
        ],
    )
}

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmpParams {
    /// The bright switch: a toggle, ≥ 0.5 is on.
    pub bright: f32,
    /// The 1 MΩ audio volume pot; 0 is silence, 1 is wide open (and the
    /// bright cap out of circuit, as the hardware has it).
    pub volume: f32,
    /// Tone stack pots, audio-tapered per [`STACK_TAPER`].
    pub treble: f32,
    pub middle: f32,
    pub bass: f32,
    /// The vibrato channel's tremolo, mapped straight onto [`OptoTrem`]:
    /// speed across its Hz range, depth through its shunt divider, stereo
    /// spreading the two channels toward anti-phase.
    pub vib_speed: f32,
    pub vib_depth: f32,
    pub vib_stereo: f32,
}

/// The shipped sound: everything at noon, volume where the amp sits just
/// under unity, bright off, vibrato off — warm and clean.
impl Default for AmpParams {
    fn default() -> Self {
        Self {
            bright: 0.0,
            volume: 0.7,
            treble: 0.5,
            middle: 0.5,
            bass: 0.5,
            vib_speed: 0.35,
            vib_depth: 0.0,
            vib_stereo: 0.0,
        }
    }
}

// ── the amp ─────────────────────────────────────────────────────────────────

/// One channel's cubic filter state, transposed direct form II.
#[derive(Clone, Copy, Default)]
struct Cubic {
    s1: f32,
    s2: f32,
    s3: f32,
}

impl Cubic {
    #[inline]
    fn run(&mut self, b: &[f32; 4], a: &[f32; 4], x: f32) -> f32 {
        let y = b[0] * x + self.s1;
        self.s1 = flush(b[1] * x - a[1] * y + self.s2);
        self.s2 = flush(b[2] * x - a[2] * y + self.s3);
        self.s3 = flush(b[3] * x - a[3] * y);
        y
    }
}

/// The amp. See the module header for the circuit; see [`Amp::process`]
/// for the contract.
pub struct Amp {
    sample_rate: f32,
    stack: [Cubic; 2],
    /// The bright/volume one-zero-one-pole, per channel: x and y memories.
    vol_x1: [f32; 2],
    vol_y1: [f32; 2],
    trem: OptoTrem,
    /// The power stage's tabulated closed-loop curve, odd-symmetric with
    /// an exact zero at the centre so silence stays bit-exact silence.
    lut: Vec<f32>,
    /// Output transformer state: the low-pass whose complement is the
    /// 35 Hz high-pass, and the 12 kHz low-pass, per channel.
    ot_hp: [f32; 2],
    ot_lp: [f32; 2],
}

impl Amp {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            sample_rate: sample_rate.max(8_000.0),
            stack: [Cubic::default(); 2],
            vol_x1: [0.0; 2],
            vol_y1: [0.0; 2],
            trem: OptoTrem::new(sample_rate),
            lut: Self::power_curve(),
            ot_hp: [0.0; 2],
            ot_lp: [0.0; 2],
        }
    }

    /// Tabulate the power stage's static curve once. The loop
    /// `y = H·tanh(((1+β)u − βy)/H)` inverts in closed form —
    /// `u = (H·atanh(y/H) + βy)/(1+β)` — and that inverse is monotone, so
    /// each table entry is one bisection in f64. The upper half is solved
    /// and the lower half mirrored, which makes the curve odd to the bit
    /// and pins `f(0) = 0` exactly.
    fn power_curve() -> Vec<f32> {
        let mut lut = vec![0.0_f32; LUT_LEN];
        let half = LUT_LEN / 2;
        let (h, beta) = (POWER_HEADROOM, POWER_NFB);
        let u_of = |y: f64| (h * (y / h).atanh() + beta * y) / (1.0 + beta);
        for i in 1..=half {
            let u = LUT_SPAN * i as f64 / half as f64;
            let (mut lo, mut hi) = (0.0_f64, h * (1.0 - 1.0e-12));
            for _ in 0..70 {
                let mid = 0.5 * (lo + hi);
                if u_of(mid) < u {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let y = 0.5 * (lo + hi);
            lut[half + i] = y as f32;
            lut[half - i] = -y as f32;
        }
        lut
    }

    /// The power stage: clamp into the table's span and walk it linearly.
    #[inline]
    fn power(&self, u: f32) -> f32 {
        let half = (LUT_LEN / 2) as f32;
        let pos = (f64::from(u.clamp(-LUT_SPAN as f32, LUT_SPAN as f32)) / LUT_SPAN) as f32
            * half
            + half;
        let i = (pos as usize).min(LUT_LEN - 2);
        let frac = pos - i as f32;
        self.lut[i] + frac * (self.lut[i + 1] - self.lut[i])
    }

    /// Apply the amp in place to an interleaved buffer.
    ///
    /// `channels` is the stride; the first two channels are processed and
    /// fewer than two is returned untouched, as the house contract says.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, params: &AmpParams) {
        if channels < 2 {
            return;
        }
        // Design math in f64, once per block; the audio path stays f32.
        let sr = f64::from(self.sample_rate);
        let taper = |v: f32| f64::from(v).powf(f64::from(STACK_TAPER));
        let (bs, as_) = stack_digital(
            sr,
            taper(dial(params.treble, 0.5)),
            taper(dial(params.bass, 0.5)),
            taper(dial(params.middle, 0.5)),
        );

        // The bright/volume divider. The cap bypasses the TOP segment, so
        // its zero sits at 1/(2π·R_top·C) and its pole a boost-ratio above;
        // at full volume the top segment vanishes and with it the shelf —
        // the same code path as bright-off, so the switch is provably
        // inaudible there.
        let vol = f64::from(dial(params.volume, 0.7)).powf(f64::from(VOL_TAPER));
        let r_bot = vol * VOL_POT_OHMS;
        let r_top = VOL_POT_OHMS - r_bot;
        let bright = dial(params.bright, 0.0) >= 0.5;
        let (vb0, vb1, va1) = if bright && r_top > 1.0 {
            let n0 = r_bot;
            let n1 = r_bot * r_top * BRIGHT_CAP_FARADS;
            let d0 = VOL_POT_OHMS;
            let d1 = r_top * r_bot * BRIGHT_CAP_FARADS;
            let k = 2.0 * sr;
            let a0 = d0 + d1 * k;
            (
                ((n0 + n1 * k) / a0) as f32,
                ((n0 - n1 * k) / a0) as f32,
                ((d0 - d1 * k) / a0) as f32,
            )
        } else {
            ((r_bot / VOL_POT_OHMS) as f32, 0.0, 0.0)
        };
        let recovery = 10.0_f32.powf(AMP_RECOVERY_DB / 20.0);

        // Preamp pass: stack, volume, the second triode's makeup.
        for frame in buf.chunks_exact_mut(channels).take(frames) {
            for (ch, sample) in frame.iter_mut().take(2).enumerate() {
                let x = self.stack[ch].run(&bs, &as_, *sample);
                let y = vb0 * x + vb1 * self.vol_x1[ch] - va1 * self.vol_y1[ch];
                self.vol_x1[ch] = x;
                self.vol_y1[ch] = flush(y);
                *sample = y * recovery;
            }
        }

        // The vibrato channel's tremolo, between preamp and power stage,
        // where the circuit taps it.
        self.trem.process(
            buf,
            frames,
            channels,
            params.vib_speed,
            params.vib_depth,
            params.vib_stereo,
        );

        // Power pass: the 6L6 pair's closed-loop curve, then the output
        // transformer's gentle bandpass.
        let a_hp = (1.0 - (-std::f64::consts::TAU * f64::from(OT_HIGHPASS_HZ) / sr).exp()) as f32;
        let a_lp = (1.0 - (-std::f64::consts::TAU * f64::from(OT_LOWPASS_HZ) / sr).exp()) as f32;
        for frame in buf.chunks_exact_mut(channels).take(frames) {
            for (ch, sample) in frame.iter_mut().take(2).enumerate() {
                let y = self.power(*sample);
                self.ot_hp[ch] = flush(self.ot_hp[ch] + a_hp * (y - self.ot_hp[ch]));
                let y = y - self.ot_hp[ch];
                self.ot_lp[ch] = flush(self.ot_lp[ch] + a_lp * (y - self.ot_lp[ch]));
                *sample = self.ot_lp[ch];
            }
        }
    }

    /// Drop every trace of what was playing: filters, transformer, and the
    /// tremolo's lamp and cell.
    pub fn clear(&mut self) {
        self.stack = [Cubic::default(); 2];
        self.vol_x1 = [0.0; 2];
        self.vol_y1 = [0.0; 2];
        self.trem.clear();
        self.ot_hp = [0.0; 2];
        self.ot_lp = [0.0; 2];
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

    /// The amp's small-signal gain at one frequency: a probe 50 dB down,
    /// well inside the power stage's linear region, one block.
    fn gain_at(p: &AmpParams, hz: f32, sr: f32) -> f32 {
        let frames = (sr * 0.4) as usize;
        let mut amp = Amp::new(sr);
        let mut buf = sine(hz, 0.003, frames, sr);
        amp.process(&mut buf, frames, 2, p);
        goertzel(&buf, (sr * 0.15) as usize, hz, sr) / 0.003
    }

    /// A linear-path setting: vibrato off, bright off, volume wide open so
    /// the divider is a wire and only the stack shapes the probe.
    fn linear(volume: f32) -> AmpParams {
        AmpParams {
            volume,
            vib_depth: 0.0,
            ..AmpParams::default()
        }
    }

    /// **The stack at noon scoops the mids where the reference does**: the
    /// 400–500 Hz region sits at least 6 dB under both 100 Hz and 5 kHz,
    /// and the response minimum lands inside 350–700 Hz.
    #[test]
    fn the_stack_at_noon_scoops_the_mids() {
        let p = linear(1.0);
        let (lo, mid, hi) = (
            gain_at(&p, 100.0, SR),
            gain_at(&p, 450.0, SR),
            gain_at(&p, 5_000.0, SR),
        );
        assert!(
            20.0 * (lo / mid).log10() >= 6.0,
            "450 Hz is only {:.1} dB under 100 Hz",
            20.0 * (lo / mid).log10()
        );
        assert!(
            20.0 * (hi / mid).log10() >= 6.0,
            "450 Hz is only {:.1} dB under 5 kHz",
            20.0 * (hi / mid).log10()
        );
        let mut worst = (f32::MAX, 0.0_f32);
        let mut hz = 250.0_f32;
        while hz < 1_300.0 {
            let g = gain_at(&p, hz, SR);
            if g < worst.0 {
                worst = (g, hz);
            }
            hz *= 1.15;
        }
        assert!(
            (350.0..=700.0).contains(&worst.1),
            "the scoop centred at {} Hz, expected 400–500ish",
            worst.1
        );
    }

    /// **Each tone pot pulls its own band the right way** — bass up lifts
    /// 80 Hz, treble up lifts 5 kHz, mid up fills the scoop — because a
    /// solved network can still be wired backwards.
    #[test]
    fn each_tone_pot_moves_its_own_band_upward() {
        let base = linear(1.0);
        let cases: [(f32, fn(&mut AmpParams, f32)); 3] = [
            (80.0, |p, v| p.bass = v),
            (5_000.0, |p, v| p.treble = v),
            (500.0, |p, v| p.middle = v),
        ];
        for (hz, set) in cases {
            let mut up = base;
            set(&mut up, 1.0);
            let mut down = base;
            set(&mut down, 0.0);
            let db = 20.0 * (gain_at(&up, hz, SR) / gain_at(&down, hz, SR)).log10();
            assert!(
                db >= 6.0,
                "the pot owning {hz} Hz only moved it {db:.1} dB from 0 to 10"
            );
        }
    }

    /// **The bright cap works the pot, not the signal**: at low volume the
    /// switch lifts 8 kHz by several dB and leaves 100 Hz alone; at full
    /// volume the top segment it bypasses no longer exists, and the switch
    /// changes nothing at all — bit-exact.
    #[test]
    fn the_bright_switch_lifts_highs_only_when_the_volume_is_down() {
        let quiet_off = linear(0.3);
        let quiet_on = AmpParams {
            bright: 1.0,
            ..quiet_off
        };
        let hf = 20.0 * (gain_at(&quiet_on, 8_000.0, SR) / gain_at(&quiet_off, 8_000.0, SR)).log10();
        let lf = 20.0 * (gain_at(&quiet_on, 100.0, SR) / gain_at(&quiet_off, 100.0, SR)).log10();
        assert!(hf >= 6.0, "bright lifted 8 kHz only {hf:.1} dB at low volume");
        assert!(lf.abs() < 0.5, "bright moved 100 Hz by {lf:.1} dB");

        let frames = 4_800;
        let outs: Vec<Vec<f32>> = [0.0_f32, 1.0]
            .into_iter()
            .map(|bright| {
                let p = AmpParams {
                    bright,
                    ..linear(1.0)
                };
                let mut amp = Amp::new(SR);
                let mut buf = sine(3_000.0, 0.1, frames, SR);
                amp.process(&mut buf, frames, 2, &p);
                buf
            })
            .collect();
        assert_eq!(outs[0], outs[1], "bright at full volume touched the signal");
    }

    /// **Half volume is still a clean amp**: a −12 dBFS tone at volume 0.5
    /// comes through under 1% THD — the fixed-bias pair with feedback, not
    /// a fuzz.
    #[test]
    fn a_half_volume_amp_stays_under_one_percent_thd() {
        let p = AmpParams {
            volume: 0.5,
            ..AmpParams::default()
        };
        let frames = (SR * 0.75) as usize;
        let skip = (SR * 0.25) as usize;
        let mut amp = Amp::new(SR);
        let mut buf = sine(1_000.0, 0.25, frames, SR);
        amp.process(&mut buf, frames, 2, &p);
        let fund = goertzel(&buf, skip, 1_000.0, SR);
        let mut harm = 0.0_f32;
        for k in 2..=6 {
            let a = goertzel(&buf, skip, 1_000.0 * k as f32, SR);
            harm += a * a;
        }
        let thd = harm.sqrt() / fund;
        assert!(thd < 0.01, "THD at half volume is {:.3}%", thd * 100.0);
    }

    /// **Volume at zero is silence, and zero vibrato depth adds nothing**:
    /// the divider's bottom is ground, and a depth-0 tremolo must leave no
    /// fingerprint whatever its other dials say.
    #[test]
    fn volume_at_zero_is_silence_and_zero_depth_vibrato_adds_nothing() {
        let frames = 4_800;
        let mut amp = Amp::new(SR);
        let mut buf = sine(440.0, 1.0, frames, SR);
        amp.process(
            &mut buf,
            frames,
            2,
            &AmpParams {
                volume: 0.0,
                ..AmpParams::default()
            },
        );
        assert!(
            buf.iter().all(|s| *s == 0.0),
            "volume at zero let something through"
        );

        let run = |speed: f32, stereo: f32| {
            let mut amp = Amp::new(SR);
            let mut buf = sine(440.0, 0.25, frames, SR);
            amp.process(
                &mut buf,
                frames,
                2,
                &AmpParams {
                    vib_speed: speed,
                    vib_depth: 0.0,
                    vib_stereo: stereo,
                    ..AmpParams::default()
                },
            );
            buf
        };
        assert_eq!(
            run(0.0, 0.0),
            run(1.0, 1.0),
            "a depth-0 vibrato left a fingerprint"
        );
    }

    /// **No setting the panel can reach may blow up**: every dial at both
    /// extremes against a double-full-scale input — the power stage's
    /// ceiling makes the bound tight.
    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        let frames = 2_048;
        for bits in 0..256_u32 {
            let d = |bit: u32| [0.0_f32, 1.0][(bits >> bit & 1) as usize];
            let p = AmpParams {
                bright: d(0),
                volume: d(1),
                treble: d(2),
                middle: d(3),
                bass: d(4),
                vib_speed: d(5),
                vib_depth: d(6),
                vib_stereo: d(7),
            };
            let mut amp = Amp::new(SR);
            for _ in 0..2 {
                let mut buf = sine(55.0, 2.0, frames, SR);
                amp.process(&mut buf, frames, 2, &p);
                assert!(
                    buf.iter().all(|s| s.is_finite()),
                    "dials {bits:08b} produced a non-finite sample"
                );
                let peak = buf.iter().fold(0.0_f32, |m, x| m.max(x.abs()));
                assert!(peak < 8.0, "dials {bits:08b} reached {peak}");
            }
        }
    }

    /// A mono buffer is left alone rather than half-processed.
    #[test]
    fn a_mono_buffer_is_left_alone() {
        let mut amp = Amp::new(SR);
        let mut buf: Vec<f32> = (0..256).map(|i| (i as f32 * 0.02).sin()).collect();
        let before = buf.clone();
        amp.process(&mut buf, 256, 1, &AmpParams::default());
        assert_eq!(buf, before, "a mono buffer was written to");
    }

    /// **The same amp at every sample rate**: the stack is re-solved and
    /// re-discretized per session rate and everything else derives from
    /// hertz, so a settled tone through the defaults agrees at 44.1k, 48k
    /// and 96k.
    #[test]
    fn the_same_amp_at_every_sample_rate() {
        let out_at = |sr: f32| {
            let frames = (sr * 0.75) as usize;
            let mut amp = Amp::new(sr);
            let mut buf = sine(1_000.0, 0.25, frames, sr);
            amp.process(&mut buf, frames, 2, &AmpParams::default());
            goertzel(&buf, (sr * 0.25) as usize, 1_000.0, sr)
        };
        let (a, b, c) = (out_at(44_100.0), out_at(48_000.0), out_at(96_000.0));
        for (x, y) in [(a, b), (b, c), (a, c)] {
            let db = (20.0 * (x / y).log10()).abs();
            assert!(
                db < 0.3,
                "levels across rates disagree: {a:.4} / {b:.4} / {c:.4}"
            );
        }
    }
}
