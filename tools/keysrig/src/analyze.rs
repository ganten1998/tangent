//! Measuring the model, so nobody has to argue about it.
//!
//! **HF ratio, not spectral centroid.** The centroid is the obvious choice and
//! it is useless here: the fundamental carries so much of the power that
//! deleting an entire upper partial moves it a few percent. The share of power
//! that survives a first-difference highpass moves by tens of decibels over
//! the same change, which is why the DX7 bank was measured that way and why
//! this is too.

/// Share of power above roughly a quarter of Nyquist, as a first-difference
/// highpass. Dimensionless, 0 to 1.
pub fn hf_ratio(x: &[f32]) -> f32 {
    if x.len() < 2 {
        return 0.0;
    }
    let mut total = 0.0_f64;
    let mut high = 0.0_f64;
    let mut prev = x[0];
    for &s in &x[1..] {
        let d = f64::from(s - prev);
        high += d * d;
        total += f64::from(s) * f64::from(s);
        prev = s;
    }
    if total <= 0.0 {
        return 0.0;
    }
    (high / (total * 4.0)) as f32
}

/// How lopsided the waveform is: 0 is symmetric, 1 is entirely one-sided.
///
/// **This is the bark, as a number.** A magnetic pickup driven hard grows one
/// half of the cycle and compresses the other, which is even-harmonic
/// distortion; a symmetric waveshaper cannot produce it and neither can a
/// clean resonator. If this does not rise with velocity, the pickup model is
/// not doing its job.
pub fn asymmetry(x: &[f32]) -> f32 {
    let mut hi = 0.0_f32;
    let mut lo = 0.0_f32;
    for &s in x {
        hi = hi.max(s);
        lo = lo.min(s);
    }
    let (hi, lo) = (hi.abs(), lo.abs());
    if hi + lo <= 1e-12 {
        return 0.0;
    }
    (hi - lo).abs() / (hi + lo)
}

/// Magnitude at one frequency, by Goertzel. Cheap enough to call per partial.
pub fn goertzel(x: &[f32], freq: f32, rate: f32) -> f32 {
    if x.is_empty() || freq <= 0.0 || freq >= rate * 0.5 {
        return 0.0;
    }
    let w = std::f32::consts::TAU * freq / rate;
    let c = 2.0 * w.cos();
    let (mut s1, mut s2) = (0.0_f32, 0.0_f32);
    for &v in x {
        let s0 = v + c * s1 - s2;
        s2 = s1;
        s1 = s0;
    }
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / (x.len() as f32 * 0.5)
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

pub fn db(v: f32) -> f32 {
    20.0 * v.max(1e-12).log10()
}

/// Seconds for the envelope to fall 60 dB from its peak, or the length of the
/// window if it never does. Measured on a decimated envelope so it is robust
/// against a single loud sample.
pub fn t60(x: &[f32], rate: f32) -> f32 {
    const HOP: usize = 256;
    let mut env = Vec::with_capacity(x.len() / HOP + 1);
    for c in x.chunks(HOP) {
        env.push(peak(c));
    }
    let Some(&pk) = env.iter().max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
    else {
        return 0.0;
    };
    if pk <= 0.0 {
        return 0.0;
    }
    let target = pk * 0.001;
    let start = env.iter().position(|&v| v >= pk * 0.999).unwrap_or(0);
    for (i, &v) in env.iter().enumerate().skip(start) {
        if v < target {
            return ((i - start) * HOP) as f32 / rate;
        }
    }
    (env.len() * HOP) as f32 / rate
}

/// Magnitude at one frequency through a Hann window.
///
/// **The plain Goertzel above is a rectangular window**, and a rectangle's
/// sidelobes fall off as 1/f: the fundamental of a piano note, thirty-odd
/// decibels above everything else, bleeds a phantom floor under every partial
/// within a couple of kilohertz of it. The first partial_t60 measured that
/// floor and reported 5.7 s on a partial that was gone in half a second.
/// Hann sidelobes fall as 1/f^3, which puts the bleed eighty decibels down
/// where it stops mattering.
pub fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
    if x.is_empty() || freq <= 0.0 || freq >= rate * 0.5 {
        return 0.0;
    }
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
    // Hann's coherent gain is half the rectangle's; normalise by N/4 so the
    // numbers stay on the same scale as `goertzel`.
    (s1 * s1 + s2 * s2 - c * s1 * s2).max(0.0).sqrt() / (n * 0.25)
}

/// A partial's decay: seconds for the level at `freq` to fall 60 dB.
///
/// **Two lies the first version told.** It tracked the partial with a
/// rectangular-window Goertzel and waited for the raw magnitude to cross a
/// threshold. The rectangle let the fundamental leak a floor under every
/// high partial (see `goertzel_hann`), so anything that died fast was
/// reported as decaying exactly as slowly as the fundamental — which is what
/// made n4 "outlive" n1 in the report. And a bare threshold crossing reads a
/// unison's beat null as decay: three strings a cent or two apart swing the
/// summed magnitude ten decibels either way with a period of seconds, so the
/// answer depended on where the beat happened to be.
///
/// So: a Hann-windowed track, squared, then Schroeder backward integration —
/// energy remaining from t to the end — which is monotone by construction,
/// so beats integrate out instead of masquerading as decay. T60 is read off
/// the -5 to -35 dB span of that curve and doubled, the T30 estimate every
/// room-acoustics meter uses, and for the same reason: the tail of the curve
/// is floor, not signal.
pub fn partial_t60(x: &[f32], freq: f32, rate: f32) -> f32 {
    let win = ((rate / freq * 12.0) as usize).clamp(1024, 16384);
    let hop = win / 2;
    let mut edc: Vec<f64> = Vec::new();
    let mut i = 0usize;
    while i + win <= x.len() {
        let m = f64::from(goertzel_hann(&x[i..i + win], freq, rate));
        edc.push(m * m);
        i += hop;
    }
    if edc.len() < 4 {
        return 0.0;
    }
    for j in (0..edc.len() - 1).rev() {
        edc[j] += edc[j + 1];
    }
    let total = edc[0];
    if total <= 0.0 {
        return 0.0;
    }
    let db_of = |v: f64| 10.0 * (v / total).max(1e-300).log10();
    // First hop index (fractional) where the curve crosses `target` dB.
    let cross = |target: f64| -> Option<f64> {
        let mut prev = db_of(edc[0]);
        for j in 1..edc.len() {
            let cur = db_of(edc[j]);
            if prev >= target && cur < target {
                let frac = (prev - target) / (prev - cur).max(1e-12);
                return Some((j - 1) as f64 + frac);
            }
            prev = cur;
        }
        None
    };
    let secs_per_hop = f64::from(hop as u32) / f64::from(rate);
    match (cross(-5.0), cross(-35.0)) {
        (Some(a), Some(b)) if b > a => (2.0 * (b - a) * secs_per_hop) as f32,
        // Never fell 35 dB inside the window: report the window, which keeps
        // the old "longer than I can see" semantics.
        _ => x.len() as f32 / rate,
    }
}
