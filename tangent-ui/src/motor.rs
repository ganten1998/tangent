//! Tape start and stop: whether Play spins the whole desk up from a
//! standstill and Stop or Pause wind it down to a halt, and how long each of
//! the two takes.
//!
//! **The setting, not the machine.** This is the desk's switch and its two
//! times, pure like [`crate::varispeed`]. The engine owns the motor that obeys
//! them (the speed laws, the playback head losing its treble and its bass as
//! the tape slows, the hold of whatever was still ringing at the stop) and is
//! handed this once a frame. Nothing here knows what a sample is.
//!
//! **Off out of the box, and per project.** A desk that never asked for it
//! starts and stops at once, as every session before it did; a desk that
//! asked for it carries it in its document, like the varispeed dial whose
//! range panel it lives in.
//!
//! **The two times come from real machines.** Half a second is the slowest
//! start published for professional quarter-inch servo-capstan machines at
//! 15 in/s, to 0.1 % weighted flutter; 0.7 s is the slowest stop from play any
//! professional machine publishes, a 30 in/s figure (at 15 in/s a stop is
//! nearer a quarter to two fifths of a second). Both are the slow end of what
//! is published, because the owner asked for a slow start. The reach runs from
//! a tenth of a second, the fastest start documented (synchronous-capstan
//! machines), to ten seconds, which no machine publishes and which an artistic
//! control wants with room to spare.

use serde_json::Value;

/// How long Play takes to bring the tape to speed, out of the box.
pub const START_DEFAULT_S: f32 = 0.5;

/// How long Stop or Pause takes to bring the tape to a halt, out of the box.
pub const STOP_DEFAULT_S: f32 = 0.7;

/// The quickest either time may be set to. The engine keeps its own copy and
/// a test there holds the two equal, as its varispeed limit mirrors the dial's.
pub const MIN_S: f32 = 0.1;

/// The slowest either time may be set to.
pub const MAX_S: f32 = 10.0;

/// [`MIN_S`] as the whole centiseconds a [`StartStop`] holds, for the reach
/// the range panel prints. Held to the constant by
/// `the_shipped_machine_is_off_at_half_a_second_up_and_seven_tenths_down`.
pub(crate) const MIN_CS: u16 = 10;

/// [`MAX_S`] as whole centiseconds, held to it by the same test.
pub(crate) const MAX_CS: u16 = 1000;

/// **The desk's tape start and stop**: the switch, how long the tape takes to
/// reach speed when Play is pressed, and how long it takes to stop.
///
/// Times are held as whole CENTISECONDS, so the file, the hash and the field
/// all read the same number: 0.12 s is 12, not 0.1199999973. A float kept as
/// it was typed would go out to the document as a decimal and come back one
/// bit away from itself, and an undo moment, which is compared word for word,
/// would see an edit nobody made.
///
/// The fields are private so that nothing can hold a time outside the reach:
/// every road in, a typed field, a settings file, a document, goes through the
/// setters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartStop {
    on: bool,
    start_cs: u16,
    stop_cs: u16,
}

impl Default for StartStop {
    /// Off, half a second up and seven tenths of a second down.
    fn default() -> Self {
        Self::new(false, START_DEFAULT_S, STOP_DEFAULT_S)
    }
}

impl StartStop {
    /// The switch at `on` and the two times as given, each through its setter.
    pub fn new(on: bool, start_s: f32, stop_s: f32) -> Self {
        let mut s = Self { on, start_cs: MIN_CS, stop_cs: MIN_CS };
        s.set_start_s(start_s);
        s.set_stop_s(stop_s);
        s
    }

    /// Whether Play spins the desk up and Stop or Pause wind it down.
    pub fn on(self) -> bool {
        self.on
    }

    /// How long Play takes to bring the tape to speed, in seconds.
    pub fn start_s(self) -> f32 {
        f32::from(self.start_cs) / 100.0
    }

    /// How long Stop or Pause takes to bring the tape to a halt, in seconds.
    pub fn stop_s(self) -> f32 {
        f32::from(self.stop_cs) / 100.0
    }

    /// The start time as it is held, in whole centiseconds: what the file
    /// writes, the fingerprint hashes and the field reads.
    pub fn start_cs(self) -> u16 {
        self.start_cs
    }

    /// The stop time as it is held, in whole centiseconds.
    pub fn stop_cs(self) -> u16 {
        self.stop_cs
    }

    pub fn set_on(&mut self, on: bool) {
        self.on = on;
    }

    /// **Set the start time, in seconds**, to the nearest centisecond.
    ///
    /// **Out of the reach is CLAMPED rather than refused**, the varispeed's
    /// rule for a typed number: 30 typed into a field that reaches 10 has said
    /// "as slow as it goes", and a field that answers by doing nothing reads as
    /// a field that is broken. A NaN or an infinity is not a time at all and
    /// reads as the shipped one, so nothing a file or a field can hold hands
    /// the engine a nonsense length.
    pub fn set_start_s(&mut self, s: f32) {
        self.start_cs = centis(f64::from(s), START_DEFAULT_S);
    }

    /// Set the stop time, in seconds. See [`StartStop::set_start_s`].
    pub fn set_stop_s(&mut self, s: f32) {
        self.stop_cs = centis(f64::from(s), STOP_DEFAULT_S);
    }

    /// **The setting as a file holds it**: `{"on", "start_s", "stop_s"}`, the
    /// seconds written from the centiseconds (0.5, 0.7, 1.25) so the decimal
    /// in the file is the number on the field and reads back as itself.
    pub fn to_json(self) -> Value {
        serde_json::json!({
            "on": self.on,
            "start_s": f64::from(self.start_cs) / 100.0,
            "stop_s": f64::from(self.stop_cs) / 100.0,
        })
    }

    /// **The setting a file holds**, tolerant member by member: a missing or
    /// wrong-typed member reads as its default and leaves the others standing,
    /// and anything that is not an object at all is the default setting.
    ///
    /// The times go through the setters' own rounding, in `f64`, so a number
    /// past the reach is clamped like a typed one, even one too large for an
    /// `f32`, which would otherwise arrive as an infinity and read as the
    /// default instead.
    pub fn from_json(v: &Value) -> Self {
        let mut out = Self::default();
        if let Some(on) = v.get("on").and_then(Value::as_bool) {
            out.on = on;
        }
        if let Some(s) = v.get("start_s").and_then(Value::as_f64) {
            out.start_cs = centis(s, START_DEFAULT_S);
        }
        if let Some(s) = v.get("stop_s").and_then(Value::as_f64) {
            out.stop_cs = centis(s, STOP_DEFAULT_S);
        }
        out
    }

    /// **What a time reads as**: `cs` centiseconds to two decimals at most,
    /// trailing zeros trimmed, then " s" (`0.5 s`, `0.75 s`, `10 s`).
    ///
    /// Worked in whole numbers, so the reading is exactly the number the file
    /// holds, never a float printed to some precision.
    pub fn reading(cs: u16) -> String {
        let (whole, frac) = (cs / 100, cs % 100);
        if frac == 0 {
            format!("{whole} s")
        } else if frac % 10 == 0 {
            format!("{whole}.{} s", frac / 10)
        } else {
            format!("{whole}.{frac:02} s")
        }
    }

    /// **A time typed into one of the two fields, in seconds.**
    ///
    /// `2`, `2s`, `2 s` and `0:02` all read as two seconds: the field's own
    /// reading ends in " s", and a time somebody has in mind may come the way
    /// a player's display writes it (`recorder::parse_time`, which the trim
    /// fields share). Anything that is not a time is refused, so the field
    /// closes on the time it opened with rather than reading `abc` as zero. A
    /// time out of the reach is the setters' to clamp, not this to refuse.
    pub fn parse(text: &str) -> Option<f32> {
        let t = text.trim();
        let t = t.strip_suffix('s').unwrap_or(t);
        crate::recorder::parse_time(t).map(|s| s.min(f64::from(f32::MAX)) as f32)
    }
}

/// `s` seconds as the whole centiseconds a [`StartStop`] holds: `default` for
/// a number that is not finite, clamped to the reach, rounded to the nearest.
fn centis(s: f64, default: f32) -> u16 {
    let s = if s.is_finite() { s } else { f64::from(default) };
    // Clamped in seconds, so the rounded count is in 10..=1000 and the cast
    // cannot wrap.
    (s.clamp(f64::from(MIN_S), f64::from(MAX_S)) * 100.0).round() as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The shipped machine is off, half a second up and 0.7 s down**, and
    /// the reach is a tenth of a second to ten.
    ///
    /// The numbers the engine lane and the Tape face build on, and the ones
    /// the module header grounds in published figures. Off matters most: a
    /// desk that never asked for tape start and stop must start and stop at
    /// once, exactly as every session did before the switch existed. The
    /// file's spelling of the default is pinned too, because it is what every
    /// document a live desk saves carries.
    ///
    /// Revert-check, performed: `STOP_DEFAULT_S` at 0.8 and the default's stop
    /// reads 80 here.
    #[test]
    fn the_shipped_machine_is_off_at_half_a_second_up_and_seven_tenths_down() {
        let d = StartStop::default();
        assert!(!d.on(), "tape start and stop ships switched on");
        assert_eq!((d.start_cs(), d.stop_cs()), (50, 70), "the shipped times moved");
        assert_eq!((d.start_s(), d.stop_s()), (START_DEFAULT_S, STOP_DEFAULT_S));
        assert_eq!((START_DEFAULT_S, STOP_DEFAULT_S), (0.5, 0.7));
        assert_eq!((MIN_S, MAX_S), (0.1, 10.0), "the reach moved");
        // The centisecond bounds are the two constants, held.
        assert_eq!(centis(f64::from(MIN_S), START_DEFAULT_S), MIN_CS);
        assert_eq!(centis(f64::from(MAX_S), START_DEFAULT_S), MAX_CS);
        assert_eq!(
            (StartStop::reading(MIN_CS), StartStop::reading(MAX_CS)),
            ("0.1 s".to_owned(), "10 s".to_owned())
        );
        assert_eq!(StartStop::reading(d.start_cs()), "0.5 s");
        assert_eq!(StartStop::reading(d.stop_cs()), "0.7 s");
        assert_eq!(d.to_json().to_string(), r#"{"on":false,"start_s":0.5,"stop_s":0.7}"#);
        assert_eq!(StartStop::new(false, 0.5, 0.7), d);
    }

    /// **A time outside the reach is clamped, and nonsense reads as the
    /// default**, on every road in: the setters, the constructor and a file.
    ///
    /// Clamped because 30 typed at a field that reaches 10 means "as slow as
    /// it goes"; the default for a NaN or an infinity because neither is a
    /// time, and the engine is handed whatever this holds. A file's member of
    /// the wrong type falls back alone; a number too large for an `f32` is
    /// still clamped, not read as an infinity.
    ///
    /// Revert-checks, performed: take the clamp out of `centis` and 0 s is
    /// held as no time at all; take the finite check out and so is a NaN.
    #[test]
    fn a_time_outside_the_reach_is_clamped_and_nonsense_reads_as_the_default() {
        let mut s = StartStop::default();
        for (typed, cs) in [
            (0.0_f32, 10),
            (-3.0, 10),
            (0.05, 10),
            (0.1, 10),
            (10.0, 1000),
            (30.0, 1000),
            (1.0e9, 1000),
            (f32::MAX, 1000),
        ] {
            s.set_start_s(typed);
            s.set_stop_s(typed);
            assert_eq!((s.start_cs(), s.stop_cs()), (cs, cs), "{typed} s");
        }
        for nonsense in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            s.set_start_s(nonsense);
            s.set_stop_s(nonsense);
            assert_eq!((s.start_cs(), s.stop_cs()), (50, 70), "{nonsense} read as a time");
        }
        let n = StartStop::new(true, f32::NAN, 1.0e9);
        assert!(n.on());
        assert_eq!((n.start_cs(), n.stop_cs()), (50, 1000));
        assert!(n.start_s().is_finite() && n.stop_s().is_finite());

        // A file, member by member.
        let read = |text: &str| StartStop::from_json(&serde_json::from_str(text).expect("json"));
        assert_eq!(read("{}"), StartStop::default());
        assert_eq!(read("null"), StartStop::default());
        assert_eq!(read("5"), StartStop::default(), "a number is not a setting");
        let odd = read(r#"{"on": "yes", "start_s": "fast", "stop_s": 1e300}"#);
        assert!(!odd.on(), "a string switched it on");
        assert_eq!((odd.start_cs(), odd.stop_cs()), (50, 1000));
        let low = read(r#"{"on": true, "start_s": -1, "stop_s": 0}"#);
        assert!(low.on());
        assert_eq!((low.start_cs(), low.stop_cs()), (10, 10));
        // A whole number of seconds is a time like any other.
        assert_eq!(read(r#"{"start_s": 2, "stop_s": 3}"#).start_cs(), 200);
    }

    /// **A typed time takes seconds however it is written**: `2`, `2s`, `2 s`
    /// and `0:02` are all two seconds, and what is not a time is refused.
    ///
    /// The field opens on its reading, "0.5 s" shown and "0.5" seeded, so a
    /// trailing `s` is somebody finishing the word the field itself prints,
    /// and the clock form is the trim fields' own, from the same parser.
    /// Refused is `None`, so the field closes on what it opened with: a field
    /// that read `abc` as zero would set the quickest start there is.
    ///
    /// Revert-check, performed: drop the `strip_suffix` and `2s` is refused.
    #[test]
    fn a_typed_time_takes_seconds_however_it_is_written() {
        for text in ["2", "2s", "2 s", " 2 s ", "0:02", "2.0", "02"] {
            assert_eq!(StartStop::parse(text), Some(2.0), "{text:?}");
        }
        assert_eq!(StartStop::parse("1.25"), Some(1.25));
        assert_eq!(StartStop::parse("0:01.5 s"), Some(1.5));
        for refused in ["abc", "", " ", "s", "2ss", "-1", "1:-2", "two s"] {
            assert_eq!(StartStop::parse(refused), None, "{refused:?} was read as a time");
        }
        // And what it reads goes through the setters: out of the reach is
        // the clamp's, not a refusal.
        let mut s = StartStop::default();
        s.set_start_s(StartStop::parse("1.25 s").expect("a time"));
        s.set_stop_s(StartStop::parse("1:00").expect("a time"));
        assert_eq!((s.start_cs(), s.stop_cs()), (125, 1000));
        s.set_stop_s(StartStop::parse("0").expect("a time"));
        assert_eq!(s.stop_cs(), 10);
    }

    /// **A time is kept to the centisecond, so the file and the hash agree**:
    /// 0.123 s is held as 12 centiseconds, reads "0.12 s", and the file says
    /// 0.12, never the float's 0.11999999731779099.
    ///
    /// The document writes the seconds as a decimal and the fingerprint hashes
    /// the centiseconds; the two only describe one setting if every time the
    /// field can hold goes out to the file and comes back as itself. So every
    /// one of them is walked: all 991 centiseconds of the reach, through the
    /// JSON text and back.
    ///
    /// Revert-check, performed: write the seconds from `start_s()` (the f32)
    /// in `to_json` and the file says 0.11999999731779099.
    #[test]
    fn a_time_is_kept_to_the_centisecond_so_the_file_and_the_hash_agree() {
        let mut s = StartStop::default();
        s.set_start_s(0.123);
        assert_eq!(s.start_cs(), 12);
        assert_eq!(s.start_s(), 0.12);
        assert_eq!(StartStop::reading(s.start_cs()), "0.12 s");
        let text = s.to_json().to_string();
        assert!(text.contains(r#""start_s":0.12,"#), "the file says {text}");
        s.set_stop_s(0.756);
        assert_eq!((s.stop_cs(), StartStop::reading(s.stop_cs())), (76, "0.76 s".to_owned()));
        for (cs, reads) in [(50, "0.5 s"), (75, "0.75 s"), (125, "1.25 s"), (150, "1.5 s"), (1000, "10 s"), (10, "0.1 s")] {
            assert_eq!(StartStop::reading(cs), reads);
        }
        for cs in MIN_CS..=MAX_CS {
            for on in [false, true] {
                let mut s = StartStop::default();
                s.set_on(on);
                s.set_start_s(f32::from(cs) / 100.0);
                s.set_stop_s(f32::from(MAX_CS + MIN_CS - cs) / 100.0);
                assert_eq!(s.start_cs(), cs, "{cs} centiseconds did not hold through the setter");
                let back = StartStop::from_json(
                    &serde_json::from_str(&s.to_json().to_string()).expect("json"),
                );
                assert_eq!(back, s, "{cs} centiseconds did not come back through the file");
                // And the reading is the file's number, word for word.
                let reading = StartStop::reading(cs);
                let file = s.to_json()["start_s"].to_string();
                assert_eq!(
                    StartStop::parse(&reading).map(|t| centis(f64::from(t), START_DEFAULT_S)),
                    Some(cs),
                    "{reading:?} does not read back as {cs}"
                );
                assert_eq!(
                    file.parse::<f64>().ok().map(|t| centis(t, START_DEFAULT_S)),
                    Some(cs),
                    "the file's {file} is not {cs} centiseconds"
                );
            }
        }
    }
}
