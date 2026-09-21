//! The MIDI editor: a piano roll in a popup over the arrangement.
//!
//! A pure painter with a layout and a hit test, like every panel — the app
//! owns the document and the gestures; this file owns where things are and
//! what a press means. The one piece of real machinery is the GRID, which
//! has three shapes — straight, swing, triplet — and [`snap_t`] is the one
//! authority for all three: the drawn lines and the magnet come from the
//! same arithmetic, so what you see is exactly what a note lands on.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use crate::arrangement::{ArrNote, ArrPedal};

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

/// Where the clip's name starts, in from the strip's left edge.
const NAME_X: f32 = 8.0;

/// **The name's reserve**: the room the strip keeps for the clip's name
/// before the navigator is allowed any. Fifteen characters of the strip's
/// monospace, about — enough to say "Piano verse 2" — and the navigator is
/// what gives way on an editor too narrow for both, because a clip you
/// cannot name is worse than a clip you cannot scroll: the wheel still
/// scrolls it.
const NAV_NAME_RESERVE: f32 = 110.0;

/// The narrowest navigator worth laying out. Under this the box is a line
/// with a window a few points wide on it, and a hand cannot tell the
/// window from its own edges — the arrangement drops its own at one and a
/// half cells for the same reason.
const NAV_MIN_W: f32 = 72.0;

/// **The navigator's share of the strip**, and its ceiling. Four tenths,
/// because the box IS the control's resolution: a second of the clip is
/// the box's width over the clip's length, and on a four-minute part a
/// hundred-point box gives a hand under half a second a point, which is
/// coarser than the notes it is trying to reach. The ceiling stops it
/// running away with a wide window's title bar.
const NAV_SHARE: f32 = 0.40;
const NAV_MAX_W: f32 = 560.0;

/// **How narrow the navigator's window may draw.** The band's viewport and
/// the arrangement's inner box both floor at three points, for the same
/// reason: a two-second window on a ten-minute clip is a fraction of a
/// point wide, and a thing you cannot see is a thing you cannot grab.
const NAV_WINDOW_MIN_W: f32 = 3.0;

/// Everything the editor draws, pushed in. Times are ABSOLUTE timeline
/// seconds — the clip's own trim (`offset_s`) is paid at the boundary, so
/// the roll and the arrangement agree where a bar stands.
pub struct MidiEditView<'a> {
    pub name: &'a str,
    pub clip_start_s: f64,
    pub clip_len_s: f64,
    /// **How long ONE PASS of the clip is**, or zero for a clip that does not
    /// loop. See `crate::arrangement::ArrClip::loop_s`.
    ///
    /// The roll draws the clip's notes ONCE — they are held once, and the
    /// repeats are laid down where the clip is scheduled — so without this
    /// the editor had no way to say where the part it is showing ends and the
    /// repetition begins. It is what [`loop_bar_x`] stands on.
    pub loop_s: f64,
    pub offset_s: f64,
    pub notes: &'a [ArrNote],
    /// **The clip's pedal**, sorted by `at_s` and in the same source seconds
    /// the notes speak. Drawn in the bottom lane when [`Self::pedal_lane`] is
    /// on, and never anywhere else — a pedal is not a note and does not
    /// belong on the roll.
    pub pedal: &'a [ArrPedal],
    /// **Which curve the bottom lane holds**: the pedal, or velocity.
    ///
    /// One lane rather than two. The editor is a tool strip, a ruler, the
    /// roll and a lane, and every row added to that stack is height the roll
    /// loses — the argument that put the navigator in the strip's unused
    /// middle rather than on a row of its own. Both of these are a curve
    /// under the notes, and neither is wanted at the same instant as the
    /// other.
    pub pedal_lane: bool,
    /// The SELECTED notes, sorted indices — the editor works on sets now.
    pub selected: &'a [usize],
    /// **Which keys are SOUNDING right now**, indexed by MIDI number.
    ///
    /// The editor auditions a note on three roads — a press on the piano
    /// column, an arrow that moves the pitch cursor, a note written or
    /// grabbed — and until this field existed not one of them showed
    /// anything. The sound came out and the picture said nothing, so a
    /// player walking up the keyboard on the arrows had no way to see where
    /// they were except by counting rows, and a press on a key that produced
    /// no sound (a lane with no instrument) was indistinguishable from one
    /// that had not registered at all. The owner: "we need proper visual
    /// feedback for arrow and click keyboard auditioning in the midi editor.
    /// Right now both of those actions provide no visual feedback."
    ///
    /// An array rather than a slice of keys: it is read once per drawn row
    /// and a linear search per row over a held chord is the wrong shape for
    /// the inner loop of a painter. 128 bytes on the stack, copied in.
    pub lit: [bool; 128],
    /// A marquee in flight, in screen space, drawn as a dashed box.
    pub marquee: Option<(Pos2, Pos2)>,
    /// The window, absolute seconds.
    pub view_start_s: f64,
    pub view_len_s: f64,
    /// **The navigator's world**, absolute seconds: what the strip's
    /// miniature spans end to end. The app computes it — the clip with the
    /// pad `me_fit_clip` gives it, stretched to hold the window whenever the
    /// wheel has put the window outside that — and freezes it for the life
    /// of a navigator drag, so the box's scale cannot change under a hand
    /// that is holding something drawn on it. Pushed in as data rather than
    /// derived here, because the pad's own formula lives in the app beside
    /// the three other places that use it, and a second copy of it in this
    /// file would be the copy that drifts.
    pub nav_start_s: f64,
    pub nav_len_s: f64,
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
    /// The lane's colour index into [`crate::mixer_panel::STRIP_COLORS`]; 0 draws the desk
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
    /// **The PED chip**: swap the bottom lane between velocity and the pedal.
    ToolPedal,
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
    /// **The PEDAL lane**, at `(absolute seconds, value 0..=127)`.
    ///
    /// The value comes from the height of the press, which is what lets one
    /// gesture do both jobs: a drag along the top paints the damper down,
    /// and a drag along the BOTTOM paints it up, because a level of zero is
    /// a pedal that is not pressed. There is no separate erase to find.
    Pedal(f64, u8),
    /// **The strip's navigator, by its parts** — the band's overview strip
    /// brought to the roll, scaled to the clip. The lit WINDOW is the part
    /// of the clip the roll is showing: drag it to pan. The window's two
    /// edges resize it, which is the zoom. Anywhere else in the box is the
    /// clip itself, at `absolute seconds`: a press there is a locate, and the
    /// locate follows the hand while it is held, exactly as a press on the
    /// band's timeline does.
    NavWindow,
    NavLeft,
    NavRight,
    NavField(f64),
    /// **The LOOP BAR**: the clip's right end, dragged left to shorten the
    /// loop and right to lengthen it. See [`loop_bar_x`].
    LoopBar,
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
    /// **The lane's switch**: PED. See `MidiEditView::pedal_lane`.
    pub tool_pedal: Rect,
    /// **The navigator**: the whole clip in miniature, in the strip's room
    /// between the clip's name and the pencil. `NOTHING` when the strip has
    /// no room for one worth having — see [`NAV_MIN_W`].
    pub tool_nav: Rect,
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
                tool_pedal: Rect::NOTHING,
                tool_triplet: Rect::NOTHING,
                tool_nav: Rect::NOTHING,
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
        // **PED, at the pencil's shoulder.** The two of them are the strip's
        // MODE switches — one says what a press writes, the other says what
        // the lane holds — so they sit together at the end of the row rather
        // than in among the grid chips, which are all settings of one thing.
        let tool_pedal = cell(side * 1.6);
        // **The NAVIGATOR, in the strip's own room.** The editor is already
        // a tool strip, a ruler, the roll and a velocity lane, and every row
        // added to that stack is height the roll loses — so the miniature
        // goes where the arrangement put its navigator: among the tools, in
        // the space the strip was not using. The strip's middle was empty
        // between the clip's name and the pencil on any editor wider than a
        // few hundred points, and a box the chips' height is exactly what
        // the arrangement's corner holds.
        //
        // It sits at the PENCIL's shoulder and walks left from there, so it
        // reads with the tools rather than with the title, and the gap
        // before it is wider than the chips' own so it does not read as one
        // more chip. The name keeps its reserve first (`NAV_NAME_RESERVE`),
        // the navigator takes its share of the strip out of what is left,
        // and under `NAV_MIN_W` it is not laid out at all: a box that
        // narrow is a line, and a hand cannot find a window on a line.
        let nav_right = tool_pedal.left() - pad * 2.0;
        let nav_left_limit = strip.left() + NAME_X + NAV_NAME_RESERVE + pad;
        let nav_w = (nav_right - nav_left_limit)
            .min((strip.width() * NAV_SHARE).clamp(NAV_MIN_W, NAV_MAX_W));
        let tool_nav = if nav_w >= NAV_MIN_W {
            Rect::from_min_max(
                Pos2::new(nav_right - nav_w, strip.top() + pad),
                Pos2::new(nav_right, strip.bottom() - pad),
            )
        } else {
            Rect::NOTHING
        };
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
            tool_pedal,
            tool_triplet,
            tool_nav,
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

/// **The pedal value a press at `y` means**: the lane's floor is 0 and its
/// ceiling is 127.
///
/// Its own function because the painter and the hit test have to agree
/// exactly — a shelf drawn at a height the hand cannot put it back at is a
/// control that lies — and because the app reads it again while a drag is in
/// flight, from outside this module.
pub fn pedal_value_at(l: &Layout, y: f32) -> u8 {
    let h = l.vel.height().max(1.0);
    // **The rails, and they are not a nicety.** Fully down and fully up are
    // the two values anybody actually wants from this lane — a pedal is
    // mostly a switch and the sweep between is the rare case — and a strict
    // pixel mapping makes those two the HARDEST to hit: the lane is sixty
    // points for a hundred and twenty-eight values, so the top pixel reads
    // 125 and only a hand exactly on the boundary reads 127. A press within
    // a couple of points of either end means the end.
    const RAIL: f32 = 2.5;
    if y <= l.vel.top() + RAIL {
        return 127;
    }
    if y >= l.vel.bottom() - RAIL {
        return 0;
    }
    // Rounded and not truncated, so the value under the hand is the nearest
    // one rather than the one below it — half a step of bias over the whole
    // lane, which on a shelf drawn at that height is visible.
    (((l.vel.bottom() - y) / h) * 127.0).round().clamp(0.0, 127.0) as u8
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

// ── the loop bar ───────────────────────────────────────────────────────────

/// **How wide the loop bar is drawn**, in points. Thin: it is a boundary, not
/// a wall, and the roll behind it has to stay readable through the column.
const LOOP_BAR_W: f32 = 3.0;

/// **How far either side of the bar a press still means it.**
///
/// Generous against the three points it is drawn at, because the thing being
/// aimed at is a line — the same bargain `EDGE_SLOP` makes around a note's
/// border. Symmetric, unlike a note's, because the bar sits at the clip's
/// right END: half of the target a hand naturally aims at is on the empty side
/// past the clip, where there is nothing else to hit.
///
/// `pub` for the app's own drag test, which measures the grab it presses at
/// rather than restating the number.
pub const LOOP_BAR_GRAB: f32 = 5.0;

/// **Where the loop's right end stands on the roll**, or nothing.
///
/// `None` on a clip that does not loop — every audio clip, and every MIDI
/// clip glued or written before looping existed — and `None` when that second
/// is off the roll: a bar drawn where it cannot be seen is a bar a hand can
/// never find, and one hit-tested off the roll would claim presses on the
/// piano column or past the right edge.
///
/// The one authority for the bar, like every other pair in this file: the
/// painter and the hit test both come here, so what is drawn is what answers.
pub fn loop_bar_x(l: &Layout, view: &MidiEditView<'_>) -> Option<f32> {
    // `is_finite` after the comparison and not instead of it: a NaN loop
    // fails every comparison it is given, so `<=` alone lets it through.
    if !l.roll.is_positive() || view.loop_s <= 1.0e-6 || !view.loop_s.is_finite() {
        return None;
    }
    let x = x_of(l, view, view.clip_start_s + view.loop_s);
    (x.is_finite() && x >= l.roll.left() && x <= l.roll.right()).then_some(x)
}

// ── the navigator's axis ───────────────────────────────────────────────────
//
// The box IS the clip: its left edge is `nav_start_s`, its right edge is
// `nav_start_s + nav_len_s`, and the two functions below are the only
// mapping between the two. The painter, the hit test and the app's drag all
// go through them, which is what the roll's own axis learned the hard way:
// a drag that did its own arithmetic would be a third mapping, and the
// third mapping is the one that drifts.

/// Where absolute second `t` stands on the navigator.
pub fn nav_x_of(l: &Layout, view: &MidiEditView<'_>, t: f64) -> f32 {
    let f = (t - view.nav_start_s) / view.nav_len_s.max(1.0e-9);
    l.tool_nav.left() + l.tool_nav.width() * f as f32
}

/// The absolute second under `x` on the navigator. Unclamped: a hand that
/// has wandered off the box while holding the window is still holding it,
/// and it is the app's rails that decide where the window may go.
pub fn nav_time_at(l: &Layout, view: &MidiEditView<'_>, x: f32) -> f64 {
    let w = f64::from(l.tool_nav.width().max(1.0));
    view.nav_start_s + f64::from(x - l.tool_nav.left()) / w * view.nav_len_s
}

/// **The navigator's WINDOW** — where the roll's view stands on the box.
///
/// One function for the picture and the grab, the band's rule for its own
/// viewport, so the rectangle a hand picks up is the rectangle it can see.
/// Clamped into the box, because a window the wheel has pushed past the
/// clip is drawn where the box ends rather than off it, and never narrower
/// than [`NAV_WINDOW_MIN_W`]. `NOTHING` where there is no navigator.
pub fn nav_window(l: &Layout, view: &MidiEditView<'_>) -> Rect {
    let b = l.tool_nav;
    if !b.is_positive() {
        return Rect::NOTHING;
    }
    let x0 = nav_x_of(l, view, view.view_start_s).clamp(b.left(), b.right());
    let x1 = nav_x_of(l, view, view.view_start_s + view.view_len_s).clamp(b.left(), b.right());
    // The floor is paid on the RIGHT, and the pair is pushed back in only
    // if that overran the box — so a window at the clip's very end still
    // ends at the box's edge.
    let x1 = x1.max(x0 + NAV_WINDOW_MIN_W);
    let shove = (x1 - b.right()).max(0.0);
    Rect::from_min_max(
        Pos2::new(x0 - shove, b.top() + 1.5),
        Pos2::new(x1 - shove, b.bottom() - 1.5),
    )
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
    // **The magnet and QUANTIZE are one implementation**, and this is the
    // line that makes that true rather than aspirational. `Grid::nearest`
    // holds the arithmetic - straight, triplet and swung - so a note dropped
    // by hand and a note moved by a quantize land on the same frame by
    // construction instead of by two functions agreeing.
    //
    // What stays here is what belongs to the EDITOR and not to the grid:
    // whether the magnet is armed at all, and that nothing is ever dragged
    // before the start of the timeline.
    let Some(grid) = view_grid(view) else {
        return t.max(0.0);
    };
    grid.nearest(t).max(0.0)
}

/// **This view's grid**, or `None` when it does not describe one.
///
/// The bridge between the editor's loose terms - a bool for swing, a
/// percentage beside it - and the value the rest of the app quantizes
/// against. Triplet beats swing here because they are mutually exclusive in
/// this app and the editor has always resolved them that way.
pub fn view_grid(view: &MidiEditView<'_>) -> Option<crate::quantize::Grid> {
    crate::quantize::Grid::new(
        grid_step_s(view),
        (view.swing && !view.triplet).then(|| view.swing_pct.clamp(0.5, 0.95)),
    )
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

/// **What a sounding key is drawn in.** One colour for both the piano column
/// and the roll's lane, so the two halves of the same signal cannot drift.
///
/// A warm amber rather than the theme's ink: this is the one mark on the
/// editor that means "right now", and every other thing here — selection,
/// velocity, the grid — is already spending the theme's own range. A player
/// has to be able to tell a key that is SOUNDING from a note that is
/// selected at a glance, and two shades of the same hue would not do it.
const LIT_KEY: Color32 = Color32::from_rgb(0xFF, 0xB0, 0x3A);

/// The gridbox's word: the arrangement's bars-as-a-power-of-two, with the
/// grid's shape appended — `1/8T`, `1/8 sw`.
pub fn grid_word(view: &MidiEditView<'_>) -> String {
    // The editor's own meter, so the word matches the magnet here too —
    // see `grid_label`, which now derives the name from the division.
    //
    // **`grid_label_text`, and he has now said otherwise.** The two
    // spellings split when the owner asked for the arrange toolbar's quarter
    // rung to read "1/4" instead of `\u{266a}` — he asked about ARRANGE, and
    // a shared helper would have moved this chip in the same edit without
    // anyone deciding to, so this call was left on the glyph with a note
    // saying it waited on him. It was not waiting long: "Replace the note
    // glyph with 1/4 in midi editor."
    //
    // The two spellings stay two functions even though both callers now want
    // the same one. The split is not dead weight: it is the record of a
    // decision that was made twice, and collapsing it would take the next
    // surface with it by accident, which is the fault the split was built to
    // prevent in the first place.
    let base = crate::arrangement_panel::grid_label_text(
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
        Stroke::new(1.5_f32, ink.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );

    // ── the strip ─────────────────────────────────────────────────────────
    let font = |size: f32| FontId::new(size, crate::fonts::courier());
    painter.rect_filled(l.strip, 4.0, Color32::from_rgba_premultiplied(0, 0, 0, 60));
    // **The name stops where the navigator starts.** It used to run on
    // under the chips unchecked, which nobody noticed while the strip's
    // middle was empty; with a box drawn there a long name would print
    // straight through the miniature. Cut with the arrangement's own
    // ellipsis, to the pencil when there is no navigator.
    {
        let name_font = font(STRIP_H * 0.45);
        let wall = if l.tool_nav.is_positive() {
            l.tool_nav.left()
        } else {
            l.tool_pedal.left()
        };
        let room = wall - 6.0 - (l.strip.left() + NAME_X);
        let name = crate::arrangement_panel::cut_to(painter, room.max(0.0), view.name, &name_font);
        painter.text(
            Pos2::new(l.strip.left() + NAME_X, l.strip.center().y),
            Align2::LEFT_CENTER,
            name,
            name_font,
            ink,
        );
    }
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
            Stroke::new(1.0_f32, ink.gamma_multiply(if lit { 0.9 } else { 0.4 })),
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
            Stroke::new(1.0_f32, ink.gamma_multiply(if view.draw_mode { 0.9 } else { 0.4 })),
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
    // **PED: what the bottom lane holds.** A word and not a glyph, like
    // SNAP beside it — the strip is text-first, and a damper pedal has no
    // icon anybody reads at sixteen points. Lit means the lane is showing
    // the pedal; dark means velocity, which is how it ships.
    chip(painter, l.tool_pedal, "PED", view.pedal_lane);
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
                Stroke::new(1.5_f32, ink.gamma_multiply(0.7)),
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
            Stroke::new(1.2_f32, ink),
            egui::StrokeKind::Inside,
        );
        painter.rect_stroke(
            Rect::from_min_size(Pos2::new(c.x - a * 0.3, c.y - a), Vec2::splat(a * 1.3)),
            1.0,
            Stroke::new(1.2_f32, ink),
            egui::StrokeKind::Inside,
        );
    }

    // ── the navigator: the whole clip in miniature, the window lit ───────
    //
    // The band's overview strip, scaled to the clip: a recessed box that
    // spans the navigator's world, the notes drawn small inside it so the
    // shape of the part shows, the roll's window as a lit rectangle a hand
    // can pick up, and the playhead. Drawn whether or not the whole clip
    // already fits the roll — at the fit the window fills the box, and
    // that is a true statement about the view rather than furniture: its
    // two edges are still the way IN, and a navigator that appeared only
    // once you had zoomed with the wheel would be no way to zoom without
    // the wheel, which is the thing it is for.
    if l.tool_nav.is_positive() {
        let b = l.tool_nav;
        painter.rect_filled(b, 2.0, Color32::from_rgba_premultiplied(0, 0, 0, 70));
        // The clip's own span, a shade lighter than the pad either side of
        // it, so the picture says where the clip begins and ends the way the
        // roll's veil does.
        let cx0 = nav_x_of(&l, view, view.clip_start_s).clamp(b.left(), b.right());
        let cx1 = nav_x_of(&l, view, view.clip_start_s + view.clip_len_s).clamp(b.left(), b.right());
        if cx1 > cx0 {
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(cx0, b.top()), Pos2::new(cx1, b.bottom())),
                0.0,
                ink.gamma_multiply(0.07),
            );
        }
        // **The notes, against their OWN pitch span.** The box is eighteen
        // points tall and a keyboard is a hundred and twenty-eight keys, so
        // a miniature drawn against the whole keyboard would be one flat
        // line whatever the part did. Spread across the notes' actual range
        // — never less than an octave, so a one-note part is not a line
        // through the middle either — the marks rise and fall with the
        // tune, which is the shape the owner wants to see. An impression,
        // not a score: a mark is one point tall at most spans.
        let (lo, hi) = view.notes.iter().fold((127u8, 0u8), |(lo, hi), n| {
            (lo.min(n.key), hi.max(n.key))
        });
        if lo <= hi {
            let range = f32::from(hi - lo) + 1.0;
            let span = range.max(12.0);
            // The key the box's floor stands on: the lowest note, or the
            // octave centred on the part when the part spans less than one.
            let base = f32::from(lo) - (span - range) * 0.5;
            let inner = b.shrink2(Vec2::new(0.0, 1.0));
            let mark_h = (inner.height() / span).max(1.0);
            // The same cull the roll uses, for the same reason: a part
            // dense enough to be worth a navigator is a part with far more
            // notes than the box has columns. A note narrower than a column
            // in a column already holding that key IS the mark already
            // there. See [`Coverage`].
            let scale = f64::from(b.width()) / view.nav_len_s.max(1.0e-9);
            let mut seen = coverage(b.width(), view.notes.len());
            for (i, n) in view.notes.iter().enumerate() {
                let t0 = view.clip_start_s + (n.at_s - view.offset_s);
                let x0 = nav_x_of(&l, view, t0);
                let x1 = nav_x_of(&l, view, t0 + n.dur_s.max(0.02)).max(x0 + 1.0);
                if x1 < b.left() || x0 > b.right() {
                    continue;
                }
                let picked = view.selected.binary_search(&i).is_ok();
                if let Some(seen) = seen.as_mut() {
                    if n.dur_s.max(0.02) * scale < 1.0
                        && !seen.claim(x0, b.left(), usize::from(n.key), picked)
                    {
                        continue;
                    }
                }
                let y1 = inner.bottom() - (f32::from(n.key) - base) / span * inner.height();
                let r = Rect::from_min_max(
                    Pos2::new(x0.max(b.left()), (y1 - mark_h).max(inner.top())),
                    Pos2::new(x1.min(b.right()), y1.min(inner.bottom())),
                );
                if r.is_positive() {
                    painter.rect_filled(
                        r,
                        0.0,
                        if picked { ink } else { ink.gamma_multiply(0.55) },
                    );
                }
            }
        }
        // The window, and its two handles. Drawn from `nav_window`, which is
        // the rectangle the hit test answers for, so the grab and the picture
        // cannot drift.
        let w = nav_window(&l, view);
        if w.is_positive() {
            painter.rect_filled(w, 1.5, ink.gamma_multiply(0.20));
            painter.rect_stroke(
                w,
                1.5,
                Stroke::new(1.0_f32, ink.gamma_multiply(0.85)),
                egui::StrokeKind::Inside,
            );
            // The edges are the zoom, so they are drawn as something to take
            // hold of: a bar two points wide at each end, the same weight a
            // selected note's rim carries.
            for x in [w.left(), w.right() - 2.0] {
                painter.rect_filled(
                    Rect::from_min_max(Pos2::new(x, w.top()), Pos2::new(x + 2.0, w.bottom())),
                    0.0,
                    ink.gamma_multiply(0.9),
                );
            }
        }
        // The playhead, in the roll's own red, so a scrub on the box shows
        // where it put the transport before the roll has scrolled to it.
        let px = nav_x_of(&l, view, view.playhead_s);
        if px >= b.left() && px <= b.right() {
            painter.line_segment(
                [Pos2::new(px, b.top()), Pos2::new(px, b.bottom())],
                Stroke::new(1.0_f32, Color32::from_rgb(0xE8, 0x3A, 0x4E)),
            );
        }
        painter.rect_stroke(
            b,
            2.0,
            Stroke::new(1.0_f32, ink.gamma_multiply(0.4)),
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
        let lit = view.lit[usize::from(key)];
        if row_keys.is_positive() {
            // **A sounding key is LIT, and the same ink either colour of
            // key.** A white key pressed is drawn down toward the face and a
            // black key up toward the ink, which is what a real keyboard
            // does — the key moves toward the instrument — and it means the
            // highlight reads on both without needing two colours somebody
            // has to keep in step.
            painter.rect_filled(
                row_keys,
                0.0,
                match (lit, is_black(key)) {
                    (true, _) => LIT_KEY,
                    (false, true) => Color32::from_rgb(0x1A, 0x16, 0x12),
                    (false, false) => ink.gamma_multiply(0.85),
                },
            );
            if key % 12 == 0 && kh >= 7.0 {
                painter.text(
                    Pos2::new(row_keys.right() - 3.0, row_keys.center().y),
                    Align2::RIGHT_CENTER,
                    format!("C{}", i32::from(key / 12) - 1),
                    font((kh * 0.7).min(10.0)),
                    if lit || !is_black(key) { face } else { ink },
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
                            if lit || !is_black(key) { face } else { ink },
                        );
                    }
                }
            }
        }
        if row_roll.is_positive() {
            if is_black(key) {
                painter.rect_filled(row_roll, 0.0, face_row_dark);
            }
            // **And the ROLL says so too**, faintly, all the way across. The
            // piano column is eighty points wide at the far left and the eye
            // editing a part is not on it; a lane that lights under the notes
            // is what makes an arrow walking the keyboard visible without
            // looking away from the work.
            if lit {
                painter.rect_filled(row_roll, 0.0, LIT_KEY.gamma_multiply(0.22));
            }
            // Every C draws its line: the octave is the roll's ruler.
            if key % 12 == 0 {
                painter.line_segment(
                    [
                        Pos2::new(row_roll.left(), row_roll.bottom()),
                        Pos2::new(row_roll.right(), row_roll.bottom()),
                    ],
                    Stroke::new(1.0_f32, ink.gamma_multiply(0.16)),
                );
            }
        }
    }
    painter.line_segment(
        [l.keys.right_top(), l.keys.right_bottom()],
        Stroke::new(1.0_f32, ink.gamma_multiply(0.4)),
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
                1.0_f32,
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
        // **A MUTED note is drawn HOLLOW.** It keeps its place, its length
        // and its outline — it is still there to be dragged, resized and
        // selected — and loses the fill, which is the part that says a note
        // sounds. Dimming the fill instead would have read as a quiet note,
        // which is the one thing it must not be confused with: this roll
        // paints velocity as colour, so a washed-out note already means
        // something else here.
        let hue = velocity_hue(vel_loud, vel_quiet, face, n.vel);
        if n.muted {
            painter.rect_filled(r, 2.0, hue.gamma_multiply(0.16));
        } else {
            painter.rect_filled(r, 2.0, hue);
        }
        painter.rect_stroke(
            r,
            2.0,
            if picked {
                Stroke::new(2.0_f32, ink)
            } else if n.muted {
                Stroke::new(1.0_f32, hue)
            } else {
                Stroke::new(1.0_f32, Color32::from_black_alpha(140))
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
                        Stroke::new(1.0_f32, ink.gamma_multiply(0.6)),
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
                                Stroke::new(1.0_f32, ink.gamma_multiply(0.35)),
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

    // ── the PEDAL lane, when the strip's PED chip is lit ────────────────
    //
    // **A step, not a curve.** The pedal is a controller that HOLDS its
    // value: it stands where it was last put until something moves it, so
    // between two edges the right picture is a flat shelf and a vertical
    // riser, never a slope. A line drawn point-to-point would say the damper
    // is easing off through a bar it is in fact holding flat — a picture of
    // an interpolation nothing performs.
    //
    // Filled down to the floor rather than drawn as a bare line, because
    // what a player wants from this lane at a glance is WHERE THE PEDAL IS
    // DOWN, and a filled span answers that from across the room while a
    // 2-point line has to be read.
    if l.vel.is_positive() && view.pedal_lane {
        painter.rect_filled(l.vel, 0.0, Color32::from_rgba_premultiplied(0, 0, 0, 70));
        let vel_loud = Color32::from_rgb(view.vel_loud.0, view.vel_loud.1, view.vel_loud.2);
        let vel_quiet =
            Color32::from_rgb(view.vel_quiet.0, view.vel_quiet.1, view.vel_quiet.2);
        // **The value's own colour, through the velocity lane's gradient.**
        //
        // Not `vel_loud` raw, and for two reasons. The colour then CARRIES
        // the reading — a half-pedal is a different colour from a full one,
        // which is a second telling of the half-pedalling this lane exists
        // to keep — and `velocity_hue` is where the contrast guard already
        // lives, so a theme whose hot end sits almost on the ground gets
        // pushed off it instead of drawing a shelf nobody can see. A second
        // colour path written here is how this file grows an eighth instance
        // of the ink bug.
        let hue = |v: u8| velocity_hue(vel_loud, vel_quiet, face, v);
        let y_of_v = |v: u8| l.vel.bottom() - l.vel.height() * (f32::from(v.min(127)) / 127.0);
        // **The clip's own span, which is as far as this lane's marks may
        // go.** Past the clip's end the timeline releases the pedal whether
        // or not anybody wrote a lift — see the engine's `clip_pedal_level`
        // — so a shelf drawn to the lane's edge would be a picture
        // disagreeing with the sound.
        let cx0 = x_of(&l, view, view.clip_start_s).max(l.vel.left());
        let cx1 = x_of(&l, view, view.clip_start_s + view.clip_len_s).min(l.vel.right());
        // **The damper itself.** One walk, closing each shelf as the next
        // edge opens — and the last shelf runs to the lane's right edge,
        // because a pedal with no lift after it is held to the end of what
        // is being shown. The clip releases it at its own end; that is the
        // timeline's business and not a mark this lane can honestly draw.
        let mut standing = 0_u8;
        let mut last_x = cx0;
        let shelf = |painter: &Painter, from: f32, to: f32, v: u8| {
            let (a, b) = (from.max(cx0), to.min(cx1));
            if v == 0 || b <= a {
                return;
            }
            let r = Rect::from_min_max(Pos2::new(a, y_of_v(v)), Pos2::new(b, l.vel.bottom()));
            if r.is_positive() {
                painter.rect_filled(r, 0.0, hue(v).gamma_multiply(0.30));
                // The shelf's own top line, at full strength: the fill says
                // "held" and this says "held at THIS much", which is the
                // half-pedalling the value exists to keep.
                painter.line_segment(
                    [Pos2::new(r.left(), r.top()), Pos2::new(r.right(), r.top())],
                    Stroke::new(1.6_f32, hue(v)),
                );
            }
        };
        for e in view.pedal.iter().filter(|e| e.cc == 64) {
            let x = x_of(&l, view, view.clip_start_s + (e.at_s - view.offset_s));
            shelf(painter, last_x, x, standing);
            // The RISER: the vertical the step turns on. Drawn between the
            // two levels rather than from the floor, so a sweep from 40 to
            // 127 reads as one move and not as two separate presses.
            // In the colour of whichever end is HIGHER, because that is the
            // one the eye is following up or down.
            if x > cx0 && x < cx1 && (standing > 0 || e.value > 0) {
                painter.line_segment(
                    [Pos2::new(x, y_of_v(standing)), Pos2::new(x, y_of_v(e.value))],
                    Stroke::new(1.6_f32, hue(standing.max(e.value))),
                );
            }
            standing = e.value;
            last_x = x;
        }
        shelf(painter, last_x, cx1, standing);

        // **Sostenuto and soft, AFTER the damper and on top of it.**
        //
        // They ride along as thin marks because a part that uses them
        // should not look like a part that does not — but they are not
        // what the hand is drawing here, so they are two lines and not
        // two more shelves.
        //
        // Drawn LAST, which is not tidiness: a damper held at 127 fills
        // the lane to its own ceiling, and a rail two points below that
        // ceiling was painted over by exactly the passages where a
        // pianist is most likely to be using both feet.
        for cc in [66_u8, 67] {
            let y = if cc == 66 { l.vel.top() + 2.0 } else { l.vel.top() + 5.0 };
            let rail = |painter: &Painter, from: f32, to: f32, v: u8| {
                let (a, b) = (from.max(cx0), to.min(cx1));
                if v == 0 || b <= a {
                    return;
                }
                painter.line_segment(
                    [Pos2::new(a, y), Pos2::new(b, y)],
                    Stroke::new(1.5_f32, ink.gamma_multiply(0.45)),
                );
            };
            let mut standing = 0_u8;
            let mut last_x = cx0;
            for e in view.pedal.iter().filter(|e| e.cc == cc) {
                let x = x_of(&l, view, view.clip_start_s + (e.at_s - view.offset_s));
                rail(painter, last_x, x, standing);
                standing = e.value;
                last_x = x;
            }
            rail(painter, last_x, cx1, standing);
        }
        // A floor line, so an empty lane still reads as a lane rather than
        // as a gap under the roll.
        painter.line_segment(
            [
                Pos2::new(l.vel.left(), l.vel.bottom() - 0.5),
                Pos2::new(l.vel.right(), l.vel.bottom() - 0.5),
            ],
            Stroke::new(1.0_f32, ink.gamma_multiply(0.25)),
        );
    }

    // ── the velocity lane: one bar per note, selected ones lit ──────────
    if l.vel.is_positive() && !view.pedal_lane {
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
            painter.circle_stroke(top, VEL_TOP_R, Stroke::new(1.2_f32, shaft));
        }
    }

    // ── the marquee, while a hand draws one ──────────────────────────────
    if let Some((a, b)) = view.marquee {
        let r = Rect::from_two_pos(a, b);
        painter.rect_stroke(
            r,
            0.0,
            Stroke::new(1.0_f32, ink.gamma_multiply(0.8)),
            egui::StrokeKind::Inside,
        );
        // UNmultiplied — the premultiplied form with RGB far above the
        // alpha composited near-additively and painted the band white;
        // same fix as the arrangement's ClipBand.
        painter.rect_filled(r, 0.0, Color32::from_rgba_unmultiplied(0xff, 0xf4, 0xe0, 28));
    }

    // ── the loop bar: the clip's right end, and it can be pulled ─────────
    //
    // The owner: "In midi editor, a bar on the right end of clip/loop will be
    // draggable to right or left, shortening or lengthening the loop."
    //
    // Drawn LAST but one, over the roll and over the ruler both, because it
    // is the one thing in this panel that says how long the part is: the roll
    // shows a clip's notes once, and the repeats happen out on the timeline
    // where this editor cannot see them, so without a mark at the turn the
    // editor cannot tell a two-bar loop from a two-bar clip. Under the
    // playhead, which stays the line that wins — the transport is where you
    // ARE and this is where the part ends.
    //
    // The tab in the ruler is what says "pull me". A bare line through the
    // roll reads as the clip's edge, which it also is, and an edge is not
    // obviously a handle; a tab standing on the ruler is, and it puts a piece
    // of the target in a band that no note can ever cover.
    if let Some(x) = loop_bar_x(&l, view) {
        let half = LOOP_BAR_W * 0.5;
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x - half, l.roll.top()),
                Pos2::new(x + half, l.roll.bottom()),
            ),
            0.0,
            ink.gamma_multiply(0.55),
        );
        if l.ruler.is_positive() {
            let tab = Rect::from_min_max(
                Pos2::new(x - LOOP_BAR_GRAB, l.ruler.top() + 1.0),
                Pos2::new(x + LOOP_BAR_GRAB, l.ruler.bottom() - 1.0),
            );
            painter.rect_filled(tab, 1.5, ink.gamma_multiply(0.75));
            // Two grip lines down the tab, the ribbing every handle in this
            // app wears, in the FACE so they read as cuts in the tab rather
            // than as more ink beside it.
            for dx in [-1.5, 1.5] {
                painter.line_segment(
                    [
                        Pos2::new(x + dx, tab.top() + 2.0),
                        Pos2::new(x + dx, tab.bottom() - 2.0),
                    ],
                    Stroke::new(1.0_f32, face),
                );
            }
        }
    }

    // ── the playhead ─────────────────────────────────────────────────────
    let px = x_of(&l, view, view.playhead_s);
    if px >= l.roll.left() && px <= l.roll.right() {
        painter.line_segment(
            [Pos2::new(px, l.roll.top()), Pos2::new(px, l.roll.bottom())],
            Stroke::new(1.5_f32, Color32::from_rgb(0xE8, 0x3A, 0x4E)),
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
        (l.tool_pedal, Hit::ToolPedal),
    ] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    // **The navigator, by its parts.** The window is tested on X alone and
    // the box on Y, because the window is inset a point and a half from the
    // box's top and bottom and a press on that inset is a press on the
    // window — the band's viewport answers over its whole height for the
    // same reason. The edges follow a NOTE's rule exactly: `EDGE_W` inside a
    // window wide enough to keep a body, nothing inside one that is not, and
    // `EDGE_SLOP` outside either way — so a two-second window on a ten-minute
    // clip, three points wide, still moves when it is grabbed and still
    // zooms from just beside it.
    if l.tool_nav.is_positive() && l.tool_nav.contains(pos) {
        let w = nav_window(&l, view);
        if w.is_positive() {
            let edge = if w.width() >= 14.0 {
                EDGE_W.min(w.width() * 0.3)
            } else {
                0.0
            };
            if pos.x >= w.left() && pos.x <= w.right() {
                if edge > 0.0 && pos.x >= w.right() - edge {
                    return Some(Hit::NavRight);
                }
                if edge > 0.0 && pos.x <= w.left() + edge {
                    return Some(Hit::NavLeft);
                }
                return Some(Hit::NavWindow);
            }
            if pos.x > w.right() && pos.x <= w.right() + EDGE_SLOP {
                return Some(Hit::NavRight);
            }
            if pos.x < w.left() && pos.x >= w.left() - EDGE_SLOP {
                return Some(Hit::NavLeft);
            }
        }
        return Some(Hit::NavField(nav_time_at(&l, view, pos.x)));
    }
    if l.strip.contains(pos) {
        return None;
    }
    // **The loop bar's TAB, before the ruler's locate.** Ten points of the
    // ruler stop locating, which is the price of the one part of this handle
    // that is guaranteed to be reachable: the roll below can be papered over
    // with notes, and notes win there (see the roll pass), so a part dense at
    // the turn would leave the bar drawn and ungrabbable if the tab did not
    // answer here.
    if l.ruler.is_positive() && l.ruler.contains(pos) {
        if loop_bar_x(&l, view).is_some_and(|x| (pos.x - x).abs() <= LOOP_BAR_GRAB) {
            return Some(Hit::LoopBar);
        }
        return Some(Hit::Ruler(time_at(&l, view, pos.x)));
    }
    if l.vel.is_positive() && l.vel.contains(pos) && view.pedal_lane {
        // **The pedal lane answers over its whole face, pen up or down.**
        //
        // The velocity lane below cannot, and the reason it cannot does not
        // apply here: there, a press has to find ONE NOTE among the bars, so
        // a lane that swallowed every press would be a lane you cannot click
        // past. The pedal is not a set of objects to aim at — it is one
        // continuous line across the whole clip, and there is nothing behind
        // it to click past to.
        //
        // The HEIGHT of the press is the value, which is what lets one
        // gesture do both jobs: along the top it paints the damper down,
        // along the bottom it paints it up, because zero is a pedal nobody
        // is pressing. There is no separate erase tool to go and find.
        return Some(Hit::Pedal(
            time_at(&l, view, pos.x),
            pedal_value_at(&l, pos.y),
        ));
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
    // **The loop bar, on the roll — and AFTER the notes.** A loop's last note
    // very often ends on the turn, so a bar that claimed its own column would
    // put a ten-point dead band around the edge of the note anybody is most
    // likely to be adjusting. Editing notes is what this panel is for; the
    // bar gives way on any row a note occupies and answers on every row that
    // is free, and its tab up in the ruler is the target that never gives way
    // at all.
    //
    // The price, said out loud: with the pen down, the ten points at the turn
    // stop writing notes. That is the one column where a note written by
    // accident would land on the boundary itself — the first beat of the next
    // pass, played by the pass before it — so of the two things that could
    // live there, the handle is the one worth keeping.
    if loop_bar_x(&l, view).is_some_and(|x| (pos.x - x).abs() <= LOOP_BAR_GRAB) {
        return Some(Hit::LoopBar);
    }
    let key = key_at(&l, view, pos.y).ceil();
    if !(0.0..=127.0).contains(&key) {
        return None;
    }
    Some(Hit::Cell(time_at(&l, view, pos.x), key as u8))
}

/// The same hit, with the resize AFFORDANCES resolved away — what a CLICK is
/// on, as opposed to what a drag would grab.
///
/// **An edge zone is a drag thing.** It exists so a hand can catch a border and
/// pull it, and [`hit_test`] spreads it wide on purpose: `EDGE_W` inside the
/// note and `EDGE_SLOP` outside it, an eleven-point band straddling every
/// border. A double-click is not a drag, and letting that band answer for one
/// left eleven points around every note edge where a double-click meant
/// nothing at all — no note written, no note removed, no feedback.
///
/// That band lands in the worst possible place. With the pen up a click on
/// empty roll LOCATES, snapped; the double-click that follows writes a note
/// from the same snapped grid line — so the note's left border, and the
/// playhead, end up on exactly the same pixel. Enter one note and the next
/// eleven points around the playhead are dead, four of them in the empty cell
/// on the far side. The owner's "unable to dbl click input if its near the
/// playhead (on either side of it)": either side is literal, and the playhead
/// is the marker rather than the cause.
///
/// So a click resolves the zone by where the hand really is. Inside the note it
/// is the note — a double-click removes it, the same as its middle. Outside it,
/// in the slop the allowance reached across, it is the empty roll it looks
/// like, and a double-click writes a note there.
pub fn click_hit(rect: Rect, view: &MidiEditView<'_>, pos: Pos2) -> Option<Hit> {
    let hit = hit_test(rect, view, pos);
    let (Hit::NoteLeft(i) | Hit::NoteRight(i)) = hit? else {
        return hit;
    };
    let l = Layout::new(rect);
    let inside = view.notes.get(i).is_some_and(|n| {
        let r = note_rect(&l, view, n);
        r.is_positive() && r.contains(pos)
    });
    if inside {
        return Some(Hit::NoteBody(i));
    }
    let key = key_at(&l, view, pos.y).ceil();
    (0.0..=127.0)
        .contains(&key)
        .then(|| Hit::Cell(time_at(&l, view, pos.x), key as u8))
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

    /// **The PED chip swaps the lane, and the swapped lane answers with the
    /// hand's HEIGHT as a value.**
    ///
    /// Two claims, because they are one feature. The chip has to be pressable
    /// or the lane is unreachable; and the lane has to answer over its whole
    /// face — which the velocity lane deliberately does not — because a pedal
    /// is one continuous line across the clip rather than a row of objects to
    /// aim at, and there is nothing behind it to click past to.
    #[test]
    fn the_ped_chip_swaps_the_lane_and_the_lane_reads_the_hands_height() {
        let notes = [ArrNote { at_s: 0.5, dur_s: 0.25, key: 60, vel: 100, muted: false }];
        let r = rect();
        let mut v = view(&notes);
        let l = Layout::new(r);

        assert!(l.tool_pedal.is_positive(), "no PED chip on the strip");
        assert_eq!(
            hit_test(r, &v, l.tool_pedal.center()),
            Some(Hit::ToolPedal),
            "the PED chip cannot be pressed"
        );
        // It does not sit on top of the pencil, which is the other mode
        // switch — two chips at one point is one chip nobody can reach.
        assert!(
            l.tool_pedal.right() <= l.tool_draw.left(),
            "PED overlaps the pencil"
        );

        // A point well away from any note's velocity handle, with the pen UP
        // — the case the velocity lane refuses.
        let away = Pos2::new(l.vel.center().x + 60.0, l.vel.center().y);
        assert!(l.vel.contains(away));
        v.draw_mode = false;
        v.pedal_lane = false;
        assert!(
            !matches!(hit_test(r, &v, away), Some(Hit::Vel(_))),
            "the velocity lane answered a press with no note under it"
        );
        v.pedal_lane = true;
        assert!(
            matches!(hit_test(r, &v, away), Some(Hit::Pedal(..))),
            "the pedal lane did not answer over its own face"
        );

        // **The height IS the value**, and the floor is zero — which is what
        // makes one gesture both the draw and the erase.
        let top = Pos2::new(away.x, l.vel.top() + 0.5);
        let floor = Pos2::new(away.x, l.vel.bottom() - 0.5);
        let Some(Hit::Pedal(_, hi)) = hit_test(r, &v, top) else {
            panic!("no hit at the lane's top")
        };
        let Some(Hit::Pedal(_, lo)) = hit_test(r, &v, floor) else {
            panic!("no hit at the lane's floor")
        };
        assert!(hi >= 126, "the top of the lane is {hi}, not the top of the range");
        assert_eq!(lo, 0, "the floor of the lane is not pedal-up");
        // And the painter and the hit test agree, which they must or a shelf
        // is drawn at a height the hand cannot put it back at.
        assert_eq!(pedal_value_at(&l, top.y), hi);
        assert_eq!(pedal_value_at(&l, floor.y), lo);
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
        let notes = [ArrNote { at_s: 0.5, dur_s: 0.25, key: 60, vel: 100, muted: false }];
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
                    Color32::from_rgb(c.r, c.g, c.b)
                };
                let face = roll_face(
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
                    let a = velocity_hue(loud, quiet, face, v);
                    let b = velocity_hue(loud, quiet, face, v);
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
        // **Three callers, and the count is a proxy for the real claim.**
        // What must stay true is that ONE function decides what a value looks
        // like; callers of it are the point, not the danger. The three are
        // the note fill, the velocity shaft, and the PEDAL lane's shelves —
        // which reuse this gradient deliberately, so that a half-pedal is a
        // different colour from a full one and so that the contrast push
        // lives in one place. A FOURTH is worth a look; a second gradient
        // function is the regression, and the assertion below is the one
        // that catches it.
        assert_eq!(
            fills, 3,
            "the note fill, the velocity shaft and the pedal lane should be \
             the only callers, found {fills}"
        );
        let retired = concat!("fn velocity", "_ink");
        assert!(
            !src.contains(retired),
            "the second gradient is back; there must be only one"
        );
    }

    use super::*;

    /// **The magnet and a full-strength quantize land on the same frame.**
    ///
    /// Not a coincidence to be checked but a wiring to be held: `snap_t`
    /// delegates to `Grid::nearest`, and this is what notices if somebody
    /// gives the editor a local implementation again. The app has already
    /// paid once for a magnet that pointed somewhere the drawn lines did not
    /// - the box said "1/2" and it snapped eighths - and a quantize that
    /// disagreed with the magnet would be the same bug wearing a menu item.
    ///
    /// Swept across straight, triplet and swung grids: two implementations
    /// agreeing on one number is luck, and agreeing across a sweep is a
    /// contract.
    #[test]
    fn a_full_strength_quantize_lands_where_the_magnet_would() {
        let notes: Vec<ArrNote> = Vec::new();
        for (exp, triplet, swing) in [
            (-2_i8, false, false),
            (-3, false, false),
            (-3, true, false),
            (-3, false, true),
            (-5, false, true),
        ] {
            let v = MidiEditView {
                snap_on: true,
                grid_exp: exp,
                triplet,
                swing,
                swing_pct: 0.62,
                bpm: 120.0,
                ..view(&notes)
            };
            // **Built from the VIEW'S OWN FIELDS, not from `view_grid`.**
            //
            // Written first as `view_grid(&v)`, which made the whole test a
            // tautology: both sides then read one function, so deleting
            // swing from it broke them identically and the test still
            // passed. Verified by doing exactly that. Deriving the expected
            // grid independently here is what gives the comparison teeth -
            // if `view_grid` stops carrying swing, or triplet, or the
            // meter, the magnet moves and this does not.
            let grid = crate::quantize::Grid::from_document(
                v.grid_exp,
                v.sig_top as u8,
                v.sig_bottom as u8,
                v.bpm,
                v.triplet,
                (v.swing && !v.triplet).then_some(v.swing_pct),
            )
            .expect("every rung describes a grid");
            let q = crate::quantize::Quantize::full(grid);
            for i in 0..400 {
                let t = f64::from(i) * 0.011;
                let magnet = snap_t(&v, t);
                let quantized = q.place(t).max(0.0);
                assert!(
                    (magnet - quantized).abs() < 1e-9,
                    "exp {exp} triplet {triplet} swing {swing} at {t}: \
                     the magnet says {magnet}, quantize says {quantized}"
                );
            }
        }
    }

    pub(super) fn view<'a>(notes: &'a [ArrNote]) -> MidiEditView<'a> {
        MidiEditView {
            lit: [false; 128],
            draw_mode: true,
            pedal: &[],
            pedal_lane: false,
            fx_strip: (0xE3, 0xD9, 0xBD),
            meter_bg: (0x0B, 0x0E, 0x0A),
            vel_loud: (0xff, 0x8d, 0x89),
            vel_quiet: (0x40, 0xbd, 0x51),
            name: "MIDI",
            clip_start_s: 8.0,
            clip_len_s: 16.0,
            // **No loop by default, deliberately.** The bar is a control that
            // stands over the roll, and giving every fixture one would put it
            // in the way of tests that are about notes — the loop tests below
            // set their own. It is also a true state: an audio clip, a glued
            // one and every document written before looping existed all say
            // zero here.
            loop_s: 0.0,
            offset_s: 0.0,
            notes,
            selected: &[],
            marquee: None,
            view_start_s: 6.0,
            view_len_s: 20.0,
            // The window as it opens: the whole clip and its pad, so the
            // navigator's window fills its box here. Tests that want a
            // window with somewhere to go narrow `view_len_s` themselves.
            nav_start_s: 6.0,
            nav_len_s: 20.0,
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
        muted: false,
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

    /// **A resize zone must not decide what a CLICK is on.**
    ///
    /// `hit_test` spreads a note's edges wide so a hand can catch them:
    /// `EDGE_W` inside the note, `EDGE_SLOP` outside it, an eleven-point band
    /// straddling every border. That is right for a drag and wrong for
    /// everything else — the roll's double-click block answers for `NoteBody`,
    /// `Vel` and `Cell` and has no case for an edge, so the whole band was
    /// eleven points where a double-click did nothing at all.
    ///
    /// Which would be a curiosity if the band landed anywhere. It lands on the
    /// PLAYHEAD: a pen-up click locates, snapped, and the double-click that
    /// follows writes a note from the same snapped grid line, so the new note's
    /// left border and the playhead are the same pixel. The owner: "unable to
    /// dbl click input if its near the playhead (on either side of it)."
    ///
    /// Revert check performed: return `hit_test`'s answer unchanged from
    /// `click_hit` and both halves go red.
    #[test]
    fn a_click_reads_past_a_notes_resize_zone() {
        let notes = [ArrNote { at_s: 2.0, dur_s: 1.0, key: 60, vel: 100, muted: false }];
        let v = view(&notes);
        let r = rect();
        let l = Layout::new(r);
        let nr = note_rect(&l, &v, &notes[0]);
        assert!(nr.width() >= 14.0, "the fixture's note is too narrow to have edges");
        let y = nr.center().y;

        // Just INSIDE the left border: a drag would resize the head, and a
        // click is on the note, which is what a double-click removes.
        let inside = Pos2::new(nr.left() + 2.0, y);
        assert_eq!(hit_test(r, &v, inside), Some(Hit::NoteLeft(0)));
        assert_eq!(
            click_hit(r, &v, inside),
            Some(Hit::NoteBody(0)),
            "a click on the note's own face answered as a resize handle"
        );

        // Just OUTSIDE the right border, in the allowance: a drag reaches back
        // for the border, and a click is on the empty cell it looks like —
        // which is the next note in a run, and the one that would not go in.
        let outside = Pos2::new(nr.right() + EDGE_SLOP - 1.0, y);
        assert_eq!(hit_test(r, &v, outside), Some(Hit::NoteRight(0)));
        match click_hit(r, &v, outside) {
            Some(Hit::Cell(t, key)) => {
                assert_eq!(key, 60, "the cell beside the note is on its own row");
                assert!(
                    t > v.clip_start_s + notes[0].at_s,
                    "the cell beside the note read as {t}, at or before the note itself"
                );
            }
            other => panic!("the empty cell beside a note answered {other:? }"),
        }

        // And everything that is not an edge is left exactly as it was.
        for p in [nr.center(), Pos2::new(l.roll.center().x, l.roll.bottom() - 4.0)] {
            assert_eq!(click_hit(r, &v, p), hit_test(r, &v, p));
        }
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
        muted: false,
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
            hit_test(rect(), &v, Pos2::new(r.right() + EDGE_SLOP + 3.0, y)),
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
        muted: false,
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

    /// **The navigator is laid out, and every part of it answers a press.**
    ///
    /// The strip's box is drawn from `tool_nav` and the window inside it
    /// from `nav_window`, and this is the pairing this codebase keeps
    /// catching: a cell that is laid out and painted and answers nothing
    /// when pressed. So each part is asked in turn — the window's face, its
    /// two edges from inside and from just outside (a note's own allowance,
    /// and for the same reason), the inset above the window that is still
    /// the window, and the clip either side of it — and the axis is asked to
    /// round-trip, because the app's drag reads `nav_time_at` for a point the
    /// painter put down with `nav_x_of`.
    ///
    /// Revert check performed: take the navigator branch out of `hit_test`
    /// and the first assertion on the window goes red with `None`.
    #[test]
    fn the_navigator_is_laid_out_and_every_part_of_it_answers_a_press() {
        let notes = [ArrNote { at_s: 2.0, dur_s: 1.0, key: 60, vel: 96, muted: false }];
        let mut v = view(&notes);
        // A window with somewhere to go: four seconds of the twenty.
        v.view_start_s = 8.0;
        v.view_len_s = 4.0;
        let r = rect();
        let l = Layout::new(r);
        assert!(l.tool_nav.is_positive(), "a 900-point editor has no navigator");
        assert!(
            l.tool_nav.right() < l.tool_draw.left(),
            "the navigator runs under the pencil"
        );
        assert!(
            l.tool_nav.left() >= l.strip.left() + NAME_X + NAV_NAME_RESERVE,
            "the navigator took the name's room"
        );
        for t in [6.0, 8.0, 17.5, 26.0] {
            let x = nav_x_of(&l, &v, t);
            assert!((nav_time_at(&l, &v, x) - t).abs() < 1.0e-4, "{t} did not round-trip");
        }
        let w = nav_window(&l, &v);
        assert!(w.is_positive(), "the window drew nowhere");
        assert!((w.left() - nav_x_of(&l, &v, 8.0)).abs() < 0.01);
        assert!((w.right() - nav_x_of(&l, &v, 12.0)).abs() < 0.01);
        assert!(w.width() >= 14.0, "the fixture's window is too narrow to have edges");
        let y = l.tool_nav.center().y;
        assert_eq!(hit_test(r, &v, Pos2::new(w.center().x, y)), Some(Hit::NavWindow));
        assert_eq!(hit_test(r, &v, Pos2::new(w.left() + 2.0, y)), Some(Hit::NavLeft));
        assert_eq!(hit_test(r, &v, Pos2::new(w.right() - 2.0, y)), Some(Hit::NavRight));
        assert_eq!(
            hit_test(r, &v, Pos2::new(w.right() + 2.0, y)),
            Some(Hit::NavRight),
            "just past the window's end is still its end"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(w.left() - 2.0, y)),
            Some(Hit::NavLeft),
            "just before the window's start is still its start"
        );
        // The inset between the box's top and the window's is the window:
        // X alone decides, the band's rule for its viewport.
        assert_eq!(
            hit_test(r, &v, Pos2::new(w.center().x, l.tool_nav.top() + 0.5)),
            Some(Hit::NavWindow)
        );
        // Past the allowance, the clip itself, at the second under the hand.
        match hit_test(r, &v, Pos2::new(nav_x_of(&l, &v, 20.0), y)) {
            Some(Hit::NavField(t)) => assert!((t - 20.0).abs() < 0.05, "the clip read as {t}"),
            other => panic!("the clip on the navigator answered {other:?}"),
        }
        // And the strip beside the box is nobody's, as it always was.
        assert_eq!(hit_test(r, &v, Pos2::new(l.tool_nav.left() - 2.0, y)), None);
    }

    /// **When the whole clip fits the roll, the window fills the box and its
    /// edges still answer.**
    ///
    /// The question was whether to draw a navigator at all for a view that
    /// cannot move. It is drawn, because it can still ZOOM — the edges are
    /// the way in — and a navigator that only appeared once the wheel had
    /// been used would be no way to zoom without the wheel. What that
    /// decision needs is for the full window's edges to be reachable at the
    /// box's own edges, which is what is pinned here.
    #[test]
    fn the_navigators_window_fills_its_box_when_the_clip_fits_and_its_edges_still_answer() {
        let notes = [ArrNote { at_s: 2.0, dur_s: 1.0, key: 60, vel: 96, muted: false }];
        // The fixture as it stands: the window IS the world.
        let v = view(&notes);
        let r = rect();
        let l = Layout::new(r);
        let b = l.tool_nav;
        let w = nav_window(&l, &v);
        assert!((w.left() - b.left()).abs() < 0.01, "the full window starts at {}", w.left());
        assert!((w.right() - b.right()).abs() < 0.01, "the full window ends at {}", w.right());
        let y = b.center().y;
        assert_eq!(hit_test(r, &v, Pos2::new(b.left() + 2.0, y)), Some(Hit::NavLeft));
        assert_eq!(hit_test(r, &v, Pos2::new(b.right() - 2.0, y)), Some(Hit::NavRight));
        assert_eq!(hit_test(r, &v, Pos2::new(b.center().x, y)), Some(Hit::NavWindow));
    }

    /// **A narrow window keeps its whole face and finds its edges outside**
    /// — a note's rule, for a note's reason. Two seconds of a ten-minute
    /// clip is a fraction of a point on the box, floored to three; a
    /// window that thin with edge zones inside it would be all edge and
    /// could never be slid.
    ///
    /// Revert check performed: drop the floor in `nav_window` and the width
    /// assertion goes red at a fraction of a point.
    #[test]
    fn a_narrow_navigator_window_keeps_its_face_and_finds_its_edges_outside() {
        let notes = [ArrNote { at_s: 2.0, dur_s: 1.0, key: 60, vel: 96, muted: false }];
        let mut v = view(&notes);
        v.view_start_s = 10.0;
        v.view_len_s = 0.02;
        let r = rect();
        let l = Layout::new(r);
        let w = nav_window(&l, &v);
        assert!(
            w.width() >= NAV_WINDOW_MIN_W - 0.01 && w.width() < 14.0,
            "the window is { } points wide",
            w.width()
        );
        let y = l.tool_nav.center().y;
        assert_eq!(
            hit_test(r, &v, Pos2::new(w.center().x, y)),
            Some(Hit::NavWindow),
            "a narrow window lost its body"
        );
        assert_eq!(hit_test(r, &v, Pos2::new(w.left() - 2.0, y)), Some(Hit::NavLeft));
        assert_eq!(hit_test(r, &v, Pos2::new(w.right() + 2.0, y)), Some(Hit::NavRight));
    }

    /// **The navigator gives way to the name, and is not laid out at all on
    /// a strip too narrow to hold one worth having.** On a wide strip it
    /// takes its share and stops at its ceiling.
    ///
    /// Revert check performed: lay the box out whatever its width and the
    /// narrow editor's assertion goes red with a box a few points wide.
    #[test]
    fn the_navigator_stands_down_when_the_strip_has_no_room_for_it() {
        let notes = [ArrNote { at_s: 2.0, dur_s: 1.0, key: 60, vel: 96, muted: false }];
        let v = view(&notes);
        let narrow = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(380.0, 300.0));
        let l = Layout::new(narrow);
        assert!(!l.tool_nav.is_positive(), "a 380-point strip grew a navigator");
        // And the strip's middle answers nothing there, as before.
        let mid = Pos2::new(l.strip.left() + NAME_X + NAV_NAME_RESERVE + 10.0, l.strip.center().y);
        assert!(l.strip.contains(mid) && !l.tool_draw.contains(mid));
        assert_eq!(hit_test(narrow, &v, mid), None);
        let wide = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(2000.0, 600.0));
        let l = Layout::new(wide);
        assert!(
            (l.tool_nav.width() - NAV_MAX_W).abs() < 0.01,
            "on a 2000-point strip the navigator is { } wide",
            l.tool_nav.width()
        );
    }

    /// **The miniature draws every note inside the box, higher pitches
    /// higher.** Read off the painted shapes rather than trusted: a mark
    /// drawn against the whole keyboard instead of the part's own span
    /// would be a flat line, and one drawn with the axis upside down would
    /// be a mirror — neither is a shape you can read the tune from.
    ///
    /// Two assertions, because one was not enough. Against a 128-key span
    /// the two marks still come out in the right order — three points
    /// apart, huddled in the middle of the box — so pitch rising alone
    /// would have passed the flat line it was written to reject. The part
    /// has to FILL the box: a two-octave part's marks stand most of the
    /// box's height apart.
    ///
    /// Revert checks performed: draw the marks against a 128-key span and
    /// the span assertion goes red at three points; measure `y` from the
    /// box's top instead of its floor and the ordering assertion goes red.
    #[test]
    fn the_miniature_draws_the_notes_inside_the_box_with_pitch_rising() {
        let notes = [
            ArrNote { at_s: 2.0, dur_s: 1.0, key: 48, vel: 96, muted: false },
            ArrNote { at_s: 10.0, dur_s: 1.0, key: 72, vel: 96, muted: false },
        ];
        let v = view(&notes);
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("nav-mini"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        let r = rect();
        let l = Layout::new(r);
        draw(&painter, r, &v);
        // The marks: filled rects inside the box no taller than half of it,
        // which leaves out the box, the clip's span, the window and its
        // handles, all of which run the box's height.
        let mut marks: Vec<Rect> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Rect(q)
                                if l.tool_nav.contains_rect(q.rect)
                                    && q.rect.height() <= l.tool_nav.height() * 0.5 =>
                            {
                                Some(q.rect)
                            }
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        marks.sort_by(|a, b| a.left().total_cmp(&b.left()));
        assert_eq!(marks.len(), 2, "two notes drew { } marks: {marks:?}", marks.len());
        let (low, high) = (marks[0], marks[1]);
        assert!((low.left() - nav_x_of(&l, &v, 10.0)).abs() < 1.5, "C3 is at { }", low.left());
        assert!((high.left() - nav_x_of(&l, &v, 18.0)).abs() < 1.5, "C5 is at {}", high.left());
        assert!(
            high.bottom() < low.top(),
            "C5 drew at y {} and C3 at y {}: the higher note is not higher",
            high.center().y,
            low.center().y
        );
        let apart = low.bottom() - high.top();
        assert!(
            apart >= l.tool_nav.height() * 0.75,
            "two octaves stand {apart} points apart in a box {} tall: the part is not \
             drawn against its own range",
            l.tool_nav.height()
        );
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

    /// The grid word says the shape, not just the size — and the quarter
    /// rung of THIS chip is "1/4", the same numerals the arrange toolbar
    /// uses.
    ///
    /// **It was the note glyph, pinned here in both directions** so that a
    /// blunt edit to the shared label could not move the editor's chip in
    /// the arrangement's stroke. That guard did its job: the owner came here
    /// to say so — "Replace the note glyph with 1/4 in midi editor" — and the
    /// assert was changed on purpose rather than the two surfaces drifting
    /// apart unnoticed. The pin stays, pointing the other way now.
    /// **A sounding key is visible, in the piano column AND across the
    /// roll.**
    ///
    /// The editor auditions on three roads — a press on the piano column, an
    /// arrow that moves the pitch cursor, a note written or grabbed — and not
    /// one of them drew anything: the sound came out and the picture said
    /// nothing. The owner: "we need proper visual feedback for arrow and
    /// click keyboard auditioning in the midi editor. Right now both of those
    /// actions provide no visual feedback."
    ///
    /// Both halves are asserted because they answer different needs. The
    /// piano column is the literal answer to "which key"; the roll's lane is
    /// what makes it visible to an eye that is on the notes rather than on a
    /// strip eighty points away at the far left.
    ///
    /// Revert check performed: drop either `rect_filled` and the matching
    /// half fails while the other still passes.
    #[test]
    fn a_sounding_key_is_lit_on_the_keys_and_across_the_roll() {
        let n: [ArrNote; 0] = [];
        let paint = |lit: [bool; 128]| -> Vec<(Rect, Color32)> {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
            let _ = ctx.run(Default::default(), |_| {});
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("lit"));
            let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
            let v = MidiEditView { lit, ..view(&n) };
            draw(&painter, rect(), &v);
            ctx.graphics(|g| {
                g.get(layer)
                    .map(|list| {
                        list.all_entries()
                            .filter_map(|c| match &c.shape {
                                egui::Shape::Rect(r) => Some((r.rect, r.fill)),
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            })
        };
        // **Nothing sounding: no amber anywhere.** The floor this stands on —
        // without it a painter that lit every key would pass the half below.
        let quiet = paint([false; 128]);
        assert!(
            !quiet.iter().any(|(_, c)| *c == LIT_KEY),
            "a silent editor lit a key"
        );

        let mut lit = [false; 128];
        lit[60] = true;
        let loud = paint(lit);
        let l = Layout::new(rect());
        let solid: Vec<Rect> = loud
            .iter()
            .filter(|(_, c)| *c == LIT_KEY)
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(solid.len(), 1, "the piano column lit {} keys", solid.len());
        assert!(
            l.keys.contains(solid[0].center()),
            "the lit key was painted outside the piano column"
        );
        // And the roll carries the same colour, washed, all the way across.
        let wash: Vec<Rect> = loud
            .iter()
            .filter(|(r, c)| {
                *c == LIT_KEY.gamma_multiply(0.22) && l.roll.contains(r.center())
            })
            .map(|(r, _)| *r)
            .collect();
        assert_eq!(wash.len(), 1, "the roll lane did not light");
        assert!(
            wash[0].width() > l.roll.width() * 0.9,
            "the roll's lane stopped short: {} of {}",
            wash[0].width(),
            l.roll.width()
        );
        // The two are the same ROW, or they are two marks about one key that
        // point at different notes.
        assert!(
            (wash[0].center().y - solid[0].center().y).abs() < 1.0,
            "the column and the roll lit different rows"
        );
    }

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
        // exp -2 in 4/4 is the quarter note: the rung that split, and the
        // rung that has now been put back together.
        v.swing = false;
        v.grid_exp = -2;
        assert_eq!(grid_word(&v), "1/4");
        v.triplet = true;
        assert_eq!(grid_word(&v), "1/4T");
        v.triplet = false;
        v.swing = true;
        assert_eq!(grid_word(&v), "1/4 sw");
        // **The two surfaces agree again**, asserted side by side for the
        // reason they were asserted side by side while they DISAGREED: so
        // that the state of the pair is a decision somebody wrote down, not a
        // difference the next reader has to go and discover.
        assert_eq!(crate::arrangement_panel::grid_label_text(-2, 4, 4), "1/4");
        // And the glyph spelling still exists, unused by either chip today.
        // It is the record of the first decision; see `grid_word`.
        assert_eq!(crate::arrangement_panel::grid_label(-2, 4, 4), "\u{266a}");
    }

    /// **The loop bar is drawn where the pass ends, and it answers a press
    /// there** — the pair every painted control in this app owes.
    ///
    /// The owner asked for "a bar on the right end of clip/loop" that can be
    /// dragged either way. This asserts the half that lives in the panel: the
    /// bar's one authority (`loop_bar_x`) places it, the painter puts ink at
    /// that column in both the roll and the ruler, and both of those columns
    /// hit-test as the bar.
    ///
    /// The ruler's tab is checked separately from the roll's shaft on
    /// purpose. It is the reachable half — the roll gives way to notes, so a
    /// part dense at the turn would leave the bar drawn and ungrabbable if
    /// the tab were ever to stop answering.
    ///
    /// Reverts performed, both red: return `None` from `loop_bar_x` and every
    /// assert here goes; drop the ruler branch from `hit_test` and the tab
    /// answers `Ruler` instead.
    #[test]
    fn the_loop_bar_is_drawn_at_the_pass_end_and_can_be_pressed_there() {
        // A four-second loop inside a sixteen-second clip: the bar stands
        // well inside the roll, so nothing here is testing an edge case of
        // the clamp instead.
        let notes: [ArrNote; 0] = [];
        let v = MidiEditView { loop_s: 4.0, ..view(&notes) };
        let r = rect();
        let l = Layout::new(r);
        let x = loop_bar_x(&l, &v).expect("a looping clip drew no bar");
        assert!(
            (x - x_of(&l, &v, v.clip_start_s + v.loop_s)).abs() < 0.01,
            "the bar is at {x}, not at the end of the pass"
        );

        // It answers on the ruler's tab, and on the empty roll under it.
        assert_eq!(
            hit_test(r, &v, Pos2::new(x, l.ruler.center().y)),
            Some(Hit::LoopBar),
            "the tab on the ruler is not the bar"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(x, l.roll.center().y)),
            Some(Hit::LoopBar),
            "the bar over empty roll is not the bar"
        );
        // Either side of it, out to the grab, and no further: a point past
        // the grab is the ruler's locate again.
        for dx in [-LOOP_BAR_GRAB + 0.5, LOOP_BAR_GRAB - 0.5] {
            assert_eq!(
                hit_test(r, &v, Pos2::new(x + dx, l.ruler.center().y)),
                Some(Hit::LoopBar),
                "the bar does not reach {dx} points away"
            );
        }
        assert!(
            matches!(
                hit_test(r, &v, Pos2::new(x + LOOP_BAR_GRAB + 3.0, l.ruler.center().y)),
                Some(Hit::Ruler(_))
            ),
            "the bar swallowed the ruler past its grab"
        );

        // A clip that does not loop has no bar and gives the ruler back.
        let flat = MidiEditView { loop_s: 0.0, ..view(&notes) };
        assert!(loop_bar_x(&l, &flat).is_none(), "a clip with no loop drew a bar");
        assert!(
            matches!(hit_test(r, &flat, Pos2::new(x, l.ruler.center().y)), Some(Hit::Ruler(_))),
            "a clip with no loop still claims the ruler"
        );

        // ── and it was PAINTED, in both bands ──
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("loop-bar"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        draw(&painter, r, &v);
        let rects: Vec<Rect> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Rect(q) => Some(q.rect),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        });
        let straddles = |q: &Rect, band: Rect| {
            q.left() <= x + LOOP_BAR_GRAB
                && q.right() >= x - LOOP_BAR_GRAB
                && q.width() <= LOOP_BAR_GRAB * 2.5
                && band.contains(q.center())
        };
        assert!(
            rects.iter().any(|q| straddles(q, l.roll) && q.height() > l.roll.height() * 0.9),
            "no shaft was painted down the roll at {x}"
        );
        assert!(
            rects.iter().any(|q| straddles(q, l.ruler)),
            "no tab was painted on the ruler at {x}"
        );
    }

    /// **A note at the turn keeps its edges; the bar gives way.**
    ///
    /// A loop's last note very often ENDS on the loop's own end, so the bar
    /// and that note's right edge land on the same column. Editing notes is
    /// what this panel is for, so the note wins there — and the ruler's tab
    /// is why that costs nothing.
    ///
    /// Revert performed: move the bar's roll branch above the note passes in
    /// `hit_test` and the first assert reads `LoopBar` where the note's edge
    /// belongs.
    #[test]
    fn a_note_ending_on_the_loop_keeps_its_edge() {
        // A note ending exactly at the pass's end: clip starts at 8, the loop
        // is 4, so a note from source second 2 running two seconds ends at
        // absolute 12 — the bar's own column.
        let notes = [ArrNote { at_s: 2.0, dur_s: 2.0, key: 72, vel: 96, muted: false }];
        let v = MidiEditView { loop_s: 4.0, ..view(&notes) };
        let r = rect();
        let l = Layout::new(r);
        let x = loop_bar_x(&l, &v).expect("a looping clip drew no bar");
        let row = note_rect(&l, &v, &notes[0]);
        assert!(row.is_positive(), "the fixture note is not on screen");
        assert!(
            (row.right() - x).abs() < 1.5,
            "the fixture note ends at {} and the bar is at {x}",
            row.right()
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(x - 1.0, row.center().y)),
            Some(Hit::NoteRight(0)),
            "the loop bar ate the edge of the note that ends on it"
        );
        // One row away there is no note, and the bar answers again.
        let free = Pos2::new(x, row.center().y + row.height() * 2.0);
        assert!(l.roll.contains(free), "the fixture has no free row to test");
        assert_eq!(
            hit_test(r, &v, free),
            Some(Hit::LoopBar),
            "the bar is unreachable on a row with no note on it"
        );
        // And the tab, which never gives way at all.
        assert_eq!(hit_test(r, &v, Pos2::new(x, l.ruler.center().y)), Some(Hit::LoopBar));
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
            muted: false,
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
            muted: false,
            })
            .collect();
        let kit: Vec<ArrNote> = (0..15_360u32)
            .map(|i| ArrNote {
                at_s: f64::from(i / 4) * 0.125,
                dur_s: 0.1,
                key: 48 + u8::try_from((i * 5) % 8).unwrap_or(0),
                vel: if i % 8 == 0 { 118 } else { 64 + u8::try_from(i % 3).unwrap_or(0) * 12 },
            muted: false,
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
