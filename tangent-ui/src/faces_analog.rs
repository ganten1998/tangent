//! The analog's face: a panel after the 1970 machine, drawn from the kit.
//!
//! **What this panel is.** Five sections across the top in the order the
//! signal flows — controllers, the oscillator bank, the mixer, the
//! modifiers, the output — each a small grid of dials under an engraved
//! caption; a lower band of three rows for what the original had no panel
//! for (voice mode, the filter switch and the LFO, the arpeggiator); the
//! app's own mark at the right of the header opposite the model name; and
//! the preset strip `faces::draw` appends under every face. Sixty-one
//! controls, every one an `id` the engine's table publishes, placed by the
//! table at the top of this file rather than by a walk over the pushed
//! parameters, because the layout IS the design and a knob that moved
//! because a parameter was inserted would be a different panel.
//!
//! **The dial is the house's, in the Moog dress.** `recorder_panel::
//! draw_knob` is the reference: the name above the control, the reading
//! below it, the sweep centred on UP so the tick skirt arcs over the top
//! where a hand on the knob is not covering it, the ticks outside the body
//! because they are a mark on the panel the knob is mounted through. The
//! derivation is followed, not the numbers. What is dressed is the black
//! moulded body, the inset cap, and a full screwdriver-slot pointer
//! straight through the centre — a short pointer at one edge is a dot at
//! this size. What is THEMED is the panel ground, the engraved legends and
//! the cap colour, one accent per section derived from the theme's own
//! accent and walked dark enough that the house's light slot reads on
//! every one of them; the slot itself is never themed, so it reads on
//! every cap. Ink is a property of the surface it lands on and nothing
//! here inherits one from a role.
//!
//! **A stepped dial is a dial**, because a range switch on the original
//! was a rotary switch and reads as one: it turns in detents, a click
//! walks it, and its reading is the position's word. Toggles are small
//! rockers; the few real multi-way selectors (voice mode, the filter, the
//! arpeggiator's mode and the note priority) are segmented switches with
//! their words engraved, since a five-way rotary at this size would be a
//! dial nobody could read.
//!
//! **The Mode dial is dead under the ladder**, and drawn dead: dimmed to
//! the caption tier, ticks unpainted, no slot, drag ignored. It is the one
//! place the panel shows a control that does nothing, and the reason is
//! the honest one — a real panel with a filter selector has the same dial
//! and the same dead half; what would be dishonest is leaving it live.
//!
//! **The floor is a promise.** The window opens at 900 by 560 and every
//! font this app can wear must fit the whole panel in it, preset strip
//! included; rows give up their AIR before anything else and never their
//! dials, the amp's rule. `the_analog_is_whole_at_its_own_window_on_every_
//! font` holds it, and this file's other tests hold the ink on every theme,
//! the slot on every cap, the dead dial, and a real drag through egui's
//! own input rather than a helper the painter calls.

use crate::face::{self, FaceStyle};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;
use crate::recorder_panel::{contrast_ratio, luminance};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2};

/// The sentinel, spelled here as the contract with the engine's
/// `builtin_fx::ANALOG_PATH`; `dialogs::ANALOG_PATH` is the same string
/// and a test pins the three.
pub const ANALOG_PATH: &str = "@tangent-analog";
/// The strap-line under the title bar.
pub const ANALOG_ABOUT: &str = "three oscillators, two filters, one voice or sixteen";

// ── the ids, the engine's table's ────────────────────────────────────────────
// Restated here because this crate cannot see the kernel across the
// firewall; `every_control_names_a_pushed_parameter` holds them against the
// symbols the host pushes, so a renumbering fails a test rather than moving
// a knob.

const P_TUNE: u32 = 0;
const P_GLIDE: u32 = 1;
const P_MOD_MIX: u32 = 2;
const P_MOD_DEPTH: u32 = 3;
const P_BEND_RANGE: u32 = 4;
const P_PRESSURE: u32 = 5;
const P_O1_RANGE: u32 = 10;
const P_O1_WAVE: u32 = 11;
const P_O2_RANGE: u32 = 12;
const P_O2_TUNE: u32 = 13;
const P_O2_WAVE: u32 = 14;
const P_O3_RANGE: u32 = 15;
const P_O3_TUNE: u32 = 16;
const P_O3_WAVE: u32 = 17;
const P_O3_TRACK: u32 = 18;
const P_SYNC: u32 = 19;
const P_PWM: u32 = 20;
const P_OSC_DRIFT: u32 = 21;
const P_MIX_O1: u32 = 22;
const P_MIX_O2: u32 = 23;
const P_MIX_O3: u32 = 24;
const P_MIX_NOISE: u32 = 25;
const P_NOISE_COLOUR: u32 = 26;
const P_DRIVE: u32 = 27;
const P_FILTER_MODEL: u32 = 30;
const P_CUTOFF: u32 = 31;
const P_EMPHASIS: u32 = 32;
const P_SEM_MODE: u32 = 33;
const P_CONTOUR: u32 = 34;
const P_KEYTRACK: u32 = 35;
const P_F_ATTACK: u32 = 36;
const P_F_DECAY: u32 = 37;
const P_F_SUSTAIN: u32 = 38;
const P_F_RELEASE: u32 = 39;
const P_A_ATTACK: u32 = 40;
const P_A_DECAY: u32 = 41;
const P_A_SUSTAIN: u32 = 42;
const P_A_RELEASE: u32 = 43;
const P_VEL_AMP: u32 = 44;
const P_VEL_FILTER: u32 = 45;
const P_VOICE_MODE: u32 = 50;
const P_VOICES: u32 = 51;
const P_DETUNE: u32 = 52;
const P_UNISON_SPREAD: u32 = 53;
const P_DRIFT: u32 = 54;
const P_PRIORITY: u32 = 55;
const P_TRIGGER: u32 = 56;
const P_LFO_RATE: u32 = 60;
const P_LFO_WAVE: u32 = 61;
const P_LFO_DELAY: u32 = 62;
const P_OSC_MOD: u32 = 63;
const P_FILTER_MOD: u32 = 64;
const P_ENSEMBLE: u32 = 70;
const P_ENS_WIDTH: u32 = 71;
const P_SPREAD: u32 = 72;
const P_VOLUME: u32 = 73;
const P_ARP_MODE: u32 = 80;
const P_ARP_RATE: u32 = 81;
const P_ARP_OCT: u32 = 82;
const P_ARP_GATE: u32 = 83;
const P_ARP_HOLD: u32 = 84;

/// One past the highest id: what a dense harness must number up to so
/// every dial finds a value. See `face::fit::FACES`.
pub const ID_CEILING: usize = 85;

/// The stepped ids, for the shared sweeps: what `builtin_fx`'s table says
/// has positions.
pub const STEPPED: &[u32] = &[
    P_BEND_RANGE,
    P_O1_RANGE,
    P_O1_WAVE,
    P_O2_RANGE,
    P_O2_WAVE,
    P_O3_RANGE,
    P_O3_WAVE,
    P_O3_TRACK,
    P_SYNC,
    P_NOISE_COLOUR,
    P_FILTER_MODEL,
    P_VOICE_MODE,
    P_VOICES,
    P_PRIORITY,
    P_TRIGGER,
    P_LFO_WAVE,
    P_OSC_MOD,
    P_FILTER_MOD,
    P_ENSEMBLE,
    P_ARP_MODE,
    P_ARP_RATE,
    P_ARP_OCT,
    P_ARP_HOLD,
];

// ── the controls and where they sit ─────────────────────────────────────────

/// What kind of control a parameter wears.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    /// A continuous dial.
    Knob,
    /// A dial with detents: `steps` positions, the reading its word.
    Stepped(i32),
    /// A rocker, on or off.
    Toggle,
    /// A row of engraved cells, one lit.
    Segmented(&'static [&'static str]),
}

/// Where a control sits: a cell of a section's grid in the upper panel,
/// or a span of the lower band's row in dial-column units.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Area {
    Upper { section: usize, row: usize, col: usize },
    Lower { row: usize, x: f32, w: f32 },
}

#[derive(Clone, Copy, Debug)]
struct Ctl {
    id: u32,
    /// The engraved legend — the panel's word, short, and its own; the
    /// automation lane's longer name is `BuiltinFaceParam::name`.
    label: &'static str,
    kind: Kind,
    area: Area,
}

const fn up(id: u32, label: &'static str, kind: Kind, section: usize, row: usize, col: usize) -> Ctl {
    Ctl { id, label, kind, area: Area::Upper { section, row, col } }
}

const fn low(id: u32, label: &'static str, kind: Kind, row: usize, x: f32, w: f32) -> Ctl {
    Ctl { id, label, kind, area: Area::Lower { row, x, w } }
}

/// The five upper sections: caption, and how many dial columns wide.
const SECTIONS: [(&str, usize); 5] = [
    ("CONTROLLERS", 2),
    ("OSCILLATOR BANK", 4),
    ("MIXER", 2),
    ("MODIFIERS", 4),
    ("OUTPUT", 2),
];
/// Rows in the upper grid.
const UPPER_ROWS: usize = 4;
/// Rows in the lower band.
const LOWER_ROWS: usize = 3;
/// Dial columns across the whole panel: the sections' widths summed.
const COLS: usize = 2 + 4 + 2 + 4 + 2;

const VOICE_WORDS: &[&str] = &["MONO", "UNISON", "POLY"];
const PRIO_WORDS: &[&str] = &["LOW", "LAST", "HIGH"];
const TRIG_WORDS: &[&str] = &["LEGATO", "RETRIG"];
const FILTER_WORDS: &[&str] = &["LADDER 24", "SEM 12"];
const ARP_WORDS: &[&str] = &["OFF", "UP", "DOWN", "UP-DN", "RND", "PLAY"];

/// **Every control, and where it is.** The table is the panel.
const CONTROLS: &[Ctl] = &[
    // CONTROLLERS
    up(P_TUNE, "Tune", Kind::Knob, 0, 0, 0),
    up(P_GLIDE, "Glide", Kind::Knob, 0, 0, 1),
    up(P_MOD_MIX, "Mod Mix", Kind::Knob, 0, 1, 0),
    up(P_MOD_DEPTH, "Depth", Kind::Knob, 0, 1, 1),
    up(P_BEND_RANGE, "Bend", Kind::Stepped(13), 0, 2, 0),
    up(P_PRESSURE, "Pressure", Kind::Knob, 0, 2, 1),
    // OSCILLATOR BANK
    up(P_O1_RANGE, "Range 1", Kind::Stepped(6), 1, 0, 0),
    up(P_O1_WAVE, "Wave 1", Kind::Stepped(6), 1, 0, 1),
    up(P_SYNC, "SYNC", Kind::Toggle, 1, 0, 2),
    up(P_O2_RANGE, "Range 2", Kind::Stepped(6), 1, 1, 0),
    up(P_O2_TUNE, "Tune 2", Kind::Knob, 1, 1, 1),
    up(P_O2_WAVE, "Wave 2", Kind::Stepped(6), 1, 1, 2),
    up(P_O3_RANGE, "Range 3", Kind::Stepped(6), 1, 2, 0),
    up(P_O3_TUNE, "Tune 3", Kind::Knob, 1, 2, 1),
    up(P_O3_WAVE, "Wave 3", Kind::Stepped(6), 1, 2, 2),
    up(P_O3_TRACK, "KBD", Kind::Toggle, 1, 2, 3),
    up(P_PWM, "PWM", Kind::Knob, 1, 3, 0),
    up(P_OSC_DRIFT, "Osc Drift", Kind::Knob, 1, 3, 1),
    // MIXER
    up(P_MIX_O1, "Osc 1", Kind::Knob, 2, 0, 0),
    up(P_MIX_O2, "Osc 2", Kind::Knob, 2, 0, 1),
    up(P_MIX_O3, "Osc 3", Kind::Knob, 2, 1, 0),
    up(P_MIX_NOISE, "Noise", Kind::Knob, 2, 1, 1),
    up(P_NOISE_COLOUR, "PINK", Kind::Toggle, 2, 2, 0),
    up(P_DRIVE, "Drive", Kind::Knob, 2, 2, 1),
    // MODIFIERS
    up(P_CUTOFF, "Cutoff", Kind::Knob, 3, 0, 0),
    up(P_EMPHASIS, "Emphasis", Kind::Knob, 3, 0, 1),
    up(P_SEM_MODE, "Mode", Kind::Knob, 3, 0, 2),
    up(P_CONTOUR, "Contour", Kind::Knob, 3, 0, 3),
    up(P_KEYTRACK, "Keyboard", Kind::Knob, 3, 1, 0),
    up(P_VEL_FILTER, "Vel>Flt", Kind::Knob, 3, 1, 1),
    up(P_VEL_AMP, "Velocity", Kind::Knob, 3, 1, 2),
    up(P_F_ATTACK, "F Attack", Kind::Knob, 3, 2, 0),
    up(P_F_DECAY, "F Decay", Kind::Knob, 3, 2, 1),
    up(P_F_SUSTAIN, "F Sustain", Kind::Knob, 3, 2, 2),
    up(P_F_RELEASE, "F Release", Kind::Knob, 3, 2, 3),
    up(P_A_ATTACK, "Attack", Kind::Knob, 3, 3, 0),
    up(P_A_DECAY, "Decay", Kind::Knob, 3, 3, 1),
    up(P_A_SUSTAIN, "Sustain", Kind::Knob, 3, 3, 2),
    up(P_A_RELEASE, "Release", Kind::Knob, 3, 3, 3),
    // OUTPUT
    up(P_VOLUME, "Volume", Kind::Knob, 4, 0, 0),
    up(P_SPREAD, "Spread", Kind::Knob, 4, 0, 1),
    up(P_ENSEMBLE, "Ensemble", Kind::Stepped(4), 4, 1, 0),
    up(P_ENS_WIDTH, "Width", Kind::Knob, 4, 1, 1),
    // VOICE
    low(P_VOICE_MODE, "VOICE", Kind::Segmented(VOICE_WORDS), 0, 0.0, 3.0),
    low(P_VOICES, "Voices", Kind::Stepped(16), 0, 3.0, 1.0),
    low(P_DETUNE, "Detune", Kind::Knob, 0, 4.0, 1.0),
    low(P_UNISON_SPREAD, "Spread", Kind::Knob, 0, 5.0, 1.0),
    low(P_DRIFT, "Drift", Kind::Knob, 0, 6.0, 1.0),
    low(P_PRIORITY, "PRIORITY", Kind::Segmented(PRIO_WORDS), 0, 7.5, 2.5),
    low(P_TRIGGER, "TRIGGER", Kind::Segmented(TRIG_WORDS), 0, 10.5, 2.0),
    // FILTER and the LFO
    low(P_FILTER_MODEL, "FILTER", Kind::Segmented(FILTER_WORDS), 1, 0.0, 3.0),
    low(P_LFO_RATE, "LFO Rate", Kind::Knob, 1, 3.0, 1.0),
    low(P_LFO_WAVE, "LFO Wave", Kind::Stepped(5), 1, 4.0, 1.0),
    low(P_LFO_DELAY, "LFO Delay", Kind::Knob, 1, 5.0, 1.0),
    low(P_OSC_MOD, "OSC MOD", Kind::Toggle, 1, 6.5, 1.2),
    low(P_FILTER_MOD, "FLT MOD", Kind::Toggle, 1, 8.0, 1.2),
    // ARP
    low(P_ARP_MODE, "ARP", Kind::Segmented(ARP_WORDS), 2, 0.0, 5.0),
    low(P_ARP_RATE, "Rate", Kind::Stepped(7), 2, 5.0, 1.0),
    low(P_ARP_OCT, "Octaves", Kind::Stepped(4), 2, 6.0, 1.0),
    low(P_ARP_GATE, "Gate", Kind::Knob, 2, 7.0, 1.0),
    low(P_ARP_HOLD, "HOLD", Kind::Toggle, 2, 8.5, 1.2),
];

// ── the geometry ────────────────────────────────────────────────────────────

/// The header band: the model name and the badge.
const HEADER_H: f32 = 22.0;
/// The engraved section captions' band.
const CAPTION_H: f32 = 13.0;
/// A row of dials, tallest and shortest. The shortest still holds a dial a
/// hand can turn and the two lines of text around it on JetBrains Mono.
const ROW_MAX: f32 = 92.0;
const ROW_MIN: f32 = 46.0;
/// The panel's inset from the window, and the air between the two bands.
const MARGIN: f32 = 4.0;
const BAND_AIR: f32 = 6.0;
/// The name and reading lines around a dial, as a share of the row.
const NAME_H: f32 = 11.0;
const READING_H: f32 = 11.0;
/// How far a drag turns a dial per point, and with shift held.
const DRAG_PER_PT: f32 = 0.006;
const FINE_PER_PT: f32 = 0.0015;
/// Points of drag per detent on a stepped dial.
const STEP_PTS: f32 = 14.0;
/// The house's light slot, and the moulded body: the two things on a dial
/// that are NOT themed, so the pointer reads on every cap.
const SLOT: Color32 = Color32::from_rgb(0xd8, 0xe4, 0xf2);
const BODY: Color32 = Color32::from_rgb(0x14, 0x12, 0x12);
/// The sweep, centred on up: three quarters of a turn.
const SWEEP: f32 = std::f32::consts::PI * 1.5;
const TICKS: usize = 11;

/// The rects everything is drawn in, from the body the panel was given.
struct Layout {
    header: Rect,
    /// Each upper section's caption band and its grid origin.
    sections: [Rect; 5],
    upper_row_h: f32,
    lower_row_h: f32,
    /// A dial column's width.
    col_w: f32,
    lower_top: f32,
    left: f32,
}

impl Layout {
    /// The whole panel from the body rect: rows give up their air first,
    /// and the dial column is the width divided evenly. Nothing here
    /// depends on the font; the text fits itself to what it is given.
    fn new(body: Rect) -> Self {
        let inner = body.shrink(MARGIN);
        let header = Rect::from_min_size(inner.min, Vec2::new(inner.width(), HEADER_H));
        let rows_room = (inner.height() - HEADER_H - CAPTION_H - BAND_AIR - 2.0).max(0.0);
        let row_h = (rows_room / (UPPER_ROWS + LOWER_ROWS) as f32).clamp(ROW_MIN, ROW_MAX);
        let col_w = (inner.width() / COLS as f32).min(120.0);
        let used_w = col_w * COLS as f32;
        let left = inner.left() + (inner.width() - used_w) * 0.5;
        let mut sections = [Rect::NOTHING; 5];
        let mut x = left;
        let top = header.bottom();
        for (i, (_, cols)) in SECTIONS.iter().enumerate() {
            let w = col_w * *cols as f32;
            sections[i] = Rect::from_min_size(
                Pos2::new(x, top),
                Vec2::new(w, CAPTION_H + row_h * UPPER_ROWS as f32),
            );
            x += w;
        }
        let lower_top = top + CAPTION_H + row_h * UPPER_ROWS as f32 + BAND_AIR;
        Self {
            header,
            sections,
            upper_row_h: row_h,
            lower_row_h: row_h,
            col_w,
            lower_top,
            left,
        }
    }

    fn rect_of(&self, c: &Ctl) -> Rect {
        match c.area {
            Area::Upper { section, row, col } => {
                let s = self.sections[section];
                Rect::from_min_size(
                    Pos2::new(
                        s.left() + col as f32 * self.col_w,
                        s.top() + CAPTION_H + row as f32 * self.upper_row_h,
                    ),
                    Vec2::new(self.col_w, self.upper_row_h),
                )
            }
            Area::Lower { row, x, w } => Rect::from_min_size(
                Pos2::new(self.left + x * self.col_w, self.lower_top + row as f32 * self.lower_row_h),
                Vec2::new(w * self.col_w, self.lower_row_h),
            ),
        }
    }

}

// ── the theme, applied ──────────────────────────────────────────────────────

/// The five sections' cap colours, from the theme's own accent: five hues
/// around it, each walked until the house's light slot clears it by a
/// reading margin and the cap still stands off the black body. One
/// derivation, so the caps are the theme's and never a plate of paint
/// somebody chose once for one theme.
fn caps(st: &FaceStyle) -> [Color32; 5] {
    let (h, s, l) = hsl(st.lcd);
    let turns = [0.0_f32, 0.11, 0.22, 0.55, 0.72];
    std::array::from_fn(|i| legible_cap(from_hsl((h + turns[i]).fract(), s.max(0.45), l)))
}

/// Walk a cap dark until the slot reads on it at 3:1 and it still reads
/// against the body at 1.4:1 — the dead-zone law applied to a dial: when
/// the ink cannot move, the surface does.
fn legible_cap(c: Color32) -> Color32 {
    let (h, s, mut l) = hsl(c);
    l = l.clamp(0.14, 0.42);
    let mut out = from_hsl(h, s, l);
    let mut guard = 0;
    while contrast_ratio(SLOT, out) < 3.0 && guard < 40 {
        l = (l - 0.02).max(0.05);
        out = from_hsl(h, s, l);
        guard += 1;
    }
    guard = 0;
    while contrast_ratio(out, BODY) < 1.4 && guard < 40 {
        l = (l + 0.02).min(0.42);
        out = from_hsl(h, s, l);
        guard += 1;
    }
    out
}

fn hsl(c: Color32) -> (f32, f32, f32) {
    let (r, g, b) = (
        f32::from(c.r()) / 255.0,
        f32::from(c.g()) / 255.0,
        f32::from(c.b()) / 255.0,
    );
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) * 0.5;
    if (max - min).abs() < 1e-6 {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    } / 6.0;
    (h, s, l)
}

fn from_hsl(h: f32, s: f32, l: f32) -> Color32 {
    let hue = |p: f32, q: f32, t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    let (r, g, b) = if s <= 0.0 {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1.0 + s) } else { l + s - l * s };
        let p = 2.0 * l - q;
        (hue(p, q, h + 1.0 / 3.0), hue(p, q, h), hue(p, q, h - 1.0 / 3.0))
    };
    let ch = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    Color32::from_rgb(ch(r), ch(g), ch(b))
}

/// The section a control's cap comes from: its own upstairs, and the
/// nearest relative downstairs.
fn cap_section(c: &Ctl) -> usize {
    match c.area {
        Area::Upper { section, .. } => section,
        Area::Lower { row, .. } => [4, 3, 0][row.min(2)],
    }
}

// ── the panel ───────────────────────────────────────────────────────────────

/// The pushed value of `id`, or the dial's rest if the host has not pushed
/// it — a face draws pushed data and nothing else.
fn value(live: &FaceLive, id: u32) -> Option<(f32, f32, &str)> {
    live.face
        .params
        .iter()
        .find(|p| p.id == id)
        .map(|p| (p.value, p.default, p.reading.as_str()))
}

/// The layout the last frame drew, for the tests: `(id, rect)` per
/// control, left in egui's own temp store rather than on `FaceLive`,
/// because the panel has no business growing a field for a harness.
fn layout_key() -> egui::Id {
    egui::Id::new("faces_analog::layout")
}

/// The whole panel. Returns at most one ask a frame.
pub fn draw_analog(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    // **The room the panel may spend, asked before it spends anything**:
    // the window less the preset strip `faces::draw` appends under every
    // face, never less than the rows' floor — the amp's bargain.
    let foot = crate::faces::preset_foot_height(ui);
    let avail = ui.available_size();
    let body_h = (avail.y - foot).max(HEADER_H + CAPTION_H + ROW_MIN * (UPPER_ROWS + LOWER_ROWS) as f32);
    let (body, _) = ui.allocate_exact_size(Vec2::new(avail.x, body_h), Sense::hover());
    let lay = Layout::new(body);
    let painter = ui.painter_at(body);
    let caps = caps(st);
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    let mut out: Option<FaceAction> = None;

    // The header: the model name at the left, the mark at the right.
    painter.text(
        Pos2::new(lay.header.left() + 2.0, lay.header.center().y),
        Align2::LEFT_CENTER,
        "TANGENT ANALOG",
        bold(11.0),
        st.s.ink,
    );
    let badge = Rect::from_center_size(
        Pos2::new(lay.header.right() - HEADER_H * 0.5, lay.header.center().y),
        Vec2::splat(HEADER_H - 4.0),
    );
    face::triad_badge(&painter, badge, st.s.ink);
    painter.line_segment(
        [
            Pos2::new(lay.header.left(), lay.header.bottom()),
            Pos2::new(lay.header.right(), lay.header.bottom()),
        ],
        Stroke::new(1.0, st.s.edge),
    );

    // The section captions and their dividers.
    for (i, (name, _)) in SECTIONS.iter().enumerate() {
        let s = lay.sections[i];
        painter.text(
            Pos2::new(s.center().x, s.top() + CAPTION_H * 0.5),
            Align2::CENTER_CENTER,
            *name,
            bold(9.0),
            st.s.faint,
        );
        if i > 0 {
            painter.line_segment(
                [Pos2::new(s.left(), s.top() + 2.0), Pos2::new(s.left(), s.bottom() - 2.0)],
                Stroke::new(1.0, st.s.edge),
            );
        }
    }
    let band_y = lay.lower_top - BAND_AIR * 0.5;
    painter.line_segment(
        [Pos2::new(lay.header.left(), band_y), Pos2::new(lay.header.right(), band_y)],
        Stroke::new(1.0, st.s.edge),
    );

    // The dead dial: Mode under the ladder.
    let ladder = value(live, P_FILTER_MODEL).is_some_and(|(v, _, _)| v < 0.5);

    let mut rects: Vec<(u32, Rect)> = Vec::with_capacity(CONTROLS.len());
    for c in CONTROLS {
        let cell = lay.rect_of(c);
        rects.push((c.id, cell));
        let Some((v, default, reading)) = value(live, c.id) else {
            continue;
        };
        let dead = c.id == P_SEM_MODE && ladder;
        let cap = caps[cap_section(c)];
        let asked = match c.kind {
            Kind::Knob => knob(ui, &painter, st, c, cell, v, default, reading, cap, dead, None),
            Kind::Stepped(n) => knob(ui, &painter, st, c, cell, v, default, reading, cap, false, Some(n)),
            Kind::Toggle => toggle(ui, &painter, st, c, cell, v),
            Kind::Segmented(words) => segmented(ui, &painter, st, c, cell, v, words),
        };
        face::keep(&mut out, asked.map(|value| FaceAction::Set { id: c.id, value }));
    }
    ui.data_mut(|d| d.insert_temp(layout_key(), rects));
    out
}

/// A font size for `text` that fits `width`, from `nominal` down to the
/// smallest legible; past that the word is cut with an ellipsis by the
/// caller.
fn fit(painter: &Painter, width: f32, text: &str, nominal: f32, family: egui::FontFamily) -> f32 {
    let mut size = nominal;
    while size > 6.5 {
        let w = painter
            .layout_no_wrap(text.to_owned(), FontId::new(size, family.clone()), Color32::WHITE)
            .size()
            .x;
        if w <= width {
            break;
        }
        size -= 0.5;
    }
    size.max(6.5)
}

/// A word at the largest size that fits, cut if it never does.
fn word(painter: &Painter, at: Pos2, anchor: Align2, width: f32, text: &str, nominal: f32, family: egui::FontFamily, ink: Color32) {
    let size = fit(painter, width, text, nominal, family.clone());
    let font = FontId::new(size, family);
    let shown = crate::drumpad_panel::cut_text(painter, width, text, &font);
    painter.text(at, anchor, shown, font, ink);
}

/// One dial: the name above, the reading below, the body between, the
/// ticks outside, the slot through the cap. `steps` makes it a rotary
/// switch: detents, a click to walk, the reading its position's word.
#[allow(clippy::too_many_arguments)]
fn knob(
    ui: &mut egui::Ui,
    painter: &Painter,
    st: &FaceStyle,
    c: &Ctl,
    cell: Rect,
    v: f32,
    default: f32,
    reading: &str,
    cap: Color32,
    dead: bool,
    steps: Option<i32>,
) -> Option<f32> {
    let id = ui.id().with(("analog", c.id));
    let resp = if dead {
        None
    } else {
        Some(ui.interact(cell, id, Sense::click_and_drag()))
    };
    let turning = resp.as_ref().is_some_and(|r| r.dragged());
    // The name on top, the reading at the bottom, the dial in what is left.
    let name_h = NAME_H.min(cell.height() * 0.25);
    let reading_h = READING_H.min(cell.height() * 0.25);
    let text_w = cell.width() - 4.0;
    word(
        painter,
        Pos2::new(cell.center().x, cell.top() + 1.0),
        Align2::CENTER_TOP,
        text_w,
        c.label,
        9.0,
        fonts::courier_bold(),
        st.s.faint,
    );
    word(
        painter,
        Pos2::new(cell.center().x, cell.bottom() - 1.0),
        Align2::CENTER_BOTTOM,
        text_w,
        reading,
        9.0,
        egui::FontFamily::Monospace,
        if turning { st.s.ink } else { st.s.faint },
    );
    let dial = Rect::from_min_max(
        Pos2::new(cell.left(), cell.top() + name_h + 1.0),
        Pos2::new(cell.right(), cell.bottom() - reading_h - 1.0),
    );
    // The ticks stand outside the body, so the body is sized to leave
    // them room inside the cell.
    let rad = (dial.width().min(dial.height()) * 0.5 / 1.32).max(4.0);
    let centre = dial.center();
    let t = v.clamp(0.0, 1.0);
    if !dead {
        let tick = Stroke::new((rad * 0.09).max(0.7), st.s.faint);
        for i in 0..TICKS {
            let a = (i as f32 / (TICKS - 1) as f32 - 0.5) * SWEEP;
            let d = Vec2::new(a.sin(), -a.cos());
            painter.line_segment([centre + d * (rad * 1.09), centre + d * (rad * 1.26)], tick);
        }
    }
    painter.circle_filled(centre, rad, BODY);
    let cap_r = rad * 0.64;
    painter.circle_filled(centre, cap_r, if dead { st.s.well } else { cap });
    if !dead {
        // A stepped dial sits on its detent; a continuous one wherever it
        // was left.
        let shown = match steps {
            Some(n) => step_of(t, n) as f32 / (n.max(2) - 1) as f32,
            None => t,
        };
        let angle = (shown - 0.5) * SWEEP;
        let d = Vec2::new(angle.sin(), -angle.cos());
        painter.line_segment(
            [centre - d * cap_r * 0.92, centre + d * cap_r * 0.92],
            Stroke::new((cap_r * 0.30).max(1.0), SLOT),
        );
    }
    let resp = resp?;
    resp.clone().on_hover_text(match steps {
        Some(_) => "drag or click to step; right-click steps back; alt-click resets",
        None => "drag to turn; shift for fine; double-click or alt-click to reset",
    });
    match steps {
        None => {
            let hand = resp.clone().interact(Sense::click());
            if face::home_gesture(&hand) {
                return Some(default);
            }
            if resp.dragged() {
                let fine = ui.input(|i| i.modifiers.shift);
                let dy = resp.drag_delta().y;
                if dy != 0.0 {
                    let per = if fine { FINE_PER_PT } else { DRAG_PER_PT };
                    return Some((v - dy * per).clamp(0.0, 1.0));
                }
            }
            None
        }
        Some(n) => {
            let n = n.max(2);
            let at = step_of(t, n) as i32;
            let to = |k: i32| Some(k.rem_euclid(n) as f32 / (n - 1) as f32);
            if face::alt_home_gesture(&resp) {
                return Some(default);
            }
            if resp.clicked() {
                return to(at + 1);
            }
            if resp.secondary_clicked() {
                return to(at - 1);
            }
            if resp.dragged() {
                // Detents: the drag accumulates and every STEP_PTS of it is
                // one position, remembered across frames in egui's store.
                let key = id.with("accum");
                let mut acc: f32 = ui.data(|d| d.get_temp(key)).unwrap_or(0.0);
                acc -= resp.drag_delta().y;
                let moved = (acc / STEP_PTS).trunc() as i32;
                acc -= moved as f32 * STEP_PTS;
                ui.data_mut(|d| d.insert_temp(key, acc));
                if moved != 0 {
                    let k = (at + moved).clamp(0, n - 1);
                    if k != at {
                        return to(k);
                    }
                }
            }
            None
        }
    }
}

/// The detent a normalised value sits on, out of `n`.
fn step_of(v: f32, n: i32) -> usize {
    let last = (n.max(2) - 1) as f32;
    ((v.clamp(0.0, 1.0) * last).round() as usize).min(n.max(2) as usize - 1)
}

/// A rocker: the word on a raised plate when on, on the well when off.
fn toggle(ui: &mut egui::Ui, painter: &Painter, st: &FaceStyle, c: &Ctl, cell: Rect, v: f32) -> Option<f32> {
    let on = v >= 0.5;
    let h = (cell.height() * 0.42).clamp(16.0, 26.0);
    let w = (cell.width() - 8.0).max(20.0);
    let plate = Rect::from_center_size(cell.center(), Vec2::new(w, h));
    let ground = if on { st.s.raised } else { st.s.well };
    painter.rect_filled(plate, 3.0, ground);
    painter.rect_stroke(plate, 3.0, Stroke::new(1.0, st.s.edge), StrokeKind::Inside);
    let ink = if on { st.s.ink } else { face::quiet_ink(st, ground) };
    word(
        painter,
        plate.center(),
        Align2::CENTER_CENTER,
        w - 6.0,
        c.label,
        9.0,
        fonts::courier_bold(),
        ink,
    );
    let resp = ui.interact(plate, ui.id().with(("analog", c.id)), Sense::click());
    resp.clone().on_hover_text(if on { "on - click to switch off" } else { "off - click to switch on" });
    resp.clicked().then_some(if on { 0.0 } else { 1.0 })
}

/// A segmented switch: the caption above, the words in a row, the chosen
/// one on the raised plate in the panel's ink, the rest engraved.
fn segmented(
    ui: &mut egui::Ui,
    painter: &Painter,
    st: &FaceStyle,
    c: &Ctl,
    cell: Rect,
    v: f32,
    words: &[&str],
) -> Option<f32> {
    let n = words.len().max(2);
    let at = step_of(v, n as i32);
    let caption_h = NAME_H.min(cell.height() * 0.25);
    word(
        painter,
        Pos2::new(cell.center().x, cell.top() + 1.0),
        Align2::CENTER_TOP,
        cell.width() - 4.0,
        c.label,
        9.0,
        fonts::courier_bold(),
        st.s.faint,
    );
    let h = (cell.height() * 0.42).clamp(16.0, 26.0);
    let row = Rect::from_min_size(
        Pos2::new(cell.left() + 3.0, cell.top() + caption_h + (cell.height() - caption_h - h) * 0.5),
        Vec2::new(cell.width() - 6.0, h),
    );
    painter.rect_stroke(row, 3.0, Stroke::new(1.0, st.s.edge), StrokeKind::Inside);
    let seg_w = row.width() / n as f32;
    let mut asked = None;
    for (i, w) in words.iter().enumerate() {
        let seg = Rect::from_min_size(
            Pos2::new(row.left() + i as f32 * seg_w, row.top()),
            Vec2::new(seg_w, h),
        );
        let chosen = i == at;
        if chosen {
            painter.rect_filled(seg.shrink(1.0), 2.0, st.s.raised);
        }
        word(
            painter,
            seg.center(),
            Align2::CENTER_CENTER,
            seg_w - 4.0,
            w,
            8.5,
            fonts::courier_bold(),
            if chosen { st.s.ink } else { st.s.faint },
        );
        let resp = ui.interact(seg, ui.id().with(("analog", c.id, i)), Sense::click());
        if resp.clicked() && asked.is_none() {
            asked = Some(i as f32 / (n - 1) as f32);
        }
    }
    asked
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::fit;

    fn style() -> FaceStyle {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        FaceStyle::new(p.bg, p.lcd, p.lcd_face)
    }

    /// A pushed face with the analog's real ids, symbols from the id
    /// number, every value at a middling place.
    fn a_face(filter_model: f32) -> FaceLive {
        let mut live = fit::a_worst_case_face(ANALOG_PATH, ID_CEILING, STEPPED, false);
        for p in &mut live.face.params {
            if p.id == P_FILTER_MODEL {
                p.value = filter_model;
            }
        }
        live
    }

    /// The three spellings of the sentinel agree, and the voice table
    /// answers for it — the title the face wears comes from there.
    #[test]
    fn the_sentinel_is_spelled_once() {
        assert_eq!(ANALOG_PATH, crate::dialogs::ANALOG_PATH);
        assert!(crate::faces::is_voice_face(ANALOG_PATH));
        let (name, about) = crate::faces::suite_unit(ANALOG_PATH).expect("a title");
        assert_eq!(name, crate::dialogs::ANALOG_NAME);
        assert_eq!(about, ANALOG_ABOUT);
        assert!(!crate::faces::SUITE_UNITS.iter().any(|(p, ..)| *p == ANALOG_PATH), "a voice in the effects table");
    }

    /// **Every control is one control**: the table names each id once,
    /// the stepped ids are exactly the ones drawn as switches or detented
    /// dials, and the ids are the engine's — every one under the ceiling
    /// and none the pushed face lacks.
    #[test]
    fn every_control_names_a_pushed_parameter_once() {
        let mut ids: Vec<u32> = CONTROLS.iter().map(|c| c.id).collect();
        ids.sort_unstable();
        let n = ids.len();
        ids.dedup();
        assert_eq!(ids.len(), n, "an id is placed twice");
        assert_eq!(n, 61, "the table has {n} controls, the engine publishes 61");
        assert!(ids.iter().all(|&i| (i as usize) < ID_CEILING));
        for c in CONTROLS {
            let stepped = matches!(c.kind, Kind::Stepped(_) | Kind::Toggle | Kind::Segmented(_));
            assert_eq!(
                stepped,
                STEPPED.contains(&c.id),
                "{} ({}) is drawn {} but the table says otherwise",
                c.label,
                c.id,
                if stepped { "stepped" } else { "continuous" }
            );
            if let Kind::Segmented(words) = c.kind {
                let n = match c.id {
                    P_VOICE_MODE | P_PRIORITY => 3,
                    P_TRIGGER | P_FILTER_MODEL => 2,
                    P_ARP_MODE => 6,
                    _ => 0,
                };
                assert_eq!(words.len(), n, "{}'s words do not match its steps", c.label);
            }
        }
        let live = a_face(0.0);
        for c in CONTROLS {
            assert!(value(&live, c.id).is_some(), "{} finds no pushed value", c.label);
        }
    }

    /// **The floor is a promise**: at 900 by 560 and at the ceiling, on
    /// every font, the whole panel and the preset strip under it stay
    /// inside the window. Through `faces::draw`, so the strip is counted.
    #[test]
    fn the_analog_is_whole_at_its_own_window_on_every_font() {
        let st = style();
        let about = crate::faces::suite_unit(ANALOG_PATH).map_or("", |(_, a)| a);
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for (edge, window) in [
                ("opens at", Vec2::new(900.0, 560.0)),
                ("grown to", Vec2::new(1400.0, 840.0)),
            ] {
                let mut live = a_face(1.0);
                let (right, bottom) = fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                if right > fit::SLACK {
                    bad.push(format!("{} {edge} {}x{}: {right:.1} off the right", font.key(), window.x, window.y));
                }
                if bottom > fit::SLACK {
                    bad.push(format!("{} {edge} {}x{}: {bottom:.1} off the bottom", font.key(), window.x, window.y));
                }
            }
        }
        assert!(bad.is_empty(), "the analog is drawn outside its own window:\n  {}", bad.join("\n  "));
    }

    /// **Ink on its surface, on every theme**, both modes, at the floor:
    /// every word the panel prints wears an ink that clears the ground it
    /// was actually painted on — read off the shape stream, not the
    /// palette.
    #[test]
    fn the_analog_prints_its_ink_on_the_surface_it_lands_on_on_every_theme() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let mut bad: Vec<String> = Vec::new();
        let mut words_seen = usize::MAX;
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mut live = a_face(0.0);
                let words = fit::ink_on_its_surface(&ctx, Vec2::new(900.0, 560.0), ANALOG_ABOUT, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                let mut counted = 0usize;
                for (ink, under, text) in &words {
                    let floor = if *ink == st.s.ink {
                        4.5
                    } else if *ink == st.s.faint || *ink == face::quiet_ink(&st, st.s.well) {
                        3.0
                    } else {
                        continue;
                    };
                    counted += 1;
                    let r = contrast_ratio(*ink, *under);
                    if r < floor {
                        bad.push(format!("{} {}: \"{text}\" at {r:.2}:1", theme.name, if dark { "dark" } else { "light" }));
                    }
                }
                words_seen = words_seen.min(counted);
            }
        }
        assert!(words_seen >= 40, "the sweep recognised only {words_seen} words");
        assert!(bad.is_empty(), "unreadable on:\n  {}", bad.join("\n  "));
    }

    /// **The slot reads on every section's cap, on every theme**: the one
    /// un-themed ink on the panel against the five themed grounds it lands
    /// on, and each cap against the body it is inset in.
    #[test]
    fn the_slot_reads_on_every_cap_on_every_theme() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                for (i, cap) in caps(&st).iter().enumerate() {
                    let slot = contrast_ratio(SLOT, *cap);
                    let body = contrast_ratio(*cap, BODY);
                    if slot < 3.0 || body < 1.4 {
                        bad.push(format!("{} {}: section {i} slot {slot:.2}:1 body {body:.2}:1", theme.name, dark));
                    }
                }
                assert!(luminance(SLOT) > 0.5, "the slot is not the light one");
            }
        }
        assert!(bad.is_empty(), "a cap the slot cannot read on:\n  {}", bad.join("\n  "));
    }

    /// The badge is the icon's nine numbers: three nodes at the scaled
    /// vertices, three edges, all in the ink it was handed and nothing
    /// else — read off the shapes.
    #[test]
    fn the_badge_is_the_icons_geometry_in_the_panels_ink() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let ink = Color32::from_rgb(0x12, 0x34, 0x56);
        let r = Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::splat(64.0));
        let out = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                face::triad_badge(ui.painter(), r, ink);
            });
        });
        let mut circles = Vec::new();
        let mut lines = 0usize;
        fn walk(s: &egui::Shape, circles: &mut Vec<(Pos2, f32)>, lines: &mut usize, ink: Color32) {
            match s {
                egui::Shape::Vec(v) => v.iter().for_each(|s| walk(s, circles, lines, ink)),
                egui::Shape::Circle(c) if c.fill == ink => circles.push((c.center, c.radius)),
                egui::Shape::LineSegment { stroke, .. } if stroke.color == ink => *lines += 1,
                _ => {}
            }
        }
        for cs in &out.shapes {
            walk(&cs.shape, &mut circles, &mut lines, ink);
        }
        assert_eq!(lines, 3);
        assert_eq!(circles.len(), 3);
        let scale = 64.0 / 512.0;
        let want = [(256.0, 150.0), (136.0, 356.0), (376.0, 356.0)];
        for (x, y) in want {
            let at = r.center() + Vec2::new(x - 256.0, y - 256.0) * scale;
            assert!(
                circles.iter().any(|(c, rad)| c.distance(at) < 0.5 && (rad - 82.0 * scale).abs() < 0.1),
                "no node at ({x}, {y})"
            );
        }
    }

    /// Drive the panel through egui's own input: the pointer at `at`,
    /// pressed, moved by `by`, released, one frame each, and whatever the
    /// face asked for on the way.
    fn drag(ctx: &egui::Context, live: &mut FaceLive, st: &FaceStyle, at: Pos2, by: Vec2) -> Vec<FaceAction> {
        use egui::{Event, PointerButton};
        let mut asked = Vec::new();
        let frames: Vec<Vec<Event>> = vec![
            vec![Event::PointerMoved(at)],
            vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }],
            vec![Event::PointerMoved(at + by)],
            vec![Event::PointerButton { pos: at + by, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }],
            vec![],
        ];
        for events in frames {
            let out = draw_once(ctx, live, st, events);
            asked.extend(out);
        }
        asked
    }

    /// One frame of the panel at the floor window, with these input
    /// events, returning what it asked.
    fn draw_once(ctx: &egui::Context, live: &mut FaceLive, st: &FaceStyle, events: Vec<egui::Event>) -> Option<FaceAction> {
        let mut asked = None;
        let room = Vec2::new(900.0, 560.0 - crate::shell::CHROME_BAR_H);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().frame(egui::Frame::NONE).show(ctx, |ui| {
                    asked = draw_analog(ui, st, live);
                });
            },
        );
        asked
    }

    fn rect_of(ctx: &egui::Context, id: u32) -> Rect {
        let rects: Vec<(u32, Rect)> = ctx.data(|d| d.get_temp(layout_key())).expect("a layout was left");
        rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r).expect("the control is placed")
    }

    /// **A drag turns a dial and the panel asks for the value**, one ask,
    /// the right id, in the direction a hand expects: up is more.
    #[test]
    fn a_drag_turns_the_cutoff_and_asks_for_it() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(0.0);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let cell = rect_of(&ctx, P_CUTOFF);
        let before = value(&live, P_CUTOFF).unwrap().0;
        let asked = drag(&ctx, &mut live, &st, cell.center(), Vec2::new(0.0, -20.0));
        let sets: Vec<(u32, f32)> = asked
            .iter()
            .filter_map(|a| match a {
                FaceAction::Set { id, value } => Some((*id, *value)),
                _ => None,
            })
            .collect();
        assert!(!sets.is_empty(), "the drag asked for nothing");
        assert!(sets.iter().all(|(id, _)| *id == P_CUTOFF), "{sets:?}");
        let last = sets.last().unwrap().1;
        assert!(last > before, "dragging up turned it down: {before} -> {last}");
        assert!((last - before - 20.0 * DRAG_PER_PT).abs() < 1e-3, "{before} -> {last}");
    }

    /// **The Mode dial is dead under the ladder and alive under the SEM.**
    /// The same drag on the same rect asks for nothing with the ladder
    /// chosen and asks for Mode with the SEM chosen — and dead, it is
    /// drawn without its ticks.
    #[test]
    fn the_mode_dial_is_dead_under_the_ladder() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        for (model, alive) in [(0.0_f32, false), (1.0, true)] {
            let mut live = a_face(model);
            let _ = draw_once(&ctx, &mut live, &st, Vec::new());
            let cell = rect_of(&ctx, P_SEM_MODE);
            let asked = drag(&ctx, &mut live, &st, cell.center(), Vec2::new(0.0, -20.0));
            let moved = asked.iter().any(|a| matches!(a, FaceAction::Set { id, .. } if *id == P_SEM_MODE));
            assert_eq!(moved, alive, "filter {model}: the Mode dial {}", if moved { "moved" } else { "did not move" });
        }
    }

    /// A click on a segmented switch chooses that cell, and a click on a
    /// rocker flips it.
    #[test]
    fn switches_answer_a_click() {
        use egui::{Event, PointerButton};
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let st = style();
        let mut live = a_face(0.0);
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        // The third cell of VOICE: POLY.
        let cell = rect_of(&ctx, P_VOICE_MODE);
        let inner = Rect::from_min_size(Pos2::new(cell.left() + 3.0, cell.top()), Vec2::new(cell.width() - 6.0, cell.height()));
        let at = Pos2::new(inner.left() + inner.width() * (2.5 / 3.0), cell.center().y + NAME_H * 0.4);
        let click = |ctx: &egui::Context, live: &mut FaceLive, at: Pos2| -> Vec<FaceAction> {
            let mut got = Vec::new();
            for events in [
                vec![Event::PointerMoved(at)],
                vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: true, modifiers: Default::default() }],
                vec![Event::PointerButton { pos: at, button: PointerButton::Primary, pressed: false, modifiers: Default::default() }],
                vec![],
            ] {
                got.extend(draw_once(ctx, live, &st, events));
            }
            got
        };
        let asked = click(&ctx, &mut live, at);
        assert!(
            asked.iter().any(|a| matches!(a, FaceAction::Set { id, value } if *id == P_VOICE_MODE && (*value - 1.0).abs() < 1e-6)),
            "POLY was not chosen: {:?}",
            asked.iter().filter_map(|a| if let FaceAction::Set { id, value } = a { Some((*id, *value)) } else { None }).collect::<Vec<_>>()
        );
        // The harness pushes every value at a half, which reads as ON for
        // a rocker; put HOLD off first so the click has an "on" to ask for.
        for p in &mut live.face.params {
            if p.id == P_ARP_HOLD {
                p.value = 0.0;
            }
        }
        let _ = draw_once(&ctx, &mut live, &st, Vec::new());
        let hold = rect_of(&ctx, P_ARP_HOLD);
        let asked = click(&ctx, &mut live, hold.center());
        assert!(asked.iter().any(|a| matches!(a, FaceAction::Set { id, value } if *id == P_ARP_HOLD && *value > 0.5)), "HOLD did not flip on");
    }
}
