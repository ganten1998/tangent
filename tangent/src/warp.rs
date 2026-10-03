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
//! the material. (Not yet for a multi-mic kit: the host detects and warps
//! each take on its own, so one hit moves by a different amount in each
//! mic and the bleed flams against itself after all.)
//!
//! **An anchored attack is not stretched**, which is the whole argument for
//! anchoring on transients rather than on bar lines. A segment runs from
//! one anchor to the next and a stretched segment's first few milliseconds
//! are the source's own (`keep_the_attack`), so a splice - the place any
//! time-domain stretcher shows its seam - lands where a new hit is about to
//! mask it. Stretching THROUGH an attack is what makes a warped drum take
//! sound soft, and it still happens to what is not an anchor: a hit the
//! detector missed, the later of two hits the quantize sent to one grid
//! line (dropped as an anchor and carried inside the segment), and whatever
//! of a long attack outlasts the copied head.
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

/// **How a segment gets from where it was played to where it belongs.**
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Fit {
    /// **Listen to the material and decide.** The default, and what nearly
    /// every clip should stay on: see [`Fit::pick`].
    #[default]
    Auto,
    /// Stretch the segment to fill the distance. No gap, ever, and the cost
    /// is whatever the stretcher does to a sustain.
    Stretch,
    /// **Move the segment and let it end where it ends** - REAPER's dynamic
    /// split, as a mode rather than as a different feature.
    ///
    /// Nothing is processed at all, so the hit comes out bit for bit as it
    /// was played. A segment that has to move later leaves a gap behind it,
    /// which the segment before fills by holding its last frame (a split
    /// would leave silence there); one that moves earlier is cut off by the
    /// hit in front. On isolated percussion with small corrections that is
    /// the best sound available, and on anything sustained it is a
    /// disaster - which is why [`Fit::pick`] asks the material rather than
    /// offering a coin toss.
    Slice,
}

impl Fit {
    /// Every fit, in the order a menu should offer them.
    pub const ALL: [Fit; 3] = [Fit::Auto, Fit::Stretch, Fit::Slice];

    /// How it is written to the document, and read back. **Append-only.**
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Fit::Auto => "auto",
            Fit::Stretch => "stretch",
            Fit::Slice => "slice",
        }
    }

    #[must_use]
    pub fn from_key(key: &str) -> Option<Fit> {
        Fit::ALL.into_iter().find(|m| m.key() == key)
    }

    /// **Which one this material wants.**
    ///
    /// The same question [`stretch::Mode::pick`] asks and the same measure
    /// it asks it of: [`stretch::percussiveness`], the crest factor of the
    /// envelope. That is not a coincidence worth hiding - "does this have
    /// edges in it" decides whether a splice is masked or exposed, and it is
    /// the only thing that separates material a slice flatters from material
    /// a slice ruins.
    ///
    /// The threshold is deliberately HIGHER than the stretcher's. Picking
    /// the wrong stretch algorithm costs some smearing; picking a slice for
    /// sustained material costs silence where the music was. When the answer
    /// is not obvious, stretch.
    #[must_use]
    pub fn pick(self, clip: &Clip) -> Fit {
        if self != Fit::Auto {
            return self;
        }
        if stretch::percussiveness(clip) >= SLICE_AT {
            Fit::Slice
        } else {
            Fit::Stretch
        }
    }
}

/// Above this, material is percussive enough that a slice is safe. Well
/// above the stretcher's own 0.30: see [`Fit::pick`].
const SLICE_AT: f32 = 0.55;

/// The shortest segment worth stretching, in frames at any rate.
///
/// Below this a segment is a handful of samples and the ratio it implies is
/// noise: two attacks detected four milliseconds apart are one attack, and
/// stretching the scrap between them by 3x makes an audible chirp out of
/// nothing. Such a segment is COPIED, then trimmed or padded to its own slot
/// like every other (the pad holds its last frame); its neighbours are
/// untouched.
const MIN_SEGMENT: usize = 256;

/// How much of each segment's head is played back exactly as recorded, in
/// frames. ~5 ms at 48 kHz - longer than a stick's own edge.
const ATTACK_KEEP: usize = 240;

/// How long the copied head takes to hand over to the stretched material,
/// in frames. ~5 ms, and it has to exist: butt-joining the two put a step
/// in the middle of a decaying click that the detector read as a SECOND
/// attack, which is worse than the smear it was fixing.
const ATTACK_FADE: usize = 240;


#[must_use]
/// **The anchors this map can actually be rendered from**: ascending in both
/// fields, inside the clip, and no two on one frame.
///
/// Its own function because [`projected_frames`] has to see exactly the list
/// [`warp`] renders from. A length predicted off the raw anchors and a render
/// made from the cleaned ones would disagree by however many the clean-up
/// dropped, which is the kind of gap that shows up as a clip drawn one
/// length and played another.
fn usable(frames: usize, anchors: &[Anchor]) -> Vec<Anchor> {
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
    pts
}

/// **How long the render will be, without making it.**
///
/// The warp is rendered off the UI thread now, and the frame that asks for it
/// still has to draw the clip and count its seconds - so the length is
/// predicted here and the render is checked against it (see
/// `a_predicted_length_is_the_length_the_render_comes_out`). Every segment is
/// trimmed or padded to exactly the length the anchor list demands, so the
/// total is the last output cut and nothing else: the last anchor's landing
/// plus the tail, carried at its own length (see [`carry_tail`]).
pub fn projected_frames(frames: usize, anchors: &[Anchor]) -> usize {
    if frames == 0 || anchors.is_empty() {
        return frames;
    }
    let pts = usable(frames, anchors);
    let Some(last) = pts.last().copied() else {
        return frames;
    };
    carry_tail(last, frames)
}

/// **Where a frame past the last anchor lands: carried, never stretched.**
///
/// Source frame `at`, at or past `last.from`, comes out `at - last.from`
/// after `last.to`: the rest of the take moves with the last anchor and
/// plays at its own length. It was carried at the ratio of the last segment,
/// "so the clip does not change character at its end", which laid one
/// segment's correction over everything after it: two hits 30 ms apart
/// pulled 125 ms apart stretched the whole tail 4.17x (the stretcher clamps
/// at 4x and holds its last frame for the rest), and under [`Fit::Slice`] a
/// last segment pulled together cut the tail off. Nothing past the last
/// attack was asked to move. The one spelling, for [`warp`],
/// [`projected_frames`] and [`project_frame`].
fn carry_tail(last: Anchor, at: usize) -> usize {
    last.to + at.saturating_sub(last.from)
}

/// **Where one source frame ends up in the render**, without making it.
///
/// The same piecewise map [`warp`] renders through: linear between the
/// anchors that survive the clean-up, and the tail carried at its own length
/// (see [`carry_tail`]). The quantize window's marks need it - an attack
/// is FOUND in the file and DRAWN over the warped audio, so a clip that has
/// already been quantized would otherwise wear ticks a whole correction
/// away from the hits they name.
///
/// Identity when there is no usable map, which is what makes the caller's
/// unwarped case free.
#[must_use]
pub fn project_frame(frames: usize, anchors: &[Anchor], at: usize) -> usize {
    if frames == 0 || anchors.is_empty() {
        return at;
    }
    let pts = usable(frames, anchors);
    let (Some(&first), Some(&last)) = (pts.first(), pts.last()) else {
        return at;
    };
    // Before the first anchor: the head is squeezed or stretched into the
    // room in front of it, exactly as the render lays it out.
    if at <= first.from {
        if first.from == 0 {
            return first.to.min(projected_frames(frames, anchors));
        }
        let f = at as f64 / first.from as f64;
        return (f * first.to as f64).round() as usize;
    }
    // Between two anchors.
    for pair in pts.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if at <= b.from {
            let span = (b.from - a.from).max(1) as f64;
            let f = (at - a.from) as f64 / span;
            return (a.to as f64 + f * (b.to - a.to) as f64).round() as usize;
        }
    }
    // The tail, carried at its own length.
    carry_tail(last, at)
}

/// **Warp `clip` so every anchor's `from` lands on its `to`.**
///
/// The anchors pass through [`usable`] first - ascending in both fields,
/// inside the clip, none on one frame - and anything else is dropped rather
/// than reordered: a list that goes backwards is a caller bug.
///
/// **What a caller may rely on** (and [`projected_frames`], [`project_frame`]
/// and the host's stand-in all lean on):
/// - The result is EXACTLY [`projected_frames`]`(clip.frames(), anchors)`
///   frames long under every [`Fit`]: each segment is trimmed or padded to
///   the length the anchor list demands, so nothing accumulates.
/// - Every surviving anchor's `from` comes out at its `to`, to the frame.
/// - On a STRETCHED segment the first [`ATTACK_KEEP`] frames (a quarter of
///   the segment, when that is shorter) are the source's own, crossfaded
///   over [`ATTACK_FADE`] into the stretcher's output. A segment whose source
///   or slot is under [`MIN_SEGMENT`], and every segment under
///   [`Fit::Slice`], is copied, then trimmed or padded with a hold of its
///   last frame.
/// - The fit and the stretch ALGORITHM are each resolved once for the whole
///   clip ([`Fit::pick`], [`stretch::Mode::for_warp`]), and the algorithm is
///   never a tape resample, so no segment's pitch moves with its correction.
/// - The head is mapped from `0..first.from` onto `0..first.to`; a first
///   anchor at frame zero that moves later gets silence in front of it. The
///   tail is carried at its own length, moved with the last anchor (see
///   [`carry_tail`]), so under every fit it comes out as recorded.
/// - The anchors and `clip` are in the SAME frame rate. A map is frames at
///   the rate it was made at, and nothing here rescales it.
pub fn warp(clip: &Clip, anchors: &[Anchor], mode: Mode, fit: Fit) -> Clip {
    let frames = clip.frames();
    if frames == 0 || anchors.is_empty() {
        return clip.clone();
    }
    let pts = usable(frames, anchors);
    if pts.is_empty() {
        return clip.clone();
    }
    // **Asked ONCE, of the whole clip.** Per segment it would answer
    // differently for the quiet bars than for the loud ones and the take
    // would change character halfway through.
    let fit = fit.pick(clip);
    // **And the algorithm once too, and never a tape resample.** Picked per
    // segment, `Mode::Auto` resampled every segment within 2% of its slot,
    // which moves its pitch by its own correction: each note of a bass take
    // came back detuned by a different amount. See `Mode::for_warp`.
    let mode = mode.for_warp(clip);

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
    // The tail is carried at its own length. See `carry_tail`.
    outs.push(carry_tail(pts[pts.len() - 1], frames));

    let mut samples: Vec<f32> = Vec::with_capacity(outs[outs.len() - 1] * 2);
    // **A first attack that is at frame zero and has to move later needs
    // silence in front of it**, because there is no material before the
    // start of a file to stretch into the gap.
    //
    // Missed entirely at first: the head above only runs when there IS
    // material before the first anchor, so a clip that opened on its
    // downbeat had the whole shift quietly dropped and came out early by
    // exactly the amount it was asked to move. Found by a slice test
    // expecting the audio to have moved and getting back the source.
    if pts[0].from == 0 && pts[0].to > 0 {
        samples.resize(pts[0].to * 2, 0.0);
    }
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
        } else if fit == Fit::Slice {
            // **Moved, not fitted.** The segment is played as recorded and
            // the trim or the pad below does the rest: short of its slot it
            // is held on its last frame where a split would leave silence,
            // long for it it is cut off by the hit in front, and either way
            // nothing has been through an algorithm. See `Fit::Slice`.
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

    /// The detector's two halves as one call. See
    /// `transient::tests::detect` for why production has no such door.
    fn detect(clip: &Clip, sensitivity: f32) -> Vec<transient::Transient> {
        transient::analyse(clip).map_or_else(Vec::new, |a| transient::pick(&a, sensitivity))
    }

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

    /// **A projected frame is where the render actually puts it.**
    ///
    /// The quantize window's marks are found in the FILE and drawn over
    /// what the clip plays, which are two timelines the moment a warp map
    /// exists - so an already-quantized take would wear its ticks a whole
    /// correction away from the hits they name. This is the map that fixes
    /// that, checked against the render rather than against itself: each
    /// anchor is asked where it goes and has to answer with its own target,
    /// and the audio is asked where its attacks came out.
    #[test]
    fn a_projected_frame_is_where_the_render_put_it() {
        let a = |from: f64, to: f64| Anchor {
            from: (f64::from(RATE) * from) as usize,
            to: (f64::from(RATE) * to) as usize,
        };
        let src = clicks_at(2.0, &[0.05, 0.47, 0.98, 1.52]);
        let frames = src.frames();

        // No map is the identity, which is what makes the unwarped case free.
        for at in [0usize, 1000, frames / 2, frames - 1] {
            assert_eq!(project_frame(frames, &[], at), at, "an empty map moved a frame");
        }

        // **The three pieces, each on its own.** A map whose first anchor
        // MOVES carries the head with it, the middle is linear between
        // anchors, and the tail is carried at its own length - and each of
        // the three is a branch that can be written as the identity without
        // the rest noticing.
        let three = vec![a(0.40, 0.20), a(0.80, 0.60)];
        // Head: half of the way to the first anchor is half of the way to
        // its target, so 0.20 s of source lands at 0.10 s.
        let head = project_frame(frames, &three, (f64::from(RATE) * 0.20) as usize);
        assert!(
            head.abs_diff((f64::from(RATE) * 0.10) as usize) <= 2,
            "the head was not carried: 0.20 s projected to {} s",
            head as f64 / f64::from(RATE)
        );
        // Middle: halfway between the anchors is halfway between targets.
        let mid = project_frame(frames, &three, (f64::from(RATE) * 0.60) as usize);
        assert!(
            mid.abs_diff((f64::from(RATE) * 0.40) as usize) <= 2,
            "the middle is not linear: 0.60 s projected to {} s",
            mid as f64 / f64::from(RATE)
        );
        // **A first anchor AT frame zero**, which is its own branch: there
        // is no material in front of it to carry, so the render writes
        // silence and frame zero simply IS the target. Without this the
        // head of a clip that opens on its downbeat projects as itself and
        // its first tick sits the whole shift early.
        let opens = vec![a(0.0, 0.10), a(0.98, 1.00)];
        let at_zero = project_frame(frames, &opens, 0);
        assert!(
            at_zero.abs_diff((f64::from(RATE) * 0.10) as usize) <= 2,
            "a clip opening on its downbeat projected frame 0 to {at_zero}"
        );

        // Tail: carried at its own length, so 0.20 s past the last anchor
        // is 0.20 s past its target.
        let tail = project_frame(frames, &three, (f64::from(RATE) * 1.00) as usize);
        assert!(
            tail.abs_diff((f64::from(RATE) * 0.80) as usize) <= 2,
            "the tail was not carried at its own length: 1.00 s projected to {} s",
            tail as f64 / f64::from(RATE)
        );

        let anchors = vec![a(0.05, 0.05), a(0.47, 0.50), a(0.98, 1.00), a(1.52, 1.50)];
        // **Every anchor lands on its own target.** That is the whole
        // property: it is why a quantized take's found tick and its target
        // tick come out on top of each other.
        for (n, an) in anchors.iter().enumerate() {
            let got = project_frame(frames, &anchors, an.from);
            assert!(
                got.abs_diff(an.to) <= 1,
                "anchor {n} projected to {got}, not to its own target {}",
                an.to
            );
        }
        // Monotone, and inside the render.
        let out_len = projected_frames(frames, &anchors);
        let mut last = 0usize;
        for k in 0..200 {
            let at = k * frames / 200;
            let got = project_frame(frames, &anchors, at);
            assert!(got >= last, "the map went backwards at {at}");
            assert!(got <= out_len + 2, "{at} projected past the render: {got} > {out_len}");
            last = got;
        }

        // **And the audio agrees.** Warp the clip, detect the result's own
        // attacks, and each has to be where this says it would be. Checking
        // the render rather than the arithmetic is the point - the two are
        // written separately and only this keeps them together.
        let out = warp(&src, &anchors, Mode::Tape, Fit::Stretch);
        let heard: Vec<usize> = detect(&out, 0.5).iter().map(|t| t.at).collect();
        assert!(heard.len() >= 3, "the warped fixture lost its clicks: {heard:?}");
        let near = |want: usize| {
            heard
                .iter()
                .any(|&h| h.abs_diff(want) < (f64::from(RATE) * 0.020) as usize)
        };
        for an in &anchors {
            let want = project_frame(frames, &anchors, an.from);
            assert!(
                near(want),
                "nothing was heard at the projected {want} (heard {heard:?})"
            );
        }
    }

    /// **A predicted length is the length the render comes out.**
    ///
    /// The warp is rendered off this thread now, so the frame that asks for
    /// it has to draw the clip and count its seconds from a number nobody
    /// has made yet. `projected_frames` is that number, and the only thing
    /// stopping it drifting from the renderer is this: every shape the
    /// cleaner can be handed, predicted and then rendered.
    #[test]
    fn a_predicted_length_is_the_length_the_render_comes_out() {
        let src = clicks_at(2.0, &[0.05, 0.47, 0.98, 1.52]);
        let frames = src.frames();
        let a = |from: f64, to: f64| Anchor {
            from: (f64::from(RATE) * from) as usize,
            to: (f64::from(RATE) * to) as usize,
        };
        for (what, anchors) in [
            ("nothing at all", vec![]),
            ("one anchor", vec![a(0.47, 0.50)]),
            ("a plain map", vec![a(0.05, 0.05), a(0.47, 0.50), a(0.98, 1.00), a(1.52, 1.50)]),
            // Pulled EARLIER, so the tail shortens rather than grows.
            ("a map that pulls back", vec![a(0.47, 0.40), a(0.98, 0.85), a(1.52, 1.30)]),
            // The head at frame zero, which is its own branch in the render.
            ("a map that opens on the downbeat", vec![a(0.0, 0.08), a(0.98, 1.00)]),
            // Anchors the cleaner throws away: out of the clip, out of
            // order, and two on one frame. The prediction has to see the
            // same survivors the render does.
            (
                "a map the cleaner has to fix",
                vec![
                    a(0.47, 0.50),
                    a(0.40, 0.60),
                    a(0.47, 0.70),
                    a(0.98, 1.00),
                    a(9.90, 9.90),
                ],
            ),
        ] {
            for fit in [Fit::Stretch, Fit::Slice] {
                let want = projected_frames(frames, &anchors);
                let got = warp(&src, &anchors, Mode::Tape, fit).frames();
                assert_eq!(got, want, "{what} with {fit:?}: predicted {want}, rendered {got}");
            }
        }
        // And an empty clip predicts its own nothing rather than panicking.
        assert_eq!(projected_frames(0, &[a(0.0, 0.1)]), 0);
    }

    /// **A flam at the end does not stretch the whole tail.** The tail past
    /// the last anchor was carried at the LAST segment's ratio, so two hits
    /// 30 ms apart pulled 125 ms apart stretched the rest of the take 4.17x
    /// (a 2.4 s ring came out 10 s long, the stretcher's clamp held with its
    /// last frame) - and under Slice a last segment pulled together cut the
    /// tail off.
    #[test]
    fn a_flam_at_the_end_does_not_stretch_the_whole_tail() {
        let c = clicks_at(4.0, &[0.0, 0.5, 1.0, 1.55, 1.58]);
        let at = |s: f64| (f64::from(RATE) * s) as usize;
        // The last two hits, 30 ms apart, straddle the midpoint of a 1/16 step
        // at 120 bpm and are pulled 125 ms apart: a 4.17x last segment.
        let a = [
            Anchor { from: 0, to: 0 },
            Anchor { from: at(0.5), to: at(0.5) },
            Anchor { from: at(1.0), to: at(1.0) },
            Anchor { from: at(1.55), to: at(1.5) },
            Anchor { from: at(1.58), to: at(1.625) },
        ];
        let tail_src = c.frames() - at(1.58);
        let tail_out = projected_frames(c.frames(), &a) - at(1.625);
        assert!(
            tail_out.abs_diff(tail_src) < tail_src / 20,
            "a flam stretched the tail from {tail_src} to {tail_out} frames"
        );
        // And the render plays the tail as it was, whatever the fit: past
        // the last anchor the source comes out frame for frame, moved.
        for fit in [Fit::Slice, Fit::Stretch] {
            let out = warp(&c, &a, Mode::Rhythmic, fit);
            assert_eq!(out.frames(), at(1.625) + tail_src, "{fit:?} rendered a tail of another length");
            let (src, got) = (at(1.58) + 2_000, at(1.625) + 2_000);
            assert_eq!(
                &out.samples[got * 2..(got + 4_000) * 2],
                &c.samples[src * 2..(src + 4_000) * 2],
                "{fit:?} did not carry the tail through untouched"
            );
        }
    }

    /// **A small correction does not retune the segment it moves.** `warp`
    /// resolved the fit once per clip but handed each segment its own
    /// `Mode::pick`, and under `Mode::Auto` a segment within 2% of its slot
    /// was tape-resampled: a 1.5% correction played a 220 Hz note at
    /// 216.75 Hz, a quarter of a semitone flat, and the next one sharp - a
    /// different detune on every note of a bass or vocal take.
    #[test]
    fn a_small_correction_does_not_retune_the_segment() {
        let n = RATE as usize * 4;
        let tone = clip_of(
            (0..n)
                .flat_map(|f| {
                    let v = (std::f32::consts::TAU * 220.0 * f as f32 / RATE as f32).sin() * 0.5;
                    [v, v]
                })
                .collect(),
        );
        // Two segments corrected by 1.5% either way, then two left alone.
        let a = [
            Anchor { from: 0, to: 0 },
            Anchor { from: 48_000, to: 48_720 },
            Anchor { from: 96_000, to: 96_000 },
            Anchor { from: 144_000, to: 144_000 },
        ];
        let hz_of = |out: &Clip, lo: usize, hi: usize| {
            let z = (lo..hi - 1)
                .filter(|&f| (out.samples[f * 2] <= 0.0) != (out.samples[(f + 1) * 2] <= 0.0))
                .count();
            z as f64 / 2.0 / ((hi - lo) as f64 / f64::from(RATE))
        };
        // Auto, and Tape named outright: inside a warp neither may resample.
        for mode in [Mode::Auto, Mode::Tape] {
            let out = warp(&tone, &a, mode, Fit::Stretch);
            // The measure itself, on a segment nothing moved.
            let still = hz_of(&out, 100_000, 140_000);
            assert!((still - 220.0).abs() < 1.5, "the fixture reads {still} Hz");
            // Inside each corrected segment, clear of the copied attack.
            for (lo, hi) in [(10_000, 40_000), (60_000, 90_000)] {
                let f = hz_of(&out, lo, hi);
                assert!(
                    (f - 220.0).abs() < 1.5,
                    "under {mode:?} a warped segment plays at {f:.1} Hz, not 220"
                );
            }
        }
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

        let found = detect(&c, 0.5);
        assert_eq!(found.len(), played.len(), "the fixture was not detected cleanly: {found:?}");

        let anchors: Vec<Anchor> = found
            .iter()
            .zip(grid.iter())
            .map(|(t, &g)| Anchor { from: t.at, to: (f64::from(RATE) * g) as usize })
            .collect();
        let out = warp(&c, &anchors, Mode::Rhythmic, Fit::Stretch);

        let after = detect(&out, 0.5);
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

    /// **A slice moves the hit and processes nothing.**
    ///
    /// The REAPER behaviour, as a mode. What proves it is a slice and not a
    /// stretch is that the moved segment is BIT FOR BIT the source: a
    /// stretched one at the same ratio is not.
    #[test]
    fn a_slice_moves_the_audio_without_touching_it() {
        let c = clicks_at(2.0, &[0.0, 0.5, 1.0]);
        // Every hit pulled 40 ms later: a real move, so the segments must
        // shift rather than stay put.
        let anchors: Vec<Anchor> = (0..3)
            .map(|i| Anchor {
                from: (48_000.0 * (i as f64 * 0.5)) as usize,
                to: (48_000.0 * (i as f64 * 0.5 + 0.04)) as usize,
            })
            .collect();
        let sliced = warp(&c, &anchors, Mode::Rhythmic, Fit::Slice);
        let stretched = warp(&c, &anchors, Mode::Rhythmic, Fit::Stretch);

        // The first segment of the slice is the source's own samples, from
        // its own start, untouched. Compared past the anchor so the head
        // pad cannot be what is being matched.
        let head = (48_000.0 * 0.04) as usize;
        let probe = 2_000;
        let a = &sliced.samples[head * 2..(head + probe) * 2];
        let b = &c.samples[..probe * 2];
        assert_eq!(a, b, "a slice altered the audio it moved");

        // And the two fits are genuinely different renders, or this test
        // would pass with `Fit` doing nothing at all.
        let s = &stretched.samples[head * 2..(head + probe) * 2];
        assert_ne!(a, s, "slice and stretch produced the same audio");
    }

    /// **Auto asks the material, and it is deliberately shy of slicing.**
    ///
    /// A click track is what a slice is for; a held chord is what it would
    /// ruin. Picking the wrong stretch algorithm costs some smearing,
    /// picking a slice for sustained material costs silence where the music
    /// was - so the threshold sits well above the stretcher's own.
    #[test]
    fn auto_slices_a_click_track_and_stretches_a_chord() {
        let clicky = clicks_at(4.0, &[0.0, 0.25, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75]);
        assert_eq!(Fit::Auto.pick(&clicky), Fit::Slice, "a click track did not want a slice");

        let n = (48_000.0 * 4.0) as usize;
        let held = clip_of(
            (0..n)
                .flat_map(|f| {
                    let t = f as f64 / 48_000.0;
                    let v = [220.0, 277.18, 329.63]
                        .iter()
                        .map(|hz| (std::f64::consts::TAU * hz * t).sin())
                        .sum::<f64>() as f32
                        * 0.2;
                    [v, v]
                })
                .collect(),
        );
        assert_eq!(Fit::Auto.pick(&held), Fit::Stretch, "a held chord wanted a slice");

        // A stated fit is returned unchanged - Auto is the only one that asks.
        assert_eq!(Fit::Slice.pick(&held), Fit::Slice);
        assert_eq!(Fit::Stretch.pick(&clicky), Fit::Stretch);
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
        let out = warp(&c, &anchors, Mode::Rhythmic, Fit::Stretch);
        // The last anchor is at 3.78 s and the tail is carried at its own
        // length: the 0.5 s of source left after it.
        let want = anchors[7].to + (f64::from(RATE) * 0.5) as usize;
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
        assert_eq!(warp(&c, &[], Mode::Rhythmic, Fit::Stretch).samples, c.samples, "no anchors");
        let empty = clip_of(Vec::new());
        assert!(warp(&empty, &[Anchor { from: 0, to: 10 }], Mode::Rhythmic, Fit::Stretch).samples.is_empty());
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
        let out = warp(&c, &bad, Mode::Rhythmic, Fit::Stretch);
        assert!(out.frames() > 0, "a partly-bad list rendered nothing");
    }
}

