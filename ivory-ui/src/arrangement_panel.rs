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
    /// What the input chip says: "IN 3", "MIDI: ALL", or "" for none.
    pub input: String,
    /// Index into [`STRIP_COLORS`]. Zero is the desk's own wood.
    pub color: usize,
    /// What feeds the lane live: keys, a microphone, or nothing. A lane with
    /// clips and no kind is a CLIP lane and wears the waveform.
    pub kind: Option<ChannelKind>,
    /// Whether the lane is anything at all — a kind, or clips standing on
    /// it. An unused lane draws faint and offers no header controls.
    pub used: bool,
    pub armed: bool,
    /// Whether the SOURCE is stereo — two meter bars — or mono, one. The
    /// mixer strip's own claim, made here too.
    pub stereo: bool,
    /// The three bays' short names — empty for an empty slot.
    pub inserts: [&'a str; crate::recorder::INSERTS],
    /// How many routes leave this lane, for the ROUTING chip.
    pub route_count: usize,
    pub muted: bool,
    /// This lane's automation curve is showing, in a row of its own under it.
    pub automation_open: bool,
    pub soloed: bool,
    /// Linear, as the mixer fader sits. The header knob is the same value.
    pub gain: f32,
    /// 0..=1, the strip's send — the second dial.
    pub send: f32,
    /// -1..1, the strip's pan — the third.
    pub pan: f32,
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
    /// The clip's notes, source-relative like the document's. Drawn as a
    /// miniature where audio draws its wave; empty draws the dashed staff.
    pub notes: &'a [crate::arrangement::ArrNote],
    /// The left trim, for placing the miniature under the drawn span.
    pub offset_s: f64,
    pub selected: bool,
    /// A MIDI item: no file, no wave — drawn with its own face.
    pub midi: bool,
    /// Being RECORDED right now: drawn hot, and no gesture lands on it.
    pub recording: bool,
}

/// Everything the arrangement draws, pushed in.
pub struct ArrangeView<'a> {
    pub lanes: [LaneView<'a>; CHANNELS],
    /// The first lane shown — the arrangement has more lanes than fit, so a
    /// window of them shows and this says which. Clamped by the app.
    pub lane_scroll: usize,
    /// How many lanes show at once — Shift+wheel over the headers zooms it.
    pub visible_lanes: usize,
    pub clips: &'a [ClipView<'a>],
    /// The toolbar's three facts: the click, the magnet, and the grid as a
    /// power of two in bars (3 = 8 bars … 0 = 1 bar … -5 = 1/32).
    pub metronome_on: bool,
    pub snap_on: bool,
    pub grid_exp: i8,
    /// The header resize strip is under the pointer (or being dragged):
    /// it lights, and the app swaps the cursor to the resize arrows.
    pub grip_lit: bool,
    /// A file is being dragged over this lane right now: it lights, so the
    /// drop lands somewhere the eye already picked.
    pub drop_lane: Option<usize>,
    /// A MIDI item being drawn by shift-drag right now: lane, from, to —
    /// ghosted until the button comes up and the document takes it.
    pub pending: Option<(usize, f64, f64)>,
    /// The tempo and meter map get a lane of their own, above the tracks.
    pub show_tempo_lane: bool,
    /// The zoom window, in seconds of song.
    pub view_start_s: f64,
    pub view_len_s: f64,
    /// The whole song's length (or the window's end past it): the nav
    /// box's outer rectangle, and never shorter than the window.
    pub song_s: f64,
    pub playhead_s: f64,
    /// The ruler's grid: bars from the band's own tempo and signature.
    pub bpm: f64,
    pub sig_top: u32,
    pub sig_bottom: u32,
    pub dark_mode: bool,
    /// The header column's width in points, when somebody has dragged the
    /// grip. `None` is the automatic share.
    pub header_w: Option<f32>,
    /// A lane NAME being typed, and what is in the field so far.
    pub naming: Option<(usize, &'a str)>,
    /// The time selection, drawn over lanes and ruler.
    pub time_sel: Option<(f64, f64)>,
    /// The selected channel: its header lights slightly.
    pub selected_channel: Option<usize>,
    /// The band's own wood, so the two surfaces are one instrument.
    pub wood: (u8, u8, u8),
    /// The strip face — the same theme role the mixer's bays wear, so a
    /// header in one view and a strip in the other are one surface.
    pub fx_strip: (u8, u8, u8),
    /// The WELL: the theme's own dark canvas, which the lane field derives
    /// from. The arrangement used to carry two hard-coded wardrobes (warm
    /// browns dark, greys light) that no theme could reach, which is why it
    /// was the one view that never changed with the rest of the app.
    pub meter_bg: (u8, u8, u8),
}

/// What a press on the arrangement means.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Hit {
    /// The NUMBER cell: the reorder handle — drag a track to another row.
    HdrNumber(usize),
    /// One of a track's three insert BAYS — the mixer's own door, here.
    HdrBay(usize, usize),
    /// The Routing button under a lane's dials: open the routing window.
    HdrRouting(usize),
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
    /// The header's pan dial — third of the set, every channel carries one.
    Pan(usize),
    /// The toolbar: the click on and off.
    ToolMetronome,
    /// The toolbar: the magnet.
    ToolSnap,
    /// The toolbar's grid box. A press steps FINER; the wheel over it is
    /// the real control, handled by the app from this same rectangle.
    ToolGrid,
    /// The navigator beside it: drag scrolls, vertical zooms, both at once.
    ToolNav,
    /// The header's INPUT chip: open the channel input menu.
    HdrInput(usize),
    /// The header's NAME: a double-click renames the lane.
    HdrName(usize),
    /// The seam between the headers and the lanes: drag to resize the
    /// header column.
    HeaderGrip,
    /// The circled plus on an EMPTY lane's header: add a bare track here.
    HdrAdd(usize),
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
const HEADER_W: (f32, f32, f32) = (0.16, 150.0, 220.0);

/// How tall the tempo lane is against a track lane.
///
/// Shorter, because it carries one curve and a handful of markers where a
/// track carries clips with names, waveforms and fades in them. Tall enough
/// that a tempo point is something you can aim at.
const TEMPO_LANE_SHARE: f32 = 0.55;

/// The same for an automation sub-lane, and the same reasoning: a curve
/// needs less room than the clips it belongs under, and the track it hangs
/// off must not shrink to make space for it.
const AUTO_LANE_SHARE: f32 = 0.55;

/// The ruler's height in points. Short — it is a scale, not a band.
const RULER_H: (f32, f32, f32) = (0.06, 16.0, 24.0);

/// How wide a clip's trim edge is, in points, and its fade corner square.
const EDGE_W: f32 = 7.0;
/// How many lanes the arrangement shows at once; the rest scroll vertically.
pub const VISIBLE_LANES: usize = 8;
const FADE_W: f32 = 11.0;

/// Where everything is. Fractions of the rect, derived once, read by both
/// the painter and the hit test.
/// What one row on the timeline IS.
///
/// **The arrangement had exactly one kind of row and the layout said so** —
/// `lane_rows: [Rect; CHANNELS]`, every row the same height, indexed by
/// channel. That is the shape a tempo lane and an automation sub-lane both
/// have to break, and if each broke it separately the second would rewrite
/// the first. So it breaks once, here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowKind {
    /// A channel's own lane, carrying its clips. `usize` is the channel.
    Track(usize),
    /// The tempo and meter map, above the tracks. One row, whatever the
    /// song's shape: tempo is a property of the SONG and not of a channel.
    Tempo,
    /// An automation curve under the track it belongs to.
    Automation(usize),
}

/// One row of the timeline, and where it is on both sides of the grip.
#[derive(Debug, Clone, Copy)]
pub struct Row {
    pub kind: RowKind,
    /// In `lanes_area` — where the clips or the curve are drawn.
    pub lane: Rect,
    /// In `header` — the name, the dials, the buttons.
    pub header: Rect,
}

pub struct Layout {
    pub header: Rect,
    pub ruler: Rect,
    /// The corner cell's toolbar: metronome, magnet, grid box.
    pub tool_met: Rect,
    pub tool_snap: Rect,
    pub tool_grid: Rect,
    /// The little navigator: outer = the whole song, inner = the window.
    /// Drag it to scroll; drag UP to zoom in at the same time.
    pub tool_nav: Rect,
    pub lanes_area: Rect,
    /// The draggable seam between headers and lanes.
    pub grip: Rect,
    /// **Every row, in drawn order — the source of truth.**
    ///
    /// `lane_rows` and `header_rows` below are PROJECTIONS of this, kept
    /// because eight call sites ask "where is channel `i`'s lane" and that is
    /// a fair question with a direct answer. They are derived in one place
    /// rather than computed twice, so the two cannot disagree about where a
    /// row is — which is the failure a parallel layout would have.
    pub rows: Vec<Row>,
    pub lane_rows: [Rect; CHANNELS],
    pub header_rows: [HeaderRow; CHANNELS],
}

impl Layout {
    /// The row a point falls in, if any.
    pub fn row_at(&self, pos: Pos2) -> Option<Row> {
        self.rows
            .iter()
            .copied()
            .find(|r| r.lane.contains(pos) || r.header.contains(pos))
    }
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
    pub pan: Rect,
    /// The readouts under the two dials: dB under the volume, % under the
    /// send. Text, not targets — the dials above them are the controls.
    pub knob_readout: Rect,
    pub send_readout: Rect,
    pub pan_readout: Rect,
    pub meter: Rect,
    pub mute: Rect,
    pub solo: Rect,
    /// The input chip, under the name — every lane wears one now: binding
    /// an input is how a bare lane BECOMES a track.
    pub input: Rect,
    /// The three insert BAYS — the mixer's slots, reachable from here.
    pub bays: [Rect; crate::recorder::INSERTS],
    /// The Routing button, under the dials. `NOTHING` where they are.
    pub routing: Rect,
    /// The circled plus, on empty lanes alone.
    pub add: Rect,
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
        routing: Rect::NOTHING,
        send_readout: Rect::NOTHING,
        pan: Rect::NOTHING,
        pan_readout: Rect::NOTHING,
        meter: Rect::NOTHING,
        mute: Rect::NOTHING,
        solo: Rect::NOTHING,
        input: Rect::NOTHING,
        bays: [Rect::NOTHING; crate::recorder::INSERTS],
        add: Rect::NOTHING,
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
                tool_nav: Rect::NOTHING,
                lanes_area: Rect::NOTHING,
                grip: Rect::NOTHING,
                rows: Vec::new(),
                lane_rows: [Rect::NOTHING; CHANNELS],
                header_rows: [HeaderRow::NONE; CHANNELS],
            };
        }
        let (share, lo, hi) = HEADER_W;
        // Chained rather than clamp(): a rect narrower than the floor would
        // hand clamp a min above its max, which is a panic in a painter.
        // A dragged width wins over the automatic share, inside the same
        // sanity bounds.
        let header_w = view
            .header_w
            .unwrap_or(rect.width() * share)
            .max(lo)
            .min(hi.max(400.0))
            .min(rect.width() * 0.5);
        let (rshare, rlo, rhi) = RULER_H;
        let ruler_h = (rect.height() * rshare).clamp(rlo, rhi);
        let header = Rect::from_min_max(
            Pos2::new(rect.left(), rect.top() + ruler_h),
            Pos2::new(rect.left() + header_w, rect.bottom()),
        );
        // **The resize STRIP is the only thing between the tracks and the
        // window** — the owner checked REAPER's and struck the extra dead
        // gutter. The strip is a real strip, not a hairline: wood-faced,
        // hover-lit, its own cursor — and a motionless CLICK on it returns
        // the playhead to the start, which keeps the generous
        // back-to-bar-one target the gutter used to provide.
        let field_x = rect.left() + header_w + GRIP_W;
        let ruler = Rect::from_min_max(
            Pos2::new(field_x, rect.top()),
            Pos2::new(rect.right(), rect.top() + ruler_h),
        );
        let lanes_area = Rect::from_min_max(
            Pos2::new(field_x, rect.top() + ruler_h),
            rect.max,
        );
        // **Eight lanes show; the rest scroll.** The desk grew past what
        // fits at a legible height, so the height is fixed to `VISIBLE_LANES`
        // and `view.lane_scroll` chooses which window shows. A lane outside
        // the window is `NOTHING` — not drawn, not hit, and clips on it
        // resolve to nothing through `clip_rect`'s `is_positive` filter.
        let visible = view.visible_lanes.clamp(1, CHANNELS);
        let lane_h = lanes_area.height() / visible as f32;
        let first = view.lane_scroll.min(CHANNELS.saturating_sub(visible));

        // **The rows, stacked in order, and the arrays derived from them.**
        //
        // A track row is `lane_h` tall — the height every row used to be —
        // so a song with nothing expanded lays out exactly as it did before
        // rows existed. `the_layout_is_unchanged_when_nothing_is_expanded`
        // is what holds that true, and it is the assertion that matters: a
        // rewrite of the thing every clip is positioned against has to be a
        // no-op until somebody asks for the new part.
        //
        // A row that would hang off the bottom of the area is `NOTHING`, the
        // same as a scrolled-off lane always was — not drawn, not hit, and
        // clips on it resolve to nothing through `clip_rect`'s filter.
        let mut rows: Vec<Row> = Vec::with_capacity(CHANNELS + 1);
        let mut y = lanes_area.top();
        let mut push = |kind: RowKind, h: f32, y: &mut f32, rows: &mut Vec<Row>| {
            let bottom = *y + h;
            // Wholly inside, or not at all: a half-height lane at the bottom
            // edge is a control you cannot read and a clip you cannot aim at.
            let fits = bottom <= lanes_area.bottom() + 0.5 && h > 0.0;
            let cut = |left: f32, right: f32| {
                if fits {
                    Rect::from_min_max(Pos2::new(left, *y), Pos2::new(right, bottom))
                } else {
                    Rect::NOTHING
                }
            };
            rows.push(Row {
                kind,
                lane: cut(lanes_area.left(), lanes_area.right()),
                header: cut(header.left(), header.right()),
            });
            if fits {
                *y = bottom;
            }
        };
        // The tempo lane sits ABOVE the tracks when it is showing: it is a
        // property of the song, and a song-wide row below the channels would
        // scroll away from the ruler it belongs to.
        if view.show_tempo_lane {
            push(RowKind::Tempo, lane_h * TEMPO_LANE_SHARE, &mut y, &mut rows);
        }
        for i in first..CHANNELS {
            push(RowKind::Track(i), lane_h, &mut y, &mut rows);
            if view.lanes.get(i).is_some_and(|l| l.automation_open) {
                push(RowKind::Automation(i), lane_h * AUTO_LANE_SHARE, &mut y, &mut rows);
            }
        }

        let lane_of = |want: usize| {
            rows.iter()
                .find(|r| r.kind == RowKind::Track(want))
                .map_or(Rect::NOTHING, |r| r.lane)
        };
        let header_of = |want: usize| {
            rows.iter()
                .find(|r| r.kind == RowKind::Track(want))
                .map_or(Rect::NOTHING, |r| r.header)
        };
        let lane_rows = std::array::from_fn(lane_of);
        let header_rows = std::array::from_fn(|i| {
            let row = header_of(i);
            let Some(lane) = view.lanes.get(i).filter(|_| row.is_positive()) else {
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
        // The NAVIGATOR takes whatever the corner has left — longer than
        // the gridbox, as asked: a rectangle inside a rectangle, and a
        // hand on it steers the whole view.
        let tool_nav = Rect::from_min_max(
            Pos2::new(tool_grid.right() + 3.0, corner.top()),
            corner.max,
        )
        .intersect(corner);
        let tool_nav = if tool_nav.width() >= side * 1.5 {
            tool_nav
        } else {
            Rect::NOTHING
        };
        Self {
            header,
            ruler,
            tool_met,
            tool_snap,
            tool_grid,
            tool_nav,
            lanes_area,
            grip: Rect::from_min_max(
                Pos2::new(rect.left() + header_w, rect.top()),
                Pos2::new(rect.left() + header_w + GRIP_W, rect.bottom()),
            ),

            rows,
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
        // An unused lane is a place, not a channel — but it DOES wear the
        // input chip: binding an input is how a bare lane becomes a track.
        if !used {
            // The + circle first — pressing it makes the track — then the
            // chip beside it for binding straight away.
            let d = (r.height() * 0.44).clamp(16.0, 26.0);
            let add = Rect::from_center_size(
                Pos2::new(r.left() + r.width() * 0.10, r.center().y),
                Vec2::splat(d),
            );
            let ch_h = (r.height() * 0.30).clamp(12.0, 16.0);
            let input = Rect::from_min_size(
                Pos2::new(add.right() + 6.0, r.center().y - ch_h * 0.5),
                Vec2::new(r.width() * 0.40, ch_h),
            );
            return HeaderRow { panel: row, input, add, ..HeaderRow::NONE };
        }
        let h = r.height();
        let cut = |a: f32, b: f32| {
            Rect::from_min_max(
                Pos2::new(r.left() + r.width() * a, r.top()),
                Pos2::new(r.left() + r.width() * b, r.bottom()),
            )
        };
        let square = |cell: Rect, s: f32| Rect::from_center_size(cell.center(), Vec2::splat(s));
        // The number and the arm keep FIXED seats at the left — the
        // proportional cuts walked the dot into the track title whenever
        // the header widened.
        let number = Rect::from_min_max(r.min, Pos2::new(r.left() + 12.0, r.bottom()));
        let arm = {
            let d = (h * 0.42).clamp(9.0, 15.0);
            Rect::from_center_size(
                Pos2::new(number.right() + 4.0 + d * 0.5, r.center().y),
                Vec2::splat(d),
            )
        };
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
        let kd = (h * 0.46).clamp(11.0, 18.0);
        // The dial standard: the VALUE reads above the knob, the LABEL
        // under it — so the dials drop to make room for the numbers.
        let dial_top = r.top() + h * 0.26;
        let send = Rect::from_min_size(
            Pos2::new(meter.left() - 5.0 - kd, dial_top),
            Vec2::splat(kd),
        );
        let knob = Rect::from_min_size(
            Pos2::new(send.left() - 4.0 - kd, dial_top),
            Vec2::splat(kd),
        );
        // The PAN, third of the set — every representation of a channel
        // carries one, the owner's rule.
        let pan = Rect::from_min_size(
            Pos2::new(knob.left() - 4.0 - kd, dial_top),
            Vec2::splat(kd),
        );
        let readout = |dial: Rect| {
            Rect::from_min_max(
                Pos2::new(dial.left() - 1.0, r.top() + 1.0),
                Pos2::new(dial.right() + 1.0, dial.top() - 1.0),
            )
        };
        // **What a NARROW header sheds: the three dials.** The vertical
        // rule below drops rows; nothing dropped columns, and at the
        // 150pt width floor the fixed cells flanking the column left it
        // INVERTED — every used lane's name, input chip and bays simply
        // vanished, rename and both menus dead with them. The mixer
        // carries the same three dials, so they are the horizontal
        // sacrifice — never the NAME.
        const COL_MIN: f32 = 40.0;
        let dials_fit = pan.left() - 6.0 - (arm.right() + 4.0) >= COL_MIN;
        let col_right = if dials_fit { pan.left() } else { meter.left() };
        let (send, knob, pan) = if dials_fit {
            (send, knob, pan)
        } else {
            (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING)
        };
        // **ROUTING sits under the dials** — the owner's placement. It goes
        // with the dials rather than with the name column because it is the
        // same kind of thing they are: what this lane DOES with its signal,
        // not what it is called. So it shares their fate on a narrow header
        // and their room on a short one: no dials, no button, and no button
        // when the space under them would make it a sliver to aim at.
        // **Under the LABELS, not on them.** The dials print VOL / SND / PAN
        // in the band between their own bottom and the row's, so a button
        // pinned straight under the dial covered the words that say which
        // dial is which. It goes below that band, and only when the row is
        // tall enough to hold the labels AND a button worth aiming at.
        const DIAL_LABEL_H: f32 = 9.0;
        let routing_top = dial_top + kd + DIAL_LABEL_H;
        let routing = if dials_fit && r.bottom() - 1.0 - routing_top >= 11.0 {
            Rect::from_min_max(
                Pos2::new(pan.left(), routing_top),
                Pos2::new(send.right(), r.bottom() - 1.0),
            )
        } else {
            Rect::NOTHING
        };
        // **Top-down: LABEL, then INPUT, then the THREE BAYS** — the
        // owner's order. Each row takes the column's whole width, so the
        // input strip and the slots are both WIDER; a short window trades
        // legibility for it, and that trade was accepted out loud.
        let col = Rect::from_min_max(
            Pos2::new(arm.right() + 4.0, r.top()),
            Pos2::new(col_right - 6.0, r.bottom() - 1.0),
        );
        let slice = |a: f32, b: f32| {
            Rect::from_min_max(
                Pos2::new(col.left(), col.top() + col.height() * a),
                Pos2::new(col.right(), col.top() + col.height() * b),
            )
        };
        // **What a short header sheds, and in what order.** The lane count
        // is adjustable now (Shift+wheel), so a header can be as tall as a
        // third of the view or as short as a strip. When it cannot hold all
        // three stacked rows legibly, the owner's rule is: drop the INSERT
        // BAYS first, then the INPUT selector only if it is still too tight,
        // and never the NAME. Each surviving row takes an equal share.
        const ROW_MIN: f32 = 15.0;
        let rows_h = col.height();
        let (name, input, bays_row) = if rows_h >= 3.0 * ROW_MIN {
            (slice(0.0, 0.30), slice(0.34, 0.63), slice(0.67, 1.0))
        } else if rows_h >= 2.0 * ROW_MIN {
            // Bays gone; the name and the input split the column.
            (slice(0.0, 0.46), slice(0.50, 1.0), Rect::NOTHING)
        } else {
            // Only the name survives — the last thing a track cannot do without.
            (slice(0.0, 1.0), Rect::NOTHING, Rect::NOTHING)
        };
        let bays = std::array::from_fn(|b| {
            let w = (bays_row.width() - 3.0 * (crate::recorder::INSERTS as f32 - 1.0))
                / crate::recorder::INSERTS as f32;
            if w < 16.0 || !bays_row.is_positive() {
                return Rect::NOTHING;
            }
            let x0 = bays_row.left() + b as f32 * (w + 3.0);
            Rect::from_min_max(
                Pos2::new(x0, bays_row.top()),
                Pos2::new(x0 + w, bays_row.bottom()),
            )
        });
        HeaderRow {
            panel: row,
            number,
            arm,
            name,
            knob,
            send,
            pan,
            knob_readout: readout(knob),
            send_readout: readout(send),
            pan_readout: readout(pan),
            meter,
            mute,
            solo,
            input,
            bays,
            routing,
            add: Rect::NOTHING,
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
/// The clip's notes as a miniature: pitch spread across the clip's height
/// (padded to at least an octave so one note does not fill the box), each
/// note a small bar over its own span of the drawn length.
/// `full_x` is the clip's UNCLAMPED pixel span. The drawn rect is cut to
/// the viewport, and mapping time onto the CUT width compressed the
/// picture: a recording that crossed the right edge watched its own notes
/// slide leftward as the take grew — the owner's "clip begins shrinking".
/// Time maps onto the full span; each bar is then clipped to what shows.
fn draw_note_miniature(
    painter: &Painter,
    r: Rect,
    full_x: (f32, f32),
    clip: &ClipView<'_>,
    p: &Ink,
) {
    let len = clip.len_s.max(1.0e-9);
    let (mut lo, mut hi) = (127u8, 0u8);
    for n in clip.notes {
        lo = lo.min(n.key);
        hi = hi.max(n.key);
    }
    if lo > hi {
        return;
    }
    // At least an octave of spread, centred on what was played.
    while u16::from(hi) - u16::from(lo) < 12 {
        lo = lo.saturating_sub(1);
        hi = (hi + 1).min(127);
        if lo == 0 && hi == 127 {
            break;
        }
    }
    let span = f32::from(hi - lo).max(1.0);
    let inner = r.shrink(2.0);
    if !inner.is_positive() {
        return;
    }
    let bar_h = (inner.height() / span).clamp(1.5, 4.0);
    let (fx0, fx1) = full_x;
    let fw = (fx1 - fx0).max(1.0);
    for n in clip.notes {
        let a = (n.at_s - clip.offset_s) / len;
        let b = (n.at_s + n.dur_s.max(0.02) - clip.offset_s) / len;
        if b <= 0.0 || a >= 1.0 {
            continue;
        }
        let x0 = (fx0 + fw * (a.max(0.0) as f32)).max(inner.left());
        let x1 = (fx0 + fw * (b.min(1.0) as f32)).min(inner.right());
        if x1 <= x0 {
            continue;
        }
        let f = 1.0 - (f32::from(n.key.clamp(lo, hi) - lo) / span);
        let y = inner.top() + (inner.height() - bar_h) * f;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.5), y + bar_h)),
            1.0,
            p.clip_ink.gamma_multiply(0.8),
        );
    }
}

/// The resize strip's width — REAPER's splitter, hover-lit, its own cursor.
const GRIP_W: f32 = 7.0;

pub fn hit_test(rect: Rect, view: &ArrangeView<'_>, pos: Pos2) -> Option<Hit> {
    let l = Layout::new(rect, view);

    for (r, hit) in [
        (l.tool_met, Hit::ToolMetronome),
        (l.tool_snap, Hit::ToolSnap),
        (l.tool_grid, Hit::ToolGrid),
        (l.tool_nav, Hit::ToolNav),
    ] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    if l.grip.is_positive() && l.grip.contains(pos) {
        return Some(Hit::HeaderGrip);
    }
    for (i, row) in l.header_rows.iter().enumerate() {
        for (r, hit) in [
            (row.arm, Hit::Arm(i)),
            (row.mute, Hit::Mute(i)),
            (row.solo, Hit::Solo(i)),
            (row.knob, Hit::Gain(i)),
            (row.send, Hit::Send(i)),
            (row.pan, Hit::Pan(i)),
            (row.input, Hit::HdrInput(i)),
            (row.add, Hit::HdrAdd(i)),
            (row.name, Hit::HdrName(i)),
            (row.number, Hit::HdrNumber(i)),
        ] {
            if r.is_positive() && r.contains(pos) {
                return Some(hit);
            }
        }
        for (b, r) in row.bays.iter().enumerate() {
            if r.is_positive() && r.contains(pos) {
                return Some(Hit::HdrBay(i, b));
            }
        }
        if row.routing.is_positive() && row.routing.contains(pos) {
            return Some(Hit::HdrRouting(i));
        }
    }
    if l.ruler.is_positive() && l.ruler.contains(pos) {
        return Some(Hit::Ruler(time_at(rect, view, pos.x)));
    }
    for clip in view.clips.iter().rev() {
        // A clip being RECORDED is a picture, not a control — the document
        // row it becomes does not exist yet.
        if clip.recording {
            continue;
        }
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
    /// The HEADER column and toolbar's fixed look — the desk's wood and
    /// bone, WHATEVER the theme: the owner scoped light/dark to the lane
    /// field and the ruler alone.
    hdr_wood: Color32,
    hdr_face: Color32,
    hdr_lane: Color32,
    /// The ruler's own strip, themed.
    ruler: Color32,
    wood: Color32,
    face: Color32,
    engrave: Color32,
    faint: Color32,
    lane: Color32,
    lane_alt: Color32,
    grid: Color32,
    /// The empty slots' ground and their etched seam.
    empty_ground: Color32,
    seam: Color32,
    clip_ink: Color32,
    playhead: Color32,
    arm: Color32,
    mute: Color32,
    solo: Color32,
    meter: Color32,
    select: Color32,
}

fn ink(view: &ArrangeView<'_>) -> Ink {
    // **DERIVED from the theme, in both modes.**
    //
    // This was two hard-coded wardrobes — warm browns for dark, greys for
    // light — that no theme could reach, which is why the arrangement was the
    // one view that never changed with the rest of the app. Every surface
    // below now comes out of three theme roles: the band's WOOD, the strip
    // FACE the mixer's bays already wear, and the WELL the timecode sits in.
    //
    // Dark mode is a different DERIVATION, not a different wardrobe: the same
    // three roles, taken further down. That is what keeps a theme recognisable
    // with D pressed instead of swapping it for somebody else's browns.
    let wood = Color32::from_rgb(view.wood.0, view.wood.1, view.wood.2);
    let strip = Color32::from_rgb(view.fx_strip.0, view.fx_strip.1, view.fx_strip.2);
    let well = Color32::from_rgb(view.meter_bg.0, view.meter_bg.1, view.meter_bg.2);
    let dark = view.dark_mode;

    // The lane field. In light mode the well is lifted toward the strip so the
    // lanes read as a SURFACE that clips sit on rather than a hole cut in the
    // desk; in dark mode it stays near the well and is lifted only enough to
    // separate from the seams.
    // Through the dead-zone guard, for the reason `legible_surface` gives:
    // the clips and the words that land on this field have to be readable
    // whatever three roles it was derived from.
    let lane = crate::recorder_panel::legible_surface(if dark {
        shade(well, 1.55)
    } else {
        blend(well, strip, 0.46)
    });
    // **Structure separates from the field PERCEPTUALLY, not by a multiplier.**
    //
    // Scaling brightness is fine on a pale field and does almost nothing on a
    // dark one: 0.78 of a near-black is still a near-black, and the grid, the
    // seams and the alternating stripe all vanished together on every dark
    // theme. So each moves AWAY from the field — toward white if the field is
    // dark, toward black if it is light — by an amount that reads either way.
    let lane_alt = separate(lane, 0.055);
    let grid = separate(lane, 0.115);
    let empty_ground = separate(lane, 0.16);
    let seam = separate(lane, 0.24);
    let ruler = blend(wood, lane, 0.55);

    // Ink follows the surface it lands on — the rule this app has now had to
    // learn six times. The headers wear the strip face; the lanes wear the
    // field.
    let on = |bg: Color32| {
        if crate::recorder_panel::wants_light_ink(bg) {
            crate::recorder_panel::INK_LIGHT
        } else {
            crate::recorder_panel::INK_DARK
        }
    };
    let engrave = on(strip);
    Ink {
        hdr_wood: wood,
        hdr_face: strip,
        // Through the guard as well: it carries the M/S/R letters, and
        // shading the field 18% darker is exactly the kind of nudge that
        // lands a pale theme in the dead zone where neither ink clears.
        hdr_lane: crate::recorder_panel::legible_surface(shade(lane, 0.82)),
        ruler,
        wood: shade(wood, if dark { 0.62 } else { 1.0 }),
        face: strip,
        engrave,
        faint: blend(engrave, strip, 0.38),
        lane,
        lane_alt,
        grid,
        empty_ground,
        seam,
        // Clips wear PALETTE colours, and every one of those is generated
        // dark enough to carry the ivory ink at 4.5:1 — that is the palette's
        // own contract, held by `every_auto_palette_colour_can_carry_the_ink`.
        // So the ink on a clip answers to the clip, not to the field behind it.
        clip_ink: crate::recorder_panel::INK_LIGHT,
        // The semantic lights are the desk's, everywhere, in every theme:
        // a record light that changed colour with the wallpaper would be a
        // record light you had to learn twice.
        // **The playhead is a red that can be SEEN on this field.** It stays
        // red in every theme — a transport light you had to learn twice would
        // be a worse idea than a themed one — but a bright crimson on a bone
        // field is a line nobody can follow, so the deep one answers there.
        playhead: redder_of(lane),
        arm: Color32::from_rgb(0xE0, 0x22, 0x22),
        mute: Color32::from_rgb(0xE0, 0xA8, 0x22),
        solo: redder_of(lane),
        meter: Color32::from_rgb(0x3D, 0xC0, 0x5A),
        select: on(lane),
    }
}

/// A quieter ink: pulled toward the ground it sits on, never made
/// transparent.
///
/// **`gamma_multiply` on text is the bug this replaces.** It lowers the
/// ALPHA, so the letters composite into whatever is behind them — which on a
/// bone strip reads as grey and on a dark themed one reads as nothing at all.
/// The mixer learned this once already ("pulled toward the face rather than
/// made transparent"); the arrangement was still doing it in six places, and
/// the owner photographed the result.
fn on_ink(bg: Color32) -> Color32 {
    if crate::recorder_panel::wants_light_ink(bg) {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    }
}

fn quiet(ink: Color32, bg: Color32, t: f32) -> Color32 {
    // **Quieter, but never quiet enough to stop being words.** How far `t`
    // can go depends on the ground: on the pale themes a third of the way
    // toward a near-white panel is already a grey nobody reads, which is what
    // the audit caught on Centennial and Porcelain. So it is blended as asked
    // and then walked back until it clears — secondary text is still text.
    // 4.5, the same bar every other word in the app answers to. Quieter is a
    // matter of WEIGHT, not of being harder to read.
    const FLOOR: f32 = 4.5;
    let out = blend(ink, bg, t);
    if crate::recorder_panel::contrast_ratio(out, bg) >= FLOOR {
        return out;
    }
    let mut back = t;
    for _ in 0..12 {
        back *= 0.7;
        let c = blend(ink, bg, back);
        if crate::recorder_panel::contrast_ratio(c, bg) >= FLOOR {
            return c;
        }
    }
    ink
}

/// Move a colour AWAY from itself by `t`: toward white when it is dark,
/// toward black when it is light. What "a shade darker" should have been on a
/// surface that might be either.
fn separate(c: Color32, t: f32) -> Color32 {
    let toward = if crate::recorder_panel::wants_light_ink(c) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    blend(c, toward, t)
}

/// A transport red that can be SEEN on `bg`.
///
/// The two shipped reds first — a playhead should look like the playhead — and
/// if the field sits between them, the winner is pushed away from the field
/// until it clears. Pushed toward white it goes pink and toward black it goes
/// maroon; both still read as the red line, which a line nobody can find does
/// not.
fn redder_of(bg: Color32) -> Color32 {
    const FLOOR: f32 = 2.0;
    let bright = Color32::from_rgb(0xE8, 0x3A, 0x4E);
    let deep = Color32::from_rgb(0x8A, 0x10, 0x20);
    let ratio = |c: Color32| crate::recorder_panel::contrast_ratio(c, bg);
    let best = if ratio(bright) >= ratio(deep) { bright } else { deep };
    if ratio(best) >= FLOOR {
        return best;
    }
    let toward = if crate::recorder_panel::wants_light_ink(bg) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut out = best;
    for step in 1u8..=40 {
        out = blend(best, toward, f32::from(step) * 0.025_f32);
        if ratio(out) >= FLOOR {
            break;
        }
    }
    out
}

/// Scale a colour's brightness, keeping its hue. Above 1.0 lifts toward
/// white rather than clipping, so a near-black well can still be lifted.
fn shade(c: Color32, k: f32) -> Color32 {
    if k <= 1.0 {
        let f = |v: u8| (f32::from(v) * k).clamp(0.0, 255.0) as u8;
        return Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()));
    }
    // Lifting: add a fraction of the headroom, so hue survives and nothing
    // saturates to grey.
    let t = (k - 1.0).min(1.0);
    let f = |v: u8| {
        let v = f32::from(v);
        (v + (255.0 - v) * t * 0.28).clamp(0.0, 255.0) as u8
    };
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

/// `t` of the way from `a` to `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
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
    painter.rect_filled(l.ruler, 0.0, p.ruler);
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
    // **The clock row: h:mm:ss along the ruler's foot** — the owner's
    // ask: bars answer "where in the music", this answers "where in the
    // afternoon". The interval widens as the view does.
    {
        let font = FontId::new(
            (l.ruler.height() * 0.34).clamp(7.0, 9.5),
            crate::fonts::courier(),
        );
        let steps: [f64; 12] = [
            1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
        ];
        let want = steps
            .iter()
            .copied()
            .find(|st| st * px_per_s >= 70.0)
            .unwrap_or(3600.0);
        let mut n = (view.view_start_s / want).floor().max(0.0);
        while n * want <= view.view_start_s + view.view_len_s {
            let t = n * want;
            let x = x_of(rect, view, t);
            if x >= l.lanes_area.left() - 0.5 && x <= l.lanes_area.right() - 30.0 {
                let secs = t.round() as u64;
                let word = if secs >= 3600 {
                    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                } else {
                    format!("{}:{:02}", secs / 60, secs % 60)
                };
                painter.text(
                    Pos2::new(x + 3.0, l.ruler.bottom() - 1.0),
                    Align2::LEFT_BOTTOM,
                    word,
                    font.clone(),
                    quiet(on_ink(p.ruler), p.ruler, 0.18),
                );
            }
            n += 1.0;
        }
    }

    // **The grid the MAGNET snaps to is the grid the eye gets** — the
    // owner's complaint was exactly this line's absence: the lanes drew
    // only the labelled bars, whatever the gridbox said. Every bar gets a
    // line while bars are wide enough to tell apart, and the gridbox's own
    // subdivision draws between them, fainter but VISIBLE.
    {
        let g = bar * 2.0f64.powi(i32::from(view.grid_exp.clamp(-6, 6)));
        let bar_px = bar * px_per_s;
        let grid_px = g * px_per_s;
        let t1 = view.view_start_s + view.view_len_s;
        // `w` < 1 DARKENS: the strong line (the snap grid) is the darker
        // one on this grey field, the reference shot's own look.
        let line = |t: f64, w: f32| {
            let x = x_of(rect, view, t);
            if x >= l.lanes_area.left() - 0.5 && x <= l.lanes_area.right() + 0.5 {
                painter.line_segment(
                    [
                        Pos2::new(x, l.lanes_area.top()),
                        Pos2::new(x, l.lanes_area.bottom()),
                    ],
                    // The STRONG line is wider as well as darker — the
                    // owner asked twice for starker, and weight is what
                    // reads at a glance where shade alone blurs. `w = 1.0`
                    // is the strong line (full ink); below it FADES — the
                    // old comment had it backwards, which is why "stark"
                    // never arrived however the base colour moved.
                    Stroke::new(if w >= 1.0 { 1.6 } else { 1.0 }, p.grid.gamma_multiply(w)),
                );
            }
        };
        if g >= bar - 1.0e-9 {
            // **A multi-bar grid draws COARSER, visibly** — the owner
            // turned the box to "4" and saw no change, because every bar
            // drew at full weight whatever the grid said. The grid step
            // gets the strong line; the bars inside it drop to the faint
            // weight the subdivisions use.
            if grid_px >= 5.0 {
                let mut n = (view.view_start_s / g).floor().max(0.0);
                while n * g <= t1 {
                    line(n * g, 1.0);
                    n += 1.0;
                }
            }
            if g > bar + 1.0e-9 && bar_px >= 5.0 {
                let mut n = (view.view_start_s / bar).floor().max(0.0);
                while n * bar <= t1 {
                    let t = n * bar;
                    let on_grid = (t / g - (t / g).round()).abs() < 1.0e-6;
                    if !on_grid {
                        line(t, 0.55);
                    }
                    n += 1.0;
                }
            }
        } else {
            // A sub-bar grid: bars strong, subdivisions between them
            // fainter but visible — the rule since the owner asked for it.
            if bar_px >= 5.0 {
                let mut n = (view.view_start_s / bar).floor().max(0.0);
                while n * bar <= t1 {
                    line(n * bar, 1.0);
                    n += 1.0;
                }
            }
            if grid_px >= 5.0 {
                let mut n = (view.view_start_s / g).floor().max(0.0);
                while n * g <= t1 {
                    let t = n * g;
                    let on_bar = (t / bar - (t / bar).round()).abs() < 1.0e-6;
                    if !on_bar {
                        line(t, 0.55);
                    }
                    n += 1.0;
                }
            }
        }
    }

    // ── every channel boundary wears a SEAM — header through field, one
    // line, drawn over the grid so it survives the lines running through
    // it. Empty lanes keep the stronger slot treatment below. ─────────────
    for (i, row) in l.lane_rows.iter().enumerate() {
        let left = l
            .header_rows
            .get(i)
            .map_or(row.left(), |h| h.panel.left());
        painter.line_segment(
            [
                Pos2::new(left, row.bottom()),
                Pos2::new(row.right(), row.bottom()),
            ],
            Stroke::new(1.0, p.seam.gamma_multiply(1.25)),
        );
    }

    // ── the lane a hovering file would land on ────────────────────────────
    //
    // Drawn UNDER the clips and over the field, so it reads as the lane
    // lighting up rather than as something laid on top of the music.
    if let Some(target) = view.drop_lane {
        if let Some(row) = target
            .checked_sub(view.lane_scroll)
            .and_then(|i| l.lane_rows.get(i))
            .filter(|r| r.is_positive())
        {
            painter.rect_filled(*row, 0.0, p.select.gamma_multiply(0.16));
            painter.rect_stroke(
                *row,
                0.0,
                Stroke::new(1.5, p.select.gamma_multiply(0.85)),
                egui::StrokeKind::Inside,
            );
        }
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
        // A MIDI item draws its NOTES in miniature — the arrangement can
        // read what the editor will say. An empty item keeps the dashed
        // staff that says "MIDI, nothing played yet".
        if clip.midi && r.width() >= 8.0 {
            if clip.notes.is_empty() {
                let mid = r.center().y;
                let mut x = r.left() + 3.0;
                while x < r.right() - 3.0 {
                    painter.line_segment(
                        [Pos2::new(x, mid), Pos2::new((x + 4.0).min(r.right() - 3.0), mid)],
                        Stroke::new(1.0, p.clip_ink.gamma_multiply(0.5)),
                    );
                    x += 8.0;
                }
            } else {
                let full_x = (
                    x_of(rect, view, clip.start_s),
                    x_of(rect, view, clip.start_s + clip.len_s),
                );
                draw_note_miniature(painter, r, full_x, clip, &p);
            }
        }
        // The wave, drawn inside at the clip's own resolution — sampled
        // through the clip's FULL pixel span, so a clip cut by the
        // viewport shows the right piece of itself rather than the whole
        // wave squeezed into what is visible.
        if !clip.wave.is_empty() && r.width() >= 8.0 {
            let mid = r.center().y;
            let half = r.height() * 0.38;
            let cols = r.width() as usize;
            let fx0 = x_of(rect, view, clip.start_s);
            let fw = (x_of(rect, view, clip.start_s + clip.len_s) - fx0).max(1.0);
            for i in 0..cols {
                let x_at = r.left() + i as f32;
                let f = ((x_at - fx0) / fw).clamp(0.0, 1.0);
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
        // A clip being recorded wears the record's own red, and no handles:
        // its edges are the take's business until the take ends.
        if clip.recording {
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.5, Color32::from_rgb(0xE8, 0x3A, 0x4E)),
                egui::StrokeKind::Inside,
            );
        }
        // The fade HANDLES, only at a size where they are grabbable.
        if !clip.recording && r.width() >= FADE_W * 3.0 {
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

    // ── the TIME SELECTION: a veil over the span, on ruler and lanes ──────
    // AFTER the clips, deliberately: the veil and its boundary lines are
    // about the material inside the span, and painting them first let
    // every opaque clip body hide exactly the part of the selection that
    // mattered — visible only in the gaps, the standard DAW picture
    // inverted. Under the playhead still, which always reads on top.
    if let Some((t0, t1)) = view.time_sel.filter(|(a, b)| b > a) {
        let x0 = x_of(rect, view, t0).max(l.lanes_area.left());
        let x1 = x_of(rect, view, t1).min(l.lanes_area.right());
        if x1 > x0 {
            let sel = Color32::from_rgb(0xC9, 0x8B, 0x3D);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0, l.lanes_area.top()),
                    Pos2::new(x1, l.lanes_area.bottom()),
                ),
                0.0,
                Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 26),
            );
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(x0, l.ruler.top()),
                    Pos2::new(x1, l.ruler.bottom()),
                ),
                0.0,
                Color32::from_rgba_unmultiplied(sel.r(), sel.g(), sel.b(), 84),
            );
            for x in [x0, x1] {
                painter.line_segment(
                    [
                        Pos2::new(x, l.ruler.top()),
                        Pos2::new(x, l.lanes_area.bottom()),
                    ],
                    Stroke::new(1.0, sel.gamma_multiply(0.8)),
                );
            }
        }
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

    // ── the headers, over their own wood — the FIXED look, both themes ────
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(l.header.right(), rect.bottom())),
        0.0,
        p.hdr_wood,
    );
    // **The resize STRIP**, REAPER's splitter: wood-faced, hover-lit,
    // a real strip rather than a hairline.
    if l.grip.is_positive() {
        painter.rect_filled(
            l.grip,
            0.0,
            if view.grip_lit {
                p.hdr_face.gamma_multiply(0.45)
            } else {
                p.hdr_wood
            },
        );
        // The handle's own seam, so the strip reads as a control even
        // unlit.
        painter.line_segment(
            [
                Pos2::new(l.grip.center().x, l.grip.top() + 6.0),
                Pos2::new(l.grip.center().x, l.grip.bottom() - 6.0),
            ],
            Stroke::new(1.0, p.engrave.gamma_multiply(if view.grip_lit { 1.0 } else { 0.55 })),
        );
    }
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
            p.hdr_face
        };
        let plate = row.panel.shrink2(Vec2::new(2.0, 1.5));
        if !plate.is_positive() {
            continue;
        }
        if lane.used {
            painter.rect_filled(plate, 2.0, face);
            if view.selected_channel == Some(i) {
                painter.rect_stroke(
                    plate,
                    2.0,
                    Stroke::new(1.2, Color32::from_rgba_unmultiplied(0xFF, 0xF4, 0xE0, 90)),
                    egui::StrokeKind::Inside,
                );
            }
        } else {
            // The empty slot's own ground fills the WHOLE panel — the wood
            // margin around a dark plate read as two surfaces, the owner's
            // "my bad" screenshot.
            painter.rect_filled(row.panel, 0.0, p.empty_ground);
        }
        // **The ink is one of the two INKS, not one of the theme's colours.**
        //
        // This measured the right thing and then answered with the wrong
        // values: on a dark plate it returned `p.hdr_face` — the themed strip
        // colour — so on every theme whose strip is dark, the lane title, the
        // dial readouts and the switch letters were drawn in a dark navy or
        // teal ON a dark navy or teal plate. Fixing the threshold and leaving
        // the branches was half a fix, and the audit did not catch it because
        // the audit computed its own ink instead of calling this. It calls
        // `on_ink` now, and so does the audit.
        let ink_on = on_ink(face);
        painter.text(
            Pos2::new(plate.left() + 4.0, plate.center().y),
            Align2::LEFT_CENTER,
            format!("{}", i + 1),
            num_font.clone(),
            if lane.used {
                quiet(ink_on, face, 0.30)
            } else {
                // Tucked faint on the dark ground, clear of the + circle.
                p.hdr_face.gamma_multiply(0.30)
            },
        );
        // The input chip draws on EVERY lane — binding an input is how a
        // bare lane becomes a track — in bone whatever the theme, so the
        // words read on a painted header and on the grey field alike.
        let chip = |painter: &Painter, r: Rect, word: &str, has: bool| {
            if !r.is_positive() {
                return;
            }
            // **The same surface the mixer's bays are**, and the same ink.
            // These were a hard-coded bone panel with hard-coded near-black
            // on it — the shipped desk's colours, frozen — so on every theme
            // but the default the input, FX and routing chips stayed white
            // modules on a desk that had moved. `hdr_face` IS the mixer's
            // strip face, and `engrave` is what that face can carry.
            let bone = p.hdr_face;
            let ink = p.engrave;
            painter.rect_filled(r, 2.0, bone);
            painter.rect_stroke(
                r,
                2.0,
                Stroke::new(1.0, ink.gamma_multiply(0.4)),
                egui::StrokeKind::Inside,
            );
            let font = FontId::new((r.height() * 0.62).clamp(7.0, 10.0), crate::fonts::courier());
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                cut_to(painter, r.width() - 4.0, word, &font),
                font,
                quiet(ink, bone, if has { 0.08 } else { 0.34 }),
            );
        };
        if !lane.used {
            if row.add.is_positive() {
                let c = row.add.center();
                // The + on an empty slot: measured against the ground it is
                // actually drawn on, and blended rather than made
                // transparent. It was the themed face at 55% alpha, which on
                // a dark theme is a circle nobody can see.
                let ink = quiet(on_ink(p.empty_ground), p.empty_ground, 0.22);
                let radius = row.add.width().min(row.add.height()) * 0.5 - 1.0;
                let w = (radius * 0.12).clamp(1.2, 2.2);
                painter.circle_stroke(c, radius, Stroke::new(w, ink));
                let arm = radius * 0.45;
                painter.line_segment(
                    [Pos2::new(c.x - arm, c.y), Pos2::new(c.x + arm, c.y)],
                    Stroke::new(w, ink),
                );
                painter.line_segment(
                    [Pos2::new(c.x, c.y - arm), Pos2::new(c.x, c.y + arm)],
                    Stroke::new(w, ink),
                );
            }
            chip(painter, row.input, "NO INPUT", false);
            continue;
        }
        // The arm dot: REAPER's record circle, lit when live.
        let c = row.arm.center();
        let rad = row.arm.width() * 0.5;
        if lane.armed {
            painter.circle_filled(c, rad, p.arm);
        }
        painter.circle_stroke(c, rad, Stroke::new(1.5, ink_on.gamma_multiply(0.8)));
        // The name, on its plate — or the rename field mid-typing.
        if row.name.is_positive() {
            let font = FontId::new(
                (row.name.height() * 0.56).clamp(8.5, 12.0),
                crate::fonts::courier_bold(),
            );
            let naming = view.naming.filter(|(at, _)| *at == i).map(|(_, b)| b);
            if let Some(buf) = naming {
                painter.rect_filled(row.name, 2.0, p.hdr_face);
                painter.rect_stroke(
                    row.name,
                    2.0,
                    Stroke::new(1.0, p.select),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    Pos2::new(row.name.left() + 2.0, row.name.center().y),
                    Align2::LEFT_CENTER,
                    format!("{buf}_"),
                    font,
                    p.engrave,
                );
            } else {
                painter.text(
                    Pos2::new(row.name.left() + 2.0, row.name.center().y),
                    Align2::LEFT_CENTER,
                    cut_to(painter, row.name.width() - 4.0, lane.name, &font),
                    font,
                    ink_on,
                );
            }
        }
        // The INPUT CHIP under the name, the mixer strip's own idiom.
        let word = if lane.input.is_empty() { "NO INPUT" } else { lane.input.as_str() };
        chip(painter, row.input, word, !lane.input.is_empty());
        // The three bays beside it — the mixer's insert slots, worded the
        // same: the plugin's short name, or a plus for an empty seat.
        for (b, r) in row.bays.iter().enumerate() {
            if !r.is_positive() {
                continue;
            }
            let bay_word = lane.inserts.get(b).copied().unwrap_or("");
            chip(
                painter,
                *r,
                if bay_word.is_empty() { "+" } else { bay_word },
                !bay_word.is_empty(),
            );
        }
        // ROUTING, under the dials: the same door the mixer's button opens,
        // wearing the same chip the bays do so the header reads as one row
        // of pressable things rather than two idioms.
        if row.routing.is_positive() {
            let n = lane.route_count;
            let word = if n > 0 {
                format!("ROUTING {n}")
            } else {
                "ROUTING".to_owned()
            };
            chip(painter, row.routing, &word, n > 0);
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
            // Sized to the CELL, width included: three readouts sit shoulder
            // to shoulder up here, and a figure that overflows its dial runs
            // into the next figure and reads as one number.
            let fit = cell.width() / (text.chars().count().max(1) as f32 * 0.64);
            painter.text(
                cell.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::new(
                    cell.height().clamp(7.0, 9.0).min(fit).max(5.5),
                    crate::fonts::courier(),
                ),
                ink_on,
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
        dial(
            painter,
            row.pan,
            (lane.pan.clamp(-1.0, 1.0) + 1.0) * 0.5,
            Color32::from_rgb(0xB0, 0x62, 0xC8),
        );
        let pan_word = {
            let v = (lane.pan.clamp(-1.0, 1.0) * 50.0).round() as i32;
            if v == 0 {
                "C".to_owned()
            } else if v < 0 {
                format!("L{}", -v)
            } else {
                format!("R{v}")
            }
        };
        readout(painter, row.pan_readout, pan_word);
        // The labels, under their dials — always visible, the standard.
        for (dial, word) in [(row.pan, "PAN"), (row.knob, "VOL"), (row.send, "SND")] {
            if !dial.is_positive() {
                continue;
            }
            let band = Rect::from_min_max(
                Pos2::new(dial.left() - 2.0, dial.bottom()),
                Pos2::new(dial.right() + 2.0, row.panel.bottom() - 1.0),
            );
            if band.height() >= 6.0 {
                painter.text(
                    Pos2::new(band.center().x, band.top()),
                    Align2::CENTER_TOP,
                    word,
                    FontId::new(band.height().clamp(6.0, 8.0), crate::fonts::courier()),
                    quiet(ink_on, face, 0.34),
                );
            }
        }
        // The meter: a PAIR of slim bars on a stereo source, one on mono —
        // the mixer's own claim, kept here.
        if row.meter.is_positive() {
            let lanes_n = if lane.stereo { 2 } else { 1 };
            let gap = 1.0;
            let w = ((row.meter.width() - gap * (lanes_n as f32 - 1.0))
                / lanes_n as f32)
                .max(1.5);
            for b in 0..lanes_n {
                let cell = Rect::from_min_max(
                    Pos2::new(row.meter.left() + b as f32 * (w + gap), row.meter.top()),
                    Pos2::new(
                        (row.meter.left() + b as f32 * (w + gap) + w)
                            .min(row.meter.right()),
                        row.meter.bottom(),
                    ),
                );
                painter.rect_filled(cell, 1.0, p.lane);
                let v = if lane.stereo {
                    lane.peak[b.min(1)]
                } else {
                    lane.peak[0].max(lane.peak[1])
                };
                if v > 0.0 {
                    let t = crate::recorder::gain_to_fader(v).clamp(0.0, 1.0);
                    painter.rect_filled(
                        Rect::from_min_max(
                            Pos2::new(cell.left(), cell.bottom() - cell.height() * t),
                            cell.max,
                        ),
                        1.0,
                        p.meter,
                    );
                }
            }
        }
        for (r, label, on, lit) in [
            (row.mute, "M", lane.muted, p.mute),
            (row.solo, "S", lane.soloed, p.solo),
        ] {
            if !r.is_positive() {
                continue;
            }
            painter.rect_filled(r, 2.0, if on { lit } else { p.hdr_lane });
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new((r.height() * 0.6).clamp(8.0, 12.0), crate::fonts::courier_bold()),
                if on { on_ink(lit) } else { quiet(on_ink(p.hdr_lane), p.hdr_lane, 0.18) },
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
        p.hdr_wood,
    );
    let tool = |painter: &Painter, r: Rect, on: bool| {
        if !r.is_positive() {
            return false;
        }
        painter.rect_filled(r, 2.0, if on { p.hdr_face } else { p.hdr_lane });
        true
    };
    if tool(painter, l.tool_met, view.metronome_on) {
        crate::recorder_panel::draw_metronome_on(
            painter,
            l.tool_met.shrink(2.0),
            if view.metronome_on { p.engrave } else { p.hdr_face.gamma_multiply(0.7) },
            if view.metronome_on { p.hdr_face } else { p.hdr_lane },
        );
    }
    if tool(painter, l.tool_snap, view.snap_on) {
        // The magnet: a U with its two poles, the sign every DAW uses.
        let m = l.tool_snap.shrink(3.5);
        let ink = if view.snap_on { p.engrave } else { p.hdr_face.gamma_multiply(0.7) };
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
    // The NAVIGATOR: the whole song as a recessed box, the window as a lit
    // rectangle inside it. A hand on it scrolls; up and down zoom.
    if l.tool_nav.is_positive() {
        painter.rect_filled(l.tool_nav, 2.0, p.lane);
        painter.rect_stroke(
            l.tool_nav,
            2.0,
            Stroke::new(1.0, p.grid.gamma_multiply(2.0)),
            egui::StrokeKind::Inside,
        );
        let song = view.song_s.max(view.view_start_s + view.view_len_s).max(1.0e-9);
        let f0 = (view.view_start_s / song).clamp(0.0, 1.0) as f32;
        let f1 = ((view.view_start_s + view.view_len_s) / song).clamp(0.0, 1.0) as f32;
        let inner = Rect::from_min_max(
            Pos2::new(l.tool_nav.left() + l.tool_nav.width() * f0, l.tool_nav.top() + 1.5),
            Pos2::new(
                (l.tool_nav.left() + l.tool_nav.width() * f1)
                    .max(l.tool_nav.left() + l.tool_nav.width() * f0 + 3.0),
                l.tool_nav.bottom() - 1.5,
            ),
        );
        if inner.is_positive() {
            painter.rect_filled(inner, 1.5, p.hdr_face.gamma_multiply(0.8));
        }
    }

    // ── the EMPTY SLOTS' borders, LAST — nothing may paint over them ──────
    // Third round on these, and the lesson was paint ORDER, not ink: the
    // header section begins by filling its whole column with wood, and the
    // empty slot's own panel is filled again inside the header loop — every
    // earlier border, etched or boxed, was buried the same frame it was
    // drawn. Painted here, on top of everything, each unoccupied slot is
    // one channel-shaped box — header through field — in faded face ink
    // that reads on both palettes.
    for (i, row) in l.lane_rows.iter().enumerate() {
        if view.lanes.get(i).is_some_and(|lane| lane.used) {
            continue;
        }
        let left = l.header_rows.get(i).map_or(row.left(), |h| h.panel.left());
        let slot = Rect::from_min_max(Pos2::new(left, row.top()), row.right_bottom());
        if slot.is_positive() {
            painter.rect_stroke(
                slot,
                0.0,
                // Half a point: the owner wants them THIN — present, not
                // loud — and the full-width run makes even a hairline read.
                Stroke::new(0.5, p.face.gamma_multiply(0.35)),
                egui::StrokeKind::Inside,
            );
        }
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
            automation_open: false,
            route_count: 0,
            stereo: true,
            inserts: [""; crate::recorder::INSERTS],
            name: "KEYS",
            input: String::new(),
            color: 0,
            kind: used.then_some(ChannelKind::Midi),
            used,
            armed: false,
            muted: false,
            soloed: false,
            gain: 1.0,
            send: 0.0,
            pan: 0.0,
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
                notes: &[],
                offset_s: 0.0,
                recording: false,
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
                notes: &[],
                offset_s: 0.0,
                recording: false,
            },
        ]
    }

    fn a_view<'a>(clips: &'a [ClipView<'a>]) -> ArrangeView<'a> {
ArrangeView {
            show_tempo_lane: false,
            fx_strip: (0xE3, 0xD9, 0xBD),
            meter_bg: (0x0B, 0x0E, 0x0A),
            header_w: None,
            naming: None,
            time_sel: None,
            selected_channel: None,
            lanes: std::array::from_fn(|i| a_lane(i < 3)),
            lane_scroll: 0,
            visible_lanes: VISIBLE_LANES,
            clips,
            metronome_on: true,
            snap_on: true,
            grip_lit: false,
            drop_lane: None,
            grid_exp: 0,
            pending: None,
            view_start_s: 0.0,
            view_len_s: 120.0,
            song_s: 120.0,
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

    /// **The arrangement wears the THEME, in both modes, legibly.**
    ///
    /// It used to carry two hard-coded wardrobes no theme could reach — warm
    /// browns for dark, greys for light — so it was the one view that stayed
    /// put while the rest of the app changed. Now every surface derives from
    /// the band's wood, the strip face and the well, and this is what keeps
    /// the derivation honest across all sixteen.
    #[test]
    fn every_theme_dresses_the_arrangement_legibly_in_both_modes() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                let clips: Vec<ClipView<'_>> = Vec::new();
                let mut v = a_view(&clips);
                v.dark_mode = dark;
                v.wood = (
                    set.recorder_bg_color.r,
                    set.recorder_bg_color.g,
                    set.recorder_bg_color.b,
                );
                v.fx_strip = (
                    set.fx_strip_color.r,
                    set.fx_strip_color.g,
                    set.fx_strip_color.b,
                );
                v.meter_bg = (
                    set.meter_bg_color.r,
                    set.meter_bg_color.g,
                    set.meter_bg_color.b,
                );
                let p = ink(&v);
                let mode = if dark { "dark" } else { "light" };
                // **The lane's OWN colour is the ground the header text lands
                // on**, not the strip face — every used lane wears a palette
                // colour. The first version of this audit left the lanes
                // uncoloured, so it measured a pairing the app never draws
                // and passed while the owner was looking at unreadable
                // titles.
                let pal = &crate::mixer_panel::PALETTES[set
                    .palette_choice
                    .rem_euclid(crate::mixer_panel::PALETTES.len() as i64)
                    as usize];
                for ch in 0..crate::recorder::CHANNELS {
                    let (r, g, b) = crate::mixer_panel::STRIP_COLORS[pal.slot(ch)];
                    let plate = Color32::from_rgb(r, g, b);
                    let ink_on = on_ink(plate);
                    let mut c2 = |what: &str, fg: Color32, floor: f32| {
                        let r = crate::recorder_panel::contrast_ratio(fg, plate);
                        if r < floor {
                            bad.push(format!("{} {mode} ch{ch}: {what} {r:.2}:1", theme.name));
                        }
                    };
                    c2("lane title", ink_on, 4.5);
                    c2("second line", quiet(ink_on, plate, 0.30), 4.0);
                    c2("dial readout", ink_on, 4.5);
                    c2("lane number", quiet(ink_on, plate, 0.34), 4.0);
                }
                let mut check = |what: &str, fg: Color32, bg: Color32, floor: f32| {
                    let r = crate::recorder_panel::contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // Words have to be readable; structure only has to be seen.
                check("header text", p.engrave, p.hdr_face, 4.5);
                // **The chips are the mixer's bays.** Input, FX and routing
                // were a hard-coded bone panel with hard-coded near-black on
                // it, so on every theme but the default they stayed white
                // modules on a desk that had moved. Same surface, same ink —
                // and the faded state has to read too, which is the one the
                // 0.55 alpha used to make invisible.
                check("chip text", p.engrave, p.hdr_face, 4.5);
                // **And the QUIETED versions**, which is what the last audit
                // missed. Six places drew text with `gamma_multiply`, which
                // lowers alpha and composites the letters into the ground —
                // full-strength ink passing says nothing about what was
                // actually on screen. These are the blends that replaced it.
                check(
                    "empty bay chip",
                    quiet(p.engrave, p.hdr_face, 0.34),
                    p.hdr_face,
                    3.0,
                );
                check(
                    "filled bay chip",
                    quiet(p.engrave, p.hdr_face, 0.08),
                    p.hdr_face,
                    4.5,
                );
                check("ruler seconds", quiet(on_ink(p.ruler), p.ruler, 0.18), p.ruler, 4.0);
                check(
                    "unlit switch letter",
                    quiet(on_ink(p.hdr_lane), p.hdr_lane, 0.18),
                    p.hdr_lane,
                    4.0,
                );
                assert_eq!(
                    p.hdr_face,
                    Color32::from_rgb(
                        set.fx_strip_color.r,
                        set.fx_strip_color.g,
                        set.fx_strip_color.b,
                    ),
                    "{} {mode}: the chips left the mixer's strip face",
                    theme.name,
                );
                // **The clip ink is checked against the CLIP**, which is the
                // only thing it is ever drawn on — the note miniatures and
                // the fade lines both sit inside a clip's own rect. Measuring
                // it against the lane behind was measuring a pairing that
                // never happens, and Sandstone failed it for no reason.
                let pal = &crate::mixer_panel::PALETTES
                    [set.palette_choice.rem_euclid(
                        crate::mixer_panel::PALETTES.len() as i64,
                    ) as usize];
                for ch in 0..crate::recorder::CHANNELS {
                    let (r, g, b) =
                        crate::mixer_panel::STRIP_COLORS[pal.slot(ch)];
                    check(
                        "clip ink on a clip",
                        p.clip_ink,
                        Color32::from_rgb(r, g, b),
                        4.5,
                    );
                }
                check("grid on lane", p.grid, p.lane, 1.12);
                check("seam on lane", p.seam, p.lane, 1.25);
                check("lane stripe", p.lane_alt, p.lane, 1.03);
                check("playhead", p.playhead, p.lane, 1.6);
            }
        }
        assert!(
            bad.is_empty(),
            "the arrangement does not hold up:\n  {}",
            bad.join("\n  ")
        );
    }
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

    /// **The rewrite is a no-op until something is expanded.**
    ///
    /// The layout is what every clip is positioned against and every hit is
    /// resolved through, so replacing the fixed-height array with a row list
    /// has to change NOTHING while the new row kinds are switched off. This
    /// reproduces the old arithmetic independently — `lanes_area.height() /
    /// visible`, stacked from `lane_scroll` — and demands the rows agree,
    /// rather than comparing the new code against itself.
    #[test]
    fn the_layout_is_unchanged_when_nothing_is_expanded() {
        for visible in [1_usize, 4, 8, CHANNELS] {
            for scroll in [0_usize, 1, 5] {
                let mut v = a_view(&[]);
                v.visible_lanes = visible;
                v.lane_scroll = scroll;
                let l = Layout::new(rect(), &v);

                let lane_h = l.lanes_area.height() / visible as f32;
                let first = scroll.min(CHANNELS.saturating_sub(visible));
                for i in 0..CHANNELS {
                    let got = l.lane_rows[i];
                    if i < first || i >= first + visible {
                        assert!(
                            !got.is_positive(),
                            "lane {i} is laid out at visible={visible} scroll={scroll}"
                        );
                        continue;
                    }
                    let want_top = l.lanes_area.top() + lane_h * (i - first) as f32;
                    assert!(
                        (got.top() - want_top).abs() < 0.5,
                        "lane {i} moved to {} from {want_top} (visible={visible} \
                         scroll={scroll})",
                        got.top()
                    );
                    assert!(
                        (got.height() - lane_h).abs() < 0.5,
                        "lane {i} is {}pt tall, not {lane_h}",
                        got.height()
                    );
                    // The header row is the same band on the other side of
                    // the grip — they are one row, and a clip that drew
                    // against a header a few points out is the whole reason
                    // both come from one list now.
                    assert!(
                        (l.header_rows[i].panel.top() - got.top()).abs() < 0.5,
                        "lane {i}'s header and lane disagree about where the row is"
                    );
                }
            }
        }
    }

    /// A tempo lane and an automation lane push the tracks down, and the
    /// track keeps its own height.
    #[test]
    fn the_new_rows_take_their_room_from_the_area_and_not_from_the_track() {
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        let plain = Layout::new(rect(), &v);
        let track_h = plain.lane_rows[0].height();

        v.show_tempo_lane = true;
        v.lanes[0].automation_open = true;
        let l = Layout::new(rect(), &v);

        // The tempo row is first, above every track.
        let first = l.rows.first().expect("no rows");
        assert_eq!(first.kind, RowKind::Tempo);
        assert!(
            first.lane.top() <= l.lane_rows[0].top() + 0.5,
            "the tempo lane is not above the tracks"
        );
        // The automation row sits directly under the track it belongs to.
        let auto = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(0))
            .expect("no automation row");
        assert!(
            (auto.lane.top() - l.lane_rows[0].bottom()).abs() < 0.5,
            "lane 0's automation is not under lane 0"
        );
        // **The track did not shrink.** A sub-lane takes room from the area,
        // not from the clips it belongs to — a track that got shorter every
        // time you opened a curve would make the curve unusable and the
        // clips too.
        assert!(
            (l.lane_rows[0].height() - track_h).abs() < 0.5,
            "opening a curve shrank the track from {track_h} to {}",
            l.lane_rows[0].height()
        );
        // ...so something falls off the bottom instead, and what falls off
        // is NOTHING rather than a half-height row.
        for r in &l.rows {
            assert!(
                !r.lane.is_positive() || r.lane.bottom() <= l.lanes_area.bottom() + 0.5,
                "{:?} hangs off the bottom of the area",
                r.kind
            );
        }
    }

    /// **Eight lanes show; the rest scroll.** With more lanes than fit, only
    /// a window is laid out — a lane outside it is `NOTHING`, so it neither
    /// draws nor answers a hit — and the scroll chooses the window.
    #[test]
    fn the_arrangement_scrolls_its_lanes() {
        let c = clips();
        let r = rect();
        // At rest: lanes 0..VISIBLE show, the rest are off-desk.
        let mut v = a_view(&c);
        v.lane_scroll = 0;
        let l = Layout::new(r, &v);
        for n in 0..VISIBLE_LANES {
            assert!(l.lane_rows[n].is_positive(), "lane {n} should show");
        }
        for n in VISIBLE_LANES..CHANNELS {
            assert!(!l.lane_rows[n].is_positive(), "lane {n} should be scrolled off");
        }
        // Scrolled down: the last lane shows, lane 0 does not.
        let mut v = a_view(&c);
        v.lane_scroll = CHANNELS - VISIBLE_LANES;
        let l = Layout::new(r, &v);
        assert!(l.lane_rows[CHANNELS - 1].is_positive(), "the last lane never shows");
        assert!(!l.lane_rows[0].is_positive(), "lane 0 should be scrolled off");
        // The first visible lane sits at the top of the lane area.
        assert!(
            (l.lane_rows[CHANNELS - VISIBLE_LANES].top() - l.lanes_area.top()).abs() < 0.5,
            "the scrolled window does not start at the top"
        );
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

    /// **At the 150pt width floor the header sheds its DIALS, never the
    /// name.** The fixed cells flanking the column used to leave it
    /// inverted at the floor: every used lane's name, input chip and bays
    /// vanished, and rename and both menus died with them.
    #[test]
    fn a_floor_width_header_keeps_name_input_and_bays() {
        let c = clips();
        let v = a_view(&c);
        // Narrow enough that the automatic share (16%) hits the 150 floor.
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 420.0));
        let l = Layout::new(r, &v);
        let row = l.header_rows[0];
        assert!(row.name.is_positive(), "the name did not survive the floor");
        assert!(row.input.is_positive(), "the input chip did not survive");
        assert!(
            row.bays.iter().all(|b| !b.is_positive()) || row.bays[0].is_positive(),
            "bays must be all-or-nothing"
        );
        assert_eq!(
            hit_test(r, &v, row.name.center()),
            Some(Hit::HdrName(0)),
            "rename is unreachable at the floor"
        );
        assert_eq!(
            hit_test(r, &v, row.input.center()),
            Some(Hit::HdrInput(0)),
            "the input menu is unreachable at the floor"
        );
        // The dials are the horizontal sacrifice — the mixer still has them.
        assert!(!row.pan.is_positive(), "the pan dial did not shed");
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
