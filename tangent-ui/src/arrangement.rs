//! The arrangement: the DOCUMENT half of the app, in its own file.
//!
//! # Why this is not in `settings.json`, and the decision is deliberate
//!
//! Settings are PREFERENCES: one set per user for ever, migrated by
//! `settings_version`, holding the desk a session starts from. An
//! arrangement is a DOCUMENT: an unbounded list of clips whose positions
//! change with the work, one per song, and eventually a thing you hand to
//! somebody. Folding the second into the first would mean exactly one
//! arrangement for ever (starting a second song destroys the first), and
//! every schema change running a migration across everyone's colours and
//! window geometry. The owner weighed this and chose the split. (The desk
//! was once a set of fixed-size arrays keyed by its width, which was a
//! third reason; it is the list of rows that exist now, in both files, read
//! and written by one serializer, `desk_format`.)
//!
//! Deliberately NOT a "project" feature: no New/Open/Save-As, no dirty flag,
//! no recent-files list. One file at a well-known path, auto-loaded and
//! auto-saved, exactly like the settings — and because it is a PATH, multiple
//! songs become possible later without any of this being rework.
//!
//! # The schema, and what is deliberately absent
//!
//! Version 1 is the smallest honest document: clips on lanes, each a file
//! path and a start in seconds. Seconds, because seconds survive a device
//! that changes rate — the engine converts at its own rate, the same rule the
//! transport follows. Unknown keys ride in `extra` and survive a round trip,
//! so a newer build's arrangement opened by this one is not stripped.
//!
//! No tempo map, no per-clip gain, no MIDI clips YET — each of those is a
//! real subsystem, and a schema that names them before they exist is a
//! promise this build cannot keep. `version` is what lets them arrive.
//!
//! # Version 2: the clip grows edges
//!
//! Trims (`offset_s`/`len_s`), fades, a display name and an id — the fields
//! the arrangement view's five operations act on, added when the engine
//! learned to play a LIST of clips rather than one backing file. A `len_s`
//! of zero means "to the end of the file": the length is the FILE's fact,
//! filled in by the host after the decode and saved so the scrubber can draw
//! without one.
//!
//! # Paths are relative to the document
//!
//! A clip inside the project's own `media/` is stored as `media/<file>`, so
//! a project directory is self-contained: move it, zip it, hand it to
//! somebody, it opens. Absolute paths still parse — a dev document may point
//! anywhere — and [`Arrangement::resolve`] is the one place the rule lives.

use serde_json::{Map, Value};
use std::path::PathBuf;

/// **A document's structural fingerprint** — one `u64` standing in for the
/// text [`Arrangement::to_json`] would write.
///
/// # Why this exists
///
/// "Is there anything unsaved?" is asked once a FRAME, and it used to be
/// answered by cloning the document, serialising it and comparing two
/// strings: 2.9 ms on a 128-clip session in a release build, a sixth of
/// every frame spent on an asterisk in the title bar. This walks the same
/// fields and mixes them into a number instead — no clone, no `Map`, no
/// `String`, no allocation at all, and 86 µs for the same 8192 notes.
///
/// # The rule that keeps it honest
///
/// **Two documents that serialise to the same text must fingerprint the
/// same.** Everything the accessors below are careful about follows from it:
///
/// * A key the writer OMITS collapses to one word here, however many
///   different floats produce the omission — see [`Fingerprint::f64`].
/// * A field written only outside a deadband — a clip's `stretch`, its
///   `gain` — arrives through [`Fingerprint::maybe_f64`], so every value
///   inside the deadband is one thing.
/// * `-0.0` is NOT folded into `0.0`. ryu writes `-0.0`, so the two are
///   different documents and the bit patterns are right to disagree.
///
/// The other direction — different text, the same number — is a COLLISION,
/// and it is possible: sixty-four bits are not a document. What that costs
/// decides where a fingerprint may be used and where the exact comparison
/// has to stay; `TangentApp::project_dirty` writes that split down.
///
/// # The words themselves: the exact form
///
/// A [`Fingerprint::recording`] KEEPS every word it mixes, and that list
/// is exact where the number is not. Two documents whose word lists are
/// equal write the same text, because the encoding is injective and the
/// walk mirrors the writer:
///
/// * every list is preceded by its length, every string by its byte
///   length, every optional thing by a tag word — so the words parse back
///   into exactly one walk;
/// * a float is its bits, with the writer's own folding of the non-finite
///   into one word (they are all "no key", or `null`, on the wire);
/// * and the walk visits every field the writer emits under the writer's
///   own conditions — the property the oracle tests
///   `the_fingerprint_moves_when_the_document_moves` and
///   `the_fingerprint_stands_when_the_document_stands` pin, and now pin
///   for the words as well as the number.
///
/// This is what lets undo keep a document as a struct and still answer
/// "would the saved file differ?" byte-exactly without serialising it:
/// compare the words. One walk, kept, costs the notes' four words each
/// and nothing per note that the fingerprint did not already pay. The
/// remaining hazard is the one the oracle guards: a field the writer
/// learns and this walk does not is invisible to BOTH answers, so the
/// oracle test is not optional.
///
/// One exclusion, written down because the words now decide the quit
/// prompt and not only the asterisk. A clip's `extra` may hold a key this
/// build OWNS but could not read — a newer build's `stretch` written as an
/// object, say; `clip_key_is_readable` leaves it where it is — and the
/// writer emits the field it owns first and an extra only where that name
/// is absent (`entry().or_insert_with`), so such an extra is walked here
/// and written nowhere. Two structs that differ only by it write the same
/// text and hold different words. It cannot be reached inside a session:
/// the spill happens only in the reader, every moment and the saved
/// baseline (`saved_point`) descend from the one parse an open makes, and
/// no road re-parses without `undo_reset`. Closing it properly means
/// spilling only the keys the writer would emit from `extra`, a
/// writer-side rule the reader would have to duplicate; until then this
/// is the line that says so.
pub struct Fingerprint {
    acc: u64,
    /// Every word mixed so far, when this fingerprint is recording.
    words: Option<Vec<u64>>,
}

impl Default for Fingerprint {
    fn default() -> Self {
        Self::new()
    }
}

/// A large odd seed, so a document whose first word is zero still moves.
const FINGERPRINT_SEED: u64 = 0x243f_6a88_85a3_08d3;

impl Fingerprint {
    pub fn new() -> Self {
        Self { acc: FINGERPRINT_SEED, words: None }
    }

    /// A fingerprint that also keeps its words — see the type's doc.
    /// `capacity` is a hint (the previous walk's length, usually) so the
    /// list is allocated once rather than grown thirty thousand times.
    pub fn recording(capacity: usize) -> Self {
        Self { acc: FINGERPRINT_SEED, words: Some(Vec::with_capacity(capacity)) }
    }

    /// One word in. rustc's own FxHash step — rotate, mix, multiply by a
    /// large odd constant.
    ///
    /// Not `DefaultHasher`: SipHash is the right default for a hash map
    /// somebody may attack, and this is a private comparison of our own
    /// bytes against our own bytes. Eight thousand notes is thirty thousand
    /// words, and the difference between one multiply and a SipHash round
    /// per word is the difference between a fingerprint that disappears
    /// inside a frame's rounding error and one that does not.
    pub fn u64(&mut self, v: u64) {
        if let Some(words) = &mut self.words {
            words.push(v);
        }
        self.acc = (self.acc.rotate_left(5) ^ v).wrapping_mul(0x517c_c1b7_2722_0a95);
    }

    pub fn bool(&mut self, v: bool) {
        self.u64(u64::from(v));
    }

    pub fn len(&mut self, v: usize) {
        self.u64(v as u64);
    }

    /// One float, exactly as the writer treats it.
    ///
    /// `serde_json::Number::from_f64` refuses anything that is not finite,
    /// and the writer then leaves the key out entirely (or, on the desk,
    /// writes `null`) — so every NaN and both infinities are the SAME
    /// document and have to be the same word here. `to_bits` alone would
    /// tell two NaN payloads apart and report an edit nobody made.
    ///
    /// No finite double wears the quiet-NaN bits, so nothing real collides
    /// with the stand-in.
    pub fn f64(&mut self, v: f64) {
        const OMITTED: u64 = 0x7ff8_0000_0000_0000;
        self.u64(if v.is_finite() { v.to_bits() } else { OMITTED });
    }

    /// A float the writer only SOMETIMES emits. `None` means "no key at
    /// all", which is the same document as a value it refuses to write.
    pub fn maybe_f64(&mut self, v: Option<f64>) {
        self.f64(v.unwrap_or(f64::NAN));
    }

    /// A string, eight bytes at a time. The length goes in first, or
    /// `"ab" + "c"` and `"a" + "bc"` would be one document.
    pub fn str(&mut self, s: &str) {
        self.len(s.len());
        let mut chunks = s.as_bytes().chunks_exact(8);
        for c in &mut chunks {
            self.u64(u64::from_le_bytes(c.try_into().unwrap_or([0; 8])));
        }
        let rest = chunks.remainder();
        let mut tail = [0u8; 8];
        tail[..rest.len()].copy_from_slice(rest);
        self.u64(u64::from_le_bytes(tail));
    }

    /// A value a newer build wrote and this one only carries — see
    /// [`ArrClip::extra`]. Hashed verbatim, ORDER included, because
    /// `preserve_order` makes a map's order part of the document's text.
    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Null => self.u64(0),
            Value::Bool(b) => {
                self.u64(1);
                self.bool(*b);
            }
            Value::Number(n) => {
                // The shapes PRINT differently — `5` is not `5.0` — so which
                // one a number is has to be part of the number.
                if let Some(u) = n.as_u64() {
                    self.u64(2);
                    self.u64(u);
                } else if let Some(i) = n.as_i64() {
                    self.u64(3);
                    self.u64(i as u64);
                } else {
                    self.u64(4);
                    self.f64(n.as_f64().unwrap_or(f64::NAN));
                }
            }
            Value::String(s) => {
                self.u64(5);
                self.str(s);
            }
            Value::Array(a) => {
                self.u64(6);
                self.len(a.len());
                for x in a {
                    self.value(x);
                }
            }
            Value::Object(m) => {
                self.u64(7);
                self.map(m);
            }
        }
    }

    /// A bag of strangers' keys, in the order it holds them.
    pub fn map(&mut self, m: &Map<String, Value>) {
        self.len(m.len());
        for (k, v) in m {
            self.str(k);
            self.value(v);
        }
    }

    /// The number, avalanched.
    ///
    /// splitmix64's finaliser. The Fx step above leaves most of its entropy
    /// in the high bits, and a fingerprint is compared whole — so the low
    /// ones are stirred back in once, at the end, rather than thirty
    /// thousand times on the way.
    pub fn finish(self) -> u64 {
        let mut x = self.acc;
        x ^= x >> 30;
        x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        x ^= x >> 27;
        x = x.wrapping_mul(0x94d0_49bb_1331_11eb);
        x ^= x >> 31;
        x
    }

    /// The words and the number, from the one walk. Empty words from a
    /// fingerprint that was not recording — the caller asked for both, so
    /// it built one with [`Fingerprint::recording`].
    pub fn finish_recorded(mut self) -> (Vec<u64>, u64) {
        let words = self.words.take().unwrap_or_default();
        (words, self.finish())
    }
}

/// The document's schema version. Bump WITH a migration arm in
/// [`Arrangement::from_map`], never without one.
///
/// **A document that already claims a HIGHER number keeps it.** See
/// [`Arrangement::version`]: writing this constant unconditionally would
/// stamp "version 4" on a file still full of version 9's keys, and the build
/// that wrote them would then run its 4→9 migrations over data that had
/// already been through them.
///
/// **Version 8: the desk is one format-2 `desk` object** (`desk_format`),
/// rows by id. The constant follows the shape rather than the build:
/// [`crate::desk_format::WRITE_V8`] decides it, so nothing is stamped 8
/// before it is format 2. The reader dispatches on the desk's own `format`,
/// never on this number, so no migration arm is needed in either direction.
pub const ARRANGEMENT_VERSION: u64 = if crate::desk_format::WRITE_V8 {
    crate::desk_format::Format::V2.document_stamp()
} else {
    crate::desk_format::Format::Legacy.document_stamp()
};

/// **Version 5: musical time.**
///
/// The document gains a tempo map and a meter map, and every clip gains an
/// ANCHOR saying whether it is nailed to a wall-clock second or to a bar.
/// Both maps are top-level; the anchor is per clip, which is what
/// [`ArrClip::extra`] arrived in version 4's lifetime to make safe.
///
/// The migration is real work rather than defaults, and it is the first arm
/// this document has ever had: a v4 file's single tempo lives on `DeskSnap`,
/// so v4→v5 is "the desk's one number becomes a map of one point". Without
/// it a song written at 90 would open at 120.
///
/// Every clip's anchor migrates to what v4 BEHAVED as, not to what is best:
/// v4 had no tempo map, so nothing followed a tempo and every clip was
/// pinned to seconds. A migration that helpfully made old MIDI musical would
/// move notes in songs people have already finished.
const V5_MUSICAL_TIME: u64 = 5;

/// **Version 7: the desk's seats stop being a dense array.**
///
/// `desk.inserts` was one string per bay of the whole seat map — 13,167 of
/// them since the pads, the folders and the amp pedals got seats — and all
/// but a handful empty in every document ever written: 132 KB of `""` lines,
/// 84% of a small project's file and a quarter of a large one, built,
/// written, parsed, cloned and hashed on every save, open, undo and frame.
///
/// From this version the writer puts the first [`HEAD_BAYS`] seats — every
/// strip and the master, the seats a document has always had — in the
/// `inserts` array as before, and every occupied seat past them in
/// `inserts_tail`, an object of `"<bay>": "<path>"` in ascending order.
/// The reader takes the array at ANY length (a v1 desk, an eighteen-strip
/// one, a 13,167-entry one, today's 108), the tail if present, and also a
/// bare object under `inserts` (the fully sparse shape, so no shape this
/// build could be handed is refused) — and dispatches on the SHAPE it
/// finds, never on the version stamp: a document keeps a HIGHER stamp than
/// the build that opens it, so an older build that opens a v7 document and
/// saves writes `arrangement_version: 7` with a dense array, and that has
/// to read correctly too.
///
/// Why a head array at all, rather than the object alone: an older build
/// reading a v7 document sees the head — its instruments and channel
/// effects — and loses only the far seats (per-pad effects, amp pedals) if
/// it saves; with a bare object it would open every project silent. The
/// head is written WHOLE, never trailing-trimmed, so its length can never
/// collide with the era lengths the migrations key on.
///
/// A sparse key names ANY bay in twenty bytes, so the reader keeps only
/// seats inside `INSERT_SPACE` (`DeskSnap::canonical` says why); a seat
/// past the map is dropped there, never spread over memory sized by it.
///
/// In memory the snapshot holds only the occupied seats, sorted by bay
/// (`DeskSnap::inserts`); the settings keep their dense `strip_inserts`
/// and the two meet in `DeskRef`, where the walk reads either shape as
/// the same sequence of occupied seats.
pub const V7_SPARSE_SEATS: u64 = 7;

/// The seats a document's desk carries as a plain array: every strip and
/// the master, `(35 + 1) * 3`. See [`V7_SPARSE_SEATS`].
///
/// **The V32 literal, not `(STRIPS + 1) * INSERTS`.** The head is a FORMAT:
/// every v7 document and v21 settings file on disk has exactly this many,
/// and the era lifts key on lengths, so a head that grew with the desk would
/// make tomorrow's files a length nobody has assigned an era to. Every seat
/// past it rides the sparse tail, whatever the live desk's width.
pub const HEAD_BAYS: usize = crate::desk_lift::V32_HEAD_BAYS;

// The sixteen-pad era's desk had exactly 84 bays and the lift that carries
// it keys on that length alone; a head of that length would be lifted as
// if it were one. The head is 108 today; this is what says so out loud the
// day the desk changes shape.
const _: () = assert!(HEAD_BAYS != (11 + 1 + 16) * 3, "the head collides with the sixteen-pad era");

/// Every key a clip object holds that THIS build understands.
///
/// One list, because it is read in two directions and the two must not
/// drift: [`Arrangement::from_map`] subtracts it from a clip object to find
/// the keys that belong to a newer build, and
/// `a_clip_knows_which_of_its_keys_are_its_own` walks a fully-populated clip
/// through a round trip to prove nothing was left off it.
/// Whether a clip key's value is the SHAPE this build reads.
///
/// Absent is readable — there is nothing to lose. Present and of another
/// type is not: see the call site.
fn clip_key_is_readable(key: &str, value: Option<&Value>) -> bool {
    let Some(v) = value else {
        return true;
    };
    match key {
        "kind" | "path" | "name" | "anchor" | "stretch_mode" => v.is_string(),
        "gain" => v.is_number(),
        "lane" | "id" => v.is_number(),
        "start_s" | "offset_s" | "len_s" | "fade_in_s" | "fade_out_s" | "stretch"
        | "loop_s" => v.is_number(),
        "follow_tempo" => v.is_boolean(),
        "notes" | "pedal" => v.is_array(),
        "warp_rate" => v.is_number(),
        "warp" => v.is_array(),
        "warp_fit" => v.is_string(),
        // A name not in the list is not a key this build claims, so nothing
        // is about to consume it either way.
        _ => true,
    }
}

/// The stretch algorithms a clip can name, and what a menu calls them.
///
/// **Keys and labels only.** The algorithms live in the binary with the DSP;
/// this crate holds what somebody chose, exactly as it holds the effect
/// parameters. `stretch::MODE_KEYS` asserts the two lists agree, so a mode
/// added on one side is a compile error on the other rather than a menu row
/// that does nothing.
///
/// The empty string means the same as `"auto"` — a clip that has never been
/// told which algorithm to use.
pub const STRETCH_MODES: [(&str, &str); 4] = [
    ("auto", "Stretch: Auto"),
    ("tape", "Stretch: Tape"),
    ("rhythmic", "Stretch: Rhythmic"),
    ("tonal", "Stretch: Tonal"),
];

/// The mode after `key`, wrapping. An unknown key steps to the first.
pub fn next_stretch_mode(key: &str) -> &'static str {
    let at = STRETCH_MODES
        .iter()
        .position(|(k, _)| *k == key)
        .unwrap_or(0);
    STRETCH_MODES[(at + 1) % STRETCH_MODES.len()].0
}

/// What a menu calls `key`. An unknown one reads as auto, which is what the
/// binary does with it.
pub fn stretch_mode_label(key: &str) -> &'static str {
    STRETCH_MODES
        .iter()
        .find(|(k, _)| *k == key)
        .map_or(STRETCH_MODES[0].1, |(_, l)| *l)
}

/// How far a clip may be time-stretched. Mirrors the binary's own
/// `stretch::RATIO`; it is written down here because the document clamps on
/// read and `tangent-ui` cannot name a constant that lives with the DSP.
pub const STRETCH_RATIO: (f64, f64) = (0.25, 4.0);

const CLIP_KEYS: [&str; 21] = [
    "kind",
    "lane",
    "path",
    "name",
    "notes",
    "pedal",
    "id",
    "start_s",
    "offset_s",
    "len_s",
    "fade_in_s",
    "fade_out_s",
    "anchor",
    "follow_tempo",
    "stretch",
    "stretch_mode",
    "gain",
    "loop_s",
    "warp_rate",
    "warp",
    "warp_fit",
];

/// What a clip is made of.
///
/// Version 3: MIDI arrived — an empty item drawn by hand, notes to follow
/// with the editor. An audio clip has a file; a MIDI clip has none (its
/// `path` is empty) and its notes will live in the document itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClipKind {
    #[default]
    Audio,
    Midi,
}

/// One note inside a MIDI clip, clip-relative and in seconds — the same
/// unit everything else in the document speaks.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrNote {
    pub at_s: f64,
    pub dur_s: f64,
    pub key: u8,
    pub vel: u8,
    /// **Written but not played.** A muted note keeps everything it is —
    /// its place, its length, its pitch, its velocity — and is skipped by
    /// every walk that turns a clip into sound.
    ///
    /// A FLAG rather than velocity zero, which is the cheap version and
    /// loses the thing that makes mute worth having: unmuting has to give
    /// the note back the velocity it had, and a velocity of zero has
    /// nowhere to have kept it. It is also not the same as deleting and
    /// undoing — a muted note is still there to be dragged, resized and
    /// read while it is silent, which is what an arrangement is for.
    pub muted: bool,
}

/// **One pedal movement inside a MIDI clip**, in the same source seconds a
/// note's `at_s` speaks.
///
/// A pedal is NOT a note and it is deliberately not folded into one. Baking
/// the damper into note lengths — extending every `dur_s` to the lift — is
/// the obvious cheap fix and it is the wrong one twice over: it throws away
/// half-pedalling, which any decent digital piano sweeps through CC64's full
/// range, and it destroys the difference between a chord ROLLED under the
/// pedal and a chord HELD, which is a difference a pianist can hear and an
/// editor should be able to show. `tangent-record`'s `.mid` writer already
/// refuses that trade (see its header, "Why this does not reuse
/// `MidiEvent`"); the arrangement was the one place that still made it.
///
/// The value is the raw controller number, 0..=127, NOT a boolean at 64. An
/// instrument that only knows down-and-up collapses it at its own door,
/// exactly as the live path does — see `mix_channels`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ArrPedal {
    pub at_s: f64,
    /// Which pedal: 64 sustain, 66 sostenuto, 67 soft. The same three
    /// `is_pedal` names in the binary, and the same three the take's `.mid`
    /// snapshots.
    pub cc: u8,
    /// 0..=127, raw. Half-pedalling lives here.
    pub value: u8,
}

/// What one lane RECORDED, handed over by the host when arrangement record
/// stops. The host owns the files — it wrote the WAVs into `media/` — and
/// the document work (the clip row, the save) is the app's.
#[derive(Debug, Clone)]
pub struct RecordedClip {
    pub lane: usize,
    /// Where the record began, seconds from 0:00. Every clip of one press
    /// starts together.
    pub start_s: f64,
    pub len_s: f64,
    /// `media/<file>` for audio; `None` is a MIDI clip.
    pub path: Option<String>,
    /// Clip-relative notes; empty on audio.
    pub notes: Vec<ArrNote>,
    /// Clip-relative pedal movements; empty on audio, and on a take played
    /// with no pedal under it.
    pub pedal: Vec<ArrPedal>,
}

/// What a clip's position is measured against.
///
/// **The fork the tempo map forces.** With one tempo for the whole song,
/// "four seconds in" and "bar three" were the same statement and nothing had
/// to choose. With a map they come apart the moment anybody edits a tempo,
/// and the right answer is not the same for both kinds of clip.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Anchor {
    /// Nailed to the wall clock. A tempo edit moves the BAR LINES around it
    /// and leaves the clip where it is.
    ///
    /// The default, and the default for audio: a recording of somebody
    /// playing exists at the moment they played it, and a tempo change is a
    /// statement about the grid, not about the performance.
    #[default]
    Time,
    /// Nailed to a bar. A tempo edit moves the clip — and, for MIDI, re-times
    /// every note inside it — so it keeps its musical seat.
    ///
    /// What a MIDI clip wants: a part written on beat three is a part on beat
    /// three at any tempo, and a ritardando that left the notes where they
    /// were would not be a ritardando at all.
    Beat,
}

impl Anchor {
    fn key(self) -> &'static str {
        match self {
            Self::Time => "time",
            Self::Beat => "beat",
        }
    }

    fn from_key(k: &str) -> Self {
        match k {
            "beat" => Self::Beat,
            _ => Self::Time,
        }
    }
}

/// One piece of audio or MIDI standing on the timeline.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrClip {
    pub kind: ClipKind,
    /// A MIDI clip's notes. Empty on audio clips, and on a MIDI item that
    /// was drawn but not yet played into.
    pub notes: Vec<ArrNote>,
    /// **A MIDI clip's pedal movements**, sorted by `at_s`, in the same
    /// source seconds the notes speak. Empty on audio, and on a part played
    /// without a pedal.
    ///
    /// Its own list rather than a field on [`ArrNote`] because a pedal is a
    /// property of the PERFORMANCE and not of any one note: it goes down
    /// between two notes as often as under one, and it goes down when nothing
    /// is sounding at all. See [`ArrPedal`].
    pub pedal: Vec<ArrPedal>,
    /// Which lane it stands on. Lane 0 is the backing track's.
    pub lane: usize,
    /// The file it came from — `media/<file>` inside a project, or absolute.
    /// See [`Arrangement::resolve`].
    pub path: String,
    /// Where it begins on the timeline, in seconds from 0:00.
    pub start_s: f64,
    /// How far into the FILE the clip starts — the left trim. Zero untrimmed.
    pub offset_s: f64,
    /// How long the clip runs on the timeline. **Zero means "to the end of
    /// the file"** — the file's length is the host's fact, filled in after
    /// the decode and saved, so a document written by hand still plays.
    pub len_s: f64,
    /// **How long the looped section is.** Zero means the clip does not loop
    /// and plays once, which is what every document written before this
    /// existed says — and what the Loop row of the clip menu leaves behind
    /// when it is unticked.
    ///
    /// **Every clip is born with one, audio and MIDI alike**, the owner's
    /// ruling: "the default behavior will be all clips audio or midi at time
    /// of creation are loops". A clip is pinned at the length it was created
    /// and REPEATS beyond it, so dragging its right edge lays down another
    /// pass instead of revealing empty time — see `arr_trim_right`, which
    /// drops the end-of-file cap for exactly this reason. A loop as long as
    /// the clip it sits on is ONE pass and sounds like no loop at all; it is
    /// the drag that makes it audible, which is why arming it at birth costs
    /// nothing and asking for it afterwards would have been a second gesture
    /// on every part anybody ever wanted twice.
    ///
    /// The material is held ONCE and expanded where it is scheduled — the
    /// notes by [`clip_notes_in_song_time`], the audio by
    /// `desktop::audio_clip_passes`, both of which read this field. A loop
    /// that copied its notes would drift out of step with the take it came
    /// from the moment one of them was edited, and a loop that copied its
    /// samples would carry the file again once per pass.
    ///
    /// **An imported audio file cannot be given one at its construction
    /// site**, because its length is the decoder's fact and `len_s` is still
    /// the whole-file sentinel when the clip is made. It is learned with the
    /// length instead — see `TangentApp::note_clip_len`, which is the host's
    /// report after the decode and the one place a clip that has never been
    /// measured gets both numbers at once.
    pub loop_s: f64,
    /// Seconds of ramp up from silence at the clip's head. Zero for none.
    pub fade_in_s: f64,
    /// Seconds of ramp down to silence at the clip's tail. Zero for none.
    pub fade_out_s: f64,
    /// What the clip is called on the lane. The file's stem until renamed.
    pub name: String,
    /// Stable identity for edits — selection, undo one day. Assigned from
    /// [`Arrangement::next_id`], never reused within a document.
    pub id: u64,
    /// What this clip's position is measured against. See [`Anchor`].
    pub anchor: Anchor,
    /// Stretch this clip to keep its musical length when the tempo moves.
    ///
    /// Only meaningful with [`Anchor::Beat`], and only on AUDIO: a
    /// beat-anchored MIDI clip re-times its notes, which is a different and
    /// exact operation. Audio has to be re-sampled or time-stretched, so it
    /// is opt-in per clip rather than a property of the anchor — moving a
    /// take to keep its bar is cheap and lossless, stretching it is neither.
    pub follow_tempo: bool,
    /// How much longer this clip plays than the file it came from, with the
    /// PITCH KEPT. 1.0 is untouched, and untouched is the fast path all the
    /// way down: the host hands the engine the decoded file itself.
    ///
    /// **The opposite of the varispeed dial**, which moves the speed and
    /// lets the pitch follow. See the binary's `stretch` module for the
    /// three algorithms and for why the stretched audio is rendered once
    /// rather than streamed.
    pub stretch: f64,
    /// Which algorithm, by its key — `"auto"`, `"tape"`, `"rhythmic"`,
    /// `"tonal"`. Empty means auto.
    ///
    /// **A string and not an enum**, because `tangent-ui` cannot name a type
    /// that lives in the binary with the DSP. The same bargain the effect
    /// parameters make: this crate holds what somebody chose and the binary
    /// knows what it means.
    pub stretch_mode: String,
    /// **Where this clip has been warped**, as `(source frame, result
    /// frame)` pairs, ascending in both. Empty for a clip nobody has
    /// quantized.
    ///
    /// The audio twin of a MIDI quantize: the host detects the attacks, this
    /// says where each one was asked to land, and `warp::warp` renders the
    /// clip that puts them there. A LIST rather than a single ratio because
    /// that is the whole difference between warping and stretching - each
    /// segment gets its own ratio, so a take that rushed one bar and dragged
    /// the next is fixed in both directions at once.
    ///
    /// Frames rather than seconds because a tempo change must not move them -
    /// but frames of the file AS DECODED at the engine rate the quantize ran at
    /// (every decode is resampled to the rate it is asked for), not samples on
    /// disk. The map records that rate in `warp_rate`, and every reader
    /// rescales it to the decode in its hands, so an export at another rate,
    /// or the session reopened on a device running another, moves the same
    /// seconds the quantize did.
    pub warp: Vec<(u64, u64)>,
    /// **How each warped segment gets to where it belongs** - the binary's
    /// `warp::Fit` by its key. Empty reads as auto, which asks the material.
    pub warp_fit: String,
    /// **The sample rate `warp`'s frames are counted at**: the rate of the
    /// decode the quantize measured its attacks in.
    ///
    /// The same file is a different number of frames in the live engine, in
    /// an export at another rate, and on a device whose rate has changed
    /// since, so a map applied to them raw pulled each anchor 8.8% of its
    /// time off its hit between 48 and 44.1 kHz. Each reader rescales the
    /// map from this rate to its own decode's (the binary's `warp_anchors`).
    ///
    /// **Zero means unknown, and the map is read as it stands.** That is
    /// what a document from before this field says, and how every build
    /// before it read the map: right at the rate it was made at. Reading it
    /// as 48 kHz instead would move every quantize made at 44.1 kHz.
    pub warp_rate: u32,
    /// The clip's own level, linear. 1.0 is untouched.
    ///
    /// **Where NORMALISE lives**, and why it is undoable: rewriting the file
    /// would be destructive and irreversible; a number on the clip is
    /// neither. It rides through to the engine on `Placed::gain`.
    pub gain: f64,
    /// Keys THIS build does not know, kept so a newer build's clip survives
    /// being opened here.
    ///
    /// **The document already made this bargain and the clips were left out
    /// of it.** [`Arrangement::extra`] has always carried the top-level keys
    /// a newer build wrote — that is what `a_newer_builds_keys_survive`
    /// proves — but the clip loop destructured each clip object, read the
    /// eleven fields it knew, and dropped the rest on the floor. So a
    /// document whose clips carry a stretch ratio, an automation curve or a
    /// beat anchor came back from an older build with the timeline intact
    /// and every clip silently reverted.
    ///
    /// It has never mattered, because no build has ever written a per-clip
    /// key this one does not know. It is about to matter three times over,
    /// which is why the bag arrives BEFORE any of them rather than after the
    /// first one loses somebody's work.
    pub extra: Map<String, Value>,
}

impl Default for ArrClip {
    /// **The same clip `from_json` builds from a document that says nothing.**
    ///
    /// It was derived, which made `stretch` and `gain` ZERO — and zero is not
    /// what an unwritten stretch means. The reader raises an absent or
    /// out-of-range ratio to 1.0, and the writer omits the key when it IS
    /// 1.0, so a derived clip wrote `"stretch": 0.0`, read back as 1.0, and
    /// wrote nothing the second time: `to_json(from_json(x)) != x`.
    ///
    /// That is undo's comparison, and a document that is not a fixed point
    /// makes every undo look like an edit. It also aimed a real gun at
    /// anything that ever builds a clip this way: `desktop.rs` stretches
    /// whenever the ratio is not 1.0, and `stretch()` CLAMPS 0.0 to the
    /// smallest ratio it allows rather than to unity — so a clip made from
    /// `Default` would have been time-stretched to a quarter of itself.
    /// Every real construction site fills these in by hand, which is the only
    /// reason it never shipped.
    fn default() -> Self {
        Self {
            kind: ClipKind::default(),
            notes: Vec::new(),
            pedal: Vec::new(),
            lane: 0,
            path: String::new(),
            start_s: 0.0,
            offset_s: 0.0,
            len_s: 0.0,
            loop_s: 0.0,
            fade_in_s: 0.0,
            fade_out_s: 0.0,
            name: String::new(),
            id: 0,
            anchor: Anchor::default(),
            follow_tempo: false,
            // The two the reader defaults to unity. See above.
            stretch: 1.0,
            gain: 1.0,
            stretch_mode: String::new(),
            warp: Vec::new(),
            warp_fit: String::new(),
            warp_rate: 0,
            extra: Map::new(),
        }
    }
}

impl ArrClip {
    /// Where the clip ENDS on the timeline, given the file length the host
    /// knows. `len_s == 0.0` reads to the end of the file, minus the trim.
    pub fn end_s(&self, file_s: f64) -> f64 {
        self.start_s + self.play_len_s(file_s)
    }

    /// How long the clip actually plays, resolving the zero sentinel.
    pub fn play_len_s(&self, file_s: f64) -> f64 {
        if self.len_s > 0.0 {
            self.len_s
        } else {
            (file_s - self.offset_s).max(0.0)
        }
    }

    /// **Where a MIDI clip's passes are counted from**, in song seconds:
    /// `start_s - offset_s`. See [`midi_loop_origin_s`].
    pub fn midi_loop_origin_s(&self) -> f64 {
        midi_loop_origin_s(self.start_s, self.offset_s)
    }

    /// **Song second `t`, folded into the ONE pass a looping MIDI clip
    /// holds.** See [`fold_into_midi_pass`].
    pub fn midi_fold_s(&self, t: f64) -> f64 {
        fold_into_midi_pass(self.start_s, self.len_s, self.loop_s, self.offset_s, t)
    }
}

/// **Where a looped clip's FIRST pass ends**, in seconds from its head, or
/// `None` for a clip that never turns over inside itself - the arrangement's
/// seam rule, in the one place it is spelled.
///
/// A MIDI clip trimmed from its front starts part-way through its loop and
/// turns over `offset mod loop` sooner (the notes stand at `at_s - offset_s
/// + loop_s * pass`); an audio clip plays the same trimmed seconds every pass,
/// so it turns over a whole loop in whatever the trim. See the panel's
/// `clip_head_s`, which draws from this, and glue, which asks it whether one
/// clip alone shows more than one pass.
pub fn first_pass_s(midi: bool, len_s: f64, loop_s: f64, offset_s: f64) -> Option<f64> {
    if loop_s <= 1.0e-6 || !loop_s.is_finite() || !len_s.is_finite() {
        return None;
    }
    let head_s = if midi {
        // `max` yields the other side of a NaN, so a clip with no offset
        // written reads as no trim rather than poisoning the arithmetic; an
        // infinite one falls out of the comparison below as `None`.
        let phase_s = offset_s.max(0.0) % loop_s;
        if phase_s <= 1.0e-9 { loop_s } else { loop_s - phase_s }
    } else {
        loop_s
    };
    (len_s > head_s + 1.0e-9).then_some(head_s)
}

impl ArrClip {
    /// **Does this clip show more than one pass?** - a seam inside it. See
    /// [`first_pass_s`].
    pub fn repeats(&self) -> bool {
        first_pass_s(self.kind == ClipKind::Midi, self.len_s, self.loop_s, self.offset_s).is_some()
    }
}

/// **What an audio glue made**: one file, and where its clip stands. The
/// host renders it and hands it to `TangentApp::finish_audio_glue`, which
/// replaces the clips it was welded from.
#[derive(Debug, Clone, PartialEq)]
pub struct GluedAudio {
    pub lane: usize,
    /// `media/<file>`, inside the project, as every clip's path is.
    pub path: String,
    /// The earliest clip's start: the file's first frame stands here.
    pub start_s: f64,
    /// The whole weld, the last pass's end included.
    pub len_s: f64,
    pub name: String,
}

/// **Where a looping MIDI clip's passes are counted from**, in song seconds.
///
/// [`clip_notes_in_song_time`] places a note at `start_s + (at_s - offset_s)
/// + loop_s * pass`, so every pass boundary sits a whole loop from
/// `start_s - offset_s` - the clip's SOURCE origin laid on the timeline. A
/// clip trimmed from its front therefore starts mid-loop, and its first pass
/// ends `offset_s mod loop_s` sooner than an untrimmed one's. The editor draws
/// a clip's notes once, in the pass that starts at this origin, so its loop
/// bar and its folded playhead count from here too: the bar marks the turn at
/// the end of the pass the roll draws. That is the arrangement's first seam
/// (`clip_head_s`) whenever the trim is shorter than a loop. The drawn pass
/// itself begins at this origin, so on such a part its first `offset_s`
/// seconds lie BEFORE the clip's head, in the roll's veil: the first pass
/// never plays them (the trim cut them), every later pass does, and the
/// folded playhead stands there for that long at the top of every pass after
/// the first. Trimmed by a loop or more, the whole drawn pass - notes, bar and
/// playhead alike - stands before the head, as the notes always have.
pub fn midi_loop_origin_s(start_s: f64, offset_s: f64) -> f64 {
    // `max` yields the other side of a NaN, so an unwritten offset reads as
    // no trim rather than poisoning every position computed from it.
    start_s - offset_s.max(0.0)
}

/// **Song second `t`, folded into the ONE pass a looping MIDI clip holds** -
/// where the roll draws whatever is sounding at `t`.
///
/// The editor draws a clip's notes once, at `start_s + (at_s - offset_s)`,
/// and the repeats happen out on the timeline where it cannot see them. So a
/// playhead drawn at the song's own second walked off the end of the part
/// while the part went on playing from its top: the owner's "the playhead for
/// a loop when viewed in the midi editor doesn't visually loop/wrap, it just
/// keeps going and the loop does play properly". Folded, it stands on the
/// note that is actually sounding, pass after pass.
///
/// Only INSIDE the clip, and only for a clip that loops. Before the clip or
/// past its end there is no pass to be in, and the song's own second is the
/// truth; a clip with no loop plays its notes once, where they are drawn.
/// The fold is the inverse of [`clip_notes_in_song_time`]'s placement, which
/// is why it counts from [`midi_loop_origin_s`] and not from `start_s`.
pub fn fold_into_midi_pass(start_s: f64, len_s: f64, loop_s: f64, offset_s: f64, t: f64) -> f64 {
    if !(loop_s > 1.0e-6) || !loop_s.is_finite() || !t.is_finite() || !len_s.is_finite() {
        return t;
    }
    if t < start_s || t >= start_s + len_s {
        return t;
    }
    let origin = midi_loop_origin_s(start_s, offset_s);
    // **A second on the boundary is the TOP of the next pass.** `t - origin`
    // is a difference of two sums and lands a hair short of a whole number of
    // loops as often as on it - 8.7 - 4.7 is 3.9999999999999996 - and the
    // remainder of that is a whole loop less a hair: the line flashed to the
    // loop bar on the frame a pass began, and Enter on a bar line wrote at
    // the loop's END. Within a nanosecond of the turn is the turn.
    let mut phase = (t - origin).rem_euclid(loop_s);
    if loop_s - phase < 1.0e-9 {
        phase = 0.0;
    }
    let folded = origin + phase;
    if folded.is_finite() {
        folded
    } else {
        t
    }
}

/// **A MIDI clip's notes as the SONG hears them**: every loop pass spelled
/// out, every note at its place on the timeline in seconds from 0:00.
///
/// A clip's own notes are held ONCE and clip-relative — measured from the
/// clip's source origin, so a note's timeline position is `start_s + (at_s -
/// offset_s)` — and a clip with a `loop_s` repeats them beyond that without
/// copying them, which is what keeps a loop in step with the take it came
/// from when one of its notes is edited. See [`ArrClip::loop_s`]. Anything
/// that wants to HEAR a clip, or write it down, has to do that expansion.
///
/// **This exists because the expansion was written three times.** The
/// binary's `desktop::midi_clip_notes` does it in FRAMES for playback and for
/// the offline bounce; `TangentApp::arr_glue_selection` did it in SECONDS to
/// weld clips together; and the MIDI export needs it in seconds too. The
/// first of those cannot be shared — it lives across the firewall and works
/// in frames — and its own doc comment records that this exact class of drift
/// has already shipped once here, when the loop was expanded for playback and
/// not for the bounce. The two that CAN share must, or an exported file will
/// eventually disagree with the sound for the same reason.
///
/// The two skips and the bound are inherited from that schedule, and the
/// engine's wall at the clip's end (`PlacedMidi::len`) is spelled out here:
/// a note that would sound before the clip's own head is dropped rather than
/// played early; a note that BEGINS at or past the clip's end is not played
/// at all, whether it is a loop's last, partial pass or a note a right trim
/// hides on a clip that does not loop; and the pass count is clamped,
/// because a clip dragged out to an hour over a loop of a sixteenth is not
/// something to hand the audio thread - or, here, to build a note list from.
///
/// **A note is let go at the clip's end.** One that STARTS inside the clip
/// and rings past it is cut there, because the engine sends its note-off at
/// the clip's end: that is what playback does and therefore what an export
/// has to write. It used to ring its whole length here, past what was heard.
pub fn clip_notes_in_song_time(clip: &ArrClip) -> Vec<ArrNote> {
    // **The clip's OFFSET is part of the span to cover.** The notes are
    // placed at `at_s - offset_s + shift`, so a clip trimmed from its front
    // starts mid-loop — and a pass count of `len_s / loop_s` then runs out
    // exactly `offset_s` before the clip's end, leaving the tail with no
    // notes in it at all. Trim half a loop off the front and the last half
    // is empty, which is how the owner found it: "midi loops that are
    // clipped leave a latter half that is empty." Covering `len_s +
    // offset_s` is what makes the last pass reach the end; the `at >= len_s`
    // skip below is what stops it running past.
    let passes: usize = if clip.loop_s > 1.0e-6 {
        (((clip.len_s + clip.offset_s.max(0.0)) / clip.loop_s).ceil() as usize)
            .clamp(1, 4_096)
    } else {
        1
    };
    // Not `with_capacity(notes * passes)`: most passes of a short loop under
    // a long clip are cut short by the skip below, so that reservation is
    // wrong by a factor of the loop count — up to four thousand of them —
    // and on a busy clip it is a hundred megabytes claimed to hold a few
    // hundred notes.
    let mut out: Vec<ArrNote> = Vec::new();
    for pass in 0..passes {
        let shift = clip.loop_s * pass as f64;
        for n in &clip.notes {
            // **A muted note is not scheduled.** This is the one walk every
            // road to sound goes through — the engine's push, the glue, the
            // MIDI export — so muting is honoured everywhere by being
            // honoured here, and a note that is still drawn, still dragged
            // and still selectable simply does not reach a synth.
            if n.muted {
                continue;
            }
            let at = n.at_s - clip.offset_s + shift;
            if at < -1.0e-9 {
                continue;
            }
            // **Looped or not.** The engine walls every clip at its end, so
            // a note past the end of a clip that does not loop - one a right
            // trim hides - is no more heard than a pass that begins there.
            if at >= clip.len_s - 1.0e-9 {
                continue;
            }
            out.push(ArrNote {
                at_s: clip.start_s + at,
                // Let go at the clip's end, where the engine lets it go.
                dur_s: n.dur_s.min(clip.len_s - at),
                key: n.key,
                vel: n.vel,
            muted: false,
            });
        }
    }
    out
}

/// **Where a MIDI clip first PLAYS a note at source time `at_s`**: the shift
/// of the first pass that sounds it (`loop_s * pass`, zero on a clip that
/// does not loop), or `None` when no pass does.
///
/// A pass stands the note at `at_s - offset_s + shift` from the clip's head
/// and sounds it only when that falls INSIDE the clip: [`clip_notes_in_song_time`]'s
/// two skips (before the head; at or past the end, the engine's wall at
/// `PlacedMidi::len`, looped or not) and its bound on the passes. So a note the trim
/// hides on the first pass is first heard one loop later, and one the trim
/// hides on every pass, or that only ever falls past the end, is heard
/// nowhere. The quantize asks this, so it moves a note where it is heard and
/// leaves alone the notes a clip does not play.
pub fn first_heard_shift_s(clip: &ArrClip, at_s: f64) -> Option<f64> {
    let at = at_s - clip.offset_s;
    if !at.is_finite() || !clip.len_s.is_finite() {
        return None;
    }
    let looped = clip.loop_s > 1.0e-6 && clip.loop_s.is_finite();
    let pass = if looped && at < -1.0e-9 {
        // The first pass that does not stand it before the head.
        ((-1.0e-9 - at) / clip.loop_s).ceil()
    } else {
        0.0
    };
    let passes = if looped {
        ((clip.len_s + clip.offset_s.max(0.0)) / clip.loop_s)
            .ceil()
            .clamp(1.0, 4_096.0)
    } else {
        1.0
    };
    // Zero for a clip that does not loop, whose `loop_s` may not even be
    // finite: `inf * 0.0` is NaN.
    let shift = if looped { clip.loop_s * pass } else { 0.0 };
    let placed = at + shift;
    (pass < passes && placed >= -1.0e-9 && placed < clip.len_s - 1.0e-9).then_some(shift)
}

/// **The twin of [`clip_notes_in_song_time`] for the pedal**, and it exists
/// for the same reason: the loop is spelled out once, here, so that glue and
/// the MIDI export cannot come to different opinions about what a repeating
/// clip does.
///
/// Every pass restates the pedal it BEGINS at, not only the edges inside it.
/// A loop whose pedal goes down in its second half and is never lifted holds
/// the damper across the seam when it is played, so a flatten that emitted
/// only the literal edges would come out drier than the clip it flattened —
/// the second pass would open with the pedal up because nothing said
/// otherwise.
///
/// The result is sorted and free of no-op repeats: a restatement equal to the
/// level already standing is dropped, so a pedal held across four passes is
/// one edge and not four.
///
/// **And it ends where the clip does.** The engine reads a clip's pedal only
/// inside the clip, so an edge at or past its end is not written, looped or
/// not, and a pedal still down there is lifted at `start_s + len_s`. Without
/// the lift, a glue or a MIDI export held the pedal down past the clip, for
/// the rest of an exported file when nothing else lifted it; and without the
/// skip, a lift or a press a right trim hides was written as if heard. A
/// consumer that joins several clips of a lane joins them through
/// [`lane_pedal_in_song_time`], which reads overlapping clips as playback
/// does: this clip's lift is not the lane's while another clip holds it.
pub fn clip_pedal_in_song_time(clip: &ArrClip) -> Vec<ArrPedal> {
    if clip.pedal.is_empty() {
        return Vec::new();
    }
    let passes: usize = if clip.loop_s > 1.0e-6 {
        (((clip.len_s + clip.offset_s.max(0.0)) / clip.loop_s).ceil() as usize)
            .clamp(1, 4_096)
    } else {
        1
    };
    let mut out: Vec<ArrPedal> = Vec::new();
    // What each pedal stands at as the walk goes, so a restatement that
    // changes nothing can be dropped.
    let mut standing = [0_u8; 3];
    let slot = |cc: u8| match cc {
        66 => 1,
        67 => 2,
        _ => 0,
    };
    for pass in 0..passes {
        let shift = clip.loop_s * pass as f64;
        let pass_start = shift.max(0.0);
        // The level this pass OPENS at: the last edge at or before its own
        // start, which for pass zero is whatever preceded the trim.
        for cc in [64_u8, 66, 67] {
            let opens = clip
                .pedal
                .iter()
                .filter(|e| e.cc == cc && e.at_s - clip.offset_s <= pass_start + 1.0e-9)
                .next_back()
                .map_or(0, |e| e.value);
            if opens != standing[slot(cc)] && pass_start < clip.len_s - 1.0e-9 {
                standing[slot(cc)] = opens;
                out.push(ArrPedal {
                    at_s: clip.start_s + pass_start,
                    cc,
                    value: opens,
                });
            }
        }
        for e in &clip.pedal {
            let at = e.at_s - clip.offset_s + shift;
            // Strictly after the pass's own start: an edge exactly there is
            // the opening level, already stated above.
            if at <= pass_start + 1.0e-9 {
                continue;
            }
            // **At or past the clip's end, looped or not**, the notes
            // twin's rule: the engine reads a clip's pedal only inside it.
            // The lift stated at the end, below, is what comes after.
            if at >= clip.len_s - 1.0e-9 {
                continue;
            }
            if e.value == standing[slot(e.cc)] {
                continue;
            }
            standing[slot(e.cc)] = e.value;
            out.push(ArrPedal {
                at_s: clip.start_s + at,
                cc: e.cc,
                value: e.value,
            });
        }
    }
    // **And the pedal comes up at the clip's end**, where playback lets it
    // go: whatever stands down there is lifted on the clip's last instant.
    if clip.len_s.is_finite() && clip.len_s > 0.0 {
        for cc in [64_u8, 66, 67] {
            if standing[slot(cc)] != 0 {
                out.push(ArrPedal {
                    at_s: clip.start_s + clip.len_s,
                    cc,
                    value: 0,
                });
            }
        }
    }
    out.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
    out
}

/// **A lane's pedal, its clips joined the way playback joins them**: at
/// every instant each pedal stands at the HIGHEST level among the clips the
/// instant falls inside, and up outside them all.
///
/// That is the engine's rule (`Renderer::clip_pedal_level`, which reads each
/// clip's schedule from [`clip_pedal_in_song_time`]), written down as the
/// edges where the level moves. Glue and the MIDI export join a lane's clips
/// through this. They used to lay each clip's own edges side by side, which
/// is the same thing until two clips overlap: then the first clip's lift at
/// its end cut a pedal the second still held, and a lower level the second
/// pressed under a held pedal lowered it. Where the clips do not overlap it
/// gives what each clip gives alone, its lift at its end included; where one
/// clip presses on the instant the one before it lets go, playback sends
/// nothing, and nothing is written.
///
/// `clips` is the caller's one lane, in any order. The result is sorted and
/// free of edges that change nothing.
pub fn lane_pedal_in_song_time<'a>(clips: impl IntoIterator<Item = &'a ArrClip>) -> Vec<ArrPedal> {
    const CCS: [u8; 3] = [64, 66, 67];
    // Each pedalled clip's span and its own schedule; a clip with no pedal
    // holds nothing, so it moves nothing.
    let lists: Vec<(f64, f64, Vec<ArrPedal>)> = clips
        .into_iter()
        .filter(|c| !c.pedal.is_empty())
        .map(|c| (c.start_s, c.start_s + c.len_s, clip_pedal_in_song_time(c)))
        .collect();
    // The instants the level can move at: every edge, and each clip's two
    // ends. One instant within the schedule's own tolerance, so a clip that
    // ends a rounding before the next begins is one move and not two.
    let mut times: Vec<f64> = lists
        .iter()
        .flat_map(|(start, end, list)| {
            [*start, *end].into_iter().chain(list.iter().map(|e| e.at_s))
        })
        .filter(|t| t.is_finite())
        .collect();
    times.sort_by(f64::total_cmp);
    times.dedup_by(|later, kept| (*later - *kept).abs() <= 1.0e-9);
    // Where each clip's own schedule stands as the walk goes, and how far
    // into it the walk has read: one pass over every list, not a search per
    // instant.
    let mut read = vec![0_usize; lists.len()];
    let mut held = vec![[0_u8; 3]; lists.len()];
    let mut standing = [0_u8; 3];
    let mut out: Vec<ArrPedal> = Vec::new();
    for t in times {
        let mut level = [0_u8; 3];
        for (i, (start, end, list)) in lists.iter().enumerate() {
            while let Some(e) = list.get(read[i]).filter(|e| e.at_s <= t + 1.0e-9) {
                if let Some(slot) = CCS.iter().position(|cc| *cc == e.cc) {
                    held[i][slot] = e.value;
                }
                read[i] += 1;
            }
            // Read only inside the clip, as the engine reads it: from its
            // start, and not at its end, where its own lift lands.
            if t >= start - 1.0e-9 && t < end - 1.0e-9 {
                for (lv, h) in level.iter_mut().zip(held[i]) {
                    *lv = (*lv).max(h);
                }
            }
        }
        for (slot, cc) in CCS.into_iter().enumerate() {
            if level[slot] != standing[slot] {
                standing[slot] = level[slot];
                out.push(ArrPedal { at_s: t, cc, value: level[slot] });
            }
        }
    }
    out
}

/// The DESK a session carries — what makes a project a SESSION rather
/// than a clip list: which tracks there are and what they are, what stands
/// in their bays, their names, colours, faders and sends, and the
/// transport's musical facts. Captured into the document on every save;
/// APPLIED only on an explicit open (launch reopens the last project against
/// the settings that are already its live desk).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeskSnap {
    /// **The rows, in lane order**, each carrying everything true of one
    /// track and its identity (`Settings::rows`' own shape): as many as the
    /// desk has, and no more. A legacy document's dense arrays are read onto
    /// rows once, at the frozen legacy width under the identity rule (legacy
    /// row r is `TrackId(r)`, `DeskModel::from_v22`), so no undo moment, no
    /// apply and no writer ever meets a per-channel array again.
    ///
    /// A row fact the document does not SAY ([`Says::PHASE`],
    /// [`Says::TO_THEORY`], [`Says::INSTRUMENTS`]) holds the vacant row's
    /// value here and the live desk's stands for it when the snapshot is
    /// applied, row for row; see [`Self::says`].
    pub rows: Vec<crate::desk_model::TrackRow>,
    /// **Every seated plugin, by the bay it sits in** (`Settings::plugins`):
    /// sparse, never an empty path. Dense it was 13,167 strings a snapshot,
    /// copied into every undo moment; see [`V7_SPARSE_SEATS`]. By identity
    /// like the live desk's, so a moment taken before a reorder and one
    /// after it hold the same bays, and only the rows say where they moved.
    /// A legacy document's seats are bound to its rows' identities as it is
    /// read (`from_value_legacy`).
    pub plugins: std::collections::BTreeMap<crate::desk_id::SeatBay, String>,
    pub drums_gain: f64,
    pub drums_pan: f64,
    pub metronome_gain: f64,
    pub master_gain: f64,
    pub fx_return: f64,
    pub tempo_bpm: f64,
    pub sig_beats: u8,
    pub sig_unit: u8,
    pub metronome_on: bool,
    /// **Which of the late row facts the rows SAY** (polarity, the theory
    /// switch, the instruments' patches, the freezes), one bit each of
    /// [`Says`]. Stored, because rows have no `None` to say "this document
    /// predates the key" with: a legacy document from before a key leaves
    /// its bit clear and the live desk's value stands for it at the apply,
    /// so nothing an old project sounds like changes because a field was
    /// added. A format-2 desk and a live desk say every one. The late facts
    /// that are not a row's say themselves by their `Option`s below.
    pub row_says: Says,
    // ── **What a desk is, continued** ──────────────────────────────────
    //
    // Everything below reached this struct late — it lived in the SETTINGS
    // alone, which made each of these a property of the machine rather than
    // of the song: open any project and it wore the last session's routing,
    // its sidechains and its bypasses. The owner, having been shown the
    // list: "all the things you flagged should be in document."
    //
    // **`None` means the document does not SAY**, and is not the same as an
    // empty one. A file written before its key predates the whole idea of
    // carrying it, so the live value stands and nothing changes for that
    // project; a file written since always carries one (`to_snap` has an
    // opinion for every field), and then the document wins outright.
    //
    // Nothing audible may change because a field was added: zeroing a
    // routing table would change what an old project SOUNDS like, silently,
    // on first open.
    /// **The desk's routing table** — `Routes`, every route's source,
    /// destination and gain. Song content by any reading: where a channel's
    /// audio GOES is not a property of the computer it is played on.
    pub routes: Option<crate::routing::Routes>,
    /// **The sidechain key edges** — which channel keys which bay. Desk
    /// topology, as `KeyEdge`'s own note says, which is exactly why it
    /// belongs to the desk the document describes.
    pub insert_keys: Option<crate::routing::KeyEdges>,
    /// **Which bays are bypassed** — out of circuit but still loaded. By
    /// identity, as `plugins` is keyed.
    pub bypass: Option<std::collections::BTreeSet<crate::desk_id::SeatBay>>,
    // ── **The rack, the levels, and the rest of what a song is** ───────
    //
    // The SECOND wave to reach this struct late, and it reached it for the
    // reason the block above gives: every one of these lived in the
    // settings alone, so opening any project wore the last session's
    // reverb, its varispeed, its click division, its key signature and its
    // editor feel. The owner: "I need the desk settings (all send knobs in
    // recorder and levels) to be saved per session. They currently don't
    // count in the parameters set by a session file. Everything should be
    // session dependent."
    //
    // They answer to the block above in every particular — `None` is a
    // document that does not SAY, the live value stands for it, and a LIVE
    // desk always says — so there is one rule here and not two.
    /// **The effects rack**: the eight band knobs and their parameter rows.
    /// The most audible of this wave by a distance — without it a saved song
    /// opens through whatever reverb the last one was using. See [`Rack`].
    pub rack: Option<Rack>,
    /// `track_gain`, the backing track's level — the one member of
    /// `recorder::Gains` that never travelled. `metronome`, `master` and
    /// `fx_return` are the plain fields above.
    pub track_gain: Option<f64>,
    /// `master_mono`: the master folded to mono on the way out.
    pub master_mono: Option<bool>,
    /// `lufs_window`: which window the loudness ladder reads. Carried as the
    /// enum and written as its INDEX, the settings file's own append-only
    /// rule — a saved 2 has to go on meaning Integrated.
    pub lufs_window: Option<crate::recorder::LoudnessWindow>,
    /// `varispeed`: the dial and its range, **as the model and never as
    /// three loose numbers** — see `Settings::varispeed`, whose invariant is
    /// the whole reason it is not taken apart.
    pub varispeed: Option<crate::varispeed::Varispeed>,
    /// See [`Click`].
    pub click: Option<Click>,
    /// `transpose`, in semitones.
    pub transpose: Option<i64>,
    /// See [`CountIn`].
    pub count_in: Option<CountIn>,
    /// `record_take_name`. **An empty string is "no name", not "does not
    /// say"** — the outer `Option` is the only thing that means that, which
    /// is what lets a document CLEAR a name the live session is holding
    /// rather than only replace it.
    pub take_name: Option<String>,
    /// See [`Feel`].
    pub feel: Option<Feel>,
    /// See [`Notation`].
    pub notation: Option<Notation>,
    /// See [`Look`].
    pub look: Option<Look>,
    /// **The arrangement's grid and window.** See [`View`]. `None` in every
    /// document written before it, which reads as "keep whatever the app
    /// has" rather than as a set of zeroes.
    pub view: Option<View>,
    /// `start_stop`: the tape's start and stop, the switch and its two times
    /// (`Settings::start_stop`), **as the model**, whose times are whole
    /// centiseconds so the file, the fingerprint and the field read one
    /// number. `None` is a document from before it, and the live switch
    /// stands: an old project must not start spinning up, or stop doing so,
    /// because a key was added.
    pub start_stop: Option<crate::motor::StartStop>,
    /// **The drum bus, the click, the effects return and the master**: each
    /// one's name, colour, sends, mute, solo and polarity (W5). Always said:
    /// a legacy document's are read off its strip arrays, a format-2 desk
    /// writes them, the live desk has them. A special's polarity counts only
    /// when the rows' does ([`Says::PHASE`]): they were one mask.
    pub specials: crate::desk_model::Specials,
    // ── **Identity** (W3) ──────────────────────────────────────────────
    /// The next id the desk mints (`Settings::next_track_id`); zero on a
    /// desk that does not say, where the ids it holds decide. Bookkeeping,
    /// not the desk: it moves only when a row's id does, so two snapshots
    /// that agree on the rows are equal whatever it says ([`Aside`]).
    pub next_id: Aside<u64>,
    /// **Which legacy shape this document's `state/` folder was written
    /// in**, while any plugin there may still have only its legacy sidecar
    /// (design 6.6): set by the legacy read, carried by every save, cleared
    /// only by the host after a verified mirror (`forget_legacy_states`).
    /// Not a fact anybody edits, so it is not in the walk, two snapshots of
    /// one desk are equal whatever it says ([`Aside`]), and undo never
    /// moves it.
    pub legacy_states: Aside<Option<crate::desk_lift::DeskEra>>,
    /// **What the reader dropped** from a format-2 desk (design 6.3), for
    /// the opener's one notice. Never written and never hashed; empty on
    /// every snapshot but a document just read.
    pub dropped: Aside<Vec<crate::desk_format::Dropped>>,
    /// **Where the arrangement was left looking**: the window onto the song
    /// (its first second and how many seconds wide), so a project opens at
    /// the stretch somebody was working on and not at whatever the last
    /// project on this machine was zoomed to. The owner: "make it so that
    /// the viewport ... is session bound".
    ///
    /// [`Aside`], and kept out of the walk on purpose, because a zoom is not
    /// an edit: the window moves on every wheel notch, every edge scroll and
    /// every frame a take grows past the right edge, so in the walk it would
    /// light the asterisk on a scroll and push an undo moment per notch. It
    /// is written with the document at a save, read back at an open, and
    /// left alone by undo (`TangentApp::desk_snapshot` is its one writer).
    /// `None` is a document that does not say, which opens at the default.
    pub viewport: Aside<Option<Viewport>>,
}

/// **The arrangement's window onto the song**, as a document carries it:
/// see [`DeskSnap::viewport`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// The window's left edge, in seconds from the song's start.
    pub start_s: f64,
    /// How many seconds the window spans: the horizontal zoom.
    pub len_s: f64,
}

impl Viewport {
    /// A window a reader may trust: finite, starting at or after zero, and
    /// some width wide. Anything else is no window, which opens at the
    /// default rather than at a picture nobody can use.
    pub fn sane(start_s: f64, len_s: f64) -> Option<Self> {
        (start_s.is_finite() && len_s.is_finite() && start_s >= 0.0 && len_s > 0.0)
            .then_some(Self { start_s, len_s })
    }
}

/// **A note the reader keeps beside a desk that is not part of the desk**:
/// equal to every other, so a snapshot read from a file and the snapshot
/// that file was written from compare equal whatever the reader noticed
/// about the FILE (its legacy era, what it dropped). The desk is the rows;
/// these are about where the rows came from.
#[derive(Debug, Clone, Default)]
pub struct Aside<T>(pub T);

impl<T> PartialEq for Aside<T> {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

/// **Which of the late fields a snapshot SAYS**, one bit each. A format-2
/// desk says all of them; a legacy document says what its build knew, and
/// the live desk stands for the rest. The four that are facts of a ROW
/// ([`Says::ROWS`]: polarity, the theory switch, the instruments, the
/// freezes) are stored ([`DeskSnap::row_says`]), because a row has no
/// `None` to say it with; the rest are derived from their `Option`s, so the
/// two halves can never disagree ([`DeskSnap::says`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Says(pub u32);

impl Says {
    pub const PHASE: Self = Self(1);
    pub const TO_THEORY: Self = Self(1 << 1);
    pub const ROUTES: Self = Self(1 << 2);
    pub const KEYS: Self = Self(1 << 3);
    pub const BYPASS: Self = Self(1 << 4);
    pub const INSTRUMENTS: Self = Self(1 << 5);
    pub const RACK: Self = Self(1 << 6);
    pub const TRACK_GAIN: Self = Self(1 << 7);
    pub const MASTER_MONO: Self = Self(1 << 8);
    pub const LUFS_WINDOW: Self = Self(1 << 9);
    pub const VARISPEED: Self = Self(1 << 10);
    pub const CLICK: Self = Self(1 << 11);
    pub const TRANSPOSE: Self = Self(1 << 12);
    pub const COUNT_IN: Self = Self(1 << 13);
    pub const TAKE_NAME: Self = Self(1 << 14);
    pub const FEEL: Self = Self(1 << 15);
    pub const NOTATION: Self = Self(1 << 16);
    pub const LOOK: Self = Self(1 << 17);
    pub const VIEW: Self = Self(1 << 18);
    pub const FROZEN: Self = Self(1 << 19);
    pub const START_STOP: Self = Self(1 << 20);
    /// Every field: what a live desk and a format-2 desk say.
    pub const ALL: Self = Self((1 << 21) - 1);
    /// The late facts that live on a row: what [`DeskSnap::row_says`] holds.
    pub const ROWS: Self =
        Self(Self::PHASE.0 | Self::TO_THEORY.0 | Self::INSTRUMENTS.0 | Self::FROZEN.0);

    pub const fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }

    /// Both sets at once.
    #[must_use]
    pub const fn with(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }
}

/// **The recorder's EFFECTS RACK** — the eight knobs across the band and
/// every parameter row behind them.
///
/// One field of [`DeskSnap`] rather than nine, on [`Instruments`]' argument:
/// they are one fact — how the effects bus is set — and nine keys that must
/// be read and written together are nine chances to carry eight of them.
///
/// The eight are the eight `Settings::knobs` hands the engine as
/// `recorder::FxSends`, in that order and in that spelling, so a knob added
/// to the band is a field added here rather than a knob that silently stops
/// travelling. `params` is `Settings::effect_params` whole — a flat map of
/// 0..=1 values keyed by the names the right-click menu uses, plus the
/// delay's division, which is a name and not a number. Copied rather than
/// filtered: the keys belong to the menu, and a build that has not learned a
/// parameter must still round-trip a document written by one that has.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Rack {
    pub reverb: f64,
    pub delay: f64,
    pub chorus: f64,
    pub phaser: f64,
    pub hpf: f64,
    pub lpf: f64,
    pub gain: f64,
    pub limiter: f64,
    pub params: Map<String, Value>,
}

/// **The click, past its switch and its fader.**
///
/// `metronome_on` and `metronome_gain` have ridden the desk since it
/// existed; these three never did — so a song in 6/8 left clicking twice a
/// bar opened clicking however the last song clicked, and a take could be
/// printed with a click somebody had turned off two projects ago.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Click {
    /// `metronome_div`: clicks per beat, 0 for one per BAR.
    pub div: i64,
    /// `metronome_in_take`: the click is mixed into the RECORDING.
    pub in_take: bool,
    /// `click_sample`: the sound file, empty for the one that ships.
    pub sample: String,
}

/// **What the session LOOKS like** — its theme, and the channel palette.
///
/// The owner asked for it after everything else had moved: "make themes
/// session dependent". A theme was one of the few things the 2026-09-15 sweep
/// left in settings on purpose, beside the hardware and the window, and the
/// reason it is a fair thing to move is that a theme is about the WORK -
/// which song this is - and not about the machine it is being made on.
///
/// **The NAME, and not the colours.** That is the app's own model of a theme
/// already: `Theme::apply`'s doc says "the theme name is the record. Someone
/// who then hand-edits one colour has a modified theme, and `capture` is how
/// they keep it." Storing forty resolved colours per document would freeze
/// every project against a theme that is later edited, and would make a
/// hand-tweaked palette undistinguishable from a named one. A name that has
/// gone - a deleted user theme - falls back the way every other name lookup
/// in this app falls back, to the one theme that always exists.
///
/// The palette travels beside it because it is a colour decision the user
/// makes in the same breath, and `palette_auto` travels beside THAT because
/// without it a document could not say the difference between "this session
/// follows its theme's palette" and "this session was left on palette 6".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Look {
    /// `theme_name`.
    pub theme: String,
    /// `palette_choice`: the index into `mixer_panel::PALETTES`.
    pub palette: i64,
    /// `palette_auto`: the palette follows the theme rather than a hand pick.
    pub palette_auto: bool,
    /// **Which view the session was left showing**: `"piano"`, `"mixer"` or
    /// `"arrange"`. A word rather than the settings' number, because this
    /// one is written into somebody's project file and read by people.
    /// Anything else, including the empty string a document from before
    /// this carries, reads as the piano.
    pub view: String,
}

/// **The count-in** — its length in bars, the legacy beats key that length
/// was converted from, and whether it is printed into the take.
///
/// The beats travel beside the bars because `Settings::convert_count_in_to_bars`
/// still reads them: carrying one without the other would leave a document
/// able to re-run that conversion against a number its other half no longer
/// agrees with.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct CountIn {
    pub bars: i64,
    pub beats: i64,
    pub in_take: bool,
}

/// **How the MIDI editor places a note** — its magnet, its grid, its swing
/// and how far HUMANIZE may pull one off both.
///
/// A feel dialled in for one song, which is precisely the thing that should
/// not follow somebody into the next one.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Feel {
    pub snap: bool,
    pub grid_exp: i64,
    pub swing: bool,
    pub swing_pct: f64,
    pub triplet: bool,
    pub humanize_timing: u8,
    pub humanize_vel: u8,
}

/// **How the ARRANGEMENT was left looking** — its grid, and the shape of
/// the window onto the song.
///
/// [`Feel`] is the MIDI editor's half of this and has been in the document
/// for a while; this is the timeline's, and the owner asked for it on the
/// same reasoning: "Make the met, grid, tempo arms all session dependent as
/// well as the viewport and related settings". The metronome, the tempo, the
/// signature and the editor's grid were already here. The arrangement's own
/// grid and window were not, so a session opened on a machine that had last
/// worked on something else came up with that other song's zoom and
/// division.
///
/// **A song's grid is a property of the song.** A piece written in bars of
/// 6/8 wants a different division from one in 4/4, and "how far in am I
/// looking" is where somebody left off rather than a preference about
/// Tangent.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// `arr_snap`: the timeline's magnet.
    pub snap: bool,
    /// `arr_grid_exp`: which division its lines are drawn at.
    pub grid_exp: i64,
    /// `arr_tempo_lane`: whether the tempo lane is showing.
    pub tempo_lane: bool,
    /// `arr_master_row`: whether the master row is showing.
    pub master_row: bool,
    /// `arr_header_w`: how wide the track headers are.
    pub header_w: f64,
    /// `arr_visible`: how many lanes fit the window — the vertical zoom.
    pub visible: u8,
    /// `arr_visible_at`: **the pane height that count was chosen at**, which
    /// has to travel WITH it. A count is a density, and `app::carried_count`
    /// reads this to give the same lane height on a different display; a
    /// document carrying the count alone would be scaled against whatever
    /// height the last session happened to choose one at. Zero is "not
    /// recorded", the settings field's own sentinel.
    pub visible_at: f64,
    /// **The quantize window's own grid and strength**, which travel here
    /// rather than in [`Feel`] because they are the arrangement's question:
    /// `Q` quantizes audio on the timeline and notes in the editor, and the
    /// window states one grid for both.
    pub quantize_grid_exp: i64,
    pub quantize_triplet: bool,
    pub quantize_swing_pct: f64,
    pub quantize_strength_pct: f64,
    pub quantize_window_ms: f64,
    /// `quantize_sensitivity`: how readily the audio side calls something an
    /// attack. A property of the MATERIAL on this timeline, which is why it
    /// travels: the setting that finds a soft brush kit's hits buries a
    /// close-miked snare in false ones.
    pub quantize_sensitivity: f64,
    /// `quantize_fit`: how a warped segment gets where it belongs, by key,
    /// verbatim. Empty is auto. Carried as the word and never as an index,
    /// the same rule `stretch_mode` and `Notation::tuning` keep — a name an
    /// older build cannot resolve falls back at the point of use and is
    /// never rewritten.
    pub quantize_fit: String,
}

/// **What the music is written in** — the staff's key signature, whether
/// accidentals read as flats, and the guitar's tuning and capo.
///
/// Song facts under any reading: a piece in E flat is in E flat on every
/// machine, and a capo at the second fret is a property of the arrangement
/// rather than of somebody's install.
///
/// `tuning` and `capo_style` are carried VERBATIM, which is the settings'
/// own rule for both: an unknown name falls back at the point of use and is
/// never rewritten, so a document that meets an older build keeps what it
/// said for the build that can read it.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Notation {
    pub staff_key: i32,
    pub prefer_flats: bool,
    pub capo: i64,
    pub capo_style: String,
    pub tuning: String,
}

/// **A frozen lane: its rendered self, and where it lives.**
///
/// A freeze renders the lane's clips through its rack, post-insert and
/// pre-fader, into one file under the project's `media/`, and while the
/// lane is frozen that file plays in place of the clips with the rack
/// stood down — the plugins keep their state and pay nothing. The fader,
/// pan, mute, solo and sends stay live, which is why the render leaves
/// them out. `len_s` is the file's length, kept here so the host never has
/// to resolve it late the way a clip's `len_s == 0` is resolved.
#[derive(Debug, Clone, PartialEq)]
pub struct FrozenLane {
    pub lane: usize,
    /// Relative to the document, like a clip's `path` — see `resolve`.
    pub path: String,
    pub len_s: f64,
}

/// **Which row a desk arms when nothing says otherwise.**
///
/// All arms off, save one, by the owner's rule: a project must be openable
/// without headphones and without being blasted, and the one exception is the
/// row that makes the piano sound — the first MIDI row holding one of
/// `dialogs::SHIPPED_PIANOS` (Tangent Keys since 5.2, the built-in DX7 before
/// it). Which bays count is the caller's to say: the settings and the app ask
/// all six (`Settings::row_holds`), a document asks the instrument seat.
/// `kinds` is a byte per row (`RowKind::byte`), as many as the desk has.
///
/// One rule, two callers: `app::default_armed` asks it about a `Settings` and
/// [`DeskSnap::from_value`] asks it about a document that carries no `armed`
/// key. A second copy of it is a launch and an open that disagree about what
/// a silent desk means.
pub fn armed_by_default(kinds: &[u8], mut bay0: impl FnMut(usize) -> bool) -> Option<usize> {
    (0..kinds.len()).find(|n| kinds[*n] == 1 && bay0(*n))
}

/// **The desk by reference** — every field of [`DeskSnap`] borrowed from
/// wherever it lives, so the once-a-frame fingerprint can walk the LIVE
/// desk without first cloning every row into a snapshot it throws away a
/// microsecond later.
///
/// This is THE walk, and the one derivation. `DeskSnap::hash_into` goes
/// through `as_ref`, the snapshot the app takes is built by `to_snap`, and
/// the app reads its settings into one of these in exactly one function —
/// so the cheap question (`project_dirty`), the exact one
/// (`project_dirty_exact`) and the document's own desk all see the same
/// fields derived the same way. Two walkers over two derivations was the
/// shape that could disagree, and a disagreement here is a title bar blind
/// to a fader the quit prompt can see.
#[derive(Clone, Copy)]
pub struct DeskRef<'a> {
    /// See [`DeskSnap::rows`].
    pub rows: &'a [crate::desk_model::TrackRow],
    /// See [`DeskSnap::row_says`]. The LIVE desk says every one.
    pub row_says: Says,
    /// See [`DeskSnap::plugins`].
    pub plugins: &'a std::collections::BTreeMap<crate::desk_id::SeatBay, String>,
    pub drums_gain: f64,
    pub drums_pan: f64,
    pub metronome_gain: f64,
    pub master_gain: f64,
    pub fx_return: f64,
    pub tempo_bpm: f64,
    pub sig_beats: u8,
    pub sig_unit: u8,
    pub metronome_on: bool,
    /// The late fields that are not a row's — see the block on
    /// [`DeskSnap`]. Borrowed like everything else here so the once-a-frame
    /// walk clones nothing; `None` is a document that does not say, and the
    /// LIVE desk always says, so `desk_view` fills every one of them.
    pub routes: Option<&'a crate::routing::Routes>,
    /// See [`DeskSnap::insert_keys`].
    pub insert_keys: Option<&'a crate::routing::KeyEdges>,
    /// See [`DeskSnap::bypass`].
    pub bypass: Option<&'a std::collections::BTreeSet<crate::desk_id::SeatBay>>,
    /// The second late wave — see the block on [`DeskSnap`]. Borrowed on
    /// the same terms as the three above: the once-a-frame walk clones
    /// nothing, and `desk_view` fills every one of them.
    pub rack: Option<RackRef<'a>>,
    /// See [`DeskSnap::track_gain`].
    pub track_gain: Option<f64>,
    /// See [`DeskSnap::master_mono`].
    pub master_mono: Option<bool>,
    /// See [`DeskSnap::lufs_window`].
    pub lufs_window: Option<crate::recorder::LoudnessWindow>,
    /// See [`DeskSnap::varispeed`].
    pub varispeed: Option<crate::varispeed::Varispeed>,
    /// See [`DeskSnap::click`].
    pub click: Option<ClickRef<'a>>,
    /// See [`DeskSnap::transpose`].
    pub transpose: Option<i64>,
    /// See [`DeskSnap::count_in`].
    pub count_in: Option<CountIn>,
    /// See [`DeskSnap::take_name`].
    pub take_name: Option<&'a str>,
    /// See [`DeskSnap::feel`].
    pub feel: Option<Feel>,
    /// See [`DeskSnap::notation`].
    pub notation: Option<NotationRef<'a>>,
    /// See [`DeskSnap::look`].
    pub look: Option<LookRef<'a>>,
    /// See [`DeskSnap::view`].
    pub view: Option<ViewRef<'a>>,
    /// See [`DeskSnap::start_stop`]. `Copy`, so it needs no borrowed twin.
    pub start_stop: Option<crate::motor::StartStop>,
    /// See [`DeskSnap::next_id`]. Not hashed: it moves only when an id is
    /// minted, which moves a row.
    pub next_id: u64,
    /// See [`DeskSnap::specials`].
    pub specials: &'a crate::desk_model::Specials,
}

/// A node into the fingerprint: its kind, and a track's id.
fn hash_node(f: &mut Fingerprint, n: crate::desk_id::Node) {
    use crate::desk_id::Node;
    match n {
        Node::Track(id) => {
            f.u64(0);
            f.u64(u64::from(id.0));
        }
        Node::Drums => f.u64(1),
        Node::Click => f.u64(2),
        Node::Fx => f.u64(3),
        Node::Master => f.u64(4),
    }
}

/// A bay into the fingerprint: its node, its rack and the bay.
fn hash_bay(f: &mut Fingerprint, sb: crate::desk_id::SeatBay) {
    use crate::desk_id::Rack;
    hash_node(f, sb.seat.node);
    match sb.seat.rack {
        Rack::Chain(c) => {
            f.u64(0);
            f.u64(u64::from(c));
        }
        Rack::Pedal(p) => {
            f.u64(1);
            f.u64(u64::from(p));
        }
        Rack::Pad(n) => {
            f.u64(2);
            f.u64(u64::from(n));
        }
    }
    f.u64(u64::from(sb.bay));
}

/// [`Rack`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct RackRef<'a> {
    pub reverb: f64,
    pub delay: f64,
    pub chorus: f64,
    pub phaser: f64,
    pub hpf: f64,
    pub lpf: f64,
    pub gain: f64,
    pub limiter: f64,
    pub params: &'a Map<String, Value>,
}

/// [`Click`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct ClickRef<'a> {
    pub div: i64,
    pub in_take: bool,
    pub sample: &'a str,
}

/// [`Notation`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct NotationRef<'a> {
    pub staff_key: i32,
    pub prefer_flats: bool,
    pub capo: i64,
    pub capo_style: &'a str,
    pub tuning: &'a str,
}

/// [`Look`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct LookRef<'a> {
    pub theme: &'a str,
    pub palette: i64,
    pub palette_auto: bool,
    /// See [`Look::view`].
    pub view: &'a str,
}

/// [`View`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct ViewRef<'a> {
    pub snap: bool,
    pub grid_exp: i64,
    pub tempo_lane: bool,
    pub master_row: bool,
    pub header_w: f64,
    pub visible: u8,
    pub visible_at: f64,
    pub quantize_grid_exp: i64,
    pub quantize_triplet: bool,
    pub quantize_swing_pct: f64,
    pub quantize_strength_pct: f64,
    pub quantize_window_ms: f64,
    pub quantize_sensitivity: f64,
    /// See [`View::quantize_fit`].
    pub quantize_fit: &'a str,
}

impl ViewRef<'_> {
    /// The one place a borrowed view is copied into an owned one - the
    /// desk's `to_snap` and the app's per-frame comparison both take it,
    /// so the two cannot disagree about what a view IS.
    #[must_use]
    pub fn to_view(&self) -> View {
        View {
            snap: self.snap,
            grid_exp: self.grid_exp,
            tempo_lane: self.tempo_lane,
            master_row: self.master_row,
            header_w: self.header_w,
            visible: self.visible,
            visible_at: self.visible_at,
            quantize_grid_exp: self.quantize_grid_exp,
            quantize_triplet: self.quantize_triplet,
            quantize_swing_pct: self.quantize_swing_pct,
            quantize_strength_pct: self.quantize_strength_pct,
            quantize_window_ms: self.quantize_window_ms,
            quantize_sensitivity: self.quantize_sensitivity,
            quantize_fit: self.quantize_fit.to_owned(),
        }
    }
}

impl DeskRef<'_> {
    /// The one place a desk is copied into an owned snapshot.
    pub fn to_snap(&self) -> DeskSnap {
        DeskSnap {
            rows: self.rows.to_vec(),
            row_says: self.row_says,
            plugins: self.plugins.iter().filter(|(_, p)| !p.is_empty()).map(|(b, p)| (*b, p.clone())).collect(),
            drums_gain: self.drums_gain,
            drums_pan: self.drums_pan,
            metronome_gain: self.metronome_gain,
            master_gain: self.master_gain,
            fx_return: self.fx_return,
            tempo_bpm: self.tempo_bpm,
            sig_beats: self.sig_beats,
            sig_unit: self.sig_unit,
            metronome_on: self.metronome_on,
            routes: self.routes.cloned(),
            insert_keys: self.insert_keys.cloned(),
            bypass: self.bypass.cloned(),
            rack: self.rack.map(|r| Rack {
                reverb: r.reverb,
                delay: r.delay,
                chorus: r.chorus,
                phaser: r.phaser,
                hpf: r.hpf,
                lpf: r.lpf,
                gain: r.gain,
                limiter: r.limiter,
                params: r.params.clone(),
            }),
            track_gain: self.track_gain,
            master_mono: self.master_mono,
            lufs_window: self.lufs_window,
            varispeed: self.varispeed,
            click: self.click.map(|c| Click {
                div: c.div,
                in_take: c.in_take,
                sample: c.sample.to_owned(),
            }),
            transpose: self.transpose,
            count_in: self.count_in,
            view: self.view.map(|v| v.to_view()),
            start_stop: self.start_stop,
            look: self.look.map(|l| Look {
                theme: l.theme.to_owned(),
                palette: l.palette,
                palette_auto: l.palette_auto,
                view: l.view.to_owned(),
            }),
            take_name: self.take_name.map(str::to_owned),
            feel: self.feel,
            notation: self.notation.map(|n| Notation {
                staff_key: n.staff_key,
                prefer_flats: n.prefer_flats,
                capo: n.capo,
                capo_style: n.capo_style.to_owned(),
                tuning: n.tuning.to_owned(),
            }),
            next_id: Aside(self.next_id),
            specials: self.specials.clone(),
            // The document's own, which the app carries across: see
            // `TangentApp::desk_snapshot`.
            legacy_states: Aside(None),
            dropped: Aside(Vec::new()),
            // Not the desk's: the app puts the live window on in
            // `TangentApp::desk_snapshot`.
            viewport: Aside(None),
        }
    }

    /// The desk into a [`Fingerprint`], field for field with
    /// `DeskSnap::to_value`.
    ///
    /// **The two walk together or the title bar goes blind.** A field added
    /// to the struct and to `to_value` but not to here is a desk change no
    /// comparison can see — the asterisk stays off while a fader stands
    /// somewhere new. `the_fingerprint_moves_when_the_document_moves`
    /// is what catches that, and it catches it by MUTATING every field
    /// rather than by reading this list; `the_cheap_dirty_answer_and_the_
    /// exact_one_agree` does the same from the settings side.
    ///
    /// **A term here is an UNDOABLE field.** This walk is what an undo
    /// moment is compared by (`TangentApp::walk`), so adding one is three
    /// jobs, not one: add it here; give every gesture that writes it exactly
    /// one commit (or make it part of what `commit_view_change` commits once
    /// a frame: `View`, `Feel` and the view on show); and make
    /// `apply_desk_snapshot` restore every value it can hold, sentinels
    /// included - a value the apply declines leaves undo re-applying that
    /// moment on every press.
    fn hash_into(&self, f: &mut Fingerprint) {
        // **Which late row facts the rows say**, then **every row whole**:
        // its identity and every fact of it, the late ones only when said
        // (a fact the document does not say is no fact of the document's).
        // Counted first, so the words parse back into exactly one desk. A
        // reorder moves rows and so moves these words; nothing below them.
        f.u64(u64::from(self.row_says.0));
        f.len(self.rows.len());
        for r in self.rows {
            hash_row(f, r, self.row_says);
        }
        // The occupied bays, counted first so the words parse back into
        // exactly one desk, then each as its bay by identity and its path.
        // A reorder moves no bay, so it moves none of these words.
        let plugins = || self.plugins.iter().filter(|(_, p)| !p.is_empty());
        f.len(plugins().count());
        for (bay, path) in plugins() {
            hash_bay(f, *bay);
            f.str(path);
        }
        f.f64(self.drums_gain);
        f.f64(self.drums_pan);
        f.f64(self.metronome_gain);
        f.f64(self.master_gain);
        f.f64(self.fx_return);
        f.f64(self.tempo_bpm);
        f.u64(u64::from(self.sig_beats));
        f.u64(u64::from(self.sig_unit));
        f.bool(self.metronome_on);
        // **The late fields that are not a row's.** Each hashes its ABSENCE
        // distinctly from an empty one — a document that does not carry
        // routes and a document that carries none are different documents,
        // and the whole meaning of `None` here rests on telling them apart.
        let opt = |f: &mut Fingerprint, present: bool| f.bool(present);
        opt(f, self.routes.is_some());
        if let Some(r) = self.routes {
            f.len(r.0.len());
            for route in &r.0 {
                hash_node(f, route.from);
                match route.target {
                    crate::routing::Target::Hardware { first, stereo } => {
                        f.u64(0);
                        f.u64(first as u64);
                        f.bool(stereo);
                    }
                    crate::routing::Target::Node(n) => {
                        f.u64(1);
                        hash_node(f, n);
                    }
                }
                f.f64(f64::from(route.gain));
            }
        }
        opt(f, self.insert_keys.is_some());
        if let Some(k) = self.insert_keys {
            f.len(k.0.len());
            for e in &k.0 {
                hash_bay(f, e.seat);
                hash_node(f, e.from);
            }
        }
        opt(f, self.bypass.is_some());
        if let Some(b) = self.bypass {
            f.len(b.len());
            for bay in b {
                hash_bay(f, *bay);
            }
        }
        // **The second late wave**, on the first wave's terms: each hashes
        // its ABSENCE distinctly from any value it could hold, because a
        // document that does not carry a rack and a document that carries a
        // dry one are different documents.
        opt(f, self.rack.is_some());
        if let Some(r) = self.rack {
            for v in [r.reverb, r.delay, r.chorus, r.phaser, r.hpf, r.lpf, r.gain, r.limiter]
            {
                f.f64(v);
            }
            // **The parameter rows by SORTED key.** The map is an
            // `IndexMap`, whose equality ignores order, so a walk in
            // insertion order would call two equal racks different — an
            // asterisk on a document nobody had touched, and a save prompt
            // on the way out of a session somebody only looked at.
            let mut rows: Vec<(&String, &Value)> = r.params.iter().collect();
            rows.sort_by(|a, b| a.0.cmp(b.0));
            f.len(rows.len());
            for (k, v) in rows {
                f.str(k);
                f.str(&v.to_string());
            }
        }
        f.maybe_f64(self.track_gain);
        opt(f, self.master_mono.is_some());
        f.bool(self.master_mono.unwrap_or(false));
        opt(f, self.lufs_window.is_some());
        f.len(self.lufs_window.unwrap_or_default().index());
        opt(f, self.varispeed.is_some());
        if let Some(v) = self.varispeed {
            f.f64(f64::from(v.pos()));
            f.u64(u64::from(v.min()));
            f.u64(u64::from(v.max()));
        }
        opt(f, self.click.is_some());
        if let Some(c) = self.click {
            f.u64(c.div as u64);
            f.bool(c.in_take);
            f.str(c.sample);
        }
        opt(f, self.transpose.is_some());
        f.u64(self.transpose.unwrap_or(0) as u64);
        opt(f, self.count_in.is_some());
        if let Some(c) = self.count_in {
            f.u64(c.bars as u64);
            f.u64(c.beats as u64);
            f.bool(c.in_take);
        }
        opt(f, self.take_name.is_some());
        f.str(self.take_name.unwrap_or(""));
        opt(f, self.feel.is_some());
        if let Some(t) = self.feel {
            f.bool(t.snap);
            f.u64(t.grid_exp as u64);
            f.bool(t.swing);
            f.f64(t.swing_pct);
            f.bool(t.triplet);
            f.u64(u64::from(t.humanize_timing));
            f.u64(u64::from(t.humanize_vel));
        }
        opt(f, self.notation.is_some());
        if let Some(n) = self.notation {
            f.u64(n.staff_key as u64);
            f.bool(n.prefer_flats);
            f.u64(n.capo as u64);
            f.str(n.capo_style);
            f.str(n.tuning);
        }
        opt(f, self.look.is_some());
        if let Some(l) = self.look {
            f.str(l.theme);
            f.u64(l.palette as u64);
            f.bool(l.palette_auto);
            f.str(l.view);
        }
        // The arrangement's grid and window.
        opt(f, self.view.is_some());
        if let Some(v) = self.view {
            f.bool(v.snap);
            f.u64(v.grid_exp as u64);
            f.bool(v.tempo_lane);
            f.bool(v.master_row);
            f.f64(v.header_w);
            f.u64(u64::from(v.visible));
            f.u64(v.quantize_grid_exp as u64);
            f.bool(v.quantize_triplet);
            f.f64(v.quantize_swing_pct);
            f.f64(v.quantize_strength_pct);
            f.f64(v.quantize_window_ms);
            f.f64(v.visible_at);
            f.f64(v.quantize_sensitivity);
            f.str(if v.quantize_fit == "auto" { "" } else { v.quantize_fit });
        }
        // **The specials** (W5): each one's name, paint, sends and switches.
        // A special's polarity is hashed only when the rows' is said: they
        // were one mask, and a document from before the key said neither.
        let phase_said = self.row_says.contains(Says::PHASE);
        for (node, sp) in self.specials.iter() {
            hash_node(f, node);
            f.str(&sp.name);
            f.u64(sp.color as u64);
            f.len(sp.sends.len());
            for send in &sp.sends {
                hash_node(f, send.to);
                f.f64(send.level);
                f.bool(send.pre);
            }
            f.bool(sp.mute);
            f.bool(sp.solo);
            f.bool(phase_said && sp.phase);
        }
        // **The tape's start and stop, APPENDED**: after every word an
        // earlier build hashed, so none of theirs moves, and its absence
        // apart from every value it could hold, as the late fields above.
        // The times as the centiseconds they are held in, the number the
        // file and the field read.
        opt(f, self.start_stop.is_some());
        if let Some(m) = self.start_stop {
            f.bool(m.on());
            f.u64(u64::from(m.start_cs()));
            f.u64(u64::from(m.stop_cs()));
        }
    }
}

/// **One row into the fingerprint**: its identity and every fact of it,
/// exhaustively (a field added to the row and not here is a compile error,
/// not a desk change the title bar cannot see), the late facts only when the
/// desk `says` them.
fn hash_row(f: &mut Fingerprint, r: &crate::desk_model::TrackRow, says: Says) {
    let crate::desk_model::TrackRow {
        id,
        kind,
        added,
        icon,
        name,
        color,
        gain,
        pan,
        input,
        midi,
        mute,
        solo,
        phase,
        arm,
        theory,
        folder,
        in_folder,
        shut,
        sends,
        dx7,
        keys,
        frozen,
    } = r;
    f.u64(u64::from(id.0));
    f.u64(u64::from(crate::desk_model::RowKind::byte(*kind)));
    f.bool(*added);
    f.u64(u64::from(*icon));
    f.str(name);
    f.u64(*color as u64);
    f.f64(*gain);
    f.f64(*pan);
    f.str(input);
    // The device set, in the hash for the reason everything else is: the
    // dirty bit is what offers to save, and a setup somebody built in the
    // MIDI Inputs dialog that the document did not notice is a setup they
    // lose on the next close. 255 for "every channel" — never a real wire
    // channel, so it cannot collide with one.
    f.len(midi.devices.len());
    for d in &midi.devices {
        f.str(d);
    }
    f.u64(u64::from(midi.channel.unwrap_or(255)));
    f.bool(*mute);
    f.bool(*solo);
    f.bool(*arm);
    f.bool(*folder);
    f.bool(*in_folder);
    f.bool(*shut);
    f.len(sends.len());
    for s in sends {
        hash_node(f, s.to);
        f.f64(s.level);
        f.bool(s.pre);
    }
    f.bool(says.contains(Says::PHASE) && *phase);
    f.bool(says.contains(Says::TO_THEORY) && *theory);
    if says.contains(Says::INSTRUMENTS) {
        f.str(&dx7.cart);
        f.u64(dx7.patch as u64);
        f.u64(keys.patch as u64);
        f.len(keys.params.len());
        for p in &keys.params {
            f.f64(*p);
        }
    }
    match frozen.as_ref().filter(|_| says.contains(Says::FROZEN)) {
        Some(z) => {
            f.u64(1);
            f.str(&z.path);
            f.u64(z.len_s.to_bits());
        }
        None => f.u64(0),
    }
}

impl DeskSnap {
    /// This snapshot by reference — see [`DeskRef`].
    pub fn as_ref(&self) -> DeskRef<'_> {
        DeskRef {
            rows: &self.rows,
            row_says: self.row_says,
            plugins: &self.plugins,
            drums_gain: self.drums_gain,
            drums_pan: self.drums_pan,
            metronome_gain: self.metronome_gain,
            master_gain: self.master_gain,
            fx_return: self.fx_return,
            tempo_bpm: self.tempo_bpm,
            sig_beats: self.sig_beats,
            sig_unit: self.sig_unit,
            metronome_on: self.metronome_on,
            routes: self.routes.as_ref(),
            insert_keys: self.insert_keys.as_ref(),
            bypass: self.bypass.as_ref(),
            rack: self.rack.as_ref().map(|r| RackRef {
                reverb: r.reverb,
                delay: r.delay,
                chorus: r.chorus,
                phaser: r.phaser,
                hpf: r.hpf,
                lpf: r.lpf,
                gain: r.gain,
                limiter: r.limiter,
                params: &r.params,
            }),
            track_gain: self.track_gain,
            master_mono: self.master_mono,
            lufs_window: self.lufs_window,
            varispeed: self.varispeed,
            click: self.click.as_ref().map(|c| ClickRef {
                div: c.div,
                in_take: c.in_take,
                sample: &c.sample,
            }),
            transpose: self.transpose,
            count_in: self.count_in,
            view: self.view.as_ref().map(|v| ViewRef {
                snap: v.snap,
                grid_exp: v.grid_exp,
                tempo_lane: v.tempo_lane,
                master_row: v.master_row,
                header_w: v.header_w,
                visible: v.visible,
                visible_at: v.visible_at,
                quantize_grid_exp: v.quantize_grid_exp,
                quantize_triplet: v.quantize_triplet,
                quantize_swing_pct: v.quantize_swing_pct,
                quantize_strength_pct: v.quantize_strength_pct,
                quantize_window_ms: v.quantize_window_ms,
                quantize_sensitivity: v.quantize_sensitivity,
                quantize_fit: &v.quantize_fit,
            }),
            start_stop: self.start_stop,
            look: self.look.as_ref().map(|l| LookRef {
                theme: &l.theme,
                palette: l.palette,
                palette_auto: l.palette_auto,
                view: &l.view,
            }),
            take_name: self.take_name.as_deref(),
            feel: self.feel,
            notation: self.notation.as_ref().map(|n| NotationRef {
                staff_key: n.staff_key,
                prefer_flats: n.prefer_flats,
                capo: n.capo,
                capo_style: &n.capo_style,
                tuning: &n.tuning,
            }),
            next_id: self.next_id.0,
            specials: &self.specials,
        }
    }

    /// **Which late fields this snapshot says**: the row facts it stores
    /// ([`Self::row_says`]) and the rest by their `Option`s. See [`Says`].
    pub fn says(&self) -> Says {
        let mut bits = self.row_says.0 & Says::ROWS.0;
        for (on, s) in [
            (self.routes.is_some(), Says::ROUTES),
            (self.insert_keys.is_some(), Says::KEYS),
            (self.bypass.is_some(), Says::BYPASS),
            (self.rack.is_some(), Says::RACK),
            (self.track_gain.is_some(), Says::TRACK_GAIN),
            (self.master_mono.is_some(), Says::MASTER_MONO),
            (self.lufs_window.is_some(), Says::LUFS_WINDOW),
            (self.varispeed.is_some(), Says::VARISPEED),
            (self.click.is_some(), Says::CLICK),
            (self.transpose.is_some(), Says::TRANSPOSE),
            (self.count_in.is_some(), Says::COUNT_IN),
            (self.take_name.is_some(), Says::TAKE_NAME),
            (self.feel.is_some(), Says::FEEL),
            (self.notation.is_some(), Says::NOTATION),
            (self.look.is_some(), Says::LOOK),
            (self.view.is_some(), Says::VIEW),
            (self.start_stop.is_some(), Says::START_STOP),
        ] {
            if on {
                bits |= s.0;
            }
        }
        Says(bits)
    }

    /// **This snapshot's rows' ids**, in lane order.
    pub fn row_ids(&self) -> Vec<crate::desk_id::TrackId> {
        self.rows.iter().map(|r| r.id).collect()
    }

    /// **Bay `bay` of row `row`'s own rack** (its first chain), by the
    /// identity this snapshot gives the row.
    pub fn row_bay(&self, row: usize, bay: usize) -> Option<crate::desk_id::SeatBay> {
        use crate::desk_id::{Node, Rack, SeatBay};
        let id = self.rows.get(row)?.id;
        Some(SeatBay::new(Node::Track(id), Rack::Chain(0), u8::try_from(bay).ok()?))
    }

    /// The plugin in [`Self::row_bay`], if any: where a track's instrument
    /// sits.
    pub fn row_plugin(&self, row: usize, bay: usize) -> Option<&str> {
        self.plugins.get(&self.row_bay(row, bay)?).map(String::as_str).filter(|p| !p.is_empty())
    }

    /// **Whether the track on `row` holds `path` in any of its six bays**
    /// (`desk_id::chain_bays`): where an instrument may sit.
    pub fn row_holds(&self, row: usize, path: &str) -> bool {
        use crate::desk_id::{Node, SeatBay};
        self.rows.get(row).is_some_and(|r| {
            crate::desk_id::chain_bays()
                .any(|(rack, bay)| self.plugins.get(&SeatBay::new(Node::Track(r.id), rack, bay)).is_some_and(|p| p == path))
        })
    }

    /// What stands at legacy FLAT bay `flat` under this snapshot's rows, if
    /// anything: the tests' way to ask by the frozen map's numbers.
    #[cfg(test)]
    pub fn bay(&self, flat: usize) -> Option<&str> {
        let sb = crate::desk_legacy::bay_of_flat(&self.row_ids(), flat)?;
        self.plugins.get(&sb).map(String::as_str)
    }

    /// Put `path` at legacy FLAT bay `flat` under this snapshot's rows, or
    /// empty it for an empty path: [`Self::bay`]'s writer.
    #[cfg(test)]
    pub fn set_bay(&mut self, flat: usize, path: &str) {
        let Some(sb) = crate::desk_legacy::bay_of_flat(&self.row_ids(), flat) else {
            return;
        };
        if path.is_empty() {
            self.plugins.remove(&sb);
        } else {
            self.plugins.insert(sb, path.to_owned());
        }
    }

    /// **The occupied bays as the frozen map's flat numbers**, ascending,
    /// under this snapshot's rows: the legacy writer's view, and the era
    /// corpus's. A bay of a row past the legacy width has no flat number and
    /// is not here.
    pub fn legacy_bays(&self) -> Vec<(usize, String)> {
        let ids = self.row_ids();
        let mut out: Vec<(usize, String)> = self
            .plugins
            .iter()
            .filter(|(_, p)| !p.is_empty())
            .filter_map(|(sb, p)| Some((crate::desk_legacy::flat_of(&ids, *sb)?, p.clone())))
            .collect();
        out.sort_by_key(|(n, _)| *n);
        out
    }

    /// Sorted, deduplicated (the first of two claims on a seat wins), free
    /// of empties and inside the seat map — the one shape a legacy file's
    /// seats may have before they are bound to identities.
    ///
    /// **The bound is the point.** A dense array's seat count was limited
    /// by the file (one `"",` per seat); a sparse key is not, and every
    /// consumer spread these pairs over a dense space sized by the bay —
    /// so `"inserts_tail": {"100000000000": "x"}` was a 20-byte request
    /// for 2.4 TB of strings, and `usize::MAX` an add that wraps to zero
    /// in release and emptied the live desk instead. Neither shape can
    /// reach the quarantine (both are valid JSON). The settings reader
    /// already refuses a `strip_inserts_tail` seat past `INSERT_SPACE`;
    /// this is the same law on the document road.
    fn canonical(mut pairs: Vec<(usize, String)>) -> Vec<(usize, String)> {
        pairs.retain(|(bay, p)| !p.is_empty() && *bay < crate::desk_legacy::V32_INSERT_SPACE);
        pairs.sort_by_key(|(n, _)| *n);
        pairs.dedup_by_key(|(n, _)| *n);
        pairs
    }

    /// The desk in the production format. Only tests ask for it bare: the
    /// writer goes through `to_value_as` with the document's own format.
    #[cfg(test)]
    fn to_value(&self) -> Value {
        self.to_value_as(crate::desk_format::Format::current())
    }

    /// **This desk as a document's `desk` object, in `format`.** Format 2
    /// is the one `desk_format` object (the rows by id, the plugins by
    /// identity, `legacy_states` while the document has one) with this
    /// snapshot's scalars beside it under the keys they always had: the
    /// fader levels of the buses, the tempo, the rack, the view. Those were
    /// never strip space, so they have nothing to move to.
    pub(crate) fn to_value_as(&self, format: crate::desk_format::Format) -> Value {
        let legacy = format == crate::desk_format::Format::Legacy;
        let Value::Object(keys) = self.write_keys(legacy) else {
            return Value::Null;
        };
        if legacy {
            return Value::Object(keys);
        }
        let Value::Object(mut desk) = crate::desk_format::write(&self.desk_model().model) else {
            return Value::Null;
        };
        for (k, v) in keys {
            desk.insert(k, v);
        }
        Value::Object(desk)
    }

    /// **This snapshot as the format-2 model**: its rows as they are (a
    /// late row fact it does not say holds the vacant row's value, which no
    /// live snapshot reaches: the live desk says every one), its bays, its
    /// wiring (none where it does not say), the marker it carries.
    pub fn desk_model(&self) -> crate::desk_format::DeskRead {
        // **A row fact the snapshot does not SAY is written as the vacant
        // row's** (`row_says`): the format-2 desk says every row fact, and
        // what a document that never said one stands for is nothing, the
        // live desk's value being the apply's to keep.
        let vacant = crate::desk_model::TrackRow::vacant(crate::desk_id::TrackId(0));
        let rows = self
            .rows
            .iter()
            .map(|r| {
                let mut r = r.clone();
                if !self.row_says.contains(Says::PHASE) {
                    r.phase = vacant.phase;
                }
                if !self.row_says.contains(Says::TO_THEORY) {
                    r.theory = vacant.theory;
                }
                if !self.row_says.contains(Says::INSTRUMENTS) {
                    (r.dx7, r.keys) = (vacant.dx7.clone(), vacant.keys.clone());
                }
                if !self.row_says.contains(Says::FROZEN) {
                    r.frozen = None;
                }
                r
            })
            .collect();
        let model = crate::desk_model::DeskModel {
            next_id: self
                .next_id
                .0
                .max(self.rows.iter().map(|r| u64::from(r.id.0) + 1).max().unwrap_or(0)),
            rows,
            specials: self.specials.clone(),
            plugins: self.plugins.iter().filter(|(_, p)| !p.is_empty()).map(|(b, p)| (*b, p.clone())).collect(),
            bypass: self.bypass.clone().unwrap_or_default(),
            routes: self.routes.as_ref().map(crate::desk_model::route_edges).unwrap_or_default(),
            keys: self.insert_keys.as_ref().map(crate::desk_model::side_keys).unwrap_or_default(),
            legacy_states: self.legacy_states.0,
        };
        let mut read = crate::desk_format::DeskRead { model, dropped: Vec::new() };
        read.dropped = read.model.prune();
        read
    }

    /// **The document's desk keys, in the order every build wrote them**:
    /// the dense legacy desk among them when `legacy` (the rows' arrays and
    /// masks in strip space, at the legacy width, for the tests that build
    /// a legacy document: no production road writes one since format 2,
    /// `desk_format::WRITE_V8`), and the scalars alone otherwise, which a
    /// format-2 desk carries beside its one desk object under the keys they
    /// always had.
    fn write_keys(&self, legacy: bool) -> Value {
        let mut m = Map::new();
        let nums = |v: &[f64]| Value::Array(v.iter().map(|x| serde_json::json!(x)).collect());
        // The legacy width of rows, and their ids, for the dense keys.
        let rows = &self.rows[..self.rows.len().min(crate::desk_lift::V32_CHANNELS)];
        let ids = self.row_ids();
        let mut tail = Map::new();
        if legacy {
            m.insert(
                "kinds".into(),
                Value::Array(rows.iter().map(|r| Value::from(crate::desk_model::RowKind::byte(r.kind))).collect()),
            );
            // Always written: an all-zero row is an OPINION (every badge
            // auto), and omitting it let the previous project's badges bleed
            // into a document that carried none.
            m.insert("icons".into(), Value::Array(rows.iter().map(|r| Value::from(r.icon)).collect()));
            // **The desk in strip space**, the shape these keys always had:
            // the rows' arrays with the specials joined at their frozen
            // strips. Polarity only when said.
            let crate::settings::StripSpace { names, colors, sends, muted, soloed, .. } = self.strip_space();
            m.insert("names".into(), Value::Array(names.iter().map(|n| Value::String(n.clone())).collect()));
            m.insert("colors".into(), Value::Array(colors.iter().map(|c| Value::from(*c)).collect()));
            // The head whole, the far seats sparse — see `V7_SPARSE_SEATS`.
            let mut head = vec![Value::String(String::new()); HEAD_BAYS];
            for (bay, path) in self.legacy_bays() {
                match head.get_mut(bay) {
                    Some(slot) => *slot = Value::String(path),
                    None => {
                        tail.insert(bay.to_string(), Value::String(path));
                    }
                }
            }
            m.insert("inserts".into(), Value::Array(head));
            m.insert("sends".into(), nums(&sends));
            m.insert("muted".into(), Value::Number(muted.into()));
            m.insert("soloed".into(), Value::Number(soloed.into()));
            m.insert("gains".into(), nums(&rows.iter().map(|r| r.gain).collect::<Vec<_>>()));
            m.insert("pans".into(), nums(&rows.iter().map(|r| r.pan).collect::<Vec<_>>()));
            m.insert("inputs".into(), Value::Array(rows.iter().map(|r| Value::String(r.input.clone())).collect()));
        }
        m.insert("drums_gain".into(), serde_json::json!(self.drums_gain));
        m.insert("drums_pan".into(), serde_json::json!(self.drums_pan));
        m.insert("metronome_gain".into(), serde_json::json!(self.metronome_gain));
        m.insert("master_gain".into(), serde_json::json!(self.master_gain));
        m.insert("fx_return".into(), serde_json::json!(self.fx_return));
        m.insert("tempo_bpm".into(), serde_json::json!(self.tempo_bpm));
        m.insert("sig_beats".into(), Value::Number(u64::from(self.sig_beats).into()));
        m.insert("sig_unit".into(), Value::Number(u64::from(self.sig_unit).into()));
        m.insert("metronome_on".into(), Value::Bool(self.metronome_on));
        if legacy {
            let mask = |f: fn(&crate::desk_model::TrackRow) -> bool| {
                rows.iter().enumerate().filter(|(_, r)| f(r)).fold(0u64, |acc, (n, _)| acc | 1 << n)
            };
            // Appended, at the END — the settings file's own rule, and this
            // struct answers to it for the same reason.
            m.insert("tracks_added".into(), Value::Number(mask(|r| r.added).into()));
            // Always written, all-false included: like the icons above, a
            // flat desk is an OPINION, and omitting it would let the previous
            // session's folders bleed into a document that carries none.
            let bools = |f: fn(&crate::desk_model::TrackRow) -> bool| {
                Value::Array(rows.iter().map(|r| Value::Bool(f(r))).collect())
            };
            m.insert("is_folder".into(), bools(|r| r.folder));
            m.insert("in_folder".into(), bools(|r| r.in_folder));
            m.insert("folder_shut".into(), bools(|r| r.shut));
            // **The MIDI device sets, at the END and sparse** — the settings
            // file's own rule and its own shape.
            let mut sels: Vec<crate::recorder::MidiInputSel> = rows.iter().map(|r| r.midi.clone()).collect();
            crate::recorder::trim_midi_inputs(&mut sels);
            if let Some(v) = crate::recorder::write_midi_inputs(&sels) {
                m.insert("midi_inputs".into(), v);
            }
            // **The late fields, at the END and only when they are SAID.**
            if self.row_says.contains(Says::PHASE) {
                m.insert("phase".into(), Value::Number(self.strip_space().phase.into()));
            }
            if self.row_says.contains(Says::TO_THEORY) {
                m.insert("to_theory".into(), bools(|r| r.theory));
            }
            if let Some(v) = self.routes.as_ref() {
                m.insert("routes".into(), v.to_lanes(&ids).to_json());
            }
            if let Some(v) = self.insert_keys.as_ref() {
                m.insert("insert_keys".into(), v.to_lanes(&ids).to_json());
            }
            if let Some(v) = self.bypass.as_ref() {
                let seats = v.iter().filter_map(|sb| crate::desk_legacy::seat_of(&ids, *sb)).collect();
                m.insert("insert_bypass".into(), crate::settings::BypassBays(seats).to_json());
            }
            if self.row_says.contains(Says::INSTRUMENTS) {
                let mut im = Map::new();
                im.insert(
                    "dx7_cartridge".into(),
                    Value::Array(rows.iter().map(|r| Value::String(r.dx7.cart.clone())).collect()),
                );
                let counts = |v: Vec<usize>| Value::Array(v.into_iter().map(|n| Value::from(n as u64)).collect());
                im.insert("dx7_patch".into(), counts(rows.iter().map(|r| r.dx7.patch).collect()));
                im.insert("keys_patch".into(), counts(rows.iter().map(|r| r.keys.patch).collect()));
                im.insert(
                    "keys_params".into(),
                    // At the legacy rack's stride, which is what every reader
                    // of this dense key expects (`desk_model::V32_KEYS_SPAN`).
                    Value::Array(
                        rows.iter()
                            .flat_map(|r| r.keys.params.iter().take(crate::desk_model::V32_KEYS_SPAN).map(|v| serde_json::json!(v)))
                            .collect(),
                    ),
                );
                m.insert("instruments".into(), Value::Object(im));
            }
            // **The ARM mask, at the END and always written**, all-zero
            // included. A desk with nothing armed is an OPINION, and omitting
            // it would send the reader to `armed_by_default`, which would arm
            // the piano on a project somebody deliberately left silent.
            m.insert("armed".into(), Value::Number(mask(|r| r.arm).into()));
            // Appended, at the END, and only when a seat past the head is
            // occupied.
            if !tail.is_empty() {
                m.insert("inserts_tail".into(), Value::Object(tail));
            }
        }
        // **The second late wave, at the END and only when SAID**, appended
        // after `inserts_tail` so that not one key a previous build wrote
        // moves position. A LIVE desk always says, so a document saved by
        // this build carries all of them and wins outright from then on.
        if let Some(r) = self.rack.as_ref() {
            let mut rm = Map::new();
            for (k, v) in [
                ("reverb", r.reverb),
                ("delay", r.delay),
                ("chorus", r.chorus),
                ("phaser", r.phaser),
                ("hpf", r.hpf),
                ("lpf", r.lpf),
                ("gain", r.gain),
                ("limiter", r.limiter),
            ] {
                rm.insert(k.into(), serde_json::json!(v));
            }
            // The parameter rows whole, under their own key, so a row this
            // build has never heard of survives a save by it.
            rm.insert("params".into(), Value::Object(r.params.clone()));
            m.insert("rack".into(), Value::Object(rm));
        }
        if let Some(v) = self.track_gain {
            m.insert("track_gain".into(), serde_json::json!(v));
        }
        if let Some(v) = self.master_mono {
            m.insert("master_mono".into(), Value::Bool(v));
        }
        if let Some(v) = self.lufs_window {
            // By INDEX, and the table is append-only — the settings file's
            // own rule for this enum, and the same one, because the two
            // files are read by the same builds.
            m.insert("lufs_window".into(), Value::Number(v.index().into()));
        }
        if let Some(v) = self.varispeed {
            let mut vm = Map::new();
            vm.insert("pos".into(), serde_json::json!(f64::from(v.pos())));
            vm.insert("min".into(), Value::Number(v.min().into()));
            vm.insert("max".into(), Value::Number(v.max().into()));
            m.insert("varispeed".into(), Value::Object(vm));
        }
        if let Some(c) = self.click.as_ref() {
            let mut cm = Map::new();
            cm.insert("div".into(), Value::Number(c.div.into()));
            cm.insert("in_take".into(), Value::Bool(c.in_take));
            cm.insert("sample".into(), Value::String(c.sample.clone()));
            m.insert("click".into(), Value::Object(cm));
        }
        if let Some(v) = self.transpose {
            m.insert("transpose".into(), Value::Number(v.into()));
        }
        if let Some(c) = self.count_in {
            let mut cm = Map::new();
            cm.insert("bars".into(), Value::Number(c.bars.into()));
            cm.insert("beats".into(), Value::Number(c.beats.into()));
            cm.insert("in_take".into(), Value::Bool(c.in_take));
            m.insert("count_in".into(), Value::Object(cm));
        }
        if let Some(l) = self.look.as_ref() {
            let mut lm = Map::new();
            lm.insert("theme".into(), Value::String(l.theme.clone()));
            lm.insert("palette".into(), Value::Number(l.palette.into()));
            lm.insert("palette_auto".into(), Value::Bool(l.palette_auto));
            lm.insert("view".into(), Value::String(l.view.to_owned()));
            m.insert("look".into(), Value::Object(lm));
        }
        if let Some(v) = self.view.as_ref() {
            let mut vm = Map::new();
            vm.insert("snap".into(), Value::Bool(v.snap));
            vm.insert("grid_exp".into(), Value::Number(v.grid_exp.into()));
            vm.insert("tempo_lane".into(), Value::Bool(v.tempo_lane));
            vm.insert("master_row".into(), Value::Bool(v.master_row));
            vm.insert("header_w".into(), serde_json::json!(v.header_w));
            vm.insert("visible".into(), Value::Number(v.visible.into()));
            vm.insert("q_grid_exp".into(), Value::Number(v.quantize_grid_exp.into()));
            vm.insert("q_triplet".into(), Value::Bool(v.quantize_triplet));
            vm.insert("q_swing_pct".into(), serde_json::json!(v.quantize_swing_pct));
            vm.insert("q_strength_pct".into(), serde_json::json!(v.quantize_strength_pct));
            vm.insert("q_window_ms".into(), serde_json::json!(v.quantize_window_ms));
            vm.insert("visible_at".into(), serde_json::json!(v.visible_at));
            vm.insert("q_sensitivity".into(), serde_json::json!(v.quantize_sensitivity));
            // Auto is the empty word, one spelling, the way `stretch_mode`
            // has it — or the document would not be a fixed point.
            if !v.quantize_fit.is_empty() && v.quantize_fit != "auto" {
                vm.insert("q_fit".into(), Value::String(v.quantize_fit.to_owned()));
            }
            m.insert("view".into(), Value::Object(vm));
        }
        if let Some(v) = self.take_name.as_ref() {
            m.insert("take_name".into(), Value::String(v.clone()));
        }
        if let Some(t) = self.feel {
            let mut fm = Map::new();
            fm.insert("snap".into(), Value::Bool(t.snap));
            fm.insert("grid_exp".into(), Value::Number(t.grid_exp.into()));
            fm.insert("swing".into(), Value::Bool(t.swing));
            fm.insert("swing_pct".into(), serde_json::json!(t.swing_pct));
            fm.insert("triplet".into(), Value::Bool(t.triplet));
            fm.insert("humanize_timing".into(), Value::Number(t.humanize_timing.into()));
            fm.insert("humanize_vel".into(), Value::Number(t.humanize_vel.into()));
            m.insert("feel".into(), Value::Object(fm));
        }
        if let Some(n) = self.notation.as_ref() {
            let mut nm = Map::new();
            nm.insert("staff_key".into(), Value::Number(n.staff_key.into()));
            nm.insert("prefer_flats".into(), Value::Bool(n.prefer_flats));
            nm.insert("capo".into(), Value::Number(n.capo.into()));
            nm.insert("capo_style".into(), Value::String(n.capo_style.clone()));
            nm.insert("tuning".into(), Value::String(n.tuning.clone()));
            m.insert("notation".into(), Value::Object(nm));
        }
        if legacy && self.row_says.contains(Says::FROZEN) {
            let frozen = rows
                .iter()
                .enumerate()
                .filter_map(|(lane, r)| {
                    let z = r.frozen.as_ref()?;
                    let mut zm = Map::new();
                    zm.insert("lane".into(), Value::Number((lane as u64).into()));
                    zm.insert("path".into(), Value::String(z.path.clone()));
                    zm.insert(
                        "len_s".into(),
                        serde_json::Number::from_f64(z.len_s).map_or(Value::Null, Value::Number),
                    );
                    Some(Value::Object(zm))
                })
                .collect();
            m.insert("frozen".into(), Value::Array(frozen));
        }
        // **The window, LAST and only when there is one**: appended after
        // every key an earlier build wrote, so none of theirs moves.
        if let Some(v) = self.viewport.0 {
            let mut vm = Map::new();
            vm.insert("start_s".into(), serde_json::json!(v.start_s));
            vm.insert("len_s".into(), serde_json::json!(v.len_s));
            m.insert("viewport".into(), Value::Object(vm));
        }
        // **The tape's start and stop, after the window and only when said**:
        // appended behind every key an earlier build wrote, the window's own
        // rule, so none of theirs moves.
        if let Some(v) = self.start_stop {
            m.insert("start_stop".into(), v.to_json());
        }
        Value::Object(m)
    }

    /// **This snapshot's strip facts in the legacy STRIP SPACE**: the rows'
    /// arrays and masks (the legacy width of rows) with the specials joined
    /// at their frozen strips (`desk_legacy::write_specials`, Appendix A),
    /// polarity zero where the snapshot does not say it. What the legacy
    /// writer spells; nothing live reads it.
    pub(crate) fn strip_space(&self) -> crate::settings::StripSpace {
        let rows = &self.rows[..self.rows.len().min(crate::desk_lift::V32_CHANNELS)];
        let mask = |f: fn(&crate::desk_model::TrackRow) -> bool| {
            rows.iter().enumerate().filter(|(_, r)| f(r)).fold(0u64, |acc, (n, _)| acc | 1 << n)
        };
        let phase_said = self.row_says.contains(Says::PHASE);
        let strip = |m: u64| crate::desk_legacy::strip_mask_of(m, |_| false);
        let mut names: Vec<String> = rows.iter().map(|r| r.name.clone()).collect();
        while names.last().is_some_and(String::is_empty) {
            names.pop();
        }
        let mut v = crate::settings::StripSpace {
            names,
            colors: rows.iter().map(|r| r.color).collect(),
            sends: rows.iter().map(|r| crate::desk_model::fx_send_level(&r.sends)).collect(),
            muted: strip(mask(|r| r.mute)),
            soloed: strip(mask(|r| r.solo)),
            phase: strip(if phase_said { mask(|r| r.phase) } else { 0 }),
        };
        crate::desk_legacy::write_specials(
            &self.specials,
            &mut v.names,
            &mut v.colors,
            &mut v.sends,
            (&mut v.muted, &mut v.soloed, &mut v.phase),
        );
        if !phase_said {
            v.phase = 0;
        }
        v
    }

    /// **A document's `desk` object, read**: dispatched on its SHAPE (a
    /// format-2 desk says `format: 2`; anything else is the legacy chain),
    /// never on the document's stamp. `Ok(None)` is no desk at all; `Err`
    /// is a format-2 desk refused whole (design 6.3), which refuses the
    /// OPEN, never the bytes: see `Arrangement::read_from`.
    ///
    /// A legacy desk carries its era as `legacy_states`, so the first
    /// format-2 save of it still knows which names its `state/` folder's
    /// plugin files wear.
    pub(crate) fn from_value(v: &Value) -> Result<Option<Self>, crate::desk_format::DeskRefused> {
        if !crate::desk_format::is_format2(v) {
            let era = crate::desk_lift::era_of_desk(v);
            return Ok(Self::from_value_legacy(v).map(|mut d| {
                d.legacy_states = Aside(Some(era));
                d
            }));
        }
        // The scalars by the legacy reader, which reads them under the keys
        // they always had and finds no strip key to lift.
        let Some(mut d) = Self::from_value_legacy(v) else {
            return Ok(None);
        };
        let read = crate::desk_format::read(v)?;
        // **Every row the document holds, as long as the desk is**: nothing
        // padded to a width and nothing past one refused or hidden. A
        // format-2 desk says every late fact.
        let crate::desk_model::DeskModel { next_id, rows, specials, plugins, bypass, routes, keys, legacy_states } =
            read.model;
        d.rows = rows;
        d.row_says = Says::ROWS;
        d.specials = specials;
        d.plugins = plugins;
        d.bypass = Some(bypass);
        d.routes = Some(crate::desk_model::routes_of(&routes));
        d.insert_keys = Some(crate::desk_model::key_edges_of(&keys));
        d.next_id = Aside(next_id);
        d.legacy_states = Aside(legacy_states);
        d.dropped = Aside(read.dropped);
        Ok(Some(d))
    }

    /// **A legacy document's desk, onto rows**: the dense arrays and masks
    /// as the file holds them, lifted by its era to the frozen V32 shape
    /// ([`lift_eras`]), gathered into the dense desk (`V22Desk`) and read
    /// onto rows ONCE by `DeskModel::from_v22` under the identity rule
    /// (legacy row r is `TrackId(r)`). A row past the end of a short array
    /// is the vacant row's, which is what reading it as nothing always gave
    /// the document; a late row fact the file does not carry leaves its bit
    /// out of [`Self::row_says`], and the live desk stands for it.
    fn from_value_legacy(v: &Value) -> Option<Self> {
        let m = v.as_object()?;
        let f = |k: &str, d: f64| m.get(k).and_then(Value::as_f64).unwrap_or(d);
        let strings = |k: &str| -> Vec<String> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|x| x.as_str().unwrap_or_default().to_owned()).collect())
                .unwrap_or_default()
        };
        let floats = |k: &str| -> Vec<f64> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default()
        };
        // Absent in every document before the folder fields — and absent
        // means FLAT, the vacant row's value.
        let bools = |k: &str| -> Vec<bool> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().map(|x| x.as_bool().unwrap_or(false)).collect())
                .unwrap_or_default()
        };
        let kinds_v: Vec<u8> = m
            .get("kinds")
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect())
            .unwrap_or_default();
        let inputs_v = strings("inputs");
        // The seats, by the SHAPE the file holds them in — see
        // `V7_SPARSE_SEATS` for why never by the version stamp.
        let pairs_of = |o: &Map<String, Value>| -> Vec<(usize, String)> {
            o.iter()
                .filter_map(|(k, v)| Some((k.parse::<usize>().ok()?, v.as_str()?.to_owned())))
                .collect()
        };
        // The era from the file's own lengths, before anything moves them.
        let era = crate::desk_lift::era_of_desk(v);
        let mut phase_v = m.get("phase").and_then(Value::as_u64);
        let (mut names_v, mut colors_v, mut sends_v, mut muted_v, mut soloed_v) = (
            strings("names"),
            m.get("colors")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect::<Vec<i64>>())
                .unwrap_or_default(),
            floats("sends"),
            m.get("muted").and_then(Value::as_u64).unwrap_or(0),
            m.get("soloed").and_then(Value::as_u64).unwrap_or(0),
        );
        let mut pairs = match m.get("inserts") {
            // A dense array of ANY length: every document before v7 and
            // the head of every one since. The era lifts run here, on the
            // dense vec, keyed on the lengths the file was written at.
            Some(Value::Array(_)) => {
                let mut dense = strings("inserts");
                lift_eras(
                    era,
                    &mut sends_v,
                    &mut colors_v,
                    &mut names_v,
                    &mut dense,
                    &mut muted_v,
                    &mut soloed_v,
                    &mut phase_v,
                );
                dense.into_iter().enumerate().filter(|(_, p)| !p.is_empty()).collect()
            }
            // The fully sparse shape: today's desk, nothing to lift.
            Some(Value::Object(o)) => pairs_of(o),
            _ => Vec::new(),
        };
        // The far seats. A seat the array already fills keeps the array's
        // word — `canonical` keeps the first claim, and the head's come
        // first.
        if let Some(Value::Object(tail)) = m.get("inserts_tail") {
            pairs.extend(pairs_of(tail));
        }
        let inserts = Self::canonical(pairs);
        let bay_at = |n: usize| inserts.binary_search_by_key(&n, |(b, _)| *b).is_ok();
        // The rows a legacy desk's seats and lanes are bound to: the
        // identity rule, legacy row r is `TrackId(r)`.
        let legacy_ids = crate::desk_legacy::identity_ids(crate::desk_lift::V32_CHANNELS);
        // Where a row's own strip and its first bays sat in the legacy maps,
        // for the reconstructions below.
        let strip_of = |n: usize| crate::desk_legacy::v32_strip(crate::recorder::StripRef::Row(n));
        let bay_of = |n: usize, b: usize| {
            let owner = crate::desk_legacy::LegacyOwner::Row(u16::try_from(n).ok()?);
            crate::desk_legacy::encode_v32(owner, crate::desk_id::Rack::Chain(0), u8::try_from(b).ok()?)
        };
        let width = crate::desk_lift::V32_CHANNELS;
        // **Which rows EXIST**: absent in older documents, and reconstructed
        // rather than assumed empty. A v4 desk said which rows it described
        // by describing them: a channel with a kind, a name, a colour, an
        // input or a plugin in a bay is a channel that was there. Defaulting
        // to zero would open every old project with no tracks on it. Read
        // AFTER the era lifts, so a channel row is a channel row — before,
        // an eight-channel-era desk's drum strip sat where channel 8 does
        // today and could pass for one.
        let tracks_added = m.get("tracks_added").and_then(Value::as_u64).map_or_else(
            || {
                let mut bits = 0u32;
                for n in 0..width {
                    let kind = kinds_v.get(n).copied().unwrap_or(0) != 0;
                    let at = strip_of(n);
                    let named = at.and_then(|at| names_v.get(at)).is_some_and(|s| !s.is_empty());
                    let coloured = at.and_then(|at| colors_v.get(at)).copied().unwrap_or(0) != 0;
                    let input = inputs_v.get(n).is_some_and(|s| !s.is_empty());
                    let bay = (0..crate::recorder::INSERTS).any(|b| bay_of(n, b).is_some_and(bay_at));
                    if kind || named || coloured || input || bay {
                        bits |= 1 << n;
                    }
                }
                bits
            },
            |v| v as u32,
        );
        // **Which rows are ARMED**: absent in older documents, and DERIVED
        // rather than assumed empty — the shipped rule, asked of this
        // document's own desk, so a project from before the arm was written
        // down opens the way it always opened (the piano's channel live,
        // nothing else; in those documents the piano is the DX7). Read
        // AFTER the era lifts, like `tracks_added`.
        let armed = m.get("armed").and_then(Value::as_u64).map_or_else(
            || {
                armed_by_default(&kinds_v, |n| {
                    // `bay_at` only answers WHETHER a seat is occupied; the
                    // rule asks WHAT is in it. Seat 0 of the channel's own
                    // rack, which is where an instrument sits.
                    let Some(seat) = bay_of(n, 0) else {
                        return false;
                    };
                    inserts
                        .binary_search_by_key(&seat, |(b, _)| *b)
                        .ok()
                        .and_then(|i| inserts.get(i))
                        .is_some_and(|(_, p)| crate::dialogs::SHIPPED_PIANOS.contains(&p.as_str()))
                })
                .map_or(0, |n| 1u32 << n)
            },
            |v| v as u32,
        );
        // The dense desk, the frozen V32 shape, as the file says it: each
        // array over the vacant desk's from the front, a short one leaving
        // the vacant values past its end and a long one cut at the width.
        fn fill<T>(dst: &mut [T], src: Vec<T>) {
            for (d, s) in dst.iter_mut().zip(src) {
                *d = s;
            }
        }
        let mut dense = crate::desk_model::V22Desk::vacant();
        fill(&mut dense.kinds, kinds_v.clone());
        // **The badges**: a document from before them DERIVES them, the drum
        // where the drumpad stands and auto everywhere else, instead of
        // leaving the last project's badges on this one's tracks.
        match m.get("icons").and_then(Value::as_array) {
            Some(a) => fill(
                &mut dense.icons,
                a.iter()
                    .filter_map(|x| x.as_u64().map(|n| n.min(u64::from(crate::recorder_panel::ICON_MAX)) as u8))
                    .collect(),
            ),
            None => {
                for (n, icon) in dense.icons.iter_mut().enumerate() {
                    let drum = bay_of(n, 0).and_then(|seat| {
                        inserts.binary_search_by_key(&seat, |(b, _)| *b).ok().and_then(|i| inserts.get(i))
                    });
                    *icon = u8::from(drum.is_some_and(|(_, p)| p == crate::dialogs::DRUMPAD_PATH));
                }
            }
        }
        fill(&mut dense.gains, floats("gains"));
        fill(&mut dense.pans, floats("pans"));
        fill(&mut dense.inputs, inputs_v.clone());
        // **The device sets: absent in every older document, and derived
        // from `inputs` rather than assumed empty** — the `tracks_added`
        // rule, for the same reason: a document that named a port per track
        // still says which keyboards those tracks wanted.
        let mut sels = crate::recorder::read_midi_inputs(m.get("midi_inputs"));
        crate::recorder::reconcile_midi_inputs(&mut sels, &inputs_v);
        fill(&mut dense.midi_inputs, sels);
        dense.tracks_added = tracks_added;
        dense.armed = armed;
        fill(&mut dense.is_folder, bools("is_folder"));
        fill(&mut dense.in_folder, bools("in_folder"));
        fill(&mut dense.folder_shut, bools("folder_shut"));
        // The strip arrays, AFTER the era lifts, in strip space: the dense
        // desk splits them onto the rows and the specials (`from_v22`).
        fill(&mut dense.names, names_v);
        fill(&mut dense.colors, colors_v);
        fill(&mut dense.sends, sends_v);
        dense.muted = muted_v;
        dense.soloed = soloed_v;
        // **The late row facts: absent means the document does not SAY**,
        // and its bit stays out of `row_says`. Not "empty", and not a
        // default: the live value stands for a document written before the
        // key, so nothing an old project sounds like changes because a field
        // was added.
        let mut row_says = Says::default();
        if let Some(p) = phase_v {
            dense.phase = p;
            row_says = row_says.with(Says::PHASE);
        }
        if let Some(a) = m.get("to_theory").and_then(Value::as_array) {
            fill(&mut dense.to_theory, a.iter().map(|v| v.as_bool().unwrap_or(false)).collect());
            row_says = row_says.with(Says::TO_THEORY);
        }
        if let Some(im) = m.get("instruments").and_then(Value::as_object) {
            let strings = |k: &str| -> Vec<String> {
                im.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|v| v.as_str().unwrap_or_default().to_owned()).collect())
                    .unwrap_or_default()
            };
            let counts = |k: &str| -> Vec<usize> {
                im.get(k)
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|v| v.as_u64().unwrap_or(0) as usize).collect())
                    .unwrap_or_default()
            };
            // Length-clamped like every other vector read out of a document:
            // the file is content and may say anything. A short list reads as
            // the shipped values past its end.
            fill(&mut dense.dx7_cartridge, strings("dx7_cartridge"));
            fill(&mut dense.dx7_patch, counts("dx7_patch"));
            fill(&mut dense.keys_patch, counts("keys_patch"));
            fill(
                &mut dense.keys_params,
                im.get("keys_params")
                    .and_then(Value::as_array)
                    .map(|a| a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect())
                    .unwrap_or_default(),
            );
            row_says = row_says.with(Says::INSTRUMENTS);
        }
        // **A freeze is a fact about THIS document's media folder**, so a
        // document that carries no `frozen` has nothing frozen: the one late
        // row fact whose absence is no fact of the live desk's. Its bit says
        // only whether the file carried the key.
        if let Some(rows) = m.get("frozen").and_then(Value::as_array) {
            for r in rows {
                let Some(zm) = r.as_object() else { continue };
                let (Some(lane), Some(path)) = (
                    zm.get("lane").and_then(Value::as_u64).and_then(|n| usize::try_from(n).ok()),
                    zm.get("path").and_then(Value::as_str),
                ) else {
                    continue;
                };
                let len_s = zm.get("len_s").and_then(Value::as_f64).unwrap_or(0.0);
                if path.is_empty() || !len_s.is_finite() {
                    continue;
                }
                // The first entry for a lane wins, and a lane past the
                // legacy width is no row of a legacy desk.
                if let Some(slot @ None) = dense.frozen.get_mut(lane) {
                    *slot = Some(crate::desk_model::Frozen { path: path.to_owned(), len_s: len_s.max(0.0) });
                }
            }
            row_says = row_says.with(Says::FROZEN);
        }
        dense.inserts = inserts.iter().cloned().collect();
        let crate::desk_format::DeskRead { model, dropped: _ } = crate::desk_model::DeskModel::from_v22(&dense, &legacy_ids);
        Some(Self {
            rows: model.rows,
            row_says,
            plugins: model.plugins,
            drums_gain: f("drums_gain", 1.0),
            drums_pan: f("drums_pan", 0.0),
            metronome_gain: f("metronome_gain", 0.7),
            master_gain: f("master_gain", 1.0),
            fx_return: f("fx_return", 1.0),
            tempo_bpm: f("tempo_bpm", 120.0),
            sig_beats: m.get("sig_beats").and_then(Value::as_u64).unwrap_or(4) as u8,
            sig_unit: m.get("sig_unit").and_then(Value::as_u64).unwrap_or(4) as u8,
            metronome_on: m.get("metronome_on").and_then(Value::as_bool).unwrap_or(false),
            // By lane, strip and seat in the file; by identity from here,
            // under the identity rule every legacy document obeys.
            routes: m.get("routes").map(|v| {
                crate::routing::Routes::from_lanes(&crate::routing::LaneRoutes::from_json(v), &legacy_ids)
            }),
            insert_keys: m.get("insert_keys").map(|v| {
                crate::routing::KeyEdges::from_lanes(&crate::routing::LaneKeys::from_json(v), &legacy_ids)
            }),
            bypass: m.get("insert_bypass").map(|v| {
                crate::settings::BypassBays::from_json(v)
                    .0
                    .iter()
                    .filter_map(|&(seat, bay)| crate::desk_legacy::bay_at(&legacy_ids, seat, bay))
                    .collect()
            }),
            // **The second late wave: absent is `None`**, on the first
            // wave's rule — the live value stands for a document written
            // before the key, so no project changes what it sounds like
            // because a field was added.
            rack: m.get("rack").and_then(Value::as_object).map(|rm| {
                let g = |k: &str| rm.get(k).and_then(Value::as_f64).unwrap_or(0.0);
                Rack {
                    reverb: g("reverb"),
                    delay: g("delay"),
                    chorus: g("chorus"),
                    phaser: g("phaser"),
                    hpf: g("hpf"),
                    lpf: g("lpf"),
                    gain: g("gain"),
                    // **The one knob whose rest is the TOP of its travel.**
                    // A threshold missing from a hand-written rack has to
                    // read as bypass; zero would be a limiter slammed shut
                    // over a mix, which is the loudest possible way to be
                    // wrong about a default.
                    limiter: rm.get("limiter").and_then(Value::as_f64).unwrap_or(1.0),
                    params: rm
                        .get("params")
                        .and_then(Value::as_object)
                        .cloned()
                        .unwrap_or_default(),
                }
            }),
            track_gain: m.get("track_gain").and_then(Value::as_f64),
            master_mono: m.get("master_mono").and_then(Value::as_bool),
            lufs_window: m
                .get("lufs_window")
                .and_then(Value::as_u64)
                .map(|n| crate::recorder::LoudnessWindow::from_index(n as usize)),
            varispeed: m.get("varispeed").and_then(Value::as_object).map(|vm| {
                let num = |k: &str, d: u16| {
                    vm.get(k)
                        .and_then(Value::as_u64)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(d)
                };
                // **The range first, then the position.** `set_pos` snaps
                // against the range it is handed, so a position read against
                // the default 85..115 and then given a range of 50..200 is a
                // different speed from the one that was saved. The settings
                // reader says exactly this at its own door.
                let mut v = crate::varispeed::Varispeed::with_range(
                    num("min", crate::varispeed::DEFAULT_MIN),
                    num("max", crate::varispeed::DEFAULT_MAX),
                );
                if let Some(pos) = vm.get("pos").and_then(Value::as_f64) {
                    // Through the setter, for the reason the settings give:
                    // a NaN or a 12.0 in a file is a dial that resamples the
                    // master by a nonsense ratio, and the place to stop it is
                    // where it enters.
                    v.set_pos(pos as f32);
                }
                v
            }),
            // **The tape's start and stop**, through the model's setters
            // member by member (`StartStop::from_json`). Absent, or not an
            // object, is a document that does not say, which leaves the live
            // switch standing, as the varispeed's block above is read.
            start_stop: m
                .get("start_stop")
                .filter(|v| v.is_object())
                .map(crate::motor::StartStop::from_json),
            click: m.get("click").and_then(Value::as_object).map(|cm| Click {
                div: cm.get("div").and_then(Value::as_i64).unwrap_or(1),
                in_take: cm.get("in_take").and_then(Value::as_bool).unwrap_or(false),
                sample: cm
                    .get("sample")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
            transpose: m.get("transpose").and_then(Value::as_i64),
            count_in: m.get("count_in").and_then(Value::as_object).map(|cm| {
                let n = |k: &str, d: i64| cm.get(k).and_then(Value::as_i64).unwrap_or(d);
                CountIn {
                    bars: n("bars", 1),
                    beats: n("beats", 4),
                    in_take: cm
                        .get("in_take")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                }
            }),
            // **A key the block does not carry falls back to the SHIPPED
            // default, not to the live value**, and `apply_desk_snapshot`
            // writes it like any other field (the one exception: a zero
            // `header_w` or `visible`, which it skips). That is safe only
            // because no build has written a partial block - all fourteen
            // keys arrived together, and the writer emits every one (`q_fit`
            // only when it is not auto, and absent IS auto) - so the defaults
            // are reached by a hand-edited file alone. An ABSENT block is what
            // leaves the live values standing. Zero for `header_w` and
            // `visible` is AUTO, the shipped default, not "unset".
            view: m.get("view").and_then(Value::as_object).map(|vm| View {
                snap: vm.get("snap").and_then(Value::as_bool).unwrap_or(true),
                // -6, the timeline's finest line (`grid_quarters`), as the
                // settings reader clamps it: -7 is the editor's floor, not
                // the arrangement's.
                grid_exp: vm.get("grid_exp").and_then(Value::as_i64).unwrap_or(-2).clamp(-6, 3),
                tempo_lane: vm.get("tempo_lane").and_then(Value::as_bool).unwrap_or(false),
                master_row: vm.get("master_row").and_then(Value::as_bool).unwrap_or(true),
                header_w: vm
                    .get("header_w")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0),
                visible: vm
                    .get("visible")
                    .and_then(Value::as_u64)
                    .and_then(|n| u8::try_from(n).ok())
                    .unwrap_or(0),
                quantize_grid_exp: vm
                    .get("q_grid_exp")
                    .and_then(Value::as_i64)
                    .unwrap_or(-3)
                    .clamp(-7, 3),
                quantize_triplet: vm.get("q_triplet").and_then(Value::as_bool).unwrap_or(false),
                quantize_swing_pct: vm
                    .get("q_swing_pct")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(50.0)
                    .clamp(50.0, 95.0),
                quantize_strength_pct: vm
                    .get("q_strength_pct")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(100.0)
                    .clamp(0.0, 100.0),
                quantize_window_ms: vm
                    .get("q_window_ms")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.0)
                    .clamp(0.0, 500.0),
                visible_at: vm
                    .get("visible_at")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite() && *v >= 0.0)
                    .unwrap_or(0.0),
                quantize_sensitivity: vm
                    .get("q_sensitivity")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .unwrap_or(0.5)
                    .clamp(0.0, 1.0),
                quantize_fit: vm
                    .get("q_fit")
                    .and_then(Value::as_str)
                    .filter(|w| *w != "auto")
                    .unwrap_or_default()
                    .to_owned(),
            }),
            look: m.get("look").and_then(Value::as_object).map(|lm| Look {
                theme: lm
                    .get("theme")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                palette: lm.get("palette").and_then(Value::as_i64).unwrap_or(0),
                // **True when the key is missing**, because that is what a
                // theme's own palette does by default: a document written by
                // a build that did not know about the hand pick should open
                // following its theme rather than pinned to palette zero.
                palette_auto: lm
                    .get("palette_auto")
                    .and_then(Value::as_bool)
                    .unwrap_or(true),
                // Missing is the empty string, which `App` reads as the
                // piano - the view a build that did not know about this
                // always opened on.
                view: lm
                    .get("view")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
            take_name: m.get("take_name").and_then(Value::as_str).map(str::to_owned),
            feel: m.get("feel").and_then(Value::as_object).map(|fm| {
                let b = |k: &str, d: bool| fm.get(k).and_then(Value::as_bool).unwrap_or(d);
                let n = |k: &str, d: u64| fm.get(k).and_then(Value::as_u64).unwrap_or(d);
                Feel {
                    snap: b("snap", true),
                    grid_exp: fm.get("grid_exp").and_then(Value::as_i64).unwrap_or(-3),
                    swing: b("swing", false),
                    swing_pct: fm
                        .get("swing_pct")
                        .and_then(Value::as_f64)
                        .unwrap_or(66.7),
                    triplet: b("triplet", false),
                    humanize_timing: n("humanize_timing", 25).min(255) as u8,
                    humanize_vel: n("humanize_vel", 15).min(255) as u8,
                }
            }),
            notation: m.get("notation").and_then(Value::as_object).map(|nm| {
                let s = |k: &str, d: &str| {
                    nm.get(k).and_then(Value::as_str).unwrap_or(d).to_owned()
                };
                Notation {
                    staff_key: nm
                        .get("staff_key")
                        .and_then(Value::as_i64)
                        .unwrap_or(0)
                        .clamp(i64::from(i32::MIN), i64::from(i32::MAX))
                        as i32,
                    prefer_flats: nm
                        .get("prefer_flats")
                        .and_then(Value::as_bool)
                        .unwrap_or(true),
                    capo: nm.get("capo").and_then(Value::as_i64).unwrap_or(0),
                    // Verbatim, both of them — see [`Notation`].
                    capo_style: s("capo_style", ""),
                    tuning: s("tuning", ""),
                }
            }),
            // A legacy desk does not say its ids: the identity rule speaks
            // for it, and the counter stands above them. `from_value` sets
            // the rest.
            next_id: Aside(width as u64),
            legacy_states: Aside(None),
            dropped: Aside(Vec::new()),
            // **Where the arrangement was left looking.** Absent in every
            // document before it, and absent or unreadable is no window: the
            // open shows the default (see `DeskSnap::viewport`).
            viewport: Aside(m.get("viewport").and_then(Value::as_object).and_then(|vm| {
                let f = |k: &str| vm.get(k).and_then(Value::as_f64).unwrap_or(f64::NAN);
                Viewport::sane(f("start_s"), f("len_s"))
            })),
            specials: model.specials,
        })
    }
}

/// **A desk written in an earlier ERA of the desk's shape, lifted onto the
/// frozen V32 shape.** The era comes off the LENGTHS the file was written at
/// (the sends count is a strip count, the bay count a seat map; see
/// [`crate::desk_lift::era_of_desk`]) and every step is `desk_lift`'s, at
/// its literals: this read the live constants once and went dead the day
/// the desk grew to 24, stranding that beta window's documents unlifted,
/// with their buses and master on channels 24..26.
///
/// Here in the READER, on the dense array as the file holds it, and not in
/// `apply_desk_snapshot` where they used to run on a clone of every desk
/// applied — every open and every undo step — although a moment can never
/// hold an old-era desk. A property of how the FILE was written belongs
/// beside the `tracks_added` reconstruction, which is the same kind of
/// fact. `a_desk_from_any_era_opens_at_todays_seats` pins each era through
/// the live desk, and the era corpus pins every one at literal seats.
///
/// A project's insert states live in its own `state/`, named by the era's
/// seats; the desk ARRAYS relocate here so plugins load at the right seats,
/// and [`crate::desk_lift::provenance_of_document`] runs this same lift
/// over bay numbers to say which legacy file holds each one's knobs.
#[allow(clippy::too_many_arguments)]
fn lift_eras(
    era: crate::desk_lift::DeskEra,
    sends: &mut Vec<f64>,
    colors: &mut Vec<i64>,
    names: &mut Vec<String>,
    inserts: &mut Vec<String>,
    muted: &mut u64,
    soloed: &mut u64,
    phase: &mut Option<u64>,
) {
    use crate::desk_lift::{lift_bays, lift_strip_mask, lift_strip_rows};
    *sends = lift_strip_rows(era, std::mem::take(sends), 0.0, false, false);
    *colors = lift_strip_rows(era, std::mem::take(colors), 0, true, false);
    *names = lift_strip_rows(era, std::mem::take(names), String::new(), true, false);
    *inserts = lift_bays(era, std::mem::take(inserts), false);
    *muted = lift_strip_mask(era, *muted, false);
    *soloed = lift_strip_mask(era, *soloed, false);
    // Polarity reached the document only after the desk was 32 wide, so no
    // older-era document says it; one that did would mean strip space.
    if let Some(p) = phase.as_mut() {
        *p = lift_strip_mask(era, *p, false);
    }
}

/// **A named place in the song.**
///
/// A marker is a fact about the SONG, so it lives in the document beside the
/// tempo and meter maps rather than in settings — which also puts it inside
/// the undo funnel for nothing, since `undo_state` serialises the whole
/// arrangement. The owner's rule for where it may be stored is the same one
/// that shape gives for free: "only be stored as values in a project where
/// they are added. Load new? Reset all? Markers gone. Save track, then new
/// track, then load saved, the markers should stay from the first save."
///
/// In SECONDS and not in bars, unlike a meter point. A meter change is a
/// property of the grid and can only happen on a bar line; a marker names a
/// moment — a downbeat, an entry, the place a take goes wrong — and pinning
/// it to the grid would move it whenever the tempo map did.
#[derive(Debug, Clone, PartialEq)]
pub struct ArrMarker {
    pub at_s: f64,
    pub name: String,
}

/// The whole document.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrangement {
    pub clips: Vec<ArrClip>,
    /// The desk this session was saved with. `None` in documents older
    /// than v4 — opening one keeps whatever desk is live.
    pub desk: Option<DeskSnap>,
    /// The next clip id to hand out. Monotonic within the document, never
    /// reused — what lets an edit name the clip it means.
    pub next_id: u64,
    /// Whether somebody SAVED this project on purpose — the Project
    /// window's Save, or a Save As. An auto-saved scratch stays `false`,
    /// which is what lets a new session reclaim an Untitled nobody kept.
    pub named: bool,
    /// Keys this build does not know, kept so a newer build's document
    /// survives being opened here — the settings file's own bargain.
    pub extra: Map<String, Value>,
    /// Every automated control's curve. See [`crate::automation`].
    ///
    /// In the document beside the tempo map, and for the same two reasons: a
    /// curve is a fact about a SONG rather than a preference, and putting it
    /// here puts it inside the undo funnel for nothing — `undo_state` is the
    /// whole `Arrangement` serialised, so drawing a point becomes undoable
    /// without a line of new code.
    pub automation: crate::automation::Automation,
    /// Tempo over the whole song, in quarter notes. See
    /// [`crate::tempo::TempoMap`].
    ///
    /// **In the DOCUMENT and not in the settings**, which is a change of
    /// home as much as of shape. The single `tempo_bpm` lived on
    /// `ExportSpec` in a preferences file and was mirrored onto `DeskSnap`;
    /// a tempo is a fact about a SONG, and putting it here also puts it
    /// inside the undo funnel for free — `undo_state` is the whole
    /// `Arrangement` serialised, so a tempo edit becomes undoable without a
    /// line of new code.
    pub tempo: crate::tempo::TempoMap,
    /// Time signatures over the whole song, by bar.
    pub meter: crate::tempo::MeterMap,
    /// Named places in the song, in no particular order — see [`ArrMarker`].
    /// Appended to this struct, and its key is absent from a document with
    /// none, so an older build reads a newer document unharmed and a project
    /// nobody marked writes nothing.
    pub markers: Vec<ArrMarker>,
    /// The schema version this document is IN, which is never lower than
    /// [`ARRANGEMENT_VERSION`].
    ///
    /// Normalised on the way in, because reading a v1 document with this
    /// build's reader **is** the v1→v4 migration — "the migration is the
    /// defaults", as the header says. So the value here is what the document
    /// became, not what the file happened to claim, and a load followed by a
    /// save is idempotent.
    ///
    /// **Read and then thrown away, until now.** `from_map` bound it into
    /// `let _version` and `to_json` wrote the constant back, which meant
    /// opening a version-9 song in this build and touching anything wrote
    /// out a file that claimed to be version 4 while still carrying every
    /// version-9 key — because `extra` faithfully preserved them. The build
    /// that made that song would open it next, believe the stamp, and run
    /// its 4→9 migrations a second time over data that had already been
    /// migrated.
    ///
    /// The extras bargain is "we do not understand your keys, so we will not
    /// touch them". Rewriting the version stamp is touching them, in the one
    /// way guaranteed to be misread. So a higher number RIDES ALONG, and the
    /// only direction this value ever moves is up.
    pub version: u64,
}

/// What reading a document actually found. The variants a caller may not
/// conflate: `Damaged` bytes are somebody's song and must never be adopted
/// as a path to write to.
pub enum LoadOutcome {
    Loaded(Arrangement),
    Missing,
    Empty,
    Damaged,
    /// **The document parses and its format-2 desk does not** (design
    /// 6.3: too many rows, two rows with one id, not a desk): why, in a
    /// sentence. Unlike `Damaged` the file is not moved aside, because
    /// nothing is wrong with the bytes a newer build could not read; the
    /// open is refused and nothing is written over it.
    Refused(String),
}

impl Default for Arrangement {
    /// A document this build invented is a document at this build's version.
    ///
    /// Hand-written rather than derived for that one field: a derived
    /// `Default` gives `version: 0`, and the empty document that a corrupt
    /// file falls back to would then be saved claiming to predate the stamp.
    fn default() -> Self {
        Self {
            clips: Vec::new(),
            desk: None,
            next_id: 0,
            named: false,
            extra: Map::new(),
            version: ARRANGEMENT_VERSION,
            tempo: crate::tempo::TempoMap::default(),
            meter: crate::tempo::MeterMap::default(),
            markers: Vec::new(),
            automation: crate::automation::Automation::default(),
        }
    }
}

/// The suffix a document's legacy bytes are kept under: see
/// [`Arrangement::pre_v8_path`]. Tangent's own file wherever it is found
/// (`holds_only_scratch` and the Untitled sweeps), and never carried into
/// a template.
pub const PRE_V8_SUFFIX: &str = ".pre-v8";

/// **The document's file name, canonical and legacy.** New sessions write
/// `project.tan` - the owner's call, and no breach of anything: file
/// extensions cannot be owned. Every session ever saved as
/// `project.tangent` keeps opening forever; `doc_in` is the one resolver
/// both facts route through, so no call site can prefer the wrong one.
pub const DOC_NAME: &str = "project.tan";
pub const DOC_NAME_LEGACY: &str = "project.tangent";

/// The project document inside `dir`: the canonical name if that file
/// exists, else the legacy one if THAT exists, else the canonical - which
/// is what a writer minting a fresh document wants too.
pub fn doc_in(dir: &std::path::Path) -> PathBuf {
    let new = dir.join(DOC_NAME);
    if new.is_file() {
        return new;
    }
    let old = dir.join(DOC_NAME_LEGACY);
    if old.is_file() {
        return old;
    }
    new
}

impl Arrangement {
    /// Where the document lives. Overridable for tests in EVERY build, for
    /// the same reason `Settings::path` is: a binary-crate test links an
    /// ordinary `tangent-ui`, and one that ran a frame against the real home
    /// directory would edit the user's own song.
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("TANGENT_ARRANGEMENT_PATH") {
            return PathBuf::from(p);
        }
        Settings_dir().join("arrangement.json")
    }

    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    /// Load a document from anywhere — the PROJECT system's road in.
    ///
    /// Collapses every failure into an empty document, which is the right
    /// manner for a template probe or a `named` peek and the WRONG one for
    /// opening: a caller about to adopt a path must go through `read_from`
    /// and look at the outcome, or a damaged song opens as an empty one,
    /// undo baselines on the emptiness, no prompt ever fires, and the next
    /// writer replaces the damaged file — which was recoverable — with the
    /// emptiness, which is not.
    pub fn load_from(path: &std::path::Path) -> Self {
        // Not through `read_from`: a REFUSED desk is still a document here,
        // read without its desk and carrying the raw desk in `extra`, so a
        // lenient caller that saves cannot write the desk away. Every other
        // failure is the empty document, as it always was.
        match std::fs::read_to_string(path).map(|t| serde_json::from_str::<Value>(&t)) {
            Ok(Ok(Value::Object(map))) => Self::from_map(map),
            _ => Self::default(),
        }
    }

    /// Load a document and SAY WHAT HAPPENED. Missing is ordinary (a fresh
    /// project has no file yet); Empty is a zero-length or whitespace file,
    /// which no writer here produces — half a crashed write, or an external
    /// tool; Damaged is bytes that exist and do not parse. Only `Loaded`
    /// may be silently adopted.
    pub fn read_from(path: &std::path::Path) -> LoadOutcome {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return LoadOutcome::Missing;
            }
            // Unreadable-but-present (permissions, I/O) is indistinguishable
            // from damage in every way that matters to the caller: the bytes
            // must not be written over.
            Err(_) => return LoadOutcome::Damaged,
        };
        if text.trim().is_empty() {
            return LoadOutcome::Empty;
        }
        match serde_json::from_str::<Value>(&text) {
            Ok(Value::Object(map)) => match Self::from_map_report(map) {
                (doc, None) => LoadOutcome::Loaded(doc),
                (_, Some(refused)) => LoadOutcome::Refused(refused.why.to_string()),
            },
            _ => LoadOutcome::Damaged,
        }
    }

    /// Write-then-rename, the settings file's own shape: a crash or a full
    /// disk mid-write must not leave half a song where a whole one was.
    pub fn save(&self) {
        self.save_to(&Self::path());
    }

    /// Save a document anywhere — the PROJECT system's road out.
    pub fn save_to(&self, path: &std::path::Path) {
        self.save_to_as(path, crate::desk_format::Format::current());
    }

    /// `save_to` in `format`. **The first format-2 write over a legacy
    /// document keeps the legacy one first**, byte for byte, as
    /// `<doc>.pre-v8` beside it (design 6.7), once and never again: a
    /// release build handed that file back opens the project whole,
    /// because no new build renames or deletes a legacy sidecar either.
    pub(crate) fn save_to_as(&self, path: &std::path::Path, format: crate::desk_format::Format) {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if format == crate::desk_format::Format::V2 {
            Self::keep_pre_v8(path);
        }
        let tmp = path.with_extension("tangent.tmp");
        if std::fs::write(&tmp, self.to_json_with_desk_as(self.desk.as_ref(), format)).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// **One top-level key of a document file, changed in place** and
    /// nothing else: `None` takes it out. The file is parsed, the key
    /// patched and the rest written back in its own order and this
    /// writer's own layout, so on a document this build (or any build of
    /// this writer) wrote, the bytes change by that key alone. Temp and
    /// rename. `false` when the file is not a document object or did not
    /// land.
    ///
    /// For the heals that must not re-serialise a document, which would
    /// write it in this build's format: `seed_template`'s `named`.
    pub fn patch_doc_key(path: &std::path::Path, key: &str, value: Option<Value>) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(&text) else {
            return false;
        };
        match value {
            Some(v) => {
                map.insert(key.to_owned(), v);
            }
            None => {
                map.shift_remove(key);
            }
        }
        serde_json::to_string_pretty(&Value::Object(map))
            .is_ok_and(|out| crate::settings::write_atomic(path, out.as_bytes()))
    }

    /// Where a document's legacy bytes are kept: `project.tan.pre-v8`
    /// beside `project.tan`, the document's own extension kept so a
    /// `project.tangent` keeps its own.
    pub fn pre_v8_path(doc: &std::path::Path) -> PathBuf {
        let mut name = doc.as_os_str().to_owned();
        name.push(PRE_V8_SUFFIX);
        PathBuf::from(name)
    }

    /// The backup half of `save_to_as`: the bytes on disk, unless they are
    /// already format 2 (their first format-2 write was earlier) or a
    /// backup already stands (the first one is the legacy file).
    fn keep_pre_v8(doc: &std::path::Path) {
        let Ok(bytes) = std::fs::read(doc) else {
            return;
        };
        let format2 = serde_json::from_slice::<Value>(&bytes)
            .ok()
            .and_then(|v| v.get("desk").map(crate::desk_format::is_format2))
            .unwrap_or(false);
        let backup = Self::pre_v8_path(doc);
        if !format2 && !backup.exists() {
            crate::settings::write_atomic(&backup, &bytes);
        }
    }

    /// A clip's path against the document's own directory.
    ///
    /// The one place the relative-path rule lives: `media/<file>` inside a
    /// project resolves beside the document, an absolute path is itself.
    pub fn resolve(doc_path: &std::path::Path, clip_path: &str) -> PathBuf {
        let p = std::path::Path::new(clip_path);
        if p.is_absolute() {
            return p.to_path_buf();
        }
        doc_path.parent().map_or_else(|| p.to_path_buf(), |d| d.join(p))
    }

    /// Add a clip, assigning its id. Returns the id.
    pub fn add_clip(&mut self, mut clip: ArrClip) -> u64 {
        // Ids start at 1: zero is the wire's "the file did not say", and a
        // real clip must never wear the sentinel.
        if self.next_id == 0 {
            self.next_id = 1;
        }
        let id = self.next_id;
        self.next_id += 1;
        clip.id = id;
        self.clips.push(clip);
        id
    }

    /// The last second anything stands on, given a file length per clip.
    ///
    /// `file_s` answers a clip's PATH with the file's length in seconds —
    /// the host's fact — and zero for a file it has not met.
    pub fn span_s(&self, file_s: impl Fn(&str) -> f64) -> f64 {
        self.clips
            .iter()
            .map(|c| c.end_s(file_s(&c.path)))
            .fold(0.0, f64::max)
    }

    pub fn from_json(text: &str) -> Self {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        Self::from_map(map)
    }

    /// This document copied with `desk: None`.
    ///
    /// For undo, which keeps a moment as the struct rather than as text and
    /// carries the LIVE desk beside it — so the desk this document happens
    /// to be holding (the one it was opened or last saved with, thousands
    /// of seat strings) must not be copied a hundred times over. Every
    /// other field is named so a field added to the struct is a compile
    /// error here rather than a moment that silently forgets it.
    pub fn clone_without_desk(&self) -> Self {
        Self {
            clips: self.clips.clone(),
            desk: None,
            next_id: self.next_id,
            named: self.named,
            extra: self.extra.clone(),
            automation: self.automation.clone(),
            tempo: self.tempo.clone(),
            meter: self.meter.clone(),
            markers: self.markers.clone(),
            version: self.version,
        }
    }

    fn from_map(map: Map<String, Value>) -> Self {
        Self::from_map_report(map).0
    }

    /// `from_map`, and the format-2 desk it refused, if it refused one.
    ///
    /// **A refused desk costs the open, never the bytes** (design 6.3). The
    /// strict road (`read_from`) answers `LoadOutcome::Refused` and the app
    /// opens nothing; the lenient roads (a template probe, a `named` peek)
    /// get the document with no desk and the raw desk kept under its own
    /// key in `extra`, so even a lenient save writes it back as it was.
    fn from_map_report(
        mut map: Map<String, Value>,
    ) -> (Self, Option<crate::desk_format::DeskRefused>) {
        // Version 1 documents simply lack the v2 fields, and every one of
        // them has an honest default — no trim, no fade, name from the file,
        // ids assigned below — so the "migration" is the defaults.
        let version = map
            .shift_remove("arrangement_version")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let secs = |c: &Map<String, Value>, key: &str| {
            c.get(key)
                .and_then(Value::as_f64)
                .filter(|s| s.is_finite() && *s >= 0.0)
                .unwrap_or(0.0)
        };
        // The desk's era, for the master's automation lane below: the
        // stamp alone cannot tell the 24-channel window from 32. And whether
        // the desk is format 2, whose curves name their rows as they are and
        // the master by name, so no era bounds them.
        let era = map
            .get("desk")
            .map_or(crate::desk_lift::DeskEra::V32, crate::desk_lift::era_of_desk);
        let v2_desk = map.get("desk").is_some_and(crate::desk_format::is_format2);
        let (desk, refused) = match map.shift_remove("desk").as_ref().map(DeskSnap::from_value) {
            None => (None, None),
            Some(Ok(desk)) => (desk, None),
            Some(Err(refused)) => (None, Some(refused)),
        };
        // **The version-4 migration, and the document's first real one.**
        //
        // A v4 file has no maps and one tempo, which lives on the desk
        // snapshot. Reading it as a map of one point is the whole migration —
        // and skipping it would open a song written at 90 at 120, which is
        // the sort of thing somebody notices as "it sounds wrong" and never
        // reports as a bug.
        let (tempo, meter) = Self::read_maps(&mut map, version, desk.as_ref());
        let automation = Self::read_automation(&mut map, version, era, v2_desk);
        let mut clips = Vec::new();
        if let Some(Value::Array(list)) = map.shift_remove("clips") {
            for item in list {
                let Value::Object(mut c) = item else { continue };
                let lane = c.get("lane").and_then(Value::as_u64).unwrap_or(0) as usize;
                let kind = match c.get("kind").and_then(Value::as_str) {
                    Some("midi") => ClipKind::Midi,
                    _ => ClipKind::Audio,
                };
                // An AUDIO clip without a file is nothing; a MIDI clip has
                // no file to name.
                let path = c.get("path").and_then(Value::as_str).unwrap_or("");
                if kind == ClipKind::Audio && path.is_empty() {
                    continue;
                }
                let name = c
                    .get("name")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| {
                        std::path::Path::new(path)
                            .file_stem()
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_default()
                    });
                let mut notes = Vec::new();
                if let Some(Value::Array(list)) = c.get("notes") {
                    for n in list {
                        let Value::Object(n) = n else { continue };
                        let num = |key: &str| n.get(key).and_then(Value::as_f64);
                        let (Some(at_s), Some(dur_s)) = (num("at_s"), num("dur_s")) else {
                            continue;
                        };
                        let key = n.get("key").and_then(Value::as_u64).unwrap_or(0);
                        if !(at_s.is_finite() && dur_s.is_finite()) || key > 127 {
                            continue;
                        }
                        notes.push(ArrNote {
                            at_s: at_s.max(0.0),
                            dur_s: dur_s.max(0.0),
                            key: key as u8,
                            vel: n.get("vel").and_then(Value::as_u64).unwrap_or(100).min(127)
                                as u8,
                            // **Absent means AUDIBLE**, the migration rule this
                            // format keeps: a document written before mute
                            // existed does not say a note is muted, and the
                            // only honest reading of silence is "no".
                            muted: n.get("muted").and_then(Value::as_bool).unwrap_or(false),
                        });
                    }
                }
                // **The pedal, read like the notes and clamped like them.**
                // Sorted on the way in rather than trusted: everything
                // downstream binary-searches this list for the level at an
                // instant, and a hand-edited document with its edges out of
                // order would answer with the wrong one.
                let mut pedal = Vec::new();
                if let Some(Value::Array(list)) = c.get("pedal") {
                    for e in list {
                        let Value::Object(e) = e else { continue };
                        let Some(at_s) = e.get("at_s").and_then(Value::as_f64) else {
                            continue;
                        };
                        // An unnamed pedal is the sustain pedal: it is the
                        // one anybody writing this by hand means, and the
                        // only one the built-in instruments model.
                        let cc = e.get("cc").and_then(Value::as_u64).unwrap_or(64);
                        if !at_s.is_finite() || !matches!(cc, 64 | 66 | 67) {
                            continue;
                        }
                        pedal.push(ArrPedal {
                            at_s: at_s.max(0.0),
                            cc: cc as u8,
                            value: e
                                .get("value")
                                .and_then(Value::as_u64)
                                .unwrap_or(0)
                                .min(127) as u8,
                        });
                    }
                    pedal.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
                }
                let clip = ArrClip {
                    kind,
                    notes,
                    pedal,
                    lane,
                    path: path.to_owned(),
                    start_s: secs(&c, "start_s"),
                    offset_s: secs(&c, "offset_s"),
                    len_s: secs(&c, "len_s"),
                    loop_s: secs(&c, "loop_s"),
                    fade_in_s: secs(&c, "fade_in_s"),
                    fade_out_s: secs(&c, "fade_out_s"),
                    name,
                    id: c.get("id").and_then(Value::as_u64).unwrap_or(0),
                    // **A version-4 clip is TIME-anchored, always.** Not
                    // because that is the better default for MIDI — it is
                    // not — but because it is what v4 DID: there was no
                    // tempo map, so nothing followed a tempo. A migration
                    // that helpfully made old MIDI musical would move notes
                    // in songs people have already finished.
                    anchor: if version >= V5_MUSICAL_TIME {
                        c.get("anchor")
                            .and_then(Value::as_str)
                            .map_or(Anchor::Time, Anchor::from_key)
                    } else {
                        Anchor::Time
                    },
                    follow_tempo: c
                        .get("follow_tempo")
                        .and_then(Value::as_bool)
                        .unwrap_or(false),
                    // **Clamped on read**, like every gain: this multiplies
                    // into a buffer length, and a NaN or a zero there is an
                    // allocation of nothing or of everything.
                    stretch: c
                        .get("stretch")
                        .and_then(Value::as_f64)
                        .filter(|v| {
                            v.is_finite()
                                && (STRETCH_RATIO.0..=STRETCH_RATIO.1).contains(v)
                        })
                        .unwrap_or(1.0),
                    // Kept as whatever string is there. An algorithm this
                    // build does not know reads as auto — see the binary's
                    // `Mode::from_key` — which is the right answer and not
                    // an error.
                    // **Anything out of order is dropped, not repaired.**
                    // A warp map that goes backwards cannot be rendered and
                    // guessing at what it meant would hide whoever wrote it.
                    warp: c
                        .get("warp")
                        .and_then(Value::as_array)
                        .map(|a| {
                            let mut out: Vec<(u64, u64)> = Vec::with_capacity(a.len());
                            for p in a {
                                let Some(p) = p.as_array() else { continue };
                                let (Some(f), Some(t)) = (
                                    p.first().and_then(Value::as_u64),
                                    p.get(1).and_then(Value::as_u64),
                                ) else {
                                    continue;
                                };
                                if out.last().is_none_or(|&(lf, lt)| f > lf && t > lt) {
                                    out.push((f, t));
                                }
                            }
                            out
                        })
                        .unwrap_or_default(),
                    // Kept as whatever string is there, like `stretch_mode`
                    // and for the same reason: a fit this build does not
                    // know reads as auto, which is the right answer.
                    warp_fit: c
                        .get("warp_fit")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    // **Absent is UNKNOWN, never 48 kHz**: a map from before
                    // the field is read as it stands. So is a rate no decode
                    // can have (the decoder clamps every one to 8-192 kHz),
                    // because the rescale multiplies it into frame counts.
                    warp_rate: c
                        .get("warp_rate")
                        .and_then(Value::as_u64)
                        .and_then(|r| u32::try_from(r).ok())
                        .filter(|r| (8_000..=192_000).contains(r))
                        .unwrap_or(0),
                    stretch_mode: c
                        .get("stretch_mode")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    // Clamped on read, like every gain: this multiplies into
                    // somebody's monitors.
                    gain: c
                        .get("gain")
                        .and_then(Value::as_f64)
                        .filter(|v| v.is_finite() && (0.0..=64.0).contains(v))
                        .unwrap_or(1.0),
                    // **Whatever is left is somebody else's.** Take the keys
                    // this build understands out of the object and keep the
                    // remainder verbatim; `shift_remove` and not `remove`,
                    // for the settings file's reason — under `preserve_order`
                    // the plain one is a swap and would rotate a preserved
                    // key into the hole it just made.
                    //
                    // The list below is every name read above and must
                    // stay in step with them. A field added to `ArrClip`
                    // without a name added here comes back DUPLICATED: once
                    // as itself and once inside `extra`, and the extras merge
                    // in `to_json` would then be writing a stale copy of a
                    // live field. `a_clip_knows_which_of_its_keys_are_its_own`
                    // is what catches that, for a field its clip SETS: `warp`
                    // and `warp_fit` went uncaught while it left them empty.
                    extra: {
                        for known in CLIP_KEYS {
                            // **A key we own but cannot READ stays where it
                            // is.**
                            //
                            // Owning a name should not mean eating a value.
                            // A newer build that writes `stretch` as an
                            // object rather than a number is exactly the
                            // case `a_newer_builds_clip_keys_survive`
                            // describes, and consuming it into a field that
                            // then falls back to its default is the silent
                            // data loss `extra` exists to stop — the version
                            // of it that is hardest to see, because the name
                            // is one this build recognises.
                            //
                            // "Readable" is the type the reader above asked
                            // for. A number that is out of range IS readable
                            // and is clamped; that is a value we understood
                            // and disagreed with, which is ours to own.
                            if !clip_key_is_readable(known, c.get(known)) {
                                continue;
                            }
                            c.shift_remove(known);
                        }
                        c
                    },
                };
                clips.push(clip);
            }
        }
        // Ids: keep what the file says where it is usable, hand out fresh
        // ones where it is not — duplicates included, because two clips with
        // one id is a selection that edits the wrong clip.
        let stored_next = map
            .shift_remove("next_clip_id")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        let mut next_id = stored_next
            .max(clips.iter().map(|c| c.id + 1).max().unwrap_or(0))
            .max(1);
        let mut seen = Vec::with_capacity(clips.len());
        for clip in &mut clips {
            if clip.id == 0 || seen.contains(&clip.id) {
                clip.id = next_id;
                next_id += 1;
            }
            seen.push(clip.id);
        }
        let named = map
            .shift_remove("named")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        // **Read out of the map, so it does not fall through to `extra`.**
        // Anything left in `map` becomes the unknown-keys bag and is written
        // back verbatim; a key this build DOES understand has to be taken
        // first or the document would carry two copies of it.
        //
        // Forgiving in the settings file's manner: a row without a usable
        // time is dropped rather than defaulted to zero, because a marker at
        // the wrong place is worse than a marker that is missing. An unnamed
        // one is kept — the name is the only thing somebody can fix in the
        // app, and the place is the thing they cannot.
        let markers: Vec<ArrMarker> = match map.shift_remove("markers") {
            Some(Value::Array(v)) => v
                .into_iter()
                .filter_map(|x| {
                    let o = x.as_object()?;
                    let at_s = o.get("at")?.as_f64()?;
                    if !at_s.is_finite() || at_s < 0.0 {
                        return None;
                    }
                    Some(ArrMarker {
                        at_s,
                        name: o
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                    })
                })
                .collect(),
            _ => Vec::new(),
        };
        if let Some(r) = refused.as_ref() {
            map.insert("desk".into(), r.raw.clone());
        }
        let doc = Self {
            clips,
            desk,
            next_id,
            named,
            extra: map,
            // Up, never down. A v1 document has just been migrated by being
            // read; a v9 one keeps the number its own build will look for.
            version: version.max(ARRANGEMENT_VERSION),
            tempo,
            meter,
            markers,
            automation,
        };
        (doc, refused)
    }

    /// The two maps, migrated from whatever the file happened to be.
    fn read_maps(
        map: &mut Map<String, Value>,
        version: u64,
        desk: Option<&DeskSnap>,
    ) -> (crate::tempo::TempoMap, crate::tempo::MeterMap) {
        use crate::tempo::{MeterMap, MeterPoint, TempoMap, TempoPoint};
        let tempo_json = map.shift_remove("tempo_map");
        let meter_json = map.shift_remove("meter_map");
        if version < V5_MUSICAL_TIME {
            // v4 and earlier: one tempo and one signature, both on the desk.
            let bpm = desk.map_or(crate::recorder::DEFAULT_BPM, |d| d.tempo_bpm);
            let sig = desk.map_or_else(crate::recorder::TimeSignature::default, |d| {
                crate::recorder::TimeSignature { beats: d.sig_beats, unit: d.sig_unit }
            });
            return (TempoMap::constant(bpm), MeterMap::constant(sig));
        }
        let tempo = match tempo_json {
            Some(Value::Array(list)) => TempoMap::sanitised(
                list.iter()
                    .filter_map(|v| {
                        let o = v.as_object()?;
                        Some(TempoPoint {
                            at_beat: o.get("at")?.as_f64()?,
                            bpm: o.get("bpm")?.as_f64()?,
                            ramp: o.get("ramp").and_then(Value::as_bool).unwrap_or(false),
                        })
                    })
                    .collect(),
            ),
            // A v5 document with no map is one saved at a single tempo, and
            // the desk still carries it.
            _ => TempoMap::constant(desk.map_or(crate::recorder::DEFAULT_BPM, |d| d.tempo_bpm)),
        };
        let meter = match meter_json {
            Some(Value::Array(list)) => MeterMap::sanitised(
                list.iter()
                    .filter_map(|v| {
                        let o = v.as_object()?;
                        Some(MeterPoint {
                            at_bar: u32::try_from(o.get("at")?.as_u64()?).ok()?,
                            sig: crate::recorder::TimeSignature {
                                beats: u8::try_from(o.get("beats")?.as_u64()?).ok()?,
                                unit: u8::try_from(o.get("unit")?.as_u64()?).ok()?,
                            },
                        })
                    })
                    .collect(),
            ),
            _ => MeterMap::constant(desk.map_or_else(
                crate::recorder::TimeSignature::default,
                |d| crate::recorder::TimeSignature { beats: d.sig_beats, unit: d.sig_unit },
            )),
        };
        (tempo, meter)
    }

    /// The automation curves, or none at all.
    ///
    /// **No version gate**, unlike the tempo maps: there is nothing in a v4
    /// document to migrate FROM, so an absent key simply means a song nobody
    /// has automated. Every value is put through `Curve::sanitised` rather
    /// than believed, for the reason that function gives.
    fn read_automation(
        map: &mut Map<String, Value>,
        version: u64,
        era: crate::desk_lift::DeskEra,
        v2_desk: bool,
    ) -> crate::automation::Automation {
        use crate::automation::{AutoLane, AutoParam, Automation, Curve, Point, Shape};
        let Some(Value::Array(list)) = map.shift_remove("automation") else {
            return Automation::default();
        };
        let curves = list
            .iter()
            .filter_map(|v| {
                let o = v.as_object()?;
                // A format-2 document names the master's curves by target
                // (`automation::MASTER_TARGET`); it lands on V32's master
                // lane, the in-memory lane every era's master is lifted to.
                // Where the curve says it lives: a row by its lane, or the
                // master by name (`automation::MASTER_TARGET`).
                let lane = match crate::automation::persisted_lane(o)? {
                    // **A format-2 document's rows are the desk's own**, as
                    // many as it has: no era bounds them, and the master is
                    // never a number one of them could reach.
                    AutoLane::Row(row) if v2_desk => {
                        (row < crate::desk_id::MAX_ROWS).then_some(AutoLane::Row(row))?
                    }
                    // **A legacy document filed its master at the lane one
                    // past its era's channels** (`desk_lift::lift_curve_lane`,
                    // at the frozen literals: 32 for a V32 document, 24 and
                    // 16 for the older ones, whose builds never wrote past
                    // it), which lands on the master by name. A curve
                    // naming a channel past that is DROPPED rather than
                    // clamped onto the last one: a level landing on the
                    // wrong instrument is worse than one that does not land
                    // at all, and it would be silent.
                    AutoLane::Row(row) => match crate::desk_lift::lift_curve_lane(version, era, row)? {
                        crate::desk_lift::V32_MASTER_LANE => AutoLane::Master,
                        row => AutoLane::Row(row),
                    },
                    AutoLane::Master => AutoLane::Master,
                };
                let param = AutoParam::from_key(o.get("param")?.as_str()?)?;
                let points = o
                    .get("points")?
                    .as_array()?
                    .iter()
                    .filter_map(|p| {
                        let p = p.as_object()?;
                        Some(Point {
                            at_s: p.get("at")?.as_f64()?,
                            value: p.get("v")?.as_f64()?,
                            shape: p
                                .get("shape")
                                .and_then(Value::as_str)
                                .map_or(Shape::Linear, Shape::from_key),
                        })
                    })
                    .collect();
                let mut curve = Curve::sanitised(lane, param, points);
                // Whose a plugin curve kept through an unload is - see
                // `Curve::tenant`. Absent on every other curve.
                if let Some(t) = o.get("tenant").and_then(Value::as_str) {
                    curve.tenant = t.to_owned();
                }
                Some(curve)
            })
            .collect();
        Automation::from_curves(curves)
    }

    pub fn to_json(&self) -> String {
        self.to_json_with_desk(self.desk.as_ref())
    }

    /// The same text with `desk` standing in for [`Arrangement::desk`].
    ///
    /// **Split out so undo need not CLONE the document to change one
    /// field.** `undo_state` wants the live desk rather than the one the
    /// document is carrying, and the only way to say so used to be
    /// `self.arrangement.clone()` — eight thousand notes and every path
    /// string copied, per committed edit and once per frame, to overwrite a
    /// single `Option`.
    pub fn to_json_with_desk(&self, desk: Option<&DeskSnap>) -> String {
        self.to_json_with_desk_as(desk, crate::desk_format::Format::current())
    }

    /// The document in `format`: the road a test takes to a format-2 file
    /// without flipping [`crate::desk_format::WRITE_V8`].
    pub(crate) fn to_json_with_desk_as(
        &self,
        desk: Option<&DeskSnap>,
        format: crate::desk_format::Format,
    ) -> String {
        let v2 = format == crate::desk_format::Format::V2;
        let mut map = Map::new();
        // The document's OWN version, not this build's constant. See
        // [`Arrangement::version`]: stamping 4 onto a file still carrying a
        // version-9 build's keys — which `extra` faithfully preserves —
        // would tell that build its own migrations had not run yet. A
        // format-2 desk raises it to 8 at least: the stamp says the shape.
        map.insert(
            "arrangement_version".into(),
            Value::Number(self.version.max(format.document_stamp()).into()),
        );
        // **Written NORMALISED, so that reading it back is a fixed point.**
        //
        // `from_json` raises whatever it finds to `max(highest id + 1, 1)`,
        // because a document whose counter is behind its clips would hand out
        // an id that is already taken. Writing the raw value meant a fresh
        // arrangement wrote 0 and read back 1 — `to_json(from_json(x)) != x`
        // for the commonest document there is.
        //
        // That is not cosmetic. UNDO compares the live document against the
        // moment it is standing on, and a mismatch means "something changed":
        // so every undo pushed a moment identical to the one already on top,
        // truncated the redo tail to make room for it, and then stepped back
        // to exactly where it started. **Undo did nothing and redo was gone.**
        // The same expression on both sides is the whole fix.
        map.insert(
            "next_clip_id".into(),
            Value::Number(
                self.next_id
                    .max(self.clips.iter().map(|c| c.id + 1).max().unwrap_or(0))
                    .max(1)
                    .into(),
            ),
        );
        if let Some(desk) = desk {
            map.insert("desk".into(), desk.to_value_as(format));
        }
        // Written only once true: older documents stay byte-identical.
        if self.named {
            map.insert("named".into(), Value::Bool(true));
        }
        map.insert(
            "clips".into(),
            Value::Array(
                self.clips
                    .iter()
                    .map(|c| {
                        let mut m = Map::new();
                        m.insert(
                            "kind".into(),
                            Value::String(
                                match c.kind {
                                    ClipKind::Audio => "audio",
                                    ClipKind::Midi => "midi",
                                }
                                .to_owned(),
                            ),
                        );
                        m.insert("lane".into(), Value::Number((c.lane as u64).into()));
                        m.insert("path".into(), Value::String(c.path.clone()));
                        m.insert("name".into(), Value::String(c.name.clone()));
                        if !c.notes.is_empty() {
                            m.insert(
                                "notes".into(),
                                Value::Array(
                                    c.notes
                                        .iter()
                                        .map(|n| {
                                            let mut o = Map::new();
                                            for (k, v) in
                                                [("at_s", n.at_s), ("dur_s", n.dur_s)]
                                            {
                                                if let Some(x) =
                                                    serde_json::Number::from_f64(v)
                                                {
                                                    o.insert(k.into(), Value::Number(x));
                                                }
                                            }
                                            o.insert(
                                                "key".into(),
                                                Value::Number(u64::from(n.key).into()),
                                            );
                                            o.insert(
                                                "vel".into(),
                                                Value::Number(u64::from(n.vel).into()),
                                            );
                                            // **Written only when TRUE**, the
                                            // rule `pedal` states below: a
                                            // document full of notes nobody
                                            // has muted stays byte-identical
                                            // to the one this build would have
                                            // written before the flag existed,
                                            // which is what keeps undo's
                                            // `to_json(from_json(x)) == x`
                                            // honest.
                                            if n.muted {
                                                o.insert(
                                                    "muted".into(),
                                                    Value::Bool(true),
                                                );
                                            }
                                            Value::Object(o)
                                        })
                                        .collect(),
                                ),
                            );
                        }
                        // **Omitted when there is none**, like `notes` above
                        // it: every document written before the pedal existed
                        // stays byte-identical, which is what keeps undo's
                        // `to_json(from_json(x)) == x` comparison honest.
                        if !c.pedal.is_empty() {
                            m.insert(
                                "pedal".into(),
                                Value::Array(
                                    c.pedal
                                        .iter()
                                        .map(|e| {
                                            let mut o = Map::new();
                                            if let Some(x) =
                                                serde_json::Number::from_f64(e.at_s)
                                            {
                                                o.insert("at_s".into(), Value::Number(x));
                                            }
                                            o.insert(
                                                "cc".into(),
                                                Value::Number(u64::from(e.cc).into()),
                                            );
                                            o.insert(
                                                "value".into(),
                                                Value::Number(u64::from(e.value).into()),
                                            );
                                            Value::Object(o)
                                        })
                                        .collect(),
                                ),
                            );
                        }
                        m.insert("id".into(), Value::Number(c.id.into()));
                        // Written only when they say something, so a document
                        // full of ordinary time-anchored clips keeps the shape
                        // it had in version 4.
                        if c.anchor != Anchor::Time {
                            m.insert("anchor".into(), Value::String(c.anchor.key().to_owned()));
                        }
                        if c.follow_tempo {
                            m.insert("follow_tempo".into(), Value::Bool(true));
                        }
                        // Written only when there is something to say, so a
                        // clip nobody has stretched keeps its byte-for-byte
                        // shape — the settings file's own rule.
                        if (c.stretch - 1.0).abs() > 1.0e-9 {
                            if let Some(n) = serde_json::Number::from_f64(c.stretch) {
                                m.insert("stretch".into(), Value::Number(n));
                            }
                        }
                        if (c.gain - 1.0).abs() > 1.0e-9 {
                            if let Some(n) = serde_json::Number::from_f64(c.gain) {
                                m.insert("gain".into(), Value::Number(n));
                            }
                        }
                        if !c.warp.is_empty() {
                            m.insert(
                                "warp".into(),
                                Value::Array(
                                    c.warp
                                        .iter()
                                        .map(|&(f, t)| {
                                            Value::Array(vec![
                                                Value::Number(f.into()),
                                                Value::Number(t.into()),
                                            ])
                                        })
                                        .collect(),
                                ),
                            );
                        }
                        if !c.warp_fit.is_empty() && c.warp_fit != "auto" {
                            m.insert("warp_fit".into(), Value::String(c.warp_fit.clone()));
                        }
                        // Unknown writes nothing, so a map from before the
                        // rate was kept stays the document it was.
                        if c.warp_rate != 0 {
                            m.insert("warp_rate".into(), Value::Number(c.warp_rate.into()));
                        }
                        if !c.stretch_mode.is_empty() && c.stretch_mode != "auto" {
                            m.insert(
                                "stretch_mode".into(),
                                Value::String(c.stretch_mode.clone()),
                            );
                        }
                        for (key, v) in [
                            ("start_s", c.start_s),
                            ("offset_s", c.offset_s),
                            ("len_s", c.len_s),
                            ("fade_in_s", c.fade_in_s),
                            ("fade_out_s", c.fade_out_s),
                            // Zero means "does not loop", which is what every
                            // document written before this said by omission.
                            ("loop_s", c.loop_s),
                        ] {
                            if let Some(n) = serde_json::Number::from_f64(v) {
                                m.insert(key.into(), Value::Number(n));
                            }
                        }
                        // The clip's unknown keys LAST, and a KNOWN key wins
                        // — the document-level rule at the tail of this
                        // function, applied one level down. (The settings
                        // file resolves the same collision the other way, and
                        // deliberately: there a preserved extra is the newer
                        // build's opinion about a key this one has since
                        // learned. Here the field was just written from live
                        // state and the extra is a stale copy of it.)
                        for (k, v) in &c.extra {
                            m.entry(k.clone()).or_insert_with(|| v.clone());
                        }
                        Value::Object(m)
                    })
                    .collect(),
            ),
        );
        // **The maps, written only when they SAY something.** A song at one
        // tempo in one signature — which is every song anybody has made with
        // this app so far — writes neither key and keeps the exact shape it
        // had at version 4, apart from its stamp. The desk snapshot still
        // carries the opening tempo, so such a file also still opens
        // correctly in a build that predates all of this.
        if !self.tempo.is_constant() {
            map.insert(
                "tempo_map".into(),
                Value::Array(
                    self.tempo
                        .points()
                        .iter()
                        .map(|p| {
                            let mut o = Map::new();
                            for (k, v) in [("at", p.at_beat), ("bpm", p.bpm)] {
                                if let Some(n) = serde_json::Number::from_f64(v) {
                                    o.insert(k.into(), Value::Number(n));
                                }
                            }
                            if p.ramp {
                                o.insert("ramp".into(), Value::Bool(true));
                            }
                            Value::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        // Written only when there is automation, so a song without any keeps
        // the shape it had — the same bargain the maps make.
        if !self.automation.is_empty() {
            map.insert(
                "automation".into(),
                Value::Array(
                    self.automation
                        .curves()
                        .iter()
                        .map(|c| {
                            let mut o = Map::new();
                            // The master by NAME in format 2 (see
                            // `automation::MASTER_TARGET`); a row by lane.
                            // A legacy document (the tests' legacy writer
                            // alone since format 2) spelt the master at
                            // V32's lane one past the channels.
                            match c.lane {
                                crate::automation::AutoLane::Master if v2 => {
                                    o.insert(
                                        "target".into(),
                                        Value::String(crate::automation::MASTER_TARGET.into()),
                                    );
                                }
                                lane => {
                                    let n = lane.index_or(crate::desk_lift::V32_MASTER_LANE);
                                    o.insert("lane".into(), Value::Number((n as u64).into()));
                                }
                            }
                            o.insert("param".into(), Value::String(c.param.key().to_owned()));
                            // Only on a curve that is waiting for its plugin.
                            if !c.tenant.is_empty() {
                                o.insert("tenant".into(), Value::String(c.tenant.clone()));
                            }
                            o.insert(
                                "points".into(),
                                Value::Array(
                                    c.points()
                                        .iter()
                                        .map(|p| {
                                            let mut q = Map::new();
                                            for (k, v) in [("at", p.at_s), ("v", p.value)] {
                                                if let Some(n) =
                                                    serde_json::Number::from_f64(v)
                                                {
                                                    q.insert(k.into(), Value::Number(n));
                                                }
                                            }
                                            // The ordinary shape is not
                                            // written; a file of linear
                                            // points stays small.
                                            if p.shape != crate::automation::Shape::Linear {
                                                q.insert(
                                                    "shape".into(),
                                                    Value::String(p.shape.key().to_owned()),
                                                );
                                            }
                                            Value::Object(q)
                                        })
                                        .collect(),
                                ),
                            );
                            Value::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        if !self.meter.is_constant() {
            map.insert(
                "meter_map".into(),
                Value::Array(
                    self.meter
                        .points()
                        .iter()
                        .map(|p| {
                            let mut o = Map::new();
                            o.insert("at".into(), Value::Number(u64::from(p.at_bar).into()));
                            o.insert("beats".into(), Value::Number(u64::from(p.sig.beats).into()));
                            o.insert("unit".into(), Value::Number(u64::from(p.sig.unit).into()));
                            Value::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        // **Only when there are any.** A project nobody marked writes no
        // `markers` key at all, which is what keeps a fresh document byte
        // for byte what it always was — `seed_template`'s stock-shape test
        // compares documents, and a new empty key would have made every
        // shipped template read as somebody's arrangement.
        if !self.markers.is_empty() {
            map.insert(
                "markers".into(),
                Value::Array(
                    self.markers
                        .iter()
                        .map(|m| {
                            let mut o = Map::new();
                            o.insert(
                                "at".into(),
                                serde_json::Number::from_f64(m.at_s)
                                    .map_or(Value::Null, Value::Number),
                            );
                            o.insert("name".into(), Value::String(m.name.clone()));
                            Value::Object(o)
                        })
                        .collect(),
                ),
            );
        }
        // Unknown keys LAST, like the settings file: a newer build's keys win
        // over nothing, and they survive.
        for (k, v) in &self.extra {
            map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into())
    }

    /// **The document as one `u64`** — the same question `to_json` answers,
    /// without building the answer.
    ///
    /// `desk` stands in for [`Arrangement::desk`], exactly as it does in
    /// `to_json_with_desk`, so the two can be handed the same one.
    pub fn fingerprint_with_desk(&self, desk: Option<&DeskSnap>) -> u64 {
        let mut f = Fingerprint::new();
        self.hash_into(desk, &mut f);
        f.finish()
    }

    /// The document, desk and all, as it stands.
    pub fn fingerprint(&self) -> u64 {
        self.fingerprint_with_desk(self.desk.as_ref())
    }

    /// The document into somebody else's [`Fingerprint`], so a caller that
    /// has more to mix in — the drum kits, which live in a sidecar — gets
    /// ONE number for the pair rather than two to keep in step.
    ///
    /// **This walks beside `to_json_with_desk` and must keep walking beside
    /// it.** Every conditional there is repeated here, because a key the
    /// writer sometimes omits has to hash as one thing across every value
    /// that produces the omission — otherwise a document nobody touched
    /// fingerprints differently from itself.
    ///
    /// The two guards are `the_fingerprint_moves_when_the_document_moves`
    /// and `the_fingerprint_stands_when_the_document_stands`. Between them
    /// they take the writer as the ORACLE: sixty-odd edits, each asserting
    /// that the fingerprint moved exactly when the text did. A field added
    /// to the writer and forgotten here fails a test rather than shipping a
    /// title bar that cannot see it.
    pub fn hash_into(&self, desk: Option<&DeskSnap>, f: &mut Fingerprint) {
        self.hash_into_ref(desk.map(DeskSnap::as_ref), f);
    }

    /// `hash_into` over a desk that is only borrowed — the app's live desk,
    /// read straight out of the settings once a frame. See [`DeskRef`].
    ///
    /// **A term in this walk is an undoable field** - see `DeskRef::hash_into`
    /// for what adding one costs.
    pub fn hash_into_ref(&self, desk: Option<DeskRef<'_>>, f: &mut Fingerprint) {
        f.u64(self.version);
        // The NORMALISED counter, the same expression the writer emits —
        // see `to_json`'s note on why a document has to be a fixed point.
        f.u64(
            self.next_id
                .max(self.clips.iter().map(|c| c.id + 1).max().unwrap_or(0))
                .max(1),
        );
        match desk {
            Some(d) => {
                f.u64(1);
                d.hash_into(f);
            }
            None => f.u64(0),
        }
        f.bool(self.named);
        f.len(self.clips.len());
        for c in &self.clips {
            f.u64(match c.kind {
                ClipKind::Audio => 0,
                ClipKind::Midi => 1,
            });
            f.len(c.lane);
            f.str(&c.path);
            f.str(&c.name);
            f.len(c.notes.len());
            for n in &c.notes {
                f.f64(n.at_s);
                f.f64(n.dur_s);
                f.u64(u64::from(n.key));
                f.u64(u64::from(n.vel));
                // **Muting is a change**, so it has to reach this hash or
                // the project would not know it needs saving and undo would
                // not know a step happened — which is the exact fault the
                // pedal's own note below records from the other side.
                f.u64(u64::from(n.muted));
            }
            // The pedal, after the notes as the writer has it. Omitted when
            // empty there, so the length is the one word that tells "no
            // pedal" from "a pedal" — the notes' own arrangement. This was
            // MISSING: the pedal lane shipped without a word here, and the
            // title bar could not see a pedal edit until the next note moved.
            f.len(c.pedal.len());
            for e in &c.pedal {
                f.f64(e.at_s);
                f.u64(u64::from(e.cc));
                f.u64(u64::from(e.value));
            }
            f.u64(c.id);
            f.u64(match c.anchor {
                Anchor::Time => 0,
                Anchor::Beat => 1,
            });
            f.bool(c.follow_tempo);
            // The writer's own deadbands: inside one, no key is written at
            // all, so every ratio inside it is the same document.
            f.maybe_f64(Some(c.stretch).filter(|s| (s - 1.0).abs() > 1.0e-9));
            f.maybe_f64(Some(c.gain).filter(|g| (g - 1.0).abs() > 1.0e-9));
            // `""` and `"auto"` write the same nothing.
            f.str(if c.stretch_mode == "auto" { "" } else { &c.stretch_mode });
            for v in [
                c.start_s,
                c.offset_s,
                c.len_s,
                c.fade_in_s,
                c.fade_out_s,
                c.loop_s,
            ] {
                f.f64(v);
            }
            f.map(&c.extra);
            // **APPENDED, at the end of the clip's own block**, which is the
            // rule this walk lives by: it runs in declaration order, so a
            // term put among the others moves every existing document's
            // words for no reason.
            //
            // **And it had to be here at all.** A quantize writes ONLY these
            // two fields, so without them the walk produced identical words
            // and `undo_commit` decided nothing had happened - no undo entry,
            // and, worse, `project_dirty` could not see it either. Normalise
            // then quantize then Ctrl+Z took the NORMALISE away, which is how
            // this was found, and a quantized take could have been closed
            // without so much as an asterisk to say it was unsaved.
            f.len(c.warp.len());
            for &(from, to) in &c.warp {
                f.u64(from);
                f.u64(to);
            }
            // `""` and `"auto"` write the same nothing, as `stretch_mode`.
            f.str(if c.warp_fit == "auto" { "" } else { &c.warp_fit });
            // Appended for the same reason: the same pairs at another rate
            // move other seconds, so a rate change is an edit undo can take
            // back and the asterisk can see.
            f.u64(u64::from(c.warp_rate));
        }
        // **A constant map is not written, so its VALUE is not in the
        // document.** One tempo all the way through lives on the desk
        // snapshot instead — so hashing the point here would report a change
        // the file cannot hold, and every undo would look like an edit.
        if self.tempo.is_constant() {
            f.u64(0);
        } else {
            f.u64(1);
            f.len(self.tempo.points().len());
            for p in self.tempo.points() {
                f.f64(p.at_beat);
                f.f64(p.bpm);
                f.bool(p.ramp);
            }
        }
        if self.automation.is_empty() {
            f.u64(0);
        } else {
            f.u64(1);
            f.len(self.automation.curves().len());
            for c in self.automation.curves() {
                // A row by its lane; the master by a word no row's lane can
                // be, so a curve moved between the two is a change.
                match c.lane {
                    crate::automation::AutoLane::Row(r) => f.len(r),
                    crate::automation::AutoLane::Master => f.u64(u64::MAX),
                }
                // The VARIANT rather than `AutoParam::key`, which allocates a
                // `String` per curve. The mapping is one-to-one with the
                // spelling — that is what `from_key` round-tripping means —
                // so the two tell exactly the same curves apart.
                match c.param {
                    crate::automation::AutoParam::Gain => f.u64(0),
                    crate::automation::AutoParam::Pan => f.u64(1),
                    crate::automation::AutoParam::Send => f.u64(2),
                    crate::automation::AutoParam::Mute => f.u64(3),
                    crate::automation::AutoParam::Plugin { bay, id } => {
                        f.u64(4);
                        f.u64(u64::from(bay));
                        f.u64(u64::from(id));
                    }
                    crate::automation::AutoParam::SendTo { to } => {
                        f.u64(5);
                        f.u64(u64::from(to.0));
                    }
                }
                // The tenant under the writer's own condition: written only
                // when there is one, so a tag word says which.
                if c.tenant.is_empty() {
                    f.u64(0);
                } else {
                    f.u64(1);
                    f.str(&c.tenant);
                }
                f.len(c.points().len());
                for p in c.points() {
                    f.f64(p.at_s);
                    f.f64(p.value);
                    f.u64(match p.shape {
                        crate::automation::Shape::Linear => 0,
                        crate::automation::Shape::Hold => 1,
                    });
                }
            }
        }
        if self.meter.is_constant() {
            f.u64(0);
        } else {
            f.u64(1);
            f.len(self.meter.points().len());
            for p in self.meter.points() {
                f.u64(u64::from(p.at_bar));
                f.u64(u64::from(p.sig.beats));
                f.u64(u64::from(p.sig.unit));
            }
        }
        // Markers, written only when there are any, so the length alone
        // says "none" the way the writer's absent key does. Also MISSING
        // until now — a marker placed, moved or renamed left the asterisk
        // off; the quit prompt, being exact, still asked, which is the one
        // reason nobody lost a marker to it.
        f.len(self.markers.len());
        for m in &self.markers {
            f.f64(m.at_s);
            f.str(&m.name);
        }
        f.map(&self.extra);
    }
}

/// The settings directory, shared with `Settings::path` by construction.
#[allow(non_snake_case)]
fn Settings_dir() -> PathBuf {
    crate::settings::Settings::path()
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default()
}

/// `Settings::word`'s twin on a snapshot's rows, for the same tests.
#[cfg(test)]
impl DeskSnap {
    pub(crate) fn word(&self, f: fn(&crate::desk_model::TrackRow) -> bool) -> u64 {
        self.rows.iter().take(64).enumerate().filter(|(_, r)| f(r)).fold(0, |w, (n, _)| w | 1 << n)
    }

    pub(crate) fn set_word(&mut self, f: fn(&mut crate::desk_model::TrackRow) -> &mut bool, word: u64) {
        for (n, r) in self.rows.iter_mut().enumerate() {
            *f(r) = n < 64 && (word >> n) & 1 == 1;
        }
    }
}

#[cfg(test)]
mod tests {

    /// **Missing, empty and damaged are three different findings**, and only
    /// a clean parse may be adopted silently. `load_from` keeps collapsing
    /// them for the probes that only peek; `read_from` is the opening road.
    /// **Markers survive a save and a load, and a document with none says
    /// nothing about them.**
    ///
    /// The owner's rule for where they may live: "only be stored as values in
    /// a project where they are added. Load new? Reset all? Markers gone.
    /// Save track, then new track, then load saved, the markers should stay
    /// from the first save." Putting them in the document gives all four for
    /// free — a new project is a fresh `Arrangement`, and a reset opens one —
    /// so what is left to check is the round trip and the silence.
    ///
    /// The silence half is not tidiness. `seed_template` decides whether a
    /// template is an untouched stock seed by comparing documents, so a key
    /// written unconditionally would make every shipped template read as
    /// somebody's arrangement and stop being re-seeded.
    ///
    /// Revert-checks, both performed: drop the `is_empty` guard on the write
    /// and the silence assert goes red; drop the `shift_remove("markers")`
    /// read and the round trip comes back with none, because the key falls
    /// through to `extra` and is written back beside a fresh empty list.
    #[test]
    fn markers_round_trip_and_an_unmarked_song_writes_none() {
        let mut arr = Arrangement::default();
        assert!(
            !arr.to_json().contains("markers"),
            "a song nobody marked wrote a markers key: {}",
            arr.to_json()
        );

        arr.markers.push(ArrMarker { at_s: 12.5, name: "Verse".to_owned() });
        arr.markers.push(ArrMarker { at_s: 48.0, name: String::new() });
        let back = Arrangement::from_json(&arr.to_json());
        assert_eq!(back.markers, arr.markers, "the markers did not come home");
        // And they did not ALSO land in the unknown-keys bag, which would
        // write two copies on the next save.
        assert!(
            !back.extra.contains_key("markers"),
            "the markers fell through to `extra` as well"
        );

        // A row with no usable time is dropped rather than defaulted to zero:
        // a marker in the wrong place is worse than one that is missing.
        let hand = r#"{"arrangement_version":6,"clips":[],"markers":[
            {"at":3.0,"name":"keep"},{"name":"no time"},{"at":-1.0,"name":"before the song"}]}"#;
        let read = Arrangement::from_json(hand);
        assert_eq!(read.markers.len(), 1, "a bad row was kept: {:?}", read.markers);
        assert_eq!(read.markers[0].name, "keep");
    }

    #[test]
    fn read_from_tells_missing_empty_and_damaged_apart() {
        let dir = std::env::temp_dir()
            .join(format!("tangent-read-outcome-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let doc = dir.join("project.tangent");

        assert!(matches!(
            Arrangement::read_from(&doc),
            LoadOutcome::Missing
        ));
        std::fs::write(&doc, "  \n\t ").unwrap();
        assert!(matches!(Arrangement::read_from(&doc), LoadOutcome::Empty));
        std::fs::write(&doc, r#"{"clips": ["#).unwrap();
        assert!(matches!(
            Arrangement::read_from(&doc),
            LoadOutcome::Damaged
        ));
        // A JSON root that is not an object is damage too, not a document.
        std::fs::write(&doc, "[1,2,3]").unwrap();
        assert!(matches!(
            Arrangement::read_from(&doc),
            LoadOutcome::Damaged
        ));
        let mut arr = Arrangement::default();
        arr.named = true;
        arr.save_to(&doc);
        match Arrangement::read_from(&doc) {
            LoadOutcome::Loaded(a) => assert!(a.named),
            _ => panic!("a clean save did not read back Loaded"),
        }
        // And load_from still answers a default for every non-Loaded case,
        // which its ~30 peeking callers rely on.
        std::fs::write(&doc, "not json").unwrap();
        assert!(Arrangement::load_from(&doc).clips.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A clip's loop length survives the document**, and its absence means
    /// "does not loop" — which is what every file written before this said.
    #[test]
    fn a_loop_length_round_trips_and_absence_means_no_loop() {
        let mut arr = Arrangement::default();
        arr.add_clip(ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Midi,
            lane: 2,
            start_s: 1.0,
            len_s: 8.0,
            loop_s: 2.0,
            notes: vec![ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100, muted: false }],
            ..Default::default()
        });
        let back = Arrangement::from_json(&arr.to_json());
        assert_eq!(back.clips.len(), 1);
        assert!(
            (back.clips[0].loop_s - 2.0).abs() < 1e-9,
            "the loop length was lost: {}",
            back.clips[0].loop_s
        );

        // A document with no `loop_s` at all reads as not looping, rather
        // than as some default that would start repeating old work.
        let older = r#"{"version":3,"clips":[{"kind":"midi","lane":0,"start_s":0.0,"len_s":4.0,"notes":[]}] }"#;
        let old = Arrangement::from_json(older);
        assert_eq!(old.clips.len(), 1);
        assert_eq!(old.clips[0].loop_s, 0.0, "an old clip came back looping");
    }
    use super::*;

    fn a_clip(lane: usize, path: &str, start_s: f64) -> ArrClip {
        ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Audio,
            notes: Vec::new(),
            lane,
            path: path.to_owned(),
            start_s,
            offset_s: 0.0,
            len_s: 0.0,
            loop_s: 0.0,
        fade_in_s: 0.0,
            fade_out_s: 0.0,
            name: std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            id: 0,
            anchor: Anchor::Time,
            follow_tempo: false,
            stretch: 1.0,
            stretch_mode: String::new(),
            gain: 1.0,
            extra: Map::new(),
            warp: Vec::new(),
            warp_fit: String::new(),
            warp_rate: 0,
        }
    }

    /// A document survives its own round trip, clips, edges and all.
    #[test]
    fn an_arrangement_round_trips() {
        let mut a = Arrangement::default();
        a.add_clip(a_clip(0, "media/backing.mp3", 0.0));
        let solo = ArrClip {
            pedal: Vec::new(),
            offset_s: 1.25,
            len_s: 30.0,
            loop_s: 0.0,
            fade_in_s: 0.5,
            fade_out_s: 2.0,
            name: "the solo".into(),
            ..a_clip(3, "/x/solo.wav", 84.5)
        };
        a.add_clip(solo);
        let mut midi = ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Midi,
            path: String::new(),
            len_s: 8.0,
            name: "MIDI".into(),
            notes: vec![
                ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100, muted: false },
                ArrNote { at_s: 1.25, dur_s: 2.0, key: 67, vel: 88, muted: false },
            ],
            ..a_clip(1, "", 4.0)
        };
        midi.kind = ClipKind::Midi;
        a.add_clip(midi);
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back, a, "the document did not survive its own round trip");
    }

    /// A version-1 document — bare lane/path/start clips — opens whole: no
    /// trims, no fades, names from the files, and every clip gets an id.
    #[test]
    fn a_v1_document_opens_with_honest_defaults() {
        let a = Arrangement::from_json(
            r#"{"arrangement_version": 1, "clips": [
                {"lane": 0, "path": "/x/Blue Bossa - backing.mp3", "start_s": 0.0},
                {"lane": 2, "path": "media/solo.wav", "start_s": 12.0}]}"#,
        );
        assert_eq!(a.clips.len(), 2);
        assert_eq!(a.clips[0].name, "Blue Bossa - backing");
        assert_eq!(a.clips[0].len_s, 0.0, "a v1 clip runs to the end of its file");
        assert_ne!(a.clips[0].id, a.clips[1].id, "two clips share an id");
        assert!(a.next_id > a.clips[1].id, "the next id would collide");
        // And ids survive a round trip rather than being re-dealt.
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back, a);
    }

    /// The relative-path rule, in its one home: `media/<file>` resolves
    /// beside the document; an absolute path is itself.
    #[test]
    fn a_clips_path_resolves_beside_the_document() {
        let doc = std::path::Path::new("/songs/nocturne/project.tangent");
        assert_eq!(
            Arrangement::resolve(doc, "media/backing.mp3"),
            std::path::Path::new("/songs/nocturne/media/backing.mp3")
        );
        assert_eq!(
            Arrangement::resolve(doc, "/elsewhere/x.wav"),
            std::path::Path::new("/elsewhere/x.wav")
        );
    }

    /// **A quantize is a CHANGE**, and the walk has to say so.
    ///
    /// The words this produces are what undo compares and what
    /// `project_dirty` compares, so a field missing from it is a field no
    /// undo can reach and no asterisk can report. Found the hard way:
    /// normalise, quantize, Ctrl+Z took the NORMALISE away, because the
    /// quantize had never been recorded as a moment at all - and a
    /// quantized take could have been closed with nothing to say it was
    /// unsaved.
    ///
    /// Asserted for EVERY field, because `warp_fit` and `warp_rate` alone
    /// move no pair and would have been the easy ones to leave out.
    #[test]
    fn a_warp_moves_the_words_that_undo_and_dirty_are_built_on() {
        let words = |a: &Arrangement| {
            let mut f = Fingerprint::recording(0);
            a.hash_into_ref(None, &mut f);
            f.finish_recorded().0
        };
        let mut a = Arrangement::default();
        a.add_clip(ArrClip {
            kind: ClipKind::Audio,
            path: "/x/a.wav".to_owned(),
            lane: 0,
            len_s: 4.0,
            ..Default::default()
        });
        let bare = words(&a);

        a.clips[0].warp = vec![(0, 0), (24_000, 22_050)];
        let warped = words(&a);
        assert_ne!(bare, warped, "a warp map left the words unchanged");

        a.clips[0].warp = vec![(0, 0), (24_000, 26_000)];
        let moved = words(&a);
        assert_ne!(warped, moved, "moving an anchor left the words unchanged");

        a.clips[0].warp_fit = "slice".to_owned();
        assert_ne!(moved, words(&a), "the fit left the words unchanged");

        // The same pairs at another rate are another quantize.
        let fitted = words(&a);
        a.clips[0].warp_rate = 44_100;
        assert_ne!(fitted, words(&a), "the map's rate left the words unchanged");

        // And "auto" is the same nothing as "", exactly as `stretch_mode`:
        // a document that writes neither must not read as two documents.
        let mut auto = a.clone();
        auto.clips[0].warp_fit = "auto".to_owned();
        let mut blank = a.clone();
        blank.clips[0].warp_fit = String::new();
        assert_eq!(words(&auto), words(&blank), "auto and empty parted ways");
    }

    /// **A warp map survives the document, and a broken one does not.**
    ///
    /// The map is what makes an audio quantize outlive the session that made
    /// it: the anchors are the edit, and the rendered audio is derived from
    /// them every load. A map that did not round-trip would quietly un-do
    /// somebody's quantize the next time they opened the song.
    ///
    /// The second half is the half worth having. A list that goes backwards
    /// cannot be rendered - `warp::warp` refuses it - so the reader drops
    /// what it cannot use rather than handing the host a map that will be
    /// silently discarded further down.
    #[test]
    fn a_warp_map_round_trips_and_a_backwards_one_is_dropped() {
        let text = r#"{"arrangement_version": 9, "clips": [
            {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
             "warp": [[0, 0], [24000, 22050], [48000, 44100]]}]}"#;
        let a = Arrangement::from_json(text);
        assert_eq!(
            a.clips[0].warp,
            vec![(0, 0), (24_000, 22_050), (48_000, 44_100)],
            "the warp map did not survive the read"
        );
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(
            back.clips[0].warp, a.clips[0].warp,
            "the warp map did not survive a round trip"
        );

        // Backwards in `from`, backwards in `to`, and a pair that is not a
        // pair: each dropped, and the ones around them kept.
        //
        // **Each is measured against the last KEPT anchor, not the last one
        // read.** Written first with `[18000, 10000]` here, expecting it to
        // be dropped - and it is not, correctly: once `[6000, 20000]` is
        // gone the pair before it is `[12000, 9000]`, and 18000/10000
        // ascends from that perfectly well. The fixture says 8000 now, which
        // genuinely goes back.
        let bad = r#"{"arrangement_version": 9, "clips": [
            {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
             "warp": [[0, 0], [12000, 9000], [6000, 20000], [18000, 8000],
                      [24000, 30000], [99]]}]}"#;
        let c = Arrangement::from_json(bad);
        assert_eq!(
            c.clips[0].warp,
            vec![(0, 0), (12_000, 9_000), (24_000, 30_000)],
            "a backwards anchor was kept"
        );

        // And a clip nobody warped writes no key at all, which is the
        // document's own rule about staying byte-for-byte.
        let plain = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0}]}"#,
        );
        assert!(!plain.to_json().contains("warp"), "an unwarped clip wrote a warp key");
    }

    /// **A warp map keeps the rate it was made at through save and load.**
    ///
    /// The map is frames of the decode the quantize measured, and its rate
    /// is what lets a reader at another rate - an export, a device that has
    /// changed rate since - find the same seconds in its own decode. Lost on
    /// save, the reopened session would apply the frames raw again.
    ///
    /// Unknown stays unknown: a map from before the field writes no key, so
    /// its document is the one it was, and a rate no decode can have (the
    /// decoder clamps every one to 8-192 kHz) is not believed.
    #[test]
    fn a_warp_maps_rate_survives_the_document() {
        let text = r#"{"arrangement_version": 9, "clips": [
            {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
             "warp": [[0, 0], [24000, 22050]], "warp_rate": 44100}]}"#;
        let a = Arrangement::from_json(text);
        assert_eq!(a.clips[0].warp_rate, 44_100, "the map's rate did not survive the read");
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back, a, "the map's rate did not survive a round trip");
        assert!(
            !back.clips[0].extra.contains_key("warp_rate"),
            "the rate came back twice, the second time as a newer build's key"
        );

        let old = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
                 "warp": [[0, 0], [24000, 22050]]}]}"#,
        );
        assert_eq!(old.clips[0].warp_rate, 0, "a map from before its rate was given one");
        assert!(!old.to_json().contains("warp_rate"), "an unknown rate wrote a key");

        let odd = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
                 "warp": [[0, 0], [24000, 22050]], "warp_rate": 3}]}"#,
        );
        assert_eq!(odd.clips[0].warp_rate, 0, "a rate no decode can have was believed");
    }

    /// The span is the furthest clip end, trims and the zero sentinel both
    /// honoured — it is what the scrubber draws against.
    #[test]
    fn the_span_is_the_furthest_clip_end() {
        let mut a = Arrangement::default();
        a.add_clip(a_clip(0, "/x/a.wav", 0.0)); // whole 100 s file
        a.add_clip(ArrClip {
            pedal: Vec::new(),
            len_s: 10.0,
            ..a_clip(1, "/x/b.wav", 95.0) // ends at 105
        });
        a.add_clip(ArrClip {
            pedal: Vec::new(),
            offset_s: 40.0,
            ..a_clip(2, "/x/a.wav", 30.0) // 100-40=60 long, ends at 90
        });
        let span = a.span_s(|_| 100.0);
        assert!((span - 105.0).abs() < 1e-9, "span was {span}");
        // A file the host has not met counts as zero-length, never a NaN.
        let none = a.span_s(|_| 0.0);
        assert!((none - 105.0).abs() < 1e-9, "the trimmed b.wav still ends at 105");
    }

    /// Keys this build does not know come back out unchanged — a newer
    /// build's song opened here is not stripped.
    #[test]
    fn a_newer_builds_keys_survive() {
        // **NOT `tempo_map` any more.** This test used that name as its
        // example of a key from the future, and version 5 made it a key from
        // the present — at which point the test would have gone on passing
        // while proving nothing at all, because `to_json` writes that key
        // itself. Picking a name no build has is the whole point of the
        // fixture.
        let text = r#"{"arrangement_version": 9, "clips": [],
                       "groove_map": [{"at": 0, "swing": 121}]}"#;
        let a = Arrangement::from_json(text);
        let out = a.to_json();
        assert!(
            out.contains("groove_map") && out.contains("121"),
            "a newer build's key was stripped: {out}"
        );
    }

    /// A clip's stretch survives the trip, and nonsense in the file does
    /// not reach the DSP.
    #[test]
    fn a_stretched_clip_round_trips_and_a_bad_ratio_does_not() {
        let doc = |body: &str| {
            Arrangement::from_json(&format!(
                r#"{{"arrangement_version": 9, "clips": [
                    {{"lane": 1, "path": "s.wav", "start_s": 0.0 {body}}}]}}"#
            ))
        };
        let a = doc(r#", "stretch": 1.5, "stretch_mode": "rhythmic""#);
        assert!((a.clips[0].stretch - 1.5).abs() < 1e-9);
        assert_eq!(a.clips[0].stretch_mode, "rhythmic");
        let back = Arrangement::from_json(&a.to_json());
        assert!((back.clips[0].stretch - 1.5).abs() < 1e-9);
        assert_eq!(back.clips[0].stretch_mode, "rhythmic");

        // **Clamped, and the clamp is a real one**: this number multiplies
        // into a buffer length, so a zero is an allocation of nothing and a
        // million is an allocation of everything.
        for bad in ["0", "-4", "1e9"] {
            let c = doc(&format!(r#", "stretch": {bad}"#));
            assert!(
                (c.clips[0].stretch - 1.0).abs() < 1e-9,
                "{bad} came through as {}",
                c.clips[0].stretch
            );
        }
        // A mode this build has never heard of is kept and reads as auto
        // downstream — not an error, and not a reason to lose the string.
        let odd = doc(r#", "stretch_mode": "spectral-9""#);
        assert_eq!(odd.clips[0].stretch_mode, "spectral-9");
        assert!(odd.to_json().contains("spectral-9"));
        // A clip nobody stretched writes neither key, so an untouched song
        // keeps its byte-for-byte shape.
        let plain = doc("");
        assert!(!plain.to_json().contains("stretch"), "{}", plain.to_json());
    }

    /// A newer build's keys ON A CLIP survive too — the half of the bargain
    /// the clips were left out of.
    ///
    /// This is the whole reason [`ArrClip::extra`] exists. Before it, a
    /// document whose clips carried a stretch ratio or a beat anchor opened
    /// here with the timeline perfectly intact and every clip quietly
    /// reverted, which is the worst shape a data-loss bug can take: nothing
    /// looks wrong until you go back to the build that wrote it.
    #[test]
    fn a_newer_builds_clip_keys_survive() {
        let a = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"lane": 1, "path": "media/solo.wav", "start_s": 4.0,
                 "stretch": {"ratio": 0.847, "mode": "wsola"},
                 "anchor": "beat"}]}"#,
        );
        assert_eq!(a.clips.len(), 1);
        let out = a.to_json();
        assert!(out.contains("0.847"), "the clip's stretch ratio was stripped: {out}");
        assert!(out.contains("wsola"), "the clip's stretch mode was stripped: {out}");
        assert!(out.contains(r#""anchor""#), "the clip's anchor was stripped: {out}");
        // And the whole document is still equal to itself across the trip,
        // which is what makes an older build safe to open a song in.
        assert_eq!(Arrangement::from_json(&out), a);
    }

    /// **A version-4 song opens at the tempo it was written at.**
    ///
    /// The document's first real migration arm, and the one that would have
    /// been noticed as "it sounds wrong" rather than reported as a bug: a v4
    /// file keeps its single tempo on the desk snapshot, so a v5 reader that
    /// just looked for `tempo_map` and found nothing would open every
    /// existing song at 120.
    #[test]
    fn a_version_four_song_keeps_the_tempo_it_was_written_at() {
        let v4 = r#"{"arrangement_version": 4,
                     "desk": {"tempo_bpm": 96.0, "sig_beats": 6, "sig_unit": 8},
                     "clips": [{"kind": "midi", "lane": 1, "path": "",
                                "start_s": 4.0, "len_s": 8.0}]}"#;
        let a = Arrangement::from_json(v4);
        assert!(a.tempo.is_constant(), "a v4 song gained a tempo map it never had");
        assert!(
            (a.tempo.bpm_at_beat(0.0) - 96.0).abs() < 1e-9,
            "opened at {} rather than 96",
            a.tempo.bpm_at_beat(0.0)
        );
        assert_eq!(
            a.meter.sig_at_bar(0),
            crate::recorder::TimeSignature { beats: 6, unit: 8 },
            "the signature did not come across"
        );
        // **And every clip stays TIME-anchored**, which is what v4 behaved
        // as. Making old MIDI musical on the way in would move notes in
        // songs that are already finished.
        assert_eq!(a.clips[0].anchor, Anchor::Time, "a v4 clip changed meaning");
        assert!(!a.clips[0].follow_tempo);
        assert_eq!(a.version, ARRANGEMENT_VERSION, "the stamp did not move up");
    }

    /// A song at one tempo writes no map at all.
    ///
    /// The shape every existing document has, kept: a file that says nothing
    /// about tempo is still readable by a build that predates the map, and
    /// its desk snapshot still carries the number.
    #[test]
    fn one_tempo_writes_no_map_and_a_real_one_round_trips() {
        let plain = Arrangement::default();
        let out = plain.to_json();
        assert!(!out.contains("tempo_map"), "a constant tempo wrote a map: {out}");
        assert!(!out.contains("meter_map"), "a constant meter wrote a map: {out}");

        let mut a = Arrangement::default();
        a.tempo = crate::tempo::TempoMap::sanitised(vec![
            crate::tempo::TempoPoint { at_beat: 0.0, bpm: 120.0, ramp: true },
            crate::tempo::TempoPoint { at_beat: 16.0, bpm: 72.0, ramp: false },
        ]);
        a.meter = crate::tempo::MeterMap::sanitised(vec![
            crate::tempo::MeterPoint {
                at_bar: 0,
                sig: crate::recorder::TimeSignature { beats: 4, unit: 4 },
            },
            crate::tempo::MeterPoint {
                at_bar: 8,
                sig: crate::recorder::TimeSignature { beats: 3, unit: 4 },
            },
        ]);
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back.tempo, a.tempo, "the tempo map did not survive");
        assert_eq!(back.meter, a.meter, "the meter map did not survive");
        // The ritardando's flag is part of it — a ramp read back as a step
        // is a different piece of music.
        assert!(back.tempo.points()[0].ramp, "the ramp was flattened");
    }

    /// Automation survives a round trip, and costs nothing without any.
    #[test]
    fn automation_round_trips_and_an_unautomated_song_writes_none() {
        use crate::automation::{AutoParam, Curve, Point, Shape};
        let plain = Arrangement::default();
        assert!(
            !plain.to_json().contains("automation"),
            "a song nobody automated wrote a curve"
        );

        let mut a = Arrangement::default();
        // A clip, so `next_id` has been handed out — an empty document keeps
        // 0 while a read one starts at 1, which is older behaviour and not
        // what this test is about.
        a.add_clip(a_clip(2, "/x/take.wav", 0.0));
        a.automation.set(Curve::sanitised(
            2,
            AutoParam::Gain,
            vec![
                Point { at_s: 0.0, value: 1.0, shape: Shape::Linear },
                Point { at_s: 8.0, value: 0.25, shape: Shape::Hold },
            ],
        ));
        a.automation.set(Curve::sanitised(
            2,
            AutoParam::Pan,
            vec![Point { at_s: 4.0, value: 0.0, shape: Shape::Linear }],
        ));
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back.automation, a.automation, "the curves did not survive");
        // The shape is part of the music: a hold read back as a ramp is a
        // level that slides where it was meant to step.
        assert_eq!(
            back.automation.get(2, AutoParam::Gain).expect("a curve").points()[1].shape,
            Shape::Hold,
            "the hold was flattened"
        );
        assert_eq!(back, a, "the document did not survive its own round trip");
    }

    /// A curve naming a channel this desk has not got is dropped.
    ///
    /// Clamping it onto the last channel would put somebody's fade on the
    /// wrong instrument, silently. Dropping it loses the curve and nothing
    /// else.
    #[test]
    fn a_curve_for_a_channel_that_does_not_exist_is_not_moved_onto_one_that_does() {
        let a = Arrangement::from_json(
            r#"{"arrangement_version": 5, "clips": [], "automation": [
                {"lane": 99, "param": "gain", "points": [{"at": 0, "v": 0.5}]},
                {"lane": 1, "param": "gain", "points": [{"at": 0, "v": 0.5}]},
                {"lane": 2, "param": "nonesuch", "points": [{"at": 0, "v": 0.5}]}]}"#,
        );
        assert_eq!(a.automation.curves().len(), 1, "{:?}", a.automation.curves());
        assert_eq!(a.automation.curves()[0].lane, crate::automation::AutoLane::Row(1));
        // A parameter from a newer build is dropped for the same reason: it
        // is better to lose one curve than to apply it to the wrong control.
        assert!(a.automation.get(2, crate::automation::AutoParam::Gain).is_none());
    }

    /// An anchor survives, and is only written when it says something.
    #[test]
    fn a_clips_anchor_survives_and_costs_nothing_when_ordinary() {
        let mut a = Arrangement::default();
        a.add_clip(ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Midi,
            anchor: Anchor::Beat,
            follow_tempo: true,
            stretch: 1.0,
            stretch_mode: String::new(),
            gain: 1.0,
            ..a_clip(1, "", 4.0)
        });
        a.add_clip(a_clip(2, "/x/take.wav", 8.0));
        let out = a.to_json();
        let back = Arrangement::from_json(&out);
        assert_eq!(back.clips[0].anchor, Anchor::Beat);
        assert!(back.clips[0].follow_tempo);
        assert_eq!(back.clips[1].anchor, Anchor::Time);
        assert!(!back.clips[1].follow_tempo);
        // The ordinary clip writes neither key: a document of plain audio
        // keeps the bytes it had at version 4.
        assert_eq!(out.matches("\"anchor\"").count(), 1, "an ordinary clip wrote an anchor");
        assert_eq!(out.matches("follow_tempo").count(), 1);
        assert_eq!(back, a, "the document did not survive its own round trip");
    }

    /// The version stamp only ever moves UP.
    ///
    /// A v9 document opened here and re-saved must still say 9. It says so
    /// while carrying v9's keys — see the test above — and a build that
    /// found "4" on that file would run its 4→9 migrations over data that
    /// had already been through them.
    #[test]
    fn the_version_stamp_never_goes_backwards() {
        let newer = Arrangement::from_json(r#"{"arrangement_version": 9, "clips": []}"#);
        assert_eq!(newer.version, 9);
        assert!(
            newer.to_json().contains(r#""arrangement_version": 9"#),
            "a newer build's document was downgraded: {}",
            newer.to_json()
        );
        // An OLDER document is upgraded, because reading it here is the
        // migration — that is the direction this is allowed to move.
        let older = Arrangement::from_json(r#"{"arrangement_version": 1, "clips": []}"#);
        assert_eq!(older.version, ARRANGEMENT_VERSION);
        // And a document with no stamp at all, which is what v0 files are.
        assert_eq!(Arrangement::from_json("{}").version, ARRANGEMENT_VERSION);
        // A document this build invented is at this build's version, never
        // zero — the reason `Default` is hand-written.
        assert_eq!(Arrangement::default().version, ARRANGEMENT_VERSION);
    }

    /// [`CLIP_KEYS`] names every key a clip writes, and nothing it does not.
    ///
    /// **The drift this catches is silent and self-inflicted.** A field
    /// added to `ArrClip` and to `to_json` but forgotten in `CLIP_KEYS` is
    /// not dropped — it comes back DOUBLED, once as the live field and once
    /// inside `extra`, and the extras merge then keeps writing a copy that
    /// stopped tracking the field the first time somebody edited it. Nothing
    /// fails, the file just grows a stale twin of a real value.
    #[test]
    fn a_clip_knows_which_of_its_keys_are_its_own() {
        let mut a = Arrangement::default();
        a.add_clip(ArrClip {
            kind: ClipKind::Midi,
            notes: vec![ArrNote { at_s: 0.5, dur_s: 1.0, key: 64, vel: 99, muted: false }],
            pedal: vec![ArrPedal { at_s: 0.5, cc: 64, value: 127 }],
            lane: 2,
            path: String::new(),
            start_s: 3.0,
            offset_s: 0.25,
            len_s: 9.0,
            loop_s: 0.0,
            fade_in_s: 0.1,
            fade_out_s: 0.2,
            name: "everything set".into(),
            id: 0,
            anchor: Anchor::Beat,
            follow_tempo: true,
            stretch: 0.75,
            stretch_mode: "tonal".into(),
            gain: 0.5,
            extra: Map::new(),
            warp: vec![(0, 0), (24_000, 25_000)],
            warp_fit: "slice".into(),
            warp_rate: 48_000,
        });
        let Ok(Value::Object(doc)) = serde_json::from_str::<Value>(&a.to_json()) else {
            panic!("the document did not parse as an object");
        };
        let Some(Value::Array(clips)) = doc.get("clips") else {
            panic!("no clips array");
        };
        let Some(Value::Object(c)) = clips.first() else {
            panic!("no clip");
        };
        for key in c.keys() {
            assert!(
                CLIP_KEYS.contains(&key.as_str()),
                "`{key}` is written by `to_json` and missing from CLIP_KEYS, so it \
                 will come back duplicated inside `extra`"
            );
        }
        // And nothing in the list is a name the writer never emits, which
        // would quietly make a real newer-build key look known and drop it.
        for known in CLIP_KEYS {
            assert!(
                c.contains_key(known),
                "CLIP_KEYS names `{known}`, which a fully-populated clip does not \
                 write — a newer build's `{known}` would be silently discarded"
            );
        }
        // The round trip leaves the bag empty, because every key was ours.
        assert!(
            Arrangement::from_json(&a.to_json()).clips[0].extra.is_empty(),
            "a clip claimed one of its own keys as a stranger's"
        );
    }

    /// **A quantize changed after a load is saved as changed.** `warp` and
    /// `warp_fit` were read but missing from [`CLIP_KEYS`], so every loaded
    /// clip also kept a copy of both in `extra`, and the writer's merge put
    /// the copy back wherever the live field writes nothing: a take
    /// re-quantized under the auto fit (which writes no fit) reopened under
    /// the fit it was loaded with, and a map taken off came back from the
    /// copy. Claiming the names must not eat a newer build's map in a shape
    /// this one cannot read, either.
    #[test]
    fn a_quantize_changed_after_a_load_is_saved_as_changed() {
        let mut a = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
                 "warp": [[0, 0], [24000, 22050]], "warp_fit": "slice",
                 "warp_rate": 48000}]}"#,
        );
        a.clips[0].warp_fit = "auto".to_owned();
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back.clips[0].warp_fit, "", "the fit the take was loaded with came back");
        a.clips[0].warp.clear();
        a.clips[0].warp_rate = 0;
        let back = Arrangement::from_json(&a.to_json());
        assert!(back.clips[0].warp.is_empty(), "a map taken off came back: {:?}", back.clips[0].warp);

        let newer = Arrangement::from_json(
            r#"{"arrangement_version": 9, "clips": [
                {"kind": "audio", "path": "/x/a.wav", "lane": 0, "start_s": 0.0,
                 "warp": {"segments": 3}, "warp_fit": 7}]}"#,
        );
        let out = newer.to_json();
        assert!(
            out.contains(r#""segments": 3"#) && out.contains(r#""warp_fit": 7"#),
            "a newer build's quantize was eaten: {out}"
        );
    }

    /// **The pedal survives the document**, values and all.
    ///
    /// And a document written before it existed reads back with an empty
    /// list rather than a guess — an unpedalled part and a part whose pedal
    /// was never recorded look the same on disk, and inventing a damper for
    /// the second would change how the first sounds.
    #[test]
    fn a_clips_pedal_makes_the_round_trip() {
        let mut a = Arrangement::default();
        a.add_clip(ArrClip {
            kind: ClipKind::Midi,
            notes: vec![ArrNote { at_s: 0.0, dur_s: 0.25, key: 60, vel: 90, muted: false }],
            // Half-pedalling: the values are the point, not a boolean at 64.
            pedal: vec![
                ArrPedal { at_s: 0.0, cc: 64, value: 127 },
                ArrPedal { at_s: 1.0, cc: 64, value: 40 },
                ArrPedal { at_s: 2.0, cc: 66, value: 100 },
            ],
            lane: 1,
            len_s: 4.0,
            ..Default::default()
        });
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back.clips[0].pedal, a.clips[0].pedal);
        assert!(back.clips[0].extra.is_empty(), "the pedal read as a stranger");

        // A document from before the pedal existed.
        let older = r#"{"version":3,"clips":[{"kind":"midi","lane":0,"start_s":0.0,"len_s":4.0,"notes":[]}] }"#;
        assert!(Arrangement::from_json(older).clips[0].pedal.is_empty());

        // And a clip with no pedal writes no key, so every document written
        // before this is still byte-identical through a round trip.
        let mut plain = Arrangement::default();
        plain.add_clip(ArrClip { kind: ClipKind::Midi, len_s: 1.0, ..Default::default() });
        assert!(!plain.to_json().contains("pedal"));
    }

    /// **Every pass of a loop restates the pedal it opens with.**
    ///
    /// A clip whose pedal goes down halfway through and is never lifted holds
    /// the damper across the seam when it plays, so a flatten that emitted
    /// only the literal edges would come out drier than the clip it
    /// flattened: the second pass would open with the pedal up because
    /// nothing said otherwise.
    ///
    /// And a level that has not changed is not restated — a pedal held
    /// across four passes is one press, not four, and one lift at the
    /// clip's end.
    #[test]
    fn each_pass_of_a_loop_opens_with_the_pedal_it_inherits() {
        let clip = ArrClip {
            kind: ClipKind::Midi,
            pedal: vec![ArrPedal { at_s: 0.5, cc: 64, value: 127 }],
            start_s: 10.0,
            len_s: 3.0,
            loop_s: 1.0,
            ..Default::default()
        };
        let out = clip_pedal_in_song_time(&clip);
        let at: Vec<f64> = out.iter().map(|e| e.at_s).collect();
        assert_eq!(
            at,
            vec![10.5, 13.0],
            "the pedal was restated on a pass that already held it: {out:?}"
        );

        // The same clip, LIFTED at the end of its pass: now every pass has to
        // press it again, because every pass ends with it up.
        let mut chopped = clip.clone();
        chopped.pedal.push(ArrPedal { at_s: 0.75, cc: 64, value: 0 });
        let out = clip_pedal_in_song_time(&chopped);
        let at: Vec<(f64, u8)> = out.iter().map(|e| (e.at_s, e.value)).collect();
        assert_eq!(
            at,
            vec![(10.5, 127), (10.75, 0), (11.5, 127), (11.75, 0), (12.5, 127), (12.75, 0)],
            "{out:?}"
        );
    }

    /// **The pedal comes up where the clip ends, as playback lets it go.**
    /// The engine reads a clip's pedal only inside the clip, so at its end
    /// the pedal is up whatever the clip's last edge said. The flattened
    /// list stated no lift there, and dropped an edge past the end only on
    /// a loop: a loop whose pedal is down at its end held it down past the
    /// clip in a glue or a MIDI export (for the rest of an exported file,
    /// when nothing lifted it), and on a clip that does not loop a lift a
    /// right trim hides came up late, and a press it hides was written as
    /// if it were heard.
    #[test]
    fn the_pedal_comes_up_where_the_clip_ends() {
        let p = |at_s: f64, cc: u8, value: u8| ArrPedal { at_s, cc, value };
        let edges = |clip: &ArrClip| -> Vec<(f64, u8, u8)> {
            clip_pedal_in_song_time(clip).iter().map(|e| (e.at_s, e.cc, e.value)).collect()
        };
        // A loop whose damper goes down halfway through and never comes up.
        let held = ArrClip {
            kind: ClipKind::Midi,
            pedal: vec![p(0.5, 64, 127)],
            start_s: 10.0,
            len_s: 3.0,
            loop_s: 1.0,
            ..Default::default()
        };
        assert_eq!(
            edges(&held),
            vec![(10.5, 64, 127), (13.0, 64, 0)],
            "a loop held its pedal down past its end"
        );
        // No loop, trimmed on the right to two seconds: the damper's lift at
        // 2.5 is hidden, so it comes up at the clip's end, and the
        // sostenuto pressed at 2.75 is never heard and never written.
        let trimmed = ArrClip {
            kind: ClipKind::Midi,
            pedal: vec![p(0.5, 64, 127), p(2.5, 64, 0), p(2.75, 66, 127)],
            start_s: 10.0,
            len_s: 2.0,
            ..Default::default()
        };
        assert_eq!(
            edges(&trimmed),
            vec![(10.5, 64, 127), (12.0, 64, 0)],
            "the edges a right trim hides were written instead of a lift at the clip's end"
        );
        // A pedal already up at the end states nothing more.
        let lifted = ArrClip { pedal: vec![p(0.5, 64, 127), p(1.5, 64, 0)], ..trimmed.clone() };
        assert_eq!(edges(&lifted), vec![(10.5, 64, 127), (11.5, 64, 0)]);
    }

    /// **A lane's clips hold the pedal as playback combines them: at the
    /// higher level where they overlap, and up only where none holds it.**
    /// The engine reads a lane's pedal as the highest level among the clips
    /// the playhead stands inside (`Renderer::clip_pedal_level`). Glue and
    /// the MIDI export laid each clip's own edges side by side instead, so
    /// one clip's lift at its end cut a pedal an overlapping clip still
    /// held, and a lower level pressed under a held one lowered it. Where
    /// the clips do not overlap, each still lets the pedal up at its own
    /// end, and the clips' order does not matter.
    #[test]
    fn overlapping_clips_hold_the_pedal_at_the_higher_level() {
        let p = |at_s: f64, cc: u8, value: u8| ArrPedal { at_s, cc, value };
        let clip = |start_s: f64, len_s: f64, pedal: Vec<ArrPedal>| ArrClip {
            kind: ClipKind::Midi,
            start_s,
            len_s,
            pedal,
            ..Default::default()
        };
        let edges = |clips: &[ArrClip]| -> Vec<(f64, u8, u8)> {
            lane_pedal_in_song_time(clips).iter().map(|e| (e.at_s, e.cc, e.value)).collect()
        };
        // The first holds the damper to its end; the second, over its second
        // half, holds a half pedal from its own start and a sostenuto.
        let first = clip(0.0, 2.0, vec![p(0.25, 64, 127)]);
        let over = clip(1.0, 2.0, vec![p(0.0, 64, 40), p(0.5, 66, 100)]);
        assert_eq!(
            edges(&[over, first.clone()]),
            vec![(0.25, 64, 127), (1.5, 66, 100), (2.0, 64, 40), (3.0, 64, 0), (3.0, 66, 0)],
            "the damper did not stand at the higher level where the clips overlap"
        );
        // A clip inside a held one, with a lower press and a lift of its
        // own, moves nothing.
        let held = clip(0.0, 4.0, vec![p(0.0, 64, 127)]);
        let inside = clip(1.0, 1.0, vec![p(0.5, 64, 100), p(0.75, 64, 0)]);
        assert_eq!(
            edges(&[held, inside]),
            vec![(0.0, 64, 127), (4.0, 64, 0)],
            "a clip inside a held pedal lowered or lifted it"
        );
        // Back to back, the second pressing on the instant the first lets
        // go: playback sends nothing there, so nothing is written there.
        let short = clip(0.0, 1.0, vec![p(0.25, 64, 127)]);
        let next = clip(1.0, 1.0, vec![p(0.0, 64, 127)]);
        assert_eq!(
            edges(&[next, short.clone()]),
            vec![(0.25, 64, 127), (2.0, 64, 0)],
            "a lift and a press were written where playback holds the pedal"
        );
        // Apart: each lets the pedal up at its own end, as one clip does.
        let later = clip(2.0, 1.0, vec![p(0.25, 64, 127)]);
        assert_eq!(
            edges(&[later, short]),
            vec![(0.25, 64, 127), (1.0, 64, 0), (2.25, 64, 127), (3.0, 64, 0)]
        );
    }

    /// Garbage is an empty document, never a panic and never a half-read.
    #[test]
    fn garbage_is_an_empty_arrangement() {
        for text in ["", "not json", "[1,2,3]", r#"{"clips": "no"}"#] {
            let a = Arrangement::from_json(text);
            assert!(a.clips.is_empty(), "{text:?} produced clips");
        }
        // A clip missing its path is skipped; a negative start is zeroed.
        let a = Arrangement::from_json(
            r#"{"clips": [{"lane": 1}, {"path": "/x.wav", "start_s": -4.0}]}"#,
        );
        assert_eq!(a.clips.len(), 1);
        assert_eq!(a.clips[0].start_s, 0.0);
    }

    /// A document with something in every field this build writes — the
    /// subject of the fingerprint tests below. Everything in it is inside
    /// the ranges the READER accepts, so it is a fixed point of a save and a
    /// load, which those tests lean on.
    fn a_document_with_everything_in_it() -> Arrangement {
        let mut a = Arrangement::default();
        a.version = ARRANGEMENT_VERSION;
        a.named = true;
        // **The rows**, the shipped thirty-two under the identity ids, the
        // first three carrying every row fact said (W6: the desk's own form,
        // where it was a set of ragged arrays a legacy writer padded).
        let rows = {
            use crate::desk_id::{Node, TrackId};
            use crate::desk_model::{Dx7, Frozen, RowKind, Send, TrackRow};
            let mut rows: Vec<TrackRow> = (0..32).map(|r| TrackRow::vacant(TrackId(r))).collect();
            rows[0].kind = Some(RowKind::Midi);
            rows[1].kind = Some(RowKind::Audio);
            rows[1].icon = 3;
            (rows[0].name, rows[2].name) = ("drums".into(), "gtr".into());
            (rows[0].color, rows[2].color) = (3, 11);
            rows[1].sends = vec![Send { to: Node::Fx, level: 0.25, pre: false }];
            rows[1].mute = true;
            rows[2].solo = true;
            (rows[1].gain, rows[1].pan) = (0.5, -0.5);
            rows[0].input = "midi:Keystation 88".into();
            rows[0].midi = crate::recorder::MidiInputSel { devices: vec!["Keystation 88".into()], channel: Some(2) };
            for row in &mut rows[..3] {
                row.added = true;
            }
            rows[0].folder = true;
            rows[1].in_folder = true;
            (rows[0].arm, rows[2].arm) = (true, true);
            rows[1].phase = true;
            (rows[0].theory, rows[2].theory) = (true, true);
            rows[0].dx7 = Dx7 { cart: "ROM1A.syx".into(), patch: 7 };
            (rows[0].keys.patch, rows[1].keys.patch) = (2, 3);
            (rows[0].keys.params[0], rows[0].keys.params[1]) = (0.25, 0.5);
            rows[1].frozen = Some(Frozen { path: "media/frozen-02-1.wav".into(), len_s: 12.5 });
            rows
        };
        a.desk = Some(DeskSnap {
            rows,
            row_says: Says::ROWS,
            // By identity, on the identity rows: row 0's bay 1, and row 16's
            // second rack (the far seat 4149 of the frozen map).
            plugins: [(1, "reverb"), (4149 * 3, "brusfri")]
                .into_iter()
                .map(|(flat, p)| {
                    let ids = crate::desk_legacy::identity_ids(32);
                    (crate::desk_legacy::bay_of_flat(&ids, flat).expect("a bay"), p.to_owned())
                })
                .collect(),
            drums_gain: 0.9,
            drums_pan: 0.1,
            metronome_gain: 0.7,
            master_gain: 1.0,
            fx_return: 0.8,
            tempo_bpm: 96.0,
            sig_beats: 3,
            sig_unit: 4,
            metronome_on: true,
            // The late fields, every one of them SAID — this fixture is the
            // round-trip's subject and a `None` here would test the absent
            // road twice and the present one never.
            routes: Some(crate::routing::Routes(vec![crate::routing::Route {
                from: crate::desk_id::Node::Track(crate::desk_id::TrackId(1)),
                target: crate::routing::Target::Hardware { first: 2, stereo: true },
                gain: 0.8,
            }])),
            insert_keys: Some(crate::routing::KeyEdges(vec![crate::routing::KeyEdge {
                seat: crate::desk_id::slot_key(crate::desk_id::Node::Track(crate::desk_id::TrackId(3)), 1)
                    .expect("a bay"),
                from: crate::desk_id::Node::Track(crate::desk_id::TrackId(0)),
            }])),
            bypass: Some(std::collections::BTreeSet::from([crate::desk_id::SeatBay::new(
                crate::desk_id::Node::Track(crate::desk_id::TrackId(4)),
                crate::desk_id::Rack::Chain(0),
                2,
            )])),
            // The second late wave, every one of them SAID, for the reason
            // the six above are: this fixture is the round-trip's subject,
            // and a `None` here would test the absent road twice and the
            // present one never.
            rack: Some(Rack {
                reverb: 0.3,
                delay: 0.15,
                chorus: 0.0,
                phaser: 0.05,
                hpf: 0.1,
                lpf: 0.9,
                gain: 0.2,
                limiter: 0.8,
                params: {
                    let mut p = Map::new();
                    p.insert("reverb_size".into(), Value::from(0.6));
                    p.insert("delay_div".into(), Value::from("1/8"));
                    p
                },
            }),
            track_gain: Some(0.75),
            master_mono: Some(true),
            lufs_window: Some(crate::recorder::LoudnessWindow::ShortTerm),
            varispeed: Some(crate::varispeed::Varispeed::with_range(80, 130)),
            // Off its default in all three, so a reader that dropped it, or
            // either time, writes other bytes than this fixture.
            start_stop: Some(crate::motor::StartStop::new(true, 1.5, 3.0)),
            click: Some(Click {
                div: 2,
                in_take: true,
                sample: "clave.wav".into(),
            }),
            transpose: Some(-3),
            count_in: Some(CountIn { bars: 2, beats: 8, in_take: true }),
            // **NOT the defaults**, deliberately: this fixture exists to
            // prove a round trip, and a field left at its default round-trips
            // whether the writer carries it or not.
            view: Some(View {
                snap: false,
                grid_exp: -5,
                tempo_lane: true,
                master_row: false,
                header_w: 173.5,
                visible: 9,
                quantize_grid_exp: -4,
                quantize_triplet: true,
                quantize_swing_pct: 62.5,
                quantize_strength_pct: 72.0,
                quantize_window_ms: 35.0,
                visible_at: 918.0,
                quantize_sensitivity: 0.7,
                quantize_fit: "slice".into(),
            }),
            look: Some(Look {
                theme: "Rhodes".into(),
                palette: 6,
                palette_auto: false,
                view: "arrange".into(),
            }),
            take_name: Some("nocturne".into()),
            feel: Some(Feel {
                snap: true,
                grid_exp: -4,
                swing: true,
                swing_pct: 66.7,
                triplet: true,
                humanize_timing: 30,
                humanize_vel: 12,
            }),
            notation: Some(Notation {
                staff_key: -3,
                prefer_flats: true,
                capo: 2,
                capo_style: "silver".into(),
                tuning: "Drop D".into(),
            }),
            // Bookkeeping: the reader's notes compare equal whatever they say.
            next_id: Aside(32),
            legacy_states: Aside(None),
            dropped: Aside(Vec::new()),
            viewport: Aside(Some(Viewport { start_s: 12.5, len_s: 96.0 })),
            // Every special with something said (W5), each fact one a
            // format-2 desk keeps: the drum bus's send is its effects knob.
            specials: {
                use crate::desk_model::{Send, Special, Specials};
                use crate::desk_id::Node;
                Specials {
                    drums: Special {
                        name: "KIT".into(),
                        color: 9,
                        sends: vec![Send { to: Node::Fx, level: 0.4, pre: false }],
                        ..Special::default()
                    },
                    click: Special { mute: true, ..Special::default() },
                    fx: Special { solo: true, color: 2, ..Special::default() },
                    master: Special { name: "OUT".into(), color: 5, phase: true, ..Special::default() },
                }
            },
        });
        let mut extra = Map::new();
        extra.insert("a_newer_builds_key".into(), Value::from("kept"));
        a.add_clip(ArrClip {
            pedal: vec![
                ArrPedal { at_s: 0.5, cc: 64, value: 127 },
                ArrPedal { at_s: 2.0, cc: 64, value: 0 },
            ],
            kind: ClipKind::Midi,
            notes: vec![
                ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100, muted: false },
                ArrNote { at_s: 1.5, dur_s: 0.25, key: 64, vel: 33, muted: false },
            ],
            lane: 2,
            path: String::new(),
            start_s: 4.0,
            offset_s: 0.5,
            len_s: 8.0,
            loop_s: 2.0,
            fade_in_s: 0.1,
            fade_out_s: 0.2,
            name: "a part".into(),
            id: 0,
            anchor: Anchor::Beat,
            follow_tempo: true,
            stretch: 1.5,
            stretch_mode: "tonal".into(),
            gain: 0.75,
            extra,
            warp: Vec::new(),
            warp_fit: String::new(),
            warp_rate: 0,
        });
        a.add_clip(ArrClip {
            pedal: Vec::new(),
            kind: ClipKind::Audio,
            lane: 0,
            path: "media/take 1.wav".into(),
            name: "take 1".into(),
            start_s: 0.0,
            len_s: 12.0,
            ..Default::default()
        });
        a.tempo = crate::tempo::TempoMap::sanitised(vec![
            crate::tempo::TempoPoint { at_beat: 0.0, bpm: 96.0, ramp: true },
            crate::tempo::TempoPoint { at_beat: 16.0, bpm: 120.0, ramp: false },
        ]);
        a.meter = crate::tempo::MeterMap::sanitised(vec![
            crate::tempo::MeterPoint {
                at_bar: 0,
                sig: crate::recorder::TimeSignature { beats: 3, unit: 4 },
            },
            crate::tempo::MeterPoint {
                at_bar: 8,
                sig: crate::recorder::TimeSignature { beats: 7, unit: 8 },
            },
        ]);
        a.automation = crate::automation::Automation::from_curves(vec![
            crate::automation::Curve::sanitised(
                1,
                crate::automation::AutoParam::Gain,
                vec![
                    crate::automation::Point {
                        at_s: 0.0,
                        value: 0.5,
                        shape: crate::automation::Shape::Linear,
                    },
                    crate::automation::Point {
                        at_s: 4.0,
                        value: 1.0,
                        shape: crate::automation::Shape::Hold,
                    },
                ],
            ),
            {
                let mut kept = crate::automation::Curve::sanitised(
                    2,
                    crate::automation::AutoParam::Plugin { bay: 1, id: 77 },
                    vec![crate::automation::Point {
                        at_s: 2.0,
                        value: 0.25,
                        shape: crate::automation::Shape::Linear,
                    }],
                );
                // Waiting for its plugin to come back - see `Curve::tenant`.
                kept.tenant = "@tangent-eq".into();
                kept
            },
        ]);
        a.markers.push(ArrMarker { at_s: 12.5, name: "Verse".into() });
        a.extra.insert("song_of_the_future".into(), Value::from(9));
        a
    }

    /// The recorded walk — the exact form undo compares. See the
    /// `Fingerprint` doc for why equal words mean equal text.
    fn words(a: &Arrangement) -> Vec<u64> {
        let mut f = Fingerprint::recording(0);
        a.hash_into(a.desk.as_ref(), &mut f);
        f.finish_recorded().0
    }

    /// The edit moved the document, and the fingerprint saw it — and so
    /// did the words, which is the stronger claim undo now rests on.
    fn moves_both(name: &str, edit: impl Fn(&mut Arrangement)) {
        let a = a_document_with_everything_in_it();
        let (json, fp, w) = (a.to_json(), a.fingerprint(), words(&a));
        let mut b = a;
        edit(&mut b);
        assert_ne!(b.to_json(), json, "`{name}` did not change the document at all");
        assert_ne!(
            b.fingerprint(),
            fp,
            "`{name}` changed the document and the fingerprint did not move"
        );
        assert_ne!(words(&b), w, "`{name}` changed the document and the words did not");
    }

    /// The edit did NOT move the document, so the fingerprint must not move
    /// either — a fingerprint that jumps where the file does not is an
    /// asterisk over work nobody did, and an undo moment for it.
    fn moves_neither(name: &str, base: Arrangement, edit: impl Fn(&mut Arrangement)) {
        let (json, fp, w) = (base.to_json(), base.fingerprint(), words(&base));
        let mut b = base;
        edit(&mut b);
        assert_eq!(b.to_json(), json, "`{name}` moved the document after all");
        assert_eq!(
            b.fingerprint(),
            fp,
            "`{name}` left the document alone and the fingerprint moved"
        );
        assert_eq!(words(&b), w, "`{name}` left the document alone and the words moved");
    }

    /// **A document from before the arm was written down opens as it always
    /// did**, and does not come up wearing an asterisk for it.
    ///
    /// The shipped rule is the answer for a desk that has never said — the
    /// channel holding the built-in DX7 live, and nothing else — and it is
    /// derived from the document's OWN kinds and bays. Defaulting to zero
    /// would silence every old project; keeping whatever the session had
    /// would be the bleed the folder fields were written to end.
    ///
    /// And a desk that says it is SILENT stays silent, which is why the key
    /// is written even when it is zero: `Some(0)` is an opinion.
    #[test]
    fn a_desk_with_no_arm_key_falls_back_to_the_shipped_rule() {
        let mut snap = a_document_with_everything_in_it().desk.take().unwrap();
        // The piano's channel, the way the shipped desk has it: a MIDI kind
        // with the built-in in bay 0.
        let seat = crate::desk_legacy::encode_v32(
            crate::desk_legacy::LegacyOwner::Row(1),
            crate::desk_id::Rack::Chain(0),
            0,
        )
        .expect("row 1's first bay");
        for (n, row) in snap.rows.iter_mut().enumerate() {
            row.kind = (n == 1).then_some(crate::desk_model::RowKind::Midi);
        }
        snap.set_word(|r| &mut r.arm, 1 << 1);

        // A document from before the arm was written down is a LEGACY one:
        // a format-2 desk says every row's arm. **Either shipped piano** in
        // that seat: the DX7, which every such document holds there, and the
        // Keys, the other one the rule names (`dialogs::SHIPPED_PIANOS`).
        for piano in crate::dialogs::SHIPPED_PIANOS {
            snap.set_bay(seat, piano);
            let mut value = snap.to_value_as(crate::desk_format::Format::Legacy);
            let obj = value.as_object_mut().expect("a desk object");
            assert!(obj.remove("armed").is_some(), "the key was never written");
            let back = DeskSnap::from_value(&value).ok().flatten().expect("the desk should read");
            assert_eq!(
                back.word(|r| r.arm),
                1 << 1,
                "a document with {piano} and no arm key did not fall back to the shipped rule"
            );
        }
        snap.set_bay(seat, crate::dialogs::BUILTIN_PATH);

        // Written, and read back as itself — including the silent desk.
        for mask in [0u64, 1 << 1, 0b1010] {
            snap.set_word(|r| &mut r.arm, mask);
            let read = DeskSnap::from_value(&snap.to_value()).ok().flatten().expect("the desk should read");
            assert_eq!(read.word(|r| r.arm), mask, "the arm mask did not round trip");
        }
    }

    /// **A start and stop that is not an object says nothing.**
    ///
    /// The desk's `start_stop` is an object of the switch and its two times
    /// (`StartStop::to_json`), read member by member through the model's
    /// setters, so a member that is missing or of the wrong type reads as
    /// its default. A value that is not an object at all (a hand-edited 5,
    /// a `true`, a list, a null, a word) would read the same way if it
    /// reached the model: the shipped setting, off, and opening the file
    /// would switch a live desk's tape start and stop off. So it never
    /// reaches it: the desk reads as one that does not say (`None`), and
    /// the live switch stands (`apply_desk_snapshot`).
    #[test]
    fn a_start_and_stop_that_is_not_an_object_says_nothing() {
        let snap = a_document_with_everything_in_it().desk.take().unwrap();
        assert!(snap.start_stop.is_some_and(|m| m.on()), "the fixture's desk does not say, so nothing is proved");
        for odd in [serde_json::json!(5), serde_json::json!(true), serde_json::json!([1, 2]), Value::Null, serde_json::json!("on")] {
            let mut value = snap.to_value();
            value.as_object_mut().expect("a desk object").insert("start_stop".into(), odd.clone());
            let back = DeskSnap::from_value(&value).ok().flatten().expect("the desk should read");
            assert_eq!(back.start_stop, None, "a start_stop of {odd} was read as {:?}", back.start_stop);
        }
    }

    /// **A trimmed loop fills its whole length**, tail included.
    ///
    /// The unroll places each note at `at_s - offset_s + shift`, so a clip
    /// whose front has been trimmed starts mid-loop. The pass count was
    /// `len_s / loop_s`, which then runs out exactly `offset_s` before the
    /// clip ends — trim half a loop off the front and the last half of the
    /// clip holds no notes at all. The owner: "midi loops that are clipped
    /// leave a latter half that is empty."
    ///
    /// Revert-check performed: dropping `+ offset_s` from the pass count
    /// leaves the second half empty and fails the tail assertion.
    #[test]
    fn a_loop_trimmed_from_its_front_still_fills_its_tail() {
        // One note at the top of a four-second loop, trimmed two seconds in,
        // eight seconds long: notes are due at 2, 6 — and, before the fix,
        // the walk stopped after the pass that lands at 2.
        let clip = ArrClip {
            kind: ClipKind::Midi,
            notes: vec![ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100, muted: false }],
            start_s: 10.0,
            offset_s: 2.0,
            len_s: 8.0,
            loop_s: 4.0,
            ..Default::default()
        };
        let at: Vec<f64> = clip_notes_in_song_time(&clip)
            .into_iter()
            .map(|n| n.at_s - clip.start_s)
            .collect();
        assert_eq!(at.len(), 2, "the trimmed loop played {at:?}");
        assert!((at[0] - 2.0).abs() < 1e-9, "{at:?}");
        assert!(
            (at[1] - 6.0).abs() < 1e-9,
            "the clip's last four seconds hold no notes: {at:? }"
        );

        // Nothing runs PAST the clip: the `at >= len_s` skip still holds.
        // Notes fall at 4k - 2, so eleven seconds holds 2, 6 and 10 and the
        // pass due at 14 is not played.
        let long = ArrClip { len_s: 11.0, ..clip.clone() };
        let at: Vec<f64> = clip_notes_in_song_time(&long)
            .into_iter()
            .map(|n| n.at_s - long.start_s)
            .collect();
        assert_eq!(at.len(), 3, "{at:?}");
        assert!(at.iter().all(|a| *a < long.len_s), "a pass ran past the clip: {at:?}");

        // An untrimmed loop is unchanged — the arithmetic adds nothing when
        // there is no offset to cover.
        let flush = ArrClip { offset_s: 0.0, len_s: 8.0, ..clip.clone() };
        let at: Vec<f64> = clip_notes_in_song_time(&flush)
            .into_iter()
            .map(|n| n.at_s - flush.start_s)
            .collect();
        assert_eq!(at, vec![0.0, 4.0], "an untrimmed loop changed: {at:?}");
    }

    /// **A clip schedules only what the engine sounds, looped or not.** The
    /// engine walls every clip at its end (`PlacedMidi::len`): a note that
    /// begins at or past it never sounds, and one that rings past it is let
    /// go there. The expansion skipped a note past the end only on a loop
    /// and rang every note its whole length, so the MIDI export wrote notes
    /// a right trim hides, and glue welded them into a clip long enough to
    /// play them, where they began to sound.
    #[test]
    fn a_clip_schedules_only_what_the_engine_sounds() {
        let n = |at_s: f64, dur_s: f64, key: u8| ArrNote { at_s, dur_s, key, vel: 100, muted: false };
        // No loop, trimmed on the right to two seconds: the note at 1.5
        // rings a second and is let go at the end; the one at 3.0 is hidden.
        let clip = ArrClip {
            kind: ClipKind::Midi,
            notes: vec![n(0.0, 0.5, 60), n(1.5, 1.0, 62), n(3.0, 0.5, 64)],
            start_s: 10.0,
            len_s: 2.0,
            loop_s: 0.0,
            ..Default::default()
        };
        let heard = |clip: &ArrClip| -> Vec<(f64, f64, u8)> {
            clip_notes_in_song_time(clip)
                .into_iter()
                .map(|n| (n.at_s - clip.start_s, n.dur_s, n.key))
                .collect()
        };
        let once = heard(&clip);
        assert_eq!(
            once.iter().map(|h| h.2).collect::<Vec<_>>(),
            vec![60, 62],
            "a note past the end of a clip that does not loop was scheduled: {once:?}"
        );
        assert!((once[1].1 - 0.5).abs() < 1e-9, "a note rang past the clip's end: {once:?}");
        assert!((once[0].1 - 0.5).abs() < 1e-9, "a note inside the clip was cut: {once:?}");
        // A loop is let go at the CLIP's end, not at a pass's: the 62 of
        // the first pass rings on into the second, the second pass's is cut.
        let looped = ArrClip {
            notes: vec![n(0.0, 0.25, 60), n(0.75, 1.0, 62)],
            loop_s: 1.0,
            ..clip.clone()
        };
        let passes = heard(&looped);
        assert_eq!(passes.len(), 4, "{passes:?}");
        assert!((passes[1].1 - 1.0).abs() < 1e-9, "a note was cut at a pass's end: {passes:?}");
        assert!((passes[3].1 - 0.25).abs() < 1e-9, "the last pass rang past the clip's end: {passes:?}");
    }

    /// **A clip repeats when the picture shows a seam inside it** - glue's
    /// "one clip is enough" and the arrangement's notch ask one rule. A MIDI
    /// clip trimmed into its loop turns over `offset mod loop` sooner, so it
    /// can repeat inside a span no longer than one loop; an audio clip plays
    /// the same trimmed seconds every pass and turns over a whole loop in.
    #[test]
    fn a_clip_repeats_where_its_seam_falls() {
        let midi = ArrClip { kind: ClipKind::Midi, len_s: 2.0, loop_s: 2.0, offset_s: 0.5, ..Default::default() };
        assert!(midi.repeats(), "a trimmed MIDI loop turns over 1.5 s in, inside its 2 s");
        assert_eq!(first_pass_s(true, 2.0, 2.0, 0.5), Some(1.5));
        let audio = ArrClip { kind: ClipKind::Audio, ..midi.clone() };
        assert!(!audio.repeats(), "an audio clip one loop long plays one pass");
        let untrimmed = ArrClip { offset_s: 0.0, ..midi.clone() };
        assert!(!untrimmed.repeats(), "one untrimmed loop's worth is one pass");
        assert!(ArrClip { len_s: 4.5, ..untrimmed.clone() }.repeats());
        assert!(!ArrClip { loop_s: 0.0, ..midi }.repeats(), "a clip with no loop never repeats");
    }

    /// **The fold is the schedule, backwards.** Every note the song plays out
    /// of a looping MIDI clip folds onto the spot the roll draws that note,
    /// untrimmed and trimmed by less or more than a loop - which is what puts
    /// the editor's playhead on the note that is sounding, pass after pass.
    ///
    /// And nothing folds outside the clip, or on a clip that does not loop:
    /// there is no pass to be in, and the song's second is the truth.
    #[test]
    fn a_played_note_folds_onto_where_the_roll_draws_it() {
        let notes: Vec<ArrNote> = [0.0, 0.25, 0.5, 0.9]
            .iter()
            .enumerate()
            .map(|(i, &at)| ArrNote { at_s: at, dur_s: 0.1, key: 60 + i as u8, vel: 100, muted: false })
            .collect();
        for offset in [0.0, 0.3, 1.0, 2.7] {
            let clip = ArrClip {
                kind: ClipKind::Midi,
                notes: notes.clone(),
                start_s: 5.0,
                offset_s: offset,
                len_s: 7.0,
                loop_s: 1.0,
                ..Default::default()
            };
            let played = clip_notes_in_song_time(&clip);
            assert!(played.len() >= 20, "offset {offset}: only {} notes played", played.len());
            for p in played {
                let src = clip.notes.iter().find(|n| n.key == p.key).expect("a played note has a source");
                let drawn = clip.start_s + (src.at_s - clip.offset_s);
                let folded = clip.midi_fold_s(p.at_s);
                assert!(
                    (folded - drawn).abs() < 1.0e-9,
                    "offset {offset}: key {} played at {} folded to {folded}, drawn at {drawn}",
                    p.key,
                    p.at_s
                );
            }
            // Before the clip and past its end, the song's own second.
            for t in [4.0, 12.0, 13.5] {
                assert_eq!(clip.midi_fold_s(t), t, "offset {offset}: folded {t} outside the clip");
            }
        }
        let once = ArrClip { kind: ClipKind::Midi, start_s: 5.0, len_s: 7.0, loop_s: 0.0, ..Default::default() };
        assert_eq!(once.midi_fold_s(9.5), 9.5, "a clip that does not loop was folded");
    }

    /// **Every meaningful change moves the fingerprint.**
    ///
    /// The list is one entry per thing `to_json` writes, because the
    /// fingerprint is a hand-written mirror of that function and the only
    /// thing stopping the two drifting apart is this test. A field added to
    /// the writer and forgotten in `Arrangement::hash_into` fails here.
    #[test]
    fn the_fingerprint_moves_when_the_document_moves() {
        moves_both("the version stamp", |a| a.version += 1);
        moves_both("the id counter, above the clips", |a| a.next_id += 50);
        moves_both("the desk going away", |a| a.desk = None);
        // **Every fact of a row on its own line** (W6: the rows are the
        // desk), each one a field `hash_row` walks.
        fn row(a: &mut Arrangement, n: usize) -> &mut crate::desk_model::TrackRow {
            &mut a.desk.as_mut().unwrap().rows[n]
        }
        fn unsay(a: &mut Arrangement, what: Says) {
            let d = a.desk.as_mut().unwrap();
            d.row_says = Says(d.row_says.0 & !what.0);
        }
        moves_both("a channel kind", |a| row(a, 0).kind = Some(crate::desk_model::RowKind::Audio));
        moves_both("a channel badge", |a| row(a, 1).icon = 5);
        moves_both("a strip name", |a| row(a, 0).name = "kit".into());
        moves_both("a strip colour", |a| row(a, 2).color = 4);
        moves_both("a plugin in a bay", |a| {
            a.desk.as_mut().unwrap().set_bay(1, "eq");
        });
        moves_both("a plugin in a far seat", |a| {
            a.desk.as_mut().unwrap().set_bay(4149 * 3, "other");
        });
        moves_both("a far seat emptied", |a| {
            a.desk.as_mut().unwrap().set_bay(4149 * 3, "");
        });
        moves_both("a seat arriving in the head", |a| {
            a.desk.as_mut().unwrap().set_bay(0, "eq");
        });
        moves_both("a send", |a| crate::desk_model::set_fx_send_level(&mut row(a, 1).sends, 0.3));
        moves_both("a mute", |a| row(a, 0).mute ^= true);
        moves_both("a solo", |a| row(a, 0).solo ^= true);
        moves_both("a fader", |a| row(a, 0).gain = 0.4);
        moves_both("a pan", |a| row(a, 0).pan = 0.2);
        moves_both("an input", |a| row(a, 1).input = "in 2".into());
        moves_both("a MIDI device set", |a| row(a, 1).midi.channel = Some(9));
        moves_both("a row's identity", |a| row(a, 5).id = crate::desk_id::TrackId(77));
        moves_both("the drum bus", |a| a.desk.as_mut().unwrap().drums_gain = 0.5);
        moves_both("the drum bus pan", |a| a.desk.as_mut().unwrap().drums_pan = -0.2);
        moves_both("the click", |a| a.desk.as_mut().unwrap().metronome_gain = 0.2);
        moves_both("the master", |a| a.desk.as_mut().unwrap().master_gain = 0.6);
        moves_both("the effects return", |a| a.desk.as_mut().unwrap().fx_return = 0.3);
        moves_both("the desk's tempo", |a| a.desk.as_mut().unwrap().tempo_bpm = 140.0);
        moves_both("the signature", |a| a.desk.as_mut().unwrap().sig_beats = 5);
        moves_both("the signature's unit", |a| a.desk.as_mut().unwrap().sig_unit = 8);
        moves_both("the click switch", |a| {
            a.desk.as_mut().unwrap().metronome_on = false;
        });
        moves_both("which rows exist", |a| a.desk.as_mut().unwrap().set_word(|r| &mut r.added, 1));
        moves_both("a folder", |a| row(a, 1).folder = true);
        moves_both("what is inside a folder", |a| row(a, 0).in_folder = true);
        moves_both("a shut folder", |a| row(a, 0).shut = true);
        moves_both("an ARM", |a| row(a, 0).arm ^= true);
        moves_both("a row arriving", |a| {
            let next = crate::desk_model::TrackRow::vacant(crate::desk_id::TrackId(40));
            a.desk.as_mut().unwrap().rows.push(next);
        });
        moves_both("a row leaving", |a| {
            a.desk.as_mut().unwrap().rows.pop();
        });
        // **The specials** (W5), each fact on its own line: they were the
        // strip arrays' slots past the rows until they left strip space, and
        // a fact that hashed nowhere would be an edit no asterisk sees.
        fn special(a: &mut Arrangement, node: crate::desk_id::Node) -> &mut crate::desk_model::Special {
            a.desk.as_mut().unwrap().specials.get_mut(node).unwrap()
        }
        use crate::desk_id::Node as N;
        moves_both("the drum bus's name", |a| special(a, N::Drums).name = "DRUMS".into());
        moves_both("the click's paint", |a| special(a, N::Click).color = 7);
        moves_both("the drum bus's send", |a| {
            crate::desk_model::set_fx_send_level(&mut special(a, N::Drums).sends, 0.6);
        });
        moves_both("the return's mute", |a| special(a, N::Fx).mute ^= true);
        moves_both("the click's solo", |a| special(a, N::Click).solo ^= true);
        moves_both("the master's polarity", |a| special(a, N::Master).phase ^= true);
        moves_both("the master's name", |a| special(a, N::Master).name = "MIX".into());
        // The six that arrived late, each moved and each REMOVED: absence is
        // a different document from an empty one, and the whole meaning of
        // `None` rests on the walk telling them apart.
        moves_both("a polarity", |a| row(a, 0).phase ^= true);
        moves_both("polarity going unsaid", |a| unsay(a, Says::PHASE));
        moves_both("a theory send", |a| row(a, 0).theory ^= true);
        moves_both("the theory sends going unsaid", |a| unsay(a, Says::TO_THEORY));
        moves_both("a route's gain", |a| {
            a.desk.as_mut().unwrap().routes.as_mut().unwrap().0[0].gain = 0.25;
        });
        moves_both("a route's destination", |a| {
            a.desk.as_mut().unwrap().routes.as_mut().unwrap().0[0].target =
                crate::routing::Target::Node(crate::desk_id::Node::Track(crate::desk_id::TrackId(5)));
        });
        moves_both("the routing going unsaid", |a| {
            a.desk.as_mut().unwrap().routes = None;
        });
        moves_both("an empty routing table", |a| {
            a.desk.as_mut().unwrap().routes = Some(crate::routing::Routes::default());
        });
        moves_both("a sidechain key", |a| {
            a.desk.as_mut().unwrap().insert_keys.as_mut().unwrap().0[0].from =
                crate::desk_id::Node::Track(crate::desk_id::TrackId(4));
        });
        moves_both("the sidechains going unsaid", |a| {
            a.desk.as_mut().unwrap().insert_keys = None;
        });
        moves_both("a bypassed bay", |a| {
            let set = a.desk.as_mut().unwrap().bypass.as_mut().unwrap();
            set.clear();
            set.insert(crate::desk_id::SeatBay::new(
                crate::desk_id::Node::Track(crate::desk_id::TrackId(5)),
                crate::desk_id::Rack::Chain(0),
                0,
            ));
        });
        moves_both("the bypasses going unsaid", |a| {
            a.desk.as_mut().unwrap().bypass = None;
        });
        moves_both("a DX7 patch", |a| row(a, 0).dx7.patch = 31);
        moves_both("a DX7 cartridge", |a| row(a, 0).dx7.cart = "ROM2A.syx".into());
        moves_both("a Keys patch", |a| row(a, 1).keys.patch = 6);
        moves_both("a Keys knob", |a| row(a, 0).keys.params[0] = 0.9);
        moves_both("the instruments going unsaid", |a| unsay(a, Says::INSTRUMENTS));
        // ── **The second late wave** ───────────────────────────────────
        //
        // Every knob of the rack on its own line. The eight hash in one
        // loop, and a loop is exactly where two of them can end up on the
        // same word — which would make a session that moved its delay look
        // like a session that moved its reverb, and neither of them look
        // unsaved. This list is the thing that would notice.
        fn rack(a: &mut Arrangement) -> &mut Rack {
            a.desk.as_mut().unwrap().rack.as_mut().unwrap()
        }
        moves_both("the REVERB send", |a| rack(a).reverb = 0.9);
        moves_both("the DELAY send", |a| rack(a).delay = 0.9);
        moves_both("the CHORUS send", |a| rack(a).chorus = 0.9);
        moves_both("the PHASER send", |a| rack(a).phaser = 0.9);
        moves_both("the high-pass", |a| rack(a).hpf = 0.9);
        moves_both("the low-pass", |a| rack(a).lpf = 0.4);
        moves_both("the output gain", |a| rack(a).gain = 0.9);
        moves_both("the limiter threshold", |a| rack(a).limiter = 0.4);
        moves_both("an effect's parameter", |a| {
            rack(a).params.insert("reverb_size".into(), Value::from(0.9));
        });
        moves_both("an effect's parameter arriving", |a| {
            rack(a).params.insert("chorus_rate".into(), Value::from(0.2));
        });
        moves_both("an effect's parameter leaving", |a| {
            rack(a).params.shift_remove("delay_div");
        });
        moves_both("the rack going unsaid", |a| a.desk.as_mut().unwrap().rack = None);
        fn desk(a: &mut Arrangement) -> &mut DeskSnap {
            a.desk.as_mut().unwrap()
        }
        fn view(a: &mut Arrangement) -> &mut View {
            desk(a).view.as_mut().unwrap()
        }
        moves_both("the backing track's level", |a| desk(a).track_gain = Some(0.25));
        moves_both("that level going unsaid", |a| desk(a).track_gain = None);
        moves_both("the mono fold", |a| desk(a).master_mono = Some(false));
        moves_both("the fold going unsaid", |a| desk(a).master_mono = None);
        moves_both("the loudness window", |a| {
            desk(a).lufs_window = Some(crate::recorder::LoudnessWindow::Integrated);
        });
        moves_both("that window going unsaid", |a| desk(a).lufs_window = None);
        moves_both("the varispeed's range", |a| {
            desk(a).varispeed = Some(crate::varispeed::Varispeed::with_range(70, 140));
        });
        moves_both("the varispeed's position", |a| {
            let mut v = crate::varispeed::Varispeed::with_range(80, 130);
            v.set_pos(0.75);
            desk(a).varispeed = Some(v);
        });
        moves_both("the varispeed going unsaid", |a| desk(a).varispeed = None);
        // The tape's start and stop: the switch and each time on its own
        // line, because the three hash side by side, and its absence.
        fn start_stop(a: &mut Arrangement) -> &mut crate::motor::StartStop {
            desk(a).start_stop.as_mut().unwrap()
        }
        moves_both("the tape start and stop going off", |a| start_stop(a).set_on(false));
        moves_both("its start time", |a| start_stop(a).set_start_s(0.25));
        moves_both("its stop time", |a| start_stop(a).set_stop_s(0.25));
        moves_both("its going unsaid", |a| desk(a).start_stop = None);
        moves_both("a lane freezing", |a| {
            row(a, 0).frozen = Some(crate::desk_model::Frozen { path: "media/frozen-01-7.wav".into(), len_s: 3.0 });
        });
        moves_both("a frozen lane's file", |a| {
            row(a, 1).frozen.as_mut().unwrap().path = "media/frozen-02-8.wav".into();
        });
        moves_both("the freeze going unsaid", |a| unsay(a, Says::FROZEN));
        moves_both("the click's division", |a| {
            desk(a).click.as_mut().unwrap().div = 0;
        });
        moves_both("the click going into the take", |a| {
            desk(a).click.as_mut().unwrap().in_take = false;
        });
        moves_both("the click's sound", |a| {
            desk(a).click.as_mut().unwrap().sample = "rim.wav".into();
        });
        moves_both("the click going unsaid", |a| desk(a).click = None);
        moves_both("the transpose", |a| desk(a).transpose = Some(5));
        moves_both("the transpose going unsaid", |a| desk(a).transpose = None);
        moves_both("the count-in", |a| {
            desk(a).count_in.as_mut().unwrap().bars = 4;
        });
        moves_both("the count-in's legacy beats", |a| {
            desk(a).count_in.as_mut().unwrap().beats = 12;
        });
        moves_both("the count-in going into the take", |a| {
            desk(a).count_in.as_mut().unwrap().in_take = false;
        });
        moves_both("the count-in going unsaid", |a| desk(a).count_in = None);
        moves_both("the theme", |a| {
            desk(a).look.as_mut().unwrap().theme = "Centennial".into();
        });
        moves_both("the channel palette", |a| {
            desk(a).look.as_mut().unwrap().palette = 2;
        });
        moves_both("the palette going back to following the theme", |a| {
            desk(a).look.as_mut().unwrap().palette_auto = true;
        });
        moves_both("the look going unsaid", |a| desk(a).look = None);
        // **The timeline's own grid and window.** Eleven fields, each one
        // its own line, because the block was appended to the fingerprint
        // as a whole and a whole block is exactly the shape that hashes
        // right while one member of it is never read.
        moves_both("the timeline's magnet", |a| view(a).snap = true);
        moves_both("the timeline's grid", |a| view(a).grid_exp = -1);
        moves_both("the tempo lane", |a| view(a).tempo_lane = false);
        moves_both("the master row", |a| view(a).master_row = true);
        moves_both("the width of the track headers", |a| view(a).header_w = 210.0);
        moves_both("how many lanes are on screen", |a| view(a).visible = 5);
        moves_both("the quantize grid", |a| view(a).quantize_grid_exp = -2);
        moves_both("quantizing to triplets", |a| view(a).quantize_triplet = false);
        moves_both("where quantize puts the off-beat", |a| {
            view(a).quantize_swing_pct = 58.0;
        });
        moves_both("how hard quantize pulls", |a| view(a).quantize_strength_pct = 40.0);
        moves_both("the window quantize leaves alone", |a| {
            view(a).quantize_window_ms = 60.0;
        });
        moves_both("the height the lane count was chosen at", |a| {
            view(a).visible_at = 1200.0;
        });
        moves_both("how readily an attack is called one", |a| {
            view(a).quantize_sensitivity = 0.35;
        });
        moves_both("how a warped segment gets there", |a| {
            view(a).quantize_fit = "stretch".into();
        });
        moves_both("the view going unsaid", |a| desk(a).view = None);
        // **And one thing that must NOT move it**: where the arrangement
        // was left looking is not an edit (`DeskSnap::viewport`).
        {
            let base = a_document_with_everything_in_it();
            let mut zoomed = a_document_with_everything_in_it();
            desk(&mut zoomed).viewport = Aside(Some(Viewport { start_s: 300.0, len_s: 2.0 }));
            assert_ne!(zoomed.to_json(), base.to_json(), "the window is not in the document");
            assert_eq!(zoomed.fingerprint(), base.fingerprint(), "a scroll moved the fingerprint");
            assert_eq!(words(&zoomed), words(&base), "a scroll moved the words undo compares");
        }
        moves_both("the take name", |a| desk(a).take_name = Some("etude".into()));
        // **A CLEARED name is not an unsaid one.** The empty string is a
        // document saying "no name", which a session holding one has to be
        // able to read as an edit — see `DeskSnap::take_name`.
        moves_both("the take name cleared", |a| desk(a).take_name = Some(String::new()));
        moves_both("the take name going unsaid", |a| desk(a).take_name = None);
        moves_both("the editor's magnet", |a| {
            desk(a).feel.as_mut().unwrap().snap = false;
        });
        moves_both("the editor's grid", |a| {
            desk(a).feel.as_mut().unwrap().grid_exp = -2;
        });
        moves_both("the swing switch", |a| {
            desk(a).feel.as_mut().unwrap().swing = false;
        });
        moves_both("where the swung off-beat sits", |a| {
            desk(a).feel.as_mut().unwrap().swing_pct = 58.5;
        });
        moves_both("the triplet grid", |a| {
            desk(a).feel.as_mut().unwrap().triplet = false;
        });
        moves_both("how far humanize pulls a note", |a| {
            desk(a).feel.as_mut().unwrap().humanize_timing = 40;
        });
        moves_both("how far humanize pushes a velocity", |a| {
            desk(a).feel.as_mut().unwrap().humanize_vel = 40;
        });
        moves_both("the feel going unsaid", |a| desk(a).feel = None);
        moves_both("the key signature", |a| {
            desk(a).notation.as_mut().unwrap().staff_key = 4;
        });
        moves_both("reading accidentals as sharps", |a| {
            desk(a).notation.as_mut().unwrap().prefer_flats = false;
        });
        moves_both("the capo", |a| desk(a).notation.as_mut().unwrap().capo = 5);
        moves_both("what the capo is made of", |a| {
            desk(a).notation.as_mut().unwrap().capo_style = "wood".into();
        });
        moves_both("the guitar's tuning", |a| {
            desk(a).notation.as_mut().unwrap().tuning = "Open G".into();
        });
        moves_both("the notation going unsaid", |a| desk(a).notation = None);
        moves_both("having been named", |a| a.named = false);
        moves_both("a clip arriving", |a| {
            a.add_clip(ArrClip { path: "media/x.wav".into(), ..Default::default() });
        });
        moves_both("a clip leaving", |a| {
            a.clips.pop();
        });
        moves_both("a clip's kind", |a| a.clips[1].kind = ClipKind::Midi);
        moves_both("a clip's LANE", |a| a.clips[0].lane = 5);
        moves_both("a clip's file", |a| a.clips[1].path = "media/other.wav".into());
        moves_both("a clip's name", |a| a.clips[0].name = "renamed".into());
        moves_both("a NOTE MOVED", |a| a.clips[0].notes[1].at_s = 1.75);
        moves_both("a note's length", |a| a.clips[0].notes[0].dur_s = 0.75);
        moves_both("a note's pitch", |a| a.clips[0].notes[0].key = 61);
        moves_both("a note's VELOCITY", |a| a.clips[0].notes[0].vel = 101);
        moves_both("a note arriving", |a| {
            a.clips[0].notes.push(ArrNote { at_s: 3.0, dur_s: 0.1, key: 48, vel: 90, muted: false });
        });
        moves_both("a note leaving", |a| {
            a.clips[0].notes.pop();
        });
        moves_both("a pedal MOVED", |a| a.clips[0].pedal[0].at_s = 0.75);
        moves_both("a pedal's depth", |a| a.clips[0].pedal[0].value = 64);
        moves_both("which pedal", |a| a.clips[0].pedal[1].cc = 67);
        moves_both("a pedal movement arriving", |a| {
            a.clips[0].pedal.push(ArrPedal { at_s: 3.0, cc: 64, value: 127 });
        });
        moves_both("the pedal leaving", |a| a.clips[0].pedal.clear());
        moves_both("a clip's id", |a| a.clips[0].id += 100);
        moves_both("a clip's anchor", |a| a.clips[1].anchor = Anchor::Beat);
        moves_both("following the tempo", |a| a.clips[1].follow_tempo = true);
        moves_both("a real stretch", |a| a.clips[1].stretch = 2.0);
        moves_both("a clip's GAIN", |a| a.clips[1].gain = 0.5);
        moves_both("the stretch algorithm", |a| {
            a.clips[0].stretch_mode = "tape".into();
        });
        moves_both("a clip's START", |a| a.clips[0].start_s = 5.0);
        moves_both("a clip's left trim", |a| a.clips[0].offset_s = 1.0);
        moves_both("a clip's LENGTH", |a| a.clips[0].len_s = 9.0);
        moves_both("a fade in", |a| a.clips[0].fade_in_s = 0.3);
        moves_both("a fade out", |a| a.clips[0].fade_out_s = 0.4);
        moves_both("a loop length", |a| a.clips[0].loop_s = 4.0);
        moves_both("a stranger's key on a clip", |a| {
            a.clips[1].extra.insert("from_tomorrow".into(), Value::from(1));
        });
        moves_both("a stranger's VALUE on a clip", |a| {
            a.clips[0].extra.insert("a_newer_builds_key".into(), Value::from("changed"));
        });
        moves_both("a tempo", |a| {
            let mut p = a.tempo.points().to_vec();
            p[1].bpm = 132.0;
            a.tempo = crate::tempo::TempoMap::sanitised(p);
        });
        moves_both("a ritardando", |a| {
            let mut p = a.tempo.points().to_vec();
            p[1].ramp = true;
            a.tempo = crate::tempo::TempoMap::sanitised(p);
        });
        moves_both("a tempo point arriving", |a| {
            let mut p = a.tempo.points().to_vec();
            p.push(crate::tempo::TempoPoint { at_beat: 32.0, bpm: 60.0, ramp: false });
            a.tempo = crate::tempo::TempoMap::sanitised(p);
        });
        moves_both("a signature change", |a| {
            let mut p = a.meter.points().to_vec();
            p[1].sig = crate::recorder::TimeSignature { beats: 5, unit: 4 };
            a.meter = crate::tempo::MeterMap::sanitised(p);
        });
        moves_both("where a signature changes", |a| {
            let mut p = a.meter.points().to_vec();
            p[1].at_bar = 9;
            a.meter = crate::tempo::MeterMap::sanitised(p);
        });
        moves_both("an automation point", |a| {
            let c = &a.automation.curves()[0];
            let mut p = c.points().to_vec();
            p[0].value = 0.9;
            let curve = crate::automation::Curve::sanitised(c.lane, c.param, p);
            let mut all = a.automation.curves().to_vec();
            all[0] = curve;
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("a point's shape", |a| {
            let c = &a.automation.curves()[0];
            let mut p = c.points().to_vec();
            p[0].shape = crate::automation::Shape::Hold;
            let curve = crate::automation::Curve::sanitised(c.lane, c.param, p);
            let mut all = a.automation.curves().to_vec();
            all[0] = curve;
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("which control a curve moves", |a| {
            let c = &a.automation.curves()[0];
            let curve = crate::automation::Curve::sanitised(
                c.lane,
                crate::automation::AutoParam::Pan,
                c.points().to_vec(),
            );
            let mut all = a.automation.curves().to_vec();
            all[0] = curve;
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("which BAY a curve moves", |a| {
            let c = &a.automation.curves()[1];
            let curve = crate::automation::Curve::sanitised(
                c.lane,
                crate::automation::AutoParam::Plugin { bay: 2, id: 77 },
                c.points().to_vec(),
            );
            let mut all = a.automation.curves().to_vec();
            all[1] = curve;
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("whose plugin a kept curve waits for", |a| {
            let mut all = a.automation.curves().to_vec();
            all[1].tenant = "@tangent-comp".into();
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("a kept curve's plugin coming back", |a| {
            let mut all = a.automation.curves().to_vec();
            all[1].tenant.clear();
            a.automation = crate::automation::Automation::from_curves(all);
        });
        moves_both("automation going away", |a| {
            a.automation = crate::automation::Automation::default();
        });
        moves_both("a marker MOVED", |a| a.markers[0].at_s = 13.0);
        moves_both("a marker renamed", |a| a.markers[0].name = "Chorus".into());
        moves_both("a marker arriving", |a| {
            a.markers.push(ArrMarker { at_s: 40.0, name: String::new() });
        });
        moves_both("the markers leaving", |a| a.markers.clear());
        moves_both("a stranger's key on the document", |a| {
            a.extra.insert("also_from_tomorrow".into(), Value::from(true));
        });
    }

    /// **And nothing else does.**
    ///
    /// Every one of these is a struct that changed while the FILE did not,
    /// and the fingerprint has to agree with the file. Each is a real trap
    /// the writer sets: a normalised counter, two deadbands, a spelling with
    /// two forms, and a map whose single point is not written down at all.
    #[test]
    fn the_fingerprint_stands_when_the_document_stands() {
        moves_neither(
            "an id counter behind its clips",
            a_document_with_everything_in_it(),
            |a| a.next_id = 0,
        );
        moves_neither(
            "a stretch inside the writer's deadband",
            a_document_with_everything_in_it(),
            |a| a.clips[1].stretch = 1.0 + 1.0e-12,
        );
        moves_neither(
            "a gain inside the writer's deadband",
            a_document_with_everything_in_it(),
            |a| a.clips[1].gain = 1.0 - 1.0e-12,
        );
        moves_neither(
            "the two spellings of no stretch algorithm",
            a_document_with_everything_in_it(),
            |a| a.clips[1].stretch_mode = "auto".into(),
        );
        // The same two spellings, one surface further out: the quantize
        // window's fit, whose empty word and "auto" mean one thing.
        let mut auto_fit = a_document_with_everything_in_it();
        auto_fit.desk.as_mut().unwrap().view.as_mut().unwrap().quantize_fit = String::new();
        moves_neither("the two spellings of no warp fit", auto_fit, |a| {
            a.desk.as_mut().unwrap().view.as_mut().unwrap().quantize_fit = "auto".into();
        });
        // A ONE-POINT tempo map is not written at all — the desk snapshot
        // carries the song's single tempo instead. So its value cannot move
        // the fingerprint, or every undo on a plain song would look like an
        // edit.
        let mut plain = a_document_with_everything_in_it();
        plain.tempo = crate::tempo::TempoMap::constant(96.0);
        plain.meter =
            crate::tempo::MeterMap::constant(crate::recorder::TimeSignature::default());
        moves_neither("a constant tempo's value", plain.clone(), |a| {
            a.tempo = crate::tempo::TempoMap::constant(140.0);
        });
        moves_neither("a constant meter's value", plain, |a| {
            a.meter = crate::tempo::MeterMap::constant(crate::recorder::TimeSignature {
                beats: 7,
                unit: 8,
            });
        });
    }

    /// **The seats are written as a whole head and a sparse tail, and read
    /// back from any shape.** The one fixture, three ways: as this build
    /// writes it, as a build before v7 wrote it (one dense string per bay,
    /// all 13,167), and as the fully sparse object — the same desk, the
    /// same fingerprint, and a fixed point of the writer.
    #[test]
    fn the_seats_are_a_whole_head_and_a_sparse_tail_from_any_shape() {
        let a = a_document_with_everything_in_it();
        // The LEGACY writer's shape, which every document before format 2
        // holds and the legacy reader still takes: format 2 names each
        // plugin by its bay instead.
        let legacy = |a: &Arrangement| a.to_json_with_desk_as(a.desk.as_ref(), crate::desk_format::Format::Legacy);
        let written = serde_json::from_str::<Value>(&legacy(&a)).expect("json");
        // **The desk as that shape reads back**, which every variant below
        // must read as. The fixture is the shipped thirty-two rows under the
        // identity ids, the legacy shape's own width, and its specials sit
        // at strips 32..=35 of the legacy arrays (W5 joins them there), so
        // every row and every special reads back as it was.
        let fixture = a.desk.clone().expect("a desk");
        let d = Arrangement::from_json(&legacy(&a)).desk.expect("the legacy shape reads");
        assert_eq!(d.specials, fixture.specials, "the legacy shape lost a special");
        assert_eq!(d.rows.len(), fixture.rows.len(), "the legacy shape invented or lost a row");
        for (n, (got, want)) in d.rows.iter().zip(&fixture.rows).enumerate() {
            assert_eq!(got, want, "the legacy shape read row {n} differently");
        }
        assert_eq!(d, fixture, "the legacy shape read something else differently");
        let desk = written.get("desk").and_then(Value::as_object).expect("a desk object");
        let head = desk.get("inserts").and_then(Value::as_array).expect("the head array");
        assert_eq!(head.len(), HEAD_BAYS, "the head is not written whole");
        assert_eq!(head[1], Value::String("reverb".into()));
        let tail = desk.get("inserts_tail").and_then(Value::as_object).expect("a tail");
        assert_eq!(tail.get("12447"), Some(&Value::String("brusfri".into())));
        assert_eq!(tail.len(), 1, "a head seat leaked into the tail");
        // **Hundreds, never thousands**: the legacy shape writes its head
        // whole and one string per strip in each of its string arrays (the
        // names, the inputs, the cartridges: a 32-row desk, the fixture's
        // width since the snapshot became rows), where the dense seats were
        // 13,167 of them. Format 2 writes a row's strings, a handful each.
        let legacy_bound = HEAD_BAYS + 4 * (crate::desk_legacy::V32_STRIPS + 1);
        let empties = legacy(&a).matches("\"\"").count();
        assert!(empties < legacy_bound, "thousands of empties again: {empties}");
        let empties = a.to_json().matches("\"\"").count();
        assert!(empties < legacy_bound, "thousands of empties in format 2: {empties}");

        // The dense shape every earlier build wrote.
        let mut dense = vec![Value::String(String::new()); crate::desk_legacy::V32_INSERT_SPACE];
        dense[1] = Value::String("reverb".into());
        dense[12447] = Value::String("brusfri".into());
        let mut old = written.clone();
        let od = old["desk"].as_object_mut().expect("desk");
        od.insert("inserts".into(), Value::Array(dense));
        od.shift_remove("inserts_tail");
        let from_dense = Arrangement::from_json(&old.to_string());
        let as_read = Arrangement::from_json(&legacy(&a));
        assert_eq!(from_dense.desk.as_ref(), Some(&d), "the dense shape read differently");
        assert_eq!(from_dense.fingerprint(), as_read.fingerprint(), "the dense shape hashes apart");
        assert_eq!(legacy(&from_dense), legacy(&a), "the dense shape is not rewritten as v7");

        // The fully sparse object.
        let mut sparse = written.clone();
        let sd = sparse["desk"].as_object_mut().expect("desk");
        sd.insert("inserts".into(), serde_json::json!({"1": "reverb", "12447": "brusfri"}));
        sd.shift_remove("inserts_tail");
        let from_sparse = Arrangement::from_json(&sparse.to_string());
        assert_eq!(from_sparse.desk.as_ref(), Some(&d), "the sparse object read differently");
        assert_eq!(from_sparse.fingerprint(), as_read.fingerprint());

        // And what the reader refuses to be confused by: an empty path, a
        // key that is not a seat, a seat claimed twice, out of order.
        let mut odd = written.clone();
        let odd_d = odd["desk"].as_object_mut().expect("desk");
        odd_d.insert(
            "inserts_tail".into(),
            serde_json::json!({"12447": "brusfri", "9000": "", "x": "y", "012447": "shadow", "500": "late"}),
        );
        let read = Arrangement::from_json(&odd.to_string()).desk.expect("a desk");
        assert_eq!(read.bay(12447), Some("brusfri"), "the second claim on a seat won");
        assert_eq!(read.bay(9000), None, "an empty path made a seat");
        assert_eq!(read.bay(500), Some("late"));
        let flat = read.legacy_bays();
        assert!(flat.windows(2).all(|w| w[0].0 < w[1].0), "the pairs are not sorted");

        // And a key that names a seat past the map, in either sparse
        // shape. A dense array had to SPEND a file byte per seat; a key
        // spends twenty on any bay at all, and everything downstream sizes
        // a dense space by the bay — so these are a terabyte and a wrap to
        // zero, not seats. The last real seat stays, the first past it goes.
        let space = crate::desk_legacy::V32_INSERT_SPACE;
        let past = serde_json::json!({
            "18446744073709551615": "usize-max",
            "100000000000": "a terabyte of strings",
            space.to_string(): "one past the map",
            (space - 1).to_string(): "the last seat",
            "12447": "brusfri",
        });
        let mut tail = written.clone();
        tail["desk"].as_object_mut().expect("desk").insert("inserts_tail".into(), past.clone());
        let mut bare = written.clone();
        let bd = bare["desk"].as_object_mut().expect("desk");
        bd.insert("inserts".into(), past);
        bd.shift_remove("inserts_tail");
        for (shape, json) in [("inserts_tail", tail), ("a bare inserts object", bare)] {
            let read = Arrangement::from_json(&json.to_string()).desk.expect("a desk");
            assert_eq!(read.bay(usize::MAX), None, "{shape}: usize::MAX made a seat");
            assert_eq!(read.bay(100_000_000_000), None, "{shape}: an eleven-digit bay made a seat");
            assert_eq!(read.bay(space), None, "{shape}: the seat one past the map was kept");
            assert_eq!(read.bay(space - 1), Some("the last seat"), "{shape}: the last real seat was lost");
            assert_eq!(read.bay(12447), Some("brusfri"), "{shape}: a real far seat was lost");
            assert!(
                read.legacy_bays().iter().all(|(b, _)| *b < space),
                "{shape}: a seat past the map survived the reader"
            );
        }
    }

    /// **A desk seen through the view and through a snapshot is one desk.**
    /// `to_snap` and `as_ref` are inverses, and the number is the same
    /// whichever way the walk reaches the fields — which is what lets the
    /// app fingerprint its live settings without a clone.
    #[test]
    fn a_desk_by_reference_is_the_same_desk() {
        let a = a_document_with_everything_in_it();
        let d = a.desk.as_ref().expect("the fixture has a desk");
        assert_eq!(d.as_ref().to_snap(), *d, "to_snap(as_ref(d)) is not d");
        let mut by_ref = Fingerprint::new();
        a.hash_into_ref(Some(d.as_ref()), &mut by_ref);
        assert_eq!(by_ref.finish(), a.fingerprint(), "the view hashed differently");
    }

    /// **An empty seat in the sparse pairs is not a word.** Every
    /// constructor keeps the pairs canonical (`from_value`, `to_snap`,
    /// `set_bay`), so this is the lock for the one that will not: a
    /// hand-pushed `(bay, "")` hashes and copies as no seat at all, the
    /// way the dense shape has always treated an empty string.
    #[test]
    fn an_empty_seat_in_the_sparse_pairs_is_not_a_word() {
        let a = a_document_with_everything_in_it();
        let mut d = a.desk.clone().expect("a desk");
        let clean = d.clone();
        let far = crate::desk_legacy::bay_of_flat(&d.row_ids(), crate::desk_legacy::V32_INSERT_SPACE - 1).expect("a bay");
        d.plugins.insert(far, String::new());
        let mut with = Fingerprint::recording(0);
        d.as_ref().hash_into(&mut with);
        let mut without = Fingerprint::recording(0);
        clean.as_ref().hash_into(&mut without);
        assert_eq!(with.finish_recorded(), without.finish_recorded(), "an empty seat became a word");
        assert_eq!(d.as_ref().to_snap(), clean, "to_snap kept the empty seat");
    }

    /// **Recording changes nothing about the number.** The words are kept
    /// beside the mix, never instead of it, so a moment's mark and the
    /// title bar's answer come from one walk and cannot disagree.
    #[test]
    fn a_recording_fingerprint_answers_the_same_number() {
        let a = a_document_with_everything_in_it();
        let mut f = Fingerprint::recording(0);
        a.hash_into(a.desk.as_ref(), &mut f);
        let (w, mark) = f.finish_recorded();
        assert_eq!(mark, a.fingerprint(), "recording moved the number");
        assert!(!w.is_empty(), "nothing was recorded");
        assert!(
            Fingerprint::new().finish_recorded().0.is_empty(),
            "a plain fingerprint kept words"
        );
        // And the copy undo keeps is the same document apart from its desk.
        let bare = a.clone_without_desk();
        assert!(bare.desk.is_none());
        assert_eq!(
            bare.to_json_with_desk(a.desk.as_ref()),
            a.to_json(),
            "the desk-less copy is a different document"
        );
    }

    /// **A save and a load do not move the fingerprint.**
    ///
    /// The comparison that decides "is there anything unsaved" runs against
    /// a document that has been through the reader; if a round trip moved
    /// the number, every freshly opened project would open dirty. The
    /// document is asserted to be a fixed point first, because that is the
    /// property the fingerprint is allowed to lean on.
    /// **The window the document carries comes back as it went, and a
    /// window nobody could use is no window**: the open shows the default
    /// rather than a picture zero seconds wide or before the song.
    #[test]
    fn the_viewport_round_trips_and_a_bad_one_reads_as_none() {
        let a = a_document_with_everything_in_it();
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(
            back.desk.expect("the desk came back").viewport.0,
            Some(Viewport { start_s: 12.5, len_s: 96.0 }),
            "the window did not come back"
        );
        for bad in [
            serde_json::json!({ "start_s": -1.0, "len_s": 10.0 }),
            serde_json::json!({ "start_s": 0.0, "len_s": 0.0 }),
            serde_json::json!({ "start_s": 0.0 }),
            serde_json::json!("wide"),
        ] {
            let mut v: Value = serde_json::from_str(&a.to_json()).unwrap();
            v["desk"]["viewport"] = bad.clone();
            let read = Arrangement::from_json(&v.to_string());
            assert_eq!(read.desk.unwrap().viewport.0, None, "{bad} was read as a window");
        }
    }

    #[test]
    fn a_round_trip_keeps_the_fingerprint() {
        // **The document as the reader hands it over.** Format 2 writes the
        // desk's ROWS, not the lengths of the arrays they came from, so this
        // fixture's three-entry arrays come back as the live desk's width,
        // the same rows with nothing else said: the asterisk compares against
        // a document that has been through the reader, and that is the one
        // whose number may not move.
        let fixture = a_document_with_everything_in_it();
        let json = fixture.to_json();
        let a = Arrangement::from_json(&json);
        assert_eq!(a.to_json(), json, "the reader's document writes other bytes than the fixture");
        let model = |a: &Arrangement| a.desk.as_ref().map(|d| d.desk_model().model);
        assert_eq!(model(&a), model(&fixture), "the reader's desk is not the fixture's, row for row");
        let back = Arrangement::from_json(&a.to_json());
        assert_eq!(back.to_json(), json, "the document is not a fixed point");
        assert_eq!(
            back.fingerprint(),
            a.fingerprint(),
            "a save and a load moved the fingerprint"
        );
        // And through a real file, which is the road a project takes.
        let dir = std::env::temp_dir().join(format!("tangent-fp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let doc = dir.join(DOC_NAME);
        a.save_to(&doc);
        assert_eq!(
            Arrangement::load_from(&doc).fingerprint(),
            a.fingerprint(),
            "the file moved the fingerprint"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **A float written at seventeen digits reads back as itself.**
    ///
    /// ryu writes the SHORTEST decimal that round-trips, and for a value
    /// one ULP under 1.0 that is `0.9999999999999999`. serde_json's default
    /// float parser takes a fast path on such inputs and answered `1.0`, so
    /// a document carrying one was not a fixed point: it opened dirty, its
    /// fingerprint moved on every load, and an undo across it pushed a
    /// moment for an edit nobody made. `float_roundtrip` in Cargo.toml is
    /// what makes this hold; the feature is a dependency flag, which is why
    /// the property gets a test of its own rather than a comment.
    #[test]
    fn a_seventeen_digit_float_is_a_fixed_point() {
        let awkward = 0.999_999_999_999_999_9_f64;
        assert_ne!(awkward, 1.0, "the literal rounded to 1.0 at compile time");
        // Through the reader once first, for `a_round_trip_keeps_the_fingerprint`'s
        // reason: format 2 writes the fixture's short arrays as whole rows.
        let mut a = Arrangement::from_json(&a_document_with_everything_in_it().to_json());
        a.clips[0].fade_out_s = awkward;
        let json = a.to_json();
        assert!(
            json.contains("0.9999999999999999"),
            "the writer did not spell the value at seventeen digits"
        );
        let back = Arrangement::from_json(&json);
        assert_eq!(
            back.clips[0].fade_out_s.to_bits(),
            awkward.to_bits(),
            "the reader rounded the fade to a neighbour"
        );
        assert_eq!(back.to_json(), json, "the document is not a fixed point");
        assert_eq!(
            back.fingerprint(),
            a.fingerprint(),
            "a save and a load moved the fingerprint"
        );
    }

    /// **The awkward floats hash the way the WRITER treats them.**
    ///
    /// `-0.0` is a different document (ryu writes `-0.0`), so it is a
    /// different number. Every non-finite value is the SAME document — the
    /// writer refuses them and leaves the key out — so they are all one
    /// number, including two NaNs with different payloads, which `to_bits`
    /// alone would have told apart and reported as an edit.
    #[test]
    fn minus_zero_and_the_nans_hash_the_way_the_writer_sees_them() {
        let base = a_document_with_everything_in_it();
        let plus = base.fingerprint();
        assert_eq!(base.clips[1].start_s, 0.0, "the fixture moved");

        let mut minus = base.clone();
        minus.clips[1].start_s = -0.0;
        assert_ne!(minus.to_json(), base.to_json(), "-0.0 is the same file after all");
        assert_ne!(minus.fingerprint(), plus, "-0.0 did not move the fingerprint");

        let quiet = f64::from_bits(0x7ff8_0000_0000_0001);
        assert!(quiet.is_nan(), "the second NaN is not one");
        let mut a = base.clone();
        a.clips[0].notes[0].at_s = f64::NAN;
        let mut b = base.clone();
        b.clips[0].notes[0].at_s = quiet;
        let mut c = base;
        c.clips[0].notes[0].at_s = f64::INFINITY;
        assert_eq!(a.to_json(), b.to_json(), "two NaNs wrote different files");
        assert_eq!(a.to_json(), c.to_json(), "a NaN and an infinity wrote different files");
        assert_eq!(a.fingerprint(), b.fingerprint(), "two NaNs fingerprinted apart");
        assert_eq!(
            a.fingerprint(),
            c.fingerprint(),
            "a NaN and an infinity fingerprinted apart"
        );
        assert_ne!(a.fingerprint(), plus, "an unwritable note time looked unchanged");
    }

}
