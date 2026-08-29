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
    pub fn load_all_beside(doc: &std::path::Path, lanes: usize) -> Vec<Self> {
        let mut out: Vec<Self> = (0..lanes).map(|_| Self::default()).collect();
        let Ok(text) = std::fs::read_to_string(Self::sidecar(doc)) else {
            return out;
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
        out
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
        let back = DrumpadDoc::load_all_beside(&doc, 16);
        assert_eq!(back.len(), 16);
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
        let back = DrumpadDoc::load_all_beside(&doc, 16);
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
