//! The bitcrusher kernel: a converter with too few bits, clocked too slowly,
//! and nothing in front of it to stop what that does.
//!
//! # The signal path
//!
//! ```text
//!  in ─┬─ DRIVE ─ [AA low-pass] ─ sample-and-hold (RATE) ─ quantizer (BITS, DITHER) ─┐
//!      │                                                             MIX ─ OUTPUT ─ out
//!      └─────────────────────────── dry ─────────────────────────────┘
//! ```
//!
//! 1. **DRIVE** is the level into the converter, ±24 dB. A quantizer's grid
//!    is fixed in absolute terms, so pushing the signal up spends more of its
//!    levels and pulling it down starves it: the knob decides how much of
//!    the grid the music gets. There is no clipper behind it. A float bus
//!    has no rails, and a crusher that also clipped would be two effects
//!    under one knob; OUTPUT is what brings a driven crush back down.
//! 2. **RATE** is a sample-and-hold, clocked by a phase accumulator from
//!    200 Hz to 48 kHz on a geometric sweep, with the top of the travel
//!    meaning the host's own rate. The clock is FRACTIONAL: at a rate that
//!    does not divide the host rate the hold alternates between the two
//!    neighbouring lengths, so the average rate is exactly the dialled one
//!    and a sweep glides instead of stepping through integer divisors.
//!    **There is no anti-alias filter, on purpose.** The folded images are
//!    the sound everybody means by "bitcrushed"; a filter in front of the
//!    hold would make this a polite resampler. Mono and stereo share one
//!    clock, as two channels of one converter do.
//! 3. **BITS** is a mid-tread quantizer, continuous from 1 to 24 bits: the
//!    step is `2^(1 − bits)` of full scale, so fractional bit depths are
//!    honest step sizes between the integer ones and the knob has no
//!    detents to zipper across. One bit is the coarsest grid, {−1, 0, +1}
//!    across full scale: a gate and a square in one. The quantizer runs once
//!    per HELD sample, at the moment the hold takes it, which is what a
//!    converter does: the stair keeps the value it was sampled at.
//! 4. **DITHER**, a switch, OFF by default. On, it adds triangular (TPDF)
//!    noise of ±1 step before the quantizer, which trades the crusher's
//!    gritty, signal-following distortion for a steady hiss: the textbook
//!    cure, offered because sometimes the hiss is the sound wanted. Off is
//!    the classic undithered crunch.
//! 5. **ANTI-ALIAS**, a switch, OFF by default, so the classic folding is
//!    what a fresh unit and every state saved before the switch existed
//!    play. On, an 8th-order Butterworth low-pass (four cascaded
//!    state-variable sections, the topology that stays stable while its
//!    cutoff moves every sample) sits between DRIVE and the hold, its cutoff
//!    at 0.45 of the crushed rate and gliding with RATE. It falls about 48 dB
//!    per octave above the cutoff, so a tone that would fold is gone before
//!    the hold can fold it: a tone 2.4 times the cutoff comes back more than
//!    60 dB down. It is a PRE-filter only. The staircase the hold draws (its
//!    images above the crushed rate, the buzz of a low sample rate) is left
//!    alone, because that edge is the other half of the sound; the switch
//!    takes away the folding and nothing else. IIR, so no latency. At RATE
//!    "full" there is nothing to fold and the filter is out of circuit:
//!    the switch fades it in and out over the same 20 ms glide as the
//!    knobs, so neither the switch nor RATE reaching the top clicks, and the
//!    wire stays bit-exact with the switch on.
//! 6. **MIX** blends the crushed path with the untouched dry, and **OUTPUT**
//!    (±24 dB) trims the sum.
//!
//! # The wire
//!
//! **BITS at the top of its travel is not a 24-bit quantizer, it is the
//! quantizer taken out.** A 24-bit grid is finer than any converter the
//! signal came through, and quantizing an `f32` to it would only scrape the
//! float's tail at −144 dBFS, an epsilon nobody can hear and every null test
//! can see. So 24 bits, RATE at "full" (a hold of exactly one sample), DRIVE
//! and OUTPUT at 0 dB and any MIX is a bit-exact wire for every finite
//! input, denormals included; `the_wire_settings_are_bit_exact` holds it.
//! MIX at 0 is a wire whatever the other knobs say.
//!
//! # Smoothing
//!
//! Every continuous knob glides through a one-pole of [`SMOOTH_MS`]: the two
//! gains and the mix in linear gain, the bit depth in bits (so the step moves
//! geometrically), the clock in samples-per-hold. Each smoother SNAPS onto
//! its target once it is within a hair of it, which is what lets the wire be
//! exact rather than nearly so, and keeps a smoother from crawling into
//! denormals. The dither switch is not smoothed: what it changes is noise at
//! the size of one step, which has no edge to click. A fresh or cleared unit
//! starts SETTLED at whatever it is first handed, because a restored preset
//! that slewed in from the knobs' rests would be audible as the preset
//! arriving late.
//!
//! # Safety
//!
//! Non-finite input reads as silence, and a sample the gains push past
//! `f32`'s range is caught again before it leaves: the unit never emits a
//! NaN or an infinity. Nothing here feeds back, so no state can decay into
//! a denormal; the held value is a copy of an input sample.
//!
//! Zero latency: every output sample is computed from the input sample at
//! the same index or one held from before it.

// ── what the dials map to ───────────────────────────────────────────────────

/// The bit depth the BITS dial sweeps, linear in bits. The top of the travel
/// takes the quantizer out of circuit; see the module header.
pub const BITS_RANGE: (f32, f32) = (1.0, 24.0);

/// The clock the RATE dial sweeps, in Hz, geometric. 200 Hz is where a hold
/// stops sounding like a sample rate at all and starts sounding like a
/// ring-modulated gate; 48 kHz is the ceiling the readings name. The very top
/// of the travel is "full": a hold of one sample at whatever rate the host
/// runs, so 96 kHz sessions reach the wire too.
pub const RATE_HZ: (f32, f32) = (200.0, 48_000.0);

/// DRIVE and OUTPUT both sweep this, in dB, linear; noon is unity exactly.
pub const DRIVE_DB: (f32, f32) = (-24.0, 24.0);
pub const OUTPUT_DB: (f32, f32) = (-24.0, 24.0);

/// How far short of a whole hold the clock may fall and still take the next
/// sample: far above the rounding a few hundred steps accumulate, far below
/// anything that could move a hold by a sample.
const CLOCK_SLACK: f64 = 1.0e-9;

/// The anti-zipper glide every continuous knob rides, in milliseconds.
pub const SMOOTH_MS: f32 = 20.0;

/// The bit depth a BITS dial position stands for.
pub fn bits_at(v: f32) -> f32 {
    BITS_RANGE.0 + (BITS_RANGE.1 - BITS_RANGE.0) * v.clamp(0.0, 1.0)
}

/// The dial position that stands for `bits`: the inverse of [`bits_at`].
pub fn bits_dial(bits: f32) -> f32 {
    ((bits - BITS_RANGE.0) / (BITS_RANGE.1 - BITS_RANGE.0)).clamp(0.0, 1.0)
}

/// The clock a RATE dial position stands for, in Hz, or `None` at the top of
/// the travel, which is the host's own rate.
pub fn rate_at(v: f32) -> Option<f32> {
    if v >= 1.0 {
        None
    } else {
        let v = v.max(0.0);
        Some(RATE_HZ.0 * (RATE_HZ.1 / RATE_HZ.0).powf(v))
    }
}

/// The dial position that stands for `hz`: the inverse of [`rate_at`].
pub fn rate_dial(hz: f32) -> f32 {
    ((hz / RATE_HZ.0).ln() / (RATE_HZ.1 / RATE_HZ.0).ln()).clamp(0.0, 1.0)
}

/// A ±dB dial's linear gain. Noon is exactly 1: `powf(0)` is exact.
fn gain_at(v: f32, range: (f32, f32)) -> f32 {
    let db = range.0 + (range.1 - range.0) * v.clamp(0.0, 1.0);
    10f32.powf(db / 20.0)
}

/// The quantizer's step at a bit depth, as a fraction of full scale (±1).
pub fn step_of(bits: f32) -> f32 {
    (1.0 - bits).exp2()
}

/// Round `x` to the nearest multiple of `step`: the mid-tread converter.
#[inline]
pub fn quantize(x: f32, step: f32) -> f32 {
    (x / step).round() * step
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

// ── the parameters ──────────────────────────────────────────────────────────

/// Every dial, normalised 0..=1. The mapping to what the DSP actually wants
/// lives in the consts above, next to the reasons.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BitcrushParams {
    /// Across [`BITS_RANGE`], linear; the top is the quantizer out.
    pub bits: f32,
    /// Across [`RATE_HZ`], geometric; the top is the host's own rate.
    pub rate: f32,
    /// Across [`DRIVE_DB`]; noon is unity.
    pub drive: f32,
    /// Crushed share of the output, 0 dry .. 1 crushed.
    pub mix: f32,
    /// Across [`OUTPUT_DB`]; noon is unity.
    pub output: f32,
    /// A toggle (≥ 0.5 is on): TPDF dither of ±1 step before the quantizer.
    pub dither: f32,
    /// A toggle (≥ 0.5 is on): the anti-alias low-pass before the hold.
    pub aa: f32,
}

/// **The shipped sound: 8 bits at about 12 kHz, fully wet, unity levels,
/// no dither**: the sampler-era grit. Unmistakable the moment anything
/// with top end goes through it (the hold folds the treble back down as
/// metallic overtones and the grid hashes the decays), while the low end,
/// which neither the clock nor eight bits can do much to, is still the
/// song.
impl Default for BitcrushParams {
    fn default() -> Self {
        Self {
            bits: BITS_DEFAULT,
            rate: RATE_DEFAULT,
            drive: 0.5,
            mix: 1.0,
            output: 0.5,
            dither: 0.0,
            aa: 0.0,
        }
    }
}

/// Eight bits, as a dial position: `(8 − 1) / 23`. A const so the face table
/// can name it; a test holds it to [`bits_dial`].
pub const BITS_DEFAULT: f32 = 7.0 / 23.0;

/// About 12.2 kHz, as a dial position: three quarters of the geometric sweep.
pub const RATE_DEFAULT: f32 = 0.75;

// ── the smoother ────────────────────────────────────────────────────────────

/// A one-pole glide that lands: within `snap` of its target it IS the target,
/// so a settled knob is an exact number and never a denormal tail.
#[derive(Clone, Copy, Debug)]
struct Glide {
    cur: f64,
    target: f64,
    snap: f64,
}

impl Glide {
    const fn new(snap: f64) -> Self {
        Self { cur: 0.0, target: 0.0, snap }
    }

    #[inline]
    fn tick(&mut self, a: f64) -> f64 {
        if self.cur != self.target {
            let next = self.cur + a * (self.target - self.cur);
            self.cur = if (self.target - next).abs() <= self.snap {
                self.target
            } else {
                next
            };
        }
        self.cur
    }
}

// ── the anti-alias filter ───────────────────────────────────────────────────

/// Where the anti-alias cutoff sits, as a share of the crushed rate: under
/// its Nyquist (0.5) by enough that the filter's knee is past before the
/// fold begins.
pub const AA_CUTOFF: f32 = 0.45;

/// The four sections' Q for an 8th-order Butterworth: `1 / (2 cos θ)` at
/// θ = 11.25°, 33.75°, 56.25° and 78.75°.
const AA_Q: [f32; 4] = [0.509_795_6, 0.601_344_9, 0.899_976_2, 2.562_915_4];

/// Below this a filter state is on its way to a denormal and is zero.
const TINY: f32 = 1.0e-20;

/// One channel's four state-variable sections (Cytomic's trapezoidal SVF):
/// two integrator states each. Stable for any cutoff sequence, which is why
/// it is this topology and not a direct-form biquad.
#[derive(Clone, Copy, Default)]
struct AaChannel {
    ic: [[f32; 2]; 4],
}

/// The coefficients all channels share for one frame.
#[derive(Clone, Copy)]
struct AaCoeffs {
    a: [[f32; 3]; 4],
}

impl AaCoeffs {
    fn at(g: f32) -> Self {
        let mut a = [[0.0; 3]; 4];
        for (row, q) in a.iter_mut().zip(AA_Q) {
            let k = 1.0 / q;
            let a1 = 1.0 / (1.0 + g * (g + k));
            let a2 = g * a1;
            *row = [a1, a2, g * a2];
        }
        Self { a }
    }
}

impl AaChannel {
    #[inline]
    fn tick(&mut self, c: &AaCoeffs, x: f32) -> f32 {
        let mut v0 = x;
        for (st, [a1, a2, a3]) in self.ic.iter_mut().zip(c.a) {
            let v3 = v0 - st[1];
            let v1 = a1 * st[0] + a2 * v3;
            let v2 = st[1] + a2 * st[0] + a3 * v3;
            st[0] = 2.0 * v1 - st[0];
            st[1] = 2.0 * v2 - st[1];
            for s in st.iter_mut() {
                if s.abs() < TINY || !s.is_finite() {
                    *s = 0.0;
                }
            }
            v0 = v2;
        }
        v0
    }
}

// ── the crusher ─────────────────────────────────────────────────────────────

/// How many channels the kernel keeps state for: one converter is a stereo
/// pair at most. Channels past this are left untouched, the house contract
/// for a stride wider than the unit. Named for the crusher, because the
/// bare word is the fixed desk's and `ban_scan` refuses it.
pub const CRUSH_CHANNELS: usize = 2;

/// The bitcrusher. See the module header for the path and the wire.
pub struct Bitcrush {
    sample_rate: f64,
    /// The one-pole coefficient every glide rides, from [`SMOOTH_MS`].
    a: f64,
    bits: Glide,
    /// Samples-per-hold's reciprocal: how far the clock moves per sample,
    /// 1.0 at the host's own rate.
    step: Glide,
    drive: Glide,
    mix: Glide,
    output: Glide,
    /// The clock's phase, in holds. A new sample is taken whenever it
    /// reaches 1. Starts at 1 so the first sample after a clear is taken.
    phase: f64,
    /// What each channel's hold is holding, already quantized.
    held: [f32; CRUSH_CHANNELS],
    /// The dither's noise, one generator per channel so a stereo crush
    /// does not dither both sides with the same numbers.
    rng: [u32; CRUSH_CHANNELS],
    /// Whether the glides have been handed their first targets since
    /// construction or the last clear. See the module header.
    primed: bool,
    /// How much of the anti-alias filter is in the path, 0..1: the switch
    /// and RATE's "full" fade it rather than cut it.
    aa_mix: Glide,
    aa: [AaChannel; CRUSH_CHANNELS],
    /// Whether the sections hold anything, so an idle filter is reset once
    /// rather than every sample.
    aa_live: bool,
    /// The clock step the coefficients were computed for.
    aa_for: f64,
    aa_coeffs: AaCoeffs,
    /// The bit depth [`Self::grid`] was computed for, so a settled depth
    /// costs no `exp2` per sample.
    grid_for: f32,
    grid: f32,
}

impl Bitcrush {
    pub fn new(sample_rate: f32) -> Self {
        let sample_rate = f64::from(sample_rate.max(8_000.0));
        Self {
            sample_rate,
            a: 1.0 - (-1.0 / (f64::from(SMOOTH_MS) * 0.001 * sample_rate)).exp(),
            bits: Glide::new(1.0e-4),
            step: Glide::new(1.0e-9),
            drive: Glide::new(1.0e-7),
            mix: Glide::new(1.0e-7),
            output: Glide::new(1.0e-7),
            phase: 1.0,
            held: [0.0; CRUSH_CHANNELS],
            rng: [0x9E37_79B9, 0x7F4A_7C15],
            primed: false,
            aa_mix: Glide::new(1.0e-7),
            aa: [AaChannel::default(); CRUSH_CHANNELS],
            aa_live: false,
            aa_for: f64::NAN,
            aa_coeffs: AaCoeffs::at(1.0),
            grid_for: f32::NAN,
            grid: 1.0,
        }
    }

    /// Hand every glide its target from the dials, once per block.
    fn aim(&mut self, p: &BitcrushParams) {
        let rest = BitcrushParams::default();
        self.bits.target = f64::from(bits_at(dial(p.bits, rest.bits)));
        self.step.target = match rate_at(dial(p.rate, rest.rate)) {
            Some(hz) => (f64::from(hz) / self.sample_rate).min(1.0),
            None => 1.0,
        };
        self.drive.target = f64::from(gain_at(dial(p.drive, rest.drive), DRIVE_DB));
        self.mix.target = f64::from(dial(p.mix, rest.mix));
        self.output.target = f64::from(gain_at(dial(p.output, rest.output), OUTPUT_DB));
        // In the path only when switched on AND there is something to fold:
        // at "full" the hold takes every sample and the filter would only
        // colour the treble.
        let aa_on = dial(p.aa, 0.0) >= 0.5 && self.step.target < 1.0;
        self.aa_mix.target = if aa_on { 1.0 } else { 0.0 };
        if !self.primed {
            for g in [
                &mut self.aa_mix,
                &mut self.bits,
                &mut self.step,
                &mut self.drive,
                &mut self.mix,
                &mut self.output,
            ] {
                g.cur = g.target;
            }
            self.primed = true;
        }
    }

    /// One TPDF draw in (−1, 1): the sum of two uniform draws, centred.
    #[inline]
    fn tpdf(&mut self, ch: usize) -> f32 {
        let mut next = || {
            let mut x = self.rng[ch];
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            self.rng[ch] = x;
            (x >> 8) as f32 * (1.0 / 16_777_216.0)
        };
        next() + next() - 1.0
    }

    /// One frame: advance every glide and the clock once, then crush each of
    /// the first `n` channels of `frame` in place.
    #[inline]
    fn frame(&mut self, frame: &mut [f32; CRUSH_CHANNELS], n: usize, dither: bool) {
        let a = self.a;
        let bits = self.bits.tick(a) as f32;
        let step = self.step.tick(a);
        let drive = self.drive.tick(a) as f32;
        let mix = self.mix.tick(a) as f32;
        let output = self.output.tick(a) as f32;
        let aa_mix = self.aa_mix.tick(a) as f32;
        let aa = if aa_mix > 0.0 {
            if step != self.aa_for {
                self.aa_for = step;
                // The cutoff follows the clock as it glides; the prewarp is
                // capped just under Nyquist so a step near one stays finite.
                let fc = (f64::from(AA_CUTOFF) * step).min(0.49);
                self.aa_coeffs = AaCoeffs::at((std::f64::consts::PI * fc).tan() as f32);
            }
            self.aa_live = true;
            Some((self.aa_coeffs, aa_mix))
        } else {
            // Out of circuit: forget the filter so the next time it fades in
            // it starts from rest, not from a tail of some older signal.
            if self.aa_live {
                self.aa = [AaChannel::default(); CRUSH_CHANNELS];
                self.aa_live = false;
            }
            None
        };
        // The quantizer is out of circuit at the top of the BITS travel. The
        // step is recomputed only while the depth is moving.
        let grid = if bits < BITS_RANGE.1 {
            if bits != self.grid_for {
                self.grid_for = bits;
                self.grid = step_of(bits);
            }
            Some(self.grid)
        } else {
            None
        };

        // A hair of tolerance on the wrap: a step like 200/48000 is not
        // exact in binary, and 240 of them land a few ulps short of one,
        // which would make an integer ratio hold for 241 samples once a
        // cycle. The wrap still subtracts exactly one, so the average rate
        // is untouched.
        let take = self.phase >= 1.0 - CLOCK_SLACK;
        if take {
            // The step never exceeds one, so the phase never reaches two and
            // one subtraction is the whole wrap.
            self.phase -= 1.0;
        }
        self.phase += step;

        for (ch, x) in frame.iter_mut().enumerate().take(n) {
            let dry = if x.is_finite() { *x } else { 0.0 };
            let mut driven = dry * drive;
            if !driven.is_finite() {
                driven = 0.0;
            }
            // The filter runs on EVERY sample, held or not: it is a filter
            // on the signal, and the hold samples its output.
            if let Some((c, m)) = &aa {
                let lp = self.aa[ch].tick(c, driven);
                driven = if *m >= 1.0 { lp } else { driven + m * (lp - driven) };
                if !driven.is_finite() {
                    driven = 0.0;
                }
            }
            if take {
                let mut s = driven;
                if let Some(q) = grid {
                    if dither {
                        s += self.tpdf(ch) * q;
                    }
                    s = quantize(s, q);
                    if !s.is_finite() {
                        s = 0.0;
                    }
                }
                self.held[ch] = s;
            }
            let wet = self.held[ch];
            // The two ends of MIX are branches rather than the blend's
            // arithmetic, so a negative zero at either end stays negative:
            // `-0.0 + 0.0` is `+0.0`, and a wire must not flip a sign bit.
            // Between them the blend is spelled as a move away from the dry,
            // and a crushed sample equal to its dry is the dry, so a wire
            // stays a wire at every MIX and not only at its ends.
            let blend = if mix >= 1.0 {
                wet
            } else if mix <= 0.0 || wet == dry {
                dry
            } else {
                dry + mix * (wet - dry)
            };
            let y = blend * output;
            *x = if y.is_finite() { y } else { 0.0 };
        }
    }

    /// Crush an interleaved buffer in place. `channels` is the stride; the
    /// first two channels are processed (ONE channel is crushed as mono,
    /// not passed) and any past the second are left alone.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, channels: usize, params: &BitcrushParams) {
        if channels == 0 {
            return;
        }
        self.aim(params);
        let dither = dial(params.dither, 0.0) >= 0.5;
        let n = channels.min(CRUSH_CHANNELS);
        for f in buf.chunks_exact_mut(channels).take(frames) {
            let mut fr = [0.0f32; CRUSH_CHANNELS];
            fr[..n].copy_from_slice(&f[..n]);
            self.frame(&mut fr, n, dither);
            f[..n].copy_from_slice(&fr[..n]);
        }
    }

    /// Crush planar channels in place, `frames` of each. One channel is
    /// mono, two are stereo; any past the second are left alone.
    pub fn process_planar<C: AsMut<[f32]>>(&mut self, chans: &mut [C], frames: usize, params: &BitcrushParams) {
        let n = chans.len().min(CRUSH_CHANNELS);
        if n == 0 {
            return;
        }
        self.aim(params);
        let dither = dial(params.dither, 0.0) >= 0.5;
        let frames = chans[..n]
            .iter_mut()
            .map(|c| c.as_mut().len())
            .fold(frames, usize::min);
        for f in 0..frames {
            let mut fr = [0.0f32; CRUSH_CHANNELS];
            for (c, slot) in chans[..n].iter_mut().zip(fr.iter_mut()) {
                *slot = c.as_mut()[f];
            }
            self.frame(&mut fr, n, dither);
            for (c, v) in chans[..n].iter_mut().zip(fr) {
                c.as_mut()[f] = v;
            }
        }
    }

    /// Forget the signal: the holds empty, the clock restarts on the next
    /// sample, and every knob settles onto whatever the next block asks for
    /// rather than gliding there. The dither's generators keep running.
    pub fn clear(&mut self) {
        self.phase = 1.0;
        self.held = [0.0; CRUSH_CHANNELS];
        self.aa = [AaChannel::default(); CRUSH_CHANNELS];
        self.primed = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// The wire: 24 bits, full rate, unity both ways; mix as asked.
    fn wire(mix: f32) -> BitcrushParams {
        BitcrushParams {
            bits: 1.0,
            rate: 1.0,
            drive: 0.5,
            mix,
            output: 0.5,
            dither: 0.0,
            aa: 0.0,
        }
    }

    /// Only the clock: full bit depth, unity levels, fully wet.
    fn hold_only(rate: f32) -> BitcrushParams {
        BitcrushParams { rate, ..wire(1.0) }
    }

    /// Only the grid: full rate, unity levels, fully wet.
    fn grid_only(bits: f32) -> BitcrushParams {
        BitcrushParams { bits: bits_dial(bits), ..wire(1.0) }
    }

    /// Stereo in, stereo out, interleaved.
    fn run(p: &BitcrushParams, input: &[f32]) -> Vec<f32> {
        let mut k = Bitcrush::new(SR);
        let mut buf = input.to_vec();
        k.process(&mut buf, input.len() / 2, 2, p);
        buf
    }

    /// A signal with the awkward values in it: both zeros, a denormal, the
    /// rails, and a sine that touches every level in between.
    fn awkward(frames: usize) -> Vec<f32> {
        let odd = [0.0f32, -0.0, 1.0e-39, -1.0e-40, 1.0, -1.0, 3.1e-4, 0.999_999_9];
        (0..frames * 2)
            .map(|i| {
                if i % 7 == 0 {
                    odd[(i / 7) % odd.len()]
                } else {
                    (i as f32 * 0.0137).sin() * 0.83
                }
            })
            .collect()
    }

    /// **The wire is bit-exact**: 24 bits, full rate, unity levels and mix
    /// 100% hands back every input to the bit, both zeros and the denormals
    /// included; so does mix 0% whatever the crusher is doing behind it.
    #[test]
    fn the_wire_settings_are_bit_exact() {
        let input = awkward(4_096);
        for p in [
            wire(1.0),
            wire(0.0),
            wire(0.37),
            // The anti-alias switch on changes nothing at "full".
            BitcrushParams { aa: 1.0, ..wire(1.0) },
            BitcrushParams { aa: 1.0, ..wire(0.37) },
            BitcrushParams { mix: 0.0, ..BitcrushParams::default() },
            BitcrushParams { mix: 0.0, bits: 0.0, rate: 0.0, dither: 1.0, ..BitcrushParams::default() },
        ] {
            let out = run(&p, &input);
            for (i, (a, b)) in input.iter().zip(&out).enumerate() {
                assert_eq!(a.to_bits(), b.to_bits(), "{p:?} moved sample {i}: {a:e} came out {b:e}");
            }
        }
    }

    /// **N bits is a grid of 2^(1−N)**: every output is a whole number of
    /// steps, and a slow ramp across full scale visits consecutive levels
    /// that far apart, at 1, 4, 8 and 12 bits, and at a fractional depth.
    #[test]
    fn n_bits_quantize_to_a_step_of_two_to_the_one_minus_n() {
        let frames = 48_000;
        let ramp: Vec<f32> = (0..frames * 2)
            .map(|i| -1.0 + 2.0 * (i / 2) as f32 / frames as f32)
            .collect();
        for bits in [1.0f32, 4.0, 8.0, 12.0, 6.5] {
            let p = grid_only(bits);
            let out = run(&p, &ramp);
            let want = (1.0 - f64::from(bits_at(p.bits))).exp2();
            for (i, y) in out.iter().enumerate() {
                let steps = f64::from(*y) / want;
                assert!(
                    (steps - steps.round()).abs() < 1.0e-3,
                    "{bits} bits: sample {i} = {y} is {steps} steps of {want}"
                );
            }
            // Consecutive distinct levels are exactly one step apart.
            let mut levels: Vec<f32> = out.iter().step_by(2).copied().collect();
            levels.dedup();
            assert!(levels.len() > 2, "{bits} bits drew {} levels", levels.len());
            for w in levels.windows(2) {
                let gap = f64::from(w[1] - w[0]);
                assert!((gap - want).abs() / want < 1.0e-3, "{bits} bits stepped {gap}, wanted {want}");
            }
            // The error is never more than half a step.
            for (x, y) in ramp.iter().zip(&out) {
                assert!(f64::from((x - y).abs()) <= want / 2.0 + 1.0e-6);
            }
        }
        // And the kernel's own arithmetic, at the two ends.
        assert_eq!(step_of(1.0), 1.0);
        assert_eq!(step_of(8.0), 1.0 / 128.0);
        assert_eq!(quantize(0.6, 1.0), 1.0);
        assert_eq!(quantize(0.4, 1.0), 0.0);
    }

    /// **The hold is as long as the rate says**: at a rate that divides the
    /// host rate every hold is that many samples; at one that does not,
    /// every hold is one of the two neighbouring lengths and they average
    /// out to the dialled rate. Mono and stereo share the clock.
    #[test]
    fn the_hold_lasts_host_rate_over_dialled_rate_samples() {
        let frames = 96_000;
        // A signal that is never the same twice, so a run of equal outputs
        // IS a hold.
        let input: Vec<f32> = (0..frames * 2).map(|i| (i as f32 * 1.0e-5) - 0.9).collect();
        for hz in [12_000.0f32, 7_000.0, 1_000.0, 200.0] {
            let p = hold_only(rate_dial(hz));
            let got_hz = rate_at(p.rate).unwrap();
            let out = run(&p, &input);
            let left: Vec<f32> = out.iter().step_by(2).copied().collect();
            let right: Vec<f32> = out.iter().skip(1).step_by(2).copied().collect();
            let mut runs = Vec::new();
            let mut len = 1usize;
            for (i, w) in left.windows(2).enumerate() {
                // The two channels change on the same frame: one clock.
                assert_eq!(w[0] == w[1], right[i] == right[i + 1], "{hz} Hz: the channels' clocks split at {i}");
                if w[0] == w[1] {
                    len += 1;
                } else {
                    runs.push(len);
                    len = 1;
                }
            }
            let ratio = f64::from(SR) / f64::from(got_hz);
            let (lo, hi) = (ratio.floor() as usize, ratio.ceil() as usize);
            for (i, r) in runs.iter().enumerate() {
                assert!(*r == lo || *r == hi, "{hz} Hz: hold {i} lasted {r}, wanted {lo} or {hi}");
            }
            let mean = runs.iter().sum::<usize>() as f64 / runs.len() as f64;
            assert!((mean - ratio).abs() / ratio < 0.01, "{hz} Hz: mean hold {mean}, wanted {ratio}");
        }
        // The top of the travel is the host's own rate: no hold at all.
        assert_eq!(rate_at(1.0), None);
        let out = run(&hold_only(1.0), &input);
        assert_eq!(out, input);
    }

    /// **The same clock in mono**: one channel is crushed, not passed, and
    /// it crushes exactly as the left of a stereo pair does.
    #[test]
    fn a_mono_buffer_is_crushed_like_the_left_of_a_pair() {
        let frames = 4_800;
        let stereo = awkward(frames);
        let mono: Vec<f32> = stereo.iter().step_by(2).copied().collect();
        let p = BitcrushParams::default();
        let s = run(&p, &stereo);
        let mut k = Bitcrush::new(SR);
        let mut m = mono.clone();
        k.process(&mut m, frames, 1, &p);
        assert_ne!(m, mono, "a mono buffer went through untouched");
        let left: Vec<f32> = s.iter().step_by(2).copied().collect();
        assert_eq!(m, left);
        // And the planar road agrees with the interleaved one.
        let mut k = Bitcrush::new(SR);
        let mut planar = [mono.clone()];
        k.process_planar(&mut planar, frames, &p);
        assert_eq!(planar[0], left);
    }

    /// **MIX is a straight blend and the levels are decibels**: drive and
    /// output at their ends are ±24 dB on the nose, mix at a half lands
    /// halfway between dry and crushed.
    #[test]
    fn mix_blends_and_the_level_knobs_read_true() {
        let input = awkward(2_048);
        let crushed = run(&BitcrushParams { mix: 1.0, ..BitcrushParams::default() }, &input);
        let half = run(&BitcrushParams { mix: 0.5, ..BitcrushParams::default() }, &input);
        for ((x, c), h) in input.iter().zip(&crushed).zip(&half) {
            assert!((h - (0.5 * x + 0.5 * c)).abs() < 1.0e-6, "{h} is not halfway between {x} and {c}");
        }
        let db = |a: f32, b: f32| 20.0 * (a / b).log10();
        let dc = vec![0.25f32; 2 * 2_048];
        let out = run(&BitcrushParams { output: 1.0, ..wire(1.0) }, &dc);
        assert!((db(out[4_000], 0.25) - 24.0).abs() < 1.0e-3);
        let out = run(&BitcrushParams { output: 0.0, ..wire(1.0) }, &dc);
        assert!((db(out[4_000], 0.25) + 24.0).abs() < 1.0e-3);
        // Drive is the level INTO the crusher: at the wire it is a gain, and
        // the dry half of a mix never hears it.
        let out = run(&BitcrushParams { drive: 1.0, ..wire(1.0) }, &dc);
        assert!((db(out[4_000], 0.25) - 24.0).abs() < 1.0e-3);
        let out = run(&BitcrushParams { drive: 1.0, ..wire(0.0) }, &dc);
        assert_eq!(out[4_000], 0.25);
        // Driven into a coarse grid, more of the grid is used: the error
        // relative to the signal falls when the drive rises and the output
        // brings it back down.
        let sine: Vec<f32> = (0..2 * 9_600).map(|i| ((i / 2) as f32 * 0.01).sin() * 0.05).collect();
        let err = |drive: f32| {
            let out = run(
                &BitcrushParams {
                    drive,
                    output: 1.0 - drive,
                    ..grid_only(6.0)
                },
                &sine,
            );
            sine.iter().zip(&out).map(|(x, y)| (x - y) * (x - y)).sum::<f32>()
        };
        assert!(err(0.75) < err(0.5) * 0.1, "driving into the grid did not use more of it");
    }

    /// **Dither is noise on the grid, and only when asked**: off, the same
    /// input crushes the same way twice and a quiet sine below half a step
    /// vanishes; on, that sine survives as an average and the output still
    /// sits on the grid.
    #[test]
    fn dither_is_off_by_default_and_keeps_a_quiet_signal_alive_when_on() {
        assert_eq!(BitcrushParams::default().dither, 0.0);
        let frames = 48_000;
        // A third of a step at 8 bits: rounds to silence undithered.
        let step = step_of(8.0);
        let quiet: Vec<f32> = (0..frames * 2)
            .map(|i| ((i / 2) as f32 * 2.0 * std::f32::consts::PI * 100.0 / SR).sin() * step / 3.0)
            .collect();
        let off = run(&grid_only(8.0), &quiet);
        assert!(off.iter().all(|y| *y == 0.0), "a sub-step sine leaked through undithered");
        let on = run(&BitcrushParams { dither: 1.0, ..grid_only(8.0) }, &quiet);
        for y in &on {
            let n = y / step;
            assert!((n - n.round()).abs() < 1.0e-4, "a dithered sample left the grid: {y}");
        }
        // Correlate with the input: the sine is still in there.
        let num: f32 = quiet.iter().zip(&on).map(|(x, y)| x * y).sum();
        let den: f32 = quiet.iter().map(|x| x * x).sum();
        let gain = num / den;
        assert!((gain - 1.0).abs() < 0.2, "the dithered sine came back at {gain:.2} of itself");
    }

    /// **No dial and no input makes it emit anything but a finite sample**:
    /// every corner of the panel, against a signal with NaN, infinities, the
    /// largest floats and denormals in it, block after block.
    #[test]
    fn no_setting_and_no_input_makes_it_explode() {
        let frames = 4_096;
        let nasty = [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX, -f32::MAX, 1.0e-42, -0.0, 3.0];
        let input: Vec<f32> = (0..frames * 2)
            .map(|i| if i % 5 == 0 { nasty[(i / 5) % nasty.len()] } else { (i as f32 * 0.01).sin() * 4.0 })
            .collect();
        for bits in 0..64u32 {
            let d = |b: u32| [0.0f32, 1.0][(bits >> b & 1) as usize];
            let p = BitcrushParams {
                bits: d(0),
                rate: d(1),
                drive: d(2),
                mix: d(3),
                output: d(4),
                dither: d(5),
                aa: if bits % 3 == 0 { 1.0 } else { 0.0 },
            };
            let mut k = Bitcrush::new(SR);
            for _ in 0..3 {
                let mut buf = input.clone();
                k.process(&mut buf, frames, 2, &p);
                assert!(buf.iter().all(|y| y.is_finite()), "{p:?} emitted a non-finite sample");
            }
        }
        // A dial that is itself garbage reads as its rest.
        let mut k = Bitcrush::new(SR);
        let mut buf = input.clone();
        let p = BitcrushParams { bits: f32::NAN, rate: f32::INFINITY, drive: -3.0, mix: f32::NAN, output: 9.0, dither: f32::NAN, aa: f32::NAN };
        k.process(&mut buf, frames, 2, &p);
        assert!(buf.iter().all(|y| y.is_finite()));
    }

    /// **Every knob glides; none clicks.** A gain slammed from one end to the
    /// other moves a DC level by no more than one glide-step per sample
    /// rather than all at once, the bit depth and the clock slide rather than
    /// jump, and every glide lands EXACTLY on its target, which is what
    /// makes a settled wire bit-exact.
    #[test]
    fn every_knob_glides_and_lands_exactly() {
        let frames = 48_000;
        let dc = vec![0.25f32; frames * 2];
        let a = 1.0 / (f64::from(SMOOTH_MS) * 0.001 * f64::from(SR));
        for (from, to) in [
            (BitcrushParams { output: 0.0, ..wire(1.0) }, BitcrushParams { output: 1.0, ..wire(1.0) }),
            (BitcrushParams { drive: 1.0, ..wire(1.0) }, BitcrushParams { drive: 0.0, ..wire(1.0) }),
            (BitcrushParams { output: 1.0, drive: 0.0, ..wire(1.0) }, BitcrushParams { output: 1.0, drive: 0.0, ..wire(0.0) }),
        ] {
            let mut k = Bitcrush::new(SR);
            let mut buf = dc.clone();
            k.process(&mut buf, frames, 2, &from);
            let before = buf[buf.len() - 2];
            let mut buf = dc.clone();
            k.process(&mut buf, frames, 2, &to);
            let after = buf[buf.len() - 2];
            let span = (after - before).abs();
            assert!(span > 0.1, "the knob moved nothing: {before} -> {after}");
            let mut prev = before;
            let mut worst = 0.0f32;
            for f in buf.chunks_exact(2) {
                worst = worst.max((f[0] - prev).abs());
                prev = f[0];
            }
            assert!(
                f64::from(worst) <= f64::from(span) * a * 1.05,
                "a {span} move jumped {worst} in one sample ({from:?} -> {to:?})"
            );
            // Landed: the last block of a settled knob is one exact value.
            assert!(buf[buf.len() - 200..].chunks_exact(2).all(|f| f[0] == after));
        }
        // The depth and the clock: one sample after a full-travel move, each
        // has gone one glide-step of the way and no further, and a few
        // hundred milliseconds later each sits exactly on its target.
        let mut k = Bitcrush::new(SR);
        let mut buf = dc.clone();
        k.process(&mut buf, frames, 2, &wire(1.0));
        let mut one = [0.25f32, 0.25];
        let to = BitcrushParams { bits: 0.0, rate: 0.0, ..wire(1.0) };
        k.process(&mut one, 1, 2, &to);
        assert!((k.bits.cur - 24.0).abs() < 23.0 * a * 1.01 + 1.0e-9, "bits jumped to {}", k.bits.cur);
        assert!((k.step.cur - 1.0).abs() < a * 1.01, "the clock jumped to {}", k.step.cur);
        let mut long = vec![0.25f32; 2 * 48_000];
        k.process(&mut long, 48_000, 2, &to);
        assert_eq!(k.bits.cur, 1.0);
        assert_eq!(k.step.cur, f64::from(RATE_HZ.0) / f64::from(SR));
    }

    /// **A fresh or cleared unit starts AT its knobs**, not gliding to them:
    /// the first sample of a restored preset is already the preset.
    #[test]
    fn a_fresh_or_cleared_unit_starts_settled() {
        let dc = vec![0.25f32; 2 * 64];
        let p = BitcrushParams { output: 1.0, ..wire(1.0) };
        let out = run(&p, &dc);
        assert!((out[0] - 0.25 * 10f32.powf(24.0 / 20.0)).abs() < 1.0e-4, "a fresh unit glided: {}", out[0]);
        let mut k = Bitcrush::new(SR);
        let mut buf = dc.clone();
        k.process(&mut buf, 64, 2, &BitcrushParams::default());
        k.clear();
        assert_eq!(k.held, [0.0; CRUSH_CHANNELS], "the holds kept what they held");
        assert_eq!(k.phase, 1.0, "the clock did not restart");
        let mut buf = dc.clone();
        k.process(&mut buf, 64, 2, &p);
        assert!((buf[0] - 0.25 * 10f32.powf(24.0 / 20.0)).abs() < 1.0e-4, "a cleared unit glided: {}", buf[0]);
    }

    /// **The same crush at every sample rate**: a 1 kHz clock holds for
    /// rate/1000 samples at 44.1, 48 and 96 kHz: the clock is Hz, not a
    /// sample count.
    #[test]
    fn the_clock_is_hertz_at_every_host_rate() {
        for sr in [44_100.0f32, 48_000.0, 96_000.0] {
            let frames = sr as usize;
            let input: Vec<f32> = (0..frames * 2).map(|i| i as f32 * 1.0e-6).collect();
            let mut k = Bitcrush::new(sr);
            let mut buf = input.clone();
            let p = hold_only(rate_dial(1_000.0));
            k.process(&mut buf, frames, 2, &p);
            let mut changes = 0usize;
            for w in buf.chunks_exact(2).collect::<Vec<_>>().windows(2) {
                if w[0][0] != w[1][0] {
                    changes += 1;
                }
            }
            let want = f64::from(rate_at(p.rate).unwrap());
            assert!((changes as f64 - want).abs() <= 2.0, "at {sr} a 1 kHz clock took {changes} samples a second");
        }
    }

    /// The maps and their inverses agree, and the shipped rests are the
    /// numbers the header promises.
    #[test]
    fn the_maps_invert_and_the_rests_are_eight_bits_at_twelve_k() {
        for v in [0.0f32, 0.1, 0.5, 0.9] {
            assert!((bits_dial(bits_at(v)) - v).abs() < 1.0e-6);
            assert!((rate_dial(rate_at(v).unwrap()) - v).abs() < 1.0e-5);
        }
        assert!((bits_at(BITS_DEFAULT) - 8.0).abs() < 1.0e-5);
        assert!((bits_dial(8.0) - BITS_DEFAULT).abs() < 1.0e-6);
        let hz = rate_at(RATE_DEFAULT).unwrap();
        assert!((hz - 12_190.0).abs() < 10.0, "the shipped clock is {hz} Hz");
        assert_eq!(gain_at(0.5, DRIVE_DB), 1.0);
        assert_eq!(gain_at(0.5, OUTPUT_DB), 1.0);
    }

    /// The plain classic crusher, settled, written the obvious way: hold at
    /// a fractional clock, round to the grid, blend, trim. What the kernel
    /// must still be, sample for sample, with the anti-alias switch off.
    fn reference(p: &BitcrushParams, input: &[f32]) -> Vec<f32> {
        let bits = f64::from(bits_at(p.bits));
        let step = rate_at(p.rate).map_or(1.0, |hz| (f64::from(hz) / f64::from(SR)).min(1.0));
        let (drive, mix, output) = (gain_at(p.drive, DRIVE_DB), p.mix, gain_at(p.output, OUTPUT_DB));
        let mut phase = 1.0f64;
        let mut held = [0.0f32; 2];
        let mut out = input.to_vec();
        for f in out.chunks_exact_mut(2) {
            let take = phase >= 1.0 - CLOCK_SLACK;
            if take {
                phase -= 1.0;
            }
            phase += step;
            for (ch, x) in f.iter_mut().enumerate() {
                let dry = *x;
                if take {
                    let s = dry * drive;
                    held[ch] = if bits < 24.0 { quantize(s, step_of(bits as f32)) } else { s };
                }
                let wet = held[ch];
                let b = if mix >= 1.0 {
                    wet
                } else if mix <= 0.0 || wet == dry {
                    dry
                } else {
                    dry + mix * (wet - dry)
                };
                *x = b * output;
            }
        }
        out
    }

    /// **With the anti-alias switch off the crusher is the classic one, to
    /// the bit**: the kernel against the plain reference above across the
    /// panel's corners and its shipped rest.
    #[test]
    fn anti_alias_off_is_the_classic_crusher_to_the_bit() {
        let input: Vec<f32> = (0..2 * 9_600).map(|i| ((i / 2) as f32 * 0.0731).sin() * 0.7 + (i % 2) as f32 * 0.01).collect();
        for p in [
            BitcrushParams::default(),
            BitcrushParams { bits: 0.0, rate: 0.0, ..BitcrushParams::default() },
            BitcrushParams { bits: 0.5, rate: 0.3, drive: 0.8, mix: 0.6, output: 0.2, ..BitcrushParams::default() },
            BitcrushParams { rate: 1.0, ..BitcrushParams::default() },
        ] {
            assert_eq!(p.aa, 0.0);
            let got = run(&p, &input);
            let want = reference(&p, &input);
            for (i, (a, b)) in got.iter().zip(&want).enumerate() {
                assert_eq!(a.to_bits(), b.to_bits(), "{p:?} sample {i}: {a} vs the classic {b}");
            }
        }
    }

    /// The level of `hz` in one channel of an interleaved block, by
    /// projection onto a sine and a cosine (a single-bin DFT).
    fn level_at(buf: &[f32], ch: usize, hz: f32) -> f32 {
        let (mut re, mut im) = (0.0f64, 0.0f64);
        let mut n = 0usize;
        for (i, f) in buf.chunks_exact(2).enumerate() {
            let w = std::f64::consts::TAU * f64::from(hz) * i as f64 / f64::from(SR);
            re += f64::from(f[ch]) * w.cos();
            im += f64::from(f[ch]) * w.sin();
            n += 1;
        }
        (2.0 * (re * re + im * im).sqrt() / n as f64) as f32
    }

    /// **On, the anti-alias filter stops the fold**: at a 4 kHz clock a
    /// 4.4 kHz tone folds to 400 Hz; with the switch on that alias comes
    /// back more than 40 dB under where it sits with the switch off (the
    /// 8th-order Butterworth at 1.8 kHz is about 62 dB down at 4.4 kHz),
    /// while a 500 Hz tone, well inside the crushed band, loses under a dB.
    #[test]
    fn anti_alias_on_stops_a_tone_above_the_crushed_nyquist_from_folding() {
        let frames = 48_000;
        let tone = |hz: f32| -> Vec<f32> {
            (0..frames * 2)
                .map(|i| (std::f32::consts::TAU * hz * (i / 2) as f32 / SR).sin() * 0.5)
                .collect()
        };
        let p = BitcrushParams { rate: rate_dial(4_000.0), ..wire(1.0) };
        let alias = (rate_at(p.rate).unwrap() - 4_400.0).abs();
        let skip = 2 * 4_800;
        let high = tone(4_400.0);
        let off = run(&p, &high);
        let on = run(&BitcrushParams { aa: 1.0, ..p }, &high);
        let (a_off, a_on) = (level_at(&off[skip..], 0, alias), level_at(&on[skip..], 0, alias));
        let down = 20.0 * (a_off / a_on.max(1.0e-12)).log10();
        assert!(a_off > 0.1, "the fold this test measures is not there: {a_off}");
        assert!(down > 40.0, "the alias at {alias} Hz only fell {down:.1} dB");
        let low = tone(500.0);
        let off = run(&p, &low);
        let on = run(&BitcrushParams { aa: 1.0, ..p }, &low);
        let lost = 20.0 * (level_at(&off[skip..], 0, 500.0) / level_at(&on[skip..], 0, 500.0)).log10();
        assert!(lost.abs() < 1.0, "a 500 Hz tone moved {lost:.2} dB through the filter");
    }

    /// **Sweeping RATE as fast as automation can, with the filter in,
    /// never blows up**: the clock slammed between its ends every 64
    /// samples, and every 7, against a full-scale tone with spikes in it.
    /// Finite every sample and never far past full scale; then the switch
    /// and RATE's top fade the filter out and the wire comes back exact.
    #[test]
    fn anti_alias_survives_the_fastest_rate_sweeps_and_leaves_cleanly() {
        for every in [64usize, 7] {
            let mut k = Bitcrush::new(SR);
            let mut worst = 0.0f32;
            for b in 0..2_000 {
                let mut buf: Vec<f32> = (0..every * 2)
                    .map(|i| if i % 97 == 0 { 1.0 } else { ((b * every + i / 2) as f32 * 0.9).sin() })
                    .collect();
                let p = BitcrushParams { aa: 1.0, rate: if b % 2 == 0 { 0.0 } else { 0.999 }, ..wire(1.0) };
                k.process(&mut buf, every, 2, &p);
                for y in &buf {
                    assert!(y.is_finite(), "a sweep every {every} emitted {y}");
                    worst = worst.max(y.abs());
                }
            }
            assert!(worst < 4.0, "a sweep every {every} rang to {worst}");
            // Back to the wire with the switch still on: once the fade has
            // landed, every sample is its input again.
            let mut settle = vec![0.3f32; 2 * 48_000];
            k.process(&mut settle, 48_000, 2, &BitcrushParams { aa: 1.0, ..wire(1.0) });
            let input = awkward(1_024);
            let mut buf = input.clone();
            k.process(&mut buf, 1_024, 2, &BitcrushParams { aa: 1.0, ..wire(1.0) });
            for (a, b) in input.iter().zip(&buf) {
                assert_eq!(a.to_bits(), b.to_bits());
            }
        }
    }
}

