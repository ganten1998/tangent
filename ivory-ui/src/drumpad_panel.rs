//! The drumpad: sixteen pads in a popup that floats over whatever is open.
//!
//! A pure painter with a layout and a hit test, like every panel — the app
//! owns the kit document and the gestures; this file owns where things are
//! and what a press means. Modelled on the RS5k manager's rack: pads count
//! up from the BOTTOM-LEFT corner the way every drum machine since the MPC
//! has, a loaded pad wears its sample's name and a ghost of its waveform,
//! an empty pad sits visibly flatter, and the pad that just sounded lights
//! from the AUDIO — the engine's own peak — so a hardware strike shows
//! exactly like a mouse one.
//!
//! Samples arrive by dropping: a file from the outside world
//! (`raw.dropped_files`), or an in-app drag carrying [`DraggedMedia`] — the
//! contract the media manager publishes into `egui::DragAndDrop`. The panel
//! only ever draws the hover; the app does the taking.

use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

use crate::fonts;
use crate::recorder::{DrumPadParams, DRUM_PADS, DRUM_PAD_BASE};

/// What an in-app drag of a media file carries.
///
/// The drumpad is the first TAKER; the media manager is the first GIVER.
/// The giver calls `egui::DragAndDrop::set_payload(ctx, DraggedMedia(path))`
/// when the drag starts; while a payload of this type is in flight the
/// drumpad highlights the pad under the pointer, and the app takes the
/// payload on release. An OS file drag arrives by the other door and lands
/// on the same code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DraggedMedia(pub std::path::PathBuf);

/// The top strip's height: title, the MIDI chip, the ×. The MIDI editor's
/// own number — the two popups are siblings and should measure alike.
pub const STRIP_H: f32 = 26.0;
/// The inspector strip along the bottom: the selected pad's knobs.
pub const INSPECTOR_H: f32 = 46.0;
/// The gap between pads, and the grid's inset from the panel edge.
const GAP: f32 = 6.0;
/// The grid's sides. [`DRUM_PADS`] is the count; nothing below bakes in a
/// shape beyond "columns × rows fills it".
const COLS: usize = 4;
const ROWS: usize = DRUM_PADS / COLS;
/// Vertical drag distance for a knob's full travel, in points. The RS5k
/// manager's own feel.
pub const KNOB_TRAVEL: f32 = 150.0;
/// How many choke groups the cycle offers. 0 is "none"; A–D are 1..=4.
pub const CHOKE_GROUPS: u8 = 4;

// The popup family's face and ink — the MIDI editor's exact pair, because
// two popups in two palettes would read as two apps.
const FACE: Color32 = Color32::from_rgb(0x18, 0x12, 0x0F);
const INK: Color32 = Color32::from_rgb(0xFF, 0xF4, 0xE0);
/// The app's one red, for CLEAR and for a pad whose file went missing.
const REC_RED: Color32 = Color32::from_rgb(0xE8, 0x3A, 0x4E);
/// The meter green every meter in the app wears.
const METER_GREEN: Color32 = Color32::from_rgb(0x3D, 0xC0, 0x5A);
/// Tape amber, the capture cassette's colour — here it badges a choke group.
const AMBER: Color32 = Color32::from_rgb(0xC9, 0x8B, 0x3D);

/// One pad, as the app knows it.
#[derive(Debug, Clone, Copy)]
pub struct PadView<'a> {
    /// The sample's name — the file stem — or empty for an empty pad.
    pub label: &'a str,
    /// Decoded and in the engine: this pad will sound.
    pub loaded: bool,
    /// The document names a file the host could not read. Drawn in red so a
    /// moved folder is a thing you see, not a pad that went quiet.
    pub missing: bool,
    /// A coarse peak envelope of the sample, 0..=1 per bucket. May be empty.
    pub wave: &'a [f32],
    pub params: DrumPadParams,
}

/// Everything the drumpad draws, pushed in.
pub struct DrumpadView<'a> {
    pub pads: [PadView<'a>; DRUM_PADS],
    /// The loudest recent sample per pad, 0..≈1, decayed by the app — what
    /// lights a pad and drives its little meter.
    pub peaks: [f32; DRUM_PADS],
    pub selected: usize,
    /// The pad the mouse is holding down, for the pressed look.
    pub down: Option<usize>,
    /// Live MIDI notes in the pads' range mirror onto the pads while lit.
    pub midi_on: bool,
    /// The pad under a file drag — OS or in-app — if one is in flight.
    pub drop_hover: Option<usize>,
    /// The kit's own fader, linear.
    pub master: f32,
}

/// What a press on the drumpad means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    Close,
    /// The MIDI chip: toggle the live-note mirror.
    Midi,
    /// A pad: strike it, and select it for the inspector.
    Pad(usize),
    /// A knob in the inspector — a vertical drag target.
    Knob(Param),
    /// The choke chip: cycle the selected pad's group –, A, B, C, D.
    Choke,
    /// Clear the selected pad.
    Clear,
    /// The title strip's empty run: pick the panel up and move it.
    Strip,
}

/// The inspector's draggable values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Param {
    Gain,
    Pan,
    Tune,
    /// The kit fader, on the right end of the inspector.
    Master,
}

impl Param {
    /// `(low, high)` of the value the drag moves.
    pub fn range(self) -> (f32, f32) {
        match self {
            Param::Gain | Param::Master => (0.0, 2.0),
            Param::Pan => (-1.0, 1.0),
            Param::Tune => (-12.0, 12.0),
        }
    }

    /// Where the knob rests when double-clicked back to default.
    pub fn default_value(self) -> f32 {
        match self {
            Param::Gain | Param::Master => 1.0,
            Param::Pan | Param::Tune => 0.0,
        }
    }
}

pub struct Layout {
    pub panel: Rect,
    pub strip: Rect,
    pub close: Rect,
    pub midi: Rect,
    pub grid: Rect,
    pub pads: [Rect; DRUM_PADS],
    pub inspector: Rect,
    pub name: Rect,
    pub knob_gain: Rect,
    pub knob_pan: Rect,
    pub knob_tune: Rect,
    pub choke: Rect,
    pub clear: Rect,
    pub knob_master: Rect,
}

impl Layout {
    pub fn new(rect: Rect) -> Self {
        let nothing = Self {
            panel: Rect::NOTHING,
            strip: Rect::NOTHING,
            close: Rect::NOTHING,
            midi: Rect::NOTHING,
            grid: Rect::NOTHING,
            pads: [Rect::NOTHING; DRUM_PADS],
            inspector: Rect::NOTHING,
            name: Rect::NOTHING,
            knob_gain: Rect::NOTHING,
            knob_pan: Rect::NOTHING,
            knob_tune: Rect::NOTHING,
            choke: Rect::NOTHING,
            clear: Rect::NOTHING,
            knob_master: Rect::NOTHING,
        };
        if !rect.is_positive() || rect.width() < 120.0 || rect.height() < 120.0 {
            return nothing;
        }
        let strip = Rect::from_min_size(rect.min, Vec2::new(rect.width(), STRIP_H));
        // The tools hang on the strip's right, exactly as the editor's do:
        // × at the very edge, then the MIDI chip walking left.
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
        let midi = cell(side * 2.2);

        let inspector = Rect::from_min_max(
            Pos2::new(rect.left(), rect.bottom() - INSPECTOR_H),
            rect.max,
        );
        let grid = Rect::from_min_max(
            Pos2::new(rect.left() + GAP, strip.bottom() + GAP),
            Pos2::new(rect.right() - GAP, inspector.top() - GAP),
        );
        if !grid.is_positive() {
            return nothing;
        }
        // Pad 0 sits BOTTOM-LEFT and rows build upward — the drum machine's
        // own reading order, and the note order: up the grid is up in pitch.
        let cw = (grid.width() - GAP * (COLS as f32 - 1.0)) / COLS as f32;
        let ch = (grid.height() - GAP * (ROWS as f32 - 1.0)) / ROWS as f32;
        let pads = std::array::from_fn(|i| {
            let col = i % COLS;
            let row = i / COLS;
            let x0 = grid.left() + col as f32 * (cw + GAP);
            let y0 = grid.bottom() - ch - row as f32 * (ch + GAP);
            Rect::from_min_size(Pos2::new(x0, y0), Vec2::new(cw, ch))
        });

        // The inspector, left to right: the name, three knobs, the choke
        // chip, CLEAR — and the kit fader alone on the right end.
        let ins = inspector.shrink2(Vec2::new(GAP, 5.0));
        let knob_w = (ins.width() * 0.11).clamp(34.0, 64.0);
        let chip_w = (ins.width() * 0.09).clamp(30.0, 52.0);
        let mut right = ins.right();
        let mut rcell = |w: f32| {
            let r = Rect::from_min_max(Pos2::new(right - w, ins.top()), Pos2::new(right, ins.bottom()));
            right -= w + GAP;
            r
        };
        let knob_master = rcell(knob_w);
        let clear = rcell(chip_w);
        let choke = rcell(chip_w);
        let knob_tune = rcell(knob_w);
        let knob_pan = rcell(knob_w);
        let knob_gain = rcell(knob_w);
        let name = Rect::from_min_max(ins.min, Pos2::new((right - GAP).max(ins.left()), ins.bottom()));

        Self {
            panel: rect,
            strip,
            close,
            midi,
            grid,
            pads,
            inspector,
            name,
            knob_gain,
            knob_pan,
            knob_tune,
            choke,
            clear,
            knob_master,
        }
    }

    /// Every knob and the value it shows, for the app's drag handler.
    pub fn knob_of(&self, p: Param) -> Rect {
        match p {
            Param::Gain => self.knob_gain,
            Param::Pan => self.knob_pan,
            Param::Tune => self.knob_tune,
            Param::Master => self.knob_master,
        }
    }
}

/// Where the panel stands: `at` is the strip's remembered top-left from a
/// drag, clamped back onto the screen; `None` centres it over the pane.
pub fn place(screen: Rect, at: Option<Pos2>) -> Rect {
    let w = (screen.width() * 0.44).clamp(340.0, 560.0).min(screen.width());
    // The grid wants square-ish pads: strip + inspector + 4 rows.
    let h = (STRIP_H + INSPECTOR_H + w * 0.82).min(screen.height() * 0.92);
    let mut panel = match at {
        Some(p) => Rect::from_min_size(p, Vec2::new(w, h)),
        None => Rect::from_center_size(screen.center(), Vec2::new(w, h)),
    };
    let dx = (screen.left() - panel.left()).max(0.0) - (panel.right() - screen.right()).max(0.0);
    let dy = (screen.top() - panel.top()).max(0.0) - (panel.bottom() - screen.bottom()).max(0.0);
    panel = panel.translate(Vec2::new(dx, dy));
    panel
}

/// Pad `i`'s note, and its name in the editor's own spelling (60 = C4).
pub fn pad_note(i: usize) -> u8 {
    DRUM_PAD_BASE.saturating_add(i as u8)
}

pub fn note_name(note: u8) -> String {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    format!(
        "{}{}",
        NAMES[usize::from(note % 12)],
        i32::from(note / 12) - 1
    )
}

// ── drawing ────────────────────────────────────────────────────────────────

fn font(size: f32) -> FontId {
    FontId::new(size, fonts::courier_bold())
}

/// Fit-to-width sizing without a galley, the recorder's own arithmetic.
const ADV: f32 = 0.62;
const MIN_TEXT: f32 = 5.0;
fn fit_text(r: Rect, text: &str, nominal: f32) -> f32 {
    let n = text.chars().count().max(1) as f32;
    nominal.min(r.width() / (n * ADV)).max(0.0)
}

fn db_word(linear: f32) -> String {
    if linear <= 0.001 {
        "-inf".to_owned()
    } else {
        format!("{:+.1}", 20.0 * linear.log10())
    }
}

fn pan_word(pan: f32) -> String {
    let p = (pan * 50.0).round() as i32;
    match p.cmp(&0) {
        std::cmp::Ordering::Less => format!("L{}", -p),
        std::cmp::Ordering::Equal => "C".to_owned(),
        std::cmp::Ordering::Greater => format!("R{p}"),
    }
}

fn tune_word(tune: f32) -> String {
    format!("{:+.0}st", tune.round())
}

pub fn choke_word(group: u8) -> &'static str {
    match group {
        1 => "A",
        2 => "B",
        3 => "C",
        4 => "D",
        _ => "-",
    }
}

/// One inspector knob: a label, the value, and a thin travel line under
/// them showing where in its range the value stands.
fn knob(painter: &Painter, r: Rect, label: &str, word: &str, frac: f32, active: bool) {
    if !r.is_positive() {
        return;
    }
    let ink = if active { INK } else { INK.gamma_multiply(0.35) };
    painter.text(
        Pos2::new(r.center().x, r.top() + 1.0),
        Align2::CENTER_TOP,
        label,
        font((r.height() * 0.30).clamp(7.0, 10.0)),
        ink.gamma_multiply(0.6),
    );
    painter.text(
        Pos2::new(r.center().x, r.center().y + 3.0),
        Align2::CENTER_CENTER,
        word,
        font(fit_text(r, word, (r.height() * 0.36).clamp(8.0, 12.0))),
        ink,
    );
    let line_y = r.bottom() - 3.0;
    painter.line_segment(
        [Pos2::new(r.left(), line_y), Pos2::new(r.right(), line_y)],
        Stroke::new(1.0, ink.gamma_multiply(0.25)),
    );
    let x = r.left() + r.width() * frac.clamp(0.0, 1.0);
    painter.line_segment(
        [Pos2::new(x, line_y - 3.0), Pos2::new(x, line_y + 2.0)],
        Stroke::new(2.0, ink),
    );
}

/// The chip idiom, the editor's exact one.
fn chip(painter: &Painter, r: Rect, word: &str, lit: bool, lit_ink: Color32) {
    if !r.is_positive() {
        return;
    }
    if lit {
        painter.rect_filled(r, 3.0, lit_ink.gamma_multiply(0.85));
    }
    painter.rect_stroke(
        r,
        3.0,
        Stroke::new(1.0, lit_ink.gamma_multiply(if lit { 0.9 } else { 0.4 })),
        egui::StrokeKind::Inside,
    );
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        word,
        font(r.height() * 0.55),
        if lit { FACE } else { lit_ink.gamma_multiply(0.7) },
    );
}

pub fn draw(painter: &Painter, rect: Rect, view: &DrumpadView<'_>) {
    let l = Layout::new(rect);
    if !l.panel.is_positive() {
        return;
    }
    // A popup over whatever is open: its own face, a firm border, and a
    // clip so nothing leaks onto the view below.
    let painter = &painter.with_clip_rect(l.panel.expand(1.0));
    painter.rect_filled(l.panel, 4.0, FACE);
    painter.rect_stroke(
        l.panel,
        4.0,
        Stroke::new(1.5, INK.gamma_multiply(0.5)),
        egui::StrokeKind::Inside,
    );

    // ── the strip ─────────────────────────────────────────────────────────
    painter.rect_filled(l.strip, 4.0, Color32::from_rgba_premultiplied(0, 0, 0, 60));
    painter.text(
        Pos2::new(l.strip.left() + 8.0, l.strip.center().y),
        Align2::LEFT_CENTER,
        "DRUMPAD",
        font(STRIP_H * 0.45),
        INK,
    );
    chip(painter, l.midi, "MIDI", view.midi_on, INK);
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

    // ── the pads ──────────────────────────────────────────────────────────
    for (i, r) in l.pads.iter().copied().enumerate() {
        if !r.is_positive() {
            continue;
        }
        let pad = &view.pads[i];
        let down = view.down == Some(i);
        let peak = view.peaks[i].clamp(0.0, 1.0);

        // A loaded pad is a solid key; an empty one is an outline you can
        // see through — the RS5k rack's own honesty about where work is.
        if pad.loaded || pad.missing {
            painter.rect_filled(r, 3.0, Color32::from_rgba_premultiplied(0, 0, 0, 110));
        } else {
            painter.rect_filled(r, 3.0, Color32::from_rgba_premultiplied(255, 255, 255, 5));
        }

        // The waveform ghost, faint, across the pad's middle.
        if pad.loaded && !pad.wave.is_empty() && r.width() > 24.0 {
            let mid = r.center().y;
            let half = r.height() * 0.28;
            let n = pad.wave.len();
            let step = r.width() / n as f32;
            for (b, v) in pad.wave.iter().enumerate() {
                let x = r.left() + (b as f32 + 0.5) * step;
                let a = (v.clamp(0.0, 1.0)) * half;
                painter.line_segment(
                    [Pos2::new(x, mid - a), Pos2::new(x, mid + a)],
                    Stroke::new(step.max(1.0), INK.gamma_multiply(0.13)),
                );
            }
        }

        // The strike, from the audio: the engine's peak lights the pad.
        let light = if down { 0.16 } else { 0.0 } + peak * 0.30;
        if light > 0.0 {
            painter.rect_filled(
                r,
                3.0,
                Color32::from_white_alpha((light * 255.0) as u8),
            );
        }

        // A file drag standing over this pad.
        if view.drop_hover == Some(i) {
            painter.rect_filled(r, 3.0, Color32::from_white_alpha(24));
            painter.rect_stroke(r, 3.0, Stroke::new(2.0, INK), egui::StrokeKind::Inside);
        } else if view.selected == i {
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.5, INK.gamma_multiply(0.9)),
                egui::StrokeKind::Inside,
            );
        } else {
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.0, Color32::from_black_alpha(140)),
                egui::StrokeKind::Inside,
            );
        }

        // The note name, small, top-left — the pad's address.
        let note = pad_note(i);
        if r.height() > 30.0 {
            painter.text(
                Pos2::new(r.left() + 4.0, r.top() + 3.0),
                Align2::LEFT_TOP,
                note_name(note),
                font((r.height() * 0.16).clamp(7.0, 10.0)),
                INK.gamma_multiply(0.4),
            );
        }
        // The choke badge, top-right, amber — the RS5k LED, as a letter.
        if pad.params.choke != 0 && r.height() > 30.0 {
            painter.text(
                Pos2::new(r.right() - 4.0, r.top() + 3.0),
                Align2::RIGHT_TOP,
                choke_word(pad.params.choke),
                font((r.height() * 0.16).clamp(7.0, 10.0)),
                AMBER,
            );
        }

        // The sample's name — or the pad's emptiness, said quietly.
        let label_r = r.shrink2(Vec2::new(4.0, r.height() * 0.30));
        if pad.missing {
            let size = fit_text(label_r, pad.label, (r.height() * 0.22).clamp(8.0, 13.0));
            if size >= MIN_TEXT {
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    pad.label,
                    font(size),
                    REC_RED,
                );
            }
        } else if pad.loaded {
            let size = fit_text(label_r, pad.label, (r.height() * 0.22).clamp(8.0, 13.0));
            if size >= MIN_TEXT {
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    pad.label,
                    font(size),
                    INK.gamma_multiply(0.85),
                );
            }
        }

        // The little meter, right edge, growing upward — RS5k's, at 3 px.
        if peak > 0.0 && r.height() > 24.0 {
            let h = (r.height() - 8.0) * peak;
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(r.right() - 5.0, r.bottom() - 4.0 - h),
                    Pos2::new(r.right() - 2.0, r.bottom() - 4.0),
                ),
                0.0,
                METER_GREEN.gamma_multiply(0.85),
            );
        }
    }

    // ── the inspector ─────────────────────────────────────────────────────
    painter.line_segment(
        [
            Pos2::new(l.panel.left() + GAP, l.inspector.top()),
            Pos2::new(l.panel.right() - GAP, l.inspector.top()),
        ],
        Stroke::new(1.0, INK.gamma_multiply(0.2)),
    );
    let sel = &view.pads[view.selected.min(DRUM_PADS - 1)];
    let has = sel.loaded || sel.missing;
    if l.name.is_positive() {
        let word = if has {
            format!("{}  {}", note_name(pad_note(view.selected)), sel.label)
        } else {
            "drop samples onto the pads".to_owned()
        };
        let size = fit_text(l.name, &word, 12.0);
        if size >= MIN_TEXT {
            painter.text(
                Pos2::new(l.name.left() + 2.0, l.name.center().y),
                Align2::LEFT_CENTER,
                word,
                font(size),
                if has {
                    INK.gamma_multiply(0.85)
                } else {
                    INK.gamma_multiply(0.35)
                },
            );
        }
    }
    let frac = |v: f32, p: Param| {
        let (lo, hi) = p.range();
        ((v - lo) / (hi - lo)).clamp(0.0, 1.0)
    };
    knob(
        painter,
        l.knob_gain,
        "GAIN",
        &db_word(sel.params.gain),
        frac(sel.params.gain, Param::Gain),
        has,
    );
    knob(
        painter,
        l.knob_pan,
        "PAN",
        &pan_word(sel.params.pan),
        frac(sel.params.pan, Param::Pan),
        has,
    );
    knob(
        painter,
        l.knob_tune,
        "TUNE",
        &tune_word(sel.params.tune),
        frac(sel.params.tune, Param::Tune),
        has,
    );
    chip(
        painter,
        l.choke,
        &format!("CHK {}", choke_word(sel.params.choke)),
        has && sel.params.choke != 0,
        if has { AMBER } else { AMBER.gamma_multiply(0.4) },
    );
    chip(
        painter,
        l.clear,
        "CLEAR",
        false,
        if has { REC_RED } else { REC_RED.gamma_multiply(0.4) },
    );
    knob(
        painter,
        l.knob_master,
        "KIT",
        &db_word(view.master),
        frac(view.master, Param::Master),
        true,
    );
}

/// What a press at `pos` means. Chips beat pads, pads beat the strip, and
/// the strip's empty run answers [`Hit::Strip`] so the panel can be moved.
pub fn hit_test(rect: Rect, view: &DrumpadView<'_>, pos: Pos2) -> Option<Hit> {
    let l = Layout::new(rect);
    if !l.panel.is_positive() || !l.panel.contains(pos) {
        return None;
    }
    for (r, hit) in [(l.close, Hit::Close), (l.midi, Hit::Midi)] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    if l.strip.contains(pos) {
        return Some(Hit::Strip);
    }
    for (i, r) in l.pads.iter().enumerate() {
        if r.is_positive() && r.contains(pos) {
            return Some(Hit::Pad(i));
        }
    }
    let sel = &view.pads[view.selected.min(DRUM_PADS - 1)];
    let has = sel.loaded || sel.missing;
    for (r, hit, want) in [
        (l.knob_gain, Hit::Knob(Param::Gain), has),
        (l.knob_pan, Hit::Knob(Param::Pan), has),
        (l.knob_tune, Hit::Knob(Param::Tune), has),
        (l.choke, Hit::Choke, has),
        (l.clear, Hit::Clear, has),
        (l.knob_master, Hit::Knob(Param::Master), true),
    ] {
        if want && r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    None
}

/// The pad under `pos`, for drop targeting — pads only, nothing else.
pub fn pad_at(rect: Rect, pos: Pos2) -> Option<usize> {
    let l = Layout::new(rect);
    if !l.panel.is_positive() {
        return None;
    }
    l.pads
        .iter()
        .position(|r| r.is_positive() && r.contains(pos))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn view<'a>() -> DrumpadView<'a> {
        let empty = PadView {
            label: "",
            loaded: false,
            missing: false,
            wave: &[],
            params: DrumPadParams::default(),
        };
        let mut pads = [empty; DRUM_PADS];
        pads[0] = PadView {
            label: "Kick",
            loaded: true,
            missing: false,
            wave: &[],
            params: DrumPadParams::default(),
        };
        DrumpadView {
            pads,
            peaks: [0.0; DRUM_PADS],
            selected: 0,
            down: None,
            midi_on: true,
            drop_hover: None,
            master: 1.0,
        }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(20.0, 20.0), Vec2::new(440.0, 460.0))
    }

    /// Pad 0 stands bottom-left and pad 15 top-right — the MPC's reading
    /// order — and the sixteen neither overlap nor leave the grid.
    #[test]
    fn the_pads_stand_like_a_drum_machine() {
        let l = Layout::new(rect());
        assert!(l.panel.is_positive());
        let first = l.pads[0];
        let last = l.pads[DRUM_PADS - 1];
        assert!(first.left() < last.left(), "pad 0 is not left of pad 15");
        assert!(first.top() > last.top(), "pad 0 is not below pad 15");
        for (i, a) in l.pads.iter().enumerate() {
            assert!(a.is_positive());
            assert!(l.grid.contains_rect(a.shrink(0.5)), "pad {i} leaks the grid");
            for (j, b) in l.pads.iter().enumerate().skip(i + 1) {
                assert!(
                    a.intersect(*b).area() < 1.0e-3,
                    "pads {i} and {j} overlap"
                );
            }
        }
    }

    /// Every pad answers a press at its centre, the chips answer theirs,
    /// the strip's empty run says "move me", and outside is nobody's.
    #[test]
    fn hits_land_where_they_look() {
        let v = view();
        let l = Layout::new(rect());
        for i in 0..DRUM_PADS {
            assert_eq!(hit_test(rect(), &v, l.pads[i].center()), Some(Hit::Pad(i)));
            assert_eq!(pad_at(rect(), l.pads[i].center()), Some(i));
        }
        assert_eq!(hit_test(rect(), &v, l.close.center()), Some(Hit::Close));
        assert_eq!(hit_test(rect(), &v, l.midi.center()), Some(Hit::Midi));
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(l.strip.left() + 40.0, l.strip.center().y)),
            Some(Hit::Strip)
        );
        assert_eq!(
            hit_test(rect(), &v, l.knob_gain.center()),
            Some(Hit::Knob(Param::Gain))
        );
        assert_eq!(
            hit_test(rect(), &v, l.knob_master.center()),
            Some(Hit::Knob(Param::Master))
        );
        assert_eq!(hit_test(rect(), &v, Pos2::new(2.0, 2.0)), None);
    }

    /// With nothing on the selected pad the per-pad controls stand down —
    /// but the kit fader still answers.
    #[test]
    fn an_empty_selection_disarms_the_pad_knobs() {
        let mut v = view();
        v.selected = 5;
        let l = Layout::new(rect());
        assert_eq!(hit_test(rect(), &v, l.knob_gain.center()), None);
        assert_eq!(hit_test(rect(), &v, l.clear.center()), None);
        assert_eq!(
            hit_test(rect(), &v, l.knob_master.center()),
            Some(Hit::Knob(Param::Master))
        );
    }

    /// The pads speak the drum map: 36 is C2 and the grid counts up from it.
    #[test]
    fn the_notes_read_like_the_drum_map() {
        assert_eq!(pad_note(0), 36);
        assert_eq!(note_name(36), "C2");
        assert_eq!(note_name(37), "C#2");
        assert_eq!(pad_note(DRUM_PADS - 1), 51);
        assert_eq!(note_name(60), "C4");
    }

    /// Degenerate rects neither panic nor answer presses.
    #[test]
    fn it_survives_small_rects() {
        let v = view();
        for size in [Vec2::ZERO, Vec2::new(60.0, 30.0), Vec2::new(4000.0, 2000.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 5.0), size);
            let _ = Layout::new(r);
            let _ = hit_test(r, &v, r.center());
            let _ = pad_at(r, r.center());
        }
    }

    /// The placement clamps back onto the screen however it is thrown.
    #[test]
    fn the_panel_stays_on_screen() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 800.0));
        for at in [
            None,
            Some(Pos2::new(-500.0, -500.0)),
            Some(Pos2::new(5000.0, 5000.0)),
        ] {
            let p = place(screen, at);
            assert!(p.is_positive());
            assert!(screen.contains_rect(p), "{at:?} left the screen: {p:?}");
        }
    }
}
