//! The DSP for Tangent Imager: mid/side width, a mono floor, a velvet
//! ensemble, and the metering arithmetic the panel will draw.
//!
//! # The one guarantee everything here is built on
//!
//! **Nothing but a scalar ever happens to the mid.** Every control acts on
//! the side signal only, and the output is rebuilt as `L = g·(M + S')`,
//! `R = g·(M - S')` — so the mono fold `(L + R) / 2` returns `g·M`, the `M`
//! that went in and not one sample of anything else, as an arithmetic
//! identity rather than a calibration. No comb, no cancellation, no phase
//! damage: whatever the knobs did, folding this unit down cannot find a hole
//! in the mix. A widener that survives the mono fold of a phone speaker, a
//! club PA or a broadcast chain is the whole product; the tests pin it at
//! every corner of the parameter space.
//!
//! `g` is [`ensemble_trim`], and it is exactly one at every setting except an
//! open ensemble, which is why the shipped default is still a bit-exact wire.
//! See that function for why an ensemble cannot be free. The important thing about it is that it is COMMON: the stereo
//! pair and the mono fold get the same number, so the relationship between
//! what a stereo listener hears and what a mono listener hears is exactly
//! what the arithmetic makes it, and no trim could ever change that. What the
//! trim chooses is only where the pair sits on a meter, and the answer an
//! insert has to give is "where it was".
//!
//! # The three controls
//!
//! **Width** scales S. **Mono-below** collapses S under a corner, because the
//! low end is where out-of-phase energy costs the most — a cutting lathe
//! rejects it and a subwoofer sums it to cancellation — and every mastering
//! chain wants it gone without touching the image above. **Ensemble**
//! manufactures decorrelated side energy out of the MID, by a sparse
//! velvet-noise convolution — width for material that has no side to widen,
//! without a detuned pitch or a comb.
//!
//! # Why the ensemble is fed the mid, which is the whole reason it works
//!
//! Most of what this unit is pointed at is one microphone. A mono source
//! reaches the desk as `L == R` — `mix_channels` says so in as many words,
//! "a mono input goes to both sides" — and a pair with `L == R` has a side of
//! exactly zero. Width scaling zero is zero, the floor folding zero is zero,
//! and an ensemble convolving zero is zero: on the commonest material in the
//! app EVERY control here was arithmetically a wire, and the vectorscope drew
//! the vertical line to prove it.
//!
//! So the decorrelator is fed the mid. The mid is the one thing present on
//! every record, mono or wide, and building the side out of it costs the
//! guarantee above nothing: the output is `M ± S'` whatever `S'` was made
//! from, so inventing a side cannot move the fold. Feeding the velvet the
//! side instead — the signal the control exists to CREATE — was a circular
//! definition that happened to be non-zero only on material that did not need
//! the control.

use std::f32::consts::{FRAC_1_SQRT_2, PI};

// ── what a person can change ────────────────────────────────────────────────

/// What the width knob spans, as a gain on S: 0 % to 200 %, unity in the
/// middle of the dial.
///
/// **200 % is where every hardware width pot stops**, and for a reason: past
/// twice the recorded side the image does not get wider, only the fold-down
/// penalty gets worse — S that dominates M is anti-phase energy a mono
/// listener pays for and a stereo listener cannot place.
pub const WIDTH_GAIN: (f32, f32) = (0.0, 2.0);

/// Where the mono-below corner sweeps, in Hz, once the knob leaves zero.
///
/// 60 Hz is the bottom of the band vinyl cutting always demanded mono and PA
/// systems still do; 500 Hz is where "protect the low end" ends and "narrow
/// the mix" begins. Geometric across the dial, like every frequency sweep in
/// `effects.rs`.
pub const MONO_BELOW_HZ: (f32, f32) = (60.0, 500.0);

/// Everything about the imager a person can change.
///
/// Every field is 0..=1, and the mapping to whatever the DSP actually wants
/// lives next to the DSP — the `effects::Params` convention.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImagerParams {
    /// Side gain across [`WIDTH_GAIN`]. 0.5 is unity — the shipped position,
    /// and a perfect null.
    pub width: f32,
    /// How much manufactured side to ADD to whatever the record already had.
    /// 0 is the record as it came; 1 puts a decorrelated copy of the mid
    /// alongside the mid at the mid's own level, which is a correlation of
    /// zero — the widest a pair can be and still fold to mono untouched. The
    /// dial in between is linear in that correlation reading, not in gain;
    /// see [`ensemble_law`]. [`VELVET_DENSITY_HZ`] is what the velvet is.
    pub ensemble: f32,
    /// The mono floor: 0 is off, the rest sweeps the corner across
    /// [`MONO_BELOW_HZ`]. Below the corner the side is collapsed; the mid is
    /// never touched, so the fold is free.
    pub mono_below: f32,
}

impl Default for ImagerParams {
    /// **The shipped sound is no sound at all.** Unity width, no ensemble, no
    /// floor: an imager defaults to a wire, because the first thing anybody
    /// does with an image tool is listen for what it ruins.
    fn default() -> Self {
        Self {
            width: 0.5,
            ensemble: 0.0,
            mono_below: 0.0,
        }
    }
}

impl ImagerParams {
    /// Clamp every field into 0..=1, with NaN going to that field's DEFAULT
    /// rather than to an arbitrary midpoint — a NaN `ensemble` that woke up
    /// at 0.5 would switch a machine on that nobody touched.
    pub fn sane(&self) -> Self {
        let field = |v: f32, rest: f32| {
            if v.is_finite() {
                v.clamp(0.0, 1.0)
            } else {
                rest
            }
        };
        Self {
            width: field(self.width, 0.5),
            ensemble: field(self.ensemble, 0.0),
            mono_below: field(self.mono_below, 0.0),
        }
    }
}

// ── the velvet ──────────────────────────────────────────────────────────────

/// How many velvet impulses land per second, and how long the kernel runs.
///
/// **Velvet noise is almost all silence**: a handful of ±1 impulses, one per
/// grid cell, jittered inside their cells. Convolving the side with it smears
/// the side's energy across 25 ms with no pitch shift and no periodicity —
/// which is why it decorrelates without the detune of a chorus or the comb of
/// a single delay. 1500 taps a second is comfortably above the density where
/// sparse noise stops sounding like crackle and starts sounding like noise;
/// 25 ms keeps the whole smear inside the precedence window, so the ear fuses
/// it into width rather than hearing an early reflection.
const VELVET_DENSITY_HZ: f64 = 1_500.0;
const VELVET_LEN_MS: f64 = 25.0;

/// One tap per grid cell: 37 cells of 1/1500 s fit in 25 ms. Spelled as a
/// literal so it can size arrays; the test derives it from the two constants
/// above and fails if they drift apart.
const VELVET_TAPS: usize = 37;

/// How far into its cell a tap may wander, as a fraction of the cell.
///
/// Short of 1.0 on purpose: with a whole cell of jitter two neighbours can
/// land on the same sample and merge, which quietly changes the tap count the
/// tests pin. 0.95 keeps every pair at least 1.4 samples apart at 44.1 kHz.
const VELVET_JITTER: f64 = 0.95;

/// How far the kernel's envelope has fallen by its last tap, in dB.
///
/// A rectangular velvet kernel ends with a cliff, and a 25 ms cliff on a
/// transient reads as a slapback. An exponential decay is the shape of every
/// diffuse tail the ear already accepts; 24 dB across the kernel is deep
/// enough that the edge disappears and shallow enough that the late taps
/// still buy decorrelation.
const VELVET_TAIL_DB: f64 = 24.0;

/// The velvet's seed — FIXED, never randomised.
///
/// The same seed builds the same kernel in every instance at every sample
/// rate, so a bounce rendered twice is the same file twice, and the two
/// speakers of an A/B never disagree about what "ensemble" means.
const VELVET_SEED: u32 = 0x1DEA_5EED;

/// The ensemble dial's taper: the gain on the manufactured side, given the
/// knob.
///
/// **The dial is linear in the CORRELATION METER, not in gain**, because the
/// correlation meter is the number the panel prints under the goniometer and
/// a knob whose reading moves in equal steps is a knob somebody can aim. For
/// a mid `M` and a manufactured side `kM` that is decorrelated from it, the
/// meter reads `(1 − k²) / (1 + k²)`; asking that to equal `1 − e` and
/// solving for `k` gives this. It lands where the ends have to land — nothing
/// at 0, unity at 1 — and walks +1.00, +0.75, +0.50, +0.25, 0.00 across the
/// quarters in between.
///
/// A quarter-sine would have been the equal-power crossfade this control used
/// to be, and it is the wrong law now: there is no crossfade left to keep
/// constant, and it overshoots the correlation it should be reading by 0.17
/// in the middle of the travel.
///
/// # An ensemble is not free, and [`ensemble_trim`] is the bill
///
/// The crossfade this replaced held the level constant by construction —
/// `cos² + sin² = 1` — and adding instead of crossfading threw that away
/// along with the fault it was hiding. Measured on a mono programme at
/// −6 dBFS, an unpaid ensemble at full travel arrived +3.0 dB louder in RMS
/// and +10.3 dB in peak: a knob nobody could A/B, on a unit whose entire job
/// is to be A/B'd. Decorrelated energy added to a signal ADDS, in power, and
/// the arithmetic says by exactly how much.
#[inline]
pub fn ensemble_law(e: f32) -> f32 {
    (e / (2.0 - e)).sqrt()
}

/// The output trim that pays for the manufactured side, given
/// [`ensemble_law`]'s gain.
///
/// `M` and the velvet's copy of `M` are decorrelated, so their powers add and
/// nothing else does: a channel that held `M²` now holds `M²·(1 + k²)`, and
/// `1/√(1 + k²)` is the whole of the correction — exact for RMS, not a
/// calibration, not a measurement, and not a taper anybody had to choose.
///
/// **Why it is applied to the reconstruction and not to the side alone.** It
/// cannot be applied to the side alone: the level arrived with the side and
/// the mid TOGETHER, and no gain on the side by itself can take it out
/// without also taking out the width the dial was turned for. So the mid pays
/// its share, which costs the module's guarantee nothing — see the header:
/// the fold still returns the mid and nothing but the mid, at the same trim
/// the stereo pair got. It cannot change the mono penalty either, which is
/// `1/√(1 + k²)` of the stereo level whether this function exists or not; all
/// it moves is which of the two sits still while the knob turns, and for an
/// insert that has to be the stereo pair.
///
/// [`ensemble_law`] does not change: a common scalar on both channels
/// cancels out of the correlation reading top and bottom, so the dial is
/// still linear in the meter the panel prints.
#[inline]
pub fn ensemble_trim(k: f32) -> f32 {
    1.0 / (1.0 + k * k).sqrt()
}

/// One sparse-FIR tap: where to read, and the signed, envelope-weighted gain.
///
/// The sign rides inside the gain, so the per-sample work is exactly one
/// multiply-add per tap — 37 of them — and never a pass over the kernel's
/// 1100 mostly-empty samples.
#[derive(Debug, Clone, Copy, PartialEq)]
struct VelvetTap {
    offset: usize,
    gain: f32,
}

/// The xorshift step, as everywhere else in this codebase: audio-adjacent
/// code must not allocate, lock or reach for a global.
#[inline]
fn xorshift(s: &mut u32) -> u32 {
    *s ^= *s << 13;
    *s ^= *s >> 17;
    *s ^= *s << 5;
    *s
}

// ── the kernel ──────────────────────────────────────────────────────────────

/// Below this a knob is at rest — the `effects.rs` threshold.
const OFF: f32 = 0.0005;

/// Below this, a sample is on its way to a denormal and is worth nothing;
/// 1e-20 is 400 dB down. Same guard, same reasoning as `effects::flush`.
const TINY: f32 = 1.0e-20;

#[inline]
fn flush(x: f32) -> f32 {
    if x.abs() < TINY {
        0.0
    } else {
        x
    }
}

/// The stereo imager: width, mono floor and ensemble, on the side only.
///
/// # The crossover
///
/// The mono floor is a complementary first-order split on S: the low band is
/// one lowpass, the high band is `S` minus it, so the two bands sum back to
/// `S` exactly, by construction — and the collapse is simply not adding the
/// low band back. The lowpass is the BILINEAR one-pole (the topology-preserving
/// form), not the `1 - exp(-w)` pole the filters in `effects.rs` use, and the
/// difference is the point: the exponential pole still passes `a/2` of the
/// low band at Nyquist, which puts a `w/2` dip — a third of a dB at a 500 Hz
/// corner — across everything ABOVE the crossover in the complement. The
/// bilinear pole is zero at Nyquist, so the high band it leaves behind is
/// unity there, and "sum-flat" survives the subtraction.
///
/// # The ensemble
///
/// The MID is convolved with the velvet kernel and the result is ADDED to
/// whatever side the record already had, at [`ensemble_law`]'s gain. Two
/// decisions there, and the module header argues the first at length: the
/// velvet is fed the mid because a mono source has no side to feed it, and
/// the manufactured side is added rather than crossfaded against the real one
/// because a mastering widener may not throw away the image it was given.
///
/// The kernel's unit energy is what makes the top of the dial mean something:
/// a decorrelated copy of the mid, at the mid's own power, is a correlation of
/// exactly zero. Nothing about the mono fold changes on the way — the output
/// is still `M ± S'`, times the one trim both channels get — so the widest
/// setting is still free to fold.
///
/// What the manufactured side is NOT free of is level: decorrelated energy
/// added to a channel adds, in power, and [`ensemble_trim`] takes it back
/// out. The dial is a character control on a meter that does not move, which
/// is what it always claimed to be and for one commit was not.
///
/// # Order of operations
///
/// Ensemble, then floor, then width. Everything in the side path is linear
/// and time-invariant, so the maths does not care — but the floor running
/// LAST of the filters means "mono below the corner" is true of what leaves
/// the unit, velvet included, not just of what entered it.
pub struct Imager {
    sample_rate: f32,
    /// The velvet's view of the recent MID. Written every sample, read only
    /// while the ensemble is in circuit.
    ring: Vec<f32>,
    at: usize,
    taps: [VelvetTap; VELVET_TAPS],
    /// The crossover's one state, TPT form.
    lp: f32,
    /// One-time tail clear on the way to rest, as in `Effects::process`:
    /// whatever the ring held must not be waiting to be heard when the
    /// ensemble comes back half an hour later.
    idle: bool,
}

impl Imager {
    pub fn new(sample_rate: f32) -> Self {
        // The engine never runs below 8 kHz; neither does this.
        let sr = f64::from(sample_rate.max(8_000.0));

        // **The kernel is designed in seconds, not in samples.** Tap times
        // come off the grid in f64 time and only meet the sample rate at the
        // final rounding, so 44.1 k, 48 k and 96 k build the same instrument
        // — same times, same signs, same gains — at different addresses.
        let mut seed = VELVET_SEED;

        // Signs in opposed pairs, one coin per pair: cell 2m and cell 2m+1
        // always disagree. That pins the sequence's sum near zero — within
        // ±1 with an odd tap — which empties the kernel at DC and starves it
        // of the low-frequency energy where interchannel phase is the most
        // audible as coloration. This is the "optimized" in optimized velvet
        // noise, done by construction instead of by search.
        let mut signs = [1.0_f64; VELVET_TAPS];
        for pair in 0..VELVET_TAPS / 2 {
            let first = if xorshift(&mut seed) & 1 == 1 { 1.0 } else { -1.0 };
            signs[2 * pair] = first;
            signs[2 * pair + 1] = -first;
        }
        if VELVET_TAPS % 2 == 1 {
            signs[VELVET_TAPS - 1] = if xorshift(&mut seed) & 1 == 1 { 1.0 } else { -1.0 };
        }

        let cell = 1.0 / VELVET_DENSITY_HZ;
        let span = VELVET_LEN_MS / 1_000.0;
        let mut times = [0.0_f64; VELVET_TAPS];
        let mut gains = [0.0_f64; VELVET_TAPS];
        let mut energy = 0.0_f64;
        for m in 0..VELVET_TAPS {
            let jitter = f64::from(xorshift(&mut seed)) / f64::from(u32::MAX);
            let t = (m as f64 + jitter * VELVET_JITTER) * cell;
            let env = 10.0_f64.powf(-VELVET_TAIL_DB / 20.0 * t / span);
            times[m] = t;
            gains[m] = signs[m] * env;
            energy += env * env;
        }
        // Unit energy, so a broadband mid leaves the convolution as loud as
        // it entered. That is what makes the top of the dial mean "a
        // decorrelated copy of the mid at the mid's own power" — it is NOT
        // what keeps the level constant, which is [`ensemble_trim`]'s job and
        // was for a while nobody's.
        let norm = 1.0 / energy.sqrt();

        let mut taps = [VelvetTap {
            offset: 0,
            gain: 0.0,
        }; VELVET_TAPS];
        for m in 0..VELVET_TAPS {
            taps[m] = VelvetTap {
                offset: (times[m] * sr).round() as usize,
                gain: (gains[m] * norm) as f32,
            };
        }

        let cap = (sr * span).ceil() as usize + 2;
        Self {
            sample_rate: sr as f32,
            ring: vec![0.0; cap],
            at: 0,
            taps,
            lp: 0.0,
            idle: true,
        }
    }

    /// In place, interleaved, stride = `channels`; the first two channels are
    /// the pair and anything past them is left alone — the `Effects::process`
    /// contract. Fewer than two channels and there is nothing to image.
    pub fn process(
        &mut self,
        buf: &mut [f32],
        frames: usize,
        channels: usize,
        params: &ImagerParams,
    ) {
        if channels < 2 {
            return;
        }
        let p = params.sane();

        // Every knob at rest: a wire, bit for bit. Width's rest is the
        // MIDDLE of its travel — unity — unlike any dial in `effects.rs`.
        let at_rest = (p.width - 0.5).abs() < OFF && p.ensemble < OFF && p.mono_below < OFF;
        if at_rest {
            if !self.idle {
                self.clear();
                self.idle = true;
            }
            return;
        }
        self.idle = false;

        let s_gain = WIDTH_GAIN.0 + (WIDTH_GAIN.1 - WIDTH_GAIN.0) * p.width;

        let wet_on = p.ensemble >= OFF;
        let wet_g = if wet_on { ensemble_law(p.ensemble) } else { 0.0 };
        // What the manufactured side costs, taken back out. Exactly one when
        // the ensemble is shut, so the shipped null is still bit for bit.
        let out_g = ensemble_trim(wet_g);

        // The corner steps from "off" straight to 60 Hz, and that is fine: a
        // 60 Hz first-order fold is the smallest move this dial can make.
        let fold_on = p.mono_below >= OFF;
        let xover_g = if fold_on {
            let hz = MONO_BELOW_HZ.0 * (MONO_BELOW_HZ.1 / MONO_BELOW_HZ.0).powf(p.mono_below);
            let g = (PI * hz / self.sample_rate).tan();
            g / (1.0 + g)
        } else {
            0.0
        };

        let cap = self.ring.len();
        for f in buf.chunks_exact_mut(channels).take(frames) {
            let (l, r) = (f[0], f[1]);
            let m = 0.5 * (l + r);
            let s = 0.5 * (l - r);

            // **The MID goes on the ring, not the side.** See the module
            // header: the side is what the ensemble is for MAKING, and on the
            // mono program this unit mostly meets there is none to convolve.
            // Fed even while the ensemble is out of circuit, so the dial
            // arrives on a warm kernel instead of a fade-in.
            self.ring[self.at] = flush(m);

            let mut side = s;
            if wet_on {
                let mut velvet = 0.0_f32;
                for t in &self.taps {
                    let i = if self.at >= t.offset {
                        self.at - t.offset
                    } else {
                        self.at + cap - t.offset
                    };
                    velvet += t.gain * self.ring[i];
                }
                // Added, not crossfaded: whatever image the record arrived
                // with is the one the mix already approved, and a widener
                // that swaps it for a synthetic one is a different unit.
                // Adding is also what costs the level the crossfade used to
                // hold for free; [`ensemble_trim`] pays for it below.
                side = s + wet_g * velvet;
            }
            if fold_on {
                // TPT one-pole lowpass; the high band is the complement.
                let v = xover_g * (side - self.lp);
                let low = v + self.lp;
                self.lp = flush(low + v);
                side -= low;
            }
            side *= s_gain;

            // The reconstruction that IS the guarantee: M appears in both
            // channels, alone, so the fold hands back the mid and nothing
            // else. Both channels take the same trim, which is why the fold
            // still cannot find anything in there that is not the mid.
            f[0] = (m + side) * out_g;
            f[1] = (m - side) * out_g;

            self.at += 1;
            if self.at == cap {
                self.at = 0;
            }
        }
    }

    pub fn clear(&mut self) {
        self.ring.fill(0.0);
        self.at = 0;
        self.lp = 0.0;
    }
}

// ── the meters ──────────────────────────────────────────────────────────────

/// How much recent signal the correlation meter reads, in milliseconds.
///
/// 400 ms is hardware correlation-meter ballistics: slow enough to sit still
/// and be read as a number, fast enough to catch one anti-phase phrase before
/// it is over.
const CORR_WINDOW_MS: f32 = 400.0;

/// Windowed normalised correlation between the two channels: +1 is mono, −1
/// is anti-phase, 0 is unrelated.
///
/// A true sliding window at O(1) a sample: the three running sums (Σlr, Σl²,
/// Σr²) take each arriving product in and, once the window is full, give the
/// leaving one back. The products are f32×f32 evaluated in f64, which is
/// EXACT — 48 bits into 53 — so the sample that leaves subtracts precisely
/// what it added and the sums cannot drift, no matter how long the session.
pub struct CorrelationMeter {
    ring: Vec<[f32; 2]>,
    at: usize,
    filled: usize,
    lr: f64,
    ll: f64,
    rr: f64,
}

impl CorrelationMeter {
    pub fn new(sample_rate: f32) -> Self {
        let window = ((sample_rate.max(8_000.0) * CORR_WINDOW_MS / 1_000.0) as usize).max(1);
        Self {
            ring: vec![[0.0; 2]; window],
            at: 0,
            filled: 0,
            lr: 0.0,
            ll: 0.0,
            rr: 0.0,
        }
    }

    #[inline]
    pub fn push(&mut self, l: f32, r: f32) {
        if self.filled == self.ring.len() {
            let [ol, or] = self.ring[self.at];
            let (ol, or) = (f64::from(ol), f64::from(or));
            self.lr -= ol * or;
            self.ll -= ol * ol;
            self.rr -= or * or;
        } else {
            self.filled += 1;
        }
        self.ring[self.at] = [l, r];
        let (l, r) = (f64::from(l), f64::from(r));
        self.lr += l * r;
        self.ll += l * l;
        self.rr += r * r;
        self.at += 1;
        if self.at == self.ring.len() {
            self.at = 0;
        }
    }

    /// The whole drained block at once, interleaved, stride = `channels` —
    /// the shape `Engine::drain_scope` hands the UI.
    pub fn feed(&mut self, buf: &[f32], frames: usize, channels: usize) {
        if channels < 2 {
            return;
        }
        for f in buf.chunks_exact(channels).take(frames) {
            self.push(f[0], f[1]);
        }
    }

    /// Silence reads 0, not 1: an empty meter has no opinion about phase.
    pub fn read(&self) -> f32 {
        const FLOOR: f64 = 1.0e-12;
        if self.ll < FLOOR || self.rr < FLOOR {
            return 0.0;
        }
        (self.lr / (self.ll * self.rr).sqrt()).clamp(-1.0, 1.0) as f32
    }

    pub fn clear(&mut self) {
        self.ring.fill([0.0; 2]);
        self.at = 0;
        self.filled = 0;
        self.lr = 0.0;
        self.ll = 0.0;
        self.rr = 0.0;
    }
}

/// One goniometer point: the L/R plane rotated 45°, so mono stands upright
/// and pure side lies flat — the orientation every hardware scope has used
/// since the Lissajous days. Energy-preserving: `x² + y² = l² + r²`.
#[inline]
pub fn scope_xy(l: f32, r: f32) -> (f32, f32) {
    ((l - r) * FRAC_1_SQRT_2, (l + r) * FRAC_1_SQRT_2)
}

// ── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::f64::consts::TAU;

    const SR: f32 = 48_000.0;

    fn rand(s: &mut u32) -> f32 {
        xorshift(s) as f32 / u32::MAX as f32 - 0.5
    }

    /// Interleaved stereo noise, both channels from one stream, ±0.8.
    fn noise(seed: u32, frames: usize) -> Vec<f32> {
        let mut s = seed;
        (0..frames * 2).map(|_| 1.6 * rand(&mut s)).collect()
    }

    /// Interleaved stereo from a mid sine and a side sine, at amplitudes
    /// chosen by the test.
    fn mid_side_tones(frames: usize, sr: f32, mid: (f64, f64), side: (f64, f64)) -> Vec<f32> {
        let mut buf = vec![0.0_f32; frames * 2];
        for (i, f) in buf.chunks_exact_mut(2).enumerate() {
            let t = i as f64 / f64::from(sr);
            let m = mid.1 * (TAU * mid.0 * t).sin();
            let s = side.1 * (TAU * side.0 * t).sin();
            f[0] = (m + s) as f32;
            f[1] = (m - s) as f32;
        }
        buf
    }

    fn mids(buf: &[f32]) -> Vec<f32> {
        buf.chunks_exact(2).map(|f| 0.5 * (f[0] + f[1])).collect()
    }

    fn sides(buf: &[f32]) -> Vec<f32> {
        buf.chunks_exact(2).map(|f| 0.5 * (f[0] - f[1])).collect()
    }

    fn rms(v: &[f32]) -> f64 {
        (v.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>() / v.len() as f64).sqrt()
    }

    fn peak(buf: &[f32]) -> f32 {
        buf.iter().fold(0.0_f32, |m, v| m.max(v.abs()))
    }

    /// The shipped defaults are a wire: not "quiet", not "close" — the same
    /// bits out that went in, because the default preset of an imager is the
    /// mix somebody already approved.
    #[test]
    fn the_shipped_defaults_are_a_bit_exact_null() {
        let mut im = Imager::new(SR);
        let mut buf = noise(0x0BAD_CAFE, 4_096);
        let before = buf.clone();
        im.process(&mut buf, 4_096, 2, &ImagerParams::default());
        assert_eq!(buf, before, "the default setting altered the signal");
    }

    /// **The flagship claim.** Whatever the three knobs do, the mono fold of
    /// the output is the mid that went in — times the one output trim both
    /// channels got, and times nothing else. At every corner of the parameter
    /// cube, on random program, sample by sample.
    ///
    /// The claim is sharper than "the fold does not move", which is what this
    /// asked before [`ensemble_trim`] existed and which an ensemble that
    /// changes the level cannot honour anyway. A fold that came back at a
    /// different LEVEL would be a knob nobody could A/B; a fold that came back
    /// as a different SIGNAL would be a comb, a cancellation, a hole in
    /// somebody's mix on a phone speaker — and it is that second thing this
    /// unit exists to make impossible. So the trim is predicted from the
    /// parameters, divided back out, and what is left has to match the input
    /// mid to the bit. Only a scalar can survive that.
    #[test]
    fn the_mono_fold_is_the_mid_it_was_given_and_a_trim_this_test_predicts() {
        const FRAMES: usize = 4_096;
        let corners = [0.0_f32, 0.5, 1.0];
        for width in corners {
            for ensemble in corners {
                for mono_below in corners {
                    let p = ImagerParams {
                        width,
                        ensemble,
                        mono_below,
                    };
                    // The trim the kernel will apply, from the published
                    // laws rather than from the kernel's own arithmetic —
                    // a test that asked the code what it did would agree
                    // with any answer it gave.
                    let expect = if ensemble >= OFF {
                        ensemble_trim(ensemble_law(ensemble))
                    } else {
                        1.0
                    };
                    let mut im = Imager::new(SR);
                    // Two blocks, so the second runs on a warm velvet ring.
                    for block in 0..2_u32 {
                        let mut buf = noise(0x5747_EE00 ^ block, FRAMES);
                        let mid_in = mids(&buf);
                        im.process(&mut buf, FRAMES, 2, &p);
                        let mid_out = mids(&buf);
                        let worst = mid_in
                            .iter()
                            .zip(&mid_out)
                            .map(|(a, b)| (a * expect - b).abs())
                            .fold(0.0_f32, f32::max);
                        assert!(
                            worst < 1.0e-4,
                            "width {width} ensemble {ensemble} mono_below {mono_below} \
                             left {worst} in the fold that is not the mid times \
                             {expect}"
                        );
                    }
                }
            }
        }
    }

    /// Identical channels read +1, inverted channels read −1, and two
    /// unrelated noise streams read near zero — the three faces a hardware
    /// correlation meter is checked against.
    #[test]
    fn the_correlation_meter_reads_the_three_textbook_cases() {
        let feed = |meter: &mut CorrelationMeter, invert: f32| {
            let mut s = 0x1234_5678_u32;
            for _ in 0..48_000 {
                let v = rand(&mut s);
                meter.push(v, v * invert);
            }
        };
        let mut mono = CorrelationMeter::new(SR);
        feed(&mut mono, 1.0);
        assert!((mono.read() - 1.0).abs() < 1.0e-3, "mono read {}", mono.read());

        let mut anti = CorrelationMeter::new(SR);
        feed(&mut anti, -1.0);
        assert!((anti.read() + 1.0).abs() < 1.0e-3, "anti-phase read {}", anti.read());

        let mut free = CorrelationMeter::new(SR);
        let (mut a, mut b) = (0x1234_5678_u32, 0x8765_4321_u32);
        for _ in 0..48_000 {
            free.push(rand(&mut a), rand(&mut b));
        }
        assert!(
            free.read().abs() < 0.2,
            "independent noise read {}",
            free.read()
        );
    }

    /// Full clockwise is 200 %: the side leaves at exactly twice the level
    /// and the mid does not notice. Full anticlockwise is mono.
    #[test]
    fn full_width_doubles_the_side_and_spares_the_mid() {
        const FRAMES: usize = 48_000;
        let input = mid_side_tones(FRAMES, SR, (240.0, 0.3), (1_200.0, 0.2));

        let mut wide = input.clone();
        Imager::new(SR).process(
            &mut wide,
            FRAMES,
            2,
            &ImagerParams {
                width: 1.0,
                ..ImagerParams::default()
            },
        );
        let ratio = rms(&sides(&wide)) / rms(&sides(&input));
        assert!((ratio - 2.0).abs() < 1.0e-3, "full width scaled S by {ratio}");
        let worst = mids(&input)
            .iter()
            .zip(mids(&wide).iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        assert!(worst < 1.0e-4, "full width moved the mid by {worst}");

        let mut narrow = input.clone();
        Imager::new(SR).process(
            &mut narrow,
            FRAMES,
            2,
            &ImagerParams {
                width: 0.0,
                ..ImagerParams::default()
            },
        );
        assert!(
            rms(&sides(&narrow)) < 1.0e-7,
            "zero width left side energy behind"
        );
    }

    /// With the floor anywhere on its dial and everything else at rest, a
    /// program whose side lives above the corner comes through level — the
    /// crossover splits and re-sums flat, and only what it was asked to
    /// collapse is missing. This is the test that made the crossover
    /// bilinear: the exponential pole's complement dips a third of a dB
    /// across the whole top of the band at a 500 Hz corner.
    #[test]
    fn the_crossover_passes_everything_above_the_corner_sum_flat() {
        const FRAMES: usize = 48_000;
        const SKIP: usize = 4_800;
        let input = mid_side_tones(FRAMES, SR, (200.0, 0.4), (8_000.0, 0.3));
        for step in 1..=8 {
            let p = ImagerParams {
                mono_below: step as f32 / 8.0,
                ..ImagerParams::default()
            };
            let mut out = input.clone();
            Imager::new(SR).process(&mut out, FRAMES, 2, &p);
            for ch in 0..2 {
                let pick = |buf: &[f32]| -> Vec<f32> {
                    buf.chunks_exact(2).skip(SKIP).map(|f| f[ch]).collect()
                };
                let db = 20.0 * (rms(&pick(&out)) / rms(&pick(&input))).log10();
                assert!(
                    db.abs() < 0.05,
                    "corner at {} of the dial moved channel {ch} by {db:.3} dB",
                    p.mono_below
                );
            }
        }
    }

    /// And below the corner it genuinely folds: side-only rumble at 30 Hz
    /// loses over 18 dB with the floor at 500 Hz, while the (empty) mid
    /// stays empty.
    #[test]
    fn side_content_below_the_corner_is_folded_to_mono() {
        const FRAMES: usize = 48_000;
        const SKIP: usize = 9_600;
        let input = mid_side_tones(FRAMES, SR, (0.0, 0.0), (30.0, 0.5));
        let mut out = input.clone();
        Imager::new(SR).process(
            &mut out,
            FRAMES,
            2,
            &ImagerParams {
                mono_below: 1.0,
                ..ImagerParams::default()
            },
        );
        let pick = |buf: &[f32]| -> Vec<f32> { sides(buf).split_off(SKIP) };
        let db = 20.0 * (rms(&pick(&out)) / rms(&pick(&input))).log10();
        assert!(db < -18.0, "30 Hz side only dropped {db:.1} dB");
        assert!(
            peak(&mids(&out)) < 1.0e-6,
            "a pure-side input grew a mid channel"
        );
    }

    /// The kernel is a constant of the product: the advertised tap count,
    /// signs balanced to within one, strictly ordered, inside the stated
    /// length — and two instances agree to the bit.
    #[test]
    fn the_velvet_kernel_is_the_same_kernel_every_time() {
        let a = Imager::new(SR);
        let b = Imager::new(SR);
        assert_eq!(a.taps, b.taps, "two instances built different kernels");
        assert_eq!(
            a.taps.len(),
            (VELVET_DENSITY_HZ * VELVET_LEN_MS / 1_000.0) as usize,
            "the literal tap count drifted from density × length"
        );
        let balance: i32 = a.taps.iter().map(|t| t.gain.signum() as i32).sum();
        assert!(balance.abs() <= 1, "signs sum to {balance}");
        assert!(
            a.taps.windows(2).all(|w| w[0].offset < w[1].offset),
            "taps out of order or merged"
        );
        assert!(
            a.taps.last().unwrap().offset < a.ring.len(),
            "a tap reaches past the ring"
        );
    }

    /// Every corner of the cube, against a full-scale anti-phase signal —
    /// the worst customer an imager has — for several blocks of fresh input:
    /// finite always, and bounded by a runaway cap, not a taste cap.
    #[test]
    fn no_setting_a_hand_can_reach_makes_it_explode() {
        const FRAMES: usize = 24_000;
        let extremes = [0.0_f32, 1.0];
        for width in extremes {
            for ensemble in extremes {
                for mono_below in extremes {
                    let p = ImagerParams {
                        width,
                        ensemble,
                        mono_below,
                    };
                    let mut im = Imager::new(SR);
                    let mut t = 0_usize;
                    for _ in 0..4 {
                        let mut buf = vec![0.0_f32; FRAMES * 2];
                        for f in buf.chunks_exact_mut(2) {
                            let ph = TAU * 110.0 * t as f64 / f64::from(SR);
                            let m = 0.5 * (TAU * 220.0 * t as f64 / f64::from(SR)).sin();
                            let s = ph.sin();
                            f[0] = (m + s) as f32;
                            f[1] = (m - s) as f32;
                            t += 1;
                        }
                        im.process(&mut buf, FRAMES, 2, &p);
                        assert!(
                            buf.iter().all(|v| v.is_finite()),
                            "width {width} ensemble {ensemble} mono_below {mono_below} \
                             produced a non-finite sample"
                        );
                        assert!(
                            peak(&buf) < 12.0,
                            "width {width} ensemble {ensemble} mono_below {mono_below} \
                             reached {}",
                            peak(&buf)
                        );
                    }
                }
            }
        }
    }

    /// The same instrument at every sample rate: tap TIMES agree to within
    /// rounding, tap gains agree to the bit, and the mono floor's corner
    /// measures the same in hertz.
    #[test]
    fn the_imager_is_the_same_instrument_at_every_sample_rate() {
        let rates = [44_100.0_f32, 48_000.0, 96_000.0];
        let built: Vec<Imager> = rates.iter().map(|sr| Imager::new(*sr)).collect();
        for (im, sr) in built.iter().zip(rates).skip(1) {
            assert_eq!(im.taps.len(), built[0].taps.len());
            for (a, b) in built[0].taps.iter().zip(im.taps.iter()) {
                let ms_a = f64::from(a.offset as u32) / f64::from(rates[0]) * 1_000.0;
                let ms_b = f64::from(b.offset as u32) / f64::from(sr) * 1_000.0;
                assert!(
                    (ms_a - ms_b).abs() < 0.02,
                    "a tap sits at {ms_a:.4} ms at 44.1k and {ms_b:.4} ms at {sr}"
                );
                assert_eq!(a.gain, b.gain, "tap gains differ across rates");
            }
        }

        // The corner in hertz: a 250 Hz side tone under a 500 Hz floor loses
        // the same amount whatever the rate is.
        let atten_db = |sr: f32| {
            let frames = sr as usize;
            let skip = frames / 10;
            let input = mid_side_tones(frames, sr, (0.0, 0.0), (250.0, 0.5));
            let mut out = input.clone();
            Imager::new(sr).process(
                &mut out,
                frames,
                2,
                &ImagerParams {
                    mono_below: 1.0,
                    ..ImagerParams::default()
                },
            );
            let pick = |buf: &[f32]| -> Vec<f32> { sides(buf).split_off(skip) };
            20.0 * (rms(&pick(&out)) / rms(&pick(&input))).log10()
        };
        let measured: Vec<f64> = rates.iter().map(|sr| atten_db(*sr)).collect();
        for pair in measured.windows(2) {
            assert!(
                (pair[0] - pair[1]).abs() < 0.5,
                "the corner moved between rates: {measured:?}"
            );
        }
    }

    /// The rotation the panel will draw: mono stands upright, side lies
    /// flat, and no energy is invented or lost on the way.
    #[test]
    fn the_goniometer_stands_mono_upright_and_side_sideways() {
        let (x, y) = scope_xy(0.5, 0.5);
        assert_eq!(x, 0.0, "a mono pair leaned off the vertical");
        assert!((y - 2.0 * 0.5 * FRAC_1_SQRT_2).abs() < 1.0e-6);

        let (x, y) = scope_xy(0.5, -0.5);
        assert_eq!(y, 0.0, "an anti-phase pair left the horizontal");
        assert!((x - 2.0 * 0.5 * FRAC_1_SQRT_2).abs() < 1.0e-6);

        let mut s = 0xD1CE_D00D_u32;
        for _ in 0..64 {
            let (l, r) = (rand(&mut s), rand(&mut s));
            let (x, y) = scope_xy(l, r);
            assert!((x * x + y * y - (l * l + r * r)).abs() < 1.0e-6);
        }
    }

    /// A clear leaves nothing behind: after loud program through the full
    /// ensemble, a cleared imager processing silence returns silence.
    #[test]
    fn clear_leaves_nothing_to_come_back() {
        const FRAMES: usize = 4_096;
        let p = ImagerParams {
            ensemble: 1.0,
            mono_below: 1.0,
            ..ImagerParams::default()
        };
        let mut im = Imager::new(SR);
        let mut buf = noise(0xFEED_F00D, FRAMES);
        im.process(&mut buf, FRAMES, 2, &p);
        im.clear();
        let mut quiet = vec![0.0_f32; FRAMES * 2];
        im.process(&mut quiet, FRAMES, 2, &p);
        assert!(
            peak(&quiet) < 1.0e-9,
            "an old side tail came back after clear: {}",
            peak(&quiet)
        );
    }

    /// One microphone, arriving on both sides exactly as `mix_channels`
    /// sends it — "a mono input goes to both sides". `L == R`, so `S` is
    /// identically zero, which is the material this unit is most often
    /// pointed at and the material it was silently a wire on.
    fn one_microphone(seed: u32, frames: usize) -> Vec<f32> {
        let mut s = seed;
        (0..frames)
            .flat_map(|_| {
                let v = 1.6 * rand(&mut s);
                [v, v]
            })
            .collect()
    }

    /// What the panel's meter would read of a finished buffer, through the
    /// module's own meter rather than a second implementation.
    fn correlation(buf: &[f32]) -> f32 {
        let mut meter = CorrelationMeter::new(SR);
        meter.feed(buf, buf.len() / 2, 2);
        meter.read()
    }

    /// The side-to-mid amplitude ratio the goniometer draws as width.
    fn side_over_mid(buf: &[f32]) -> f64 {
        rms(&sides(buf)) / rms(&mids(buf)).max(1.0e-12)
    }

    /// **The fault a demo session found: on a single microphone every
    /// control on this unit was a wire.**
    ///
    /// A mono source reaches the desk as `L == R`, so the side it carries is
    /// exactly zero. Width scaled that zero, the floor folded it, and the
    /// ensemble — the one control whose whole charter is "width for material
    /// that has no side to widen" — was convolving the very signal it exists
    /// to manufacture, so it too returned zero. Three dead knobs and a
    /// vectorscope drawing a vertical line, from one arithmetic mistake.
    ///
    /// The claim: with the ensemble open the image widens, monotonely, and
    /// the width knob then has something to scale.
    #[test]
    fn one_microphone_can_still_be_widened_and_the_meter_says_so() {
        const FRAMES: usize = 48_000;
        let input = one_microphone(0xB0A7_1DEA, FRAMES);
        assert!(
            rms(&sides(&input)) < 1.0e-12,
            "the test's own source is not mono"
        );

        // The ensemble, quarter by quarter: side energy appears out of the
        // mid and the correlation walks down from +1.
        let mut last = (-1.0_f64, 2.0_f32);
        for step in 0..=4 {
            let ensemble = step as f32 / 4.0;
            let mut out = input.clone();
            Imager::new(SR).process(
                &mut out,
                FRAMES,
                2,
                &ImagerParams {
                    ensemble,
                    ..ImagerParams::default()
                },
            );
            let (ratio, corr) = (side_over_mid(&out), correlation(&out));
            assert!(
                ratio > last.0 + 0.05 || step == 0,
                "ensemble {ensemble} widened no further than the step below \
                 it: {ratio:.3} after {:.3}",
                last.0
            );
            assert!(
                corr < last.1 - 0.05 || step == 0,
                "ensemble {ensemble} left the correlation at {corr:.3}, \
                 after {:.3}",
                last.1
            );
            // The dial is linear in CORRELATION, which is the reading the
            // panel prints: half the dial is half the way to zero.
            assert!(
                (corr - (1.0 - ensemble)).abs() < 0.06,
                "ensemble {ensemble} read {corr:.3}, not {:.3}",
                1.0 - ensemble
            );
            last = (ratio, corr);
        }

        // And now the width knob has something to scale. Full ensemble puts
        // the manufactured side at the mid's own level, so the ratio walks
        // 0, 1, 2 across the dial and the correlation follows it through
        // zero and out the anti-phase side.
        let mut last = (-1.0_f64, 2.0_f32);
        for width in [0.0_f32, 0.5, 1.0] {
            let mut out = input.clone();
            Imager::new(SR).process(
                &mut out,
                FRAMES,
                2,
                &ImagerParams {
                    width,
                    ensemble: 1.0,
                    ..ImagerParams::default()
                },
            );
            let (ratio, corr) = (side_over_mid(&out), correlation(&out));
            let want = 2.0 * f64::from(width);
            assert!(
                (ratio - want).abs() < 0.1,
                "width {width} on a widened microphone read {ratio:.3}, \
                 not {want:.3}"
            );
            assert!(
                ratio > last.0 + 0.5 && corr < last.1 - 0.2,
                "width {width} did not widen past the step below it: \
                 ratio {ratio:.3} after {:.3}, correlation {corr:.3} after {:.3}",
                last.0,
                last.1
            );
            last = (ratio, corr);
        }
    }

    /// The other end of the same sweep: an ANTI-correlated pair, which is
    /// all side and no mid. Width is the only thing that can act on it, the
    /// ratio is unbounded so the side's own level is what is measured, and
    /// the mid must stay empty — a widener may not invent a centre.
    #[test]
    fn an_anti_correlated_pair_answers_the_width_knob_alone() {
        const FRAMES: usize = 48_000;
        let mut s = 0x5EED_1E55_u32;
        let input: Vec<f32> = (0..FRAMES)
            .flat_map(|_| {
                let v = 1.6 * rand(&mut s);
                [v, -v]
            })
            .collect();
        let reference = rms(&sides(&input));

        let mut last = -1.0_f64;
        for width in [0.0_f32, 0.5, 1.0] {
            let mut out = input.clone();
            Imager::new(SR).process(
                &mut out,
                FRAMES,
                2,
                &ImagerParams {
                    width,
                    ..ImagerParams::default()
                },
            );
            let level = rms(&sides(&out)) / reference;
            let want = 2.0 * f64::from(width);
            assert!(
                (level - want).abs() < 1.0e-3,
                "width {width} scaled a pure-side pair by {level:.4}, not {want}"
            );
            assert!(level >= last, "width {width} narrowed the image");
            last = level;

            assert!(
                peak(&mids(&out)) < 1.0e-6,
                "width {width} invented a centre on a pure-side pair"
            );
            // The meter agrees with the arithmetic at both ends: pure side
            // is −1, and the collapsed end has no opinion because there is
            // no signal left to have one about.
            let corr = correlation(&out);
            let want_corr = if width < OFF { 0.0 } else { -1.0 };
            assert!(
                (corr - want_corr).abs() < 1.0e-3,
                "width {width} read {corr:.4}, not {want_corr}"
            );
        }
    }

    /// **The ensemble is a character control, and here is the meter that has
    /// to sit still while it turns.**
    ///
    /// It did not. Replacing the equal-power crossfade with a plain addition
    /// fixed a real fault — the crossfade threw away the image the mix
    /// arrived with — and threw away the one thing the crossfade was FOR:
    /// `cos² + sin² = 1` had been holding the level for free, and the comment
    /// promising that it still did outlived the code that did it. Measured on
    /// a mono programme at −6 dBFS, an unpaid ensemble at full travel arrived
    /// +3.0 dB in RMS and +10.3 dB in peak.
    ///
    /// [`ensemble_trim`] pays the bill, and what it can and cannot buy back
    /// is worth stating exactly, because the difference is arithmetic rather
    /// than taste:
    ///
    /// - **RMS, on the mono programme this control exists for: exact.** The
    ///   manufactured side is decorrelated from the mid, so their powers add
    ///   and nothing else does, and `1/√(1 + k²)` is the whole of it. The bar
    ///   below is a twentieth of a decibel and the measurement is three
    ///   thousandths.
    /// - **RMS on any other programme: never louder, and by a bounded
    ///   amount quieter.** A record that already had a side of its own pays
    ///   the mono trim on side energy it did not need to manufacture; at the
    ///   extreme — two independent noises, correlation zero before the unit
    ///   is even reached — that is 1.24 dB, on the programme an ensemble has
    ///   the least to offer. The law errs quiet on purpose: an insert that
    ///   can only ever hand back less is one a mastering chain can leave in.
    /// - **Peak: within about a decibel and a half, and not exactly.** No
    ///   widener can hold a peak the way it holds an RMS. Two independent
    ///   signals summed are peakier than either — a bounded programme meets a
    ///   velvet copy with Gaussian tails and the sum has the tails — so the
    ///   trim that is exact in power leaves crest behind. On programme with
    ///   the crest factor of music it is 1.2 dB; on synthetic uniform noise,
    ///   which is flatter-topped than anything a microphone ever made, it is
    ///   7.3. What matters is that it is now the CREST that moved and not the
    ///   level, and that neither is the ten decibels it was.
    #[test]
    fn the_ensemble_holds_its_level_across_the_whole_travel() {
        const FRAMES: usize = 96_000;
        const SKIP: usize = 4_800 * 2;
        let db = |r: f64| 20.0 * r.log10();

        // A mono programme with the crest factor of a real one: twelve
        // uniforms summed is close enough to Gaussian that its peak
        // statistics stop being an artefact of the generator.
        let mut seed = 0x51DE_0003_u32;
        let mono: Vec<f32> = (0..FRAMES)
            .map(|_| (0..12).map(|_| rand(&mut seed)).sum::<f32>() * (1.0 / 3.464))
            .collect();
        let mid_only: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();

        // And a record that arrived fully decorrelated, which is the worst
        // case for a trim solved on the mono one.
        let (mut a, mut b) = (0x0BAD_0001_u32, 0x0BAD_0002_u32);
        let already_wide: Vec<f32> = (0..FRAMES)
            .flat_map(|_| [0.8 * rand(&mut a), 0.8 * rand(&mut b)])
            .collect();

        for step in 0..=10 {
            let e = step as f32 / 10.0;
            let p = ImagerParams {
                ensemble: e,
                ..ImagerParams::default()
            };

            let mut out = mid_only.clone();
            Imager::new(SR).process(&mut out, FRAMES, 2, &p);
            let d_rms = db(rms(&out[SKIP..]) / rms(&mid_only[SKIP..]));
            let d_peak = db(f64::from(peak(&out[SKIP..])) / f64::from(peak(&mid_only[SKIP..])));
            assert!(
                d_rms.abs() < 0.05,
                "ensemble {e} moved a mono programme by {d_rms:.3} dB of RMS — \
                 the trim is not paying for what the addition costs"
            );
            assert!(
                d_peak.abs() < 1.5,
                "ensemble {e} moved a mono programme by {d_peak:.3} dB of PEAK"
            );

            let mut out = already_wide.clone();
            Imager::new(SR).process(&mut out, FRAMES, 2, &p);
            let d_rms = db(rms(&out[SKIP..]) / rms(&already_wide[SKIP..]));
            assert!(
                (-1.4..=0.001).contains(&d_rms),
                "ensemble {e} moved an already-wide programme by {d_rms:.3} dB \
                 — the trim may only ever hand back less, and never by more \
                 than the mono law's own worst case"
            );
        }
    }
}
