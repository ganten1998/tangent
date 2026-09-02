//! The DSP for Tangent EQ: up to eight second-order sections, designed so
//! that the curve the display will draw is the curve the audio gets.
//!
//! # Natural phase
//!
//! Every section is minimum-phase and causal: no lookahead, no latency, no
//! pre-ring. The phase a bell adds is the phase an analog bell adds, which is
//! what "natural" means here as opposed to "linear" — a linear-phase EQ buys
//! its flat phase with latency, and the built-in chain has no way to report
//! latency into the desk's compensation (docs/FX-DSP-HANDOFF.md; the PDC
//! plumbing serves plugins only). Minimum phase is the honest choice before
//! it is the cheap one.
//!
//! # Cramping, and the matched fix
//!
//! The bilinear transform folds the ENTIRE analog frequency axis into the
//! span below Nyquist. Near DC the fold is invisible; near Nyquist it
//! presses everything against the wall. A +12 dB, Q=4 bell at 10 kHz in a
//! 44.1 kHz stream keeps its peak — the corner is prewarped — but its upper
//! skirt is squeezed: the analog prototype says +1.7 dB at 14 kHz and an RBJ
//! section delivers +0.6. On a visual EQ that is the handle promising one
//! curve and the section shipping another.
//!
//! Bell and both shelves are therefore MATCHED, after Vicanek, "Matched
//! Second Order Digital Filters" (2016). Two ideas:
//!
//! 1. **Poles by the matched z-transform.** `z = e^{sT}` maps the analog
//!    pole pair without warping — an exponential has no wall to press
//!    against — so the section's resonance sits where the prototype put it
//!    at every sample rate, and stability is free (`|e^{-σT}| < 1` always).
//! 2. **Numerator solved against the analog magnitude.** A biquad's squared
//!    magnitude is a ratio of quadratics in `φ = sin²(ω/2)`; the analog
//!    prototype's is a ratio of quadratics in `Ω²`. Two curves of the same
//!    algebraic shape that agree at three points track each other in
//!    between, so the three numerator coefficients are solved to make the
//!    digital magnitude EQUAL the analog one at three anchors: DC, the node
//!    frequency, and Nyquist. The identity doing the work, verifiable by
//!    expanding `|b₀ + b₁e^{-jω} + b₂e^{-j2ω}|²`:
//!
//!    `|B(ω)|² = (b₀+b₁+b₂)² − 4(b₀b₁ + 4b₀b₂ + b₁b₂)φ + 16b₀b₂φ²`
//!
//!    which turns "match a magnitude at three frequencies" into one linear
//!    solve and a quadratic root, all in f64.
//!
//! High-pass, low-pass and notch stay RBJ (bilinear, Butterworth-shaped at
//! the default Q): prewarping pins their corner exactly, and their cramping
//! bends the stopband TOWARD more rejection — a low-pass that reaches the
//! wall a shade early is not a curve anybody has ever filed a bug about,
//! where a bell's folded skirt is.
//!
//! # The display contract
//!
//! [`response_db`] evaluates the exact digital `|H|` of the same
//! coefficients the audio runs — same design code, same anchors, evaluated
//! on whatever grid the curve painter asks for. The audio path quantizes
//! those coefficients to f32 (house law: f64 to design, f32 to play), which
//! bites hardest where poles crowd `z = 1` — the lowest corners at the
//! highest rates — and a test holds the drawn curve and a measured sine to
//! within a fifth of a decibel of each other across the band.

use std::f64::consts::{PI, TAU};

/// How many bands the kernel carries. Eight, because that is what the panel
/// will offer: enough for a corrective pass and a tone pass at once, few
/// enough that every handle stays reachable on a laptop-width display.
pub const MAX_NODES: usize = 8;

/// Where a node's frequency knob sweeps, in Hz. Geometric, because the ear
/// walks in ratios rather than in hertz: every octave gets the same width of
/// knob, as it gets the same width of keyboard.
pub const EQ_FREQ_HZ: (f32, f32) = (20.0, 20_000.0);

/// How far the gain knob reaches either way, in dB. ±24 covers repair work
/// with room to spare; the knob's centre is exactly 0 dB so a handle at rest
/// is a handle doing nothing.
pub const EQ_GAIN_DB: f32 = 24.0;

/// What the Q knob spans, geometrically. 0.1 is a tone control wider than
/// the band it lives in; 24 is a surgical notch-width bell for chasing one
/// resonance.
pub const EQ_Q: (f32, f32) = (0.1, 24.0);

/// The knob position that lands [`EQ_Q`]'s geometric sweep on Butterworth's
/// 1/√2 — `ln(0.7071/0.1) / ln(24.0/0.1)`. The default, because a bell at
/// this width neither rings nor smears, and a test pins the arithmetic.
pub const EQ_Q_BUTTERWORTH: f32 = 0.356_92;

/// Below this, a sample is on its way to a denormal and is worth nothing —
/// same guard, same reasoning as the effects chain: a float 400 dB down
/// costs a hundred times a normal one on some hardware, and a filter tail
/// decays toward zero forever without ever arriving.
const TINY: f32 = 1.0e-20;

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

// ── what a node is ──────────────────────────────────────────────────────────

/// What shape a band takes. Stepped, so the panel offers it as a menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NodeKind {
    #[default]
    Bell,
    LowShelf,
    HighShelf,
    HighPass,
    LowPass,
    Notch,
}

impl NodeKind {
    /// Every value, in the order a menu should offer them: the gain-carrying
    /// shapes first, because they are what an EQ is for. Append-only forever,
    /// as with every stepped enum that reaches a settings file.
    pub const ALL: [NodeKind; 6] = [
        NodeKind::Bell,
        NodeKind::LowShelf,
        NodeKind::HighShelf,
        NodeKind::HighPass,
        NodeKind::LowPass,
        NodeKind::Notch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            NodeKind::Bell => "bell",
            NodeKind::LowShelf => "low shelf",
            NodeKind::HighShelf => "high shelf",
            NodeKind::HighPass => "high-pass",
            NodeKind::LowPass => "low-pass",
            NodeKind::Notch => "notch",
        }
    }

    /// How it will be written to a settings file, and read back.
    pub fn key(self) -> &'static str {
        match self {
            NodeKind::Bell => "bell",
            NodeKind::LowShelf => "low-shelf",
            NodeKind::HighShelf => "high-shelf",
            NodeKind::HighPass => "high-pass",
            NodeKind::LowPass => "low-pass",
            NodeKind::Notch => "notch",
        }
    }

    pub fn from_key(key: &str) -> Option<NodeKind> {
        NodeKind::ALL.into_iter().find(|k| k.key() == key)
    }

    /// Whether the gain knob means anything to this shape. A high-pass at
    /// +6 dB is not a thing; the panel greys the knob rather than lying.
    pub fn has_gain(self) -> bool {
        matches!(
            self,
            NodeKind::Bell | NodeKind::LowShelf | NodeKind::HighShelf
        )
    }
}

/// One band, in physical units — what the designer and the curve display
/// speak. The knob-space twin is [`NodeParams`]; the mapping between them is
/// the consts at the top of this file and nothing else.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub kind: NodeKind,
    pub freq_hz: f32,
    pub gain_db: f32,
    pub q: f32,
}

// ── the coefficient designer ────────────────────────────────────────────────

/// One second-order section's coefficients, in f64 because they are DESIGN,
/// not audio: the matched solve subtracts near-equal quantities for
/// low-frequency nodes, and f32 there moves the pole by whole percent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BiquadCoeffs {
    pub b0: f64,
    pub b1: f64,
    pub b2: f64,
    pub a1: f64,
    pub a2: f64,
}

impl BiquadCoeffs {
    /// A wire: `H(z) = 1`. What a gain shape at 0 dB collapses to, so a
    /// parked handle costs its multiply-adds and changes nothing else.
    pub const IDENTITY: BiquadCoeffs = BiquadCoeffs {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    /// The exact digital squared magnitude at radian frequency `w` — the
    /// audio path's transfer function evaluated on the unit circle, no
    /// approximation. This is the arithmetic the curve display draws.
    pub fn mag2(&self, w: f64) -> f64 {
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let nr = self.b0 + self.b1 * c1 + self.b2 * c2;
        let ni = self.b1 * s1 + self.b2 * s2;
        let dr = 1.0 + self.a1 * c1 + self.a2 * c2;
        let di = self.a1 * s1 + self.a2 * s2;
        (nr * nr + ni * ni) / (dr * dr + di * di).max(1.0e-300)
    }
}

impl Node {
    /// Design this node against a sample rate.
    ///
    /// Inputs are clamped here, not trusted: a hand-edited settings file or
    /// a NaN that slipped a mapping must produce a stable section, never a
    /// pole outside the circle. The frequency ceiling is 0.46·sr — close
    /// enough to keep 20 kHz honest in a 44.1 kHz stream, far enough from π
    /// that the matched solve's `φ(1−φ)` pivot stays conditioned and the
    /// bilinear `tan` stays finite.
    pub fn coeffs(&self, sample_rate: f64) -> BiquadCoeffs {
        let sr = if sample_rate.is_finite() {
            sample_rate.max(8_000.0)
        } else {
            44_100.0
        };
        let sane = |v: f32, lo: f64, hi: f64, or: f64| {
            let v = f64::from(v);
            if v.is_finite() {
                v.clamp(lo, hi)
            } else {
                or
            }
        };
        let hz = sane(self.freq_hz, 5.0, 0.46 * sr, 1_000.0);
        let q = sane(self.q, 0.05, 40.0, 0.707);
        let gain_db = sane(self.gain_db, -30.0, 30.0, 0.0);
        let w0 = TAU * hz / sr;
        match self.kind {
            NodeKind::Bell | NodeKind::LowShelf | NodeKind::HighShelf => {
                matched(self.kind, w0, gain_db, q)
            }
            NodeKind::HighPass | NodeKind::LowPass | NodeKind::Notch => {
                rbj(self.kind, w0, q)
            }
        }
    }
}

/// The analog prototype's squared magnitude, `omega` in units of the node
/// frequency, `a = √G` per the RBJ convention (so a bell peaks at `a²`, a
/// shelf lands there). One function feeds both the matched designer's three
/// anchors and the test that holds the finished filter against it — if they
/// ever disagreed about what "the analog prototype" is, the match would be
/// to nothing.
fn analog_mag2(kind: NodeKind, omega: f64, a: f64, q: f64) -> f64 {
    let o2 = omega * omega;
    match kind {
        // H(s) = (s² + (a/q)s + 1) / (s² + s/(a·q) + 1)
        NodeKind::Bell => {
            let base = (1.0 - o2) * (1.0 - o2);
            (base + o2 * (a / q) * (a / q)) / (base + o2 / ((a * q) * (a * q)))
        }
        // H(s) = a·(s² + (√a/q)s + a) / (a·s² + (√a/q)s + 1)
        NodeKind::LowShelf => {
            let t = a * o2 / (q * q);
            a * a * ((a - o2) * (a - o2) + t) / ((1.0 - a * o2) * (1.0 - a * o2) + t)
        }
        // H(s) = a·(a·s² + (√a/q)s + 1) / (s² + (√a/q)s + a)
        NodeKind::HighShelf => {
            let t = a * o2 / (q * q);
            a * a * ((1.0 - a * o2) * (1.0 - a * o2) + t) / ((a - o2) * (a - o2) + t)
        }
        _ => 1.0,
    }
}

/// `|1 + a₁e^{-jω} + a₂e^{-j2ω}|²` as a quadratic in `φ = sin²(ω/2)` — the
/// identity from the module header, applied to a denominator.
fn den_mag2_phi(a1: f64, a2: f64, phi: f64) -> f64 {
    let c0 = (1.0 + a1 + a2) * (1.0 + a1 + a2);
    let c1 = -4.0 * (a1 + 4.0 * a2 + a1 * a2);
    let c2 = 16.0 * a2;
    c0 + c1 * phi + c2 * phi * phi
}

/// The matched z-transform of one analog pole pair: natural frequency `w`
/// (radians/sample), `qn = 1/(2Q)` the damping. Unconditionally stable —
/// `a₂ = e^{-2·qn·w}` cannot leave the unit circle.
fn mzt_pair(w: f64, qn: f64) -> (f64, f64) {
    let a2 = (-2.0 * qn * w).exp();
    let a1 = if qn <= 1.0 {
        -2.0 * (-qn * w).exp() * ((1.0 - qn * qn).sqrt() * w).cos()
    } else {
        // Overdamped: the pair has split onto the real axis and the cosine
        // becomes its hyperbolic twin. The cosh grows like e^{qn·w} and the
        // front factor shrinks like e^{-qn·w}, so the product stays near
        // unity; at the range's far corner the cosh alone reaches ~1e74,
        // still two hundred decades inside f64.
        -2.0 * (-qn * w).exp() * ((qn * qn - 1.0).sqrt() * w).cosh()
    };
    (a1, a2)
}

/// The Vicanek-matched bell and shelves. See the module header for the two
/// ideas; here is only the arithmetic.
fn matched(kind: NodeKind, w0: f64, gain_db: f64, q: f64) -> BiquadCoeffs {
    // 0 dB collapses to a wire exactly, which keeps "a handle at rest does
    // nothing" a bit-for-bit truth rather than an epsilon one.
    if gain_db.abs() < 1.0e-4 {
        return BiquadCoeffs::IDENTITY;
    }
    let a = 10.0_f64.powf(gain_db / 40.0);

    // The prototype's pole pair, per shape. The RBJ split puts a bell's
    // poles AT the node frequency with quality a·q (boost lowers their
    // damping, cut raises it); a shelf's poles sit a half-gain off the
    // corner — at ω₀/√a below it for the low shelf, ω₀·√a above for the
    // high — with the node's own Q. At an extreme of gain times frequency
    // the shelf pole can walk past Nyquist; it is pinned just inside, and
    // the numerator anchors below absorb the difference.
    let (wp, qp) = match kind {
        NodeKind::Bell => (w0, a * q),
        NodeKind::LowShelf => (w0 / a.sqrt(), q),
        NodeKind::HighShelf => (w0 * a.sqrt(), q),
        _ => unreachable!("only the gain shapes are matched"),
    };
    let (a1, a2) = mzt_pair(wp.min(0.98 * PI), 1.0 / (2.0 * qp));

    // The three anchors: what |B|² must be so that |B|²/|D|² equals the
    // analog magnitude at DC, at the node frequency, and at Nyquist. The
    // analog frequency corresponding to digital ω is Ω = ω/ω₀ — same hertz,
    // two rulers.
    let phi0 = (0.5 * w0).sin() * (0.5 * w0).sin();
    let r_dc = analog_mag2(kind, 0.0, a, q) * (1.0 + a1 + a2) * (1.0 + a1 + a2);
    let r_ny = analog_mag2(kind, PI / w0, a, q) * (1.0 - a1 + a2) * (1.0 - a1 + a2);
    let r_w0 = analog_mag2(kind, 1.0, a, q) * den_mag2_phi(a1, a2, phi0);

    // |B|² = B₀ + B₁φ + B₂φ²: B₀ is the DC anchor outright (φ=0), and the
    // other two anchors are two linear equations in B₁, B₂.
    let b_0 = r_dc;
    let b_1 = ((r_w0 - r_dc) - (r_ny - r_dc) * phi0 * phi0) / (phi0 * (1.0 - phi0));
    let b_2 = (r_ny - r_dc) - b_1;

    // Back from the φ-basis to coefficients. √B₀ = b₀+b₁+b₂ and
    // √(B₀+B₁+B₂) = b₀−b₁+b₂ (both positive: these shapes never invert),
    // which yields b₁ and the sum b₀+b₂; with the product b₀b₂ = B₂/16 the
    // last two are the roots of a quadratic. b₀ takes the larger root so
    // |b₂| < |b₀|, which puts the zeros inside the circle: minimum phase,
    // which for an EQ means the phase is exactly what the magnitude implies
    // and nothing extra. The radicand is clamped: at the far corners of the
    // range it can graze zero from below by rounding, and a NaN coefficient
    // is an infinite one by the time it reaches a feedback path.
    let sum = b_0.max(0.0).sqrt();
    let dif = r_ny.max(0.0).sqrt();
    let b1 = 0.5 * (sum - dif);
    let p = 0.5 * (sum + dif);
    let b0 = 0.5 * p + (0.25 * p * p - b_2 / 16.0).max(0.0).sqrt();
    let b2 = p - b0;
    BiquadCoeffs { b0, b1, b2, a1, a2 }
}

/// RBJ cookbook high-pass, low-pass and notch, at the node's Q. Bilinear,
/// so the corner is prewarped exact and the stopband cramps toward MORE
/// rejection — the direction nobody minds. See the module header for why
/// these three are not matched.
fn rbj(kind: NodeKind, w0: f64, q: f64) -> BiquadCoeffs {
    let (sn, cs) = w0.sin_cos();
    let alpha = sn / (2.0 * q);
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = match kind {
        NodeKind::HighPass => {
            let c = (1.0 + cs) / 2.0;
            (c, -2.0 * c, c)
        }
        NodeKind::LowPass => {
            let c = (1.0 - cs) / 2.0;
            (c, 2.0 * c, c)
        }
        NodeKind::Notch => (1.0, -2.0 * cs, 1.0),
        _ => unreachable!("the gain shapes go through the matched designer"),
    };
    BiquadCoeffs {
        b0: b0 / a0,
        b1: b1 / a0,
        b2: b2 / a0,
        a1: (-2.0 * cs) / a0,
        a2: (1.0 - alpha) / a0,
    }
}

// ── the curve for the display ───────────────────────────────────────────────

/// The exact digital magnitude of the whole cascade, in dB, at each asked
/// frequency — THE curve the display draws, produced by the same designer
/// the audio path runs so the two cannot drift apart. Notches genuinely
/// reach −∞ at centre; the floor at −120 dB keeps the painter out of
/// infinities without drawing anything a screen could distinguish.
pub fn response_db(nodes: &[Node], freqs_hz: &[f32], sample_rate: f32) -> Vec<f32> {
    let sr = if sample_rate.is_finite() && sample_rate >= 8_000.0 {
        f64::from(sample_rate)
    } else {
        44_100.0
    };
    let coeffs: Vec<BiquadCoeffs> = nodes.iter().map(|n| n.coeffs(sr)).collect();
    freqs_hz
        .iter()
        .map(|&f| {
            let w = (TAU * f64::from(f) / sr).clamp(0.0, PI);
            let mut db = 0.0;
            for c in &coeffs {
                db += 10.0 * c.mag2(w).max(1.0e-12).log10();
            }
            db as f32
        })
        .collect()
}

// ── knob space ──────────────────────────────────────────────────────────────

/// One band as the panel will hold it: everything 0..=1, mapped by the
/// consts at the top of this file. `on` is a switch rather than a knob
/// because a band out of circuit must cost nothing and ring nothing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NodeParams {
    pub on: bool,
    pub kind: NodeKind,
    /// Corner/centre across [`EQ_FREQ_HZ`], geometric.
    pub freq: f32,
    /// Cut to boost across ±[`EQ_GAIN_DB`]; 0.5 is exactly 0 dB.
    pub gain: f32,
    /// Width across [`EQ_Q`], geometric; [`EQ_Q_BUTTERWORTH`] is 1/√2.
    pub q: f32,
}

impl Default for NodeParams {
    fn default() -> Self {
        NodeParams {
            on: false,
            kind: NodeKind::Bell,
            freq: 0.5,
            gain: 0.5,
            q: EQ_Q_BUTTERWORTH,
        }
    }
}

impl NodeParams {
    /// Knob space to physical units. NaN lands on the neutral middle, same
    /// policy as `Params::sane` in the effects chain: a corrupt settings
    /// file must never reach a feedback path.
    pub fn node(&self) -> Node {
        let norm = |t: f32| if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.5 };
        let sweep = |range: (f32, f32), t: f32| range.0 * (range.1 / range.0).powf(norm(t));
        Node {
            kind: self.kind,
            freq_hz: sweep(EQ_FREQ_HZ, self.freq),
            gain_db: (norm(self.gain) * 2.0 - 1.0) * EQ_GAIN_DB,
            q: sweep(EQ_Q, self.q),
        }
    }
}

/// The whole panel. Default is the shipped sound: FLAT — every band off,
/// handles parked log-evenly across the width so the display opens looking
/// like the ruler it is, gains centred, widths at Butterworth.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EqParams {
    pub nodes: [NodeParams; MAX_NODES],
}

impl Default for EqParams {
    fn default() -> Self {
        let mut nodes = [NodeParams::default(); MAX_NODES];
        for (i, n) in nodes.iter_mut().enumerate() {
            n.freq = i as f32 / (MAX_NODES - 1) as f32;
        }
        EqParams { nodes }
    }
}

impl EqParams {
    /// The live bands in physical units, for the curve display. Allocates,
    /// so this is for the UI thread; the audio path maps slots in place.
    pub fn nodes(&self) -> Vec<Node> {
        self.nodes
            .iter()
            .filter(|n| n.on)
            .map(NodeParams::node)
            .collect()
    }
}

// ── the kernel ──────────────────────────────────────────────────────────────

/// One section's audio-rate coefficients. f32, per the house split: the
/// design math above is where precision buys accuracy; here it would buy
/// nanoseconds.
#[derive(Debug, Clone, Copy)]
struct Coef {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Coef {
    const WIRE: Coef = Coef {
        b0: 1.0,
        b1: 0.0,
        b2: 0.0,
        a1: 0.0,
        a2: 0.0,
    };

    fn from(c: &BiquadCoeffs) -> Coef {
        Coef {
            b0: c.b0 as f32,
            b1: c.b1 as f32,
            b2: c.b2 as f32,
            a1: c.a1 as f32,
            a2: c.a2 as f32,
        }
    }
}

/// One channel of one section: transposed direct form II, the form that
/// behaves when coefficients move under it — which here they do on any
/// block where a handle is being dragged.
#[derive(Debug, Clone, Copy, Default)]
struct State {
    z1: f32,
    z2: f32,
}

impl State {
    #[inline]
    fn run(&mut self, c: &Coef, x: f32) -> f32 {
        let y = c.b0 * x + self.z1;
        self.z1 = flush(c.b1 * x - c.a1 * y + self.z2);
        self.z2 = flush(c.b2 * x - c.a2 * y);
        y
    }
}

/// The EQ: up to [`MAX_NODES`] sections in series, two channels of state
/// each, coefficients redesigned only on the block where a band's knobs
/// actually moved.
pub struct EqKernel {
    sample_rate: f32,
    coef: [Coef; MAX_NODES],
    state: [[State; 2]; MAX_NODES],
    live: [bool; MAX_NODES],
    /// What each slot's coefficients were designed from, so an unmoved
    /// panel costs zero design work per block.
    cached: [NodeParams; MAX_NODES],
    designed: bool,
}

impl EqKernel {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = if sample_rate.is_finite() {
            sample_rate.max(8_000.0)
        } else {
            44_100.0
        };
        EqKernel {
            sample_rate,
            coef: [Coef::WIRE; MAX_NODES],
            state: [[State::default(); 2]; MAX_NODES],
            live: [false; MAX_NODES],
            cached: [NodeParams::default(); MAX_NODES],
            designed: false,
        }
    }

    /// In place on an interleaved buffer; `channels` is the stride. Stereo
    /// only, same contract as `Effects::process`: the first two channels are
    /// filtered, anything past them is untouched, and fewer than two is not
    /// a thing to guess about.
    ///
    /// Coefficients step once per block. The house per-sample knob slew
    /// would mean redesigning eight sections per sample — fifty-odd
    /// transcendentals each — which is the wrong trade. When a dragged
    /// handle audibly zippers, the fix is a coefficient CROSSFADE: run the
    /// old and new sections in parallel for one block and equal-power fade
    /// between them, paying one extra section only while a knob is actually
    /// moving. Deferred to the wiring pass; nothing here precludes it.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &EqParams,
    ) {
        if channels < 2 {
            return;
        }
        for i in 0..MAX_NODES {
            let want = params.nodes[i];
            if !self.designed || want != self.cached[i] {
                if want.on {
                    self.coef[i] = Coef::from(&want.node().coeffs(f64::from(self.sample_rate)));
                    if !self.cached[i].on {
                        // Arriving from off: whatever rang here under some
                        // earlier setting must not replay into the new one.
                        self.state[i] = [State::default(); 2];
                    }
                } else if self.cached[i].on {
                    // Leaving circuit: the tail must not sit in the delay
                    // registers waiting for the band to be switched back on
                    // half an hour later.
                    self.state[i] = [State::default(); 2];
                }
                self.cached[i] = want;
            }
            self.live[i] = want.on;
        }
        self.designed = true;

        for f in 0..frames {
            let at = f * channels;
            for ch in 0..2 {
                let mut x = buf[at + ch];
                for i in 0..MAX_NODES {
                    if self.live[i] {
                        x = self.state[i][ch].run(&self.coef[i], x);
                    }
                }
                buf[at + ch] = x;
            }
        }
    }

    /// Empty every section's delay registers. Coefficients stay: a cleared
    /// EQ is silent, not flat.
    pub fn clear(&mut self) {
        self.state = [[State::default(); 2]; MAX_NODES];
    }
}

// ── proof ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// The RBJ cookbook peaking EQ, bilinear — built here ONLY to hold up
    /// next to the matched section and show the cramping the module header
    /// claims. It must derive from the same analog prototype
    /// (`analog_mag2(Bell, ..)`) or the comparison compares nothing.
    fn rbj_bell(w0: f64, gain_db: f64, q: f64) -> BiquadCoeffs {
        let a = 10.0_f64.powf(gain_db / 40.0);
        let (sn, cs) = w0.sin_cos();
        let alpha = sn / (2.0 * q);
        let a0 = 1.0 + alpha / a;
        BiquadCoeffs {
            b0: (1.0 + alpha * a) / a0,
            b1: (-2.0 * cs) / a0,
            b2: (1.0 - alpha * a) / a0,
            a1: (-2.0 * cs) / a0,
            a2: (1.0 - alpha / a) / a0,
        }
    }

    /// Knob position that lands the geometric sweep on a wanted physical
    /// value — the tests speak hertz and dB, the kernel speaks 0..=1.
    fn t_of(range: (f32, f32), v: f32) -> f32 {
        (v / range.0).ln() / (range.1 / range.0).ln()
    }

    fn band(kind: NodeKind, hz: f32, gain_db: f32, q: f32) -> NodeParams {
        NodeParams {
            on: true,
            kind,
            freq: t_of(EQ_FREQ_HZ, hz),
            gain: gain_db / (2.0 * EQ_GAIN_DB) + 0.5,
            q: t_of(EQ_Q, q),
        }
    }

    /// A log-spaced frequency grid, endpoints included.
    fn grid(lo: f64, hi: f64, n: usize) -> Vec<f64> {
        (0..n)
            .map(|i| lo * (hi / lo).powf(i as f64 / (n - 1) as f64))
            .collect()
    }

    /// The house noise source: xorshift, uniform in ±amp.
    fn noise(seed: &mut u32, amp: f32) -> f32 {
        *seed ^= *seed << 13;
        *seed ^= *seed >> 17;
        *seed ^= *seed << 5;
        (*seed as f32 / u32::MAX as f32 - 0.5) * 2.0 * amp
    }

    #[test]
    fn a_matched_bell_tracks_the_analog_curve_where_rbj_visibly_cramps() {
        // The hostile case from the module header: +12 dB, Q=4, 10 kHz,
        // 44.1 kHz — high enough that the top of the skirt lives against
        // Nyquist, narrow enough that the fold is not hidden by width.
        let sr = 44_100.0_f64;
        let f0 = 10_000.0_f64;
        let (gain_db, q) = (12.0_f64, 4.0_f64);
        let a = 10.0_f64.powf(gain_db / 40.0);
        let node = Node {
            kind: NodeKind::Bell,
            freq_hz: f0 as f32,
            gain_db: gain_db as f32,
            q: q as f32,
        };
        let matched = node.coeffs(sr);
        let rbj = rbj_bell(TAU * f0 / sr, gain_db, q);

        let mut worst_matched = 0.0_f64;
        let mut worst_rbj = 0.0_f64;
        for f in grid(200.0, 0.45 * sr, 400) {
            let w = TAU * f / sr;
            let analog = 10.0 * analog_mag2(NodeKind::Bell, f / f0, a, q).log10();
            let m = 10.0 * matched.mag2(w).log10() - analog;
            let r = 10.0 * rbj.mag2(w).log10() - analog;
            worst_matched = worst_matched.max(m.abs());
            worst_rbj = worst_rbj.max(r.abs());
        }
        assert!(
            worst_matched <= 0.35,
            "matched bell strays {worst_matched:.3} dB from the analog prototype"
        );
        assert!(
            worst_rbj > 1.0,
            "RBJ only strayed {worst_rbj:.3} dB — the hostile case is not hostile"
        );
    }

    #[test]
    fn the_default_settings_pass_audio_bit_for_bit() {
        let mut k = EqKernel::new(48_000.0);
        let mut seed = 0x1234_5678_u32;
        let frames = 2_048;
        let mut buf: Vec<f32> = (0..frames * 2).map(|_| noise(&mut seed, 0.5)).collect();
        let before = buf.clone();

        // Flat two ways: every band off (the shipped default), and every
        // band ON at 0 dB — the gain shapes collapse to a wire exactly, so
        // both must be the identity down to the bit. HP/LP/notch have no
        // 0 dB to collapse to and are deliberately absent from the second
        // half.
        k.process(&mut buf, frames, 2, &EqParams::default());
        assert_eq!(buf, before, "all-off is not a bit-exact null");

        let mut p = EqParams::default();
        for (i, n) in p.nodes.iter_mut().enumerate() {
            n.on = true;
            n.kind = [NodeKind::Bell, NodeKind::LowShelf, NodeKind::HighShelf][i % 3];
            n.gain = 0.5;
        }
        k.process(&mut buf, frames, 2, &p);
        assert_eq!(buf, before, "eight live bands at 0 dB are not a wire");
    }

    #[test]
    fn no_setting_the_panel_can_reach_makes_it_explode() {
        // Every shape at every corner of every knob, including both band
        // edges, Q from 0.1 to 24 and the full ±24 dB — plus a NaN, which a
        // hand-edited settings file can always supply. Amplitude in is
        // 0.05; the largest honest single-band gain is a Q=24 SHELF at full
        // boost, whose pole-pair resonance rides on top of the shelf's own
        // step — +52.8 dB by direct evaluation of |H| — so the cap sits at
        // +62 dB over the input. Past that is not equalization, it is a
        // pole outside the circle; a genuinely unstable section blows
        // through the finite check within a block anyway.
        for sr in [44_100.0_f32, 96_000.0] {
            for kind in NodeKind::ALL {
                for freq in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
                    for gain in [0.0_f32, 0.5, 1.0] {
                        for q in [0.0_f32, 0.5, 1.0] {
                            let mut p = EqParams::default();
                            p.nodes[0] = NodeParams {
                                on: true,
                                kind,
                                freq,
                                gain,
                                q,
                            };
                            let mut k = EqKernel::new(sr);
                            let mut seed = 0x9E37_79B9_u32;
                            for _ in 0..2 {
                                let frames = 512;
                                let mut buf: Vec<f32> =
                                    (0..frames * 2).map(|_| noise(&mut seed, 0.05)).collect();
                                k.process(&mut buf, frames, 2, &p);
                                for v in &buf {
                                    assert!(
                                        v.is_finite() && v.abs() <= 64.0,
                                        "{kind:?} f={freq} g={gain} q={q} at {sr} Hz \
                                         produced {v}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }

        // The corrupt-file case: NaN in every knob must design to something
        // stable, per the sane() policy in NodeParams::node.
        let mut p = EqParams::default();
        p.nodes[0] = NodeParams {
            on: true,
            kind: NodeKind::Bell,
            freq: f32::NAN,
            gain: f32::NAN,
            q: f32::NAN,
        };
        let mut k = EqKernel::new(48_000.0);
        let mut buf = vec![0.25_f32; 1_024];
        k.process(&mut buf, 512, 2, &p);
        assert!(buf.iter().all(|v| v.is_finite()), "NaN knobs reached the audio");

        // All eight bands at once, boosts and cuts alternating at full
        // reach and full Q. This is not a sound anybody should make; the
        // assertion is stability, not taste — bounded output from a bounded
        // input, forever finite.
        let mut p = EqParams::default();
        let kinds = [
            NodeKind::HighPass,
            NodeKind::Bell,
            NodeKind::Bell,
            NodeKind::Notch,
            NodeKind::Bell,
            NodeKind::Bell,
            NodeKind::LowShelf,
            NodeKind::HighShelf,
        ];
        for (i, n) in p.nodes.iter_mut().enumerate() {
            n.on = true;
            n.kind = kinds[i];
            n.gain = if i % 2 == 0 { 1.0 } else { 0.0 };
            n.q = 1.0;
        }
        let mut k = EqKernel::new(48_000.0);
        let mut seed = 0x0BAD_CAFE_u32;
        for _ in 0..8 {
            let frames = 512;
            let mut buf: Vec<f32> = (0..frames * 2).map(|_| noise(&mut seed, 0.05)).collect();
            k.process(&mut buf, frames, 2, &p);
            for v in &buf {
                assert!(v.is_finite() && v.abs() < 256.0, "the stacked panel diverged: {v}");
            }
        }
    }

    #[test]
    fn the_curve_reads_the_same_at_every_sample_rate() {
        // The matched shapes' whole promise: the drawn curve belongs to the
        // node, not to the stream it happens to run in. RBJ high/low-pass
        // keep their bilinear stopband cramping BY DESIGN, so they appear
        // here only where the audible band leaves them identical: a
        // high-pass at the bottom of the band and a notch in the middle.
        let sets: [&[Node]; 3] = [
            &[
                Node { kind: NodeKind::Bell, freq_hz: 10_000.0, gain_db: 12.0, q: 4.0 },
                Node { kind: NodeKind::Bell, freq_hz: 200.0, gain_db: 6.0, q: 1.0 },
            ],
            &[
                Node { kind: NodeKind::LowShelf, freq_hz: 120.0, gain_db: 9.0, q: 0.9 },
                Node { kind: NodeKind::HighShelf, freq_hz: 8_000.0, gain_db: -6.0, q: 0.8 },
            ],
            &[
                Node { kind: NodeKind::HighPass, freq_hz: 60.0, gain_db: 0.0, q: 0.707 },
                Node { kind: NodeKind::Notch, freq_hz: 1_000.0, gain_db: 0.0, q: 8.0 },
                Node { kind: NodeKind::Bell, freq_hz: 3_000.0, gain_db: -9.0, q: 2.5 },
            ],
        ];
        let freqs: Vec<f32> = grid(40.0, 15_000.0, 160).iter().map(|&f| f as f32).collect();
        for (si, set) in sets.iter().enumerate() {
            let r44 = response_db(set, &freqs, 44_100.0);
            let r48 = response_db(set, &freqs, 48_000.0);
            let r96 = response_db(set, &freqs, 96_000.0);
            for i in 0..freqs.len() {
                for (other, name) in [(r48[i], "48k"), (r96[i], "96k")] {
                    let d = (r44[i] - other).abs();
                    assert!(
                        d <= 0.25,
                        "set {si} at {:.0} Hz: 44.1k and {name} disagree by {d:.3} dB",
                        freqs[i]
                    );
                }
            }
        }
    }

    /// Push a steady sine through the kernel and read its gain by
    /// projection onto the quadrature pair at the same frequency — with the
    /// frequency snapped to a whole number of cycles per window, the
    /// projection is an exact single-bin DFT and there is no leakage to
    /// argue with. Returns (frequency actually used, measured dB).
    fn sine_through(np: NodeParams, hz: f64, sr: f32) -> (f32, f64) {
        let n = 16_384_usize;
        let srf = f64::from(sr);
        let f = (hz * n as f64 / srf).round().max(1.0) * srf / n as f64;
        let step = TAU * f / srf;
        let amp = 0.25_f64;

        let mut k = EqKernel::new(sr);
        let mut p = EqParams::default();
        p.nodes[0] = np;
        // Two windows of warm-up: the narrowest band under test settles in
        // milliseconds, this is hundreds of them.
        let total = n * 3;
        let mut buf = vec![0.0_f32; total * 2];
        for i in 0..total {
            let s = ((step * i as f64).sin() * amp) as f32;
            buf[i * 2] = s;
            buf[i * 2 + 1] = s;
        }
        k.process(&mut buf, total, 2, &p);

        let (mut re, mut im) = (0.0_f64, 0.0_f64);
        for i in total - n..total {
            let ph = step * i as f64;
            let v = f64::from(buf[i * 2]);
            re += v * ph.cos();
            im += v * ph.sin();
        }
        let out = 2.0 * (re * re + im * im).sqrt() / n as f64;
        (f as f32, 20.0 * (out / amp).log10())
    }

    #[test]
    fn the_curve_the_display_draws_is_the_gain_the_audio_gets() {
        // Several (band, probe frequency) pairs, spread across shapes and
        // across the band: the drawn curve and a measured sine must agree.
        // This is the whole point of sharing one designer between the two
        // paths, and it also bounds what the f32 cast of the coefficients
        // costs where it is priced.
        let cases: [(NodeParams, f64); 6] = [
            (band(NodeKind::Bell, 1_000.0, 6.0, 1.0), 1_000.0),
            (band(NodeKind::Bell, 3_000.0, -12.0, 4.0), 3_000.0),
            (band(NodeKind::LowShelf, 150.0, 9.0, 0.707), 90.0),
            (band(NodeKind::HighShelf, 8_000.0, -6.0, 0.707), 12_000.0),
            (band(NodeKind::LowPass, 2_000.0, 0.0, 0.707), 4_000.0),
            (band(NodeKind::HighPass, 100.0, 0.0, 2.0), 100.0),
        ];
        for (np, hz) in cases {
            let sr = 48_000.0_f32;
            let (f, measured) = sine_through(np, hz, sr);
            let drawn = f64::from(response_db(&[np.node()], &[f], sr)[0]);
            let d = (measured - drawn).abs();
            assert!(
                d <= 0.2,
                "{:?} probed at {f:.0} Hz: display says {drawn:.3} dB, audio measures \
                 {measured:.3} dB",
                np.kind
            );
        }
    }

    #[test]
    fn clear_leaves_no_tail_behind() {
        // Ring a resonant band hard, clear, and the very next silent block
        // must come back all zeros — not decaying, GONE.
        let mut k = EqKernel::new(48_000.0);
        let mut p = EqParams::default();
        p.nodes[0] = band(NodeKind::Bell, 1_000.0, 24.0, 24.0);
        let mut seed = 0xDEAD_BEEF_u32;
        let frames = 1_024;
        let mut buf: Vec<f32> = (0..frames * 2).map(|_| noise(&mut seed, 0.25)).collect();
        k.process(&mut buf, frames, 2, &p);
        k.clear();
        let mut silent = vec![0.0_f32; frames * 2];
        k.process(&mut silent, frames, 2, &p);
        assert!(silent.iter().all(|&v| v == 0.0), "a cleared EQ still rang");
    }

    #[test]
    fn the_q_knobs_resting_place_is_butterworth() {
        let q = NodeParams { q: EQ_Q_BUTTERWORTH, ..NodeParams::default() }.node().q;
        assert!(
            (q - std::f32::consts::FRAC_1_SQRT_2).abs() < 1.0e-3,
            "the documented default position maps to Q = {q}, not 1/√2"
        );
    }

    #[test]
    fn every_kind_survives_the_settings_round_trip() {
        for kind in NodeKind::ALL {
            assert_eq!(NodeKind::from_key(kind.key()), Some(kind));
        }
    }

    #[test]
    fn a_third_channel_is_left_alone() {
        // Same contract as Effects::process: stride past the stereo pair
        // belongs to somebody else.
        let mut k = EqKernel::new(48_000.0);
        let mut p = EqParams::default();
        p.nodes[0] = band(NodeKind::Bell, 1_000.0, 12.0, 2.0);
        let frames = 256;
        let mut buf = vec![0.0_f32; frames * 3];
        for f in 0..frames {
            buf[f * 3] = 0.5;
            buf[f * 3 + 1] = 0.5;
            buf[f * 3 + 2] = 0.5;
        }
        k.process(&mut buf, frames, 3, &p);
        assert!(
            (0..frames).all(|f| buf[f * 3 + 2] == 0.5),
            "the third channel was touched"
        );
    }
}
