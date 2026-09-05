//! The clamped-free beam: where a stiff bar actually puts its overtones.
//!
//! **Why a solver, when the tine and the reed already carry numbers.** Both
//! of them are a bar clamped at one end, and a clamped bar's overtones are
//! not harmonics: the textbook uniform beam runs 1 : 6.27 : 17.55 : 34.39.
//! Neither instrument is a uniform beam. A Rhodes tine carries a tuning
//! spring on its free end, a Wurlitzer reed carries a blob of solder, and
//! both are filed narrower or thinner toward the tip. Those two facts pull
//! the ratios in OPPOSITE directions, and each instrument file carries a
//! number that is right for a reason its comment gets wrong. The tine's
//! "bell" at 5.6 is not a bell, it is the second mode of a lightly tapered
//! bar. The reed's 4.4 is a heavily tapered reed with no effective tip mass
//! at all, and its comment says the solder blob drags the overtones DOWN. It
//! does the opposite. This file is the correct record, and its guard tests
//! are named so that the two facts cannot be quietly reversed again:
//!
//! - **A tip mass RAISES every ratio.** A mass on the tip cannot follow the
//!   fast modes, so for them it is very nearly a node and they migrate
//!   toward the clamped-pinned set; for the fundamental it is a bob on a
//!   spring, and the fundamental sinks toward that spring-mass frequency.
//!   The gap between them opens. Uniform beam, `f2/f1`: 6.27 at β = 0,
//!   8.38 at β = 0.5, 10.43 at β = 1, 13.69 at β = 2.
//! - **A taper LOWERS every ratio.** Filing the tip removes stiffness as the
//!   cube of the thickness and mass only as the first power, and it removes
//!   them where the fundamental keeps its mass but not its bending — the
//!   fundamental bends at the root and swings at the tip, the higher modes
//!   bend all the way along — so the fundamental gains more than the
//!   overtones do and the ratios close up. Width 0.25 at the tip: 4.88.
//!   Width 0.5 and thickness 0.6: 4.50.
//!
//! A real reed has both, and the blob is biggest in the bass, so the ratio
//! should be highest in the bass and fall toward the treble — the opposite
//! of what the reed file schedules today.
//!
//! **What it computes, and why it can be trusted.** Rayleigh–Ritz on the
//! Euler–Bernoulli clamped-free beam: the strain and kinetic energies of a
//! beam with linearly tapered width `b(x)` and thickness `h(x)` and a point
//! mass `β·ρAL` on its tip are projected onto the first six mode shapes of
//! the UNIFORM beam, which gives a 6 × 6 generalised eigenproblem
//! `K v = ω² M v`. Cholesky on `M` turns it into a symmetric standard one and
//! a cyclic Jacobi diagonalises that. It is a line-for-line port of
//! `evidence/beam.py` in the Keys handoff, which was validated against the
//! exact transcendental for the uniform beam; [`uniform_exact`] IS that
//! transcendental, kept here so the tests can measure the solve against it
//! for as long as the file exists. Six basis functions put `f2` and `f3`
//! within 0.3 % of exact across β ∈ [0, 2] and `f4` within 0.7 %; that
//! truncation, not the quadrature, is the floor, and it is a tenth of a
//! semitone on partials the pickup barely sees.
//!
//! **Real-time contract.** This may be called from `Keys::set_patch`, which
//! runs on the audio thread, so nothing here allocates: every matrix is a
//! fixed array and the quadrature is a fixed number of panels. A call is a
//! few tens of microseconds in release — `a_call_costs_well_under_a_millisecond`
//! prints the number — which is cheap enough to call once per patch or once
//! per key. It is not cheap enough to call once per sample, and nothing
//! should.

/// Basis functions, and so the size of every matrix. Six is beam.py's `N`:
/// the fourth ratio is the highest anyone asks for, and a Ritz solve is
/// only trustworthy in roughly its lower half.
const N: usize = 6;

/// Midpoint-rule panels for the energy integrals along the beam.
///
/// **Two hundred, measured, not two thousand.** beam.py uses 2000 and does
/// not need them: the same rule integrates both `K` and `M`, so most of its
/// error cancels in the eigenvalue ratio. Against a 40 000-panel reference
/// the worst deviation over the whole test table was 0.09 % at 50 panels,
/// 0.02 % at 100, 0.005 % at 200 and 0.0001 % at 2000 — all far below the
/// 0.3 % that six basis functions cost. Two hundred keeps a call in the tens
/// of microseconds with a hundred times more margin than the tests need.
const M: usize = 200;

/// The roots of `1 + cos λ cosh λ = 0`: the uniform clamped-free beam's own
/// eigenvalues, which shape the basis. Hard-coded because finding them costs
/// more than the whole solve; `the_basis_roots_are_the_roots` checks them
/// against the root finder behind [`uniform_exact`] to nine digits, which is
/// about what `cosh 17` leaves of a double.
const LAM: [f64; N] = [
    1.875_104_068_711_961_3,
    4.694_091_132_974_174,
    7.854_757_438_237_613,
    10.995_540_734_875_465,
    14.137_168_391_046_469,
    17.278_759_532_088_237,
];

/// The heaviest tip the solve is asked for. Sixteen times the bar's own
/// mass is a tuning fork on a cannonball and the ratios are past forty;
/// beyond it the answer is "a spring and a weight", not a beam.
const BETA_MAX: f64 = 16.0;

/// Tapers outside this range are not a bar that was filed, they are a
/// different object — and a tip thinned to nothing is where the mass matrix
/// stops being something Cholesky should be handed.
const TAPER_MIN: f64 = 0.05;
const TAPER_MAX: f64 = 4.0;

/// The first four modes of a clamped-free beam relative to its first, as
/// frequency ratios `f_n / f_1`. The fundamental is 1 by definition and is
/// not carried.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BeamRatios {
    pub f2: f32,
    pub f3: f32,
    pub f4: f32,
}

/// The first four modes of a clamped-free Euler–Bernoulli beam relative to
/// its first, for a tip mass ratio `beta` = M_tip / (ρ·A·L) and linear
/// tapers of width and thickness from root to tip (1.0 = uniform, 0.5 = the
/// tip is half the root).
///
/// Inputs are clamped to the range the solve is trusted over and a NaN is
/// read as "no tip mass, no taper" rather than propagated — a knob that has
/// gone wrong must not become a NaN in a resonator's frequency.
pub fn clamped_free(beta: f32, width_taper: f32, thickness_taper: f32) -> BeamRatios {
    let beta = sane(beta, 0.0, BETA_MAX, 0.0);
    let tw = sane(width_taper, TAPER_MIN, TAPER_MAX, 1.0);
    let th = sane(thickness_taper, TAPER_MIN, TAPER_MAX, 1.0);
    let ev = ritz(beta, tw, th);
    debug_assert!(ev[0] > 0.0, "a clamped beam has no zero mode: {ev:?}");
    // The eigenvalues are ω²; a frequency ratio is the square root of their ratio.
    let ratio = |n: usize| (ev[n] / ev[0]).sqrt() as f32;
    BeamRatios {
        f2: ratio(1),
        f3: ratio(2),
        f4: ratio(3),
    }
}

/// The uniform beam by the exact transcendental
/// `1 + cos λ cosh λ + β λ (cos λ sinh λ − sin λ cosh λ) = 0`, with
/// `f_n / f_1 = (λ_n / λ_1)²`, for the tests to check the Ritz solve against.
///
/// Slower than [`clamped_free`] and only a uniform beam, so it is an oracle
/// and not the instrument's path; it is public so the tests of whatever
/// consumes the ratios can hold the same yardstick.
#[allow(dead_code)]
pub fn uniform_exact(beta: f32) -> BeamRatios {
    let beta = sane(beta, 0.0, BETA_MAX, 0.0);
    let r = roots_uniform::<4>(beta);
    let ratio = |n: usize| ((r[n] / r[0]) * (r[n] / r[0])) as f32;
    BeamRatios {
        f2: ratio(1),
        f3: ratio(2),
        f4: ratio(3),
    }
}

/// A finite value inside its range, or the default for one that is not.
fn sane(v: f32, lo: f64, hi: f64, dflt: f64) -> f64 {
    let v = f64::from(v);
    if v.is_finite() {
        v.clamp(lo, hi)
    } else {
        dflt
    }
}

/// The characteristic function of the uniform beam with a tip mass.
fn char_uniform(lam: f64, beta: f64) -> f64 {
    let (s, c) = lam.sin_cos();
    let (sh, ch) = (lam.sinh(), lam.cosh());
    1.0 + c * ch + beta * lam * (c * sh - s * ch)
}

/// The first `K` roots of [`char_uniform`], by a scan for sign changes and
/// a bisection inside each. The roots sit a few units apart, so a step of a
/// hundredth cannot straddle two, and a hundred halvings is more than a
/// double can use.
fn roots_uniform<const K: usize>(beta: f64) -> [f64; K] {
    let mut out = [0.0; K];
    let mut found = 0;
    let step = 1e-2;
    let mut x = 1e-4;
    let mut prev = char_uniform(x, beta);
    while found < K && x < 40.0 {
        let x2 = x + step;
        let cur = char_uniform(x2, beta);
        if prev * cur < 0.0 {
            let (mut a, mut b) = (x, x2);
            for _ in 0..100 {
                let mid = 0.5 * (a + b);
                if char_uniform(a, beta) * char_uniform(mid, beta) <= 0.0 {
                    b = mid;
                } else {
                    a = mid;
                }
            }
            out[found] = 0.5 * (a + b);
            found += 1;
        }
        prev = cur;
        x = x2;
    }
    debug_assert_eq!(found, K, "the characteristic scan ran out of roots");
    out
}

/// The `n`th uniform mode shape and its second derivative at `x ∈ [0, 1]`,
/// unnormalised: `φ = (cosh λx − cos λx) − σ (sinh λx − sin λx)` with
/// `σ = (cosh λ + cos λ) / (sinh λ + sin λ)`. One `exp` and one `sin_cos`
/// per point; the hyperbolics are assembled from the exponential.
#[inline]
fn shape(n: usize, sigma: f64, x: f64) -> (f64, f64) {
    let l = LAM[n];
    let y = l * x;
    let e = y.exp();
    let ch = 0.5 * (e + 1.0 / e);
    let sh = 0.5 * (e - 1.0 / e);
    let (s, c) = y.sin_cos();
    let phi = (ch - c) - sigma * (sh - s);
    let d2 = l * l * ((ch + c) - sigma * (sh + s));
    (phi, d2)
}

/// The Ritz eigenvalues `ω²` of the tapered, tip-loaded beam, ascending,
/// in units where the uniform beam's first is `λ₁⁴`.
fn ritz(beta: f64, tw: f64, th: f64) -> [f64; N] {
    let mut sigma = [0.0; N];
    for (n, s) in sigma.iter_mut().enumerate() {
        let l = LAM[n];
        *s = (l.cosh() + l.cos()) / (l.sinh() + l.sin());
    }

    // The energy integrals by the midpoint rule, on the RAW basis; beam.py
    // normalises each φ to unit ∫φ² first, and scaling the finished
    // integrals by the same factors is the same arithmetic done once.
    let dx = 1.0 / M as f64;
    let mut k = [[0.0; N]; N];
    let mut m = [[0.0; N]; N];
    let mut nrm = [0.0; N];
    for g in 0..M {
        let x = (g as f64 + 0.5) * dx;
        // Width and thickness, 1 at the root and the taper at the tip.
        let b = 1.0 - (1.0 - tw) * x;
        let h = 1.0 - (1.0 - th) * x;
        // Section area carries the mass; second moment b·h³/12 the stiffness.
        let ax = b * h;
        let ix = b * h * h * h;
        let mut v = [0.0; N];
        let mut d2 = [0.0; N];
        for n in 0..N {
            let (p, q) = shape(n, sigma[n], x);
            v[n] = p;
            d2[n] = q;
            nrm[n] += p * p;
        }
        for i in 0..N {
            for j in i..N {
                k[i][j] += ix * d2[i] * d2[j];
                m[i][j] += ax * v[i] * v[j];
            }
        }
    }
    for s in nrm.iter_mut() {
        *s = 1.0 / (*s * dx).sqrt();
    }

    // The tip mass: β times the basis at x = 1, outer product, added to M.
    // This term is the whole of "a tip mass raises the ratios".
    let mut e1 = [0.0; N];
    for (n, e) in e1.iter_mut().enumerate() {
        *e = shape(n, sigma[n], 1.0).0 * nrm[n];
    }
    for i in 0..N {
        for j in i..N {
            let s = dx * nrm[i] * nrm[j];
            k[i][j] *= s;
            m[i][j] *= s;
            m[i][j] += beta * e1[i] * e1[j];
            k[j][i] = k[i][j];
            m[j][i] = m[i][j];
        }
    }

    // K v = ω² M v  →  with M = L Lᵀ,  (L⁻¹ K L⁻ᵀ) u = ω² u.
    let l = cholesky(&m);
    let mut y = [[0.0; N]; N]; // Y = L⁻¹ K, one column at a time
    for j in 0..N {
        let mut col = [0.0; N];
        for i in 0..N {
            col[i] = k[i][j];
        }
        let col = fsub(&l, col);
        for i in 0..N {
            y[i][j] = col[i];
        }
    }
    let mut c = [[0.0; N]; N]; // C = Y L⁻ᵀ  ⇔  Cᵀ = L⁻¹ Yᵀ
    for i in 0..N {
        let col = fsub(&l, y[i]);
        for j in 0..N {
            c[j][i] = col[j];
        }
    }
    for i in 0..N {
        for j in i + 1..N {
            let a = 0.5 * (c[i][j] + c[j][i]);
            c[i][j] = a;
            c[j][i] = a;
        }
    }
    jacobi_eig(&mut c)
}

/// Cholesky, `A = L Lᵀ`, lower triangle. `A` is a mass matrix and so
/// positive definite by construction; there is no pivoting to do.
fn cholesky(a: &[[f64; N]; N]) -> [[f64; N]; N] {
    let mut l = [[0.0; N]; N];
    for i in 0..N {
        for j in 0..=i {
            let mut s = a[i][j];
            for k in 0..j {
                s -= l[i][k] * l[j][k];
            }
            if i == j {
                l[i][i] = s.sqrt();
            } else {
                l[i][j] = s / l[j][j];
            }
        }
    }
    l
}

/// Forward substitution: `y` with `L y = b`.
fn fsub(l: &[[f64; N]; N], b: [f64; N]) -> [f64; N] {
    let mut y = [0.0; N];
    for i in 0..N {
        let mut s = b[i];
        for k in 0..i {
            s -= l[i][k] * y[k];
        }
        y[i] = s / l[i][i];
    }
    y
}

/// The eigenvalues of a symmetric matrix by cyclic Jacobi rotations,
/// ascending. Destroys its argument.
///
/// beam.py's exit test is an absolute `off < 1e-26`, which a matrix whose
/// entries run to `λ₆⁴ ≈ 9·10⁴` can never meet in double precision, so it
/// quietly does all hundred sweeps every time. The test here is the same
/// one made scale-free — off-diagonal energy under `10⁻²⁸` of the diagonal's
/// — which the rotations reach in about eight sweeps; the hundred-sweep cap
/// stays as the backstop.
fn jacobi_eig(a: &mut [[f64; N]; N]) -> [f64; N] {
    for _ in 0..100 {
        let mut off = 0.0;
        let mut diag = 0.0;
        for i in 0..N {
            diag += a[i][i] * a[i][i];
            for j in i + 1..N {
                off += a[i][j] * a[i][j];
            }
        }
        if off <= 1e-28 * diag {
            break;
        }
        for p in 0..N {
            for q in p + 1..N {
                if a[p][q].abs() < 1e-30 {
                    continue;
                }
                let theta = 0.5 * (a[q][q] - a[p][p]) / a[p][q];
                let sign = if theta >= 0.0 { 1.0 } else { -1.0 };
                let t = sign / (theta.abs() + (theta * theta + 1.0).sqrt());
                let c = 1.0 / (t * t + 1.0).sqrt();
                let s = t * c;
                for row in a.iter_mut() {
                    let (akp, akq) = (row[p], row[q]);
                    row[p] = c * akp - s * akq;
                    row[q] = s * akp + c * akq;
                }
                for k in 0..N {
                    let (apk, aqk) = (a[p][k], a[q][k]);
                    a[p][k] = c * apk - s * aqk;
                    a[q][k] = s * apk + c * aqk;
                }
            }
        }
    }
    let mut ev = [0.0; N];
    for (i, e) in ev.iter_mut().enumerate() {
        *e = a[i][i];
    }
    // Insertion sort: six values, no allocation, no comparator to unwrap.
    for i in 1..N {
        let mut j = i;
        while j > 0 && ev[j - 1] > ev[j] {
            ev.swap(j - 1, j);
            j -= 1;
        }
    }
    ev
}

#[cfg(test)]
mod tests {
    use super::*;

    fn off_pct(got: f32, want: f32) -> f32 {
        ((got - want) / want * 100.0).abs()
    }

    fn within(what: &str, got: f32, want: f32, tol_pct: f32) {
        let off = off_pct(got, want);
        assert!(
            off <= tol_pct,
            "{what}: got {got:.4}, wanted {want:.4}, off by {off:.3} % against a tolerance of {tol_pct} %"
        );
    }

    /// **The transcendental at β = 0 is the textbook beam.** If this fails
    /// the oracle is wrong, and nothing below it means anything.
    #[test]
    fn the_uniform_beam_is_the_textbook_one() {
        let r = uniform_exact(0.0);
        println!("uniform exact, beta 0: f2 {:.4}  f3 {:.4}  f4 {:.4}", r.f2, r.f3, r.f4);
        within("f2/f1", r.f2, 6.267, 0.05);
        within("f3/f1", r.f3, 17.547, 0.05);
        within("f4/f1", r.f4, 34.386, 0.05);
    }

    /// The hard-coded basis roots are what the root finder finds. Nine
    /// digits: `cosh 17` is `1.6·10⁷`, and that is how much of a double the
    /// characteristic function spends before the bisection sees a sign.
    #[test]
    fn the_basis_roots_are_the_roots() {
        let found = roots_uniform::<N>(0.0);
        for n in 0..N {
            let off = ((found[n] - LAM[n]) / LAM[n]).abs();
            assert!(off < 1e-9, "root {n}: table {} vs found {}", LAM[n], found[n]);
        }
    }

    /// **The Ritz solve against the transcendental, uniform beam, β from 0
    /// to 2.** `f2` and `f3` within 0.5 %. `f4` within 1 %: with six basis
    /// functions the fourth eigenvalue is the least converged, and at β = 2
    /// beam.py itself sits 0.62 % high — the truncation is the method's, not
    /// the port's, and `the_port_agrees_with_beam_py` pins the port.
    #[test]
    fn the_ritz_solve_matches_the_transcendental() {
        println!(" beta   f2 ritz  exact    f3 ritz  exact    f4 ritz  exact");
        for beta in [0.0, 0.25, 0.5, 1.0, 2.0] {
            let rr = clamped_free(beta, 1.0, 1.0);
            let ex = uniform_exact(beta);
            println!(
                " {beta:4.2}   {:7.3} {:7.3}   {:7.3} {:7.3}   {:7.3} {:7.3}",
                rr.f2, ex.f2, rr.f3, ex.f3, rr.f4, ex.f4
            );
            within(&format!("beta {beta} f2"), rr.f2, ex.f2, 0.5);
            within(&format!("beta {beta} f3"), rr.f3, ex.f3, 0.5);
            within(&format!("beta {beta} f4"), rr.f4, ex.f4, 1.0);
        }
    }

    /// **The brief's §3.2B table**, which is the transcendental at four
    /// tip masses. These are the numbers wave B schedules a tuning spring by.
    #[test]
    fn the_tip_mass_table_of_the_brief() {
        for (beta, f2, f3, f4) in [
            (0.25, 7.208, 21.406, 43.423),
            (0.5, 8.382, 25.641, 52.600),
            (1.0, 10.435, 32.682, 67.552),
            (2.0, 13.694, 43.557, 90.429),
        ] {
            let r = uniform_exact(beta);
            within(&format!("beta {beta} f2"), r.f2, f2, 0.05);
            within(&format!("beta {beta} f3"), r.f3, f3, 0.05);
            within(&format!("beta {beta} f4"), r.f4, f4, 0.05);
        }
    }

    /// **The brief's §3.4A taper table**, within 1.5 % of its rounded
    /// numbers. These are the numbers that put a 4.4 on a reed.
    #[test]
    fn the_taper_table_of_the_brief() {
        println!("   tw    th   beta    f2/f1   f3/f1   f4/f1");
        for (tw, th, beta, f2, f3) in [
            (1.0, 1.0, 0.0, 6.27, 17.55),
            (0.25, 1.0, 0.0, 4.88, 12.64),
            (0.5, 0.6, 0.0, 4.50, 11.40),
            (0.5, 1.0, 0.5, 8.45, 25.6),
            (0.5, 0.6, 1.0, 10.98, 32.8),
        ] {
            let r = clamped_free(beta, tw, th);
            println!(" {tw:4.2}  {th:4.2}  {beta:4.2}   {:6.3}  {:6.3}  {:6.3}", r.f2, r.f3, r.f4);
            within(&format!("tw {tw} th {th} beta {beta} f2"), r.f2, f2, 1.5);
            within(&format!("tw {tw} th {th} beta {beta} f3"), r.f3, f3, 1.5);
        }
    }

    /// **The port against beam.py at its own 2000 panels**, to four decimals
    /// over every case the brief and the gates use, and one past them. Any
    /// slip in the basis, the quadrature, the tip term or the eigensolver
    /// shows here first, at a hundredth of the physics tolerances.
    #[test]
    fn the_port_agrees_with_beam_py() {
        for (beta, tw, th, f2, f3, f4) in [
            (0.0, 1.0, 1.0, 6.2669, 17.5475, 34.3861),
            (0.25, 1.0, 1.0, 7.2108, 21.4466, 43.6224),
            (0.5, 1.0, 1.0, 8.3872, 25.7026, 52.8871),
            (1.0, 1.0, 1.0, 10.4419, 32.7702, 67.9543),
            (2.0, 1.0, 1.0, 13.7047, 43.6818, 90.9927),
            (0.0, 0.25, 1.0, 4.8817, 12.6432, 24.1938),
            (0.0, 0.5, 0.6, 4.4955, 11.3975, 21.7157),
            (0.5, 0.5, 1.0, 8.4529, 25.6348, 52.8342),
            (1.0, 0.5, 0.6, 10.9798, 32.8164, 67.8078),
            (4.0, 0.25, 0.6, 22.9734, 68.6036, 141.9114),
        ] {
            let r = clamped_free(beta, tw, th);
            let what = format!("beta {beta} tw {tw} th {th}");
            within(&format!("{what} f2"), r.f2, f2, 0.05);
            within(&format!("{what} f3"), r.f3, f3, 0.05);
            within(&format!("{what} f4"), r.f4, f4, 0.05);
        }
    }

    /// **A tip mass RAISES f2/f1** — and f3, and f4 — monotonically. This is
    /// the fact the reed file's comment has backwards. If this goes red the
    /// tip term in `ritz` has changed sign, or a solder blob has started
    /// dragging ratios down, and it must not.
    #[test]
    fn a_tip_mass_raises_f2_over_f1() {
        let light = clamped_free(0.0, 1.0, 1.0);
        let heavy = clamped_free(1.0, 1.0, 1.0);
        assert!(
            heavy.f2 > light.f2,
            "a tip mass must raise f2/f1: beta 1 gave {:.3}, beta 0 gave {:.3}",
            heavy.f2,
            light.f2
        );
        let mut prev = light;
        for beta in [0.25, 0.5, 1.0, 2.0, 4.0] {
            let r = clamped_free(beta, 1.0, 1.0);
            assert!(r.f2 > prev.f2 && r.f3 > prev.f3 && r.f4 > prev.f4, "not monotone at beta {beta}: {r:?} after {prev:?}");
            prev = r;
        }
    }

    /// **A taper LOWERS f2/f1** — width, thickness, and both together lower
    /// than either. This is what actually put the reed's overtone at 4.4.
    /// If this goes red the taper runs the wrong way along the bar.
    #[test]
    fn a_taper_lowers_f2_over_f1() {
        let uniform = clamped_free(0.0, 1.0, 1.0);
        let narrow = clamped_free(0.0, 0.5, 1.0);
        let thin = clamped_free(0.0, 1.0, 0.6);
        let both = clamped_free(0.0, 0.5, 0.6);
        assert!(narrow.f2 < uniform.f2, "a width taper must lower f2/f1: {:.3} vs uniform {:.3}", narrow.f2, uniform.f2);
        assert!(thin.f2 < uniform.f2, "a thickness taper must lower f2/f1: {:.3} vs uniform {:.3}", thin.f2, uniform.f2);
        assert!(both.f2 < narrow.f2 && both.f2 < thin.f2, "both tapers must lower f2/f1 below either: {both:?}");
        assert!(narrow.f3 < uniform.f3 && thin.f3 < uniform.f3, "the tapers must lower f3/f1 too");
    }

    /// The knob can hand over anything. Whatever it hands over, the answer
    /// is finite, ordered, and above one — a resonator gets a frequency.
    #[test]
    fn extremes_and_garbage_stay_finite_and_ordered() {
        for (beta, tw, th) in [
            (0.0, TAPER_MIN as f32, TAPER_MIN as f32),
            (BETA_MAX as f32, TAPER_MIN as f32, TAPER_MIN as f32),
            (BETA_MAX as f32, TAPER_MAX as f32, TAPER_MAX as f32),
            (0.0, TAPER_MAX as f32, 1.0),
            (-3.0, 0.0, 99.0),
            (f32::NAN, f32::INFINITY, f32::NEG_INFINITY),
            (1e30, 1e-30, 1.0),
        ] {
            let r = clamped_free(beta, tw, th);
            assert!(r.f2.is_finite() && r.f3.is_finite() && r.f4.is_finite(), "{beta} {tw} {th}: {r:?}");
            assert!(1.0 < r.f2 && r.f2 < r.f3 && r.f3 < r.f4, "{beta} {tw} {th}: {r:?}");
        }
        let nan = clamped_free(f32::NAN, f32::NAN, f32::NAN);
        assert_eq!(nan, clamped_free(0.0, 1.0, 1.0), "a NaN must read as the plain beam");
    }

    /// **What a call costs**, in release, printed so the number is on the
    /// record whenever the bench runs. The gate is a millisecond; the
    /// expectation is tens of microseconds, and anything that consumes this
    /// per key should read the printed figure before deciding when to call.
    #[test]
    fn a_call_costs_well_under_a_millisecond() {
        use std::hint::black_box;
        use std::time::Instant;
        let inputs = [(0.0, 1.0, 1.0), (0.5, 0.5, 1.0), (1.0, 0.5, 0.6), (2.0, 0.25, 0.6)];
        for &(b, w, t) in &inputs {
            black_box(clamped_free(b, w, t));
        }
        let mut best = f64::MAX;
        for _ in 0..5 {
            let t0 = Instant::now();
            for _ in 0..50 {
                for &(b, w, t) in &inputs {
                    black_box(clamped_free(black_box(b), black_box(w), black_box(t)));
                }
            }
            let per_call = t0.elapsed().as_secs_f64() * 1e6 / (50.0 * inputs.len() as f64);
            best = best.min(per_call);
        }
        println!("clamped_free: {best:.1} us per call (best of 5 x 200, M = {M} panels, N = {N})");
        assert!(best < 1000.0, "clamped_free costs {best:.0} us, which is not well under a millisecond");
    }
}
