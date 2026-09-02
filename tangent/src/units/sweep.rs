//! The offline math for IR capture: exponential sweeps, inverse filters,
//! deconvolution, and the cleanup that turns a raw capture into a shippable
//! impulse response.
//!
//! # Why a sweep at all
//!
//! An exponential sine sweep (ESS, Farina AES #5093) spends equal time per
//! octave, so every harmonic a device generates arrives with a fixed time
//! ADVANCE over the fundamental: the order-N distortion product of an
//! exponential sweep is itself a time-advanced copy of the sweep, and after
//! deconvolution it collapses into a compact "harmonic impulse response"
//! sitting exactly `dt_N = T·ln(N)/ln(f2/f1)` seconds BEFORE the linear one
//! (Muller & Massarani JAES 49(6)). Distortion separates out of the linear
//! IR instead of smearing across it as it does with MLS or noise — which is
//! the whole reason this file exists, and also a free THD probe: the energy
//! parked at `-dt_N` IS the order-N distortion estimate.
//!
//! # The two deconvolution routes
//!
//! - **Classic (Farina)**: convolve the recording with the time-reversed
//!   sweep carrying a −6 dB/oct amplitude envelope. The log sweep's spectrum
//!   is pink (−3 dB/oct amplitude, it lingers on the lows), so sweep ⊛
//!   reversed-sweep is −6 dB/oct; tilting the reversed copy by +6 dB/oct
//!   (0 dB at the f2 end down to −6·log2(f2/f1) at f1) flattens the product
//!   back to a bandpassed Dirac. Linear FFT convolution, no wrap, so the
//!   harmonic images land at honest negative times before the peak — this is
//!   the route the distortion probe reads.
//! - **Reference division (Muller ch.5 / Farina AES-122)**: divide the
//!   recording's spectrum by a recorded reference pass of the same loop,
//!   `H = Y·conj(X)/(|X|² + eps(f))`, with Kirkeby-style regularization —
//!   eps tiny inside [f1..f2], large outside, half-cosine transitions. This
//!   is exact where the sweep put energy: the loop's own latency, converter
//!   ripple and interface coloration cancel in the quotient, and the direct
//!   impulse lands at t = 0 by construction. The envelope route above is a
//!   stationary-phase approximation and only flat to a fraction of a dB;
//!   when the reference exists (it always does for an electrical loop),
//!   division is the quality route.
//!
//! Everything here is offline: f64 throughout the math, f32 only at the
//! edges, allocation wherever it helps, no realtime constraints. The
//! realtime half of the story — playing a captured IR — lives next door in
//! `convolver.rs`.

// The IR wizard wires this now — `desktop::process_ir_capture` is the road
// — but a few pieces (`ess_with_fades`, the constants a caller might tune)
// remain spare parts for the next capture mode, so the allow stays.
#![allow(dead_code)]

// ── the numbers, and why ────────────────────────────────────────────────────

/// Default half-cosine fade-in for a generated sweep, in seconds. A sweep
/// that switches on abruptly rings the whole band below f1 (Muller 5.1:
/// "sudden switch-on ... unwanted ripple at the extremities of the
/// excitation spectrum"); 20 ms of half-cosine onto a 20 Hz start covers a
/// half cycle and keeps the ripple under a tenth of a dB.
pub const SWEEP_FADE_IN_S: f64 = 0.020;

/// Default fade-out, in seconds. Short on purpose: the sweep ends at its
/// TOP frequency, where a long fade would carve a visible notch out of the
/// last octave and Farina (AES-122) blames exactly that for HF pre-ringing.
/// 2 ms is dozens of cycles at 20 kHz — enough to kill the switch-off click,
/// too short to dent the spectrum.
pub const SWEEP_FADE_OUT_S: f64 = 0.002;

/// In-band Kirkeby regularization, relative to the reference spectrum's
/// power peak: −80 dB. Small enough that the division is exact to well below
/// any capture's noise floor, big enough that a notch in the reference
/// (a converter's last bin, a fade artifact) divides to "quiet" instead of
/// to a screaming resonance.
pub const KIRKEBY_EPS_IN: f64 = 1e-8;

/// Out-of-band regularization: −10 dB relative to the peak. Outside
/// [f1..f2] the sweep put no energy down, so whatever the division would
/// reconstruct there is pure noise gain; a large eps simply throws it away.
pub const KIRKEBY_EPS_OUT: f64 = 0.1;

/// Width of the half-cosine transition between the two eps regimes, in
/// octaves beyond each band edge. A third of an octave: sharp enough that
/// the passband is honestly the passband, soft enough that the time-domain
/// image of the transition rings for well under a millisecond.
pub const KIRKEBY_TRANSITION_OCTAVES: f64 = 1.0 / 3.0;

/// Sliding window for the tail-vs-floor level reading, in seconds. 20 ms
/// averages over a full cycle of anything above 50 Hz, so a reverb tail
/// reads as a level rather than as a wobble.
pub const TAIL_WINDOW_S: f64 = 0.020;

/// Half-cosine fade applied to the kept tail's last stretch after
/// truncation, in seconds. 10 ms: long enough that the cut is inaudible
/// under any tail, short next to the window that chose the cut point.
pub const TAIL_FADE_S: f64 = 0.010;

/// Truncate where the tail's sliding level sinks to within this many dB of
/// the measured floor. Cutting exactly AT the floor keeps a window of pure
/// noise; cutting 3 dB shy keeps the last audible tail and drops the hiss.
pub const TAIL_MARGIN_DB: f64 = 3.0;

/// The furthest under an IR's own peak [`recovered_floor_db`] will ever put
/// the truncation threshold, in dB.
///
/// A deconvolution has enormous processing gain — a fifteen-second sweep
/// concentrates its whole energy into one impulse — so a clean capture's
/// recovered noise floor can sit 140 dB or more below the peak, and a
/// threshold derived from it refuses to cut at all: the IR comes back the
/// length of the whole capture window, seconds of inaudible nothing that
/// the convolver then pays for in partitions every block, forever. Measured
/// on a −3 dBFS synthetic capture through a −96 dBFS loop, the uncapped
/// threshold produced a twenty-six-second IR of a three-second plate.
///
/// 80 dB, for two reasons that happen to agree. It is past the point where
/// any playback gain lifts a tail back into hearing. And it is about as deep
/// as [`deconvolve_by_reference`] can actually reach: the Kirkeby
/// regularization's own ringing settles around 82 dB under the recovered
/// peak, so a threshold set below that is a threshold no window ever
/// crosses.
pub const TAIL_RANGE_MAX_DB: f64 = 80.0;

/// How far below the peak a sample still counts as part of the direct
/// impulse when hunting for its foot. −40 dB: below the pre-ringing a
/// bandpassed Dirac carries, above any capture's noise floor.
pub const ONSET_FLOOR_DB: f64 = -40.0;

/// Default peak target for a normalized IR set, in dBFS. Voxengo's −0.2 dB
/// convention: full scale minus enough headroom that a resampler's
/// interpolation overshoot does not clip on load.
pub const NORM_TARGET_DB: f32 = -0.2;

// ── the FFT ─────────────────────────────────────────────────────────────────

/// In-place radix-2 complex FFT, f64, sized for long buffers.
///
/// The same idiom as the f32 one in `stretch.rs`, rewritten here in f64
/// because a deconvolution wants −60 dB error norms out of transforms a
/// couple of million points long — the recurrence twiddle drifts by about a
/// part in 1e-10 at that length in f64, which is nothing, and by a part in
/// 1e-3 in f32, which is everything. Silently does nothing unless the
/// lengths match and are a power of two, mirroring the house guard.
pub fn fft64(re: &mut [f64], im: &mut [f64], inverse: bool) {
    let n = re.len();
    if n < 2 || n & (n - 1) != 0 || im.len() != n {
        return;
    }
    // Bit-reversal permutation.
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let mut len = 2usize;
    while len <= n {
        let ang = std::f64::consts::TAU / len as f64 * if inverse { 1.0 } else { -1.0 };
        let (wr, wi) = (ang.cos(), ang.sin());
        let mut i = 0;
        while i < n {
            let (mut cr, mut ci) = (1.0f64, 0.0f64);
            for k in 0..len / 2 {
                let (ur, ui) = (re[i + k], im[i + k]);
                let (vr, vi) = (
                    re[i + k + len / 2] * cr - im[i + k + len / 2] * ci,
                    re[i + k + len / 2] * ci + im[i + k + len / 2] * cr,
                );
                re[i + k] = ur + vr;
                im[i + k] = ui + vi;
                re[i + k + len / 2] = ur - vr;
                im[i + k + len / 2] = ui - vi;
                let nr = cr * wr - ci * wi;
                ci = cr * wi + ci * wr;
                cr = nr;
            }
            i += len;
        }
        len <<= 1;
    }
    if inverse {
        let s = 1.0 / n as f64;
        for v in re.iter_mut() {
            *v *= s;
        }
        for v in im.iter_mut() {
            *v *= s;
        }
    }
}

fn next_pow2(n: usize) -> usize {
    n.next_power_of_two()
}

// ── sweep generation ────────────────────────────────────────────────────────

/// Clamp a requested band to something the sample rate can carry: f1 at
/// least 1 Hz, f2 no higher than 0.98 of Nyquist (a sweep generated in the
/// time domain that runs hard into Nyquist folds its last milliseconds into
/// aliases), and the two an audible distance apart.
fn sane_band(f1: f64, f2: f64, sample_rate: f64) -> Option<(f64, f64)> {
    if !(f1.is_finite() && f2.is_finite() && sample_rate.is_finite()) || sample_rate <= 0.0 {
        return None;
    }
    let f1 = f1.max(1.0);
    let f2 = f2.min(0.98 * sample_rate * 0.5);
    if f2 <= f1 * 1.0001 {
        return None;
    }
    Some((f1, f2))
}

/// The sweep itself, in f64, shared by [`ess`] and [`inverse_filter`] so the
/// two are sample-identical before the reversal.
fn ess64(f1: f64, f2: f64, duration: f64, sample_rate: f64, fade_in: f64, fade_out: f64) -> Vec<f64> {
    let Some((f1, f2)) = sane_band(f1, f2, sample_rate) else {
        return Vec::new();
    };
    if !(duration.is_finite() && duration > 0.0) {
        return Vec::new();
    }
    let len = (duration * sample_rate).round() as usize;
    if len < 2 {
        return Vec::new();
    }
    // Farina 2000, verbatim:
    //   x(t) = sin[ (w1·T / ln(w2/w1)) · ( e^{(t/T)·ln(w2/w1)} − 1 ) ]
    let lr = (f2 / f1).ln();
    let k = std::f64::consts::TAU * f1 * duration / lr;
    let mut x: Vec<f64> = (0..len)
        .map(|i| {
            let t = i as f64 / sample_rate;
            (k * ((t / duration * lr).exp() - 1.0)).sin()
        })
        .collect();
    // Half-cosine fades, the "indispensable" ones (Muller 5.1/5.2). Each is
    // capped at half the sweep so a silly duration cannot make them cross.
    let nin = ((fade_in.max(0.0) * sample_rate) as usize).min(len / 2);
    for i in 0..nin {
        x[i] *= 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / nin as f64).cos());
    }
    let nout = ((fade_out.max(0.0) * sample_rate) as usize).min(len / 2);
    for i in 0..nout {
        let w = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / nout as f64).cos());
        x[len - 1 - i] *= w;
    }
    x
}

/// An exponential sine sweep from `f1` to `f2` Hz over `duration` seconds,
/// with the default fades ([`SWEEP_FADE_IN_S`], [`SWEEP_FADE_OUT_S`]).
/// Returns an empty Vec when the request is nonsense (non-finite, band
/// inverted, f2 past Nyquist headroom) rather than guessing.
pub fn ess(f1: f64, f2: f64, duration: f64, sample_rate: f64) -> Vec<f32> {
    ess_with_fades(f1, f2, duration, sample_rate, SWEEP_FADE_IN_S, SWEEP_FADE_OUT_S)
}

/// [`ess`] with the fades dialled explicitly, in seconds.
pub fn ess_with_fades(
    f1: f64,
    f2: f64,
    duration: f64,
    sample_rate: f64,
    fade_in: f64,
    fade_out: f64,
) -> Vec<f32> {
    ess64(f1, f2, duration, sample_rate, fade_in, fade_out)
        .into_iter()
        .map(|v| v as f32)
        .collect()
}

/// The classic Farina inverse filter for the sweep the same arguments would
/// generate: the sweep time-reversed, carrying the −6 dB/oct amplitude
/// envelope — 0 dB at the f2 end falling to −6·log2(f2/f1) dB at the f1 end.
///
/// The envelope's job: the log sweep's amplitude spectrum is pink
/// (∝ f^−1/2), so the plain reversed sweep would deconvolve to a −6 dB/oct
/// tilt; tilting the reversed copy by +6 dB/oct in time (which is what a
/// per-sample gain of f(t)/f2 does to a chirp, by stationary phase) hands
/// back a flat, bandpassed Dirac. Because the instantaneous frequency is
/// exponential in t, the dB envelope is simply linear in forward time.
pub fn inverse_filter(f1: f64, f2: f64, duration: f64, sample_rate: f64) -> Vec<f32> {
    let x = ess64(f1, f2, duration, sample_rate, SWEEP_FADE_IN_S, SWEEP_FADE_OUT_S);
    let len = x.len();
    if len == 0 {
        return Vec::new();
    }
    let Some((f1c, f2c)) = sane_band(f1, f2, sample_rate) else {
        return Vec::new();
    };
    let span_db = -6.0 * (f2c / f1c).log2();
    let mut inv = vec![0.0f32; len];
    for (i, &v) in x.iter().enumerate() {
        // Forward time t = i/sr sits at −6·log2(f2/f1)·(1 − t/T) dB; the
        // sample lands time-reversed.
        let t_frac = i as f64 / (len - 1) as f64;
        let db = span_db * (1.0 - t_frac);
        inv[len - 1 - i] = (v * 10.0f64.powf(db / 20.0)) as f32;
    }
    inv
}

// ── deconvolution drivers ───────────────────────────────────────────────────

/// Linear (zero-padded) FFT convolution in f64, the driver everything here
/// shares. Full length `a + b − 1`; no wrap, so negative-time content such
/// as harmonic pre-responses keeps its honest place ahead of the peak.
fn fft_convolve64(a: &[f32], b: &[f32]) -> Vec<f64> {
    if a.is_empty() || b.is_empty() {
        return Vec::new();
    }
    let out_len = a.len() + b.len() - 1;
    let n = next_pow2(out_len);
    let mut are = vec![0.0f64; n];
    let mut aim = vec![0.0f64; n];
    let mut bre = vec![0.0f64; n];
    let mut bim = vec![0.0f64; n];
    for (d, &s) in are.iter_mut().zip(a) {
        *d = f64::from(s);
    }
    for (d, &s) in bre.iter_mut().zip(b) {
        *d = f64::from(s);
    }
    fft64(&mut are, &mut aim, false);
    fft64(&mut bre, &mut bim, false);
    for k in 0..n {
        let (r, i) = (
            are[k] * bre[k] - aim[k] * bim[k],
            are[k] * bim[k] + aim[k] * bre[k],
        );
        are[k] = r;
        aim[k] = i;
    }
    fft64(&mut are, &mut aim, true);
    are.truncate(out_len);
    are
}

/// Linear FFT convolution of two signals, f64 inside, f32 out. Public
/// because the wizard host wants it too — synthesizing a capture pass,
/// auditioning an IR against test audio.
pub fn fft_convolve(a: &[f32], b: &[f32]) -> Vec<f32> {
    fft_convolve64(a, b).into_iter().map(|v| v as f32).collect()
}

/// What the classic driver hands back: the full deconvolved sequence and
/// where t = 0 falls in it.
pub struct Deconvolved {
    /// The whole sequence, harmonic pre-responses included: the order-N
    /// image sits `harmonic_advance_s(N, ..)` seconds BEFORE `onset`.
    pub ir: Vec<f32>,
    /// The index where the delayed Dirac of a straight wire would land —
    /// t = 0 of the recovered IR. Everything earlier is pre-response.
    pub onset: usize,
}

/// The classic Farina route: convolve the recording with the inverse
/// filter, then normalize and locate t = 0 by putting the SWEEP ITSELF
/// through the same filter — the sweep's own deconvolution is the delayed
/// Dirac that defines both the scale and the onset, so a wire loop recovers
/// a unit impulse without any magic constants.
///
/// Flatness caveat, out loud: the −6 dB/oct envelope is a stationary-phase
/// approximation, flat to a fraction of a dB in-band, not to −60 dB. Use
/// this route when the pre-response layout matters (the distortion probe);
/// use [`deconvolve_by_reference`] when exactness matters.
pub fn deconvolve_classic(recorded: &[f32], sweep: &[f32], inverse: &[f32]) -> Deconvolved {
    let raw = fft_convolve64(recorded, inverse);
    let reference = fft_convolve64(sweep, inverse);
    let onset = reference
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i)
        .unwrap_or(0);
    let peak = reference.get(onset).copied().unwrap_or(0.0);
    let scale = if peak.abs() > 0.0 { 1.0 / peak } else { 0.0 };
    Deconvolved {
        ir: raw.into_iter().map(|v| (v * scale) as f32).collect(),
        onset,
    }
}

/// The exact route: spectral division of the recording by a recorded
/// reference pass of the same loop, `H = Y·conj(X)/(|X|² + eps(f))`, with
/// Kirkeby-style frequency-dependent regularization — [`KIRKEBY_EPS_IN`]
/// inside [f1..f2], [`KIRKEBY_EPS_OUT`] outside, half-cosine transitions
/// [`KIRKEBY_TRANSITION_OCTAVES`] wide.
///
/// Whatever the loop did to BOTH passes — converter latency, interface
/// ripple, the sweep's own envelope — cancels in the quotient, so the
/// direct impulse lands at index 0 exactly. The division is circular:
/// negative-time content (harmonic pre-responses, regularization ringing)
/// wraps to the END of the returned buffer, which is padded to at least
/// twice the longer input so the wrap never reaches into the IR (Muller's
/// windowing rule); window the tail off, never into the IR.
pub fn deconvolve_by_reference(
    recorded: &[f32],
    reference: &[f32],
    f1: f64,
    f2: f64,
    sample_rate: f64,
) -> Vec<f32> {
    if recorded.is_empty() || reference.is_empty() {
        return Vec::new();
    }
    let Some((f1, f2)) = sane_band(f1, f2, sample_rate) else {
        return Vec::new();
    };
    let n = next_pow2(recorded.len().max(reference.len()) * 2);
    let mut yre = vec![0.0f64; n];
    let mut yim = vec![0.0f64; n];
    let mut xre = vec![0.0f64; n];
    let mut xim = vec![0.0f64; n];
    for (d, &s) in yre.iter_mut().zip(recorded) {
        *d = f64::from(s);
    }
    for (d, &s) in xre.iter_mut().zip(reference) {
        *d = f64::from(s);
    }
    fft64(&mut yre, &mut yim, false);
    fft64(&mut xre, &mut xim, false);

    let pmax = (0..n)
        .map(|k| xre[k] * xre[k] + xim[k] * xim[k])
        .fold(0.0f64, f64::max);
    if pmax <= 0.0 {
        return vec![0.0; n];
    }
    // The eps schedule's band edges, one transition width past f1 and f2.
    let lo_out = f1 * 2.0f64.powf(-KIRKEBY_TRANSITION_OCTAVES);
    let hi_out = f2 * 2.0f64.powf(KIRKEBY_TRANSITION_OCTAVES);
    for k in 0..n {
        // Bin frequency, mirrored: bin n−k carries the same |frequency|.
        let freq = k.min(n - k) as f64 * sample_rate / n as f64;
        // 0 in-band, 1 out, half-cosine across each transition.
        let w = if freq >= f1 && freq <= f2 {
            0.0
        } else if freq > lo_out && freq < f1 {
            0.5 * (1.0 + (std::f64::consts::PI * (freq - lo_out) / (f1 - lo_out)).cos())
        } else if freq > f2 && freq < hi_out {
            0.5 * (1.0 - (std::f64::consts::PI * (freq - f2) / (hi_out - f2)).cos())
        } else {
            1.0
        };
        let eps = pmax * (KIRKEBY_EPS_IN + (KIRKEBY_EPS_OUT - KIRKEBY_EPS_IN) * w);
        let denom = xre[k] * xre[k] + xim[k] * xim[k] + eps;
        let (r, i) = (
            (yre[k] * xre[k] + yim[k] * xim[k]) / denom,
            (yim[k] * xre[k] - yre[k] * xim[k]) / denom,
        );
        yre[k] = r;
        yim[k] = i;
    }
    fft64(&mut yre, &mut yim, true);
    yre.into_iter().map(|v| v as f32).collect()
}

// ── IR post-processing ──────────────────────────────────────────────────────

/// Index of the largest-magnitude sample — the linear IR's peak.
pub fn linear_peak(ir: &[f32]) -> usize {
    ir.iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// How far ahead of the linear response the order-N harmonic image lands,
/// in seconds: `dt_N = T·ln(N)/ln(f2/f1)` (Farina 2000). Constant per order
/// — the orders crowd logarithmically toward the linear peak.
pub fn harmonic_advance_s(order: u32, duration: f64, f1: f64, f2: f64) -> f64 {
    if order < 1 || f2 <= f1 || f1 <= 0.0 {
        return 0.0;
    }
    duration * f64::from(order).ln() / (f2 / f1).ln()
}

/// The free THD probe: energy parked at the order-N pre-response, relative
/// to the linear IR's energy, in dB. Reads a window centred `dt_N` before
/// `onset` (the t = 0 index from [`Deconvolved`]), half as wide as the gap
/// to the nearest neighbouring image so orders never bleed into each
/// other's reading, clamped to [1..20] ms. Returns −200 when the window
/// falls off the buffer.
pub fn harmonic_level_db(
    ir: &[f32],
    onset: usize,
    order: u32,
    duration: f64,
    f1: f64,
    f2: f64,
    sample_rate: f64,
) -> f32 {
    if order < 2 || ir.is_empty() {
        return -200.0;
    }
    let dt = harmonic_advance_s(order, duration, f1, f2);
    let gap_down = dt - harmonic_advance_s(order - 1, duration, f1, f2);
    let gap_up = harmonic_advance_s(order + 1, duration, f1, f2) - dt;
    let hw_s = (0.5 * gap_down.min(gap_up)).clamp(0.001, 0.020);
    let hw = (hw_s * sample_rate).round() as usize;
    let center = onset as i64 - (dt * sample_rate).round() as i64;
    let lo = center - hw as i64;
    let hi = center + hw as i64;
    if lo < 0 || hi as usize >= ir.len() {
        return -200.0;
    }
    let energy = |from: usize, to: usize| -> f64 {
        ir[from..=to.min(ir.len() - 1)]
            .iter()
            .map(|&v| f64::from(v) * f64::from(v))
            .sum()
    };
    let harm = energy(lo as usize, hi as usize);
    let lin = energy(onset.saturating_sub(hw), onset + hw);
    if lin <= 0.0 {
        return -200.0;
    }
    (10.0 * (harm.max(1e-30) / lin).log10()) as f32
}

/// The loop's noise floor from a caller-supplied silence slice: plain RMS
/// in dBFS. The wizard records pre-roll silence anyway; this is what it is
/// for. An empty slice reads −200 (no floor is not a loud floor).
pub fn noise_floor_db(silence: &[f32]) -> f32 {
    if silence.is_empty() {
        return -200.0;
    }
    let ms = silence.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>()
        / silence.len() as f64;
    (10.0 * ms.max(1e-30).log10()) as f32
}

/// **Where a recovered IR stops being signal, in the IR's OWN scale.**
///
/// [`noise_floor_db`] reads the RECORDING, in dBFS at the converter. A
/// deconvolution is a division, and it lands the recovered IR in a
/// completely different scale — nothing about the recording's dBFS survives
/// it. Handing that number straight to [`truncate_at_floor`] compares a
/// level against a yardstick from another measurement, and the resulting cut
/// means nothing.
///
/// It was doing exactly that, and here is the arithmetic that says so.
/// A −3 dBFS capture of a three-second plate through a −96 dBFS loop
/// deconvolves to an IR whose peak is −44 dB in its own units. The threshold
/// applied was −96 + 3 = −93, so the cut landed 49 dB under the IR's peak
/// rather than at its noise — and because the classic route's scale tracks
/// the send level one-for-one, dropping the send to −20 dBFS moved the IR's
/// peak to −61 dB and moved the cut with it, to 0.75 s. The same plate, the
/// same loop, the same noise: half the tail, thrown away by a threshold that
/// had drifted 17 dB relative to the thing it was cutting. That is the
/// short-IR report, and it is a units bug, not a truncator bug.
///
/// The honest translation needs no model of the deconvolution at all. What
/// the capture could resolve is its own signal-to-noise ratio —
/// `capture_peak_db − capture_floor_db` — and that ratio is a RATIO, so it
/// survives any scaling the deconvolution applies. Put the threshold that
/// far under the recovered IR's own peak and the cut sits where the capture
/// stopped knowing anything, whatever scale the IR arrived in.
///
/// **Deliberately conservative.** The sweep's processing gain pushes the
/// recovered noise floor tens of dB BELOW this line (it is why the same
/// −96 dBFS loop leaves a recovered tail still falling at −140 dB), so this
/// keeps less than the measurement strictly supports and never keeps noise.
/// Capped at [`TAIL_RANGE_MAX_DB`] for the reason given there.
///
/// Peaks arrive as linear magnitudes because that is what the callers
/// already have; a peak of zero (silence either side) answers −200, the
/// house reading for "no signal", which no window can fall to and which
/// therefore leaves the IR uncut.
pub fn recovered_floor_db(ir_peak: f32, capture_peak: f32, capture_floor_db: f32) -> f32 {
    if !(ir_peak > 0.0 && capture_peak > 0.0) || !capture_floor_db.is_finite() {
        return -200.0;
    }
    let ir_peak_db = 20.0 * f64::from(ir_peak).log10();
    let capture_peak_db = 20.0 * f64::from(capture_peak).log10();
    let snr = (capture_peak_db - f64::from(capture_floor_db)).clamp(0.0, TAIL_RANGE_MAX_DB);
    (ir_peak_db - snr) as f32
}

/// Truncate the tail where its sliding level ([`TAIL_WINDOW_S`]) sinks to
/// within [`TAIL_MARGIN_DB`] of `floor_db`, then fade the last
/// [`TAIL_FADE_S`] of what remains with a half-cosine.
///
/// **`floor_db` is in `ir`'s OWN scale**, not the recording's. This function
/// compares a level it computes from these very samples against the number
/// it is handed, so the two have to be the same kind of number — pass a
/// recording's dBFS floor and the cut is arbitrary. [`recovered_floor_db`]
/// exists to make that translation and carries the measurements that show
/// what the mistake costs.
///
/// The direct impulse is sacred: the cut is clamped so the fade begins
/// strictly AFTER the peak sample, and when the floor is so high (or the IR
/// so short) that no honest cut exists, the IR is left untouched — a
/// truncation that would eat the impulse is not a truncation.
pub fn truncate_at_floor(ir: &mut Vec<f32>, floor_db: f32, sample_rate: f64) {
    let len = ir.len();
    if len == 0 || !floor_db.is_finite() || sample_rate <= 0.0 {
        return;
    }
    let peak = linear_peak(ir);
    let win = ((TAIL_WINDOW_S * sample_rate).round() as usize).max(1);
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    // Prefix sums of squares, so each window is O(1) to read.
    let mut prefix = vec![0.0f64; len + 1];
    for (i, &v) in ir.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(v) * f64::from(v);
    }
    let threshold_db = f64::from(floor_db) + TAIL_MARGIN_DB;
    // Earliest legal cut: the fade must live entirely after the peak.
    let first = peak + 1 + fade;
    let mut cut = None;
    let mut start = first;
    while start + win <= len {
        let ms = (prefix[start + win] - prefix[start]) / win as f64;
        if 10.0 * ms.max(1e-30).log10() <= threshold_db {
            cut = Some(start);
            break;
        }
        start += 1;
    }
    let Some(cut) = cut else {
        return; // The tail never reaches the floor inside this buffer.
    };
    for i in 0..fade.min(cut) {
        // Half-cosine down over the kept region's last `fade` samples.
        let w = 0.5 * (1.0 + (std::f64::consts::PI * (i + 1) as f64 / fade as f64).cos());
        ir[cut - fade + i] = (f64::from(ir[cut - fade + i]) * w) as f32;
    }
    ir.truncate(cut);
}

/// The foot of the direct impulse: walking left from the peak, the first
/// sample of the contiguous run that stays within [`ONSET_FLOOR_DB`] of it.
/// Deliberately local — harmonic pre-responses further out do not fool it,
/// because the walk stops at the first quiet sample.
pub fn find_onset(ir: &[f32]) -> usize {
    if ir.is_empty() {
        return 0;
    }
    let peak = linear_peak(ir);
    let floor = f64::from(ir[peak].abs()) * 10.0f64.powf(ONSET_FLOOR_DB / 20.0);
    let mut i = peak;
    while i > 0 && f64::from(ir[i - 1].abs()) >= floor {
        i -= 1;
    }
    i
}

/// Drop `direct` whole samples of lead-in so the direct impulse sits at
/// t = 0. Whole samples only — none of the surveyed shipping tools
/// sub-sample-shifts an exported IR, and neither do we.
pub fn trim_leading(ir: &mut Vec<f32>, direct: usize) {
    ir.drain(..direct.min(ir.len()));
}

/// Subtract the mean. A capture chain's converter offset becomes a DC step
/// convolved into everything the IR ever touches; killing it here is one
/// line (Voxengo ships the same filter).
pub fn remove_dc(ir: &mut [f32]) {
    if ir.is_empty() {
        return;
    }
    let mean = (ir.iter().map(|&v| f64::from(v)).sum::<f64>() / ir.len() as f64) as f32;
    for v in ir.iter_mut() {
        *v -= mean;
    }
}

/// Normalize a GROUP of IRs by their common peak, to `target_db` dBFS
/// (see [`NORM_TARGET_DB`]). One scale for the whole set: the level
/// differences between the channels of a true-stereo capture ARE the
/// stereo image, and per-file normalization would erase them (Voxengo's
/// warning, verbatim in spirit). Silence stays silence.
pub fn normalize_set(irs: &mut [Vec<f32>], target_db: f32) {
    let peak = irs
        .iter()
        .flat_map(|ir| ir.iter())
        .fold(0.0f32, |m, &v| m.max(v.abs()));
    if peak <= 0.0 || !target_db.is_finite() {
        return;
    }
    let scale = 10.0f32.powf(target_db / 20.0) / peak;
    for ir in irs.iter_mut() {
        for v in ir.iter_mut() {
            *v *= scale;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic little generator, so tests never pull in a crate.
    fn lcg(seed: &mut u64) -> f32 {
        *seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        ((*seed >> 33) as f32 / (1u64 << 31) as f32) - 1.0
    }

    /// A synthetic IR that is genuinely band-limited AS A LINEAR SIGNAL —
    /// not circularly windowed and truncated, which would leak broadband
    /// energy no band-limited capture could ever return. Gabor wavelets for
    /// the discrete reflections (a Gaussian's spectrum is a Gaussian: gone
    /// to −300 dB at the band edges) and a modal tail of decaying cosines,
    /// each switched on with a smooth 3 ms rise and released before the
    /// buffer's end, so every edge is quiet in both domains.
    fn synthetic_ir(sample_rate: f64) -> Vec<f32> {
        let sr = sample_rate;
        let len = (0.15 * sr) as usize;
        let mut ir = vec![0.0f64; len];
        let gabor = |ir: &mut [f64], t0: f64, hz: f64, sigma: f64, amp: f64| {
            for (i, v) in ir.iter_mut().enumerate() {
                let t = i as f64 / sr - t0;
                if t.abs() < 6.0 * sigma {
                    *v += amp
                        * (-0.5 * (t / sigma).powi(2)).exp()
                        * (std::f64::consts::TAU * hz * t).cos();
                }
            }
        };
        gabor(&mut ir, 0.004, 2_000.0, 0.0007, 1.0);
        gabor(&mut ir, 0.017, 500.0, 0.0016, -0.55);
        gabor(&mut ir, 0.026, 5_000.0, 0.0004, 0.35);
        let mut seed = 0x1234_5678_9abc_def0u64;
        for m in 0..24 {
            let hz = 300.0 * (7_000.0f64 / 300.0).powf(m as f64 / 23.0);
            let tau = 0.012 + 0.012 * f64::from(lcg(&mut seed)).abs();
            let amp = 0.02 * f64::from(lcg(&mut seed));
            let phase = std::f64::consts::PI * f64::from(lcg(&mut seed));
            let onset = 0.003;
            for (i, v) in ir.iter_mut().enumerate() {
                let t = i as f64 / sr - 0.03;
                if t < 0.0 {
                    continue;
                }
                let a = if t < onset {
                    0.5 * (1.0 - (std::f64::consts::PI * t / onset).cos())
                } else {
                    1.0
                };
                *v += amp * a * (-t / tau).exp() * (std::f64::consts::TAU * hz * t + phase).cos();
            }
        }
        let rel = (0.020 * sr) as usize;
        for i in 0..rel {
            let w = 0.5 * (1.0 - (std::f64::consts::PI * i as f64 / rel as f64).cos());
            ir[len - 1 - i] *= w;
        }
        ir.into_iter().map(|v| v as f32).collect()
    }

    fn err_norm_db(got: &[f32], want: &[f32]) -> f32 {
        let mut err = 0.0f64;
        let mut refe = 0.0f64;
        for i in 0..want.len() {
            let g = f64::from(got.get(i).copied().unwrap_or(0.0));
            let w = f64::from(want[i]);
            err += (g - w) * (g - w);
            refe += w * w;
        }
        (10.0 * (err.max(1e-30) / refe.max(1e-30)).log10()) as f32
    }

    /// **A known IR round-trips through the sweep within −60 dB**, at 44.1,
    /// 48 and 96 k — the same math at every rate, which is the offline
    /// half's rate-invariance statement. The sweep is convolved through the
    /// IR, the result deconvolved by reference division against the sweep
    /// itself, and the recovered IR compared to the original.
    #[test]
    fn a_known_ir_round_trips_through_the_sweep_within_minus_sixty_db() {
        for sr in [44_100.0f64, 48_000.0, 96_000.0] {
            let (f1, f2) = (20.0, 0.45 * sr);
            let sweep = ess(f1, f2, 0.5, sr);
            assert!(!sweep.is_empty(), "no sweep at {sr}");
            let ir = synthetic_ir(sr);
            let recorded = fft_convolve(&sweep, &ir);
            let recovered = deconvolve_by_reference(&recorded, &sweep, f1, f2, sr);
            let err = err_norm_db(&recovered[..ir.len()], &ir);
            assert!(
                err < -60.0,
                "round-trip error at {sr} Hz is {err:.1} dB, wanted under -60"
            );
        }
    }

    /// **Distortion packs ahead of the linear response exactly where Farina
    /// says.** A 5% square-law DUT: the recovered linear IR stays a clean
    /// spike while the 2nd-harmonic image appears at dt_2 = T·ln2/ln(f2/f1)
    /// before it, within a millisecond — and the probe reads it.
    #[test]
    fn distortion_packs_ahead_of_the_linear_response_where_farina_says() {
        let sr = 48_000.0f64;
        let (f1, f2, t) = (20.0, 20_000.0, 1.0);
        let sweep = ess(f1, f2, t, sr);
        let inv = inverse_filter(f1, f2, t, sr);

        // The DUT: y = x + 0.05·x².
        let bent: Vec<f32> = sweep.iter().map(|&x| x + 0.05 * x * x).collect();
        let d = deconvolve_classic(&bent, &sweep, &inv);

        // The linear IR survives: unit peak at the onset, within half a dB.
        let peak = linear_peak(&d.ir);
        assert!(
            (peak as i64 - d.onset as i64).abs() <= 1,
            "linear peak wandered from the onset: {peak} vs {}",
            d.onset
        );
        let peak_db = 20.0 * d.ir[peak].abs().log10();
        assert!(
            peak_db.abs() < 0.5,
            "the wire's impulse recovered at {peak_db:.2} dB, wanted 0"
        );

        // The 2nd-harmonic image sits at the predicted advance, within 1 ms.
        let dt2 = harmonic_advance_s(2, t, f1, f2);
        let expect = d.onset as i64 - (dt2 * sr).round() as i64;
        let (lo, hi) = (expect - (0.005 * sr) as i64, expect + (0.005 * sr) as i64);
        let found = (lo..=hi)
            .max_by(|&a, &b| {
                d.ir[a as usize].abs().total_cmp(&d.ir[b as usize].abs())
            })
            .unwrap();
        let off_ms = (found - expect).abs() as f64 / sr * 1000.0;
        assert!(
            off_ms <= 1.0,
            "2nd-harmonic image landed {off_ms:.2} ms from the Farina prediction"
        );

        // The probe sees it, and sees nothing on a clean pass.
        let bent_h2 = harmonic_level_db(&d.ir, d.onset, 2, t, f1, f2, sr);
        let clean = deconvolve_classic(&sweep, &sweep, &inv);
        let clean_h2 = harmonic_level_db(&clean.ir, clean.onset, 2, t, f1, f2, sr);
        assert!(
            bent_h2 > -40.0,
            "5% square law read only {bent_h2:.1} dB of 2nd harmonic"
        );
        // 15 dB of daylight: the clean pass's own floor is the stationary-
        // phase approximation's junk at around −51 dB, and the 5% square law
        // reads near −32 — clearly apart, but not by a full 20.
        assert!(
            bent_h2 > clean_h2 + 15.0,
            "bent {bent_h2:.1} dB vs clean {clean_h2:.1} dB - the probe cannot tell them apart"
        );

        // And the region AFTER the linear impulse stays an error floor: the
        // separation put the distortion before the peak, not into the tail.
        let from = d.onset + (0.002 * sr) as usize;
        let to = (d.onset + (0.050 * sr) as usize).min(d.ir.len() - 1);
        let tail = d.ir[from..to]
            .iter()
            .fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(
            20.0 * tail.log10() < -40.0,
            "the linear tail carries {:.1} dB of junk",
            20.0 * tail.log10()
        );
    }

    /// **Dividing by a recorded reference cancels the loop delay exactly.**
    /// Both passes ride the same 137-sample interface latency; after the
    /// division the wire's impulse sits at t = 0, whole and dominant — and
    /// a DUT that adds its own delay keeps exactly that delay.
    #[test]
    fn dividing_by_a_recorded_reference_cancels_the_loop_delay_exactly() {
        let sr = 48_000.0f64;
        let (f1, f2) = (20.0, 20_000.0);
        let sweep = ess(f1, f2, 0.5, sr);
        let mut reference = vec![0.0f32; 137];
        reference.extend_from_slice(&sweep);

        // The wire: recording == reference.
        let h = deconvolve_by_reference(&reference, &reference, f1, f2, sr);
        assert_eq!(linear_peak(&h), 0, "the loop delay leaked into the IR");
        // A band-limited wire's impulse peaks at the in-band fraction of the
        // spectrum, not at 1.0 — [f1..f2] out of [0..Nyquist] is the energy
        // the division could honestly reconstruct.
        let fraction = ((f2 - f1) / (sr / 2.0)) as f32;
        assert!(
            (h[0] - fraction).abs() < 0.05,
            "the wire's impulse came back at {} against a band fraction of {fraction:.3}",
            h[0]
        );
        // Dominant: nothing past the first millisecond within 30 dB of it.
        let guard = (0.001 * sr) as usize;
        let rest = h[guard..h.len() - guard]
            .iter()
            .fold(0.0f32, |m, &v| m.max(v.abs()));
        assert!(
            rest < h[0].abs() * 10.0f32.powf(-30.0 / 20.0),
            "sidelobes reach {:.1} dB of the impulse",
            20.0 * (rest / h[0].abs()).log10()
        );

        // A DUT delay of 250 samples survives as exactly 250 samples.
        let mut delayed = vec![0.0f32; 250];
        delayed.extend_from_slice(&reference);
        let h = deconvolve_by_reference(&delayed, &reference, f1, f2, sr);
        assert_eq!(linear_peak(&h), 250, "the DUT's own delay was not preserved");
    }

    /// **Truncation and its fade never touch the direct impulse.** A spike
    /// with a decaying tail over a noise floor: after truncation the region
    /// up to and past the peak is bit-identical, the buffer is shorter, and
    /// the new tail end has been faded to the floor's scale.
    #[test]
    fn truncation_and_its_fade_never_touch_the_direct_impulse() {
        let sr = 48_000.0f64;
        let len = (0.5 * sr) as usize;
        let mut seed = 42u64;
        let mut ir = vec![0.0f32; len];
        ir[100] = 1.0;
        for i in 101..len {
            let t = (i - 100) as f64 / sr;
            ir[i] = (f64::from(lcg(&mut seed)) * 0.3 * (-t / 0.02).exp()) as f32
                + lcg(&mut seed) * 1.0e-4;
        }
        let original = ir.clone();
        let floor = noise_floor_db(&original[len - (0.05 * sr) as usize..]);
        truncate_at_floor(&mut ir, floor, sr);
        assert!(
            ir.len() < original.len(),
            "a tail sitting on a -80 dB floor was never cut"
        );
        let keep = 100 + (TAIL_FADE_S * sr) as usize / 2;
        assert_eq!(
            &ir[..keep],
            &original[..keep],
            "truncation reached back and touched the impulse"
        );
        let last = ir[ir.len() - 1].abs();
        assert!(
            last < 10.0f32.powf((floor + 12.0) / 20.0),
            "the cut end was left hanging at {last}"
        );

        // A floor above the whole tail cuts hard right after the impulse -
        // and the impulse still survives, whole.
        let mut short = original.clone();
        truncate_at_floor(&mut short, 0.0, sr);
        assert!(short.len() >= 101, "a high floor ate the direct impulse");
        assert_eq!(short[100], 1.0, "a high floor scaled the direct impulse");
    }

    /// **The truncation threshold follows the IR's own scale, not the
    /// recording's.** This is the arithmetic behind the short-IR report.
    ///
    /// The same tail, in two scales 20 dB apart, is the same tail: the
    /// threshold has to move with it, so that the CUT lands in the same
    /// place. What the wizard was doing instead — handing the recording's
    /// dBFS floor straight to `truncate_at_floor` — is pinned here too, as
    /// the thing that moves when it must not.
    #[test]
    fn the_truncation_threshold_travels_with_the_irs_own_scale() {
        // A capture that peaked at −6 dBFS over a −86 dBFS loop: 80 dB of
        // measurement, whatever scale the deconvolution then lands in.
        let capture_peak = 10.0f32.powf(-6.0 / 20.0);
        let floor_db = -86.0f32;
        let loud = recovered_floor_db(1.0, capture_peak, floor_db);
        let quiet = recovered_floor_db(0.1, capture_peak, floor_db);
        assert!(
            (loud - (-80.0)).abs() < 0.01,
            "80 dB under a unit-peak IR is {loud}, not -80"
        );
        assert!(
            (quiet - loud - (-20.0)).abs() < 0.01,
            "an IR 20 dB quieter got a threshold {} dB away, not 20",
            quiet - loud
        );

        // And the cut it produces is the SAME cut. One decaying tail, built
        // once and scaled by a tenth: both truncate at the same sample.
        let sr = 48_000.0f64;
        let len = (2.0 * sr) as usize;
        let mut seed = 7u64;
        let mut ir = vec![0.0f32; len];
        ir[0] = 1.0;
        for i in 1..len {
            let t = i as f64 / sr;
            ir[i] = (f64::from(lcg(&mut seed)) * 0.5 * (-t / 0.15).exp()) as f32;
        }
        let mut a = ir.clone();
        let mut b: Vec<f32> = ir.iter().map(|v| v * 0.1).collect();
        let a_peak = a.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        let b_peak = b.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
        truncate_at_floor(&mut a, recovered_floor_db(a_peak, capture_peak, floor_db), sr);
        truncate_at_floor(&mut b, recovered_floor_db(b_peak, capture_peak, floor_db), sr);
        assert!(a.len() < len, "the tail was never cut at all");
        assert_eq!(
            a.len(),
            b.len(),
            "the same tail in two scales cut in two different places"
        );

        // The old road, for the record: the RECORDING's floor against both
        // scales cuts them in different places, which is the bug.
        let mut a2 = ir.clone();
        let mut b2: Vec<f32> = ir.iter().map(|v| v * 0.1).collect();
        truncate_at_floor(&mut a2, floor_db, sr);
        truncate_at_floor(&mut b2, floor_db, sr);
        assert_ne!(
            a2.len(),
            b2.len(),
            "the mismatched comparison was supposed to be scale-dependent"
        );

        // A capture with no headroom at all cannot be trusted anywhere, and
        // a silent one answers the house's "no signal" reading rather than
        // a number a threshold could act on.
        assert!(recovered_floor_db(1.0, capture_peak, -6.0) >= -0.01);
        assert!(recovered_floor_db(0.0, capture_peak, floor_db) <= -199.0);
        // Past the cap the threshold stops descending, or no window ever
        // crosses it and the whole capture window ships as an "IR".
        let pristine = recovered_floor_db(1.0, capture_peak, -200.0);
        assert!(
            (pristine + TAIL_RANGE_MAX_DB as f32).abs() < 0.01,
            "a noiseless loop asked for {pristine} dB of range"
        );
    }


    /// **The inverse filter alone pulls the sweep to a bandpassed click**:
    /// convolving the sweep with its classic inverse concentrates the
    /// energy at the onset — the envelope is doing its −6 dB/oct job.
    #[test]
    fn the_inverse_filter_pulls_the_sweep_to_a_bandpassed_click() {
        let sr = 48_000.0f64;
        let (f1, f2, t) = (20.0, 20_000.0, 0.5);
        let sweep = ess(f1, f2, t, sr);
        let inv = inverse_filter(f1, f2, t, sr);
        let d = deconvolve_classic(&sweep, &sweep, &inv);
        // The onset lands where a pair of equal-length sequences says.
        assert!(
            (d.onset as i64 - (sweep.len() as i64 - 1)).abs() < (0.001 * sr) as i64,
            "the delayed Dirac sits at {} of {}",
            d.onset,
            sweep.len()
        );
        let hw = (0.005 * sr) as usize;
        let energy = |from: usize, to: usize| -> f64 {
            d.ir[from..to].iter().map(|&v| f64::from(v) * f64::from(v)).sum()
        };
        let near = energy(d.onset - hw, (d.onset + hw).min(d.ir.len()));
        let total = energy(0, d.ir.len());
        // 95%: the stationary-phase envelope is an approximation and the
        // band edges keep a little spread energy; the exact route is the
        // reference division, tested above.
        assert!(
            near / total > 0.95,
            "only {:.1}% of the energy landed inside +-5 ms of the click",
            100.0 * near / total
        );
    }

    /// **Set normalization preserves relative gain.** Two IRs at 1.0 and
    /// 0.25 peak: after normalizing the SET to −0.2 dB the louder sits at
    /// the target and the ratio between them is exactly what it was.
    #[test]
    fn set_normalization_preserves_relative_gain_between_irs() {
        let mut set = vec![vec![0.0f32, 1.0, -0.5], vec![0.25f32, -0.1, 0.0]];
        normalize_set(&mut set, NORM_TARGET_DB);
        let target = 10.0f32.powf(NORM_TARGET_DB / 20.0);
        assert!((set[0][1] - target).abs() < 1.0e-6, "the set peak missed the target");
        assert!(
            (set[1][0] / set[0][1] - 0.25).abs() < 1.0e-6,
            "the inter-IR ratio was not preserved"
        );
        // DC removal and the noise floor helper, while the file is open.
        let mut dc = vec![0.5f32; 64];
        remove_dc(&mut dc);
        assert!(dc.iter().all(|&v| v.abs() < 1.0e-6), "DC survived remove_dc");
        assert!((noise_floor_db(&[0.01f32; 4800]) - (-40.0)).abs() < 0.1);

        // And the leading-trim: an impulse at 300 lands at 0, whole samples.
        let mut ir = vec![0.0f32; 1000];
        ir[300] = 1.0;
        ir[301] = 0.4;
        let onset = find_onset(&ir);
        assert_eq!(onset, 300);
        trim_leading(&mut ir, onset);
        assert_eq!(linear_peak(&ir), 0);
        assert_eq!(ir.len(), 700);
    }
}
