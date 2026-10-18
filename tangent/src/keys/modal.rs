//! The mode bank: a few hundred two-pole resonators, rendered a block at a
//! time, which is what a struck string IS once it is written down as the
//! sum of its modes.
//!
//! **Why a bank and not a waveguide.** `string.rs` folds a whole string into
//! one delay line, and that is exact for the thing a delay line is good at
//! (a harmonic series) and a compromise for everything a grand does on top
//! of it. Its inharmonicity lives in an all-pass cascade that is capped
//! where the cascade stops being a fit, so the bass, whose B is small, and
//! the treble, whose B is large, cannot both be right; its two-stage decay
//! is three loops given three different lives, not three strings sharing a
//! bridge. A mode bank has neither limit: every partial of every string is
//! a resonator at exactly the frequency and the decay the physics hands it,
//! so inharmonicity, the stretch, the double decay and the beating are
//! numbers placed where they belong rather than shapes a filter can or
//! cannot make. It costs what it costs — one resonator per partial per
//! string — and this file exists to make that cost small.
//!
//! **The resonator.** Each mode is the real part of a complex one-pole,
//! `z[n+1] = p z[n] + G F[n]`, `p = r e^{iw}`, `r = exp(-sigma / fs)`,
//! `w = 2 pi f / fs`. Its real part obeys the familiar two-pole recursion
//! `y[n+1] = c1 y[n] + c2 y[n-1] + bi0 F[n] + bi1 F[n-1]` (`c1 = 2 r cos w`,
//! `c2 = -r^2`, `bi0 = Re G`, `bi1 = -Re(G p*)`), so a real force drives the
//! mode with an amplitude AND a phase and `y` is exactly `Re z` at every
//! sample, forcing or not.
//!
//! **Not in that form, though: in its DIFFERENCE form.** In `f32` the
//! direct form cannot hold the numbers this instrument needs. A 27.5 Hz
//! mode at 48 kHz has `c1 = 2 - 1.3e-5`, and `f32`'s spacing at 2 is 1.2e-7:
//! the frequency lives in the fifth significant digit of a seven-digit
//! number, so it lands up to 4 cents from where it was put — more than the
//! whole unison detune it was supposed to carry. A 70-second T60 has
//! `c2 = -(1 - 4.8e-6)` and loses 1.3 % of its decay the same way. So the
//! bank keeps `y[n]` and the difference `v[n] = y[n] - y[n-1]`, and steps
//! `v += b y - d2 v; y += v` with `b = -|1 - p|^2` and `d2 = 1 - r^2`: the
//! same recursion, exactly (`b = c1 + c2 - 1`, `-c2 = 1 - d2`), with both
//! coefficients SMALL numbers that `f32` carries to full relative
//! precision. Measured (`a_single_mode_rings_where_it_was_put`): 27.5 Hz to
//! 1e-8 relative, a 70 s T60 to 4e-5. It costs two adds a mode-sample over
//! the direct form, about a sixth of the loop, and it is the price of
//! tuning the bass.
//!
//! **What the state is: the mode's MID output.** Every mode is read four
//! ways — the two microphones, the bridge drive for the shared body and
//! harp, and, while the hammer touches, the displacement under the felt —
//! and each reading is `Re(weight * z)` for its own complex weight. Two
//! taps on the state realise any complex weight exactly (the derivation is
//! at `taps`). The state could be any one of those readings; it is the mid
//! channel, `(L + R) / 2`, for a measured reason: with the mid as the state
//! the mid output is the state itself (an add, no multiply), the side
//! `(L - R) / 2` is the one two-tap reading left in the hot loop, and the
//! coefficients of two groups of eight then fit the register file at once.
//! Measured on this machine (`tests::timing_of_the_bank` and the lane's
//! microbench, best of five, same minute): this loop, difference form and
//! three outputs, 5.4 G mode-samples per second on one core; the obvious
//! direct-form loop with two taps per channel, 4.9; the direct form with
//! the mid as state, 6.5 — the mid basis buys back what the difference
//! form costs, and both together are what this instrument's CPU budget was
//! met with. Under a hammer the bank runs a block at a time too, by
//! superposition (`render_contact`: a free run, the probe's response to a
//! unit force, and the forced run), at 1.4 to 1.5 G mode-samples a second
//! where stepping every mode a sample at a time ran 0.6 to 0.7. The drive
//! is read with ONE tap, its amplitude only:
//! the body and the harp take what a partial puts into the bridge, and the
//! phase each microphone adds on its way out of the board is not theirs to
//! hear.
//!
//! **Layout.** Structure of arrays, eight lanes per group (`[[f32; 8]; G]`),
//! dense from index zero: `count` modes live in the first
//! `count.div_ceil(8)` groups and every lane past the end is zero in every
//! array, so a partly filled group computes zeros and needs no tail loop.
//! The render runs a group PAIR at a time over the whole block, states and
//! coefficients in registers, accumulating per lane into `[[f32; 8]; 128]`
//! scratch that is reduced once at the end — not a horizontal sum per
//! sample. Plain auto-vectorised `std`; no intrinsics, and no `mul_add`,
//! which is a libm CALL on baseline x86, where the app also ships.
//!
//! **Capacity.** `MAX_MODES` is 768, the brief's floor. The largest voice
//! the grand asks for is a bass bichord: two strings for every partial
//! under its count cap, and the phantom series on top — `grand.rs` measures
//! the most any key uses (`the_largest_voice_fits_the_bank`: 223 modes,
//! phantoms in), so three quarters of the bank is room for a
//! larger partial budget, or a monochord's second polarisation on every
//! partial, without a re-size. The bank is 20
//! words per mode, 60 KB a voice, 4.1 MB for the pool of sixty-four and its
//! corpses — the order of what the pool already carries for the waveguide
//! piano.

/// Lanes per group: one group is two 128-bit vectors, eight `f32`.
pub const LANES: usize = 8;

/// The most modes one bank holds. See the module doc for the sizing.
pub const MAX_MODES: usize = 768;

/// Groups of eight. Even, so the render's group pairs never run off the end.
const GROUPS: usize = MAX_MODES / LANES;

/// The longest block `render` takes in one call. The engine's callback block
/// is 128 frames; a caller with a longer one calls twice.
pub const BLOCK: usize = 128;

/// The cull floor: a mode whose loudest reading (either microphone, or the
/// bridge drive) has fallen below this is removed.
///
/// **-120 dB re full scale, and why that is inaudible.** Removing a mode is
/// a step of at most its own amplitude in each output. 1e-6 is 26 dB under
/// the line at which the frame already declares a waveguide piano voice
/// finished and clears it outright (a peak follower at 2e-5, and a peak
/// follower reads the SUM of the modes, each of which is smaller still),
/// 24 dB under 16-bit dither, and 20 dB under the floor `symp.rs` puts its
/// harp strings to sleep at. Removals do not arrive together — every mode
/// crosses the floor at its own time, on its own decay — and `grand.rs`
/// measures the worst block of a fortissimo render
/// (`culling_never_steps_more_than_the_floor`). It was -100 first, and
/// measured: an A0 fundamental, which the board radiates 35 dB under its
/// own partials, crossed -100 dBFS five seconds into a mezzo note and was
/// removed while its partials rang on — inaudibly, but the decay table read
/// the cut as a 5.6 s late T60. Twenty decibels more costs 6 % of the
/// pedalled stress render.
pub const CULL_FLOOR: f32 = 1.0e-6;

/// States smaller than this are flushed to zero once per block. A decaying
/// resonator left alone walks down into the subnormals, where some FPUs
/// take a trap per operation; 1e-20 is 400 dB under full scale and
/// nothing above it is touched.
const TINY: f32 = 1.0e-20;

type Lane = [f32; LANES];

/// A complex number, in `f64`, for the arithmetic that happens once per
/// strike: the coupled-string eigenproblem, the radiation weights, and the
/// taps derived from them. The render loop never sees one.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct C64 {
    pub re: f64,
    pub im: f64,
}

impl C64 {
    pub const ZERO: C64 = C64 { re: 0.0, im: 0.0 };
    pub const ONE: C64 = C64 { re: 1.0, im: 0.0 };

    pub const fn new(re: f64, im: f64) -> C64 {
        C64 { re, im }
    }

    /// `m e^{i a}`.
    pub fn polar(m: f64, a: f64) -> C64 {
        C64::new(m * a.cos(), m * a.sin())
    }

    pub fn conj(self) -> C64 {
        C64::new(self.re, -self.im)
    }

    pub fn norm2(self) -> f64 {
        self.re * self.re + self.im * self.im
    }

    /// The modulus, as the root of the squared one: `hypot`'s guard against
    /// overflow is a libm call several times the price, for magnitudes
    /// (1e-150 to 1e150) nothing in a strike comes near.
    pub fn abs(self) -> f64 {
        self.norm2().sqrt()
    }

    pub fn scale(self, k: f64) -> C64 {
        C64::new(self.re * k, self.im * k)
    }

    /// The principal square root.
    pub fn sqrt(self) -> C64 {
        let m = self.abs();
        if m == 0.0 {
            return C64::ZERO;
        }
        let re = ((m + self.re) * 0.5).max(0.0).sqrt();
        let im = ((m - self.re) * 0.5).max(0.0).sqrt();
        C64::new(re, if self.im < 0.0 { -im } else { im })
    }

    pub fn recip(self) -> C64 {
        let d = self.norm2();
        C64::new(self.re / d, -self.im / d)
    }
}

impl std::ops::Add for C64 {
    type Output = C64;
    fn add(self, o: C64) -> C64 {
        C64::new(self.re + o.re, self.im + o.im)
    }
}

impl std::ops::Sub for C64 {
    type Output = C64;
    fn sub(self, o: C64) -> C64 {
        C64::new(self.re - o.re, self.im - o.im)
    }
}

impl std::ops::Mul for C64 {
    type Output = C64;
    fn mul(self, o: C64) -> C64 {
        C64::new(self.re * o.re - self.im * o.im, self.re * o.im + self.im * o.re)
    }
}

impl std::ops::Div for C64 {
    type Output = C64;
    fn div(self, o: C64) -> C64 {
        self * o.recip()
    }
}

impl std::ops::Neg for C64 {
    type Output = C64;
    fn neg(self) -> C64 {
        C64::new(-self.re, -self.im)
    }
}

/// One mode as the physics hands it over: a complex coordinate `z` (the
/// caller's own unit — the grand uses the mode's share of the bridge-sum
/// displacement, metres) that obeys `dz/dt = (-sigma + i 2 pi f) z +
/// force * F(t)`, and the complex weights each reading takes of it.
#[derive(Clone, Copy, Debug, Default)]
pub struct ModeSpec {
    /// Frequency, Hz. Below `0.49 fs`, or `add` refuses it.
    pub freq: f64,
    /// Decay rate, nepers per second: the amplitude falls as `e^{-sigma t}`,
    /// so the T60 is `ln(1000) / sigma`.
    pub sigma: f64,
    /// `dz/dt` per newton of contact force.
    pub force: C64,
    /// The displacement under the contact is `Re(probe * z)`, metres.
    pub probe: C64,
    /// The left and right microphone readings, `Re(left * z)` and
    /// `Re(right * z)`, in output units.
    pub left: C64,
    pub right: C64,
    /// What this mode puts into the bridge, as an amplitude in output
    /// units per unit of `|z|`: the drive the shared body and harp hear.
    /// See the module doc for why it carries no phase.
    pub drive: f64,
    /// The coordinate at the moment the mode is added. Zero for a mode a
    /// hammer is about to excite; the product of its parents' coordinates
    /// for a phantom.
    pub start: C64,
    /// The decay rate a fully pressed damper ADDS, nepers per second.
    pub damp: f64,
    /// The caller's label, carried through culling untouched.
    pub tag: u32,
}

/// One mode as the bank holds it AT REST: every coefficient `write` derives
/// from a `ModeSpec`, and nothing of its motion. A bank cleared and loaded
/// from the images of another bank's modes (`Bank::load`) is the bank those
/// specs would have built at rest, to the bit, without deriving any of it
/// again: what `grand::GrandDesigns` keeps for every key, so that a fresh
/// strike copies its strings instead of solving them.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModeImage {
    b: f32,
    d2: f32,
    h0: f32,
    h1: f32,
    gd: f32,
    bi0: f32,
    bi1: f32,
    q0: f32,
    q1: f32,
    cw: f32,
    sw: f32,
    /// The free decay (the damper's share is the bank's, not the mode's).
    sigma: f32,
    damp: f32,
    wgt: f32,
    mid_re: f32,
    mid_im: f32,
    tag: u32,
}

/// A contact block in progress (`Bank::begin_contact`): the displacement
/// under the contact with no new force, read for the whole block when it
/// began, what the forces solved so far add to it, the block's length and
/// how far it has run. Kept by the caller between the parts it renders the
/// block in, so that a block a host's call cuts in two is solved as it would
/// have been whole: solved afresh at the cut, the second part's free run
/// starts from states the first part's forces moved, and sums the same
/// forces in another order (the electric grand, which solves its contact a
/// part at a time, -105 to -113 dB re peak off a whole block's take).
pub struct Contact {
    u_free: [f32; BLOCK + 1],
    owed: [f32; BLOCK + 1],
    len: usize,
    at: usize,
}

impl Default for Contact {
    fn default() -> Self {
        Contact { u_free: [0.0; BLOCK + 1], owed: [0.0; BLOCK + 1], len: 0, at: 0 }
    }
}

/// The bank. Allocated whole in `new`; nothing after that allocates.
///
/// `Clone` is for the tests that render a culled bank beside an unculled
/// twin; the instrument never clones one.
#[derive(Clone)]
pub struct Bank {
    rate: f32,
    count: usize,
    // ── hot: read every sample ──
    /// The mid reading `Re(z_mid[n])`.
    y: Box<[Lane; GROUPS]>,
    /// `y[n] - y[n-1]`.
    v: Box<[Lane; GROUPS]>,
    /// `-|1 - p|^2`.
    b: Box<[Lane; GROUPS]>,
    /// `1 - r^2`.
    d2: Box<[Lane; GROUPS]>,
    /// The side reading's taps on `y` and `v`.
    h0: Box<[Lane; GROUPS]>,
    h1: Box<[Lane; GROUPS]>,
    /// The drive's one tap.
    gd: Box<[Lane; GROUPS]>,
    // ── contact only: read while a force acts ──
    bi0: Box<[Lane; GROUPS]>,
    bi1: Box<[Lane; GROUPS]>,
    q0: Box<[Lane; GROUPS]>,
    q1: Box<[Lane; GROUPS]>,
    // ── cold: per mode, read at a block boundary or a strike ──
    cw: Box<[f32; MAX_MODES]>,
    sw: Box<[f32; MAX_MODES]>,
    sigma: Box<[f32; MAX_MODES]>,
    sigma_free: Box<[f32; MAX_MODES]>,
    damp: Box<[f32; MAX_MODES]>,
    /// `(loudest reading per unit of |z_mid|)^2 / sin^2 w`: what turns the
    /// state's energy invariant into the loudest reading's amplitude
    /// squared, precomputed for the cull.
    wgt: Box<[f32; MAX_MODES]>,
    /// The mid weight, so the raw coordinate can be recovered from the
    /// state (`coordinate`).
    mid_re: Box<[f32; MAX_MODES]>,
    mid_im: Box<[f32; MAX_MODES]>,
    tag: Box<[u32; MAX_MODES]>,
    /// The contact force of the previous sample: the `bi1 F[n-1]` term, and
    /// the readings' one-sample correction while a force is acting.
    f_prev: f32,
    /// What the damper is doing now, 0 free to 1 fully pressed.
    damper: f32,
    /// The level the last cull measured: `sqrt(sum of each mode's loudest
    /// reading squared)`.
    level: f32,
    /// What the last cull took: the sum of the removed modes' loudest
    /// readings, the most the output can have stepped by.
    removed: f32,
    /// What the last cull left: the sum of the remaining modes' loudest
    /// readings (`bound`).
    bound: f32,
    /// The probe's response to a unit force, `BLOCK + 1` samples of it
    /// (`probe_impulse`), and whether it is still the bank's: it depends on
    /// nothing but the coefficients, so a contact that spans blocks reads
    /// it once, and anything that changes a coefficient clears the flag.
    impulse: [f32; BLOCK + 1],
    impulse_ok: bool,
    /// Every state is zero (cleared, and only modes at rest added since):
    /// a fresh strike's first contact block, whose free run is nothing.
    quiet: bool,
}

/// The exact two-tap reading of `Re(w z)` from the state, in the
/// difference basis.
///
/// With `z[m] = x + i u` and `y[m] = x`, a free step `z[m] = p z[m-1]` gives
/// `y[m-1] = Re(z[m] / p) = (x cos w + u sin w) / r`, so
/// `u = (r y[m-1] - x cos w) / sin w`, and
/// `Re(w z) = Re(w) x - Im(w) u = g0 y[m] + g1 y[m-1]` with
/// `g0 = Re(w) + Im(w) cot w`, `g1 = -Im(w) r / sin w`. In the bank's basis
/// `y[m-1] = y - v`, so the taps are `h0 = g0 + g1` on `y` and `h1 = -g1` on
/// `v`; `h0` is formed as `Re(w) + Im(w) (cos w - r) / sin w` with the small
/// difference computed directly, which keeps it accurate at 27 Hz where
/// `g0` and `g1` are each 300 times `|w|` and nearly cancel. While a force
/// acts, `z[m] = p z[m-1] + G F[m-1]` adds `g1 Re(G / p) F[m-1] =
/// h1 bi1 / r^2 * F[m-1]` to the reading: `render_contact` adds that, summed
/// over the bank, as one term per sample.
fn taps(w: C64, sin_w: f64, one_minus_cos: f64, one_minus_r: f64, r: f64) -> (f64, f64) {
    // cos w - r = (1 - r) - (1 - cos w), the second written without the
    // cancellation (`2 sin^2(w / 2)`, from the caller).
    let cos_minus_r = one_minus_r - one_minus_cos;
    (w.re + w.im * cos_minus_r / sin_w, w.im * r / sin_w)
}

impl Bank {
    pub fn new(sample_rate: f32) -> Self {
        let z = || Box::new([[0.0_f32; LANES]; GROUPS]);
        let zm = || Box::new([0.0_f32; MAX_MODES]);
        Bank {
            rate: if sample_rate > 0.0 { sample_rate } else { 48_000.0 },
            count: 0,
            y: z(),
            v: z(),
            b: z(),
            d2: z(),
            h0: z(),
            h1: z(),
            gd: z(),
            bi0: z(),
            bi1: z(),
            q0: z(),
            q1: z(),
            cw: zm(),
            sw: zm(),
            sigma: zm(),
            sigma_free: zm(),
            damp: zm(),
            wgt: zm(),
            mid_re: zm(),
            mid_im: zm(),
            tag: Box::new([0; MAX_MODES]),
            f_prev: 0.0,
            damper: 0.0,
            level: 0.0,
            removed: 0.0,
            bound: 0.0,
            impulse: [0.0; BLOCK + 1],
            impulse_ok: false,
            quiet: true,
        }
    }

    /// How many modes are ringing.
    pub fn count(&self) -> usize {
        self.count
    }

    /// Every mode gone, every state zero: what a voice is before its first
    /// strike. Zeroes only the groups that were in use.
    pub fn clear(&mut self) {
        let used = self.count.div_ceil(LANES);
        for arr in [
            &mut self.y,
            &mut self.v,
            &mut self.b,
            &mut self.d2,
            &mut self.h0,
            &mut self.h1,
            &mut self.gd,
            &mut self.bi0,
            &mut self.bi1,
            &mut self.q0,
            &mut self.q1,
        ] {
            for g in arr.iter_mut().take(used) {
                *g = [0.0; LANES];
            }
        }
        self.count = 0;
        self.f_prev = 0.0;
        self.damper = 0.0;
        self.level = 0.0;
        self.bound = 0.0;
        self.impulse_ok = false;
        self.quiet = true;
    }

    /// Add one mode. Returns false, and changes nothing, when the bank is
    /// full or the request cannot be a resonator: a frequency outside
    /// `(0, 0.49 fs)`, a decay or a weight that is not finite, or no
    /// reading at all.
    pub fn add(&mut self, m: &ModeSpec) -> bool {
        let k = self.count;
        if k >= MAX_MODES || !self.write(k, m, m.start) {
            return false;
        }
        self.count = k + 1;
        true
    }

    /// Re-make ringing mode `k` from `m` — a new frequency, decay, input
    /// and readings — WITHOUT touching what it is doing: its complex
    /// coordinate `z` is read off the state (`coordinate`) and written back
    /// into the new resonator, so at this sample it is where it was and
    /// moving as it was, and only its future follows the new spec.
    /// `m.start` is ignored. False, and the mode untouched, when `m` could
    /// not be added.
    ///
    /// What a knob moved between two strikes of a ringing key does to it:
    /// the strings did not stop because DECAY moved, and the modes the
    /// physics now asks for take over the motion the old ones had. The
    /// output steps only by what the readings themselves changed by, which
    /// for a mode whose weights are kept is nothing at all
    /// (`a_respec_keeps_the_coordinate_and_takes_the_new_mode`).
    pub fn respec(&mut self, k: usize, m: &ModeSpec) -> bool {
        if k >= self.count {
            return false;
        }
        let z = self.coordinate(k);
        self.write(k, m, z)
    }

    /// `respec`, to the coordinate `z` rather than the mode's own: the
    /// motion of several ringing modes that a landing's new solve shares
    /// out among new ones (`grand::GrandVoice::build`), each slot taking
    /// its part, exactly as `add` would start it (the last force's term
    /// included). False, and the mode untouched, as `respec`.
    pub fn respec_to(&mut self, k: usize, m: &ModeSpec, z: C64) -> bool {
        if k >= self.count {
            return false;
        }
        self.write(k, m, z)
    }

    /// Write slot `k` as mode `m` at coordinate `start`, every array at
    /// once. False, and nothing written, if `m` cannot be a resonator.
    fn write(&mut self, k: usize, m: &ModeSpec, start: C64) -> bool {
        let fs = f64::from(self.rate);
        let w = std::f64::consts::TAU * m.freq / fs;
        if !(w > 0.0 && w < 0.98 * std::f64::consts::PI) || !m.sigma.is_finite() {
            return false;
        }
        // Every weight finite, one at a time: `f64::max` drops a NaN, so the
        // loudness test below would let a NaN reading through (a NaN WIDTH
        // knob did exactly that, 640 non-finite samples a strike).
        let finite = |z: C64| z.re.is_finite() && z.im.is_finite();
        if ![m.force, m.probe, m.left, m.right, start].into_iter().all(finite) || !m.drive.is_finite() || !m.damp.is_finite()
        {
            return false;
        }
        let sigma = m.sigma.max(0.0);
        let one_minus_r = -(-sigma / fs).exp_m1();
        let r = 1.0 - one_minus_r;
        let (sin_w, cos_w) = w.sin_cos();
        let p = C64::polar(r, w);
        // The state is the mid reading. A mode the two microphones hear in
        // exact antiphase has no mid, and a state of nothing cannot carry
        // the side; the grand's radiation keeps its phase spread well
        // inside that, and this nudge is the backstop: a millionth of the
        // loudest reading, along the left microphone's own phase.
        let loud = m.left.abs().max(m.right.abs()).max(m.drive.abs());
        if !(loud > 0.0) || !loud.is_finite() {
            return false;
        }
        let mut mid = (m.left + m.right).scale(0.5);
        if mid.abs() < 1.0e-6 * loud {
            let dir = if m.left.abs() > 0.0 {
                m.left.scale(1.0 / m.left.abs())
            } else {
                C64::ONE
            };
            mid = dir.scale(1.0e-6 * loud);
        }
        let side = (m.left - m.right).scale(0.5) / mid;
        let probe = m.probe / mid;
        // Zero-order hold: a force held over one sample moves `z` by
        // `force * (p - 1) / lambda`, the exact integral of the free
        // response over the sample, where the plain `force * dt` would put
        // a half-sample phase error into every mode.
        let lambda = C64::new(-sigma, w * fs);
        let zoh = (p - C64::ONE) / lambda;
        let g = mid * m.force * zoh;
        let bi1 = -(g * p.conj()).re;
        let half = (0.5 * w).sin();
        let one_minus_cos = 2.0 * half * half;
        let (h0, h1) = taps(side, sin_w, one_minus_cos, one_minus_r, r);
        let (q0, q1) = taps(probe, sin_w, one_minus_cos, one_minus_r, r);
        // |1 - p|^2 = (1 - r cos w)^2 + (r sin w)^2, with 1 - r cos w written
        // as (1 - r) + r (1 - cos w) so neither term cancels.
        let one_minus_rc = one_minus_r + r * one_minus_cos;
        let b = -(one_minus_rc * one_minus_rc + (r * sin_w) * (r * sin_w));
        let d2 = one_minus_r * (1.0 + r);
        // The state at coordinate `start`: `y[n] = Re z_mid` and, for the
        // sample before, `Re(z_mid / p)` less what the force of the sample
        // just gone (`f_prev`) still owes it — the `bi1 F[n-1]` term the next
        // step adds — so that a mode written while a hammer is in contact
        // feels only the forces from here on. Zero when no force acts.
        let now = mid * start;
        let prev = (now / p).re + bi1 / (r * r) * f64::from(self.f_prev);
        let (gi, li) = (k / LANES, k % LANES);
        self.impulse_ok = false;
        if now.re != 0.0 || prev != 0.0 {
            self.quiet = false;
        }
        self.y[gi][li] = now.re as f32;
        self.v[gi][li] = (now.re - prev) as f32;
        self.b[gi][li] = b as f32;
        self.d2[gi][li] = d2 as f32;
        self.h0[gi][li] = h0 as f32;
        self.h1[gi][li] = h1 as f32;
        self.gd[gi][li] = (m.drive.abs() / mid.abs()) as f32;
        self.bi0[gi][li] = g.re as f32;
        self.bi1[gi][li] = bi1 as f32;
        self.q0[gi][li] = q0 as f32;
        self.q1[gi][li] = q1 as f32;
        self.cw[k] = cos_w as f32;
        self.sw[k] = sin_w as f32;
        self.sigma[k] = sigma as f32;
        self.sigma_free[k] = sigma as f32;
        self.damp[k] = m.damp.max(0.0) as f32;
        self.wgt[k] = ((loud / mid.abs()) / sin_w).powi(2) as f32;
        self.mid_re[k] = mid.re as f32;
        self.mid_im[k] = mid.im as f32;
        self.tag[k] = m.tag;
        // A mode written under a pressed damper is damped like its
        // neighbours from its first sample: the felt does not know which
        // strike a partial came from.
        if self.damper > 0.0 {
            let s = self.sigma_free[k] + self.damper * self.damp[k];
            self.set_sigma(k, s);
        }
        true
    }

    /// Re-decay mode `k` to `sigma` nepers per second WITHOUT a click.
    ///
    /// The state holds the mode's complex amplitude at sample `n`: `y[n]`
    /// and, through `v`, `y[n-1]`. Changing the coefficients alone would
    /// read the same two numbers as a different place on a different
    /// curve. The new-decay trajectory through the same complex amplitude
    /// at sample `n` sat `r_old / r_new` times further from zero one sample
    /// back, so `y[n-1]` is rescaled by exactly that (in this basis,
    /// `v' = (1 - rho) y + rho v`, `rho = r_old / r_new`): the complex
    /// amplitude at `n` is unchanged and only the future decays differently
    /// (`a_decay_change_continues_the_trajectory`). The taps that carry `r`
    /// — the `g1` half of each reading, and `bi1` — move with it.
    fn set_sigma(&mut self, k: usize, sigma: f32) {
        let old = self.sigma[k];
        if sigma == old {
            return;
        }
        self.impulse_ok = false;
        let fs = f64::from(self.rate);
        let r_old = (-f64::from(old) / fs).exp();
        let omr_new = -(-f64::from(sigma) / fs).exp_m1();
        let r_new = 1.0 - omr_new;
        let up = r_new / r_old;
        let rho = r_old / r_new;
        let (gi, li) = (k / LANES, k % LANES);
        let (y, v) = (f64::from(self.y[gi][li]), f64::from(self.v[gi][li]));
        self.v[gi][li] = ((1.0 - rho) * y + rho * v) as f32;
        let (c, s) = (f64::from(self.cw[k]), f64::from(self.sw[k]));
        let half = 0.5 * s.atan2(c);
        let one_minus_rc = omr_new + r_new * 2.0 * half.sin() * half.sin();
        self.b[gi][li] = -(one_minus_rc * one_minus_rc + (r_new * s) * (r_new * s)) as f32;
        self.d2[gi][li] = (omr_new * (1.0 + r_new)) as f32;
        // h0 = g0 + g1 with g1 = -h1 proportional to r: h0 moves by the
        // change in g1, h1 by the ratio.
        let h1 = f64::from(self.h1[gi][li]);
        let q1 = f64::from(self.q1[gi][li]);
        self.h0[gi][li] = (f64::from(self.h0[gi][li]) - h1 * (up - 1.0)) as f32;
        self.h1[gi][li] = (h1 * up) as f32;
        self.q0[gi][li] = (f64::from(self.q0[gi][li]) - q1 * (up - 1.0)) as f32;
        self.q1[gi][li] = (q1 * up) as f32;
        self.bi1[gi][li] = (f64::from(self.bi1[gi][li]) * up) as f32;
        self.sigma[k] = sigma;
    }

    /// Re-decay every mode for a damper at `amount` of its full pressure
    /// (0 free, 1 pressed): each mode's own free decay plus `amount` times
    /// the damping its own position under the felt earned it. Called once
    /// per block while the damper is moving; see `set_sigma` for why that
    /// is clickless.
    pub fn set_damper(&mut self, amount: f32) {
        let a = amount.clamp(0.0, 1.0);
        if a == self.damper {
            return;
        }
        self.damper = a;
        for k in 0..self.count {
            let s = self.sigma_free[k] + a * self.damp[k];
            self.set_sigma(k, s);
        }
    }

    /// The damper's pressure as last set.
    pub fn damper(&self) -> f32 {
        self.damper
    }

    /// Mode `k`'s coordinate `z`, recovered from its state: what a phantom
    /// is seeded from, and what `respec` carries across. Exact at any
    /// sample, a hammer in contact or not.
    pub fn coordinate(&self, k: usize) -> C64 {
        if k >= self.count {
            return C64::ZERO;
        }
        let (gi, li) = (k / LANES, k % LANES);
        let x = f64::from(self.y[gi][li]);
        let r2 = 1.0 - f64::from(self.d2[gi][li]);
        let r = r2.sqrt();
        // `y[n-1]` as a free step from `z[n]` would have it: the stored one
        // less what the last force put in (zero once the hammer has left).
        let prev = x - f64::from(self.v[gi][li]) - f64::from(self.bi1[gi][li]) / r2 * f64::from(self.f_prev);
        let (c, s) = (f64::from(self.cw[k]), f64::from(self.sw[k]));
        let u = (r * prev - x * c) / s;
        C64::new(x, u) / C64::new(f64::from(self.mid_re[k]), f64::from(self.mid_im[k]))
    }

    /// Set mode `k`'s coordinate to `z`, as if it had been added with
    /// `start: z`: what a phantom is re-seeded with when a restrike changes
    /// the partials it is the product of. A step in that one mode, by
    /// design; see `grand::GrandVoice::add_phantoms` for why it is inaudible.
    pub fn reseed(&mut self, k: usize, z: C64) {
        if k >= self.count {
            return;
        }
        let (gi, li) = (k / LANES, k % LANES);
        let mid = C64::new(f64::from(self.mid_re[k]), f64::from(self.mid_im[k]));
        let r = (1.0 - f64::from(self.d2[gi][li])).sqrt();
        let w = f64::from(self.sw[k]).atan2(f64::from(self.cw[k]));
        self.quiet = false;
        let now = mid * z;
        let prev = now / C64::polar(r, w);
        self.y[gi][li] = now.re as f32;
        self.v[gi][li] = (now.re - prev.re) as f32;
    }

    /// Mode `k` as an image: its coefficients, with its free decay, and not
    /// its motion (`ModeImage`). Zeroes for a slot past the end.
    pub fn image(&self, k: usize) -> ModeImage {
        if k >= self.count {
            return ModeImage::default();
        }
        let (gi, li) = (k / LANES, k % LANES);
        ModeImage {
            b: self.b[gi][li],
            d2: self.d2[gi][li],
            h0: self.h0[gi][li],
            h1: self.h1[gi][li],
            gd: self.gd[gi][li],
            bi0: self.bi0[gi][li],
            bi1: self.bi1[gi][li],
            q0: self.q0[gi][li],
            q1: self.q1[gi][li],
            cw: self.cw[k],
            sw: self.sw[k],
            sigma: self.sigma_free[k],
            damp: self.damp[k],
            wgt: self.wgt[k],
            mid_re: self.mid_re[k],
            mid_im: self.mid_im[k],
            tag: self.tag[k],
        }
    }

    /// Clear, then load `modes` at rest in their order, and with them the
    /// probe's response to a unit force if it is given (the one
    /// `impulse_response` gave on the bank they were imaged from). Exactly
    /// what clearing and `add`ing the specs those images came from at rest
    /// would leave (`an_image_loads_back_the_bank_it_was_taken_from`), for a
    /// copy. False, and the bank left cleared, when there are more than
    /// `MAX_MODES`.
    pub fn load(&mut self, modes: &[ModeImage], impulse: Option<&[f32; BLOCK + 1]>) -> bool {
        self.clear();
        if modes.len() > MAX_MODES {
            return false;
        }
        for (k, m) in modes.iter().enumerate() {
            let (gi, li) = (k / LANES, k % LANES);
            // At rest: y and v stay the zeros `clear` left.
            self.b[gi][li] = m.b;
            self.d2[gi][li] = m.d2;
            self.h0[gi][li] = m.h0;
            self.h1[gi][li] = m.h1;
            self.gd[gi][li] = m.gd;
            self.bi0[gi][li] = m.bi0;
            self.bi1[gi][li] = m.bi1;
            self.q0[gi][li] = m.q0;
            self.q1[gi][li] = m.q1;
            self.cw[k] = m.cw;
            self.sw[k] = m.sw;
            self.sigma[k] = m.sigma;
            self.sigma_free[k] = m.sigma;
            self.damp[k] = m.damp;
            self.wgt[k] = m.wgt;
            self.mid_re[k] = m.mid_re;
            self.mid_im[k] = m.mid_im;
            self.tag[k] = m.tag;
        }
        self.count = modes.len();
        if let Some(h) = impulse {
            self.impulse = *h;
            self.impulse_ok = true;
        }
        true
    }

    /// The probe's response to one newton held over the first sample, as
    /// `render_contact` reads it (`probe_impulse`): computed now if it is not
    /// the bank's already, and kept.
    pub fn impulse_response(&mut self) -> [f32; BLOCK + 1] {
        if !self.impulse_ok {
            let mut h = [0.0_f32; BLOCK + 1];
            self.probe_impulse(&mut h);
            self.impulse = h;
            self.impulse_ok = true;
        }
        self.impulse
    }

    /// Mode `k`'s label.
    pub fn tag(&self, k: usize) -> u32 {
        self.tag[k]
    }

    /// Mode `k`'s frequency in Hz and current decay in nepers per second.
    #[cfg(test)]
    pub fn freq_sigma(&self, k: usize) -> (f32, f32) {
        let w = self.sw[k].atan2(self.cw[k]);
        (w * self.rate / std::f32::consts::TAU, self.sigma[k])
    }

    /// The level the last `cull` measured: the root of the sum of every
    /// mode's loudest reading squared, in output units. Cheap — it is read,
    /// not measured, which is what the stealing policy needs.
    pub fn amplitude(&self) -> f32 {
        self.level
    }

    /// Remove every mode whose loudest reading is below `floor`, and
    /// measure the level of what is left.
    ///
    /// **The amplitude of a two-pole state, without running it.** For
    /// `y[n] = A r^n sin(a)` and `y[n-1] = A r^(n-1) sin(a - w)`,
    /// `y[n]^2 - c1 y[n] y[n-1] - c2 y[n-1]^2 = (A r^n)^2 sin^2 w` — the
    /// identity `sin^2 a + sin^2 b - 2 cos(a - b) sin a sin b = sin^2(a - b)`
    /// with the decay folded in through `c1` and `c2` themselves, so it is
    /// exact for a decaying mode and not only a free one. In the difference
    /// basis it reads `-b y^2 + (b + d2) y v + (1 - d2) v^2`
    /// (`the_amplitude_identity_holds_for_a_decaying_mode`). A few multiplies
    /// per mode, vectorised a group at a time.
    ///
    /// **Removal is a swap with the last mode**, every array at once, and
    /// the vacated slot is zeroed, so the groups stay dense and a survivor's
    /// numbers are copied, never recomputed: its state after a cull is
    /// bit-identical to what it would have been without one
    /// (`culling_removes_only_what_is_under_the_floor`).
    pub fn cull(&mut self, floor: f32) -> f32 {
        let groups = self.count.div_ceil(LANES);
        // Every mode's loudest reading, squared, a group at a time.
        let mut a2 = [0.0_f32; MAX_MODES];
        for g in 0..groups {
            let (y, v, b, d2) = (&self.y[g], &self.v[g], &self.b[g], &self.d2[g]);
            let wgt = &self.wgt[g * LANES..(g + 1) * LANES];
            let out = &mut a2[g * LANES..(g + 1) * LANES];
            for j in 0..LANES {
                let e = -b[j] * y[j] * y[j] + (b[j] + d2[j]) * y[j] * v[j] + (1.0 - d2[j]) * v[j] * v[j];
                out[j] = e.max(0.0) * wgt[j];
            }
        }
        // Judged from the END: a removal swaps the last mode into the hole,
        // and walking backwards the last mode has always been judged
        // already and kept, so every slot is looked at exactly once.
        let f2 = floor * floor;
        let mut total = 0.0_f32;
        let mut bound = 0.0_f32;
        let mut removed = 0.0_f32;
        let mut k = self.count;
        while k > 0 {
            k -= 1;
            if a2[k] >= f2 {
                total += a2[k];
                bound += a2[k].sqrt();
            } else {
                removed += a2[k].sqrt();
                self.remove(k);
            }
        }
        self.level = total.sqrt();
        self.bound = bound;
        self.removed = removed;
        self.level
    }

    /// The sum of every remaining mode's loudest reading, as the last
    /// `cull` measured it: the most any output can reach if every mode
    /// peaked at once, so a bank whose bound is under a line cannot put a
    /// sample over it.
    pub fn bound(&self) -> f32 {
        self.bound
    }

    /// What the last `cull` removed: the sum of the removed modes' loudest
    /// readings, which bounds the step the removal put into any output.
    ///
    /// A measurement: `grand::tests::culling_never_steps_more_than_the_floor`
    /// is its reader, and the instrument has no use for it.
    #[allow(dead_code)]
    pub fn removed(&self) -> f32 {
        self.removed
    }

    /// Swap mode `m` with the last one and zero the vacated slot.
    fn remove(&mut self, m: usize) {
        self.impulse_ok = false;
        let last = self.count - 1;
        let (gm, lm) = (m / LANES, m % LANES);
        let (gl, ll) = (last / LANES, last % LANES);
        for arr in [
            &mut self.y,
            &mut self.v,
            &mut self.b,
            &mut self.d2,
            &mut self.h0,
            &mut self.h1,
            &mut self.gd,
            &mut self.bi0,
            &mut self.bi1,
            &mut self.q0,
            &mut self.q1,
        ] {
            arr[gm][lm] = arr[gl][ll];
            arr[gl][ll] = 0.0;
        }
        for arr in [
            &mut self.cw,
            &mut self.sw,
            &mut self.sigma,
            &mut self.sigma_free,
            &mut self.damp,
            &mut self.wgt,
            &mut self.mid_re,
            &mut self.mid_im,
        ] {
            arr[m] = arr[last];
            arr[last] = 0.0;
        }
        self.tag[m] = self.tag[last];
        self.tag[last] = 0;
        self.count = last;
    }

    /// Run every mode for `l.len()` frames (at most `BLOCK`) and ADD the two
    /// microphones and the bridge drive into the slices. No force acts.
    pub fn render(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32]) {
        let n = l.len().min(r.len()).min(drive.len()).min(BLOCK);
        if n == 0 || self.count == 0 {
            return;
        }
        let (l, r, drive) = (&mut l[..n], &mut r[..n], &mut drive[..n]);
        if self.f_prev != 0.0 {
            // The block after a contact still owes the last force its
            // `bi1 F[n-1]` term: once through the forced loop with nothing
            // pushing, and the fast loop from the next block on.
            self.render_forced(l, r, drive, &[0.0; BLOCK][..n]);
        } else {
            self.render_free(l, r, drive);
        }
    }

    fn render_free(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32]) {
        let n = l.len();
        if n == 0 {
            return;
        }
        self.quiet = false;
        let mut am = [[0.0_f32; LANES]; BLOCK];
        let mut asd = [[0.0_f32; LANES]; BLOCK];
        let mut ad = [[0.0_f32; LANES]; BLOCK];
        let groups = self.count.div_ceil(LANES);
        let mut g = 0;
        while g < groups {
            // Two groups at once: sixteen lanes of state and five arrays of
            // coefficients fill the register file without spilling, and
            // each accumulator load and store is shared by sixteen modes
            // instead of eight. `GROUPS` is even, so `g + 1` exists; past
            // `count` it is all zeros and computes zeros.
            let (ga, gb) = (g, g + 1);
            let (ba, da, h0a, h1a, gda) = (self.b[ga], self.d2[ga], self.h0[ga], self.h1[ga], self.gd[ga]);
            let (bb, db, h0b, h1b, gdb) = (self.b[gb], self.d2[gb], self.h0[gb], self.h1[gb], self.gd[gb]);
            let (mut ya, mut va) = (self.y[ga], self.v[ga]);
            let (mut yb, mut vb) = (self.y[gb], self.v[gb]);
            for s in 0..n {
                for j in 0..LANES {
                    va[j] += ba[j] * ya[j] - da[j] * va[j];
                    vb[j] += bb[j] * yb[j] - db[j] * vb[j];
                }
                for j in 0..LANES {
                    ya[j] += va[j];
                    yb[j] += vb[j];
                }
                for j in 0..LANES {
                    am[s][j] += ya[j] + yb[j];
                }
                for j in 0..LANES {
                    asd[s][j] += (h0a[j] * ya[j] + h1a[j] * va[j]) + (h0b[j] * yb[j] + h1b[j] * vb[j]);
                }
                for j in 0..LANES {
                    ad[s][j] += gda[j] * ya[j] + gdb[j] * yb[j];
                }
            }
            flush(&mut ya, &mut va);
            flush(&mut yb, &mut vb);
            self.y[ga] = ya;
            self.v[ga] = va;
            self.y[gb] = yb;
            self.v[gb] = vb;
            g += 2;
        }
        for s in 0..n {
            let m: f32 = am[s].iter().sum();
            let sd: f32 = asd[s].iter().sum();
            let d: f32 = ad[s].iter().sum();
            l[s] += m + sd;
            r[s] += m - sd;
            drive[s] += d;
        }
    }

    /// Run every mode for `l.len()` frames (at most `BLOCK`) under a contact
    /// force, and ADD the outputs. Each sample, `contact(now, next_free,
    /// compliance)` is asked for the force in newtons, given the
    /// displacement under the contact now (`now`: every probe reading
    /// summed, everything already ringing included — which is why a
    /// restrike meets the string as it is moving), where that displacement
    /// will be one sample on if this sample's force is zero (`next_free`),
    /// and how far one newton of it moves that (`compliance`, metres per
    /// newton, constant over the block). That is what an IMPLICIT contact
    /// needs (`hammer::Hammer::tick_implicit`).
    ///
    /// **A block at a time, by superposition.** The bank is linear in the
    /// force, so the displacement under the contact at sample `t` of the
    /// block is the displacement with no force at all, plus each force so
    /// far times the probe's response to a unit force that many samples on:
    /// `u[t] = u_free[t] + sum_(j < t) F[j] h[t - j]`, and `h[1]` is the
    /// compliance. So the block is three passes of the fast loop's shape —
    /// every group run through the whole block with its state in registers
    /// — and one scalar loop: the free run reads `u_free` (`probe_free`),
    /// the impulse run reads `h` (`probe_impulse`), the scalar loop asks the
    /// contact for each force in turn and adds that force's future into the
    /// readings still to come, and the forced run steps every mode under
    /// the forces it now knows, outputs and all (`render_forced`). The first
    /// draft stepped every mode a sample at a time, every group through
    /// memory and a horizontal sum each sample, at 0.6 to 0.7 G
    /// mode-samples per second; `tests::timing_of_the_bank` has this one's.
    pub fn render_contact(
        &mut self,
        l: &mut [f32],
        r: &mut [f32],
        drive: &mut [f32],
        contact: &mut impl FnMut(f32, f32, f32) -> f32,
    ) {
        let n = l.len().min(r.len()).min(drive.len()).min(BLOCK);
        if n == 0 {
            return;
        }
        let mut c = Contact::default();
        self.begin_contact(&mut c, n);
        self.render_contact_part(&mut c, l, r, drive, contact);
    }

    /// A contact block of `n` frames (at most `BLOCK`) begins: its free run
    /// read from the bank as it stands, and nothing owed yet. The block is
    /// then rendered in as many parts as its caller likes
    /// (`render_contact_part`), and the parts are the block rendered whole
    /// to the bit: each force is solved against the same readings and the
    /// modes step through the same samples.
    pub fn begin_contact(&mut self, c: &mut Contact, n: usize) {
        let n = n.min(BLOCK);
        c.len = n;
        c.at = 0;
        c.u_free[..=n].fill(0.0);
        c.owed[..=n].fill(0.0);
        if self.quiet && self.f_prev == 0.0 {
            // At rest: a fresh strike's first block has no free run at all.
            if !self.impulse_ok {
                let mut h = [0.0_f32; BLOCK + 1];
                self.probe_impulse(&mut h);
                self.impulse = h;
                self.impulse_ok = true;
            }
        } else if self.impulse_ok {
            self.probe_free(&mut c.u_free[..=n]);
        } else {
            // Both runs at once on a contact's first block: they share every
            // coefficient, and the two independent chains interleave.
            let mut h = [0.0_f32; BLOCK + 1];
            self.probe_both(&mut c.u_free[..=n], &mut h);
            self.impulse = h;
            self.impulse_ok = true;
        }
    }

    /// The next `l.len()` frames of the contact block `c` (at most what is
    /// left of it), ADDED into the slices as `render_contact` adds them.
    /// Nothing may change a coefficient between `begin_contact` and the
    /// block's last part: the impulse the forces are spread by is the one
    /// the block began with.
    pub fn render_contact_part(
        &mut self,
        c: &mut Contact,
        l: &mut [f32],
        r: &mut [f32],
        drive: &mut [f32],
        contact: &mut impl FnMut(f32, f32, f32) -> f32,
    ) {
        let m = l.len().min(r.len()).min(drive.len()).min(c.len - c.at);
        if m == 0 {
            return;
        }
        let (n, from) = (c.len, c.at);
        let h = self.impulse;
        let comp = h[1].max(0.0);
        // What the forces decided so far add to the readings still to come,
        // to the block's end, whichever part they are read in.
        let mut force = [0.0_f32; BLOCK];
        for t in from..from + m {
            let now = c.u_free[t] + c.owed[t];
            let next_free = c.u_free[t + 1] + c.owed[t + 1];
            let mut f = contact(now, next_free, comp);
            if !f.is_finite() {
                f = 0.0;
            }
            force[t - from] = f;
            if f != 0.0 {
                for (o, &g) in c.owed[t + 1..=n].iter_mut().zip(&h[1..=n - t]) {
                    *o += f * g;
                }
            }
        }
        c.at = from + m;
        self.render_forced(&mut l[..m], &mut r[..m], &mut drive[..m], &force[..m]);
    }

    /// The displacement under the contact at each of `out.len()` samples
    /// from now, if no new force acted: the state run forward from where it
    /// is (the last force's `bi1` term included), read by the probe taps.
    /// The state itself is not touched.
    fn probe_free(&self, out: &mut [f32]) {
        let len = out.len();
        let groups = self.count.div_ceil(LANES);
        let mut acc = [[0.0_f32; LANES]; BLOCK + 1];
        let fp = self.f_prev;
        let mut g = 0;
        while g < groups {
            let (ga, gb) = (g, g + 1);
            let (ba, da, qa0, qa1, ea) = (self.b[ga], self.d2[ga], self.q0[ga], self.q1[ga], self.bi1[ga]);
            let (bb, db, qb0, qb1, eb) = (self.b[gb], self.d2[gb], self.q0[gb], self.q1[gb], self.bi1[gb]);
            let (mut ya, mut va) = (self.y[ga], self.v[ga]);
            let (mut yb, mut vb) = (self.y[gb], self.v[gb]);
            for j in 0..LANES {
                acc[0][j] += qa0[j] * ya[j] + qa1[j] * va[j] + qb0[j] * yb[j] + qb1[j] * vb[j];
            }
            for (t, row) in acc.iter_mut().enumerate().take(len).skip(1) {
                let kick = if t == 1 { fp } else { 0.0 };
                for j in 0..LANES {
                    va[j] += ba[j] * ya[j] - da[j] * va[j] + ea[j] * kick;
                    vb[j] += bb[j] * yb[j] - db[j] * vb[j] + eb[j] * kick;
                }
                for j in 0..LANES {
                    ya[j] += va[j];
                    yb[j] += vb[j];
                }
                for j in 0..LANES {
                    row[j] += qa0[j] * ya[j] + qa1[j] * va[j] + qb0[j] * yb[j] + qb1[j] * vb[j];
                }
            }
            g += 2;
        }
        let gam_q = self.gamma_probe();
        for (o, row) in out.iter_mut().zip(acc.iter()) {
            *o = row.iter().sum();
        }
        out[0] += gam_q * fp;
    }

    /// `probe_free` into `free` and `probe_impulse` into `imp` (all of it,
    /// `BLOCK + 1` samples), in one pass: a group's coefficients loaded once
    /// for both runs, and the free run's dependency chain interleaved with
    /// the impulse run's — the passes are latency-bound, so the second
    /// costs a fraction of the first.
    fn probe_both(&self, free: &mut [f32], imp: &mut [f32; BLOCK + 1]) {
        let len = free.len();
        let groups = self.count.div_ceil(LANES);
        let mut acc_f = [[0.0_f32; LANES]; BLOCK + 1];
        let mut acc_h = [[0.0_f32; LANES]; BLOCK + 1];
        let fp = self.f_prev;
        for g in 0..groups {
            let (b, d2, q0, q1, e0, e1) = (self.b[g], self.d2[g], self.q0[g], self.q1[g], self.bi0[g], self.bi1[g]);
            let (mut yf, mut vf) = (self.y[g], self.v[g]);
            let (mut yh, mut vh) = (e0, e0);
            for j in 0..LANES {
                acc_f[0][j] += q0[j] * yf[j] + q1[j] * vf[j];
                acc_h[1][j] += q0[j] * yh[j] + q1[j] * vh[j];
            }
            for t in 1..=BLOCK {
                let kick_f = if t == 1 { fp } else { 0.0 };
                let kick_h = if t == 1 { 1.0 } else { 0.0 };
                for j in 0..LANES {
                    vf[j] += b[j] * yf[j] - d2[j] * vf[j] + e1[j] * kick_f;
                    vh[j] += b[j] * yh[j] - d2[j] * vh[j] + e1[j] * kick_h;
                }
                for j in 0..LANES {
                    yf[j] += vf[j];
                    yh[j] += vh[j];
                }
                if t < len {
                    for j in 0..LANES {
                        acc_f[t][j] += q0[j] * yf[j] + q1[j] * vf[j];
                    }
                }
                if t < BLOCK {
                    for j in 0..LANES {
                        acc_h[t + 1][j] += q0[j] * yh[j] + q1[j] * vh[j];
                    }
                }
            }
        }
        let gam_q = self.gamma_probe();
        for (o, row) in free.iter_mut().zip(acc_f.iter()) {
            *o = row.iter().sum();
        }
        free[0] += gam_q * fp;
        for (o, row) in imp.iter_mut().zip(acc_h.iter()) {
            *o = row.iter().sum();
        }
        imp[1] += gam_q;
    }

    /// The probe's response to one newton held over the first sample:
    /// `out[d]` is the displacement under the contact `d` samples after it
    /// was applied (`out[0]` is zero, and `out[1]` is the compliance).
    fn probe_impulse(&self, out: &mut [f32]) {
        let len = out.len();
        let groups = self.count.div_ceil(LANES);
        let mut acc = [[0.0_f32; LANES]; BLOCK + 1];
        let mut g = 0;
        while g < groups {
            let (ga, gb) = (g, g + 1);
            let (ba, da, qa0, qa1) = (self.b[ga], self.d2[ga], self.q0[ga], self.q1[ga]);
            let (bb, db, qb0, qb1) = (self.b[gb], self.d2[gb], self.q0[gb], self.q1[gb]);
            let (mut ya, mut va) = (self.bi0[ga], self.bi0[ga]);
            let (mut yb, mut vb) = (self.bi0[gb], self.bi0[gb]);
            if len > 1 {
                for j in 0..LANES {
                    acc[1][j] += qa0[j] * ya[j] + qa1[j] * va[j] + qb0[j] * yb[j] + qb1[j] * vb[j];
                }
            }
            let (ea, eb) = (self.bi1[ga], self.bi1[gb]);
            for (t, row) in acc.iter_mut().enumerate().take(len).skip(2) {
                let kick = if t == 2 { 1.0 } else { 0.0 };
                for j in 0..LANES {
                    va[j] += ba[j] * ya[j] - da[j] * va[j] + ea[j] * kick;
                    vb[j] += bb[j] * yb[j] - db[j] * vb[j] + eb[j] * kick;
                }
                for j in 0..LANES {
                    ya[j] += va[j];
                    yb[j] += vb[j];
                }
                for j in 0..LANES {
                    row[j] += qa0[j] * ya[j] + qa1[j] * va[j] + qb0[j] * yb[j] + qb1[j] * vb[j];
                }
            }
            g += 2;
        }
        for (o, row) in out.iter_mut().zip(acc.iter()) {
            *o = row.iter().sum();
        }
        if len > 1 {
            out[1] += self.gamma_probe();
        }
    }

    /// The probe reading's one-sample correction while a force acts,
    /// summed over the bank: `q1 bi1 / r^2` per mode (see `taps`).
    fn gamma_probe(&self) -> f32 {
        let groups = self.count.div_ceil(LANES);
        let mut acc = 0.0_f32;
        for g in 0..groups {
            for j in 0..LANES {
                if self.bi1[g][j] != 0.0 {
                    acc += self.q1[g][j] * self.bi1[g][j] / (1.0 - self.d2[g][j]);
                }
            }
        }
        acc
    }

    /// Run every mode for `l.len()` frames under the known forces
    /// `force[t]` (newtons, one a sample) and ADD the outputs: the fast
    /// loop with the two input taps added, the last force before the block
    /// (`f_prev`) carried into its first sample and the block's last one
    /// left for the next.
    fn render_forced(&mut self, l: &mut [f32], r: &mut [f32], drive: &mut [f32], force: &[f32]) {
        let n = l.len().min(force.len());
        if n == 0 {
            return;
        }
        self.quiet = false;
        let mut am = [[0.0_f32; LANES]; BLOCK];
        let mut asd = [[0.0_f32; LANES]; BLOCK];
        let mut ad = [[0.0_f32; LANES]; BLOCK];
        // The side reading's correction while a force acts (`h1 bi1 / r^2`),
        // summed over the bank, as for the probe.
        let mut gam_s = 0.0_f32;
        let groups = self.count.div_ceil(LANES);
        for g in 0..groups {
            for j in 0..LANES {
                if self.bi1[g][j] != 0.0 {
                    gam_s += self.h1[g][j] * self.bi1[g][j] / (1.0 - self.d2[g][j]);
                }
            }
        }
        let f0 = self.f_prev;
        let mut g = 0;
        while g < groups {
            let (ga, gb) = (g, g + 1);
            let (ba, da, h0a, h1a, gda) = (self.b[ga], self.d2[ga], self.h0[ga], self.h1[ga], self.gd[ga]);
            let (bb, db, h0b, h1b, gdb) = (self.b[gb], self.d2[gb], self.h0[gb], self.h1[gb], self.gd[gb]);
            let (ia0, ia1, ib0, ib1) = (self.bi0[ga], self.bi1[ga], self.bi0[gb], self.bi1[gb]);
            let (mut ya, mut va) = (self.y[ga], self.v[ga]);
            let (mut yb, mut vb) = (self.y[gb], self.v[gb]);
            for s in 0..n {
                let (f, fp) = (force[s], if s == 0 { f0 } else { force[s - 1] });
                for j in 0..LANES {
                    va[j] += ba[j] * ya[j] - da[j] * va[j] + ia0[j] * f + ia1[j] * fp;
                    vb[j] += bb[j] * yb[j] - db[j] * vb[j] + ib0[j] * f + ib1[j] * fp;
                }
                for j in 0..LANES {
                    ya[j] += va[j];
                    yb[j] += vb[j];
                }
                for j in 0..LANES {
                    am[s][j] += ya[j] + yb[j];
                }
                for j in 0..LANES {
                    asd[s][j] += (h0a[j] * ya[j] + h1a[j] * va[j]) + (h0b[j] * yb[j] + h1b[j] * vb[j]);
                }
                for j in 0..LANES {
                    ad[s][j] += gda[j] * ya[j] + gdb[j] * yb[j];
                }
            }
            self.y[ga] = ya;
            self.v[ga] = va;
            self.y[gb] = yb;
            self.v[gb] = vb;
            g += 2;
        }
        for s in 0..n {
            let m: f32 = am[s].iter().sum();
            let sd: f32 = asd[s].iter().sum::<f32>() + gam_s * force[s];
            let d: f32 = ad[s].iter().sum();
            l[s] += m + sd;
            r[s] += m - sd;
            drive[s] += d;
        }
        self.f_prev = force[n - 1];
    }
}

/// States under `TINY` to zero, eight lanes at a time.
fn flush(y: &mut Lane, v: &mut Lane) {
    for j in 0..LANES {
        if y[j].abs() < TINY && v[j].abs() < TINY {
            y[j] = 0.0;
            v[j] = 0.0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// A mode heard equally by both microphones, unit reading, ringing from
    /// a unit coordinate.
    fn plain(freq: f64, sigma: f64) -> ModeSpec {
        ModeSpec {
            freq,
            sigma,
            left: C64::ONE,
            right: C64::ONE,
            drive: 1.0,
            start: C64::ONE,
            ..ModeSpec::default()
        }
    }

    /// Render `frames` in blocks of `block`, returning (L, R, drive).
    fn run(bank: &mut Bank, frames: usize, block: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
        let (mut l, mut r, mut d) = (vec![0.0; frames], vec![0.0; frames], vec![0.0; frames]);
        let mut at = 0;
        while at < frames {
            let n = block.min(frames - at);
            bank.render(&mut l[at..at + n], &mut r[at..at + n], &mut d[at..at + n]);
            at += n;
        }
        (l, r, d)
    }

    /// The amplitude and phase of one component at a known frequency, by
    /// a Hann-windowed heterodyne over `len` samples from `from`: a
    /// rectangle's sidelobes would bias both by a part in a few hundred.
    fn demod(x: &[f32], f: f64, from: usize, len: usize) -> (f64, f64) {
        let w = std::f64::consts::TAU * f / f64::from(RATE);
        let (mut re, mut im, mut wsum) = (0.0, 0.0, 0.0);
        for i in from..from + len {
            let win = 0.5 - 0.5 * (std::f64::consts::TAU * (i - from) as f64 / len as f64).cos();
            let s = f64::from(x[i]) * win;
            re += s * (w * i as f64).cos();
            im -= s * (w * i as f64).sin();
            wsum += win;
        }
        ((re * re + im * im).sqrt() * 2.0 / wsum, im.atan2(re))
    }

    /// **Frequency within 0.01 %, T60 within 1 %, from the deep bass to the
    /// top of the cap** — the difference form is what makes the low ones
    /// pass (see the module doc: the direct form puts 27.5 Hz 4 cents off).
    /// Frequency from the phase advance of the analytic signal over a long
    /// span; T60 from the energy invariant at two instants, which is exact
    /// for a decaying two-pole and needs no envelope fit.
    #[test]
    fn a_single_mode_rings_where_it_was_put() {
        for &(f, t60) in &[(27.5, 70.0), (27.5, 2.0), (65.4, 35.0), (440.0, 3.0), (4186.0, 0.4), (19_000.0, 0.2)] {
            let sigma = 1000.0_f64.ln() / t60;
            let mut b = Bank::new(RATE);
            assert!(b.add(&plain(f, sigma)));
            let span = (RATE as f64 * (40.0 / f).max(0.05)) as usize;
            let (l, _, _) = run(&mut b, 2 * span + 10_000, 128);
            // Phase advance between two windows `span` apart, unwrapped
            // against the nominal advance.
            let probe = |at: usize, g: f64| demod(&l, g, at, (RATE as f64 / f * 8.0) as usize).1;
            let p0 = probe(0, f);
            let p1 = probe(span, f);
            let dphi = (p1 - p0 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
            // demod at exactly f: a frequency error shows as phase drift.
            let f_meas = f + dphi / (std::f64::consts::TAU * span as f64 / f64::from(RATE));
            let ferr = (f_meas / f - 1.0).abs();
            // Decay from the bank's own energy invariant, through `cull`.
            let mut b2 = Bank::new(RATE);
            b2.add(&plain(f, sigma));
            let a0 = b2.cull(0.0);
            let n = (0.25 * RATE as f64 * t60.min(4.0)) as usize;
            run(&mut b2, n, 128);
            let a1 = b2.cull(0.0);
            let s_meas = -(f64::from(a1) / f64::from(a0)).ln() / (n as f64 / f64::from(RATE));
            let t_meas = 1000.0_f64.ln() / s_meas;
            let terr = (t_meas / t60 - 1.0).abs();
            println!("f {f:8.1} Hz: measured {f_meas:10.4} ({:.1e}); T60 {t60:5.1} s: measured {t_meas:8.3} ({terr:.1e})", ferr);
            assert!(ferr < 1.0e-4, "{f} Hz rang at {f_meas}");
            assert!(terr < 0.01, "{f} Hz T60 {t60} measured {t_meas}");
        }
    }

    /// The energy invariant reads the amplitude of a decaying mode at any
    /// instant, without running it: against the closed-form envelope.
    #[test]
    fn the_amplitude_identity_holds_for_a_decaying_mode() {
        for &(f, sigma) in &[(30.0, 0.3), (523.0, 2.0), (6000.0, 30.0)] {
            let mut b = Bank::new(RATE);
            let mut m = plain(f, sigma);
            m.start = C64::polar(0.7, 1.1);
            b.add(&m);
            for step in 0..5 {
                let frames = 977 * step;
                let mut bb = b.clone();
                run(&mut bb, frames, 128);
                let got = f64::from(bb.cull(0.0));
                let want = 0.7 * (-sigma * frames as f64 / f64::from(RATE)).exp();
                assert!((got / want - 1.0).abs() < 2.0e-4, "{f} Hz after {frames}: {got} vs {want}");
            }
        }
    }

    /// **A decay change continues the trajectory.** Ring a mode, change its
    /// decay mid-block-stream, and compare sample by sample with the closed
    /// form of a sinusoid that changes decay at that instant — the same
    /// amplitude and phase at the switch, the new rate after. And the first
    /// difference across the switch is no larger than the sinusoid's own.
    #[test]
    fn a_decay_change_continues_the_trajectory() {
        let (f, s1, s2) = (220.0_f64, 0.5_f64, 40.0_f64);
        let mut b = Bank::new(RATE);
        b.add(&ModeSpec { damp: s2 - s1, ..plain(f, s1) });
        let switch = 5 * 128;
        let (mut l, _, _) = run(&mut b, switch, 128);
        b.set_damper(1.0);
        let (l2, _, _) = run(&mut b, 4096, 128);
        l.extend(l2);
        let w = std::f64::consts::TAU * f / f64::from(RATE);
        let mut worst = 0.0_f64;
        for (i, &got) in l.iter().enumerate() {
            // Output `i` is the state after step `i + 1`.
            let n = i + 1;
            let t = n as f64 / f64::from(RATE);
            let ts = switch as f64 / f64::from(RATE);
            let env = if n <= switch { (-s1 * t).exp() } else { (-s1 * ts).exp() * (-s2 * (t - ts)).exp() };
            // `start = 1` on a cos-phase coordinate: y[n] = r^n cos(n w).
            let want = env * (w * n as f64).cos();
            worst = worst.max((f64::from(got) - want).abs());
        }
        assert!(worst < 2.0e-5, "the switched mode left the closed form by {worst}");
        let d1 = |a: usize, b: usize| (a..b).map(|n| (l[n] - l[n - 1]).abs()).fold(0.0_f32, f32::max);
        let before = d1(switch - 200, switch);
        let across = d1(switch - 2, switch + 3);
        assert!(across <= before * 1.001, "the switch stepped: {across} against {before}");
    }

    /// **A respec keeps the coordinate and takes the new mode.** A mode
    /// ringing at one frequency and decay is re-made at another mid-stream:
    /// sample for sample the output is the closed form of a sinusoid whose
    /// frequency and decay change at that instant with its complex
    /// amplitude kept. And under a force: a mode re-made with its own spec
    /// halfway through a contact pulse carries on exactly as the f64
    /// reference that was never touched — `coordinate` and the state
    /// `respec` writes both account for the force still owed.
    #[test]
    fn a_respec_keeps_the_coordinate_and_takes_the_new_mode() {
        let (f1, s1, f2, s2) = (220.0_f64, 0.5_f64, 233.0_f64, 6.0_f64);
        let mut b = Bank::new(RATE);
        b.add(&plain(f1, s1));
        let switch = 5 * 128;
        let (mut l, _, _) = run(&mut b, switch, 128);
        assert!(b.respec(0, &plain(f2, s2)));
        let (l2, _, _) = run(&mut b, 4096, 128);
        l.extend(l2);
        let fs = f64::from(RATE);
        let p1 = C64::polar((-s1 / fs).exp(), std::f64::consts::TAU * f1 / fs);
        let p2 = C64::polar((-s2 / fs).exp(), std::f64::consts::TAU * f2 / fs);
        let mut z = C64::ONE;
        let mut worst = 0.0_f64;
        for (i, &got) in l.iter().enumerate() {
            z = if i < switch { p1 * z } else { p2 * z };
            worst = worst.max((f64::from(got) - z.re).abs());
        }
        assert!(worst < 2.0e-5, "the re-made mode left the closed form by {worst}");
        // Under a force.
        let force = C64::new(0.0, -2.0);
        let spec = ModeSpec {
            force,
            probe: C64::new(0.5, 0.0),
            start: C64::ZERO,
            ..plain(300.0, 3.0)
        };
        let pulse = |n: usize| -> f32 { 40.0 * (std::f32::consts::PI * n as f32 / 96.0).sin().powi(2) };
        let mut a = Bank::new(RATE);
        let mut c = Bank::new(RATE);
        a.add(&spec);
        c.add(&spec);
        let (mut la, mut ra, mut da) = (vec![0.0; 96], vec![0.0; 96], vec![0.0; 96]);
        let (mut lc, mut rc, mut dc) = (vec![0.0; 96], vec![0.0; 96], vec![0.0; 96]);
        let (mut na, mut nc) = (0usize, 0usize);
        a.render_contact(&mut la[..40], &mut ra[..40], &mut da[..40], &mut |_, _, _| {
            na += 1;
            pulse(na - 1)
        });
        c.render_contact(&mut lc[..40], &mut rc[..40], &mut dc[..40], &mut |_, _, _| {
            nc += 1;
            pulse(nc - 1)
        });
        // The reference coordinate after 40 forced steps.
        let p = C64::polar((-3.0 / fs).exp(), std::f64::consts::TAU * 300.0 / fs);
        let zoh = (p - C64::ONE) / C64::new(-3.0, std::f64::consts::TAU * 300.0);
        let mut zr = C64::ZERO;
        for i in 0..40 {
            zr = p * zr + force * zoh.scale(f64::from(pulse(i)));
        }
        let zc = c.coordinate(0);
        let scale = zr.abs();
        assert!((zc - zr).abs() < 1.0e-5 * scale, "coordinate under a force {zc:?} against {zr:?}");
        assert!(c.respec(0, &spec));
        a.render_contact(&mut la[40..], &mut ra[40..], &mut da[40..], &mut |_, _, _| {
            na += 1;
            pulse(na - 1)
        });
        c.render_contact(&mut lc[40..], &mut rc[40..], &mut dc[40..], &mut |_, _, _| {
            nc += 1;
            pulse(nc - 1)
        });
        let pk = la.iter().fold(0.0_f32, |m, s| m.max(s.abs()));
        let off = la.iter().zip(&lc).fold(0.0_f32, |m, (x, y)| m.max((x - y).abs()));
        println!("respec: free switch {worst:.1e}; mid-contact respec moved the output {off:.1e} against a peak of {pk:.2e}");
        assert!(off < 1.0e-5 * pk, "a respec mid-contact moved the output by {off}");
    }

    /// **Per-channel amplitude and phase.** One mode, left weight `a e^{i x}`
    /// and right `b e^{i y}`: the two outputs are that mode at those
    /// amplitudes and that phase difference — what makes the stereo
    /// decorrelated rather than panned.
    #[test]
    fn the_channel_taps_put_the_requested_amplitude_and_phase() {
        for &(f, (al, pl), (ar, pr)) in &[
            (110.0, (1.0, 0.0), (0.5, 1.2)),
            (1500.0, (0.3, -2.0), (0.9, 0.7)),
            (31.0, (0.8, 0.4), (0.8, -0.4)),
        ] {
            let mut b = Bank::new(RATE);
            let m = ModeSpec {
                left: C64::polar(al, pl),
                right: C64::polar(ar, pr),
                ..plain(f, 0.0)
            };
            b.add(&m);
            let len = (RATE as f64 / f * 40.0).max(4800.0) as usize;
            let (l, r, _) = run(&mut b, len + 256, 128);
            let (ml, phl) = demod(&l, f, 0, len);
            let (mr, phr) = demod(&r, f, 0, len);
            let dp = ((phl - phr) - (pl - pr) + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU)
                - std::f64::consts::PI;
            println!("{f} Hz: L {ml:.4} (want {al}) R {mr:.4} (want {ar}) phase diff error {dp:.5} rad");
            assert!((ml - al).abs() < 2.0e-3 * al.max(0.1), "{f}: left {ml} vs {al}");
            assert!((mr - ar).abs() < 2.0e-3 * ar.max(0.1), "{f}: right {mr} vs {ar}");
            assert!(dp.abs() < 3.0e-3, "{f}: phase difference off by {dp}");
        }
    }

    /// **Culling removes only what is under the floor and leaves the rest
    /// bit-identical.** A bank with modes spread over 120 dB, culled at
    /// -100 dB, against an unculled twin: the culled ones are exactly the
    /// quiet ones, and after more rendering every survivor's state is the
    /// twin's, bit for bit (found by tag, since the swap moves them).
    #[test]
    fn culling_removes_only_what_is_under_the_floor() {
        let mut b = Bank::new(RATE);
        let mut quiet = Vec::new();
        for i in 0..100u32 {
            let db = -20.0 - 1.2 * f64::from(i);
            let a = 10.0_f64.powf(db / 20.0);
            let f = 80.0 + 37.0 * f64::from(i);
            let m = ModeSpec {
                start: C64::polar(a, 0.3 * f64::from(i)),
                tag: i,
                ..plain(f, 0.2)
            };
            assert!(b.add(&m));
            // loudest reading = |start| (both mics at unit weight)
            if a < f64::from(CULL_FLOOR) {
                quiet.push(i);
            }
        }
        let mut twin = b.clone();
        b.cull(CULL_FLOOR);
        let mut kept: Vec<u32> = (0..b.count()).map(|k| b.tag(k)).collect();
        kept.sort_unstable();
        let want: Vec<u32> = (0..100).filter(|i| !quiet.contains(i)).collect();
        assert_eq!(kept, want, "the cull took the wrong modes");
        run(&mut b, 3000, 128);
        run(&mut twin, 3000, 128);
        for k in 0..b.count() {
            let t = b.tag(k) as usize;
            let (g, j) = (k / LANES, k % LANES);
            let (tg, tj) = (t / LANES, t % LANES);
            assert_eq!(b.y[g][j].to_bits(), twin.y[tg][tj].to_bits(), "mode {t} moved");
            assert_eq!(b.v[g][j].to_bits(), twin.v[tg][tj].to_bits(), "mode {t} moved");
        }
        // And everything past the end is zero, so a partial group computes
        // nothing.
        for k in b.count()..MAX_MODES {
            let (g, j) = (k / LANES, k % LANES);
            assert!(b.y[g][j] == 0.0 && b.v[g][j] == 0.0 && b.b[g][j] == 0.0 && b.h0[g][j] == 0.0);
        }
    }

    /// A short render, a long one, and odd block sizes are the same samples:
    /// the block boundary is invisible.
    #[test]
    fn blocks_of_any_size_are_one_long_render() {
        let mut b = Bank::new(RATE);
        for i in 0..37 {
            let m = ModeSpec {
                left: C64::polar(1.0, 0.1 * f64::from(i)),
                right: C64::polar(0.7, -0.2 * f64::from(i)),
                start: C64::polar(0.01, f64::from(i)),
                ..plain(50.0 + 211.0 * f64::from(i), 1.0 + f64::from(i))
            };
            b.add(&m);
        }
        let mut a = b.clone();
        let mut c = b.clone();
        let (l1, r1, d1) = run(&mut a, 1000, 128);
        let (l2, r2, d2) = run(&mut c, 1000, 37);
        let (l3, r3, d3) = run(&mut b, 1000, 1);
        for i in 0..1000 {
            assert_eq!(l1[i].to_bits(), l2[i].to_bits(), "L at {i}");
            assert_eq!(r1[i].to_bits(), r3[i].to_bits(), "R at {i}");
            assert_eq!(d2[i].to_bits(), d3[i].to_bits(), "drive at {i}");
            assert_eq!(l1[i].to_bits(), l3[i].to_bits());
            assert_eq!(r1[i].to_bits(), r2[i].to_bits());
            assert_eq!(d1[i].to_bits(), d2[i].to_bits());
        }
    }

    /// **Sixty seconds of silence costs what a fresh bank costs.** A bank
    /// of fast-decaying modes left running for a minute without a cull has
    /// every state flushed to an exact zero (no subnormals left to trap on),
    /// and a block of it renders in the time a block of a fresh bank with
    /// the same coefficients does.
    #[test]
    fn a_bank_left_ringing_in_silence_costs_what_a_fresh_one_costs() {
        let make = |start: f64| {
            let mut b = Bank::new(RATE);
            for i in 0..512 {
                let m = ModeSpec {
                    start: C64::polar(start, f64::from(i)),
                    ..plain(60.0 + 31.0 * f64::from(i), 25.0)
                };
                b.add(&m);
            }
            b
        };
        let mut old = make(0.1);
        run(&mut old, 60 * RATE as usize, 128);
        for k in 0..old.count() {
            let (g, j) = (k / LANES, k % LANES);
            assert!(old.y[g][j] == 0.0 && old.v[g][j] == 0.0, "mode {k} still holds {:e}", old.y[g][j]);
        }
        let mut fresh = make(0.0);
        // Interleaved, best of fifteen each: a wall clock on a loaded
        // machine is noise in both directions, and the minimum over trials
        // taken alternately is what neither a preemption nor a frequency
        // step can bias toward one of the two (the first draft timed five
        // of one, then five of the other, and could flake under load).
        let once = |b: &mut Bank| {
            let (mut l, mut r, mut d) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK]);
            let t0 = std::time::Instant::now();
            for _ in 0..200 {
                b.render(&mut l, &mut r, &mut d);
            }
            let t = t0.elapsed().as_secs_f64();
            std::hint::black_box(&l);
            t
        };
        let (mut t_old, mut t_fresh) = (f64::MAX, f64::MAX);
        for _ in 0..15 {
            t_old = t_old.min(once(&mut old));
            t_fresh = t_fresh.min(once(&mut fresh));
        }
        println!("a minute-old bank {:.3} ms, a fresh one {:.3} ms per 200 blocks", t_old * 1e3, t_fresh * 1e3);
        assert!(t_old < 2.0 * t_fresh, "the old bank costs {t_old} against {t_fresh}");
    }

    /// **The contact path drives every mode with the force it is handed.**
    /// One mode, a force pulse of known shape through `render_contact`: the
    /// output is that pulse convolved with the mode's impulse response,
    /// against a direct f64 integration; and the probe reading is the
    /// coordinate's own real part times the probe weight.
    #[test]
    fn a_contact_force_drives_the_mode_as_the_equation_says() {
        let (f, sigma) = (300.0_f64, 3.0_f64);
        let force = C64::new(0.0, -2.0); // -i 2: the RWA shape of a struck mode
        let probe = C64::new(0.5, 0.0);
        let mut b = Bank::new(RATE);
        b.add(&ModeSpec {
            force,
            probe,
            left: C64::polar(0.8, 0.3),
            right: C64::polar(0.6, -0.4),
            start: C64::ZERO,
            ..plain(f, sigma)
        });
        let pulse = |n: usize| -> f32 {
            if n < 96 {
                let x = n as f32 / 96.0;
                40.0 * (std::f32::consts::PI * x).sin().powi(2)
            } else {
                0.0
            }
        };
        let total = 600;
        let (mut l, mut r, mut d) = (vec![0.0; total], vec![0.0; total], vec![0.0; total]);
        let mut seen = Vec::new();
        let mut n = 0usize;
        // Two blocks under the force, the pulse straddling the boundary, so
        // the block's readings are carried across it as well as through it.
        for (a, bnd) in [(0usize, 64usize), (64, 192)] {
            b.render_contact(&mut l[a..bnd], &mut r[a..bnd], &mut d[a..bnd], &mut |now, next_free, comp| {
                seen.push((now, next_free, comp));
                let f = pulse(n);
                n += 1;
                f
            });
        }
        b.render(&mut l[192..320], &mut r[192..320], &mut d[192..320]);
        b.render(&mut l[320..384], &mut r[320..384], &mut d[320..384]);
        // Reference: z' = lambda z + force F, integrated exactly per sample
        // under a zero-order hold, in f64. What the contact is told: the
        // probe now, the probe one free step on, and the probe's response
        // to a newton held over the step.
        let fs = f64::from(RATE);
        let lambda = C64::new(-sigma, std::f64::consts::TAU * f);
        let p = C64::polar((-sigma / fs).exp(), std::f64::consts::TAU * f / fs);
        let zoh = (p - C64::ONE) / lambda;
        let comp_want = (probe * force * zoh).re;
        let mut z = C64::ZERO;
        let mut worst = 0.0_f64;
        let mut worst_u = 0.0_f64;
        let mut worst_c = 0.0_f64;
        for i in 0..384 {
            if i < 192 {
                let (now, next_free, comp) = seen[i];
                worst_u = worst_u
                    .max(((probe * z).re - f64::from(now)).abs())
                    .max(((probe * p * z).re - f64::from(next_free)).abs());
                worst_c = worst_c.max((f64::from(comp) - comp_want).abs() / comp_want.abs());
            }
            z = p * z + force * zoh.scale(f64::from(pulse(i)));
            let lw = (C64::polar(0.8, 0.3) * z).re;
            let rw = (C64::polar(0.6, -0.4) * z).re;
            worst = worst.max((lw - f64::from(l[i])).abs()).max((rw - f64::from(r[i])).abs());
        }
        let scale = (0..384).map(|i| l[i].abs()).fold(0.0_f32, f32::max);
        println!(
            "contact: output error {worst:.2e} against a peak of {scale:.3e}; probe error {worst_u:.2e}; \
             compliance {comp_want:.4e} read to {worst_c:.1e}"
        );
        assert!(worst < 1.0e-5 * f64::from(scale), "the driven mode left the equation by {worst}");
        assert!(worst_u < 1.0e-5 * f64::from(scale), "the probe read {worst_u} off");
        assert!(worst_c < 1.0e-4, "the compliance read {worst_c} off");
    }

    /// Timing, not a test: `cargo test --release timing_of_the_bank --
    /// --ignored --nocapture`. The render's throughput in mode-samples per
    /// second, best of five, for a full bank and a few sizes, with the
    /// contact path beside it.
    #[test]
    #[ignore]
    fn timing_of_the_bank() {
        for &modes in &[64usize, 256, 512, 768] {
            let mut b = Bank::new(RATE);
            for i in 0..modes {
                let m = ModeSpec {
                    left: C64::polar(1.0, 0.1 * i as f64),
                    right: C64::polar(0.7, -0.3 * i as f64),
                    start: C64::polar(0.001, i as f64),
                    ..plain(30.0 + 23.0 * i as f64, 0.5)
                };
                b.add(&m);
            }
            let (mut l, mut r, mut d) = ([0.0; BLOCK], [0.0; BLOCK], [0.0; BLOCK]);
            let blocks = 2000;
            let mut best = f64::MAX;
            for _ in 0..5 {
                let t0 = std::time::Instant::now();
                for _ in 0..blocks {
                    b.render(&mut l, &mut r, &mut d);
                }
                best = best.min(t0.elapsed().as_secs_f64());
            }
            let mut best_c = f64::MAX;
            for _ in 0..3 {
                let t0 = std::time::Instant::now();
                for _ in 0..50 {
                    b.render_contact(&mut l, &mut r, &mut d, &mut |_, _, _| 0.0);
                }
                best_c = best_c.min(t0.elapsed().as_secs_f64());
            }
            std::hint::black_box(&l);
            let ms = (modes * blocks * BLOCK) as f64 / best;
            let mc = (modes * 50 * BLOCK) as f64 / best_c;
            println!(
                "{modes:4} modes: render {:.2} G mode-samples/s; contact {:.2} G mode-samples/s",
                ms / 1e9,
                mc / 1e9
            );
        }
    }
}
