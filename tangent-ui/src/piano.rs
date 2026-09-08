//! 88-key piano renderer + hit-testing, replicating Qt geometry exactly
//! (spec §4): 52 white keys, blackW = 0.7*whiteW, blackH = 0.65*H, black keys
//! centered on the white-key boundary via the whiteKeysBefore formula, with
//! Python's int() truncation. Sub-pixel background slivers between truncated
//! key rects are part of the look and are preserved.

use crate::fonts;
use crate::settings::Settings;
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind};
use std::collections::HashSet;

pub const NOTE_START: u8 = 21; // A0
pub const NOTE_END: u8 = 108; // C8
pub const WHITE_KEYS: usize = 52;

pub fn is_white(note: u8) -> bool {
    matches!(note % 12, 0 | 2 | 4 | 5 | 7 | 9 | 11)
}

/// Number of white keys strictly to the left of a black key (spec §4.3.4):
/// octave block note//12 == 1 (A#0) => 0; otherwise 2 (A0,B0) plus
/// 7*(note//12 - 2) plus a per-pitch-class offset.
pub fn white_keys_before(note: u8) -> Option<usize> {
    if is_white(note) {
        return None;
    }
    if note / 12 == 1 {
        return Some(0); // A#0
    }
    let offset = match note % 12 {
        1 => 0,  // C#
        3 => 1,  // D#
        6 => 3,  // F#
        8 => 4,  // G#
        10 => 5, // A#
        _ => return None,
    };
    Some(2 + 7 * (note as usize / 12 - 2) + offset)
}

/// Idle fill for a key.
///
/// **Dark mode INVERTS the keyboard rather than dimming it.** Shading each
/// colour down was an improvement on not responding at all, but it is not
/// what this toggle used to do and not what it should do: the naturals go to
/// the sharps' colour and the sharps to the naturals', so a stock keyboard
/// reads as ivory sharps on black naturals - the same ivory-on-black the
/// theory panel above it wears, out of the same two colours the user already
/// chose. A themed or hand-painted keyboard inverts with its own pair, so it
/// stays recognisably itself.
fn idle_colors(s: &Settings) -> (Color32, Color32) {
    let white = s.white_key_idle_color.to_color32();
    let black = s.black_key_idle_color.to_color32();
    if !s.dark_mode {
        // The naturals ARE the page's ground by design, so the SHARPS are
        // what make a keyboard visible. On a dark-designed theme they are
        // near-black on near-black; see [`keys_apart`].
        return keys_the_right_way_up(white, keys_apart(white, black), false);
    }
    // **What the theme SAYS, before what dark mode would derive.**
    //
    // The swap below is the derivation, and it is a good one: the owner asked
    // for a pure inversion to ivory on black and that is what it gives. But a
    // theme that names its own dark keyboard means it, and a theme is allowed
    // to disagree with the transform — that is what naming a colour is for.
    let naturals = s.dark_role("white_key").map_or(black, |c| c.to_color32());
    let sharps = s.dark_role("black_key").map_or(white, |c| c.to_color32());
    // **And then the same lift the light mode gets, because a theme is
    // allowed to disagree with the transform and is not allowed to hand back
    // a keyboard with no keys in it.**
    //
    // This used to stop at the line above, on the argument that dark mode has
    // an escape a theme can actually reach — `dark.white_key` and
    // `dark.black_key`, which the two dark-designed themes both named on
    // purpose to sit at their own quieter level. The argument was wrong in
    // one specific way: naming a colour says where a theme wants its keys,
    // not that the pair it named can still be told apart. Nocturne and
    // Phosphor named two near-blacks, and measured they were 1.81:1 and
    // 2.09:1 apart on grounds they cleared by 2.65:1 and 2.75:1 — against
    // every other theme in either mode at 9.34:1 or better. That is the
    // owner's "dark mode only mask over the piano", reported for the third
    // time, and it is the same fault the light mode already fixes by the
    // same means. A theme that is already clear is returned untouched, which
    // is every other theme in the set, so this is inert on thirty-two of the
    // thirty-four theme-and-mode pairs.
    //
    // Once the lift walked here too, those two overrides had nothing left to
    // do and were doing harm: they held the NATURALS at 0.099 and 0.093
    // luminance over grounds of 0.006 and 0.002, so on exactly two themes the
    // dark keyboard was a mid slab floating above its own page while the
    // other sixteen paint their naturals ON it. They are gone from the theme
    // table, and the fallbacks below — the sharps' colour for the naturals,
    // the naturals' for the sharps — are what every theme in the set uses.
    keys_the_right_way_up(naturals, keys_apart(naturals, sharps), true)
}

/// **The naturals are the PALE key in light mode and the dark one in dark
/// mode**, on every theme, whatever the theme named.
///
/// Sixteen of the eighteen themes get this for nothing: their light ground is
/// pale, `white_key` is pinned to it, and the sharps come out near-black. The
/// two dark-designed ones do not. Nocturne's and Phosphor's LIGHT grounds are
/// 0.011 and 0.004 luminance — that is what makes them what they are — so
/// their naturals came out near-black and the lift then made the sharps the
/// pale ones. Measured across the set: every other theme paints naturals
/// between 0.61 and 0.97 in light mode; those two painted 0.011 and 0.004.
///
/// A keyboard with black naturals and pale sharps is not a dim keyboard, it is
/// an INVERTED one, and it was inverted in the mode where every other theme is
/// the right way up — so on those two, and only those two, light mode looked
/// like everybody else's dark mode and dark mode looked like everybody else's
/// light. The owner: "they are the only themes with dark keyboards in light
/// mode, and light keyboards in dark mode."
///
/// So the polarity is decided here rather than inherited. The two colours are
/// kept exactly as they are — their contrast has already been earned by
/// `keys_apart`, and swapping a pair cannot change how far apart it is — and
/// only their ROLES are put the right way round.
fn keys_the_right_way_up(
    naturals: Color32,
    sharps: Color32,
    dark: bool,
) -> (Color32, Color32) {
    use crate::recorder_panel::luminance;
    let naturals_paler = luminance(naturals) > luminance(sharps);
    // Light mode wants pale naturals; dark mode wants dark ones.
    if naturals_paler == dark {
        (sharps, naturals)
    } else {
        (naturals, sharps)
    }
}

/// **How far apart the two idle key colours have to be** for a keyboard to
/// read as a keyboard rather than as one flat slab, as a contrast ratio.
///
/// Not a floor picked in the abstract: it is where the seventeen themes that
/// have never had this problem actually sit. The narrowest of them is
/// Sandstone at 9.34:1 and the widest Iris at 15.29:1, so a derived pair that
/// clears 9 lands at the bottom of the band everybody else is already in
/// rather than at a bar of its own.
///
/// **A target the walk may not reach, and deliberately stops short of.** Where
/// the naturals are themselves middling — Nocturne's and Phosphor's dark
/// naturals sit at 0.099 and 0.093 luminance — nothing the sharps can become
/// clears 9:1, so the walk used to run out of steps at pure white and miss the
/// bar anyway. It now stops at the desk's own pale ink instead, which reads
/// without glaring; the floor those two actually have to meet is the 3:1 in
/// `every_theme_keeps_a_legible_keyboard_in_both_modes`, and they clear it by
/// a wide margin. See `keys_apart`.
const KEYS_APART: f32 = 9.0;

/// **The sharps, lifted clear of the naturals when the theme could not do it
/// itself.**
///
/// In light mode the naturals are the theme's `white_key` and the sharps its
/// `black_key`, and on seventeen themes that is a near-black sharp on an
/// ivory natural — nothing to derive. On the two dark-designed themes
/// (Nocturne, Phosphor) both are near-black and the whole piano band was one
/// slab: 1.10:1 and 1.04:1 measured, which is not a dim keyboard but an
/// absent one. The owner photographed it twice.
///
/// **The theme cannot fix this from its own fields, which is why the piano
/// does.** `white_key` is pinned to `background` and `black_key` to
/// `dark.background` — the theory page sits on the naturals' colour in light
/// mode and on the sharps' in dark, which is the owner's own rule and is
/// tested (`theme::the_theory_ground_is_the_keyboard_not_the_band`). A theme
/// whose two grounds are both near-black has therefore already spent both key
/// colours before the piano is drawn. Every other theme has slack because its
/// light ground is pale; these have none.
///
/// So the sharps walk AWAY from the naturals — toward white on a dark
/// natural, toward black on a pale one — until they clear [`KEYS_APART`], and
/// the result is the same picture a light theme wears in dark mode: pale
/// sharps on a dark field. A theme already clear of the line is returned
/// untouched, so this is inert on all seventeen.
///
/// **In both modes.** It was light-mode only, on the argument that dark mode
/// has an escape a theme can actually reach — `dark.white_key` /
/// `dark.black_key` — and that a derivation there would overrule a theme that
/// had spoken. Both halves of that were wrong: the pair those two themes
/// spoke was itself a mask at 1.81:1 and 2.09:1, and holding the naturals off
/// their own ground to make room for it is what made the dark keyboard read
/// bright. The overrides are gone and this walks in either mode, where the
/// theme has been silenced by the theory-ground rule and nowhere else.
fn keys_apart(natural: Color32, sharp: Color32) -> Color32 {
    use crate::recorder_panel::contrast_ratio;
    if contrast_ratio(sharp, natural) >= KEYS_APART {
        return sharp;
    }
    // Away from the naturals: a dark keyboard gets pale sharps and a pale one
    // dark sharps. `luminance` rather than a channel average, because what is
    // being escaped is a contrast ratio and that is what a ratio is made of.
    // **Toward the desk's own ink, not toward the screen's limit.**
    //
    // This walked to pure `WHITE`, and on the two themes it exists for the
    // walk ran all twenty steps and arrived there: Nocturne and Phosphor
    // hand it naturals at 0.099 and 0.093 luminance, from which nothing —
    // not even white — reaches `KEYS_APART`, so the loop below fell out at
    // `k = 20` every time. The keyboard was legible again and the owner's
    // next word on it was that the pale keys are "too bright": a key at
    // luminance 1.000 beside a natural at 0.099, where every other theme's
    // pale key sits at 0.88 or below.
    //
    // `INK_LIGHT` and `INK_DARK` are the two the rest of this app already
    // paints with — a bone and a near-black, chosen to read rather than to
    // shine — so a saturated walk now lands on the same pale this desk uses
    // everywhere else instead of on the brightest thing a monitor can do.
    // The ratio it reaches is lower for those two themes and that is the
    // trade being made knowingly: measured, they go from 7.05:1 at pure
    // white — which never met the bar anyway — to a little under it in a
    // colour that does not glare. Every theme that could already clear
    // `KEYS_APART` returns above this line untouched.
    let away = if crate::recorder_panel::luminance(natural) < 0.18 {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    };
    // Toward it in twentieths, keeping as much of the theme's own hue as the
    // ratio allows: the first step that clears is the one taken.
    let mut out = sharp;
    for k in 1..=20u8 {
        out = mix(sharp, away, f32::from(k) * 0.05);
        if contrast_ratio(out, natural) >= KEYS_APART {
            break;
        }
    }
    out
}

/// Blend two colours, `t` of the way from `a` to `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// The ground behind the keyboard — a THEME ROLE now.
///
/// It was two fixed values, one per mode, identical on all sixteen themes.
pub fn bg_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::from_rgb(0xE8, 0xE8, 0xE8)
    }
}

/// The background this theme asks for, in the mode it is in.
///
/// Dark mode takes the theme's own dark ground when it named one and darkens
/// the light one when it did not, which is the derivation every other role
/// gets.
pub fn themed_bg(s: &Settings) -> Color32 {
    if !s.dark_mode {
        return s.background_color.to_color32();
    }
    s.dark_role("background").map_or_else(
        || {
            let c = s.background_color.to_color32();
            let f = |v: u8| (f32::from(v) * 0.12) as u8;
            Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
        },
        |c| c.to_color32(),
    )
}

/// Draw the piano into `rect`. `display_notes` must already include manual
/// (keytoggle) notes. While the sustain pedal is down, EVERY active key is
/// drawn in the sustain color (spec §4.4).
pub fn draw(
    painter: &Painter,
    rect: Rect,
    display_notes: &HashSet<u8>,
    sustain_down: bool,
    s: &Settings,
) {
    let w = rect.width() as f64;
    let h = rect.height() as f64;
    let left = rect.left();
    let top = rect.top();

    // 1. Background fill (visible only through truncation slivers).
    painter.rect_filled(rect, 0.0, themed_bg(s));

    let white_key_w = w / WHITE_KEYS as f64;
    let black_key_w = white_key_w * 0.7;
    let black_key_h = h * 0.65;

    let (white_idle, black_idle) = idle_colors(s);
    let sustain = s.sustain_color.to_color32();
    let white_active = s.white_key_active_color.to_color32();
    let black_active = s.black_key_active_color.to_color32();

    // The walnut separator and outline are drawn FOR the bone keyboard; on
    // the dark one they are the brightest thing on screen. Darkened together
    // so the key edges stay visible without ruling the view.
    // **The separators are a theme role, drawn for the keyboard they are
    // on.** The shipped walnut is right on a bone keyboard: dark cuts between
    // light keys. Inverted, the naturals ARE dark — a dark separator is a
    // line nobody can see, and a brown one reads as the recorder band's
    // colour leaking into the instrument, which is what the owner saw.
    //
    // So the theme names the colour, and it is flipped toward the SHARP key's
    // colour when it would otherwise disappear into the natural it is cut
    // into. One rule, and it holds for a themed keyboard as well as a stock
    // one: the cut always contrasts with the key it divides.
    let (separator, outline) = {
        let base = s.key_separator_color.to_color32();
        let natural = white_idle;
        let cut = if crate::recorder_panel::wants_light_ink(natural) {
            // Dark keys: lift the separator toward the sharps' light colour.
            mix(base, black_idle, 0.72)
        } else {
            base
        };
        (cut, mix(cut, natural, 0.42))
    };

    // 2. White keys, left to right (int truncation per key, like Qt fillRect).
    // Fill every white key FIRST, then draw the separators on top — otherwise each
    // key's fill overpaints the previous key's separator line and they vanish.
    let mut idx = 0usize;
    for note in NOTE_START..=NOTE_END {
        if !is_white(note) {
            continue;
        }
        let fill = if display_notes.contains(&note) {
            if sustain_down {
                sustain
            } else {
                white_active
            }
        } else {
            white_idle
        };
        // Each key runs to where the NEXT one starts, rather than being a
        // truncated fixed width. Both are integer-aligned, but the fixed width
        // is 31 while the pitch is 31.25, so every fourth key used to leave a
        // one-pixel gap the background showed through: twelve grey slivers
        // across the keyboard, irregularly spaced. They were invisible in dark
        // mode, where the background happens to equal the key colour, and
        // obvious in light mode. The separators still land on exactly the same
        // pixels, so the look is otherwise unchanged.
        let x0 = (idx as f64 * white_key_w).trunc() as f32;
        let x1 = ((idx + 1) as f64 * white_key_w).trunc() as f32;
        let key_rect = Rect::from_min_max(
            Pos2::new(left + x0, top),
            Pos2::new(left + x1, top + h.trunc() as f32),
        );
        painter.rect_filled(key_rect, 0.0, fill);
        idx += 1;
    }
    // 1px separators between adjacent white keys (drawn after all fills).
    for sep in 1..WHITE_KEYS {
        let lx = left + (sep as f64 * white_key_w).trunc() as f32 + 0.5;
        painter.line_segment(
            [Pos2::new(lx, top), Pos2::new(lx, top + h.trunc() as f32)],
            Stroke::new(1.0_f32, separator),
        );
    }

    // 3. Black keys on top.
    for note in NOTE_START..=NOTE_END {
        let Some(before) = white_keys_before(note) else {
            continue;
        };
        if !(before < WHITE_KEYS && before + 1 < WHITE_KEYS) {
            continue;
        }
        // gapCenter = (before + 1) * whiteKeyW (exactly on the separator line)
        let gap_center = (before as f64 + 1.0) * white_key_w;
        let x = gap_center - black_key_w / 2.0;

        let fill = if display_notes.contains(&note) {
            if sustain_down {
                sustain
            } else {
                black_active
            }
        } else {
            black_idle
        };
        let bx = x.trunc() as f32;
        let bw = black_key_w.trunc() as f32;
        let bh = black_key_h.trunc() as f32;
        let key_rect = Rect::from_min_size(Pos2::new(left + bx, top), egui::vec2(bw, bh));
        painter.rect_filled(key_rect, 0.0, fill);
        // 1px outline; Qt drawRect includes the far edge, so stroke the
        // half-pixel-centered boundary for a crisp 1px ring.
        let stroke_rect = Rect::from_min_max(
            Pos2::new(left + bx + 0.5, top + 0.5),
            Pos2::new(left + bx + bw + 0.5, top + bh + 0.5),
        );
        painter.rect_stroke(
            stroke_rect,
            0.0,
            Stroke::new(1.0_f32, outline),
            StrokeKind::Middle,
        );
    }

    if s.show_piano_note_names {
        draw_names(painter, rect, display_notes, sustain_down, s);
    }
}

/// Letter names on the keys, the way a beginner's stickers sit.
///
/// **Last, over everything.** They are the only thing on this band that has to
/// be readable against four different fills — idle and lit, white and black —
/// so they are drawn after all of them and coloured against the key they
/// landed on rather than against the theme.
///
/// Near the bottom of each key, because that is where a sticker goes and
/// because it keeps the letters clear of the black keys entirely.
fn draw_names(
    painter: &Painter,
    rect: Rect,
    display_notes: &HashSet<u8>,
    sustain_down: bool,
    s: &Settings,
) {
    let w = rect.width() as f64;
    let h = rect.height() as f64;
    let white_key_w = w / WHITE_KEYS as f64;
    let black_key_w = white_key_w * 0.7;
    let black_key_h = h * 0.65;

    // Sized off the key, and not drawn at all below the point where the letters
    // would be a grey smear. A label nobody can read is worse than the space it
    // took.
    let size = fonts::px((white_key_w as f32 * 0.46).min(rect.height() * 0.12));
    if size < 6.0 {
        return;
    }
    let font = FontId::new(size, fonts::courier_bold());
    // The SAME pair the keys were filled from — dark mode swaps them, and the
    // ink has to follow the swap.
    let (white_idle, black_idle) = idle_colors(s);

    // **MEASURED against the key it lands on, every time.**
    //
    // The four idle/lit cases used to be two fixed greys — 0x30 on a natural
    // and 0xD0 on a sharp — which is right on exactly one keyboard: the bone
    // one this app shipped with. A theme that picks its own key colours
    // breaks it, and DARK MODE breaks it twice over, because the naturals and
    // the sharps SWAP (`idle_colors`): the dark grey meant for ivory lands on
    // a near-black natural and the light grey meant for a black key lands on
    // an ivory one. Both illegible at once, which is what the owner
    // photographed.
    //
    // A lit key was already measured, by a brightness sum. That is the rule
    // `wants_light_ink` exists to replace — it MEASURES both candidates
    // instead of testing a threshold — so all four cases go through it now,
    // against the colour actually painted.
    let ink_on = |lit: bool, on_black: bool| {
        let bg = if lit {
            if sustain_down {
                s.sustain_color.to_color32()
            } else if on_black {
                s.black_key_active_color.to_color32()
            } else {
                s.white_key_active_color.to_color32()
            }
        } else if on_black {
            black_idle
        } else {
            white_idle
        };
        if crate::recorder_panel::wants_light_ink(bg) {
            crate::recorder_panel::INK_LIGHT
        } else {
            crate::recorder_panel::INK_DARK
        }
    };

    // White keys: centred in the key, low enough to clear the black ones.
    let mut idx = 0usize;
    for note in NOTE_START..=NOTE_END {
        if !is_white(note) {
            continue;
        }
        let x0 = (idx as f64 * white_key_w).trunc() as f32;
        let x1 = ((idx + 1) as f64 * white_key_w).trunc() as f32;
        idx += 1;
        painter.text(
            Pos2::new(rect.left() + (x0 + x1) * 0.5, rect.bottom() - size * 0.95),
            Align2::CENTER_CENTER,
            pc_name(note % 12, s.prefer_flats),
            font.clone(),
            ink_on(display_notes.contains(&note), false),
        );
    }

    // Black keys, only where the sharp's two characters actually fit.
    if black_key_w as f32 >= size * 1.5 {
        for note in NOTE_START..=NOTE_END {
            let Some(before) = white_keys_before(note) else {
                continue;
            };
            if !(before < WHITE_KEYS && before + 1 < WHITE_KEYS) {
                continue;
            }
            let key_x = (before as f64 + 1.0) * white_key_w - black_key_w / 2.0;
            painter.text(
                Pos2::new(
                    rect.left() + (key_x + black_key_w * 0.5) as f32,
                    rect.top() + black_key_h as f32 - size * 0.9,
                ),
                Align2::CENTER_CENTER,
                pc_name(note % 12, s.prefer_flats),
                font.clone(),
                ink_on(display_notes.contains(&note), true),
            );
        }
    }
}

/// A pitch class as a letter, in the user's own preference.
fn pc_name(pc: u8, prefer_flats: bool) -> &'static str {
    let i = usize::from(pc % 12);
    if prefer_flats {
        tangent_core::patterns::NOTE_NAMES_FLAT[i]
    } else {
        tangent_core::patterns::NOTE_NAMES[i]
    }
}

/// Hit-test a point (widget-local) against the keys, matching the drawing math
/// (spec §4.5): black keys first when y < blackKeyH, else white by index.
pub fn hit_test(x: f32, y: f32, w: f32, h: f32) -> Option<u8> {
    if x < 0.0 || y < 0.0 || x >= w || y >= h {
        return None;
    }
    let x = x as f64;
    let y = y as f64;
    let w = w as f64;
    let h = h as f64;
    let white_key_w = w / WHITE_KEYS as f64;
    let black_key_w = white_key_w * 0.7;
    let black_key_h = h * 0.65;

    if y < black_key_h {
        for note in NOTE_START..=NOTE_END {
            let Some(before) = white_keys_before(note) else {
                continue;
            };
            if !(before < WHITE_KEYS && before + 1 < WHITE_KEYS) {
                continue;
            }
            let key_x = (before as f64 + 1.0) * white_key_w - black_key_w / 2.0;
            if x >= key_x && x <= key_x + black_key_w {
                return Some(note);
            }
        }
    }

    // White key by index. (All black keys were already checked above, so the
    // Python "not within an adjacent black key" re-check is vacuous here.)
    let idx = (x / white_key_w) as usize;
    if idx >= WHITE_KEYS {
        return None;
    }
    let mut wi = 0usize;
    for note in NOTE_START..=NOTE_END {
        if !is_white(note) {
            continue;
        }
        if wi == idx {
            return Some(note);
        }
        wi += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;


    /// **Every note name is legible, on every theme, in BOTH modes.**
    ///
    /// The owner's screenshot: names printed in two fixed greys chosen for
    /// the bone keyboard this app shipped with. A theme picking its own key
    /// colours breaks that, and dark mode breaks it twice over, because the
    /// naturals and the sharps swap — so the grey meant for ivory lands on a
    /// near-black key and vice versa.
    ///
    /// Four fills per keyboard (natural and sharp, idle and lit) times two
    /// modes times every factory theme. 4.5:1 throughout, the same bar the
    /// strip faces answer to.
    #[test]
    fn every_theme_prints_readable_note_names_in_both_modes() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut s = Settings::default();
                theme.apply(&mut s);
                s.dark_mode = dark;
                let (white_idle, black_idle) = idle_colors(&s);
                let cases = [
                    ("natural idle", white_idle),
                    ("sharp idle", black_idle),
                    ("natural lit", s.white_key_active_color.to_color32()),
                    ("sharp lit", s.black_key_active_color.to_color32()),
                    ("sustain", s.sustain_color.to_color32()),
                ];
                for (what, bg) in cases {
                    let ink = if crate::recorder_panel::wants_light_ink(bg) {
                        crate::recorder_panel::INK_LIGHT
                    } else {
                        crate::recorder_panel::INK_DARK
                    };
                    let ratio = crate::recorder_panel::contrast_ratio(bg, ink);
                    if ratio < 4.5 {
                        bad.push(format!(
                            "{} {}: {what} {ratio:.2}:1",
                            theme.name,
                            if dark { "dark" } else { "light" },
                        ));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "note names unreadable:\n  {}", bad.join("\n  "));
    }

    /// **The keyboard must not collapse into its own ground, in either
    /// mode.**
    ///
    /// Dark mode SWAPS the keys (`idle_colors`): the naturals take the
    /// sharps' colour, which is why on every theme they equal the theory
    /// ground (`themed_bg`, the sharps' near-black), so the SHARPS — the
    /// inverted naturals — are what carry the keyboard. On the sixteen light
    /// themes the sharps invert to a bright ivory and stand 9–15:1 clear of
    /// the ground. The three dark-designed themes shipped naming only
    /// `dark.background`, so their already-dark `white_key` became a
    /// near-black sharp and BOTH idle key colours fell onto the ground at
    /// ~1.1:1: the whole piano band read as one flat mask, only in dark mode.
    /// The note-name sweep above never caught it, because dark-on-dark keys
    /// still take a legible ink — it took a screenshot. This is the test that
    /// screenshot is worth.
    ///
    /// **And it only ever walked DARK, which is how the same fault survived
    /// in the other mode for a whole release.** The owner, on the two themes
    /// this test was written for: "Phosphor and Nocturne both still have the
    /// dark mode mask over the keyboard issue… the mask is in light mode."
    /// In light mode there is no swap to go wrong — the naturals are simply
    /// `white_key` and the ground is `background` — and both of those themes
    /// set the two to the same value, so the band was one flat slab at
    /// 1.10:1 and 1.04:1 while every other theme sat at 9–15:1. A sweep
    /// written to catch exactly this shape, in the one mode where it was not
    /// happening. It walks both now.
    ///
    /// **The floor was 2:1 and 1.25:1, and those numbers were what let the
    /// fault come back a third time.** They were chosen when the dark-mode
    /// lift did not exist, on the reading that a theme naming its own dark
    /// keyboard had spoken and that a key light enough for 3:1 on a
    /// 0x05–0x0d ground would cross into the dead zone where its own note
    /// name stops clearing 4.5:1 (the sweep above). The first half was
    /// wrong — naming a colour says where a theme wants its keys, not that
    /// the pair it named can be told apart — and it left Nocturne and
    /// Phosphor sitting at 2.65:1 and 2.75:1 on their grounds with 1.81:1
    /// and 2.09:1 between the keys, passing a test written for exactly this
    /// shape while the owner photographed the mask a third time. The second
    /// half is answered by the note-name sweep, which runs on the lifted
    /// colours and would say so.
    ///
    /// So the floor is now where the themes actually are. Every one of the
    /// seventeen, in both modes, clears 9.34:1 on its ground and 9.34:1
    /// between its keys — Sandstone is the narrowest and Iris the widest at
    /// 15.29:1 — and the bar sits at half of that. It is still a FLOOR and
    /// not a target: nothing in the set is near it, and anything that
    /// arrives near it is a keyboard somebody cannot see.
    /// **Every keyboard is the right way up, on every theme, in both modes.**
    ///
    /// Sixteen of the eighteen get this for nothing: a pale light ground makes
    /// pale naturals and the sharps come out near-black. The two dark-designed
    /// themes do not — Nocturne's and Phosphor's LIGHT grounds are 0.011 and
    /// 0.004 luminance, which is what makes them what they are — so their
    /// naturals were the dark key and the lift made the sharps the pale one.
    ///
    /// That is not a dim keyboard, it is an INVERTED one, and it was inverted
    /// in the mode where all sixteen others are the right way up. The owner:
    /// "they are the only themes with dark keyboards in light mode, and light
    /// keyboards in dark mode. Please pay attention and compare them to the
    /// other themes."
    ///
    /// Measured across the set before the fix: light-mode naturals ran 0.607
    /// to 0.965 on the sixteen and 0.011 / 0.004 on those two. The rule is
    /// asserted for all of them rather than for the pair, so a new
    /// dark-designed theme cannot join them quietly.
    ///
    /// Revert check performed: return the pair unswapped from
    /// `keys_the_right_way_up` and both themes name themselves in light mode.
    #[test]
    fn every_theme_paints_its_naturals_the_right_way_up() {
        use crate::recorder_panel::luminance;
        let mut wrong: Vec<String> = Vec::new();
        for (theme, dark) in crate::theme::factory()
            .into_iter()
            .flat_map(|t| [(t.clone(), false), (t, true)])
        {
            let mut s = Settings::default();
            theme.apply(&mut s);
            s.dark_mode = dark;
            let (naturals, sharps) = idle_colors(&s);
            let naturals_paler = luminance(naturals) > luminance(sharps);
            // Light mode: the naturals are the pale key. Dark mode: they are
            // the dark one, which is the inversion the whole set already wears.
            if naturals_paler == dark {
                wrong.push(format!(
                    "{} {}: naturals {:.3}, sharps {:.3}",
                    theme.name,
                    if dark { "dark" } else { "light" },
                    luminance(naturals),
                    luminance(sharps)
                ));
            }
        }
        assert!(
            wrong.is_empty(),
            "these keyboards are upside down against every other theme:\n  {}",
            wrong.join("\n  ")
        );
    }

    /// **A key the piano DERIVES never glares.**
    ///
    /// The lift walks the sharps away from the naturals until they are far
    /// enough apart, and it used to walk toward pure white. On the two themes
    /// it exists for, no amount of white is far enough — their dark naturals
    /// sit at about 0.095 luminance — so the walk ran every step and arrived
    /// at `#ffffff`, a key at the screen's limit beside a middling ground.
    /// The owner, on the build that finally had a visible keyboard on those
    /// themes: "their white key colors are too bright".
    ///
    /// So a derived key is capped at the pale this desk already paints with.
    /// Only a DERIVED one: a theme that named its own pale key keeps it, and
    /// Centennial's sits above this line on purpose.
    ///
    /// Revert check performed: walk toward `Color32::WHITE` again and both
    /// dark-designed themes name themselves here.
    #[test]
    fn a_derived_key_is_never_brighter_than_the_desks_own_ink() {
        use crate::recorder_panel::{luminance, INK_LIGHT};
        let ceiling = luminance(INK_LIGHT);
        let mut loud: Vec<String> = Vec::new();
        for (theme, dark) in crate::theme::factory()
            .into_iter()
            .flat_map(|t| [(t.clone(), false), (t, true)])
        {
            let mut s = Settings::default();
            theme.apply(&mut s);
            s.dark_mode = dark;
            // What the theme NAMED for the sharps in this mode, before the
            // lift: the light mode's own field, the dark mode's role or the
            // swap it falls back to.
            let named = if dark {
                s.dark_role("black_key")
                    .map_or(s.white_key_idle_color, |c| c)
                    .to_color32()
            } else {
                s.black_key_idle_color.to_color32()
            };
            let (_, painted) = idle_colors(&s);
            if painted == named {
                continue;
            }
            if luminance(painted) > ceiling + 1.0e-6 {
                loud.push(format!(
                    "{} {}: derived key at {:.3} luminance, past the desk's own {:.3}",
                    theme.name,
                    if dark { "dark" } else { "light" },
                    luminance(painted),
                    ceiling
                ));
            }
        }
        assert!(
            loud.is_empty(),
            "the piano derived a key brighter than anything else it paints:\n  {}",
            loud.join("\n  ")
        );
    }

    /// **The naturals ARE the page, and in dark mode two themes were not.**
    ///
    /// The theory page sits on the naturals' colour — the owner's own rule,
    /// checked from the theme table by
    /// `theme::the_theory_ground_is_the_keyboard_not_the_band`. Checked from
    /// the theme TABLE, which is why it never noticed: it compares the fields
    /// a theme declares, and a theme can declare `background` and then hand
    /// the piano a different natural through `dark.white_key`. Sixteen themes
    /// never did, so the two colours agreed everywhere anybody looked.
    ///
    /// Nocturne and Phosphor did. Their dark naturals came out at 0.099 and
    /// 0.093 luminance over pages of 0.006 and 0.002, while the other sixteen
    /// paint theirs exactly ON the page at 0.010 to 0.014 — so on those two,
    /// and only those two, the dark keyboard was a mid slab hanging above its
    /// own ground with bone sharps on top of it. That is the owner's "Dark
    /// mode - bright (and probably wrong) Keyboard", and it is not a
    /// brightness fault at heart: it is a seam. The band and the page below it
    /// were made of different colours.
    ///
    /// Asserted as equality rather than as a contrast floor, because that is
    /// what the rule actually says. A ratio would have let 0.093 on 0.002
    /// through under any bar loose enough to be arguable.
    ///
    /// Revert check performed: put `dark.white_key`/`dark.black_key` back on
    /// either theme and it names itself here.
    #[test]
    fn every_theme_paints_its_dark_naturals_on_its_own_page() {
        let mut adrift: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            let mut s = Settings::default();
            theme.apply(&mut s);
            s.dark_mode = true;
            let naturals = idle_colors(&s).0;
            let page = themed_bg(&s);
            if naturals != page {
                adrift.push(format!(
                    "{}: naturals {:?}, page {:?}",
                    theme.name, naturals, page
                ));
            }
        }
        assert!(
            adrift.is_empty(),
            "the keyboard is not made of the page it sits on:\n  {}",
            adrift.join("\n  ")
        );
    }

    #[test]
    fn every_theme_keeps_a_legible_keyboard_in_both_modes() {
        use crate::recorder_panel::contrast_ratio;
        let mut bad: Vec<String> = Vec::new();
        for (theme, dark) in crate::theme::factory()
            .into_iter()
            .flat_map(|t| [(t.clone(), false), (t, true)])
        {
            let mut s = Settings::default();
            theme.apply(&mut s);
            s.dark_mode = dark;
            let (naturals, sharps) = idle_colors(&s);
            let ground = themed_bg(&s);
            let shows =
                contrast_ratio(naturals, ground).max(contrast_ratio(sharps, ground));
            let distinct = contrast_ratio(naturals, sharps);
            if shows < 4.5 || distinct < 3.0 {
                bad.push(format!(
                    "{} {}: keyboard {shows:.2}:1 on its ground, keys {distinct:.2}:1 apart",
                    theme.name,
                    if dark { "dark" } else { "light" }
                ));
            }
        }
        assert!(
            bad.is_empty(),
            "the keyboard collapses into its ground:\n  {}",
            bad.join("\n  ")
        );

        // **And the lift is INERT where it is not needed.** `keys_apart` is a
        // derivation, and a derivation that quietly repaints seventeen themes
        // to fix two is worse than the fault. Every theme whose light sharps
        // already clear the line keeps exactly the colour it named.
        for theme in crate::theme::factory() {
            let mut s = Settings::default();
            theme.apply(&mut s);
            s.dark_mode = false;
            let named = s.black_key_idle_color.to_color32();
            let painted = idle_colors(&s).1;
            let clear = contrast_ratio(named, s.white_key_idle_color.to_color32())
                >= KEYS_APART;
            if clear {
                assert_eq!(
                    painted, named,
                    "{}: a theme already clear of the line was repainted",
                    theme.name
                );
            } else {
                assert_ne!(
                    painted, named,
                    "{}: a theme whose keys sit on top of each other was left there",
                    theme.name
                );
            }
        }
        // The two that need it are the two dark-designed themes, and nobody
        // else — named, so this notices if a new theme joins them silently.
        let lifted: Vec<String> = crate::theme::factory()
            .into_iter()
            .filter(|theme| {
                let mut s = Settings::default();
                theme.apply(&mut s);
                s.dark_mode = false;
                idle_colors(&s).1 != s.black_key_idle_color.to_color32()
            })
            .map(|t| t.name)
            .collect();
        assert_eq!(
            lifted,
            ["Nocturne", "Phosphor"],
            "a different set of themes needs the sharps lifted now"
        );
    }

    #[test]
    fn there_are_52_white_and_36_black_keys() {
        let whites = (NOTE_START..=NOTE_END).filter(|&n| is_white(n)).count();
        let blacks = (NOTE_START..=NOTE_END)
            .filter_map(white_keys_before)
            .count();
        assert_eq!(whites, 52);
        assert_eq!(blacks, 36);
    }

    #[test]
    fn white_keys_before_formula_matches_counting() {
        // `whiteKeysBefore` is the index of the white key to the black key's
        // LEFT, i.e. (number of white notes strictly below it) - 1.
        for note in NOTE_START..=NOTE_END {
            if is_white(note) {
                continue;
            }
            let below = (NOTE_START..note).filter(|&n| is_white(n)).count();
            assert_eq!(
                white_keys_before(note),
                Some(below - 1),
                "whiteKeysBefore mismatch for note {note}"
            );
        }
    }

    /// The white keys must TILE: no pixel between two of them may belong to
    /// neither. A truncated fixed width leaves a gap wherever the fractional
    /// part of the key pitch rolls over, which showed up as twelve grey
    /// slivers on a 1625pt keyboard.
    #[test]
    fn white_keys_tile_with_no_gaps_at_any_window_size() {
        for w in [
            650.0_f64, 975.0, 1300.0, 1625.0, 1950.0, 2275.0, 2600.0, 1000.0, 1301.0,
        ] {
            let wkw = w / WHITE_KEYS as f64;
            let mut prev_end: Option<f32> = None;
            for idx in 0..WHITE_KEYS {
                let x0 = (idx as f64 * wkw).trunc() as f32;
                let x1 = ((idx + 1) as f64 * wkw).trunc() as f32;
                assert!(x1 > x0, "key {idx} is empty at width {w}");
                if let Some(end) = prev_end {
                    assert_eq!(x0, end, "gap before key {idx} at width {w}");
                }
                prev_end = Some(x1);
            }
            // And the last key ends exactly at the right edge.
            assert_eq!(
                prev_end,
                Some(w.trunc() as f32),
                "keyboard short at width {w}"
            );
        }
    }

    /// **Every key answers to the place it is drawn.**
    ///
    /// The hit test and the painter compute the same geometry twice, in two
    /// functions, and nothing but this makes them agree. A drift of one white
    /// key sounds and lights the note next door; a drift of seven is an
    /// octave, which is the shape of "I played one note and another showed
    /// up" — and it would look completely deliberate on screen.
    #[test]
    fn every_key_hit_tests_back_to_itself() {
        for (w, h) in [(1300.0_f32, 150.0_f32), (900.0, 120.0), (620.0, 90.0)] {
            let white_key_w = f64::from(w) / WHITE_KEYS as f64;
            let mut idx = 0usize;
            for note in NOTE_START..=NOTE_END {
                if !is_white(note) {
                    continue;
                }
                // The middle of the key as the PAINTER lays it out, and low
                // enough to be under the black keys' reach.
                let x0 = (idx as f64 * white_key_w).trunc();
                let x1 = ((idx + 1) as f64 * white_key_w).trunc();
                let x = ((x0 + x1) * 0.5) as f32;
                let y = h * 0.92;
                assert_eq!(
                    hit_test(x, y, w, h),
                    Some(note),
                    "at {w}x{h} the white key drawn for {note} answers to \
                     something else"
                );
                idx += 1;
            }
            // And the black keys, at their own centres and above the white
            // keys' reach.
            for note in NOTE_START..=NOTE_END {
                if is_white(note) {
                    continue;
                }
                let Some(before) = white_keys_before(note) else {
                    continue;
                };
                if !(before < WHITE_KEYS && before + 1 < WHITE_KEYS) {
                    continue;
                }
                let x = ((before as f64 + 1.0) * white_key_w) as f32;
                assert_eq!(
                    hit_test(x, h * 0.2, w, h),
                    Some(note),
                    "at {w}x{h} the black key drawn for {note} answers to \
                     something else"
                );
            }
        }
    }

    #[test]
    fn hit_test_matches_black_first_semantics() {
        let (w, h) = (1300.0_f32, 150.0_f32);
        let wkw = w as f64 / 52.0;
        // Dead center of the boundary between white keys 0 (A0) and 1 (B0)
        // above black-key height => A#0 (note 22).
        let bx = 1.0 * wkw;
        assert_eq!(hit_test(bx as f32, 10.0, w, h), Some(22));
        // Same x below black-key height => the white key under it (B0, idx 1).
        assert_eq!(hit_test(bx as f32 + 1.0, h * 0.9, w, h), Some(23));
        // Far left bottom => A0.
        assert_eq!(hit_test(1.0, h - 1.0, w, h), Some(21));
        // Far right => C8.
        assert_eq!(hit_test(w - 1.0, h - 1.0, w, h), Some(108));
        // Outside.
        assert_eq!(hit_test(-1.0, 10.0, w, h), None);
        assert_eq!(hit_test(w + 1.0, 10.0, w, h), None);
    }
}
