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
}

/// The kit, in memory.
#[derive(Debug, Clone, PartialEq)]
pub struct DrumpadDoc {
    pub pads: [PadSlot; DRUM_PADS],
    /// The kit's own fader, linear.
    pub master: f32,
}

impl Default for DrumpadDoc {
    fn default() -> Self {
        Self {
            pads: std::array::from_fn(|_| PadSlot::default()),
            master: 1.0,
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
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) else {
            return out;
        };
        if let Some(m) = map.get("master").and_then(Value::as_f64) {
            out.master = (m as f32).clamp(0.0, 2.0);
        }
        let Some(Value::Array(pads)) = map.get("pads") else {
            return out;
        };
        // **The sixteen-pad era loads at its old home.** Before the pads
        // covered the whole keyboard, the sidecar held exactly sixteen
        // slots and they WERE keys 36..51 — so a sixteen-entry file lands
        // at offset 36, and every old kit keeps striking the pads it
        // always struck. Full-width files map straight through.
        let offset = if pads.len() == 16 { 36 } else { 0 };
        for (slot, v) in out.pads.iter_mut().skip(offset).zip(pads) {
            let Value::Object(p) = v else { continue };
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
        }
        out
    }

    fn to_json(&self) -> String {
        let mut map = Map::new();
        map.insert("version".into(), Value::from(1));
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
                Value::Object(p)
            })
            .collect();
        map.insert("pads".into(), Value::Array(pads));
        serde_json::to_string_pretty(&Value::Object(map)).unwrap_or_else(|_| "{}".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
