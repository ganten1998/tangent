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

use crate::arrangement::Fingerprint;
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

/// The keys the LANES-era sidecar writes at its root beside the primary
/// kit's own — see `save_all_beside`. This file format's keys, not a
/// stranger's, so a kit read from that root never carries them in `extra`.
/// It did, once: the primary kit came back wearing `primary_lane` and a
/// stale copy of the whole `lane_kits` object whenever its own lane was
/// empty, which made the first undo after every open see a kit that
/// "differed" and rewrite the sidecar for nothing.
const SIDECAR_KEYS: [&str; 2] = ["primary_lane", "lane_kits"];

/// **The format-2 sidecar's kits, by track** (design 6.8): `"t<id>": kit`.
/// Present, it is the whole answer; absent, the file is legacy.
const TRACK_KITS: &str = "track_kits";

/// The format-2 sidecar's panel track, `"t<id>"`: `primary_lane` by
/// identity.
const PRIMARY_TRACK: &str = "primary_track";

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
        Self::load_all_beside_ids(doc, &crate::desk_legacy::identity_ids(lanes), legacy_lane)
    }

    /// **Every track's kit, by identity** (design 6.8): one per entry of
    /// `ids`, the desk's rows in lane order.
    ///
    /// A file with `track_kits` (`"t<id>": kit`) is read by id alone, its
    /// `primary_track` naming the panel's track: a kit follows its TRACK,
    /// so a reorder saved in the kit but not yet in the document lands
    /// every kit on its own track when both are next read. Such a file
    /// ignores a top-level kit, `lane_kits` and `primary_lane` entirely:
    /// they can only be strays from a legacy build that saved over it.
    ///
    /// Without `track_kits` the file is legacy (only legacy builds wrote
    /// those keys) and reads as it always did, its lanes read as ids by the
    /// identity rule: legacy row r is `TrackId(r)`.
    pub fn load_all_beside_ids(
        doc: &std::path::Path,
        ids: &[crate::desk_id::TrackId],
        legacy_lane: Option<usize>,
    ) -> (Vec<Self>, Option<usize>) {
        let lanes = ids.len();
        let mut out: Vec<Self> = (0..lanes).map(|_| Self::default()).collect();
        let Ok(text) = std::fs::read_to_string(Self::sidecar(doc)) else {
            return (out, None);
        };
        // **Parsed once.** The text used to be parsed twice — once as the
        // legacy kit and once for the lane keys — and every lane's kit was
        // then serialised back to text and parsed a third time, a
        // `Value -> String -> Value` trip per lane for nothing. A file that
        // is not an object reads as it always did: every lane empty, no
        // opinion about the primary.
        let Ok(Value::Object(mut root)) = serde_json::from_str::<Value>(&text) else {
            return (out, None);
        };
        if let Some(Value::Object(kits)) = root.shift_remove(TRACK_KITS) {
            let lane_of = |key: &str| {
                let id = key.strip_prefix('t')?.parse::<u32>().ok()?;
                // Canonical spelling only, the grammar's rule: `t03` is
                // not `t3`.
                (format!("t{id}") == key).then_some(())?;
                ids.iter().position(|t| t.0 == id)
            };
            for (key, v) in kits {
                // A kit for a track this desk does not hold is left out:
                // there is no lane to put it on.
                let Some(lane) = lane_of(&key) else { continue };
                out[lane] = match v {
                    Value::Object(m) => Self::from_value(m),
                    _ => Self::default(),
                };
            }
            let primary = root.get(PRIMARY_TRACK).and_then(Value::as_str).and_then(lane_of);
            return (out, primary);
        }
        // The file's own word first; the desk's answer second; lane 0 only
        // when neither has an opinion.
        let said = root.get("primary_lane").and_then(Value::as_u64).map(|n| n as usize);
        let primary = said.unwrap_or_else(|| legacy_lane.unwrap_or(0));
        // Only if the file actually said so, and said something in range. A
        // project written before `primary_lane` existed has no opinion, and
        // inventing lane 0 for it would be the same guess the caller
        // already makes.
        let stated = said.filter(|n| *n < out.len());
        // The lane kits leave the root before the legacy kit is read from
        // it, so they are neither parsed as its strangers nor cloned.
        // `shift_remove`, because `remove` is a swap under `preserve_order`
        // and would reorder the root's real strangers.
        let lane_kits = root.shift_remove("lane_kits");
        // The legacy kit, always — it is the whole file for a project
        // written before this existed.
        if primary < out.len() {
            out[primary] = Self::from_value(root);
        }
        // Then anything the newer key names, which overwrites the legacy
        // placement when it speaks about the same lane. A lane whose entry
        // is not an object reads as the empty kit, as it always did.
        if let Some(Value::Object(kits)) = lane_kits {
            for (key, v) in kits {
                let Ok(lane) = key.parse::<usize>() else {
                    continue;
                };
                if lane >= out.len() {
                    continue;
                }
                out[lane] = match v {
                    Value::Object(m) => Self::from_value(m),
                    _ => Self::default(),
                };
            }
        }
        (out, stated)
    }

    /// **Every kit in the sidecar beside `doc`, by the track it is**, empty
    /// kits left out (an empty kit is the absence of one, the writers'
    /// rule). For a caller with no desk to read the file against: a scratch
    /// whose document is missing, or two sidecars compared kit for kit.
    ///
    /// A `track_kits` file names its tracks itself, so every kit in it is
    /// here whatever desk wrote it: read through the identity desk, a kit
    /// on a track minted past 31 (`t32`, every track Add makes) had no lane
    /// and vanished, and the launch sweep then took a scratch holding one
    /// for an empty clone and deleted it, kit and all. A legacy file reads
    /// as it always did, its lanes as ids by the identity rule.
    pub fn load_by_track(
        doc: &std::path::Path,
        legacy_lane: Option<usize>,
    ) -> std::collections::BTreeMap<crate::desk_id::TrackId, Self> {
        let named: Option<Vec<crate::desk_id::TrackId>> = std::fs::read_to_string(Self::sidecar(doc))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .and_then(|v| match v.get(TRACK_KITS) {
                // The grammar's own parse: canonical spellings only, the
                // rule `load_all_beside_ids` keeps.
                Some(Value::Object(kits)) => Some(kits.keys().filter_map(|k| k.parse().ok()).collect()),
                _ => None,
            });
        let ids = named.unwrap_or_else(|| crate::desk_legacy::identity_ids(crate::recorder::CHANNELS));
        let (kits, _) = Self::load_all_beside_ids(doc, &ids, legacy_lane);
        ids.into_iter().zip(kits).filter(|(_, k)| !k.is_empty()).collect()
    }

    /// **Every lane's kit into the one sidecar**, its lanes read as ids by
    /// the identity rule. The host's road until it holds the desk's ids;
    /// the app writes through [`Self::save_all_beside_ids`].
    pub fn save_all_beside(kits: &[Self], primary: usize, doc: &std::path::Path) {
        let ids = crate::desk_legacy::identity_ids(kits.len());
        Self::save_all_beside_as(kits, &ids, primary, doc, crate::desk_format::Format::current());
    }

    /// **Every track's kit into the one sidecar**: `kits[lane]` is the kit
    /// of the track `ids[lane]`. Format 2 when [`crate::desk_format::WRITE_V8`]
    /// says so, today's bytes otherwise.
    pub fn save_all_beside_ids(
        kits: &[Self],
        ids: &[crate::desk_id::TrackId],
        primary: usize,
        doc: &std::path::Path,
    ) {
        Self::save_all_beside_as(kits, ids, primary, doc, crate::desk_format::Format::current());
    }

    /// The sidecar in `format`. **Format 2 names each kit by its track**
    /// (`track_kits`, `primary_track`) and nothing else: no lane key and no
    /// top-level kit is ever written again, so a reorder can never strand a
    /// kit on the lane its track left. A kit whose lane has no id is not
    /// written; the desk is the one that says which tracks exist.
    pub(crate) fn save_all_beside_as(
        kits: &[Self],
        ids: &[crate::desk_id::TrackId],
        primary: usize,
        doc: &std::path::Path,
        format: crate::desk_format::Format,
    ) {
        if format == crate::desk_format::Format::Legacy {
            Self::save_all_beside_legacy(kits, primary, doc);
            return;
        }
        let mut track_kits = Map::new();
        for (lane, kit) in kits.iter().enumerate() {
            let Some(id) = ids.get(lane) else { continue };
            // An empty kit is the absence of one, the legacy writer's rule.
            if !kit.is_empty() {
                track_kits.insert(id.to_string(), Value::Object(kit.to_value()));
            }
        }
        let mut root = Map::new();
        root.insert(TRACK_KITS.into(), Value::Object(track_kits));
        if let Some(id) = ids.get(primary) {
            root.insert(PRIMARY_TRACK.into(), Value::String(id.to_string()));
        }
        let path = Self::sidecar(doc);
        let text = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
        crate::settings::write_atomic(&path, text.as_bytes());
    }

    /// Today's bytes: the primary kit at the top level for a pre-lanes
    /// build, `lane_kits` by lane, `primary_lane`.
    fn save_all_beside_legacy(kits: &[Self], primary: usize, doc: &std::path::Path) {
        // The kit's map straight into the root. This used to pretty-print
        // each kit to text and parse it back into a `Value` — 128 pads
        // through the printer and the parser per kit per save — to arrive
        // at the map `to_value` builds in the first place. Byte-identical
        // output: `the_sidecar_is_written_as_it_always_was` builds the file
        // the old way and compares.
        let mut root = kits.get(primary).map(Self::to_value).unwrap_or_default();
        root.insert("primary_lane".into(), Value::from(primary as u64));
        let mut lane_kits = Map::new();
        for (lane, kit) in kits.iter().enumerate() {
            // An empty kit is the absence of one; writing it would grow the
            // file by a lane nobody has for no reading anybody does.
            if kit.is_empty() {
                continue;
            }
            lane_kits.insert(lane.to_string(), Value::Object(kit.to_value()));
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
        match serde_json::from_str::<Value>(text) {
            Ok(Value::Object(map)) => Self::from_value(map),
            // Garbage and non-objects answer the empty kit — the settings
            // file's manners, pinned by `bad_documents_answer_the_empty_kit`.
            _ => Self::default(),
        }
    }

    /// A kit from its parsed object — the text readers and the lanes-era
    /// sidecar reader all come through here, so a file is parsed once
    /// however many kits it holds.
    fn from_value(mut map: Map<String, Value>) -> Self {
        let mut out = Self::default();
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
        for known in KIT_KEYS.iter().chain(SIDECAR_KEYS.iter()) {
            map.shift_remove(*known);
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
        serde_json::to_string_pretty(&Value::Object(self.to_value()))
            .unwrap_or_else(|_| "{}".into())
    }

    /// The kit as the object the file holds — what `to_json` prints and
    /// what `save_all_beside` puts under a lane, built once either way.
    fn to_value(&self) -> Map<String, Value> {
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
        map
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

        // And the LEGACY writer's top level still says what an old build
        // would read, so a project it saved does not come up empty over
        // there. The format-2 sidecar names each kit by its track and writes
        // no top-level kit and no lane key at all (design 6.8).
        let identity = crate::desk_legacy::identity_ids(kits.len());
        DrumpadDoc::save_all_beside_as(&kits, &identity, 3, &doc, crate::desk_format::Format::Legacy);
        let old_reader = DrumpadDoc::load_beside(&doc);
        assert_eq!(
            old_reader.pads[36].path, "media/kick.wav",
            "an older build would open this project with no kit at all"
        );
        DrumpadDoc::save_all_beside(&kits, 3, &doc);
        let root: Value = serde_json::from_str(&std::fs::read_to_string(DrumpadDoc::sidecar(&doc)).expect("the sidecar"))
            .expect("json");
        for legacy in ["lane_kits", "primary_lane", "pads", "master"] {
            assert!(root.get(legacy).is_none(), "the format-2 sidecar still writes `{legacy}`");
        }
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

    /// **The lanes-era root keys are the file's, not the primary kit's
    /// strangers.** Kits on lanes 0 and 5, the panel left on an empty lane
    /// 3: the kit read for lane 3 is the empty kit, with nothing in `extra`
    /// — not a kit wearing `primary_lane` and a copy of every other lane.
    /// Through the one-kit reader too, which reads the same root.
    #[test]
    fn the_lanes_keys_are_the_files_not_a_kits_strangers() {
        let dir = std::env::temp_dir().join(format!(
            "tangent-lanekeys-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let doc = dir.join("project.tangent");
        let mut kits: Vec<DrumpadDoc> = (0..8).map(|_| DrumpadDoc::default()).collect();
        kits[0].pads[36].path = "media/kick.wav".into();
        kits[5].pads[40].path = "media/hat.wav".into();
        kits[5].extra.insert("from_tomorrow".into(), Value::from(1));
        DrumpadDoc::save_all_beside(&kits, 3, &doc);
        let (back, primary) = DrumpadDoc::load_all_beside(&doc, 8, None);
        assert_eq!(primary, Some(3), "the file's primary was not reported");
        assert_eq!(back[3], DrumpadDoc::default(), "lane 3 came back wearing the root's keys");
        assert!(back[3].extra.is_empty(), "lane 3's extra: {:?}", back[3].extra);
        assert_eq!(back[0].pads[36].path, "media/kick.wav");
        assert_eq!(back[5].pads[40].path, "media/hat.wav");
        assert_eq!(
            back[5].extra.get("from_tomorrow"),
            Some(&Value::from(1)),
            "a real stranger on a lane kit was lost"
        );
        // The one-kit reader is an older build's, which only ever met the
        // LANES-era root: written by the legacy writer for it.
        DrumpadDoc::save_all_beside_as(
            &kits,
            &crate::desk_legacy::identity_ids(kits.len()),
            3,
            &doc,
            crate::desk_format::Format::Legacy,
        );
        let one = DrumpadDoc::load_beside(&doc);
        assert!(one.extra.is_empty(), "the one-kit reader kept the root's keys: {:?}", one.extra);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// **The sidecar's bytes are what they always were.** `save_all_beside`
    /// used to print each kit and parse it back to build the root; it uses
    /// the kit's map directly now, and this builds the file the old way
    /// and compares. With `float_roundtrip` on, the parse of the printed
    /// float is exact, so the two roads agree byte for byte.
    #[test]
    fn the_sidecar_is_written_as_it_always_was() {
        let dir = std::env::temp_dir().join(format!(
            "tangent-sidecar-bytes-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::create_dir_all(&dir);
        let doc = dir.join("project.tangent");
        let mut kits: Vec<DrumpadDoc> = (0..4).map(|_| DrumpadDoc::default()).collect();
        kits[1].master = 0.8;
        kits[1].pads[36].path = "media/kick.wav".into();
        kits[1].pads[36].params.gain = 1.3;
        kits[1].pads[36].params.tune = -0.7;
        kits[1].pads[36].extra.insert("reverse".into(), Value::Bool(true));
        kits[2].pads[0].path = "media/snare.wav".into();
        kits[2].pads[0].params.pan = 0.25;
        kits[2].extra.insert("swing".into(), Value::from(0.24));
        // The LEGACY writer's bytes, which a format-2 sidecar no longer
        // writes (design 6.8) and every legacy project still holds.
        DrumpadDoc::save_all_beside_as(
            &kits,
            &crate::desk_legacy::identity_ids(kits.len()),
            1,
            &doc,
            crate::desk_format::Format::Legacy,
        );
        let written = std::fs::read_to_string(DrumpadDoc::sidecar(&doc)).expect("the sidecar");

        // The old road, verbatim.
        let mut root = serde_json::from_str::<Value>(&kits[1].to_json())
            .ok()
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        root.insert("primary_lane".into(), Value::from(1u64));
        let mut lane_kits = Map::new();
        for (lane, kit) in kits.iter().enumerate() {
            if kit.is_empty() {
                continue;
            }
            if let Ok(v) = serde_json::from_str::<Value>(&kit.to_json()) {
                lane_kits.insert(lane.to_string(), v);
            }
        }
        root.insert("lane_kits".into(), Value::Object(lane_kits));
        let expected = serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default();
        assert_eq!(written, expected, "the sidecar's bytes moved");
        let _ = std::fs::remove_dir_all(&dir);
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

    /// **The kits into a [`Fingerprint`]**, walking beside `compact_all`.
    ///
    /// The undo timeline compares the COMPACT form, so this hashes exactly
    /// what that writes and nothing else: a lane the compaction skips is not
    /// in the number, and neither are the two things it drops — the kit's
    /// `version` and its kit-level `extra`. Hashing them would report an
    /// edit the moment nobody could ever undo.
    ///
    /// Mixed into a fingerprint the caller already owns rather than
    /// answering one of its own, so the arrangement and its sidecar make ONE
    /// number. Two numbers is two things to keep in step.
    pub fn hash_all_into(kits: &[Self], f: &mut Fingerprint) {
        for (lane, kit) in kits.iter().enumerate() {
            if kit.is_empty() && (kit.master - 1.0).abs() < f32::EPSILON {
                continue;
            }
            // The lane's own number and its pad COUNT frame the run, so
            // "lane 0 holding two pads" cannot read as two lanes holding one.
            f.len(lane);
            f.f64(f64::from(kit.master));
            f.len(kit.pads.iter().filter(|p| !p.path.is_empty()).count());
            for (key, pad) in kit.pads.iter().enumerate() {
                if pad.path.is_empty() {
                    continue;
                }
                f.len(key);
                // The strangers FIRST, because that is the order the
                // compaction builds the object in — `preserve_order` makes
                // that order part of the text.
                f.map(&pad.extra);
                f.str(&pad.path);
                f.str(&pad.name);
                f.f64(f64::from(pad.params.gain));
                f.f64(f64::from(pad.params.pan));
                f.f64(f64::from(pad.params.tune));
                f.u64(u64::from(pad.params.choke));
                f.bool(pad.params.obey);
                f.f64(f64::from(pad.params.trim_start));
                f.f64(f64::from(pad.params.trim_end));
            }
        }
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

/// The extension a NEW saved kit wears. It was `tgkit`; the owner wanted
/// `.kit`, and the container's magic is what actually identifies the file —
/// so saves changed name and loads never will: [`PRESET_EXT_OLD`] stays in
/// every file filter forever, and [`KitPreset::from_bytes`] sniffs the bytes
/// whatever the name says.
pub const PRESET_EXT: &str = "kit";

/// The extension every kit saved before 5.0 wears. Read forever, written
/// never again.
pub const PRESET_EXT_OLD: &str = "tgkit";

/// The first bytes of a kit file that carries its own samples. Version 2 —
/// version 1 was the same manifest as bare JSON with no audio at all, and
/// [`KitPreset::from_bytes`] still reads it.
const MAGIC: &[u8] = b"TGKIT2\n";

/// **How much audio one kit may carry inside it: 256 MiB.**
///
/// Version 1 was a manifest and nothing else, and a ceiling on it would have
/// been a ceiling on a few kilobytes of JSON. Version 2 puts the samples in
/// the file, and nobody went back and asked what the biggest one could be —
/// so today the answer is "as much as the pads point at", and the cost of
/// that is paid three times over on the UI thread with no window to say so:
/// [`KitPreset::from_kit`] holds every blob, [`KitPreset::to_bytes`]
/// allocates a second buffer the size of all of them plus the manifest, and
/// [`KitPreset::save`] hands that whole thing to one synchronous
/// `fs::write`. A 500 MB kit therefore peaks near a gigabyte, and a Rust
/// allocation that cannot be served does not return an error — it aborts the
/// process. The session goes with it.
///
/// So the ceiling is not a policy about what a kit ought to be. It is the
/// line between "a sentence in the status strip" and "Tangent quit and the
/// last hour is gone".
///
/// **It never refuses the save.** Past the ceiling the remaining pads keep
/// their absolute source paths and no blob, which is exactly the shape of a
/// version 1 kit — a form every reader in this module already handles — and
/// the count comes back in [`KitEmbedReport`] so the writing end can say out
/// loud that this kit will not travel. Refusing would be the one behaviour
/// that destroys work somebody thought they had saved.
///
/// 256 MiB because a drum kit is one-shots: a hundred and twenty-eight pads
/// of 24-bit stereo 48 kHz one-shots at two seconds each is about 74 MB, so
/// the ceiling is more than three times the fattest kit anyone would build
/// pad by pad, while still bounding the peak at half a gigabyte on a machine
/// that is already running a DAW.
pub const MAX_KIT_BYTES: usize = 256 * 1024 * 1024;

/// **How large a kit FILE may be before it is read at all: 512 MiB.**
///
/// The mirror of [`MAX_KIT_BYTES`], and it has to be a separate, larger
/// number rather than the same one. [`KitPreset::load`] reads the whole file
/// and then [`KitPreset::from_bytes`] copies each blob out of it, so opening
/// a kit peaks at twice the file — and a kit written by a build from before
/// the ceiling existed can be any size at all. Set the two equal and Tangent
/// would refuse to reopen files it wrote itself last week, which is a worse
/// failure than the one being prevented.
///
/// Twice the write ceiling leaves room for the manifest and for every kit
/// this build can produce, and still turns a two-gigabyte file — a truncated
/// download, a rename of something that was never a kit, a hostile file —
/// into a sentence instead of four gigabytes of allocation.
///
/// Note what does NOT need a ceiling: a manifest that LIES about its blob
/// lengths cannot make `from_bytes` allocate anything, because every copy is
/// bounded by `body.len()` before it is taken and a length that runs past the
/// end stops the table. The hazard is only ever the real size of the file on
/// the disk, which is why the guard is a `metadata` call before the read.
pub const MAX_KIT_FILE_BYTES: u64 = 512 * 1024 * 1024;

/// **Does this file look like a kit rather than a sample?**
///
/// Asked on the drop road, where a `.tgkit` a user downloaded and dragged
/// onto the pads was being copied into `media/` as though it were audio —
/// leaving a pad pointing at a container the decoder cannot read, which is a
/// dead pad and no message.
///
/// The extension answers first because it is free and because it is what the
/// save panel forces. The magic sniff is the second answer, for the file that
/// came out of a zip with its extension eaten or was renamed on the way: the
/// header is ASCII on purpose (see [`KitPreset::to_bytes`]), so recognising it
/// costs one open and seven bytes.
///
/// A version 1 kit — bare JSON, no magic — is caught by its extension alone.
/// That is the whole of what can be done for it: sniffing JSON would mean
/// parsing every dropped file to find out whether it was audio, and a WAV
/// whose first bytes happen to parse is not a risk worth inventing.
pub fn looks_like_a_kit(path: &std::path::Path) -> bool {
    // **Both spellings.** New saves wear `.kit` and every `.tgkit` ever
    // written still opens (`from_bytes` sniffs the magic, so the name is only
    // ever a filter) — so a sniffer that knew one of them would send half the
    // kits in existence down the sample road. This function was written
    // against a tree that predated the rename and knew only one name; the
    // merge is what exposed it, and the test below is what caught it.
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| {
            e.eq_ignore_ascii_case(PRESET_EXT) || e.eq_ignore_ascii_case(PRESET_EXT_OLD)
        })
    {
        return true;
    }
    use std::io::Read as _;
    let mut head = [0u8; MAGIC.len()];
    std::fs::File::open(path).is_ok_and(|mut f| f.read_exact(&mut head).is_ok()) && head == *MAGIC
}

/// **What a snapshot could not put inside the kit file.**
///
/// [`KitPreset::from_kit`] degrades rather than fails — a pad whose sample
/// cannot be embedded keeps its absolute path and no blob — and until this
/// existed the degrade was completely silent. The save reported plain
/// success, and the user learned that the kit was not portable when somebody
/// else opened it and got a hundred and twenty-eight empty pads.
///
/// Counted per DISTINCT SOURCE FILE, not per pad: a kit with the same
/// unreadable hat on four pads has lost one sample, and saying "4" would
/// overstate what went wrong.
///
/// **Fields are append-only** — nothing here is serialized today, but this is
/// the report a future save dialog would render, and a renumbering would be a
/// silent lie rather than a compile error.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct KitEmbedReport {
    /// Source files the snapshot could not read at all: deleted, on an
    /// unmounted volume, or not ours to read.
    pub unreadable: usize,
    /// Source files left out because the kit had already reached
    /// [`MAX_KIT_BYTES`]. Readable, and deliberately not taken.
    pub over_ceiling: usize,
    /// Source files whose bytes are really in the file. The denominator for
    /// anything the caller wants to say about the other two.
    pub embedded: usize,
}

impl KitEmbedReport {
    /// Whether this kit is whole — every sample it names is inside it.
    pub fn is_complete(&self) -> bool {
        self.unreadable == 0 && self.over_ceiling == 0
    }
}

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
    ///
    /// **The bytes it could not take are counted** — see
    /// [`Self::from_kit_within`], which this is the plain form of. Callers
    /// that write the file to disk should use that one and say what came
    /// back; a degrade nobody is told about is indistinguishable, from the
    /// chair, from a kit that works.
    pub fn from_kit(name: &str, kit: &DrumpadDoc, doc: &std::path::Path) -> Self {
        Self::from_kit_within(name, kit, doc, MAX_KIT_BYTES).0
    }

    /// The same snapshot, with the embedding ceiling named and the shortfall
    /// reported.
    ///
    /// **Why the ceiling is a parameter and not just [`MAX_KIT_BYTES`]:** the
    /// only way to prove a 256 MiB ceiling with the constant baked in is to
    /// write a quarter of a gigabyte of temporary files, and a test that does
    /// that is a test that gets marked `#[ignore]` within a month and then
    /// stops being a guard at all. The seam costs one argument and lets
    /// `a_kit_past_the_ceiling_keeps_the_layout_and_drops_the_audio` prove the
    /// real branch with two five-hundred-byte files. Every shipping caller
    /// passes `MAX_KIT_BYTES`.
    ///
    /// **The ceiling is checked before the bytes are added, never after.** A
    /// sample that would carry the total past the line is not embedded at
    /// all — it is not truncated, and it does not push the total over and get
    /// caught next time around. A half sample in the file would parse, land in
    /// `media/`, and play as a click.
    ///
    /// A file that fails to read and a file that is past the ceiling both
    /// leave `blob: None`, which is the version 1 shape and needs no new
    /// handling anywhere downstream. They are counted separately because they
    /// deserve different sentences: one is "your sample moved", the other is
    /// "this kit is too big to carry itself".
    pub fn from_kit_within(
        name: &str,
        kit: &DrumpadDoc,
        doc: &std::path::Path,
        ceiling: usize,
    ) -> (Self, KitEmbedReport) {
        let mut media: Vec<KitBlob> = Vec::new();
        // **One entry per distinct file, and it remembers the ANSWER.** A kit
        // with the same hat on two pads at different tunings is one sample,
        // and a preset that wrote it twice would be twice the size for
        // nothing. Holding `Option<usize>` rather than only the paths that
        // worked means a file that could not be read is not read again once
        // per pad that names it, and the counts below stay per-file rather
        // than per-pad.
        let mut seen: Vec<(std::path::PathBuf, Option<usize>)> = Vec::new();
        let mut pads = Vec::new();
        let mut report = KitEmbedReport::default();
        let mut carried = 0usize;
        for (key, p) in kit.pads.iter().enumerate() {
            if p.path.is_empty() {
                continue;
            }
            let file = resolve(doc, &p.path);
            let blob = match seen.iter().find(|(s, _)| *s == file) {
                Some((_, at)) => *at,
                None => {
                    let at = match std::fs::read(&file) {
                        Err(_) => {
                            report.unreadable += 1;
                            None
                        }
                        Ok(bytes) if carried.saturating_add(bytes.len()) > ceiling => {
                            report.over_ceiling += 1;
                            None
                        }
                        Ok(bytes) => {
                            carried += bytes.len();
                            report.embedded += 1;
                            let label = file
                                .file_name()
                                .map(|n| n.to_string_lossy().into_owned())
                                .unwrap_or_else(|| format!("pad-{key}.wav"));
                            media.push(KitBlob { name: label, bytes });
                            Some(media.len() - 1)
                        }
                    };
                    seen.push((file.clone(), at));
                    at
                }
            };
            pads.push(KitPad {
                key,
                file,
                name: p.name.clone(),
                params: p.params,
                blob,
            });
        }
        (
            Self {
                name: name.to_owned(),
                master: kit.master,
                pads,
                media,
            },
            report,
        )
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
        let tmp = path.with_extension("kit.tmp");
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

    /// Read a kit from disk, refusing an absurd file **before** it is read.
    ///
    /// See [`MAX_KIT_FILE_BYTES`] for the number and the argument. The short
    /// version: opening a kit costs twice the file in RAM, on the UI thread,
    /// and an allocation that cannot be served aborts the process rather than
    /// returning something this `Result` could carry. A `metadata` call is the
    /// only place a two-gigabyte file can still be turned into a sentence.
    ///
    /// A file whose size cannot be read at all is let through: that is a
    /// permissions or race question, and `fs::read` below is about to give a
    /// far better message about it than a guess here would.
    pub fn load(path: &std::path::Path) -> Result<Self, String> {
        Self::load_within(path, MAX_KIT_FILE_BYTES)
    }

    /// [`Self::load`] with the ceiling named, for the same reason
    /// [`Self::from_kit_within`] takes one: proving a 512 MiB guard with the
    /// constant baked in would mean writing half a gigabyte in a unit test.
    /// Every shipping caller passes [`MAX_KIT_FILE_BYTES`].
    pub fn load_within(path: &std::path::Path, max_file: u64) -> Result<Self, String> {
        if let Ok(len) = std::fs::metadata(path).map(|m| m.len()) {
            if len > max_file {
                return Err(format!(
                    "that file is {} MB — too big to be a kit, and reading it \
                     would take twice that in memory",
                    len / (1024 * 1024)
                ));
            }
        }
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

    /// **The extension changed; no file did.** New saves wear `.kit`, and a
    /// byte-identical TGKIT2 blob opens under either name because
    /// [`KitPreset::from_bytes`] reads the magic, never the filename — which
    /// is what makes the rename safe to make and impossible to regret.
    #[test]
    fn the_same_kit_bytes_open_under_both_extensions_forever() {
        let dir = tmp("dualext");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![9u8; 300]).expect("kick");
        let mut kit = DrumpadDoc::default();
        kit.pads[36] = PadSlot { path: "media/kick.wav".to_owned(), ..Default::default() };
        let blob = KitPreset::from_kit("Dual", &kit, &doc).to_bytes();

        // One set of bytes, the old name and the new.
        let old = dir.join("Dual.tgkit");
        let new = dir.join("Dual.kit");
        std::fs::write(&old, &blob).expect("write .tgkit");
        std::fs::write(&new, &blob).expect("write .kit");
        let from_old = KitPreset::load(&old).expect("a .tgkit still opens");
        let from_new = KitPreset::load(&new).expect("a .kit opens");
        assert_eq!(from_old, from_new, "the name changed what the bytes meant");
        assert_eq!(from_old.pads.len(), 1);
        assert_eq!(from_old.media.len(), 1, "the audio did not survive the name");

        // And a fresh save wears the new name.
        assert_eq!(PRESET_EXT, "kit");
        let written = from_new.save(&dir.join("Fresh")).expect("save");
        assert_eq!(written.extension().and_then(|e| e.to_str()), Some("kit"));
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

    /// **Past the ceiling the kit keeps its layout and gives up the audio,
    /// and it never refuses to save.**
    ///
    /// The whole point of the ceiling is that the failure it prevents — a
    /// synchronous allocation of twice a very large kit on the UI thread,
    /// which aborts the process rather than returning an error — costs the
    /// user the session, while the failure it introduces costs them a
    /// sentence. So this asserts BOTH halves: the pad past the line is still
    /// in the preset, still holds its absolute source path, and is still
    /// pointed at by nothing; and the count comes back so somebody can be
    /// told.
    ///
    /// Six hundred bytes of ceiling rather than 256 MiB, through
    /// `from_kit_within` — the argument for that seam is on the function.
    #[test]
    fn a_kit_past_the_ceiling_keeps_the_layout_and_drops_the_audio() {
        let dir = tmp("ceiling");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![1u8; 500]).expect("kick");
        std::fs::write(media.join("snare.wav"), vec![2u8; 500]).expect("snare");

        let mut kit = DrumpadDoc::default();
        kit.pads[36] = PadSlot { path: "media/kick.wav".to_owned(), ..Default::default() };
        kit.pads[38] = PadSlot { path: "media/snare.wav".to_owned(), ..Default::default() };

        let (preset, report) = KitPreset::from_kit_within("Fat", &kit, &doc, 600);
        assert_eq!(preset.pads.len(), 2, "the ceiling ate a pad off the layout");
        assert_eq!(preset.media.len(), 1, "the ceiling did not stop the embedding");
        assert_eq!(preset.pads[0].blob, Some(0), "the first sample fitted and is not here");
        assert_eq!(preset.pads[1].blob, None, "a sample past the ceiling was embedded anyway");
        assert_eq!(
            preset.pads[1].file,
            media.join("snare.wav"),
            "the pad past the ceiling lost the path it has to fall back to"
        );
        assert_eq!(report.embedded, 1);
        assert_eq!(report.over_ceiling, 1, "the shortfall was not counted");
        assert_eq!(report.unreadable, 0, "a readable file was called unreadable");
        assert!(!report.is_complete(), "a kit missing a sample called itself whole");

        // And it still WRITES. A ceiling that refused the save would destroy
        // work the user believed they had.
        let at = dir.join("Fat.tgkit");
        let written = preset.save(&at).expect("the ceiling refused the save");
        assert!(written.is_file(), "nothing was written");
        let back = KitPreset::load(&written).expect("load");
        assert_eq!(back.pads.len(), 2, "the written kit lost the layout");
        assert_eq!(back.media.len(), 1);
    }

    /// **A sample that cannot be read is counted, not swallowed.**
    ///
    /// `from_kit` degrades to path-only for an unreadable file, which is
    /// right — and used to be completely silent, so the only way to discover
    /// that a kit was not portable was to hand it to somebody. The count is
    /// per DISTINCT FILE: the same missing hat on two pads has lost one
    /// sample, and this asserts that rather than the pad count, because the
    /// tempting implementation counts pads and overstates every failure.
    #[test]
    fn a_kit_counts_the_samples_it_could_not_read() {
        let dir = tmp("unreadable");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![1u8; 40]).expect("kick");

        let mut kit = DrumpadDoc::default();
        kit.pads[36] = PadSlot { path: "media/kick.wav".to_owned(), ..Default::default() };
        // One missing file, on two pads.
        kit.pads[38] = PadSlot { path: "media/gone.wav".to_owned(), ..Default::default() };
        kit.pads[40] = PadSlot { path: "media/gone.wav".to_owned(), ..Default::default() };

        let (preset, report) = KitPreset::from_kit_within("Holey", &kit, &doc, MAX_KIT_BYTES);
        assert_eq!(preset.pads.len(), 3, "a pad was dropped from the layout");
        assert_eq!(report.unreadable, 1, "one missing FILE was counted as {}", report.unreadable);
        assert_eq!(report.embedded, 1);
        assert_eq!(report.over_ceiling, 0);
        assert!(!report.is_complete());
        assert_eq!(preset.pads[1].blob, None);
        assert_eq!(preset.pads[2].blob, None);
    }

    /// **A file too large to be a kit is refused before it is read.**
    ///
    /// The guard has to sit on `metadata`, not on the bytes: by the time
    /// `from_bytes` has a slice the whole file is already in memory and the
    /// hazard has happened. Proved with a tiny ceiling for the reason
    /// `load_within` argues — half a gigabyte of temp file is not a unit
    /// test.
    #[test]
    fn a_file_too_large_to_be_a_kit_is_refused_before_it_is_read() {
        let dir = tmp("toobig");
        let doc = dir.join("project.tangent");
        let media = dir.join("media");
        std::fs::create_dir_all(&media).expect("media");
        std::fs::write(media.join("kick.wav"), vec![3u8; 4096]).expect("kick");
        let mut kit = DrumpadDoc::default();
        kit.pads[36] = PadSlot { path: "media/kick.wav".to_owned(), ..Default::default() };
        let at = KitPreset::from_kit("Big", &kit, &doc)
            .save(&dir.join("Big.tgkit"))
            .expect("write");

        let err = KitPreset::load_within(&at, 1024).expect_err("a huge file was read anyway");
        assert!(
            err.contains("too big to be a kit"),
            "the refusal does not say what happened: {err}"
        );
        // **The same file, under a ceiling it fits.** A real kit, so this
        // also proves the guard is a size test and not a refusal that
        // happens to be worded like one — the very same bytes come back as
        // the kit they are.
        let back = KitPreset::load_within(&at, 1024 * 1024)
            .expect("a kit inside the ceiling was refused");
        assert_eq!(back.pads.len(), 1, "the kit under the ceiling did not load");
        assert_eq!(back.media.len(), 1);
        assert_eq!(back.media[0].bytes.len(), 4096);
    }

    /// **A kit is recognised by extension or by its magic**, which is what
    /// keeps a dropped `.tgkit` off the drop road that copies samples into
    /// `media/`.
    ///
    /// Both answers are asserted separately because they cover different
    /// files: the extension is what the save panel forces, and the sniff is
    /// for the one that came out of an archive with its name mangled. And a
    /// WAV is asserted NOT to be a kit, because a predicate that says yes to
    /// everything would make every dropped sample clear the lane.
    #[test]
    fn a_kit_is_known_by_its_extension_or_its_first_seven_bytes() {
        let dir = tmp("sniff");
        let by_ext = dir.join("Room.tgkit");
        std::fs::write(&by_ext, b"not even a kit").expect("write");
        assert!(looks_like_a_kit(&by_ext), "the extension was not enough");

        let renamed = dir.join("mystery.bin");
        std::fs::write(&renamed, KitPreset::default().to_bytes()).expect("write");
        assert!(looks_like_a_kit(&renamed), "the magic was not sniffed");

        let wav = dir.join("kick.wav");
        std::fs::write(&wav, b"RIFF....WAVEfmt ").expect("write");
        assert!(!looks_like_a_kit(&wav), "a sample was called a kit");

        // A file that is not there is not a kit either, and asking must not
        // panic — the drop road asks about every path the OS hands it.
        assert!(!looks_like_a_kit(&dir.join("nothing at all")));
    }
}

/// The kits' half of the undo fingerprint. See `Fingerprint` in
/// [`crate::arrangement`] for the rule these tests exist to hold: two kits
/// that COMPACT to the same text must fingerprint the same, and two that do
/// not, must not.
#[cfg(test)]
mod fingerprint_tests {
    use super::*;

    fn mark(kits: &[DrumpadDoc]) -> u64 {
        let mut f = Fingerprint::new();
        DrumpadDoc::hash_all_into(kits, &mut f);
        f.finish()
    }

    /// Three lanes: one with two pads in it, one silent but with its fader
    /// moved, one entirely untouched.
    fn some_kits() -> Vec<DrumpadDoc> {
        let mut kits = vec![DrumpadDoc::default(), DrumpadDoc::default(), DrumpadDoc::default()];
        kits[0].pads[0].path = "media/kick.wav".into();
        kits[0].pads[0].name = "the kick".into();
        kits[0].pads[0].params.gain = 0.9;
        kits[0].pads[3].path = "media/snare.wav".into();
        kits[0].pads[3].params.choke = 2;
        kits[0].master = 0.8;
        kits[1].master = 0.5;
        kits
    }

    fn moves_both(name: &str, edit: impl Fn(&mut Vec<DrumpadDoc>)) {
        let kits = some_kits();
        let (text, fp) = (DrumpadDoc::compact_all(&kits), mark(&kits));
        let mut k = kits;
        edit(&mut k);
        assert_ne!(DrumpadDoc::compact_all(&k), text, "`{name}` changed no kit at all");
        assert_ne!(mark(&k), fp, "`{name}` changed a kit and the fingerprint stood still");
    }

    fn moves_neither(name: &str, edit: impl Fn(&mut Vec<DrumpadDoc>)) {
        let kits = some_kits();
        let (text, fp) = (DrumpadDoc::compact_all(&kits), mark(&kits));
        let mut k = kits;
        edit(&mut k);
        assert_eq!(DrumpadDoc::compact_all(&k), text, "`{name}` moved a kit after all");
        assert_eq!(mark(&k), fp, "`{name}` moved no kit and the fingerprint moved");
    }

    /// **Every kit edit the undo timeline can see, the fingerprint sees.**
    #[test]
    fn the_fingerprint_moves_when_a_kit_moves() {
        moves_both("a sample loaded onto an empty pad", |k| {
            k[0].pads[7].path = "media/hat.wav".into();
        });
        moves_both("a pad cleared", |k| k[0].pads[0].path = String::new());
        moves_both("a pad renamed", |k| k[0].pads[0].name = "kick 2".into());
        moves_both("a pad's gain", |k| k[0].pads[0].params.gain = 0.4);
        moves_both("a pad's pan", |k| k[0].pads[0].params.pan = -0.3);
        moves_both("a pad's tuning", |k| k[0].pads[3].params.tune = -2.0);
        moves_both("a choke group", |k| k[0].pads[3].params.choke = 1);
        moves_both("obeying note-offs", |k| k[0].pads[0].params.obey = true);
        moves_both("a trim", |k| k[0].pads[0].params.trim_start = 0.1);
        moves_both("the other trim", |k| k[0].pads[0].params.trim_end = 0.9);
        moves_both("a kit's fader", |k| k[0].master = 0.6);
        moves_both("a fader on a SILENT lane", |k| k[1].master = 0.25);
        moves_both("a whole kit arriving on a new lane", |k| {
            k[2].pads[0].path = "media/clap.wav".into();
        });
        moves_both("a stranger's key on a pad", |k| {
            k[0].pads[0].extra.insert("reverse".into(), Value::from(true));
        });
    }

    /// **And what the compaction throws away cannot move it.**
    ///
    /// `compact_all` writes only the pads that hold something and only the
    /// lanes that hold a pad — the undo timeline compares that form, so a
    /// fingerprint that saw more than it does would push a moment nobody
    /// could ever travel back to.
    #[test]
    fn the_fingerprint_ignores_what_the_compaction_drops() {
        moves_neither("a name on an EMPTY pad", |k| {
            k[0].pads[9].name = "nothing here".into();
        });
        moves_neither("a knob on an empty pad", |k| k[0].pads[9].params.gain = 0.1);
        moves_neither("a stranger's key on an empty pad", |k| {
            k[0].pads[9].extra.insert("reverse".into(), Value::from(true));
        });
        moves_neither("a kit's version stamp", |k| k[0].version = 9);
        moves_neither("a stranger's key on the KIT", |k| {
            k[0].extra.insert("swing".into(), Value::from(0.24));
        });
        moves_neither("a lane that is empty at unity", |k| {
            k[2].pads[4].name = "still nothing".into();
        });
    }

    /// **A kit that has been through the sidecar fingerprints the same.**
    /// Otherwise every project would open with its pads looking edited.
    #[test]
    fn a_round_trip_through_the_compact_form_keeps_the_fingerprint() {
        let kits = some_kits();
        let text = DrumpadDoc::compact_all(&kits);
        let back = DrumpadDoc::from_compact_all(&text, kits.len());
        assert_eq!(
            DrumpadDoc::compact_all(&back),
            text,
            "the compact form is not a fixed point"
        );
        assert_eq!(mark(&back), mark(&kits), "a round trip moved the fingerprint");
    }
}

/// **The sidecar by identity** (W3-persist-v23-v8, design 6.8).
#[cfg(test)]
mod track_kit_tests {
    use super::*;
    use crate::desk_format::Format;
    use crate::desk_id::TrackId;

    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tangent-w3-kits-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch");
        dir.join(crate::arrangement::DOC_NAME)
    }

    fn kit(tag: &str) -> DrumpadDoc {
        let mut k = DrumpadDoc::default();
        k.pads[36].path = format!("media/{tag}.wav");
        k
    }

    /// **A kit follows its TRACK**: written by id and read against a desk
    /// whose rows have moved since, every kit lands on its own track and the
    /// panel's track is still the panel's. No lane key is written.
    #[test]
    fn a_kit_follows_its_track_across_a_reorder() {
        let doc = scratch("follow");
        let ids = [TrackId(0), TrackId(7), TrackId(40), TrackId(3)];
        let kits = vec![kit("a"), DrumpadDoc::default(), kit("c"), kit("d")];
        DrumpadDoc::save_all_beside_as(&kits, &ids, 2, &doc, Format::V2);
        let text = std::fs::read_to_string(DrumpadDoc::sidecar(&doc)).expect("sidecar");
        let v: Value = serde_json::from_str(&text).expect("json");
        assert!(
            v.get("lane_kits").is_none() && v.get("primary_lane").is_none() && v.get("pads").is_none(),
            "a legacy key was written: {text}"
        );
        // The desk moved: t40 is on lane 0 now, t0 on lane 3.
        let moved = [TrackId(40), TrackId(3), TrackId(7), TrackId(0)];
        let (back, primary) = DrumpadDoc::load_all_beside_ids(&doc, &moved, None);
        assert_eq!(back[0], kits[2]);
        assert_eq!(back[1], kits[3]);
        assert!(back[2].is_empty());
        assert_eq!(back[3], kits[0]);
        assert_eq!(primary, Some(0), "the panel's track was not followed");
        let _ = std::fs::remove_dir_all(doc.parent().expect("dir"));
    }

    /// **A file with `track_kits` ignores every legacy key in it**: a stray
    /// top-level kit, `lane_kits` and `primary_lane` can only be a legacy
    /// build's leftovers, and reading them would put a kit on a lane by a
    /// position that no longer means anything.
    #[test]
    fn track_kits_ignore_a_stray_top_level_kit_and_primary_lane() {
        let doc = scratch("stray");
        let mut root = kit("stray").to_value();
        root.insert("primary_lane".into(), Value::from(1u64));
        root.insert("lane_kits".into(), serde_json::json!({ "1": kit("lane").to_value() }));
        root.insert("track_kits".into(), serde_json::json!({ "t5": kit("five").to_value() }));
        root.insert("primary_track".into(), Value::from("t5"));
        std::fs::write(DrumpadDoc::sidecar(&doc), Value::Object(root).to_string()).expect("file");
        let ids = crate::desk_legacy::identity_ids(8);
        let (back, primary) = DrumpadDoc::load_all_beside_ids(&doc, &ids, Some(0));
        assert_eq!(back[5], kit("five"));
        assert_eq!(primary, Some(5));
        for (lane, k) in back.iter().enumerate().filter(|(l, _)| *l != 5) {
            assert!(k.is_empty(), "lane {lane} took a legacy kit");
        }
        let _ = std::fs::remove_dir_all(doc.parent().expect("dir"));
    }

    /// **Without the switch, today's bytes**: the id-keyed writer in the
    /// legacy format is the legacy writer, whatever the ids say.
    #[test]
    fn the_legacy_format_writes_todays_bytes() {
        let doc = scratch("legacy");
        let kits = vec![kit("a"), kit("b")];
        DrumpadDoc::save_all_beside_legacy(&kits, 1, &doc);
        let want = std::fs::read(DrumpadDoc::sidecar(&doc)).expect("legacy");
        DrumpadDoc::save_all_beside_as(&kits, &[TrackId(9), TrackId(4)], 1, &doc, Format::Legacy);
        assert_eq!(std::fs::read(DrumpadDoc::sidecar(&doc)).expect("ids"), want);
        let _ = std::fs::remove_dir_all(doc.parent().expect("dir"));
    }
}
