//! **The picture of a clip at every zoom, built once.**
//!
//! A waveform column is the lowest and the highest sample under one pixel.
//! Computing that from the samples every time the view moves is a pass over
//! the whole visible span per frame — which is what the host used to do, at a
//! five-percent deadband, and what made a zoom smear and then hitch. This is
//! the alternative: a pyramid of `[min, max]` blocks, the finest [`BASE_BLOCK`]
//! frames wide and every level above it twice as coarse, built ONCE when a
//! file is decoded and read at any zoom in a handful of steps per column.
//!
//! **Why it lives on the firewall side.** `tangent-ui` never sees a file or a
//! sample; it is handed pictures through `ports`. A pyramid IS a picture — a
//! stack of min/max envelopes — and holding it lets the painter answer any
//! window without a round trip to the host. The host builds it from the
//! decoded samples (off the UI thread, beside the decode) and hands over an
//! `Arc`, so a clip's picture costs one allocation for the life of the file.
//!
//! **What "exact" means here.** A query for frames `[from, to)` folds every
//! base block that INTERSECTS the range, so its answer is exactly the min and
//! max of a range that may extend up to `BASE_BLOCK - 1` frames past either
//! end. At any zoom where a column spans more than a block the picture is
//! indistinguishable from a sample walk; below that (a 16-frame column is a
//! third of a millisecond of audio) the columns plateau in blocks. Sixteen
//! frames was chosen against memory: the chain costs `8 / BASE_BLOCK` bytes
//! per frame of audio, an eighth of the f32 stereo it describes, so a
//! twenty-minute clip (the decoder's ceiling) carries a 58 MB picture.
//!
//! The rules of the picture are the host's old ones, kept: **min and max, not
//! a peak**; non-finite samples skipped; an empty or wholly non-finite range
//! is the flat line `(0.0, 0.0)`; the answer is clamped to `-1..=1`; both
//! channels fold into one outline.

use std::sync::Arc;

/// Frames per block at the finest level. See the module doc for the trade.
pub const BASE_BLOCK: usize = 16;

/// The identity for a min/max fold: nothing seen yet.
const NOTHING: [f32; 2] = [f32::INFINITY, f32::NEG_INFINITY];

/// A min/max pyramid over one decoded file. Built with [`Peaks::build`],
/// read with [`Peaks::minmax`].
#[derive(Debug, Clone, PartialEq)]
pub struct Peaks {
    rate: u32,
    frames: usize,
    base: usize,
    /// `levels[k]` holds one `[min, max]` per block of `base << k` frames;
    /// `levels[0]` is the finest, the last has a single block. A block that
    /// saw no finite sample holds [`NOTHING`], never `(0, 0)`, so it cannot
    /// pull a real neighbour's minimum down to zero when folded.
    levels: Vec<Vec<[f32; 2]>>,
}

#[inline]
fn fold(into: &mut [f32; 2], b: [f32; 2]) {
    into[0] = into[0].min(b[0]);
    into[1] = into[1].max(b[1]);
}

/// The drawn answer for a fold: flat for nothing, clamped otherwise.
///
/// **Both ends of both values.** This clamped the minimum from below and
/// the maximum from above only, which is enough while the block straddles
/// the drawable range and wrong the moment it does not: a block whose every
/// sample sits above 1.0 (a hot file, a float WAV that was never
/// normalised) answered `(1.5, 1.0)`, a minimum ABOVE its maximum, and a
/// block wholly below -1.0 the mirror of it. The host's column loop clamped
/// each end to `-1..=1` and drew such a block as a bar pinned to the rail,
/// which is what the picture promises in the module doc.
#[inline]
fn finish(acc: [f32; 2]) -> (f32, f32) {
    if acc[0] <= acc[1] {
        (acc[0].clamp(-1.0, 1.0), acc[1].clamp(-1.0, 1.0))
    } else {
        (0.0, 0.0)
    }
}

impl Peaks {
    /// Build the pyramid over interleaved samples with `channels` per frame,
    /// recorded at `rate` Hz. One pass over the samples plus a geometric tail.
    pub fn build(samples: &[f32], channels: usize, rate: u32) -> Self {
        Self::build_with(samples, channels, rate, BASE_BLOCK)
    }

    /// [`Peaks::build`] with a chosen finest block, for tests and tuning.
    pub fn build_with(samples: &[f32], channels: usize, rate: u32, base: usize) -> Self {
        let channels = channels.max(1);
        let base = base.max(1);
        let frames = samples.len() / channels;
        let blocks = frames.div_ceil(base);
        let mut finest = Vec::with_capacity(blocks);
        for b in 0..blocks {
            let from = b * base * channels;
            let to = ((b + 1) * base).min(frames) * channels;
            let mut acc = NOTHING;
            for &v in &samples[from..to] {
                if v.is_finite() {
                    fold(&mut acc, [v, v]);
                }
            }
            finest.push(acc);
        }
        let mut levels = vec![finest];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let prev = levels.last().expect("just checked");
            let next: Vec<[f32; 2]> = prev
                .chunks(2)
                .map(|pair| {
                    let mut acc = pair[0];
                    if let Some(&b) = pair.get(1) {
                        fold(&mut acc, b);
                    }
                    acc
                })
                .collect();
            levels.push(next);
        }
        Self {
            rate,
            frames,
            base,
            levels,
        }
    }

    /// How many frames the file has.
    pub fn frames(&self) -> usize {
        self.frames
    }

    /// The file's rate, so seconds and frames can be converted beside it.
    pub fn rate(&self) -> u32 {
        self.rate
    }

    /// The file's length in seconds; zero for an empty or rateless one.
    pub fn seconds(&self) -> f64 {
        if self.rate == 0 {
            return 0.0;
        }
        self.frames as f64 / f64::from(self.rate)
    }

    /// Frames per block at the finest level.
    pub fn base_block(&self) -> usize {
        self.base
    }

    /// Frames at `seconds` into the file, rounded to the nearest frame and
    /// clamped to the file — the one conversion every reader should use, so
    /// two columns that meet at a second cannot disagree about which frame it
    /// is.
    pub fn frame_at(&self, seconds: f64) -> usize {
        if !(seconds > 0.0) || self.rate == 0 {
            return 0;
        }
        let f = (seconds * f64::from(self.rate)).round();
        if f >= self.frames as f64 {
            self.frames
        } else {
            f as usize
        }
    }

    /// The lowest and highest sample across frames `[from, to)`, both
    /// channels folded, clamped to `-1..=1`; `(0.0, 0.0)` when the range is
    /// empty, past the end, or holds nothing finite. Exact to the base block
    /// (see the module doc). `O(log blocks)`.
    pub fn minmax(&self, from: usize, to: usize) -> (f32, f32) {
        let to = to.min(self.frames);
        if from >= to || self.levels.is_empty() {
            return (0.0, 0.0);
        }
        // Block indices at the finest level: [a, b).
        let mut a = from / self.base;
        let mut b = (to - 1) / self.base + 1;
        let mut acc = NOTHING;
        for level in &self.levels {
            if a >= b {
                break;
            }
            // A short range is cheaper to read where it is than to climb.
            if b - a <= 3 {
                for blk in &level[a..b] {
                    fold(&mut acc, *blk);
                }
                break;
            }
            // Peel the odd edges at this level, then climb one.
            if a % 2 == 1 {
                fold(&mut acc, level[a]);
                a += 1;
            }
            if b % 2 == 1 {
                b -= 1;
                fold(&mut acc, level[b]);
            }
            a /= 2;
            b /= 2;
        }
        finish(acc)
    }

    /// [`Peaks::minmax`] over a span in seconds — the painter's unit.
    pub fn minmax_s(&self, from_s: f64, to_s: f64) -> (f32, f32) {
        let a = self.frame_at(from_s);
        let b = self.frame_at(to_s);
        // A span narrower than a frame still shows the frame it stands on.
        self.minmax(a, b.max(a + 1))
    }

    /// A whole-file picture `buckets` wide, `(min, max)` per bucket.
    pub fn envelope_minmax(&self, buckets: usize) -> Vec<(f32, f32)> {
        if buckets == 0 || self.frames == 0 {
            return Vec::new();
        }
        (0..buckets)
            .map(|i| {
                let a = i * self.frames / buckets;
                let b = ((i + 1) * self.frames / buckets).max(a + 1);
                self.minmax(a, b)
            })
            .collect()
    }

    /// A whole-file PEAK envelope in `0..=1`, `buckets` wide — the band's
    /// scrubber picture, the shape `decode::Clip::envelope` used to walk the
    /// samples for.
    pub fn envelope(&self, buckets: usize) -> Vec<f32> {
        self.envelope_minmax(buckets)
            .into_iter()
            .map(|(lo, hi)| lo.abs().max(hi.abs()).min(1.0))
            .collect()
    }

    /// Wrap for the port: one picture, shared for the life of the file.
    pub fn shared(self) -> Arc<Self> {
        Arc::new(self)
    }

    /// Bytes this picture holds, for anybody accounting for a cache.
    pub fn bytes(&self) -> usize {
        self.levels.iter().map(|l| l.len() * 8).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A deterministic signal with structure at every scale: a slow sine, a
    /// fast one, DC on the right channel, and a burst of silence.
    fn signal(frames: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let t = f as f32;
            let slow = (t * 0.0007).sin() * 0.8;
            let fast = (t * 0.31).sin() * 0.15;
            let l = if (3000..3400).contains(&f) { 0.0 } else { slow + fast };
            let r = 0.2 + slow * 0.5;
            v.push(l);
            v.push(r);
        }
        v
    }

    /// The host's old walk, verbatim in spirit: min and max, both channels,
    /// non-finite skipped, seeded at the infinities, flat for nothing.
    fn brute(samples: &[f32], channels: usize, from: usize, to: usize) -> (f32, f32) {
        let frames = samples.len() / channels;
        let to = to.min(frames);
        let mut acc = NOTHING;
        if from < to {
            for &v in &samples[from * channels..to * channels] {
                if v.is_finite() {
                    fold(&mut acc, [v, v]);
                }
            }
        }
        finish(acc)
    }

    #[test]
    fn a_block_aligned_query_is_the_sample_walk_exactly() {
        let s = signal(10_000);
        let p = Peaks::build(&s, 2, 48_000);
        for (from, to) in [(0, 16), (0, 10_000), (16, 4096), (4096, 4112), (9_984, 10_000), (512, 9_984)] {
            assert_eq!(p.minmax(from, to), brute(&s, 2, from, to), "{from}..{to}");
        }
    }

    #[test]
    fn an_unaligned_query_is_the_walk_over_the_blocks_it_touches() {
        let s = signal(10_000);
        let p = Peaks::build(&s, 2, 48_000);
        let mut seed = 7u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            (seed >> 33) as usize
        };
        for _ in 0..2_000 {
            let a = next() % 10_000;
            let b = a + 1 + next() % 3_000;
            let got = p.minmax(a, b);
            let lo_blk = a / BASE_BLOCK * BASE_BLOCK;
            let hi_blk = ((b.min(10_000) - 1) / BASE_BLOCK + 1) * BASE_BLOCK;
            assert_eq!(got, brute(&s, 2, lo_blk, hi_blk), "{a}..{b} expands to {lo_blk}..{hi_blk}");
            // And it never UNDERSTATES the exact range.
            let exact = brute(&s, 2, a, b);
            assert!(got.0 <= exact.0 && got.1 >= exact.1, "{a}..{b}: {got:?} vs exact {exact:?}");
        }
    }

    #[test]
    fn every_level_agrees_with_the_finest() {
        let s = signal(5_003);
        let p = Peaks::build_with(&s, 2, 48_000, 8);
        for (k, level) in p.levels.iter().enumerate().skip(1) {
            let span = 8usize << k;
            for (i, blk) in level.iter().enumerate() {
                let from = i * span;
                let to = ((i + 1) * span).min(5_003);
                assert_eq!(finish(*blk), brute(&s, 2, from, to), "level {k} block {i}");
            }
        }
        assert_eq!(p.levels.last().map(Vec::len), Some(1), "the top is one block");
    }

    #[test]
    fn nothing_is_a_flat_line_and_never_leaks_into_a_neighbour() {
        // A block of NaN beside audio that is entirely above zero: the old
        // (0, 0) seed would have dragged the minimum to zero.
        let mut s = vec![0.5f32; 64 * 2];
        for v in &mut s[..16 * 2] {
            *v = f32::NAN;
        }
        let p = Peaks::build(&s, 2, 48_000);
        assert_eq!(p.minmax(0, 16), (0.0, 0.0), "a wholly non-finite block is flat");
        assert_eq!(p.minmax(0, 64), (0.5, 0.5), "and it does not pull the fold to zero");
        assert_eq!(p.minmax(64, 128), (0.0, 0.0), "past the end is flat");
        assert_eq!(p.minmax(10, 10), (0.0, 0.0), "empty is flat");
        let empty = Peaks::build(&[], 2, 48_000);
        assert_eq!(empty.minmax(0, 100), (0.0, 0.0));
        assert_eq!(empty.envelope(10), Vec::<f32>::new());
        assert_eq!(empty.seconds(), 0.0);
    }

    #[test]
    fn the_answer_is_clamped_to_the_drawable_range() {
        let s = vec![3.0f32, -2.5, 3.0, -2.5];
        let p = Peaks::build(&s, 2, 48_000);
        assert_eq!(p.minmax(0, 2), (-1.0, 1.0));
    }

    /// A block that never comes back inside the range is a bar on the rail,
    /// not an inverted pair: `finish` used to clamp the minimum from below
    /// and the maximum from above only, so all-above-one answered
    /// `(1.5, 1.0)` and all-below-minus-one `(-1.0, -1.5)`.
    #[test]
    fn a_block_wholly_outside_the_range_pins_to_the_rail_and_stays_ordered() {
        let hot = vec![1.5f32; 64 * 2];
        let p = Peaks::build(&hot, 2, 48_000);
        assert_eq!(p.minmax(0, 16), (1.0, 1.0), "all above one");
        assert_eq!(p.minmax(0, 64), (1.0, 1.0), "all above one, every level");
        let cold = vec![-1.5f32; 64 * 2];
        let p = Peaks::build(&cold, 2, 48_000);
        assert_eq!(p.minmax(0, 16), (-1.0, -1.0), "all below minus one");
        assert_eq!(p.minmax(0, 64), (-1.0, -1.0), "all below minus one, every level");
        // And a mixed file keeps lo <= hi under every query it can be asked.
        let mixed: Vec<f32> = (0..2_000).map(|f| if f % 700 < 350 { 2.0 } else { -3.0 }).collect();
        let p = Peaks::build(&mixed, 1, 48_000);
        for (a, b) in [(0, 16), (0, 2_000), (340, 360), (700, 1_050), (1_999, 2_000)] {
            let (lo, hi) = p.minmax(a, b);
            assert!(lo <= hi, "{a}..{b}: lo {lo} above hi {hi}");
            assert!((-1.0..=1.0).contains(&lo) && (-1.0..=1.0).contains(&hi), "{a}..{b}: {lo} {hi}");
        }
    }

    #[test]
    fn mono_and_stereo_fold_the_same_way() {
        let mono: Vec<f32> = (0..1_000).map(|f| (f as f32 * 0.01).sin()).collect();
        let stereo: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();
        let pm = Peaks::build(&mono, 1, 44_100);
        let ps = Peaks::build(&stereo, 2, 44_100);
        assert_eq!(pm.frames(), 1_000);
        assert_eq!(ps.frames(), 1_000);
        for (a, b) in [(0, 1_000), (3, 700), (999, 1_000)] {
            assert_eq!(pm.minmax(a, b), ps.minmax(a, b));
        }
    }

    #[test]
    fn seconds_round_to_frames_and_clamp_to_the_file() {
        let p = Peaks::build(&signal(48_000), 2, 48_000);
        assert_eq!(p.seconds(), 1.0);
        assert_eq!(p.frame_at(-1.0), 0);
        assert_eq!(p.frame_at(f64::NAN), 0);
        assert_eq!(p.frame_at(0.5), 24_000);
        assert_eq!(p.frame_at(0.50001), 24_000);
        assert_eq!(p.frame_at(9.0), 48_000);
        // A span narrower than a frame still reads the frame it stands on.
        assert_ne!(p.minmax_s(0.25, 0.25), (0.0, 0.0));
        assert_eq!(p.minmax_s(0.0, 1.0), p.minmax(0, 48_000));
    }

    #[test]
    fn the_envelope_is_the_old_peak_picture() {
        let s = signal(20_000);
        let p = Peaks::build(&s, 2, 48_000);
        let env = p.envelope(100);
        assert_eq!(env.len(), 100);
        for (i, e) in env.iter().enumerate() {
            // Bucket edges that miss a block boundary are widened to it, as
            // the module doc says; the old picture read the exact range.
            let a = i * 20_000 / 100 / BASE_BLOCK * BASE_BLOCK;
            let b = (((i + 1) * 20_000 / 100 - 1) / BASE_BLOCK + 1) * BASE_BLOCK;
            let (lo, hi) = brute(&s, 2, a, b);
            let want = lo.abs().max(hi.abs()).min(1.0);
            assert!((e - want).abs() < 1e-6, "bucket {i}: {e} vs {want}");
            assert!((0.0..=1.0).contains(e));
        }
    }

    #[test]
    fn the_picture_is_an_eighth_of_the_audio() {
        let frames = 48_000 * 60;
        let p = Peaks::build(&vec![0.1f32; frames * 2], 2, 48_000);
        let audio = frames * 2 * 4;
        assert!(p.bytes() < audio / 7, "{} bytes for {} of audio", p.bytes(), audio);
        assert!(p.bytes() > audio / 9);
    }
}
