//! The arrangement view: eight lanes of clips against bars, headers on the
//! left, exactly where the piano stands until Tab brings this up instead.
//!
//! A pure painter, like every surface in this crate: [`draw`] takes a rect
//! and an [`ArrangeView`] and paints; [`hit_test`] is its inverse; and the
//! geometry helpers ([`time_at`], [`x_of`]) are the same functions both use,
//! so a clip cannot be drawn one place and grabbed another. No state, no
//! `egui::Ui`, nothing that can open a device or read a file — the APP owns
//! the selection, the drag, the zoom window and the document; this module
//! answers "where is everything, given those".
//!
//! # The time axis
//!
//! The view is a WINDOW onto the song: `view_start_s` and `view_len_s` are
//! the zoom, owned by the app, steered by the wheel, the ruler drag, and the
//! band scrubber's viewport. Everything here maps through one pair of
//! functions, which is what keeps a clip, its wave, the playhead and the
//! ruler on the same ruler.
//!
//! # What a press means
//!
//! The header cells are ordinary controls (arm, mute, solo, a gain knob).
//! On the lanes: a clip's BODY moves it, its EDGES trim it, its top corners
//! drag its fades — hit-tested in that priority, smallest target first,
//! because a fade handle lives inside the edge that lives inside the body.
//! Empty lane space is a LOCATE, and it clears the selection, which is the
//! gesture Ableton taught everyone. The ruler is a locate too; dragging it
//! vertically zooms (the app's half — the panel only names the surface).

use crate::mixer_panel::STRIP_COLORS;
use crate::recorder::{ChannelKind, CHANNELS};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

/// One lane's header, as the painter sees it. Lane N IS channel N — name,
/// colour, kind, arm, mute and solo are the mixer strip's own facts.
pub struct LaneView<'a> {
    pub name: &'a str,
    /// Index into [`STRIP_COLORS`]. Zero is the desk's own wood.
    pub color: usize,
    /// What feeds the lane live: keys, a microphone, or nothing. A lane with
    /// clips and no kind is a CLIP lane and wears the waveform.
    pub kind: Option<ChannelKind>,
    /// Whether the lane is anything at all — a kind, or clips standing on
    /// it. An unused lane draws faint and offers no header controls.
    pub used: bool,
    pub armed: bool,
    pub muted: bool,
    pub soloed: bool,
    /// Linear, as the mixer fader sits. The header knob is the same value.
    pub gain: f32,
    /// 0..=1, the strip's send — the second dial.
    pub send: f32,
    pub peak: [f32; 2],
}

/// One clip on a lane, resolved: `len_s` is real seconds (the host resolved
/// the document's zero sentinel), `max_len_s` is how long it COULD get
/// before running out of file (`f64::INFINITY` when the host has not met
/// the file).
pub struct ClipView<'a> {
    pub id: u64,
    pub lane: usize,
    pub start_s: f64,
    pub len_s: f64,
    pub max_len_s: f64,
    pub fade_in_s: f64,
    pub fade_out_s: f64,
    pub name: &'a str,
    /// A peak envelope of the PLAYED span, 0..=1. May be empty.
    pub wave: &'a [f32],
    pub selected: bool,
    /// A MIDI item: no file, no wave — drawn with its own face.
    pub midi: bool,
}

/// Everything the arrangement draws, pushed in.
pub struct ArrangeView<'a> {
    pub lanes: [LaneView<'a>; CHANNELS],
    pub clips: &'a [ClipView<'a>],
    /// The toolbar's three facts: the click, the magnet, and the grid as a
    /// power of two in bars (3 = 8 bars … 0 = 1 bar … -5 = 1/32).
    pub metronome_on: bool,
    pub snap_on: bool,
    pub grid_exp: i8,
    /// A MIDI item being drawn by shift-drag right now: lane, from, to —
    /// ghosted until the button comes up and the document takes it.
    pub pending: Option<(usize, f64, f64)>,
    /// The zoom window, in seconds of song.
    pub view_start_s: f64,
    pub view_len_s: f64,
    pub playhead_s: f64,
    /// The ruler's grid: bars from the band's own tempo and signature.
    pub bpm: f64,
    pub sig_top: u32,
    pub sig_bottom: u32,
    pub dark_mode: bool,
    /// The band's own wood, so the two surfaces are one instrument.
    pub wood: (u8, u8, u8),
}

/// What a press on the arrangement means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// The R dot on a lane's header.
    Arm(usize),
    Mute(usize),
    Solo(usize),
    /// The header's gain knob — draggable, vertical, the mixer fader's twin.
    Gain(usize),
    /// A clip's body: select it, and a drag MOVES it along its lane.
    ClipBody(u64),
    /// A clip's left edge: a drag trims its head (start, offset and length
    /// move together).
    ClipLeft(u64),
    /// A clip's right edge: a drag trims its tail.
    ClipRight(u64),
    /// The top-left corner: drag right for a fade in.
    FadeIn(u64),
    /// The top-right corner: drag left for a fade out.
    FadeOut(u64),
    /// Empty lane space: put the playhead HERE (seconds), clear the
    /// selection. The lane index rides along for the app's drop logic.
    Lane(usize, f64),
    /// The bars ruler: a locate on press; a vertical drag is the zoom, which
    /// the app runs through [`time_at`] against the press's anchor.
    Ruler(f64),
    /// The header's send dial — draggable and vertical, like the gain.
    Send(usize),
    /// The toolbar: the click on and off.
    ToolMetronome,
    /// The toolbar: the magnet.
    ToolSnap,
    /// The toolbar's grid box. A press steps FINER; the wheel over it is
    /// the real control, handled by the app from this same rectangle.
    ToolGrid,
}

impl Hit {
    /// The clip a hit is about, if it is about one.
    pub fn clip(self) -> Option<u64> {
        match self {
            Hit::ClipBody(id)
            | Hit::ClipLeft(id)
            | Hit::ClipRight(id)
            | Hit::FadeIn(id)
            | Hit::FadeOut(id) => Some(id),
            _ => None,
        }
    }
}

/// The header column's share of the width, and its bounds in points.
///
/// Measured against the REAPER header this is modelled on: number, arm,
/// name, knob, meter, M/S want about 180 points and stop being readable
/// under 120.
const HEADER_W: (f32, f32, f32) = (0.16, 120.0, 220.0);

/// The ruler's height in points. Short — it is a scale, not a band.
const RULER_H: (f32, f32, f32) = (0.06, 16.0, 24.0);

/// How wide a clip's trim edge is, in points, and its fade corner square.
const EDGE_W: f32 = 7.0;
const FADE_W: f32 = 11.0;

/// Where everything is. Fractions of the rect, derived once, read by both
/// the painter and the hit test.
pub struct Layout {
    pub header: Rect,
    pub ruler: Rect,
    /// The corner cell's toolbar: metronome, magnet, grid box.
    pub tool_met: Rect,
    pub tool_snap: Rect,
    pub tool_grid: Rect,
    pub lanes_area: Rect,
    pub lane_rows: [Rect; CHANNELS],
    pub header_rows: [HeaderRow; CHANNELS],
}

/// One header's cells.
#[derive(Debug, Clone, Copy)]
pub struct HeaderRow {
    pub panel: Rect,
    pub number: Rect,
    pub arm: Rect,
    pub name: Rect,
    pub knob: Rect,
    pub send: Rect,
    /// The readouts under the two dials: dB under the volume, % under the
    /// send. Text, not targets — the dials above them are the controls.
    pub knob_readout: Rect,
    pub send_readout: Rect,
    pub meter: Rect,
    pub mute: Rect,
    pub solo: Rect,
}

impl HeaderRow {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        number: Rect::NOTHING,
        arm: Rect::NOTHING,
        name: Rect::NOTHING,
        knob: Rect::NOTHING,
        send: Rect::NOTHING,
        knob_readout: Rect::NOTHING,
        send_readout: Rect::NOTHING,
        meter: Rect::NOTHING,
        mute: Rect::NOTHING,
        solo: Rect::NOTHING,
    };
}

impl Layout {
    pub fn new(rect: Rect, view: &ArrangeView<'_>) -> Self {
        if !rect.is_positive() {
            return Self {
                header: Rect::NOTHING,
                ruler: Rect::NOTHING,
                tool_met: Rect::NOTHING,
                tool_snap: Rect::NOTHING,
                tool_grid: Rect::NOTHING,
                lanes_area: Rect::NOTHING,
                lane_rows: [Rect::NOTHING; CHANNELS],
                header_rows: [HeaderRow::NONE; CHANNELS],
            };
        }
        let (share, lo, hi) = HEADER_W;
        // Chained rather than clamp(): a rect narrower than the floor would
        // hand clamp a min above its max, which is a panic in a painter.
        let header_w = (rect.width() * share).max(lo).min(hi).min(rect.width() * 0.5);
        let (rshare, rlo, rhi) = RULER_H;
        let ruler_h = (rect.height() * rshare).clamp(rlo, rhi);
        let header = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + ruler_h),
            Pos2::new(rect.left() + header_w, rect.bottom()),
        );
        let ruler = Rect::from_min_max(
            Pos2::new(rect.left() + header_w, rect.top()),
            Pos2::new(rect.right(), rect.top() + ruler_h),
        );
        let lanes_area = Rect::from_min_max(
            Pos2::new(rect.left() + header_w, rect.top() + ruler_h),
            rect.max,
        );
        let lane_h = lanes_area.height() / CHANNELS as f32;
        let lane_rows = std::array::from_fn(|i| {
            Rect::from_min_max(
                Pos2::new(lanes_area.left(), lanes_area.top() + lane_h * i as f32),
                Pos2::new(
                    lanes_area.right(),
                    lanes_area.top() + lane_h * (i + 1) as f32,
                ),
            )
        });
        let header_rows = std::array::from_fn(|i| {
            let row = Rect::from_min_max(
                Pos2::new(header.left(), header.top() + lane_h * i as f32),
                Pos2::new(header.right(), header.top() + lane_h * (i + 1) as f32),
            );
            let Some(lane) = view.lanes.get(i) else {
                return HeaderRow::NONE;
            };
            Self::header_row(row, lane.used)
        });
        // **The toolbar, in the corner cell** — the room above the track
        // list, before the ruler's first tick. Three small controls: the
        // click, the magnet, and the grid box the wheel turns.
        let corner = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.left() + header_w, rect.top() + ruler_h),
        )
        .shrink2(Vec2::new(3.0, 2.0));
        let side = (corner.height()).clamp(1.0, 20.0);
        let cell = |k: f32| {
            Rect::from_min_size(
                Pos2::new(corner.left() + k * (side + 3.0), corner.top()),
                Vec2::splat(side),
            )
        };
        let (tool_met, tool_snap) = (cell(0.0), cell(1.0));
        let tool_grid = Rect::from_min_max(
            Pos2::new(tool_snap.right() + 3.0, corner.top()),
            Pos2::new(
                (tool_snap.right() + 3.0 + side * 2.4).min(corner.right()),
                corner.bottom(),
            ),
        );
        Self {
            header,
            ruler,
            tool_met,
            tool_snap,
            tool_grid,
            lanes_area,
            lane_rows,
            header_rows,
        }
    }

    /// The cells across one header, REAPER's row at Tangent's density:
    /// number, arm dot, name, gain knob, slim meter, M/S.
    fn header_row(row: Rect, used: bool) -> HeaderRow {
        let r = row.shrink2(Vec2::new(3.0, 2.0));
        if !r.is_positive() {
            return HeaderRow { panel: row, ..HeaderRow::NONE };
        }
        // An unused lane is a place, not a channel: no controls to press.
        if !used {
            return HeaderRow { panel: row, ..HeaderRow::NONE };
        }
        let h = r.height();
        let cut = |a: f32, b: f32| {
            Rect::from_min_max(
                Pos2::new(r.left() + r.width() * a, r.top()),
                Pos2::new(r.left() + r.width() * b, r.bottom()),
            )
        };
        let square = |cell: Rect, s: f32| Rect::from_center_size(cell.center(), Vec2::splat(s));
        let number = cut(0.0, 0.08);
        let arm = square(cut(0.08, 0.18), (h * 0.42).clamp(9.0, 15.0));
        // The switches keep the right end; the meter stands inside them.
        let sw = (h * 0.44).clamp(11.0, 17.0);
        let solo = Rect::from_min_size(
            Pos2::new(r.right() - sw, r.center().y - sw * 0.5),
            Vec2::splat(sw),
        );
        let mute = Rect::from_min_size(
            Pos2::new(solo.left() - 2.0 - sw, r.center().y - sw * 0.5),
            Vec2::splat(sw),
        );
        let meter = Rect::from_min_max(
            Pos2::new(mute.left() - 8.0, r.top() + h * 0.15),
            Pos2::new(mute.left() - 3.0, r.bottom() - h * 0.15),
        );
        // **Two dials now — volume, then send — a shade smaller than the
        // one was**, each with a readout under it: the number is the other
        // half of a dial this size.
        let kd = (h * 0.52).clamp(12.0, 20.0);
        let dial_top = r.top() + h * 0.08;
        let send = Rect::from_min_size(
            Pos2::new(meter.left() - 5.0 - kd, dial_top),
            Vec2::splat(kd),
        );
        let knob = Rect::from_min_size(
            Pos2::new(send.left() - 4.0 - kd, dial_top),
            Vec2::splat(kd),
        );
        let readout = |dial: Rect| {
            Rect::from_min_max(
                Pos2::new(dial.left() - 1.0, dial.bottom() + 1.0),
                Pos2::new(dial.right() + 1.0, r.bottom() - 1.0),
            )
        };
        let name = Rect::from_min_max(
            Pos2::new(arm.right() + 4.0, r.top()),
            Pos2::new(knob.left() - 6.0, r.bottom()),
        );
        HeaderRow {
            panel: row,
            number,
            arm,
            name,
            knob,
            send,
            knob_readout: readout(knob),
            send_readout: readout(send),
            meter,
            mute,
            solo,
        }
    }
}

/// Song seconds at `x`, through the zoom window. The one mapping.
pub fn time_at(rect: Rect, view: &ArrangeView<'_>, x: f32) -> f64 {
    let l = Layout::new(rect, view);
    if !l.lanes_area.is_positive() || view.view_len_s <= 0.0 {
        return 0.0;
    }
    let t = f64::from((x - l.lanes_area.left()) / l.lanes_area.width());
    (view.view_start_s + t * view.view_len_s).max(0.0)
}

/// The x a song second draws at. `time_at`'s inverse.
pub fn x_of(rect: Rect, view: &ArrangeView<'_>, t: f64) -> f32 {
    let l = Layout::new(rect, view);
    if !l.lanes_area.is_positive() || view.view_len_s <= 0.0 {
        return l.lanes_area.left();
    }
    let f = ((t - view.view_start_s) / view.view_len_s) as f32;
    l.lanes_area.left() + f * l.lanes_area.width()
}

/// A clip's rectangle on its lane, clamped into the lanes area.
pub fn clip_rect(rect: Rect, view: &ArrangeView<'_>, clip: &ClipView<'_>) -> Rect {
    let l = Layout::new(rect, view);
    let Some(row) = l.lane_rows.get(clip.lane).copied().filter(Rect::is_positive) else {
        return Rect::NOTHING;
    };
    let x0 = x_of(rect, view, clip.start_s).max(l.lanes_area.left());
    let x1 = x_of(rect, view, clip.start_s + clip.len_s).min(l.lanes_area.right());
    if x1 <= x0 {
        return Rect::NOTHING;
    }
    Rect::from_min_max(
        Pos2::new(x0, row.top() + 1.5),
        Pos2::new(x1, row.bottom() - 1.5),
    )
}

/// What a press at `pos` means, or nothing.
///
/// Clips first, smallest target first — fade corner, then edge, then body —
/// then the ruler and the lane's empty room. Later clips win over earlier
/// ones where they overlap, because later is what was added on top.
pub fn hit_test(rect: Rect, view: &ArrangeView<'_>, pos: Pos2) -> Option<Hit> {
    let l = Layout::new(rect, view);
    for (r, hit) in [
        (l.tool_met, Hit::ToolMetronome),
        (l.tool_snap, Hit::ToolSnap),
        (l.tool_grid, Hit::ToolGrid),
    ] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    for (i, row) in l.header_rows.iter().enumerate() {
        for (r, hit) in [
            (row.arm, Hit::Arm(i)),
            (row.mute, Hit::Mute(i)),
            (row.solo, Hit::Solo(i)),
            (row.knob, Hit::Gain(i)),
            (row.send, Hit::Send(i)),
        ] {
            if r.is_positive() && r.contains(pos) {
                return Some(hit);
            }
        }
    }
    if l.ruler.is_positive() && l.ruler.contains(pos) {
        return Some(Hit::Ruler(time_at(rect, view, pos.x)));
    }
    for clip in view.clips.iter().rev() {
        let r = clip_rect(rect, view, clip);
        if !r.is_positive() || !r.contains(pos) {
            continue;
        }
        let fade = FADE_W.min(r.width() * 0.25);
        let fade_h = fade.min(r.height() * 0.4);
        let fade_in = Rect::from_min_size(r.min, Vec2::new(fade, fade_h));
        let fade_out = Rect::from_min_size(
            Pos2::new(r.right() - fade, r.top()),
            Vec2::new(fade, fade_h),
        );
        if fade_in.contains(pos) {
            return Some(Hit::FadeIn(clip.id));
        }
        if fade_out.contains(pos) {
            return Some(Hit::FadeOut(clip.id));
        }
        let edge = EDGE_W.min(r.width() * 0.2);
        if pos.x <= r.left() + edge {
            return Some(Hit::ClipLeft(clip.id));
        }
        if pos.x >= r.right() - edge {
            return Some(Hit::ClipRight(clip.id));
        }
        return Some(Hit::ClipBody(clip.id));
    }
    for (i, row) in l.lane_rows.iter().enumerate() {
        if row.is_positive() && row.contains(pos) {
            return Some(Hit::Lane(i, time_at(rect, view, pos.x)));
        }
    }
    None
}

// ── painting ───────────────────────────────────────────────────────────────

struct Ink {
    wood: Color32,
    face: Color32,
    engrave: Color32,
    faint: Color32,
    lane: Color32,
    lane_alt: Color32,
    grid: Color32,
    clip_ink: Color32,
    playhead: Color32,
    arm: Color32,
    mute: Color32,
    solo: Color32,
    meter: Color32,
    select: Color32,
}

fn ink(view: &ArrangeView<'_>) -> Ink {
    let (r, g, b) = view.wood;
    let wood = Color32::from_rgb(r, g, b);
    let face = if view.dark_mode {
        Color32::from_rgb(0xC9, 0xC0, 0xA6)
    } else {
        Color32::from_rgb(0xE3, 0xD9, 0xBD)
    };
    Ink {
        wood,
        face,
        engrave: Color32::from_rgb(0x24, 0x20, 0x1A),
        faint: face.gamma_multiply(0.4),
        // The lanes are a recessed field, the way the timeline's box is.
        lane: Color32::from_rgb(0x20, 0x1C, 0x16),
        lane_alt: Color32::from_rgb(0x25, 0x20, 0x19),
        grid: face.gamma_multiply(0.13),
        clip_ink: Color32::from_rgb(0xE8, 0xDC, 0xC0),
        playhead: Color32::from_rgb(0xE8, 0x3A, 0x4E),
        arm: Color32::from_rgb(0xE0, 0x22, 0x22),
        mute: Color32::from_rgb(0xE0, 0xA8, 0x22),
        solo: Color32::from_rgb(0xE8, 0x3A, 0x4E),
        meter: Color32::from_rgb(0x3D, 0xC0, 0x5A),
        select: Color32::from_rgb(0xFF, 0xF4, 0xE0),
    }
}

/// What the grid box SAYS: bars as a power of two — "8", "1", "1/16".
pub fn grid_label(exp: i8) -> String {
    if exp >= 0 {
        format!("{}", 1u32 << exp.min(6) as u32)
    } else {
        format!("1/{}", 1u32 << (-exp).min(6) as u32)
    }
}

/// Seconds per bar, from the band's own tempo and signature.
fn bar_s(view: &ArrangeView<'_>) -> f64 {
    let bpm = view.bpm.clamp(20.0, 400.0);
    let top = view.sig_top.clamp(1, 32) as f64;
    let bottom = view.sig_bottom.clamp(1, 32) as f64;
    top * (60.0 / bpm) * (4.0 / bottom)
}

/// Paint the whole view into `rect`.
pub fn draw(painter: &Painter, rect: Rect, view: &ArrangeView<'_>) {
    let l = Layout::new(rect, view);
    if !l.lanes_area.is_positive() {
        return;
    }
    let p = ink(view);
    painter.rect_filled(rect, 0.0, p.wood);

    // ── the lanes' field, striped so eight rows read as eight ─────────────
    for (i, row) in l.lane_rows.iter().enumerate() {
        painter.rect_filled(*row, 0.0, if i % 2 == 0 { p.lane } else { p.lane_alt });
    }

    // ── the grid: bar lines through the lanes, numbers on the ruler ───────
    let bar = bar_s(view);
    let px_per_s = f64::from(l.lanes_area.width()) / view.view_len_s.max(1e-9);
    // The step that keeps labels a hand apart: 1, 2, 4, 8... bars.
    let mut step = 1u64;
    while (bar * step as f64) * px_per_s < 64.0 && step < 1 << 20 {
        step *= 2;
    }
    painter.rect_filled(l.ruler, 0.0, p.wood);
    let first = (view.view_start_s / (bar * step as f64)).floor().max(0.0) as u64;
    let last = ((view.view_start_s + view.view_len_s) / (bar * step as f64)).ceil() as u64;
    let font = FontId::new((l.ruler.height() * 0.55).clamp(8.0, 12.0), crate::fonts::courier());
    for n in first..=last {
        let t = n as f64 * bar * step as f64;
        let x = x_of(rect, view, t);
        if x < l.lanes_area.left() - 0.5 || x > l.lanes_area.right() + 0.5 {
            continue;
        }
        painter.line_segment(
            [
                Pos2::new(x, l.lanes_area.top()),
                Pos2::new(x, l.lanes_area.bottom()),
            ],
            Stroke::new(1.0, p.grid),
        );
        painter.line_segment(
            [
                Pos2::new(x, l.ruler.bottom() - 4.0),
                Pos2::new(x, l.ruler.bottom()),
            ],
            Stroke::new(1.0, p.faint),
        );
        // Bars count from ONE, the way musicians do.
        painter.text(
            Pos2::new(x + 3.0, l.ruler.top() + 1.0),
            Align2::LEFT_TOP,
            format!("{}", n * step + 1),
            font.clone(),
            p.faint,
        );
    }

    // ── the clips ─────────────────────────────────────────────────────────
    for clip in view.clips {
        let r = clip_rect(rect, view, clip);
        if !r.is_positive() {
            continue;
        }
        let color = view
            .lanes
            .get(clip.lane)
            .map(|lane| lane.color)
            .filter(|c| *c != 0)
            .map(|c| {
                let (cr, cg, cb) = STRIP_COLORS[c.min(STRIP_COLORS.len() - 1)];
                Color32::from_rgb(cr, cg, cb)
            })
            // An unpainted lane's clips are the desk's green — the timeline
            // wave's own colour, so the two surfaces agree what audio looks
            // like.
            .unwrap_or(Color32::from_rgb(0x2E, 0x6B, 0x46));
        painter.rect_filled(r, 3.0, color);
        // A MIDI item wears a dashed midline where audio wears its wave —
        // the empty staff the editor will fill.
        if clip.midi && r.width() >= 8.0 {
            let mid = r.center().y;
            let mut x = r.left() + 3.0;
            while x < r.right() - 3.0 {
                painter.line_segment(
                    [Pos2::new(x, mid), Pos2::new((x + 4.0).min(r.right() - 3.0), mid)],
                    Stroke::new(1.0, p.clip_ink.gamma_multiply(0.5)),
                );
                x += 8.0;
            }
        }
        // The wave, drawn inside at the clip's own resolution.
        if !clip.wave.is_empty() && r.width() >= 8.0 {
            let mid = r.center().y;
            let half = r.height() * 0.38;
            let cols = r.width() as usize;
            for i in 0..cols {
                let f = i as f32 / cols as f32;
                let v = clip.wave[(f * clip.wave.len() as f32) as usize % clip.wave.len()];
                let x = r.left() + i as f32;
                let h = (v * half).max(0.5);
                painter.line_segment(
                    [Pos2::new(x, mid - h), Pos2::new(x, mid + h)],
                    Stroke::new(1.0, p.clip_ink.gamma_multiply(0.55)),
                );
            }
        }
        // The fades: translucent wedges from the top corners, their feet at
        // the fade's end — the picture IS the envelope.
        let veil = Color32::from_black_alpha(110);
        if clip.fade_in_s > 0.0 {
            let fx = x_of(rect, view, clip.start_s + clip.fade_in_s).clamp(r.left(), r.right());
            painter.add(egui::Shape::convex_polygon(
                vec![r.left_top(), Pos2::new(fx, r.top()), Pos2::new(r.left(), r.bottom())],
                veil,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(r.left(), r.bottom()), Pos2::new(fx, r.top())],
                Stroke::new(1.0, p.clip_ink),
            );
        }
        if clip.fade_out_s > 0.0 {
            let fx = x_of(rect, view, clip.start_s + clip.len_s - clip.fade_out_s)
                .clamp(r.left(), r.right());
            painter.add(egui::Shape::convex_polygon(
                vec![Pos2::new(fx, r.top()), r.right_top(), Pos2::new(r.right(), r.bottom())],
                veil,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(fx, r.top()), Pos2::new(r.right(), r.bottom())],
                Stroke::new(1.0, p.clip_ink),
            );
        }
        // The fade HANDLES, only at a size where they are grabbable.
        if r.width() >= FADE_W * 3.0 {
            for x in [r.left() + FADE_W * 0.5, r.right() - FADE_W * 0.5] {
                painter.circle_filled(Pos2::new(x, r.top() + FADE_W * 0.4), 2.0, p.clip_ink);
            }
        }
        // The name, cut to the clip.
        if r.width() >= 30.0 {
            let font = FontId::new((r.height() * 0.22).clamp(8.0, 11.0), crate::fonts::courier_bold());
            painter.text(
                Pos2::new(r.left() + FADE_W + 2.0, r.top() + 2.0),
                Align2::LEFT_TOP,
                cut_to(painter, r.width() - FADE_W * 2.0 - 4.0, clip.name, &font),
                font,
                p.clip_ink,
            );
        }
        painter.rect_stroke(
            r,
            3.0,
            if clip.selected {
                Stroke::new(2.0, p.select)
            } else {
                Stroke::new(1.0, Color32::from_black_alpha(140))
            },
            egui::StrokeKind::Inside,
        );
    }

    // ── the playhead, through the lanes and the ruler ─────────────────────
    let px = x_of(rect, view, view.playhead_s);
    if px >= l.lanes_area.left() - 0.5 && px <= l.lanes_area.right() + 0.5 {
        painter.line_segment(
            [Pos2::new(px, l.ruler.top()), Pos2::new(px, l.lanes_area.bottom())],
            Stroke::new(1.5, p.playhead),
        );
        painter.add(egui::Shape::convex_polygon(
            vec![
                Pos2::new(px - 4.0, l.ruler.top()),
                Pos2::new(px + 4.0, l.ruler.top()),
                Pos2::new(px, l.ruler.top() + 6.0),
            ],
            p.playhead,
            Stroke::NONE,
        ));
    }

    // ── the headers, over their own wood ──────────────────────────────────
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(l.header.right(), rect.bottom())),
        0.0,
        p.wood,
    );
    painter.line_segment(
        [
            Pos2::new(l.header.right(), rect.top()),
            Pos2::new(l.header.right(), rect.bottom()),
        ],
        Stroke::new(1.0, p.engrave.gamma_multiply(0.5)),
    );
    let num_font = FontId::new(10.0, crate::fonts::courier());
    for (i, row) in l.header_rows.iter().enumerate() {
        let Some(lane) = view.lanes.get(i) else { continue };
        // The strip's own colour as a plate behind the header, the mixer's
        // rule: any colour, and the text still reads.
        let face = if lane.color != 0 {
            let (cr, cg, cb) = STRIP_COLORS[lane.color.min(STRIP_COLORS.len() - 1)];
            Color32::from_rgb(cr, cg, cb)
        } else {
            p.face
        };
        let plate = row.panel.shrink2(Vec2::new(2.0, 1.5));
        if !plate.is_positive() {
            continue;
        }
        if lane.used {
            painter.rect_filled(plate, 2.0, face);
        } else {
            painter.rect_stroke(
                plate,
                2.0,
                Stroke::new(1.0, p.face.gamma_multiply(0.3)),
                egui::StrokeKind::Inside,
            );
        }
        let ink_on = if luma(face) > 140.0 { p.engrave } else { p.face };
        painter.text(
            Pos2::new(plate.left() + 4.0, plate.center().y),
            Align2::LEFT_CENTER,
            format!("{}", i + 1),
            num_font.clone(),
            if lane.used { ink_on.gamma_multiply(0.7) } else { p.faint },
        );
        if !lane.used {
            continue;
        }
        // The arm dot: REAPER's record circle, lit when live.
        let c = row.arm.center();
        let rad = row.arm.width() * 0.5;
        if lane.armed {
            painter.circle_filled(c, rad, p.arm);
        }
        painter.circle_stroke(c, rad, Stroke::new(1.5, ink_on.gamma_multiply(0.8)));
        // The name, on its plate.
        if row.name.is_positive() {
            let font = FontId::new(
                (row.name.height() * 0.32).clamp(8.5, 12.0),
                crate::fonts::courier_bold(),
            );
            painter.text(
                Pos2::new(row.name.left() + 2.0, row.name.center().y),
                Align2::LEFT_CENTER,
                cut_to(painter, row.name.width() - 4.0, lane.name, &font),
                font,
                ink_on,
            );
        }
        // The two dials: VOLUME (the mixer fader as a dial) and SEND, each
        // with its number under it — a dial this small needs the readout to
        // be a control you can trust.
        let dial = |painter: &Painter, cell: Rect, t: f32, cap: Color32| {
            if !cell.is_positive() {
                return;
            }
            let kc = cell.center();
            let kr = cell.width() * 0.5;
            painter.circle_filled(kc, kr, p.lane);
            painter.circle_filled(kc, kr * 0.82, cap);
            let sweep = std::f32::consts::PI * 1.5;
            let a = std::f32::consts::PI * 0.75 + sweep * t.clamp(0.0, 1.0);
            painter.line_segment(
                [kc, Pos2::new(kc.x + a.cos() * kr * 0.8, kc.y + a.sin() * kr * 0.8)],
                Stroke::new((kr * 0.22).max(1.5), p.clip_ink),
            );
        };
        let readout = |painter: &Painter, cell: Rect, text: String| {
            if !cell.is_positive() || cell.height() < 7.0 {
                return;
            }
            painter.text(
                cell.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::new(cell.height().clamp(7.0, 9.0), crate::fonts::courier()),
                ink_on.gamma_multiply(0.8),
            );
        };
        dial(
            painter,
            row.knob,
            crate::recorder::gain_to_fader(lane.gain),
            Color32::from_rgb(0x1C, 0x6F, 0xD6),
        );
        // Compact: the unit has no room at this size, and a number under a
        // VOLUME dial needs none.
        let db = if lane.gain <= 0.001 {
            "-inf".to_owned()
        } else {
            format!("{:+.1}", 20.0 * lane.gain.log10())
        };
        readout(painter, row.knob_readout, db);
        dial(
            painter,
            row.send,
            lane.send,
            Color32::from_rgb(0x2f, 0x8f, 0x5c),
        );
        readout(
            painter,
            row.send_readout,
            format!("{:.0}%", f64::from(lane.send.clamp(0.0, 1.0)) * 100.0),
        );
        // The meter: one slim bar, the loudest of the pair.
        if row.meter.is_positive() {
            painter.rect_filled(row.meter, 1.0, p.lane);
            let v = lane.peak[0].max(lane.peak[1]);
            if v > 0.0 {
                let t = crate::recorder::gain_to_fader(v).clamp(0.0, 1.0);
                painter.rect_filled(
                    Rect::from_min_max(
                        Pos2::new(row.meter.left(), row.meter.bottom() - row.meter.height() * t),
                        row.meter.max,
                    ),
                    1.0,
                    p.meter,
                );
            }
        }
        for (r, label, on, lit) in [
            (row.mute, "M", lane.muted, p.mute),
            (row.solo, "S", lane.soloed, p.solo),
        ] {
            if !r.is_positive() {
                continue;
            }
            painter.rect_filled(r, 2.0, if on { lit } else { p.lane });
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new((r.height() * 0.6).clamp(8.0, 12.0), crate::fonts::courier_bold()),
                if on { p.engrave } else { p.face.gamma_multiply(0.75) },
            );
        }
    }
    // **A MIDI item being drawn right now**: a ghost until the button comes
    // up and the document takes it.
    if let Some((lane, t0, t1)) = view.pending {
        if let Some(row) = l.lane_rows.get(lane).copied().filter(|r| r.is_positive()) {
            let x0 = x_of(rect, view, t0).max(l.lanes_area.left());
            let x1 = x_of(rect, view, t1).min(l.lanes_area.right());
            if x1 > x0 {
                let ghost = Rect::from_min_max(
                    Pos2::new(x0, row.top() + 1.5),
                    Pos2::new(x1, row.bottom() - 1.5),
                );
                painter.rect_filled(ghost, 3.0, Color32::from_white_alpha(24));
                painter.rect_stroke(
                    ghost,
                    3.0,
                    Stroke::new(1.5, p.select),
                    egui::StrokeKind::Inside,
                );
            }
        }
    }

    // The corner cell over the header, where the ruler has no numbers —
    // and the TOOLBAR standing in it: the click, the magnet, the grid.
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(l.header.right(), l.ruler.bottom())),
        0.0,
        p.wood,
    );
    let tool = |painter: &Painter, r: Rect, on: bool| {
        if !r.is_positive() {
            return false;
        }
        painter.rect_filled(r, 2.0, if on { p.face } else { p.lane });
        true
    };
    if tool(painter, l.tool_met, view.metronome_on) {
        crate::recorder_panel::draw_metronome_on(
            painter,
            l.tool_met.shrink(2.0),
            if view.metronome_on { p.engrave } else { p.face.gamma_multiply(0.7) },
            if view.metronome_on { p.face } else { p.lane },
        );
    }
    if tool(painter, l.tool_snap, view.snap_on) {
        // The magnet: a U with its two poles, the sign every DAW uses.
        let m = l.tool_snap.shrink(3.5);
        let ink = if view.snap_on { p.engrave } else { p.face.gamma_multiply(0.7) };
        let w = (m.width() * 0.18).clamp(1.5, 3.0);
        let cx = m.center().x;
        let radius = m.width() * 0.5 - w * 0.5;
        // The bend, drawn as an arc of segments over the top.
        let steps = 10;
        let mut last: Option<Pos2> = None;
        for i in 0..=steps {
            let a = std::f32::consts::PI * (1.0 + i as f32 / steps as f32);
            let pt = Pos2::new(
                cx + a.cos() * radius,
                m.top() + radius + a.sin() * radius,
            );
            if let Some(prev) = last {
                painter.line_segment([prev, pt], Stroke::new(w, ink));
            }
            last = Some(pt);
        }
        // The legs and their pole tips.
        for side in [-1.0f32, 1.0] {
            let x = cx + side * radius;
            painter.line_segment(
                [
                    Pos2::new(x, m.top() + radius),
                    Pos2::new(x, m.bottom() - m.height() * 0.22),
                ],
                Stroke::new(w, ink),
            );
            painter.line_segment(
                [
                    Pos2::new(x, m.bottom() - m.height() * 0.2),
                    Pos2::new(x, m.bottom()),
                ],
                Stroke::new(w, if view.snap_on { p.solo } else { ink }),
            );
        }
    }
    if tool(painter, l.tool_grid, true) {
        painter.text(
            l.tool_grid.center(),
            Align2::CENTER_CENTER,
            grid_label(view.grid_exp),
            FontId::new(
                (l.tool_grid.height() * 0.55).clamp(8.0, 11.0),
                crate::fonts::courier_bold(),
            ),
            p.engrave,
        );
    }
}

fn luma(c: Color32) -> f32 {
    0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
}

fn cut_to(painter: &Painter, width: f32, text: &str, font: &FontId) -> String {
    let measure = |t: &str| {
        painter
            .layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE)
            .rect
            .width()
    };
    if measure(text) <= width {
        return text.to_owned();
    }
    let mut keep = text.len();
    while keep > 0 {
        keep -= 1;
        while keep > 0 && !text.is_char_boundary(keep) {
            keep -= 1;
        }
        let cut = format!("{}\u{2026}", text[..keep].trim_end());
        if measure(&cut) <= width {
            return cut;
        }
    }
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a_lane(used: bool) -> LaneView<'static> {
        LaneView {
            name: "KEYS",
            color: 0,
            kind: used.then_some(ChannelKind::Midi),
            used,
            armed: false,
            muted: false,
            soloed: false,
            gain: 1.0,
            send: 0.0,
            peak: [0.0; 2],
        }
    }

    fn clips() -> Vec<ClipView<'static>> {
        vec![
            ClipView {
                id: 1,
                lane: 0,
                start_s: 10.0,
                len_s: 30.0,
                max_len_s: 100.0,
                fade_in_s: 2.0,
                fade_out_s: 3.0,
                name: "backing",
                wave: &[],
                selected: false,
                midi: false,
            },
            ClipView {
                id: 2,
                lane: 2,
                start_s: 50.0,
                len_s: 20.0,
                max_len_s: f64::INFINITY,
                fade_in_s: 0.0,
                fade_out_s: 0.0,
                name: "solo",
                wave: &[],
                selected: true,
                midi: false,
            },
        ]
    }

    fn a_view<'a>(clips: &'a [ClipView<'a>]) -> ArrangeView<'a> {
        ArrangeView {
            lanes: std::array::from_fn(|i| a_lane(i < 3)),
            clips,
            metronome_on: true,
            snap_on: true,
            grid_exp: 0,
            pending: None,
            view_start_s: 0.0,
            view_len_s: 120.0,
            playhead_s: 15.0,
            bpm: 120.0,
            sig_top: 4,
            sig_bottom: 4,
            dark_mode: true,
            wood: (0x4A, 0x3B, 0x2C),
        }
    }

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 420.0))
    }

    /// The time mapping round-trips, which is what keeps a clip drawn where
    /// it is grabbed.
    #[test]
    fn the_time_axis_round_trips() {
        let c = clips();
        let v = a_view(&c);
        let r = rect();
        for t in [0.0, 10.0, 59.7, 120.0] {
            let x = x_of(r, &v, t);
            let back = time_at(r, &v, x);
            assert!((back - t).abs() < 0.05, "{t} came back as {back}");
        }
        // And zoomed in, the same promise.
        let mut z = a_view(&c);
        z.view_start_s = 40.0;
        z.view_len_s = 10.0;
        let x = x_of(r, &z, 45.0);
        assert!((time_at(r, &z, x) - 45.0).abs() < 0.01);
    }

    /// **A clip is grabbed where it is drawn**, and its parts hit in the
    /// right priority: fade corners, then edges, then body.
    #[test]
    fn a_clip_hits_by_part() {
        let c = clips();
        let v = a_view(&c);
        let r = rect();
        let cr = clip_rect(r, &v, &c[0]);
        assert!(cr.is_positive(), "the clip has no rectangle");
        assert_eq!(
            hit_test(r, &v, cr.center()),
            Some(Hit::ClipBody(1)),
            "the body does not grab"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(cr.left() + 2.0, cr.center().y)),
            Some(Hit::ClipLeft(1)),
            "the left edge does not trim"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(cr.right() - 2.0, cr.center().y)),
            Some(Hit::ClipRight(1)),
            "the right edge does not trim"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(cr.left() + 2.0, cr.top() + 2.0)),
            Some(Hit::FadeIn(1)),
            "the top-left corner is not the fade"
        );
        assert_eq!(
            hit_test(r, &v, Pos2::new(cr.right() - 2.0, cr.top() + 2.0)),
            Some(Hit::FadeOut(1)),
            "the top-right corner is not the fade"
        );
    }

    /// Empty lane space is a locate that names its lane and second; the
    /// ruler is a locate too.
    #[test]
    fn empty_space_locates() {
        let c = clips();
        let v = a_view(&c);
        let r = rect();
        let l = Layout::new(r, &v);
        let row = l.lane_rows[5];
        let x = x_of(r, &v, 60.0);
        match hit_test(r, &v, Pos2::new(x, row.center().y)) {
            Some(Hit::Lane(5, t)) => assert!((t - 60.0).abs() < 0.2, "lane read {t}"),
            other => panic!("empty lane answered {other:?}"),
        }
        match hit_test(r, &v, Pos2::new(x, l.ruler.center().y)) {
            Some(Hit::Ruler(t)) => assert!((t - 60.0).abs() < 0.2, "ruler read {t}"),
            other => panic!("the ruler answered {other:?}"),
        }
    }

    /// **A used lane's header offers its controls; an unused lane offers
    /// nothing.** The mixer's own bargain, at header scale.
    #[test]
    fn headers_offer_controls_only_where_there_is_a_channel() {
        let c = clips();
        let v = a_view(&c);
        let r = rect();
        let l = Layout::new(r, &v);
        for i in 0..3 {
            let row = l.header_rows[i];
            for (cell, want) in [
                (row.arm, Hit::Arm(i)),
                (row.mute, Hit::Mute(i)),
                (row.solo, Hit::Solo(i)),
                (row.knob, Hit::Gain(i)),
            ] {
                assert!(cell.is_positive(), "lane {i} lost a control");
                assert_eq!(hit_test(r, &v, cell.center()), Some(want));
            }
        }
        let row = l.header_rows[6];
        assert!(!row.arm.is_positive(), "an unused lane grew an arm");
        assert!(!row.knob.is_positive(), "an unused lane grew a knob");
    }

    /// No header control collides with another, on any lane.
    #[test]
    fn no_two_header_controls_overlap() {
        let c = clips();
        let v = a_view(&c);
        let l = Layout::new(rect(), &v);
        for (i, row) in l.header_rows.iter().enumerate() {
            let cells = [
                row.number,
                row.arm,
                row.name,
                row.knob,
                row.send,
                row.knob_readout,
                row.send_readout,
                row.meter,
                row.mute,
                row.solo,
            ];
            for (a, ra) in cells.iter().enumerate() {
                for (b, rb) in cells.iter().enumerate().skip(a + 1) {
                    if ra.is_positive() && rb.is_positive() {
                        assert!(
                            !ra.intersects(*rb),
                            "lane {i}: header cells {a} and {b} overlap"
                        );
                    }
                }
            }
        }
    }

    /// **The toolbar and the second dial can be pressed**, and the gridbox
    /// says its bars the way a musician does.
    #[test]
    fn the_toolbar_and_the_send_dial_can_be_pressed() {
        let c = clips();
        let v = a_view(&c);
        let r = rect();
        let l = Layout::new(r, &v);
        for (cell, want) in [
            (l.tool_met, Hit::ToolMetronome),
            (l.tool_snap, Hit::ToolSnap),
            (l.tool_grid, Hit::ToolGrid),
        ] {
            assert!(cell.is_positive(), "{want:?} has no room in the corner");
            assert_eq!(hit_test(r, &v, cell.center()), Some(want));
        }
        let row = l.header_rows[0];
        assert_eq!(hit_test(r, &v, row.knob.center()), Some(Hit::Gain(0)));
        assert_eq!(hit_test(r, &v, row.send.center()), Some(Hit::Send(0)));
        assert!(row.knob_readout.is_positive(), "the volume lost its readout");
        assert!(row.send_readout.is_positive(), "the send lost its readout");
        assert_eq!(grid_label(0), "1");
        assert_eq!(grid_label(3), "8");
        assert_eq!(grid_label(-4), "1/16");
        assert_eq!(grid_label(-5), "1/32");
    }

    /// It survives a rect far too small to be useful, and clips off-window
    /// simply have no rectangle.
    #[test]
    fn it_survives_small_rects_and_offscreen_clips() {
        let c = clips();
        for size in [
            Vec2::new(1300.0, 420.0),
            Vec2::new(300.0, 100.0),
            Vec2::new(10.0, 10.0),
            Vec2::ZERO,
        ] {
            let v = a_view(&c);
            let r = Rect::from_min_size(Pos2::new(3.0, 7.0), size);
            let _ = Layout::new(r, &v);
            let _ = hit_test(r, &v, r.center());
        }
        // A clip entirely out of the window has no rect and takes no press.
        let mut v = a_view(&c);
        v.view_start_s = 200.0;
        v.view_len_s = 50.0;
        assert_eq!(clip_rect(rect(), &v, &c[0]), Rect::NOTHING);
    }
}
