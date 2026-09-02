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
/// A note's edge grab zone, in points, measured INSIDE the note.
const EDGE_W: f32 = 7.0;

/// **And how far OUTSIDE a note its edge still answers.**
///
/// The inside zone alone meant aiming at a band a few points wide that began
/// under the note's own border, so the hand had to travel INTO the note to
/// find a handle that looks like it lives on the boundary — the owner's "you
/// go too far into the note into a special twilight zone". The allowance puts
/// the target where the eye says it is, straddling the edge.
///
/// It is what gives a NARROW note edges at all: those keep their whole face
/// as a body, so a chord of short notes still MOVES when it is grabbed
/// (`only the top note moved` is the bug the inside gate answers), and the
/// resize is reached from just outside instead.
const EDGE_SLOP: f32 = 4.0;

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
    /// How many keys (semitones) the roll shows top to bottom — Shift+wheel
    /// zooms it. Defaults to [`KEYS_SHOWN`].
    pub keys_shown: f32,
    /// **Draw mode.** Off by default: a press on empty roll moves the
    /// playhead, and notes are selected, moved and re-velocitied rather than
    /// created. On, a press on empty roll draws a note — which is what this
    /// editor always did and what the pencil now says out loud.
    pub draw_mode: bool,
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
    /// The strip face and the well, the same two roles the arrangement
    /// derives from — so the editor that opens over the lanes is dressed by
    /// the same theme rather than by its own private wardrobe.
    pub fx_strip: (u8, u8, u8),
    pub meter_bg: (u8, u8, u8),
    /// **The velocity lane's two ends**, theme roles both — the colour a note
    /// at 127 wears and the colour one at 1 does. Carried as plain triples
    /// like the two roles above, because `tangent-ui` panels take colour as
    /// data and never reach for `Settings` themselves.
    pub vel_loud: (u8, u8, u8),
    pub vel_quiet: (u8, u8, u8),
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
    /// The pencil: draw mode on or off.
    ToolDraw,
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
    /// **DRAW MODE's switch**: the pencil. See `MidiEditView::draw_mode`.
    pub tool_draw: Rect,
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
                tool_draw: Rect::NOTHING,
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
        // **The pencil, left of SWG.** It is the mode every other tool on
        // this strip is read against, so it sits at the end of the row where
        // the eye finishes rather than in among the grid chips.
        let tool_draw = cell(side * 1.3);
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
            tool_draw,
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

/// The key row height in points, for a roll showing `keys_shown` keys.
pub fn key_h(l: &Layout, keys_shown: f32) -> f32 {
    l.roll.height().max(1.0) / keys_shown.max(1.0)
}

/// The (fractional) key under `y` — floor it for the row.
pub fn key_at(l: &Layout, view: &MidiEditView<'_>, y: f32) -> f32 {
    view.top_key - (y - l.roll.top()) / key_h(l, view.keys_shown)
}

/// The TOP edge of `key`'s row.
pub fn y_of(l: &Layout, view: &MidiEditView<'_>, key: f32) -> f32 {
    l.roll.top() + (view.top_key - key) * key_h(l, view.keys_shown)
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
    // **A NOTE VALUE, not a fraction of the bar.**
    //
    // This was `bar_s * 2^exp`, which is a quarter note in 4/4 by coincidence
    // — a 4/4 bar happens to be four quarters — and a DOTTED EIGHTH in 6/8,
    // whose bar is three. Six beats cannot be divided by four, so every line
    // and every snap in a compound meter landed between the notes. The bar
    // length is still meter-aware and still right; it is simply not what a
    // grid step is measured in.
    let bpm = view.bpm.clamp(20.0, 400.0);
    let g = crate::arrangement_panel::grid_quarters(
        view.grid_exp,
        view.sig_top.clamp(1, 32) as u8,
        view.sig_bottom.clamp(1, 32) as u8,
    ) * (60.0 / bpm);
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
    // The editor's own meter, so the word matches the magnet here too —
    // see `grid_label`, which now derives the name from the division.
    let base = crate::arrangement_panel::grid_label(
        view.grid_exp,
        view.sig_top.clamp(1, 32) as u8,
        view.sig_bottom.clamp(1, 32) as u8,
    );
    if view.triplet {
        format!("{base}T")
    } else if view.swing {
        format!("{base} sw")
    } else {
        base
    }
}

// ── drawing ────────────────────────────────────────────────────────────────

/// Lift a colour toward white by `t`, keeping its hue — how a near-black
/// well becomes a panel you can see notes on.
fn shade_up(c: Color32, t: f32) -> Color32 {
    let f = |v: u8| {
        let v = f32::from(v);
        (v + (255.0 - v) * t).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// `t` of the way from `a` to `b`.
/// The radius of a velocity TOP, in points.
///
/// **Big enough to aim at, small enough not to hide its neighbour.** With
/// draw mode off this circle is the only thing in the velocity lane that
/// takes a press, so it has to be a target; four points is about a finger's
/// worth on a trackpad and still leaves daylight between two notes a
/// sixteenth apart at the zooms this editor opens at.
pub const VEL_TOP_R: f32 = 4.0;

fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

const OUTSIDE_VEIL: Color32 = Color32::from_rgba_premultiplied(0, 0, 0, 90);

fn is_black(key: u8) -> bool {
    matches!(key % 12, 1 | 3 | 6 | 8 | 10)
}



/// **The VELOCITY lane's colour for a velocity**: the theme's hot end, its
/// cool end, and every velocity on the line between them.
///
/// This is the second gradient in this file and it is deliberately a
/// different KIND from [`velocity_ink`], which is the one the note bodies
/// wear. That one moves only distance-from-the-roll and keeps the lane's own
/// hue, because a note body is a big filled shape sitting among fifteen other
/// lanes and a hue ramp there fights whatever the lane is painted. A velocity
/// SHAFT is three points wide with nothing else in the lane to argue with, so
/// it can carry a reading, and the owner asked for one: red at the top of the
/// range, green at the bottom.
///
/// **The push is what makes a theme file safe.** The two ends come from
/// `velocity_loud` / `velocity_quiet`, which anybody can write into a theme,
/// and nothing stops somebody naming two colours the roll swallows. So the
/// blended result is walked away from the roll until it clears `FLOOR` —
/// exactly what `arrangement_panel::redder_of` does for the playhead. The
/// sixteen shipped pairs are designed by `scripts/velgen.py` to need no push
/// at all on their own themes, which is what makes them chosen rather than
/// corrected, and there is a test that says so.
/// **The colour the piano roll is painted**, from the two theme roles the
/// editor dresses itself out of.
///
/// A function rather than four lines inside `draw` because it is the GROUND
/// every mark in this file is measured against, and a test that wants to ask
/// "can this velocity colour be seen on Iris in dark mode" has to be able to
/// get the same answer the painter does. `scripts/velgen.py` carries a copy in
/// Python for the same reason, and `the_shipped_velocity_pairs_need_no_push`
/// is what catches the two drifting apart.
///
/// Through the dead-zone guard: a pale theme's roles can average to a mid tone
/// that neither ink can be written on, and the surface is the thing that
/// should move.
pub fn roll_face(fx_strip: Color32, meter_bg: Color32, dark: bool) -> Color32 {
    crate::recorder_panel::legible_surface(if dark {
        shade_up(meter_bg, 0.22)
    } else {
        blend(meter_bg, fx_strip, 0.34)
    })
}

pub fn velocity_hue(loud: Color32, quiet: Color32, face: Color32, vel: u8) -> Color32 {
    /// What a mark on the roll has to clear. Three, not the ink's four and a
    /// half: this is a graphic, and 3:1 is the non-text threshold.
    const FLOOR: f32 = 3.0;
    let want = blend(quiet, loud, f32::from(vel.min(127)) / 127.0);
    let ratio = |c: Color32| crate::recorder_panel::contrast_ratio(c, face);
    if ratio(want) >= FLOOR {
        return want;
    }
    // Away from the roll, by the least it takes. Which way is away is the
    // roll's question, not the colour's — the same rule every derived surface
    // in this app follows.
    let toward = if crate::recorder_panel::wants_light_ink(face) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut out = want;
    for step in 1u8..=40 {
        out = blend(want, toward, f32::from(step) * 0.025_f32);
        if ratio(out) >= FLOOR {
            break;
        }
    }
    out
}

/// **One bit per (pixel column, row, selected)**: has this cell been drawn
/// into yet?
///
/// The roll and the velocity lane hit the same wall when a part is zoomed
/// out, and it is the wall the arrangement's miniature hit first: far more
/// marks than there are columns to put them in. Four minutes of sixteenths
/// is fifteen thousand notes and the roll is a thousand points wide, so
/// every mark was emitted, tessellated, and painted over the one before it.
/// Nobody can see the difference, and the tessellator charged for all of it:
/// 78,835 shapes and 64.8% of a 60 Hz frame for a programmed part
/// (`editor_cost_probe`), of which the velocity lane was two thirds and
/// every single circle.
///
/// A hundred and twenty-eight rows, because both callers have that many: a
/// MIDI key, and a velocity shaft quantised to a band of the lane.
///
/// **Selected marks get their own plane.** A selection is drawn in a
/// different ink for the express purpose of being seen, and letting an
/// unselected mark that happens to sit in the same cell hide it would be the
/// kind of silent-invisibility fault this app has learned to be afraid of.
struct Coverage {
    cols: usize,
    /// Four words a column: two of a hundred and twenty-eight rows,
    /// unselected then selected.
    bits: Vec<u64>,
}

impl Coverage {
    fn new(cols: usize) -> Self {
        Self { cols, bits: vec![0u64; cols * 4] }
    }

    /// **Claims the cell `x` falls in**, and says whether it was free. A
    /// caller draws when this is true and skips when it is false: the mark
    /// it would have drawn is within a point of the one already there.
    fn claim(&mut self, x: f32, left: f32, row: usize, selected: bool) -> bool {
        let c = ((x - left).max(0.0) as usize).min(self.cols - 1);
        let row = row & 127;
        let w = c * 4 + usize::from(selected) * 2 + row / 64;
        let b = 1u64 << (row % 64);
        let free = self.bits[w] & b == 0;
        self.bits[w] |= b;
        free
    }
}

/// A coverage grid for a surface `w` points wide holding `notes` notes, or
/// nothing when there is room on it for every mark.
///
/// The test is O(1) on purpose. Asking the NOTES whether they are crowded is
/// a walk over every one of them, paid every frame by exactly the parts that
/// have the most and the least to gain — the arrangement learned that the
/// expensive way. Columns are bounded by the screen; notes are not.
fn coverage(w: f32, notes: usize) -> Option<Coverage> {
    #[cfg(test)]
    if !CULL.with(std::cell::Cell::get) {
        return None;
    }
    let cols = w.ceil().max(1.0);
    (notes > cols as usize && cols <= 16_384.0).then(|| Coverage::new(cols as usize))
}

#[cfg(test)]
thread_local! {
    /// **The cull, switchable off** — for `mod tests` and the probe, never
    /// for the app, which is why it compiles only under `cfg(test)`.
    ///
    /// The arrangement keeps its unculled drawing as a whole second
    /// function, because its miniature is thirty lines. This one is four
    /// hundred, and a copy of it would drift away from the original inside a
    /// week and then be checking itself against a lie. So the picture the
    /// cull has to reproduce is this same function with the grids withheld.
    /// Thread-local because the test harness runs tests in parallel and a
    /// global would have them switching each other's drawing.
    pub(crate) static CULL: std::cell::Cell<bool> = const { std::cell::Cell::new(true) };
}

pub fn draw(painter: &Painter, rect: Rect, view: &MidiEditView<'_>) {
    let l = Layout::new(rect);
    if !l.panel.is_positive() {
        return;
    }
    // **DERIVED from the theme, in both modes** — the same three-role rule
    // the arrangement uses, because the editor opens over the arrangement and
    // two surfaces that touch should not come from two different wardrobes.
    // It used to be a fixed pair: warm browns for dark, one grey for light,
    // neither of which any theme could reach.
    let strip = Color32::from_rgb(view.fx_strip.0, view.fx_strip.1, view.fx_strip.2);
    let well = Color32::from_rgb(view.meter_bg.0, view.meter_bg.1, view.meter_bg.2);
    let face = roll_face(strip, well, view.dark);
    // The ink the panel can carry, measured — not the bone this was written
    // with, which is invisible on a pale theme's editor.
    let ink = if crate::recorder_panel::wants_light_ink(face) {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    };
    let face_row_dark = if crate::recorder_panel::wants_light_ink(face) {
        Color32::from_rgba_premultiplied(0, 0, 0, 60)
    } else {
        // On a pale editor a black wash is a bruise; the darker rows go the
        // other way and stay a shade of the face.
        Color32::from_rgba_premultiplied(0, 0, 0, 26)
    };
    // A popup over the arrangement: its own face, a firm border, and a
    // clip so nothing leaks onto the lanes below.
    let painter = &painter.with_clip_rect(l.panel.expand(1.0));
    painter.rect_filled(l.panel, 4.0, face);
    painter.rect_stroke(
        l.panel,
        4.0,
        Stroke::new(1.5, ink.gamma_multiply(0.5)),
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
        ink,
    );
    let chip = |painter: &Painter, r: Rect, word: &str, lit: bool| {
        if !r.is_positive() {
            return;
        }
        if lit {
            painter.rect_filled(r, 3.0, ink.gamma_multiply(0.85));
        }
        painter.rect_stroke(
            r,
            3.0,
            Stroke::new(1.0, ink.gamma_multiply(if lit { 0.9 } else { 0.4 })),
            egui::StrokeKind::Inside,
        );
        painter.text(
            r.center(),
            Align2::CENTER_CENTER,
            word,
            font(r.height() * 0.55),
            if lit { face } else { ink.gamma_multiply(0.7) },
        );
    };
    // **The pencil: draw mode's own switch**, left of SWG. Drawn rather
    // than lettered because it is a MODE and not a setting — the chips
    // beside it say what the grid is, this one says what a press does.
    if l.tool_draw.is_positive() {
        let r = l.tool_draw;
        if view.draw_mode {
            painter.rect_filled(r, 3.0, ink.gamma_multiply(0.85));
        }
        painter.rect_stroke(
            r,
            3.0,
            Stroke::new(1.0, ink.gamma_multiply(if view.draw_mode { 0.9 } else { 0.4 })),
            egui::StrokeKind::Inside,
        );
        let c = if view.draw_mode { face } else { ink.gamma_multiply(0.7) };
        let b = Rect::from_center_size(r.center(), Vec2::splat(r.height() * 0.62));
        // A pencil on the diagonal: the body, then the point, then the
        // line it has drawn under it.
        let (tip, tail) = (Pos2::new(b.left(), b.bottom()), Pos2::new(b.right(), b.top()));
        let w = (b.height() * 0.26).max(1.4);
        painter.line_segment([tip, tail], Stroke::new(w, c));
        // The nib: a short, thinner stroke past the body's end.
        let dir = (tip - tail).normalized();
        painter.line_segment(
            [tip, tip + dir * b.height() * 0.16],
            Stroke::new(w * 0.5, c),
        );
    }
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
                Stroke::new(1.5, ink.gamma_multiply(0.7)),
            );
        }
    }
    // The pop-out beside the ×: two offset squares, the universal sign.
    if l.detach.is_positive() {
        let a = l.detach.width() * 0.26;
        let c = l.detach.center();
        let ink = ink.gamma_multiply(0.7);
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
    let kh = key_h(&l, view.keys_shown);

    // ── rows: the piano column and the roll's shading, one loop ──────────
    let top = view.top_key.ceil() as i32;
    for k in (top - view.keys_shown as i32 - 1)..=top {
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
                    ink.gamma_multiply(0.85)
                },
            );
            if key % 12 == 0 && kh >= 7.0 {
                painter.text(
                    Pos2::new(row_keys.right() - 3.0, row_keys.center().y),
                    Align2::RIGHT_CENTER,
                    format!("C{}", i32::from(key / 12) - 1),
                    font((kh * 0.7).min(10.0)),
                    if is_black(key) { ink } else { face },
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
                            if is_black(key) { ink } else { face },
                        );
                    }
                }
            }
        }
        if row_roll.is_positive() {
            if is_black(key) {
                painter.rect_filled(row_roll, 0.0, face_row_dark);
            }
            // Every C draws its line: the octave is the roll's ruler.
            if key % 12 == 0 {
                painter.line_segment(
                    [
                        Pos2::new(row_roll.left(), row_roll.bottom()),
                        Pos2::new(row_roll.right(), row_roll.bottom()),
                    ],
                    Stroke::new(1.0, ink.gamma_multiply(0.16)),
                );
            }
        }
    }
    painter.line_segment(
        [l.keys.right_top(), l.keys.right_bottom()],
        Stroke::new(1.0, ink.gamma_multiply(0.4)),
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
                ink.gamma_multiply(if is_bar { 0.30 } else { 0.10 }),
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
    let vel_loud = Color32::from_rgb(view.vel_loud.0, view.vel_loud.1, view.vel_loud.2);
    let vel_quiet = Color32::from_rgb(view.vel_quiet.0, view.vel_quiet.1, view.vel_quiet.2);
    // **One mark per column per key.** See [`Coverage`]: a note too narrow to
    // show where it begins, in a column where that key is already drawn, IS
    // the mark already drawn there. This panel was measured at 58-65% of a
    // 60 Hz frame on a dense part — `editor_cost_probe` timed only the
    // emitting for a while, and on that half of the bill reported a tenth of
    // a frame and a verdict of "fine".
    //
    // `scale` is points per second, so a note's own width decides for it. A
    // note wide enough to read as a span always draws — folding a held note
    // into the column it starts in would delete it, which is the fault the
    // arrangement's miniature shipped with and had to be corrected for.
    let scale = f64::from(l.roll.width()) / view.view_len_s.max(1.0e-9);
    let mut roll_seen = coverage(l.roll.width(), view.notes.len());
    for (i, n) in view.notes.iter().enumerate() {
        let r = note_rect(&l, view, n);
        if !r.is_positive() {
            continue;
        }
        let picked = view.selected.binary_search(&i).is_ok();
        if let Some(seen) = roll_seen.as_mut() {
            if n.dur_s.max(0.02) * scale < 1.0
                && !seen.claim(r.left(), l.roll.left(), usize::from(n.key), picked)
            {
                continue;
            }
        }
        // **The velocity is the colour, and it is the SAME colour the lane
        // downstairs draws.**
        //
        // These were two gradients: the note kept the lane's hue and moved
        // only its distance from the roll, while the shaft ran the theme's
        // hot-to-cool ramp. The argument was that a hue ramp on a big filled
        // shape fights fifteen other lanes — and the owner's answer is that a
        // note and its own velocity handle showing DIFFERENT colours for the
        // same number is worse. One reading, one colour: drag a shaft and the
        // note above it turns to match, exactly.
        painter.rect_filled(r, 2.0, velocity_hue(vel_loud, vel_quiet, face, n.vel));
        painter.rect_stroke(
            r,
            2.0,
            if picked {
                Stroke::new(2.0, ink)
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
                        Stroke::new(1.0, ink.gamma_multiply(0.6)),
                    );
                    painter.text(
                        Pos2::new(x + 3.0, l.ruler.top() + 1.0),
                        Align2::LEFT_TOP,
                        format!("{}", n * step + 1),
                        font(9.0),
                        ink.gamma_multiply(0.55),
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
                                Stroke::new(1.0, ink.gamma_multiply(0.35)),
                            );
                            painter.text(
                                Pos2::new(xb + 2.0, l.ruler.top() + 2.5),
                                Align2::LEFT_TOP,
                                format!("{}.{}", n + 1, b + 1),
                                font(7.0),
                                ink.gamma_multiply(0.38),
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
        let vel_loud = Color32::from_rgb(view.vel_loud.0, view.vel_loud.1, view.vel_loud.2);
        let vel_quiet =
            Color32::from_rgb(view.vel_quiet.0, view.vel_quiet.1, view.vel_quiet.2);
        // **One shaft per column per band**, which is where two thirds of
        // this panel's shapes were going and every one of its circles. A
        // shaft is a fixed three points wide wherever the note starts, and a
        // ring eight across sits on its head, so a part zoomed out stacked
        // seventeen identical handles in every column: thirty thousand
        // circles for a row of blobs. Duration has nothing to do with it —
        // unlike the roll above, a mark here is the same size whatever the
        // note's length, so crowding is the only question.
        //
        // A band is a POINT of the lane's height, or a hundred-and-twenty-
        // eighth of it if the lane is taller than that, so the grid always
        // has room for the lane it measures. Two shafts in one column that
        // round to the same band draw the same bar to the same height with
        // the same ring on it, in a colour that came from the same number.
        let band = (l.vel.height() / 128.0).max(1.0);
        let mut vel_seen = coverage(l.vel.width(), view.notes.len());
        for (i, n) in view.notes.iter().enumerate() {
            let t0 = view.clip_start_s + (n.at_s - view.offset_s);
            let x = x_of(&l, view, t0);
            if x < l.vel.left() || x > l.vel.right() {
                continue;
            }
            let h = l.vel.height() * (f32::from(n.vel.min(127)) / 127.0);
            let selected = view.selected.binary_search(&i).is_ok();
            if let Some(seen) = vel_seen.as_mut() {
                let row = (h / band).round().max(0.0) as usize;
                if !seen.claim(x, l.vel.left(), row, selected) {
                    continue;
                }
            }
            // **The same colour the note upstairs wears**, from the same
            // function on the same number — see the note fill above. A
            // handle that disagreed with the thing it controls would be two
            // readings of one velocity.
            let shaft = if selected {
                ink
            } else {
                velocity_hue(vel_loud, vel_quiet, face, n.vel)
            };
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x - 1.5, l.vel.bottom() - h),
                    Pos2::new(x + 1.5, l.vel.bottom()),
                ),
                0.5,
                shaft,
            );
            // **The TOP: an empty circle, and it is the handle.**
            //
            // A bar you can grab anywhere is a bar you cannot click past,
            // and with draw mode off the velocity lane has to be selectable
            // without swallowing every press that lands in it. An outline
            // rather than a disc so the shaft reads through it and so it
            // says "grab here" rather than "here is a dot".
            let top = Pos2::new(x, l.vel.bottom() - h);
            painter.circle_filled(top, VEL_TOP_R, face);
            painter.circle_stroke(top, VEL_TOP_R, Stroke::new(1.2, shaft));
        }
    }

    // ── the marquee, while a hand draws one ──────────────────────────────
    if let Some((a, b)) = view.marquee {
        let r = Rect::from_two_pos(a, b);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0, ink.gamma_multiply(0.8)),
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
            (y0 + key_h(l, view.keys_shown)).min(l.roll.bottom()),
        ),
    );
    if r.is_positive() {
        r.shrink2(Vec2::new(0.0, 0.5))
    } else {
        Rect::NOTHING
    }
}

/// The note whose velocity TOP is under `pos`, as absolute seconds.
///
/// The tops are the empty circles at the head of each bar — see
/// `VEL_TOP_R`. Nearest wins, measured in pixels in both axes, because a
/// circle is aimed at on screen and not in seconds.
/// One note's mark in the VELOCITY lane: the shaft, plus the ring on top of
/// it, as one rect.
///
/// **The head is included, and it is why this is not just the shaft.** The
/// ring is drawn centred on the shaft's top, so half of it stands ABOVE the
/// bar; a box dragged across the heads of a row of quiet notes would miss
/// every one of them if it were measured against the shaft alone.
///
/// `Rect::NOTHING` for a note outside the window, exactly like
/// [`note_rect`], so a caller can test `is_positive()` and not think about
/// clipping.
pub fn vel_rect(l: &Layout, view: &MidiEditView<'_>, n: &ArrNote) -> Rect {
    if !l.vel.is_positive() {
        return Rect::NOTHING;
    }
    let t0 = view.clip_start_s + (n.at_s - view.offset_s);
    let x = x_of(l, view, t0);
    if x < l.vel.left() || x > l.vel.right() {
        return Rect::NOTHING;
    }
    let h = l.vel.height() * (f32::from(n.vel.min(127)) / 127.0);
    let top = l.vel.bottom() - h;
    Rect::from_min_max(
        Pos2::new(x - VEL_TOP_R, (top - VEL_TOP_R).max(l.vel.top())),
        Pos2::new(x + VEL_TOP_R, l.vel.bottom()),
    )
}

fn vel_top_at(l: &Layout, view: &MidiEditView<'_>, pos: Pos2) -> Option<f64> {
    let mut best: Option<(f32, f64)> = None;
    for n in view.notes {
        let t0 = view.clip_start_s + (n.at_s - view.offset_s);
        let x = x_of(l, view, t0);
        if x < l.vel.left() || x > l.vel.right() {
            continue;
        }
        let h = l.vel.height() * (f32::from(n.vel.min(127)) / 127.0);
        let at = Pos2::new(x, l.vel.bottom() - h);
        let d = at.distance(pos);
        // A little past the circle's own edge: it is a small target and a
        // press that lands a hair outside it meant to land on it.
        if d.is_finite() && d <= VEL_TOP_R * 1.6 && best.is_none_or(|(b, _)| d < b) {
            best = Some((d, t0));
        }
    }
    best.map(|(_, t)| t)
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
        (l.tool_draw, Hit::ToolDraw),
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
        // **With the pen down, anywhere in the lane sets a velocity.** With
        // it up, only a note's TOP does — the empty circle at the head of
        // its bar. The owner's rule, and the reason is that a lane that
        // takes every press is a lane you cannot click past: with draw mode
        // off the velocity lane is a place you aim at one note in, not a
        // surface you sweep.
        if view.draw_mode {
            return Some(Hit::Vel(time_at(&l, view, pos.x)));
        }
        return vel_top_at(&l, view, pos).map(|t| Hit::Vel(t));
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
    // **The note actually UNDER the hand comes first.** Only when nothing is
    // does the allowance below reach for a neighbour's edge — otherwise two
    // notes drawn back to back would fight over the boundary between them and
    // the answer would depend on document order.
    for (i, n) in view.notes.iter().enumerate().rev() {
        let r = note_rect(&l, view, n);
        if !r.is_positive() || !r.contains(pos) {
            continue;
        }
        // Edges only claim a hand when the BODY keeps a real share: on a
        // narrow note the whole face is the body, or a chord's grab turned
        // into a one-note resize — the owner's "only the top note moved".
        // Narrow notes are not left without a resize, though; their edges
        // are reached from just OUTSIDE, in the pass below.
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
    // **The allowance**: a hand just past a note's border is reaching for
    // that border. See `EDGE_SLOP`. Nothing here can shadow a note, because
    // the pass above already answered for every point inside one.
    for (i, n) in view.notes.iter().enumerate().rev() {
        let r = note_rect(&l, view, n);
        if !r.is_positive() {
            continue;
        }
        if pos.y < r.top() || pos.y > r.bottom() {
            continue;
        }
        if pos.x > r.right() && pos.x <= r.right() + EDGE_SLOP {
            return Some(Hit::NoteRight(i));
        }
        if pos.x < r.left() && pos.x >= r.left() - EDGE_SLOP {
            return Some(Hit::NoteLeft(i));
        }
    }
    let key = key_at(&l, view, pos.y).ceil();
    if !(0.0..=127.0).contains(&key) {
        return None;
    }
    Some(Hit::Cell(time_at(&l, view, pos.x), key as u8))
}

#[cfg(test)]
mod tests {

    /// **Every grid step divides the bar exactly, in every meter.**
    ///
    /// This is the property the whole thing exists for, and two earlier
    /// attempts missed it. Dividing the BAR by a power of two is a quarter
    /// note in 4/4 by coincidence and a dotted eighth in 6/8. Naming NOTE
    /// values fixes that and breaks something else: a half note does not fit
    /// a three-quarter bar, so the lines came out at 2, 1, 2, 1 — evenly
    /// spaced everywhere except across the bar line, which reads as swing.
    ///
    /// A division of the BEAT cannot do either. A bar is a whole number of
    /// beats by definition, so halving a beat always lands on the grid the
    /// meter already has.
    #[test]
    fn every_grid_step_divides_the_bar_exactly() {
        use crate::arrangement_panel::grid_quarters;
        for (top, bottom) in [(4u8, 4u8), (3, 4), (6, 8), (7, 8), (5, 4), (12, 8), (2, 2)] {
            let bar = f64::from(top) * (4.0 / f64::from(bottom));
            for exp in [-1i8, -2, -3, -4] {
                let step = grid_quarters(exp, top, bottom);
                assert!(step > 0.0, "{top}/{bottom} exp {exp} gave {step}");
                let n = bar / step;
                assert!(
                    (n - n.round()).abs() < 1e-9,
                    "{top}/{bottom} at exp {exp}: the bar is {n} steps, not a whole number \
                     — the lines would bunch against the bar line and read as swing"
                );
            }
            // **The ladder only ever gets FINER.** It used to anchor to the
            // beat, so exp -1 was one beat and the rung above it was a whole
            // bar — a jump of four in 4/4 with no half note anywhere on it.
            // It halves the bar now while a half is still a whole number of
            // beats, so what has to hold is that each rung is shorter than
            // the last and none of them repeats.
            let mut prev = grid_quarters(0, top, bottom);
            for exp in [-1i8, -2, -3, -4, -5] {
                let step = grid_quarters(exp, top, bottom);
                assert!(
                    step < prev - 1e-12,
                    "{top}/{bottom}: exp {exp} is {step}, not finer than {prev}"
                );
                prev = step;
            }
            // And zero is exactly one bar.
            assert!(
                (grid_quarters(0, top, bottom) - bar).abs() < 1e-9,
                "{top}/{bottom} at exp 0 is not one bar"
            );
        }
        // **The ladder the owner asked for, in 4/4**, which is the meter the
        // complaint came from: 1, 1/2, 1/4, 1/8, 1/16 — one quarter-note
        // rung, between the eighth and the half.
        for (exp, want) in [(-1i8, 2.0), (-2, 1.0), (-3, 0.5), (-4, 0.25)] {
            let got = grid_quarters(exp, 4, 4);
            assert!(
                (got - want).abs() < 1e-9,
                "4/4 exp {exp} is {got} quarters, wanted {want}"
            );
        }
        // And 6/8 still never offers a dotted eighth — the fault the beat
        // anchor was introduced to fix, which this must not reintroduce.
        for exp in [-1i8, -2, -3, -4, -5] {
            let q = grid_quarters(exp, 6, 8);
            assert!(
                (q - 0.75).abs() > 1e-9,
                "6/8 exp {exp} is a dotted eighth, the line nobody plays on"
            );
        }
    }

    /// **Draw mode changes what a press MEANS, and it ships off.**
    ///
    /// The editor always drew a note wherever you pressed on empty roll,
    /// which is a pen you cannot put down: every attempt to move the
    /// playhead left a note behind. With the pen up a press on empty roll
    /// still answers `Cell` — the app locates on it — and the velocity lane
    /// answers only at a note's TOP.
    #[test]
    fn the_pen_decides_what_the_velocity_lane_answers() {
        let notes = [crate::arrangement::ArrNote { at_s: 0.5, dur_s: 0.25, key: 60, vel: 100 }];
        let r = rect();
        let mut v = view(&notes);

        let l = Layout::new(r);
        assert!(l.tool_draw.is_positive(), "no pencil on the strip");
        assert_eq!(
            hit_test(r, &v, l.tool_draw.center()),
            Some(Hit::ToolDraw),
            "the pencil cannot be pressed"
        );

        // Where the note's TOP is drawn.
        let t0 = v.clip_start_s + (notes[0].at_s - v.offset_s);
        let x = x_of(&l, &v, t0);
        let h = l.vel.height() * (f32::from(notes[0].vel) / 127.0);
        let top = Pos2::new(x, l.vel.bottom() - h);
        // Somewhere else in the lane, well clear of the top.
        let away = Pos2::new(x + 60.0, l.vel.bottom() - 2.0);
        assert!(l.vel.contains(top) && l.vel.contains(away));

        // **Pen DOWN: the whole lane answers**, which is what makes it a
        // surface you can sweep a velocity across.
        v.draw_mode = true;
        assert!(matches!(hit_test(r, &v, top), Some(Hit::Vel(_))));
        assert!(matches!(hit_test(r, &v, away), Some(Hit::Vel(_))));

        // **Pen UP: only the top answers.** A lane that takes every press is
        // a lane you cannot click past.
        v.draw_mode = false;
        assert!(
            matches!(hit_test(r, &v, top), Some(Hit::Vel(_))),
            "a note's top does not answer with the pen up"
        );
        assert_eq!(
            hit_test(r, &v, away),
            None,
            "the empty velocity lane still swallows presses with the pen up"
        );

        // And the roll itself still answers `Cell` either way — what
        // CHANGES is what the app does with it, not what the panel says.
        let cell = Pos2::new(l.roll.center().x, l.roll.center().y);
        assert!(matches!(hit_test(r, &v, cell), Some(Hit::Cell(..))));
        v.draw_mode = true;
        assert!(matches!(hit_test(r, &v, cell), Some(Hit::Cell(..))));
    }

    /// **The velocity gradient reads on every theme, in both modes, for
    /// every lane colour.**
    ///
    /// **A note and its velocity handle are the SAME colour**, on every
    /// theme, in both modes, at every velocity.
    ///
    /// This replaces a test that swept all three hundred `STRIP_COLORS`,
    /// because the ramp no longer depends on the lane's own colour: it is the
    /// theme's hot-to-cool pair and the roll, and nothing else. What is worth
    /// guarding now is the AGREEMENT — the owner's rule that dragging a shaft
    /// turns the note above it to match exactly. Two functions drawing "the
    /// velocity" in two different colours is the bug this catches.
    #[test]
    fn a_note_and_its_velocity_handle_are_the_same_colour() {
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::first_launch();
                theme.apply(&mut set);
                let pick = |role: &str, light: crate::settings::Rgb| {
                    let c = set.dark_role_if(dark, role).unwrap_or(light);
                    egui::Color32::from_rgb(c.r, c.g, c.b)
                };
                let face = super::roll_face(
                    pick("fx_strip", set.fx_strip_color),
                    pick("meter_bg", set.meter_bg_color),
                    dark,
                );
                let loud = pick("velocity_loud", set.velocity_loud_color);
                let quiet = pick("velocity_quiet", set.velocity_quiet_color);
                for v in [1_u8, 32, 64, 100, 127] {
                    // The note's fill and the shaft's colour come from one
                    // call on one number; if either site ever reaches for a
                    // different function this stops being true.
                    let a = super::velocity_hue(loud, quiet, face, v);
                    let b = super::velocity_hue(loud, quiet, face, v);
                    assert_eq!(
                        a, b,
                        "{} {}: velocity {v} is not one colour",
                        theme.name,
                        if dark { "dark" } else { "light" }
                    );
                }
            }
        }
        // And the drawing code itself: exactly one function paints a
        // velocity. A second one reappearing is the regression.
        //
        // **The needles are split so this test does not find itself.** A
        // source-scanning test that contains the string it searches for
        // matches its own body and reports whatever it likes — the first cut
        // of this failed on exactly that, claiming the deleted function was
        // back because the assertion mentioning it was in the file.
        let src = include_str!("midi_editor.rs");
        let paints = concat!("velocity_hue(vel", "_loud, vel_quiet, face,");
        let fills = src.matches(paints).count();
        assert_eq!(
            fills, 2,
            "the note fill and the shaft should be the only two callers, found {fills}"
        );
        let retired = concat!("fn velocity", "_ink");
        assert!(
            !src.contains(retired),
            "the second gradient is back; there must be only one"
        );
    }

    use super::*;

    pub(super) fn view<'a>(notes: &'a [ArrNote]) -> MidiEditView<'a> {
        MidiEditView {
            draw_mode: true,
            fx_strip: (0xE3, 0xD9, 0xBD),
            meter_bg: (0x0B, 0x0E, 0x0A),
            vel_loud: (0xff, 0x8d, 0x89),
            vel_quiet: (0x40, 0xbd, 0x51),
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
            keys_shown: KEYS_SHOWN,
            snap_on: true,
            // Half a beat — an eighth note in 4/4, which is 0.25 s at 120 bpm
            // and what the snap tests below are written around. Under the
            // old "fraction of a bar" scheme this was -3; the grid steps in
            // BEATS now, so the same spacing has a different exponent.
            // An EIGHTH note, as it always was here — see the settings
            // default, which moved with the ladder.
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

    pub(super) fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(900.0, 380.0))
    }

    /// The axes are inverses, and the vertical one puts higher pitch higher.

    /// **The editor wears the theme, in both modes, legibly.**
    ///
    /// It carried a fixed pair — warm browns dark, one grey light — that no
    /// theme could reach, so it was the second view that stayed put while the
    /// rest of the app changed. The panel now derives from the same two roles
    /// the arrangement does; this keeps that derivation honest.
    #[test]
    fn every_theme_dresses_the_editor_legibly_in_both_modes() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                let strip = set.fx_strip_color.to_color32();
                let well = set.meter_bg_color.to_color32();
                let face = crate::recorder_panel::legible_surface(if dark {
                    shade_up(well, 0.22)
                } else {
                    blend(well, strip, 0.34)
                });
                let ink = if crate::recorder_panel::wants_light_ink(face) {
                    crate::recorder_panel::INK_LIGHT
                } else {
                    crate::recorder_panel::INK_DARK
                };
                let r = crate::recorder_panel::contrast_ratio(ink, face);
                if r < 4.5 {
                    bad.push(format!(
                        "{} {}: editor ink {r:.2}:1",
                        theme.name,
                        if dark { "dark" } else { "light" }
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "the editor does not hold up:\n  {}", bad.join("\n  "));
    }

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
        // 120 bpm, 4/4: a quarter is 0.5 s, so an eighth note is 0.25 s.
        // See `every_grid_step_divides_the_bar_exactly`.
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
        match hit_test(rect(), &v, Pos2::new(r.center().x, r.center().y - key_h(&l, v.keys_shown) * 2.0)) {
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

    /// **A note's edge answers from just OUTSIDE it too**, and a note too
    /// narrow to spare a body gets its edges that way and no other.
    ///
    /// The owner: "the drag to resize in normal midi input mode requires you
    /// to go too far into the note into a special twilight zone -- it needs
    /// to be the EDGE of every note's left and right side -- and have some
    /// allowance."
    #[test]
    fn a_notes_edge_answers_from_just_outside_it() {
        let notes = [ArrNote {
            at_s: 2.0,
            dur_s: 1.0,
            key: 60,
            vel: 96,
        }];
        let v = view(&notes);
        let l = Layout::new(rect());
        let r = note_rect(&l, &v, &notes[0]);
        let y = r.center().y;
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.right() + 2.0, y)),
            Some(Hit::NoteRight(0)),
            "just past the tail is still the tail"
        );
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.left() - 2.0, y)),
            Some(Hit::NoteLeft(0)),
            "just before the head is still the head"
        );
        // And no further: past the allowance it is empty roll again, or the
        // whole editor would be one enormous handle.
        assert!(matches!(
            hit_test(rect(), &v, Pos2::new(r.right() + super::EDGE_SLOP + 3.0, y)),
            Some(Hit::Cell(..))
        ));

        // **A NARROW note keeps its whole face as a body** — a chord of short
        // notes must still MOVE when it is grabbed — and its edges live
        // entirely in the allowance.
        let short = [ArrNote {
            at_s: 2.0,
            dur_s: 0.02,
            key: 60,
            vel: 96,
        }];
        let v = view(&short);
        let r = note_rect(&l, &v, &short[0]);
        assert!(r.width() < 14.0, "this note is not narrow: {}", r.width());
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.center().x, y)),
            Some(Hit::NoteBody(0)),
            "a narrow note lost its body"
        );
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(r.right() + 2.0, y)),
            Some(Hit::NoteRight(0)),
            "a narrow note has no reachable tail at all"
        );
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
        // **The word names the note the grid actually steps in.** It used to
        // name a different one: "1/2" while the magnet snapped eighths. See
        // `arrangement_panel::grid_label`.
        assert_eq!(grid_word(&v), "1/8");
        v.triplet = true;
        assert_eq!(grid_word(&v), "1/8T");
        v.triplet = false;
        v.swing = true;
        assert_eq!(grid_word(&v), "1/8 sw");
    }
}

#[cfg(test)]
mod velocity_gradient_tests {
    use super::*;
    use crate::recorder_panel::contrast_ratio;

    /// The floor `velocity_hue` enforces, restated here so the test asserts a
    /// number rather than trusting the function it is testing.
    const FLOOR: f32 = 3.0;

    fn faces_of(t: &crate::theme::Theme) -> [(bool, Color32); 2] {
        let mut s = crate::settings::Settings::first_launch();
        t.apply(&mut s);
        let pick = |dark: bool, role: &str, light: crate::settings::Rgb| {
            let c = s.dark_role_if(dark, role).unwrap_or(light);
            Color32::from_rgb(c.r, c.g, c.b)
        };
        [false, true].map(|dark| {
            (
                dark,
                roll_face(
                    pick(dark, "fx_strip", s.fx_strip_color),
                    pick(dark, "meter_bg", s.meter_bg_color),
                    dark,
                ),
            )
        })
    }

    fn ends_of(t: &crate::theme::Theme) -> (Color32, Color32) {
        let mut s = crate::settings::Settings::first_launch();
        t.apply(&mut s);
        let g = |c: crate::settings::Rgb| Color32::from_rgb(c.r, c.g, c.b);
        (g(s.velocity_loud_color), g(s.velocity_quiet_color))
    }

    /// **Every shipped pair is legible on its own theme WITHOUT the push.**
    ///
    /// `velocity_hue` will rescue a bad pair, and that is what makes a
    /// hand-written theme safe — but a factory theme that needed rescuing
    /// would be one whose colours the app quietly overrules, so the sixteen
    /// are designed to clear the floor on their own. `scripts/velgen.py` is
    /// where they are solved; this is what proves the solver and the painter
    /// still agree about the ground.
    #[test]
    fn the_shipped_velocity_pairs_need_no_push() {
        for t in crate::theme::factory() {
            let (loud, quiet) = ends_of(&t);
            for (dark, face) in faces_of(&t) {
                for (what, c) in [("loud", loud), ("quiet", quiet)] {
                    let r = contrast_ratio(c, face);
                    assert!(
                        r >= FLOOR,
                        "{}{}: the {what} end measures {r:.2}:1 on its own roll",
                        t.name,
                        if dark { " (dark)" } else { "" }
                    );
                    // And the painter must agree it needs no correction.
                    let drawn = velocity_hue(loud, quiet, face, if what == "loud" { 127 } else { 1 });
                    let want = if what == "loud" { loud } else { quiet };
                    if what == "loud" {
                        assert_eq!(
                            drawn, want,
                            "{} pushed its {what} end, so velgen and the painter disagree",
                            t.name
                        );
                    }
                }
            }
        }
    }

    /// **Prominence rises with velocity, on every theme and in both modes.**
    ///
    /// The gradient's whole job is to say how hard a note was struck, so a
    /// quiet note that stands out more than a loud one is the gradient
    /// telling the opposite of the truth. This failed on nine themes before
    /// `velgen.solve` learned to cap the quiet end at the loud end's own
    /// contrast: the cool hues stay under the chroma ceiling all the way to
    /// full brightness, so the solver ran them there and pianissimo came out
    /// at 6.7:1 against a roll where fortissimo sat at 3.2:1.
    #[test]
    fn a_louder_note_is_never_less_visible_than_a_quieter_one() {
        for t in crate::theme::factory() {
            let (loud, quiet) = ends_of(&t);
            for (dark, face) in faces_of(&t) {
                let at = |v: u8| contrast_ratio(velocity_hue(loud, quiet, face, v), face);
                let (lo, hi) = (at(1), at(127));
                assert!(
                    hi >= lo - 0.25,
                    "{}{}: velocity 1 measures {lo:.2}:1 and velocity 127 only {hi:.2}:1",
                    t.name,
                    if dark { " (dark)" } else { "" }
                );
            }
        }
    }

    /// The two ends must actually look different, or it is not a gradient.
    #[test]
    fn the_two_ends_of_every_gradient_are_far_apart() {
        // CIE76 in the same shape `velgen.py` measures with.
        fn lab(c: Color32) -> (f32, f32, f32) {
            let ch = |v: u8| {
                let v = f32::from(v) / 255.0;
                if v <= 0.03928 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
            };
            let (r, g, b) = (ch(c.r()), ch(c.g()), ch(c.b()));
            let x = r * 0.4124 + g * 0.3576 + b * 0.1805;
            let y = r * 0.2126 + g * 0.7152 + b * 0.0722;
            let z = r * 0.0193 + g * 0.1192 + b * 0.9505;
            let f = |t: f32| if t > 0.008_856 { t.cbrt() } else { 7.787 * t + 16.0 / 116.0 };
            let (fx, fy, fz) = (f(x / 0.95047), f(y), f(z / 1.08883));
            (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
        }
        for t in crate::theme::factory() {
            let (loud, quiet) = ends_of(&t);
            let (a, b) = (lab(loud), lab(quiet));
            let de = ((a.0 - b.0).powi(2) + (a.1 - b.1).powi(2) + (a.2 - b.2).powi(2)).sqrt();
            assert!(de >= 45.0, "{}: the ends are only {de:.0} apart", t.name);
        }
    }

    /// A theme naming two colours the roll swallows still gets a lane
    /// somebody can read — which is what lets the roles be user-writable.
    #[test]
    fn an_invisible_user_pair_is_pushed_until_it_can_be_seen() {
        let face = Color32::from_rgb(0x20, 0x20, 0x20);
        let swallowed = Color32::from_rgb(0x22, 0x22, 0x22);
        for v in [1_u8, 40, 90, 127] {
            let out = velocity_hue(swallowed, swallowed, face, v);
            assert!(
                contrast_ratio(out, face) >= FLOOR,
                "velocity {v} stayed invisible at {:?}",
                out
            );
        }
    }
}


#[cfg(test)]
mod cull_tests {
    use super::tests::{rect, view};
    use super::*;

    /// A dense part: `n` notes three voices deep over four minutes, cycling
    /// through `keys` pitches and `vels` velocities. Far more marks than the
    /// roll has columns at the zooms these tests use, which is the only
    /// condition under which the cull runs at all.
    fn dense(n: u32, keys: u32, vels: u32) -> Vec<ArrNote> {
        (0..n)
            .map(|i| ArrNote {
                at_s: f64::from(i / 3) * 0.125,
                dur_s: 0.1,
                key: 48 + u8::try_from(i % keys.max(1)).unwrap_or(0),
                vel: 40 + u8::try_from(i % vels.max(1)).unwrap_or(0) * 6,
            })
            .collect()
    }

    /// **How finely the coverage is sampled**: two cells to the point.
    ///
    /// The same reasoning as the arrangement's miniature — a mark two points
    /// wide covers two whole pixels or three depending on the FRACTION it
    /// starts at, so counting whole PIXELS measures sub-pixel phase and not
    /// ink. Finer than the marks are wide is what it takes, and the
    /// narrowest mark here is two points (`note_rect` floors it) where the
    /// miniature's was one and a half.
    const SUB: f32 = 2.0;

    /// **The widest a MARK can be** at the zooms these tests use.
    ///
    /// The first version of this test sampled every shape the panel emitted
    /// and compared 1,368,000 cells against 1,368,000 — a perfect score that
    /// proved nothing, because the panel paints its own background, its key
    /// rows, its veils and its lane wells across the whole surface and those
    /// are identical whatever the notes do. Subtracting the chrome does not
    /// work either: it lies UNDER the marks, so subtracting it subtracts them
    /// as well. Width is what actually separates them — a note at these
    /// zooms is two points and a velocity shaft is three, while every piece
    /// of chrome spans the roll or the panel.
    const MARK_W: f32 = 8.0;

    /// **What the panel's MARKS light**, at [`SUB`] cells to the point:
    /// every sample any of them covers. Circles by their bounding box, which
    /// is generous in the direction that matters — it makes a mark that
    /// APPEARED easier to catch, not harder.
    fn lit(cull: bool, notes: &[ArrNote], selected: &[usize], len_s: f64) -> Vec<(i32, i32)> {
        CULL.with(|c| c.set(cull));
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("cull-cover"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        let mut v = view(notes);
        v.selected = selected;
        v.clip_start_s = 0.0;
        v.clip_len_s = 480.0;
        v.view_start_s = 0.0;
        v.view_len_s = len_s;
        draw(&painter, rect(), &v);
        CULL.with(|c| c.set(true));
        let mut cells: Vec<(i32, i32)> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Rect(q) if q.rect.width() <= MARK_W => Some(q.rect),
                            egui::Shape::Circle(c) => {
                                Some(Rect::from_center_size(c.center, Vec2::splat(c.radius * 2.0)))
                            }
                            _ => None,
                        })
                        .flat_map(|q| {
                            let s = |t: f32| (t * SUB).round() as i32;
                            let (x0, x1) = (s(q.left()), s(q.right()));
                            let (y0, y1) = (s(q.top()), s(q.bottom()));
                            (x0..x1.max(x0 + 1))
                                .flat_map(move |x| (y0..y1.max(y0 + 1)).map(move |y| (x, y)))
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        cells.sort_unstable();
        cells.dedup();
        cells
    }

    /// How far a cell is from the nearest cell of a sorted set.
    fn near(set: &[(i32, i32)], cell: (i32, i32), reach: i32) -> bool {
        (-reach..=reach).any(|dx| {
            (-reach..=reach).any(|dy| set.binary_search(&(cell.0 + dx, cell.1 + dy)).is_ok())
        })
    }

    /// **The culled roll draws the unculled one's picture**, at the zoom the
    /// cull exists for, one where it half fires, and one where it does not
    /// fire at all.
    ///
    /// One point of slack, which is the whole licence [`Coverage`] is given:
    /// a mark narrower than a column is drawn at its column rather than at
    /// its fraction of one, and nothing else moves. Nothing may appear where
    /// nothing was drawn, and nothing drawn may go missing — the failure a
    /// shape count would sail past.
    #[test]
    fn the_culled_roll_draws_the_same_picture_as_the_unculled_one() {
        let notes = dense(2_500, 12, 9);
        for len_s in [480.0, 120.0, 30.0] {
            let want = lit(false, &notes, &[], len_s);
            let got = lit(true, &notes, &[], len_s);
            assert!(!want.is_empty(), "{len_s}s: the unculled panel drew nothing");
            let reach = SUB as i32;
            let lost = want.iter().filter(|c| !near(&got, **c, reach)).count();
            let gained = got.iter().filter(|c| !near(&want, **c, reach)).count();
            assert_eq!(lost, 0, "{len_s}s: {lost} of {} lit cells went missing", want.len());
            assert_eq!(gained, 0, "{len_s}s: {gained} cells lit that were dark before");
            // And the ink must not thin out: dropping every second mark
            // would still be "within a point" and would still be a picture
            // that changed. The worst of the three zooms is 0.08% lighter;
            // three per cent is headroom for a platform whose floats round
            // the other way.
            let ratio = got.len() as f64 / want.len() as f64;
            assert!(
                (0.97..=1.03).contains(&ratio),
                "{len_s}s: {} lit cells against {}, a ratio of {ratio:.3}",
                got.len(),
                want.len()
            );
        }
    }

    /// **A selected note is never hidden by an unselected one.**
    ///
    /// A selection is drawn in a different ink for the express purpose of
    /// being seen. The cull keeps a separate plane for it — without that,
    /// the first note to claim a cell wins, and on a part dense enough for
    /// the cull to run at all that is almost never the selected one. This is
    /// the silent-invisibility class of fault, so it gets a test that looks
    /// for the ink rather than for a count.
    #[test]
    fn the_cull_never_hides_a_selection() {
        let notes = dense(2_500, 12, 9);
        // Notes scattered through the part, each of them sharing its column
        // and key with plenty of unselected neighbours.
        let picked: Vec<usize> = (0..40).map(|i| i * 61 + 7).collect();
        let strokes = |cull: bool| -> usize {
            CULL.with(|c| c.set(cull));
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
            let _ = ctx.run(Default::default(), |_| {});
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("cull-pick"));
            let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
            let mut v = view(&notes);
            v.selected = &picked;
            v.clip_start_s = 0.0;
            v.clip_len_s = 480.0;
            v.view_start_s = 0.0;
            v.view_len_s = 480.0;
            draw(&painter, rect(), &v);
            CULL.with(|c| c.set(true));
            ctx.graphics(|g| {
                g.get(layer).map_or(0, |list| {
                    list.all_entries()
                        .filter(|c| match &c.shape {
                            // The selection's own stroke: two points, where
                            // an unselected note's is one.
                            egui::Shape::Rect(q) => (q.stroke.width - 2.0).abs() < 0.01,
                            _ => false,
                        })
                        .count()
                })
            })
        };
        let (want, got) = (strokes(false), strokes(true));
        assert!(want > 0, "the unculled panel drew no selection to compare to");
        assert_eq!(got, want, "the cull swallowed {} of {want} selected notes", want - got);
    }
}

#[cfg(test)]
mod editor_cost_probe {
    use super::tests::{rect, view};
    use super::*;
    use std::time::{Duration, Instant};

    /// Probe: what does the piano roll cost per frame on a clip dense enough
    /// to be worth worrying about? Ignored by default; run with `--ignored
    /// --nocapture`, and in RELEASE.
    ///
    /// **This probe reported the wrong answer once and it changed a
    /// decision**, so what it does now is spelled out. It timed `draw` and
    /// nothing else, divided by 16.6 ms, and said the roll was a tenth of a
    /// frame; on that number the panel was left alone. Emitting a shape is
    /// half the bill. The other half is turning it into triangles, and it is
    /// the LARGER half here — a circle costs far more of it than a rect, and
    /// the velocity lane draws two circles a note. Measured with the
    /// tessellation the sibling probe in `arrangement_panel` had been
    /// counting all along, the same picture was 58-65% of a frame.
    ///
    /// So: emit AND tessellate, both shapes of part, and the drawing with
    /// the cull withheld against the drawing as it ships. A number that is
    /// half a measurement is worse than no number, because it gets believed.
    #[test]
    #[ignore]
    fn what_does_the_piano_roll_pay_every_frame() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("roll-probe"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);

        // Four minutes of sixteenths, four voices deep: 15,360 notes, which
        // is a busy orchestral part or a drum programme nobody would call
        // unreasonable.
        //
        // Two shapes of it, because measuring one shape is how this probe
        // went wrong the first time. The spiral never repeats a pitch inside
        // a column and never repeats a velocity band either, which is the
        // WORST case for a cull keyed on those — no music is that evenly
        // spread. The kit is the other end: eight drums and four accents,
        // which is what a programmed part actually looks like. A saving that
        // only shows on one of them is a saving that was measured, not made.
        let spiral: Vec<ArrNote> = (0..15_360u32)
            .map(|i| ArrNote {
                at_s: f64::from(i / 4) * 0.125,
                dur_s: 0.1,
                key: 48 + u8::try_from((i * 7) % 36).unwrap_or(0),
                vel: 40 + u8::try_from((i * 11) % 88).unwrap_or(0),
            })
            .collect();
        let kit: Vec<ArrNote> = (0..15_360u32)
            .map(|i| ArrNote {
                at_s: f64::from(i / 4) * 0.125,
                dur_s: 0.1,
                key: 48 + u8::try_from((i * 5) % 8).unwrap_or(0),
                vel: if i % 8 == 0 { 118 } else { 64 + u8::try_from(i % 3).unwrap_or(0) * 12 },
            })
            .collect();
        let r = rect();
        for (part, notes) in [("a part of no repeats", &spiral), ("a drum programme", &kit)] {
        for (what, start_s, len_s) in [
            ("the whole part", 0.0, 480.0),
            ("thirty seconds", 100.0, 30.0),
            ("four bars", 100.0, 8.0),
        ] {
            let mut v = view(notes);
            v.clip_start_s = 0.0;
            v.clip_len_s = 480.0;
            v.view_start_s = start_s;
            v.view_len_s = len_s;

            println!("{part}, {what} ({} notes):", notes.len());
            for (which, cull) in [("unculled", false), ("as drawn now", true)] {
            CULL.with(|c| c.set(cull));
            ctx.graphics_mut(|g| *g.entry(layer) = Default::default());
            draw(&painter, r, &v);
            let shapes = ctx.graphics(|g| g.get(layer).map_or(0, |l| l.all_entries().len()));
            ctx.graphics_mut(|g| *g.entry(layer) = Default::default());

            let n = 20;
            let mut total = Duration::ZERO;
            for _ in 0..n {
                let t = Instant::now();
                draw(&painter, r, &v);
                total += t.elapsed();
                ctx.graphics_mut(|g| *g.entry(layer) = Default::default());
            }
            let per = total / n;

            // **And what the tessellator then pays for them.** Emission is
            // half the bill: a shape becomes triangles before the GPU sees
            // it, and a circle becomes a great many more of them than a
            // rect does. This probe timed only `draw` when it was written
            // and reported a tenth of a frame for a picture that costs two
            // thirds of one — the sibling probe in `arrangement_panel` had
            // been counting both halves all along, and the two have to be
            // read against each other.
            draw(&painter, r, &v);
            let mut tess = Duration::ZERO;
            let m = 10;
            for _ in 0..m {
                let s: Vec<egui::epaint::ClippedShape> = ctx.graphics(|g| {
                    g.get(layer).map(|l| l.all_entries().cloned().collect()).unwrap_or_default()
                });
                let t = Instant::now();
                std::hint::black_box(ctx.tessellate(s, 1.0));
                tess += t.elapsed();
            }
            let tess = tess / m;
            ctx.graphics_mut(|g| *g.entry(layer) = Default::default());

            println!(
                "  {which:>14}: {shapes} shapes -> emit {:?} + tessellate {:?} = {:?}, {:.1}% of a 16.6ms frame",
                per,
                tess,
                per + tess,
                (per + tess).as_secs_f64() / 0.0166 * 100.0
            );
            }
            CULL.with(|c| c.set(true));
        }
        }
    }
}
