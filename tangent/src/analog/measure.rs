//! The ruler: how the module's tests and the bench measure the instrument,
//! in one place so they cannot disagree.
//!
//! **Two measurement traps, both already paid for in this repo**, and both
//! written into the tools below rather than left as a warning:
//!
//! - A rectangular-window Goertzel LIES about a decay. Its sidelobes fall
//!   as 1/f, so a loud fundamental leaks a floor under every partial and a
//!   threshold reads a phantom. Everything spectral here goes through a
//!   Hann window ([`goertzel_hann`]), whose sidelobes fall as 1/f³.
//! - A threshold crossing on a raw level reads beats as decay. Decays are
//!   read from a Schroeder backward integral ([`decay_t60`]), which is
//!   monotone by construction, over the −5..−35 dB span, doubled — the T30
//!   estimate every room-acoustics meter uses.
//!
//! And one the brief adds: a spectral centroid is useless with a
//! fundamental in the frame, so nothing here computes one.

use std::f32::consts::TAU;

pub fn db(v: f32) -> f32 {
    20.0 * v.max(1e-12).log10()
}

pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0_f32, |m, s| m.max(s.abs()))
}

pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    let e: f64 = x.iter().map(|s| f64::from(*s) * f64::from(*s)).sum();
    (e / x.len() as f64).sqrt() as f32
}

/// Magnitude at one frequency through a Hann window, normalised so a
/// full-scale sine reads 1.0. f64 accumulation, so −80 dB components read
/// without the sum's own rounding noise under them.
pub fn goertzel_hann(x: &[f32], freq: f32, rate: f32) -> f32 {
    let (re, im) = dft_bin_hann(x, freq, rate);
    (re * re + im * im).sqrt() as f32
}

/// The complex DFT bin at `freq` through a Hann window, as (re, im),
/// normalised like [`goertzel_hann`]. Direct summation rather than the
/// Goertzel recurrence: the recurrence's phase is defined relative to
/// its own last sample and the callers below want the phase relative to
/// the window's first, which is what makes two bins comparable.
pub fn dft_bin_hann(x: &[f32], freq: f32, rate: f32) -> (f64, f64) {
    if x.is_empty() || freq <= 0.0 || freq >= rate * 0.5 {
        return (0.0, 0.0);
    }
    let n = x.len();
    let w = f64::from(TAU) * f64::from(freq) / f64::from(rate);
    let step = f64::from(TAU) / n as f64;
    let (cw, sw) = (w.cos(), w.sin());
    // Rotate a unit phasor rather than call sin/cos per sample; the drift
    // over a few hundred thousand samples is far below f32 noise.
    let (mut c, mut s) = (1.0_f64, 0.0_f64);
    let (mut re, mut im) = (0.0_f64, 0.0_f64);
    for (i, &v) in x.iter().enumerate() {
        let win = 0.5 - 0.5 * (step * i as f64).cos();
        let v = f64::from(v) * win;
        re += v * c;
        im -= v * s;
        let c2 = c * cw - s * sw;
        s = s * cw + c * sw;
        c = c2;
    }
    // Hann's coherent gain is one half; a unit sine's bin is N/4.
    let norm = 4.0 / n as f64;
    (re * norm, im * norm)
}

/// The frequency of the strongest component near `guess`, to a small
/// fraction of a cent, by phase advance: the bin at `guess` is taken over
/// two windows one window apart, and the phase the component turned
/// between them, beyond what `guess` itself would have turned, is the
/// error. Unambiguous to ±rate/(2·window), which is why the window is
/// sized from the guess — long enough to hold the neighbouring harmonic
/// outside the Hann main lobe, short enough that a note a few cents off
/// cannot wrap.
pub fn measure_hz(x: &[f32], guess: f32, rate: f32) -> f32 {
    // Eight periods keeps the second harmonic eight bins away, where the
    // Hann sidelobes sit near −50 dB; the unambiguous range is then a
    // sixteenth of the guess, about a semitone.
    let win = ((rate / guess * 8.0) as usize).clamp(256, 16384).min(x.len() / 2);
    if win < 32 {
        return 0.0;
    }
    let w = f64::from(TAU) * f64::from(guess) / f64::from(rate);
    let a = dft_bin_hann(&x[..win], guess, rate);
    let b = dft_bin_hann(&x[win..2 * win], guess, rate);
    let pa = a.1.atan2(a.0);
    let pb = b.1.atan2(b.0);
    // Each window's phasor restarts at its first sample, so the phase a
    // component of exactly `guess` turns between windows is ω·win — which
    // is a whole number of turns only when `win` is a whole number of
    // periods, and it was rounded. Take it out before wrapping.
    let mut d = (pb - pa - w * win as f64).rem_euclid(f64::from(TAU));
    while d > std::f64::consts::PI {
        d -= f64::from(TAU);
    }
    while d < -std::f64::consts::PI {
        d += f64::from(TAU);
    }
    // The sign: the bin correlates with e^{−jωt}, so a component above the
    // bin's frequency turns its phase FORWARD by Δω per sample, and the
    // second window reads +Δω·win more than the first.
    let hop_secs = win as f64 / f64::from(rate);
    (f64::from(guess) + d / (f64::from(TAU) * hop_secs)) as f32
}

/// The envelope of `x` in hops of `hop` samples: the RMS of each hop.
pub fn envelope(x: &[f32], hop: usize) -> Vec<f32> {
    x.chunks(hop.max(1)).map(rms).collect()
}

/// T60 of a decaying signal by Schroeder backward integration of its
/// energy, read over −5..−35 dB and doubled. Returns `None` when the
/// signal never fell 35 dB inside the window.
pub fn decay_t60(x: &[f32], rate: f32) -> Option<f32> {
    const HOP: usize = 64;
    let mut edc: Vec<f64> = x
        .chunks(HOP)
        .map(|c| c.iter().map(|s| f64::from(*s) * f64::from(*s)).sum::<f64>())
        .collect();
    if edc.len() < 4 {
        return None;
    }
    for j in (0..edc.len() - 1).rev() {
        edc[j] += edc[j + 1];
    }
    let total = edc[0];
    if total <= 0.0 {
        return None;
    }
    let db_of = |v: f64| 10.0 * (v / total).max(1e-300).log10();
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
    let secs_per_hop = HOP as f64 / f64::from(rate);
    match (cross(-5.0), cross(-35.0)) {
        (Some(a), Some(b)) if b > a => Some((2.0 * (b - a) * secs_per_hop) as f32),
        _ => None,
    }
}

/// Seconds for a rising envelope to go from 10% to 90% of its peak; the
/// envelope is read in short RMS hops so a waveform's own cycle does not
/// masquerade as the rise.
pub fn rise_10_90(x: &[f32], rate: f32) -> Option<f32> {
    const HOP: usize = 16;
    let env = envelope(x, HOP);
    let pk = env.iter().cloned().fold(0.0_f32, f32::max);
    if pk <= 0.0 {
        return None;
    }
    let first_over = |level: f32| env.iter().position(|&v| v >= level * pk);
    let (a, b) = (first_over(0.1)?, first_over(0.9)?);
    Some((b.saturating_sub(a) * HOP) as f32 / rate)
}

/// Largest one-sample step in `x[a..b]`: the first difference, the plain
/// measure of an edge.
pub fn d1(x: &[f32], a: usize, b: usize) -> f32 {
    let b = b.min(x.len());
    let mut m = 0.0_f32;
    for n in a.max(1)..b {
        m = m.max((x[n] - x[n - 1]).abs());
    }
    m
}

/// Largest second difference in `x[a..b]`. A sinusoid's second difference
/// is its first scaled by a small angle, so a step — whose second
/// difference is the whole step, twice — stands clear of a loud low bed
/// that the first difference alone would hide it under.
pub fn d2(x: &[f32], a: usize, b: usize) -> f32 {
    let b = b.min(x.len());
    let mut m = 0.0_f32;
    for n in a.max(2)..b {
        m = m.max((x[n] - 2.0 * x[n - 1] + x[n - 2]).abs());
    }
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    /// Phase accumulated in f64: `i · hz · 2π / rate` in f32 is off by
    /// tenths of a radian within a second, and a ruler checked against a
    /// bent sine learns the bend.
    fn sine(hz: f32, secs: f32) -> Vec<f32> {
        (0..(secs * SR) as usize)
            .map(|i| (i as f64 * f64::from(hz) * f64::from(TAU) / f64::from(SR)).sin() as f32)
            .collect()
    }

    /// The ruler is checked against a sine it cannot argue with: level,
    /// frequency, both directions of error.
    #[test]
    fn the_ruler_reads_a_sine_true() {
        let x = sine(1_000.0, 0.5);
        let m = goertzel_hann(&x, 1_000.0, SR);
        assert!((m - 1.0).abs() < 0.01, "unit sine read {m}");
        for off in [-3.0_f32, -0.3, 0.0, 0.7, 4.0] {
            let hz = 261.63 + off;
            let x = sine(hz, 0.5);
            let got = measure_hz(&x, 261.63, SR);
            let cents = 1200.0 * (got / hz).log2();
            assert!(cents.abs() < 0.05, "{hz} Hz read {got} ({cents:+.3} cents)");
        }
    }

    /// An exact exponential decay reads its own T60 within a percent.
    #[test]
    fn the_decay_ruler_reads_an_exponential_true() {
        let t60 = 0.8_f32;
        let tau = t60 / (1000.0_f32).ln();
        let x: Vec<f32> = (0..(SR * 1.5) as usize)
            .map(|i| {
                let t = i as f32 / SR;
                (-t / tau).exp() * (t * 440.0 * TAU).sin()
            })
            .collect();
        let got = decay_t60(&x, SR).expect("fell 35 dB");
        assert!((got / t60 - 1.0).abs() < 0.02, "T60 {t60} read {got}");
    }
}
