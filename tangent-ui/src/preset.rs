//! The preset FILE: what a saved unit setting looks like on disk.
//!
//! Pure data, deliberately. This crate never reads or writes one of these
//! (the firewall that keeps it from raising dialogs also keeps it from
//! wandering Documents), so what lives here is the struct, the JSON
//! shape and the filename rules, and the HOST does the disk work in
//! `desktop.rs`. The same bargain [`crate::drumpad::KitPreset`] struck, minus
//! the embedded audio: a preset is a handful of knob positions, so it is
//! plain JSON with the house manners — a `kind` tag so `head` on a mystery
//! file says what it is, a version read max'd up and never down, unknown
//! symbols carried rather than dropped, and rubbish answering an error the
//! host can show rather than a half-applied sound.
//!
//! One format for every unit, keyed by the unit's IDENTITY string:
//! `"@tangent-eq"` names a built-in effect the way the insert bays already
//! do (the sentinel path), and `"keys:2"` names a Keys patch. The values are
//! `(symbol, value)` pairs — symbols are the append-only address space the
//! built-ins persist by, so a file from a newer build carries symbols an
//! older build skips, and a file from an older build leaves the missing
//! knobs at their defaults.

use serde_json::{Map, Value};

use crate::dialogs::{KEYS_PARAMS, KEYS_PARAM_SPECS, KEYS_PATCHES};

/// The extension a saved preset wears. Ours alone — `.preset` and `.json`
/// belong to everybody, and a scan that has to open every file to know
/// whether it is one of ours is a scan that reads junk for a living.
pub const PRESET_EXT: &str = "tangentpreset";

/// The `kind` tag inside the file. A file without it is not a preset,
/// whatever it is named.
pub const KIND: &str = "tangent-preset";

/// This build's format version. Written as the max of this and what a loaded
/// file said — up, never down — so a file that has been through a newer
/// build never loses its claim to be one.
pub const PRESET_VERSION: u64 = 1;

/// One saved preset: which unit it belongs to, what it is called, and where
/// every knob stands.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Preset {
    /// The file's format version, max'd up on read.
    pub version: u64,
    /// The unit's identity — `"@tangent-eq"`, `"keys:2"`. The applying side
    /// compares this against the unit it asked for, so a file moved into
    /// the wrong folder moves nothing.
    pub unit: String,
    /// What the preset is called, as typed — the filename carries the
    /// SANITIZED form, this carries the real one, the same double-entry the
    /// kit format keeps.
    pub name: String,
    /// `(symbol, value)` per knob, in the order the file had them. Symbols
    /// this build does not know are KEPT: they are a newer build's knobs,
    /// and dropping them would make a round trip lossy.
    pub values: Vec<(String, f64)>,
    /// **The unit's STATE that is not a knob**, as `(key, text)` — the
    /// sampler's loaded file, the convolver's impulse.
    ///
    /// A preset that remembers only the knobs gives you back the right
    /// envelope over silence: reopen a sampler patch and every crop, every
    /// stage and every voice count is exactly where you left it, playing
    /// nothing, because the FILE was never written down. That is not a
    /// missing feature, it is a preset that lies about what it restored.
    ///
    /// Text and not numbers because what it holds is a PATH, which is the
    /// same thing `ports::BuiltinFace::extra` carries across the firewall
    /// and the same thing `builtin_fx::is_extra_symbol` gates on the engine
    /// side — one shape for the one kind of state that is not a knob,
    /// rather than a second per unit.
    ///
    /// Append-only by symbol, exactly as [`Self::values`] is: a key this
    /// build does not know is a newer build's state and is carried, not
    /// dropped, so a round trip through an older Tangent is lossless.
    ///
    /// **The sampler's file is written ABSOLUTE.** Its bay stores it
    /// relative to its own project (`media/<name>`), and a preset has to
    /// play in any project; loading one copies the file into the project
    /// that loads it and binds that copy (an audio file only: a preset can
    /// come from anybody). The convolver's impulse is written as its bay
    /// names it, an absolute path into the IR library, and loading the
    /// preset binds it there again; a value shaped like one of the IR road's
    /// asks is never handed on.
    pub state: Vec<(String, String)>,
    /// Top-level keys this build does not know, preserved verbatim — the
    /// same courtesy the settings file and the drumpad sidecar extend.
    pub extra: Map<String, Value>,
}

/// The keys this build writes itself, so [`Preset::from_json`] knows what is
/// somebody else's.
const KNOWN_KEYS: &[&str] = &["version", "kind", "unit", "name", "values", "state"];

impl Preset {
    pub fn to_json(&self) -> String {
        let mut root = Map::new();
        root.insert(
            "version".into(),
            Value::from(self.version.max(PRESET_VERSION)),
        );
        root.insert("kind".into(), Value::from(KIND));
        root.insert("unit".into(), Value::from(self.unit.clone()));
        root.insert("name".into(), Value::from(self.name.clone()));
        let mut values = Map::new();
        for (symbol, value) in &self.values {
            if value.is_finite() {
                values.insert(symbol.clone(), Value::from(*value));
            }
        }
        root.insert("values".into(), Value::Object(values));
        // Written only when there IS state, so every preset written before
        // this existed and every unit that has none looks exactly as it did.
        if !self.state.is_empty() {
            let mut state = Map::new();
            for (key, text) in &self.state {
                state.insert(key.clone(), Value::from(text.clone()));
            }
            root.insert("state".into(), Value::Object(state));
        }
        for (k, v) in &self.extra {
            root.insert(k.clone(), v.clone());
        }
        serde_json::to_string_pretty(&Value::Object(root)).unwrap_or_default()
    }

    /// Read a preset, leniently past the front door: not-JSON or the wrong
    /// `kind` is an `Err` the host can put in front of the user — another
    /// program's file deserves a sentence, not defaults dressed as a restore
    /// — but past that, every missing key is simply absent and every value
    /// that is not a finite number is skipped, so a hand-edited file costs
    /// its broken lines and nothing else.
    pub fn from_json(text: &str) -> Result<Self, String> {
        let Ok(Value::Object(root)) = serde_json::from_str::<Value>(text) else {
            return Err("not a Tangent preset".to_owned());
        };
        if root.get("kind").and_then(Value::as_str) != Some(KIND) {
            return Err("not a Tangent preset".to_owned());
        }
        let mut out = Self {
            version: root
                .get("version")
                .and_then(Value::as_u64)
                .unwrap_or(0)
                .max(PRESET_VERSION),
            unit: root
                .get("unit")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            name: root
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_owned(),
            values: Vec::new(),
            state: Vec::new(),
            extra: Map::new(),
        };
        if let Some(Value::Object(values)) = root.get("values") {
            for (symbol, value) in values {
                if let Some(v) = value.as_f64().filter(|v| v.is_finite()) {
                    out.values.push((symbol.clone(), v));
                }
            }
        }
        if let Some(Value::Object(state)) = root.get("state") {
            for (key, text) in state {
                if let Some(t) = text.as_str() {
                    out.state.push((key.clone(), t.to_owned()));
                }
            }
        }
        for (k, v) in &root {
            if !KNOWN_KEYS.contains(&k.as_str()) {
                out.extra.insert(k.clone(), v.clone());
            }
        }
        Ok(out)
    }
}

/// A preset name as a file STEM that stays inside its folder.
///
/// This answers "what do I write into a folder I also DELETE from", where
/// the name came from a text box, so as well as the separators, the
/// platform-dependent `\` and `:` go (a preset synced to Windows must not
/// change name on the way), and leading dots go (`"../evil"` must not
/// become a dotfile, and `.` has no business starting a filename here).
/// Empty after all that is "preset", never an empty path.
///
/// **Not the whole of what Windows asks of a name.** This said it was
/// stricter than `desktop.rs`'s `sanitize_file_stem`, and was, until that
/// one learnt Windows' refused characters (`< > " | ? *` and the controls),
/// its trailing dots and spaces and its device names. A typed preset name
/// goes through both on its way to a file (`desktop::preset_file_for`), so
/// the Windows rules stay in the one place every file this app names keeps
/// them.
pub fn safe_stem(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if std::path::is_separator(c) || c == '/' || c == '\\' || c == ':' {
                '-'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned
        .trim()
        .trim_start_matches(['.', '-'])
        .trim_end_matches('.')
        .trim();
    if cleaned.is_empty() {
        "preset".to_owned()
    } else {
        cleaned.to_owned()
    }
}

// ── a Keys sound's preset ───────────────────────────────────────────────────
//
// The built-ins name a knob by the symbol their `ParamSpec` table carries. A
// Keys sound has no such table on this side of the wall, only
// `dialogs::KEYS_PARAM_SPECS`, whose names are what the panel prints; what
// the settings file has always stored a Keys knob by is its POSITION, which
// the engine's `keys::Patch::params` keeps append-only. So a Keys preset
// spells positions too, and these are the one place the spelling lives.

/// **A Keys sound's unit identity**: `"keys:<n>"`, `n` its place in
/// [`KEYS_PATCH_NAMES`](crate::dialogs::KEYS_PATCH_NAMES) and in the
/// engine's `Patch::ALL`.
///
/// The place and never the name, for the settings file's reason: the list
/// is appended to and never reordered, so the index is the contract the two
/// tables keep, while a name is a label on the instrument that changes when
/// the instrument is re-voiced. A preset saved under a sound goes on naming
/// that sound across a rename.
pub fn keys_unit(sound: usize) -> String {
    format!("keys:{sound}")
}

/// The sound a unit identity names, when it is one of this build's: only
/// the exact spelling [`keys_unit`] writes, because the host matches a
/// file's unit as a string. `"keys:03"` names no row the host would list,
/// and a sound past the end of the list is a newer build's.
pub fn keys_sound(unit: &str) -> Option<usize> {
    let n = unit.strip_prefix("keys:")?.parse::<usize>().ok()?;
    (n < KEYS_PATCHES && keys_unit(n) == unit).then_some(n)
}

/// **A Keys knob's symbol: `knob<k>`, from the knob's INDEX.**
///
/// The index is what the settings file stores a knob by, and what
/// `keys::Patch::params` promises never to reorder or reuse; the name in
/// `KEYS_PARAM_SPECS` is display copy, free to be reworded. A preset that
/// spelled the name would stop turning that knob the day it was renamed.
/// This one names the same knob in every build that can read the file.
pub fn keys_symbol(knob: usize) -> String {
    format!("knob{knob}")
}

/// One sound's knobs as a preset's values: every one of them, by
/// [`keys_symbol`], in knob order.
pub fn keys_values(knobs: &[f32]) -> Vec<(String, f64)> {
    knobs
        .iter()
        .enumerate()
        .map(|(knob, v)| (keys_symbol(knob), f64::from(*v)))
        .collect()
}

/// **A Keys preset's values as one sound's knobs.**
///
/// A preset is a whole sound, not an edit to whatever the knobs held, so a
/// knob the file does not name (a file from an older build, or one edited by
/// hand) comes back at the sound's own default, and so does one whose value
/// is not a finite number: a NaN never reaches the engine. Every other value
/// is clamped into the 0..=1 every Keys knob travels in. A symbol this build
/// has no knob for turns nothing; it is a newer build's to read, and the
/// file keeps it ([`Preset::values`]).
pub fn keys_knobs(sound: usize, values: &[(String, f64)]) -> [f32; KEYS_PARAMS] {
    let specs = KEYS_PARAM_SPECS.get(sound);
    std::array::from_fn(|knob| {
        let default = specs.map_or(0.5, |s| s[knob].2);
        let symbol = keys_symbol(knob);
        values
            .iter()
            .find(|(s, _)| *s == symbol)
            .map(|(_, v)| *v)
            .filter(|v| v.is_finite())
            .map_or(default, |v| v.clamp(0.0, 1.0) as f32)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_preset_round_trips_with_every_knob_where_it_was() {
        let p = Preset {
            version: PRESET_VERSION,
            unit: "@tangent-eq".to_owned(),
            name: "Warm Bus".to_owned(),
            state: Vec::new(),
            values: vec![
                ("low_gain".to_owned(), 0.62),
                ("high_gain".to_owned(), 0.41),
            ],
            extra: Map::new(),
        };
        let back = Preset::from_json(&p.to_json()).expect("its own JSON reads back");
        assert_eq!(back, p);
    }

    #[test]
    fn unknown_symbols_and_unknown_keys_survive_a_round_trip() {
        // A newer build's knob and a newer build's top-level key, both of
        // which this build must carry rather than eat.
        let text = r#"{
            "version": 3,
            "kind": "tangent-preset",
            "unit": "keys:2",
            "name": "Glassy",
            "values": {"bell": 0.9, "from_the_future": 0.25},
            "author": "somebody"
        }"#;
        let p = Preset::from_json(text).unwrap();
        assert_eq!(p.version, 3, "the version reads max'd up, never down");
        assert_eq!(p.unit, "keys:2");
        assert!(p.values.iter().any(|(s, v)| s == "from_the_future" && *v == 0.25));
        assert_eq!(p.extra.get("author"), Some(&Value::from("somebody")));
        let again = Preset::from_json(&p.to_json()).unwrap();
        assert_eq!(again, p);
    }

    #[test]
    fn a_missing_or_broken_value_costs_its_own_line_and_nothing_else() {
        let text = r#"{
            "kind": "tangent-preset",
            "unit": "@tangent-comp",
            "values": {"ratio": 0.5, "attack": "fast", "release": null}
        }"#;
        let p = Preset::from_json(text).unwrap();
        assert_eq!(p.values, vec![("ratio".to_owned(), 0.5)]);
        assert_eq!(p.name, "", "an unnamed preset is legal; the stem covers it");
        assert_eq!(p.version, PRESET_VERSION, "an unversioned file reads as ours");
    }

    #[test]
    fn what_is_not_a_preset_is_refused_out_loud() {
        for junk in [
            "",
            "not json at all",
            "{}",
            r#"{"kind": "tangent-kit", "values": {}}"#,
            "[1, 2, 3]",
        ] {
            assert!(Preset::from_json(junk).is_err(), "{junk:?} was accepted");
        }
    }

    #[test]
    fn a_hostile_name_becomes_a_filename_that_stays_in_its_folder() {
        // The choice under test: separators and drive colons become dashes,
        // and the leading dots that make `..` a traversal are stripped, so
        // the joined path can only ever land INSIDE the preset folder.
        let evil = safe_stem("../evil");
        assert_eq!(evil, "evil");
        for name in ["../../etc/passwd", "..\\..\\evil", "C:\\boot", "a/b/c", ". ."] {
            let stem = safe_stem(name);
            assert!(!stem.contains('/') && !stem.contains('\\') && !stem.contains(':'));
            assert!(!stem.starts_with('.'), "{name:?} kept a leading dot: {stem:?}");
            assert!(!stem.is_empty());
        }
        // And an ordinary name is left alone.
        assert_eq!(safe_stem("Warm Bus 2"), "Warm Bus 2");
        assert_eq!(safe_stem(""), "preset");
    }

    /// **A preset remembers the SAMPLE, not only the knobs.**
    ///
    /// Without this a sampler patch reopens with every crop, every envelope
    /// stage and every voice count exactly where it was left, playing
    /// nothing — the right envelope over silence, which is not a missing
    /// feature but a preset that lies about what it restored.
    ///
    /// The state round-trips as text and is APPEND-ONLY by key: a key this
    /// build does not know is a newer build's state and is carried back out
    /// rather than dropped, so a patch that has been through an older
    /// Tangent still names its file.
    #[test]
    fn a_preset_remembers_the_sample_and_not_only_the_knobs() {
        let p = Preset {
            version: PRESET_VERSION,
            unit: "@tangent-sampler".to_owned(),
            name: "Room Kick".to_owned(),
            values: vec![("start".to_owned(), 0.125), ("attack".to_owned(), 0.0)],
            state: vec![
                ("sample".to_owned(), "/m/Takes/kick 03.wav".to_owned()),
                // A key from a build that does not exist yet.
                ("from_tomorrow".to_owned(), "whatever it means".to_owned()),
            ],
            extra: Map::new(),
        };
        let back = Preset::from_json(&p.to_json()).expect("a preset it just wrote");
        assert_eq!(back.state, p.state, "the sample did not survive the file");
        assert_eq!(back.values, p.values);
        assert_eq!(back.unit, p.unit);
    }

    /// **A preset with no state writes no `state` key**, so every file
    /// written before this existed, and every unit that has none, looks
    /// exactly as it did — and reads back the same.
    #[test]
    fn a_preset_with_nothing_to_remember_is_the_file_it_always_was() {
        let p = Preset {
            version: PRESET_VERSION,
            unit: "@tangent-eq".to_owned(),
            name: "Warm".to_owned(),
            values: vec![("low_gain".to_owned(), 0.5)],
            state: Vec::new(),
            extra: Map::new(),
        };
        let text = p.to_json();
        assert!(!text.contains("state"), "an empty state took a key:\n{text}");
        assert!(Preset::from_json(&text).expect("round trip").state.is_empty());
        // And a file written by an OLDER build simply has no state.
        let old = r#"{"kind":"tangent-preset","version":1,"unit":"@tangent-sampler",
                      "name":"Old","values":{"vol":0.5}}"#;
        let back = Preset::from_json(old).expect("an older file is still a preset");
        assert!(back.state.is_empty());
        assert_eq!(back.values, vec![("vol".to_owned(), 0.5)]);
    }

    /// **A Keys sound is named by its PLACE, and only by the spelling the
    /// host compares.** Every sound round-trips through its identity; a
    /// sound past the end of the list, a padded number, another unit and
    /// the bare drawer name are no sound of this build's.
    #[test]
    fn a_keys_sound_is_named_by_its_place_in_the_list() {
        for sound in 0..KEYS_PATCHES {
            assert_eq!(keys_sound(&keys_unit(sound)), Some(sound), "sound {sound}");
        }
        assert_eq!(keys_unit(3), "keys:3", "the identity the module doc and the host's drawer read");
        let past = format!("keys:{KEYS_PATCHES}");
        for other in [past.as_str(), "keys:03", "keys:+3", "keys:-1", "keys:", "keys", "@tangent-eq", ""] {
            assert_eq!(keys_sound(other), None, "{other:?} was read as a sound");
        }
    }

    /// **A Keys preset names its knobs by index and gives them back whole.**
    ///
    /// Saved, every knob is a `knobN` and none is missing; read back, they
    /// are the knobs it was saved from. What a hand-edited or a newer file
    /// can hold costs nothing: a knob it does not name comes back at the
    /// SOUND's own default (not the last sound's, not 0.5), a value that is
    /// not a finite number does too, an out-of-range one is clamped, and a
    /// symbol no knob has, the display name of a knob included, turns
    /// nothing.
    #[test]
    fn a_keys_preset_names_its_knobs_by_index_and_lands_whole() {
        let knobs = [0.0_f32, 0.125, 0.25, 0.5, 0.625, 0.75, 0.875, 1.0];
        let values = keys_values(&knobs);
        let symbols: Vec<&str> = values.iter().map(|(s, _)| s.as_str()).collect();
        assert_eq!(symbols, ["knob0", "knob1", "knob2", "knob3", "knob4", "knob5", "knob6", "knob7"]);
        for sound in 0..KEYS_PATCHES {
            assert_eq!(keys_knobs(sound, &values), knobs, "sound {sound} did not read back what it saved");
        }

        // The clav's Mute rests at 0 and its Tone at 0.75: a default that is
        // the sound's own is the only way a missing knob can come back right.
        let clav = 4;
        let defaults: Vec<f32> = KEYS_PARAM_SPECS[clav].iter().map(|s| s.2).collect();
        assert_eq!(keys_knobs(clav, &[]).to_vec(), defaults, "an empty preset is not the sound's defaults");
        let rubbish = vec![
            ("knob0".to_owned(), f64::NAN),
            ("knob1".to_owned(), f64::INFINITY),
            ("knob2".to_owned(), 7.0),
            ("knob3".to_owned(), -2.0),
            ("knob9".to_owned(), 0.3),
            ("Voicing".to_owned(), 0.9),
            ("from_the_future".to_owned(), 0.1),
        ];
        let landed = keys_knobs(clav, &rubbish);
        assert_eq!(landed[0], defaults[0], "a NaN landed");
        assert_eq!(landed[1], defaults[1], "an infinity landed");
        assert_eq!(landed[2], 1.0, "a value past the top was not clamped");
        assert_eq!(landed[3], 0.0, "a value under the bottom was not clamped");
        assert_eq!(landed[4..].to_vec(), defaults[4..].to_vec(), "a symbol no knob has turned one");
        assert!(landed.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
    }

    /// **A Keys preset file carries what this build cannot read.** A newer
    /// build's ninth knob and a key of its own survive a read and a write,
    /// and the knobs this build knows still come off the file.
    #[test]
    fn a_keys_preset_from_a_newer_build_keeps_its_extra_knob() {
        let text = r#"{
            "version": 2,
            "kind": "tangent-preset",
            "unit": "keys:1",
            "name": "Bell Tree",
            "values": {"knob0": 0.25, "knob3": 0.75, "knob8": 0.5},
            "voiced_by": "somebody"
        }"#;
        let p = Preset::from_json(text).expect("a newer build's Keys preset");
        assert_eq!(keys_sound(&p.unit), Some(1));
        let again = Preset::from_json(&p.to_json()).expect("its own JSON");
        assert_eq!(again, p, "the round trip lost something");
        assert!(again.values.iter().any(|(s, v)| s == "knob8" && *v == 0.5), "the ninth knob was dropped");
        let knobs = keys_knobs(1, &again.values);
        assert_eq!((knobs[0], knobs[3]), (0.25, 0.75));
        assert_eq!(knobs[1], KEYS_PARAM_SPECS[1][1].2, "a knob the file does not name is not at its default");
    }
}
