//! **Quantize: moving what was played onto the grid, by how much, and
//! leaving alone what should be left alone.**
//!
//! One module for MIDI and for audio, deliberately. A MIDI quantize moves a
//! note's `at`; an audio quantize moves a detected transient. Those are the
//! same arithmetic aimed at different things, and writing them twice is how
//! the two drift until a drummer's take and the MIDI doubling it no longer
//! agree about where the beat is.
//!
//! # Where the grid comes from
//!
//! **The song's own lines.** A [`Grid`] holds the song's tempo and meter
//! maps - `TangentApp::live_tempo_map` and `live_meter_map`, the maps the
//! arrangement draws its lines from and the engine plays to - beside a grid
//! exponent, triplet and swing. Every answer goes from seconds to quarter
//! notes through the tempo map, finds the line in the bar that quarter is
//! in, and comes back through the tempo map, so after a tempo or a meter
//! change a quantize aims at the lines on screen. It used to be one tempo
//! (the dial's) and one meter counted from 0:00: with 120 bpm changing to 90
//! at beat 16, a note on beat 17 (8.667 s) was pulled to 8.5 s, a line
//! nobody could see.
//!
//! Inside a bar the lines step
//! [`arrangement_panel::grid_quarters`](crate::arrangement_panel::grid_quarters) -
//! the one authority on what a grid step IS, asked in that bar's own
//! signature - from the bar's own line. **A bar line is always a line**:
//! triplet runs and swung pairs start again at every bar line, so a 3/4 bar
//! of four and a half quarter-note triplets leaves the next downbeat on a
//! line rather than between two. A rung of a bar or more is a line every
//! `2^exp` bars, counted from the first, and its triplet or swung pair runs
//! over two of those steps.
//!
//! The nearest line is the nearest in quarter notes, as the arrangement's
//! magnet measures it, and that magnet IS this walk:
//! `TangentApp::snap_quarter` asks [`nearest_quarter`]. The MIDI editor
//! draws, snaps, steps and walks on a `Grid` built from the same maps
//! (`midi_editor::view_grid`), the MIDI quantize and the audio quantize aim
//! with the one the quantize window built (`QuantizeAsk::grid`), and the
//! host draws the window's ticks from that one too.
//!
//! Whoever LABELS a grid has to spell it in the meter it quantizes in -
//! `grid_label_text` with the same `(beats, unit)` - or it is a magnet
//! pointing somewhere the eye cannot see, and this app has already paid for
//! that once: the box said "1/2" while the magnet snapped eighths. The
//! quantize window's GRID chips are spelled in the dial's signature
//! (`time_signature`, the song's meter while it has one), read on every
//! frame the window is drawn. Not yet per bar: in a song whose meter
//! changes, a bar in another signature takes its own note value for the
//! rung a chip names.
//!
//! # The three knobs, and why each exists
//!
//! **Strength** is a partial move, not a switch. At 1.0 a note lands on the
//! line; at 0.5 it travels half the distance and keeps half of whatever the
//! hand did. That is the difference between a take that sits better and a
//! take that sounds sequenced, and it is the knob most people actually want
//! — full quantize is the special case, not the default.
//!
//! **Window** is what stops a quantize from being destructive. An event
//! further from every line than this is left exactly where it is, because it
//! is not a late note, it is a different note: a grace note, a flam, a drag,
//! the second half of a roll. Without it a quantize collapses an ornament
//! onto the beat it decorates and there is no undo that brings the
//! performance back, only the notes.
//!
//! **Swing** is where the off-beat of each pair sits, as a fraction of the
//! pair. 0.5 is straight; 0.67 is a triplet feel. It is applied as a
//! CANDIDATE, exactly as the editor's magnet applies it, so a swung grid
//! quantizes to the swung lines rather than to the straight ones underneath.

use crate::tempo::{MeterMap, TempoMap};

/// **How near a line still counts as ON it**, in quarter notes: float
/// noise, and nothing a hand can play. The finest grid allowed (a step of a
/// millisecond) keeps its lines thousands of times further apart than this.
const ON_LINE_Q: f64 = 1.0e-9;

/// **Where the lines are**, in seconds on the song's timeline.
///
/// Seconds at the edge rather than frames because the grid is a musical
/// quantity and the sample rate is not: the same bar is the same bar at 44.1
/// and at 96, and a project that changes device should not quantize
/// differently. Callers working in frames convert at their own edge, where
/// they know the rate.
///
/// **Quarter notes inside**, against the song's maps - see the module
/// header. That is why it is `Clone` and not `Copy`: it carries the two
/// maps, a handful of points each, and carrying them is what keeps it plain
/// data that can cross into the host inside a `QuantizeAsk`.
#[derive(Debug, Clone, PartialEq)]
pub struct Grid {
    /// Seconds to quarter notes and back.
    tempo: TempoMap,
    /// Where each bar line is, and the signature each bar's rung is asked in.
    meter: MeterMap,
    /// The rung, triplet and swing.
    shape: Shape,
}

/// **A grid without its tempo**: the rung, triplet and swing, whose lines
/// are quarter notes on a meter map. The arrangement's magnet works in
/// quarters already and asks this directly ([`nearest_quarter`]).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shape {
    /// As `grid_quarters` takes it: below zero a note value of the bar,
    /// zero and above `2^exp` bars.
    exp: i8,
    /// Three in the space of two: a step of two thirds of the rung.
    triplet: bool,
    /// Where the off-beat of each PAIR of steps sits, as a fraction of the
    /// pair - `None` for a straight grid, and always for a triplet one. Held
    /// as the fraction and not as a percentage because everything downstream
    /// multiplies by it.
    swing: Option<f64>,
}

/// **One run of lines**, in quarter notes: from `a`, the line every step in
/// it is counted from, at `a + k * period` (and when swung, `off` after each
/// of those), up to `b`, the next run's `a` - always a line, and never one
/// of the steps past it. The last step may be cut short by `b`.
#[derive(Debug, Clone, Copy)]
struct Span {
    a: f64,
    b: f64,
    period: f64,
    off: Option<f64>,
    /// The bar `b` stands on, where the next run is asked for.
    next_bar: u32,
}

impl Span {
    /// How many steps of the run start inside it: four and a half triplets
    /// in a 3/4 bar is five.
    fn steps(&self) -> f64 {
        ((self.b - self.a) / self.period - ON_LINE_Q).ceil().max(1.0)
    }

    /// The last line at or before `q`, `b` among them.
    fn floor(&self, q: f64) -> f64 {
        let q = q + ON_LINE_Q;
        if q >= self.b {
            return self.b;
        }
        if self.period <= 0.0 {
            return self.a;
        }
        let k = ((q - self.a) / self.period).floor().clamp(0.0, self.steps() - 1.0);
        let head = self.a + k * self.period;
        match self.off {
            Some(off) if head + off <= q && head + off < self.b - ON_LINE_Q => head + off,
            _ => head,
        }
    }

    /// The first line after `q` in this run, `b` among them, or `None` when
    /// `q` is not before `b`.
    fn next(&self, q: f64) -> Option<f64> {
        let q = q + ON_LINE_Q;
        if self.b <= q {
            return None;
        }
        if self.period <= 0.0 {
            return Some(self.b);
        }
        let k = ((q - self.a) / self.period).floor().max(0.0);
        // The step `q` is in and the next, and one more for the float that
        // leaves `q` a hair on the next one's head.
        for i in 0..3 {
            let head = self.a + (k + f64::from(i)) * self.period;
            for c in [Some(head), self.off.map(|o| head + o)].into_iter().flatten() {
                if c >= self.b - ON_LINE_Q {
                    return Some(self.b);
                }
                if c > q {
                    return Some(c);
                }
            }
        }
        Some(self.b)
    }
}

impl Shape {
    /// **The run of lines that bar `bar` is in.**
    ///
    /// Below a bar, the bar itself: the rung of that bar's own signature,
    /// stepped from its own line, so a bar after a 3/4 bar is not a beat out
    /// for the rest of the song and a triplet run or swung pair the bar cuts
    /// short starts again on the next one. A bar or more, a line every
    /// `2^exp` bars counted from the first, and a triplet or a swung pair
    /// runs over two of those steps - three in the space of two, a pair of
    /// two - from every other one.
    fn span_at_bar(self, meter: &MeterMap, bar: u32) -> Span {
        let paired = self.triplet || self.swing.is_some();
        let (lo, hi, rung) = if self.exp < 0 {
            let sig = meter.sig_at_bar(bar);
            let rung = crate::arrangement_panel::grid_quarters(self.exp, sig.beats, sig.unit);
            (bar, bar.saturating_add(1), Some(rung))
        } else {
            let every = 1u32 << self.exp.min(6);
            let run = if paired { every * 2 } else { every };
            let lo = bar / run * run;
            (lo, lo.saturating_add(run), None)
        };
        let a = meter.quarter_at_bar(lo);
        let b = meter.quarter_at_bar(hi);
        let rung = rung.unwrap_or(if paired { (b - a) / 2.0 } else { b - a });
        let (period, off) = match (self.triplet, self.swing) {
            (true, _) => (rung * 2.0 / 3.0, None),
            (false, Some(s)) => (rung * 2.0, Some(rung * 2.0 * s)),
            (false, None) => (rung, None),
        };
        Span { a, b, period, off, next_bar: hi }
    }

    /// The run of lines `q` stands in.
    fn span(self, meter: &MeterMap, q: f64) -> Span {
        self.span_at_bar(meter, meter.bar_beat_at_quarter(q).0)
    }

    /// The line at or before `q`.
    fn floor(self, meter: &MeterMap, q: f64) -> f64 {
        self.span(meter, q).floor(q)
    }

    /// The first line after `q`.
    fn next(self, meter: &MeterMap, q: f64) -> f64 {
        let span = self.span(meter, q);
        span.next(q).unwrap_or_else(|| {
            // `q` is on `b` itself, which the meter's walk put in the bar
            // before by a hair: the next run has the answer.
            let after = self.span_at_bar(meter, span.next_bar);
            after.next(q).unwrap_or(after.b)
        })
    }

    /// **The line nearest `q`.**
    ///
    /// **A tie goes UP**, which is what `f64::round` does and what every
    /// other snap in this app therefore does. Landing a dead-centre drop on
    /// the earlier line instead is a half-bar jump backwards that only shows
    /// on exact midpoints - the hardest kind of inconsistency to notice and
    /// the most annoying to work with.
    fn nearest(self, meter: &MeterMap, q: f64) -> f64 {
        let (lo, hi) = (self.floor(meter, q), self.next(meter, q));
        if q - lo < hi - q { lo } else { hi }
    }
}

/// **The arrangement's magnet**: the line of a straight rung-`exp` grid
/// nearest `q`, in quarter notes on `meter`, for a finite `q`. The walk a
/// [`Grid`] makes, so the arrangement, the MIDI editor and a quantize land
/// on one set of lines; `TangentApp::snap_quarter` is this.
#[must_use]
pub fn nearest_quarter(meter: &MeterMap, q: f64, exp: i8) -> f64 {
    Shape { exp, triplet: false, swing: None }.nearest(meter, q)
}

impl Grid {
    /// The narrowest grid worth aiming at. Below a millisecond the step is
    /// smaller than the jitter of the thing being quantized, so a "quantize"
    /// to it is a no-op that costs an undo entry.
    const MIN_STEP: f64 = 0.001;

    /// How many lines [`Self::lines`] hands out at most: the arrangement's
    /// own bound on a walk of its lines.
    const MAX_LINES: usize = 4_096;

    /// **The song's grid**: the rung `grid_exp` on the song's tempo and
    /// meter maps, optionally triplet or swung.
    ///
    /// `triplet` and `swing` are mutually exclusive in this app - a swung
    /// triplet grid is two answers to one question - and the editor already
    /// treats them that way, so triplet wins here for the same reason it
    /// wins there. Outside 0.5..=0.95 swing stops meaning swing and is
    /// dropped: at 0.5 it is the straight grid it already had, and past 0.95
    /// the off-beat has arrived at the next on-beat.
    ///
    /// Returns `None` rather than a grid with a step under a millisecond
    /// anywhere in the song: a caller that asked for one has a bug upstream,
    /// and silently giving it a working grid hides that bug until somebody
    /// wonders why quantize did nothing.
    #[must_use]
    pub fn new(
        tempo: TempoMap,
        meter: MeterMap,
        grid_exp: i8,
        triplet: bool,
        swing: Option<f64>,
    ) -> Option<Self> {
        let grid = Self {
            tempo,
            meter,
            shape: Shape {
                exp: grid_exp,
                triplet,
                swing: swing.filter(|s| !triplet && s.is_finite() && (0.5..=0.95).contains(s)),
            },
        };
        (grid.finest_step_s() >= Self::MIN_STEP).then_some(grid)
    }

    /// **One tempo and one meter for the whole song**: [`Self::new`] on a
    /// constant map of each, for a caller with no song to ask.
    #[must_use]
    pub fn from_document(
        grid_exp: i8,
        beats: u8,
        unit: u8,
        bpm: f64,
        triplet: bool,
        swing_pct: Option<f64>,
    ) -> Option<Self> {
        Self::new(
            TempoMap::constant(bpm),
            MeterMap::constant(crate::recorder::TimeSignature { beats, unit }),
            grid_exp,
            triplet,
            swing_pct,
        )
    }

    /// The finest step anywhere in the song, in seconds: the finest rung
    /// any signature in the meter takes, at the fastest tempo in the map (a
    /// ramp is fastest at one of its ends).
    fn finest_step_s(&self) -> f64 {
        let fastest = self.tempo.points().iter().fold(0.0_f64, |m, p| m.max(p.bpm));
        let finest = self.meter.points().iter().fold(f64::INFINITY, |m, p| {
            let rung = crate::arrangement_panel::grid_quarters(self.shape.exp, p.sig.beats, p.sig.unit);
            m.min(if self.shape.triplet { rung * 2.0 / 3.0 } else { rung })
        });
        finest * 60.0 / fastest.max(1.0)
    }

    /// Whether this grid is swung, and by how much.
    #[must_use]
    pub fn swing(&self) -> Option<f64> {
        self.shape.swing
    }

    /// **One step at `t`**, in seconds: the rung of the bar `t` is in (two
    /// thirds of it on a triplet grid) at the tempo from `t` on. A swung
    /// grid's step is its straight one; its pair is two. What the editor
    /// nudges, duplicates and resizes by.
    #[must_use]
    pub fn step_at(&self, t: f64) -> f64 {
        let q = self.tempo.beat_at_seconds(t);
        let span = self.shape.span(&self.meter, q);
        let step = if self.shape.swing.is_some() { span.period / 2.0 } else { span.period };
        self.tempo.seconds_at_beat(q + step) - self.tempo.seconds_at_beat(q)
    }

    /// **The line nearest `t`.**
    ///
    /// The nearest in quarter notes - the arrangement's magnet is the same
    /// walk (see the module header) - with a swung grid's off-beats and a
    /// triplet grid's thirds among the lines. The editor's magnet IS this
    /// function - `midi_editor::snap_t` delegates through `view_grid` - and
    /// `a_full_strength_quantize_lands_where_the_magnet_would` holds that
    /// wiring.
    #[must_use]
    pub fn nearest(&self, t: f64) -> f64 {
        if !t.is_finite() {
            return t;
        }
        let q = self.shape.nearest(&self.meter, self.tempo.beat_at_seconds(t));
        self.tempo.seconds_at_beat(q)
    }

    /// **The line at or before `t`**: where a note is created, and where a
    /// walk back lands. A caller that counts a line just past `t` as the one
    /// `t` stands on adds that tolerance itself (`midi_editor::SNAP_EPS_S`).
    #[must_use]
    pub fn floor(&self, t: f64) -> f64 {
        if !t.is_finite() {
            return t;
        }
        let q = self.shape.floor(&self.meter, self.tempo.beat_at_seconds(t));
        self.tempo.seconds_at_beat(q)
    }

    /// **The first line after `t`**: how far a note drawn on a line runs,
    /// and where a walk forward lands.
    #[must_use]
    pub fn next(&self, t: f64) -> f64 {
        if !t.is_finite() {
            return t;
        }
        let q = self.shape.next(&self.meter, self.tempo.beat_at_seconds(t));
        self.tempo.seconds_at_beat(q)
    }

    /// **Every line from the one at or before `from` up to `to`**, in
    /// order: `f(t, is_bar)`, where `is_bar` says the line is a bar line
    /// too. Bounded, so a zoomed-out window cannot ask for millions.
    pub fn lines(&self, from: f64, to: f64, mut f: impl FnMut(f64, bool)) {
        if !from.is_finite() || !to.is_finite() || to < from {
            return;
        }
        let end = self.tempo.beat_at_seconds(to) + ON_LINE_Q;
        let mut q = self.shape.floor(&self.meter, self.tempo.beat_at_seconds(from));
        for _ in 0..Self::MAX_LINES {
            if q > end {
                break;
            }
            let bar = self.meter.bar_beat_at_quarter(q + ON_LINE_Q).0;
            let is_bar = (q - self.meter.quarter_at_bar(bar)).abs() <= ON_LINE_Q;
            f(self.tempo.seconds_at_beat(q), is_bar);
            q = self.shape.next(&self.meter, q);
        }
    }
}

/// **A quantize: where to aim, how hard, and what to leave alone.**
#[derive(Debug, Clone, PartialEq)]
pub struct Quantize {
    /// The lines to aim at.
    pub grid: Grid,
    /// How far of the distance to travel, 0 to 1. See the module header.
    pub strength: f64,
    /// Leave an event further than this from every line exactly where it is.
    /// `None` moves everything, which is what a hand asking for 100% on a
    /// whole clip usually means.
    pub window: Option<f64>,
}

impl Quantize {
    /// A full-strength quantize with no window: the plain "put it on the
    /// grid", with no knobs to turn. Only tests build one today: the
    /// quantize window's Apply states every knob.
    #[must_use]
    pub fn full(grid: Grid) -> Self {
        Self { grid, strength: 1.0, window: None }
    }

    /// **Where `t` should go.**
    ///
    /// Returns `t` unchanged when the event is outside the window or the
    /// strength is zero, so a caller can compare and skip the edit entirely
    /// rather than writing an undo entry for a move of nothing.
    #[must_use]
    pub fn place(&self, t: f64) -> f64 {
        if !t.is_finite() {
            return t;
        }
        let target = self.grid.nearest(t);
        if let Some(w) = self.window {
            if (target - t).abs() > w {
                return t;
            }
        }
        let strength = self.strength.clamp(0.0, 1.0);
        t + (target - t) * strength
    }

    /// [`place`](Self::place) in FRAMES, for a caller that counts them.
    /// Nothing outside this module's tests calls it: notes are held in
    /// seconds, and the host's `quantize_map` places seconds too.
    ///
    /// The rate arrives here rather than living in the grid for the reason
    /// [`Grid`]'s own comment gives: the grid is musical and the rate is not.
    /// Saturating at zero because nothing plays before the start of the
    /// timeline, and a quantize that pulled the first note of a take to a
    /// negative frame would silently drop it.
    #[must_use]
    pub fn place_frames(&self, at: u64, rate: u32) -> u64 {
        if rate == 0 {
            return at;
        }
        let rate = f64::from(rate);
        let moved = self.place(at as f64 / rate) * rate;
        if moved <= 0.0 { 0 } else { moved.round() as u64 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 120 bpm in 4/4: a quarter is half a second, so the numbers below are
    /// readable by eye rather than by calculator.
    fn quarter_at_120() -> Grid {
        Grid::from_document(-2, 4, 4, 120.0, false, None).expect("a quarter-note grid")
    }

    /// **A grid that cannot be aimed at is refused, not repaired.** The
    /// maps cannot say nonsense - they are sanitised where they are built -
    /// so what is left is a step too fine to mean anything: a 128th in 1/32
    /// at 300 bpm is a fifth of a millisecond, and a song that reaches it
    /// anywhere is refused whole.
    #[test]
    fn a_nonsense_grid_is_refused_rather_than_clamped() {
        let fine = crate::recorder::TimeSignature { beats: 1, unit: 32 };
        let at_300 = TempoMap::constant(300.0);
        assert!(
            Grid::new(at_300, MeterMap::constant(fine), -7, false, None).is_none(),
            "a sub-millisecond step built a grid"
        );
        let to_300 = TempoMap::sanitised(vec![
            crate::tempo::TempoPoint { at_beat: 0.0, bpm: 60.0, ramp: false },
            crate::tempo::TempoPoint { at_beat: 64.0, bpm: 300.0, ramp: false },
        ]);
        assert!(
            Grid::new(to_300.clone(), MeterMap::constant(fine), -7, false, None).is_none(),
            "a song that reaches a sub-millisecond step only at its fastest built a grid"
        );
        assert!(
            Grid::new(to_300, MeterMap::constant(fine), -4, false, None).is_some(),
            "a grid of more than a millisecond everywhere was refused"
        );
        // And swing outside the range where it means anything is dropped,
        // leaving a straight grid rather than a broken one.
        let swung = |s: f64| Grid::from_document(-3, 4, 4, 120.0, false, Some(s)).and_then(|g| g.swing());
        assert_eq!(swung(0.2), None);
        assert_eq!(swung(0.99), None);
        assert_eq!(swung(0.67), Some(0.67));
    }

    /// **Strength is a partial move, which is the whole point of it.**
    ///
    /// Full quantize is the special case. A take played 40 ms late and
    /// quantized at half strength should end up 20 ms late - still human,
    /// closer to the beat - and the arithmetic has to be the distance, not a
    /// blend of two positions, or a note already ON the line would drift.
    #[test]
    fn strength_moves_part_of_the_way_and_leaves_a_note_on_the_line_alone() {
        let g = quarter_at_120();
        let q = |strength: f64| Quantize { grid: g.clone(), strength, window: None };

        // 40 ms late of the 1.0 s line.
        let late = 1.04;
        assert!((q(1.0).place(late) - 1.0).abs() < 1e-9, "full strength did not land");
        assert!((q(0.5).place(late) - 1.02).abs() < 1e-9, "half strength moved the wrong distance");
        assert!((q(0.0).place(late) - late).abs() < 1e-9, "zero strength moved the note");

        // A note already on the line does not move at ANY strength.
        for s in [0.0, 0.25, 0.5, 1.0] {
            assert!((q(s).place(1.0) - 1.0).abs() < 1e-9, "strength {s} moved a note that was on the line");
        }
    }

    /// **The window is what keeps a quantize from eating an ornament.**
    ///
    /// A grace note 120 ms before the beat is not a late note; it is a
    /// different note. Without a window a quantize collapses it onto the
    /// beat it decorates, and no undo brings the performance back.
    #[test]
    fn an_event_outside_the_window_is_left_exactly_where_it_is() {
        let g = quarter_at_120();
        let q = Quantize { grid: g, strength: 1.0, window: Some(0.05) };
        // 20 ms out: inside the window, so it lands.
        assert!((q.place(1.02) - 1.0).abs() < 1e-9, "an event inside the window did not move");
        // 120 ms out: outside, so it is untouched - to the bit, not to a
        // tolerance, because "left alone" has to mean left alone.
        let grace = 0.88;
        assert_eq!(q.place(grace), grace, "the window let a grace note be eaten");
    }

    /// **A swung grid quantizes to the swung lines, not the straight ones
    /// underneath them.**
    #[test]
    fn swing_aims_at_the_swung_off_beat() {
        // Eighths at 120 bpm: a straight eighth is 0.25 s, a pair is 0.5 s,
        // and a 2:1 swing puts the off-beat two thirds through the pair.
        let g = Grid::from_document(-3, 4, 4, 120.0, false, Some(2.0 / 3.0))
            .expect("a swung eighth grid");
        // Just after the first third: the nearest swung line is the off-beat
        // at 0.3333, NOT the straight eighth at 0.25.
        let played = 0.32;
        let landed = Quantize::full(g.clone()).place(played);
        assert!(
            (landed - 1.0 / 3.0).abs() < 1e-9,
            "a swung grid put the note on the straight eighth: {landed}"
        );
        // And the on-beats of each pair are still where they always were.
        assert!((Quantize::full(g).place(0.49) - 0.5).abs() < 1e-9, "the pair head moved");
    }

    /// **A triplet grid is three in the space of the next size up**, and a
    /// triplet grid is never swung - two answers to one question.
    #[test]
    fn a_triplet_grid_is_three_in_the_space_of_two_and_never_swung() {
        let straight = Grid::from_document(-3, 4, 4, 120.0, false, None).expect("eighths");
        let triplet = Grid::from_document(-3, 4, 4, 120.0, true, Some(2.0 / 3.0))
            .expect("eighth triplets");
        assert!(
            (triplet.step_at(0.0) - straight.step_at(0.0) * 2.0 / 3.0).abs() < 1e-12,
            "a triplet step is not two thirds of the straight one"
        );
        assert_eq!(triplet.swing(), None, "a triplet grid took a swing as well");
    }

    /// **A bar line is a line, whatever the grid does inside the bar.**
    /// Triplet runs and swung pairs were counted from 0:00, so wherever a
    /// bar holds an odd number of them every other downbeat fell between
    /// two lines and a note played on it was moved off it. Each run now
    /// starts again at its bar line.
    #[test]
    fn a_bar_line_is_a_line_whatever_the_grid_does_inside_the_bar() {
        // 3/4 at 120: a bar is 1.5 s and holds four and a half quarter-note
        // triplets.
        let q = Quantize::full(Grid::from_document(-1, 3, 4, 120.0, true, None).expect("a grid"));
        assert!((q.place(1.5) - 1.5).abs() < 1e-9, "bar 2's downbeat was moved to {}", q.place(1.5));
        // 7/8 at 120 with swung eighths: a bar is 1.75 s, three and a half
        // pairs, and its downbeat went to the swung off-beat at 1.8 s.
        let q = Quantize::full(Grid::from_document(-1, 7, 8, 120.0, false, Some(0.6)).expect("a grid"));
        assert!((q.place(1.75) - 1.75).abs() < 1e-9, "bar 2's downbeat was moved to {}", q.place(1.75));
        // And inside bar 2 the run starts again from its line: its first
        // swung off-beat is 0.3 s after the downbeat.
        assert!((q.place(2.04) - 2.05).abs() < 1e-9, "bar 2's first off-beat is at {}", q.place(2.04));
    }

    /// **The lines are the song's**, through a tempo change and a meter
    /// change: each bar is stepped in its own signature's rung from its own
    /// line, and turned into seconds by the tempo map, so every answer -
    /// the lines walked, the nearest, the one before and the one after, and
    /// how long a step is - is the arrangement's.
    #[test]
    fn the_grid_follows_the_songs_tempo_and_meter_maps() {
        use crate::tempo::{MeterPoint, TempoPoint};
        // 120 for four bars, then 90; two bars of 4/4, one of 3/4, then 4/4.
        let tempo = TempoMap::sanitised(vec![
            TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: false },
            TempoPoint { at_beat: 16.0, bpm: 90.0, ramp: false },
        ]);
        let sig = |beats: u8| crate::recorder::TimeSignature { beats, unit: 4 };
        let meter = MeterMap::sanitised(vec![
            MeterPoint { at_bar: 0, sig: sig(4) },
            MeterPoint { at_bar: 2, sig: sig(3) },
            MeterPoint { at_bar: 3, sig: sig(4) },
        ]);
        let g = Grid::new(tempo.clone(), meter, -1, false, None).expect("a half-note grid");
        let at = |q: f64| tempo.seconds_at_beat(q);
        // Half notes in 4/4, the beat in the 3/4 bar (its own rung), and half
        // notes again from bar 4's line at quarter 11, not from 0:00.
        let want = [
            (0.0, true), (2.0, false), (4.0, true), (6.0, false),
            (8.0, true), (9.0, false), (10.0, false),
            (11.0, true), (13.0, false), (15.0, true), (17.0, false), (19.0, true),
        ];
        let mut got = Vec::new();
        g.lines(0.0, at(19.0), |t, bar| got.push((t, bar)));
        assert_eq!(got.len(), want.len(), "the lines walked: {got:?}");
        for (&(t, bar), &(q, is_bar)) in got.iter().zip(&want) {
            assert!((t - at(q)).abs() < 1e-9 && bar == is_bar, "a line at {t} ({bar}), wanted quarter {q} ({is_bar})");
        }
        // Beat 17, after the change to 90, is 8 + 2/3 s.
        assert!((at(17.0) - (8.0 + 60.0 / 90.0)).abs() < 1e-9, "fixture: beat 17 is at {}", at(17.0));
        assert!((g.nearest(at(17.0) - 0.2) - at(17.0)).abs() < 1e-9, "nearest missed beat 17");
        assert!((g.floor(at(17.0) + 0.2) - at(17.0)).abs() < 1e-9, "floor missed beat 17");
        assert!((g.next(at(15.0)) - at(17.0)).abs() < 1e-9, "next from beat 15 missed beat 17");
        assert!((g.next(at(10.0)) - at(11.0)).abs() < 1e-9, "next from the 3/4 bar's last line missed the bar line");
        // A step is the rung where it is asked, at the tempo there.
        assert!((g.step_at(at(1.0)) - 1.0).abs() < 1e-9, "a half note at 120 is a second");
        assert!((g.step_at(at(8.5)) - 0.5).abs() < 1e-9, "the 3/4 bar steps its beat");
        assert!((g.step_at(at(17.5)) - 4.0 / 3.0).abs() < 1e-9, "a half note at 90 is 4/3 s");
    }

    /// **Frames in, frames out, and never before the start of the timeline.**
    #[test]
    fn a_frame_quantize_cannot_push_a_note_off_the_front() {
        let g = quarter_at_120();
        let q = Quantize::full(g);
        // 48_000 frames at 48 kHz is 1.0 s, already on the line.
        assert_eq!(q.place_frames(48_000, 48_000), 48_000);
        // A note just after zero pulls BACK to zero, not past it.
        assert_eq!(q.place_frames(500, 48_000), 0, "a note was pushed off the front");
        // And a rate of zero is refused rather than dividing by it.
        assert_eq!(q.place_frames(500, 0), 500);
    }
}
