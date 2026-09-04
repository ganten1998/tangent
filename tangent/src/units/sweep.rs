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

/// How much of a recovered IR's END [`tail_floor_db`] reads to find the IR's
/// own noise floor, in seconds — and how much it reads again earlier, to
/// prove the end IS noise.
///
/// Half a second, twice. Long enough that a reading taken off deconvolved
/// hiss is steady to a couple of tenths of a dB rather than wandering with
/// whichever crest fell inside it. Short enough that both reads, the gap
/// between them ([`TAIL_FLOOR_SPAN_S`]) and the fade all fit inside the
/// shortest window this road can produce: the wizard's TAIL row is 5, 10 or
/// 20 seconds (the literal row in `dialogs::draw_ir_sweep`), and even the 5 s
/// setting clears the ~2.51 s the three of them need by half the buffer. An
/// earlier draft of this paragraph said the row bottomed out at one second
/// and leaned on that setting to justify the number; no such setting exists,
/// which made the guard look far closer to its edge than it is. A window too
/// short
/// for the pair reads no floor at all and the bound the caller was handed
/// stands alone, which is the old rule — a defensive path here rather than
/// the answer for any setting somebody can select.
pub const TAIL_FLOOR_READ_S: f64 = 0.5;

/// How far apart the two [`TAIL_FLOOR_READ_S`] reads sit, in seconds —
/// start to start.
///
/// **The lever arm, and it has to be long.** What the pair is really
/// measuring is a RATE: a decay falls 60/RT60 dB per second and a noise floor
/// falls at nothing, so the two reads differ by `60 · span / RT60` dB and the
/// guard's real threshold is a slope. With the reads touching — half a second
/// apart, which is what this was when it was written — 1.5 dB of allowance is
/// 3 dB per second, and every box with an RT60 of 20 seconds or more reads as
/// stationary. Measured on the road at that separation, a flat −85 dBFS loop
/// and a 10 s tail row: RT60 24 s came back 8.256 s, RT60 30 s 7.852 s, RT60
/// 45 s 7.172 s — a fifth to a third of a perfectly good capture deleted,
/// which is precisely the failure [`TAIL_FLOOR_FLAT_DB`] exists to prevent,
/// with the constant sitting exactly ON the boundary rather than past it.
///
/// The fix is not a tighter allowance: two reads half a second apart are
/// steady to a couple of tenths of a dB, so anything under about a dB is
/// asking them to resolve their own noise. It is a longer arm, which grows
/// the signal while the noise stays where it is. Two seconds puts the same
/// 1.5 dB at 0.75 dB per second, so the guard now believes an end is
/// stationary only below 60/0.75 = 80 seconds of RT60 — past every reverb
/// anybody captures and well past every room.
///
/// The cost is that a buffer still decaying two and a half seconds from its
/// end reads no floor. That is the right answer rather than a regression: a
/// buffer that has not reached its floor has no floor to read, and the whole
/// window is what an honest truncator gives back.
pub const TAIL_FLOOR_SPAN_S: f64 = 2.0;

/// How little two [`TAIL_FLOOR_READ_S`] reads a [`TAIL_FLOOR_SPAN_S`] apart
/// may differ before the end of a buffer counts as NOISE rather than as
/// something still moving, in dB.
///
/// **This is the entire guard on reading a floor off a buffer's own end, and
/// without it the cure is worse than the disease.** A noise floor is
/// stationary: two half-second reads of deconvolved hiss land within a couple
/// of tenths of a dB of each other however far apart they are taken. A decay
/// is not — a box whose RT60 outruns the recorded window is still falling at
/// 60/RT60 dB per second where the reading is taken. Call that a floor and
/// the threshold lands three dB under a level the tail is still comfortably
/// above, and the cut throws away a third of the window of a capture that had
/// nothing whatever wrong with it. What separates the two is a SLOPE, and the
/// slope this pair can resolve is set by how far apart they are; see
/// [`TAIL_FLOOR_SPAN_S`], which is the half of this rule that was wrong.
///
/// **The test is two-sided, and that half is not symmetry for its own sake.**
/// Read as "the end is not still falling" it only ever needed the one
/// direction, and that is how it was written. But what the caller needs to
/// know is that the end is NOISE, and an end LOUDER than the read before it
/// is not noise either — it is a chair, a door, a passing car, the HVAC
/// coming on. The TAIL row runs to twenty seconds and this road is pointed at
/// rooms and halls, so a stray transient in the last half second is ordinary,
/// and a one-sided test accepts it as the floor however far above the real
/// one it sits. Measured on a 3 s decay over a −60 dB floor with a −30 dB
/// disturbance across the last 0.4 s: the floor read −35.5 dB instead of
/// −60.0, the threshold landed 24 dB too high, and the truncation cut at
/// 0.295 s where the clean buffer cut at 0.751 — sixty per cent of a valid
/// measurement, deleted silently, in the destructive direction.
///
/// 1.5 dB over the span sits between the two with room on both sides, and a
/// buffer that fails the test in EITHER direction simply has no floor to
/// read: the caller falls back to the bound it was handed and whatever
/// earlier cut applied still stands.
pub const TAIL_FLOOR_FLAT_DB: f64 = 1.5;

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

/// **The range below which a capture is not a measurement, in dB.**
///
/// The capture's peak minus the loop's own floor is all the dynamic range
/// there is to find a decay in, and the truncator can only follow a tail
/// down as far as that number reaches. Past some point it is no longer
/// cutting the hiss off the end of the decay, it is cutting inside the
/// decay — and the IR that comes back is a fraction of the box with nothing
/// on screen to say so.
///
/// 30 dB, and the bench says why. A 1.5 s plate through the wizard's own
/// settings, with the range walked down by raising a dead return's hiss:
///
///   range 58.8 dB → 0.99 s kept, RT60 1.495 s against the plate's 1.5
///   range 38.8 dB → 0.49 s kept, RT60 1.011 s — short, but still the box
///   range 28.8 dB → 0.24 s kept, RT60 0.363 s — no longer the box at all
///   range 18.8 dB → 0.05 s kept, RT60 0.016 s
///
/// The recovered decay stops being the box's somewhere between 39 and 29 dB
/// of range, so the line is drawn at 30. **This is a line for TELLING the
/// user, not for refusing the capture**: a short honest IR of a quiet box is
/// still worth having, and the wizard prints the peak, the floor and the
/// range beside it. What must not happen is handing one back in silence.
///
/// Below about 22 dB there is not even a decay to cut: see
/// [`floor_crossing`], where a level that never stood above its own floor is
/// refused a vote rather than allowed to end the IR at the earliest legal
/// sample.
pub const TRUSTWORTHY_RANGE_DB: f32 = 30.0;

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

/// **What a recovered IR's TAIL is doing in frequency over time** — the
/// measurement that tells a reverb decaying from a device folding.
///
/// # The fault this exists to catch
///
/// An exponential sweep deconvolves cleanly only against a device that
/// answers it linearly. A device whose own sample rate is BELOW the host's
/// does not: everything the sweep plays above that device's Nyquist comes
/// home as an alias, and an alias climbs DOWN while the excitation climbs
/// up. That is not a linear response to the sweep, so the inverse filter
/// cannot cancel it, and what lands in the recovered IR is a copy of the
/// inverse — a narrowband tone sliding downward, at the inverse's own rate,
/// for as long as the capture window allows.
///
/// The owner met this on an Alesis Quadraverb, a late-eighties box sampling
/// near 31 kHz, with the wizard sweeping to 20.9 kHz. He called it "odd
/// descending chirpy artifacts". Measured on his file: the head fits
/// log-frequency against time at R² 0.124 — modes, jumping about — and the
/// tail at **R² 0.987 across 3.6 octaves, sliding at 0.734 octaves a second
/// against the inverse filter's own 0.736**. Capping the sweep at 14 kHz on
/// the same rig in the same minute took the tail to R² 0.079 and the IR from
/// 5.48 s to 2.85 s.
///
/// # Why the fit, and not the bandwidth
///
/// A real reverb NARROWS as it decays: its top modes die first, so the
/// surviving energy is both quieter and darker, and a bandwidth test alone
/// calls that a chirp. The thing a plate cannot do is SLIDE — its modes are
/// at fixed frequencies. So the discriminator is not how narrow the tail is
/// but how straight it is: a coherent line in log-frequency against time,
/// over octaves, is an excitation that did not cancel.
///
/// `None` when there is not enough tail to say anything, which is the honest
/// answer for a short IR and is never a verdict.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TailChirp {
    /// Octaves per second, signed. Negative is descending, which is the sign
    /// a foldover has.
    pub slope_oct_s: f64,
    /// How well a straight line fits, 0..1. A plate's tail sits near zero.
    pub r2: f64,
    /// Octaves the fit covers. A slide of half an octave is noise finding a
    /// direction; three octaves is not.
    pub span_oct: f64,
}

/// How wide a window the tail fit uses, and how far it steps. 8192 points is
/// 186 ms at 44.1 kHz — long enough to resolve a few hundred hertz, short
/// enough that a slide of three quarters of an octave a second moves less
/// than a fifth of an octave inside one window and is not smeared away.
const CHIRP_WIN: usize = 8192;

/// Fit the tail of `ir` from `from_s` onward. See [`TailChirp`].
pub fn tail_chirp(ir: &[f32], sample_rate: f64, from_s: f64) -> Option<TailChirp> {
    if sample_rate <= 0.0 || !from_s.is_finite() || from_s < 0.0 {
        return None;
    }
    let start = (from_s * sample_rate) as usize;
    let tail = ir.get(start..)?;
    // A window the tail can actually hold, and never bigger than the one the
    // frequency resolution was chosen for.
    let win = CHIRP_WIN.min(next_pow2(tail.len() / 4).max(256));
    if !win.is_power_of_two() || tail.len() < win * 3 {
        return None;
    }
    let hop = win / 2;
    let taper: Vec<f64> = (0..win)
        .map(|i| {
            0.5 - 0.5 * (std::f64::consts::TAU * i as f64 / win as f64).cos()
        })
        .collect();
    // The band a fit is allowed to live in: above the lowest bin a window
    // this long can resolve, below anything a converter is doing on its own.
    let lo_bin = ((100.0 * win as f64 / sample_rate) as usize).max(2);
    let hi_bin = (((18_000.0 * win as f64 / sample_rate) as usize) + 1).min(win / 2);
    if hi_bin <= lo_bin + 1 {
        return None;
    }
    let (mut ts, mut fs) = (Vec::new(), Vec::new());
    let mut at = 0usize;
    while at + win <= tail.len() {
        let mut re = vec![0.0f64; win];
        let mut im = vec![0.0f64; win];
        for i in 0..win {
            re[i] = f64::from(tail[at + i]) * taper[i];
        }
        fft64(&mut re, &mut im, false);
        let mut best = (0usize, 0.0f64);
        for k in lo_bin..hi_bin {
            let p = re[k] * re[k] + im[k] * im[k];
            if p > best.1 {
                best = (k, p);
            }
        }
        if best.1 > 0.0 {
            ts.push((at + win / 2) as f64 / sample_rate);
            fs.push((best.0 as f64 * sample_rate / win as f64).log2());
        }
        at += hop;
    }
    if ts.len() < 4 {
        return None;
    }
    let n = ts.len() as f64;
    let (st, sf) = (ts.iter().sum::<f64>(), fs.iter().sum::<f64>());
    let stt = ts.iter().map(|v| v * v).sum::<f64>();
    let stf = ts.iter().zip(&fs).map(|(a, b)| a * b).sum::<f64>();
    let denom = n * stt - st * st;
    if denom.abs() < 1.0e-12 {
        return None;
    }
    let slope = (n * stf - st * sf) / denom;
    let icept = (sf - slope * st) / n;
    let ss_res: f64 = ts
        .iter()
        .zip(&fs)
        .map(|(a, b)| (b - (slope * a + icept)).powi(2))
        .sum();
    let mean = sf / n;
    let ss_tot: f64 = fs.iter().map(|v| (v - mean).powi(2)).sum();
    let r2 = if ss_tot > 1.0e-12 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };
    let (lo, hi) = fs
        .iter()
        .fold((f64::MAX, f64::MIN), |(a, b), &v| (a.min(v), b.max(v)));
    Some(TailChirp {
        slope_oct_s: slope,
        r2,
        span_oct: hi - lo,
    })
}

/// **Does this tail look like the excitation coming back uncancelled?**
///
/// Three conditions, all of them, and the reason for each is on
/// [`TailChirp`]: the line has to FIT (a plate's does not), it has to cover
/// real ground (half an octave of wandering is not a slide), and it has to
/// descend at the rate the INVERSE FILTER descends at — which is the sweep's
/// own rate, since the inverse is the sweep reversed.
///
/// The rate tolerance is generous on purpose. What is being distinguished is
/// a coherent slide from no slide at all, and the peak track is a coarse
/// instrument: on the owner's file the same tail fitted −0.734 by this route
/// and −0.710 by a Hilbert phase derivative. Being within a quarter of the
/// expected rate is a long way from anything a plate produces.
pub fn tail_is_foldover(fit: TailChirp, f1: f64, f2: f64, duration: f64) -> bool {
    if !(f1 > 0.0 && f2 > f1 && duration > 0.0) {
        return false;
    }
    let want = -((f2 / f1).log2() / duration);
    fit.r2 >= FOLDOVER_R2
        && fit.span_oct >= FOLDOVER_SPAN_OCT
        && (fit.slope_oct_s - want).abs() <= want.abs() * FOLDOVER_RATE_TOLERANCE
}

/// How straight, how far, and how close to the inverse's own rate a tail has
/// to be before the wizard will call it a foldover.
///
/// Measured against both sides of the case that produced them. His folded
/// capture fits at R² 0.987 over 3.6 octaves; the same rig with the sweep
/// capped fits at 0.079 over less than one. A synthetic plate fits at 0.124.
/// 0.85 sits in the wide gap between those, and nothing in the material
/// argues for a tighter number.
const FOLDOVER_R2: f64 = 0.85;
const FOLDOVER_SPAN_OCT: f64 = 1.5;
const FOLDOVER_RATE_TOLERANCE: f64 = 0.25;

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

/// **The deepest a truncation threshold may ever sit, in the IR's OWN
/// scale.**
///
/// **A BOUND, not the answer — read [`tail_floor_db`] first.** This is a
/// figure derived from the CAPTURE, and the buffer it ends up judging is the
/// deconvolution's, whose noise floor is its own affair. Where the two
/// disagree the truncator takes the higher of them, because a threshold
/// below the recovered IR's own noise is a threshold no window ever reaches
/// and the IR then comes back the length of the whole recorded tail. What
/// this number is still good for is the other side of that: the capture
/// could not resolve anything below its own signal-to-noise ratio, so a
/// measured floor further down than this line is measuring the sweep's
/// processing gain rather than the box, and cutting there keeps seconds of
/// detail nobody can hear.
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

/// **The recovered IR's OWN noise floor, read off the end of the buffer
/// itself** — or `None` when the buffer has not reached one.
///
/// [`recovered_floor_db`] answers the question "how far down could the
/// capture hear", which is a fact about the RECORDING. This answers "how far
/// down does this buffer actually go", which is a fact about the buffer, and
/// the two are not the same number: a deconvolution redistributes the loop's
/// noise across its whole output, and adds its own — the division's
/// regularization ringing, the classic route's linear-convolution overhang,
/// and whatever a channel with no reference behind it turned into. The
/// truncator needs the second number, because the level it compares against
/// the threshold is computed from these very samples.
///
/// **The trap this exists to close, with its sign, because the sign is what
/// hid it.** The capture-derived threshold is `ir_peak_db − snr`: the
/// CLEANER the capture, the LOWER the threshold, and the lower the threshold
/// the harder it is for any window to reach. When the recovered IR's own
/// floor sits above that line, no window ever crosses it,
/// [`truncate_at_floor`] takes its "the tail never reaches the floor inside
/// this buffer" path, and the IR comes back the full length of the recorded
/// tail — 10.000 s for a 10 s TAIL setting, every time, whatever box was in
/// the loop. Measured on the reference road with one reference channel
/// carrying no sweep: the dead side's division explodes, the SET's peak
/// becomes that garbage, `ir_peak_db` goes with it, and the threshold lands
/// about a hundred dB under the side that actually holds the measurement.
///
/// The reading is taken from the last [`TAIL_FLOOR_READ_S`] before the
/// buffer's own fade — a fade is not a floor, and reading through one drags
/// the answer down by however much of it was included — and it is only
/// believed when a second read a [`TAIL_FLOOR_SPAN_S`] earlier lands within
/// [`TAIL_FLOOR_FLAT_DB`] of it, in EITHER direction. See those two
/// constants for what each half is holding back: a box whose decay outruns
/// the recorded window is still falling where the reading is taken, and a
/// room capture with a door closing in its last half second is louder there
/// than it was before. Calling either one a floor cuts a large part of a
/// perfectly good capture off the end.
///
/// `first` is the caller's earliest legal cut — the same index its search
/// starts at — so a buffer with no room for both reads after the direct
/// impulse answers `None` rather than reading a "floor" off the impulse's
/// own ringing.
pub fn tail_floor_db(ir: &[f32], first: usize, sample_rate: f64) -> Option<f32> {
    if sample_rate <= 0.0 {
        return None;
    }
    let read = ((TAIL_FLOOR_READ_S * sample_rate).round() as usize).max(1);
    let span = ((TAIL_FLOOR_SPAN_S * sample_rate).round() as usize).max(1);
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    // Back off the buffer's own fade first: it is a ramp to nothing, and
    // averaging it in reads as a floor several dB below the real one.
    let end = ir.len().checked_sub(fade)?;
    let late_at = end.checked_sub(read)?;
    // The earlier read is a SPAN back from the later one, not touching it.
    // See `TAIL_FLOOR_SPAN_S`: what the pair measures is a slope, and a slope
    // read over half a second cannot be told from the reads' own noise.
    let start = late_at.checked_sub(span)?;
    if start < first {
        return None;
    }
    let late = noise_floor_db(&ir[late_at..end]);
    let early = noise_floor_db(&ir[start..start + read]);
    // Still falling means the buffer never got to its floor and the level out
    // here is tail rather than noise; RISING means something happened in the
    // room while the tail was being recorded, and that is not a floor either.
    if f64::from(early - late).abs() > TAIL_FLOOR_FLAT_DB {
        return None;
    }
    Some(late)
}

/// **How far a recorded reference pass's SWEEP stands above its own pre-roll
/// silence, in dB: the measurement that says whether this jack carried the
/// loop back at all.**
///
/// [`deconvolve_by_reference`] divides by whatever it is handed, and a
/// reference channel with no sweep in it is a divisor of noise. The two ways
/// that ends are both wrong and neither announces itself:
///
/// * **A silent channel divides to silence.** An input the interface hands
///   back as true zeros makes the reference spectrum's power peak zero, the
///   division bails, and that side of the IR is a buffer of zeros — a stereo
///   file with one channel deleted, which is what "the capture is all in the
///   left" sounds like.
/// * **A hissing channel divides to garbage, and takes the good side with
///   it.** Bin by bin the quotient is a real return over a noise floor, so
///   the recovered "IR" stands about a hundred dB above the side that holds
///   the measurement — +99.97 dB, measured on the fixture in
///   `desktop::tests::a_true_stereo_capture_survives_a_reference_jack_that_brought_nothing_back`
///   — and since
///   [`normalize_set`] scales the pair by ONE peak so the stereo image
///   survives, the good channel is normalized down into inaudibility. It
///   poisons the truncation as well: every threshold on this road is
///   measured under the SET's peak, and the set's peak is now the garbage.
///
/// Both are one fault with one cause, and the cause is not the division: it
/// is that nobody asked whether the reference was a reference. The engine
/// plays the SAME sample on both output jacks — the loop is one mono probe
/// into a stereo return — so when only one jack comes back, that one is a
/// perfectly good reference for both channels, and the caller borrows it.
///
/// The reading is deliberately WITHIN one channel: the sweep's own level
/// against that same channel's pre-roll silence. A comparison ACROSS the
/// channels would refuse an honestly trimmed loop, and a channel's absolute
/// level says nothing at all — what says the jack carried the sweep is that
/// the jack got loud when the sweep played and was quiet before it. The
/// pre-roll read stops four fifths of the way in for the same reason
/// `process_ir_capture`'s floor read does: the last stretch of it may
/// already carry the sweep's onset through the loop's latency. A channel of
/// digital silence reads 0 dB, because two nothings are the same nothing.
pub fn reference_range_db(channel: &[f32], pre: usize, sweep_len: usize) -> f32 {
    let quiet = (pre * 4) / 5;
    let loud_end = (pre + sweep_len).min(channel.len());
    if quiet == 0 || loud_end <= pre {
        return 0.0;
    }
    noise_floor_db(&channel[pre..loud_end]) - noise_floor_db(&channel[..quiet])
}

/// **The one place an IR is shortened: cut at `cut`, and fade the last
/// [`TAIL_FADE_S`] of what is kept with a half-cosine.**
///
/// Every truncation on the capture road goes through here, and that is the
/// point of the function existing at all. There are two cuts — the level
/// one below, and the capture road's hard window at the end of the recorded
/// silence — and when the fade lived inside [`truncate_at_floor`], only the
/// level cut got it: the window cut, which is the BINDING one on any clean
/// capture, ended the IR at full tail level. A convolver handed a tail that
/// stops mid-decay rings the discontinuity into everything it touches, and
/// at default settings that was the ordinary outcome, not an edge case.
///
/// **The direct impulse is sacred.** The fade has to live entirely after the
/// peak sample, so a cut earlier than `peak + 1 + fade` is not a short IR,
/// it is a deleted impulse — and this refuses it and leaves the buffer
/// alone. That refusal was already [`truncate_at_floor`]'s rule (it simply
/// never searched below that index); it is stated here so the window cut
/// inherits it rather than having to remember it. A cut at or past the end
/// is likewise nothing to do.
///
/// The clamp reads THIS buffer's peak, which is the right reading for one
/// channel standing alone. A stereo pair is not that: see
/// [`cut_set_with_fade`], which is what the capture road uses and why.
pub fn cut_with_fade(ir: &mut Vec<f32>, cut: usize, sample_rate: f64) {
    if sample_rate <= 0.0 {
        return;
    }
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    fade_and_cut(ir, cut, linear_peak(ir) + 1 + fade, fade);
}

/// **[`cut_with_fade`] for a whole SET: one cut index, one clamp, read off
/// the set's own peak and applied to every channel.**
///
/// The pair is ONE measurement, so it gets ONE cut — the rule [`set_onset`]
/// and [`normalize_set`] already follow, stated here for the truncations
/// too. Cutting the channels separately breaks it two ways, and both were
/// measured on a mono box patched into one side of the loop.
///
/// The clamp is one of them. `cut_with_fade` refuses any cut earlier than
/// its OWN buffer's `peak + 1 + fade`, and a channel carrying nothing but
/// converter hiss has its "peak" wherever the noise happened to crest —
/// a number with nothing whatever to do with the measurement, deciding how
/// early that channel may be cut. Reading the clamp off the SET's peak —
/// the direct impulse, which is what the clamp was always about — takes the
/// decision away from the noise.
///
/// **A WITHDRAWN CLAIM, kept here because it was in the tree as measured
/// and is not.** The commit that made this change said the dead channel's
/// crest landed PAST the window cut, so the cut was refused outright and
/// that channel kept the entire deconvolution buffer un-faded, ending on a
/// hard step at full noise level. It does not reproduce. Twelve noise seeds
/// on the reference route put the dead channel's crest between sample
/// 17,434 and 178,100 of a 768,000-sample buffer, and the window cut sits at
/// 480,000: nothing was ever refused, and the channel ended faded every
/// time. The reason is structural rather than lucky — the division's
/// regularization concentrates a noise channel's largest excursions near the
/// front of the buffer, so the crest does not roam the way a uniform draw
/// would. The hazard the clamp change removes is real and is the paragraph
/// above; the hard step was never observed and no longer claimed.
///
/// And the LENGTH. With the channels cut apart, the capture road's
/// `resize` to the longer of them pads the good channel out to match the
/// bad one: the same mono box, reference route, came back 480,000 samples
/// (the full 10 s window) where the identical capture with a live right
/// return gave 71,690 (1.494 s) — a 6.7× IR of a 1.5 s plate, and the
/// convolver pays for every partition of it every block, forever. One cut
/// makes the lengths equal by construction.
pub fn cut_set_with_fade(irs: &mut [Vec<f32>], cut: usize, sample_rate: f64) {
    if sample_rate <= 0.0 {
        return;
    }
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    let earliest = set_peak(irs).0 + 1 + fade;
    for ir in irs.iter_mut() {
        fade_and_cut(ir, cut, earliest, fade);
    }
}

/// The cut itself, with the earliest legal index and the fade length handed
/// in — shared so the single-channel and set-wide spellings above differ
/// only in where the clamp comes from, which is the whole of the argument
/// between them.
fn fade_and_cut(ir: &mut Vec<f32>, cut: usize, earliest: usize, fade: usize) {
    if cut >= ir.len() || cut < earliest {
        return;
    }
    ir.truncate(cut);
    fade_tail(ir, fade);
}

/// The half-cosine down over a buffer's last `fade` samples, in place.
///
/// Split out of [`fade_and_cut`] because a buffer that is never cut still
/// has to end somewhere: [`truncate_at_floor`] and [`truncate_set_at_floor`]
/// fade here on the path where they find no crossing, so "the IR ends on a
/// ramp, not on a step" is a law of the road rather than a property of
/// having been shortened.
///
/// **The caller owns the clamp.** This scales the last `fade` samples of
/// whatever it is handed and clamps only to the buffer's length, so a caller
/// that hands in more fade than there is tail after the direct impulse gets
/// the impulse scaled — and the direct impulse is sacred everywhere else on
/// this road ([`fade_and_cut`] refuses a cut earlier than `earliest` for
/// exactly that reason). Both no-crossing arms therefore pass
/// `fade.min(room)`, where `room` is the distance from their own earliest
/// legal cut to the end. Probed before that clamp existed: a one-sample IR
/// came back `[0.0]` — the whole measurement zeroed — and a 200-sample IR
/// peaking 1.0 at index 150 came back peaking 0.141.
///
/// **A second application squares the ramp, and on the wizard's road that
/// happens.** `desktop::process_ir_capture` runs `cut_set_with_fade` for the
/// post-sweep window first, which always bites and always fades; when the
/// floor pass then finds no crossing it fades the same last samples again,
/// so those carry `w²` rather than `w`. Measured on a flat buffer of 9600
/// ones cut to 4800: the mid-fade sample reads 0.4967 after the window cut
/// and 0.2467 after the second pass — exactly `w²`. It is left alone
/// deliberately. The shape is still monotone to zero and still continuous
/// where the fade begins, so nothing rings; the alternative is a caller
/// telling this function whether somebody already faded, which is a fact no
/// buffer carries and a flag two roads would have to keep in step.
fn fade_tail(ir: &mut [f32], fade: usize) {
    let n = ir.len();
    let fade = fade.min(n);
    for i in 0..fade {
        let w = 0.5 * (1.0 + (std::f64::consts::PI * (i + 1) as f64 / fade as f64).cos());
        ir[n - fade + i] = (f64::from(ir[n - fade + i]) * w) as f32;
    }
}

/// **The level a tail has to sink to before it is cut: the higher of what
/// the buffer's own end says and the deepest the capture could resolve, plus
/// [`TAIL_MARGIN_DB`].**
///
/// The higher, and it matters which way round. [`tail_floor_db`] is a fact
/// about these samples and [`recovered_floor_db`] is a fact about the
/// recording; taking the lower of them would put the threshold under the
/// buffer's own noise, where no window ever reaches it and the IR comes back
/// the full length of the recorded tail. Taking the higher keeps whichever
/// of the two rules bites first — the measurement stops where the capture
/// stopped hearing, OR where the deconvolution's own noise begins, whichever
/// comes up first out of the decay.
///
/// **And the bound has to earn its vote too.** It is a level in the SET's
/// scale, derived under the SET's peak, and it says something about THIS
/// channel only while this channel ever stood above it. When a channel's own
/// peak is below the bound, nothing in it was ever above the line, so the
/// line is not this channel's floor — it is a reading off some other
/// channel's scale, and taking the higher of the two would let that other
/// channel decide that this one has no measurement in it. That is not a
/// hypothetical: a reference channel with no sweep in it divides to a buffer
/// a hundred dB up (see [`reference_range_db`]), and it takes the set's peak
/// and the bound with it.
///
/// A buffer whose end is still falling has no floor to read, and a bound its
/// own peak cannot reach has no vote; with neither, this is exactly the old
/// rule and the number the caller handed in stands alone.
fn tail_threshold_db(ir: &[f32], first: usize, floor_db: f32, sample_rate: f64) -> f64 {
    let own = tail_floor_db(ir, first, sample_rate).map(f64::from);
    let peak = ir.iter().fold(0.0f32, |m, &v| m.max(v.abs()));
    // −200 is the house reading for "no signal", the same one
    // `noise_floor_db` and `recovered_floor_db` answer with.
    let peak_db = if peak > 0.0 {
        20.0 * f64::from(peak).log10()
    } else {
        -200.0
    };
    let bound = (f64::from(floor_db) < peak_db).then(|| f64::from(floor_db));
    let floor = match (own, bound) {
        (Some(own), Some(bound)) => own.max(bound),
        (Some(own), None) => own,
        (None, Some(bound)) => bound,
        (None, None) => f64::from(floor_db),
    };
    floor + TAIL_MARGIN_DB
}

/// Truncate the tail where its sliding level ([`TAIL_WINDOW_S`]) sinks to
/// within [`TAIL_MARGIN_DB`] of `floor_db`, then fade the last
/// [`TAIL_FADE_S`] of what remains with a half-cosine.
///
/// **`floor_db` is in `ir`'s OWN scale**, not the recording's, and it is a
/// BOUND rather than the threshold. This function compares a level it
/// computes from these very samples against the number it acts on, so the
/// two have to be the same kind of number — pass a recording's dBFS floor
/// and the cut is arbitrary. [`recovered_floor_db`] exists to make that
/// translation and carries the measurements that show what the mistake
/// costs; [`tail_threshold_db`] then weighs it against what this buffer's
/// own end says, because a bound derived from the capture can sit below the
/// deconvolution's own noise and then nothing is ever cut at all.
///
/// The direct impulse is sacred: the search starts at the earliest index
/// whose fade would still fall entirely after the peak, and the cut itself
/// is made by [`cut_with_fade`], which refuses anything earlier. When the
/// floor is so high (or the IR so short) that no honest cut exists, the IR
/// keeps its length — but it still gets the half-cosine, because an IR that
/// ends on a step is a step the convolver rings into everything it touches
/// and there is no path on this road where that is acceptable.
pub fn truncate_at_floor(ir: &mut Vec<f32>, floor_db: f32, sample_rate: f64) -> bool {
    if ir.is_empty() || !floor_db.is_finite() || sample_rate <= 0.0 {
        return false;
    }
    let win = ((TAIL_WINDOW_S * sample_rate).round() as usize).max(1);
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    // Earliest legal cut: the fade must live entirely after the peak.
    let first = linear_peak(ir) + 1 + fade;
    let threshold_db = tail_threshold_db(ir, first, floor_db, sample_rate);
    match floor_crossing(ir, first, win, threshold_db) {
        Some(cut) => {
            cut_with_fade(ir, cut, sample_rate);
            true
        }
        // The fade may not reach back past the earliest legal cut — see
        // `fade_tail`. `first` is that index, so what is left after it is all
        // the room a fade is allowed.
        None => {
            let room = ir.len().saturating_sub(first);
            fade_tail(ir, fade.min(room));
            false
        }
    }
}

/// **[`truncate_at_floor`] for a whole SET: one cut, at the LATEST index
/// any channel that actually reaches the floor asks for.**
///
/// The pair is one measurement and gets one cut, made by
/// [`cut_set_with_fade`], which carries what truncating the channels apart
/// cost. This is the other half: which channel decides where.
///
/// **A channel that never sinks to the floor has no measurement in it, and
/// does not get to say when the measurement ends.** That sounds like a
/// technicality and it is the whole bug. A return carrying nothing but
/// converter hiss deconvolves to a level that simply sits there — stationary
/// noise does not decay — so its sliding window never crosses the threshold
/// at any length, and it kept the full window while the good channel was
/// cut at 1.56 s. Reading the level off the set's envelope instead of per
/// channel does NOT fix that, and the bench says why: on the reference
/// route the division hands a noise channel back at roughly the capture's
/// own signal-to-noise ratio under the peak, with none of the matched
/// filter's processing gain, which lands it right AT the threshold rather
/// than the tens of dB below it that [`recovered_floor_db`] assumes. So it
/// holds any envelope open forever. Ignoring the channels that never cross,
/// and taking the LATEST of the ones that do, keeps a genuinely longer tail
/// on either side of a true-stereo pair and gives a dead return no vote at
/// all.
///
/// **And the DUAL of that rule, which is where this shipped a stub.** A
/// channel that never sinks to the floor has no measurement in it; so does a
/// channel that was never ABOVE the floor to begin with, and that one used
/// to answer with the very first index the search looked at. Since the
/// search starts at `peak + 1 + fade`, which is also the earliest cut
/// [`cut_set_with_fade`] will accept, the answer was a cut nothing could
/// refuse and the whole pair came back 10 ms long — the impulse, its fade,
/// and none of the box. [`floor_crossing`] carries the measurement and the
/// fix: a crossing has to cross.
///
/// **The THRESHOLD, though, is each channel's own** — one cut, but not one
/// number to find it with, and this is the half that shipped broken.
///
/// `floor_db` is a bound derived from the capture and applied under the
/// SET's peak, which is fine right up until one channel is not a
/// measurement. A reference channel with no sweep in it divides to garbage a
/// hundred dB above the side that holds the box (see
/// [`reference_range_db`]); the set's peak becomes that garbage, `floor_db`
/// is measured under it, and the real channel's entire decay then sits below
/// a threshold it is supposed to sink THROUGH. Nothing votes, nothing is
/// cut, and a 10 s TAIL setting hands back a 10.000 s "IR" — measured, on
/// exactly that fixture. Reading each channel's floor off its own end
/// ([`tail_floor_db`], weighed against the bound by [`tail_threshold_db`])
/// takes the decision away from the other channel's scale, and it makes the
/// rule above self-enforcing rather than lucky: a side of stationary hiss
/// measures its own level as its own floor, so it never stands above its own
/// threshold, so it never votes.
///
/// **Answers whether it CUT**, and that answer is not a nicety: the caller's
/// other cut is the recorded window, so a set this declines to shorten comes
/// back exactly as long as the TAIL row said. Ten captures of a plate that
/// all measure 10.000 s look like a working tool until one is loaded, and the
/// wizard could not tell the user which of its two cuts it had made. See
/// `ports::IrLengthCut`.
///
/// When no channel reaches its floor the set keeps its length: whatever
/// bound the caller applied before this one still stands. It does not keep
/// its ENDING, though — every channel gets the half-cosine here too, so the
/// path that declines to cut leaves an IR that ends on a ramp like every
/// other path. That is a law about IRs, not about truncations.
/// [`TRUSTWORTHY_RANGE_DB`] is the level at which the app says a capture is
/// poor out loud rather than handing back a long IR in silence.
pub fn truncate_set_at_floor(irs: &mut [Vec<f32>], floor_db: f32, sample_rate: f64) -> bool {
    if irs.is_empty() || !floor_db.is_finite() || sample_rate <= 0.0 {
        return false;
    }
    let win = ((TAIL_WINDOW_S * sample_rate).round() as usize).max(1);
    let fade = ((TAIL_FADE_S * sample_rate).round() as usize).max(1);
    // The clamp is the SET's, so every channel searches from the same index
    // and the direct impulse is sacred for the pair rather than per channel.
    let first = set_peak(irs).0 + 1 + fade;
    let cut = irs
        .iter()
        .filter_map(|ir| {
            let threshold_db = tail_threshold_db(ir, first, floor_db, sample_rate);
            floor_crossing(ir, first, win, threshold_db)
        })
        .max();
    match cut {
        Some(cut) => {
            cut_set_with_fade(irs, cut, sample_rate);
            true
        }
        None => {
            for ir in irs.iter_mut() {
                // The SET's earliest legal cut, so a short channel's fade
                // cannot reach the pair's direct impulse. See `fade_tail`.
                let room = ir.len().saturating_sub(first);
                fade_tail(ir, fade.min(room));
            }
            false
        }
    }
}

/// The first index at or after `first` whose `win`-long window reads at or
/// below `threshold_db`, or `None` when the level never gets there inside
/// this buffer. Prefix sums of squares, so each window is O(1) to read.
///
/// **A crossing has to be a CROSSING.** The level must stand ABOVE the
/// threshold where the search begins; a buffer that is already at or under
/// its own floor at the first window never rose above it, and the index the
/// search happened to start at is not "where the tail ended" — there is no
/// tail. That distinction is the whole of this function's contract and it
/// was missing, at a cost the bench put a number on.
///
/// The callers hand `first` in as `peak + 1 + fade`, which is also the
/// earliest cut [`cut_with_fade`] and [`cut_set_with_fade`] will accept — so
/// a crossing returned AT `first` is a cut those functions cannot refuse,
/// and it deletes everything except the direct impulse and the fade that
/// follows it. Measured on the road, at 48 kHz: a capture running 21.4 dB
/// above the loop's floor came back **481 samples — 10.0 ms — of a 3 s
/// window**, on both deconvolution routes, with the wizard reporting it as a
/// successful capture. A quiet outboard unit is not an edge case; it is what
/// half the reason to own this wizard sounds like.
///
/// **The clamp is not the lever, and this is why it is fixed here.** Raising
/// [`cut_set_with_fade`]'s `earliest` above `first` would make the same cut
/// get refused instead of made, which stops the destruction — but it stops
/// it by accident, at whatever gap the two numbers happen to differ by, and
/// it would refuse honest short cuts with it. The fault is that the search
/// answered at all. A pure delay captured at full range still crosses
/// properly and is still cut at 1,357 samples, because its recovered ringing
/// genuinely stands above the threshold at `first` and genuinely sinks below
/// it later; it is only the capture with no measurement in it that now
/// declines to vote.
///
/// When nothing crosses, the caller leaves the buffer alone and whatever
/// bound it applied before this one — on the capture road, the post-sweep
/// window — still stands. A long IR of an untrustworthy capture is a poor
/// result the user can see and re-take; a 10 ms one looks like a good result
/// and is a deleted measurement. See [`TRUSTWORTHY_RANGE_DB`] for the level
/// the app says this out loud at.
fn floor_crossing(level: &[f32], first: usize, win: usize, threshold_db: f64) -> Option<usize> {
    let len = level.len();
    let mut prefix = vec![0.0f64; len + 1];
    for (i, &v) in level.iter().enumerate() {
        prefix[i + 1] = prefix[i] + f64::from(v) * f64::from(v);
    }
    let window_db = |start: usize| {
        let ms = (prefix[start + win] - prefix[start]) / win as f64;
        10.0 * ms.max(1e-30).log10()
    };
    // Nothing to measure, and nothing above the floor to measure it from.
    if first + win > len || window_db(first) <= threshold_db {
        return None;
    }
    let mut start = first + 1;
    while start + win <= len {
        if window_db(start) <= threshold_db {
            return Some(start);
        }
        start += 1;
    }
    None
}

/// The set's envelope: the largest magnitude any channel carries at each
/// sample, over the longest channel's length. The pair read as ONE signal,
/// which is what [`set_onset`] needs — a lead-in that ends when nothing at
/// all is still arriving, rather than when each channel separately says so.
fn set_envelope<S: AsRef<[f32]>>(channels: &[S]) -> Vec<f32> {
    let len = channels.iter().map(|c| c.as_ref().len()).max().unwrap_or(0);
    let mut env = vec![0.0f32; len];
    for c in channels {
        for (d, &s) in env.iter_mut().zip(c.as_ref()) {
            *d = d.max(s.abs());
        }
    }
    env
}

/// Where the loudest sample in the whole set is, and how loud: `(index,
/// magnitude)`, `(0, 0.0)` for a set with nothing in it.
fn set_peak<S: AsRef<[f32]>>(channels: &[S]) -> (usize, f32) {
    let mut at = 0usize;
    let mut best = 0.0f32;
    for c in channels {
        for (i, &v) in c.as_ref().iter().enumerate() {
            if v.abs() > best {
                best = v.abs();
                at = i;
            }
        }
    }
    (at, best)
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

/// **The foot of the direct impulse for a whole SET of channels**, so a
/// stereo pair can be trimmed by ONE number and stay sample-locked (the IRU
/// rule: an edit applies across all tracks, or the image moves).
///
/// The obvious spelling is the earliest [`find_onset`] over the channels,
/// and it is wrong. `find_onset` walks left from ITS OWN channel's peak, so
/// a channel with no signal in it — a mono box patched into one side of the
/// loop, a dead cable, a return that never came back — has its "peak"
/// somewhere in the noise, and answers with an index that has nothing to do
/// with the impulse. A silent channel answers 0. Take the minimum and that
/// channel decides the trim for the pair: the lead-in stays in, the direct
/// arrival sits deep inside the IR instead of at t = 0, and every later
/// bound measured from the front — the capture window, the tail — is
/// measured from the wrong place.
///
/// **Letting only the channels that CARRY the capture vote does not fix it,
/// because whether a noise channel clears the line is a question about how
/// LONG the buffer is.** That was the first repair — a channel votes if any
/// sample of it comes within [`ONSET_FLOOR_DB`] of the set's peak — and the
/// bench measured what it is worth. A right return carrying nothing but
/// −40 dBFS converter hiss deconvolves to a peak −39.1 dB under the left
/// channel's impulse, which is INSIDE the −40 dB line, while its RMS sits
/// −55.1 dB under it: the peak of a two-million-sample noise buffer stands
/// about 16 dB above its own RMS, and it climbs with the buffer, so the
/// longer the sweep the more certainly hiss buys a vote. It won the
/// minimum, and the trim landed 455,466 samples — 9.5 seconds — early. The
/// direct impulse came back 67,517 samples into the shipped IR instead of
/// at its front: not a short IR, a destroyed one, and a threshold test on a
/// single sample of a multi-million-sample buffer is a coin flip that noise
/// eventually wins.
///
/// So the onset is read off the set's ENVELOPE, the largest magnitude any
/// channel carries at each sample, walked by the same [`find_onset`]. The
/// walk starts at the SET's peak — the direct impulse, wherever it lives —
/// and stops at the first sample where NO channel is still within
/// [`ONSET_FLOOR_DB`] of it. A lucky noise sample four hundred thousand
/// samples away is not on the path and cannot be reached; only a contiguous
/// run above the line holds the walk open, and stationary hiss 55 dB down
/// does not make one. A true-stereo pair whose arrivals are a few samples
/// apart still keeps the earlier one, because the earlier arrival lifts the
/// envelope and the walk simply continues through it — which is the case
/// this rule exists to protect, and the reason the answer is not just
/// `find_onset` of the loudest channel.
pub fn set_onset(channels: &[&[f32]]) -> usize {
    find_onset(&set_envelope(channels))
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


    /// **A hard cut is faded and clamped exactly like the level cut, because
    /// it is the same function.**
    ///
    /// Two faults lived in the capture road's second cut, the hard window at
    /// the end of the recorded silence. It had no fade, so on any clean
    /// capture — where the level threshold is never crossed and the window
    /// is the binding cut — the IR ended at full tail level, a step a
    /// convolver then rings into everything it touches. And it had no
    /// clamp, so an onset read a little early could put the cut in front of
    /// the direct impulse and delete the very thing being measured.
    /// [`cut_with_fade`] owns both cuts now; this is its contract.
    #[test]
    fn a_hard_cut_fades_out_and_refuses_to_eat_the_impulse() {
        let sr = 48_000.0f64;
        let fade = (TAIL_FADE_S * sr) as usize;
        let len = (0.5 * sr) as usize;
        // A spike at 100 with a tail that is still LOUD where the cut falls
        // — the case the level cut never sees, because nothing here sinks
        // anywhere near a floor.
        let mut ir = vec![0.0f32; len];
        ir[100] = 1.0;
        for (i, v) in ir.iter_mut().enumerate().skip(101) {
            *v = 0.5 * ((i % 7) as f32 - 3.0) / 3.0;
        }
        let original = ir.clone();

        let cut = (0.2 * sr) as usize;
        let mut hard = original.clone();
        cut_with_fade(&mut hard, cut, sr);
        assert_eq!(hard.len(), cut, "the cut did not land where it was asked to");
        // Everything before the fade is bit-identical: a cut is a cut, not a
        // gain stage.
        assert_eq!(
            &hard[..cut - fade],
            &original[..cut - fade],
            "the cut reached back past its own fade"
        );
        // And the last sample is on its way to nothing rather than hanging
        // at tail level, which is what shipped.
        let last = hard[hard.len() - 1].abs();
        let was = original[cut - 1].abs();
        assert!(
            last < was * 0.05 && last < 0.02,
            "the hard cut ended at {last} against a tail level of {was} - unfaded"
        );

        // The impulse is sacred. A cut anywhere inside `peak + 1 + fade`
        // would put the fade over the direct arrival, so it is refused
        // outright and the buffer is handed back whole.
        for early in [0usize, 1, 100, 101, 100 + fade] {
            let mut eaten = original.clone();
            cut_with_fade(&mut eaten, early, sr);
            assert_eq!(
                eaten, original,
                "a cut at {early} was allowed to touch the direct impulse at 100"
            );
        }
        // One sample past the clamp it is legal again, and the impulse is
        // still there untouched.
        let mut just = original.clone();
        cut_with_fade(&mut just, 101 + fade, sr);
        assert_eq!(just.len(), 101 + fade, "the first legal cut was refused");
        assert_eq!(just[100], 1.0, "the first legal cut scaled the impulse");
        // A cut at or past the end is nothing to do.
        let mut past = original.clone();
        cut_with_fade(&mut past, len, sr);
        assert_eq!(past, original, "a cut at the end rewrote the buffer");
    }

    /// **A dead channel does not decide where the pair starts.**
    ///
    /// The stereo trim is one number for both channels, which is right — the
    /// level difference between them IS the image, and trimming them apart
    /// moves it. But the number was the EARLIER of the two channels'
    /// onsets, and `find_onset` walks left from its own channel's peak: hand
    /// it a channel with no capture in it and the "peak" is a noise sample,
    /// so it answers an index that has nothing to do with the impulse, and
    /// being early it won. A mono box patched into one side of the loop is
    /// not an exotic setup; it is Tuesday.
    #[test]
    fn a_silent_channel_does_not_drag_the_pairs_onset_to_the_front() {
        let sr = 48_000.0f64;
        let len = (0.2 * sr) as usize;
        let at = 900usize;
        // The channel that actually carries the capture: a lead-in of
        // nothing, then the arrival.
        let mut live = vec![0.0f32; len];
        live[at] = 1.0;
        for i in 1..400 {
            live[at + i] = (0.6 * (-(i as f64) / 300.0).exp()) as f32;
        }
        assert_eq!(find_onset(&live), at, "the fixture's own onset moved");

        // Dead: nothing at all. Its own `find_onset` answers 0.
        let dead = vec![0.0f32; len];
        assert_eq!(find_onset(&dead), 0);
        assert_eq!(
            set_onset(&[&live, &dead]),
            at,
            "a silent channel dragged the trim to the front of the buffer"
        );
        assert_eq!(set_onset(&[&dead, &live]), at, "and the same in the other order");

        // Not dead, just not the capture: a converter hissing 60 dB down,
        // with its loudest sample early. Still no vote — it is below the
        // -40 dB line that decides what counts as an impulse at all.
        let mut hiss = vec![0.0f32; len];
        let mut seed = 5u64;
        for v in hiss.iter_mut() {
            *v = lcg(&mut seed) * 1.0e-3;
        }
        hiss[12] = 2.0e-3;
        assert!(find_onset(&hiss) < at, "the fixture's hiss was supposed to read early");
        assert_eq!(
            set_onset(&[&live, &hiss]),
            at,
            "a channel 54 dB down decided where the impulse starts"
        );

        // A genuine stereo pair keeps the EARLIER arrival, though: two
        // channels of comparable level a few samples apart is a real
        // capture, and clipping the leading one is the damage this whole
        // rule exists to avoid.
        let mut early = vec![0.0f32; len];
        early[at - 30] = 0.7;
        for i in 1..400 {
            early[at - 30 + i] = (0.4 * (-(i as f64) / 300.0).exp()) as f32;
        }
        assert_eq!(
            set_onset(&[&live, &early]),
            at - 30,
            "a true-stereo pair lost its leading arrival"
        );

        // **And the vote that a level test cannot win.** The first repair
        // let a channel vote if any sample of it came within −40 dB of the
        // set's peak, which is a question about how LONG the buffer is: a
        // noise channel's peak stands well above its own RMS and climbs
        // with the sample count, so on a real capture (two million samples,
        // hiss at −40 dBFS) it read −39.1 dB against the impulse and
        // cleared the line. Here is that channel in miniature — its RMS is
        // 54 dB down, its loudest sample is only 34 dB down, and it is
        // early. Reading the onset off the set's envelope makes the lucky
        // sample irrelevant: it is nowhere near the peak the walk starts
        // from, and stationary noise 54 dB down makes no contiguous run
        // above the line to hold the walk open.
        let mut lucky = hiss.clone();
        lucky[7] = 2.0e-2;
        assert!(
            lucky.iter().any(|&v| f64::from(v.abs()) >= 1.0e-2),
            "the fixture's crest was supposed to clear the −40 dB line"
        );
        assert_eq!(
            set_onset(&[&live, &lucky]),
            at,
            "one lucky noise sample decided where the impulse starts"
        );

        // Nothing anywhere is 0, not a panic.
        assert_eq!(set_onset(&[&dead, &dead]), 0);
        assert_eq!(set_onset(&[]), 0);
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

    /// **A folding device is told apart from a decaying plate.**
    ///
    /// The fault: a device whose own sample rate is below the host's folds
    /// everything the sweep plays above its Nyquist back down, and a
    /// descending alias is not a linear response the inverse filter can
    /// cancel. It lands in the recovered IR as a narrowband tone sliding
    /// downward at the inverse's own rate, and the owner heard it as "odd
    /// descending chirpy artifacts" on a Quadraverb sampling near 31 kHz
    /// while the wizard swept to 20.9 kHz.
    ///
    /// The control matters as much as the case. A real plate NARROWS and
    /// DARKENS as it decays — its top modes die first — so a bandwidth test
    /// alone calls that a chirp too. What a plate cannot do is slide: its
    /// modes are at fixed frequencies. Both are built here, and the fit has
    /// to separate them.
    ///
    /// Numbers from the owner's own rig, same minute, only the sweep's top
    /// changed: folded, R² 0.987 over 3.6 octaves at −0.734 oct/s against
    /// the inverse's −0.736; capped at 14 kHz, R² 0.079 over less than one.
    ///
    /// Revert-check: drop `FOLDOVER_R2` to 0.05 and the plate is called a
    /// foldover; raise it past 0.99 and the chirp stops being one.
    #[test]
    fn a_folding_device_is_told_apart_from_a_plate_decaying() {
        let sr = 44_100.0f64;
        let (f1, f2, dur) = (10.0, 20_947.5f64, 15.0);
        let want = (f2 / f1).log2() / dur; // the sweep's rate, +0.736

        // (a) A FOLDOVER: the inverse filter coming back uncancelled. That is
        // a tone descending from the top of the band at the sweep's own rate.
        let n = (6.0 * sr) as usize;
        let mut chirp = vec![0.0f32; n];
        let mut phase = 0.0f64;
        for (i, v) in chirp.iter_mut().enumerate() {
            let t = i as f64 / sr;
            let hz = f2 * 2.0f64.powf(-want * t);
            phase += std::f64::consts::TAU * hz / sr;
            // −6 dB an octave, as the inverse's own envelope is.
            *v = (phase.sin() * 2.0f64.powf(-want * t)) as f32;
        }
        let fit = tail_chirp(&chirp, sr, 0.5).expect("six seconds is enough tail");
        assert!(
            fit.r2 > 0.9,
            "a pure chirp did not fit a line: R2 {:.3}",
            fit.r2
        );
        assert!(
            (fit.slope_oct_s + want).abs() < want * 0.25,
            "a chirp at {:+.3} oct/s was measured at {:+.3}",
            -want,
            fit.slope_oct_s
        );
        assert!(
            tail_is_foldover(fit, f1, f2, dur),
            "the foldover was not called one: {fit:?}"
        );

        // (b) THE CONTROL: a plate. Decaying noise, low-passed harder as it
        // goes, so it narrows and darkens exactly as a real one does — and
        // still must not be called a foldover, because nothing slides.
        let mut seed = 0x9E37_79B9u32;
        let mut rnd = || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed as f32 / u32::MAX as f32) * 2.0 - 1.0
        };
        let mut plate = vec![0.0f32; n];
        let mut lp = 0.0f32;
        for (i, v) in plate.iter_mut().enumerate() {
            let t = i as f32 / sr as f32;
            // The corner falls with time: 8 kHz down to a few hundred hertz.
            let a = 1.0 - (-std::f32::consts::TAU * (8_000.0 * (-t / 2.0).exp() + 200.0)
                / sr as f32)
                .exp();
            lp += a * (rnd() - lp);
            *v = lp * (-t / 1.2).exp();
        }
        let fit = tail_chirp(&plate, sr, 0.5).expect("six seconds is enough tail");
        assert!(
            !tail_is_foldover(fit, f1, f2, dur),
            "a plate narrowing as it decays was called a foldover: {fit:?}"
        );

        // (c) Silence says nothing rather than something wrong.
        assert!(tail_chirp(&vec![0.0f32; n], sr, 0.5).is_none_or(
            |f| !tail_is_foldover(f, f1, f2, dur)
        ));
        // (d) Too short to judge is None, never a verdict.
        assert!(tail_chirp(&vec![0.1f32; 64], sr, 0.0).is_none());
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

    /// A decaying tail sitting on a flat noise floor, in one buffer: an
    /// impulse at `peak_at`, an exponential decay with time constant `tau`,
    /// and stationary noise `floor_db` under the impulse underneath the
    /// whole thing. The shape every recovered IR has, in miniature and
    /// without a deconvolution in the way.
    fn decay_over_floor(
        len: usize,
        peak_at: usize,
        tau_s: f64,
        floor_db: f64,
        rate: f64,
        seed: &mut u64,
    ) -> Vec<f32> {
        let amp = 10.0f64.powf(floor_db / 20.0) * 3.0f64.sqrt();
        let mut ir = vec![0.0f32; len];
        for v in ir.iter_mut() {
            *v = (f64::from(lcg(seed)) * amp) as f32;
        }
        ir[peak_at] = 1.0;
        for i in peak_at + 1..len {
            let t = (i - peak_at) as f64 / rate;
            ir[i] += (f64::from(lcg(seed)) * 0.5 * (-t / tau_s).exp()) as f32;
        }
        ir
    }



    /// **A tail that never reaches the floor it was HANDED is still cut, at
    /// the floor it actually has.**
    ///
    /// The long-IR report, in one buffer. [`recovered_floor_db`] answers
    /// "how far down could the capture hear" — a fact about the RECORDING —
    /// and the truncator then compares it against levels computed from the
    /// deconvolution's buffer, which has a noise floor of its own and not
    /// the recording's. Note the SIGN of the trap, because it is what kept
    /// it hidden: the derived threshold is `ir_peak_db − snr`, so the
    /// CLEANER the capture the LOWER the threshold and the harder it is for
    /// any window to reach. A pristine loop therefore produced the longest
    /// IRs — the whole recorded tail, every time, whatever was in the loop —
    /// which reads as "is every IR supposed to come out the length of the
    /// TAIL row" and not as a truncation bug at all.
    ///
    /// Here the buffer's own floor is 60 dB under its peak and the handed
    /// bound is 95 dB under it: 35 dB out of reach. Before, nothing was cut
    /// and the full two seconds came back. Now the cut lands where the decay
    /// meets the floor the buffer actually has, and the bound still does its
    /// own job — it is a bound, so it is taken when it is the HIGHER of the
    /// two, and a bound above the buffer's floor cuts earlier.
    #[test]
    fn a_tail_that_never_reaches_the_handed_floor_is_cut_at_the_one_it_has() {
        let sr = 48_000.0f64;
        // **Five seconds because that is the shortest buffer this road can
        // produce**, and the floor read now needs a lever arm: two half-second
        // reads a [`TAIL_FLOOR_SPAN_S`] apart plus the fade is ~2.51 s of
        // settled end. A two-second fixture — which is what this was — is not
        // a capture the wizard's TAIL row (5 / 10 / 20 s) can make, and sizing
        // the guard to fit one would be sizing it to fit the test.
        let len = (5.0 * sr) as usize;
        let mut seed = 11u64;
        let ir = decay_over_floor(len, 100, 0.12, -60.0, sr, &mut seed);
        // Where the decay honestly meets that floor: the envelope is
        // 0.5·e^(−t/τ) against an RMS 60 dB down, so it crosses at
        // τ·ln(0.5/10^(−3)) ≈ 0.75 s. The cut has to land near there and
        // nowhere near the end of the buffer.
        let mut cut = ir.clone();
        truncate_at_floor(&mut cut, -95.0, sr);
        assert!(
            cut.len() < len,
            "a tail on a −60 dB floor was handed a −95 dB bound and never cut: \
             {} samples of {len}",
            cut.len()
        );
        let at = cut.len() as f64 / sr;
        assert!(
            (0.5..1.1).contains(&at),
            "the cut landed at {at:.3} s, nowhere near where a −60 dB floor \
             meets a 0.12 s decay"
        );
        // The impulse is untouched, as it is on every path here.
        assert_eq!(cut[100], 1.0, "the cut scaled the direct impulse");

        // And the handed bound is still a bound. Above the buffer's own
        // floor it is the one that bites, and it bites EARLIER — the
        // capture could not resolve past it, so there is nothing down there
        // worth keeping.
        let mut higher = ir.clone();
        truncate_at_floor(&mut higher, -40.0, sr);
        assert!(
            higher.len() < cut.len(),
            "a −40 dB bound kept {} samples against the −95 dB bound's {} — \
             the higher of the two floors is supposed to win",
            higher.len(),
            cut.len()
        );
    }


    /// **A room that is still ringing at the end of its window keeps the
    /// whole window — and "still ringing" now reaches out to eighty seconds
    /// of RT60, not twenty.**
    ///
    /// The other side of [`TAIL_FLOOR_FLAT_DB`]'s guard, and the side that
    /// shipped wrong. What the two reads measure is a SLOPE: a decay falls
    /// 60/RT60 dB per second, so reads a span apart differ by
    /// `60 · span / RT60`. With the reads touching — half a second apart,
    /// which is how this was written — 1.5 dB of allowance is 3 dB per
    /// second, and every box at or above RT60 20 s read as stationary. The
    /// threshold then landed three dB under a level the tail was still
    /// comfortably above, and the cut deleted a fifth to a third of a
    /// capture that had nothing whatever wrong with it. Measured on the
    /// road, flat −85 dBFS loop, 10 s tail row, before → after:
    /// RT60 24 s 8.257 → 10.000, RT60 30 s 7.853 → 10.000, RT60 45 s
    /// 7.173 → 10.000; at the 20 s row, RT60 24 s 18.318 → 20.000 and
    /// RT60 45 s 16.892 → 20.000.
    ///
    /// [`TAIL_FLOOR_SPAN_S`] is the fix — a longer lever arm rather than a
    /// tighter allowance, because two reads half a second apart are steady to
    /// a couple of tenths of a dB and anything under about a dB asks them to
    /// resolve their own noise.
    ///
    /// This test is the boundary's far side. The near side —
    /// `a_buffer_still_decaying_at_its_end_is_not_cut_at_its_own_last_level`
    /// — pins that a decay is refused; `desktop::a_box_that_only_hisses…`
    /// pins that a real floor is still found and still cuts. All three have
    /// to hold at once or the guard is only useful in one direction.
    #[test]
    fn a_thirty_second_decay_is_not_mistaken_for_a_noise_floor() {
        let sr = 48_000.0f64;
        let first = 100 + 1 + (TAIL_FADE_S * sr) as usize;
        // 10 s of window, which is the wizard's default TAIL row, holding
        // decays that outrun it by two to four and a half times over.
        let len = (10.0 * sr) as usize;
        // τ from RT60: the envelope is e^(−t/τ) and RT60 is where it reaches
        // 10^(−3), so τ = RT60 / (3·ln 10). The same conversion the fixture
        // one door up names in words ("RT60 12 s is τ ≈ 1.74 s").
        let tau_of = |rt60: f64| rt60 / (3.0 * std::f64::consts::LN_10);
        for rt60 in [24.0f64, 30.0, 45.0] {
            let mut seed = 77u64;
            let ir = decay_over_floor(len, 100, tau_of(rt60), -120.0, sr, &mut seed);
            assert!(
                tail_floor_db(&ir, first, sr).is_none(),
                "RT60 {rt60} s: the end of a still-falling buffer was read as a floor"
            );
            let mut kept = ir.clone();
            truncate_at_floor(&mut kept, -95.0, sr);
            assert_eq!(
                kept.len(),
                len,
                "RT60 {rt60} s: {:.3} s of a {:.0} s window came back",
                kept.len() as f64 / sr,
                len as f64 / sr
            );
        }
        // **And the far side, stated rather than left to be discovered.** Any
        // two reads a finite distance apart have a slope they can no longer
        // resolve, and 1.5 dB over two seconds puts it at RT60 80 s: past
        // that, a decay does read as stationary and is cut. Ninety seconds of
        // RT60 is a freeze rather than a room, and the alternative — a longer
        // span — no longer fits the 5 s tail row, which is a setting people
        // actually use. The boundary is a choice; this is where it is.
        let mut seed = 78u64;
        let freeze = decay_over_floor(len, 100, tau_of(90.0), -120.0, sr, &mut seed);
        assert!(
            tail_floor_db(&freeze, first, sr).is_some(),
            "the documented boundary moved: RT60 90 s was refused, so the span \
             is longer than TAIL_FLOOR_SPAN_S says"
        );
    }

    /// **A door closing in the last half second is not a noise floor
    /// either.**
    ///
    /// The guard was written as "the end is not still FALLING" and tested only
    /// in that direction, so an end LOUDER than the read before it passed
    /// unconditionally, however far louder. What the caller actually needs to
    /// know is that the end is NOISE, and this road is pointed at rooms and
    /// halls with a TAIL row that runs to twenty seconds — a chair, a door, a
    /// passing car or the HVAC starting in the last stretch is ordinary, not
    /// exotic.
    ///
    /// Measured on this fixture before the second direction existed: the
    /// floor read −35.46 dB instead of −59.99, the threshold landed 24 dB too
    /// high, and the truncation cut at 0.295 s where the clean buffer cut at
    /// 0.751 — sixty per cent of a valid measurement, deleted silently. It
    /// fails in the destructive direction, which is the one an archival tool
    /// may not fail in.
    #[test]
    fn a_disturbance_in_the_last_half_second_is_not_a_floor() {
        let sr = 48_000.0f64;
        let len = (5.0 * sr) as usize;
        let first = 100 + 1 + (TAIL_FADE_S * sr) as usize;
        let mut seed = 991u64;
        let clean = decay_over_floor(len, 100, 0.12, -60.0, sr, &mut seed);
        // The clean buffer is the control: it has a floor and it is cut at it.
        let read = tail_floor_db(&clean, first, sr).expect("a settled end read no floor");
        assert!(
            (f64::from(read) + 60.0).abs() < 3.0,
            "a −60 dB floor read {read} dB"
        );
        let mut cut = clean.clone();
        truncate_at_floor(&mut cut, -95.0, sr);
        let clean_at = cut.len() as f64 / sr;
        assert!(
            (0.5..1.1).contains(&clean_at),
            "the control cut at {clean_at:.3} s"
        );
        // The same buffer with something loud happening in its last stretch.
        // Thirty dB above the floor is a chair; the point is the DIRECTION,
        // so it is checked at three levels and two lengths.
        for (db, secs) in [(-30.0f64, 0.4f64), (-45.0, 0.4), (-50.0, 1.0)] {
            let mut ir = clean.clone();
            let amp = 10.0f64.powf(db / 20.0) * 3.0f64.sqrt();
            let from = len - (secs * sr) as usize;
            for v in ir[from..].iter_mut() {
                *v += (f64::from(lcg(&mut seed)) * amp) as f32;
            }
            assert!(
                tail_floor_db(&ir, first, sr).is_none(),
                "a {db} dB disturbance across the last {secs} s was read as this \
                 buffer's noise floor"
            );
            let mut bumped = ir.clone();
            truncate_at_floor(&mut bumped, -95.0, sr);
            assert_eq!(
                bumped.len(),
                len,
                "a disturbed end cut the measurement at {:.3} s instead of \
                 falling back to the bound it was handed",
                bumped.len() as f64 / sr
            );
        }
    }

    /// **The fade on a cut that never happened cannot reach the direct
    /// impulse.**
    ///
    /// [`fade_and_cut`] refuses a cut earlier than `earliest` precisely so a
    /// fade can never touch the peak, and this module asserts that invariant
    /// by name elsewhere ("a high floor scaled the direct impulse"). The
    /// no-crossing arms bypass it: they call [`fade_tail`] directly, and that
    /// clamps only to the buffer's length.
    ///
    /// Probed before the clamp: `truncate_at_floor` on a one-sample IR
    /// returned `[0.0]` — the whole measurement zeroed — and a 200-sample IR
    /// peaking 1.0 at index 150 came back peaking 0.141. No caller on the
    /// wizard's road reaches it today, because its buffers are five seconds
    /// and up with the peak at the front, so this is a contract this module
    /// states in its own doc rather than a shipped bug — which is exactly the
    /// kind that gets found by the caller who arrives later.
    #[test]
    fn a_declined_cut_never_fades_into_the_direct_impulse() {
        let sr = 48_000.0f64;
        // The smallest buffer there is: a single sample that IS the impulse.
        let mut one = vec![1.0f32];
        truncate_at_floor(&mut one, -100.0, sr);
        assert_eq!(one, vec![1.0f32], "a one-sample IR was faded to nothing");

        // A short buffer whose peak sits near its end, so a full 10 ms fade
        // would reach back over it.
        let mut late = vec![0.0f32; 200];
        late[150] = 1.0;
        truncate_at_floor(&mut late, -100.0, sr);
        assert_eq!(late[150], 1.0, "the declined cut's fade scaled the peak");

        // And the same for the set-wide spelling, where the clamp is the
        // SET's earliest cut rather than each channel's own.
        let mut set = [vec![0.0f32; 200], vec![0.0f32; 200]];
        set[0][150] = 1.0;
        set[1][150] = 0.5;
        truncate_set_at_floor(&mut set, -100.0, sr);
        assert_eq!(set[0][150], 1.0, "the set's declined cut scaled the peak");
        assert_eq!(set[1][150], 0.5, "the set's declined cut scaled the quiet channel");
    }

    /// **A buffer still falling at its end has no floor to read, and is left
    /// at the length whatever ran before this decided.**
    ///
    /// The guard on the fix above, and without it the cure is worse than the
    /// disease. Reading a "floor" off the last stretch of a box whose decay
    /// outruns the recorded window reads the TAIL, not the noise, and a
    /// threshold three dB under a level the tail is still comfortably above
    /// cuts a large fraction of a perfectly good capture off the end.
    ///
    /// [`TAIL_FLOOR_FLAT_DB`] is what separates them, and it separates them
    /// on the one property that actually differs: noise is stationary and a
    /// decay is not. Here a twelve-second decay is still falling by 5 dB a
    /// second where the reading is taken, so the two reads land about 10 dB
    /// apart across the [`TAIL_FLOOR_SPAN_S`] between them, while the same
    /// buffer's noise floor would read the same twice however far apart they
    /// were taken. The span is what makes that true of a THIRTY-second decay
    /// as well; see its own doc for the measurements at the boundary.
    #[test]
    fn a_buffer_still_decaying_at_its_end_is_not_cut_at_its_own_last_level() {
        let sr = 48_000.0f64;
        // The road's shortest window; see the test above for why the fixture
        // is that length rather than shorter.
        let len = (5.0 * sr) as usize;
        let mut seed = 23u64;
        // RT60 12 s is τ ≈ 1.74 s: a decay that has barely started when the
        // buffer runs out, over a floor far below anything here.
        let ir = decay_over_floor(len, 100, 1.74, -120.0, sr, &mut seed);
        let mut kept = ir.clone();
        truncate_at_floor(&mut kept, -95.0, sr);
        assert_eq!(
            kept.len(),
            len,
            "a decay that never reached a floor was cut at {:.3} s anyway",
            kept.len() as f64 / sr
        );
        // The reading itself refuses, rather than the crossing happening to
        // miss — that is the rule, said in the one place it is decided.
        let first = linear_peak(&ir) + 1 + (TAIL_FADE_S * sr) as usize;
        assert!(
            tail_floor_db(&ir, first, sr).is_none(),
            "a buffer falling 10 dB across the span between the reads was called flat"
        );
        // And a buffer whose end IS flat answers with that level. The flat
        // stretch has to cover the whole footprint the reading takes — both
        // reads, the span between them and the fade — or the earlier read
        // lands back in the decay and the answer is `None` for the right
        // reason at the wrong moment.
        let mut settled = ir.clone();
        let settled_s = TAIL_FLOOR_SPAN_S + TAIL_FLOOR_READ_S + 0.2;
        let from = len - (settled_s * sr) as usize;
        let mut noise = 101u64;
        for v in settled[from..].iter_mut() {
            *v = lcg(&mut noise) * 1.0e-3;
        }
        let read = tail_floor_db(&settled, first, sr)
            .expect("a flat end was not recognized as a floor");
        assert!(
            (f64::from(read) - (-65.2)).abs() < 2.0,
            "a −65 dB flat end read {read} dB"
        );
    }

    /// **An IR that is not cut still ends on a ramp.**
    ///
    /// The fade used to live only on the path that found a crossing, so the
    /// capture road's clean captures — where nothing crosses and the window
    /// is the binding cut — ended at full tail level. [`cut_with_fade`] took
    /// that half of the fault; this is the other half, on the truncator's
    /// own early return. A convolver rings a step into everything it
    /// touches, and "it was not shortened" is not a reason to hand it one.
    #[test]
    fn a_truncation_that_declines_to_cut_still_fades_what_it_leaves() {
        let sr = 48_000.0f64;
        let fade = (TAIL_FADE_S * sr) as usize;
        let len = (1.4 * sr) as usize;
        let mut seed = 31u64;
        // The same never-reaches-its-floor buffer as above: nothing crosses,
        // so nothing is cut, so the fade is the only thing that can happen.
        let ir = decay_over_floor(len, 100, 1.74, -120.0, sr, &mut seed);
        let ms = |w: &[f32]| {
            w.iter().map(|&v| f64::from(v) * f64::from(v)).sum::<f64>() / w.len().max(1) as f64
        };
        let before_uncut = ms(&ir[len - fade..]);

        let mut left = ir.clone();
        truncate_at_floor(&mut left, -95.0, sr);
        assert_eq!(left.len(), len, "this fixture is supposed not to be cut");
        assert!(
            ms(&left[len - fade..]) < 0.6 * before_uncut,
            "the un-cut IR ends at {:.3e} against the {:.3e} it carried — \
             unfaded",
            ms(&left[len - fade..]),
            before_uncut
        );
        // Everything before the fade is bit-identical: declining to cut is
        // not a licence to touch the measurement.
        assert_eq!(
            &left[..len - fade],
            &ir[..len - fade],
            "the fade reached back past its own last {fade} samples"
        );
        // The set spelling too, and on both channels.
        let mut set = [ir.clone(), ir.clone()];
        truncate_set_at_floor(&mut set, -95.0, sr);
        for (i, ch) in set.iter().enumerate() {
            assert_eq!(ch.len(), len, "channel {i} was cut");
            assert!(
                ms(&ch[len - fade..]) < 0.6 * before_uncut,
                "channel {i} of an un-cut set was left ending at tail level"
            );
        }
    }

    /// **One channel's scale cannot decide where the OTHER channel's
    /// measurement ends.**
    ///
    /// The set-level half of the long-IR fault, and the mechanism the
    /// capture road actually hit. Every threshold on this road used to be
    /// measured under the SET's peak — right, while both channels are
    /// measurements. A reference channel with no sweep in it is not one:
    /// [`reference_range_db`] carries what the division does with it, which
    /// is a buffer a hundred dB above the side that holds the box. The set's
    /// peak becomes that garbage, the derived bound is measured under it,
    /// and the real channel's entire decay then sits BELOW a threshold it is
    /// supposed to sink through. Nothing votes, nothing is cut, and the pair
    /// comes back the full length of the recorded tail.
    ///
    /// Each channel reading its own floor off its own end takes the decision
    /// away from the other channel's scale — and it makes the older rule
    /// self-enforcing rather than lucky, because the garbage channel now
    /// measures its own level as its own floor and so never stands above its
    /// own threshold, and so never votes.
    #[test]
    fn a_channel_a_hundred_db_up_does_not_hold_the_pair_open() {
        let sr = 48_000.0f64;
        // The road's shortest window; see
        // `a_tail_that_never_reaches_the_handed_floor_is_cut_at_the_one_it_has`
        // for why the fixture is that length.
        let len = (5.0 * sr) as usize;
        let mut seed = 47u64;
        let real = decay_over_floor(len, 100, 0.12, -60.0, sr, &mut seed);
        // What the division hands back for a channel it had no reference
        // for: stationary, and five orders of magnitude up — with its
        // largest excursion near the FRONT, which is where the
        // regularization actually concentrates a noise channel's crest
        // (twelve seeds of it, in [`cut_set_with_fade`]). That detail is not
        // decoration: the clamp every channel searches from is read off the
        // SET's peak, so a crest parked late would put the earliest legal
        // cut past the real channel's whole decay and no cut would be
        // possible at all — correctly, since the impulse is sacred. What
        // this fixture isolates is the THRESHOLD, which is the part that was
        // wrong.
        let mut garbage = vec![0.0f32; len];
        for v in garbage.iter_mut() {
            *v = lcg(&mut seed) * 1.0e5;
        }
        garbage[3] = 2.0e5;
        // The bound exactly as the capture road computes it: the SET's peak,
        // minus everything the capture could resolve.
        let set_peak_db = 20.0 * f64::from(garbage.iter().fold(0.0f32, |m, &v| m.max(v.abs())))
            .log10();
        let bound = (set_peak_db - TAIL_RANGE_MAX_DB) as f32;

        let mut set = [real.clone(), garbage.clone()];
        truncate_set_at_floor(&mut set, bound, sr);
        assert!(
            set[0].len() < len,
            "the pair came back {} samples of {len} — the dead side's scale \
             held the measurement open",
            set[0].len()
        );
        assert_eq!(set[0].len(), set[1].len(), "the pair was cut apart");
        let at = set[0].len() as f64 / sr;
        assert!(
            (0.5..1.1).contains(&at),
            "the cut landed at {at:.3} s rather than where the real channel's \
             decay meets its own floor"
        );
        // And the same pair without the garbage cuts in the same place, to
        // within a window: the dead side buys nothing and costs nothing.
        let mut alone = [real.clone(), real];
        truncate_set_at_floor(&mut alone, bound, sr);
        let win = (TAIL_WINDOW_S * sr) as usize;
        assert!(
            set[0].len().abs_diff(alone[0].len()) <= win,
            "with the dead side the cut is {} and without it {}",
            set[0].len(),
            alone[0].len()
        );
    }

    /// **A reference channel with no sweep in it reads as no reference.**
    ///
    /// [`deconvolve_by_reference`] divides by whatever it is handed, so the
    /// one thing nobody may skip is asking whether the thing being divided
    /// by is a reference at all. The reading is deliberately WITHIN each
    /// channel — the sweep's own level against that same channel's pre-roll
    /// silence — because a comparison across the channels would refuse an
    /// honestly trimmed loop, and an absolute level says nothing.
    ///
    /// Three ways a jack comes back with nothing on it, and all three have
    /// to fall under [`TRUSTWORTHY_RANGE_DB`] while a real loop clears it by
    /// tens of dB.
    #[test]
    fn a_reference_channel_that_never_got_loud_is_not_a_reference() {
        let sr = 48_000.0f64;
        let (f1, f2) = (20.0, 20_000.0);
        let pre = sr as usize;
        let sweep = ess(f1, f2, 1.0, sr);
        let tail = (sr * 0.5) as usize;
        let total = pre + sweep.len() + tail;
        let mut seed = 53u64;
        let mut live = vec![0.0f32; total];
        for (i, v) in live.iter_mut().enumerate() {
            *v = lcg(&mut seed) * 1.0e-4;
            if let Some(&s) = i.checked_sub(pre).and_then(|k| sweep.get(k)) {
                *v += s * 0.5;
            }
        }
        let range = reference_range_db(&live, pre, sweep.len());
        assert!(
            range > TRUSTWORTHY_RANGE_DB,
            "a real loop pass read only {range} dB of range"
        );

        let silent = vec![0.0f32; total];
        assert!(
            reference_range_db(&silent, pre, sweep.len()) < TRUSTWORTHY_RANGE_DB,
            "a jack of digital silence passed as a reference"
        );
        let mut hiss = vec![0.0f32; total];
        for v in hiss.iter_mut() {
            *v = lcg(&mut seed) * 1.0e-4;
        }
        assert!(
            reference_range_db(&hiss, pre, sweep.len()) < TRUSTWORTHY_RANGE_DB,
            "a jack of converter hiss passed as a reference"
        );
        // **A quiet but real loop still clears it, and the quiet has to be
        // REAL.** The first spelling of this scaled the whole buffer by 0.01
        // — signal and converter noise together — and asserted the result
        // still cleared the line. `reference_range_db` is a DIFFERENCE of two
        // dB readings taken from the same buffer, so a uniform gain cancels
        // exactly: live read 75.66 dB and the "quiet" copy read 75.66 dB, and
        // the assertion could not fail whatever the function did.
        //
        // What a quiet loop actually is — a mis-set trim, a padded DI, a long
        // unbalanced run — is a smaller RETURN over the same converter floor,
        // so only the sweep term is scaled here. Measured across that fixture,
        // 1 s sweep at 48 kHz over a −80 dBFS floor: trim 0 dB reads 75.66,
        // −20 dB reads 55.66, −40 dB reads 35.66, −60 dB reads 15.77. The two
        // dead cases read 0.00 and 0.03. That is the separation the line sits
        // in, and it is why the line stays where the capture's is rather than
        // being lowered to buy margin: a jack wrongly called DEAD costs the
        // user that jack's coloration and nothing else, because the other
        // one is borrowed and the engine played the same sample into both. A
        // jack wrongly called LIVE is divided by, and that is the fault this
        // whole measurement exists to prevent.
        let mut quiet = vec![0.0f32; total];
        let mut qseed = 53u64;
        for (i, v) in quiet.iter_mut().enumerate() {
            *v = lcg(&mut qseed) * 1.0e-4;
            if let Some(&s) = i.checked_sub(pre).and_then(|k| sweep.get(k)) {
                *v += s * 0.5 * 0.01;
            }
        }
        let quiet_range = reference_range_db(&quiet, pre, sweep.len());
        assert!(
            quiet_range > TRUSTWORTHY_RANGE_DB,
            "a loop trimmed 40 dB down over its own converter floor was \
             refused for being quiet: {quiet_range} dB"
        );
        // And the margin is stated rather than implied, so a change that eats
        // it is visible here rather than in somebody's session.
        assert!(
            (30.0..45.0).contains(&f64::from(quiet_range)),
            "the 40 dB-trim margin moved: {quiet_range} dB"
        );
        // Nothing to read is not a reference either, rather than a panic.
        assert_eq!(reference_range_db(&live, 0, sweep.len()), 0.0);
        assert_eq!(reference_range_db(&live, total, sweep.len()), 0.0);
    }
}
