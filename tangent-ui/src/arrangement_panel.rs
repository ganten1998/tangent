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
use crate::recorder::{ChannelKind, Column, VISIBLE_MAX};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

/// One lane's header, as the painter sees it. Lane N IS channel N — name,
/// colour, kind, arm, mute and solo are the mixer strip's own facts.
pub struct LaneView<'a> {
    pub name: &'a str,
    /// What the input chip says: "IN 3", "MIDI: ALL", or "" for none.
    pub input: String,
    /// **The same fact in as few characters as it can be told in**, for the
    /// button beside the name.
    ///
    /// The selector used to have a band of the column to itself and could
    /// carry a sentence; every lane wears six bays now and the second rack
    /// took that band, so the button has a third of the width it had. "MIDI:
    /// Scarlett 2i2" in twenty-six points is "MIDI:..." — a word nobody can
    /// read is not a button. The long form stays: the mixer strip still
    /// prints it, and it is what this header's screen-reader node says.
    pub input_short: String,
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
    /// extra rack, `Rack::Chain(1)`), everybody else the first
    /// three and blanks.
    pub inserts: [&'a str; crate::recorder::INSERTS * 2],
    /// **Which of those bays are BYPASSED** — kept, loaded, editable, and
    /// not applied, the `AutoView::bypassed` bargain one layer down. Indexed
    /// like `inserts`, so a FOLDER's slots 3..5 answer for its extra rack;
    /// the app resolves that through `desk_id::slot_key` before it asks.
    pub bypassed: [bool; crate::recorder::INSERTS * 2],
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
    /// **Frozen**: the lane plays its rendered self and its rack rests.
    /// Painted under frost with a snowflake on the header; its clips are
    /// shown and not editable. Appended, the struct's law.
    pub frozen: bool,
    /// **What an unused lane's plus is**: an empty row's, or the desk's
    /// trailing add under the last lane, open or locked
    /// (`mixer_panel::Plus`). Read only on a lane that is not `used`.
    pub plus: crate::mixer_panel::Plus,
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
    /// **The file's picture at every zoom**, when the host has built one —
    /// `ports::ClipInfo::peaks`, borrowed through its `Arc`. When this is
    /// here the painter reads the window itself, column by column, and the
    /// `wave*` fields above are not consulted: the pyramid answers any zoom,
    /// so there is no span to be inside of and nothing for the host to
    /// redraw when the view moves. `None` falls back to the host-drawn wave.
    pub peaks: Option<&'a crate::peaks::Peaks>,
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
    /// **The clip's own level, and the picture wears it.**
    ///
    /// The waveform was drawn from the raw file's peaks, so normalising a
    /// quiet take changed how it SOUNDED and not how it LOOKED - the one
    /// place a level edit is supposed to be obvious. Reported as "when
    /// normalizing, the clip's waveform should reflect the new volume to
    /// visually see these things better".
    pub gain: f32,
    /// **Where the quantize window's detector finds attacks, and where it
    /// would put them** - `(found, target)`, in seconds of the audio the
    /// clip PLAYS from its start: the file as its warp map leaves it, then
    /// stretched, which is the axis `offset_s` is on. `found` is the attack
    /// projected through the current map (`warp::project_frame`), so a take
    /// quantized once wears its ticks on its hits, and the trim is subtracted
    /// here, at the draw. Only the attacks the clip plays
    /// arrive - inside `offset_s ..` its end, or one pass of its loop, the
    /// span `wave_band` folds each pass into - and a looped clip repeats
    /// them in every pass, as it repeats its sound. Each pass draws only the
    /// marks inside that span, whatever arrives: the host chooses in the
    /// frames it anchors in, and a hit the current map carries across the
    /// seam is projected outside it. Empty unless that window is open over
    /// this clip.
    ///
    /// The owner asked for these: "the waveform should have transient marks
    /// shown as the different settings are tweaked in its menu". A
    /// detection threshold is not a number anybody can picture, and this is
    /// the only way to turn the dial and see what it did.
    pub marks: &'a [(f32, f32)],
}

/// **A box being drawn over one automation row** to choose the nodes inside
/// it: times in song seconds, values normalised, so the row draws it through
/// the same two mappings its nodes use and the box and the nodes it lights
/// cannot disagree about what is inside.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoMarquee {
    pub lane: LaneId,
    pub slot: usize,
    pub t0: f64,
    pub t1: f64,
    pub v0: f64,
    pub v1: f64,
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

/// One named place, as the ruler needs it.
///
/// The SIGNATURE that shares its bar rides along, because the two are drawn
/// as one label — "Chorus (3/4)" — and deciding that here would mean the
/// painter asking the meter map a question the app has already answered.
pub struct MarkerView {
    pub at_s: f64,
    pub name: String,
    /// Whether this is the flag a left-press picked up. A marker's left
    /// button now SELECTS and drags and does nothing else — the owner's
    /// rule — so the strip has to say which one is held.
    pub selected: bool,
}

/// **The lanes a view carries, by row.**
///
/// It was `[LaneView; CHANNELS]`: every lane of the desk built every frame,
/// with its name, its input chip and its six bays, whether it was on screen
/// or thirty rows under it. The desk stops being a fixed width (docs/design/
/// unlimited-tracks.md, section 10), so a view now carries the WINDOW the
/// layout will place: the drawn rows from the scroll down, as many as can
/// show, each with the row it is. Rows run in order (folders are
/// positional, and a shut folder only removes rows), so a lookup by row is
/// a binary search and a walk is O(visible).
pub struct Lanes<'a> {
    lanes: Vec<(usize, LaneView<'a>)>,
    /// The RANK of the first drawn lane carried, among every drawn lane of
    /// the desk: zero for a whole desk, the scroll's own rank for the
    /// window the app builds. See [`ArrangeView::lane_scroll`].
    first: usize,
}

impl<'a> Lanes<'a> {
    /// The lanes of these rows, in row order.
    pub fn new(lanes: Vec<(usize, LaneView<'a>)>) -> Self {
        debug_assert!(lanes.windows(2).all(|w| w[0].0 < w[1].0), "rows out of order");
        Self { lanes, first: 0 }
    }

    /// These lanes begin at drawn rank `first` (the app's window).
    pub fn starting_at(mut self, first: usize) -> Self {
        self.first = first;
        self
    }

    /// Every lane from row 0, one per row: a whole desk, for a test or a
    /// picture of the whole song.
    pub fn dense(lanes: Vec<LaneView<'a>>) -> Self {
        Self { lanes: lanes.into_iter().enumerate().collect(), first: 0 }
    }

    fn slot(&self, row: usize) -> Option<usize> {
        self.lanes.binary_search_by_key(&row, |(r, _)| *r).ok()
    }

    /// The lane of a row, when this view carries it.
    pub fn get(&self, row: usize) -> Option<&LaneView<'a>> {
        self.slot(row).map(|k| &self.lanes[k].1)
    }

    /// [`Self::get`], to change.
    pub fn get_mut(&mut self, row: usize) -> Option<&mut LaneView<'a>> {
        self.slot(row).map(|k| &mut self.lanes[k].1)
    }

    /// Every lane carried, with its row, top to bottom.
    pub fn rows(&self) -> impl Iterator<Item = (usize, &LaneView<'a>)> {
        self.lanes.iter().map(|(r, l)| (*r, l))
    }

    /// Every lane carried, top to bottom.
    pub fn iter(&self) -> impl Iterator<Item = &LaneView<'a>> {
        self.lanes.iter().map(|(_, l)| l)
    }

    /// Every lane carried, to change.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut LaneView<'a>> {
        self.lanes.iter_mut().map(|(_, l)| l)
    }

    /// How many lanes were BUILT for this view: what the bounds tests hold
    /// to the visible count.
    pub fn len(&self) -> usize {
        self.lanes.len()
    }

    /// Whether the view carries no lane at all.
    pub fn is_empty(&self) -> bool {
        self.lanes.is_empty()
    }
}

impl<'a> std::ops::Index<usize> for Lanes<'a> {
    type Output = LaneView<'a>;
    /// Panics on a row this view does not carry, as the array did past its
    /// end. Tests index; the panel itself asks [`Lanes::get`].
    fn index(&self, row: usize) -> &LaneView<'a> {
        self.get(row).unwrap_or_else(|| panic!("row {row} is not in this view"))
    }
}

impl<'a> std::ops::IndexMut<usize> for Lanes<'a> {
    fn index_mut(&mut self, row: usize) -> &mut LaneView<'a> {
        self.get_mut(row).unwrap_or_else(|| panic!("row {row} is not in this view"))
    }
}

/// **A lane of the timeline, by what it is**: a track's row, or the master.
///
/// It replaces two numbers that meant "the master" by being one and two
/// past the channels (`MASTER_LANE` and `MASTER_COLUMN`), which moved every
/// time the desk grew and made every comparison against them a width test
/// in disguise. A row is a row and the master is the master, and so is a
/// stored automation lane ([`LaneId::auto_lane`], `automation::AutoLane`).
/// No number means the master any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LaneId {
    /// The lane of the track on this row.
    Row(usize),
    /// The master's row, above the tracks.
    Master,
}

impl LaneId {
    /// The track row, or `None` for the master.
    pub fn row(self) -> Option<usize> {
        match self {
            LaneId::Row(r) => Some(r),
            LaneId::Master => None,
        }
    }

    /// Whose curves this lane's are in the automation store: a row's, or
    /// the master's by name (`automation::AutoLane`).
    pub const fn auto_lane(self) -> crate::automation::AutoLane {
        match self {
            LaneId::Row(r) => crate::automation::AutoLane::Row(r),
            LaneId::Master => crate::automation::AutoLane::Master,
        }
    }

    /// The lane a stored automation lane is: the master's own, or a row.
    pub const fn from_auto_lane(lane: crate::automation::AutoLane) -> LaneId {
        match lane {
            crate::automation::AutoLane::Row(r) => LaneId::Row(r),
            crate::automation::AutoLane::Master => LaneId::Master,
        }
    }

    /// Which of the two alternating grounds this lane's automation rows
    /// stand on. The master's rows took lane thirty-two's, the even one,
    /// when they were addressed as that lane, and keep it.
    fn ground(self) -> usize {
        self.row().map_or(0, |r| r % 2)
    }
}

/// Everything the arrangement draws, pushed in.
pub struct ArrangeView<'a> {
    /// The lanes the layout will place: see [`Lanes`].
    pub lanes: Lanes<'a>,
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
    /// Whether the master's row is drawn. `Cmd`/`Ctrl+Shift+M` (plain
    /// `Cmd`/`Ctrl+M` is the metronome panel); shown by default.
    pub show_master_row: bool,
    /// The master rack's six short names — empty for an empty bay.
    ///
    /// Six, not three: the master carries two racks. Which of its slots lives
    /// in which is `desk_id::slot_key`'s business and not this panel's, so
    /// what arrives here is already in strip order, top to bottom.
    pub master_inserts: [&'a str; crate::recorder::INSERTS * 2],
    /// Which of the master's six are bypassed — `LaneView::bypassed` for the
    /// master row, in the same strip order as `master_inserts`.
    pub master_bypassed: [bool; crate::recorder::INSERTS * 2],
    /// The first lane shown — the arrangement has more lanes than fit, so a
    /// window of them shows and this says which — as its RANK among the
    /// drawn lanes. Clamped by the app.
    ///
    /// A rank and not a row: the desk's vacant rows are not drawn, and
    /// neither are a shut folder's children, so a scroll counted in rows
    /// spent a wheel notch per hidden row moving nothing, and a legacy
    /// desk's thirty empty rows let the tracks scroll clean away.
    pub lane_scroll: usize,
    /// How many lanes show at once — Shift+wheel over the headers zooms it.
    pub visible_lanes: usize,
    /// How many lanes the field draws, the trailing add's included: what
    /// the scroll can reach, in ranks. Never a tier.
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
    /// **A box being drawn over one automation row** to choose its nodes.
    /// See [`AutoMarquee`].
    pub auto_marquee: Option<AutoMarquee>,
    /// **The MASTER's own automation rows.**
    ///
    /// Separate from `lanes` because `lanes` is the sixteen CHANNELS and the
    /// master is not one of them — it is a bus, with its own row above them
    /// and its own strip on the desk. Everything else about these rows is a
    /// track's: the same `AutoView`, the same header, the same curve, the
    /// same gestures. They are addressed as [`LaneId::Master`] so that one
    /// `RowKind::Automation` still covers every automation row in the
    /// timeline and the drawing, hit-testing and editing do not fork.
    pub master_automation: Vec<AutoView<'a>>,
    /// A MIDI item being drawn by cmd-drag right now: lane, from, to —
    /// ghosted until the button comes up and the document takes it.
    pub pending: Option<(usize, f64, f64)>,
    /// The tempo and meter map get a lane of their own, above the tracks.
    pub show_tempo_lane: bool,
    /// Which tempo point a hand is on, if any. Drawn a size larger, exactly
    /// as an automation point under the pointer is.
    pub tempo_hot: Option<usize>,
    /// A signature chip being typed into, and what is in the field so far.
    pub meter_edit: Option<(u32, &'a str)>,
    /// **The song's named places**, in document order — see
    /// `arrangement::ArrMarker`. Drawn as flags in the ruler's top strip,
    /// beside the signature chips, because both are facts about the song at
    /// a point in it and a second row would take height from the tracks.
    pub markers: Vec<MarkerView>,
    /// A flag being typed into, by index, and what is in the field so far —
    /// `meter_edit`'s twin, and the same shape on purpose.
    pub marker_edit: Option<(usize, &'a str)>,
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
    /// `recorder_panel::selection_rim`. Owned rows, found from the tracks
    /// the app's selection names as the view is built.
    pub selected_tracks: Vec<usize>,
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
    /// Its own variant rather than an `HdrBay` on the master's number, because
    /// every `HdrBay` handler reads its first field as a strip index — and the
    /// master's old column number was a perfectly valid one belonging to
    /// somebody else, so overloading it would have silently edited another
    /// rack's plugins. The slot is in STRIP order; `desk_id::slot_key` turns
    /// it into the rack it actually lives in.
    BusBay(Column, usize),
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
    AutoPoint(LaneId, usize, usize),
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
    AutoLane(LaneId, usize, f64, f64),
    /// The sub-lane's own header: which parameter it shows.
    /// One automation ROW's header — `(lane, row)`.
    /// **Ø** — invert this track's polarity.
    Phase(usize),
    AutoParamPick(LaneId, usize),
    /// **×** on an automation row: stop showing this lane.
    AutoClose(LaneId, usize),
    /// The record dot on an automation row: arm it to write.
    AutoArm(LaneId, usize),
    /// **B**: bypass this curve without deleting it.
    AutoBypass(LaneId, usize),
    /// The dial on an automation row — sets the control itself.
    AutoDial(LaneId, usize),
    /// The header's fold arrow: show or hide this lane's automation.
    /// The **A** button on a track's header: open the automation menu.
    AutoMenu(LaneId),
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
    /// A marker flag in the ruler's strip, by index into `markers`.
    Flag(usize),
    /// A time-signature chip on the ruler, by the bar it starts at. A press
    /// opens it for editing.
    MeterChip(u32),
    /// The ruler's chip strip, where there is no chip: `bar`. A press adds a
    /// signature change at that bar.
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
    /// than a second road to the same number.
    BusGain(Column),
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

/// The ruler's height in points. Short — it is a scale, not a band.
///
/// **Raised from `(0.06, 16, 24)` when the strip inside it grew words.** The
/// signature chips and the marker flags share the ruler's top third, and at
/// the old height that third was eight points, which put their labels at a
/// six-point font: present, hittable, and not readable. The owner asked to
/// "heighten the loop/timereadout strip" for exactly that reason. The ruler
/// has to grow with it or the bar numbers and the clock below lose the room
/// the strip takes.
const RULER_H: (f32, f32, f32) = (0.115, 18.0, 52.0);

/// **The ruler strip is two rows: names on top, signatures under them.**
///
/// It was one row carrying both, and a marker that shared a bar with a
/// signature change was printed as a single label — "Chorus (3/4)". The
/// owner asked for that undone: "no more 'name (timesig)' just 'name' and
/// time sig is seperate and when they are both on one spot, the time sig
/// will sit below the label."
///
/// Two fixed rows rather than a rule that moves a chip down only when
/// something is over it. A signature that lived on the upper row until a
/// marker arrived and then jumped to the lower one is a thing that moves
/// under the hand for reasons the hand did not cause; a reader who has
/// learned "names up here, signatures down there" has learned it once and
/// it is true on every song. The stacking the owner asked for then falls
/// out of the layout rather than out of a special case.
///
/// The name row is the taller of the two: it carries WORDS, where the other
/// carries three characters on a chip.
fn flag_row(strip: Rect) -> Rect {
    Rect::from_min_max(
        strip.min,
        Pos2::new(strip.right(), strip.top() + strip.height() * 0.55),
    )
}

/// The signature row: the strip's lower band. See [`flag_row`].
fn sig_row(strip: Rect) -> Rect {
    Rect::from_min_max(
        Pos2::new(strip.left(), strip.top() + strip.height() * 0.55),
        strip.max,
    )
}

/// **The bar numbers' row: the band under the strip, upper half.**
///
/// The ruler carries four rows now — marker names, signatures, bar numbers,
/// the clock — and until this existed the bottom two were positioned by
/// hand from opposite edges: the number from the strip's foot downward, the
/// clock from the ruler's foot upward. Whatever was left in the middle was
/// whatever was left, and once the strip grew to carry two rows of its own
/// there was nothing left: the two overlapped by nearly nine points on
/// every theme. The owner: "the timecode text has the beats overlapping the
/// seconds. they need a tiny bit of breathing room."
///
/// Bands rather than edges, so the gap is a fact of the layout instead of
/// an accident of two fonts. The numbers get the larger share: bars answer
/// "where in the music", which is the ruler's first job.
fn number_row(ruler: Rect, strip: Rect) -> Rect {
    let below = Rect::from_min_max(Pos2::new(ruler.left(), strip.bottom()), ruler.max);
    Rect::from_min_max(
        below.min,
        Pos2::new(below.right(), below.top() + below.height() * 0.54),
    )
}

/// The clock's row: the ruler's foot. See [`number_row`].
fn clock_row(ruler: Rect, strip: Rect) -> Rect {
    let below = Rect::from_min_max(Pos2::new(ruler.left(), strip.bottom()), ruler.max);
    Rect::from_min_max(
        Pos2::new(below.left(), below.top() + below.height() * 0.54),
        below.max,
    )
}

/// The size a row's words are set at, from that ROW's height.
///
/// **One definition**, because the painter lays the chips and flags out at
/// this size and the hit-test re-derives their widths from it: the two
/// drifting apart by a point is a label you can read and cannot press. It
/// was two copies of `(h * 0.78).clamp(6.0, 10.0)` until the strip grew.
fn strip_font_h(row: Rect) -> f32 {
    (row.height() * 0.72).clamp(7.5, 12.0)
}

/// How much of its row a chip or a flag actually fills.
///
/// Not all of it: the pole has to be seen leaving the box on its way down,
/// which is the whole job of a flag over a ruler.
fn strip_box_h(row: Rect) -> f32 {
    (row.height() * 0.82).max(8.0).min(row.height())
}

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
    /// An automation curve under the track it belongs to. The first field is
    /// whose (a track's row, or the master); the second is WHICH of its open
    /// rows, counting down from the track — a lane can show several
    /// parameters at once now.
    Automation(LaneId, usize),
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
    /// `Cmd`/`Ctrl+Shift+M` hides it (plain `Cmd`/`Ctrl+M` is the metronome
    /// panel). Shown by default, because a control nobody can find is a
    /// control nobody has.
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

/// The `AutoView` for a row, wherever it lives.
///
/// One lookup, because `lanes` holds the channels and the master's rows sit
/// beside it — and a caller that reached for `lanes[lane]` directly would
/// silently find nothing for the master and draw an empty row. The master
/// is [`LaneId::Master`], one `RowKind::Automation` still describes every
/// automation row on the timeline, and nothing forks.
pub fn auto_view_of<'a>(
    view: &'a ArrangeView<'a>,
    lane: LaneId,
    slot: usize,
) -> Option<&'a AutoView<'a>> {
    match lane {
        LaneId::Master => view.master_automation.get(slot),
        // The CHANNELS' own table. Deliberately spelled out rather than
        // routed back through this function — that is infinite recursion,
        // and was.
        LaneId::Row(row) => view.lanes.get(row).and_then(|l| l.automation.get(slot)),
    }
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
    /// The top sliver of the ruler: the time-signature chips and the
    /// MARKER FLAGS, which share it.
    ///
    /// Carved OUT of the ruler rather than added above it: the ruler already
    /// has a bar-number row and a clock row, and a third band would push the
    /// lanes down to say something that belongs beside the bar numbers. The
    /// strip takes the top third, where nothing was drawn.
    ///
    /// **Always present now.** It used to be `NOTHING` on a song in one
    /// signature — nearly all of them — so the bar numbers could keep the
    /// whole ruler. Markers live here too and a song has none until somebody
    /// makes one, so a strip that appears only once there is something in it
    /// is a strip nobody can right-click to put the first thing in. It costs
    /// nothing to reserve: the top third was empty space either way.
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
    pub lane_rows: ByRow<Rect>,
    pub header_rows: ByRow<HeaderRow>,
    /// The master's header cells. `NONE` when the row is hidden.
    pub master_row: BusRow,
    /// **The lane scrollbar** down the lanes' right edge, `LaneBar::NONE`
    /// when every lane fits. See [`LaneBar`].
    pub lane_bar: LaneBar,
}

/// **A thin scrollbar down the lanes: where the window stands among the
/// tracks, and a handle to move it.**
///
/// The wheel over the headers was the only way down a desk taller than the
/// window, and nothing said how many lanes were below. A desk that grows by
/// appending needs both (docs/design/unlimited-tracks.md, section 10). The
/// track is every row of the desk, the thumb the rows on screen; it stands
/// right of the lanes, where every timeline keeps it, and a press on it is
/// the bar's (see [`lane_bar_grab`]).
///
/// **Beside the lanes, never on them.** It was laid inside the lanes'
/// right edge, and the press the bar claims first took the last few points
/// of every lane: a clip running off the window's right edge is clamped
/// there, so its right trim zone and fade-out corner sat wholly under the
/// bar, and a press meant to trim scrolled the lanes instead. The bar has
/// its own gutter now ([`LANE_GUTTER`]), taken out of the lanes area while
/// the bar shows, so nothing a lane draws or answers lies under it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LaneBar {
    /// Every row of the desk, laid down the lanes' right edge.
    pub track: Rect,
    /// The rows on screen, where they stand.
    pub thumb: Rect,
    /// The scroll, in rows, that puts the thumb at the bottom.
    pub max_rows: usize,
}

impl LaneBar {
    /// No bar: every lane fits.
    pub const NONE: Self = Self { track: Rect::NOTHING, thumb: Rect::NOTHING, max_rows: 0 };
}

/// The bar's width and its shortest thumb.
const LANE_BAR_W: f32 = 5.0;
const LANE_BAR_MIN: f32 = 18.0;
/// **The bar's gutter**: the strip right of the lanes it stands in, one
/// point clear of the lanes and of the window's edge.
pub const LANE_GUTTER: f32 = LANE_BAR_W + 2.0;

/// **Whether the lane scrollbar shows**: the desk is taller than the window
/// of lanes. Kept beside [`lane_window_start`], whose clamps it reads, so
/// the frame (which reserves the gutter) and the layout (which draws the
/// bar) cannot disagree.
pub fn lane_bar_shown(visible_lanes: usize, active: usize) -> bool {
    let active = active.max(1);
    let (_, visible) = lane_window_start(0, visible_lanes, active);
    visible < active
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

/// **Something per placed row, looked up by row.** Only the rows the layout
/// placed are in it, so a walk is O(visible); a row it did not place answers
/// the same "not here" its slot in the old full-width array held (`NOTHING`,
/// `HeaderRow::NONE`) when indexed, and `None` from [`ByRow::get`].
#[derive(Debug, Clone)]
pub struct ByRow<T>(Vec<(usize, T)>);

impl<T> Default for ByRow<T> {
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl<T> ByRow<T> {
    /// What the layout placed for a row.
    pub fn get(&self, row: usize) -> Option<&T> {
        self.0
            .binary_search_by_key(&row, |(r, _)| *r)
            .ok()
            .map(|k| &self.0[k].1)
    }

    /// Every placed row, top to bottom, with what was placed for it.
    pub fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.0.iter().map(|(r, t)| (*r, t))
    }

    /// The first placed row, top down, whose entry answers `pred`.
    pub fn find_row(&self, mut pred: impl FnMut(&T) -> bool) -> Option<usize> {
        self.0.iter().find(|(_, t)| pred(t)).map(|(r, _)| *r)
    }

    /// How many rows were placed.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether no row was placed.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

static NO_RECT: Rect = Rect::NOTHING;
static NO_HEADER: HeaderRow = HeaderRow::NONE;

impl std::ops::Index<usize> for ByRow<Rect> {
    type Output = Rect;
    fn index(&self, row: usize) -> &Rect {
        self.get(row).unwrap_or(&NO_RECT)
    }
}

impl std::ops::Index<usize> for ByRow<HeaderRow> {
    type Output = HeaderRow;
    fn index(&self, row: usize) -> &HeaderRow {
        self.get(row).unwrap_or(&NO_HEADER)
    }
}

/// **The rows a timeline scrolled to `lane_scroll` places**: the first row
/// it lays out, and how many TRACK rows it can show at most.
///
/// The layout's own arithmetic, shared with the app so the window of lanes
/// it builds is the window the layout reads: the scroll is a ROW and is
/// clamped so the last `visible` rows of the desk can always fill the view.
/// Rect-free, because the rows that do not fit are dropped by height in the
/// layout and the window only needs to cover the most that could fit.
///
/// **The count is the one asked for, never the lane count** (design section
/// 10: a lane's height follows the count). A desk shorter than the window
/// is laid out at the height the count gives each lane, its stack ending
/// where its lanes do; it used to be clamped to the lanes there were, which
/// never showed while every desk drew thirty-two of them and, once vacant
/// rows were hidden, made a fresh desk's two lanes and its add a third of
/// the timeline each.
pub fn lane_window_start(lane_scroll: usize, visible_lanes: usize, active: usize) -> (usize, usize) {
    let active = active.max(1);
    let visible = visible_lanes.clamp(1, VISIBLE_MAX);
    (lane_scroll.min(active.saturating_sub(visible)), visible)
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

/// **The frame of the panel: what follows from the rect and the header
/// width, and from nothing else.**
///
/// `Layout::new` needs the whole view to stack the rows — which lanes are
/// hidden, how many automation rows are open, whether the master shows —
/// but the header column, the ruler, the chip strip, the lanes area and the
/// grip fall out of the rect the moment the header width is known. Two
/// callers ask every frame and want only those: the grip's hover check (is
/// the pointer on the seam?) and the host's wave-view question (how wide are
/// the lanes?). Each was building every clip view, the whole `ArrangeView`
/// and the full row stack to read one rectangle. So the frame is its own
/// step, and `Layout::new` begins with it — the same expressions, in the
/// same order, so the two cannot disagree about where the lanes begin.
#[derive(Debug, Clone, Copy)]
pub struct Frame {
    pub header: Rect,
    pub ruler: Rect,
    pub meter_strip: Rect,
    pub lanes_area: Rect,
    pub grip: Rect,
    /// The resolved header width and ruler height, kept for the corner cell
    /// `Layout::new` still lays out itself.
    header_w: f32,
    ruler_h: f32,
}

impl Layout {
    /// See [`Frame`]. `header_w` is `ArrangeView::header_w`: a dragged width,
    /// or `None` for the automatic share. `lane_bar` is [`lane_bar_shown`]
    /// for the view: the bar's gutter comes out of the lanes, the ruler and
    /// the chip strip with them, so a time read off the ruler and one read
    /// off a lane are the same span.
    pub fn frame(rect: Rect, header_w: Option<f32>, lane_bar: bool) -> Frame {
        if !rect.is_positive() {
            return Frame {
                header: Rect::NOTHING,
                ruler: Rect::NOTHING,
                meter_strip: Rect::NOTHING,
                lanes_area: Rect::NOTHING,
                grip: Rect::NOTHING,
                header_w: 0.0,
                ruler_h: 0.0,
            };
        }
        let (share, lo, _hi) = HEADER_W;
        // Chained rather than clamp(): a rect narrower than the floor would
        // hand clamp a min above its max, which is a panic in a painter.
        // A dragged width wins over the automatic share, inside the same
        // sanity bounds.
        //
        // **The ceiling is HALF THE VIEW, and nothing tighter.** It used to be
        // four hundred points as well, which is a number two things quietly
        // disagreed with. The grip clamps a drag to `[150, width/2]`, so a
        // column dragged past four hundred was stored at one width and drawn
        // at another. And the DEFAULT is tied to the band's camera/timecode
        // seam — one column across two surfaces, the owner's rule — so on a
        // maximised window, where that seam sits well past four hundred, the
        // tie simply stopped: "make the default track length at maximized
        // tied to the separator line between camera and timecode". It was
        // tied; it was being clipped. `the_default_track_header_lines_up_with_
        // the_camera_seam` used to dodge this by choosing two window sizes
        // under the ceiling, and now asserts a maximised one as well.
        let header_w = header_w
            .unwrap_or(rect.width() * share)
            .max(lo)
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
        // Never left of the field's start: a window too narrow for the
        // gutter keeps an empty field, not an inverted one.
        let field_r = if lane_bar { (rect.right() - LANE_GUTTER).max(field_x) } else { rect.right() };
        let ruler = Rect::from_min_max(
            Pos2::new(field_x, rect.top()),
            Pos2::new(field_r, rect.top() + ruler_h),
        );
        // Always: the flags and the chips share it, and a song has no
        // markers until somebody right-clicks the strip to make one.
        let meter_strip = {
            // **TWO rows now, so the band is bigger again.** Markers on
            // top, signatures under them — see [`flag_row`]. It was one row
            // holding both, with a co-located pair printed as one label,
            // and the owner asked for them separated. Two rows need the
            // height of two rows; the ruler grew with it so the bar numbers
            // and the clock keep the room they got.
            // **No hard floor**, only a share and a ceiling. A floor of 24
            // points was 19% of a 150-point arrangement's whole height and
            // pushed the lanes' own controls off the bottom — a guard
            // caught it. The band shrinks with the ruler instead; at a size
            // where the strip cannot carry two readable rows, nothing else
            // on this view is readable either.
            let h = (ruler_h * 0.50).clamp(8.0, 28.0);
            Rect::from_min_max(ruler.min, Pos2::new(ruler.right(), ruler.top() + h))
        };
        let lanes_area = Rect::from_min_max(
            Pos2::new(field_x, rect.top() + ruler_h),
            Pos2::new(field_r, rect.bottom()),
        );
        let grip = Rect::from_min_max(
            Pos2::new(rect.left() + header_w, rect.top()),
            Pos2::new(rect.left() + header_w + GRIP_W, rect.bottom()),
        );
        Frame { header, ruler, meter_strip, lanes_area, grip, header_w, ruler_h }
    }

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
                lane_rows: ByRow::default(),
                header_rows: ByRow::default(),
                master_row: BusRow::NONE,
                lane_bar: LaneBar::NONE,
            };
        }
        // The frame first — see [`Frame`] for why it is its own step.
        let Frame { header, ruler, meter_strip, lanes_area, grip, header_w, ruler_h } =
            Self::frame(rect, view.header_w, lane_bar_shown(view.visible_lanes, view.active));
        // **Eight lanes show; the rest scroll.** The desk grew past what
        // fits at a legible height, so the height is fixed to `VISIBLE_LANES`
        // and `view.lane_scroll` chooses which window shows. A lane outside
        // the window is `NOTHING` — not drawn, not hit, and clips on it
        // resolve to nothing through `clip_rect`'s `is_positive` filter.
        let active = view.active.max(1);
        let (first, visible) = lane_window_start(view.lane_scroll, view.visible_lanes, active);
        // The lanes this layout can place: the drawn rows from `first` down,
        // never past the desk. The view carries a window that starts here
        // (or a whole desk, for a picture of the song); a lane inside a SHUT
        // folder is not laid out at all: no row, so nothing is drawn,
        // nothing is hit, and its clips resolve to nothing through
        // `clip_rect`'s own `is_positive` filter — the same road a
        // scrolled-off lane already takes.
        // By RANK (`ArrangeView::lane_scroll`): the drawn lanes the view
        // carries, from the one at rank `first`.
        let skip = first.saturating_sub(view.lanes.first);
        let placeable = || view.lanes.rows().filter(|(_, l)| !l.hidden).skip(skip);
        // **The bus rows are ADDED to the stack, not taken out of it.**
        //
        // The master and the click are half a track each — one track's
        // height between them — and dividing the area by `visible` alone
        // would have spent that height on them, so a window set to show one
        // lane showed none at all. `visible` still means TRACKS; the pair
        // costs every track a slice of its height instead, which is the same
        // bargain an open automation row already makes.
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
            (placeable()
                .take(shown)
                .map(|(_, l)| l.automation.len() as f32)
                .sum::<f32>()
                + master)
                * AUTO_LANE_SHARE
        };
        // Settles: evicting a track can only REMOVE automation from the
        // count, which can only let tracks back in, so `shown` walks down
        // and stops. Bounded by the window regardless.
        let mut shown = visible;
        let mut auto_share = auto_of(shown);
        for _ in 0..=visible {
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
                // The tempo shares the MASTER's lane now, so it buys no
                // height of its own — see the row push below.
                + 0.0
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
        let mut rows: Vec<Row> = Vec::with_capacity(2 * visible + 4);
        let mut y = lanes_area.top();
        let push = |kind: RowKind, h: f32, y: &mut f32, rows: &mut Vec<Row>| {
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
        // **The master and the click, above the channels.** Half a track
        // each, so the two together take exactly one track's height and the
        // pair reads as one band rather than as two short tracks.
        if view.show_master_row {
            push(RowKind::Master, lane_h * MASTER_ROW_SHARE, &mut y, &mut rows);
            // **The tempo draws in the MASTER's lane, not in a row of its
            // own.**
            //
            // It used to take half a track above the master, and the row was
            // wrong twice over: it needed no header, so the desk grew an
            // unlabelled empty strip down the left, and it sat above the very
            // thing it controls. The owner: "clicking the tempo arm currently
            // opens its own lane above the master -- this is tacky because it
            // controls the master, and doesn't have its own label (because it
            // shouldn't) so theres just an empty strip. INSTEAD: clicking
            // tempo arm reveals the tempo automation, BUT -- it uses the empty
            // lane directly to the right of the master instead."
            //
            // The master's lane IS empty — a master carries no clips — so the
            // curve moves in and costs no height at all. A row rather than a
            // special case, and the SAME row kind, so every line that draws
            // or hits the tempo goes on finding it by `RowKind::Tempo` and
            // reading `row.lane`. Its header is deliberately `NOTHING`: there
            // is no strip to label because there is no strip.
            if view.show_tempo_lane {
                if let Some(master) = rows.iter().rev().find(|r| r.kind == RowKind::Master)
                {
                    let lane = master.lane;
                    rows.push(Row { kind: RowKind::Tempo, lane, header: Rect::NOTHING });
                }
            }
            // **Its automation hangs under it**, exactly as a track's does,
            // and by the same row kind — see `LaneId::Master`.
            for slot in 0..view.master_automation.len() {
                push(
                    RowKind::Automation(LaneId::Master, slot),
                    lane_h * AUTO_LANE_SHARE,
                    &mut y,
                    &mut rows,
                );
            }
        }
        // **Only as many tracks as could ever fit**, plus one: the rows past
        // the window's height were pushed as `NOTHING` and never drawn, so a
        // desk of any length lays out in O(visible). `shown` cannot exceed
        // `visible`, and the one past it is the row that proves the stack
        // ends flush.
        let mut lane_rows = Vec::with_capacity(visible + 1);
        let mut header_rows = Vec::with_capacity(visible + 1);
        for (i, lane) in placeable().take(visible + 1) {
            let at = rows.len();
            push(RowKind::Track(i), lane_h, &mut y, &mut rows);
            // **A rank lookup, not a search**: the row just pushed is this
            // lane's, so its rectangles are read where they were written
            // rather than found again by a walk over every row.
            let row = rows[at];
            lane_rows.push((i, row.lane));
            header_rows.push((
                i,
                if row.header.is_positive() {
                    let mut h = Self::header_row(
                        row.header,
                        lane.used,
                        if lane.in_folder { FOLDER_INDENT } else { 0.0 },
                        lane.is_folder,
                        lane.plus,
                    );
                    // **A bus has no input to bind, nothing to arm and no
                    // MIDI for the theory view** (design 9): the three seats
                    // stand empty, as a folder's arm and note do.
                    if lane.kind == Some(ChannelKind::Bus) {
                        h.input = Rect::NOTHING;
                        h.arm = Rect::NOTHING;
                        h.theory = Rect::NOTHING;
                    }
                    h
                } else {
                    HeaderRow::NONE
                },
            ));
            // One row per parameter this lane is showing, in the order the
            // view lists them — which is the order the menu offers them, so
            // the rows and the menu read the same way down.
            for slot in 0..lane.automation.len() {
                push(
                    RowKind::Automation(LaneId::Row(i), slot),
                    lane_h * AUTO_LANE_SHARE,
                    &mut y,
                    &mut rows,
                );
            }
        }
        let master_row = rows
            .iter()
            .find(|r| r.kind == RowKind::Master)
            .filter(|r| r.header.is_positive())
            .map_or(BusRow::NONE, |r| BusRow::of(r.header));
        let lane_rows = ByRow(lane_rows);
        let header_rows = ByRow(header_rows);
        // **The bar, when the desk is taller than the window**: the thumb is
        // the window's share of the rows, at the scroll's share of its range.
        let max_rows = active.saturating_sub(visible);
        let lane_bar = if max_rows > 0 && lanes_area.is_positive() {
            // In the gutter the frame left for it, one point off the lanes.
            let track = Rect::from_min_max(
                Pos2::new(lanes_area.right() + 1.0, lanes_area.top()),
                Pos2::new(lanes_area.right() + 1.0 + LANE_BAR_W, lanes_area.bottom()),
            );
            let h = (track.height() * visible as f32 / active as f32)
                .clamp(LANE_BAR_MIN.min(track.height()), track.height());
            let y = track.top() + (track.height() - h) * (first as f32 / max_rows as f32);
            LaneBar {
                track,
                thumb: Rect::from_min_size(Pos2::new(track.left(), y), Vec2::new(LANE_BAR_W, h)),
                max_rows,
            }
        } else {
            LaneBar::NONE
        };
        // **The toolbar, in the corner cell** — the room above the track
        // list, before the ruler's first tick. Four small controls: the
        // click, the magnet, the tempo lane's switch, and the grid box the
        // wheel turns.
        let corner = Rect::from_min_max(
            rect.min,
            Pos2::new(rect.left() + header_w, rect.top() + ruler_h),
        )
        .shrink2(Vec2::new(3.0, 2.0));
        // **All four the same size, and sized to the room they now have.**
        //
        // They were three squares capped at twenty points and a grid box
        // 2.4 times as wide as the rest — four controls in three sizes,
        // which read as three unrelated things and a mistake. The owner:
        // "make the 4 buttons (met, grid, tempo arm, and frequency) equal
        // sized and fit their new area". The corner grew when the ruler did,
        // so a fixed twenty-point cap now leaves half of it empty.
        //
        // The side is whichever runs out first: the corner's HEIGHT, or the
        // width five and a half cells need — the four buttons, their gaps,
        // and the navigator's own minimum of one and a half. Solving for the
        // navigator here rather than letting it take what is left is what
        // stops a narrow header from growing four fat buttons and leaving it
        // nothing, which is how it would disappear on the smallest header
        // the seam allows.
        const TOOL_GAP: f32 = 3.0;
        // **Back to the height they always had.** They were capped at twenty
        // points before the ruler grew; letting them fill the taller corner
        // made a row of buttons that dominated it. The owner: "shrink the
        // scanner/met tempo arm and grid section to be the original height —
        // a half strip (i think) it looks better that way." Twenty IS about
        // half the corner now, so the two readings agree.
        const TOOL_MAX: f32 = 20.0;
        let side = corner
            .height()
            .min((corner.width() - TOOL_GAP * 4.0) / 5.5)
            .min(corner.height() * 0.5)
            .clamp(1.0, TOOL_MAX);
        // Centred in the corner's height: the cells are square and the
        // corner is taller than they are, and a row of buttons pinned to the
        // top of a band with room under it reads as having slipped.
        let top = corner.top() + ((corner.height() - side) * 0.5).max(0.0);
        let cell = |k: f32| {
            Rect::from_min_size(
                Pos2::new(corner.left() + k * (side + TOOL_GAP), top),
                Vec2::splat(side),
            )
        };
        let (tool_met, tool_snap, tool_tempo, tool_grid) =
            (cell(0.0), cell(1.0), cell(2.0), cell(3.0));
        // The NAVIGATOR takes whatever the corner has left — longer than
        // the gridbox, as asked: a rectangle inside a rectangle, and a
        // hand on it steers the whole view.
        let tool_nav = Rect::from_min_max(
            Pos2::new(tool_grid.right() + TOOL_GAP, top),
            Pos2::new(corner.right(), top + side),
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
            grip,
            rows,
            lane_rows,
            header_rows,
            master_row,
            lane_bar,
        }
    }

    /// The cells across one header, REAPER's row at Tangent's density:
    /// number, arm dot, name, gain knob, slim meter, M/S.
    fn header_row(row: Rect, used: bool, indent: f32, folder: bool, plus: crate::mixer_panel::Plus) -> HeaderRow {
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
            // **The trailing add is the plus and nothing else**: it is no
            // row yet, so it has no input to bind and no slot to select.
            if plus != crate::mixer_panel::Plus::Row {
                return HeaderRow { panel: row, add, ..HeaderRow::NONE };
            }
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
        // **How much of its band a chip actually fills.**
        //
        // The bays, the input button and ROUTING took the whole of their
        // band's height, so the column read as a stack of slabs with hairline
        // gaps rather than as a rack of chips. The owner asked for them
        // "slightly less in height, but keep the length" — the width is what
        // makes a word readable and the height is only what makes the box a
        // box. Centred in the band, so the air is shared top and bottom.
        // 0.88, up from 0.82: the first cut of this took too much off and the
        // owner said so — "it was overcorrected".
        const CHIP_FILL: f32 = 0.88;
        let chip_band = |band: Rect| {
            if !band.is_positive() {
                return Rect::NOTHING;
            }
            let hh = band.height() * CHIP_FILL;
            Rect::from_min_max(
                Pos2::new(band.left(), band.center().y - hh * 0.5),
                Pos2::new(band.right(), band.center().y + hh * 0.5),
            )
        };
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
        // **The switches keep the right end, STACKED**; the meter stands
        // inside them.
        //
        // Side by side they cost the row two switches' width out of the
        // middle, which is width the dials and the name column were
        // competing for. Stacked they cost one — the owner's "make mute and
        // solo atop one another" — and they read as the pair they are, the
        // way the note and the wrench beside the arm already do. Mute above
        // solo: it is the one you reach for.
        let sw = (h * 0.44).clamp(11.0, 17.0);
        let gap = 1.5;
        let stack_of_two = |top: f32| Rect::from_min_size(Pos2::new(r.right() - sw, top), Vec2::splat(sw));
        let (mute, solo) = if r.height() >= sw * 2.0 + gap {
            let top = r.center().y - sw - gap * 0.5;
            (stack_of_two(top), stack_of_two(top + sw + gap))
        } else {
            // Too short to stack: side by side, the way they were, rather
            // than one of them shed. A row this short has no dials to
            // reclaim the width for anyway.
            let solo = Rect::from_min_size(
                Pos2::new(r.right() - sw, r.center().y - sw * 0.5),
                Vec2::splat(sw),
            );
            (
                Rect::from_min_size(
                    Pos2::new(solo.left() - 2.0 - sw, r.center().y - sw * 0.5),
                    Vec2::splat(sw),
                ),
                solo,
            )
        };
        // **Polarity, in the row's bottom-right corner** — the owner's
        // placement. It used to sit above the mute, which put a third switch
        // in the M/S column and made the three read as one set of three; it
        // is not one of that set. Two thirds their size, because it is a
        // thing you set once and they are things you reach for while playing.
        //
        // Shed when the row is too short for it to sit clear of the switches
        // above it, rather than drawn on top of them.
        let phase = {
            let d = (sw * 0.62).clamp(8.0, 12.0);
            // Inside the corner rather than on it: the plate this stands on
            // is the row shrunk by two, so a switch flush with `r` overhangs
            // its own panel.
            const CORNER: f32 = 2.0;
            let top = r.bottom() - CORNER - d;
            if d >= 8.0 && top >= solo.bottom() + 2.0 {
                Rect::from_min_size(Pos2::new(r.right() - CORNER - d, top), Vec2::splat(d))
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
        // **The same square the mute and the solo wear** — the owner's word.
        // They were a third smaller, which said "these two are lesser
        // controls"; they are not, and a note and a wrench need the room a
        // letter does not.
        let (theory_seat, automation) = {
            let d = sw;
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
        // **A FOLDER has no note.** It carries no MIDI of its own, so there
        // is nothing for it to send to the theory view - the desk has said
        // so since the T left the switch row (`mixer_panel`'s `has_arm`),
        // and the timeline went on drawing a glyph for a routing that could
        // never be on. The SEAT stands where the note would be, the way the
        // arm's does on this same row: the name column anchors off it, and a
        // column that moves on folders is a header that does not line up
        // with the tracks under it.
        let theory = if folder { Rect::NOTHING } else { theory_seat };
        // ── **The three horizontal bands, decided before anything sits in
        // them.** ────────────────────────────────────────────────────────
        //
        // They used to be worked out at the bottom of this function, after
        // the dials, because only the name column used them. Both columns
        // use them now — the racks stack in one and the dial stack has to
        // END where the second rack begins — so they are settled first. They
        // depend on the row's HEIGHT alone; only the columns' widths depend
        // on anything below.
        //
        // **What a short header sheds, and in what order.** The lane count is
        // adjustable (Shift+wheel), so a header can be as tall as a third of
        // the view or as short as a strip. Three bands hold the name, the
        // first rack and the second; two drop the second rack; one keeps the
        // name alone. The owner's rule — never the NAME — is unchanged; what
        // changed is what the middle band holds, because the INPUT moved up
        // beside the name and left its band to the second rack.
        const ROW_MIN: f32 = 15.0;
        let band_top = r.top();
        let band_h = (r.bottom() - 1.0 - band_top).max(0.0);
        let band_y = |a: f32, b: f32| (band_top + band_h * a, band_top + band_h * b);
        let none_y = (f32::INFINITY, f32::NEG_INFINITY);
        // **The two racks are the SAME height**, and they were not: 0.34..0.63
        // against 0.67..1.00 is a top rack an eighth thinner than the bottom
        // one, which is what the owner saw - "it feels like the top one is
        // slightly thinner". Six bays of one size is the whole point of the
        // band; the fractions split what is left under the name evenly and
        // keep the same gap between them that the name already has above.
        let [top_y, rack_y, rack2_y] = if band_h >= 3.0 * ROW_MIN {
            [band_y(0.0, 0.30), band_y(0.34, 0.655), band_y(0.685, 1.0)]
        } else if band_h >= 2.0 * ROW_MIN {
            [band_y(0.0, 0.46), band_y(0.50, 1.0), none_y]
        } else {
            [band_y(0.0, 1.0), none_y, none_y]
        };
        // **The dial column is a STACK, and it ends above the second rack.**
        //
        // The readout reads above the knob and the label under it — the dial
        // standard — and both used to be whatever was left after the knob had
        // taken a fixed share of the whole row. That gave the label a nine
        // point band by way of a constant, `DIAL_LABEL_H`, and a nine point
        // band carries an eight point word however the size is worked out:
        // the owner asked for the knob text to be bigger, got the arithmetic
        // fixed underneath it, and saw no change at all, because the constant
        // was the binding one. There is no constant now. The stack is given
        // the room above the ROUTING button, the knob takes its share of
        // that, and the readout and the label split what is left — so a
        // taller lane makes both of them bigger, which is what a taller lane
        // is for.
        let stack_top = r.top() + 1.0;
        let stack_bot = if rack2_y.1 > rack2_y.0 { rack2_y.0 - 2.0 } else { r.bottom() - 1.0 };
        let stack = (stack_bot - stack_top).max(0.0);
        // Twelve points held back for the two figures, and never so much that
        // the knob stops being a knob.
        //
        // **Eighteen is still the ceiling**, unchanged, and that is a
        // HORIZONTAL fact rather than a vertical one: three dials and their
        // gaps come out of the same width the name column is cut from, so a
        // knob allowed to grow on a tall row takes the column with it —
        // measured, a 22-point ceiling pushed `dials_fit` false on a tall
        // window and the whole set vanished. It reaches the ceiling on a tall
        // row and the spare goes to the two figures, which is what the owner
        // asked for.
        // **A shade smaller than they were** — the owner's eye, twice: the
        // ceiling went up so the figure under the knob could reach the row's
        // size, and then the knobs themselves were a touch large for the
        // panel. The figure keeps its room because the READOUT cell is a
        // share of the dial column rather than the knob's own width, and the
        // column did not move.
        let kd = (stack * 0.41).clamp(11.0, 27.0).min((stack - 12.0).max(9.0));
        // **And never wider than the room the NAME column can spare.**
        //
        // The ceiling used to be eighteen, and eighteen was a horizontal fact
        // wearing a vertical one's clothes: three dials and their gaps come
        // out of the same width the name column is cut from, so a knob
        // allowed to grow on a tall row took the column with it and the whole
        // set SHED — a cliff, at a size where a slightly smaller dial would
        // have been fine.
        //
        // Named properly, the two rules compose: the height says how big a
        // knob this row would like, the width says how big it may be, and the
        // set only sheds when even the floor will not fit. A wide header —
        // and the default is tied to the band's camera seam, so on a big
        // display it IS wide — now spends that width on bigger dials and, with
        // them, on figures under the dials somebody can read. That is most of
        // "the volume readout text needs to be bigger": the figure is capped
        // by the cell, and the cell is the dial.
        const COL_MIN: f32 = 40.0;
        let dial_room = meter.left() - 5.0 - (arm_seat.right() + 4.0 + COL_MIN + 6.0);
        let kd = kd.min(((dial_room - 8.0) / 3.0).max(0.0));
        let spare = (stack - kd).max(0.0);
        // The readout takes a shade under half of what the knob left, and the
        // label the rest: a four-figure number is shorter than the word under
        // it and needs less room to say so. No cap on either — a band with
        // more room than its word needs draws the word at the size its own
        // clamp allows, centred, with air around it, and the stack stays in
        // the middle of the column. See `fit_text`.
        let readout_h = spare * 0.46;
        let dial_top = stack_top + readout_h;
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
        // **The readout takes its dial's whole share of the column**, gaps
        // included — the dials sit four points apart and a figure is centred,
        // so the two points either side are room the number can have and the
        // dial cannot. Two points of a five-character figure is most of a
        // point of type. See `fit_text` and `dial_type_size`.
        let readout = |dial: Rect| {
            Rect::from_min_max(
                Pos2::new(dial.left() - 1.75, stack_top),
                Pos2::new(dial.right() + 1.75, dial.top() - 1.0),
            )
        };
        // **What a NARROW header sheds: the three dials.** The vertical
        // rule below drops rows; nothing dropped columns, and at the
        // 150pt width floor the fixed cells flanking the column left it
        // INVERTED — every used lane's name, input chip and bays simply
        // vanished, rename and both menus dead with them. The mixer
        // carries the same three dials, so they are the horizontal
        // sacrifice — never the NAME.
        let dials_fit = kd >= 11.0 && pan.left() - 6.0 - (arm_seat.right() + 4.0) >= COL_MIN;
        let col_right = if dials_fit { pan.left() } else { meter.left() };
        let (send, knob, pan) = if dials_fit {
            (send, knob, pan)
        } else {
            (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING)
        };
        // **ROUTING sits at the BOTTOM, level with the second rack** — the
        // owner's placement: "the routing button can be at the bottom the
        // same height of the fx inserts". It stays in the dial column rather
        // than the name column because it is the same kind of thing the dials
        // are — what this lane DOES with its signal, not what it is called —
        // and it shares their fate on a narrow header: no dials, no button.
        //
        // It used to hang nine points under the dials by a constant, which
        // put it in the middle of the header at some heights and squeezed the
        // labels above it at every height. Level with the rack beside it, the
        // bottom of the row reads as one band across both columns, and the
        // labels get the whole of what is left.
        // **And it keeps the rack's margin too** — "routing to match as
        // well". It ran from the pan dial's left edge to the send's right,
        // which is the dial column's full width and flush against the meter.
        let routing = if dials_fit && rack2_y.1 > rack2_y.0 && pan.is_positive() {
            chip_band(Rect::from_min_max(
                Pos2::new(pan.left() + RACK_INSET, rack2_y.0),
                Pos2::new(send.right() - RACK_INSET, rack2_y.1),
            ))
        } else {
            Rect::NOTHING
        };
        // **Top-down: the NAME with its INPUT beside it, then the two
        // RACKS** — the owner's order, revised. It used to be name, then the
        // input selector on a band of its own, then three bays; every track
        // and folder wears SIX bays now, so the second rack needs a band and
        // the input moved up beside the name to give it one.
        let col = Rect::from_min_max(
            // After the T switch's SEAT, which sits between the arm and the
            // name - the seat and not the field, or a folder (which wears no
            // note) would start its name column off the left of the row.
            Pos2::new(theory_seat.right() + 4.0, r.top()),
            Pos2::new(col_right - 6.0, r.bottom() - 1.0),
        );
        let band = |(a, b): (f32, f32)| {
            if b <= a {
                return Rect::NOTHING;
            }
            Rect::from_min_max(Pos2::new(col.left(), a), Pos2::new(col.right(), b))
        };
        let top_band = band(top_y);
        let rack_band = band(rack_y);
        let rack2_band = band(rack2_y);
        // **A rack keeps a margin at both ends of its band.** The three ran
        // edge to edge of the column, so the outer two sat flush against the
        // switches on one side and the dial column on the other and the rack
        // read as a bar rather than as three things — the owner's "they have
        // breathing room between them and the margin of the element".
        const RACK_INSET: f32 = 3.0;
        // **A bay's width, for the controls that are asked to match one.**
        // Worked out from the same arithmetic `three` uses rather than read
        // back off a laid-out bay, because the racks shed and this is wanted
        // whether or not any bay survived.
        let bay_w = {
            let n = crate::recorder::INSERTS as f32;
            ((top_band.width() - RACK_INSET * 2.0 - 3.0 * (n - 1.0)) / n).max(0.0)
        };
        // **The NAME and, on a track, the INPUT button at its right end.**
        //
        // The selector had a whole band to itself and the second rack needed
        // one; the owner's call was to make it a button next to the name. It
        // takes a share of the band rather than a fixed width, so it grows
        // with a header the grip has widened, and it SHEDS rather than
        // squeezing the name below what a name needs — the same rule the rest
        // of this column follows.
        //
        // A FOLDER has nothing to select, so its whole band is name.
        //
        // **And a column too narrow to hold both gives the selector its old
        // band back** rather than shedding it: binding an input is how a bare
        // lane becomes a track, so it is the last thing on this column to go,
        // ahead of every bay. The racks then shed from the bottom, which is
        // the order they shed in anyway. A header dragged wider crosses the
        // floor and the second rack appears.
        //
        // **As thin as an fx slot, and centred on the name's line** — the
        // owner's revision. It filled the name band's whole height, which is
        // taller than a bay, so the one control on this column that is not a
        // rack slot was the biggest box on it. A bay's height is the measure
        // it takes now, and it sits in the middle of the band rather than
        // filling it.
        // **A BAY's width, to the point** — the owner's "lower the width of
        // the input selector button so it matches the dimensions of the fx
        // inserts". It had a share of the band, which made it wider than
        // anything else on the column and the odd one out on a panel whose
        // whole idiom is a row of equal chips.
        const NAME_MIN: f32 = 26.0;
        const CHIP_MIN: f32 = 16.0;
        let chip_w = bay_w;
        let beside = !folder
            && top_band.is_positive()
            && chip_w >= CHIP_MIN
            && top_band.width() - chip_w - 3.0 >= NAME_MIN;
        // A bay's own height, or the band's when there are no bays to match.
        let chip_h = if rack_band.is_positive() {
            (rack_band.height() * CHIP_FILL).min(top_band.height())
        } else {
            top_band.height() * CHIP_FILL
        };
        let (name, input, rack_a, rack_b) = if folder {
            // A folder has nothing to select, so its whole band is name and
            // both racks keep their own.
            (top_band, Rect::NOTHING, rack_band, rack2_band)
        } else if beside {
            (
                Rect::from_min_max(
                    top_band.min,
                    Pos2::new(top_band.right() - RACK_INSET - chip_w - 3.0, top_band.bottom()),
                ),
                // Its right edge on the rack's right edge, so the column
                // reads as one stack of chips rather than one that starts
                // three points further out than the two under it.
                Rect::from_center_size(
                    Pos2::new(top_band.right() - RACK_INSET - chip_w * 0.5, top_band.center().y),
                    Vec2::new(chip_w, chip_h),
                ),
                rack_band,
                rack2_band,
            )
        } else {
            (top_band, rack_band, rack2_band, Rect::NOTHING)
        };
        // **SIX bays on every lane** — the owner's ruling, folder or not. The
        // seat map has always answered slots 3..5 out of the channel's own
        // folder-extra seat whether or not it was a group (`seat_bay`); the
        // folder flag was only ever gating what got drawn. Bays 0..2 take the
        // middle band and 3..5 the bottom one, which is the order the chain
        // runs in and the order the mixer's rack shows them in.
        let three = |band: Rect, offset: usize, out: &mut [Rect; crate::recorder::INSERTS * 2]| {
            let band = chip_band(band);
            let n = crate::recorder::INSERTS;
            let inner = band.width() - RACK_INSET * 2.0;
            let w = (inner - 3.0 * (n as f32 - 1.0)) / n as f32;
            if w < 16.0 || !band.is_positive() {
                return;
            }
            for b in 0..n {
                let x0 = band.left() + RACK_INSET + b as f32 * (w + 3.0);
                out[offset + b] = Rect::from_min_max(
                    Pos2::new(x0, band.top()),
                    Pos2::new(x0 + w, band.bottom()),
                );
            }
        };
        let mut bays = [Rect::NOTHING; crate::recorder::INSERTS * 2];
        // The squeezed header keeps one rack band: the FIRST rack takes it
        // and the second sheds, the same order every other control here
        // sheds in.
        three(rack_a, 0, &mut bays);
        three(rack_b, 3, &mut bays);
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

/// How opaque a flag or a signature chip is over the ruler.
///
/// The owner: "Markers and timesigs now rectangles -- not rounded and with
/// no border. They must look or be slightly translucent (use theming
/// convention here)." The convention in this file is
/// `Color32::from_rgba_unmultiplied` on a colour the theme already owns —
/// the loop band does it at 26 and 84 — so that is what these use rather
/// than a second colour mixed by hand.
///
/// Translucent changes what the LABEL has to read against: not the body,
/// but the body seen through to the ruler. [`through`] is that colour, and
/// every ink on these two surfaces is chosen from it.
const CHIP_ALPHA: u8 = 210;

/// The colour a `CHIP_ALPHA` fill of `c` actually presents over `bg`.
fn through(c: Color32, bg: Color32) -> Color32 {
    blend(c, bg, 1.0 - f32::from(CHIP_ALPHA) / 255.0)
}

/// The same fill, as the painter states it.
fn translucent(c: Color32) -> Color32 {
    Color32::from_rgba_unmultiplied(c.r(), c.g(), c.b(), CHIP_ALPHA)
}

/// **A marker flag's body: the theme's own accent, walked until it stands
/// off the ruler.**
///
/// The owner asked for "a good, per themed flag system", and the accent —
/// `settings.white_key_active_color` — is the role that already means "the
/// user put this here" on this view: it is what an automation dial wears.
/// A flag in the same colour says the two systems belong to the same hand.
///
/// It is deliberately NOT `playhead`. That is a red in every theme, and the
/// flag draws a vertical line down the ruler exactly like the playhead does
/// — a red pole beside a red pole is a second transport line, which is the
/// one thing a landmark must not look like.
///
/// The walk is [`auto_dial_body`]'s, against a different surface: the flag
/// sits on the RULER, not on the header face, and measuring it against the
/// header would be the mistake this file's theme audit exists to catch. A
/// theme whose accent already reads keeps its colour exactly; one that does
/// not gives up the least brightness that works; and if nothing does — an
/// accent the same tone as that theme's ruler — the ruler's own ink is the
/// honest answer, because a flag you can see in the wrong colour beats one
/// you cannot see in the right one.
fn flag_body(p: &Ink, accent: (u8, u8, u8)) -> Color32 {
    // **Judged on what the fill PRESENTS, not on the fill.** The flags are
    // translucent over the ruler now, so the colour a reader sees is
    // `through(c, ruler)` and the label is chosen from that. A walk that
    // cleared 4.5:1 against the opaque body guaranteed a pairing that is
    // never drawn — five themes' labels landed between 3.6 and 4.1 the
    // moment the fill went translucent, which is how this was found.
    let ok = |c: Color32| {
        let seen = through(c, p.ruler);
        crate::recorder_panel::contrast_ratio(seen, p.ruler) >= 1.5
            && crate::recorder_panel::contrast_ratio(on_ink(seen), seen) >= 4.5
    };
    let base = Color32::from_rgb(accent.0, accent.1, accent.2);
    if ok(base) {
        return base;
    }
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
    on_ink(p.ruler)
}

/// The master dial's OUTLINE and its needle. Fixed on every theme — see the
/// note where they are used.
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


/// Song seconds at `x`, through the zoom window. The one mapping.
pub fn time_at(rect: Rect, view: &ArrangeView<'_>, x: f32) -> f64 {
    time_at_in(&Layout::new(rect, view), view, x)
}

/// [`time_at`] for a caller that already holds the frame's [`Layout`].
///
/// **The `_in` forms exist because the axis was being rebuilt per call.**
/// `x_of`, `time_at` and `clip_rect` each constructed a whole `Layout` to
/// read one pair of numbers off it — the lanes area's left edge and width —
/// and `clip_rect` constructed three (its own and one per `x_of`). A Layout
/// is 1.3 µs: thirty-three rows stacked, sixteen header rows measured cell by
/// cell, a bus row, thirteen kilobytes returned by value. `draw` asked for
/// one per grid line, per ruler label, per automation point and three per
/// clip whether the clip was on screen or not, and `targets` asked again;
/// measured, that was 1.9 µs per clip in the document per frame, which at two
/// thousand clips is a quarter of a frame spent re-deriving a rectangle that
/// had not moved. So the frame builds one Layout at its top and hands it
/// down, and the public forms stay as they were for the seventy-odd callers
/// in `app` that ask once per gesture, where a Layout is nothing.
///
/// Same arithmetic, same degenerate answers: the two must agree to the bit,
/// because a clip drawn through one and hit-tested through the other is the
/// pair that drifts. `clip_rect_agrees_with_its_in_form_to_the_bit` holds it.
pub fn time_at_in(l: &Layout, view: &ArrangeView<'_>, x: f32) -> f64 {
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
    x_of_in(&Layout::new(rect, view), view, t)
}

/// [`x_of`] on a Layout the caller already built — see [`time_at_in`].
pub fn x_of_in(l: &Layout, view: &ArrangeView<'_>, t: f64) -> f32 {
    if !l.lanes_area.is_positive() || view.view_len_s <= 0.0 {
        return l.lanes_area.left();
    }
    let f = ((t - view.view_start_s) / view.view_len_s) as f32;
    l.lanes_area.left() + f * l.lanes_area.width()
}

/// A clip's rectangle on its lane, clamped into the lanes area.
pub fn clip_rect(rect: Rect, view: &ArrangeView<'_>, clip: &ClipView<'_>) -> Rect {
    clip_rect_in(&Layout::new(rect, view), view, clip)
}

/// [`clip_rect`] on a Layout the caller already built — see [`time_at_in`].
///
/// The clamp against `l.lanes_area.right()` is the same edge the time
/// selection's veil, the pending ghost and the grid guards clamp against, so
/// a clip and the veil over it end on the same pixel.
pub fn clip_rect_in(l: &Layout, view: &ArrangeView<'_>, clip: &ClipView<'_>) -> Rect {
    let Some(row) = l.lane_rows.get(clip.lane).copied().filter(Rect::is_positive) else {
        return Rect::NOTHING;
    };
    let x0 = x_of_in(l, view, clip.start_s).max(l.lanes_area.left());
    let x1 = x_of_in(l, view, clip.start_s + clip.len_s).min(l.lanes_area.right());
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

/// **How narrow a pass may be drawn before its seams are dropped**, in points.
///
/// A one-bar loop under a two-hundred-bar clip, seen whole, is a pass a point
/// wide: two hundred notches at that spacing are not a picture of a loop, they
/// are a moiré over the whole clip, and the eye reads it as texture rather than
/// as structure. The miniature already refuses to draw marks it cannot place
/// (see [`draw_note_miniature`]); a seam that cannot say WHERE the loop turns
/// over is worth less than nothing, because it is claiming to.
const SEAM_MIN_PASS_W: f32 = 6.0;

/// **How wide one pass of a looped clip draws**, in points, across the clip's
/// unclamped span. Zero for a clip that does not loop.
///
/// One formula with two callers — where the seams fall, and how far a notch is
/// allowed to lean before it leans into the next one — because a lean measured
/// against a pass width computed a second way is the pair that drifts.
fn loop_pass_w(full_x: (f32, f32), clip: &ClipView<'_>) -> f32 {
    if clip.loop_s <= 1.0e-6
        || !clip.loop_s.is_finite()
        || clip.len_s <= 0.0
        || !clip.len_s.is_finite()
    {
        return 0.0;
    }
    let w = (f64::from(full_x.1 - full_x.0) * clip.loop_s / clip.len_s) as f32;
    if w.is_finite() {
        w.max(0.0)
    } else {
        0.0
    }
}

/// **Where a looped clip's seams fall**, as x in points, left to right.
///
/// One boundary per place [`crate::arrangement::clip_notes_in_song_time`]
/// starts another pass — the first at [`clip_head_s`], the rest a loop apart
/// after it — and only the ones that land inside the drawn rect with room to
/// be seen.
///
/// `full_x` is the clip's UNCLAMPED pixel span, the same one the miniature is
/// mapped through and for the same reason: the drawn rect is cut to the
/// viewport, so counting passes across the CUT width would slide every seam
/// sideways the moment the clip crossed an edge — which is the fault the
/// miniature shipped with once already ("clip begins shrinking").
///
/// Arithmetic and not a walk. The passes are evenly spaced, so the two that
/// bound the rect invert directly, and a clip holding five hundred passes of
/// which four are on screen costs four.
/// **Where a looped clip's FIRST pass ends**, in seconds from its left edge,
/// or `None` for a clip that never turns over inside itself.
///
/// A whole loop only for a clip that begins ON a boundary — and **the two
/// kinds of clip disagree about what beginning on one means**, so this
/// follows the sound rather than picking a rule.
///
/// On a MIDI clip the trim moves the material past a fixed grid: the notes
/// are placed at `at_s - offset_s + loop_s * pass`
/// ([`crate::arrangement::clip_notes_in_song_time`]), so a clip trimmed into
/// its bar starts mid-loop and its first pass is short by exactly how far in
/// it starts. On an AUDIO clip the trim chooses which seconds of the FILE
/// play, and every pass gets the same one: `audio_clip_passes` places pass
/// `k` at `start_s + loop_s * k` whatever the offset, which `wave_band`
/// mirrors by folding with `rem_euclid` before adding it. So an audio clip
/// turns over a whole loop in however far it has been trimmed, and giving it
/// the MIDI phase would slide its notches off its own wave.
///
/// Every boundary after the first is a whole loop along either way, which is
/// why one number answers the whole question.
///
/// **Shorter than a loop is not the same as not looping.** A MIDI clip half a
/// bar long that begins three quarters of the way through its bar does turn
/// over, once, in the middle; the `len_s <= loop_s` test that used to stand
/// here called that clip unlooped and drew it flat.
fn clip_head_s(clip: &ClipView<'_>) -> Option<f64> {
    // One spelling, shared with glue's "does it repeat" - see the fn.
    crate::arrangement::first_pass_s(clip.midi, clip.len_s, clip.loop_s, clip.offset_s)
}

/// **Does this clip show a seam at all?** — the loop question without the
/// pixels, so the fill, the miniature's inset and `loop_seam_xs` cannot
/// disagree about whether a clip is bitten.
fn clip_has_seam(clip: &ClipView<'_>) -> bool {
    clip_head_s(clip).is_some()
}

/// How deep a divot bites into the clip's top and bottom edge.
///
/// A tenth of the lane, floored at two points. The tenth is not a taste: the
/// waveform is drawn within forty per cent of the height either side of the
/// middle, so a tenth is the most that can be taken without the bite reaching
/// into it, and the miniature is inset by the same number for the same
/// reason. Two points is the least that reads as a bite rather than as an
/// artefact.
///
/// **The four-point ceiling that used to sit here was the bug the owner kept
/// reporting.** It bound above forty points of lane, which is every lane on
/// a real desk — a track row is ninety-odd points at four visible lanes — so
/// the tenth never applied and every clip got the same four-point nick
/// whatever its height. At that size the two bites do not read as a shape
/// change at all; what reaches the eye is a faint vertical line, which is
/// precisely what taking the material away was meant to replace. Nothing
/// justified the ceiling: the doc argued the tenth and then capped it.
///
/// So the tenth stands on its own, and the geometry that has to be respected
/// is respected where it lives — `notched_clip_pieces` still holds the bite
/// inside half the height, and the wave still owns the middle eighty per
/// cent by construction.
fn clip_bite_d(r: Rect) -> f32 {
    (r.height() * 0.10).max(2.0)
}

/// The clip's own outline, with a triangular divot bitten out of the top and
/// the bottom edge at every visible loop seam — the owner's "a small cutout
/// in the CLIP shape -- no line".
///
/// **Convex pieces, one per pass, and that is not a stylistic choice.**
/// epaint fills a closed path as a triangle fan from its first point, so a
/// single polygon carrying a notch in its edge comes back with the notch
/// filled in: the shape would be right in the point list and wrong on the
/// screen. A pass between two seams is a rectangle with its corners
/// chamfered, which is convex, and a fan over a convex polygon is exact.
///
/// The clip's OUTER corners keep their rounding — an arc where an inner end
/// carries a chamfer — so a looped clip and an unlooped one are the same
/// object, one of them with bites taken out. The previous seam was two
/// strokes printed ON the clip; this takes the material away instead, which
/// is why nothing here paints a colour of its own and nothing has to guess
/// what colour the lane behind it is.
///
/// Each end's features are budgeted against the segment's OWN width, because
/// `loop_seam_xs` may hand back a seam a single point inside the clip's edge
/// and a chamfer wider than the piece it sits on turns a convex polygon
/// inside out.
fn notched_clip_pieces(r: Rect, seams: &[f32], round: f32) -> Vec<Vec<Pos2>> {
    let d = clip_bite_d(r).min(r.height() * 0.5 - 0.5).max(0.0);
    let w = d * 1.5;
    let (top, bot) = (r.top(), r.bottom());
    let mut bounds: Vec<f32> = Vec::with_capacity(seams.len() + 2);
    bounds.push(r.left());
    bounds.extend_from_slice(seams);
    bounds.push(r.right());
    let mut out: Vec<Vec<Pos2>> = Vec::with_capacity(bounds.len().saturating_sub(1));
    for pair in bounds.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if !(b - a > 0.5) {
            continue;
        }
        // Forty-five per cent each, so the two ends together can never claim
        // more than the piece has.
        let room = (b - a) * 0.45;
        let (rad, bw) = (round.min(room), w.min(room));
        let mut pts: Vec<Pos2> = Vec::with_capacity(12);
        // Clockwise: the top edge left to right, then the bottom right to
        // left. `0.293` is the 45-degree point of the quarter arc, which is
        // all the rounding a three-point radius can show.
        if a <= r.left() {
            pts.push(Pos2::new(a, top + rad));
            pts.push(Pos2::new(a + rad * 0.293, top + rad * 0.293));
            pts.push(Pos2::new(a + rad, top));
        } else {
            pts.push(Pos2::new(a, top + d));
            pts.push(Pos2::new(a + bw, top));
        }
        if b >= r.right() {
            pts.push(Pos2::new(b - rad, top));
            pts.push(Pos2::new(b - rad * 0.293, top + rad * 0.293));
            pts.push(Pos2::new(b, top + rad));
            pts.push(Pos2::new(b, bot - rad));
            pts.push(Pos2::new(b - rad * 0.293, bot - rad * 0.293));
            pts.push(Pos2::new(b - rad, bot));
        } else {
            pts.push(Pos2::new(b - bw, top));
            pts.push(Pos2::new(b, top + d));
            pts.push(Pos2::new(b, bot - d));
            pts.push(Pos2::new(b - bw, bot));
        }
        if a <= r.left() {
            pts.push(Pos2::new(a + rad, bot));
            pts.push(Pos2::new(a + rad * 0.293, bot - rad * 0.293));
            pts.push(Pos2::new(a, bot - rad));
        } else {
            pts.push(Pos2::new(a + bw, bot));
            pts.push(Pos2::new(a, bot - d));
        }
        out.push(pts);
    }
    out
}

fn loop_seam_xs(r: Rect, full_x: (f32, f32), clip: &ClipView<'_>) -> Vec<f32> {
    let (fx0, fx1) = full_x;
    if !r.is_positive()
        || !fx0.is_finite()
        || !fx1.is_finite()
        || clip.loop_s <= 1.0e-6
        || !clip.loop_s.is_finite()
        || !clip.len_s.is_finite()
    {
        return Vec::new();
    }
    let pass_w = loop_pass_w(full_x, clip);
    if pass_w < SEAM_MIN_PASS_W {
        return Vec::new();
    }
    // **A trimmed MIDI clip's first pass is short**, and every seam after it
    // carries that shift. Laid at whole loops from the clip's left edge
    // instead — right for audio, and for a MIDI clip that begins on a
    // boundary — the notches sat a phase out, over the middle of a pass,
    // claiming the loop restarts where it does not.
    let Some(head_s) = clip_head_s(clip) else {
        return Vec::new();
    };
    let head_px = (f64::from(fx1 - fx0) * head_s / clip.len_s) as f32;
    let first = fx0 + head_px;
    // The boundaries that fall INSIDE the clip: one at `head_s`, then every
    // loop after it. A boundary landing exactly on the clip's end is its end
    // rather than a seam.
    let count = (((clip.len_s - head_s) / clip.loop_s).ceil() as usize).clamp(1, 512);
    let lo = ((r.left() - first) / pass_w).ceil();
    let hi = ((r.right() - first) / pass_w).floor();
    if !lo.is_finite() || !hi.is_finite() || hi < lo {
        return Vec::new();
    }
    // `as` saturates rather than wrapping, which is what is wanted at both
    // ends: a clip scrolled off to the left asks for a negative first seam
    // and gets the first one there is.
    let from = (lo.max(0.0) as usize).min(count);
    let to = ((hi.max(0.0) as usize).saturating_add(1)).clamp(from, count);
    (from..to)
        .map(|k| first + pass_w * k as f32)
        // Clear of the clip's own border, which would otherwise swallow a
        // seam sitting on it and leave a pass that looks like it has none.
        .filter(|x| *x > r.left() + 1.0 && *x < r.right() - 1.0)
        .collect()
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
/// **The band a miniature's notes may occupy** — inside the border, under the
/// title, and clear of the loop divots.
///
/// One function because there are two walks over these notes: the shipped
/// one, which culls the passes that cannot reach the window, and the
/// reference in the tests, which walks all of them. They exist to differ in
/// the cull and in nothing else, so a band computed twice is a way for them
/// to disagree about something the test was never watching.
///
/// The divot inset is vertical only. The bites are on the top and bottom
/// edges, and narrowing the band sideways would move every note in the
/// picture; a note bar drawn ACROSS a bite would hang in the lane outside its
/// own clip, which is the one way this can look broken rather than merely cut.
fn miniature_band(r: Rect, clip: &ClipView<'_>) -> Rect {
    let base = r.shrink(2.0);
    let base = if clip_has_seam(clip) {
        let extra = (clip_bite_d(r) - 2.0).max(0.0);
        Rect::from_min_max(
            Pos2::new(base.left(), base.top() + extra),
            Pos2::new(base.right(), base.bottom() - extra),
        )
    } else {
        base
    };
    let band = clip_name_band(r);
    if band > 0.0 && base.height() > band {
        Rect::from_min_max(Pos2::new(base.left(), base.top() + band), base.max)
    } else {
        base
    }
}

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
        (((clip.len_s + clip.offset_s.max(0.0)) / clip.loop_s).ceil() as usize)
            .clamp(1, 512)
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
    // For the clip of ONE pass: the longest mark, and whether the notes are
    // in time order — gathered in the same pass as the pitch range, so the
    // clip that does not loop is walked ONCE for its facts and the marks
    // below are drawn from the notes under the window alone. A recorded
    // part is exactly this clip: thousands of notes, no loop to skip, and
    // it was walked twice a frame however far in the view was zoomed.
    let mut longest = 0.02f64;
    let mut in_order = true;
    let mut prev_at = f64::NEG_INFINITY;
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
            longest = longest.max(n.dur_s);
            // A NaN compares false and marks the clip out of order, which
            // sends it down the whole walk: slower, never wrong.
            in_order &= n.at_s >= prev_at;
            prev_at = n.at_s;
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
    // **Under the title, and clear of the bites.** See `miniature_band`.
    let inner = miniature_band(r, clip);
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
    // **Every mark of this clip goes into ONE shape.**
    //
    // A mark used to be a `rect_filled`, which is a `Shape` of its own: a
    // hundred bytes pushed on the frame's list, and then a whole rounded
    // rectangle tessellated out of it — an arc at each of four corners and
    // a feathered skirt around the outside, eight vertices and thirty
    // indices for a dash a point and a half wide. Measured on the perf
    // fixture's largest project at the whole-song zoom, that was 51,240 of
    // the frame's 53,232 shapes, and the tessellator spent 3.9 ms of the
    // frame's 5.7 on them: the miniatures WERE the frame.
    //
    // They are four vertices and two triangles each now, written straight
    // into a mesh the clip hands over once — 2,480 shapes and 3.2 ms.
    // The geometry is the same rectangle to the bit —
    // the arithmetic above is untouched, and `mod tests` still holds the
    // picture against the plain walk cell by cell — so what is given up is
    // only what epaint was doing UNDERNEATH the rectangle:
    //
    // * the feathered edge. A mark is 1.5 pt wide against a feather of half
    //   a point on a retina screen, so the softening was a third of it; a
    //   miniature is an impression of a part, drawn in ink at 80% and
    //   overlapping itself, and a crisper dash is not a different picture.
    // * epaint's pixel rounding of a filled rect (`round_rects_to_pixels`,
    //   on by default). The merged marks all sit on whole-point column
    //   boundaries and so all shared one rounding; unrounded they still
    //   share one phase, so they stay a comb rather than becoming a ragged
    //   one. A held note's span moves by less than a pixel.
    // * the 1 pt corner radius, which epaint clamped to 0.75 on a mark this
    //   size and which `what_do_the_note_miniatures_pay_every_frame` had
    //   already measured as costing nothing to drop.
    //
    // What it must NOT give up is the order: a mark's ink is translucent, so
    // two marks that overlap read darker, and triangles are drawn in the
    // order they are indexed. The span pass writes first and the merged
    // columns after, exactly as the two `rect_filled` passes did.
    let mark_ink = ink.gamma_multiply(0.8);
    let mut mesh = egui::epaint::Mesh::default();
    let bar = |mesh: &mut egui::epaint::Mesh, x0: f32, top: f32, bottom: f32| {
        mesh.add_colored_rect(
            Rect::from_min_max(Pos2::new(x0, top), Pos2::new(x0 + 1.5, bottom)),
            mark_ink,
        );
    };
    // One mark drawn where it actually falls, over the span it actually
    // covers. This is the plain walk's own drawing, kept as a closure so
    // that the merged path can reach for it note by note.
    let span_bar = |mesh: &mut egui::epaint::Mesh, x0: f32, x1: f32, k: u8| {
        let y = y_of(usize::from(k));
        mesh.add_colored_rect(
            Rect::from_min_max(Pos2::new(x0, y), Pos2::new(x1.max(x0 + 1.5), y + bar_h)),
            mark_ink,
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
    // **The notes under the window, when the clip is one pass in time
    // order.** `mark` answers None for a note whose span misses `inner`, so
    // the walk used to visit every note to learn which few to draw; with
    // the notes sorted by their start the two edges are a binary search
    // each. The left edge is pulled back by the longest note, because a
    // note that STARTS before the window can still reach into it. Same
    // marks as the whole walk — the notes skipped are exactly the ones
    // `mark` would have refused — and
    // `a_recorded_part_draws_the_same_picture_as_the_plain_walk` says so on
    // a sorted part, a reversed one and a shuffled one.
    let notes: &[crate::arrangement::ArrNote] = if passes == 1 && in_order {
        let at = |x: f32| (f64::from(x) - f64::from(fx0)) / f64::from(fw) * len + clip.offset_s;
        let s_lo = at(inner.left()) - longest;
        let s_hi = at(inner.right());
        if s_lo.is_finite() && s_hi.is_finite() {
            let a = clip.notes.partition_point(|n| n.at_s < s_lo);
            let b = clip.notes.partition_point(|n| n.at_s < s_hi);
            &clip.notes[a.min(b)..b]
        } else {
            clip.notes
        }
    } else {
        clip.notes
    };
    for (pass, n) in seen.flat_map(|k| notes.iter().map(move |n| (k, n))) {
        if let Some((x0, x1, k)) = mark(pass, n) {
            // **Wide enough to show where it begins**: draw it as the span
            // it is. A held note crosses many columns and reads as a line;
            // folding it into the column it starts in would delete it.
            if lit.is_empty() || x1 - x0 >= 1.0 {
                span_bar(&mut mesh, x0, x1, k);
            } else {
                let c = (((x0 - inner.left()).max(0.0)) as usize).min(cols - 1);
                lit[c * 2 + usize::from(k) / 64] |= 1u64 << (usize::from(k) % 64);
            }
        }
    }
    // **The one exit that hands the mesh over.** The merged columns are
    // skipped when there is no grid, not returned from: the span pass has
    // already written into the mesh by here, and a `return` above the
    // handover is how a clip's whole miniature goes missing.
    if !lit.is_empty() {
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
                            bar(&mut mesh, x0, top, bottom);
                            (y, y + bar_h)
                        }
                        None => (y, y + bar_h),
                    });
                }
            }
            if let Some((top, bottom)) = run {
                bar(&mut mesh, x0, top, bottom);
            }
        }
    }
    if !mesh.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// The resize strip's width — REAPER's splitter, hover-lit, its own cursor.
const GRIP_W: f32 = 7.0;

/// The same hit, with a clip's resize and fade AFFORDANCES resolved away —
/// what a CLICK is on, as opposed to what a drag would grab.
///
/// A clip wears four grab zones: `EDGE_W` at each end for the resize, and an
/// `FADE_W` corner at each top for the fade handle. They exist so a hand can
/// catch a border and pull, and they are right for a drag. They are wrong for a
/// double-click, which has exactly one meaning on a MIDI item — open it — and
/// no meaning at all on its edges: an item grabbed anywhere in those zones
/// simply would not open, and the zones are the two ends and both top corners,
/// which is most of a short clip's face.
///
/// The roll has the same fault around a note's borders, for the same reason and
/// with the same fix — see `midi_editor::click_hit`, and the owner's report
/// that took us to both.
pub fn click_hit(rect: Rect, view: &ArrangeView<'_>, pos: Pos2) -> Option<Hit> {
    match hit_test(rect, view, pos) {
        Some(Hit::ClipLeft(id) | Hit::ClipRight(id) | Hit::FadeIn(id) | Hit::FadeOut(id)) => {
            // Every one of those zones is INSIDE the clip's own rectangle —
            // there is no allowance pass here reaching across empty lane — so
            // the hand is on the clip whichever of them answered.
            Some(Hit::ClipBody(id))
        }
        other => other,
    }
}

pub fn hit_test(rect: Rect, view: &ArrangeView<'_>, pos: Pos2) -> Option<Hit> {
    hit_test_in(&Layout::new(rect, view), view, pos)
}

/// [`hit_test`] on a Layout the caller already built — see [`time_at_in`].
/// The wheel handler asks this and the axis on the same frame, and it used
/// to build a Layout for each.
pub fn hit_test_in(l: &Layout, view: &ArrangeView<'_>, pos: Pos2) -> Option<Hit> {

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
        return Some(Hit::AutoMenu(LaneId::Master));
    }
    if l.master_row.knob.is_positive() && l.master_row.knob.contains(pos) {
        return Some(Hit::BusGain(Column::Master));
    }
    for (b, cell) in l.master_row.bays.iter().enumerate() {
        if cell.is_positive() && cell.contains(pos) {
            return Some(Hit::BusBay(Column::Master, b));
        }
    }
    for (i, row) in l.header_rows.iter() {
        for (r, hit) in [
            (row.arm, Hit::Arm(i)),
            (row.mute, Hit::Mute(i)),
            (row.phase, Hit::Phase(i)),
            (row.solo, Hit::Solo(i)),
            (row.theory, Hit::ToTheory(i)),
            (row.automation, Hit::AutoMenu(LaneId::Row(i))),
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
    // **The strip's two rows, before the ruler itself.** They sit ON the
    // ruler and a press on one must not also locate — the same pre-pass rule
    // the band's timeline × follows.
    if l.meter_strip.is_positive() && l.meter_strip.contains(pos) {
        let names = flag_row(l.meter_strip);
        let sigs = sig_row(l.meter_strip);
        // **Each row answers for its own kind**, which is what separating
        // them bought: a flag and a signature on the same beat no longer
        // overlap, so there is no "which was drawn last" to resolve. The
        // reverse walk stays, for two markers close enough to overlap each
        // other — there the later one is still the one on top.
        if names.contains(pos) {
            let font_h = strip_font_h(names);
            for (i, m) in view.markers.iter().enumerate().rev() {
                let x = x_of_in(l, view, m.at_s);
                let shown = match view.marker_edit.filter(|(n, _)| *n == i) {
                    Some((_, t)) => format!("{t}|"),
                    None if m.name.is_empty() => "\u{2691}".to_owned(),
                    None => m.name.clone(),
                };
                let w = (font_h * 0.62 * shown.chars().count() as f32 + 8.0).max(font_h * 2.2);
                if pos.x >= x && pos.x <= x + w {
                    return Some(Hit::Flag(i));
                }
            }
            // **Bare name row is RULER, and that is deliberate.** It used
            // to answer `MeterStrip`, whose left press opened a signature
            // field — which was tolerable when the strip was eight points
            // and is not now that it is twenty-five: a quarter of the ruler
            // would swallow every click meant to move the playhead. Left
            // locates, right opens the menu (`flag_menu_subject` maps
            // `Ruler` to the same rows), which is the split the owner asked
            // for on the flags and is the right one here too.
        }
        let font_h = strip_font_h(sigs);
        for pt in view.meter.points() {
            let typing = view.meter_edit.filter(|(b, _)| *b == pt.at_bar);
            // Skipped exactly where the painter skips it, typing included —
            // see the `continue` in `draw`. A chip that is on screen only
            // while a field is open is a chip that has to be pressable only
            // while a field is open.
            if pt.at_bar == 0 && view.meter.points().len() == 1 && typing.is_none() {
                continue;
            }
            let x = x_of_in(l, view, bar_at(view, pt.at_bar));
            // The same width the chip is DRAWN at, including the field's
            // caret: a target that disagreed with the paint by a character
            // would be a chip you cannot press once you have opened it.
            let text = match typing {
                Some((_, t)) => format!("{t}|"),
                None => format!("{}/{}", pt.sig.beats, pt.sig.unit),
            };
            let w = (font_h * 0.62 * text.chars().count() as f32 + 6.0).max(font_h * 2.2);
            if pos.x >= x && pos.x <= x + w {
                return Some(Hit::MeterChip(pt.at_bar));
            }
        }
        // Bare signature row falls through to the ruler as well, for the
        // reason above: the whole band locates unless something is under
        // the pointer.
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
                            tempo_x_of(l, view, row.lane, q.at_beat),
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
                        time_at_in(l, view, pos.x),
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
                let at = Pos2::new(x_of_in(l, view, p.at_s), auto_y_of(row.lane, p.value));
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
                time_at_in(l, view, pos.x),
                auto_value_at(row.lane, pos.y),
            ));
        }
        continue;
    }
    if l.ruler.is_positive() && l.ruler.contains(pos) {
        return Some(Hit::Ruler(time_at_in(l, view, pos.x)));
    }
    for clip in view.clips.iter().rev() {
        // A clip being RECORDED is a picture, not a control — the document
        // row it becomes does not exist yet.
        if clip.recording {
            continue;
        }
        let r = clip_rect_in(l, view, clip);
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
    for (i, row) in l.lane_rows.iter() {
        if row.is_positive() && row.contains(pos) {
            return Some(Hit::Lane(i, time_at_in(l, view, pos.x)));
        }
    }
    // The master's field, last: its header's own controls are pushed above
    // and have already answered, so what is left is the ground beside them.
    if let Some(row) = l.rows.iter().find(|r| r.kind == RowKind::Master) {
        if row.lane.is_positive() && row.lane.contains(pos) {
            return Some(Hit::MasterLane(time_at_in(l, view, pos.x)));
        }
    }
    None
}


/// **The lane a hovering file would land on**, where it is drawn, or
/// `None` when no file hovers or its lane is not on screen.
///
/// By ROW: `drop_lane` is the lane the drop lands on, which is what the
/// layout's rows are keyed by. It was read relative to the scroll
/// (`drop_lane - lane_scroll` into the rows), so a scrolled timeline lit a
/// lane `lane_scroll` rows away from the one the file would land on, and
/// usually lit nothing at all.
pub fn drop_lane_rect(l: &Layout, view: &ArrangeView<'_>) -> Option<Rect> {
    view.drop_lane
        .and_then(|target| l.lane_rows.get(target).copied())
        .filter(|r| r.is_positive())
}

/// **A press on the lane scrollbar**: where on the thumb the hand took it,
/// or `None` when the press is not on the bar. A press on the thumb keeps
/// its grip; a press beside it centres the thumb under the hand, so the
/// lanes jump to where it was pressed. The whole gutter and a little past
/// the window's edge, because five points is a line to look at and not a
/// target to hit; never a point of the lanes, which belong to the clips,
/// nodes and points drawn there (see [`LaneBar`]).
pub fn lane_bar_grab(rect: Rect, view: &ArrangeView<'_>, pos: Pos2) -> Option<f32> {
    let l = Layout::new(rect, view);
    let bar = l.lane_bar;
    if !bar.track.is_positive() {
        return None;
    }
    let band = Rect::from_min_max(
        Pos2::new(l.lanes_area.right(), bar.track.top()),
        Pos2::new(bar.track.right() + 3.0, bar.track.bottom()),
    );
    // Strictly right of the lanes: their last point is a lane's.
    if pos.x <= l.lanes_area.right() || !band.contains(pos) {
        return None;
    }
    Some(if bar.thumb.y_range().contains(pos.y) {
        pos.y - bar.thumb.top()
    } else {
        bar.thumb.height() * 0.5
    })
}

/// **The lane scroll with the hand at `pos`**, holding the thumb `grab`
/// points below its top: the first row shown, clamped to the desk. `None`
/// when there is no bar.
pub fn lane_bar_scroll(rect: Rect, view: &ArrangeView<'_>, pos: Pos2, grab: f32) -> Option<usize> {
    let bar = Layout::new(rect, view).lane_bar;
    let travel = bar.track.height() - bar.thumb.height();
    if !bar.track.is_positive() || travel <= 0.0 {
        return None;
    }
    let t = ((pos.y - grab - bar.track.top()) / travel).clamp(0.0, 1.0);
    Some((t * bar.max_rows as f32).round() as usize)
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
    targets_in(&Layout::new(rect, view), view)
}

/// [`targets`] on a Layout the caller already built — see [`time_at_in`].
pub fn targets_in(l: &Layout, view: &ArrangeView<'_>) -> Vec<(Rect, Hit)> {
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
    push(l.master_row.automation, Hit::AutoMenu(LaneId::Master));
    push(l.master_row.knob, Hit::BusGain(Column::Master));
    for (b, cell) in l.master_row.bays.iter().enumerate() {
        push(*cell, Hit::BusBay(Column::Master, b));
    }
    for row in &l.rows {
        match row.kind {
            // The tempo has no header to hang a node on any more — it
            // draws in the master's lane. Its one control, flatten, is the
            // right button over the curve; see `app`'s secondary-press arm.
            RowKind::Tempo => {}
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
                    (hdr.automation, Hit::AutoMenu(LaneId::Row(i))),
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
                    let r = clip_rect_in(l, view, clip);
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
    a11y_nodes_from(view, &targets(rect, view))
}

/// [`a11y_nodes`] over a target list the caller already has.
///
/// **Two hundred nodes were being built and dropped every frame.** `paint`
/// ran `a11y_nodes` (which ran `targets`) and then `targets` again, and
/// handed the result to `offer_nodes`, whose first line returns when no
/// reader is on — so for everybody but a screen-reader user the frame
/// formatted two hundred labels, two hundred Debug strings for the ids and
/// two target walks, and threw them away. The nodes are now built only on a
/// frame that will install them, from the one `targets` walk that frame
/// also needs for the map back from a focused node to its hit.
pub fn a11y_nodes_from(
    view: &ArrangeView<'_>,
    hits: &[(Rect, Hit)],
) -> Vec<crate::a11y::Node> {
    use crate::a11y::{Node, Travel};
    let mut out = Vec::with_capacity(hits.len());
    let up = |norm: f32| Travel { norm: norm.clamp(0.0, 1.0), up: true };
    // The name a person would use for a row, whether or not it is a track
    // yet. Never an empty string — a node labelled "" is a node a reader
    // announces as nothing at all.
    let who_row = |i: usize| -> String {
        match view.lanes.get(i) {
            Some(l) if !l.name.is_empty() => l.name.to_owned(),
            _ => format!("Track {}", i + 1),
        }
    };
    // The bus has a word of its own: when the master was addressed as the
    // lane one past the channels, the row lookup found nothing there and
    // the fallback named it "Track 17", which is what a reader heard on
    // every master automation row. The same word the bus row's own labels
    // use.
    let who = |lane: LaneId| -> String {
        match lane {
            LaneId::Master => "Master".to_owned(),
            LaneId::Row(i) => who_row(i),
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
    for &(r, hit) in hits {
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
            Hit::Flag(i) => Node::button(
                id,
                r,
                &match view.markers.get(i) {
                    Some(m) if !m.name.is_empty() => format!("Marker {}, go to it", m.name),
                    _ => "Unnamed marker, go to it".to_owned(),
                },
            ),
            // By NAME, like every other header control: "Track 3" read a
            // renamed track by its seat.
            Hit::Phase(i) => Node::toggle(
                id,
                r,
                &format!("{} polarity invert", who_row(i)),
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
                format!("{} record arm", who_row(i)),
                view.lanes.get(i).is_some_and(|l| l.armed),
            ),
            Hit::Mute(i) => Node::toggle(
                id,
                r,
                format!("{} mute", who_row(i)),
                view.lanes.get(i).is_some_and(|l| l.muted),
            ),
            Hit::Solo(i) => Node::toggle(
                id,
                r,
                format!("{} solo", who_row(i)),
                view.lanes.get(i).is_some_and(|l| l.soloed),
            ),
            Hit::ToTheory(i) => Node::toggle(
                id,
                r,
                format!("{} sends its MIDI to the theory view", who_row(i)),
                view.lanes.get(i).is_some_and(|l| l.to_theory),
            ),
            Hit::HdrCollapse(i) => Node::toggle(
                id,
                r,
                format!("{} shows its tracks", who_row(i)),
                !view.lanes.get(i).is_some_and(|l| l.folder_shut),
            ),
            Hit::HdrAdd(i) => Node::button(
                id,
                r,
                match view.lanes.get(i).map_or(crate::mixer_panel::Plus::Row, |l| l.plus) {
                    crate::mixer_panel::Plus::Row => format!("Add a track at row {}", i + 1),
                    crate::mixer_panel::Plus::Trailing => "Add a track".to_owned(),
                    crate::mixer_panel::Plus::Locked => {
                        "Add a track (the free desk is full: a supporter key lifts the limit)".to_owned()
                    }
                },
            ),
            Hit::HdrName(i) => Node::readout(id, r, format!("{} name", who_row(i)), who_row(i)),
            Hit::HdrInput(i) => Node::readout(
                id,
                r,
                format!("{} input", who_row(i)),
                match view.lanes.get(i) {
                    Some(l) if !l.input.is_empty() => l.input.clone(),
                    _ => "none".to_owned(),
                },
            ),
            Hit::HdrBay(i, bay) => {
                let what = view.lanes.get(i).map_or("", |l| l.inserts[bay]);
                // Bypassed said in the VALUE, the mixer's wording exactly:
                // the amber bar and the quieted word are the whole visual
                // answer and a reader gets neither.
                let off = view.lanes.get(i).is_some_and(|l| l.bypassed[bay]);
                Node::readout(
                    id,
                    r,
                    format!("{} insert {}", who_row(i), bay + 1),
                    match (what.is_empty(), off) {
                        (true, _) => "empty".to_owned(),
                        (false, true) => format!("{what}, bypassed"),
                        (false, false) => what.to_owned(),
                    },
                )
            }
            Hit::HdrRouting(i) => Node::button(id, r, format!("{} routing", who_row(i))),
            // The two rows above the tracks. Their level is the desk's own,
            // so it reads in the desk's own words.
            Hit::BusBay(_, slot) => {
                let word = view.master_inserts.get(slot).copied().unwrap_or("");
                let off = view.master_bypassed.get(slot).copied().unwrap_or(false);
                Node::button(
                    id,
                    r,
                    match (word.is_empty(), off) {
                        (true, _) => format!("Master insert {}, empty", slot + 1),
                        (false, true) => {
                            format!("Master insert {}, {word}, bypassed", slot + 1)
                        }
                        (false, false) => format!("Master insert {}, {word}", slot + 1),
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
                    format!("{} volume", who_row(i)),
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
                    format!("{} send", who_row(i)),
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
                    format!("{} pan", who_row(i)),
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
                let mut says = format!("{}, {}", bars(c.start_s, c.len_s), who_row(c.lane));
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
            | Hit::TempoPoint(_)
            | Hit::TempoLane(..)
            | Hit::MeterChip(_) => continue,
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
    // ── Worked out ONCE per frame from the roles above, because they were
    // being worked out per switch, per chip, per bar label and per
    // automation row: `quiet` and `legible_surface` each cost a contrast
    // walk (three powf per colour per step), and none of these depend on
    // anything but the theme. See `ink`. ──
    /// A switch's letter when it is OFF, on `hdr_lane`.
    switch_off: Color32,
    /// An empty slot's number and its +, on `empty_ground`.
    empty_ink: Color32,
    /// The folder arrow and its count, on `hdr_lane`.
    collapse_ink: Color32,
    /// A bay chip's word: a tenant, an empty +, a bypassed tenant.
    bay_ink: [Color32; 3],
    /// The bar numbers and the clock, on the ruler — the numbers a notch
    /// stronger than the clock, see the ruler loop in `draw`.
    ruler_num: Color32,
    ruler_clock: Color32,
    /// An automation row's ground under an even and an odd lane — see
    /// [`auto_ground`], which reads this.
    auto_ground: [Color32; 2],
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
/// `bypassed` is the bay's tenant being kept and not applied — see
/// `settings::BypassBays`. It shows the way the MIXER's rack shows it, and
/// deliberately so: these chips already wear the mixer's own strip face
/// (`hdr_face`), and a hand that has learned "amber bar, grey word" on one
/// desk has learned it on both.
fn bay_chip(
    painter: &Painter,
    r: Rect,
    word: &str,
    has: bool,
    bypassed: bool,
    p: &Ink,
    // **The ROW's size, not this box's.** Every word on a header row is one
    // size — see `header_type_size` — and a chip that worked its own out from
    // its own height is how ROUTING came to be bigger than the figure beside
    // it. The master row, which has no dial column to be held down by, passes
    // its own.
    size: f32,
) {
    if !r.is_positive() {
        return;
    }
    let bone = p.hdr_face;
    let ink = p.engrave;
    painter.rect_filled(r, 2.0, bone);
    painter.rect_stroke(
        r,
        2.0,
        Stroke::new(1.0_f32, ink.gamma_multiply(0.4)),
        egui::StrokeKind::Inside,
    );
    // **The amber bar down the left edge, on a bypassed bay only.**
    //
    // The mixer's rack has a tab there always — blue for loaded, amber for
    // bypassed — while this chip has never had one, so an unbypassed chip is
    // left exactly as it was drawn yesterday and only the new state grows a
    // new mark. Through `lit_on`, because a fixed amber on a themed face is
    // a mark that disappears on the warm themes; the all-theme sweep in this
    // file's tests holds it to a floor.
    let text_left = if bypassed && has {
        let tab = crate::recorder_panel::lit_on(p.mute, bone);
        painter.rect_filled(
            Rect::from_min_max(r.min, Pos2::new(r.left() + 3.0, r.bottom())),
            2.0,
            tab,
        );
        4.0
    } else {
        0.0
    };
    let font = FontId::new(size, crate::fonts::courier());
    // **And the word goes as quiet as a bypassed automation row's does** —
    // 0.45, the figure two hundred lines down where the same state is drawn
    // on a lane. `quiet` re-measures against this chip's own ground and
    // stops at 4.5:1, so "quieter" never becomes "gone" on a dark theme.
    let word_ink = if bypassed && has {
        p.bay_ink[2]
    } else if has {
        p.bay_ink[0]
    } else {
        p.bay_ink[1]
    };
    let inner = Rect::from_min_max(Pos2::new(r.left() + text_left, r.top()), r.max);
    painter.text(
        inner.center(),
        Align2::CENTER_CENTER,
        cut_to(painter, inner.width() - 4.0, word, &font),
        font,
        word_ink,
    );
}

/// **A chip whose word shrinks to fit rather than being cut off.**
///
/// [`bay_chip`]'s twin for the one chip on this header that carries a
/// SENTENCE rather than a plugin's four-letter short name. A bay says "COMP"
/// or "+"; the input says "NO INPUT" or "MIDI: Scarlett", and it now shares
/// the name's row instead of having a band of the column to itself — so at
/// the header widths this app opens at, printing at the box's own height and
/// truncating gave "NO...", which is not a button, it is a smudge with a
/// border.
///
/// The height names the ceiling and the WIDTH decides, measured in the face
/// that is installed (the lesson `fit_text` records). Under the floor —
/// seven points, below which shrinking is the other way of being unreadable —
/// it stops shrinking and `cut_to` takes over, which is the honest failure:
/// an ellipsis says there is more, a four-point word says nothing.
fn input_chip(painter: &Painter, r: Rect, word: &str, has: bool, p: &Ink, size: f32) {
    if !r.is_positive() {
        return;
    }
    let bone = p.hdr_face;
    let ink = p.engrave;
    painter.rect_filled(r, 2.0, bone);
    painter.rect_stroke(
        r,
        2.0,
        Stroke::new(1.0_f32, ink.gamma_multiply(0.4)),
        egui::StrokeKind::Inside,
    );
    let ceiling = size;
    let family = crate::fonts::courier();
    let word_ink = if has { p.bay_ink[0] } else { p.bay_ink[1] };
    let room = r.width() - 6.0;
    let measure = |size: f32, text: &str| {
        painter
            .layout_no_wrap(text.to_owned(), FontId::new(size, family.clone()), word_ink)
            .rect
            .width()
    };
    let wide = measure(ceiling, word);
    let size = if wide > room && wide > 0.0 {
        crate::fonts::px(ceiling * room / wide).max(6.5)
    } else {
        ceiling
    };
    let font = FontId::new(size, family);
    painter.text(
        r.center(),
        Align2::CENTER_CENTER,
        cut_to(painter, room, word, &font),
        font,
        word_ink,
    );
}

/// **The smallest a word that shrinks to its box is drawn: six and a half
/// points.**
///
/// The floor of the other words in this app that shrink rather than cut:
/// the INPUT chip's ([`input_chip`]) and the analog faces' own
/// (`faces_analog::fit`, "the smallest legible"). Under it a grid step is a
/// smudge, and a smudge in a grid box reads as a wrong number rather than a
/// small one.
pub const WORD_FLOOR: f32 = 6.5;

/// **The air a word that had to shrink keeps from each side of its room**,
/// in points.
///
/// Fitted with none, a word would fill its room to the last hundredth and
/// its ink would meet the box's edge. A quarter of a point because the room
/// is small: the arrange gridbox is twenty points, and a five-character step
/// (1/128) at the floor is 18.9 to 19.4 of them in Courier Prime and
/// JetBrains Mono, depending on the display's scale. Half a point a side
/// would put that step under the floor on a Retina screen and not on a
/// plain one.
pub const WORD_AIR: f32 = 0.25;

/// **The size a word is drawn at in a room `width` wide: shrunk to the
/// width, never spilled.**
///
/// `ceiling` when the word is laid out inside the room at that size, so a
/// word its box was drawn for keeps exactly the size it always had (BAR and
/// 1/2 in the arrange gridbox, 11 points). Otherwise the largest half point
/// under the ceiling at which it is laid out inside with [`WORD_AIR`] either
/// side, measured in the face that is installed, and never under
/// [`WORD_FLOOR`]. A word that does not fit even at the floor is given the
/// floor and left to the clip in [`paint_fitted_word`]: past that point a
/// smaller word is a smudge, and which words those are is a question for
/// the box, not for the type.
///
/// This is the INPUT chip's rule ([`input_chip`]: the height names the
/// ceiling and the width decides), except that it never truncates. A grid
/// step cut to "1/1..." names a different step.
///
/// Half points rather than any size: each word gets one size that does not
/// move from frame to frame, so the glyph atlas is not handed a new fraction
/// every frame (why `fonts::px` rounds), and on a 2x display half a point is
/// a whole pixel. Found by walking down rather than by scaling the width
/// once, because glyphs land on pixels and a word's width is not quite in
/// proportion to its size.
pub fn fitted_word_size(
    painter: &Painter,
    word: &str,
    family: &egui::FontFamily,
    ceiling: f32,
    width: f32,
) -> f32 {
    let wide = |size: f32| {
        painter
            .layout_no_wrap(word.to_owned(), FontId::new(size, family.clone()), Color32::WHITE)
            .rect
            .width()
    };
    if wide(ceiling) <= width {
        return ceiling;
    }
    // A ceiling already under the floor is its own floor: shrinking stops
    // there, and growing is not what this is for.
    let floor = WORD_FLOOR.min(ceiling);
    let room = width - 2.0 * WORD_AIR;
    // The first half point under the ceiling (10.5 under 11, 9.5 under the
    // MIDI chip's 9.9), then down by halves.
    let mut size = (ceiling * 2.0).ceil() * 0.5 - 0.5;
    while size > floor && wide(size) > room {
        size -= 0.5;
    }
    size.max(floor)
}

/// **A word painted in its box at [`fitted_word_size`], and never outside
/// it.**
///
/// `room` is where the word may be laid out: the box itself, or the part of
/// it inside a border. `bx` is the box, and the word is clipped to it,
/// within whatever clip the painter already has. The clip is the backstop
/// and not the fit: every word a grid ladder can print fits its box above
/// the floor except the few `fitted_word_size` cannot, and those are cut at
/// the box's edge instead of painting over the next control. That is the
/// class of fault behind the owner's "0.000000": a grid step six characters
/// wide in the arrange gridbox, spilling onto the tempo lane's switch, with
/// the navigator painted over its tail.
///
/// **Centred as `Align2::CENTER_CENTER` centres it, then held inside by
/// whole pixels.** A word is laid out by its advances, but its glyphs are
/// drawn as whole pixels rounded outward and the text is snapped to a pixel,
/// so a word that fits by its advances can still put one column of ink past
/// the room's edge: BAR in Courier Prime at 150%, 1/128 in JetBrains Mono at
/// 100%. The clip would cut that column off a word that fits. So when the
/// ink the snap is going to draw crosses an edge and the room is wide
/// enough for it, the word moves in by the pixels it crosses by; otherwise
/// it stays exactly where centring put it.
pub fn paint_fitted_word(
    painter: &Painter,
    bx: Rect,
    room: Rect,
    word: &str,
    family: egui::FontFamily,
    ceiling: f32,
    ink: Color32,
) {
    if !room.is_positive() || word.is_empty() {
        return;
    }
    let size = fitted_word_size(painter, word, &family, ceiling, room.width());
    let galley = painter.layout_no_wrap(word.to_owned(), FontId::new(size, family), ink);
    let mut pos = room.center() - galley.size() * 0.5;
    let drawn = galley.mesh_bounds;
    if drawn.is_positive() && drawn.width() <= room.width() {
        // Where the tessellator will put it (`round_text_to_pixels`), and
        // the ink that lands there.
        let ppp = painter.ctx().pixels_per_point();
        let snapped = (pos.x * ppp).round() / ppp;
        let (left, right) = (snapped + drawn.min.x, snapped + drawn.max.x);
        // A thousandth of slack, so float noise is not a pixel.
        if right > room.right() + 1.0e-3 {
            pos.x = snapped - ((right - room.right()) * ppp).ceil() / ppp;
        } else if left < room.left() - 1.0e-3 {
            pos.x = snapped + ((room.left() - left) * ppp).ceil() / ppp;
        }
    }
    painter
        .with_clip_rect(bx.intersect(painter.clip_rect()))
        .galley(pos, galley, ink);
}

/// **The size the arrange gridbox prints at**: 0.55 of the box's height,
/// held to 8 to 11 points. Eleven in the twenty-point box the toolbar draws
/// whenever the arrangement is tall enough for it. A word wider than the
/// box shrinks from here ([`grid_box_word`]).
fn grid_box_type(cell: Rect) -> f32 {
    (cell.height() * 0.55).clamp(8.0, 11.0)
}

/// **The arrange gridbox's word, in its box**: [`grid_box_type`] when it
/// fits, shrunk to the box's width when it does not, and clipped to the box.
///
/// The box is a twenty-point square at most, drawn for three characters
/// (1/2, 1/4, BAR), and at eleven points a fourth is already too many: 1/16
/// is 25.6 points wide in Courier Prime and JetBrains Mono and 21.5 in
/// Terminess. The ladder prints five characters too (1/128, at the finest
/// steps of meters as common as 3/4 and 7/8) and six (1/1024).
fn grid_box_word(painter: &Painter, cell: Rect, word: &str, ink: Color32) {
    paint_fitted_word(
        painter,
        cell,
        cell,
        word,
        crate::fonts::courier_bold(),
        grid_box_type(cell),
        ink,
    );
}

/// **The lane field**: the ground every clip in this view sits on.
///
/// In light mode the timecode WELL is lifted toward the strip face so the
/// lanes read as a surface that clips sit on rather than a hole cut in the
/// desk; in dark mode it stays near the well and is lifted only enough to
/// separate from the seams. Through the dead-zone guard either way, for the
/// reason `legible_surface` gives: the clips and the words that land on this
/// field have to be readable whatever roles it was derived from.
///
/// **Public, and lifted out of [`ink`], because a second thing now needs the
/// answer.** The notification column is dressed for whatever is under the
/// window's top-left corner, and in this view that is this field — but the
/// column is painted at the end of the frame, from the app, with no
/// `ArrangeView` in reach. A derivation copied there would be a second place
/// that decides what the arrangement's ground is, and the two would come apart
/// the first time either was tuned. So there is one, and both callers use it.
/// The frost over a frozen lane: a cool wash that reads on a dark and a
/// light surface alike, and the ink its snowflake is drawn in.
pub const FROST_TINT: Color32 = Color32::from_rgba_premultiplied(48, 72, 96, 72);
pub const FROST_INK: Color32 = Color32::from_rgb(184, 226, 255);

/// A six-spoke snowflake of radius `r` about `c`: three lines through the
/// centre, a pair of short barbs near each tip. Drawn, not typed, so it
/// needs no glyph a face might lack.
pub fn snowflake(painter: &Painter, c: Pos2, r: f32, ink: Color32) {
    let stroke = Stroke::new(1.0_f32, ink);
    for k in 0..3 {
        let a = std::f32::consts::PI / 3.0 * k as f32;
        let d = Vec2::new(a.cos(), a.sin()) * r;
        painter.line_segment([c - d, c + d], stroke);
        let side = Vec2::new(-d.y, d.x) * 0.3;
        for s in [-1.0f32, 1.0] {
            let tip = c + d * s;
            let back = tip - d * s * 0.4;
            painter.line_segment([back, back + side], stroke);
            painter.line_segment([back, back - side], stroke);
        }
    }
}

pub fn lane_field(strip: Color32, well: Color32, dark: bool) -> Color32 {
    crate::recorder_panel::legible_surface(if dark {
        shade(well, 1.55)
    } else {
        blend(well, strip, 0.46)
    })
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

    let lane = lane_field(strip, well, dark);
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
    // **The ruler carries the markers now, so it has to read as its own
    // surface against the band above it — and it did not.**
    //
    // A 55% blend of the band's wood and the lane field is a colour BETWEEN
    // its two neighbours, which is the one place a third surface cannot be
    // seen. On a dark theme both neighbours are near-black and the blend is
    // near-black with them: measured across all seventeen themes in both
    // modes the strip sat between 1.00:1 and 2.29:1 against the band, and
    // fourteen of the thirty-four pairs were under 1.10 — Valentine and
    // Blueprint in dark mode were EXACTLY the same colour, 1.00:1. The
    // owner: "there are some where i can barely discern it from the
    // recorder color, it should be legibly separate."
    //
    // So the blend is where the ruler's character comes from and `separate`
    // is what makes it a surface: away from the band, toward white on a dark
    // one and toward black on a pale one, then checked and pushed further if
    // that was not enough. The step is the same shape the grid, the seam and
    // the alternating stripe already use ten lines above, for the same
    // reason and against the same problem.
    let ruler = {
        const APART: f32 = 1.35;
        // **Through `legible_surface` at every step, because moving a
        // surface is exactly how a surface lands in the dead zone.** The
        // band at ~0.12-0.22 luminance is where neither ink clears 4.5:1,
        // and the bar numbers take their ink FROM this colour: the first cut
        // of this walk pushed Centennial's dark ruler straight into it and
        // dropped the numbers to 3.68:1, which its own test said so. The
        // surface moves, the ink follows, and the surface has to stay
        // somewhere an ink can live.
        let mut r = crate::recorder_panel::legible_surface(
            separate(blend(wood, lane, 0.55), 0.10),
        );
        // A blend can land on the far side of its neighbour, so the walk is
        // measured rather than assumed. Bounded: nine steps of 0.06 is a
        // little over half the way to the extreme, past which a "ruler" is a
        // white bar and the theme is gone.
        for _ in 0..9 {
            if crate::recorder_panel::contrast_ratio(r, wood) >= APART {
                break;
            }
            r = crate::recorder_panel::legible_surface(separate(r, 0.06));
        }
        r
    };

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
    // Through the guard as well: it carries the M/S/R letters, and
    // shading the field 18% darker is exactly the kind of nudge that
    // lands a pale theme in the dead zone where neither ink clears.
    let hdr_lane = crate::recorder_panel::legible_surface(shade(lane, 0.82));
    // The per-frame derivations — the same calls the draw loops made, made
    // here once. `quiet`'s figures are each caller's own and are named at
    // the call sites that used to compute them.
    let switch_off = quiet(on_ink(hdr_lane), hdr_lane, 0.18);
    let empty_ink = quiet(on_ink(empty_ground), empty_ground, 0.22);
    let collapse_ink = quiet(on_ink(hdr_lane), hdr_lane, 0.22);
    let bay_ink = [
        quiet(engrave, strip, 0.08),
        quiet(engrave, strip, 0.34),
        quiet(engrave, strip, 0.45),
    ];
    let ruler_num = quiet(on_ink(ruler), ruler, 0.08);
    let ruler_clock = quiet(on_ink(ruler), ruler, 0.18);
    // Character first, then DISTANCE — see [`auto_ground`] for the whole
    // argument and the measurements behind 0.35 and 0.26.
    let auto_ground_of = |base: Color32| {
        crate::recorder_panel::legible_surface(separate(blend(base, ruler, 0.35), 0.26))
    };
    let auto_ground = [auto_ground_of(lane), auto_ground_of(lane_alt)];
    Ink {
        hdr_wood: wood,
        hdr_face: strip,
        hdr_lane,
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
        switch_off,
        empty_ink,
        collapse_ink,
        bay_ink,
        ruler_num,
        ruler_clock,
        auto_ground,
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

// How many times `quiet` has been asked on this thread, for the test that
// holds a frame to a handful: each ask is a contrast walk, and a frame
// once made 124 of them for answers that do not change between frames.
// Thread-local because the tests run in parallel and a frame is drawn on
// the thread that asked for it.
#[cfg(test)]
thread_local! {
    static QUIET_CALLS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// `pub(crate)` because the MIXER's rack needs it too, for the bypassed bay
/// label: one contrast-guarded dim for the whole app rather than a second
/// copy in the other panel that could be given a different floor by somebody
/// who never saw this one. It lives here because this is where it was
/// written and where every other caller of it still is.
pub(crate) fn quiet(ink: Color32, bg: Color32, t: f32) -> Color32 {
    #[cfg(test)]
    QUIET_CALLS.with(|n| n.set(n.get() + 1));
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

/// **The grid's step in QUARTER NOTES - the one authority on what a rung
/// is.**
///
/// A rung is not a fixed note value. `exp >= 0` is `2^exp` bars, so `0` is
/// one bar whatever the meter. Below that the ladder halves the BAR while the
/// half is still a whole number of beats, drops to the BEAT the first time a
/// half would land between beats, and halves from there:
///
///   4/4  bar, 1/2, 1/4, 1/8 ...     3/4  bar, 1/4, 1/8 ...
///   6/8  bar, dotted 1/4, 1/8 ...   7/8  bar, 1/8, 1/16 ...
///
/// Every rung divides the bar exactly. Every consumer of a grid exponent has
/// to ask this, with the meter it is working in: the arrangement's lines
/// (per bar), the gridbox (`grid_label_text`), the MIDI editor
/// (`midi_editor::grid_step_s`), the quantize `Grid` and the quantize
/// window's chips, and the arrangement's magnet (`TangentApp::snap_quarter`)
/// and arrow step (`arr_grid_s`) all do.
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

/// **The ONE-BAR rung's name, on every surface that prints the ladder: BAR,
/// in every meter.**
///
/// Above it the ladder counts bars (2, 4, 8) and below it are note values
/// (1, 1/2, 1/4 ...), so the bar is the one rung that is neither, and every
/// spelling it had read as something it is not. In 4/4 it was "1", the name
/// of a whole note; in 3/4 "1/2·", a dotted half; in a meter whose bar is no
/// one note it was the time signature (2d292a27; a decimal like "8.000q"
/// before that). The owner met the last of those in 4/2, whose ladder went
/// 2, 4/2, 1: "why does the option between 1 and 2 = 4/2 in grid frequency
/// and why is it even an option?" The rung is there because a grid of one
/// bar is one every song wants (the magnet on the bar lines). What was wrong
/// was its name: "4/2" is a fraction, and one that reads as twice the "1"
/// under it. Of the spellings put to him, the owner chose this one.
///
/// A WORD, so it cannot be read as a count of bars or as a note value, and
/// the same word in every meter, 4/4 included, so nobody has to know the
/// meter to read it. Three letters, so it is exactly as wide as the
/// three-character rungs (1/2, 1/4, 1/8) in all three bundled faces, which
/// are monospaced: the twenty-point arrange box is sized for those, and
/// `the_bar_rung_is_painted_bar_in_every_meter` measures it on the frame in
/// each face. Aloud it is "1 bar", as it always was ([`grid_spoken`]).
pub const GRID_BAR: &str = "BAR";

/// **What the box prints, derived from what the magnet actually does** —
/// in the spelling that keeps the note glyph at the quarter, which is now
/// the MIDI EDITOR's chip and nothing else. The arrange gridbox takes
/// [`grid_label_text`]; the argument for the split is written out there.
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
/// **The glyph is not what enforces that**, and the comment this replaces
/// said it was — "kept for the quarter note alone, which is what makes it
/// the single 1/4 state". That was true of the code it was written beside
/// and false about the mechanism. The duplicate 1/4 was a symptom of the
/// label being a SECOND OPINION about the division; deriving it from
/// `grid_quarters` is what killed the duplicate, and once the derivation is
/// the rule only one exponent can ever come back as one quarter, whatever
/// character that rung is drawn in. The glyph was standing guard over a hole
/// that had already been filled — so removing it from the arrange box costs
/// nothing the ruling was protecting, and
/// `only_one_rung_of_the_arrange_ladder_is_a_quarter` now holds that
/// property as a test rather than as a character.
///
/// What is left is a spelling, and this spelling stays HERE because the
/// editor's chip is not what the owner asked about. It stays `\u{266a}`
/// rather than the "more correct" `\u{2669}`, though not for the reason the
/// old note gave — it said U+266A was "known to render in the panel's font",
/// and reading the cmap of both weights shipped in `assets/fonts` says
/// Courier Prime carries NEITHER (it does carry the U+00B7 the dotted rungs
/// wear, and Δ). Both come off egui's appended fallback face, so the choice
/// is which character that face is likeliest to have: U+266A, the one that
/// has been drawing on every platform this ships to. A hollow box is worse
/// than approximate, and it is worth knowing this rung depends on a fallback
/// at all — the arrange box's numerals are ASCII and depend on nothing.
pub fn grid_label(exp: i8, beats: u8, unit: u8) -> String {
    grid_label_with(exp, beats, unit, true)
}

/// **The same ladder, spelled in numerals the whole way down** — what the
/// ARRANGE gridbox prints.
///
/// The owner, on that box: "it's easier to spot at a glance for a beginner
/// and realize that's what that button does." In 4/4 the glyph spelling
/// reads BAR, 1/2, ♪, 1/8, 1/16. Under the bar, the glyph is the ONE note
/// value not written the way the ones on either side of it are, and it sits
/// at the division a beginner is most likely to be hunting for, because a
/// quarter note is the first thing anyone is taught to count. `tool_grid`
/// is a square of at most 20 points (it was 48 wide when this was written)
/// and the label is drawn between 8 and 11 points, so ♪ arrives as a stem
/// and a blob that has to be recognised before it can be read; "1/4"
/// arrives as a fraction the two rungs under it have already taught the eye
/// to parse.
///
/// This supersedes the earlier ruling quoted on [`grid_label`], and only its
/// second half: "there should exist a single state of that button that
/// produces 1/4th" is still the law here and still holds — it is the
/// derivation from `grid_quarters` that holds it — while "and that's the
/// glyph" is the part being overruled. Anyone reading 629955e's commit
/// message and reaching for the glyph again should read that paragraph
/// first.
///
/// Two functions rather than one with a flag in the caller's hands, because
/// the callers are two panels and neither should have to know the other's
/// taste to get its own label. Both are one line over the same
/// `grid_quarters`/`note_label` pair, so the two spellings cannot drift
/// apart in the ARITHMETIC — the only place a drift would be a bug rather
/// than a preference.
pub fn grid_label_text(exp: i8, beats: u8, unit: u8) -> String {
    grid_label_with(exp, beats, unit, false)
}

/// The one ladder both public labels walk. Kept private and shared so the
/// bar branches (the multiples of a bar, counted, and the bar itself,
/// [`GRID_BAR`]: the half of the label that has nothing to do with note
/// glyphs) exist once; two copies of `1u32 << exp` is exactly the kind of
/// duplicate that let the label disagree with the magnet in the first place.
fn grid_label_with(exp: i8, beats: u8, unit: u8, quarter_glyph: bool) -> String {
    if exp > 0 {
        // Multiples of a BAR, and it says so by counting them.
        return format!("{}", 1u32 << exp.min(6) as u32);
    }
    if exp == 0 {
        // One bar, by name, whatever the meter: see `GRID_BAR`. Not handed
        // to `note_label`, which can only name the bar's LENGTH, as a note
        // (4/4's "1", 3/4's "1/2·") or counted in beats (4/2's "4/2"), and
        // every one of those reads as a note value.
        return GRID_BAR.to_owned();
    }
    note_label(grid_quarters(exp, beats, unit), unit, quarter_glyph)
}

/// A length in quarter notes, as a note value.
///
/// Whole = 4 quarters, half = 2, quarter = 1, and so on down. A dotted value
/// is one and a half of the next one up and wears a dot; 6/8's second rung is
/// a dotted quarter and there is no other honest way to write it.
///
/// **A length no note names is COUNTED, in the meter's own beats**: `n/unit`,
/// n notes of 1/unit, the way a time signature counts its bar. Every rung
/// [`grid_quarters`] walks is a whole number of beats or a power-of-two part
/// of one, and a part of one always has a name, so the count is exact. The
/// BAR rung never arrives here (it is [`GRID_BAR`], by name, from
/// [`grid_label_with`]), so what is counted is a rung UNDER the bar that no
/// note names: 10/8's half bar, five eighths, is 5/8. `unit` is read by this
/// branch alone; the fallback it replaced is written up in the branch.
///
/// **`quarter_glyph` decides one rung and one rung only**: whether q == 1
/// comes back as `\u{266a}` or falls through into the plain-fraction branch,
/// which already spells that same rung "1/4" out of the division rather than
/// out of a second table of its own. Every other string (the whole, the
/// plain fractions, the dotted values and the counted lengths) is identical
/// on both paths, so the two panels' labels differ in exactly the one way
/// they were asked to differ and in no other.
///
/// The DOTTED branch in particular is deliberately outside the flag's reach.
/// 6/8's second rung is "1/4·" under both spellings and has to be: let the
/// flag reach it and the numeral ladder would carry two things called 1/4
/// again — plain and dotted, at two different divisions — which is precisely
/// the fault the derivation above was written to end.
fn note_label(q: f64, unit: u8, quarter_glyph: bool) -> String {
    if !q.is_finite() || q <= 0.0 {
        return "1".to_owned();
    }
    // The glyph IS the quarter note, and only the quarter note — for the
    // caller that wants a glyph at all.
    if quarter_glyph && (q - 1.0).abs() < 1.0e-9 {
        return "\u{266a}".to_owned();
    }
    match note_value(q, unit) {
        NoteValue::Plain(d) if d <= 1 => "1".to_owned(),
        NoteValue::Plain(d) => format!("1/{d}"),
        // Dotted: one and a half of a clean value.
        NoteValue::Dotted(d) if d <= 1 => "1\u{00b7}".to_owned(),
        NoteValue::Dotted(d) => format!("1/{d}\u{00b7}"),
        // Nothing nameable: count it, in beats of the meter.
        //
        // This used to say the length in quarters, `format!("{q:.3}q")`, and
        // that is what the owner found: "there's a '0 frequency on the grid
        // frequency in arrange mode reads as quite a lot of 0.000000s". In
        // 4/2 the bar rung printed "8.000q", six characters in a box drawn
        // for three: the 8 spilled onto the tempo-lane switch, where it
        // barely shows, and the navigator, painted after it, covered the q,
        // so a row of zeros was what read. Nor was it one rare meter: 5/4,
        // 7/8 and 9/8, three of the eight in `TIME_SIGNATURES`, reached it at
        // the same rung. `no_rung_of_any_signature_is_spelled_as_a_decimal`
        // sweeps them all. The bar rung has since been given a name of its
        // own, `GRID_BAR`, and no longer comes here; what still does is a
        // rung under the bar that no note names, like 10/8's five eighths.
        NoteValue::Counted(n, unit) => format!("{n}/{unit}"),
    }
}

/// **What a length in quarters is, as a note**: one note value, one dotted
/// note value, or a count of the meter's beats.
///
/// The one decision [`note_label`] prints and [`grid_spoken`] says aloud, so
/// the box and the screen reader cannot sort a step differently. The reader
/// used to sort it a second time, by matching the box's text against a
/// table of eight spellings, and every step the table did not list fell
/// through to "of a bar": 428 of the (signature, step) pairs, a whole note
/// read as "1 of a bar" in 4/2 and a 128th as "1/128 of a bar" in 4/4.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NoteValue {
    /// One 1/d note: 1 is a whole note, 2 a half, 4 a quarter.
    Plain(u64),
    /// A dotted 1/d note, one and a half of it: 6/8's beat is `Dotted(4)`.
    Dotted(u64),
    /// n notes of 1/unit, the meter's own beat: 10/8's half bar is
    /// `Counted(5, 8)`.
    Counted(u64, u8),
}

/// Sorts `q` quarter notes into a [`NoteValue`]: a plain note when `4 / q`
/// is a whole number, a dotted one when two thirds of `q` is, and otherwise
/// counted in the meter's beats ([`beats_counted`]).
fn note_value(q: f64, unit: u8) -> NoteValue {
    let denom = 4.0 / q;
    let rounded = denom.round();
    if (denom - rounded).abs() < 1.0e-9 && rounded >= 1.0 {
        return NoteValue::Plain(rounded as u64);
    }
    let d = 4.0 / (q * 2.0 / 3.0);
    let dr = d.round();
    if (d - dr).abs() < 1.0e-9 && dr >= 1.0 {
        return NoteValue::Dotted(dr as u64);
    }
    let (n, unit) = beats_counted(q, unit);
    NoteValue::Counted(n, unit)
}

/// **A length in quarters, counted in the meter's beats**: `(n, unit)`, n
/// notes of 1/unit. What [`note_label`] prints when no single note names the
/// length, and what [`grid_spoken`] reads back to say it in words, so the
/// box and the reader cannot count differently.
///
/// Rounded because a float's last bit is not a beat, and never below one
/// because a grid step is never nothing.
fn beats_counted(q: f64, unit: u8) -> (u64, u8) {
    let unit = unit.max(1);
    let n = (q * f64::from(unit) / 4.0).round().max(1.0);
    (n as u64, unit)
}

/// **What a 1/d note is called aloud**, for every d the grid can reach: the
/// note values a signature counts in (whole to thirty-second) and every
/// halving under them the ladder walks to, down to the 4096th note that the
/// finest step of 1/32 is.
///
/// Said the way a player says them, "hundred twenty-eighth" and not "one
/// slash one hundred twenty-eight": a reader that has to spell a fraction
/// out has not named the note.
fn note_word(d: u64) -> Option<&'static str> {
    Some(match d {
        1 => "whole",
        2 => "half",
        4 => "quarter",
        8 => "eighth",
        16 => "sixteenth",
        32 => "thirty-second",
        64 => "sixty-fourth",
        128 => "hundred twenty-eighth",
        256 => "two hundred fifty-sixth",
        512 => "five hundred twelfth",
        1024 => "thousand twenty-fourth",
        2048 => "two thousand forty-eighth",
        4096 => "four thousand ninety-sixth",
        _ => return None,
    })
}

/// The same grid, in the words a person would say it in.
///
/// The box prints "8" and "1/4" because it is a box the width of a thumb.
/// A screen reader announcing "Grid, 8" has said nothing: the unit is the
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
        // The box prints BAR here (`GRID_BAR`); aloud it is one bar, in
        // every meter, as it was before the box had a word for it.
        return "1 bar".to_owned();
    }
    let q = grid_quarters(exp, beats, unit);
    // Spoken in words rather than in the box's shorthand: a reader saying
    // "one slash eight" has not said "an eighth note".
    //
    // **Sorted by `note_value`, the decision the box prints**, rather than
    // by reading the box's text back. It read the text, against a table of
    // eight spellings, and everything the table lacked fell through to
    // "{label} of a bar": in 4/2 the whole note under the bar was "1 of a
    // bar", right after the bar's "1 bar"; 12/8's dotted half was "1/2· of a
    // bar", a dot read aloud; and the finest steps were "1/128 of a bar" and
    // finer, each a fraction of a bar that was not one. 428 (signature,
    // step) pairs in all, in 182 of the 192 meters. Before that table it
    // was the quarter: the glyph spelling sent "♪ of a bar".
    // `every_step_of_every_meter_is_said_as_a_note` sweeps all 192 meters.
    let counted = |n: u64, u: u8| match note_word(u64::from(u)) {
        // **A counted step is said as the notes it counts.** The box prints
        // 10/8's half bar as "5/8"; as "5/8 of a bar" that is a wrong
        // fraction (it is HALF the bar) said with confidence.
        Some(word) if n == 1 => format!("1 {word} note"),
        Some(word) => format!("{n} {word} notes"),
        // A unit no signature has; the app never builds one.
        None => format!("{n} beats"),
    };
    let named = match note_value(q, unit) {
        NoteValue::Plain(d) => note_word(d).map(|word| format!("{word} note")),
        NoteValue::Dotted(d) => note_word(d).map(|word| format!("dotted {word} note")),
        NoteValue::Counted(n, u) => Some(counted(n, u)),
    };
    named.unwrap_or_else(|| {
        // A note the table has no name for, which no valid meter reaches:
        // said as a count of the meter's beats rather than as a fraction.
        let (n, u) = beats_counted(q, unit);
        counted(n, u)
    })
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
fn tempo_x_of(l: &Layout, view: &ArrangeView<'_>, lane: Rect, at_beat: f64) -> f32 {
    let x = x_of_in(l, view, view.tempo.seconds_at_beat(at_beat));
    if !x.is_finite() {
        return x;
    }
    x.max(lane.left() + AUTO_POINT_R)
}

/// Where a bar LINE falls, in seconds. Bars count from zero here and from
/// one on the ruler, the way musicians write them.
pub(crate) fn bar_at(view: &ArrangeView<'_>, bar: u32) -> f64 {
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
fn auto_cells(view: &ArrangeView<'_>, lane: LaneId, header: Rect) -> AutoHeader {
    let indent = if lane.row().and_then(|r| view.lanes.get(r)).is_some_and(|l| l.in_folder) {
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
                    Stroke::new(1.0_f32, auto_header_seam(&p)),
                );
            }
            if auto.selected {
                painter.rect_stroke(
                    row.header.shrink(1.0),
                    2.0,
                    Stroke::new(1.2_f32, crate::recorder_panel::selection_rim(face)),
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
                let stroke = Stroke::new(crate::fonts::px((h.close.width() * 0.10) * crate::fonts::size_scale(painter.ctx())), quiet(ink, face, 0.28));
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
                painter.circle_stroke(c, rad, Stroke::new(1.0_f32, quiet(ink, face, 0.30)));
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
                let size = crate::fonts::px((h.bypass.height() * 0.62).clamp(7.0, 11.0) * crate::fonts::size_scale(painter.ctx()));
                if size >= 7.0 {
                    painter.text(
                        h.bypass.center(),
                        Align2::CENTER_CENTER,
                        "M",
                        FontId::new(size, crate::fonts::courier_bold()),
                        // Off is on `hdr_lane`, whose quieted ink `ink`
                        // already worked out once for every switch.
                        if on { on_ink(plate) } else { p.switch_off },
                    );
                }
            }
            // What it controls.
            let size = crate::fonts::px((row.header.height() * 0.46).clamp(7.0, 11.0) * crate::fonts::size_scale(painter.ctx()));
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
                fit_text(
                    painter,
                    h.readout,
                    &format!("{:.0}%", auto.resting * 100.0),
                    &crate::fonts::courier(),
                    quiet(ink, face, 0.22),
                    0.92,
                    (7.0, 12.0),
                );
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
fn auto_seam(p: &Ink, lane: LaneId) -> Color32 {
    separate(auto_ground(p, lane), 0.22)
}

/// The header half of the same seam. A row is one object across the grip, so
/// the line has to be visible on both surfaces — and they are different
/// surfaces, so it is not the same colour.
fn auto_header_seam(p: &Ink) -> Color32 {
    separate(shade(p.hdr_face, 0.92), 0.22)
}

fn auto_ground(p: &Ink, lane: LaneId) -> Color32 {
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
    //
    // Two answers per frame, not one per row per loop: the base is the
    // lane's stripe, and there are two stripes. `ink` derives both.
    p.auto_ground[lane.ground()]
}

/// **The colour a detected attack is marked in.** Amber, and not the
/// clip's own ink: these are an answer laid OVER the picture, and one drawn
/// in the picture's colour reads as part of it. Named so a test can find
/// them among everything else vertical on the surface.
pub(crate) const MARK_INK: Color32 = Color32::from_rgb(0xFF, 0xC3, 0x4D);

/// **The quantize window's marks over one clip's body.**
///
/// `(found, target)` in seconds from the start of the FILE - the clip's own
/// trim is subtracted here, once, so a trimmed take marks the same attacks
/// as an untrimmed one. Anything outside the drawn body is skipped rather
/// than clamped: a tick pinned to the edge would claim an attack at a place
/// there is none.
///
/// **Tall, and full height.** These are answers to "where", and a mark that
/// only reaches the middle of the wave reads as part of the wave.
fn draw_clip_marks(
    painter: &Painter,
    r: Rect,
    clip: &ClipView<'_>,
    l: &Layout,
    view: &ArrangeView<'_>,
    ink: Color32,
) {
    // A mark is a MOVE when it would land more than a pixel from where it
    // was played; below that the two ticks would sit on each other.
    let t0 = time_at_in(l, view, r.left());
    let t1 = time_at_in(l, view, r.left() + 1.0);
    let per_px = (t1 - t0).abs().max(1.0e-9);
    let found_ink = MARK_INK;
    let target_ink = ink.gamma_multiply(0.55);
    let (top, bot) = (r.top() + 1.0, r.bottom() - 1.0);
    // **Every pass of a loop, not just the first.** A looped clip plays its
    // section over and over and the wave is drawn that way (`wave_band`
    // folds the same way), so ticks on the first pass alone would say the
    // attacks stop after one bar. Clamped, because `len_s / loop_s` on a
    // very short loop is a number nobody wants to iterate.
    let looped = clip.loop_s > 1.0e-6 && clip.loop_s.is_finite();
    let passes: usize = if looped {
        ((clip.len_s / clip.loop_s).ceil() as usize).clamp(1, 512)
    } else {
        1
    };
    // A hair either side of a pass's span, for the fold below: the host
    // rounds the trim to a whole frame and sends seconds as `f32`, so a hit
    // ON the loop's first frame can arrive a fraction of a frame before
    // `offset_s`, and dropping it would take the downbeat off every pass.
    const SEAM_SLACK_S: f64 = 5.0e-4;
    for pass in 0..passes {
        // Zero for an unlooped clip, whose `loop_s` may not even be finite -
        // `inf * 0.0` is NaN, and a NaN x is a line drawn nowhere in
        // particular.
        let shift = if looped { clip.loop_s * pass as f64 } else { 0.0 };
        for &(found_s, target_s) in clip.marks {
            // **Only what THIS pass plays**: `offset_s .. offset_s +
            // loop_s`, the span `wave_band` folds the pass's picture into.
            // Every pass drew every mark it was handed, so one from outside
            // the loop stood in the next pass's audio, over a place the loop
            // plays something else. The host sends only what the clip plays,
            // but in the axis it anchors in; a hit the current warp carries
            // across the seam is projected outside the span, and the fold is
            // the painter's to keep, as the picture's is.
            if looped {
                let into = f64::from(found_s) - clip.offset_s;
                if !(into > -SEAM_SLACK_S && into < clip.loop_s + SEAM_SLACK_S) {
                    continue;
                }
            }
            // Source seconds -> the clip's own drawn timeline.
            let at = clip.start_s + (f64::from(found_s) - clip.offset_s) + shift;
            let to = clip.start_s + (f64::from(target_s) - clip.offset_s) + shift;
            let x = x_of_in(l, view, at);
            if x < r.left() || x > r.right() {
                continue;
            }
            if (to - at).abs() > per_px {
                let tx = x_of_in(l, view, to);
                if tx >= r.left() && tx <= r.right() {
                    painter.line_segment(
                        [Pos2::new(tx, top), Pos2::new(tx, bot)],
                        Stroke::new(1.0_f32, target_ink),
                    );
                }
            }
            painter.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bot)],
                Stroke::new(1.0_f32, found_ink),
            );
        }
    }
}

/// **A clip's waveform from its pyramid, as one mesh.**
///
/// Two things at once, and the second is the larger.
///
/// The picture comes from `peaks` rather than from a host-drawn span: for
/// every column the panel works out which seconds of the SOURCE stand under
/// it — the clip's trim, and for a looped clip the fold back into its loop —
/// and asks the pyramid for the lowest and highest sample there. So the
/// wave is right at the zoom it is drawn at, on the frame the window moves,
/// and the host is never asked to walk samples for a view change. The fold
/// is the engine's own rule and the old host picture's: pass `k` plays
/// `offset_s .. offset_s + loop_s` of the file again, and a column that
/// straddles a seam reads on past the seam rather than wrapping, which is
/// what `looped_span_envelope` did and what
/// `a_looped_audio_clip_repeats_its_first_pass` holds.
///
/// And it is ONE shape. The column loop emitted a feathered `LineSegment`
/// per pixel per clip — thirteen thousand shapes on a 1920-wide window with
/// eight clips, each tessellated as a stroke with end caps (eight vertices,
/// sixteen triangles), 0.63 ms a frame before a single pixel was painted.
/// A min/max band is two vertices per column and two triangles between
/// neighbours; the tessellator passes a mesh through. The look is kept:
/// at each column the band spans exactly the rows the stroke did (held to
/// a point by `the_band_from_the_pyramid_covers_the_rows_the_columns_did`),
/// and between columns it joins them, which is the outline a waveform has
/// always been. A silent column keeps a one-point floor so a flat stretch
/// still reads as a line at every pixel density.
///
/// A column outside the clip's own seconds (a rounding hair at either end)
/// breaks the band, and the next one starts a new run — never a triangle
/// across a gap. The pyramid's answer is finite and clamped by contract, so
/// no column can carry a NaN into the mesh.
fn wave_band(
    painter: &Painter,
    r: Rect,
    clip: &ClipView<'_>,
    peaks: &crate::peaks::Peaks,
    t0: f64,
    dt: f64,
    ink: Color32,
) {
    let mid = r.center().y;
    let half = r.height() * 0.40;
    let cols = r.width().ceil() as usize;
    let looped = clip.loop_s > 1.0e-6 && clip.loop_s.is_finite();
    let mut mesh = egui::epaint::Mesh::default();
    mesh.reserve_vertices(cols * 2);
    mesh.reserve_triangles(cols * 2);
    // The previous column's top vertex in the run being built, if any.
    let mut prev: Option<u32> = None;
    for i in 0..cols {
        let x = r.left() + i as f32;
        if x > r.right() {
            break;
        }
        let t = t0 + f64::from(i as f32) * dt;
        let into = t - clip.start_s;
        if !into.is_finite() || into < 0.0 || into >= clip.len_s {
            prev = None;
            continue;
        }
        let src = clip.offset_s + if looped { into.rem_euclid(clip.loop_s) } else { into };
        if !src.is_finite() {
            prev = None;
            continue;
        }
        let (lo, hi) = peaks.minmax_s(src, src + dt);
        // **Clamped, not just scaled.** A clip lifted past full scale would
        // otherwise draw outside its own band and over its neighbour, and
        // the picture would say "louder" by spilling rather than by being
        // taller. Pinning at the edge is also what a hand expects from a
        // take that has been pushed as far as it goes.
        let (lo, hi) = (
            (lo * clip.gain).clamp(-1.0, 1.0),
            (hi * clip.gain).clamp(-1.0, 1.0),
        );
        let y0 = mid - hi * half;
        let y1 = (mid - lo * half).max(y0 + 1.0);
        let top = mesh.vertices.len() as u32;
        mesh.colored_vertex(Pos2::new(x, y0), ink);
        mesh.colored_vertex(Pos2::new(x, y1), ink);
        if let Some(p) = prev {
            mesh.add_triangle(p, p + 1, top);
            mesh.add_triangle(p + 1, top + 1, top);
        }
        prev = Some(top);
    }
    if !mesh.indices.is_empty() {
        painter.add(egui::Shape::mesh(mesh));
    }
}

/// **The trailing add's ground, run on to the foot of the track area.**
///
/// Since rows nobody has made stopped being drawn (design section 10), a
/// desk shorter than the window ends at its trailing add, and past it the
/// plain panel showed: the header column's wood under the add's own
/// `empty_ground`, and the field's wood under its stripe. The owner: "i
/// want the unlimited track mode to have continuity of the color used for
/// the background of the track adder button below tracks in the arrange
/// view ... can we make the draw just infinite?"
///
/// So the add's two grounds are painted TALLER, each to the foot of its
/// own column: `header` is its header panel run down to the bottom of the
/// header column (it wears `empty_ground` there), `lane` is its lane run
/// down to the bottom of the lanes (it wears `lane`, the field's own
/// ground, whatever row the add stands on: see `field_ground`). One rect a
/// column however much room is left, and each IS the add's own ground, so
/// the add and the room under it read as one surface: the seam under the
/// add is not drawn, and its slot box closes round the whole run rather
/// than across the add's foot.
///
/// **Paint, and nothing else.** The add's cells (its `HeaderRow::panel`,
/// its lane row, its plus) are what a press is tested against and they are
/// not touched, so a press under the add answers exactly what it did when
/// the wood showed there, and the scroll range never counts the run.
/// `None` when the add is not placed, is not the last row placed, or ends
/// at the foot already (a desk taller than the window): nothing changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EndlessGround {
    /// The trailing add's row.
    pub row: usize,
    /// Its header panel, run down to the foot of the header column.
    pub header: Rect,
    /// Its lane, run down to the foot of the lanes.
    pub lane: Rect,
}

/// See [`EndlessGround`].
pub fn endless_ground(l: &Layout, view: &ArrangeView<'_>) -> Option<EndlessGround> {
    // The last row the layout placed. The add ranks after every lane and
    // has no automation rows, so when it is on screen it is this one.
    let last = l.rows.iter().rev().find(|r| r.lane.is_positive())?;
    let RowKind::Track(row) = last.kind else {
        return None;
    };
    if !view.lanes.get(row).is_some_and(is_trailing_add) {
        return None;
    }
    let panel = l.header_rows.get(row)?.panel;
    // Within the half point a row may overhang the area in `Layout::new`: a
    // stack that ends flush has nothing under it to fill.
    if !panel.is_positive() || l.lanes_area.bottom() - last.lane.bottom() <= 0.5 {
        return None;
    }
    Some(EndlessGround {
        row,
        header: Rect::from_min_max(panel.min, Pos2::new(panel.right(), l.header.bottom())),
        lane: Rect::from_min_max(
            last.lane.min,
            Pos2::new(last.lane.right(), l.lanes_area.bottom()),
        ),
    })
}

/// Whether `lane` is the desk's trailing add, open or locked: no track,
/// and not a row's own empty plus. What the add wears in the field and
/// where its ground runs on are both read off this, so the two cannot
/// disagree about which row is the add.
fn is_trailing_add(lane: &LaneView<'_>) -> bool {
    !lane.used && lane.plus != crate::mixer_panel::Plus::Row
}

/// **The ground row `row` wears in the field.**
///
/// A track's row wears one of the two stripes by its row, so eight rows
/// read as eight. The trailing add wears `lane`, the field's own ground,
/// whatever row it stands on. It is no row yet (it wears no number
/// either), and its row is the desk's length, which a removal, a track
/// opened between two others and a track that grows the desk all change.
/// Striped by that row, the add took the other stripe whenever the length
/// changed by an odd count, and since its ground runs on to the foot of
/// the lanes (`EndlessGround`), the largest surface on the timeline
/// changed shade with it: the colour the owner asked to carry on held only
/// until the desk changed length. `lane` is the stripe the add wore on the
/// shipped desk and on an empty one, so those look as they did.
fn field_ground(p: &Ink, view: &ArrangeView<'_>, row: usize) -> Color32 {
    if row % 2 == 0 || view.lanes.get(row).is_some_and(is_trailing_add) {
        p.lane
    } else {
        p.lane_alt
    }
}

/// Paint the whole view into `rect`.

pub fn draw(painter: &Painter, rect: Rect, view: &ArrangeView<'_>) {
    let l = Layout::new(rect, view);
    if !l.lanes_area.is_positive() {
        return;
    }
    let p = ink(view);
    painter.rect_filled(rect, 0.0, p.wood);
    // The trailing add's ground where it runs on below the add, read by the
    // stripes, the seams, the header plates and the slot boxes alike so the
    // four cannot disagree about where the add ends. See `EndlessGround`.
    let endless = endless_ground(&l, view);
    let endless_at = |i: usize| endless.filter(|g| g.row == i);

    // ── the lanes' field, striped so eight rows read as eight ─────────────
    for (i, row) in l.lane_rows.iter() {
        // The trailing add's ground runs on to the foot of the lanes, and
        // is the field's own ground whatever row the add stands on (see
        // `field_ground`).
        let ground = endless_at(i).map_or(*row, |g| g.lane);
        painter.rect_filled(ground, 0.0, field_ground(&p, view, i));
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
                Stroke::new(1.0_f32, auto_seam(&p, lane)),
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
    let nums = number_row(l.ruler, l.meter_strip);
    let font = FontId::new(((nums.height() * 0.76).clamp(7.0, 12.0)) * crate::fonts::size_scale(painter.ctx()), crate::fonts::courier());
    // **Bounded.** The step is chosen so this is a few dozen iterations, and
    // the bound is here because a hand-edited map with a one-beat bar in it
    // could otherwise ask the UI thread for millions.
    let mut bars: Vec<(u32, f32)> = Vec::new();
    let mut n = first;
    while n <= last && bars.len() < 512 {
        let x = x_of_in(&l, view, bar_at(view, n));
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
            Stroke::new(1.0_f32, p.faint),
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
        // **Ink measured on the RULER, not `faint`.**
        //
        // `faint` is a blend toward the strip face, and on three themes it
        // lands almost exactly on the ruler's own tone: 1.29:1 on Default
        // light, 1.57:1 on Centennial dark, 1.65:1 on Porcelain dark, where
        // the clock beside it reads 4.4:1 or better. The owner reported it
        // on Centennial — "the beats are way too light and illegible
        // compared to the seconds" — and Default, the theme that ships, was
        // the worst of the three. So the fix is the derivation and not one
        // palette: the same `on_ink(p.ruler)` the clock already uses, held
        // one notch STRONGER than the clock, because bars are what this
        // ruler is for and the seconds are the second opinion.
        painter.text(
            Pos2::new(x + 3.0, nums.top()),
            Align2::LEFT_TOP,
            format!("{}", n.saturating_add(1)),
            font.clone(),
            p.ruler_num,
        );
    }
    // **The clock row: h:mm:ss along the ruler's foot** — the owner's
    // ask: bars answer "where in the music", this answers "where in the
    // afternoon". The interval widens as the view does.
    {
        let clk = clock_row(l.ruler, l.meter_strip);
        let font = FontId::new(((clk.height() * 0.74).clamp(6.5, 10.0)) * crate::fonts::size_scale(painter.ctx()), crate::fonts::courier());
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
            let x = x_of_in(&l, view, t);
            if x >= l.lanes_area.left() - 0.5 && x <= l.lanes_area.right() - 30.0 {
                let secs = t.round() as u64;
                let word = if secs >= 3600 {
                    format!("{}:{:02}:{:02}", secs / 3600, (secs % 3600) / 60, secs % 60)
                } else {
                    format!("{}:{:02}", secs / 60, secs % 60)
                };
                painter.text(
                    Pos2::new(x + 3.0, clk.bottom() - 1.0),
                    Align2::LEFT_BOTTOM,
                    word,
                    font.clone(),
                    p.ruler_clock,
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
                    Stroke::new(if w >= 1.0 { 1.6_f32 } else { 1.0_f32 }, p.grid.gamma_multiply(w)),
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
            let x0 = x_of_in(&l, view, t0);
            let x1 = x_of_in(&l, view, view.tempo.seconds_at_beat(q0 + qn));
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
                        line(x_of_in(&l, view, t), 0.55);
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
    for (i, row) in l.lane_rows.iter() {
        // Not under the trailing add when its ground runs on below it: that
        // is no boundary, only more of the same surface.
        if endless_at(i).is_some() {
            continue;
        }
        let left = l
            .header_rows
            .get(i)
            .map_or(row.left(), |h| h.panel.left());
        painter.line_segment(
            [
                Pos2::new(left, row.bottom()),
                Pos2::new(row.right(), row.bottom()),
            ],
            Stroke::new(1.0_f32, p.seam.gamma_multiply(1.25)),
        );
    }

    // ── the lane a hovering file would land on ────────────────────────────
    //
    // Drawn UNDER the clips and over the field, so it reads as the lane
    // lighting up rather than as something laid on top of the music.
    if let Some(row) = drop_lane_rect(&l, view) {
        painter.rect_filled(row, 0.0, p.select.gamma_multiply(0.16));
        painter.rect_stroke(
            row,
            0.0,
            Stroke::new(1.5_f32, p.select.gamma_multiply(0.85)),
            egui::StrokeKind::Inside,
        );
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
        let (cr, cg, cb) = STRIP_COLORS[lane
            .row()
            .and_then(|r| view.lanes.get(r))
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
        // **The box being drawn to choose nodes**, under the curve so the
        // nodes it lights stay on top of it. Clipped to the row: a box is a
        // question about THIS row's nodes.
        if let Some(m) = view.auto_marquee.filter(|m| m.lane == lane && m.slot == slot) {
            let (xa, xb) = (x_of_in(&l, view, m.t0), x_of_in(&l, view, m.t1));
            let (ya, yb) = (auto_y_of(row.lane, m.v0), auto_y_of(row.lane, m.v1));
            if xa.is_finite() && xb.is_finite() && ya.is_finite() && yb.is_finite() {
                let r = Rect::from_two_pos(Pos2::new(xa, ya), Pos2::new(xb, yb))
                    .intersect(row.lane);
                if r.is_positive() {
                    let rim = crate::recorder_panel::selection_rim(ground);
                    painter.rect_filled(r, 0.0, rim.gamma_multiply(0.14));
                    painter.rect_stroke(r, 0.0, Stroke::new(1.0_f32, rim), egui::StrokeKind::Inside);
                }
            }
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
                Stroke::new(1.2_f32, curve.gamma_multiply(0.55)),
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
            let at = Pos2::new(x_of_in(&l, view, q.at_s), auto_y_of(row.lane, q.value));
            line.push(at);
            if q.shape == crate::automation::Shape::Hold {
                if let Some(next) = shown.get(i + 1) {
                    line.push(Pos2::new(x_of_in(&l, view, next.at_s), at.y));
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
            painter.line_segment([a, b], Stroke::new(1.5_f32, curve));
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
            let at = Pos2::new(x_of_in(&l, view, q.at_s), auto_y_of(row.lane, q.value));
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
            painter.circle_stroke(at, AUTO_POINT_R, Stroke::new(1.0_f32, ground));
            // **Chosen wears the selection rim**, the one a chosen track and
            // a chosen automation row already wear — outside the ground ring
            // so it reads at the sizes these handles actually are.
            if chosen {
                painter.circle_stroke(
                    at,
                    AUTO_POINT_R + 2.0,
                    Stroke::new(1.4_f32, crate::recorder_panel::selection_rim(ground)),
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
                let size = crate::fonts::px((row.lane.height() * 0.34).clamp(8.0, 12.0) * crate::fonts::size_scale(painter.ctx()));
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
                Stroke::new(1.0_f32, p.seam),
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
                let x = x_of_in(&l, view, view.tempo.seconds_at_beat(q.at_beat));
                let at = Pos2::new(x, tempo_y_of(row.lane, q.bpm));
                line.push(at);
                if let Some(next) = pts.get(i + 1) {
                    let nx = x_of_in(&l, view, view.tempo.seconds_at_beat(next.at_beat));
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
                painter.line_segment([a, b], Stroke::new(1.5_f32, curve));
            }
            let font = FontId::new(
                (row.lane.height() * 0.34).clamp(7.0, 11.0),
                crate::fonts::courier(),
            );
            for (i, q) in pts.iter().enumerate() {
                let x = tempo_x_of(&l, view, row.lane, q.at_beat);
                let at = Pos2::new(x, tempo_y_of(row.lane, q.bpm));
                if !at.x.is_finite() || !at.y.is_finite() {
                    continue;
                }
                if at.x < row.lane.left() - AUTO_POINT_R || at.x > row.lane.right() + AUTO_POINT_R {
                    continue;
                }
                let hot = view.tempo_hot == Some(i);
                painter.circle_filled(at, if hot { AUTO_POINT_R + 1.0 } else { AUTO_POINT_R }, curve);
                painter.circle_stroke(at, AUTO_POINT_R, Stroke::new(1.0_f32, ground));
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
        let names = flag_row(l.meter_strip);
        let sigs = sig_row(l.meter_strip);
        let font = FontId::new(strip_font_h(sigs), crate::fonts::courier());
        let box_h = strip_box_h(sigs);
        for pt in view.meter.points() {
            let typing = view.meter_edit.filter(|(b, _)| *b == pt.at_bar);
            // Bar 0's signature is the song's, not a CHANGE — there is
            // nothing before it for it to be different from, and a chip on
            // the first bar of every song would be noise on the overwhelming
            // majority that never change.
            //
            // **Unless it is being typed into**, and that exception is the
            // whole reason this reads the way it does. "New time sig." puts
            // the field on the bar the playhead is in, which on a fresh song
            // is bar zero — and this `continue` fired before the field was
            // ever drawn, so the caret went somewhere invisible and the
            // owner's report was "doesn't let me change from 4/4". It did
            // change it; there was simply nothing on screen saying so.
            if pt.at_bar == 0 && view.meter.points().len() == 1 && typing.is_none() {
                continue;
            }
            let x = x_of_in(&l, view, bar_at(view, pt.at_bar));
            if x < l.meter_strip.left() - 2.0 || x > l.meter_strip.right() + 2.0 {
                continue;
            }
            let text = match typing {
                Some((_, t)) => format!("{t}|"),
                None => format!("{}/{}", pt.sig.beats, pt.sig.unit),
            };
            // A minimum width, so a chip emptied down to its caret is still
            // a rectangle somebody can see and aim at.
            let w = (font.size * 0.62 * text.chars().count() as f32 + 6.0).max(font.size * 2.2);
            let chip = Rect::from_min_size(Pos2::new(x, sigs.top()), Vec2::new(w, box_h));
            // **A chip has a pole too.** It marks a bar the same way a flag
            // marks a moment, and at the strip's new height a box floating
            // over the ruler with nothing under it does not say WHICH bar.
            painter.line_segment(
                [
                    Pos2::new(x, chip.bottom()),
                    Pos2::new(x, l.ruler.bottom()),
                ],
                Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.55)),
            );
            let face = if typing.is_some() { p.select } else { p.face };
            // Square, and the ink chosen from what the fill PRESENTS rather
            // than from the fill itself — a translucent chip whose label was
            // picked against the opaque colour is a label measured against a
            // surface that is not on screen.
            painter.rect_filled(chip, 0.0, translucent(face));
            painter.text(
                chip.center(),
                Align2::CENTER_CENTER,
                &text,
                font.clone(),
                on_ink(through(face, p.ruler)),
            );
        }
        // **The flags, over the chips.**
        //
        // A pennant and a label, in the theme's own accent — see
        // [`flag_body`] for why that role and not the transport red. The
        // owner asked for "a good, per themed flag system", and a flag that
        // carried its own colour would be the one thing on this ruler that
        // does not follow the theme.
        //
        // Drawn AFTER the signature chips so a marker sitting on a change is
        // legible: the two share a place by design, and the label says both
        // — "Chorus (3/4)" — rather than stacking a chip under a flag and
        // letting the eye work it out.
        let nfont = FontId::new(strip_font_h(names), crate::fonts::courier());
        let nbox = strip_box_h(names);
        for (i, m) in view.markers.iter().enumerate() {
            let x = x_of_in(&l, view, m.at_s);
            if x < l.meter_strip.left() - 2.0 || x > l.meter_strip.right() + 2.0 {
                continue;
            }
            let typing = view.marker_edit.filter(|(n, _)| *n == i);
            // **The name, and only the name.** A signature that starts on
            // this marker's bar is drawn as its own chip on the row below —
            // see [`flag_row`] for why the two are separated rather than
            // printed as "Chorus (3/4)".
            let shown = match typing {
                Some((_, t)) => format!("{t}|"),
                None if m.name.is_empty() => "\u{2691}".to_owned(),
                None => m.name.clone(),
            };
            let w = (nfont.size * 0.62 * shown.chars().count() as f32 + 8.0)
                .max(nfont.size * 2.2);
            let flag = Rect::from_min_size(Pos2::new(x, names.top()), Vec2::new(w, nbox));
            let body = if typing.is_some() {
                p.select
            } else {
                flag_body(&p, view.accent)
            };
            // **The pole, from the flag's foot down through the strip.**
            //
            // It used to start at the strip's top, which put the whole of it
            // behind the flag's own box for the strip's height — a pole you
            // could only see below the ruler's chip band. The owner asked
            // for flags "atop it with lines going through the strip", and
            // that is the same picture from the other side: the box sits on
            // the strip's top edge, and the line leaves its foot and crosses
            // the rest of the strip on its way down. So a flag reads as
            // pointing AT a moment rather than as a label that happens to
            // start there.
            painter.line_segment(
                [
                    Pos2::new(x, flag.bottom()),
                    Pos2::new(x, l.ruler.bottom()),
                ],
                Stroke::new(1.0_f32, body),
            );
            // **The held one is SOLID; the rest are translucent.**
            //
            // It was an outline, and the owner asked for no borders. Opacity
            // says the same thing without one and says it better: a flag you
            // have picked up is the one in front. The colour does not change,
            // so a held flag never looks like a different KIND of marker
            // while it is moving — which was the point of the outline.
            let face = if m.selected { body } else { translucent(body) };
            painter.rect_filled(flag, 0.0, face);
            let shows = if m.selected { body } else { through(body, p.ruler) };
            painter.text(
                Pos2::new(flag.left() + 4.0, flag.center().y),
                Align2::LEFT_CENTER,
                &shown,
                nfont.clone(),
                on_ink(shows),
            );
        }
    }

    // ── the clips ─────────────────────────────────────────────────────────
    //
    // A lane's clip colour and its ink are a function of the lane, worked
    // out the first time a clip on it is drawn and read back for the rest:
    // the dead-zone guard is a contrast walk of up to fifty steps, and a
    // lane full of clips paid it once per clip.
    // Keyed by row, and only for the lanes that have a clip on screen, so
    // a desk of any length pays for what it draws.
    let mut lane_ink: std::collections::HashMap<usize, (Color32, Color32)> =
        std::collections::HashMap::new();
    for clip in view.clips {
        let r = clip_rect_in(&l, view, clip);
        if !r.is_positive() {
            continue;
        }
        let (color, clip_ink) = match lane_ink.get(&clip.lane).copied() {
            Some(pair) => pair,
            None => {
                // Through the dead-zone guard, like the strip and the header
                // plate: a hand-painted lane can point its clips at the
                // grid's three unreadable third tones, and the auto
                // palettes' contract does not cover the hand grid.
                let color = crate::recorder_panel::legible_surface(
                    view.lanes
                        .get(clip.lane)
                        .map(|lane| lane.color)
                        .filter(|c| *c != 0)
                        .map(|c| {
                            let (cr, cg, cb) = STRIP_COLORS[c.min(STRIP_COLORS.len() - 1)];
                            Color32::from_rgb(cr, cg, cb)
                        })
                        // An unpainted lane's clips are the desk's green —
                        // the timeline wave's own colour, so the two
                        // surfaces agree what audio looks like.
                        .unwrap_or(Color32::from_rgb(0x2E, 0x6B, 0x46)),
                );
                // Measured on THIS clip rather than taken from `p`: the
                // guard can move an olive to the side of the dead zone where
                // the dark ink is the one that reads, and `p.clip_ink` is
                // the light one. Everything printed inside the clip — name,
                // staff, wave, fades, handles — takes this ink, so the clip
                // cannot half-agree with itself.
                let pair = (color, on_ink(color));
                lane_ink.insert(clip.lane, pair);
                pair
            }
        };
        // The clip's UNCLAMPED pixel span, measured once: where the whole
        // clip would stand if the viewport were not cutting it. The note
        // miniature and the loop seams are both mapped through it, and `x_of`
        // builds a `Layout` every time it is asked, so it is worth the
        // binding.
        //
        // **Above the fill now**, because the fill needs the seams: the clip
        // is not a rectangle any more when it repeats.
        let full_x = (
            x_of_in(&l, view, clip.start_s),
            x_of_in(&l, view, clip.start_s + clip.len_s),
        );
        // ── the clip's own shape ──────────────────────────────────────────
        //
        // **A divot bitten out of the top and bottom edge at every seam**,
        // the owner's "a simple shape change on the clip itself ... a small
        // cutout in the CLIP shape -- no line". What stood here was two
        // diagonal strokes printed on the surface, which is a picture OF a
        // seam; a bite is the seam, and it needs no colour of its own and no
        // guess about what the lane behind it is wearing.
        //
        // A clip with nothing to divide keeps the plain rounded rectangle it
        // has always been — which is every clip in an old project, and every
        // new one until its edge is pulled past its first pass.
        let seams = loop_seam_xs(r, full_x, clip);
        if seams.is_empty() {
            painter.rect_filled(r, 3.0, color);
        } else {
            for piece in notched_clip_pieces(r, &seams, 3.0) {
                painter.add(egui::Shape::convex_polygon(piece, color, Stroke::NONE));
            }
        }
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
                        Stroke::new(1.0_f32, clip_ink.gamma_multiply(0.5)),
                    );
                    x += 8.0;
                }
            } else {
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
        //
        // **From the pyramid when there is one, and then as ONE shape.** A
        // clip with `peaks` is drawn by `wave_band`: the column loop below
        // is the older road, kept for a clip the host has pictured but not
        // yet pyramided, and drawn exactly as it was.
        let pairs = clip.wave.len() / 2;
        if r.width() >= 4.0 && clip.peaks.is_some() {
            let t0 = time_at_in(&l, view, r.left());
            let t1 = time_at_in(&l, view, r.left() + 1.0);
            let dt = (t1 - t0).abs().max(1.0e-9);
            if let Some(peaks) = clip.peaks {
                wave_band(painter, r, clip, peaks, t0, dt, clip_ink.gamma_multiply(0.62));
            }
        } else if pairs > 0 && clip.wave_span_s > 0.0 && r.width() >= 4.0 {
            let mid = r.center().y;
            let half = r.height() * 0.40;
            let cols = r.width().ceil() as usize;
            // Seconds per pixel, from the panel's own mapping so this cannot
            // drift from the grid.
            let t0 = time_at_in(&l, view, r.left());
            let t1 = time_at_in(&l, view, r.left() + 1.0);
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
                    Stroke::new(1.0_f32, ink),
                );
            }
        }
        // ── the quantize window's marks ───────────────────────────────────
        //
        // **What the detector found, and where it would go.** Drawn over
        // the wave and under the fades, only while that window is open:
        // a detection threshold is not a number anybody can picture, and
        // this is the only way to turn the dial and see what it did.
        //
        // The pair is what makes it readable. The SOLID tick is the attack
        // as played; the hollow one is the grid line it would be pulled to,
        // and the gap between them is the correction. Marks that would not
        // move draw once, because a doubled tick on a hit already in time
        // says "something will happen here" when nothing will.
        if !clip.marks.is_empty() && r.width() >= 4.0 {
            draw_clip_marks(painter, r, clip, &l, view, clip_ink);
        }
        // The fades: translucent wedges from the top corners, their feet at
        // the fade's end — the picture IS the envelope.
        let veil = Color32::from_black_alpha(110);
        if clip.fade_in_s > 0.0 {
            let fx = x_of_in(&l, view, clip.start_s + clip.fade_in_s).clamp(r.left(), r.right());
            painter.add(egui::Shape::convex_polygon(
                vec![r.left_top(), Pos2::new(fx, r.top()), Pos2::new(r.left(), r.bottom())],
                veil,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(r.left(), r.bottom()), Pos2::new(fx, r.top())],
                Stroke::new(1.0_f32, clip_ink),
            );
        }
        if clip.fade_out_s > 0.0 {
            let fx = x_of_in(&l, view, clip.start_s + clip.len_s - clip.fade_out_s)
                .clamp(r.left(), r.right());
            painter.add(egui::Shape::convex_polygon(
                vec![Pos2::new(fx, r.top()), r.right_top(), Pos2::new(r.right(), r.bottom())],
                veil,
                Stroke::NONE,
            ));
            painter.line_segment(
                [Pos2::new(fx, r.top()), Pos2::new(r.right(), r.bottom())],
                Stroke::new(1.0_f32, clip_ink),
            );
        }
        // A clip being recorded wears the record's own red, and no handles:
        // its edges are the take's business until the take ends.
        if clip.recording {
            painter.rect_stroke(
                r,
                3.0,
                Stroke::new(1.5_f32, Color32::from_rgb(0xE8, 0x3A, 0x4E)),
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
            let font = FontId::new(((r.height() * 0.22).clamp(8.0, 11.0)) * crate::fonts::size_scale(painter.ctx()), crate::fonts::courier_bold());
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
                Stroke::new(2.0_f32, p.select)
            } else {
                Stroke::new(1.0_f32, Color32::from_black_alpha(140))
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
    for (i, row) in l.lane_rows.iter() {
        let Some(lane) = view.lanes.get(i).filter(|l| l.folder_silent) else {
            continue;
        };
        let _ = lane;
        if row.is_positive() {
            painter.rect_filled(*row, 0.0, Color32::from_black_alpha(96));
        }
    }
    for (i, row) in l.header_rows.iter() {
        let Some(lane) = view.lanes.get(i).filter(|l| l.folder_silent) else {
            continue;
        };
        let _ = lane;
        if row.panel.is_positive() {
            painter.rect_filled(row.panel, 0.0, Color32::from_black_alpha(96));
        }
    }
    // **A frozen lane is under frost**, clips and header alike, and wears
    // a snowflake at the header's right edge. Over the clips, so what is
    // drawn reads as "here, but not for touching" - the clips are what the
    // frozen file was rendered from and thawing brings them back.
    for row in &l.rows {
        let RowKind::Track(lane) = row.kind else {
            continue;
        };
        if !view.lanes.get(lane).is_some_and(|v| v.frozen) || !row.lane.is_positive() {
            continue;
        }
        painter.rect_filled(row.lane, 0.0, FROST_TINT);
    }
    for (i, row) in l.header_rows.iter() {
        if !view.lanes.get(i).is_some_and(|v| v.frozen) || !row.panel.is_positive() {
            continue;
        }
        painter.rect_filled(row.panel, 0.0, FROST_TINT);
        let r = (row.panel.height() * 0.22).clamp(3.0, 6.0);
        snowflake(painter, Pos2::new(row.panel.right() - r - 4.0, row.panel.top() + r + 3.0), r, FROST_INK);
    }

    // ── the TIME SELECTION: a veil over the span, on ruler and lanes ──────
    // AFTER the clips, deliberately: the veil and its boundary lines are
    // about the material inside the span, and painting them first let
    // every opaque clip body hide exactly the part of the selection that
    // mattered — visible only in the gaps, the standard DAW picture
    // inverted. Under the playhead still, which always reads on top.
    if let Some((t0, t1)) = view.time_sel.filter(|(a, b)| b > a) {
        let x0 = x_of_in(&l, view, t0).max(l.lanes_area.left());
        let x1 = x_of_in(&l, view, t1).min(l.lanes_area.right());
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
                    Stroke::new(1.0_f32, sel.gamma_multiply(0.8)),
                );
            }
        }
    }

    // ── the playhead, through the lanes and the ruler ─────────────────────
    let px = x_of_in(&l, view, view.playhead_s);
    if px >= l.lanes_area.left() - 0.5 && px <= l.lanes_area.right() + 0.5 {
        painter.line_segment(
            [Pos2::new(px, l.ruler.top()), Pos2::new(px, l.lanes_area.bottom())],
            Stroke::new(1.5_f32, p.playhead),
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
            Stroke::new(1.0_f32, p.engrave.gamma_multiply(if view.grip_lit { 1.0 } else { 0.55 })),
        );
    }
    painter.line_segment(
        [
            Pos2::new(l.header.right(), rect.top()),
            Pos2::new(l.header.right(), rect.bottom()),
        ],
        Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.5)),
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
                    STRIP_COLORS[view.master_color.min(
                        STRIP_COLORS.len() - 1,
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
                Stroke::new(1.0_f32, ink.gamma_multiply(0.35)),
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
                let size = crate::fonts::px((r.automation.height() * 0.68).clamp(7.0, 11.0) * crate::fonts::size_scale(painter.ctx()));
                if size >= 7.0 {
                    let plate = if on { p.select } else { face };
                    painter.text(
                        r.automation.center(),
                        Align2::CENTER_CENTER,
                        // The wrench the track headers below wear, for the
                        // same button. See the switch loop down there.
                        crate::fonts::GLYPH_WRENCH,
                        FontId::new(size, crate::fonts::icon()),
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
            // The master has no dial column to be held down by, so its chips
            // work their size out from their own height — `header_type_size`
            // is a TRACK row's rule.
            let bay_size = crate::fonts::px(
                (r.bays[0].height() * 0.62).clamp(7.0, 10.0)
                    * crate::fonts::size_scale(painter.ctx()),
            );
            for (b, cell) in r.bays.iter().enumerate() {
                let word = view.master_inserts.get(b).copied().unwrap_or("");
                bay_chip(
                    painter,
                    *cell,
                    if word.is_empty() { "+" } else { word },
                    !word.is_empty(),
                    view.master_bypassed.get(b).copied().unwrap_or(false),
                    &p,
                    bay_size,
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
                    // The outline and the needle are FIXED, the way the VU
                    // face's colours are and for the same reason: they are
                    // the instrument, not the panel, and a near-black
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
                fit_text(
                    painter,
                    r.knob_readout,
                    &db,
                    &crate::fonts::courier(),
                    quiet(ink, face, 0.22),
                    0.92,
                    (7.0, 12.0),
                );
            }
            // The word under the meter, saying what the dial is — the track
            // rows write theirs under the knob and this row has no room
            // below, so it goes beside.
            if r.vol_label.is_positive() {
                fit_text(
                    painter,
                    r.vol_label,
                    "VOL",
                    &crate::fonts::courier(),
                    quiet(ink, face, 0.28),
                    0.62,
                    (6.0, 11.0),
                );
            }
        }
    }
    for (i, row) in l.header_rows.iter() {
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
                    Stroke::new(1.2_f32, crate::recorder_panel::selection_rim(face)),
                    egui::StrokeKind::Inside,
                );
            }
        } else {
            // The empty slot's own ground fills the WHOLE panel — the wood
            // margin around a dark plate read as two surfaces, the owner's
            // "my bad" screenshot. The trailing add's runs on to the foot
            // of the column (see `EndlessGround`).
            painter.rect_filled(
                endless_at(i).map_or(row.panel, |g| g.header),
                0.0,
                p.empty_ground,
            );
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
                    Stroke::new(1.2_f32, crate::recorder_panel::selection_rim(p.empty_ground)),
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
        // The trailing add is no row yet, so it wears no number.
        let numbered = lane.used || lane.plus == crate::mixer_panel::Plus::Row;
        painter.text(
            Pos2::new(plate.left() + 4.0, plate.center().y),
            Align2::LEFT_CENTER,
            if numbered { format!("{}", i + 1) } else { String::new() },
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
                p.empty_ink
            },
        );
        // Compact: the unit has no room at this size, and a number under a
        // VOLUME dial needs none.
        let db = if lane.gain <= 0.001 {
            "-inf".to_owned()
        } else {
            format!("{:+.1}", 20.0 * lane.gain.log10())
        };
        // **One size for every word on this row** — the name, the input
        // button, the six bays, ROUTING, the three figures and the three
        // words under the dials. See `header_type_size`.
        let type_size = header_type_size(painter, row, &db);
        // The input chip draws on EVERY lane — binding an input is how a
        // bare lane becomes a track — in bone whatever the theme, so the
        // words read on a painted header and on the grey field alike.
        // Not bypassable, these two: the input chip and the ROUTING button
        // borrow the bay's LOOK and are not bays, so they pass `false` and
        // the bays below call `bay_chip` themselves with their own flag.
        let chip = |painter: &Painter, r: Rect, word: &str, has: bool| {
            bay_chip(painter, r, word, has, false, &p, type_size);
        };
        // **The INPUT button shrinks its word rather than cutting it.**
        //
        // It used to have a band of the column to itself and now it shares
        // the name's, which is a third of the room — and a bay chip prints at
        // the size its BOX is tall and truncates whatever does not fit, so
        // "NO INPUT" came out as "NO...". A word nobody can read is not a
        // button, it is a smudge with a border. This one is sized to the
        // WIDTH and falls back to truncating only under the floor where
        // shrinking further would be a smudge of another kind.
        let input_button = |painter: &Painter, r: Rect, word: &str, has: bool| {
            input_chip(painter, r, word, has, &p, type_size);
        };
        if !lane.used {
            if row.add.is_positive() {
                let c = row.add.center();
                // The + on an empty slot: measured against the ground it is
                // actually drawn on, and blended rather than made
                // transparent. It was the themed face at 55% alpha, which on
                // a dark theme is a circle nobody can see.
                let ink = p.empty_ink;
                let radius = row.add.width().min(row.add.height()) * 0.5 - 1.0;
                let w = (radius * 0.12).clamp(1.2, 2.2);
                painter.circle_stroke(c, radius, Stroke::new(w, ink));
                if lane.plus == crate::mixer_panel::Plus::Locked {
                    crate::mixer_panel::padlock(painter, c, radius, w, ink);
                } else {
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
            }
            if lane.plus == crate::mixer_panel::Plus::Row {
                chip(painter, row.input, "NO INPUT", false);
            }
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
            painter.circle_stroke(c, rad, Stroke::new(1.5_f32, ink_on.gamma_multiply(0.8)));
        }
        // The name, on its plate — or the rename field mid-typing.
        if row.name.is_positive() {
            // The row's own size — see `header_type_size`. Bold, because a
            // name is the thing being read; the same size as everything else
            // on the row, because the owner asked for one size and a name a
            // point and a half taller than its own bays was the largest part
            // of what made the row look like three panels.
            let font = FontId::new(type_size, crate::fonts::courier_bold());
            let naming = view.naming.filter(|(at, _)| *at == i).map(|(_, b)| b);
            if let Some(buf) = naming {
                painter.rect_filled(row.name, 2.0, p.hdr_face);
                painter.rect_stroke(
                    row.name,
                    2.0,
                    Stroke::new(1.0_f32, p.select),
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
        // **The INPUT BUTTON, at the right end of the name's own row** — the
        // owner's placement. It had a band of the column to itself; every
        // lane wears six bays now and the second rack took that band.
        let word = if lane.input_short.is_empty() {
            "IN"
        } else {
            lane.input_short.as_str()
        };
        input_button(painter, row.input, word, !lane.input.is_empty());
        // The three bays beside it — the mixer's insert slots, worded the
        // same: the plugin's short name, or a plus for an empty seat.
        for (b, r) in row.bays.iter().enumerate() {
            if !r.is_positive() {
                continue;
            }
            let bay_word = lane.inserts.get(b).copied().unwrap_or("");
            // Not through `chip`: a BAY is the one of these three that can
            // be taken out of circuit, so it carries its own flag.
            bay_chip(
                painter,
                *r,
                if bay_word.is_empty() { "+" } else { bay_word },
                !bay_word.is_empty(),
                lane.bypassed.get(b).copied().unwrap_or(false),
                &p,
                type_size,
            );
        }
        // **The frost goes OVER the bays and the input, not under them.**
        //
        // The header panel is washed with `FROST_TINT` further up this
        // function, and then this row paints its chips on top of it at full
        // strength - so the two things on a frozen lane that do not apply
        // went on reading as live. The owner: "i can still interact with
        // the fx inserts and they still dont look frozen to me".
        //
        // The bays are in the rendered file, and a frozen lane records
        // nothing, so neither the plugins nor the input do anything until
        // it thaws. `apply_mixer_hit_one` refuses the press; this is the
        // half that says so before the press is made.
        if lane.frozen {
            for r in row.bays.iter().chain(std::iter::once(&row.input)) {
                if r.is_positive() {
                    painter.rect_filled(*r, 2.0, FROST_TINT);
                }
            }
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
            dial(painter, cell, t, cap, p.lane, p.clip_ink);
        };
        let readout = |painter: &Painter, cell: Rect, text: String| {
            if !cell.is_positive() || type_size < 5.5 {
                return;
            }
            painter.text(
                cell.center(),
                Align2::CENTER_CENTER,
                text,
                FontId::new(type_size, crate::fonts::courier()),
                ink_on,
            );
        };
        dial(painter, row.knob, crate::recorder::gain_to_fader(lane.gain), DIAL_VOLUME);
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
        let label_ink = quiet(ink_on, face, 0.34);
        // **The band a label stands in ENDS where the ROUTING button starts.**
        //
        // It used to run to the bottom of the panel and the word was hung
        // from its top edge, which is two faults wearing each other's
        // clothes: on a row with no routing button the band is half as tall
        // again as the word, so the label sat jammed under its dial with a
        // finger of empty panel below it — the owner's "proper vertical
        // centering" — and on a row WITH one the band overlapped a button
        // whose own rectangle starts nine points down. Naming the real floor
        // is what lets the word be centred in it and lets it grow.
        let label_floor = if row.routing.is_positive() {
            row.routing.top()
        } else {
            row.panel.bottom() - 1.0
        };
        for (dial, word) in [(row.pan, "PAN"), (row.knob, "VOL"), (row.send, "SND")] {
            if !dial.is_positive() {
                continue;
            }
            let band = Rect::from_min_max(
                Pos2::new(dial.left() - 2.0, dial.bottom()),
                Pos2::new(dial.right() + 2.0, label_floor),
            );
            // The same size the figures above wear, and shed rather than
            // shrunk when the band cannot carry it: three words at three
            // sizes was the fault, and a fourth size here would be it again.
            if band.height() >= type_size && type_size >= 5.5 {
                painter.text(
                    band.center(),
                    Align2::CENTER_CENTER,
                    word,
                    FontId::new(type_size, crate::fonts::courier()),
                    label_ink,
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
        // **The NOTE**, beside the arm: this track's MIDI reaches the theory
        // view. It was a **T** here and an eighth note on the mixer's own
        // switch, which is one control with two faces — the owner's "use the
        // note glyph globally for the theory routing button". The desk had it
        // right; the timeline follows.
        //
        // Drawn with the M and the S rather than on its own, so a hand that
        // has learned those two recognises it — in the theme's own selection
        // colour, because it is neither a mute nor a solo and three switches
        // wearing two colours is one of them being mistaken for another.
        //
        // **And the WRENCH** where the automation button said A. A letter
        // standing for a word is a legend to learn; a wrench is what every
        // other tool this owner uses puts on the thing that adjusts a thing.
        //
        // Both come out of `fonts::icon`, which is Terminess whatever face is
        // chosen — neither Courier Prime nor JetBrains Mono has either glyph,
        // read out of their cmaps, so drawn in the text family they would be
        // a tofu box on two faces out of three.
        // **The theory send's note is DRAWN**, not set — see
        // `recorder_panel::draw_note`. A notehead is an oblong and it is
        // tilted, and no glyph in any face this app bundles is both.
        if row.theory.is_positive() {
            let on = lane.to_theory;
            let plate = if on { p.select } else { p.hdr_lane };
            painter.rect_filled(row.theory, 2.0, plate);
            crate::recorder_panel::draw_note(
                painter,
                row.theory.shrink(row.theory.width() * 0.12),
                if on { on_ink(plate) } else { p.switch_off },
            );
        }
        for (r, label, on, lit, family, icon) in [

            // Lit when this track is showing any automation row, so the
            // header says at a glance that the rows below belong to it.
            (
                row.automation,
                crate::fonts::GLYPH_WRENCH,
                !lane.automation.is_empty(),
                p.select,
                crate::fonts::icon(),
                true,
            ),
            // Ø on the theme's own selection ink, like the two above it: it
            // is neither a mute nor a solo, and a third warning colour on a
            // header carrying two would read as a third kind of danger.
            (
                row.phase,
                "\u{d8}",
                lane.phase,
                p.select,
                crate::fonts::courier_bold(),
                false,
            ),
            (row.mute, "M", lane.muted, p.mute, crate::fonts::courier_bold(), false),
            (row.solo, "S", lane.soloed, p.solo, crate::fonts::courier_bold(), false),
        ] {
            if !r.is_positive() {
                continue;
            }
            painter.rect_filled(r, 2.0, if on { lit } else { p.hdr_lane });
            // **A glyph fills more of its box than a letter does.** A capital
            // M is drawn to sit on a baseline with room over it for an
            // ascender; a note and a wrench are drawn to fill the em, and at
            // a letter's fraction they came out small and lost in a switch
            // the same size. The owner asked for them bigger "to fit the
            // button better", and this is the whole of it.
            let k = if icon { 0.86 } else { 0.6 };
            painter.text(
                r.center(),
                Align2::CENTER_CENTER,
                label,
                FontId::new(((r.height() * k).clamp(8.0, 17.0)) * crate::fonts::size_scale(painter.ctx()), family),
                if on { on_ink(lit) } else { p.switch_off },
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
            let ink = p.collapse_ink;
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
            let x0 = x_of_in(&l, view, t0).max(l.lanes_area.left());
            let x1 = x_of_in(&l, view, t1).min(l.lanes_area.right());
            if x1 > x0 {
                let ghost = Rect::from_min_max(
                    Pos2::new(x0, row.top() + 1.5),
                    Pos2::new(x1, row.bottom() - 1.5),
                );
                painter.rect_filled(ghost, 3.0, Color32::from_white_alpha(24));
                painter.rect_stroke(
                    ghost,
                    3.0,
                    Stroke::new(1.5_f32, p.select),
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
    // **The click's own drawing, and the STATE is the tint.** There is one
    // metronome in the set and this button has two states, which is not the
    // problem it looks like: `tool` above has already painted the ground and
    // worked out the ink that reads on it - the desk's green when the click
    // is on, the ground's own ink pulled back when it is off - so on and off
    // differ by colour and by the plate behind them, exactly as they did.
    // The glyph never changed between the two; the old one was only called
    // `draw_metronome_on` because the click being on is what it stood for.
    if let Some(ink) = tool(painter, l.tool_met, view.metronome_on) {
        crate::icons::draw(
            painter,
            l.tool_met.shrink(2.0),
            ink,
            crate::icons::Icon::Metronome,
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
        // **Numerals, so `grid_label_text` and not `grid_label`.** This box
        // is the one somebody meets on their first afternoon, and one rung
        // of its ladder used to be a picture instead of a fraction — the
        // owner wants "1/4" here because it is what a beginner can spot at a
        // glance. The whole argument is on `grid_label_text`; what matters
        // at this call site is that the two names are not interchangeable.
        // The MIDI editor's chip still calls `grid_label` and still shows
        // the glyph, so pointing this line back at `grid_label` would not
        // undo one decision, it would move two surfaces at once.
        //
        // **Shrunk to the box and clipped to it** (`grid_box_word`): the
        // box is drawn for three characters and the ladder prints up to
        // six. A step wider than the box spilled out of it, and the widest
        // onto the tempo lane's switch and under the navigator.
        grid_box_word(
            painter,
            l.tool_grid,
            &grid_label_text(view.grid_exp, sig_now.beats, sig_now.unit),
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
            Stroke::new(1.0_f32, p.grid.gamma_multiply(2.0)),
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

    // ── the lane scrollbar, over the lanes' right edge ────────────────────
    if l.lane_bar.track.is_positive() {
        painter.rect_filled(l.lane_bar.track, LANE_BAR_W * 0.5, p.face.gamma_multiply(0.18));
        painter.rect_filled(l.lane_bar.thumb, LANE_BAR_W * 0.5, p.face.gamma_multiply(0.7));
    }

    // ── the EMPTY SLOTS' borders, LAST — nothing may paint over them ──────
    // Third round on these, and the lesson was paint ORDER, not ink: the
    // header section begins by filling its whole column with wood, and the
    // empty slot's own panel is filled again inside the header loop — every
    // earlier border, etched or boxed, was buried the same frame it was
    // drawn. Painted here, on top of everything, each unoccupied slot is
    // one channel-shaped box — header through field — in faded face ink
    // that reads on both palettes.
    for (i, row) in l.lane_rows.iter() {
        if view.lanes.get(i).is_some_and(|lane| lane.used) {
            continue;
        }
        let left = l.header_rows.get(i).map_or(row.left(), |h| h.panel.left());
        // The trailing add's box closes round its whole run, so the add and
        // the ground under it are one box rather than a box with a floor.
        let foot = endless_at(i).map_or(row.bottom(), |g| g.lane.bottom());
        let slot = Rect::from_min_max(Pos2::new(left, row.top()), Pos2::new(row.right(), foot));
        if slot.is_positive() {
            painter.rect_stroke(
                slot,
                0.0,
                // Half a point: the owner wants them THIN — present, not
                // loud — and the full-width run makes even a hairline read.
                Stroke::new(0.5_f32, p.face.gamma_multiply(0.35)),
                egui::StrokeKind::Inside,
            );
        }
    }
}

/// **A label cut to `width` with an ellipsis, in a few layouts rather than
/// one per character.**
///
/// It measured the whole text and then, on a miss, shrank it one character
/// at a time, laying out every candidate — ten to thirty galleys per chip
/// per frame, for every bay chip, input chip, routing button, lane name and
/// clip name that did not fit, which on a busy header is most of them.
/// Cached galleys are hash-and-probe hits, but a truncation point that
/// moves (every clip name during a zoom) is that many real shapings.
///
/// The galley the first measure makes already carries every glyph's
/// advance, so where the cut falls is ESTIMATED from it: the ellipsis after
/// `k` glyphs lands about where their advances add up to. About, not
/// exactly — the layout rounds each glyph's x to the pixel together with
/// that glyph's own bearing, so `の` at 12.6 stands at 13 and `…` at 12.6
/// stands at 12, and no prefix arithmetic reproduces it. So the estimate is
/// given a pixel and a quarter of slack, which makes it an UPPER bound on
/// what the walk would accept (a rounding of at most a pixel plus the
/// sixteenth the galley's rect is quantised to), the candidate is measured
/// once, and if it does not fit the old walk carries on DOWNWARD from
/// there — one step, usually. The answer is therefore the walk's own answer
/// by construction, in three or four layouts instead of twenty, two of
/// them (the whole text, the ellipsis) stable strings the cache answers
/// without shaping. `cut_to_gives_the_walk_its_own_answer` holds the two
/// equal over a sweep of names, widths and both faces, CJK and runs of
/// spaces included. Text with a newline has more than one row and goes to
/// the walk as it always did.
/// **A figure written as big as its cell will carry it**, centred in that cell.
///
/// The dial readouts on a track header — `-12.3`, `100%`, `L50` — and the
/// words under the dials that say which dial is which. Both used to be sized
/// off the cell's HEIGHT and then cut down by a width budget of
/// `chars * 0.64 * size`, which is Courier Prime's advance. Terminess, the
/// face this app ships in, is a fifth narrower than that, so every readout on
/// the timeline was drawn a fifth smaller than the room it actually had: at
/// the default lane height the volume figure came out at six points against a
/// cell that would carry eight. The owner asked for the knob text and the
/// readouts to be bigger, and most of "bigger" was already there.
///
/// So the budget is MEASURED, in the face that is installed, and the cell's
/// height only names the ceiling. `size_scale` rides along for the same
/// reason it rides on every other size in this file: the three faces have
/// different cap heights, and the point size that reads on one is small on
/// another.
///
/// Rounded through `fonts::px` before it is used, so a figure that changes
/// every frame while a dial is being turned does not mint a new galley for
/// every fractional point size on the way.
/// **One size for every figure and word in a track's dial column.**
///
/// The three readouts and the three labels used to be sized one at a time,
/// each against its own cell and its own string — so `C` under the pan dial
/// came out at its cell's full height while `-12.3` under the volume was cut
/// to two thirds of it by its own width, and the words below were bigger than
/// either. Three sizes in an inch of panel, none of them agreeing. The owner:
/// "make sure that text is made larger, while the pan/send readouts must be
/// smaller to match even though they have less characters".
///
/// So the row picks ONE size and everything wears it. It is what the WIDEST
/// thing the row can ever show will fit in — five characters, the volume's
/// `-12.3` — measured in the face that is installed, which is the lesson
/// `fit_text` records.
///
/// **The VOLUME's own figure is what decides it**, because it is the longest
/// — a sign, two digits, a point and a tenth against the pan's `C` and the
/// send's `0%`. That is the owner's rule stated in his own terms: the volume
/// readout is the one that has to fit, and everything else comes down to
/// meet it rather than the other way round.
fn dial_type_size(painter: &Painter, cell: Rect, longest: &str) -> f32 {
    if !cell.is_positive() || longest.is_empty() {
        return 0.0;
    }
    let scale = crate::fonts::size_scale(painter.ctx());
    let ceiling = crate::fonts::px((cell.height() * 0.92).clamp(7.0, 13.0) * scale);
    let wide = painter
        .layout_no_wrap(
            longest.to_owned(),
            FontId::new(ceiling, crate::fonts::courier()),
            Color32::WHITE,
        )
        .rect
        .width();
    let room = cell.width() - 2.0;
    if wide > room && wide > 0.0 {
        crate::fonts::px(ceiling * room / wide)
    } else {
        ceiling
    }
}

/// **One size for every word on a track's header row.**
///
/// The name, the input button, the six bays, ROUTING, the three figures over
/// the dials and the three words under them. The owner's rule, in his own
/// terms: the knob text and ROUTING "must not be bigger than the text for the
/// track label and fx insert text — make all text on these the same size".
///
/// So it is the SMALLEST of what each cell can carry, and the tightest is
/// almost always the dial's figure: `-12.0` is five characters in a cell as
/// wide as the knob under it, while a bay has room for four in a box three
/// times that. Taking the minimum is what makes the "must not be bigger"
/// half true by construction rather than by a number somebody tuned.
///
/// A cell that has SHED contributes nothing — a narrow header with no dials
/// must not size the whole row off a rectangle that is not there.
fn header_type_size(painter: &Painter, row: &HeaderRow, db: &str) -> f32 {
    let scale = crate::fonts::size_scale(painter.ctx());
    let mut size = f32::INFINITY;
    let mut take = |v: f32| {
        if v > 0.0 && v < size {
            size = v;
        }
    };
    take(dial_type_size(painter, row.knob_readout, db));
    // The chip rule, `bay_chip`'s own: a box's height at 0.62, floored and
    // capped where a chip stops being readable and stops being a chip.
    for cell in [row.bays[0], row.input, row.routing] {
        if cell.is_positive() {
            take(crate::fonts::px((cell.height() * 0.62).clamp(7.0, 10.0) * scale));
        }
    }
    if row.name.is_positive() {
        take(crate::fonts::px((row.name.height() * 0.56).clamp(8.5, 12.0) * scale));
    }
    if size.is_finite() {
        size
    } else {
        crate::fonts::px(9.0 * scale)
    }
}

fn fit_text(
    painter: &Painter,
    cell: Rect,
    text: &str,
    family: &egui::FontFamily,
    ink: Color32,
    k: f32,
    span: (f32, f32),
) {
    if !cell.is_positive() || text.is_empty() {
        return;
    }
    let scale = crate::fonts::size_scale(painter.ctx());
    let want = crate::fonts::px((cell.height() * k).clamp(span.0, span.1) * scale);
    let measure = |size: f32| {
        painter
            .layout_no_wrap(text.to_owned(), FontId::new(size, family.clone()), ink)
            .rect
            .width()
    };
    let wide = measure(want);
    // A hair of air either side, so a figure never touches the neighbour's.
    let room = cell.width() - 2.0;
    let size = if wide > room && wide > 0.0 {
        crate::fonts::px(want * room / wide)
    } else {
        want
    };
    // Below the floor it is a smudge, and a smudge under a dial is worse than
    // nothing: the dial itself still says where it is standing.
    if size < 5.5 {
        return;
    }
    painter.text(
        cell.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::new(size, family.clone()),
        ink,
    );
}

pub(crate) fn cut_to(painter: &Painter, width: f32, text: &str, font: &FontId) -> String {
    let measure = |t: &str| painter.layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE);
    let whole = measure(text);
    if whole.rect.width() <= width {
        return text.to_owned();
    }
    let mut keep = text.len();
    if let [row] = whole.rows.as_slice() {
        // A pixel for the glyph rounding (half from the cursor, half from a
        // bearing), a sixteenth for the rect's quantum, and a little air.
        const SLACK: f32 = 1.25;
        let ell = measure("\u{2026}").rect.width();
        // One glyph per char, in order, so the byte count walks with them
        // and every stop is a char boundary. `fits` is the byte length of
        // the glyphs BEFORE the one under the cursor: keep those, and the
        // ellipsis takes that one's place.
        let mut cursor = 0.0f32;
        let mut bytes = 0usize;
        let mut fits = 0usize;
        for g in &row.glyphs {
            if cursor + ell > width + SLACK {
                break;
            }
            fits = bytes;
            cursor += g.advance_width;
            bytes += g.chr.len_utf8();
        }
        if fits < text.len() && text.is_char_boundary(fits) {
            let cut = format!("{}\u{2026}", text[..fits].trim_end());
            if measure(&cut).rect.width() <= width {
                return cut;
            }
            keep = fits;
        }
    }
    while keep > 0 {
        keep -= 1;
        while keep > 0 && !text.is_char_boundary(keep) {
            keep -= 1;
        }
        let cut = format!("{}\u{2026}", text[..keep].trim_end());
        if measure(&cut).rect.width() <= width {
            return cut;
        }
    }
    String::new()
}

/// The walk `cut_to` replaced, kept as its oracle: every candidate laid out
/// and the first that fits returned.
#[cfg(test)]
pub(crate) fn cut_to_reference(painter: &Painter, width: f32, text: &str, font: &FontId) -> String {
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

/// **Every time signature the app accepts**, `(beats, unit)`, found by
/// asking `TimeSignature::is_valid` over a range wider than it allows rather
/// than by writing its rule out again: 1 to 32 beats over 1, 2, 4, 8, 16 or
/// 32, which is 192. Test support for the sweeps that hold every grid ladder
/// to something; each of them asserts the 192.
#[cfg(test)]
pub(crate) fn valid_signatures() -> Vec<(u8, u8)> {
    let mut sigs = Vec::new();
    for beats in 0u8..=64 {
        for unit in 0u8..=64 {
            if (crate::recorder::TimeSignature { beats, unit }).is_valid() {
                sigs.push((beats, unit));
            }
        }
    }
    sigs
}

/// **A context dressed in `face`, at `ppp` pixels to the point**, the way
/// the app's is: test support for measuring words as the screen gets them.
/// The scale matters to a word's width (glyphs land on whole pixels) and to
/// where its ink falls, so the fit sweeps run at 100% to 200%.
#[cfg(test)]
pub(crate) fn context_at(face: crate::fonts::FontChoice, ppp: f32) -> egui::Context {
    let ctx = egui::Context::default();
    crate::fonts::install(&ctx, face, None);
    crate::fonts::apply_text_styles(&ctx);
    let mut raw = egui::RawInput::default();
    raw.viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .native_pixels_per_point = Some(ppp);
    // Twice: the first pass is what adopts the scale and the fonts.
    let _ = ctx.run(raw.clone(), |_| {});
    let _ = ctx.run(raw, |_| {});
    assert_eq!(ctx.pixels_per_point(), ppp, "the context did not take the scale");
    ctx
}

/// **A word as it reaches the screen**: its text and size, where it was laid
/// out (its advances, from where it was placed), the ink the tessellator
/// draws for it (every glyph's pixels, unclipped) and the clip it carries.
#[cfg(test)]
#[derive(Debug, Clone)]
pub(crate) struct PaintedWord {
    pub text: String,
    pub size: f32,
    pub laid: Rect,
    pub ink: Rect,
    pub clip: Rect,
}

#[cfg(test)]
impl PaintedWord {
    /// Every text `shapes` holds, measured. `ctx` tessellates them, at its
    /// own scale, for the ink.
    pub(crate) fn all(ctx: &egui::Context, shapes: &[egui::epaint::ClippedShape]) -> Vec<Self> {
        shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) => {
                    let mut ink = Rect::NOTHING;
                    for p in ctx.tessellate(vec![s.clone()], ctx.pixels_per_point()) {
                        if let egui::epaint::Primitive::Mesh(m) = &p.primitive {
                            ink = ink.union(m.calc_bounds());
                        }
                    }
                    Some(Self {
                        text: t.galley.text().to_owned(),
                        size: t.galley.job.sections.first().map_or(0.0, |x| x.format.font_id.size),
                        laid: Rect::from_min_size(t.pos, t.galley.size()),
                        ink,
                        clip: s.clip_rect,
                    })
                }
                _ => None,
            })
            .collect()
    }

    /// What `paint` paints on a layer of its own, keyed by `key`, measured.
    pub(crate) fn painted_by(
        ctx: &egui::Context,
        key: impl std::hash::Hash,
        paint: impl FnOnce(&Painter),
    ) -> Vec<Self> {
        let id = egui::Id::new(("painted word", key));
        let layer = egui::LayerId::new(egui::Order::Foreground, id);
        paint(&Painter::new(ctx.clone(), layer, Rect::EVERYTHING));
        let shapes: Vec<egui::epaint::ClippedShape> =
            ctx.graphics(|g| g.get(layer).map(|l| l.all_entries().cloned().collect()).unwrap_or_default());
        Self::all(ctx, &shapes)
    }
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
        let x = tempo_x_of(&l, &v, row.lane, 0.0);
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
    use crate::desk_lift::V32_CHANNELS as CHANNELS;

    fn a_lane(used: bool) -> LaneView<'static> {
LaneView {
    to_theory: false,
    is_folder: false,
    in_folder: false,
    folder_shut: false,
    folder_count: 0,
    hidden: false,
    folder_silent: false,
    frozen: false,
            automation: Vec::new(),
            plus: crate::mixer_panel::Plus::Row,
            route_count: 0,
            stereo: true,
            inserts: [""; crate::recorder::INSERTS * 2],
            bypassed: [false; crate::recorder::INSERTS * 2],
            name: "KEYS",
            input: String::new(),
            input_short: String::new(),
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
                peaks: None,
                selected: false,
                midi: false,
                loop_s: 0.0,
                notes: &[],
                offset_s: 0.0,
                recording: false,
                gain: 1.0,
                marks: &[],
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
                peaks: None,
                selected: true,
                midi: false,
                loop_s: 0.0,
                notes: &[],
                offset_s: 0.0,
                recording: false,
                gain: 1.0,
                marks: &[],
            },
        ]
    }


    /// **The signature field at bar zero is ON SCREEN while it is open.**
    ///
    /// A song in one signature draws no chip at bar 0 — bar one's signature
    /// is the song's, not a change, and a chip on the first bar of every
    /// song would be noise on the vast majority that never change. But "New
    /// time sig." puts the field on the bar the PLAYHEAD is in, which on a
    /// fresh song is bar 0, and the skip fired before the field was drawn:
    /// the caret went somewhere invisible, and the owner reported it as
    /// "doesn't let me change from 4/4". The value was changing. Nothing on
    /// screen said so.
    ///
    /// So: the exception is exactly "while somebody is typing into it", the
    /// hit-test carries the same exception (a chip you can see and cannot
    /// press is the same bug wearing a hat), and the closed state still
    /// draws nothing.
    ///
    /// Revert-checks, both performed: drop `&& typing.is_none()` from the
    /// painter's `continue` and the first block goes red; drop it from
    /// `hit_test`'s and the last block goes red.
    #[test]
    fn a_signature_being_typed_at_bar_zero_is_on_screen() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&clips);
        assert_eq!(v.meter.points().len(), 1, "the fixture is not a plain song");

        // Closed: bar 0 says nothing, which is the rule this exception is
        // carved out of.
        assert!(
            !strip_words(&v).iter().any(|w| w.contains("4/4")),
            "a song in one signature drew a chip at bar 0"
        );

        // Open: the field is there, caret and all.
        v.meter_edit = Some((0, "3/"));
        let words = strip_words(&v);
        assert!(
            words.iter().any(|w| w == "3/|"),
            "the field at bar 0 was invisible while being typed into: {words:?}"
        );

        // And it can be pressed where it was drawn.
        let l = Layout::new(rect(), &v);
        let x = x_of(rect(), &v, bar_at(&v, 0)) + 3.0;
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(x, sig_row(l.meter_strip).center().y)),
            Some(Hit::MeterChip(0)),
            "the open field at bar 0 is not where it was painted"
        );
    }

    /// **The corner's four buttons are one size, and they use the room the
    /// taller ruler gave them.**
    ///
    /// They were three squares capped at twenty points and a grid box 2.4
    /// times as wide — four controls in three sizes. The owner: "make the 4
    /// buttons (met, grid, tempo arm, and frequency) equal sized and fit
    /// their new area." Checked across the whole span of header widths the
    /// seam allows, because the narrow end is where equal-sizing and the
    /// navigator's minimum fight each other, and the wide end is where a
    /// missing cap would let them grow silly.
    ///
    /// Revert-checks, all three performed: give `tool_grid` its old 2.4x
    /// width and the equality assert goes red; cap `side` at 20 again and
    /// the "uses the corner" assert goes red at the wide end; drop the
    /// 5.5-cell division and the navigator vanishes on a 150-point header.
    #[test]
    fn the_corner_holds_four_buttons_of_one_size() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        for hw in [150.0_f32, 170.0, 200.0, 220.0] {
            let mut v = a_view(&clips);
            v.header_w = Some(hw);
            let l = Layout::new(rect(), &v);
            let four = [l.tool_met, l.tool_snap, l.tool_tempo, l.tool_grid];
            let first = four[0];
            for (n, r) in four.iter().enumerate() {
                assert!(r.is_positive(), "button {n} is not drawn at header {hw}");
                assert!(
                    (r.width() - first.width()).abs() < 0.01
                        && (r.height() - first.height()).abs() < 0.01,
                    "button {n} is {:.1}x{:.1} where the first is {:.1}x{:.1} at header {hw}",
                    r.width(),
                    r.height(),
                    first.width(),
                    first.height()
                );
                assert!(
                    (r.width() - r.height()).abs() < 0.01,
                    "button {n} is not square at header {hw}"
                );
            }
            // **The original height, about half the corner.** They were
            // briefly allowed to fill the taller corner and it made a row of
            // buttons that dominated it — the owner asked for them back:
            // "shrink the scanner/met tempo arm and grid section to be the
            // original height — a half strip". Both bounds, because "equal
            // sized" is satisfied by four buttons of any one wrong size.
            assert!(
                first.height() <= 20.01,
                "the buttons are {:.1}pt, past the original twenty, at header {hw}",
                first.height()
            );
            assert!(
                first.height() >= 14.0 && first.height() <= l.ruler.height() * 0.6,
                "the buttons are {:.1}pt in a {:.1}pt corner at header {hw}",
                first.height(),
                l.ruler.height()
            );
            // And the navigator still has somewhere to be, which is what
            // stops the four from eating the corner.
            assert!(
                l.tool_nav.is_positive(),
                "the navigator was squeezed out at header {hw}"
            );
            // In order, left to right, with air between them.
            for pair in four.windows(2) {
                assert!(
                    pair[1].left() >= pair[0].right() + 1.0,
                    "two buttons touch or overlap at header {hw}"
                );
            }
            assert!(
                l.tool_nav.left() >= l.tool_grid.right() + 1.0,
                "the navigator overlaps the grid box at header {hw}"
            );
        }
    }

    /// Every word painted BELOW the ruler's chip strip, with the ink it was
    /// actually drawn in and the box it actually occupies.
    ///
    /// Classified by CONTENT, never by where it landed — a clock word has a
    /// colon in it and a bar number does not. The first version of this
    /// sorted the two by which band their centre fell into, which is
    /// circular when the thing being measured is whether one has grown into
    /// the other's band: an oversized bar number was counted as a clock
    /// word and the overlap measured itself away. All three reverts passed.
    fn ruler_words(v: &ArrangeView<'_>) -> Vec<(String, Rect, Color32)> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let r = rect();
        let l = Layout::new(r, v);
        let (strip, ruler) = (l.meter_strip, l.ruler);
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(r), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(r);
                    draw(ui.painter(), r, v);
                });
            },
        );
        let mut got = Vec::new();
        for s in &out.shapes {
            let egui::Shape::Text(t) = &s.shape else { continue };
            let b = t.visual_bounding_rect();
            // In the RULER and under its chip strip. Without the ruler
            // test this harvested the track headers and the clip names
            // too — twenty-five "bar numbers" on a ruler that shows a
            // dozen, and a gap of minus three hundred points.
            if !ruler.contains(b.center()) || b.center().y < strip.bottom() {
                continue;
            }
            let Some(ink) = t
                .override_text_color
                .or_else(|| t.galley.job.sections.first().map(|s| s.format.color))
            else {
                continue;
            };
            got.push((t.galley.text().to_owned(), b, ink));
        }
        got
    }

    /// **The bar numbers and the clock do not touch, measured as INK.**
    ///
    /// They overlapped by nearly nine points on every theme. The two were
    /// positioned from opposite edges — the number down from the strip's
    /// foot, the clock up from the ruler's — so whatever sat between them
    /// was whatever happened to be left, and once the strip grew to carry
    /// marker names AND signatures there was nothing left. The owner: "the
    /// timecode text has the beats overlapping the seconds. they need a
    /// tiny bit of breathing room."
    ///
    /// Two rectangles that do not overlap prove nothing if the glyphs
    /// inside them do, so this compares the lowest painted bar number with
    /// the highest painted clock word.
    ///
    /// Revert-check performed: anchor the clock below its band again and
    /// the gap goes negative.
    ///
    /// **What this does NOT cover, stated because it was tried:** sizing the
    /// number from `ruler.height() * 0.55` again leaves it GREEN. A twelve
    /// point number still fits inside a thirteen point band, so the font was
    /// never what made the two collide — the anchoring was, and a doc
    /// comment claiming otherwise would be describing a guard nobody has.
    /// The floors below are what stop the opposite cheat: a spacing test can
    /// always be satisfied by shrinking the text until it fits, so the words
    /// have to stay readable while they stay apart.
    #[test]
    fn the_bar_numbers_and_the_clock_have_room_between_them() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let v = a_view(&clips);
        let words = ruler_words(&v);
        let numbers: Vec<&(String, Rect, Color32)> = words
            .iter()
            .filter(|(w, ..)| w.chars().all(|c| c.is_ascii_digit()) && !w.is_empty())
            .collect();
        let clocks: Vec<&(String, Rect, Color32)> =
            words.iter().filter(|(w, ..)| w.contains(':')).collect();
        assert!(!numbers.is_empty(), "no bar numbers were painted: {words:?}");
        assert!(!clocks.is_empty(), "no clock words were painted: {words:?}");

        let lowest = numbers.iter().fold(f32::NEG_INFINITY, |a, (.., r, _)| a.max(r.bottom()));
        let highest = clocks.iter().fold(f32::INFINITY, |a, (.., r, _)| a.min(r.top()));
        assert!(
            highest - lowest >= 1.0,
            "the beats and the seconds are {:.1}pt apart — {} numbers, {} clock words",
            highest - lowest,
            numbers.len(),
            clocks.len()
        );
        // **And both are still worth reading.** Room between two words is
        // trivially bought by making them smaller, so the floors travel with
        // the gap rather than in a test somebody could satisfy separately.
        let shortest = |set: &[&(String, Rect, Color32)]| {
            set.iter().fold(f32::INFINITY, |a, (.., r, _)| a.min(r.height()))
        };
        // **Measured as painted INK, which is the glyph box and not the
        // font size**: a 9.9-point digit paints 7.0 points of actual mark.
        // The first floors here were written against font sizes and failed
        // on correct code, which is its own small lesson about measuring the
        // frame — measure it, then calibrate to what it says.
        assert!(
            shortest(&numbers) >= 6.5,
            "the bar numbers paint {:.1}pt of ink",
            shortest(&numbers)
        );
        assert!(
            shortest(&clocks) >= 5.5,
            "the clock paints {:.1}pt of ink",
            shortest(&clocks)
        );
    }

    /// **The bar numbers can be READ, on every theme, in the ink the
    /// painter actually used.**
    ///
    /// They were `faint` — a blend toward the strip face — which on three
    /// themes landed almost exactly on the ruler's own tone: 1.29:1 on
    /// Default light, 1.57:1 on Centennial dark, 1.65:1 on Porcelain dark,
    /// against a clock beside them reading 4.4:1 or better. The owner saw it
    /// on Centennial: "the beats are way too light and illegible compared to
    /// the seconds." The theme that SHIPS was the worst of the three, which
    /// is why the answer was the derivation and not one palette's colour.
    ///
    /// **Harvested from the frame, not recomputed.** The first version of
    /// this asked the audit to work out `quiet(on_ink(p.ruler), …)` for
    /// itself and compare that against the ruler — which is true whatever
    /// the painter does, so putting `faint` back left it green. Reading the
    /// ink off the painted glyph is the only version that knows what was
    /// drawn.
    ///
    /// Revert-check performed: put `p.faint` back and this goes red on
    /// Default light at 1.29:1.
    #[test]
    fn the_bar_numbers_are_legible_on_every_theme() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                let mut v = a_view(&clips);
                v.dark_mode = dark;
                v.wood = (set.recorder_bg_color.r, set.recorder_bg_color.g, set.recorder_bg_color.b);
                v.fx_strip = (set.fx_strip_color.r, set.fx_strip_color.g, set.fx_strip_color.b);
                v.meter_bg = (set.meter_bg_color.r, set.meter_bg_color.g, set.meter_bg_color.b);
                let ruler = ink(&v).ruler;
                let mode = if dark { "dark" } else { "light" };
                let words = ruler_words(&v);
                let mut seen = false;
                for (w, _, tint) in &words {
                    if w.is_empty() || !w.chars().all(|c| c.is_ascii_digit()) {
                        continue;
                    }
                    seen = true;
                    let r = crate::recorder_panel::contrast_ratio(*tint, ruler);
                    if r < 4.0 {
                        bad.push(format!("{} {mode}: bar number {r:.2}:1", theme.name));
                        break;
                    }
                }
                assert!(seen, "{} {mode}: no bar number was painted", theme.name);
            }
        }
        assert!(bad.is_empty(), "unreadable bar numbers:\n  {}", bad.join("\n  "));
    }

    /// **The strip is tall enough to read, and its words are one size.**
    ///
    /// The owner asked to "heighten the loop/timereadout strip" because at
    /// the old height its third of the ruler was eight points and its labels
    /// were set at six — present, hittable, unreadable. This pins the floor
    /// so a later tightening of `RULER_H` cannot quietly take it back, and
    /// pins the flags to the TOP of the strip so the pole can be seen
    /// crossing the rest of it, which is the picture that was asked for.
    ///
    /// Revert-check performed: put `RULER_H` back to `(0.06, 16.0, 24.0)`
    /// and the font floor goes red at 8.88pt.
    #[test]
    fn the_ruler_strip_can_be_read_and_its_flags_sit_on_top_of_it() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let v = a_view(&clips);
        let l = Layout::new(rect(), &v);
        assert!(
            l.meter_strip.height() >= 12.0,
            "the strip is {:.1}pt tall, which cannot carry a word",
            l.meter_strip.height()
        );
        assert!(
            strip_font_h(l.meter_strip) >= 9.0,
            "the strip's words are {:.2}pt",
            strip_font_h(l.meter_strip)
        );
        // Atop, not filling: the pole needs strip left to cross.
        let box_h = strip_box_h(l.meter_strip);
        assert!(
            box_h < l.meter_strip.height(),
            "a box filling the strip leaves the pole nowhere to be seen"
        );
        assert!(
            box_h >= 9.0,
            "the box is {box_h:.1}pt and the words are {:.1}pt",
            strip_font_h(l.meter_strip)
        );
        // And the ruler still has room under the strip for the bar numbers
        // and the clock, which is what it grew for.
        assert!(
            l.ruler.height() - l.meter_strip.height() >= 12.0,
            "the strip ate the ruler: {:.1} of {:.1}",
            l.meter_strip.height(),
            l.ruler.height()
        );
    }

    /// **A flag says its name and nothing else, a signature sits on the row
    /// under it, and each answers the pointer in its own row.**
    ///
    /// The owner's redirection: "no more 'name (timesig)' just 'name' and
    /// time sig is seperate and when they are both on one spot, the time sig
    /// will sit below the label." So the strip is two rows — see
    /// [`flag_row`] — and a marker sharing a bar with a change no longer
    /// prints "Chorus (3/4)"; it prints "Chorus", with "3/4" beneath it.
    ///
    /// The fixture puts a marker on bar 4 AND a signature change on bar 4,
    /// because that co-location is the whole subject: at 120 BPM in 4/4 a
    /// bar is two seconds, so bar 4 is eight seconds and the two really do
    /// land on the same pixel.
    ///
    /// Revert-checks, all three performed: put the `sig` back into the
    /// label and the "no parentheses" assert goes red; draw the flags in
    /// `sig_row` and the stacking assert goes red; hit-test the whole strip
    /// for flags rather than the name row and the last block goes red,
    /// because the flag then answers for a press on the signature under it.
    #[test]
    fn a_flag_says_its_name_and_a_signature_sits_on_the_row_below_it() {
        let clips: Vec<ClipView<'_>> = Vec::new();
        let mut v = a_view(&clips);
        v.meter = crate::tempo::MeterMap::sanitised(vec![
            crate::tempo::MeterPoint {
                at_bar: 0,
                sig: crate::recorder::TimeSignature { beats: 4, unit: 4 },
            },
            crate::tempo::MeterPoint {
                at_bar: 4,
                sig: crate::recorder::TimeSignature { beats: 3, unit: 4 },
            },
        ]);
        v.markers = vec![
            MarkerView { at_s: 4.0, name: "Verse".to_owned(), selected: false },
            MarkerView { at_s: 8.0, name: "Chorus".to_owned(), selected: false },
            MarkerView { at_s: 12.0, name: String::new(), selected: false },
        ];
        assert!(
            (bar_at(&v, 4) - 8.0).abs() < 1e-9,
            "the fixture's chip and flag are not on the same moment"
        );

        let words = strip_words(&v);
        assert!(words.iter().any(|w| w == "Verse"), "{words:?}");
        assert!(
            words.iter().any(|w| w == "Chorus"),
            "the marker sharing a bar with a change did not say its plain name: {words:?}"
        );
        assert!(
            !words.iter().any(|w| w.contains('(')),
            "a flag is still printing its signature in parentheses: {words:?}"
        );
        assert!(words.iter().any(|w| w == "3/4"), "the signature is gone: {words:?}");
        assert!(
            words.iter().any(|w| w == "\u{2691}"),
            "an unnamed marker drew nothing to aim at: {words:?}"
        );

        // **Stacked, not merged**: the name is ABOVE the signature that
        // shares its moment.
        let l = Layout::new(rect(), &v);
        let names = flag_row(l.meter_strip);
        let sigs = sig_row(l.meter_strip);
        assert!(
            names.bottom() <= sigs.top() + 0.01 && names.top() < sigs.top(),
            "the rows are not stacked: names {names:?} sigs {sigs:?}"
        );
        assert_eq!(
            painted_in(&v, names),
            vec!["Verse", "Chorus", "\u{2691}"],
            "the name row is not carrying exactly the names"
        );
        // Bar 0's chip draws too, because this song HAS a change: the
        // "no chip on bar zero" rule is for songs in one signature, where
        // there is nothing for the first bar to differ from.
        assert_eq!(
            painted_in(&v, sigs),
            vec!["4/4", "3/4"],
            "the signature row is not carrying exactly the signatures"
        );

        // And each row answers the pointer for its own kind.
        for (i, m) in v.markers.iter().enumerate() {
            let x = x_of(rect(), &v, m.at_s) + 3.0;
            assert_eq!(
                hit_test(rect(), &v, Pos2::new(x, names.center().y)),
                Some(Hit::Flag(i)),
                "the flag for {:?} is not where it was painted",
                m.name
            );
        }
        // The chip under "Chorus" belongs to the chip, not to the flag.
        let x = x_of(rect(), &v, bar_at(&v, 4)) + 3.0;
        assert_eq!(
            hit_test(rect(), &v, Pos2::new(x, sigs.center().y)),
            Some(Hit::MeterChip(4)),
            "a press on the signature under a flag was taken by the flag"
        );
    }

    /// **Render the timeline's track headers to a file, so a person can
    /// look at them.**
    ///
    /// Not a test of anything: an eye. Every layout fault this panel has had
    /// was found by looking at a picture and none of them by a test — a
    /// number that fits its cell arithmetically can still be a word jammed
    /// under a knob, and a test that measures rectangles cannot see it.
    ///
    /// `face::fit` is `busy.rs`'s software rasteriser: no window, no GPU,
    /// every triangle egui would have sent to the card, text included.
    ///
    ///     IVORY_SHOT=/tmp/headers.png \
    ///       cargo test -p tangent-ui --lib arrangement_panel::shot \
    ///       -- --ignored --nocapture
    ///
    /// `IVORY_SHOT_W` / `IVORY_SHOT_H` set the window and `IVORY_SHOT_HDR`
    /// the header column, because this panel's whole shape is a function of
    /// how much room it was given. `IVORY_SHOT_PANEL` draws a panel WIDER
    /// than the window and lets the window clip it, which is how to get a
    /// picture of the header column alone at full size rather than a
    /// thumbnail of the whole timeline.
    #[test]
    #[ignore = "writes a picture for a person to look at"]
    fn headers() {
        let Ok(out) = std::env::var("IVORY_SHOT") else {
            eprintln!("IVORY_SHOT not set");
            return;
        };
        let num = |k: &str, d: f32| {
            std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(d)
        };
        let (w, hgt) = (num("IVORY_SHOT_W", 1512.0), num("IVORY_SHOT_H", 620.0));
        let hdr = std::env::var("IVORY_SHOT_HDR").ok().and_then(|v| v.parse::<f32>().ok());
        let c = clips();
        let mut v = a_view(&c);
        v.lanes[1].is_folder = true;
        if let Some(x) = hdr {
            v.header_w = Some(x);
        }
        let names = ["VOX", "GTR BUS", "SNARE"];
        let ins = ["IN 1", "", "MIDI+ 10"];
        let gains = [1.0_f32, 0.25, 0.5];
        let pans = [0.0_f32, -0.5, 0.32];
        for (i, lane) in v.lanes.iter_mut().enumerate().take(3) {
            lane.name = names[i];
            lane.input = ins[i].to_owned();
            lane.input_short = ins[i].to_owned();
            lane.gain = gains[i];
            lane.pan = pans[i];
            lane.send = 0.4;
        }
        let ctx = crate::face::fit::a_ctx(crate::fonts::FontChoice::default());
        let surface = crate::recorder_panel::surface_of(Color32::from_rgb(0x61, 0x37, 0x3A));
        let panel_w = num("IVORY_SHOT_PANEL", w);
        let (pw, ph, rgba) = crate::face::fit::shoot(
            &ctx,
            Vec2::new(w, hgt + crate::shell::CHROME_BAR_H),
            "Tangent \u{2014} arrangement",
            &surface,
            &mut |ui| {
                let win = ui.max_rect();
                ui.set_clip_rect(win);
                draw(
                    ui.painter(),
                    Rect::from_min_size(win.min, Vec2::new(panel_w, win.height())),
                    &v,
                );
            },
        );
        crate::face::fit::write_png(std::path::Path::new(&out), &rgba, pw, ph);
        println!("wrote {out} ({pw}x{ph}) from a {w}x{hgt} panel");
    }

    /// The words one frame painted inside `band`.
    fn painted_in(v: &ArrangeView<'_>, band: Rect) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let r = rect();
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(r), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(r);
                    draw(ui.painter(), r, v);
                });
            },
        );
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if band.contains(t.visual_bounding_rect().center()) => {
                    Some(t.galley.text().to_owned())
                }
                _ => None,
            })
            .collect()
    }

    /// Every word one frame of the arrangement painted INTO THE RULER'S
    /// STRIP — the band the flags and the signature chips share.
    ///
    /// Harvested out of a real `draw` into a real context rather than asked
    /// of the label-making code, because this panel has twice been caught by
    /// a test that measured a helper the painter happens to call.
    fn strip_words(v: &ArrangeView<'_>) -> Vec<String> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let r = rect();
        let strip = Layout::new(r, v).meter_strip;
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(r), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(r);
                    draw(ui.painter(), r, v);
                });
            },
        );
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if strip.intersects(t.visual_bounding_rect()) => {
                    Some(t.galley.text().to_owned())
                }
                _ => None,
            })
            .collect()
    }
    /// **The marker strip is its own surface, on every theme, in both
    /// modes.**
    ///
    /// It is drawn in the ruler's colour and it sits directly under the
    /// band, so if the two match there is no strip — just a taller band with
    /// flags floating in it. That is what shipped: a 55% blend of the band's
    /// wood and the lane field is a colour BETWEEN its two neighbours, and on
    /// a dark theme both neighbours are near-black. Measured, fourteen of the
    /// thirty-four theme-and-mode pairs sat under 1.10:1 against the band and
    /// two — Valentine and Blueprint in dark mode — were the same colour to
    /// the byte. The owner: "i can barely discern it from the recorder
    /// color, it should be legibly separate."
    ///
    /// 1.3:1 is a floor for two large adjacent FIELDS, not for ink on a
    /// ground: it is the point where an edge between two fills reads as an
    /// edge rather than as a seam you have to look for. The whole set clears
    /// 1.35 now and the widest is Porcelain dark at 2.77, so this is a floor
    /// with the set above it rather than a bar drawn around the worst case.
    ///
    /// Revert-check: take the `separate` walk out of `ink`'s `ruler` and
    /// this names fourteen pairs, the first of them at 1.00:1.
    #[test]
    fn the_marker_strip_is_legibly_separate_from_the_band() {
        use crate::recorder_panel::contrast_ratio;
        let clips: Vec<ClipView<'_>> = Vec::new();
        let mut bad: Vec<String> = Vec::new();
        for (theme, dark) in crate::theme::factory()
            .into_iter()
            .flat_map(|t| [(t.clone(), false), (t, true)])
        {
            let mut st = crate::settings::Settings::default();
            theme.apply(&mut st);
            st.dark_mode = dark;
            let mut v = a_view(&clips);
            v.dark_mode = dark;
            let c = st.recorder_bg_color;
            v.wood = (c.r, c.g, c.b);
            let c = st.fx_strip_color;
            v.fx_strip = (c.r, c.g, c.b);
            let c = st.meter_bg_color;
            v.meter_bg = (c.r, c.g, c.b);
            let band = Color32::from_rgb(v.wood.0, v.wood.1, v.wood.2);
            let r = contrast_ratio(ink(&v).ruler, band);
            if r < 1.3 {
                bad.push(format!(
                    "{} {}: strip {r:.2}:1 against the band",
                    theme.name,
                    if dark { "dark" } else { "light" }
                ));
            }
        }
        assert!(
            bad.is_empty(),
            "the marker strip disappears into the band:\n  {}",
            bad.join("\n  ")
        );
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
            master_bypassed: [false; crate::recorder::INSERTS * 2],
            show_tempo_lane: false,
            tempo_hot: None,
            meter_edit: None,
            markers: Vec::new(),
            marker_edit: None,
            fx_strip: (0xE3, 0xD9, 0xBD),
            meter_bg: (0x0B, 0x0E, 0x0A),
            header_w: None,
            naming: None,
            time_sel: None,
            selected_channel: None,
            selected_tracks: Vec::new(),
            lanes: Lanes::dense((0..CHANNELS).map(|i| a_lane(i < 3)).collect()),
            lane_scroll: 0,
            visible_lanes: VISIBLE_LANES,
            active: CHANNELS,
            clips,
            metronome_on: true,
            snap_on: true,
            grip_lit: false,
            drop_lane: None,
            auto_marquee: None,
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

    /// What size, and where, every word in a band was painted at.
    fn sized_in_face_at(
        v: &ArrangeView<'_>,
        band: Rect,
        face: crate::fonts::FontChoice,
        r: Rect,
    ) -> Vec<(String, f32, Rect)> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, face, None);
        crate::fonts::apply_text_styles(&ctx);
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(r), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(r);
                    draw(ui.painter(), r, v);
                });
            },
        );
        out.shapes
            .iter()
            .filter_map(|s| match &s.shape {
                egui::Shape::Text(t) if band.contains(t.visual_bounding_rect().center()) => Some((
                    t.galley.job.text.clone(),
                    t.galley.job.sections.first().map_or(0.0, |x| x.format.font_id.size),
                    t.visual_bounding_rect(),
                )),
                _ => None,
            })
            .collect()
    }

    /// **Every word on a track's header row is ONE size.**
    ///
    /// The name, the input button, the bays, ROUTING, the three figures over
    /// the dials and the three words under them. The owner's rule in his own
    /// terms: the knob text and ROUTING "must not be bigger than the text for
    /// the track label and fx insert text — make all text on these the same
    /// size".
    ///
    /// They used to be four rules in four places — a name off its own band's
    /// height, a chip off its own box's, a figure off its cell's width and a
    /// word under a dial off the band below it — so the row read as three
    /// panels that happened to be adjacent. Now one number is worked out from
    /// the tightest cell (almost always the volume figure: five characters in
    /// a cell as wide as the knob) and everything wears it. See
    /// `header_type_size`.
    ///
    /// Asserted on all three faces, because the width budget is measured in
    /// whichever face is installed and a test pinned to one would not see a
    /// face-specific fault.
    #[test]
    fn one_size_dresses_a_tracks_whole_header_row() {
        let c = clips();
        let mut v = a_view(&c);
        // A lane whose volume figure is the LONG one — five characters, which
        // is the case the size is decided by. At unity it is four and the
        // test would be measuring the easy half.
        v.lanes[0].gain = 0.25;
        v.lanes[0].pan = -0.5;
        v.lanes[0].send = 0.4;
        v.lanes[0].name = "VOX";
        v.lanes[0].input = "IN 1".to_owned();
        v.lanes[0].input_short = "IN 1".to_owned();
        v.lanes[0].inserts = ["COMP", "", "", "", "", ""];
        // Measured at a header the size this app opens at rather than the
        // narrow fixture, whose dial column is thirty-eight points wide and
        // would be asserting the squeeze rather than the rule.
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let l = Layout::new(r, &v);
        let row = l.header_rows[0];
        // The label band ENDS where ROUTING starts, or ROUTING's own word
        // would be sampled twice.
        let floor_y = if row.routing.is_positive() {
            row.routing.top()
        } else {
            row.panel.bottom() - 1.0
        };
        let labels = Rect::from_min_max(
            Pos2::new(row.pan.left() - 2.0, row.knob.bottom()),
            Pos2::new(row.send.right() + 2.0, floor_y),
        );
        let mid = (row.knob.bottom() + floor_y) * 0.5;
        let mut bad: Vec<String> = Vec::new();
        for face in [
            crate::fonts::FontChoice::Courier,
            crate::fonts::FontChoice::Terminess,
            crate::fonts::FontChoice::JetBrains,
        ] {
            let mut seen: Vec<(String, f32)> = Vec::new();
            for (what, cell) in [
                ("name", row.name),
                ("input", row.input),
                ("bay", row.bays[0]),
                ("routing", row.routing),
                ("volume", row.knob_readout),
                ("send", row.send_readout),
                ("pan", row.pan_readout),
                ("labels", labels),
            ] {
                let painted = sized_in_face_at(&v, cell, face, r);
                if painted.is_empty() {
                    bad.push(format!("{face:?}: {what} painted nothing"));
                }
                for (text, size, at) in painted {
                    // It fits its cell. A figure that overflows its dial runs
                    // into the next figure and the three read as one number.
                    if what != "labels" && at.width() > cell.width() {
                        bad.push(format!(
                            "{face:?}: {what} {text:?} is {:.1} wide in {:.1}",
                            at.width(),
                            cell.width()
                        ));
                    }
                    // And a word under a dial is CENTRED in its band, not
                    // hung from the top edge.
                    if what == "labels" && (at.center().y - mid).abs() > 1.5 {
                        bad.push(format!(
                            "{face:?}: the {text:?} label is {:+.2} off the middle of its band",
                            at.center().y - mid
                        ));
                    }
                    seen.push((format!("{what} {text:?}"), size));
                }
            }
            // **The whole point: one size.**
            if let Some(first) = seen.first().map(|(_, s)| *s) {
                for (what, size) in &seen {
                    if (size - first).abs() > 0.01 {
                        bad.push(format!(
                            "{face:?}: {what} is {size:.1}pt against {first:.1}pt elsewhere \
                             on the same row: {seen:?}"
                        ));
                    }
                }
                // And worth reading. Nine on the shipped face at a maximised
                // pane, which is where the owner works.
                let floor = if face == crate::fonts::FontChoice::Terminess { 9.0 } else { 7.0 };
                if first < floor {
                    bad.push(format!("{face:?}: the whole row is {first:.1}pt"));
                }
            } else {
                bad.push(format!("{face:?}: the header row painted nothing at all"));
            }
        }
        assert!(bad.is_empty(), "the header's type:\n  {}", bad.join("\n  "));
    }

    /// **Four switches of one size, and Ø in the corner on its own.**
    ///
    /// The theory send and the automation button were a third smaller than
    /// the mute and the solo, which said "these two are lesser controls";
    /// they are not, and a note and a wrench need room a letter does not.
    /// The owner: "same size buttons now for theory view route, automation,
    /// mute, and solo", and "move the phase button to the bottom right".
    ///
    /// Ø is deliberately NOT one of the four. It is a thing you set once and
    /// they are things you reach for while playing, and it sat in their
    /// column making three switches read as one set of three.
    #[test]
    fn the_four_switches_are_one_size_and_phase_sits_in_the_corner() {
        let c = clips();
        let v = a_view(&c);
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let l = Layout::new(r, &v);
        let row = l.header_rows[0];
        let four = [
            ("theory", row.theory),
            ("automation", row.automation),
            ("mute", row.mute),
            ("solo", row.solo),
        ];
        for (what, cell) in four {
            assert!(cell.is_positive(), "{what} was shed at a full-size header");
            assert!(
                (cell.width() - row.mute.width()).abs() < 0.5
                    && (cell.height() - row.mute.height()).abs() < 0.5,
                "{what} is {:?} against the mute's {:?}",
                cell.size(),
                row.mute.size()
            );
        }
        // **Ø, in the bottom-right corner**: below the switches, right of the
        // meter, inside its own plate.
        assert!(row.phase.is_positive(), "the polarity switch was shed");
        assert!(
            row.phase.top() > row.solo.bottom(),
            "Ø is still up in the switch column"
        );
        assert!(
            row.phase.right() <= row.panel.right() && row.phase.bottom() <= row.panel.bottom(),
            "Ø overhangs its own row"
        );
        assert!(
            row.panel.right() - row.phase.right() < 6.0
                && row.panel.bottom() - row.phase.bottom() < 6.0,
            "Ø is not in the corner: {:?} in a row ending at {:?}",
            row.phase.max,
            row.panel.max
        );
        assert!(
            row.phase.width() < row.mute.width(),
            "Ø is one of the four rather than the quiet one beside them"
        );
        // **The mute above the solo, not beside it** — the owner's "make mute
        // and solo atop one another". Side by side they cost the row two
        // switches' width out of the middle, which is width the dials and the
        // name column were competing for.
        assert!(
            (row.mute.left() - row.solo.left()).abs() < 0.5,
            "M and S are not in one column: {:?} and {:?}",
            row.mute.min,
            row.solo.min
        );
        assert!(
            row.mute.bottom() <= row.solo.top() + 0.5,
            "the mute is not above the solo"
        );
    }

    /// **A chip keeps its width and gives up a little height.**
    ///
    /// The bays, the input button and ROUTING took the whole of their band,
    /// so the column read as a stack of slabs with hairline gaps rather than
    /// as a rack of chips — the owner's "slightly less in height, but keep
    /// the length". The width is what makes a word readable; the height is
    /// only what makes the box a box.
    #[test]
    fn a_chip_keeps_its_width_and_gives_up_a_little_height() {
        let c = clips();
        let v = a_view(&c);
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let l = Layout::new(r, &v);
        let row = l.header_rows[0];
        // The band a rack was cut from: the gap between one rack's row and
        // the next is what the chips gave back.
        let pitch = row.bays[3].center().y - row.bays[0].center().y;
        for (what, cell) in [
            ("bay", row.bays[0]),
            ("input", row.input),
            ("routing", row.routing),
        ] {
            assert!(cell.is_positive(), "{what} was shed at a full-size header");
            assert!(
                cell.height() < pitch * 0.92,
                "{what} still fills its band: {:.1} tall on a {:.1} pitch",
                cell.height(),
                pitch
            );
            assert!(
                cell.height() > pitch * 0.60,
                "{what} gave up more than a little: {:.1} tall on a {:.1} pitch",
                cell.height(),
                pitch
            );
        }
        // And the WIDTH did not move: a bay still spans its third of the
        // column, and the input button still matches one.
        assert!(
            (row.input.width() - row.bays[0].width()).abs() < 0.5,
            "the input button stopped matching a bay's width"
        );
    }

    /// **A FOLDER wears no note, and the name column does not move because
    /// of it.**
    ///
    /// A group carries no MIDI of its own, so there is nothing for it to
    /// send to the theory view — the desk has said so since the T left the
    /// switch row, and this header went on drawing a glyph for a routing
    /// that could never be on. The owner: "the note glyph needs to be gone
    /// on folders in arrangement view."
    ///
    /// The second half is the fault this kind of change invites: shed the
    /// CELL and the name column, which anchors off it, starts somewhere else
    /// on a folder's row — a header whose columns do not line up with the
    /// tracks under it. The SEAT stands; only the field goes.
    #[test]
    fn a_folder_wears_no_note_and_keeps_the_name_column_where_it_is() {
        let c = clips();
        let mut v = a_view(&c);
        v.lanes[1].is_folder = true;
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let l = Layout::new(r, &v);
        let track = l.header_rows[0];
        let folder = l.header_rows[1];
        assert!(track.theory.is_positive(), "a track lost its note");
        assert!(
            !folder.theory.is_positive(),
            "a folder still wears the theory note"
        );
        // And it cannot be pressed, which is the other half of gone: the
        // seat the note WOULD have had, on the folder's own row.
        let seat = Pos2::new(track.theory.center().x, folder.panel.center().y);
        assert_ne!(
            hit_test(r, &v, seat),
            Some(Hit::ToTheory(1)),
            "the folder's absent note still answers a press"
        );
        assert!(
            (track.name.left() - folder.name.left()).abs() < 0.5,
            "the folder's name column moved: {} against the track's {}",
            folder.name.left(),
            track.name.left()
        );
        // The wrench below it stays: a group's rows are automatable.
        assert!(
            folder.automation.is_positive(),
            "the folder lost its automation button with the note"
        );
    }

    /// **The two racks are the same height.**
    ///
    /// Six bays on a lane is six bays of ONE size; the bands were 0.34..0.63
    /// and 0.67..1.00, which is a top rack an eighth thinner than the bottom
    /// one — the owner's "right now it feels like the top one is slightly
    /// thinner". Measured on the bays themselves rather than on the bands,
    /// because the bays are what anybody looks at.
    #[test]
    fn the_two_racks_are_the_same_height() {
        let c = clips();
        let v = a_view(&c);
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let l = Layout::new(r, &v);
        let bays = l.header_rows[0].bays;
        for b in &bays {
            assert!(b.is_positive(), "a bay was shed at a full-size header");
        }
        let top = bays[0].height();
        let bottom = bays[crate::recorder::INSERTS].height();
        assert!(
            (top - bottom).abs() < 0.5,
            "the top rack is {top:.2} tall against the bottom's {bottom:.2}"
        );
        // And every bay in a rack matches its neighbours, which is what
        // makes the pair of racks read as one set of six.
        for (i, b) in bays.iter().enumerate() {
            assert!(
                (b.height() - top).abs() < 0.5,
                "bay {i} is {:.2} tall against bay 0's {top:.2}",
                b.height()
            );
        }
    }

    /// **The theory switch's note is DRAWN, not set.**
    ///
    /// Two faces were tried and neither read as a note: U+266A out of a
    /// terminal face is a square head on a straight flag, and the Nerd Font's
    /// is a round OPEN head, which is a minim. A notehead is a filled oblong
    /// and it is tilted, and that tilt is most of what the eye recognises —
    /// the owner's "fill in the note glyph, make it oblong and angled like a
    /// real note".
    ///
    /// So the assertion is about the shapes in the frame: a filled polygon
    /// inside the switch, and no TEXT there at all. See
    /// `recorder_panel::draw_note`.
    #[test]
    fn the_theory_switch_draws_its_note_rather_than_setting_it() {
        let c = clips();
        let mut v = a_view(&c);
        v.lanes[0].to_theory = true;
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 900.0));
        let cell = Layout::new(r, &v).header_rows[0].theory;
        assert!(cell.is_positive(), "the theory switch was shed");
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(r), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(r);
                    draw(ui.painter(), r, &v);
                });
            },
        );
        let mut polys = 0usize;
        for s in &out.shapes {
            match &s.shape {
                egui::Shape::Path(pp) => {
                    if pp.fill.a() > 0 && cell.contains(pp.visual_bounding_rect().center()) {
                        polys += 1;
                    }
                }
                egui::Shape::Text(t) => {
                    assert!(
                        !cell.contains(t.visual_bounding_rect().center()),
                        "the theory switch is setting {:?} rather than drawing a note",
                        t.galley.text()
                    );
                }
                _ => {}
            }
        }
        // The head and the flag: two filled polygons, plus a stroked stem
        // which is a line and not counted here.
        assert!(
            polys >= 2,
            "the theory switch drew {polys} filled shapes; the note is a head and a flag"
        );
    }

    /// **Six bays on every lane, the input beside the name, ROUTING level
    /// with the bottom rack.**
    ///
    /// Three rulings in one header, all the owner's: "ALL tracks and folders
    /// now have 6 fx slots", "move the input selector on tracks in
    /// arrangement view to be a button to the right of the name field", and
    /// "the routing button can be at the bottom the same height of the fx
    /// inserts". They are one test because they are one rearrangement — the
    /// second rack needed a band, the input gave up the one it had, and
    /// routing came down to sit level with the rack it now shares a row with.
    ///
    /// Measured at a header the size this app actually opens at rather than
    /// the narrow fixture: the fixture's column is 38 points wide, which is
    /// under the floor for both a second rack and a chip beside the name, so
    /// it would assert the fallback and never the arrangement.
    #[test]
    fn every_lane_wears_six_bays_with_its_input_beside_its_name() {
        let c = clips();
        let mut v = a_view(&c);
        v.lanes[1].is_folder = true;
        let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(1512.0, 620.0));
        let l = Layout::new(r, &v);
        for i in [0usize, 1] {
            let row = l.header_rows[i];
            let what = if v.lanes[i].is_folder { "folder" } else { "track" };
            for (b, cell) in row.bays.iter().enumerate() {
                assert!(cell.is_positive(), "{what}: bay {b} was shed");
            }
            // The chain reads DOWN the column: 0..2 above 3..5, which is the
            // order the mixer's rack shows and the order the audio takes.
            assert!(
                row.bays[2].bottom() <= row.bays[3].top() + 0.5,
                "{what}: the second rack is not under the first"
            );
            // **ROUTING is level with the bottom rack**, to the point.
            assert!(row.routing.is_positive(), "{what}: no routing button");
            assert!(
                (row.routing.top() - row.bays[3].top()).abs() < 0.5
                    && (row.routing.bottom() - row.bays[3].bottom()).abs() < 0.5,
                "{what}: routing spans {:?} and the bays beside it {:?}",
                (row.routing.top(), row.routing.bottom()),
                (row.bays[3].top(), row.bays[3].bottom()),
            );
            // And it is in the DIAL column, not the name column.
            assert!(
                row.routing.left() >= row.bays[3].right(),
                "{what}: routing is over the rack"
            );
        }
        // The INPUT: on a track, beside the name and to its right, sharing
        // the band. On a folder there is nothing to select and no button.
        let track = l.header_rows[0];
        assert!(track.input.is_positive(), "the track lost its input button");
        assert!(
            track.input.left() >= track.name.right(),
            "the input button is not to the right of the name"
        );
        // **Thin as an fx slot and centred on the name's line**, the owner's
        // revision: it used to fill the band's whole height, which made the
        // one control on this column that is not a rack slot the biggest box
        // on it.
        assert!(
            (track.input.center().y - track.name.center().y).abs() < 0.5,
            "the input button is not centred on the name's own line"
        );
        assert!(
            track.input.height() <= track.bays[0].height() + 0.5,
            "the input button is taller than a bay: {:.1} against {:.1}",
            track.input.height(),
            track.bays[0].height()
        );
        // **And a bay's WIDTH** — "lower the width of the input selector
        // button so it matches the dimensions of the fx inserts". It had a
        // share of the band, which made it the widest chip on a column whose
        // whole idiom is a row of equal ones.
        assert!(
            (track.input.width() - track.bays[0].width()).abs() < 0.5,
            "the input button is {:.1} wide against a bay's {:.1}",
            track.input.width(),
            track.bays[0].width()
        );
        assert!(
            (track.input.right() - track.bays[2].right()).abs() < 0.5,
            "the input button does not end where the rack under it ends"
        );
        // **Breathing room at both ends of the rack**, and ROUTING keeping
        // the same margin — the owner's "so they have breathing room between
        // them and the margin of the element", and "routing to match as
        // well". The bays used to run edge to edge of the column.
        // The FOLDER's name band is the column's full width — it has no input
        // button eating its right end — so it is the column both rows'
        // margins are measured against.
        let col = l.header_rows[1].name;
        for (what, row) in [("track", track), ("folder", l.header_rows[1])] {
            assert!(
                row.bays[0].left() - col.left() > 1.5,
                "{what}: the first bay is flush with the column's left edge"
            );
            assert!(
                col.right() - row.bays[2].right() > 1.5,
                "{what}: the last bay is flush with the column's right edge"
            );
            assert!(
                row.routing.left() > row.knob.left() - row.knob.width() - 4.0,
                "{what}: routing runs past the dial column's own margin"
            );
        }
        assert!(
            track.input.bottom() <= track.bays[0].top() + 0.5,
            "the input button is not above the first rack"
        );
        assert!(
            !l.header_rows[1].input.is_positive(),
            "the folder grew an input selector"
        );
    }

    /// **A column too narrow for both gives the input its own band back.**
    ///
    /// Binding an input is how a bare lane becomes a track, so it is the last
    /// thing on this column to go — ahead of every bay. At the narrow
    /// fixture the chip cannot sit beside the name, and what must NOT happen
    /// is that it simply disappears.
    #[test]
    fn a_narrow_header_keeps_its_input_and_sheds_bays_instead() {
        let c = clips();
        let v = a_view(&c);
        let l = Layout::new(rect(), &v);
        let row = l.header_rows[0];
        assert!(row.input.is_positive(), "the narrow header shed its input");
        assert!(
            row.input.top() >= row.name.bottom() - 0.5,
            "the input did not fall back to its own band"
        );
        assert!(
            !row.bays[3].is_positive(),
            "the second rack survived a header too narrow for the input"
        );
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
                .find(|x| x.kind == RowKind::Automation(LaneId::Row(0), 0))
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
            Some(Hit::BusGain(Column::Master)),
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
                .find(|x| x.kind == RowKind::Automation(LaneId::Row(0), 0))
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
    /// mixer's own colours — and `Cmd`/`Ctrl+Shift+M` hides it.**
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
        // Above every track. The tempo is not above it any more — it draws
        // IN the master's lane, which is the empty half of a row that
        // carries no clips.
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
        assert_eq!(
            tempo.lane, masters[0].lane,
            "the tempo does not draw in the master's lane"
        );
        assert!(
            !tempo.header.is_positive(),
            "the tempo took a header again - an unlabelled empty strip is \
             the thing this layout exists to stop"
        );
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
            Some(Hit::BusGain(Column::Master)),
            "the master's level cannot be pressed"
        );
        // (It answers the master's COLUMN by name: it was the number two
        // past the last channel, which moved whenever the desk grew, pinned
        // here by an equality that is now the type itself.)

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
            // "Song tempo, flatten to one tempo" is NOT here any more: the
            // tempo lost its header when it moved into the master's lane,
            // and flatten is the right button over the curve.
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
    /// bus was addressed as the lane one past the lanes array, so
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
        // The number a reader heard when the master was the lane one past
        // the channels: its automation lane, counted from one.
        let ghost = format!("Track {}", crate::desk_lift::V32_MASTER_LANE + 1);
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
    ///
    /// The printed spelling changed under this test — the quarter rung reads
    /// "1/4" rather than `\u{266a}` now — and the SPOKEN assertion below did
    /// not, deliberately: the reader names the note, whatever the box draws.
    /// It used to name it by matching the box's text, so a respelt box could
    /// drop the quarter into an "of a bar" catch-all; it sorts the division
    /// itself now (`note_value`), and `every_step_of_every_meter_is_said_as_a_note`
    /// holds every step of every meter to a note's name.
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
        assert_eq!(grid_label_text(-2, 4, 4), "1/4");
        assert!((grid_quarters(-2, 4, 4) - 1.0).abs() < 1e-9);
        // 6/8 cannot halve its bar twice without landing between beats, so
        // the second rung is the beat itself: an eighth.
        assert_eq!(grid_label_text(-2, 6, 8), "1/8");
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
                for ch in 0..CHANNELS {
                    let (r, g, b) = STRIP_COLORS[pal.slot(ch)];
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
                for row in [0usize, 1] {
                    let lane = LaneId::Row(row);
                    let g = auto_ground(&p, lane);
                    let parent = if row % 2 == 0 { p.lane } else { p.lane_alt };
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
                // **The ruler's bar numbers are NOT audited here.** They
                // were, briefly, by working out the same `quiet(on_ink(...))`
                // this file's painter uses and measuring that — which is
                // true whatever the painter actually does, so putting the
                // old `faint` back left it green. See
                // `the_bar_numbers_are_legible_on_every_theme`, which reads
                // the ink off the painted glyph instead.
                // **A marker flag, per theme.** Its body is the theme's own
                // accent and its label is whatever ink that body can carry,
                // so both pairings move together on all sixteen — and the
                // surface is the RULER, which is where the flag is actually
                // drawn. The first thing this catches is a theme whose
                // accent happens to be its ruler's own tone: a flag that is
                // there, hittable, and invisible.
                {
                    let fb = flag_body(
                        &p,
                        (
                            set.white_key_active_color.r,
                            set.white_key_active_color.g,
                            set.white_key_active_color.b,
                        ),
                    );
                    // **Measured on what the fill PRESENTS**, not on the
                    // fill: the flags and chips are translucent over the
                    // ruler now, and a label chosen against the opaque
                    // colour is a label measured against a surface that is
                    // not on screen. `through` is that colour.
                    let seen = through(fb, p.ruler);
                    check("marker flag", seen, p.ruler, 1.5);
                    check("marker flag label", on_ink(seen), seen, 4.5);
                    // The pole is drawn SOLID in the same colour — it is one
                    // pixel wide and a translucent hairline is no line at
                    // all. Structure, so it only has to be seen.
                    check("marker pole", fb, p.ruler, 1.6);
                    // **The signature chip is read by its LABEL, not by
                    // standing off the ruler**, and that is not a
                    // consequence of translucency — it never did. Measured
                    // opaque, `face` against `ruler` is 1.02:1 on Hydra
                    // light, 1.03:1 on Valentine dark, 1.20 to 1.31 across
                    // most of the set, and only Default separates properly
                    // at 5.59:1. Translucency costs it almost nothing (1.20
                    // to 1.15 on Valentine). So there is no surface floor
                    // here: a floor this design has never met would be a
                    // requirement invented by its own test. The label is
                    // what has to read, and it is held to the full bar.
                    let chip = through(p.face, p.ruler);
                    check("signature chip label", on_ink(chip), chip, 4.5);
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
                // **And the BYPASSED one**, which is the same chip with its
                // word pulled twice as far toward the ground and an amber
                // bar down its left edge. Quieter is a matter of weight: a
                // plugin whose name has gone unreadable is a plugin you
                // cannot identify to put back, so it answers the full 4.5.
                check(
                    "bypassed bay chip",
                    quiet(p.engrave, p.hdr_face, 0.45),
                    p.hdr_face,
                    4.5,
                );
                // The bar is a graphic mark and takes the graphic floor —
                // through `lit_on`, because a fixed amber on a themed face
                // is exactly the fault this whole audit exists to catch, and
                // a warm theme is where it would have hidden.
                check(
                    "bypassed bay bar",
                    crate::recorder_panel::lit_on(p.mute, p.hdr_face),
                    p.hdr_face,
                    3.0,
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
                for ch in 0..CHANNELS {
                    let (r, g, b) =
                        STRIP_COLORS[pal.slot(ch)];
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


    /// **The `_in` forms ARE the public forms, to the bit.**
    ///
    /// `draw`, `hit_test` and `targets` read the axis through a Layout built
    /// once per frame; `app` reads it through the public forms once per
    /// gesture. A clip drawn through one and grabbed through the other has to
    /// be the same rectangle — not within a pixel, the SAME — or the clip you
    /// see and the clip you can press come apart by a fraction that grows
    /// with the zoom. So every corner is compared by its bits, over the
    /// panel's own clips, at several windows (whole song, zoomed in, scrolled
    /// past the clips, an hour wide, a second wide) and several rects down to
    /// the degenerate. `hit_test` and `targets` are held to the same bar.
    #[test]
    fn clip_rect_agrees_with_its_in_form_to_the_bit() {
        let c = clips();
        let bits = |r: Rect| {
            [r.min.x.to_bits(), r.min.y.to_bits(), r.max.x.to_bits(), r.max.y.to_bits()]
        };
        for (w, h) in [(1300.0, 420.0), (1920.0, 960.0), (300.0, 100.0), (10.0, 10.0), (0.0, 0.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 7.0), Vec2::new(w, h));
            for (start, len) in
                [(0.0, 120.0), (40.0, 10.0), (200.0, 50.0), (0.0, 3600.0), (9.5, 1.0), (0.0, 0.0)]
            {
                let mut v = a_view(&c);
                v.view_start_s = start;
                v.view_len_s = len;
                let l = Layout::new(r, &v);
                for clip in &c {
                    let a = clip_rect(r, &v, clip);
                    let b = clip_rect_in(&l, &v, clip);
                    assert_eq!(bits(a), bits(b), "clip {} at {w}x{h}, window {start}+{len}: {a:?} vs {b:?}", clip.id);
                }
                for t in [0.0, 10.0, 59.7, 120.0, start, start + len, -3.0] {
                    let x = x_of(r, &v, t);
                    assert_eq!(x.to_bits(), x_of_in(&l, &v, t).to_bits(), "x_of({t})");
                    assert_eq!(
                        time_at(r, &v, x).to_bits(),
                        time_at_in(&l, &v, x).to_bits(),
                        "time_at({x})"
                    );
                }
                for p in [r.center(), r.left_top(), Pos2::new(r.left() + 200.0, r.top() + 60.0)] {
                    assert_eq!(hit_test(r, &v, p), hit_test_in(&l, &v, p), "hit at {p:?}");
                }
                assert_eq!(targets(r, &v), targets_in(&l, &v), "targets at {w}x{h}");
            }
        }
    }

    /// One painted frame of `v`: the shapes it emitted and how many galleys
    /// the font cache holds afterwards — the two things a clip that is not
    /// on screen must not add to.
    fn frame_of(v: &ArrangeView<'_>, area: Rect) -> (Vec<egui::epaint::ClippedShape>, usize) {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(area), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(area);
                    draw(ui.painter(), area, v);
                });
            },
        );
        let galleys = ctx.fonts(|f| f.num_galleys_in_cache());
        (out.shapes, galleys)
    }

    /// **A clip that is not on screen costs the rect test and nothing else.**
    ///
    /// The clip loop derives a colour, measures an ink, lays out a name and
    /// walks a miniature for every clip it reaches; a clip past the window's
    /// end, or on a lane that is scrolled away, must be turned back before
    /// any of that — a two-thousand-clip song is mostly clips nobody is
    /// looking at, and the frame has to be priced by the window, not by the
    /// document. Held as what the frame EMITS: a thousand clips that cannot
    /// be seen add no shape and no galley to a frame that already draws two.
    #[test]
    fn an_offscreen_clip_costs_no_shape_and_no_galley() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        let two = clips();
        let mut many = clips();
        for i in 0..1_000u64 {
            // Past the window's end, on a visible lane; and inside the
            // window, on a lane the eight-lane window does not reach.
            let (lane, start_s) = if i % 2 == 0 { (1, 1_000.0 + i as f64) } else { (30, 5.0) };
            many.push(ClipView {
                id: 100 + i,
                lane,
                start_s,
                len_s: 4.0,
                name: "a name that would need a galley",
                midi: true,
                notes: &notes,
                loop_s: bar_s,
                ..a_looped_clip(&notes, bar_s, 2.0)
            });
        }
        let area = rect();
        let v2 = a_view(&two);
        let vn = a_view(&many);
        assert_eq!(vn.clips.len(), 1_002);
        let (s2, g2) = frame_of(&v2, area);
        let (sn, gn) = frame_of(&vn, area);
        assert_eq!(sn.len(), s2.len(), "a thousand clips off screen added shapes");
        assert_eq!(gn, g2, "a thousand clips off screen laid out text");
    }

    /// **The grid is priced by the pixels, not by the song.**
    ///
    /// Full-height lines through the lanes: one per bar while bars are five
    /// points apart, the gridbox's subdivisions between them when THEY are
    /// five apart, and past that the walk skips by the ruler's own step. So
    /// however wide the window and whatever the gridbox says, the count is
    /// bounded by the width in points — a thousand-hour window at 1/32 must
    /// not ask the UI thread for a million lines.
    #[test]
    fn grid_lines_are_bounded_by_the_width_at_every_zoom() {
        let c = clips();
        let area = rect();
        let l = Layout::new(area, &a_view(&c));
        let field = l.lanes_area;
        let cap = (field.width() / 5.0 * 2.0) as usize + 16;
        for len_s in [1.0, 10.0, 100.0, 1_000.0, 36_000.0, 3.6e6] {
            for exp in [-6i8, -2, 0, 3] {
                let mut v = a_view(&c);
                v.view_start_s = 0.0;
                v.view_len_s = len_s;
                v.song_s = len_s;
                v.grid_exp = exp;
                let (shapes, _) = frame_of(&v, area);
                let lines = shapes
                    .iter()
                    .filter(|s| match &s.shape {
                        egui::Shape::LineSegment { points, .. } => {
                            (points[0].x - points[1].x).abs() < 0.01
                                && (points[0].y - field.top()).abs() < 0.01
                                && (points[1].y - field.bottom()).abs() < 0.01
                        }
                        _ => false,
                    })
                    .count();
                assert!(
                    lines <= cap,
                    "{len_s} s wide at grid {exp}: {lines} grid lines, more than {cap}"
                );
                // A fine gridbox on a wide window draws NO lines, by design:
                // bars under five points are skipped and their subdivisions
                // with them. The bar-or-coarser settings always show the
                // strong lines the walk steps by.
                if exp >= 0 {
                    assert!(lines > 0, "{len_s} s wide at grid {exp}: no grid at all");
                }
            }
        }
    }

    // ── the trailing add's ground, run on to the foot ────────────────────

    /// **A desk that ends at its trailing add**: tracks on rows `0..used`
    /// and the add on row `used`, the way the app builds a desk with fewer
    /// drawn lanes than the window asks for. `plus` is the add's tier:
    /// `Trailing`, or `Locked` (the padlock) at the free cap.
    fn ending_at_the_add<'a>(
        clips: &'a [ClipView<'a>],
        used: usize,
        plus: crate::mixer_panel::Plus,
    ) -> ArrangeView<'a> {
        let mut v = a_view(clips);
        v.lanes = Lanes::dense(
            (0..=used)
                .map(|i| if i < used { a_lane(true) } else { LaneView { plus, ..a_lane(false) } })
                .collect(),
        );
        v.active = used + 1;
        v
    }

    /// One frame of `v` at `area`, in a context the caller keeps: the
    /// sweeps below paint hundreds of frames, and fonts are installed once.
    fn paint_in(ctx: &egui::Context, v: &ArrangeView<'_>, area: Rect) -> Vec<egui::epaint::ClippedShape> {
        ctx.run(egui::RawInput { screen_rect: Some(area), ..Default::default() }, |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                ui.set_clip_rect(area);
                draw(ui.painter(), area, v);
            });
        })
        .shapes
    }

    /// **The ground the eye sees over `region`**: the LAST filled rect
    /// painted over all of it, and its fill. `None` when a fill painted
    /// after it covers more of the region than a glyph does (the add's plus
    /// or padlock can overhang a short row), since that would be the ground
    /// there instead.
    fn ground_over(shapes: &[egui::epaint::ClippedShape], region: Rect) -> Option<(Rect, Color32)> {
        let filled = |cs: &egui::epaint::ClippedShape| match &cs.shape {
            egui::Shape::Rect(r) if r.fill.a() > 0 => Some((r.rect, r.fill)),
            _ => None,
        };
        let k = shapes.iter().rposition(|cs| filled(cs).is_some_and(|(r, _)| r.contains_rect(region)))?;
        let covered = shapes[k + 1..]
            .iter()
            .filter_map(filled)
            .map(|(r, _)| r.intersect(region))
            .any(|i| i.is_positive() && i.area() > 200.0);
        if covered {
            None
        } else {
            filled(&shapes[k])
        }
    }

    /// **Everything [`EndlessGround`] promises, read off one painted frame.**
    ///
    /// Under the add, each column's last fill is ONE rect from the add's
    /// own top to the foot of that column, as wide as the add, in what the
    /// add wears there: `ink`'s `empty_ground` in the header column, and
    /// across the field `lane`, the field's own ground, on an odd row as on
    /// an even one (`field_ground`); and it is the only rect of that colour
    /// there, so the run costs a rect a column however far it goes.
    /// Nothing is drawn ACROSS the add's foot, neither the seam a boundary
    /// between rows wears nor a box edge, and the add's slot box closes
    /// round the whole run. Returns the two runs under the add: the header
    /// column's and the field's.
    fn assert_endless(
        shapes: &[egui::epaint::ClippedShape],
        l: &Layout,
        v: &ArrangeView<'_>,
        g: EndlessGround,
        what: &str,
    ) -> (Rect, Rect) {
        let p = ink(v);
        let panel = l.header_rows[g.row].panel;
        let lane = l.lane_rows[g.row];
        let foot = lane.bottom();
        let header_run = Rect::from_min_max(
            Pos2::new(panel.left(), foot),
            Pos2::new(panel.right(), l.header.bottom()),
        );
        let lane_run = Rect::from_min_max(Pos2::new(lane.left(), foot), l.lanes_area.right_bottom());
        assert!(
            header_run.is_positive() && lane_run.is_positive(),
            "{what}: no room under the add {lane:?} to run on into"
        );
        for (column, run, own, want) in
            [("header column", header_run, panel, p.empty_ground), ("field", lane_run, lane, p.lane)]
        {
            let (rect, fill) = ground_over(shapes, run)
                .unwrap_or_else(|| panic!("{what}: nothing is painted under the add in the {column}"));
            assert_eq!(fill, want, "{what}: the {column} under the add is not the add's own ground");
            assert!(
                rect.top() <= own.top() + 0.01
                    && rect.bottom() >= run.bottom() - 0.01
                    && rect.left() <= own.left() + 0.01
                    && rect.right() >= own.right() - 0.01,
                "{what}: the {column}'s ground {rect:?} is not one surface from the add {own:?} to the foot at {:.1}",
                run.bottom()
            );
            // And it costs one rect however far it runs: never a rect per
            // row that is not there.
            let pieces = shapes
                .iter()
                .filter(|cs| match &cs.shape {
                    egui::Shape::Rect(r) => r.fill == want && r.rect.intersect(run).is_positive(),
                    _ => false,
                })
                .count();
            assert_eq!(pieces, 1, "{what}: the {column} under the add is painted in {pieces} pieces");
        }
        // Nothing across the add's foot: a line or a box edge running the
        // width there would cut the one surface in two.
        let across = |y: f32, x0: f32, x1: f32| {
            (y - foot).abs() <= 1.0 && x1.min(l.lanes_area.right()) - x0.max(panel.left()) > 40.0
        };
        for cs in shapes {
            match &cs.shape {
                egui::Shape::LineSegment { points: [a, b], stroke }
                    if stroke.width > 0.0 && stroke.color.a() > 0 =>
                {
                    assert!(
                        !((a.y - b.y).abs() < 0.5 && across(a.y, a.x.min(b.x), a.x.max(b.x))),
                        "{what}: a line runs across the add's foot at {foot:.1}: {a:?} to {b:?}"
                    );
                }
                egui::Shape::Rect(r) if r.stroke.width > 0.0 && r.stroke.color.a() > 0 => {
                    for y in [r.rect.top(), r.rect.bottom()] {
                        assert!(
                            !across(y, r.rect.left(), r.rect.right()),
                            "{what}: a box edge runs across the add's foot: {:?}",
                            r.rect
                        );
                    }
                }
                _ => {}
            }
        }
        let boxed = shapes.iter().any(|cs| {
            matches!(&cs.shape, egui::Shape::Rect(r)
                if r.stroke.width > 0.0
                    && r.fill.a() == 0
                    && (r.rect.left() - panel.left()).abs() < 0.01
                    && (r.rect.top() - lane.top()).abs() < 0.01
                    && (r.rect.right() - lane.right()).abs() < 0.01
                    && (r.rect.bottom() - l.lanes_area.bottom()).abs() < 0.01)
        });
        assert!(boxed, "{what}: the add's slot box does not close round its run");
        (header_run, lane_run)
    }

    /// An automation row to open on a lane, for the stacks below.
    fn an_open_row() -> AutoView<'static> {
        AutoView {
            selected: false,
            label: "volume".to_owned(),
            resting: 0.5,
            armed: false,
            bypassed: false,
            param: crate::automation::AutoParam::Gain,
            points: &[],
            hot: None,
            chosen: Vec::new(),
            moving: None,
        }
    }

    /// **The trailing add's ground runs on to the foot of the timeline.**
    ///
    /// The owner: "i want the unlimited track mode to have continuity of the
    /// color used for the background of the track adder button below tracks
    /// in the arrange view ... can we make the draw just infinite?" Held at
    /// five window sizes, in both tiers (the padlock at the free cap wears
    /// the same ground), on desks of none to twelve tracks at three zooms,
    /// with automation rows and a shut folder above the add moving it, at
    /// every scroll from the top to past the end: wherever the add stands
    /// with room under it, its run reaches the foot of both columns as one
    /// surface with it (see `assert_endless`). A desk that fills the window
    /// ends flush at its add (scrolled to its end) or does not show it, and
    /// has no run to paint.
    ///
    /// Revert-check: paint the add's two grounds over its own row alone
    /// (the `endless_at` reads in `draw`) and the header column under the
    /// add is the wood again.
    #[test]
    fn the_trailing_adds_ground_runs_on_to_the_foot_of_the_timeline() {
        use crate::mixer_panel::Plus;
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let none: Vec<ClipView<'_>> = Vec::new();
        let (mut ran, mut flush) = (0usize, 0usize);
        for (w, h) in [(1300.0_f32, 420.0_f32), (900.0, 673.0), (1920.0, 960.0), (640.0, 300.0), (1300.0, 150.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 7.0), Vec2::new(w, h));
            for plus in [Plus::Trailing, Plus::Locked] {
                // (tracks, lanes the window asks for, automation rows open on
                // the first lane, lanes 1..=k shut away in a folder)
                for (used, visible, opened, shut) in [
                    (2usize, 8usize, 0usize, 0usize),
                    (0, 8, 0, 0),
                    (5, 8, 0, 0),
                    (2, 16, 0, 0),
                    (2, 8, 2, 0),
                    (5, 8, 1, 0),
                    (12, 8, 0, 8),
                    (3, 4, 0, 0),
                    (12, 8, 0, 0),
                ] {
                    for scroll in [0usize, 1, 4, 5, usize::MAX] {
                        let mut v = ending_at_the_add(&none, used, plus);
                        v.visible_lanes = visible;
                        v.lane_scroll = scroll;
                        if opened > 0 {
                            v.lanes[0].automation = (0..opened).map(|_| an_open_row()).collect();
                        }
                        for k in 1..=shut {
                            v.lanes[k].hidden = true;
                        }
                        v.active = used + 1 - shut;
                        let what = format!(
                            "{w}x{h}, {plus:?}, {used} tracks ({shut} shut away) in {visible} lanes, \
                             {opened} automation rows, scroll {scroll}"
                        );
                        let l = Layout::new(r, &v);
                        let shapes = paint_in(&ctx, &v, r);
                        match endless_ground(&l, &v) {
                            Some(g) => {
                                assert_eq!(g.row, used, "{what}: the run is not the add's");
                                assert_endless(&shapes, &l, &v, g, &what);
                                ran += 1;
                            }
                            None => {
                                if let Some(add) = l.lane_rows.get(used).copied().filter(|a| a.is_positive()) {
                                    assert!(
                                        l.lanes_area.bottom() - add.bottom() <= 0.5,
                                        "{what}: the add {add:?} has room under it and no run"
                                    );
                                    flush += 1;
                                }
                            }
                        }
                    }
                }
            }
        }
        assert!(ran >= 200 && flush >= 40, "{ran} frames ran the add on, {flush} ended flush at it");
    }

    /// **On every theme, in both modes, the run wears the add's own
    /// grounds**, which are the theme's: `empty_ground` in the header column
    /// (the token every empty row's header wears) and `lane`, the field's
    /// own ground, in the field, with the add on an even row and on an odd
    /// one. Read off the painted frame against `ink`, so no colour is
    /// written down here and a theme that moves its grounds moves the run
    /// with them.
    #[test]
    fn every_theme_runs_the_add_on_in_its_own_grounds() {
        use crate::mixer_panel::Plus;
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let none: Vec<ClipView<'_>> = Vec::new();
        let r = rect();
        let mut seen = 0usize;
        for (theme, dark) in crate::theme::factory()
            .into_iter()
            .flat_map(|t| [(t.clone(), false), (t, true)])
        {
            let mut st = crate::settings::Settings::default();
            theme.apply(&mut st);
            st.dark_mode = dark;
            for used in [2usize, 3] {
                let mut v = ending_at_the_add(&none, used, Plus::Trailing);
                v.dark_mode = dark;
                let c = st.recorder_bg_color;
                v.wood = (c.r, c.g, c.b);
                let c = st.fx_strip_color;
                v.fx_strip = (c.r, c.g, c.b);
                let c = st.meter_bg_color;
                v.meter_bg = (c.r, c.g, c.b);
                let what = format!("{} {}, the add on row {used}", theme.name, if dark { "dark" } else { "light" });
                let l = Layout::new(r, &v);
                let g = endless_ground(&l, &v).unwrap_or_else(|| panic!("{what}: no run under the add"));
                assert_endless(&paint_in(&ctx, &v, r), &l, &v, g, &what);
                seen += 1;
            }
        }
        assert!(seen >= 64, "only {seen} theme frames were painted");
    }

    /// **The add wears one ground in the field however long the desk is.**
    ///
    /// The add's row is the desk's length, so removing a track, opening one
    /// between two others and adding one that grows the desk each stand it
    /// on a row of the other parity. Striped by that row, the add and the
    /// run under it (the largest surface on the timeline) swapped between
    /// the two stripes at every one of those edits, so the colour the owner
    /// asked to carry on held only until the desk changed length. Held on
    /// desks of none to nine tracks, so the add stands on even rows and odd
    /// ones, in both tiers, at three window sizes, with automation rows open
    /// and a shut folder above it, scrolled to the top and to the end: the
    /// add's own lane and the run under it, where there is one, are `lane`,
    /// the field's own ground, and one rect; a desk ending flush at its add
    /// wears it too; and every track above keeps its stripe by row, so
    /// nothing but the add changed.
    ///
    /// Revert-check: stripe the add by its row again (`field_ground`) and
    /// every desk whose add stands on an odd row goes red.
    #[test]
    fn the_trailing_adds_ground_is_one_colour_however_long_the_desk_is() {
        use crate::mixer_panel::Plus;
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let none: Vec<ClipView<'_>> = Vec::new();
        // The fill the eye sees at `at`: a speck, so nothing painted beside
        // it (a scrollbar, a neighbour) is read as its ground.
        let fill_at = |shapes: &[egui::epaint::ClippedShape], at: Pos2| {
            ground_over(shapes, Rect::from_center_size(at, Vec2::splat(4.0)))
        };
        let (mut odd_run, mut odd_flush, mut striped) = (0usize, 0usize, 0usize);
        for (w, h) in [(1300.0_f32, 420.0_f32), (900.0, 673.0), (640.0, 300.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 7.0), Vec2::new(w, h));
            for plus in [Plus::Trailing, Plus::Locked] {
                // (automation rows open on the first lane, lanes 1..=k shut
                // away in a folder), each on the desks that have the tracks
                // to carry it
                for (opened, shut, least) in [(0usize, 0usize, 0usize), (2, 0, 1), (0, 2, 3)] {
                    for used in least..=9 {
                        for scroll in [0usize, usize::MAX] {
                            let mut v = ending_at_the_add(&none, used, plus);
                            v.visible_lanes = 6;
                            v.lane_scroll = scroll;
                            if opened > 0 {
                                v.lanes[0].automation = (0..opened).map(|_| an_open_row()).collect();
                            }
                            for k in 1..=shut {
                                v.lanes[k].hidden = true;
                            }
                            v.active = used + 1 - shut;
                            let what = format!(
                                "{w}x{h}, {plus:?}, the add on row {used} ({shut} shut away, \
                                 {opened} automation rows), scroll {scroll}"
                            );
                            let l = Layout::new(r, &v);
                            let Some(add) = l.lane_rows.get(used).copied().filter(|a| a.height() > 8.0) else {
                                continue;
                            };
                            let p = ink(&v);
                            let shapes = paint_in(&ctx, &v, r);
                            let at = Pos2::new(add.left() + add.width() * 0.3, add.center().y);
                            let (own, fill) =
                                fill_at(&shapes, at).unwrap_or_else(|| panic!("{what}: nothing painted on the add"));
                            assert_eq!(fill, p.lane, "{what}: the add does not wear the field's own ground");
                            match endless_ground(&l, &v) {
                                Some(g) => {
                                    let under = Pos2::new(at.x, (add.bottom() + g.lane.bottom()) * 0.5);
                                    assert_eq!(
                                        fill_at(&shapes, under),
                                        Some((own, p.lane)),
                                        "{what}: the run under the add is not the add's own ground"
                                    );
                                    odd_run += used % 2;
                                }
                                None => odd_flush += used % 2,
                            }
                            for (i, row) in l.lane_rows.iter().filter(|(i, x)| *i < used && x.height() > 8.0) {
                                let stripe = if i % 2 == 0 { p.lane } else { p.lane_alt };
                                let at = Pos2::new(row.left() + row.width() * 0.3, row.center().y);
                                assert_eq!(
                                    fill_at(&shapes, at).map(|(_, c)| c),
                                    Some(stripe),
                                    "{what}: track row {i} lost its stripe"
                                );
                                striped += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(
            odd_run >= 40 && odd_flush >= 8 && striped >= 300,
            "{odd_run} runs and {odd_flush} flush adds on odd rows, {striped} tracks' stripes read"
        );
    }

    /// **The run under the add is paint, and answers nothing.**
    ///
    /// A press anywhere in it answers what the bare wood there did: no hit,
    /// no header panel holding it (a press on bare header ground selects the
    /// row whose panel holds the point: `app`'s `None` arm) and no lane row.
    /// No target the panel offers a keyboard or a reader reaches into it.
    /// And the layout is the one a desk whose last row is an ordinary empty
    /// row gets, which has no run: the same rows in the same places (the
    /// add's own among them, ending where it always did) and no lane
    /// scrollbar, so the run is not content and the scroll range never
    /// counts it.
    ///
    /// Revert-check: with the run unpainted, the first assertion goes red
    /// here as in every test of the run; the rest hold it to paint.
    #[test]
    fn the_run_under_the_add_is_paint_and_answers_nothing() {
        use crate::mixer_panel::Plus;
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let none: Vec<ClipView<'_>> = Vec::new();
        for (w, h) in [(1300.0_f32, 420.0_f32), (900.0, 673.0), (640.0, 300.0)] {
            let r = Rect::from_min_size(Pos2::new(3.0, 7.0), Vec2::new(w, h));
            for plus in [Plus::Trailing, Plus::Locked] {
                let what = format!("{w}x{h}, {plus:?}");
                let v = ending_at_the_add(&none, 2, plus);
                let l = Layout::new(r, &v);
                let g = endless_ground(&l, &v).unwrap_or_else(|| panic!("{what}: no run under the add"));
                let (header_run, lane_run) = assert_endless(&paint_in(&ctx, &v, r), &l, &v, g, &what);
                for run in [header_run, lane_run] {
                    for fx in [0.02_f32, 0.5, 0.98] {
                        for fy in [0.02_f32, 0.5, 0.98] {
                            let at = Pos2::new(run.left() + run.width() * fx, run.top() + run.height() * fy);
                            assert_eq!(hit_test(r, &v, at), None, "{what}: a press at {at:?} in the run hits");
                            assert_eq!(
                                l.header_rows.find_row(|h| h.panel.contains(at)),
                                None,
                                "{what}: a header panel holds {at:?} in the run"
                            );
                            assert_eq!(
                                l.lane_rows.find_row(|x| x.contains(at)),
                                None,
                                "{what}: a lane holds {at:?} in the run"
                            );
                        }
                    }
                    for (t, hit) in targets_in(&l, &v) {
                        assert!(
                            !t.intersect(run).is_positive(),
                            "{what}: {hit:?} at {t:?} reaches into the run {run:?}"
                        );
                    }
                }
                // The same desk ending in an ordinary empty row: no run, and
                // the same layout to the point.
                let twin = ending_at_the_add(&none, 2, Plus::Row);
                let lt = Layout::new(r, &twin);
                assert_eq!(endless_ground(&lt, &twin), None, "{what}: an ordinary empty row ran on");
                assert_eq!(l.rows.len(), lt.rows.len(), "{what}: the run changed the rows");
                for (a, b) in l.rows.iter().zip(&lt.rows) {
                    assert_eq!(
                        (a.kind, a.lane, a.header),
                        (b.kind, b.lane, b.header),
                        "{what}: the run moved a row"
                    );
                }
                assert_eq!((l.lane_bar, lt.lane_bar), (LaneBar::NONE, LaneBar::NONE), "{what}: a scroll range");
            }
        }
    }

    /// **`cut_to` answers exactly what the walk did**, over every name
    /// shape the panel prints (plugin names, ROUTING counts, the input
    /// words, accents, CJK, runs of spaces, a long run of one letter, the
    /// empty string), widths swept at a quarter point from nothing to the
    /// whole word, in both faces at the sizes the header uses. The walk
    /// laid out every candidate; the shipped one reads the cut off the
    /// first galley and checks it once. Same string or the test says where.
    #[test]
    fn cut_to_gives_the_walk_its_own_answer() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("cut-to"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        let names = [
            "Tangent Compressor",
            "Brusfri",
            "Valhalla Supermassive",
            "ROUTING 12",
            "NO INPUT",
            "a",
            "",
            "\u{dc}n\u{ef}c\u{f6}d\u{e9} N\u{e4}m\u{e9}",
            "\u{65e5}\u{672c}\u{8a9e}\u{306e}\u{30c6}\u{30ad}\u{30b9}\u{30c8}",
            "two  spaces  here ",
            "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx",
            "Vocal take 3 (comp) - final FINAL v2",
        ];
        let fonts = [
            FontId::new(7.0, crate::fonts::courier()),
            FontId::new(10.0, crate::fonts::courier()),
            FontId::new(11.5, crate::fonts::courier()),
            FontId::new(8.5, crate::fonts::courier_bold()),
            FontId::new(12.0, crate::fonts::courier_bold()),
        ];
        let mut checked = 0usize;
        let mut cut = 0usize;
        for font in &fonts {
            for name in names {
                let full = painter
                    .layout_no_wrap(name.to_owned(), font.clone(), Color32::WHITE)
                    .rect
                    .width();
                let mut w = -1.0f32;
                while w <= full + 2.0 {
                    let want = cut_to_reference(&painter, w, name, font);
                    let got = cut_to(&painter, w, name, font);
                    assert_eq!(got, want, "{name:?} at {w} in {font:?}");
                    checked += 1;
                    cut += usize::from(got != name);
                    w += 0.25;
                }
            }
        }
        assert!(checked > 2_000 && cut > 500, "{checked} checked, {cut} of them cut");

        // **And the count.** Every distinct string laid out lands in the
        // font's galley cache, so the cache's growth across one call is the
        // number of layouts it made. The walk paid one per character it
        // removed; the shipped one pays for the text, the ellipsis, the
        // candidate and at most a step or two of the walk.
        let layouts = |f: &dyn Fn(&Painter, f32, &str, &FontId) -> String| -> usize {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
            let _ = ctx.run(Default::default(), |_| {});
            let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
            let before = ctx.fonts(|f| f.num_galleys_in_cache());
            let font = FontId::new(10.0, crate::fonts::courier());
            let got = f(&painter, 40.0, "Tangent Compressor", &font);
            assert!(got.ends_with('\u{2026}') && got.len() < 12, "{got:?}");
            ctx.fonts(|f| f.num_galleys_in_cache()) - before
        };
        let (walk, now) = (layouts(&cut_to_reference), layouts(&cut_to));
        assert!(walk >= 10, "the walk laid out only {walk} galleys");
        assert!(now <= 5, "cut_to laid out {now} galleys against the walk's {walk}");
    }

    /// **A frame asks `quiet` a handful of times, not one per switch.**
    ///
    /// `quiet` is a contrast walk: a blend and up to thirteen `contrast_ratio`
    /// calls, three `powf` each. It was asked per bar number, per clock label,
    /// per bay chip, per switch letter and per dial label — about a hundred
    /// and eighty times a frame on eight lanes — for answers that depend on
    /// the theme alone. Those live in `Ink` now, derived once; what is left
    /// per lane is the two inks that depend on the lane's own plate. Eight
    /// used lanes with every chip and switch drawn: a fixed dozen plus two
    /// a lane, and the bound says so.
    #[test]
    fn quiet_is_asked_a_handful_of_times_per_frame() {
        let c = clips();
        let mut v = a_view(&c);
        for lane in v.lanes.iter_mut().take(8) {
            lane.used = true;
            lane.inserts = ["Comp", "EQ", "Verb", "", "", ""];
        }
        v.show_tempo_lane = true;
        let tall = Rect::from_min_size(Pos2::ZERO, Vec2::new(1300.0, 900.0));
        QUIET_CALLS.with(|n| n.set(0));
        let (shapes, _) = frame_of(&v, tall);
        let asked = QUIET_CALLS.with(std::cell::Cell::get);
        assert!(shapes.len() > 100, "the frame drew {} shapes", shapes.len());
        println!("quiet asked {asked} times for the frame");
        assert!(asked <= 12 + 2 * 8, "quiet was asked {asked} times in one frame");
        assert!(asked >= 8, "quiet was asked only {asked} times: the count is broken");
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
                    let _ = ctx.run(Default::default(), |ctx| {
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
                let _ = ctx.run(Default::default(), |ctx| {
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
            .find(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0))
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
            Some(Hit::AutoPoint(LaneId::Row(0), 0, want)),
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
            .find(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0))
            .expect("no automation row")
            .lane;
        assert!(row.is_positive());

        // Straight at the first point.
        let at = Pos2::new(x_of(rect(), &v, 2.0), auto_y_of(row, 0.75));
        assert_eq!(hit_test(rect(), &v, at), Some(Hit::AutoPoint(LaneId::Row(0), 0, 0)));
        // A little off it, still within the handle's reach.
        let near = Pos2::new(at.x + 2.0, at.y - 2.0);
        assert_eq!(hit_test(rect(), &v, near), Some(Hit::AutoPoint(LaneId::Row(0), 0, 0)));
        // Well away from both: empty room, and it says WHERE.
        let empty = Pos2::new(x_of(rect(), &v, 4.0), row.center().y);
        match hit_test(rect(), &v, empty) {
            Some(Hit::AutoLane(LaneId::Row(0), 0, t, val)) => {
                assert!((t - 4.0).abs() < 0.2, "the press landed at {t}s");
                assert!((val - 0.5).abs() < 0.1, "the middle of the row read as {val}");
            }
            other => panic!("empty room answered {other:?}"),
        }
        // The nearer of two points wins, not the first.
        let second = Pos2::new(x_of(rect(), &v, 6.0), auto_y_of(row, 0.25));
        assert_eq!(hit_test(rect(), &v, second), Some(Hit::AutoPoint(LaneId::Row(0), 0, 1)));
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
            .find(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0))
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
            matches!(hit_test(rect(), &v, inside), Some(Hit::AutoLane(LaneId::Row(0), 0, ..))),
            "the row stopped accepting presses"
        );
        // A handle overhanging the edge is still grabbable, which is the
        // whole reason the grab area is expanded.
        let handle = Pos2::new(x_of(rect(), &v, 2.0), auto_y_of(row, 0.5));
        assert_eq!(hit_test(rect(), &v, handle), Some(Hit::AutoPoint(LaneId::Row(0), 0, 0)));
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

        // The MASTER heads the stack now; the tempo shares its lane rather
        // than buying a row above it.
        let first = l.rows.first().expect("no rows");
        assert_eq!(first.kind, RowKind::Master);
        assert!(
            first.lane.top() <= l.lane_rows[0].top() + 0.5,
            "the master row is not above the tracks"
        );
        let tempo = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Tempo)
            .expect("no tempo row");
        assert_eq!(tempo.lane, first.lane, "the tempo left the master's lane");
        // The automation row sits directly under the track it belongs to.
        let auto = l
            .rows
            .iter()
            .find(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0))
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
                .any(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0) && r.lane.is_positive()),
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
            .find(|r| r.kind == RowKind::Automation(LaneId::Row(0), 0))
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
        for (i, row) in l.header_rows.iter() {
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
                // **Ø joined the sweep when it MOVED.** It used to sit above
                // the mute, in the column this list already covered; it is in
                // the row's bottom-right corner now, which is a corner two
                // other things reach toward.
                row.phase,
                row.theory,
                row.automation,
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
        // Going finer: BAR, 1/2, 1/4, 1/8, 1/16, with one 1/4 state, sitting
        // exactly between 1/8 and 1/2, and now written the way the rungs
        // either side of it are written. The middle rung used to assert
        // `\u{266a}`; the owner asked for the fraction back because "it's
        // easier to spot at a glance for a beginner and realize that's what
        // that button does", and the single-1/4-state property that the
        // glyph used to stand in for is asserted on its own terms by
        // `only_one_rung_of_the_arrange_ladder_is_a_quarter`, so nothing was
        // weakened by respelling it here.
        //
        // The top of it asserted "1" until the bar rung was named: "1" is a
        // whole note, which in 4/4 is the bar's length, and in 4/2 it is the
        // rung UNDER the bar. The owner ruled the bar reads BAR in every
        // meter, this one included (see `GRID_BAR`).
        assert_eq!(grid_label_text(0, 4, 4), "BAR");
        assert_eq!(grid_label_text(3, 4, 4), "8");
        assert_eq!(grid_label_text(-1, 4, 4), "1/2");
        assert_eq!(grid_label_text(-2, 4, 4), "1/4");
        assert_eq!(grid_label_text(-3, 4, 4), "1/8");
        assert_eq!(grid_label_text(-4, 4, 4), "1/16");
        // And nowhere on the arrange ladder, at either end of the range the
        // wheel, the arrows and the saved setting can between them reach.
        // The rung at -2 is the one anybody would edit; a sweep is what
        // catches a "fix" applied to the wrong branch of `note_label`.
        for e in -7i8..=6 {
            for (beats, unit) in [(4u8, 4u8), (6, 8), (3, 4), (7, 8)] {
                let w = grid_label_text(e, beats, unit);
                assert!(
                    !w.contains('\u{266a}'),
                    "exp {e} in {beats}/{unit} still prints a glyph: {w}"
                );
            }
        }
    }

    /// **What the ARRANGE gridbox actually PAINTS**, harvested off the frame
    /// rather than asked of the helper that feeds it.
    ///
    /// Every assertion above would stay green if somebody pointed the draw
    /// call at `grid_label` again: the text ladder would go on spelling
    /// "1/4" for anyone who called it, and the button on screen would go on
    /// showing a note glyph. That is the shape of mistake this panel has
    /// been caught by twice — a test that measures a function the painter
    /// happens to call is not a test of the picture. So this one runs the
    /// real `draw` into a real context, keeps the `Shape::Text` that lands
    /// inside `tool_grid`, and reads the box the way the owner does.
    ///
    /// The neighbouring rungs are checked too, so a failure here says which
    /// thing broke: all three wrong means the box stopped drawing or the
    /// harvest is aimed at the wrong rectangle; only the middle one wrong
    /// means the quarter's spelling is what moved.
    #[test]
    fn the_arrange_gridbox_paints_its_quarter_as_a_fraction() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let r = rect();
        let printed = |exp: i8| -> Vec<String> {
            let cs: Vec<ClipView<'_>> = Vec::new();
            let mut v = a_view(&cs);
            v.grid_exp = exp;
            let cell = Layout::new(r, &v).tool_grid;
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
                .filter_map(|s| match &s.shape {
                    egui::Shape::Text(t) if cell.intersects(t.visual_bounding_rect()) => {
                        Some(t.galley.text().to_owned())
                    }
                    _ => None,
                })
                .collect()
        };
        // exp -2 in 4/4 is the quarter note, and the quarter note is the one
        // rung this item is about.
        let quarter = printed(-2);
        assert!(
            quarter.iter().any(|t| t == "1/4"),
            "the gridbox did not print 1/4: {quarter:?}"
        );
        assert!(
            !quarter.iter().any(|t| t.contains('\u{266a}')),
            "the gridbox is still painting the glyph: {quarter:?}"
        );
        assert!(
            printed(-1).iter().any(|t| t == "1/2"),
            "the rung above the quarter stopped printing"
        );
        assert!(
            printed(-3).iter().any(|t| t == "1/8"),
            "the rung below the quarter stopped printing"
        );
    }

    /// **Exactly one rung of the arrange ladder is a quarter**, and no two
    /// rungs anywhere on it print the same thing.
    ///
    /// This is the property the glyph was believed to be holding up. The old
    /// note on `grid_label` said the glyph was "what makes it the single 1/4
    /// state", which read a fixed bug as a live one: "1/4" appeared twice on
    /// the way down because the label was a second opinion about the
    /// division, and it stopped appearing twice when the label began reading
    /// `grid_quarters` back out. The character was never what enforced it.
    ///
    /// The argument was prose, though, and prose does not fail a build — so
    /// respelling the rung as text is only safe if something else holds the
    /// line. This does. Give the label a formula of its own again, or let
    /// `note_label`'s `quarter_glyph` flag reach the DOTTED branch, and two
    /// rungs start answering to the same string in some meter here.
    #[test]
    fn only_one_rung_of_the_arrange_ladder_is_a_quarter() {
        // Every meter the app's clamps and the ladder's own doc mention,
        // plus the odd ones where a bar is not a power of two of its beat —
        // those are where a second label formula would first disagree.
        for (beats, unit) in [(4u8, 4u8), (2, 2), (3, 4), (5, 4), (6, 8), (7, 8), (9, 8), (12, 8)] {
            let mut seen: Vec<(i8, String)> = Vec::new();
            for e in -7i8..=6 {
                let w = grid_label_text(e, beats, unit);
                if let Some((prev, _)) = seen.iter().find(|(_, s)| *s == w) {
                    panic!("{beats}/{unit}: exp {prev} and exp {e} both print {w:?}");
                }
                seen.push((e, w));
            }
        }
        // In 4/4 there is one 1/4 state and it is the one the magnet snaps
        // to a whole quarter note on — the box and the maths naming the same
        // division, which is the entire reason the label is derived rather
        // than computed twice.
        let quarters: Vec<i8> =
            (-7i8..=6).filter(|e| grid_label_text(*e, 4, 4) == "1/4").collect();
        assert_eq!(quarters, vec![-2], "the 4/4 ladder's 1/4 states: {quarters:?}");
        assert!((grid_quarters(-2, 4, 4) - 1.0).abs() < 1e-9);
        // And 6/8 has no plain quarter rung at all: a quarter note is a
        // THIRD of that bar, so the ladder steps dotted quarter, eighth. The
        // dot is what keeps that rung distinct from the one above, and it is
        // why the glyph flag must never reach the dotted branch.
        assert!(!(-7i8..=6).any(|e| grid_label_text(e, 6, 8) == "1/4"));
        assert_eq!(grid_label_text(-1, 6, 8), "1/4\u{00b7}");
        assert_eq!(grid_label(-1, 6, 8), "1/4\u{00b7}");
    }

    /// **No rung of any signature is printed as a decimal**, no ladder says
    /// one of its rungs twice, and the bar is BAR in every one of them.
    ///
    /// Every signature `TimeSignature::is_valid` lets in, all 192 of them,
    /// over the whole exponent range `grid_quarters` takes, in both
    /// spellings. 247 of those (signature, rung) pairs used to fall through
    /// `note_label` into `"{q:.3}q"`: the BAR rung of every meter whose bar
    /// is not one note or one dotted note (5/4, 7/8, 9/8, 4/2 ...), and the
    /// halves of a big bar that land on a length no note names (10/8's five
    /// eighths). The sweep is what catches a fix made for 4/2 alone.
    ///
    /// The bar rung then read as its signature, and the owner, in 4/2, asked
    /// "why does the option between 1 and 2 = 4/2 in grid frequency and why
    /// is it even an option?" In 4/2 the box steps from 2 bars to one bar to
    /// a whole note, so the rung "between 1 and 2" is the bar. He ruled it
    /// reads BAR in every meter, 4/4 included, where it read "1" (see
    /// `GRID_BAR`). This holds all 192 to that, holds every other rung to
    /// NOT reading BAR, so the word means one bar and nothing else, and
    /// holds the reader to saying "1 bar" there.
    ///
    /// Revert check: hand the bar rung back to `note_label` and this fails
    /// at the first signature it sweeps, 1/1, whose bar prints "1".
    #[test]
    fn no_rung_of_any_signature_is_spelled_as_a_decimal() {
        let spellings: [fn(i8, u8, u8) -> String; 2] = [grid_label_text, grid_label];
        let mut swept = 0;
        for beats in 0u8..=64 {
            for unit in 0u8..=64 {
                let sig = crate::recorder::TimeSignature { beats, unit };
                if !sig.is_valid() {
                    continue;
                }
                swept += 1;
                for spell in spellings {
                    let mut seen: Vec<(i8, String)> = Vec::new();
                    for e in -7i8..=6 {
                        let w = spell(e, beats, unit);
                        assert!(
                            !w.contains('.') && !w.contains('q'),
                            "{beats}/{unit} at exp {e} prints {w:?}"
                        );
                        if e == 0 {
                            assert_eq!(w, GRID_BAR, "{beats}/{unit}: the bar rung prints {w:?}");
                        } else {
                            assert_ne!(w, GRID_BAR, "{beats}/{unit}: exp {e} is not one bar");
                        }
                        if let Some((prev, _)) = seen.iter().find(|(_, s)| *s == w) {
                            panic!("{beats}/{unit}: exp {prev} and exp {e} both print {w:?}");
                        }
                        seen.push((e, w));
                    }
                }
                assert_eq!(grid_spoken(0, beats, unit), "1 bar", "{beats}/{unit}, said aloud");
            }
        }
        assert_eq!(swept, 192, "the sweep no longer covers every valid signature");
        // The owner's meter, coarse to fine over the arrange box's range: the
        // rung he asked about is BAR, and the 1 under it is the whole note it
        // always was.
        let ladder: Vec<String> = (-6i8..=3).rev().map(|e| grid_label_text(e, 4, 2)).collect();
        assert_eq!(ladder, ["8", "4", "2", "BAR", "1", "1/2", "1/4", "1/8", "1/16", "1/32"]);
        // And 4/4's, whose bar read "1", the whole note's name.
        let ladder: Vec<String> = (-6i8..=3).rev().map(|e| grid_label_text(e, 4, 4)).collect();
        assert_eq!(ladder, ["8", "4", "2", "BAR", "1/2", "1/4", "1/8", "1/16", "1/32", "1/64"]);
        // A rung UNDER the bar that no note names is still counted in the
        // meter's beats, and a reader hears it as notes, not as a fraction of
        // the bar: in 10/8 five eighths are HALF the bar, so "5/8 of a bar"
        // would be a wrong number read aloud.
        assert_eq!(grid_label_text(-1, 10, 8), "5/8");
        assert_eq!(grid_spoken(-1, 10, 8), "5 eighth notes");
        assert_eq!(grid_spoken(0, 4, 2), "1 bar");
    }

    /// **The bar rung is painted BAR in every meter: as narrow as the
    /// three-character rungs, and inside its box, in every bundled face.**
    ///
    /// The owner met this rung twice. First as "8.000q" in 4/2: "there's a
    /// '0 frequency on the grid frequency in arrange mode reads as quite a
    /// lot of 0.000000s", six characters in a box drawn for three, so the 8
    /// spilled onto the tempo-lane switch and the navigator (painted after
    /// it) covered the q. Then, once that was counted as the signature, as
    /// "4/2": "why does the option between 1 and 2 = 4/2 in grid frequency
    /// and why is it even an option?" It reads BAR now in every meter (see
    /// `GRID_BAR`), and this reads it off the real frame, for the reason the
    /// quarter's test above gives: the picture is what was wrong.
    ///
    /// The width is held to two things on the same frame: the 1/2 rung, the
    /// three-character label the box was drawn for, and the box itself,
    /// which the label has to be laid out inside. It used to be held to the
    /// 1/16 rung, the widest the 4/4 ladder prints, but at the box's eleven
    /// points that one is wider than the box (25.6 points of its 20 in
    /// Courier Prime and JetBrains Mono, 21.5 in Terminess), so the bound let
    /// a label spill and pass. (It is drawn smaller now, to fit: see
    /// `every_arrange_grid_step_is_painted_inside_its_box`.) In
    /// every bundled face, since the box draws in whichever one the settings
    /// chose and they are not one width: three characters at the box's
    /// eleven points are 19.6 in Courier Prime and JetBrains Mono and 16.5
    /// in Terminess. What is held is the laid-out width, not the glyphs'
    /// quads, which round out to whole pixels and overhang their advance by
    /// up to a pixel on every rung, 1/4 included.
    ///
    /// Revert check: hand the bar rung back to `note_label` and this fails
    /// on 4/4 painting "1".
    #[test]
    fn the_bar_rung_is_painted_bar_in_every_meter() {
        let r = rect();
        for face in crate::fonts::FontChoice::ALL {
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx, face, None);
            crate::fonts::apply_text_styles(&ctx);
            // What the box paints at `exp` in `beats/unit`: the box, and each
            // text that lands on it, with where it was laid out.
            let printed = |exp: i8, beats: u8, unit: u8| -> (Rect, Vec<(String, Rect)>) {
                let cs: Vec<ClipView<'_>> = Vec::new();
                let mut v = a_view(&cs);
                v.grid_exp = exp;
                let sig = crate::recorder::TimeSignature { beats, unit };
                v.meter = crate::tempo::MeterMap::constant(sig);
                let cell = Layout::new(r, &v).tool_grid;
                let out = ctx.run(
                    egui::RawInput { screen_rect: Some(r), ..Default::default() },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_clip_rect(r);
                            draw(ui.painter(), r, &v);
                        });
                    },
                );
                let texts = out
                    .shapes
                    .iter()
                    .filter_map(|s| match &s.shape {
                        egui::Shape::Text(t) if cell.intersects(t.visual_bounding_rect()) => Some((
                            t.galley.text().to_owned(),
                            Rect::from_min_size(t.pos, t.galley.size()),
                        )),
                        _ => None,
                    })
                    .collect();
                (cell, texts)
            };
            // The three-character rung the box was drawn for, painted in it.
            let (_, half) = printed(-1, 4, 4);
            let three = half
                .iter()
                .find(|(t, _)| t == "1/2")
                .map(|(_, laid)| laid.width())
                .unwrap_or_else(|| panic!("{face:?}: the 4/4 box stopped printing 1/2: {half:?}"));
            // The stock meters, whose bar read as a note value (4/4 "1", 3/4
            // and 6/8 "1/2·", 2/4 "1/2", 12/8 "1·"), and odd ones, whose bar
            // read as their own signature.
            let meters = [
                (4u8, 4u8),
                (3, 4),
                (6, 8),
                (2, 4),
                (12, 8),
                (4, 2),
                (5, 4),
                (7, 8),
                (9, 8),
                (10, 8),
            ];
            for (beats, unit) in meters {
                let (cell, got) = printed(0, beats, unit);
                let names: Vec<&str> = got.iter().map(|(t, _)| t.as_str()).collect();
                assert_eq!(names, [GRID_BAR], "{face:?} {beats}/{unit}: the bar painted {names:?}");
                let laid = got[0].1;
                assert!(
                    laid.width() <= three + 0.01,
                    "{face:?} {beats}/{unit}: BAR is {}pt wide, the 1/2 rung {three}pt",
                    laid.width()
                );
                assert!(
                    laid.left() >= cell.left() - 0.01 && laid.right() <= cell.right() + 0.01,
                    "{face:?} {beats}/{unit}: BAR is laid out at {laid:?}, outside its box {cell:?}"
                );
            }
        }
    }

    /// **Every step of every arrange ladder is painted inside its box: as it
    /// always was when it fits, shrunk to the box when it does not, never
    /// under the floor, and clipped to the box whatever happens.**
    ///
    /// The box is drawn for three characters and the ladder prints up to
    /// six. At the box's eleven points 1/16 is 25.6 points wide in Courier
    /// Prime and JetBrains Mono and 21.5 in Terminess, in a box of 20, and
    /// every step of four characters or more spilled out of it; from five
    /// on (1/128 is 32.6 points) it reached the tempo lane's switch on the
    /// left and ran under the navigator on the right, the overflow behind
    /// the owner's "0.000000".
    ///
    /// Swept over every word the box can print: all 192 signatures, every
    /// step the box reaches (-6 to 3, the settings' clamp), in both
    /// spellings. Each is painted by `grid_box_word`, the call `draw` makes
    /// (`the_arrange_gridbox_paints_its_step_through_the_fit` holds `draw` to
    /// it), into the real box, in every bundled face and at 100% to 200%
    /// display scale, and read back as the tessellator draws it. Held:
    ///
    /// - a word that is laid out inside the box at eleven points is painted
    ///   at exactly eleven: BAR and 1/2 are, by name, in every face;
    /// - a word that is not is painted smaller, never under `WORD_FLOOR`,
    ///   and laid out no wider than the box less `WORD_AIR` either side;
    /// - every pixel of a word that fits is inside the box, so the clip cuts
    ///   nothing from it (the pixel snap moves it in when it would);
    /// - every word is clipped to the box, so nothing is ever painted on a
    ///   neighbour.
    ///
    /// Two words do not fit above the floor, and only in two faces: 1/1024
    /// and 1/2048 in Courier Prime and JetBrains Mono need 5.5 points or
    /// less (at 6.5 they are 22.9 to 23.4 points wide). They are pinned here
    /// by name, so a third cannot join them unnoticed, and so are the 19
    /// (signature, step) pairs that print them: the signatures 1/16 and 2/32
    /// at the box's finest step, 1/32 at its two finest, and 3/32, 5/32 and
    /// every odd count of 32nds up to 31/32 at its finest. They are painted
    /// at the floor and cut at the box's edges.
    ///
    /// Revert checks, all performed: paint at the box's own size again and
    /// this fails on the first word it sweeps, 1/64 (the finest step of
    /// 1/1), left at eleven points where it does not fit; take the clip away
    /// and it fails on that same word, whose clip is then the whole screen;
    /// take the pixel snap away and it fails on 1/128 in JetBrains Mono at
    /// 100%, a column of whose ink lands past the box's right edge (BAR in
    /// Courier Prime at 150% does the same).
    #[test]
    fn every_arrange_grid_step_is_painted_inside_its_box() {
        let sigs = valid_signatures();
        assert_eq!(sigs.len(), 192, "the sweep no longer covers every valid signature");
        let mut words: Vec<String> = Vec::new();
        let mut cut_pairs: std::collections::BTreeSet<(u8, u8, i8)> = Default::default();
        for &(beats, unit) in &sigs {
            for e in -6i8..=3 {
                for spell in [grid_label_text as fn(i8, u8, u8) -> String, grid_label] {
                    let w = spell(e, beats, unit);
                    if w == "1/1024" || w == "1/2048" {
                        cut_pairs.insert((beats, unit, e));
                    }
                    if !words.contains(&w) {
                        words.push(w);
                    }
                }
            }
        }
        let cs: Vec<ClipView<'_>> = Vec::new();
        let cell = Layout::new(rect(), &a_view(&cs)).tool_grid;
        assert!(
            (cell.width() - 20.0).abs() < 1.0e-3,
            "the box is not at its full twenty points here: {cell:?}"
        );
        // The size the twenty-point box prints at, and has always printed at.
        let ceiling = 11.0;
        assert_eq!(grid_box_type(cell), ceiling);
        for ppp in [1.0f32, 1.25, 1.5, 1.75, 2.0] {
            for face in crate::fonts::FontChoice::ALL {
                let ctx = context_at(face, ppp);
                let measure = Painter::new(ctx.clone(), egui::LayerId::background(), Rect::EVERYTHING);
                let mut cut: Vec<&str> = Vec::new();
                for (k, w) in words.iter().enumerate() {
                    let at = format!("{face:?} at {ppp}x, {w:?}");
                    let painted = PaintedWord::painted_by(&ctx, ("arrange", k), |p| {
                        grid_box_word(p, cell, w, Color32::WHITE);
                    });
                    let [word] = painted.as_slice() else {
                        panic!("{at}: painted {painted:?}, not one word");
                    };
                    assert_eq!(&word.text, w, "{at}");
                    assert!(word.size >= WORD_FLOOR, "{at}: painted at {}, under the floor", word.size);
                    assert!(
                        cell.contains_rect(word.clip),
                        "{at}: clipped to {:?}, not to its box {cell:?}",
                        word.clip
                    );
                    let wide = measure
                        .layout_no_wrap(w.clone(), FontId::new(ceiling, crate::fonts::courier_bold()), Color32::WHITE)
                        .rect
                        .width();
                    if wide <= cell.width() {
                        assert_eq!(
                            word.size, ceiling,
                            "{at}: fits the box at {ceiling} points ({wide} wide) and was painted at {}",
                            word.size
                        );
                    } else if word.laid.width() > cell.width() - 2.0 * WORD_AIR + 1.0e-3 {
                        // Too wide even at the floor: the clip holds it.
                        assert_eq!(word.size, WORD_FLOOR, "{at}: does not fit, and is not at the floor");
                        cut.push(w.as_str());
                        continue;
                    } else {
                        // Shrunk, with the air either side. Held on its
                        // width: where it lands is the ink's business, below,
                        // since the pixel snap may move it by a pixel.
                        assert!(word.size < ceiling, "{at}: {wide} wide at {ceiling} and not shrunk");
                        assert!(
                            word.laid.width() <= cell.width() - 2.0 * WORD_AIR + 1.0e-3,
                            "{at}: shrunk to {} and {} wide, with less than {WORD_AIR} either side in {}",
                            word.size,
                            word.laid.width(),
                            cell.width()
                        );
                    }
                    assert!(
                        word.ink.left() >= cell.left() - 1.0e-3
                            && word.ink.right() <= cell.right() + 1.0e-3
                            && word.ink.top() >= cell.top() - 1.0e-3
                            && word.ink.bottom() <= cell.bottom() + 1.0e-3,
                        "{at}: at {} points its ink is at {:?}, outside its box {cell:?}",
                        word.size,
                        word.ink
                    );
                    if w == GRID_BAR || w == "1/2" {
                        assert_eq!(word.size, ceiling, "{at}: the box was drawn for this one");
                    }
                }
                let want: &[&str] = match face {
                    crate::fonts::FontChoice::Terminess => &[],
                    _ => &["1/1024", "1/2048"],
                };
                cut.sort_unstable();
                assert_eq!(cut, want, "{face:?} at {ppp}x: the words that do not fit above the floor");
            }
        }
        assert_eq!(cut_pairs.len(), 19, "the steps that print 1/1024 and 1/2048: {cut_pairs:?}");
    }

    /// **The arrange gridbox paints its step through the fit**, read off the
    /// real frame: the same word, the same size and the same place that
    /// `grid_box_word` gives it, clipped to the box. The sweep above holds
    /// `grid_box_word` to every word; this holds `draw` to `grid_box_word`,
    /// at steps three to six characters wide, in every bundled face.
    ///
    /// Revert check, performed: hand the box back to `painter.text` at its
    /// own size and this fails on its first step, BAR, which is no longer
    /// clipped to its box.
    #[test]
    fn the_arrange_gridbox_paints_its_step_through_the_fit() {
        let r = rect();
        let steps = [
            (4u8, 4u8, 0i8, GRID_BAR),
            (4, 4, -1, "1/2"),
            (4, 4, -4, "1/16"),
            (6, 8, -1, "1/4\u{00b7}"),
            (3, 4, -6, "1/128"),
            (22, 16, -1, "11/16"),
            (1, 16, -6, "1/1024"),
        ];
        for face in crate::fonts::FontChoice::ALL {
            let ctx = context_at(face, 1.0);
            // The word alone is painted on a context of its own: a shape
            // painted between frames is handed out with the next frame's,
            // and the next frame is the real one.
            let apart = context_at(face, 1.0);
            for (k, (beats, unit, exp, want)) in steps.into_iter().enumerate() {
                let cs: Vec<ClipView<'_>> = Vec::new();
                let mut v = a_view(&cs);
                v.grid_exp = exp;
                v.meter = crate::tempo::MeterMap::constant(crate::recorder::TimeSignature { beats, unit });
                let cell = Layout::new(r, &v).tool_grid;
                let out = ctx.run(
                    egui::RawInput { screen_rect: Some(r), ..Default::default() },
                    |ctx| {
                        egui::CentralPanel::default().show(ctx, |ui| {
                            ui.set_clip_rect(r);
                            draw(ui.painter(), r, &v);
                        });
                    },
                );
                let on_box: Vec<PaintedWord> = PaintedWord::all(&ctx, &out.shapes)
                    .into_iter()
                    .filter(|w| cell.intersects(w.laid))
                    .collect();
                let at = format!("{face:?} {beats}/{unit} step {exp}");
                let [drawn] = on_box.as_slice() else {
                    panic!("{at}: the box painted {on_box:?}");
                };
                let alone = PaintedWord::painted_by(&apart, ("arrange draw", k), |p| {
                    grid_box_word(p, cell, want, Color32::WHITE);
                });
                let [alone] = alone.as_slice() else {
                    panic!("{at}: grid_box_word painted {alone:?}");
                };
                assert_eq!(drawn.text, want, "{at}");
                assert_eq!(drawn.size, alone.size, "{at}: {want} is not painted at its fitted size");
                assert_eq!(drawn.laid, alone.laid, "{at}: {want} is not where the fit puts it");
                assert_eq!(drawn.clip, cell, "{at}: {want} is not clipped to its box");
            }
        }
    }

    /// **Every step of every meter is said as a note**, the way a player
    /// would say it, and no ladder says one twice.
    ///
    /// The box prints shorthand because it is the width of a thumb; the
    /// screen reader is the one place the step is said in full, and it used
    /// to fall through to "{label} of a bar" for 428 (signature, step) pairs:
    /// in 4/2 the whole note under the bar was "1 of a bar" right after the
    /// bar's own "1 bar", 12/8's dotted half was "1/2· of a bar", and the
    /// finest steps were "1/128 of a bar" and finer, none of them a fraction
    /// of a bar at all. Every valid signature, every step `grid_quarters`
    /// takes (-7 to 6). Held:
    ///
    /// - nothing says "of a bar", a dot or a slash: no fraction is read out;
    /// - above the bar it counts bars ("2 bars"), the bar is "1 bar", and
    ///   every step under it is a note: "eighth note", "dotted quarter
    ///   note", "5 eighth notes";
    /// - it names what the box prints, so the two cannot disagree: a box's
    ///   1/d is the 1/d note, its dot is "dotted", its n/u is n notes of
    ///   1/u;
    /// - no ladder says one of its steps twice.
    ///
    /// Revert check, performed: put back the reader that matched the box's
    /// text against a table, and this fails on the first signature it
    /// sweeps, 1/1, whose finest step it said as "1/128 of a bar".
    #[test]
    fn every_step_of_every_meter_is_said_as_a_note() {
        let sigs = valid_signatures();
        assert_eq!(sigs.len(), 192, "the sweep no longer covers every valid signature");
        let fractions = ['/', '\u{00b7}', '\u{2044}', '\u{2215}', '\u{00bc}', '\u{00bd}', '\u{00be}'];
        for &(beats, unit) in &sigs {
            let mut said: Vec<(i8, String)> = Vec::new();
            for e in -7i8..=6 {
                let s = grid_spoken(e, beats, unit);
                let at = format!("{beats}/{unit} at step {e}");
                assert!(!s.contains("of a bar"), "{at} is said {s:?}");
                assert!(
                    !s.chars().any(|c| fractions.contains(&c) || ('\u{2150}'..='\u{215f}').contains(&c)),
                    "{at} is said with a fraction: {s:?}"
                );
                if e > 0 {
                    assert_eq!(s, format!("{} bars", 1u32 << e), "{at}");
                } else if e == 0 {
                    assert_eq!(s, "1 bar", "{at}");
                } else {
                    // What the box prints, said: 1 and 1/d are one note,
                    // a dot makes it dotted, and n/u counts n notes of 1/u.
                    let label = grid_label_text(e, beats, unit);
                    let (body, dotted) = match label.strip_suffix('\u{00b7}') {
                        Some(body) => (body, true),
                        None => (label.as_str(), false),
                    };
                    let (n, d) = match body.split_once('/') {
                        Some((n, d)) => (
                            n.parse::<u64>().expect("a count"),
                            d.parse::<u64>().expect("a note value"),
                        ),
                        None => (1, body.parse::<u64>().expect("a whole note")),
                    };
                    let word = note_word(d).unwrap_or_else(|| panic!("{at}: no name for a 1/{d} note"));
                    let want = match (n, dotted) {
                        (1, false) => format!("{word} note"),
                        (1, true) => format!("dotted {word} note"),
                        (n, false) => format!("{n} {word} notes"),
                        (_, true) => panic!("{at}: the box prints a dotted count, {label:?}"),
                    };
                    assert_eq!(s, want, "{at}, which the box prints {label:?}");
                }
                if let Some((prev, _)) = said.iter().find(|(_, w)| *w == s) {
                    panic!("{beats}/{unit}: steps {prev} and {e} are both said {s:?}");
                }
                said.push((e, s));
            }
        }
        // Where the owner and the sweep met them.
        let ladder: Vec<String> = (-7i8..=3).rev().map(|e| grid_spoken(e, 4, 2)).collect();
        assert_eq!(
            ladder,
            [
                "8 bars",
                "4 bars",
                "2 bars",
                "1 bar",
                "whole note",
                "half note",
                "quarter note",
                "eighth note",
                "sixteenth note",
                "thirty-second note",
                "sixty-fourth note",
            ]
        );
        assert_eq!(grid_spoken(-1, 12, 8), "dotted half note");
        assert_eq!(grid_spoken(-1, 6, 8), "dotted quarter note");
        assert_eq!(grid_spoken(-7, 4, 4), "hundred twenty-eighth note");
        assert_eq!(grid_spoken(-7, 3, 4), "two hundred fifty-sixth note");
        assert_eq!(grid_spoken(-7, 1, 32), "four thousand ninety-sixth note");
        assert_eq!(grid_spoken(-1, 12, 4), "dotted whole note");
        assert_eq!(grid_spoken(-1, 6, 32), "dotted sixteenth note");
        assert_eq!(grid_spoken(-1, 10, 8), "5 eighth notes");
        assert_eq!(grid_spoken(-1, 4, 1), "2 whole notes");
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
            (((clip.len_s + clip.offset_s.max(0.0)) / clip.loop_s).ceil() as usize)
                .clamp(1, 512)
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
        let inner = miniature_band(r, clip);
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
            muted: false,
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
        muted: false,
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
            peaks: None,
            notes,
            loop_s: bar_s,
            offset_s: 0.0,
            selected: false,
            midi: true,
            recording: false,
            gain: 1.0,
            marks: &[],
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

    /// **Every mark a miniature painted, whichever shape carried it.**
    ///
    /// A mark used to be one `Shape::Rect` and is two triangles inside the
    /// clip's own `Shape::Mesh` now — see `draw_note_miniature`. The
    /// rectangle did not move, so the readers below recover it rather than
    /// the drawing being bent to suit them: `Mesh::add_colored_rect` writes
    /// four vertices per mark, top-left, top-right, bottom-left,
    /// bottom-right, so a chunk of four IS one mark and the first and last
    /// of them are its corners. The reference walk still paints plain rects
    /// — which is the point of these tests, and the reason this reads BOTH.
    pub(super) fn marks_of(shape: &egui::Shape) -> Vec<Rect> {
        match shape {
            egui::Shape::Rect(q) => vec![q.rect],
            egui::Shape::Mesh(m) => m
                .vertices
                .chunks_exact(4)
                .map(|v| Rect::from_min_max(v[0].pos, v[3].pos))
                .collect(),
            _ => Vec::new(),
        }
    }

    /// **What a miniature LIGHTS**, at [`SUB`] cells to the point: every
    /// sample any of its marks covers. This is the picture as an eye gets
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
                        .flat_map(|c| marks_of(&c.shape))
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
                    .map(|list| list.all_entries().flat_map(|c| marks_of(&c.shape)).collect())
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

    /// **A recorded part — one pass, thousands of notes — draws the plain
    /// walk's picture from the notes under the window alone.**
    ///
    /// The clip that does not loop is the one `visible_passes` cannot help
    /// and the one with the most notes, and it was walked whole twice a
    /// frame. The windowed walk depends on the notes being in time order,
    /// which the commit points keep but nothing guarantees, so the same
    /// part is drawn sorted, reversed and shuffled: the sorted one takes
    /// the window and the other two the whole walk, and all three must
    /// light the plain walk's cells at every zoom and scroll, both trims.
    #[test]
    fn a_recorded_part_draws_the_same_picture_as_the_plain_walk() {
        let bar_s = 2.0;
        let one = a_looped_bar(bar_s);
        // A hundred bars: the views reach sixty, and the plain walk this is
        // held against draws every note of every bar in debug.
        let sorted: Vec<crate::arrangement::ArrNote> = (0..100)
            .flat_map(|b| {
                one.iter().map(move |n| crate::arrangement::ArrNote {
                    at_s: n.at_s + b as f64 * bar_s,
                    ..*n
                })
            })
            .collect();
        assert!(sorted.windows(2).all(|w| w[0].at_s <= w[1].at_s));
        let mut reversed = sorted.clone();
        reversed.reverse();
        let mut shuffled = sorted.clone();
        let mut seed = 11u64;
        for i in (1..shuffled.len()).rev() {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            shuffled.swap(i, (seed >> 33) as usize % (i + 1));
        }
        // Both trims for the windowed road; the whole-walk roads at one.
        for (what, notes, trims) in [
            ("sorted", &sorted, &[0.0, bar_s * 0.375][..]),
            ("reversed", &reversed, &[bar_s * 0.375][..]),
            ("shuffled", &shuffled, &[0.0][..]),
        ] {
            for &offset_s in trims {
                let clips = [ClipView {
                    loop_s: 0.0,
                    offset_s,
                    notes,
                    ..a_looped_clip(notes, bar_s, 100.0)
                }];
                assert!(clips[0].len_s > 120.0, "{what}: the part must outlast the views");
                the_two_miniatures_agree(&clips, offset_s);
            }
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

    /// Every DIAGONAL line segment the whole panel painted inside `r`, as
    /// `(top point, bottom point, ink)`.
    ///
    /// Diagonal is the filter that isolates a loop seam from everything else
    /// drawn over a clip: the grid, the playhead and the lane seams are
    /// vertical, the empty item's staff is horizontal, and the miniature and
    /// the wave are rects. The fades are diagonal, which is why every fixture
    /// here has none — and why this reads the whole `draw` rather than calling
    /// the seam pass on its own. A seam computed and not painted is the fault
    /// this exists to catch.
    /// The convex pieces the clip's own fill was painted as, in paint order.
    ///
    /// `Shape::convex_polygon` lands as a `Shape::Path`, so a clip drawn as a
    /// plain rounded rectangle contributes nothing here and a clip with
    /// divots contributes one piece a pass. That asymmetry is the point: it
    /// is what lets a test say "this clip is not a rectangle any more".
    fn clip_pieces_in(v: &ArrangeView<'_>, r: Rect, fill: Color32) -> Vec<Vec<Pos2>> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let area = rect();
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(area), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(area);
                    draw(ui.painter(), area, v);
                });
            },
        );
        let mut got = Vec::new();
        for s in &out.shapes {
            let egui::Shape::Path(path) = &s.shape else {
                continue;
            };
            if path.fill != fill || !path.closed {
                continue;
            }
            if path.points.iter().any(|p| !r.expand(1.0).contains(*p)) {
                continue;
            }
            got.push(path.points.clone());
        }
        got
    }

    /// The waveform's own columns inside `r`, as their top and bottom.
    fn wave_columns_in(v: &ArrangeView<'_>, r: Rect) -> Vec<(f32, f32)> {
        wave_columns_xy_in(v, r).into_iter().map(|(_, top, bot)| (top, bot)).collect()
    }

    /// The waveform's columns inside `r` with their x: one per pixel column,
    /// whichever road drew them. The host-drawn wave is a vertical
    /// `LineSegment` per column; the pyramid's band is one `Mesh` per clip
    /// with two vertices per column, top then bottom, and a column's rows
    /// are read straight off that pair — the band between neighbours is
    /// the tessellator's business, not the picture's.
    fn wave_columns_xy_in(v: &ArrangeView<'_>, r: Rect) -> Vec<(f32, f32, f32)> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let area = rect();
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(area), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(area);
                    draw(ui.painter(), area, v);
                });
            },
        );
        let mut got = Vec::new();
        for s in &out.shapes {
            match &s.shape {
                egui::Shape::LineSegment { points, .. } => {
                    let (a, b) = (points[0], points[1]);
                    // A wave column is VERTICAL and stands inside the clip.
                    if (a.x - b.x).abs() > 0.25 || !r.contains(a) || !r.contains(b) {
                        continue;
                    }
                    got.push((a.x, a.y.min(b.y), a.y.max(b.y)));
                }
                egui::Shape::Mesh(m) => {
                    for pair in m.vertices.chunks_exact(2) {
                        let (a, b) = (pair[0].pos, pair[1].pos);
                        if (a.x - b.x).abs() > 0.25 || !r.contains(a) || !r.contains(b) {
                            continue;
                        }
                        got.push((a.x, a.y.min(b.y), a.y.max(b.y)));
                    }
                }
                _ => {}
            }
        }
        got
    }

    /// **A looped clip is marked in every pass.**
    ///
    /// The wave is drawn pass by pass (`wave_band` folds the same way), so
    /// ticks on the first pass alone say the attacks stop after one bar -
    /// which for a looped take is the whole of what somebody is looking at.
    #[test]
    fn a_looped_clip_is_marked_in_every_pass() {
        let area = rect();
        let peaks = a_pyramid(8.0, 8.0, 0.0);
        let notes: Vec<crate::arrangement::ArrNote> = Vec::new();
        let marks = [(0.5_f32, 0.5_f32)];
        let ticks = |loop_s: f64, len_s: f64| -> usize {
            let clips = [ClipView {
                peaks: Some(&peaks),
                len_s,
                max_len_s: 8.0,
                loop_s,
                marks: &marks,
                ..a_looped_clip(&notes, 2.0, 2.0)
            }];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = 8.0;
            v.song_s = 12.0;
            let body = clip_rect(area, &v, &clips[0]);
            wave_columns_xy_in(&v, body)
                .into_iter()
                .filter(|(_, top, bot)| bot - top > body.height() * 0.8)
                .count()
        };
        // Unlooped: one attack, one tick.
        assert_eq!(ticks(0.0, 4.0), 1, "an unlooped clip drew the wrong count");
        // Four passes of a one-second loop: four.
        assert_eq!(ticks(1.0, 4.0), 4, "a looped clip was marked in its first pass only");
        // A loop longer than the clip is one pass, not a fraction of one.
        assert_eq!(ticks(6.0, 4.0), 1, "a loop longer than its clip drew twice");
    }

    /// **Each pass of a loop is marked with the attacks THAT pass plays**:
    /// the ones inside `offset_s .. offset_s + loop_s`, the span `wave_band`
    /// folds the pass's picture into. Every pass repeated every mark it was
    /// handed, so a mark from outside the loop stood in the next pass's
    /// audio, over a place the loop plays something else.
    #[test]
    fn a_looped_clip_marks_only_the_attacks_its_loop_plays() {
        let area = rect();
        let peaks = a_pyramid(8.0, 8.0, 0.0);
        let notes: Vec<crate::arrangement::ArrNote> = Vec::new();
        // Attacks at 0.3, 1.5 and 2.8 s of the FILE; the clip loops 1.0..2.0 of it, four passes.
        let marks = [(0.3_f32, 0.3_f32), (1.5, 1.5), (2.8, 2.8)];
        let clips = [ClipView {
            peaks: Some(&peaks),
            len_s: 4.0,
            max_len_s: 8.0,
            loop_s: 1.0,
            offset_s: 1.0,
            marks: &marks,
            ..a_looped_clip(&notes, 2.0, 2.0)
        }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 8.0;
        v.song_s = 12.0;
        let body = clip_rect(area, &v, &clips[0]);
        let n = wave_columns_xy_in(&v, body)
            .into_iter()
            .filter(|(_, top, bot)| bot - top > body.height() * 0.8)
            .count();
        assert_eq!(n, 4, "drew {n} ticks where each of four passes plays one attack");
    }

    /// **The quantize window's marks are drawn, where the detector put
    /// them, and only while it is asking.**
    ///
    /// The owner: "the waveform should have transient marks shown as the
    /// different settings are tweaked in its menu". A mark is full height
    /// on purpose - it is an answer to "where", and one that reached only
    /// the middle of the wave would read as part of the wave - which is
    /// also what lets this tell them from the wave's own columns.
    #[test]
    fn the_quantize_marks_land_on_the_attacks_they_name() {
        let area = rect();
        let peaks = a_pyramid(8.0, 8.0, 0.0);
        let notes: Vec<crate::arrangement::ArrNote> = Vec::new();
        let base = ClipView {
            peaks: Some(&peaks),
            len_s: 4.0,
            max_len_s: 8.0,
            loop_s: 0.0,
            ..a_looped_clip(&notes, 2.0, 2.0)
        };
        // Every FULL-HEIGHT vertical inside the clip, which is a mark; the
        // wave's own columns reach two fifths of the band about its middle.
        let ticks = |marks: &[(f32, f32)]| -> Vec<f32> {
            let clips = [ClipView { marks, ..base }];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = 8.0;
            v.song_s = 12.0;
            let r = clip_rect(area, &v, &clips[0]);
            let mut xs: Vec<f32> = wave_columns_xy_in(&v, r)
                .into_iter()
                .filter(|(_, top, bot)| bot - top > r.height() * 0.8)
                .map(|(x, ..)| x)
                .collect();
            xs.sort_by(f32::total_cmp);
            xs
        };

        // Nothing asked, nothing drawn - the ordinary clip is untouched.
        assert!(ticks(&[]).is_empty(), "a clip with no marks drew some anyway");

        // A hit already in time draws ONE tick, at its own second. The
        // mapping is the panel's own, so this cannot drift from the draw.
        let clips = [ClipView { marks: &[], ..base }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 8.0;
        v.song_s = 12.0;
        let l = Layout::new(area, &v);
        let x_at = |t: f64| x_of_in(&l, &v, t);
        let one = ticks(&[(1.0, 1.0)]);
        assert_eq!(one.len(), 1, "a hit in time drew {} ticks", one.len());
        assert!(
            (one[0] - x_at(1.0)).abs() < 1.5,
            "the tick landed at {} and the attack is at {}",
            one[0],
            x_at(1.0)
        );

        // One that would MOVE draws two: where it was played, and where it
        // would go. That gap is the whole reading.
        let two = ticks(&[(1.0, 1.5)]);
        assert_eq!(two.len(), 2, "a moved hit drew {} ticks", two.len());
        assert!(
            (two[0] - x_at(1.0)).abs() < 1.5 && (two[1] - x_at(1.5)).abs() < 1.5,
            "the pair landed at {two:?}, not at {} and {}",
            x_at(1.0),
            x_at(1.5)
        );

        // Several, each at its own place.
        let many = ticks(&[(0.5, 0.5), (1.0, 1.0), (2.0, 2.0), (3.5, 3.5)]);
        assert_eq!(many.len(), 4, "four attacks drew {} ticks", many.len());

        // **And an attack outside the drawn body is not drawn at all.**
        // Captured over the WHOLE surface rather than inside the clip,
        // because the fault this guards against is a tick landing on the
        // neighbouring clip - which a capture bounded by this one cannot
        // see. The painter is clipped to the panel, not to the body.
        // Over the whole surface, and told apart from the grid, the
        // playhead and everything else vertical by their own INK.
        let wide = |marks: &[(f32, f32)]| -> Vec<f32> {
            let clips = [ClipView { marks, ..base }];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = 8.0;
            v.song_s = 12.0;
            let ctx = egui::Context::default();
            crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
            crate::fonts::apply_text_styles(&ctx);
            let out = ctx.run(
                egui::RawInput { screen_rect: Some(area), ..Default::default() },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        ui.set_clip_rect(area);
                        draw(ui.painter(), area, &v);
                    });
                },
            );
            let mut xs: Vec<f32> = out
                .shapes
                .iter()
                .filter_map(|s| match &s.shape {
                    egui::Shape::LineSegment { points, stroke }
                        if stroke.color == MARK_INK && (points[0].x - points[1].x).abs() < 0.25 =>
                    {
                        Some(points[0].x)
                    }
                    _ => None,
                })
                .collect();
            xs.sort_by(f32::total_cmp);
            xs
        };
        let outside = wide(&[(50.0, 50.0), (2.0, 2.0)]);
        assert_eq!(
            outside.len(),
            1,
            "a mark past the clip reached the surface: {outside:?}"
        );
        assert!(
            (outside[0] - x_at(2.0)).abs() < 1.5,
            "the surviving mark is not the one inside the clip"
        );
        // The target half too: a hit that would be thrown past the clip
        // draws its source and nothing else.
        let far_target = wide(&[(2.0, 50.0)]);
        assert_eq!(far_target.len(), 1, "a target past the clip was drawn: {far_target:?}");

        // **A TRIMMED clip marks the same attacks.** The seconds are the
        // FILE's, so the trim comes off here - without that, every mark on
        // a trimmed take sits `offset_s` too far right.
        let trimmed = ClipView { offset_s: 1.0, ..base };
        let clips_t = [ClipView { marks: &[(2.0, 2.0)], ..trimmed }];
        let mut vt = a_view(&clips_t);
        vt.view_start_s = 0.0;
        vt.view_len_s = 8.0;
        vt.song_s = 12.0;
        let rt = clip_rect(area, &vt, &clips_t[0]);
        let xs: Vec<f32> = wave_columns_xy_in(&vt, rt)
            .into_iter()
            .filter(|(_, top, bot)| bot - top > rt.height() * 0.8)
            .map(|(x, ..)| x)
            .collect();
        assert_eq!(xs.len(), 1, "the trimmed clip drew {} ticks", xs.len());
        // An attack at file second 2 on a clip trimmed by 1 is heard at
        // second 1 of the timeline.
        let lt = Layout::new(area, &vt);
        assert!(
            (xs[0] - x_of_in(&lt, &vt, 1.0)).abs() < 1.5,
            "the trim was not taken off: drew at {}, the attack is heard at {}",
            xs[0],
            x_of_in(&lt, &vt, 1.0)
        );
    }

    /// **The waveform wears the clip's gain.**
    ///
    /// Reported: "when normalizing, the clip's waveform should reflect the
    /// new volume to visually see these things better." It did not - the
    /// picture came off the raw file's peaks, so a normalise changed how a
    /// take SOUNDED and not how it LOOKED, which is the one place a level
    /// edit is supposed to be obvious.
    ///
    /// Measured as the band's HEIGHT rather than as a pixel row, because
    /// where the middle sits is the layout's business and how tall the wave
    /// is is this one's.
    #[test]
    fn the_drawn_wave_follows_the_clips_gain() {
        let area = rect();
        let peaks = a_pyramid(8.0, 8.0, 0.0);
        let notes: Vec<crate::arrangement::ArrNote> = Vec::new();
        let base = ClipView {
            peaks: Some(&peaks),
            len_s: 4.0,
            max_len_s: 8.0,
            loop_s: 0.0,
            ..a_looped_clip(&notes, 2.0, 2.0)
        };
        let height = |g: f32| -> f32 {
            let clips = [ClipView { gain: g, ..base }];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = 8.0;
            v.song_s = 12.0;
            let r = clip_rect(area, &v, &clips[0]);
            wave_columns_xy_in(&v, r)
                .into_iter()
                .map(|(_, top, bot)| bot - top)
                .fold(0.0f32, f32::max)
        };
        let unity = height(1.0);
        assert!(unity > 2.0, "the fixture drew no wave at all: {unity}");

        // Quieter draws SHORTER, louder draws TALLER.
        let quiet = height(0.25);
        assert!(quiet < unity * 0.6, "a quarter gain drew {quiet} against {unity}");
        let loud = height(2.0);
        assert!(loud > unity, "twice the gain drew {loud}, no taller than {unity}");

        // **And it stops at the band**, rather than spilling over the
        // neighbouring lane to say "louder".
        let huge = height(64.0);
        assert!(
            huge <= loud + 1.5,
            "a gain of 64 drew {huge} and ran past the clamp at {loud}"
        );
    }

    /// A pyramid over `seconds` of stereo audio at 48 kHz: a 5 kHz tone
    /// under a slow amplitude swell. Five kilohertz so that every column at
    /// every zoom these tests draw holds whole cycles — a column's min and
    /// max are then the envelope and not the phase, and two columns a hair
    /// apart in time read the same. (At 220 Hz a millisecond column held a
    /// fifth of a cycle and neighbours disagreed by half the height.)
    /// `after` is the amplitude from `loud_until_s` on — a step the
    /// loop-fold test reads.
    fn a_pyramid(seconds: f64, loud_until_s: f64, after: f32) -> crate::peaks::Peaks {
        let rate = 48_000u32;
        let frames = (seconds * f64::from(rate)) as usize;
        let mut v = Vec::with_capacity(frames * 2);
        for f in 0..frames {
            let t = f as f64 / f64::from(rate);
            let env = if t < loud_until_s {
                0.55 + 0.35 * (t * std::f64::consts::TAU * 0.25).sin()
            } else {
                f64::from(after)
            };
            let x = ((t * std::f64::consts::TAU * 5_000.0).sin() * env) as f32;
            v.push(x);
            v.push(x * 0.5);
        }
        crate::peaks::Peaks::build(&v, 2, rate)
    }

    /// The host's old per-pixel picture of the same pyramid's file: `cols`
    /// min/max pairs over `from_s..from_s + span_s` of the source, each the
    /// exact fold of its frames — what `ClipInfo::wave` carried before the
    /// pyramid, built from the pyramid so the two pictures share one file.
    fn a_legacy_wave(p: &crate::peaks::Peaks, from_s: f64, span_s: f64, cols: usize) -> Vec<f32> {
        (0..cols)
            .flat_map(|i| {
                let a = from_s + span_s * i as f64 / cols as f64;
                let b = from_s + span_s * (i + 1) as f64 / cols as f64;
                let (lo, hi) = p.minmax_s(a, b);
                [lo, hi]
            })
            .collect()
    }

    /// **The band from the pyramid covers the rows the columns did.**
    ///
    /// Same file, two roads: the host's per-pixel picture drawn as a line
    /// per column, and the pyramid drawn as one band. At every column the
    /// two must agree to a point — the band is a different SHAPE (one mesh,
    /// two vertices a column, a one-point floor where the stroke had a half)
    /// and must not be a different picture. Checked at three zooms, from
    /// the clip seen whole to a bar of it, because the pyramid picks a
    /// different level at each and the host's buckets do not.
    #[test]
    fn the_band_from_the_pyramid_covers_the_rows_the_columns_did() {
        let none: [crate::arrangement::ArrNote; 0] = [];
        let peaks = a_pyramid(8.0, 8.0, 0.0);
        let area = rect();
        for (start_s, len_s) in [(0.0f64, 12.0f64), (1.0, 4.0), (3.5, 1.0)] {
            let clip = ClipView {
                midi: false,
                notes: &none,
                loop_s: 0.0,
                ..a_looped_clip(&none, 2.0, 4.0)
            };
            assert!((clip.len_s - 8.0).abs() < 1e-9);
            // The host's picture for THIS window: one pair per pixel over
            // the seconds on screen, as `wave_span` used to ask for.
            let (vs, vl) = (start_s, len_s);
            let from = vs.max(clip.start_s) - clip.start_s;
            let to = (vs + vl).min(clip.start_s + clip.len_s) - clip.start_s;
            let px = {
                let v = a_view(std::slice::from_ref(&clip));
                Layout::new(area, &v).lanes_area.width()
            };
            let cols = ((to - from) / vl * f64::from(px)).ceil().max(1.0) as usize;
            let wave = a_legacy_wave(&peaks, from + clip.offset_s, to - from, cols);
            let legacy = [ClipView {
                wave: &wave,
                wave_from_s: from,
                wave_span_s: to - from,
                ..clip
            }];
            let banded = [ClipView { peaks: Some(&peaks), ..clip }];
            let mut vl_ = a_view(&legacy);
            vl_.view_start_s = start_s;
            vl_.view_len_s = len_s;
            vl_.song_s = 12.0;
            let mut vb = a_view(&banded);
            vb.view_start_s = start_s;
            vb.view_len_s = len_s;
            vb.song_s = 12.0;
            let r = clip_rect(area, &vb, &banded[0]);
            assert!(r.is_positive());
            let old = wave_columns_xy_in(&vl_, r);
            let new = wave_columns_xy_in(&vb, r);
            assert!(old.len() > 100, "window {start_s}+{len_s}: the columns drew {}", old.len());
            assert!(
                (new.len() as i64 - old.len() as i64).abs() <= 2,
                "window {start_s}+{len_s}: {} columns against {} band vertices",
                old.len(),
                new.len()
            );
            let mut worst = 0.0f32;
            for (x, top, bot) in &new {
                let Some((_, t2, b2)) = old.iter().find(|(ox, ..)| (ox - x).abs() < 0.01) else {
                    continue;
                };
                worst = worst.max((top - t2).abs()).max((bot - b2).abs());
                assert!(
                    (top - t2).abs() <= 1.0 && (bot - b2).abs() <= 1.0,
                    "window {start_s}+{len_s}, column {x}: band {top}..{bot} vs columns {t2}..{b2}"
                );
            }
            assert!(worst > 0.0 || new.len() == old.len(), "nothing was compared");
        }
    }

    /// **A looped audio clip's picture repeats its first pass.**
    ///
    /// The fold used to live in the host (`looped_span_envelope`), and it
    /// was the only place a looped clip's wave was folded into its loop; a
    /// painter reading the pyramid straight through the clip's seconds
    /// would draw eight bars of FILE under a two-bar loop — the very fault
    /// that fold was written for. So: a four-pass clip over a file whose
    /// sound drops to a whisper after the loop's end, and every pass must be
    /// the loud first one, column for column, and none of them the whisper.
    #[test]
    fn a_looped_audio_clip_repeats_its_first_pass() {
        let none: [crate::arrangement::ArrNote; 0] = [];
        let loop_s = 2.0;
        // Loud for the loop's two seconds, a whisper for the six after.
        let peaks = a_pyramid(8.0, loop_s, 0.05);
        let clips = [ClipView {
            midi: false,
            notes: &none,
            peaks: Some(&peaks),
            ..a_looped_clip(&none, loop_s, 4.0)
        }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 8.0;
        v.song_s = 8.0;
        let area = rect();
        let r = clip_rect(area, &v, &clips[0]);
        assert!(r.is_positive());
        let cols = wave_columns_xy_in(&v, r);
        assert!(cols.len() > 400, "the fixture drew {} columns", cols.len());
        let pass_w = f64::from(x_of(area, &v, loop_s) - x_of(area, &v, 0.0));
        let half = r.height() * 0.40;
        let first: Vec<(f32, f32, f32)> =
            cols.iter().copied().filter(|(x, ..)| f64::from(x - r.left()) < pass_w).collect();
        assert!(first.len() > 100);
        let mut compared = 0usize;
        for (x, top, bot) in &cols {
            let k = (f64::from(x - r.left()) / pass_w).floor();
            if k < 1.0 {
                continue;
            }
            // The column ON a seam reads its first side and then on past
            // the seam — the host's old rule, kept — so it is the one
            // column of a pass that may differ from the first pass's.
            let phase = f64::from(x - r.left()) - k * pass_w;
            if phase < 1.0 || phase > pass_w - 1.0 {
                continue;
            }
            // The same column of the first pass, to the nearest point: a
            // pass is not a whole number of points wide, so the phase
            // inside it drifts by a fraction of a column per pass.
            let x1 = f64::from(*x) - k * pass_w;
            let Some((_, t1, b1)) = first
                .iter()
                .min_by(|a, b| (f64::from(a.0) - x1).abs().total_cmp(&(f64::from(b.0) - x1).abs()))
            else {
                continue;
            };
            assert!(
                (top - t1).abs() <= 1.5 && (bot - b1).abs() <= 1.5,
                "pass {k}, column {x}: {top}..{bot} against the first pass's {t1}..{b1}"
            );
            // And loud: a whisper would be a couple of points tall.
            assert!(
                bot - top > half * 0.5,
                "pass {k}, column {x} is {} tall: the file was read past the loop",
                bot - top
            );
            compared += 1;
        }
        assert!(compared > 300, "only {compared} columns in later passes");
    }

    fn diagonals_in(v: &ArrangeView<'_>, r: Rect) -> Vec<(Pos2, Pos2, Color32)> {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let area = rect();
        let out = ctx.run(
            egui::RawInput { screen_rect: Some(area), ..Default::default() },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.set_clip_rect(area);
                    draw(ui.painter(), area, v);
                });
            },
        );
        let mut got = Vec::new();
        for s in &out.shapes {
            let egui::Shape::LineSegment { points, stroke } = &s.shape else {
                continue;
            };
            let (a, b) = (points[0], points[1]);
            if (a.x - b.x).abs() < 0.25 || (a.y - b.y).abs() < 0.25 {
                continue;
            }
            if !r.contains(a) || !r.contains(b) {
                continue;
            }
            let (top, bot) = if a.y <= b.y { (a, b) } else { (b, a) };
            got.push((top, bot, stroke.color));
        }
        got.sort_by(|x, y| x.0.x.total_cmp(&y.0.x));
        got
    }

    /// **A looped clip shows where every pass ends**, as a diagonal recess.
    ///
    /// The owner: "make it such that theres a diagonal recess between loops."
    /// Two bars and twenty bars of the same one-bar part were the same green
    /// box, and the only way to find out how many times it repeated was to
    /// open the editor and count.
    ///
    /// Checked as GEOMETRY and as PAINT, because the two are the pair that
    /// drifts: `loop_seam_xs` is asserted to put a seam at every multiple of
    /// the loop inside the clip, and then the whole panel is run and the
    /// diagonals it actually emitted are matched against those same seconds.
    /// An empty item is used for the paint half — a MIDI item drawn and not
    /// yet played into is the case the seam matters most for, since it has no
    /// notes to hint at its own shape.
    ///
    /// Reverts performed, both red: give the two faces one colour and the
    /// groove assert goes; drop the `saturating_add(1)` from the `to` bound
    /// and the clip cut off at five seconds loses its last visible seam —
    /// which is why the cut window is here at all, since a clip seen whole
    /// gets that bound from its own end and the fault hides.
    #[test]
    fn a_looped_clip_is_notched_where_each_pass_ends() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        let none: [crate::arrangement::ArrNote; 0] = [];
        // Four passes of one bar, seen whole with room to spare, so a pass is
        // a couple of hundred points and nothing is near the density floor.
        let clips = [ClipView { notes: &none, ..a_looped_clip(&notes, bar_s, 4.0) }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 12.0;
        v.song_s = 12.0;
        let area = rect();
        let r = clip_rect(area, &v, &clips[0]);
        assert!(r.is_positive(), "the fixture clip is not on screen");
        let full_x = (
            x_of(area, &v, clips[0].start_s),
            x_of(area, &v, clips[0].start_s + clips[0].len_s),
        );
        // Three boundaries inside four passes: 2s, 4s and 6s. Not four — the
        // clip's own right edge is where it ends, not where it turns over.
        let want: Vec<f32> = [2.0, 4.0, 6.0].iter().map(|t| x_of(area, &v, *t)).collect();
        let got = loop_seam_xs(r, full_x, &clips[0]);
        assert_eq!(got.len(), want.len(), "seams at {got:?}, wanted {want:?}");
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 0.5, "a seam at {g} belongs at {w}");
        }

        // ── and the clip's SHAPE carries them ─────────────────────────
        //
        // Four passes, so the fill is four convex pieces rather than one
        // rounded rectangle. The previous seam was two diagonal STROKES
        // printed on the surface and this is deliberately not that: the
        // owner asked for "a simple shape change on the clip itself ... a
        // small cutout in the CLIP shape -- no line", so what is asserted
        // here is the outline, and the absence of any line drawn along it.
        let face = Color32::from_rgb(0x2E, 0x6B, 0x46);
        let fill = crate::recorder_panel::legible_surface(face);
        let pieces = clip_pieces_in(&v, r, fill);
        assert_eq!(
            pieces.len(),
            4,
            "four passes were painted as {} pieces, not one a pass",
            pieces.len()
        );

        // **Every seam is a bite out of the top edge AND the bottom edge.**
        // At the boundary the outline stands `bite` inside the clip; a
        // sixteenth of a pass either side of it, it is back on the edge. A
        // notch that only dented one edge, or that leaned all the way across
        // its pass, passes a presence check and fails this.
        let d = clip_bite_d(r);
        let on = |pts: &[Pos2], x: f32, y: f32| {
            pts.iter().any(|p| (p.x - x).abs() < 0.75 && (p.y - y).abs() < 0.75)
        };
        // **The depth first, on its own.** Without this the apex checks below
        // are satisfied by a bite of DEPTH ZERO — `top + 0.0` is the edge, so
        // an outline that never leaves the edge answers "yes, there is a
        // point there". A revert setting `clip_bite_d` to zero passed all
        // three of these tests before this line existed.
        assert!(d >= 2.0, "a bite {d} deep is not a bite");
        for (n, x) in got.iter().enumerate() {
            let left = &pieces[n];
            let right = &pieces[n + 1];
            assert!(
                on(left, *x, r.top() + d) && on(right, *x, r.top() + d),
                "seam {n} at {x} has no top bite: the pieces meet at full height"
            );
            assert!(
                on(left, *x, r.bottom() - d) && on(right, *x, r.bottom() - d),
                "seam {n} at {x} has no bottom bite"
            );
            // And the outline is AWAY from the edge there — the half that
            // makes this a shape check rather than a point lookup.
            assert!(
                !on(left, *x, r.top()) && !on(right, *x, r.top()),
                "seam {n} at {x} still meets the clip's top edge"
            );
            assert!(
                !on(left, *x, r.bottom()) && !on(right, *x, r.bottom()),
                "seam {n} at {x} still meets the clip's bottom edge"
            );
        }

        // **The bite is shallow enough to leave the material alone.** A tenth
        // of the lane, which is the band the waveform and the miniature are
        // both kept out of — see `clip_bite_d` and `miniature_band`.
        assert!(
            d <= r.height() * 0.1 + 1.0e-3,
            "a bite {d} deep eats into a {} lane's waveform",
            r.height()
        );

        // **And nothing was STROKED.** The old seam drew six diagonals here.
        let drawn = diagonals_in(&v, r);
        assert!(
            drawn.is_empty(),
            "the seam is a shape, not a line, and {} were drawn",
            drawn.len()
        );
    }

    /// **A clip trimmed from its front is notched where the loop actually
    /// turns over**, which is not a whole loop from its left edge.
    ///
    /// The owner: "midi loops that are clipped leave a latter half that is
    /// empty." That fault was the SCHEDULE's, and it was fixed in
    /// `clip_notes_in_song_time` by covering `len_s + offset_s`. The PICTURE
    /// had the same fault from the same cause and outlived the fix: seams
    /// were laid at whole loops from the clip's left edge, which is right
    /// only for a clip that begins ON a boundary. Trim a quarter of a bar off
    /// the front and every notch sat a quarter of a bar late, over the middle
    /// of a pass — the one control whose whole job is to say where a loop
    /// restarts, pointing somewhere it does not.
    ///
    /// The notes are placed at `at_s - offset_s + loop_s * pass`, so the
    /// boundaries inside a clip are at `loop_s * p - offset_s`: a two-second
    /// bar trimmed half a second in turns over at 1.5s, 3.5s, 5.5s and 7.5s
    /// across four bars of clip. FOUR seams, not the three an untrimmed clip
    /// of the same length gets — the short head pushes the last boundary half
    /// a second clear of the end instead of onto it.
    ///
    /// Two corners ride along. A clip trimmed by exactly one WHOLE loop is
    /// untrimmed as far as the seams are concerned, so a phase of zero must
    /// not put a seam on the clip's own left edge. And a clip SHORTER than
    /// its loop still turns over if its offset leaves a boundary inside it,
    /// which the old `len_s <= loop_s` early-out refused outright.
    #[test]
    fn a_clip_trimmed_from_its_front_is_notched_where_the_loop_turns_over() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        let none: [crate::arrangement::ArrNote; 0] = [];
        let area = rect();
        let face = Color32::from_rgb(0x2E, 0x6B, 0x46);
        let fill = crate::recorder_panel::legible_surface(face);
        for (what, offset_s, bars, want_s) in [
            ("trimmed a quarter of a bar in", 0.5, 4.0, vec![1.5, 3.5, 5.5, 7.5]),
            ("trimmed by one whole bar", bar_s, 4.0, vec![2.0, 4.0, 6.0]),
            ("shorter than its loop, trimmed across a boundary", 1.5, 0.75, vec![0.5]),
        ] {
            let clips =
                [ClipView { notes: &none, offset_s, ..a_looped_clip(&notes, bar_s, bars) }];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = 12.0;
            v.song_s = 12.0;
            let r = clip_rect(area, &v, &clips[0]);
            assert!(r.is_positive(), "{what}: the fixture is not on screen");
            let full_x = (
                x_of(area, &v, clips[0].start_s),
                x_of(area, &v, clips[0].start_s + clips[0].len_s),
            );
            let want: Vec<f32> = want_s.iter().map(|t| x_of(area, &v, *t)).collect();
            let got = loop_seam_xs(r, full_x, &clips[0]);
            assert_eq!(
                got.len(),
                want.len(),
                "{what}: seams at {got:?}, wanted {want:?} ({want_s:?}s)"
            );
            for (g, w) in got.iter().zip(&want) {
                assert!((g - w).abs() < 0.5, "{what}: a seam at {g} belongs at {w}");
            }

            // **And the clip's own outline carries them**, one piece a pass,
            // each seam a bite out of both edges. The geometry half alone
            // would pass on a function nobody draws with.
            let pieces = clip_pieces_in(&v, r, fill);
            assert_eq!(
                pieces.len(),
                got.len() + 1,
                "{what}: {} pieces of outline for {} seams",
                pieces.len(),
                got.len()
            );
            let d = clip_bite_d(r);
            assert!(d >= 2.0, "a bite {d} deep is not a bite");
            let on = |pts: &[Pos2], x: f32, y: f32| {
                pts.iter().any(|p| (p.x - x).abs() < 0.75 && (p.y - y).abs() < 0.75)
            };
            for (n, x) in got.iter().enumerate() {
                let (left, right) = (&pieces[n], &pieces[n + 1]);
                assert!(
                    on(left, *x, r.top() + d) && on(right, *x, r.top() + d),
                    "{what}: seam {n} at {x} has no top bite"
                );
                assert!(
                    on(left, *x, r.bottom() - d) && on(right, *x, r.bottom() - d),
                    "{what}: seam {n} at {x} has no bottom bite"
                );
            }

            // **And the miniature was moved out of the bites' way**, which is
            // the agreement `clip_has_seam` exists to keep. The third case is
            // where the two used to part company: it is shorter than its own
            // loop, so the old question answered "this clip does not turn
            // over" while the seams — correctly — bit it anyway, and a note
            // bar would have hung in the lane through the hole.
            let band = miniature_band(r, &clips[0]);
            assert!(
                band.top() >= r.top() + d - 0.01 && band.bottom() <= r.bottom() - d + 0.01,
                "{what}: the miniature band {band:?} reaches into a {d}-point bite"
            );
        }

        // ── and an AUDIO clip's trim does NOT move its seams ──────────
        //
        // The two kinds mean different things by the same number. A MIDI
        // trim slides the material across a fixed grid; an audio trim picks
        // which seconds of the FILE play, and `audio_clip_passes` hands the
        // same offset to every pass while placing pass `k` at `loop_s * k`
        // from the clip's start. So the identical trim that moves the MIDI
        // seams above must leave these where they were, on the loop, over
        // the wave that really does restart there.
        let clips = [ClipView {
            notes: &none,
            midi: false,
            offset_s: 0.5,
            ..a_looped_clip(&notes, bar_s, 4.0)
        }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 12.0;
        v.song_s = 12.0;
        let r = clip_rect(area, &v, &clips[0]);
        let full_x = (
            x_of(area, &v, clips[0].start_s),
            x_of(area, &v, clips[0].start_s + clips[0].len_s),
        );
        let want: Vec<f32> = [2.0, 4.0, 6.0].iter().map(|t| x_of(area, &v, *t)).collect();
        let got = loop_seam_xs(r, full_x, &clips[0]);
        assert_eq!(
            got.len(),
            want.len(),
            "a trimmed audio clip: seams at {got:?}, wanted {want:?} (2, 4, 6s)"
        );
        for (g, w) in got.iter().zip(&want) {
            assert!((g - w).abs() < 0.5, "a trimmed audio clip: a seam at {g} belongs at {w}");
        }
    }

    /// **An AUDIO clip that loops is bitten too, and the bite clears its
    /// waveform.**
    ///
    /// The seam pass lived inside the `clip.midi` branch until the day audio
    /// started repeating, and it is now the clip's own outline rather than a
    /// mark on it, so this has to hold for a clip whose material is a wave.
    ///
    /// The second half is the one a presence check cannot see. When the seam
    /// was two strokes, the risk was ORDER — a notch printed before the
    /// waveform had the waveform painted back over it. Now that the notch is
    /// a hole, the risk is the mirror of that: the wave is drawn after the
    /// fill and would hang in the lane outside its own clip if it reached
    /// into a bite. So what is asserted is the CLEARANCE — every column of
    /// the waveform stays inside the band the bites leave alone.
    ///
    /// Revert checks performed: return `0.0` from `clip_bite_d` and the bite
    /// asserts name the missing divot; widen it to a third of the height and
    /// the clearance assert names the column that reaches into it.
    #[test]
    fn a_looped_audio_clip_is_notched_where_each_pass_ends() {
        let bar_s = 2.0;
        let none: [crate::arrangement::ArrNote; 0] = [];
        // A wave with something in it, spanning the whole clip: every bucket
        // a full-scale pair, so the columns are unmistakable and none of them
        // is skipped for being flat.
        let wave: Vec<f32> = (0..256).flat_map(|_| [-0.8f32, 0.8]).collect();
        let clips = [ClipView {
            midi: false,
            notes: &none,
            wave: &wave,
            wave_from_s: 0.0,
            wave_span_s: bar_s * 4.0,
            peaks: None,
            ..a_looped_clip(&none, bar_s, 4.0)
        }];
        let mut v = a_view(&clips);
        v.view_start_s = 0.0;
        v.view_len_s = 12.0;
        v.song_s = 12.0;
        let area = rect();
        let r = clip_rect(area, &v, &clips[0]);
        assert!(r.is_positive(), "the fixture clip is not on screen");

        // Three boundaries inside four passes, so four pieces.
        let want: Vec<f32> = [2.0, 4.0, 6.0].iter().map(|t| x_of(area, &v, *t)).collect();
        let fill = crate::recorder_panel::legible_surface(Color32::from_rgb(0x2E, 0x6B, 0x46));
        let pieces = clip_pieces_in(&v, r, fill);
        assert_eq!(
            pieces.len(),
            4,
            "a looping AUDIO clip was painted as {} pieces, not one a pass",
            pieces.len()
        );
        let d = clip_bite_d(r);
        let on = |pts: &[Pos2], x: f32, y: f32| {
            pts.iter().any(|p| (p.x - x).abs() < 0.75 && (p.y - y).abs() < 0.75)
        };
        assert!(d >= 2.0, "a bite {d} deep is not a bite");
        for (n, x) in want.iter().enumerate() {
            assert!(
                on(&pieces[n], *x, r.top() + d) && on(&pieces[n + 1], *x, r.top() + d),
                "seam {n} at {x} has no top bite"
            );
            assert!(
                on(&pieces[n], *x, r.bottom() - d) && on(&pieces[n + 1], *x, r.bottom() - d),
                "seam {n} at {x} has no bottom bite"
            );
            assert!(
                !on(&pieces[n], *x, r.top()) && !on(&pieces[n + 1], *x, r.top()),
                "seam {n} at {x} still meets the clip's top edge"
            );
        }
        assert!(
            diagonals_in(&v, r).is_empty(),
            "the seam is a shape now, and a line was drawn"
        );

        // **The wave stays out of the bites.** Full-scale on every bucket, so
        // this is the worst case the clip can offer — on BOTH roads, the
        // host-drawn columns and the pyramid's band.
        let full: Vec<f32> = vec![1.0f32, -1.0, 1.0, -1.0].repeat(48_000 * 2);
        let loud = crate::peaks::Peaks::build(&full, 2, 48_000);
        let banded = [ClipView { peaks: Some(&loud), ..clips[0] }];
        let mut vb = a_view(&banded);
        vb.view_start_s = 0.0;
        vb.view_len_s = 12.0;
        vb.song_s = 12.0;
        for (road, cols) in [("columns", wave_columns_in(&v, r)), ("band", wave_columns_in(&vb, r))] {
            assert!(!cols.is_empty(), "{road}: the fixture drew no waveform");
            for (top, bot) in &cols {
                assert!(
                    *top >= r.top() + d - 0.5 && *bot <= r.bottom() - d + 0.5,
                    "{road}: a waveform column runs {top}..{bot}, into a bite {d} deep on \
                     a clip spanning {}..{}",
                    r.top(),
                    r.bottom()
                );
            }
        }
    }

    /// **Nothing that does not loop, and nothing too fine to place, is
    /// notched.**
    ///
    /// Three cases, each of which drew a seam in some draft of this: a clip
    /// with no loop at all (a document written before looping existed, and a
    /// clip whose Loop row has been unticked), a clip that holds exactly one
    /// pass — which is every clip at the moment it is made, since a loop as
    /// long as its clip has nothing yet to end against — and a one-bar loop
    /// over two hundred bars seen whole, where a seam every point is a moiré
    /// that claims to be structure.
    ///
    /// Revert performed: drop the `SEAM_MIN_PASS_W` gate and the last case
    /// paints a hundred and ninety-odd notches across one clip.
    #[test]
    fn a_clip_with_no_loop_or_no_room_for_one_is_not_notched() {
        let bar_s = 2.0;
        let notes = a_looped_bar(bar_s);
        let area = rect();
        for (what, clip, window_s) in [
            (
                "a clip that does not loop",
                ClipView { loop_s: 0.0, ..a_looped_clip(&notes, bar_s, 4.0) },
                12.0,
            ),
            ("a clip holding one pass", a_looped_clip(&notes, bar_s, 1.0), 12.0),
            (
                "two hundred passes seen whole",
                a_looped_clip(&notes, bar_s, 200.0),
                bar_s * 200.0,
            ),
        ] {
            let clips = [clip];
            let mut v = a_view(&clips);
            v.view_start_s = 0.0;
            v.view_len_s = window_s;
            v.song_s = window_s;
            let r = clip_rect(area, &v, &clips[0]);
            assert!(r.is_positive(), "{what}: the fixture is not on screen");
            let full_x = (
                x_of(area, &v, clips[0].start_s),
                x_of(area, &v, clips[0].start_s + clips[0].len_s),
            );
            let got = loop_seam_xs(r, full_x, &clips[0]);
            assert!(got.is_empty(), "{what} was given {} seams", got.len());
            let drawn = diagonals_in(&v, r);
            assert!(drawn.is_empty(), "{what} painted {} diagonals", drawn.len());
        }
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

    /// **A note whose numbers are nonsense draws no panic.**
    ///
    /// The times in a clip come out of a document, and a document can hold
    /// whatever a file on disk holds: seconds far past any song, seconds
    /// before the start, a length of zero to divide by. The marks are
    /// vertices in a mesh now rather than rects, so this walks the whole
    /// drawing — the plain span pass, the merge grid and the handover — with
    /// each of those in turn, at a zoom where the grid is allocated and at
    /// one where it is not, and checks that what came out is a mesh
    /// epaint will accept: whole marks, and every index inside its own
    /// vertex list.
    ///
    /// It asserts survival, not a picture: what a NaN should LOOK like is
    /// not a question with an answer, and the answer today is that it maps
    /// to the whole clip (`f64::max(NaN, 0.0)` is 0.0 and `min(NaN, 1.0)` is
    /// 1.0), which is what it did as rects too. The column index is the one
    /// place this could go wrong quietly — `as usize` saturates a NaN to 0
    /// and an infinity to the maximum, and the clamp behind it is what keeps
    /// both inside the grid.
    ///
    /// It reads the shape list rather than `lit_cells` on purpose. A NaN
    /// note draws a mark the full width of the clip, and rasterising five
    /// hundred passes of those at four cells to the point took the first
    /// version of this test seven minutes to say "no panic".
    #[test]
    fn a_note_with_impossible_times_survives_the_drawing() {
        use crate::arrangement::ArrNote;
        let wild = [
            ArrNote { at_s: f64::NAN, dur_s: 0.25, key: 60, vel: 100, muted: false },
            ArrNote { at_s: 0.0, dur_s: f64::NAN, key: 61, vel: 100, muted: false },
            ArrNote { at_s: f64::INFINITY, dur_s: 1.0, key: 62, vel: 100, muted: false },
            ArrNote { at_s: f64::NEG_INFINITY, dur_s: 1.0, key: 63, vel: 100, muted: false },
            ArrNote { at_s: -1.0e9, dur_s: 1.0e9, key: 64, vel: 100, muted: false },
            ArrNote { at_s: 1.0e18, dur_s: 1.0e18, key: 65, vel: 100, muted: false },
            ArrNote { at_s: f64::MAX, dur_s: f64::MAX, key: 66, vel: 100, muted: false },
            ArrNote { at_s: 0.5, dur_s: -4.0, key: 67, vel: 100, muted: false },
            // One ordinary note, so the clip has a picture to lose.
            ArrNote { at_s: 1.0, dur_s: 0.5, key: 68, vel: 100, muted: false },
        ];
        let ctx = egui::Context::default();
        let layer = egui::LayerId::new(egui::Order::Background, egui::Id::new("wild-mini"));
        let painter = Painter::new(ctx.clone(), layer, Rect::EVERYTHING);
        let bar_s = 2.0;
        let mut drawn = 0usize;
        for loop_s in [0.0, bar_s] {
            for len_s in [0.0, 1.0e-12, bar_s * 8.0, 1.0e12] {
                for offset_s in [0.0, f64::NAN, -1.0e9] {
                    let clips =
                        [ClipView { len_s, loop_s, offset_s, ..a_looped_clip(&wild, bar_s, 8.0) }];
                    // Two windows: one where 0.02 s is under a point and the
                    // merge grid is built, one where it is not.
                    for window_s in [bar_s * 400.0, bar_s * 0.5] {
                        let mut v = a_view(&clips);
                        v.view_start_s = 0.0;
                        v.view_len_s = window_s;
                        v.song_s = window_s;
                        ctx.graphics_mut(|g| *g.entry(layer) = Default::default());
                        draw_note_miniature(
                            &painter,
                            rect(),
                            (0.0, 1300.0),
                            &clips[0],
                            Color32::WHITE,
                        );
                        let what = format!("loop {loop_s} len {len_s} trim {offset_s} at {window_s}");
                        ctx.graphics(|g| {
                            for c in g.get(layer).into_iter().flat_map(|l| l.all_entries()) {
                                if let egui::Shape::Mesh(m) = &c.shape {
                                    assert!(m.is_valid(), "{what}: an index past the vertices");
                                    assert_eq!(
                                        m.vertices.len() % 4,
                                        0,
                                        "{what}: a mark with {} vertices",
                                        m.vertices.len()
                                    );
                                    drawn += m.vertices.len() / 4;
                                }
                            }
                        });
                    }
                }
            }
        }
        // Every one of those clips could have bailed out before it drew, and
        // a sweep that painted nothing would assert nothing.
        assert!(drawn > 0, "no mark was drawn at all, so nothing was checked");
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


    /// The lane a hit is about, for the bounds test: a track's row, the
    /// master, or nothing (a clip, the toolbar, the ruler).
    fn lane_of_hit(hit: Hit) -> Option<LaneId> {
        Some(match hit {
            Hit::HdrNumber(i)
            | Hit::HdrCollapse(i)
            | Hit::HdrBay(i, _)
            | Hit::HdrRouting(i)
            | Hit::Arm(i)
            | Hit::Mute(i)
            | Hit::Solo(i)
            | Hit::Gain(i)
            | Hit::Send(i)
            | Hit::Pan(i)
            | Hit::Phase(i)
            | Hit::ToTheory(i)
            | Hit::HdrInput(i)
            | Hit::HdrName(i)
            | Hit::HdrAdd(i)
            | Hit::Lane(i, _) => LaneId::Row(i),
            Hit::AutoPoint(l, ..)
            | Hit::AutoLane(l, ..)
            | Hit::AutoParamPick(l, _)
            | Hit::AutoClose(l, _)
            | Hit::AutoArm(l, _)
            | Hit::AutoBypass(l, _)
            | Hit::AutoDial(l, _)
            | Hit::AutoMenu(l) => l,
            Hit::BusBay(..) | Hit::BusGain(_) | Hit::MasterLane(_) => LaneId::Master,
            _ => return None,
        })
    }

    /// **A view the app's way, from an index**: the window
    /// `lane_window_start` names, built from the drawn order. What
    /// `TangentApp::arrange_view` does with its own index.
    fn windowed<'a>(
        ix: &crate::desk_index::DeskIndex,
        clips: &'a [ClipView<'a>],
        visible: usize,
        lane_scroll: usize,
    ) -> ArrangeView<'a> {
        // In RANKS among the drawn rows (`ArrangeView::lane_scroll`), as the
        // app builds its window.
        let mut v = a_view(clips);
        v.active = ix.shown_len();
        v.visible_lanes = visible;
        v.lane_scroll = lane_scroll.min(ix.shown_len().saturating_sub(visible));
        let (first, can) = lane_window_start(v.lane_scroll, visible, v.active);
        let rows = ix.window(first, can + 1);
        v.lanes = Lanes::new(rows.iter().map(|r| (*r as usize, a_lane(true))).collect()).starting_at(first);
        v
    }

    /// **A timeline of any length builds, lays out, hit-tests and describes
    /// only its window.** On the XXL and XXXL rungs (128 and 512 rows,
    /// folders open and shut), at four zooms and three scrolls: the lanes
    /// BUILT, the track rows LAID OUT, the lanes the hit test answers for
    /// and the lanes the screen reader is told about are each bounded by
    /// what shows plus the pinned master plus two. And the window is the
    /// right one: the first lane on screen is the first drawn row at the
    /// scroll, and the last row of the desk shows at the end of the rail.
    ///
    /// Revert-check: build `windowed` from every drawn row and the first
    /// assertion names the 512-row desk.
    #[test]
    fn a_timeline_of_any_length_builds_and_lays_out_only_its_window() {
        use crate::perf_fixtures::Wide;
        let cs: Vec<ClipView<'_>> = Vec::new();
        let r = rect();
        for wide in Wide::ALL {
            for shut in [false, true] {
                let ix = wide.index(shut);
                for visible in [1usize, 4, 8, 64] {
                    for scroll in [0usize, 77, usize::MAX] {
                        let v = windowed(&ix, &cs, visible, scroll);
                        let bound = visible + 1 + 2;
                        let what = format!(
                            "{} shut={shut} visible={visible} scroll={scroll}",
                            wide.tag()
                        );
                        assert!(v.lanes.len() <= bound, "{what}: built {} lanes", v.lanes.len());
                        let l = Layout::new(r, &v);
                        let tracks = l
                            .rows
                            .iter()
                            .filter(|row| matches!(row.kind, RowKind::Track(_)))
                            .count();
                        assert!(tracks <= bound, "{what}: laid out {tracks} track rows");
                        assert!(l.lane_rows.len() <= bound && l.header_rows.len() <= bound);
                        let hits = targets_in(&l, &v);
                        let mut lanes: Vec<LaneId> =
                            hits.iter().filter_map(|(_, h)| lane_of_hit(*h)).collect();
                        lanes.sort();
                        lanes.dedup();
                        assert!(lanes.len() <= bound, "{what}: {} lanes answer presses", lanes.len());
                        let nodes = a11y_nodes_from(&v, &hits);
                        assert!(nodes.len() <= hits.len(), "{what}: more nodes than targets");
                        // The first lane laid out is the drawn row at the
                        // (clamped) scroll, a rank.
                        let first_drawn = ix.row_at(v.lane_scroll);
                        let first_laid = l
                            .rows
                            .iter()
                            .find_map(|row| match row.kind {
                                RowKind::Track(i) if row.lane.is_positive() => Some(i),
                                _ => None,
                            });
                        assert_eq!(first_laid, first_drawn, "{what}: the window starts elsewhere");
                        if scroll == usize::MAX && !shut {
                            assert!(
                                l.lane_rows[ix.extent() - 1].is_positive(),
                                "{what}: the end of the rail does not show the last row"
                            );
                        }
                    }
                }
            }
        }
    }

    /// **The lane scrollbar says where the window stands, and moves it.**
    #[test]
    fn the_lane_scrollbar_shows_the_window_and_carries_the_lanes() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let r = rect();
        let mut v = a_view(&cs);
        v.visible_lanes = CHANNELS;
        assert_eq!(Layout::new(r, &v).lane_bar, LaneBar::NONE, "a desk that fits drew a bar");
        let mut v = a_view(&cs);
        v.visible_lanes = 8;
        let l = Layout::new(r, &v);
        let bar = l.lane_bar;
        assert!(bar.track.is_positive(), "a desk taller than the window drew no bar");
        // Beside the lanes, inside the panel: it was pinned ON the lanes,
        // which is where it took the clips' edge presses (see
        // `the_lane_scrollbar_takes_no_press_from_a_clip_at_the_right_edge`).
        assert!(r.contains_rect(bar.track), "the bar left the panel");
        assert!(bar.track.left() > l.lanes_area.right(), "the bar is on the lanes");
        assert_eq!(bar.max_rows, CHANNELS - 8);
        let share = 8.0 / CHANNELS as f32;
        assert!((bar.thumb.height() - bar.track.height() * share).abs() < 0.5);
        assert!((bar.thumb.top() - bar.track.top()).abs() < 0.5, "unscrolled, not at the top");
        v.lane_scroll = CHANNELS;
        let end = Layout::new(r, &v).lane_bar;
        assert!((end.thumb.bottom() - end.track.bottom()).abs() < 0.5, "not at the bottom");
        // Grab the thumb and drag it past the end: the last window.
        let v = a_view(&cs);
        let v = ArrangeView { visible_lanes: 8, ..v };
        let grab = lane_bar_grab(r, &v, bar.thumb.center()).expect("the thumb took no press");
        let far = Pos2::new(bar.thumb.center().x, r.bottom() + 100.0);
        assert_eq!(lane_bar_scroll(r, &v, far, grab), Some(CHANNELS - 8));
        let near = Pos2::new(bar.thumb.center().x, r.top() - 100.0);
        assert_eq!(lane_bar_scroll(r, &v, near, grab), Some(0));
        // A press in the middle of a lane is never the bar's.
        assert_eq!(lane_bar_grab(r, &v, l.lane_rows[0].center()), None);
    }

    /// **The bar stands beside the lanes and takes none of their presses.**
    /// A clip running off the window's right edge is clamped to the lanes'
    /// right edge, so its right trim zone and its fade-out corner are the
    /// last few points of the lanes. With the bar laid on the lanes, and
    /// claimed before any hit, those points were the bar's: a press to trim
    /// scrolled the lanes instead.
    ///
    /// Revert-check: lay the track back inside `lanes_area.right()` with its
    /// band at `expand2(2, 0)`, and both presses below are the bar's.
    #[test]
    fn the_lane_scrollbar_takes_no_press_from_a_clip_at_the_right_edge() {
        let mut c = clips();
        // Lane 0, running far past the window's end.
        c[0].start_s = 30.0;
        c[0].len_s = 1.0e4;
        c[0].max_len_s = f64::INFINITY;
        let r = rect();
        let v = ArrangeView { visible_lanes: 8, ..a_view(&c) };
        let l = Layout::new(r, &v);
        let bar = l.lane_bar;
        assert!(bar.track.is_positive(), "fixture: a desk taller than the window drew no bar");
        assert!(bar.track.left() > l.lanes_area.right(), "the bar is on the lanes");
        assert!(r.contains_rect(bar.track), "the bar left the panel");
        let cr = clip_rect_in(&l, &v, &c[0]);
        assert!(
            cr.is_positive() && (cr.right() - l.lanes_area.right()).abs() < 0.5,
            "fixture: the clip is not clamped at the lanes' edge ({cr:?})"
        );
        for (p, want) in [
            (Pos2::new(cr.right() - 1.0, cr.center().y), Hit::ClipRight(1)),
            (Pos2::new(cr.right() - 0.5, cr.top() + 2.0), Hit::FadeOut(1)),
        ] {
            assert_eq!(lane_bar_grab(r, &v, p), None, "the bar took {want:?}'s press at {p:?}");
            assert_eq!(hit_test(r, &v, p), Some(want), "at {p:?}");
        }
        // The bar still takes a press anywhere in its gutter, on the thumb
        // or beside it, and a little past the window's edge.
        assert!(lane_bar_grab(r, &v, bar.thumb.center()).is_some(), "the thumb took no press");
        let beside = Pos2::new(bar.track.center().x, bar.track.bottom() - 2.0);
        assert!(!bar.thumb.contains(beside), "fixture: the thumb reaches the bottom");
        assert!(lane_bar_grab(r, &v, beside).is_some(), "the track beside the thumb took no press");
        let past = Pos2::new(r.right() + 1.0, bar.thumb.center().y);
        assert!(lane_bar_grab(r, &v, past).is_some(), "the window's edge took no press");
        // One span of time: the ruler ends where the lanes do, and the
        // frame the app reads on its own agrees with the layout.
        assert_eq!(l.ruler.right(), l.lanes_area.right());
        let f = Layout::frame(r, v.header_w, lane_bar_shown(v.visible_lanes, v.active));
        assert_eq!(f.lanes_area, l.lanes_area);
        // No bar, no gutter: a desk that fits keeps every point.
        let all = ArrangeView { visible_lanes: v.active, ..a_view(&c) };
        assert_eq!(Layout::new(r, &all).lanes_area.right(), r.right());
    }

    /// **A hovering file lights the lane it will land on, scrolled or not.**
    /// `drop_lane` is a ROW; it was read relative to the scroll, so a
    /// scrolled timeline lit the wrong lane, or none.
    #[test]
    fn a_drop_lights_its_own_lane_on_a_scrolled_timeline() {
        let cs: Vec<ClipView<'_>> = Vec::new();
        let r = rect();
        let mut v = a_view(&cs);
        v.visible_lanes = 8;
        v.lane_scroll = 10;
        v.drop_lane = Some(12);
        let l = Layout::new(r, &v);
        assert!(l.lane_rows[12].is_positive(), "lane 12 is not on screen at scroll 10");
        assert_eq!(drop_lane_rect(&l, &v), Some(l.lane_rows[12]), "the drop lit another lane");
        v.drop_lane = Some(2);
        let l = Layout::new(r, &v);
        assert_eq!(drop_lane_rect(&l, &v), None, "a lane scrolled away was lit");
    }
}


#[cfg(test)]
mod miniature_cost_probe {
    use super::tests::{a_view, marks_of, reference_miniature, rect};
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
            muted: false,
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
            muted: false,
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

    /// Every MARK the miniature pass emitted, in draw order — a rect from
    /// the reference walk, a quad out of the drawing's mesh. See
    /// `tests::marks_of`; a count that stopped seeing the marks the moment
    /// they moved into a mesh would have reported the whole cost gone.
    fn rects(ctx: &egui::Context, layer: egui::LayerId) -> Vec<Rect> {
        ctx.graphics(|g| {
            g.get(layer)
                .map(|list| {
                    list.all_entries().flat_map(|c| marks_of(&c.shape)).collect()
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
        // The same part in TIME ORDER, which is how a committed clip holds
        // its notes (the MIDI commit and the recorder both sort by at_s):
        // `a_bar` appends its held notes after the short ones, so `flat` is
        // out of order within every bar and takes the whole walk, while
        // this one is walked only under the window. Both are measured.
        let mut recorded = flat.clone();
        recorded.sort_by(|a, b| a.at_s.total_cmp(&b.at_s));
        for (part, notes, loop_s) in [
            ("a looped clip of music", &looped_music, bar_s),
            ("a looped clip of sub-pixel notes", &looped_short, bar_s),
            ("a clip that does not loop", &flat, 0.0),
            ("a recorded clip in time order", &recorded, 0.0),
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
                peaks: None,
                notes,
                loop_s,
                offset_s: 0.0,
                selected: false,
                midi: true,
                recording: false,
                gain: 1.0,
                marks: &[],
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
