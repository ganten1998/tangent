//! The drumpad's document: which sample each pad holds and how its knobs
//! stand, saved beside the arrangement as `drumpad.json`.
//!
//! A SIDECAR, not rows in the arrangement document, on purpose: the kit is
//! an instrument's state, not timeline material, and a sidecar leaves the
//! arrangement's schema exactly as the other work on it expects. It rides
//! the project directory like everything else — copy the folder and the kit
//! comes along, because the samples are in `media/` and the paths in here
//! are relative to it.
//!
//! Write-then-rename on save and lenient on load, the settings file's own
//! manners: a missing or half-written sidecar is an empty kit, never an
//! error at launch.

use serde_json::{Map, Value};

use crate::recorder::{DrumKitParams, DrumPadParams, DRUM_PADS};

/// The sidecar's name, beside the project document.
pub const FILE_NAME: &str = "drumpad.json";

/// The sidecar's schema version.
///
/// **It was written and never read**, which is the shape of a version stamp
/// that does nothing: `to_json` stamped a 1 on every file and `from_json`
/// did not mention the key, so a newer build's kit — new pad parameters, a
/// new field on the kit — was silently flattened to this build's nine
/// per-pad keys by any save from here. The arrangement made this bargain
/// long ago; the kit rides along with it now.
pub const DRUMPAD_VERSION: u64 = 1;

/// Every key a pad object holds that THIS build understands, for the same
/// reason [`crate::arrangement`] keeps one: it is subtracted from a pad
/// object to find the keys that are somebody else's.
const PAD_KEYS: [&str; 9] = [
    "path",
    "name",
    "gain",
    "pan",
    "tune",
    "choke",
    "obey",
    "trim_start",
    "trim_end",
];

/// The same, one level up. `pads` and `master` are the kit's own; `version`
/// is written from [`DRUMPAD_VERSION`] and must not be preserved as a
/// stranger's key or it would pin the file at whatever it first said.
const KIT_KEYS: [&str; 3] = ["version", "master", "pads"];

/// One pad's slot in the document.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct PadSlot {
    /// `media/<file>` against the project directory — [`resolve`] pays the
    /// rule — or empty for an empty pad.
    pub path: String,
    /// What the pad CALLS its clip — the menu's editable field. Empty
    /// falls back to the file's stem.
    pub name: String,
    pub params: DrumPadParams,
    /// Keys this build does not know, kept verbatim. See [`PAD_KEYS`].
    pub extra: Map<String, Value>,
}

/// The kit, in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumpadDoc {
    pub pads: [PadSlot; DRUM_PADS],
    /// The kit's own fader, linear.
    pub master: f32,
    /// The version this kit is IN, never below [`DRUMPAD_VERSION`] — the
    /// arrangement document's rule, and for its reason.
    pub version: u64,
    /// Kit-level keys this build does not know. See [`KIT_KEYS`].
    pub extra: Map<String, Value>,
}

impl Default for DrumpadDoc {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(|_| PadSlot::default()),
            master: 1.0,
            version: DRUMPAD_VERSION,
            extra: Map::new(),
        }
    }
}

/// A pad path against the document's own directory — the arrangement's
/// exact rule, repeated here because this module may not depend on where
/// that one keeps it.
pub fn resolve(doc_path: &std::path::Path, pad_path: &str) -> std::path::PathBuf {
    let p = std::path::Path::new(pad_path);
    if p.is_absolute() {
        return p.to_path_buf();
    }
    doc_path
        .parent()
        .map_or_else(|| p.to_path_buf(), |d| d.join(p))
}

impl DrumpadDoc {
    /// Where the sidecar stands, beside the project document.
    pub fn sidecar(doc: &std::path::Path) -> std::path::PathBuf {
        doc.parent()
            .map_or_else(|| std::path::PathBuf::from(FILE_NAME), |d| d.join(FILE_NAME))
    }

    pub fn is_empty(&self) -> bool {
        self.pads.iter().all(|p| p.path.is_empty())
    }

    /// The knobs alone, in the shape the engine takes them.
    pub fn params(&self) -> DrumKitParams {
        DrumKitParams {
            pads: std::array::from_fn(|i| self.pads[i].params),
            master: self.master,
        }
    }

    /// The kit saved beside `doc`, or the empty kit — a missing sidecar is
    /// a project from before the drumpad existed, which is most of them.
    pub fn load_beside(doc: &std::path::Path) -> Self {
        std::fs::read_to_string(Self::sidecar(doc))
            .map(|text| Self::from_json(&text))
            .unwrap_or_default()
    }

    /// **Every lane's kit from the one sidecar**, `lanes` of them.
    ///
    /// A drum instrument loads a full instance per channel, so a project
    /// carries a kit per lane. The file keeps its ORIGINAL shape at the top
    /// level — `pads` and `master`, the kit a pre-lanes build wrote and the
    /// only thing such a build can read — and adds `lane_kits`, an object
    /// keyed by channel number, for the rest.
    ///
    /// The legacy kit lands on the lane named by `lane_kits`' own
    /// `primary`; failing that, on `legacy_lane` — **the channel whose first
    /// bay actually holds the drumpad**, which the caller reads off the desk.
    /// That is what makes an old project open sounding as it did: its single
    /// kit belongs to whichever channel was carrying the drumpad, and the app
    /// writes that number down the first time it saves.
    ///
    /// **Lane 0 was a guess, and for most real projects it was wrong.** This
    /// took the caller's hint only after a project with its drumpad on
    /// channel 3 opened silent: the kit went to channel 0, the piano lane,
    /// where nothing strikes a pad, and the drum lane came up with a hundred
    /// and twenty-eight empty slots under a track full of notes. Nothing was
    /// lost on disk — the sidecar still held every sample — but the session
    /// played no drums and reads exactly like a kit that has been destroyed.
    /// `Settings::move_legacy_pad_racks` asks the identical question of the
    /// identical array for the identical reason; this is the other half of
    /// that fix, arrived at four months later.
    ///
    /// `None` keeps the old behaviour — lane 0 — for a caller with no desk
    /// to consult, which is only the tests and a project with no drumpad in
    /// it anywhere.
    /// Every lane's kit, and **which lane was showing when it was saved**.
    ///
    /// The second half is not decoration. `primary_lane` is written on every
    /// save and was read only to place the legacy kit — so the app reopened a
    /// project on the LOWEST non-empty lane instead of the one that had been
    /// in front of you. On a session with kits on lanes 0, 3 and 5 and the
    /// panel left on 3, it came back showing 0: different samples on every
    /// pad, in the panel you had just been working in, which reads as the
    /// kit having been destroyed. It had not been; it was on another lane the
    /// whole time. The owner: "the update clobbered my old drum layouts."
    pub fn load_all_beside(
        doc: &std::path::Path,
        lanes: usize,
        legacy_lane: Option<usize>,
    ) -> (Vec<Self>, Option<usize>) {
        let mut out: Vec<Self> = (0..lanes).map(|_| Self::default()).collect();
        let Ok(text) = std::fs::read_to_string(Self::sidecar(doc)) else {
            return (out, None);
        };
        // The legacy kit, always — it is the whole file for a project
        // written before this existed.
        let legacy = Self::from_json(&text);
        let map = serde_json::from_str::<Value>(&text).ok();
        let obj = map.as_ref().and_then(Value::as_object);
        // The file's own word first; the desk's answer second; lane 0 only
        // when neither has an opinion.
        let primary = obj
            .and_then(|m| m.get("primary_lane"))
            .and_then(Value::as_u64)
            .map_or_else(|| legacy_lane.unwrap_or(0), |n| n as usize);
        if primary < out.len() {
            out[primary] = legacy;
        }
        // Then anything the newer key names, which overwrites the legacy
        // placement when it speaks about the same lane.
        if let Some(Value::Object(kits)) = obj.and_then(|m| m.get("lane_kits")) {
            for (key, v) in kits {
                let Ok(lane) = key.parse::<usize>() else {
                    continue;
                };
                if lane >= out.len() {
                    continue;
                }
                if let Ok(text) = serde_json::to_string(v) {
                    out[lane] = Self::from_json(&text);
                }
            }
        }
        // Only if the file actually said so. A project written before
        // `primary_lane` existed has no opinion, and inventing lane 0 for it
        // would be the same guess the caller already makes.
        let stated = obj
            .and_then(|m| m.get("primary_lane"))
            .and_then(Value::as_u64)
            .map(|n| n as usize)
            .filter(|n| *n < out.len());
        (out, stated)
    }

    /// **Every lane's kit into the one sidecar.** See `load_all_beside` for
    /// why the primary kit is also written at the top level.
    pub fn save_all_beside(kits: &[Self], primary: usize, doc: &std::path::Path) {
        let mut root = match kits.get(primary) {
            Some(k) => serde_json::from_str::<Value>(&k.to_json())
                .ok()
                .and_then(|v| v.as_object().cloned())
                .unwrap_or_default(),
            None => serde_json::Map::new(),
        };
        root.insert("primary_lane".into(), Value::from(primary as u64));
        let mut lane_kits = serde_json::Map::new();
        for (lane, kit) in kits.iter().enumerate() {
            // An empty kit is the absence of one; writing it would grow the
            // file by a lane nobody has for no reading anybody does.
            if kit.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&kit.to_json()) {
                lane_kits.insert(lane.to_string(), v);
            }
        }
        root.insert("lane_kits".into(), Value::Object(lane_kits));

        let path = Self::sidecar(doc);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        let text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
        if std::fs::write(&tmp, text).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    pub fn save_beside(&self, doc: &std::path::Path) {
        let path = Self::sidecar(doc);
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, self.to_json()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    fn from_json(text: &str) -> Self {
        let mut out = Self::default();
        let Ok(Value::Object(mut map)) = serde_json::from_str::<Value>(text) else {
            return out;
        };
        // Up, never down: a kit written by a newer build keeps the number
        // that build will look for, and an unstamped one becomes this
        // build's, because reading it here is what made it one.
        out.version = map
            .get("version")
            .and_then(Value::as_u64)
            .unwrap_or(0)
            .max(DRUMPAD_VERSION);
        if let Some(m) = map.get("master").and_then(Value::as_f64) {
            out.master = (m as f32).clamp(0.0, 2.0);
        }
        let pads = map.shift_remove("pads");
        for known in KIT_KEYS {
            map.shift_remove(known);
        }
        out.extra = map;
        let Some(Value::Array(pads)) = pads else {
            return out;
        };
        // **The sixteen-pad era loads at its old home.** Before the pads
        // covered the whole keyboard, the sidecar held exactly sixteen
        // slots and they WERE keys 36..51 — so a sixteen-entry file lands
        // at offset 36, and every old kit keeps striking the pads it
        // always struck. Full-width files map straight through.
        let offset = if pads.len() == 16 { 36 } else { 0 };
        for (slot, v) in out.pads.iter_mut().skip(offset).zip(pads) {
            let Value::Object(mut p) = v else { continue };
            if let Some(path) = p.get("path").and_then(Value::as_str) {
                slot.path = path.to_owned();
            }
            if let Some(name) = p.get("name").and_then(Value::as_str) {
                slot.name = name.to_owned();
            }
            let f = |key: &str, lo: f32, hi: f32, or: f32| {
                p.get(key)
                    .and_then(Value::as_f64)
                    .map_or(or, |v| (v as f32).clamp(lo, hi))
            };
            slot.params.gain = f("gain", 0.0, 2.0, 1.0);
            slot.params.pan = f("pan", -1.0, 1.0, 0.0);
            slot.params.tune = f("tune", -12.0, 12.0, 0.0);
            slot.params.choke = p
                .get("choke")
                .and_then(Value::as_u64)
                .map_or(0, |v| v.min(255) as u8);
            slot.params.obey = p.get("obey").and_then(Value::as_bool).unwrap_or(false);
            slot.params.trim_start = f("trim_start", 0.0, 1.0, 0.0);
            slot.params.trim_end = f("trim_end", 0.0, 1.0, 1.0);
            if slot.params.trim_end <= slot.params.trim_start + 0.005 {
                slot.params.trim_start = 0.0;
                slot.params.trim_end = 1.0;
            }
            // Whatever this build did not read belongs to whoever wrote it.
            for known in PAD_KEYS {
                p.shift_remove(known);
            }
            slot.extra = p;
        }
        out
    }

    fn to_json(&self) -> String {
        let mut map = Map::new();
        // The kit's OWN version, not the constant — see [`DRUMPAD_VERSION`].
        map.insert("version".into(), Value::from(self.version));
        map.insert("master".into(), Value::from(f64::from(self.master)));
        let pads: Vec<Value> = self
            .pads
            .iter()
            .map(|slot| {
                let mut p = Map::new();
                p.insert("path".into(), Value::from(slot.path.clone()));
                p.insert("name".into(), Value::from(slot.name.clone()));
                p.insert("gain".into(), Value::from(f64::from(slot.params.gain)));
                p.insert("pan".into(), Value::from(f64::from(slot.params.pan)));
                p.insert("tune".into(), Value::from(f64::from(slot.params.tune)));
                p.insert("choke".into(), Value::from(slot.params.choke));
                p.insert("obey".into(), Value::from(slot.params.obey));
                p.insert(
                    "trim_start".into(),
                    Value::from(f64::from(slot.params.trim_start)),
                );
                p.insert(
                    "trim_end".into(),
                    Value::from(f64::from(slot.params.trim_end)),
                );
                // The pad's strangers last, and a known key wins — the
                // arrangement document's rule.
                for (k, v) in &slot.extra {
                    p.entry(k.clone()).or_insert_with(|| v.clone());
                }
                Value::Object(p)
            })
            .collect();
        map.insert("pads".into(), Value::Array(pads));
        for (k, v) in &self.extra {
            map.entry(k.clone()).or_insert_with(|| v.clone());
        }
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    /// **Every lane's kit round-trips, and an OLD project still opens right.**
    ///
    /// A drum instrument loads a full instance per channel, so the sidecar
    /// carries a kit per lane. It must do that WITHOUT breaking a project
    /// written before lanes existed: those files have one kit at the top
    /// level, and it belongs to whichever channel was carrying the drumpad.
    #[test]
    fn every_lanes_kit_round_trips_and_a_pre_lanes_project_still_opens() {
        let dir = std::env::temp_dir().join(format!(
            "tangent-drumkits-{}",
            std::process::id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let doc = dir.join("project.tangent");

        let kit = |name: &str| {
            let mut k = DrumpadDoc::default();
            k.pads[36].path = format!("media/{name}.wav");
            k.pads[36].params.gain = 0.5;
            k
        };
        let mut kits: Vec<DrumpadDoc> = (0..16).map(|_| DrumpadDoc::default()).collect();
        kits[3] = kit("kick");
        kits[7] = kit("snare");

        DrumpadDoc::save_all_beside(&kits, 3, &doc);
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 16, None);
        assert_eq!(back.len(), 16);
        // **The lane that was showing comes back with the kits.** Reading the
        // kits and forgetting which one was in front of you is how a project
        // reopens on somebody else's pads.
        assert_eq!(primary, Some(3), "the saved lane was not restored");
        assert_eq!(back[3].pads[36].path, "media/kick.wav", "lane 3's kit");
        assert_eq!(back[7].pads[36].path, "media/snare.wav", "lane 7's kit");
        assert!(back[0].is_empty(), "a lane with no kit came back with one");
        assert!((back[7].pads[36].params.gain - 0.5).abs() < 1e-6, "the knobs");

        // **An OLD sidecar — one kit, no lane keys — lands where the DESK
        // says the drumpad is**, and on lane 0 only when nobody says
        // otherwise. Lane 0 alone was the original rule and it was a guess:
        // the project that found this had its drumpad on channel 3, so its
        // only kit went to the piano lane and the drum track played a
        // hundred and twenty-eight empty pads under a full arrangement.
        let legacy = kit("legacy");
        std::fs::write(DrumpadDoc::sidecar(&doc), legacy.to_json()).expect("write legacy");
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 16, None);
        // A file with no opinion says so, rather than claiming lane 0 —
        // the caller's fallback is the same answer and it knows it guessed.
        assert_eq!(primary, None, "a pre-lanes file invented a primary lane");
        assert_eq!(
            back[0].pads[36].path, "media/legacy.wav",
            "a pre-lanes kit did not survive the upgrade"
        );
        // The hint moves it, and moves it ALONE: no other lane gains a kit.
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 16, Some(3));
        assert_eq!(primary, None, "a hint is not the file speaking");
        assert_eq!(
            back[3].pads[36].path, "media/legacy.wav",
            "the pre-lanes kit ignored the lane that actually holds the drumpad"
        );
        assert!(
            back[0].is_empty(),
            "the kit was left on lane 0 as well as moved"
        );
        // A hint past the end places nothing rather than panicking — the
        // same guard an out-of-range `primary_lane` has always met. The file
        // on disk is untouched, so the next load with a sane hint still
        // finds the kit. The real caller cannot produce this: it searches
        // the same channel count it asks for.
        let (back, _) = DrumpadDoc::load_all_beside(&doc, 16, Some(99));
        assert!(
            back.iter().all(DrumpadDoc::is_empty),
            "an out-of-range hint put a kit somewhere"
        );

        // And the top level still says what an old build would read, so a
        // project saved here does not come up empty over there.
        DrumpadDoc::save_all_beside(&kits, 3, &doc);
        let old_reader = DrumpadDoc::load_beside(&doc);
        assert_eq!(
            old_reader.pads[36].path, "media/kick.wav",
            "an older build would open this project with no kit at all"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    use super::*;

    /// A newer build's kit is not flattened by a save from this one.
    ///
    /// **This was a live data-loss bug, not a hypothetical.** The sidecar
    /// stamped `"version": 1` on every write and never once read the key
    /// back, and neither the kit nor a pad had anywhere to keep a name this
    /// build did not recognise — so opening a project in an older Tangent
    /// and touching the drumpad silently rewrote every pad down to the nine
    /// parameters that build happened to know.
    #[test]
    fn a_newer_builds_kit_survives_a_save_from_this_one() {
        let kit = DrumpadDoc::from_json(
            r#"{"version": 7, "master": 0.8, "swing": 0.24,
                "pads": [{"path": "media/kick.wav", "gain": 0.9,
                          "reverse": true, "filter": {"cutoff": 800}}]}"#,
        );
        assert_eq!(kit.version, 7, "a newer kit's version was not kept");
        assert_eq!(kit.pads[0].path, "media/kick.wav");
        let out = kit.to_json();
        assert!(out.contains("swing"), "a kit-level stranger was dropped: {out}");
        assert!(out.contains("reverse"), "a pad-level stranger was dropped: {out}");
        assert!(out.contains("cutoff"), "a nested pad stranger was dropped: {out}");
        assert!(
            out.contains(r#""version": 7"#),
            "the kit was downgraded on write: {out}"
        );
        assert_eq!(DrumpadDoc::from_json(&out), kit, "the kit changed on a second trip");
    }

    /// An unstamped kit becomes this build's, and the known keys are never
    /// mistaken for strangers.
    #[test]
    fn the_kits_own_keys_are_not_kept_twice() {
        let mut kit = DrumpadDoc::default();
        kit.pads[3].path = "media/snare.wav".into();
        kit.pads[3].name = "the snare".into();
        kit.pads[3].params.gain = 0.75;
        let back = DrumpadDoc::from_json(&kit.to_json());
        assert_eq!(back.version, DRUMPAD_VERSION);
        assert!(back.extra.is_empty(), "the kit claimed its own keys: {:?}", back.extra);
        assert!(
            back.pads[3].extra.is_empty(),
            "a pad claimed its own keys: {:?}",
            back.pads[3].extra
        );
        assert_eq!(back, kit);
        // And every name the writer emits is in the two lists, or a real
        // stranger by that name would be silently swallowed.
        let Ok(Value::Object(m)) = serde_json::from_str::<Value>(&kit.to_json()) else {
            panic!("the kit did not parse as an object");
        };
        for key in m.keys() {
            assert!(KIT_KEYS.contains(&key.as_str()), "`{key}` is missing from KIT_KEYS");
        }
        let Some(Value::Array(pads)) = m.get("pads") else { panic!("no pads") };
        let Some(Value::Object(p)) = pads.first() else { panic!("no pad") };
        for key in p.keys() {
            assert!(PAD_KEYS.contains(&key.as_str()), "`{key}` is missing from PAD_KEYS");
        }
        for known in PAD_KEYS {
            assert!(p.contains_key(known), "PAD_KEYS names `{known}`, never written");
        }
    }

    /// Save and load are inverses, through a real file.
    #[test]
    fn the_kit_survives_the_round_trip() {
        let dir = std::env::temp_dir().join(format!("drumpad-doc-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let doc = dir.join("project.tangent");

        let mut kit = DrumpadDoc::default();
        kit.master = 0.8;
        kit.pads[0].path = "media/Kick.wav".into();
        kit.pads[0].params.gain = 1.5;
        kit.pads[0].params.pan = -0.25;
        kit.pads[0].params.tune = -2.0;
        kit.pads[0].params.choke = 3;
        kit.pads[15].path = "media/Crash.wav".into();
        kit.save_beside(&doc);

        let back = DrumpadDoc::load_beside(&doc);
        assert_eq!(back, kit);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A missing sidecar, garbage, and half a document all answer a kit —
    /// the settings file's manners.
    #[test]
    fn bad_documents_answer_the_empty_kit() {
        let nowhere = std::path::Path::new("/nonexistent/project.tangent");
        assert!(DrumpadDoc::load_beside(nowhere).is_empty());
        assert!(DrumpadDoc::from_json("not json").is_empty());
        let half = DrumpadDoc::from_json(r#"{"version":1,"pads":[{"path":"media/a.wav","gain":9.0}]}"#);
        assert_eq!(half.pads[0].path, "media/a.wav");
        assert!((half.pads[0].params.gain - 2.0).abs() < 1.0e-6, "gain unclamped");
        assert!(half.pads[1].path.is_empty());
    }

    /// Relative paths resolve beside the document; absolute ones stand.
    #[test]
    fn paths_resolve_like_the_arrangement() {
        let doc = std::path::Path::new("/tmp/proj/project.tangent");
        assert_eq!(
            resolve(doc, "media/a.wav"),
            std::path::PathBuf::from("/tmp/proj/media/a.wav")
        );
        assert_eq!(resolve(doc, "/abs/b.wav"), std::path::PathBuf::from("/abs/b.wav"));
    }
}

impl DrumpadDoc {
    /// **Every lane's kit, small enough to keep a hundred of.**
    ///
    /// The full form writes all 128 pads whether or not they hold anything —
    /// 24 KB a kit, which is right for a file written once per edit and wrong
    /// for the undo timeline, where a hundred moments of three kits would be
    /// several megabytes of mostly empty slots.
    ///
    /// So: only the pads that hold something, and only the lanes that hold a
    /// pad. Everything else is the default and rebuilds itself.
    ///
    /// **The one thing this drops** is `extra` on an EMPTY pad — keys a newer
    /// build wrote on a slot with no sample in it. The sidecar still
    /// preserves them; only the undo snapshot does not, so the cost of a
    /// round trip through undo is a stranger's key on a pad that was empty
    /// anyway. That is worth three orders of magnitude.
    pub fn compact_all(kits: &[Self]) -> String {
        let mut root = Map::new();
        for (lane, kit) in kits.iter().enumerate() {
            if kit.is_empty() && (kit.master - 1.0).abs() < f32::EPSILON {
                continue;
            }
            let mut k = Map::new();
            k.insert("master".into(), Value::from(f64::from(kit.master)));
            let mut pads = Map::new();
            for (key, pad) in kit.pads.iter().enumerate() {
                if pad.path.is_empty() {
                    continue;
                }
                let mut m = pad.extra.clone();
                m.insert("path".into(), Value::from(pad.path.clone()));
                m.insert("name".into(), Value::from(pad.name.clone()));
                m.insert("gain".into(), Value::from(f64::from(pad.params.gain)));
                m.insert("pan".into(), Value::from(f64::from(pad.params.pan)));
                m.insert("tune".into(), Value::from(f64::from(pad.params.tune)));
                m.insert("choke".into(), Value::from(u64::from(pad.params.choke)));
                m.insert("obey".into(), Value::from(pad.params.obey));
                m.insert(
                    "trim_start".into(),
                    Value::from(f64::from(pad.params.trim_start)),
                );
                m.insert("trim_end".into(), Value::from(f64::from(pad.params.trim_end)));
                pads.insert(key.to_string(), Value::Object(m));
            }
            k.insert("pads".into(), Value::Object(pads));
            root.insert(lane.to_string(), Value::Object(k));
        }
        serde_json::to_string(&Value::Object(root)).unwrap_or_default()
    }

    /// The inverse. Unreadable text is `lanes` empty kits, never a panic.
    pub fn from_compact_all(text: &str, lanes: usize) -> Vec<Self> {
        let mut out: Vec<Self> = (0..lanes).map(|_| Self::default()).collect();
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return out;
        };
        for (key, v) in &root {
            let Ok(lane) = key.parse::<usize>() else { continue };
            if lane >= out.len() {
                continue;
            }
            let Some(k) = v.as_object() else { continue };
            if let Some(m) = k.get("master").and_then(Value::as_f64) {
                out[lane].master = m as f32;
            }
            let Some(Value::Object(pads)) = k.get("pads") else { continue };
            for (pk, pv) in pads {
                let Ok(idx) = pk.parse::<usize>() else { continue };
                if idx >= DRUM_PADS {
                    continue;
                }
                let Some(o) = pv.as_object() else { continue };
                let num = |n: &str, d: f32| {
                    o.get(n).and_then(Value::as_f64).map_or(d, |x| x as f32)
                };
                let mut params = DrumPadParams {
                    gain: num("gain", 1.0),
                    pan: num("pan", 0.0),
                    tune: num("tune", 0.0),
                    trim_start: num("trim_start", 0.0),
                    trim_end: num("trim_end", 1.0),
                    ..DrumPadParams::default()
                };
                if let Some(c) = o.get("choke").and_then(Value::as_u64) {
                    params.choke = c as u8;
                }
                if let Some(b) = o.get("obey").and_then(Value::as_bool) {
                    params.obey = b;
                }
                let mut extra = o.clone();
                for k in PAD_KEYS {
                    extra.shift_remove(k);
                }
                out[lane].pads[idx] = PadSlot {
                    path: o
                        .get("path")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    name: o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                    params,
                    extra,
                };
            }
        }
        out
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Kit presets
// ───────────────────────────────────────────────────────────────────────────

/// The extension a saved kit wears.
pub const PRESET_EXT: &str = "tgkit";

/// The first bytes of a kit file that carries its own samples. Version 2 —
/// version 1 was the same manifest as bare JSON with no audio at all, and
/// [`KitPreset::from_bytes`] still reads it.
const MAGIC: &[u8] = b"TGKIT2\n";

/// **A kit as a THING you can keep**, independent of the project it was
/// built in.
///
/// The sidecar beside a project is not a preset and cannot be made into one:
/// its paths are `media/<file>` against that project's own folder, so the
/// same file read from another project points at samples that are not there.
/// The owner spent a session building a layout, and the only copy of it lived
/// inside one project directory: "add a system to save a kit in the drum vst,
/// saving to an actual file which holds which samples go on which pads for
/// that preset."
///
/// So a preset records **absolute** source paths, and loading one copies each
/// sample into the destination project the same way dragging the file in
/// would — `TangentApp::drumpad_load`, one pad at a time. A project stays
/// self-contained; the preset is the recipe, not the ingredients.
///
/// Only the pads that hold something are written. A hundred and twenty-eight
/// empty slots is most of a 24 KB file saying nothing.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KitPreset {
    /// What the kit is called — the file's stem, kept in the file too so a
    /// renamed file still knows what it was.
    pub name: String,
    /// Every pad that holds something, in key order.
    pub pads: Vec<KitPad>,
    /// The kit's own fader.
    pub master: f32,
    /// **The samples themselves.** One entry per distinct source file, in
    /// the order the pads first reach for them; a pad points at one by index.
    ///
    /// Empty for a preset READ from the old path-only form, and for one built
    /// in memory by a test. Everything downstream therefore treats an absent
    /// blob as "go and find the file", which is exactly what the old form
    /// meant.
    pub media: Vec<KitBlob>,
}

/// One pad of a saved kit.
#[derive(Debug, Clone, PartialEq)]
pub struct KitPad {
    /// Which key it sits on.
    pub key: usize,
    /// Where the sample came from, absolute. Kept even when the audio is
    /// embedded: it is the provenance, and the fallback if the blob is gone.
    pub file: std::path::PathBuf,
    /// The pad's own label.
    pub name: String,
    /// Gain, pan, tune, choke, obey and the trim — the layout, which is half
    /// of what makes a kit somebody's rather than a folder of samples.
    pub params: DrumPadParams,
    /// Which entry of [`KitPreset::media`] holds this pad's audio, if the
    /// preset carries it.
    pub blob: Option<usize>,
}

/// One embedded sample: what it was called, and its bytes exactly as they sat
/// on the disk it came from. No transcoding — a kit is a copy, not a render.
#[derive(Debug, Clone, PartialEq)]
pub struct KitBlob {
    /// The original file name, used for the copy made in the destination
    /// project so a shared kit lands with the names its author gave it.
    pub name: String,
    pub bytes: Vec<u8>,
}

impl KitPreset {
    /// Take a snapshot of `kit`, resolving every pad against `doc`.
    ///
    /// A pad whose file has since gone missing is written anyway: the preset
    /// is a record of the layout, and a sample that is missing today may be
    /// on the disk the preset is carried to. Loading reports what it could
    /// not find rather than refusing the whole kit.
    pub fn from_kit(name: &str, kit: &DrumpadDoc, doc: &std::path::Path) -> Self {
        let mut media: Vec<KitBlob> = Vec::new();
        let mut seen: Vec<std::path::PathBuf> = Vec::new();
        let mut pads = Vec::new();
        for (key, p) in kit.pads.iter().enumerate() {
            if p.path.is_empty() {
                continue;
            }
            let file = resolve(doc, &p.path);
            // **One blob per distinct file.** A kit with the same hat on two
            // pads at different tunings is one sample, and a preset that
            // wrote it twice would be twice the size for nothing.
            let blob = seen.iter().position(|s| *s == file).or_else(|| {
                let bytes = std::fs::read(&file).ok()?;
                let label = file
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("pad-{key}.wav"));
                seen.push(file.clone());
                media.push(KitBlob { name: label, bytes });
                Some(media.len() - 1)
            });
            pads.push(KitPad {
                key,
                file,
                name: p.name.clone(),
                params: p.params,
                blob,
            });
        }
        Self {
            name: name.to_owned(),
            master: kit.master,
            pads,
            media,
        }
    }

    pub fn to_json(&self) -> String {
        let mut root = Map::new();
        root.insert("version".into(), Value::from(DRUMPAD_VERSION));
        root.insert("kind".into(), Value::from("tangent-kit"));
        root.insert("name".into(), Value::from(self.name.clone()));
        root.insert("master".into(), Value::from(f64::from(self.master)));
        let pads: Vec<Value> = self
            .pads
            .iter()
            .map(|pad| {
                let params = &pad.params;
                let mut m = Map::new();
                m.insert("key".into(), Value::from(pad.key as u64));
                m.insert(
                    "file".into(),
                    Value::from(pad.file.to_string_lossy().into_owned()),
                );
                m.insert("name".into(), Value::from(pad.name.clone()));
                m.insert("gain".into(), Value::from(f64::from(params.gain)));
                m.insert("pan".into(), Value::from(f64::from(params.pan)));
                m.insert("tune".into(), Value::from(f64::from(params.tune)));
                m.insert("choke".into(), Value::from(u64::from(params.choke)));
                m.insert("obey".into(), Value::from(params.obey));
                m.insert("trim_start".into(), Value::from(f64::from(params.trim_start)));
                m.insert("trim_end".into(), Value::from(f64::from(params.trim_end)));
                if let Some(b) = pad.blob {
                    m.insert("blob".into(), Value::from(b as u64));
                }
                Value::Object(m)
            })
            .collect();
        root.insert("pads".into(), Value::Array(pads));
        // The blob table: names and lengths, in order. Offsets are IMPLICIT —
        // each blob begins where the last one ended — because an offset
        // written down is an offset that can disagree with the bytes.
        if !self.media.is_empty() {
            root.insert(
                "blobs".into(),
                Value::Array(
                    self.media
                        .iter()
                        .map(|b| {
                            let mut m = Map::new();
                            m.insert("name".into(), Value::from(b.name.clone()));
                            m.insert("len".into(), Value::from(b.bytes.len() as u64));
                            Value::Object(m)
                        })
                        .collect(),
                ),
            );
        }
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }

    /// **Manners identical to the sidecar's**: anything unreadable is an
    /// empty preset, never a panic and never a half-applied kit.
    pub fn from_json(text: &str) -> Self {
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return Self::default();
        };
        let mut out = Self {
            name: root
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            master: root
                .get("master")
                .and_then(Value::as_f64)
                .map_or(1.0, |v| v as f32),
            pads: Vec::new(),
            media: Vec::new(),
        };
        let Some(Value::Array(pads)) = root.get("pads") else {
            return out;
        };
        for p in pads {
            let Some(o) = p.as_object() else { continue };
            let Some(key) = o.get("key").and_then(Value::as_u64) else {
                continue;
            };
            let key = key as usize;
            if key >= DRUM_PADS {
                continue;
            }
            let Some(file) = o.get("file").and_then(Value::as_str) else {
                continue;
            };
            let num = |k: &str, d: f32| {
                o.get(k).and_then(Value::as_f64).map_or(d, |v| v as f32)
            };
            let mut params = DrumPadParams {
                gain: num("gain", 1.0),
                pan: num("pan", 0.0),
                tune: num("tune", 0.0),
                trim_start: num("trim_start", 0.0),
                trim_end: num("trim_end", 1.0),
                ..DrumPadParams::default()
            };
            if let Some(c) = o.get("choke").and_then(Value::as_u64) {
                params.choke = c as u8;
            }
            if let Some(b) = o.get("obey").and_then(Value::as_bool) {
                params.obey = b;
            }
            out.pads.push(KitPad {
                key,
                file: std::path::PathBuf::from(file),
                name: o
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                params,
                blob: o.get("blob").and_then(Value::as_u64).map(|b| b as usize),
            });
        }
        out
    }

    /// Write it, atomically, the way the sidecar is written.
    ///
    /// The extension is FORCED: a panel that returned "My Kit" or "My Kit.json"
    /// still produces a file the open panel's filter will show.
    pub fn save(&self, path: &std::path::Path) -> Result<std::path::PathBuf, String> {
        let path = path.with_extension(PRESET_EXT);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        let tmp = path.with_extension("tgkit.tmp");
        std::fs::write(&tmp, self.to_bytes()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    /// **The file, whole**: a text header, the manifest, then every sample's
    /// bytes end to end.
    ///
    /// ```text
    /// TGKIT2\n
    /// <manifest length in decimal>\n
    /// <manifest JSON>
    /// <blob 0><blob 1>...
    /// ```
    ///
    /// A container and not base64-in-JSON because a kit is mostly audio and
    /// base64 is a third again as large for it; a container and not a zip
    /// because a zip is a dependency, and a new crate in this app's lock file
    /// costs a THIRD-PARTY-LICENSES diff at release time. The header is ASCII
    /// so `head` on a mystery file says what it is.
    ///
    /// Blob ORDER is the contract, not offsets: each begins where the last
    /// ended. There is nothing to disagree with the bytes.
    pub fn to_bytes(&self) -> Vec<u8> {
        let manifest = self.to_json();
        let mut out = Vec::with_capacity(
            manifest.len() + 64 + self.media.iter().map(|b| b.bytes.len()).sum::<usize>(),
        );
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(format!("{}\n", manifest.len()).as_bytes());
        out.extend_from_slice(manifest.as_bytes());
        for blob in &self.media {
            out.extend_from_slice(&blob.bytes);
        }
        out
    }

    /// Read a file written by [`Self::to_bytes`], **or** one from before the
    /// samples travelled with the kit: those are plain JSON, and a preset
    /// whose pads name absolute paths and carry no blobs is exactly what the
    /// applying end already knows how to handle.
    pub fn from_bytes(raw: &[u8]) -> Result<Self, String> {
        if !raw.starts_with(MAGIC) {
            // The old form. Anything that is not valid UTF-8 was never one of
            // ours, and `from_json` answers rubbish with an empty preset, so
            // the caller's "no pads in it" is the message either way.
            let text = std::str::from_utf8(raw)
                .map_err(|_| "not a Tangent kit".to_owned())?;
            return Ok(Self::from_json(text));
        }
        let rest = &raw[MAGIC.len()..];
        let nl = rest
            .iter()
            .position(|b| *b == b'\n')
            .ok_or_else(|| "the kit's header is truncated".to_owned())?;
        let len: usize = std::str::from_utf8(&rest[..nl])
            .ok()
            .and_then(|s| s.trim().parse().ok())
            .ok_or_else(|| "the kit's manifest length is not a number".to_owned())?;
        let body = &rest[nl + 1..];
        if body.len() < len {
            return Err("the kit's manifest is truncated".to_owned());
        }
        let manifest = std::str::from_utf8(&body[..len])
            .map_err(|_| "the kit's manifest is not text".to_owned())?;
        let mut out = Self::from_json(manifest);
        // The blob table, then the bytes it describes.
        let table: Vec<(String, usize)> = serde_json::from_str::<Value>(manifest)
            .ok()
            .and_then(|v| v.get("blobs").cloned())
            .and_then(|v| match v {
                Value::Array(a) => Some(a),
                _ => None,
            })
            .unwrap_or_default()
            .iter()
            .filter_map(|b| {
                let o = b.as_object()?;
                Some((
                    o.get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("sample.wav")
                        .to_owned(),
                    o.get("len").and_then(Value::as_u64)? as usize,
                ))
            })
            .collect();
        let mut at = len;
        for (name, blen) in table {
            let end = at.saturating_add(blen);
            if end > body.len() {
                // Truncated. Keep what parsed — the layout is still worth
                // having, and every pad whose blob is missing falls back to
                // looking for the file.
                break;
            }
            out.media.push(KitBlob {
                name,
                bytes: body[at..end].to_vec(),
            });
            at = end;
        }
        // A pad pointing past the end of what actually arrived has no audio
        // in the file; let it fall back rather than index into nothing.
        let have = out.media.len();
        for pad in &mut out.pads {
            if pad.blob.is_some_and(|b| b >= have) {
                pad.blob = None;
            }
        }
        Ok(out)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let raw = std::fs::read(path).map_err(|e| e.to_string())?;
        let mut out = Self::from_bytes(&raw)?;
        if out.name.is_empty() {
            out.name = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
        }
        Ok(out)
    }
}

#[cfg(test)]
mod preset_tests {
    use super::*;

    fn tmp(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("tangent-kit-{name}"));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).expect("tmp dir");
        d
    }

    /// **A preset carries the layout across projects**, which is the whole
    /// reason it is not the sidecar: the sidecar's paths are `media/<file>`
    /// against ONE project folder, so the same file read from anywhere else
    /// points at samples that are not there.
    #[test]
    fn a_kit_saved_in_one_project_names_files_another_can_find() {
        let dir = tmp("roundtrip");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), b"riff").expect("sample");

        let mut kit = DrumpadDoc::default();
        kit.master = 0.75;
        kit.pads[36] = PadSlot {
            path: "media/kick.wav".to_owned(),
            name: "Kick".to_owned(),
            params: DrumPadParams { gain: 0.5, pan: -0.25, choke: 2, ..Default::default() },
            extra: Map::new(),
        };

        let preset = KitPreset::from_kit("My Kit", &kit, &doc);
        assert_eq!(preset.pads.len(), 1, "only the filled pad is written");
        let KitPad { key, file, name, params, .. } = &preset.pads[0];
        assert_eq!(*key, 36);
        assert_eq!(name, "Kick");
        assert!((params.gain - 0.5).abs() < 1e-6);
        // **Absolute, and it exists.** A relative path here is the bug this
        // type was made to avoid.
        assert!(file.is_absolute(), "the preset kept a relative path: {file:?}");
        assert!(file.exists(), "the preset points at nothing: {file:?}");

        // Round trip through the file, extension forced.
        let written = preset.save(&dir.join("My Kit")).expect("save");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some(PRESET_EXT));
        let back = KitPreset::load(&written).expect("load");
        assert_eq!(back.name, "My Kit");
        assert!((back.master - 0.75).abs() < 1e-6, "the kit fader");
        assert_eq!(back.pads.len(), 1);
        assert_eq!(back.pads[0].key, 36);
        assert_eq!(back.pads[0].file, *file, "the sample moved");
        assert_eq!(back.pads[0].params.choke, 2, "the choke group");
        assert!((back.pads[0].params.pan + 0.25).abs() < 1e-6, "the pan");
        // **And the audio came with it.** The whole point of the format:
        // the bytes that were on the pad are inside the kit file.
        assert_eq!(back.media.len(), 1, "the kit carried no samples");
        assert_eq!(back.media[0].name, "kick.wav");
        assert_eq!(back.media[0].bytes, b"riff", "the sample's bytes changed");
        assert_eq!(back.pads[0].blob, Some(0), "the pad lost its sample");

        // A name given only by the filename still comes back.
        let unnamed = KitPreset { name: String::new(), ..back.clone() };
        let at = unnamed.save(&dir.join("Second Kit")).expect("save");
        assert_eq!(KitPreset::load(&at).expect("load").name, "Second Kit");
    }

    /// Rubbish in is an empty preset, never a panic — the sidecar's manners,
    /// and for its reason: a file somebody hand-edited must not take the app
    /// down with it.
    #[test]
    fn an_unreadable_kit_is_an_empty_one() {
        for text in ["", "{", "[]", "{\"pads\": 7}", "{\"pads\": [{\"key\": 999}]}"] {
            let p = KitPreset::from_json(text);
            assert!(p.pads.is_empty(), "{text:?} produced pads");
        }
        // A pad past the keyboard's end is dropped, not clamped onto a real
        // one — a preset that quietly moved a sample would be worse than one
        // that lost it.
        let p = KitPreset::from_json(
            "{\"pads\": [{\"key\": 500, \"file\": \"/x.wav\"}, {\"key\": 4, \"file\": \"/y.wav\"}]}",
        );
        assert_eq!(p.pads.len(), 1);
        assert_eq!(p.pads[0].key, 4);
    }

    /// **A kit is shareable: the samples travel inside it.**
    ///
    /// The test deletes the sources before reading the file back, which is
    /// the honest simulation of the case this format exists for — a kit
    /// downloaded from somebody whose disk you have never seen. A preset that
    /// only recorded paths came back as a hundred and twenty-eight empty
    /// pads and an apology.
    #[test]
    fn a_kit_carries_its_own_samples_and_survives_losing_the_originals() {
        let dir = tmp("selfcontained");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![7u8; 900]).expect("kick");
        std::fs::write(media.join("hat.wav"), vec![3u8; 400]).expect("hat");

        let mut kit = DrumpadDoc::default();
        let slot = |file: &str| PadSlot {
            path: format!("media/{file}"),
            ..Default::default()
        };
        kit.pads[36] = slot("kick.wav");
        kit.pads[42] = slot("hat.wav");
        // The same sample on two pads at different tunings — one blob, two
        // pads, which is what the de-duplication is for.
        kit.pads[44] = PadSlot {
            path: "media/hat.wav".to_owned(),
            params: DrumPadParams { tune: 5.0, ..Default::default() },
            ..Default::default()
        };

        let preset = KitPreset::from_kit("Shared", &kit, &doc);
        assert_eq!(preset.media.len(), 2, "the same file was stored twice");
        let written = preset.save(&dir.join("Shared")).expect("save");
        // Bigger than its own manifest by the audio it carries.
        let size = std::fs::metadata(&written).expect("stat").len();
        assert!(size > 1_300, "the file is too small to hold the samples: {size}");

        // The disk the kit was made on is gone.
        std::fs::remove_dir_all(&media).expect("delete the sources");

        let back = KitPreset::load(&written).expect("load");
        assert_eq!(back.pads.len(), 3);
        assert_eq!(back.media.len(), 2);
        assert_eq!(back.media[0].bytes.len(), 900, "the kick's bytes");
        assert_eq!(back.media[1].bytes.len(), 400, "the hat's bytes");
        assert_eq!(back.pads[1].blob, back.pads[2].blob, "two pads, one sample");
        assert!(
            (back.pads[2].params.tune - 5.0).abs() < 1e-6,
            "the second pad's tuning"
        );
        assert!(
            back.pads.iter().all(|p| p.blob.is_some()),
            "a pad came back with no audio"
        );
    }

    /// The old form still reads: a plain-JSON kit from before the samples
    /// travelled is a kit whose pads name paths and carry no blobs, which is
    /// precisely what the applying end falls back to.
    #[test]
    fn a_kit_from_before_the_samples_travelled_still_opens() {
        let dir = tmp("legacyform");
        let at = dir.join("Old.tgkit");
        std::fs::write(
            &at,
            "{\"name\": \"Old\", \"master\": 1.0, \"pads\": [{\"key\": 38, \"file\": \"/somewhere/snare.wav\", \"gain\": 0.5}]}",
        )
        .expect("write");
        let back = KitPreset::load(&at).expect("load");
        assert_eq!(back.pads.len(), 1);
        assert_eq!(back.pads[0].key, 38);
        assert_eq!(back.pads[0].blob, None, "an old kit invented a blob");
        assert!(back.media.is_empty());
        assert_eq!(back.pads[0].file, std::path::Path::new("/somewhere/snare.wav"));
    }

    /// A truncated file gives up the audio it lost and keeps the layout,
    /// rather than refusing the kit or indexing into bytes that are not there.
    #[test]
    fn a_truncated_kit_keeps_its_layout() {
        let dir = tmp("truncated");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![1u8; 500]).expect("kick");
        let mut kit = DrumpadDoc::default();
        kit.pads[36] = PadSlot { path: "media/kick.wav".to_owned(), ..Default::default() };
        let whole = KitPreset::from_kit("Cut", &kit, &doc).to_bytes();
        let cut = &whole[..whole.len() - 200];
        let back = KitPreset::from_bytes(cut).expect("a truncated kit still parses");
        assert_eq!(back.pads.len(), 1, "the layout was lost with the audio");
        assert!(back.media.is_empty(), "half a sample was kept");
        assert_eq!(back.pads[0].blob, None, "a pad points at audio that is not there");
    }
}
