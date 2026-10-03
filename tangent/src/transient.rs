//! **Where the attacks are**: onset detection over a decoded clip.
//!
//! The one piece of DSP the quantize work needed that this tree did not
//! already have. Audio quantize - whichever shape it takes - has to know
//! where a hit STARTS before it can move it, and a warp has to know where to
//! anchor so it stretches between attacks rather than through them.
//!
//! # Energy flux, not a transform
//!
//! The textbook detector is spectral flux: FFT every hop, sum the positive
//! change per bin. It is the right answer for polyphonic tonal material,
//! where a new note can arrive with no change in total energy at all - a
//! chord voicing moving under a held top note.
//!
//! This one works on the ENERGY envelope instead, and that is a deliberate
//! trade rather than a shortcut. What gets quantized is rhythm: drums, a
//! plucked bass, a rhythm guitar, a vocal consonant. All of them announce
//! themselves as a step up in energy, an FFT per hop over a ten-minute
//! import is a visible pause on a UI thread, and this file already pays for
//! a transform once when it stretches. If a tonal onset case ever needs to
//! be caught, the shape here - detection function, adaptive floor, peak
//! pick - takes a spectral flux function without changing anything else.
//!
//! # The three things that make it work on real material
//!
//! **An adaptive floor, not a fixed one.** A threshold that works on a
//! verse misses the intro and fires on the chorus. The floor here is the
//! local median of the detection function, which tracks the material and is
//! what stops a crescendo reading as one long onset.
//!
//! **A minimum spacing.** One drum hit is several samples of rising energy,
//! and without a refractory period a single snare reports three or four
//! onsets a millisecond apart. Nothing musical arrives faster than about
//! 30 ms, and a buzz roll that does is not something anybody quantizes.
//!
//! **The reported position is the FOOT of the rise, not its peak.** This is
//! the detail that decides whether the feature sounds right. A detector
//! finds a hit where the energy is greatest, which is several milliseconds
//! AFTER the attack began; anchoring a warp there puts the start of the
//! stick on the wrong side of the beat and the groove drags by exactly that
//! much. So every detection walks backwards down its own slope to where the
//! rise left the floor, and that is what is reported.

use tangent_record::decode::Clip;

/// One detected attack: where it is, and how strongly it announced itself.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Transient {
    /// Frames from the start of the clip - the decoded FILE, at the rate it
    /// was decoded at. The FOOT of the rise - see the module header.
    pub at: usize,
    /// The detection function at this peak: how far, in dB, the hop rose
    /// over the loudest hop of the last [`LOOKBACK_S`] (not a multiple of the
    /// local floor, which only decides whether it counts). A caller ranking
    /// or thinning detections reads this; the detector itself only uses it to
    /// order equals.
    pub strength: f32,
}

/// The envelope hop, in seconds. ~1.3 ms at 48 kHz.
///
/// Finer than [`stretch::percussiveness`](crate::stretch::percussiveness)'s
/// 4 ms because that measures a PROPERTY of a whole clip and this reports a
/// POSITION: a snare anchored 4 ms from where it was played flams audibly
/// against anything else on the grid.
const HOP_S: f64 = 0.0013;

/// How far apart two attacks must be to count as two. ~30 ms: faster than
/// anything a hand plays as separate events, slower than the several
/// milliseconds one hit spends rising.
const MIN_GAP_S: f64 = 0.030;

/// How far back the detection function looks for something louder.
///
/// ~26 ms: several cycles of any beating worth ignoring, and well under the
/// [`MIN_GAP_S`] that separates two events. See the detection function.
const LOOKBACK_S: f64 = 0.026;

/// Half-width of the window the local floor is taken over, in hops.
/// ~130 ms each side, so the floor tracks a phrase rather than a bar.
const FLOOR_HALF: usize = 100;

/// **What a file sounds like, before anybody has said how readily to call
/// a rise an attack** - the sensitivity-free half of the detector, made by
/// [`analyse`] and read by [`pick`].
///
/// It is in the frames of the decode it was made from (`rate`, `frames`).
/// Positions picked from it mean nothing against a decode at another rate
/// or of another file, so a cache of these has to be emptied wherever the
/// decode cache it was filled from is. Not yet true of the host's
/// (`DesktopApp::attacks_of`): it is keyed by path alone and emptied only
/// when it reaches its cap.
pub struct Analysis {
    env: Vec<f32>,
    /// Every hop the flux PEAKS on, with the local median it stands over.
    ///
    /// **The median is here and not in [`pick`] because it does not depend
    /// on the dial either**, and it is where the time goes: a sort of two
    /// hundred numbers at every peak of a three-minute take measured 35 ms,
    /// against 10 ms for the envelope and the flux together. Left in
    /// `pick`, dragging the detect slider would have run that sort tens of
    /// thousands of times a frame.
    cand: Vec<Cand>,
    hop: usize,
    rate: u32,
    frames: usize,
}

/// One rise that turned over, and what it has to beat.
struct Cand {
    /// Which hop.
    k: usize,
    /// The detection function there.
    flux: f32,
    /// The local median of the detection function around it - the adaptive
    /// floor, before the sensitivity's margin is added.
    median: f32,
}

/// **Every rise in `clip` that turned over, with the floor it stands on.**
///
/// The whole cost of a detection - the envelope of the differenced signal
/// (one pass over every sample), the flux against the recent peak, and the
/// local median at every positive flux peak - and none of it reads the
/// sensitivity. The host analyses a file once and [`pick`]s from it per
/// dial position - see `DesktopApp::attacks_of`. `None` for a clip under
/// eight hops (~10 ms).
#[must_use]
pub fn analyse(clip: &Clip) -> Option<Analysis> {
    let hop = ((f64::from(clip.rate.max(1)) * HOP_S) as usize).max(1);
    let frames = clip.frames();
    if frames < hop * 8 {
        return None;
    }

    // ── the envelope, of the DIFFERENCED signal ─────────────────────────
    //
    // **`x[n] - x[n-1]`, a one-line high-pass, and it is what makes this
    // work on a mix rather than only on a click track.**
    //
    // Built on the plain envelope first, and clicks over a held chord found
    // FOUR of sixteen. The reason is not the detector's arithmetic: the
    // chord simply holds the envelope up at the same level the clicks reach,
    // so "louder than anything recent" has nothing to separate. Every real
    // recording is that case - something is always sustaining underneath.
    //
    // A transient is BROADBAND and a held note is not, which is the one
    // property that does separate them. Differencing tilts the spectrum up
    // 6 dB an octave, so the chord's low partials shrink and the attack's
    // edge - a step, and therefore energy everywhere - survives intact. It
    // costs one subtraction per sample and needs no transform.
    let mut env: Vec<f32> = Vec::with_capacity(frames / hop + 1);
    let mut i = 0;
    let d = |f: usize, ch: usize| -> f32 {
        let now = clip.samples.get(f * 2 + ch).copied().unwrap_or(0.0);
        let was = if f == 0 {
            0.0
        } else {
            clip.samples.get((f - 1) * 2 + ch).copied().unwrap_or(0.0)
        };
        (now - was).abs()
    };
    while i + hop <= frames {
        let mut peak = 0.0f32;
        for f in i..i + hop {
            peak = peak.max(d(f, 0).max(d(f, 1)));
        }
        env.push(peak);
        i += hop;
    }
    if env.len() < 8 {
        return None;
    }

    // ── the detection function: this hop over the RECENT PEAK, in dB ────
    //
    // **In dB, because loudness is how a rise is heard.** A linear
    // difference makes the loudest passage's every wobble outrank a quiet
    // passage's real hit, so a detector built on it finds the chorus and
    // nothing else. A ratio does not care how loud the material is.
    //
    // **Against the recent PEAK, not against the previous hop**, and this is
    // the line the whole detector turns on. Written the obvious way first -
    // `env[k] / env[k-1]` - a held chord reported FORTY-SIX attacks in four
    // seconds. Three partials beat against each other, the sum swings from
    // nearly treble to nearly nothing, and every upswing of that is a large
    // positive change with no attack anywhere near it.
    //
    // A rise measured against the loudest thing in the last `LOOKBACK_S` is
    // immune to it: an oscillation only ever returns to a level it has
    // already been at, so it scores zero, while a real attack exceeds
    // everything recent by definition. The window has to be longer than the
    // slowest beat worth ignoring and shorter than the gap between events -
    // 26 ms is several cycles of any audible beating and a fraction of the
    // ~30 ms that separates two hits.
    const FLOOR_DB: f32 = 1.0e-5;
    let look = ((f64::from(clip.rate.max(1)) * LOOKBACK_S) as usize / hop).max(1);
    //
    // **Hop zero is measured against SILENCE**, which is what removes the
    // special case a clip that opens on a hit would otherwise need. There is
    // nothing before the start of a file, so a clip beginning on a downbeat
    // has an attack there by the same arithmetic as every other - and a clip
    // that fades in still has none, because a quiet first hop over silence
    // is a small ratio. Written first as an explicit "is the clip loud at
    // its start" test, which fought the refractory period and lost: a
    // crossfade artefact a few milliseconds in outranked the downbeat and a
    // warped take came back 6.5 ms behind its own grid.
    let flux: Vec<f32> = (0..env.len())
        .map(|k| {
            let from = k.saturating_sub(look);
            let recent = if k == 0 {
                FLOOR_DB
            } else {
                env[from..k].iter().copied().fold(FLOOR_DB, f32::max)
            };
            let now = env[k].max(FLOOR_DB);
            (20.0 * (now / recent.max(FLOOR_DB)).log10()).max(0.0)
        })
        .collect();

    // ── the peaks, and the floor each stands over ───────────────────────
    //
    // Both sensitivity-free, so both are done once here. A rise that has
    // not turned over is not an attack; hop zero has nothing before it, so
    // it only has to out-rank what follows, and the last hop is never a
    // peak because a rise still climbing at the end of the clip is not one
    // this can place.
    let mut cand: Vec<Cand> = Vec::new();
    let mut scratch: Vec<f32> = Vec::with_capacity(FLOOR_HALF * 2 + 1);
    for k in 0..flux.len() {
        let prev = if k == 0 { 0.0 } else { flux[k - 1] };
        let Some(&next) = flux.get(k + 1) else { continue };
        if flux[k] < prev || flux[k] < next || flux[k] <= 0.0 {
            continue;
        }
        let lo = k.saturating_sub(FLOOR_HALF);
        let hi = (k + FLOOR_HALF + 1).min(flux.len());
        scratch.clear();
        scratch.extend_from_slice(&flux[lo..hi]);
        scratch.sort_by(f32::total_cmp);
        cand.push(Cand { k, flux: flux[k], median: scratch[scratch.len() / 2] });
    }
    Some(Analysis { env, cand, hop, rate: clip.rate.max(1), frames })
}

/// **Which of an [`Analysis`]'s rises count as attacks at this
/// sensitivity.** The cheap half: one walk of the candidates [`analyse`]
/// found, each already carrying its local median.
///
/// `sensitivity` runs 0 to 1 and sets the MARGIN over that median: 0 reports
/// only what stands far above its surroundings, 1 reports nearly every rise.
/// 0.5 is the default and is tuned so a click track gives one detection per
/// click and a held chord gives none after its own start.
///
/// Returns positions in ascending order, each the foot of its rise.
#[must_use]
pub fn pick(a: &Analysis, sensitivity: f32) -> Vec<Transient> {
    let Analysis { env, cand, hop, rate, frames } = a;
    let (hop, rate, frames) = (*hop, *rate, *frames);
    // ── the adaptive floor: the local median, plus a margin ─────────────
    let margin = 1.5 + (1.0 - sensitivity.clamp(0.0, 1.0)) * 6.0;
    let mut out: Vec<Transient> = Vec::new();
    let min_gap = ((f64::from(rate) * MIN_GAP_S) as usize / hop).max(1);
    // How far back the foot walk may reach: ~6 ms, a few hops. See `foot`.
    let max_back = ((f64::from(rate) * 0.006) as usize / hop).max(1);
    let mut last: Option<usize> = None;

    for c in cand {
        let (k, flux) = (c.k, c.flux);
        if flux < c.median + margin {
            continue;
        }
        // **The refractory period keeps the LOUDER of two close calls**, so
        // a hit whose second hop is bigger than its first is not reported at
        // the quieter one and then suppressed at the real peak.
        if let Some(prev) = last {
            if k - prev < min_gap {
                if let Some(back) = out.last_mut() {
                    if flux > back.strength {
                        *back = Transient { at: foot(env, k, max_back) * hop, strength: flux };
                        last = Some(k);
                    }
                }
                continue;
            }
        }
        out.push(Transient { at: foot(env, k, max_back) * hop, strength: flux });
        last = Some(k);
    }
    // A guard, not a case that happens today: the foot walk reaches back at
    // most `max_back` hops (~6 ms) and two kept detections are at least
    // `min_gap` hops (~30 ms) apart, so at any real sample rate no two land
    // on one frame. It keeps the list strictly ascending if either constant
    // moves.
    out.dedup_by_key(|t| t.at);
    out.retain(|t| t.at < frames);
    out
}

/// **Walk back down the rise to where it started**, by LEVEL.
///
/// Written first as "step back while the envelope is still falling", which
/// works on a click track and fails on a mix: with something sustaining
/// underneath, the envelope never stops falling on the way back down, so
/// the walk descends through the bed and reports an attack seven
/// milliseconds early - measured, on clicks over a held chord.
///
/// A level crossing has no such problem. From the hop the flux peaked on,
/// step back to where the envelope was last below a fraction of that peak:
/// that is the foot of THIS rise whatever is going on beneath it. Bounded
/// to a few hops besides, because nothing musical takes longer than a few
/// milliseconds to get off the ground and an unbounded walk can only ever
/// make the answer worse.
fn foot(env: &[f32], peak: usize, max_back: usize) -> usize {
    let target = env[peak] * 0.3;
    let limit = peak.saturating_sub(max_back);
    let mut k = peak;
    while k > limit && env[k] > target {
        k -= 1;
    }
    k
}

#[cfg(test)]
mod tests {
    use super::*;
    use tangent_record::decode::Clip;

    const RATE: u32 = 48_000;

    /// The two halves as one call, which is how every test here reads.
    /// Production has no such door: its one caller analyses once per file
    /// and picks per dial position.
    fn detect(clip: &Clip, sensitivity: f32) -> Vec<Transient> {
        analyse(clip).map_or_else(Vec::new, |a| pick(&a, sensitivity))
    }

    fn clip_of(samples: Vec<f32>) -> Clip {
        Clip { samples, rate: RATE, source: std::path::PathBuf::from("t.wav") }
    }

    /// A click every `every` seconds: a short decaying burst. The same
    /// fixture `stretch`'s tests use, so the two modules are characterised
    /// on one piece of material.
    fn clicks(seconds: f64, every: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        let period = (f64::from(RATE) * every) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let k = f % period;
                    let env = (-(k as f32) / (f64::from(RATE) as f32 * 0.01)).exp();
                    let v = (std::f32::consts::TAU * 900.0 * k as f32 / f64::from(RATE) as f32)
                        .sin()
                        * env
                        * 0.7;
                    [v, v]
                })
                .collect(),
        )
    }

    /// A held chord: three partials, no edges anywhere after the first.
    fn chord(seconds: f64) -> Clip {
        let n = (f64::from(RATE) * seconds) as usize;
        clip_of(
            (0..n)
                .flat_map(|f| {
                    let t = f as f64 / f64::from(RATE);
                    let v = [220.0, 277.18, 329.63]
                        .iter()
                        .map(|hz| (std::f64::consts::TAU * hz * t).sin())
                        .sum::<f64>() as f32
                        * 0.2;
                    [v, v]
                })
                .collect(),
        )
    }

    /// **Every click is found, once, where it actually is.**
    ///
    /// The whole feature rests on this. Positions are checked against where
    /// the fixture PUT the clicks - arithmetic this module never sees - and
    /// not against the detector's own output, which would pass on any
    /// implementation that was merely self-consistent.
    #[test]
    fn a_click_track_reports_one_attack_per_click_on_the_click() {
        let every = 0.25;
        let c = clicks(4.0, every);
        let found = detect(&c, 0.5);
        let want = (4.0 / every) as usize;
        assert_eq!(
            found.len(),
            want,
            "wanted {want} attacks, got {}: {:?}",
            found.len(),
            found.iter().map(|t| t.at).collect::<Vec<_>>()
        );
        // **Within 3 ms of the truth.** A hop is 1.3 ms, so this is two
        // hops: tighter than the ear's flam threshold and loose enough not
        // to pin the exact bucket the envelope landed in.
        let tol = (f64::from(RATE) * 0.003) as usize;
        for (i, t) in found.iter().enumerate() {
            let truth = (f64::from(RATE) * every * i as f64) as usize;
            let off = t.at.abs_diff(truth);
            assert!(
                off <= tol,
                "attack {i} at {} frames, the click is at {truth} - {off} frames out",
                t.at
            );
        }
    }

    /// **A held chord announces itself once and then stops.**
    ///
    /// The classic false positive: a detector with a fixed floor keeps
    /// firing on a sustained signal's own ripple. Anything past the first
    /// attack here is the detector hearing edges that are not there.
    #[test]
    fn a_held_chord_has_no_attacks_after_its_own_start() {
        let c = chord(4.0);
        let found = detect(&c, 0.5);
        let after = found
            .iter()
            .filter(|t| t.at > (f64::from(RATE) * 0.1) as usize)
            .count();
        assert_eq!(
            after, 0,
            "a held chord reported {after} attacks after its start: {:?}",
            found.iter().map(|t| t.at).collect::<Vec<_>>()
        );
    }

    /// **A crescendo is not an attack**, and this is what the adaptive floor
    /// is for. A tone rising steadily over four seconds has positive flux at
    /// every single hop; a fixed threshold reports hundreds of onsets.
    #[test]
    fn a_crescendo_is_not_a_row_of_attacks() {
        let n = (f64::from(RATE) * 4.0) as usize;
        let c = clip_of(
            (0..n)
                .flat_map(|f| {
                    let t = f as f64 / f64::from(RATE);
                    let v = ((std::f64::consts::TAU * 220.0 * t).sin() * (t / 4.0) * 0.8) as f32;
                    [v, v]
                })
                .collect(),
        );
        let found = detect(&c, 0.5);
        assert!(
            found.len() <= 2,
            "a crescendo reported {} attacks",
            found.len()
        );
    }

    /// **Silence has nothing in it**, and a clip too short to analyse
    /// answers nothing rather than dividing by a window it does not have.
    #[test]
    fn silence_and_scraps_report_nothing() {
        assert!(detect(&clip_of(vec![0.0; 48_000 * 2]), 0.5).is_empty(), "silence");
        assert!(detect(&clip_of(vec![0.1; 40]), 0.5).is_empty(), "a scrap");
        assert!(detect(&clip_of(Vec::new()), 0.5).is_empty(), "nothing at all");
    }

    /// **The hits are found THROUGH sustained material**, which is the case
    /// the feature actually meets.
    ///
    /// A click track alone is the easy test: the envelope returns to nothing
    /// between hits. An ordinary mix does not - the chord holds the floor up
    /// the whole time - and a detector that only worked on the easy case
    /// would pass every test above this one and be useless on a real take.
    #[test]
    fn clicks_over_a_held_chord_are_still_found_on_the_clicks() {
        let every = 0.25;
        let (a, b) = (chord(4.0), clicks(4.0, every));
        let mix = clip_of(
            a.samples.iter().zip(b.samples.iter()).map(|(x, y)| 0.6 * x + 0.5 * y).collect(),
        );
        let found = detect(&mix, 0.5);
        let want = (4.0 / every) as usize;
        assert_eq!(
            found.len(),
            want,
            "wanted {want} attacks through the chord, got {}: {:?}",
            found.len(),
            found.iter().map(|t| t.at).collect::<Vec<_>>()
        );
        let tol = (f64::from(RATE) * 0.005) as usize;
        for (i, t) in found.iter().enumerate() {
            let truth = (f64::from(RATE) * every * i as f64) as usize;
            assert!(
                t.at.abs_diff(truth) <= tol,
                "attack {i} at {} frames, the click is at {truth}",
                t.at
            );
        }
    }

    /// **The dial is free to turn.**
    ///
    /// `analyse` is the whole cost of a detection and `pick` is what a hand
    /// on the detect slider re-runs, so the split is only worth anything if
    /// the second one is nearly nothing. Measured on a three-minute take at
    /// 48 kHz: 12 ms to analyse, 0.01 ms to pick. Before the median moved
    /// out of `pick` it was 35 ms - a sort of two hundred numbers at every
    /// peak - which is three dropped frames per pixel of slider travel.
    ///
    /// The ratio is what is asserted, not the milliseconds: this runs on
    /// whatever machine it runs on. A hundredfold is far inside the
    /// thousandfold measured and far outside anything the old shape could
    /// reach.
    #[test]
    fn picking_a_sensitivity_costs_almost_nothing_next_to_analysing() {
        let rate = 48_000u32;
        let frames = rate as usize * 20;
        let mut v = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let t = f as f32 / rate as f32;
            let hit = ((t * 4.0).fract() * 40.0).exp2().recip();
            let x = (t * 220.0 * std::f32::consts::TAU).sin() * 0.2 + hit * 0.7;
            v.push(x);
            v.push(x);
        }
        let clip = Clip { samples: v, rate, source: std::path::PathBuf::from("t.wav") };
        let t0 = std::time::Instant::now();
        let a = analyse(&clip).expect("an analysis");
        let analysed = t0.elapsed();
        // Enough picks that the timer has something to read.
        const N: u32 = 50;
        let t1 = std::time::Instant::now();
        let mut found = 0usize;
        for i in 0..N {
            found += pick(&a, 0.2 + (i % 40) as f32 * 0.02).len();
        }
        let picked = t1.elapsed() / N;
        assert!(found > 0, "the fixture has no attacks to pick");
        assert!(
            picked.as_secs_f64() * 100.0 < analysed.as_secs_f64(),
            "a pick costs {:?} against an analysis of {analysed:?} - the dial is not free",
            picked
        );
    }

    /// **Sensitivity moves the count in the direction it says**, which is
    /// the only promise the dial makes.
    #[test]
    fn a_lower_sensitivity_never_finds_more() {
        let c = clicks(4.0, 0.25);
        let low = detect(&c, 0.0).len();
        let mid = detect(&c, 0.5).len();
        let high = detect(&c, 1.0).len();
        assert!(low <= mid && mid <= high, "counts went {low}/{mid}/{high}");
    }
}
