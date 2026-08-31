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
    /// `primary`, or on lane 0 when there is none. That is what makes an old
    /// project open sounding as it did: its single kit belongs to whichever
    /// channel was carrying the drumpad, and the app writes that number down
    /// the first time it saves.
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
    pub fn load_all_beside(doc: &std::path::Path, lanes: usize) -> (Vec<Self>, Option<usize>) {
        let mut out: Vec<Self> = (0..lanes).map(|_| Self::default()).collect();
        let Ok(text) = std::fs::read_to_string(Self::sidecar(doc)) else {
            return (out, None);
        };
        // The legacy kit, always — it is the whole file for a project
        // written before this existed.
        let legacy = Self::from_json(&text);
        let map = serde_json::from_str::<Value>(&text).ok();
        let obj = map.as_ref().and_then(Value::as_object);
        let primary = obj
            .and_then(|m| m.get("primary_lane"))
            .and_then(Value::as_u64)
            .map_or(0usize, |n| n as usize);
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
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 16);
        assert_eq!(back.len(), 16);
        // **The lane that was showing comes back with the kits.** Reading the
        // kits and forgetting which one was in front of you is how a project
        // reopens on somebody else's pads.
        assert_eq!(primary, Some(3), "the saved lane was not restored");
        assert_eq!(back[3].pads[36].path, "media/kick.wav", "lane 3's kit");
        assert_eq!(back[7].pads[36].path, "media/snare.wav", "lane 7's kit");
        assert!(back[0].is_empty(), "a lane with no kit came back with one");
        assert!((back[7].pads[36].params.gain - 0.5).abs() < 1e-6, "the knobs");

        // **An OLD sidecar — one kit, no lane keys — lands on lane 0.**
        // That is the only lane a pre-lanes build could have meant, and a
        // project that opened silent because its kit went nowhere would be
        // the worst possible way to ship this.
        let legacy = kit("legacy");
        std::fs::write(DrumpadDoc::sidecar(&doc), legacy.to_json()).expect("write legacy");
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 16);
        // A file with no opinion says so, rather than claiming lane 0 —
        // the caller's fallback is the same answer and it knows it guessed.
        assert_eq!(primary, None, "a pre-lanes file invented a primary lane");
        assert_eq!(
            back[0].pads[36].path, "media/legacy.wav",
            "a pre-lanes kit did not survive the upgrade"
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
    /// `(key, absolute source, pad name, params)`, in key order.
    pub pads: Vec<(usize, std::path::PathBuf, String, DrumPadParams)>,
    /// The kit's own fader.
    pub master: f32,
}

impl KitPreset {
    /// Take a snapshot of `kit`, resolving every pad against `doc`.
    ///
    /// A pad whose file has since gone missing is written anyway: the preset
    /// is a record of the layout, and a sample that is missing today may be
    /// on the disk the preset is carried to. Loading reports what it could
    /// not find rather than refusing the whole kit.
    pub fn from_kit(name: &str, kit: &DrumpadDoc, doc: &std::path::Path) -> Self {
        Self {
            name: name.to_owned(),
            master: kit.master,
            pads: kit
                .pads
                .iter()
                .enumerate()
                .filter(|(_, p)| !p.path.is_empty())
                .map(|(key, p)| {
                    (key, resolve(doc, &p.path), p.name.clone(), p.params)
                })
                .collect(),
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
            .map(|(key, file, name, params)| {
                let mut m = Map::new();
                m.insert("key".into(), Value::from(*key as u64));
                m.insert("file".into(), Value::from(file.to_string_lossy().into_owned()));
                m.insert("name".into(), Value::from(name.clone()));
                m.insert("gain".into(), Value::from(f64::from(params.gain)));
                m.insert("pan".into(), Value::from(f64::from(params.pan)));
                m.insert("tune".into(), Value::from(f64::from(params.tune)));
                m.insert("choke".into(), Value::from(u64::from(params.choke)));
                m.insert("obey".into(), Value::from(params.obey));
                m.insert("trim_start".into(), Value::from(f64::from(params.trim_start)));
                m.insert("trim_end".into(), Value::from(f64::from(params.trim_end)));
                Value::Object(m)
            })
            .collect();
        root.insert("pads".into(), Value::Array(pads));
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
            out.pads.push((
                key,
                std::path::PathBuf::from(file),
                o.get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                params,
            ));
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
        std::fs::write(&tmp, self.to_json()).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &path).map_err(|e| e.to_string())?;
        Ok(path)
    }

    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut out = Self::from_json(&text);
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
        let (key, file, name, params) = &preset.pads[0];
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
        assert_eq!(back.pads[0].0, 36);
        assert_eq!(back.pads[0].1, *file, "the sample moved");
        assert_eq!(back.pads[0].3.choke, 2, "the choke group");
        assert!((back.pads[0].3.pan + 0.25).abs() < 1e-6, "the pan");

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
        assert_eq!(p.pads[0].0, 4);
    }
}
