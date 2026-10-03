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
//! Not from here. [`Grid::from_document`] takes a grid exponent, ONE meter,
//! ONE tempo, triplet and swing, and asks
//! [`arrangement_panel::grid_quarters`](crate::arrangement_panel::grid_quarters) -
//! the one authority on what a grid step IS - how long a step is.
//!
//! **A `Grid` is uniform from 0:00**: its lines stand at `k * step`, and
//! swung pairs and triplet runs are counted from 0:00 too. That is the grid
//! the arrangement draws only while the song's tempo and meter are constant,
//! and - for triplet and swing - only while a bar holds a whole number of
//! steps (of pairs, when swung). Both callers (`quantize_ask_of` and
//! `quantize_window_grid`) hand it the session's one `tempo_bpm()` and one
//! `time_signature()`, so after a tempo or meter change a quantize aims at
//! lines nobody can see.
//!
//! Whoever LABELS a grid has to spell it in the meter it quantizes in -
//! `grid_label_text` with the same `(beats, unit)` - or it is a magnet
//! pointing somewhere the eye cannot see, and this app has already paid for
//! that once: the box said "1/2" while the magnet snapped eighths. The
//! quantize window's GRID chips are spelled in the song's meter, read on
//! every frame the window is drawn.
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

/// **Where the lines are**, in seconds on the timeline.
///
/// Seconds rather than frames because the grid is a musical quantity and the
/// sample rate is not: the same bar is the same bar at 44.1 and at 96, and a
/// project that changes device should not quantize differently. Callers
/// working in frames convert at their own edge, where they know the rate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    /// One step, seconds. Always positive; a non-positive step is refused at
    /// construction because every operation here divides by it.
    step: f64,
    /// Where the off-beat of each PAIR of steps sits, as a fraction of the
    /// pair — `None` for a straight grid. Held as the fraction and not as a
    /// percentage because everything downstream multiplies by it.
    swing: Option<f64>,
}

impl Grid {
    /// The narrowest grid worth aiming at. Below a millisecond the step is
    /// smaller than the jitter of the thing being quantized, so a "quantize"
    /// to it is a no-op that costs an undo entry.
    const MIN_STEP: f64 = 0.001;

    /// A grid of `step` seconds, optionally swung.
    ///
    /// Returns `None` rather than clamping a nonsense step: a caller that
    /// computed zero has a bug upstream, and silently giving it a working
    /// grid hides that bug until somebody wonders why quantize did nothing.
    #[must_use]
    pub fn new(step: f64, swing: Option<f64>) -> Option<Self> {
        if !step.is_finite() || step < Self::MIN_STEP {
            return None;
        }
        Some(Self {
            step,
            // Outside this range swing stops meaning swing: at 0.5 it is the
            // straight grid it already had, and past 0.95 the off-beat has
            // arrived at the next on-beat.
            swing: swing.filter(|s| s.is_finite() && (0.5..=0.95).contains(s)),
        })
    }

    /// **The grid the document is drawing**, in the document's own terms -
    /// while its tempo and meter are constant: this takes one of each (see
    /// the module header).
    ///
    /// `triplet` and `swing` are mutually exclusive in this app — a swung
    /// triplet grid is two answers to one question — and the editor already
    /// treats them that way, so triplet wins here for the same reason it
    /// wins there.
    #[must_use]
    pub fn from_document(
        grid_exp: i8,
        beats: u8,
        unit: u8,
        bpm: f64,
        triplet: bool,
        swing_pct: Option<f64>,
    ) -> Option<Self> {
        let bpm = bpm.clamp(20.0, 400.0);
        let quarters = crate::arrangement_panel::grid_quarters(grid_exp, beats, unit);
        let step = quarters * (60.0 / bpm);
        // A triplet divides the next size UP into three, which is the 2/3 —
        // the editor's own words, and the same constant.
        let step = if triplet { step * 2.0 / 3.0 } else { step };
        Self::new(step, if triplet { None } else { swing_pct })
    }

    /// One step, seconds.
    #[must_use]
    pub fn step(self) -> f64 {
        self.step
    }

    /// Whether this grid is swung, and by how much.
    #[must_use]
    pub fn swing(self) -> Option<f64> {
        self.swing
    }

    /// **The line nearest `t`.**
    ///
    /// The straight case is a round. The swung case walks the three
    /// candidates that can straddle `t` — the pair's head, its swung
    /// off-beat, and the next pair's head. The editor's magnet IS this
    /// function - `midi_editor::snap_t` delegates through `view_grid` - and
    /// `a_full_strength_quantize_lands_where_the_magnet_would` holds that
    /// wiring.
    #[must_use]
    pub fn nearest(self, t: f64) -> f64 {
        if !t.is_finite() {
            return t;
        }
        let Some(swing) = self.swing else {
            return (t / self.step).round() * self.step;
        };
        let pair = self.step * 2.0;
        let base = (t / pair).floor() * pair;
        let mut best = base;
        for c in [base, base + pair * swing, base + pair] {
            if (t - c).abs() < (t - best).abs() {
                best = c;
            }
        }
        best
    }
}

/// **A quantize: where to aim, how hard, and what to leave alone.**
#[derive(Debug, Clone, Copy, PartialEq)]
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
    pub fn place(self, t: f64) -> f64 {
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
    pub fn place_frames(self, at: u64, rate: u32) -> u64 {
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

    /// **A grid that cannot be aimed at is refused, not repaired.**
    #[test]
    fn a_nonsense_grid_is_refused_rather_than_clamped() {
        assert!(Grid::new(0.0, None).is_none(), "a zero step built a grid");
        assert!(Grid::new(-1.0, None).is_none(), "a negative step built a grid");
        assert!(Grid::new(f64::NAN, None).is_none(), "NaN built a grid");
        assert!(Grid::new(0.0005, None).is_none(), "a sub-millisecond step built a grid");
        // And swing outside the range where it means anything is dropped,
        // leaving a straight grid rather than a broken one.
        assert_eq!(Grid::new(0.5, Some(0.2)).and_then(Grid::swing), None);
        assert_eq!(Grid::new(0.5, Some(0.99)).and_then(Grid::swing), None);
        assert_eq!(Grid::new(0.5, Some(0.67)).and_then(Grid::swing), Some(0.67));
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
        let q = |strength: f64| Quantize { grid: g, strength, window: None };

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
        let landed = Quantize::full(g).place(played);
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
            (triplet.step() - straight.step() * 2.0 / 3.0).abs() < 1e-12,
            "a triplet step is not two thirds of the straight one"
        );
        assert_eq!(triplet.swing(), None, "a triplet grid took a swing as well");
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
