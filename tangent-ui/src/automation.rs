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
    /// `TangentApp::fx_params` and not a constant.
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
    ///
    /// A plugin parameter has no name of its own here: the NAME lives with
    /// the plugin and reaches the app through `ports::FxParams`, so this
    /// answers with the address and the caller that has the list puts the
    /// word in. See `TangentApp::auto_param_label`.
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

/// **Two points closer than this in time share an instant**: a STEP.
///
/// The owner: nodes "should be able to be dragged such that 2 are on the
/// same timeline point -- this allows for instant movement of the value to
/// another value". A curve arrives at the first point of a pair and leaves
/// from the second, so the pair is a jump with no ramp between.
///
/// **One window, three readings.** `sanitised` folds points LESS than this
/// apart, measured from a run's first point, so the instants it keeps are at
/// least this far apart and hold at most two points each; `put` treats every
/// point within it, inclusive, as the one to REPLACE (the last of them, moved
/// to the new time); the app re-finds moved or pasted nodes within twice it
/// (`arr_auto_find`). No road makes a third point at one instant.
pub const SAME_INSTANT_S: f64 = 1e-9;

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
    /// **Whose a plugin curve is, once that plugin has left its bay**: the
    /// path of the tenant it was drawn for. Empty for every desk curve, and
    /// for a plugin curve while its plugin is still seated - the bay's
    /// tenant says whose it is then.
    ///
    /// A plugin curve names a bay and the plugin's OWN parameter id, and
    /// ids are small numbers every plugin reuses: the EQ's band-1 frequency
    /// and the compressor's attack are both id 2. So a curve kept through
    /// its plugin's unload (it is somebody's work, and the plugin may come
    /// back) has to remember whose it is, or the next plugin put in the bay
    /// plays it on its own knobs. `TangentApp::set_insert` keeps the rule:
    /// marked when the bay empties, played again when its own plugin comes
    /// back, let go when another arrives.
    pub tenant: String,
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
        // STABLE, and that is load-bearing: points that share an instant
        // keep the order they were given in, which is what says which of a
        // step's two points the curve ARRIVES at and which it LEAVES from.
        points.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
        // **Two points may share an instant, and that is a step** — see
        // [`SAME_INSTANT_S`]. The curve arrives at the first and leaves from
        // the second. It used to keep only the last, so a node could never
        // sit on its neighbour's time and a jump had to be faked with a ramp
        // one grid step wide.
        //
        // A third point at the same instant would live for no time at all
        // and could never be heard, so a run keeps its first and last; and
        // two that agree on the VALUE as well are one point, not a step.
        //
        // **Except at the very start.** A step there would arrive at a value
        // no time is left to play — the song begins AT the instant — and its
        // unheard first half would still be what the row's dial reads (the
        // base is the first node) while the second is what sounds. So a run
        // at zero keeps its LAST point: a node dropped on the base carries
        // its value in, and the base stays a single node.
        let mut kept: Vec<Point> = Vec::with_capacity(points.len());
        let mut i = 0;
        while i < points.len() {
            let mut j = i + 1;
            while j < points.len() && points[j].at_s - points[i].at_s < SAME_INSTANT_S {
                j += 1;
            }
            let first = points[i];
            // Snapped onto the first's time exactly, so a step drawn by two
            // hands a nanosecond apart is one instant everywhere downstream.
            let last = Point { at_s: first.at_s, ..points[j - 1] };
            let at_start = first.at_s < SAME_INSTANT_S;
            if j - i > 1 && !at_start && (first.value - last.value).abs() > 1e-12 {
                kept.push(first);
            }
            kept.push(last);
            i = j;
        }
        // **No seeded node at zero.** A curve begins with exactly the
        // points somebody drew.
        //
        // Every mutator used to call an `ensure_zero` that inserted one at
        // the start whenever the first point was later — so the very first
        // alt-click on an empty lane produced TWO nodes, the one under the
        // hand and a seed before it. The owner's "alt click to add a node
        // adds that missing first one AND another one".
        //
        // Nothing needs the seed: `value_at` is flat before the first point,
        // so a curve with one node at bar 40 already holds that value from
        // the beginning of the song. The node was only ever a drawing of a
        // rule the maths already followed.
        Self { lane, param, points: kept, tenant: String::new() }
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
    ///
    /// **Right-continuous at a step - the contract every reader shares.** The
    /// segment is the one that begins at the LAST point at or before `seconds`,
    /// so at an instant holding two points (see [`SAME_INSTANT_S`]) the curve
    /// ramps toward the first and plays the second from the instant itself on.
    /// The engine plays exactly this - `fill_automation_params`,
    /// `automation_now` and the master's three reads all call it - and so does
    /// the host's face overlay (`desktop::live_face`). The arrangement's painter
    /// draws the same shape from the points (flat before the first, a Hold's
    /// level then a vertical jump, flat after the last), so a change to this
    /// rule is a change to the painter too. Only sortedness is relied on: a
    /// zero-length segment answers its second point and the fraction is
    /// clamped. NaN or −∞ reads as before the first point, +∞ as after the last.
    pub fn value_at(&self, seconds: f64) -> Option<f64> {
        let first = self.points.first()?;
        // The segment `seconds` falls in, found as the LAST point at or
        // before it. A binary search rather than a walk because this runs
        // on the audio thread, once per block per lane.
        //
        // **The last, not any.** At an instant holding a step there are two
        // points, and from that instant on it is the SECOND that plays: the
        // curve arrives at the first and leaves from the second. The engine
        // reads this same function, so what is drawn is what is heard.
        let i = self.points.partition_point(|p| p.at_s <= seconds);
        if i == 0 {
            return Some(first.value);
        }
        let a = self.points[i - 1];
        let Some(&b) = self.points.get(i) else {
            return Some(a.value);
        };
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
    /// **Replaces rather than stacks** when a point is already at that
    /// instant: a node ADDED on top of one, or a level written over one while
    /// recording, means "this value here", not "a step here". A step is made
    /// by DRAGGING a node onto its neighbour's time — see
    /// [`Curve::set_points`]. At an instant that already holds a step it is
    /// the second point, the one the curve leaves from, that is replaced.
    ///
    /// Answers the INDEX it landed at — which is the only way a caller can
    /// name it: at a step's instant the time names two points, and the one
    /// replaced is the second.
    ///
    /// **The value must be a number.** Unlike [`Curve::sanitised`] this drops
    /// nothing: `f64::clamp` passes a NaN value through and it is stored as
    /// given (a NaN time becomes 0.0 through `max`), and `value_at` then answers
    /// NaN for that stretch of the curve.
    pub fn put(&mut self, point: Point) -> usize {
        let Point { at_s, value, shape } = point;
        let p = Point {
            at_s: at_s.max(0.0),
            value: value.clamp(0.0, 1.0),
            shape,
        };
        let from = self.points.partition_point(|q| q.at_s < p.at_s - SAME_INSTANT_S);
        let to = self.points.partition_point(|q| q.at_s <= p.at_s + SAME_INSTANT_S);
        if to > from {
            self.points[to - 1] = p;
            to - 1
        } else {
            self.points.insert(from, p);
            from
        }
    }

    /// Take out the point at `index`, if there is one.
    ///
    /// By index rather than by time, because a time no longer names one
    /// point: the two halves of a step share it, and a delete aimed at the
    /// second must not take the first.
    pub fn remove_at(&mut self, index: usize) -> Option<Point> {
        (index < self.points.len()).then(|| self.points.remove(index))
    }

    /// Replace every point at once, **keeping the given order among points
    /// that share an instant** — which is how a drag lands a node exactly on
    /// its neighbour's time and makes a step rather than a replacement. The
    /// same sanitising a file gets: see [`Curve::sanitised`]. Whose the
    /// curve is ([`Curve::tenant`]) stays as it was.
    pub fn set_points(&mut self, points: Vec<Point>) {
        let tenant = std::mem::take(&mut self.tenant);
        *self = Self::sanitised(self.lane, self.param, points);
        self.tenant = tenant;
    }

    /// Take the point nearest `seconds` out, if one is within `reach`.
    ///
    /// Only its tests call it: the app removes a node by index
    /// ([`Curve::remove_at`]) and moves nodes with [`Curve::set_points`].
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
        // **No base-node rule here.** Protecting the first point is a
        // decision about a GESTURE, and the app's own gestures do not come
        // through here: a drag moves a node with `set_points`, and the guard
        // on a deletion lives in `TangentApp::arr_take_auto_node`. Picked by
        // TIME, so at a step this takes the arrival - which is why the app
        // removes by index.
        self.points.remove(i);
        // **A lone node at the start USED to be swept away with the rest.**
        //
        // That rule existed because the node at zero was seeded, not drawn:
        // taking out the one point somebody had actually placed left a flat
        // line nobody put there, and a lane that could not be un-drawn.
        //
        // The base node is not that. It is the lane's LEVEL — what the row's
        // dial reads and writes, and what `value_at` holds back to the left
        // edge — so a lane that has had its drawn points removed is a lane
        // resting at the level its dial is showing, which is exactly right
        // and is not "automated by a flat line nobody put there".
        //
        // Clearing it here would also fight the refusal in
        // `arr_take_auto_node`: the base cannot be deleted directly, and
        // deleting it as a side effect of removing the last drawn point would
        // be the same deletion by another road.
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

    /// Move every curve on `from` to `to` — the era lift's tool. The master
    /// lane IS the channel count, so when the desk grew from sixteen to
    /// twenty-four every pre-6 document's master curves sat at 16, which is
    /// an ordinary channel now.
    pub fn remap_lane(&mut self, from: usize, to: usize) {
        for c in &mut self.curves {
            if c.lane == from {
                c.lane = to;
            }
        }
    }

    pub fn is_empty(&self) -> bool {
        self.curves.is_empty()
    }

    pub fn get(&self, lane: usize, param: AutoParam) -> Option<&Curve> {
        self.curves.iter().find(|c| c.lane == lane && c.param == param)
    }

    /// The value of one control at a moment, or `None` when nothing
    /// automates it — which is what leaves the fader in charge.
    ///
    /// Exactly [`Curve::value_at`]: flat outside the points, right-continuous
    /// at a step. The engine asks once per rendered chunk, at the chunk's
    /// FIRST frame (`secs = at / rate` in `resolve_automation`), so on the
    /// desk's own controls a step at `t` is heard from the first chunk that
    /// starts at or after `t`. A plugin parameter is read at the chunk's
    /// position too, and a lane carrying one is not rendered ahead, so it
    /// hears its curve on that same chunk - see the engine's `grain_ok`.
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
        assert!(p.windows(2).all(|w| w[0].at_s <= w[1].at_s), "not sorted: {p:?}");
        assert!(
            p.windows(3).all(|w| w[0].at_s < w[2].at_s),
            "three points share an instant: {p:?}"
        );
        assert!(p.iter().all(|q| (0.0..=1.0).contains(&q.value)), "{p:?}");
        assert!(p.iter().all(|q| q.at_s >= 0.0), "{p:?}");
        // Two points at one instant are a STEP, in the order the file gave
        // them, and from that instant on it is the later one that plays.
        assert_eq!(c.value_at(20.0), Some(0.9));
        let at_20: Vec<f64> = p.iter().filter(|q| q.at_s == 20.0).map(|q| q.value).collect();
        assert_eq!(at_20, vec![0.5, 0.9], "the step was not kept in order: {p:?}");
    }

    /// **Two nodes on one instant are a step**: the curve ARRIVES at the
    /// first and LEAVES from the second, with no ramp between.
    ///
    /// The owner: nodes "should be able to be dragged such that 2 are on the
    /// same timeline point -- this allows for instant movement of the value
    /// to another value -- currently they cannot exist 2 on a single point
    /// even when dragged."
    #[test]
    fn two_nodes_on_one_instant_are_a_step() {
        let mut c = Curve::sanitised(0, AutoParam::Gain, vec![pt(0.0, 0.2), pt(4.0, 0.5)]);
        // A node carried onto its neighbour's time, BEFORE it in order: the
        // arrival. What a drag hands `set_points`.
        c.set_points(vec![pt(0.0, 0.2), pt(4.0, 0.8), pt(4.0, 0.5), pt(8.0, 0.5)]);
        assert_eq!(c.points().len(), 4, "the step lost a node: {:?}", c.points());
        // Rising towards the ARRIVAL value right up to the instant...
        let before = c.value_at(4.0 - 1e-6).expect("a value");
        assert!((before - 0.8).abs() < 1e-4, "arrived at {before}, not 0.8");
        let mid = c.value_at(2.0).expect("a value");
        assert!((mid - 0.5).abs() < 1e-9, "the ramp in was {mid}, not 0.5");
        // ...and leaving from the SECOND from the instant on: the jump.
        assert_eq!(c.value_at(4.0), Some(0.5));
        assert_eq!(c.value_at(6.0), Some(0.5));

        // A step at the very FIRST instant jumps there too, rather than
        // holding the arrival value at the instant itself.
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(2.0, 0.1), pt(2.0, 0.9)]);
        assert_eq!(c.value_at(0.0), Some(0.1), "before the step");
        assert_eq!(c.value_at(2.0), Some(0.9), "at the step");

        // A THIRD node on the instant lives for no time at all: the run keeps
        // its first and last. Two that agree on the value are one node.
        let c = Curve::sanitised(
            0,
            AutoParam::Gain,
            vec![pt(3.0, 0.1), pt(3.0, 0.4), pt(3.0, 0.7), pt(5.0, 0.2), pt(5.0, 0.2)],
        );
        let at: Vec<(f64, f64)> = c.points().iter().map(|q| (q.at_s, q.value)).collect();
        assert_eq!(at, vec![(3.0, 0.1), (3.0, 0.7), (5.0, 0.2)], "{at:?}");
    }

    /// **No step at the very start.** The song begins AT the instant, so a
    /// step there would arrive at a value no time is left to play — and the
    /// row's dial (which reads the first node) would show that unheard half
    /// while the second sounded. A run at zero keeps its last point.
    #[test]
    fn a_step_cannot_stand_at_the_start() {
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(0.0, 0.2), pt(0.0, 0.7), pt(3.0, 0.5)]);
        let at: Vec<(f64, f64)> = c.points().iter().map(|q| (q.at_s, q.value)).collect();
        assert_eq!(at, vec![(0.0, 0.7), (3.0, 0.5)], "{at:?}");
        assert_eq!(c.value_at(0.0), Some(0.7));
    }

    /// **A step's halves are told apart by INDEX**, because they share a
    /// time: removing the second must leave the first, and putting a level
    /// on the instant replaces the one the curve leaves from.
    #[test]
    fn a_steps_two_halves_are_edited_apart() {
        let steps = || {
            Curve::sanitised(0, AutoParam::Gain, vec![pt(0.0, 0.2), pt(4.0, 0.8), pt(4.0, 0.3)])
        };
        let mut c = steps();
        assert_eq!(c.remove_at(2).map(|q| q.value), Some(0.3));
        assert_eq!(c.points().len(), 2);
        assert_eq!(c.points()[1].value, 0.8, "the wrong half went");
        assert_eq!(c.remove_at(9), None, "an index past the end removed something");

        let mut c = steps();
        c.put(pt(4.0, 0.6));
        let at: Vec<f64> = c.points().iter().map(|q| q.value).collect();
        assert_eq!(at, vec![0.2, 0.8, 0.6], "put did not replace the leaving half: {at:?}");
    }

    /// **A curve holds exactly the nodes that were drawn on it.**
    ///
    /// Every mutator used to seed a node at zero when the first point was
    /// later, so the very first alt-click on an empty lane produced TWO
    /// nodes — the one under the hand and a seed in front of it. The owner:
    /// "the current bug is no nodes goes to 2 when adding the FIRST".
    ///
    /// Nothing is lost by dropping the seed: `value_at` is flat before the
    /// first point, so a lane with one node at bar 40 already holds that
    /// value from the beginning of the song. The seed drew a rule the maths
    /// was following anyway, and charged a spurious node for it.
    #[test]
    fn a_curve_holds_only_the_nodes_that_were_drawn() {
        // Built from a point well into the song: ONE point, not two.
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(40.0, 0.75)]);
        assert_eq!(c.points().len(), 1, "a node was seeded: {:?}", c.points());
        // And it still reads as beginning at the beginning, which is what
        // the seed was there to say.
        for t in [0.0, 1.0, 20.0, 39.9] {
            assert_eq!(c.value_at(t), Some(0.75), "the curve is not flat at {t}");
        }

        // The FIRST node put on an empty curve is one node.
        let mut c = Curve::sanitised(0, AutoParam::Gain, Vec::new());
        assert!(c.is_empty());
        c.put(pt(40.0, 0.75));
        assert_eq!(
            c.points().len(),
            1,
            "the first alt-click made {} nodes: {:?}",
            c.points().len(),
            c.points()
        );

        // A curve that already starts at zero is untouched.
        let c = Curve::sanitised(0, AutoParam::Gain, vec![pt(0.0, 0.1), pt(5.0, 0.9)]);
        assert_eq!(c.points().len(), 2, "a node was added to a curve that had one");

        // An empty curve stays empty — an empty one is not written down at
        // all, and seeding it would make every lane look automated.
        let c = Curve::sanitised(0, AutoParam::Gain, Vec::new());
        assert!(c.is_empty(), "an empty curve was given a point");
    }

    /// **PUTTING a point where one already is replaces it.** A node added,
    /// or a level written while recording, on an instant that has one means
    /// "this value here". A step is a DRAG's doing, through `set_points` —
    /// see `two_nodes_on_one_instant_are_a_step`.
    #[test]
    fn a_point_put_on_an_occupied_instant_replaces_it() {
        // Two: nothing is seeded, so the curve holds what it was built
        // from. See `a_curve_holds_only_the_nodes_that_were_drawn`.
        let mut c = Curve::sanitised(0, AutoParam::Gain, vec![pt(1.0, 0.2), pt(2.0, 0.8)]);
        c.put(pt(2.0, 0.3));
        assert_eq!(c.points().len(), 2, "{:?}", c.points());
        assert_eq!(c.value_at(2.0), Some(0.3));
        // ...and a new instant inserts in order.
        c.put(pt(1.5, 0.5));
        assert_eq!(c.points().len(), 3);
        assert!(c.points().windows(2).all(|w| w[0].at_s < w[1].at_s));
        // Removing takes the nearest within reach, and nothing outside it.
        assert!(!c.remove_near(9.0, 0.1), "something far away was removed");
        assert!(c.remove_near(1.55, 0.1));
        // Three went in, one came out: two remain. It used to stay at three
        // because removing re-seeded a node at zero.
        assert_eq!(c.points().len(), 2, "{:?}", c.points());
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
