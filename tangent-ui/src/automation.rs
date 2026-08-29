//! Automation: a control's value written down as a shape over time.
//!
//! # Normalised, always
//!
//! Every point in every curve is `0.0..=1.0`, whatever the control it
//! belongs to. A fader's position, not its decibels; a pan mapped from
//! `-1..1`; a send's own fraction. Three reasons, and the third is the one
//! that would have bitten:
//!
//! 1. One interpolator, rather than one per parameter.
//! 2. It is already the currency the keyboard uses — [`crate::a11y::Travel`]
//!    is normalised for exactly this reason, so a nudge and a curve point
//!    mean the same number.
//! 3. **Several of these controls are not linear in their units.** A fader
//!    runs through a decibel curve; the tempo knob through `knob_to_tempo`.
//!    Storing "the value" would mean storing which units, and a curve read
//!    back through the wrong one moves the control somewhere it never was.
//!    That mistake has already been made once on this branch, in the
//!    keyboard nudge, where the master's amplitude was passed off as its
//!    fader position and one arrow press moved -6 dB to -20.4.
//!
//! # Seconds, not beats
//!
//! Points are in seconds, like every other position in the document. A
//! tempo edit therefore does not move automation — which is the same bargain
//! [`crate::arrangement::Anchor::Time`] makes for clips, and the right
//! default for the same reason: a curve drawn against a performance belongs
//! where it was drawn.

/// Which control a curve moves.
///
/// **An index space that is persisted**, so it is append-only like every
/// other one in this app — see `STRIP_COLORS`. A reordered variant silently
/// re-points every saved curve at a different control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AutoParam {
    /// The channel fader, as a fader POSITION — see the module header.
    Gain,
    /// Pan, with `-1..1` laid onto `0..1`; 0.5 is centre.
    Pan,
    /// The channel's send to the effects bus.
    Send,
    /// Muted above half, heard below it. A switch written as a curve,
    /// because a separate kind of lane for the switches would be a second
    /// editor to build and learn for one bit of information.
    Mute,
    /// **A parameter of the plugin in one of this lane's bays.**
    ///
    /// `bay` is the insert slot, `id` the plugin's OWN parameter id — never
    /// an index. Ids survive a plugin update and positions do not, so a curve
    /// that stored a position would point at a different knob after the user
    /// installed a new version of the same effect.
    ///
    /// Carrying the bay as well as the id is what keeps two bays holding the
    /// SAME plugin apart: their parameter ids are identical, and a curve that
    /// named only the id would move both.
    Plugin { bay: u8, id: u32 },
}

impl AutoParam {
    /// Every DESK parameter, in the order a lane offers them.
    ///
    /// The plugin ones are not here and cannot be: they depend on what is
    /// loaded in the bays right now, which is a question for
    /// `IvoryApp::fx_params` and not a constant.
    pub const ALL: [AutoParam; 4] =
        [AutoParam::Gain, AutoParam::Pan, AutoParam::Send, AutoParam::Mute];

    /// The spelling in the document. **Append-only, and never re-spelled:**
    /// these strings are in files people already have.
    pub fn key(self) -> String {
        match self {
            Self::Gain => "gain".to_owned(),
            Self::Pan => "pan".to_owned(),
            Self::Send => "send".to_owned(),
            Self::Mute => "mute".to_owned(),
            // **`fx:<bay>:<id>`**, and the shape is deliberate: it cannot
            // collide with any of the four names above, it is readable in a
            // hand-edited document, and it round-trips through
            // `from_key` without a table. A parameter id is a `u32` and is
            // written in decimal because that is what a plugin's own
            // documentation prints.
            Self::Plugin { bay, id } => format!("fx:{bay}:{id}"),
        }
    }

    pub fn from_key(k: &str) -> Option<Self> {
        if let Some(rest) = k.strip_prefix("fx:") {
            let (bay, id) = rest.split_once(':')?;
            return Some(Self::Plugin {
                bay: bay.parse().ok()?,
                id: id.parse().ok()?,
            });
        }
        Self::ALL.into_iter().find(|p| p.key() == k)
    }

    /// Which bay this parameter lives in, if it is a plugin's.
    pub fn bay(self) -> Option<usize> {
        match self {
            Self::Plugin { bay, .. } => Some(usize::from(bay)),
            _ => None,
        }
    }

    /// What a person calls it — the lane's label, and what a screen reader
    /// says.
    /// What a person calls it — the lane's label, and what a screen reader
    /// says.
    ///
    /// A plugin parameter has no name of its own here: the NAME lives with
    /// the plugin and reaches the app through `ports::FxParams`, so this
    /// answers with the address and the caller that has the list puts the
    /// word in. See `IvoryApp::auto_param_label`.
    pub fn label(self) -> String {
        match self {
            Self::Gain => "volume".to_owned(),
            Self::Pan => "pan".to_owned(),
            Self::Send => "send".to_owned(),
            Self::Mute => "mute".to_owned(),
            Self::Plugin { bay, id } => format!("bay {} parameter {id}", bay + 1),
        }
    }
}

/// What a lane's automation does while the transport rolls.
///
/// **Session state, never persisted, and that is a safety decision rather
/// than an oversight.** A lane left in `Write` and saved would overwrite its
/// own curve the next time anybody pressed play — the work destroyed by an
/// action nobody would think of as destructive, in a file they opened to
/// listen to. Every mode returns to [`AutoMode::Read`] when the song does.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum AutoMode {
    /// The curve is kept but not obeyed: the fader is the fader again.
    ///
    /// What somebody wants while they work out what the part should be,
    /// without throwing away what they drew.
    Off,
    /// The curve plays the control. The ordinary state, and the default.
    #[default]
    Read,
    /// Reading, until a control is TOUCHED — then writing, until it is let
    /// go, and reading again after.
    Touch,
    /// Reading, until a control is touched — then writing until the
    /// transport stops. What you want for a move you make once and mean.
    Latch,
    /// Writing from the moment the transport rolls, touched or not.
    ///
    /// The dangerous one, and the reason none of these is saved.
    Write,
}

impl AutoMode {
    /// In the order the row cycles them.
    pub const ALL: [AutoMode; 5] = [
        AutoMode::Read,
        AutoMode::Touch,
        AutoMode::Latch,
        AutoMode::Write,
        AutoMode::Off,
    ];

    /// The word on the row.
    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Read => "Read",
            Self::Touch => "Touch",
            Self::Latch => "Latch",
            Self::Write => "Write",
        }
    }

    /// Whether the curve should drive the control at all.
    pub fn reads(self) -> bool {
        !matches!(self, Self::Off)
    }

    /// Whether MOVING a control writes it down, given whether the control is
    /// being held right now and whether a latch is already armed.
    ///
    /// The whole of the mode's behaviour in one place, so the audio side and
    /// the UI side cannot come to different conclusions about it.
    pub fn writes(self, touching: bool, latched: bool) -> bool {
        match self {
            Self::Off | Self::Read => false,
            Self::Touch => touching,
            Self::Latch => touching || latched,
            Self::Write => true,
        }
    }
}

/// How a curve gets from this point to the next.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shape {
    /// A straight line — the ordinary one.
    #[default]
    Linear,
    /// Hold this value until the next point, then jump.
    ///
    /// What a switch needs, and what a fader needs when somebody wants a
    /// level to stay put rather than drift towards the next move.
    Hold,
}

impl Shape {
    pub fn key(self) -> &'static str {
        match self {
            Self::Linear => "linear",
            Self::Hold => "hold",
        }
    }

    pub fn from_key(k: &str) -> Self {
        match k {
            "hold" => Self::Hold,
            _ => Self::Linear,
        }
    }
}

/// One point on a curve.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub at_s: f64,
    /// `0.0..=1.0`, always. See the module header.
    pub value: f64,
    /// How the curve leaves this point towards the next.
    pub shape: Shape,
}

/// One control's automation over the whole song.
#[derive(Debug, Clone, PartialEq)]
pub struct Curve {
    pub lane: usize,
    pub param: AutoParam,
    /// Sorted by time, never empty once [`Curve::sanitised`] has been. A
    /// curve with no points is not written down at all — see
    /// [`Automation::set`].
    points: Vec<Point>,
}

impl Curve {
    /// Build from arbitrary points, putting them in the state every reader
    /// here assumes: sorted, finite, in range.
    ///
    /// The same bargain [`crate::tempo::TempoMap::sanitised`] makes, and for
    /// the same reason — a document is a file somebody can edit by hand, so
    /// "the file said so" guarantees nothing.
    pub fn sanitised(lane: usize, param: AutoParam, mut points: Vec<Point>) -> Self {
        points.retain(|p| p.at_s.is_finite() && p.value.is_finite() && p.at_s >= 0.0);
        for p in &mut points {
            p.value = p.value.clamp(0.0, 1.0);
        }
        points.sort_by(|a, b| {
            a.at_s.partial_cmp(&b.at_s).unwrap_or(std::cmp::Ordering::Equal)
        });
        // Two points at one instant is a step somebody drew by hand, and the
        // LAST of them wins — a later edit is the one they meant.
        //
        // Written out rather than `dedup_by`, which keeps the FIRST of each
        // run and would have quietly meant the opposite of this comment.
        let mut kept: Vec<Point> = Vec::with_capacity(points.len());
        for p in points {
            if kept.last().is_some_and(|q| (q.at_s - p.at_s).abs() < 1e-9) {
                kept.pop();
            }
            kept.push(p);
        }
        // **A curve always begins with a node at zero.**
        //
        // `value_at` is already flat before the first point, so this changes
        // nothing anyone can HEAR — the value from zero up to that first
        // point is the same either way. What it changes is what a person can
        // see and grab: a lane whose curve started at bar 40 drew a flat
        // stretch with no node on it, which reads as "nothing here yet"
        // rather than "held at this value", and there was no handle to take
        // hold of to say otherwise.
        //
        // Same shape as the point it copies, so the first segment leaves zero
        // exactly as it leaves the point it was derived from.
        let mut me = Self { lane, param, points: kept };
        me.ensure_zero();
        me
    }

    /// Put a node at zero if the curve has points and none of them is there.
    ///
    /// **Called by every mutator, not just by `sanitised`.** Seeding only on
    /// load meant the rule held for a curve read from a file and not for one
    /// somebody had just drawn — which is the case that matters, because a
    /// fresh curve is exactly the one whose beginning is unmarked.
    ///
    /// Inaudible: `value_at` is flat before the first point, so the seeded
    /// node holds the value that stretch already had.
    fn ensure_zero(&mut self) {
        if let Some(first) = self.points.first() {
            if first.at_s > 0.0 {
                let seed = Point { at_s: 0.0, value: first.value, shape: first.shape };
                self.points.insert(0, seed);
            }
        }
    }

    pub fn points(&self) -> &[Point] {
        &self.points
    }

    pub fn is_empty(&self) -> bool {
        self.points.is_empty()
    }

    /// The value at `seconds`.
    ///
    /// **Flat before the first point and after the last**, which is what
    /// makes a curve safe to add to a song that is already playing: drawing
    /// one point at bar 40 does not ramp the whole song up to it from
    /// wherever it happened to start.
    pub fn value_at(&self, seconds: f64) -> Option<f64> {
        let first = self.points.first()?;
        if seconds <= first.at_s {
            return Some(first.value);
        }
        let last = self.points.last()?;
        if seconds >= last.at_s {
            return Some(last.value);
        }
        // The segment `seconds` falls in. A binary search rather than a walk
        // because this runs on the audio thread, once per block per lane.
        let i = match self
            .points
            .binary_search_by(|p| p.at_s.partial_cmp(&seconds).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => return Some(self.points[i].value),
            Err(0) => 0,
            Err(i) => i - 1,
        };
        let a = self.points[i];
        let b = self.points[i + 1];
        match a.shape {
            Shape::Hold => Some(a.value),
            Shape::Linear => {
                let span = b.at_s - a.at_s;
                if span <= 0.0 {
                    return Some(b.value);
                }
                let t = ((seconds - a.at_s) / span).clamp(0.0, 1.0);
                Some(a.value + (b.value - a.value) * t)
            }
        }
    }

    /// Add or replace a point, keeping the curve sorted.
    ///
    /// Replaces rather than stacks when one is already at that instant:
    /// dragging a point back over its neighbour must not leave two.
    pub fn put(&mut self, point: Point) {
        let Point { at_s, value, shape } = point;
        let p = Point {
            at_s: at_s.max(0.0),
            value: value.clamp(0.0, 1.0),
            shape,
        };
        match self
            .points
            .binary_search_by(|q| q.at_s.partial_cmp(&p.at_s).unwrap_or(std::cmp::Ordering::Equal))
        {
            Ok(i) => self.points[i] = p,
            Err(i) => self.points.insert(i, p),
        }
        self.ensure_zero();
    }

    /// Take the point nearest `seconds` out, if one is within `reach`.
    pub fn remove_near(&mut self, seconds: f64, reach: f64) -> bool {
        let Some((i, _)) = self
            .points
            .iter()
            .enumerate()
            .map(|(i, p)| (i, (p.at_s - seconds).abs()))
            .filter(|(_, d)| *d <= reach)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        else {
            return false;
        };
        self.points.remove(i);
        // Taking the FIRST point away would leave the curve beginning
        // somewhere other than zero, so the next one seeds a new node there.
        // Deleting the node at zero therefore moves it rather than removing
        // it — which is what "a curve always begins at zero" has to mean if
        // it is to be true after an edit as well as before one.
        self.ensure_zero();
        true
    }
}

/// Every curve in a song.
///
/// A flat list rather than a map, because it is written to a document as a
/// JSON array and a list keeps the file's order stable across saves.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Automation {
    curves: Vec<Curve>,
}

impl Automation {
    pub fn curves(&self) -> &[Curve] {
        &self.curves
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    pub fn get(&self, lane: usize, param: AutoParam) -> Option<&Curve> {
        self.curves.iter().find(|c| c.lane == lane && c.param == param)
    }

    /// The value of one control at a moment, or `None` when nothing
    /// automates it — which is what leaves the fader in charge.
    pub fn value_at(&self, lane: usize, param: AutoParam, seconds: f64) -> Option<f64> {
        self.get(lane, param)?.value_at(seconds)
    }

    /// Replace one control's curve. An EMPTY curve is removed rather than
    /// stored, so "no automation" has one representation and a document does
    /// not accumulate the ghosts of curves somebody drew and undrew.
    pub fn set(&mut self, curve: Curve) {
        self.curves
            .retain(|c| !(c.lane == curve.lane && c.param == curve.param));
        if !curve.is_empty() {
            self.curves.push(curve);
        }
        self.curves.sort_by_key(|c| (c.lane, c.param));
    }

    /// The curve for a control, made if it is not there yet.
    pub fn entry(&mut self, lane: usize, param: AutoParam) -> &mut Curve {
        if self.get(lane, param).is_none() {
            self.curves.push(Curve::sanitised(lane, param, Vec::new()));
            self.curves.sort_by_key(|c| (c.lane, c.param));
        }
        self.curves
            .iter_mut()
            .find(|c| c.lane == lane && c.param == param)
            .expect("just inserted")
    }

    pub fn from_curves(curves: Vec<Curve>) -> Self {
        let mut out = Self::default();
        for c in curves {
            out.set(c);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pt(at_s: f64, value: f64) -> Point {
        Point { at_s, value, shape: Shape::Linear }
    }

    /// A curve is flat outside its own points.
    ///
    /// The property that makes one safe to add to a finished song: drawing a
    /// single point at bar 40 sets the level from bar 40, it does not ramp
    /// the whole song up to it from wherever it happened to start.
    #[test]
    fn a_curve_is_flat_before_its_first_point_and_after_its_last() {
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(10.0, 0.25), pt(20.0, 0.75)]);
        assert_eq!(c.value_at(0.0), Some(0.25));
        assert_eq!(c.value_at(10.0), Some(0.25));
        assert_eq!(c.value_at(1_000.0), Some(0.75));
        // ...and interpolates between them.
        let mid = c.value_at(15.0).expect("a value");
        assert!((mid - 0.5).abs() < 1e-9, "halfway gave {mid}");
        // One point is a constant, not a ramp from zero.
        let one = Curve::sanitised(0, AutoParam::Gain, vec![pt(30.0, 0.4)]);
        assert_eq!(one.value_at(0.0), Some(0.4));
        assert_eq!(one.value_at(90.0), Some(0.4));
        // No points is no opinion, which is what leaves the fader alone.
        assert_eq!(Curve::sanitised(0, AutoParam::Gain, Vec::new()).value_at(5.0), None);
    }

    /// Hold steps; linear ramps.
    #[test]
    fn a_held_point_does_not_drift_towards_the_next_one() {
        let c = Curve::sanitised(
            0,
            AutoParam::Mute,
            vec![
                Point { at_s: 0.0, value: 0.0, shape: Shape::Hold },
                Point { at_s: 4.0, value: 1.0, shape: Shape::Hold },
            ],
        );
        assert_eq!(c.value_at(1.0), Some(0.0), "a hold drifted");
        assert_eq!(c.value_at(3.999), Some(0.0));
        assert_eq!(c.value_at(4.0), Some(1.0), "the step did not happen");
    }

    /// A hand-edited file is put in order rather than believed.
    #[test]
    fn a_curve_from_a_file_is_made_safe_before_it_is_read() {
        let c = Curve::sanitised(
            0,
            AutoParam::Gain,
            vec![
                pt(20.0, 0.5),
                pt(5.0, 9.0),            // out of range
                pt(-1.0, 0.5),           // before the song
                pt(f64::NAN, 0.5),       // not a number
                pt(10.0, f64::INFINITY), // nor this
                pt(20.0, 0.9),           // a duplicate instant
            ],
        );
        let p = c.points();
        assert!(p.windows(2).all(|w| w[0].at_s < w[1].at_s), "not sorted: {p:?}");
        assert!(p.iter().all(|q| (0.0..=1.0).contains(&q.value)), "{p:?}");
        assert!(p.iter().all(|q| q.at_s >= 0.0), "{p:?}");
        // The later of two points at one instant wins.
        assert_eq!(c.value_at(20.0), Some(0.9));
    }

    /// **A curve always begins with a node at zero**, whatever it was built
    /// from — and adding it changes nothing anyone can hear.
    ///
    /// `value_at` was already flat before the first point, so the value from
    /// zero up to it is identical either way. What the node adds is something
    /// to SEE and something to grab: a lane whose curve started at bar 40
    /// drew an unmarked flat stretch, which reads as "nothing here" rather
    /// than "held at this value".
    #[test]
    fn the_curve_always_begins_at_zero() {
        // Built from a point well into the song.
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(40.0, 0.75)]);
        assert_eq!(c.points().len(), 2, "{:?}", c.points());
        assert_eq!(c.points()[0].at_s, 0.0, "no node at the beginning");
        assert_eq!(
            c.points()[0].value,
            0.75,
            "the seeded node must hold the first point's value, or the sound changed"
        );
        assert_eq!(c.points()[0].shape, c.points()[1].shape, "the shape must carry");
        // Inaudible: the value everywhere before the first real point is what
        // it always was.
        for t in [0.0, 1.0, 20.0, 39.9] {
            assert_eq!(c.value_at(t), Some(0.75), "the curve changed at {t}");
        }

        // A curve that already starts at zero is untouched.
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(0.0, 0.1), pt(5.0, 0.9)]);
        assert_eq!(c.points().len(), 2, "a node was added to a curve that had one");

        // An empty curve stays empty — an empty one is not written down at
        // all, and seeding it would make every lane look automated.
        let c = Curve::sanitised(0, AutoParam::Gain, Vec::new());
        assert!(c.is_empty(), "an empty curve was given a point");
    }

    /// Putting a point where one already is replaces it.
    #[test]
    fn a_point_dragged_onto_its_neighbour_does_not_leave_two() {
        // Three, not two: `sanitised` seeds a node at zero, because a curve
        // always begins with one. See `the_curve_always_begins_at_zero`.
        let mut c = Curve::sanitised(0, AutoParam::Gain, vec![pt(1.0, 0.2), pt(2.0, 0.8)]);
        c.put(pt(2.0, 0.3));
        assert_eq!(c.points().len(), 3, "{:?}", c.points());
        assert_eq!(c.value_at(2.0), Some(0.3));
        // ...and a new instant inserts in order.
        c.put(pt(1.5, 0.5));
        assert_eq!(c.points().len(), 4);
        assert!(c.points().windows(2).all(|w| w[0].at_s < w[1].at_s));
        // Removing takes the nearest within reach, and nothing outside it.
        assert!(!c.remove_near(9.0, 0.1), "something far away was removed");
        assert!(c.remove_near(1.55, 0.1));
        assert_eq!(c.points().len(), 3);
    }

    /// An emptied curve is removed, not stored empty.
    #[test]
    fn a_control_with_no_points_has_no_curve() {
        let mut a = Automation::default();
        a.entry(0, AutoParam::Gain).put(pt(1.0, 0.5));
        assert_eq!(a.curves().len(), 1);
        assert_eq!(a.value_at(0, AutoParam::Gain, 1.0), Some(0.5));
        // Nothing automates a control nobody has drawn on.
        assert_eq!(a.value_at(0, AutoParam::Pan, 1.0), None);
        assert_eq!(a.value_at(3, AutoParam::Gain, 1.0), None);
        // Emptying it takes it out of the document.
        a.set(Curve::sanitised(0, AutoParam::Gain, Vec::new()));
        assert!(a.is_empty(), "an emptied curve was kept: {:?}", a.curves());
    }

    /// Each mode writes exactly when it should.
    ///
    /// The table in one test, because the four modes differ only in when
    /// they write and getting one wrong is a curve that either will not
    /// record or will not stop.
    #[test]
    fn the_modes_differ_only_in_when_they_write() {
        use AutoMode::{Latch, Off, Read, Touch, Write};
        // (mode, touching, latched) -> writes
        let table = [
            ((Off, false, false), false),
            ((Off, true, true), false),
            ((Read, true, true), false),
            // Touch writes only while the control is held.
            ((Touch, false, false), false),
            ((Touch, true, false), true),
            ((Touch, false, true), false),
            // Latch keeps writing after the hand comes off.
            ((Latch, false, false), false),
            ((Latch, true, false), true),
            ((Latch, false, true), true),
            // Write writes regardless — the dangerous one.
            ((Write, false, false), true),
        ];
        for ((mode, touching, latched), want) in table {
            assert_eq!(
                mode.writes(touching, latched),
                want,
                "{mode:?} touching={touching} latched={latched}"
            );
        }
        // Only Off stops the curve being obeyed.
        for m in AutoMode::ALL {
            assert_eq!(m.reads(), m != Off, "{m:?}");
        }
        // The default is the safe one.
        assert_eq!(AutoMode::default(), Read);
    }

    /// The persisted spellings do not move.
    ///
    /// They are in files people already have; re-spelling one silently
    /// points every saved curve at a different control, or at none.
    #[test]
    fn the_parameter_names_are_append_only() {
        assert_eq!(AutoParam::Gain.key(), "gain");
        assert_eq!(AutoParam::Pan.key(), "pan");
        assert_eq!(AutoParam::Send.key(), "send");
        assert_eq!(AutoParam::Mute.key(), "mute");
        for p in AutoParam::ALL {
            assert_eq!(AutoParam::from_key(&p.key()), Some(p), "{p:?} does not round trip");
        }
        assert_eq!(AutoParam::from_key("nonesuch"), None);
        assert_eq!(Shape::from_key(Shape::Hold.key()), Shape::Hold);
        assert_eq!(Shape::from_key(Shape::Linear.key()), Shape::Linear);
        // An unknown shape from a newer build reads as the ordinary one
        // rather than refusing the point.
        assert_eq!(Shape::from_key("bezier"), Shape::Linear);
    }
}
