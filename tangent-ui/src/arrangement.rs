//! The arrangement: the DOCUMENT half of the app, in its own file.
//!
//! # Why this is not in `settings.json`, and the decision is deliberate
//!
//! Settings are PREFERENCES: fixed-size arrays keyed by the desk's shape,
//! migrated by `settings_version`, one set per user for ever. An arrangement
//! is a DOCUMENT: an unbounded list of clips whose positions change with the
//! work, one per song, and eventually a thing you hand to somebody. Folding
//! the second into the first would mean exactly one arrangement for ever
//! (starting a second song destroys the first), every schema change running a
//! migration across everyone's colours and window geometry, and the
//! `LEGACY_MAX_STRIPS` caps losing their meaning the moment an unbounded
//! document shares the file. The owner weighed this and chose the split.
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
pub const ARRANGEMENT_VERSION: u64 = 7;

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
/// the master, `(STRIPS + 1) * INSERTS`. See [`V7_SPARSE_SEATS`].
pub const HEAD_BAYS: usize = (crate::recorder::STRIPS + 1) * crate::recorder::INSERTS;

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
            // Past the end of a LOOP an edge is a repeat's partial, and goes
            // as the notes twin's note does. Past the end of a clip that
            // does not loop it is kept, where a note is not: the engine lets
            // the pedal go at the clip's end and this list states no lift
            // there, so dropping the edge would hold a pedal down for good.
            if clip.loop_s > 1.0e-6 && at >= clip.len_s - 1.0e-9 {
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
    out.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
    out
}

/// The DESK a session carries — what makes a project a SESSION rather
/// than a clip list: which channels are what, what stands in their bays,
/// their names, colours, faders and sends, and the transport's musical
/// facts. Captured into the document on every save; APPLIED only on an
/// explicit open (launch reopens the last project against the settings
/// that are already its live desk).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct DeskSnap {
    pub kinds: Vec<u8>,
    /// Channel badges, `Settings::channel_icons` — empty in older docs.
    pub icons: Vec<u8>,
    pub names: Vec<String>,
    pub colors: Vec<i64>,
    /// **The occupied seats only**, `(bay, path)` sorted by bay, never an
    /// empty path — the `BypassBays` shape. Dense it was 13,167 strings a
    /// snapshot, copied into every undo moment; see [`V7_SPARSE_SEATS`].
    /// Kept canonical by every constructor (`from_value` sorts and drops
    /// empties, `DeskRef::to_snap` filters them, `set_bay` keeps the order)
    /// so that two desks that write the same text are equal as structs.
    pub inserts: Vec<(usize, String)>,
    pub sends: Vec<f64>,
    pub muted: u64,
    pub soloed: u64,
    pub gains: Vec<f64>,
    pub pans: Vec<f64>,
    pub inputs: Vec<String>,
    /// **Which MIDI devices each channel listens to, and on which channel.**
    ///
    /// Beside `inputs` rather than folded into it, because `inputs` is what a
    /// build without this feature reads and it must go on saying exactly what
    /// it said. Trimmed of trailing defaults, so a document whose tracks all
    /// listen to everything carries nothing extra — see
    /// `recorder::trim_midi_inputs`.
    ///
    /// Absent in every document written before this, and derived from
    /// `inputs` when it is: see `from_value`, which reconciles the two the
    /// same way the settings file does.
    pub midi_inputs: Vec<crate::recorder::MidiInputSel>,
    pub drums_gain: f64,
    pub drums_pan: f64,
    pub metronome_gain: f64,
    pub master_gain: f64,
    pub fx_return: f64,
    pub tempo_bpm: f64,
    pub sig_beats: u8,
    pub sig_unit: u8,
    pub metronome_on: bool,
    /// **Which channels EXIST**, one bit each.
    ///
    /// A desk is not only how the strips are set — it is which of them are
    /// there at all, and this was the one part of that not written down. So
    /// opening a document (or making a New one from a template) applied its
    /// levels, names and bays over whatever rows happened to be on screen
    /// and left the rows themselves behind: the owner's "click new track →
    /// no channels are deleted".
    ///
    /// Appended, and absent in every document written before this. See
    /// `from_value`, which reconstructs it from the rest of the snapshot
    /// rather than assuming an empty desk — a v4 document that says nothing
    /// about its rows still knows which ones it described.
    pub tracks_added: u32,
    /// **The folder skeleton** — `strip_is_folder` per channel, as the
    /// settings spell it.
    ///
    /// Appended, and absent in every document before this. The three
    /// folder vectors lived only in the shared settings file, which made a
    /// folder a property of the MACHINE rather than of the song: opening
    /// any project (or File > New) wore the previous session's folder
    /// shape over a desk that never had one, and undo restored every
    /// other per-channel fact while the folder flags kept their
    /// post-gesture values. Carried here they ride the same snapshot
    /// everything else rides — the document, the template, and every undo
    /// moment. Absent reads as FLAT, never as "keep whatever is live":
    /// a document that says nothing about folders has none, and
    /// preserving the live flags instead is exactly the bleed these
    /// fields end.
    pub is_folder: Vec<bool>,
    /// `strip_in_folder` per channel — see `is_folder`.
    pub in_folder: Vec<bool>,
    /// `strip_folder_shut` per channel — see `is_folder`. A stale shut
    /// flag is the worst of the three: it HIDES the next project's real
    /// lanes, not just indents its empty ones.
    pub folder_shut: Vec<bool>,
    /// **Which channels are ARMED** — record-ready and live — one bit each.
    ///
    /// Appended, and absent in every document written before this. Arming
    /// used to be recomputed at every launch and never written down: a
    /// session opened with the DX7's channel live and nothing else, whatever
    /// the project had been left holding. The owner: "record arming and
    /// other related states like mute and solo are also remembered and
    /// respected by loading projects ... load my default with the piano
    /// still hooked up and theory-view bound and arm a few extra tracks."
    /// Mute and solo have ridden this snapshot since it existed; the arm is
    /// the one desk switch that did not, which is the whole of the fault.
    ///
    /// Absent reads as [`armed_by_default`] — the shipped rule, computed
    /// from THIS snapshot's own kinds and bays — never as "keep whatever
    /// the session had". An old project therefore opens exactly as it
    /// always did, and one written since opens as it was left. Deriving
    /// rather than defaulting to zero is also what keeps the asterisk
    /// honest: a document whose desk differs from the live one only in a
    /// field it does not carry is not a document with unsaved changes.
    pub armed: u32,
    // ── **What a desk is, continued** ──────────────────────────────────
    //
    // Everything below travels the same road as everything above, and
    // every one of them reached this struct late — they lived in the
    // SETTINGS alone, which made each of them a property of the machine
    // rather than of the song: open any project and it wore the last
    // session's polarity, its theory sends, its routing, its sidechains,
    // its bypasses and its piano patches. The owner, having been shown the
    // list: "all the things you flagged should be in document."
    //
    // **`None` means the document does not SAY**, and is not the same as an
    // empty one — which is why each is an `Option` where the fields above
    // are plain. A file written before its key predates the whole idea of
    // carrying it, so the live value stands and nothing changes for that
    // project; a file written since always carries one (`to_snap` has an
    // opinion for every field), and then the document wins outright.
    //
    // This is deliberately NOT the folder fields' rule one screen up, where
    // absent reads as FLAT. That rule is a judgement about HARM, not a law:
    // a stale folder flag HIDES lanes, so keeping it was worse than
    // dropping it. Here the harm runs the other way — zeroing a routing
    // table or resetting every piano to patch 0 would change what an old
    // project SOUNDS like, silently, on first open. Nothing audible may
    // change because a field was added.
    /// **Which strips are polarity-inverted**, one bit each — `strip_phase`,
    /// the Ø switch. The one per-strip signal switch that never travelled
    /// beside `muted` and `soloed`.
    pub phase: Option<u64>,
    /// **Which channels send their MIDI to the theory view** — the note
    /// switch, `strip_to_theory`.
    pub to_theory: Option<Vec<bool>>,
    /// **The desk's routing table** — `Routes`, every route's source,
    /// destination and gain. Song content by any reading: where a channel's
    /// audio GOES is not a property of the computer it is played on.
    pub routes: Option<crate::routing::Routes>,
    /// **The sidechain key edges** — which channel keys which bay. Desk
    /// topology, as `KeyEdge`'s own note says, which is exactly why it
    /// belongs to the desk the document describes.
    pub insert_keys: Option<crate::routing::KeyEdges>,
    /// **Which bays are bypassed** — out of circuit but still loaded.
    pub bypass: Option<crate::settings::BypassBays>,
    /// **What the built-in instruments are playing.** The DX7's cartridge
    /// and patch per strip, the Keys patch and its knobs. The most audible
    /// of the six: without it a saved song's pianos play whatever patch the
    /// last session happened to leave on that channel.
    pub instruments: Option<Instruments>,
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
    /// `recorder::Gains` that never travelled. `channels`, `metronome`,
    /// `master` and `fx_return` are the plain fields above.
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
    /// **Which lanes are frozen, and to what file.** See [`FrozenLane`].
    ///
    /// The one desk field where "the document does not say" does NOT mean
    /// "the live value stands": a freeze is a fact about THIS document's
    /// media folder, so a document that carries no `frozen` has nothing
    /// frozen, and opening it thaws whatever the last one had. The live
    /// desk always says `Some`, empty when nothing is frozen.
    pub frozen: Option<Vec<FrozenLane>>,
}

/// **What the built-in instruments are set to**, per strip.
///
/// One field of [`DeskSnap`] rather than four, because they are one fact —
/// what the desk's own instruments are playing — and because four keys that
/// must be read and written together are four chances to carry three of
/// them. The vectors are the settings' own, trimmed of nothing: they are
/// short (one entry per strip) and a sparse form would buy bytes at the cost
/// of a second shape to get right.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Instruments {
    /// `strip_dx7_cartridge`: the file each strip's DX7 has open.
    pub dx7_cartridge: Vec<String>,
    /// `strip_dx7_patch`: which voice of it is selected.
    pub dx7_patch: Vec<usize>,
    /// `strip_keys_patch`: which Keys patch the strip plays.
    pub keys_patch: Vec<usize>,
    /// `strip_keys_params`: the Keys knobs, stride-8 per strip. See
    /// `Settings::strip_keys_params`, whose own note explains why the
    /// stride is what it is and why it may never shrink.
    pub keys_params: Vec<f64>,
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

/// **Which channels a desk arms when nothing says otherwise.**
///
/// All arms off, save one, by the owner's rule: a project must be openable
/// without headphones and without being blasted, and the one exception is the
/// channel that makes the piano sound — the one whose first bay holds the
/// built-in DX7.
///
/// One rule, two callers: `app::default_armed` asks it about a `Settings` and
/// [`DeskSnap::from_value`] asks it about a document that carries no `armed`
/// key. A second copy of it is a launch and an open that disagree about what
/// a silent desk means.
pub fn armed_by_default(kinds: &[u8], mut bay0: impl FnMut(usize) -> bool) -> u32 {
    (0..crate::recorder::CHANNELS.min(32))
        .find(|n| kinds.get(*n).copied().unwrap_or(0) == 1 && bay0(*n))
        .map_or(0, |n| 1 << n)
}

/// **The insert seats a `DeskRef` looks at, in whichever shape their owner
/// keeps them**: the settings' dense `strip_inserts` (one string per bay,
/// nearly all empty) or a snapshot's sparse pairs. `iter` yields the
/// OCCUPIED seats in ascending bay order from either, which is the one
/// thing the walk and the writer read — so a settings desk and the
/// snapshot taken from it hash alike without either being converted.
#[derive(Clone, Copy)]
pub enum Bays<'a> {
    Dense(&'a [String]),
    Sparse(&'a [(usize, String)]),
}

impl<'a> Bays<'a> {
    pub fn iter(self) -> BaysIter<'a> {
        match self {
            Self::Dense(v) => BaysIter::Dense(v.iter().enumerate()),
            Self::Sparse(v) => BaysIter::Sparse(v.iter()),
        }
    }
}

/// See [`Bays::iter`].
pub enum BaysIter<'a> {
    Dense(std::iter::Enumerate<std::slice::Iter<'a, String>>),
    Sparse(std::slice::Iter<'a, (usize, String)>),
}

impl<'a> Iterator for BaysIter<'a> {
    type Item = (usize, &'a str);

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Dense(it) => it.find(|(_, p)| !p.is_empty()).map(|(n, p)| (n, p.as_str())),
            // Canonical pairs hold no empties; the filter is for the
            // constructor that will not be — an empty seat must not be a
            // word from either shape, or the two stop hashing alike.
            Self::Sparse(it) => it.find(|(_, p)| !p.is_empty()).map(|(n, p)| (*n, p.as_str())),
        }
    }
}

/// **The desk by reference** — every field of [`DeskSnap`] borrowed from
/// wherever it lives, so the once-a-frame fingerprint can walk the LIVE
/// desk without first cloning thousands of seat strings into a snapshot it
/// throws away a microsecond later.
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
    pub kinds: &'a [u8],
    pub icons: &'a [u8],
    pub names: &'a [String],
    pub colors: &'a [i64],
    pub inserts: Bays<'a>,
    pub sends: &'a [f64],
    pub muted: u64,
    pub soloed: u64,
    pub gains: &'a [f64],
    pub pans: &'a [f64],
    pub inputs: &'a [String],
    pub midi_inputs: &'a [crate::recorder::MidiInputSel],
    pub drums_gain: f64,
    pub drums_pan: f64,
    pub metronome_gain: f64,
    pub master_gain: f64,
    pub fx_return: f64,
    pub tempo_bpm: f64,
    pub sig_beats: u8,
    pub sig_unit: u8,
    pub metronome_on: bool,
    pub tracks_added: u32,
    pub is_folder: &'a [bool],
    pub in_folder: &'a [bool],
    pub folder_shut: &'a [bool],
    /// See [`DeskSnap::armed`]. A mask, like `muted` and `soloed` beside it.
    pub armed: u32,
    /// The six that arrived late — see the block on [`DeskSnap`]. Borrowed
    /// like everything else here so the once-a-frame walk clones nothing;
    /// `None` is a document that does not say, and the LIVE desk always
    /// says, so `desk_view` fills every one of them.
    pub phase: Option<u64>,
    /// See [`DeskSnap::to_theory`].
    pub to_theory: Option<&'a [bool]>,
    /// See [`DeskSnap::routes`].
    pub routes: Option<&'a crate::routing::Routes>,
    /// See [`DeskSnap::insert_keys`].
    pub insert_keys: Option<&'a crate::routing::KeyEdges>,
    /// See [`DeskSnap::bypass`].
    pub bypass: Option<&'a crate::settings::BypassBays>,
    /// See [`DeskSnap::instruments`].
    pub instruments: Option<InstrumentsRef<'a>>,
    /// The second late wave — see the block on [`DeskSnap`]. Borrowed on
    /// the same terms as the six above: the once-a-frame walk clones
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
    /// See [`DeskSnap::frozen`].
    pub frozen: Option<&'a [FrozenLane]>,
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

/// [`Instruments`] borrowed — see [`DeskRef`].
#[derive(Clone, Copy)]
pub struct InstrumentsRef<'a> {
    pub dx7_cartridge: &'a [String],
    pub dx7_patch: &'a [usize],
    pub keys_patch: &'a [usize],
    pub keys_params: &'a [f64],
}

impl DeskRef<'_> {
    /// The one place a desk is copied into an owned snapshot.
    pub fn to_snap(&self) -> DeskSnap {
        DeskSnap {
            kinds: self.kinds.to_vec(),
            icons: self.icons.to_vec(),
            names: self.names.to_vec(),
            colors: self.colors.to_vec(),
            inserts: self.inserts.iter().map(|(n, p)| (n, p.to_owned())).collect(),
            sends: self.sends.to_vec(),
            muted: self.muted,
            soloed: self.soloed,
            gains: self.gains.to_vec(),
            pans: self.pans.to_vec(),
            inputs: self.inputs.to_vec(),
            midi_inputs: self.midi_inputs.to_vec(),
            drums_gain: self.drums_gain,
            drums_pan: self.drums_pan,
            metronome_gain: self.metronome_gain,
            master_gain: self.master_gain,
            fx_return: self.fx_return,
            tempo_bpm: self.tempo_bpm,
            sig_beats: self.sig_beats,
            sig_unit: self.sig_unit,
            metronome_on: self.metronome_on,
            tracks_added: self.tracks_added,
            is_folder: self.is_folder.to_vec(),
            in_folder: self.in_folder.to_vec(),
            folder_shut: self.folder_shut.to_vec(),
            armed: self.armed,
            phase: self.phase,
            to_theory: self.to_theory.map(<[bool]>::to_vec),
            routes: self.routes.cloned(),
            insert_keys: self.insert_keys.cloned(),
            bypass: self.bypass.cloned(),
            instruments: self.instruments.map(|i| Instruments {
                dx7_cartridge: i.dx7_cartridge.to_vec(),
                dx7_patch: i.dx7_patch.to_vec(),
                keys_patch: i.keys_patch.to_vec(),
                keys_params: i.keys_params.to_vec(),
            }),
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
            frozen: self.frozen.map(<[FrozenLane]>::to_vec),
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
    fn hash_into(&self, f: &mut Fingerprint) {
        f.len(self.kinds.len());
        for k in self.kinds {
            f.u64(u64::from(*k));
        }
        f.len(self.icons.len());
        for i in self.icons {
            f.u64(u64::from(*i));
        }
        f.len(self.names.len());
        for n in self.names {
            f.str(n);
        }
        f.len(self.colors.len());
        for c in self.colors {
            f.u64(*c as u64);
        }
        // The occupied seats, counted first so the words parse back into
        // exactly one desk, then each as its bay and its path. The same
        // pairs from a dense settings vec and from a snapshot, and the
        // same pairs the writer puts in the head and the tail — a desk of
        // thirteen thousand empty seats is a handful of words now.
        f.len(self.inserts.iter().count());
        for (bay, path) in self.inserts.iter() {
            f.len(bay);
            f.str(path);
        }
        f.len(self.sends.len());
        for s in self.sends {
            f.f64(*s);
        }
        f.u64(self.muted);
        f.u64(self.soloed);
        f.len(self.gains.len());
        for g in self.gains {
            f.f64(*g);
        }
        f.len(self.pans.len());
        for p in self.pans {
            f.f64(*p);
        }
        f.len(self.inputs.len());
        for i in self.inputs {
            f.str(i);
        }
        // The device sets, in the hash for the reason everything else is: the
        // dirty bit is what offers to save, and a setup somebody built in the
        // MIDI Inputs dialog that the document did not notice is a setup they
        // lose on the next close. 255 for "every channel" — never a real wire
        // channel, so it cannot collide with one.
        f.len(self.midi_inputs.len());
        for sel in self.midi_inputs {
            f.len(sel.devices.len());
            for d in &sel.devices {
                f.str(d);
            }
            f.u64(u64::from(sel.channel.unwrap_or(255)));
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
        f.u64(u64::from(self.tracks_added));
        for v in [self.is_folder, self.in_folder, self.folder_shut] {
            f.len(v.len());
            for b in v {
                f.bool(*b);
            }
        }
        f.u64(u64::from(self.armed));
        // **The six that arrived late.** Each hashes its ABSENCE distinctly
        // from an empty one — a document that does not carry routes and a
        // document that carries none are different documents, and the whole
        // meaning of `None` here rests on telling them apart.
        let opt = |f: &mut Fingerprint, present: bool| f.bool(present);
        opt(f, self.phase.is_some());
        f.u64(self.phase.unwrap_or(0));
        opt(f, self.to_theory.is_some());
        if let Some(v) = self.to_theory {
            f.len(v.len());
            for b in v {
                f.bool(*b);
            }
        }
        opt(f, self.routes.is_some());
        if let Some(r) = self.routes {
            f.len(r.0.len());
            for route in &r.0 {
                f.u64(route.from as u64);
                match route.target {
                    crate::routing::Target::Hardware { first, stereo } => {
                        f.u64(0);
                        f.u64(first as u64);
                        f.bool(stereo);
                    }
                    crate::routing::Target::Channel(n) => {
                        f.u64(1);
                        f.u64(n as u64);
                    }
                }
                f.f64(f64::from(route.gain));
            }
        }
        opt(f, self.insert_keys.is_some());
        if let Some(k) = self.insert_keys {
            f.len(k.0.len());
            for e in &k.0 {
                f.u64(e.strip as u64);
                f.u64(e.slot as u64);
                f.u64(e.from as u64);
            }
        }
        opt(f, self.bypass.is_some());
        if let Some(b) = self.bypass {
            f.len(b.0.len());
            for (seat, bay) in &b.0 {
                f.u64(*seat as u64);
                f.u64(*bay as u64);
            }
        }
        opt(f, self.instruments.is_some());
        if let Some(i) = self.instruments {
            f.len(i.dx7_cartridge.len());
            for c in i.dx7_cartridge {
                f.str(c);
            }
            f.len(i.dx7_patch.len());
            for p in i.dx7_patch {
                f.u64(*p as u64);
            }
            f.len(i.keys_patch.len());
            for p in i.keys_patch {
                f.u64(*p as u64);
            }
            f.len(i.keys_params.len());
            for v in i.keys_params {
                f.f64(*v);
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
        opt(f, self.frozen.is_some());
        if let Some(fr) = self.frozen {
            f.u64(fr.len() as u64);
            for z in fr {
                f.u64(z.lane as u64);
                f.str(&z.path);
                f.u64(z.len_s.to_bits());
            }
        }
        // **Appended, and that is the rule for every field added here**: the
        // fingerprint is a walk in declaration order, so a term inserted
        // among the others moves every existing document's hash and reads as
        // "everything changed" the first time an old session is opened.
        //
        // **And a term here is an UNDOABLE field.** This walk is what an
        // undo moment is compared by (`TangentApp::walk`), so adding one is
        // three jobs, not one: append it here; give every gesture that
        // writes it exactly one commit (or make it part of what
        // `commit_view_change` commits once a frame: `View`, `Feel` and the
        // view on show); and make
        // `apply_desk_snapshot` restore every value it can hold, sentinels
        // included - a value the apply declines leaves undo re-applying
        // that moment on every press.
        opt(f, self.look.is_some());
        if let Some(l) = self.look {
            f.str(l.theme);
            f.u64(l.palette as u64);
            f.bool(l.palette_auto);
            // Appended, per the rule above: a term inserted among the
            // others moves every existing document's hash.
            f.str(l.view);
        }
        // The arrangement's grid and window, appended by the same rule.
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
            // Appended after the block's first eleven, by the same rule.
            f.f64(v.visible_at);
            f.f64(v.quantize_sensitivity);
            f.str(if v.quantize_fit == "auto" { "" } else { v.quantize_fit });
        }
    }
}

impl DeskSnap {
    /// This snapshot by reference — see [`DeskRef`].
    pub fn as_ref(&self) -> DeskRef<'_> {
        DeskRef {
            kinds: &self.kinds,
            icons: &self.icons,
            names: &self.names,
            colors: &self.colors,
            inserts: Bays::Sparse(&self.inserts),
            sends: &self.sends,
            muted: self.muted,
            soloed: self.soloed,
            gains: &self.gains,
            pans: &self.pans,
            inputs: &self.inputs,
            midi_inputs: &self.midi_inputs,
            drums_gain: self.drums_gain,
            drums_pan: self.drums_pan,
            metronome_gain: self.metronome_gain,
            master_gain: self.master_gain,
            fx_return: self.fx_return,
            tempo_bpm: self.tempo_bpm,
            sig_beats: self.sig_beats,
            sig_unit: self.sig_unit,
            metronome_on: self.metronome_on,
            tracks_added: self.tracks_added,
            is_folder: &self.is_folder,
            in_folder: &self.in_folder,
            folder_shut: &self.folder_shut,
            armed: self.armed,
            phase: self.phase,
            to_theory: self.to_theory.as_deref(),
            routes: self.routes.as_ref(),
            insert_keys: self.insert_keys.as_ref(),
            bypass: self.bypass.as_ref(),
            instruments: self.instruments.as_ref().map(|i| InstrumentsRef {
                dx7_cartridge: &i.dx7_cartridge,
                dx7_patch: &i.dx7_patch,
                keys_patch: &i.keys_patch,
                keys_params: &i.keys_params,
            }),
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
            frozen: self.frozen.as_deref(),
        }
    }

    /// What stands in seat `bay`, if anything. Binary search, since the
    /// pairs are sorted.
    pub fn bay(&self, bay: usize) -> Option<&str> {
        self.inserts
            .binary_search_by_key(&bay, |(n, _)| *n)
            .ok()
            .and_then(|i| self.inserts.get(i))
            .map(|(_, p)| p.as_str())
    }

    /// Put `path` in seat `bay` — or empty the seat, for an empty path —
    /// keeping the pairs sorted and free of empties.
    pub fn set_bay(&mut self, bay: usize, path: &str) {
        match self.inserts.binary_search_by_key(&bay, |(n, _)| *n) {
            Ok(i) if path.is_empty() => {
                self.inserts.remove(i);
            }
            Ok(i) => {
                if let Some(slot) = self.inserts.get_mut(i) {
                    slot.1 = path.to_owned();
                }
            }
            Err(_) if path.is_empty() => {}
            Err(i) => self.inserts.insert(i, (bay, path.to_owned())),
        }
    }

    /// Sorted, deduplicated (the first of two claims on a seat wins), free
    /// of empties and inside the seat map — the one shape `inserts` is
    /// allowed to have.
    ///
    /// **The bound is the point.** A dense array's seat count was limited
    /// by the file (one `"",` per seat); a sparse key is not, and every
    /// consumer spreads these pairs over a dense space sized by the bay —
    /// so `"inserts_tail": {"100000000000": "x"}` was a 20-byte request
    /// for 2.4 TB of strings, and `usize::MAX` an add that wraps to zero
    /// in release and emptied the live desk instead. Neither shape can
    /// reach the quarantine (both are valid JSON). The settings reader
    /// already refuses a `strip_inserts_tail` seat past `INSERT_SPACE`;
    /// this is the same law on the document road. What is given up is a
    /// future build's seat past today's map surviving an open here — and
    /// the settings mirror was already dropping that at the next launch,
    /// so it was never a real round trip.
    fn canonical(mut pairs: Vec<(usize, String)>) -> Vec<(usize, String)> {
        pairs.retain(|(bay, p)| !p.is_empty() && *bay < crate::recorder::INSERT_SPACE);
        pairs.sort_by_key(|(n, _)| *n);
        pairs.dedup_by_key(|(n, _)| *n);
        pairs
    }

    fn to_value(&self) -> Value {
        let mut m = Map::new();
        let nums = |v: &[f64]| {
            Value::Array(v.iter().map(|x| serde_json::json!(x)).collect())
        };
        m.insert(
            "kinds".into(),
            Value::Array(self.kinds.iter().map(|k| Value::Number((*k as u64).into())).collect()),
        );
        // Always written: an all-zero row is an OPINION (every badge
        // auto), and omitting it let the previous project's badges bleed
        // into a document that carried none.
        m.insert(
            "icons".into(),
            Value::Array(
                self.icons.iter().map(|k| Value::Number((*k as u64).into())).collect(),
            ),
        );
        m.insert(
            "names".into(),
            Value::Array(self.names.iter().map(|n| Value::String(n.clone())).collect()),
        );
        m.insert(
            "colors".into(),
            Value::Array(self.colors.iter().map(|c| Value::Number((*c).into())).collect()),
        );
        // The head whole, the far seats sparse — see `V7_SPARSE_SEATS`.
        let mut head = vec![Value::String(String::new()); HEAD_BAYS];
        let mut tail = Map::new();
        for (bay, path) in &self.inserts {
            match head.get_mut(*bay) {
                Some(slot) => *slot = Value::String(path.clone()),
                None => {
                    tail.insert(bay.to_string(), Value::String(path.clone()));
                }
            }
        }
        m.insert("inserts".into(), Value::Array(head));
        m.insert("sends".into(), nums(&self.sends));
        m.insert("muted".into(), Value::Number(u64::from(self.muted).into()));
        m.insert("soloed".into(), Value::Number(u64::from(self.soloed).into()));
        m.insert("gains".into(), nums(&self.gains));
        m.insert("pans".into(), nums(&self.pans));
        m.insert(
            "inputs".into(),
            Value::Array(self.inputs.iter().map(|i| Value::String(i.clone())).collect()),
        );
        m.insert("drums_gain".into(), serde_json::json!(self.drums_gain));
        m.insert("drums_pan".into(), serde_json::json!(self.drums_pan));
        m.insert("metronome_gain".into(), serde_json::json!(self.metronome_gain));
        m.insert("master_gain".into(), serde_json::json!(self.master_gain));
        m.insert("fx_return".into(), serde_json::json!(self.fx_return));
        m.insert("tempo_bpm".into(), serde_json::json!(self.tempo_bpm));
        m.insert("sig_beats".into(), Value::Number(u64::from(self.sig_beats).into()));
        m.insert("sig_unit".into(), Value::Number(u64::from(self.sig_unit).into()));
        m.insert("metronome_on".into(), Value::Bool(self.metronome_on));
        // Appended, at the END — the settings file's own rule, and this
        // struct answers to it for the same reason.
        m.insert("tracks_added".into(), Value::Number(u64::from(self.tracks_added).into()));
        // Always written, all-false included: like the icons above, a flat
        // desk is an OPINION, and omitting it would let the previous
        // session's folders bleed into a document that carries none.
        let bools =
            |v: &[bool]| Value::Array(v.iter().map(|b| Value::Bool(*b)).collect());
        m.insert("is_folder".into(), bools(&self.is_folder));
        m.insert("in_folder".into(), bools(&self.in_folder));
        m.insert("folder_shut".into(), bools(&self.folder_shut));
        // **The MIDI device sets, at the END and sparse** — the settings
        // file's own rule and its own shape, so a document from a desk
        // nobody has narrowed keeps its byte-for-byte form and an older
        // build meets no key it has to know to ignore.
        if let Some(v) = crate::recorder::write_midi_inputs(&self.midi_inputs) {
            m.insert("midi_inputs".into(), v);
        }
        // **The six late fields, at the END and only when they are SAID.**
        //
        // `None` is a snapshot taken from a document that predates the key
        // (see the block on the struct), and writing a default for it would
        // invent an opinion the document never held. A LIVE desk always has
        // one — `desk_view` fills every field — so a document saved by this
        // build carries all six, and one that carries none is one nobody
        // has re-saved yet.
        if let Some(v) = self.phase {
            m.insert("phase".into(), Value::Number(v.into()));
        }
        if let Some(v) = self.to_theory.as_ref() {
            m.insert(
                "to_theory".into(),
                Value::Array(v.iter().map(|b| Value::Bool(*b)).collect()),
            );
        }
        if let Some(v) = self.routes.as_ref() {
            m.insert("routes".into(), v.to_json());
        }
        if let Some(v) = self.insert_keys.as_ref() {
            m.insert("insert_keys".into(), v.to_json());
        }
        if let Some(v) = self.bypass.as_ref() {
            m.insert("insert_bypass".into(), v.to_json());
        }
        if let Some(i) = self.instruments.as_ref() {
            let mut im = Map::new();
            im.insert(
                "dx7_cartridge".into(),
                Value::Array(
                    i.dx7_cartridge.iter().map(|c| Value::String(c.clone())).collect(),
                ),
            );
            let nums = |v: &[usize]| {
                Value::Array(v.iter().map(|n| Value::from(*n as u64)).collect())
            };
            im.insert("dx7_patch".into(), nums(&i.dx7_patch));
            im.insert("keys_patch".into(), nums(&i.keys_patch));
            im.insert(
                "keys_params".into(),
                Value::Array(i.keys_params.iter().map(|v| serde_json::json!(v)).collect()),
            );
            m.insert("instruments".into(), Value::Object(im));
        }
        // **The ARM mask, at the END and always written**, all-zero
        // included. A desk with nothing armed is an OPINION — the same
        // argument the folder bools above make — and omitting it would send
        // the reader to `armed_by_default`, which would arm the piano on a
        // project somebody deliberately left silent.
        m.insert("armed".into(), Value::Number(u64::from(self.armed).into()));
        // Appended, at the END, and only when a seat past the head is
        // occupied: a desk that never had one keeps its shape, and a build
        // that has never heard of the key reads the head as it always did.
        if !tail.is_empty() {
            m.insert("inserts_tail".into(), Value::Object(tail));
        }
        // **The second late wave, at the END and only when SAID** — the six
        // above's rule, word for word, and appended after `inserts_tail` so
        // that not one key a previous build wrote moves position. A LIVE
        // desk always says, so a document saved by this build carries all
        // eleven and wins outright from then on.
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
        if let Some(fr) = self.frozen.as_ref() {
            let rows = fr
                .iter()
                .map(|z| {
                    let mut zm = Map::new();
                    zm.insert("lane".into(), Value::Number((z.lane as u64).into()));
                    zm.insert("path".into(), Value::String(z.path.clone()));
                    zm.insert(
                        "len_s".into(),
                        serde_json::Number::from_f64(z.len_s)
                            .map_or(Value::Null, Value::Number),
                    );
                    Value::Object(zm)
                })
                .collect();
            m.insert("frozen".into(), Value::Array(rows));
        }
        Value::Object(m)
    }

    fn from_value(v: &Value) -> Option<Self> {
        let m = v.as_object()?;
        let f = |k: &str, d: f64| m.get(k).and_then(Value::as_f64).unwrap_or(d);
        let strings = |k: &str| -> Vec<String> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .map(|x| x.as_str().unwrap_or_default().to_owned())
                        .collect()
                })
                .unwrap_or_default()
        };
        let floats = |k: &str| -> Vec<f64> {
            m.get(k)
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_f64).collect())
                .unwrap_or_default()
        };
        // Absent in every document before the folder fields — and absent
        // means FLAT (see the struct), so the default is the empty vec
        // that `apply_desk_snapshot` reads as all-false.
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
                    &mut sends_v,
                    &mut colors_v,
                    &mut names_v,
                    &mut dense,
                    &mut muted_v,
                    &mut soloed_v,
                );
                dense
                    .into_iter()
                    .enumerate()
                    .filter(|(_, p)| !p.is_empty())
                    .collect()
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
        let bay_at = |n: usize| {
            inserts.binary_search_by_key(&n, |(b, _)| *b).is_ok()
        };
        Some(Self {
            kinds: kinds_v.clone(),
            icons: m
                .get("icons")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n.min(u64::from(crate::recorder_panel::ICON_MAX)) as u8)).collect())
                .unwrap_or_default(),
            names: names_v.clone(),
            colors: colors_v.clone(),
            sends: sends_v,
            muted: muted_v,
            soloed: soloed_v,
            gains: floats("gains"),
            pans: floats("pans"),
            inputs: strings("inputs"),
            // **Absent in every older document, and derived from `inputs`
            // rather than assumed empty** — the `tracks_added` rule below,
            // for the same reason: a document that named a port per track
            // still says which keyboards those tracks wanted, and reading
            // nothing would silently widen every one of them to the whole
            // room. `reconcile_midi_inputs` does both jobs at once, because
            // an older build that rewrote `inputs` and carried this key
            // through unread leaves exactly the same disagreement.
            midi_inputs: {
                let mut sels = crate::recorder::read_midi_inputs(m.get("midi_inputs"));
                crate::recorder::reconcile_midi_inputs(&mut sels, &inputs_v);
                sels
            },
            drums_gain: f("drums_gain", 1.0),
            drums_pan: f("drums_pan", 0.0),
            metronome_gain: f("metronome_gain", 0.7),
            master_gain: f("master_gain", 1.0),
            fx_return: f("fx_return", 1.0),
            tempo_bpm: f("tempo_bpm", 120.0),
            sig_beats: m.get("sig_beats").and_then(Value::as_u64).unwrap_or(4) as u8,
            sig_unit: m.get("sig_unit").and_then(Value::as_u64).unwrap_or(4) as u8,
            metronome_on: m
                .get("metronome_on")
                .and_then(Value::as_bool)
                .unwrap_or(false),
            // **Absent in older documents, and reconstructed rather than
            // assumed empty.** A v4 desk said which rows it described by
            // describing them: a channel with a kind, a name, a colour, an
            // input or a plugin in a bay is a channel that was there.
            // Defaulting to zero would open every old project with no
            // tracks on it. Read AFTER the era lifts, so a channel row is
            // a channel row — before, an eight-channel-era desk's drum
            // strip sat where channel 8 does today and could pass for one.
            tracks_added: m
                .get("tracks_added")
                .and_then(Value::as_u64)
                .map_or_else(
                    || {
                        let mut bits = 0u32;
                        for n in 0..crate::recorder::CHANNELS.min(32) {
                            let kind = kinds_v.get(n).copied().unwrap_or(0) != 0;
                            let at = crate::recorder::Strip::Channel(n).index();
                            let named =
                                names_v.get(at).is_some_and(|s| !s.is_empty());
                            let coloured = colors_v.get(at).copied().unwrap_or(0) != 0;
                            let input =
                                inputs_v.get(n).is_some_and(|s| !s.is_empty());
                            let bay = (0..crate::recorder::INSERTS)
                                .any(|b| bay_at(at * crate::recorder::INSERTS + b));
                            if kind || named || coloured || input || bay {
                                bits |= 1 << n;
                            }
                        }
                        bits
                    },
                    |v| v as u32,
                ),
            is_folder: bools("is_folder"),
            in_folder: bools("in_folder"),
            folder_shut: bools("folder_shut"),
            // **Absent in older documents, and DERIVED rather than assumed
            // empty** — the `tracks_added` bargain one field up, for the
            // same two reasons. A project from before the arm was written
            // down opens the way it always opened (the DX7's channel live,
            // nothing else), and the fingerprint sees the same mask the live
            // desk resolved to, so an old document does not come up wearing
            // an asterisk it did not earn. Read AFTER the era lifts, like
            // `tracks_added`, so a channel row is a channel row.
            armed: m.get("armed").and_then(Value::as_u64).map_or_else(
                || {
                    armed_by_default(&kinds_v, |n| {
                        // `bay_at` only answers WHETHER a seat is occupied;
                        // the rule asks WHAT is in it, so the pairs are read
                        // directly. Seat 0 of the channel's own rack, which
                        // is where an instrument sits.
                        let seat = crate::recorder::Strip::Channel(n).index()
                            * crate::recorder::INSERTS;
                        inserts
                            .binary_search_by_key(&seat, |(b, _)| *b)
                            .ok()
                            .and_then(|i| inserts.get(i))
                            .is_some_and(|(_, p)| p == crate::dialogs::BUILTIN_PATH)
                    })
                },
                |v| v as u32,
            ),
            // **The six late fields: absent is `None`, which means the
            // document does not SAY** — see the block on the struct. Not
            // "empty", and not a default: the live value stands for a
            // document written before the key, so nothing an old project
            // sounds like changes because a field was added.
            phase: m.get("phase").and_then(Value::as_u64),
            to_theory: m.get("to_theory").and_then(Value::as_array).map(|a| {
                a.iter().map(|v| v.as_bool().unwrap_or(false)).collect()
            }),
            routes: m.get("routes").map(crate::routing::Routes::from_json),
            insert_keys: m
                .get("insert_keys")
                .map(crate::routing::KeyEdges::from_json),
            bypass: m
                .get("insert_bypass")
                .map(crate::settings::BypassBays::from_json),
            instruments: m.get("instruments").and_then(Value::as_object).map(|im| {
                let strings = |k: &str| {
                    im.get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter()
                                .map(|v| v.as_str().unwrap_or_default().to_owned())
                                .collect()
                        })
                        .unwrap_or_default()
                };
                let counts = |k: &str| {
                    im.get(k)
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter().map(|v| v.as_u64().unwrap_or(0) as usize).collect()
                        })
                        .unwrap_or_default()
                };
                Instruments {
                    dx7_cartridge: strings("dx7_cartridge"),
                    dx7_patch: counts("dx7_patch"),
                    keys_patch: counts("keys_patch"),
                    keys_params: im
                        .get("keys_params")
                        .and_then(Value::as_array)
                        .map(|a| {
                            a.iter().map(|v| v.as_f64().unwrap_or(0.0)).collect()
                        })
                        .unwrap_or_default(),
                }
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
            frozen: m.get("frozen").and_then(Value::as_array).map(|rows| {
                rows.iter()
                    .filter_map(|r| {
                        let zm = r.as_object()?;
                        let lane = usize::try_from(zm.get("lane")?.as_u64()?).ok()?;
                        let path = zm.get("path")?.as_str()?.to_owned();
                        let len_s = zm.get("len_s").and_then(Value::as_f64).unwrap_or(0.0);
                        (lane < crate::recorder::CHANNELS && !path.is_empty() && len_s.is_finite())
                            .then_some(FrozenLane { lane, path, len_s: len_s.max(0.0) })
                    })
                    .collect()
            }),
            inserts,
        })
    }
}

/// **A desk written in an earlier ERA of the desk's shape, grown to
/// today's.** Four lifts, each keyed on the LENGTH the file was written at
/// — the sends count is a strip count, the bay count a seat map — and each
/// frozen at its era's literals, per the migration rule: this read the live
/// constants once and went dead the day the desk grew to 24, stranding that
/// beta window's documents unlifted.
///
/// Here in the READER, on the dense array as the file holds it, and not in
/// `apply_desk_snapshot` where they used to run on a clone of every desk
/// applied — every open and every undo step — although a moment can never
/// hold an old-era desk. A property of how the FILE was written belongs
/// beside the `tracks_added` reconstruction, which is the same kind of
/// fact. `a_desk_from_any_era_opens_at_todays_seats` pins each era through
/// the live desk, which is the same observation whichever function lifts.
fn lift_eras(
    sends: &mut Vec<f64>,
    colors: &mut Vec<i64>,
    names: &mut Vec<String>,
    inserts: &mut Vec<String>,
    muted: &mut u64,
    soloed: &mut u64,
) {
    // A document saved before the DRUM BUS carries eighteen-strip arrays:
    // its click, bus and master rows would land one seat early. Frozen at
    // the era's numbers (16 channels, 18 strips).
    const V18_CH: usize = 16;
    if sends.len() == 18 {
        sends.insert(V18_CH, 0.0);
        if colors.len() == 19 {
            colors.insert(V18_CH, 0);
        }
        if names.len() == 19 {
            names.insert(V18_CH, String::new());
        }
        if inserts.len() == 19 * crate::recorder::INSERTS {
            for _ in 0..crate::recorder::INSERTS {
                inserts.insert(V18_CH * crate::recorder::INSERTS, String::new());
            }
        }
        let move_mask = |mask: u64| -> u64 {
            let keep = mask & ((1u64 << V18_CH) - 1);
            let above = mask >> V18_CH;
            keep | (above << (V18_CH + 1))
        };
        *muted = move_mask(*muted);
        *soloed = move_mask(*soloed);
    }
    // **An eight-channel-era document, grown to today's count.** The
    // settings path does this in `adopt_desk`; a project carries its own
    // desk snapshot and needs the same lift, or opening a session saved at
    // eight channels scatters its drums/click/fx/master and every pad's FX
    // onto the new empty channels. Frozen eight-channel shape: STRIPS 11,
    // master at 11, pad-insert base 12.
    const OLD_CH: usize = 8;
    const OLD_STRIPS: usize = 11;
    let ins3 = crate::recorder::INSERTS;
    // A sixteen-pad-era doc first grows its pads to 128 at base 12 — its
    // pads were keys 36..51 — before the channels widen the base.
    if inserts.len() == (OLD_STRIPS + 1 + 16) * ins3 {
        inserts.resize((OLD_STRIPS + 1 + 128) * ins3, String::new());
        for pad in (0..16usize).rev() {
            for bay in 0..ins3 {
                let from = (OLD_STRIPS + 1 + pad) * ins3 + bay;
                let to = (OLD_STRIPS + 1 + 36 + pad) * ins3 + bay;
                let moved = std::mem::take(&mut inserts[from]);
                inserts[to] = moved;
            }
        }
    }
    if sends.len() == OLD_STRIPS && crate::recorder::CHANNELS > OLD_CH {
        let grow = crate::recorder::CHANNELS - OLD_CH;
        for _ in 0..grow {
            if OLD_CH <= colors.len() {
                colors.insert(OLD_CH, 0);
            }
            if OLD_CH <= names.len() {
                names.insert(OLD_CH, String::new());
            }
            if OLD_CH <= sends.len() {
                sends.insert(OLD_CH, 0.0);
            }
        }
        let ins_at = OLD_CH * ins3;
        for _ in 0..(grow * ins3) {
            if ins_at <= inserts.len() {
                inserts.insert(ins_at, String::new());
            }
        }
        let grow_mask = |mask: u64| -> u64 {
            let keep = mask & ((1u64 << OLD_CH) - 1);
            let above = mask >> OLD_CH;
            keep | (above << (OLD_CH + grow))
        };
        *muted = grow_mask(*muted);
        *soloed = grow_mask(*soloed);
        // Not the global `desk_state_shift_v19` flag: that renames the
        // global state dir, and a project's insert states live in its own
        // `state/`. The desk ARRAYS relocate here so plugins load at the
        // right seats; a moved plugin's saved knobs fall back to defaults,
        // the same graceful cost every shuffle accepts.
    }
    // **A sixteen-channel-era document, grown to twenty-four.** The
    // 4.82-through-5.0 shape: nineteen sends is its fingerprint. Same fold,
    // same bargain on the project's own state/ sidecars.
    const V19_CH: usize = 16;
    if sends.len() == 19 && crate::recorder::CHANNELS > V19_CH {
        let grow = crate::recorder::CHANNELS - V19_CH;
        for _ in 0..grow {
            if V19_CH <= colors.len() {
                colors.insert(V19_CH, 0);
            }
            if V19_CH <= names.len() {
                names.insert(V19_CH, String::new());
            }
            if V19_CH <= sends.len() {
                sends.insert(V19_CH, 0.0);
            }
        }
        let ins_at = V19_CH * ins3;
        for _ in 0..(grow * ins3) {
            if ins_at <= inserts.len() {
                inserts.insert(ins_at, String::new());
            }
        }
        let grow_mask = |mask: u64| -> u64 {
            let keep = mask & ((1u64 << V19_CH) - 1);
            let above = mask >> V19_CH;
            keep | (above << (V19_CH + grow))
        };
        *muted = grow_mask(*muted);
        *soloed = grow_mask(*soloed);
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
        if let Some(p) = std::env::var_os("IVORY_ARRANGEMENT_PATH") {
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
        match Self::read_from(path) {
            LoadOutcome::Loaded(a) => a,
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
            Ok(Value::Object(map)) => LoadOutcome::Loaded(Self::from_map(map)),
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
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("tangent.tmp");
        if std::fs::write(&tmp, self.to_json()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
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

    fn from_map(mut map: Map<String, Value>) -> Self {
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
        let desk = map.shift_remove("desk").as_ref().and_then(DeskSnap::from_value);
        // **The version-4 migration, and the document's first real one.**
        //
        // A v4 file has no maps and one tempo, which lives on the desk
        // snapshot. Reading it as a map of one point is the whole migration —
        // and skipping it would open a song written at 90 at 120, which is
        // the sort of thing somebody notices as "it sounds wrong" and never
        // reports as a bug.
        let (tempo, meter) = Self::read_maps(&mut map, version, desk.as_ref());
        let mut automation = Self::read_automation(&mut map);
        // **The master lane moved when the desk grew.** MASTER_LANE is the
        // channel count, so documents written at sixteen channels keep
        // their master curves at lane 16 — an ordinary channel on the
        // twenty-four desk. Version 6 is the stamp that says "my master is
        // at today's lane"; anything older is lifted here, at the frozen
        // literal, per the migration rule.
        if version < 6 {
            automation.remap_lane(16, crate::recorder::CHANNELS);
        }
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
        Self {
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
        }
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
    fn read_automation(map: &mut Map<String, Value>) -> crate::automation::Automation {
        use crate::automation::{AutoParam, Automation, Curve, Point, Shape};
        let Some(Value::Array(list)) = map.shift_remove("automation") else {
            return Automation::default();
        };
        let curves = list
            .iter()
            .filter_map(|v| {
                let o = v.as_object()?;
                let lane = usize::try_from(o.get("lane")?.as_u64()?).ok()?;
                // A curve naming a channel this desk does not have is
                // DROPPED rather than clamped onto channel 15: a level
                // landing on the wrong instrument is worse than one that
                // does not land at all, and it would be silent.
                //
                // **The MASTER is one past the channels and is real.** It is
                // a bus rather than one of the sixteen, and it keeps its
                // curves under `arrangement_panel::MASTER_LANE` — so the
                // bound is "past the master", not "past the channels", or
                // the master's automation is silently discarded every time
                // the document is read back.
                if lane > crate::recorder::CHANNELS {
                    return None;
                }
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
        let mut map = Map::new();
        // The document's OWN version, not this build's constant. See
        // [`Arrangement::version`]: stamping 4 onto a file still carrying a
        // version-9 build's keys — which `extra` faithfully preserves —
        // would tell that build its own migrations had not run yet.
        map.insert(
            "arrangement_version".into(),
            Value::Number(self.version.into()),
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
            map.insert("desk".into(), desk.to_value());
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
                            o.insert("lane".into(), Value::Number((c.lane as u64).into()));
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
                f.len(c.lane);
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
        assert_eq!(a.automation.curves()[0].lane, 1);
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
    /// across four passes is one edge, not four.
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
            vec![10.5],
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
        a.desk = Some(DeskSnap {
            kinds: vec![1, 2, 0],
            icons: vec![0, 3],
            names: vec!["drums".into(), String::new(), "gtr".into()],
            colors: vec![3, 0, 11],
            inserts: vec![(1, "reverb".into()), (4149 * 3, "brusfri".into())],
            sends: vec![0.0, 0.25],
            muted: 0b0010,
            soloed: 0b0100,
            gains: vec![1.0, 0.5],
            pans: vec![0.0, -0.5],
            inputs: vec!["midi:Keystation 88".into(), String::new()],
            midi_inputs: vec![crate::recorder::MidiInputSel {
                devices: vec!["Keystation 88".into()],
                channel: Some(2),
            }],
            drums_gain: 0.9,
            drums_pan: 0.1,
            metronome_gain: 0.7,
            master_gain: 1.0,
            fx_return: 0.8,
            tempo_bpm: 96.0,
            sig_beats: 3,
            sig_unit: 4,
            metronome_on: true,
            tracks_added: 0b0111,
            is_folder: vec![true, false],
            in_folder: vec![false, true],
            folder_shut: vec![false, false],
            armed: 0b0101,
            // The six late fields, every one of them SAID — this fixture is
            // the round-trip's subject and a `None` here would test the
            // absent road twice and the present one never.
            phase: Some(0b0010),
            to_theory: Some(vec![true, false, true]),
            routes: Some(crate::routing::Routes(vec![crate::routing::Route {
                from: 1,
                target: crate::routing::Target::Hardware { first: 2, stereo: true },
                gain: 0.8,
            }])),
            insert_keys: Some(crate::routing::KeyEdges(vec![crate::routing::KeyEdge {
                strip: 3,
                slot: 1,
                from: 0,
            }])),
            bypass: Some(crate::settings::BypassBays(vec![(4, 2)])),
            instruments: Some(Instruments {
                dx7_cartridge: vec!["ROM1A.syx".into(), String::new()],
                dx7_patch: vec![7, 0],
                keys_patch: vec![2, 3],
                keys_params: vec![0.25, 0.5],
            }),
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
            frozen: Some(vec![FrozenLane {
                lane: 1,
                path: "media/frozen-02-1.wav".into(),
                len_s: 12.5,
            }]),
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
        let seat = crate::recorder::Strip::Channel(1).index() * crate::recorder::INSERTS;
        snap.kinds = vec![0, 1, 0];
        snap.set_bay(seat, crate::dialogs::BUILTIN_PATH);
        snap.armed = 1 << 1;

        let mut value = snap.to_value();
        let obj = value.as_object_mut().expect("a desk object");
        assert!(obj.remove("armed").is_some(), "the key was never written");
        let back = DeskSnap::from_value(&value).expect("the desk should read");
        assert_eq!(
            back.armed,
            1 << 1,
            "a document with no arm key did not fall back to the shipped rule"
        );

        // Written, and read back as itself — including the silent desk.
        for mask in [0u32, 1 << 1, 0b1010] {
            snap.armed = mask;
            let read = DeskSnap::from_value(&snap.to_value()).expect("the desk should read");
            assert_eq!(read.armed, mask, "the arm mask did not round trip");
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
        moves_both("a channel kind", |a| a.desk.as_mut().unwrap().kinds[0] = 2);
        moves_both("a channel badge", |a| a.desk.as_mut().unwrap().icons[1] = 5);
        moves_both("a strip name", |a| {
            a.desk.as_mut().unwrap().names[0] = "kit".into();
        });
        moves_both("a strip colour", |a| a.desk.as_mut().unwrap().colors[2] = 4);
        moves_both("a plugin in a bay", |a| {
            a.desk.as_mut().unwrap().inserts[0].1 = "eq".into();
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
        moves_both("a send", |a| a.desk.as_mut().unwrap().sends[1] = 0.3);
        moves_both("a mute", |a| a.desk.as_mut().unwrap().muted ^= 1);
        moves_both("a solo", |a| a.desk.as_mut().unwrap().soloed ^= 1);
        moves_both("a fader", |a| a.desk.as_mut().unwrap().gains[0] = 0.4);
        moves_both("a pan", |a| a.desk.as_mut().unwrap().pans[0] = 0.2);
        moves_both("an input", |a| {
            a.desk.as_mut().unwrap().inputs[1] = "in 2".into();
        });
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
        moves_both("which rows exist", |a| a.desk.as_mut().unwrap().tracks_added = 1);
        moves_both("a folder", |a| a.desk.as_mut().unwrap().is_folder[1] = true);
        moves_both("what is inside a folder", |a| {
            a.desk.as_mut().unwrap().in_folder[0] = true;
        });
        moves_both("a shut folder", |a| {
            a.desk.as_mut().unwrap().folder_shut[0] = true;
        });
        moves_both("an ARM", |a| a.desk.as_mut().unwrap().armed ^= 1);
        // The six that arrived late, each moved and each REMOVED: absence is
        // a different document from an empty one, and the whole meaning of
        // `None` rests on the walk telling them apart.
        moves_both("a polarity", |a| {
            *a.desk.as_mut().unwrap().phase.as_mut().unwrap() ^= 1;
        });
        moves_both("polarity going unsaid", |a| a.desk.as_mut().unwrap().phase = None);
        moves_both("a theory send", |a| {
            a.desk.as_mut().unwrap().to_theory.as_mut().unwrap()[0] ^= true;
        });
        moves_both("the theory sends going unsaid", |a| {
            a.desk.as_mut().unwrap().to_theory = None;
        });
        moves_both("a route's gain", |a| {
            a.desk.as_mut().unwrap().routes.as_mut().unwrap().0[0].gain = 0.25;
        });
        moves_both("a route's destination", |a| {
            a.desk.as_mut().unwrap().routes.as_mut().unwrap().0[0].target =
                crate::routing::Target::Channel(5);
        });
        moves_both("the routing going unsaid", |a| {
            a.desk.as_mut().unwrap().routes = None;
        });
        moves_both("an empty routing table", |a| {
            a.desk.as_mut().unwrap().routes = Some(crate::routing::Routes::default());
        });
        moves_both("a sidechain key", |a| {
            a.desk.as_mut().unwrap().insert_keys.as_mut().unwrap().0[0].from = 4;
        });
        moves_both("the sidechains going unsaid", |a| {
            a.desk.as_mut().unwrap().insert_keys = None;
        });
        moves_both("a bypassed bay", |a| {
            a.desk.as_mut().unwrap().bypass.as_mut().unwrap().0[0] = (5, 0);
        });
        moves_both("the bypasses going unsaid", |a| {
            a.desk.as_mut().unwrap().bypass = None;
        });
        moves_both("a DX7 patch", |a| {
            a.desk.as_mut().unwrap().instruments.as_mut().unwrap().dx7_patch[0] = 31;
        });
        moves_both("a DX7 cartridge", |a| {
            a.desk.as_mut().unwrap().instruments.as_mut().unwrap().dx7_cartridge[0] =
                "ROM2A.syx".into();
        });
        moves_both("a Keys patch", |a| {
            a.desk.as_mut().unwrap().instruments.as_mut().unwrap().keys_patch[1] = 6;
        });
        moves_both("a Keys knob", |a| {
            a.desk.as_mut().unwrap().instruments.as_mut().unwrap().keys_params[0] = 0.9;
        });
        moves_both("the instruments going unsaid", |a| {
            a.desk.as_mut().unwrap().instruments = None;
        });
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
        moves_both("a lane freezing", |a| {
            desk(a).frozen = Some(vec![FrozenLane {
                lane: 0,
                path: "media/frozen-01-7.wav".into(),
                len_s: 3.0,
            }]);
        });
        moves_both("a frozen lane's file", |a| {
            desk(a).frozen.as_mut().unwrap()[0].path = "media/frozen-02-8.wav".into();
        });
        moves_both("the freeze going unsaid", |a| desk(a).frozen = None);
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
        let d = a.desk.clone().expect("a desk");
        let written = serde_json::from_str::<Value>(&a.to_json()).expect("json");
        let desk = written.get("desk").and_then(Value::as_object).expect("a desk object");
        let head = desk.get("inserts").and_then(Value::as_array).expect("the head array");
        assert_eq!(head.len(), HEAD_BAYS, "the head is not written whole");
        assert_eq!(head[1], Value::String("reverb".into()));
        let tail = desk.get("inserts_tail").and_then(Value::as_object).expect("a tail");
        assert_eq!(tail.get("12447"), Some(&Value::String("brusfri".into())));
        assert_eq!(tail.len(), 1, "a head seat leaked into the tail");
        assert!(a.to_json().matches("\"\"").count() < 200, "thousands of empties again");

        // The dense shape every earlier build wrote.
        let mut dense = vec![Value::String(String::new()); crate::recorder::INSERT_SPACE];
        dense[1] = Value::String("reverb".into());
        dense[12447] = Value::String("brusfri".into());
        let mut old = written.clone();
        let od = old["desk"].as_object_mut().expect("desk");
        od.insert("inserts".into(), Value::Array(dense));
        od.shift_remove("inserts_tail");
        let from_dense = Arrangement::from_json(&old.to_string());
        assert_eq!(from_dense.desk.as_ref(), Some(&d), "the dense shape read differently");
        assert_eq!(from_dense.fingerprint(), a.fingerprint(), "the dense shape hashes apart");
        assert_eq!(from_dense.to_json(), a.to_json(), "the dense shape is not rewritten as v7");

        // The fully sparse object.
        let mut sparse = written.clone();
        let sd = sparse["desk"].as_object_mut().expect("desk");
        sd.insert("inserts".into(), serde_json::json!({"1": "reverb", "12447": "brusfri"}));
        sd.shift_remove("inserts_tail");
        let from_sparse = Arrangement::from_json(&sparse.to_string());
        assert_eq!(from_sparse.desk.as_ref(), Some(&d), "the sparse object read differently");
        assert_eq!(from_sparse.fingerprint(), a.fingerprint());

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
        assert!(read.inserts.windows(2).all(|w| w[0].0 < w[1].0), "the pairs are not sorted");

        // And a key that names a seat past the map, in either sparse
        // shape. A dense array had to SPEND a file byte per seat; a key
        // spends twenty on any bay at all, and everything downstream sizes
        // a dense space by the bay — so these are a terabyte and a wrap to
        // zero, not seats. The last real seat stays, the first past it goes.
        let space = crate::recorder::INSERT_SPACE;
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
                read.inserts.iter().all(|(b, _)| *b < space),
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
        d.inserts.push((crate::recorder::INSERT_SPACE - 1, String::new()));
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
    #[test]
    fn a_round_trip_keeps_the_fingerprint() {
        let a = a_document_with_everything_in_it();
        let json = a.to_json();
        let back = Arrangement::from_json(&json);
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
        let mut a = a_document_with_everything_in_it();
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
