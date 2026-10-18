//! The right-click context menu (spec §6) — the entire UI surface.
//!
//! Qt shows menus as their own top-level popup windows; the fixed 200px-tall
//! main window cannot host a ~460px menu, so the menu is rendered in its own
//! borderless immediate viewport at the global cursor position. An open
//! submenu is a second sibling viewport to its right (Qt-like placement) —
//! one viewport, reused, because only one submenu is ever open.
//!
//! Chrome parity (spec §6.1): bold Courier Prime, per-mode colors, item
//! padding 4px 20px, no rounding, 1px separators, toggle items rename
//! themselves (no checkmarks anywhere).
//!
//! The rows are grouped into CATEGORIES: the top level names a subject
//! (Window, Colors, Keyboard, Chords, Theory, Fretboard) and the hover carries
//! the verbs. It was twenty-six near-flat rows, which is a list to read rather
//! than a menu to aim at. **Two levels, and only two** — see `Entry::Submenu`
//! and `build_entries`, which is written around that limit rather than against
//! it.

use crate::fonts;
use crate::fretboard_panel;
use crate::host::Caps;
use crate::staff::Clef;
use egui::{Button, Color32, CornerRadius, FontId, Margin, Pos2, Rect, Stroke, Vec2};
use tangent_core::fretboard;
use std::time::Instant;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorTarget {
    WhiteIdle,
    BlackIdle,
    Active,
    Sustain,
    /// The chord label. Free to change; the bloom around it is the extra.
    ChordText,
    /// The Recorder band's own background.
    ///
    /// The band used to borrow the piano's, so it read as another band of the
    /// same window — which is right until you want to tell them apart at a
    /// glance while playing. Its own colour, and the band's ink follows the
    /// colour's brightness rather than the theme, so any choice stays readable.
    RecorderBg,
}

/// `Clone` but not `Copy`: `SetTuning` carries an owned name, because a custom
/// tuning's name is user input.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum MenuAction {
    SetSizePercent(i64),
    ToggleBorderless,
    SelectMidiInput,
    /// Stand media on the timeline at the playhead, on the selected track.
    InsertMedia,
    PickColor(ColorTarget),
    ToggleDarkMode,
    /// Wear a theme, by name. Factory names win over user ones — see
    /// `crate::theme::by_name`.
    ApplyTheme(String),
    /// Write the colours the app is wearing into the user themes folder.
    SaveThemeAs,
    ToggleKeytoggle,
    ToggleNotePreference,
    ToggleFretNoteNames,
    ToggleChordDetection,
    ToggleChordStrip,
    /// Cycle the built-in UI typeface (Courier Prime <-> Terminess).
    CycleFont,
    TeachChordName,
    ManageTaughtChords,
    /// D-UI-9: correct the current reading, training the learned re-ranker.
    CorrectChordName,
    /// D-UI-9: master switch for the learned re-ranker (weights are kept).
    ToggleChordLearning,
    /// Open the supporter-key dialog.
    ShowSupporterKey,
    /// Supporter decoration: show/hide the pixel heart.
    ToggleHeart,
    /// D-UI-15: the guitar view.
    ToggleFretboard,
    /// The interval column beside the neck.
    ToggleGuitarIntervals,
    /// Show or hide the camera pane beside the theory band.
    ToggleCameraPane,
    /// Step to the next clef preset.
    CycleClef,
    /// Set the staves outright, by key.
    SetStaffSet(&'static str),
    /// Choose THE stave — exclusive, one at a time: "grand" or a single
    /// clef's key. The multi-staff stack is a later feature; see the
    /// "Stave" category.
    SetStave(&'static str),
    /// The key signature, as a count of sharps (positive) or flats (negative).
    SetStaffKey(i32),
    /// A fresh, empty project beside the others, opened immediately.
    NewProject,
    /// Raise the picker for a `project.tangent` to open.
    OpenProject,
    /// Copy the whole project — document and media — somewhere, and open
    /// the copy. Save As, spelled honestly: a copy under a new name.
    DuplicateProject,
    /// Look for VST3 bundles again, now.
    RescanPlugins,
    /// Add another folder to the list of places that are looked in.
    AddPluginFolder,
    /// Forget every folder the user added, back to the standard paths.
    ClearPluginFolders,
    /// Name from `fretboard::TUNINGS`.
    /// `String`, not `&'static str`: a custom tuning's name is user input and
    /// has no static lifetime.
    SetTuning(String),
    /// Open the custom-tuning editor.
    ///
    /// A dialog, not a window, so it is offered under every `Caps` for the
    /// same reason "Teach Chord Name..." is: it needs no second OS window, no
    /// device and no size of its own. It is offered only while the fretboard is
    /// showing: an editor for something the user cannot see is a control with
    /// no visible effect.
    EditCustomTuning,
    SetCapo(u8),
    /// Fingerboard wood key (see `fretboard_panel::Wood`).
    SetWood(&'static str),
    /// D-UI-17: turn one theory diagram on or off. Independent, because any
    /// combination of the three may be showing at once.
    ToggleTheoryView(crate::theory_panel::View),
    /// Put every theory element back, in the numbered order. The way out of a
    /// collapsed band without having to remember which number is which.
    /// D-UI-17: whether the theory band follows live playing or stays put.
    ToggleTheoryFollowsMidi,
    /// §5: show and hide the Recorder band.
    ///
    /// This row is the ONLY way the feature is reachable. `show_recorder`
    /// defaults to false and is deliberately kept out of `first_launch()`, and
    /// this menu is the entire chrome — there is no menu bar and no preferences
    /// dialog — so an earlier draft of the recorder shipped completely
    /// invisible. Every row below exists because of that.
    ToggleRecorder,
    /// Draw the menu and the dialogs in the canvas, so a screen reader can
    /// reach them. See `Settings::screen_reader_mode`.
    ToggleScreenReaderMode,
    ShowAbout,
    /// Put one part of the settings back. Every one of these opens a
    /// confirmation — see [`crate::settings::ResetScope`], which is also
    /// where the words on the rows live.
    Reset(crate::settings::ResetScope),
    /// One row of the spectrum analyzer's menu: advance that choice. The
    /// rows print their current value, so a press is "the next one" and
    /// the label says what that was.
    Analyzer(crate::analyzer::Param),
}

/// Everything the menu needs to know to render its labels.
/// `Clone` but no longer `Copy`: `tuning` is a `String` since custom tunings
/// exist. Every call site takes it by value already, so the only cost is one
/// explicit `.clone()` where a view is reused.
#[derive(Clone)]
pub struct MenuView {
    /// The dialogs are drawn in the canvas for a screen reader, so the row
    /// offers the way back rather than the way in.
    pub screen_reader: bool,
    pub dark_mode: bool,
    /// The theme's strip face, already resolved for the mode the app is in.
    /// The menu is made of the same material as the desk under it.
    pub face: (u8, u8, u8),
    /// The theme now worn, so its own row can be greyed: this app has no
    /// checkmarks (Qt parity), and a row you cannot pick because you already
    /// have it says the same thing with the idiom that is already here.
    pub theme_name: String,
    /// Factory theme names, in menu order, Default first.
    pub factory_themes: Vec<String>,
    /// Whatever is in the user themes folder, by file name.
    pub user_themes: Vec<String>,
    pub borderless: bool,
    pub keytoggle: bool,
    pub prefer_flats: bool,
    pub key_note_names: bool,
    pub fret_note_names: bool,
    /// The interval column beside the neck is showing.
    pub guitar_intervals: bool,
    pub detection_enabled: bool,
    /// Whether the chord strip band is up. Independent of detection: the staff
    /// reads the same chord, so detection can be on with no strip at all.
    pub chord_strip: bool,
    /// "Teach Chord Name..." and "Correct Chord Name..." are greyed when no
    /// notes are held — both act on the voicing you are playing.
    pub notes_held: bool,
    /// D-UI-9: whether the learned re-ranker is currently influencing readings.
    pub learning_on: bool,
    /// Label of the typeface that will be active after the next Font click.
    /// `None` hides the row entirely (only Courier Prime is available here).
    pub next_font: Option<&'static str>,
    /// A valid supporter license is installed. Gates the extras block.
    pub supporter: bool,
    pub heart_on: bool,
    /// D-UI-15: the guitar view is showing. Its Tuning and Capo submenus are
    /// hidden while it is off, rather than offered and inert.
    pub fretboard_on: bool,
    /// The tuning currently in use, by name.
    ///
    /// `String` rather than `&'static str` since custom tunings exist: a
    /// user-entered name has no static lifetime, and the alternative — leaking
    /// it to fake one — would leak a little more on every edit.
    pub tuning: String,
    pub capo: u8,
    pub wood: &'static str,
    /// D-UI-17: which theory diagrams are showing.
    pub theory: crate::theory_panel::Views,
    /// D-UI-17: whether the theory band follows live playing.
    pub theory_follows_midi: bool,
    /// §5: the Recorder band is showing.
    ///
    /// Named to match `fretboard_on` rather than the `show_recorder` setting it
    /// comes from: the two rows behave identically, and this is the label's
    /// input, not the file's key.
    pub recorder_on: bool,
    /// The camera pane beside the theory band is showing.
    pub camera_pane_on: bool,
    /// How many folders the user has added to the plugin search list.
    pub extra_plugin_folders: usize,
    /// The take folder is remembered between sessions.
    /// The take's folder opens when it finishes.
    pub open_when_done: bool,
    /// The sheet music band is showing.
    pub staff_on: bool,
    /// Letter names are printed inside the noteheads.
    pub staff_note_names: bool,
    /// Which staves, as the settings file spells it.
    pub staff_set: String,
    /// The user's own stack of clefs, if they have one, ready to be listed.
    pub staff_custom_label: Option<String>,
    /// The staff set's own key ("grand", a clef key, or custom:...), for
    /// the exclusive Stave list's bullet.
    pub stave_key: String,
    /// Which clefs are on screen right now, by key, for the Staves list.
    pub staff_clefs: Vec<String>,
    /// The right-click landed on the sheet music panel.
    pub staff_first: bool,
    /// The key signature, sharps positive.
    pub staff_key: i32,
    /// §5: the take's time signature, so the count-in submenu can say what a
    /// bar is worth.
    pub time_signature: crate::recorder::TimeSignature,
    /// §5: the count-in length in BARS.
    pub count_in_bars: u32,
    /// §5: whether the count-in is written into the take.
    pub count_in_in_take: bool,
    /// The menu was opened by right-clicking the Recorder band, so the
    /// Recorder's own category and its choice lists come FIRST.
    ///
    /// The rest of the menu is unchanged: this reorders, it does not filter. A
    /// context menu that showed only the thing under the pointer would be a
    /// second menu to learn, and the one thing everybody does with a right
    /// click — dark mode, the fretboard — would stop working depending on
    /// where they clicked.
    pub recorder_first: bool,
    /// The analyzer's current choices while the camera pane is showing it,
    /// so its rows can print them; `None` hides the category, because a
    /// menu for an instrument that is not on screen is a menu for nothing.
    pub analyzer: Option<crate::analyzer::Config>,
    /// The GLOBAL cut of the menu: Project, Plugin folders, MIDI input,
    /// Window, Colors and the tail — nothing about the piano, the theory
    /// band or the neck. What a right-click on the ARRANGEMENT or the MIDI
    /// editor opens: those surfaces have their own gestures for their own
    /// things, and a menu full of another surface's rows there is noise.
    pub global_only: bool,
    /// §5: the live count-in, in beats. Comes from `Settings::count_in_beats()`,
    /// which has already clamped a stray value from a later build's file to
    /// something this menu can mark.
    pub count_in_beats: u32,
    /// The click is running.
    pub metronome_on: bool,
    /// The click is mixed into the recording as well as the monitors.
    pub metronome_in_take: bool,
    /// §5: the running clock is hidden while recording.
    pub hide_elapsed: bool,
    /// What the host allows. Rows whose action needs a window, a device list,
    /// or control of its own size are not shown where they cannot work — an
    /// inert row is worse than an absent one, because the user cannot tell
    /// whether they mis-clicked or the app is broken.
    pub caps: Caps,
}

#[derive(Clone, Copy)]
pub struct MenuColors {
    pub bg: Color32,
    pub text: Color32,
    pub sel: Color32,
    pub sep: Color32,
}

/// The right-click menu's colours, **derived from the desk it belongs to**.
///
/// `face` is the theme's strip face — the same role the mixer's bays and the
/// arrangement's chips wear — so a menu opened over any surface in the app is
/// made of the same material as that surface. On the DEFAULT theme that face
/// is the shipped bone, so the menu keeps the cream-and-black signature it
/// has always had; every other theme gets its own, and the text is measured
/// rather than assumed.
///
/// It was a frozen pair before: black-on-cream light, cream-on-black dark, on
/// all sixteen themes.
pub fn colors(dark_mode: bool, face: Color32) -> MenuColors {
    let _ = dark_mode;
    let p = crate::recorder_panel::surface_of(face);
    MenuColors { bg: p.bg, text: p.ink, sel: p.raised, sep: p.ink }
}

const MENU_FONT_SIZE: f32 = 13.0;
const PAD_X: f32 = 20.0; // Qt item padding 4px 20px
const PAD_Y: f32 = 4.0;
const SEP_H: f32 = 3.0; // 1px line + 1px margin above/below
/// **Three rungs, at the owner's word**: "make only options 75%, 100%, and
/// 125% available in the right click --> Window menu."
///
/// It was seven, from 50 to 200, and the ends were not sizes anybody chose
/// twice — 50% is unreadable and 200% is off the side of the monitor the
/// share is measured against. The middle moved with them: `BASE_OF_MONITOR`
/// grew a quarter, so this 100% is the old 125% and this 125% is past
/// anything the old list offered.
const SIZE_PERCENTS: [i64; 3] = [75, 100, 125];
/// Highest capo offered. Past this it stops being a capo and starts being a
/// different instrument, and the list has to end somewhere.
const CAPO_MAX: u8 = 9;
const ARROW: &str = "\u{23F5}"; // ⏵ submenu indicator
/// The most a menu may be squeezed to fit a short editor before it stops being
/// worth reading and the scroll fallback takes over instead.
const MIN_SQUEEZE: f32 = 0.62;
const MIN_ROW_H: f32 = 13.0;
/// Layers in the inline menu's drop shadow.
const SHADOW_STEPS: u32 = 5;

/// Total height of a menu whose rows are `row_h` tall.
///
/// Separate from the loop that builds `subs` because the squeeze has to know
/// the answer BEFORE choosing `row_h`, and computing it two different ways is
/// how a menu ends up one row taller than the space it was measured for.
fn measured_height(entries: &[Entry], row_h: f32) -> f32 {
    entries
        .iter()
        .map(|e| match e {
            Entry::Separator => SEP_H,
            _ => row_h,
        })
        .sum()
}

/// Pull an open submenu back onto the monitor.
///
/// Slide up rather than off the bottom, and flip to the menu's LEFT rather than
/// off the right edge, which is what a native menu does. `menu_left` is the
/// parent menu's left edge, so the flipped submenu lands against it.
///
/// A function rather than four lines inside `show` because the test used to
/// re-implement it, and a re-implemented clamp passes while the real one is
/// wrong. Grouping the menu into categories made that a much bigger risk:
/// Chords, Theory, Fretboard, Wood, Tuning and Capo all open from rows below
/// the halfway mark, so nearly every hover in the menu now depends on this.
/// **The box the screen actually occupies, or `None` when we cannot say.**
///
/// This is the fault behind "the right click context menu is jumping to the
/// top", reported three times, and it is not in the menu's logic at all.
///
/// egui hands the app a monitor SIZE and no origin: `ViewportInfo::
/// monitor_size` is `window.current_monitor().size()`, full stop. Both
/// clamps here then treated the screen as the box from `(0, 0)` to that
/// size. On a single display that is true. On a SECOND display it is not:
/// window positions on macOS are global across the whole desktop, so a
/// window on a monitor placed below or to the right of the primary one has
/// coordinates outside that box entirely. Every `pos + size > mon` test is
/// then true, and the clamp does what it is told - it moves the menu, and
/// its submenu, to `mon - size`, which is the top of the PRIMARY display's
/// coordinate box. The menu jumps to the top, every time, on the external
/// display, and not once on the built-in.
///
/// The honest answer with the data available: clamp only when the anchor is
/// inside the box we were given, because only then do we know the box is
/// the screen the menu is on. A menu near the edge of a second display may
/// now run off it, which is a far smaller fault than being teleported, and
/// the real fix is a monitor RECT from the host, which knows.
fn screen_box(anchor: Pos2, screen: Option<Rect>) -> Option<Rect> {
    let screen = screen?;
    if !screen.is_positive() {
        return None;
    }
    // Where the host could name the display, this is that display and the
    // anchor is on it. Where it could not, the box is the size-only guess
    // and containing the anchor is the only evidence that it is the right
    // screen at all.
    screen.contains(anchor).then_some(screen)
}

fn clamp_submenu(pos: Pos2, size: Vec2, menu_left: f32, screen: Rect) -> Pos2 {
    let mut p = pos;
    if p.y + size.y > screen.max.y {
        p.y = (screen.max.y - size.y).max(screen.min.y);
    }
    if p.x + size.x > screen.max.x {
        p.x = (menu_left - size.x).max(screen.min.x);
    }
    p
}

/// One row inside a submenu.
///
/// A `(String, MenuAction)` pair until the menu was grouped into categories.
/// It carries `enabled` for the same reason the top-level `Item` does: "Teach
/// Chord Name..." and "Correct Chord Name..." act on the voicing you are
/// holding and grey out without one, and grouping moved them a level down. A
/// submenu item that could not be greyed would have silently re-enabled them,
/// and the teach dialog would open with nothing to teach.
struct SubItem {
    label: String,
    action: MenuAction,
    enabled: bool,
}

enum Entry {
    Separator,
    Item {
        label: String,
        action: MenuAction,
        enabled: bool,
    },
    /// A parent row that opens a sibling viewport to its right. There used to
    /// be exactly one of these (Size), hard-coded from the entry list all the
    /// way down to the viewport; the guitar view needs two more, so the whole
    /// path is now driven by the list.
    ///
    /// **Items, not entries: this is the last level.** A submenu holds rows and
    /// nothing else, and `show` draws exactly one sibling popup, so there is no
    /// third level to nest into. Every category below is designed around that.
    Submenu {
        label: String,
        items: Vec<SubItem>,
    },
}

/// Push a category, as a hover when there is something worth hovering for.
///
/// The shape follows the content, which is what keeps `Caps` honest: under
/// `Caps::NONE` the Window category loses Borderless and the reader row, so a
/// category can arrive here with one item or none. An empty
/// hover is a dead end the user cannot tell from a bug, and a one-item hover
/// charges a hover to reach a single row. So nothing vanishes, one item is
/// drawn as an ordinary row, and two or more open.
///
/// It also gives the fretboard block its shape for free: with the guitar view
/// off there is only "Show Fretboard" to say, so the category IS that row.
fn push_category(entries: &mut Vec<Entry>, label: &str, mut items: Vec<SubItem>) {
    match items.len() {
        0 => {}
        1 => {
            let only = items.remove(0);
            entries.push(Entry::Item {
                label: only.label,
                action: only.action,
                enabled: only.enabled,
            });
        }
        _ => entries.push(Entry::Submenu {
            label: label.to_owned(),
            items,
        }),
    }
}

/// Where an open submenu goes and how big it is. Measured at open time with
/// the rest of the menu, because a Qt menu is static once it is showing.
struct SubGeom {
    row_top: f32,
    size: Vec2,
}

pub struct MenuState {
    pos: Pos2, // global (monitor points), top-left
    size: Vec2,
    entries: Vec<Entry>,
    row_h: f32, // uniform item height; buttons are forced to it so the
    // stacked rows exactly fill the computed viewport size
    /// Geometry per submenu, in entry order.
    subs: Vec<SubGeom>,
    /// Which submenu is showing, as an index into `subs`. At most one, which
    /// is why they can all share a single viewport id.
    submenu_open: Option<usize>,
    /// The state the pointer is asking for, and when it started asking.
    ///
    /// **The diagonal-travel fix.** Submenus open to the RIGHT, so reaching one
    /// means moving right and usually down or up — and every row crossed on the
    /// way is a row that hovers. Without a dwell, the submenu under the pointer
    /// changes on the way to the one that was wanted, and what opens is
    /// whichever row the path happened to end on. The owner's report was that
    /// hovering anything gave the TOP category about eight times in ten.
    ///
    /// `Some(i)` is a submenu to open and `None` is "close what is showing", so
    /// opening, switching and closing all wait the same way. See
    /// `settle_submenu` for why none of the three may be instant.
    pending_sub: Option<(Option<usize>, Instant)>,
    /// Where the pointer has been sitting, and since when, in the menu's own
    /// coordinates. `note_rest` owns it; nothing else may write it.
    rest: Option<(Pos2, Instant)>,
    /// Where the pointer first was, in the menu's own coordinates.
    ///
    /// **The menu opens AT the cursor**, so on the frame it appears the pointer
    /// is already sitting on its first row — and the first row is a submenu.
    /// That is the whole of "it always opens the top one": nobody pointed at
    /// Window, the menu was placed under a pointer that happened to be there.
    first_pointer: Option<Pos2>,
    /// Whether the pointer has moved far enough since then to count as
    /// pointing at something. Sticky once true.
    armed: bool,
    /// **Where the open panel is, in the same coordinates the parent reads
    /// its pointer in.**
    ///
    /// The corridor needs it; see `heading_for`. Set on the frame the panel
    /// is placed, so it is one frame behind on the first frame of a new
    /// panel, which is the frame the pointer is still on the parent row
    /// anyway.
    sub_rect: Option<Rect>,
    /// Where the pointer was on the previous frame, in the parent's own
    /// coordinates: the apex of the corridor. See `heading_for`.
    last_pointer: Option<Pos2>,
    /// When the pointer first entered the corridor, so a hand that stops
    /// inside it can still choose a different category. See `CORRIDOR_FOR`.
    corridor_since: Option<Instant>,
    /// **The submenu's own arming**, the parent's rule applied to its panel:
    /// where the pointer was when the panel last (re)opened, in the panel's
    /// own coordinates, and whether it has moved `ARM_SLOP` inside it since.
    /// Until it has, the panel draws no hover at all.
    ///
    /// On the desktop the panel is its own window with its own pointer
    /// stream, and the one surface serves every submenu in turn - it moves
    /// and refills as the pointer travels down the parent. A window the
    /// pointer is not in reports the LAST position it saw there, which is
    /// the row the hand left by; refilled for another submenu, that row lit
    /// before the hand had touched the panel. The owner: "jumps to the top
    /// selection when entering a submenu again." Reset whenever
    /// `submenu_open` changes, so every panel earns its hover afresh.
    sub_first_pointer: Option<Pos2>,
    sub_hover_armed: bool,
    /// The screen the menu is on, as a RECTANGLE and not merely a size.
    /// See `screen_box`: a size alone is not a screen on a second display.
    ///
    /// Kept so a submenu can be clamped too, not just the menu. Tuning and
    /// Capo sit near the BOTTOM of a long menu and Capo is ten rows deep, so
    /// unclamped they run off the screen and their lower rows cannot be
    /// clicked. Size never hit this: it is the first row and seven rows tall.
    monitor: Option<Rect>,
    dark_mode: bool,
    face: (u8, u8, u8),
    opened_at: Instant,
    saw_focus: bool,
    /// Text size multiplier, below 1.0 only when the menu had to be squeezed
    /// to fit a short editor. Applied to the row font so the labels shrink
    /// with the rows rather than overflowing them.
    font_scale: f32,
    /// Captured at open time, not read from the app each frame, so a menu can
    /// never be half-drawn as a window and half as a layer.
    caps: Caps,
}

/// Whether the pointer has come to REST, tracked here rather than read from
/// egui.
///
/// `input.pointer.velocity()` looks like the obvious answer and is a trap. It
/// is `Vec2::ZERO` until three positions have been sampled over at least ten
/// milliseconds, and the history is CLEARED whenever the pointer leaves the
/// window (`Event::PointerGone`, `input_state`). So it reports "not moving"
/// for the first two frames of every gesture, and again on every crossing
/// between the menu window and its panel — which is exactly when a menu must
/// not act. That is what let a pointer travelling towards one submenu open
/// whichever row it was passing over.
///
/// This asks the only question that matters and cannot be faked by a gap in
/// the samples: has the pointer been within `REST_SLOP` of one place for
/// `REST_FOR`? A hand pushing a mouse across a row moves further than the slop
/// every frame and can never accumulate the time.
fn note_rest(rest: &mut Option<(Pos2, Instant)>, pointer: Option<Pos2>, now: Instant) -> bool {
    let Some(p) = pointer else {
        // Not in this window at all, so not resting in it. Starting over on
        // the way back in is the point: re-entry is a crossing, not an arrival.
        *rest = None;
        return false;
    };
    match *rest {
        Some((anchor, since)) if (p - anchor).length() <= REST_SLOP => {
            now.duration_since(since) >= REST_FOR
        }
        _ => {
            *rest = Some((p, now));
            false
        }
    }
}

/// Decide which submenu is open, given what the pointer is on.
///
/// Its own function because it is the whole of the bug the owner reported and
/// none of it is about drawing: reaching for a submenu opened a different one,
/// the TOP category most often, because a panel that opens to the RIGHT is
/// reached by travelling across the rows in between and every one of them
/// hovers on the way past.
///
/// **Every change of state waits, and they are all the same change.** Opening
/// the first submenu, switching to another and closing on a plain row are one
/// mechanism with one rule: the wanted state is remembered, and it commits
/// when the pointer comes to rest or after `SUB_SWITCH_DWELL` on the same
/// target. The version before this one made opening the FIRST submenu instant,
/// on the reasoning that with nothing open there was no journey to protect.
/// But the menu opens UNDER the cursor with nothing open, so "the first one"
/// is whichever row the menu happened to land on, and it opened the moment the
/// pointer twitched — six points of movement, less than half a row. That is
/// the jump to the top of the menu.
///
/// Closing waits for the same reason. A plain row used to shut the panel on
/// the frame it was crossed, so travelling from one category to another past
/// an ordinary row destroyed the panel window and built it again — visible as
/// a flicker everywhere and, on Windows, as a white flash.
///
/// `clicked` is the deliberate answer and skips all of it: pressing a category
/// opens it on that frame, whatever the pointer is doing.
///
/// Returns whether a change is still pending, so the caller can ask for the
/// frame that will complete it.
#[allow(clippy::too_many_arguments)]
fn settle_submenu(
    open: &mut Option<usize>,
    pending: &mut Option<(Option<usize>, Instant)>,
    hovered: Option<usize>,
    hovered_plain_row: bool,
    armed: bool,
    still: bool,
    clicked: bool,
    // The pointer is inside the corridor to the open panel, so the rows it
    // is crossing are on the way to somewhere and not a choice. See
    // `heading_for`.
    travelling: bool,
    now: Instant,
) -> bool {
    // **Nothing at all until the pointer has moved.** The menu is placed under
    // the cursor, so the row it appears beneath was never chosen — acting on it
    // is how every single opening produced the top submenu.
    if !armed {
        return false;
    }
    // **Travelling beats resting.** A hand on its way to the open panel is
    // crossing rows, not picking them, and a pause halfway across is still
    // the same journey. A PRESS still decides, below: the corridor withholds
    // a guess and never a decision.
    if travelling && !clicked {
        *pending = None;
        return false;
    }
    // A press says which one, and says it now. This is the escape hatch: when
    // the dwell feels wrong, clicking the category is exact.
    if let Some(i) = hovered.filter(|_| clicked) {
        *open = Some(i);
        *pending = None;
        return false;
    }
    // What the pointer is asking for: a submenu, or none at all. Being over
    // NOTHING is not an answer — the gap between the menu and its panel is
    // "nothing", and closing there would put the panel out of reach on any
    // host that does not place the two flush.
    let want = match (hovered, hovered_plain_row) {
        (Some(i), _) => Some(i),
        (None, true) => None,
        (None, false) => {
            *pending = None;
            return false;
        }
    };
    if want == *open {
        // Already showing, and any half-finished change is stale — which is
        // what coming back to the open row means.
        *pending = None;
        return false;
    }
    if still {
        *open = want;
        *pending = None;
        return false;
    }
    match *pending {
        Some((w, at)) if w == want && now.duration_since(at) >= SUB_SWITCH_DWELL => {
            *open = want;
            *pending = None;
            false
        }
        Some((w, _)) if w == want => true,
        _ => {
            *pending = Some((want, now));
            true
        }
    }
}


/// **Is the pointer on its way to the open panel?**
///
/// The classic safe triangle, and the reason a delay alone was not enough.
/// A panel that would overflow the bottom of the screen is pulled UP by
/// `clamp_submenu`, sometimes a long way: most of this menu's categories
/// open from rows below the halfway mark and the deepest are ten rows tall.
/// Reaching such a panel means travelling up and right, ACROSS the rows in
/// between, and every one of those rows is a submenu that would open if the
/// hand paused on it for `SUB_SWITCH_DWELL`. The owner's report, twice:
/// "the right click context menu is still jumping to the top when a submenu
/// is selected."
///
/// So while a panel is open, the triangle from where the pointer was to that
/// panel's near edge is a corridor: a pointer inside it is going somewhere,
/// not choosing, and the rows it crosses do not count. This is what Apple's
/// menus do and what the note on `SUB_SWITCH_DWELL` said a delay would stand
/// in for. It does not.
///
/// `from` is the previous pointer position, `to` the panel. Both in the same
/// space, which the caller is responsible for.
fn heading_for(from: Pos2, now: Pos2, panel: Rect) -> bool {
    if !panel.is_positive() {
        return false;
    }
    // **A hand that has not moved is resting, not travelling.** The apex of
    // the triangle is the previous position, so a stationary pointer sits
    // exactly on it and would read as inside for as long as it stayed
    // there: the corridor would hold off every choice until its cap ran
    // out. Resting is the gesture the dwell exists to serve.
    if (now - from).length() < REST_SLOP {
        return false;
    }
    // The near edge: the side of the panel that faces the menu. A panel
    // flipped to the LEFT by the clamp faces the other way, and the whole
    // point of a corridor is that it works for that one too.
    let near_x = if panel.center().x >= from.x { panel.left() } else { panel.right() };
    // Widened by a row's worth so a hand that clips the corner of the
    // triangle is still travelling rather than choosing.
    let top = Pos2::new(near_x, panel.top() - CORRIDOR_SLOP);
    let bottom = Pos2::new(near_x, panel.bottom() + CORRIDOR_SLOP);
    // Inside the triangle (from, top, bottom), by the sign of the three
    // cross products. Degenerate triangles answer false, which is the safe
    // side: it means the ordinary dwell decides.
    let side = |a: Pos2, b: Pos2, p: Pos2| (b.x - a.x) * (p.y - a.y) - (b.y - a.y) * (p.x - a.x);
    let d1 = side(from, top, now);
    let d2 = side(top, bottom, now);
    let d3 = side(bottom, from, now);
    let neg = d1 < 0.0 || d2 < 0.0 || d3 < 0.0;
    let pos = d1 > 0.0 || d2 > 0.0 || d3 > 0.0;
    !(neg && pos)
}

/// How far outside the panel's own height the corridor still reaches. One
/// row, so a hand that cuts the corner is still read as travelling.
const CORRIDOR_SLOP: f32 = 24.0;

/// How long the corridor may hold a switch off before the hand is taken to
/// have stopped rather than travelled.
///
/// Apple caps its triangle the same way. Without a cap, a pointer parked
/// inside the corridor - which includes a good deal of the panel's own
/// approach - would never be able to choose a different category at all.
const CORRIDOR_FOR: std::time::Duration = std::time::Duration::from_millis(400);

/// **How long until a wait that began at `since` is over** - what the menu
/// asks its next frame for while a dwell or a rest is running.
///
/// It used to ask for the whole `SUB_SWITCH_DWELL` (or `REST_FOR`) again on
/// every frame, and that was fine for as long as the window painted sixty
/// frames a second regardless: the state machine was sampled every 16 ms and
/// the dwell ended within a frame of when it should. The window rests now
/// (2026-09-15), so the menu's own asks are the only frames it gets - and
/// egui subtracts its predicted frame time from every ask "to make it less
/// likely we over-shoot the target". A 140 ms ask therefore landed at
/// 124 ms, sixteen short of the dwell, and that frame asked for another
/// full 140: every switch took two rounds, 248 ms, and a rest 88 instead
/// of 60. The owner: "the right click main context menu is finnicky."
///
/// Asking for what is LEFT lands the second frame at the deadline itself,
/// which is when the dwell was designed to end.
fn due_in(wait: std::time::Duration, since: Instant, now: Instant) -> std::time::Duration {
    wait.saturating_sub(now.duration_since(since))
}

/// How long the pointer must stay on a different row before the menu acts.
///
/// Short enough to feel immediate when it is where you meant to go, long enough
/// to survive crossing two or three rows on the way to the one that is already
/// open. Apple's own menus use a shape (a triangle toward the open panel)
/// rather than a delay; a delay is a fraction of the code and covers the same
/// gesture, and unlike the triangle it also survives a submenu that opened to
/// the LEFT because it was clamped against the screen edge.
const SUB_SWITCH_DWELL: std::time::Duration = std::time::Duration::from_millis(140);

/// How far the pointer may drift and still count as resting in one place.
///
/// One point. A mouse that has been let go sends no events at all, so this is
/// only about sub-pixel drift and trackpad jitter; anything a hand is pushing
/// crosses it within a single frame.
const REST_SLOP: f32 = 1.0;

/// How long it must stay there. Four frames at 60 Hz: imperceptible once you
/// have arrived, and unreachable while you are still moving.
const REST_FOR: std::time::Duration = std::time::Duration::from_millis(60);

/// How far the pointer must move before the menu will act on what it is over.
///
/// Small: the point is only to tell "the menu appeared under my cursor" from
/// "I moved onto a row". It no longer has to carry the whole fix on its own —
/// nothing opens without a rest or a dwell now — so it stays at half a row,
/// where a whole row would make the first deliberate hover feel dead.
const ARM_SLOP: f32 = 6.0;

/// Stable surface identities. As windows these are viewport ids; inline they
/// are `Area` ids. One string each, so the two paths cannot drift.
const MENU_ID: &str = "tangent-menu";
const SUBMENU_ID: &str = "tangent-menu-sub";

/// The menu, in four compartments.
///
/// **Everything here is either global or about a surface you can see.** It was
/// twenty-six rows deep and almost entirely flat, then it was twenty-five
/// hovers — a list of every subject the app has, whether or not the thing it
/// configures was on screen. It is now five blocks with a rule each: what is
/// true everywhere, then the piano's own, the theory band's own, the guitar's
/// own and the recorder's own, each appearing only while its surface does.
///
/// **What left, and why none of it is a loss.** Note names, the Recorder
/// block, the take's sources, the time signature, the count-in, the Keyboard
/// block, Dark Mode, the typeface and the theory toggles are gone from here,
/// and every one of them is a KEY — U, V, D, F, 1-4, T, K, P, C — listed on
/// the help card that `keys.rs` draws. The camera pane has been keyboard-only
/// for releases and nobody has missed it. A row that duplicates a key charges
/// a hover forever and teaches nothing; the take's own settings, which a row
/// cannot show the VALUE of, are behind the cog in the band where they can be
/// read. What survives a key is what opens a dialog you then have to fill in,
/// because that row is the front door somebody finds the feature through.
///
/// **Detach is gone too, and that one is a pivot rather than a tidy-up.** Four
/// surfaces could be popped into their own window; all four are now in the box
/// or filling the screen (`Z`), and the actions that popped them out went with
/// their rows.
///
/// Two rules still shape what follows, and both are the type's, not a
/// preference: a submenu holds items and not more submenus (`Entry::Submenu`),
/// and a category with nothing in it must not draw as an empty hover
/// (`push_category`). Where those two collide with the tidy grouping, the
/// comment at the collision says which one won.
fn build_entries(view: MenuView) -> Vec<Entry> {
    let item = |label: &str, action: MenuAction| Entry::Item {
        label: label.to_owned(),
        action,
        enabled: true,
    };
    let row = |label: &str, action: MenuAction| SubItem {
        label: label.to_owned(),
        action,
        enabled: true,
    };

    // ── Everywhere ─────────────────────────────────────────────────────────
    // True wherever you right-click, because none of it is about a surface:
    // which keyboard is playing, how big the window is, what colour it is, and
    // what the detector does with what you play.
    let mut everywhere: Vec<Entry> = Vec::new();

    // TOP, and no longer buried in a "Keyboard" hover. It is the first thing a
    // new user needs and the one row in this menu that the app cannot do
    // anything useful without; a hover to reach it was a hover charged to
    // everybody once, on the day they could least afford it.
    //
    // Only where there are ports to choose from (`Caps::midi_ports`).
    if view.caps.midi_ports {
        everywhere.push(item("Select MIDI Input...", MenuAction::SelectMidiInput));
    }

    // Size was a submenu of its own, and it CANNOT become a child of Window:
    // `Entry::Submenu` holds items, not submenus, and `show` draws exactly one
    // sibling popup — there is no third level, and inventing one would rewrite
    // the placement, clamping and hover-tracking path all at once. So the
    // percents are Window's own items. Same seven choices, same actions, still
    // one hover from the top, and Borderless joins them because it is the only
    // other thing there is to say about the window.
    let mut window: Vec<SubItem> = Vec::new();
    window.extend(SIZE_PERCENTS.iter().map(|&p| SubItem {
        label: format!("{p}%"),
        action: MenuAction::SetSizePercent(p),
        enabled: true,
    }));
    // Borderless is a different question with a different answer: window
    // chrome belongs to whoever owns the window, so it is offered only where
    // that is us (`Caps::window_sizing`). Label shows what you would switch TO.
    if view.caps.window_sizing {
        window.push(row(
            if view.borderless {
                "Bordered"
            } else {
                "Borderless"
            },
            MenuAction::ToggleBorderless,
        ));
    }
    // **Where a person goes looking for it**, which is the same category as
    // every other question about how the window behaves. Only where the
    // window is ours to arrange (`Caps::window_sizing`), which is every build.
    if view.caps.window_sizing {
        window.push(row(
            if view.screen_reader {
                "Menus in Their Own Windows"
            } else {
                "Menus Inside the Window (Screen Reader)"
            },
            MenuAction::ToggleScreenReaderMode,
        ));
    }
    push_category(&mut everywhere, "Window", window);

    // Spelled the American way because every label inside it already is ("Set
    // White Key Color...") and a "Colours" hover full of "Color..." items reads
    // like a bug.
    // ── Themes ─────────────────────────────────────────────────────────────
    //
    // **Two levels only, so the Factory/User split is drawn with greyed
    // headers rather than nested hovers** (`Entry::Submenu` holds items, not
    // submenus). A disabled row is already this menu's way of saying "not
    // available", and it turns out to be exactly the way to say "heading" as
    // well — it cannot be clicked and it cannot be mistaken for a theme.
    //
    // The theme now worn is greyed for the same reason: picking what you
    // already have is a no-op, and this app has no checkmarks.
    {
        let mut themes: Vec<SubItem> = Vec::new();
        let head = |label: &str| SubItem {
            label: label.to_owned(),
            action: MenuAction::SaveThemeAs, // never reached; the row is dead
            enabled: false,
        };
        if !view.factory_themes.is_empty() {
            themes.push(head("- Factory -"));
            for name in &view.factory_themes {
                themes.push(SubItem {
                    label: name.clone(),
                    action: MenuAction::ApplyTheme(name.clone()),
                    enabled: *name != view.theme_name,
                });
            }
        }
        // The User heading appears only when there is something under it: an
        // empty heading is a promise the folder has not kept yet.
        if !view.user_themes.is_empty() {
            themes.push(head("- User -"));
            for name in &view.user_themes {
                themes.push(SubItem {
                    label: name.clone(),
                    action: MenuAction::ApplyTheme(name.clone()),
                    enabled: *name != view.theme_name,
                });
            }
        }
        themes.push(head(" "));
        themes.push(row("Save Theme As...", MenuAction::SaveThemeAs));
        push_category(&mut everywhere, "Themes", themes);
    }

    push_category(
        &mut everywhere,
        "Colors",
        vec![
            row(
                "Set White Key Color...",
                MenuAction::PickColor(ColorTarget::WhiteIdle),
            ),
            row(
                "Set Black Key Color...",
                MenuAction::PickColor(ColorTarget::BlackIdle),
            ),
            row(
                "Set Active Key Color...",
                MenuAction::PickColor(ColorTarget::Active),
            ),
            row(
                "Set Sustain Color...",
                MenuAction::PickColor(ColorTarget::Sustain),
            ),
            row(
                "Set Chord Color...",
                MenuAction::PickColor(ColorTarget::ChordText),
            ),
            row(
                "Set Recorder Color...",
                MenuAction::PickColor(ColorTarget::RecorderBg),
            ),
        ],
    );

    // **The detector, and everything that teaches it.** Global rather than the
    // piano's, because it reads whatever is played and every surface shows the
    // answer: the strip, the staff, the triangles and the neck all print the
    // same reading.
    //
    // Every row here is also a key — C, N, E, M, L — and they stay anyway. They
    // are the ones that open a DIALOG you then have to fill in, so the menu row
    // is where somebody who has not learned the letters finds out the feature
    // exists at all. That is the line the deletions above were drawn on: a row
    // that only flips a switch you can flip with one finger is a toll; a row
    // that is the front door to a feature is not.
    let mut chords = vec![row(
        if view.detection_enabled {
            "Disable Chord Detection"
        } else {
            "Enable Chord Detection"
        },
        MenuAction::ToggleChordDetection,
    )];
    // D-UI-5: "Teach Chord Name..." is greyed only when no notes are held;
    // "Manage Taught Chords..." is always available.
    chords.push(SubItem {
        label: "Teach Chord Name...".to_owned(),
        action: MenuAction::TeachChordName,
        enabled: view.notes_held,
    });
    chords.push(row(
        "Manage Taught Chords...",
        MenuAction::ManageTaughtChords,
    ));
    // D-UI-9: the learned re-ranker. Forgetting what was learned lives in
    // "Manage Taught Chords...", one step away from the button that trains.
    chords.push(SubItem {
        label: "Correct Chord Name...".to_owned(),
        action: MenuAction::CorrectChordName,
        // Needs both a voicing AND a visible reading: with detection off,
        // detection_tick() nulls current_chord, so the dialog would show
        // "Now reads: (none)" and the result would land somewhere invisible.
        enabled: view.notes_held && view.detection_enabled,
    });
    // The toggle renames itself like every other toggle here (Qt parity — no
    // checkmarks anywhere).
    chords.push(row(
        if view.learning_on {
            "Disable Chord Learning"
        } else {
            "Enable Chord Learning"
        },
        MenuAction::ToggleChordLearning,
    ));
    if !view.global_only {
        push_category(&mut everywhere, "Chords", chords);
    }

    // ── The piano's own ────────────────────────────────────────────────────
    // One row, and it is here rather than under Chords because it is a BAND:
    // it takes height from the window, it sits under the keys, and it is the
    // piano-and-strip window this app was for years.
    //
    // "(legacy)" is in the label and is not a warning — the sheet music prints
    // the chord name itself now, so the strip is off by default and this row is
    // how somebody who wants the old window gets it back. Nothing to say about
    // a strip with no detector behind it, so with detection off the row is not
    // there at all.
    let mut piano: Vec<Entry> = Vec::new();
    if view.detection_enabled {
        piano.push(item(
            if view.chord_strip {
                "Disable Chord Strip (legacy)"
            } else {
                "Enable Chord Strip (legacy)"
            },
            MenuAction::ToggleChordStrip,
        ));
    }

    // ── The theory band's own ──────────────────────────────────────────────
    // Clef, Key and Staves used to be filed under "Sheet music" as if they were
    // the notation's business alone. They are not: `Key` now sets what the
    // harmonic triangles are drawn AROUND as well as what the staff is spelled
    // in, which makes it the band's key rather than the staff's, and the three
    // read as one subject once they are in one block.
    // Any diagram at all counts, not just the notation: the key sets what the
    // triangles are drawn around, so a band showing only them still has a key
    // to choose.
    let theory_showing = view.staff_on || view.theory.any();
    let mut theory: Vec<Entry> = Vec::new();
    if view.staff_on {
        // Every preset, marked, plus whatever custom stack the user built —
        // which is listed and marked like the rest rather than hidden behind
        // the word "custom", because a set you cannot see is one you cannot
        // trust you are looking at.
        let mut clefs: Vec<SubItem> = Vec::new();
        for (key, label) in STAFF_PRESETS {
            clefs.push(SubItem {
                label: if *key == view.staff_set {
                    format!("{label}  \u{2022}")
                } else {
                    (*label).to_owned()
                },
                action: MenuAction::SetStaffSet(key),
                enabled: true,
            });
        }
        if let Some(custom) = view.staff_custom_label.clone() {
            clefs.push(SubItem {
                label: if view.staff_set.starts_with("custom:") {
                    format!("{custom}  \u{2022}")
                } else {
                    custom
                },
                action: MenuAction::SetStaffSet("__custom__"),
                enabled: true,
            });
        }
        push_category(&mut theory, "Clef", clefs);
    }
    // **Every key, on the thing it is printed on.** Fifteen rows is a long
    // hover and it is the right shape for this: they are one exclusive choice,
    // they have an order everybody already knows -- flats down, sharps up, C in
    // the middle -- and the marked one says where you are in it.
    //
    // Offered whenever ANY theory diagram is up, not just the staff. It sets
    // the tonic the key-centred diagrams orient around, so a band showing only
    // the triangles still has a key to choose — and the staff, when it is
    // there, is spelled in it.
    if theory_showing {
        push_category(
            &mut theory,
            "Key",
            (-crate::staff::MAX_KEY..=crate::staff::MAX_KEY)
                .map(|k| SubItem {
                    label: if k == view.staff_key {
                        format!("{}  \u{2022}", crate::staff::key_label(k))
                    } else {
                        crate::staff::key_label(k).to_owned()
                    },
                    action: MenuAction::SetStaffKey(k),
                    enabled: true,
                })
                .collect(),
        );
    }
    if view.staff_on {
        // **ONE stave at a time — the owner's ruling.** The multi-staff
        // stack (a clef each for a room of instruments) is a later feature;
        // until then this is an exclusive pick: the grand staff first and
        // default, then each single clef.
        let mark = |label: &str, key: &str| {
            if view.stave_key == key {
                format!("{label}  \u{2022}")
            } else {
                label.to_owned()
            }
        };
        let mut rows = vec![SubItem {
            label: mark("Grand Staff", "grand"),
            action: MenuAction::SetStave("grand"),
            enabled: true,
        }];
        rows.extend(Clef::ALL.into_iter().map(|c| SubItem {
            label: mark(c.label(), c.key()),
            action: MenuAction::SetStave(c.key()),
            enabled: true,
        }));
        push_category(&mut theory, "Stave", rows);
    }
    // The one theory row that is not a list of choices and has no key of its
    // own. It stays because without it there is no way at all to stop the band
    // chasing the piano — and a diagram that will not hold still is the whole
    // reason somebody goes looking for this.
    if theory_showing {
        theory.push(item(
            if view.theory_follows_midi {
                "Stop Following MIDI"
            } else {
                "Follow MIDI"
            },
            MenuAction::ToggleTheoryFollowsMidi,
        ));
    }

    // ── The guitar's own ───────────────────────────────────────────────────
    // All four appear only while the neck does. `G` is what brings it back —
    // the same arrangement the camera pane has had for releases, where the
    // toggle is a key and the settings are on the surface it opens.
    let mut guitar: Vec<Entry> = Vec::new();
    if view.fretboard_on {
        push_category(
            &mut guitar,
            "Fretboard",
            vec![
                row("Hide Fretboard", MenuAction::ToggleFretboard),
                row(
                    if view.fret_note_names {
                        "Hide Note Names on the Neck"
                    } else {
                        "Show Note Names on the Neck"
                    },
                    MenuAction::ToggleFretNoteNames,
                ),
                row(
                    if view.guitar_intervals {
                        "Hide Intervals"
                    } else {
                        "Show Intervals"
                    },
                    MenuAction::ToggleGuitarIntervals,
                ),
                row("Custom Tuning...", MenuAction::EditCustomTuning),
            ],
        );
        // Wood, Tuning and Capo are SIBLINGS of the Fretboard row rather than
        // rows inside it. They are lists of choices — three woods, every
        // shipped tuning, ten frets — so each is already a submenu, and a
        // submenu cannot hold another one (see `Entry::Submenu`). The
        // alternatives were both worse: flattening ~20 choices into the
        // Fretboard hover, or inventing a third menu level for three rows.
        push_category(
            &mut guitar,
            "Wood",
            fretboard_panel::Wood::ALL
                .iter()
                .map(|w| SubItem {
                    label: if w.key() == view.wood {
                        format!("{}  \u{2022}", w.label())
                    } else {
                        w.label().to_owned()
                    },
                    action: MenuAction::SetWood(w.key()),
                    enabled: true,
                })
                .collect(),
        );
        push_category(
            &mut guitar,
            "Tuning",
            fretboard::TUNINGS
                .iter()
                .map(|t| SubItem {
                    // The current one is marked rather than hidden: a submenu
                    // that never says what is selected makes you close it again
                    // to find out.
                    label: if t.name == view.tuning {
                        format!("{}  \u{2022}", t.name)
                    } else {
                        t.name.to_string()
                    },
                    action: MenuAction::SetTuning(t.name.to_string()),
                    enabled: true,
                })
                .collect(),
        );
        push_category(
            &mut guitar,
            "Capo",
            (0..=CAPO_MAX)
                .map(|f| {
                    let label = if f == 0 {
                        "No Capo".to_owned()
                    } else {
                        format!("Fret {f}")
                    };
                    SubItem {
                        label: if f == view.capo {
                            format!("{label}  \u{2022}")
                        } else {
                            label
                        },
                        action: MenuAction::SetCapo(f),
                        enabled: true,
                    }
                })
                .collect(),
        );
    }

    // ── The band's own ─────────────────────────────────────────────────────
    // **The only thing left of the Recorder block, and the only one that could
    // not go.** Show/hide is `V`; the take's name, folder, count-in, time
    // signature, sources, export and now the audio system are behind the cog,
    // where a setting can show its VALUE instead of being a row that only
    // hints at one.
    //
    // "My plugin is not in the list" is the single most common thing that goes
    // wrong with a plugin host, it has no key and it cannot have one — the
    // answer is a rescan, another folder, or starting over — so it stays a
    // menu, and it stays with the band whose slots it fills.
    let mut band: Vec<Entry> = Vec::new();
    // **The PROJECT, above the plugin folders.** The app grew documents:
    // self-contained directories a song lives in (docs/ARRANGEMENT.md), and
    // a document must be creatable, openable and duplicable from the one
    // menu the app has. "Duplicate As" rather than "Save As" because the
    // document autosaves on every edit — the only thing left for Save As to
    // mean is "copy everything, media included, and work there".
    if view.caps.capture_devices && view.recorder_on {
        push_category(
            &mut band,
            "Project",
            vec![
                row("New Project", MenuAction::NewProject),
                row("Open Project...", MenuAction::OpenProject),
                row("Duplicate Project As...", MenuAction::DuplicateProject),
                // **Media IN, with the project it brings media into.** It
                // wants to be reachable from a right-click on the
                // arrangement, where the lane bodies are spoken for by the
                // selection box and only a track's own header opens a menu,
                // so an empty desk had nowhere to ask from. No new top-level
                // row. (It once stood beside the export rows, which went to
                // the Export dialog in 4.52.0.)
                row("Insert Media...", MenuAction::InsertMedia),
            ],
        );
    }
    if view.caps.capture_devices && view.recorder_on {
        let mut plugins = vec![
            row("Rescan for Plugins", MenuAction::RescanPlugins),
            row("Add a Folder...", MenuAction::AddPluginFolder),
        ];
        if view.extra_plugin_folders > 0 {
            plugins.push(row(
                &format!(
                    "Forget {} Added Folder{}",
                    view.extra_plugin_folders,
                    if view.extra_plugin_folders == 1 { "" } else { "s" }
                ),
                MenuAction::ClearPluginFolders,
            ));
        }
        push_category(&mut band, "Plugin folders", plugins);
    }
    // ── The analyzer ───────────────────────────────────────────────────────
    // Only while it is on screen. Every row is a choice printed with its
    // current value; a press advances it, which is one row per parameter
    // rather than a submenu per parameter — `Entry::Submenu` is the last
    // level, and eleven parameters as eleven hovers would be a menu nobody
    // finishes reading.
    if let Some(cfg) = view.analyzer {
        let rows: Vec<SubItem> = crate::analyzer::Param::ALL
            .iter()
            .map(|p| SubItem {
                label: cfg.row_label(*p),
                action: MenuAction::Analyzer(*p),
                enabled: true,
            })
            .collect();
        push_category(&mut band, "Analyzer", rows);
    }

    // ── The rows that are not a category ───────────────────────────────────
    // Three one-shot actions and the supporter pair. None of them groups with
    // anything: burying "About" under a "Help" hover would be a hover invented
    // to hold one row.
    let mut tail: Vec<Entry> = Vec::new();
    tail.push(item(
        if view.supporter {
            "Supporter Key..."
        } else {
            "Support Tangent..."
        },
        MenuAction::ShowSupporterKey,
    ));
    if view.supporter {
        // Stays beside the supporter row rather than under Colors: it is a
        // visibility toggle for a decoration, not a colour, and it only exists
        // for the person the row above it is addressed to.
        tail.push(item(
            if view.heart_on {
                "Hide Heart"
            } else {
                "Show Heart"
            },
            MenuAction::ToggleHeart,
        ));
    }
    tail.push(Entry::Separator);
    // **The band's own door.** Its key (V) retired as unused clutter, and
    // the rule stands: a feature whose key leaves gets its row back. Gated
    // like every recorder row: a build with no capture devices has no band
    // to show.
    if view.caps.capture_devices {
        tail.push(item(
            if view.recorder_on {
                "Hide Recorder Band"
            } else {
                "Show Recorder Band"
            },
            MenuAction::ToggleRecorder,
        ));
    }
    // **The shortcut card's doorway.** Hold-H is taught exactly once, in a
    // Welcome card with a "don't show this again" box, and after that the
    // only way to the card was already knowing the key — one dismissed
    // dialog and the app's entire keyboard went invisible. This menu is the
    // whole chrome, so the teaching lives here, permanently. GREYED, which
    // is this menu's idiom for a heading (see the Themes hover): the card
    // is held open by design — press to read, let go and it is gone — so
    // there is nothing for a click to toggle, and the label itself is the
    // affordance. If the card ever grows a pinned mode, this row is where
    // its action belongs.
    tail.push(Entry::Item {
        label: "Keyboard Shortcuts (hold H)".to_owned(),
        action: MenuAction::ShowAbout, // never reached; the row is dead
        enabled: false,
    });
    tail.push(item("About", MenuAction::ShowAbout));
    // **A category, where there was one row.** "Reset Settings to Default"
    // destroyed all one hundred and sixty-four settings from directly under
    // About, with no confirmation; see `ResetScope` for why the answer is to
    // name the scopes rather than to put a speed bump in front of the wide
    // one. This is a submenu and not six top-level rows for the obvious
    // reason — and it leaves the top-level row budget one BETTER than it
    // found it.
    push_category(
        &mut tail,
        "Reset",
        crate::settings::ResetScope::ALL
            .iter()
            .map(|scope| SubItem {
                label: scope.row().to_owned(),
                action: MenuAction::Reset(*scope),
                enabled: true,
            })
            .collect(),
    );

    // **The compartments, joined.** A separator between blocks and never two in
    // a row, never one at the top and never one at the bottom — which is the
    // whole reason this is a join over a list of blocks rather than a `push`
    // wherever a separator looked right. Three of the four blocks can be empty
    // (no neck, no diagrams, no band), and every empty one used to leave its
    // separator behind: a menu whose gaps move around as bands open and close
    // reads as a rendering bug.
    let mut e: Vec<Entry> = Vec::new();
    // The GLOBAL cut leads with the Project — the screenshotted order the
    // owner asked for — and drops every surface-specific block whole.
    let blocks: Vec<Vec<Entry>> = if view.global_only {
        vec![band, everywhere, tail]
    } else {
        vec![everywhere, piano, theory, guitar, band, tail]
    };
    for block in blocks {
        if block.is_empty() {
            continue;
        }
        if !e.is_empty() {
            e.push(Entry::Separator);
        }
        e.extend(block);
    }

    if view.recorder_first && !view.global_only {
        move_recorder_to_the_front(&mut e);
    }
    // Checked AFTER the recorder, so a click that is somehow both lands on the
    // sheet music -- the more specific of the two, and the one with a control
    // you cannot reach any other way.
    if view.staff_first && view.staff_on {
        move_staff_to_the_front(&mut e);
    }
    e
}

/// Bring the Recorder's categories to the top, in their existing order.
///
/// A right-click ON the band opens its own settings first. The band is a
/// surface with fifteen controls on it, and reaching the sixteenth meant a
/// right-click anywhere at all followed by a hunt down a list of subjects that
/// are mostly about the piano.
///
/// It REORDERS and does not filter. A context menu showing only what is under
/// the pointer would be a second menu to learn, and the things everybody does
/// with a right click — dark mode, the fretboard — would start depending on
/// where they happened to click.
///
/// Matched by NAME rather than rebuilt, so there is one definition of what the
/// Recorder's rows are and this cannot drift from it.
/// The clef presets the Clef submenu offers, in the order `I` walks them.
///
/// A table rather than a loop over `Clef::ALL`, because the grand staff is not
/// a clef and belongs first — it is what a pianist wants and what the band
/// opens with.
pub const STAFF_PRESETS: &[(&str, &str)] = &[
    ("grand", "Grand staff"),
    ("treble", "Treble"),
    ("bass", "Bass"),
    ("alto", "Alto"),
    ("tenor", "Tenor"),
    ("treble8vb", "Treble 8vb (guitar, tenor)"),
    ("bass8vb", "Bass 8vb (double bass)"),
];

/// Bring the sheet music's own categories to the front, for a right-click that
/// landed on it. Same rule and same reasons as `move_recorder_to_the_front`:
/// reorder, never filter.
fn move_staff_to_the_front(e: &mut Vec<Entry>) {
    const OURS: [&str; 3] = ["Clef", "Key", "Stave"];
    let mut moved: Vec<Entry> = Vec::new();
    for name in OURS {
        if let Some(i) = e.iter().position(|x| match x {
            Entry::Submenu { label, .. } => label == name,
            _ => false,
        }) {
            moved.push(e.remove(i));
        }
    }
    moved.extend(e.drain(..));
    *e = moved;
}

fn move_recorder_to_the_front(e: &mut Vec<Entry>) {
    // **One name, where there were five.** Recorder, Sources, Time signature
    // and Count-in all left the menu — show/hide is `V` and the rest are behind
    // the cog — so the band's business up here is the one category that could
    // not become either: where its instruments are found.
    const OURS: [&str; 3] = ["Project", "Plugin folders", "Analyzer"];
    let mut moved: Vec<Entry> = Vec::new();
    // Kept in the order OURS lists, which is the order they already appear in —
    // so this is a move, not a re-sort, and adding a fifth category to the
    // block needs one edit here rather than a guess about position.
    for want in OURS {
        if let Some(i) = e.iter().position(|x| {
            matches!(x, Entry::Submenu { label, .. } if label == want)
        }) {
            moved.push(e.remove(i));
        }
    }
    if moved.is_empty() {
        return;
    }
    // A separator under the block, so the rest of the menu still reads as the
    // list of subjects it was.
    moved.push(Entry::Separator);
    moved.extend(e.drain(..));
    *e = moved;
}

impl MenuState {
    /// Snapshot labels and measure geometry at open time (Qt menus are static
    /// while shown). `global_pos` is monitor-space points.
    pub fn open(
        ctx: &egui::Context,
        view: MenuView,
        global_pos: Pos2,
        screen: Option<Rect>,
    ) -> Self {
        let entries = build_entries(view.clone());
        let font = FontId::new(MENU_FONT_SIZE, fonts::courier_bold());

        let measure = |ctx: &egui::Context, text: &str| -> Vec2 {
            ctx.fonts_mut(|f| {
                f.layout_no_wrap(text.to_owned(), font.clone(), Color32::WHITE)
                    .size()
            })
        };

        let mut text_h: f32 = 0.0;
        let mut max_w: f32 = 0.0;
        let arrow_w = measure(ctx, ARROW).x;
        for entry in &entries {
            match entry {
                Entry::Separator => {}
                Entry::Item { label, .. } => {
                    let sz = measure(ctx, label);
                    text_h = text_h.max(sz.y);
                    max_w = max_w.max(sz.x);
                }
                Entry::Submenu { label, .. } => {
                    let sz = measure(ctx, label);
                    text_h = text_h.max(sz.y);
                    // text + gap + arrow
                    max_w = max_w.max(sz.x + 12.0 + arrow_w);
                }
            }
        }
        let mut row_h = (text_h + 2.0 * PAD_Y).ceil();
        let width = (max_w + 2.0 * PAD_X).ceil();

        // Squeeze to fit, when there is a hard ceiling.
        //
        // A menu that scrolls is a menu that LOOKS cut off: the rows past the
        // edge are simply absent, and nothing on screen says to scroll. With
        // child windows it never comes up: the menu is its own window and may
        // be taller than the app. Drawn inside the window (screen-reader mode,
        // fullscreen), in a window that can easily be shorter than the ~550
        // points this menu wants, it came up immediately.
        //
        // Menus are static once shown, so the fix is to measure first and
        // shrink the rows until the whole thing fits. Down to a floor: below
        // about nine points it stops being readable, and at that point the
        // scroll fallback in `shell::surface` takes over rather than shrinking
        // into illegibility.
        let mut font_scale = 1.0_f32;
        if let Some(mon) = screen.map(|s| s.size()) {
            // **The tallest HOVER counts too, not just the menu.** A submenu
            // shares the menu's rows, and `clamp_submenu` can slide one up
            // the screen but cannot shrink it. Themes is the tall one: the
            // factory set went to thirty-two on 2026-10-08, which with its
            // headings is thirty-five rows (735 points at 21) before any
            // user theme, and on a short screen the bottom of the list
            // simply was not there.
            let tallest_sub = entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Submenu { items, .. } => Some(items.len()),
                    _ => None,
                })
                .max()
                .unwrap_or(0) as f32;
            if measured_height(&entries, row_h) > mon.y || tallest_sub * row_h > mon.y {
                // Solved, not approximated. Separators are a fixed 3 points
                // and do not shrink, so scaling the whole height by a ratio
                // leaves the menu a few points too tall — which is the one
                // outcome this is trying to avoid.
                let n_sep = entries
                    .iter()
                    .filter(|e| matches!(e, Entry::Separator))
                    .count() as f32;
                let n_row = entries.len() as f32 - n_sep;
                if n_row >= 1.0 {
                    let room = mon.y - n_sep * SEP_H;
                    let mut fitted = (room / n_row).floor();
                    if tallest_sub >= 1.0 {
                        fitted = fitted.min((mon.y / tallest_sub).floor());
                    }
                    let floor = (row_h * MIN_SQUEEZE).max(MIN_ROW_H);
                    let chosen = fitted.max(floor).min(row_h);
                    font_scale = (chosen / row_h).clamp(MIN_SQUEEZE, 1.0);
                    row_h = chosen;
                }
            }
        }

        let mut height = 0.0;
        let mut subs: Vec<SubGeom> = Vec::new();
        for entry in &entries {
            match entry {
                Entry::Separator => height += SEP_H,
                Entry::Item { .. } => height += row_h,
                Entry::Submenu { items, .. } => {
                    let w = items
                        .iter()
                        .fold(0.0_f32, |acc, it| acc.max(measure(ctx, &it.label).x));
                    subs.push(SubGeom {
                        row_top: height,
                        size: Vec2::new((w + 2.0 * PAD_X).ceil(), items.len() as f32 * row_h),
                    });
                    height += row_h;
                }
            }
        }

        let pos = place(global_pos, Vec2::new(width, height), screen);

        Self {
            pos,
            size: Vec2::new(width, height),
            entries,
            row_h,
            font_scale,
            subs,
            submenu_open: None,
            pending_sub: None,
            rest: None,
            first_pointer: None,
            armed: false,
            sub_rect: None,
            last_pointer: None,
            corridor_since: None,
            sub_first_pointer: None,
            sub_hover_armed: false,
            monitor: screen,
            dark_mode: view.dark_mode,
            face: view.face,
            opened_at: Instant::now(),
            saw_focus: false,
            caps: view.caps,
        }
    }
}

/// Where a menu of `size` goes when it is summoned at `at`.
///
/// **Nudged onto the screen, never flipped.**
///
/// Flipping about the pointer — hanging the menu's bottom edge from the
/// click when it will not fit below — is what a context menu does on most
/// platforms, and it was tried here. It is strictly WORSE: pinning moves the
/// menu by exactly its overflow, flipping moves it by its whole height, and
/// the overflow is smaller than the height by definition. On the owner's
/// screen a 478-point menu clicked forty points too low moves forty points
/// when pinned and 478 when flipped.
///
/// Its own function because it is what the tests assert against, and because
/// `open` needs a live `egui::Context` to measure text with — the arithmetic
/// needs neither.
pub fn place(at: Pos2, size: Vec2, screen: Option<Rect>) -> Pos2 {
    let mut pos = at;
    // See `screen_box`: a monitor SIZE with no origin is only a screen when
    // the point being placed is inside it. On a second display it is not,
    // and clamping against it moves the menu to another screen's top left.
    let Some(screen) = screen_box(at, screen) else {
        return pos;
    };
    if pos.x + size.x > screen.max.x {
        pos.x = (screen.max.x - size.x).max(screen.min.x);
    }
    if pos.y + size.y > screen.max.y {
        pos.y = (screen.max.y - size.y).max(screen.min.y);
    }
    pos
}

/// The clickable rows, in draw order, for a test that wants to click one.
///
/// Separators and submenu parents are excluded: a separator cannot be clicked
/// and a submenu parent opens rather than acts.
/// The menu's natural size, for a test that needs to know whether it is taller
/// than the editor it is being drawn into.
#[doc(hidden)]
pub fn size_for_test(state: &MenuState) -> Vec2 {
    state.size
}

#[doc(hidden)]
pub fn row_height_for_test(state: &MenuState) -> f32 {
    state.row_h
}

#[doc(hidden)]
pub fn rows_for_test(view: MenuView) -> Vec<(String, MenuAction)> {
    build_entries(view)
        .into_iter()
        .filter_map(|e| match e {
            Entry::Item { label, action, .. } => Some((label, action)),
            _ => None,
        })
        .collect()
}

/// The centre of clickable row `idx`, in the same coordinates `show` draws in.
///
/// Walks the entry list exactly as the row loop does — separators are `SEP_H`
/// tall, everything else is `row_h` — so a test clicks where the row actually
/// is rather than where it is assumed to be.
#[doc(hidden)]
pub fn row_center_for_test(state: &MenuState, idx: usize) -> Pos2 {
    let mut y = state.pos.y;
    let mut seen = 0usize;
    for entry in &state.entries {
        match entry {
            Entry::Separator => y += SEP_H,
            Entry::Item { .. } => {
                if seen == idx {
                    return Pos2::new(state.pos.x + state.size.x * 0.5, y + state.row_h * 0.5);
                }
                seen += 1;
                y += state.row_h;
            }
            Entry::Submenu { .. } => y += state.row_h,
        }
    }
    Pos2::new(state.pos.x + state.size.x * 0.5, y)
}

/// The menu's own background, and the edge it needs to look like an object.
///
/// On the desktop this is deliberately borderless: the menu is its own OS
/// window, so the window edge and its drop shadow already separate it from
/// whatever is behind, and a drawn border on top of that looks heavy.
///
/// Drawn INLINE there is no window and no shadow — and the menu's background
/// is the same cream as the app's. The result was a menu with no visible
/// extent at all: the rows read as text lying across the theory band and the
/// separators as stray lines through it, which is what "the menu is cut off"
/// looks like when nothing is actually missing. So inline it gets a real
/// border and a shadow, and becomes a thing sitting on top of the app.
fn draw_menu_backdrop(painter: &egui::Painter, rect: Rect, c: MenuColors, caps: Caps) {
    if !caps.child_windows {
        // A soft shadow, down and to the right, built from a few translucent
        // rects. Cheaper than a blur and enough to lift the menu off the page.
        for i in (1..=SHADOW_STEPS).rev() {
            let k = i as f32;
            painter.rect_filled(
                rect.translate(Vec2::splat(k * 0.9)).expand(k * 0.4),
                2.0,
                Color32::from_black_alpha((14.0 / k) as u8),
            );
        }
    }
    painter.rect_filled(rect, 0.0, c.bg);
}

/// **The edge, drawn LAST — over the rows and not under them.**
///
/// A menu row is justified to the full width of the menu and its padding is
/// zero, because that is what makes a hovered row a full-width bar the way
/// every native menu draws one. The consequence is that the highlight lands on
/// the same pixels the border does — so the border simply disappeared under
/// whichever row the pointer was on, and the menu lost its outline exactly
/// where the eye was looking. The owner: "right click dialogue options, when
/// hovered, are filled correctly, but they overlap the outline of the dialog."
///
/// Painting it after the content costs nothing — the same painter, one shape
/// later in the same layer — and needs no change to the row geometry, which is
/// what the menu's measured size was computed from. On the desktop the edge is
/// the background colour (the OS window edge and its shadow do the separating
/// there), so what it draws is a one-point inset around the highlight: the
/// gap between a hovered row and the frame that a native menu has.
fn draw_menu_edge(painter: &egui::Painter, rect: Rect, c: MenuColors, caps: Caps) {
    painter.rect_stroke(
        rect,
        0.0,
        // Borderless on the desktop (the window edge does the work), a real
        // 1px edge inline.
        Stroke::new(1.0_f32, if caps.child_windows { c.bg } else { c.sep }),
        egui::StrokeKind::Inside,
    );
}

/// Shrink the row text along with the rows, when the menu had to be squeezed.
///
/// Without this the rows get shorter and the labels do not, so they overflow
/// their own row and the menu looks broken rather than compact. Padding comes
/// down with it, or the text has no room left inside the row at all.
fn scale_menu_font(style: &mut egui::Style, scale: f32) {
    if scale >= 0.999 {
        return;
    }
    let size = (MENU_FONT_SIZE * scale).max(8.0);
    style.text_styles.insert(
        egui::TextStyle::Button,
        FontId::new(size, fonts::courier_bold()),
    );
    style.spacing.button_padding = egui::vec2(PAD_X * scale, (PAD_Y * scale).max(1.0));
}

fn apply_menu_style(style: &mut egui::Style, c: MenuColors) {
    style.spacing.button_padding = egui::vec2(PAD_X, PAD_Y);
    style.spacing.item_spacing = egui::vec2(0.0, 0.0);
    style.spacing.menu_margin = Margin::ZERO;
    style.spacing.window_margin = Margin::ZERO;
    style.spacing.interact_size = egui::vec2(0.0, 0.0);

    let set = |wv: &mut egui::style::WidgetVisuals, bg: Color32, fg: Color32| {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::NONE;
        wv.fg_stroke = Stroke::new(1.0_f32, fg);
        wv.corner_radius = CornerRadius::ZERO;
        wv.expansion = 0.0;
    };
    set(
        &mut style.visuals.widgets.inactive,
        Color32::TRANSPARENT,
        c.text,
    );
    set(&mut style.visuals.widgets.hovered, c.sel, c.text);
    set(&mut style.visuals.widgets.active, c.sel, c.text);
    set(&mut style.visuals.widgets.open, c.sel, c.text);
    set(
        &mut style.visuals.widgets.noninteractive,
        Color32::TRANSPARENT,
        c.text.gamma_multiply(0.4),
    );
    // Separators draw with noninteractive.bg_stroke.
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, c.sep);
    style.visuals.selection.bg_fill = c.sel;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, c.text);
    style.visuals.window_fill = c.bg;
    style.visuals.window_stroke = Stroke::new(1.0_f32, c.bg);
    style.visuals.popup_shadow = egui::Shadow::NONE;
    style.visuals.window_shadow = egui::Shadow::NONE;
    style.visuals.menu_corner_radius = CornerRadius::ZERO;
    style.visuals.override_text_color = None;
}

fn menu_button(ui: &mut egui::Ui, label: &str, enabled: bool, row_h: f32) -> egui::Response {
    ui.add_enabled(
        enabled,
        Button::new(label)
            .wrap_mode(egui::TextWrapMode::Extend)
            .min_size(egui::vec2(0.0, row_h)),
    )
}

/// Render the open menu (and submenu). Returns a chosen action, if any;
/// `state_opt` is set to None when the menu closes for any reason.
pub fn show(ctx: &egui::Context, state_opt: &mut Option<MenuState>) -> Option<MenuAction> {
    let state = state_opt.as_mut()?;
    let c = colors(
        state.dark_mode,
        Color32::from_rgb(state.face.0, state.face.1, state.face.2),
    );
    let caps = state.caps;
    let row_h = state.row_h;
    let submenu_open = state.submenu_open;
    let mut action: Option<MenuAction> = None;
    let mut close = false;

    let mut hover_close_submenu = false;
    let mut hover_open_submenu: Option<usize> = None;
    let mut clicked_submenu = false;
    // Where each submenu's parent row actually landed, in whatever coordinates
    // it was drawn in. Inline the menu may be inside a `ScrollArea`, so the
    // measured-at-open `row_top` is not where the row IS.
    let mut sub_row_rects: Vec<Rect> = Vec::new();

    // ── Main menu ──────────────────────────────────────────────────────────
    let menu_spec = crate::shell::SurfaceSpec {
        id: MENU_ID,
        size: state.size,
        min_size: state.size,
        pos: Some(state.pos),
        order: egui::Order::Foreground,
        // Tells a Linux WM this is a menu, not an app window to tile.
        window_type: Some(egui::X11WindowType::PopupMenu),
        ..Default::default()
    };
    let font_scale = state.font_scale;
    // Read INSIDE the menu's own surface. On the desktop the menu is a separate
    // viewport with its own input, so the parent context's pointer is not over
    // it and would report whatever the main window last saw.
    let mut pointer: Option<Pos2> = None;
    let menu_report = crate::shell::surface(ctx, caps, &menu_spec, &mut |ui, want_close| {
        // `latest_pos`, and NOT `velocity`, is the whole of what this frame
        // needs to know about the pointer. See `note_rest`.
        pointer = ui.ctx().input(|i| i.pointer.latest_pos());
        apply_menu_style(ui.style_mut(), c);
        scale_menu_font(ui.style_mut(), font_scale);
        let rect = ui.max_rect();
        draw_menu_backdrop(ui.painter(), rect, c, caps);

        ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
            let mut sub_idx = 0usize;
            for entry in &state.entries {
                match entry {
                    Entry::Separator => {
                        ui.add(egui::Separator::default().spacing(SEP_H));
                    }
                    Entry::Item {
                        label,
                        action: a,
                        enabled,
                    } => {
                        let r = menu_button(ui, label, *enabled, row_h);
                        if r.hovered() {
                            hover_close_submenu = true;
                        }
                        if r.clicked() {
                            action = Some(a.clone());
                            *want_close = true;
                        }
                    }
                    Entry::Submenu { label, .. } => {
                        let r = ui.add(
                            Button::new(label.as_str())
                                .right_text(ARROW)
                                .selected(submenu_open == Some(sub_idx))
                                .wrap_mode(egui::TextWrapMode::Extend)
                                .min_size(egui::vec2(0.0, row_h)),
                        );
                        if r.hovered() || r.clicked() {
                            hover_open_submenu = Some(sub_idx);
                        }
                        // Reported apart from the hover: a press is a decision
                        // and skips the dwell, a hover is a guess and does not.
                        if r.clicked() {
                            clicked_submenu = true;
                        }
                        sub_row_rects.push(r.rect);
                        sub_idx += 1;
                    }
                }
            }
        });
        draw_menu_edge(ui.painter(), rect, c, caps);
    });
    close |= menu_report.close;

    // Armed once the pointer has moved away from where the menu appeared under
    // it. Sticky, and measured from the FIRST position seen inside the menu
    // rather than from the click, so a menu clamped against a screen edge is
    // judged by where the pointer actually is.
    if let Some(p) = pointer {
        match state.first_pointer {
            None => state.first_pointer = Some(p),
            Some(first) => {
                if (p - first).length() > ARM_SLOP {
                    state.armed = true;
                }
            }
        }
    }
    let now = Instant::now();
    let still = note_rest(&mut state.rest, pointer, now);
    // **On its way to the open panel, or choosing?** See `heading_for`: a
    // panel pulled up the screen by the clamp is reached by travelling
    // ACROSS the rows between, and without this every one of them is a
    // category the hand appears to have paused on.
    let travelling = match (pointer, state.last_pointer, state.sub_rect) {
        (Some(now_at), Some(was), Some(panel)) => heading_for(was, now_at, panel),
        _ => false,
    };
    // Capped, so a hand that STOPS inside the corridor can still choose.
    let travelling = if travelling {
        let since = *state.corridor_since.get_or_insert(now);
        now.duration_since(since) < CORRIDOR_FOR
    } else {
        state.corridor_since = None;
        false
    };
    if pointer.is_some() {
        state.last_pointer = pointer;
    }
    if settle_submenu(
        &mut state.submenu_open,
        &mut state.pending_sub,
        hover_open_submenu,
        hover_close_submenu,
        state.armed,
        still,
        clicked_submenu,
        travelling,
        now,
    ) {
        // Still waiting out the dwell. Ask for the frame that will end it, or a
        // pointer that has stopped moving would sit there forever.
        //
        // **For the time LEFT, not the whole dwell again** - see `due_in`.
        let wait = state
            .pending_sub
            .map_or(SUB_SWITCH_DWELL, |(_, at)| due_in(SUB_SWITCH_DWELL, at, now));
        ctx.request_repaint_after(wait);
    }
    // And the frame that will notice it has come to rest. A pointer let go of
    // produces no events at all, so without this the menu would only settle if
    // something else happened to be animating.
    if pointer.is_some() && !still {
        let wait = state
            .rest
            .map_or(REST_FOR, |(_, since)| due_in(REST_FOR, since, now));
        ctx.request_repaint_after(wait);
    }
    // A different panel, or the same one again, has to earn its hover afresh -
    // see `sub_hover_armed`.
    if state.submenu_open != submenu_open {
        state.sub_first_pointer = None;
        state.sub_hover_armed = false;
    }

    // ── Submenu (sibling, Qt-style to the right) ──────────────────────────
    // Only one submenu can be open at a time, so they all share one surface id
    // and it simply moves and resizes as the pointer travels down the menu.
    let open_sub = state
        .submenu_open
        .filter(|_| !close)
        .and_then(|i| state.subs.get(i).map(|g| (i, g.row_top, g.size)));
    let mut submenu_report = None;
    if let Some((sub_i, row_top, sub_size)) = open_sub {
        // Beside the row, and the two hosts measure that differently.
        //
        // On the desktop the menu is its own window: rows are drawn at
        // window-local coordinates and `row_top` — measured at open time — is
        // exact, so the submenu goes at the menu window's origin plus it.
        //
        // Inline there is no window, the row rect is already in canvas
        // coordinates, and the menu may be scrolled. Using `row_top` there put
        // the submenu beside a DIFFERENT row: reaching for it crossed other
        // rows, which re-points `submenu_open`, and the click then landed in
        // whichever submenu had taken its place — silently setting a capo
        // nobody asked for. The row's own rect is the only thing that knows
        // where the row ended up.
        // **Beside the row where it actually IS**, on both hosts.
        //
        // The desktop used to place it from `row_top`, measured when the menu
        // opened. That is only right while the measurement and the layout agree
        // about row heights, separators and font scale — and when they drift,
        // the submenu appears beside a different row, so reaching for it drags
        // the pointer across the rows in between. The inline host already had
        // this fix and the comment explaining it; the desktop had the bug.
        //
        // `x` still comes from the menu's own width rather than the row rect:
        // a justified button stops short of the frame's padding, and hanging
        // the submenu off THAT leaves it overlapping the menu by a few points.
        let mut sub_pos = match sub_row_rects.get(sub_i) {
            Some(r) if !caps.child_windows => Pos2::new(r.max.x, r.min.y),
            Some(r) => Pos2::new(state.pos.x + state.size.x, state.pos.y + r.min.y),
            None => Pos2::new(state.pos.x + state.size.x, state.pos.y + row_top),
        };
        // **Only against a screen we know the menu is on.** See `screen_box`:
        // the anchor here is the menu's own left edge, in the same global
        // coordinates the monitor box would be.
        if let Some(screen) = screen_box(state.pos, state.monitor) {
            sub_pos = clamp_submenu(sub_pos, sub_size, state.pos.x, screen);
        }
        // **Kept in the coordinates the parent reads its pointer in**, which
        // is the whole of the difference between the two hosts: inline the
        // menu is a layer and both are canvas points; on the desktop the
        // menu is a window, its pointer is window-local, and `sub_pos` is
        // where the panel sits on the SCREEN.
        state.sub_rect = Some(Rect::from_min_size(
            if caps.child_windows { sub_pos - state.pos.to_vec2() } else { sub_pos },
            sub_size,
        ));

        let sub_spec = crate::shell::SurfaceSpec {
            id: SUBMENU_ID,
            size: sub_size,
            min_size: sub_size,
            pos: Some(sub_pos),
            // Don't steal key focus from the menu — and inline, sit above it.
            takes_focus: false,
            order: egui::Order::Tooltip,
            window_type: Some(egui::X11WindowType::PopupMenu),
            ..Default::default()
        };
        let report = crate::shell::surface(ctx, caps, &sub_spec, &mut |ui, want_close| {
            apply_menu_style(ui.style_mut(), c);
            scale_menu_font(ui.style_mut(), font_scale);
            // **Hover only once the hand has moved inside the panel** - see
            // `sub_hover_armed`. Read from THIS surface's own input, which
            // on the desktop is the panel window's and not the parent's.
            if let Some(p) = ui.ctx().input(|i| i.pointer.latest_pos()) {
                match state.sub_first_pointer {
                    None => state.sub_first_pointer = Some(p),
                    Some(first) => {
                        if (p - first).length() > ARM_SLOP {
                            state.sub_hover_armed = true;
                        }
                    }
                }
            }
            if !state.sub_hover_armed {
                let inactive = ui.style().visuals.widgets.inactive;
                ui.style_mut().visuals.widgets.hovered = inactive;
            }
            let rect = ui.max_rect();
            draw_menu_backdrop(ui.painter(), rect, c, caps);
            let items = state
                .entries
                .iter()
                .filter_map(|e| match e {
                    Entry::Submenu { items, .. } => Some(items),
                    _ => None,
                })
                .nth(sub_i);
            ui.with_layout(egui::Layout::top_down_justified(egui::Align::Min), |ui| {
                for it in items.into_iter().flatten() {
                    // `it.enabled`, not `true`: "Teach Chord Name..." lives in
                    // a submenu now, and hard-coding this is how a dialog opens
                    // on a voicing that is not being held.
                    if menu_button(ui, &it.label, it.enabled, row_h).clicked() {
                        action = Some(it.action.clone());
                        *want_close = true;
                    }
                }
            });
            draw_menu_edge(ui.painter(), rect, c, caps);
        });
        close |= report.close;
        submenu_report = Some(report);
    }

    // ── Closing when the user goes elsewhere ───────────────────────────────
    //
    // Two different signals for the same intent, because the two hosts offer
    // different evidence. A window knows it lost focus; a layer inside the
    // main window has no focus of its own to lose and has to watch the
    // pointer instead.
    if caps.child_windows {
        if menu_report.focused == Some(true)
            || submenu_report.is_some_and(|r| r.focused == Some(true))
        {
            state.saw_focus = true;
        }
        let grace = state.opened_at.elapsed() > std::time::Duration::from_millis(250);
        let all_unfocused = menu_report.focused == Some(false)
            && submenu_report.is_none_or(|r| r.focused != Some(true));
        // Focus loss alone is NOT enough evidence while the pointer is inside
        // one of the two windows. Under i3 a freshly mapped submenu window
        // takes the OS focus and the menu never gets it back — so the frame
        // that closed the submenu (any hover of a plain row) used to read as
        // "nobody is focused" and shut the whole menu while the user was in
        // the middle of using it. That was the "menu vanishes when moving to
        // the lower items" bug on Linux.
        let pointer_inside = menu_report.pointer_over == Some(true)
            || submenu_report.is_some_and(|r| r.pointer_over == Some(true));
        if state.saw_focus && grace && all_unfocused && !pointer_inside {
            close = true;
        }
    } else if menu_report.pressed_outside && submenu_report.is_none_or(|r| r.pressed_outside) {
        // The press that OPENED the menu is always inside it — the menu is
        // positioned at the cursor — so this needs no opening grace.
        close = true;
    }

    if close {
        *state_opt = None;
    }
    action
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// `settle_submenu` without the click, which is all but one of these.
    fn settle(
        open: &mut Option<usize>,
        pending: &mut Option<(Option<usize>, Instant)>,
        hovered: Option<usize>,
        plain: bool,
        armed: bool,
        still: bool,
        now: Instant,
    ) -> bool {
        settle_submenu(open, pending, hovered, plain, armed, still, false, false, now)
    }

    /// **The box is wide enough for the words, in every bundled face.**
    ///
    /// The owner reported the Project menu not fitting its font, and only on
    /// one of the three. The width is measured at open time from
    /// `MENU_FONT_SIZE` in the bold family, and the rows are drawn by
    /// `menu_button` with `TextWrapMode::Extend` — which does not wrap and
    /// does not shrink, it simply runs past the edge. So a measurement that
    /// disagrees with the drawing by any amount is a word off the end of the
    /// menu, and nothing in egui will say so.
    ///
    /// Asserted over `FontChoice::ALL` and over every cut of the menu, so a
    /// fourth face or a new row is covered the day it lands.
    #[test]
    fn every_menu_is_wide_enough_for_its_own_words_in_every_face() {
        for face in fonts::FontChoice::ALL {
            let ctx = egui::Context::default();
            fonts::install(&ctx, face, None);
            fonts::apply_text_styles(&ctx);
            let _ = ctx.run(Default::default(), |_| {});
            let mut v = view();
            v.recorder_on = true;
            v.fretboard_on = true;
            v.staff_on = true;
            v.chord_strip = true;
            v.camera_pane_on = true;
            let m = MenuState::open(&ctx, v, Pos2::new(100.0, 100.0), None);
            // What the rows will actually be drawn at: the Button text style,
            // which is what `menu_button` uses, NOT the constant the geometry
            // measured with.
            let drawn = ctx.style().text_styles[&egui::TextStyle::Button].clone();
            for e in &m.entries {
                let (label, extra) = match e {
                    Entry::Separator => continue,
                    Entry::Item { label, .. } => (label.clone(), 0.0),
                    Entry::Submenu { label, .. } => {
                        let arrow = ctx.fonts_mut(|f| {
                            f.layout_no_wrap(ARROW.to_owned(), drawn.clone(), Color32::WHITE)
                                .size()
                                .x
                        });
                        (label.clone(), arrow)
                    }
                };
                let w = ctx.fonts_mut(|f| {
                    f.layout_no_wrap(label.clone(), drawn.clone(), Color32::WHITE)
                        .size()
                        .x
                });
                let needs = w + extra + 2.0 * PAD_X;
                assert!(
                    m.size.x + 0.5 >= needs,
                    "{face:?}: the menu is {:.1} wide and {label:?} needs {needs:.1}",
                    m.size.x
                );
            }
            // **And every SUBMENU, which is where the Project rows live.**
            // Their width is computed by the same formula off their own
            // items, so the same disagreement is possible one level down and
            // is harder to notice: a submenu is only on screen while the
            // pointer is on its parent row.
            let mut sub_i = 0usize;
            for e in &m.entries {
                let Entry::Submenu { label: parent, items } = e else {
                    continue;
                };
                let box_w = m.subs[sub_i].size.x;
                sub_i += 1;
                for it in items {
                    let w = ctx.fonts_mut(|f| {
                        f.layout_no_wrap(it.label.clone(), drawn.clone(), Color32::WHITE)
                            .size()
                            .x
                    });
                    let needs = w + 2.0 * PAD_X;
                    assert!(
                        box_w + 0.5 >= needs,
                        "{face:?}: the {parent:?} submenu is {box_w:.1} wide and \
                         {:?} needs {needs:.1}",
                        it.label
                    );
                }
            }
        }
    }

    /// The REAL menu, at the size it really measures out at, on the owner's
    /// own screen — the case the arithmetic above is abstract about.
    #[test]
    fn the_app_menu_opens_under_the_hand_on_a_real_screen() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let mon = Vec2::new(2560.0, 1440.0);
        let mut v = view();
        // The band, the fretboard and the notation all ON — the shape the
        // owner's own window is in, and the tallest the menu gets.
        v.recorder_on = true;
        v.fretboard_on = true;
        v.staff_on = true;
        v.chord_strip = true;
        v.camera_pane_on = true;
        let m = MenuState::open(&ctx, v, Pos2::new(900.0, 1000.0), Some(Rect::from_min_size(Pos2::ZERO, mon)));
        // Whatever it measures out at, it may not sit further from the hand
        // than the amount by which it overflows the screen.
        let overflow = (1000.0 + m.size.y - mon.y).max(0.0);
        assert!(
            1000.0 - m.pos.y <= overflow + 0.5,
            "a {}-point menu summoned at y=1000 opened at y={} on a {}-point \
             screen: {} points away, with {overflow} of overflow",
            m.size.y,
            m.pos.y,
            mon.y,
            1000.0 - m.pos.y
        );
    }

    /// **A menu stays as near the hand as the screen allows.**
    ///
    /// It moves by its OVERFLOW and no further. Flipping about the pointer
    /// was tried and is strictly worse: it moves the menu by its whole
    /// height where pinning moves it by the part that does not fit.
    #[test]
    fn a_menu_moves_no_further_than_it_has_to() {
        let mon = Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 1440.0)));
        let size = Vec2::new(300.0, 560.0);
        // Room below: it opens exactly where it was asked to.
        let at = Pos2::new(400.0, 200.0);
        assert_eq!(place(at, size, mon), at);
        // No room: on screen, and moved by exactly the overflow.
        let low = Pos2::new(400.0, 1200.0);
        let p = place(low, size, mon);
        assert!((p.y + size.y - 1440.0).abs() < 0.5, "{p:?} is not on the screen");
        assert!((low.y - p.y - 320.0).abs() < 0.5, "moved further than it had to");
        // The other axis, the same rule.
        let right = Pos2::new(2500.0, 200.0);
        assert!((place(right, size, mon).x + size.x - 2560.0).abs() < 0.5);
        // A menu taller than the screen is pinned to the top rather than
        // pushed off it.
        let huge = Vec2::new(300.0, 1600.0);
        assert!(place(Pos2::new(400.0, 900.0), huge, mon).y >= 0.0);
        // No monitor known: nothing is moved. A guess is worse than the
        // place the user pointed at.
        assert_eq!(place(low, size, None), low);
    }


    /// **A hovered row does not paint over the menu's own edge.**
    ///
    /// The rows are justified to the menu's full width with zero padding —
    /// that is what makes a highlight a full-width bar the way a native menu
    /// draws one — so the hover fill lands on exactly the pixels the border
    /// occupies. Drawn in the old order the border vanished under whichever
    /// row the pointer was on, which is the one row the eye is on. The owner:
    /// "right click dialogue options, when hovered, are filled correctly, but
    /// they overlap the outline of the dialog."
    ///
    /// Asserted on the ORDER the shapes were emitted in, because that is the
    /// whole of the fix and the only thing that can regress: any shape landing
    /// on the border band after the edge is a shape drawn over it.
    #[test]
    fn a_hovered_row_does_not_paint_over_the_menus_edge() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0));
        let _ = ctx.run(
            egui::RawInput { screen_rect: Some(screen), ..Default::default() },
            |_| {},
        );
        // The INLINE menu, because that is the one with a border of its own to
        // lose. The desktop's is the same code drawing the same rectangle in
        // the background colour, so the order it is drawn in is the same
        // question with a quieter answer.
        let mut v = view();
        v.caps = Caps::NONE;
        let mut state = Some(MenuState::open(&ctx, v, Pos2::new(30.0, 30.0), Some(screen)));
        let menu = {
            let st = state.as_ref().expect("the menu did not open");
            Rect::from_min_size(st.pos, st.size)
        };
        // A row well inside the list, and two frames: the first lays the rows
        // out, the second is the one the pointer is over.
        let over = Pos2::new(menu.center().x, menu.top() + 40.0);
        let input = || egui::RawInput {
            screen_rect: Some(screen),
            focused: true,
            events: vec![egui::Event::PointerMoved(over)],
            ..Default::default()
        };
        for _ in 0..3 {
            let _ = ctx.run(input(), |ctx| {
                let _ = show(ctx, &mut state);
            });
        }
        assert_eq!(
            ctx.input(|i| i.pointer.hover_pos()),
            Some(over),
            "the pointer never reached the menu, so nothing was hovered"
        );
        let out = ctx.run(input(), |ctx| {
            let _ = show(ctx, &mut state);
        });
        // The band the border lives in: one point inside the menu's outline.
        let inner = menu.shrink(1.0);
        let on_the_border = |r: Rect| r.intersects(menu) && !inner.contains_rect(r.shrink(0.01));
        let mut edge: Option<usize> = None;
        let mut over_it: Vec<usize> = Vec::new();
        for (i, s) in out.shapes.iter().enumerate() {
            let egui::Shape::Rect(rs) = &s.shape else { continue };
            let is_edge = rs.stroke.width > 0.0 && rs.rect.expand(0.6).contains_rect(menu);
            if is_edge {
                edge = Some(i);
            } else if rs.fill.a() > 0 && on_the_border(rs.rect) {
                over_it.push(i);
            }
        }
        let edge = edge.expect("the menu drew no edge at all");
        // And the row really was lit: a test that hovers nothing would pass
        // this whatever order the edge is drawn in, which is how the first
        // draft of it passed against the old code.
        assert!(
            !over_it.is_empty(),
            "no row was filled at all, so nothing could have covered the edge"
        );
        let after: Vec<usize> = over_it.into_iter().filter(|i| *i > edge).collect();
        assert!(
            after.is_empty(),
            "{} filled rect(s) were painted over the menu's edge at {edge}: {after:?}",
            after.len()
        );
    }

    /// **A wait is asked for by what is left of it, not restarted every
    /// frame.** See `due_in`: egui takes its predicted frame time off every
    /// ask, so an ask for the whole `REST_FOR` on the frame that fell 16 ms
    /// short of it doubled the rest, and the dwell with it. Measured here on
    /// the rest, because a moving pointer's rest ask is the shorter of the
    /// two and so the one the frame is scheduled by.
    #[test]
    fn a_rest_is_asked_for_by_what_is_left_of_it() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 800.0));
        let _ = ctx.run(
            egui::RawInput { screen_rect: Some(screen), ..Default::default() },
            |_| {},
        );
        let mut v = view();
        v.caps = Caps::NONE;
        let mut state = Some(MenuState::open(&ctx, v, Pos2::new(30.0, 30.0), Some(screen)));
        let (menu, row_h, row_top) = {
            let st = state.as_ref().expect("the menu did not open");
            (Rect::from_min_size(st.pos, st.size), st.row_h, st.subs[0].row_top)
        };
        let frame = |state: &mut Option<MenuState>, at: Pos2| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    focused: true,
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                },
                |ctx| {
                    let _ = show(ctx, state);
                },
            )
        };
        // Arrive on a submenu row from a point of the menu a row's height
        // away, so the menu is armed and a dwell is pending as well.
        let on_row = Pos2::new(menu.left() + 20.0, menu.top() + row_top + row_h * 0.5);
        let _ = frame(&mut state, Pos2::new(on_row.x, on_row.y + 2.0 * row_h));
        let _ = frame(&mut state, on_row);
        let since = state
            .as_ref()
            .and_then(|st| st.rest)
            .map(|(_, since)| since)
            .expect("the pointer on a row was not noted as resting");
        std::thread::sleep(Duration::from_millis(30));
        // A stalled test runner could sleep through the whole rest, and then
        // there is no ask left to measure; say nothing rather than lie.
        if since.elapsed() >= REST_FOR - Duration::from_millis(10) {
            return;
        }
        let out = frame(&mut state, on_row);
        let asked = out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .map_or(Duration::MAX, |o| o.repaint_delay);
        // What is left of the 60 ms rest, less egui's 1/60 s: about 13 ms.
        // Restarting the rest asked for 60 less 1/60 s, which is 43.
        assert!(
            asked < Duration::from_millis(35),
            "the frame after 30 ms of a 60 ms rest was asked for in {asked:?}"
        );
    }

    /// `due_in` on its own: the time left, and never less than nothing.
    #[test]
    fn due_in_counts_down_and_stops_at_zero() {
        let ms = Duration::from_millis;
        let t0 = Instant::now();
        assert_eq!(due_in(ms(140), t0, t0 + ms(40)), ms(100));
        assert_eq!(due_in(ms(140), t0, t0 + ms(140)), ms(0));
        assert_eq!(due_in(ms(140), t0, t0 + ms(500)), ms(0));
    }

    /// **A submenu panel lights no row until the hand has moved inside it,
    /// and starts over for every panel it becomes.** See `sub_hover_armed`.
    ///
    /// Drawn inline, where the panel shares the pointer with the menu and a
    /// stale position cannot happen, so the stale state is put there by hand:
    /// a panel that believes the pointer has not moved since it opened, with
    /// the pointer on one of its rows. The desktop's panel arrives in that
    /// state by itself whenever the window it serves is refilled.
    #[test]
    fn a_panel_earns_its_hover_afresh_for_every_submenu_it_shows() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0));
        let _ = ctx.run(
            egui::RawInput { screen_rect: Some(screen), ..Default::default() },
            |_| {},
        );
        let mut v = view();
        v.caps = Caps::NONE;
        let mut state = Some(MenuState::open(&ctx, v, Pos2::new(30.0, 30.0), Some(screen)));
        let (menu, row_h, tops) = {
            let st = state.as_ref().expect("the menu did not open");
            assert!(st.subs.len() >= 2, "the test needs two submenus to switch between");
            (
                Rect::from_min_size(st.pos, st.size),
                st.row_h,
                st.subs.iter().map(|g| g.row_top).collect::<Vec<_>>(),
            )
        };
        let frame = |state: &mut Option<MenuState>, at: Pos2| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    focused: true,
                    events: vec![egui::Event::PointerMoved(at)],
                    ..Default::default()
                },
                |ctx| {
                    let _ = show(ctx, state);
                },
            )
        };
        // Whether any row to the RIGHT of the menu - the panel - is lit: a
        // row-high rectangle with any fill at all. An unlit row is drawn
        // fully transparent, and a lit one wears the selection colour under
        // the inline panel's own alpha, so the fill is asked for and not
        // the colour; the backdrop and its shadow are taller than a row.
        let panel_lit = |out: &egui::FullOutput| {
            out.shapes.iter().any(|s| match &s.shape {
                egui::Shape::Rect(rs) => {
                    rs.fill.a() > 0
                        && rs.rect.min.x >= menu.right() - 1.0
                        && rs.rect.height() <= row_h + 1.0
                }
                _ => false,
            })
        };
        let open_by_rest = |state: &mut Option<MenuState>, at: Pos2, want: usize| {
            let _ = frame(state, at);
            std::thread::sleep(REST_FOR + Duration::from_millis(15));
            let _ = frame(state, at);
            assert_eq!(
                state.as_ref().and_then(|st| st.submenu_open),
                Some(want),
                "resting on submenu row {want} did not open it"
            );
        };
        let row_0 = Pos2::new(menu.left() + 20.0, menu.top() + tops[0] + row_h * 0.5);
        let row_1 = Pos2::new(menu.left() + 20.0, menu.top() + tops[1] + row_h * 0.5);
        // Arm the menu from a row away, then rest on the first submenu row.
        let _ = frame(&mut state, Pos2::new(row_0.x, row_0.y + 2.0 * row_h));
        open_by_rest(&mut state, row_0, 0);
        // The panel's first row sits beside the row that opened it.
        let in_panel = Pos2::new(menu.right() + 20.0, row_0.y);
        // The stale state, by hand: the panel opened with the pointer already
        // where it is, and nothing has moved since.
        {
            let st = state.as_mut().expect("menu");
            st.sub_first_pointer = Some(in_panel);
            st.sub_hover_armed = false;
        }
        let out = frame(&mut state, in_panel);
        assert!(
            !panel_lit(&out),
            "a panel the hand has not moved in lit the row under a stale pointer"
        );
        // Moved inside it: the row lights.
        let out = frame(&mut state, in_panel + Vec2::new(ARM_SLOP + 4.0, 0.0));
        assert!(
            state.as_ref().is_some_and(|st| st.sub_hover_armed),
            "moving inside the panel did not arm it"
        );
        assert!(panel_lit(&out), "an armed panel lit nothing under the pointer");
        // Another submenu: the panel is refilled, and it starts over.
        //
        // Approached from ABOVE, down the menu. Coming up from below would
        // cross the corridor to the open panel, which is exactly the journey
        // `heading_for` exists to hold off, and the switch would correctly
        // not happen. See `a_hand_crossing_rows_to_reach_a_clamped_panel`.
        let _ = frame(&mut state, row_0);
        open_by_rest(&mut state, row_1, 1);
        assert!(
            state.as_ref().is_some_and(|st| !st.sub_hover_armed),
            "a panel refilled for another submenu kept the old one's arming"
        );
    }

    /// **The corridor to an open panel**, which is the fault the owner
    /// reported twice: "the right click context menu is still jumping to
    /// the top when a submenu is selected."
    ///
    /// A panel that would overflow the bottom of the screen is pulled UP by
    /// `clamp_submenu`, and most of this menu's categories open from rows
    /// below the halfway mark. Reaching the displaced panel means moving up
    /// and right ACROSS the rows in between, and a pause of one dwell on any
    /// of them used to switch to it: with a panel clamped to the top of the
    /// screen, that is the first category in the list.
    ///
    /// Asserted on `heading_for` directly, because the geometry is the whole
    /// of the fix and driving it through a real menu would make the test
    /// about layout instead.
    #[test]
    fn a_hand_crossing_rows_to_reach_a_clamped_panel_chooses_none_of_them() {
        // A menu at x 30..250, and a panel for a row near the bottom that
        // the clamp has pulled right up to the top of the screen.
        let panel = Rect::from_min_size(Pos2::new(250.0, 20.0), Vec2::new(200.0, 300.0));
        // The hand leaves a row low in the menu and heads up and right.
        let from = Pos2::new(150.0, 560.0);
        for step in [
            Pos2::new(180.0, 480.0),
            Pos2::new(200.0, 400.0),
            Pos2::new(230.0, 300.0),
            Pos2::new(248.0, 200.0),
        ] {
            assert!(
                heading_for(from, step, panel),
                "a hand at {step:?} on its way to a panel at {panel:?} was read as choosing"
            );
        }
        // Straight down the menu, away from the panel: that IS a choice and
        // the corridor must not swallow it.
        for step in [Pos2::new(150.0, 600.0), Pos2::new(140.0, 640.0)] {
            assert!(
                !heading_for(from, step, panel),
                "a hand at {step:?} walking down the menu was held off by the corridor"
            );
        }
        // And a panel that the clamp flipped to the LEFT is reached the
        // other way, which is the case a "moving right" rule would miss.
        let flipped = Rect::from_min_size(Pos2::new(-180.0, 20.0), Vec2::new(200.0, 300.0));
        assert!(
            heading_for(from, Pos2::new(90.0, 300.0), flipped),
            "a hand reaching a left-flipped panel was read as choosing"
        );
    }

    /// **Travelling withholds a guess and never a decision.** The corridor
    /// stops a hover from switching the open panel; a PRESS still does,
    /// because a press is the escape hatch the dwell's own note promises.
    #[test]
    fn the_corridor_holds_a_hover_and_yields_to_a_press() {
        let t = Instant::now();
        let mut open = Some(2);
        let mut pending = None;
        // Resting on row 5 while travelling: nothing moves.
        assert!(!settle_submenu(&mut open, &mut pending, Some(5), false, true, true, false, true, t));
        assert_eq!(open, Some(2), "the corridor let a crossed row take the panel");
        assert!(pending.is_none(), "the corridor left a wait behind");
        // The same row, pressed: it opens at once.
        assert!(!settle_submenu(&mut open, &mut pending, Some(5), false, true, false, true, true, t));
        assert_eq!(open, Some(5), "a press was swallowed by the corridor");
    }

    /// **A menu on a second display is not dragged onto the first.**
    ///
    /// The fault the owner reported three times, and it was never in the
    /// menu's logic. egui gives the app a monitor SIZE and no origin, so
    /// both clamps treated the screen as the box from the origin to that
    /// size. Window positions on macOS are global across the desktop, so a
    /// window on a display placed below or to the right of the primary one
    /// sits outside that box, every overflow test is true, and the clamp
    /// moves the menu - and its submenu - to the primary display's top.
    ///
    /// On the primary display nothing changes, which is the other half of
    /// what has to be true.
    #[test]
    fn a_menu_on_a_second_display_is_left_where_it_opened() {
        // The external, as macOS reports it: the primary display's box
        // begins at the origin and the second one's does not.
        let mon = Rect::from_min_size(Pos2::ZERO, Vec2::new(2560.0, 1440.0));
        let size = Vec2::new(220.0, 300.0);
        // The primary display: clamped exactly as before, at both edges.
        assert_eq!(place(Pos2::new(100.0, 100.0), size, Some(mon)), Pos2::new(100.0, 100.0));
        assert_eq!(
            place(Pos2::new(2500.0, 1400.0), size, Some(mon)),
            Pos2::new(2340.0, 1140.0),
            "a menu at the primary display's corner was not pulled back onto it"
        );
        // A laptop screen ARRANGED BELOW the external: global coordinates
        // past the external's height. The menu stays where the hand is.
        let below = Pos2::new(400.0, 1900.0);
        assert_eq!(
            place(below, size, Some(mon)),
            below,
            "a menu on a display below the primary one was dragged to the top of the primary"
        );
        // And to the right, which is the other common arrangement.
        let right = Pos2::new(3000.0, 300.0);
        assert_eq!(place(right, size, Some(mon)), right);
        // The box itself: known only where the anchor is inside it.
        assert!(screen_box(Pos2::new(10.0, 10.0), Some(mon)).is_some());
        assert!(screen_box(below, Some(mon)).is_none());
        assert!(screen_box(Pos2::new(10.0, 10.0), None).is_none());
        assert!(screen_box(Pos2::new(10.0, 10.0), Some(Rect::ZERO)).is_none());
        // And with the host's real answer, a menu on the second display is
        // clamped to THAT display rather than left unprotected.
        let second = Rect::from_min_size(Pos2::new(0.0, 1440.0), Vec2::new(1512.0, 982.0));
        let low = Pos2::new(400.0, 2300.0);
        let p = place(low, size, Some(second));
        assert!(
            p.y + size.y <= second.max.y + 0.5 && p.y >= second.min.y,
            "{p:?} was not kept on the second display {second:?}"
        );
    }

    fn view() -> MenuView {
        MenuView {
            screen_reader: false,
            dark_mode: false,
            face: (0xE3, 0xD9, 0xBD),
            theme_name: "Default".to_owned(),
            factory_themes: crate::theme::factory().into_iter().map(|t| t.name).collect(),
            // NOT read from disk in a test: a developer's own themes folder
            // would change what the menu tests see.
            user_themes: Vec::new(),
            borderless: false,
            keytoggle: false,
            prefer_flats: true,
            key_note_names: false,
            fret_note_names: false,
            guitar_intervals: true,
            detection_enabled: true,
            chord_strip: true,
            notes_held: false,
            learning_on: false,
            next_font: None,
            supporter: false,
            heart_on: true,
            fretboard_on: false,
            tuning: "Standard".to_string(),
            capo: 0,
            wood: "rosewood",
            theory: crate::theory_panel::Views::default(),
            theory_follows_midi: false,
            // Off, like the setting it mirrors: the band is 200 points tall and
            // a window that grows on its own is the geometry surprise this app
            // has already been bitten by twice.
            recorder_on: false,
            camera_pane_on: false,
            extra_plugin_folders: 0,
            open_when_done: false,
            staff_on: true,
            staff_note_names: false,
            staff_set: "grand".to_owned(),
            staff_custom_label: None,
            staff_clefs: Vec::new(),
            staff_first: false,
            stave_key: "grand".to_owned(),
            staff_key: 0,
            count_in_beats: 4,
            time_signature: crate::recorder::TimeSignature::default(),
            count_in_bars: 1,
            count_in_in_take: false,
            recorder_first: false,
            analyzer: None,
            global_only: false,
            metronome_on: false,
            metronome_in_take: false,
            hide_elapsed: false,
            caps: Caps::DESKTOP,
        }
    }

    /// Everything on at once, for the tests that need the biggest menu there
    /// is. A helper rather than four copies of the same literal, because the
    /// copies drifted the moment the recorder arrived and the "fullest" menu
    /// silently stopped being the fullest one.
    fn fullest() -> MenuView {
        MenuView {
            fretboard_on: true,
            recorder_on: true,
            supporter: true,
            next_font: Some("Terminess"),
            ..view()
        }
    }


    /// **Every right-click menu is readable, on every theme, in both modes —
    /// and the DEFAULT theme keeps its cream-and-black signature.**
    ///
    /// The owner's ruling: the shipped look stays the shipped look, and the
    /// rest are themed and checked. It was a frozen pair before, black on
    /// cream and its inverse, on all sixteen.
    #[test]
    fn every_theme_dresses_the_menus_readably() {
        let mut bad: Vec<String> = Vec::new();
        let mut grounds = std::collections::HashSet::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let face = set
                    .dark_role("fx_strip")
                    .filter(|_| dark)
                    .map_or_else(
                        || {
                            let c = set.fx_strip_color.to_color32();
                            if dark { crate::mixer_panel::shade_face(c, 0.88) } else { c }
                        },
                        |c| c.to_color32(),
                    );
                let c = colors(dark, face);
                grounds.insert((c.bg.r(), c.bg.g(), c.bg.b()));
                let mode = if dark { "dark" } else { "light" };
                let words = crate::recorder_panel::contrast_ratio(c.text, c.bg);
                if words < 4.5 {
                    bad.push(format!("{} {mode}: menu text {words:.2}:1", theme.name));
                }
                // The row under the pointer has to be findable, and its text
                // still readable on it.
                let hl = crate::recorder_panel::contrast_ratio(c.sel, c.bg);
                if hl < 1.06 {
                    bad.push(format!("{} {mode}: highlight {hl:.2}:1", theme.name));
                }
                let on_sel = crate::recorder_panel::contrast_ratio(c.text, c.sel);
                if on_sel < 4.0 {
                    bad.push(format!("{} {mode}: text on highlight {on_sel:.2}:1", theme.name));
                }
            }
        }
        assert!(bad.is_empty(), "menus do not hold up:\n  {}", bad.join("\n  "));
        assert!(
            grounds.len() > 8,
            "only {} distinct menu grounds across sixteen themes",
            grounds.len()
        );

        // **Default is the signature**: the shipped bone, with black on it.
        let mut set = crate::settings::Settings::default();
        crate::theme::Theme::factory_default().apply(&mut set);
        let c = colors(false, set.fx_strip_color.to_color32());
        assert_eq!(
            (c.bg.r(), c.bg.g(), c.bg.b()),
            (0xE3, 0xD9, 0xBD),
            "the default menu stopped being the desk's own bone"
        );
        assert!(
            !crate::recorder_panel::wants_light_ink(c.bg),
            "the default menu stopped taking black text"
        );
    }
    #[test]
    #[ignore = "structure dump, not an assertion"]
    fn dump_menu_structure() {
        // `fullest()`, not `view()`: a dump that leaves out the fretboard and
        // recorder blocks is a dump of two thirds of the menu, which is not
        // what anyone runs this for.
        for e in build_entries(fullest()) {
            match e {
                Entry::Submenu { label, items } => {
                    println!("[{label}]");
                    for it in items {
                        // Greyed items are marked: the whole point of `SubItem`
                        // carrying `enabled` is that a hover can hold one.
                        println!(
                            "    {}{}",
                            it.label,
                            if it.enabled { "" } else { "  (greyed)" }
                        );
                    }
                }
                other => println!("{}", label_of(&other)),
            }
        }
    }

    fn label_of(e: &Entry) -> String {
        match e {
            Entry::Item { label, .. } => label.clone(),
            Entry::Separator => "-----".into(),
            Entry::Submenu { label, .. } => format!("[{label}]"),
        }
    }

    /// Submenu rows, in menu order: (parent label, item labels, item actions).
    fn submenus(v: MenuView) -> Vec<(String, Vec<String>, Vec<MenuAction>)> {
        build_entries(v)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Submenu { label, items } => Some((
                    label,
                    items.iter().map(|it| it.label.clone()).collect(),
                    items.into_iter().map(|it| it.action).collect(),
                )),
                _ => None,
            })
            .collect()
    }


    /// The chord strip says how to get it back, at the top level.
    ///
    /// **The regression this exists for**: it shipped inside the Chords
    /// submenu, seventh of seven rows, and the owner could not find it. It now
    /// has a compartment to itself — the piano's — and the label carries
    /// "(legacy)": the sheet music prints the chord name itself, so the strip
    /// is off by default and this row exists for somebody who wants back the
    /// piano-and-strip window this app was for years.
    #[test]
    fn the_chord_strip_is_the_pianos_own_row_and_says_it_is_legacy() {
        let mut v = view();
        v.chord_strip = false;
        assert_eq!(
            find(v.clone(), MenuAction::ToggleChordStrip),
            Some(("Enable Chord Strip (legacy)".to_owned(), true))
        );
        // TOP level, not inside a hover: `rows` is the top level only.
        assert!(rows(v.clone())
            .iter()
            .any(|(_, a, _)| *a == MenuAction::ToggleChordStrip));

        // It says the other thing when it is up.
        v.chord_strip = true;
        assert_eq!(
            find(v.clone(), MenuAction::ToggleChordStrip),
            Some(("Disable Chord Strip (legacy)".to_owned(), true))
        );

        // And it is gone with no detector behind it, since the row that brings
        // the detector back brings the strip with it.
        v.detection_enabled = false;
        assert!(
            !all_rows(v).iter().any(|(l, ..)| l.contains("Chord Strip")),
            "a strip offered with detection off"
        );
    }

    /// One submenu by name. Positional indexing broke every time a submenu
    /// was added above another, which is a test failing for the wrong reason.
    fn sub(v: MenuView, name: &str) -> (String, Vec<String>, Vec<MenuAction>) {
        submenus(v.clone())
            .into_iter()
            .find(|(n, ..)| n == name)
            .unwrap_or_else(|| panic!("no {name} submenu"))
    }

    /// The hover labels, in menu order — what the top level READS as.
    /// **The analyzer's menu is there while the analyzer is, and not
    /// otherwise**, and its rows print the choice they would advance — so a
    /// person reads "Tilt: 4.5 dB/oct" and knows what a press does to it.
    /// On the band it leads, with the band's other categories.
    #[test]
    fn the_analyzer_category_follows_the_pane() {
        let mut v = view();
        assert!(
            !category_names(v.clone()).iter().any(|c| c == "Analyzer"),
            "an analyzer menu with no analyzer on screen"
        );
        let mut cfg = crate::analyzer::Config::default();
        cfg.tilt = crate::analyzer::Tilt::Three;
        v.analyzer = Some(cfg);
        v.recorder_first = true;
        let cats = category_names(v.clone());
        assert!(cats.iter().any(|c| c == "Analyzer"), "no Analyzer category: {cats:?}");
        assert!(
            cats.iter().position(|c| c == "Analyzer") < cats.iter().position(|c| c == "Window"),
            "the analyzer does not lead on the band: {cats:?}"
        );
        let rows = all_rows(v);
        assert!(rows.iter().any(|(l, ..)| l == "Tilt: 3 dB/oct"), "the tilt row does not say its value");
        assert!(rows.iter().any(|(l, ..)| l == "Mode: Curve"));
        assert!(rows.iter().any(|(l, a, _)| l == "Reset peaks" && matches!(a, MenuAction::Analyzer(crate::analyzer::Param::ResetPeaks))));
    }

    fn category_names(v: MenuView) -> Vec<String> {
        submenus(v).into_iter().map(|(n, ..)| n).collect()
    }

    /// TOP-LEVEL clickable rows only.
    fn rows(v: MenuView) -> Vec<(String, MenuAction, bool)> {
        build_entries(v)
            .into_iter()
            .filter_map(|e| match e {
                Entry::Item {
                    label,
                    action,
                    enabled,
                } => Some((label, action, enabled)),
                _ => None,
            })
            .collect()
    }

    /// Every clickable row in the whole menu: the top level and the inside of
    /// every hover, in the order the user meets them.
    ///
    /// Most of what these tests assert is "this row exists, with this label,
    /// greyed exactly when it cannot act". Grouping the menu into categories
    /// moved a dozen rows one level down without changing any of that, so the
    /// tests ask the whole menu rather than the top level of it. A test of
    /// what `Caps` takes away in particular MUST use this: a row that needs a
    /// device is just as dead one level down.
    fn all_rows(v: MenuView) -> Vec<(String, MenuAction, bool)> {
        build_entries(v)
            .into_iter()
            .flat_map(|e| match e {
                Entry::Item {
                    label,
                    action,
                    enabled,
                } => vec![(label, action, enabled)],
                Entry::Submenu { items, .. } => items
                    .into_iter()
                    .map(|it| (it.label, it.action, it.enabled))
                    .collect(),
                Entry::Separator => Vec::new(),
            })
            .collect()
    }

    fn find(v: MenuView, action: MenuAction) -> Option<(String, bool)> {
        all_rows(v)
            .into_iter()
            .find(|(_, a, _)| *a == action)
            .map(|(l, _, e)| (l, e))
    }

    /// D-UI-9: correcting acts on the held voicing AND needs a visible reading,
    /// so it greys out with no notes down or with detection off.
    ///
    /// Unchanged in intent by the regrouping; it now proves it about a row
    /// inside the Chords hover, which is the reason `SubItem` has an `enabled`
    /// field at all.
    #[test]
    fn correct_item_needs_notes_and_detection() {
        let mut v = view();
        let label = "Correct Chord Name...".to_owned();
        assert_eq!(
            find(v.clone(), MenuAction::CorrectChordName),
            Some((label.clone(), false))
        );
        v.notes_held = true;
        assert_eq!(
            find(v.clone(), MenuAction::CorrectChordName),
            Some((label.clone(), true))
        );
        // Detection off nulls current_chord — nothing to correct against.
        v.detection_enabled = false;
        assert_eq!(
            find(v.clone(), MenuAction::CorrectChordName),
            Some((label, false))
        );
        // Teaching still works with detection off: it pins a name outright.
        assert_eq!(
            find(v.clone(), MenuAction::TeachChordName).map(|(_, e)| e),
            Some(true)
        );
    }

    /// Qt parity: toggles rename themselves rather than showing a checkmark.
    #[test]
    fn learning_toggle_renames_itself() {
        let mut v = view();
        assert_eq!(
            find(v.clone(), MenuAction::ToggleChordLearning),
            Some(("Enable Chord Learning".to_owned(), true))
        );
        v.learning_on = true;
        assert_eq!(
            find(v.clone(), MenuAction::ToggleChordLearning),
            Some(("Disable Chord Learning".to_owned(), true))
        );
    }

    /// The size choices are the ones that existed before submenus were plural,
    /// and they must come out of the generalised path unchanged.
    ///
    /// Same intent as when this test was called `size_is_still_the_first_
    /// submenu...`, asserted against the new shape: "Size" could not become a
    /// child of "Window" (a submenu holds items, not submenus), so the percents
    /// ARE Window's items. Still the first hover, and 100% is still the
    /// MIDDLE one — there are three now rather than seven, at the owner's
    /// word, and the ends of the old list were sizes nobody chose twice.
    #[test]
    fn the_size_percents_are_still_the_first_hover_and_still_read_the_same() {
        let subs = submenus(view());
        assert_eq!(subs[0].0, "Window");
        assert_eq!(subs[0].1[..3], ["75%", "100%", "125%"]);
        assert_eq!(subs[0].2[1], MenuAction::SetSizePercent(100));
        // Borderless joined them rather than sitting loose at the top level.
        assert_eq!(subs[0].1[3], "Borderless");
    }


    /// **No row does what a key already does.**
    ///
    /// This is the whole rule the tidy-up was made of, and it is the one that
    /// rots: every one of these was a perfectly reasonable row to add, and
    /// twenty-six reasonable additions is the menu nobody could read. If a
    /// binding is deleted from `keys.rs`, the feature it carried has to come
    /// back here — which is why this asserts the BINDING exists too, rather
    /// than just that the row is gone.
    #[test]
    fn no_menu_row_does_what_a_key_already_does() {
        use crate::keys::KeyAction;
        let gone = [
            (MenuAction::ToggleDarkMode, KeyAction::ToggleDarkMode),
            (MenuAction::CycleFont, KeyAction::CycleFont),
            (MenuAction::ToggleKeytoggle, KeyAction::ToggleKeytoggle),
            (MenuAction::ToggleNotePreference, KeyAction::ToggleNotePreference),
            (
                MenuAction::ToggleTheoryView(crate::theory_panel::View::Circle),
                KeyAction::ToggleTheoryElement(1),
            ),
        ];
        for (row, key) in gone {
            assert_eq!(
                find(fullest(), row.clone()),
                None,
                "{row:?} is back in the menu, and a key already does it"
            );
            assert!(
                crate::keys::binding_for_test(key),
                "{row:?} left the menu and its key went with it"
            );
        }
        // The rows a key CANNOT replace stay: each opens a dialog you then
        // have to fill in, so the row is the front door people find it through.
        for kept in [
            MenuAction::SelectMidiInput,
            MenuAction::TeachChordName,
            MenuAction::ManageTaughtChords,
            MenuAction::PickColor(ColorTarget::WhiteIdle),
            MenuAction::RescanPlugins,
        ] {
            assert!(
                find(fullest(), kept.clone()).is_some(),
                "{kept:?} has no key and left the menu anyway"
            );
        }
    }

    /// **Every action this menu can carry is built somewhere.** An action no
    /// row and no key builds is an arm of `apply_menu_action` nothing can
    /// reach, and those arms rot unseen: `ToggleMetronome`, `SetCountIn`,
    /// `SetTimeSignature` and twenty-one more outlived the rows that built them,
    /// and committed no undo moment while the live roads to the same
    /// settings had learned to. Read out of the source, because a variant
    /// nothing builds cannot be named by a test without being built: each
    /// variant must appear as `MenuAction::Variant` somewhere other than the
    /// head of a match arm, in the code of this file or of the app, comments
    /// and tests cut away.
    #[test]
    fn every_menu_action_is_built_somewhere() {
        fn code(src: &str) -> String {
            src[..src.find("\nmod tests {").unwrap_or(src.len())]
                .lines()
                .map(|l| l.find("//").map_or(l, |c| &l[..c]))
                .collect::<Vec<_>>()
                .join("\n")
        }
        let menu = code(include_str!("menu.rs"));
        let app = code(include_str!("app.rs"));
        let body = {
            let at = menu.find("pub enum MenuAction {").expect("the enum");
            let rest = &menu[at..];
            &rest[..rest.find("\n}").expect("the enum's end")]
        };
        let variants: Vec<&str> = body
            .lines()
            .filter_map(|l| {
                let name = l.strip_prefix("    ")?;
                let end = name.find(|c: char| !c.is_ascii_alphanumeric()).unwrap_or(name.len());
                let name = &name[..end];
                name.starts_with(|c: char| c.is_ascii_uppercase()).then_some(name)
            })
            .collect();
        assert!(variants.len() > 30, "the scan found only {} variants", variants.len());
        // Past the name, the payload's parentheses, then what follows: an arm
        // head goes on to `=>` or `|`; anything else builds the action.
        let built = |src: &str, needle: &str| {
            src.match_indices(needle).any(|(at, _)| {
                let mut rest = &src[at + needle.len()..];
                if rest.starts_with(|c: char| c.is_ascii_alphanumeric() || c == '_') {
                    return false;
                }
                if rest.starts_with('(') {
                    let mut depth = 0;
                    for (i, c) in rest.char_indices() {
                        depth += match c {
                            '(' => 1,
                            ')' => -1,
                            _ => 0,
                        };
                        if depth == 0 {
                            rest = &rest[i + 1..];
                            break;
                        }
                    }
                }
                let rest = rest.trim_start();
                !(rest.starts_with("=>") || rest.starts_with('|'))
            })
        };
        let dead: Vec<&str> = variants
            .into_iter()
            .filter(|v| {
                let needle = format!("MenuAction::{v}");
                !built(&menu, &needle) && !built(&app, &needle)
            })
            .collect();
        assert!(dead.is_empty(), "built nowhere, so their arms are dead: {dead:?}");
    }

    /// The four blocks that can be empty must take their separators with them.
    ///
    /// A menu whose gaps move around as bands open and close reads as a
    /// rendering bug, and the shapes that produce it — a leading separator, a
    /// trailing one, two in a row — are exactly what a `push` at each block
    /// boundary would have left behind.
    #[test]
    fn no_menu_ever_shows_a_stray_separator() {
        let both = |v: MenuView| {
            let e = build_entries(v);
            assert!(
                !matches!(e.first(), Some(Entry::Separator)),
                "a separator leads the menu"
            );
            assert!(
                !matches!(e.last(), Some(Entry::Separator)),
                "a separator ends the menu"
            );
            for w in e.windows(2) {
                assert!(
                    !(matches!(w[0], Entry::Separator) && matches!(w[1], Entry::Separator)),
                    "two separators in a row"
                );
            }
        };
        both(fullest());
        both(view());
        // Every block empty that can be: no neck, no diagrams, no band, and
        // no detector to hang the strip on.
        both(MenuView {
            fretboard_on: false,
            recorder_on: false,
            staff_on: false,
            theory: crate::theory_panel::Views::of(Vec::new()),
            detection_enabled: false,
            ..view()
        });
        for caps in [Caps::DESKTOP, Caps::NONE, Caps::MINIMAL] {
            both(MenuView { caps, ..fullest() });
        }
    }


    /// D-UI-15: the guitar's four categories appear with the neck and not
    /// before, and that now includes the row that hides it. `G` is what brings
    /// the view back — the arrangement the camera pane has had for releases.
    ///
    /// A Tuning row on a hidden fretboard is a control for something you cannot
    /// see; so, it turns out, is "Show Fretboard" in a menu that is otherwise
    /// only about what is on screen.
    #[test]
    fn the_fretboard_toggle_brings_its_submenus_with_it() {
        let mut v = view();
        assert_eq!(
            find(v.clone(), MenuAction::ToggleFretboard),
            None,
            "the guitar block showed up without a guitar"
        );
        assert_eq!(
            category_names(v.clone()),
            vec!["Window", "Themes", "Colors", "Chords", "Clef", "Key", "Stave", "Reset"],
            "no fretboard hovers while the fretboard is off"
        );

        v.fretboard_on = true;
        assert_eq!(
            find(v.clone(), MenuAction::ToggleFretboard),
            Some(("Hide Fretboard".to_owned(), true))
        );
        // Asserted as the whole list, in order: an inserted submenu that
        // silently shifts Wood/Tuning/Capo is exactly what this catches.
        // Wood, Tuning and Capo are SIBLINGS of Fretboard rather than children
        // of it because a submenu cannot hold a submenu — see `build_entries`.
        assert_eq!(
            category_names(v.clone()),
            vec![
                "Window", "Themes", "Colors", "Chords", "Clef", "Key", "Stave", "Fretboard", "Wood",
                "Tuning", "Capo", "Reset"
            ]
        );
        let wood = sub(v.clone(), "Wood");
        assert_eq!(wood.1.len(), 3, "three woods");
        assert!(
            wood.1[0].starts_with("Rosewood"),
            "rosewood is the default and comes first"
        );
        assert!(wood.1[0].ends_with('\u{2022}'));
        assert_eq!(wood.2[0], MenuAction::SetWood("rosewood"));
        // Every shipped tuning is offered, and the live one is marked rather
        // than hidden: a submenu that never says what is selected makes you
        // close it again to find out.
        let tuning = sub(v.clone(), "Tuning");
        assert_eq!(tuning.1.len(), fretboard::TUNINGS.len());
        assert!(tuning.1[0].starts_with("Standard"));
        assert!(
            tuning.1[0].ends_with('\u{2022}'),
            "the current tuning is marked"
        );
        assert!(!tuning.1[1].ends_with('\u{2022}'));
        assert_eq!(tuning.2[0], MenuAction::SetTuning("Standard".to_string()));
        assert!(
            tuning.2.iter().all(|a| matches!(a, MenuAction::SetTuning(n)
                if fretboard::Tuning::by_name(n).is_some())),
            "every offered tuning must resolve"
        );

        let capo = sub(v.clone(), "Capo");
        assert_eq!(capo.1[0], "No Capo  \u{2022}");
        assert_eq!(capo.1[1], "Fret 1");
        assert_eq!(capo.2[0], MenuAction::SetCapo(0));
        assert_eq!(capo.2.len() as u8, CAPO_MAX + 1);
    }

    #[test]
    fn the_marked_row_follows_the_settings() {
        let v = MenuView {
            fretboard_on: true,
            tuning: "DADGAD".to_string(),
            capo: 3,
            ..view()
        };
        let tuning = sub(v.clone(), "Tuning");
        let marked: Vec<&String> = tuning
            .1
            .iter()
            .filter(|l| l.ends_with('\u{2022}'))
            .collect();
        assert_eq!(marked.len(), 1);
        assert!(marked[0].starts_with("DADGAD"));
        assert_eq!(sub(v.clone(), "Capo").1[3], "Fret 3  \u{2022}");
    }

    /// A submenu low in the menu must slide up rather than run off the bottom
    /// of the screen, and flip left rather than off the right edge.
    ///
    /// Same intent as before, but asserted about EVERY hover and against the
    /// real `clamp_submenu` rather than a copy of it — the copy could pass
    /// while `show` was wrong. It matters more now: grouping the menu into
    /// categories means most hovers open from rows below the halfway mark,
    /// where only Capo used to be.
    #[test]
    fn every_submenu_is_pulled_back_onto_the_screen_near_a_corner() {
        let mon = Rect::from_min_size(Pos2::ZERO, Vec2::new(1440.0, 900.0));
        let row_h = 21.0; // a typical measured row; the clamp is height-blind
        let width = 260.0;
        // The fullest menu there is: fretboard on, so Wood/Tuning/Capo exist,
        // and recorder on, so Pre-roll does — the newest hover and the lowest,
        // which makes it the one most likely to run off the bottom.
        let v = fullest();
        for (name, items, _) in submenus(v.clone()) {
            let size = Vec2::new(width, items.len() as f32 * row_h);
            // Opened from the bottom-right, the corner where a menu that was
            // right-clicked low on a small screen puts its rows.
            let p = clamp_submenu(Pos2::new(1380.0, 860.0), size, 1120.0, mon);
            assert!(
                p.y >= mon.min.y && p.y + size.y <= mon.max.y,
                "[{name}] runs off the bottom at {p:?}"
            );
            assert!(
                p.x >= mon.min.x && p.x + size.x <= mon.max.x,
                "[{name}] runs off the right at {p:?}"
            );
            assert!(
                p.x < 1380.0,
                "[{name}] should flip to the menu's left, not just shrink back"
            );
        }
        // Comfortably inside: untouched.
        let inside = Pos2::new(300.0, 200.0);
        assert_eq!(
            clamp_submenu(inside, Vec2::new(120.0, 260.0), 200.0, mon),
            inside
        );
    }

    /// **The Themes hover fits a short screen, all thirty-two and then
    /// some.** The squeeze used to measure the menu and not its hovers, so a
    /// submenu taller than the screen was slid to the top by
    /// `clamp_submenu` and then simply ran off the bottom. Themes is the
    /// tallest there is since the factory set went to thirty-two, and a
    /// person with a few themes of their own makes it taller still; a
    /// 640-point screen is the short case that matters.
    #[test]
    fn the_themes_hover_fits_a_short_screen() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        let _ = ctx.run(Default::default(), |_| {});
        let mut v = view();
        assert_eq!(v.factory_themes.len(), 32, "the fixture is the whole factory set");
        v.user_themes = (1..=4).map(|i| format!("Mine {i}")).collect();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1280.0, 640.0));
        let m = MenuState::open(&ctx, v, Pos2::new(200.0, 100.0), Some(screen));
        for sub in &m.subs {
            assert!(
                sub.size.y <= screen.height(),
                "a {}-point hover on a {}-point screen",
                sub.size.y,
                screen.height()
            );
        }
        // And it is a squeeze, not a scroll: still above the floor where
        // the rows stop being readable.
        assert!(m.row_h >= MIN_ROW_H, "rows squeezed to {}", m.row_h);
    }

    /// A category must be worth the hover it costs.
    ///
    /// `Caps` can hollow one out (Window loses Borderless and the reader row
    /// without `window_sizing`), and an empty hover is a dead end the
    /// user cannot tell from a bug, while a one-item hover charges a hover for
    /// a single row.
    #[test]
    fn no_category_opens_onto_nothing_or_onto_a_single_row() {
        for caps in [Caps::DESKTOP, Caps::NONE, Caps::MINIMAL] {
            for on in [false, true] {
                let v = MenuView {
                    caps,
                    fretboard_on: on,
                    // Both states matter under every `Caps`: what the band
                    // brings into a hover comes and goes with it.
                    recorder_on: on,
                    supporter: on,
                    next_font: on.then_some("Terminess"),
                    ..view()
                };
                for (name, items, _) in submenus(v) {
                    assert!(
                        items.len() >= 2,
                        "[{name}] is a hover over {} row(s) under {caps:?}",
                        items.len()
                    );
                }
            }
        }
    }

    /// The point of the whole exercise: a top level of subjects, not a
    /// screenful of verbs. It was twenty-six near-flat rows, which is a list
    /// you read rather than a menu you aim at.
    #[test]
    fn the_top_level_is_a_short_list_of_subjects() {
        let count = |v: MenuView| {
            build_entries(v)
                .iter()
                .filter(|e| !matches!(e, Entry::Separator))
                .count()
        };
        // Was 16, then 18, then 27 once the recorder had brought its four
        // categories with it. It is 18 with EVERYTHING on — every band, a
        // supporter's heart and the guitar's three choice lists — because the
        // compartments are gated on the surface being there.
        //
        // Raise this only for a reason that can be written down in the same
        // breath. Nine rows left in one release and none of them was missed;
        // the next nine will be added one reasonable row at a time.
        //
        // 19: the app grew DOCUMENTS — self-contained project directories a
        // song lives in — and a document must be creatable, openable and
        // duplicable from the one menu the app has. One subject row.
        //
        // 20: the recorder band's toggle came BACK when its key (V)
        // retired as unused clutter — the rule that a feature whose key
        // leaves regains its row, written down in
        // `no_menu_row_does_what_a_key_already_does`.
        //
        // 21: THEMES, by the owner's word — a global menu option listing the
        // factory and user sets. It earns a top-level row rather than hiding
        // under Colors for the reason this budget exists at all: a theme is a
        // SUBJECT (the whole app's appearance at once), where every row under
        // Colors is one colour. It also cannot nest under Colors even if we
        // wanted it to — a submenu holds items, never more submenus
        // (`Entry::Submenu`), so folding it in would flatten eight themes and
        // eight colour pickers into one seventeen-row hover.
        //
        // 22: the greyed "Keyboard Shortcuts (hold H)" teaching row. Hold-H
        // was taught once, in a Welcome card with a "don't show this again"
        // box, and then never again — one dismissal and the app's entire
        // keyboard went invisible. This menu is the whole chrome, so the
        // teaching lives here, in every cut.
        assert!(
            count(fullest()) <= 22,
            "the fullest menu is back to {} top-level rows",
            count(fullest())
        );
        // The everyday menu: a piano, the notation and no guitar — plus
        // the recorder band's returned toggle, Themes (see above), and the
        // shortcuts teaching row, all present in every view because neither
        // appearance nor the keyboard is about a surface.
        assert!(
            count(view()) <= 15,
            "the everyday menu is {} top-level rows",
            count(view())
        );
    }


    /// **Nothing detaches any more, anywhere.**
    ///
    /// Four surfaces could be popped into a window of their own, and between
    /// them they were the source of most of what felt janky: a band that could
    /// be on screen and in another window at once, a window that outlived the
    /// band it showed, and a fullscreen main window with children stacked
    /// behind it. The app is now either filling the screen (`Z`) or in the box.
    ///
    /// Asserted over `fullest()` and every `Caps`, because each pair used to be
    /// gated differently and a row that comes back under one host only is a row
    /// nobody would find until a user did.
    ///
    /// The actions themselves are gone (`every_menu_action_is_built_somewhere`
    /// keeps a dead one from lingering), and so are the windows and the state
    /// that said a surface was out, so what is left to hold is the words: no
    /// row offers to pop a surface out or put one back.
    #[test]
    fn nothing_can_be_detached_any_more() {
        for caps in [Caps::DESKTOP, Caps::NONE, Caps::MINIMAL] {
            let v = MenuView { caps, ..fullest() };
            for (label, ..) in all_rows(v) {
                assert!(
                    !label.to_lowercase().contains("attach"),
                    "{label:?} survived at caps={caps:?}"
                );
            }
        }
    }

    /// The custom-tuning editor is a fretboard control, so it appears with the
    /// fretboard and not before. It is a dialog rather than a window, so it is
    /// offered under every `Caps`, by the same reasoning that keeps "Teach
    /// Chord Name..." there.
    #[test]
    fn custom_tuning_is_offered_only_while_the_fretboard_is_showing() {
        assert_eq!(
            find(view(), MenuAction::EditCustomTuning),
            None,
            "an editor for a view the user cannot see"
        );
        let on = MenuView {
            fretboard_on: true,
            ..view()
        };
        assert_eq!(
            find(on.clone(), MenuAction::EditCustomTuning),
            Some(("Custom Tuning...".to_owned(), true))
        );
        let bare = MenuView {
            caps: Caps::NONE,
            ..on.clone()
        };
        assert!(find(bare, MenuAction::EditCustomTuning).is_some());
        // It sits in the Fretboard hover, NOT in Tuning, where every item must
        // resolve to a shipped tuning.
        assert!(sub(on.clone(), "Fretboard")
            .2
            .contains(&MenuAction::EditCustomTuning));
        assert!(sub(on, "Tuning")
            .2
            .iter()
            .all(|a| matches!(a, MenuAction::SetTuning(_))));
    }

    /// The desktop menu must still offer everything it offered under
    /// `Caps::DESKTOP`. Regrouping is only safe if nothing WENT AWAY; where a
    /// row lives is what changed, so this asks the whole menu rather than the
    /// top level of it.
    #[test]
    fn desktop_caps_change_nothing() {
        let mut v = view();
        v.fretboard_on = true;
        v.detection_enabled = true;
        // The band too: its one surviving category is gated on it, and a
        // "nothing went away" test that leaves a surface off cannot see what
        // went away with it.
        v.recorder_on = true;
        let with = all_rows(v.clone());
        // The detach pairs used to head this list. They are gone on purpose
        // now — see `nothing_can_be_detached_any_more`, which is the assertion
        // that replaced them and asserts the opposite deliberately, so that
        // this test cannot quietly become the reason they come back.
        for want in [
            MenuAction::ToggleBorderless,
            MenuAction::SelectMidiInput,
            MenuAction::ToggleChordStrip,
            MenuAction::SetStaffKey(0),
            MenuAction::RescanPlugins,
        ] {
            assert!(
                with.iter().any(|(_, a, _)| *a == want),
                "{want:?} went missing on the desktop"
            );
        }
        assert_eq!(submenus(v.clone())[0].0, "Window");
        // Reset is the last thing on the menu, and it is a CATEGORY now
        // rather than the one row that destroyed everything.
        let subs = submenus(v.clone());
        let (label, _, actions) = subs.last().expect("no submenu at the tail");
        assert_eq!(label, "Reset");
        assert_eq!(actions.len(), crate::settings::ResetScope::ALL.len());
    }

    /// With every capability withheld, no row that needs a window, a MIDI
    /// port or a capture device survives. An inert row is worse than an
    /// absent one: the user cannot tell whether they mis-clicked or the app
    /// is broken.
    #[test]
    fn no_surviving_row_needs_a_window_or_a_device_the_host_lacks() {
        let v = MenuView {
            caps: Caps::NONE,
            fretboard_on: true,
            detection_enabled: true,
            chord_strip: true,
            // Turned ON deliberately. A settings file a Full build wrote says
            // `show_recorder: true`, and any host may be handed it; the
            // category has to be absent because of `caps`, not because the
            // flag happened to be false.
            recorder_on: true,
            // And an instrument LOADED, for the same reason: the settings file
            // remembers a plugin path, so any host may be handed one. The
            // Instrument Window and Unload rows have to be absent because of
            // `caps`, not because nothing happened to be loaded.
            metronome_on: true,
            ..view()
        };
        // `EditCustomTuning` is deliberately NOT here. It raises a dialog, and
        // dialogs are drawn in the canvas when `caps.child_windows` is false —
        // the same road "Teach Chord Name..." takes, which this test asserts
        // survives a few lines down.
        //
        // EVERY recorder action is here. It is forbidden by what it is ABOUT:
        // a capture this host may not perform, so a row offering it is a
        // promise it cannot keep. The detach pairs, the take's own
        // rows, the click and the count-in that stood here went with their
        // rows (`every_menu_action_is_built_somewhere`).
        let forbidden = [
            MenuAction::SelectMidiInput,
            MenuAction::ToggleBorderless,
            MenuAction::ToggleRecorder,
        ];
        for (label, action, _) in all_rows(v.clone()) {
            assert!(
                !forbidden.contains(&action),
                "{label} needs something this host does not have"
            );
        }
        // Borderless is the row that genuinely cannot survive: window chrome
        // belongs to whoever owns the window.
        assert!(
            !all_rows(v.clone())
                .iter()
                .any(|(_, a, _)| *a == MenuAction::ToggleBorderless),
            "window chrome was offered by a host that does not own the window"
        );
        // And what SHOULD survive still does: a host that can do none of
        // that can still teach a chord, change tuning and pick a colour.
        let kept = all_rows(v.clone());
        for want in [
            MenuAction::TeachChordName,
            MenuAction::ManageTaughtChords,
            MenuAction::ToggleFretboard,
            MenuAction::EditCustomTuning,
            MenuAction::ShowAbout,
        ] {
            assert!(
                kept.iter().any(|(_, a, _)| *a == want),
                "{want:?} went missing"
            );
        }
        // Every category is still there, and still in the same order: none of
        // them is emptied out by `Caps::NONE`.
        assert_eq!(
            category_names(v.clone()),
            vec![
                "Window", "Themes", "Colors", "Chords", "Clef", "Key", "Stave", "Fretboard", "Wood",
                "Tuning", "Capo", "Reset"
            ]
        );
    }


    /// **The theory compartment is Clef, Key, Staves and one row.**
    ///
    /// Which diagrams are up is `1`-`4` and `T`; what they are drawn in is
    /// here. The split is the point: the toggles are things you flip while
    /// looking at the band, and the key is something you set once for a piece.
    #[test]
    fn the_theory_compartment_is_the_key_and_the_notation() {
        use crate::theory_panel::{View, Views};
        let v = view();
        let names = category_names(v.clone());
        let at = |want: &str| names.iter().position(|n| n == want);
        assert!(at("Clef") < at("Key") && at("Key") < at("Stave"));
        // The follow row is the one theory toggle with no key of its own, so it
        // is the one that had to stay.
        assert_eq!(
            find(v.clone(), MenuAction::ToggleTheoryFollowsMidi).map(|(l, _)| l),
            Some("Follow MIDI".to_owned())
        );
        assert_eq!(
            find(
                MenuView {
                    theory_follows_midi: true,
                    ..v.clone()
                },
                MenuAction::ToggleTheoryFollowsMidi
            )
            .map(|(l, _)| l),
            Some("Stop Following MIDI".to_owned())
        );

        // **The key belongs to the BAND, not to the staff.** It sets what the
        // harmonic triangles are drawn around as well as how the notation is
        // spelled, so a band showing only the triangles still offers it — and
        // a window with no theory at all offers none of the three.
        let triangles_only = MenuView {
            staff_on: false,
            theory: Views::of(vec![View::Triangles]),
            ..view()
        };
        assert!(category_names(triangles_only.clone())
            .iter()
            .any(|n| n == "Key"));
        assert!(!category_names(triangles_only.clone())
            .iter()
            .any(|n| n == "Clef" || n == "Staves"));
        assert!(find(triangles_only, MenuAction::ToggleTheoryFollowsMidi).is_some());

        let nothing = MenuView {
            staff_on: false,
            theory: Views::of(Vec::new()),
            ..view()
        };
        for gone in [
            MenuAction::SetStaffKey(2),
            MenuAction::ToggleTheoryFollowsMidi,
            MenuAction::SetStaffSet("grand"),
        ] {
            assert_eq!(
                find(nothing.clone(), gone.clone()),
                None,
                "{gone:?} offered with no theory on screen"
            );
        }
    }

    /// The learning block sits with the teach block, after it, and the menu
    /// still ends with About / Reset.
    ///
    /// Same intent, new shape: those four rows are inside the Chords hover
    /// now, so the order is asserted there instead of across the top level.
    #[test]
    fn learning_block_sits_after_the_teach_block() {
        let chords = sub(view(), "Chords").2;
        let pos = |a: MenuAction| {
            chords
                .iter()
                .position(|x| *x == a)
                .unwrap_or_else(|| panic!("{a:?} is not in the Chords hover"))
        };
        assert!(
            pos(MenuAction::ToggleChordDetection) < pos(MenuAction::TeachChordName),
            "the detector's own switch comes before the things that train it"
        );
        assert!(pos(MenuAction::TeachChordName) < pos(MenuAction::ManageTaughtChords));
        assert!(pos(MenuAction::ManageTaughtChords) < pos(MenuAction::CorrectChordName));
        assert!(pos(MenuAction::CorrectChordName) < pos(MenuAction::ToggleChordLearning));

        // **About and Reset are still the last two things on the menu** —
        // but Reset is a submenu, so it is not in `rows`, which lists plain
        // items only. It used to be the last ROW, and the row directly under
        // About destroyed all 164 settings with no confirmation; see
        // `ResetScope`.
        let top = rows(view());
        assert_eq!(
            top.last().map(|(_, a, _)| a.clone()),
            Some(MenuAction::ShowAbout),
            "About is the last plain row"
        );
        // `all_rows` flattens a category into its items, so the last thing
        // it sees is the last ROW of the Reset menu.
        let all = all_rows(view());
        assert_eq!(
            all.last().map(|(_, a, _)| a.clone()),
            Some(MenuAction::Reset(crate::settings::ResetScope::Everything)),
            "Reset is not the last thing on the menu"
        );
        // And every scope is reachable, each one behind its own confirmation.
        let (_, labels, actions) = submenus(view())
            .into_iter()
            .find(|(l, ..)| l == "Reset")
            .expect("the Reset category went missing");
        for scope in crate::settings::ResetScope::ALL {
            assert!(
                actions.iter().any(|a| *a == MenuAction::Reset(scope)),
                "{scope:?} is not on the Reset menu"
            );
            assert!(
                labels.iter().any(|l| l == scope.row()),
                "{scope:?}'s row is not labelled {:?}",
                scope.row()
            );
        }
    }

    /// **The menu teaches hold-H, on every cut, forever.** The Welcome card
    /// taught it once behind a "don't show this again" box, and after that
    /// dismissal the shortcut card — the app's entire keyboard — was
    /// discoverable only by someone who already knew it. The row is GREYED,
    /// this menu's idiom for a heading: the card is held open by design, so
    /// there is nothing for a click to do and the label is the teaching.
    /// And it sits directly above About, where the tail's one-shot rows
    /// live, so the menu still ends About / Reset.
    #[test]
    fn the_menu_teaches_the_shortcut_card_on_every_cut() {
        for v in [
            view(),
            fullest(),
            MenuView {
                global_only: true,
                ..view()
            },
            MenuView {
                caps: Caps::NONE,
                ..view()
            },
        ] {
            let top = rows(v);
            let idx = top
                .iter()
                .position(|(l, ..)| l == "Keyboard Shortcuts (hold H)")
                .expect("the Keyboard Shortcuts row left the menu");
            let (_, _, enabled) = &top[idx];
            assert!(
                !enabled,
                "the shortcuts row is a teaching, not a control — it must \
                 stay greyed until a click has something to do"
            );
            assert_eq!(
                top.get(idx + 1).map(|(l, ..)| l.as_str()),
                Some("About"),
                "the shortcuts row moved away from the tail's About"
            );
        }
    }

    /// §5: this row is the entire affordance. `show_recorder` is off by default
    /// and out of `first_launch()`, this menu is the whole chrome, and an
    /// earlier draft of the recorder shipped with no way to switch it on at
    /// all. So: it exists, it renames itself rather than growing a checkmark,
    /// and the band's other controls arrive with it.
    /// **Crossing rows on the way to a submenu must not change which one is
    /// open.**
    ///
    /// The owner's report: hovering any submenu gave the TOP category about
    /// eight times in ten. Submenus open to the RIGHT, so reaching one means
    /// travelling across the rows between here and there, and every one of them
    /// hovered — what opened was whichever row the path happened to end on.
    #[test]
    fn travelling_across_rows_does_not_switch_the_open_submenu() {
        let t0 = Instant::now();
        let mut open = None;
        let mut pending = None;

        // Arriving on row 3 and stopping there opens it.
        assert!(!settle(&mut open, &mut pending, Some(3), false, true, true, t0));
        assert_eq!(open, Some(3));

        // Now the pointer crosses rows 2, 1 and 0 on its way to the panel. None
        // of them may take over, because none was dwelt on.
        let mut t = t0;
        for row in [2usize, 1, 0] {
            t += Duration::from_millis(20);
            assert!(
                settle(&mut open, &mut pending, Some(row), false, true, false, t),
                "row {row} should have started a wait, not a switch"
            );
            assert_eq!(open, Some(3), "row {row} stole the submenu in transit");
        }

        // Arriving and STAYING is a different thing, and it does switch.
        let arrive = t + Duration::from_millis(20);
        assert!(settle(&mut open, &mut pending, Some(0), false, true, false, arrive));
        let settled = arrive + SUB_SWITCH_DWELL;
        assert!(!settle(&mut open, &mut pending, Some(0), false, true, false, settled));
        assert_eq!(open, Some(0), "a deliberate hover must still work");
    }


    /// **The GLOBAL cut: the arrangement's and the editor's right-click.**
    ///
    /// Leads with the Project, keeps the plugin folders, MIDI input, Window,
    /// Colors and the tail — and carries NOTHING about the piano, the
    /// theory band or the neck: those surfaces are not under the pointer.
    #[test]
    fn the_global_cut_holds_the_world_and_nothing_of_the_surfaces() {
        let v = MenuView {
            recorder_on: true,
            global_only: true,
            ..view()
        };
        let cats = category_names(v.clone());
        assert_eq!(
            cats.first().map(String::as_str),
            Some("Project"),
            "the global cut does not lead with the Project: {cats:?}"
        );
        assert_eq!(cats.get(1).map(String::as_str), Some("Plugin folders"));
        for wanted in ["Window", "Themes", "Colors"] {
            assert!(cats.iter().any(|c| c == wanted), "{wanted} left the global cut");
        }
        for banned in ["Chords", "Theory", "Clef", "Tuning", "Capo", "Wood"] {
            assert!(
                !cats.iter().any(|c| c == banned),
                "{banned} does not belong on the arrangement's menu: {cats:?}"
            );
        }
        // The tail survives whole.
        let rows = all_rows(v);
        assert!(rows.iter().any(|(l, ..)| l == "About"), "About left the tail");
        assert!(
            rows.iter()
                .any(|(_, a, _)| matches!(a, MenuAction::Reset(_))),
            "the Reset category left the tail"
        );
    }

    /// **Right-clicking the band leads with the band's own category.**
    ///
    /// One category, where there were five: show/hide is `V`, and the take's
    /// name, folder, sources, count-in, signature and audio system are behind
    /// the cog, where a setting can show its value. What is left up here is
    /// where the instruments are found — the one recorder question with no
    /// answer on the surface itself.
    #[test]
    fn a_right_click_on_the_band_leads_with_the_recorder() {
        let v = MenuView {
            recorder_on: true,
            recorder_first: true,
            global_only: false,
            ..view()
        };
        // The PROJECT leads — the document the whole surface works in —
        // then where the instruments are found.
        assert_eq!(
            category_names(v.clone()).first().map(String::as_str),
            Some("Project"),
            "the band's categories do not lead: {:?}",
            category_names(v.clone())
        );
        assert_eq!(
            category_names(v.clone()).get(1).map(String::as_str),
            Some("Plugin folders"),
            "the band's categories do not lead: {:?}",
            category_names(v.clone())
        );

        // **And nothing is lost.** It reorders; it does not filter. A context
        // menu showing only what is under the pointer would be a second menu to
        // learn, and the colours would start depending on where you clicked.
        let elsewhere = MenuView {
            recorder_on: true,
            recorder_first: false,
            analyzer: None,
            global_only: false,
            ..view()
        };
        let mut a = category_names(v);
        let mut b = category_names(elsewhere.clone());
        a.sort();
        b.sort();
        assert_eq!(a, b, "the two menus hold different categories");
        assert_eq!(
            all_rows(elsewhere.clone()).len(),
            all_rows(MenuView {
                recorder_first: true,
            global_only: false,
                ..elsewhere
            })
            .len(),
            "a row went missing when the block moved"
        );
    }

    /// **A menu that appears under the cursor must not act on what it landed
    /// on.**
    ///
    /// The menu's top-left is placed AT the click, so on the frame it opens the
    /// pointer is already inside its first row — and the first row is a
    /// submenu. Every single opening therefore opened Window, which is exactly
    /// what the owner reported twice: "most areas flash then default to the top
    /// submenu". Nobody pointed at it.
    #[test]
    fn a_menu_opening_under_the_cursor_opens_nothing() {
        let t = Instant::now();
        let mut open = None;
        let mut pending = None;

        // Not armed: the pointer has not moved since the menu appeared.
        assert!(!settle(&mut open, &mut pending, Some(0), false, false, true, t));
        assert_eq!(open, None, "the top submenu opened without being pointed at");

        // Still not, however long it sits there.
        let later = t + Duration::from_secs(2);
        assert!(!settle(&mut open, &mut pending, Some(0), false, false, true, later));
        assert_eq!(open, None);

        // Move, and it is armed — but a moving pointer still opens nothing.
        assert!(settle(&mut open, &mut pending, Some(0), false, true, false, later));
        assert_eq!(open, None, "arming alone opened the row under the cursor");

        // Come to rest on it, and it is a choice.
        assert!(!settle(&mut open, &mut pending, Some(0), false, true, true, later));
        assert_eq!(open, Some(0));
    }

    /// **A pointer that has STOPPED on a row has arrived.**
    ///
    /// Stillness rather than elapsed time is what makes the switch work at
    /// all: a stationary pointer produces no events, so a dwell waiting only on
    /// a timer can sit unfinished on the row you meant to leave — which is how
    /// the first fix left it still showing the wrong submenu.
    #[test]
    fn a_pointer_that_has_stopped_switches_at_once() {
        let t = Instant::now();
        let mut open = Some(4);
        let mut pending = None;
        // Moving across row 1: no switch.
        assert!(settle(&mut open, &mut pending, Some(1), false, true, false, t));
        assert_eq!(open, Some(4));
        // Stopped on it: switch, on this very frame.
        assert!(!settle(&mut open, &mut pending, Some(1), false, true, true, t));
        assert_eq!(open, Some(1));
    }

    /// Hovering the row that is ALREADY open changes nothing and cancels any
    /// half-finished switch — which is what moving back to it means.
    #[test]
    fn returning_to_the_open_row_cancels_a_pending_switch() {
        let t = Instant::now();
        let mut open = Some(2);
        let mut pending = Some((Some(5), t));
        assert!(!settle(&mut open, &mut pending, Some(2), false, true, false, t));
        assert_eq!(open, Some(2));
        assert!(pending.is_none(), "the abandoned switch was still armed");
    }

    /// **The gesture the owner reported: reaching for a submenu opens the
    /// top one.**
    ///
    /// The menu opens under the cursor with nothing showing, so the cursor is
    /// already on the first row; flicking down towards row 7 hovers every row
    /// in between on the way past. The code before this one opened the first
    /// row it saw, because "nothing is open yet" was read as "there is no
    /// journey to protect" — the journey had barely started.
    #[test]
    fn a_flick_down_the_menu_opens_nothing_on_the_way() {
        let mut t = Instant::now();
        let mut open = None;
        let mut pending = None;
        for row in [0usize, 1, 2, 3, 4, 5, 6] {
            t += Duration::from_millis(16);
            settle(&mut open, &mut pending, Some(row), false, true, false, t);
            assert_eq!(
                open, None,
                "row {row} opened while the pointer was passing over it"
            );
        }
        // And landing is still landing.
        t += Duration::from_millis(16);
        settle(&mut open, &mut pending, Some(7), false, true, true, t);
        assert_eq!(open, Some(7), "the row the pointer stopped on did not open");
    }

    /// A press is a decision, and decisions do not wait.
    ///
    /// The dwell is a guess about intent; a click is the intent itself. It is
    /// also the escape hatch — whatever the pointer heuristics do on a machine
    /// nobody has tested, clicking the category is exact.
    #[test]
    fn a_click_on_a_category_opens_it_without_waiting() {
        let t = Instant::now();
        let mut open = Some(2);
        let mut pending = Some((Some(9), t));
        assert!(!settle_submenu(
            &mut open,
            &mut pending,
            Some(5),
            false,
            true,
            false,
            true,
            false,
            t
        ));
        assert_eq!(open, Some(5), "a click was made to wait for a dwell");
        assert!(pending.is_none(), "the click left a stale wait behind");
    }

    /// **Stillness is ours, because egui's is not stillness.**
    ///
    /// `pointer.velocity()` is `Vec2::ZERO` until three positions have been
    /// sampled over ten milliseconds, so it reads "stopped" for the first two
    /// frames of every gesture. A menu that trusted it was told the pointer had
    /// arrived exactly while it was crossing rows.
    #[test]
    fn a_pointer_still_travelling_is_never_at_rest() {
        let mut rest = None;
        let mut t = Instant::now();
        let mut p = Pos2::new(10.0, 10.0);
        for _ in 0..60 {
            t += Duration::from_millis(16);
            p.y += 10.0;
            assert!(
                !note_rest(&mut rest, Some(p), t),
                "a pointer moving ten points a frame was called still"
            );
        }
        // Let go of, and it arrives within a handful of frames.
        for _ in 0..5 {
            t += Duration::from_millis(16);
            note_rest(&mut rest, Some(p), t);
        }
        assert!(
            note_rest(&mut rest, Some(p), t),
            "a pointer that was let go of never settled"
        );
    }

    /// Coming back in from the panel is a crossing, not an arrival.
    ///
    /// This is the case egui cannot answer at all: `PointerGone` CLEARS the
    /// velocity history, so the first frames after re-entry report a perfectly
    /// motionless pointer — over whichever row it happens to have re-entered
    /// on, which is how a submenu changed under a hand that was reaching for
    /// the one already open.
    #[test]
    fn leaving_the_window_starts_the_rest_over() {
        let mut rest = None;
        let t0 = Instant::now();
        let p = Pos2::new(10.0, 10.0);
        assert!(!note_rest(&mut rest, Some(p), t0));
        let settled = t0 + REST_FOR + Duration::from_millis(1);
        assert!(
            note_rest(&mut rest, Some(p), settled),
            "a pointer sitting in one place was never noticed"
        );

        assert!(!note_rest(&mut rest, None, settled));
        let back = settled + Duration::from_millis(16);
        assert!(
            !note_rest(&mut rest, Some(Pos2::new(10.0, 120.0)), back),
            "re-entering the menu counted as having arrived"
        );
    }

    /// A plain row closes the submenu; being over NOTHING does not.
    ///
    /// The gap between the menu and its panel is "nothing", and closing there
    /// would make the panel impossible to reach on any host that does not
    /// place them flush.
    #[test]
    fn a_plain_row_closes_the_submenu_but_empty_space_does_not() {
        let t = Instant::now();
        let mut open = Some(1);
        let mut pending = None;
        assert!(!settle(&mut open, &mut pending, None, false, true, false, t));
        assert_eq!(open, Some(1), "the gap between menu and panel closed it");

        // **Crossing a plain row is a journey too.** Closing on the frame it is
        // crossed destroys the panel's window and builds it again as soon as
        // the next category is reached: a flicker on every host and a white
        // flash on Windows, for a row nobody was pointing at.
        assert!(settle(&mut open, &mut pending, None, true, true, false, t));
        assert_eq!(open, Some(1), "a plain row crossed in transit closed the panel");

        // Stopping on one is pointing at it, and that does close it.
        assert!(!settle(&mut open, &mut pending, None, true, true, true, t));
        assert_eq!(open, None, "a plain row rested on should have closed it");
    }


    /// The subject is absent, not greyed, where no device may be opened: a
    /// Minimal build has not linked the code that would, and `Caps::NONE`
    /// withholds the devices a build has.
    #[test]
    fn the_recorder_category_is_absent_entirely_without_capture_devices() {
        for caps in [Caps::NONE, Caps::MINIMAL] {
            let v = MenuView {
                caps,
                recorder_on: true,
                ..view()
            };
            assert!(
                !category_names(v.clone())
                    .iter()
                    .any(|n| n == "Recorder" || n == "Count-in"),
                "a Recorder hover survived {caps:?}"
            );
            for gone in [MenuAction::ToggleRecorder] {
                assert_eq!(
                    find(v.clone(), gone.clone()),
                    None,
                    "{gone:?} survived {caps:?}"
                );
            }
        }
    }


}
