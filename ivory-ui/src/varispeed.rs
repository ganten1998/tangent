//! The varispeed dial: one control that plays the whole desk fast or slow,
//! with the pitch following the speed.
//!
//! **This is a tape control, not a time-stretch.** Turning it up does not
//! leave the pitch where it was — it plays everything faster and everything
//! comes out sharper, exactly as running a tape machine above nominal does,
//! which is what the word varispeed has always meant. The clip stretcher is
//! the other thing and is still the other thing: that one keeps the pitch and
//! moves the time, this one moves both together because they are the same
//! movement.
//!
//! The dial has no switch. **Twelve o'clock is off**, and the way to engage it
//! is to move off twelve o'clock — which is why the arithmetic below is built
//! around the centre rather than around the ends, and why a detent puts
//! exactly 100 back under the hand instead of leaving 99.97 there for ever.

/// Normal speed, as a percentage. The dial reads this at twelve o'clock and
/// nowhere else.
pub const OFF: f32 = 100.0;

/// The slowest and fastest the dial may EVER be set to reach, by the owner's
/// word: "down to 50 optionally and up to 200 (2x) optionally. no more no
/// less." A custom range is chosen inside these; nothing widens them.
pub const HARD_MIN: u16 = 50;
pub const HARD_MAX: u16 = 200;

/// The range out of the box: thirty percent, centred. Full down is −15%, full
/// up is +15%.
pub const DEFAULT_MIN: u16 = 85;
pub const DEFAULT_MAX: u16 = 115;

/// How close to normal counts AS normal.
///
/// **A dial with no detent is a dial you cannot switch off.** Twelve o'clock
/// is a single point in a continuum of pixels, so without this a hand aiming
/// at it lands on 99.97% — inaudible, indistinguishable on the readout, and
/// permanently *engaged*, which means the whole resampling path stays in
/// circuit and every export goes through it for nothing.
///
/// Half a percent, which is about two pixels of travel on the default range
/// and under a tenth of a semitone. Wide enough to hit, narrow enough that
/// nobody aiming at a real value gets snapped off it.
const DETENT: f32 = 0.5;

/// Where the dial is and how far it reaches.
///
/// Two numbers the user owns — the range — and one the hand moves. The
/// position is kept as the 0..=1 the knob draws and drags in, and the
/// percentage is DERIVED, because the range can change under a position that
/// has not moved and the reverse would make a stored percentage drift every
/// time somebody opened the range fields.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Varispeed {
    /// 0..=1, and 0.5 is twelve o'clock. The knob's own unit.
    pos: f32,
    /// Full down, as a percentage. `HARD_MIN..=100`.
    min: u16,
    /// Full up, as a percentage. `100..=HARD_MAX`.
    max: u16,
}

impl Default for Varispeed {
    fn default() -> Self {
        Self { pos: 0.5, min: DEFAULT_MIN, max: DEFAULT_MAX }
    }
}

impl Varispeed {
    /// The centre of travel, which is where OFF lives.
    pub const CENTRE: f32 = 0.5;

    /// A dial with a given range, at rest.
    pub fn with_range(min: u16, max: u16) -> Self {
        let mut v = Self::default();
        v.set_min(min);
        v.set_max(max);
        v
    }

    pub fn pos(self) -> f32 {
        self.pos
    }

    pub fn min(self) -> u16 {
        self.min
    }

    pub fn max(self) -> u16 {
        self.max
    }

    /// Move the dial. The knob's drag, the arrow keys and a restore all land
    /// here, so the detent is applied in one place.
    pub fn set_pos(&mut self, pos: f32) {
        let p = if pos.is_finite() { pos.clamp(0.0, 1.0) } else { Self::CENTRE };
        // Snap in PERCENT, not in travel: a wide range and a narrow one put
        // very different numbers of pixels either side of normal, and a
        // detent measured in pixels would be half a percent wide on one and
        // three percent wide on the other.
        self.pos = if (Self::percent_of(p, self.min, self.max) - OFF).abs() <= DETENT {
            Self::CENTRE
        } else {
            p
        };
    }

    /// What the dial says, as a percentage of normal speed.
    pub fn percent(self) -> f32 {
        Self::percent_of(self.pos, self.min, self.max)
    }

    /// What the engine multiplies time by. 1.0 is off.
    ///
    /// `f64` because it is a rate the audio thread accumulates across a
    /// session: at `f32` a phase accumulator running for an hour at 44.1 kHz
    /// has lost the low bits of the fraction long before then.
    pub fn ratio(self) -> f64 {
        f64::from(self.percent()) / 100.0
    }

    /// **Off is a position, not a switch.** Anything but dead centre is
    /// engaged, and dead centre is reachable because of [`DETENT`].
    pub fn engaged(self) -> bool {
        (self.percent() - OFF).abs() > f32::EPSILON
    }

    /// Put the dial where it reads `pct`, for a typed-in number and for a
    /// range change that has to leave the speed alone.
    ///
    /// Out of range is CLAMPED rather than refused: somebody typing 150 into
    /// a dial that only reaches 115 has said "as fast as it goes", and a
    /// field that answers by doing nothing reads as a field that is broken.
    pub fn set_percent(&mut self, pct: f32) {
        let p = if pct.is_finite() { pct } else { OFF };
        let lo = f32::from(self.min);
        let hi = f32::from(self.max);
        let pct = p.clamp(lo, hi);
        let pos = if pct >= OFF {
            // A range whose top IS normal has no upper half to travel; the
            // division would be by zero and the answer is the centre.
            if hi > OFF { Self::CENTRE + (pct - OFF) / (hi - OFF) * 0.5 } else { Self::CENTRE }
        } else if lo < OFF {
            Self::CENTRE - (OFF - pct) / (OFF - lo) * 0.5
        } else {
            Self::CENTRE
        };
        self.set_pos(pos);
    }

    /// Widen or narrow the bottom of the range, **keeping the speed**.
    ///
    /// The alternative — keeping the POSITION — means opening the range
    /// fields, changing the number the dial does not currently sit on, and
    /// hearing the pitch move. The user set a speed; a range is the reach of
    /// the control, not the value of it.
    pub fn set_min(&mut self, min: u16) {
        let was = self.percent();
        self.min = min.clamp(HARD_MIN, 100);
        self.set_percent(was);
    }

    /// The top of the range. See [`Varispeed::set_min`].
    ///
    /// **The two ends cannot cross, because normal is between them by
    /// construction**: the bottom is clamped at or below 100 and the top at
    /// or above it. That is not a tidiness rule — it is what makes twelve
    /// o'clock mean OFF on every range anybody can dial in, which is the
    /// whole gesture this control has instead of a switch.
    pub fn set_max(&mut self, max: u16) {
        let was = self.percent();
        self.max = max.clamp(100, HARD_MAX);
        self.set_percent(was);
    }

    /// Back to dead centre. The one gesture that is always available and
    /// always means "stop doing this".
    pub fn disengage(&mut self) {
        self.pos = Self::CENTRE;
    }

    /// What the readout over the dial says.
    ///
    /// **Signed, and relative to normal.** `+15.0%` says what the control is
    /// doing; `115.0%` says what the tape is doing, and the two are the same
    /// number wearing different meanings. The dial is a trim about a centre —
    /// it says so by being centred, by having a detent, and by reading zero
    /// when it is off — so the readout says the same thing.
    pub fn reading(self) -> String {
        let d = self.percent() - OFF;
        if d.abs() <= f32::EPSILON {
            "0.0%".to_owned()
        } else {
            format!("{d:+.1}%")
        }
    }

    /// How far off normal in SEMITONES, for the status line.
    ///
    /// Pitch follows speed, so doubling the speed is an octave: the interval
    /// is `12·log2(ratio)` and not anything proportional to the percentage.
    /// +15% is +2.4 semitones, which is nearly a whole tone and is worth
    /// somebody knowing before they record over a take with it engaged.
    pub fn semitones(self) -> f32 {
        let r = self.ratio();
        if r > 0.0 {
            12.0 * (r.log2() as f32)
        } else {
            0.0
        }
    }

    fn percent_of(pos: f32, min: u16, max: u16) -> f32 {
        let (lo, hi) = (f32::from(min), f32::from(max));
        let p = pos.clamp(0.0, 1.0);
        if p >= Self::CENTRE {
            OFF + (p - Self::CENTRE) * 2.0 * (hi - OFF)
        } else {
            OFF - (Self::CENTRE - p) * 2.0 * (OFF - lo)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn twelve_oclock_is_off_on_every_range_anybody_can_dial_in() {
        // The control has no switch. If twelve o'clock ever stopped meaning
        // normal speed, there would be no way to turn it off at all.
        for (lo, hi) in [(85, 115), (50, 200), (50, 100), (100, 200), (99, 101)] {
            let v = Varispeed::with_range(lo, hi);
            assert_eq!(v.percent(), OFF, "{lo}..{hi} is not off at rest");
            assert!(!v.engaged(), "{lo}..{hi} is engaged at rest");
            assert_eq!(v.ratio(), 1.0);
        }
    }

    #[test]
    fn the_default_range_is_thirty_percent_centred() {
        let mut v = Varispeed::default();
        v.set_pos(1.0);
        assert!((v.percent() - 115.0).abs() < 1e-3, "full up is {}", v.percent());
        v.set_pos(0.0);
        assert!((v.percent() - 85.0).abs() < 1e-3, "full down is {}", v.percent());
        // And the two halves are the same size, which is what "centred"
        // means and what makes the detent land in the middle of the travel.
        v.set_pos(0.75);
        assert!((v.percent() - 107.5).abs() < 1e-3);
        v.set_pos(0.25);
        assert!((v.percent() - 92.5).abs() < 1e-3);
    }

    #[test]
    fn an_asymmetric_range_still_reads_normal_at_the_centre() {
        // The point of mapping the two halves separately. A range of 50..200
        // is 50 below and 100 above; a single linear sweep across it would
        // put normal speed at a third of the way round, and twelve o'clock
        // would be playing everything a sixth flat with the dial apparently
        // at rest.
        let mut v = Varispeed::with_range(50, 200);
        assert_eq!(v.percent(), OFF);
        v.set_pos(1.0);
        assert!((v.percent() - 200.0).abs() < 1e-3);
        v.set_pos(0.0);
        assert!((v.percent() - 50.0).abs() < 1e-3);
        v.set_pos(0.25);
        assert!((v.percent() - 75.0).abs() < 1e-3, "{}", v.percent());
        v.set_pos(0.75);
        assert!((v.percent() - 150.0).abs() < 1e-3, "{}", v.percent());
    }

    #[test]
    fn the_detent_makes_off_reachable_by_hand() {
        let mut v = Varispeed::default();
        // Two pixels either side of centre on a 120-point dial.
        v.set_pos(0.5 + 1.0 / 120.0);
        assert_eq!(v.percent(), OFF, "a near miss did not snap");
        assert!(!v.engaged());
        // And a real value is NOT snapped away from.
        v.set_pos(0.6);
        assert!(v.engaged());
        assert!((v.percent() - 103.0).abs() < 1e-3, "{}", v.percent());
    }

    #[test]
    fn narrowing_the_range_keeps_the_speed_rather_than_the_position() {
        // Opening the range fields and changing a number the dial is not
        // sitting on must not move the pitch.
        let mut v = Varispeed::with_range(50, 200);
        v.set_percent(110.0);
        assert!((v.percent() - 110.0).abs() < 1e-3);
        v.set_max(115);
        assert!((v.percent() - 110.0).abs() < 1e-3, "the speed moved: {}", v.percent());
        // The POSITION did move, which is the point: the same speed is now
        // most of the way up a shorter travel.
        assert!(v.pos() > 0.8, "{}", v.pos());
        // And a range that no longer reaches the current speed clamps it,
        // because the alternative is a dial reading a number it cannot
        // produce.
        v.set_max(105);
        assert!((v.percent() - 105.0).abs() < 1e-3, "{}", v.percent());
    }

    #[test]
    fn the_hard_limits_are_the_hard_limits() {
        let mut v = Varispeed::default();
        v.set_min(0);
        assert_eq!(v.min(), HARD_MIN);
        v.set_max(1000);
        assert_eq!(v.max(), HARD_MAX);
        // **And neither end may cross normal.** A bottom above 100 or a top
        // below it would put OFF outside the dial's own travel, and twelve
        // o'clock — the only way this control has of being switched off —
        // would name a speed the dial cannot reach.
        v.set_min(150);
        assert_eq!(v.min(), 100);
        v.set_max(70);
        assert_eq!(v.max(), 100);
        assert_eq!(v.percent(), OFF, "a degenerate range must still be OFF");
        assert!(!v.engaged());
    }

    #[test]
    fn typing_a_number_puts_the_dial_on_it_and_out_of_reach_clamps() {
        let mut v = Varispeed::default();
        for want in [85.0_f32, 92.5, 100.0, 107.5, 115.0] {
            v.set_percent(want);
            assert!((v.percent() - want).abs() < 1e-3, "typed {want}, got {}", v.percent());
        }
        // "as fast as it goes", not "do nothing".
        v.set_percent(400.0);
        assert!((v.percent() - 115.0).abs() < 1e-3);
        v.set_percent(1.0);
        assert!((v.percent() - 85.0).abs() < 1e-3);
        // Nonsense lands on OFF rather than poisoning the audio thread with a
        // NaN ratio — a NaN read position resamples to silence at best.
        v.set_percent(f32::NAN);
        assert_eq!(v.percent(), OFF);
        v.set_pos(f32::NAN);
        assert_eq!(v.percent(), OFF);
        assert!(v.ratio().is_finite());
    }

    #[test]
    fn the_readout_is_a_trim_and_the_status_line_is_an_interval() {
        let mut v = Varispeed::default();
        assert_eq!(v.reading(), "0.0%");
        assert!((v.semitones() - 0.0).abs() < 1e-4);
        v.set_pos(1.0);
        assert_eq!(v.reading(), "+15.0%");
        // +15% is nearly a whole tone, which is the fact the percentage
        // hides and somebody about to record needs.
        assert!((v.semitones() - 2.42).abs() < 0.02, "{}", v.semitones());
        v.set_pos(0.0);
        assert_eq!(v.reading(), "-15.0%");
        // And the octave, which is the one interval the number 200 makes
        // obvious and the number 100 does not.
        let mut wide = Varispeed::with_range(50, 200);
        wide.set_pos(1.0);
        assert!((wide.semitones() - 12.0).abs() < 1e-3);
        wide.set_pos(0.0);
        assert!((wide.semitones() + 12.0).abs() < 1e-3);
    }

    #[test]
    fn disengage_always_lands_exactly_on_normal() {
        let mut v = Varispeed::with_range(50, 200);
        v.set_pos(0.93);
        assert!(v.engaged());
        v.disengage();
        assert!(!v.engaged());
        assert_eq!(v.ratio(), 1.0, "an engaged path left in circuit for nothing");
    }
}
