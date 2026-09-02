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
    /// **Six wide, like the mixer's**: a FOLDER lane fills all six (its
    /// extra rack, `recorder::folder_extra_seat`), everybody else the first
    /// three and blanks.
    pub inserts: [&'a str; crate::recorder::INSERTS * 2],
    /// How many routes leave this lane, for the ROUTING chip.
    pub route_count: usize,
    /// Polarity inverted — see `mixer_panel::StripView::phase`.
    pub phase: bool,
    pub muted: bool,
    /// **Every automation row this lane is showing**, in row order, top to
    /// bottom under the track.
    ///
    /// A list rather than one optional row: a track can expose its volume and
    /// its send at once, which is what mixing a track means.
    pub automation: Vec<AutoView<'a>>,
    pub soloed: bool,
    /// This lane's MIDI is sent to the theory view — the **T** switch.
    pub to_theory: bool,
    /// **This lane is a FOLDER** — the little box at its bottom-left is lit
    /// and the tracks under it are its children.
    pub is_folder: bool,
    /// **This lane is INSIDE the folder above it** — the one thing that draws
    /// an indent. A folder itself never carries one: the indent says "I am
    /// contained", and the container is not.
    pub in_folder: bool,
    /// This lane is a folder and it is SHUT: its children are not drawn and
    /// the arrow points right.
    pub folder_shut: bool,
    /// How many children this folder holds — written beside a shut one, so a
    /// closed group says what it is hiding rather than just vanishing.
    pub folder_count: usize,
    /// **This lane is not drawn at all**: it lives inside a shut folder. Its
    /// row is never pushed, so nothing is laid out, hit or painted for it.
    pub hidden: bool,
    /// **Silent because the folder holding it is muted** — or, on a folder,
    /// because the folder itself is.
    ///
    /// Its own M switch is untouched and draws untouched; this only dims the
    /// lane. Without it a child of a muted folder is a track that makes no
    /// sound with its mute clearly off, which reads as a fault rather than as
    /// a group.
    pub folder_silent: bool,
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
    /// The clip's picture: `[min, max, min, max, …]`, each in −1..=1, over
    /// the seconds `wave_from_s`..`wave_from_s + wave_span_s`. May be empty.
    ///
    /// **Pairs, and a span** — see `crate::ports::ClipInfo`, which says why
    /// both.
    pub wave: &'a [f32],
    pub wave_from_s: f64,
    pub wave_span_s: f64,
    /// The clip's notes, source-relative like the document's. Drawn as a
    /// miniature where audio draws its wave; empty draws the dashed staff.
    pub notes: &'a [crate::arrangement::ArrNote],
    /// **How long the looped section is**, or zero for a clip that does not
    /// loop. The notes are held once and DRAWN repeatedly, exactly as they
    /// are scheduled — a clip whose picture stopped after one pass would
    /// look broken however right it sounded.
    pub loop_s: f64,
    /// The left trim, for placing the miniature under the drawn span.
    pub offset_s: f64,
    pub selected: bool,
    /// A MIDI item: no file, no wave — drawn with its own face.
    pub midi: bool,
    /// Being RECORDED right now: drawn hot, and no gesture lands on it.
    pub recording: bool,
}

/// One automation lane's contents, pushed in.
///
/// **Points in SCREEN-independent terms** — seconds and a normalised value —
/// exactly as the document holds them. The panel turns them into pixels; it
/// is handed nothing that has already been through a rect, so a resize
/// cannot leave a stale coordinate behind.
pub struct AutoView<'a> {
    pub param: crate::automation::AutoParam,
    /// **What to CALL it**, resolved by the app.
    ///
    /// A desk parameter names itself, but a plugin's is a bay and an id here
    /// and a word over in `ports::FxParams` — so the app, which holds both,
    /// does the joining and the panel is handed the answer. The same rule
    /// every other colour and number in this struct follows.
    pub label: String,
    pub points: &'a [crate::automation::Point],
    /// The point under the pointer or being dragged, so it can be drawn lit.
    pub hot: Option<usize>,
    /// **The points that are CHOSEN**, by index into `points`.
    ///
    /// Separate from `hot`, which is one point and is about the pointer.
    /// These are what Delete, copy and cut act on, and a thing those keys
    /// will act on has to look chosen before it is pressed — the same
    /// **The point being DRAGGED right now**, which gets its value written
    /// above it.
    ///
    /// Not `hot`: that is the point under the pointer and is set by hovering
    /// too, so a readout keyed to it would flicker on across a row somebody
    /// was only crossing. A value you are setting is worth a number; a value
    /// you are passing over is not.
    pub moving: Option<usize>,
    /// argument `selected` makes for the row.
    ///
    /// Owned, like `label` beside it: the app resolves this from a selection
    /// held as `(lane, slot, index)` triples, and there is no slice on its
    /// side already shaped the way a row wants. A handful of `usize` per
    /// open row per frame, against a `String` that is already allocated here.
    pub chosen: Vec<usize>,
    /// **The keyboard is aimed at this row**, so Delete would close it.
    ///
    /// Drawn as the rim a chosen track wears, for the same reason: a thing
    /// the Delete key will act on has to look chosen before it is pressed.
    pub selected: bool,
    /// **Where the control is standing right now**, normalised, for a curve
    /// with no points on it yet.
    ///
    /// An empty lane used to draw NOTHING — an empty stripe with a name
    /// nobody had written, which reads as a lane that is broken rather than
    /// one that is untouched. A flat line at the control's own value says
    /// what automation would do if it were switched on, and is the line the
    /// first alt-click lands on.
    pub resting: f64,
    /// **Armed to WRITE.** Moving the control while the transport rolls lays
    /// points down on this curve. Off by default: a fader nudged during
    /// playback should not silently become automation.
    pub armed: bool,
    /// **Bypassed.** The curve is drawn, kept and editable, and the engine
    /// does not apply it — which is what makes it possible to hear the
    /// difference automation is making without deleting the work.
    pub bypassed: bool,
}

/// **One automation row's header cells**, left to right.
///
/// A track's strip, at half the height and with the questions an automation
/// lane actually has: whether to keep looking at it, whether it is writing,
/// whether it is doing anything, what it controls, and where that control is
/// standing right now.
///
/// Computed by [`auto_header`] and by nothing else — the same rule the rest
/// of this file follows, so a click cannot land somewhere other than what it
/// looks like it is on.
#[derive(Debug, Clone, Copy)]
pub struct AutoHeader {
    /// **×**, far left: stop showing this lane. The same thing the parameter
    /// menu's tick does, put where a hand already is.
    pub close: Rect,
    /// The record dot, drawn exactly as a track's is.
    pub arm: Rect,
    /// **B** — bypass. A letter, like M and S, because it is the same kind of
    /// per-lane switch about signal.
    pub bypass: Rect,
    /// What this lane controls. Whatever is left in the middle.
    pub name: Rect,
    /// The control itself, on the right, so a value can be set without
    /// drawing a point.
    pub dial: Rect,
    /// Its number, ABOVE the dial rather than below as a track's is — half a
    /// row has no room under one, and above is where the eye already is
    /// coming from the name.
    pub readout: Rect,
}

/// Carve an automation row's header into its cells.
///
/// Returns cells that are `Rect::NOTHING` when the row is too short or too
/// narrow to carry them, so a cramped header degrades to the name alone
/// rather than to a row of targets nobody can hit.
pub fn auto_header(header: Rect) -> AutoHeader {
    let none = AutoHeader {
        close: Rect::NOTHING,
        arm: Rect::NOTHING,
        bypass: Rect::NOTHING,
        name: header,
        dial: Rect::NOTHING,
        readout: Rect::NOTHING,
    };
    // **The floor is where a CONTROL stops being one, not where the design
    // stops being comfortable.**
    //
    // It was 12 points tall and 120 wide, and that was wrong by the width of
    // the whole feature: an automation row is half a track, and a track is
    // one eighth of an arrangement that gets whatever the recorder band
    // leaves. Turn the piano, the fretboard and the theory band OFF — which
    // is exactly what somebody working in the arrangement DOES, to get the
    // room — and the stack is 150 points tall, a track is 15, and a row is
    // SEVEN. Every switch, the dial and its number all fell through the floor
    // together and the header drew one dim word on an empty plate. The
    // owner: "no draw of the panels occurs, i can't see any of your added
    // features at all just dead space."
    //
    // Measured, not guessed: `every_control_is_drawn_at_every_height_a_row_
    // can_have` counts the shapes that actually reach the header.
    let h = header.height();
    let w = header.width();
    if !header.is_positive() || h < 7.0 || w < 96.0 {
        return none;
    }
    // Padding and switch size both follow the row, so a short row gets small
    // square switches rather than none.
    let pad = (h * 0.12).clamp(1.0, 3.0);
    let b = (h - pad * 2.0).clamp(6.0, 16.0).min(h - 1.0);
    let cy = header.center().y;
    let cell = |x: f32, wd: f32| {
        Rect::from_center_size(Pos2::new(x + wd * 0.5, cy), Vec2::new(wd, b))
    };
    // The indent is a fraction of the header on a narrow one: a fixed inset
    // that is fine at 208 points is a sixth of the row at 96.
    let mut x = header.left() + FOLDER_INDENT.min(w * 0.06);
    let close = cell(x, b);
    x += b + pad;
    let arm = cell(x, b);
    x += b + pad;
    let bypass = cell(x, b);
    x += b + pad;

    // **The dial is SQUARE, and that is a fix as well as a shape.**
    //
    // Its rect was as wide as the column and as tall as whatever was left,
    // and `dial` sizes its knob from the WIDTH — so on a 36-point row it
    // painted a 34-point circle into a 20-point slot and hung six points into
    // the row below. A square rect cannot do that whatever the row does.
    //
    // The number stacks ABOVE it only when there is room for both. Under
    // about eighteen points there is not, and a row that spent half its
    // height on a percentage would have no dial worth turning — so the dial
    // takes the whole column and the number goes. The dial is the control;
    // the readout is a convenience.
    let stacked = h >= 18.0;
    let col_h = h - 2.0;
    let knob = if stacked { col_h * 0.60 } else { col_h };
    let side = knob.min((w * 0.22).max(14.0)).min(34.0);
    let right = header.right() - pad;
    let dial = Rect::from_center_size(
        Pos2::new(
            right - side * 0.5,
            if stacked { header.bottom() - 1.0 - side * 0.5 } else { cy },
        ),
        Vec2::splat(side),
    );
    let readout = if stacked {
        Rect::from_min_max(
            Pos2::new(dial.left(), header.top() + 1.0),
            Pos2::new(right, dial.top()),
        )
    } else {
        Rect::NOTHING
    };
    let name = Rect::from_min_max(
        Pos2::new(x, header.top()),
        Pos2::new((dial.left() - pad).max(x), header.bottom()),
    );
    if !name.is_positive() {
        return none;
    }
    AutoHeader { close, arm, bypass, name, dial, readout }
}

/// Everything the arrangement draws, pushed in.
pub struct ArrangeView<'a> {
    pub lanes: [LaneView<'a>; CHANNELS],
    /// The MASTER's level — linear, as the desk's fader sits.
    pub master_gain: f32,
    /// What it is making right now, for the slim meter beside the knob.
    pub master_peak: [f32; 2],
    /// Its colour, as an index into `STRIP_COLORS` — the same plate its
    /// mixer strip wears, so the two views agree about what the master is.
    pub master_color: usize,
    /// **The theme's own master red**, `settings.master_knob_color`.
    ///
    /// Sixteen themes, sixteen of these, every one in the red family and
    /// none of them shared — which is what makes the master's dial and its
    /// plate belong to the theme rather than being one fixed crimson laid
    /// over all of them. The owner: "the dial color for master should be
    /// black outline with white strip and colored red. It needs to be per
    /// theme though so all of these must be unique colors that are matched
    /// to the 16 themes."
    pub master_knob: (u8, u8, u8),
    /// **The theme's own accent**, `settings.white_key_active_color`: what
    /// an automation dial wears. See [`auto_dial_body`].
    pub accent: (u8, u8, u8),
    /// Whether the master's row is drawn. `Cmd`/`Ctrl+M`; shown by default.
    pub show_master_row: bool,
    /// The master rack's six short names — empty for an empty bay.
    ///
    /// Six, not three: the master carries two racks. Which of its slots lives
    /// in which is `recorder::seat_bay`'s business and not this panel's, so
    /// what arrives here is already in strip order, top to bottom.
    pub master_inserts: [&'a str; crate::recorder::INSERTS * 2],
    /// The first lane shown — the arrangement has more lanes than fit, so a
    /// window of them shows and this says which. Clamped by the app.
    pub lane_scroll: usize,
    /// How many lanes show at once — Shift+wheel over the headers zooms it.
    pub visible_lanes: usize,
    /// The paywall's number (`TangentApp::active_channels`): how many lanes
    /// the field draws and the scroll can reach. Capacity stays CHANNELS.
    pub active: usize,
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
    /// **The MASTER's own automation rows.**
    ///
    /// Separate from `lanes` because `lanes` is the sixteen CHANNELS and the
    /// master is not one of them — it is a bus, with its own row above them
    /// and its own strip on the desk. Everything else about these rows is a
    /// track's: the same `AutoView`, the same header, the same curve, the
    /// same gestures. They are addressed as lane [`MASTER_LANE`] so that one
    /// `RowKind::Automation` still covers every automation row in the
    /// timeline and the drawing, hit-testing and editing do not fork.
    pub master_automation: Vec<AutoView<'a>>,
    /// A MIDI item being drawn by shift-drag right now: lane, from, to —
    /// ghosted until the button comes up and the document takes it.
    pub pending: Option<(usize, f64, f64)>,
    /// The tempo and meter map get a lane of their own, above the tracks.
    pub show_tempo_lane: bool,
    /// Which tempo point a hand is on, if any. Drawn a size larger, exactly
    /// as an automation point under the pointer is.
    pub tempo_hot: Option<usize>,
    /// A signature chip being typed into, and what is in the field so far.
    pub meter_edit: Option<(u32, &'a str)>,
    /// The zoom window, in seconds of song.
    pub view_start_s: f64,
    pub view_len_s: f64,
    /// The whole song's length (or the window's end past it): the nav
    /// box's outer rectangle, and never shorter than the window.
    pub song_s: f64,
    pub playhead_s: f64,
    /// The ruler's grid, and everything else that has to know where a bar
    /// is: the song's tempo and its meter, as MAPS.
    ///
    /// **Not a bpm and a signature.** They were, and the grid drawn from
    /// them was a ruler of evenly spaced bars laid over a song whose bars
    /// are not evenly spaced the moment anybody puts a tempo change in
    /// it — every line in the wrong place, every clip apparently off the
    /// grid, and snapping (which already went through the maps) landing
    /// somewhere the grid did not draw.
    ///
    /// Owned rather than borrowed because the app builds them per frame out
    /// of the document and the export override; a handful of points cloned
    /// sixty times a second is nothing, and a borrow would mean the caller
    /// keeping a temporary alive across the whole draw.
    pub tempo: crate::tempo::TempoMap,
    pub meter: crate::tempo::MeterMap,
    pub dark_mode: bool,
    /// The header column's width in points, when somebody has dragged the
    /// grip. `None` is the automatic share.
    pub header_w: Option<f32>,
    /// A lane NAME being typed, and what is in the field so far.
    pub naming: Option<(usize, &'a str)>,
    /// The time selection, drawn over lanes and ruler.
    pub time_sel: Option<(f64, f64)>,
    /// The selected channel: its header lights slightly.
    ///
    /// The PRIMARY. Everything selected is in [`Self::selected_tracks`].
    pub selected_channel: Option<usize>,
    /// Every selected channel, all of which wear the rim. See
    /// `recorder_panel::selection_rim`.
    pub selected_tracks: &'a [usize],
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
    /// The arrow beside it: hide this folder's children, or show them.
    HdrCollapse(usize),
    /// One of a track's three insert BAYS — the mixer's own door, here.
    HdrBay(usize, usize),
    /// The Routing button under a lane's dials: open the routing window.
    HdrRouting(usize),
    /// The R dot on a lane's header.
    Arm(usize),
    Mute(usize),
    Solo(usize),
    /// One of the MASTER row's six bays: `(column, slot)`.
    ///
    /// Its own variant rather than `HdrBay(MASTER_COLUMN, ..)`, because every
    /// `HdrBay` handler reads its first field as a strip index — and the
    /// master's column number is a perfectly valid one belonging to somebody
    /// else, so overloading it would have silently edited another rack's
    /// plugins. The slot is in STRIP order; `recorder::seat_bay` turns it into
    /// the rack it actually lives in.
    BusBay(usize, usize),
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
    /// A point on an automation curve, by lane and by index into that
    /// curve's points. Grabbed to drag, or pressed to select.
    /// `(lane, row, point)`.
    AutoPoint(usize, usize, usize),
    /// **The MASTER row's field**, carrying the time under the pointer.
    ///
    /// It is a bus and has no clips, so its whole width was dead ground —
    /// a strip across the timeline that answered nothing. It locates the
    /// playhead now, which is what every other empty surface on this
    /// timeline already does and what a hand crossing it expects.
    MasterLane(f64),
    /// Empty room on an automation lane: `(lane, seconds, value)`. A press
    /// here MAKES a point, which is how every DAW draws one.
    /// `(lane, row, seconds, value)`.
    AutoLane(usize, usize, f64, f64),
    /// The sub-lane's own header: which parameter it shows.
    /// One automation ROW's header — `(lane, row)`.
    /// **Ø** — invert this track's polarity.
    Phase(usize),
    AutoParamPick(usize, usize),
    /// **×** on an automation row: stop showing this lane.
    AutoClose(usize, usize),
    /// The record dot on an automation row: arm it to write.
    AutoArm(usize, usize),
    /// **B**: bypass this curve without deleting it.
    AutoBypass(usize, usize),
    /// The dial on an automation row — sets the control itself.
    AutoDial(usize, usize),
    /// The header's fold arrow: show or hide this lane's automation.
    /// The **A** button on a track's header: open the automation menu.
    AutoMenu(usize),
    /// **T** — send this track's MIDI to the theory view, or stop.
    ToTheory(usize),
    /// A point on the tempo curve, by index. Grabbed to drag.
    TempoPoint(usize),
    /// Empty room on the tempo lane: `(seconds, bpm)`. A press MAKES a
    /// point, the same gesture an automation lane has.
    TempoLane(f64, f64),
    /// The tempo lane's own header: show the song's tempo map, or a button
    /// to put the whole thing back to one tempo.
    TempoHeader,
    /// A time-signature chip on the ruler, by the bar it starts at. A press
    /// opens it for editing.
    MeterChip(u32),
    /// The ruler's chip strip, where there is no chip: `bar`. A press adds a
    /// signature change at that bar.
    MeterStrip(u32),
    ToolMetronome,
    /// The toolbar: the magnet.
    ToolSnap,
    /// The toolbar: show or hide the tempo lane.
    ToolTempoLane,
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
    /// A BUS row's level — the master's or the click's. Carries the MIXER
    /// COLUMN, so the drag goes through the desk's own fader hit rather
    /// than a second road to the same number. See [`Bus::column`].
    BusGain(usize),
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

/// The same for an automation sub-lane: a curve needs less room than the
/// clips it belongs under.
///
/// **Exactly HALF, and the half is load-bearing now.** It was 0.55, justified
/// by a rule that no longer holds — "the track it hangs off must not shrink
/// to make space for it" — and the owner replaced that rule with this one:
/// every half strip of automation is absorbed by the tracks getting shorter,
/// and every WHOLE strip's worth costs one track its place in the window.
///
/// Two lanes have to be exactly one track for that to work. At 0.55 they are
/// 1.1, so two lanes evicted a track AND left the rest a tenth short — the
/// heights never came back to where they started and the arithmetic drifted
/// further with every lane. See
/// `automation_rows_shrink_the_tracks_then_evict_one_and_repeat`.
const AUTO_LANE_SHARE: f32 = 0.5;

/// The master row's height, as a fraction of a track's.
///
/// **Half, at the owner's word.** It carries a name, a level and a rack and
/// none of the things that make a track tall — no clips land on it, and there
/// is no waveform to look at. Every point it does not take is a point the
/// tracks get back.
const MASTER_ROW_SHARE: f32 = 0.5;

/// The MIXER COLUMN the master is, so its level goes through the desk's own
/// fader hit rather than a second road to the same number. See
/// `recorder::Column::desk_index`, where the convention is written down.
pub const MASTER_COLUMN: usize = CHANNELS + 1;

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
    /// An automation curve under the track it belongs to. `usize` is the
    /// channel; the second is WHICH of its open rows, counting down from the
    /// track — a lane can show several parameters at once now.
    Automation(usize, usize),
    /// **The MASTER, above the tracks.**
    ///
    /// The desk draws a strip for it and the timeline drew none, so a
    /// session's output level was reachable from one view and not the other.
    /// A FULL track's height, wearing the mixer's own colours, and it cannot
    /// be removed — a session always has an output.
    ///
    /// **The CLICK had a row here and lost it.** It was tried at half height
    /// beside the master, and the owner's call was that the metronome is not
    /// a thing you arrange against; the desk is where its level lives.
    ///
    /// `Cmd`/`Ctrl+M` hides it. Shown by default, because a control nobody
    /// can find is a control nobody has.
    Master,
}

/// **How far a track inside a folder steps to the right.**
///
/// Small on purpose. The indent is the ONLY thing that says "contained", so
/// it has to be legible at a glance — but the header is already tight enough
/// that the shedding rules exist, and every point spent here is a point the
/// name gives up. Ten reads clearly beside an un-indented neighbour and costs
/// the title roughly one character.
pub const FOLDER_INDENT: f32 = 10.0;

/// **The lane number the MASTER's automation rows answer to.**
///
/// One past the channels, so it can never collide with one, and so a single
/// `RowKind::Automation(lane, slot)` still describes every automation row on
/// the timeline. The alternative was a second row kind, which would have
/// forked the layout, the paint, the hit test, the selection, the clipboard
/// and the keyboard — six places to keep in step for one bus.
pub const MASTER_LANE: usize = CHANNELS;

/// The `AutoView` for a row, wherever it lives.
///
/// One lookup, because `lanes` holds the channels and the master's rows sit
/// beside it — and a caller that reached for `lanes[lane]` directly would
/// silently find nothing for the master and draw an empty row.
pub fn auto_view_of<'a>(
    view: &'a ArrangeView<'a>,
    lane: usize,
    slot: usize,
) -> Option<&'a AutoView<'a>> {
    if lane == MASTER_LANE {
        return view.master_automation.get(slot);
    }
    // The CHANNELS' own table. Deliberately spelled out rather than routed
    // back through this function — that is infinite recursion, and was.
    view.lanes.get(lane).and_then(|l| l.automation.get(slot))
}

/// How many automation rows a lane is showing, master included.
fn auto_rows_of(view: &ArrangeView<'_>, lane: usize) -> usize {
    if lane == MASTER_LANE {
        return view.master_automation.len();
    }
    view.lanes.get(lane).map_or(0, |l| l.automation.len())
}

/// **How much of a folder's row counts as its bottom EDGE** — the strip a
/// track is dropped on to snap into it, as a fraction of the row's height.
///
/// A quarter. Small enough that an ordinary reorder onto a folder row still
/// reorders, which is the gesture somebody arriving without knowing about
/// folders will make; large enough to hit without aiming, which is the
/// gesture somebody who knows will make.
pub const FOLDER_DROP_EDGE: f32 = 0.25;

/// **The strip along a row's bottom that takes a dropped track INTO it.**
///
/// One function because three things need the same rectangle — the hit that
/// arms it, the paint that shows it, and the drop that acts on it — and a
/// drop zone you can see in one place and hit in another is worse than none.
///
/// It had no paint at all: the gesture existed, was documented, and was
/// invisible, so the only feedback during a drag was the row highlight at the
/// TOP of the target. The owner: "the highlight edge shows ATOP the folder or
/// last child instead of where it's ATTACHING to, the bottom."
pub fn folder_drop_strip(row: Rect) -> Rect {
    if !row.is_positive() {
        return Rect::NOTHING;
    }
    let h = (row.height() * FOLDER_DROP_EDGE).max(2.0);
    Rect::from_min_max(Pos2::new(row.left(), row.bottom() - h), row.max)
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
    /// The top sliver of the ruler, where the time-signature chips sit.
    ///
    /// Carved OUT of the ruler rather than added above it: the ruler already
    /// has a bar-number row and a clock row, and a third band would push the
    /// lanes down to say something that belongs beside the bar numbers. The
    /// chips take the top third, where nothing was drawn.
    pub meter_strip: Rect,
    /// The corner cell's toolbar: metronome, magnet, grid box.
    pub tool_met: Rect,
    pub tool_snap: Rect,
    pub tool_grid: Rect,
    /// The tempo lane's switch: the row above the tracks, on and off.
    pub tool_tempo: Rect,
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
    /// The master's header cells. `NONE` when the row is hidden.
    pub master_row: BusRow,
}


/// A bus row's header: what it is, and the one control it has.
///
/// **A name and a level, and nothing else.** A track's header carries an
/// arm, a mute, a solo, three dials and a rack because a track is a source;
/// these two are the ends of the signal path and the only thing anybody
/// reaches for on either is how loud it is. Half a track's height would not
/// hold the rest anyway.
#[derive(Debug, Clone, Copy)]
pub struct BusRow {
    pub panel: Rect,
    pub name: Rect,
    pub knob: Rect,
    /// The word "VOL" beside the dial. The track rows write their dial's
    /// name under it; this row is half a track tall and has no room below,
    /// so it sits to the left of the meter instead.
    pub vol_label: Rect,
    pub meter: Rect,
    /// The master's rack, in strip order. `Rect::NOTHING` where the row is too
    /// short or too narrow to carry one — the same rule the track headers use,
    /// so a rack that cannot be read is not drawn rather than drawn as a smudge.
    pub bays: [Rect; crate::recorder::INSERTS * 2],
    /// **A**, the automation menu — the same button a track header wears and
    /// in the same place relative to the name, because the master's rows are
    /// a track's rows and the way to them should not be a different gesture.
    pub automation: Rect,
    /// The dial's number, ABOVE it — a track writes its own under the knob,
    /// and this row is half a track tall with nothing below.
    pub knob_readout: Rect,
}

impl BusRow {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        name: Rect::NOTHING,
        knob: Rect::NOTHING,
        vol_label: Rect::NOTHING,
        meter: Rect::NOTHING,
        bays: [Rect::NOTHING; crate::recorder::INSERTS * 2],
        automation: Rect::NOTHING,
        knob_readout: Rect::NOTHING,
    };

    fn of(row: Rect) -> Self {
        let r = row.shrink2(Vec2::new(3.0, 1.0));
        if !r.is_positive() {
            return Self { panel: row, ..Self::NONE };
        }
        let h = r.height();
        // The knob keeps the right end, the meter stands inside it, and the
        // name takes what is left — the track header's own order, so the two
        // kinds of row read as one list.
        // **The dial and its number share a column.** The master had a dial
        // with no reading at all — the one fader on the desk that can undo
        // every other one, and no way to see where it was standing without
        // opening the mixer. The owner asked for a readout atop it; a track
        // writes its own UNDER the knob, and this row is half a track tall
        // with nothing below, so it goes above.
        //
        // The number is shed first when the row is short: a dial you can
        // turn beats a number you cannot read.
        let col_w = (h * 0.86).clamp(9.0, 20.0);
        let col = Rect::from_min_max(
            Pos2::new(r.right() - col_w - 2.0, r.top()),
            Pos2::new(r.right() - 2.0, r.bottom()),
        );
        let stacked = h >= 18.0;
        let split = if stacked { col.top() + col.height() * 0.38 } else { col.top() };
        let knob_readout = if stacked {
            Rect::from_min_max(col.min, Pos2::new(col.right(), split))
        } else {
            Rect::NOTHING
        };
        let d = (col.bottom() - split).min(col_w).max(9.0);
        let knob = Rect::from_center_size(
            Pos2::new(col.center().x, (split + col.bottom()) * 0.5),
            Vec2::splat(d),
        );
        let meter = Rect::from_min_max(
            Pos2::new(knob.left() - 8.0, r.top() + h * 0.18),
            Pos2::new(knob.left() - 3.0, r.bottom() - h * 0.18),
        );
        // Shed below a width that cannot carry the word, like every other
        // label on this panel.
        let vol_label = {
            let w = 22.0_f32;
            let left = meter.left() - 3.0 - w;
            if left > r.left() + 30.0 {
                Rect::from_min_max(
                    Pos2::new(left, r.top() + h * 0.18),
                    Pos2::new(meter.left() - 3.0, r.bottom() - h * 0.18),
                )
            } else {
                Rect::NOTHING
            }
        };
        // **The name keeps the left, the rack takes the middle.** A track's
        // header stacks its name over its bays; this row is half a track tall
        // and has no room to stack anything, so the two sit side by side —
        // which is also how the mixer strip reads left to right.
        let left = Rect::from_min_max(
            Pos2::new(r.left(), r.top()),
            Pos2::new(
                if vol_label.is_positive() { vol_label.left() } else { meter.left() } - 4.0,
                r.bottom(),
            ),
        );
        let n = crate::recorder::INSERTS * 2;
        let name_w = (left.width() * 0.28).min(70.0);
        let full_name = Rect::from_min_max(
            left.min,
            Pos2::new(left.left() + name_w, left.bottom()),
        );
        // The A button takes the right end of the name cell, square and
        // centred on the row — the same shape it has on a track, so a hand
        // that has learned one has learned this. It sheds when the row is
        // too small to carry a readable letter, like every other switch here.
        let side = (h * 0.5).clamp(8.0, 14.0);
        let (name, automation) = if full_name.width() > side + 24.0 {
            (
                Rect::from_min_max(
                    full_name.min,
                    Pos2::new(full_name.right() - side - 3.0, full_name.bottom()),
                ),
                Rect::from_center_size(
                    Pos2::new(full_name.right() - side * 0.5, full_name.center().y),
                    Vec2::splat(side),
                ),
            )
        } else {
            (full_name, Rect::NOTHING)
        };
        let rack = Rect::from_min_max(
            Pos2::new(full_name.right() + 4.0, left.top() + h * 0.14),
            Pos2::new(left.right(), left.bottom() - h * 0.14),
        );
        let bays = std::array::from_fn(|b| {
            let w = (rack.width() - 2.0 * (n as f32 - 1.0)) / n as f32;
            // Below this a chip carries no readable word, and six unreadable
            // chips are worse than none: they take the presses a rack would
            // and say nothing about what is in them.
            if w < 14.0 || !rack.is_positive() {
                return Rect::NOTHING;
            }
            let x0 = rack.left() + b as f32 * (w + 2.0);
            Rect::from_min_max(
                Pos2::new(x0, rack.top()),
                Pos2::new(x0 + w, rack.bottom()),
            )
        });
        Self { panel: row, name, knob, vol_label, meter, bays, automation, knob_readout }
    }
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
    /// **Ø — polarity**, directly above the mute, and small.
    ///
    /// Above rather than beside: the row is already M and S wall-to-wall at
    /// the right end, and this is a different kind of switch — M and S are
    /// about where a track goes, this is about the waveform itself. Set once
    /// when two mics disagree and then left alone.
    pub phase: Rect,
    pub solo: Rect,
    /// **T** — send this track's MIDI to the theory view.
    ///
    /// Beside the mute and the solo because it is the same kind of thing: a
    /// per-track switch about where the track goes. Same size, so a hand
    /// that has learned M and S finds it without looking.
    pub theory: Rect,
    /// **A** — this track's automation lanes. Directly under the T, because
    /// the two are the same kind of question about the same track and a hand
    /// that has found one has found the other.
    pub automation: Rect,
    /// The input chip, under the name — every lane wears one now: binding
    /// an input is how a bare lane BECOMES a track.
    pub input: Rect,
    /// The insert BAYS — the mixer's slots, reachable from here. Six wide
    /// for a FOLDER lane, three (and three NOTHINGs) for the rest.
    pub bays: [Rect; crate::recorder::INSERTS * 2],
    /// The Routing button, under the dials. `NOTHING` where they are.
    pub routing: Rect,
    /// The circled plus, on empty lanes alone.
    pub add: Rect,
    /// **The collapse arrow**, on a FOLDER only, immediately right of the box.
    /// `NOTHING` on every other track.
    ///
    /// A second target rather than a second meaning for the box: the box is
    /// "is this a group", the arrow is "am I looking at it". One control that
    /// did both would mean un-collapsing a folder to stop it being one.
    pub collapse: Rect,
    /// **The FOLDER box, at the header's bottom-left.**
    ///
    /// Bottom-left because it is the only corner nothing else claims — the
    /// number and the arm keep fixed seats at the top-left, the dials and
    /// the switches own the right, and the input chip runs under the name.
    /// It is also where a hand goes looking for structure rather than for a
    /// control: below the track's own row of things, at the edge the indent
    /// is measured from.
    pub folder: Rect,
}

impl HeaderRow {
    const NONE: Self = Self {
        panel: Rect::NOTHING,
        number: Rect::NOTHING,
        arm: Rect::NOTHING,
        theory: Rect::NOTHING,
        automation: Rect::NOTHING,
        folder: Rect::NOTHING,
        collapse: Rect::NOTHING,
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
        phase: Rect::NOTHING,
        solo: Rect::NOTHING,
        input: Rect::NOTHING,
        bays: [Rect::NOTHING; crate::recorder::INSERTS * 2],
        add: Rect::NOTHING,
    };
}

impl Layout {
    pub fn new(rect: Rect, view: &ArrangeView<'_>) -> Self {
        if !rect.is_positive() {
            return Self {
                header: Rect::NOTHING,
                ruler: Rect::NOTHING,
                meter_strip: Rect::NOTHING,
                tool_met: Rect::NOTHING,
                tool_snap: Rect::NOTHING,
                tool_grid: Rect::NOTHING,
                tool_tempo: Rect::NOTHING,
                tool_nav: Rect::NOTHING,
                lanes_area: Rect::NOTHING,
                grip: Rect::NOTHING,
                rows: Vec::new(),
                lane_rows: [Rect::NOTHING; CHANNELS],
                header_rows: [HeaderRow::NONE; CHANNELS],
                master_row: BusRow::NONE,
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
        // Only when there is a change to show: on a song in one signature —
        // which is nearly all of them — the strip is nothing and the bar
        // numbers keep the whole ruler.
        let meter_strip = if view.meter.is_constant() {
            Rect::NOTHING
        } else {
            let h = (ruler_h * 0.34).clamp(8.0, 13.0).min(ruler_h * 0.5);
            Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), ruler.top() + h))
        };
        let lanes_area = Rect::from_min_max(
            Pos2::new(field_x, rect.top() + ruler_h),
            rect.max,
        );
        // **Eight lanes show; the rest scroll.** The desk grew past what
        // fits at a legible height, so the height is fixed to `VISIBLE_LANES`
        // and `view.lane_scroll` chooses which window shows. A lane outside
        // the window is `NOTHING` — not drawn, not hit, and clips on it
        // resolve to nothing through `clip_rect`'s `is_positive` filter.
        let active = view.active.clamp(1, CHANNELS);
        let visible = view.visible_lanes.clamp(1, active);
        // **The bus rows are ADDED to the stack, not taken out of it.**
        //
        // The master and the click are half a track each — one track's
        // height between them — and dividing the area by `visible` alone
        // would have spent that height on them, so a window set to show one
        // lane showed none at all. `visible` still means TRACKS; the pair
        // costs every track a slice of its height instead, which is the same
        // bargain an open automation row already makes.
        let first = view.lane_scroll.min(active.saturating_sub(visible));
        // **Automation rows shrink the tracks until they have cost a whole
        // one, and then a track leaves.**
        //
        // The divisor counted the tracks and the master's half and nothing
        // else, so opening a curve made the stack taller than the area —
        // and `push` drops a row that does not WHOLLY fit, which left half a
        // strip of dead space at the bottom with a track missing. That is
        // the owner's "leaves half a strip of empty space at the bottom".
        //
        // The rule he asked for, and it scales: eight tracks, open a volume
        // lane and all eight shrink to make room for the extra half strip;
        // open a pan lane on the same track and the two halves are a whole
        // track's worth, so the last channel leaves the window and the rest
        // return to full height. Three lanes shrink again, four evict a
        // second, and so on.
        //
        // `visible - floor(share)` is that rule exactly, and the remainder
        // rides in the divisor so the stack always ends flush with the
        // bottom — no dead space at any count.
        let auto_of = |shown: usize| -> f32 {
            // The master's rows are counted whatever the scroll is doing:
            // the master row itself is always at the top, so its automation
            // is always on screen and always costs its half strip.
            let master = if view.show_master_row {
                view.master_automation.len() as f32
            } else {
                0.0
            };
            ((first..CHANNELS)
                .filter(|i| !view.lanes.get(*i).is_some_and(|l| l.hidden))
                .take(shown)
                .map(|i| view.lanes.get(i).map_or(0, |l| l.automation.len()) as f32)
                .sum::<f32>()
                + master)
                * AUTO_LANE_SHARE
        };
        // Settles: evicting a track can only REMOVE automation from the
        // count, which can only let tracks back in, so `shown` walks down
        // and stops. Bounded by the channel count regardless.
        let mut shown = visible;
        let mut auto_share = auto_of(shown);
        for _ in 0..CHANNELS {
            let want = visible.saturating_sub(auto_share.floor() as usize).max(1);
            if want >= shown {
                break;
            }
            shown = want;
            auto_share = auto_of(shown);
        }
        let lane_h = lanes_area.height()
            / (shown as f32
                + auto_share
                + if view.show_tempo_lane { TEMPO_LANE_SHARE } else { 0.0 }
                + if view.show_master_row { MASTER_ROW_SHARE } else { 0.0 });

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
        // **The master and the click, above the channels.** Half a track
        // each, so the two together take exactly one track's height and the
        // pair reads as one band rather than as two short tracks.
        if view.show_master_row {
            push(RowKind::Master, lane_h * MASTER_ROW_SHARE, &mut y, &mut rows);
            // **Its automation hangs under it**, exactly as a track's does,
            // and by the same row kind — see `MASTER_LANE`.
            for slot in 0..view.master_automation.len() {
                push(
                    RowKind::Automation(MASTER_LANE, slot),
                    lane_h * AUTO_LANE_SHARE,
                    &mut y,
                    &mut rows,
                );
            }
        }
        for i in first..active {
            // A track inside a SHUT folder is not laid out at all: no row, so
            // nothing is drawn, nothing is hit, and its clips resolve to
            // nothing through `clip_rect`'s own `is_positive` filter — the
            // same road a scrolled-off lane already takes.
            if view.lanes.get(i).is_some_and(|l| l.hidden) {
                continue;
            }
            push(RowKind::Track(i), lane_h, &mut y, &mut rows);
            // One row per parameter this lane is showing, in the order the
            // view lists them — which is the order the menu offers them, so
            // the rows and the menu read the same way down.
            let shown = auto_rows_of(view, i);
            for slot in 0..shown {
                push(
                    RowKind::Automation(i, slot),
                    lane_h * AUTO_LANE_SHARE,
                    &mut y,
                    &mut rows,
                );
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
        let master_row = rows
            .iter()
            .find(|r| r.kind == RowKind::Master)
            .filter(|r| r.header.is_positive())
            .map_or(BusRow::NONE, |r| BusRow::of(r.header));
        let lane_rows = std::array::from_fn(lane_of);
        let header_rows = std::array::from_fn(|i| {
            let row = header_of(i);
            let Some(lane) = view.lanes.get(i).filter(|_| row.is_positive()) else {
                return HeaderRow::NONE;
            };
            Self::header_row(
                row,
                lane.used,
                if lane.in_folder { FOLDER_INDENT } else { 0.0 },
                lane.is_folder,
            )
        });
        // **The toolbar, in the corner cell** — the room above the track
        // list, before the ruler's first tick. Four small controls: the
        // click, the magnet, the tempo lane's switch, and the grid box the
        // wheel turns.
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
        let (tool_met, tool_snap, tool_tempo) = (cell(0.0), cell(1.0), cell(2.0));
        let tool_grid = Rect::from_min_max(
            Pos2::new(tool_tempo.right() + 3.0, corner.top()),
            Pos2::new(
                (tool_tempo.right() + 3.0 + side * 2.4).min(corner.right()),
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
            meter_strip,
            header,
            ruler,
            tool_met,
            tool_snap,
            tool_grid,
            tool_tempo,
            tool_nav,
            lanes_area,
            grip: Rect::from_min_max(
                Pos2::new(rect.left() + header_w, rect.top()),
                Pos2::new(rect.left() + header_w + GRIP_W, rect.bottom()),
            ),

            rows,
            lane_rows,
            header_rows,
            master_row,
        }
    }

    /// The cells across one header, REAPER's row at Tangent's density:
    /// number, arm dot, name, gain knob, slim meter, M/S.
    fn header_row(row: Rect, used: bool, indent: f32, folder: bool) -> HeaderRow {
        // **The indent moves the CONTENTS, and `panel` keeps the whole row.**
        //
        // The panel is what a press is tested against and what the plate is
        // drawn from; indenting it would make the strip of wood on the left
        // dead to the pointer, so a track's own row would stop answering
        // along the edge that says it is in a folder. The contents step
        // right; the row is still the row.
        let r = row.shrink2(Vec2::new(3.0, 2.0));
        let r = if indent > 0.0 && r.width() > indent + 40.0 {
            Rect::from_min_max(Pos2::new(r.left() + indent, r.top()), r.max)
        } else {
            r
        };
        if !r.is_positive() {
            return HeaderRow { panel: row, ..HeaderRow::NONE };
        }
        // An unused lane is a place, not a channel — but it DOES wear the
        // input chip: binding an input is how a bare lane becomes a track.
        if !used {
            // The + circle first — pressing it makes the track — then the
            // chip beside it for binding straight away.
            // Clear of the number's top-left seat, whatever the row
            // height: at a squeezed row the old full-height centre put the
            // plus on top of the lane's own number.
            let d = (r.height() * 0.44).clamp(16.0, 26.0).min((r.height() - 16.0).max(12.0));
            let add = Rect::from_center_size(
                Pos2::new(
                    r.left() + r.width() * 0.10,
                    r.center().y.max(r.top() + 13.0 + d * 0.5),
                ),
                Vec2::splat(d),
            );
            let ch_h = (r.height() * 0.30).clamp(12.0, 16.0);
            let input = Rect::from_min_size(
                Pos2::new(add.right() + 6.0, r.center().y - ch_h * 0.5),
                Vec2::new(r.width() * 0.40, ch_h),
            );
            // **And the rest of the row is the slot itself.** An empty lane
            // had no `name` and no `number`, so `hit_test` could not name it
            // and a press on it produced nothing — which meant an empty slot
            // could not be SELECTED, so it could not wear the rim and could
            // not be a paste destination you had pointed at. The + circle and
            // the input chip keep their own seats; what is left over answers
            // for the slot.
            let name = Rect::from_min_max(
                Pos2::new(input.right() + 6.0, r.top()),
                Pos2::new(r.right(), r.bottom()),
            );
            let name = if name.width() > 4.0 { name } else { Rect::NOTHING };
            return HeaderRow { panel: row, input, add, name, ..HeaderRow::NONE };
        }
        let h = r.height();
        // The number and the arm keep FIXED seats at the left — the
        // proportional cuts walked the dot into the track title whenever
        // the header widened.
        let number = Rect::from_min_max(r.min, Pos2::new(r.left() + 12.0, r.bottom()));
        // The arm's GEOMETRY stands even where the arm does not: the
        // theory/automation pair anchors off its right edge, so a folder -
        // which cannot record and returns NOTHING for the field - still
        // needs the rect for the chain.
        let arm_seat = {
            let d = (h * 0.42).clamp(9.0, 15.0);
            Rect::from_center_size(
                Pos2::new(number.right() + 4.0 + d * 0.5, r.center().y),
                Vec2::splat(d),
            )
        };
        let arm = if folder { Rect::NOTHING } else { arm_seat };
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
        // Polarity, above the mute and two thirds its size — shed entirely
        // when the row is too short for it to sit clear of the mute's top
        // edge, rather than drawn on top of it.
        let phase = {
            let d = (sw * 0.62).clamp(8.0, 12.0);
            let top = mute.top() - 1.5 - d;
            if d >= 8.0 && top >= r.top() + 1.0 {
                Rect::from_min_size(Pos2::new(mute.center().x - d * 0.5, top), Vec2::splat(d))
            } else {
                Rect::NOTHING
            }
        };
        let meter = Rect::from_min_max(
            Pos2::new(mute.left() - 8.0, r.top() + h * 0.15),
            Pos2::new(mute.left() - 3.0, r.bottom() - h * 0.15),
        );
        // **T sits beside the ARM, not beside the mute.**
        //
        // Its first home was left of the M/S pair, which took its width out
        // of the meter and the dials — and at the header widths this app
        // opens at, the volume knob simply stopped fitting. The left end has
        // the room: the arm dot and the number keep fixed seats there and
        // everything to their right is proportional, so a switch after the
        // dot costs the NAME a character and costs no control anything.
        //
        // It also reads better there. The arm decides whether this track
        // records what you play; this decides whether what you play reaches
        // the chord readout. They are two questions about the same signal.
        //
        // **T and A are a PAIR, stacked, straddling the centre.** T alone sat
        // on the row's middle; putting A under it and leaving T where it was
        // would push the pair off-centre, so the two are placed together and
        // the centre falls between them.
        let (theory, automation) = {
            let d = (h * 0.34).clamp(8.0, 13.0);
            let x = arm_seat.right() + 3.0 + d * 0.5;
            let gap = 1.5;
            let top = r.center().y - d - gap * 0.5;
            let mk = |y: f32| Rect::from_min_size(Pos2::new(x - d * 0.5, y), Vec2::splat(d));
            // Shed together and below the same floor: an A with no T beside
            // it is a switch nobody would recognise.
            if d >= 8.0 && r.height() >= d * 2.0 + gap {
                (mk(top), mk(top + d + gap))
            } else {
                let one = (h * 0.42).clamp(9.0, 15.0);
                (
                    Rect::from_center_size(
                        Pos2::new(arm_seat.right() + 3.0 + one * 0.5, r.center().y),
                        Vec2::splat(one),
                    ),
                    Rect::NOTHING,
                )
            }
        };
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
        let dials_fit = pan.left() - 6.0 - (arm_seat.right() + 4.0) >= COL_MIN;
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
            // After the T switch, which sits between the arm and the name.
            Pos2::new(theory.right() + 4.0, r.top()),
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
        // **THREE ON THREE for a folder** - the owner's ruling: a folder
        // is a grouper, not a track, so it has no input to select, and the
        // second rack takes the input selector's band. Bays 0..2 sit where
        // every track's bays sit; 3..5 ride the band above. A plain track
        // keeps its three across the bays band, as ever.
        let three = |band: Rect, offset: usize, out: &mut [Rect; crate::recorder::INSERTS * 2]| {
            let n = crate::recorder::INSERTS;
            let w = (band.width() - 3.0 * (n as f32 - 1.0)) / n as f32;
            if w < 16.0 || !band.is_positive() {
                return;
            }
            for b in 0..n {
                let x0 = band.left() + b as f32 * (w + 3.0);
                out[offset + b] = Rect::from_min_max(
                    Pos2::new(x0, band.top()),
                    Pos2::new(x0 + w, band.bottom()),
                );
            }
        };
        let mut bays = [Rect::NOTHING; crate::recorder::INSERTS * 2];
        if folder {
            if bays_row.is_positive() {
                three(bays_row, 0, &mut bays);
                three(input, 3, &mut bays);
            } else {
                // The squeezed header keeps one band: the FIRST rack takes
                // it, the second sheds - the same order a track's controls
                // shed in.
                three(input, 0, &mut bays);
            }
        } else {
            three(bays_row, 0, &mut bays);
        }
        // And the folder sheds the selector and the arm whole: nothing to
        // choose, nothing to record.
        let input = if folder { Rect::NOTHING } else { input };
        // **The folder box: bottom-left, under the number.**
        //
        // Sized off the row like every other switch, and SHED like them too —
        // a header squeezed short enough that the box would collide with the
        // number's own cell gets no box rather than an overlapping one. The
        // shedding order this file already keeps drops dials, then bays, then
        // the chip, never the name; this joins at the front of that list
        // because it is structure rather than a control you reach for while
        // playing.
        // **The BOX retired** - the owner's ruling: a folder is made and
        // unmade by Cmd/Ctrl+F alone, so the toggle square is gone and the
        // collapse ARROW takes its old bottom-left seat. Folder lanes only:
        // an arrow on a plain track is a control for a thing that is not
        // there.
        let collapse = if folder {
            let d = (h * 0.30).clamp(8.0, 12.0);
            let top = r.bottom() - d;
            if top > number.top() + 2.0 && d >= 8.0 {
                Rect::from_min_size(Pos2::new(r.left(), top), Vec2::splat(d))
            } else {
                Rect::NOTHING
            }
        } else {
            Rect::NOTHING
        };
        let folder_box = Rect::NOTHING;
        HeaderRow {
            theory,
            automation,
            folder: folder_box,
            collapse,
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
            phase,
            solo,
            input,
            bays,
            routing,
            add: Rect::NOTHING,
        }
    }
}
/// **One dial**, the timeline's own: a ring, a coloured cap, and a needle
/// through 270 degrees from the seven-o'clock position.
///
/// A free function rather than the closure it used to be, because the MASTER
/// wants exactly this control and is drawn before the header loop the closure
/// lived in. It was a white bar in a slot up there — the one control on the
/// timeline that looked like nothing else on it — and the owner's word is
/// that the master's level is a level like any other.
fn dial(painter: &Painter, cell: Rect, t: f32, cap: Color32, ring: Color32, needle: Color32) {
    if !cell.is_positive() {
        return;
    }
    let kc = cell.center();
    let kr = cell.width() * 0.5;
    painter.circle_filled(kc, kr, ring);
    painter.circle_filled(kc, kr * 0.82, cap);
    let sweep = std::f32::consts::PI * 1.5;
    let a = std::f32::consts::PI * 0.75 + sweep * t.clamp(0.0, 1.0);
    painter.line_segment(
        [kc, Pos2::new(kc.x + a.cos() * kr * 0.8, kc.y + a.sin() * kr * 0.8)],
        Stroke::new((kr * 0.22).max(1.5), needle),
    );
}

/// The blue a VOLUME dial wears, wherever one is drawn.
const DIAL_VOLUME: Color32 = Color32::from_rgb(0x1C, 0x6F, 0xD6);

/// **An automation dial wears the THEME's accent, not one fixed blue.**
///
/// Every automation row on every theme drew the same `DIAL_VOLUME` — a blue
/// belonging to the Default palette and to no other — so sixteen themes had
/// one dial between them. The owner: "I want a true color picked per theme
/// freshly for the automation dials."
///
/// Picked, not stated: the theme's own accent is the colour, and it is moved
/// only as far as it has to be to stay legible where it is actually drawn —
/// the cap has to read against the header face it sits on, and the needle
/// drawn ACROSS it has to read against the cap. That last pairing is the one
/// this app keeps getting wrong: ink is a property of the surface, and a
/// dial whose colour was chosen against the lane field is a dial nobody
/// checked against the header.
///
/// Walking outward from the accent rather than jumping to a safe value means
/// a theme that is already legible keeps its colour exactly, and one that is
/// not gives up the least brightness that works. If nothing does — a
/// near-white accent on a near-white face — the ink for that face is the
/// honest answer: it is guaranteed to read, and a dial you can see in the
/// wrong colour beats one you cannot see in the right one.
fn auto_dial_body(p: &Ink, accent: (u8, u8, u8)) -> Color32 {
    let face = shade(p.hdr_face, 0.92);
    let ok = |c: Color32| {
        crate::recorder_panel::contrast_ratio(c, face) >= 1.8
            && crate::recorder_panel::contrast_ratio(p.clip_ink, c) >= 2.0
    };
    let base = Color32::from_rgb(accent.0, accent.1, accent.2);
    if ok(base) {
        return base;
    }
    // Outward in both directions, nearest first, so the answer is the
    // smallest change to the theme's own colour.
    for step in 1..=24 {
        let d = step as f32 * 0.05;
        for k in [1.0 - d, 1.0 + d] {
            if k <= 0.0 {
                continue;
            }
            let c = shade(base, k);
            if ok(c) {
                return c;
            }
        }
    }
    on_ink(face)
}

/// The MASTER's cap. Black, and the only dial on the timeline that is.
///
/// Not pure black: the needle drawn across it is the row's own ink, and on a
/// dark theme an ink-on-black needle is a needle nobody can find. This is
/// dark enough to read as black beside the channels' blue and light enough
/// that the needle still stands off it.
const DIAL_MASTER: Color32 = Color32::from_rgb(0x14, 0x13, 0x12);

/// The master dial's OUTLINE and its needle. Fixed on every theme — see the
/// note where they are used, and `bus_strip`, which makes the same bargain.
const DIAL_MASTER_RING: Color32 = Color32::from_rgb(0x0E, 0x0D, 0x0C);
const DIAL_MASTER_NEEDLE: Color32 = Color32::from_rgb(0xF2, 0xEE, 0xE4);

/// **The master dial's body: the theme's red, deep enough to carry a white
/// needle.**
///
/// The needle is fixed white on purpose — it is the instrument, not the
/// panel — and two of the sixteen themes bring a coral pale enough that a
/// white line across it reads at 2.07:1, which is a needle you have to look
/// for. Darkening the red until the needle clears keeps both halves of what
/// was asked for: the colour is still the theme's own and still red, and the
/// stripe is still white and still visible.
///
/// Walked rather than clamped to one shade, so a theme that already has a
/// deep red is left exactly as it drew it.
fn master_dial_body(knob: (u8, u8, u8)) -> Color32 {
    let mut c = Color32::from_rgb(knob.0, knob.1, knob.2);
    for _ in 0..12 {
        if crate::recorder_panel::contrast_ratio(DIAL_MASTER_NEEDLE, c) >= 2.3 {
            break;
        }
        c = shade(c, 0.92);
    }
    c
}


/// One bus row's level dial: the track headers' own dial, drawn from a
/// free function because the bus rows are painted before the header loop
/// that owns the closure version.
/// The master's level, as a black slot with a white bar in it.
///
/// **Not a dial**, at the owner's word. The master's plate is its own colour —
/// red by default — and a dial drawn in the plate's ink sat on that red as one
/// more round red thing. A slot reads as a level at a glance and at half a
/// track's height, where a dial that small is a dot with a scratch on it.
///
/// The two colours are FIXED, the way `VU_FACE` is and for the same reason:
/// this is an instrument let into the panel rather than a region of it, and a
/// near-black slot with a near-white bar means the same thing on all sixteen
/// themes. They are not the theme's ink, so they cannot be dragged dark-on-dark
/// by a plate somebody painted.
fn bus_strip(painter: &Painter, cell: Rect, t: f32) {
    /// The slot, and the bar in it.
    const SLOT: Color32 = Color32::from_rgb(0x0E, 0x0D, 0x0C);
    const BAR: Color32 = Color32::from_rgb(0xF2, 0xEE, 0xE4);
    let slot = cell.shrink2(Vec2::new(cell.width() * 0.18, 1.5));
    if !slot.is_positive() {
        return;
    }
    painter.rect_filled(slot, 1.5, SLOT);
    // Fills upward, the way a fader reads.
    let h = slot.height() * t.clamp(0.0, 1.0);
    if h > 0.5 {
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(slot.left() + 1.0, slot.bottom() - h),
                Pos2::new(slot.right() - 1.0, slot.bottom() - 1.0),
            ),
            1.0,
            BAR,
        );
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

/// **Where a point sits inside an automation row, and its inverse.**
///
/// Two functions, one truth. The drawing and the hit test are the classic
/// pair that drift: one gains an inset and the other does not, and the
/// result is a curve you can see but cannot grab — worse near the top and
/// bottom, which is exactly where the interesting values are. So the pixels
/// come from here in both directions and nowhere else.
///
/// The inset is why it matters at all: a value of 1.0 drawn ON the row's top
/// edge is half a point tall and clipped by the row above, so the usable
/// band is inset by the radius a handle is drawn at.
pub fn auto_y_of(row: Rect, value: f64) -> f32 {
    let Some((top, bottom)) = auto_band(row) else {
        return 0.0;
    };
    let v = if value.is_finite() { value.clamp(0.0, 1.0) as f32 } else { 0.0 };
    bottom - (bottom - top) * v
}

/// The value a `y` inside an automation row means. [`auto_y_of`]'s inverse.
pub fn auto_value_at(row: Rect, y: f32) -> f64 {
    let Some((top, bottom)) = auto_band(row) else {
        return 0.0;
    };
    if !y.is_finite() || bottom <= top {
        return 0.0;
    }
    f64::from(((bottom - y) / (bottom - top)).clamp(0.0, 1.0))
}

/// The band a curve may actually occupy — the row, inset so a handle drawn
/// at either end is whole.
///
/// **`None` for a row that is not a rectangle**, and that guard is not
/// theoretical: `Rect::NOTHING` in egui is `[+inf, +inf]..[-inf, -inf]`, so
/// its height is `-inf`, the inset is `-inf`, and `inf + -inf` is NaN. A NaN
/// reaching the painter is a shape that silently does not draw and a hit
/// test where every comparison is false — the kind of wrong that looks like
/// nothing happening.
///
/// Callers already skip non-positive rows, but these two are `pub` and the
/// drag path reaches them from `app.rs`, so the guard lives where the
/// arithmetic is rather than at each place somebody remembers.
fn auto_band(row: Rect) -> Option<(f32, f32)> {
    if !row.is_positive() || !row.height().is_finite() || !row.top().is_finite() {
        return None;
    }
    // Half the row at most: a row too short for two insets would otherwise
    // invert its own band, and every value would read as the same one.
    let inset = AUTO_POINT_R.min(row.height() * 0.25).max(0.0);
    Some((row.top() + inset, row.bottom() - inset))
}

/// How big a point's handle is drawn, and the reach of a press for one.
pub const AUTO_POINT_R: f32 = 4.0;

// ── the tempo lane ──────────────────────────────────────────────────────────

/// The BPM range the tempo lane draws across.
///
/// **Fixed rather than fitted to the song.** A lane whose scale moved with
/// its contents would redraw every point every time one of them changed, and
/// a 4 bpm nudge would look like a cliff. These are the ends of the band's
/// own tempo dial, so a point at the top of the lane is a tempo the rest of
/// the app agrees exists.
pub const TEMPO_LANE_BPM: (f64, f64) = (20.0, 300.0);

/// Where a bpm sits in the tempo row. The lane's `auto_y_of`.
pub fn tempo_y_of(row: Rect, bpm: f64) -> f32 {
    let Some((top, bottom)) = auto_band(row) else {
        return 0.0;
    };
    let (lo, hi) = TEMPO_LANE_BPM;
    // **Logarithmic**, because tempo is: the step from 60 to 70 is the same
    // musical distance as 120 to 140, and a linear lane gives the top half
    // of the travel to tempos nobody writes.
    let t = if bpm.is_finite() && bpm > 0.0 {
        ((bpm.clamp(lo, hi) / lo).ln() / (hi / lo).ln()) as f32
    } else {
        0.0
    };
    bottom - (bottom - top) * t.clamp(0.0, 1.0)
}

/// The bpm a `y` in the tempo row means. [`tempo_y_of`]'s inverse.
pub fn tempo_bpm_at(row: Rect, y: f32) -> f64 {
    let Some((top, bottom)) = auto_band(row) else {
        return TEMPO_LANE_BPM.0;
    };
    if !y.is_finite() || bottom <= top {
        return TEMPO_LANE_BPM.0;
    }
    let (lo, hi) = TEMPO_LANE_BPM;
    let t = f64::from(((bottom - y) / (bottom - top)).clamp(0.0, 1.0));
    lo * (hi / lo).powf(t)
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
/// **The band at the top of a clip that the NAME occupies.**
///
/// The name is drawn at `r.top() + 2.0` in a font sized off the row, and the
/// note miniature filled the whole clip — so on a MIDI item the notes ran
/// straight through the letters and neither could be read. One function, used
/// by the name, the miniature and the empty item's dashed staff, so the three
/// cannot disagree about where the title ends.
///
/// Zero when there is no name drawn (a clip too narrow for one) or when
/// reserving the band would leave nothing to draw notes in — a squeezed clip
/// is better as a readable smear of notes than as a title with no picture.
fn clip_name_band(r: Rect) -> f32 {
    if r.width() < 30.0 {
        return 0.0;
    }
    let font = (r.height() * 0.22).clamp(8.0, 11.0);
    let band = 2.0 + font + 1.5;
    if r.height() - band < 6.0 {
        0.0
    } else {
        band
    }
}

/// **Which loop passes can be seen**, as a half-open range of pass numbers.
///
/// A looped clip is drawn by REPEATING its notes: a one-bar part dragged
/// over two hundred bars holds two hundred passes. The miniature used to
/// walk every note of every pass and let the per-note clamp throw away the
/// ones off screen, so the cost grew with the SONG rather than with the
/// window — zoomed to four bars of a two-hundred-bar clip it did a hundred
/// thousand note placements to draw two thousand marks.
///
/// The passes are evenly spaced in time, so the two that bound the window
/// are arithmetic and not a search: a pass puts source time `t` at
/// `fx0 + fw * (t - offset + loop_s * k) / len`, and that inverts. One pass
/// either side of the answer is kept — this decides only what to WALK, the
/// per-note clamp still decides what shows, and being generous by one costs
/// a pass while being mean by one drops notes at the edge.
///
/// The whole range comes back for anything that is not a plain forward
/// loop: no loop, a loop of no length, a clip of no length, notes whose
/// span is not a number. A miniature that is merely slow is a nuisance; one
/// that silently drops half its notes is a bug report about a broken file.
fn visible_passes(
    passes: usize,
    loop_s: f64,
    offset_s: f64,
    len_s: f64,
    notes_s: (f64, f64),
    full_x: (f32, f32),
    window: (f32, f32),
) -> std::ops::Range<usize> {
    let (first_s, last_s) = notes_s;
    let (fx0, fx1) = full_x;
    let fw = f64::from(fx1 - fx0).max(1.0);
    if passes <= 1
        || loop_s <= 1.0e-6
        || !loop_s.is_finite()
        || len_s <= 0.0
        || !first_s.is_finite()
        || !last_s.is_finite()
        || !window.0.is_finite()
        || !window.1.is_finite()
    {
        return 0..passes;
    }
    // The source second drawn at `x`, in the first pass's own terms.
    let at = |x: f32| (f64::from(x) - f64::from(fx0)) / fw * len_s + offset_s;
    // The last note of pass k reaches past the left edge; the first note of
    // pass k starts before the right one.
    let from = (at(window.0) - last_s) / loop_s;
    let to = (at(window.1) - first_s) / loop_s;
    if !from.is_finite() || !to.is_finite() || to < from {
        return 0..passes;
    }
    // `as` saturates on a float that will not fit, which is the behaviour
    // wanted here: a window a hundred loops wide of a clip that holds three
    // clamps to three rather than wrapping to something enormous.
    let start = (from.floor().max(0.0) as usize).min(passes);
    let end = ((to.ceil() + 1.0).max(0.0) as usize).clamp(start, passes);
    start..end
}

/// The clip's notes as a miniature: pitch spread across the clip's height
/// (padded to at least an octave so one note does not fill the box), each
/// note a small bar over its own span of the drawn length.
/// `full_x` is the clip's UNCLAMPED pixel span. The drawn rect is cut to
/// the viewport, and mapping time onto the CUT width compressed the
/// picture: a recording that crossed the right edge watched its own notes
/// slide leftward as the take grew — the owner's "clip begins shrinking".
/// Time maps onto the full span; each bar is then clipped to what shows.
///
/// **Two things keep a looped clip off the frame budget.** Measured on
/// eight lanes each carrying a one-bar part of music dragged over two
/// hundred bars, the plain walk emitted 108,440 marks and cost 8.0 ms a
/// frame — forty-eight per cent of a 60 Hz budget spent on dashes a fifth
/// of a pixel wide, before the grid, the headers or the waves are drawn. So:
///
/// * only the passes that reach the window are walked ([`visible_passes`]),
///   which is exact — the same marks, minus the ones that were computed
///   solely to be thrown away;
/// * and a mark too narrow to show where it begins is merged with the other
///   marks in its pixel column into one bar per run of TOUCHING pitches. A
///   miniature is an impression, not a score, and at that width the marks
///   already overlap each other: the merge draws the union it was going to
///   paint anyway.
///
/// **Each mark decides that for itself.** The first version of this asked
/// the clip's LONGEST note and merged nothing if that one was wide, which
/// looked right against a probe whose notes were all the same length and was
/// worthless against music: one held note — a pad, a bass note, a whole-bar
/// chord — sent every other note in the clip down the plain path and gave
/// the whole saving back. A held note is drawn as the span it is; the
/// sub-pixel notes around it still merge.
///
/// The merge is the only place the picture moves, and it moves by less than
/// a pixel. `mod tests` keeps the plain walk as the drawing this one has to
/// agree with, cell by cell, at every zoom, with and without a held note.
fn draw_note_miniature(
    painter: &Painter,
    r: Rect,
    full_x: (f32, f32),
    clip: &ClipView<'_>,
    // The ink of the CLIP this miniature sits inside, measured there by the
    // caller — not `Ink`, because handing the whole wardrobe in is how a
    // miniature ends up printed in an ink its own clip cannot carry.
    ink: Color32,
) {
    let len = clip.len_s.max(1.0e-9);
    // How many passes the clip's length holds. One when it does not loop,
    // which is every clip written before looping existed.
    let passes: usize = if clip.loop_s > 1.0e-6 {
        ((clip.len_s / clip.loop_s).ceil() as usize).clamp(1, 512)
    } else {
        1
    };
    let (mut lo, mut hi) = (127u8, 0u8);
    // The notes' own span, which places a pass — and which ONLY
    // [`visible_passes`] reads, and which it does not read at all on a clip
    // holding one pass. So a clip that does not loop does not walk for it.
    // That is not a rounding error dressed up: the clips carrying the most
    // notes are the long recorded ones, and a recorded clip is exactly the
    // kind that has no loop to skip, so the span would be a walk over
    // thousands of notes bought and thrown away every frame.
    let (mut first_s, mut last_s) = (f64::INFINITY, f64::NEG_INFINITY);
    if passes > 1 {
        for n in clip.notes {
            lo = lo.min(n.key);
            hi = hi.max(n.key);
            first_s = first_s.min(n.at_s);
            last_s = last_s.max(n.at_s + n.dur_s.max(0.02));
        }
    } else {
        for n in clip.notes {
            lo = lo.min(n.key);
            hi = hi.max(n.key);
        }
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
    // **Under the title, not through it.** See `clip_name_band`.
    let inner = {
        let base = r.shrink(2.0);
        let band = clip_name_band(r);
        if band > 0.0 && base.height() > band {
            Rect::from_min_max(Pos2::new(base.left(), base.top() + band), base.max)
        } else {
            base
        }
    };
    if !inner.is_positive() {
        return;
    }
    let bar_h = (inner.height() / span).clamp(1.5, 4.0);
    let (fx0, fx1) = full_x;
    let fw = (fx1 - fx0).max(1.0);
    let seen = visible_passes(
        passes,
        clip.loop_s,
        clip.offset_s,
        len,
        (first_s, last_s),
        full_x,
        (inner.left(), inner.right()),
    );
    // Where one note of one pass lands, or nothing if it falls outside the
    // clip or outside what shows of it.
    let mark = |pass: usize, n: &crate::arrangement::ArrNote| -> Option<(f32, f32, u8)> {
        let shift = clip.loop_s * pass as f64;
        let a = (n.at_s - clip.offset_s + shift) / len;
        let b = (n.at_s + n.dur_s.max(0.02) - clip.offset_s + shift) / len;
        if b <= 0.0 || a >= 1.0 {
            return None;
        }
        let x0 = (fx0 + fw * (a.max(0.0) as f32)).max(inner.left());
        let x1 = (fx0 + fw * (b.min(1.0) as f32)).min(inner.right());
        if x1 <= x0 {
            return None;
        }
        Some((x0, x1, n.key.clamp(lo, hi) - lo))
    };
    let y_of = |k: usize| {
        inner.top() + (inner.height() - bar_h) * (1.0 - k as f32 / span)
    };
    let bar = |x0: f32, top: f32, bottom: f32| {
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x0 + 1.5, bottom)),
            1.0,
            ink.gamma_multiply(0.8),
        );
    };
    // One mark drawn where it actually falls, over the span it actually
    // covers. This is the plain walk's own drawing, kept as a closure so
    // that the merged path can reach for it note by note.
    let span_bar = |x0: f32, x1: f32, k: u8| {
        let y = y_of(usize::from(k));
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.5), y + bar_h)),
            1.0,
            ink.gamma_multiply(0.8),
        );
    };
    // One column of the merge grid per point of width, and a ceiling on it.
    // The rect handed in has already been cut to the viewport, so this is a
    // few thousand columns on any real screen; the ceiling is there so that
    // a window nobody has drops back to the plain walk rather than folding
    // an entire clip into one column and calling it a picture.
    let cols = inner.width().ceil().max(1.0);
    // **The grid is built only when some mark could land inside another's
    // column**, and the question is asked as cheaply as it can be asked.
    //
    // It used to be asked of the clip's LONGEST note, and that was the whole
    // fault: one held note anywhere in the bar — a pad, a bass note, a
    // whole-bar chord, which is to say ordinary music — made the longest
    // wide and vetoed the merge for every other note in the clip. The width
    // that matters is each MARK's own, so each mark decides for itself
    // below; all this decides is whether to allocate a grid at all.
    //
    // `scale` is points per source second, and every note draws at least
    // 0.02 s long (`dur_s.max(0.02)`) — so when even that would be a point
    // wide, nothing in this clip can merge, and NO NOTE IS LOOKED AT to
    // learn it. That matters more than the arithmetic makes it sound: this
    // question is asked of every clip every frame, and asking it of the
    // notes is a per-note cost paid by exactly the clips that have the most
    // of them and the least to gain — a long recorded part, zoomed in, where
    // every mark is already wider than a point. The decision is O(columns)
    // and the columns are bounded by the screen.
    //
    // The price of guessing wrong the other way is an empty grid: a few
    // kilobytes and a sweep over columns that hold nothing, which does not
    // grow with the part.
    let scale = f64::from(fw) / len;
    let mut lit: Vec<u64> = if cols > 16_384.0 || 0.02 * scale >= 1.0 {
        Vec::new()
    } else {
        vec![0u64; cols as usize * 2]
    };
    let cols = cols as usize;
    for (pass, n) in seen.flat_map(|k| clip.notes.iter().map(move |n| (k, n))) {
        if let Some((x0, x1, k)) = mark(pass, n) {
            // **Wide enough to show where it begins**: draw it as the span
            // it is. A held note crosses many columns and reads as a line;
            // folding it into the column it starts in would delete it.
            if lit.is_empty() || x1 - x0 >= 1.0 {
                span_bar(x0, x1, k);
            } else {
                let c = (((x0 - inner.left()).max(0.0)) as usize).min(cols - 1);
                lit[c * 2 + usize::from(k) / 64] |= 1u64 << (usize::from(k) % 64);
            }
        }
    }
    if lit.is_empty() {
        return;
    }
    for c in 0..cols {
        let words = [lit[c * 2], lit[c * 2 + 1]];
        if words == [0, 0] {
            continue;
        }
        let x0 = inner.left() + c as f32;
        // Pitches come out ascending, so y comes out descending: a run
        // grows UPWARD and is drawn when the next pitch stops touching it.
        let mut run: Option<(f32, f32)> = None;
        for (w, mut bits) in words.into_iter().enumerate() {
            while bits != 0 {
                let k = bits.trailing_zeros() as usize + w * 64;
                bits &= bits - 1;
                let y = y_of(k);
                run = Some(match run {
                    Some((top, bottom)) if y + bar_h >= top => (y, bottom),
                    Some((top, bottom)) => {
                        bar(x0, top, bottom);
                        (y, y + bar_h)
                    }
                    None => (y, y + bar_h),
                });
            }
        }
        if let Some((top, bottom)) = run {
            bar(x0, top, bottom);
        }
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
        (l.tool_tempo, Hit::ToolTempoLane),
        (l.tool_nav, Hit::ToolNav),
    ] {
        if r.is_positive() && r.contains(pos) {
            return Some(hit);
        }
    }
    if l.grip.is_positive() && l.grip.contains(pos) {
        return Some(Hit::HeaderGrip);
    }
    // **The bus rows first**, before the tracks: they sit above them and a
    // press on one must not fall through to the lane beneath.
    if l.master_row.automation.is_positive() && l.master_row.automation.contains(pos) {
        return Some(Hit::AutoMenu(MASTER_LANE));
    }
    if l.master_row.knob.is_positive() && l.master_row.knob.contains(pos) {
        return Some(Hit::BusGain(MASTER_COLUMN));
    }
    for (b, cell) in l.master_row.bays.iter().enumerate() {
        if cell.is_positive() && cell.contains(pos) {
            return Some(Hit::BusBay(MASTER_COLUMN, b));
        }
    }
    for (i, row) in l.header_rows.iter().enumerate() {
        for (r, hit) in [
            (row.arm, Hit::Arm(i)),
            (row.mute, Hit::Mute(i)),
            (row.phase, Hit::Phase(i)),
            (row.solo, Hit::Solo(i)),
            (row.theory, Hit::ToTheory(i)),
            (row.automation, Hit::AutoMenu(i)),
            (row.knob, Hit::Gain(i)),
            (row.send, Hit::Send(i)),
            (row.pan, Hit::Pan(i)),
            (row.input, Hit::HdrInput(i)),
            (row.add, Hit::HdrAdd(i)),
            (row.name, Hit::HdrName(i)),
            // **Before the NUMBER, and the order is load-bearing.** The box
            // sits at the header's bottom-left, inside the number cell's own
            // column; the number is the reorder handle, so a box tested after
            // it would never be reached and clicking it would start a drag.
            (row.collapse, Hit::HdrCollapse(i)),
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
    // **The signature chips, before the ruler itself.** They sit ON the
    // ruler and a press on one must not also locate — the same pre-pass rule
    // the band's timeline × follows.
    if l.meter_strip.is_positive() && l.meter_strip.contains(pos) {
        let font_h = (l.meter_strip.height() * 0.78).clamp(6.0, 10.0);
        for pt in view.meter.points() {
            if pt.at_bar == 0 && view.meter.points().len() == 1 {
                continue;
            }
            let x = x_of(rect, view, bar_at(view, pt.at_bar));
            // The same width the chip is DRAWN at, including the field's
            // caret: a target that disagreed with the paint by a character
            // would be a chip you cannot press once you have opened it.
            let text = match view.meter_edit.filter(|(b, _)| *b == pt.at_bar) {
                Some((_, t)) => format!("{t}|"),
                None => format!("{}/{}", pt.sig.beats, pt.sig.unit),
            };
            let w = (font_h * 0.62 * text.chars().count() as f32 + 6.0).max(font_h * 2.2);
            if pos.x >= x && pos.x <= x + w {
                return Some(Hit::MeterChip(pt.at_bar));
            }
        }
        // Empty strip: the bar under the pointer, which is where a new
        // signature would start. Snapped to a BAR by construction — a
        // signature change part-way through a bar is not a thing.
        return Some(Hit::MeterStrip(bar_of(view, time_at(rect, view, pos.x))));
    }
    // **The tempo lane**, on the same terms as an automation row: the
    // handles overhang, so it is checked before the rows below it.
    if let Some(row) = l.rows.iter().find(|r| r.kind == RowKind::Tempo) {
        if row.header.is_positive() && row.header.contains(pos) {
            return Some(Hit::TempoHeader);
        }
        if row.lane.is_positive() && pos.x.is_finite() && pos.y.is_finite() {
            let grab = row.lane.expand(AUTO_POINT_R);
            if grab.is_positive() && grab.contains(pos) {
                let pts = view.tempo.points();
                let near = pts
                    .iter()
                    .enumerate()
                    .map(|(i, q)| {
                        let at = Pos2::new(
                            tempo_x_of(rect, view, row.lane, q.at_beat),
                            tempo_y_of(row.lane, q.bpm),
                        );
                        (i, at.distance(pos))
                    })
                    .filter(|(_, d)| d.is_finite() && *d <= AUTO_POINT_R * 2.0)
                    .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
                if let Some((i, _)) = near {
                    return Some(Hit::TempoPoint(i));
                }
                // Empty room that is really in the row — the automation
                // lane's own rule, and for the same reason: a press a few
                // pixels above lands visually in the ruler.
                if row.lane.contains(pos) {
                    return Some(Hit::TempoLane(
                        time_at(rect, view, pos.x),
                        tempo_bpm_at(row.lane, pos.y),
                    ));
                }
            }
        }
    }
    // **The automation rows, before the clips.** They are their own rows, so
    // there is no overlap to resolve — but they are checked here because a
    // point's handle sticks out past the row's own edges by its radius, and
    // a press on the top of a handle belongs to the point rather than to
    // whatever row that pixel is technically in.
    for row in &l.rows {
        let RowKind::Automation(lane, slot) = row.kind else {
            continue;
        };
        if row.header.is_positive() && row.header.contains(pos) {
            // The cells first: the header as a whole still opens the
            // parameter menu, so a switch has to be asked about before the
            // room it sits in.
            let h = auto_cells(view, lane, row.header);
            for (r, hit) in [
                (h.close, Hit::AutoClose(lane, slot)),
                (h.arm, Hit::AutoArm(lane, slot)),
                (h.bypass, Hit::AutoBypass(lane, slot)),
                (h.dial, Hit::AutoDial(lane, slot)),
            ] {
                if r.is_positive() && r.contains(pos) {
                    return Some(hit);
                }
            }
            return Some(Hit::AutoParamPick(lane, slot));
        }
        if !row.lane.is_positive() {
            continue;
        }
        let Some(auto) = auto_view_of(view, lane, slot) else {
            continue;
        };
        // The row plus a handle's radius: a press on the top of a point
        // whose handle overhangs the row's edge belongs to that point.
        // Guarded, because expanding a degenerate rect gives one that
        // contains everything or nothing.
        if !pos.x.is_finite() || !pos.y.is_finite() {
            continue;
        }
        let grab = row.lane.expand(AUTO_POINT_R);
        if !grab.is_positive() || !grab.contains(pos) {
            continue;
        }
        // The nearest point within reach wins, measured in PIXELS in both
        // axes: a curve is looked at, so "near" means near on screen and not
        // near in seconds — at a wide zoom a second is a hair's breadth and
        // at a tight one it is half the window.
        //
        // Only the points that can be SEEN are candidates, by the same
        // binary search the drawing uses: a point scrolled off the left edge
        // is drawn nowhere, so grabbing one would be grabbing something
        // invisible. It also keeps the cost of a press independent of how
        // long the song is.
        let from_s = view.view_start_s;
        let to_s = view.view_start_s + view.view_len_s;
        let lo = auto.points.partition_point(|q| q.at_s < from_s).saturating_sub(1);
        let hi = (auto.points.partition_point(|q| q.at_s <= to_s) + 1)
            .min(auto.points.len());
        let near = auto.points[lo.min(hi)..hi]
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let at = Pos2::new(x_of(rect, view, p.at_s), auto_y_of(row.lane, p.value));
                (lo + i, at.distance(pos))
            })
            // **Finite first.** A non-finite distance compares false against
            // everything, so it would neither be filtered out by the reach
            // nor lose the `min_by` — it would simply be whichever the
            // iterator happened to reach first.
            .filter(|(_, d)| d.is_finite() && *d <= AUTO_POINT_R * 2.0)
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        if let Some((i, _)) = near {
            return Some(Hit::AutoPoint(lane, slot, i));
        }
        // **Empty room, but only room that is really in this row.**
        //
        // The grab area above is the row EXPANDED by a handle's radius, which
        // is right for catching a point whose handle overhangs the edge and
        // wrong for placing a new one. Rows stack against each other, so a
        // press a few pixels above an open automation row is visually in the
        // clip strip of the track ABOVE — and this made a point out of it, at
        // full value, because `auto_value_at` clamps and the caller never
        // learned the press was out of band. `arr_put_auto_point` then wrote
        // it to disk immediately.
        //
        // Falling through rather than returning, so the press goes on to the
        // row it actually landed in.
        if row.lane.contains(pos) {
            return Some(Hit::AutoLane(
                lane,
                slot,
                time_at(rect, view, pos.x),
                auto_value_at(row.lane, pos.y),
            ));
        }
        continue;
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
    // The master's field, last: its header's own controls are pushed above
    // and have already answered, so what is left is the ground beside them.
    if let Some(row) = l.rows.iter().find(|r| r.kind == RowKind::Master) {
        if row.lane.is_positive() && row.lane.contains(pos) {
            return Some(Hit::MasterLane(time_at(rect, view, pos.x)));
        }
    }
    None
}


/// **Every DISCRETE control on the timeline, in reading order.**
///
/// The arrangement's `hit_test` cannot be walked backwards the way the
/// desk's can: half of what it answers is COMPUTED from the point — a
/// second on a lane, a bar under the chip strip, a bpm on the tempo
/// curve — and those have no rectangle to hand a keyboard. What is left
/// does: the toolbar, the header cells, and the clips.
///
/// So this enumerates that half, and it is checked against `hit_test`
/// rather than trusted: `every_target_is_the_hit_it_claims` presses each
/// rect's centre and asserts the panel answers with the hit named here.
/// A cell that moves in one and not the other fails that test.
///
/// Reading order, because **allocation order is tab order**: the toolbar,
/// then each row from the top — a track's header cells left to right,
/// then the clips standing on it.
pub fn targets(rect: Rect, view: &ArrangeView<'_>) -> Vec<(Rect, Hit)> {
    let l = Layout::new(rect, view);
    let mut out: Vec<(Rect, Hit)> = Vec::new();
    let mut push = |r: Rect, hit: Hit| {
        if r.is_positive() {
            out.push((r, hit));
        }
    };
    for (r, hit) in [
        (l.tool_met, Hit::ToolMetronome),
        (l.tool_snap, Hit::ToolSnap),
        (l.tool_tempo, Hit::ToolTempoLane),
        (l.tool_grid, Hit::ToolGrid),
        (l.tool_nav, Hit::ToolNav),
        (l.grip, Hit::HeaderGrip),
    ] {
        push(r, hit);
    }
    push(l.master_row.automation, Hit::AutoMenu(MASTER_LANE));
    push(l.master_row.knob, Hit::BusGain(MASTER_COLUMN));
    for (b, cell) in l.master_row.bays.iter().enumerate() {
        push(*cell, Hit::BusBay(MASTER_COLUMN, b));
    }
    for row in &l.rows {
        match row.kind {
            RowKind::Tempo => push(row.header, Hit::TempoHeader),
            RowKind::Master => {}
            RowKind::Automation(lane, slot) => {
                let h = auto_cells(view, lane, row.header);
                push(h.close, Hit::AutoClose(lane, slot));
                push(h.arm, Hit::AutoArm(lane, slot));
                push(h.bypass, Hit::AutoBypass(lane, slot));
                push(h.dial, Hit::AutoDial(lane, slot));
                push(row.header, Hit::AutoParamPick(lane, slot));
            }
            RowKind::Track(i) => {
                let Some(hdr) = l.header_rows.get(i) else { continue };
                // Left to right across the header: the handle, the two
                // dots, the name column, the dials, then the switches at
                // the right end.
                for (r, hit) in [
                    (hdr.number, Hit::HdrNumber(i)),
                    (hdr.collapse, Hit::HdrCollapse(i)),
                    (hdr.arm, Hit::Arm(i)),
                    (hdr.theory, Hit::ToTheory(i)),
                    (hdr.automation, Hit::AutoMenu(i)),
                    (hdr.add, Hit::HdrAdd(i)),
                    (hdr.name, Hit::HdrName(i)),
                    (hdr.input, Hit::HdrInput(i)),
                ] {
                    push(r, hit);
                }
                for (b, r) in hdr.bays.iter().enumerate() {
                    push(*r, Hit::HdrBay(i, b));
                }
                for (r, hit) in [
                    (hdr.pan, Hit::Pan(i)),
                    (hdr.knob, Hit::Gain(i)),
                    (hdr.send, Hit::Send(i)),
                    (hdr.routing, Hit::HdrRouting(i)),
                    (hdr.phase, Hit::Phase(i)),
                    (hdr.mute, Hit::Mute(i)),
                    (hdr.solo, Hit::Solo(i)),
                ] {
                    push(r, hit);
                }
                // The clips standing on this lane, left to right. A clip
                // being RECORDED is a picture and not a row — `hit_test`
                // refuses it too.
                let mut on_lane: Vec<&ClipView<'_>> = view
                    .clips
                    .iter()
                    .filter(|c| c.lane == i && !c.recording)
                    .collect();
                on_lane.sort_by(|a, b| {
                    a.start_s.partial_cmp(&b.start_s).unwrap_or(std::cmp::Ordering::Equal)
                });
                for clip in on_lane {
                    // **The BODY's own rectangle, not the clip's.** The
                    // trim edges and the fade corners are cut out of the
                    // ends, and a centre offered as "the clip" that landed
                    // on an edge would hand a keyboard a trim it never
                    // asked for. A clip too narrow to have a body is one
                    // there is nothing to press.
                    let r = clip_rect(rect, view, clip);
                    if !r.is_positive() {
                        continue;
                    }
                    let edge = EDGE_W.min(r.width() * 0.2);
                    let body = Rect::from_min_max(
                        Pos2::new(r.left() + edge + 0.5, r.top()),
                        Pos2::new(r.right() - edge - 0.5, r.bottom()),
                    );
                    push(body, Hit::ClipBody(clip.id));
                }
            }
        }
    }
    out
}

/// Everything on the timeline a keyboard can reach and a screen reader can
/// say.
///
/// Built from [`targets`], so this cannot disagree with `hit_test` about
/// where a control is.
///
/// **Named by their TRACK, always** — the desk's rule, and the arrangement
/// needs it more: sixteen rows of identical dials read as sixteen identical
/// labels, and the row is the only thing that tells them apart.
///
/// Two things here are READ rather than operated, and they are the half that
/// makes the view useful rather than merely operable: the clips, which are
/// the song, and the dial readouts. A timeline a blind student cannot read
/// the shape of has been made navigable and not useful.
pub fn a11y_nodes(rect: Rect, view: &ArrangeView<'_>) -> Vec<crate::a11y::Node> {
    use crate::a11y::{Node, Travel};
    let mut out = Vec::new();
    let up = |norm: f32| Travel { norm: norm.clamp(0.0, 1.0), up: true };
    // The name a person would use for a row, whether or not it is a track
    // yet. Never an empty string — a node labelled "" is a node a reader
    // announces as nothing at all.
    let who = |i: usize| -> String {
        // The bus first: `MASTER_LANE` is one PAST the lanes array, so the
        // lookup below finds nothing there and the fallback would name the
        // master "Track 17" — which is what a reader heard on every master
        // automation row. The same word the bus row's own labels use.
        if i == MASTER_LANE {
            return "Master".to_owned();
        }
        match view.lanes.get(i) {
            Some(l) if !l.name.is_empty() => l.name.to_owned(),
            _ => format!("Track {}", i + 1),
        }
    };
    // Where a clip sits, in the bars the ruler prints — never in seconds.
    // "36.4 seconds" is a readout that has to be decoded.
    let bars = |from_s: f64, len_s: f64| {
        let a = bar_of(view, from_s) + 1;
        let b = bar_of(view, from_s + len_s.max(0.0)) + 1;
        if a == b {
            format!("bar {a}")
        } else {
            format!("bar {a} to {b}")
        }
    };
    for (r, hit) in targets(rect, view) {
        let id = egui::Id::new(("arrangement", format!("{hit:?}")));
        let node = match hit {
            // ── the toolbar ───────────────────────────────────────────
            Hit::ToolMetronome => Node::toggle(id, r, "Metronome", view.metronome_on),
            Hit::ToolSnap => Node::toggle(id, r, "Snap to grid", view.snap_on),
            Hit::ToolTempoLane => Node::toggle(id, r, "Tempo lane", view.show_tempo_lane),
            // **A slider and not a button**, so both arrows reach it: a
            // control that only ever steps one way walks a keyboard down to
            // 1/32 and strands it there. The announced value is the grid the
            // box prints; the travel is the exponent laid onto 0..1.
            Hit::ToolGrid => Node::slider(
                id,
                r,
                "Grid",
                f64::from(view.grid_exp),
                -5.0,
                3.0,
                {
                    let sg = view.meter.sig_at_bar(bar_of(view, view.view_start_s));
                    grid_spoken(view.grid_exp, sg.beats, sg.unit)
                },
                Travel {
                    norm: (f32::from(view.grid_exp) + 5.0) / 8.0,
                    up: false,
                },
            ),
            Hit::ToolNav => Node::readout(
                id,
                r,
                "Visible span",
                bars(view.view_start_s, view.view_len_s),
            ),
            // The seam is a drag and nothing else: there is no press of it
            // that means anything, so it is not a tab stop.
            Hit::HeaderGrip => continue,
            // ── the rows' own headers ─────────────────────────────────
            Hit::TempoHeader => Node::button(id, r, "Song tempo, flatten to one tempo"),
            Hit::Phase(i) => Node::toggle(
                id,
                r,
                &format!("Track {} polarity invert", i + 1),
                view.lanes.get(i).is_some_and(|l| l.phase),
            ),
            // `who(i)`, not "track {i+1}": the hardcoded number called the
            // master "track 17" and read a renamed track by its seat.
            Hit::AutoClose(i, slot) => {
                Node::button(id, r, &format!("Hide automation row {} on {}", slot + 1, who(i)))
            }
            Hit::AutoArm(i, slot) => Node::toggle(
                id,
                r,
                "Arm this automation lane to write",
                auto_view_of(view, i, slot).is_some_and(|a| a.armed),
            ),
            Hit::AutoBypass(i, slot) => Node::toggle(
                id,
                r,
                // The word a reader hears matches the letter on screen.
                "Mute this automation lane",
                auto_view_of(view, i, slot).is_some_and(|a| a.bypassed),
            ),
            Hit::AutoDial(i, slot) => Node::button(
                id,
                r,
                &format!("Automation lane {} on {}", slot + 1, who(i)),
            ),
            Hit::AutoParamPick(i, slot) => Node::button(
                id,
                r,
                format!(
                    "{} automation, {}",
                    who(i),
                    // Through `auto_view_of`, which knows where the master's
                    // rows live — the inline `lanes` lookup this replaces
                    // found nothing there and read a bound parameter as
                    // "none".
                    auto_view_of(view, i, slot)
                        .map_or_else(|| "none".to_owned(), |a| a.label.clone())
                ),
            ),
            Hit::AutoMenu(i) => {
                Node::button(id, r, format!("{} automation lanes", who(i)))
            }
            // The NUMBER is the reorder handle and carries no press worth
            // announcing; the name beside it says which row this is.
            Hit::HdrNumber(_) => continue,
            Hit::Arm(i) => Node::toggle(
                id,
                r,
                format!("{} record arm", who(i)),
                view.lanes.get(i).is_some_and(|l| l.armed),
            ),
            Hit::Mute(i) => Node::toggle(
                id,
                r,
                format!("{} mute", who(i)),
                view.lanes.get(i).is_some_and(|l| l.muted),
            ),
            Hit::Solo(i) => Node::toggle(
                id,
                r,
                format!("{} solo", who(i)),
                view.lanes.get(i).is_some_and(|l| l.soloed),
            ),
            Hit::ToTheory(i) => Node::toggle(
                id,
                r,
                format!("{} sends its MIDI to the theory view", who(i)),
                view.lanes.get(i).is_some_and(|l| l.to_theory),
            ),
            Hit::HdrCollapse(i) => Node::toggle(
                id,
                r,
                format!("{} shows its tracks", who(i)),
                !view.lanes.get(i).is_some_and(|l| l.folder_shut),
            ),
            Hit::HdrAdd(i) => Node::button(id, r, format!("Add a track at row {}", i + 1)),
            Hit::HdrName(i) => Node::readout(id, r, format!("{} name", who(i)), who(i)),
            Hit::HdrInput(i) => Node::readout(
                id,
                r,
                format!("{} input", who(i)),
                match view.lanes.get(i) {
                    Some(l) if !l.input.is_empty() => l.input.clone(),
                    _ => "none".to_owned(),
                },
            ),
            Hit::HdrBay(i, bay) => {
                let what = view.lanes.get(i).map_or("", |l| l.inserts[bay]);
                Node::readout(
                    id,
                    r,
                    format!("{} insert {}", who(i), bay + 1),
                    if what.is_empty() { "empty".to_owned() } else { what.to_owned() },
                )
            }
            Hit::HdrRouting(i) => Node::button(id, r, format!("{} routing", who(i))),
            // The two rows above the tracks. Their level is the desk's own,
            // so it reads in the desk's own words.
            Hit::BusBay(_, slot) => {
                let word = view.master_inserts.get(slot).copied().unwrap_or("");
                Node::button(
                    id,
                    r,
                    if word.is_empty() {
                        format!("Master insert {}, empty", slot + 1)
                    } else {
                        format!("Master insert {}, {word}", slot + 1)
                    },
                )
            }
            Hit::BusGain(_) => {
                let g = view.master_gain;
                Node::slider(
                    id,
                    r,
                    "Master volume".to_owned(),
                    f64::from(g),
                    0.0,
                    1.0,
                    crate::recorder::gain_text(g),
                    up(crate::recorder::gain_to_fader(g)),
                )
            }
            // ── the three dials ───────────────────────────────────────
            //
            // The same values, words and travel the desk's own controls
            // carry: they ARE the desk's controls, reached from here.
            Hit::Gain(i) => {
                let g = view.lanes.get(i).map_or(0.0, |l| l.gain);
                Node::slider(
                    id,
                    r,
                    format!("{} volume", who(i)),
                    f64::from(g),
                    0.0,
                    1.0,
                    crate::recorder::gain_text(g),
                    // The dial's POSITION, not its amplitude — the two are
                    // equal only at the ends.
                    up(crate::recorder::gain_to_fader(g)),
                )
            }
            Hit::Send(i) => {
                let s = view.lanes.get(i).map_or(0.0, |l| l.send);
                Node::slider(
                    id,
                    r,
                    format!("{} send", who(i)),
                    f64::from(s),
                    0.0,
                    1.0,
                    format!("{:.0}%", s * 100.0),
                    up(s),
                )
            }
            Hit::Pan(i) => {
                let p = view.lanes.get(i).map_or(0.0, |l| l.pan);
                Node::slider(
                    id,
                    r,
                    format!("{} pan", who(i)),
                    f64::from(p),
                    -1.0,
                    1.0,
                    crate::mixer_panel::pan_text(p),
                    up((p + 1.0) * 0.5),
                )
            }
            // ── the song itself ───────────────────────────────────────
            //
            // A clip is READ — where it is, how long, and whether it is in
            // the selection. Enter selects it, which is what makes the
            // clip commands (delete, normalise, the clip menu) reachable
            // without a mouse.
            Hit::ClipBody(clip_id) => {
                let Some(c) = view.clips.iter().find(|c| c.id == clip_id) else {
                    continue;
                };
                let name = if c.name.is_empty() { "Untitled" } else { c.name };
                let mut says = format!("{}, {}", bars(c.start_s, c.len_s), who(c.lane));
                if c.midi {
                    says.push_str(", MIDI");
                }
                if c.selected {
                    says.push_str(", selected");
                }
                Node::readout(id, r, format!("Clip {name}"), says)
            }
            // Everything `targets` does not enumerate. Named rather than
            // caught by `_`, so a new hit has to be decided about here.
            Hit::MasterLane(_)
            | Hit::ClipLeft(_)
            | Hit::ClipRight(_)
            | Hit::FadeIn(_)
            | Hit::FadeOut(_)
            | Hit::Lane(..)
            | Hit::Ruler(_)
            | Hit::AutoPoint(..)
            | Hit::AutoLane(..)
            | Hit::AutoMenu(_)
            | Hit::TempoPoint(_)
            | Hit::TempoLane(..)
            | Hit::MeterChip(_)
            | Hit::MeterStrip(_) => continue,
        };
        out.push(node);
    }
    out
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

/// One rack chip: a plugin's short name, or a plus for an empty bay.
///
/// **The same surface the mixer's bays are**, and the same ink. These were a
/// hard-coded bone panel with hard-coded near-black on it — the shipped desk's
/// colours, frozen — so on every theme but the default the input, FX and
/// routing chips stayed white modules on a desk that had moved. `hdr_face` IS
/// the mixer's strip face, and `engrave` is what that face can carry.
///
/// A free function rather than a closure inside the header loop, because the
/// MASTER row grew a rack of its own and two racks drawn by two pieces of code
/// is two racks that drift apart.
fn bay_chip(painter: &Painter, r: Rect, word: &str, has: bool, p: &Ink) {
    if !r.is_positive() {
        return;
    }
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
        // So the ink on a clip answers to the clip, not to the field behind
        // it — and the clip loop MEASURES its own (`on_ink` on the guarded
        // clip colour), because a hand-painted lane can reach the grid's
        // three dead-zone tones the palettes' contract never covered. This
        // constant is what the automation dials' needles still wear.
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
/// **The grid's step in QUARTER NOTES.**
///
/// A note value, not a fraction of the bar — which is what the box has always
/// said and never meant. `exp = -2` is a quarter note, `-3` an eighth, `0` a
/// whole note; the bar it lands in is irrelevant.
///
/// The difference only shows in a meter whose bar is not a power of two of
/// its own beat. In 4/4 a quarter of a bar IS a quarter note, so dividing the
/// bar by four was right by accident for years. In 6/8 a bar is three
/// quarters, so a quarter of it is a DOTTED EIGHTH — a line on nothing anyone
/// plays, and a magnet that pulls notes to it. Six beats cannot be divided by
/// four, and both this panel and the MIDI editor were trying.
pub fn grid_quarters(exp: i8, beats: u8, unit: u8) -> f64 {
    let beat = 4.0 / f64::from(unit.max(1));
    let per_bar = f64::from(beats.max(1));
    let bar = beat * per_bar;
    // **-7, not -6.** The ladder gained a rung at the TOP (the half note),
    // so every division below it moved one place further down the exponent
    // range; stopping at -6 would have quietly taken the finest grid the app
    // used to offer away from it.
    let e = i32::from(exp.clamp(-7, 6));
    if e >= 0 {
        // Bars, and multiples of them.
        return bar * 2.0f64.powi(e);
    }
    // **Halve the BAR while a half is still a whole number of beats; below
    // that, halve from the beat.**
    //
    // Anchoring straight to the beat — what this did before — skipped a step
    // in the commonest meter there is. In 4/4 the ladder went bar, quarter,
    // eighth: no HALF note anywhere on it, and a jump of four between the
    // first two rungs. The labels were written for the ladder somebody
    // expected instead of the one the code walked, so from the second rung
    // down every one of them was two steps too coarse — the box said "1/2"
    // and the magnet snapped eighths. The owner found it from the other end:
    // "there should exist a single state of that button that produces 1/4th
    // and that's the glyph, placed exactly between 1/8 and 1/2."
    //
    // Halving the bar all the way down would bring back the fault the beat
    // anchor was introduced to fix: in 6/8 a bar is three quarters, so a
    // quarter of it is a DOTTED EIGHTH — a line on nothing anybody plays.
    // Hence the condition. A half that lands between beats is refused and
    // the beat is taken instead, so the ladder never offers one.
    //
    //   4/4   1 bar,  1/2,  1/4,  1/8,  1/16, 1/32
    //   6/8   1 bar,  dotted 1/4,  1/8,  1/16, 1/32
    //
    // Every rung still divides the bar exactly, which is the property the
    // beat anchor was for and which this keeps.
    let mut v = bar;
    for _ in 0..(-e) {
        let half = v * 0.5;
        let beats_in_half = half / beat;
        if beats_in_half >= 1.0 && (beats_in_half - beats_in_half.round()).abs() > 1.0e-9 {
            // Between beats: drop to the beat rather than onto a line
            // nothing is played on.
            v = beat;
        } else {
            v = half;
        }
    }
    v
}

/// **What the box prints, derived from what the magnet actually does.**
///
/// It used to be a formula of its own, written for the ladder somebody
/// expected rather than the one [`grid_quarters`] walks — so from the second
/// rung down every label was two steps too coarse: the box said "1/2" while
/// the magnet snapped eighths, and "1/4" while it snapped sixteenths. Worse,
/// "1/4" appeared TWICE on the way down, once as the note glyph and once in
/// numerals, at two different divisions. The owner: "there should exist a
/// single state of that button that produces 1/4th and that's the glyph."
///
/// Reading the value back out is what makes that impossible to reintroduce:
/// there is now one source of truth and the label is a rendering of it.
///
/// The glyph is kept for the quarter note alone, which is what makes it the
/// single 1/4 state. It stays `\u{266a}` — the character already on screen
/// and known to render in the panel's font; a "more correct" `\u{2669}` that
/// the font lacks would draw a hollow box, which is worse than approximate.
pub fn grid_label(exp: i8, beats: u8, unit: u8) -> String {
    if exp > 0 {
        // Multiples of a BAR, and it says so by counting them.
        return format!("{}", 1u32 << exp.min(6) as u32);
    }
    note_label(grid_quarters(exp, beats, unit))
}

/// A length in quarter notes, as a note value.
///
/// Whole = 4 quarters, half = 2, quarter = 1, and so on down. A dotted value
/// is one and a half of the next one up and wears a dot; 6/8's second rung is
/// a dotted quarter and there is no other honest way to write it.
fn note_label(q: f64) -> String {
    if !q.is_finite() || q <= 0.0 {
        return "1".to_owned();
    }
    // The glyph IS the quarter note, and only the quarter note.
    if (q - 1.0).abs() < 1.0e-9 {
        return "\u{266a}".to_owned();
    }
    let denom = 4.0 / q;
    let rounded = denom.round();
    if (denom - rounded).abs() < 1.0e-9 && rounded >= 1.0 {
        return if rounded <= 1.0 {
            "1".to_owned()
        } else {
            format!("1/{}", rounded as u64)
        };
    }
    // Dotted: one and a half of a clean value.
    let plain = q * 2.0 / 3.0;
    let d = 4.0 / plain;
    let dr = d.round();
    if (d - dr).abs() < 1.0e-9 && dr >= 1.0 {
        return if dr <= 1.0 {
            "1\u{00b7}".to_owned()
        } else {
            format!("1/{}\u{00b7}", dr as u64)
        };
    }
    // Nothing nameable — a meter this build has no word for. Say the length
    // rather than a wrong note value.
    format!("{q:.3}q")
}

/// The same grid, in the words a person would say it in.
///
/// The box prints "1" and "1/4" because it is a box the width of a thumb.
/// A screen reader announcing "Grid, 1" has said nothing: the unit is the
/// whole content, and a readout that has to be decoded has not been made
/// accessible. So the spoken form carries it.
fn grid_spoken(exp: i8, beats: u8, unit: u8) -> String {
    // **Read off the same division the box prints and the magnet uses.**
    //
    // This carried its own third opinion, in BEATS, while the box counted in
    // note values and the maths did neither — so a reader was told "1/2 beat"
    // where the box said "1/2" and the grid snapped an eighth. Three
    // statements, no two of them agreeing. One source now.
    if exp > 0 {
        return format!("{} bars", 1u32 << exp.min(6) as u32);
    }
    if exp == 0 {
        return "1 bar".to_owned();
    }
    let q = grid_quarters(exp, beats, unit);
    // Spoken in words rather than in the box's shorthand: a reader saying
    // "one slash eight" has not said "an eighth note".
    let label = note_label(q);
    match label.as_str() {
        "\u{266a}" => "quarter note".to_owned(),
        "1/2" => "half note".to_owned(),
        "1/8" => "eighth note".to_owned(),
        "1/16" => "sixteenth note".to_owned(),
        "1/32" => "thirty-second note".to_owned(),
        "1/64" => "sixty-fourth note".to_owned(),
        "1/4\u{00b7}" => "dotted quarter note".to_owned(),
        "1/8\u{00b7}" => "dotted eighth note".to_owned(),
        other => format!("{other} of a bar"),
    }
}

/// **Where a tempo handle is drawn, and grabbed.**
///
/// The x it would have, held far enough inside the lane to be a whole dot.
///
/// The first point of every map is at beat zero, so with the view at the
/// start it lands exactly on the lane's left edge: half the handle is drawn
/// outside the lane, and everything left of that edge belongs to the HEADER,
/// which the hit test asks about first. That left only the right half of the
/// dot clickable — "barely or not at all", which is exactly right.
///
/// One function because both the drawing and the hit test need the same
/// answer; two copies of this is how a handle comes to be drawn somewhere it
/// cannot be pressed.
fn tempo_x_of(rect: Rect, view: &ArrangeView<'_>, lane: Rect, at_beat: f64) -> f32 {
    let x = x_of(rect, view, view.tempo.seconds_at_beat(at_beat));
    if !x.is_finite() {
        return x;
    }
    x.max(lane.left() + AUTO_POINT_R)
}

/// Where a bar LINE falls, in seconds. Bars count from zero here and from
/// one on the ruler, the way musicians write them.
fn bar_at(view: &ArrangeView<'_>, bar: u32) -> f64 {
    view.tempo.seconds_at_beat(view.meter.quarter_at_bar(bar))
}

/// Which bar a moment falls in.
fn bar_of(view: &ArrangeView<'_>, seconds: f64) -> u32 {
    view.meter
        .bar_beat_at_quarter(view.tempo.beat_at_seconds(seconds.max(0.0)))
        .0
}

/// How many bars to skip between drawn lines, so the numbers stay a hand
/// apart.
///
/// **Measured over the whole window rather than off one bar**, because with
/// a tempo change in view the bars are not the same width: a step chosen
/// from the first bar of a ritardando would put four lines in the last inch
/// and none in the first foot. The count of bars actually in the window is
/// the honest average, and the labels that would still collide are dropped
/// individually where they are drawn.
fn bar_step(view: &ArrangeView<'_>, width: f32) -> u32 {
    let first = bar_of(view, view.view_start_s);
    let last = bar_of(view, view.view_start_s + view.view_len_s);
    let bars = last.saturating_sub(first).max(1);
    let px = f64::from(width) / f64::from(bars);
    let mut step = 1u32;
    // 1 << 20 bars is an hour of 4/4 at 120: past that the ruler is a line.
    while px * f64::from(step) < 64.0 && step < (1 << 20) {
        step *= 2;
    }
    step
}

/// **Every open automation row's header**, drawn over the header column's
/// wood.
///
/// Split out of `draw`'s lane loop for one reason: ORDER. The loop runs long
/// before the header column is filled with `hdr_wood`, so anything it painted
/// there was covered. Everything a track's header draws happens after that
/// fill; these rows have to join it.
/// **A child's automation rows step right with the track they belong to.**
///
/// The indent is the only thing on screen that says "contained" — see
/// [`FOLDER_INDENT`] — and an automation row that did not take it left a
/// folder's children's rows sitting where the FOLDER's own would, so a
/// column of dials no longer lined up with the tracks they belong to.
///
/// The face still fills the whole row, exactly as a track's header does: what
/// moves is the contents. One function because THREE callers need the same
/// rect — the paint, the hit test and the tab order — and a hit test that
/// disagrees with the paint is a control you can see and cannot press.
fn auto_cells(view: &ArrangeView<'_>, lane: usize, header: Rect) -> AutoHeader {
    let indent = if view.lanes.get(lane).is_some_and(|l| l.in_folder) {
        FOLDER_INDENT
    } else {
        0.0
    };
    if indent > 0.0 && header.width() > indent + 40.0 {
        auto_header(Rect::from_min_max(
            Pos2::new(header.left() + indent, header.top()),
            header.max,
        ))
    } else {
        auto_header(header)
    }
}

fn draw_auto_headers(
    painter: &Painter,
    l: &Layout,
    view: &ArrangeView<'_>,
    p: &Ink,
) {
    for row in &l.rows {
        let RowKind::Automation(lane, slot) = row.kind else {
            continue;
        };
        if !row.lane.is_positive() {
            continue;
        }
        if row.header.is_positive() {
            let Some(auto) = auto_view_of(view, lane, slot)
            else {
                continue;
            };
            let face = shade(p.hdr_face, 0.92);
            painter.rect_filled(row.header, 0.0, face);
            // The header half of the seam above, so a row is one object
            // across the grip rather than two that stop at it.
            if slot > 0 {
                painter.line_segment(
                    [
                        Pos2::new(row.header.left(), row.header.top()),
                        Pos2::new(row.header.right(), row.header.top()),
                    ],
                    Stroke::new(1.0, auto_header_seam(&p)),
                );
            }
            if auto.selected {
                painter.rect_stroke(
                    row.header.shrink(1.0),
                    2.0,
                    Stroke::new(1.2, crate::recorder_panel::selection_rim(face)),
                    egui::StrokeKind::Inside,
                );
            }
            let ink = on_ink(face);
            let h = auto_cells(view, lane, row.header);
            // **The × — leave.** A cross rather than a letter: the three
            // switches beside it are about the SIGNAL and this one is about
            // the view, so it says so by not looking like them. Drawn, not
            // set: this crate has no icon font.
            if h.close.is_positive() {
                let c = h.close.center();
                let a = h.close.width() * 0.26;
                let stroke = Stroke::new(crate::fonts::px(h.close.width() * 0.10), quiet(ink, face, 0.28));
                painter.line_segment(
                    [Pos2::new(c.x - a, c.y - a), Pos2::new(c.x + a, c.y + a)],
                    stroke,
                );
                painter.line_segment(
                    [Pos2::new(c.x - a, c.y + a), Pos2::new(c.x + a, c.y - a)],
                    stroke,
                );
            }
            // The record dot, exactly as a track's — a hand that has learned
            // one has learned this.
            if h.arm.is_positive() {
                let c = h.arm.center();
                let rad = h.arm.width() * 0.5;
                if auto.armed {
                    painter.circle_filled(c, rad, p.arm);
                }
                painter.circle_stroke(c, rad, Stroke::new(1.0, quiet(ink, face, 0.30)));
            }
            // **M** — the row's mute. It said B for bypass, which is the
            // same switch under a name only this app used: what it does is
            // stop the curve reaching the control, and on a strip that is
            // what M has always meant. The owner asked for the letter people
            // already know. Lit in the mute colour, as M is everywhere else.
            if h.bypass.is_positive() {
                let on = auto.bypassed;
                let plate = if on { p.mute } else { p.hdr_lane };
                painter.rect_filled(h.bypass, 2.0, plate);
                let size = crate::fonts::px((h.bypass.height() * 0.62).clamp(7.0, 11.0));
                if size >= 7.0 {
                    painter.text(
                        h.bypass.center(),
                        Align2::CENTER_CENTER,
                        "M",
                        FontId::new(size, crate::fonts::courier_bold()),
                        if on { on_ink(plate) } else { quiet(on_ink(plate), plate, 0.18) },
                    );
                }
            }
            // What it controls.
            let size = crate::fonts::px((row.header.height() * 0.46).clamp(7.0, 11.0));
            let room = h.name.width() - 4.0;
            if h.name.is_positive() && size >= 7.0 && room > 20.0 {
                let font = FontId::new(size, crate::fonts::courier());
                // **A muted lane says so in its name too.** The letter is
                // eleven points wide and the name is the thing being read.
                let word_ink = if auto.bypassed {
                    quiet(ink, face, 0.45)
                } else {
                    quiet(ink, face, 0.10)
                };
                painter.text(
                    Pos2::new(h.name.left(), h.name.center().y),
                    Align2::LEFT_CENTER,
                    cut_to(painter, room, &auto.label, &font),
                    font,
                    word_ink,
                );
            }
            // **The control itself, with its number over it.** Under a track
            // the readouts sit below their dials; half a row has no room
            // beneath one, and above is where the eye is already coming from
            // the name.
            if h.dial.is_positive() {
                // Same three colours the track's own dials wear, so the
                // strip below a track reads as part of it.
                dial(
                    painter,
                    h.dial,
                    auto.resting as f32,
                    if auto.bypassed {
                        p.hdr_lane
                    } else {
                        auto_dial_body(p, view.accent)
                    },
                    p.lane,
                    p.clip_ink,
                );
            }
            if h.readout.is_positive() {
                let size = crate::fonts::px((h.readout.height() * 0.86).clamp(6.0, 9.0));
                if size >= 6.0 {
                    painter.text(
                        h.readout.center(),
                        Align2::CENTER_CENTER,
                        &format!("{:.0}%", auto.resting * 100.0),
                        FontId::new(size, crate::fonts::courier()),
                        quiet(ink, face, 0.22),
                    );
                }
            }
        }
    }
}

/// **The ground an automation row stands on**, and it is not a track's.
///
/// It was the parent lane's own stripe, eight per cent darker. That says
/// "part of the track above" and nothing else, and eight per cent is inside
/// the range a lane already varies by between its two stripes — so on most
/// themes the row simply read as a third stripe in the alternation, and on
/// the dark ones as no boundary at all. The owner asked for the grid under an
/// automation row to be a distinct colour, per theme.
///
/// So it is pulled toward the RULER, which is the surface this app already
/// uses for "a strip about the song rather than about a track" — a different
/// character on every theme by construction, because each one derives its
/// ruler from its own wood. The parent's stripe is still in the mix, so odd
/// and even rows stay distinguishable and a row still belongs to the lane it
/// hangs under.
///
/// Through `legible_surface` because a curve, its handles and now a value
/// readout all land on this, and a blend of two themed roles is exactly the
/// kind of derived colour that lands in the dead zone where neither ink
/// clears 4.5:1.
/// **The seam between two stacked automation rows**, derived from the ground
/// it is drawn on.
///
/// `Ink::seam` is separated from the LANE field, and an automation row does
/// not stand on the lane field — it stands on `auto_ground`, which has been
/// moved away from it on purpose. A seam that kept the lane's figure was a
/// line at 1.01:1 against the surface under it on half the themes: drawn,
/// and not there.
fn auto_seam(p: &Ink, lane: usize) -> Color32 {
    separate(auto_ground(p, lane), 0.22)
}

/// The header half of the same seam. A row is one object across the grip, so
/// the line has to be visible on both surfaces — and they are different
/// surfaces, so it is not the same colour.
fn auto_header_seam(p: &Ink) -> Color32 {
    separate(shade(p.hdr_face, 0.92), 0.22)
}

fn auto_ground(p: &Ink, lane: usize) -> Color32 {
    let base = if lane % 2 == 0 { p.lane } else { p.lane_alt };
    // Character first, then DISTANCE. The blend gives the row the ruler's
    // hue so it reads as a strip about something other than a track; on the
    // themes whose ruler already sits near the lane that blend alone changed
    // almost nothing, which is what `separate` is for — it moves a colour
    // away from itself, toward white on a dark field and toward black on a
    // pale one, by an amount that shows either way. The audit above measures
    // the result on every theme in both modes rather than trusting it.
    //
    // 0.26 and not less: measured across every factory theme in both modes,
    // 0.14 and 0.20 both left Centennial and Porcelain at 1.02:1 against
    // their own lane — indistinguishable — and 0.26 is where every theme
    // clears. Ink still reads at 4.7:1 on the worst of them.
    crate::recorder_panel::legible_surface(separate(blend(base, p.ruler, 0.35), 0.26))
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
    // **An automation row takes the ground of the track it belongs to**, a
    // shade recessed. Its own stripe colour would read as a ninth track;
    // being a darker version of its parent's is what says "part of that
    // one" without a line or a label doing the work.
    for row in &l.rows {
        let RowKind::Automation(lane, slot) = row.kind else {
            continue;
        };
        if !row.lane.is_positive() {
            continue;
        }
        painter.rect_filled(row.lane, 0.0, auto_ground(&p, lane));
        // **A seam between stacked rows.** Two lanes on one track are two
        // shades of the same recessed ground with nothing between them, so
        // three open rows read as one tall band and the eye has to count
        // handles to tell where one parameter ends. Only BETWEEN them —
        // `slot > 0` — because the boundary with the track above is already
        // drawn by the track's own plate.
        if slot > 0 {
            painter.line_segment(
                [
                    Pos2::new(row.lane.left(), row.lane.top()),
                    Pos2::new(row.lane.right(), row.lane.top()),
                ],
                Stroke::new(1.0, auto_seam(&p, lane)),
            );
        }
        // **Its HEADER is not drawn here**, and that is the whole point of
        // this comment. See `draw_auto_headers`, called after the header
        // column's wood: painting it in this loop put it UNDER that wood,
        // and every control on it was invisible while remaining perfectly
        // hittable. The owner: "still dont draw, but controls are probably
        // there (i can find the x button i think)."
    }

    // ── the grid: bar lines through the lanes, numbers on the ruler ───────
    //
    // **Walked in BARS, not stepped in seconds.** A bar is a fixed number of
    // seconds only while the tempo is; the moment a song has a change in it,
    // a ruler laid out by multiplying one bar's length puts every line after
    // the change in the wrong place — and the SNAP, which has gone through
    // the maps all along, then lands where the grid is not drawn. See
    // `bar_at`.
    painter.rect_filled(l.ruler, 0.0, p.ruler);
    let step = bar_step(view, l.lanes_area.width());
    let first = (bar_of(view, view.view_start_s) / step) * step;
    let last = bar_of(view, view.view_start_s + view.view_len_s).saturating_add(step);
    let font = FontId::new((l.ruler.height() * 0.55).clamp(8.0, 12.0), crate::fonts::courier());
    // **Bounded.** The step is chosen so this is a few dozen iterations, and
    // the bound is here because a hand-edited map with a one-beat bar in it
    // could otherwise ask the UI thread for millions.
    let mut bars: Vec<(u32, f32)> = Vec::new();
    let mut n = first;
    while n <= last && bars.len() < 512 {
        let x = x_of(rect, view, bar_at(view, n));
        if x >= l.lanes_area.left() - 0.5 && x <= l.lanes_area.right() + 0.5 {
            bars.push((n, x));
        }
        n = n.saturating_add(step);
        if n == u32::MAX {
            break;
        }
    }
    // The numbers, with any that would land on top of the last one dropped —
    // which a ritardando makes routine, because its bars get wider as they go
    // and its step was chosen from the average.
    let mut last_label = f32::NEG_INFINITY;
    let room = font.size * 2.4;
    for &(n, x) in &bars {
        painter.line_segment(
            [
                Pos2::new(x, l.ruler.bottom() - 4.0),
                Pos2::new(x, l.ruler.bottom()),
            ],
            Stroke::new(1.0, p.faint),
        );
        if x - last_label < room {
            continue;
        }
        last_label = x;
        // Bars count from ONE, the way musicians do.
        //
        // **Under the chip strip when there is one.** The strip is carved
        // out of the ruler's top, so a number drawn at the ruler's top edge
        // would be printed behind the signature chips — which is a bar
        // number you cannot read on exactly the songs that have the most to
        // say about their bars.
        painter.text(
            Pos2::new(x + 3.0, l.ruler.top() + 1.0 + l.meter_strip.height().max(0.0)),
            Align2::LEFT_TOP,
            format!("{}", n.saturating_add(1)),
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
            .find(|st| st * (f64::from(l.lanes_area.width()) / view.view_len_s.max(1e-9)) >= 70.0)
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
        // `w` < 1 DARKENS: the strong line (the snap grid) is the darker
        // one on this grey field, the reference shot's own look.
        let line = |x: f32, w: f32| {
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
        // **Walked bar by bar, and the subdivisions live INSIDE their own
        // bar.** The grid used to be a fixed number of seconds stepped from
        // zero, which is two separate wrongs the moment a song has changes
        // in it: after a tempo change every line is in the wrong place, and
        // in a 3/4 bar a quarter-note grid drew four lines where there are
        // three beats. A bar's subdivisions are that bar's own length
        // divided, so 6/8 gets six and 4/4 gets four.
        let exp = i32::from(view.grid_exp.clamp(-6, 6));
        let t1 = view.view_start_s + view.view_len_s;
        let first = bar_of(view, view.view_start_s);
        let mut n = first;
        // Bounded for the reason the ruler's walk is: a hand-edited meter
        // with a one-beat bar in it must not ask the UI thread for millions.
        let mut drawn = 0usize;
        while drawn < 4_096 {
            let q0 = view.meter.quarter_at_bar(n);
            let qn = view.meter.quarters_in_bar(n).max(1.0e-6);
            let t0 = view.tempo.seconds_at_beat(q0);
            if t0 > t1 {
                break;
            }
            let x0 = x_of(rect, view, t0);
            let x1 = x_of(rect, view, view.tempo.seconds_at_beat(q0 + qn));
            let bar_px = f64::from(x1 - x0);
            if exp >= 0 {
                // **A multi-bar grid draws COARSER, visibly** — the owner
                // turned the box to "4" and saw no change, because every bar
                // drew at full weight whatever the grid said. The grid step
                // gets the strong line; the bars inside it drop to the faint
                // weight the subdivisions use.
                let every = 1u32 << exp.min(20);
                let strong = n % every == 0;
                if strong || bar_px >= 5.0 {
                    line(x0, if strong { 1.0 } else { 0.55 });
                    drawn += 1;
                }
            } else {
                if bar_px >= 5.0 {
                    line(x0, 1.0);
                    drawn += 1;
                }
                // **Step in NOTE VALUES across the bar**, rather than
                // dividing the bar by a power of two. A 6/8 bar is three
                // quarters: divided by four it gives dotted eighths, and
                // stepped in eighths it gives the six lines the meter
                // actually has.
                let sig = view.meter.sig_at_bar(n);
                let step_q = grid_quarters(view.grid_exp, sig.beats, sig.unit);
                if step_q > 0.0 && bar_px * step_q / qn >= 5.0 {
                    let mut k = 1u32;
                    while (f64::from(k) * step_q) < qn - 1.0e-9 && k < 512 {
                        let t = view.tempo.seconds_at_beat(q0 + f64::from(k) * step_q);
                        line(x_of(rect, view, t), 0.55);
                        drawn += 1;
                        k += 1;
                    }
                }
            }
            // A bar too narrow to draw is a view too wide to walk one bar at
            // a time: skip ahead by whatever the ruler settled on rather
            // than stepping through ten thousand of them.
            n = n.saturating_add(if bar_px >= 5.0 { 1 } else { step.max(1) });
            if n == u32::MAX {
                break;
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

    // ── the automation curves ─────────────────────────────────────────────
    //
    // **In the CHANNEL's own colour**, which is the same decision the MIDI
    // editor's velocity lane already made (`lane_color`). It costs no new
    // theme role, those colours are already chroma- and luminance-capped per
    // palette, and it means a volume curve under the guitar track is the
    // guitar's green — identifiable at a glance in a stack of them.
    for row in &l.rows {
        let RowKind::Automation(lane, slot) = row.kind else {
            continue;
        };
        if !row.lane.is_positive() {
            continue;
        }
        let Some(auto) = auto_view_of(view, lane, slot) else {
            continue;
        };
        let (cr, cg, cb) = STRIP_COLORS[view
            .lanes
            .get(lane)
            .map_or(0, |v| v.color)
            .min(STRIP_COLORS.len() - 1)];
        // The same surface the row was filled with — one function, so the
        // curve cannot be contrasted against a colour the row is not.
        let ground = auto_ground(&p, lane);
        // The curve has to be legible on the ground it is drawn on, and a
        // channel colour is chosen to CARRY ink rather than to be ink. Lift
        // it away from the ground until it reads — the same walk `quiet`
        // does for text, in the other direction.
        let mut curve = Color32::from_rgb(cr, cg, cb);
        for _ in 0..8 {
            if crate::recorder_panel::contrast_ratio(curve, ground) >= 2.2 {
                break;
            }
            curve = shade(curve, 1.25);
        }
        // **Only the points that can be seen**, found by binary search
        // rather than by walking. A curve is sorted, and a song with a
        // thousand points on a lane would otherwise cost a thousand
        // segments a frame at every zoom — most of them off-screen, and all
        // of them recomputed sixty times a second.
        //
        // One point either side of the window, because the segment that
        // ENTERS the view starts outside it: culling to exactly the visible
        // points would leave the curve beginning at the left edge of the
        // screen instead of arriving there.
        let from_s = view.view_start_s;
        let to_s = view.view_start_s + view.view_len_s;
        let lo = auto
            .points
            .partition_point(|q| q.at_s < from_s)
            .saturating_sub(1);
        let hi = auto.points.partition_point(|q| q.at_s <= to_s);
        let hi = (hi + 1).min(auto.points.len());
        let shown = &auto.points[lo.min(hi)..hi];

        // **An untouched lane draws its control's resting level**, flat, all
        // the way across — see `AutoView::resting`. Without it a freshly
        // exposed lane was an empty stripe, which reads as broken rather than
        // as untouched, and gave the first alt-click nothing to aim at.
        if auto.points.is_empty() {
            let y = auto_y_of(row.lane, auto.resting);
            painter.line_segment(
                [Pos2::new(row.lane.left(), y), Pos2::new(row.lane.right(), y)],
                // Dashed would say "not real"; this level IS what plays. It
                // is drawn quieter than a written curve because nothing has
                // been written yet.
                Stroke::new(1.2, curve.gamma_multiply(0.55)),
            );
            continue;
        }

        // The polyline, built in ONE pass. A hold contributes the step
        // across before the next point's own vertex, which is the whole
        // difference between a switch and a ramp.
        let mut line: Vec<Pos2> = Vec::with_capacity(shown.len() * 2 + 2);
        // **Flat back to the left edge**, because that is what the curve
        // MEANS before its first point — see `Curve::value_at`. A line that
        // began at the first handle would draw a level the song never plays.
        if let Some(first) = auto.points.first() {
            let y = auto_y_of(row.lane, first.value);
            line.push(Pos2::new(row.lane.left(), y));
        }
        for (i, q) in shown.iter().enumerate() {
            let at = Pos2::new(x_of(rect, view, q.at_s), auto_y_of(row.lane, q.value));
            line.push(at);
            if q.shape == crate::automation::Shape::Hold {
                if let Some(next) = shown.get(i + 1) {
                    line.push(Pos2::new(x_of(rect, view, next.at_s), at.y));
                }
            }
        }
        // ...and flat on to the right edge, for the same reason.
        if let Some(last) = auto.points.last() {
            let y = auto_y_of(row.lane, last.value);
            line.push(Pos2::new(row.lane.right(), y));
        }
        for pair in line.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            // A segment wholly outside the row's own x-range draws nothing
            // anybody can see, and a non-finite one draws nothing at all
            // while quietly poisoning the shape it is in.
            if !a.x.is_finite() || !b.x.is_finite() || !a.y.is_finite() || !b.y.is_finite() {
                continue;
            }
            if a.x.max(b.x) < row.lane.left() || a.x.min(b.x) > row.lane.right() {
                continue;
            }
            painter.line_segment([a, b], Stroke::new(1.5, curve));
        }
        // **The handles, clipped to their own row.**
        //
        // A point at time ZERO sits on the lane's left edge, and a handle is
        // a disc with a radius — so four points of it spilled left, into the
        // seven-point resize grip, which is painted later and won over it.
        // The node a freshly opened lane is given lived exactly there: drawn,
        // half buried, and read as absent. The owner: "opening a lane doesn't
        // start with a node at the beginning."
        //
        // Clipping rather than nudging the position: a handle drawn a few
        // points in would be a handle drawn at a time the point is not at,
        // and the first thing anybody does with it is drag it somewhere
        // exact. A half disc against the edge is honest and is what every
        // desk draws there.
        let painter = &painter.with_clip_rect(row.lane);
        for (i, q) in shown.iter().enumerate() {
            let at = Pos2::new(x_of(rect, view, q.at_s), auto_y_of(row.lane, q.value));
            if !at.x.is_finite() || !at.y.is_finite() {
                continue;
            }
            if at.x < row.lane.left() - AUTO_POINT_R || at.x > row.lane.right() + AUTO_POINT_R {
                continue;
            }
            let hot = auto.hot == Some(lo + i);
            let chosen = auto.chosen.contains(&(lo + i));
            painter.circle_filled(at, if hot { AUTO_POINT_R + 1.0 } else { AUTO_POINT_R }, curve);
            // A ring in the ground's own colour, so two points close
            // together read as two rather than as a blob.
            painter.circle_stroke(at, AUTO_POINT_R, Stroke::new(1.0, ground));
            // **Chosen wears the selection rim**, the one a chosen track and
            // a chosen automation row already wear — outside the ground ring
            // so it reads at the sizes these handles actually are.
            if chosen {
                painter.circle_stroke(
                    at,
                    AUTO_POINT_R + 2.0,
                    Stroke::new(1.4, crate::recorder_panel::selection_rim(ground)),
                );
            }
            // **The value, over the point being dragged.**
            //
            // A curve is the one editable thing on this timeline whose value
            // could not be read while it was being set: the height of a dot
            // in a half-height row is not a number anybody can name, and the
            // header's readout shows where the CONTROL is standing, not where
            // the hand has taken this node. The owner asked for it while
            // moving, so it appears while moving and not on hover.
            if auto.moving == Some(lo + i) {
                let size = crate::fonts::px((row.lane.height() * 0.34).clamp(8.0, 12.0));
                let font = FontId::new(size, crate::fonts::courier_bold());
                // Above the handle, and pushed back inside the row when the
                // point is near the top — a readout off the top of the lane
                // is the same as no readout.
                let want_y = at.y - AUTO_POINT_R - size * 0.75;
                let y = want_y.max(row.lane.top() + size * 0.6);
                // And inside it horizontally, so a node at either end still
                // shows its number.
                let half = size * 2.0;
                let x = at.x.clamp(row.lane.left() + half, row.lane.right() - half);
                let text = format!("{:.0}%", q.value * 100.0);
                // A plate under it: the curve, the grid and the clips are all
                // behind this, and a bare number over them is unreadable on
                // half the themes.
                let galley = painter.layout_no_wrap(text, font.clone(), on_ink(ground));
                let plate = Rect::from_center_size(
                    Pos2::new(x, y),
                    galley.size() + Vec2::new(6.0, 2.0),
                );
                painter.rect_filled(plate, 2.0, ground);
                painter.galley(plate.center() - galley.size() * 0.5, galley, on_ink(ground));
            }
        }
    }

    // ── the tempo lane ────────────────────────────────────────────────────
    //
    // The song's own speed, drawn where a curve belongs: a line across a
    // row, with a handle at every point. Two things make it not an
    // automation lane — the scale is LOGARITHMIC in bpm (see `tempo_y_of`),
    // and a segment is either a step or a RAMP, which is the difference
    // between "and now it is slower" and a ritardando.
    if let Some(row) = l.rows.iter().find(|r| r.kind == RowKind::Tempo) {
        if row.lane.is_positive() {
            // **Its own ground, and visibly so.** At a shade off the lane
            // stripes the row read as a ninth track with a red line through
            // it; the tempo is a property of the SONG and the row has to say
            // that before anything in it is read.
            let ground = shade(p.lane, 0.82);
            painter.rect_filled(row.lane, 0.0, ground);
            painter.line_segment(
                [row.lane.left_bottom(), row.lane.right_bottom()],
                Stroke::new(1.0, p.seam),
            );
            let mut curve = p.playhead;
            for _ in 0..8 {
                if crate::recorder_panel::contrast_ratio(curve, ground) >= 2.2 {
                    break;
                }
                curve = shade(curve, 1.25);
            }
            let pts = view.tempo.points();
            // The polyline, in seconds. A HOLD steps across to the next
            // point's x at the old height; a RAMP goes straight there.
            let mut line: Vec<Pos2> = Vec::with_capacity(pts.len() * 2 + 2);
            if let Some(first) = pts.first() {
                line.push(Pos2::new(row.lane.left(), tempo_y_of(row.lane, first.bpm)));
            }
            for (i, q) in pts.iter().enumerate() {
                let x = x_of(rect, view, view.tempo.seconds_at_beat(q.at_beat));
                let at = Pos2::new(x, tempo_y_of(row.lane, q.bpm));
                line.push(at);
                if let Some(next) = pts.get(i + 1) {
                    let nx = x_of(rect, view, view.tempo.seconds_at_beat(next.at_beat));
                    // **The RAMP belongs to the point it slides FROM**, which
                    // is `TempoPoint::ramp`'s own words and what
                    // `bpm_at_beat` reads: a ramp on point *i* interpolates
                    // across the span to *i+1*. So the segment leaving this
                    // point is a slope when THIS point says so, and a step
                    // held at this point's height when it does not.
                    if !q.ramp {
                        line.push(Pos2::new(nx, at.y));
                    }
                }
            }
            if let Some(last) = pts.last() {
                line.push(Pos2::new(row.lane.right(), tempo_y_of(row.lane, last.bpm)));
            }
            for pair in line.windows(2) {
                let (a, b) = (pair[0], pair[1]);
                if !a.x.is_finite() || !b.x.is_finite() || !a.y.is_finite() || !b.y.is_finite() {
                    continue;
                }
                if a.x.max(b.x) < row.lane.left() || a.x.min(b.x) > row.lane.right() {
                    continue;
                }
                painter.line_segment([a, b], Stroke::new(1.5, curve));
            }
            let font = FontId::new(
                (row.lane.height() * 0.34).clamp(7.0, 11.0),
                crate::fonts::courier(),
            );
            for (i, q) in pts.iter().enumerate() {
                let x = tempo_x_of(rect, view, row.lane, q.at_beat);
                let at = Pos2::new(x, tempo_y_of(row.lane, q.bpm));
                if !at.x.is_finite() || !at.y.is_finite() {
                    continue;
                }
                if at.x < row.lane.left() - AUTO_POINT_R || at.x > row.lane.right() + AUTO_POINT_R {
                    continue;
                }
                let hot = view.tempo_hot == Some(i);
                painter.circle_filled(at, if hot { AUTO_POINT_R + 1.0 } else { AUTO_POINT_R }, curve);
                painter.circle_stroke(at, AUTO_POINT_R, Stroke::new(1.0, ground));
                // The number beside the handle, so a lane read at a glance
                // says what it is and not merely that something happens.
                if font.size >= 7.0 && (hot || pts.len() <= 24) {
                    painter.text(
                        Pos2::new(at.x + AUTO_POINT_R + 2.0, at.y),
                        Align2::LEFT_CENTER,
                        format!("{:.0}", q.bpm),
                        font.clone(),
                        p.faint,
                    );
                }
            }
        }
        // The header: the word, and what the map is doing.
        if row.header.is_positive() {
            painter.rect_filled(row.header, 0.0, p.hdr_lane);
            let font = FontId::new(
                (row.header.height() * 0.42).clamp(7.0, 11.0),
                crate::fonts::courier(),
            );
            painter.text(
                Pos2::new(row.header.left() + 6.0, row.header.center().y),
                Align2::LEFT_CENTER,
                "TEMPO",
                font.clone(),
                p.engrave,
            );
            let says = if view.tempo.is_constant() {
                format!("{:.0}", view.tempo.bpm_at_beat(0.0))
            } else {
                format!("{} pts", view.tempo.points().len())
            };
            painter.text(
                Pos2::new(row.header.right() - 6.0, row.header.center().y),
                Align2::RIGHT_CENTER,
                says,
                font,
                p.faint,
            );
        }
    }

    // ── the time signatures, as chips along the ruler's own top ───────────
    //
    // On the ruler and not in a lane of their own: a signature change is a
    // property of a BAR, the ruler is what counts bars, and a second row of
    // song-wide markers would take height from the tracks to say something
    // the bar numbers are already sitting next to.
    if l.meter_strip.is_positive() {
        let font = FontId::new(
            (l.meter_strip.height() * 0.78).clamp(6.0, 10.0),
            crate::fonts::courier(),
        );
        for pt in view.meter.points() {
            // Bar 0's signature is the song's, not a CHANGE — there is
            // nothing before it for it to be different from, and a chip on
            // the first bar of every song would be noise on the overwhelming
            // majority that never change.
            if pt.at_bar == 0 && view.meter.points().len() == 1 {
                continue;
            }
            let x = x_of(rect, view, bar_at(view, pt.at_bar));
            if x < l.meter_strip.left() - 2.0 || x > l.meter_strip.right() + 2.0 {
                continue;
            }
            let typing = view.meter_edit.filter(|(b, _)| *b == pt.at_bar);
            let text = match typing {
                Some((_, t)) => format!("{t}|"),
                None => format!("{}/{}", pt.sig.beats, pt.sig.unit),
            };
            // A minimum width, so a chip emptied down to its caret is still
            // a rectangle somebody can see and aim at.
            let w = (font.size * 0.62 * text.chars().count() as f32 + 6.0).max(font.size * 2.2);
            let chip = Rect::from_min_size(
                Pos2::new(x, l.meter_strip.top()),
                Vec2::new(w, l.meter_strip.height()),
            );
            painter.rect_filled(chip, 2.0, if typing.is_some() { p.select } else { p.face });
            painter.text(
                chip.center(),
                Align2::CENTER_CENTER,
                &text,
                font.clone(),
                p.engrave,
            );
        }
    }

    // ── the clips ─────────────────────────────────────────────────────────
    for clip in view.clips {
        let r = clip_rect(rect, view, clip);
        if !r.is_positive() {
            continue;
        }
        // Through the dead-zone guard, like the strip and the header plate:
        // a hand-painted lane can point its clips at the grid's three
        // unreadable third tones, and the auto palettes' contract does not
        // cover the hand grid.
        let color = crate::recorder_panel::legible_surface(
            view.lanes
                .get(clip.lane)
                .map(|lane| lane.color)
                .filter(|c| *c != 0)
                .map(|c| {
                    let (cr, cg, cb) = STRIP_COLORS[c.min(STRIP_COLORS.len() - 1)];
                    Color32::from_rgb(cr, cg, cb)
                })
                // An unpainted lane's clips are the desk's green — the
                // timeline wave's own colour, so the two surfaces agree what
                // audio looks like.
                .unwrap_or(Color32::from_rgb(0x2E, 0x6B, 0x46)),
        );
        // Measured on THIS clip rather than taken from `p`: the guard can
        // move an olive to the side of the dead zone where the dark ink is
        // the one that reads, and `p.clip_ink` is the light one. Everything
        // printed inside the clip — name, staff, wave, fades, handles —
        // takes this ink, so the clip cannot half-agree with itself.
        let clip_ink = on_ink(color);
        painter.rect_filled(r, 3.0, color);
        // A MIDI item draws its NOTES in miniature — the arrangement can
        // read what the editor will say. An empty item keeps the dashed
        // staff that says "MIDI, nothing played yet".
        if clip.midi && r.width() >= 8.0 {
            if clip.notes.is_empty() {
                // The dashed staff sits under the title too, or an empty
                // MIDI item's line runs through its own name.
                let band = clip_name_band(r);
                let mid = (r.top() + band + r.bottom()) * 0.5;
                let mut x = r.left() + 3.0;
                while x < r.right() - 3.0 {
                    painter.line_segment(
                        [Pos2::new(x, mid), Pos2::new((x + 4.0).min(r.right() - 3.0), mid)],
                        Stroke::new(1.0, clip_ink.gamma_multiply(0.5)),
                    );
                    x += 8.0;
                }
            } else {
                let full_x = (
                    x_of(rect, view, clip.start_s),
                    x_of(rect, view, clip.start_s + clip.len_s),
                );
                draw_note_miniature(painter, r, full_x, clip, clip_ink);
            }
        }
        // ── the wave ──────────────────────────────────────────────────────
        //
        // **Mapped through the seconds the picture actually covers**, not
        // through the clip. The host draws only what is on screen, at the
        // pixels it occupies — so the picture's span moves and narrows as
        // the view does, and a mapping that assumed the whole clip would
        // stretch a close-up of four seconds across four minutes.
        //
        // **And drawn as MIN to MAX**, not as a peak mirrored about the
        // middle. Audio is not symmetric; the outline of both sides is the
        // shape people read, and one number per column is a bar chart of
        // loudness wearing a waveform's clothes.
        let pairs = clip.wave.len() / 2;
        if pairs > 0 && clip.wave_span_s > 0.0 && r.width() >= 4.0 {
            let mid = r.center().y;
            let half = r.height() * 0.40;
            let cols = r.width().ceil() as usize;
            // Seconds per pixel, from the panel's own mapping so this cannot
            // drift from the grid.
            let t0 = time_at(rect, view, r.left());
            let t1 = time_at(rect, view, r.left() + 1.0);
            let dt = (t1 - t0).abs().max(1.0e-9);
            let ink = clip_ink.gamma_multiply(0.62);
            for i in 0..cols {
                let x = r.left() + i as f32;
                if x > r.right() {
                    break;
                }
                let t = t0 + f64::from(i as f32) * dt;
                let f = (t - clip.wave_from_s) / clip.wave_span_s;
                if !(0.0..1.0).contains(&f) {
                    continue;
                }
                // **The whole bucket range this column covers**, so zooming
                // OUT is a filled band rather than a comb of samples: one
                // pixel can hold many buckets, and taking only the first is
                // how a waveform turns into aliasing hash.
                let a = (f * pairs as f64) as usize;
                let b = (((t + dt - clip.wave_from_s) / clip.wave_span_s) * pairs as f64)
                    .ceil() as usize;
                let (a, b) = (a.min(pairs - 1), b.clamp(a + 1, pairs));
                let mut lo = f32::INFINITY;
                let mut hi = f32::NEG_INFINITY;
                for k in a..b {
                    lo = lo.min(clip.wave[k * 2]);
                    hi = hi.max(clip.wave[k * 2 + 1]);
                }
                if !lo.is_finite() || !hi.is_finite() {
                    continue;
                }
                let y0 = mid - hi.clamp(-1.0, 1.0) * half;
                let y1 = mid - lo.clamp(-1.0, 1.0) * half;
                painter.line_segment(
                    [Pos2::new(x, y0.min(y1)), Pos2::new(x, y1.max(y0) + 0.5)],
                    Stroke::new(1.0, ink),
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
                Stroke::new(1.0, clip_ink),
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
                Stroke::new(1.0, clip_ink),
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
                painter.circle_filled(Pos2::new(x, r.top() + FADE_W * 0.4), 2.0, clip_ink);
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
                clip_ink,
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


    // ── SILENCED BY A FOLDER: a veil over the row ────────────────────────
    //
    // **After the clips, so the clips dim too**, and before the playhead and
    // the time selection, which are about where you are rather than about
    // what is heard.
    //
    // The child's own M switch is off and stays off — a folder does not mute
    // its children, it contains them, and unmuting the folder gives every
    // child back exactly as it was. But a track that makes no sound with its
    // mute clearly off reads as a fault, so the lane says it plainly. The
    // FOLDER dims too: it is silent for the same reason and a group half
    // dimmed would look like a bug in the dimming.
    for (i, row) in l.lane_rows.iter().enumerate() {
        let Some(lane) = view.lanes.get(i).filter(|l| l.folder_silent) else {
            continue;
        };
        let _ = lane;
        if row.is_positive() {
            painter.rect_filled(*row, 0.0, Color32::from_black_alpha(96));
        }
    }
    for (i, row) in l.header_rows.iter().enumerate() {
        let Some(lane) = view.lanes.get(i).filter(|l| l.folder_silent) else {
            continue;
        };
        let _ = lane;
        if row.panel.is_positive() {
            painter.rect_filled(row.panel, 0.0, Color32::from_black_alpha(96));
        }
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
    // **The automation rows' headers, ON the wood rather than under it.**
    //
    // They used to be painted up in the lane loop, hundreds of lines before
    // the header column gets its `hdr_wood` — so the face, the x, the arm,
    // the bypass, the name and the dial were all drawn and then covered by a
    // single rect. Nothing failed and nothing looked broken from the code's
    // side: the hit test is geometric and went on working, so the controls
    // could be PRESSED and not seen.
    draw_auto_headers(painter, &l, view, &p);

    let num_font = FontId::new(10.0, crate::fonts::courier());
    // **The MASTER, above the tracks, in the mixer's own colours.**
    //
    // The desk draws a strip for it and the timeline drew none, so a
    // session's output level was reachable from one view and not the other.
    // A full track's height, and it wears the SAME painted plate its mixer
    // strip does — `master_color` is that strip's own index — so the two
    // views cannot disagree about what the master looks like on a theme.
    {
        let r = l.master_row;
        if r.panel.is_positive() {
            // The strip's colour as a plate, and the ink measured against
            // it — the mixer's rule, and the same one the track headers
            // below follow: any colour, and the text still reads.
            // **Themed, always.** With a palette on, the master takes its
            // palette slot like every track. With one off it used to fall
            // back to the bare header face and read as a strip nobody had
            // dressed — the owner's "make master 'channel' strip have a
            // themed background and text subject to the theming of tracks."
            // The theme's own master red is what it falls back to now: it is
            // per theme by construction, and it is the same colour its dial
            // wears, so the row reads as one object.
            let face = crate::recorder_panel::legible_surface(if view.master_color != 0 {
                let (cr, cg, cb) =
                    crate::mixer_panel::STRIP_COLORS[view.master_color.min(
                        crate::mixer_panel::STRIP_COLORS.len() - 1,
                    )];
                Color32::from_rgb(cr, cg, cb)
            } else {
                // Darkened toward a PLATE: the knob colour at full strength
                // is a control's colour, and a whole row of it would shout
                // over the sixteen tracks it sits above.
                shade(master_dial_body(view.master_knob), 0.55)
            });
            let ink = if crate::recorder_panel::wants_light_ink(face) {
                crate::recorder_panel::INK_LIGHT
            } else {
                crate::recorder_panel::INK_DARK
            };
            painter.rect_filled(r.panel, 0.0, face);
            painter.line_segment(
                [
                    Pos2::new(r.panel.left(), r.panel.bottom()),
                    Pos2::new(r.panel.right(), r.panel.bottom()),
                ],
                Stroke::new(1.0, ink.gamma_multiply(0.35)),
            );
            // **A** — its automation menu, the same button a track wears,
            // lit while the master is showing any row. Drawn against the
            // master's own plate rather than the header face, because that
            // plate is a palette colour and the ink was measured against it
            // two lines up.
            if r.automation.is_positive() {
                let on = !view.master_automation.is_empty();
                painter.rect_filled(
                    r.automation,
                    2.0,
                    if on { p.select } else { ink.gamma_multiply(0.14) },
                );
                let size = crate::fonts::px((r.automation.height() * 0.68).clamp(7.0, 11.0));
                if size >= 7.0 {
                    let plate = if on { p.select } else { face };
                    painter.text(
                        r.automation.center(),
                        Align2::CENTER_CENTER,
                        "A",
                        FontId::new(size, crate::fonts::courier_bold()),
                        on_ink(plate),
                    );
                }
            }
            if r.name.is_positive() {
                let size = (r.name.height() * 0.42).clamp(6.0, 11.0);
                painter.text(
                    Pos2::new(r.name.left() + 4.0, r.name.center().y),
                    Align2::LEFT_CENTER,
                    "MASTER",
                    FontId::new(size, crate::fonts::courier()),
                    ink,
                );
            }
            if r.meter.is_positive() {
                painter.rect_filled(r.meter, 1.0, ink.gamma_multiply(0.18));
                for (k, v) in view.master_peak.iter().enumerate() {
                    let w = r.meter.width() * 0.5;
                    let x = r.meter.left() + w * k as f32;
                    let h = r.meter.height() * v.clamp(0.0, 1.0);
                    if h > 0.5 {
                        painter.rect_filled(
                            Rect::from_min_max(
                                Pos2::new(x, r.meter.bottom() - h),
                                Pos2::new(x + w - 0.5, r.meter.bottom()),
                            ),
                            0.0,
                            p.meter,
                        );
                    }
                }
            }
            // **The master's own rack, six bays wide.** The same chips the
            // track headers carry and the same door behind them, so the
            // output chain is reachable from the timeline as well as the desk.
            for (b, cell) in r.bays.iter().enumerate() {
                let word = view.master_inserts.get(b).copied().unwrap_or("");
                bay_chip(
                    painter,
                    *cell,
                    if word.is_empty() { "+" } else { word },
                    !word.is_empty(),
                    &p,
                );
            }
            if r.knob.is_positive() {
                // **The desk's own dial, in the desk's own volume blue.** The
                // ring is the master's plate rather than the lane field the
                // track dials sit on, so it reads as raised out of THIS row.
                dial(
                    painter,
                    r.knob,
                    crate::recorder::gain_to_fader(view.master_gain),
                    // **The theme's red, outlined black, with a white
                    // needle.** The channels' volume dials wear the blue; the
                    // master is the one control that can undo every other
                    // one, and it says so in the theme's own master colour
                    // rather than in a fixed crimson laid over all sixteen.
                    //
                    // The outline and the needle are FIXED, the way the bus
                    // slot's two colours are and for the same reason: they
                    // are the instrument, not the panel, and a near-black
                    // ring with a near-white needle means the same thing on
                    // every theme. Being fixed is also what stops them being
                    // dragged dark-on-dark by whatever red the theme brought.
                    master_dial_body(view.master_knob),
                    DIAL_MASTER_RING,
                    DIAL_MASTER_NEEDLE,
                );
            }
            // Its number, above the dial. Same shape a track's takes, and
            // the same compact dB: the unit has no room at this size.
            if r.knob_readout.is_positive() {
                let g = view.master_gain;
                let db = if g <= 0.001 {
                    "-inf".to_owned()
                } else {
                    format!("{:+.1}", 20.0 * g.log10())
                };
                let size = crate::fonts::px((r.knob_readout.height() * 0.86).clamp(6.0, 9.0));
                if size >= 6.0 {
                    painter.text(
                        r.knob_readout.center(),
                        Align2::CENTER_CENTER,
                        &db,
                        FontId::new(size, crate::fonts::courier()),
                        quiet(ink, face, 0.22),
                    );
                }
            }
            // The word under the meter, saying what the dial is — the track
            // rows write theirs under the knob and this row has no room
            // below, so it goes beside.
            if r.vol_label.is_positive() {
                painter.text(
                    r.vol_label.center(),
                    Align2::CENTER_CENTER,
                    "VOL",
                    FontId::new(
                        (r.vol_label.height() * 0.62).clamp(6.0, 9.0),
                        crate::fonts::courier(),
                    ),
                    quiet(ink, face, 0.28),
                );
            }
        }
    }
    for (i, row) in l.header_rows.iter().enumerate() {
        let Some(lane) = view.lanes.get(i) else { continue };
        // The strip's own colour as a plate behind the header, the mixer's
        // rule: any colour, and the text still reads. Through the dead-zone
        // guard for the same reason the mixer's strip goes through it: three
        // of the hand-picking grid's third tones sit where neither ink
        // clears, and the indices are saved in desks so the SURFACE moves at
        // draw time rather than the table.
        let face = if lane.color != 0 {
            let (cr, cg, cb) = STRIP_COLORS[lane.color.min(STRIP_COLORS.len() - 1)];
            crate::recorder_panel::legible_surface(Color32::from_rgb(cr, cg, cb))
        } else {
            p.hdr_face
        };
        let plate = {
            let base = row.panel.shrink2(Vec2::new(2.0, 1.5));
            // The same step the controls took, so the plate and what stands
            // on it agree about where this track begins.
            if lane.in_folder && base.width() > FOLDER_INDENT + 40.0 {
                Rect::from_min_max(
                    Pos2::new(base.left() + FOLDER_INDENT, base.top()),
                    base.max,
                )
            } else {
                base
            }
        };
        if !plate.is_positive() {
            continue;
        }
        if lane.used {
            painter.rect_filled(plate, 2.0, face);
            if view.selected_tracks.contains(&i) {
                painter.rect_stroke(
                    plate,
                    2.0,
                    Stroke::new(1.2, crate::recorder_panel::selection_rim(face)),
                    egui::StrokeKind::Inside,
                );
            }
        } else {
            // The empty slot's own ground fills the WHOLE panel — the wood
            // margin around a dark plate read as two surfaces, the owner's
            // "my bad" screenshot.
            painter.rect_filled(row.panel, 0.0, p.empty_ground);
            // **And an empty slot wears the rim too**, like the mixer's.
            //
            // An empty slot is a destination now — a copied track is pasted
            // INTO one — so it has to be able to say it is the one chosen.
            // Without this a press on an empty row changed the selection and
            // nothing on screen moved, which reads as a press that did not
            // land. On the panel rather than the plate, because the ground
            // it is drawn against is the panel.
            if view.selected_tracks.contains(&i) {
                painter.rect_stroke(
                    row.panel.shrink(1.0),
                    2.0,
                    Stroke::new(1.2, crate::recorder_panel::selection_rim(p.empty_ground)),
                    egui::StrokeKind::Inside,
                );
            }
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
                // **Measured against the EMPTY ground, like the + beside it.**
                //
                // This was `hdr_face` dimmed to a third — the themed STRIP
                // colour, used as ink, on a ground it has nothing to do with.
                // On every theme whose strip is dark that is a dark number on
                // a dark ground, and the row numbers were unreadable on most
                // of the sixteen. The + circle two inches away had the right
                // answer the whole time.
                quiet(on_ink(p.empty_ground), p.empty_ground, 0.22)
            },
        );
        // The input chip draws on EVERY lane — binding an input is how a
        // bare lane becomes a track — in bone whatever the theme, so the
        // words read on a painted header and on the grey field alike.
        let chip =
            |painter: &Painter, r: Rect, word: &str, has: bool| bay_chip(painter, r, word, has, &p);
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
        // The arm dot: REAPER's record circle, lit when live. A FOLDER has
        // no arm - its rect is NOTHING - and a stroke from a degenerate
        // rect is a smudge, so the guard is load-bearing.
        if row.arm.is_positive() {
            let c = row.arm.center();
            let rad = row.arm.width() * 0.5;
            if lane.armed {
                painter.circle_filled(c, rad, p.arm);
            }
            painter.circle_stroke(c, rad, Stroke::new(1.5, ink_on.gamma_multiply(0.8)));
        }
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
            self::dial(painter, cell, t, cap, p.lane, p.clip_ink);
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
        dial(painter, row.knob, crate::recorder::gain_to_fader(lane.gain), DIAL_VOLUME);
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
        // **T**, beside the arm: this track's MIDI reaches the theory view.
        //
        // Drawn with the M and the S rather than on its own, so a hand that
        // has learned those two recognises it — in the theme's own selection
        // colour, because it is neither a mute nor a solo and three switches
        // wearing two colours is one of them being mistaken for another.
        for (r, label, on, lit) in [
            (row.theory, "T", lane.to_theory, p.select),
            // **A** — lit when this track is showing any automation row, so
            // the header says at a glance that the rows below belong to it.
            (row.automation, "A", !lane.automation.is_empty(), p.select),
            // Ø on the theme's own selection ink, like T and A above it: it
            // is neither a mute nor a solo, and a third warning colour on a
            // header carrying two would read as a third kind of danger.
            (row.phase, "\u{d8}", lane.phase, p.select),
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
        // **The FOLDER box, bottom-left.**
        //
        // **The collapse arrow, and what a shut folder is hiding.**
        //
        // Down when the group is open, right when it is shut — the direction
        // every file list has used for forty years, so it needs no legend.
        // The count is written beside it only when SHUT: an open folder's
        // children are on screen and counting them out loud says nothing.
        if row.collapse.is_positive() && lane.is_folder {
            let cr = row.collapse;
            let ink = quiet(on_ink(p.hdr_lane), p.hdr_lane, 0.22);
            let c = cr.center();
            let a = cr.width() * 0.30;
            let tri = if lane.folder_shut {
                [
                    Pos2::new(c.x - a * 0.6, c.y - a),
                    Pos2::new(c.x - a * 0.6, c.y + a),
                    Pos2::new(c.x + a * 0.8, c.y),
                ]
            } else {
                [
                    Pos2::new(c.x - a, c.y - a * 0.6),
                    Pos2::new(c.x + a, c.y - a * 0.6),
                    Pos2::new(c.x, c.y + a * 0.8),
                ]
            };
            painter.add(egui::Shape::convex_polygon(tri.to_vec(), ink, Stroke::NONE));
            if lane.folder_shut && lane.folder_count > 0 {
                painter.text(
                    Pos2::new(cr.right() + 3.0, c.y),
                    Align2::LEFT_CENTER,
                    format!("{}", lane.folder_count),
                    FontId::new(
                        (cr.height() * 0.82).clamp(7.0, 10.0),
                        crate::fonts::courier_bold(),
                    ),
                    ink,
                );
            }
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
    // and the TOOLBAR standing in it: the click, the magnet, the tempo
    // lane's switch, the grid.
    painter.rect_filled(
        Rect::from_min_max(rect.min, Pos2::new(l.header.right(), l.ruler.bottom())),
        0.0,
        p.hdr_wood,
    );
    // **A tool button, and the ink it must be drawn with.**
    //
    // Returning the ink rather than a bool is the fix for the owner's "the
    // met and the snap are basically invisible when disabled". Every caller
    // worked its own out as `p.hdr_face.gamma_multiply(0.7)` — the STRIP's
    // colour, made transparent, painted on the LANE's. Two things wrong with
    // that and both were doing damage: `gamma_multiply` lowers ALPHA, so the
    // glyph composites into whatever is behind it (the bug this file's
    // `quiet` already exists to replace), and `hdr_face` is a plate colour
    // that on many themes is a near neighbour of `hdr_lane` — a faint version
    // of a colour that already matched the ground.
    //
    // So the OFF state is the ground's own ink pulled back a little, which is
    // legible by construction on any theme, and the ON state is a COLOUR
    // change rather than an appearance from nothing: the desk's green, held
    // clear of whichever ground it lands on.
    let tool = |painter: &Painter, r: Rect, on: bool| -> Option<Color32> {
        if !r.is_positive() {
            return None;
        }
        let ground = if on { p.hdr_face } else { p.hdr_lane };
        painter.rect_filled(r, 2.0, ground);
        Some(if on {
            crate::recorder_panel::distinct_surface_at(p.meter, ground, 3.0)
        } else {
            quiet(on_ink(ground), ground, 0.16)
        })
    };
    if let Some(ink) = tool(painter, l.tool_met, view.metronome_on) {
        crate::recorder_panel::draw_metronome_on(
            painter,
            l.tool_met.shrink(2.0),
            ink,
            if view.metronome_on { p.hdr_face } else { p.hdr_lane },
        );
    }
    // The tempo lane's switch: a small line that steps, which is what the
    // lane draws. A metronome would be the click's own sign twice over.
    if let Some(ink) = tool(painter, l.tool_tempo, view.show_tempo_lane) {
        let m = l.tool_tempo.shrink(3.5);
        let w = (m.width() * 0.14).clamp(1.2, 2.2);
        let (lo, hi) = (m.bottom() - m.height() * 0.18, m.top() + m.height() * 0.18);
        let mid = m.left() + m.width() * 0.5;
        for pair in [
            [Pos2::new(m.left(), lo), Pos2::new(mid, lo)],
            [Pos2::new(mid, lo), Pos2::new(mid, hi)],
            [Pos2::new(mid, hi), Pos2::new(m.right(), hi)],
        ] {
            painter.line_segment(pair, Stroke::new(w, ink));
        }
    }
    if let Some(ink) = tool(painter, l.tool_snap, view.snap_on) {
        // The magnet: a U with its two poles, the sign every DAW uses.
        let m = l.tool_snap.shrink(3.5);
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
    // The GRID box is always "on" — it is a readout, not a switch — so it
    // takes the lit ground and prints in the panel's own ink rather than the
    // green a switch uses to say it is engaged.
    // The meter at the START of the view: the grid box names a note value,
    // and in a song that changes meter the one on screen is the one that
    // matters. `sig_at_bar` is the same lookup the grid lines use.
    let sig_now = view.meter.sig_at_bar(bar_of(view, view.view_start_s));
    if tool(painter, l.tool_grid, true).is_some() {
        painter.text(
            l.tool_grid.center(),
            Align2::CENTER_CENTER,
            grid_label(view.grid_exp, sig_now.beats, sig_now.unit),
            FontId::new(
                (l.tool_grid.height() * 0.55).clamp(8.0, 11.0),
                crate::fonts::courier_bold(),
            ),
            on_ink(p.hdr_face),
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
    /// **An empty slot can be pointed at.**
    ///
    /// It had no `name` and no `number` rect, so `hit_test` answered nothing
    /// over most of its row — which meant it could not be selected, could not
    /// wear the selection rim, and could not be the paste destination you had
    /// aimed at. The + circle and the input chip keep their seats; the rest of
    /// the row is the slot.
    #[test]
    fn an_empty_slot_answers_a_press_on_its_row() {
        // `a_view` makes lanes 0..3 used and the rest empty.
        let clips: [ClipView<'_>; 0] = [];
        let l = Layout::new(rect(), &a_view(&clips));
        let row = l.header_rows[4];
        assert!(row.panel.is_positive(), "the empty row has no panel");
        assert!(row.add.is_positive(), "the + circle lost its seat");
        assert!(row.input.is_positive(), "the input chip lost its seat");
        assert!(
            row.name.is_positive(),
            "an empty slot still has nowhere to be pressed"
        );
        // The three do not overlap: pressing the + must make a track and
        // pressing the chip must bind an input, not select.
        assert!(row.add.right() <= row.input.left());
        assert!(row.input.right() <= row.name.left());
    }


    /// **The first tempo handle can actually be pressed.**
    ///
    /// Every map begins at beat zero, so with the view at the song's start
    /// that handle lands exactly on the lane's left edge — half of it drawn
    /// outside the lane, and everything to the left of that edge belonging to
    /// the header, which the hit test asks about first. Only the right half
    /// was clickable, which is why it read as barely there.
    #[test]
    fn the_first_tempo_handle_is_inside_the_lane_and_hittable() {
        let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 600.0));
        let mut v = a_view(&[]);
        v.show_tempo_lane = true;
        v.view_start_s = 0.0;
        v.view_len_s = 30.0;
        let l = Layout::new(rect, &v);
        let row = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Tempo)
            .expect("no tempo row");
        assert!(row.lane.is_positive(), "the tempo lane has no room");

        // The handle for the point at beat zero.
        let x = tempo_x_of(rect, &v, row.lane, 0.0);
        assert!(
            x >= row.lane.left() + AUTO_POINT_R - 0.01,
            "the first handle is drawn half outside the lane: x={x} left={}",
            row.lane.left()
        );
        // A whole dot's width of it is inside, so a press anywhere on it lands.
        for dx in [-AUTO_POINT_R + 0.5, 0.0, AUTO_POINT_R - 0.5] {
            let at = Pos2::new(x + dx, tempo_y_of(row.lane, v.tempo.points()[0].bpm));
            assert!(
                row.lane.expand(AUTO_POINT_R).contains(at),
                "a point on the first handle at dx={dx} is outside the grab area"
            );
            assert!(
                !row.header.contains(at),
                "a point on the first handle at dx={dx} belongs to the header"
            );
        }
    }

    /// **The automation strip's cells never overlap and never leave the row.**
    ///
    /// `Rect::contains` is inclusive at both edges, so two cells that merely
    /// touch share a line of pixels and a press on it belongs to whichever
    /// the hit test asks about first — which is exactly how a control ends up
    /// doing its neighbour's job.
    #[test]
    fn the_automation_strip_cells_are_separate_and_inside_the_row() {
        for w in [140.0_f32, 220.0, 320.0] {
            for h in [13.0_f32, 18.0, 26.0] {
                let header = Rect::from_min_size(Pos2::new(10.0, 40.0), Vec2::new(w, h));
                let a = auto_header(header);
                let cells = [
                    ("close", a.close),
                    ("arm", a.arm),
                    ("bypass", a.bypass),
                    ("name", a.name),
                    ("dial", a.dial),
                    ("readout", a.readout),
                ];
                for (n, r) in cells {
                    if !r.is_positive() {
                        continue;
                    }
                    assert!(
                        header.contains(r.min) && header.contains(r.max),
                        "{n} leaves the row at {w}x{h}: {r:?} not in {header:?}"
                    );
                }
                for (i, (an, ar)) in cells.iter().enumerate() {
                    for (bn, br) in cells.iter().skip(i + 1) {
                        // The readout sits directly over the dial by design;
                        // they are one column and only the dial is a target.
                        if [*an, *bn].contains(&"readout") && [*an, *bn].contains(&"dial") {
                            continue;
                        }
                        if !ar.is_positive() || !br.is_positive() {
                            continue;
                        }
                        assert!(
                            !ar.intersects(*br),
                            "{an} and {bn} overlap at {w}x{h}: {ar:?} / {br:?}"
                        );
                    }
                }
            }
        }
    }

    /// A row too small to carry switches degrades to the name alone, rather
    /// than to a row of targets too small to hit.
    #[test]
    fn a_cramped_automation_row_keeps_its_name_and_drops_its_switches() {
        let tiny = Rect::from_min_size(Pos2::ZERO, Vec2::new(80.0, 9.0));
        let a = auto_header(tiny);
        assert!(a.name.is_positive(), "the name went with the switches");
        for (n, r) in [("close", a.close), ("arm", a.arm), ("bypass", a.bypass), ("dial", a.dial)] {
            assert!(!r.is_positive(), "{n} survived a row too small for it");
        }
    }
    use super::*;

    fn a_lane(used: bool) -> LaneView<'static> {
LaneView {
    to_theory: false,
    is_folder: false,
    in_folder: false,
    folder_shut: false,
    folder_count: 0,
    hidden: false,
    folder_silent: false,
            automation: Vec::new(),
            route_count: 0,
            stereo: true,
            inserts: [""; crate::recorder::INSERTS * 2],
            name: "KEYS",
            input: String::new(),
            color: 0,
            kind: used.then_some(ChannelKind::Midi),
            used,
            armed: false,
            muted: false,
    phase: false,
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
                wave_from_s: 0.0,
                wave_span_s: 0.0,
                selected: false,
                midi: false,
                loop_s: 0.0,
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
                wave_from_s: 0.0,
                wave_span_s: 0.0,
                selected: true,
                midi: false,
                loop_s: 0.0,
                notes: &[],
                offset_s: 0.0,
                recording: false,
            },
        ]
    }

    pub(super) fn a_view<'a>(clips: &'a [ClipView<'a>]) -> ArrangeView<'a> {
ArrangeView {
            master_gain: 1.0,
            master_peak: [0.0; 2],
            master_color: 0,
            master_knob: (0xE8, 0x3A, 0x4E),
            accent: (0x6C, 0x9B, 0xD2),
            show_master_row: true,
            master_inserts: [""; crate::recorder::INSERTS * 2],
            show_tempo_lane: false,
            tempo_hot: None,
            meter_edit: None,
            fx_strip: (0xE3, 0xD9, 0xBD),
            meter_bg: (0x0B, 0x0E, 0x0A),
            header_w: None,
            naming: None,
            time_sel: None,
            selected_channel: None,
            selected_tracks: &[],
            lanes: std::array::from_fn(|i| a_lane(i < 3)),
            lane_scroll: 0,
            visible_lanes: VISIBLE_LANES,
            active: CHANNELS,
            clips,
            metronome_on: true,
            snap_on: true,
            grip_lit: false,
            drop_lane: None,
            master_automation: Vec::new(),
            grid_exp: 0,
            pending: None,
            view_start_s: 0.0,
            view_len_s: 120.0,
            song_s: 120.0,
            playhead_s: 15.0,
            tempo: crate::tempo::TempoMap::constant(120.0),
            meter: crate::tempo::MeterMap::constant(crate::recorder::TimeSignature {
                beats: 4,
                unit: 4,
            }),
            dark_mode: true,
            wood: (0x4A, 0x3B, 0x2C),
        }
    }

    pub(super) fn rect() -> Rect {
        Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 420.0))
    }

    /// **A node being dragged says what it is worth.**
    ///
    /// The one editable thing on this timeline whose value could not be read
    /// while it was being set: the height of a dot in a half-height row is
    /// not a number anybody can name. Asserted the same way the header
    /// controls are — by running the real `draw` and looking for text that
    /// reaches the row — because "it is in the code" was exactly the claim
    /// that turned out to be false for the header.
    #[test]
    fn the_node_being_dragged_shows_its_value_and_hovering_one_does_not() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let pts = vec![crate::automation::Point {
            at_s: 8.0,
            value: 0.75,
            shape: crate::automation::Shape::Linear,
        }];
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 673.0));

        let text_in = |moving: Option<usize>, hot: Option<usize>| -> Vec<String> {
            let mut v = a_view(&[]);
            v.visible_lanes = 8;
            v.lanes[0].automation = vec![AutoView {
                selected: false,
                label: "volume".to_owned(),
                resting: 0.5,
                armed: false,
                bypassed: false,
                param: crate::automation::AutoParam::Gain,
                points: &pts,
                hot,
                chosen: Vec::new(),
                moving,
            }];
            let l = Layout::new(r, &v);
            let row = l
                .rows
                .iter()
                .find(|x| x.kind == RowKind::Automation(0, 0))
                .expect("no automation row")
                .lane;
            let out = ctx.run(
                egui::RawInput { screen_rect: Some(r), ..Default::default() },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_clip_rect(r);
                        draw(ui.painter(), r, &v);
                    });
                },
            );
            out.shapes
                .iter()
                .filter_map(|cs| match &cs.shape {
                    egui::Shape::Text(t) if row.intersects(t.visual_bounding_rect()) => {
                        Some(t.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect()
        };

        // Dragging it: the value, as a percentage, over the row.
        let while_moving = text_in(Some(0), Some(0));
        assert!(
            while_moving.iter().any(|t| t == "75%"),
            "the dragged node showed no value: {while_moving:?}"
        );
        // Merely hovering it: nothing. A value you are passing over is not
        // worth a number, and one that appeared on hover would flicker
        // across every row somebody crossed.
        let while_hovering = text_in(None, Some(0));
        assert!(
            !while_hovering.iter().any(|t| t == "75%"),
            "hovering drew the readout too: {while_hovering:?}"
        );
    }

    /// **Every control on an automation row is actually PAINTED    /// **Every control on an automation row is actually PAINTED, at every
    /// height a row can have.**
    ///
    /// The regression this exists for: `auto_header` refused to lay out any
    /// cell below twelve points of row height, and an automation row is half
    /// a track of an arrangement that gets whatever the recorder band leaves.
    /// Switch the piano, the fretboard and the theory band off — which is
    /// what somebody working in the arrangement does, to get the room — and
    /// the stack is 150 points, a track is 15 and a row is SEVEN. The ×, the
    /// arm, the bypass and the dial all vanished at once, leaving one dim
    /// word on a plate, and nothing failed: the layout said `NOTHING`, the
    /// paint skipped what the layout omitted, and the hit test agreed with
    /// both. A cell can be laid out, hit-tested and never drawn.
    ///
    /// So this does not ask the layout what it INTENDS. It runs the real
    /// `draw` and looks at the shapes that came out, which is the only
    /// question that was ever in doubt.
    /// **The master's field answers a press with the time under it.**
    ///
    /// It is a bus, so it holds no clips, and its whole width across the
    /// timeline was the one horizontal strip here that answered nothing at
    /// all. The owner asked for it to move the playhead, which is what every
    /// other empty surface on this timeline already does.
    #[test]
    fn the_master_strip_reads_a_time_across_its_width() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&clips);
        v.visible_lanes = 8;
        v.show_master_row = true;
        v.view_start_s = 0.0;
        v.view_len_s = 120.0;
        let r = rect();
        let l = Layout::new(r, &v);
        let row = l
            .rows
            .iter()
            .find(|x| x.kind == RowKind::Master)
            .expect("no master row");
        assert!(row.lane.is_positive(), "the master row has no field");

        // Across the width, the time rises and matches the ruler's own
        // mapping — the same `time_at` a lane press uses, so the magnet
        // means the same thing wherever the hand lands.
        let mut last = f64::NEG_INFINITY;
        for frac in [0.05f32, 0.3, 0.5, 0.9] {
            let x = row.lane.left() + row.lane.width() * frac;
            let at = Pos2::new(x, row.lane.center().y);
            match hit_test(r, &v, at) {
                Some(Hit::MasterLane(t)) => {
                    let want = time_at(r, &v, x);
                    assert!((t - want).abs() < 1e-6, "read {t}, ruler says {want}");
                    assert!(t > last, "time did not rise across the strip");
                    last = t;
                }
                other => panic!("the master field answered {other:?} at {frac}"),
            }
        }

        // Its own controls still win over the field: the fader is on that
        // row and pressing it must not move the playhead.
        assert_eq!(
            hit_test(r, &v, l.master_row.knob.center()),
            Some(Hit::BusGain(MASTER_COLUMN)),
            "the master fader was swallowed by the field"
        );

        // And with the row switched off there is nothing there to press.
        v.show_master_row = false;
        let l2 = Layout::new(r, &v);
        assert!(
            !l2.rows.iter().any(|x| x.kind == RowKind::Master),
            "the master row survived being switched off"
        );
    }

    #[test]
    fn every_control_is_drawn_at_every_height_a_row_can_have() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let pts: Vec<crate::automation::Point> = Vec::new();
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        v.lanes[0].automation = vec![AutoView {
            selected: false,
            label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: crate::automation::AutoParam::Gain,
            points: &pts,
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        // 673 is the arrangement with every band on; 150 is the arrangement
        // with them off, which is the case that was broken. The rest are the
        // sizes in between that a dragged window passes through.
        for hh in [673.0f32, 400.0, 300.0, 220.0, 180.0, 150.0] {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, hh));
            let l = Layout::new(r, &v);
            let row = l
                .rows
                .iter()
                .find(|x| x.kind == RowKind::Automation(0, 0))
                .unwrap_or_else(|| panic!("no automation row at {hh}"));
            let cells = auto_header(row.header);
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(r),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_clip_rect(r);
                        draw(ui.painter(), r, &v);
                    });
                },
            );
            // **The last thing that COVERS the whole header**, by emission
            // order. Anything drawn before it is buried.
            //
            // This is the assertion the first version of this test was
            // missing. It counted the shapes that reached the header and was
            // satisfied, while every one of them was painted hundreds of
            // lines before the header column's `hdr_wood` and covered by it.
            // The controls were drawn, and invisible, and still hittable —
            // so nothing in the code or the hit test could notice.
            let buried_up_to = out
                .shapes
                .iter()
                .rposition(|cs| {
                    let b = cs.shape.visual_bounding_rect();
                    b.is_positive() && b.contains_rect(row.header)
                })
                .map_or(0, |i| i + 1);
            // A control counts only if it is emitted AFTER that, and only if
            // it is smaller than the header — a backdrop proves nothing.
            let painted = |cell: Rect| {
                cell.is_positive()
                    && out.shapes.iter().skip(buried_up_to).any(|cs| {
                        let b = cs.shape.visual_bounding_rect();
                        b.is_positive()
                            && b.width() < row.header.width() * 0.9
                            && cell.intersects(b)
                    })
            };
            for (what, cell) in [
                ("the close X", cells.close),
                ("the record arm", cells.arm),
                ("the bypass", cells.bypass),
                ("the name", cells.name),
                ("the dial", cells.dial),
            ] {
                assert!(
                    painted(cell),
                    "{what} is not VISIBLE on a {:.1}pt row (arrangement {hh}pt): \
                     either it was never drawn, or something painted over it",
                    row.header.height()
                );
            }
            // **The dial stays inside its own row.** It used to be sized from
            // the column's WIDTH into a slot defined by the height, so on a
            // tall row it painted a 34-point knob into a 20-point space and
            // hung into the row below.
            assert!(
                row.header.contains_rect(cells.dial),
                "the dial {:?} hangs out of its row {:?}",
                cells.dial,
                row.header
            );
        }
    }

    /// The time mapping round-trips, which is what keeps a clip drawn where
    /// it is grabbed.


    /// **The MASTER heads the track list, at a full track's height, in the
    /// mixer's own colours — and `Cmd`/`Ctrl+M` hides it.**
    ///
    /// The desk draws a strip for it and the timeline drew none, so a
    /// session's output level was reachable from one view and not the other.
    /// The CLICK had a row here too and lost it: the metronome is not a
    /// thing you arrange against.
    #[test]
    fn the_master_heads_the_track_list_and_can_be_hidden() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        v.show_tempo_lane = true;
        // One automation row on lane 0. A lane shows as many as are ticked
        // now, so the view has to say so rather than a flag.
        v.lanes[0].automation = vec![AutoView {
            param: crate::automation::AutoParam::Gain,
            label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            selected: false,
            points: &[],
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let l = Layout::new(rect(), &v);

        let masters: Vec<&Row> = l
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Master)
            .collect();
        assert_eq!(masters.len(), 1, "the master's row is not laid out once");
        // Above every track, and below the tempo lane, which is a property
        // of the song rather than a channel.
        let first_track = l
            .rows
            .iter()
            .find(|r| matches!(r.kind, RowKind::Track(_)))
            .expect("no tracks");
        let tempo = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Tempo)
            .expect("no tempo lane");
        assert!(tempo.lane.bottom() <= masters[0].lane.top() + 0.5);
        assert!(masters[0].lane.bottom() <= first_track.lane.top() + 0.5);

        // **HALF a track's height**, at the owner's word. It went from half
        // (beside the click) to full when it became a track in its own right,
        // and back to half once it was clear what it actually carries: a name,
        // a level and a rack, and no clips at all. Every point it gives up is
        // a point the tracks get back.
        let want = first_track.lane.height() * MASTER_ROW_SHARE;
        assert!(
            (masters[0].lane.height() - want).abs() < 0.6,
            "the master's row is {}pt against a {}pt track, wanted {want}pt",
            masters[0].lane.height(),
            first_track.lane.height()
        );

        // Its level is the DESK's own column, so the two views cannot
        // disagree about how loud the master is.
        let knob = l.master_row.knob;
        assert!(knob.is_positive(), "the master's row has no level");
        assert_eq!(
            hit_test(rect(), &v, knob.center()),
            Some(Hit::BusGain(MASTER_COLUMN)),
            "the master's level cannot be pressed"
        );
        assert_eq!(MASTER_COLUMN, crate::recorder::CHANNELS + 1);

        // **Hidden, and the tracks take the room back.** Not removed: a
        // session always has an output.
        let mut off = a_view(&cs);
        off.show_tempo_lane = true;
        off.show_master_row = false;
        let l2 = Layout::new(rect(), &off);
        assert!(
            !l2.rows.iter().any(|r| r.kind == RowKind::Master),
            "the master's row survived being hidden"
        );
        assert!(!l2.master_row.knob.is_positive());
        let hidden_track = l2
            .rows
            .iter()
            .find(|r| matches!(r.kind, RowKind::Track(_)))
            .expect("no tracks with the master hidden");
        assert!(
            hidden_track.lane.height() > first_track.lane.height(),
            "hiding the master gave the tracks no room back"
        );
    }

    /// **Every enumerated target IS the hit it claims.**
    ///
    /// `targets` is a second walk of the same layout `hit_test` walks, and a
    /// second walk is a second thing to get out of step. This presses each
    /// rect's own centre and asks the panel what it means, so a cell that
    /// moves in one and not the other fails here rather than as a screen
    /// reader announcing the wrong control.
    #[test]
    fn every_target_is_the_hit_it_claims() {
        let cs = clips();
        let mut v = a_view(&cs);
        v.show_tempo_lane = true;
        let mut bad: Vec<String> = Vec::new();
        for (r, want) in targets(rect(), &v) {
            let got = hit_test(rect(), &v, r.center());
            let agrees = match (want, got) {
                // The lane hits carry a computed second, so they compare by
                // their variant and their lane rather than by value.
                (a, Some(b)) => a == b,
                (_, None) => false,
            };
            if !agrees {
                bad.push(format!("{want:?} at {r:?} answers {got:?}"));
            }
        }
        assert!(bad.is_empty(), "targets disagrees with hit_test:\n  {}", bad.join("\n  "));
    }

    /// Nothing is enumerated twice, and nothing shares a rect: the map from
    /// a focused node back to its hit is BY RECT, so two controls at one
    /// rectangle would hand the keyboard whichever came first.
    #[test]
    fn no_two_targets_share_a_rectangle() {
        let cs = clips();
        let mut v = a_view(&cs);
        v.show_tempo_lane = true;
        let all = targets(rect(), &v);
        for (i, (a, ha)) in all.iter().enumerate() {
            for (b, hb) in all.iter().skip(i + 1) {
                assert!(
                    a != b,
                    "{ha:?} and {hb:?} are both at {a:?} — the map back is by rect"
                );
            }
        }
    }

    /// **Every node can be reached, says what it is, and reads in words.**
    ///
    /// The whole point of the module: a label a reader announces as nothing,
    /// or a value that is a raw fraction, is a control that has been made
    /// operable and not accessible.
    #[test]
    fn every_node_in_the_timeline_says_what_it_is() {
        let cs = clips();
        let mut v = a_view(&cs);
        v.show_tempo_lane = true;
        // **A window tall enough to hold the whole header.** A short row
        // sheds the insert bays and then the routing button — deliberately,
        // and there are tests for it — so a fixture that sheds them cannot
        // tell "this node is missing because the panel dropped the cell"
        // from "this node is missing because the walk is broken".
        v.visible_lanes = 4;
        let tall = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 900.0));
        let nodes = a11y_nodes(tall, &v);
        // **A floor, not merely non-empty.** The first cut of this test
        // passed with the loop nearly empty; a walk that finds three things
        // is a walk that is broken and silent about it. Three used lanes,
        // thirteen cells each, plus the toolbar, the tempo header and two
        // clips.
        assert!(
            nodes.len() >= 30,
            "the timeline offers only {} tab stops: {:?}",
            nodes.len(),
            nodes.iter().map(|n| n.label.clone()).collect::<Vec<_>>()
        );
        for want in [
            "Metronome",
            "Snap to grid",
            "Grid",
            "Song tempo, flatten to one tempo",
            "KEYS volume",
            "KEYS routing",
            // **Not the insert BAYS.** At every header width this app
            // actually opens at, the name column is under the 54 points
            // three bays need and they are dropped — the T switch was added
            // between the arm and the column without `dials_fit` learning
            // about it, so the guard still measures from the arm dot and
            // passes on a column 32 points wide. Written down here rather
            // than asserted, because asserting it would be asserting the
            // bug.
            "KEYS input",
            "Clip backing",
        ] {
            assert!(
                nodes.iter().any(|n| n.label == want),
                "no node labelled {want}"
            );
        }
        let mut ids = std::collections::HashSet::new();
        for n in &nodes {
            assert!(!n.label.trim().is_empty(), "a node with no label: {n:?}");
            assert!(
                n.rect.is_positive(),
                "{} has no rectangle to focus",
                n.label
            );
            assert!(
                tall.contains_rect(n.rect),
                "{} is outside the panel at {:?}",
                n.label,
                n.rect
            );
            assert!(
                ids.insert(n.id),
                "two nodes share the id behind {} — focus would jump",
                n.label
            );
            // No raw fraction anywhere: a value a person has to decode has
            // not been made accessible, only made noisy.
            if let Some(val) = &n.value {
                assert!(!val.trim().is_empty(), "{} reads as nothing", n.label);
                assert!(
                    val.parse::<f32>().is_err(),
                    "{} reads the bare number {val} instead of words",
                    n.label
                );
            }
            if let crate::a11y::Kind::Slider { .. } = n.kind {
                assert!(n.travel.is_some(), "{} cannot be nudged", n.label);
            }
        }
    }

    /// Every dial's travel is its POSITION, not its amplitude — the mistake
    /// that once sent one arrow press on the master from -6 dB to -20.4.
    #[test]
    fn a_headers_dials_travel_where_the_dial_stands() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        // A gain well below unity, where amplitude and fader position are
        // furthest apart.
        v.lanes[0].gain = 0.25;
        v.lanes[0].send = 0.4;
        v.lanes[0].pan = -0.5;
        let nodes = a11y_nodes(rect(), &v);
        let of = |label: &str| {
            nodes
                .iter()
                .find(|n| n.label == label)
                .unwrap_or_else(|| panic!("no node labelled {label}"))
                .clone()
        };
        let vol = of("KEYS volume");
        let want = crate::recorder::gain_to_fader(0.25);
        assert!(
            (vol.travel.unwrap().norm - want).abs() < 1e-6,
            "the volume dial travels on amplitude, not position"
        );
        assert_eq!(vol.value.as_deref(), Some(crate::recorder::gain_text(0.25).as_str()));
        assert!((of("KEYS send").travel.unwrap().norm - 0.4).abs() < 1e-6);
        // -1..1 laid onto the 0..1 the keyboard moves.
        assert!((of("KEYS pan").travel.unwrap().norm - 0.25).abs() < 1e-6);
        assert_eq!(of("KEYS pan").value.as_deref(), Some("left 50"));
    }

    /// A clip reads as WHERE IT IS, in bars — never in seconds.
    #[test]
    fn a_clip_reads_as_bars_and_says_whether_it_is_selected() {
        let cs = clips();
        let v = a_view(&cs);
        let nodes = a11y_nodes(rect(), &v);
        let backing = nodes
            .iter()
            .find(|n| n.label == "Clip backing")
            .expect("the first clip has no node");
        let said = backing.spoken();
        assert!(said.contains("bar "), "a clip reads as {said}");
        assert!(!said.contains("selected"), "an unselected clip claims it is");
        let solo = nodes
            .iter()
            .find(|n| n.label == "Clip solo")
            .expect("the second clip has no node");
        assert!(solo.spoken().ends_with("selected"), "{}", solo.spoken());
    }

    /// The switches read their own state rather than a fixed one — the bug
    /// that a toggle built from the layout and not the view would have.
    #[test]
    fn the_header_switches_read_the_track() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        v.lanes[0].muted = true;
        v.lanes[0].soloed = false;
        v.lanes[0].armed = true;
        v.lanes[0].to_theory = true;
        let nodes = a11y_nodes(rect(), &v);
        let said = |label: &str| {
            nodes
                .iter()
                .find(|n| n.label == label)
                .map(crate::a11y::Node::spoken)
                .unwrap_or_else(|| panic!("no node labelled {label}"))
        };
        assert_eq!(said("KEYS mute"), "KEYS mute, on");
        assert_eq!(said("KEYS solo"), "KEYS solo, off");
        assert_eq!(said("KEYS record arm"), "KEYS record arm, on");
        assert_eq!(
            said("KEYS sends its MIDI to the theory view"),
            "KEYS sends its MIDI to the theory view, on"
        );
    }

    /// An UNUSED row offers its two controls and none of a track's.
    #[test]
    fn an_empty_row_offers_only_the_way_to_fill_it() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let v = a_view(&cs);
        let nodes = a11y_nodes(rect(), &v);
        // Lane 3 upward is unused in the fixture.
        let has = |s: &str| nodes.iter().any(|n| n.label == s);
        assert!(has("Add a track at row 4"), "an empty row cannot be filled");
        assert!(
            !nodes.iter().any(|n| n.label == "Track 4 volume"),
            "an empty row offers a dial it does not draw"
        );
    }

    /// **The master's automation rows say "Master", not "Track 17".** The
    /// bus is addressed as lane `MASTER_LANE`, one past the lanes array, so
    /// every label site that indexed `lanes` or printed `i + 1` directly
    /// misnamed it to a reader — and the param row went further, reading a
    /// bound parameter as "none" because the inline lookup found no lane
    /// there. Sighted users saw a correctly drawn row the whole time; this
    /// is the label a reader actually hears.
    #[test]
    fn the_master_automation_row_reads_as_master_not_as_a_seventeenth_track() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        v.master_automation = vec![AutoView {
            selected: false,
            label: "Master gain".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: crate::automation::AutoParam::Gain,
            points: &[],
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let nodes = a11y_nodes(rect(), &v);
        let has = |s: &str| nodes.iter().any(|n| n.label == s);
        // The param picker names the bus AND the parameter it really holds.
        assert!(
            has("Master automation, Master gain"),
            "the master's param row is missing or misnamed; automation rows: {:?}",
            nodes
                .iter()
                .filter(|n| n.label.contains("utomation"))
                .map(|n| n.label.as_str())
                .collect::<Vec<_>>()
        );
        // The close button and the dial follow the same name.
        assert!(has("Hide automation row 1 on Master"), "the close row misnames the bus");
        assert!(has("Automation lane 1 on Master"), "the dial row misnames the bus");
        // And nothing anywhere in the tree calls the bus a track.
        let ghost = format!("Track {}", MASTER_LANE + 1);
        assert!(
            !nodes.iter().any(|n| n.label.contains(&ghost)),
            "a node still calls the master {ghost}"
        );
    }

    /// A row scrolled out of the window is not a tab stop: every node's
    /// rect is inside the lanes the panel actually drew.
    #[test]
    fn scrolled_away_rows_are_not_reachable() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        v.visible_lanes = 2;
        let nodes = a11y_nodes(rect(), &v);
        assert!(
            !nodes.iter().any(|n| n.label.starts_with("Track 8")),
            "a row below the window is still a tab stop"
        );
    }

    /// The grid box is a slider so both arrows reach it, and it reads the
    /// grid the box PRINTS.
    #[test]
    fn the_grid_box_can_be_stepped_both_ways() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&cs);
        v.grid_exp = -2;
        let nodes = a11y_nodes(rect(), &v);
        let g = nodes.iter().find(|n| n.label == "Grid").expect("no grid node");
        // **The box, the spoken form and the magnet all name ONE division.**
        // They used to give three different answers — see `grid_label`.
        // exp -2 in 4/4 is the quarter note: bar, half, quarter.
        assert_eq!(g.value.as_deref(), Some("quarter note"));
        assert_eq!(grid_label(-2, 4, 4), "\u{266a}");
        assert!((grid_quarters(-2, 4, 4) - 1.0).abs() < 1e-9);
        // 6/8 cannot halve its bar twice without landing between beats, so
        // the second rung is the beat itself: an eighth.
        assert_eq!(grid_label(-2, 6, 8), "1/8");
        assert!((grid_quarters(-2, 6, 8) - 0.5).abs() < 1e-9);
        let t = g.travel.expect("the grid box cannot be nudged");
        assert!((t.norm - 3.0 / 8.0).abs() < 1e-6, "{}", t.norm);
        // Both ends are reachable, and neither runs off the rails.
        assert!(crate::a11y::Gesture::Least.nudged(t.norm) <= 0.0);
        assert!(crate::a11y::Gesture::Most.nudged(t.norm) >= 1.0);
    }

    /// **Every theme, in both modes, measured where the paint actually
    /// lands.**
    ///
    /// This view was two hard-coded wardrobes that no theme could reach, and
    /// what keeps the derivation honest across all sixteen is measuring each
    /// pairing against the surface it is really drawn on — the header text on
    /// the strip face, a lane title on that lane's own plate, the automation
    /// dial on the header rather than on the field below it. Every entry
    /// here is a pairing that was got wrong once.
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
                v.master_knob = (
                    set.master_knob_color.r,
                    set.master_knob_color.g,
                    set.master_knob_color.b,
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
                // **An automation row's ground, on every theme.** It is a
                // blend of two themed roles, which is precisely the kind of
                // derived colour that lands in the dead zone where neither
                // ink clears — and a value readout is written on it now.
                //
                // The second check is not a reading threshold: nothing is
                // read across that edge. 1.12:1 is the point at which the
                // edge can be SEEN, which is the whole job — a row that
                // matches its parent stripe is a third stripe in the
                // alternation rather than a row about something else.
                for lane in [0usize, 1] {
                    let g = auto_ground(&p, lane);
                    let parent = if lane % 2 == 0 { p.lane } else { p.lane_alt };
                    check("automation readout", on_ink(g), g, 4.5);
                    check("automation ground against its lane", g, parent, 1.12);
                    // **The seam between two stacked rows has to be SEEN**,
                    // or three open lanes read as one tall band. Structure,
                    // not text: the same "there is an edge here" bar.
                    check("automation seam", auto_seam(&p, lane), g, 1.12);
                }
                // The row's dial stands on its HEADER, not on the lane
                // ground — checking it against the ground was measuring a
                // pairing the app never draws, which is the same mistake
                // this audit was written to catch elsewhere.
                {
                    let hface = shade(p.hdr_face, 0.92);
                    check("automation header seam", auto_header_seam(&p), hface, 1.12);
                    // **The automation dial, in the theme's own accent, measured
                // against the header face it is DRAWN on** — not against the
                // lane field below it, which is the mistake this audit
                // exists to catch and made once already.
                let cap = auto_dial_body(&p, (
                    set.white_key_active_color.r,
                    set.white_key_active_color.g,
                    set.white_key_active_color.b,
                ));
                check("automation dial", cap, hface, 1.35);
                // And the needle drawn ACROSS it has to read on it.
                check("automation needle", p.clip_ink, cap, 1.9);
                }
                // **The MASTER's dial, per theme.** Its body is the theme's
                // own master red and its outline and needle are fixed, so
                // the pairing is checked on all sixteen rather than assumed
                // from the one it was drawn against.
                {
                    let body = master_dial_body(v.master_knob);
                    check("master dial needle", DIAL_MASTER_NEEDLE, body, 2.2);
                    check("master dial outline", DIAL_MASTER_RING, body, 1.6);
                    let plate = crate::recorder_panel::legible_surface(shade(body, 0.55));
                    check("master row text", on_ink(plate), plate, 4.5);
                }
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

    /// **Any swatch a hand can paint a lane carries measured ink here.**
    ///
    /// The header plate and the clip body both wear `STRIP_COLORS` raw when a
    /// lane is painted, and the hand-picking grid holds three third tones in
    /// the dead zone where neither ink clears. Both surfaces now go through
    /// `legible_surface` and then MEASURE their ink, so this walks the whole
    /// table through exactly that pipeline. Raw, indices 11, 14 and 17 fail.
    #[test]
    fn a_painted_lane_reads_whatever_swatch_the_hand_picked() {
        for (i, &(r, g, b)) in STRIP_COLORS.iter().enumerate() {
            let surface =
                crate::recorder_panel::legible_surface(Color32::from_rgb(r, g, b));
            let ink = on_ink(surface);
            let c = crate::recorder_panel::contrast_ratio(ink, surface);
            assert!(
                c >= 4.5,
                "swatch {i} ({r:02x}{g:02x}{b:02x}): plate and clip ink at {c:.2}:1"
            );
        }
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

    /// **The drawing and the hit test agree about where a point is.**
    ///
    /// The classic pair that drifts: one gains an inset and the other does
    /// not, and you get a curve you can see and cannot grab — worse at the
    /// top and bottom, which is exactly where the interesting values are. So
    /// both go through `auto_y_of` / `auto_value_at`, and this walks the
    /// whole band demanding they invert.
    #[test]
    fn a_point_is_grabbed_where_it_is_drawn() {
        let row = Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(400.0, 160.0));
        for n in 0..=20 {
            let v = f64::from(n) / 20.0;
            let y = auto_y_of(row, v);
            let back = auto_value_at(row, y);
            assert!(
                (back - v).abs() < 1e-3,
                "{v} drew at {y} and read back as {back}"
            );
            // ...and inside the row, so the handle is whole.
            assert!(
                y >= row.top() && y <= row.bottom(),
                "{v} drew at {y}, outside {row:?}"
            );
        }
        // Up is MORE. A lane that ran the other way would be the one control
        // in the app that did.
        assert!(auto_y_of(row, 1.0) < auto_y_of(row, 0.0), "the lane is upside down");
        // Out-of-range values are clamped rather than drawn off the row.
        assert!((auto_y_of(row, 9.0) - auto_y_of(row, 1.0)).abs() < 1e-6);
        assert!((auto_value_at(row, row.top() - 50.0) - 1.0).abs() < 1e-6);
    }

    /// **A row that is not a rectangle answers finitely.**
    ///
    /// `Rect::NOTHING` in egui is `[+inf, +inf]..[-inf, -inf]`, so its height
    /// is `-inf`, the inset computed from it is `-inf`, and `inf + -inf` is
    /// NaN. A NaN reaching the painter is a shape that silently does not
    /// draw; a NaN in a hit test is a comparison that is false against
    /// everything. Both look exactly like nothing happening.
    #[test]
    fn a_degenerate_row_never_produces_a_nan() {
        let bad = [
            Rect::NOTHING,
            Rect::ZERO,
            Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(10.0, 10.0)),
            // Inverted, which is what a layout produces when it runs out of
            // room and something subtracts past zero.
            Rect::from_min_max(Pos2::new(10.0, 60.0), Pos2::new(200.0, 10.0)),
            Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(f32::NAN, 40.0)),
        ];
        for r in bad {
            for v in [0.0_f64, 0.5, 1.0, f64::NAN, f64::INFINITY, -3.0] {
                let y = auto_y_of(r, v);
                assert!(y.is_finite(), "{r:?} at {v} drew at {y}");
            }
            for y in [0.0_f32, 42.0, f32::NAN, f32::INFINITY, -1e30] {
                let got = auto_value_at(r, y);
                assert!(got.is_finite(), "{r:?} at y={y} read {got}");
                assert!((0.0..=1.0).contains(&got), "{r:?} at y={y} read {got}");
            }
        }
        // A row TOO SHORT for two insets does not invert its own band — every
        // value would otherwise read as the same one.
        let thin = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(100.0, 3.0));
        assert!(
            auto_y_of(thin, 1.0) <= auto_y_of(thin, 0.0),
            "a thin row runs backwards"
        );
    }

    /// **The T switch reads on every theme, lit and unlit.**
    ///
    /// The owner asked for this one by name. It is a two-state control the
    /// size of a fingernail carrying a single letter, which is the shape
    /// every ink failure this app has had took — and it wears the theme's
    /// own selection colour rather than a fixed one, so there are sixteen
    /// grounds to be legible on and not one.
    #[test]
    fn every_theme_dresses_the_theory_switch_legibly() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.arr_dark = dark;
                let mut v = a_view(&[]);
                v.dark_mode = dark;
                v.wood = {
                    let c = set.recorder_bg_color;
                    (c.r, c.g, c.b)
                };
                v.fx_strip = {
                    let c = set.fx_strip_color;
                    (c.r, c.g, c.b)
                };
                v.meter_bg = {
                    let c = set.meter_bg_color;
                    (c.r, c.g, c.b)
                };
                let p = ink(&v);
                let mode = if dark { "dark" } else { "light" };
                let mut check = |what: &str, fg: Color32, bg: Color32| {
                    let r = crate::recorder_panel::contrast_ratio(fg, bg);
                    if r < 4.5 {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // Lit and unlit both, which is the whole of what the switch
                // can look like. `on_ink` measures the letter against the
                // ground it lands on rather than inheriting it, which is the
                // rule this app has learned eight times.
                check("T lit", on_ink(p.select), p.select);
                check(
                    "T at rest",
                    quiet(on_ink(p.hdr_lane), p.hdr_lane, 0.18),
                    p.hdr_lane,
                );
                // And the lit switch has to be tellable from the two beside
                // it — a T that looks like an M is a track somebody thinks
                // they muted.
                for (name, other) in [("the mute", p.mute), ("the solo", p.solo)] {
                    let d = crate::recorder_panel::delta_e(p.select, other);
                    if d < 25.0 {
                        bad.push(format!(
                            "{} {mode}: the T and {name} are {d:.1} apart",
                            theme.name
                        ));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "{} unreadable: {bad:#?}", bad.len());
    }

    /// The tempo lane's scale is monotonic, logarithmic, and reversible.
    #[test]
    fn the_tempo_lane_reads_both_ways() {
        let row = Rect::from_min_max(Pos2::new(0.0, 100.0), Pos2::new(400.0, 160.0));
        let (lo, hi) = TEMPO_LANE_BPM;
        assert!(tempo_y_of(row, hi) < tempo_y_of(row, lo), "faster is not up");
        for bpm in [lo, 60.0, 90.0, 120.0, 174.0, hi] {
            let back = tempo_bpm_at(row, tempo_y_of(row, bpm));
            assert!(
                (back - bpm).abs() / bpm < 0.01,
                "{bpm} came back as {back}"
            );
        }
        // **Logarithmic**: the middle of the travel is the GEOMETRIC mean of
        // the ends, not their average. A linear lane would put the whole
        // useful range of tempos in its bottom third.
        let mid = tempo_bpm_at(row, (tempo_y_of(row, lo) + tempo_y_of(row, hi)) * 0.5);
        assert!(
            (mid - (lo * hi).sqrt()).abs() < 1.0,
            "halfway up the lane is {mid}, not {}",
            (lo * hi).sqrt()
        );
        // Out of range clamps rather than running off the row.
        assert!(tempo_y_of(row, 1.0) <= row.bottom());
        assert!(tempo_y_of(row, 10_000.0) >= row.top());
        assert!(tempo_y_of(row, f64::NAN).is_finite());
        assert!(tempo_bpm_at(row, f32::NAN).is_finite());
        // A degenerate row answers a number rather than a NaN — the guard
        // `auto_band` exists for.
        assert!(tempo_y_of(Rect::NOTHING, 120.0).is_finite());
        assert!(tempo_bpm_at(Rect::NOTHING, 10.0).is_finite());
    }

    /// **The tempo lane and the signature chips draw at every zoom**, with a
    /// map in them, without asking the painter for a coordinate it cannot
    /// use — and a press anywhere answers something sane.
    ///
    /// The ruler now WALKS bars rather than stepping seconds, so a
    /// pathological map is a loop bound rather than an arithmetic slip: this
    /// is what says a one-beat bar at a ten-million-second zoom finishes.
    #[test]
    fn the_tempo_lane_paints_at_every_size_and_zoom() {
        use crate::recorder::TimeSignature;
        use crate::tempo::{MeterMap, MeterPoint, TempoMap, TempoPoint};
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let tempo = TempoMap::sanitised(
            (0..40)
                .map(|i| TempoPoint {
                    at_beat: f64::from(i) * 7.0,
                    bpm: 40.0 + f64::from(i % 11) * 22.0,
                    ramp: i % 3 == 0,
                })
                .collect(),
        );
        let meter = MeterMap::sanitised(vec![
            MeterPoint { at_bar: 0, sig: TimeSignature { beats: 4, unit: 4 } },
            MeterPoint { at_bar: 5, sig: TimeSignature { beats: 7, unit: 8 } },
            // **A one-beat bar**, which is the shape that turns "walk the
            // bars" into an unbounded loop if nothing bounds it.
            MeterPoint { at_bar: 9, sig: TimeSignature { beats: 1, unit: 16 } },
            MeterPoint { at_bar: 40, sig: TimeSignature { beats: 3, unit: 4 } },
        ]);
        for size in [
            Vec2::new(1300.0, 420.0),
            Vec2::new(320.0, 120.0),
            Vec2::new(60.0, 40.0),
            Vec2::new(4000.0, 1400.0),
        ] {
            for (start, len) in [
                (0.0_f64, 30.0_f64),
                (0.0, 1e-4),
                (0.0, 1e7),
                (5_000.0, 10.0),
            ] {
                for exp in [-4i8, 0, 3] {
                    let mut v = a_view(&[]);
                    v.visible_lanes = 8;
                    v.view_start_s = start;
                    v.view_len_s = len;
                    v.show_tempo_lane = true;
                    v.grid_exp = exp;
                    v.tempo = tempo.clone();
                    v.meter = meter.clone();
                    v.tempo_hot = Some(2);
                    v.meter_edit = Some((5, "7/"));
                    let r = Rect::from_min_size(Pos2::ZERO, size);
                    ctx.run(Default::default(), |ctx| {
                        egui::Area::new("tempo-probe".into()).show(ctx, |ui| {
                            draw(ui.painter(), r, &v);
                        });
                    });
                    for at in [r.center(), r.min, r.max, Pos2::new(r.right(), r.top())] {
                        let _ = hit_test(r, &v, at);
                    }
                }
            }
        }
    }

    /// The curve draws at every zoom and every size, without panicking and
    /// without asking the painter for a coordinate it cannot use.
    #[test]
    fn the_automation_lane_paints_at_every_size_and_zoom() {
        use crate::automation::{AutoParam, Point, Shape};
        let ctx = egui::Context::default();
        // The panel writes bar numbers on the ruler, and a bare context has
        // no font bound to the family they ask for.
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let pts: Vec<Point> = (0..64)
            .map(|i| Point {
                at_s: f64::from(i) * 1.7,
                value: f64::from(i % 7) / 6.0,
                shape: if i % 3 == 0 { Shape::Hold } else { Shape::Linear },
            })
            .collect();
        for size in [
            Vec2::new(1300.0, 420.0),
            Vec2::new(320.0, 120.0),
            Vec2::new(60.0, 40.0),
            Vec2::new(4000.0, 1400.0),
        ] {
            for (start, len) in [
                (0.0_f64, 30.0_f64),
                (0.0, 1e-4),      // absurdly deep zoom
                (0.0, 1e7),       // the whole imaginable song
                (5_000.0, 10.0),  // long past the last point
            ] {
                let mut v = a_view(&[]);
                v.visible_lanes = 8;
                v.view_start_s = start;
                v.view_len_s = len;
                v.lanes[0].automation = vec![AutoView {
selected: false,
                                        label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
                    param: AutoParam::Gain,
                    points: &pts,
                    hot: Some(3),
                    chosen: Vec::new(),
                    moving: None,
                }];
                let r = Rect::from_min_size(Pos2::ZERO, size);
                // Painting is the assertion: a NaN or an inverted rect
                // panics in egui's tessellator rather than drawing wrong.
                ctx.run(Default::default(), |ctx| {
                    egui::Area::new("auto-probe".into()).show(ctx, |ui| {
                        draw(ui.painter(), r, &v);
                    });
                });
                // ...and a press anywhere in it answers something sane.
                for at in [r.center(), r.min, r.max, Pos2::new(r.right(), r.top())] {
                    let _ = hit_test(r, &v, at);
                }
            }
        }
    }

    /// **A long curve costs what is on screen, not what is in the song.**
    ///
    /// A thousand points on a lane would otherwise be a thousand segments a
    /// frame at every zoom, nearly all of them off-screen and every one of
    /// them recomputed sixty times a second.
    #[test]
    fn only_the_visible_part_of_a_curve_is_considered() {
        use crate::automation::{AutoParam, Point, Shape};
        let pts: Vec<Point> = (0..4_000)
            .map(|i| Point {
                at_s: f64::from(i),
                value: f64::from(i % 2),
                shape: Shape::Linear,
            })
            .collect();
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        v.view_start_s = 2_000.0;
        v.view_len_s = 4.0;
        v.lanes[0].automation = vec![AutoView {
selected: false,
                                        label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: AutoParam::Gain,
            points: &pts,
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let l = Layout::new(rect(), &v);
        let row = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(0, 0))
            .expect("no automation row")
            .lane;
        // **The point under the pointer is the RIGHT one**, four thousand
        // points in — which is what would break if the culling and the
        // indexing disagreed by the offset the search skipped.
        let want = 2_001usize;
        let at = Pos2::new(
            x_of(rect(), &v, pts[want].at_s),
            auto_y_of(row, pts[want].value),
        );
        assert_eq!(
            hit_test(rect(), &v, at),
            Some(Hit::AutoPoint(0, 0, want)),
            "the index came back offset by the cull"
        );
    }

    /// A press on a point grabs it; a press on empty room makes one.
    #[test]
    fn the_automation_row_answers_points_and_empty_room_differently() {
        use crate::automation::{AutoParam, Point, Shape};
        let pts = [
            Point { at_s: 2.0, value: 0.75, shape: Shape::Linear },
            Point { at_s: 6.0, value: 0.25, shape: Shape::Linear },
        ];
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        v.lanes[0].automation = vec![AutoView {
selected: false,
                                        label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: AutoParam::Gain,
            points: &pts,
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let l = Layout::new(rect(), &v);
        let row = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(0, 0))
            .expect("no automation row")
            .lane;
        assert!(row.is_positive());

        // Straight at the first point.
        let at = Pos2::new(x_of(rect(), &v, 2.0), auto_y_of(row, 0.75));
        assert_eq!(hit_test(rect(), &v, at), Some(Hit::AutoPoint(0, 0, 0)));
        // A little off it, still within the handle's reach.
        let near = Pos2::new(at.x + 2.0, at.y - 2.0);
        assert_eq!(hit_test(rect(), &v, near), Some(Hit::AutoPoint(0, 0, 0)));
        // Well away from both: empty room, and it says WHERE.
        let empty = Pos2::new(x_of(rect(), &v, 4.0), row.center().y);
        match hit_test(rect(), &v, empty) {
            Some(Hit::AutoLane(0, 0, t, val)) => {
                assert!((t - 4.0).abs() < 0.2, "the press landed at {t}s");
                assert!((val - 0.5).abs() < 0.1, "the middle of the row read as {val}");
            }
            other => panic!("empty room answered {other:?}"),
        }
        // The nearer of two points wins, not the first.
        let second = Pos2::new(x_of(rect(), &v, 6.0), auto_y_of(row, 0.25));
        assert_eq!(hit_test(rect(), &v, second), Some(Hit::AutoPoint(0, 0, 1)));
    }

    /// **A press just above an automation row does not make a point in it.**
    ///
    /// The grab area is the row expanded by a handle's radius, so an
    /// overhanging handle can be caught. That is right for GRABBING and
    /// wrong for PLACING: rows stack against each other, so those few pixels
    /// are visually the clip strip of the track above — and a press there
    /// made a point at full value and wrote it to disk at once, because
    /// `auto_value_at` clamps and the caller never learned it was out of
    /// band.
    #[test]
    fn a_press_above_an_automation_row_belongs_to_the_track_above() {
        use crate::automation::{AutoParam, Point, Shape};
        let pts = [Point { at_s: 2.0, value: 0.5, shape: Shape::Linear }];
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        v.lanes[0].automation = vec![AutoView {
selected: false,
                                        label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: AutoParam::Gain,
            points: &pts,
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let l = Layout::new(rect(), &v);
        let row = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(0, 0))
            .expect("no automation row")
            .lane;

        // Just above the row, and far from the one point so the handle
        // search cannot claim it.
        let above = Pos2::new(x_of(rect(), &v, 20.0), row.top() - 2.0);
        assert!(!row.contains(above), "the probe is inside the row after all");
        assert!(
            !matches!(hit_test(rect(), &v, above), Some(Hit::AutoLane(..))),
            "a press above the row made a point in it"
        );

        // ...and a press really INSIDE the row still works, or the fix has
        // made the lane unusable instead of safe.
        let inside = Pos2::new(x_of(rect(), &v, 20.0), row.center().y);
        assert!(
            matches!(hit_test(rect(), &v, inside), Some(Hit::AutoLane(0, 0, ..))),
            "the row stopped accepting presses"
        );
        // A handle overhanging the edge is still grabbable, which is the
        // whole reason the grab area is expanded.
        let handle = Pos2::new(x_of(rect(), &v, 2.0), auto_y_of(row, 0.5));
        assert_eq!(hit_test(rect(), &v, handle), Some(Hit::AutoPoint(0, 0, 0)));
    }

    /// A closed row is not hit at all, and its clips still are.
    #[test]
    fn a_closed_automation_row_gets_in_nobodys_way() {
        let mut v = a_view(&[]);
        v.visible_lanes = 8;
        let l = Layout::new(rect(), &v);
        assert!(
            !l.rows.iter().any(|r| matches!(r.kind, RowKind::Automation(..))),
            "a closed lane laid out an automation row"
        );
        // The lane under the pointer is still the track's own.
        let p = l.lane_rows[0].center();
        assert!(
            matches!(hit_test(rect(), &v, p), Some(Hit::Lane(0, _))),
            "the track lane stopped answering"
        );
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

                // **Measured from the first TRACK row**, not from the top
                // of the area: the master and the click stand above them
                // now — see `BUS_ROW_SHARE` — and what this test is about is
                // that the tracks are evenly spaced and the window is the
                // right one, both of which the bus rows leave alone.
                // The master's row stands above the tracks and takes HALF a
                // track's height — see `MASTER_ROW_SHARE` — so the tracks
                // divide what is left of the area after its share.
                let lane_h =
                    l.lanes_area.height() / (visible as f32 + MASTER_ROW_SHARE);
                let first = scroll.min(CHANNELS.saturating_sub(visible));
                let top = l
                    .rows
                    .iter()
                    .find(|r| matches!(r.kind, RowKind::Track(_)) && r.lane.is_positive())
                    .map_or(l.lanes_area.top(), |r| r.lane.top());
                for i in 0..CHANNELS {
                    let got = l.lane_rows[i];
                    if i < first || i >= first + visible {
                        assert!(
                            !got.is_positive(),
                            "lane {i} is laid out at visible={visible} scroll={scroll}"
                        );
                        continue;
                    }
                    let want_top = top + lane_h * (i - first) as f32;
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
        // One automation row on lane 0. A lane shows as many as are ticked
        // now, so the view has to say so rather than a flag.
        v.lanes[0].automation = vec![AutoView {
            param: crate::automation::AutoParam::Gain,
            label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            selected: false,
            points: &[],
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
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
            .find(|r| r.kind == RowKind::Automation(0, 0))
            .expect("no automation row");
        assert!(
            (auto.lane.top() - l.lane_rows[0].bottom()).abs() < 0.5,
            "lane 0's automation is not under lane 0"
        );
        // **The track SHRANK, and that is the point.**
        //
        // This used to assert the opposite — a sub-lane took its room from
        // the area and never from the track — and the consequence was that
        // opening a curve made the stack taller than the area, so `push`
        // dropped whatever no longer wholly fitted and left half a strip of
        // dead space at the bottom with a track missing. The owner asked for
        // the other trade by name: "it still takes half a strip, but tracks
        // ALWAYS fit such that there is never dead space when they are
        // added."
        //
        // So every open row is in the divisor and the tracks give up a slice
        // each. A curve costs half a track's height spread across all of
        // them, rather than one whole track falling off the end.
        assert!(
            l.lane_rows[0].height() < track_h,
            "opening a curve did not cost the tracks anything: still {track_h}"
        );

        // **And NOTHING hangs off the bottom, which is the whole ask.**
        let mut lowest = l.lanes_area.top();
        for r in &l.rows {
            assert!(
                !r.lane.is_positive() || r.lane.bottom() <= l.lanes_area.bottom() + 0.5,
                "{:?} hangs off the bottom of the area",
                r.kind
            );
            if r.lane.is_positive() {
                lowest = lowest.max(r.lane.bottom());
            }
        }
        // No dead space either: the stack reaches the bottom. A row short of
        // it is the gap the owner was looking at.
        assert!(
            (l.lanes_area.bottom() - lowest).abs() < 1.0,
            "the rows stop {} points above the bottom — that is the dead space",
            l.lanes_area.bottom() - lowest
        );

        // And every row that SHOULD exist does: nothing was dropped to make
        // the rest fit.
        assert!(
            l.rows
                .iter()
                .any(|r| r.kind == RowKind::Automation(0, 0) && r.lane.is_positive()),
            "the automation row itself fell off"
        );
    }

    /// **Automation shrinks the tracks, then evicts one, and repeats.**
    ///
    /// The owner's rule, in his words: "Start with 8 tracks visible, enable
    /// vol auto lane on channel 1, all tracks shrink height equally to fit an
    /// extra halfstrip; add a second lane, pan on channel one, now volume and
    /// pan (equivalent of 1 channel height) exist, push the last channel out
    /// of view." And it "repeats and scales with 3 and 4 inserts and beyond".
    ///
    /// So: every half strip of automation is absorbed by the tracks getting
    /// shorter, and every WHOLE strip's worth costs one track its place.
    /// Nothing ever hangs off the bottom and nothing ever leaves a gap.
    #[test]
    fn automation_rows_shrink_the_tracks_then_evict_one_and_repeat() {
        let c = clips();
        let lanes_shown = |l: &Layout| {
            l.rows
                .iter()
                .filter(|r| matches!(r.kind, RowKind::Track(_)) && r.lane.is_positive())
                .count()
        };
        let track_h = |l: &Layout| {
            l.rows
                .iter()
                .find(|r| matches!(r.kind, RowKind::Track(_)) && r.lane.is_positive())
                .map_or(0.0, |r| r.lane.height())
        };
        // The stack must always END flush with the area: a row short of the
        // bottom IS the dead space this is about.
        let flush = |l: &Layout, n: usize| {
            let lowest = l
                .rows
                .iter()
                .filter(|r| r.lane.is_positive())
                .fold(l.lanes_area.top(), |m, r| m.max(r.lane.bottom()));
            assert!(
                (l.lanes_area.bottom() - lowest).abs() < 1.0,
                "with {n} automation rows the stack stops {} points short — dead space",
                l.lanes_area.bottom() - lowest
            );
            for r in &l.rows {
                assert!(
                    !r.lane.is_positive() || r.lane.bottom() <= l.lanes_area.bottom() + 0.5,
                    "with {n} rows, {:?} hangs off the bottom",
                    r.kind
                );
            }
        };

        let auto = |n: usize| -> Vec<AutoView<'static>> {
            (0..n)
                .map(|_| AutoView {
                    param: crate::automation::AutoParam::Gain,
                    label: "Volume".to_owned(),
                    resting: 0.5,
                    armed: false,
                    bypassed: false,
                    selected: false,
                    points: &[],
                    hot: None,
                    chosen: Vec::new(),
                    moving: None,
                })
                .collect()
        };

        // No automation: the baseline both counts are measured against.
        let mut v = a_view(&c);
        v.show_tempo_lane = false;
        v.show_master_row = false;
        let base = Layout::new(rect(), &v);
        let (base_n, base_h) = (lanes_shown(&base), track_h(&base));
        assert!(base_n >= 4, "too few lanes visible to test eviction: {base_n}");
        flush(&base, 0);

        let mut last_n = base_n;
        let mut last_h = base_h;
        for n in 1..=5usize {
            let mut v = a_view(&c);
            v.show_tempo_lane = false;
            v.show_master_row = false;
            let rows = auto(n);
            v.lanes[0].automation = rows;
            let l = Layout::new(rect(), &v);
            flush(&l, n);

            let want_n = base_n - n / 2;
            assert_eq!(
                lanes_shown(&l),
                want_n,
                "with {n} automation rows {} tracks show, not {want_n}",
                lanes_shown(&l)
            );
            // An ODD count leaves half a strip over, so the tracks are
            // shorter than the baseline; an EVEN one is a whole track's
            // worth and they come back to full height.
            if n % 2 == 0 {
                assert!(
                    (track_h(&l) - base_h).abs() < 0.5,
                    "{n} rows is a whole track's worth, so the tracks should be \
                     back to {base_h}, not {}",
                    track_h(&l)
                );
            } else {
                assert!(
                    track_h(&l) < base_h - 0.5,
                    "{n} rows left half a strip over and the tracks did not shrink"
                );
            }
            // Never grows a track back while also keeping every lane: the
            // sequence only ever costs.
            assert!(
                lanes_shown(&l) <= last_n,
                "adding a row put a track BACK on screen"
            );
            last_n = lanes_shown(&l);
            last_h = track_h(&l);
        }
        let _ = last_h;
    }

    /// **An automation row's header has every cell it is supposed to.**
    ///
    /// The owner reported the lanes as having "no labels, and no x button to
    /// close them". Every one of them is drawn — so if they are missing, the
    /// header they are carved out of is degenerate. This asks for each cell
    /// at the size a real row actually gets.
    #[test]
    fn an_automation_rows_header_has_its_switches_label_and_dial() {
        let c = clips();
        let rows = vec![AutoView {
            param: crate::automation::AutoParam::Gain,
            label: "Volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            selected: false,
            points: &[],
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }];
        let mut v = a_view(&c);
        v.lanes[0].automation = rows;
        let l = Layout::new(rect(), &v);
        let row = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(0, 0))
            .expect("no automation row");
        assert!(row.header.is_positive(), "the row has no header to carve");

        let h = auto_header(row.header);
        assert!(h.close.is_positive(), "no × to close the lane");
        assert!(h.arm.is_positive(), "no record arm");
        assert!(h.bypass.is_positive(), "no bypass");
        assert!(h.name.is_positive(), "nowhere to write the label");
        assert!(h.dial.is_positive(), "no dial");
        assert!(h.readout.is_positive(), "no readout");

        // The label needs ROOM, not just a rect: the draw refuses under
        // twenty points and would leave the lane looking unnamed.
        assert!(
            h.name.width() > 24.0,
            "the label has {} points to print in — it will not draw",
            h.name.width()
        );
        // And nothing overlaps: a switch under the label is a switch that
        // cannot be pressed.
        for (a, an) in [(h.close, "close"), (h.arm, "arm"), (h.bypass, "bypass")] {
            assert!(
                a.right() <= h.name.left() + 0.5,
                "the {an} switch runs into the label"
            );
        }
        assert!(
            h.name.right() <= h.dial.left() + 0.5,
            "the label runs into the dial"
        );
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
        // The first visible lane sits at the top of the TRACKS — under the
        // master's row, which is not a track and does not scroll.
        let buses: f32 = l
            .rows
            .iter()
            .filter(|r| r.kind == RowKind::Master)
            .map(|r| r.lane.height())
            .sum();
        assert!(
            (l.lane_rows[CHANNELS - VISIBLE_LANES].top() - l.lanes_area.top() - buses).abs()
                < 0.5,
            "the scrolled window does not start at the top of the tracks"
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
        (l.tool_tempo, Hit::ToolTempoLane),
        ] {
            assert!(cell.is_positive(), "{want:?} has no room in the corner");
            assert_eq!(hit_test(r, &v, cell.center()), Some(want));
        }
        let row = l.header_rows[0];
        assert_eq!(hit_test(r, &v, row.knob.center()), Some(Hit::Gain(0)));
        assert_eq!(hit_test(r, &v, row.send.center()), Some(Hit::Send(0)));
        assert!(row.knob_readout.is_positive(), "the volume lost its readout");
        assert!(row.send_readout.is_positive(), "the send lost its readout");
        // **The ladder the owner asked for, in the meter he works in.**
        // Going finer: 1, 1/2, the glyph, 1/8, 1/16 — one 1/4 state, and it
        // is the glyph, sitting exactly between 1/8 and 1/2.
        assert_eq!(grid_label(0, 4, 4), "1");
        assert_eq!(grid_label(3, 4, 4), "8");
        assert_eq!(grid_label(-1, 4, 4), "1/2");
        assert_eq!(grid_label(-2, 4, 4), "\u{266a}");
        assert_eq!(grid_label(-3, 4, 4), "1/8");
        assert_eq!(grid_label(-4, 4, 4), "1/16");
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

    // ── the note miniature ──────────────────────────────────────────────
    //
    // A MIDI clip's miniature is drawn from its notes and its LOOP: a bar
    // written once and dragged over two hundred bars is two hundred passes
    // of the same notes. What these check is that the picture still lights
    // the same pixels now that the drawing skips the passes that are off
    // screen and merges marks that share a column — see `draw_note_miniature`
    // and `visible_passes`.

    /// **The miniature as it was drawn before it learned to skip and merge:**
    /// every note of every pass, one rect each.
    ///
    /// Kept because it is the SPECIFICATION of the picture. The drawing in
    /// `draw_note_miniature` walks only the passes that reach the window and
    /// merges marks that share a pixel column, and the only honest way to
    /// say those two changes did not alter what the eye gets is to keep the
    /// plain walk and check the pixels against it. A golden number would
    /// have said the same count came out; this says the same picture did.
    pub(super) fn reference_miniature(
        painter: &Painter,
        r: Rect,
        full_x: (f32, f32),
        clip: &ClipView<'_>,
    ) {
        let len = clip.len_s.max(1.0e-9);
        let passes: usize = if clip.loop_s > 1.0e-6 {
            ((clip.len_s / clip.loop_s).ceil() as usize).clamp(1, 512)
        } else {
            1
        };
        let (mut lo, mut hi) = (127u8, 0u8);
        for n in clip.notes {
            lo = lo.min(n.key);
            hi = hi.max(n.key);
        }
        if lo > hi {
            return;
        }
        while u16::from(hi) - u16::from(lo) < 12 {
            lo = lo.saturating_sub(1);
            hi = (hi + 1).min(127);
            if lo == 0 && hi == 127 {
                break;
            }
        }
        let span = f32::from(hi - lo).max(1.0);
        let inner = {
            let base = r.shrink(2.0);
            let band = clip_name_band(r);
            if band > 0.0 && base.height() > band {
                Rect::from_min_max(Pos2::new(base.left(), base.top() + band), base.max)
            } else {
                base
            }
        };
        if !inner.is_positive() {
            return;
        }
        let bar_h = (inner.height() / span).clamp(1.5, 4.0);
        let (fx0, fx1) = full_x;
        let fw = (fx1 - fx0).max(1.0);
        for (pass, n) in (0..passes).flat_map(|k| clip.notes.iter().map(move |n| (k, n))) {
            let shift = clip.loop_s * pass as f64;
            let a = (n.at_s - clip.offset_s + shift) / len;
            let b = (n.at_s + n.dur_s.max(0.02) - clip.offset_s + shift) / len;
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
                Color32::WHITE,
            );
        }
    }

    /// One bar of the part that gets looped: sixteen sixteenths, four voices
    /// deep, spread over three octaves so no two marks share a row by luck.
    pub(super) fn a_looped_bar(bar_s: f64) -> Vec<crate::arrangement::ArrNote> {
        (0..64u32)
            .map(|i| crate::arrangement::ArrNote {
                at_s: f64::from(i / 4) * (bar_s / 16.0),
                dur_s: bar_s / 32.0,
                key: 48 + u8::try_from((i * 7) % 36).unwrap_or(0),
                vel: 100,
            })
            .collect()
    }

    /// **The same bar with a note held across it**, which is what almost all
    /// real music has and what the merge used to be vetoed by.
    ///
    /// A bass note under the part, sounding the whole bar. The merge was
    /// first gated on the clip's LONGEST note, so this one note used to send
    /// every other note in the clip down the plain path — and the probe that
    /// said the merge was worth 2x had no bar like this in it.
    pub(super) fn a_bar_with_a_held_note(bar_s: f64) -> Vec<crate::arrangement::ArrNote> {
        let mut notes = a_looped_bar(bar_s);
        notes.push(crate::arrangement::ArrNote {
            at_s: 0.0,
            dur_s: bar_s,
            key: 36,
            vel: 100,
        });
        notes
    }

    /// A looped MIDI clip `bars` bars long, built from a one-bar loop.
    pub(super) fn a_looped_clip<'a>(
        notes: &'a [crate::arrangement::ArrNote],
        bar_s: f64,
        bars: f64,
    ) -> ClipView<'a> {
        ClipView {
            id: 1,
            lane: 0,
            start_s: 0.0,
            len_s: bar_s * bars,
            max_len_s: f64::INFINITY,
            fade_in_s: 0.0,
            fade_out_s: 0.0,
            name: "loop",
            wave: &[],
            wave_from_s: 0.0,
            wave_span_s: 0.0,
            notes,
            loop_s: bar_s,
            offset_s: 0.0,
            selected: false,
            midi: true,
            recording: false,
        }
    }

    /// **How finely the coverage is sampled**: four cells to the point, in
    /// both directions.
    ///
    /// Not one, which was the first try and was wrong. A mark 1.5 points
    /// wide covers one whole pixel or two depending on the FRACTION it
    /// starts at, so counting whole pixels measures the sub-pixel phase as
    /// much as the ink — and the merged drawing, which places every mark on
    /// a column boundary, has one phase where the plain walk has all of
    /// them. Sampling finer than the marks are wide measures the ink.
    const SUB: f32 = 4.0;

    /// The samples a drawing covers, sorted, at [`SUB`] to the point.
    type Cells = Vec<(i32, i32)>;

    /// **What a miniature LIGHTS**, at [`SUB`] cells to the point: every
    /// sample any of its rects covers. This is the picture as an eye gets
    /// it, so two drawings that agree here are the same miniature however
    /// differently they were built.
    fn lit_cells(draw: impl FnOnce(&Painter)) -> Cells {
        let ctx = egui::Context::default();
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("mini-cover"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        draw(&painter);
        let mut cells: Vec<(i32, i32)> = ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Rect(q) => Some(q.rect),
                            _ => None,
                        })
                        .flat_map(|q| {
                            let s = |v: f32| (v * SUB).round() as i32;
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

    /// Both drawings of one clip in one view, as lit cells.
    fn both_miniatures(clip: &ClipView<'_>, view: &ArrangeView<'_>, area: Rect) -> (Cells, Cells) {
        let r = clip_rect(area, view, clip);
        let full_x = (
            x_of(area, view, clip.start_s),
            x_of(area, view, clip.start_s + clip.len_s),
        );
        let go = |f: fn(&Painter, Rect, (f32, f32), &ClipView<'_>)| {
            lit_cells(|p| {
                if r.is_positive() {
                    f(p, r, full_x, clip);
                }
            })
        };
        (
            go(reference_miniature),
            go(|p, r, fx, c| draw_note_miniature(p, r, fx, c, Color32::WHITE)),
        )
    }

    /// Both drawings of one clip in one view, as the rects they emitted —
    /// what `both_miniatures` gives as coverage, given as marks instead, for
    /// the checks that are about a mark's WIDTH rather than about ink.
    fn both_rects(clip: &ClipView<'_>, view: &ArrangeView<'_>, area: Rect) -> (Vec<Rect>, Vec<Rect>) {
        let r = clip_rect(area, view, clip);
        let full_x = (
            x_of(area, view, clip.start_s),
            x_of(area, view, clip.start_s + clip.len_s),
        );
        let go = |f: &dyn Fn(&Painter, Rect, (f32, f32), &ClipView<'_>)| {
            let ctx = egui::Context::default();
            let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("mini-rects"));
            let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
            if r.is_positive() {
                f(&painter, r, full_x, clip);
            }
            ctx.graphics(|g| {
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
            })
        };
        (
            go(&reference_miniature),
            go(&|p, r, fx, c| draw_note_miniature(p, r, fx, c, Color32::WHITE)),
        )
    }

    /// How far a cell is from the nearest cell of a sorted set, in the
    /// square sense — 0 when the set holds it.
    fn is_near(set: &[(i32, i32)], cell: (i32, i32), reach: i32) -> bool {
        (-reach..=reach).any(|dx| {
            (-reach..=reach).any(|dy| set.binary_search(&(cell.0 + dx, cell.1 + dy)).is_ok())
        })
    }

    /// The zooms and scroll positions the two drawings are compared at. The
    /// scrolled ones matter most: they are where the passes BEFORE the
    /// window live, and skipping those is the whole point of the change.
    const MINIATURE_VIEWS: [(&str, f64, f64); 6] = [
        ("whole clip", 0.0, 120.0),
        ("half", 0.0, 60.0),
        ("half, scrolled", 60.0, 60.0),
        ("16 bars", 0.0, 32.0),
        ("16 bars, scrolled", 64.0, 32.0),
        ("4 bars, scrolled", 70.0, 8.0),
    ];

    /// **The fast miniature draws the plain one's picture**, at every zoom
    /// and both scroll positions.
    ///
    /// Cell for cell, one pixel of slack: the column merge places a mark
    /// narrower than a pixel at its column rather than at its fraction of
    /// one, and that is the entire licence it is given. Nothing may appear
    /// where the plain walk drew nothing, and nothing the plain walk drew
    /// may go missing — the failure that a count-only check would pass.
    #[test]
    fn the_fast_miniature_draws_the_same_picture_as_the_plain_walk() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        // Both trims: a clip as recorded, and one dragged in from its left.
        // The offset moves every pass, and a pass range worked out without
        // it would be right on one of these and wrong on the other.
        for offset_s in [0.0, bar_s * 0.375] {
            let clips = [ClipView { offset_s, ..a_looped_clip(&notes, bar_s, 60.0) }];
            the_two_miniatures_agree(&clips, offset_s);
        }
    }

    fn the_two_miniatures_agree(clips: &[ClipView<'_>], offset_s: f64) {
        for (what, start_s, len_s) in MINIATURE_VIEWS {
            let what = &format!("{what} (trim {offset_s})");
            let mut v = a_view(clips);
            v.view_start_s = start_s;
            v.view_len_s = len_s;
            v.song_s = 120.0;
            let (want, got) = both_miniatures(&clips[0], &v, rect());
            assert!(!want.is_empty(), "{what}: the plain walk drew nothing");
            let reach = SUB as i32;
            let lost = want.iter().filter(|c| !is_near(&got, **c, reach)).count();
            let gained = got.iter().filter(|c| !is_near(&want, **c, reach)).count();
            assert_eq!(lost, 0, "{what}: {lost} of {} lit cells went missing", want.len());
            assert_eq!(gained, 0, "{what}: {gained} cells lit that the plain walk left dark");
            // The ink must not thin out either: a merge that dropped every
            // second mark would still be "within a point" of the original,
            // and a miniature half as dark is a miniature that changed. The
            // worst of the six views is 1.6% lighter; three per cent is
            // headroom for a platform whose floats round the other way.
            let ratio = got.len() as f64 / want.len() as f64;
            assert!(
                (0.97..=1.03).contains(&ratio),
                "{what}: {} lit cells against {}, a ratio of {ratio:.3}",
                got.len(),
                want.len()
            );
        }
    }

    /// **A held note does not veto the merge, and is not folded into it.**
    ///
    /// This is the case the first version of the fix got wrong, and it got it
    /// wrong because the probe it was measured against had no example of it:
    /// with every note the same length, a merge gated on the LONGEST note is
    /// a merge gated on nothing, and the 2x it reported was an artifact of
    /// its own notes. One held note is ordinary music, and it used to send
    /// the whole clip down the plain path.
    ///
    /// Two halves, and both are needed. The picture still has to be the plain
    /// walk's — a held note crossing a hundred columns must be drawn as the
    /// line it is and not folded into the column it starts in, which is the
    /// obvious way to make this fast and wrong. And the saving has to
    /// survive: the sub-pixel notes around it still merge.
    #[test]
    fn a_held_note_is_drawn_as_a_span_and_the_notes_around_it_still_merge() {
        let bar_s = 2.0;
        let notes = a_bar_with_a_held_note(bar_s);
        for offset_s in [0.0, bar_s * 0.375] {
            let clips = [ClipView { offset_s, ..a_looped_clip(&notes, bar_s, 60.0) }];
            the_two_miniatures_agree(&clips, offset_s);
        }
        // Zoomed right out over two hundred bars, which is the shape the
        // probe measures and the shape the complaint was about: every short
        // note a fifth of a point wide, the held one several points, and
        // twelve marks fighting for every column.
        let clips = [a_looped_clip(&notes, bar_s, 200.0)];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = bar_s * 200.0;
        v.song_s = bar_s * 200.0;
        let (want, got) = both_rects(&clips[0], &v, rect());
        // The saving: the merge must still be doing its work, not sitting
        // vetoed by the one long note. The plain walk draws every note of
        // every pass; the gate this replaced would have made these equal.
        assert!(
            got.len() * 4 < want.len() * 3,
            "one held note gave the merge back: {} marks against the plain walk's {}",
            got.len(),
            want.len()
        );
        // And the held note is still a LINE. The widest mark the plain walk
        // draws is the held note's span; the drawing has to have one about
        // that wide, or it folded a bar-long note into a stub.
        let widest = |v: &[Rect]| v.iter().map(|q| q.width()).fold(0.0f32, f32::max);
        let (wide_want, wide_got) = (widest(&want), widest(&got));
        assert!(
            wide_want > 4.0,
            "the plain walk's widest mark is {wide_want}, so this zoom proves nothing"
        );
        assert!(
            wide_got >= wide_want - 1.0,
            "the held note came out {wide_got} wide against the plain walk's {wide_want} \
             — a bar-long note folded into the column it starts in"
        );
    }

    /// **A clip whose notes all land in one column still draws something.**
    ///
    /// The column merge folds a whole loop into one bar here, and folding it
    /// into nothing is the obvious way to get that wrong.
    #[test]
    fn a_loop_squeezed_into_one_column_still_draws() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        // A clip two bars long against a window seven minutes wide: four
        // seconds of it is a dozen points, so its whole loop — both passes,
        // 128 notes — shares a handful of columns. Any narrower and the
        // clip has no inner rect at all, which is the next test's business.
        let clips = [a_looped_clip(&notes, bar_s, 2.0)];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 433.0;
        v.song_s = 433.0;
        let (want, got) = both_miniatures(&clips[0], &v, rect());
        assert!(!want.is_empty(), "the plain walk drew nothing to compare to");
        assert!(!got.is_empty(), "a squeezed loop drew nothing at all");
        assert!(
            got.len() * 2 >= want.len(),
            "a squeezed loop faded to {} cells from {}",
            got.len(),
            want.len()
        );
    }

    /// **Nothing to draw, and no room to draw it**: neither panics, and
    /// neither paints.
    #[test]
    fn an_empty_or_flattened_miniature_draws_nothing_and_survives() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        let none: [crate::arrangement::ArrNote; 0] = [];
        let empty = ClipView { notes: &none, ..a_looped_clip(&notes, bar_s, 8.0) };
        let clips = [empty];
        let v = a_view(&clips);
        let cells = lit_cells(|p| {
            draw_note_miniature(p, Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 40.0)), (0.0, 200.0), &clips[0], Color32::WHITE);
        });
        assert!(cells.is_empty(), "an empty clip painted {} cells", cells.len());
        // No width, no height, and a rect that is not one at all.
        let held = [a_looped_clip(&notes, bar_s, 8.0)];
        for r in [
            Rect::from_min_size(Pos2::ZERO, Vec2::new(0.0, 40.0)),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, 0.0)),
            Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
            Rect::NOTHING,
        ] {
            let cells = lit_cells(|p| {
                draw_note_miniature(p, r, (0.0, 200.0), &held[0], Color32::WHITE);
            });
            assert!(cells.is_empty(), "{r:?} painted {} cells", cells.len());
        }
        // A clip with no length at all: the mapping divides by it.
        let flat = ClipView { len_s: 0.0, loop_s: 0.0, ..a_looped_clip(&notes, bar_s, 8.0) };
        let flats = [flat];
        let _ = lit_cells(|p| {
            draw_note_miniature(p, rect(), (10.0, 10.0), &flats[0], Color32::WHITE);
        });
        let _ = v;
    }

    /// **A loop with an enormous pass count is bounded work.**
    ///
    /// A one-beat loop over an hour holds thousands of passes; zoomed to a
    /// few bars, all but a handful are off screen. The old walk touched
    /// every one of them (capped at 512) whatever the window showed, so this
    /// pins the new rule: what gets walked is what can be SEEN.
    #[test]
    fn a_huge_pass_count_walks_only_the_passes_that_show() {
        let beat_s = 0.5;
        let notes = a_looped_bar(beat_s);
        let hour = ClipView { loop_s: beat_s, ..a_looped_clip(&notes, beat_s, 7200.0) };
        // Four bars of an hour-long clip: two passes' worth, plus the one
        // either side that `visible_passes` is deliberately generous with.
        let seen = visible_passes(
            (3600.0f64 / beat_s) as usize,
            beat_s,
            0.0,
            3600.0,
            (0.0, beat_s),
            (0.0, 1300.0 * 3600.0 / 8.0),
            (600.0, 700.0),
        );
        assert!(
            seen.len() <= 4,
            "a hundred-point window walked {} passes of a beat-long loop",
            seen.len()
        );
        // And the picture it produces is still the plain walk's. The plain
        // walk caps at 512 passes, so compare where that cap is not reached.
        let clips = [hour];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 60.0;
        v.song_s = 7200.0;
        let (want, got) = both_miniatures(&clips[0], &v, rect());
        assert!(!got.is_empty(), "an hour-long loop drew nothing");
        assert!(!want.is_empty(), "the plain walk drew nothing");
    }

    /// **Every pass that reaches the window is walked, and not many more.**
    ///
    /// The range is deliberately generous by one at each end — dropping a
    /// note at the edge would be a picture that changed as you scrolled — so
    /// this pins both halves: no pass that shows is skipped, and the slack
    /// stays slack rather than growing back into the whole song.
    #[test]
    fn the_visible_pass_range_is_generous_by_one_and_no_more() {
        // A 200-bar clip of a one-bar loop, drawn 1300 points wide, with a
        // window over bars 100..104.
        let (bar_s, bars) = (2.0, 200.0);
        let len = bar_s * bars;
        let full_w = 1300.0 * (len / (bar_s * 4.0)) as f32;
        let seen = visible_passes(
            200,
            bar_s,
            0.0,
            len,
            (0.0, bar_s),
            (-full_w * 100.0 / 200.0, full_w - full_w * 100.0 / 200.0),
            (0.0, 1300.0),
        );
        assert!(seen.contains(&100), "the pass under the window was skipped");
        assert!(seen.contains(&103), "the last pass in the window was skipped");
        assert!(seen.len() <= 8, "four bars walked {} passes", seen.len());
        // Degenerate inputs hand back the whole range rather than an empty
        // one: slow beats blank.
        for (loops, l, notes) in [
            (0.0, 400.0, (0.0, 2.0)),
            (2.0, 0.0, (0.0, 2.0)),
            (f64::NAN, 400.0, (0.0, 2.0)),
            (2.0, 400.0, (f64::NAN, 2.0)),
        ] {
            let all = visible_passes(200, loops, 0.0, l, notes, (0.0, 1300.0), (0.0, 1300.0));
            assert_eq!(all, 0..200, "loop {loops} len {l} narrowed the walk");
        }
    }

}


#[cfg(test)]
mod miniature_cost_probe {
    use super::tests::{a_view, reference_miniature, rect};
    use super::*;
    use crate::arrangement::ArrNote;
    use std::time::{Duration, Instant};

    /// One bar of nothing but short notes: sixteen sixteenths, four voices
    /// deep, so 64 notes over 36 semitones, every one of them a thirty-second
    /// long.
    ///
    /// This is the shape the first version of the fix was measured against,
    /// and it flattered it: with every note the same length, a merge gated on
    /// the LONGEST note is a merge gated on nothing. Kept so the two shapes
    /// can be read side by side.
    fn a_bar_of_short_notes(bar_s: f64) -> Vec<ArrNote> {
        (0..64u32)
            .map(|i| ArrNote {
                at_s: f64::from(i / 4) * (bar_s / 16.0),
                dur_s: bar_s / 32.0,
                key: 48 + u8::try_from((i * 7) % 36).unwrap_or(0),
                vel: 100,
            })
            .collect()
    }

    /// **One bar of music**, which is the same part with the notes a player
    /// would actually hold held.
    ///
    /// Every fourth beat carries a note that sounds through to the end of
    /// the bar — a bass note, a pad, a chord let ring. That is not an exotic
    /// case, it is what a bar of almost anything looks like, and it is the
    /// case the first probe had no example of. Measure only the bar above and
    /// a merge vetoed by one long note reports a win it does not have.
    fn a_bar(bar_s: f64) -> Vec<ArrNote> {
        let mut notes = a_bar_of_short_notes(bar_s);
        for (i, held) in [(0u32, 36u8), (16, 43), (32, 48), (48, 55)] {
            notes.push(ArrNote {
                at_s: f64::from(i / 4) * (bar_s / 16.0),
                dur_s: bar_s - f64::from(i / 4) * (bar_s / 16.0),
                key: held,
                vel: 100,
            });
        }
        notes
    }

    /// The same music laid out FLAT over `bars` bars instead of looped: one
    /// clip holding every note it plays, which is what a recorded part is.
    ///
    /// The pass skip does nothing here — one pass is all there is — so this
    /// isolates what the merge and the pre-pass cost and buy on their own.
    fn a_flat_part(bar_s: f64, bars: usize) -> Vec<ArrNote> {
        let one = a_bar(bar_s);
        (0..bars)
            .flat_map(|b| {
                one.iter().map(move |n| ArrNote {
                    at_s: n.at_s + b as f64 * bar_s,
                    ..*n
                })
            })
            .collect()
    }

    /// A layer that nothing else paints into, so the shapes read back are
    /// this probe's and only this probe's.
    fn a_painter(ctx: &egui::Context) -> (egui::LayerId, Painter) {
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("mini-probe"));
        (layer, Painter::new(ctx.clone(), layer, Rect::EVERYTHING))
    }

    fn shed(ctx: &egui::Context, layer: egui::LayerId) {
        ctx.graphics_mut(|g| *g.entry(layer) = Default::default());
    }

    /// The shapes the miniature pass emitted, ready to hand to the
    /// tessellator — the second half of what a frame actually pays for a
    /// rect, and the half a shape count alone hides.
    fn shapes(ctx: &egui::Context, layer: egui::LayerId) -> Vec<egui::epaint::ClippedShape> {
        ctx.graphics(|g| {
            g.get(layer).map(|list| list.all_entries().cloned().collect()).unwrap_or_default()
        })
    }

    /// Every rect the miniature pass emitted, in draw order.
    fn rects(ctx: &egui::Context, layer: egui::LayerId) -> Vec<Rect> {
        ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries()
                        .filter_map(|c| match &c.shape {
                            egui::Shape::Rect(r) => Some(r.rect),
                            _ => None,
                        })
                        .collect()
                })
                .unwrap_or_default()
        })
    }

    /// Probe: what do the arrangement's MIDI miniatures cost every frame,
    /// drawn plainly and drawn as they are now? Ignored by default; run with
    /// `--ignored --nocapture`, and in RELEASE — a debug build reports about
    /// ten times the truth and the app ships optimised.
    ///
    /// The document is the survey's: eight lanes each carrying the same part
    /// stretched over 200 bars. The notes are scattered over three octaves
    /// on purpose — that is the WORST case for the column merge, because
    /// marks at neighbouring pitches are what it gets to fold together.
    ///
    /// **Three shapes of clip, because one of them was hiding two faults.**
    /// The first version of this probe measured a single looped bar whose
    /// notes were all the same short length, and on that one shape a merge
    /// gated on the clip's LONGEST note is a merge gated on nothing: it
    /// reported a 2x that one held note anywhere in the bar would have taken
    /// straight back. And it never drew a clip that does NOT loop, so a
    /// pre-pass added for the benefit of looped clips went unmeasured on the
    /// clips that could not use it. A bar of music and a flat recorded part
    /// are here now, and the numbers are read across all three.
    ///
    /// Two suspects this ruled out before the fix was written, recorded so
    /// nobody pays to find out twice. The 1-point corner radius on a mark
    /// 1.5 points wide costs nothing — epaint clamps it, and tessellating
    /// the same picture square came out within 5%. And deduplicating marks
    /// by (column, row) removes only 14% of them: the marks really are at
    /// different heights, so the merge has to be by touching RUNS or it is
    /// not worth writing.
    #[test]
    #[ignore]
    fn what_do_the_note_miniatures_pay_every_frame() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let (layer, painter) = a_painter(&ctx);

        // 120 bpm, 4/4: a bar is two seconds, 200 bars is 400.
        let bar_s = 2.0;
        let song_s = bar_s * 200.0;
        let r = rect();

        // Three clips, because one of them was hiding two faults. The bar of
        // music is the case that matters; the bar of short notes is what the
        // first measurement used, kept so the difference is visible; and the
        // flat part is the one nothing here ever measured — a clip with no
        // loop, which the pass skip cannot help and which therefore has to be
        // shown paying nothing for it.
        let looped_music = a_bar(bar_s);
        let looped_short = a_bar_of_short_notes(bar_s);
        let flat = a_flat_part(bar_s, 200);
        for (part, notes, loop_s) in [
            ("a looped clip of music", &looped_music, bar_s),
            ("a looped clip of sub-pixel notes", &looped_short, bar_s),
            ("a clip that does not loop", &flat, 0.0),
        ] {
        let clips: Vec<ClipView<'_>> = (0..VISIBLE_LANES)
            .map(|lane| ClipView {
                id: lane as u64 + 1,
                lane,
                start_s: 0.0,
                len_s: song_s,
                max_len_s: f64::INFINITY,
                fade_in_s: 0.0,
                fade_out_s: 0.0,
                name: "loop",
                wave: &[],
                wave_from_s: 0.0,
                wave_span_s: 0.0,
                notes,
                loop_s,
                offset_s: 0.0,
                selected: false,
                midi: true,
                recording: false,
            })
            .collect();

        // The whole song, sixteen bars, and four bars: zoomed out is the
        // worst case for how much is drawn, zoomed in is the case an owner
        // actually spends the day in.
        for (what, start_s, len_s) in [
            ("whole song (200 bars)", 0.0, song_s),
            ("16 bars", 100.0, bar_s * 16.0),
            ("4 bars", 100.0, bar_s * 4.0),
        ] {
            let mut v = a_view(&clips);
            v.view_start_s = start_s;
            v.view_len_s = len_s;
            v.song_s = song_s;

            // Both drawings, so the before and the after are one command
            // and not a number somebody wrote down once. `plain` is the walk
            // this function used to be, kept in `mod tests` as the picture
            // the fast one has to reproduce.
            let sweep = |painter: &Painter, plain: bool| {
                for clip in &clips {
                    let cr = clip_rect(r, &v, clip);
                    if !cr.is_positive() {
                        continue;
                    }
                    let full_x = (
                        x_of(r, &v, clip.start_s),
                        x_of(r, &v, clip.start_s + clip.len_s),
                    );
                    if plain {
                        reference_miniature(painter, cr, full_x, clip);
                    } else {
                        draw_note_miniature(painter, cr, full_x, clip, Color32::WHITE);
                    }
                }
            };

            println!("{part}, {what} ({} notes a clip):", notes.len());
            for (which, plain) in [("the plain walk", true), ("as drawn now", false)] {
                shed(&ctx, layer);
                sweep(&painter, plain);
                let drawn = rects(&ctx, layer);
                // How many columns the picture spans, which is the
                // resolution it can carry: marks past one per column per
                // pitch band are paint laid on paint.
                let mut per_col: Vec<i32> =
                    drawn.iter().map(|q| q.left().floor() as i32).collect();
                per_col.sort_unstable();
                per_col.dedup();
                shed(&ctx, layer);

                let n = 30;
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let t = Instant::now();
                    sweep(&painter, plain);
                    total += t.elapsed();
                    shed(&ctx, layer);
                }
                let per = total / n;

                // And what the tessellator then pays for them — the half of
                // the bill a shape count hides, and the larger half.
                sweep(&painter, plain);
                let mut tess = Duration::ZERO;
                let m = 10;
                for _ in 0..m {
                    let s = shapes(&ctx, layer);
                    let t = Instant::now();
                    std::hint::black_box(ctx.tessellate(s, 1.0));
                    tess += t.elapsed();
                }
                let tess = tess / m;
                shed(&ctx, layer);

                println!(
                    "  {which:>14}: {} rects over {} columns -> emit {:?} + tessellate {:?} = {:?}, {:.1}% of a 16.6ms frame",
                    drawn.len(),
                    per_col.len(),
                    per,
                    tess,
                    per + tess,
                    (per + tess).as_secs_f64() / 0.0166 * 100.0
                );
            }
        }
        }
    }
}
