//! Themes: a named set of colours that paints the whole app.
//!
//! **A theme is a set of values for the colour fields `Settings` already
//! carries, not a parallel colour system.** Every panel in this app already
//! builds its `Palette` from `Settings`, so applying a theme is one write into
//! settings and every surface follows on the next frame. The alternative — a
//! Theme object threaded through twelve panels — would have meant two places
//! a colour could live and two places to forget.
//!
//! **Every role is optional.** `None` means "leave the shipped default", which
//! is what makes a three-line user theme legal: set the two colours you care
//! about, inherit the rest. It is also what `Default` is — a theme that sets
//! nothing at all.
//!
//! **Factory themes are code; user themes are files.** The files live beside
//! `settings.json` in a folder called `user themes`, so "Reset Settings to
//! Default" — which rewrites settings.json — cannot delete somebody's work. A
//! reset unsets the CHOICE and the colours go back to default; the theme file
//! is still there to pick again.

use crate::settings::{Rgb, Settings};
use std::path::PathBuf;

/// Where user themes live: beside the settings file, in a folder named for
/// what a person would call it.
pub fn user_dir() -> PathBuf {
    Settings::path()
        .parent()
        .unwrap_or_else(|| std::path::Path::new("."))
        .join("user themes")
}

/// One theme. Every field is a role a panel asks for by name.
///
/// The roles are the owner's list, in the order a person meets them: the
/// keyboard, the words and lines over it, the recorder band, the desk, and
/// the fretboard.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Theme {
    pub name: String,

    // ── the keyboard ───────────────────────────────────────────────────────
    pub white_key: Option<Rgb>,
    pub black_key: Option<Rgb>,
    /// The colour a held key wears. One value drives both white and black
    /// active keys — two was a distinction nobody ever set differently.
    pub key_active: Option<Rgb>,
    /// Held-by-the-pedal. Reaches EVERY theory module, not just the piano.
    pub sustain: Option<Rgb>,
    /// The cuts between keys. Its own role rather than a shade of the keys:
    /// the shipped walnut is a hue neither key colour contains, and deriving
    /// it would have quietly repainted the default keyboard.
    pub key_separator: Option<Rgb>,

    // ── the ground everything else sits on ─────────────────────────────────
    /// **The app's own background** — behind the keyboard, and the field every
    /// theory module is drawn on.
    ///
    /// It was two fixed values, one per mode: near-black dark, ivory light,
    /// the same on all sixteen themes. The owner's words were that no theme
    /// changes the theory view's background, and they were right — it was the
    /// one surface with no role at all.
    ///
    /// **It is the KEYBOARD's colour, not the band's.** The first pass took it
    /// from `recorder_bg`, which on the default theme is the band's wine — so
    /// the theory modules came up on pink. A page of music belongs on the same
    /// bone the naturals are, with the same black on it, and in dark mode on
    /// the same near-black the sharps are. Default is `#e8dcc0` over
    /// `#1a1a1a`, which is exactly what this view always looked like.
    pub background: Option<Rgb>,

    // ── words and lines ────────────────────────────────────────────────────
    pub text: Option<Rgb>,
    /// Every line the theory modules draw: staff rules, grid, outlines.
    pub theory_line: Option<Rgb>,
    pub interval_text: Option<Rgb>,

    // ── the recorder band ──────────────────────────────────────────────────
    pub recorder_bg: Option<Rgb>,
    /// The six effect knobs under the VU strip.
    pub fx_knob: Option<Rgb>,
    /// The master, which is deliberately its own colour: it is the one
    /// control that can undo every other one.
    pub master_knob: Option<Rgb>,
    /// The varispeed dial's face. Its own role and not a shade of `fx_knob`,
    /// because it is not one of the effects: it is the one control that
    /// changes the PITCH of everything, and a theme should be able to say so.
    pub varispeed: Option<Rgb>,

    // ── the velocity gradient ──────────────────────────────────────────────
    /// **The loudest end of the MIDI editor's velocity lane**, and its
    /// quietest. A pair rather than one colour, because a gradient needs two
    /// ends and deriving the second from the first is how you get a ramp that
    /// is a shade of one hue instead of a reading.
    ///
    /// Hot for loud and cool for quiet in every theme — the meaning is fixed
    /// for the same reason the record light is one red everywhere — and WHICH
    /// hot and WHICH cool is the theme's to say. Designed by
    /// `scripts/velgen.py` against each theme's own roll, in both modes.
    pub velocity_loud: Option<Rgb>,
    pub velocity_quiet: Option<Rgb>,
    /// The well the timecode and the oscilloscope sit in.
    pub meter_bg: Option<Rgb>,
    /// The oscilloscope's trace — the green, on a stock install.
    pub scope_draw: Option<Rgb>,

    // ── the desk ───────────────────────────────────────────────────────────
    /// The insert-bay strips, in the mixer and the arrangement alike.
    pub fx_strip: Option<Rgb>,
    /// The input chip under a channel.
    pub input_chip: Option<Rgb>,
    /// Which auto-colour palette the channels wear: an index into
    /// `settings::PALETTES`, cycled rather than clamped, so a theme written
    /// against a build with more palettes still lands somewhere sensible.
    pub channel_palette: Option<i64>,

    // ── the fretboard ──────────────────────────────────────────────────────
    pub fretboard: Option<Rgb>,
    pub string: Option<Rgb>,

    /// **The same roles again, for DARK mode — and anything else it wants.**
    ///
    /// Keyed by role name, so a theme file writes `"dark": {"fx_strip": ...}`
    /// and every role above is available inside it. Unset roles are DERIVED,
    /// which is what dark mode has always done: the piano swaps its keys, the
    /// mixer darkens its face, the arrangement re-derives its field. A theme
    /// that says nothing here behaves exactly as it did.
    ///
    /// Named, it wins. Somebody who cares enough to write a theme should be
    /// able to say what every surface is in both modes rather than inherit a
    /// transform designed for the colours this app happened to ship with.
    pub dark: std::collections::BTreeMap<String, Rgb>,
}

/// Every role a theme can name, in the spelling a theme FILE uses. One list,
/// so the writer, the reader, the example and the dark map cannot drift.
pub const ROLES: [&str; 21] = [
    "background",
    "white_key",
    "black_key",
    "key_active",
    "sustain",
    "key_separator",
    "text",
    "theory_line",
    "interval_text",
    "recorder_bg",
    "fx_knob",
    "master_knob",
    "varispeed",
    "meter_bg",
    "scope_draw",
    "fx_strip",
    "input_chip",
    "fretboard",
    "string",
    // Appended, and appended is the rule: `ROLES` is walked by the tests and
    // written into every example file, so inserting in the middle rewrites a
    // document somebody may be reading beside their own.
    "velocity_loud",
    "velocity_quiet",
];

/// A `const`-friendly colour literal, because `Rgb::new` is not const and a
/// factory table of ninety colours should not need ninety function calls.
const fn c(r: u8, g: u8, b: u8) -> Option<Rgb> {
    Some(Rgb { r, g, b })
}

/// The dark-mode set for a factory theme, written as pairs.
///
/// A `BTreeMap` cannot be a const, and a factory theme should not need ten
/// lines of insertion to name two colours.
fn dark_of(pairs: &[(&str, (u8, u8, u8))]) -> std::collections::BTreeMap<String, Rgb> {
    pairs
        .iter()
        .map(|(role, (r, g, b))| {
            (
                (*role).to_owned(),
                Rgb { r: *r, g: *g, b: *b },
            )
        })
        .collect()
}

impl Theme {
    /// The theme a fresh install wears — every role SET, to the exact value
    /// the app ships with.
    ///
    /// **Explicit, and that is the whole point.** It began as a theme that
    /// set nothing, on the reasoning that unset means "keep the default".
    /// That is true of a fresh install and false the moment somebody tries
    /// another theme: applying one writes its colours into the settings, and
    /// a Default that sets nothing then leaves them exactly where they are.
    /// The owner met this as "resetting gives the real default, but picking
    /// Default in the menu gives a different one" - and they were right.
    /// Picking Default has to be the way BACK, so Default names every value.
    pub fn factory_default() -> Self {
        Self {
            name: "Default".to_owned(),
            background: c(0xe8, 0xdc, 0xc0),
            white_key: c(0xE8, 0xDC, 0xC0),
            black_key: c(0x1a, 0x1a, 0x1a),
            key_active: c(0x6C, 0x9B, 0xD2),
            sustain: c(0xD2, 0xA3, 0x6C),
            key_separator: c(0x5c, 0x3f, 0x1f),
            text: c(0xE8, 0xDC, 0xC0),
            theory_line: c(0x9c, 0x8f, 0x74),
            interval_text: c(0xE8, 0xDC, 0xC0),
            recorder_bg: c(0x61, 0x37, 0x3a),
            fx_knob: c(0x6C, 0x9B, 0xD2),
            master_knob: c(0xE8, 0x3A, 0x4E),
            varispeed: c(0xE8, 0xDC, 0xC0),
            meter_bg: c(0x0B, 0x0E, 0x0A),
            scope_draw: c(0xA3, 0xE0, 0x48),
            fx_strip: c(0xE3, 0xD9, 0xBD),
            input_chip: c(0xE3, 0xD9, 0xBD),
            fretboard: c(0x4a, 0x2b, 0x22),
            string: c(0xC9, 0xC4, 0xBC),
            velocity_loud: c(0xff, 0x8d, 0x89),
            velocity_quiet: c(0x40, 0xbd, 0x51),
            channel_palette: Some(0),
            dark: dark_of(&[("background", (0x1a, 0x1a, 0x1a))]),
        }
    }

    /// Read a theme out of the settings it would produce. This is "Save Theme
    /// As...": whatever the app is wearing right now becomes a file.
    pub fn capture(name: &str, s: &Settings) -> Self {
        Self {
            name: name.to_owned(),
            background: Some(s.background_color),
            white_key: Some(s.white_key_idle_color),
            black_key: Some(s.black_key_idle_color),
            key_active: Some(s.white_key_active_color),
            sustain: Some(s.sustain_color),
            key_separator: Some(s.key_separator_color),
            text: Some(s.chord_text_color),
            theory_line: Some(s.theory_line_color),
            interval_text: Some(s.interval_text_color),
            recorder_bg: Some(s.recorder_bg_color),
            fx_knob: Some(s.fx_knob_color),
            master_knob: Some(s.master_knob_color),
            varispeed: Some(s.varispeed_color),
            velocity_loud: Some(s.velocity_loud_color),
            velocity_quiet: Some(s.velocity_quiet_color),
            meter_bg: Some(s.meter_bg_color),
            scope_draw: Some(s.scope_draw_color),
            fx_strip: Some(s.fx_strip_color),
            input_chip: Some(s.input_chip_color),
            channel_palette: Some(s.palette_choice),
            fretboard: Some(s.fretboard_color),
            string: Some(s.string_color),
            dark: s.dark_roles.clone(),
        }
    }

    /// Paint the settings with this theme. Unset roles are left exactly as
    /// they are — which is what makes a partial user theme work, and what
    /// makes `Default` a no-op over freshly-reset settings.
    ///
    /// Applying is deliberately NOT recorded per-role: the theme name is the
    /// record. Someone who then hand-edits one colour has a modified theme,
    /// and `capture` is how they keep it.
    pub fn apply(&self, s: &mut Settings) {
        // The dark overrides replace whatever was there wholesale: a theme
        // owns both modes, and half of the last theme's dark set surviving
        // under this one's light set is the "Default is not the default" bug
        // in a new costume.
        s.dark_roles = self.dark.clone();
        if let Some(v) = self.background {
            s.background_color = v;
        }
        if let Some(v) = self.white_key {
            s.white_key_idle_color = v;
        }
        if let Some(v) = self.black_key {
            s.black_key_idle_color = v;
        }
        if let Some(v) = self.key_active {
            // Both, for the reason `key_active` gives.
            s.white_key_active_color = v;
            s.black_key_active_color = v;
        }
        if let Some(v) = self.sustain {
            s.sustain_color = v;
        }
        if let Some(v) = self.key_separator {
            s.key_separator_color = v;
        }
        if let Some(v) = self.text {
            s.chord_text_color = v;
        }
        if let Some(v) = self.theory_line {
            s.theory_line_color = v;
        }
        if let Some(v) = self.interval_text {
            s.interval_text_color = v;
        }
        if let Some(v) = self.recorder_bg {
            s.recorder_bg_color = v;
        }
        if let Some(v) = self.fx_knob {
            s.fx_knob_color = v;
        }
        if let Some(v) = self.master_knob {
            s.master_knob_color = v;
        }
        if let Some(v) = self.varispeed {
            s.varispeed_color = v;
        }
        if let Some(v) = self.velocity_loud {
            s.velocity_loud_color = v;
        }
        if let Some(v) = self.velocity_quiet {
            s.velocity_quiet_color = v;
        }
        if let Some(v) = self.meter_bg {
            s.meter_bg_color = v;
        }
        if let Some(v) = self.scope_draw {
            s.scope_draw_color = v;
        }
        if let Some(v) = self.fx_strip {
            s.fx_strip_color = v;
        }
        if let Some(v) = self.input_chip {
            s.input_chip_color = v;
        }
        // **Only when the palette is following the theme.** Somebody who has
        // picked one by hand in the track menu has said what they want, and
        // the next theme must not take it back.
        if let Some(v) = self.channel_palette {
            if s.palette_auto {
                s.palette_choice = v;
            }
        }
        if let Some(v) = self.fretboard {
            s.fretboard_color = v;
        }
        if let Some(v) = self.string {
            s.string_color = v;
        }
        s.theme_name = self.name.clone();
    }

    /// One role by the name a theme FILE uses.
    ///
    /// **So that a caller can walk [`ROLES`] instead of keeping a second list
    /// beside it.** There was such a second list — a hard-coded seventeen
    /// inside `every_non_default_factory_theme_fills_every_role` — and it had
    /// already fallen behind: `varispeed` went into `ROLES`, into all sixteen
    /// factory themes and into the reader, and the test that exists to catch a
    /// half-filled theme was not checking it. A role added to `ROLES` and
    /// forgotten here is a compile error; a role forgotten in a `match` arm is
    /// not, so this is exhaustive on purpose and takes no `_` arm.
    pub fn role(&self, name: &str) -> Option<Rgb> {
        match name {
            "background" => self.background,
            "white_key" => self.white_key,
            "black_key" => self.black_key,
            "key_active" => self.key_active,
            "sustain" => self.sustain,
            "key_separator" => self.key_separator,
            "text" => self.text,
            "theory_line" => self.theory_line,
            "interval_text" => self.interval_text,
            "recorder_bg" => self.recorder_bg,
            "fx_knob" => self.fx_knob,
            "master_knob" => self.master_knob,
            "varispeed" => self.varispeed,
            "meter_bg" => self.meter_bg,
            "scope_draw" => self.scope_draw,
            "fx_strip" => self.fx_strip,
            "input_chip" => self.input_chip,
            "fretboard" => self.fretboard,
            "string" => self.string,
            "velocity_loud" => self.velocity_loud,
            "velocity_quiet" => self.velocity_quiet,
            _ => None,
        }
    }

    /// Serialise to the JSON a user theme file holds. Unset roles are simply
    /// absent, so a captured theme and a hand-written two-line one round-trip
    /// through the same reader.
    pub fn to_json(&self) -> String {
        use serde_json::{Map, Value};
        let mut m = Map::new();
        m.insert("name".into(), Value::String(self.name.clone()));
        let mut put = |k: &str, v: Option<Rgb>| {
            if let Some(v) = v {
                m.insert(k.to_owned(), Value::String(v.to_hex()));
            }
        };
        put("white_key", self.white_key);
        put("black_key", self.black_key);
        put("key_active", self.key_active);
        put("sustain", self.sustain);
        put("key_separator", self.key_separator);
        put("text", self.text);
        put("theory_line", self.theory_line);
        put("interval_text", self.interval_text);
        put("recorder_bg", self.recorder_bg);
        put("fx_knob", self.fx_knob);
        put("master_knob", self.master_knob);
        put("varispeed", self.varispeed);
        put("velocity_loud", self.velocity_loud);
        put("velocity_quiet", self.velocity_quiet);
        put("meter_bg", self.meter_bg);
        put("scope_draw", self.scope_draw);
        put("fx_strip", self.fx_strip);
        put("background", self.background);
        put("input_chip", self.input_chip);
        put("fretboard", self.fretboard);
        put("string", self.string);
        if let Some(v) = self.channel_palette {
            m.insert("channel_palette".into(), Value::from(v));
        }
        // The dark set, only when there is one: a theme that leaves dark mode
        // to the derivation should not grow an empty object in its file.
        if !self.dark.is_empty() {
            let mut d = Map::new();
            for (role, c) in &self.dark {
                d.insert(role.clone(), Value::String(c.to_hex()));
            }
            m.insert("dark".into(), Value::Object(d));
        }
        serde_json::to_string_pretty(&Value::Object(m)).unwrap_or_default()
    }

    /// Read a theme file. Unknown keys are ignored and unreadable colours are
    /// simply unset, so a typo costs one role rather than the whole file —
    /// the same forgiveness `Settings::from_map` gives.
    pub fn from_json(text: &str, fallback_name: &str) -> Option<Self> {
        let serde_json::Value::Object(map) = serde_json::from_str(text).ok()? else {
            return None;
        };
        let get = |k: &str| -> Option<Rgb> {
            map.get(k).and_then(serde_json::Value::as_str).and_then(Rgb::parse)
        };
        Some(Self {
            name: map
                .get("name")
                .and_then(serde_json::Value::as_str)
                .filter(|n| !n.trim().is_empty())
                .unwrap_or(fallback_name)
                .to_owned(),
            white_key: get("white_key"),
            black_key: get("black_key"),
            key_active: get("key_active"),
            sustain: get("sustain"),
            key_separator: get("key_separator"),
            text: get("text"),
            theory_line: get("theory_line"),
            interval_text: get("interval_text"),
            recorder_bg: get("recorder_bg"),
            fx_knob: get("fx_knob"),
            master_knob: get("master_knob"),
            varispeed: get("varispeed"),
            velocity_loud: get("velocity_loud"),
            velocity_quiet: get("velocity_quiet"),
            meter_bg: get("meter_bg"),
            background: get("background"),
            scope_draw: get("scope_draw"),
            fx_strip: get("fx_strip"),
            input_chip: get("input_chip"),
            channel_palette: map.get("channel_palette").and_then(serde_json::Value::as_i64),
            // **Anything the file names for dark mode.** Unknown role names
            // are kept rather than dropped: a theme written against a newer
            // build should not lose a role by passing through an older one,
            // and a role this build does not read costs nothing to carry.
            dark: map
                .get("dark")
                .and_then(serde_json::Value::as_object)
                .map(|d| {
                    d.iter()
                        .filter_map(|(k, v)| {
                            v.as_str().and_then(Rgb::parse).map(|c| (k.clone(), c))
                        })
                        .collect()
                })
                .unwrap_or_default(),
            fretboard: get("fretboard"),
            string: get("string"),
        })
    }

    /// Write this theme into the user folder. The file name is the theme's
    /// name with anything path-shaped removed — a theme called `../../evil`
    /// writes `evil.json` in the themes folder and nowhere else.
    pub fn save(&self) -> std::io::Result<PathBuf> {
        let dir = user_dir();
        std::fs::create_dir_all(&dir)?;
        let stem: String = self
            .name
            .chars()
            .map(|ch| if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' { ch } else { '_' })
            .collect();
        let stem = stem.trim().to_owned();
        let stem = if stem.is_empty() { "theme".to_owned() } else { stem };
        let path = dir.join(format!("{stem}.json"));
        std::fs::write(&path, self.to_json())?;
        Ok(path)
    }
}

/// Every theme that ships in the binary, Default first.
///
/// **Sixteen, and they are a spread rather than variations on one idea.**
/// One light-ivory default, one warm-paper light theme, and six darks that do
/// not repeat a hue: plum, teal, navy, grey, warm-console, indigo. Three of
/// them are the owner's own applications' palettes, kept faithful to what
/// those apps look like rather than approximated.
pub fn factory() -> Vec<Theme> {
    vec![
        Theme::factory_default(),
        // ── Valentine: the owner's Focusrite control panel. Dark plum with a
        // rose highlight and one coral spark for anything engaged; meters ramp
        // jade → amber → crimson, so the scope is jade and the master crimson.
        Theme {
            name: "Valentine".to_owned(),
            background: c(0xf3, 0xdc, 0xe7),
            white_key: c(0xf3, 0xdc, 0xe7),
            black_key: c(0x25, 0x15, 0x24),
            key_active: c(0xff, 0x5f, 0xa2),
            // Amber, not the coral it started as: rose and coral differ in
            // HUE but barely in brightness (1.22:1), so a held note and a
            // struck one looked alike at a glance and identical to anyone
            // who does not separate red from orange. Amber is 1.6:1 clear of
            // the rose and is Valentine's own warning colour.
            sustain: c(0xff, 0xb4, 0x54),
            key_separator: c(0x5e, 0x2c, 0x4e),
            text: c(0xf3, 0xdc, 0xe7),
            theory_line: c(0xa3, 0x76, 0x8f),
            interval_text: c(0x4c, 0xc7, 0xa4),
            recorder_bg: c(0x52, 0x30, 0x4a),
            fx_knob: c(0xff, 0x8a, 0x5b),
            master_knob: c(0xff, 0x3b, 0x5c),
            varispeed: c(0xF3, 0xDC, 0xE7),
            meter_bg: c(0x25, 0x15, 0x24),
            scope_draw: c(0x4c, 0xc7, 0xa4),
            fx_strip: c(0x5e, 0x2c, 0x4e),
            input_chip: c(0x4c, 0xc7, 0xa4),
            fretboard: c(0x25, 0x15, 0x24),
            string: c(0xa3, 0x76, 0x8f),
                        velocity_loud: c(0xff, 0x70, 0x83),
            velocity_quiet: c(0x19, 0xb3, 0x84),
            channel_palette: Some(1),
            dark: dark_of(&[("background", (0x25, 0x15, 0x24))]),
        },
        // ── Hydra: the owner's audio-routing TUI. Near-black teal depths, a
        // bioluminescent seafoam glow, and exactly one warm coral — which is
        // the sustain, because sustain is the held/engaged state.
        Theme {
            name: "Hydra".to_owned(),
            background: c(0xd6, 0xe8, 0xe4),
            white_key: c(0xd6, 0xe8, 0xe4),
            black_key: c(0x10, 0x24, 0x28),
            key_active: c(0x3f, 0xd9, 0xb8),
            sustain: c(0xe8, 0x84, 0x5c),
            key_separator: c(0x1c, 0x38, 0x3c),
            text: c(0xd6, 0xe8, 0xe4),
            theory_line: c(0x6a, 0x8c, 0x88),
            interval_text: c(0xe0, 0xb0, 0x50),
            recorder_bg: c(0x1d, 0x4a, 0x4e),
            fx_knob: c(0x3f, 0xd9, 0xb8),
            master_knob: c(0xf0, 0x5c, 0x78),
            varispeed: c(0xD6, 0xE8, 0xE4),
            meter_bg: c(0x10, 0x24, 0x28),
            scope_draw: c(0x4f, 0xd4, 0x9a),
            fx_strip: c(0x1c, 0x38, 0x3c),
            input_chip: c(0x4f, 0xd4, 0x9a),
            fretboard: c(0x10, 0x24, 0x28),
            string: c(0x6a, 0x8c, 0x88),
                        velocity_loud: c(0xfe, 0x88, 0x56),
            velocity_quiet: c(0x1b, 0xbf, 0x82),
            channel_palette: Some(2),
            dark: dark_of(&[("background", (0x10, 0x24, 0x28))]),
        },
        // ── Navi: the owner's graph viewer. Layered night blues with
        // periwinkle nodes rimmed in electric cyan, and exactly one warm
        // colour — amber — which that app reserves for SELECTION, so it is
        // the master here: the one control that is about intent.
        Theme {
            name: "Navi".to_owned(),
            background: c(0xe0, 0xf0, 0xff),
            white_key: c(0xe0, 0xf0, 0xff),
            black_key: c(0x0a, 0x1c, 0x34),
            // Lifted from the app's own #5578ff: that periwinkle is drawn on
            // Navi's near-black canvas, and Tangent's band is a mid navy - the
            // same colour that glows there is 2.7:1 here.
            key_active: c(0x7f, 0x9b, 0xff),
            sustain: c(0x40, 0xe8, 0xff),
            key_separator: c(0x26, 0x50, 0x80),
            text: c(0xe0, 0xf0, 0xff),
            theory_line: c(0x26, 0x50, 0x80),
            interval_text: c(0xa8, 0xc8, 0xd8),
            recorder_bg: c(0x26, 0x42, 0x6a),
            fx_knob: c(0x00, 0xd8, 0xf8),
            master_knob: c(0xf0, 0x90, 0x30),
            varispeed: c(0xC9, 0xD8, 0xE8),
            meter_bg: c(0x0a, 0x1c, 0x34),
            scope_draw: c(0x40, 0xe8, 0xff),
            fx_strip: c(0x19, 0x2e, 0x48),
            input_chip: c(0x55, 0x78, 0xff),
            fretboard: c(0x0a, 0x1c, 0x34),
            string: c(0x78, 0x98, 0xb8),
                        velocity_loud: c(0xfe, 0x6c, 0x56),
            velocity_quiet: c(0x18, 0xad, 0x8f),
            channel_palette: Some(3),
            dark: dark_of(&[("background", (0x0a, 0x1c, 0x34))]),
        },
        // ── Mono: greys, and one red.
        //
        // Monochrome MOSTLY, by the owner's word, and the exception is the
        // master: a fader that can silence everything has to be the one thing
        // on screen that is not grey, or the discipline costs more than it
        // buys. The scope draws white, like a scope.
        Theme {
            name: "Mono".to_owned(),
            background: c(0xd8, 0xd8, 0xd8),
            white_key: c(0xd8, 0xd8, 0xd8),
            black_key: c(0x14, 0x14, 0x14),
            key_active: c(0xf0, 0xf0, 0xf0),
            sustain: c(0x90, 0x90, 0x90),
            key_separator: c(0x3a, 0x3a, 0x3a),
            text: c(0xe0, 0xe0, 0xe0),
            theory_line: c(0x5a, 0x5a, 0x5a),
            interval_text: c(0xa8, 0xa8, 0xa8),
            recorder_bg: c(0x3c, 0x3c, 0x3c),
            fx_knob: c(0xc8, 0xc8, 0xc8),
            // Bright enough to read on the grey band — the ONE non-grey, and
            // it has to be findable or the discipline costs more than it buys.
            master_knob: c(0xe8, 0x69, 0x5a),
            // **Mono's one grey that is not the effect knobs' grey.** A theme
            // built out of near-monochrome has nowhere to go but LIGHTNESS,
            // and 0xEF was thirteen ΔE from the dials — visibly the same
            // control in a different mood. 0x80 is twenty-seven away and
            // still unmistakably Mono.
            varispeed: c(0x80, 0x80, 0x80),
            meter_bg: c(0x14, 0x14, 0x14),
            scope_draw: c(0xd0, 0xd0, 0xd0),
            fx_strip: c(0x26, 0x26, 0x26),
            input_chip: c(0x8a, 0x8a, 0x8a),
            fretboard: c(0x1a, 0x1a, 0x1a),
            string: c(0x78, 0x78, 0x78),
                        velocity_loud: c(0xff, 0x70, 0x70),
            velocity_quiet: c(0x29, 0xab, 0x3f),
            channel_palette: Some(4),
            dark: dark_of(&[("background", (0x14, 0x14, 0x14))]),
        },
        // ── Studio: the analogue console. Warm charcoal, cream legends, VU
        // amber for anything that moves, an oxblood master, and the sage-green
        // lamp old gear uses for "connected" on the input chip.
        Theme {
            name: "Studio".to_owned(),
            background: c(0xe8, 0xe2, 0xd4),
            white_key: c(0xe8, 0xe2, 0xd4),
            black_key: c(0x19, 0x17, 0x14),
            key_active: c(0xd8, 0xa0, 0x3c),
            sustain: c(0x79, 0x5b, 0x28),
            key_separator: c(0x3a, 0x35, 0x2e),
            text: c(0xe8, 0xe2, 0xd4),
            theory_line: c(0x6b, 0x63, 0x55),
            interval_text: c(0xc0, 0x8f, 0x4a),
            recorder_bg: c(0x4a, 0x40, 0x38),
            fx_knob: c(0xd8, 0xa0, 0x3c),
            // Oxblood lifted twice: once because it was 2.1:1 against the
            // old band, again when the band itself came up out of near-black.
            // The one control that can silence everything must be findable.
            master_knob: c(0xe8, 0x72, 0x58),
            varispeed: c(0xDC, 0xD3, 0xBE),
            meter_bg: c(0x27, 0x24, 0x20),
            scope_draw: c(0xd8, 0xa0, 0x3c),
            fx_strip: c(0x3a, 0x35, 0x2e),
            input_chip: c(0x7f, 0xa0, 0x5a),
            fretboard: c(0x27, 0x24, 0x20),
            string: c(0xa8, 0x98, 0x80),
                        velocity_loud: c(0xff, 0x7e, 0x57),
            velocity_quiet: c(0x80, 0xb1, 0x5f),
            channel_palette: Some(5),
            dark: dark_of(&[("background", (0x19, 0x17, 0x14))]),
        },
        // ── Aurora: indigo night with a mint/violet complementary pair — mint
        // for what is sounding, violet for what is HELD, so the two states of
        // a note are opposite ends of the wheel and never confusable.
        Theme {
            name: "Aurora".to_owned(),
            background: c(0xdf, 0xe4, 0xee),
            white_key: c(0xdf, 0xe4, 0xee),
            black_key: c(0x12, 0x14, 0x1b),
            key_active: c(0x5c, 0xe8, 0xb4),
            sustain: c(0xa9, 0x7b, 0xf0),
            key_separator: c(0x2a, 0x2e, 0x40),
            text: c(0xdf, 0xe4, 0xee),
            theory_line: c(0x4a, 0x50, 0x68),
            interval_text: c(0x7f, 0xd4, 0xf0),
            recorder_bg: c(0x33, 0x3a, 0x54),
            fx_knob: c(0x5c, 0xe8, 0xb4),
            master_knob: c(0xe0, 0x56, 0x7a),
            varispeed: c(0x8F, 0xD8, 0xF5),
            meter_bg: c(0x1e, 0x21, 0x30),
            scope_draw: c(0x5c, 0xe8, 0xb4),
            fx_strip: c(0x2a, 0x2e, 0x40),
            input_chip: c(0xa9, 0x7b, 0xf0),
            fretboard: c(0x1e, 0x21, 0x30),
            string: c(0x6b, 0x72, 0x90),
                        velocity_loud: c(0xff, 0x70, 0xa5),
            velocity_quiet: c(0x1a, 0xb8, 0x63),
            channel_palette: Some(6),
            dark: dark_of(&[("background", (0x12, 0x14, 0x1b))]),
        },
        // ── Sandstone: the LIGHT one, and the reason the set needs it — every
        // other theme here is dark, and a person working in daylight has
        // nowhere to go. Warm sand paper, umber ink, terracotta for what
        // moves, sage for what is held. The scope draws DARK, because on a
        // pale meter a bright trace is invisible: the one place a light theme
        // cannot simply invert a dark one.
        Theme {
            name: "Sandstone".to_owned(),
            background: c(0xfa, 0xf4, 0xe8),
            white_key: c(0xfa, 0xf4, 0xe8),
            black_key: c(0x4a, 0x3f, 0x34),
            // **A light theme is not a dark one inverted.** On paper the
            // marks have to go DARKER to be seen, not brighter: the
            // terracotta is deepened until it reads on sand, and the sustain
            // is deepened further still, so a held note and a struck one are
            // told apart by weight the way ink is.
            key_active: c(0x9a, 0x49, 0x25),
            sustain: c(0x3a, 0x4a, 0x24),
            key_separator: c(0x9c, 0x8a, 0x70),
            text: c(0x3a, 0x31, 0x28),
            theory_line: c(0x9c, 0x8a, 0x70),
            interval_text: c(0x8a, 0x5a, 0x2c),
            recorder_bg: c(0xe0, 0xd3, 0xba),
            fx_knob: c(0xa8, 0x4f, 0x28),
            master_knob: c(0xa0, 0x38, 0x28),
            varispeed: c(0x40, 0x60, 0x6E),
            meter_bg: c(0xde, 0xd2, 0xbc),
            scope_draw: c(0x5a, 0x70, 0x40),
            fx_strip: c(0xd5, 0xc8, 0xb0),
            input_chip: c(0x7d, 0x8f, 0x5a),
            fretboard: c(0xc9, 0xb5, 0x96),
            string: c(0x6b, 0x5c, 0x48),
                        velocity_loud: c(0xc6, 0x3e, 0x1c),
            velocity_quiet: c(0x31, 0x85, 0x13),
            channel_palette: Some(7),
            dark: dark_of(&[("background", (0x4a, 0x3f, 0x34))]),
        },
        // ── Centennial: the DX7 II Centennial, 1987 — Yamaha's hundredth
        // birthday in brushed silver with gold legends and black cheeks. The
        // gold is an ACCENT and never the text: it is beautiful at 3:1 on
        // silver and unreadable as a paragraph. Slate blue on the input chip
        // is the one cool note, as it is on the instrument.
        Theme {
            name: "Centennial".to_owned(),
            background: c(0xf2, 0xf1, 0xee),
            white_key: c(0xf2, 0xf1, 0xee),
            black_key: c(0x26, 0x26, 0x2a),
            key_active: c(0x78, 0x5c, 0x17),
            sustain: c(0x3f, 0x4a, 0x55),
            key_separator: c(0x9a, 0x9a, 0xa0),
            text: c(0x1f, 0x1f, 0x23),
            theory_line: c(0x8e, 0x8e, 0x96),
            interval_text: c(0x6f, 0x57, 0x14),
            recorder_bg: c(0xc2, 0xc2, 0xc8),
            fx_knob: c(0x7d, 0x5f, 0x18),
            master_knob: c(0x9c, 0x32, 0x26),
            varispeed: c(0x35, 0x50, 0x6E),
            meter_bg: c(0x2a, 0x2a, 0x2e),
            scope_draw: c(0xd8, 0xb4, 0x5a),
            fx_strip: c(0xd5, 0xd5, 0xda),
            input_chip: c(0x5d, 0x6b, 0x78),
            fretboard: c(0x3a, 0x3a, 0x40),
            string: c(0xc9, 0xc9, 0xcf),
                        velocity_loud: c(0xff, 0xa3, 0xab),
            velocity_quiet: c(0x47, 0xd0, 0x67),
            channel_palette: Some(8),
            dark: dark_of(&[("background", (0x26, 0x26, 0x2a))]),
        },
        // ── Blueprint: a cyanotype. Prussian ground, chalk lines, and one
        // ochre for what is HELD — the pencil annotation on a blue print.
        Theme {
            name: "Blueprint".to_owned(),
            background: c(0xe6, 0xee, 0xf7),
            white_key: c(0xe6, 0xee, 0xf7),
            black_key: c(0x0d, 0x1c, 0x33),
            key_active: c(0x7f, 0xb2, 0xe8),
            sustain: c(0xf7, 0xd9, 0x8a),
            key_separator: c(0x2b, 0x4a, 0x70),
            text: c(0xe6, 0xee, 0xf7),
            theory_line: c(0x5b, 0x7f, 0xa8),
            interval_text: c(0xa8, 0xc4, 0xe0),
            recorder_bg: c(0x20, 0x45, 0x6e),
            fx_knob: c(0x7f, 0xb2, 0xe8),
            master_knob: c(0xf0, 0x70, 0x5f),
            varispeed: c(0xE8, 0xE4, 0xD8),
            meter_bg: c(0x0f, 0x24, 0x40),
            scope_draw: c(0x9f, 0xd0, 0xf5),
            fx_strip: c(0x1a, 0x3a, 0x5e),
            input_chip: c(0x6f, 0xc3, 0xb8),
            fretboard: c(0x13, 0x2b, 0x47),
            string: c(0x8f, 0xa8, 0xc0),
                        velocity_loud: c(0xfe, 0x7d, 0x3d),
            velocity_quiet: c(0x2b, 0xb4, 0xaa),
            channel_palette: Some(9),
            dark: dark_of(&[("background", (0x0d, 0x1c, 0x33))]),
        },
        // ── Porcelain: the other light theme, and cool where Sandstone is
        // warm. Delft cobalt on cool white, with a violet sustain so the two
        // note states are told apart by hue as well as by weight.
        Theme {
            name: "Porcelain".to_owned(),
            background: c(0xfb, 0xfb, 0xfc),
            white_key: c(0xfb, 0xfb, 0xfc),
            black_key: c(0x2a, 0x33, 0x40),
            key_active: c(0x2f, 0x5f, 0xa8),
            sustain: c(0x9b, 0x6f, 0xd0),
            key_separator: c(0xb6, 0xbc, 0xc4),
            text: c(0x23, 0x2a, 0x33),
            theory_line: c(0x9a, 0xa4, 0xb0),
            interval_text: c(0x3a, 0x6e, 0xa8),
            recorder_bg: c(0xcc, 0xd2, 0xd9),
            fx_knob: c(0x2f, 0x5f, 0xa8),
            master_knob: c(0xa8, 0x3a, 0x30),
            varispeed: c(0x24, 0x54, 0x4A),
            meter_bg: c(0x28, 0x31, 0x3c),
            scope_draw: c(0x4a, 0x90, 0xd0),
            fx_strip: c(0xdd, 0xe2, 0xe8),
            input_chip: c(0x5a, 0x8a, 0x7a),
            fretboard: c(0x39, 0x42, 0x4e),
            string: c(0xc2, 0xc8, 0xd0),
                        velocity_loud: c(0xff, 0xa3, 0xb6),
            velocity_quiet: c(0x33, 0xd7, 0x7a),
            channel_palette: Some(10),
            dark: dark_of(&[("background", (0x2a, 0x33, 0x40))]),
        },
        // ── Moss: deep forest, lichen text, and a low amber sustain. The
        // sustain is DARKER than the struck note here — on a green ground a
        // second bright mark competes with the first.
        Theme {
            name: "Moss".to_owned(),
            background: c(0xe4, 0xea, 0xd9),
            white_key: c(0xe4, 0xea, 0xd9),
            black_key: c(0x13, 0x1a, 0x12),
            key_active: c(0x8f, 0xc4, 0x6a),
            sustain: c(0xb0, 0x7f, 0x28),
            key_separator: c(0x2c, 0x3a, 0x26),
            text: c(0xe4, 0xea, 0xd9),
            theory_line: c(0x6b, 0x7d, 0x5e),
            interval_text: c(0xb9, 0xcf, 0x9a),
            recorder_bg: c(0x2f, 0x40, 0x29),
            fx_knob: c(0x8f, 0xc4, 0x6a),
            master_knob: c(0xe8, 0x6a, 0x4a),
            varispeed: c(0xD8, 0xCF, 0xA8),
            meter_bg: c(0x16, 0x1f, 0x13),
            scope_draw: c(0xa8, 0xd8, 0x78),
            fx_strip: c(0x26, 0x33, 0x1f),
            input_chip: c(0x7f, 0xae, 0x94),
            fretboard: c(0x1d, 0x27, 0x18),
            string: c(0x9a, 0xa8, 0x8c),
                        velocity_loud: c(0xff, 0x8a, 0x3d),
            velocity_quiet: c(0x1b, 0xbe, 0x9d),
            channel_palette: Some(11),
            dark: dark_of(&[("background", (0x13, 0x1a, 0x12))]),
        },
        // ── Rhodes: black tolex, cream keys, and the nameplate's red on the
        // master. Warm where Studio is neutral: this is a wooden instrument
        // in a case, not a console in a rack.
        Theme {
            name: "Rhodes".to_owned(),
            background: c(0xef, 0xe6, 0xd2),
            white_key: c(0xef, 0xe6, 0xd2),
            black_key: c(0x16, 0x13, 0x11),
            key_active: c(0xd9, 0xa4, 0x41),
            sustain: c(0x96, 0x74, 0xc0),
            key_separator: c(0x3a, 0x30, 0x2a),
            text: c(0xef, 0xe6, 0xd2),
            theory_line: c(0x7a, 0x6a, 0x58),
            interval_text: c(0xc9, 0xa0, 0x6a),
            recorder_bg: c(0x3c, 0x32, 0x2a),
            fx_knob: c(0xd9, 0xa4, 0x41),
            master_knob: c(0xe0, 0x58, 0x4a),
            varispeed: c(0x7F, 0xB6, 0xA8),
            meter_bg: c(0x1a, 0x15, 0x12),
            scope_draw: c(0xe8, 0xc4, 0x6a),
            fx_strip: c(0x2e, 0x26, 0x20),
            input_chip: c(0x8f, 0xa0, 0x6a),
            fretboard: c(0x24, 0x1d, 0x18),
            string: c(0xb8, 0xac, 0x98),
                        velocity_loud: c(0xff, 0x67, 0x57),
            velocity_quiet: c(0x18, 0xad, 0x71),
            channel_palette: Some(12),
            dark: dark_of(&[("background", (0x16, 0x13, 0x11))]),
        },
        // ── Copper: verdigris and rust, the two things copper becomes. The
        // patina is the ground and the oxide is what moves.
        Theme {
            name: "Copper".to_owned(),
            background: c(0xe8, 0xe2, 0xd4),
            white_key: c(0xe8, 0xe2, 0xd4),
            black_key: c(0x14, 0x20, 0x1d),
            key_active: c(0xe0, 0x8a, 0x4a),
            sustain: c(0x6f, 0xd8, 0xc0),
            key_separator: c(0x2f, 0x4a, 0x44),
            text: c(0xe8, 0xe2, 0xd4),
            theory_line: c(0x6a, 0x8a, 0x82),
            interval_text: c(0x8f, 0xd8, 0xc8),
            recorder_bg: c(0x2c, 0x4a, 0x44),
            fx_knob: c(0xe0, 0x8a, 0x4a),
            master_knob: c(0xf0, 0x70, 0x58),
            varispeed: c(0xE6, 0xDA, 0xC2),
            meter_bg: c(0x16, 0x24, 0x23),
            scope_draw: c(0x6f, 0xd8, 0xc0),
            fx_strip: c(0x23, 0x3a, 0x36),
            input_chip: c(0xc0, 0x8a, 0x5a),
            fretboard: c(0x1b, 0x2f, 0x2b),
            string: c(0xa8, 0xb8, 0xb2),
                        velocity_loud: c(0xff, 0x83, 0x57),
            velocity_quiet: c(0x1a, 0xbb, 0x90),
            channel_palette: Some(13),
            dark: dark_of(&[("background", (0x14, 0x20, 0x1d))]),
        },
        // ── Dusk: the sky twenty minutes after sunset — violet overhead,
        // warm at the horizon. The horizon colour is what sounds; the
        // overhead blue is what is held.
        Theme {
            name: "Dusk".to_owned(),
            background: c(0xec, 0xe6, 0xf0),
            white_key: c(0xec, 0xe6, 0xf0),
            black_key: c(0x16, 0x12, 0x28),
            key_active: c(0xf0, 0x92, 0x5a),
            sustain: c(0x7f, 0x8c, 0xe0),
            key_separator: c(0x33, 0x2a, 0x4a),
            text: c(0xec, 0xe6, 0xf0),
            theory_line: c(0x6a, 0x5f, 0x8a),
            interval_text: c(0xc0, 0xa8, 0xe8),
            recorder_bg: c(0x3a, 0x2f, 0x55),
            fx_knob: c(0xf0, 0x92, 0x5a),
            master_knob: c(0xe0, 0x4a, 0x6a),
            varispeed: c(0xC0, 0xA8, 0xE8),
            meter_bg: c(0x1b, 0x16, 0x30),
            scope_draw: c(0xf0, 0xb0, 0x70),
            fx_strip: c(0x2c, 0x24, 0x44),
            input_chip: c(0x8f, 0x7f, 0xd0),
            fretboard: c(0x22, 0x1b, 0x3a),
            string: c(0x9a, 0x90, 0xb8),
                        velocity_loud: c(0xff, 0x70, 0x96),
            velocity_quiet: c(0x19, 0xb3, 0x99),
            channel_palette: Some(14),
            dark: dark_of(&[("background", (0x16, 0x12, 0x28))]),
        },
        // ── Iris: deep violet with its complement, chartreuse. The widest
        // hue gap in the set, so a sounding note cannot be mistaken for a
        // held one at any glance.
        Theme {
            name: "Iris".to_owned(),
            background: c(0xee, 0xe8, 0xf4),
            white_key: c(0xee, 0xe8, 0xf4),
            black_key: c(0x19, 0x10, 0x26),
            key_active: c(0xc8, 0xe0, 0x5a),
            sustain: c(0xb0, 0x7f, 0xe8),
            key_separator: c(0x3a, 0x2a, 0x52),
            text: c(0xee, 0xe8, 0xf4),
            theory_line: c(0x6f, 0x5a, 0x8c),
            interval_text: c(0xd8, 0xe8, 0x8a),
            recorder_bg: c(0x3f, 0x2d, 0x5c),
            fx_knob: c(0xc8, 0xe0, 0x5a),
            master_knob: c(0xe0, 0x50, 0x7a),
            varispeed: c(0x9A, 0xB4, 0xF0),
            meter_bg: c(0x1e, 0x13, 0x30),
            scope_draw: c(0xd8, 0xf0, 0x70),
            fx_strip: c(0x31, 0x22, 0x43),
            input_chip: c(0x9f, 0x7f, 0xd8),
            fretboard: c(0x25, 0x18, 0x40),
            string: c(0xa8, 0x98, 0xc0),
                        velocity_loud: c(0xff, 0x57, 0xab),
            velocity_quiet: c(0x18, 0xae, 0x4a),
            channel_palette: Some(15),
            dark: dark_of(&[("background", (0x19, 0x10, 0x26))]),
        },
    ]
}

/// Every user theme on disk, by file name order. Unreadable files are skipped
/// rather than reported: a themes folder is somewhere people put junk, and one
/// bad file must not cost the menu.
pub fn user_themes() -> Vec<Theme> {
    // This IS "the first time the app looks", so the example goes in here —
    // `seed_example` sat fully written and never called, which meant the
    // folder the menu told people about was empty when they opened it. Best
    // effort and cheap: one `exists()` stat once the file is there, and a
    // read-only config dir costs a shrug rather than the menu.
    seed_example();
    let dir = user_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("json")))
        .collect();
    paths.sort();
    paths
        .iter()
        .filter_map(|p| {
            let text = std::fs::read_to_string(p).ok()?;
            let stem = p.file_stem().map(|s| s.to_string_lossy().into_owned())?;
            Theme::from_json(&text, &stem)
        })
        // A theme that sets NOTHING is a template, not a theme — which is
        // exactly what the seeded example is until somebody edits it. In the
        // menu it would be a row that does nothing, and it would do nothing
        // under whatever name the file happens to carry. The moment one role
        // is filled in, it earns its row.
        .filter(|t| {
            t.channel_palette.is_some()
                || !t.dark.is_empty()
                || ROLES.iter().any(|r| t.role(r).is_some())
        })
        .collect()
}

/// Find a theme by name: factory first, then the user folder. A user theme
/// that shadows a factory name loses, so "Default" always means Default.
pub fn by_name(name: &str) -> Option<Theme> {
    factory()
        .into_iter()
        .find(|t| t.name == name)
        .or_else(|| user_themes().into_iter().find(|t| t.name == name))
}

/// The example file, written into the user folder the first time the app
/// looks, so somebody who opens that folder finds a template rather than
/// nothing. Every role present and EMPTY, which is legal and does nothing —
/// the point is that the key names are discoverable without documentation.
pub const EXAMPLE: &str = r##"{
  "name": "My Theme",

  "_comment": "Every value below is optional. Delete a line to keep Tangent's",
  "_comment2": "default for that role, or set it to a hex colour like \"#ff5fa2\".",
  "_comment3": "Colours accept #rgb, #rrggbb, or rrggbb.",

  "background": "",
  "white_key": "",
  "black_key": "",
  "key_active": "",
  "sustain": "",
  "key_separator": "",

  "text": "",
  "theory_line": "",
  "interval_text": "",

  "recorder_bg": "",
  "fx_knob": "",
  "master_knob": "",
  "varispeed": "",
  "meter_bg": "",
  "scope_draw": "",

  "fx_strip": "",
  "input_chip": "",

  "fretboard": "",
  "string": "",

  "_comment_velocity": "The MIDI editor's velocity lane: the colour a note at",
  "_comment_velocity2": "127 wears and the colour one at 1 does; every velocity",
  "_comment_velocity3": "between them is on the line. Both are pushed away from the",
  "_comment_velocity4": "roll until they can be seen, so a hard-to-read pair still",
  "_comment_velocity5": "gives you a lane you can read.",
  "velocity_loud": "",
  "velocity_quiet": "",

  "_comment_dark": "DARK MODE. Any role above can be named again in here, and",
  "_comment_dark2": "what you name WINS. Leave a role out and it is derived, the",
  "_comment_dark3": "way dark mode has always worked - the keyboard inverts, the",
  "_comment_dark4": "desk darkens. Leave the whole block out and nothing changes.",
  "dark": {
    "background": "",
    "white_key": "",
    "black_key": "",
    "key_active": "",
    "sustain": "",
    "key_separator": "",
    "text": "",
    "theory_line": "",
    "interval_text": "",
    "recorder_bg": "",
    "fx_knob": "",
    "master_knob": "",
    "varispeed": "",
    "meter_bg": "",
    "scope_draw": "",
    "fx_strip": "",
    "input_chip": "",
    "fretboard": "",
    "string": "",
    "velocity_loud": "",
    "velocity_quiet": ""
  },

  "_comment4": "channel_palette picks which theme's auto-palette the sixteen",
  "_comment5": "channels, the click and the master wear. A number, not a colour,",
  "_comment6": "one palette per factory theme, in the menu's order:",
  "_comment7": "0 Default     1 Valentine   2 Hydra       3 Navi",
  "_comment8": "4 Mono        5 Studio      6 Aurora      7 Sandstone",
  "_comment9": "8 Centennial  9 Blueprint  10 Porcelain  11 Moss",
  "_comment10": "12 Rhodes    13 Copper    14 Dusk       15 Iris",
  "_comment11": "It only takes effect while the track menu's palette is on AUTO;",
  "_comment12": "a palette picked by hand there outlives any theme. Leave it out",
  "_comment13": "and the channels keep dressing for whichever theme is applied.",
  "channel_palette": ""
}
"##;

/// Put the example in the user folder if that folder has no example yet.
/// Never overwrites: somebody may have edited it into a real theme.
pub fn seed_example() {
    let dir = user_dir();
    let path = dir.join("example.json");
    if path.exists() {
        return;
    }
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = std::fs::write(&path, EXAMPLE);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_factory_set_is_sixteen_themes_with_default_first() {
        let f = factory();
        assert_eq!(f.len(), 16, "Default, seven, and eight more");
        assert_eq!(f[0].name, "Default");
        // **Default RESTORES rather than declining to speak.** A theme that
        // set nothing left whatever the last theme wrote, so picking Default
        // after trying one gave neither. Every role is named.
        let d = &f[0];
        assert_eq!(d.recorder_bg, Some(Rgb::new(0x61, 0x37, 0x3a)));
        assert_eq!(d.white_key, Some(Rgb::new(0xE8, 0xDC, 0xC0)));
        // And applying it over a desk wearing another theme puts every value
        // back to the one a fresh install has.
        let mut s = Settings::default();
        let shipped = Settings::default();
        f[3].apply(&mut s);
        assert_ne!(s.recorder_bg_color, shipped.recorder_bg_color);
        d.apply(&mut s);
        assert_eq!(s.recorder_bg_color, shipped.recorder_bg_color);
        assert_eq!(s.white_key_idle_color, shipped.white_key_idle_color);
        assert_eq!(s.scope_draw_color, shipped.scope_draw_color);
        assert_eq!(s.fretboard_color, shipped.fretboard_color);
        // Names are unique, or the menu has two rows that do the same thing.
        let mut names: Vec<&str> = f.iter().map(|t| t.name.as_str()).collect();
        names.sort_unstable();
        let before = names.len();
        names.dedup();
        assert_eq!(names.len(), before, "duplicate theme name");
    }

    /// **A theme that does not name a palette is a theme the channels
    /// ignore.** Every factory theme used to leave `channel_palette` unset,
    /// so switching theme repainted the whole desk EXCEPT the sixteen strips
    /// in the middle of it — they kept whichever palette happened to be there,
    /// which is how the owner ended up looking at a purple theme wearing an
    /// orange click and a red master.

    /// **A user theme can name ANY colour, for either mode.**
    ///
    /// The owner's word: somebody who cares enough to write a theme gets the
    /// freedom to say what everything looks like. Dark mode used to be a
    /// transform designed around the colours this app shipped with, and a
    /// theme had no way to disagree with it.
    #[test]
    fn a_theme_can_name_every_role_for_dark_mode_too() {
        let mut light = String::from("{\n  \"name\": \"Nightshift\"");
        for role in ROLES {
            light.push_str(&format!(",\n  \"{role}\": \"#123456\""));
        }
        light.push_str(",\n  \"dark\": {");
        for (i, role) in ROLES.iter().enumerate() {
            light.push_str(&format!(
                "{}\n    \"{role}\": \"#abcdef\"",
                if i == 0 { "" } else { "," }
            ));
        }
        light.push_str("\n  }\n}");

        let t = Theme::from_json(&light, "x").expect("the theme parses");
        assert_eq!(t.dark.len(), ROLES.len(), "a role was dropped on the way in");
        for role in ROLES {
            assert_eq!(
                t.dark.get(role).map(|c| c.to_hex()),
                Some("#abcdef".to_owned()),
                "{role} did not survive"
            );
        }

        // It reaches the settings every panel reads...
        let mut set = Settings::default();
        t.apply(&mut set);
        for role in ROLES {
            assert!(set.dark_role(role).is_some(), "{role} never reached settings");
        }
        // ...and comes back out again, so Save Theme As keeps both modes.
        let again = Theme::capture("Nightshift", &set);
        assert_eq!(again.dark, t.dark, "capture lost the dark set");
        let round = Theme::from_json(&again.to_json(), "x").expect("round trips");
        assert_eq!(round.dark, t.dark, "the file lost the dark set");
    }

    /// A theme that says nothing about dark mode still behaves exactly as it
    /// did — the derivation is the default, not a fallback nobody reaches.
    #[test]
    fn a_theme_with_no_dark_block_leaves_dark_mode_derived() {
        let t = Theme::from_json(r##"{"name": "Quiet", "text": "#ffffff"}"##, "x")
            .expect("parses");
        assert!(t.dark.is_empty());
        let mut set = Settings::default();
        t.apply(&mut set);
        for role in ROLES {
            assert!(set.dark_role(role).is_none(), "{role} was invented");
        }
    }


    /// **The theory ground is the KEYBOARD's, not the band's.**
    ///
    /// The first pass derived it from `recorder_bg`, which on the default
    /// theme is the band's wine — so the theory modules came up on PINK. The
    /// owner's rule: a page of music sits on the same bone the naturals are,
    /// with the same black on it, and in dark mode on the same near-black the
    /// sharps are.
    #[test]
    fn the_theory_ground_is_the_keyboard_not_the_band() {
        for t in factory() {
            let bg = t.background.expect("every theme names a ground");
            assert_eq!(
                Some(bg),
                t.white_key,
                "{}: the light ground is not the naturals' colour",
                t.name
            );
            let dark = t.dark.get("background").copied();
            assert_eq!(
                dark, t.black_key,
                "{}: the dark ground is not the sharps' colour",
                t.name
            );
            // And specifically NOT the band, which is what produced the pink.
            if t.recorder_bg != t.white_key {
                assert_ne!(
                    Some(bg),
                    t.recorder_bg,
                    "{}: the theory ground came from the band again",
                    t.name
                );
            }
        }
        // The default, named outright: black on bone, the way this view has
        // always looked.
        let d = Theme::factory_default();
        assert_eq!(d.background, Some(Rgb { r: 0xE8, g: 0xDC, b: 0xC0 }));
        assert_eq!(
            d.dark.get("background").copied(),
            Some(Rgb { r: 0x1a, g: 0x1a, b: 0x1a })
        );
    }


    /// **A hand-picked palette outlives the next theme; AUTO does not.**
    ///
    /// The track menu offers all eight and an AUTO row. Auto is the default
    /// and means "whatever this theme names" — which is what makes sixteen
    /// themes each arrive dressed. Choosing one by hand is a decision, and a
    /// decision that the next theme quietly undid would be worse than not
    /// offering the choice.
    #[test]
    fn a_hand_picked_palette_survives_a_theme_change() {
        let themed: Vec<Theme> = factory()
            .into_iter()
            .filter(|t| t.channel_palette.is_some())
            .collect();
        assert!(themed.len() > 8, "too few themes name a palette to test with");

        // AUTO: every theme brings its own along.
        let mut s = Settings::default();
        assert!(s.palette_auto, "auto is not the default");
        let mut seen = std::collections::HashSet::new();
        for t in &themed {
            t.apply(&mut s);
            assert_eq!(
                Some(s.palette_choice),
                t.channel_palette,
                "{} did not bring its palette",
                t.name
            );
            seen.insert(s.palette_choice);
        }
        assert!(seen.len() > 1, "every theme landed on the same palette");

        // HAND-PICKED: nothing takes it back.
        let mut s = Settings::default();
        s.palette_auto = false;
        s.palette_choice = 6;
        for t in &themed {
            t.apply(&mut s);
            assert_eq!(s.palette_choice, 6, "{} overrode a chosen palette", t.name);
        }
    }


    /// **A palette per theme, named for it, and each theme points at its own.**
    ///
    /// They used to be eight abstract sets that sixteen themes shared. The
    /// owner's word was "pick and choose": you can put Rhodes' colours on the
    /// Iris desk, and to do that they have to be NAMED, which means one each.
    #[test]
    fn every_theme_has_a_palette_of_its_own_name() {
        let themes = factory();
        assert_eq!(
            themes.len(),
            crate::mixer_panel::PALETTES.len(),
            "there is no longer one palette per theme"
        );
        for (i, t) in themes.iter().enumerate() {
            assert_eq!(
                t.channel_palette,
                Some(i as i64),
                "{} does not point at its own palette",
                t.name
            );
            assert_eq!(
                crate::mixer_panel::PALETTES[i].name, t.name,
                "palette {i} is not named for the theme that uses it"
            );
        }
    }

    #[test]
    fn every_factory_theme_dresses_its_channels() {
        for t in factory() {
            let choice = t
                .channel_palette
                .unwrap_or_else(|| panic!("{} names no palette", t.name));
            assert!(
                (choice as usize) < crate::mixer_panel::PALETTES.len(),
                "{} names palette {choice}, which does not exist",
                t.name
            );
        }
    }

    #[test]
    fn every_non_default_factory_theme_fills_every_role() {
        // A half-filled factory theme reads as a bug on somebody's screen:
        // half the app in the new colours, half in the old ones.
        for t in factory().into_iter().skip(1) {
            // **Walked from `ROLES`, never from a list written here.** The
            // list that used to live in this test held seventeen names and
            // `ROLES` held nineteen, so `varispeed` — a role in every factory
            // theme and in the reader — was going unchecked. A test that
            // keeps its own copy of the thing it is checking will always
            // eventually check the copy.
            for role in ROLES {
                assert!(
                    t.role(role).is_some(),
                    "{} leaves {role} unset",
                    t.name
                );
            }
        }
    }

    /// Relative luminance, the WCAG definition, for the contrast check below.
    fn lum(c: Rgb) -> f64 {
        let f = |v: u8| {
            let s = f64::from(v) / 255.0;
            if s <= 0.03928 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * f(c.r) + 0.7152 * f(c.g) + 0.0722 * f(c.b)
    }

    fn contrast(a: Rgb, b: Rgb) -> f64 {
        let (x, y) = (lum(a), lum(b));
        let (hi, lo) = if x > y { (x, y) } else { (y, x) };
        (hi + 0.05) / (lo + 0.05)
    }

    #[test]
    fn every_factory_theme_is_readable_on_its_own_background() {
        // The whole point of shipping themes rather than leaving it to a
        // config file is that the shipped ones are KNOWN to work. Text on the
        // band, the scope trace in its own well, and the knobs against the
        // band all have to clear a real threshold — a theme that looks
        // striking in a table and is unreadable on screen is worse than no
        // theme at all. 4.5:1 is WCAG AA for text; the marks are held to 3:1,
        // the large-object threshold.
        for t in factory().into_iter().skip(1) {
            let bg = t.recorder_bg.expect("checked above");
            let text = t.text.expect("checked above");
            assert!(
                contrast(text, bg) >= 4.5,
                "{}: text on the band is {:.1}:1",
                t.name,
                contrast(text, bg)
            );
            let meter = t.meter_bg.expect("checked above");
            let scope = t.scope_draw.expect("checked above");
            assert!(
                contrast(scope, meter) >= 3.0,
                "{}: the scope trace is {:.1}:1 against its own well",
                t.name,
                contrast(scope, meter)
            );
            for (role, mark) in [
                ("fx_knob", t.fx_knob.expect("checked")),
                ("master_knob", t.master_knob.expect("checked")),
                ("key_active", t.key_active.expect("checked")),
            ] {
                assert!(
                    contrast(mark, bg) >= 3.0,
                    "{}: {role} is {:.1}:1 against the band",
                    t.name,
                    contrast(mark, bg)
                );
            }
            // A held note must not look like a sounding one.
            let (a, s) = (t.key_active.expect("checked"), t.sustain.expect("checked"));
            assert!(
                contrast(a, s) >= 1.25,
                "{}: sustain and active are {:.2}:1 apart - too close to tell",
                t.name,
                contrast(a, s)
            );
        }
    }

    /// **A band has to have room UNDER and OVER it.**
    ///
    /// The recorder derives everything it paints from one colour: the well a
    /// meter sits in is the band times 0.55, the face of a control is the
    /// band times 1.35. Hand it a near-black and both land on black - every
    /// recess, every control face and every fader track collapse into one
    /// dark mush. That is exactly what the first pass of these themes did,
    /// because they took each app's TERMINAL BACKGROUND, which is a colour
    /// meant to sit behind text rather than to be a surface with parts.
    ///
    /// So: every band must separate its own derived parts about as well as
    /// the shipped one does.
    #[test]
    fn every_band_leaves_room_for_the_parts_derived_from_it() {
        let shade = |c: Rgb, k: f32| Rgb {
            r: (f32::from(c.r) * k).min(255.0) as u8,
            g: (f32::from(c.g) * k).min(255.0) as u8,
            b: (f32::from(c.b) * k).min(255.0) as u8,
        };
        for t in factory() {
            let bg = t.recorder_bg.expect("every factory theme names its band");
            // Which way the band shades depends on which ink it takes, the
            // same question `recorder_panel::wants_light_ink` asks.
            let light_ink = contrast(bg, Rgb::new(0xE8, 0xDC, 0xC0))
                > contrast(bg, Rgb::new(0x1a, 0x1a, 0x1a));
            let (well, field) = if light_ink {
                (shade(bg, 0.55), shade(bg, 1.35))
            } else {
                (shade(bg, 0.88), shade(bg, 0.94))
            };
            let sep = contrast(well, field);
            assert!(
                sep >= 1.12,
                "{}: the band's well and control face are {sep:.2}:1 apart - \
                 the whole strip reads as one flat colour",
                t.name
            );
        }
    }

    #[test]
    fn a_theme_round_trips_through_its_file_format() {
        let t = &factory()[1];
        let back = Theme::from_json(&t.to_json(), "ignored").expect("valid json");
        assert_eq!(&back, t);
    }

    #[test]
    fn a_partial_theme_leaves_every_other_role_alone() {
        // The example file's whole premise: set one colour, inherit the rest.
        let json = r##"{"name": "Two Lines", "sustain": "#123456"}"##;
        let t = Theme::from_json(json, "file-name").expect("valid json");
        assert_eq!(t.name, "Two Lines");
        assert_eq!(t.sustain, Some(Rgb::new(0x12, 0x34, 0x56)));
        assert_eq!(t.text, None);
        assert_eq!(t.recorder_bg, None);
    }

    #[test]
    fn empty_strings_in_the_example_are_unset_rather_than_black() {
        // The shipped example has every key present and empty. If "" parsed
        // as a colour, opening the app with it would paint everything black.
        let t = Theme::from_json(EXAMPLE, "example").expect("the example is valid json");
        assert_eq!(t.name, "My Theme");
        assert_eq!(t.white_key, None, "an empty value must not become a colour");
        assert_eq!(t.scope_draw, None);
        assert_eq!(t.channel_palette, None);
    }

    /// **The example's palette list is the shipped palette list.**
    ///
    /// It described the eight retired abstract palettes for a full release
    /// after the sixteen theme-named ones replaced them — nobody noticed
    /// because the file it documents was never being written. Walked from
    /// `PALETTES` so the list cannot go quietly stale again.
    #[test]
    fn the_example_names_every_palette_by_its_current_name() {
        for (i, p) in crate::mixer_panel::PALETTES.iter().enumerate() {
            assert!(
                EXAMPLE.contains(&format!("{i} {}", p.name)),
                "the example does not list palette {i} as {}",
                p.name
            );
        }
        assert!(
            !EXAMPLE.contains("IVORY"),
            "the retired abstract palette names are back in the example"
        );
    }

    /// **The example file exists once the app has looked, and stays out of
    /// the menu.** `seed_example` was written, documented and never called;
    /// the folder the menu advertised was empty when people opened it. It
    /// seeds on the first listing now — and the untouched template, which
    /// sets nothing, must not become a "My Theme" row that does nothing.
    #[test]
    fn the_example_appears_the_first_time_the_app_looks() {
        let path = user_dir().join("example.json");
        let _ = std::fs::remove_file(&path);
        let listed = user_themes();
        assert!(path.exists(), "looking did not leave the example behind");
        let text = std::fs::read_to_string(&path).expect("the example reads back");
        assert_eq!(text, EXAMPLE, "something other than the example was seeded");
        assert!(
            !listed.iter().any(|t| t.name == "My Theme"),
            "the untouched template took a row in the menu"
        );
        // Filled in — one role is enough — it earns its row.
        let mut mine = Theme::from_json(EXAMPLE, "example").expect("parses");
        mine.sustain = Some(Rgb::new(0x12, 0x34, 0x56));
        std::fs::write(&path, mine.to_json()).expect("the edit lands");
        assert!(
            user_themes().iter().any(|t| t.name == "My Theme"),
            "an edited template is a real theme and must be listed"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_theme_name_can_never_write_outside_the_themes_folder() {
        let evil = Theme {
            name: "../../../etc/passwd".to_owned(),
            ..Theme::default()
        };
        let stem: String = evil
            .name
            .chars()
            .map(|ch| {
                if ch.is_alphanumeric() || ch == ' ' || ch == '-' || ch == '_' {
                    ch
                } else {
                    '_'
                }
            })
            .collect();
        assert!(!stem.contains('/'), "path separators survive: {stem}");
        assert!(!stem.contains(".."), "traversal survives: {stem}");
    }
}
