//! The MIDI editor: a piano roll in a popup over the arrangement.
//!
//! A pure painter with a layout and a hit test, like every panel — the app
//! owns the document and the gestures; this file owns where things are and
//! what a press means. The one piece of real machinery is the GRID, which
//! has three shapes — straight, swing, triplet — and [`snap_t`] is the one
//! authority for all three: the drawn lines and the magnet come from the
//! same arithmetic, so what you see is exactly what a note lands on.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use crate::arrangement::ArrNote;
use crate::mixer_panel::STRIP_COLORS;

/// The top strip's height: title, the grid tools, the ×.
pub const STRIP_H: f32 = 26.0;
/// The piano column on the left.
// 66, up from 46: the drum lane writes its pad NAMES on the keys now,
// and they need the room to say a word.
pub const KEYS_W: f32 = 66.0;
/// Keys visible at once — the roll's vertical span.
pub const KEYS_SHOWN: f32 = 36.0;
/// The swing presets the SWG menu offers, exactly as the owner listed
/// them: the word name, the ratio in parentheses, and where the off-beat
/// line sits as a percentage of its pair.
pub const SWING_PRESETS: [(&str, &str, f64); 6] = [
    ("Straight", "1:1", 50.0),
    ("Septuplet", "4:3", 57.1),
    ("Quintuplet", "3:2", 60.0),
    ("Golden Ratio", "1 : 0.618", 61.8),
    ("Triplet (Shuffle)", "2:1", 66.7),
    ("Dotted (Hard)", "3:1", 75.0),
];
/// The right-edge grab zone of a note, in points.
const EDGE_W: f32 = 6.0;

/// Everything the editor draws, pushed in. Times are ABSOLUTE timeline
/// seconds — the clip's own trim (`offset_s`) is paid at the boundary, so
/// the roll and the arrangement agree where a bar stands.
pub struct MidiEditView<'a> {
    pub name: &'a str,
    pub clip_start_s: f64,
    pub clip_len_s: f64,
    pub offset_s: f64,
    pub notes: &'a [ArrNote],
    /// The SELECTED notes, sorted indices — the editor works on sets now.
    pub selected: &'a [usize],
    /// A marquee in flight, in screen space, drawn as a dashed box.
    pub marquee: Option<(Pos2, Pos2)>,
    /// The window, absolute seconds.
    pub view_start_s: f64,
    pub view_len_s: f64,
    /// The (fractional) key whose row TOP sits at the roll's top edge.
    pub top_key: f32,
    pub snap_on: bool,
    pub grid_exp: i8,
    pub swing: bool,
    /// Where the off-beat line sits, as a fraction of its pair — 0.667 is
    /// the shuffle everybody means by "swing". Only read while `swing`.
    pub swing_pct: f64,
    pub triplet: bool,
    pub bpm: f64,
    pub sig_top: u32,
    pub sig_bottom: u32,
    pub playhead_s: f64,
    /// The lane's colour index into [`STRIP_COLORS`]; 0 draws the desk
    /// green, exactly as the arrangement does.
    pub color: usize,
    /// The editor's own DARK flag: warm browns on, the arrange greys off.
    pub dark: bool,
    /// On the DRUM LANE: the sixteen pads' sample names, keyed 36..=51 —
    /// written on the keys, because a kit whose keys are anonymous is not
    /// an instrument. `None` everywhere else.
    pub pad_names: Option<[&'a str; 128]>,
}

/// What a press on the editor means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Close,
    /// The pop-out beside the ×: float the editor over the lanes, or give
    /// it the whole view back.
    Detach,
    ToolSnap,
    ToolGrid,
    ToolSwing,
    ToolTriplet,
    /// A note's body — select and move. The index into the clip's notes.
    NoteBody(usize),
    /// A note's right edge — resize the tail.
    NoteRight(usize),
    /// A note's LEFT edge — resize the head, snapped like the tail.
    NoteLeft(usize),
    /// A piano key in the left column — audition.
    Key(u8),
    /// Empty roll: `(absolute seconds, key)`.
    Cell(f64, u8),
    /// The bar ruler: locate the playhead here.
    Ruler(f64),
    /// The velocity lane at `absolute seconds` — the app finds the note.
    Vel(f64),
}

pub struct Layout {
    pub panel: Rect,
    pub strip: Rect,
    /// The bar ruler under the strip: numbers, ticks, and a click locates.
    pub ruler: Rect,
    /// The velocity lane along the foot: one bar per note.
    pub vel: Rect,
    pub close: Rect,
    pub detach: Rect,
    pub tool_snap: Rect,
    pub tool_grid: Rect,
    pub tool_swing: Rect,
    pub tool_triplet: Rect,
    pub keys: Rect,
    pub roll: Rect,
}

impl Layout {
    pub fn new(rect: Rect) -> Self {
        if !rect.is_positive() || rect.width() < 60.0 || rect.height() < 40.0 {
            return Self {
                panel: Rect::NOTHING,
                strip: Rect::NOTHING,
                detach: Rect::NOTHING,
                ruler: Rect::NOTHING,
                vel: Rect::NOTHING,
                close: Rect::NOTHING,
                tool_snap: Rect::NOTHING,
                tool_grid: Rect::NOTHING,
                tool_swing: Rect::NOTHING,
                tool_triplet: Rect::NOTHING,
                keys: Rect::NOTHING,
                roll: Rect::NOTHING,
            };
        }
        let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), STRIP_H));
        // The tools hang on the strip's right: × at the very edge, then the
        // gridbox, the magnet, and the two grid-shape chips walking left.
        let pad = 4.0;
        let side = STRIP_H - pad * 2.0;
        let mut x = strip.right() - pad;
        let mut cell = |w: f32| {
            let r = Rect::from_min_max(
                Pos2::new(x - w, strip.top() + pad),
                Pos2::new(x, strip.bottom() - pad),
            );
            x -= w + pad;
            r
        };
        let close = cell(side);
        let detach = cell(side);
        let tool_grid = cell(side * 2.2);
        let tool_snap = cell(side * 1.9);
        let tool_triplet = cell(side * 1.6);
        let tool_swing = cell(side * 1.6);
        // Strip, then the RULER, then keys+roll, then the VELOCITY lane.
        let ruler = Rect::from_min_size(
            Pos2::new(rect.left(), strip.bottom()),
            Vec2::new(rect.width(), 15.0),
        );
        let vel_h = (rect.height() * 0.16).clamp(24.0, 64.0);
        let body = Rect::from_min_max(
            Pos2::new(rect.left(), ruler.bottom()),
            Pos2::new(rect.right(), rect.bottom() - vel_h),
        );
        let keys = Rect::from_min_size(body.min, Vec2::new(KEYS_W.min(body.width() * 0.2), body.height()));
        let roll = Rect::from_min_max(Pos2::new(keys.right(), body.top()), body.max);
        let vel = Rect::from_min_max(
            Pos2::new(roll.left(), body.bottom() + 1.0),
            Pos2::new(roll.right(), rect.bottom() - 2.0),
        );
        Self {
            panel: rect,
            strip,
            ruler,
            vel,
            close,
            detach,
            tool_snap,
            tool_grid,
            tool_swing,
            tool_triplet,
            keys,
            roll,
        }
    }
}

// ── the axes ───────────────────────────────────────────────────────────────

pub fn time_at(l: &Layout, view: &MidiEditView<'_>, x: f32) -> f64 {
    let w = f64::from(l.roll.width().max(1.0));
    view.view_start_s + f64::from(x - l.roll.left()) / w * view.view_len_s.max(1.0e-9)
}

pub fn x_of(l: &Layout, view: &MidiEditView<'_>, t: f64) -> f32 {
    let f = (t - view.view_start_s) / view.view_len_s.max(1.0e-9);
    l.roll.left() + l.roll.width() * f as f32
}

/// The key row height in points.
pub fn key_h(l: &Layout) -> f32 {
    l.roll.height().max(1.0) / KEYS_SHOWN
}

/// The (fractional) key under `y` — floor it for the row.
pub fn key_at(l: &Layout, view: &MidiEditView<'_>, y: f32) -> f32 {
    view.top_key - (y - l.roll.top()) / key_h(l)
}

/// The TOP edge of `key`'s row.
pub fn y_of(l: &Layout, view: &MidiEditView<'_>, key: f32) -> f32 {
    l.roll.top() + (view.top_key - key) * key_h(l)
}

// ── the grid ───────────────────────────────────────────────────────────────

fn bar_s(view: &MidiEditView<'_>) -> f64 {
    let bpm = view.bpm.clamp(20.0, 400.0);
    let top = view.sig_top.clamp(1, 32) as f64;
    let bottom = view.sig_bottom.clamp(1, 32) as f64;
    top * (60.0 / bpm) * (4.0 / bottom)
}

/// One grid step in seconds. Triplet divides the next size UP into three —
/// an eighth-triplet is a third of a quarter — which is the 2/3 here.
pub fn grid_step_s(view: &MidiEditView<'_>) -> f64 {
    let g = bar_s(view) * 2.0f64.powi(i32::from(view.grid_exp.clamp(-6, 3)));
    if view.triplet {
        g * 2.0 / 3.0
    } else {
        g
    }
}

/// `t`, magnetised to the grid's own lines — straight, triplet or swung.
/// The ONE authority: the drawn lines call the same maths.
pub fn snap_t(view: &MidiEditView<'_>, t: f64) -> f64 {
    if !view.snap_on {
        return t.max(0.0);
    }
    let step = grid_step_s(view);
    if step <= 0.0 {
        return t.max(0.0);
    }
    if view.swing && !view.triplet {
        // Pairs of steps: the on-beat at the pair's head, the off-beat
        // pushed late to SWING of the pair. Candidates straddle `t`.
        let pair = step * 2.0;
        let base = (t / pair).floor() * pair;
        let mut best = base;
        for c in [base, base + pair * view.swing_pct.clamp(0.5, 0.95), base + pair] {
            if (t - c).abs() < (t - best).abs() {
                best = c;
            }
        }
        best.max(0.0)
    } else {
        ((t / step).round() * step).max(0.0)
    }
}

/// As [`snap_t`], but FLOOR: the grid line at or before `t`. Note
/// CREATION uses it — a click lands in the cell under the hand, never the
/// next one, however close to the boundary. Swing floors to the last
/// candidate at or before `t`.
pub fn snap_floor_t(view: &MidiEditView<'_>, t: f64) -> f64 {
    if !view.snap_on {
        return t.max(0.0);
    }
    let step = grid_step_s(view);
    if step <= 0.0 {
        return t.max(0.0);
    }
    if view.swing && !view.triplet {
        let pair = step * 2.0;
        let base = (t / pair).floor() * pair;
        let mut best = base;
        for c in [base, base + pair * view.swing_pct.clamp(0.5, 0.95), base + pair] {
            if c <= t + 1.0e-9 && c > best {
                best = c;
            }
        }
        best.max(0.0)
    } else {
        ((t / step).floor() * step).max(0.0)
    }
}

/// The first grid line STRICTLY after `t` — shape-aware, so a note drawn
/// on the on-beat of a swung pair runs long to the off-beat line and one
/// drawn on the off-beat runs short to the next pair. Durations and
/// nudges walk the same lines the magnet lands on.
pub fn snap_next_t(view: &MidiEditView<'_>, t: f64) -> f64 {
    let step = grid_step_s(view);
    if step <= 0.0 {
        return t;
    }
    if view.snap_on && view.swing && !view.triplet {
        let pair = step * 2.0;
        let base = (t / pair).floor() * pair;
        for c in [
            base + pair * view.swing_pct.clamp(0.5, 0.95),
            base + pair,
            base + pair + pair * view.swing_pct.clamp(0.5, 0.95),
        ] {
            if c > t + 1.0e-9 {
                return c;
            }
        }
        base + pair * 2.0
    } else {
        ((t / step).floor() + 1.0) * step
    }
}

/// Every grid line in `[from, to]`: `f(t, is_bar)`. Bounded, so a zoomed-out
/// window cannot ask for ten thousand lines.
pub fn grid_lines(view: &MidiEditView<'_>, from: f64, to: f64, mut f: impl FnMut(f64, bool)) {
    let bar = bar_s(view);
    let step = grid_step_s(view);
    if bar <= 0.0 || step <= 0.0 || to <= from {
        return;
    }
    if (to - from) / step > 2000.0 {
        // Too fine to see anyway: bars only.
        let mut n = (from / bar).floor().max(0.0);
        while n * bar <= to && n < 100_000.0 {
            f(n * bar, true);
            n += 1.0;
        }
        return;
    }
    if view.swing && !view.triplet {
        let pair = step * 2.0;
        let mut n = (from / pair).floor().max(0.0);
        while n * pair <= to {
            let base = n * pair;
            f(base, (base / bar).round() * bar == base || base.rem_euclid(bar) < 1.0e-9);
            f(base + pair * view.swing_pct.clamp(0.5, 0.95), false);
            n += 1.0;
        }
    } else {
        let mut n = (from / step).floor().max(0.0);
        while n * step <= to {
            let t = n * step;
            f(t, t.rem_euclid(bar) < 1.0e-6 || bar - t.rem_euclid(bar) < 1.0e-6);
            n += 1.0;
        }
    }
}

/// The gridbox's word: the arrangement's bars-as-a-power-of-two, with the
/// grid's shape appended — `1/8T`, `1/8 sw`.
pub fn grid_word(view: &MidiEditView<'_>) -> String {
    let base = crate::arrangement_panel::grid_label(view.grid_exp);
    if view.triplet {
        format!("{base}T")
    } else if view.swing {
        format!("{base} sw")
    } else {
        base
    }
}

// ── drawing ────────────────────────────────────────────────────────────────

const FACE: Color32 = Color32::from_rgb(0x2A, 0x26, 0x21);
const FACE_ROW_DARK: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 60);
const INK: Color32 = Color32::from_rgb(0xFF, 0xF4, 0xE0);
const OUTSIDE_VEIL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 90);

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}

fn lane_color(view: &MidiEditView<'_>) -> Color32 {
    if view.color != 0 {
        let (r, g, b) = STRIP_COLORS[view.color.min(STRIP_COLORS.len() - 1)];
        Color32::from_rgb(r, g, b)
    } else {
        Color32::from_rgb(0x2E, 0x6B, 0x46)
    }
}

pub fn draw(painter: &Painter, rect: Rect, view: &MidiEditView<'_>) {
    let l = Layout::new(rect);
    if !l.panel.is_positive() {
        return;
    }
    // **Two faces, the owner's pair**: dark keeps the editor's shipped
    // warm browns; light borrows the arrangement's greys, so the two
    // views share one wardrobe and D swaps it per view.
    let face = if view.dark {
        FACE
    } else {
        Color32::from_rgb(0x46, 0x46, 0x46)
    };
    // A popup over the arrangement: its own face, a firm border, and a
    // clip so nothing leaks onto the lanes below.
    let painter = &painter.with_clip_rect(l.panel.expand(1.0));
    painter.rect_filled(l.panel, 4.0, face);
    painter.rect_stroke(
        l.panel,
        4.0,
        Stroke::new(1.5, INK.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );

    // ── the strip ─────────────────────────────────────────────────────────
    let font = |size: f32| FontId::monospace(size);
    painter.rect_filled(l.strip, 4.0, Color32::from_rgba_premultiplied(0, 0, 0, 60));
    painter.text(
        Pos2::new(l.strip.left() + 8.0, l.strip.center().y),
        Align2::LEFT_CENTER,
        view.name,
        font(STRIP_H * 0.45),
        INK,
    );
    let chip = |painter: &Painter, r: Rect, word: &str, lit: bool| {
        if !r.is_positive() {
            return;
        }
        if lit {
            painter.rect_filled(r, 3.0, INK.gamma_multiply(0.85));
        }
        painter.rect_stroke(
            r,
            3.0,
            Stroke::new(1.0, INK.gamma_multiply(if lit { 0.9 } else { 0.4 })),
            egui::StrokeKind::Inside,
        );
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            word,
            font(r.height() * 0.55),
            if lit { face } else { INK.gamma_multiply(0.7) },
        );
    };
    chip(painter, l.tool_swing, "SWG", view.swing);
    chip(painter, l.tool_triplet, "TRP", view.triplet);
    // The magnet, as a word — the strip is text-first, and a lit chip
    // says ON just as loudly as a glyph would.
    chip(painter, l.tool_snap, "SNAP", view.snap_on);
    chip(painter, l.tool_grid, &grid_word(view), false);
    if l.close.is_positive() {
        let c = l.close.center();
        let a = l.close.width() * 0.28;
        for s in [-1.0f32, 1.0] {
            painter.line_segment(
                [Pos2::new(c.x - a, c.y - a * s), Pos2::new(c.x + a, c.y + a * s)],
                Stroke::new(1.5, INK.gamma_multiply(0.7)),
            );
        }
    }
    // The pop-out beside the ×: two offset squares, the universal sign.
    if l.detach.is_positive() {
        let a = l.detach.width() * 0.26;
        let c = l.detach.center();
        let ink = INK.gamma_multiply(0.7);
        painter.rect_stroke(
            Rect::from_min_size(Pos2::new(c.x - a, c.y - a * 0.4), Vec2::splat(a * 1.3)),
            1.0,
            Stroke::new(1.2, ink),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            Rect::from_min_size(Pos2::new(c.x - a * 0.3, c.y - a), Vec2::splat(a * 1.3)),
            1.0,
            Stroke::new(1.2, ink),
            egui::StrokeKind::Inside,
        );
    }

    if !l.roll.is_positive() || !l.keys.is_positive() {
        return;
    }
    let kh = key_h(&l);

    // ── rows: the piano column and the roll's shading, one loop ──────────
    let top = view.top_key.ceil() as i32;
    for k in (top - KEYS_SHOWN as i32 - 1)..=top {
        if !(0..=127).contains(&k) {
            continue;
        }
        let key = k as u8;
        let y0 = y_of(&l, view, k as f32);
        let row_keys = Rect::from_min_max(
            Pos2::new(l.keys.left(), y0.max(l.keys.top())),
            Pos2::new(l.keys.right(), (y0 + kh).min(l.keys.bottom())),
        );
        let row_roll = Rect::from_min_max(
            Pos2::new(l.roll.left(), y0.max(l.roll.top())),
            Pos2::new(l.roll.right(), (y0 + kh).min(l.roll.bottom())),
        );
        if row_keys.is_positive() {
            painter.rect_filled(
                row_keys,
                0.0,
                if is_black(key) {
                    Color32::from_rgb(0x1A, 0x16, 0x12)
                } else {
                    INK.gamma_multiply(0.85)
                },
            );
            if key % 12 == 0 && kh >= 7.0 {
                painter.text(
                    Pos2::new(row_keys.right() - 3.0, row_keys.center().y),
                    Align2::RIGHT_CENTER,
                    format!("C{}", i32::from(key / 12) - 1),
                    font((kh * 0.7).min(10.0)),
                    if is_black(key) { INK } else { FACE },
                );
            }
            // **The kit's own names on its keys** — inverse ink, so a
            // white key reads dark and a black key reads light. This is
            // what makes the drum lane an instrument you can SEE.
            if let Some(names) = view.pad_names.as_ref() {
                if kh >= 6.0 {
                    let name = names[usize::from(key.min(127))];
                    if !name.is_empty() {
                        painter.text(
                            Pos2::new(row_keys.left() + 3.0, row_keys.center().y),
                            Align2::LEFT_CENTER,
                            name,
                            font((kh * 0.62).clamp(6.0, 9.5)),
                            if is_black(key) { INK } else { FACE },
                        );
                    }
                }
            }
        }
        if row_roll.is_positive() {
            if is_black(key) {
                painter.rect_filled(row_roll, 0.0, FACE_ROW_DARK);
            }
            // Every C draws its line: the octave is the roll's ruler.
            if key % 12 == 0 {
                painter.line_segment(
                    [
                        Pos2::new(row_roll.left(), row_roll.bottom()),
                        Pos2::new(row_roll.right(), row_roll.bottom()),
                    ],
                    Stroke::new(1.0, INK.gamma_multiply(0.16)),
                );
            }
        }
    }
    painter.line_segment(
        [l.keys.right_top(), l.keys.right_bottom()],
        Stroke::new(1.0, INK.gamma_multiply(0.4)),
    );

    // ── the grid ─────────────────────────────────────────────────────────
    let t1 = view.view_start_s + view.view_len_s;
    grid_lines(view, view.view_start_s.max(0.0), t1, |t, is_bar| {
        let x = x_of(&l, view, t);
        if x < l.roll.left() - 1.0 || x > l.roll.right() + 1.0 {
            return;
        }
        painter.line_segment(
            [Pos2::new(x, l.roll.top()), Pos2::new(x, l.roll.bottom())],
            Stroke::new(
                1.0,
                INK.gamma_multiply(if is_bar { 0.30 } else { 0.10 }),
            ),
        );
    });

    // ── the clip's own span: everything outside it is not this clip ──────
    let cx0 = x_of(&l, view, view.clip_start_s);
    let cx1 = x_of(&l, view, view.clip_start_s + view.clip_len_s);
    if cx0 > l.roll.left() {
        painter.rect_filled(
            Rect::from_min_max(l.roll.min, Pos2::new(cx0.min(l.roll.right()), l.roll.bottom())),
            0.0,
            OUTSIDE_VEIL,
        );
    }
    if cx1 < l.roll.right() {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(cx1.max(l.roll.left()), l.roll.top()), l.roll.max),
            0.0,
            OUTSIDE_VEIL,
        );
    }

    // ── the notes ────────────────────────────────────────────────────────
    let color = lane_color(view);
    for (i, n) in view.notes.iter().enumerate() {
        let r = note_rect(&l, view, n);
        if !r.is_positive() {
            continue;
        }
        painter.rect_filled(r, 2.0, color);
        painter.rect_stroke(
            r,
            2.0,
            if view.selected.binary_search(&i).is_ok() {
                Stroke::new(2.0, INK)
            } else {
                Stroke::new(1.0, Color32::from_black_alpha(140))
            },
            egui::StrokeKind::Inside,
        );
    }

    // ── the ruler: bars numbered, beats ticked, one press a locate ──────
    if l.ruler.is_positive() {
        painter.rect_filled(l.ruler, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 70));
        let bar = {
            let bpm = view.bpm.clamp(20.0, 400.0);
            f64::from(view.sig_top.clamp(1, 32)) * (60.0 / bpm)
                * (4.0 / f64::from(view.sig_bottom.clamp(1, 32)))
        };
        if bar > 0.0 {
            let px_per_s = f64::from(l.roll.width()) / view.view_len_s.max(1.0e-9);
            let mut step = 1u64;
            while bar * step as f64 * px_per_s < 44.0 && step < 1 << 16 {
                step *= 2;
            }
            let mut n = (view.view_start_s / (bar * step as f64)).floor().max(0.0) as u64;
            loop {
                let t = n as f64 * bar * step as f64;
                if t > view.view_start_s + view.view_len_s {
                    break;
                }
                let x = x_of(&l, view, t);
                if x >= l.roll.left() - 0.5 && x <= l.roll.right() + 0.5 {
                    painter.line_segment(
                        [Pos2::new(x, l.ruler.bottom() - 4.0), Pos2::new(x, l.ruler.bottom())],
                        Stroke::new(1.0, INK.gamma_multiply(0.6)),
                    );
                    painter.text(
                        Pos2::new(x + 3.0, l.ruler.top() + 1.0),
                        Align2::LEFT_TOP,
                        format!("{}", n * step + 1),
                        font(9.0),
                        INK.gamma_multiply(0.55),
                    );
                }
                // **The beats between the numbers, as decimals** — 16, then
                // smaller: 16.2, 16.3, 16.4, until 17 — whenever a beat has
                // the room. Only at step 1: a decimated ruler has no beats.
                if step == 1 {
                    let beats = u64::from(view.sig_top.clamp(1, 32));
                    let beat_px = bar * px_per_s / beats as f64;
                    if beat_px >= 26.0 {
                        for b in 1..beats {
                            let tb = t + bar * b as f64 / beats as f64;
                            if tb > view.view_start_s + view.view_len_s {
                                break;
                            }
                            let xb = x_of(&l, view, tb);
                            if xb < l.roll.left() - 0.5 || xb > l.roll.right() + 0.5 {
                                continue;
                            }
                            painter.line_segment(
                                [
                                    Pos2::new(xb, l.ruler.bottom() - 2.5),
                                    Pos2::new(xb, l.ruler.bottom()),
                                ],
                                Stroke::new(1.0, INK.gamma_multiply(0.35)),
                            );
                            painter.text(
                                Pos2::new(xb + 2.0, l.ruler.top() + 2.5),
                                Align2::LEFT_TOP,
                                format!("{}.{}", n + 1, b + 1),
                                font(7.0),
                                INK.gamma_multiply(0.38),
                            );
                        }
                    }
                }
                n += 1;
            }
        }
    }

    // ── the velocity lane: one bar per note, selected ones lit ──────────
    if l.vel.is_positive() {
        painter.rect_filled(l.vel, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 70));
        let color = lane_color(view);
        for (i, n) in view.notes.iter().enumerate() {
            let t0 = view.clip_start_s + (n.at_s - view.offset_s);
            let x = x_of(&l, view, t0);
            if x < l.vel.left() || x > l.vel.right() {
                continue;
            }
            let h = l.vel.height() * (f32::from(n.vel.min(127)) / 127.0);
            let selected = view.selected.binary_search(&i).is_ok();
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x - 1.5, l.vel.bottom() - h),
                    Pos2::new(x + 1.5, l.vel.bottom()),
                ),
                0.5,
                if selected { INK } else { color },
            );
        }
    }

    // ── the marquee, while a hand draws one ──────────────────────────────
    if let Some((a, b)) = view.marquee {
        let r = Rect::from_two_pos(a, b);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, INK.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
        // UNmultiplied — the premultiplied form with RGB far above the
        // alpha composited near-additively and painted the band white;
        // same fix as the arrangement's ClipBand.
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 28));
    }

    // ── the playhead ─────────────────────────────────────────────────────
    let px = x_of(&l, view, view.playhead_s);
    if px >= l.roll.left() && px <= l.roll.right() {
        painter.line_segment(
            [Pos2::new(px, l.roll.top()), Pos2::new(px, l.roll.bottom())],
            Stroke::new(1.5, Color32::from_rgb(0xE8, 0x3A, 0x4E)),
        );
    }
}

/// A note's rectangle on the roll, clamped into it. `NOTHING` off screen.
pub fn note_rect(l: &Layout, view: &MidiEditView<'_>, n: &ArrNote) -> Rect {
    let t0 = view.clip_start_s + (n.at_s - view.offset_s);
    let t1 = t0 + n.dur_s.max(0.02);
    let x0 = x_of(l, view, t0);
    let x1 = x_of(l, view, t1);
    if x1 < l.roll.left() || x0 > l.roll.right() {
        return Rect::NOTHING;
    }
    let y0 = y_of(l, view, f32::from(n.key));
    let r = Rect::from_min_max(
        Pos2::new(x0.max(l.roll.left()), y0.max(l.roll.top())),
        Pos2::new(
            x1.min(l.roll.right()).max(x0 + 2.0),
            (y0 + key_h(l)).min(l.roll.bottom()),
        ),
    );
    if r.is_positive() {
        r.shrink2(Vec2::new(0.0, 0.5))
    } else {
        Rect::NOTHING
    }
}

/// What a press at `pos` means. Notes beat cells; late clips beat early
/// ones, like the arrangement.
pub fn hit_test(rect: Rect, view: &MidiEditView<'_>, pos: Pos2) -> Option<Hit> {
    let l = Layout::new(rect);
    if !l.panel.is_positive() || !l.panel.contains(pos) {
        return None;
    }
    for (r, hit) in [
        (l.close, Hit::Close),
        (l.detach, Hit::Detach),
        (l.tool_snap, Hit::ToolSnap),
        (l.tool_grid, Hit::ToolGrid),
        (l.tool_swing, Hit::ToolSwing),
        (l.tool_triplet, Hit::ToolTriplet),
    ] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    if l.strip.contains(pos) {
        return None;
    }
    if l.ruler.is_positive() && l.ruler.contains(pos) {
        return Some(Hit::Ruler(time_at(&l, view, pos.x)));
    }
    if l.vel.is_positive() && l.vel.contains(pos) {
        return Some(Hit::Vel(time_at(&l, view, pos.x)));
    }
    if l.keys.is_positive() && l.keys.contains(pos) {
        // Within key k's row, `key_at` runs (k-1, k] — the CEILING names
        // the row, not the floor.
        let key = key_at(&l, view, pos.y).ceil();
        if (0.0..=127.0).contains(&key) {
            return Some(Hit::Key(key as u8));
        }
        return None;
    }
    if !l.roll.is_positive() || !l.roll.contains(pos) {
        return None;
    }
    for (i, n) in view.notes.iter().enumerate().rev() {
        let r = note_rect(&l, view, n);
        if !r.is_positive() || !r.contains(pos) {
            continue;
        }
        // Edges only claim a hand when the BODY keeps a real share: on a
        // narrow note the whole face is the body, or a chord's grab turned
        // into a one-note resize — the owner's "only the top note moved".
        let edge = if r.width() >= 14.0 {
            EDGE_W.min(r.width() * 0.3)
        } else {
            0.0
        };
        if edge > 0.0 {
            if pos.x >= r.right() - edge {
                return Some(Hit::NoteRight(i));
            }
            if pos.x <= r.left() + edge {
                return Some(Hit::NoteLeft(i));
            }
        }
        return Some(Hit::NoteBody(i));
    }
    let key = key_at(&l, view, pos.y).ceil();
    if !(0.0..=127.0).contains(&key) {
        return None;
    }
    Some(Hit::Cell(time_at(&l, view, pos.x), key as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>(notes: &'a [ArrNote]) -> MidiEditView<'a> {
        MidiEditView {
            name: "MIDI",
            clip_start_s: 8.0,
            clip_len_s: 16.0,
            offset_s: 0.0,
            notes,
            selected: &[],
            marquee: None,
            view_start_s: 6.0,
            view_len_s: 20.0,
            top_key: 72.0,
            snap_on: true,
            grid_exp: -3,
            swing: false,
            swing_pct: 0.58,
            triplet: false,
            bpm: 120.0,
            sig_top: 4,
            sig_bottom: 4,
            playhead_s: 10.0,
            color: 0,
            dark: true,
            pad_names: None,
        }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(900.0, 380.0))
    }

    /// The axes are inverses, and the vertical one puts higher pitch higher.
    #[test]
    fn the_axes_round_trip_and_pitch_rises() {
        let n: [ArrNote; 0] = [];
        let v = view(&n);
        let l = Layout::new(rect());
        for t in [6.0, 8.0, 17.5, 26.0] {
            let x = x_of(&l, &v, t);
            assert!((time_at(&l, &v, x) - t).abs() < 1.0e-6);
        }
        assert!(
            y_of(&l, &v, 72.0) < y_of(&l, &v, 60.0),
            "C5 drew below C4"
        );
        let y = y_of(&l, &v, 60.0);
        assert!((key_at(&l, &v, y) - 60.0).abs() < 1.0e-4);
    }

    /// **The magnet's three shapes.** Straight rounds to the step; triplet
    /// divides the doubled step into three; swing pushes the off-beat line
    /// late — and every snapped value is one the grid actually draws.
    #[test]
    fn the_grid_snaps_straight_triplet_and_swung() {
        let n: [ArrNote; 0] = [];
        // 120 bpm, 4/4: bar 2 s; exp -3 → an eighth of a bar = 0.25 s.
        let mut v = view(&n);
        assert!((grid_step_s(&v) - 0.25).abs() < 1.0e-9);
        assert!((snap_t(&v, 0.13) - 0.25).abs() < 1.0e-9);
        assert!((snap_t(&v, 0.11) - 0.0).abs() < 1.0e-9);

        v.triplet = true;
        let t_step = 0.25 * 2.0 / 3.0;
        assert!((grid_step_s(&v) - t_step).abs() < 1.0e-9);
        assert!((snap_t(&v, 0.17) - t_step).abs() < 1.0e-9);

        v.triplet = false;
        v.swing = true;
        // Pairs of 0.5 s: lines at 0.0 and 0.29 (58% of the pair).
        assert!((snap_t(&v, 0.20) - 0.29).abs() < 1.0e-9);
        assert!((snap_t(&v, 0.05) - 0.0).abs() < 1.0e-9);
        assert!((snap_t(&v, 0.45) - 0.5).abs() < 1.0e-9);

        // Off, the magnet lets go entirely.
        v.snap_on = false;
        assert!((snap_t(&v, 0.1234) - 0.1234).abs() < 1.0e-12);

        // And whatever snapped is a line the grid draws.
        v.snap_on = true;
        for shape in [(false, false), (true, false), (false, true)] {
            v.swing = shape.0;
            v.triplet = shape.1;
            let snapped = snap_t(&v, 1.1);
            let mut found = false;
            grid_lines(&v, 0.0, 4.0, |t, _| {
                if (t - snapped).abs() < 1.0e-6 {
                    found = true;
                }
            });
            assert!(found, "snap answered {snapped} but no line stands there");
        }
    }

    /// A note beats the cell under it, its right edge beats its body, the
    /// keys column answers pitches, and the strip's tools are pressable.
    #[test]
    fn hits_rank_notes_edges_keys_and_tools() {
        let notes = [ArrNote {
            at_s: 2.0,
            dur_s: 1.0,
            key: 60,
            vel: 96,
        }];
        let v = view(&notes);
        let l = Layout::new(rect());
        let r = note_rect(&l, &v, &notes[0]);
        assert!(r.is_positive(), "the note is off screen");
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.left() + 2.0, r.center().y)),
            Some(Hit::NoteLeft(0)),
            "the head's grab zone answers the left edge"
        );
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.center().x, r.center().y)),
            Some(Hit::NoteBody(0))
        );
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.right() - 1.0, r.center().y)),
            Some(Hit::NoteRight(0))
        );
        match hit_test(rect(), &v, Pos2::new(r.center().x, r.center().y - key_h(&l) * 2.0)) {
            Some(Hit::Cell(t, key)) => {
                assert_eq!(key, 62, "two rows up from C4 is D4");
                assert!((t - 10.5).abs() < 0.2, "the cell's second read as {t}");
            }
            other => panic!("empty roll answered {other:?}"),
        }
        match hit_test(rect(), &v, Pos2::new(l.keys.center().x, r.center().y)) {
            Some(Hit::Key(60)) => {}
            other => panic!("the C4 key answered {other:?}"),
        }
        for (r, want) in [
            (l.close, Hit::Close),
            (l.tool_snap, Hit::ToolSnap),
            (l.tool_grid, Hit::ToolGrid),
            (l.tool_swing, Hit::ToolSwing),
            (l.tool_triplet, Hit::ToolTriplet),
        ] {
            assert!(r.is_positive());
            assert_eq!(hit_test(rect(), &v, r.center()), Some(want));
        }
        // Outside the panel: nobody's.
        assert_eq!(hit_test(rect(), &v, Pos2::new(2.0, 2.0)), None);
    }

    /// Degenerate rects neither panic nor answer presses.
    #[test]
    fn it_survives_small_rects() {
        let n: [ArrNote; 0] = [];
        let v = view(&n);
        for size in [Vec2::ZERO, Vec2::new(20.0, 8.0), Vec2::new(2000.0, 900.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 5.0), size);
            let _ = Layout::new(r);
            let _ = hit_test(r, &v, r.center());
        }
    }

    /// **The swing fraction is the view's, not a constant** — the SWG menu
    /// writes it, and every line, magnet and duration follows. And the next
    /// line after an on-beat is the off-beat, however hard the swing.
    #[test]
    fn the_swing_fraction_moves_the_lines_and_next_walks_them() {
        let n: [ArrNote; 0] = [];
        let mut v = view(&n);
        v.swing = true;
        v.swing_pct = 0.667;
        // Pairs of 0.5 s: the off-beat line now sits at 0.3335.
        assert!((snap_t(&v, 0.30) - 0.3335).abs() < 1.0e-9);
        // Creation on the on-beat runs LONG to the swung line; on the
        // swung line it runs SHORT to the next pair.
        assert!((snap_next_t(&v, 0.0) - 0.3335).abs() < 1.0e-9);
        assert!((snap_next_t(&v, 0.3335) - 0.5).abs() < 1.0e-9);
        // Straight, next is simply the following step.
        v.swing = false;
        assert!((snap_next_t(&v, 0.25) - 0.5).abs() < 1.0e-9);
        // The presets are exactly the owner's six, shuffle standard.
        assert_eq!(SWING_PRESETS[0], ("Straight", "1:1", 50.0));
        assert_eq!(SWING_PRESETS[4], ("Triplet (Shuffle)", "2:1", 66.7));
        assert_eq!(SWING_PRESETS[5], ("Dotted (Hard)", "3:1", 75.0));
    }

    /// The grid word says the shape, not just the size.
    #[test]
    fn the_gridbox_word_carries_the_shape() {
        let n: [ArrNote; 0] = [];
        let mut v = view(&n);
        assert_eq!(grid_word(&v), "1/8");
        v.triplet = true;
        assert_eq!(grid_word(&v), "1/8T");
        v.triplet = false;
        v.swing = true;
        assert_eq!(grid_word(&v), "1/8 sw");
    }
}
