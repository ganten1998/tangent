//! The soundboard: a plate with modes, which is what a piano's body IS.
//!
//! **What this file is for.** A string on its own moves almost no air. What
//! is heard is the board it is strung over, driven through the bridge, and
//! a board is a plate: it has modes, and they are what a piano's body sounds
//! like. The lowest few — roughly 50 to 250 Hz on a grand — sit far enough
//! apart that the ear hears each one as a resonance: a note that lands on
//! one blooms, a note that lands between two does not, and that unevenness
//! is most of why one instrument's bass reads as woody and another's as
//! boomy. Above a few hundred hertz the modes crowd together until they
//! overlap, and what is left is a dense, smooth colouration that no single
//! resonator can stand for. The four-line FDN in `string::Body` has always
//! been a fair model of THAT region and no model at all of the first,
//! because nothing in it knows a pitch: measured, the whole board sat 17 dB
//! under the strings and turning it off moved the total by a tenth of a
//! decibel. So the work is split. This file is the resolved modes, the FDN
//! stays for the diffuse tail, and `Body::tick` sums the two.
//!
//! **Per instrument, not per voice.** There is one board, every string
//! drives it, and it costs the same whether one key is down or sixty. The
//! sixty resonators run in the time of a few string loops, and the bench
//! pins it (`cost_is_a_fraction_of_the_piano`).
//!
//! **The mode set is a rule, not a list.** Every number in a board follows
//! from four inputs, so the second plate is four more numbers rather than
//! sixty more hand-typed hertz, and a third would be too. From the bottom
//! up:
//!
//! 1. *Where the modes start.* A nine-foot concert grand's board is a third
//!    again the area of a seven-footer's, and a bigger plate's first mode is
//!    lower. Suzuki (JASA 80, 1986) measured a grand's lowest mode near
//!    50 Hz; the smaller plate here starts at 62.
//! 2. *How close they sit.* A plate's average modal density is FLAT in
//!    frequency (Cremer, Heckl & Ungar, *Structure-Borne Sound*, unlike a
//!    room's, which climbs): one mode every so many hertz, the "so many"
//!    set by area over bending stiffness. Ege, Boutillon & Rébillat
//!    (J. Sound Vib. 2013) put an upright's ribbed board at about 0.05
//!    modes per hertz below a kilohertz; the two plates here bracket that
//!    by their area ratio.
//! 3. *Where the ear stops resolving them.* A mode is a mode only while its
//!    neighbours sit outside its own bandwidth. The modal overlap
//!    `M = density × f / Q` passes one a few hundred hertz up, and above
//!    that a resonator per mode buys nothing the ear can separate — so from
//!    that knee the density THINS as 1/f (a fixed number of modes per
//!    octave) and the list stops at 2.4 kHz. Those upper resonators are the
//!    board's colour rather than its ring; the FDN owns everything denser.
//!    Above its knee every plate carries the same modes per octave (the Q
//!    at the knee, by construction), so a larger plate's extra density is
//!    all in the resolved band, which is the band that matters; its total
//!    count can even come out lower, and the concert board's does.
//! 4. *Which hertz exactly.* A real plate's spectrum is not a comb:
//!    neighbouring modes repel, and the gaps between them follow the Wigner
//!    surmise, the same statistics Weaver (JASA 85, 1989) measured in
//!    elastic blocks. The gaps are drawn from that distribution by a
//!    fixed-seed generator per plate, so the set is irregular the way wood
//!    is and identical on every run.
//! 5. *How long each rings.* Ege et al. put a strung board's loss factors
//!    between one and three per cent: Q 32 at the lowest mode here, falling
//!    with frequency as wood's own damping grows (`(f_lo / f)^0.3`), which
//!    is why the first mode rings for a second and a mode at two kilohertz
//!    for tens of milliseconds.
//! 6. *How hard each is driven.* The bridge is a long foot, not a point; it
//!    lands part-way across every mode shape, so no mode is quite on a node
//!    and none quite on an antinode: a drive between half and full, drawn
//!    from the same sequence.
//! 7. *Where each sits in the field.* Two ears over a board hear its first
//!    mode as one lobe, identically; a high mode has many lobes and the two
//!    positions see different ones. So the modes alternate left and right
//!    with a spread that grows from nothing at the first mode to ±35° four
//!    octaves up, constant-power. The board gets its width from its own
//!    shapes rather than from a chorus, and it folds to mono without a comb.
//!
//! **The resonator form.** Not the two-pole of `string::Knock`. At 49 Hz
//! and Q 32 a direct form's coefficients sit a few parts in a million from
//! the unit circle, and `f32` cannot hold that at a pitch: the centre
//! wanders by hertz and the round-off hisses. The topology-preserving
//! state-variable filter (Zavalishin, *The Art of VA Filter Design*, ch. 3)
//! keeps the tuning in `tan(πf/fs)`, a small number `f32` holds well, and
//! is stable for any coefficients at all. They are designed in `f64` and
//! cast once. The filters are laid out as arrays of coefficients and arrays
//! of states rather than an array of filters, so `tick` is one straight
//! loop the compiler vectorises, and the two stereo sums are folded eight
//! lanes at a time for the same reason.

use std::f64::consts::PI;

/// Which grand's board this is. The two differ in size and in nothing
/// else: the same spruce and the same losses, one plate larger, so its
/// modes start lower and sit denser where the ear can still tell them apart.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Plate {
    /// The seven-foot instrument `PianoBase::JAZZ` is strung on.
    #[default]
    Jazz,
    /// The nine-foot instrument `PianoBase::CONCERT` is strung on.
    Concert,
}

/// Room for the resonators. A fixed count so the state is a handful of
/// flat arrays; `tick` runs the lanes a plate designed rounded up to a
/// multiple of eight (`lanes`), so the fold below stays whole and the
/// bound is a loop invariant the compiler can still vectorise across. The
/// first draft ran all sixty-four on the argument that an unused lane
/// costs the same as a used one; measured, the fifty-six the plates use
/// cost a fifth less (23 ns a sample against 30), which is real money on
/// a bill that is mostly this loop.
const MAX_MODES: usize = 64;

/// The four inputs a plate is designed from.
struct Spec {
    /// The lowest mode, in hertz (rule 1).
    f_lo: f64,
    /// Modes per hertz while they are still resolved (rule 2).
    density: f64,
    /// Q of the lowest mode (rule 5).
    q_lo: f64,
    /// Seed of the gap-and-drive sequence (rules 4 and 6).
    seed: u64,
}

impl Plate {
    fn spec(self) -> Spec {
        match self {
            Plate::Jazz => Spec {
                f_lo: 62.0,
                density: 0.050,
                q_lo: 32.0,
                seed: 0x4A41_5A5A_0000_0001,
            },
            Plate::Concert => Spec {
                f_lo: 49.0,
                density: 0.065,
                q_lo: 32.0,
                seed: 0x434F_4E43_0000_0001,
            },
        }
    }
}

impl Spec {
    /// The knee of rule 3: where the modal overlap `density × f / Q(f)`
    /// reaches one. Solved in closed form from `Q(f) = q_lo (f_lo/f)^s`.
    fn knee(&self) -> f64 {
        (self.q_lo * self.f_lo.powf(Q_SLOPE) / self.density).powf(1.0 / (1.0 + Q_SLOPE))
    }

    /// Q at a frequency (rule 5).
    fn q(&self, f: f64) -> f64 {
        self.q_lo * (self.f_lo / f).powf(Q_SLOPE)
    }
}

/// How fast Q falls with frequency (rule 5).
const Q_SLOPE: f64 = 0.3;
/// The top of the list (rule 3), in hertz.
const F_TOP: f64 = 2400.0;
/// The narrowest and widest gap, in mean gaps (rule 4). The Wigner surmise
/// has a tail on both sides; clamping it keeps a pair from landing inside
/// one bandwidth and keeps a hole from swallowing an octave.
const GAP_MIN: f64 = 0.4;
const GAP_MAX: f64 = 2.2;
/// The quietest drive a mode gets, of the loudest (rule 6).
const DRIVE_MIN: f64 = 0.5;
/// The furthest a mode sits from centre, and how many octaves above the
/// first mode it takes to get there (rule 7).
const SPREAD_MAX: f64 = 35.0 * PI / 180.0;
const SPREAD_OCTAVES: f64 = 4.0;

/// Below this the board is silent as far as anyone counting voices cares.
const RINGING: f32 = 1.0e-5;
/// Below this, once it has been ringing, the state is zeroed outright: what
/// is left is 140 dB down and heading for the denormals.
const SILENT: f32 = 1.0e-7;

/// SplitMix64. The board's irregularity has to come from somewhere, and it
/// has to be the same somewhere on every machine and every run — a test
/// that measured a different plate each time would measure nothing. Three
/// lines of integer arithmetic, no crate, no state beyond one word.
fn uniform(state: &mut u64) -> f64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 11) as f64 * (1.0 / (1u64 << 53) as f64)
}

/// The modal soundboard: up to [`MAX_MODES`] resonators, one plate at a
/// time, driven by the bridge sum and radiating a stereo pair.
pub struct Board {
    plate: Plate,
    rate: f32,
    /// How many lanes the plate designed, for `modes()`.
    n: usize,
    /// How many lanes `tick` runs: `n` rounded up to a multiple of eight,
    /// never zero.
    lanes: usize,
    /// The designed centre frequencies, for the tests and the bench.
    freq: [f32; MAX_MODES],
    /// The state-variable filter's three coefficients, per lane.
    a1: [f32; MAX_MODES],
    a2: [f32; MAX_MODES],
    a3: [f32; MAX_MODES],
    /// Where each lane's band output goes: level, pan and the unity-peak
    /// normalisation folded into one multiply per side.
    gl: [f32; MAX_MODES],
    gr: [f32; MAX_MODES],
    /// The two integrator states, per lane.
    ic1: [f32; MAX_MODES],
    ic2: [f32; MAX_MODES],
    /// A follower on the output, for `ringing()` and for the one-time zero
    /// below `SILENT`.
    env: f32,
    /// Set once the board has been heard since the last zero, so the zero
    /// happens once on the way down rather than every sample of a silence.
    live: bool,
}

impl Board {
    pub fn new(sample_rate: f32, plate: Plate) -> Self {
        let mut b = Board {
            plate,
            rate: if sample_rate.is_finite() { sample_rate.max(8_000.0) } else { 48_000.0 },
            n: 0,
            lanes: 8,
            freq: [0.0; MAX_MODES],
            a1: [0.0; MAX_MODES],
            a2: [0.0; MAX_MODES],
            a3: [0.0; MAX_MODES],
            gl: [0.0; MAX_MODES],
            gr: [0.0; MAX_MODES],
            ic1: [0.0; MAX_MODES],
            ic2: [0.0; MAX_MODES],
            env: 0.0,
            live: false,
        };
        b.design();
        b
    }

    /// Switch plates without allocating — this is reached from `set_patch`,
    /// which runs on the audio thread — by recomputing the coefficients in
    /// place. The ring is zeroed with them, as `Body::set_patch` zeroes its
    /// lines: a patch change is the player choosing a different instrument,
    /// not a strike, and the old board's decay has no business in the new
    /// one. Asking for the plate already fitted does nothing at all, so a
    /// redundant `set_patch` cannot cut a ring short.
    pub fn set_plate(&mut self, plate: Plate) {
        if plate == self.plate {
            return;
        }
        self.plate = plate;
        self.design();
        self.clear();
    }

    /// Rules 1 through 7, in `f64`, into the `f32` lanes.
    fn design(&mut self) {
        let spec = self.plate.spec();
        let rate = f64::from(self.rate);
        let knee = spec.knee();
        let mut seq = spec.seed;
        self.freq = [0.0; MAX_MODES];
        self.a1 = [0.0; MAX_MODES];
        self.a2 = [0.0; MAX_MODES];
        self.a3 = [0.0; MAX_MODES];
        self.gl = [0.0; MAX_MODES];
        self.gr = [0.0; MAX_MODES];
        let mut f = spec.f_lo;
        let mut n = 0;
        while n < MAX_MODES && f <= F_TOP && f < rate * 0.45 {
            // The resonator. `g` is the tuning, `k` the damping; `a1..a3`
            // are the TPT solution of the two-integrator loop, and the band
            // output has a peak gain of Q, which `k` in the level undoes.
            let q = spec.q(f);
            let g = (PI * f / rate).tan();
            let k = 1.0 / q;
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            let a3 = g * a2;
            // Rule 6: how much of the bridge this shape sees.
            let drive = DRIVE_MIN + (1.0 - DRIVE_MIN) * uniform(&mut seq);
            // Rule 7: which side, and how far.
            let side = if n % 2 == 0 { 1.0 } else { -1.0 };
            let octaves = (f / spec.f_lo).log2();
            let spread = SPREAD_MAX * (octaves / SPREAD_OCTAVES).min(1.0);
            let theta = PI / 4.0 + side * spread;
            let level = k * drive;
            self.freq[n] = f as f32;
            self.a1[n] = a1 as f32;
            self.a2[n] = a2 as f32;
            self.a3[n] = a3 as f32;
            self.gl[n] = (level * theta.cos()) as f32;
            self.gr[n] = (level * theta.sin()) as f32;
            n += 1;
            // Rules 2 through 4: the gap to the next mode. Flat density
            // below the knee, 1/f above it, and a Wigner-surmise draw
            // (`P(s) = (π s / 2) e^(-π s² / 4)`, mean one) on top.
            let mean = if f < knee { 1.0 / spec.density } else { f / (spec.density * knee) };
            let u = uniform(&mut seq);
            let s = (-(4.0 / PI) * (1.0 - u).ln()).sqrt().clamp(GAP_MIN, GAP_MAX);
            f += mean * s;
        }
        self.n = n;
        self.lanes = n.div_ceil(8).max(1) * 8;
    }

    /// One sample: the bridge sum in, the board's radiation out, left and
    /// right. The whole lane loop is branch-free on purpose; the two
    /// branches that remain are on the input and on the follower.
    pub fn tick(&mut self, drive: f32) -> (f32, f32) {
        // A NaN in is a NaN in every lane forever; the FDN guards its lines
        // the same way.
        let x = if drive.is_finite() { drive } else { 0.0 };
        let mut bl = [0.0_f32; MAX_MODES];
        let mut br = [0.0_f32; MAX_MODES];
        let lanes = self.lanes.min(MAX_MODES);
        for i in 0..lanes {
            let v3 = x - self.ic2[i];
            let v1 = self.a1[i] * self.ic1[i] + self.a2[i] * v3;
            let v2 = self.ic2[i] + self.a2[i] * self.ic1[i] + self.a3[i] * v3;
            self.ic1[i] = 2.0 * v1 - self.ic1[i];
            self.ic2[i] = 2.0 * v2 - self.ic2[i];
            bl[i] = v1 * self.gl[i];
            br[i] = v1 * self.gr[i];
        }
        let l = fold(&bl[..lanes]);
        let r = fold(&br[..lanes]);
        // The follower: quick up so a fresh strike counts as ringing on the
        // block it lands in, slow down so it tracks the decay rather than
        // the waveform.
        let a = l.abs() + r.abs();
        self.env += (a - self.env) * if a > self.env { 0.1 } else { 0.001 };
        if self.live {
            if !(self.env >= SILENT) {
                // Also the NaN path: a non-finite follower fails the
                // comparison and lands here, and the zero is the recovery.
                self.clear();
            }
        } else if self.env > RINGING {
            self.live = true;
        }
        (l, r)
    }

    /// Whether anything audible is still coming out. Read once per block by
    /// the voice count, so it is a compare, not a scan of the lanes.
    pub fn ringing(&self) -> bool {
        self.env > RINGING
    }

    /// Silence the board outright. The coefficients stay; only the ring goes.
    pub fn clear(&mut self) {
        self.ic1 = [0.0; MAX_MODES];
        self.ic2 = [0.0; MAX_MODES];
        self.env = 0.0;
        self.live = false;
    }

    /// The mode frequencies this plate was designed with, lowest first.
    pub fn modes(&self) -> impl Iterator<Item = f32> + '_ {
        self.freq[..self.n].iter().copied()
    }
}

/// Sum a lane array eight partials at a time. A plain fold is one chain of
/// sixty-four dependent adds, which the compiler may not reorder (the
/// result would differ in the last bit) and so cannot vectorise; eight
/// independent partials are eight chains it can run side by side, and the
/// last seven adds are the only serial part.
#[inline]
fn fold(v: &[f32]) -> f32 {
    let mut acc = [0.0_f32; 8];
    for lane in v.chunks_exact(8) {
        for j in 0..8 {
            acc[j] += lane[j];
        }
    }
    acc.iter().sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: f32 = 48_000.0;

    /// Magnitude at one frequency through a Hann window: the bench's
    /// `analyze::goertzel_hann`, repeated here because `keys/` cannot see
    /// the bench, and for the reason the bench has it — a rectangle's
    /// sidelobes fall as 1/f and bleed a floor under every neighbour;
    /// Hann's fall as 1/f³ and do not.
    fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
        let w = std::f32::consts::TAU * freq / rate;
        let c = 2.0 * w.cos();
        let n = x.len() as f32;
        let step = std::f32::consts::TAU / n;
        let (mut s1, mut s2) = (0.0_f32, 0.0_f32);
        for (i, &v) in x.iter().enumerate() {
            let win = 0.5 - 0.5 * (step * i as f32).cos();
            let s0 = v * win + c * s1 - s2;
            s2 = s1;
            s1 = s0;
        }
        (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / (n * 0.25)
    }

    fn db(v: f32) -> f32 {
        20.0 * v.max(1e-12).log10()
    }

    /// The board's answer to one unit sample, `secs` long.
    fn impulse(plate: Plate, secs: f32) -> (Vec<f32>, Vec<f32>) {
        let mut b = Board::new(RATE, plate);
        let n = (secs * RATE) as usize;
        let (mut l, mut r) = (Vec::with_capacity(n), Vec::with_capacity(n));
        for i in 0..n {
            let (a, c) = b.tick(if i == 0 { 1.0 } else { 0.0 });
            l.push(a);
            r.push(c);
        }
        (l, r)
    }

    /// Full-scale white noise, the same on every run.
    fn noise(seed: &mut u64) -> f32 {
        (uniform(seed) * 2.0 - 1.0) as f32
    }

    /// **Test 1.** An impulse rings at the designed modes: for each of the
    /// lowest six, the level AT the mode against the level midway to its
    /// nearest neighbour on either side (the worse side counts). Three of
    /// the six must clear 12 dB. Not all six, on purpose: the Wigner draw
    /// puts the occasional pair inside two bandwidths of each other, which
    /// is what a real board does too, and a test that forbade it would be
    /// a test against the physics.
    #[test]
    fn an_impulse_rings_at_the_designed_modes() {
        for plate in [Plate::Jazz, Plate::Concert] {
            let (l, r) = impulse(plate, 3.0);
            let mono: Vec<f32> = l.iter().zip(&r).map(|(a, b)| a + b).collect();
            let modes: Vec<f32> = Board::new(RATE, plate).modes().collect();
            const LOW: usize = 6;
            let mut clear = 0;
            for k in 0..LOW {
                let f = modes[k];
                let at = db(goertzel_hann(&mono, f, RATE));
                let mut between = f32::NEG_INFINITY;
                if k > 0 {
                    let mid = 0.5 * (f + modes[k - 1]);
                    between = between.max(db(goertzel_hann(&mono, mid, RATE)));
                }
                let mid = 0.5 * (f + modes[k + 1]);
                between = between.max(db(goertzel_hann(&mono, mid, RATE)));
                let contrast = at - between;
                println!(
                    "{plate:?} mode {k}: {f:7.2} Hz  at {at:6.1} dB  between {between:6.1} dB  contrast {contrast:5.1} dB"
                );
                if contrast >= 12.0 {
                    clear += 1;
                }
            }
            assert!(
                clear >= 3,
                "{plate:?}: only {clear} of the lowest {LOW} modes stand 12 dB above their gaps"
            );
        }
    }

    /// **Test 2.** The two plates differ as claimed — the concert board's
    /// first mode is lower and it has at least as many modes below 250 Hz —
    /// and `set_plate` lands on exactly the design `new` would have made,
    /// which is the promise `set_patch` relies on.
    #[test]
    fn the_plates_differ_as_claimed() {
        let jazz: Vec<f32> = Board::new(RATE, Plate::Jazz).modes().collect();
        let concert: Vec<f32> = Board::new(RATE, Plate::Concert).modes().collect();
        let below = |m: &[f32]| m.iter().filter(|&&f| f < 250.0).count();
        println!(
            "jazz: {} modes, {:.1}..{:.1} Hz, {} below 250 Hz",
            jazz.len(),
            jazz[0],
            jazz[jazz.len() - 1],
            below(&jazz)
        );
        println!(
            "concert: {} modes, {:.1}..{:.1} Hz, {} below 250 Hz",
            concert.len(),
            concert[0],
            concert[concert.len() - 1],
            below(&concert)
        );
        for m in [&jazz, &concert] {
            assert!((30..=MAX_MODES).contains(&m.len()), "{} modes", m.len());
            assert!(m.windows(2).all(|w| w[1] > w[0]), "modes out of order");
        }
        assert!(concert[0] < jazz[0], "the concert board should start lower");
        assert!(below(&concert) >= below(&jazz), "the concert board should be denser");

        let mut switched = Board::new(RATE, Plate::Jazz);
        switched.set_plate(Plate::Concert);
        let after: Vec<f32> = switched.modes().collect();
        assert_eq!(after, concert, "set_plate and new disagree on the design");
        switched.set_plate(Plate::Jazz);
        let back: Vec<f32> = switched.modes().collect();
        assert_eq!(back, jazz, "set_plate cannot find its way back");
    }

    /// **Test 3.** Five seconds of full-scale broadband input stays finite
    /// and bounded, and once it stops the decay ENDS: `ringing()` is false
    /// within four seconds. The bound is stated: the lowest concert mode at
    /// Q 32 has a T60 of 1.4 s, and the way from its driven level to the
    /// silence threshold is about ninety decibels.
    #[test]
    fn full_scale_noise_stays_bounded_and_decays() {
        for plate in [Plate::Jazz, Plate::Concert] {
            let mut b = Board::new(RATE, plate);
            let mut seed = 0x5EED;
            let mut peak = 0.0_f32;
            for _ in 0..(5.0 * RATE) as usize {
                let (l, r) = b.tick(noise(&mut seed));
                assert!(l.is_finite() && r.is_finite(), "{plate:?} went non-finite");
                peak = peak.max(l.abs()).max(r.abs());
            }
            assert!(b.ringing(), "{plate:?} is not ringing under full-scale drive");
            assert!(peak < 8.0, "{plate:?} peaked at {peak}");
            let mut quiet = None;
            for i in 0..(6.0 * RATE) as usize {
                let (l, r) = b.tick(0.0);
                assert!(l.is_finite() && r.is_finite(), "{plate:?} went non-finite in decay");
                if !b.ringing() {
                    quiet = Some(i as f32 / RATE);
                    break;
                }
            }
            println!("{plate:?}: peak {peak:.3} under noise, silent {quiet:?} s after it stops");
            let quiet = quiet.expect("the board never stopped ringing");
            assert!(quiet < 4.0, "{plate:?} rang for {quiet} s after the input stopped");
        }
    }

    /// **Test 4.** The cost, in nanoseconds per sample, against a stated
    /// budget.
    ///
    /// The budget, worked from the brief's ceiling: the piano runs 64
    /// voices of three string loops at 78× realtime. At 48 kHz a sample
    /// period is 20,833 ns, so the piano at its ceiling spends 267 ns of
    /// it — 4.2 ns a voice, 1.4 ns a string loop. The brief's "0.3 %" for
    /// sixty biquads is 0.3 % of the PERIOD (a nanosecond a biquad), which
    /// is 62.5 ns per sample, and that is the number pinned here. Set
    /// against the piano itself it is a quarter of the ceiling cost, paid
    /// once per instrument rather than per voice — not nothing, which is
    /// why the loop is laid out to vectorise and why this test exists.
    ///
    /// Only meaningful in a release build; a debug build prints and
    /// returns, because the bench is only ever run with `--release`.
    #[test]
    fn cost_is_a_fraction_of_the_piano() {
        const BUDGET_NS: f64 = 62.5;
        let mut b = Board::new(RATE, Plate::Concert);
        let mut seed = 0xC0_57;
        for _ in 0..RATE as usize {
            std::hint::black_box(b.tick(noise(&mut seed)));
        }
        let n = 10 * RATE as usize;
        let input: Vec<f32> = (0..n).map(|_| noise(&mut seed)).collect();
        let t = std::time::Instant::now();
        let mut acc = 0.0_f32;
        for &x in &input {
            let (l, r) = b.tick(x);
            acc += l - r;
        }
        let ns = t.elapsed().as_nanos() as f64 / n as f64;
        std::hint::black_box(acc);
        println!(
            "board: {ns:.1} ns per sample for {MAX_MODES} lanes (budget {BUDGET_NS} ns = 0.3 % of a 48 kHz sample period)"
        );
        if cfg!(debug_assertions) {
            return;
        }
        assert!(ns < BUDGET_NS, "{ns:.1} ns per sample is over the {BUDGET_NS} ns budget");
    }

    /// **Test 5.** The two outputs differ — this is a board with a width,
    /// not a mono board sent to both sides — and they are not opposites
    /// either, because a board that cancelled in mono would be a board
    /// nobody could record.
    #[test]
    fn the_board_is_stereo_and_folds_to_mono() {
        for plate in [Plate::Jazz, Plate::Concert] {
            let (l, r) = impulse(plate, 1.0);
            let mut diff = 0.0_f64;
            let mut sum = 0.0_f64;
            let mut total = 0.0_f64;
            for (a, b) in l.iter().zip(&r) {
                diff += f64::from((a - b).abs());
                sum += f64::from((a + b).abs());
                total += f64::from(a.abs() + b.abs());
            }
            let width = diff / total;
            let mono = sum / total;
            println!("{plate:?}: |L-R|/(|L|+|R|) = {width:.3}, |L+R|/(|L|+|R|) = {mono:.3}");
            assert!(width > 0.05, "{plate:?} is a mono board twice ({width:.3})");
            assert!(mono > 0.5, "{plate:?} cancels in mono ({mono:.3})");
        }
    }
}
