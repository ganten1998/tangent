//! The two filter characters, and the 4× pair they run inside.
//!
//! **Both zero-delay-feedback, both solved.** A filter with feedback that is
//! delayed one sample to make it computable is a different filter: its
//! resonance peak walks flat with cutoff, its self-oscillation lands off
//! the dial, and near Nyquist it turns into something else. Both here are
//! trapezoidal-integrator (TPT) forms with the feedback solved at the
//! current sample — the ladder by Newton's method on one scalar, the
//! state-variable in closed form.
//!
//! **Ladder 24.** Four one-pole sections in series, the differential pair
//! at the input saturating the DIFFERENCE between the signal and the fed-
//! back output ([`Ladder::tick`]). Once the four linear stages are folded
//! into `y = G⁴·u + S`, the whole nonlinear loop is one equation in `y`,
//! `f(y) = G⁴·L·tanh((x − k·y)/L) + S − y`, monotone in `y`, so Newton with
//! the previous sample as its guess and the physical bracket
//! `S ± G⁴·L` as its rails converges in two steps at 4×. Three things fall
//! out of this rather than being added: resonance thins the bass (the DC
//! gain is `1/(1+k)`, and it is NOT compensated — the brief calls that a
//! feature, and it is), it self-oscillates at `k = 4` with the amplitude the
//! saturation allows, and a hot input distorts INSIDE the loop, which is
//! why the drive knob lives in the mixer and not after the filter.
//!
//! **SEM 12.** A two-pole state-variable in the Oberheim manner: low-pass,
//! band-pass and high-pass from one pair of integrators, with the Mode knob
//! walking low-pass → notch → high-pass continuously. No bass loss — the
//! DC gain of the low-pass is one whatever the resonance — and a resonance
//! law of its own: it PEAKS, up to [`SEM_Q`] at the top of the knob, and
//! does not oscillate. Chamberlin's form would go unstable exactly where a
//! resonant sweep lives; this one is stable at any cutoff the knob reaches.
//!
//! **The 4× pair is the suite's** (`units::drive`): the same Kaiser
//! designer, the same tap counts, the same shaded cutoff on the stage that
//! guards the audible fold — run polyphase here because a synth pays for
//! it per VOICE, and the direct form costs twice the multiplies for the
//! same numbers.

use crate::units::drive::{windowed_sinc, HB_A_CUTOFF, HB_A_TAPS, HB_B_TAPS};

/// The cutoff dial's range, hertz, logarithmic.
pub const CUTOFF_HZ: (f32, f32) = (20.0, 20_000.0);

/// The ladder's feedback at the top of the Emphasis dial. Four is the
/// oscillation threshold; a little past it so the top of the knob sings
/// rather than hovers, with the threshold at about 0.91 of the travel.
pub const LADDER_K_MAX: f32 = 4.4;

/// Where the ladder's differential pair saturates, in signal units. One
/// oscillator at full level is about [`crate::analog::voice::OSC_GAIN`],
/// so three of them lean on this and one does not.
pub const LADDER_SAT: f32 = 1.0;

/// The SEM's Q across its resonance dial, geometric: a shade under
/// Butterworth at the bottom, and at the top a peak of eleven decibels —
/// the brief's stability gate is twelve, and the real one never
/// oscillated either.
pub const SEM_Q: (f32, f32) = (0.6, 3.7);

/// The dial to hertz.
#[inline]
pub fn cutoff_hz(dial: f32) -> f32 {
    CUTOFF_HZ.0 * (CUTOFF_HZ.1 / CUTOFF_HZ.0).powf(dial.clamp(0.0, 1.0))
}

/// The TPT integrator gain for a cutoff at the running rate: `tan(π·f/fs)`,
/// prewarped so the resonance lands where the dial says all the way up.
/// Held below the top of the band, where the tangent goes vertical.
#[inline]
pub fn tpt_g(hz: f32, rate: f32) -> f32 {
    let x = (std::f32::consts::PI * hz / rate).min(1.4);
    x.tan()
}

/// `tanh`, rational, to a few parts in a million over the range that
/// matters and within a ten-thousandth of ±1 beyond it, where it is
/// clamped. The slope at zero is exactly one, which is what puts the
/// oscillation threshold at k = 4 and not nearby.
#[inline]
pub fn fast_tanh(x: f32) -> f32 {
    let x = x.clamp(-4.97, 4.97);
    let x2 = x * x;
    let x4 = x2 * x2;
    // Estrin's scheme rather than Horner's: the same polynomial, two
    // fewer dependent steps, and this sits in the ladder's critical
    // chain four times per sample.
    let num = x * ((135_135.0 + 17_325.0 * x2) + x4 * (378.0 + x2));
    let den = (135_135.0 + 62_370.0 * x2) + x4 * (3_150.0 + 28.0 * x2);
    num / den
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum FilterModel {
    #[default]
    Ladder,
    Sem,
}

impl FilterModel {
    pub fn from_norm(v: f32) -> FilterModel {
        if v >= 0.5 {
            FilterModel::Sem
        } else {
            FilterModel::Ladder
        }
    }
}

/// What a cutoff costs to prepare, once per base-rate sample and shared
/// by the four substeps: the TPT stage gain and its complement, the
/// stage gain to the fourth, and the SEM's own denominator.
#[derive(Clone, Copy, Debug, Default)]
pub struct Cutoff {
    /// `g / (1 + g)`
    big_g: f32,
    /// `1 / (1 + g)`
    h: f32,
    g4: f32,
    g: f32,
    /// `1 / (1 + damp·g + g²)` for the SEM at the given damping.
    sem_inv: f32,
}

impl Cutoff {
    /// For a cutoff of `hz` at the running (4×) rate and the SEM's `damp`.
    #[inline]
    pub fn new(hz: f32, rate: f32, damp: f32) -> Self {
        let g = tpt_g(hz, rate);
        let h = 1.0 / (1.0 + g);
        let big_g = g * h;
        let g2 = big_g * big_g;
        Self {
            big_g,
            h,
            g4: g2 * g2,
            g,
            sem_inv: 1.0 / (1.0 + damp * g + g * g),
        }
    }
}

/// Ladder 24, state only: the coefficients arrive per sample.
#[derive(Clone, Copy, Debug, Default)]
pub struct Ladder {
    s: [f32; 4],
    /// The last two outputs, for the predictor.
    y1: f32,
    y2: f32,
}

impl Ladder {
    pub const fn new() -> Self {
        Self { s: [0.0; 4], y1: 0.0, y2: 0.0 }
    }

    /// One sample at the running rate. `c` is the [`Cutoff`], `k` the
    /// feedback (0 ..= [`LADDER_K_MAX`]).
    ///
    /// The solve: predict `y` linearly from the last two outputs, take one
    /// Newton step on `f(y) = G⁴·L·tanh((x − k·y)/L) + S − y`, and use the
    /// tanh's own linearisation for the input the stages then see — the
    /// same first-order accuracy as the step, and one tanh per sample
    /// instead of three. The bracket `S ± G⁴·L` is the physical range of
    /// `y` and the iterate is held inside it, so the method cannot
    /// diverge whatever the input does. The bracket is applied as
    /// `max().min()` rather than `clamp()`: `clamp` asserts `lo <= hi`,
    /// which is always true here but the compiler cannot see it, and a
    /// panic branch in the body is what kept the laned form of this
    /// function scalar. One step suffices at 4×: the
    /// prediction is within a few parts in a thousand and the step is
    /// quadratic, and the self-oscillation and bass-loss tests below read
    /// the same numbers they read with three.
    #[inline]
    pub fn tick(&mut self, x: f32, c: &Cutoff, k: f32) -> f32 {
        let big_g = c.big_g;
        let h = c.h;
        let g4 = c.g4;
        let s = h * (big_g * big_g * big_g * self.s[0] + big_g * big_g * self.s[1] + big_g * self.s[2] + self.s[3]);
        let reach = g4 * LADDER_SAT;
        let (lo, hi) = (s - reach, s + reach);
        let y0 = (2.0 * self.y1 - self.y2).max(lo).min(hi);
        let t = fast_tanh((x - k * y0) * (1.0 / LADDER_SAT));
        let dudy = -k * (1.0 - t * t);
        let f = g4 * LADDER_SAT * t + s - y0;
        let y = (y0 - f / (g4 * dudy - 1.0)).max(lo).min(hi);
        // The stages' input: the tanh at the corrected y, to first order.
        let mut u = LADDER_SAT * t + dudy * (y - y0);
        for st in &mut self.s {
            let out = big_g * u + h * *st;
            *st = 2.0 * out - *st;
            u = out;
        }
        // `u` is now the fourth stage's output, the filter's, and exactly
        // what the states were advanced with.
        self.y2 = self.y1;
        self.y1 = u;
        u
    }

    pub fn clear(&mut self) {
        self.s = [0.0; 4];
        self.y1 = 0.0;
        self.y2 = 0.0;
    }
}

/// SEM 12, state only.
#[derive(Clone, Copy, Debug, Default)]
pub struct Sem {
    s1: f32,
    s2: f32,
}

impl Sem {
    pub const fn new() -> Self {
        Self { s1: 0.0, s2: 0.0 }
    }

    /// One sample. `damp` is `1/Q` (the one `c` was made with); `mode` 0
    /// low-pass, ½ notch, 1 high-pass, continuous between.
    #[inline]
    pub fn tick(&mut self, x: f32, c: &Cutoff, damp: f32, mode: f32) -> f32 {
        let g = c.g;
        let hp = (x - (damp + g) * self.s1 - self.s2) * c.sem_inv;
        let v1 = g * hp;
        let bp = v1 + self.s1;
        self.s1 = bp + v1;
        let v2 = g * bp;
        let lp = v2 + self.s2;
        self.s2 = lp + v2;
        // Low-pass, then the high-pass fades in to make the notch at the
        // middle (they cancel at cutoff), then the low-pass fades out.
        if mode <= 0.5 {
            lp + 2.0 * mode * hp
        } else {
            (2.0 - 2.0 * mode) * lp + hp
        }
    }

    pub fn clear(&mut self) {
        self.s1 = 0.0;
        self.s2 = 0.0;
    }
}

/// The SEM's damping for a resonance dial: `1/Q`, Q geometric over
/// [`SEM_Q`].
#[inline]
pub fn sem_damp(dial: f32) -> f32 {
    1.0 / (SEM_Q.0 * (SEM_Q.1 / SEM_Q.0).powf(dial.clamp(0.0, 1.0)))
}

/// The ladder's feedback for an emphasis dial.
#[inline]
pub fn ladder_k(dial: f32) -> f32 {
    LADDER_K_MAX * dial.clamp(0.0, 1.0)
}

// ── the filters in lanes ────────────────────────────────────────────────────
//
// **The same arithmetic, `L` voices wide.** Each of these holds the state
// of `L` filters as arrays and runs the scalar tick above as a loop over
// the lane index — no intrinsics, no `cfg(target_arch)`: a plain loop
// over fixed-size arrays with no branch in its body is what LLVM's loop
// vectoriser turns into NEON on arm64 and SSE on x86_64 by itself, so
// there is ONE definition and the six shipped artifacts compute the same
// numbers. The body is the scalar body verbatim, so a lane's output is
// the scalar filter's output bit for bit, and a test holds it to that.
//
// Why lanes at all: the scalar substep is a chain of dependent operations
// about a hundred cycles long, and a chain's cost is its latency. Eight
// chains side by side in vector registers cost about what one does.

/// [`Cutoff`] for `L` voices.
#[derive(Clone, Copy, Debug)]
pub struct CutoffLanes<const L: usize> {
    big_g: [f32; L],
    h: [f32; L],
    g4: [f32; L],
    g: [f32; L],
    sem_inv: [f32; L],
}

impl<const L: usize> Default for CutoffLanes<L> {
    fn default() -> Self {
        Self {
            big_g: [0.0; L],
            h: [1.0; L],
            g4: [0.0; L],
            g: [0.0; L],
            sem_inv: [1.0; L],
        }
    }
}

impl<const L: usize> CutoffLanes<L> {
    /// Lane `l` takes a cutoff computed the scalar way.
    #[inline]
    pub fn set(&mut self, l: usize, c: &Cutoff) {
        self.big_g[l] = c.big_g;
        self.h[l] = c.h;
        self.g4[l] = c.g4;
        self.g[l] = c.g;
        self.sem_inv[l] = c.sem_inv;
    }
}

/// [`Ladder`] for `L` voices.
#[derive(Clone, Copy, Debug)]
pub struct LadderLanes<const L: usize> {
    s: [[f32; L]; 4],
    y1: [f32; L],
    y2: [f32; L],
}

impl<const L: usize> Default for LadderLanes<L> {
    fn default() -> Self {
        Self {
            s: [[0.0; L]; 4],
            y1: [0.0; L],
            y2: [0.0; L],
        }
    }
}

impl<const L: usize> LadderLanes<L> {
    /// One sample for every lane: [`Ladder::tick`], `L` wide.
    #[inline]
    pub fn tick(&mut self, x: &[f32; L], c: &CutoffLanes<L>, k: &[f32; L], out: &mut [f32; L]) {
        for l in 0..L {
            let big_g = c.big_g[l];
            let h = c.h[l];
            let g4 = c.g4[l];
            let kk = k[l];
            let s = h
                * (big_g * big_g * big_g * self.s[0][l]
                    + big_g * big_g * self.s[1][l]
                    + big_g * self.s[2][l]
                    + self.s[3][l]);
            let reach = g4 * LADDER_SAT;
            let (lo, hi) = (s - reach, s + reach);
            let y0 = (2.0 * self.y1[l] - self.y2[l]).max(lo).min(hi);
            let t = fast_tanh((x[l] - kk * y0) * (1.0 / LADDER_SAT));
            let dudy = -kk * (1.0 - t * t);
            let f = g4 * LADDER_SAT * t + s - y0;
            let y = (y0 - f / (g4 * dudy - 1.0)).max(lo).min(hi);
            let mut u = LADDER_SAT * t + dudy * (y - y0);
            for st in 0..4 {
                let o = big_g * u + h * self.s[st][l];
                self.s[st][l] = 2.0 * o - self.s[st][l];
                u = o;
            }
            self.y2[l] = self.y1[l];
            self.y1[l] = u;
            out[l] = u;
        }
    }

    pub fn clear_lane(&mut self, l: usize) {
        for st in 0..4 {
            self.s[st][l] = 0.0;
        }
        self.y1[l] = 0.0;
        self.y2[l] = 0.0;
    }

    pub fn copy_lane(&mut self, from: usize, to: usize) {
        for st in 0..4 {
            self.s[st][to] = self.s[st][from];
        }
        self.y1[to] = self.y1[from];
        self.y2[to] = self.y2[from];
    }

    /// Lane `l` of `other` into lane `l` of this: how a state crosses
    /// from one group to another.
    pub fn take_lane(&mut self, other: &Self, l: usize) {
        for st in 0..4 {
            self.s[st][l] = other.s[st][l];
        }
        self.y1[l] = other.y1[l];
        self.y2[l] = other.y2[l];
    }
}

/// [`Sem`] for `L` voices.
#[derive(Clone, Copy, Debug)]
pub struct SemLanes<const L: usize> {
    s1: [f32; L],
    s2: [f32; L],
}

impl<const L: usize> Default for SemLanes<L> {
    fn default() -> Self {
        Self {
            s1: [0.0; L],
            s2: [0.0; L],
        }
    }
}

impl<const L: usize> SemLanes<L> {
    /// One sample for every lane: [`Sem::tick`], `L` wide, one mode.
    #[inline]
    pub fn tick(&mut self, x: &[f32; L], c: &CutoffLanes<L>, damp: &[f32; L], mode: f32, out: &mut [f32; L]) {
        for l in 0..L {
            let g = c.g[l];
            let hp = (x[l] - (damp[l] + g) * self.s1[l] - self.s2[l]) * c.sem_inv[l];
            let v1 = g * hp;
            let bp = v1 + self.s1[l];
            self.s1[l] = bp + v1;
            let v2 = g * bp;
            let lp = v2 + self.s2[l];
            self.s2[l] = lp + v2;
            out[l] = if mode <= 0.5 {
                lp + 2.0 * mode * hp
            } else {
                (2.0 - 2.0 * mode) * lp + hp
            };
        }
    }

    pub fn clear_lane(&mut self, l: usize) {
        self.s1[l] = 0.0;
        self.s2[l] = 0.0;
    }

    pub fn copy_lane(&mut self, from: usize, to: usize) {
        self.s1[to] = self.s1[from];
        self.s2[to] = self.s2[from];
    }

    /// Lane `l` of `other` into lane `l` of this.
    pub fn take_lane(&mut self, other: &Self, l: usize) {
        self.s1[l] = other.s1[l];
        self.s2[l] = other.s2[l];
    }
}

// ── the 4× pair, polyphase ──────────────────────────────────────────────────

/// A dot product the compiler will vectorise, in an order the laned form
/// ([`dot_lanes`]) can repeat exactly: eight accumulators, tap `t` into
/// accumulator `t % 8`, each an explicit `mul_add` — Rust does not
/// contract `a * b + c` into a fused multiply-add on its own — and the
/// same sum tree at the end. Eight because the pair's cost is latency:
/// eight independent chains of `N/8` fused adds each. `b` must be at
/// least `N` long and `N` a multiple of 8.
#[inline(always)]
fn dot<const N: usize>(a: &[f32; N], b: &[f32]) -> f32 {
    let (ac, _) = a.as_chunks::<8>();
    let (bc, _) = b[..N].as_chunks::<8>();
    let mut acc = [0.0_f32; 8];
    for (x, y) in ac.iter().zip(bc) {
        for k in 0..8 {
            acc[k] = x[k].mul_add(y[k], acc[k]);
        }
    }
    ((acc[0] + acc[1]) + (acc[2] + acc[3])) + ((acc[4] + acc[5]) + (acc[6] + acc[7]))
}

/// [`dot`] for `L` voices at once: `w[t][l]` is tap `t`'s window sample
/// for lane `l`. Tap by tap, accumulator `t % 8`, lanes side by side —
/// one vector operation per tap for every lane, and the same order as
/// the scalar dot, so a lane's result is the scalar result by bits.
#[inline(always)]
fn dot_lanes<const L: usize, const N: usize>(a: &[f32; N], w: &[[f32; L]], out: &mut [f32; L]) {
    let w = &w[..N];
    let mut acc = [[0.0_f32; L]; 8];
    let (ac, _) = a.as_chunks::<8>();
    let (wc, _) = w.as_chunks::<8>();
    for (x, y) in ac.iter().zip(wc) {
        for k in 0..8 {
            for l in 0..L {
                acc[k][l] = x[k].mul_add(y[k][l], acc[k][l]);
            }
        }
    }
    for l in 0..L {
        out[l] = ((acc[0][l] + acc[1][l]) + (acc[2][l] + acc[3][l]))
            + ((acc[4][l] + acc[5][l]) + (acc[6][l] + acc[7][l]));
    }
}

/// The two tap sets, designed once per instrument and shared by every
/// voice's four FIRs — split into the even and odd phases the
/// interpolators read and padded to a multiple of sixteen with zeros so
/// the dot products stay on the vector path.
#[derive(Clone)]
pub struct Taps {
    a_even: [f32; 32],
    a_odd: [f32; 32],
    a_full: [f32; 64],
    b_even: [f32; 16],
    b_odd: [f32; 16],
    b_full: [f32; 32],
}

impl Taps {
    pub fn new() -> Self {
        let a: [f32; HB_A_TAPS] = windowed_sinc(8.0, HB_A_CUTOFF);
        let b: [f32; HB_B_TAPS] = windowed_sinc(8.0, 0.25);
        let mut t = Self {
            a_even: [0.0; 32],
            a_odd: [0.0; 32],
            a_full: [0.0; 64],
            b_even: [0.0; 16],
            b_odd: [0.0; 16],
            b_full: [0.0; 32],
        };
        for (i, v) in a.iter().enumerate() {
            t.a_full[i] = *v;
            if i % 2 == 0 {
                t.a_even[i / 2] = *v;
            } else {
                t.a_odd[i / 2] = *v;
            }
        }
        for (i, v) in b.iter().enumerate() {
            t.b_full[i] = *v;
            if i % 2 == 0 {
                t.b_even[i / 2] = *v;
            } else {
                t.b_odd[i / 2] = *v;
            }
        }
        t
    }
}

impl Default for Taps {
    fn default() -> Self {
        Self::new()
    }
}

/// A history the FIRs read as one contiguous slice, newest first: every
/// sample is written twice, `H` apart, so the window `z[pos..pos+H]` never
/// wraps. `H` is the window length.
#[derive(Clone, Copy, Debug)]
struct History<const H: usize, const H2: usize> {
    z: [f32; H2],
    pos: usize,
}

impl<const H: usize, const H2: usize> History<H, H2> {
    const fn new() -> Self {
        Self { z: [0.0; H2], pos: 0 }
    }
    #[inline]
    fn push(&mut self, x: f32) {
        self.pos = if self.pos == 0 { H - 1 } else { self.pos - 1 };
        self.z[self.pos] = x;
        self.z[self.pos + H] = x;
    }
    #[inline]
    fn window(&self) -> &[f32] {
        &self.z[self.pos..self.pos + H]
    }
    fn clear(&mut self) {
        self.z = [0.0; H2];
        self.pos = 0;
    }
}

/// One voice's 4× pair: up through A then B, down through B then A.
#[derive(Clone, Copy, Debug)]
pub struct Oversampler {
    up_a: History<32, 64>,
    up_b: History<16, 32>,
    dn_b: History<32, 64>,
    dn_a: History<64, 128>,
}

/// Samples of delay the pair adds at the base rate: the four FIRs' group
/// delays, `(63−1)/2` at 2× twice and `(23−1)/2` at 4× twice, which sums
/// to 36.5; the decimators keep the earlier of each pair, so an impulse
/// peaks on 36. A test measures it.
pub const OVERSAMPLER_LATENCY: usize = 36;

impl Oversampler {
    pub const fn new() -> Self {
        Self {
            up_a: History::new(),
            up_b: History::new(),
            dn_b: History::new(),
            dn_a: History::new(),
        }
    }

    /// One base-rate sample in, four 4× samples out. The ×2 on each
    /// interpolator restores the amplitude its inserted zeros dilute.
    #[inline]
    pub fn up(&mut self, taps: &Taps, x: f32) -> [f32; 4] {
        self.up_a.push(x);
        let w = self.up_a.window();
        let a0 = dot(&taps.a_even, w) * 2.0;
        let a1 = dot(&taps.a_odd, w) * 2.0;
        self.up_b.push(a0);
        let w = self.up_b.window();
        let b0 = dot(&taps.b_even, w) * 2.0;
        let b1 = dot(&taps.b_odd, w) * 2.0;
        self.up_b.push(a1);
        let w = self.up_b.window();
        let b2 = dot(&taps.b_even, w) * 2.0;
        let b3 = dot(&taps.b_odd, w) * 2.0;
        [b0, b1, b2, b3]
    }

    /// Four 4× samples in, one base-rate sample out.
    #[inline]
    pub fn down(&mut self, taps: &Taps, y: [f32; 4]) -> f32 {
        self.dn_b.push(y[0]);
        self.dn_b.push(y[1]);
        let d0 = dot(&taps.b_full, self.dn_b.window());
        self.dn_a.push(d0);
        self.dn_b.push(y[2]);
        self.dn_b.push(y[3]);
        let d1 = dot(&taps.b_full, self.dn_b.window());
        self.dn_a.push(d1);
        dot(&taps.a_full, self.dn_a.window())
    }

    pub fn clear(&mut self) {
        self.up_a.clear();
        self.up_b.clear();
        self.dn_b.clear();
        self.dn_a.clear();
    }
}

impl Default for Oversampler {
    fn default() -> Self {
        Self::new()
    }
}

/// [`History`] for `L` voices: each row is one sample of every lane, so a
/// tap's window entry for all lanes is one contiguous `[f32; L]`. Written
/// twice, `H` rows apart, for the same reason the scalar one is.
#[derive(Clone, Copy, Debug)]
pub struct HistLanes<const L: usize, const H: usize, const H2: usize> {
    z: [[f32; L]; H2],
    pos: usize,
}

impl<const L: usize, const H: usize, const H2: usize> HistLanes<L, H, H2> {
    pub const fn new() -> Self {
        Self {
            z: [[0.0; L]; H2],
            pos: 0,
        }
    }
    #[inline]
    fn push(&mut self, x: &[f32; L]) {
        self.pos = if self.pos == 0 { H - 1 } else { self.pos - 1 };
        self.z[self.pos] = *x;
        self.z[self.pos + H] = *x;
    }
    #[inline]
    fn window(&self) -> &[[f32; L]] {
        &self.z[self.pos..self.pos + H]
    }
    fn clear_lane(&mut self, l: usize) {
        for row in &mut self.z {
            row[l] = 0.0;
        }
    }
    /// Lane `fl` of `from` into lane `tl` of this, realigned: the two
    /// groups' write positions differ, so the copy walks both windows
    /// from their own heads and writes both mirror rows.
    fn take_lane(&mut self, from: &Self, fl: usize, tl: usize) {
        for k in 0..H {
            let v = from.z[from.pos + k][fl];
            let idx = (self.pos + k) % H;
            self.z[idx][tl] = v;
            self.z[idx + H][tl] = v;
        }
    }
}

/// [`Oversampler`] for `L` voices.
#[derive(Clone, Copy, Debug)]
pub struct PairLanes<const L: usize> {
    up_a: HistLanes<L, 32, 64>,
    up_b: HistLanes<L, 16, 32>,
    dn_b: HistLanes<L, 32, 64>,
    dn_a: HistLanes<L, 64, 128>,
}

impl<const L: usize> Default for PairLanes<L> {
    fn default() -> Self {
        Self {
            up_a: HistLanes::new(),
            up_b: HistLanes::new(),
            dn_b: HistLanes::new(),
            dn_a: HistLanes::new(),
        }
    }
}

impl<const L: usize> PairLanes<L> {
    /// [`Oversampler::up`], `L` wide: one base-rate sample per lane in,
    /// four 4× samples per lane out.
    #[inline]
    pub fn up(&mut self, taps: &Taps, x: &[f32; L], out: &mut [[f32; L]; 4]) {
        let mut a0 = [0.0_f32; L];
        let mut a1 = [0.0_f32; L];
        self.up_a.push(x);
        {
            let w = self.up_a.window();
            dot_lanes(&taps.a_even, w, &mut a0);
            dot_lanes(&taps.a_odd, w, &mut a1);
        }
        for l in 0..L {
            a0[l] *= 2.0;
            a1[l] *= 2.0;
        }
        self.up_b.push(&a0);
        {
            let w = self.up_b.window();
            dot_lanes(&taps.b_even, w, &mut out[0]);
            dot_lanes(&taps.b_odd, w, &mut out[1]);
        }
        self.up_b.push(&a1);
        {
            let w = self.up_b.window();
            dot_lanes(&taps.b_even, w, &mut out[2]);
            dot_lanes(&taps.b_odd, w, &mut out[3]);
        }
        for o in out.iter_mut() {
            for l in 0..L {
                o[l] *= 2.0;
            }
        }
    }

    /// [`Oversampler::down`], `L` wide.
    #[inline]
    pub fn down(&mut self, taps: &Taps, y: &[[f32; L]; 4], out: &mut [f32; L]) {
        let mut d = [0.0_f32; L];
        self.dn_b.push(&y[0]);
        self.dn_b.push(&y[1]);
        dot_lanes(&taps.b_full, self.dn_b.window(), &mut d);
        self.dn_a.push(&d);
        self.dn_b.push(&y[2]);
        self.dn_b.push(&y[3]);
        dot_lanes(&taps.b_full, self.dn_b.window(), &mut d);
        self.dn_a.push(&d);
        dot_lanes(&taps.a_full, self.dn_a.window(), out);
    }

    pub fn clear_lane(&mut self, l: usize) {
        self.up_a.clear_lane(l);
        self.up_b.clear_lane(l);
        self.dn_b.clear_lane(l);
        self.dn_a.clear_lane(l);
    }

    /// Lane `fl` of `from` into lane `tl` of this, every history
    /// realigned.
    pub fn take_lane(&mut self, from: &Self, fl: usize, tl: usize) {
        self.up_a.take_lane(&from.up_a, fl, tl);
        self.up_b.take_lane(&from.up_b, fl, tl);
        self.dn_b.take_lane(&from.dn_b, fl, tl);
        self.dn_a.take_lane(&from.dn_a, fl, tl);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::measure::{db, goertzel_hann, measure_hz, peak};

    const SR: f32 = 48_000.0;
    /// The rate the filters actually run at.
    const SR4: f32 = 4.0 * SR;

    /// One cycle of a sine at `hz`, amplitude ½, then silence: a kick the
    /// filter can feel at any cutoff, where a one-sample impulse into a
    /// 24 dB/octave low-pass at 80 Hz is a few parts in a million.
    fn burst(hz: f32, i: usize) -> f32 {
        let period = SR4 / hz;
        if (i as f32) < period {
            0.5 * (i as f32 / period * std::f32::consts::TAU).sin()
        } else {
            0.0
        }
    }

    /// **The brief's self-oscillation gate**: with the emphasis at the top
    /// and nothing but a nudge at the input, the ladder sings at the
    /// cutoff the dial says, within fifteen cents, from the bass to the
    /// top of the band.
    #[test]
    fn the_ladder_self_oscillates_at_the_dials_pitch() {
        for hz in [80.0_f32, 220.0, 1_000.0, 3_000.0, 8_000.0] {
            let mut f = Ladder::new();
            let g = Cutoff::new(hz, SR4, 1.0);
            let k = ladder_k(1.0);
            // Long enough for the slowest growth (80 Hz) to reach its
            // limit cycle; the last third is what gets read.
            let n = (SR4 * 1.5) as usize;
            let x: Vec<f32> = (0..n).map(|i| f.tick(burst(hz, i), &g, k)).collect();
            let settled = &x[n * 2 / 3..];
            let amp = peak(settled);
            assert!(amp > 0.05, "{hz} Hz: no oscillation (peak {amp})");
            let got = measure_hz(settled, hz, SR4);
            let cents = 1200.0 * (got / hz).log2();
            assert!(
                cents.abs() <= 15.0,
                "{hz} Hz: oscillates at {got:.2} Hz ({cents:+.1} cents)"
            );
        }
    }

    /// Below the threshold it does NOT sing: at 0.85 of the dial an
    /// impulse rings and dies.
    #[test]
    fn under_the_threshold_the_ladder_rings_and_dies() {
        let mut f = Ladder::new();
        let g = Cutoff::new(1_000.0, SR4, 1.0);
        let k = ladder_k(0.85);
        let n = (SR4 * 0.5) as usize;
        let x: Vec<f32> = (0..n).map(|i| f.tick(burst(1_000.0, i), &g, k)).collect();
        let early = peak(&x[..n / 10]);
        let late = peak(&x[n * 9 / 10..]);
        assert!(early > 0.05, "no ring at all: {early}");
        assert!(late < early * 1e-3, "still ringing: {late} against {early}");
    }

    /// The level of a 100 Hz sine under a 1 kHz cutoff as the emphasis
    /// climbs: a sine's steady level through the ladder.
    fn bass_level_db(emphasis: f32) -> f32 {
        let mut f = Ladder::new();
        let g = Cutoff::new(1_000.0, SR4, 1.0);
        let k = ladder_k(emphasis);
        let n = (SR4 * 0.5) as usize;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(SR4);
                f.tick((0.1 * (t * 100.0 * std::f64::consts::TAU).sin()) as f32, &g, k)
            })
            .collect();
        db(goertzel_hann(&x[n / 2..], 100.0, SR4) / 0.1)
    }

    /// **The bass loss is real, monotone and uncompensated.** Each step of
    /// emphasis takes more bass than the last, and at 0.9 of the dial the
    /// loss is the `1/(1+k)` the circuit predicts, within a decibel.
    #[test]
    fn the_ladder_loses_bass_with_emphasis_and_nobody_fixes_it() {
        let mut last = f32::MAX;
        for step in 0..=9 {
            let e = step as f32 / 10.0;
            let level = bass_level_db(e);
            assert!(level < last - 0.05, "emphasis {e}: bass {level:.2} dB did not fall from {last:.2}");
            last = level;
        }
        let want = db(1.0 / (1.0 + ladder_k(0.9)));
        assert!(
            (last - want).abs() < 1.0,
            "at 0.9 the bass sits {last:.2} dB; 1/(1+k) says {want:.2} dB"
        );
    }

    /// **The brief's SEM stability gate**: resonance at the top, the cutoff
    /// swept 20 Hz to 20 kHz under a sine, every sample finite and the peak
    /// no more than twelve decibels over the input — in every mode.
    #[test]
    fn the_sem_stays_finite_and_under_twelve_db_through_a_full_sweep() {
        for mode in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
            let mut f = Sem::new();
            let damp = sem_damp(1.0);
            let n = (SR4 * 2.0) as usize;
            let mut worst = 0.0_f32;
            for i in 0..n {
                let t = i as f64 / f64::from(SR4);
                let hz = 20.0 * 1000.0_f32.powf(i as f32 / n as f32);
                let x = (0.5 * (t * 1_000.0 * std::f64::consts::TAU).sin()) as f32;
                let y = f.tick(x, &Cutoff::new(hz, SR4, damp), damp, mode);
                assert!(y.is_finite(), "mode {mode}: non-finite at {hz:.0} Hz");
                worst = worst.max(y.abs());
            }
            let over = db(worst / 0.5);
            assert!(over <= 12.0, "mode {mode}: peak {over:+.1} dB over the input");
        }
    }

    /// The SEM has no bass loss: a 50 Hz sine under a 2 kHz cutoff comes
    /// through at unity at any resonance.
    #[test]
    fn the_sem_keeps_its_bass_at_any_resonance() {
        for res in [0.0_f32, 0.5, 1.0] {
            let mut f = Sem::new();
            let g = Cutoff::new(2_000.0, SR4, sem_damp(res));
            let n = (SR4 * 0.5) as usize;
            let x: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f64 / f64::from(SR4);
                    f.tick((0.1 * (t * 50.0 * std::f64::consts::TAU).sin()) as f32, &g, sem_damp(res), 0.0)
                })
                .collect();
            let level = db(goertzel_hann(&x[n / 2..], 50.0, SR4) / 0.1);
            assert!(level.abs() < 0.3, "resonance {res}: bass at {level:+.2} dB");
        }
    }

    /// The mode knob's middle is a notch: a sine AT the cutoff drops thirty
    /// decibels, and the ends are the low-pass and high-pass they claim.
    #[test]
    fn the_sem_mode_knob_walks_low_pass_notch_high_pass() {
        let level = |mode: f32, hz: f32| {
            let mut f = Sem::new();
            let g = Cutoff::new(1_000.0, SR4, sem_damp(0.3));
            let n = (SR4 * 0.5) as usize;
            let x: Vec<f32> = (0..n)
                .map(|i| {
                    let t = i as f64 / f64::from(SR4);
                    f.tick((0.1 * (t * f64::from(hz) * std::f64::consts::TAU).sin()) as f32, &g, sem_damp(0.3), mode)
                })
                .collect();
            db(goertzel_hann(&x[n / 2..], hz, SR4) / 0.1)
        };
        assert!(level(0.5, 1_000.0) < -30.0, "notch depth {:.1} dB", level(0.5, 1_000.0));
        assert!(level(0.0, 100.0).abs() < 0.5 && level(0.0, 10_000.0) < -30.0, "not a low-pass");
        assert!(level(1.0, 10_000.0).abs() < 0.5 && level(1.0, 100.0) < -30.0, "not a high-pass");
    }

    /// The 4× pair passes a tone at unity and delays it by exactly
    /// [`OVERSAMPLER_LATENCY`] samples.
    #[test]
    fn the_pair_is_unity_and_its_latency_is_what_it_says() {
        let taps = Taps::new();
        let mut os = Oversampler::new();
        let n = 200;
        let y: Vec<f32> = (0..n)
            .map(|i| {
                let u = os.up(&taps, if i == 0 { 1.0 } else { 0.0 });
                os.down(&taps, u)
            })
            .collect();
        let at = (0..n).max_by(|&a, &b| y[a].abs().partial_cmp(&y[b].abs()).unwrap()).unwrap();
        assert_eq!(at, OVERSAMPLER_LATENCY, "impulse peaked at {at}");
        let mut os = Oversampler::new();
        let n = (SR * 0.5) as usize;
        let x: Vec<f32> = (0..n)
            .map(|i| {
                let t = i as f64 / f64::from(SR);
                let u = os.up(&taps, (0.5 * (t * 1_000.0 * std::f64::consts::TAU).sin()) as f32);
                os.down(&taps, u)
            })
            .collect();
        let level = db(goertzel_hann(&x[n / 2..], 1_000.0, SR) / 0.5);
        assert!(level.abs() < 0.1, "1 kHz through the pair: {level:+.3} dB");
    }

    /// **The lanes are the scalar filters, bit for bit.** Eight lanes fed
    /// eight different signals, cutoffs and feedbacks against eight scalar
    /// filters fed the same — every sample equal, not close. The lane
    /// bodies are the scalar bodies verbatim and Rust contracts nothing,
    /// so any difference is a transcription error, and this is where it
    /// would show.
    #[test]
    fn the_laned_filters_are_the_scalar_filters_bit_for_bit() {
        const L: usize = 8;
        let mut scalar_l: Vec<Ladder> = (0..L).map(|_| Ladder::new()).collect();
        let mut scalar_s: Vec<Sem> = (0..L).map(|_| Sem::new()).collect();
        let mut lanes_l = LadderLanes::<L>::default();
        let mut lanes_s = SemLanes::<L>::default();
        let mut cl = CutoffLanes::<L>::default();
        let mut k = [0.0_f32; L];
        let mut damp = [0.0_f32; L];
        let mut cuts = Vec::new();
        for l in 0..L {
            let hz = 200.0 * 1.7_f32.powi(l as i32);
            k[l] = ladder_k(0.1 + 0.12 * l as f32);
            damp[l] = sem_damp(0.1 + 0.12 * l as f32);
            let c = Cutoff::new(hz, SR4, damp[l]);
            cl.set(l, &c);
            cuts.push(c);
        }
        let n = (SR4 * 0.25) as usize;
        let mut x = [0.0_f32; L];
        let mut out_l = [0.0_f32; L];
        let mut out_s = [0.0_f32; L];
        for i in 0..n {
            for l in 0..L {
                x[l] = 0.6 * ((i as f32) * (0.011 + 0.003 * l as f32)).sin();
            }
            lanes_l.tick(&x, &cl, &k, &mut out_l);
            lanes_s.tick(&x, &cl, &damp, 0.3, &mut out_s);
            for l in 0..L {
                let a = scalar_l[l].tick(x[l], &cuts[l], k[l]);
                let b = scalar_s[l].tick(x[l], &cuts[l], damp[l], 0.3);
                assert!(a.to_bits() == out_l[l].to_bits(), "ladder lane {l} sample {i}: {a} vs {}", out_l[l]);
                assert!(b.to_bits() == out_s[l].to_bits(), "SEM lane {l} sample {i}: {b} vs {}", out_s[l]);
            }
        }
        // And the lanes did something: not silence.
        assert!(out_l.iter().any(|v| v.abs() > 1e-3));
    }

    /// **The laned pair is the scalar pair, bit for bit** — including
    /// after a lane is copied across groups mid-stream, which is what a
    /// steal does to it.
    #[test]
    fn the_laned_pair_is_the_scalar_pair_bit_for_bit() {
        const L: usize = 8;
        let taps = Taps::new();
        let mut scalar: Vec<Oversampler> = (0..L).map(|_| Oversampler::new()).collect();
        let mut lanes = PairLanes::<L>::default();
        let mut other = PairLanes::<L>::default();
        let n = 3_000;
        let mut x = [0.0_f32; L];
        let mut up = [[0.0_f32; L]; 4];
        let mut down = [0.0_f32; L];
        for i in 0..n {
            for l in 0..L {
                x[l] = 0.7 * ((i as f32) * (0.02 + 0.005 * l as f32)).sin();
            }
            lanes.up(&taps, &x, &mut up);
            let mut y4 = [[0.0_f32; L]; 4];
            for j in 0..4 {
                for l in 0..L {
                    // Something nonlinear between, as the filter is.
                    y4[j][l] = fast_tanh(up[j][l] * 1.5);
                }
            }
            lanes.down(&taps, &y4, &mut down);
            for l in 0..L {
                let u = scalar[l].up(&taps, x[l]);
                for j in 0..4 {
                    assert!(u[j].to_bits() == up[j][l].to_bits(), "up lane {l} sample {i} phase {j}");
                }
                let y = [fast_tanh(u[0] * 1.5), fast_tanh(u[1] * 1.5), fast_tanh(u[2] * 1.5), fast_tanh(u[3] * 1.5)];
                let d = scalar[l].down(&taps, y);
                assert!(d.to_bits() == down[l].to_bits(), "down lane {l} sample {i}: {d} vs {}", down[l]);
            }
            // Halfway: lane 2 moves to lane 5 of another group whose head
            // sits elsewhere, and from then on that lane must read the
            // same as scalar 2.
            if i == n / 2 {
                for _ in 0..7 {
                    other.up(&taps, &[0.0; L], &mut [[0.0; L]; 4]);
                    other.down(&taps, &[[0.0; L]; 4], &mut [0.0; L]);
                }
                other.take_lane(&lanes, 2, 5);
            }
            if i > n / 2 {
                let mut x2 = [0.0_f32; L];
                x2[5] = x[2];
                let mut up2 = [[0.0_f32; L]; 4];
                other.up(&taps, &x2, &mut up2);
                assert!(up2[0][5].to_bits() == up[0][2].to_bits(), "moved lane, sample {i}");
                let mut y2 = [[0.0_f32; L]; 4];
                for j in 0..4 {
                    y2[j][5] = y4[j][2];
                }
                let mut d2 = [0.0_f32; L];
                other.down(&taps, &y2, &mut d2);
                assert!(d2[5].to_bits() == down[2].to_bits(), "moved lane down, sample {i}");
            }
        }
        assert!(down.iter().any(|v| v.abs() > 1e-3));
    }

    /// The rational tanh keeps its slope at zero and its rails, and sits
    /// within a few parts in a million of the real one.
    #[test]
    fn the_fast_tanh_is_the_real_one_where_it_matters() {
        let mut t = -6.0_f32;
        while t <= 6.0 {
            let (a, b) = (fast_tanh(t), t.tanh());
            assert!((a - b).abs() < 1e-4, "tanh({t}): {a} vs {b}");
            t += 0.01;
        }
        let slope = fast_tanh(1e-3) / 1e-3;
        assert!((slope - 1.0).abs() < 1e-5, "slope at zero {slope}");
    }
}
