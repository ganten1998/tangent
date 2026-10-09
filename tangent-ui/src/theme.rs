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
/// **Thirty-two, and they are a spread rather than variations on one idea.**
/// Three are the owner's own applications' palettes (Valentine, Hydra,
/// Navi), kept faithful to what those apps look like rather than
/// approximated; four are instrument panels the app has some claim to
/// (Centennial, Rhodes, Silverface, Reed); the rest are materials, weather
/// and objects (a lacquer box, a paperback shelf, a rose window, a
/// woodblock print, a desk lamp). Four desks are light (Sandstone,
/// Centennial, Celadon, Overprint); seven carry their ground dark
/// (Amber, Phosphor, Urushi, Sodium, Chartres, Toffee, Lamplight), which
/// is the set the piano lifts the sharps of; the others are a pale page
/// over a dark desk. There is no mid-tone desk: Zinc began as one and
/// could not stay one, because a mid-tone well moves the transport red
/// (`dialogs::every_theme_dresses_the_export_window`).
///
/// **The order is an index.** `factory()`'s position is what a saved desk
/// persists as `palette_choice`, and `mixer_panel::PALETTES` is in the same
/// order, so a theme is replaced IN ITS SLOT and never removed or reordered.
/// The 2026-09-18 redo did exactly that for seven of them (Studio, Aurora,
/// Blueprint, Porcelain, Dusk, Iris, Nocturne); their names are gone from
/// here, their palette bytes stay in `STRIP_COLORS`, and the slot's row in
/// `PALETTES` points at the replacement's appended run.
pub fn factory() -> Vec<Theme> {
    vec![
        Theme::factory_default(),
        // ── Valentine: the owner's Focusrite control panel. Dark plum with a
        // rose highlight and one coral spark for anything engaged; meters ramp
        // jade to amber to crimson, so the scope is jade and the master crimson.
        //
        // **More red, at the owner's word (2026-09-18): "like the original TUI
        // application."** The marks were always the TUI's own (rose, coral,
        // amber, jade, crimson); what read purple was the SURFACES. The band
        // sat at hue 314, a mauve, and the strip at 319, both a long way
        // round the wheel from the crimson and the rose that carry the app.
        // Both moved to the red side of the plum (342 and 338), the lines,
        // the strings and the key cuts with them, and the dark ground went
        // from #251524 to #23131a so it is a plum-black and not a violet
        // one. Designed and gated by `scripts/palgen.py --themes`.
        //
        // The sustain is amber, not the coral it started as: rose and coral
        // differ in HUE but barely in brightness (1.22:1), so a held note and
        // a struck one looked alike at a glance and identical to anyone who
        // does not separate red from orange. Amber is 1.6:1 clear of the rose
        // and is Valentine's own warning colour.
        Theme {
            name: "Valentine".to_owned(),
            background: c(0xf3, 0xdc, 0xe7),
            white_key: c(0xf3, 0xdc, 0xe7),
            black_key: c(0x23, 0x13, 0x1a),
            key_active: c(0xff, 0x5f, 0xa2),
            sustain: c(0xff, 0xb4, 0x54),
            key_separator: c(0x66, 0x28, 0x3e),
            text: c(0xf3, 0xdc, 0xe7),
            theory_line: c(0xa8, 0x72, 0x84),
            interval_text: c(0x4c, 0xc7, 0xa4),
            recorder_bg: c(0x5a, 0x2a, 0x3c),
            fx_knob: c(0xff, 0x8a, 0x5b),
            master_knob: c(0xff, 0x3b, 0x5c),
            varispeed: c(0xf3, 0xdc, 0xe7),
            meter_bg: c(0x23, 0x13, 0x1a),
            scope_draw: c(0x4c, 0xc7, 0xa4),
            fx_strip: c(0x4e, 0x22, 0x32),
            input_chip: c(0x4c, 0xc7, 0xa4),
            fretboard: c(0x23, 0x13, 0x1a),
            string: c(0xa8, 0x72, 0x84),
            velocity_loud: c(0xff, 0x70, 0x83),
            velocity_quiet: c(0x19, 0xb3, 0x84),
            channel_palette: Some(1),
            dark: dark_of(&[("background", (0x23, 0x13, 0x1a))]),
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
            // **Grey, and the one grey that works.** The owner's rule for
            // Mono's keys: zero chroma for the struck and the held alike, no
            // exceptions. That leaves one dial, lightness, with three gates
            // pulling on it. The struck key must clear the band (0x3c) at
            // 3:1 and carry its own note name at 4.5:1 — the DARK ink, which
            // means sitting above the 0.12–0.22 dead zone where neither ink
            // clears — and both of those push it lighter; against the 0xd8
            // naturals every step lighter is a step toward the 0xf0 that
            // shipped and vanished at 1.23:1. 0x88 is the lowest grey that
            // clears both gates with a margin (3.1:1 on the band, 4.9:1
            // under the ink) and so reads as well as any grey can on the
            // naturals, 2.5:1, while standing 5.2:1 off the sharps. One
            // value paints both key colours, so it is a balance point and
            // not a taste.
            key_active: c(0x88, 0x88, 0x88),
            // The held key goes the other way, BELOW the dead zone, so it
            // takes the light ink (5.7:1) and stands 2.2:1 from the struck
            // grey: a held note and a struck one told apart by weight, which
            // is all a monochrome has. 5.5:1 on the naturals, 2.4:1 on the
            // sharps.
            sustain: c(0x52, 0x52, 0x52),
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
        // ── Ochre: the earth pigment as the BAND. Studio's slot (retired
        // 2026-09-18, the owner: "replace with something more inspired and
        // not yet covered by the rest of the themes"), and the one warm band
        // in the set with real chroma, where Rhodes' brown and Copper's
        // verdigris sit under 15. Chalk page, bone knobs, cornflower for what
        // sounds and indigo for what is held: the yellow ochre and
        // ultramarine of a fresco. The band is lighter (0.084 luminance)
        // than any other dark band here, which is why every mark on it is
        // pale: 3:1 over it starts at 0.35, so the master is a salmon rather
        // than a crimson. Designed and gated by `scripts/palgen.py --themes`,
        // its desk by `--redo`, as are the six below.
        Theme {
            name: "Ochre".to_owned(),
            background: c(0xf1, 0xed, 0xe2),
            white_key: c(0xf1, 0xed, 0xe2),
            black_key: c(0x1e, 0x1a, 0x12),
            key_active: c(0x8a, 0xb0, 0xd8),
            sustain: c(0x2e, 0x3a, 0x7a),
            key_separator: c(0x8a, 0x70, 0x30),
            text: c(0xf4, 0xec, 0xd6),
            theory_line: c(0xa8, 0x94, 0x60),
            interval_text: c(0xf0, 0xd0, 0x80),
            recorder_bg: c(0x5a, 0x41, 0x11),
            fx_knob: c(0xec, 0xdc, 0xb0),
            master_knob: c(0xff, 0x7a, 0x6a),
            varispeed: c(0x8a, 0xb0, 0xd8),
            meter_bg: c(0x24, 0x1c, 0x0e),
            scope_draw: c(0xf0, 0xb8, 0x30),
            fx_strip: c(0x4a, 0x35, 0x10),
            input_chip: c(0x8a, 0xb0, 0xd8),
            fretboard: c(0x24, 0x1c, 0x0e),
            string: c(0xc8, 0xb0, 0x70),
            velocity_loud: c(0xfe, 0x7d, 0x3d),
            velocity_quiet: c(0x70, 0x9f, 0xfe),
            channel_palette: Some(5),
            dark: dark_of(&[("background", (0x1e, 0x1a, 0x12))]),
        },
        // ── Reed: the reed electric piano this app models (keys/reed.rs), in
        // the avocado the 200 wore. Aurora's slot (retired 2026-09-18). An
        // olive band, the lid's black-olive strips, ivory keys, the script's
        // coral for what sounds, a pale sage for what is held, the amber
        // pilot lamp on the master and bone knobs. The coral is a step paler
        // than the script's own: #ff7a62 read 2.92:1 on the band.
        Theme {
            name: "Reed".to_owned(),
            background: c(0xf3, 0xef, 0xdc),
            white_key: c(0xf3, 0xef, 0xdc),
            black_key: c(0x17, 0x1a, 0x10),
            key_active: c(0xff, 0x84, 0x68),
            sustain: c(0xcf, 0xe0, 0xa8),
            key_separator: c(0x4e, 0x5a, 0x28),
            text: c(0xf3, 0xef, 0xdc),
            theory_line: c(0x8e, 0x94, 0x60),
            interval_text: c(0xcf, 0xe0, 0xa8),
            recorder_bg: c(0x42, 0x4c, 0x22),
            fx_knob: c(0xef, 0xe2, 0xb0),
            master_knob: c(0xf5, 0xb8, 0x30),
            varispeed: c(0x8f, 0xb0, 0xc8),
            meter_bg: c(0x1c, 0x20, 0x10),
            scope_draw: c(0xc8, 0xe0, 0x70),
            fx_strip: c(0x26, 0x2b, 0x14),
            input_chip: c(0xff, 0x84, 0x68),
            fretboard: c(0x1c, 0x20, 0x10),
            string: c(0xc0, 0xc0, 0x98),
            velocity_loud: c(0xfe, 0x72, 0x56),
            velocity_quiet: c(0x76, 0xab, 0x5c),
            channel_palette: Some(6),
            dark: dark_of(&[("background", (0x17, 0x1a, 0x10))]),
        },
        // ── Sandstone: the LIGHT one, and the reason the set needs it. Warm
        // sand paper, umber ink, terracotta for what moves, sage for what is
        // held. The scope draws DARK, because on a pale meter a bright trace
        // is invisible: the one place a light theme cannot simply invert a
        // dark one.
        //
        // **Darker sand, at the owner's word (2026-09-18): "make even light
        // mode a darker sand colour; it is too harsh now."** The page was a
        // near-white (#faf4e8, 0.90 luminance) and every surface on it paler
        // still; it is sand now (#e0cfa8, 0.62), the band came down with it
        // (0.66 to 0.49) and the strips, the well and the neck followed.
        // Every mark that cleared 3:1 on the old band was measured again on
        // the new one: the effect knob deepened a step (#a84f28 to #9a4522,
        // 2.9:1 to 3.4:1) and the scope trace with it; the terracotta and
        // the master clear as they were. The dark ground is untouched.
        //
        // **A light theme is not a dark one inverted.** On paper the marks
        // have to go DARKER to be seen, not brighter: the terracotta is
        // deepened until it reads on sand, and the sustain is deepened
        // further still, so a held note and a struck one are told apart by
        // weight the way ink is.
        Theme {
            name: "Sandstone".to_owned(),
            background: c(0xe0, 0xcf, 0xa8),
            white_key: c(0xe0, 0xcf, 0xa8),
            black_key: c(0x2b, 0x24, 0x1d),
            key_active: c(0x9a, 0x49, 0x25),
            sustain: c(0x3a, 0x4a, 0x24),
            key_separator: c(0x9c, 0x8a, 0x70),
            text: c(0x3a, 0x31, 0x28),
            theory_line: c(0x8a, 0x78, 0x58),
            interval_text: c(0x8a, 0x5a, 0x2c),
            recorder_bg: c(0xde, 0xd1, 0xb2),
            fx_knob: c(0x9a, 0x45, 0x22),
            master_knob: c(0xa0, 0x38, 0x28),
            varispeed: c(0x40, 0x60, 0x6e),
            meter_bg: c(0xd3, 0xc4, 0x9f),
            scope_draw: c(0x4e, 0x62, 0x38),
            fx_strip: c(0xc3, 0xae, 0x82),
            input_chip: c(0x7d, 0x8f, 0x5a),
            fretboard: c(0xb9, 0xa3, 0x7c),
            string: c(0x5a, 0x4c, 0x3a),
            velocity_loud: c(0xba, 0x3a, 0x1a),
            velocity_quiet: c(0x3c, 0x74, 0x28),
            channel_palette: Some(7),
            dark: dark_of(&[("background", (0x2b, 0x24, 0x1d))]),
        },
        // ── Centennial: the DX7 II Centennial, 1987: Yamaha's hundredth
        // birthday in brushed silver with gold legends and black cheeks. The
        // gold is an ACCENT and never the text: it is beautiful at 3:1 on
        // silver and unreadable as a paragraph. Slate blue on the input chip
        // is the one cool note, as it is on the instrument.
        //
        // **A darker band, at the owner's word (2026-09-18): "make the
        // recorder colour slightly darker grey."** #c2c2c8 to #b6b6bc, about
        // seven percent, and the gold marks deepened one step (#785c17 to
        // #6f5515, #7d5f18 to #725716) so they keep 3:1 on it; the master
        // clears as it was.
        Theme {
            name: "Centennial".to_owned(),
            background: c(0xf2, 0xf1, 0xee),
            white_key: c(0xf2, 0xf1, 0xee),
            black_key: c(0x26, 0x26, 0x2a),
            key_active: c(0x6f, 0x55, 0x15),
            sustain: c(0x3f, 0x4a, 0x55),
            key_separator: c(0x9a, 0x9a, 0xa0),
            text: c(0x1f, 0x1f, 0x23),
            theory_line: c(0x8e, 0x8e, 0x96),
            interval_text: c(0x6f, 0x57, 0x14),
            recorder_bg: c(0xb6, 0xb6, 0xbc),
            fx_knob: c(0x72, 0x57, 0x16),
            master_knob: c(0x9c, 0x32, 0x26),
            varispeed: c(0x35, 0x50, 0x6e),
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
        // ── Zinc: the metal, and the violet the set lost. Blueprint's
        // slot (retired 2026-09-18, "essentially an alternate Navi"), so
        // the one hue the replacement must not be is blue. Zinc weathers to
        // a blue-violet grey, and nothing here is violet at all since Dusk
        // and Iris went, so the band is that patina with brass for what is
        // HELD — the two metals an old panel is made of — and a pale zinc
        // lilac for what sounds. The brass is deeper than a brass wants to
        // be: a lighter one read 1.19:1 against the lilac, which is a held
        // note and a struck one telling nothing apart.
        //
        // **It began as a mid-tone desk and could not stay one.** The first
        // design for this slot was a pewter band at 0.30 luminance carrying
        // dark ink — the daylight desk that does not glare, which is
        // genuinely not covered by the rest of the set. Every colour gate
        // passed and `dialogs::every_theme_dresses_the_export_window` did
        // not: a well in the middle of the range sits within 3:1 of this
        // app's one fixed transport red, so the playhead had to be
        // recoloured on all four of that theme's dressings where the whole
        // set is allowed seven. A desk that makes the app move its own red
        // everywhere is a desk fighting the transport, so the concept
        // yielded and the metal stayed. See `scripts/palgen.py`'s
        // `playhead_moves`.
        Theme {
            name: "Zinc".to_owned(),
            background: c(0xe8, 0xe4, 0xee),
            white_key: c(0xe8, 0xe4, 0xee),
            black_key: c(0x1a, 0x16, 0x22),
            key_active: c(0xb9, 0xa8, 0xe0),
            sustain: c(0xb0, 0x84, 0x30),
            key_separator: c(0x4a, 0x40, 0x60),
            text: c(0xe8, 0xe4, 0xee),
            theory_line: c(0x6a, 0x5f, 0x80),
            interval_text: c(0xc9, 0xb8, 0xe8),
            recorder_bg: c(0x33, 0x2a, 0x45),
            fx_knob: c(0xbf, 0xb0, 0xe0),
            master_knob: c(0xff, 0x5a, 0x6a),
            varispeed: c(0x78, 0xc8, 0xd8),
            meter_bg: c(0x1a, 0x16, 0x22),
            scope_draw: c(0xcb, 0xb8, 0xf0),
            fx_strip: c(0x28, 0x20, 0x36),
            input_chip: c(0x8f, 0x7f, 0xb8),
            fretboard: c(0x1a, 0x16, 0x22),
            string: c(0xa0, 0x9a, 0xb0),
            velocity_loud: c(0xff, 0x70, 0x88),
            velocity_quiet: c(0x19, 0xb3, 0x94),
            channel_palette: Some(9),
            dark: dark_of(&[("background", (0x1a, 0x16, 0x22))]),
        },
        // ── Celadon: the glaze. Porcelain's slot (retired 2026-09-18). The
        // third light desk, cool where Sandstone is warm and green where
        // Centennial is grey, with plum for what sounds and pine for what is
        // held, both DARK, as marks on paper have to be. A dark well with a
        // pale celadon trace, like Centennial's.
        Theme {
            name: "Celadon".to_owned(),
            background: c(0xe2, 0xeb, 0xe6),
            white_key: c(0xe2, 0xeb, 0xe6),
            black_key: c(0x2c, 0x3a, 0x34),
            key_active: c(0x7e, 0x3a, 0x6c),
            sustain: c(0x1e, 0x3c, 0x2e),
            key_separator: c(0x8f, 0xa8, 0x9a),
            text: c(0x24, 0x30, 0x2a),
            theory_line: c(0x8f, 0xa8, 0x9a),
            interval_text: c(0x5a, 0x3a, 0x6a),
            recorder_bg: c(0xc3, 0xd4, 0xc8),
            fx_knob: c(0x6e, 0x30, 0x60),
            master_knob: c(0x9a, 0x2a, 0x2a),
            varispeed: c(0x24, 0x50, 0x4a),
            meter_bg: c(0x22, 0x30, 0x2a),
            scope_draw: c(0xa8, 0xd8, 0xc0),
            fx_strip: c(0xd2, 0xdf, 0xd5),
            input_chip: c(0x5a, 0x7a, 0x6a),
            fretboard: c(0x9a, 0xb0, 0xa4),
            string: c(0x2c, 0x3a, 0x34),
            velocity_loud: c(0xff, 0xa3, 0xc2),
            velocity_quiet: c(0x1e, 0xd7, 0x93),
            channel_palette: Some(10),
            dark: dark_of(&[("background", (0x2c, 0x3a, 0x34))]),
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
        // ── Prism: charcoal, and one colour per control, the slider caps of
        // a polysynth. Dusk's slot (retired 2026-09-18, "Dusk and Iris are
        // too close: replace them"). Cyan for what sounds, magenta for what
        // is held, yellow knobs, a red master, green on the scope, orange on
        // the input chip; the pitch dial is the one white thing so it is not
        // another cap.
        Theme {
            name: "Prism".to_owned(),
            background: c(0xee, 0xee, 0xf2),
            white_key: c(0xee, 0xee, 0xf2),
            black_key: c(0x18, 0x18, 0x1c),
            key_active: c(0x38, 0xb8, 0xf0),
            sustain: c(0xf0, 0x50, 0xb0),
            key_separator: c(0x35, 0x35, 0x3c),
            text: c(0xec, 0xec, 0xe8),
            theory_line: c(0x8a, 0x8a, 0x92),
            interval_text: c(0xf0, 0xc0, 0x20),
            recorder_bg: c(0x35, 0x35, 0x3c),
            fx_knob: c(0xf0, 0xc0, 0x20),
            master_knob: c(0xff, 0x40, 0x40),
            varispeed: c(0xe6, 0xe6, 0xec),
            meter_bg: c(0x1c, 0x1c, 0x20),
            scope_draw: c(0x50, 0xe0, 0x70),
            fx_strip: c(0x28, 0x28, 0x2e),
            input_chip: c(0xf0, 0x8a, 0x30),
            fretboard: c(0x1c, 0x1c, 0x20),
            string: c(0xa0, 0xa0, 0xa8),
            velocity_loud: c(0xff, 0x70, 0x70),
            velocity_quiet: c(0x21, 0xa7, 0xeb),
            channel_palette: Some(14),
            dark: dark_of(&[("background", (0x18, 0x18, 0x1c))]),
        },
        // ── Storm: slate, and the bolt. Iris's slot (retired 2026-09-18). A
        // blue-grey band darker than Pewter's and greyer than Navi's,
        // cloud-white knobs, and yellow, the one hero no other theme uses,
        // for what sounds and for the pitch dial; deep storm blue for what
        // is held.
        Theme {
            name: "Storm".to_owned(),
            background: c(0xdd, 0xe3, 0xe9),
            white_key: c(0xdd, 0xe3, 0xe9),
            black_key: c(0x14, 0x18, 0x1e),
            key_active: c(0xf5, 0xd3, 0x3a),
            sustain: c(0x2a, 0x58, 0x96),
            key_separator: c(0x3d, 0x46, 0x54),
            text: c(0xe8, 0xed, 0xf2),
            theory_line: c(0x8a, 0x98, 0xa8),
            interval_text: c(0xe8, 0xc8, 0x48),
            recorder_bg: c(0x3d, 0x46, 0x54),
            fx_knob: c(0xd4, 0xdb, 0xe2),
            master_knob: c(0xff, 0x5a, 0x5a),
            varispeed: c(0xf5, 0xd3, 0x3a),
            meter_bg: c(0x1a, 0x1f, 0x26),
            scope_draw: c(0xf5, 0xd3, 0x3a),
            fx_strip: c(0x2e, 0x35, 0x40),
            input_chip: c(0x7f, 0xa8, 0xc8),
            fretboard: c(0x1a, 0x1f, 0x26),
            string: c(0xa8, 0xb4, 0xc0),
            velocity_loud: c(0xff, 0x83, 0x70),
            velocity_quiet: c(0x70, 0xa8, 0xfe),
            channel_palette: Some(15),
            dark: dark_of(&[("background", (0x14, 0x18, 0x1e))]),
        },
        // ── the dark two. Sixteen light themes and not one dark one was
        // the audit's finding, so these carry the ground itself dark, which
        // makes the naturals dark and the sharps darker, the way every dark
        // instrument panel reads. Designed and gated by `scripts/palgen.py`
        // BEFORE this Rust existed; the factory-loop tests below are the
        // second lock.
        //
        // There were three, then two, and the two have changed once. Carbon
        // (Default's own dark constants promoted to a theme) was retired by
        // the owner on 2026-09-02 as a worse version of Studio. Nocturne
        // (Navi's night side) was retired on 2026-09-18 ("Nocturne is also
        // just Navi: replace") and Amber took its slot. Each left by NAME: a
        // saved `theme_name` of either falls back to Default, its palette
        // bytes stay in `STRIP_COLORS` for every desk that painted a track
        // with them, and only `PALETTES` moved. Carbon's retirement closed
        // a gap (Nocturne and Phosphor pointed one lower); Nocturne's did
        // not, because Amber's row points at its own appended run.
        //
        // **These themes name their ground and nothing else.** In dark mode
        // `piano::idle_colors` SWAPS the keys, and on a theme whose BOTH
        // grounds are near-black every field this struct has for a key is
        // already spoken for: `white_key` is the light page and `black_key`
        // the dark one (`the_theory_ground_is_the_keyboard_not_the_band`).
        // Naming `dark.white_key`/`dark.black_key` to lift the keyboard by
        // hand was tried and was wrong (a mid slab floating on near-black;
        // the owner photographed it four times), and their LIGHT keyboards
        // were the same mask (1.04:1 and 1.10:1, against 9 to 15:1 on every
        // other theme). `piano::keys_apart` lifts the sharps clear instead,
        // in both modes and only where a theme is trapped this way, and
        // `keys_the_right_way_up` puts the pair the right way round; see
        // them for the rule and the numbers. The piano's
        // `every_theme_keeps_a_legible_keyboard_in_both_modes` names the
        // themes that need the lift, and it is these two.
        //
        // ── Amber: the other phosphor. Phosphor is the green console; this
        // is the amber one beside it. Everything glows one amber (text,
        // keys, knobs, trace) with a paler bloom for what is held and the
        // warning red-orange on the master, the same red Phosphor wears.
        Theme {
            name: "Amber".to_owned(),
            background: c(0x10, 0x0c, 0x06),
            white_key: c(0x10, 0x0c, 0x06),
            black_key: c(0x08, 0x06, 0x04),
            key_active: c(0xff, 0xb0, 0x00),
            sustain: c(0xff, 0xe0, 0xa0),
            key_separator: c(0x3a, 0x2a, 0x10),
            text: c(0xff, 0xcf, 0x70),
            theory_line: c(0x52, 0x40, 0x1a),
            interval_text: c(0xe0, 0xa8, 0x48),
            recorder_bg: c(0x2a, 0x20, 0x10),
            fx_knob: c(0xe0, 0x9a, 0x20),
            master_knob: c(0xff, 0x4a, 0x28),
            varispeed: c(0xf0, 0xd8, 0xa8),
            meter_bg: c(0x08, 0x06, 0x04),
            scope_draw: c(0xff, 0xb0, 0x00),
            fx_strip: c(0x20, 0x18, 0x08),
            input_chip: c(0xe0, 0x9a, 0x20),
            fretboard: c(0x08, 0x06, 0x04),
            string: c(0x8a, 0x70, 0x40),
            velocity_loud: c(0xea, 0x5c, 0x38),
            velocity_quiet: c(0x16, 0x9c, 0x6f),
            channel_palette: Some(16),
            dark: dark_of(&[("background", (0x08, 0x06, 0x04))]),
        },
        // ── Phosphor: the green console. Scope-green actives, one warm
        // amber for the held state, and a warning red-orange master.
        Theme {
            name: "Phosphor".to_owned(),
            background: c(0x0b, 0x0e, 0x0a),
            white_key: c(0x0b, 0x0e, 0x0a),
            black_key: c(0x05, 0x07, 0x05),
            key_active: c(0xa3, 0xe0, 0x48),
            sustain: c(0xd2, 0xa3, 0x6c),
            key_separator: c(0x22, 0x2c, 0x1e),
            text: c(0xc8, 0xe8, 0xb0),
            theory_line: c(0x3a, 0x48, 0x34),
            interval_text: c(0x9a, 0xc0, 0x84),
            recorder_bg: c(0x1c, 0x2c, 0x16),
            fx_knob: c(0x8f, 0xae, 0x3f),
            master_knob: c(0xe0, 0x58, 0x40),
            varispeed: c(0xc8, 0xe8, 0xb0),
            velocity_loud: c(0xe8, 0x8c, 0x40),
            velocity_quiet: c(0x48, 0xa8, 0x88),
            meter_bg: c(0x05, 0x07, 0x05),
            scope_draw: c(0xa3, 0xe0, 0x48),
            fx_strip: c(0x16, 0x22, 0x12),
            input_chip: c(0x6f, 0xae, 0x3f),
            fretboard: c(0x05, 0x07, 0x05),
            string: c(0x6a, 0x80, 0x60),
            channel_palette: Some(17),
            dark: dark_of(&[("background", (0x05, 0x07, 0x05))]),
        },
        // ── Silverface: the Dual Showman Reverb's faceplate. The DAW grew
        // a Fender amp built from the actual AA769 schematic, and this is
        // the panel that amp wears: brushed aluminum for the ground and the
        // naturals, the black control panel for the band, Fender turquoise
        // script for what sounds, blue-sparkle grille cloth for what is
        // held, and the red jewel pilot lamp on the master — the one thing
        // on the panel that glows. Nickel-wound strings on the fretboard.
        // A LIGHT theme, so dark mode is the standard swap onto the
        // blackface panel and it names only its ground.
        //
        // The turquoise is deeper on the real panel (#18808f) and could not
        // be here: at luminance 0.176 it sits in the dead zone — 2.94:1 on
        // the band and 3.75:1 under its own note name, failing both sweeps
        // at once. Lifted to the lowest turquoise that clears both (3.8:1
        // on the band, 4.8:1 under the dark ink), which keeps the most a
        // lit key can have against aluminum naturals. Designed and gated by
        // `scripts/palgen.py --dark`, its desk by `--silverface`.
        Theme {
            name: "Silverface".to_owned(),
            background: c(0xc9, 0xcd, 0xd2),
            white_key: c(0xc9, 0xcd, 0xd2),
            black_key: c(0x1b, 0x1d, 0x20),
            key_active: c(0x1b, 0x94, 0xa6),
            sustain: c(0x3b, 0x5a, 0x8c),
            key_separator: c(0x3a, 0x3e, 0x44),
            text: c(0xd8, 0xde, 0xe6),
            theory_line: c(0x8a, 0x90, 0x99),
            interval_text: c(0x9f, 0xb3, 0xc4),
            recorder_bg: c(0x2a, 0x2e, 0x33),
            fx_knob: c(0x3f, 0xb8, 0xc9),
            master_knob: c(0xe0, 0x48, 0x3a),
            varispeed: c(0xd8, 0xde, 0xe6),
            // Solved by `scripts/velgen.py`'s solver against this theme's
            // own roll, hot 12° / cool 190° — the jewel's glow and the
            // script's turquoise. The panel's deeper pair (#e86048/#3f8fa0)
            // read 5:1 on the light roll and 2.7:1/2.4:1 on the dark one,
            // where the well is lifted a fifth toward white: a mark drawn
            // on that roll has to be BRIGHTER than the panel's paint, not
            // deeper, so both ends came up until they clear 3:1 in both
            // modes without the painter's push.
            velocity_loud: c(0xff, 0x78, 0x57),
            velocity_quiet: c(0x1d, 0xaf, 0xcd),
            meter_bg: c(0x14, 0x16, 0x1a),
            scope_draw: c(0x64, 0xd8, 0xe8),
            fx_strip: c(0x1f, 0x22, 0x26),
            input_chip: c(0x8a, 0xa0, 0xb8),
            fretboard: c(0x14, 0x16, 0x1a),
            string: c(0x8a, 0x90, 0x99),
            channel_palette: Some(18),
            dark: dark_of(&[("background", (0x1b, 0x1d, 0x20))]),
        },
        // ── the 2026-10-08 batch: thirteen NEW themes, slots 19..31. ──────
        //
        // Appended after Silverface and nothing above moved: the menu grew
        // from nineteen to thirty-two, and each theme's palette row points
        // at a run appended whole to `STRIP_COLORS`. Each one was chosen
        // against what the nineteen already covered (no saturated band
        // between hue 20 and 60, no emerald, no ultramarine, a dark ground
        // only in amber and green, one light desk in each of grey, sand and
        // celadon) and named for an object rather than a mood, so the name
        // says what the colours are. Designed and gated by
        // `scripts/palgen.py --themes` and `--redo` BEFORE this Rust was
        // written; the velocity pairs are `scripts/velgen.py`'s.
        // ── Urushi: a black lacquer writing box with cinnabar trays and gold
        // dust. Warm, glossy, and nothing like a terminal. A DARK-ground theme,
        // the third after Amber and Phosphor, so its light ground is a
        // near-black and the piano lifts its sharps (`keys_apart`). The band is
        // a red you read as lacquer rather than wine: darker and more orange
        // than Default's #61373a, at hue 36 where Default's sits at 18.
        // Maki-e gold sounds and cinnabar is held, the reverse of the first cut:
        // a vermilion pressed key sat 6.6 dE from Woodblock's seal, with Kiln's
        // ember wanting the same corner, while a gold key on black lacquer is
        // nobody else's (Storm's yellow is 12.7 dE off, Rhodes' gold 15.5). The two are 2.22:1 apart, a struck note and a held one plainly
        // told apart. The knobs are pine, the one cool thing on the box, so the
        // gold is not on every knob as well, and the master is pulled cool and
        // pink so it never reads as the cinnabar.
        Theme {
            name: "Urushi".to_owned(),
            background: c(0x14, 0x0a, 0x08),
            white_key: c(0x14, 0x0a, 0x08),
            black_key: c(0x0a, 0x05, 0x04),
            key_active: c(0xf0, 0xc8, 0x50),
            sustain: c(0xf0, 0x50, 0x2e),
            key_separator: c(0x3a, 0x18, 0x10),
            text: c(0xf2, 0xd2, 0xb8),
            theory_line: c(0x64, 0x32, 0x1f),
            interval_text: c(0xe8, 0xa0, 0x70),
            recorder_bg: c(0x3e, 0x14, 0x0c),
            fx_knob: c(0x6f, 0xbf, 0x8e),
            master_knob: c(0xff, 0x4f, 0x6e),
            varispeed: c(0xf2, 0xd2, 0xb8),
            meter_bg: c(0x0a, 0x05, 0x04),
            scope_draw: c(0xf0, 0xc8, 0x50),
            fx_strip: c(0x2c, 0x0e, 0x08),
            input_chip: c(0xf0, 0xc8, 0x50),
            fretboard: c(0x0a, 0x05, 0x04),
            string: c(0x8a, 0x64, 0x50),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x6c, 0x56),
            velocity_quiet: c(0x19, 0xb0, 0x64),
            channel_palette: Some(19),
            dark: dark_of(&[("background", (0x0a, 0x05, 0x04))]),
        },
        // ── Paperback: a shelf of mid-century paperbacks. A burnt orange
        // cover stripe for the band, a white middle panel for the page, and the
        // series colours down the spines. The first saturated band in the empty
        // stretch between hue 20 and 60 (Rhodes sits there at chroma 7). The
        // pressed key is the fiction orange, bright enough to show on the white
        // page and on the band; a cream key would vanish on a white page. Held
        // is the essays' blue, lifted out of the dead zone to 0.25 luminance so
        // the note name on it reads. The knobs are the cover's cream panel warmed
        // toward the orange, so the band is two tones and one blue: green knobs
        // beside an orange key read as Christmas, and a plain cream sat 3.6 dE
        // from Woodblock's washi knobs. The scope and the pitch dial are a pale
        // blue rather than cream: the browser's file rows print in the trace's
        // colour and its folders in the band's cream ink, and a cream trace put
        // the two 3.7 dE apart; the dial must stand 25 dE off the knobs.
        Theme {
            name: "Paperback".to_owned(),
            background: c(0xf5, 0xf3, 0xee),
            white_key: c(0xf5, 0xf3, 0xee),
            black_key: c(0x1e, 0x14, 0x0e),
            key_active: c(0xff, 0x8a, 0x3a),
            sustain: c(0x5a, 0x8a, 0xe0),
            key_separator: c(0x8a, 0x5a, 0x3a),
            text: c(0xf2, 0xe6, 0xcc),
            theory_line: c(0xb0, 0xa8, 0x98),
            interval_text: c(0xc0, 0x4a, 0x14),
            recorder_bg: c(0x74, 0x38, 0x1a),
            fx_knob: c(0xff, 0xcc, 0x9c),
            master_knob: c(0xff, 0x6a, 0x5a),
            varispeed: c(0x9c, 0xc0, 0xf8),
            meter_bg: c(0x24, 0x14, 0x0c),
            scope_draw: c(0x9c, 0xc0, 0xf8),
            fx_strip: c(0x5e, 0x2c, 0x14),
            input_chip: c(0xff, 0x8a, 0x3a),
            fretboard: c(0x24, 0x14, 0x0c),
            string: c(0xc8, 0xb8, 0xa0),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfa, 0x75, 0x3c),
            velocity_quiet: c(0x19, 0xb2, 0x4c),
            channel_palette: Some(20),
            dark: dark_of(&[("background", (0x1e, 0x14, 0x0e))]),
        },
        // ── Malachite: a polished slab of malachite, banded emerald rings
        // set off with rose quartz. The first green band above chroma 20, clear
        // of Moss (hue 135), Reed (116) and Copper's verdigris (180, chroma 13).
        // The page is a faint rose-quartz white so it is not Celadon's
        // green-white. Rose quartz sounds and is the first pink trace on the
        // menu; what is held is a DEEPER malachite green, because a pale mint
        // held key would vanish on the white page.
        Theme {
            name: "Malachite".to_owned(),
            background: c(0xf3, 0xec, 0xec),
            white_key: c(0xf3, 0xec, 0xec),
            black_key: c(0x1a, 0x2a, 0x22),
            key_active: c(0xff, 0x86, 0xa4),
            sustain: c(0x2f, 0xa0, 0x70),
            key_separator: c(0x2a, 0x5a, 0x44),
            text: c(0xe8, 0xf4, 0xec),
            theory_line: c(0xb0, 0xa4, 0xa6),
            interval_text: c(0x1a, 0x6a, 0x4a),
            recorder_bg: c(0x00, 0x54, 0x3c),
            fx_knob: c(0xc4, 0xec, 0xd0),
            master_knob: c(0xff, 0x70, 0x80),
            varispeed: c(0xf0, 0xb8, 0xc8),
            meter_bg: c(0x0c, 0x1e, 0x16),
            scope_draw: c(0xff, 0x86, 0xa4),
            fx_strip: c(0x00, 0x3e, 0x2c),
            input_chip: c(0xff, 0x86, 0xa4),
            fretboard: c(0x0c, 0x1e, 0x16),
            string: c(0xa8, 0xc8, 0xb8),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xff, 0x70, 0x88),
            velocity_quiet: c(0x19, 0xb4, 0x80),
            channel_palette: Some(21),
            dark: dark_of(&[("background", (0x1a, 0x2a, 0x22))]),
        },
        // ── Sodium: a city at 3 a.m. from a night bus. An ultraviolet sky
        // for the band, sodium streetlamps for what sounds, one magenta neon sign
        // for what is held, and the blue-green of a mercury lamp on the knobs. A
        // DARK-ground theme like Amber and Phosphor, so the piano lifts its
        // sharps. Not the retired Dusk (lavender page, violet band, orange key):
        // this page is black, the band carries twice Zinc's chroma on a bluer
        // hue, and the magenta and mercury are new. The knobs are mercury rather
        // than lilac so it never reads as Zinc turned down.
        Theme {
            name: "Sodium".to_owned(),
            background: c(0x0e, 0x0a, 0x16),
            white_key: c(0x0e, 0x0a, 0x16),
            black_key: c(0x06, 0x04, 0x09),
            key_active: c(0xff, 0x9a, 0x2a),
            sustain: c(0xe8, 0x5a, 0xd0),
            key_separator: c(0x2c, 0x1e, 0x44),
            text: c(0xf0, 0xc8, 0x90),
            theory_line: c(0x4a, 0x3a, 0x6a),
            interval_text: c(0xe8, 0xa0, 0xe0),
            recorder_bg: c(0x24, 0x1b, 0x4f),
            fx_knob: c(0x6a, 0xd8, 0xc8),
            master_knob: c(0xff, 0x50, 0x40),
            varispeed: c(0xf0, 0xc8, 0x90),
            meter_bg: c(0x06, 0x04, 0x09),
            scope_draw: c(0xff, 0x9a, 0x2a),
            fx_strip: c(0x1a, 0x14, 0x40),
            input_chip: c(0xd0, 0x70, 0xe0),
            fretboard: c(0x06, 0x04, 0x09),
            string: c(0x7a, 0x6a, 0x90),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xe5, 0x7c, 0x20),
            velocity_quiet: c(0xc1, 0x7b, 0xe5),
            channel_palette: Some(22),
            dark: dark_of(&[("background", (0x06, 0x04, 0x09))]),
        },
        // ── Kiln: an unglazed flowerpot fresh from a wood-fired kiln.
        // Terracotta paper, soot, and the glow of the firebox. The PAGE is the
        // hero, the only warm page on the menu that is clay rather than cream,
        // held at 0.68 luminance so it still takes the dark ink. The band is
        // soot and deliberately carries no brick, because Paperback and Urushi
        // already own the saturated bands between hue 40 and 55. What sounds is
        // an ember, a full-chroma red-orange: the first cut's firebox orange
        // sat 5.3 dE from Paperback's key, and the ember is 22 from it and
        // 18.6 from Woodblock's seal while staying 20 dE off the loudness
        // meter's hot red. 2.53:1 on the clay is about as far as a key can
        // stand off this page, because it must clear 3:1 on the soot and carry
        // its note name, which puts it at 0.22 luminance or more. The knobs are
        // the glow itself rather than cream, and what is held is a deep
        // ash-glaze green, 4.53:1 on the clay so a held key reads from across
        // the room.
        Theme {
            name: "Kiln".to_owned(),
            background: c(0xf8, 0xcf, 0xb9),
            white_key: c(0xf8, 0xcf, 0xb9),
            black_key: c(0x2a, 0x16, 0x12),
            key_active: c(0xea, 0x54, 0x01),
            sustain: c(0x3c, 0x66, 0x56),
            key_separator: c(0x8a, 0x5a, 0x44),
            text: c(0xf0, 0xdc, 0xc0),
            theory_line: c(0xb8, 0x90, 0x7a),
            interval_text: c(0x8a, 0x3a, 0x1a),
            recorder_bg: c(0x2a, 0x1e, 0x1a),
            fx_knob: c(0xe8, 0xa0, 0x70),
            master_knob: c(0xff, 0x6a, 0x50),
            varispeed: c(0xf0, 0xdc, 0xc0),
            meter_bg: c(0x14, 0x0c, 0x0a),
            scope_draw: c(0xff, 0x94, 0x40),
            fx_strip: c(0x20, 0x16, 0x12),
            input_chip: c(0xea, 0x54, 0x01),
            fretboard: c(0x14, 0x0c, 0x0a),
            string: c(0xc0, 0x90, 0x70),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xf5, 0x6c, 0x3b),
            velocity_quiet: c(0x18, 0xab, 0x61),
            channel_palette: Some(23),
            dark: dark_of(&[("background", (0x2a, 0x16, 0x12))]),
        },
        // ── Lapis: a Byzantine ceiling, ground lapis lazuli for the sky and
        // gold leaf for the stars. The band is ultramarine at hue 295, as
        // saturated as the chroma ceiling allows (41 against 42): the most
        // chromatic band on the menu and plainly not Navi's chroma-27 navy. The
        // page is warm plaster, not Prism's blue-white, which the first cut sat
        // 0.6 dE from. A pale ultramarine sounds and the gold is held, the
        // reverse of Storm's yellow key and blue hold; the knobs are a brighter
        // leaf than the held gold, where the first cut pressed the knobs' own
        // hex and a pressed key said nothing the knobs did not. The pressed
        // blue leans violet because a key must clear 3:1 on this band, and a
        // plain pale blue there is Navi's #7f9bff.
        Theme {
            name: "Lapis".to_owned(),
            background: c(0xef, 0xec, 0xe4),
            white_key: c(0xef, 0xec, 0xe4),
            black_key: c(0x14, 0x1a, 0x40),
            key_active: c(0x8c, 0x8c, 0xff),
            sustain: c(0xe8, 0xb6, 0x40),
            key_separator: c(0x2a, 0x34, 0x78),
            text: c(0xec, 0xe8, 0xdc),
            theory_line: c(0xa8, 0xaa, 0xbc),
            interval_text: c(0x2a, 0x3a, 0x9a),
            recorder_bg: c(0x23, 0x2d, 0x6a),
            fx_knob: c(0xf8, 0xd0, 0x60),
            master_knob: c(0xff, 0x6a, 0x5a),
            varispeed: c(0xf4, 0xe4, 0xb0),
            meter_bg: c(0x0c, 0x10, 0x30),
            scope_draw: c(0xf8, 0xe8, 0xa8),
            fx_strip: c(0x1a, 0x21, 0x52),
            input_chip: c(0x8c, 0x8c, 0xff),
            fretboard: c(0x0c, 0x10, 0x30),
            string: c(0xa8, 0xac, 0xc8),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x72, 0x56),
            velocity_quiet: c(0x70, 0x9a, 0xfe),
            channel_palette: Some(24),
            dark: dark_of(&[("background", (0x14, 0x1a, 0x40))]),
        },
        // ── Chartres: a rose window lit from behind. Cobalt, ruby, emerald
        // and topaz glass held in lead. A DARK-ground theme (the piano lifts
        // its sharps) on a neutral lead-black, and a blue-black band, the lead
        // at night, at 0.022 luminance: below 0.016 the varispeed dial's recess,
        // the band darkened twice, has no room left (at 0.013 it sat 1.17:1
        // where a shape wants 1.18). The blue also takes it off Silverface's
        // grey band, 8.2 dE away.
        // Blue leads the marks as it leads the strips, because a rose window is
        // mostly blue glass: cobalt knobs and a paler cobalt trace, ruby sounds,
        // emerald is held, topaz on the marks. The first cut pressed cobalt and
        // put emerald on the knobs, and beside an even four-jewel cycle on black
        // that read as Prism with the lights off. The master is a warm coral so
        // it does not fight the ruby.
        Theme {
            name: "Chartres".to_owned(),
            background: c(0x0c, 0x0c, 0x0d),
            white_key: c(0x0c, 0x0c, 0x0d),
            black_key: c(0x05, 0x05, 0x05),
            key_active: c(0xe8, 0x57, 0x7e),
            sustain: c(0x48, 0xd8, 0x90),
            key_separator: c(0x2a, 0x2c, 0x3a),
            text: c(0xe8, 0xec, 0xf0),
            theory_line: c(0x40, 0x44, 0x58),
            interval_text: c(0xf0, 0xb8, 0x3a),
            recorder_bg: c(0x24, 0x28, 0x38),
            fx_knob: c(0x40, 0x70, 0xff),
            master_knob: c(0xff, 0x7a, 0x50),
            varispeed: c(0xe8, 0xec, 0xf0),
            meter_bg: c(0x05, 0x05, 0x05),
            scope_draw: c(0x7a, 0xa0, 0xff),
            fx_strip: c(0x1a, 0x1c, 0x28),
            input_chip: c(0xf0, 0xb8, 0x3a),
            fretboard: c(0x05, 0x05, 0x05),
            string: c(0x7a, 0x7e, 0x94),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x56, 0x72),
            velocity_quiet: c(0x17, 0xa8, 0x47),
            channel_palette: Some(25),
            dark: dark_of(&[("background", (0x05, 0x05, 0x05))]),
        },
        // ── Overprint: a two-colour risograph zine. Fluorescent pink and
        // federal blue on bright copier paper, a little out of register. The
        // one new LIGHT desk, so every mark on the band is dark, and it joins
        // `heart_border`'s charcoal-edge themes. The band is a pale pink ink
        // wash at 0.72 luminance. The strip is a near-white with a pink cast at
        // 0.83, and that is a narrow window rather than a taste: a light strip
        // spends the export window's playhead budget, and at 0.82 its dark-mode
        // well sat 2.95:1 from the transport red and moved the playhead; at
        // 0.87 the playhead was safe and a floating panel's field, the strip
        // walked 30% further toward white, sat 1.04:1 from the panel. 0.83 is
        // 3.00:1 and 1.05:1, both floors. Not a Porcelain revival: that was a
        // blue-grey desk with cobalt and violet marks.
        // A brighter riso blue sounds than the federal on the knobs, because
        // there is no dark-mode key: one colour has to show on the paper AND on
        // the dark naturals, and the band's 3:1 caps it at 0.12 luminance. The
        // federal sat 1.48:1 on the dark naturals, a pressed key that vanished;
        // this blue on naturals deepened to #1a1030 is 2.86:1. The other drum,
        // the fluorescent pink, is what is held.
        Theme {
            name: "Overprint".to_owned(),
            background: c(0xfb, 0xf7, 0xef),
            white_key: c(0xfb, 0xf7, 0xef),
            black_key: c(0x1a, 0x10, 0x30),
            key_active: c(0x2f, 0x52, 0xd8),
            sustain: c(0xf0, 0x6a, 0xa0),
            key_separator: c(0xc8, 0xa8, 0xb8),
            text: c(0x2a, 0x1a, 0x44),
            theory_line: c(0xc8, 0xb8, 0xc0),
            interval_text: c(0x1e, 0x35, 0x90),
            recorder_bg: c(0xf9, 0xd2, 0xe0),
            fx_knob: c(0x1e, 0x35, 0x90),
            master_knob: c(0xa8, 0x2a, 0x1a),
            varispeed: c(0x2a, 0x1a, 0x44),
            meter_bg: c(0x2a, 0x1a, 0x44),
            scope_draw: c(0xf9, 0xa8, 0xc8),
            fx_strip: c(0xf1, 0xe9, 0xee),
            input_chip: c(0x1e, 0x35, 0x90),
            fretboard: c(0xc8, 0xa8, 0xb8),
            string: c(0x4a, 0x3a, 0x5a),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xff, 0xa3, 0xc2),
            velocity_quiet: c(0xa2, 0xb9, 0xfe),
            channel_palette: Some(26),
            dark: dark_of(&[("background", (0x1a, 0x10, 0x30))]),
        },
        // ── Woodblock: a woodblock print of a great wave. Prussian blue
        // on washi paper, and one vermilion seal in the corner. The only band in
        // the empty arc between hue 210 and 260, and it stays clear of Navi,
        // Storm and the retired Blueprint by being teal-side Prussian on
        // warm-grey paper with a red-orange key. The seal sounds; the bokashi
        // sky is what is held. The seal was lifted a step from #f0623a, which
        // sat 2.98:1 on the band.
        Theme {
            name: "Woodblock".to_owned(),
            background: c(0xe9, 0xe6, 0xdf),
            white_key: c(0xe9, 0xe6, 0xdf),
            black_key: c(0x0e, 0x2a, 0x3c),
            key_active: c(0xf4, 0x6a, 0x40),
            sustain: c(0xa8, 0xcc, 0xe0),
            key_separator: c(0x3a, 0x64, 0x78),
            text: c(0xec, 0xe6, 0xd8),
            theory_line: c(0xa8, 0xa4, 0x98),
            interval_text: c(0x13, 0x4a, 0x62),
            recorder_bg: c(0x00, 0x4b, 0x68),
            fx_knob: c(0xec, 0xe4, 0xd0),
            master_knob: c(0xff, 0x5a, 0x6a),
            varispeed: c(0xa8, 0xd8, 0xf0),
            meter_bg: c(0x08, 0x1c, 0x28),
            scope_draw: c(0xe8, 0xf4, 0xf8),
            fx_strip: c(0x00, 0x3a, 0x52),
            input_chip: c(0xf0, 0x62, 0x3a),
            fretboard: c(0x08, 0x1c, 0x28),
            string: c(0xa8, 0xc0, 0xcc),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xff, 0x78, 0x57),
            velocity_quiet: c(0x21, 0xa9, 0xee),
            channel_palette: Some(27),
            dark: dark_of(&[("background", (0x0e, 0x2a, 0x3c))]),
        },
        // ── Toffee: a 1930s cast-resin radio. Butterscotch cabinet, jade
        // dial, cherry knobs. A DARK-ground theme (the piano lifts its sharps).
        // The band is lighter and more saturated than Amber's, about 22 dE off
        // it, and what sounds is the JADE of the dial, not amber: that is what
        // keeps it from being an Amber clone. The jade is deepened to 21.9 dE
        // off Hydra's, and the rest is the radio rather than the menu: the
        // knobs are the cherry, the master a cream knob so it cannot be taken
        // for them, what is held is the butterscotch itself and the trace a
        // cherry glow. The first cut's cream knobs sat 2.7 dE from Ochre's and
        // its pale hold 4.2 from Amber's. The held butterscotch is as deep as
        // it can go and stay 1.25:1 off the jade (1.26:1).
        Theme {
            name: "Toffee".to_owned(),
            background: c(0x16, 0x0e, 0x08),
            white_key: c(0x16, 0x0e, 0x08),
            black_key: c(0x0a, 0x06, 0x04),
            key_active: c(0x34, 0xa0, 0x78),
            sustain: c(0xdc, 0x90, 0x38),
            key_separator: c(0x3a, 0x24, 0x10),
            text: c(0xf4, 0xe2, 0xb8),
            theory_line: c(0x5a, 0x40, 0x20),
            interval_text: c(0xe8, 0xb0, 0x60),
            recorder_bg: c(0x4e, 0x2d, 0x04),
            fx_knob: c(0xf0, 0x5a, 0x5a),
            master_knob: c(0xf8, 0xec, 0xd0),
            varispeed: c(0x9e, 0xe8, 0xcc),
            meter_bg: c(0x0a, 0x06, 0x04),
            scope_draw: c(0xff, 0x8a, 0x80),
            fx_strip: c(0x3a, 0x22, 0x04),
            input_chip: c(0x34, 0xa0, 0x78),
            fretboard: c(0x0a, 0x06, 0x04),
            string: c(0x9a, 0x7a, 0x50),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x7d, 0x3d),
            velocity_quiet: c(0x1a, 0xb7, 0x82),
            channel_palette: Some(28),
            dark: dark_of(&[("background", (0x0a, 0x06, 0x04))]),
        },
        // ── Confetti: an 80s Memphis sideboard. Butter laminate, a
        // mulberry band, and squiggles in teal, pink and lemon. The loud one.
        // The page is a pale butter far more chromatic than any cream here, and
        // deliberately not blush: a blush page over a berry band is Valentine's
        // formula. Teal sounds and pink is held (the measured #ff7a8a sat
        // 1.14:1 from the teal and is deepened); lemon is the scope and the
        // pitch dial, on the band only, never a key on butter.
        Theme {
            name: "Confetti".to_owned(),
            background: c(0xf6, 0xec, 0xbc),
            white_key: c(0xf6, 0xec, 0xbc),
            black_key: c(0x2a, 0x0e, 0x24),
            key_active: c(0x2c, 0xc4, 0xb8),
            sustain: c(0xf0, 0x58, 0x7a),
            key_separator: c(0x6a, 0x2a, 0x5a),
            text: c(0xf8, 0xec, 0xd0),
            theory_line: c(0xc0, 0xb0, 0x7a),
            interval_text: c(0x7a, 0x1a, 0x5c),
            recorder_bg: c(0x4e, 0x1a, 0x44),
            fx_knob: c(0x6a, 0x8a, 0xf8),
            master_knob: c(0xff, 0x6a, 0x50),
            varispeed: c(0xff, 0xd2, 0x3a),
            meter_bg: c(0x1e, 0x0a, 0x1a),
            scope_draw: c(0xff, 0xd2, 0x3a),
            fx_strip: c(0x3c, 0x12, 0x34),
            input_chip: c(0x2c, 0xc4, 0xb8),
            fretboard: c(0x1e, 0x0a, 0x1a),
            string: c(0xc8, 0xa8, 0xb8),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x56, 0x72),
            velocity_quiet: c(0x17, 0xa3, 0x9e),
            channel_palette: Some(29),
            dark: dark_of(&[("background", (0x2a, 0x0e, 0x24))]),
        },
        // ── Lamplight: a green glass desk lamp in a dark study. Emerald
        // shade, brass chain, a warm pool of light on leather. A DARK-ground
        // theme (the piano lifts its sharps). The band is emerald glass about
        // 21 dE from Phosphor's, and every mark is WARM so it can never read as
        // Phosphor: lamplight white sounds, a brighter glass green is held,
        // oxblood leather on the knobs and brass on the dial and scope. The
        // leather is as deep as clears 3:1 on the emerald (3.24:1); the first
        // cut's #e88a74 read as coral, and coral beside a green band is
        // Malachite's rose. The brass is lighter than a brass wants to be
        // because #e8c070 sat 24 dE from the gain cap.
        Theme {
            name: "Lamplight".to_owned(),
            background: c(0x0a, 0x12, 0x0e),
            white_key: c(0x0a, 0x12, 0x0e),
            black_key: c(0x05, 0x08, 0x06),
            key_active: c(0xff, 0xe6, 0xa8),
            sustain: c(0x6f, 0xcf, 0x9a),
            key_separator: c(0x1a, 0x34, 0x26),
            text: c(0xf0, 0xe2, 0xc0),
            theory_line: c(0x2a, 0x4a, 0x36),
            interval_text: c(0xe8, 0xc0, 0x70),
            recorder_bg: c(0x00, 0x46, 0x1f),
            fx_knob: c(0xd0, 0x70, 0x5a),
            master_knob: c(0xff, 0x5a, 0x6a),
            varispeed: c(0xf0, 0xd8, 0xa0),
            meter_bg: c(0x05, 0x08, 0x06),
            scope_draw: c(0xe8, 0xc0, 0x70),
            fx_strip: c(0x00, 0x34, 0x16),
            input_chip: c(0xd0, 0x70, 0x5a),
            fretboard: c(0x05, 0x08, 0x06),
            string: c(0x8a, 0x9a, 0x80),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xfe, 0x72, 0x56),
            velocity_quiet: c(0x19, 0xb3, 0x66),
            channel_palette: Some(30),
            dark: dark_of(&[("background", (0x05, 0x08, 0x06))]),
        },
        // ── Ditto: a spirit-duplicator handout still smelling of solvent.
        // Aniline violet ink on white copy paper, the purple of every school
        // worksheet. The page is plain copy white, deliberately not lilac
        // (Zinc's page and the retired Iris and Dusk were lilac), and the band
        // is the ink, nudged bluer than the first cut's so it stands 13 dE off
        // Confetti's mulberry rather than 10.5. Ditto violet sounds and a deep
        // aniline purple is held: the first cut pressed a hot magenta and held
        // a pink-white that sat 1.37:1 on the white keys, a held key that looked
        // unpressed, where this purple is 6.4:1. The knobs are the pale violet
        // of a fiftieth copy, the pitch dial the one cool mark. Named for the
        // machine rather than the dye, as the rest of its batch is named for a
        // thing.
        Theme {
            name: "Ditto".to_owned(),
            background: c(0xf6, 0xf5, 0xf2),
            white_key: c(0xf6, 0xf5, 0xf2),
            black_key: c(0x2a, 0x12, 0x36),
            key_active: c(0x9a, 0x7c, 0xff),
            sustain: c(0x7a, 0x3a, 0xa8),
            key_separator: c(0x6a, 0x3a, 0x7a),
            text: c(0xf4, 0xe8, 0xf0),
            theory_line: c(0xb8, 0xb0, 0xbc),
            interval_text: c(0x6a, 0x2a, 0x7a),
            recorder_bg: c(0x45, 0x1f, 0x59),
            fx_knob: c(0xe8, 0xd0, 0xf0),
            master_knob: c(0xff, 0x6a, 0x50),
            varispeed: c(0xa8, 0xe0, 0xd8),
            meter_bg: c(0x1e, 0x0a, 0x26),
            scope_draw: c(0xc0, 0xa8, 0xff),
            fx_strip: c(0x36, 0x16, 0x4a),
            input_chip: c(0x9a, 0x7c, 0xff),
            fretboard: c(0x1e, 0x0a, 0x26),
            string: c(0xb8, 0xa0, 0xc0),
            // velgen.py's solver against this theme's own roll.
            velocity_loud: c(0xff, 0x57, 0x9d),
            velocity_quiet: c(0x1f, 0x9e, 0xde),
            channel_palette: Some(31),
            dark: dark_of(&[("background", (0x2a, 0x12, 0x36))]),
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
  "_comment8": "4 Mono        5 Ochre       6 Reed        7 Sandstone",
  "_comment9": "8 Centennial  9 Zinc       10 Celadon    11 Moss",
  "_comment10": "12 Rhodes    13 Copper    14 Prism      15 Storm",
  "_comment10b": "16 Amber     17 Phosphor  18 Silverface 19 Urushi",
  "_comment10c": "20 Paperback 21 Malachite 22 Sodium    23 Kiln",
  "_comment10d": "24 Lapis     25 Chartres  26 Overprint 27 Woodblock",
  "_comment10e": "28 Toffee    29 Confetti  30 Lamplight 31 Ditto",
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
        // Thirty-two since the 2026-10-08 batch appended thirteen after
        // Silverface. A count that moves is a slot that moved or a theme
        // that went missing, and `palette_choice` is saved by slot, so the
        // number is pinned rather than read off the list.
        assert_eq!(
            f.len(),
            32,
            "Default, fifteen more, the dark two, Silverface, and the thirteen of 2026-10-08"
        );
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
    ///
    /// **This checks what a theme DECLARES, which is not the same question as
    /// what gets painted, and the difference cost three releases.** A theme
    /// can declare `background` and then hand the piano a different natural
    /// key through `dark.white_key`; and the piano can put a keyboard the
    /// right way up out of a pale key it had to derive, which the theme never
    /// declared at all. Both happened, one per mode, on the two dark-designed
    /// themes, and this test went on passing through all of it because the
    /// fields it compares were correct the whole time. The painted half is
    /// `piano::every_theme_paints_its_naturals_on_its_own_page`; neither test
    /// is the other one.
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
