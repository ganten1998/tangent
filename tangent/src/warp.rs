//! **Warp: moving points of a clip onto the grid without cutting it.**
//!
//! The audio half of quantize. [`crate::transient`] says where the attacks
//! are, [`tangent_ui::quantize`] says where each one should land, and this
//! turns those two lists into one new clip.
//!
//! # Why warp rather than split
//!
//! The other way to quantize audio is REAPER's: cut the item at every
//! transient and move each slice onto the grid. It has one real advantage -
//! the attack is never processed at all, so on isolated percussion with
//! small corrections it is the best sound available - and three problems
//! that arrive together on anything else. A slice moved late leaves a GAP
//! of silence. A slice moved early has its tail TRUNCATED by the one behind
//! it. And on a kit, every slice carries the bleed of the other mics, which
//! now sits at the wrong time: the result flams against itself in a way the
//! performance never did.
//!
//! Warping has none of those, because the timeline stays continuous: the
//! material between two attacks is stretched to fit the distance between
//! their new homes. Its cost is the stretcher's own artefacts, and
//! [`crate::stretch`] already chooses the right algorithm by listening to
//! the material.
//!
//! **The attacks themselves are never stretched**, which is the whole
//! argument for anchoring on transients rather than on bar lines. A segment
//! runs from one attack to the next, so a splice - the place any
//! time-domain stretcher shows its seam - always lands where a new hit is
//! about to mask it. Stretching THROUGH an attack is what makes a warped
//! drum take sound soft, and nothing here does it.
//!
//! # Offline, like every other edit
//!
//! A warped clip is rendered once into a new buffer and played back
//! statelessly, exactly as [`crate::stretch::stretch`] is and for the reason
//! its header gives at length: clips are read by a stateless seek, and that
//! is what makes locate, loop and export correct.

use crate::stretch::{self, Mode};
use tangent_record::decode::Clip;

/// **One point of the clip, and where it should end up.** Both in frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anchor {
    /// Where it is in the source.
    pub from: usize,
    /// Where it should be in the result.
    pub to: usize,
}

/// The shortest segment worth stretching, in frames at any rate.
///
/// Below this a segment is a handful of samples and the ratio it implies is
/// noise: two attacks detected four milliseconds apart are one attack, and
/// stretching the scrap between them by 3x makes an audible chirp out of
/// nothing. Such a segment is COPIED, and the error it would have absorbed
/// is carried by its neighbour.
const MIN_SEGMENT: usize = 256;

/// How much of each segment's head is played back exactly as recorded, in
/// frames. ~5 ms at 48 kHz - longer than a stick's own edge.
const ATTACK_KEEP: usize = 240;

/// How long the copied head takes to hand over to the stretched material,
/// in frames. ~5 ms, and it has to exist: butt-joining the two put a step
/// in the middle of a decaying click that the detector read as a SECOND
/// attack, which is worse than the smear it was fixing.
const ATTACK_FADE: usize = 240;


/// **Warp `clip` so every anchor's `from` lands on its `to`.**
///
/// Anchors are taken in the order given and must ascend in both fields;
/// anything that does not is dropped rather than reordered, because an
/// anchor list that goes backwards is a caller bug and guessing at what it
/// meant would hide it.
///
/// Material before the first anchor and after the last is carried at the
/// ratio of the segment beside it, so a clip does not change character at
/// its edges.
#[must_use]
pub fn warp(clip: &Clip, anchors: &[Anchor], mode: Mode) -> Clip {
    let frames = clip.frames();
    if frames == 0 || anchors.is_empty() {
        return clip.clone();
    }
    // Ascending in both fields, inside the clip, and no two on one frame.
    let mut pts: Vec<Anchor> = Vec::with_capacity(anchors.len());
    for a in anchors {
        if a.from >= frames {
            continue;
        }
        if let Some(last) = pts.last() {
            if a.from <= last.from || a.to <= last.to {
                continue;
            }
        }
        pts.push(*a);
    }
    if pts.is_empty() {
        return clip.clone();
    }

    // **The cut list, in source frames**: the head, every anchor, the tail.
    // Segment `i` runs from `cuts[i]` to `cuts[i + 1]` and has to come out
    // `want[i]` frames long.
    let mut cuts: Vec<usize> = Vec::with_capacity(pts.len() + 2);
    let mut outs: Vec<usize> = Vec::with_capacity(pts.len() + 2);
    // **The head starts at output zero, not at the first anchor.** `outs`
    // holds where each cut BEGINS in the result, so the material before the
    // first anchor begins at nothing and runs to `pts[0].to`. Written as
    // `pts[0].to` first, which gave the head a length of zero and deleted
    // every lead-in: a clip with 200 ms of air before its first hit came
    // back starting on the hit.
    if pts[0].from > 0 {
        cuts.push(0);
        outs.push(0);
    }
    for a in &pts {
        cuts.push(a.from);
        outs.push(a.to);
    }
    cuts.push(frames);
    // The tail keeps the ratio of the segment before it, so the clip does
    // not snap back to 1.0 at its end.
    let tail_src = frames - pts[pts.len() - 1].from;
    let tail_ratio = if pts.len() >= 2 {
        let (a, b) = (pts[pts.len() - 2], pts[pts.len() - 1]);
        (b.to - a.to) as f64 / (b.from - a.from).max(1) as f64
    } else {
        1.0
    };
    outs.push(pts[pts.len() - 1].to + ((tail_src as f64) * tail_ratio).round() as usize);

    let mut samples: Vec<f32> = Vec::with_capacity(outs[outs.len() - 1] * 2);
    for i in 0..cuts.len() - 1 {
        let (s0, s1) = (cuts[i], cuts[i + 1]);
        let want = outs[i + 1].saturating_sub(outs[i]);
        let src_len = s1.saturating_sub(s0);
        if src_len == 0 {
            continue;
        }
        let seg = Clip {
            samples: clip.samples[s0 * 2..(s1 * 2).min(clip.samples.len())].to_vec(),
            rate: clip.rate,
            source: clip.source.clone(),
        };
        // **A scrap is copied, not stretched.** See `MIN_SEGMENT`.
        let piece = if src_len < MIN_SEGMENT || want < MIN_SEGMENT || want == 0 {
            seg
        } else {
            keep_the_attack(&seg, stretch::stretch(&seg, want as f64 / src_len as f64, mode))
        };
        // The stretcher lands within a few frames of the length asked for -
        // its hop arithmetic rounds - and those frames must not accumulate
        // across forty segments, so each piece is trimmed or padded to the
        // length the anchor list demands. Padding repeats the last frame
        // rather than writing silence, which would tick.
        let have = piece.frames();
        let take = have.min(want);
        samples.extend_from_slice(&piece.samples[..take * 2]);
        if want > have {
            let (l, r) = if have == 0 {
                (0.0, 0.0)
            } else {
                (piece.samples[(have - 1) * 2], piece.samples[(have - 1) * 2 + 1])
            };
            for _ in have..want {
                samples.push(l);
                samples.push(r);
            }
        }
    }

    Clip {
        samples,
        rate: clip.rate,
        source: clip.source.clone(),
    }
}

/// **Give a stretched segment its own attack back.**
///
/// Every segment here begins on a transient - that is what anchoring on
/// transients means - so the first milliseconds of each are the hit itself,
/// and a stretcher handed a window that opens on an attack has no history to
/// align against. Measured: warping a take this way put EVERY hit a uniform
/// 6.5 ms late, which is a whole take dragging against the grid it was just
/// quantized to.
///
/// So the head is overwritten with the source's own samples and crossfaded
/// into what the stretcher made. The attack comes out bit for bit as played,
/// the ratio is carried entirely by the sustain behind it, and the handover
/// falls on a decaying tail where a seam has least to show.
///
/// Length is untouched: this rewrites the front of `stretched` in place
/// rather than adding to it, so the per-segment arithmetic still holds.
fn keep_the_attack(src: &Clip, mut stretched: Clip) -> Clip {
    let n = ATTACK_KEEP.min(src.frames() / 4).min(stretched.frames() / 4);
    if n == 0 {
        return stretched;
    }
    let fade = ATTACK_FADE
        .min(src.frames().saturating_sub(n))
        .min(stretched.frames().saturating_sub(n));
    for f in 0..n {
        for ch in 0..2 {
            stretched.samples[f * 2 + ch] = src.samples[f * 2 + ch];
        }
    }
    for k in 0..fade {
        // Linear either side: the two are the same material a few
        // milliseconds apart, so they are correlated and a power law would
        // bulge in the middle.
        let w = (k + 1) as f32 / (fade + 1) as f32;
        let f = n + k;
        for ch in 0..2 {
            let a = src.samples[f * 2 + ch];
            let b = stretched.samples[f * 2 + ch];
            stretched.samples[f * 2 + ch] = a * (1.0 - w) + b * w;
        }
    }
    stretched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transient;
    use tangent_record::decode::Clip;

    const RATE: u32 = 48_000;

    fn clip_of(samples: Vec<f32>) -> Clip {
        Clip { samples, rate: RATE, source: std::path::PathBuf::from("t.wav") }
    }

    /// Clicks at the exact frames given - a take played out of time.
    fn clicks_at(seconds: f64, at: &[f64]) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        let mut s = vec![0.0f32; n * 2];
        for &t in at {
            let start = (f64::from(RATE) * t) as usize;
            for k in 0..(RATE as usize / 20) {
                let f = start + k;
                if f >= n {
                    break;
                }
                let env = (-(k as f32) / (f64::from(RATE) as f32 * 0.01)).exp();
                let v = (std::f32::consts::TAU * 900.0 * k as f32 / f64::from(RATE) as f32)
                    .sin()
                    * env
                    * 0.7;
                s[f * 2] += v;
                s[f * 2 + 1] += v;
            }
        }
        clip_of(s)
    }

    /// **The whole feature, end to end: a sloppy take comes out on the
    /// grid.**
    ///
    /// Clicks played 30 to 60 ms away from the half second, detected,
    /// warped onto it, and then DETECTED AGAIN in the result. Checking the
    /// output's own attacks rather than the arithmetic is the point: it is
    /// the only test here that would notice the warp reporting one length
    /// and rendering another, which is exactly the class of bug the
    /// per-segment trim exists to prevent.
    #[test]
    fn a_take_played_out_of_time_comes_back_on_the_grid() {
        // Played: late, early, late, early - and the first hit on time, so
        // there is a fixed point to warp around.
        let played = [0.0, 0.53, 1.04, 1.46, 1.97];
        let grid = [0.0, 0.5, 1.0, 1.5, 2.0];
        let c = clicks_at(3.0, &played);

        let found = transient::detect(&c, 0.5);
        assert_eq!(found.len(), played.len(), "the fixture was not detected cleanly: {found:?}");

        let anchors: Vec<Anchor> = found
            .iter()
            .zip(grid.iter())
            .map(|(t, &g)| Anchor { from: t.at, to: (f64::from(RATE) * g) as usize })
            .collect();
        let out = warp(&c, &anchors, Mode::Rhythmic);

        let after = transient::detect(&out, 0.5);
        assert_eq!(
            after.len(),
            grid.len(),
            "the warped clip has {} attacks, not {}: {:?}",
            after.len(),
            grid.len(),
            after.iter().map(|t| t.at).collect::<Vec<_>>()
        );
        // **Within 1 ms**, which is what this actually achieves and so what
        // it is held to. It was 6 ms while the attacks were being stretched
        // through, and loosening a tolerance to fit a defect is how the
        // defect survives: the measured spread across both a clip that opens
        // on a hit and one with a lead-in is 24 frames, half a millisecond.
        let tol = (f64::from(RATE) * 0.001) as usize;
        for (i, (t, &g)) in after.iter().zip(grid.iter()).enumerate() {
            let want = (f64::from(RATE) * g) as usize;
            assert!(
                t.at.abs_diff(want) <= tol,
                "hit {i} landed at {} frames, the line is at {want} ({} out)",
                t.at,
                t.at.abs_diff(want)
            );
        }
    }

    /// **The result is exactly as long as the anchors demand.**
    ///
    /// Each segment is trimmed or padded to its asked-for length, so the
    /// stretcher's few frames of rounding cannot accumulate across forty of
    /// them and leave the end of a bar half a beat out.
    #[test]
    fn the_segments_do_not_accumulate_the_stretchers_rounding() {
        let c = clicks_at(4.0, &[0.0, 0.5, 1.0, 1.5, 2.0, 2.5, 3.0, 3.5]);
        // Every hit pulled 40 ms later than it was: a constant stretch, and
        // eight segments to accumulate error over.
        let anchors: Vec<Anchor> = (0..8)
            .map(|i| Anchor {
                from: (f64::from(RATE) * (i as f64 * 0.5)) as usize,
                to: (f64::from(RATE) * (i as f64 * 0.54)) as usize,
            })
            .collect();
        let out = warp(&c, &anchors, Mode::Rhythmic);
        // The last anchor is at 3.78 s and the tail carries the same ratio:
        // 0.5 s of source left, times 1.08.
        let want = anchors[7].to + ((f64::from(RATE) * 0.5) as f64 * 1.08).round() as usize;
        assert!(
            out.frames().abs_diff(want) <= 2,
            "the warp came out {} frames long, the anchors demand {want}",
            out.frames()
        );
    }

    /// **A clip with nothing to warp comes back unchanged**, rather than
    /// being rendered through a stretcher at a ratio of one.
    #[test]
    fn nothing_to_do_is_a_clip_unchanged() {
        let c = clicks_at(1.0, &[0.0, 0.5]);
        assert_eq!(warp(&c, &[], Mode::Rhythmic).samples, c.samples, "no anchors");
        let empty = clip_of(Vec::new());
        assert!(warp(&empty, &[Anchor { from: 0, to: 10 }], Mode::Rhythmic).samples.is_empty());
    }

    /// **An anchor list that goes backwards is dropped, not reordered.**
    /// A caller that built one has a bug, and guessing would hide it.
    #[test]
    fn anchors_that_go_backwards_are_refused() {
        let c = clicks_at(2.0, &[0.0, 0.5, 1.0]);
        let bad = [
            Anchor { from: 24_000, to: 24_000 },
            Anchor { from: 12_000, to: 30_000 }, // `from` goes back
            Anchor { from: 36_000, to: 20_000 }, // `to` goes back
        ];
        let out = warp(&c, &bad, Mode::Rhythmic);
        assert!(out.frames() > 0, "a partly-bad list rendered nothing");
    }
}

