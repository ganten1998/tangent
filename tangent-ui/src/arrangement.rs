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

/// The document's schema version. Bump WITH a migration arm in
/// [`Arrangement::from_map`], never without one.
///
/// **A document that already claims a HIGHER number keeps it.** See
/// [`Arrangement::version`]: writing this constant unconditionally would
/// stamp "version 4" on a file still full of version 9's keys, and the build
/// that wrote them would then run its 4→9 migrations over data that had
/// already been through them.
pub const ARRANGEMENT_VERSION: u64 = 5;

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
        "notes" => v.is_array(),
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

const CLIP_KEYS: [&str; 17] = [
    "kind",
    "lane",
    "path",
    "name",
    "notes",
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
    /// **How long the looped section is**, for a MIDI clip. Zero means the
    /// clip does not loop and its notes play once, which is what every
    /// document written before this existed says.
    ///
    /// A MIDI clip is pinned at the length it was created and REPEATS beyond
    /// it, so dragging its right edge lays down another pass instead of
    /// revealing empty time. The notes are held once and expanded where they
    /// are scheduled — a loop that copied its notes would drift out of step
    /// with the take it came from the moment one of them was edited.
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
    pub inserts: Vec<String>,
    pub sends: Vec<f64>,
    pub muted: u32,
    pub soloed: u32,
    pub gains: Vec<f64>,
    pub pans: Vec<f64>,
    pub inputs: Vec<String>,
    pub drums_gain: f64,
    pub drums_pan: f64,
    pub metronome_gain: f64,
    pub master_gain: f64,
    pub fx_return: f64,
    pub tempo_bpm: f64,
    pub sig_beats: u8,
    pub sig_unit: u8,
    pub metronome_on: bool,
}

impl DeskSnap {
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
        m.insert(
            "inserts".into(),
            Value::Array(self.inserts.iter().map(|i| Value::String(i.clone())).collect()),
        );
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
        Some(Self {
            kinds: m
                .get("kinds")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n as u8)).collect())
                .unwrap_or_default(),
            icons: m
                .get("icons")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(|x| x.as_u64().map(|n| n.min(3) as u8)).collect())
                .unwrap_or_default(),
            names: strings("names"),
            colors: m
                .get("colors")
                .and_then(Value::as_array)
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default(),
            inserts: strings("inserts"),
            sends: floats("sends"),
            muted: m.get("muted").and_then(Value::as_u64).unwrap_or(0) as u32,
            soloed: m.get("soloed").and_then(Value::as_u64).unwrap_or(0) as u32,
            gains: floats("gains"),
            pans: floats("pans"),
            inputs: strings("inputs"),
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
        })
    }
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
            automation: crate::automation::Automation::default(),
        }
    }
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
    pub fn load_from(path: &std::path::Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::from_json(&text),
            Err(_) => Self::default(),
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
        let automation = Self::read_automation(&mut map);
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
                        });
                    }
                }
                let clip = ArrClip {
                    kind,
                    notes,
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
                    // The list below is the eleven names read above and must
                    // stay in step with them. A field added to `ArrClip`
                    // without a name added here comes back DUPLICATED: once
                    // as itself and once inside `extra`, and the extras merge
                    // in `to_json` would then be writing a stale copy of a
                    // live field. `a_clip_knows_which_of_its_keys_are_its_own`
                    // is what catches that.
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
                Some(Curve::sanitised(lane, param, points))
            })
            .collect();
        Automation::from_curves(curves)
    }

    pub fn to_json(&self) -> String {
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
        if let Some(desk) = &self.desk {
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
        // Unknown keys LAST, like the settings file: a newer build's keys win
        // over nothing, and they survive.
        for (k, v) in &self.extra {
            map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into())
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

    /// **A clip's loop length survives the document**, and its absence means
    /// "does not loop" — which is what every file written before this said.
    #[test]
    fn a_loop_length_round_trips_and_absence_means_no_loop() {
        let mut arr = Arrangement::default();
        arr.add_clip(ArrClip {
            kind: ClipKind::Midi,
            lane: 2,
            start_s: 1.0,
            len_s: 8.0,
            loop_s: 2.0,
            notes: vec![ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100 }],
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
        let older = r#"{"version":3,"clips":[{"kind":"midi","lane":0,"start_s":0.0,"len_s":4.0,"notes":[]}]}"#;
        let old = Arrangement::from_json(older);
        assert_eq!(old.clips.len(), 1);
        assert_eq!(old.clips[0].loop_s, 0.0, "an old clip came back looping");
    }
    use super::*;

    fn a_clip(lane: usize, path: &str, start_s: f64) -> ArrClip {
        ArrClip {
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
        }
    }

    /// A document survives its own round trip, clips, edges and all.
    #[test]
    fn an_arrangement_round_trips() {
        let mut a = Arrangement::default();
        a.add_clip(a_clip(0, "media/backing.mp3", 0.0));
        let solo = ArrClip {
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
            kind: ClipKind::Midi,
            path: String::new(),
            len_s: 8.0,
            name: "MIDI".into(),
            notes: vec![
                ArrNote { at_s: 0.0, dur_s: 0.5, key: 60, vel: 100 },
                ArrNote { at_s: 1.25, dur_s: 2.0, key: 67, vel: 88 },
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

    /// The span is the furthest clip end, trims and the zero sentinel both
    /// honoured — it is what the scrubber draws against.
    #[test]
    fn the_span_is_the_furthest_clip_end() {
        let mut a = Arrangement::default();
        a.add_clip(a_clip(0, "/x/a.wav", 0.0)); // whole 100 s file
        a.add_clip(ArrClip {
            len_s: 10.0,
            ..a_clip(1, "/x/b.wav", 95.0) // ends at 105
        });
        a.add_clip(ArrClip {
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
            notes: vec![ArrNote { at_s: 0.5, dur_s: 1.0, key: 64, vel: 99 }],
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
}
