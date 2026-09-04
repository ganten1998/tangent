//! The ensemble: three bucket-brigade delay lines wobbling against each
//! other, which is what "strings" meant on every polysynth of the era.
//!
//! **Three settings that differ in kind, not in depth** (brief §2). I is
//! two lines and one slow wobble each — a chorus. II is three lines, each
//! with a slow and a fast wobble at rates that never line up — the
//! Solina's arrangement, the one everyone means by ensemble. III is three
//! lines thrown wider and further apart, slower and deeper — the big,
//! seasick one. A single depth knob would have made three copies of I.
//!
//! **What a bucket brigade does to the signal**, modelled: it loses the
//! top. Every delayed path goes through a one-pole at [`BBD_LOSS_HZ`], so
//! the wet sound sits a little behind and a little darker than the dry,
//! which is why the effect thickens rather than shimmers. The dry passes
//! untouched, in stereo, so the voices' own spread survives the effect.
//!
//! The lines read the mono sum and are spread across the field by Width:
//! the first line left, the last right, the middle (when there is one) in
//! the centre. Delay is read with linear interpolation, which at these
//! modulation depths is below the BBD's own noise.

/// The corner of the loss on every delayed path, hertz.
pub const BBD_LOSS_HZ: f32 = 6_000.0;
/// The longest delay any setting asks for, milliseconds; sizes the line.
const MAX_DELAY_MS: f32 = 32.0;
/// The wet level against the dry, per line.
const WET: f32 = 0.55;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum EnsembleMode {
    #[default]
    Off,
    I,
    II,
    III,
}

impl EnsembleMode {
    pub const ALL: [EnsembleMode; 4] = [
        EnsembleMode::Off,
        EnsembleMode::I,
        EnsembleMode::II,
        EnsembleMode::III,
    ];
    pub fn from_norm(v: f32) -> EnsembleMode {
        let last = (Self::ALL.len() - 1) as f32;
        let at = ((v.clamp(0.0, 1.0) * last).round() as usize).min(Self::ALL.len() - 1);
        Self::ALL[at]
    }
    pub fn label(self) -> &'static str {
        match self {
            EnsembleMode::Off => "off",
            EnsembleMode::I => "I",
            EnsembleMode::II => "II",
            EnsembleMode::III => "III",
        }
    }
}

/// One line's recipe: centre delay, slow LFO rate and depth, fast LFO
/// rate and depth, all in milliseconds and hertz.
#[derive(Clone, Copy)]
struct Line {
    centre_ms: f32,
    slow_hz: f32,
    slow_ms: f32,
    fast_hz: f32,
    fast_ms: f32,
}

/// The three settings. Rates are chosen so no two lines share a period —
/// incommensurate, so the beating never repeats within a phrase.
fn recipe(mode: EnsembleMode) -> &'static [Line] {
    const I: [Line; 2] = [
        Line { centre_ms: 9.0, slow_hz: 0.53, slow_ms: 1.1, fast_hz: 0.0, fast_ms: 0.0 },
        Line { centre_ms: 13.0, slow_hz: 0.67, slow_ms: 1.1, fast_hz: 0.0, fast_ms: 0.0 },
    ];
    const II: [Line; 3] = [
        Line { centre_ms: 7.0, slow_hz: 0.57, slow_ms: 1.4, fast_hz: 5.9, fast_ms: 0.16 },
        Line { centre_ms: 11.0, slow_hz: 0.71, slow_ms: 1.4, fast_hz: 6.2, fast_ms: 0.16 },
        Line { centre_ms: 15.0, slow_hz: 0.83, slow_ms: 1.4, fast_hz: 6.5, fast_ms: 0.16 },
    ];
    const III: [Line; 3] = [
        Line { centre_ms: 6.0, slow_hz: 0.37, slow_ms: 2.6, fast_hz: 6.9, fast_ms: 0.22 },
        Line { centre_ms: 15.0, slow_hz: 0.46, slow_ms: 2.6, fast_hz: 7.3, fast_ms: 0.22 },
        Line { centre_ms: 24.0, slow_hz: 0.59, slow_ms: 2.6, fast_hz: 7.7, fast_ms: 0.22 },
    ];
    match mode {
        EnsembleMode::Off => &[],
        EnsembleMode::I => &I,
        EnsembleMode::II => &II,
        EnsembleMode::III => &III,
    }
}

pub struct Ensemble {
    rate: f32,
    buf: Vec<f32>,
    mask: usize,
    pos: usize,
    slow: [f32; 3],
    fast: [f32; 3],
    lp: [f32; 3],
    loss: f32,
}

impl Ensemble {
    pub fn new(rate: f32) -> Self {
        let need = (MAX_DELAY_MS * 0.001 * rate) as usize + 4;
        let len = need.next_power_of_two();
        Self {
            rate,
            buf: vec![0.0; len],
            mask: len - 1,
            pos: 0,
            // The lines start at different phases so the effect is in
            // motion from the first sample; the offsets are constants.
            slow: [0.0, 0.33, 0.67],
            fast: [0.0, 0.5, 0.25],
            lp: [0.0; 3],
            loss: 1.0 - (-std::f32::consts::TAU * BBD_LOSS_HZ / rate).exp(),
        }
    }

    /// Process `frames` of interleaved stereo in place: dry stays, the
    /// lines are added. `width` 0..1 spreads them across the field.
    pub fn process(&mut self, buf: &mut [f32], frames: usize, mode: EnsembleMode, width: f32) {
        let lines = recipe(mode);
        if lines.is_empty() {
            return;
        }
        let ms = 0.001 * self.rate;
        let width = width.clamp(0.0, 1.0);
        let n = lines.len();
        for frame in buf.chunks_exact_mut(2).take(frames) {
            let mono = 0.5 * (frame[0] + frame[1]);
            self.buf[self.pos] = mono;
            let (mut l, mut r) = (0.0_f32, 0.0_f32);
            for (i, line) in lines.iter().enumerate() {
                self.slow[i] += line.slow_hz / self.rate;
                if self.slow[i] >= 1.0 {
                    self.slow[i] -= 1.0;
                }
                self.fast[i] += line.fast_hz / self.rate;
                if self.fast[i] >= 1.0 {
                    self.fast[i] -= 1.0;
                }
                let wobble = line.slow_ms * (self.slow[i] * std::f32::consts::TAU).sin()
                    + line.fast_ms * (self.fast[i] * std::f32::consts::TAU).sin();
                let delay = (line.centre_ms + wobble) * ms;
                let d = delay.max(1.0);
                let di = d as usize;
                let frac = d - di as f32;
                let a = self.buf[(self.pos + self.mask + 1 - di) & self.mask];
                let b = self.buf[(self.pos + self.mask - di) & self.mask];
                let tap = a + frac * (b - a);
                // The brigade's loss.
                self.lp[i] += self.loss * (tap - self.lp[i]);
                let wet = self.lp[i] * WET;
                // Line i's place in the field: −1 .. +1 across the lines,
                // scaled by the width; a lone middle line sits centred.
                let place = if n == 1 {
                    0.0
                } else {
                    (2.0 * i as f32 / (n - 1) as f32 - 1.0) * width
                };
                let angle = (place + 1.0) * 0.25 * std::f32::consts::PI;
                l += wet * angle.cos();
                r += wet * angle.sin();
            }
            frame[0] += l;
            frame[1] += r;
            self.pos = (self.pos + 1) & self.mask;
        }
    }

    pub fn clear(&mut self) {
        self.buf.iter_mut().for_each(|v| *v = 0.0);
        self.lp = [0.0; 3];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::analog::measure::{db, goertzel_hann};

    const SR: f32 = 48_000.0;

    /// Off is a wire: the buffer comes back bit for bit.
    #[test]
    fn off_is_a_wire() {
        let mut e = Ensemble::new(SR);
        let mut buf: Vec<f32> = (0..512).map(|i| (i as f32 * 0.1).sin()).collect();
        let before = buf.clone();
        e.process(&mut buf, 256, EnsembleMode::Off, 0.6);
        assert_eq!(buf, before);
    }

    /// Each setting moves the sound — a steady tone comes out with
    /// sidebands and a level that differs between the channels as the
    /// lines swing — and the three settings differ from each other.
    #[test]
    fn the_three_settings_are_three_different_things() {
        let render = |mode: EnsembleMode| {
            let mut e = Ensemble::new(SR);
            let n = (SR * 2.0) as usize;
            let mut buf = vec![0.0_f32; n * 2];
            for (i, f) in buf.chunks_exact_mut(2).enumerate() {
                let v = 0.3 * (i as f64 * 440.0 * std::f64::consts::TAU / f64::from(SR)).sin() as f32;
                f[0] = v;
                f[1] = v;
            }
            e.process(&mut buf, n, mode, 0.6);
            buf
        };
        let dry = render(EnsembleMode::Off);
        let mut prints = Vec::new();
        for mode in [EnsembleMode::I, EnsembleMode::II, EnsembleMode::III] {
            let wet = render(mode);
            // Not a wire.
            let diff: f32 = wet.iter().zip(&dry).map(|(a, b)| (a - b).abs()).sum::<f32>() / wet.len() as f32;
            assert!(diff > 0.01, "{} changed nothing", mode.label());
            // Sidebands: energy a few hertz off the tone that the dry
            // signal does not have.
            let left: Vec<f32> = wet.chunks_exact(2).map(|f| f[0]).collect();
            let side = goertzel_hann(&left, 443.0, SR);
            prints.push(side);
            assert!(side > 1e-3, "{}: no sideband ({})", mode.label(), db(side));
            // Stereo: the two channels are not the same signal.
            let ch_diff: f32 = wet.chunks_exact(2).map(|f| (f[0] - f[1]).abs()).sum::<f32>() / left.len() as f32;
            assert!(ch_diff > 0.005, "{}: mono", mode.label());
        }
        assert!(
            prints[0] != prints[1] && prints[1] != prints[2],
            "settings I, II, III read the same sideband: {prints:?}"
        );
    }

    /// The brigade's loss: a 12 kHz tone's wet share is well under a
    /// 200 Hz tone's, with the dry removed.
    #[test]
    fn the_delayed_paths_lose_the_top() {
        let wet_gain = |hz: f32| {
            let mut e = Ensemble::new(SR);
            let n = (SR * 1.0) as usize;
            let mut buf = vec![0.0_f32; n * 2];
            let mut dry = vec![0.0_f32; n * 2];
            for (i, f) in buf.chunks_exact_mut(2).enumerate() {
                let v = 0.3 * (i as f64 * f64::from(hz) * std::f64::consts::TAU / f64::from(SR)).sin() as f32;
                f[0] = v;
                f[1] = v;
                dry[2 * i] = v;
                dry[2 * i + 1] = v;
            }
            e.process(&mut buf, n, EnsembleMode::II, 0.0);
            let wet: Vec<f32> = buf.iter().zip(&dry).map(|(a, b)| a - b).step_by(2).collect();
            goertzel_hann(&wet[n / 2..], hz, SR) / 0.3
        };
        let (lo, hi) = (wet_gain(200.0), wet_gain(12_000.0));
        assert!(db(hi / lo) < -5.0, "12 kHz wet is only {:.1} dB under 200 Hz", db(hi / lo));
    }
}
