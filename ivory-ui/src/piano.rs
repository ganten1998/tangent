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
        return (white, black);
    }
    // **What the theme SAYS, before what dark mode would derive.**
    //
    // The swap below is the derivation, and it is a good one: the owner asked
    // for a pure inversion to ivory on black and that is what it gives. But a
    // theme that names its own dark keyboard means it, and a theme is allowed
    // to disagree with the transform — that is what naming a colour is for.
    (
        s.dark_role("white_key").map_or(black, |c| c.to_color32()),
        s.dark_role("black_key").map_or(white, |c| c.to_color32()),
    )
}

/// Blend two colours, `t` of the way from `a` to `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn bg_color(dark_mode: bool) -> Color32 {
    if dark_mode {
        Color32::from_rgb(0x1a, 0x1a, 0x1a)
    } else {
        Color32::from_rgb(0xE8, 0xE8, 0xE8)
    }
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
    painter.rect_filled(rect, 0.0, bg_color(s.dark_mode));

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
    let size = (white_key_w as f32 * 0.46).min(rect.height() * 0.12);
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
        ivory_core::patterns::NOTE_NAMES_FLAT[i]
    } else {
        ivory_core::patterns::NOTE_NAMES[i]
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
