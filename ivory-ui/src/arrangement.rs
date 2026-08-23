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
pub const ARRANGEMENT_VERSION: u64 = 4;

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
    /// Seconds of ramp up from silence at the clip's head. Zero for none.
    pub fade_in_s: f64,
    /// Seconds of ramp down to silence at the clip's tail. Zero for none.
    pub fade_out_s: f64,
    /// What the clip is called on the lane. The file's stem until renamed.
    pub name: String,
    /// Stable identity for edits — selection, undo one day. Assigned from
    /// [`Arrangement::next_id`], never reused within a document.
    pub id: u64,
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
        // Written only when set: older documents stay byte-identical.
        if self.icons.iter().any(|i| *i != 0) {
            m.insert(
                "icons".into(),
                Value::Array(
                    self.icons.iter().map(|k| Value::Number((*k as u64).into())).collect(),
                ),
            );
        }
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
#[derive(Debug, Clone, Default, PartialEq)]
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
}

impl Arrangement {
    /// Where the document lives. Overridable for tests in EVERY build, for
    /// the same reason `Settings::path` is: a binary-crate test links an
    /// ordinary `ivory-ui`, and one that ran a frame against the real home
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
        let _version = map
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
        let mut clips = Vec::new();
        if let Some(Value::Array(list)) = map.shift_remove("clips") {
            for item in list {
                let Value::Object(c) = item else { continue };
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
                clips.push(ArrClip {
                    kind,
                    notes,
                    lane,
                    path: path.to_owned(),
                    start_s: secs(&c, "start_s"),
                    offset_s: secs(&c, "offset_s"),
                    len_s: secs(&c, "len_s"),
                    fade_in_s: secs(&c, "fade_in_s"),
                    fade_out_s: secs(&c, "fade_out_s"),
                    name,
                    id: c.get("id").and_then(Value::as_u64).unwrap_or(0),
                });
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
        }
    }

    pub fn to_json(&self) -> String {
        let mut map = Map::new();
        map.insert(
            "arrangement_version".into(),
            Value::Number(ARRANGEMENT_VERSION.into()),
        );
        map.insert(
            "next_clip_id".into(),
            Value::Number(self.next_id.into()),
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
                        for (key, v) in [
                            ("start_s", c.start_s),
                            ("offset_s", c.offset_s),
                            ("len_s", c.len_s),
                            ("fade_in_s", c.fade_in_s),
                            ("fade_out_s", c.fade_out_s),
                        ] {
                            if let Some(n) = serde_json::Number::from_f64(v) {
                                m.insert(key.into(), Value::Number(n));
                            }
                        }
                        Value::Object(m)
                    })
                    .collect(),
            ),
        );
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
            fade_in_s: 0.0,
            fade_out_s: 0.0,
            name: std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            id: 0,
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
        let text = r#"{"arrangement_version": 9, "clips": [],
                       "tempo_map": [{"at": 0, "bpm": 121}]}"#;
        let a = Arrangement::from_json(text);
        let out = a.to_json();
        assert!(
            out.contains("tempo_map") && out.contains("121"),
            "a newer build's tempo map was stripped: {out}"
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
