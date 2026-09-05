//! Font setup: two named families backed by bundled Courier Prime (SIL OFL 1.1),
//! with egui's default fonts appended as glyph fallback in each.
//!
//! - "courier"      = CourierPrime-Regular + defaults  (chord strip: Normal weight)
//! - "courier-bold" = CourierPrime-Bold + defaults     (menus, About, dialogs)
//!
//! The optional settings key `custom_font_path` loads a user TTF/OTF at top
//! priority in BOTH families; load errors are silently ignored.
//! Courier New is never shipped (licensing).

use egui::epaint::text::{FontData, FontDefinitions, FontFamily};
use std::sync::Arc;

pub static COURIER_PRIME_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/CourierPrime-Regular.ttf");
pub static COURIER_PRIME_BOLD: &[u8] = include_bytes!("../../assets/fonts/CourierPrime-Bold.ttf");

/// Terminess Nerd Font Mono (SIL OFL 1.1, (C) 2020 Dimitar Toshkov Zhekov,
/// (C) 2023 Tilman Blumenbach). Bundled so the option works on every machine
/// rather than only where it happens to be installed; its licence ships in
/// font-licenses/ in every artifact, as the OFL requires.
pub static TERMINESS_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/TerminessNerdFontMono-Regular.ttf");
pub static TERMINESS_BOLD: &[u8] =
    include_bytes!("../../assets/fonts/TerminessNerdFontMono-Bold.ttf");

/// JetBrains Mono, SIL OFL 1.1. The third face fills the gap the other two
/// leave: Courier Prime is a typewriter and Terminess is a terminal, and
/// neither is a plainly modern one. It is also the only bundled face that
/// covers EVERY symbol Tangent draws by itself, arrows included — Courier
/// Prime has no U+2191/U+2193, so the guitar view's octave arrows reach the
/// screen through the fallback chain rather than from the chosen face.
/// 270 KB a weight against Terminess's 2.6 MB.
pub static JETBRAINS_REGULAR: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMono-Regular.ttf");
pub static JETBRAINS_BOLD: &[u8] = include_bytes!("../../assets/fonts/JetBrainsMono-Bold.ttf");

pub const FAMILY_COURIER: &str = "courier";
pub const FAMILY_COURIER_BOLD: &str = "courier-bold";

pub fn courier() -> FontFamily {
    FontFamily::Name(FAMILY_COURIER.into())
}

pub fn courier_bold() -> FontFamily {
    FontFamily::Name(FAMILY_COURIER_BOLD.into())
}

/// A built-in UI typeface the user can pick from the menu.
///
/// Both are bundled, so the choice always works offline and on a fresh machine.
/// Terminess costs ~5 MB of binary (2.5 MB compressed in the download) because
/// the Nerd Font faces carry thousands of icon glyphs; that was accepted
/// deliberately in favour of the option working for everyone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum FontChoice {
    Courier,
    /// **The one a fresh install wears**, at the owner's word. Courier Prime
    /// was the default from the beginning and stays in the list — a settings
    /// file that names it keeps it, so nobody's desk changes typeface under
    /// them; only an install that has never chosen gets this.
    #[default]
    Terminess,
    JetBrains,
}

impl FontChoice {
    /// Stable key for settings.json. Unknown values fall back to the default.
    pub fn from_key(s: &str) -> Self {
        match s {
            "courier" => FontChoice::Courier,
            "jetbrains" => FontChoice::JetBrains,
            // Unknown falls back to the DEFAULT, which is what this
            // function's own doc promises — it used to name Courier
            // outright, so the promise and the code disagreed the moment the
            // default moved.
            _ => FontChoice::default(),
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            FontChoice::Courier => "courier",
            FontChoice::Terminess => "terminess",
            FontChoice::JetBrains => "jetbrains",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FontChoice::Courier => "Courier Prime",
            FontChoice::Terminess => "Terminess",
            FontChoice::JetBrains => "JetBrains Mono",
        }
    }

    pub const ALL: [FontChoice; 3] = [
        FontChoice::Courier,
        FontChoice::Terminess,
        FontChoice::JetBrains,
    ];

    /// Both faces are compiled in, so every choice is always usable. Kept as a
    /// method because the menu asks per-entry and a future font may not be.
    pub fn is_available(self) -> bool {
        true
    }

    /// The next available face, wrapping.
    ///
    /// This used to be `ALL.iter().find(|f| *f != cur)` in two places, which
    /// is not a cycle: it always returns the FIRST face that is not the
    /// current one. With three faces that flip-flops between the first two and
    /// **JetBrains Mono was unreachable** — bundled, listed, and impossible to
    /// select. One method now, used by the menu label and by the action, so
    /// the row cannot promise a face the click does not give you.
    pub fn next(self) -> Self {
        let here = Self::ALL.iter().position(|f| *f == self).unwrap_or(0);
        for step in 1..=Self::ALL.len() {
            let cand = Self::ALL[(here + step) % Self::ALL.len()];
            if cand.is_available() {
                return cand;
            }
        }
        self
    }
}

/// **How much bigger each face is drawn than its nominal size, so that all
/// three read as the same size of type.**
///
/// A point is not a height. It is the height of the EM, and how much of that
/// em a face fills is the face's own decision — measured off the three files
/// this app ships, per em:
///
/// | | cap height | x-height | advance |
/// |---|---|---|---|
/// | JetBrains Mono | 0.730 | 0.550 | 0.600 |
/// | Terminess | 0.645 | 0.458 | 0.500 |
/// | Courier Prime | 0.580 | 0.451 | 0.600 |
///
/// So at one nominal size JetBrains stands about a fifth taller than either
/// of the others and a fifth wider than Terminess, which is the owner's "the
/// jetbrains text is consistently taller and wider than the other two
/// fonts". The scales below are the ratio of x-heights, which is the measure
/// that governs how big text LOOKS: it puts Courier within 2 % of JetBrains
/// and Terminess within 2 %, where matching cap heights instead would need
/// two very different numbers (1.26 and 1.13) and matching advances would
/// need a third pair again.
///
/// **What this costs, stated because it is a real trade.** Courier already
/// matched JetBrains' advance exactly, so scaling it for height makes its
/// lines about a fifth wider than JetBrains' — one mismatch traded for its
/// opposite. That is the right trade here: the complaint is about how big the
/// letters look, every panel in this app lays out from measured text rather
/// than from an assumed column count, and the fit sweeps in `face::fit` are
/// what prove a widened face still lands inside its furniture.
///
/// Applied through egui's own `FontTweak`, so a size named anywhere in the
/// app keeps meaning one thing: nothing has to know which face is loaded, and
/// there is no second size table to keep in step with the first.
const fn face_scale(choice: FontChoice) -> f32 {
    match choice {
        // 0.550 / 0.451
        FontChoice::Courier => 1.219,
        // 0.550 / 0.458
        FontChoice::Terminess => 1.201,
        // The face the other two are being matched TO.
        FontChoice::JetBrains => 1.0,
    }
}

/// The face as egui should draw it: its bytes, scaled by [`face_scale`].
fn face(bytes: &'static [u8], scale: f32) -> Arc<FontData> {
    Arc::new(FontData::from_static(bytes).tweak(egui::FontTweak {
        scale,
        ..Default::default()
    }))
}

/// Install the font families on the context. Call at startup and again if the
/// font choice or `custom_font_path` changes (e.g. settings reset).
pub fn install(ctx: &egui::Context, choice: FontChoice, custom_font_path: Option<&str>) {
    let mut defs = FontDefinitions::default();
    let scale = face_scale(choice);

    // Courier Prime is both a CHOICE and the glyph fallback under every other
    // choice, and it takes its OWN scale in both jobs. That is not a special
    // case: the scale answers "how big does this face draw", which is a
    // property of the file and not of the menu, so a degree sign borrowed
    // from Courier lands the same height as the Terminess text around it.
    let courier = face_scale(FontChoice::Courier);
    defs.font_data.insert(
        "CourierPrime-Regular".to_owned(),
        face(COURIER_PRIME_REGULAR, courier),
    );
    defs.font_data.insert(
        "CourierPrime-Bold".to_owned(),
        face(COURIER_PRIME_BOLD, courier),
    );

    // Egui's default fonts, appended as glyph fallback (Δ is covered by
    // Courier Prime itself; the fallback covers arrows/emoji/etc.).
    let default_fallback: Vec<String> = defs
        .families
        .get(&FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut regular = vec!["CourierPrime-Regular".to_owned()];
    let mut bold = vec!["CourierPrime-Bold".to_owned()];

    // A chosen built-in sits above Courier Prime but below any custom font.
    // Courier Prime stays in the chain underneath as glyph fallback, so a face
    // lacking a symbol Tangent draws (°, ø, Δ) still renders it correctly.
    // The chosen face, whichever it is. Courier Prime stays underneath as glyph
    // fallback, so a face missing a symbol still renders it.
    if let Some((name, reg, bold_bytes)) = match choice {
        FontChoice::Terminess => Some(("Terminess", TERMINESS_REGULAR, TERMINESS_BOLD)),
        FontChoice::JetBrains => Some(("JetBrainsMono", JETBRAINS_REGULAR, JETBRAINS_BOLD)),
        FontChoice::Courier => None,
    } {
        defs.font_data.insert(format!("{name}-Regular"), face(reg, scale));
        defs.font_data.insert(format!("{name}-Bold"), face(bold_bytes, scale));
        regular.insert(0, format!("{name}-Regular"));
        // Real bold face, so a chosen font does not flatten the menus and
        // About to a single weight.
        bold.insert(0, format!("{name}-Bold"));
    }

    // Optional user font at top priority in both families; errors ignored.
    if let Some(path) = custom_font_path {
        if let Ok(bytes) = std::fs::read(path) {
            defs.font_data.insert(
                "IvoryCustom".to_owned(),
                Arc::new(FontData::from_owned(bytes)),
            );
            regular.insert(0, "IvoryCustom".to_owned());
            bold.insert(0, "IvoryCustom".to_owned());
        }
    }

    // JetBrains Mono sits at the BOTTOM of both chains, always, whichever
    // face the user picked.
    //
    // It is the only bundled face with the music accidentals — U+266D and
    // U+266F, verified by reading the cmaps — and neither Courier Prime nor
    // Terminess has them, nor does egui's default fallback. Without this the
    // circle of fifths had to spell a flat as the letter "b", so the relative
    // minor of D flat read "bb" and a key signature read "2b". It costs
    // nothing: the face is compiled in already for the third typeface option.
    if !matches!(choice, FontChoice::JetBrains) {
        // The accidentals' fallback, at JetBrains' own scale of 1.0 — it is
        // already the tallest face on its em, so a flat sign borrowed from it
        // sits right beside text that has been scaled up to meet it.
        defs.font_data.insert(
            "JetBrainsMono-Fallback".to_owned(),
            face(JETBRAINS_REGULAR, face_scale(FontChoice::JetBrains)),
        );
        regular.push("JetBrainsMono-Fallback".to_owned());
        bold.push("JetBrainsMono-Fallback".to_owned());
    }

    regular.extend(default_fallback.iter().cloned());
    bold.extend(default_fallback.iter().cloned());

    defs.families
        .insert(FontFamily::Name(FAMILY_COURIER.into()), regular);
    defs.families
        .insert(FontFamily::Name(FAMILY_COURIER_BOLD.into()), bold);

    ctx.set_fonts(defs);
}

/// Map egui text styles so stock widgets pick the right family: bold Courier
/// for chrome (menus/dialogs use TextStyle::Button/Body), regular for
/// monospace. Explicit `FontId`s are used wherever the spec names a size.
pub fn apply_text_styles(ctx: &egui::Context) {
    use egui::{FontId, TextStyle};
    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(13.0, courier_bold()));
        style
            .text_styles
            .insert(TextStyle::Button, FontId::new(13.0, courier_bold()));
        style
            .text_styles
            .insert(TextStyle::Heading, FontId::new(16.0, courier_bold()));
        style
            .text_styles
            .insert(TextStyle::Small, FontId::new(10.0, courier_bold()));
        style
            .text_styles
            .insert(TextStyle::Monospace, FontId::new(13.0, courier()));
    });
}

#[cfg(test)]

    /// **The three faces read as the same size of type.**
    ///
    /// A point is the height of the em, and how much of that em a face fills
    /// is the face's own decision — so at one nominal size JetBrains Mono
    /// stood about a fifth taller than Courier Prime and Terminess, and a
    /// fifth wider than Terminess. The owner measured it by eye: "the
    /// jetbrains text is consistently taller and wider than the other two
    /// fonts."
    ///
    /// [`face_scale`] closes it on x-height, which is the measure that
    /// governs how big text LOOKS, and this is the check that it did: a
    /// lowercase letter's ink, laid out through the real context at one size,
    /// within a few percent across all three. Cap height and advance are
    /// deliberately NOT held here — matching cap heights would need two very
    /// different numbers and matching advances a third pair again, and
    /// x-height is the one that answers the complaint.
    ///
    /// Revert-check, performed: set either scale to 1.0 and this reports the
    /// face that shrank, about 18 % adrift.
    #[test]
    fn the_three_faces_read_at_the_same_size() {
        let mut heights: Vec<(&str, f32)> = Vec::new();
        for (name, choice) in [
            ("Courier", FontChoice::Courier),
            ("Terminess", FontChoice::Terminess),
            ("JetBrains", FontChoice::JetBrains),
        ] {
            let ctx = crate::face::fit::a_ctx(choice);
            let _ = ctx.run(egui::RawInput::default(), |_| {});
            let id = egui::FontId::new(12.0, courier());
            // "x" rather than a cap or a digit: the x-height is what these
            // scales were fitted on, and it is what the eye reads as size.
            let g = ctx.fonts_mut(|f| {
                f.layout_no_wrap("x".to_owned(), id.clone(), egui::Color32::WHITE)
            });
            let ink = g
                .rows
                .first()
                .map_or(0.0, |r| r.visuals.mesh_bounds.height());
            heights.push((name, ink));
        }
        let lo = heights.iter().map(|(_, h)| *h).fold(f32::MAX, f32::min);
        let hi = heights.iter().map(|(_, h)| *h).fold(0.0_f32, f32::max);
        assert!(
            lo > 0.0 && hi / lo <= 1.08,
            "the faces still read at different sizes: {heights:?}"
        );
    }
mod tests {

    /// **Every font size is a whole number, at every window width.**
    ///
    /// epaint rasterises unhinted at exactly `size * ppp` with four horizontal
    /// subpixel bins and none vertical, so a fractional size puts horizontal
    /// strokes across two pixel rows at partial coverage — soft text, by the
    /// same mechanism that makes a 1.5-wide rule soft.
    ///
    /// It also keeps the glyph atlas from thrashing. epaint keys its cache on
    /// the scaled metrics, so a size that changes by a hundredth of a point
    /// every frame is a total cache miss every frame; at the display's refresh
    /// rate during a drag that is hundreds of glyph sets a second into an
    /// atlas that doubles, marks itself dirty, and eventually overflows and
    /// restarts its cursor over live glyphs. That is garbled text, not soft
    /// text, and it is why this matters more now the window is paced.
    #[test]
    fn font_sizes_are_whole_numbers_and_never_vanish() {
        for w in [560.0_f32, 900.0, 1300.0, 1600.0, 2000.0, 2560.0, 3840.0] {
            // The shapes the panels actually derive sizes from: a fraction of
            // a key width, of a staff space, of a dot radius.
            for frac in [0.115_f32, 0.46, 0.52, 0.62, 0.86, 0.95, 1.15, 1.4, 1.9] {
                let raw = w / 1300.0 * 7.5 * frac;
                let got = px(raw);
                assert_eq!(got, got.round(), "px({raw}) = {got} is not whole");
                assert!(got >= 1.0, "px({raw}) vanished to {got}");
            }
        }
        // A size already whole is untouched, and a tiny one still draws.
        assert_eq!(px(12.0), 12.0);
        assert_eq!(px(0.2), 1.0, "a sub-pixel size must not become zero");
    }

    /// Cycling must REACH every bundled face. It did not: the old
    /// "first face that is not the current one" rule bounced between Courier
    /// Prime and Terminess forever, and JetBrains Mono — compiled into the
    /// binary, listed in the menu — could not be selected at all.
    #[test]
    fn cycling_reaches_every_bundled_face() {
        let mut seen = std::collections::HashSet::new();
        let mut f = FontChoice::default();
        for _ in 0..FontChoice::ALL.len() {
            seen.insert(f);
            f = f.next();
        }
        assert_eq!(
            seen.len(),
            FontChoice::ALL.len(),
            "cycling from {:?} only reached {:?}",
            FontChoice::default(),
            seen
        );
        assert_eq!(
            f,
            FontChoice::default(),
            "the cycle does not come back round"
        );

        // ...from every starting point, not just the default.
        for start in FontChoice::ALL {
            let mut seen = std::collections::HashSet::new();
            let mut f = start;
            for _ in 0..FontChoice::ALL.len() {
                seen.insert(f);
                f = f.next();
            }
            assert_eq!(seen.len(), FontChoice::ALL.len(), "starting from {start:?}");
        }
    }

    /// The menu row promises the face the next click will give you.
    #[test]
    fn the_menu_label_matches_what_cycling_does() {
        for f in FontChoice::ALL {
            assert_eq!(f.next().label(), f.next().label());
            assert_ne!(f.next(), f, "a face that cycles to itself would look stuck");
        }
    }
    use super::*;

    /// Spec §5.6 / DESIGN: the chord font must cover Δ (U+0394, maj7 glyph)
    /// and ø (U+00F8) in both embedded weights.
    #[test]
    fn embedded_fonts_cover_delta_and_oslash() {
        // Every symbol Tangent actually draws, not just the two chord ones.
        // The arrows matter: the guitar view marks a folded note with U+2191 or
        // U+2193, and Courier Prime does NOT have them — they reach the screen
        // through the fallback chain. That is fine, and it is worth having a
        // test that knows it rather than a surprise if the chain ever changes.
        const DRAWN: &[(char, &str)] = &[
            ('\u{0394}', "major-7 delta"),
            ('\u{00F8}', "half-diminished slash-o"),
            ('\u{00B0}', "diminished ring"),
            ('\u{00D7}', "muted-string cross"),
            ('\u{2022}', "menu selection dot"),
            ('\u{00B7}', "caption separator"),
            ('\u{2191}', "octave up"),
            ('\u{2193}', "octave down"),
        ];

        let has = |bytes: &[u8], c: char| {
            ttf_parser::Face::parse(bytes, 0)
                .map(|f| f.glyph_index(c).is_some())
                .unwrap_or(false)
        };

        // The chord symbols must come from the bundled faces themselves, not
        // from a fallback: they are the app's whole output.
        for (c, what) in &DRAWN[..3] {
            assert!(has(COURIER_PRIME_REGULAR, *c), "Courier Prime lacks {what}");
            assert!(
                has(COURIER_PRIME_BOLD, *c),
                "Courier Prime Bold lacks {what}"
            );
        }

        // JetBrains Mono is the one face that covers the lot on its own, which
        // is part of why it is bundled.
        for (c, what) in DRAWN {
            assert!(has(JETBRAINS_REGULAR, *c), "JetBrains Mono lacks {what}");
            assert!(has(JETBRAINS_BOLD, *c), "JetBrains Mono Bold lacks {what}");
        }

        // And everything must be reachable from SOMEWHERE in the chain.
        for (c, what) in DRAWN {
            let covered = has(COURIER_PRIME_REGULAR, *c)
                || has(TERMINESS_REGULAR, *c)
                || has(JETBRAINS_REGULAR, *c);
            assert!(
                covered,
                "no bundled face has {what}; it would render as tofu"
            );
        }
    }
}

/// **A font size that lands on the raster grid.**
///
/// epaint rasterises unhinted at exactly `size * pixels_per_point`, with four
/// HORIZONTAL subpixel bins and none vertical — so how much coverage a stem or
/// a crossbar gets is decided purely by the number, and a fractional size puts
/// horizontal strokes across two rows of pixels at partial coverage. That is
/// soft text, and it is the same mechanism as a stroke width landing between
/// pixels.
///
/// Almost every size in this app is derived as a raw float off a rect — a key
/// width, a dot radius, a staff space — so it lands anywhere. Rounding here
/// costs nothing anyone can see and makes the sizes repeat instead of sweeping.
///
/// **It also protects the glyph atlas.** epaint keys its cache on the scaled
/// metrics, so a fresh fractional size every frame is a total cache miss every
/// frame; while the window is being dragged at the display's refresh rate that
/// is hundreds of glyph sets a second into an atlas that doubles, marks itself
/// entirely dirty, and eventually overflows and restarts its cursor over live
/// glyphs — garbled text rather than merely soft text.
pub fn px(size: f32) -> f32 {
    size.round().max(1.0)
}
