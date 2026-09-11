//! The mixer view: every source in the app, drawn as a channel strip.
//!
//! **The band is the rack and this is the routing**, and that division is why
//! nothing in the band had to move to make room. The six effect knobs stay
//! where a hand reaches for them during a take; what lives here is the thing
//! they never had — who feeds them, how much, and what is heard.
//!
//! A pure painter, like every other module in this crate: [`draw`] takes a
//! rect and a [`MixerView`] and paints; [`hit_test`] is its exact inverse, and
//! [`Layout::targets`] is the single source of truth both are derived from. No
//! state, no `egui::Ui`, and nothing here can open a device or read a file.

use crate::recorder::{gain_text, gain_to_fader, Column, Strip, CHANNELS, INSERTS};
use egui::{Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2};

/// One channel, as the painter sees it.
///
/// **Levels come in as they are, not as decibels.** `gain` is linear and
/// `peak` is a magnitude, because that is what the audio produced; turning
/// them into a position and a number is this module's job and doing it twice,
/// differently, in two places is how a fader and its readout disagree.
pub struct StripView<'a> {
    /// `None` is the master, which has no send and cannot be muted or soloed.
    pub strip: Option<Strip>,
    /// What the channel is called. Empty on an unfilled slot, which is drawn
    /// as somewhere to put an instrument rather than as a channel with no name.
    pub name: &'a str,
    /// The second line under the name: the device, the file, the patch.
    pub detail: &'a str,
    /// A user effect across this strip, by name. Empty for none.
    /// What is in each of this channel's three insert slots, by short name.
    /// Empty for an empty slot, which is drawn as somewhere to put one.
    /// **Six, because the master has two racks.** A channel fills the first
    /// three and leaves the rest empty. See `recorder::seat_bay`, which is
    /// the one place that knows a slot's real seat.
    pub inserts: [&'a str; INSERTS * 2],
    /// How many routes leave this channel, for the strips under the button.
    /// The routes THEMSELVES live on the view, not the strip: one borrow for
    /// the whole desk beats a list per column rebuilt every frame.
    pub route_count: usize,
    /// What feeds a general channel, for its icon. `None` on everything else
    /// and on an unused channel.
    pub kind: Option<crate::recorder::ChannelKind>,
    /// The channel's CHOSEN badge: 0 draws whatever [`auto_icon`] says the
    /// strip is worth; the rest name rows of `recorder_panel::ICONS`.
    pub icon: u8,
    /// Index into [`STRIP_COLORS`]. Zero is the desk's own wood.
    pub color: usize,
    /// An instrument slot with nothing in it.
    ///
    /// **Drawn, not hidden.** Five slots that appear one at a time as they are
    /// filled is a desk that changes shape under your hands; five outlines with
    /// a plus in them is a rack with room in it, and the plus is a second way
    /// into the instrument picker for somebody who is already looking at the
    /// mixer.
    pub empty: bool,
    /// Linear, as the fader sits.
    pub gain: f32,
    /// 0..=1, how much of it reaches the effects bus.
    pub send: f32,
    /// The channel's pan, -1..1, centre 0.
    pub pan: f32,
    /// **Master only**: the output is folded to mono. Meaningless on a
    /// channel and ignored there.
    pub mono: bool,
    /// The loudest thing it made since the last frame, left and right.
    pub peak: [f32; 2],
    /// Whether it is a stereo source. A mono one draws ONE bar, because two
    /// identical bars is a lie about the signal.
    pub stereo: bool,
    /// Decibels the limiter took off, for the master alone. Zero for none.
    pub gr_db: f32,
    pub muted: bool,
    /// **Polarity inverted.** A per-strip switch beside the mute, and the
    /// one thing on a desk that is inaudible alone and decisive in a pair:
    /// two mics on one source out of phase cancel, and this is the fix.
    pub phase: bool,
    pub soloed: bool,
    /// Whether the channel can be taken off the desk — a used general
    /// channel. Decided by the APP; a removable channel wears a small × at
    /// its top right.
    pub removable: bool,
    /// Whether the channel is ARMED — record-ready and LIVE: an armed MIDI
    /// channel sounds when the keys are played, an armed AUDIO channel
    /// monitors its input. Session state, never saved: a project must be
    /// openable without headphones. See docs/ARRANGEMENT.md.
    pub armed: bool,
    /// This channel's MIDI goes to the theory view — the **T** switch.
    pub to_theory: bool,
    /// This channel IS a folder — a group head, wearing six bays like the
    /// master. See `recorder::folder_extra_seat`.
    pub is_folder: bool,
    /// This channel lives inside a folder above it. Drawn a step LOWER —
    /// the mixer's vertical answer to the arrangement's indent.
    pub in_folder: bool,
    /// The folder is shut. Said by the folder glyph's arrow.
    pub folder_shut: bool,
    /// A child of a SHUT folder: the strip collapses out of the row, the
    /// same disappearance the arrangement performs - the owner unified the
    /// two views' folding.
    pub hidden: bool,
    /// Which bays wear a sidechain key, for the dot in the chip's corner.
    /// Same indexing as `inserts`: the strip's visible slots.
    pub keyed: [bool; INSERTS * 2],
    /// **Which bays are BYPASSED** — kept, loaded, editable, and not
    /// applied. Same indexing as `inserts` and `keyed`: the strip's visible
    /// slots, which for the master and a folder span two racks. The caller
    /// resolves that through `recorder::seat_bay` before asking, exactly as
    /// it does for the names.
    ///
    /// Appended after `keyed` and not woven in beside it, for the reason
    /// every list in this app grows at the end: an existing field's meaning
    /// is a thing other code has already been written against.
    pub bypassed: [bool; INSERTS * 2],
}

impl StripView<'_> {
    /// The master and the effects return take no send, and neither does a slot
    /// with nothing in it to send.
    fn sends(&self) -> bool {
        self.strip.is_some_and(Strip::sends) && !self.empty
    }

    /// Whether it has controls at all. An empty slot has one: the plus.
    fn live(&self) -> bool {
        self.strip.is_some() && !self.empty
    }
}

/// Everything the mixer draws, pushed in.
/// Scale a face's channels, keeping its hue — the same trick the band's
/// `shade` uses, so a themed strip darkens into its own colour rather than
/// into grey.
/// Blend two colours, `t` of the way from `a` to `b`.
fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

pub fn shade_face(c: Color32, k: f32) -> Color32 {
    let f = |v: u8| (f32::from(v) * k).clamp(0.0, 255.0) as u8;
    Color32::from_rgb(f(c.r()), f(c.g()), f(c.b()))
}

pub struct MixerView<'a> {
    /// Every channel, then the master last. Fixed, because the desk is fixed:
    /// an app whose strips appear and disappear as things are loaded is a desk
    /// that changes shape under your hands.
    pub strips: [StripView<'a>; COLUMNS],
    /// **What the click is playing**, as a word: the chosen file's name, or
    /// the bundled click's. Drawn on the click's own button, which opens the
    /// metronome panel.
    pub click_sound: &'a str,
    /// How far the CHANNEL region is scrolled, in PIXELS — the desk has more
    /// channels than fit, so a window of them shows and this says where it
    /// stands. Pixels and not a column index, so the wheel moves the desk
    /// the way it moves a web page: continuously, with a half-visible strip
    /// at the edge instead of a jump. The click and the master are pinned at
    /// the right and never scroll. Clamped by `Layout`, so any stale value
    /// lands on the nearest legal position.
    pub scroll_px: f32,
    /// How many channel columns show at once — Shift+wheel zooms it. The
    /// click and the master are pinned beside them and not counted here.
    pub visible: usize,
    /// True when anything at all is soloed, so the strips that are not can be
    /// drawn as what they are — silent, not merely unlit.
    pub any_solo: bool,
    /// How many channels this install works at — the paywall's number
    /// (`TangentApp::active_channels`). Capacity stays [`CHANNELS`]; this
    /// bounds what the desk DRAWS and how far it scrolls, so a free
    /// install neither shows nor reaches the supporter lanes.
    pub active: usize,
    pub dark_mode: bool,
    /// Every route on the desk: source channel, label, fader. Each strip
    /// draws the ones that leave it, in order.
    pub routes: &'a [(usize, String, f32)],
    /// Theme roles the desk paints with.
    pub fx_strip: Color32,
    pub input_chip: Color32,
    /// The band's own wood, so the two surfaces are one instrument.
    pub wood: (u8, u8, u8),
    /// A decibel figure being typed, and into which strip.
    ///
    /// **A fader is a bad way to ask for exactly -6.0.** Its whole range is
    /// eighty decibels; the drag was slowed until it was usable, and this is
    /// the other half of that — click the number and say it.
    pub typing: Option<(Column, &'a str)>,
    /// The channel whose NAME is being typed, and what is in the field.
    pub naming: Option<(Column, &'a str)>,
    /// A loaded insert bay whose two-entry menu is open: delete or replace.
    ///
    /// **A right-click used to unload the bay outright.** One gesture, no
    /// confirmation, aimed at a chip twelve points tall — which is how a
    /// plugin somebody had spent an evening dialling in went away by accident.
    /// The menu is the palette's own pattern: drawn over the strip in the
    /// same frame, hit-tested first, dismissed by a press anywhere else.
    pub fx_menu: Option<(Column, usize)>,
    /// The selected channel: its strip wears a faint light rim.
    ///
    /// The PRIMARY — the last one clicked. Everything selected is in
    /// [`Self::selected_tracks`], and that is what the rim is drawn from.
    pub selected: Option<usize>,
    /// Every selected channel. Shift takes a range, Cmd or Ctrl takes them one
    /// at a time; all of them wear the rim, because a selection you cannot see
    /// the extent of is not one.
    pub selected_tracks: &'a [usize],
    /// Whether the OPEN insert menu's bay can take a sidechain key, which is
    /// what earns it the third row. On the view rather than per strip
    /// because at most one menu is up, and the app is the side that can ask
    /// the host's pushed facts about the bay under it.
    pub fx_menu_key: bool,
    /// Whether the OPEN insert menu's bay has anything IN it, which is what
    /// earns it every row but the paste. An empty bay's menu is one row long
    /// and exists only because something is on the effect clipboard.
    pub fx_menu_loaded: bool,
    /// Whether an effect is held for pasting, which is what earns the open
    /// menu its PASTE FX row. On the view for `fx_menu_key`'s reason: at
    /// most one menu is up, and the app is the side that knows.
    pub fx_menu_paste: bool,
}

impl MixerView<'_> {
    /// Whether a strip is heard right now, which is what dims it.
    pub fn heard(&self, at: usize) -> bool {
        let Some(s) = self.strips.get(at) else {
            return true;
        };
        if !s.live() {
            return true;
        }
        if self.any_solo {
            return s.soloed;
        }
        !s.muted
    }
}

/// What a press on the mixer means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Hit {
    /// The fader of the strip drawn in this column.
    Fader(Column),
    /// The send knob of the strip drawn in this column.
    Send(Column),
    /// The pan knob — general channels only, -1..1 carried as 0..1.
    Pan(Column),
    /// The input chip under the icon: open the channel's INPUT menu.
    InputChip(Column),
    /// The click's sound button: open the metronome panel.
    MetFile,
    Mute(Column),
    /// **Ø** — invert this strip's polarity.
    Phase(Column),
    Solo(Column),
    /// **What is on this channel**: the plus on an empty slot, and the icon
    /// above the name on a filled one. Both open the same picker — the
    /// interface for an input, the instrument for a slot, the file for the
    /// backing track — because both ask the same question.
    Add(Column),
    /// **T** — send this channel's MIDI to the theory view, or stop.
    ToTheory(Column),
    /// **Master only**: fold the output to mono, or stop. A mix check.
    Mono(Column),
    /// One of the three insert chips: put an effect in it, or take one out.
    /// The second field is the BAY, which is not a channel of anything.
    Insert(Column, usize),
    /// The Routing button under the insert bays: open the routing window.
    Routing(Column),
    /// One route strip's fader. The second field is which route, counting
    /// from the top of THIS channel's list — not an index into the desk's
    /// table, which the panel has no business knowing.
    RouteGain(Column, usize),
    /// The decibel readout: type a number into it.
    Db(Column),
    /// The channel's name: type a new one.
    ///
    /// **On an INPUT this renames the interface, not the channel.** Every
    /// input of one box shares the half of the label worth shortening, so
    /// typing "x" over "Scarlett 18i20 USB - 3" makes all of them "x - N" at
    /// once — which is the only rename that fits and the only one anybody
    /// wants to type more than once.
    Name(Column),
    /// The × at the top right of a removable channel: take it off the desk.
    Remove(Column),
    /// The R switch: arm the channel — record-ready, and LIVE. The third of
    /// the M S R row, on general channels alone.
    Arm(Column),
    /// The F switch, leftmost of the row: CYCLE the channel's folder state
    /// — not a folder, an open folder, a shut folder, and around. The
    /// owner's one-button answer to the arrangement's box-and-arrow pair.
    Folder(Column),
    /// One of the two rows of an open insert menu — or, with
    /// [`FxAction::Dismiss`], a press anywhere else while it is up.
    FxMenu(Column, usize, FxAction),
}

/// What a press on an open insert menu means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FxAction {
    /// Take the plugin out of the bay.
    Delete,
    /// Open the picker aimed at this bay, so choosing swaps it in place.
    Replace,
    /// **Hold this bay's effect**, state and all, for a later `Paste`.
    ///
    /// The road a DRAG stopped offering. A carry inside one strip is a MOVE
    /// now (the owner's "fx inserts 1-3 can drag between themselves AND
    /// between 4-6"), and a move is not a copy — so duplicating an effect
    /// into another bay of the same track had no gesture left. "To overcome
    /// the copy paste issue this creates, add copy FX and paste FX to right
    /// click menu for FX inserts."
    Copy,
    /// **Put the held effect in this bay**, over whatever is there. Offered
    /// only when something is held — see `MixerView::fx_menu_paste`.
    Paste,
    /// Open the sidechain source picker on this bay. Offered only when the
    /// view says the tenant can take a key — see `MixerView::fx_menu_key`.
    Sidechain,
    /// Anywhere that is not a row: close the menu and do nothing.
    Dismiss,
}

impl FxAction {
    /// The row's word on the DESK, where a strip is ninety points wide and
    /// everything printed on it is capitals.
    pub fn word(self) -> &'static str {
        match self {
            FxAction::Delete => "DELETE FX",
            FxAction::Replace => "REPLACE FX",
            FxAction::Copy => "COPY FX",
            FxAction::Paste => "PASTE FX",
            FxAction::Sidechain => "SIDECHAIN...",
            FxAction::Dismiss => "",
        }
    }

    /// The row's word in the ARRANGEMENT's floating menu, which is a list of
    /// sentences beside the clip menu's and is written like one.
    pub fn label(self) -> &'static str {
        match self {
            FxAction::Delete => "Delete FX",
            FxAction::Replace => "Replace FX",
            FxAction::Copy => "Copy FX",
            FxAction::Paste => "Paste FX",
            FxAction::Sidechain => "Sidechain...",
            FxAction::Dismiss => "",
        }
    }
}

/// **The rows an open bay menu offers, in order.**
///
/// One list, and both surfaces read it: the desk paints these rows into the
/// strip and the timeline paints them into a floating menu, and a row that
/// existed on one and not the other would be a gesture that works in one
/// view and quietly does something else in the other. It is also what makes
/// the hit test and the painter twins — the actions are POSITIONAL, so a
/// conditional row must not be able to re-point the rows above it.
///
/// An EMPTY bay gets a menu at all only for the paste: there is nothing to
/// delete, replace, copy or key.
pub fn fx_menu_actions(loaded: bool, key: bool, paste: bool) -> Vec<FxAction> {
    let mut out = Vec::with_capacity(5);
    if loaded {
        out.push(FxAction::Delete);
        out.push(FxAction::Replace);
        out.push(FxAction::Copy);
    }
    if paste {
        out.push(FxAction::Paste);
    }
    if loaded && key {
        out.push(FxAction::Sidechain);
    }
    out
}

impl Hit {
    /// Which column it is on. Every one of them is on exactly one.
    pub fn column(self) -> Column {
        match self {
            Hit::Fader(i)
            | Hit::Send(i)
            | Hit::Pan(i)
            | Hit::Mute(i)
            | Hit::Phase(i)
            | Hit::Solo(i)
            | Hit::Add(i)
            | Hit::Insert(i, _)
            | Hit::Db(i)
            | Hit::Name(i)
            | Hit::Remove(i)
            | Hit::Arm(i)
            | Hit::Folder(i)
            | Hit::ToTheory(i)
            | Hit::Mono(i)
            | Hit::FxMenu(i, ..)
            | Hit::InputChip(i)
            | Hit::Routing(i)
            | Hit::RouteGain(i, _) => i,
            // The click's sound button is on the click's column, which is
            // where it is drawn.
            Hit::MetFile => Column(Strip::Click.index()),
        }
    }

    /// Whether it travels under the hand, and along which axis.
    ///
    /// The same contract the band's controls use, and deliberately the same
    /// feel: relative, so a press never makes the handle jump, and the hand
    /// can leave the control and keep going.
    pub fn axis(self) -> Option<DragAxis> {
        match self {
            Hit::Fader(_) => Some(DragAxis::Vertical),
            Hit::Send(_) => Some(DragAxis::Vertical),
            Hit::Pan(_) => Some(DragAxis::Vertical),
            // **A route strip drags SIDEWAYS.** It is drawn as a level lying
            // down, and a control that fills to the right has to be swept to
            // the right or the picture and the hand disagree.
            Hit::RouteGain(..) => Some(DragAxis::Horizontal),
            Hit::Mute(_) | Hit::Phase(_) | Hit::Solo(_) | Hit::Add(_) | Hit::Insert(..)
            | Hit::Db(_) | Hit::Name(_) | Hit::Remove(_) | Hit::Arm(_) | Hit::Folder(_)
            | Hit::ToTheory(_) | Hit::Mono(_) | Hit::FxMenu(..) | Hit::InputChip(_)
            | Hit::MetFile
            | Hit::Routing(_) => None,
        }
    }

    /// How far the pointer must move to sweep it end to end, in points.
    pub fn travel(self) -> Option<f32> {
        match self {
            Hit::Fader(_) => Some(FADER_TRAVEL),
            Hit::Send(_) => Some(SEND_TRAVEL),
            Hit::Pan(_) => Some(SEND_TRAVEL),
            Hit::RouteGain(..) => Some(SEND_TRAVEL),
            Hit::Mute(_) | Hit::Phase(_) | Hit::Solo(_) | Hit::Add(_) | Hit::Insert(..)
            | Hit::Db(_) | Hit::Name(_) | Hit::Remove(_) | Hit::Arm(_) | Hit::Folder(_)
            | Hit::ToTheory(_) | Hit::Mono(_) | Hit::FxMenu(..) | Hit::InputChip(_)
            | Hit::MetFile
            | Hit::Routing(_) => None,
        }
    }
}

/// What a channel can be painted.
///
/// **A row per hue, three tones across.** Eight colours were enough to tell
/// eight channels apart and not enough to tell them apart the way somebody
/// wants to — two quiet blues for two microphones, a loud red for the one
/// thing that must not be missed. So: nine rows of three.
///
/// The first row is the neutrals, and index 0 is the band's own wood — a
/// channel nobody has coloured is not a colour choice at all. Nearly every
/// value is dark enough to carry the face's text, because this paints the
/// whole strip and not a dot on it; the labels sit on plates of their own for
/// the same reason. NEARLY: the third tones of olive, green and teal sit in
/// the dead zone where neither ink clears 4.5:1, which is why every surface
/// painted from this table goes through `legible_surface` on its way to the
/// screen — the indices are saved in desks and cannot move, so the paint
/// deepens or lifts at draw time instead.
///
/// [`PALETTE_COLUMNS`] is what makes the grid read as hues rather than as
/// twenty-seven swatches: reading down a column is one tone, across a row is
/// one colour.
pub const STRIP_COLORS: [(u8, u8, u8); 708] = [
    (0x00, 0x00, 0x00), // the desk's own wood — replaced at draw time
    (0x36, 0x33, 0x30), // slate
    (0x56, 0x51, 0x4D), // stone
    (0x3F, 0x12, 0x15), (0x63, 0x17, 0x1C), (0x86, 0x27, 0x2D), // red
    (0x3F, 0x27, 0x12), (0x63, 0x39, 0x17), (0x86, 0x52, 0x27), // amber
    (0x3F, 0x3B, 0x12), (0x63, 0x5C, 0x17), (0x86, 0x7D, 0x27), // olive
    (0x21, 0x3F, 0x12), (0x30, 0x63, 0x17), (0x46, 0x86, 0x27), // green
    (0x12, 0x3A, 0x3F), (0x17, 0x5A, 0x63), (0x27, 0x7B, 0x86), // teal
    (0x12, 0x24, 0x3F), (0x17, 0x36, 0x63), (0x27, 0x4D, 0x86), // blue
    (0x29, 0x12, 0x3F), (0x3D, 0x17, 0x63), (0x57, 0x27, 0x86), // violet
    (0x3F, 0x12, 0x33), (0x63, 0x17, 0x4E), (0x86, 0x27, 0x6C), // magenta
    // ── the auto-palette colours, past the hand-picking grid ──────────────
    //
    // **Generated, not eyeballed** — `scripts/palgen.py` solves each one for
    // a target luminance under a chroma ceiling, because the two things that
    // went wrong when these were picked by hand are exactly the two things a
    // person cannot do by eye. Every colour here is dark enough that the
    // ivory ink wins on it at 4.5:1 or better, and no more colourful than its
    // neighbours — equal luminance is NOT equal vividness, and a violet
    // forced onto an ochre's luminance climbs to a neon #7736c1 to get there.
    //
    // They are APPENDED, and appending is the whole trick: every index
    // already written into somebody's saved desk still means the colour it
    // meant. See `PALETTE_V1_TO_V2` for what renumbering this table costs.
    (0xb0, 0x20, 0x33), // 27  the master, shared by every palette
    // Default: channels 28..43, click 44
    (0x76, 0x38, 0x21), (0x2b, 0x21, 0x0c), (0x43, 0x4e, 0x16), (0x0f, 0x27, 0x0b),
    (0x17, 0x52, 0x47), (0x10, 0x23, 0x38), (0x59, 0x3d, 0x79), (0x3d, 0x11, 0x2f),
    (0x8e, 0x47, 0x2d), (0x45, 0x34, 0x13), (0x53, 0x60, 0x1b), (0x18, 0x3f, 0x12),
    (0x1c, 0x65, 0x57), (0x19, 0x38, 0x5b), (0x6b, 0x4d, 0x8e), (0x60, 0x1b, 0x49),
    (0x3f, 0x34, 0x31),
    // Valentine: channels 45..60, click 61
    (0x7f, 0x29, 0x54), (0x3a, 0x10, 0x3a), (0x84, 0x2a, 0x39), (0x2d, 0x15, 0x4a),
    (0x71, 0x3b, 0x20), (0x0b, 0x26, 0x1d), (0x5d, 0x47, 0x1a), (0x37, 0x11, 0x3e),
    (0x92, 0x3d, 0x67), (0x57, 0x1f, 0x57), (0x97, 0x3c, 0x4c), (0x45, 0x29, 0x65),
    (0x8b, 0x49, 0x27), (0x11, 0x3e, 0x2f), (0x72, 0x56, 0x20), (0x54, 0x22, 0x5d),
    (0x41, 0x33, 0x3a),
    // Hydra: channels 62..77, click 78
    (0x17, 0x52, 0x4b), (0x0d, 0x25, 0x2d), (0x17, 0x54, 0x36), (0x29, 0x22, 0x0b),
    (0x82, 0x2a, 0x47), (0x10, 0x23, 0x37), (0x1b, 0x54, 0x1b), (0x29, 0x16, 0x4e),
    (0x1c, 0x65, 0x5c), (0x15, 0x3b, 0x4a), (0x1d, 0x67, 0x42), (0x41, 0x36, 0x12),
    (0x96, 0x3c, 0x5a), (0x19, 0x38, 0x58), (0x2d, 0x66, 0x2d), (0x3f, 0x2b, 0x68),
    (0x2d, 0x39, 0x38),
    // Navi: channels 79..94, click 95
    (0x1d, 0x4f, 0x67), (0x12, 0x21, 0x40), (0x17, 0x52, 0x51), (0x30, 0x1e, 0x0d),
    (0x4b, 0x40, 0x81), (0x0b, 0x26, 0x1d), (0x6e, 0x33, 0x6e), (0x14, 0x26, 0x0b),
    (0x24, 0x61, 0x7f), (0x1d, 0x35, 0x67), (0x1c, 0x64, 0x62), (0x4c, 0x31, 0x15),
    (0x5d, 0x52, 0x92), (0x11, 0x3e, 0x2f), (0x7f, 0x45, 0x7f), (0x20, 0x3e, 0x11),
    (0x30, 0x39, 0x3d),
    // Mono: channels 96..111, click 112
    (0x14, 0x11, 0x0f), (0x1a, 0x1f, 0x24), (0x2a, 0x26, 0x20), (0x27, 0x2d, 0x36),
    (0x38, 0x32, 0x2b), (0x31, 0x3a, 0x44), (0x43, 0x3c, 0x33), (0x39, 0x43, 0x4f),
    (0x4d, 0x45, 0x3b), (0x40, 0x4b, 0x59), (0x55, 0x4c, 0x41), (0x46, 0x51, 0x61),
    (0x5c, 0x52, 0x46), (0x4b, 0x58, 0x68), (0x63, 0x58, 0x4b), (0x50, 0x5d, 0x6f),
    (0x42, 0x35, 0x1e),
    // Studio: channels 113..128, click 129
    (0x66, 0x41, 0x1d), (0x29, 0x21, 0x0c), (0x74, 0x39, 0x20), (0x23, 0x23, 0x0a),
    (0x7f, 0x30, 0x24), (0x18, 0x25, 0x0a), (0x82, 0x2a, 0x47), (0x0d, 0x25, 0x30),
    (0x7e, 0x51, 0x23), (0x43, 0x35, 0x13), (0x8d, 0x48, 0x2a), (0x38, 0x38, 0x10),
    (0x94, 0x42, 0x35), (0x27, 0x3c, 0x11), (0x96, 0x3c, 0x5a), (0x16, 0x3b, 0x4d),
    (0x3c, 0x36, 0x2f),
    // Aurora: channels 130..145, click 146
    (0x17, 0x53, 0x3f), (0x0d, 0x25, 0x2d), (0x3f, 0x44, 0x82), (0x31, 0x14, 0x46),
    (0x7f, 0x29, 0x54), (0x0b, 0x27, 0x10), (0x50, 0x4c, 0x16), (0x3a, 0x10, 0x3a),
    (0x1d, 0x66, 0x4e), (0x14, 0x3c, 0x49), (0x4f, 0x55, 0x98), (0x4a, 0x27, 0x62),
    (0x92, 0x3d, 0x67), (0x12, 0x3f, 0x19), (0x62, 0x5d, 0x1c), (0x57, 0x1f, 0x57),
    (0x2d, 0x39, 0x35),
    // Sandstone: channels 147..162, click 163
    (0x6f, 0x3d, 0x1f), (0x2c, 0x20, 0x0c), (0x54, 0x49, 0x17), (0x1b, 0x25, 0x0a),
    (0x7f, 0x30, 0x24), (0x0b, 0x27, 0x0b), (0x82, 0x2a, 0x47), (0x0d, 0x25, 0x30),
    (0x89, 0x4a, 0x26), (0x48, 0x33, 0x14), (0x67, 0x5b, 0x1d), (0x2b, 0x3c, 0x11),
    (0x94, 0x42, 0x35), (0x12, 0x3f, 0x12), (0x96, 0x3c, 0x5a), (0x16, 0x3b, 0x4d),
    (0x3e, 0x36, 0x30),
    // Centennial: channels 164..179, click 180
    (0x58, 0x49, 0x19), (0x30, 0x1e, 0x0d), (0x21, 0x4c, 0x76), (0x29, 0x16, 0x4e),
    (0x7f, 0x30, 0x24), (0x18, 0x25, 0x0a), (0x78, 0x2e, 0x60), (0x0b, 0x26, 0x22),
    (0x6c, 0x59, 0x1e), (0x4c, 0x31, 0x15), (0x29, 0x5d, 0x92), (0x3f, 0x2b, 0x68),
    (0x94, 0x42, 0x35), (0x27, 0x3c, 0x11), (0x8c, 0x40, 0x72), (0x11, 0x3d, 0x36),
    (0x3a, 0x36, 0x2d),
    // Blueprint: channels 181..196, click 197
    (0x21, 0x4c, 0x76), (0x13, 0x20, 0x44), (0x1a, 0x51, 0x5b), (0x22, 0x18, 0x51),
    (0x6c, 0x3e, 0x1e), (0x0b, 0x26, 0x1d), (0x6e, 0x33, 0x6e), (0x0b, 0x27, 0x0b),
    (0x29, 0x5d, 0x92), (0x1f, 0x33, 0x6e), (0x1f, 0x63, 0x70), (0x38, 0x2d, 0x6b),
    (0x85, 0x4d, 0x25), (0x11, 0x3e, 0x2f), (0x7f, 0x45, 0x7f), (0x12, 0x3f, 0x12),
    (0x31, 0x37, 0x3e),
    // Porcelain: channels 198..213, click 214
    (0x21, 0x4c, 0x76), (0x15, 0x1e, 0x4b), (0x1a, 0x51, 0x5b), (0x29, 0x16, 0x4e),
    (0x77, 0x37, 0x21), (0x0b, 0x27, 0x19), (0x78, 0x2e, 0x60), (0x14, 0x26, 0x0b),
    (0x29, 0x5d, 0x92), (0x25, 0x32, 0x6e), (0x1f, 0x63, 0x70), (0x3f, 0x2b, 0x68),
    (0x8f, 0x46, 0x2e), (0x11, 0x3e, 0x28), (0x8c, 0x40, 0x72), (0x20, 0x3e, 0x11),
    (0x31, 0x37, 0x3e),
    // Moss: channels 215..230, click 231
    (0x30, 0x52, 0x17), (0x1d, 0x24, 0x0a), (0x18, 0x54, 0x22), (0x0b, 0x26, 0x1d),
    (0x58, 0x49, 0x19), (0x0d, 0x25, 0x30), (0x71, 0x3b, 0x20), (0x34, 0x13, 0x44),
    (0x3e, 0x64, 0x22), (0x2f, 0x3b, 0x11), (0x25, 0x67, 0x30), (0x11, 0x3e, 0x2f),
    (0x6c, 0x59, 0x1e), (0x16, 0x3b, 0x4d), (0x8b, 0x49, 0x27), (0x4d, 0x25, 0x62),
    (0x32, 0x39, 0x2d),
    // Rhodes: channels 232..247, click 248
    (0x61, 0x44, 0x1b), (0x34, 0x1c, 0x0f), (0x55, 0x49, 0x18), (0x3c, 0x17, 0x11),
    (0x41, 0x4f, 0x16), (0x0d, 0x25, 0x30), (0x7f, 0x29, 0x54), (0x29, 0x16, 0x4e),
    (0x77, 0x54, 0x21), (0x53, 0x2d, 0x17), (0x69, 0x5a, 0x1d), (0x5f, 0x24, 0x1b),
    (0x50, 0x61, 0x1b), (0x16, 0x3b, 0x4d), (0x92, 0x3d, 0x67), (0x3f, 0x2b, 0x68),
    (0x3c, 0x36, 0x2f),
    // Copper: channels 249..264, click 265
    (0x6c, 0x3e, 0x1e), (0x0b, 0x26, 0x23), (0x5d, 0x47, 0x1a), (0x0c, 0x26, 0x2b),
    (0x7f, 0x30, 0x24), (0x0b, 0x27, 0x19), (0x6e, 0x33, 0x6e), (0x14, 0x26, 0x0b),
    (0x85, 0x4d, 0x25), (0x11, 0x3d, 0x38), (0x72, 0x56, 0x20), (0x13, 0x3d, 0x45),
    (0x94, 0x42, 0x35), (0x11, 0x3e, 0x28), (0x7f, 0x45, 0x7f), (0x20, 0x3e, 0x11),
    (0x3d, 0x36, 0x30),
    // Dusk: channels 266..281, click 282
    (0x5f, 0x3a, 0x79), (0x3a, 0x10, 0x3a), (0x6c, 0x3e, 0x1e), (0x1a, 0x1a, 0x55),
    (0x7f, 0x29, 0x54), (0x0d, 0x25, 0x30), (0x50, 0x4c, 0x16), (0x0b, 0x26, 0x1d),
    (0x71, 0x4b, 0x8b), (0x57, 0x1f, 0x57), (0x85, 0x4d, 0x25), (0x2f, 0x2f, 0x6c),
    (0x92, 0x3d, 0x67), (0x16, 0x3b, 0x4d), (0x62, 0x5d, 0x1c), (0x11, 0x3e, 0x2f),
    (0x3c, 0x33, 0x42),
    // Iris: channels 283..298, click 299
    (0x62, 0x39, 0x76), (0x3a, 0x10, 0x3a), (0x41, 0x4f, 0x16), (0x22, 0x18, 0x51),
    (0x7f, 0x29, 0x54), (0x0d, 0x25, 0x30), (0x58, 0x49, 0x19), (0x0b, 0x26, 0x1d),
    (0x74, 0x4a, 0x89), (0x57, 0x1f, 0x57), (0x50, 0x61, 0x1b), (0x38, 0x2d, 0x6b),
    (0x92, 0x3d, 0x67), (0x16, 0x3b, 0x4d), (0x6c, 0x59, 0x1e), (0x11, 0x3e, 0x2f),
    (0x3d, 0x33, 0x42),
    // ── each palette's own MASTER, appended 2026-08-30 ────────────────────
    //
    // The master used to be one crimson (index 27) shared by all sixteen, on
    // the argument that it is the strip you go looking for and should not
    // move. The owner asked for the opposite — "a true color picked per
    // theme freshly ... for the master lane strip" — so each palette names
    // its own, and index 27 stays exactly what it was for every desk that
    // already points at it.
    //
    // Generated against three floors rather than eyeballed: the ivory ink
    // wins on each at 4.5:1 (the rule every colour in this table follows),
    // each is at least 22 ΔE from its OWN palette's sixteen channels and its
    // click — a master that reads as track seventeen is no master — and they
    // are spread from one another, because sixteen palettes derived from
    // sixteen themes otherwise converge on the same brick red. That last one
    // is why several of these are not the theme's accent: a palette already
    // CONTAINS its theme's hues, so the accent collides with the tracks by
    // construction and the honest answer is the nearest colour that does not.
    (0xB0, 0x2A, 0x37), // 300  Default
    (0xbd, 0x0f, 0x58), // 301  Valentine
    (0x81, 0x5a, 0x0e), // 302  Hydra
    (0x91, 0x4c, 0x08), // 303  Navi
    (0xc2, 0x0a, 0x0a), // 304  Mono
    (0x34, 0x6e, 0x0c), // 305  Studio
    (0xc2, 0x0a, 0x3a), // 306  Aurora
    (0xa5, 0x39, 0x09), // 307  Sandstone
    (0x24, 0x6b, 0x34), // 308  Centennial
    (0x9b, 0x34, 0x27), // 309  Blueprint
    (0xa5, 0x16, 0x09), // 310  Porcelain
    (0xb8, 0x2d, 0x0a), // 311  Moss
    (0x24, 0x60, 0x48), // 312  Rhodes
    (0xaa, 0x2f, 0x18), // 313  Copper
    (0xa0, 0x22, 0x3d), // 314  Dusk
    (0x09, 0x71, 0x2c), // 315  Iris
    // ── the dark batch, APPENDED (the rule this table lives by): three
    // dark factory themes' runs, then their masters. The first sixteen
    // palettes' bases follow the 28+17i formula; these start at 316 and
    // their masters sit after the last run rather than in the 300 block,
    // because that block was full the day it shipped and indices already
    // live in saved desks. Designed and gated by scripts/palgen.py --dark.
    //
    // **Carbon is RETIRED** (2026-09-02, the owner: "a worse version of
    // Studio") and its four blocks — this run, master 367, ext 498..505,
    // ext2 650..657 — stay exactly where they are. Every index here was
    // written into somebody's desk the moment a track was painted, so a
    // retired run is table history the way the eight abstract palettes
    // are: reachable by a saved colour, offered by no menu. Out of
    // `PALETTES`, in the table, still verified byte for byte by
    // scripts/palgen.py --check.
    // Carbon: channels 316..331, click 332
    (0x38, 0x19, 0x11), (0x48, 0x34, 0x15), (0x4d, 0x4b, 0x17), (0x49, 0x62, 0x1e),
    (0x5a, 0x28, 0x1b), (0x5f, 0x45, 0x1d), (0x5f, 0x5d, 0x1d), (0x1c, 0x25, 0x0b),
    (0x79, 0x35, 0x24), (0x75, 0x54, 0x23), (0x23, 0x23, 0x0b), (0x2c, 0x3b, 0x12),
    (0x91, 0x45, 0x31), (0x2c, 0x20, 0x0d), (0x3a, 0x38, 0x11), (0x3c, 0x50, 0x18),
    (0x2e, 0x2b, 0x26),
    // Nocturne: channels 333..348, click 349
    (0x10, 0x23, 0x38), (0x2a, 0x31, 0x6e), (0x52, 0x3f, 0x7d), (0x1f, 0x63, 0x70),
    (0x19, 0x38, 0x5b), (0x3d, 0x44, 0x85), (0x63, 0x50, 0x90), (0x0c, 0x26, 0x2b),
    (0x22, 0x4b, 0x7a), (0x4e, 0x56, 0x97), (0x27, 0x16, 0x50), (0x13, 0x3d, 0x45),
    (0x2a, 0x5c, 0x96), (0x17, 0x1c, 0x50), (0x3e, 0x2c, 0x68), (0x1a, 0x51, 0x5b),
    (0x3a, 0x33, 0x28),
    // Phosphor: channels 350..365, click 366
    (0x16, 0x26, 0x0c), (0x32, 0x3a, 0x12), (0x19, 0x54, 0x25), (0x6b, 0x59, 0x20),
    (0x24, 0x3d, 0x12), (0x44, 0x4e, 0x18), (0x23, 0x67, 0x31), (0x28, 0x22, 0x0c),
    (0x30, 0x52, 0x19), (0x53, 0x60, 0x1d), (0x0c, 0x27, 0x11), (0x40, 0x36, 0x13),
    (0x3d, 0x64, 0x22), (0x1f, 0x24, 0x0b), (0x13, 0x3f, 0x1c), (0x57, 0x48, 0x1a),
    (0x26, 0x2c, 0x22),
    (0xb6, 0x0f, 0x25), // 367  Carbon
    (0x86, 0x4d, 0x14), // 368  Nocturne
    (0x8e, 0x21, 0x0b), // 369  Phosphor
    // ── the EXTENSION runs, APPENDED (the rule): channels 17..24 for
    // every palette, eight per, grown from each run's own hues two tones
    // away so the new lanes read as family without twinning 1..8. Bases
    // ride in Palette::ext. Designed and gated by scripts/palgen.py.
    // Default ext: channels 17..24 at 370..377
    (0x37, 0x1a, 0x10), (0x5c, 0x46, 0x1a), (0x1f, 0x24, 0x0a), (0x20, 0x54, 0x18),
    (0x0b, 0x26, 0x21), (0x23, 0x4c, 0x79), (0x2a, 0x1c, 0x38), (0x78, 0x2e, 0x61),
    // Valentine ext: channels 17..24 at 378..385
    (0x3c, 0x13, 0x28), (0x6c, 0x34, 0x6c), (0x3d, 0x14, 0x1b), (0x59, 0x3c, 0x7b),
    (0x35, 0x1b, 0x0f), (0x18, 0x53, 0x40), (0x2b, 0x20, 0x0c), (0x69, 0x36, 0x72),
    // Hydra ext: channels 17..24 at 386..393
    (0x0b, 0x26, 0x23), (0x1c, 0x50, 0x61), (0x0b, 0x27, 0x19), (0x57, 0x49, 0x17),
    (0x3c, 0x14, 0x21), (0x23, 0x4b, 0x77), (0x0d, 0x27, 0x0d), (0x54, 0x3f, 0x7c),
    // Navi ext: channels 17..24 at 394..401
    (0x0d, 0x24, 0x30), (0x29, 0x48, 0x87), (0x0b, 0x26, 0x26), (0x67, 0x41, 0x1c),
    (0x23, 0x1d, 0x3b), (0x18, 0x53, 0x40), (0x33, 0x18, 0x33), (0x2c, 0x53, 0x18),
    // Mono ext: channels 17..24 at 402..409
    (0x54, 0x47, 0x3f), (0x3f, 0x4c, 0x58), (0x50, 0x49, 0x3d), (0x41, 0x4b, 0x59),
    (0x64, 0x59, 0x4d), (0x4e, 0x5c, 0x6c), (0x64, 0x59, 0x4c), (0x1d, 0x22, 0x29),
    // Studio ext: channels 17..24 at 410..417
    (0x30, 0x1e, 0x0e), (0x59, 0x47, 0x1a), (0x36, 0x1a, 0x0f), (0x4c, 0x4c, 0x16),
    (0x3c, 0x16, 0x11), (0x35, 0x51, 0x16), (0x3c, 0x14, 0x21), (0x1c, 0x4f, 0x66),
    // Aurora ext: channels 17..24 at 418..425
    (0x0b, 0x26, 0x1d), (0x1c, 0x50, 0x61), (0x1d, 0x20, 0x3c), (0x5e, 0x3a, 0x78),
    (0x3c, 0x13, 0x28), (0x18, 0x54, 0x23), (0x24, 0x23, 0x0a), (0x6c, 0x34, 0x6c),
    // Sandstone ext: channels 17..24 at 426..433
    (0x34, 0x1d, 0x0e), (0x5f, 0x45, 0x1a), (0x27, 0x22, 0x0b), (0x3b, 0x50, 0x16),
    (0x3c, 0x16, 0x11), (0x1b, 0x54, 0x1b), (0x3c, 0x14, 0x21), (0x1c, 0x4f, 0x66),
    // Centennial ext: channels 17..24 at 434..441
    (0x28, 0x21, 0x0b), (0x67, 0x41, 0x1c), (0x0f, 0x24, 0x37), (0x54, 0x3f, 0x7c),
    (0x3c, 0x16, 0x11), (0x35, 0x51, 0x16), (0x38, 0x15, 0x2d), (0x18, 0x52, 0x4a),
    // Blueprint ext: channels 17..24 at 442..449
    (0x0f, 0x24, 0x37), (0x30, 0x47, 0x85), (0x0c, 0x26, 0x2a), (0x4c, 0x41, 0x7e),
    (0x32, 0x1d, 0x0e), (0x18, 0x53, 0x40), (0x33, 0x18, 0x33), (0x1b, 0x54, 0x1b),
    // Porcelain ext: channels 17..24 at 450..457
    (0x0f, 0x24, 0x37), (0x38, 0x45, 0x86), (0x0c, 0x26, 0x2a), (0x54, 0x3f, 0x7c),
    (0x37, 0x1a, 0x0f), (0x18, 0x53, 0x36), (0x38, 0x15, 0x2d), (0x2c, 0x53, 0x18),
    // Moss ext: channels 17..24 at 458..465
    (0x17, 0x26, 0x0b), (0x40, 0x4f, 0x16), (0x0b, 0x27, 0x10), (0x18, 0x53, 0x40),
    (0x28, 0x21, 0x0b), (0x1c, 0x4f, 0x66), (0x35, 0x1b, 0x0f), (0x62, 0x39, 0x76),
    // Rhodes ext: channels 17..24 at 466..473
    (0x2d, 0x20, 0x0d), (0x70, 0x3d, 0x20), (0x27, 0x22, 0x0b), (0x7f, 0x30, 0x24),
    (0x1e, 0x24, 0x0a), (0x1c, 0x4f, 0x66), (0x3c, 0x13, 0x28), (0x54, 0x3f, 0x7c),
    // Copper ext: channels 17..24 at 474..481
    (0x32, 0x1d, 0x0e), (0x18, 0x52, 0x4c), (0x2b, 0x20, 0x0c), (0x19, 0x51, 0x5b),
    (0x3c, 0x16, 0x11), (0x18, 0x53, 0x36), (0x33, 0x18, 0x33), (0x2c, 0x53, 0x18),
    // Dusk ext: channels 17..24 at 482..489
    (0x2c, 0x1b, 0x37), (0x6c, 0x34, 0x6c), (0x32, 0x1d, 0x0e), (0x42, 0x42, 0x83),
    (0x3c, 0x13, 0x28), (0x1c, 0x4f, 0x66), (0x24, 0x23, 0x0a), (0x18, 0x53, 0x40),
    // Iris ext: channels 17..24 at 490..497
    (0x2e, 0x1a, 0x37), (0x6c, 0x34, 0x6c), (0x1e, 0x24, 0x0a), (0x4c, 0x41, 0x7e),
    (0x3c, 0x13, 0x28), (0x1c, 0x4f, 0x66), (0x28, 0x21, 0x0b), (0x18, 0x53, 0x40),
    // Carbon ext: channels 17..24 at 498..505
    (0x78, 0x36, 0x25), (0x75, 0x55, 0x22), (0x23, 0x23, 0x0b), (0x2c, 0x3b, 0x12),
    (0x90, 0x44, 0x31), (0x2c, 0x20, 0x0d), (0x3a, 0x39, 0x12), (0x3d, 0x50, 0x18),
    // Nocturne ext: channels 17..24 at 506..513
    (0x23, 0x4c, 0x79), (0x4e, 0x55, 0x96), (0x26, 0x1d, 0x3a), (0x13, 0x3c, 0x44),
    (0x29, 0x5c, 0x96), (0x1c, 0x1f, 0x3e), (0x3c, 0x30, 0x57), (0x19, 0x51, 0x5b),
    // Phosphor ext: channels 17..24 at 514..521
    (0x30, 0x52, 0x1a), (0x53, 0x60, 0x1e), (0x0c, 0x27, 0x11), (0x40, 0x36, 0x13),
    (0x3e, 0x64, 0x22), (0x20, 0x24, 0x0b), (0x15, 0x3e, 0x1e), (0x55, 0x49, 0x1a),
    // ── the SECOND extension: channels 25..32, eight more per palette,
    // appended when the desk went to thirty-two. Same growth scheme as
    // the first block, seeded from slots 9..16. Bases in Palette::ext2.
    // Default ext2: channels 25..32 at 522..529
    (0x56, 0x2b, 0x1b), (0x72, 0x56, 0x20), (0x33, 0x3a, 0x10), (0x0b, 0x27, 0x0c),
    (0x11, 0x3d, 0x35), (0x29, 0x5c, 0x96), (0x41, 0x2f, 0x56), (0x3e, 0x12, 0x29),
    // Valentine ext2: channels 25..32 at 530..537
    (0x59, 0x25, 0x3e), (0x38, 0x14, 0x33), (0x5c, 0x25, 0x2e), (0x2f, 0x18, 0x3c),
    (0x55, 0x2c, 0x18), (0x1c, 0x66, 0x4e), (0x45, 0x34, 0x14), (0x37, 0x14, 0x38),
    // Hydra ext2: channels 25..32 at 538..545
    (0x11, 0x3d, 0x38), (0x23, 0x61, 0x7a), (0x12, 0x3e, 0x28), (0x6b, 0x59, 0x1e),
    (0x5b, 0x24, 0x37), (0x2a, 0x5d, 0x93), (0x1c, 0x3e, 0x1c), (0x2b, 0x19, 0x3e),
    // Navi ext2: channels 25..32 at 546..553
    (0x16, 0x3b, 0x4d), (0x15, 0x1f, 0x4b), (0x11, 0x3d, 0x3c), (0x7d, 0x51, 0x23),
    (0x38, 0x31, 0x58), (0x1c, 0x66, 0x4e), (0x4d, 0x2a, 0x4d), (0x10, 0x27, 0x0b),
    // Mono ext2: channels 25..32 at 554..561
    (0x25, 0x22, 0x1d), (0x1d, 0x22, 0x29), (0x25, 0x22, 0x1d), (0x1e, 0x22, 0x29),
    (0x3c, 0x36, 0x2e), (0x2f, 0x37, 0x42), (0x3c, 0x36, 0x2e), (0x30, 0x37, 0x42),
    // Studio ext2: channels 25..32 at 562..569
    (0x4c, 0x31, 0x15), (0x6f, 0x58, 0x1f), (0x55, 0x2b, 0x19), (0x5d, 0x5d, 0x1b),
    (0x5a, 0x28, 0x20), (0x43, 0x63, 0x20), (0x5b, 0x24, 0x37), (0x24, 0x61, 0x7e),
    // Aurora ext2: channels 25..32 at 570..577
    (0x12, 0x3e, 0x30), (0x0e, 0x24, 0x33), (0x30, 0x33, 0x5c), (0x31, 0x17, 0x3b),
    (0x59, 0x25, 0x3e), (0x26, 0x67, 0x30), (0x3b, 0x38, 0x11), (0x38, 0x14, 0x33),
    // Sandstone ext2: channels 25..32 at 578..585
    (0x53, 0x2d, 0x17), (0x77, 0x54, 0x21), (0x3e, 0x37, 0x12), (0x48, 0x63, 0x1e),
    (0x5a, 0x28, 0x20), (0x0b, 0x27, 0x10), (0x5b, 0x24, 0x37), (0x24, 0x61, 0x7e),
    // Centennial ext2: channels 25..32 at 586..593
    (0x41, 0x35, 0x12), (0x7d, 0x51, 0x23), (0x19, 0x38, 0x59), (0x2b, 0x19, 0x3e),
    (0x5a, 0x28, 0x20), (0x43, 0x63, 0x20), (0x54, 0x27, 0x45), (0x1c, 0x65, 0x5a),
    // Blueprint ext2: channels 25..32 at 594..601
    (0x19, 0x38, 0x59), (0x17, 0x1d, 0x50), (0x13, 0x3c, 0x44), (0x27, 0x1b, 0x41),
    (0x50, 0x2f, 0x16), (0x1c, 0x66, 0x4e), (0x4d, 0x2a, 0x4d), (0x0b, 0x27, 0x10),
    // Porcelain ext2: channels 25..32 at 602..609
    (0x19, 0x38, 0x59), (0x1a, 0x1c, 0x4d), (0x13, 0x3c, 0x44), (0x2b, 0x19, 0x3e),
    (0x57, 0x2b, 0x1c), (0x1c, 0x67, 0x43), (0x54, 0x27, 0x45), (0x10, 0x27, 0x0b),
    // Moss ext2: channels 25..32 at 610..617
    (0x26, 0x3c, 0x15), (0x4e, 0x61, 0x1c), (0x16, 0x3e, 0x1d), (0x1c, 0x66, 0x4e),
    (0x41, 0x35, 0x12), (0x24, 0x61, 0x7e), (0x55, 0x2c, 0x18), (0x33, 0x16, 0x3b),
    // Rhodes ext2: channels 25..32 at 618..625
    (0x48, 0x33, 0x14), (0x89, 0x4a, 0x26), (0x40, 0x37, 0x12), (0x36, 0x1a, 0x0f),
    (0x30, 0x3b, 0x10), (0x24, 0x61, 0x7e), (0x59, 0x25, 0x3e), (0x2b, 0x19, 0x3e),
    // Copper ext2: channels 25..32 at 626..633
    (0x50, 0x2f, 0x16), (0x1c, 0x65, 0x5d), (0x45, 0x34, 0x14), (0x0d, 0x24, 0x2f),
    (0x5a, 0x28, 0x20), (0x1c, 0x67, 0x43), (0x4d, 0x2a, 0x4d), (0x10, 0x27, 0x0b),
    // Dusk ext2: channels 25..32 at 634..641
    (0x45, 0x2d, 0x54), (0x38, 0x14, 0x33), (0x50, 0x2f, 0x16), (0x22, 0x1d, 0x41),
    (0x59, 0x25, 0x3e), (0x24, 0x61, 0x7e), (0x3b, 0x38, 0x11), (0x1c, 0x66, 0x4e),
    // Iris ext2: channels 25..32 at 642..649
    (0x46, 0x2d, 0x52), (0x38, 0x14, 0x33), (0x30, 0x3b, 0x10), (0x27, 0x1b, 0x41),
    (0x59, 0x25, 0x3e), (0x24, 0x61, 0x7e), (0x41, 0x35, 0x12), (0x1c, 0x66, 0x4e),
    // Carbon ext2: channels 25..32 at 650..657
    (0x38, 0x19, 0x11), (0x48, 0x34, 0x15), (0x4c, 0x4c, 0x18), (0x49, 0x62, 0x1e),
    (0x57, 0x2a, 0x1e), (0x5f, 0x45, 0x1c), (0x60, 0x5d, 0x1c), (0x1c, 0x25, 0x0b),
    // Nocturne ext2: channels 25..32 at 658..665
    (0x10, 0x23, 0x38), (0x2f, 0x34, 0x5a), (0x51, 0x3f, 0x7e), (0x1f, 0x63, 0x70),
    (0x19, 0x38, 0x5b), (0x3d, 0x44, 0x84), (0x64, 0x50, 0x92), (0x0c, 0x26, 0x2a),
    // Phosphor ext2: channels 25..32 at 666..673
    (0x17, 0x26, 0x0c), (0x33, 0x3a, 0x12), (0x1a, 0x54, 0x25), (0x6a, 0x5a, 0x1f),
    (0x26, 0x3d, 0x15), (0x44, 0x4e, 0x18), (0x23, 0x67, 0x31), (0x28, 0x22, 0x0c),
    // ── Silverface, APPENDED (the rule): the twentieth run, and the first
    // appended WHOLE — channels, click, master and both extensions in one
    // block — because every earlier block was full the day it shipped.
    // The Dual Showman's faceplate on the desk: steel-blue, aluminum,
    // Fender turquoise, one red lineage, the jewel lamp as the master.
    // Designed and gated by scripts/palgen.py --silverface.
    // Silverface: channels 674..689, click 690
    (0x10, 0x23, 0x38), (0x3c, 0x37, 0x2d), (0x17, 0x51, 0x58), (0x94, 0x42, 0x35),
    (0x19, 0x38, 0x5b), (0x50, 0x49, 0x3c), (0x1c, 0x64, 0x6b), (0x3c, 0x16, 0x10),
    (0x22, 0x4b, 0x7a), (0x63, 0x5a, 0x49), (0x0b, 0x25, 0x28), (0x60, 0x23, 0x19),
    (0x2a, 0x5c, 0x96), (0x25, 0x22, 0x1b), (0x11, 0x3d, 0x41), (0x7f, 0x30, 0x24),
    (0x4e, 0x52, 0x61),
    (0xb3, 0x1b, 0x25), // 691  Silverface
    // Silverface ext: channels 17..24 at 692..699
    (0x19, 0x2b, 0x58), (0x45, 0x41, 0x33), (0x1d, 0x5a, 0x70), (0x92, 0x4a, 0x2c),
    (0x23, 0x3e, 0x7d), (0x57, 0x53, 0x41), (0x21, 0x66, 0x7e), (0x49, 0x23, 0x13),
    // Silverface ext2: channels 25..32 at 700..707
    (0x21, 0x57, 0x77), (0x6a, 0x5b, 0x4f), (0x0d, 0x33, 0x31), (0x76, 0x24, 0x26),
    (0x26, 0x63, 0x87), (0x34, 0x2c, 0x26), (0x13, 0x48, 0x45), (0x8e, 0x36, 0x39),
];

/// How much of [`STRIP_COLORS`] a person can point at.
///
/// The rest is the auto-palettes' own, reachable by choosing a palette and
/// never by choosing a swatch: a hundred and thirty-odd swatches is not a
/// picker, it is a wall.
pub const MANUAL_COLORS: usize = 27;

/// Three, because the hand-picking grid is hues down and tones across.
pub const PALETTE_COLUMNS: usize = 3;

/// One auto-palette: what every channel nobody has painted by hand wears.
///
/// **Sixteen slots, because the desk has sixteen channels.** They used to be
/// eight and cycle, so channel 8 wore channel 0's colour and two tracks that
/// had nothing to do with each other matched.
pub struct Palette {
    pub name: &'static str,
    /// Where this palette's run starts in [`STRIP_COLORS`]: sixteen channel
    /// colours, then its click.
    pub base: usize,
    /// This palette's own MASTER, appended at the end of the table rather
    /// than inside the run — the runs are contiguous and every index in them
    /// is already written into somebody's saved desk. See [`Palette::master`].
    pub master_at: usize,
    /// Where this palette's EXTENSION run starts: channels 17..24, eight
    /// colours appended when the desk grew past the frozen sixteen-slot
    /// runs. The original runs are append-only history; the extension is
    /// its own block for the same reason the masters are.
    pub ext: usize,
    /// And the second extension: channels 25..32, appended when the desk
    /// went to thirty-two the same evening it went to twenty-four.
    pub ext2: usize,
}

impl Palette {
    /// The colour for channel `i`. The first sixteen live in the frozen
    /// original run; 17..24 in the palette's extension block. PALETTE_RUN
    /// and not CHANNELS: the run length is table HISTORY, and history does
    /// not grow when the desk does.
    pub fn slot(&self, i: usize) -> usize {
        let i = i % CHANNELS;
        if i < PALETTE_RUN {
            self.base + i
        } else if i < PALETTE_RUN + 8 {
            self.ext + (i - PALETTE_RUN)
        } else {
            self.ext2 + (i - PALETTE_RUN - 8)
        }
    }

    /// The click's colour in this palette.
    ///
    /// The click is not a channel and should not dress like one — but it is
    /// not the master either, so it gets the palette's own quiet neutral
    /// rather than a hue out of the set.
    pub fn click(&self) -> usize {
        self.base + PALETTE_RUN
    }

    /// **The master's colour, this palette's own.**
    ///
    /// It was one crimson ([`MASTER_COLOR`]) shared by all sixteen, on the
    /// argument that the master is the strip you go looking for and a master
    /// that moved with the theme is one you have to find twice. The owner
    /// weighed that and asked for the other thing — "a true color picked per
    /// theme freshly ... for the master lane strip" — so this is per palette
    /// now, and the trade is deliberate rather than overlooked.
    ///
    /// `MASTER_COLOR` itself is untouched and still means what it meant: it
    /// is a hand-pickable swatch and the migration target for desks written
    /// before the palettes existed.
    pub fn master(&self) -> usize {
        // **One red, on every theme, at the owner's word.** Each palette
        // still CARRIES its own master in `master_at` — the field is written
        // into saved desks and the runs are append-only history — but the
        // desk asks for the master's colour here, and the answer is the
        // crimson that sits above the channel colours' chroma ceiling. A
        // master tuned to each theme's vividness is another dark rose in a
        // rack of them; the one channel you look for should not have to be
        // found. Its dials go with it: see `dial_ink`.
        let _ = self.master_at;
        MASTER_COLOR
    }
}

/// The AUTO-PALETTES: **one per THEME, named for it.**
///
/// They were eight abstract sets (IVORY, EMBER, TIDE…) that sixteen themes
/// mapped onto, several sharing. The owner's word was "pick and choose": a
/// palette per theme, named so you can put Rhodes' colours on the Iris desk
/// if that is what you want. AUTO in the track menu is the ordinary answer
/// and gives each theme its own.
///
/// The ORDER is the index `palette_choice` saves, so it matches `factory()`
/// and must not be shuffled. Retiring a theme is the one thing that moves
/// it: Carbon left by NAME, its rows stayed in [`STRIP_COLORS`], and the two
/// after it shifted up one here — so a `palette_choice` saved while Carbon
/// was 16 lands one palette over, which the owner weighed against a
/// migration and declined.
pub const PALETTES: [Palette; 19] = [
    Palette { name: "Default", base: 28, master_at: 300, ext: 370, ext2: 522 },
    Palette { name: "Valentine", base: 45, master_at: 301, ext: 378, ext2: 530 },
    Palette { name: "Hydra", base: 62, master_at: 302, ext: 386, ext2: 538 },
    Palette { name: "Navi", base: 79, master_at: 303, ext: 394, ext2: 546 },
    Palette { name: "Mono", base: 96, master_at: 304, ext: 402, ext2: 554 },
    Palette { name: "Studio", base: 113, master_at: 305, ext: 410, ext2: 562 },
    Palette { name: "Aurora", base: 130, master_at: 306, ext: 418, ext2: 570 },
    Palette { name: "Sandstone", base: 147, master_at: 307, ext: 426, ext2: 578 },
    Palette { name: "Centennial", base: 164, master_at: 308, ext: 434, ext2: 586 },
    Palette { name: "Blueprint", base: 181, master_at: 309, ext: 442, ext2: 594 },
    Palette { name: "Porcelain", base: 198, master_at: 310, ext: 450, ext2: 602 },
    Palette { name: "Moss", base: 215, master_at: 311, ext: 458, ext2: 610 },
    Palette { name: "Rhodes", base: 232, master_at: 312, ext: 466, ext2: 618 },
    Palette { name: "Copper", base: 249, master_at: 313, ext: 474, ext2: 626 },
    Palette { name: "Dusk", base: 266, master_at: 314, ext: 482, ext2: 634 },
    Palette { name: "Iris", base: 283, master_at: 315, ext: 490, ext2: 642 },
    Palette { name: "Nocturne", base: 333, master_at: 368, ext: 506, ext2: 658 },
    Palette { name: "Phosphor", base: 350, master_at: 369, ext: 514, ext2: 666 },
    Palette { name: "Silverface", base: 674, master_at: 691, ext: 692, ext2: 700 },
];

/// The drum bus's colour everywhere: dark violet, no palette uses it, so
/// the bus can never dress like one of the eight.
pub const DRUMS_COLOR: usize = 22;

/// The master's colour. Crimson, because it is the one channel you look for.
///
/// It sits ABOVE the chroma ceiling the channel colours obey — deliberately.
/// Pulled down to their vividness it stops being crimson and becomes another
/// dark rose in a rack of them, which is the opposite of the job.
pub const MASTER_COLOR: usize = 27;

/// What the master used to be: the old table's top red. Recognised on load so
/// a desk that never had its master repainted joins the palette instead of
/// keeping a colour the owner never actually chose.
pub const LEGACY_MASTER_COLOR: i64 = 5;

/// What the click used to be, for the same reason.
pub const LEGACY_CLICK_COLOR: i64 = 8;

/// Where the eight-colour table's indices moved to when it became this one.
///
/// **A palette is a set of indices somebody has already saved.** Growing the
/// table from eight to twenty-seven renumbered every one of them, so a desk
/// with a teal microphone and a violet backing track came back dark red and
/// brown — colours the user never chose, on a build where the old ones were
/// still on screen a minute earlier.
///
/// Each entry is the nearest tone of the same hue. `slate` becomes `stone`,
/// which is the only one that is not an exact match and is the closest the new
/// neutrals get.
pub const PALETTE_V1_TO_V2: [usize; 8] = [0, 5, 8, 14, 16, 20, 23, 2];

/// Which way a control travels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DragAxis {
    Vertical,
    /// Swept sideways: a control drawn as a level lying down has to fill in
    /// the direction the hand moves, or the picture and the gesture disagree.
    Horizontal,
}

/// How far the hand moves to sweep a fader end to end, in points.
///
/// **Long, and longer than it looks.** The first pair were a fader's own
/// height, on the reasoning that a fader should feel one-to-one under the
/// pointer — but the range is eighty decibels and the track is a hundred
/// points, so every point was most of a decibel and nobody could land on a
/// number. A sweep wider than the screen is not a problem: the drag is
/// RELATIVE, so the hand leaves the fader and goes on pulling.
const FADER_TRAVEL: f32 = 420.0;

/// The same, for a send. A percentage is read to the nearest few, so it wants
/// the same room a fader does.
const SEND_TRAVEL: f32 = 420.0;

/// Where everything on one strip is, and where the strips are.
///
/// Fractions of the rect rather than points, so the same layout holds at every
/// window size the app offers — and so the hit test and the painter cannot
/// drift, because they read the same struct.
/// The drawn channels, plus the master.
///
/// Every instrument slot, every input of the interface, the backing track and
/// the master. The click and the effects return keep their place on the desk
/// and are not drawn — their controls are the band's.
// Eight general channels, the click, and the master. BACKING had a column
// here and is retired: the arrangement's lanes carry audio now.
pub const COLUMNS: usize = CHANNELS + 2;

/// How many route strips a mixer channel shows before it stops growing.
///
/// **Three, because they come out of the fader's travel.** Every strip is
/// vertical space the fader used to have; three is enough for the patches
/// people actually build (a headphone feed, a reverb send, a stage monitor)
/// and leaves the fader long enough to ride. Past three, the routes are all
/// still THERE and all still audible - the strip simply stops drawing them,
/// and the routing window remains where they are managed.
pub const STRIP_ROUTES: usize = 3;

pub struct Layout {
    pub strips: [StripStrip; COLUMNS],
    /// The scrolling channel region — everything left of the pinned click.
    /// Channel strips CLIP to it (a half-scrolled strip stops at its edge
    /// rather than bleeding under the click), and a hit outside it belongs
    /// to nobody however far a partial strip's rectangle reaches.
    pub region: Rect,
}

/// One strip's rectangles.
#[derive(Debug, Clone, Copy)]
pub struct StripStrip {
    pub panel: Rect,
    pub icon: Rect,
    /// **The room the badge was allowed**, before it was squared off into
    /// [`Self::icon`].
    ///
    /// Carried so a test can ask whether the badge takes ALL of it. Measuring
    /// the fill any other way means re-deriving the band's fractions in the
    /// test, and a re-derivation is a copy of the thing under test — it would
    /// go on agreeing with itself after the layout had shrunk.
    pub icon_room: Rect,
    pub name: Rect,
    pub detail: Rect,
    pub meter: Rect,
    pub send: Rect,
    /// The pan knob, sharing the send's band on general channels.
    pub pan: Rect,
    /// The input chip — the detail row, pressable on general channels.
    pub input_chip: Rect,
    /// **A FOLDER's chip, and it reads FOLDER.** The band a track spends on
    /// its input was empty paint on a group — a group has no input to bind —
    /// and a row of chips across the desk with holes in it reads as chips
    /// that failed to draw rather than as strips that have none. It says
    /// what the strip IS, which is the one true thing there is to say there,
    /// and it is not pressable: there is nothing to choose.
    pub folder_chip: Rect,
    /// **The click's own sound**, in the band a channel spends on its input
    /// chip. It reads the file that is playing and it opens the metronome
    /// panel — the same one the arrangement's toolbar opens, because there is
    /// one metronome and it should not have two menus.
    ///
    /// The WORD on it is the strip's `detail`, like every other chip on the
    /// desk: the bundled file's own name rather than "CLICK", which is what
    /// the name plate two bands above it already says.
    pub met: Rect,
    /// The F switch — the folder cycle, leftmost of the switch row.
    pub folder: Rect,
    pub fader: Rect,
    pub db: Rect,
    pub mute: Rect,
    /// **Ø — polarity**, a thin band directly above the switch row.
    ///
    /// Above rather than among them because it is a different KIND of switch:
    /// M, S, R and T are about where a track goes, and this is about the
    /// waveform itself. Small, because it is set once when two mics disagree
    /// and then never touched again.
    pub phase: Rect,
    pub solo: Rect,
    /// The R switch. `NOTHING` on everything but a general channel.
    pub arm: Rect,
    /// The T switch: send this track's MIDI to the theory view. `NOTHING`
    /// on everything but a general channel, for the same reason.
    pub theory: Rect,
    /// **The master's mono fold**, at the foot of its strip and nowhere
    /// else. `NOTHING` on every channel.
    pub mono: Rect,
    /// A user effect plugin across this strip. Only the bus has one today.
    /// **Six, because the master has two racks.** A channel uses the first
    /// three and leaves the rest `NOTHING`; `recorder::bays_on` says how
    /// many a strip actually draws. See `recorder::MASTER_EXTRA_SEAT`.
    pub inserts: [Rect; INSERTS * 2],
    /// The Routing button, under the insert bays — the owner's placement.
    pub routing: Rect,
    /// One small strip per route: a label and its own fader. `NOTHING` past
    /// what this channel actually routes. See [`STRIP_ROUTES`].
    pub routes: [Rect; STRIP_ROUTES],
    /// The plus on an empty slot.
    pub add: Rect,
    /// The × at the top right of a removable channel. `NOTHING` on the rest.
    pub close: Rect,
}

impl StripStrip {
    /// Every rectangle absent, for a strip that is not there or a rect too
    /// small to hold one. `Rect::NOTHING` rather than a derive, because
    /// `Rect` has no `Default` and a zero-sized rect at the origin would be
    /// INSIDE small windows — a target you can press by accident.
    const NONE: Self = Self {
        theory: Rect::NOTHING,
        mono: Rect::NOTHING,
        panel: Rect::NOTHING,
        icon: Rect::NOTHING,
        icon_room: Rect::NOTHING,
        name: Rect::NOTHING,
        detail: Rect::NOTHING,
        meter: Rect::NOTHING,
        send: Rect::NOTHING,
        pan: Rect::NOTHING,
        input_chip: Rect::NOTHING,
        folder_chip: Rect::NOTHING,
        met: Rect::NOTHING,
        folder: Rect::NOTHING,
        fader: Rect::NOTHING,
        db: Rect::NOTHING,
        mute: Rect::NOTHING,
        phase: Rect::NOTHING,
        solo: Rect::NOTHING,
        arm: Rect::NOTHING,
        inserts: [Rect::NOTHING; INSERTS * 2],
        routing: Rect::NOTHING,
        routes: [Rect::NOTHING; STRIP_ROUTES],
        add: Rect::NOTHING,
        close: Rect::NOTHING,
    };
}

/// Space around the rack, and between the strips.
const PAD: f32 = 10.0;
const GAP: f32 = 6.0;
/// How many channel columns the mixer shows at once; the rest scroll. The
/// click and the master are pinned beside them and are not counted here.
pub const PALETTE_RUN: usize = 16;

pub const VISIBLE_CHANNELS: usize = 8;

/// **The least width a channel column can usefully be.**
///
/// A strip is a fader, a meter beside it, a name and a row of switches. Below
/// about this it is a toothpick: the fader is still draggable but the meter is
/// a hairline and the switches stop being separate targets.
///
/// It exists because how many strips to show is a SETTING, and a setting made
/// on a desktop travels to a laptop inside the same file. Sixteen across a
/// 1280-point screen is 60 points a strip before the click and the master take
/// theirs. The setting is not rewritten — taste is not the app's to edit — it
/// is simply not honoured past what the space can carry.
const MIN_STRIP_W: f32 = 46.0;

/// **The folder chop and the child indent are ONE number** - the owner's
/// spec: the 90-degree chop off a folder's top-right corner ends exactly
/// where its children's strips begin, so the eye reads containment as a
/// step down.
pub const CHILD_INDENT: f32 = 12.0;

/// **How much of a folder's top edge the chop takes**, as a share of the
/// strip's width. Written down rather than spelled twice: the notch is
/// painted in `strip` and the SELECTION RIM has to follow the same corner,
/// and a rim that traced a chop of its own would be a folder with two
/// different top-right corners depending on whether it was picked.
pub const NOTCH_W: f32 = 0.28;

impl Layout {
    /// The column unit width and pitch (unit + gap) at `rect` with `visible`
    /// channels showing. One derivation, shared with the wheel handler's
    /// clamp — two copies of this arithmetic is a scroll that overshoots.
    pub fn metrics(rect: Rect, visible: usize) -> (f32, f32) {
        let inner_w = (rect.width() - 2.0 * PAD).max(1.0);
        let slots = (visible.clamp(1, CHANNELS) + 2) as f32;
        let unit = ((inner_w - GAP * (slots - 1.0)) / slots).max(1.0);
        (unit, unit + GAP)
    }

    /// The furthest `scroll_px` can legally stand at `rect`: the desk's
    /// off-screen width. Zero when everything fits.
    /// `active` here is the SHOWN channel count - a shut folder's children
    /// are not columns and must not add scroll range; the app counts them
    /// out before asking (see `mixer_shown_channels`).
    pub fn max_scroll_px(rect: Rect, visible: usize, active: usize) -> f32 {
        let active = active.clamp(1, CHANNELS);
        let visible = visible.clamp(1, active);
        let (_, pitch) = Self::metrics(rect, visible);
        (active.saturating_sub(visible)) as f32 * pitch
    }

    pub fn new(rect: Rect, view: &MixerView<'_>) -> Self {
        let inner = Rect::from_min_max(
            Pos2::new(rect.min.x + PAD, rect.min.y + PAD),
            Pos2::new(rect.max.x - PAD, rect.max.y - PAD),
        );
        if !inner.is_positive() {
            return Self {
                strips: [StripStrip::NONE; COLUMNS],
                region: Rect::NOTHING,
            };
        }
        // **Eight channels show; the CLICK and the MASTER are pinned right.**
        // The desk grew past what fits, so the channel columns scroll (see
        // `MixerView::scroll_px`) while the two utility strips stay put at
        // the edge — the master is the one you always want in reach, and the
        // click beside it. Every column is one unit wide: a desk where one
        // strip is a different size is a desk with two kinds of strip.
        //
        // Layout: [ 8 scrolling channels | GAP | click | GAP | master ].
        let click_col = CHANNELS; // column index of the click
        let master_col = COLUMNS - 1;
        let active = view.active.clamp(1, CHANNELS);
        let visible = view.visible.clamp(1, active);
        // **Never more columns than the width can carry.** See `MIN_STRIP_W`.
        // Two seats are always spoken for — the click and the master are
        // pinned at the right and are not part of the scrolling run — which
        // is the `+ 2` `metrics` also applies.
        let fits_across = {
            let inner_w = (rect.width() - 2.0 * PAD).max(1.0);
            let slots = ((inner_w + GAP) / (MIN_STRIP_W + GAP)).floor() as usize;
            slots.saturating_sub(2).max(1)
        };
        let visible = visible.min(fits_across);
        // **There is no folded desk any more.** Sixteen over sixteen was a
        // way to get every channel on screen at a usable width when the
        // window was tall, and it cost every strip half its height to do it
        // — a fader and a meter at half length, on the one view whose whole
        // job is reading levels. The owner asked for it gone: one row,
        // always, at whatever width the desk can carry. What it was FOR is
        // answered instead by `app::default_mixer_visible`, which now opens
        // a large window with all sixteen across.
        // **A shut folder's children do not occupy columns.** Every strip
        // keeps its CHANNEL index in `strips` - identity is baked as
        // column==channel all over the desk - but position runs on RANK
        // among the shown, so collapsing a folder closes the gap instead
        // of leaving invisible pillars.
        let mut rank = [0usize; COLUMNS];
        let mut shown = 0usize;
        for i in 0..active {
            rank[i] = shown;
            if !view.strips.get(i).is_some_and(|v| v.hidden) {
                shown += 1;
            }
        }
        let shown = shown.max(1);
        let visible = visible.min(shown);
        let cols_across = visible;
        let (unit, pitch) = Self::metrics(rect, cols_across);
        // The two pinned columns, from the right edge inward.
        let master_x = inner.right() - unit;
        let click_x = master_x - pitch;
        // The scrolling region ends just left of the click.
        let region_left = inner.left();
        let region = Rect::from_min_max(
            Pos2::new(region_left, inner.top()),
            Pos2::new(click_x - GAP, inner.bottom()),
        );
        // **Pixels, then a window with a remainder.** The scroll lands
        // wherever the wheel left it; the first fully-or-partly visible
        // column and how much of it hangs off the left edge both fall out
        // of one division. One extra column draws on the right when the
        // remainder exposes it — that half strip is the whole point.
        let max_px = (shown.saturating_sub(visible)) as f32 * pitch;
        let px = view.scroll_px.clamp(0.0, max_px.max(0.0));
        let first = ((px / pitch).floor() as usize).min(shown.saturating_sub(1));
        let frac = px - first as f32 * pitch;
        let last_shown = (first + visible + usize::from(frac > 0.5)).min(shown);
        let _row_h = inner.height();
        let strips = std::array::from_fn(|i| {
            let Some(v) = view.strips.get(i) else {
                return StripStrip::NONE;
            };
            let (x, y, h) = if i == master_col {
                (master_x, inner.top(), inner.height())
            } else if i == click_col {
                (click_x, inner.top(), inner.height())
            } else if i >= active || v.hidden {
                // Past the active desk (the paywall's lanes), or a
                // collapsed child: no column at all.
                return StripStrip::NONE;
            } else {
                // A channel column: only the visible window (plus the
                // partial at the edge) is laid out, in RANK space so a shut
                // folder's children leave no gap. Everything else is
                // off-desk.
                let r = rank[i];
                if r < first || r >= last_shown {
                    return StripStrip::NONE;
                }
                (
                    region_left + (r - first) as f32 * pitch - frac,
                    inner.top(),
                    inner.height(),
                )
            };
            // **A child's strip starts where the folder's chop ends** -
            // the whole panel steps down by the chop's height, which is
            // the visual indent the desk was missing.
            let (y, h) = if v.in_folder {
                (y + CHILD_INDENT, (h - CHILD_INDENT).max(1.0))
            } else {
                (y, h)
            };
            let panel = Rect::from_min_size(Pos2::new(x, y), Vec2::new(unit, h));
            Self::one(panel, Some(v))
        });
        Self { strips, region }
    }

    /// The rows inside one strip, top to bottom.
    ///
    /// **Read as a sentence**: what it is, how loud it is arriving, how much
    /// goes to the effects, how loud it leaves, and whether it is heard at all.
    /// A strip with no send gives that row's height to its meter rather than
    /// leaving a hole, because a hole reads as a control that failed to draw.
    fn one(panel: Rect, view: Option<&StripView<'_>>) -> StripStrip {
        let Some(view) = view else {
            return StripStrip::NONE;
        };
        // **An empty slot is a panel and a plus, and nothing else.** Drawing a
        // dead fader on it would be five controls that do nothing, which is
        // the thing the plugin rack was already doing wrong.
        if view.empty {
            // **The CIRCLE alone is the button now** — the owner's ruling:
            // pressing it adds a bare track on the spot, and the rest of
            // the empty panel is quiet ground where the right-click menu
            // lives. A whole-column button was one mis-aimed press away
            // from a track nobody meant to add.
            // Centred in the space BELOW the number's cap, never over it:
            // on a short panel (the two-row fold, a squeezed window) a
            // full-panel centre put the plus on top of the slot's own
            // number. The cap arithmetic mirrors `empty_slot`'s exactly.
            let cap_h = (panel.height() * 0.09).min(16.0);
            let below = Rect::from_min_max(
                Pos2::new(panel.left(), panel.top() + cap_h + 2.0),
                panel.max,
            );
            let d = (panel.width() * 0.42)
                .clamp(18.0, 40.0)
                .min((below.height() * 0.8).max(1.0));
            return StripStrip {
                panel,
                add: Rect::from_center_size(below.center(), Vec2::splat(d)),
                ..StripStrip::NONE
            };
        }
        let sends = view.sends();
        let switchable = view.live();
        let cut = |a: f32, b: f32| {
            Rect::from_min_max(
                Pos2::new(panel.left() + 4.0, panel.top() + panel.height() * a),
                Pos2::new(panel.right() - 4.0, panel.top() + panel.height() * b),
            )
        };
        // **The meter stands BESIDE the fader, not above it.** That is what a
        // channel strip looks like on every desk ever built, and it is not
        // decoration: the two answer one question between them — how much is
        // arriving and how much you are letting through — and reading them
        // means looking at one place. Stacked, the meter also left a hand's
        // width of empty face above the fader that nothing could use.
        // BELOW the insert row, which every channel has now. The two shared a
        // band for one build and the guard test found it before anybody could
        // press a send and get an effect picker.
        // The knob band: a channel splits it — send LEFT, pan RIGHT, the
        // owner's rule that every channel has a pan knob. The click and the
        // bus keep the whole band for their send.
        let (send, pan) = if sends {
            let band = cut(0.41, 0.53);
            if matches!(view.strip, Some(Strip::Channel(_) | Strip::Drums)) {
                (
                    Rect::from_min_max(
                        band.min,
                        Pos2::new(band.center().x - 1.0, band.bottom()),
                    ),
                    Rect::from_min_max(
                        Pos2::new(band.center().x + 1.0, band.top()),
                        band.max,
                    ),
                )
            } else {
                (band, Rect::NOTHING)
            }
        } else {
            (Rect::NOTHING, Rect::NOTHING)
        };
        // Short of the bottom edge: a switch flush against the frame reads as
        // something that has been cut off rather than as the end of the strip.
        // **Three inserts, on every channel, and the number never changes.**
        //
        // There used to be ONE, on the effects bus alone, on the argument that
        // a reverb is a send effect and per-channel inserts would be nine
        // copies of the same plugin. That argument is still true of reverb and
        // was never true of an amp sim on the input, a compressor on the piano
        // or an EQ on the backing track.
        //
        // Three because a desk you have to remember the rules of is not a desk
        // you can see, and because this started as a MIDI monitor: a rack that
        // grows until the machine gives out is a different program. Thirty-six
        // across the desk if every one is filled, which is deliberately
        // reachable — anybody who fills thirty-six finds out what their
        // hardware does, and that is worth learning from the app rather than
        // from a number the app made up.
        //
        // **Stacked, not shoulder to shoulder.** Three chips side by side in a
        // column ninety points wide are thirty points each, and thirty points
        // holds about three characters of "FabFilter Pro-R 2" — a rack you
        // have to click to find out what is in it. Down the strip each one is
        // as wide as the channel, which is where the reading room comes from;
        // the height it costs comes out of the fader's travel, which had the
        // most of anything on the strip.
        // **Shorter than it was, to make room for the Routing button under
        // it** - the owner's call about what gives when something new needs
        // a place: the bays first.
        // **The master's rack is twice as tall, because it holds twice as
        // many.** Its three extra bays took the empty paint between the
        // title and the original three — the owner's spot for them. A
        // channel's rack is exactly where it was.
        // **ONE rack band, on every strip on the desk.**
        //
        // The master's and a folder's used to start at 0.100 — reaching up
        // over the band the ICON sits in, which is why a group's badge was
        // covered by its own first bay. The owner: "lower the height of
        // folders' fx sends such that their icons are visible", and then
        // "make sure at the end that all fx sends are visually at the same
        // level through the whole mixer view to be the same as the current
        // tracks'". One band, the track's, and the bays inside it divide by
        // however many the strip wears.
        let rack = cut(0.195, 0.345);
        // **Six on every channel now**, folder or not — the owner's ruling,
        // see `recorder::bays_on`. A plain channel fits them into the band it
        // has always had rather than reaching up over its own icon and input
        // chip, so its bays are shorter than a folder's and nothing else on
        // the strip moved. The two buses keep their three.
        let bays = crate::recorder::bays_on(view.strip);
        let inserts: [Rect; INSERTS * 2] = std::array::from_fn(|i| {
            if !rack.is_positive() || i >= bays {
                return Rect::NOTHING;
            }
            let h = rack.height() / bays as f32;
            let y = rack.top() + h * i as f32;
            Rect::from_min_max(
                Pos2::new(rack.left() + 2.0, y + 1.0),
                Pos2::new(rack.right() - 2.0, y + h - 1.0),
            )
        });
        // **The master has a switch row too**, now that it carries a mute and
        // a solo — see the pair below. It is not `switchable`, deliberately:
        // that flag also arms the polarity switch and the theory note, and
        // neither means anything on a sum of everything.
        let master_live = view.strip.is_none() && !view.empty;
        let switches = if switchable || master_live {
            cut(0.902, 0.975)
        } else {
            Rect::NOTHING
        };
        // **Polarity shares the dB row**, at its right end.
        //
        // There is no band of its own to have: the gap between the readout
        // and the switches is under two percent of the strip, which is five
        // pixels — a target nobody could hit. The readout is centred text
        // with room at both ends, so the switch takes the right end and the
        // number keeps the rest. Still above the four switches, which is
        // where it was asked for.
        //
        // Shed with them: a strip too narrow for a legible number is too
        // narrow for a switch beside it.
        let db_band = cut(0.83, 0.888);
        let phase = if switchable && db_band.width() > 46.0 {
            // Bigger than the readout beside it wants to allow: the owner
            // could not hit the old 9..14 square. It may stand proud of the
            // band's height - the band has air above and below.
            let d = (db_band.height() * 1.5).clamp(14.0, 20.0);
            Rect::from_min_size(
                Pos2::new(db_band.right() - d - 1.0, db_band.center().y - d * 0.5),
                Vec2::splat(d),
            )
        } else {
            Rect::NOTHING
        };
        // **The MONO fold, on the master and nowhere else.** It takes the
        // switch row that a channel spends on M/S/R — the master has none of
        // those, so the row was empty paint at the foot of the one strip
        // that had nothing there. A mix check belongs at the end of the
        // signal path, which is where this strip is.
        // **The master's fold, ABOVE the fader** — the owner's placement,
        // and it is what frees the switch row for a mute and a solo so the
        // master and the click read as the same kind of column. It takes the
        // band a channel spends on ROUTING, which the master has no use for.
        let mono = if master_live {
            let row = cut(0.352, 0.404);
            Rect::from_min_max(
                Pos2::new(row.left() + 2.0, row.top()),
                Pos2::new(row.right() - 2.0, row.bottom()),
            )
        } else {
            Rect::NOTHING
        };
        // **M S R on a general channel; M S on the click.** Arm is
        // record-ready AND live — a thing only a channel can be. Three
        // switches share the row the pair had; the letters carry them at the
        // width that leaves.
        let has_arm = matches!(view.strip, Some(Strip::Channel(_)));
        // **FOUR switches on a general channel now** — M, S, R and T, the
        // last being "send this track's MIDI to the theory view". A channel
        // is the only thing that can be any of them, so the strips that are
        // not one keep the pair they had.
        // **M S R, and no more.** The F cycle and the T letter both left
        // the row - the owner's "F M S R T is a nightmare": folding lives
        // on the folder's own glyph now, and the theory send is the note
        // button at the strip's foot. A FOLDER is not a track and cannot
        // record, so it keeps only M and S.
        let has_arm = has_arm && !view.is_folder;
        let (mute, solo, arm) = if switchable && has_arm {
            let gap = 3.0;
            let w = ((switches.width() - gap * 2.0) / 3.0).max(1.0);
            let cell = |k: f32| {
                let left = switches.left() + k * (w + gap);
                Rect::from_min_max(
                    Pos2::new(left, switches.top()),
                    Pos2::new(left + w, switches.bottom()),
                )
            };
            (cell(0.0), cell(1.0), cell(2.0))
        } else if switchable || master_live {
            // **The master takes the pair too.** It is the one strip that
            // could not be silenced or soloed from the desk, which made it
            // the odd column out beside the click — the owner's "for click
            // and master looking the same in mixer view". `switchable` is
            // not widened to cover it: that flag also arms the polarity
            // switch and the theory note, and neither means anything on a
            // sum of everything.
            let mid = switches.center().x;
            (
                Rect::from_min_max(switches.min, Pos2::new(mid - 3.0, switches.max.y)),
                Rect::from_min_max(Pos2::new(mid + 3.0, switches.min.y), switches.max),
                Rect::NOTHING,
            )
        } else {
            (Rect::NOTHING, Rect::NOTHING, Rect::NOTHING)
        };
        // **The theory send is a NOTE at the strip's foot** - left edge on
        // the inserts' own left, flush to the bottom, channels only: a
        // folder cannot carry MIDI of its own, and the owner said so in
        // capitals.
        let theory = if switchable && has_arm {
            // Below the switch row, never grazing it, and on the RIGHT -
            // the owner's revision - flush with the inserts' right edge at
            // the strip's foot. Sheds whole when the sliver is under seven
            // points: a control half under the mute is the overlap class
            // the guard test exists for.
            let top = switches.bottom() + 2.0;
            let side = ((panel.bottom() - 1.5) - top).min(14.0);
            if side >= 7.0 {
                Rect::from_min_max(
                    Pos2::new(panel.right() - 6.0 - side, panel.bottom() - 1.5 - side),
                    Pos2::new(panel.right() - 6.0, panel.bottom() - 1.5),
                )
            } else {
                Rect::NOTHING
            }
        } else {
            Rect::NOTHING
        };
        // A strip with no send starts its fader where the send would have been,
        // rather than leaving a hole: a hole reads as a control that failed to
        // draw.
        // A strip with no send starts its fader where the send would have been,
        // rather than leaving a hole — unless something else took that room.
        // The effects bus has no send and DOES have an insert, and the two
        // wanted the same band; the guard test found it before anybody could
        // press one and get the other.
        // Every channel has an insert row now, so every channel's travel
        // starts below it — a send is what still moves the line.
        // **The ROUTING button stays; the route strips are gone.**
        //
        // Each route a channel had drew its own little level strip under the
        // button, and the fader gave up a slice of its travel for every one
        // — so a channel's shape changed as you routed it, and the strips
        // that appeared were three more things to aim at on a column that is
        // already wall-to-wall controls. The owner's call: keep the button
        // that summons the routing window, drop the strips that only appear
        // after a route is added. The levels are set in that window, which
        // is where the routes themselves are chosen.
        //
        // The fader gets its full travel back, and it is the SAME travel on
        // every channel whatever it routes.
        let routing = if matches!(view.strip, Some(Strip::Channel(_))) && !view.empty {
            cut(0.352, 0.398)
        } else {
            Rect::NOTHING
        };
        let routes: [Rect; STRIP_ROUTES] = [Rect::NOTHING; STRIP_ROUTES];
        let travel = cut(if sends { 0.55 } else { 0.42 }, 0.82);
        // The meter takes the left third and the fader the rest, so the fader
        // still sits near the middle of the strip where a hand expects it.
        let split = travel.left() + travel.width() * 0.42;
        // **Every source channel carries one, and it is the way in.** The
        // microphone and the waveform were labels: they said what the column
        // was, which the name underneath already said. An instrument had none
        // at all, so the desk's three kinds of source read as one kind of
        // column with different words on it.
        //
        // They are now the picker — press the microphone to choose the
        // interface, the keyboard to choose the instrument, the waveform to
        // choose the file — which is why the instrument needed a glyph of its
        // own. The bus, the click and the master have no source to choose and
        // keep the room for their names.
        // Every source channel carries one — the eight generals (whose glyph
        // is their KIND, and clicking it cycles the kind), the backing track,
        // and now the click. The bus and the master have nothing to pick and
        // keep the room for their names.
        // **And the MASTER**, which has a drawing of its own now — see
        // `auto_icon`. It was the one strip on the desk with a hole where
        // every other one has a badge.
        let has_icon = matches!(
            view.strip,
            None | Some(Strip::Channel(_) | Strip::Click | Strip::Drums)
        ) && !view.empty;
        // **The way OFF the desk, where every window puts it.** Subtle — a
        // small mark in faded ink, drawn by `strip` — but a real target, and
        // only on a channel there is something to remove from. The name band
        // gives up its right end so the two can never contend.
        // The × retired: removing a track lives in the right-click menu
        // now, where a destructive act belongs behind a named row.
        let close = Rect::NOTHING;
        let _ = view.removable;
        // **Under the name, not over it.** The master has no source to choose
        // and so no icon, and with the icon on top every other channel's name
        // sat a band lower than the master's — twelve labels at one height and
        // one at another, which reads as the master being a different kind of
        // object rather than the same strip with nothing to pick.
        let mut icon_room = Rect::NOTHING;
        let icon = if has_icon {
            // **The badge takes the whole room between the name and whatever
            // is under it** — the owner's ask, in his words: as large as they
            // can fit.
            //
            // The band used to be a pair of fractions that left slack on both
            // sides and then a 0.82 shrink on top of that, so the badge used
            // about two thirds of what it had. The band IS the clearance now:
            // its top sits just under the name plate (which ends at 0.078)
            // and its bottom just over the row beneath, so a badge that fills
            // it cannot reach either of them however the desk is sized. That
            // is what makes this safe in the folded sixteen-over-sixteen
            // desk, where every strip is half height — the room and the badge
            // scale together, and the clearance is a fraction rather than a
            // number of points that runs out.
            //
            // The click and the drum bus draw nothing in their detail row
            // (`app.rs` gives DRUM BUS and CLICK no second line), so theirs
            // reaches down through it. If a bus is ever given a second line,
            // `the_strip_badge_clears_the_name_above_it_and_the_chip_below`
            // is what fails.
            // **The track's band, on every strip that wears a badge.** The
            // buses used to take a taller one because they drew nothing in
            // the detail row beneath — and the owner's "for click, match the
            // track icon size" settles it: a badge is a badge, and the click
            // has something to put in that row now.
            let r = cut(0.082, 0.149);
            // Centred, and no longer nudged down. The nudge was there because
            // a badge two thirds the size of its band floated high in it; a
            // badge that fills the band has nowhere to float, and any offset
            // now would push it into a neighbour.
            //
            // Square, and the SHORT side wins: a strip is narrow, and a badge
            // as tall as its band on a tall window would be wider than the
            // strip it is drawn on.
            icon_room = r;
            Rect::from_center_size(
                r.center(),
                Vec2::splat(r.height().min(r.width())),
            )
        } else {
            Rect::NOTHING
        };
        StripStrip {
            panel,
            routing,
            routes,
            icon,
            icon_room,
            // **Loosely fitting the text, not a band of face.** These were a
            // fifteenth of the strip each, which at any usable window size is
            // three times the height of what is written in them: the plate
            // behind them then reads as a panel with a word lost in it.
            // The same band on every channel now, master included — trimmed
            // where the × took the corner, so the two are never one target.
            name: {
                let full = cut(0.022, 0.078);
                if close.is_positive() {
                    Rect::from_min_max(
                        full.min,
                        Pos2::new((close.left() - 3.0).max(full.left() + 1.0), full.max.y),
                    )
                } else {
                    full
                }
            },
            detail: if view.is_folder {
                // The folder's extra bays live here now — see `rack`.
                Rect::NOTHING
            } else if has_icon {
                cut(0.152, 0.19)
            } else {
                cut(0.086, 0.128)
            },
            // **The input chip is the detail row on a general channel** —
            // the row already NAMES the input; now pressing it chooses one.
            input_chip: if matches!(view.strip, Some(Strip::Channel(_)))
                && !view.empty
                && !view.is_folder
            {
                cut(0.152, 0.19)
            } else {
                Rect::NOTHING
            },
            // **A FOLDER says so, in the band a track names its input in.**
            // See the field: a group has no input, and the hole that left
            // was the only gap in the desk's one continuous row of chips.
            folder_chip: if view.is_folder && has_icon {
                cut(0.152, 0.19)
            } else {
                Rect::NOTHING
            },
            // The CLICK's band, the same one — the owner's "add the met menu
            // from arrange view as its own button ... it should read as the
            // file chosen". And only when there IS a file chosen: see the
            // field.
            met: if matches!(view.strip, Some(Strip::Click)) && !view.empty {
                cut(0.152, 0.19)
            } else {
                Rect::NOTHING
            },
            meter: Rect::from_min_max(travel.min, Pos2::new(split, travel.max.y)),
            send,
            pan,
            fader: Rect::from_min_max(Pos2::new(split, travel.min.y), travel.max),
            // The number keeps everything the polarity switch did not take.
            db: if phase.is_positive() {
                Rect::from_min_max(
                    db_band.min,
                    Pos2::new(phase.left() - 2.0, db_band.max.y),
                )
            } else {
                db_band
            },
            mute,
            phase,
            solo,
            arm,
            theory,
            mono,
            inserts,
            // **The collapse control is the TAB at the name plate's
            // top-left** - the owner's revision: the icon spot stays free
            // for a badge of the user's choosing, on folders like any
            // track, and the little tab that was only drawn is now the
            // thing you press.
            folder: if view.is_folder {
                let full = cut(0.022, 0.078);
                let side = full.height().clamp(11.0, 17.0);
                Rect::from_min_size(
                    Pos2::new(panel.left() + 5.0, full.center().y - side * 0.5),
                    Vec2::splat(side),
                )
            } else {
                Rect::NOTHING
            },
            // **The same target as the plus on an empty one**, because it is
            // the same question: what is on this channel. An empty slot draws
            // a plus and a filled one draws its icon, and both open the picker
            // that answers it.
            add: icon,
            close,
        }
    }

    /// Every clickable region and what it means.
    ///
    /// **The one source of truth.** `hit_test` reads it and the painter draws
    /// from the same rects, so a control that moved cannot become a control
    /// you can see and not press.
    pub fn targets(&self) -> Vec<(Rect, Hit)> {
        let mut out = Vec::with_capacity(6 * 4);
        for (i, s) in self.strips.iter().enumerate() {
            let i = Column(i);
            for (r, hit) in [
                // Polarity BEFORE the fader: on short strips the enlarged
                // switch can graze the fader's band by a pixel, and the
                // first registered rect wins a contested press - the
                // smaller control takes the sliver.
                (s.phase, Hit::Phase(i)),
                (s.fader, Hit::Fader(i)),
                (s.send, Hit::Send(i)),
                (s.pan, Hit::Pan(i)),
                (s.input_chip, Hit::InputChip(i)),
                (s.met, Hit::MetFile),
                (s.mute, Hit::Mute(i)),
                (s.solo, Hit::Solo(i)),
                (s.arm, Hit::Arm(i)),
                (s.folder, Hit::Folder(i)),
                (s.theory, Hit::ToTheory(i)),
                (s.mono, Hit::Mono(i)),
                (s.add, Hit::Add(i)),
                (s.inserts[0], Hit::Insert(i, 0)),
                (s.inserts[1], Hit::Insert(i, 1)),
                (s.inserts[2], Hit::Insert(i, 2)),
                // Slots 3..5: the master always had these and a FOLDER has
                // them now — and the master's were laid out, drawn, and
                // never registered, so its lower rack could not be clicked
                // in the mixer at all. The same disease the T switch had.
                (s.inserts[3], Hit::Insert(i, 3)),
                (s.inserts[4], Hit::Insert(i, 4)),
                (s.inserts[5], Hit::Insert(i, 5)),
                (s.routing, Hit::Routing(i)),
                (s.routes[0], Hit::RouteGain(i, 0)),
                (s.routes[1], Hit::RouteGain(i, 1)),
                (s.routes[2], Hit::RouteGain(i, 2)),
                (s.db, Hit::Db(i)),
                (s.name, Hit::Name(i)),
                (s.close, Hit::Remove(i)),
            ] {
                if r.is_positive() {
                    out.push((r, hit));
                }
            }
        }
        out
    }
}

/// Everything on the desk a keyboard can reach and a screen reader can say.
///
/// Built from [`Layout::targets`], which the panel already had, so this
/// cannot disagree with `hit_test` about where a control is. `Hit::axis`
/// does the rest of the work: a hit that drags is a slider and knows which
/// way it travels, and one that does not is a button.
///
/// **Named by their STRIP, always.** "Volume" on its own is useless on a desk
/// with sixteen of them, and a screen reader announcing sixteen identical
/// labels is a desk nobody can navigate. Every node here leads with the
/// channel's own name.
pub fn a11y_nodes(rect: Rect, view: &MixerView<'_>) -> Vec<crate::a11y::Node> {
    a11y_nodes_from(view, &Layout::new(rect, view).targets())
}

/// [`a11y_nodes`] over a target list the caller already has.
///
/// **A few hundred nodes were being built and dropped every frame.** The
/// desk branch of `paint` ran `a11y_nodes` (a Layout and its `targets`) and
/// then `targets` again for the map back from a focused node to its hit,
/// and handed the nodes to `offer_nodes`, whose first line returns when no
/// reader is on — so for everybody but a screen-reader user every frame
/// with the desk open laid the desk out twice, formatted a label and a
/// Debug id per control on sixteen strips, and threw them away. The
/// timeline had the same shape and was fixed first
/// (`arrangement_panel::a11y_nodes_from`); this is the desk's half. The
/// nodes are now built only on a frame that will install them, from the
/// one `targets` walk that frame also needs.
pub fn a11y_nodes_from(view: &MixerView<'_>, hits: &[(Rect, Hit)]) -> Vec<crate::a11y::Node> {
    use crate::a11y::{Node, Travel};
    let mut out = Vec::with_capacity(hits.len());
    for &(r, hit) in hits {
        let Some(col) = hit_column(hit) else { continue };
        let Some(s) = view.strips.get(col.0) else { continue };
        // An empty slot is a plus sign and nothing else; the eight controls
        // that would be on a channel are not there to be reached.
        if s.empty && !matches!(hit, Hit::Add(_)) {
            continue;
        }
        let who = if s.name.is_empty() {
            format!("Channel {}", col.0 + 1)
        } else {
            s.name.to_owned()
        };
        let id = egui::Id::new(("desk", col.0, format!("{hit:?}")));
        let travel = |norm: f32, axis: DragAxis| Travel {
            norm: norm.clamp(0.0, 1.0),
            up: matches!(axis, DragAxis::Vertical),
        };
        let node = match hit {
            Hit::Fader(_) => Node::slider(
                id,
                r,
                format!("{who} volume"),
                f64::from(s.gain),
                0.0,
                1.0,
                gain_text(s.gain),
                // The fader's POSITION, not its amplitude — they are equal
                // only at the ends. See the master knob in `recorder_panel`,
                // where getting this wrong sent one arrow press from -6 dB
                // to -20.4.
                travel(gain_to_fader(s.gain), DragAxis::Vertical),
            ),
            Hit::Send(_) => Node::slider(
                id,
                r,
                format!("{who} send"),
                f64::from(s.send),
                0.0,
                1.0,
                format!("{:.0}%", s.send * 100.0),
                travel(s.send, DragAxis::Vertical),
            ),
            Hit::Pan(_) => Node::slider(
                id,
                r,
                format!("{who} pan"),
                f64::from(s.pan),
                -1.0,
                1.0,
                pan_text(s.pan),
                // -1..1 laid onto the 0..1 the keyboard moves.
                travel((s.pan + 1.0) * 0.5, DragAxis::Vertical),
            ),
            Hit::Mute(_) => Node::toggle(id, r, format!("{who} mute"), s.muted),
            Hit::Phase(_) => Node::toggle(id, r, format!("{who} polarity invert"), s.phase),
            Hit::Solo(_) => Node::toggle(id, r, format!("{who} solo"), s.soloed),
            Hit::Arm(_) => Node::toggle(id, r, format!("{who} record arm"), s.armed),
            Hit::ToTheory(_) => Node::toggle(
                id,
                r,
                format!("{who} sends its MIDI to the theory view"),
                s.to_theory,
            ),
            Hit::Mono(_) => Node::toggle(id, r, "Master folded to mono", s.mono),
            Hit::Folder(_) => Node::toggle(
                id,
                r,
                format!("{who} shows its tracks"),
                !s.folder_shut,
            ),
            Hit::Add(_) => Node::button(id, r, format!("Add a channel at slot {}", col.0 + 1)),
            Hit::Remove(_) => Node::button(id, r, format!("Remove {who}")),
            Hit::Name(_) => Node::readout(id, r, format!("{who} name"), who.clone()),
            Hit::InputChip(_) => Node::readout(
                id,
                r,
                format!("{who} input"),
                if s.detail.is_empty() { "none".to_owned() } else { s.detail.to_owned() },
            ),
            Hit::MetFile => Node::readout(
                id,
                r,
                "metronome sound".to_owned(),
                view.click_sound.to_owned(),
            ),
            Hit::Db(_) => Node::readout(
                id,
                r,
                format!("{who} level"),
                gain_text(s.gain),
            ),
            Hit::Routing(_) => Node::button(id, r, format!("{who} routing")),
            Hit::Insert(_, bay) => {
                let what = s.inserts.get(bay).copied().unwrap_or("");
                // **A reader is told the bay is bypassed**, because the
                // amber bar and the greyed name are the whole of the on-screen
                // answer and neither reaches a reader. Said in the value
                // rather than in the label, so the bay is still "insert 2"
                // when it is switched on again.
                let off = s.bypassed.get(bay).copied().unwrap_or(false);
                Node::readout(
                    id,
                    r,
                    format!("{who} insert {}", bay + 1),
                    match (what.is_empty(), off) {
                        (true, _) => "empty".to_owned(),
                        (false, true) => format!("{what}, bypassed"),
                        (false, false) => what.to_owned(),
                    },
                )
            }
            Hit::RouteGain(_, k) => {
                let level = view.routes.get(k).map_or(0.0, |(_, _, g)| *g);
                Node::slider(
                    id,
                    r,
                    format!("{who} route {} level", k + 1),
                    f64::from(level),
                    0.0,
                    1.0,
                    format!("{:.0}%", level * 100.0),
                    travel(level, DragAxis::Horizontal),
                )
            }
            // The insert menu is modal over the whole desk while it is up;
            // its two rows are not part of the desk's tab order.
            Hit::FxMenu(..) => continue,
        };
        out.push(node);
    }
    out
}

/// Which column a hit belongs to. Every variant carries one.
fn hit_column(hit: Hit) -> Option<Column> {
    Some(match hit {
        Hit::Fader(i)
        | Hit::Send(i)
        | Hit::Pan(i)
        | Hit::InputChip(i)
        | Hit::Mute(i)
        | Hit::Phase(i)
        | Hit::Solo(i)
        | Hit::Arm(i)
        | Hit::ToTheory(i)
        | Hit::Mono(i)
        | Hit::Add(i)
        | Hit::Routing(i)
        | Hit::Db(i)
        | Hit::Name(i)
        | Hit::Folder(i)
        | Hit::Remove(i) => i,
        Hit::MetFile => Column(Strip::Click.index()),
        Hit::Insert(i, _) | Hit::RouteGain(i, _) | Hit::FxMenu(i, _, _) => i,
    })
}

/// "L64", "C", "R20" — where a pan sits, in the words the desk uses.
///
/// `pub(crate)` because the arrangement's header carries the same three
/// dials and its screen reader must say the same words about them; a second
/// copy over there is a second chance to disagree about what "centre" is.
pub(crate) fn pan_text(pan: f32) -> String {
    let p = (pan.clamp(-1.0, 1.0) * 100.0).round() as i32;
    match p {
        0 => "centre".to_owned(),
        n if n < 0 => format!("left {}", -n),
        n => format!("right {n}"),
    }
}

/// What a press at `pos` means, or nothing.

/// **The folder-drop strip of a COLUMN**: the right quarter of the panel -
/// the arrangement's bottom-quarter rule, stood upright, one function so
/// the hover paint and the drop cannot disagree here either.
pub fn folder_drop_strip(panel: Rect) -> Rect {
    if !panel.is_positive() {
        return Rect::NOTHING;
    }
    let w = (panel.width() * 0.25).max(2.0);
    Rect::from_min_max(Pos2::new(panel.right() - w, panel.top()), panel.max)
}

pub fn hit_test(rect: Rect, view: &MixerView<'_>, pos: Pos2) -> Option<Hit> {
    let l = Layout::new(rect, view);
    // **The insert menu, and it is MODAL across the whole mixer.** The
    // palette only swallows its own strip, which is right for a colour — a
    // press on the next fader is somebody done choosing. A menu offering
    // DELETE is different: while it is up, everything that is not one of its
    // two rows is a dismissal and nothing else, because the mis-click this
    // exists to prevent is exactly a press that lands somewhere it should not.
    if let Some((at, bay)) = view.fx_menu {
        if let Some(s) = l.strips.get(at.0) {
            // The rows the menu is offering, from the one list the painter
            // draws from: the actions are POSITIONAL and a conditional row
            // must not re-point the rows above it. See `fx_menu_actions`.
            let actions =
                fx_menu_actions(view.fx_menu_loaded, view.fx_menu_key, view.fx_menu_paste);
            let rows = fx_menu_rows(s, bay, actions.len());
            for (r, action) in rows.iter().zip(actions) {
                if r.contains(pos) {
                    return Some(Hit::FxMenu(at, bay, action));
                }
            }
        }
        return Some(Hit::FxMenu(at, bay, FxAction::Dismiss));
    }
    // **A press lands only on what is painted.** A half-scrolled channel
    // strip's rectangles reach past the region it is clipped to; the part a
    // person cannot see must not answer. The pinned click and master live
    // to the region's right and keep answering everywhere they draw.
    let in_bounds = |i: usize| i >= CHANNELS || l.region.contains(pos);
    l.targets()
        .into_iter()
        .find(|(r, h)| r.contains(pos) && in_bounds(h.column().0))
        .map(|(_, h)| h)
}

/// Which column a point is on, for a right-click. `None` off the rack.
pub fn strip_at(rect: Rect, view: &MixerView<'_>, pos: Pos2) -> Option<Column> {
    Layout::new(rect, view)
        .strips
        .iter()
        .position(|s| s.panel.contains(pos))
        .map(Column)
}

/// How far a control travels, for the caller that is dragging it.
pub fn drag_travel(hit: Hit) -> Option<f32> {
    hit.travel()
}

// ── painting ───────────────────────────────────────────────────────────────

struct Ink {
    wood: Color32,
    face: Color32,
    /// The input chip, which is its own theme role — see where it is drawn.
    chip: Color32,
    engrave: Color32,
    faint: Color32,
    track: Color32,
    /// The fader's slot: a dark channel cut into the face, not a hairline.
    slot: Color32,
    /// The fader's cap: bone, the colour of the real thing.
    cap: Color32,
    /// The ridges across it, a shade under the cap so they read as cuts.
    cap_rib: Color32,
    lit: Color32,
    mute: Color32,
    solo: Color32,
    /// The R switch's light: record red, brighter than the solo's crimson.
    arm: Color32,
    meter_lo: Color32,
    gr: Color32,
    meter_hi: Color32,
    /// An unoccupied slot's ground, cut out of the wood — see
    /// [`empty_ground_for`].
    empty_ground: Color32,
    /// The hairline round that hole, measured against the hole.
    empty_edge: Color32,
}

fn ink(view: &MixerView<'_>) -> Ink {
    let (r, g, b) = view.wood;
    let wood = Color32::from_rgb(r, g, b);
    let empty_ground = empty_ground_for(wood);
    // The strip faces are the band's own bone, lifted a little so a rack of
    // them does not read as one slab.
    // The strip face is a THEME role now (`fx_strip`): the insert bays are
    // the biggest painted area on the desk, and a theme that could not reach
    // them changed the band and left the mixer behind. Dark mode still
    // darkens whatever the theme chose, so the two controls compose instead
    // of one cancelling the other.
    let themed = view.fx_strip;
    let face = if view.dark_mode {
        shade_face(themed, 0.88)
    } else {
        themed
    };
    // **The ink follows the FACE, exactly as the band's ink follows the
    // band.** Every word on a strip - the name, the input chip, the bay
    // labels, the dial ticks and their readouts, the dB line, M/S/R and the
    // plus on an empty slot - was one fixed near-black, which is right on the
    // bone face this desk ships with and unreadable on any dark one. The
    // owner's screenshot showed a whole rack of dark-on-dark. The strip is a
    // surface like any other: what is printed on it has to answer to how
    // light it is.
    let dark_face = crate::recorder_panel::wants_light_ink(face);
    let engrave = if dark_face {
        Color32::from_rgb(0xE8, 0xDC, 0xC0)
    } else {
        Color32::from_rgb(0x24, 0x20, 0x1A)
    };
    Ink {
        wood,
        face,
        chip: view.input_chip,
        engrave,
        // Secondary text: pulled toward the face rather than made
        // transparent, so it reads as quieter ink on either ground instead of
        // fading into the panel.
        faint: mix(engrave, face, 0.42),
        // Dark enough to read as a well rather than as a lighter panel: at
        // rest a meter should look like somewhere a level would go.
        track: Color32::from_rgb(0x1A, 0x17, 0x12),
        // **Ivory on black, from the owner's photograph of the real desk** —
        // and the SAME ivory and the same black the band's faders run in, out
        // of the one place they are written down. Two views of one desk that
        // disagreed about what a fader is made of would be two desks.
        slot: crate::recorder_panel::CAP_SLOT,
        cap: crate::recorder_panel::CAP_BONE,
        cap_rib: crate::recorder_panel::CAP_RIB,
        lit: Color32::from_rgb(0x1C, 0x6F, 0xD6),
        mute: Color32::from_rgb(0xE0, 0xA8, 0x22),
        solo: Color32::from_rgb(0xE8, 0x3A, 0x4E),
        arm: Color32::from_rgb(0xE0, 0x22, 0x22),
        meter_lo: Color32::from_rgb(0x3D, 0xC0, 0x5A),
        gr: Color32::from_rgb(0xE8, 0x8C, 0x2A),
        meter_hi: Color32::from_rgb(0xE0, 0xA8, 0x22),
        empty_ground,
        empty_edge: crate::recorder_panel::distinct_surface_at(
            recess(empty_ground, 0.34),
            empty_ground,
            1.6,
        ),
    }
}

/// Paint the whole rack into `rect`.
pub fn draw(painter: &Painter, rect: Rect, view: &MixerView<'_>) {
    let l = Layout::new(rect, view);
    let p = ink(view);
    painter.rect_filled(rect, 0.0, p.wood);

    // Channel strips clip to the scrolling region: a half-scrolled strip
    // ends at the region's edge instead of bleeding under the pinned click.
    // The pinned columns keep the raw painter — they never overhang.
    let clipped = painter.with_clip_rect(l.region);
    for (i, s) in l.strips.iter().enumerate() {
        let Some(v) = view.strips.get(i) else { continue };
        if !s.panel.is_positive() {
            continue;
        }
        let painter = if i < CHANNELS { &clipped } else { painter };
        let heard = view.heard(i);
        let typed = view.typing.filter(|(at, _)| at.0 == i).map(|(_, b)| b);
        let naming = view.naming.filter(|(at, _)| at.0 == i).map(|(_, b)| b);
        strip(painter, s, v, &p, view.routes, heard, typed, naming);
        // The selected channels' faint rim — the owner's slight light.
        if view
            .selected_tracks
            .iter()
            .any(|n| v.strip == Some(Strip::Channel(*n)))
        {
            // Measured against THIS strip's own plate, which a painted
            // channel chooses for itself. See `selection_rim`. Through the
            // same dead-zone guard the strip runs its face through, or the
            // rim would be measured against a colour the strip is not.
            let under = if v.color != 0 {
                let (r, g, b) = STRIP_COLORS[v.color.min(STRIP_COLORS.len() - 1)];
                crate::recorder_panel::legible_surface(Color32::from_rgb(r, g, b))
            } else {
                p.face
            };
            selection_outline(
                painter,
                s.panel,
                v.is_folder,
                crate::recorder_panel::selection_rim(under),
            );
        }
    }
    // The insert menu and the kind ask: a veil over
    // the strip they belong to, then their rows on top. `two_rows` because
    // they are the same drawing with different words on it.
    let two_rows = |anchor: &StripStrip, rows: &[Rect], labels: &[&str]| {
        painter.rect_filled(anchor.panel, 3.0, Color32::from_black_alpha(180));
        for (r, label) in rows.iter().zip(labels) {
            if !r.is_positive() {
                continue;
            }
            painter.rect_filled(*r, 2.0, p.face);
            painter.rect_stroke(
                *r,
                2.0,
                Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.4)),
                egui::StrokeKind::Inside,
            );
            centred(
                painter,
                *r,
                label,
                FontId::new(
                    (r.height() * 0.5).clamp(7.5, 11.0),
                    crate::fonts::courier_bold(),
                ),
                p.engrave,
            );
        }
    };
    if let Some((at, bay)) = view.fx_menu {
        if let Some(s) = l.strips.get(at.0) {
            // Row order is the hit test's: the two lists must stay twins,
            // which is why neither of them writes one down.
            let actions =
                fx_menu_actions(view.fx_menu_loaded, view.fx_menu_key, view.fx_menu_paste);
            let labels: Vec<&str> = actions.iter().map(|a| a.word()).collect();
            two_rows(s, &fx_menu_rows(s, bay, actions.len()), &labels);
        }
    }
}

/// **What a strip draws when nobody has chosen a badge for it.**
///
/// A pure function of what the strip IS, so the answer can be pinned by a
/// test rather than inferred from a painter. It was a `match` inside the
/// painting code, and what it matched on was the channel's KIND: a keyboard
/// for MIDI, a microphone for audio, a waveform for a clip lane with nothing
/// live on it. The owner's call is that the split is gone. Every track draws
/// the WAVE whatever it carries - a track that is MIDI today and audio
/// tomorrow should not change its picture underneath you, and the badge is
/// there to be chosen if a user wants it to say more than "track".
///
/// The two buses keep pictures of their own, because they are not tracks and
/// there is nothing to choose for them: the drum bus wears the kit and the
/// click wears the metronome. A FOLDER answers `None` on purpose - the
/// owner's "leave the main icon empty for users to choose" - and so does
/// anything with no source at all, the master included.
pub(crate) fn auto_icon(strip: Option<Strip>, is_folder: bool) -> Option<crate::icons::Icon> {
    // **A folder has a drawing of its own now**, and it is a folder. It used
    // to answer `None` — "leave the main icon empty for users to choose" —
    // which left a group as the one strip on the desk with a hole where every
    // other one has a badge. Choosing still overrides it, exactly as it does
    // on a track; what changed is what it falls back to.
    if is_folder {
        return Some(crate::icons::Icon::Folder);
    }
    match strip {
        Some(Strip::Channel(_)) => Some(crate::recorder_panel::DEFAULT_TRACK_ICON),
        Some(Strip::Drums) => Some(crate::icons::Icon::Drumset),
        Some(Strip::Click) => Some(crate::icons::Icon::Metronome),
        // **The master wears its own**, for the same reason. `None` here was
        // "nothing to choose", which is true and was drawn as nothing at all.
        None => Some(crate::icons::Icon::Master),
        _ => None,
    }
}

fn strip(
    painter: &Painter,
    l: &StripStrip,
    v: &StripView<'_>,
    p: &Ink,
    routes: &[(usize, String, f32)],
    heard: bool,
    typed: Option<&str>,
    naming: Option<&str>,
) {
    if v.empty {
        // The slot's own NUMBER, so an empty desk still says which column is
        // which — the mixer had none at all, and the arrange view's have been
        // there all along.
        let number = match v.strip {
            Some(Strip::Channel(n)) => Some(n + 1),
            _ => None,
        };
        empty_slot(painter, l, p, number);
        return;
    }
    // **A painted channel keeps its own face for the labels.** The whole panel
    // takes the colour, and the name and what it is carrying sit on a plate of
    // the ordinary face — so a channel can be any colour at all and its label
    // is still dark text on bone, which is the one thing that must not depend
    // on a choice somebody made for fun.
    let painted = v.color != 0;
    let face = if painted {
        let (r, g, b) = STRIP_COLORS[v.color.min(STRIP_COLORS.len() - 1)];
        // Through the dead-zone guard, because the hand-picking grid predates
        // it: three of its third tones — olive, green, teal — sit where
        // NEITHER ink clears 4.5:1, so a channel painted one of them carried
        // unreadable text on every theme, the solo/record failure class over
        // again. The table itself is append-only and saved into desks, so the
        // swatch keeps its index and the SURFACE moves at draw time — the
        // compliant 313 come through untouched.
        crate::recorder_panel::legible_surface(Color32::from_rgb(r, g, b))
    } else if heard {
        p.face
    } else {
        // **Not merely unlit: silent.** A strip that solo has taken out of the
        // mix is not producing anything, and drawing it exactly like one that
        // is would make solo a light rather than a routing decision.
        p.face.gamma_multiply(0.62)
    };
    painter.rect_filled(l.panel, 3.0, face);
    painter.rect_stroke(
        l.panel,
        3.0,
        Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.30)),
        egui::StrokeKind::Inside,
    );
    // **A FOLDER is chopped at its top-right** - a 90-degree notch, and
    // where the chop's underside lands is exactly where its children's
    // strips begin (`CHILD_INDENT`): the step the eye follows from parent
    // to child.
    if v.is_folder {
        let notch = Rect::from_min_max(
            Pos2::new(l.panel.right() - l.panel.width() * NOTCH_W, l.panel.top() - 1.0),
            Pos2::new(l.panel.right() + 1.0, l.panel.top() + CHILD_INDENT),
        );
        painter.rect_filled(notch, 0.0, p.wood);
        let edge = Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.30));
        painter.line_segment(
            [
                Pos2::new(notch.left(), l.panel.top()),
                Pos2::new(notch.left(), notch.bottom()),
            ],
            edge,
        );
        painter.line_segment(
            [
                Pos2::new(notch.left(), notch.bottom()),
                Pos2::new(l.panel.right(), notch.bottom()),
            ],
            edge,
        );
    }

    // **Every size on this panel is a fraction of a strip**, which lands
    // between seven and twelve points — small enough that the face's own cap
    // height decides whether it can be read at all. `size_scale` is one on
    // the face the numbers were chosen against and larger on a smaller face;
    // see `fonts::cap_scale` for the measurements and the owner's report that
    // this view and the arrangement were "hard to read".
    let fs = crate::fonts::size_scale(painter.ctx());
    let cap = |size: f32| FontId::new(size * fs, crate::fonts::courier_bold());
    let plain = |size: f32| FontId::new(size * fs, crate::fonts::courier());
    let h = l.panel.height();
    // **Everything printed ON the panel, in ink the panel can carry.**
    //
    // The name sits on a plate of the ordinary face and is always dark on
    // bone. Nothing else does: the icon, the second line, the decibel readout
    // and the fader's scale are all painted straight onto the channel — so on
    // a channel somebody painted dark red they were dark on dark, which is
    // where the keyboard icon and "+0.0 dB" went when the desk got colours.
    // Chosen from the face as composited, for the reason `flatten` gives.
    let solid = flatten(face, p.wood);
    let ink = print_on(solid, p);

    // The plate the label sits on, so it reads on any colour.
    // **The name and nothing else.** It used to run down to the second line
    // when there was one, which was a plate sized for two lines on a channel
    // with one — and now the icon sits between them, so a plate that reached
    // the second line would paint over the icon as well.
    // **A child sits a step lower.** The arrangement indents a grouped
    // lane sideways; a vertical strip has no sideways, so the plate and its
    // name drop three points instead — enough to read as "under", too
    // little to cost the layout anything.
    let plate = Rect::from_min_max(
        Pos2::new(l.panel.left() + 3.0, l.name.top() - 2.0),
        Pos2::new(l.panel.right() - 3.0, l.name.bottom() + 2.0),
    );
    // **Under every name, painted or not.** It was drawn only on a coloured
    // channel, where it exists to keep the label legible on any background —
    // but a name is a FIELD now, and a field needs an edge to look like one
    // before it is clicked. The two reasons want the same rectangle.
    let _ = painted;
    if plate.is_positive() {
        painter.rect_filled(plate, 2.0, p.face);
        // **The folder TAB, and it is the collapse control now** - top-left
        // of the plate, where it always sat, with the open/shut arrow
        // inside its body. The icon spot below stays free for whatever
        // badge the user chooses.
        if l.folder.is_positive() {
            let b = l.folder;
            let hh = (b.height() * 0.72).clamp(8.0, 13.0);
            let ww = hh * 1.5;
            let at = Pos2::new(b.left(), b.center().y - hh * 0.5);
            painter.rect_filled(
                Rect::from_min_size(at, Vec2::new(ww * 0.45, hh * 0.32)),
                1.0,
                p.engrave,
            );
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(at.x, at.y + hh * 0.26),
                    Vec2::new(ww, hh * 0.74),
                ),
                1.0,
                p.engrave,
            );
            // The arrow, in the plate's own face: down = open, right = shut.
            let cx = at.x + ww * 0.5;
            let cy = at.y + hh * 0.62;
            let a = (hh * 0.24).max(2.0);
            let tri = if v.folder_shut {
                vec![
                    Pos2::new(cx - a * 0.5, cy - a),
                    Pos2::new(cx + a, cy),
                    Pos2::new(cx - a * 0.5, cy + a),
                ]
            } else {
                vec![
                    Pos2::new(cx - a, cy - a * 0.5),
                    Pos2::new(cx + a, cy - a * 0.5),
                    Pos2::new(cx, cy + a),
                ]
            };
            painter.add(egui::Shape::convex_polygon(tri, p.face, Stroke::NONE));
        }
    }
    let name_rect = l.name;
    // **The band's own icons, under the names.** The microphone and the
    // waveform are how the input and the backing track are already labelled
    // one band up; drawing them again here is what makes the three kinds of
    // source read as three kinds of thing rather than as three rows of text.
    // Under, so that every label on the desk — the master's included — sits at
    // one height.
    if let Some(buf) = naming {
        painter.rect_filled(l.name, 2.0, p.face);
        painter.rect_stroke(
            l.name,
            2.0,
            Stroke::new(1.0_f32, p.lit),
            egui::StrokeKind::Inside,
        );
        centred(
            painter,
            l.name,
            &format!("{buf}_"),
            cap((h * 0.032).clamp(7.5, 12.0)),
            p.engrave,
        );
    }
    // A chosen badge beats the strip's own picture, and only a CHANNEL can
    // wear one - the buses always draw for themselves. A code this build has
    // no drawing for (a NEWER install's desk) falls through to `auto_icon`
    // rather than leaving the spot blank.
    let badge = v.icon != 0
        && matches!(v.strip, Some(Strip::Channel(_)))
        && crate::recorder_panel::draw_badge(painter, l.icon, ink, face, v.icon);
    let named = if badge {
        true
    } else {
        match auto_icon(v.strip, v.is_folder) {
            Some(icon) => {
                crate::icons::draw(painter, l.icon, ink, icon);
                true
            }
            // A folder with no chosen badge keeps its icon spot EMPTY - the
            // owner's "leave the main icon empty for users to choose". The
            // picker still answers a press there. So does the master, which
            // has no icon rect at all.
            None => v.is_folder,
        }
    };
    let _ = named;
    if naming.is_none() {
        // Centred on the TRACK, never pushed by the folder tab - the
        // owner's call: the tab marks the corner, the name keeps the strip.
        centred(painter, name_rect, v.name, cap((h * 0.032).clamp(7.5, 12.0)), p.engrave);
    }
    // **The × that takes the channel off the desk.** Subtle — thin strokes in
    // faded ink, on the plate the name shares — but where every window in the
    // world puts its close, which is what makes it clear.
    if l.close.is_positive() {
        let c = l.close.center();
        let arm = l.close.width().min(l.close.height()) * 0.26;
        let ink = p.engrave.gamma_multiply(0.5);
        let w = (arm * 0.45).clamp(1.0, 1.8);
        painter.line_segment(
            [Pos2::new(c.x - arm, c.y - arm), Pos2::new(c.x + arm, c.y + arm)],
            Stroke::new(w, ink),
        );
        painter.line_segment(
            [Pos2::new(c.x - arm, c.y + arm), Pos2::new(c.x + arm, c.y - arm)],
            Stroke::new(w, ink),
        );
    }
    // **The input chip is a PANEL, like the name plate above it** — bone
    // ground, an edge, dark ink — so the one pressable row under the icon
    // looks pressable on any colour a channel is painted. A channel with
    // no input yet says so; the press is how one is chosen.
    if l.input_chip.is_positive() {
        // Its own theme role: the chip says what is PLUGGED IN, which is a
        // different question from what the strip is made of, and themes give
        // it a signal colour (Studio's sage lamp, Hydra's phosphor).
        painter.rect_filled(l.input_chip, 2.0, p.chip);
        painter.rect_stroke(
            l.input_chip,
            2.0,
            Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.4)),
            egui::StrokeKind::Inside,
        );
        let word = if v.detail.is_empty() { "NO INPUT" } else { v.detail };
        centred(
            painter,
            l.input_chip,
            word,
            plain((h * 0.024).clamp(7.0, 11.0)),
            p.engrave.gamma_multiply(if v.detail.is_empty() { 0.55 } else { 0.85 }),
        );
    } else if !v.detail.is_empty() && !l.met.is_positive() {
        // Not under the click's chip: the click's detail IS its sound's name,
        // and the chip above is already drawing it on a panel.
        centred(
            painter,
            l.detail,
            v.detail,
            plain((h * 0.024).clamp(7.0, 11.0)),
            ink.gamma_multiply(0.72),
        );
    }
    // **A FOLDER's chip, wearing the panel a track's input wears** — the
    // same ground, the same edge, the same band — because it answers the
    // same question the chip under every other badge answers: what is this
    // strip. It is a READOUT and not a door: a group has no input to pick,
    // so it is drawn here and registered in no target list.
    if l.folder_chip.is_positive() {
        painter.rect_filled(l.folder_chip, 2.0, p.chip);
        painter.rect_stroke(
            l.folder_chip,
            2.0,
            Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.4)),
            egui::StrokeKind::Inside,
        );
        centred(
            painter,
            l.folder_chip,
            "FOLDER",
            plain((h * 0.024).clamp(7.0, 11.0)),
            p.engrave.gamma_multiply(0.85),
        );
    }
    // **The CLICK's own sound**, in the band a channel spends on its input
    // chip and wearing the same panel — it is the same kind of control,
    // "what is feeding this strip", and it opens the metronome panel. The
    // owner's "add the met menu from arrange view as its own button ... it
    // should read as the file chosen".
    if l.met.is_positive() {
        painter.rect_filled(l.met, 2.0, p.chip);
        painter.rect_stroke(
            l.met,
            2.0,
            Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.4)),
            egui::StrokeKind::Inside,
        );
        centred(
            painter,
            l.met,
            // **The strip's own detail**, which is what the app puts the
            // playing sound's name in — the chip used to be handed a second
            // copy of the same string through its own parameter, and two
            // roads to one word is one of them eventually saying something
            // else.
            v.detail,
            plain((h * 0.024).clamp(7.0, 11.0)),
            p.engrave.gamma_multiply(0.85),
        );
    }

    // ── Routing: the button, and a strip per route ─────────────────────────
    //
    // The button says what it opens and nothing else; the strips under it are
    // the routes themselves, each one a label and its own fader. A route is a
    // send, so the strip is deliberately SMALL - it is not a second channel,
    // it is a tap off this one.
    if l.routing.is_positive() {
        let lit = v.route_count > 0;
        painter.rect_filled(l.routing, 2.0, if lit { p.face } else { shade_face(p.face, 0.94) });
        painter.rect_stroke(
            l.routing,
            2.0,
            Stroke::new(1.0_f32, p.engrave),
            egui::StrokeKind::Inside,
        );
        let label = if lit {
            format!("ROUTING  {}", v.route_count)
        } else {
            "ROUTING".to_owned()
        };
        let size = (l.routing.height() * 0.52).clamp(6.0, 10.0);
        painter.text(
            l.routing.center(),
            egui::Align2::CENTER_CENTER,
            &label,
            FontId::new(size, crate::fonts::courier_bold()),
            p.engrave,
        );
    }
    if let Some(Strip::Channel(n)) = v.strip {
        let mine: Vec<&(usize, String, f32)> =
            routes.iter().filter(|(from, _, _)| *from == n).collect();
        for (i, r) in l.routes.iter().enumerate() {
            let Some((_, label, gain)) = mine.get(i) else {
                break;
            };
            if !r.is_positive() {
                continue;
            }
            // The well, then the fill to the route's own level, then the
            // name over it: the same picture as a fader lying down, so it
            // reads as a level without needing a second idiom.
            painter.rect_filled(*r, 1.0, p.slot);
            let filled = Rect::from_min_max(
                r.min,
                Pos2::new(r.left() + r.width() * gain.clamp(0.0, 1.0), r.bottom()),
            );
            if filled.width() > 0.5 {
                painter.rect_filled(filled, 1.0, p.lit.gamma_multiply(0.55));
            }
            let size = (r.height() * 0.62).clamp(5.0, 9.0);
            painter.text(
                Pos2::new(r.left() + 3.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                label,
                FontId::new(size, crate::fonts::courier_bold()),
                p.face,
            );
        }
    }

    // A dimmed channel is dimmed by a veil rather than by a paler face, so
    // painting one does not cost it its "not heard" state.
    if !heard {
        painter.rect_filled(l.panel, 3.0, Color32::from_black_alpha(90));
    }

    // The master gets the full scale; a channel gets the four marks its width
    // has room for. `strip: None` IS the master — see `StripView`.
    let ticks: &[f32] = if v.strip.is_none() {
        &MASTER_TICKS
    } else {
        &METER_TICKS
    };
    // The face as it will LOOK, not as it is written down: a silent strip's is
    // translucent, and the scale has to be chosen against what is on screen.
    meter(
        painter,
        l.meter,
        v.peak,
        v.stereo,
        v.gr_db,
        heard,
        ticks,
        solid,
        p,
    );

    if l.send.is_positive() {
        // **Value, knob, label — all INSIDE the band.** The caps used to
        // hang above the cells and the rack clipped them to illegibility
        // on the owner's screen; the cell now carries its own three rows.
        let carve = |r: Rect| {
            let vh = (r.height() * 0.26).clamp(8.0, 12.0);
            let lh = (r.height() * 0.22).clamp(7.0, 10.0);
            let value = Rect::from_min_max(r.min, Pos2::new(r.right(), r.top() + vh));
            let label = Rect::from_min_max(Pos2::new(r.left(), r.bottom() - lh), r.max);
            let knob = Rect::from_min_max(
                Pos2::new(r.left(), value.bottom()),
                Pos2::new(r.right(), label.top()),
            );
            (value, knob, label)
        };
        let cap_ink = ink;
        let word = |painter: &Painter, band: Rect, text: &str, main: bool| {
            if !band.is_positive() {
                return;
            }
            painter.text(
                band.center(),
                egui::Align2::CENTER_CENTER,
                text,
                FontId::new(
                    (band.height() * 0.92).clamp(6.5, 9.5),
                    crate::fonts::courier(),
                ),
                cap_ink.gamma_multiply(if main { 0.9 } else { 0.55 }),
            );
        };
        let (sv, sk, sl) = carve(l.send);
        send_knob(painter, sk, v.send, p, v.strip.is_none());
        word(painter, sv, &format!("{:.0}%", v.send.clamp(0.0, 1.0) * 100.0), true);
        word(painter, sl, "SEND", false);
        if l.pan.is_positive() {
            let (pv, pk, pl) = carve(l.pan);
            pan_knob(painter, pk, v.pan, p, v.strip.is_none());
            let w = (v.pan.clamp(-1.0, 1.0) * 50.0).round() as i32;
            let pw = if w == 0 {
                "C".to_owned()
            } else if w < 0 {
                format!("L{}", -w)
            } else {
                format!("R{w}")
            };
            word(painter, pv, &pw, true);
            word(painter, pl, "PAN", false);
        }
    }
    // **A rack: an ivory field with three strips down it.**
    //
    // On the ordinary face rather than in the channel's own colour, for the
    // reason the name plate is: a plugin's name is text to be read, and text
    // to be read is dark on bone whatever somebody painted the channel. The
    // field is drawn once behind all three so the rack reads as one object
    // with three bays in it, which is what a rack is.
    let bays: Vec<Rect> = l.inserts.iter().copied().filter(|r| r.is_positive()).collect();
    if let (Some(first), Some(last)) = (bays.first(), bays.last()) {
        let field = Rect::from_min_max(
            Pos2::new(first.left() - 2.0, first.top() - 1.0),
            Pos2::new(last.right() + 2.0, last.bottom() + 1.0),
        );
        painter.rect_filled(field, 2.0, p.face);
        painter.rect_stroke(
            field,
            2.0,
            Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.30)),
            egui::StrokeKind::Inside,
        );
    }
    for (n, cell) in l.inserts.iter().enumerate() {
        if !cell.is_positive() {
            continue;
        }
        let name = v.inserts.get(n).copied().unwrap_or("");
        let filled = !name.is_empty();
        // The hairline between bays, so three empty ones are three bays and
        // not one tall blank. Not under the last, which would be a line along
        // the bottom edge of the field.
        // Against the DRAWN count: gating on INSERTS left the master's
        // (and now a folder's) bays 2..4 with no line under them.
        if l.inserts.get(n + 1).is_some_and(|c| c.is_positive()) {
            painter.line_segment(
                [
                    Pos2::new(cell.left() + 1.0, cell.bottom() + 1.0),
                    Pos2::new(cell.right() - 1.0, cell.bottom() + 1.0),
                ],
                Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.18)),
            );
        }
        let font = FontId::new(
            (cell.height() * 0.46).clamp(6.5, 10.0),
            crate::fonts::courier_bold(),
        );
        if filled {
            // The tab at the left edge is the "loaded" light the whole chip
            // used to be. Blue on bone says the same thing and leaves the
            // width for the name, which is the point of stacking them.
            //
            // **And AMBER when the bay is bypassed** — the mute colour, out
            // of the same `Ink` the M switch two rows down reads, because
            // "this is loaded and you are not hearing it" is the same
            // sentence a muted channel says. A light that changes colour
            // rather than a mark that appears somewhere else: the eye is
            // already on this bar to know whether the bay is filled, so it
            // is the one place a second fact costs nothing to find.
            //
            // Through `lit_on`, which the desk uses wherever a fixed light
            // sits on a themed ground: raw amber on an amber theme is a tab
            // that vanishes on precisely the themes somebody chose for being
            // warm. See the all-theme sweep in this file's tests.
            let bypassed = v.bypassed.get(n).copied().unwrap_or(false);
            let tab = if bypassed {
                crate::recorder_panel::lit_on(p.mute, p.face)
            } else {
                p.lit
            };
            painter.rect_filled(
                Rect::from_min_max(
                    cell.min,
                    Pos2::new(cell.left() + 3.0, cell.bottom()),
                ),
                1.0,
                tab,
            );
            let text = Rect::from_min_max(Pos2::new(cell.left() + 6.0, cell.top()), cell.max);
            // **The name goes quiet with it**, the second half of the
            // arrangement's own bypass grammar (`AutoView::bypassed`: the M
            // plate lights AND the row's word is pulled toward its ground).
            // Two signals rather than one because a colour alone is a thing
            // you have to have been told; a name that has gone grey reads as
            // "off" to anybody. Blended toward the face rather than made
            // transparent — `quiet` keeps 4.5:1 on whatever the theme put
            // underneath, which `gamma_multiply` cannot promise since it
            // lowers alpha and lets the panel through.
            let word_ink = if bypassed {
                crate::arrangement_panel::quiet(p.engrave, p.face, 0.45)
            } else {
                p.engrave
            };
            centred(
                painter,
                text,
                &shorten_plugin(painter, text.width(), name, &font),
                font.clone(),
                word_ink,
            );
            // A keyed bay wears a dot in its corner — the loaded-tab's own
            // light, so "listening to another channel" reads in the same
            // colour "loaded" does, and the name loses no width to it.
            if v.keyed.get(n).copied().unwrap_or(false) {
                painter.circle_filled(
                    Pos2::new(cell.right() - 4.0, cell.top() + 4.0),
                    1.7,
                    p.lit,
                );
            }
        } else {
            // Big enough to be an offer. At the chip's own text size it was a
            // speck in the middle of an empty bay, which reads as dirt on the
            // panel rather than as somewhere to put a plugin.
            let plus = FontId::new(
                (cell.height() * 0.62).clamp(9.0, 15.0),
                crate::fonts::courier_bold(),
            );
            centred(painter, *cell, "+", plus, p.engrave.gamma_multiply(0.45));
        }
    }

    fader(painter, l.fader, v.gain, solid, p);
    let db_font = plain((h * 0.026).clamp(8.0, 12.0));
    match typed {
        Some(buf) => {
            painter.rect_filled(l.db, 2.0, p.face);
            painter.rect_stroke(
                l.db,
                2.0,
                Stroke::new(1.0_f32, p.lit),
                egui::StrokeKind::Inside,
            );
            centred(painter, l.db, &format!("{buf}_"), db_font, p.engrave);
        }
        None => centred(painter, l.db, &gain_text(v.gain), db_font, ink),
    }

    if l.mute.is_positive() {
        switch(painter, l.mute, "M", v.muted, p.mute, p);
        switch(painter, l.solo, "S", v.soloed, p.solo, p);
    }
    // **Ø — polarity**, above the switch row and drawn like it, so a hand
    // that has learned M and S reads this without being told.
    //
    // On the theme's ENGRAVE ink rather than a light of its own: it is
    // neither a mute nor a solo, and a third warning colour on a strip that
    // already carries two would be read as a third kind of danger. The same
    // choice the T switch below it makes, for the same reason.
    if l.phase.is_positive() {
        switch(painter, l.phase, "\u{d8}", v.phase, p.engrave, p);
    }
    if l.arm.is_positive() {
        switch(painter, l.arm, "R", v.armed, p.arm, p);
    }
    // **T, and it was laid out and hit-tested and never drawn.** A control
    // you can press and cannot see is worse than one that is missing: the
    // owner found it by pressing the empty paint beside R.
    //
    // On the theme's own INK rather than a light of its own, exactly as the
    // timeline's header carries it — it is neither a mute nor a solo, and a
    // third red on a strip is one of the other two being misread.
    // **The master's mono fold**, at the foot of the one strip that has no
    // M/S/R row to put there. It says what it IS rather than what pressing
    // it does — a switch that reads MONO while the output is stereo is a
    // switch nobody can read at a glance.
    if l.mono.is_positive() {
        switch(
            painter,
            l.mono,
            if v.mono { "MONO" } else { "ST" },
            v.mono,
            p.engrave,
            p,
        );
    }
    if l.theory.is_positive() {
        // **The NOTE, not a letter** - the owner's call, and the character
        // is the one the MIDI EDITOR's gridbox shows for the quarter
        // division (`\u{266a}`), which renders through egui's fallback face
        // on every platform this app ships to. It used to say "the grid box"
        // with no panel named, and that reference has since gone half stale:
        // the ARRANGE gridbox spells that rung "1/4" now, because a beginner
        // reads a fraction faster than a five-point picture (the argument is
        // on `arrangement_panel::grid_label_text`). This switch is not a
        // grid readout and keeps the glyph either way; the pointer is simply
        // aimed at the surface where it is still checkable. Drawn like a
        // switch: framed, lit while the channel's MIDI feeds the theory view.
        // Lit in the SELECT colour, not engrave: the owner pressed the old
        // engrave-lit letter and saw nothing change.
        // **DRAWN, not set** — see `recorder_panel::draw_note`, and the
        // timeline's own switch, which draws the same note the same way. A
        // notehead is an oblong and it is tilted, and no glyph in any face
        // this app bundles is both.
        {
            let (ground, ink) = switch_face(v.to_theory, p.lit, p);
            if l.theory.is_positive() {
                painter.rect_filled(l.theory, 2.0, ground);
                crate::recorder_panel::draw_note(
                    painter,
                    l.theory.shrink(l.theory.width() * 0.12),
                    ink,
                );
            }
        }
    }
}

/// The same menu with however many rows it needs — a keyed-capable bay
/// carries a third. One function for any count, so the two-row and
/// three-row menus cannot disagree about where a row is.
/// **How heavy the selected strip's rim is.**
///
/// It was 1.2 and the owner could not find the selection on a full desk -
/// "make the selector overall bigger in mixer view, just a slight bump for
/// visibility". A bump and not a frame: a rim thick enough to read as a
/// border would be a strip that changes SIZE when it is picked, and the one
/// thing a desk must not do is move under the hand.
const RIM_W: f32 = 2.2;

/// The selected strip's rim, in the shape the strip actually is.
///
/// **A folder is not a rectangle.** Its top-right corner is chopped
/// (`CHILD_INDENT`, and see the notch in `strip`), so a rounded rectangle
/// drawn round one cut the corner off the wrong way: the rim crossed the
/// empty paint above the chop and told the eye the group reached somewhere
/// it does not. The outline follows the notch instead, which is the same
/// six points the chop itself is drawn from.
///
/// One function for both shapes so the selection cannot be drawn one way on
/// a track and another on a group by two pieces of code that disagree.
fn selection_outline(painter: &Painter, panel: Rect, folder: bool, ink: Color32) {
    if !panel.is_positive() {
        return;
    }
    let stroke = Stroke::new(RIM_W, ink);
    if !folder {
        painter.rect_stroke(panel, 3.0, stroke, egui::StrokeKind::Inside);
        return;
    }
    // Half the stroke inward on every edge, because a closed line is
    // CENTRED on its path where `StrokeKind::Inside` is not - without it
    // half the rim hangs over the strip's own border and over the gap.
    let i = RIM_W * 0.5;
    let r = Rect::from_min_max(
        Pos2::new(panel.left() + i, panel.top() + i),
        Pos2::new(panel.right() - i, panel.bottom() - i),
    );
    let chop_x = r.right() - panel.width() * NOTCH_W;
    let chop_y = r.top() + CHILD_INDENT;
    painter.add(egui::Shape::closed_line(
        vec![
            Pos2::new(r.left(), r.top()),
            Pos2::new(chop_x, r.top()),
            Pos2::new(chop_x, chop_y),
            Pos2::new(r.right(), chop_y),
            Pos2::new(r.right(), r.bottom()),
            Pos2::new(r.left(), r.bottom()),
        ],
        stroke,
    ));
}

fn fx_menu_rows(l: &StripStrip, bay: usize, count: usize) -> Vec<Rect> {
    let panel = l.panel;
    if !panel.is_positive() {
        return vec![Rect::NOTHING; count];
    }
    let anchor = l
        .inserts
        .get(bay)
        .copied()
        .filter(Rect::is_positive)
        .unwrap_or(panel);
    // Taller than the chip it serves: the chip's height is a reading size,
    // and these are pressing sizes.
    let gap = 3.0;
    let n = count.max(1) as f32;
    let h = (anchor.height() * 1.6)
        .clamp(16.0, 28.0)
        .min((panel.height() - 8.0 - gap * (n - 1.0)).max(2.0) / n);
    let w = (panel.width() - 8.0).max(1.0);
    let total = h * n + gap * (n - 1.0);
    let top = anchor
        .top()
        .clamp(panel.top() + 4.0, (panel.bottom() - 4.0 - total).max(panel.top() + 4.0));
    (0..count)
        .map(|i| {
            Rect::from_min_size(
                Pos2::new(panel.left() + 4.0, top + (h + gap) * i as f32),
                Vec2::new(w, h),
            )
        })
        .collect()
}



/// Somewhere to put an instrument: an outline and a plus.
///
/// **An outline rather than a filled panel**, because it is not a channel yet
/// and drawing it like one would be five faders that do nothing. The rack on
/// the right of the band says "empty (click to load)"; this says the same
/// thing in the shape a mixer says it in.
/// **The ground an unoccupied slot paints — the desk's own wood, recessed.**
///
/// It was one fixed near-black grey on all nineteen themes, and on a pale desk
/// that is a hole nobody can miss. On a dark one it is the wood again:
/// measured across the factory set it sat at 1.01:1 against the band on
/// Silverface — the same colour to two decimal places — 1.09:1 on Rhodes and
/// Phosphor, and under 1.40:1 on twelve of the nineteen. The owner: "they are
/// all gray, and some are imperceptible because of that."
///
/// So it is DERIVED, the way the arrangement's empty LANE already derives its
/// ground from the lane field it cuts into: away from whichever end of the
/// scale the wood is at — toward white under a dark desk, toward black under
/// a pale one — by a step that reads either way, and then walked until it
/// clears a floor. An empty slot keeps its theme's character and is still a
/// hole in it.
///
/// Named because the ink on it is measured against it — see `empty_slot_ink`.
pub fn empty_ground_for(wood: Color32) -> Color32 {
    /// A large flat surface has no text threshold to answer to, but it does
    /// have to be seen from across the desk, and 1.4:1 is not seen.
    const FLOOR: f32 = 1.7;
    crate::recorder_panel::distinct_surface_at(recess(wood, 0.20), wood, FLOOR)
}

/// One step AWAY from a surface, in whichever direction that surface has room
/// to move: toward white when it is dark, toward black when it is pale.
///
/// A multiplier cannot do this. Scaling brightness separates a pale ground
/// nicely and does almost nothing to a near-black one — 0.78 of `0x1C2C16` is
/// still `0x1C2C16` to the eye — which is exactly how twelve of these themes
/// ended up with an invisible slot. The arrangement panel learned the same
/// lesson about its grid and its seams and answers it the same way.
fn recess(c: Color32, t: f32) -> Color32 {
    let toward = if crate::recorder_panel::wants_light_ink(c) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    mix(c, toward, t)
}

/// What is printed on [`EMPTY_GROUND`]: the circled plus, and the slot number.
///
/// **One value for both**, because they sit on the same ground half an inch
/// apart and two answers is two chances for one of them to be wrong. The
/// theme's face, quietened — the value the plus has always used and which the
/// owner confirmed reads — and then pushed until it clears the ground, which
/// costs nothing on the themes where it already did.
/// [`empty_slot_ink`] from the face alone, for the test that walks the themes.
pub fn empty_slot_ink_for(face: Color32, ground: Color32) -> Color32 {
    crate::recorder_panel::distinct_surface_at(face.gamma_multiply(0.55), ground, 3.0)
}

fn empty_slot_ink(p: &Ink) -> Color32 {
    empty_slot_ink_for(p.face, p.empty_ground)
}

fn empty_slot(painter: &Painter, l: &StripStrip, p: &Ink, number: Option<usize>) {
    // The empty slot's own ground, cut out of this theme's wood — under the
    // circled plus and the quiet space around it.
    painter.rect_filled(l.panel, 3.0, p.empty_ground);
    // **And its rim is measured against the ground it rims.** It was the
    // strip FACE at 22% alpha, which is a colour belonging to the panels
    // either side of this hole rather than to the hole; on a dark theme it
    // fell to within a point of the wood and the slot had no edge at all.
    painter.rect_stroke(
        l.panel,
        3.0,
        Stroke::new(1.0_f32, p.empty_edge),
        egui::StrokeKind::Inside,
    );
    // **The number at the top**, in the same ink the plus below it wears —
    // which is the value that already reads on this ground on every theme, so
    // there is no second answer to keep in step with the first.
    if let Some(n) = number {
        let cap = Rect::from_min_max(
            l.panel.min,
            Pos2::new(l.panel.right(), l.panel.top() + (l.panel.height() * 0.09).min(16.0)),
        );
        if cap.is_positive() {
            let size = (cap.height() * 0.78).clamp(7.0, 12.0);
            painter.text(
                Pos2::new(cap.center().x, cap.center().y),
                egui::Align2::CENTER_CENTER,
                format!("{n}"),
                FontId::new(size, crate::fonts::courier()),
                empty_slot_ink(p),
            );
        }
    }
    let r = l.add;
    if !r.is_positive() {
        return;
    }
    // **A circled plus, and the circle IS the button**: press it and a
    // bare track exists — no menu, no icon, a NO INPUT chip. The ring is
    // what says "this much is pressable" on a panel that otherwise only
    // answers the right-click.
    let c = r.center();
    let ink = empty_slot_ink(p);
    let radius = r.width().min(r.height()) * 0.5 - 1.0;
    let w = (radius * 0.11).clamp(1.2, 2.5);
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

/// A vertical bar, filling from the bottom.
///
/// **Narrow, and centred in the room it was given.** A meter drawn the full
/// width of the strip is not a meter, it is a panel with a colour in it: at
/// rest it is the largest thing on the channel and it reads as something that
/// failed to load rather than as a level of nothing.
/// The marks down the side of a CHANNEL's meter, in decibels.
///
/// **Four, because a strip is a fifth of the width the master has.** More
/// labels in a column that narrow is a grey smear, and these are the ones
/// anybody actually reads a level against.
const METER_TICKS: [f32; 4] = [0.0, -12.0, -24.0, -48.0];

/// And the MASTER's, which has the room for a scale you can read a number off.
///
/// The standard set, thinned where the fader's own curve crowds it: the marks
/// near unity are worth having one every three decibels and the ones at the
/// bottom are not.
///
/// **Neither END of the travel gets one.** +12 and -60 are where the fader
/// stops, so a label there sits half off the meter and reads against nothing.
/// `every_meter_tick_lands_where_the_fader_would_put_it` caught +12 the first
/// time this was written.
const MASTER_TICKS: [f32; 9] = [6.0, 0.0, -3.0, -6.0, -12.0, -18.0, -24.0, -36.0, -48.0];

fn meter(
    painter: &Painter,
    r: Rect,
    peak: [f32; 2],
    stereo: bool,
    gr_db: f32,
    heard: bool,
    ticks: &[f32],
    // **The strip's own face**, because the scale is printed on it. The
    // numbers used to be a faded ivory, which is legible on a channel somebody
    // painted and completely invisible on one they did not: ivory on ivory.
    // Every unpainted channel on the desk had a meter with no scale beside it,
    // and the two kinds of strip looked like two different controls.
    face: Color32,
    p: &Ink,
) {
    if !r.is_positive() {
        return;
    }
    // **Tall and thin, with the ticks to its left.** A meter you read a number
    // off wants a scale beside it, and the scale is what decides how much of
    // the cell the bars can have.
    let scale_w = (r.width() * 0.5).clamp(14.0, 30.0);
    let bars = Rect::from_min_max(Pos2::new(r.left() + scale_w, r.top()), r.max);
    // **Legible or absent.** The floor used to be 5.5 points, which is not a
    // number anybody reads, it is a grey mark that means "there was a number
    // here". Sized from the column the numbers actually sit in.
    let font = FontId::new((scale_w * 0.46).clamp(6.5, 9.0), crate::fonts::courier());
    for &db in ticks {
        let t = gain_to_fader(10f32.powf(db / 20.0)).clamp(0.0, 1.0);
        let y = bars.bottom() - bars.height() * t;
        painter.text(
            Pos2::new(r.left() + scale_w - 3.0, y),
            egui::Align2::RIGHT_CENTER,
            // Unsigned: the ladder descends from 0 and draws it. See
            // `recorder::meter_number`.
            crate::recorder::meter_number(db),
            font.clone(),
            print_on(face, p).gamma_multiply(0.62),
        );
        painter.line_segment(
            [Pos2::new(bars.left(), y), Pos2::new(bars.right(), y)],
            Stroke::new(0.5_f32, p.face.gamma_multiply(0.18)),
        );
    }

    // One bar or two, and that is a claim about the SOURCE rather than about
    // the samples: a mono microphone drawn as two identical bars says it is
    // stereo, which is the one thing a meter must not do.
    let lanes = if stereo { 2 } else { 1 };
    let gap = 2.0;
    let w = ((bars.width() - gap * (lanes as f32 - 1.0)) / lanes as f32).max(2.0);
    for lane in 0..lanes {
        let x = bars.left() + lane as f32 * (w + gap);
        let cell = Rect::from_min_max(
            Pos2::new(x, bars.top()),
            Pos2::new(x + w, bars.bottom()),
        );
        painter.rect_filled(cell, 2.0, p.track);
        // **Gain reduction hangs from the top of EVERY lane**, the way it does
        // on the band's own master, because that is the direction it means:
        // the ceiling coming down. One strip down the right-hand edge of the
        // pair was the first version and it read as the right channel being
        // limited on its own — the limiter is stereo-linked and takes the same
        // decibels off both sides, so showing it on one is a picture of
        // something that does not happen.
        //
        // Before the bar and outside the `heard` test: a limiter working on a
        // channel you have muted is still worth seeing, and a limiter working
        // on silence is not possible.
        if gr_db > 0.01 {
            let t = (gr_db / 24.0).clamp(0.0, 1.0);
            let w = (cell.width() * 0.3).clamp(2.0, 5.0);
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(cell.right() - w, cell.top()),
                    Pos2::new(cell.right(), cell.top() + cell.height() * t),
                ),
                1.0,
                p.gr,
            );
        }
        let v = if stereo { peak[lane.min(1)] } else { peak[0].max(peak[1]) };
        if !heard || v <= 0.0 {
            continue;
        }
        // The same curve the faders use, so a strip at unity reads at the same
        // height on both — a meter on a different scale from the fader beside
        // it is two rulers on one wall.
        let t = gain_to_fader(v).clamp(0.0, 1.0);
        let top = cell.bottom() - cell.height() * t;
        painter.rect_filled(
            Rect::from_min_max(Pos2::new(cell.left(), top), cell.max),
            2.0,
            if v >= 0.9 { p.meter_hi } else { p.meter_lo },
        );
    }

}

/// **The master's dials, un-themed, the same on every desk.**
///
/// The owner asked for the master to be the one strip you cannot mistake:
/// its colour red on every theme, and its dials the black-outlined silver
/// cap the Analog's panel wears — "black outlined silver dial with black
/// strip". So the master's knobs are dressed here rather than from the
/// palette: an outer ring of near-black, a spun-grey face inside it, and a
/// pointer in the same near-black, whichever theme is loaded and whatever it
/// has done to everything around them. The channels keep the theme's own
/// dress, which is what makes the master read as the odd one out — the whole
/// point of it.
const MASTER_DIAL_RING: Color32 = Color32::from_rgb(0x10, 0x10, 0x10);
const MASTER_DIAL_FACE: Color32 = Color32::from_rgb(0xb4, 0xb6, 0xba);

/// The master's dress, or the theme's, by which strip this is.
fn dial_ink(p: &Ink, master: bool) -> (Color32, Color32, Color32, Color32) {
    if master {
        (
            MASTER_DIAL_RING,
            MASTER_DIAL_FACE,
            MASTER_DIAL_RING,
            MASTER_DIAL_RING,
        )
    } else {
        (p.track, p.cap, p.lit, p.faint)
    }
}

/// A knob, drawn as an arc rather than a dial: it is a percentage, and an arc
/// says how much where a pointer says where.
fn send_knob(painter: &Painter, r: Rect, amount: f32, p: &Ink, master: bool) {
    let (track, cap, lit, faint) = dial_ink(p, master);
    // **Smaller than the cell it sits in.** The first version filled the row
    // and read as the biggest thing on the channel, which a send that is
    // usually at zero has no business being.
    let d = r.height().min(r.width() * 0.72) * 0.78;
    if d <= 2.0 {
        return;
    }
    let c = Pos2::new(r.center().x, r.center().y);
    painter.circle_filled(c, d * 0.5, track);
    let t = amount.clamp(0.0, 1.0);
    // A quarter-turn short of each stop, like a real pot.
    let sweep = std::f32::consts::PI * 1.5;
    let start = std::f32::consts::PI * 0.75;
    let angle = start + sweep * t;
    let tip = Pos2::new(c.x + angle.cos() * d * 0.42, c.y + angle.sin() * d * 0.42);
    painter.circle_filled(c, d * 0.42, cap);
    painter.line_segment(
        [c, tip],
        Stroke::new((d * 0.09).max(1.0), if t > 0.001 { lit } else { faint }),
    );
}

/// The pan knob: the send's twin, its pointer straight up at centre.
fn pan_knob(painter: &Painter, r: Rect, pan: f32, p: &Ink, master: bool) {
    let (track, cap, lit, faint) = dial_ink(p, master);
    let d = r.height().min(r.width() * 0.72) * 0.78;
    if d <= 2.0 || !r.is_positive() {
        return;
    }
    let c = Pos2::new(r.center().x, r.center().y);
    painter.circle_filled(c, d * 0.5, track);
    let t = (pan.clamp(-1.0, 1.0) + 1.0) * 0.5;
    // Straight UP at centre; three-quarters of a turn lock to lock.
    let sweep = std::f32::consts::PI * 1.5;
    let angle = -std::f32::consts::FRAC_PI_2 - sweep * 0.5 + sweep * t;
    let tip = Pos2::new(c.x + angle.cos() * d * 0.42, c.y + angle.sin() * d * 0.42);
    painter.circle_filled(c, d * 0.42, cap);
    painter.line_segment(
        [c, tip],
        Stroke::new(
            (d * 0.09).max(1.0),
            if pan.abs() > 0.01 { lit } else { faint },
        ),
    );
}

/// A vertical fader: a track, and a cap on it.
/// A fader: an ivory cap, ribbed, riding in a black slot.
///
/// **Modelled on the real thing** — the owner's photograph of a period desk,
/// where the slot is a dark channel cut into the face and the cap is a bone
/// block with fine ridges across it for a thumb to find. What was here was a
/// pale line with a pale block on it, which reads as a diagram of a fader
/// rather than as one.
///
/// The ribs are drawn only when there is room for them to be ribs; below that
/// they are a smear and the cap is better plain.
fn fader(painter: &Painter, r: Rect, gain: f32, face: Color32, p: &Ink) {
    if !r.is_positive() {
        return;
    }
    let x = r.center().x;
    // The slot: dark, and wide enough to read as a cut rather than a hairline.
    let slot_w = (r.width() * 0.16).clamp(3.0, 7.0);
    let slot = Rect::from_min_max(
        Pos2::new(x - slot_w * 0.5, r.top()),
        Pos2::new(x + slot_w * 0.5, r.bottom()),
    );
    painter.rect_filled(slot, slot_w * 0.5, p.slot);
    painter.rect_stroke(
        slot,
        slot_w * 0.5,
        Stroke::new(1.0_f32, p.engrave.gamma_multiply(0.25)),
        egui::StrokeKind::Inside,
    );

    // **The scale, printed on the panel either side of the slot.** The band's
    // faders have had one since they were drawn; the desk's had a cap in a
    // channel and no way to see where along it you were. Eleven marks, the
    // fifths longer, in the panel's own ink faded well back — a scale is
    // something you read when you look for it, not something that competes
    // with the cap.
    //
    // Nothing at all where the arms would be under three points wide: eleven
    // marks that short are a grey smudge, which is the band's rule too.
    let arm = (r.width() - slot_w) * 0.5 - 2.0;
    if arm >= 3.0 && r.height() >= 60.0 {
        let ink = print_on(face, p).gamma_multiply(0.30);
        for i in 0_u8..=10 {
            let y = r.bottom() - r.height() * f32::from(i) / 10.0;
            // A hair inside, so the first and last are marks rather than the
            // ends of the travel.
            let y = y.clamp(r.top() + 0.5, r.bottom() - 0.5);
            let len = if i % 5 == 0 { arm * 0.85 } else { arm * 0.45 };
            painter.line_segment(
                [
                    Pos2::new(slot.left() - 2.0 - len, y),
                    Pos2::new(slot.left() - 2.0, y),
                ],
                Stroke::new(1.0_f32, ink),
            );
            painter.line_segment(
                [
                    Pos2::new(slot.right() + 2.0, y),
                    Pos2::new(slot.right() + 2.0 + len, y),
                ],
                Stroke::new(1.0_f32, ink),
            );
        }
        // Unity, which is the one position a hand looks for and is nowhere
        // near the middle of a decibel scale. Across the slot, so it reads as
        // a mark ON the scale and not as a twelfth tick.
        let uy = r.bottom() - r.height() * gain_to_fader(1.0).clamp(0.0, 1.0);
        painter.line_segment(
            [
                Pos2::new(slot.left() - 1.0, uy),
                Pos2::new(slot.right() + 1.0, uy),
            ],
            Stroke::new(1.0_f32, p.cap.gamma_multiply(0.5)),
        );
    }

    let t = gain_to_fader(gain).clamp(0.0, 1.0);
    let y = r.bottom() - r.height() * t;
    let w = (r.width() * 0.72).clamp(12.0, 34.0);
    let h = (w * 0.52).clamp(9.0, 20.0);
    let cap = Rect::from_center_size(Pos2::new(x, y), Vec2::new(w, h));
    // A dark seat under the cap, so it sits ON the slot rather than in it.
    painter.rect_filled(cap.expand(1.0), 3.0, p.slot);
    painter.rect_filled(cap, 2.0, p.cap);
    // The ribs, and the line through the middle that is the cap's own edge —
    // the one thing on it a hand aims at.
    let ribs = ((h / 3.0).floor() as usize).min(4);
    if ribs >= 2 {
        for i in 1..ribs {
            let ry = cap.top() + cap.height() * (i as f32 / ribs as f32);
            painter.line_segment(
                [
                    Pos2::new(cap.left() + 2.0, ry),
                    Pos2::new(cap.right() - 2.0, ry),
                ],
                Stroke::new(0.75_f32, p.cap_rib),
            );
        }
    }
    painter.line_segment(
        [
            Pos2::new(cap.left() + 1.0, y),
            Pos2::new(cap.right() - 1.0, y),
        ],
        Stroke::new(1.25_f32, p.engrave.gamma_multiply(0.55)),
    );
}

/// The ground a switch is filled with, and the ink that can be printed on
/// it.
///
/// **A lit switch is deepened until what is on it fits.**
///
/// The solo crimson and the record red both land in the dead zone where
/// neither ink clears 4.5:1 — 4.27 and 3.66 — so S and R were the two least
/// readable things on the desk, on every theme, and had been since they were
/// drawn. They stay recognisably the same lights; they are just taken far
/// enough down to carry a letter. The meters and the playhead keep the
/// undimmed reds, because nothing is printed on those.
///
/// The ink is then measured against THAT ground rather than inherited from
/// anywhere: the rule this app has now learned eight times.
///
/// Its own function because the T switch carries a drawn glyph rather than a
/// letter and must land on exactly the same two colours.
fn switch_face(on: bool, lit: Color32, p: &Ink) -> (Color32, Color32) {
    let ground = if on {
        crate::recorder_panel::legible_surface(lit)
    } else {
        p.track
    };
    let ink = if crate::recorder_panel::wants_light_ink(ground) {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    };
    (ground, ink)
}

fn switch(painter: &Painter, r: Rect, label: &str, on: bool, lit: Color32, p: &Ink) {
    if !r.is_positive() {
        return;
    }
    let (ground, ink) = switch_face(on, lit, p);
    painter.rect_filled(r, 2.0, ground);
    let size = (r.height() * 0.62).clamp(8.0, 13.0);
    centred(painter, r, label, FontId::new(size, crate::fonts::courier_bold()), ink);
}

/// Centred, and **cut to the cell rather than run out of it**.
///
/// A channel's name is whatever the user typed and whatever the interface
/// calls itself, and a mixer strip is a hundred points wide: "Scarlett 18i20
/// USB - 3" drawn whole reaches across two neighbours and reads as a fault in
/// the drawing. An ellipsis says "there is more of this"; overflow says
/// nothing at all.
fn centred(painter: &Painter, r: Rect, text: &str, font: FontId, colour: Color32) {
    if !r.is_positive() || text.is_empty() {
        return;
    }
    painter.text(
        r.center(),
        egui::Align2::CENTER_CENTER,
        // The timeline's cut, which reads where the ellipsis lands off the
        // one galley the first measure makes. The desk kept its own copy of
        // the walk that one replaced — a layout per character per chip that
        // did not fit, every frame — and the two had drifted apart only in
        // cost, never in answer.
        &crate::arrangement_panel::cut_to(painter, r.width(), text, &font),
        font,
        colour,
    );
}

/// A plugin's name for a rack bay: the end of it, not the beginning.
///
/// **The vendor is the first word and the useless one.** "FabFilter Pro-R 2"
/// cut to fit gives "FabFilter Pro…", and a rack of those is three bays that
/// all say FabFilter — the manufacturer, which is the same on all of them,
/// crowding out the part that says which plugin it is. So leading words are
/// dropped until what is left fits, and only then is it cut.
fn shorten_plugin(painter: &Painter, width: f32, name: &str, font: &FontId) -> String {
    let fits = |t: &str| {
        painter
            .layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE)
            .rect
            .width()
            <= width
    };
    if fits(name) {
        return name.to_owned();
    }
    let words: Vec<&str> = name.split_whitespace().collect();
    for drop in 1..words.len() {
        let tail = words[drop..].join(" ");
        if fits(&tail) {
            return tail;
        }
    }
    // Nothing fits whole, so the LAST word cut is still better than the first
    // word whole: "Pro-R…" names a plugin and "FabFilt…" names a company.
    crate::arrangement_panel::cut_to(painter, width, words.last().copied().unwrap_or(name), font)
}

/// What `c` actually looks like once it is painted over `under`.
///
/// **Colours here are premultiplied and some of them are translucent.** A
/// silent strip's face is the bone at 62%, whose raw bytes read as a mid grey
/// while the eye sees a warm tan over the walnut — so any decision made from
/// the bytes is made about a colour that is never on screen.
fn flatten(c: Color32, under: Color32) -> Color32 {
    let a = f32::from(c.a()) / 255.0;
    let mix = |s: u8, d: u8| (f32::from(s) + f32::from(d) * (1.0 - a)).min(255.0) as u8;
    Color32::from_rgb(
        mix(c.r(), under.r()),
        mix(c.g(), under.g()),
        mix(c.b(), under.b()),
    )
}

/// Ink that can be read on `bg`, whatever somebody painted the channel.
///
/// `bg` must be a colour that is really on screen — see [`flatten`].
fn print_on(bg: Color32, p: &Ink) -> Color32 {
    // **Never the FACE.** This used to answer `p.face` on a dark channel,
    // which was right for exactly as long as the face was the desk's bone:
    // light, neutral, and legible on anything dark. Once `fx_strip` became a
    // theme role the face became whatever colour the theme wanted, and the
    // rule started printing the Iris theme's purple onto an olive track —
    // the owner's screenshot, and every reading of "a text colour that
    // doesn't contrast or fit".
    //
    // The ink a surface can carry is a property OF THAT SURFACE. So it is
    // measured, out of the same pair and by the same authority the band and
    // the strip faces already use.
    let _ = p;
    if crate::recorder_panel::wants_light_ink(bg) {
        crate::recorder_panel::INK_LIGHT
    } else {
        crate::recorder_panel::INK_DARK
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The T switch is legible on every theme, lit and at rest.**
    ///
    /// The standing rule for anything new on a surface: sixteen palettes,
    /// both modes, and the glyph measured against the ground it is actually
    /// filled onto rather than inherited from a themed role. The note is
    /// drawn in the same ink `switch` prints its letters in, so proving the
    /// ink proves the glyph.
    #[test]
    fn every_theme_dresses_the_mixer_theory_switch_legibly() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            let mut set = crate::settings::Settings::default();
            theme.apply(&mut set);
            let mut v = a_view();
            v.wood = {
                let c = set.recorder_bg_color;
                (c.r, c.g, c.b)
            };
            v.fx_strip = set.fx_strip_color.to_color32();
            let p = ink(&v);
            for (state, on) in [("lit", true), ("at rest", false)] {
                let (ground, glyph) = switch_face(on, p.engrave, &p);
                let ratio = crate::recorder_panel::contrast_ratio(glyph, ground);
                if ratio < 4.5 {
                    bad.push(format!("{} T {state}: {ratio:.2}:1", theme.name));
                }
            }
            // And it must not be mistaken for the three beside it. Measured
            // on the GROUNDS, which is what a person actually sees.
            let t = switch_face(true, p.engrave, &p).0;
            for (name, other) in
                [("the mute", p.mute), ("the solo", p.solo), ("the arm", p.arm)]
            {
                let lit = switch_face(true, other, &p).0;
                let d = delta_e(t, lit);
                if d < 25.0 {
                    bad.push(format!(
                        "{}: a lit T is {d:.1} from {name} — too close to tell apart",
                        theme.name
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "the T switch is not readable:\n  {}", bad.join("\n  "));
    }

    /// **Every cell the layout carves is one something draws into.**
    ///
    /// The T switch was laid out, hit-tested, and never painted: a control
    /// you can press and cannot see, which is worse than one that is
    /// missing. The owner found it by pressing the empty paint beside R.
    ///
    /// Source-scanned rather than rendered because what is being asserted is
    /// that a NAME is used, and the alternative — sifting a tessellated
    /// shape list for a rectangle in the right place — proved to be a test
    /// about egui's internals rather than about this panel.
    #[test]
    fn every_cell_the_layout_carves_is_drawn() {
        let src = include_str!("mixer_panel.rs");
        // The painter for one strip: from its signature to the start of the
        // next item.
        let from = src.find("\nfn strip(").expect("no strip painter");
        let body = &src[from..];
        // **Its own closing brace, not the next item's doc comment.** This
        // used to end the window at the sentence that opened the function
        // below, so deleting that function moved the end of `strip` and the
        // scan silently read the whole rest of the file. A `}` in the first
        // column is where a top-level item ends and nowhere else.
        let end = body.find("\n}\n").expect("the strip painter never closes");
        let body = &body[..end];
        let mut missing: Vec<&str> = Vec::new();
        for cell in [
            "theory", "mute", "solo", "arm", "folder", "fader", "pan", "send",
            "name", "db", "meter", "panel",
        ] {
            if !body.contains(&format!("l.{cell}")) {
                missing.push(cell);
            }
        }
        assert!(
            missing.is_empty(),
            "the strip painter never touches: {missing:?} — laid out, \
             pressable, invisible"
        );
    }

    fn a_strip(strip: Option<Strip>, empty: bool) -> StripView<'static> {
        StripView {
            mono: false,
            to_theory: false,
            is_folder: false,
            in_folder: false,
            folder_shut: false,
            hidden: false,
            route_count: 0,
            icon: 0,
            strip,
            name: "CHANNEL",
            detail: "",
            inserts: [""; INSERTS * 2],
            // A used general channel in the tests is MIDI unless a test says
            // otherwise; track/click/master ignore it.
            kind: (!empty && matches!(strip, Some(Strip::Channel(_))))
                .then_some(crate::recorder::ChannelKind::Midi),
            color: 0,
            stereo: true,
            gr_db: 0.0,
            empty,
            gain: 1.0,
            send: 0.0,
            pan: 0.0,
            peak: [0.0; 2],
            muted: false,
    phase: false,
            soloed: false,
            // What the app decides: a used general channel can be removed.
            removable: !empty && matches!(strip, Some(Strip::Channel(_))),
            armed: false,
            keyed: [false; INSERTS * 2],
            bypassed: [false; INSERTS * 2],
        }
    }

    /// Every slot filled, so the ordinary strip is what is being measured.
    /// **Render the desk to a file, so a person can look at it.**
    ///
    /// Not a test of anything: an eye. Every channel wears six bays now
    /// (`recorder::bays_on`) and a plain channel fits them into the band that
    /// held three, so what a test cannot see is whether a bay is still tall
    /// enough to read a plugin's name in.
    ///
    /// **It cannot photograph the ICONS.** `face::fit` soaks one texture —
    /// egui's font atlas, `TextureId::Managed(0)` — so a mesh drawn with any
    /// other texture is rasterised against the glyph coverage and every badge
    /// comes out as the same scribble. That is the tool, not the drawings.
    /// To look at those, render `assets/icons/*.a8` directly: it is a bare
    /// 192x192 block of 8-bit coverage with no header.
    ///
    ///     IVORY_SHOT=/tmp/desk.png IVORY_SHOT_W=900 IVORY_SHOT_H=700 \
    ///       cargo test -p tangent-ui --lib mixer_panel::tests::desk \
    ///       -- --ignored --nocapture
    #[test]
    #[ignore = "writes a picture for a person to look at"]
    fn desk() {
        let Ok(out) = std::env::var("IVORY_SHOT") else {
            eprintln!("IVORY_SHOT not set");
            return;
        };
        let num = |k: &str, d: f32| {
            std::env::var(k).ok().and_then(|v| v.parse::<f32>().ok()).unwrap_or(d)
        };
        let (w, hgt) = (num("IVORY_SHOT_W", 900.0), num("IVORY_SHOT_H", 700.0));
        let mut v = a_view();
        v.strips[1].is_folder = true;
        for i in 0..3 {
            v.strips[i].inserts = ["COMP", "EQ", "VERB", "TAPE", "", ""];
        }
        let ctx = crate::face::fit::a_ctx(crate::fonts::FontChoice::default());
        let surface = crate::recorder_panel::surface_of(Color32::from_rgb(0x61, 0x37, 0x3A));
        let (pw, ph, rgba) = crate::face::fit::shoot(
            &ctx,
            Vec2::new(w, hgt + crate::shell::CHROME_BAR_H),
            "Tangent \u{2014} mixer",
            &surface,
            &mut |ui| {
                let r = ui.max_rect();
                ui.set_clip_rect(r);
                // `IVORY_SHOT_PANEL` draws a desk WIDER than the window and
                // `IVORY_SHOT_FROM` slides it left, so a strip at the right
                // end can be photographed at full size rather than as a
                // thumbnail of the whole desk.
                let panel_w = num("IVORY_SHOT_PANEL", w);
                let from = num("IVORY_SHOT_FROM", 0.0);
                draw(
                    ui.painter(),
                    Rect::from_min_size(
                        Pos2::new(r.left() - from, r.top()),
                        Vec2::new(panel_w, r.height()),
                    ),
                    &v,
                );
            },
        );
        crate::face::fit::write_png(std::path::Path::new(&out), &rgba, pw, ph);
        println!("wrote {out} ({pw}x{ph})");
    }

    fn a_view() -> MixerView<'static> {
        let channels = Strip::shown();
        MixerView {
            selected_tracks: &[],
            routes: &[],
            fx_strip: Color32::from_rgb(0xE3, 0xD9, 0xBD),
            input_chip: Color32::from_rgb(0xE3, 0xD9, 0xBD),
            strips: std::array::from_fn(|i| {
                a_strip(channels.get(i).copied(), false)
            }),
            click_sound: "CLICK",
            any_solo: false,
            scroll_px: 0.0,
            active: CHANNELS,
            visible: VISIBLE_CHANNELS,
            selected: None,
            typing: None,
            naming: None,
            fx_menu: None,
            fx_menu_key: false,
            fx_menu_loaded: true,
            fx_menu_paste: false,
            dark_mode: true,
            wood: (0x4A, 0x3B, 0x2C),
        }
    }

    /// The index of the last channel before the master.
    const MASTER: usize = COLUMNS - 1;
    /// The effects return, which is the channel before that.
    /// The click's column, which is the one before the master.
    const CLICK: usize = COLUMNS - 2;

    fn rect() -> Rect {
        Rect::from_min_size(Pos2::new(0.0, 0.0), Vec2::new(1300.0, 420.0))
    }

    /// **WAVE is the default badge, for a MIDI track and an audio one alike.**
    ///
    /// This is the behaviour the owner asked for and the one a later edit is
    /// most likely to undo, because undoing it looks like a fix: the strip
    /// KNOWS whether a channel is MIDI or audio, so branching on it is the
    /// obvious thing to write, and the app did exactly that until now - a
    /// keyboard for MIDI, a microphone for audio, a waveform for a clip lane
    /// with nothing live on it. Three pictures for one thing. Both kinds are
    /// named here so that restoring the split fails on the case that made the
    /// owner ask, rather than on some abstract invariant.
    #[test]
    fn a_track_with_no_chosen_badge_draws_the_wave_whatever_it_carries() {
        use crate::icons::Icon;
        for kind in [
            Some(crate::recorder::ChannelKind::Midi),
            Some(crate::recorder::ChannelKind::Audio),
            // A CLIP lane: used, and with no live source at all.
            None,
        ] {
            let mut v = a_strip(Some(Strip::Channel(0)), false);
            v.kind = kind;
            assert_eq!(
                auto_icon(v.strip, v.is_folder),
                Some(Icon::Wave),
                "a {kind:?} track drew something other than the wave"
            );
        }
        // The two buses keep pictures of their own — and so, now, do a
        // FOLDER and the MASTER. Both used to answer `None`, which was true
        // ("nothing to choose") and was drawn as a hole where every other
        // strip has a badge. Choosing still overrides either.
        assert_eq!(auto_icon(Some(Strip::Drums), false), Some(Icon::Drumset));
        assert_eq!(auto_icon(Some(Strip::Click), false), Some(Icon::Metronome));
        assert_eq!(
            auto_icon(Some(Strip::Channel(0)), true),
            Some(Icon::Folder),
            "a folder did not draw the folder"
        );
        assert_eq!(auto_icon(None, false), Some(Icon::Master), "the master drew nothing");
        // And the default is spelt in ONE place, which is what the strip and
        // the arrangement both read.
        assert_eq!(crate::recorder_panel::DEFAULT_TRACK_ICON, Icon::Wave);
    }

    /// **The badge fills its seat and still clears everything around it.**
    ///
    /// The owner's acceptance criterion is "big enough to see, and nothing
    /// cut off", and the new drawings are trimmed to their own bounds - they
    /// use their whole seat, where every vector badge they replace inset
    /// itself by a tenth. So the clearances that were comfortable by accident
    /// have to be measured rather than assumed.
    ///
    /// The one that looked alarming and is not: the badge's seat reaches
    /// about a point PAST the band it was laid out in, at every window
    /// height. On a channel that is still points clear of the input chip
    /// below it. On the two buses the seat overlaps the detail row outright -
    /// and the detail row of a bus is empty (`app.rs` gives DRUM BUS and
    /// CLICK no second line), so nothing is drawn there to collide with. If
    /// a bus is ever given a second line, this test is what fails.
    #[test]
    fn the_strip_badge_clears_the_name_above_it_and_the_chip_below() {
        let mut worst = f32::MAX;
        let mut tightest_fill = f32::MAX;
        // **Both desks, and the narrow window too.** The fold into sixteen
        // over sixteen halves every strip's height, which is the case the
        // owner named when he asked for bigger badges — and a narrow window
        // is the case where the strip's WIDTH is what stops one growing. A
        // sweep that only walked the height would miss both.
        for (w, h) in [
            (1300.0_f32, 340.0_f32),
            (1300.0, 420.0),
            (1300.0, 520.0),
            (1300.0, 640.0),
            (1300.0, 780.0),
            (1300.0, 1000.0),
            (1300.0, 1400.0),
            // The tall, wide windows the desk used to fold in.
            (1900.0, 900.0),
            (1900.0, 1100.0),
            (2560.0, 1400.0),
            // And a window narrow enough that the columns are near the floor.
            (760.0, 900.0),
            (620.0, 620.0),
        ] {
            let v = a_view();
            let l = Layout::new(Rect::from_min_size(Pos2::ZERO, Vec2::new(w, h)), &v);
            // Did the desk actually fold here? Read off the layout rather
            // than re-deriving the rule: a second row is a channel strip
            // whose panel starts below the middle of the window.
            if l.strips.iter().enumerate().any(|(i, s)| {
                i != MASTER && i != CLICK && s.panel.is_positive() && s.panel.top() > h * 0.4
            }) {
            }
            for (i, s) in l.strips.iter().enumerate() {
                if !s.icon.is_positive() {
                    continue;
                }
                // What is actually PAINTED, letterboxing and the clear margin
                // the assets carry included - not the seat it was handed.
                let seat = crate::icons::fitted(s.icon);
                let pad = seat.height() * 8.0 / 192.0;
                let ink = seat.shrink(pad);
                assert!(
                    s.icon.contains_rect(seat),
                    "{w}x{h}: strip {i} paints outside its seat"
                );
                assert!(
                    ink.top() > s.name.bottom(),
                    "{w}x{h}: strip {i} badge overlaps its own name"
                );
                // A bus draws nothing in its detail row, so only a strip with
                // a real chip under it has something to collide with.
                if s.input_chip.is_positive() {
                    let gap = s.input_chip.top() - ink.bottom();
                    assert!(gap > 0.0, "{w}x{h}: strip {i} badge overlaps the input chip by {:.2}", -gap);
                    worst = worst.min(gap);
                }
                // And it is not a speck: the seat takes most of the band it
                // was given, which is what "big enough to see" means here.
                let band = s.icon.width().min(s.icon.height());
                assert!(band >= 11.0, "{w}x{h}: strip {i} badge is only {band:.1}pt");
                // **And it takes ALL of the room, which is the ask.** The
                // badge is square, so what it can have is the shorter of the
                // band's height and the strip's usable width; anything less
                // than the whole of that is the 0.82 shrink coming back.
                let room = s.icon_room.height().min(s.icon_room.width());
                if room > 0.0 {
                    tightest_fill = tightest_fill.min(band / room);
                }
            }
        }
        assert!(
            worst.is_finite() && worst > 1.0,
            "the tightest badge-to-chip gap is {worst:.2}pt"
        );
        assert!(
            tightest_fill > 0.999,
            "the badge uses only {:.0}% of the room it has",
            tightest_fill * 100.0
        );
    }

    /// **A tinted icon is legible on the strip it lands on, every theme,
    /// both modes.**
    ///
    /// The badge takes the same ink the strip's own labels take, and the
    /// ground it lands on is the CHANNEL's colour, not the theme's face - a
    /// desk somebody painted, run through the dead-zone guard. That pairing
    /// is the one this file has got wrong before (the icon and the decibel
    /// readout went dark on dark when the desk got colours), and a mask has
    /// no colour of its own to fall back on, so if the ink disappears the
    /// icon disappears completely rather than becoming hard to read.
    ///
    /// 3.0 rather than 4.5: a badge is a picture, and the floor the rest of
    /// this app holds its graphic marks to is 3.0. Failures are collected so
    /// a run names every theme that is wrong rather than the first.
    #[test]
    fn every_theme_tints_a_badge_legibly_on_every_colour_a_strip_can_be() {
        let mut bad: Vec<String> = Vec::new();
        let themes = crate::theme::factory();
        assert!(themes.len() >= 16, "only {} themes to sweep", themes.len());
        for t in &themes {
            let Some(strip) = t.fx_strip else { continue };
            let mut v = a_view();
            v.fx_strip = strip.to_color32();
            for dark in [false, true] {
                v.dark_mode = dark;
                let p = ink(&v);
                let mode = if dark { "dark" } else { "light" };
                for colour in 0..MANUAL_COLORS {
                    // Exactly what `strip` does with a painted channel.
                    let face = if colour == 0 {
                        p.face
                    } else {
                        let (r, g, b) = STRIP_COLORS[colour];
                        crate::recorder_panel::legible_surface(Color32::from_rgb(r, g, b))
                    };
                    let solid = flatten(face, p.wood);
                    let c = crate::recorder_panel::contrast_ratio(print_on(solid, &p), solid);
                    if c < 3.0 {
                        bad.push(format!("{} {mode} colour {colour}: {c:.2}:1", t.name));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "a badge is invisible on {} pairings:\n{}", bad.len(), bad.join("\n"));
    }

    /// **Every control on the desk has words, and they name their strip.**
    ///
    /// Sixteen channels of identical controls is the case where a screen
    /// reader label like "volume" is worse than none — a person hears the
    /// same word sixteen times and cannot tell which fader they are on. Every
    /// node here leads with the channel.
    #[test]
    fn every_control_on_the_desk_says_which_strip_it_belongs_to() {
        let view = a_view();
        let r = rect();
        let nodes = a11y_nodes(r, &view);
        assert!(nodes.len() > 40, "only {} controls have names", nodes.len());

        // Coverage against the panel's own enumeration, minus the modal
        // insert menu, which is not part of the desk's tab order.
        let l = Layout::new(r, &view);
        let mut unnamed = Vec::new();
        for (rr, hit) in l.targets() {
            if matches!(hit, Hit::FxMenu(..)) {
                continue;
            }
            if !nodes.iter().any(|n| n.rect == rr) {
                unnamed.push(format!("{hit:?}"));
            }
        }
        assert!(unnamed.is_empty(), "unnamed desk controls: {unnamed:#?}");

        for n in &nodes {
            assert!(!n.label.is_empty(), "a desk node has no label");
            assert!(
                r.contains_rect(n.rect),
                "{:?} is outside the desk it belongs to",
                n.label
            );
        }
        // Distinct ids, or Tab lands on the wrong fader.
        let mut ids: Vec<String> = nodes.iter().map(|n| format!("{:?}", n.id)).collect();
        ids.sort();
        let before = ids.len();
        ids.dedup();
        assert_eq!(before, ids.len(), "two desk controls share an id");
    }

    /// A fader reads in decibels and a pan in sides, never in 0..1.
    #[test]
    fn the_desk_speaks_in_the_numbers_printed_on_it() {
        let mut view = a_view();
        view.strips[0].gain = 0.5;
        view.strips[0].pan = -0.64;
        let nodes = a11y_nodes(rect(), &view);
        let said = |needle: &str| {
            nodes
                .iter()
                .find(|n| n.label.contains(needle))
                .map(crate::a11y::Node::spoken)
                .unwrap_or_default()
        };
        assert!(said("volume").contains("dB"), "the fader reads {:?}", said("volume"));
        assert!(
            said("pan").contains("left"),
            "a pan of -0.64 reads {:?} rather than a side",
            said("pan")
        );
        assert_eq!(pan_text(0.0), "centre");
        assert_eq!(pan_text(-0.5), "left 50");
        assert_eq!(pan_text(0.25), "right 25");

        // Mute and solo say which they are AND whether they are on, because
        // a toggle that only says its name is one nobody can read the state
        // of without pressing it.
        let mut on = a_view();
        on.strips[0].muted = true;
        let muted = a11y_nodes(rect(), &on);
        let m = muted
            .iter()
            .find(|n| n.label.contains("mute"))
            .expect("no mute node");
        assert!(m.spoken().ends_with(", on"), "a lit mute reads {:?}", m.spoken());
    }

    /// An empty slot offers a plus and nothing else.
    ///
    /// Eight controls belonging to a channel that is not there would be eight
    /// tab stops onto nothing.
    #[test]
    fn an_empty_slot_is_one_tab_stop_and_not_nine() {
        let channels = Strip::shown();
        let view = MixerView {
            strips: std::array::from_fn(|i| a_strip(channels.get(i).copied(), i == 2)),
            ..a_view()
        };
        let nodes = a11y_nodes(rect(), &view);
        let on_two: Vec<_> = nodes
            .iter()
            .filter(|n| {
                // Anchored, not substring: at thirty-two channels "slot 33"
                // and "Channel 30" both CONTAIN the strings this looked for.
                n.label == "Add a channel at slot 3"
                    || n.label.starts_with("Channel 3 ")
                    || n.label == "Channel 3"
            })
            .collect();
        assert!(
            on_two.len() <= 1,
            "an empty slot offers {} tab stops: {:?}",
            on_two.len(),
            on_two.iter().map(|n| &n.label).collect::<Vec<_>>()
        );
    }

    /// **What is printed on a strip is legible on the strip.**
    ///
    /// The owner sent a screenshot of a themed rack where the track titles,
    /// the icons, the bay labels, the dial ticks and readouts, the dB line,
    /// M/S/R and the plus on an empty slot were all dark ink on a dark face.
    /// The ink was one fixed near-black — right on the bone face this desk
    /// ships with, unreadable on any dark one a theme sets. This holds the
    /// rule for every theme the app can wear.
    #[test]
    fn every_theme_prints_legibly_on_its_own_strip_face() {
        for t in crate::theme::factory() {
            let Some(strip) = t.fx_strip else { continue };
            let mut v = a_view();
            v.fx_strip = strip.to_color32();
            for dark in [false, true] {
                v.dark_mode = dark;
                let p = ink(&v);
                let c = crate::recorder_panel::contrast_ratio(p.engrave, p.face);
                assert!(
                    c >= 4.5,
                    "{}{}: the ink is {c:.1}:1 on the strip face",
                    t.name,
                    if dark { " (dark mode)" } else { "" }
                );
                // And the quieter ink still has to be readable, not merely
                // present: it carries the input chip and the bay labels.
                let f = crate::recorder_panel::contrast_ratio(p.faint, p.face);
                assert!(
                    f >= 2.2,
                    "{}{}: the secondary ink is {f:.1}:1 on the strip face",
                    t.name,
                    if dark { " (dark mode)" } else { "" }
                );
            }
        }
    }

    /// **A BYPASSED bay reads as bypassed on every theme, in both modes.**
    ///
    /// Three things have to survive the wardrobe, and each of them has been
    /// got wrong somewhere in this app before:
    ///
    /// 1. the amber tab has to be VISIBLE on the strip face — a fixed light
    ///    on a themed ground is the oldest fault in this file, and a warm
    ///    theme is exactly where amber-on-amber would disappear;
    /// 2. the tab has to be visibly DIFFERENT from the blue "loaded" tab it
    ///    replaces, or the switch has no picture at all — "quieter blue"
    ///    would be a state you can only learn by clicking;
    /// 3. the dimmed name has to stay READABLE. Quieter is a matter of
    ///    weight; a plugin whose name you cannot read is a plugin you cannot
    ///    identify to put back.
    ///
    /// Failures are collected rather than asserted one at a time, so a run
    /// names every theme that is wrong instead of the first.
    #[test]
    fn a_bypassed_bay_reads_as_bypassed_on_every_theme_in_both_modes() {
        let mut bad: Vec<String> = Vec::new();
        for t in crate::theme::factory() {
            let Some(strip) = t.fx_strip else { continue };
            let mut v = a_view();
            v.fx_strip = strip.to_color32();
            for dark in [false, true] {
                v.dark_mode = dark;
                let p = ink(&v);
                let mode = if dark { "dark" } else { "light" };
                let tab = crate::recorder_panel::lit_on(p.mute, p.face);
                let word = crate::arrangement_panel::quiet(p.engrave, p.face, 0.45);

                // A graphic mark, so the 3.0 floor the rest of the app holds
                // its lights to rather than the 4.5 text asks for.
                let seen = crate::recorder_panel::contrast_ratio(tab, p.face);
                if seen < 3.0 {
                    bad.push(format!("{} {mode}: the amber tab is {seen:.2}:1 on the face", t.name));
                }
                // And a DIFFERENT colour from loaded, not merely a legible
                // one: `delta_e` is this app's "is it another colour" answer.
                let apart = delta_e(tab, p.lit);
                if apart < 15.0 {
                    bad.push(format!(
                        "{} {mode}: bypassed and loaded are {apart:.1} apart, which is one light",
                        t.name
                    ));
                }
                let read = crate::recorder_panel::contrast_ratio(word, p.face);
                if read < 4.5 {
                    bad.push(format!(
                        "{} {mode}: a bypassed plugin's name is {read:.2}:1 on the face",
                        t.name
                    ));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the bypassed bay does not read on every theme:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **A channel that routes gets the BUTTON and nothing else.**
    ///
    /// It used to grow a little level strip per route, and the fader gave up
    /// a slice of its travel for each one — so a column changed shape as you
    /// routed it. The owner's call: keep the button that summons the routing
    /// window, drop the strips that only appeared after a route was added.
    /// The levels are set in that window, beside the routes themselves.
    #[test]
    fn routing_is_a_button_and_the_fader_never_gives_up_travel_for_it() {
        let mut none = a_view();
        none.strips[1].route_count = 0;
        let unrouted = Layout::new(rect(), &none).strips[1];

        let mut v = a_view();
        v.strips[1].route_count = 3;
        let s = Layout::new(rect(), &v).strips[1];

        assert!(s.routing.is_positive(), "no ROUTING button rect");
        for n in 0..STRIP_ROUTES {
            assert!(
                !s.routes[n].is_positive(),
                "route {n} still draws a strip on the desk"
            );
        }
        // **The same fader whatever it routes**, which is the point.
        assert!(
            (s.fader.height() - unrouted.fader.height()).abs() < 0.51,
            "routing cost the fader {} points of travel",
            unrouted.fader.height() - s.fader.height()
        );
        // And a routed channel is still a pressable button.
        assert_eq!(
            hit_test(rect(), &v, s.routing.center()),
            Some(Hit::Routing(Column(1))),
            "the routing button cannot be pressed"
        );
    }

    /// **Every control can be pressed, and presses where it is drawn.**
    ///
    /// `hit_test` is the painter's inverse and both read `targets`, so this is
    /// the test that keeps them one thing: a control that moved and took its
    /// hit region with it passes, and one that moved without it does not.
    #[test]
    fn every_control_is_where_it_is_drawn() {
        let v = a_view();
        let r = rect();
        let l = Layout::new(r, &v);
        for (region, hit) in l.targets() {
            assert!(
                region.is_positive(),
                "{hit:?} has no rectangle but is offered as a target"
            );
            assert_eq!(
                hit_test(r, &v, region.center()),
                Some(hit),
                "{hit:?} could not be pressed at its own centre"
            );
            assert!(
                r.contains(region.center()),
                "{hit:?} is drawn outside the mixer"
            );
        }
    }

    /// **A FOLDER wears a chip, and it reads FOLDER.**
    ///
    /// The band a track names its input in was empty paint on a group — a
    /// group has no input to bind — and a desk whose one continuous row of
    /// chips has holes in it reads as chips that failed to draw. The owner:
    /// "add an input chip that reads FOLDER on folders in mixer view to
    /// close the gap."
    ///
    /// It is a READOUT: it sits on the track's own chip row and answers no
    /// press, because there is nothing on a group to choose.
    #[test]
    fn a_folder_wears_a_chip_reading_folder() {
        let mut v = a_view();
        v.strips[1].is_folder = true;
        let r = rect();
        let l = Layout::new(r, &v);
        let chip = l.strips[1].folder_chip;
        let track = l.strips[0].input_chip;
        assert!(chip.is_positive(), "the folder's band is still a hole");
        assert!(track.is_positive(), "the reference track has no input chip");
        assert!(
            (chip.top() - track.top()).abs() < 0.5
                && (chip.bottom() - track.bottom()).abs() < 0.5,
            "the folder's chip is not on the input chip's own row"
        );
        assert!(
            !l.strips[1].input_chip.is_positive(),
            "a folder grew an input chip it could bind nothing with"
        );
        // Nothing to press: it is not in the target list at all.
        assert!(
            !l.targets().iter().any(|(rect, _)| *rect == chip),
            "the folder's chip answers a press"
        );
        // And the word is drawn on it.
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
        let said = out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Text(t) => {
                t.galley.text() == "FOLDER" && chip.contains(t.visual_bounding_rect().center())
            }
            _ => false,
        });
        assert!(said, "the folder's chip is blank");
    }

    /// **The selected strip's rim follows the shape of the strip.**
    ///
    /// A folder's top-right corner is chopped (`NOTCH_W`, `CHILD_INDENT`),
    /// and a rounded rectangle drawn round one crosses the empty paint above
    /// the chop — a rim that says the group reaches somewhere it does not.
    /// The owner: "make it so it matches the shape of a folder (has cutout
    /// at top right) when one is selected."
    ///
    /// A track keeps the rectangle, and both are drawn at `RIM_W`, which is
    /// the "slight bump for visibility" the same ask came with.
    #[test]
    fn a_selected_folder_is_rimmed_in_the_shape_of_a_folder() {
        let mut v = a_view();
        v.strips[1].is_folder = true;
        let picked = [0usize, 1];
        v.selected_tracks = &picked;
        let r = rect();
        let l = Layout::new(r, &v);
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
        // The FOLDER's rim is a path, and it keeps out of the chop: no point
        // on it stands in the notch the strip cuts out of its own corner.
        let panel = l.strips[1].panel;
        let notch = Rect::from_min_max(
            Pos2::new(panel.right() - panel.width() * NOTCH_W + 1.0, panel.top()),
            Pos2::new(panel.right(), panel.top() + CHILD_INDENT - 1.0),
        );
        let mut rim = None;
        for s in &out.shapes {
            if let egui::Shape::Path(pp) = &s.shape {
                if pp.stroke.width > 0.0 && panel.contains(pp.visual_bounding_rect().center()) {
                    rim = Some(pp.clone());
                }
            }
        }
        let rim = rim.expect("the selected folder was not rimmed");
        assert!(
            (rim.stroke.width - RIM_W).abs() < 0.01,
            "the rim is {} wide, not the bumped {RIM_W}",
            rim.stroke.width
        );
        assert!(
            !rim.points.iter().any(|p| notch.contains(*p)),
            "the folder's rim runs through its own chop"
        );
        assert!(
            rim.points.iter().any(|p| notch.right() - p.x < 1.5
                && (p.y - (panel.top() + CHILD_INDENT)).abs() < 2.0),
            "the rim does not come back out to the strip's edge under the chop"
        );
        // A plain track keeps the rectangle, at the same weight.
        let track = l.strips[0].panel;
        let rect_rim = out.shapes.iter().any(|s| match &s.shape {
            egui::Shape::Rect(rr) => {
                (rr.stroke.width - RIM_W).abs() < 0.01
                    && track.contains(rr.rect.center())
                    && rr.fill.a() == 0
            }
            _ => false,
        });
        assert!(rect_rim, "a selected track lost its rectangular rim");
    }

    /// **Every rack on the desk is at the same height, and holds six.**
    ///
    /// The master's and a folder's used to start at 0.100 — reaching up over
    /// the band the ICON sits in, which is why a group's badge was covered by
    /// its own first bay. The owner: "lower the height of folders' fx sends
    /// such that their icons are visible", then "make sure at the end that
    /// all fx sends are visually at the same level through the whole mixer
    /// view to be the same as the current tracks'".
    ///
    /// The CLICK is in the sweep because it grew a second rack to be here
    /// with: six slots seated in `recorder::BUS_EXTRA_SEATS`, and the engine
    /// runs them.
    #[test]
    fn every_rack_on_the_desk_is_at_one_height() {
        let mut v = a_view();
        v.strips[1].is_folder = true;
        let l = Layout::new(rect(), &v);
        let track = l.strips[0].inserts[0];
        assert!(track.is_positive(), "the reference track has no rack");
        for (what, col) in [("folder", 1), ("click", CLICK), ("master", MASTER)] {
            let bays = &l.strips[col].inserts;
            assert!(bays[0].is_positive(), "{what}: no rack at all");
            assert!(
                (bays[0].top() - track.top()).abs() < 0.5,
                "{what}'s rack starts at {:.1} and a track's at {:.1}",
                bays[0].top(),
                track.top()
            );
            // Six, and the last one ends where a track's last one ends.
            assert!(bays[5].is_positive(), "{what}: only three bays");
            assert!(
                (bays[5].bottom() - l.strips[0].inserts[5].bottom()).abs() < 0.5,
                "{what}'s rack ends somewhere else"
            );
        }
        // **And a folder's ICON is clear of it**, which is the fault that
        // started this: the rack used to be drawn over the badge.
        let folder = &l.strips[1];
        assert!(folder.icon.is_positive(), "the folder has no icon seat");
        assert!(
            folder.icon.bottom() <= folder.inserts[0].top() + 0.5,
            "the folder's rack still covers its badge"
        );
    }

    /// **The click's own sound button**, where a channel keeps its input
    /// chip: it reads the file that is playing and opens the metronome panel.
    /// The owner's "add the met menu from arrange view as its own button —
    /// it should read as the file chosen".
    ///
    /// **And the word on it is the SOUND**, which is the strip's own detail
    /// — the file that ships has a name of its own ("UREI CLICK") and it is
    /// not "CLICK", which is what the name plate two bands above it already
    /// says. The owner: "make the name what the actual default file urei
    /// click is."
    #[test]
    fn the_click_carries_the_metronomes_own_button() {
        let mut v = a_view();
        v.strips[CLICK].detail = "UREI CLICK";
        let l = Layout::new(rect(), &v);
        let click = &l.strips[CLICK];
        assert!(click.met.is_positive(), "the click has no sound button");
        assert!(!click.input_chip.is_positive(), "the click grew an input chip");
        // In the band a channel spends on its input, so the two columns line
        // up row for row.
        let track = &l.strips[0];
        assert!(
            (click.met.top() - track.input_chip.top()).abs() < 0.5
                && (click.met.bottom() - track.input_chip.bottom()).abs() < 0.5,
            "the click's button is not on the input chip's own row"
        );
        // And it is reachable.
        assert_eq!(
            hit_test(rect(), &v, click.met.center()),
            Some(Hit::MetFile),
            "the click's sound button cannot be pressed"
        );
        // Every other column keeps its own; nobody else grew one.
        for (i, s) in l.strips.iter().enumerate() {
            assert!(
                i == CLICK || !s.met.is_positive(),
                "column {i} grew a metronome button"
            );
        }
    }

    /// No two controls overlap, or one of them is unreachable.
    #[test]
    fn no_two_controls_are_in_the_same_place() {
        let v = a_view();
        let all = Layout::new(rect(), &v).targets();
        for (i, (a, ha)) in all.iter().enumerate() {
            for (b, hb) in all.iter().skip(i + 1) {
                assert!(
                    !a.intersects(*b),
                    "{ha:?} and {hb:?} overlap, so one of them cannot be pressed"
                );
            }
        }
    }

    /// **The master takes no send; it takes a mute and a solo now.**
    ///
    /// It could do neither, on the argument that muting the master is turning
    /// it down and soloing it means nothing — and it left the master as the
    /// one column on the desk shaped unlike every other. The owner's call:
    /// "for click and master looking the same in mixer view … add mute and
    /// solo to the bottom". The mute is a real control (`Shared::master_mute`
    /// — a hard zero, the same one its automation lane could already write);
    /// the solo is a light, and the engine masks the master's bit out of the
    /// solo test so it cannot silence the desk it belongs to.
    ///
    /// The effects return still takes no send: a bus that can feed itself is
    /// a bus that howls.
    #[test]
    fn the_master_and_the_bus_are_not_offered_what_they_cannot_do() {
        let v = a_view();
        let l = Layout::new(rect(), &v);
        let master = &l.strips[MASTER];
        assert!(!master.send.is_positive(), "the master was given a send");
        assert!(master.mute.is_positive(), "the master lost its mute");
        assert!(master.solo.is_positive(), "the master lost its solo");
        assert!(master.fader.is_positive(), "the master has no fader");
        // **Its fold moved ABOVE the fader**, which is what freed the switch
        // row for the pair.
        assert!(master.mono.is_positive(), "the master lost its mono fold");
        assert!(
            master.mono.bottom() <= master.fader.top(),
            "the master's fold is not above its fader"
        );
        assert!(
            master.mute.top() >= master.fader.bottom(),
            "the master's mute is not below its fader"
        );

        // The click, before the master: a fader and both switches but NO
        // send — a metronome feeding the reverb is not a thing, and the send
        // was deleted rather than defaulted to zero.
        let click = &l.strips[CLICK];
        assert!(!click.send.is_positive(), "the click was given a send");
        assert!(click.mute.is_positive() && click.solo.is_positive());
        assert!(click.fader.is_positive(), "the click has no fader");
        // And an ordinary channel has both a send and its switches. Channel
        // 0, not the one just left of the click: only eight channels show at
        // once now, and the click and master are pinned past them.
        let track = &l.strips[0];
        assert!(track.send.is_positive() && track.mute.is_positive());
    }

    /// **Eight channels show, the rest scroll, and the master is pinned
    /// right.** The desk has more channels than fit; only a window of them is
    /// laid out, the click and master always are, and the master sits at the
    /// far right whatever the scroll.
    #[test]
    fn the_mixer_scrolls_channels_and_pins_the_master() {
        let r = rect();
        // At rest: channels 0..VISIBLE show, the rest are off-desk.
        let mut v = a_view();
        v.scroll_px = 0.0;
        let l = Layout::new(r, &v);
        for n in 0..VISIBLE_CHANNELS {
            assert!(l.strips[n].panel.is_positive(), "channel {n} should show");
        }
        for n in VISIBLE_CHANNELS..CHANNELS {
            assert!(!l.strips[n].panel.is_positive(), "channel {n} should be scrolled off");
        }
        // The click and the master are always laid out...
        assert!(l.strips[CLICK].panel.is_positive(), "the click vanished");
        assert!(l.strips[MASTER].panel.is_positive(), "the master vanished");
        // ...and the master is the rightmost thing on the desk.
        let master_x = l.strips[MASTER].panel.left();
        for s in &l.strips {
            if s.panel.is_positive() {
                assert!(
                    s.panel.left() <= master_x + 0.5,
                    "something sits right of the pinned master"
                );
            }
        }
        // Scrolled to the end: the last channels show, the master still pins.
        let mut v = a_view();
        v.scroll_px = (CHANNELS - VISIBLE_CHANNELS) as f32 * Layout::metrics(r, VISIBLE_CHANNELS).1;
        let l = Layout::new(r, &v);
        assert!(l.strips[CHANNELS - 1].panel.is_positive(), "the last channel never shows");
        assert!(!l.strips[0].panel.is_positive(), "channel 0 should be scrolled off");
        assert!(
            (l.strips[MASTER].panel.left() - master_x).abs() < 0.5,
            "the master moved when the channels scrolled"
        );
    }

    /// **The scroll is pixels now, and a half-position shows half strips.**
    /// Halfway between two columns the first column hangs off the region's
    /// left edge, an extra column is exposed on the right — and the hidden
    /// part of a strip answers no press, however far its rectangle reaches.
    #[test]
    fn a_half_scrolled_desk_shows_partial_strips_that_do_not_answer_outside() {
        let r = rect();
        let (_, pitch) = Layout::metrics(r, VISIBLE_CHANNELS);
        let mut v = a_view();
        v.scroll_px = pitch * 0.5;
        let l = Layout::new(r, &v);
        // Channel 0 is half off: its panel starts left of the region.
        assert!(l.strips[0].panel.is_positive(), "the partial column vanished");
        assert!(
            l.strips[0].panel.left() < l.region.left() - 0.5,
            "channel 0 does not overhang, so nothing scrolled by pixels"
        );
        // The extra column on the right is exposed.
        assert!(
            l.strips[VISIBLE_CHANNELS].panel.is_positive(),
            "the half position exposed no extra column"
        );
        // A press on the overhanging, clipped part of channel 0 lands on
        // nothing; the same control pressed inside the region still answers.
        let hidden = Pos2::new(
            (l.strips[0].panel.left() + l.region.left()) / 2.0 - 0.1,
            l.strips[0].mute.center().y,
        );
        assert!(
            hit_test(r, &v, hidden).is_none(),
            "the clipped part of a strip answered a press"
        );
        let seen = Pos2::new(l.region.left() + 2.0, l.strips[0].mute.center().y);
        if l.strips[0].mute.contains(seen) {
            assert_eq!(hit_test(r, &v, seen), Some(Hit::Mute(Column(0))));
        }
        // And the clamp holds: past the end lands at the end, not beyond.
        let mut v = a_view();
        v.scroll_px = 1.0e9;
        let l = Layout::new(r, &v);
        assert!(l.strips[CHANNELS - 1].panel.is_positive());
        assert!(
            l.strips[CHANNELS - 1].panel.right() <= l.region.right() + 0.5,
            "the clamp let the last channel drift past the region"
        );
    }

    /// **All sixteen fold into two rows of sixteen, size permitting.** The
    /// pinned click and master keep the full height at the right; every
    /// channel keeps the width eight-across always had; nothing scrolls
    /// because nothing is off-desk. Too short for a usable half-height
    /// strip, and the fold declines.
    /// **How many strips to show is a wish, and the width has the last word.**
    ///
    /// The count is a saved setting, so one made on a desktop travels to a
    /// laptop in the same file — and sixteen across a small screen is a row
    /// of toothpicks with a meter each too thin to read. The setting is left
    /// alone; it is simply not honoured past what the space can carry.
    #[test]
    fn a_narrow_desk_shows_fewer_strips_than_it_was_asked_for() {
        let narrow = Rect::from_min_size(Pos2::ZERO, Vec2::new(640.0, 320.0));
        let mut v = a_view();
        v.visible = CHANNELS;
        let l = Layout::new(narrow, &v);
        let drawn: Vec<f32> = l
            .strips
            .iter()
            .map(|s| s.panel.width())
            .filter(|w| *w > 0.0)
            .collect();
        assert!(!drawn.is_empty(), "the desk drew nothing at all");
        for w in &drawn {
            assert!(
                *w >= MIN_STRIP_W - 0.5,
                "a strip came out {w} wide, under the {MIN_STRIP_W} floor"
            );
        }
        // And a desk with room honours the whole wish, so the clamp only
        // ever takes away what would not have fitted.
        let wide = Rect::from_min_size(Pos2::ZERO, Vec2::new(2400.0, 320.0));
        let l = Layout::new(wide, &v);
        let shown = l.strips.iter().filter(|s| s.panel.width() > 0.0).count();
        assert!(
            shown >= CHANNELS,
            "a wide desk showed only {shown} of {CHANNELS}"
        );
    }

    /// **The desk is one row at any height, and the fold is gone.**
    ///
    /// Sixteen over sixteen existed to get every channel on screen at a
    /// usable width, and it paid for that with half of every fader and every
    /// meter — on the view whose whole job is reading levels. The owner
    /// asked for it removed; `default_mixer_visible` answers what it was for
    /// by opening a large window with all sixteen across instead.
    ///
    /// Driven at the height the fold used to fire at and well past it,
    /// because "it does not fold" is only worth asserting where it used to.
    /// Revert-check: put the two-row branch back in `Layout::new` and the
    /// tall case reports two tops.
    #[test]
    fn the_desk_never_folds_into_two_rows() {
        let mut v = a_view();
        v.visible = CHANNELS;
        for h in [300.0_f32, 904.0, 1400.0, 2000.0] {
            let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, h));
            let l = Layout::new(r, &v);
            let tops: std::collections::BTreeSet<i64> = l
                .strips
                .iter()
                .filter(|s| s.panel.is_positive())
                .map(|s| s.panel.top().round() as i64)
                .collect();
            assert_eq!(
                tops.len(),
                1,
                "at {h} points tall the desk laid out {} rows",
                tops.len()
            );
            // And every strip is the full height of the desk, which is what
            // the fold was spending.
            let full = l.strips[MASTER].panel.height();
            for n in 0..CHANNELS {
                if l.strips[n].panel.is_positive() {
                    assert!(
                        (l.strips[n].panel.height() - full).abs() < 0.6,
                        "channel {n} is {} tall against the master's {full}",
                        l.strips[n].panel.height()
                    );
                }
            }
        }
    }

    /// **The free desk ends at sixteen, everywhere the layout reaches.**
    /// The paywall's geometry: with `active` at the free count, the
    /// supporter lanes are never laid out, the scroll cannot reach them,
    /// and the fold never fires however big the screen - one scrolling
    /// row is the free mixer's whole shape, by the owner's word.
    #[test]
    fn a_free_desk_never_shows_or_reaches_the_supporter_lanes() {
        let tall = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 800.0));
        let mut v = a_view();
        v.active = crate::recorder::FREE_CHANNELS;
        v.visible = CHANNELS; // asks for everything; the clamp answers
        v.scroll_px = 1.0e9; // and so does the scroll's rail
        let l = Layout::new(tall, &v);
        for n in crate::recorder::FREE_CHANNELS..CHANNELS {
            assert!(
                !l.strips[n].panel.is_positive(),
                "supporter lane {n} was laid out on a free desk"
            );
        }
        // No fold: every visible channel shares one row.
        let tops: std::collections::BTreeSet<i64> = (0..crate::recorder::FREE_CHANNELS)
            .filter(|n| l.strips[*n].panel.is_positive())
            .map(|n| l.strips[n].panel.top().round() as i64)
            .collect();
        assert_eq!(tops.len(), 1, "the free desk folded");
        // And the scroll's ceiling respects the same edge.
        assert_eq!(
            Layout::max_scroll_px(tall, VISIBLE_CHANNELS, crate::recorder::FREE_CHANNELS),
            (crate::recorder::FREE_CHANNELS - VISIBLE_CHANNELS) as f32
                * Layout::metrics(tall, VISIBLE_CHANNELS).1,
        );
    }

    /// **The visible count is what the view says, not a constant.** Shift+
    /// wheel changes it; the layout must honour whatever number arrives.
    #[test]
    fn the_mixer_shows_as_many_channels_as_asked() {
        let r = rect();
        for want in [4usize, 8, 12] {
            let mut v = a_view();
            v.visible = want;
            v.scroll_px = 0.0;
            let l = Layout::new(r, &v);
            let shown = (0..CHANNELS).filter(|n| l.strips[*n].panel.is_positive()).count();
            assert_eq!(shown, want.min(CHANNELS), "asked for {want}, showed {shown}");
            assert!(l.strips[MASTER].panel.is_positive(), "the master must always show");
        }
    }

    /// A strip with no send gives that room to its meter rather than leaving a
    /// hole where a control would be.
    #[test]
    fn a_strip_without_a_send_has_a_taller_meter() {
        let v = a_view();
        let l = Layout::new(rect(), &v);
        assert!(
            l.strips[MASTER].meter.height() > l.strips[0].meter.height(),
            "the master left a hole where a send would have been"
        );
    }

    /// **Solo takes the others out of the mix, and the drawing says so.**
    #[test]
    fn soloing_one_strip_silences_the_rest() {
        let mut v = a_view();
        v.strips[2].soloed = true;
        v.any_solo = true;
        assert!(v.heard(2), "the soloed strip was silent");
        assert!(!v.heard(0), "an unsoloed strip was still heard");
        // And the master is never taken out by a solo, or soloing anything
        // would mute the app.
        assert!(v.heard(MASTER), "solo silenced the master");
    }

    /// **An empty slot offers one control, and it is the plus.**
    ///
    /// Not a dead fader, a dead send and two dead switches — which is exactly
    /// what the plugin rack was already doing wrong in the other direction.
    #[test]
    fn an_empty_slot_is_a_plus_and_nothing_else() {
        let mut v = a_view();
        v.strips[0].empty = true;
        let l = Layout::new(rect(), &v);
        let s = &l.strips[0];
        assert!(s.add.is_positive(), "there is no way to fill the slot");
        for (r, what) in [
            (s.fader, "fader"),
            (s.send, "send"),
            (s.mute, "mute"),
            (s.solo, "solo"),
            (s.meter, "meter"),
        ] {
            assert!(!r.is_positive(), "an empty slot was given a {what}");
        }
        assert_eq!(
            hit_test(rect(), &v, s.add.center()),
            Some(Hit::Add(Column(0))),
            "the plus does not open anything"
        );
        // And a filled one has no plus in the MIDDLE of it, where the fader
        // is. It keeps the same target on its icon — the picker is how you
        // change what is on a channel, filled or not — so what is checked is
        // that the two are not in two places: `add` is the icon exactly.
        let filled = Layout::new(rect(), &a_view());
        assert!(filled.strips[0].icon.is_positive(), "a filled slot has no icon");
        assert_eq!(
            filled.strips[0].add, filled.strips[0].icon,
            "the picker is somewhere other than the icon that opens it"
        );
        assert!(
            !filled.strips[0].add.contains(filled.strips[0].fader.center()),
            "the picker is over the fader"
        );
        assert_eq!(
            hit_test(rect(), &a_view(), filled.strips[0].icon.center()),
            Some(Hit::Add(Column(0))),
            "the icon does not open the picker"
        );
    }

    /// **An empty channel is pressable everywhere, not just on the plus.**
    ///
    /// A hundred points of empty column that ignores a press is a channel that
    /// looks dead until you find the one spot in the middle that is not.
    #[test]
    fn an_empty_channels_circle_adds_and_the_rest_is_quiet_ground() {
        let mut v = a_view();
        v.strips[0].empty = true;
        let r = rect();
        let l = Layout::new(r, &v);
        let panel = l.strips[0].panel;
        // The circle answers with ADD…
        assert_eq!(
            hit_test(r, &v, l.strips[0].add.center()),
            Some(Hit::Add(Column(0))),
            "the circled plus cannot be pressed"
        );
        // …and the rest of the panel is quiet ground — where the
        // right-click menu lives, and where a mis-aimed press adds nothing.
        for (what, at) in [
            ("the top-left", Pos2::new(panel.left() + 3.0, panel.top() + 3.0)),
            ("the bottom-right", Pos2::new(panel.right() - 3.0, panel.bottom() - 3.0)),
        ] {
            assert_eq!(
                hit_test(r, &v, at),
                None,
                "{what} of an empty channel is not quiet ground"
            );
        }
    }

    /// **The label sits at the same height on every column, master included.**
    ///
    /// The icon used to go above the name, and the master has no source to
    /// choose and so no icon — so twelve labels sat at one height and the
    /// master's at another, which reads as the master being a different kind
    /// of object rather than the same strip with nothing to pick.
    #[test]
    fn every_label_is_at_the_same_height_as_the_masters() {
        let v = a_view();
        let l = Layout::new(rect(), &v);
        let master = l.strips[COLUMNS - 1].name;
        assert!(master.is_positive(), "the master has no name plate");
        for (i, s) in l.strips.iter().enumerate() {
            if !s.name.is_positive() {
                continue;
            }
            assert!(
                (s.name.top() - master.top()).abs() < 0.5,
                "column {i}'s label is at {} where the master's is at {}",
                s.name.top(),
                master.top()
            );
            // And where a column has an icon it is UNDER the writing.
            if s.icon.is_positive() {
                assert!(
                    s.icon.top() >= s.name.bottom(),
                    "column {i} draws its icon over its own name"
                );
            }
        }
    }

    /// Perceived luminance, 0..=255, for the scale test below — weighted
    /// rather than averaged, because at equal numbers green reads far lighter
    /// than blue. Test-only: the panel itself measures with `contrast_ratio`.
    fn luma(c: Color32) -> f32 {
        0.299 * f32::from(c.r()) + 0.587 * f32::from(c.g()) + 0.114 * f32::from(c.b())
    }

    /// **Every swatch in the table can carry measured ink, once guarded.**
    ///
    /// The hand-picking grid predates the dead-zone rule, and three of its
    /// third tones — olive, green, teal — sit at the luminance where neither
    /// ink clears 4.5:1. Raw, they fail this test; through `legible_surface`,
    /// the way the strip and the arrangement now paint them, every index in
    /// the table carries its measured ink. The wrap must also leave the
    /// compliant colours untouched — a guard that repainted the whole table
    /// would be a silent palette change on every saved desk.
    #[test]
    fn every_swatch_carries_its_ink_once_the_dead_zone_guard_runs() {
        let v = a_view();
        let p = ink(&v);
        let mut moved = Vec::new();
        for (i, &(r, g, b)) in STRIP_COLORS.iter().enumerate() {
            let raw = Color32::from_rgb(r, g, b);
            let face = crate::recorder_panel::legible_surface(raw);
            let ink = print_on(flatten(face, p.wood), &p);
            let c = crate::recorder_panel::contrast_ratio(ink, face);
            assert!(
                c >= 4.5,
                "swatch {i} ({r:02x}{g:02x}{b:02x}) carries its ink at {c:.2}:1"
            );
            if face != raw {
                moved.push(i);
            }
        }
        // Exactly the three dead-zone tones, and no more: olive3, green3,
        // teal3. A fourth entry moving means somebody appended a swatch the
        // guard has to repaint, which palgen's --check exists to catch first.
        assert_eq!(
            moved,
            vec![11, 14, 17],
            "the guard moved a different set of swatches than the known three"
        );
    }

    /// **The scale beside a meter can be read on every channel.**
    ///
    /// The numbers were a faded ivory, chosen against the walnut the desk sits
    /// on — and every UNPAINTED channel has an ivory face, so on eight strips
    /// out of twelve the scale was ivory on ivory and simply absent. The
    /// owner's words were "ensure every single track has legible readout
    /// ticks", and the fault was never the size.
    ///
    /// Contrast rather than a colour: this has to hold for all twenty-seven
    /// paints as well as for the bare face, and naming the right answer for
    /// each of them is a table that would go stale the next time one changed.
    #[test]
    fn the_scale_can_be_read_on_every_face_a_channel_can_have() {
        let v = a_view();
        let p = ink(&v);
        let mut faces = vec![
            ("the bare face", p.face),
            ("a silent strip", p.face.gamma_multiply(0.62)),
        ];
        for (i, &(r, g, b)) in STRIP_COLORS.iter().enumerate() {
            faces.push((
                match i {
                    0 => "colour 0",
                    _ => "a painted strip",
                },
                // Through the guard, because that is what the panel paints.
                crate::recorder_panel::legible_surface(Color32::from_rgb(r, g, b)),
            ));
        }
        for (what, face) in faces {
            // Exactly what the panel does: the face over the desk, then the
            // ink over that.
            let face = flatten(face, p.wood);
            let printed = flatten(print_on(face, &p).gamma_multiply(0.62), face);
            let d = (luma(printed) - luma(face)).abs();
            assert!(
                d > 40.0,
                "{what} ({face:?}) prints its scale at a difference of {d:.0}, \
                 which is a mark where a number should be"
            );
        }
    }

    /// Every instrument slot is a channel, filled or not.
    #[test]
    fn the_desk_has_a_strip_for_every_slot() {
        let channels = Strip::shown();
        for n in 0..CHANNELS {
            assert_eq!(channels[n], Strip::Channel(n), "channel {n} has no column");
        }
        // **Drawn: the eight and the click.** BACKING retired with v13;
        // the drum bus column retired when the drumpad became a lane's
        // instrument — the kit answers to its lane's strip now, and the
        // bus row survives only as hidden desk plumbing. Only the effects
        // return keeps its off-desk life: its three knobs are the band's,
        // and a soloable bus is a feedback question.
        assert_eq!(
            COLUMNS,
            CHANNELS + 2,
            "a drawn channel went missing"
        );
        assert_eq!(channels.len(), CHANNELS + 1);
        assert!(!channels.contains(&Strip::Drums), "the retired drum bus came back");
        assert!(channels.contains(&Strip::Click), "the click lost its column");
        assert!(!channels.contains(&Strip::Fx), "the bus is drawn twice");
        // Every strip owns a distinct place in the arrays, or two of them
        // would share a send and mute together.
        let mut seen: Vec<usize> = Strip::all().iter().map(|s| s.index()).collect();
        let n = seen.len();
        seen.sort_unstable();
        seen.dedup();
        assert_eq!(seen.len(), n, "two channels share an index");
    }

    /// **Three inserts on every channel, and all three can be pressed.**
    ///
    /// There used to be ONE, on the effects bus alone. A desk where one
    /// channel can hold an effect and another cannot is a desk you have to
    /// remember the rules of.
    #[test]
    fn every_channel_has_three_inserts_you_can_fill() {
        let v = a_view();
        let r = rect();
        let l = Layout::new(r, &v);
        for (i, s) in l.strips.iter().enumerate() {
            if !s.panel.is_positive() || v.strips[i].empty {
                continue;
            }
            for n in 0..INSERTS {
                assert!(
                    s.inserts[n].is_positive(),
                    "channel {i} has no room for insert {n}"
                );
                assert_eq!(
                    hit_test(r, &v, s.inserts[n].center()),
                    Some(Hit::Insert(Column(i), n)),
                    "channel {i}'s insert {n} cannot be pressed"
                );
            }
            // **And each bay is as wide as the channel.** The whole reason
            // they were stacked: three across a hundred-point strip held about
            // three characters of a plugin's name, which is a rack you have to
            // click to read.
            for n in 0..INSERTS {
                assert!(
                    s.inserts[n].width() > s.panel.width() / INSERTS as f32 * 2.0,
                    "channel {i}'s insert {n} is no wider than a third of the \
                     strip, which is what stacking them was for"
                );
            }
            for n in 1..INSERTS {
                // **Down the strip now, not across it.** Each bay is as wide
                // as the channel — that is where the reading room came from —
                // so what separates them is the top edge of one against the
                // bottom of the one above.
                assert!(
                    s.inserts[n].top() >= s.inserts[n - 1].bottom(),
                    "channel {i}'s inserts {} and {n} overlap",
                    n - 1
                );
            }
        }
        // An EMPTY slot is an outline and a plus, and nothing else: three
        // effect chips on a channel with no instrument in it would be three
        // controls for something that is not there.
        let mut empty = a_view();
        empty.strips[0] = a_strip(Some(Strip::Channel(0)), true);
        let l = Layout::new(r, &empty);
        assert!(!l.strips[0].inserts[0].is_positive());
    }

    /// **A channel's name is a field, and a long one is cut.**
    ///
    /// A desk is a set of names before it is a set of faders — "BELL KEYS" is
    /// what the app knows and "Rhodes" is what the person at the desk knows —
    /// so every drawn channel has a name you can press and type into. And the
    /// name that arrives from an interface is "Scarlett 18i20 USB - 3", which
    /// has never fitted a strip a hundred points wide.
    #[test]
    fn every_channel_has_a_name_you_can_press() {
        let v = a_view();
        let r = rect();
        let l = Layout::new(r, &v);
        for (i, s) in l.strips.iter().enumerate() {
            if !s.panel.is_positive() {
                continue;
            }
            assert!(s.name.is_positive(), "channel {i} has nowhere to put a name");
            assert_eq!(
                hit_test(r, &v, s.name.center()),
                Some(Hit::Name(Column(i))),
                "channel {i}'s name cannot be pressed"
            );
        }
        // An EMPTY slot has no name to change: it is an outline and a plus,
        // and a name field on it would be a control for a channel that is not
        // there yet.
        let mut empty = a_view();
        empty.strips[0] = a_strip(Some(Strip::Channel(0)), true);
        let l = Layout::new(r, &empty);
        assert!(!l.strips[0].name.is_positive(), "an empty slot got a name field");
        assert_eq!(hit_test(r, &empty, l.strips[0].panel.center()), Some(Hit::Add(Column(0))));
    }

    /// **Every mark has to land on the scale it is drawn against.**
    ///
    /// The fader's travel is +12 down to -60. A tick outside that is clamped to
    /// an end and sits on top of its neighbour, which is a scale that says two
    /// different levels are the same height — the exact thing a scale exists
    /// to stop.
    #[test]
    fn every_meter_tick_lands_where_the_fader_would_put_it() {
        for (which, ticks) in [
            ("channel", &METER_TICKS[..]),
            ("master", &MASTER_TICKS[..]),
        ] {
            let mut seen: Vec<f32> = Vec::new();
            for &db in ticks {
                let t = gain_to_fader(10f32.powf(db / 20.0));
                assert!(
                    t > 0.0 && t < 1.0,
                    "{which}'s {db} dB mark sits at {t}, which is off the end of \
                     the travel and on top of whatever is there"
                );
                for other in &seen {
                    assert!(
                        (other - t).abs() > 0.01,
                        "{which} has two marks at {t}, so one of them is a label \
                         nobody can read"
                    );
                }
                seen.push(t);
            }
        }
        // The master column is the one you read a number off, and it is wider
        // — so it carries the scale a channel has no room for.
        assert!(
            MASTER_TICKS.len() > METER_TICKS.len(),
            "the master's scale is no fuller than a channel's"
        );
    }

    /// **The decibel readout is a target.** The thing the first version had
    /// no room for, and now the difference between a fader you fight and one
    /// you tell.
    #[test]
    fn a_number_can_be_typed() {
        let v = a_view();
        let r = rect();
        let l = Layout::new(r, &v);
        assert_eq!(
            hit_test(r, &v, l.strips[0].db.center()),
            Some(Hit::Db(Column(0))),
            "the number could not be clicked"
        );
        // The swatch grid moved into the TRACK dialog; the master's default
        // still has to BE in the table it indexes.
        assert!(MASTER_COLOR < STRIP_COLORS.len());
        assert_eq!(
            (MANUAL_COLORS - PALETTE_COLUMNS) % PALETTE_COLUMNS,
            0,
            "the neutrals and the hues do not line up into rows"
        );
    }


    /// **M, S and R are readable lit and unlit, on every theme.**
    ///
    /// Both branches used to be wrong at once. Lit, the letter took the ink
    /// for the strip FACE and printed it on the mute's amber or the arm's
    /// red. Unlit, it took the themed face colour AS INK and went
    /// dark-on-near-black the moment a theme picked a dark face.
    #[test]
    fn the_switch_letters_read_lit_and_unlit_on_every_theme() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                let mut v = a_view();
                v.dark_mode = dark;
                v.fx_strip = set.fx_strip_color.to_color32();
                let p = ink(&v);
                let grounds = [
                    ("unlit", p.track),
                    ("mute", crate::recorder_panel::legible_surface(p.mute)),
                    ("solo", crate::recorder_panel::legible_surface(p.solo)),
                    ("arm", crate::recorder_panel::legible_surface(p.arm)),
                ];
                for (what, ground) in grounds {
                    let letter = if crate::recorder_panel::wants_light_ink(ground) {
                        crate::recorder_panel::INK_LIGHT
                    } else {
                        crate::recorder_panel::INK_DARK
                    };
                    let r = crate::recorder_panel::contrast_ratio(letter, ground);
                    if r < 4.5 {
                        bad.push(format!(
                            "{} {}: {what} switch {r:.2}:1",
                            theme.name,
                            if dark { "dark" } else { "light" }
                        ));
                    }
                }
            }
        }
        assert!(bad.is_empty(), "switch letters unreadable:\n  {}", bad.join("\n  "));
    }

    fn rgb(i: usize) -> Color32 {
        let (r, g, b) = STRIP_COLORS[i];
        Color32::from_rgb(r, g, b)
    }

    /// Moved to `recorder_panel`, beside `contrast_ratio` and the two inks,
    /// once a second surface needed the same question answered.
    use crate::recorder_panel::delta_e;

    /// **A palette colour is a whole painted surface with small text on it.**
    ///
    /// Not a dot beside a name — the icon, the second line, the dial ticks and
    /// the decibel readout are all printed straight onto the channel. So every
    /// colour a palette can hand out has to carry that ink at full strength,
    /// and the ONLY thing standing between a palette and an unreadable rack is
    /// this test. It is the rule `scripts/palgen.py` solves for.
    #[test]
    fn every_auto_palette_colour_can_carry_the_ink_printed_on_it() {
        for pal in &PALETTES {
            for slot in (0..CHANNELS).map(|c| pal.slot(c)).chain([pal.click(), pal.master()]) {
                let face = rgb(slot);
                let ink = print_on(face, &ink(&a_view()));
                let ratio = crate::recorder_panel::contrast_ratio(face, ink);
                assert!(
                    ratio >= 4.5,
                    "{} slot {slot} {face:?} carries its ink at only {ratio:.2}:1",
                    pal.name
                );
            }
        }
    }

    /// Sixteen channels, sixteen colours, none of them repeated: the eight
    /// that cycled meant channel 8 wore channel 0's colour, so two tracks
    /// with nothing to do with each other matched.
    #[test]
    fn every_auto_palette_dresses_all_sixteen_channels_differently() {
        for pal in &PALETTES {
            let mut seen = std::collections::HashSet::new();
            for c in 0..CHANNELS {
                assert!(
                    seen.insert(pal.slot(c)),
                    "{} repeats a colour by channel {c}",
                    pal.name
                );
            }
            assert_eq!(seen.len(), CHANNELS, "{} is not sixteen wide", pal.name);
        }
    }

    /// **Neighbours have to be TELLABLE APART**, which is the entire job of
    /// colouring a channel.
    #[test]
    fn adjacent_channels_never_wear_the_same_colour_twice_over() {
        for pal in &PALETTES {
            // MONO is the exception it is allowed to be: sixteen steps inside
            // one ink-safe luminance range cannot be more than a few dE apart,
            // and being nearly uniform is what a monochrome theme is FOR.
            let floor = if pal.name == "Mono" { 3.0 } else { 10.0 };
            for c in 1..CHANNELS {
                let d = delta_e(rgb(pal.slot(c - 1)), rgb(pal.slot(c)));
                assert!(
                    d >= floor,
                    "{}: channels {} and {c} are {d:.1} apart, under {floor}",
                    pal.name,
                    c - 1
                );
            }
        }
    }

    /// The click and the master are not channels and must not read as one —
    /// the owner's rack had a fixed amber click and a fixed red master sitting
    /// beside sixteen channels that had all moved with the theme.
    #[test]
    fn the_click_and_the_master_stand_apart_from_every_channel() {
        for pal in &PALETTES {
            for (what, slot) in [("click", pal.click()), ("master", pal.master())] {
                let nearest = (0..CHANNELS)
                    .map(|c| delta_e(rgb(pal.slot(c)), rgb(slot)))
                    .fold(f32::MAX, f32::min);
                assert!(
                    nearest >= 8.0,
                    "{}'s {what} is only {nearest:.1} from a channel",
                    pal.name
                );
            }
            let split = delta_e(rgb(pal.click()), rgb(pal.master()));
            assert!(split >= 20.0, "{}: click and master are {split:.1} apart", pal.name);
        }
    }
    /// **One master red, on every theme, and it stands off every palette.**
    ///
    /// The desk used to derive a master per palette, so each theme's master
    /// was tuned to its own vividness. The owner reversed it: "make the
    /// master strip and track in mixer to be red", on every theme. A master
    /// tuned to each theme's palette is another dark rose in a rack of
    /// them, and the one channel you always look for should not have to be
    /// found.
    ///
    /// Each palette still CARRIES a master slot — `master_at` is written
    /// into saved desks and the runs are append-only history — and this is
    /// why the check is on `master()`, the thing the desk actually asks.
    ///
    /// Two of the three old floors survive and are the ones that matter:
    /// the ivory ink has to land on it, and it has to stand off every
    /// palette's own sixteen tracks, or a master that reads as track
    /// seventeen is no master. The third — that the masters differ from each
    /// other — is what has been deliberately given up.
    #[test]
    fn one_red_master_stands_off_every_palette() {
        const INK: Color32 = crate::recorder_panel::INK_LIGHT;
        let slot = PALETTES[0].master();
        for pal in &PALETTES {
            assert_eq!(
                pal.master(),
                slot,
                "{} brought a master of its own",
                pal.name
            );
        }
        assert!(slot < STRIP_COLORS.len(), "the master runs off the table");
        assert!(
            slot >= MANUAL_COLORS,
            "the master is inside the hand-picked swatches"
        );
        let ratio = crate::recorder_panel::contrast_ratio(INK, rgb(slot));
        assert!(
            ratio >= 4.5,
            "the master reads at {ratio:.2}:1 under the ink"
        );
        // **And it is not any palette's track.** The floor that made the
        // per-palette masters worth having in the first place; one shared
        // red has to clear it against all sixteen sets, not just its own.
        let mut close: Vec<String> = Vec::new();
        for pal in &PALETTES {
            for i in 0..CHANNELS {
                let d = delta_e(rgb(slot), rgb(pal.slot(i)));
                if d < 7.0 {
                    close.push(format!("{} track {i}: {d:.1} apart", pal.name));
                }
            }
        }
        assert!(
            close.is_empty(),
            "the master reads as somebody's track:\n  {}",
            close.join("\n  ")
        );
    }

    /// **Appending is the contract.** The hand-pickable swatches are the first
    /// twenty-seven and the palettes' own colours come after, so every index
    /// already written into somebody's saved desk still means what it meant.
    #[test]
    fn the_palettes_live_past_the_swatches_a_person_can_point_at() {
        assert_eq!(MANUAL_COLORS, 27, "the hand-picking grid changed size");
        for pal in &PALETTES {
            assert!(
                pal.base >= MANUAL_COLORS,
                "{} starts inside the hand-picked swatches",
                pal.name
            );
            assert!(pal.click() < STRIP_COLORS.len(), "{} runs off the table", pal.name);
        }
        assert_eq!(
            STRIP_COLORS[MANUAL_COLORS - 1],
            (0x86, 0x27, 0x6C),
            "the last hand-pickable swatch moved — saved desks just changed colour"
        );
    }

    /// **The insert menu offers two rows and swallows everything else.**
    ///
    /// A right-click used to unload the bay in one gesture; the menu is what
    /// stands between an evening of dialled-in settings and a mis-click. It
    /// is modal across the whole mixer: while it is up, a press is DELETE,
    /// REPLACE, or a dismissal — never someone else's fader.
    #[test]
    fn the_insert_menu_offers_delete_and_replace_and_nothing_else() {
        let mut v = a_view();
        v.fx_menu = Some((Column(0), 1));
        let r = rect();
        let l = Layout::new(r, &v);
        let rows = fx_menu_rows(&l.strips[0], 1, 2);
        for (i, row) in rows.iter().enumerate() {
            assert!(row.is_positive(), "row {i} has no area");
            assert!(
                l.strips[0].panel.contains(row.min) && l.strips[0].panel.contains(row.max),
                "row {i} hangs outside its own strip"
            );
        }
        assert_eq!(
            hit_test(r, &v, rows[0].center()),
            Some(Hit::FxMenu(Column(0), 1, FxAction::Delete))
        );
        assert_eq!(
            hit_test(r, &v, rows[1].center()),
            Some(Hit::FxMenu(Column(0), 1, FxAction::Replace))
        );
        // A press on ANOTHER strip's fader is a dismissal, not a fader.
        assert_eq!(
            hit_test(r, &v, l.strips[3].fader.center()),
            Some(Hit::FxMenu(Column(0), 1, FxAction::Dismiss))
        );
        // And with no menu open the same point is the fader again.
        assert_eq!(
            hit_test(r, &a_view(), l.strips[3].fader.center()),
            Some(Hit::Fader(Column(3)))
        );
    }

    /// **A key-capable bay's menu grows a SIDECHAIN row, and only then** —
    /// and every conditional row is positional under the same painter, so
    /// what this pins is that the hit test and the row list stay twins: a
    /// press on row N means the Nth thing `fx_menu_actions` offers, never a
    /// mis-aimed DELETE.
    ///
    /// Driven from that list rather than from hard indices, because the list
    /// is what both sides read; a test carrying its own copy of the order
    /// would go on agreeing with itself after the menu had grown.
    #[test]
    fn the_sidechain_row_appears_only_where_a_key_can_go() {
        let mut v = a_view();
        v.fx_menu = Some((Column(0), 1));
        v.fx_menu_key = true;
        let r = rect();
        let l = Layout::new(r, &v);
        let actions = fx_menu_actions(v.fx_menu_loaded, v.fx_menu_key, v.fx_menu_paste);
        assert!(
            actions.contains(&FxAction::Sidechain),
            "a key-capable bay was offered no sidechain row"
        );
        let rows = fx_menu_rows(&l.strips[0], 1, actions.len());
        for (i, row) in rows.iter().enumerate() {
            assert!(row.is_positive(), "row {i} has no area");
            assert!(
                l.strips[0].panel.contains(row.min) && l.strips[0].panel.contains(row.max),
                "row {i} hangs outside its own strip"
            );
        }
        for (row, action) in rows.iter().zip(&actions) {
            assert_eq!(
                hit_test(r, &v, row.center()),
                Some(Hit::FxMenu(Column(0), 1, *action)),
                "the rows and the actions have come apart"
            );
        }
        // Without the capability the same menu is one row shorter: the space
        // where it would sit is a dismissal like any other stray press.
        v.fx_menu_key = false;
        let plain = fx_menu_actions(v.fx_menu_loaded, v.fx_menu_key, v.fx_menu_paste);
        assert!(!plain.contains(&FxAction::Sidechain));
        let short = fx_menu_rows(&l.strips[0], 1, plain.len());
        assert_eq!(
            hit_test(r, &v, short[0].center()),
            Some(Hit::FxMenu(Column(0), 1, FxAction::Delete))
        );
        let last = short[short.len() - 1];
        let below = Pos2::new(last.center().x, last.bottom() + last.height());
        if !short.iter().any(|r| r.contains(below)) {
            assert_eq!(
                hit_test(r, &v, below),
                Some(Hit::FxMenu(Column(0), 1, FxAction::Dismiss)),
                "a plain bay's menu answered a row it does not have"
            );
        }
    }

    /// **COPY FX and PASTE FX, and what earns each row.**
    ///
    /// The pair is the answer to what a drag stopped doing when a carry
    /// inside one strip became a MOVE — see `FxAction::Copy`. A bay with
    /// nothing in it offers the paste ALONE: there is nothing there to
    /// delete, replace or copy, and a menu of dead rows is a menu people
    /// learn not to open.
    #[test]
    fn a_bay_offers_copy_always_and_paste_only_when_something_is_held() {
        let loaded = fx_menu_actions(true, false, false);
        assert!(loaded.contains(&FxAction::Copy), "a loaded bay cannot be copied");
        assert!(!loaded.contains(&FxAction::Paste), "a paste row with nothing held");
        let held = fx_menu_actions(true, false, true);
        assert_eq!(
            held,
            vec![FxAction::Delete, FxAction::Replace, FxAction::Copy, FxAction::Paste],
            "the held-effect menu is not the plain one plus a paste"
        );
        assert_eq!(
            fx_menu_actions(false, false, true),
            vec![FxAction::Paste],
            "an empty bay's menu is the paste and nothing else"
        );
        assert!(
            fx_menu_actions(false, true, false).is_empty(),
            "an empty bay with nothing held has a menu at all"
        );
    }

    /// **Only a removable channel wears the ×, and it can be pressed.**
    ///
    /// The master, the click and the bus cannot be taken off the desk, and a
    /// close button on them would be a control with no meaning behind it. On
    /// a channel that wears one, the name gives up the corner rather than
    /// sharing it.
    #[test]
    fn no_strip_wears_an_x_any_more() {
        // The × retired: removal lives behind the right-click TRACK menu,
        // where a destructive act has a named row. No strip lays one out
        // and no press can find one.
        let mut v = a_view();
        v.strips[1].removable = true;
        let l = Layout::new(rect(), &v);
        for (i, s) in l.strips.iter().enumerate() {
            assert!(!s.close.is_positive(), "column {i} still wears an ×");
        }
    }

    /// **M S R on a general channel; M S on the click; nothing armable
    /// anywhere else.** Arm is record-ready AND live — a thing only a
    /// channel can be — and all three switches have to be pressable where
    /// they are drawn.
    #[test]
    fn the_switch_row_is_m_s_r_on_channels_and_m_s_on_the_click() {
        let r = rect();
        // Only VISIBLE_CHANNELS show at once, so scroll each channel into the
        // window before checking it — the desk is wider than the desk shows.
        for n in 0..CHANNELS {
            let mut v = a_view();
            v.scroll_px = n.min(CHANNELS - VISIBLE_CHANNELS) as f32
                * Layout::metrics(r, VISIBLE_CHANNELS).1;
            let l = Layout::new(r, &v);
            let s = &l.strips[n];
            assert!(s.arm.is_positive(), "channel {n} has no R switch");
            assert_eq!(
                hit_test(r, &v, s.arm.center()),
                Some(Hit::Arm(Column(n))),
                "channel {n}'s R cannot be pressed"
            );
            assert_eq!(hit_test(r, &v, s.mute.center()), Some(Hit::Mute(Column(n))));
            assert_eq!(hit_test(r, &v, s.solo.center()), Some(Hit::Solo(Column(n))));
        }
        let v = a_view();
        let l = Layout::new(r, &v);
        assert!(
            !l.strips[CLICK].arm.is_positive(),
            "the click grew an arm switch; a metronome cannot be recorded"
        );
        assert!(l.strips[CLICK].mute.is_positive());
        assert!(
            !l.strips[MASTER].arm.is_positive(),
            "the master grew an arm switch"
        );
        // An empty slot has no switches at all — its one control is the plus.
        let mut empty = a_view();
        empty.strips[0] = a_strip(Some(Strip::Channel(0)), true);
        assert!(!Layout::new(r, &empty).strips[0].arm.is_positive());
    }

    /// Everything that travels does so vertically, and knows how far.
    #[test]
    fn every_travelling_control_has_an_axis_and_a_distance() {
        for hit in [Hit::Fader(Column(0)), Hit::Send(Column(1))] {
            // Faders, sends and pans stand up; a route strip lies down.
            let want = if matches!(hit, Hit::RouteGain(..)) {
                DragAxis::Horizontal
            } else {
                DragAxis::Vertical
            };
            assert_eq!(hit.axis(), Some(want), "{hit:?}");
            assert!(hit.travel().is_some_and(|t| t > 0.0), "{hit:?}");
        }
        for hit in [Hit::Mute(Column(0)), Hit::Solo(Column(0))] {
            assert_eq!(hit.axis(), None, "{hit:?} is not a thing you drag");
            assert_eq!(hit.travel(), None, "{hit:?}");
        }
    }

    /// It draws at the smallest band the app will ever hand it, without
    /// panicking and without putting anything outside the rect.
    #[test]
    fn it_survives_a_rect_far_too_small_to_be_useful() {
        let v = a_view();
        for size in [
            Vec2::new(1300.0, 420.0),
            Vec2::new(640.0, 180.0),
            Vec2::new(120.0, 60.0),
            Vec2::new(4.0, 4.0),
            Vec2::new(0.0, 0.0),
        ] {
            let r = Rect::from_min_size(Pos2::new(7.0, 11.0), size);
            let l = Layout::new(r, &v);
            for (region, hit) in l.targets() {
                assert!(
                    r.contains(region.center()),
                    "{hit:?} escaped a {size:?} mixer"
                );
            }
        }
    }
}

