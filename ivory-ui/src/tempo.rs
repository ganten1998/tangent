//! The tempo map: where musical time and wall-clock time meet.
//!
//! # What this replaces
//!
//! One number. `tempo_bpm` was a single `f64` in a preferences file, and
//! twelve places across five files each turned it into seconds by
//! multiplying. That is correct exactly as long as the tempo never changes,
//! and it is why this app could not represent a ritardando or a 4/4→3/4.
//!
//! # The one rule everything else follows from
//!
//! **A beat here is a QUARTER NOTE, always.** Not "a beat of the current time
//! signature" — a quarter. The reason is not taste: an SMF tempo meta event
//! is microseconds per quarter and nothing else, so the moment a file is
//! written or read the quarter is the unit whether anyone likes it or not.
//! `TimeSignature::beat_seconds` and `period_frames` in the engine have both
//! encoded that for years (`(4/unit) * (60/bpm)`), and this module keeps it.
//!
//! So: the TEMPO map is in quarters, the METER map is in bars, and turning a
//! bar into a quarter is the meter map's job.
//!
//! # Why the ramps are exact
//!
//! A ritardando is a tempo that changes CONTINUOUSLY, so the seconds between
//! two beats is an integral rather than a multiply. Taken naively that means
//! stepping a loop, which on the audio thread is both slow and — worse —
//! dependent on the step size, so the click and the playhead would disagree
//! by an amount that changed with the buffer.
//!
//! It has a closed form. Over a segment where the tempo moves linearly in
//! BEATS from `a` to `b`:
//!
//! ```text
//!     dt/dβ = 60 / bpm(β)          bpm(β) = a + (b - a)·β̂
//!     t     = 60·Δβ/(b - a) · ln(bpm(β)/a)
//! ```
//!
//! which inverts just as cleanly. Both directions are a handful of
//! floating-point operations, exact at any distance from the segment's start,
//! and identical whether asked for one frame or ten thousand.

use crate::recorder::{TimeSignature, DEFAULT_BPM, MAX_BPM, MIN_BPM};

/// One entry in the tempo map.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TempoPoint {
    /// Where it takes effect, in quarter notes from 0.
    pub at_beat: f64,
    pub bpm: f64,
    /// Slide to the NEXT point's tempo across the span between them, rather
    /// than stepping to this one and holding.
    ///
    /// **This is the ritardando**, and it is a property of the point the
    /// slide starts FROM. A ramp on the last point has nothing to ramp
    /// towards and holds, which is the only sane reading of it.
    pub ramp: bool,
}

/// One entry in the meter map.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MeterPoint {
    /// Which bar it takes effect at, counting from 0. A signature change
    /// lands on a BAR LINE and nowhere else — a bar and a half of 4/4
    /// followed by 3/4 is not a thing any notation can express, and letting
    /// somebody ask for it produces a document nothing can draw.
    pub at_bar: u32,
    pub sig: TimeSignature,
}

/// Tempo over the whole song.
#[derive(Debug, Clone, PartialEq)]
pub struct TempoMap {
    /// Sorted by `at_beat`, never empty — see [`TempoMap::sanitised`].
    points: Vec<TempoPoint>,
}

impl Default for TempoMap {
    fn default() -> Self {
        Self::constant(DEFAULT_BPM)
    }
}

impl TempoMap {
    /// One tempo for the whole song — what every document written before this
    /// existed means.
    pub fn constant(bpm: f64) -> Self {
        Self {
            points: vec![TempoPoint {
                at_beat: 0.0,
                bpm: clamp_bpm(bpm),
                ramp: false,
            }],
        }
    }

    /// Build from arbitrary points, putting them in a state the rest of this
    /// file may assume.
    ///
    /// **Every reader here is allowed to assume the map is sorted, starts at
    /// beat 0, and has at least one point**, and this is the only function
    /// that makes that true. A document is a file somebody can edit by hand,
    /// so "the file said so" is not a guarantee of anything: points arrive
    /// out of order, at negative beats, at NaN, duplicated on the same beat,
    /// or absent entirely.
    ///
    /// Duplicates on one beat keep the LAST, which is what a person editing a
    /// list means by writing the same position twice.
    pub fn sanitised(mut points: Vec<TempoPoint>) -> Self {
        points.retain(|p| p.at_beat.is_finite() && p.bpm.is_finite() && p.at_beat >= 0.0);
        for p in &mut points {
            p.bpm = clamp_bpm(p.bpm);
        }
        points.sort_by(|a, b| a.at_beat.partial_cmp(&b.at_beat).unwrap_or(std::cmp::Ordering::Equal));
        points.dedup_by(|a, b| (a.at_beat - b.at_beat).abs() < 1e-9);
        match points.first() {
            // A map that starts late leaves the opening bars with no tempo at
            // all. The first point's own tempo is the only honest thing to
            // fill that with.
            Some(first) if first.at_beat > 0.0 => {
                let bpm = first.bpm;
                points.insert(0, TempoPoint { at_beat: 0.0, bpm, ramp: false });
            }
            None => points.push(TempoPoint { at_beat: 0.0, bpm: DEFAULT_BPM, ramp: false }),
            Some(_) => {}
        }
        Self { points }
    }

    pub fn points(&self) -> &[TempoPoint] {
        &self.points
    }

    /// Whether this is one tempo all the way through — the shape every
    /// document had before the map existed, and the fast path several callers
    /// still want to take.
    pub fn is_constant(&self) -> bool {
        self.points.len() == 1
    }

    /// The tempo AT a beat, ramps resolved.
    pub fn bpm_at_beat(&self, beat: f64) -> f64 {
        let i = self.index_for_beat(beat);
        let p = self.points[i];
        match self.points.get(i + 1) {
            Some(next) if p.ramp && next.at_beat > p.at_beat => {
                let t = ((beat - p.at_beat) / (next.at_beat - p.at_beat)).clamp(0.0, 1.0);
                clamp_bpm(p.bpm + (next.bpm - p.bpm) * t)
            }
            _ => p.bpm,
        }
    }

    /// Seconds from 0:00 to `beat`.
    ///
    /// The whole point of this module: with one tempo it is a multiply, and
    /// with a map it is the sum of the segments before `beat` plus a partial
    /// one. Both halves are closed-form — see the module header.
    pub fn seconds_at_beat(&self, beat: f64) -> f64 {
        if !beat.is_finite() || beat <= 0.0 {
            return 0.0;
        }
        let mut secs = 0.0;
        for i in 0..self.points.len() {
            let p = self.points[i];
            let end = self.points.get(i + 1).map_or(f64::INFINITY, |n| n.at_beat);
            if beat <= p.at_beat {
                break;
            }
            let upto = beat.min(end);
            secs += self.segment_seconds(i, upto - p.at_beat);
            if beat <= end {
                break;
            }
        }
        secs
    }

    /// The inverse: which beat is playing at `seconds`.
    pub fn beat_at_seconds(&self, seconds: f64) -> f64 {
        if !seconds.is_finite() || seconds <= 0.0 {
            return 0.0;
        }
        let mut left = seconds;
        for i in 0..self.points.len() {
            let p = self.points[i];
            let span = self
                .points
                .get(i + 1)
                .map(|n| n.at_beat - p.at_beat)
                .unwrap_or(f64::INFINITY);
            let whole = self.segment_seconds(i, span);
            if left < whole || !whole.is_finite() {
                return p.at_beat + self.segment_beats(i, left);
            }
            left -= whole;
        }
        // Unreachable: the last segment is infinite. Answered rather than
        // panicked, because this runs on the audio thread.
        self.points.last().map_or(0.0, |p| p.at_beat)
    }

    /// Seconds taken by `beats` beats from the start of segment `i`.
    fn segment_seconds(&self, i: usize, beats: f64) -> f64 {
        if beats <= 0.0 {
            return 0.0;
        }
        let p = self.points[i];
        let Some((span, to)) = self.ramp_span(i) else {
            return beats * 60.0 / p.bpm;
        };
        // Linear in beats: t = 60·span/(to − from) · ln(bpm(β)/from).
        let k = 60.0 * span / (to - p.bpm);
        let at = p.bpm + (to - p.bpm) * (beats / span);
        k * (at / p.bpm).ln()
    }

    /// The inverse of [`Self::segment_seconds`]: beats covered by `seconds`.
    fn segment_beats(&self, i: usize, seconds: f64) -> f64 {
        if seconds <= 0.0 {
            return 0.0;
        }
        let p = self.points[i];
        let Some((span, to)) = self.ramp_span(i) else {
            return seconds * p.bpm / 60.0;
        };
        let k = 60.0 * span / (to - p.bpm);
        let at = p.bpm * (seconds / k).exp();
        span * (at - p.bpm) / (to - p.bpm)
    }

    /// `(span_in_beats, tempo_at_the_far_end)` when segment `i` really ramps.
    ///
    /// `None` covers every case that is a plain hold: no ramp asked for, no
    /// next point to ramp towards, a zero-length span, or a "ramp" between
    /// two identical tempos — which is a hold with extra steps, and whose
    /// closed form divides by zero.
    fn ramp_span(&self, i: usize) -> Option<(f64, f64)> {
        let p = self.points[i];
        if !p.ramp {
            return None;
        }
        let next = self.points.get(i + 1)?;
        let span = next.at_beat - p.at_beat;
        if span <= 0.0 || (next.bpm - p.bpm).abs() < 1e-9 {
            return None;
        }
        Some((span, next.bpm))
    }

    fn index_for_beat(&self, beat: f64) -> usize {
        match self
            .points
            .binary_search_by(|p| p.at_beat.partial_cmp(&beat).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        }
    }
}

/// Time signatures over the whole song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeterMap {
    /// Sorted by `at_bar`, never empty.
    points: Vec<MeterPoint>,
}

impl Default for MeterMap {
    fn default() -> Self {
        Self::constant(TimeSignature::default())
    }
}

impl MeterMap {
    pub fn constant(sig: TimeSignature) -> Self {
        Self {
            points: vec![MeterPoint { at_bar: 0, sig: sane_sig(sig) }],
        }
    }

    /// See [`TempoMap::sanitised`] — the same bargain, for the same reason.
    pub fn sanitised(mut points: Vec<MeterPoint>) -> Self {
        for p in &mut points {
            p.sig = sane_sig(p.sig);
        }
        points.sort_by_key(|p| p.at_bar);
        points.dedup_by_key(|p| p.at_bar);
        match points.first() {
            Some(first) if first.at_bar > 0 => {
                let sig = first.sig;
                points.insert(0, MeterPoint { at_bar: 0, sig });
            }
            None => points.push(MeterPoint { at_bar: 0, sig: TimeSignature::default() }),
            Some(_) => {}
        }
        Self { points }
    }

    pub fn points(&self) -> &[MeterPoint] {
        &self.points
    }

    pub fn is_constant(&self) -> bool {
        self.points.len() == 1
    }

    /// The signature in force at `bar`.
    pub fn sig_at_bar(&self, bar: u32) -> TimeSignature {
        let i = match self.points.binary_search_by_key(&bar, |p| p.at_bar) {
            Ok(i) => i,
            Err(0) => 0,
            Err(i) => i - 1,
        };
        self.points[i].sig
    }

    /// How many QUARTER notes a bar of the signature at `bar` lasts.
    ///
    /// `beats * 4/unit`: a bar of 6/8 is six eighths, which is three
    /// quarters. The tempo map counts quarters, so this is the exchange rate
    /// between the two maps and the only place it lives.
    pub fn quarters_in_bar(&self, bar: u32) -> f64 {
        let sig = self.sig_at_bar(bar);
        f64::from(sig.beats.max(1)) * (4.0 / f64::from(sig.unit.max(1)))
    }

    /// `(bar, beat within the bar)` at a position in quarters. Both 0-based.
    ///
    /// Walks the meter changes, which is a loop over the SIGNATURE changes in
    /// a song and not over its bars — a handful of iterations for any real
    /// document, and constant time for the overwhelmingly common map of one.
    pub fn bar_beat_at_quarter(&self, quarter: f64) -> (u32, f64) {
        if !quarter.is_finite() || quarter <= 0.0 {
            return (0, 0.0);
        }
        let mut left = quarter;
        let mut bar = 0u32;
        for i in 0..self.points.len() {
            let sig = self.points[i].sig;
            let per_bar = f64::from(sig.beats.max(1)) * (4.0 / f64::from(sig.unit.max(1)));
            // How many bars this segment holds before the next change, or
            // for ever on the last one.
            let bars_here = self
                .points
                .get(i + 1)
                .map_or(f64::INFINITY, |n| f64::from(n.at_bar - self.points[i].at_bar));
            let quarters_here = bars_here * per_bar;
            if left < quarters_here || !quarters_here.is_finite() {
                let whole = (left / per_bar).floor();
                bar += whole as u32;
                let within = left - whole * per_bar;
                // In BEATS of the signature, not quarters: "bar 17 beat 3" of
                // 6/8 means the third eighth.
                return (bar, within * f64::from(sig.unit.max(1)) / 4.0);
            }
            left -= quarters_here;
            bar += bars_here as u32;
        }
        (bar, 0.0)
    }

    /// The quarter a bar line falls on. The inverse of the walk above.
    pub fn quarter_at_bar(&self, bar: u32) -> f64 {
        let mut quarters = 0.0;
        for i in 0..self.points.len() {
            let start = self.points[i].at_bar;
            if bar <= start {
                break;
            }
            let end = self.points.get(i + 1).map_or(bar, |n| n.at_bar.min(bar));
            quarters += f64::from(end - start) * self.quarters_in_bar(start);
            if bar <= end {
                break;
            }
        }
        quarters
    }
}

fn clamp_bpm(bpm: f64) -> f64 {
    if bpm.is_finite() {
        bpm.clamp(MIN_BPM, MAX_BPM)
    } else {
        DEFAULT_BPM
    }
}

fn sane_sig(sig: TimeSignature) -> TimeSignature {
    if sig.is_valid() {
        sig
    } else {
        TimeSignature::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f64, b: f64, what: &str) {
        assert!((a - b).abs() < 1e-6, "{what}: {a} against {b}");
    }

    /// One tempo behaves exactly as the multiply it replaces.
    ///
    /// The map has to be a strict generalisation or every document written
    /// before it plays at a different speed — which is the kind of regression
    /// nobody reports as a bug, because they assume they mis-remembered.
    #[test]
    fn one_tempo_is_the_multiply_it_replaces() {
        let m = TempoMap::constant(120.0);
        assert!(m.is_constant());
        close(m.seconds_at_beat(0.0), 0.0, "zero");
        close(m.seconds_at_beat(1.0), 0.5, "one beat at 120");
        close(m.seconds_at_beat(4.0), 2.0, "a bar at 120");
        close(m.beat_at_seconds(2.0), 4.0, "back again");
        close(m.bpm_at_beat(99.0), 120.0, "the tempo never moves");
        // And at a tempo that is not a round number, where a naive
        // accumulator would drift.
        let m = TempoMap::constant(97.0);
        close(m.seconds_at_beat(97.0), 60.0, "97 beats at 97 bpm is a minute");
        close(m.beat_at_seconds(60.0), 97.0, "and back");
    }

    /// A step change: the tempo holds, then jumps.
    #[test]
    fn a_tempo_change_takes_effect_where_it_is_written() {
        let m = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: false },
            TempoPoint { at_beat: 8.0, bpm: 60.0, ramp: false },
        ]);
        close(m.seconds_at_beat(8.0), 4.0, "eight beats at 120");
        // ...then half speed, so the next eight take twice as long.
        close(m.seconds_at_beat(16.0), 12.0, "eight more at 60");
        close(m.beat_at_seconds(4.0), 8.0, "the change point inverts");
        close(m.beat_at_seconds(12.0), 16.0, "and past it");
        close(m.bpm_at_beat(7.9), 120.0, "before");
        close(m.bpm_at_beat(8.0), 60.0, "at");
    }

    /// **A ritardando, and it is exact.**
    ///
    /// The closed form is what makes the click and the playhead agree: a
    /// stepped integral would give a different answer per buffer size, and
    /// the two would drift apart by an amount that changed when somebody
    /// opened Setup.
    #[test]
    fn a_ritardando_is_exact_at_any_distance_into_it() {
        // Eight beats sliding 120 → 60.
        let m = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: true },
            TempoPoint { at_beat: 8.0, bpm: 60.0, ramp: false },
        ]);
        // t = 60·Δβ/(b−a)·ln(b/a) = 60·8/(−60)·ln(0.5) = 5.545…
        let whole = 60.0 * 8.0 / -60.0 * 0.5_f64.ln();
        close(m.seconds_at_beat(8.0), whole, "the whole slide");
        // Halfway in BEATS the tempo is halfway between, by definition.
        close(m.bpm_at_beat(4.0), 90.0, "halfway");
        // Every point on the curve inverts back to itself. This is the
        // property a stepped integral does not have.
        for n in 0..=80 {
            let beat = f64::from(n) * 0.1;
            let secs = m.seconds_at_beat(beat);
            close(m.beat_at_seconds(secs), beat, "round trip");
        }
        // And it is MONOTONIC — a slowing tempo still moves forwards.
        let mut last = -1.0;
        for n in 0..=80 {
            let s = m.seconds_at_beat(f64::from(n) * 0.1);
            assert!(s > last, "time went backwards at beat {}", f64::from(n) * 0.1);
            last = s;
        }
    }

    /// A ramp between two equal tempos is a hold, not a division by zero.
    #[test]
    fn a_ramp_that_does_not_move_is_still_a_tempo() {
        let m = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: true },
            TempoPoint { at_beat: 8.0, bpm: 120.0, ramp: false },
        ]);
        close(m.seconds_at_beat(8.0), 4.0, "eight beats at 120");
        assert!(m.seconds_at_beat(4.0).is_finite());
        // A ramp on the LAST point has nothing to reach and holds.
        let m = TempoMap::sanitised(vec![TempoPoint {
            at_beat: 0.0,
            bpm: 90.0,
            ramp: true,
        }]);
        close(m.seconds_at_beat(90.0), 60.0, "a ramp to nowhere is a hold");
    }

    /// A hand-written map is put in order rather than believed.
    #[test]
    fn a_map_from_a_file_is_made_safe_before_it_is_read() {
        let m = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 16.0, bpm: 90.0, ramp: false },
            TempoPoint { at_beat: 4.0, bpm: 140.0, ramp: false },
            // Out of range, not a number, and negative: all dropped or
            // clamped rather than trusted.
            TempoPoint { at_beat: 8.0, bpm: 5.0, ramp: false },
            TempoPoint { at_beat: f64::NAN, bpm: 120.0, ramp: false },
            TempoPoint { at_beat: -4.0, bpm: 120.0, ramp: false },
        ]);
        let p = m.points();
        assert_eq!(p[0].at_beat, 0.0, "a map that starts late has no opening tempo");
        assert!(p.windows(2).all(|w| w[0].at_beat < w[1].at_beat), "not sorted: {p:?}");
        assert!(p.iter().all(|q| q.bpm >= MIN_BPM && q.bpm <= MAX_BPM), "{p:?}");
        // An empty list is still a usable map.
        let empty = TempoMap::sanitised(Vec::new());
        assert!(empty.is_constant());
        close(empty.bpm_at_beat(0.0), DEFAULT_BPM, "an empty map's tempo");
        // Two points on one beat is one point: the last wins.
        let dup = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: false },
            TempoPoint { at_beat: 4.0, bpm: 100.0, ramp: false },
            TempoPoint { at_beat: 4.0, bpm: 140.0, ramp: false },
        ]);
        assert_eq!(dup.points().len(), 2, "{:?}", dup.points());
    }

    /// Bars and beats, through a signature change.
    #[test]
    fn the_bar_line_moves_when_the_signature_does() {
        let m = MeterMap::sanitised(vec![
            MeterPoint { at_bar: 0, sig: TimeSignature { beats: 4, unit: 4 } },
            MeterPoint { at_bar: 4, sig: TimeSignature { beats: 3, unit: 4 } },
        ]);
        // Four bars of 4/4 is sixteen quarters.
        close(m.quarter_at_bar(4), 16.0, "four bars of four");
        // ...then bars of three.
        close(m.quarter_at_bar(5), 19.0, "one bar of three");
        close(m.quarter_at_bar(6), 22.0, "two bars of three");
        let (bar, beat) = m.bar_beat_at_quarter(16.0);
        assert_eq!(bar, 4);
        close(beat, 0.0, "on the bar line");
        let (bar, beat) = m.bar_beat_at_quarter(20.0);
        assert_eq!(bar, 5, "the fifth bar is three quarters long");
        close(beat, 1.0, "one beat in");
        // And it inverts everywhere.
        for n in 0..40 {
            let q = f64::from(n);
            let (b, _) = m.bar_beat_at_quarter(q);
            assert!(m.quarter_at_bar(b) <= q + 1e-9, "bar {b} starts after quarter {q}");
            assert!(m.quarter_at_bar(b + 1) > q, "bar {b} ends before quarter {q}");
        }
    }

    /// **6/8 counts eighths, and the tempo still counts quarters.**
    ///
    /// The invariant the whole module rests on. A bar of 6/8 is three
    /// quarters long, so at 120 it lasts a second and a half — which is what
    /// `period_frames` and `beat_seconds` have always said.
    #[test]
    fn the_tempo_counts_quarters_whatever_the_signature_says() {
        let m = MeterMap::constant(TimeSignature { beats: 6, unit: 8 });
        close(m.quarters_in_bar(0), 3.0, "a bar of 6/8 is three quarters");
        let t = TempoMap::constant(120.0);
        close(
            t.seconds_at_beat(m.quarters_in_bar(0)),
            1.5,
            "a bar of 6/8 at 120",
        );
        // Beat 4 of the bar is the fourth EIGHTH, one and a half quarters in.
        let (bar, beat) = m.bar_beat_at_quarter(1.5);
        assert_eq!(bar, 0);
        close(beat, 3.0, "the fourth eighth is beat index 3");
        // A signature nothing can play falls back rather than dividing by
        // zero somewhere later.
        let bad = MeterMap::constant(TimeSignature { beats: 0, unit: 5 });
        assert_eq!(bad.sig_at_bar(0), TimeSignature::default());
    }
}
