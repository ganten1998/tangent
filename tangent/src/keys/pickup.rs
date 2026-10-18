//! The transducer: a magnet, or a charged plate, listening to a piece of metal
//! move. Three instruments share it; none of them share a vibrating thing.
//!
//! **Why a derivative and not a waveshaper.** What Tangent shipped for the
//! tine was `x / (1 - s·x)³` applied to the displacement — a saturator with
//! the right even-harmonic flavour at one swing and no idea what a pickup
//! actually is. A pickup is three things, and every one of them is missing
//! from that expression:
//!
//! - **A static characteristic `f`.** Flux through the coil, or capacitance
//!   of the plate, as a function of where the metal is. For the two electric
//!   pianos it is NOT monotonic: the Rhodes tine's tip swings *across* the
//!   magnet's axis and the Wurlitzer reed swings *through* the plate's plane,
//!   so both pass an extremum of `f` and both produce two events per
//!   mechanical cycle — Pfeifle's measured direct-outs show it plainly, and a
//!   monotonic saturator can never make a second event. The clavinet's
//!   string vibrates above a magnet it never crosses, so its `f` really is
//!   monotonic, which is why that instrument's dirt is a different and
//!   gentler colour and must stay one.
//! - **A rest offset `x₀`.** The tine-height screw on a Rhodes, the reed
//!   centring procedure on a Wurlitzer. It is a physical quantity with a
//!   physical range and it belongs on a knob, which is why it is a setter
//!   here and not a constant.
//! - **A time derivative.** Faraday's law for the coil, the charge amplifier
//!   for the plate. It is a real 6 dB/octave tilt and a 90° shift, and it is
//!   why a measured Wurlitzer output is spiky where the reed's motion is a
//!   clean sine. The `evidence/pickup.py` tables that this file's tests
//!   reproduce were made with exactly this: drive `f` with a sinusoid,
//!   differentiate, read harmonics.
//!
//! **Who owns what.** This file is the transducer and nothing else. It does
//! not know what is vibrating, how it was struck or how it decays; it is
//! handed a displacement once per sample by the mechanism that does, and it
//! answers with what the coil would put out. The three curves are data on
//! one structure because the three instruments should differ by their
//! transducers and not — as they do today — only by their modal banks.
//!
//! **Real-time contract.** No allocation, no locking, nothing but arithmetic
//! on the audio thread. `calibrate` does a few hundred evaluations of `f` and
//! is meant for strike time, not the sample loop.

/// The nearest the rest position may sit to the centre of the characteristic,
/// in gaps (the curve's own scale).
///
/// **Trap 1: a centred rest position plays an octave.** With `x₀ = 0` the
/// transfer of either electric piano is an even function, the output is
/// `∝ x·ẋ`, and the fundamental vanishes: `pickup.py` measured H2 at +55 dB
/// above H1 at every swing — and that figure is only the analysis window's
/// leakage floor; measured cleanly the fundamental is simply absent. The
/// Wurlitzer 200/200A service manual describes the same failure from the
/// other end — "One or More Notes That Are Extra Loud or Harsh … check to see
/// if the reed involved is slightly off center in the pickup or electrode" —
/// and its remedy for a reed that is still too loud is to "bend the ends of
/// the pickup up slightly (1/32″ to 1/16″)", which is to say: change the gap.
///
/// **Why 0.2 and not smaller.** Measured with the tests below at the lightest
/// touch this instrument makes (a swing of a tenth of a gap): at 0.1 gap the
/// octave is only 6 dB under the fundamental and OVERTAKES it (+3.6 dB) at a
/// mezzo-piano swing of 0.3, on both the capacitive and the magnetic curve —
/// that is the manual's harsh reed. At 0.2 gap the octave sits 13–14 dB under
/// at the lightest touch and still 3–4 dB under at 0.3, so the note keeps its
/// pitch at every dynamic and the regulation knob keeps its whole bright end.
/// The brief's reference regulation of 0.5 gap is two and a half times this
/// floor; it is a guard against a knob or a default landing on the centre,
/// not a limiter on the knob's range.
pub const OFFSET_FLOOR: f32 = 0.2;

/// Where the rest position sits until the knob says otherwise: the brief's
/// reference regulation, and the offset every table in `pickup.py` was
/// measured at. A transducer that has not been told anything is voiced the
/// way the evidence was.
const OFFSET_DEFAULT: f32 = 0.5;

/// How many intervals the swing range is divided into when the normalisation
/// is measured. The characteristics here are smooth on the scale of a gap;
/// 256 points over the whole excursion places the true peak of `|f'|` to
/// within a few parts per hundred thousand, which is more than enough for a
/// level and cheap enough for a strike.
const CALIBRATION_STEPS: usize = 256;

/// The half-step of the central difference that measures `|f'|`, in gaps.
/// Small enough that the second-order error is ~10⁻⁵, large enough that the
/// two flux values differ by far more than an `f32` can lose subtracting them.
const SLOPE_STEP: f32 = 0.01;

/// Points per period in [`Transducer::coil_spectrum`]. A power of two is not
/// needed (it is a plain sum, not an FFT); sixty-four keeps a strike's two
/// spectra at 256 flux evaluations, under a microsecond.
const SPECTRUM_POINTS: usize = 64;

/// The shallowest slope the normalisation will divide by. A characteristic
/// flatter than this across the whole swing is a plate parked so far from the
/// metal that it hears nothing, and the right output of a plate that hears
/// nothing is zero — not the NaN that `0 × (1/0)` would put into the body
/// filter after it. A guard against arithmetic, not a limiter.
const SLOPE_FLOOR: f32 = 1e-9;

/// The lowest note frequency, in radians per sample, the derivative will be
/// referred to. A consumer that hands us zero — an unpitched voice, a
/// half-initialised one — gets the same finite answer as one at 0.008 Hz
/// instead of a division by zero on the audio thread.
const OMEGA_FLOOR: f32 = 1e-6;

/// The static characteristic: what the coil or the plate sees as a function
/// of where the metal is. Plain data, `Copy`, chosen by the instrument.
///
/// Every curve is dimensionless and of order one, and every one has a single
/// length that sets its scale — the gap for the electrics, the decay length
/// for the clavinet. Displacements, offsets and [`OFFSET_FLOOR`] are all
/// measured in that length, so "half a gap" means the same thing for every
/// instrument that uses this file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Curve {
    /// Wurlitzer: the capacitance of a reed leaving the plane of the pickup
    /// plate, `C(x) = 1 / (1 + (x/gap)²)`. Even, and so non-monotonic — the
    /// reed passes through the plate's plane and the capacitance peaks there.
    Capacitive { gap: f32 },
    /// Rhodes: the flux `Δz / |r|³` of a tine tip riding an arc of radius
    /// `arc` past the pole, Pfeifle's eqs. (6) and (8). The gap opens as the
    /// tip swings away — `Δz = gap + (arc − √(arc² − x²))` — and
    /// `r = √(Δz² + x²)`. Non-monotonic: the tip crosses the pole's axis.
    /// `pickup.py` measures this with `z0 = 1`, `r = 6`, which is
    /// `gap: 1.0, arc: 6.0` here.
    Magnetic { gap: f32, arc: f32 },
    /// Clavinet: the flux `exp(−x / scale)` of a string vibrating above a
    /// magnet whose axis it never crosses. Monotonic, and meant to stay so:
    /// this is the curve that CANNOT make a second event per cycle.
    Exponential { scale: f32 },
}

impl Curve {
    /// The one length this curve is measured in.
    fn scale(self) -> f32 {
        match self {
            Curve::Capacitive { gap } => gap,
            Curve::Magnetic { gap, .. } => gap,
            Curve::Exponential { scale } => scale,
        }
    }

    /// `f(x)` at the absolute position `x` — no offset applied. The
    /// characteristic is a property of the geometry; the offset is a property
    /// of the regulation, and it is added by [`Transducer`], not here.
    fn flux(self, x: f32) -> f32 {
        match self {
            Curve::Capacitive { gap } => {
                let u = x / gap;
                1.0 / (1.0 + u * u)
            }
            Curve::Magnetic { gap, arc } => {
                // The tip cannot leave its arc, and neither may the square
                // root: past the arc's end the gap simply stays fully open.
                // `pickup.py` clamps the same quantity the same way.
                let chord = (arc * arc - x * x).max(1e-9);
                let dz = gap + (arc - chord.sqrt());
                let r2 = dz * dz + x * x;
                dz / (r2 * r2.sqrt())
            }
            Curve::Exponential { scale } => (-x / scale).exp(),
        }
    }

    /// `f(x)` at the absolute position `x`, for an instrument that reads the
    /// characteristic in a closed form of its own and takes its reference
    /// values from the curve itself: the reed's plate is read rest-referred,
    /// `C(x₀ ± x) − C(x₀)` without cancellation, and its rest value `C(x₀)`
    /// comes from here (`reed::Plate`).
    pub fn at(self, x: f32) -> f32 {
        self.flux(x)
    }
}

/// One pickup: a curve, a regulation, a normalisation, and the one sample of
/// memory a derivative needs.
#[derive(Clone, Copy, Debug)]
pub struct Transducer {
    curve: Curve,
    /// The rest offset, already clamped by [`OFFSET_FLOOR`].
    x0: f32,
    /// The swing the normalisation was last measured for, kept so that
    /// moving the offset can re-measure without being told the swing again.
    swing: f32,
    /// `1 / max|f'|` over the calibrated excursion. See [`Self::calibrate`].
    norm: f32,
    /// `f(x₀ + x)` from the previous sample.
    prev: f32,
    /// Whether `prev` is a real sample or the rest value. See
    /// [`Self::clear`] for why this is a flag and not a sentinel.
    primed: bool,
}

impl Transducer {
    /// A pickup at the brief's reference regulation, normalised for a swing
    /// of one gap. Never in a state where `tick` produces garbage: a consumer
    /// that forgets to regulate or calibrate gets the evidence's voicing.
    pub fn new(curve: Curve) -> Self {
        debug_assert!(curve.scale() > 0.0, "a pickup needs a positive gap");
        if let Curve::Magnetic { arc, .. } = curve {
            debug_assert!(arc > 0.0, "a tine tip needs a positive arc");
        }
        let scale = curve.scale();
        let mut t = Transducer {
            curve,
            x0: OFFSET_DEFAULT * scale,
            swing: scale,
            norm: 1.0,
            prev: 0.0,
            primed: false,
        };
        t.calibrate(scale);
        t
    }

    /// Regulate: where the metal rests relative to the centre of the
    /// characteristic, in the same units as the displacement handed to
    /// [`Self::tick`]. Clamped away from zero by [`OFFSET_FLOOR`] — see there
    /// for the octave that lives at the centre. The sign survives the clamp:
    /// a reed regulated slightly below the plate stays below it.
    ///
    /// Re-measures the normalisation for the last calibrated swing, so the
    /// level never depends on the ORDER a consumer regulates and calibrates
    /// in: an instrument that reads the knob after it has worked out the
    /// swing is voiced exactly as one that reads it before.
    pub fn set_offset(&mut self, x0: f32) {
        let floor = OFFSET_FLOOR * self.curve.scale();
        self.x0 = if x0.abs() < floor {
            floor.copysign(x0)
        } else {
            x0
        };
        let swing = self.swing;
        self.calibrate(swing);
    }

    /// Measure the normalisation for a note that is about to swing `swing`
    /// either side of the rest position.
    ///
    /// **Trap 2: the compensation must be computed from the transfer.** The
    /// tine's old pickup divided by `(1 − u_max)³` — the gain at the swing's
    /// EXTREME — so that BARK would add dirt rather than level, and that
    /// algebra is only right for a monotonic curve, where the extreme is also
    /// the steepest point. On the curves in this file the steepest point sits
    /// somewhere inside the excursion, and normalising by the slope at the
    /// extreme — which goes to zero as the swing grows — turns the level knob
    /// upside down. That has happened twice already; the header of `tine.rs`
    /// records both. So the normalisation here is the one thing that cannot
    /// be wrong about where the peak is: the largest `|f'|` found anywhere in
    /// `[x₀ − swing, x₀ + swing]`, sampled at [`CALIBRATION_STEPS`] points,
    /// measured on `f` itself by a central difference rather than on a
    /// hand-derived formula that could disagree with the curve it claims to
    /// describe. With it, a sinusoid at the note's frequency comes out no
    /// louder than its own displacement, whatever the swing and whatever the
    /// curve, and the tests hold that as an inequality.
    pub fn calibrate(&mut self, swing: f32) {
        let swing = swing.abs();
        self.swing = swing;
        let h = SLOPE_STEP * self.curve.scale();
        let mut steepest = 0.0_f32;
        for i in 0..=CALIBRATION_STEPS {
            let s = -swing + 2.0 * swing * (i as f32 / CALIBRATION_STEPS as f32);
            let x = self.x0 + s;
            let slope = (self.curve.flux(x + h) - self.curve.flux(x - h)).abs() / (2.0 * h);
            steepest = steepest.max(slope);
        }
        self.norm = 1.0 / steepest.max(SLOPE_FLOOR);
    }

    /// The static characteristic `f` at absolute position `x`, offset not
    /// applied — for tests, for a consumer whose wave variable is already a
    /// velocity and wants the curve without the derivative (the clav
    /// reading Gabrielli's way, which is how `pickup.py` measures that row),
    /// and for the clav's coils, which advance their flux a sample at a
    /// time and re-take it exactly from here once a block (`clav::Coil`).
    pub fn flux(&self, x: f32) -> f32 {
        self.curve.flux(x)
    }

    /// One sample: the derivative of the flux at `x₀ + x`.
    ///
    /// `out = (f(now) − f(previous)) · norm / omega0_per_sample`, where
    /// `omega0_per_sample` is `2π·f₀ / rate` for the NOTE being played.
    ///
    /// **The derivative is physics; the division is voicing.** A coil's
    /// output really is the time derivative of its flux, so a backward
    /// difference is the honest thing to compute, and its 6 dB/octave tilt
    /// across the partials of ONE note is the sound. But taken literally the
    /// same tilt runs across the KEYBOARD too: C6 would come out sixteen
    /// times louder than C2 for the same swing, and every level curve this
    /// module has tuned by ear would have to be re-fought. Dividing by the
    /// note's own angular frequency refers the derivative to the fundamental
    /// instead, so a small sinusoid at the note's frequency comes out at
    /// about the displacement's amplitude — same as the waveshaper it
    /// replaces — and the tilt applies to a note's partials relative to its
    /// fundamental rather than to the note relative to its neighbours. What
    /// we lose is a real instrument's tendency to get brighter and louder up
    /// the keyboard; that is a per-patch decision the level curves already
    /// make, and this file declines to make it a second time.
    #[allow(dead_code)] // The normalised read: this file's tests pin it; the tine reads `tick_unnormalised` and the reed's plate the closed form (`Curve::at`).
    pub fn tick(&mut self, x: f32, omega0_per_sample: f32) -> f32 {
        let now = self.curve.flux(self.x0 + x);
        if !self.primed {
            self.prev = self.curve.flux(self.x0);
            self.primed = true;
        }
        let out = (now - self.prev) * self.norm / omega0_per_sample.max(OMEGA_FLOOR);
        self.prev = now;
        out
    }

    /// `tick` without the normalisation: `(f(now) − f(previous)) / omega0`,
    /// the coil referred to the note's fundamental and nothing else.
    ///
    /// **For a consumer that owns its level.** `norm` is re-measured at
    /// every `calibrate`, and a consumer that recalibrates under a ringing
    /// note (a strike landing on one) steps its output by the ratio of the
    /// two norms in one sample. One that sets its own output gain at the
    /// strike — and can glide it — reads the coil raw and multiplies by its
    /// own number; [`Self::coil_spectrum`] is measured in the same raw units,
    /// so the two agree by construction. Same memory and the same `primed`
    /// rule as `tick`; the two may not be mixed on one note.
    pub fn tick_unnormalised(&mut self, x: f32, omega0_per_sample: f32) -> f32 {
        let now = self.curve.flux(self.x0 + x);
        if !self.primed {
            self.prev = self.curve.flux(self.x0);
            self.primed = true;
        }
        let out = (now - self.prev) / omega0_per_sample.max(OMEGA_FLOOR);
        self.prev = now;
        out
    }

    /// The fundamental's amplitude and the RMS of the whole output that
    /// [`Self::tick_unnormalised`] would produce for a sinusoid of `swing`
    /// about the current rest position: one period sampled at
    /// [`SPECTRUM_POINTS`] points, the coil read as `f′(x₀ + s·sin θ)·s·cos θ`
    /// (the derivative referred to ω₀, which is what the backward difference
    /// converges to at a note's own frequency), `f′` by the same central
    /// difference `calibrate` uses.
    ///
    /// **Strike-time, and the reason it exists.** A consumer that wants a
    /// level law separate from what the swing does to the timbre needs to
    /// know, before the note sounds, how loud this regulation makes this
    /// swing: the RMS is that number, Parseval over every harmonic the curve
    /// makes. Sixty-four points resolve harmonics to the thirty-first,
    /// beyond anything the tables this module carries ask for (the deepest
    /// bark measured is still 10 dB down by the sixteenth). Raw units — the
    /// normalisation is not applied — so it pairs with `tick_unnormalised`.
    pub fn coil_spectrum(&self, swing: f32) -> (f32, f32) {
        self.spectrum_about(self.x0, swing)
    }

    /// [`Self::coil_spectrum`] about a rest position of the caller's choosing,
    /// clamped as [`Self::set_offset`] clamps it, leaving this transducer as it
    /// is: what the coil WOULD put out at another regulation. For a consumer
    /// that is moving its rest position and has to know, at a strike, how the
    /// coil hears a swing both where the tip is and where it is going.
    pub fn coil_spectrum_at(&self, x0: f32, swing: f32) -> (f32, f32) {
        let floor = OFFSET_FLOOR * self.curve.scale();
        let x0 = if x0.abs() < floor { floor.copysign(x0) } else { x0 };
        self.spectrum_about(x0, swing)
    }

    fn spectrum_about(&self, x0: f32, swing: f32) -> (f32, f32) {
        let h = SLOPE_STEP * self.curve.scale();
        let s = swing.abs();
        let (mut re, mut im, mut sq) = (0.0_f32, 0.0_f32, 0.0_f32);
        for i in 0..SPECTRUM_POINTS {
            let th = std::f32::consts::TAU * i as f32 / SPECTRUM_POINTS as f32;
            let (sn, cs) = (super::sine_apart(th), th.cos());
            let x = x0 + s * sn;
            let slope = (self.curve.flux(x + h) - self.curve.flux(x - h)) / (2.0 * h);
            let out = slope * s * cs;
            sq += out * out;
            re += out * cs;
            im += out * sn;
        }
        let n = SPECTRUM_POINTS as f32;
        ((re * re + im * im).sqrt() * 2.0 / n, (sq / n).sqrt())
    }

    /// Forget the previous sample.
    ///
    /// A voice that is reused keeps its transducer, and the transducer keeps
    /// the last flux it saw — which, for a voice that was stolen mid-swing,
    /// can be the far end of the characteristic. The first tick of the new
    /// note would then differentiate from there to rest in one sample: a
    /// spike of the whole excursion's worth of flux over one `omega0`, tens of
    /// times full scale. So the first tick after `clear` differentiates from
    /// the REST value `f(x₀)` — evaluated then, not now, so that a regulation
    /// changed between clear and strike is the one it starts from — which is
    /// the truthful history of a voice that was silent. A flag rather than a
    /// stored rest value for that reason: the rest value is not known until
    /// the strike.
    ///
    /// Not for restrikes. A note struck again while it rings has a
    /// continuous displacement, and its derivative must be continuous too;
    /// the superposition law that governs every other piece of state in this
    /// module governs this one. Clear a transducer only when its voice is
    /// actually silent.
    pub fn clear(&mut self) {
        self.primed = false;
        self.prev = 0.0;
    }

    /// The clamp bypassed, so a test can put the rest position exactly on the
    /// centre and measure the octave that lives there. Test-only by
    /// construction: nothing that ships can reach the centre.
    #[cfg(test)]
    fn set_offset_unclamped(&mut self, x0: f32) {
        self.x0 = x0;
        let swing = self.swing;
        self.calibrate(swing);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const RATE: f32 = 48_000.0;
    const F0: f32 = 220.0;
    /// 220 Hz at 48 kHz repeats every 2400 samples (eleven periods exactly);
    /// 154 periods is the nearest whole number of those to `pickup.py`'s
    /// 32768-sample window.
    const HONEST_N: usize = 33_600;
    /// What `pickup.py` actually uses: 150.19 periods under a rectangular
    /// window, whatever its comment says about Hann.
    const REPLICA_N: usize = 1 << 15;

    fn omega() -> f32 {
        std::f32::consts::TAU * F0 / RATE
    }

    fn wurli() -> Transducer {
        Transducer::new(Curve::Capacitive { gap: 1.0 })
    }

    fn rhodes() -> Transducer {
        Transducer::new(Curve::Magnetic { gap: 1.0, arc: 6.0 })
    }

    fn clav() -> Transducer {
        Transducer::new(Curve::Exponential { scale: 1.0 })
    }

    /// Drive `t` with `swing · sin(2π·f0·t)` for `n` samples through `tick`.
    /// The phase is accumulated in f64 so that the signal generator is not
    /// what limits the measurement.
    fn render(t: &mut Transducer, swing: f32, n: usize, f0: f32) -> Vec<f32> {
        let w = std::f32::consts::TAU * f0 / RATE;
        let w64 = TAU * f0 as f64 / RATE as f64;
        (0..n)
            .map(|i| t.tick(swing * (w64 * i as f64).sin() as f32, w))
            .collect()
    }

    /// The static characteristic driven the same way, WITHOUT a derivative:
    /// `pickup.py`'s clavinet row.
    fn render_flux(t: &Transducer, offset: f32, swing: f32, n: usize) -> Vec<f32> {
        let w64 = TAU * F0 as f64 / RATE as f64;
        (0..n)
            .map(|i| t.flux(offset + swing * (w64 * i as f64).sin() as f32))
            .collect()
    }

    /// Amplitudes of harmonics 1..=k of `F0`, in dB re harmonic 1, by a
    /// Goertzel at each exact harmonic after removing the mean, the way
    /// `pickup.py`'s `harmonics()` does it. `hann` selects the window: on an
    /// integer number of periods a Hann window leaks nothing measurable; a
    /// rectangular one over a non-integer count leaks the fundamental into
    /// every other bin at about −58 dB, which is `pickup.py`'s floor.
    fn harmonics_db(sig: &[f32], k: usize, hann: bool) -> Vec<f64> {
        let n = sig.len();
        let mean = sig.iter().map(|&v| v as f64).sum::<f64>() / n as f64;
        let mut amps = Vec::with_capacity(k);
        for h in 1..=k {
            let w = TAU * F0 as f64 * h as f64 / RATE as f64;
            let (mut re, mut im) = (0.0_f64, 0.0_f64);
            for (i, &s) in sig.iter().enumerate() {
                let win = if hann {
                    0.5 - 0.5 * (TAU * i as f64 / n as f64).cos()
                } else {
                    1.0
                };
                let v = (s as f64 - mean) * win;
                re += v * (w * i as f64).cos();
                im += v * (w * i as f64).sin();
            }
            amps.push(re.hypot(im) * 2.0 / n as f64);
        }
        let h1 = amps[0].max(1e-30);
        amps.iter().map(|a| 20.0 * (a.max(1e-30) / h1).log10()).collect()
    }

    /// One table row: calibrate for the swing, render, read H2..H5.
    fn row(t: &mut Transducer, swing: f32, n: usize, hann: bool) -> [f64; 4] {
        t.clear();
        t.calibrate(swing);
        let sig = render(t, swing, n, F0);
        let db = harmonics_db(&sig, 5, hann);
        [db[1], db[2], db[3], db[4]]
    }

    fn print_row(label: &str, swing: f32, got: [f64; 4], want: [f64; 4]) {
        println!(
            "{label:>7} swing {swing:3.1}  got {:6.1} {:6.1} {:6.1} {:6.1}   brief {:6.1} {:6.1} {:6.1} {:6.1}",
            got[0], got[1], got[2], got[3], want[0], want[1], want[2], want[3]
        );
    }

    /// The brief's §4.1 tables, verbatim.
    const WURLI_TABLE: [(f32, [f64; 4]); 5] = [
        (0.1, [-33.1, -42.6, -66.2, -68.6]),
        (0.3, [-21.8, -23.5, -43.6, -61.9]),
        (0.6, [-11.4, -11.0, -27.8, -34.0]),
        (1.0, [-2.0, -2.0, -28.9, -14.2]),
        (1.6, [6.5, 5.0, -4.9, 0.7]),
    ];
    const RHODES_TABLE: [(f32, [f64; 4]); 4] = [
        (0.1, [-41.2, -41.7, -72.5, -68.5]),
        (0.6, [-16.2, -9.1, -20.8, -33.2]),
        (1.0, [-2.4, 0.5, -16.6, -10.4]),
        (1.6, [7.5, 7.8, -2.7, 5.3]),
    ];
    const CLAV_TABLE: [(f32, [f64; 4]); 3] = [
        (0.1, [-32.0, -64.0, -63.7, -67.3]),
        (0.6, [-16.6, -37.1, -55.5, -70.6]),
        (1.6, [-8.8, -20.8, -34.8, -51.4]),
    ];

    /// The same rows measured CLEANLY: `pickup.py`'s formulas evaluated in
    /// f64, a Hann window, 154 whole periods. Where these differ from the
    /// brief, the brief is reporting its window, not the model — see
    /// [`electric_tables_measured_honestly_pin_the_physics`].
    const WURLI_CLEAN: [(f32, [f64; 4]); 5] = [
        (0.1, [-33.7, -42.8, -70.4, -100.9]),
        (0.3, [-21.9, -23.5, -42.6, -61.1]),
        (0.6, [-11.4, -11.0, -27.6, -33.9]),
        (1.0, [-2.0, -2.0, -28.5, -14.2]),
        (1.6, [6.5, 5.1, -4.9, 0.7]),
    ];
    const RHODES_CLEAN: [(f32, [f64; 4]); 4] = [
        (0.1, [-40.0, -41.9, -64.5, -122.3]),
        (0.6, [-16.3, -9.1, -20.7, -33.1]),
        (1.0, [-2.4, 0.5, -16.5, -10.4]),
        (1.6, [7.6, 7.9, -2.7, 5.3]),
    ];
    const CLAV_CLEAN: [(f32, [f64; 4]); 3] = [
        (0.1, [-32.0, -67.6, -105.7, -145.7]),
        (0.6, [-16.6, -36.7, -59.2, -83.7]),
        (1.6, [-8.8, -20.7, -35.0, -51.1]),
    ];

    /// `pickup.py`'s rectangular window over 150.19 periods leaks the
    /// fundamental into every other bin at about −58 dB re H1. On an entry
    /// this far above that floor the leakage moves the number by under a dB;
    /// below it the brief's figure carries up to a few dB of window and is
    /// not a target an honest measurement can be held to.
    const BRIEF_TRUSTED_ABOVE_DB: f64 = -35.0;

    /// Below this an `f32` finite difference is measuring its own rounding,
    /// so the clean value is held as an upper bound rather than pinned.
    const F32_PIN_ABOVE_DB: f64 = -95.0;

    /// One honest row against both tables. Returns the largest deviation from
    /// the clean reference among the pinned entries, for the report.
    fn check_row(label: &str, swing: f32, got: [f64; 4], brief: [f64; 4], clean: [f64; 4]) -> f64 {
        print_row(label, swing, got, brief);
        let mut worst = 0.0_f64;
        for h in 0..4 {
            let (g, b, c) = (got[h], brief[h], clean[h]);
            if b > BRIEF_TRUSTED_ABOVE_DB {
                assert!(
                    (g - b).abs() <= 1.0,
                    "{label} swing {swing} H{}: got {g:.1} dB, brief {b:.1} dB",
                    h + 2
                );
            }
            if c > F32_PIN_ABOVE_DB {
                assert!(
                    (g - c).abs() <= 1.0,
                    "{label} swing {swing} H{}: got {g:.1} dB, clean reference {c:.1} dB",
                    h + 2
                );
                worst = worst.max((g - c).abs());
            } else {
                assert!(
                    g < F32_PIN_ABOVE_DB,
                    "{label} swing {swing} H{}: got {g:.1} dB where the model has nothing",
                    h + 2
                );
            }
        }
        worst
    }

    /// §4.1 measured the way the brief asks — a Hann window over a whole
    /// number of periods — for the two electric pianos at the brief's
    /// regulation, and held to two references at once.
    ///
    /// **Against the brief's table: 1.0 dB on every entry above −35 dB;
    /// achieved 0.6 dB.** Below that the brief is not the model. Its
    /// `harmonics()` says Hann in a comment and applies no window at all, over
    /// 32768 samples that are 150.19 periods of 220 Hz, so H1 leaks into every
    /// other bin at about −58 dB re H1 and shifts what is there: Rhodes swing
    /// 0.1 H2 reads −41.2 there and is −40.0; Wurli swing 0.3 H4 reads −43.6
    /// and is −42.6; the four swing-0.1 H4/H5 entries (−66.2/−68.6,
    /// −72.5/−68.5) are the floor itself and the model sits at −70/−101 and
    /// −64/−122. The verbatim test below reproduces all of those to 0.1 dB by
    /// using the window that made them.
    ///
    /// **Against the clean reference: 1.0 dB on every entry above −95 dB;
    /// achieved 0.1 dB.** That is the same f64 model under this window, and
    /// it is what a re-baseline of these tables should be measured against.
    #[test]
    fn electric_tables_measured_honestly_pin_the_physics() {
        let cases: [(&str, Transducer, &[(f32, [f64; 4])], &[(f32, [f64; 4])]); 2] = [
            ("wurli", wurli(), &WURLI_TABLE, &WURLI_CLEAN),
            ("rhodes", rhodes(), &RHODES_TABLE, &RHODES_CLEAN),
        ];
        let mut worst = 0.0_f64;
        for (label, mut t, brief, clean) in cases {
            t.set_offset(0.5);
            for (&(swing, want), &(_, truth)) in brief.iter().zip(clean.iter()) {
                let got = row(&mut t, swing, HONEST_N, true);
                worst = worst.max(check_row(label, swing, got, want, truth));
            }
        }
        println!("largest deviation from the clean reference: {worst:.2} dB");
    }

    /// §4.1 reproduced the way `pickup.py` actually measured it — a
    /// rectangular window over 32768 samples — so that even the entries that
    /// are the window's own leakage floor come back, and the reader can be
    /// sure this is the same model and not merely a similar one.
    ///
    /// **Held to 1.0 dB on every entry, including the floor; achieved 0.1.**
    #[test]
    fn electric_tables_reproduce_pickup_py_verbatim_with_its_own_window() {
        let cases: [(&str, Transducer, &[(f32, [f64; 4])]); 2] = [
            ("wurli", wurli(), &WURLI_TABLE),
            ("rhodes", rhodes(), &RHODES_TABLE),
        ];
        for (label, mut t, table) in cases {
            t.set_offset(0.5);
            for &(swing, want) in table {
                let got = row(&mut t, swing, REPLICA_N, false);
                print_row(label, swing, got, want);
                for (h, (g, w)) in got.iter().zip(want.iter()).enumerate() {
                    assert!(
                        (g - w).abs() <= 1.0,
                        "{label} swing {swing} H{}: got {g:.1} dB, pickup.py {w:.1} dB",
                        h + 2
                    );
                }
            }
        }
    }

    /// §4.1's clavinet row is `pickup.py` reading `exp(−x)` with NO
    /// derivative — the waveguide's wave variable is already a velocity — so
    /// it is `flux()` that must match it, and it does: 1.0 dB against the
    /// brief on every entry above −35 dB (achieved 0.1), 1.0 dB against the
    /// clean reference on every entry above −95 dB (achieved 0.1). The
    /// brief's −55.5 at swing 0.6 H4 is −59.2 without its window, the same
    /// leakage the electric tables carry.
    ///
    /// Separately, what `tick()`'s derivative adds to that row: exactly the
    /// 6 dB/octave tilt, 20·log10(h) on harmonic h — +6.0, +9.5, +12.0 dB on
    /// H2..H4 — held to 0.5 dB. That is the physics a velocity-reading
    /// consumer gets for free and a displacement-reading one gets from here.
    #[test]
    fn clav_row_matches_flux_and_tick_adds_the_six_db_per_octave_tilt() {
        let mut t = clav();
        for (&(swing, want), &(_, truth)) in CLAV_TABLE.iter().zip(CLAV_CLEAN.iter()) {
            let sig = render_flux(&t, 0.0, swing, HONEST_N);
            let db = harmonics_db(&sig, 5, true);
            let got = [db[1], db[2], db[3], db[4]];
            check_row("clav f", swing, got, want, truth);

            // The offset does not matter to a monotonic exponential — it is a
            // constant factor the normalisation removes — so tick() at the
            // clamped default reads the same shape, tilted.
            let tilted = row(&mut t, swing, HONEST_N, true);
            print_row("clav d", swing, tilted, got);
            for h in 0..3 {
                let expect = 20.0 * ((h + 2) as f64).log10();
                let added = tilted[h] - got[h];
                assert!(
                    (added - expect).abs() <= 0.5,
                    "clav swing {swing} H{}: the derivative added {added:.2} dB, expected {expect:.2}",
                    h + 2
                );
            }
        }
    }

    /// Trap 1, as a measurement: with the rest position exactly on the
    /// centre the fundamental of either electric piano is gone and the octave
    /// stands tens of dB above it — `pickup.py` said +55 dB and that was its
    /// window's floor; measured cleanly it is more. Bypasses the clamp to get
    /// there, because nothing else can.
    #[test]
    fn trap1_a_centred_rest_position_plays_an_octave() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes())] {
            t.set_offset_unclamped(0.0);
            for swing in [0.1, 0.6] {
                let got = row(&mut t, swing, HONEST_N, true);
                println!("{label:>7} centred, swing {swing:3.1}: H2 re H1 = {:+.1} dB", got[0]);
                assert!(
                    got[0] > 40.0,
                    "{label} centred at swing {swing}: H2 only {:+.1} dB over H1; the octave should dominate",
                    got[0]
                );
            }
        }
    }

    /// Trap 1, as the guard: `set_offset` holds `|x₀| ≥ OFFSET_FLOOR · gap`,
    /// keeps the sign, and leaves a legal regulation alone. And at the floor
    /// itself the note still has its pitch: the octave sits more than 10 dB
    /// under the fundamental at the lightest touch on both curves.
    #[test]
    fn trap1_the_offset_clamp_holds_and_the_floor_keeps_its_pitch() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            let floor = OFFSET_FLOOR * t.curve.scale();
            t.set_offset(0.0);
            assert!(t.x0 >= floor, "{label}: offset 0 was not lifted to the floor ({})", t.x0);
            t.set_offset(0.05);
            assert!(t.x0 >= floor, "{label}: offset 0.05 was not lifted to the floor ({})", t.x0);
            t.set_offset(-0.05);
            assert!(t.x0 <= -floor, "{label}: a negative offset lost its sign or its floor ({})", t.x0);
            t.set_offset(0.5);
            assert_eq!(t.x0, 0.5, "{label}: a legal regulation was moved");
        }
        // A gap that is not 1.0 scales the floor with it.
        let mut wide = Transducer::new(Curve::Capacitive { gap: 4.0 });
        wide.set_offset(0.0);
        assert_eq!(wide.x0, OFFSET_FLOOR * 4.0);

        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes())] {
            t.set_offset(0.0); // lands on the floor
            let got = row(&mut t, 0.1, HONEST_N, true);
            println!("{label:>7} at the floor, swing 0.1: H2 re H1 = {:+.1} dB", got[0]);
            assert!(
                got[0] < -10.0,
                "{label} at the floor: the octave is only {:+.1} dB under the fundamental",
                got[0]
            );
        }
    }

    /// The peak of one period of `tick()` for a calibrated swing.
    fn calibrated_peak(t: &mut Transducer, swing: f32) -> f32 {
        t.clear();
        t.calibrate(swing);
        render(t, swing, 2400, F0)
            .iter()
            .fold(0.0_f32, |m, v| m.max(v.abs()))
    }

    const SWINGS: [f32; 5] = [0.1, 0.3, 0.6, 1.0, 1.6];

    /// Trap 2, the brief's own check: after `calibrate(swing)` the peak
    /// output rises MONOTONICALLY with the swing on every curve. This is the
    /// assertion that caught BARK turning into an inverse volume knob twice
    /// before, and it is here so it cannot happen a third time quietly.
    ///
    /// **It is not sufficient on its own, and we checked.** Normalising by
    /// the slope at the swing's EXTREME — the old algebra, `calibrate` with
    /// `steepest = slope` in place of the max — passes this test: the peaks
    /// still climb, 0.10 → 0.32 → 0.86 → 2.28 → 7.23 on the Wurlitzer and up
    /// to 16.8 on the Rhodes, because on these curves the slope at the
    /// extreme falls faster than the swing grows and the level knob runs
    /// AWAY rather than backwards. What that break fails is the bound in
    /// [`trap2_calibrated_peak_is_bounded_by_the_swing`]. Dropping the
    /// `.abs()` from the slope fails both.
    #[test]
    fn trap2_calibrated_peak_rises_monotonically_with_swing() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            t.set_offset(0.5);
            let peaks: Vec<f32> = SWINGS.iter().map(|&s| calibrated_peak(&mut t, s)).collect();
            println!("{label:>7} peaks over swing {SWINGS:?}: {peaks:?}");
            for w in peaks.windows(2) {
                assert!(
                    w[1] > w[0],
                    "{label}: peak fell from {} to {} as the swing grew: {peaks:?}",
                    w[0],
                    w[1]
                );
            }
        }
    }

    /// Trap 2, the tight form: a calibrated sinusoid at the note's frequency
    /// never comes out louder than its own displacement — that is what
    /// normalising by the largest `|f'|` over the ACTUAL excursion means, and
    /// it is an inequality the mean value theorem guarantees, so the 2% slack
    /// is the calibration grid and `f32`, nothing else. Nor is it allowed to
    /// vanish: the exponential's steepest point is where the velocity is
    /// zero, so its peak trails the swing, but never below a fifth of it.
    ///
    /// This is the test that catches the old algebra carried over — the
    /// slope at the swing's extreme instead of the max — which the monotonic
    /// check above lets through: revert-checked, it fails here first at the
    /// Wurlitzer's 0.3 swing with a peak of 0.32.
    #[test]
    fn trap2_calibrated_peak_is_bounded_by_the_swing() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            t.set_offset(0.5);
            for &swing in &SWINGS {
                let peak = calibrated_peak(&mut t, swing);
                assert!(
                    peak <= swing * 1.02,
                    "{label} swing {swing}: peak {peak} exceeds the displacement — the normalisation is not the max of |f'|"
                );
                assert!(
                    peak >= swing * 0.2,
                    "{label} swing {swing}: peak {peak} is a fifth of the displacement or less"
                );
            }
        }
    }

    /// The voicing decision in `tick`'s doc comment, held: the same swing at
    /// 110 Hz and at 880 Hz comes out at the same level, because the
    /// derivative is referred to the note's own fundamental. And at a small
    /// swing that level is about the displacement's amplitude.
    #[test]
    fn level_is_referred_to_the_fundamental_not_the_keyboard() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            t.set_offset(0.5);
            t.calibrate(0.3);
            let mut peak_at = |f0: f32| {
                t.clear();
                render(&mut t, 0.3, 4800, f0)
                    .iter()
                    .fold(0.0_f32, |m, v| m.max(v.abs()))
            };
            let low = peak_at(110.0);
            let high = peak_at(880.0);
            assert!(
                (high / low - 1.0).abs() < 0.01,
                "{label}: 880 Hz peaks at {high}, 110 Hz at {low}; the tilt is running across the keyboard"
            );
        }
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            t.set_offset(0.5);
            let peak = calibrated_peak(&mut t, 0.05);
            assert!(
                peak > 0.05 * 0.85 && peak <= 0.05 * 1.02,
                "{label}: a 0.05 swing came out at {peak}, not about 0.05"
            );
        }
    }

    /// `clear()` forgets the previous sample, so the first tick of a reused
    /// voice differentiates from REST — exactly `(f(x₀ + x) − f(x₀))·norm/ω`,
    /// which for a 0.001 step is about 0.03 — and not from wherever the
    /// stolen voice happened to be, which is the whole excursion's flux over
    /// one `omega0`: measured 33, tens of times full scale.
    #[test]
    fn clear_forgets_the_previous_sample_so_the_first_tick_does_not_spike() {
        for (label, mut t) in [("wurli", wurli()), ("rhodes", rhodes()), ("clav", clav())] {
            t.set_offset(0.5);
            t.calibrate(1.6);
            // Park the transducer at the far end of a fortissimo swing.
            t.tick(0.0, omega());
            t.tick(1.6, omega());
            // Ask what a fresh, tiny displacement produces without clearing,
            // then with.
            let mut stale = t;
            let spike = stale.tick(0.001, omega()).abs();
            t.clear();
            let first = t.tick(0.001, omega());
            let from_rest = (t.flux(0.5 + 0.001) - t.flux(0.5)) * t.norm / omega();
            println!(
                "{label:>7}: first tick after clear {first:.3e} (from rest {from_rest:.3e}), without clear {spike:.2e}"
            );
            assert!(spike > 1.0, "{label}: the un-cleared spike is only {spike}; the test has no teeth");
            assert!(
                (first - from_rest).abs() < 1e-6,
                "{label}: the first tick after clear() is {first}, not the {from_rest} a step from rest makes"
            );
        }
    }

    /// A regulation changed between clear and strike is the one the first
    /// tick starts from, because the rest value is read at the strike.
    #[test]
    fn clear_then_regulate_starts_from_the_new_rest() {
        let mut t = wurli();
        t.set_offset(0.9);
        t.tick(0.0, omega());
        t.clear();
        t.set_offset(0.3);
        let first = t.tick(0.0, omega());
        assert_eq!(first, 0.0, "the first tick differentiated from the OLD rest value");
    }

    /// `coil_spectrum` is what `tick_unnormalised` puts out: the same
    /// sinusoid rendered through the coil and measured by a Hann DFT over
    /// whole periods, at the inflection, on the flank and in saturation,
    /// fundamental and RMS both within 1 %. (The tine's level law divides by
    /// this number; if the two disagreed, every level in that file would be
    /// off by the disagreement.)
    #[test]
    fn coil_spectrum_is_what_the_raw_coil_puts_out() {
        for (x0, swing) in [(0.4675_f32, 0.2_f32), (0.25, 0.3), (0.25, 0.9), (0.55, 1.6)] {
            let mut t = rhodes();
            t.set_offset(x0);
            t.calibrate(swing);
            let (h1, rms) = t.coil_spectrum(swing);
            t.clear();
            let w = std::f32::consts::TAU * F0 / RATE;
            let w64 = TAU * F0 as f64 / RATE as f64;
            let sig: Vec<f32> = (0..HONEST_N).map(|i| t.tick_unnormalised(swing * (w64 * i as f64).sin() as f32, w)).collect();
            let tail = &sig[2400..];
            let m = tail.iter().map(|&v| f64::from(v)).sum::<f64>() / tail.len() as f64;
            let got_rms = (tail.iter().map(|&v| (f64::from(v) - m).powi(2)).sum::<f64>() / tail.len() as f64).sqrt();
            let n = tail.len();
            let (mut re, mut im) = (0.0_f64, 0.0_f64);
            for (i, &v) in tail.iter().enumerate() {
                let win = 0.5 - 0.5 * (TAU * i as f64 / n as f64).cos();
                re += f64::from(v) * win * (w64 * i as f64).cos();
                im += f64::from(v) * win * (w64 * i as f64).sin();
            }
            let got_h1 = re.hypot(im) * 4.0 / n as f64;
            println!("x0 {x0} swing {swing}: h1 {h1:.5} against {got_h1:.5}, rms {rms:.5} against {got_rms:.5}");
            assert!((f64::from(h1) / got_h1 - 1.0).abs() < 0.01 && (f64::from(rms) / got_rms - 1.0).abs() < 0.01);
        }
    }

    /// Two arithmetic guards, each with the line that removes it. A note
    /// frequency of zero must not divide by zero (`OMEGA_FLOOR`), and a plate
    /// parked so far away that the characteristic is flat to `f32` must put
    /// out nothing, not `0 × ∞` (`SLOPE_FLOOR`). "Nothing" is the last bit of
    /// a flux of 10⁻¹² times a normalisation of 10⁹: measured 3×10⁻⁸.
    #[test]
    fn a_zero_frequency_and_a_flat_characteristic_stay_finite() {
        let mut t = wurli();
        t.set_offset(0.5);
        t.calibrate(0.3);
        t.tick(0.0, 0.0);
        let out = t.tick(0.1, 0.0);
        assert!(out.is_finite(), "omega 0 produced {out}");

        let mut far = wurli();
        far.set_offset(1.0e6);
        far.calibrate(1.0);
        far.tick(0.0, omega());
        let out = far.tick(0.5, omega());
        assert!(out.is_finite(), "a flat characteristic produced {out}");
        assert!(out.abs() < 1e-6, "a plate that hears nothing put out {out}");
    }

    /// The characteristics themselves, at the points where a reader can check
    /// them by hand: the centre, and one gap out.
    #[test]
    fn the_curves_are_the_formulas_in_the_brief() {
        let w = wurli();
        assert_eq!(w.flux(0.0), 1.0);
        assert!((w.flux(1.0) - 0.5).abs() < 1e-6);
        assert_eq!(w.flux(1.0), w.flux(-1.0), "the capacitive curve is even");

        let r = rhodes();
        assert!((r.flux(0.0) - 1.0).abs() < 1e-6, "dz = gap = 1 at the pole, r = 1");
        assert_eq!(r.flux(0.5), r.flux(-0.5), "the magnetic curve is even");
        // dz = 1 + 6 - sqrt(35), r² = dz² + 1
        let dz = 7.0 - 35.0_f32.sqrt();
        let r2 = dz * dz + 1.0;
        assert!((r.flux(1.0) - dz / (r2 * r2.sqrt())).abs() < 1e-6);

        let c = clav();
        assert_eq!(c.flux(0.0), 1.0);
        assert!((c.flux(1.0) - (-1.0_f32).exp()).abs() < 1e-6);
        assert!(c.flux(0.5) > c.flux(1.0) && c.flux(1.0) > c.flux(1.5), "the exponential is monotonic");
    }
}
