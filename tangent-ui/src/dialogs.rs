//! Dialogs (spec §7): MIDI picker (default styling), About (themed, D-UI-6),
//! color modals (egui picker, themed, D-UI-7), MIDI info/error boxes, the
//! custom tuning editor for the guitar view (D-UI-15), and the recorder's
//! Export dialog (RECORDER-PLAN §5).
//! Each dialog is its own immediate viewport, like Qt's top-level dialogs
//! (the fixed-size main window is too small to host them) — or, in a plugin,
//! a layer in the canvas. `show_dialog_viewport` is the funnel that makes
//! that the same code, and nothing here may open a window by itself. A
//! QUESTION wears the OS's chrome and freezes the desk; a PLUGIN window (a
//! face, the Keys panel, the pickers they open) wears the in-house bar from
//! `shell::Chrome` and leaves the desk live — `Dialog::is_modal` is the one
//! place that says which is which.

use std::path::{Path, PathBuf};

use crate::fonts;
use crate::menu::ColorTarget;
// `Layout` is taken by egui's own, and the recorder's is the composite's
// camera-over-display arrangement. Aliased rather than qualified at every use,
// because `unused_qualifications` is a workspace lint and the two would
// otherwise have to be spelled out in full on alternate lines.
use crate::recorder::{
    DisplayShows, ExportSpec, Resolution, VideoMode, FPS_CHOICES, MAX_BPM,
    MIN_BPM,
};
use egui::{
    Align, Align2, Button, Color32, CornerRadius, FontId, Layout, Pos2, Rect, RichText, Stroke,
    Vec2,
};
use tangent_core::fretboard::{self, Tuning, TuningError, MAX_STRINGS};
use tangent_core::patterns::{NOTE_NAMES, NOTE_NAMES_FLAT};
use tangent_core::OverrideInfo;

pub enum Dialog {
    /// **The Konami dialog.** Its words and its one button are the owner's
    /// spec, verbatim; dismissing it is what mints the Cheater unlock.
    Cheated,
    /// The code entered by somebody already standing as a supporter - the
    /// repeat run. Same button, same unlock refresh, different eyebrow.
    CheatFun,
    /// The project menu behind the band's folder button: the session's
    /// name, and the five things you do with sessions.
    Project {
        name: String,
        /// The "Create Project Subdirectory" tick, mirrored from settings so
        /// the box moves under the pointer before the app answers.
        subdir: bool,
    },
    /// The TRACK menu behind a right-click on a strip or a lane header:
    /// remove it, paint it, or choose the auto-palette everybody unpainted
    /// wears.
    /// **The click's own panel.** Tempo, signature, how often it sounds, and
    /// what it sounds like.
    ///
    /// One place rather than four scattered ones: these are the four things
    /// anybody adjusts about a metronome, and three of them lived in
    /// different corners of the app while the fourth did not exist.
     Track {
        lane: usize,
        removable: bool,
        /// The lane's current badge, 0..=3 — the picker's lit slot.
        icon: u8,
        /// Which auto-palette is in force, and whether it is following the
        /// theme — the palette list marks the live one either way.
        palette: i64,
        palette_auto: bool,
        /// Bumped every time this window is opened, so the palette list's
        /// scroll state cannot be carried over from the last time. egui keys
        /// scroll offsets by id and remembers them for the life of the
        /// process; the old whole-dialog scroll came up halfway down a strip
        /// somebody had never scrolled.
        serial: u64,
        /// Whether a track is held on the clipboard, so an EMPTY slot's menu
        /// can offer to receive it — greyed rather than hidden when it is
        /// not, so the row always says what the slot is for.
        can_paste: bool,
    },
    /// The project export: sources, rate, shape, formats, speed — and the
    /// arrange view's time selection, if one stands.
    ExportProject {
        spec: crate::ports::ProjectExportSpec,
        sel: Option<(f64, f64)>,
    },
    MidiPicker {
        ports: Vec<String>,
        selected: Option<usize>,
        current: Option<String>,
    },
    /// Choose a camera or an audio input for the recorder.
    ///
    /// One dialog for both, because the app does exactly the same thing with
    /// each — the same reason `ports::CaptureDevices` is one trait. Separate
    /// from [`Dialog::MidiPicker`] despite looking identical, because the
    /// values are different: a MIDI port is chosen and reported by NAME, and a
    /// capture device by stable UID. Merging them would mean the merged dialog
    /// carried both and each caller ignored one.
    DevicePicker {
        kind: DeviceKind,
        devices: Vec<crate::ports::DeviceInfo>,
        /// `None` is the "None" row, which is a real choice: closing the camera
        /// is how you record audio-only without unplugging anything.
        selected: Option<usize>,
        /// The uid currently open.
        current: Option<String>,
    },
    /// Choose a patch for the built-in FM instrument, and the cartridge it
    /// comes from.
    ///
    /// **Selecting is auditioning.** Every other picker in this file waits for
    /// a Load button, because loading a VST3 costs a second and running
    /// somebody else's code is a decision. A DX7 patch is 155 numbers and
    /// applies between one buffer and the next, so a click here changes the
    /// sound at once and the only button is Done. Dialing in a patch means
    /// playing while you move down the list, and a picker that made you confirm
    /// each one would make that impossible.
    PatchPicker {
        /// The instrument slot the built-in is in, carried so the action can
        /// name it without the app having to look it up again.
        slot: usize,
        /// What the loaded cartridge calls itself, or empty for the patch that
        /// is compiled in.
        bank: String,
        /// The loaded bank is the one that ships, so there is nowhere to go
        /// back to and no Factory button.
        factory: bool,
        /// The cartridge failed its own checksum. Shown, never refused — see
        /// `Cartridge::parse`.
        bad_checksum: bool,
        /// The patch names, in cartridge order. Empty until one is loaded, and
        /// that is the state a fresh install is in.
        voices: Vec<String>,
        selected: Option<usize>,
        filter: String,
        focus: bool,
        /// What went wrong with the last cartridge somebody chose. The whole
        /// point of the lenient parser is that this is rare and specific.
        error: String,
    },
    /// Which instrument the modelled keyboard is being right now.
    ///
    /// **Two sounds, two knobs, nothing else.** The first cut of this dialog
    /// was the two patches alone, on the argument that every knob on a
    /// physical model is a way to make the instrument worse. The owner asked
    /// for controls, and the two that ship are the two that survive that
    /// argument: VOICING is the hammer felt (what a technician's needle
    /// does, not a filter over the output) and WIDTH is where the strings
    /// sit in the field. Both are per STRIP — every lane is its own
    /// instrument now — and neither can reach a value the engine has not
    /// been voiced for.
    KeysPatch {
        slot: usize,
        /// Index into [`KEYS_PATCH_NAMES`].
        selected: usize,
        /// **The chosen sound's whole rack**, normalised 0..=1 and in the
        /// order [`KEYS_PARAM_SPECS`] lists it for that sound.
        ///
        /// The panel holds its own copy so a drag is smooth, and the app
        /// re-seeds it whenever the sound changes: the knobs BELONG to the
        /// sound, so the rack the panel is showing has to change with it.
        params: [f32; KEYS_PARAMS],
    },
    /// Build a patch by hand: every number a DX7 stores, and a picture of the
    /// algorithm.
    ///
    /// **No envelope curves and no animation.** A DX7 has a two-line display
    /// and a membrane keypad, and patches were made on it for forty years by
    /// people who could hear what a rate of 62 does. Drawing the envelope
    /// would be a picture of the numbers rather than the sound, and it would
    /// be the thing the eye went to instead of the ear. The one thing worth a
    /// diagram is the ALGORITHM, because which operator feeds which is a fact
    /// about the patch that no list of numbers makes visible.
    PatchEditor {
        slot: usize,
        /// Everything on show, pushed in by the host — see `ports::PatchEdit`.
        patch: crate::ports::PatchEdit,
        /// Which operator's page is open. 0..=5 are OP1..OP6 and 6 is the
        /// patch itself.
        group: usize,
        /// The name being typed, kept as text so it can pass through empty.
        name: String,
        /// What the last Save said, good or bad.
        note: String,
    },
    /// Choose the VST3 instrument Tangent plays through.
    ///
    /// **The list is a directory listing and nothing more** — see the "plugin
    /// picker" section comment for why this dialog opens no modules, and
    /// [`Dialog::plugin_picker`] for how the caller builds it.
    PluginPicker {
        /// One row per installed VST3 bundle, already sorted for humans.
        plugins: Vec<PluginEntry>,
        /// An index into `plugins`, NEVER into the filtered list. The two are
        /// only ever reconciled in [`plugin_choice`], which is the one place a
        /// selection becomes a value.
        selected: Option<usize>,
        /// The bundle path currently loaded, if any.
        current: Option<String>,
        /// Vendor folders currently EXPANDED. Empty at open — every folder
        /// collapsed, the owner's ruling — except the one holding the
        /// loaded plugin, so the preselected row is not invisible.
        open_folders: Vec<String>,
        /// Free-text filter. There are 112 VST3s on the owner's machine and
        /// scrolling a list that long to find "Pianoteq" is the difference
        /// between a usable picker and a bad one, so this is the primary
        /// interaction rather than a nicety.
        filter: String,
        /// Take keyboard focus on the first frame, like the other text fields
        /// do. It is also what scrolls the loaded plugin into view once, which
        /// is the only thing that makes marking it worth anything ninety rows
        /// down.
        focus: bool,
    },
    /// **A file browser of our own, for a Linux box with no portal.**
    ///
    /// `rfd` on Linux has exactly two backends: xdg-desktop-portal, then a
    /// zenity subprocess. On a machine with neither — which is an ordinary
    /// Void or minimal XFCE install — `pick_file` returns `None`, and rfd
    /// makes that **indistinguishable from the user pressing Cancel**. So the
    /// button did nothing, silently, with no way for the app to know it had
    /// failed. Installing a portal is a fix for one machine; this is the fix
    /// for the app.
    ///
    /// A directory listing and nothing more, in the same spirit as
    /// [`Dialog::PluginPicker`]: no thumbnails, no places sidebar, no
    /// bookmarks. It exists so that somebody with nothing installed can still
    /// choose a file.
    FileBrowser {
        title: String,
        what: BrowseFor,
        /// The directory being shown. The host lists it; this only draws.
        at: std::path::PathBuf,
        /// Directories first, then matching files, sorted by the host.
        entries: Vec<FileEntry>,
        selected: Option<usize>,
        /// Why the last listing came back empty, if it was a reason rather
        /// than an empty folder.
        error: String,
    },
    // **`TakeSummary` is gone, and so are `Notice`, `MidiError` and
    // `NoMidiInput`.**
    //
    // Four dialogs that asked nothing. Each opened a child viewport, took the
    // focus, froze the desk through `is_modal`, and waited to be found and
    // clicked — for "Recorded 2:14 of audio + MIDI to take-0412", for "no MIDI
    // ports were found", for a plugin that had been taken out of its slot. The
    // owner's ruling was to remove every system dialog that is not a question,
    // and those four were the whole of that list: what they said now goes to
    // `App::notify`, which puts it in the corner of the window the user is
    // already looking at for four or five seconds, dismissable with a click and
    // gone on its own if it is not.
    //
    // The two MIDI ones went to `notify_raise` rather than `notify`, because
    // neither is an event: a port is either failed or it is not, and a message
    // about it has to be able to stand while that is true and go when it stops
    // being true.
    //
    // What did NOT go is `LearnResult` — see its own doc for why one of these
    // is genuinely a report and not a message.
    /// **A yes/no in front of something that cannot be taken back.**
    ///
    /// One dialog rather than one per action, and the app's first: before
    /// this there was no "Are you sure" anywhere in it, while "Reset Settings
    /// to Default" sat one slip below About in every context menu and New
    /// Project ran `remove_dir_all` over an unsaved session.
    ///
    /// **Three lines, and the third is the one that was missing.** What this
    /// will do, what it will NOT touch, and the buttons. A destructive dialog
    /// that only names the damage makes somebody cancel out of an action that
    /// was safe, which teaches them to click through the next one.
    Confirm {
        title: String,
        /// What is about to happen, in the words somebody would use.
        body: String,
        /// What SURVIVES, drawn quieter under the body. Empty for none.
        kept: String,
        /// The buttons, right to left: `choices[0]` is rightmost and is where
        /// the eye lands, so the SAFEST answer goes first when there is more
        /// than one. Cancel is always drawn, leftmost, and is never in here.
        choices: Vec<ConfirmChoice>,
        /// The theme's warning colour — `master_knob_color`, the one control
        /// that can undo every other one — handed in rather than looked up,
        /// because this module draws and does not read settings.
        ///
        /// It goes through `surface_of` before anything is written on it, for
        /// the reason every surface in this app does: the ink belongs to the
        /// surface and is measured, never inherited from the role.
        warn: Color32,
    },
    /// **Which automation lanes a track shows** — the A button's menu.
    ///
    /// Checkboxes rather than a picker, because a track can show several at
    /// once: mixing means watching a volume and a send together, and a
    /// control that could only show one made that a thing you toggled
    /// between rather than saw.
    Automation {
        lane: usize,
        /// What the track is called, for the title.
        who: String,
        groups: Vec<AutoGroup>,
        /// Which headings are expanded. The desk's is in here when the menu
        /// opens; the plugins' are not.
        open_groups: Vec<String>,
    },
    /// **Unsaved work is about to be lost.**
    ///
    /// New, Open and Quit all raise this and all carry on afterwards. It
    /// exists because the autosave is gone: every edit used to reach the
    /// disk within a frame, so there was never a change to lose and never a
    /// question to ask. Now there is.
    ///
    /// Three answers and no fourth. Cancel is the one this dialog must NOT
    /// have as an afterthought — it is drawn like every other dialog's, and
    /// it means "I did not mean to do that", which is the commonest reason
    /// anybody reads this box at all.
    SaveChanges {
        /// What is waiting: "quitting", "opening another project".
        what: String,
        /// The project's own name, so the question names the thing at risk.
        name: String,
    },
    About,
    /// **Setup.** What the audio path is doing, and everything that changes it.
    ///
    /// It began as a read-only status card, because every number in it was
    /// invisible: somebody whose interface quietly came up at 44.1 while the
    /// monitor ran at 48 had no way to find out, and the symptom — an
    /// instrument ring overflowing and reporting the losses against the take —
    /// points nowhere near the cause. It is now where the path is CHOSEN as
    /// well as read, which is the same argument one step further: a panel that
    /// shows you a rate mismatch and offers no way to fix it is a panel that
    /// tells you your take will drift and then stands there.
    ///
    /// Everything except `status` is snapshotted when the panel opens.
    /// `systems` and `rates` come from [`crate::ports::AudioSetup`] and
    /// `rates` re-enumerates hardware, so neither may be pushed per frame;
    /// `status` is pushed per frame and must be, because a rate that changed
    /// under you is exactly what this is here to show.
    AudioSetup {
        status: crate::recorder::AudioStatus,
        /// Frames per callback, or `None` for the device's own default.
        buffer: Option<u32>,
        /// Rate for both streams, or `None` for the devices' own.
        rate: Option<u32>,
        /// Every audio system this build can open.
        systems: Vec<String>,
        /// The one in use, or `None` for the platform's default.
        system: Option<String>,
        /// The rates the selected input device offers.
        rates: Vec<u32>,
        /// How many inputs the selected device has, so the channel row can say
        /// whether there is anything to choose between.
        input_channels: u16,
        /// The inputs of it the picker is offering as rows of their own.
        exposed: Vec<(u16, Option<u16>)>,
        /// Whether an ordinary take still reports itself when it finishes.
        ///
        /// **A setting with no switch is worse than no setting.** The tick
        /// that wrote this lived on the take summary, which was a modal box
        /// and is a four-second notification now; the box went and took the
        /// tick with it, leaving `Settings::show_take_summary` read on every
        /// take and reachable by nobody — so anybody who had ever turned the
        /// report off was stuck with it off, for good. It is here because
        /// this panel is where the recording path is configured and because
        /// a switch about takes belongs with the device that makes them.
        /// Snapshotted at open like everything else here but `status`; the
        /// only writer is `DialogAction::SetShowTakeSummary`.
        take_summary: bool,
        /// **The interface's short name**, or empty for its real one.
        ///
        /// It lives HERE, beside the device it renames, and not on the mixer
        /// strip where it began. A channel's name field should name the
        /// channel; this names the box, once, for every channel that comes out
        /// of it. Two fields that looked identical did two different things,
        /// and the one that looked local was the global one.
        alias: String,
        /// **What the monitor is actually playing into.** Snapshotted with
        /// `systems` and `rates`, and for the same reason: it enumerates
        /// hardware, so it may not be pushed per frame.
        output_path: crate::ports::OutputPath,
    },
    /// **Which of an interface's inputs the microphone picker should offer.**
    ///
    /// Two columns of tick boxes, and ticking is the whole interaction: MONO
    /// down the left, one box per input, and STEREO down the right, one box
    /// per adjacent pair — 1/2, 2/3, 3/4, so a stereo source patched across
    /// the middle of the box is as reachable as one on the first two inputs.
    ///
    /// **Any number of them at once, and that is the point.** The version
    /// before this one was a chooser: one input, or one pair, and picking the
    /// room mic meant giving up the pair the piano is on. They are not
    /// alternatives — they are separate microphones that happen to arrive down
    /// one cable — so every ticked box becomes a row of its own in the picker,
    /// named for the interface and the inputs it holds, and the picker goes on
    /// doing what it already did: saying which one is open.
    InputChannels {
        /// What the device is called, for the title and for every row it puts
        /// in the picker.
        device: String,
        /// How many inputs it has.
        channels: u16,
        /// The ticked boxes. `(a, None)` is a mono input, `(a, Some(b))` a
        /// pair.
        exposed: Vec<(u16, Option<u16>)>,
    },
    /// Shown at startup until dismissed with its checkbox. Tangent is free; this
    /// asks for support without gating anything, so it must never feel like a
    /// paywall prompt.
    Welcome {
        dont_show_again: bool,
    },
    /// Where a channel goes besides the master. Two steps in one window: the
    /// kind first (a send or a pair of jacks), then the destination.
    Routing {
        /// The channel this is about.
        channel: usize,
        /// `None` while the two kinds are showing; `Some(true)` listing
        /// hardware outputs, `Some(false)` listing channels to send to.
        picking: Option<bool>,
        /// Hardware destinations the OPEN DEVICE offers: label, first
        /// channel, stereo. Built by the app, because only the host knows
        /// what the interface has.
        hardware: Vec<(String, usize, bool)>,
        /// Channels worth sending to: index and name. Empty slots are left
        /// out — sending into a channel with nothing on it is a route to
        /// silence, and the owner asked for used channels only.
        channels: Vec<(usize, String)>,
        /// What this channel already routes to: the desk-table index, the
        /// destination's label, and the route's LEVEL. Greys the duplicates
        /// in the picker AND gives every existing route a row with a level
        /// and a way OFF - the first pass forgot the way off, and the second
        /// forgot the level, which left a route that could only ever be made
        /// at full and never turned down from the view the owner was in.
        existing: Vec<(usize, String, f32)>,
    },
    /// Name the colours the app is wearing and write them to the user themes
    /// folder. Stays open on failure so the typed name is not lost.
    SaveTheme {
        name: String,
        message: Option<String>,
        focus: bool,
    },
    /// Stream Tangent's master out over ReaStream to OBS/REAPER. Working copies
    /// of the settings, applied on the checkbox and on Done.
    ReaStream {
        enabled: bool,
        identifier: String,
        host: String,
        /// Focus the identifier field on the first frame.
        focus: bool,
    },
    /// Paste a supporter key. Stays open on failure so the message can explain
    /// what went wrong without the user losing what they pasted.
    SupporterKey {
        input: String,
        /// Feedback from the last attempt, if any.
        message: Option<String>,
        /// Name on the currently installed licence, when there is one.
        installed_as: Option<String>,
        /// As above: focus and select-all on the first frame.
        focus: bool,
    },
    ColorPick {
        target: ColorTarget,
        color: Color32,
    },
    // D-UI-5 teach dialogs.
    TeachChord {
        /// Snapshot of the held MIDI notes (used to key the override on save).
        notes: Vec<u8>,
        /// Held notes pre-rendered as names under the active preference.
        note_names: String,
        /// The current detected label (or a placeholder when none).
        current_label: String,
        /// Editable name, prefilled with the current label.
        input: String,
        apply_all_keys: bool,
        /// Take keyboard focus and select the whole field on the first frame.
        /// Cleared once done, so a later frame does not fight the user's own
        /// cursor.
        focus: bool,
    },
    ManageTaught {
        rows: Vec<OverrideInfo>,
        /// D-UI-9 footer: learned-re-ranker state, refreshed on open.
        learning: LearningStatus,
    },
    // D-UI-9 learned re-ranker.
    /// Pick a different reading for the held voicing and train toward it.
    CorrectChord {
        notes: Vec<u8>,
        note_names: String,
        current_label: String,
        /// The names the re-ranker can actually be trained toward, best first.
        candidates: Vec<(String, f64)>,
        selected: Option<usize>,
    },
    /// Plain themed report of what a correction achieved.
    ///
    /// **The one OK-only box that stayed, and the reason is its length.**
    /// Every other dialog that asked nothing — the take summary, the startup
    /// notice, the two MIDI ones — became a notification in the corner. This
    /// one did not, and the decision was made against the actual text rather
    /// than in principle.
    ///
    /// A notification's box grows to `notify::MAX_ROWS` and then elides with
    /// an ellipsis. All but one of the bodies `App::train_and_report` and
    /// `App::open_correct_dialog` compose run to six authored lines or more —
    /// several to nine — and they wrap far past five at the column's 300
    /// points, so a notification would show the opening and hide the rest. The
    /// hidden part is the part that matters: these messages exist to explain
    /// why the thing the user just asked for did NOT happen, and almost every
    /// one of them ENDS by naming the command that would have worked ("use
    /// \"Teach Chord Name...\" to pin it instead"). A four-second box cut off
    /// before the answer is worse than no box, because it looks like it said
    /// something.
    ///
    /// So this one is genuinely a REPORT and not a message, and a report is
    /// what a dialog is for. It is still the only one: the test
    /// `every_ok_only_dialog_is_gone_except_the_one_report` names it and
    /// nothing else.
    LearnResult {
        title: &'static str,
        message: String,
    },
    /// Build a tuning by hand (the "Custom..." entry under Tuning).
    ///
    /// Opened with [`Dialog::custom_tuning`], which seeds it from whatever
    /// tuning is live — almost every custom tuning is "standard but…", and
    /// starting from an empty grid would make the common case the slow one.
    CustomTuning {
        /// The tuning's name. Free text, and the one field the user must
        /// actually think about: see [`Problem::NameClash`] for what happens
        /// when it collides with a preset.
        name: String,
        /// One row per string, index 0 the LOWEST — the same order as
        /// `Tuning::open`, so no reversal happens anywhere except in the
        /// drawing loop, which paints them highest-first the way tab and every
        /// chord chart do.
        strings: Vec<TuningString>,
        /// Sharps or flats, mirrored from `Settings::prefer_flats` when the
        /// dialog is opened. Held here rather than read from settings because
        /// this crate's dialogs are handed their state, and because a tuning
        /// spelled Eb2 in the editor and D#2 on the piano is the one thing a
        /// note-name field must never do.
        prefer_flats: bool,
        /// Focus and select the name on the first frame, as the other dialogs
        /// with a prefilled field do.
        focus: bool,
    },
    /// What one take writes (RECORDER-PLAN §5).
    ///
    /// Opened with [`Dialog::export`], which is also where the one rule that
    /// carries real information is applied — see [`Rederivable`].
    Export {
        /// The spec being edited.
        ///
        /// **`tempo_bpm` in here is the value the dialog OPENED with, not the
        /// live one.** `tempo_text` is what the user is looking at and
        /// [`export_ready`] is the single place the two are reconciled; a
        /// keystroke never writes this field, which is what stops "9" on the
        /// way to "92" from being stored as 20.
        spec: ExportSpec,
        /// Tempo as text, because a partially-typed number is not a number.
        /// "9" on the way to "92" must not be clamped to 20 under the user's
        /// cursor.
        tempo_text: String,
        /// "Use these settings for every take" — the same one-click
        /// persistence affordance as the output folder's "Default" tick, and
        /// it writes to the same place.
        remember: bool,
        /// Opened from the result strip after a take rather than before one.
        /// Camera controls are greyed and the reason is stated.
        post_take: bool,
        /// What the finished take actually captured, when `post_take`. Used to
        /// decide which controls can still do anything.
        had_camera: bool,
    },
    /// A suite unit's face: the kit-drawn panel over one insert bay,
    /// refreshed by the app each frame with pushed data. `strip`/`slot`
    /// are DESK terms.
    InsertFace {
        strip: usize,
        slot: usize,
        live: Box<crate::faces::FaceLive>,
    },
    /// Pick which channel keys one insert bay's sidechain — the routing
    /// window's sibling, one step instead of two because there is only one
    /// kind of source. `strip`/`slot` are DESK terms (`Strip::index` and
    /// the strip's visible slot), like `InsertFace`'s.
    InsertKeySource {
        strip: usize,
        slot: usize,
        /// What the bay listens to now, so the picker can grey it and the
        /// "No sidechain" row can say whether it is already the state.
        current: Option<usize>,
        /// Channels worth keying from: index and name. Used channels only —
        /// an empty channel is a key that never ducks — and never the
        /// destination itself. Folders ARE offered, unlike the routing
        /// picker's list: a group's stem holds the group's whole mix, and
        /// ducking to the drum bus is the classic ask.
        channels: Vec<(usize, String)>,
    },
    /// **The IR wizard**: capture a hardware unit's impulse response through
    /// a physical loop — interface outs into the box, box outs back into the
    /// interface. Staged, because the measurement has an order (patch, set
    /// levels, sweep, judge), and boxed because the stages carry more state
    /// than any other dialog holds. Appended at the enum's END, the
    /// append-only law every parallel lane obeys.
    IrWizard { state: Box<IrWizardState> },
}

impl Dialog {
    /// Whether the main window stands still while this dialog is up.
    ///
    /// **The plugin windows do not take the desk away.** A face, the Keys
    /// panel and the pickers they open are things a person works IN while
    /// the desk stays live — the owner's words: keep the Keys tab open and
    /// tweak the dials in the recorder, the mixer or the arrangement
    /// without closing it. Every other dialog is a QUESTION, and the app
    /// waits for its answer as it always has.
    ///
    /// This is the ONE classification, and every modality decision in the
    /// app goes through it: `handle_main_interaction`'s event drop, the
    /// shortcut gates, the inline scrim, the OS chrome. A `match` with no
    /// wildcard on purpose — a new variant does not compile until somebody
    /// has said which kind it is.
    pub fn is_modal(&self) -> bool {
        match self {
            Dialog::InsertFace { .. }
            | Dialog::KeysPatch { .. }
            | Dialog::PatchPicker { .. }
            | Dialog::PatchEditor { .. }
            | Dialog::PluginPicker { .. }
            | Dialog::InsertKeySource { .. }
            | Dialog::IrWizard { .. } => false,
            Dialog::Cheated
            | Dialog::CheatFun
            | Dialog::Project { .. }
            | Dialog::Track { .. }
            | Dialog::ExportProject { .. }
            | Dialog::MidiPicker { .. }
            | Dialog::DevicePicker { .. }
            | Dialog::FileBrowser { .. }
            | Dialog::Confirm { .. }
            | Dialog::Automation { .. }
            | Dialog::SaveChanges { .. }
            | Dialog::About
            | Dialog::AudioSetup { .. }
            | Dialog::InputChannels { .. }
            | Dialog::Welcome { .. }
            | Dialog::Routing { .. }
            | Dialog::SaveTheme { .. }
            | Dialog::ReaStream { .. }
            | Dialog::SupporterKey { .. }
            | Dialog::ColorPick { .. }
            | Dialog::TeachChord { .. }
            | Dialog::ManageTaught { .. }
            | Dialog::CorrectChord { .. }
            | Dialog::LearnResult { .. }
            | Dialog::CustomTuning { .. }
            | Dialog::Export { .. } => true,
        }
    }
}

/// Which page of the IR wizard is showing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IrStage {
    /// Pick the loop's jacks and patch the cables.
    Io,
    /// The test tone and the return meter — levels before sweeps.
    Level,
    /// Length, tail, the optional loop-only reference pass, and ARM.
    Sweep,
    /// The captured IR: picture, numbers, warnings, Save.
    Result,
}

/// Everything the IR wizard holds between frames. The app snapshots the
/// device's channel lists when it opens the dialog (the `AudioSetup`
/// bargain: enumerating hardware is never a per-frame act) and refreshes
/// `live` each frame from the host's push.
pub struct IrWizardState {
    pub stage: IrStage,
    /// Stereo output pairs the open device offers, as zero-based firsts:
    /// (0,1) is "Out 1/2". Snapshot from the routing plumbing's count.
    pub out_pairs: Vec<(usize, usize)>,
    /// Input pairs, likewise, from the input device's width.
    pub in_pairs: Vec<(usize, usize)>,
    pub out_sel: usize,
    pub in_sel: usize,
    /// The test tone / sweep level, dBFS. −18 first, then −12 — the two
    /// alignment levels the references use; REW's own default is −12.
    pub level_db: f32,
    /// The stage-2 tone is sounding right now.
    pub tone_on: bool,
    /// Index into [`IR_SWEEP_CHOICES`].
    pub sweep_choice: usize,
    /// Post-sweep capture window, seconds — at least the tail expected.
    pub tail_s: f64,
    /// The host's per-frame push: levels, progress, result.
    pub live: crate::ports::IrLive,
    /// The Save field.
    pub name: String,
    /// **A pass fired from THIS wizard is in flight, and the capture id the
    /// live picture carried when it was fired.** The only thing that may
    /// pull the wizard to its result page.
    ///
    /// The host keeps the last finished capture on `IrLive` until a new pass
    /// replaces it, deliberately — closing the wizard mid-thought must not
    /// cost somebody the pass they already took. But the wizard is rebuilt
    /// from `Default` every time it opens and then handed that same live
    /// picture, so a SECOND run arrived at the sweep page already carrying
    /// the FIRST run's result, and the auto-advance fired the instant the
    /// page was reached: stage 3 flashed past and the wizard showed the old
    /// capture as if it were new. That is "it skips stage 3 if the wizard is
    /// used again in succession", and the latch is half the fix — a result
    /// only moves the wizard if the wizard is the one that asked for it.
    ///
    /// The other half is the NUMBER, and it is why this is not a bool.
    /// "The wizard asked" is not the same as "the pass ran": the host's
    /// `start_ir_capture` gives up before it starts anything when no engine
    /// is running or the sweep cannot be generated, and a bool latch left
    /// the wizard armed and waiting over a `result` that still held the
    /// PREVIOUS capture — which it then showed, and would have SAVED under
    /// the new name. So what is remembered is `crate::ports::IrLive`'s
    /// capture id AT THE MOMENT OF ASKING, and the advance waits for that
    /// number to move. A failed arm never moves it, so a stale result can
    /// no longer be mistaken for the one being waited on.
    ///
    /// It replaces the older `retried` flag, which said the same thing
    /// inside out (and only for the Retry button, which is why re-entry was
    /// never covered).
    pub armed: Option<u64>,
}

/// The offered sweep lengths, seconds. 15 is the default (Farina's own
/// experiments; between Apple's 10 and 50); 5 for quick passes on short
/// tails, 50 for the last few dB of noise floor.
pub const IR_SWEEP_CHOICES: [f64; 3] = [5.0, 15.0, 50.0];

impl Default for IrWizardState {
    fn default() -> Self {
        Self {
            stage: IrStage::Io,
            out_pairs: Vec::new(),
            in_pairs: Vec::new(),
            out_sel: 0,
            in_sel: 0,
            level_db: -18.0,
            tone_on: false,
            sweep_choice: 1,
            tail_s: 10.0,
            live: crate::ports::IrLive::default(),
            name: String::new(),
            armed: None,
        }
    }
}

impl IrWizardState {
    /// The chosen device channels, resolved — the wizard's actions carry
    /// these, so the app never re-derives a selection.
    pub fn out_pair(&self) -> (usize, usize) {
        self.out_pairs.get(self.out_sel).copied().unwrap_or((0, 1))
    }

    pub fn in_pair(&self) -> (usize, usize) {
        self.in_pairs.get(self.in_sel).copied().unwrap_or((0, 1))
    }

    pub fn sweep_s(&self) -> f64 {
        IR_SWEEP_CHOICES
            .get(self.sweep_choice)
            .copied()
            .unwrap_or(15.0)
    }

    /// **Back to the sweep page for another capture, loop and levels
    /// intact.** "Record another IR" and Retry are the same move: the jacks,
    /// the send level, the sweep length, the tail and any held loop-only
    /// reference all stay exactly as they were staged, and only the two
    /// things that belong to ONE capture are cleared.
    ///
    /// The name goes because carrying it forward is how the second capture
    /// silently overwrites the first — the field would still say
    /// "Holmes Plate" and Save would still be one click. The latch goes
    /// because the result still on the live push belongs to the pass just
    /// finished, and leaving it set would bounce this page straight back to
    /// the result the moment it was reached.
    pub fn record_another(&mut self) {
        self.stage = IrStage::Sweep;
        self.armed = None;
        self.name.clear();
    }

    /// **Fire a capture through the unit: the pass whose result may move
    /// this wizard on.** The sweep page's Capture button, and nothing else,
    /// calls this.
    ///
    /// The name field is cleared with it: a second capture must not arrive
    /// on the result page already named after the first one, one click from
    /// writing over it.
    ///
    /// A METHOD rather than two lines at the button, because
    /// [`Self::settle_from_live`] is the other end of the same latch and a
    /// test that cannot press an egui button has to be able to arm the way
    /// the button arms. Two spellings of one rule is how the rule drifts.
    pub fn arm_capture(&mut self) {
        self.armed = Some(self.live.capture_id);
        self.name.clear();
    }

    /// **Fire the loop-only reference pass**, which produces no result to
    /// show and therefore does NOT arm the advance: the wizard stays on the
    /// sweep page waiting for the pass through the unit.
    pub fn arm_reference(&mut self) {
        self.armed = None;
    }

    /// **A finished capture pulls the wizard to its result page** — the
    /// deconvolution ran on the host and pushed the outcome; nobody should
    /// have to click "next" to see what they just measured.
    ///
    /// Three things have to be true, and each of them is a bug that was
    /// reported. The wizard has to be ON the sweep page, or an auto-advance
    /// would drag somebody off the page they walked back to. It has to have
    /// ARMED the pass itself, or a reopened wizard advances on the previous
    /// run's held result. And the host's capture id has to have MOVED past
    /// the one this wizard saw when it armed, or an arm the host refused —
    /// no engine, no sweep — leaves the wizard advancing onto the previous
    /// capture and offering to save it under the new name.
    ///
    /// `show` calls exactly this, and so does its test. Lifting it out of
    /// the frame is the whole point: an auto-advance written once inside an
    /// egui closure can only be tested by a copy of itself, and a copy
    /// proves nothing — revert this body to the broken predicate and the
    /// test that drives it goes red, which is the only reason to trust it.
    pub fn settle_from_live(&mut self) {
        let Some(asked_at) = self.armed else { return };
        if self.stage == IrStage::Sweep
            && self.live.result.is_some()
            && self.live.capture_id != asked_at
        {
            self.stage = IrStage::Result;
            self.armed = None;
        }
    }
}

/// Snapshot of learned-re-ranker state for display.
#[derive(Clone, Default)]
pub struct LearningStatus {
    pub on: bool,
    pub corrections: u32,
    pub has_learned: bool,
    /// One row per perceptron feature: (label, weight).
    pub weights: Vec<(&'static str, f64)>,
}

/// `TrackPalette` with this means "follow the theme" rather than naming one.
pub const PALETTE_AUTO: i64 = -1;

/// How wide each of `count` cells may be to fit `available` with `gap`
/// between them.
///
/// **The gap is the part that gets forgotten.** `ui.horizontal` inserts
/// `item_spacing.x` between every pair, so a row of nine cells has eight
/// gaps and a row of eighteen has seventeen — and a width computed by
/// dividing the available space by the count alone overflows by exactly that
/// much. egui then CLIPS it, so there is no overdraw and no panic: the last
/// cells simply are not there, which is the shape this bug arrived in.
///
/// Both swatch rows in the track dialog got it wrong in different ways: one
/// assumed a three-point gap where there were eight, and the other budgeted
/// eight points in total for seventeen of them.
///
/// **Zero when the gaps alone do not fit.** Eighteen eight-point gaps do not
/// go into a hundred and twenty points however narrow the cells are, and no
/// width this could return would make them; the caller needs a smaller gap.
pub fn cells_across(available: f32, count: usize, gap: f32) -> f32 {
    if count == 0 || !available.is_finite() {
        return 0.0;
    }
    let gaps = (count.saturating_sub(1)) as f32 * gap.max(0.0);
    ((available - gaps) / count as f32).max(0.0)
}

/// One button on a [`Dialog::Confirm`].
/// One row in the automation menu: a parameter, and whether its lane shows.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoRow {
    /// `AutoParam::key()` — the address, not the position.
    pub key: String,
    pub label: String,
    pub on: bool,
}

/// One heading in the automation menu, with its rows under it.
///
/// **The desk first, then a group per loaded bay.** The desk's four are what
/// anybody opening this is most likely to want, so they are expanded when the
/// menu opens; a plugin with sixty parameters is a list you go looking in, so
/// its group starts shut.
#[derive(Debug, Clone, PartialEq)]
pub struct AutoGroup {
    pub name: String,
    pub rows: Vec<AutoRow>,
}

pub struct ConfirmChoice {
    pub label: String,
    /// What the app does if this one is pressed. An ordinary
    /// [`DialogAction`], so a confirm is a GATE in front of the action rather
    /// than a second way of expressing it.
    pub action: DialogAction,
    /// Wears the warning colour. At most one choice should be destructive;
    /// two of them is a dialog with no safe answer, which is a dialog that
    /// should have had a different question.
    pub destructive: bool,
}

impl ConfirmChoice {
    pub fn new(label: &str, action: DialogAction, destructive: bool) -> Self {
        Self { label: label.to_owned(), action, destructive }
    }
}

/// **`Clone`, so a confirmation can hold one.** Every other action in this
/// file is produced and consumed in one frame; a [`ConfirmChoice`] has to
/// keep one alive across the frames the dialog is open.
#[derive(Clone)]
pub enum DialogAction {
    /// The "..." on the Konami dialog: accept the unlock.
    AcceptCheat,
    /// The unsaved-changes prompt's four answers. See `Dialog::SaveChanges`.
    SaveChangesSave,
    SaveChangesSaveAs,
    SaveChangesDiscard,
    SaveChangesCancel,
    /// Show or hide one automation lane. The `String` is `AutoParam::key()`.
    AutomationToggle(usize, String),
    /// Expand or collapse one heading in the automation menu.
    AutomationGroup(String),
    /// New session from the `default` template.
    ProjectNew,
    /// Open the project picker.
    ProjectOpen,
    /// Write the session down now — desk, document, plugin states.
    ProjectSave {
        /// The name the dialog's field is staging: Save saves THIS name,
        /// renaming the session if it differs — no panel in between.
        name: String,
    },
    /// Duplicate the whole session somewhere and open the copy.
    ProjectSaveAs,
    /// Save the session as a template under `Projects/Templates`.
    ProjectTemplate,
    TrackRemove(usize),
    /// Take this whole track — kind, name, colour, levels, bays and clips —
    /// and hold it for an empty slot.
    TrackCopy(usize),
    /// Put the held track on this EMPTY slot.
    TrackPaste(usize),
    /// Paint one channel by hand — index into `STRIP_COLORS`.
    TrackColor(usize, i64),
    /// The lane's badge: 0 auto, 1 drum, 2 guitar, 3 bass.
    TrackIcon(usize, u8),
    /// Choose the auto-palette every unpainted slot wears.
    TrackPalette(i64),
    /// Bounce the arrangement to one WAV.
    ProjectExportTrack,
    /// Open the export dialog.
    ProjectExportOpen,
    /// Run the export the dialog assembled.
    ProjectExportRun(crate::ports::ProjectExportSpec),
    /// Pick the export directory with the native chooser.
    ProjectExportBrowse,
    /// Bounce every used lane to its own WAV.
    ProjectExportStems,
    ConnectPort(String),
    /// List this directory and hand the rows back. The host reads the disk;
    /// the dialog draws what it is given, the same division the plugin picker
    /// uses.
    BrowseTo(std::path::PathBuf),
    /// The browser's answer. `None` is a cancel, and is sent so the host can
    /// drop whatever it was holding for the request.
    Browsed {
        what: BrowseFor,
        path: Option<std::path::PathBuf>,
    },
    /// Persist the "Create Project Subdirectory" tick.
    SetProjectSubdir(bool),
    /// Persist the welcome dialog's "don't show again" choice.
    SetShowWelcome(bool),
    /// Open the routing window for a channel.
    OpenRouting(usize),
    /// Step into one of the two lists: `true` for hardware outputs.
    RoutingPick { channel: usize, hardware: bool },
    /// Make the route. The app owns the no-duplicates rule.
    AddRoute {
        from: usize,
        target: crate::routing::Target,
    },
    /// Take a route off, by its index in the desk's table.
    RemoveRoute(usize),
    /// Put media on this lane at the playhead, through the OS's own panel.
    InsertMedia(usize),
    /// Set one route's level, by its index in the desk's table.
    SetRouteGain(usize, f32),
    /// Write the current colours to the user themes folder under this name.
    SaveThemeNamed(String),
    /// Open the ReaStream (stream-to-OBS) settings dialog.
    OpenReaStream,
    /// Persist the ReaStream send settings and reconcile the sender.
    SetReaStream {
        enabled: bool,
        identifier: String,
        host: String,
    },
    /// Persist the take summary's "report every take" choice.
    ///
    /// **Its tick went out with the modal box and now has a home again.** The
    /// take summary was a dialog with a "Don't show this again" checkbox in
    /// it; the report is a four-second notification now and the box is gone,
    /// which left this action with no caller outside a test and the setting
    /// behind it readable but unwritable — a switch somebody had already
    /// thrown and could never throw back. The switch is in Setup, under
    /// REPORTS, beside the device the takes are recorded from. See
    /// `Settings::show_take_summary`.
    SetShowTakeSummary(bool),
    /// Frames per audio callback, or `None` for the device's own default.
    /// Applies to both streams and reopens them.
    SetBufferFrames(Option<u32>),
    /// Sample rate for both streams, or `None` for the devices' own.
    /// Applies to both and reopens them, exactly as the buffer does.
    SetSampleRate(Option<u32>),
    /// The audio system every stream opens through. `None` is the platform's.
    ///
    /// The heaviest of the three: it does not merely reopen the streams, it
    /// changes which driver stack they are opened against, so the device
    /// selected under the old system may not exist under the new one.
    SetAudioSystem(Option<String>),
    /// Offer exactly these inputs of the selected device as picker rows.
    /// Empty means the device on its own, which is where everybody starts.
    SetInputChannels(Vec<(u16, Option<u16>)>),
    /// Open the channel chooser for the device now selected.
    ChooseInputChannels,
    /// The interface's short name changed. See `Dialog::AudioSetup::alias`.
    SetInputAlias(String),
    /// Open the microphone picker from Setup. The device half of "the audio
    /// system and device", without a second copy of the list to keep in step.
    ChooseAudioDevice,
    /// Try to install a pasted supporter key. The app reports the outcome by
    /// updating or closing the dialog.
    InstallLicense {
        key: String,
    },
    ApplyColor(ColorTarget, Color32),
    /// Teach `name` for the held `notes`, transposition-invariant when
    /// `apply_all_keys` and the name begins with a note name.
    TeachSave {
        notes: Vec<u8>,
        name: String,
        apply_all_keys: bool,
    },
    /// Delete the override with this interval-set-from-bass key.
    DeleteOverride {
        intervals: Vec<u8>,
    },
    /// D-UI-9: train the re-ranker to read `notes` as `name`.
    TrainCorrection {
        notes: Vec<u8>,
        name: String,
    },
    /// D-UI-9: discard all learned weights.
    ForgetLearning,
    /// Adopt a tuning the user built by hand.
    ///
    /// It is already valid: the only path to this action is through
    /// [`confirm`], which builds it with `Tuning::custom`, so the solver's
    /// monotone invariant holds and the app does not have to check again.
    ///
    /// **It cannot be stored the way a preset is.** `Settings` keeps
    /// `fretboard_tuning` as a NAME, and `Settings::fretboard_spec` resolves
    /// that name through `Tuning::by_name`, which knows only the preset table:
    /// a custom tuning saved as a bare name comes back as Standard on the next
    /// launch, silently. The pitches have to be persisted next to the name.
    SetCustomTuning(Tuning),
    /// Use this spec for the rest of this session, without persisting it.
    ///
    /// Already valid: the only path to either of these is [`export_ready`], so
    /// `problem()` is None and the tempo mark is inside `MIN_BPM..=MAX_BPM`.
    /// The app does not have to check again.
    /// A capture device was chosen. `None` means the None row: close whatever
    /// is open and select nothing.
    ChooseDevice {
        kind: DeviceKind,
        uid: Option<String>,
    },
    /// Load a VST3 instrument, or unload the one that is loaded.
    ///
    /// The value is the BUNDLE PATH and not a name, because the path is the
    /// identity and the settings key: two vendors ship a "Piano", and a plugin
    /// renamed between versions would come back from the settings file as
    /// nothing at all.
    ///
    /// `None` is the picker's None row: unload, and play nothing.
    ///
    /// **This is the one action in the file that runs somebody else's code
    /// inside Tangent.** It is why a Full build carries
    /// `com.apple.security.cs.disable-library-validation`, and why the dialog
    /// says so before the button is clicked rather than after.
    LoadPlugin {
        path: Option<String>,
    },
    /// Play this patch of the loaded cartridge now. See [`Dialog::PatchPicker`]:
    /// selecting is auditioning, so this fires on the click and not on a
    /// button.
    /// Load one of the modelled keyboard's patches, on one strip's own
    /// instrument.
    ChooseKeysPatch {
        slot: usize,
        index: usize,
    },
    /// One of the keyboard panel's knobs moved, 0..=1. Fired per change so
    /// the sound follows the drag, exactly as the DX7 editor's rows do.
    SetKeysParam {
        slot: usize,
        knob: usize,
        value: f32,
    },
    /// **Initialise the sound**: every knob of the strip's current patch back
    /// to the value the bench signed off. The other sound is untouched.
    ResetKeysParams {
        slot: usize,
    },
    ChoosePatch {
        slot: usize,
        index: usize,
    },
    /// Raise a file panel for a `.syx` cartridge, for one strip's own DX7.
    /// The dialog stays open: the answer comes back into it.
    LoadCartridge {
        slot: usize,
    },
    /// Put the shipped cartridge back on one strip. The dialog stays open
    /// and refills with its thirty-two names, exactly as loading a file does.
    UseFactoryCartridge {
        slot: usize,
    },
    /// Open the patch editor on whatever the built-in is playing.
    EditPatch {
        slot: usize,
    },
    /// One row of the patch moved. Addressed by GROUP and INDEX — see
    /// `dx7::edit`, where that order is defined.
    SetPatchParam {
        group: usize,
        index: usize,
        value: i32,
    },
    /// Rename the patch being edited.
    SetPatchName(String),
    /// Write the patch being edited into the user's own bank.
    SavePatch,
    /// Back from the editor to the list of patches.
    ShowPatches {
        slot: usize,
    },
    SetExport(ExportSpec),
    /// As above, and write it to the settings file so every later take starts
    /// from it. This is the "Use these settings for every take" tick, and it
    /// goes to the same place the output folder's "Default" tick does.
    SetExportAndRemember(ExportSpec),
    /// Put one part of the settings back — the CONFIRMED half of a reset.
    ///
    /// There is deliberately no unconfirmed road to this: the menu row opens
    /// a [`Dialog::Confirm`] and this arrives from its button.
    ResetTo(crate::settings::ResetScope),
    /// Put the settings back to the copy taken just before the last reset.
    UndoReset,
    /// Put the whole song back to one tempo, dropping its map.
    FlattenTempo,
    /// Start a new project, and take the unsaved one's directory with it.
    NewProjectDiscarding,
    /// Start a new project and leave the unsaved one where it is.
    NewProjectKeeping,
    // ── the suite faces ─────────────────────────────────────────────────
    /// One knob on a built-in insert, in DESK terms — the app converts to
    /// the seat once, on its side of the wall.
    SetInsertParam { strip: usize, slot: usize, id: u32, value: f32 },
    /// Save the face's current values under a name; the app reads its own
    /// mirror for the values, so the action stays a wish, not a payload.
    FaceSavePreset { strip: usize, slot: usize, name: String },
    /// Load a preset file into whatever the open face controls.
    FaceLoadPreset { unit: String, path: String },
    /// Point one bay's sidechain key at a channel, or take it off with
    /// `None`. DESK terms like the dialog that fires it; the desktop's
    /// per-frame publish owns the `seat_bay` translation, so the edge
    /// survives every seat renumbering the desk migrations perform.
    SetInsertKeySource { strip: usize, slot: usize, from: Option<usize> },
    /// The amp face's pedal chip: open the insert picker over pedal
    /// `pedal` of the channel whose bay the face sits on. `strip`/`slot`
    /// are the face's own DESK terms; the app resolves the channel through
    /// `seat_bay` and the pedal through `amp_pedal_seat`, on its side of
    /// the wall. Appended at the END, per this enum's law.
    OpenPedalPicker { strip: usize, slot: usize, pedal: usize },
    // ── the IR wizard ───────────────────────────────────────────────────
    // Appended at the enum's END, the append-only law. The wizard's five:
    // a door in Setup, and the four asks the app maps onto requests.
    /// Open the IR wizard — Setup's "Capture hardware IR..." door. The app
    /// builds the channel lists when it opens the dialog.
    OpenIrWizard,
    /// The stage-2 test tone, on or off, on these device channels.
    IrTone {
        on: bool,
        out: (usize, usize),
        inp: (usize, usize),
        level_db: f32,
    },
    /// Arm one sweep pass — `reference: true` is the loop-only bypass pass.
    IrCapture {
        out: (usize, usize),
        inp: (usize, usize),
        sweep_s: f64,
        tail_s: f64,
        level_db: f32,
        reference: bool,
    },
    /// Stop whatever the IR road is doing. Also fired when the wizard
    /// closes, so a tone can never outlive its meter.
    IrCancel,
    /// Write the finished capture as `IRs/<name>.wav` and refresh the shelf.
    IrSave { name: String },
    /// The IR face picked an impulse off the shelf; desk terms, the app
    /// converts and asks the host to rebind the bay.
    ///
    /// **[`IR_BROWSE`] in `path` means "ask me instead".** See there.
    SetInsertIr { strip: usize, slot: usize, path: String },
    /// The theme key was pressed inside a plugin window. On the desktop
    /// that window has its own input and the main window never sees the
    /// key, so the dialog carries it back and the app cycles exactly as it
    /// would from its own keymap. Appended at the END, the law.
    CycleTheme,
}

/// **The `path` that means "raise the file chooser for this bay" rather
/// than "load this file".**
///
/// [`DialogAction::SetInsertIr`] carries a path from the IR face's shelf to
/// the host. The face's NAME FIELD needs the same road but with the path
/// still unknown — the OS panel has to be raised on the host's thread,
/// outside an egui frame, which is where every other chooser in this app is
/// raised from — and a new action would need an arm in `app.rs`, which this
/// lane does not own.
///
/// So the ask rides the path field as a sentinel, the idiom the insert
/// reload already uses (`"\u{1f}reload"` in `desktop.rs`): a leading unit
/// separator cannot begin a real filesystem path, and the host tests for
/// this exact string before treating the value as a file. `desktop.rs`
/// answers it by raising the chooser and, on an answer, rebinding the bay
/// through the same code an ordinary path takes.
///
/// **The ask is PARKED, never answered in the frame that made it.** The
/// face closes itself as it asks (below), but a closed viewport is not a
/// gone window: eframe destroys it a frame later, and the OS's panel is
/// parented to the MAIN window, so a panel raised now opens underneath a
/// face that is still on screen and still always-on-top — the app's own
/// "Save As loads below the menu" bug, wearing a different hat.
/// `desktop::take_ir_browse` therefore holds the request for the same
/// settle every other picker in that file takes, and comes out of
/// fullscreen first on the window managers that need it.
pub const IR_BROWSE: &str = "\u{1f}browse";

// ── the custom tuning editor ────────────────────────────────────────────────
//
// A tuning is six-to-twelve numbers, so the temptation is a row of numeric
// fields and a Save button. Two things make it more than that.
//
// The first is that a guitarist thinks in note names. "The low string is E2"
// is a sentence; "the low string is 40" is a lookup. Both are offered on every
// row and they stay in step, and the SPELLING comes from `tangent_core::patterns`
// rather than a table written here, so the editor cannot start calling a note
// Eb while the piano above it calls the same note D#.
//
// The second is that one of the rules being enforced is load-bearing rather
// than cosmetic. `Tuning::custom` refuses a tuning whose strings do not ascend,
// because the voicing solver's search IS the assumption that they do; a tuning
// that dips does not produce a wrong shape, it produces a search that quietly
// stops covering real candidates. An editor that answered that with "invalid"
// would be hiding the only interesting thing it knows, so every refusal here is
// reported as a reason.

/// The editor as a window. Wide enough that "Remove low string" and "Strings:"
/// share a line, tall enough that a six-string tuning needs no scrolling.
const TUNING_DIALOG_W: f32 = 480.0;
const TUNING_DIALOG_H: f32 = 500.0;
/// The `Frame` margin on every side. Named rather than inline because the width
/// the message wraps into is the dialog's less two of these, and the test that
/// proves the message fits has to measure at exactly that width.
const TUNING_MARGIN: i8 = 12;
/// Point size of the message under the string list.
const TUNING_FOOTER_PT: f32 = 10.0;
/// Height reserved for that message, whether or not there is one.
///
/// Reserved rather than measured per frame, and deliberately generous. A footer
/// that grew as you typed would walk the button row up and down under the
/// cursor; a footer reserved too SMALL pushes the confirm button off the bottom
/// edge, which is the bug the D-UI-9 result box already had once and which only
/// Esc could get you out of. `the_longest_refusal_fits_the_space_reserved_for_it`
/// measures the real messages against this rather than trusting the arithmetic.
const TUNING_FOOTER_H: f32 = 58.0;

/// One editable string in the tuning editor.
///
/// The two fields are the same value in two notations, and the invariant is
/// that `text` is what the user last committed to: it is what [`confirm`] reads
/// and it is what is on screen. `pitch` is the last value `text` successfully
/// parsed to, which is what the number spinner drags, and the two can only
/// disagree while a half-typed name (`"E"`, `"A#"`) sits in the field. Editing
/// either one writes the other, which is the whole reason both can be offered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TuningString {
    /// A note name (`E2`, `A#3`, `Db4`) or a bare MIDI number.
    pub text: String,
    /// The MIDI pitch of the open string.
    pub pitch: u8,
}

impl TuningString {
    /// A row showing `pitch`, spelled under the caller's accidental
    /// preference.
    pub fn new(pitch: u8, prefer_flats: bool) -> Self {
        Self {
            text: pitch_name(pitch, prefer_flats),
            pitch,
        }
    }
}

/// A new low string is tuned a fourth below the one above it.
///
/// Not a guess: every extended-range instrument in `TUNINGS` grows downward in
/// fourths. 7-String is Standard with a low B at 35 (a fourth under the low E
/// at 40), 8-String is that with a low F# at 30, Bass (5) is Bass (4) with a
/// low B at 23 under its low E at 28. The test
/// `adding_a_string_grows_the_low_end_like_a_seven_string` asserts the result
/// is those presets note for note.
const ADDED_STRING_DROP: u8 = 5;

/// What a first string is tuned to when there is none to reason from.
///
/// Unreachable through the menu — the editor is always seeded from a live
/// tuning — but `strings` is a `Vec` and a caller may hand over an empty one.
/// A low E is the string every guitarist would expect to appear.
const DEFAULT_OPEN_PITCH: u8 = 40;

impl Dialog {
    /// Open the tuning editor on a copy of `from`.
    ///
    /// The name is prefilled with something that is NOT the preset's own name,
    /// because saving a custom tuning called "Standard" is the trap described
    /// on [`DialogAction::SetCustomTuning`] — the app would resolve that name
    /// back to the preset and the user's pitches would be gone. Editing a
    /// tuning that is already custom keeps its name, which is what "edit"
    /// means.
    pub fn custom_tuning(from: &Tuning, prefer_flats: bool) -> Dialog {
        Dialog::CustomTuning {
            name: if from.is_preset() {
                format!("{} (custom)", from.name)
            } else {
                from.name.to_string()
            },
            strings: rows_for(&from.open, prefer_flats),
            prefer_flats,
            focus: true,
        }
    }
}

/// Spell a MIDI pitch the way the rest of the app spells it, with an octave.
///
/// The pitch-class table is `tangent_core::patterns`, the same one the chord
/// strip, the teach layer and the theory panel render from — a second table
/// here would be a second spelling convention, and the day it disagreed the
/// fretboard would name a string Eb2 while the piano lit D#.
///
/// The octave is scientific pitch notation, MIDI 60 = C4, which is what makes
/// `TUNINGS`'s Standard read back as the E2 A2 D3 G3 B3 E4 written on every
/// guitar chart (asserted in the tests, because an off-by-one octave here would
/// be invisible until a guitarist read it).
pub fn pitch_name(pitch: u8, prefer_flats: bool) -> String {
    let names = if prefer_flats {
        NOTE_NAMES_FLAT
    } else {
        NOTE_NAMES
    };
    format!("{}{}", names[(pitch % 12) as usize], pitch as i32 / 12 - 1)
}

/// Read a pitch the way a guitarist writes one.
///
/// `Err` carries the hint shown under the field: a parser that can only say
/// "no" makes the user guess which of six things was wrong, and the six things
/// are cheap to tell apart here and impossible to tell apart later.
///
/// Deliberately more forgiving than [`pitch_name`] is expressive. Sharps and
/// flats are both accepted whichever the app prefers, case does not matter, and
/// a bare MIDI number is taken as one — nobody typing a tuning off a forum post
/// should have to notice which notation it was written in. One thing it does
/// NOT do is spell by convention across an octave boundary: `Cb2` is read as
/// "the pitch class of Cb, in octave 2" rather than as B1. No instrument is
/// tuned to Cb, and the flat table has no Cb in it at all, so inventing a rule
/// for it here would only create a spelling the rest of the app cannot produce.
pub fn parse_pitch(text: &str) -> Result<u8, &'static str> {
    let s = text.trim();
    if s.is_empty() {
        return Err("Type a note name like E2, or a MIDI number.");
    }
    // A bare number is a MIDI note number. Unambiguous, because no note name is
    // all digits.
    if s.bytes().all(|b| b.is_ascii_digit()) {
        return match s.parse::<u32>() {
            Ok(n) if n <= 127 => Ok(n as u8),
            _ => Err("A MIDI note number runs from 0 to 127."),
        };
    }
    // Uppercase the LETTER, one character, not the string. `parse_root` matches
    // 'A'..'G' in upper case only, so "e2" would be thrown out — but
    // `to_uppercase()` on the whole thing turns "Bb2" into "BB2", which parses
    // as a B with no accidental and then chokes on the octave. That is the
    // entire reason this is done by index.
    let head: String = s
        .char_indices()
        .map(|(i, c)| if i == 0 { c.to_ascii_uppercase() } else { c })
        .collect();
    // The letter and accidental come from the teach layer's own parser, so this
    // field accepts exactly the note names "Teach Chord Name" accepts.
    let Some((pc, rest)) = tangent_core::overrides::parse_root(&head) else {
        return Err("A pitch starts with a note letter A to G, like E2.");
    };
    if rest.is_empty() {
        return Err("A pitch needs an octave: E2, not E.");
    }
    if matches!(rest.as_bytes().first(), Some(b'#' | b'b')) {
        return Err("One accidental only: E#2 or Eb2.");
    }
    let Ok(octave) = rest.parse::<i32>() else {
        return Err("The octave has to be a number: E2, or E-1 at the very bottom.");
    };
    // The inverse of `pitch_name`'s octave, and tested as a round trip in both
    // directions over the whole MIDI range.
    let midi = (octave + 1) * 12 + pc as i32;
    if !(0..=127).contains(&midi) {
        return Err("That pitch is outside MIDI's 0 to 127.");
    }
    Ok(midi as u8)
}

/// The highest pitch core will accept as an open string.
///
/// ASKED, not copied. The ceiling is `MAX_OPEN_PITCH`, a private constant in
/// `fretboard.rs` that exists because a 24-fret neck adds two octaves and there
/// has to be MIDI left above the nut for them. A second copy of that number
/// here would be a number that stops agreeing the day the fret count changes,
/// and it would stop agreeing silently — the spinner would offer a pitch the
/// validator refuses. Probing costs 128 calls, once, behind a `OnceLock`.
fn highest_open_pitch() -> u8 {
    static CEILING: std::sync::OnceLock<u8> = std::sync::OnceLock::new();
    *CEILING.get_or_init(|| {
        (0..=127u8)
            .rev()
            .find(|p| Tuning::custom("probe", vec![*p]).is_ok())
            .unwrap_or(0)
    })
}

/// Add a string BELOW the lowest one, tuned a fourth down.
///
/// Which end grows is the difference between "add a string" doing the obvious
/// thing and the annoying one: a 7-string guitar is a 6-string with a low B
/// added *below*, not a seventh string stapled above the top E. See
/// [`ADDED_STRING_DROP`] for the evidence that this is the instruments' rule
/// and not a preference.
fn add_low_string(strings: &mut Vec<TuningString>, prefer_flats: bool) {
    if strings.len() >= MAX_STRINGS {
        // `Tuning::custom` would refuse it, and the solver's leaf count is why
        // the cap exists at all. The button is disabled here too; this is the
        // guard that makes the function safe to call anyway.
        return;
    }
    let pitch = strings
        .first()
        // Saturating, so the new string can never be tuned ABOVE the one it is
        // added under — that would be the monotone break the solver cannot
        // survive, introduced by the button meant to be safe.
        .map_or(DEFAULT_OPEN_PITCH, |low| {
            low.pitch.saturating_sub(ADDED_STRING_DROP)
        });
    strings.insert(0, TuningString::new(pitch, prefer_flats));
}

/// Take the lowest string away again: the exact inverse of [`add_low_string`].
///
/// It has to be the same end. Press Add then Remove and you must get back what
/// you had, and the presets shed from that end too — Bass (5) without its low B
/// is Bass (4), note for note, and 8-String without its low F# is 7-String.
fn remove_low_string(strings: &mut Vec<TuningString>) {
    // One string is a legal tuning (a diddley bow, or a work in progress);
    // none is not, and `Tuning::custom` refuses it.
    if strings.len() > 1 {
        strings.remove(0);
    }
}

/// A whole set of rows for a tuning's open strings, spelled and in order.
fn rows_for(open: &[u8], prefer_flats: bool) -> Vec<TuningString> {
    open.iter()
        .map(|&p| TuningString::new(p, prefer_flats))
        .collect()
}

/// Why the editor will not build a tuning yet, in a form that can be said out
/// loud.
///
/// `Debug` for the tests' sake: a failing assertion has to be able to print the
/// refusal it did not expect.
#[derive(Debug)]
enum Problem {
    /// A row's text is not a pitch. Carries the string's number counted 1-based
    /// from the LOW end — the numbering `TuningError` itself uses, so a message
    /// from here and a message from core point at the same row.
    Unreadable {
        string: usize,
        text: String,
        hint: &'static str,
    },
    /// The name is a preset's, with different pitches behind it.
    NameClash { preset: String },
    /// `Tuning::custom` refused it.
    Refused(TuningError),
}

impl Problem {
    /// One sentence of what is wrong, then why it matters.
    ///
    /// `NotAscending` and `PitchRange` are re-worded rather than deferred to
    /// `TuningError`'s own `Display`, for one reason each: core quotes raw MIDI
    /// numbers ("string 3 is 45"), which is the wrong language to answer
    /// somebody who is looking at a field that says A2 — and `NotAscending` is
    /// the one refusal whose reason is worth more than the fact. `StringCount`
    /// and `EmptyName` already say the whole thing and mention no pitches, so
    /// they are passed through as core writes them.
    fn message(&self, prefer_flats: bool) -> String {
        match self {
            Problem::Unreadable { string, text, hint } => {
                format!("String {string}: \"{text}\" is not a pitch. {hint}")
            }
            Problem::NameClash { preset } => format!(
                "\"{preset}\" is already a preset. Tangent saves a tuning by name and looks \
                 that name back up in the preset table when it starts, so this one would \
                 return as the preset and your pitches would be gone. Give it a name of its \
                 own."
            ),
            Problem::Refused(TuningError::NotAscending {
                string,
                previous,
                found,
            }) => format!(
                "String {} ({}) sounds below string {} ({}). Strings must run low to high, \
                 and that is not tidiness: the voicing solver searches by assuming each \
                 string is higher than the last, so a tuning that dips makes it stop finding \
                 shapes your instrument can really play.",
                string + 1,
                pitch_name(*found, prefer_flats),
                string,
                pitch_name(*previous, prefer_flats),
            ),
            Problem::Refused(TuningError::PitchRange(p)) => format!(
                "{} is too high for an open string: a 24-fret neck reaches two octaves above \
                 it and there has to be MIDI left for them. {} is as high as an open string \
                 goes.",
                pitch_name(*p, prefer_flats),
                pitch_name(highest_open_pitch(), prefer_flats),
            ),
            Problem::Refused(e) => e.to_string(),
        }
    }
}

/// Build the tuning the editor is currently describing, or say why not.
///
/// **The single gate.** The confirm button is enabled on `is_ok()` and the
/// action is built from the `Ok` value, so "you cannot confirm an invalid
/// tuning" is one function rather than two conditions that can drift apart.
///
/// It reads `text`, never `pitch`: the field is what the user is looking at,
/// and a tuning built from a number they can no longer see would be a tuning
/// they did not ask for.
fn confirm(name: &str, strings: &[TuningString]) -> Result<Tuning, Problem> {
    let mut open = Vec::with_capacity(strings.len());
    for (i, row) in strings.iter().enumerate() {
        match parse_pitch(&row.text) {
            Ok(p) => open.push(p),
            Err(hint) => {
                return Err(Problem::Unreadable {
                    string: i + 1,
                    text: row.text.clone(),
                    hint,
                })
            }
        }
    }
    // Checked here rather than in core, because it is not a fact about tunings:
    // it is a fact about how THIS app stores one. See
    // `DialogAction::SetCustomTuning`. Identical pitches under a preset's name
    // are harmless — that is simply the preset — so only a real collision is
    // refused.
    if let Some(preset) = Tuning::by_name(name.trim()) {
        if preset.open.as_ref() != open.as_slice() {
            return Err(Problem::NameClash {
                preset: preset.name.to_string(),
            });
        }
    }
    Tuning::custom(name, open).map_err(Problem::Refused)
}

// ── the export dialog ───────────────────────────────────────────────────────
//
// The owner asked for "a dialog menu with a selector that can include: Camera +
// Synced sound + Tangent's Display all in one video ALONG with midi in the
// export", with the other combinations available too. So it is a real selector
// over a real spec (`recorder::ExportSpec`), and every rule about what is a
// legal spec lives THERE and is already tested. This file renders it and
// refuses to commit an invalid one; it does not get a second opinion about
// what "valid" means.
//
// It is reached from two places and they mean different things. From the
// Recorder band before a take, it chooses what that take will produce. From the
// result strip after a take, it re-runs the subset that is genuinely
// re-derivable — and that subset is much smaller than it looks, which is what
// `Rederivable` is for.

/// Whether the video encoder exists yet.
///
/// **It does, on macOS.** The compositor renders an offscreen egui pass into a
/// wgpu texture and AVAssetWriter turns it into H.264 with AAC beside it; the
/// whole path is covered by `the_whole_pipeline_writes_a_vertical_video_with_
/// sound`, which produces a file and has `ffprobe` check it.
///
/// It was a constant so that turning the section on would be ONE edit rather
/// than eleven scattered conditions, and that is how it turned out — this line
/// is the edit.
///
/// **True everywhere since 5.0.** Windows and Linux encode through `ffmpeg`
/// (see `tangent_record::encode::ffmpeg`) and the compositor was never
/// platform-specific: it is wgpu and egui, the same two things that draw the
/// window. What is left is a runtime question rather than a build-time one, so
/// it is answered at the moment a take starts: if ffmpeg cannot be found, the
/// take says so, names the install command, and still writes its audio and its
/// MIDI. A dialog that greys the controls cannot say any of that.
pub const VIDEO_EXPORT_READY: bool = true;

/// What the Welcome card says.
///
/// A `const` rather than an inline array so the fit test can measure the real
/// strings: a card whose text overflows its own window is a first impression
/// nobody recovers from, and it is exactly the sort of thing that breaks when
/// somebody adds a line.
const WELCOME_LINES: [&str; 20] = [
    "Welcome to the Tangent 5.0 beta!",
    "",
    "Tangent is an attempt to create a seamless",
    "environment between learning/synthesizing",
    "musical information, and creating as soon",
    "as the inspiration strikes. Start off",
    "exploring with TAB, which cycles between",
    "three views, or press 'H' for the full",
    "keybind list, which differs per-view.",
    "Anything that is extra lives in the",
    "right-click menu.",
    "",
    "It may seem like nothing more than eye-candy",
    "at first, but the substance is there, and I",
    "want this to evolve into a home for those",
    "who want to do it all in one place.",
    "",
    "More to come,",
    "",
    "Ganten",
];

/// The Audio Status window. Two device blocks, a round-trip line, and a row of
/// buffer sizes — measured by `the_audio_status_card_fits_its_own_text`.
const AUDIO_W: f32 = 430.0;

/// The most characters of a DEVICE MAKER the audio panel will print.
///
/// Every other string this panel draws is one it wrote itself, so
/// `the_audio_status_card_fits_its_own_text` can measure them all and prove
/// they fit. A manufacturer name is the exception: it comes from whatever
/// driver is installed, and no measurement of strings we know about says
/// anything about one we do not. Clipping is what turns it back into a
/// string the panel controls.
const MAKER_MAX: usize = 46;

/// `s`, cut to `max` characters with an ellipsis when it was longer.
///
/// Counted in CHARACTERS and not bytes: a maker name with an accent in it
/// would panic a byte slice, and the one string here that comes from outside
/// is exactly the one that might have one.
fn clipped(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_owned();
    }
    s.chars().take(max.saturating_sub(1)).collect::<String>() + "\u{2026}"
}
/// The channel chooser. Wide enough for eight buttons on a row, so a
/// sixteen-in interface is two rows and not five.
const CHANNELS_W: f32 = 460.0;

/// As tall as the chooser is ever allowed to get. Past this the two columns
/// scroll: a thirty-two-in interface would otherwise ask for a dialog taller
/// than the screen, and a dialog that runs off the bottom has an OK button
/// nobody can reach.
const CHANNELS_MAX_H: f32 = 560.0;
/// Taller since the panel became Setup: four choice blocks and their captions
/// where there was one, and now the interface's short name as well. The body
/// scrolls, so this is the size at which nothing HAS to — a machine whose
/// interface offers all six rates is the tallest case, and it fits.
///
/// **Grown with the block that was added, not left to the scroll.** A field
/// whose caption is under the Close button is a field somebody has to discover
/// by dragging, and the caption is the half that says what the field is for.
const AUDIO_H: f32 = 580.0;

/// The Welcome card's window, and the size it opens at.
///
/// **With real slack, not seven points of it.** The measurement below came to
/// 293 against a height of 300 and the card was still being cut off in front
/// of users: a test that measures labels and a constant that has to survive a
/// checkbox, a button and a theme's own spacing are not the same arithmetic,
/// and the difference has to live in the constant rather than in hope.
/// `the_welcome_card_fits_its_own_text` now demands [`WELCOME_SLACK`] on top of
/// what it can measure.
const WELCOME_W: f32 = 470.0;
const WELCOME_H: f32 = 486.0;
/// How much more than the measurable text the Welcome card must have.
///
/// The chrome this test cannot see: a checkbox's real height against the 22 it
/// is assumed to be, a button's real height against 30, and whatever
/// `apply_theme` does to spacing. It is the first thing anybody sees, so it
/// gets a margin rather than a squeeze.
const WELCOME_SLACK: f32 = 60.0;

/// The dialog as a window. Wide enough for the left column and the longest
/// radio label on one line, tall enough that the §5 mock needs no scrolling
/// before a take. After one it does, because the reason the camera controls are
/// dead is worth several lines and the body scrolls rather than the buttons
/// moving.
const EXPORT_DIALOG_W: f32 = 560.0;
const EXPORT_DIALOG_H: f32 = 620.0;
/// The `Frame` margin on every side; see `TUNING_MARGIN` for why it is named.
const EXPORT_MARGIN: i8 = 12;
/// The left column. Every control in the dialog starts at the same x whichever
/// section it is in, which is what makes the §5 mock read as one form rather
/// than four stacked ones.
const EXPORT_LABEL_W: f32 = 148.0;
/// How far a sub-section's label ("Layout", "Display shows") sits in from a
/// section's ("Video"). The mock's indentation, in points.
const EXPORT_SUB_INDENT: f32 = 12.0;
/// Point size of the footer — the estimate and the refusal.
const EXPORT_FOOTER_PT: f32 = 10.0;
/// Height reserved for that footer, whether or not there is a refusal in it.
///
/// Reserved rather than measured, for the reason spelled out on
/// `TUNING_FOOTER_H`: a band that grows as you click walks the button row up
/// and down under the cursor, and a band reserved too small pushes Export off
/// the bottom edge where only Esc can reach it.
/// `the_longest_export_refusal_fits_the_space_reserved_for_it` measures the
/// real messages against this rather than trusting the arithmetic.
const EXPORT_FOOTER_H: f32 = 46.0;

/// What a take can still be turned into after it has been recorded.
///
/// **This is the one rule in the dialog that carries real information.** Camera
/// frames are composited and encoded *while the take runs* — §0 rules out
/// keeping them (one 20-minute 1080p30 take is 112 GB as raw NV12) and there is
/// no decoder anywhere in this design, so nothing can read the finished file
/// back. Therefore:
///
/// | after Stop | re-derivable? | why |
/// |---|---|---|
/// | the SMF tempo mark | yes | rewriting the tempo meta is a file edit |
/// | a display-only video, any panel selection | yes | replay the recorded `.mid` offline through the compositor; the four `draw` functions are pure painters with no time dependence |
/// | anything containing the camera | **no** | those frames were composited and encoded live |
///
/// It is a value rather than a handful of `if post_take &&` scattered through
/// the rendering code because getting it wrong offers the user an option that
/// cannot work, which is worse than not offering it at all — and because a rule
/// with one home can be tested, which is what
/// `a_take_recorded_without_a_camera_can_never_gain_one` does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rederivable {
    /// The camera tick, the video modes that imply a camera, and "Match
    /// camera" — the resolution of a camera that is no longer running.
    pub camera: bool,
    /// The composite layout. Fixed after a take even when the take HAD a
    /// camera: those frames were composited in that arrangement already, so
    /// "Camera full frame" can never become "Side by side".
    pub layout: bool,
    /// The display layer and which panels are in it. Always true: this is the
    /// half that genuinely survives, because it is repainted from the MIDI.
    pub display: bool,
    /// The SMF tempo mark. Always true: it is a file edit.
    pub tempo: bool,
    /// Why something is greyed, in words the dialog prints verbatim. `None`
    /// before a take, when nothing is greyed and there is nothing to explain.
    pub why: Option<&'static str>,
}

impl Rederivable {
    /// Before a take, nothing has happened yet, so nothing is fixed.
    pub const EVERYTHING: Rederivable = Rederivable {
        camera: true,
        layout: true,
        display: true,
        tempo: true,
        why: None,
    };

    /// Bring a spec down to what this take can actually still produce.
    ///
    /// Applied once, when the dialog opens, so it can never OPEN showing a
    /// camera video it cannot make — the seed comes from the settings file or
    /// from the last take, and neither of them knows what this take caught.
    /// After that the user's own ticks stand: a spec they break themselves is
    /// refused with a reason, not silently re-ticked.
    pub fn clamp(self, mut spec: ExportSpec) -> ExportSpec {
        if !self.camera {
            spec.composite.camera = false;
            if spec.video.wants_video() {
                // With the camera layer gone, every multi-file mode collapses
                // to the one video that is still re-derivable: the display,
                // replayed offline from the recorded `.mid`. And if the
                // display is not in it either, there is no video left to make
                // at all — offering an empty composite would be offering the
                // option that cannot work.
                spec.video = if spec.composite.display && spec.composite.shows.any() {
                    VideoMode::Composite
                } else {
                    VideoMode::None
                };
            }
        }
        spec
    }
}

/// The rule, in one place. See [`Rederivable`].
pub fn rederivable(post_take: bool, had_camera: bool) -> Rederivable {
    if !post_take {
        return Rederivable::EVERYTHING;
    }
    Rederivable {
        // False either way, and the reason differs: a take with no camera has
        // no frames to composite, and a take WITH one has frames that were
        // already composited and encoded. Neither can be re-made.
        camera: false,
        layout: false,
        display: true,
        tempo: true,
        why: Some(if had_camera {
            "This take's camera was composited and encoded as it ran, in the layout chosen \
             then, and the frames were never kept - Tangent has no decoder to read them back. \
             So the camera and the layout are settled now. The tempo mark, and a display-only \
             video in any panel selection, can still be re-made from the recorded MIDI."
        } else {
            "This take was recorded without the camera, and it can never gain one: camera \
             frames are composited and encoded while the take runs and nothing keeps them. \
             The tempo mark, and a display-only video in any panel selection, can still be \
             re-made from the recorded MIDI."
        }),
    }
}

/// Seed "Display shows" from the panels the user actually has on screen.
///
/// The video's panel selection then **overrides the live settings, for the
/// video only** — that is the whole reason `DisplayShows` exists as its own set
/// of flags rather than the dialog reading `Settings` at render time. Recording
/// a clean piano-and-chord video while keeping the fretboard on screen for your
/// own use is the case it is for, and because the compositor drives the same
/// `draw` functions with its own `Settings` copy, it costs a struct field
/// rather than a code path.
pub fn shows_from_settings(s: &crate::settings::Settings) -> DisplayShows {
    DisplayShows {
        // No flags to read: the piano and the chord readout are never off.
        // They are the app.
        piano: true,
        chord: true,
        fretboard: s.show_fretboard,
        // One tick for three flags, because the theory band is one panel on
        // screen however many diagrams are inside it. Seeding from
        // `theory_circle` alone would silently drop somebody who has only the
        // Tonnetz showing — hence `Views::any`, which is the same question the
        // band itself asks.
        theory: s.theory_views().any(),
    }
}

/// Read a tempo mark the way somebody types one.
///
/// Refuses out of range rather than clamping, and that is the point:
/// `MIN_BPM..=MAX_BPM` exists because a DAW will happily import 0.001 BPM and
/// draw a bar ruler several centuries long, and a field that quietly turned a
/// typed 400 into 300 would be a field the user did not notice lying to them.
fn parse_bpm(text: &str) -> Result<f64, String> {
    let s = text.trim();
    if s.is_empty() {
        return Err("The tempo mark is a number of beats per minute, like 120.".to_owned());
    }
    let Ok(v) = s.parse::<f64>() else {
        return Err(format!("\"{s}\" is not a tempo. Type a number, like 120."));
    };
    if !v.is_finite() || !(MIN_BPM..=MAX_BPM).contains(&v) {
        return Err(format!(
            "A tempo mark runs from {MIN_BPM:.0} to {MAX_BPM:.0} BPM. It does not move a single \
             note - it decides where a DAW's bar lines land."
        ));
    }
    Ok(v)
}

/// A tempo for the field: `120`, not `120.0`, and `92.5` when it really is.
fn bpm_text(bpm: f64) -> String {
    if bpm.fract() == 0.0 {
        format!("{bpm:.0}")
    } else {
        format!("{bpm}")
    }
}

/// Build the spec the dialog is currently describing, or say why not.
///
/// **The single gate**, exactly as `confirm` is for the tuning editor: the
/// Export button is enabled on `is_ok()` and the action carries the `Ok` value,
/// so "you cannot export an invalid spec" is one function rather than two
/// conditions that drift apart.
///
/// The tempo comes from the TEXT, never from `spec.tempo_bpm`, because the text
/// is what the user is looking at. `problem()` is asked of the spec that would
/// actually be written, and its own words are used verbatim — a second wording
/// here would be a second answer to the same question.
fn export_ready(spec: &ExportSpec, tempo_text: &str) -> Result<ExportSpec, String> {
    // A tempo mark is written into the SMF and nowhere else, so with no MIDI
    // there is nothing to validate. Checking anyway would grey Export out over
    // a stale value in a field that is itself greyed, which is a dead end with
    // no way for the user to see what it wants.
    let tempo_bpm = if spec.midi {
        parse_bpm(tempo_text)?
    } else {
        spec.tempo_bpm
    };
    let out = ExportSpec { tempo_bpm, ..*spec };
    match out.problem() {
        Some(p) => Err(p.message().to_owned()),
        None => Ok(out),
    }
}

/// Which action a commit is. One function, so the tick and the consequence
/// cannot drift apart.
fn export_action(spec: ExportSpec, remember: bool) -> DialogAction {
    if remember {
        DialogAction::SetExportAndRemember(spec)
    } else {
        DialogAction::SetExport(spec)
    }
}

/// The footer's first line: what this take costs, in disk and in CPU.
///
/// A rate and an encoder count rather than a byte total, because the two
/// questions somebody actually has are "will this fill my disk" and "will this
/// stutter while I play". `megabytes_per_minute` and `encoder_count` are the
/// recorder's own arithmetic; nothing is recomputed here.
fn estimate_line(spec: &ExportSpec) -> String {
    let mb = spec.megabytes_per_minute();
    let encoders = match spec.encoder_count() {
        0 => "no video encoder".to_owned(),
        1 => "1 video encoder".to_owned(),
        n => format!("{n} video encoders"),
    };
    format!(
        "~{mb:.0} MB a minute, about {:.1} GB an hour, {encoders}.",
        mb * 60.0 / 1000.0
    )
}

/// What those encoders cost on the machine this is running on.
///
/// Three simultaneous 1080p30 encodes is nothing for VideoToolbox and is
/// genuinely expensive for a software encoder, and somebody about to run
/// three of them deserves to know which one they have *before* they find out
/// during a take.
///
/// **Windows is the SOFTWARE case, and used to be told the opposite.** This
/// said "runs on Media Foundation, so three 1080p30 streams cost the CPU
/// almost nothing" — and Windows does not encode through Media Foundation at
/// all. `encode.rs` is explicit: macOS encodes natively through AVFoundation,
/// and "Windows and Linux pipe raw frames into `ffmpeg`", which
/// `encode/ffmpeg.rs` invokes as `-c:v libx264 -preset veryfast`. That is
/// x264 on the CPU. So the one platform being promised it was free was the
/// platform that pays most, and the sentence it needed was the one directly
/// below it.
///
/// `cfg!` and not a probe: this asks about the TARGET, which is where the
/// dialog will be read, so a Windows build cross-compiled on this Mac still
/// answers for Windows.
fn encoder_note() -> &'static str {
    if cfg!(target_os = "macos") {
        "Video encoding here runs on VideoToolbox, so three 1080p30 streams cost the CPU almost \
         nothing."
    } else {
        "Video encoding here is a software encoder: every stream is real CPU, and three 1080p30 \
         streams will be felt while you play."
    }
}

/// Said out loud under the video modes while [`VIDEO_EXPORT_READY`] is false.
const VIDEO_PENDING_NOTE: &str =
    "Video export is not written yet - it arrives with the encoder. This take will write the \
     audio and the MIDI. The controls below are the finished design and are greyed until then.";

/// The dialog's left column: a label of a fixed width.
///
/// The space is ALLOCATED and the text painted into it, rather than laid out
/// from the label's own size, because egui advances a sub-layout by what the
/// child actually used — so "Video" would pull its row's controls left and the
/// straight column the mock draws would bend.
fn export_label(ui: &mut egui::Ui, t: &DialogTheme, text: &str, sub: bool) {
    let h = ui.spacing().interact_size.y;
    let (rect, _) = ui.allocate_exact_size(Vec2::new(EXPORT_LABEL_W, h), egui::Sense::hover());
    if text.is_empty() {
        return;
    }
    let indent = if sub { EXPORT_SUB_INDENT } else { 0.0 };
    ui.painter().text(
        rect.left_center() + Vec2::new(indent, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::new(if sub { 10.0 } else { 11.0 }, fonts::courier_bold()),
        if sub {
            t.text.gamma_multiply(0.72)
        } else {
            t.text
        },
    );
}

/// One themed checkbox that may be greyed.
///
/// `add_enabled` fades the whole widget through the painter, which is what
/// makes a disabled control LOOK disabled rather than merely ignore the click —
/// and a control that ignores clicks silently is how a user concludes the app
/// is broken.
fn export_check(ui: &mut egui::Ui, t: &DialogTheme, enabled: bool, on: &mut bool, label: &str) {
    let text = RichText::new(label)
        .font(FontId::new(11.0, fonts::courier_bold()))
        .color(t.text);
    ui.add_enabled(enabled, egui::Checkbox::new(on, text));
}

/// One themed radio button that may be greyed. Sets `current` when chosen.
fn export_radio<T: PartialEq>(
    ui: &mut egui::Ui,
    t: &DialogTheme,
    enabled: bool,
    current: &mut T,
    value: T,
    label: &str,
) {
    let text = RichText::new(label)
        .font(FontId::new(11.0, fonts::courier_bold()))
        .color(t.text);
    if ui
        .add_enabled(enabled, egui::RadioButton::new(*current == value, text))
        .clicked()
    {
        *current = value;
    }
}

/// A wrapped explanatory paragraph, full width. Full width and not in the label
/// column, because a horizontal layout has infinite width and a long line in
/// one would run off the edge of the dialog instead of wrapping.
fn export_para(ui: &mut egui::Ui, t: &DialogTheme, text: &str) {
    ui.add_space(3.0);
    ui.label(
        RichText::new(text)
            .font(FontId::new(9.0, fonts::courier_bold()))
            .color(t.text.gamma_multiply(0.72)),
    );
    ui.add_space(3.0);
}

impl Dialog {
    /// Open the Export dialog on `spec`.
    ///
    /// `post_take` is the difference between the dialog's two entry points and
    /// they mean different things: from the Recorder band's `Export...` button
    /// it chooses what the NEXT take produces, and from the result strip after
    /// a take it re-runs what is genuinely re-derivable from the one just
    /// finished. `had_camera` is what that finished take actually caught.
    ///
    /// The spec is [`Rederivable::clamp`]ed on the way in, so a dialog opened
    /// after a take can never OPEN promising a camera video that cannot exist —
    /// the seed comes from the settings file or the last take, and neither of
    /// them knows what this take caught.
    ///
    /// The app opens it like this:
    ///
    /// ```ignore
    /// let mut spec = /* the remembered ExportSpec, or ExportSpec::default() */;
    /// spec.composite.shows = dialogs::shows_from_settings(&self.settings);
    /// self.dialog = Some(Dialog::export(spec, false, false));   // before a take
    /// self.dialog = Some(Dialog::export(spec, true, take.had_camera)); // after one
    /// ```
    ///
    /// and drains `DialogAction::SetExport` / `SetExportAndRemember`, the
    /// second of which also writes the spec to settings.
    pub fn export(spec: ExportSpec, post_take: bool, had_camera: bool) -> Dialog {
        let mut spec = rederivable(post_take, had_camera).clamp(spec);
        if !VIDEO_EXPORT_READY {
            // Honesty, in the same edit as the greying: while there is no
            // encoder, a remembered `video` from the settings file would
            // otherwise be committed as a promise of a file that will never
            // appear. Delete these three lines when the constant flips.
            spec.video = VideoMode::None;
        }
        Dialog::Export {
            tempo_text: bpm_text(spec.tempo_bpm),
            spec,
            remember: false,
            post_take,
            had_camera,
        }
    }
}

/// One row in the in-app file browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileEntry {
    /// What to show. The file name, or `..` for the way up.
    pub name: String,
    pub path: std::path::PathBuf,
    pub is_dir: bool,
}

/// What the in-app browser is being opened for, so its answer can be routed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BrowseFor {
    File(crate::ports::FilePurpose),
    Folder(crate::ports::DirPurpose),
}

/// Which kind of device a [`Dialog::DevicePicker`] is choosing.
///
/// Carries its own copy through the action so the app does not have to remember
/// which picker it opened — the answer arrives saying what question it answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Camera,
    AudioInput,
    /// One of the inputs open BESIDE the first, numbered from 1.
    ///
    /// **The same list, aimed somewhere else.** A second microphone is not a
    /// different kind of thing from the first — it is the same picker filling
    /// a different strip, which is why this carries the strip and not a copy
    /// of everything the audio picker knows.
    ExtraInput(usize),
}

impl DeviceKind {
    pub fn title(self) -> &'static str {
        match self {
            DeviceKind::Camera => "Select Camera",
            DeviceKind::AudioInput | DeviceKind::ExtraInput(_) => "Select Audio Input",
        }
    }

    fn prompt(self) -> &'static str {
        match self {
            DeviceKind::Camera => "Camera to record and preview:",
            DeviceKind::AudioInput => "Audio input to record:",
            // The channels of the interface that is already open, because one
            // interface is one clock — see `Selection::extra`.
            DeviceKind::ExtraInput(_) => "A second input of the same interface:",
        }
    }

    /// What the None row says. Not just "None": what choosing it DOES is the
    /// part worth spelling out, because a recorder with no camera is a normal
    /// thing to want and a recorder with no audio input used to be a mistake.
    ///
    /// It no longer is, and this row used to say so — "record MIDI only" was
    /// true when the input device was the only thing that could drive a writer,
    /// so a loaded instrument went unrecorded and the user was told to go and
    /// choose a microphone in order to record something the microphone was not
    /// going to record. A take with no input now records the instrument on the
    /// output device's own clock.
    fn none_row(self) -> &'static str {
        match self {
            DeviceKind::Camera => "None  -  record without video",
            DeviceKind::AudioInput => "None  -  record the instrument, not an input",
            DeviceKind::ExtraInput(_) => "None  -  leave this channel empty",
        }
    }

    /// Per-platform, because the advice is: "System Settings > Privacy &
    /// Security" on a Linux box sends the user hunting for a panel that does
    /// not exist, when the actual fix is group membership on the device node.
    ///
    /// **On Windows there is no camera backend at all** (`tangent-record`'s
    /// `camera::stub`), so the list is empty on every machine, working webcam
    /// or not. A tester opened the picker, found it empty, and was told to
    /// check a System Settings panel that does not exist on their operating
    /// system — which reads as "the app is broken" rather than "this is not
    /// built yet". An empty list needs a reason, and the reason has to be the
    /// true one.
    #[cfg(target_os = "windows")]
    fn nothing_found(self) -> &'static str {
        match self {
            DeviceKind::Camera => {
                "No camera found. Check that a webcam is connected and that \
                 Camera access is on under Settings > Privacy & security > \
                 Camera (Tangent uses the system camera through its own \
                 bundled encoder)."
            }
            DeviceKind::AudioInput => {
                "No audio inputs found. If an interface is connected, check \
                 that it appears under Settings > System > Sound > Input."
            }
            DeviceKind::ExtraInput(_) => {
                "No other inputs on this interface. Tick the ones you want in \
                 Setup, under the device."
            }
        }
    }

    #[cfg(target_os = "linux")]
    fn nothing_found(self) -> &'static str {
        match self {
            DeviceKind::Camera => {
                "No cameras found. If one is connected, your user may not be \
                 able to read /dev/video* - add yourself to the 'video' \
                 group, then log out and back in."
            }
            DeviceKind::AudioInput => {
                "No audio inputs found. If an interface is connected, check \
                 that it shows up in your sound settings (or `arecord -l`)."
            }
            DeviceKind::ExtraInput(_) => {
                "No other inputs on this interface. Tick the ones you want in \
                 Setup, under the device."
            }
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    fn nothing_found(self) -> &'static str {
        match self {
            DeviceKind::Camera => {
                "No cameras found. If one is connected, Tangent may not have \
                 been granted camera access yet - check System Settings > \
                 Privacy & Security."
            }
            DeviceKind::AudioInput => {
                "No audio inputs found. If an interface is connected, Tangent \
                 may not have been granted microphone access yet - check \
                 System Settings > Privacy & Security."
            }
            DeviceKind::ExtraInput(_) => {
                "No other inputs on this interface. Tick the ones you want in \
                 Setup, under the device."
            }
        }
    }
}

// ── the plugin picker ───────────────────────────────────────────────────────
//
// There are 112 VST3 bundles installed on the owner's machine, and the one
// decision this dialog makes is that it will not open a single one of them.
//
// A picker showing each plugin's REAL name has to ask each plugin, and asking
// means `Module::open` — which on macOS runs the bundle's `bundleEntry`, where
// plugins locate sample libraries, read preset folders and spawn threads (see
// the header of `tangent-host/src/scan.rs`). A hundred of those in a row is
// seconds of a frozen window, and it is precisely where a broken plugin takes
// the process down: before the user has chosen anything at all, from a dialog
// they opened to look. So the rows come from `tangent_host::discover()`, which is
// a `read_dir` and nothing more — paths and file names.
//
// What that costs is honest and small: the vendor column is usually empty, and
// the dialog says why rather than leaving it looking like missing data. One
// module is opened, at the moment one is loaded.
//
// Nothing here takes a rescan, a refresh or a "read them all properly" flag. An
// API that invited scanning would eventually be taken up.

/// The picker as a window. As wide as the Export dialog so the two notes at the
/// bottom wrap the same way, and tall enough that a filtered list is worth
/// looking at.
const PLUGIN_DIALOG_W: f32 = 560.0;
const PLUGIN_DIALOG_H: f32 = 460.0;
/// The `Frame` margin on every side; see `TUNING_MARGIN` for why it is named.
const PLUGIN_MARGIN: i8 = 12;
/// Point size of the two notes under the list.
const PLUGIN_NOTE_PT: f32 = 9.0;
/// Height reserved for those notes, whether or not the list is empty.
///
/// Reserved rather than measured, for the reason spelled out on
/// `TUNING_FOOTER_H`: a band that changed size would walk the button row under
/// the cursor, and a band reserved too small pushes Load off the bottom edge
/// where only Esc can reach it. The test
/// `the_notes_under_the_list_fit_the_space_reserved_for_them` measures the real
/// strings against this rather than trusting the arithmetic.
const PLUGIN_FOOTER_H: f32 = 60.0;

/// What the None row says. Not just "None": what choosing it DOES is the part
/// worth spelling out, exactly as on [`DeviceKind::none_row`].
const PLUGIN_NONE_ROW: &str = "None  -  no instrument; unloads the one that is loaded";

/// The path that means "the instrument Tangent ships with".
///
/// **Not a real path, on purpose.** It travels through every layer that
/// already carries a plugin's bundle path — the picker's selection, the slot's
/// setting, the saved session — without any of them needing to know there is
/// such a thing as a built-in. The one place that looks at it is where a slot
/// is actually loaded.
///
/// It begins with a character no filesystem path does, so it can never collide
/// with a bundle somebody has installed.
pub const BUILTIN_PATH: &str = "@tangent-dx7";

/// What the built-in calls itself in the picker.
pub const BUILTIN_NAME: &str = "Tangent DX7";
/// The drumpad's sentinel path: the sixteen-pad sampler as a bay-1
/// instrument. Like [`BUILTIN_PATH`] it rides the bay so it can be picked,
/// saved and named by the same code, but the ENGINE strikes the pads in
/// `mix_channels` — the bay itself is handed nothing.
pub const DRUMPAD_PATH: &str = "@tangent-drums";
pub const DRUMPAD_NAME: &str = "Tangent Drums";

/// The modelled keyboard's sentinel: a piano and an electric piano that are
/// computed rather than sampled. Rides a bay exactly as the DX7 does, and for
/// the same reason — every layer between the picker and the saved session
/// carries it as a path and none of them has to learn what it is.
pub const KEYS_PATH: &str = "@tangent-keys";
/// **The DISPLAY name only.** The preset folder is spelled separately, in
/// `desktop::preset_subfolder`, and deliberately still reads "Tangent Keys":
/// a saved preset lives at a path, and renaming the drawer would hide every
/// preset anybody has already written. The parenthetical is a label on the
/// instrument, not a change of identity.
pub const KEYS_NAME: &str = "Tangent Keys (Experimental)";

/// How many patches the modelled keyboard ships, and what they are called.
///
/// **The names live here rather than only in the engine** because the picker,
/// the settings clamp and the strip label all need them, and none of those can
/// see the audio crate. The engine's `keys::Patch::ALL` is the same list in the
/// same order; the index is the contract between them, which is why the
/// settings key stores a number.
pub const KEYS_PATCHES: usize = 8;
/// APPENDED, never reordered: the first two indices were in settings files
/// for months before the other six existed, and the index IS the contract.
pub const KEYS_PATCH_NAMES: [(&str, &str); KEYS_PATCHES] = [
    ("Piano 1", "jazz grand, voiced for solo playing"),
    (
        "EP Mk.1",
        "tine electric piano, dry - light is bell, hard is bark",
    ),
    ("Piano 2", "concert grand: longer ring, sweeter top, wider swing"),
    (
        "EP Suitcase",
        "the same tines through their own amp - the stereo throb",
    ),
    ("Clav", "struck-and-held strings, a sliding pickup, taken dry"),
    ("Clav Wah", "the same strings through a filter that chases your touch"),
    ("Reed 200", "reed piano, transistor amp: hollow soft, growling hard"),
    ("Reed 140", "the tube reed piano: darker, and never entirely clean"),
];

/// How many knobs a keyboard sound exposes, and what they are.
///
/// **The mirror of `keys::ParamSpec`, for the same reason the names above are
/// a mirror**: the panel, the settings clamp and the reset button all need
/// this table and none of them can see the audio crate. A test in that crate
/// pins the two tables against each other name for name and default for
/// default, so a knob added there and forgotten here fails a build rather
/// than shipping a panel that quietly cannot reach half the instrument.
///
/// `(name, what it does, where it sits when the sound is initialised)`.
pub const KEYS_PARAMS: usize = 8;
pub const KEYS_PARAM_SPECS: [[(&str, &str, f32); KEYS_PARAMS]; KEYS_PATCHES] = [
    [
        ("Voicing", "hammer felt: needled soft to lacquered bright", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long a struck string rings", 0.5),
        (
            "Brilliance",
            "how long the upper partials outlast the fundamental",
            0.5,
        ),
        (
            "Unison",
            "detune between the strings of a course: the beating",
            0.5,
        ),
        ("Stretch", "string stiffness, which pulls the partials sharp", 0.5),
        ("Release", "how fast the damper stops a string", 0.5),
        ("Soundboard", "the body under everything, and the pedalled wash", 0.5),
    ],
    [
        ("Voicing", "hammer tip: soft to hard against the tine", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long the tine rings", 0.5),
        ("Bell", "the glassy chime on the attack", 0.5),
        ("Bark", "how hard the tine leans on the pickup", 0.5),
        ("Chime", "where the bell partial sits above the note", 0.5),
        ("Release", "how fast the damper stops the tine", 0.5),
        ("Tonebar", "the bar the tine is bolted to: its ring and the slow beat", 0.5),
    ],
    [
        ("Voicing", "hammer felt: needled soft to lacquered bright", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long a struck string rings", 0.5),
        (
            "Brilliance",
            "how long the upper partials outlast the fundamental",
            0.5,
        ),
        (
            "Unison",
            "detune between the strings of a course: the beating",
            0.5,
        ),
        ("Stretch", "string stiffness, which pulls the partials sharp", 0.5),
        ("Release", "how fast the damper stops a string", 0.5),
        ("Soundboard", "the body under everything, and the pedalled wash", 0.5),
    ],
    [
        ("Voicing", "hammer tip: soft to hard against the tine", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long the tine rings", 0.5),
        ("Bell", "the glassy chime on the attack", 0.5),
        ("Bark", "how hard the tine leans on the pickup", 0.5),
        ("Tremolo", "the stereo throb of the twin speakers", 0.45),
        ("Release", "how fast the damper stops the tine", 0.5),
        ("Tonebar", "the bar the tine is bolted to: its ring and the slow beat", 0.5),
    ],
    [
        ("Voicing", "tangent tip: worn soft to fresh hard", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long a held string rings", 0.5),
        ("Pickup", "where the coil sits under the string", 0.35),
        ("Tone", "the filter rockers, folded to one sweep", 0.75),
        ("Snap", "the spit of the string when the key lifts", 0.5),
        ("Release", "how fast the yarn chokes the string", 0.5),
        ("Mute", "the sliding damper pad: funk at the top", 0.0),
    ],
    [
        ("Voicing", "tangent tip: worn soft to fresh hard", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long a held string rings", 0.5),
        ("Pickup", "where the coil sits under the string", 0.35),
        ("Wah", "how hard the filter chases your touch", 0.55),
        ("Snap", "the spit of the string when the key lifts", 0.5),
        ("Release", "how fast the yarn chokes the string", 0.5),
        ("Mute", "the sliding damper pad: funk at the top", 0.0),
    ],
    [
        ("Voicing", "hammer felt: worn soft to packed hard", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long the reed rings", 0.5),
        ("Reed", "how much of the bar's own metal speaks", 0.5),
        ("Growl", "how hard the reed leans into the plate", 0.5),
        ("Tremolo", "the amp's throb", 0.4),
        ("Release", "how fast the damper stops the reed", 0.5),
        ("Thump", "the hammer's knock against the reed", 0.5),
    ],
    [
        ("Voicing", "hammer felt: worn soft to packed hard", 0.5),
        ("Width", "how far bass and treble sit apart", 1.0 / 1.5),
        ("Decay", "how long the reed rings", 0.5),
        ("Reed", "how much of the bar's own metal speaks", 0.5),
        ("Growl", "how hard the reed leans into the plate", 0.62),
        ("Tremolo", "the amp's throb", 0.0),
        ("Release", "how fast the damper stops the reed", 0.5),
        ("Thump", "the hammer's knock against the reed", 0.5),
    ],
];

/// Where strip `ch`'s knob `knob` for `patch` sits in the flat settings
/// array. One definition, because an index computed in three places is three
/// chances to write one sound's knob into another's slot.
pub fn keys_param_index(ch: usize, patch: usize, knob: usize) -> usize {
    (ch * KEYS_PATCHES + patch) * KEYS_PARAMS + knob
}

/// A whole desk's worth of knobs at their initialised values.
pub fn keys_params_default(channels: usize) -> Vec<f64> {
    let mut out = vec![0.0; channels * KEYS_PATCHES * KEYS_PARAMS];
    for ch in 0..channels {
        for (patch, specs) in KEYS_PARAM_SPECS.iter().enumerate() {
            for (knob, spec) in specs.iter().enumerate() {
                out[keys_param_index(ch, patch, knob)] = f64::from(spec.2);
            }
        }
    }
    out
}

/// Every effect Tangent ships, as a picker row.
///
/// **The same sentinel trick as the DX7 and the drumpad, for the same
/// reason.** A row here rides the bay through the picker, the settings file
/// and the saved session without any of those layers learning that there is
/// such a thing as a built-in effect; the one place that looks is where a bay
/// is actually filled. Nothing is written to disk and nothing appears in a
/// plugin scan — these exist only inside the app, which is the whole point.
///
/// `(path, name, what it is)`. The third column is the line under the name in
/// the picker, and it is where the hardware gets named: the row's NAME is
/// Tangent's, and the reference goes in the description. See
/// `recorder_panel::Fx::subtitle`, which has done it that way since the
/// phaser, and see LICENSING.md on why the split matters.
pub const FX_UNITS: [(&str, &str, &str); 8] = [
    (
        "@tangent-plate",
        "Tangent Plate",
        "legendary plate, after the EMT-140",
    ),
    (
        "@tangent-echo",
        "Tangent Echo",
        "warbling saturation, after the RE-201 Space Echo",
    ),
    (
        "@tangent-chorus",
        "Tangent Chorus",
        "true stereo, after the Boss CE-1",
    ),
    (
        "@tangent-phaser",
        "Tangent Phaser",
        "four stages, after the MXR Phase 90",
    ),
    (
        "@tangent-hpf",
        "Tangent HPF",
        "takes the bottom off, after everything",
    ),
    (
        "@tangent-lpf",
        "Tangent LPF",
        "takes the top off - up is darker",
    ),
    (
        "@tangent-gain",
        "Tangent Gain",
        "drive into the limiter - it CAN clip",
    ),
    (
        "@tangent-limiter",
        "Tangent Limiter",
        "true peak ceiling - no makeup",
    ),
];

/// Whether a bay's contents are one of Tangent's own rather than a file.
///
/// **The leading `@` is the whole test, and it is the invariant
/// [`BUILTIN_PATH`] already documents**: no filesystem path starts with one,
/// so a sentinel can never collide with a bundle somebody has installed.
///
/// This exists because the test used to be written out as
/// `p != BUILTIN_PATH && p != DRUMPAD_PATH` in ten places across three files.
/// Two sentinels made that survivable; ten does not, and the failure is
/// silent — a site left on the old two-way test lets an effect through a
/// filter meant to exclude it, which surfaces as a row that can be "deleted
/// from disk" or one that turns up in a plugin rescan. One function, and
/// adding the eleventh sentinel needs no edit here at all.
pub fn is_builtin(path: &str) -> bool {
    path.starts_with('@')
}

/// Said under the list, every time, in full-strength text rather than the faint
/// grey the other note gets.
///
/// A plugin is not a document Tangent reads: it is code, in this process, with
/// this process's file access and this process's crash. Somebody deserves to
/// know that before they click Load rather than after their first bad plugin,
/// and the sentence is short enough that they will actually read it.
const PLUGIN_TRUST_NOTE: &str =
    "A plugin is somebody else's code, running inside Tangent. It shares this process, so one that \
     crashes takes Tangent with it - which is also why this build ships with macOS library \
     validation disabled. Load ones you trust.";

/// Said under that: why the vendor column is mostly blank.
///
/// Without it an empty vendor reads as a bug. With it, it reads as the
/// deliberate choice described in this section's header.
const PLUGIN_SCAN_NOTE: &str =
    "Names come from the files on disk. Tangent opens a plugin, and asks it what it really is, only \
     when it loads it - asking all of them would mean starting all of them.";

/// One installed VST3, as a directory listing knows it.
///
/// Public because the fields are the whole contract with the app: the caller
/// builds these from `tangent_host::discover()`, and a caller that already knows
/// a vendor (because it has loaded that bundle before) may fill it in. What it
/// must never do is go and find out.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PluginEntry {
    /// Absolute path to the `.vst3` bundle. This is the identity and the
    /// settings key — a display name is not unique and changes with the
    /// vendor's whims.
    pub path: String,
    /// What to show: the bundle's file stem, cleaned up.
    pub name: String,
    /// Vendor, when known. Usually empty, and that is not a gap to be filled:
    /// filling it requires OPENING the module. See the section header.
    pub vendor: String,
}

impl PluginEntry {
    /// Read a row off a bundle path, opening nothing.
    ///
    /// The stem, because a `.vst3` is a DIRECTORY and its name is the only
    /// thing on disk that reads like a product ("Pianoteq 8.vst3"). A bundle
    /// whose path has no stem at all — a trailing `..`, a caller's mistake —
    /// falls back to the whole path rather than an unclickable-looking blank
    /// row.
    /// **This app's OWN VST3, which is not an instrument.**
    ///
    /// Tangent ships a plugin that draws the same display inside a DAW. It has
    /// no audio by design — `AUDIO_IO_LAYOUTS` is empty — so loading it here
    /// fails, correctly, with "no audio output channels". A Linux tester met it
    /// as a row called "Tangent" sitting under "Tangent DX7", chose the
    /// familiar name, and read the failure as the app being broken.
    ///
    /// Matched by bundle stem, which is what the picker has: the list is a
    /// directory listing and opening every module to ask its name is the one
    /// thing this dialog will not do.
    pub const OWN_PLUGIN_STEM: &'static str = "Tangent";

    pub fn from_bundle(path: &Path) -> PluginEntry {
        Self::from_bundle_with_vendor(path, None, None)
    }

    /// The picker's own builder: the vendor rides in from the scan, and an
    /// unnamed one stays empty — the bottom group, per the owner.
    pub fn from_bundle_with_vendor(
        path: &Path,
        vendor: Option<&str>,
        // Given when the caller knows a better name than the path carries -
        // see `App::plugin_list`. `None` keeps the original behaviour exactly.
        display: Option<&str>,
    ) -> PluginEntry {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().trim().to_owned())
            .filter(|s| !s.is_empty());
        let full = path.to_string_lossy().into_owned();
        let name = display
            .map(|d| d.trim().to_owned())
            .filter(|d| !d.is_empty())
            .or(stem)
            .unwrap_or_else(|| full.clone());
        // Said in the row rather than hidden from it. The file IS on the disk,
        // and somebody who has just built it should see it where they put it —
        // what they should not do is choose it and be told the app is broken.
        let vendor = if name == Self::OWN_PLUGIN_STEM {
            "this app's own display plugin - for a DAW, not for here".to_owned()
        } else {
            vendor.unwrap_or("").to_owned()
        };
        PluginEntry {
            name,
            path: full,
            vendor,
        }
    }
}

/// Whether a row survives the filter box.
///
/// Case-insensitive, and on the vendor as well as the name, because "Arturia"
/// is how you look for the eleven of them at once and "arturia" is how it gets
/// typed. An empty filter matches everything, so the list needs no special case
/// for the state it opens in.
fn plugin_matches(entry: &PluginEntry, filter: &str) -> bool {
    let needle = filter.trim().to_lowercase();
    needle.is_empty()
        || entry.name.to_lowercase().contains(&needle)
        || entry.vendor.to_lowercase().contains(&needle)
}

/// The rows the filter leaves on screen, as indices into `plugins`.
///
/// Indices and not references, because `selected` is an index and the two have
/// to be comparable. Both the list and [`plugin_choice`] are built from this
/// one function, so what is on screen and what OK commits cannot disagree —
/// which is the whole of the index-into-a-filtered-list bug.
fn visible_plugins(plugins: &[PluginEntry], filter: &str) -> Vec<usize> {
    plugins
        .iter()
        .enumerate()
        .filter(|(_, p)| plugin_matches(p, filter))
        .map(|(i, _)| i)
        .collect()
}

/// What Load would do, when it would do anything.
#[derive(Debug, PartialEq, Eq)]
enum PluginChoice<'a> {
    Load(&'a PluginEntry),
    /// The None row: unload whatever is loaded and play nothing.
    Unload,
}

impl PluginChoice<'_> {
    /// The action, built from the entry — so the PATH travels and the name
    /// stays on screen where it belongs.
    fn action(&self) -> DialogAction {
        DialogAction::LoadPlugin {
            path: match self {
                PluginChoice::Load(e) => Some(e.path.clone()),
                PluginChoice::Unload => None,
            },
        }
    }
}

/// What the picker is currently offering to do, or nothing.
///
/// **The single gate**, exactly as `confirm` is for the tuning editor and
/// `export_ready` is for Export: the Load button is enabled on `is_some()` and
/// the action is built from the same value, so "you cannot load a row you
/// cannot see" is one function rather than two conditions that drift apart.
///
/// Three rules, in this order:
///
/// 1. The highlighted entry, **if the filter still shows it** — and if its
///    vendor folder is OPEN, the same rule in a second hat: select Pianoteq,
///    collapse MODARTT, and the highlight is gone from the screen; committing
///    it would be the classic filtered-list bug wearing a different hat.
///    Filtering forces every folder open (the draw does the same), and the
///    built-ins stand above the folders.
/// 2. Otherwise, the only entry the filter left — the reason typing four
///    letters and pressing Enter works, which with 112 plugins is how the
///    dialog is actually used. It needs a NON-EMPTY filter: on a machine with
///    exactly one plugin installed, an empty filter also leaves exactly one
///    entry, and that rule would make the None row unreachable.
/// 3. Otherwise the None row (`selected` is nothing), and only when there is
///    something to unload. Unloading nothing is not an action, which is what
///    greys Load out on a picker opened with no instrument running.
fn plugin_choice<'a>(
    plugins: &'a [PluginEntry],
    selected: Option<usize>,
    current: Option<&str>,
    filter: &str,
    open_folders: &[String],
) -> Option<PluginChoice<'a>> {
    let visible = visible_plugins(plugins, filter);
    if let Some(i) = selected {
        let in_open_folder = plugins.get(i).is_some_and(|p| {
            is_builtin(&p.path)
                || !filter.trim().is_empty()
                || open_folders.iter().any(|f| f == plugin_folder(p))
        });
        if visible.contains(&i) && in_open_folder {
            // Resolved through `get` rather than indexed. The list is rebuilt
            // from disk every time the dialog opens, and a stale index must
            // select nothing rather than panic.
            return plugins.get(i).map(PluginChoice::Load);
        }
    }
    if !filter.trim().is_empty() {
        if let [only] = visible[..] {
            return plugins.get(only).map(PluginChoice::Load);
        }
    }
    if selected.is_none() && current.is_some() {
        return Some(PluginChoice::Unload);
    }
    None
}

/// One row's text: a mark for the plugin that is loaded, the name, and the
/// vendor when there is one.
///
/// The mark LEADS. It trails on [`Dialog::DevicePicker`], where there are four
/// devices and you can see them all; ninety rows down a list of 112 a trailing
/// bullet is invisible, and the dialog's font is monospace, so a one-character
/// gutter lines every row up into a column you can run an eye down.
/// The folder a scanned plugin files under. One rule, used by the
/// builder (to pre-open the loaded plugin's folder) and the draw (to
/// interleave the headings), so the two cannot disagree.
/// The house's own folder in the picker — the owner's pending LLC, worn
/// as the vendor every built-in files under. Always sorted first, dressed
/// in the cluster's own colour, and opened FOR the user exactly once: the
/// first picker they ever see shows what ships before what they bought.
pub const LABS_VENDOR: &str = "Tangent Labs";

fn plugin_folder(entry: &PluginEntry) -> &str {
    if entry.vendor.trim().is_empty() {
        "Other"
    } else {
        entry.vendor.as_str()
    }
}

fn plugin_row(entry: &PluginEntry, current: Option<&str>) -> String {
    let mark = if current == Some(entry.path.as_str()) {
        '\u{2022}'
    } else {
        ' '
    };
    // The vendor is the FOLDER heading, never a row suffix — true for the
    // house shelf too, now that the built-ins live under their own heading
    // instead of standing bare above the sort.
    let _ = &entry.vendor;
    format!("{mark} {}", entry.name)
}

/// Where VST3s live on this platform, said out loud when none were found.
///
/// An empty box teaches nothing, and an empty list has an ordinary cause on
/// every platform: the installer offered AU or VST2 and the user took it. So
/// the dialog names the directories it looked in.
///
/// `cfg!` and not a probe, exactly as [`encoder_note`] does it: this asks about
/// the TARGET, which is the machine the dialog will be read on, so a Windows
/// build cross-compiled on this Mac still names the Windows folder.
fn vst3_locations() -> &'static str {
    if cfg!(target_os = "macos") {
        "No VST3 instruments found. macOS keeps them in ~/Library/Audio/Plug-Ins/VST3 for one user \
         and /Library/Audio/Plug-Ins/VST3 for everybody. An installer that offered you AU or VST2 \
         and took the default put nothing in either."
    } else if cfg!(target_os = "windows") {
        "No VST3 instruments found. Windows keeps them in %COMMONPROGRAMFILES%\\VST3 - usually \
         C:\\Program Files\\Common Files\\VST3. A folder of .dll plug-ins is VST2, which \
         Tangent does not host (it loads VST3 only). Rescan from the right-click menu after \
         installing a VST3 plug-in."
    } else if cfg!(target_os = "freebsd") {
        // The paths the scanner actually walks — `scan.rs` names `~/.vst3`
        // and `/usr/local/lib/vst3` and deliberately NOT `/usr/lib/vst3`,
        // because ports install under /usr/local and /usr/lib is base-system
        // territory where a VST3 cannot exist. This arm used to fall into the
        // Linux text below and told FreeBSD users to put plugins in a folder
        // the scan would never open. And LV2 gets a sentence because on
        // FreeBSD it is not a footnote: the audio ports overwhelmingly ship
        // LV2, so "no VST3s found" is the EXPECTED state of a machine that
        // has plenty of instruments to offer.
        "No VST3 instruments found. FreeBSD keeps them in ~/.vst3 for one user and \
         /usr/local/lib/vst3 for everybody. Most FreeBSD audio ports ship LV2 rather than VST3; \
         LV2 plugins from /usr/local/lib/lv2 and ~/.lv2 appear in this same list."
    } else {
        "No VST3 instruments found. Linux keeps them in ~/.vst3 for one user and /usr/lib/vst3 for \
         everybody."
    }
}

impl Dialog {
    /// Open the picker on the bundles the caller found.
    ///
    /// `bundles` is `tangent_host::discover()`'s output verbatim — a directory
    /// listing, which is the whole point (see the section header above). This
    /// crate cannot depend on `tangent-host` and does not want to: paths in,
    /// a path out.
    ///
    /// The app opens it like this:
    ///
    /// ```ignore
    /// self.dialog = Some(Dialog::plugin_picker(
    ///     &tangent_host::discover(),
    ///     self.settings.plugin_path.clone(),   // what is loaded now, or None
    /// ));
    /// ```
    ///
    /// and drains `DialogAction::LoadPlugin { path }`, where `None` means
    /// unload.
    ///
    /// Two things happen on the way in. The rows are sorted the way a person
    /// reads a list rather than the way `discover` returns them — it sorts
    /// `PathBuf`s, so the system folder lands before the user's and "Zebra"
    /// before "arturia", which in a list of 112 is indistinguishable from
    /// unsorted. And the loaded plugin is preselected, so the dialog opens on
    /// the answer to "what am I playing through", not on row one.
    pub fn plugin_picker(
        bundles: &[(PathBuf, Option<String>, Option<String>)],
        current: Option<String>,
        labs_open: bool,
    ) -> Dialog {
        let mut plugins: Vec<PluginEntry> = bundles
            .iter()
            .map(|(p, v, n)| {
                PluginEntry::from_bundle_with_vendor(p, v.as_deref(), n.as_deref())
            })
            .collect();
        // **By DEVELOPER, the owner's ruling**: the named vendors in
        // alphabetical order, each one's plugins alphabetical inside it, and
        // everything that names no vendor together at the BOTTOM. The row
        // prints the vendor, so the grouping reads as sections.
        plugins.sort_by(|a, b| {
            a.vendor
                .is_empty()
                .cmp(&b.vendor.is_empty())
                .then_with(|| a.vendor.to_lowercase().cmp(&b.vendor.to_lowercase()))
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
                // Two bundles can share a name — the user's copy and the system
                // one — and a comparator that called them equal would order
                // them differently on different runs.
                .then_with(|| a.path.cmp(&b.path))
        });
        // **The built-in goes first, above the sort.** It is the instrument
        // that is always there, it is what a fresh install plays, and it is
        // where somebody who has never installed a VST3 should find something
        // rather than an empty list. Inserted after the sort so it stays at the
        // top rather than landing under T.
        // **The house's shelf, above the sort.** Everything that ships in
        // the box files under one vendor folder — the instruments first,
        // then the suite — so somebody who has never installed a VST3 finds
        // a full rack rather than an empty list, and somebody with ninety
        // plugins finds ours exactly once, at the top, closed.
        for (at, (path, name)) in [
            (0, (BUILTIN_PATH, BUILTIN_NAME)),
            (1, (DRUMPAD_PATH, DRUMPAD_NAME)),
            (2, (KEYS_PATH, KEYS_NAME)),
        ] {
            plugins.insert(
                at,
                PluginEntry {
                    path: path.to_owned(),
                    name: name.to_owned(),
                    vendor: LABS_VENDOR.to_owned(),
                },
            );
        }
        for (i, (path, name, _)) in crate::faces::SUITE_UNITS.iter().enumerate() {
            plugins.insert(
                3 + i,
                PluginEntry {
                    path: (*path).to_owned(),
                    name: (*name).to_owned(),
                    vendor: LABS_VENDOR.to_owned(),
                },
            );
        }
        let selected = current
            .as_deref()
            .and_then(|c| plugins.iter().position(|p| p.path == c));
        // Every folder closed at open — except the loaded plugin's, so the
        // preselected, marked row has somewhere to be seen; and the house
        // folder on the very first visit, so the beginner's first picker
        // is not a wall of shut drawers.
        let mut open_folders: Vec<String> = selected
            .and_then(|i| plugins.get(i))
            .map(|e| vec![plugin_folder(e).to_owned()])
            .unwrap_or_default();
        if labs_open && !open_folders.iter().any(|f| f == LABS_VENDOR) {
            open_folders.push(LABS_VENDOR.to_owned());
        }
        Dialog::PluginPicker {
            plugins,
            selected,
            current,
            open_folders,
            filter: String::new(),
            focus: true,
        }
    }
}

// ── the patch editor ────────────────────────────────────────────────────────

/// **Wide and short, so it fits inside the window it belongs to.** The
/// parameters are in columns rather than one tall list — see the editor's own
/// comment — which is what lets a whole operator be on screen at once.
const EDITOR_W: f32 = 780.0;
const EDITOR_H: f32 = 470.0;

/// How many columns the parameters are laid out in.
///
/// Three: an operator has twenty-one numbers and three sevens is a block a
/// glance takes in. Two makes the window tall again and four makes each column
/// too narrow for "Pitch mod sens" and a number beside it.
const PARAM_COLS: usize = 3;

/// Characters the format has room for. Ten, and not one more.
const NAME_LEN: usize = 10;

/// Draw the algorithm: which operator feeds which, and which one feeds itself.
///
/// **Laid out from the routing, not from thirty-two stored pictures.** The
/// routing is what the synth plays; a table of drawings would be a second
/// description of the same fact, and the way those drift is a diagram that
/// lies about the sound.
///
/// Carriers sit on the bottom line with their output under them, and each
/// operator stands directly above the one it modulates. An operator with two
/// modulators has them side by side above it, which is how every chart on the
/// front of a DX7 is drawn.
fn draw_algorithm(painter: &egui::Painter, rect: Rect, patch: &crate::ports::PatchEdit, t: &DialogTheme) {
    if !rect.is_positive() {
        return;
    }
    let dest = patch.routing;
    // Who modulates whom. `children[i]` are the operators feeding operator
    // `i + 1`; `children[6]` would be the carriers, which are kept separately
    // because they are the roots rather than a node's children.
    let mut children: [Vec<usize>; 6] = Default::default();
    let mut carriers: Vec<usize> = Vec::new();
    for (i, d) in dest.iter().enumerate() {
        if *d == 0 {
            carriers.push(i);
        } else if let Some(list) = children.get_mut(usize::from(*d) - 1) {
            list.push(i);
        }
    }
    // How many columns a subtree needs, and how deep it goes.
    fn span(op: usize, children: &[Vec<usize>; 6]) -> usize {
        let n: usize = children[op].iter().map(|c| span(*c, children)).sum();
        n.max(1)
    }
    fn depth(op: usize, children: &[Vec<usize>; 6]) -> usize {
        1 + children[op]
            .iter()
            .map(|c| depth(*c, children))
            .max()
            .unwrap_or(0)
    }
    let cols: usize = carriers.iter().map(|c| span(*c, &children)).sum::<usize>().max(1);
    let rows: usize = carriers
        .iter()
        .map(|c| depth(*c, &children))
        .max()
        .unwrap_or(1)
        .max(1);

    // A cell per column and per row, with the output line under the bottom.
    //
    // **Cells are square-ish and the whole thing is centred**, rather than each
    // column taking a share of whatever width the panel has. Two stacks spread
    // across five hundred points is a diagram with a hole in the middle: what
    // says "these two feed the same output" is them being NEAR each other.
    let out_h = rect.height() * 0.12;
    let grid = Rect::from_min_max(rect.min, Pos2::new(rect.right(), rect.bottom() - out_h));
    let ch = grid.height() / rows as f32;
    let cw = (grid.width() / cols as f32).min(ch * 1.5);
    let used = cw * cols as f32;
    let grid = Rect::from_min_size(
        Pos2::new(grid.center().x - used * 0.5, grid.top()),
        Vec2::new(used, grid.height()),
    );
    let side = (cw.min(ch) * 0.66).max(6.0);
    let font = FontId::new((side * 0.52).max(6.0), fonts::courier_bold());

    // Place every operator: a column centre and a row from the bottom.
    let mut at = [Pos2::ZERO; 6];
    fn place(
        op: usize,
        col: &mut f32,
        row: usize,
        children: &[Vec<usize>; 6],
        grid: Rect,
        cw: f32,
        ch: f32,
        at: &mut [Pos2; 6],
    ) -> f32 {
        let kids = &children[op];
        let x = if kids.is_empty() {
            let x = grid.left() + (*col + 0.5) * cw;
            *col += 1.0;
            x
        } else {
            let xs: Vec<f32> = kids
                .iter()
                .map(|c| place(*c, col, row + 1, children, grid, cw, ch, at))
                .collect();
            (xs.iter().sum::<f32>()) / xs.len() as f32
        };
        at[op] = Pos2::new(x, grid.bottom() - (row as f32 + 0.5) * ch);
        x
    }
    let mut col = 0.0_f32;
    for c in &carriers {
        place(*c, &mut col, 0, &children, grid, cw, ch, &mut at);
    }

    // The wires first, so the boxes sit on top of their ends.
    let wire = Stroke::new(1.0_f32, t.text.gamma_multiply(0.7));
    for (i, d) in dest.iter().enumerate() {
        let from = at[i];
        let to = if *d == 0 {
            Pos2::new(from.x, rect.bottom() - out_h * 0.35)
        } else {
            at[usize::from(*d) - 1]
        };
        painter.line_segment([from, to], wire);
    }
    // The boxes.
    for (i, p) in at.iter().enumerate() {
        let r = Rect::from_center_size(*p, Vec2::splat(side));
        painter.rect_filled(r, 2.0, t.bg);
        // A carrier is drawn heavier: its output is what you hear, and that is
        // the first thing to read off one of these.
        let heavy = dest[i] == 0;
        painter.rect_stroke(
            r,
            2.0,
            Stroke::new(if heavy { 2.0 } else { 1.0 }, t.text),
            egui::StrokeKind::Inside,
        );
        painter.text(
            *p,
            egui::Align2::CENTER_CENTER,
            format!("{}", i + 1),
            font.clone(),
            t.text,
        );
        // The feedback loop, as a small arc over the operator it is taken
        // from. One per algorithm, always.
        if usize::from(patch.feedback_op) == i + 1 {
            let top = r.center().y - side * 0.5;
            let x0 = r.center().x + side * 0.5;
            let x1 = x0 + side * 0.42;
            for seg in [
                [Pos2::new(x0, top + side * 0.25), Pos2::new(x1, top + side * 0.25)],
                [Pos2::new(x1, top + side * 0.25), Pos2::new(x1, top - side * 0.25)],
                [Pos2::new(x1, top - side * 0.25), Pos2::new(r.center().x, top - side * 0.25)],
                [Pos2::new(r.center().x, top - side * 0.25), Pos2::new(r.center().x, top)],
            ] {
                painter.line_segment(seg, wire);
            }
        }
    }
    // And the name of the thing, bottom left.
    let label = format!("ALGORITHM {}", patch.algorithm + 1);
    let size = (out_h * 0.72).max(6.0);
    painter.text(
        Pos2::new(rect.left(), rect.bottom()),
        egui::Align2::LEFT_BOTTOM,
        &label,
        FontId::new(size, fonts::courier_bold()),
        t.text,
    );
}

// ── the patch picker ────────────────────────────────────────────────────────

/// Voices in a DX7 cartridge. The format has exactly this many, always.
pub const CARTRIDGE_VOICES: usize = 32;

/// The row shown when no cartridge is loaded: the patch compiled into the app.
pub const BUILTIN_PATCH_ROW: &str = "E.PIANO 1  (built in)";

/// What the buttons on the cartridge row need between them.
///
/// Reserved rather than measured: measuring means laying the row out twice,
/// and the failure it prevents is a bank name running under "Edit...".
const BANK_BUTTONS_W: f32 = 176.0;

/// Clear air between the bank name and the first button.
///
/// Without it the name is allowed to fill every remaining point and ends up
/// touching "Edit...", which reads as a layout bug even though nothing is
/// overlapping. The shipped bank's own name is long enough to do it.
const BANK_NAME_GAP: f32 = 14.0;

/// And what "Factory" needs, on top, when there is a cartridge to leave.
const FACTORY_BUTTON_W: f32 = 74.0;

const PATCH_DIALOG_W: f32 = 520.0;
const PATCH_DIALOG_H: f32 = 470.0;

impl Dialog {
    /// Open the patch picker for the built-in in `slot`.
    ///
    /// `voices` empty means no cartridge: the dialog still opens, because the
    /// button that loads one is inside it and a picker you can only reach after
    /// loading a cartridge is a picker nobody finds.
    pub fn patch_picker(
        slot: usize,
        bank: String,
        bad_checksum: bool,
        factory: bool,
        voices: Vec<String>,
        selected: Option<usize>,
    ) -> Dialog {
        Dialog::PatchPicker {
            slot,
            bank,
            bad_checksum,
            factory,
            // A cartridge is 32 voices or it is not a cartridge, but this comes
            // from outside and indexes an array.
            selected: selected.filter(|i| *i < voices.len()),
            voices,
            filter: String::new(),
            focus: true,
            error: String::new(),
        }
    }
}

/// Rows matching `filter`, as indices into `voices`.
///
/// By name only. A cartridge's patches have no other text, and the number is
/// shown but not searched: typing "3" to find every patch with a 3 in its
/// position is not a thing anybody wants.
fn visible_patches(voices: &[String], filter: &str) -> Vec<usize> {
    let f = filter.trim().to_lowercase();
    voices
        .iter()
        .enumerate()
        .filter(|(_, v)| f.is_empty() || v.to_lowercase().contains(&f))
        .map(|(i, _)| i)
        .collect()
}

/// Stable surface identity: a viewport id on the desktop, an `Area` id in a
/// plugin. `dialog: Option<Dialog>` means there is never more than one open, so
/// one id serves them all and the window keeps its place between dialogs.
const DIALOG_ID: &str = "ivory-dialog";

pub fn color_pick_title(target: ColorTarget) -> &'static str {
    match target {
        ColorTarget::WhiteIdle => "Choose White Key Color",
        ColorTarget::BlackIdle => "Choose Black Key Color",
        ColorTarget::Active => "Choose Active Key Color",
        ColorTarget::Sustain => "Choose Sustain Pedal Color",
        ColorTarget::ChordText => "Choose Chord Color",
        ColorTarget::RecorderBg => "Choose Recorder Color",
    }
}

/// Theme palette shared by About and the color modals (spec §7.3 / D-UI-7).
struct DialogTheme {
    bg: Color32,
    text: Color32,
    button_bg: Color32,
    button_hover: Color32,
    button_border: Color32,
    /// The ground under a TYPED value — text edits and `DragValue`s, which
    /// egui paints on `extreme_bg_color` rather than on any widget fill. It
    /// needs its own field because a field must contrast with the theme's
    /// TEXT, and on the light theme the button fill is too close to the
    /// ground to read as an input box. See `apply_theme`.
    field_bg: Color32,
    /// Secondary text: quieter by WEIGHT, never by being harder to read.
    /// See `Surface::faint`.
    faint: Color32,
}

/// A dialog is a floating panel like any other — **made of the desk's own
/// face**, through the one derivation every panel now shares.
///
/// It was the frozen pair: black-on-ivory and its inverse, reachable by no
/// theme. Worse, the only caller passed `false` forever, so a dialog never
/// went dark either — it was a light card over a dark app.
fn theme(face: Color32) -> DialogTheme {
    theme_of(face)
}

/// The same, under a name a test can call while a loop variable is called
/// `theme`.
fn theme_of(face: Color32) -> DialogTheme {
    let p = crate::recorder_panel::surface_of(face);
    DialogTheme {
        bg: p.bg,
        text: p.ink,
        button_bg: p.raised,
        button_hover: p.hover,
        button_border: p.edge,
        field_bg: p.well,
        faint: p.faint,
    }
}

/// Where this frame's face is kept for the windows that are NOT dialogs.
const FACE_ID: &str = "ivory-dialog-face";

/// The face the frozen pair used to be made of, and the answer [`live_face`]
/// gives on a frame [`show`] has not run yet — which, with `show` called
/// unconditionally at the top of every update, is no frame at all.
const FALLBACK_FACE: Color32 = Color32::from_rgb(0xE3, 0xD9, 0xBD);

/// Remember the face [`show`] was handed, so a window that does not come
/// through `show` can follow the theme too.
///
/// The export window is the customer: it is deliberately not a `Dialog` — it
/// answers no question and takes no turn in the one dialog slot — so it never
/// sees the `face` parameter and hard-coded the old ivory instead. That held
/// for exactly as long as ivory was the only face; sixteen themes later it
/// was the one surface that stayed beige, on screen for minutes at a time.
/// `show` runs every frame whether or not a dialog is up, and runs before the
/// export window draws, so what it leaves here is always THIS frame's answer.
/// One fact, one carrier: a second `face` parameter threaded to the export
/// call site would be a copy that could drift from the one every dialog gets.
fn remember_face(ctx: &egui::Context, face: Color32) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(FACE_ID), face));
}

/// The complement: the face [`show`] last remembered.
fn live_face(ctx: &egui::Context) -> Color32 {
    ctx.memory(|m| m.data.get_temp(egui::Id::new(FACE_ID)))
        .unwrap_or(FALLBACK_FACE)
}

/// The cluster's glass pair, remembered the same way the face is — the app
/// stamps it once per frame, and the effect faces build their `FaceStyle`
/// from it without a `Settings` in reach. Same trick, same reasons, same
/// lifetime as `remember_face`.
const LCD_ID: &str = "dialog-lcd";

pub fn remember_lcd(ctx: &egui::Context, lcd: Color32, lcd_face: Color32) {
    ctx.memory_mut(|m| m.data.insert_temp(egui::Id::new(LCD_ID), (lcd, lcd_face)));
}

/// The house folder's ink: the cluster's digit colour, held legible on the
/// dialog's ground by the same walk every face tier takes — distinct where
/// the theme allows it, readable everywhere, never a colour the eye must
/// squint at to be sold something.
fn labs_ink(ctx: &egui::Context, bg: Color32, fallback: Color32) -> Color32 {
    let (lcd, _) = live_lcd(ctx);
    let mut c = lcd;
    let mut t = 0.0f32;
    while crate::recorder_panel::contrast_ratio(c, bg) < 4.5 && t < 1.0 {
        t += 0.1;
        let lerp = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8
        };
        c = Color32::from_rgb(
            lerp(lcd.r(), fallback.r()),
            lerp(lcd.g(), fallback.g()),
            lerp(lcd.b(), fallback.b()),
        );
    }
    if crate::recorder_panel::contrast_ratio(c, bg) < 4.5 {
        fallback
    } else {
        c
    }
}

/// The pair last remembered; the shipped tape-green on near-black when the
/// app has not spoken yet, which is also the settings default.
pub fn live_lcd(ctx: &egui::Context) -> (Color32, Color32) {
    ctx.memory(|m| m.data.get_temp(egui::Id::new(LCD_ID)))
        .unwrap_or((
            Color32::from_rgb(0xA3, 0xE0, 0x48),
            Color32::from_rgb(0x0B, 0x0E, 0x0A),
        ))
}

fn apply_theme(style: &mut egui::Style, t: &DialogTheme) {
    style.spacing.button_padding = egui::vec2(12.0, 4.0); // Qt: padding 4px 12px
    let set = |wv: &mut egui::style::WidgetVisuals, bg: Color32| {
        wv.bg_fill = bg;
        wv.weak_bg_fill = bg;
        wv.bg_stroke = Stroke::new(1.0_f32, t.button_border);
        wv.fg_stroke = Stroke::new(1.0_f32, t.text);
        wv.corner_radius = CornerRadius::ZERO;
        wv.expansion = 0.0;
    };
    set(&mut style.visuals.widgets.inactive, t.button_bg);
    set(&mut style.visuals.widgets.hovered, t.button_hover);
    set(&mut style.visuals.widgets.active, t.button_hover);
    set(&mut style.visuals.widgets.open, t.button_hover);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, t.text);
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, t.text);
    style.visuals.override_text_color = None;
    style.visuals.window_fill = t.bg;
    style.visuals.selection.bg_fill = t.button_hover;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, t.text);
    style.visuals.hyperlink_color = t.text;
    // **The field a value is TYPED into, which is not a button.** egui paints
    // text edits and `DragValue`s on `extreme_bg_color`, and nothing here set
    // it — so every themed field inherited egui's default near-black while the
    // theme's text stayed black: the colour picker's R/G/B boxes were legible
    // until you clicked into one, and then they were black on black. It is one
    // field for every typed box in every themed dialog, so it belongs here
    // rather than at each call site.
    style.visuals.extreme_bg_color = t.field_bg;
    style.visuals.widgets.noninteractive.bg_fill = t.bg;
}

/// Stock egui styling for the MIDI dialogs, which Qt showed with the platform
/// default look, NOT the app theme (spec §7.1).
fn stock_style(ctx: &egui::Context) -> egui::Style {
    egui::Style {
        visuals: match ctx.input(|i| i.raw.system_theme) {
            Some(egui::Theme::Dark) => egui::Visuals::dark(),
            _ => egui::Visuals::light(),
        },
        ..Default::default()
    }
}

struct VpResult {
    close: bool,
    /// The theme key, pressed in a plugin window. See `DialogAction::CycleTheme`.
    cycle_theme: bool,
}

/// Focus a text field and select everything in it, once.
///
/// The point of the shortcut that opens these dialogs is that you can start
/// typing. Without this you land on a dialog whose field is not focused, so
/// the first thing you do is click it — and even then the caret sits at one
/// end and the prefilled name has to be cleared by hand. Selecting the whole
/// value means the first keystroke replaces it, which is what every rename
/// field on every platform does.
fn focus_and_select_all(ui: &egui::Ui, resp: &egui::Response, text: &str) {
    resp.request_focus();
    if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), resp.id) {
        let all = egui::text_selection::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        );
        state.cursor.set_char_range(Some(all));
        state.store(ui.ctx(), resp.id);
    }
}

/// Where child windows open. A viewport can only see its own geometry, so the
/// main window's rect has to be handed in from the app rather than read here.
#[derive(Clone, Copy, Default)]
pub struct Placement {
    /// The main window's inner rect, in monitor coordinates.
    pub parent: Option<Rect>,
    pub monitor: Option<Vec2>,
    /// What the host allows. `child_windows` decides whether a dialog is an OS
    /// window or is drawn in the canvas.
    pub caps: crate::host::Caps,
    /// The OS is showing a file panel of its own right now.
    ///
    /// Dialogs stop floating while it is, or they float above it — see
    /// `SurfaceSpec::on_top`. The panel is parented to the MAIN window, so
    /// nothing else can put it in front.
    pub native_panel_up: bool,
    /// The user just pressed the main window while this dialog was up.
    ///
    /// See `SurfaceSpec::raise`. Passed through rather than read here, because
    /// only the app knows a press happened.
    pub raise: bool,
    /// **Open BESIDE this point rather than centred**, in the main window's
    /// own coordinates.
    ///
    /// A dialog that answers a right-click belongs where the click was: the
    /// metronome's panel is the toolbar button's menu, and a menu that opens
    /// in the middle of the screen reads as a different kind of thing
    /// entirely. Set by the arm that opens such a dialog; every other one
    /// leaves it `None` and centres as it always has.
    ///
    /// Window coordinates, NOT monitor: the point comes from a hit test on
    /// the main surface, and `position_for` adds the parent's origin. The two
    /// spaces are not the same and mixing them puts the panel on another
    /// display.
    pub at: Option<Pos2>,
    /// The in-house bar a PLUGIN window wears, or `None` for OS chrome.
    ///
    /// Filled in by [`show`] from [`Dialog::is_modal`] — the one
    /// classification — so no arm of the big match ever has to say which
    /// kind of window it is drawing. Callers leave it `None`.
    pub chrome: Option<crate::shell::Chrome>,
}

impl Placement {
    /// Centred on the parent, kept fully on the monitor — and centred on the
    /// MONITOR when there is no parent to centre on yet.
    ///
    /// "Not known yet" has to mean exactly that. A viewport cannot see its own
    /// position on the first frames, so the app used to hand over a rect at
    /// (0, 0) rather than nothing — and the welcome dialog, which opens on the
    /// very first frame, centred itself on the top-left of the SCREEN. It was
    /// centred, on the wrong thing.
    ///
    /// Handing back `None` fixed that and left a smaller version of it: the
    /// platform then places the window whatever it likes, which for the welcome
    /// card — the ONE dialog that opens before the main window has a position —
    /// meant it never looked centred either. Falling back to the middle of the
    /// monitor is what it should have been all along. It is not a guess about
    /// the parent; it is the right answer to a different question, which is
    /// where to put a window that has nothing to belong to yet.
    ///
    /// `None` survives for the case that really has nowhere to go: no parent
    /// AND no monitor, which is a host that has told us nothing at all.
    fn position_for(self, size: Vec2) -> Option<Pos2> {
        let parent = self.believable_parent();
        // The DISPLAY, not the size of one — see [`Self::screen`]. Every
        // clamp below is into this rather than into a box at the origin,
        // because a window on the second monitor is centred on a rect that
        // the origin's box does not contain and clamping it there drags the
        // dialog onto the wrong screen.
        let screen = self.screen();
        // **Anchored, when something asked to be beside a point.** Offset a
        // few points down and right of the hand, the way every menu on this
        // desk hangs off what opened it, and clamped like any other so a
        // press near the screen's edge does not push it off.
        if let (Some(at), Some(parent)) = (self.at, parent) {
            return Some(clamp_into(
                parent.min + at.to_vec2() + Vec2::new(6.0, 6.0),
                size,
                screen,
            ));
        }
        let centre = match (parent, screen) {
            (Some(parent), _) => parent.center(),
            (None, Some(screen)) => screen.center(),
            (None, None) => return None,
        };
        Some(clamp_into(centre - size * 0.5, size, screen))
    }

    /// **The display the parent is on, in the coordinates the parent is
    /// reported in.**
    ///
    /// `monitor` is a SIZE and nothing more: the host says how big the
    /// display the main window is on IS, never where on the desk it starts.
    /// The parent arrives in the other space entirely — `app::last_drawn`
    /// translated by the main window's origin — and a desk with a second
    /// display has room in it for a negative origin and for coordinates
    /// well past the first screen's width.
    ///
    /// Comparing the two directly is what [`Self::believable_parent`] used
    /// to do, and it is a category error: the centre of a perfectly real
    /// window on the monitor to the right is not inside
    /// `Rect::from_min_size(ZERO, monitor)`, so that window was called a
    /// lie and every dialog it owned opened on the wrong screen — and the
    /// clamp that followed pinned them to the first screen's edge.
    ///
    /// What IS known is that the parent is ON this display: the size came
    /// from the display the MAIN WINDOW is on, and the parent is the main
    /// window's own pane. So the screen is a monitor-sized box that covers
    /// the parent — the one at the origin when the parent is where a
    /// single-screen desk puts it, and otherwise slid by the least it takes
    /// to cover it. No grid of identical displays is assumed, because a
    /// laptop panel beside a 4K is not one.
    ///
    /// A parent WIDER than the whole display cannot be covered by any slide
    /// — a stale origin with a live size can look like that — so the box
    /// centres on it instead, and the dialog still lands beside the middle
    /// of the thing it belongs to.
    fn screen(self) -> Option<Rect> {
        let m = self.monitor.filter(|m| m.x > 0.0 && m.y > 0.0)?;
        let base = Rect::from_min_size(Pos2::ZERO, m);
        let Some(parent) = self.believable_parent() else {
            return Some(base);
        };
        // Per axis: stay at the origin if that already covers the parent,
        // otherwise slide by the smallest amount that does.
        let slide = |lo: f32, hi: f32, size: f32| {
            if hi - lo <= size {
                0.0_f32.clamp(hi - size, lo)
            } else {
                (lo + hi) * 0.5 - size * 0.5
            }
        };
        Some(base.translate(Vec2::new(
            slide(parent.left(), parent.right(), m.x),
            slide(parent.top(), parent.bottom(), m.y),
        )))
    }

    /// **The parent rect, when it is describing a window somebody is
    /// actually looking at.**
    ///
    /// The rect travels a long way to get here: `app::last_drawn` is the pane
    /// the layout was drawn into, translated by the main window's own origin
    /// as the host last reported it. Either half can be a frame behind, and
    /// around a hide, a Space change or a display change the host can report
    /// an origin that puts the whole window off the screen it is supposedly
    /// on.
    ///
    /// **What that costs is not a dialog slightly off-centre.** The centred
    /// position is clamped, and a clamp handed a position far off the screen
    /// does not nudge it — it returns exactly `monitor - size`, which is the
    /// bottom-right corner. That is the owner's report, verbatim: "tab back
    /// = the plugin window is now on the right bottom of your monitor.
    /// consistent." A window does not creep towards that corner; it arrives
    /// there in one frame, because the corner is what the clamp answers with
    /// when the question was nonsense.
    ///
    /// So the question is asked of the one part of the rect this function
    /// uses: **is the MIDDLE of the parent anywhere a window could be?** If
    /// it is not, the rect is not a window anybody is looking at, centring
    /// on it is centring on nothing, and the middle of the screen is the
    /// honest answer — the same answer [`Self::position_for`] already gives
    /// for a parent it has not been told about yet.
    ///
    /// **"Anywhere a window could be" is the DESK, not one screen**, and
    /// that distinction is the whole of the second bug here. The first draft
    /// tested the parent's centre against `Rect::from_min_size(ZERO,
    /// monitor)` — a box in a space the parent does not live in, see
    /// [`Self::screen`] — so a window on the display to the right, or on one
    /// hung above and therefore at a negative y, was thrown out as a lie.
    /// On a two-monitor desk that is not an edge case, it is Tuesday.
    ///
    /// So the box is the primary screen grown by [`DESK_REACH`] displays in
    /// every direction. That is a bound and not a measurement, and it is
    /// worth being honest about which: nothing here can be told where the
    /// second display starts, so nothing here can tell a window on it from
    /// a stale origin that happens to land nearby. What the bound DOES
    /// catch is the shape the real fault has — a host parking a hidden or
    /// Space-switched window at a coordinate no desk reaches — while never
    /// again rejecting a display somebody could actually own.
    ///
    /// Note the case this deliberately does NOT reject: a window dragged hard
    /// against an edge, whose centre is still on screen. A dialog centred on
    /// that really does belong close to the edge, and the clamp putting it as
    /// near as it can get is the right answer rather than a symptom.
    ///
    /// With no monitor there is nothing to test the rect against and the
    /// parent is taken at its word — a host that has told us nothing is not a
    /// host that has told us something wrong.
    fn believable_parent(self) -> Option<Rect> {
        let parent = self.parent.filter(|r| r.is_positive())?;
        let Some(m) = self.monitor.filter(|m| m.x > 0.0 && m.y > 0.0) else {
            return Some(parent);
        };
        Rect::from_min_size(Pos2::ZERO, m)
            .expand2(m * DESK_REACH)
            .contains(parent.center())
            .then_some(parent)
    }
}

/// **How many displays out from the primary a desk is allowed to reach**,
/// for the one question [`Placement::believable_parent`] asks with it.
///
/// Four in every direction — a desk nine screens across and nine tall, which
/// nobody owns — because the cost of being too generous is that one wrong
/// rect gets believed for the one frame a window is placed on, and the cost
/// of being too mean is a real second monitor whose dialogs all open on the
/// wrong screen. The two are not symmetrical.
const DESK_REACH: f32 = 4.0;

/// [`crate::settings::clamp_to_monitor`], asked about a screen that is not
/// necessarily the one at the origin.
///
/// The same rule — a window fully inside the display, rounded to whole
/// points — against a box that can sit anywhere on the desk. It answers
/// exactly what that function answers when the box IS the origin's, which is
/// the single-display desk and most of the world.
fn clamp_into(pos: Pos2, size: Vec2, screen: Option<Rect>) -> Pos2 {
    let Some(s) = screen.filter(|s| s.width() > 0.0 && s.height() > 0.0) else {
        return pos;
    };
    Pos2::new(
        pos.x.clamp(s.left(), (s.right() - size.x).max(s.left())).round(),
        pos.y.clamp(s.top(), (s.bottom() - size.y).max(s.top())).round(),
    )
}

/// **The size and place a floating surface was BORN with, held for as long
/// as it is alive.**
///
/// **A window's position belongs to the user the moment it exists.**
/// Centring is the right answer to where a window OPENS and the wrong answer
/// to where it IS; [`Placement::position_for`] answers the first question,
/// and this is what stops it being asked over and over.
/// [`show_dialog_viewport`] rebuilds its `ViewportBuilder` every frame, and
/// egui's builder diff turns a `pos` that has changed into a
/// `ViewportCommand::OuterPosition` (`viewport.rs`, `ViewportBuilder::patch`).
/// So a surface that recomputes its position from the parent walks back to
/// the middle every time the main window moves and cannot be dragged aside at
/// all. The export window had exactly that and was fixed by holding its own
/// geometry; every plugin face still had it, and this is that fix made
/// general.
///
/// **One bad frame is enough**, which is why holding beats hardening the
/// inputs. A single frame in which the host reports a stale or off-screen
/// origin is a single `OuterPosition`, and the window is moved for good — it
/// does not drift back, because nothing asks again. That is the difference
/// between a window that wobbles and the owner's, which lands in the
/// bottom-right corner and stays there.
///
/// **The size is held WITH the position, and that is not tidiness.** A
/// position is only a position for a size: `position_for` centres one inside
/// the other, so holding half of a pair computed together is how a window
/// ends up off-centre when the other half moves. And now that a face's size
/// follows the screen it opens on ([`face_window_size`]), re-deriving it
/// mid-life would send an `InnerSize` that throws away whatever the user did
/// with the resize grip.
///
/// **`what` is WHICH window this is**, because every dialog shares one
/// viewport id and one slot in the app: choosing a pedal from a face replaces
/// the dialog with no undrawn frame in between, and the window that arrives
/// must not inherit the rectangle of the one that left. The title is what the
/// window says it is, so the title is where identity starts.
///
/// **And the title is not enough on its own, because titles are not
/// unique.** `Dialog::Cheated` is called "Tangent" and so is a face whose
/// unit the suite table has never heard of; a `FileBrowser` is called
/// whatever the arm that opened it says. Two windows sharing a name is not a
/// bug, but a window inheriting the SIZE of a different window that shares
/// its name is: the second one came here with a size its caller had just
/// worked out, and handing back the first one's threw that away. So the ask
/// is part of the identity. A window whose caller wants a different size
/// from the one being held is not the window being held, whatever it is
/// called, and it is born rather than inherited.
///
/// **The monitor is held too, and for the opposite reason.** A size that
/// changes because the DISPLAY changed is not a different window — it is the
/// same window on a different screen, and re-deriving it there would send an
/// `InnerSize` that throws away whatever the user did with the resize grip.
/// But neither may it simply be left alone: a window placed on a display that
/// is then unplugged is stranded off-screen with nothing to rescue it, which
/// is the one job the per-frame clamp used to do for free. So a monitor
/// change re-VALIDATES rather than re-derives — the size capped to what the
/// new screen can hold, the position clamped back onto it, and nothing else
/// touched. It happens on the frame the screen changes and on no other, which
/// is the difference between rescuing a window and re-placing one.
///
/// Let go by the same one-pass liveness `shell::surface` uses for a window's
/// birth, so closing a dialog and opening it again centres afresh — which is
/// the reveal this whole rule turns on. Not drawing a surface is what
/// destroys it, here as there.
///
/// **A window with no position has not been placed, and nothing is held for
/// it.** `position_for` answers `None` only for a host that has told us
/// nothing at all — no parent and no monitor — which on the desktop is the
/// first frame or two of a launch, and the welcome card is the one dialog
/// that opens inside them. Holding that `None` would leave the first thing
/// anybody ever sees wherever the platform felt like putting it, which is
/// the exact bug `position_for`'s own note is about. So `None` keeps asking,
/// and the first real answer is the one that sticks.
fn held_geometry(
    ctx: &egui::Context,
    id: &str,
    what: &str,
    placement: Placement,
    asked: Vec2,
) -> (Vec2, Option<Pos2>) {
    let slot = egui::Id::new((id, "geometry"));
    let pass = ctx.cumulative_pass_nr();
    let key = egui::Id::new(what);
    let held = ctx
        .memory(|m| m.data.get_temp::<Held>(slot))
        .filter(|h| pass.saturating_sub(h.pass) <= 1 && h.what == key);
    // Every arm answers with all four — what the window IS, and the two
    // inputs it is being held against — because carrying the wrong pair
    // forward is how a window forgets which screen it was placed on.
    let born = || (asked, placement.position_for(asked), placement.monitor, asked);
    let (size, pos, screen, ask) = match (held, placement.monitor) {
        // Nothing alive under this name: a birth.
        (None, _) => born(),
        // **A frame that says nothing about the display says nothing.** The
        // host can go quiet for a frame around a hide or a Space switch, and
        // one silent frame is neither a display change nor a new window. The
        // window keeps what it has AND keeps remembering the screen it was
        // placed on: forgetting that would make the display coming back look
        // like a different one, and the size derived on it look like some
        // other window's.
        (Some(h), None) => (h.size, h.pos, h.monitor, h.asked),
        // A screen arriving where there has never been one is not a display
        // change either — nothing was ever really placed against a screen,
        // so this is the first frame that can place it.
        (Some(h), Some(_)) if h.monitor.is_none() => born(),
        // **The same window, on the same screen.** Whatever it has become is
        // what it is, and nothing here has anything to say about it.
        (Some(h), m) if h.monitor == m && h.asked == asked => (h.size, h.pos, m, h.asked),
        // The same NAME on the same screen, asking to be a different size:
        // two windows that share a title, and this is the second of them.
        (Some(h), m) if h.monitor == m => born(),
        // **The screen moved out from under it.** Keep the size — a hand may
        // be on the grip — and put the window back where it can be seen. The
        // ask carried forward is THIS frame's, worked out on the new screen,
        // so every frame after it takes the same-window arm above.
        (Some(h), m) => {
            let size = crate::settings::clamp_size_to_monitor(h.size, m);
            (
                size,
                h.pos.map(|p| clamp_into(p, size, placement.screen())),
                m,
                asked,
            )
        }
    };
    if pos.is_some() {
        ctx.memory_mut(|m| {
            m.data.insert_temp(
                slot,
                Held {
                    pass,
                    what: key,
                    asked: ask,
                    monitor: screen,
                    size,
                    pos,
                },
            );
        });
    }
    (size, pos)
}

/// What [`held_geometry`] keeps between frames. `asked` and `monitor` are
/// the two inputs it has to be able to notice a change in; `size` and `pos`
/// are the answer it is holding on to.
#[derive(Clone, Copy)]
struct Held {
    pass: u64,
    what: egui::Id,
    asked: Vec2,
    monitor: Option<Vec2>,
    size: Vec2,
    pos: Option<Pos2>,
}

fn show_dialog_viewport(
    ctx: &egui::Context,
    placement: Placement,
    title: &str,
    size: Vec2,
    min_size: Vec2,
    content: impl FnMut(&mut egui::Ui, &mut VpResult),
) -> VpResult {
    let mut content = content;
    let mut result = VpResult {
        close: false,
        cycle_theme: false,
    };

    // Window or layer — `shell::surface` decides, and it is the only thing in
    // this crate that does. Everything below is what a dialog adds on top:
    // rounded fill on the inline path, because a layer has no title bar and no
    // window edge to tell it apart from the app behind it.
    let inline = !placement.caps.child_windows;
    // **A plugin window or a question.** The plugin windows (a face, the
    // Keys panel, their pickers) wear the in-house bar and leave the desk
    // live; everything else is modal and wears the OS's chrome. Both facts
    // come from the one `Dialog::is_modal`, carried here in `chrome`.
    let plugin = placement.chrome.is_some();
    // **Where it opens, decided once and then left alone** — see
    // [`held_geometry`], which is where the whole argument lives. The size
    // comes back through the same door because a position is only ever a
    // position for a size.
    let (size, pos) = held_geometry(ctx, DIALOG_ID, title, placement, size);
    let spec = crate::shell::SurfaceSpec {
        id: DIALOG_ID,
        title,
        size,
        // Never larger than the window we just asked for. The arms name a
        // floor the layout is designed to, and a face's size now follows the
        // screen it opens on — on a display too small to hold that floor, a
        // minimum bigger than the size is a window that cannot be placed.
        min_size: min_size.min(size),
        // The modal dialogs are MODAL — `handle_main_interaction` drops every
        // event while one is open — so a dialog that ends up behind the main
        // window is not merely untidy, it is an app that has stopped
        // responding with no visible reason why. The welcome note and the
        // supporter key are the two that open on top of a window the user is
        // already looking at. A plugin window floats for a different reason:
        // it is the thing being worked in, and it must not vanish behind the
        // desk it is being worked against.
        pos,
        decorated: !plugin,
        resizable: true,
        takes_focus: true,
        raise: placement.raise,
        modal: !plugin,
        // **A question comes forward over everything of ours, including the
        // render window.** `handle_main_interaction` drops every event on the
        // main window while a modal dialog is open, so a question that ends
        // up underneath something is an app frozen with nothing on screen
        // saying why. A plugin window asks nothing and is left on the
        // ordinary rung, where the export window may hold it down.
        tier: if plugin {
            crate::shell::SurfaceTier::Ordinary
        } else {
            crate::shell::SurfaceTier::Asking
        },
        // Down for as long as the OS's own panel is up. See `on_top`.
        on_top: !placement.native_panel_up,
        // Without this a tiling WM (i3, sway) TILES the dialog — it opens as
        // a pane underneath the floating main window, invisible, while
        // `handle_main_interaction` drops every event waiting on it. That is
        // an app that has frozen for no visible reason, which is exactly what
        // the comment above `pos` warns about.
        window_type: Some(egui::X11WindowType::Dialog),
        chrome: placement.chrome,
        ..Default::default()
    };
    let report = crate::shell::surface(ctx, placement.caps, &spec, &mut |ui, want_close| {
        if inline {
            ui.painter()
                .rect_filled(ui.max_rect(), 4.0, ui.style().visuals.window_fill);
        }
        let mut r = VpResult {
            close: false,
            cycle_theme: false,
        };
        content(ui, &mut r);
        if r.close {
            *want_close = true;
        }
    });
    result.close = report.close;
    result.cycle_theme = report.cycle_theme;
    result
}

/// Whether the pointer is over the dialog drawn IN THE CANVAS last pass.
///
/// The app's raw-pointer handlers ask this before touching the desk under a
/// non-modal dialog: egui's layers shield its own widgets, but a handler
/// that reads `i.pointer` directly sees straight through an `Area`. On the
/// desktop the dialog is another window and this is always false — its
/// presses never reach the main window's input at all.
pub fn pointer_over_dialog(ctx: &egui::Context) -> bool {
    crate::shell::inline_rect(ctx, DIALOG_ID)
        .zip(ctx.pointer_interact_pos())
        .is_some_and(|(r, p)| r.contains(p))
}

/// The same question about the EXPORT window, which is not a dialog and so
/// is not covered by [`pointer_over_dialog`].
///
/// `shell::surface` makes every inline surface swallow what lands on it, so
/// every widget in the app is already shielded. This is the other half, for
/// the handlers that read `i.pointer` straight off the input and see through
/// any layer there has ever been — the piano's own note press is one, and
/// the recorder band's hit test is another. On the desktop the export window
/// is a window of its own and its presses never reach the main window at
/// all, so this is false there, which is the honest answer.
pub fn pointer_over_export(ctx: &egui::Context) -> bool {
    inline_export_rect(ctx)
        .zip(ctx.pointer_interact_pos())
        .is_some_and(|(r, p)| r.contains(p))
}

/// Where the export window was drawn IN THE CANVAS last pass, or `None` on the
/// desktop, where it is a window of its own.
///
/// **The one place that knows the export window's id**, which is why this is
/// public rather than the id being. The app has to be able to ask about this
/// window — see `App::dialog_blocks_pointer` — and a caller that reached for
/// `shell::inline_rect` with an id of its own spelling would go on answering
/// `None` for ever the day this one changed.
pub fn inline_export_rect(ctx: &egui::Context) -> Option<Rect> {
    crate::shell::inline_rect(ctx, EXPORT_ID)
}

/// Whether the dialog was drawn IN THE CANVAS last pass, sharing the main
/// window's keyboard. Inline the keys have one owner and it is the layer on
/// top; on the desktop each window hears only its own keys, so this is
/// false and focus decides.
pub fn dialog_drawn_inline(ctx: &egui::Context) -> bool {
    crate::shell::inline_rect(ctx, DIALOG_ID).is_some()
}

/// Render the active dialog. Returns an action for the app to apply;
/// `dialog_opt` becomes None when the dialog closes.
/// An ellipsis in the middle of a long path, keeping both ends readable.
fn shorten_middle(s: &str, max: usize) -> String {
    let n = s.chars().count();
    if n <= max {
        return s.to_owned();
    }
    let keep = max.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().skip(n - keep).collect();
    format!("{head}\u{2026}{tail}")
}

// ── the export window ───────────────────────────────────────────────────────

/// The export window's own identity, apart from [`DIALOG_ID`].
///
/// **It used to share the dialog's window, and that cost it twice.**
///
/// A dialog opened during a render drew INTO THE SAME WINDOW, alternately
/// with the waveform: two `show_viewport_immediate` calls under one
/// `ViewportId` in one frame are one window asked to be two things.
///
/// And the export window was never brought to the front. Taking over from
/// the Export dialog's own window, `shell::surface` saw a surface that
/// already existed — `existing = true`, so no reveal — and emitted only
/// patch commands: a Title, an InnerSize, a re-centre. Never a
/// `Visible(true)`, which is the one command that reaches
/// `makeKeyAndOrderFront:`. The window that is the entire reason to watch a
/// render was the one window that never came forward, and anything born
/// after it — the band's "Rendering at full speed…" notice, chief among
/// them — landed straight on top.
///
/// An id of its own gives it its own birth, its own reveal and its own
/// remembered geometry. Nothing else keyed off `DIALOG_ID` on its behalf:
/// `pointer_over_dialog` and `dialog_drawn_inline` are both gated on a
/// `Dialog` being open, which an export is not.
const EXPORT_ID: &str = "ivory-export";

/// **How much of the screen the export window takes.**
///
/// A share rather than a count of points, for the reason `app::main_width`
/// gives about the main window: 520 by 210 is a reasonable window on the
/// display this app was written on and a postage stamp on a 5K, so the
/// percentage was a percentage of nothing in particular. The waveform is
/// the whole point of this window — it is what somebody watches for the
/// minutes a render takes — so it is sized to the room the machine has.
///
/// Wider than it is tall because the wave runs across time, and under half
/// the screen in both directions because the desk behind it is still worth
/// seeing.
const EXPORT_SHARE: Vec2 = Vec2::new(0.46, 0.40);

/// **The floor.** Below this the wave stops being worth watching, which is
/// the only reason the window exists — and it is already well over twice
/// the area of the 520 by 210 this window opened at for years.
const EXPORT_MIN: Vec2 = Vec2::new(560.0, 300.0);

/// **The ceiling.** A share with no cap is a window that eats a 6K display
/// to draw one waveform; past about this size the wave gains resolution
/// nobody is reading and the desk loses the room it is being rendered from.
const EXPORT_MAX: Vec2 = Vec2::new(1200.0, 620.0);

/// The export window's size on this display — see [`EXPORT_SHARE`].
///
/// `monitor` is whatever [`Placement`] was handed: the screen on the
/// desktop, the plugin editor's own canvas inline, and the same share is
/// the right answer to both.
///
/// The rule itself is [`crate::shell::share_of_screen`] and lives there
/// rather than here: a share of the display with a floor, a ceiling and a
/// last clamp inside the screen is what every window of this app wants, and
/// it was written out twice before anybody noticed. This function is now
/// only the export window's four numbers.
pub fn export_window_size(monitor: Option<Vec2>) -> Vec2 {
    crate::shell::share_of_screen(monitor, EXPORT_SHARE, EXPORT_MIN, EXPORT_MAX)
}

/// The smallest the window may be dragged to, which is the floor — except
/// on a display too small to hold even that, where a minimum larger than
/// the size we just asked for would be a window that cannot be placed.
fn export_min_size(size: Vec2) -> Vec2 {
    EXPORT_MIN.min(size)
}

// ── how big a suite face opens ──────────────────────────────────────────────

/// **How big a plugin face opens, per unit: a share, a floor and a ceiling.**
///
/// The owner: "Make default plugin window size a bit bigger." These were five
/// fixed point counts, and a fixed point count is a percentage of nothing in
/// particular — the same thing `app::main_width` and [`EXPORT_SHARE`] say
/// about the windows they size. 400 by 360 is a comfortable panel on the
/// display this file was written on, a postage stamp on a 5K and, on a small
/// laptop, most of the height there is.
///
/// So each face keeps the shape it was drawn at and gets the room the machine
/// has:
///
/// * **the floor is exactly the size it opened at before**, so nothing this
///   change touches is ever smaller than it is today, and a genuinely small
///   screen gets the layout the panel was designed to;
/// * **the share** is set so a 14-inch laptop — the small screen this has to
///   stay sane on — lands about a third larger in each direction, which is
///   half again the area; and
/// * **the ceiling is the floor scaled by 1.6 in both directions**, so the
///   proportions survive the cap. A share with no cap is a knob panel eating
///   a 6K, and a face is a fixed set of controls: past a point it gains air
///   rather than anything to read.
///
/// The shares are given rather than derived because the faces are not one
/// shape: the amp is wide, the imager is tall, the EQ is square, and each was
/// laid out that way on purpose. What is shared is the RULE —
/// [`crate::shell::share_of_screen`], the same one the main window and the
/// export window use, floor and ceiling and a last clamp inside the screen.
///
/// [`FACE_IR`] is the exception and says why in its own note.
const FACE_SIZES: &[(&str, Vec2, Vec2, Vec2)] = &[
    // unit, share of the screen, floor, ceiling
    ("@tangent-amp", Vec2::new(0.464, 0.477), Vec2::new(540.0, 360.0), Vec2::new(864.0, 576.0)),
    ("@tangent-eq", Vec2::new(0.404, 0.622), Vec2::new(470.0, 470.0), Vec2::new(752.0, 752.0)),
    ("@tangent-imager", Vec2::new(0.292, 0.635), Vec2::new(340.0, 480.0), Vec2::new(544.0, 768.0)),
    ("@tangent-comp", Vec2::new(0.344, 0.569), Vec2::new(400.0, 430.0), Vec2::new(640.0, 688.0)),
];

/// Every other face: the shape the knob rows were laid out in.
const FACE_DEFAULT: (Vec2, Vec2, Vec2) = (
    Vec2::new(0.344, 0.477),
    Vec2::new(400.0, 360.0),
    Vec2::new(640.0, 576.0),
);

/// **The IR face is a drumpad-sized window**, because it is about to be one.
///
/// The face is growing a file browser down its right half, which makes it the
/// same kind of window as the sixteen-pad sampler: a machine on the left, a
/// list of files on the right, and a list wants rows more than it wants
/// anything else. So it is sized the drumpad's way rather than the other
/// faces' — `drumpad_panel::place` takes four fifths of the screen's WIDTH,
/// floors it at 600 so neither half collapses and caps it at 1160 so it does
/// not eat a display, and derives its height from what is inside rather than
/// from the shape of a knob panel.
///
/// The numbers here are this window's own and not the drumpad's: the height
/// share is set so the browser gets more rows than the drumpad's pad grid
/// needs, and the ceiling is the height that share reaches on the width
/// ceiling. What is followed is the derivation — four fifths, a floor that
/// keeps both halves usable, a cap — and not a measurement lifted from
/// another panel, which would go stale the day the drumpad's own layout
/// moves.
const FACE_IR: (Vec2, Vec2, Vec2) = (
    Vec2::new(0.80, 0.66),
    Vec2::new(600.0, 400.0),
    Vec2::new(1160.0, 620.0),
);

/// The face window for `unit` on this display. See [`FACE_SIZES`].
///
/// `monitor` is whatever [`Placement`] was handed — the screen on the
/// desktop, the plugin editor's own canvas inline — and `None` on the frames
/// before anything has been measured, where `share_of_screen` answers with
/// the floor, which is the size these faces opened at for their whole life so
/// far.
pub fn face_window_size(unit: &str, monitor: Option<Vec2>) -> Vec2 {
    let (share, floor, ceiling) = if unit == "@tangent-ir" {
        FACE_IR
    } else {
        FACE_SIZES
            .iter()
            .find(|(u, ..)| *u == unit)
            .map_or(FACE_DEFAULT, |&(_, share, floor, ceiling)| {
                (share, floor, ceiling)
            })
    };
    crate::shell::share_of_screen(monitor, share, floor, ceiling)
}

/// **The smallest a face may be dragged to.**
///
/// A face is a fixed set of controls in a fixed arrangement, so its minimum
/// is a property of the layout and not of the screen — 320 by 300 is what the
/// knob rows need, and it has been the answer for every face since there were
/// faces. The IR window is the exception for the same reason it is sized
/// differently: two halves, one of them a list, and a window dragged down to
/// a knob panel's minimum would leave the browser a column of ellipses.
///
/// Capped by the size actually asked for, because on a display too small to
/// hold the floor a minimum larger than the window is a window that cannot be
/// placed. [`show_dialog_viewport`] applies that cap to every dialog; this
/// says the same thing at the one call site where the two numbers are far
/// enough apart to matter.
pub(crate) fn face_min_size(unit: &str, size: Vec2) -> Vec2 {
    // **A face's own floor IS its minimum**, because the floor is the size it
    // was laid out in - the size it opened at for its whole life before the
    // windows started following the screen. A flat 320x300 for everything let
    // the EQ be dragged 129 points shorter than its own rows, the Imager 99
    // and the amp 94, and a face has no scroll area under it on the desktop
    // path, so what went past the bottom was drawn, clipped and unreachable:
    // the owner's cut-off complaint wearing its other edge.
    face_window_size(unit, None).min(size)
}

/// The window's margin, the label's line, the bar's thickness, and the air
/// between them: everything the wave does NOT get. See [`export_layout`].
const EXPORT_PAD: f32 = 14.0;
const EXPORT_LABEL_H: f32 = 18.0;
const EXPORT_BAR_H: f32 = 12.0;
const EXPORT_GAP: f32 = 10.0;

/// **Where the export window's parts are.**
///
/// One function for the whole window, so a size that now follows the
/// monitor cannot drift the wave off the bar or the playhead off the wave.
/// The parts were three `allocate_exact_size` calls in a layout that only
/// happened to agree with itself at one window size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ExportLayout {
    /// The line of words: what is happening, and how far in.
    pub label: Rect,
    /// **The point of the window**, and everything left over once the
    /// furniture above has been paid for.
    pub wave: Rect,
    /// The progress bar under the wave.
    pub bar: Rect,
}

impl ExportLayout {
    /// The playhead's x at `fraction`.
    ///
    /// Measured on the WAVE, which is the only rect it may ever be measured
    /// on: the peak columns behind it are indexed across the same width, so
    /// a playhead taken from anything else drifts away from the sample it
    /// claims to be pointing at.
    pub fn playhead(&self, fraction: f32) -> f32 {
        self.wave.left() + self.wave.width() * fraction.clamp(0.0, 1.0)
    }
}

/// The window's parts, from the one rect it is handed.
///
/// **The furniture is paid first and the wave gets the rest.** That is the
/// order that makes the window worth growing: every point the window gains
/// goes to the waveform, and the label and the bar stay the size a label
/// and a bar should be. Each part is floored at nothing so a window
/// squeezed under its own minimum degrades instead of inverting.
///
/// **What that is worth, in points.** The wave was a hard-coded 96 in a 520
/// by 210 window. It is 222 at [`EXPORT_MIN`], 242 on a 1280 by 800 laptop,
/// 482 on the owner's 2560 by 1400 reference display, and 542 at
/// [`EXPORT_MAX`] — which needs 1550 points of screen height, so 542 is the
/// CEILING's number and not the reference display's. The commit that shipped
/// this window quoted 542 for the reference display, which is where those
/// two got crossed; `the_export_window_is_a_share_of_the_screen_between_a_
/// floor_and_a_ceiling` now asserts all four.
pub fn export_layout(rect: Rect) -> ExportLayout {
    let inner = rect.shrink(EXPORT_PAD);
    let w = inner.width().max(0.0);
    let h = inner.height().max(0.0);
    let label_h = EXPORT_LABEL_H.min(h);
    let bar_h = EXPORT_BAR_H.min(h - label_h);
    let gaps = (2.0 * EXPORT_GAP).min(h - label_h - bar_h);
    let wave_h = (h - label_h - bar_h - gaps).max(0.0);
    let gap = gaps * 0.5;
    let label = Rect::from_min_size(inner.min, Vec2::new(w, label_h));
    let wave = Rect::from_min_size(
        Pos2::new(inner.left(), label.bottom() + gap),
        Vec2::new(w, wave_h),
    );
    let bar = Rect::from_min_size(
        Pos2::new(inner.left(), wave.bottom() + gap),
        Vec2::new(w, bar_h),
    );
    ExportLayout { label, wave, bar }
}

/// **The playhead's red, kept findable on the well it is drawn on.**
///
/// `#E83A4E` has a relative luminance of 0.207 — inside the 0.12–0.22 dead
/// zone this app has learned about from a screenshot seven times, the band
/// where neither ink wins. The sweep dresses nineteen factory themes in two
/// modes against the two faces this window can read, which is seventy-six
/// dressings, and the shipped red comes out under 3:1 on SEVEN of them: as
/// low as 2.43:1 on Sandstone dark's pale well, and short on Default,
/// Centennial and Porcelain too. A red line on a cream ground that the eye
/// has to hunt for. That was survivable while the wave was 96 points tall in
/// the corner of a small window. It is not now that the wave IS the window.
///
/// So the red is moved off the well by the house helper every other surface
/// here uses, at the NON-TEXT floor of 3:1 — this is a one-point marker and
/// not a word, and dragging a line this thin to 4.5:1 would take the red out
/// of the red. Hand-rolling the walk was the mistake the first draft made:
/// [`crate::recorder_panel::distinct_surface_at`] is the same rule, already
/// measured on the surface rather than guessed from a role, and it is what
/// keeps this line honest when the palette moves under it.
///
/// In practice it leaves sixty-nine of the seventy-six dressings on the
/// shipped red untouched, and at its furthest lands on `#C43142`, so the
/// playhead is plainly red everywhere and findable everywhere.
fn playhead_ink(well: Color32) -> Color32 {
    const RED: Color32 = Color32::from_rgb(0xE8, 0x3A, 0x4E);
    /// The non-text floor: a marker, not a word.
    const FLOOR: f32 = 3.0;
    crate::recorder_panel::distinct_surface_at(RED, well, FLOOR)
}

/// **Where the export window opens, and how big — decided once, then left
/// alone.**
///
/// This window is where the rule was learned. It walked back to the middle of
/// the main window every time the main window moved and could not be dragged
/// aside at all, and it is up for the length of a render, which is exactly
/// long enough to want it somewhere else. [`held_geometry`] is that fix, now
/// serving every floating surface in this file; the four numbers below it are
/// all this window still owns.
///
/// One window, one name for it, and the name never changes: the identity
/// [`held_geometry`] keys on is the id rather than the title, because the
/// title says which pass of the render it is on.
fn export_geometry(ctx: &egui::Context, placement: Placement) -> (Vec2, Option<Pos2>) {
    held_geometry(
        ctx,
        EXPORT_ID,
        EXPORT_ID,
        placement,
        export_window_size(placement.monitor),
    )
}

/// The export window: a wave that draws itself while the render walks the
/// timeline, and a bar under it.
///
/// **Not a dialog.** It answers no question, it does not take the one dialog
/// slot, and it cannot be dismissed - it goes when the export goes. It floats
/// over the main window because that is where an eye already is, and because
/// an export is the one thing in this app that can run long enough for a
/// person to wonder whether it is running at all.
///
/// **Nothing of this app's that merely ANNOUNCES may cover it while it is
/// up** ([`crate::shell::SurfaceTier::Uncovered`]). It was losing the reveal
/// race to every sibling — see [`EXPORT_ID`] for why it was losing it worse
/// than anything else — and a progress window you cannot see is a progress
/// window that does not exist.
///
/// **A QUESTION still comes forward, and must.** A modal dialog opened
/// mid-render sits in front of this window, because the app drops every
/// main-window event while one is open: a question held down under the
/// waveform would be an invisible question with the app frozen behind it,
/// which is worse than the covering. See [`crate::shell::SurfaceTier`].
///
/// **What that used to cost the band's notice, and no longer does.** A
/// message raised during a render — a camera warming up, a device gone — was
/// a window of its own, an announcement, so it lost the ladder; and since
/// both windows were centred on the main window with this one the bigger, it
/// was not merely behind but entirely hidden. That window is gone. Everything
/// the app SAYS is now `crate::notify`'s column, painted inside the main
/// window on `Order::Foreground`, which is not in this ladder at all and
/// cannot lose to anything in it: it is over the desk, this window floats
/// over the desk beside it, and neither can hide the other. The z-order fight
/// a message could not win is a fight it no longer enters — which is the
/// reason that module exists, and is written up in its own header.
///
/// # Checking the z-order by hand
///
/// None of this can be proved by a unit test: a window manager has the last
/// word and nothing headless may ask it. The tests below prove the app asks
/// for the right levels; the following is what the answers look like on a
/// screen, and it is the list to walk before shipping a change to
/// [`crate::shell::SurfaceTier`].
///
/// 1. **The render is the biggest thing on the screen.** Start an export of
///    something a minute or two long. The window opens centred, in front of
///    the desk, with the waveform drawing itself left to right.
/// 2. **Drag it aside.** It stays where it is put, for the whole render, and
///    does not walk back to the middle when the main window is moved.
/// 3. **A message does not fight this window any more.** With the render
///    running, do something that says something (starting the camera is the
///    easy one). It appears in the notification column in the top-left corner
///    of the main window, NOT as a window, and it neither covers the waveform
///    nor is covered by it. Move the export window over the column: the
///    column is inside the main window, so the export window passes in front
///    of it, and the message is still there when the render ends.
/// 4. **A question wins.** With the render still running, open Setup from
///    the menu. Setup comes to the FRONT of the export window. Click in it,
///    type in it, change a device: it responds. The waveform is still
///    drawing where the two do not overlap.
/// 5. **A question that got lost comes back.** With Setup open and the
///    render running, click the main window. The desk does not respond (it
///    is waiting on Setup) and Setup comes forward again rather than the
///    click going nowhere.
/// 6. **Close Setup.** The export window is back in front of everything
///    within a frame or two, still drawing.
/// 7. **The plugin path.** Open the suite plugin in a host, start an export
///    from it, and check the same things in the canvas: the window is drawn
///    over the piano and the recorder band, the canvas behind it is NOT
///    dimmed (it is reporting, not asking), a click on the window's face
///    does not operate a control underneath it, a message raised mid-render
///    appears in the corner column rather than as a second window, and a
///    dialog opened from the menu paints OVER it and can be used — including
///    its dropdowns and its hover text, which is what keeps a question in the
///    band its own popups are drawn in.
/// 7a. **The raw-pointer hole, now closed.** Inline, `shell` swallows what
///    lands on a surface at egui's hit test, which covers every widget and
///    the main background's own response. The handlers that read `i.pointer`
///    directly — the piano's note press, the recorder band's hit test — are
///    gated by `App::dialog_blocks_pointer`, which used to answer only about
///    a `Dialog`, so a click on the waveform sounded a note in the plugin.
///    That function now asks [`pointer_over_export`] first. Press the middle
///    of the window and listen: nothing.
/// 8. **A native file panel beats everything.** Start an export, then open
///    any "Save as…" — the OS panel is in front of the export window, which
///    is what `on_top: false` is for.
pub fn show_export_progress(
    ctx: &egui::Context,
    p: &crate::ports::ExportProgress,
    placement: Placement,
) {
    // The desk's own face, like every dialog — read from the frame's record
    // rather than taken as a parameter, because this window does not come
    // through `show`. It hard-coded ivory here long after the dialogs
    // learned to follow the theme, and it is on screen for minutes at a
    // stretch. See `remember_face`.
    let t = theme(live_face(ctx));
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    let inline = !placement.caps.child_windows;
    // Size and position together, and frozen together. See `export_geometry`.
    let (size, pos) = export_geometry(ctx, placement);
    let spec = crate::shell::SurfaceSpec {
        id: EXPORT_ID,
        title: "Exporting",
        size,
        min_size: export_min_size(size),
        pos,
        decorated: true,
        resizable: true,
        // It comes forward when it opens, because it opens the instant the
        // Export dialog it replaces goes away and the eye is already there.
        takes_focus: true,
        // **Not modal.** It blocks nothing and dims nothing: the desk stays
        // live under a render, which is the whole reason this is not a
        // `Dialog`. It went through `show_dialog_viewport` before, which
        // decides modality from `chrome` and therefore made it modal inline
        // — a plugin canvas greyed out behind a window that was reporting,
        // not asking.
        //
        // Not modal is not the same as transparent to a press: `shell`
        // swallows what lands on any inline surface whether or not it dims,
        // which is where the scrim's one useful job went. See the allocation
        // in `shell::surface`'s inline path.
        modal: false,
        // Down for as long as the OS's own panel is up, like every other
        // surface here. See `SurfaceSpec::on_top`.
        on_top: !placement.native_panel_up,
        // Watched, not asking: it outranks every announcement of ours and
        // stands down for a question. See `SurfaceTier`.
        tier: crate::shell::SurfaceTier::Uncovered,
        // A tiling WM would otherwise TILE it into the layout, which for a
        // window whose only job is to be looked at is the same failure as
        // being covered.
        window_type: Some(egui::X11WindowType::Dialog),
        ..Default::default()
    };
    // **The report is dropped on purpose.** Escape, the title bar's cross
    // and a click outside all arrive here, and none of them may take this
    // window down: it goes when the export goes, and an export that is
    // still running with nothing on screen saying so is exactly what this
    // window exists to prevent. That was its contract before and it is its
    // contract now.
    let _ = crate::shell::surface(ctx, placement.caps, &spec, &mut |ui, _want_close| {
        apply_theme(ui.style_mut(), &t);
        let rect = ui.max_rect();
        let painter = ui.painter_at(rect);
        // Rounded inline, where there is no window edge to tell the surface
        // apart from the app behind it; square in a window of its own.
        painter.rect_filled(rect, if inline { 4.0 } else { 0.0 }, t.bg);
        let l = export_layout(rect);
        painter.text(
            l.label.left_center(),
            Align2::LEFT_CENTER,
            format!("{}  {:.0}%", p.label, p.fraction * 100.0),
            bold(13.0),
            t.text,
        );
        // ── the wave ──────────────────────────────────────────────────────
        let wave = painter.with_clip_rect(l.wave);
        wave.rect_filled(l.wave, 2.0, t.field_bg);
        // **A hairline round the well**, because on three of the seventy-six
        // dressings the well and the window's ground are within 1.1:1 of
        // each other (Porcelain light, Phosphor dark and Silverface dark,
        // each on the strip face) and the waveform was floating in a
        // rectangle with no findable edge. Another twelve sit under 1.15:1,
        // so this is not one theme being awkward. The
        // surface cannot move — it is the ground the wave is measured
        // against — so the box gets a line instead, in the theme's own edge
        // shade. See `every_theme_dresses_the_export_window`.
        wave.rect_stroke(
            l.wave,
            2.0,
            Stroke::new(1.0_f32, t.button_border),
            egui::StrokeKind::Inside,
        );
        let mid = l.wave.center().y;
        // The zero line, so a silent passage still reads as rendered rather
        // than as nothing drawn yet.
        wave.line_segment(
            [
                Pos2::new(l.wave.left(), mid),
                Pos2::new(l.wave.right(), mid),
            ],
            Stroke::new(1.0_f32, t.text.gamma_multiply(0.25)),
        );
        let cols = p.peaks.len().max(1);
        let half = l.wave.height() * 0.5 - 2.0;
        // **A column is as wide as its share of the wave**, not one point.
        // The feed is a fixed 480 columns (`ports::EXPORT_PEAKS`), which was
        // about one point each in the 496-point wave this window used to
        // have; across a wave three times that wide, one-point strokes draw
        // a comb with two points of ground showing between every pair of
        // samples. Floored at a point so a narrow window still draws
        // something rather than hairlines with gaps.
        let col_w = (l.wave.width() / cols as f32).max(1.0);
        for (i, (lo, hi)) in p.peaks.iter().enumerate() {
            if *lo == 0.0 && *hi == 0.0 {
                continue;
            }
            let x = l.wave.left() + l.wave.width() * (i as f32 + 0.5) / cols as f32;
            let top = mid - hi.clamp(-1.0, 1.0) * half;
            let bot = mid - lo.clamp(-1.0, 1.0) * half;
            wave.line_segment(
                [Pos2::new(x, top), Pos2::new(x, bot.max(top + 1.0))],
                Stroke::new(col_w, t.text),
            );
        }
        // The playhead: where the render has got to.
        let x = l.playhead(p.fraction);
        wave.line_segment(
            [Pos2::new(x, l.wave.top()), Pos2::new(x, l.wave.bottom())],
            Stroke::new(1.0_f32, playhead_ink(t.field_bg)),
        );
        // ── the bar ───────────────────────────────────────────────────────
        // Filled to the SAME x the playhead stands at — the bar and the wave
        // share a left edge and a width, so the two read as one measurement
        // rather than two that nearly agree.
        painter.rect_filled(l.bar, 1.0, t.field_bg);
        let filled = Rect::from_min_max(
            l.bar.min,
            Pos2::new(l.playhead(p.fraction), l.bar.bottom()),
        );
        if filled.width() > 0.5 {
            painter.rect_filled(filled, 1.0, t.text);
        }
    });
}

// ── the IR wizard's four pages ──────────────────────────────────────────────
//
// Free functions rather than arms of the big closure, because each page is a
// small essay and the wizard arm would otherwise be four hundred lines deep
// in one match. They speak through the same two channels every dialog does:
// mutate the boxed state, and put at most one `DialogAction` in `action`.

/// A channel pair the way an interface prints it: one-based, "1/2".
fn ir_pair_label(prefix: &str, pair: (usize, usize)) -> String {
    format!("{prefix} {}/{}", pair.0 + 1, pair.1 + 1)
}

fn draw_ir_io(ui: &mut egui::Ui, t: &DialogTheme, s: &mut IrWizardState) {
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    ui.label(
        RichText::new(
            "Capture a hardware unit as an impulse response: a measurement \
             sweep goes OUT of the interface, through the box, and back IN.",
        )
        .font(bold(10.5))
        .color(t.text),
    );
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Patch the chosen outputs into the unit's inputs and the unit's \
             outputs into the chosen inputs. Set the unit 100% WET - its \
             dry/wet mix becomes part of the capture.",
        )
        .font(bold(10.0))
        .color(t.faint),
    );
    ui.add_space(10.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("SEND ON").font(bold(11.0)).color(t.text));
        egui::ComboBox::from_id_salt("ir-out-pair")
            .selected_text(
                RichText::new(ir_pair_label("Out", s.out_pair()))
                    .font(bold(11.0))
                    .color(t.text),
            )
            .show_ui(ui, |ui| {
                for (i, &pair) in s.out_pairs.iter().enumerate() {
                    if ui
                        .selectable_label(
                            s.out_sel == i,
                            ir_pair_label("Out", pair),
                        )
                        .clicked()
                    {
                        s.out_sel = i;
                    }
                }
            });
    });
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("RETURN ON").font(bold(11.0)).color(t.text));
        egui::ComboBox::from_id_salt("ir-in-pair")
            .selected_text(
                RichText::new(ir_pair_label("In", s.in_pair()))
                    .font(bold(11.0))
                    .color(t.text),
            )
            .show_ui(ui, |ui| {
                for (i, &pair) in s.in_pairs.iter().enumerate() {
                    if ui
                        .selectable_label(s.in_sel == i, ir_pair_label("In", pair))
                        .clicked()
                    {
                        s.in_sel = i;
                    }
                }
            });
    });
    if s.in_pairs.is_empty() {
        ui.add_space(8.0);
        ui.label(
            RichText::new(
                "No audio input is open - pick an input device first \
                 (Setup, or the microphone picker).",
            )
            .font(bold(10.0))
            .color(t.text),
        );
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new(
            "Sending on the main monitor pair works but you will hear the \
             sweep at full level - a spare pair of jacks is kinder.",
        )
        .font(bold(9.5))
        .color(t.faint),
    );
}

/// The send levels the wizard offers, dBFS, quietest first.
///
/// **This row is the whole fix for "the levels section is limited at around
/// −20 dBFS and that readout is just hitting a wall".** There was no row: the
/// only send levels the wizard could produce were −18 and −12, the two
/// alignment tones the references use, and −18 dBFS of sine reads −21 dBFS
/// RMS on the return meter. Turn the box up all you like — the number the
/// meter shows is the number the wizard SENT, and it could not be raised.
///
/// The send level is not a cosmetic choice. It is the ONE lever this loop
/// has: the sweep goes out at exactly this level, so it sets the capture's
/// signal-to-noise ratio, and the recovered IR is honest to about that many
/// dB under its own peak and no further (`sweep::recovered_floor_db`). Every
/// dB of send that a box can take without distorting is a dB of tail.
///
/// −18 and −12 keep their place because they are the alignment convention;
/// the rest exist because a hardware output stage that will not clip until
/// −3 dBFS should be allowed to run there.
pub const IR_SEND_CHOICES: [f32; 6] = [-24.0, -18.0, -12.0, -9.0, -6.0, -3.0];

fn draw_ir_level(
    ui: &mut egui::Ui,
    t: &DialogTheme,
    s: &mut IrWizardState,
    action: &mut Option<DialogAction>,
) {
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    ui.label(
        RichText::new(
            "Set the unit's input level with the tone, then drive the loop as \
             hard as the box takes without distorting. The SWEEP goes out at \
             this same level, and every dB of it is a dB of tail.",
        )
        .font(bold(10.5))
        .color(t.text),
    );
    ui.add_space(8.0);
    // The send level, as a row of choices — the sweep page's own idiom, and
    // the reason it exists is in `IR_SEND_CHOICES`.
    ui.horizontal(|ui| {
        ui.label(RichText::new("SEND").font(bold(11.0)).color(t.text));
        let mut moved = false;
        for &level in IR_SEND_CHOICES.iter() {
            let here = (s.level_db - level).abs() < 0.5;
            let label = if (level + 18.0).abs() < 0.5 || (level + 12.0).abs() < 0.5 {
                // The two alignment levels wear a mark, so the convention is
                // still legible in a row that is now six wide.
                format!("{level:.0}\u{2217}")
            } else {
                format!("{level:.0}")
            };
            if ui
                .selectable_label(here, RichText::new(label).font(bold(11.0)))
                .clicked()
                && !here
            {
                s.level_db = level;
                moved = true;
            }
        }
        ui.label(RichText::new("dBFS").font(bold(10.0)).color(t.faint));
        // A level chosen while the tone sounds re-emits it at once — one
        // action per click, so nothing churns the engine's job slot.
        if moved && s.tone_on {
            *action = Some(DialogAction::IrTone {
                on: true,
                out: s.out_pair(),
                inp: s.in_pair(),
                level_db: s.level_db,
            });
        }
    });
    ui.add_space(4.0);
    ui.label(
        RichText::new("\u{2217} the two alignment levels the references use")
            .font(bold(9.0))
            .color(t.faint),
    );
    ui.add_space(6.0);
    if ui
        .add(Button::new(
            RichText::new(if s.tone_on {
                "Stop the tone \u{25a0}"
            } else {
                "Play the 1 kHz tone"
            })
            .font(bold(11.0))
            .color(t.text),
        ))
        .clicked()
    {
        s.tone_on = !s.tone_on;
        *action = Some(DialogAction::IrTone {
            on: s.tone_on,
            out: s.out_pair(),
            inp: s.in_pair(),
            level_db: s.level_db,
        });
    }
    ui.add_space(10.0);
    // The return meter: a bar on the field ground, RMS as the fill, peak as
    // a tick, −60..0 dBFS. Drawn, not a widget, because it is a readout.
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 20.0),
        egui::Sense::hover(),
    );
    ui.painter().rect_filled(rect, 2.0, t.field_bg);
    let frac = |db: f32| ((db + 60.0) / 60.0).clamp(0.0, 1.0);
    let (rms, peak) = (s.live.rms_db, s.live.peak_db);
    if rms > -200.0 {
        let w = rect.width() * frac(rms);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(w, rect.height())),
            2.0,
            t.faint,
        );
    }
    if peak > -200.0 {
        let x = rect.left() + rect.width() * frac(peak);
        ui.painter().line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
            Stroke::new(2.0, t.text),
        );
    }
    // Where the headroom runs out, so "how much further can I push this" is
    // a distance on the bar rather than an arithmetic problem.
    let gx = rect.left() + rect.width() * frac(-6.0);
    ui.painter().line_segment(
        [Pos2::new(gx, rect.top()), Pos2::new(gx, rect.bottom())],
        Stroke::new(1.0, t.faint),
    );
    ui.add_space(4.0);
    // **PEAK first.** Staging a loop is a peak job — the question is how
    // close to clipping the hottest sample runs — and the RMS of a sine sits
    // 3 dB under its peak by arithmetic, which is what made a −18 dBFS tone
    // read "−21" and look like a wall. Both numbers are here; only one of
    // them is the one to set levels by, and it now says so.
    let reading = if peak <= -200.0 {
        "return: silence".to_owned()
    } else {
        format!(
            "return: {peak:.1} dBFS peak  ({:.1} dB of headroom)   {rms:.1} rms",
            -peak
        )
    };
    ui.label(RichText::new(reading).font(bold(10.5)).color(t.text));
    // The warnings, in the order they matter.
    let warn = if peak > -0.5 {
        Some("CLIPPING - bring the unit's output (or the send) down.")
    } else if peak > -6.0 {
        Some("Under 6 dB of headroom - the sweep will hit harder than this tone.")
    } else if s.tone_on && peak <= -200.0 {
        Some("Nothing is coming back - check the loop's cables and the unit's bypass.")
    } else if s.tone_on && peak < -30.0 {
        // The case the owner hit from the other side: a quiet return is a
        // quiet capture is a short IR, and nothing else in this wizard says
        // so until the tail comes back stubbed.
        Some(
            "Very quiet - raise the SEND, or the unit's own output. A return \
             this low costs tail, not just level.",
        )
    } else {
        None
    };
    if let Some(w) = warn {
        ui.add_space(4.0);
        ui.label(RichText::new(w).font(bold(10.5)).color(t.text));
    }
    ui.add_space(8.0);
    ui.label(
        RichText::new(
            "Aim the return peak between -12 and -6 dBFS. Hardware often keeps \
             less headroom on its wet path than its dry one, so set levels by \
             the box's own meters, below its clip - then take the loudest send \
             that stays clean.",
        )
        .font(bold(9.5))
        .color(t.faint),
    );
}

fn draw_ir_sweep(
    ui: &mut egui::Ui,
    t: &DialogTheme,
    s: &mut IrWizardState,
    action: &mut Option<DialogAction>,
) {
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    ui.horizontal(|ui| {
        ui.label(RichText::new("SWEEP").font(bold(11.0)).color(t.text));
        for (i, secs) in IR_SWEEP_CHOICES.iter().enumerate() {
            if ui
                .selectable_label(
                    s.sweep_choice == i,
                    RichText::new(format!("{secs:.0} s")).font(bold(11.0)),
                )
                .clicked()
            {
                s.sweep_choice = i;
            }
        }
    });
    ui.horizontal(|ui| {
        ui.label(RichText::new("TAIL").font(bold(11.0)).color(t.text));
        for secs in [5.0f64, 10.0, 20.0] {
            if ui
                .selectable_label(
                    (s.tail_s - secs).abs() < 0.5,
                    RichText::new(format!("{secs:.0} s")).font(bold(11.0)),
                )
                .clicked()
            {
                s.tail_s = secs;
            }
        }
    });
    // **The tail is a hard ceiling on the IR, not a suggestion.** The
    // response to the sweep's last sample is only observed for as long as
    // the recording ran on afterwards, so nothing past this window was ever
    // measured and `process_ir_capture` cuts there. Said here because it is
    // the one setting on this page that can silently shorten a capture.
    ui.label(
        RichText::new(
            "The tail is the longest IR this pass can produce - the decay \
             after it was never recorded. Pick at least the decay you expect.",
        )
        .font(bold(9.5))
        .color(t.faint),
    );
    ui.add_space(10.0);
    // The optional exact route: capture the LOOP alone once, and every
    // later pass divides by it — interface, converters and latency cancel.
    let ref_line = if s.live.has_reference {
        "reference held - the next capture divides by it"
    } else {
        "no reference - the capture will use the inverse-filter route"
    };
    ui.label(RichText::new(ref_line).font(bold(10.0)).color(t.faint));
    if !s.live.capturing
        && ui
            .add(Button::new(
                RichText::new("Capture loop-only reference (recommended)")
                    .font(bold(10.5))
                    .color(t.text),
            ))
            .on_hover_text(
                "Bypass the unit (or patch out straight to in), then run \
                 this once. Same sweep, same levels.",
            )
            .clicked()
    {
        // A reference pass produces no result to show, so it does NOT arm
        // the advance — the wizard stays here waiting for the DUT pass.
        s.arm_reference();
        *action = Some(DialogAction::IrCapture {
            out: s.out_pair(),
            inp: s.in_pair(),
            sweep_s: s.sweep_s(),
            tail_s: s.tail_s,
            level_db: s.level_db,
            reference: true,
        });
    }
    ui.add_space(6.0);
    if !s.live.capturing
        && ui
            .add(Button::new(
                RichText::new("ARM - capture through the unit")
                    .font(bold(12.0))
                    .color(t.text),
            ))
            .clicked()
    {
        // THIS is the pass whose result may move the wizard on, and what
        // arming means — the latch, the capture id it is waiting to see
        // move, the cleared name — is `arm_capture`'s, so the button and
        // the test arm through one body.
        s.arm_capture();
        *action = Some(DialogAction::IrCapture {
            out: s.out_pair(),
            inp: s.in_pair(),
            sweep_s: s.sweep_s(),
            tail_s: s.tail_s,
            level_db: s.level_db,
            reference: false,
        });
    }
    if s.live.capturing {
        ui.add_space(8.0);
        let (rect, _) = ui.allocate_exact_size(
            Vec2::new(ui.available_width(), 16.0),
            egui::Sense::hover(),
        );
        ui.painter().rect_filled(rect, 2.0, t.field_bg);
        let w = rect.width() * s.live.progress.clamp(0.0, 1.0);
        ui.painter().rect_filled(
            Rect::from_min_size(rect.min, Vec2::new(w, rect.height())),
            2.0,
            t.faint,
        );
        ui.add_space(4.0);
        if ui
            .add(Button::new(RichText::new("Stop").color(t.text)))
            .clicked()
        {
            *action = Some(DialogAction::IrCancel);
        }
    }
    if !s.live.phase.is_empty() {
        ui.add_space(6.0);
        ui.label(
            RichText::new(s.live.phase.as_str())
                .font(bold(10.5))
                .color(t.text),
        );
    }
}

fn draw_ir_result(
    ui: &mut egui::Ui,
    t: &DialogTheme,
    st: &crate::face::FaceStyle,
    s: &mut IrWizardState,
    action: &mut Option<DialogAction>,
) {
    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
    let Some(r) = s.live.result.clone() else {
        ui.label(
            RichText::new("No capture yet - go back and arm one.")
                .font(bold(10.5))
                .color(t.text),
        );
        return;
    };
    // The picture: the IR's level over time in dB, on the kit's glass.
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 140.0),
        egui::Sense::hover(),
    );
    crate::face::lcd_glass(ui.painter(), rect, st);
    if r.envelope_db.len() > 1 {
        let n = r.envelope_db.len();
        let pts: Vec<Pos2> = r
            .envelope_db
            .iter()
            .enumerate()
            .map(|(i, &db)| {
                let x = rect.left()
                    + rect.width() * i as f32 / (n - 1) as f32;
                let y = rect.top()
                    + rect.height() * (1.0 - ((db + 100.0) / 100.0).clamp(0.0, 1.0));
                Pos2::new(x, y)
            })
            .collect();
        ui.painter().add(egui::Shape::line(
            pts,
            Stroke::new(1.5, crate::face::lcd_ink(st, crate::face::LcdTier::Primary)),
        ));
        // The measured floor, as a grid line the tail can be judged against.
        let fy = rect.top()
            + rect.height()
                * (1.0 - ((r.noise_floor_db + 100.0) / 100.0).clamp(0.0, 1.0));
        ui.painter().line_segment(
            [Pos2::new(rect.left(), fy), Pos2::new(rect.right(), fy)],
            Stroke::new(1.0, crate::face::lcd_ink(st, crate::face::LcdTier::Label)),
        );
    }
    ui.add_space(6.0);
    let route = if r.has_reference {
        "reference-divided (exact)"
    } else {
        "inverse-filter route"
    };
    // Both lengths, and they are different questions: how long the file is,
    // and how much of it is decay. A capture of a box with pre-delay carries
    // that delay at the front, and calling the whole length "the tail" would
    // overstate it.
    ui.label(
        RichText::new(format!(
            "{:.2} s long   {:.2} s after the peak   {route}",
            r.seconds, r.usable_s
        ))
        .font(bold(10.5))
        .color(t.text),
    );
    // **How the capture ran, next to how long it came out.** The truncation
    // keeps the tail down to about `tail_range_db` under the IR's own peak
    // and no further, because that is where the capture stopped knowing
    // anything (`sweep::recovered_floor_db`). So when an IR comes back
    // shorter than the box sounds, these two numbers are the answer — the
    // send was low, or the loop is noisy — and printing the length alone is
    // what left "why is my plate 0.6 seconds" unanswerable.
    ui.label(
        RichText::new(format!(
            "captured at {:.1} dBFS peak over a {:.0} dBFS floor \u{2014} \
             {:.0} dB of usable range",
            r.capture_peak_db, r.noise_floor_db, r.tail_range_db
        ))
        .font(bold(10.0))
        .color(t.faint),
    );
    if r.tail_range_db < 55.0 {
        ui.label(
            RichText::new(
                "A quiet capture is a short tail. Go back to LEVELS, raise the \
                 SEND (or the unit's output), and record another.",
            )
            .font(bold(10.0))
            .color(t.text),
        );
    }
    // The sweep's free distortion probe. −200 means the window fell off the
    // buffer, which a short capture can do; say nothing rather than a lie.
    if r.thd2_db > -200.0 {
        ui.label(
            RichText::new(format!(
                "distortion: 2nd {:+.0} dB, 3rd {:+.0} dB against the linear response",
                r.thd2_db, r.thd3_db
            ))
            .font(bold(10.0))
            .color(t.faint),
        );
    }
    ui.add_space(8.0);
    // The caveats list — what an IR can NEVER carry, said before Save.
    ui.label(
        RichText::new(
            "An IR keeps only the linear, time-invariant part of the sound.\n\
             - Gated-reverb programs capture as the UNGATED reverb: the gate \
             triggers on input level and cannot round-trip.\n\
             - Chorus, flange, phaser, detune and rotary programs will not \
             round-trip - the sweep smears against the moving delay.\n\
             - Some reverbs modulate their tails (and some units default \
             that ON): switch tail chorus off before capturing.\n\
             - Reverse programs usually capture fine - A/B the result.",
        )
        .font(bold(9.5))
        .color(t.faint),
    );
    ui.add_space(8.0);
    ui.horizontal(|ui| {
        let edit = egui::TextEdit::singleline(&mut s.name)
            .hint_text("name the capture...")
            .font(bold(11.0))
            .desired_width(220.0);
        ui.add(edit);
        let can = !s.name.trim().is_empty();
        if ui
            .add_enabled(
                can,
                Button::new(RichText::new("Save IR").font(bold(11.0)).color(t.text)),
            )
            .clicked()
        {
            *action = Some(DialogAction::IrSave {
                name: s.name.trim().to_owned(),
            });
        }
    });
    if !s.live.phase.is_empty() {
        ui.add_space(4.0);
        ui.label(
            RichText::new(s.live.phase.as_str())
                .font(bold(10.0))
                .color(t.faint),
        );
    }
}

pub fn show(
    ctx: &egui::Context,
    dialog_opt: &mut Option<Dialog>,
    // The desk's face, resolved for the mode the app is in.
    face: Color32,
    placement: Placement,
) -> Option<DialogAction> {
    // Before the early return, not after: the export window reads this on
    // the frames when NO dialog is open, which is most of them. See
    // `remember_face`.
    remember_face(ctx, face);
    let dialog = dialog_opt.as_mut()?;
    let mut action = None;
    // The one place the classification becomes a window: a plugin dialog
    // gets the in-house bar in this frame's face, a question gets the OS's.
    // Decided here rather than in each arm so an arm cannot disagree.
    let placement = Placement {
        chrome: (!dialog.is_modal()).then(|| crate::shell::Chrome::of(face)),
        ..placement
    };

    let result = match dialog {
        Dialog::DevicePicker {
            kind,
            devices,
            selected,
            current,
        } => {
            let kind = *kind;
            let stock = stock_style(ctx);
            show_dialog_viewport(
                ctx,
                placement,
                kind.title(),
                Vec2::new(420.0, 320.0),
                Vec2::new(360.0, 200.0),
                |ui, result| {
                    *ui.style_mut() = stock.clone();
                    let bg = ui.style().visuals.window_fill;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.label(kind.prompt());
                            if devices.is_empty() {
                                // Not an empty list with an OK button. On macOS
                                // an empty world is what a missing entitlement
                                // looks like, and on every platform it is what
                                // an unplugged interface looks like — either
                                // way a silent empty box teaches nothing.
                                ui.add_space(8.0);
                                ui.label(kind.nothing_found());
                            }
                            ui.add_space(4.0);
                            let bottom_h = 34.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // The None row first, and always present.
                                    if ui
                                        .selectable_label(selected.is_none(), kind.none_row())
                                        .clicked()
                                    {
                                        *selected = None;
                                    }
                                    for (i, d) in devices.iter().enumerate() {
                                        // The default is MARKED, not
                                        // preselected: "System Default" has to
                                        // stay a visible choice, or somebody
                                        // with two interfaces cannot tell which
                                        // one they are about to get.
                                        let label = if d.default {
                                            format!("{}   (system default)", d.name)
                                        } else {
                                            d.name.clone()
                                        };
                                        let live = current.as_deref() == Some(d.uid.as_str());
                                        let label = if live {
                                            format!("{label}  \u{2022}")
                                        } else {
                                            label
                                        };
                                        if ui.selectable_label(*selected == Some(i), label).clicked()
                                        {
                                            *selected = Some(i);
                                        }
                                    }
                                });
                            ui.add_space(6.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("OK").clicked() {
                                    // An out-of-range index cannot happen from
                                    // the UI, but the list is rebuilt from live
                                    // hardware every time this opens — so it is
                                    // resolved through `get` rather than
                                    // indexed, and a device unplugged while the
                                    // dialog was open selects nothing instead
                                    // of panicking.
                                    let uid = selected
                                        .and_then(|i| devices.get(i))
                                        .map(|d| d.uid.clone());
                                    action = Some(DialogAction::ChooseDevice { kind, uid });
                                    result.close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::FileBrowser {
            title,
            what,
            at,
            entries,
            selected,
            error,
        } => {
            let what = *what;
            let stock = stock_style(ctx);
            let folder = matches!(what, BrowseFor::Folder(_));
            show_dialog_viewport(
                ctx,
                placement,
                title,
                Vec2::new(560.0, 420.0),
                Vec2::new(360.0, 240.0),
                |ui, result| {
                    *ui.style_mut() = stock.clone();
                    let bg = ui.style().visuals.window_fill;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            // Where we are. Truncated from the LEFT: the end
                            // of a path is the part that says where you are.
                            let shown = at.to_string_lossy();
                            let shown = if shown.chars().count() > 64 {
                                let tail: String =
                                    shown.chars().rev().take(61).collect::<Vec<_>>()
                                        .into_iter().rev().collect();
                                format!("...{tail}")
                            } else {
                                shown.into_owned()
                            };
                            ui.label(shown);
                            if !error.is_empty() {
                                ui.add_space(4.0);
                                ui.label(error.as_str());
                            }
                            ui.add_space(4.0);
                            let bottom_h = 34.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            let mut go: Option<std::path::PathBuf> = None;
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    if entries.is_empty() && error.is_empty() {
                                        ui.label("nothing here that can be opened");
                                    }
                                    for (i, e) in entries.iter().enumerate() {
                                        let label = if e.is_dir {
                                            format!("{}/", e.name)
                                        } else {
                                            e.name.clone()
                                        };
                                        let r =
                                            ui.selectable_label(*selected == Some(i), label);
                                        if r.clicked() {
                                            *selected = Some(i);
                                        }
                                        // **A double click on a folder goes
                                        // in.** One click selects, because a
                                        // folder is a valid answer when the
                                        // browser is picking one.
                                        if r.double_clicked() && e.is_dir {
                                            go = Some(e.path.clone());
                                        }
                                    }
                                });
                            ui.add_space(6.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let chosen = selected.and_then(|i| entries.get(i));
                                let into_dir = chosen.filter(|e| e.is_dir).map(|e| e.path.clone());
                                // Choosing a FOLDER can mean "this one", so
                                // the button says which it will do.
                                let verb = match (&into_dir, folder) {
                                    (Some(_), false) => "Open",
                                    (Some(_), true) => "Open",
                                    (None, _) => "Choose",
                                };
                                if ui.button(verb).clicked() {
                                    if let Some(dir) = into_dir {
                                        go = Some(dir);
                                    } else if let Some(e) = chosen {
                                        action = Some(DialogAction::Browsed {
                                            what,
                                            path: Some(e.path.clone()),
                                        });
                                        result.close = true;
                                    }
                                }
                                if folder && ui.button("Use this folder").clicked() {
                                    action = Some(DialogAction::Browsed {
                                        what,
                                        path: Some(at.clone()),
                                    });
                                    result.close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    action = Some(DialogAction::Browsed { what, path: None });
                                    result.close = true;
                                }
                            });
                            if let Some(dir) = go {
                                *selected = None;
                                action = Some(DialogAction::BrowseTo(dir));
                            }
                        });
                },
            )
        }

        Dialog::InsertFace { strip, slot, live } => {
            let p = crate::recorder_panel::surface_of(face);
            let (lcd, lcd_glass) = live_lcd(ctx);
            let st = crate::face::FaceStyle::new(face, lcd, lcd_glass);
            let strip = *strip;
            let slot = *slot;
            let unit = live.face.path.clone();
            let (title, about) =
                crate::faces::suite_unit(&unit).unwrap_or(("Tangent", ""));
            // A share of the screen with a floor and a ceiling, per unit —
            // see `FACE_SIZES`. It was five fixed point counts, which is a
            // percentage of nothing in particular. Taken through
            // `held_geometry` inside `show_dialog_viewport`, so the window
            // is sized once on the display it opens on and a monitor change
            // cannot resize it out from under a hand on the grip.
            let size = face_window_size(&unit, placement.monitor);
            let least = face_min_size(&unit, size);
            show_dialog_viewport(
                ctx,
                placement,
                title,
                size,
                least,
                |ui, result| {
                    apply_theme(ui.style_mut(), &theme(face));
                    ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(PLUGIN_MARGIN))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(about)
                                    .font(FontId::new(10.0, fonts::courier_bold()))
                                    .color(p.faint),
                            );
                            ui.add_space(4.0);
                            if let Some(act) = crate::faces::draw(ui, &st, live) {
                                action = Some(match act {
                                    crate::faces::FaceAction::Set { id, value } => {
                                        DialogAction::SetInsertParam {
                                            strip,
                                            slot,
                                            id,
                                            value,
                                        }
                                    }
                                    crate::faces::FaceAction::SavePreset { name } => {
                                        DialogAction::FaceSavePreset { strip, slot, name }
                                    }
                                    crate::faces::FaceAction::LoadPreset { path } => {
                                        DialogAction::FaceLoadPreset {
                                            unit: unit.clone(),
                                            path,
                                        }
                                    }
                                    crate::faces::FaceAction::OpenPedal { pedal } => {
                                        DialogAction::OpenPedalPicker { strip, slot, pedal }
                                    }
                                    crate::faces::FaceAction::OpenWizard => {
                                        DialogAction::OpenIrWizard
                                    }
                                    crate::faces::FaceAction::PickIr { path } => {
                                        DialogAction::SetInsertIr { strip, slot, path }
                                    }
                                    crate::faces::FaceAction::BrowseIr => {
                                        // **The face window goes down with
                                        // the ask.** The OS's panel is
                                        // parented to the MAIN window and
                                        // this face is a floating child, so
                                        // leaving it up opens the chooser
                                        // UNDERNEATH it — the app's own
                                        // "load sysex opens behind the
                                        // instrument window" bug, which
                                        // reads as a button that does
                                        // nothing. `app.rs` has the lever
                                        // that puts dialogs down for a
                                        // panel (`native_panel_up`) and
                                        // this lane does not own that file,
                                        // so the face closes instead: the
                                        // chooser is unmistakably in front,
                                        // and the bay reopens on a click.
                                        //
                                        // Closing it is not enough on its
                                        // own, and saying so here because
                                        // this line looks like it is: the
                                        // viewport is torn down a frame
                                        // AFTER it stops being drawn, so
                                        // the host holds the ask for that
                                        // long before it raises anything.
                                        // See `IR_BROWSE`.
                                        result.close = true;
                                        DialogAction::SetInsertIr {
                                            strip,
                                            slot,
                                            path: IR_BROWSE.to_owned(),
                                        }
                                    }
                                });
                            }
                        });
                },
            )
        }
        Dialog::KeysPatch {
            slot,
            selected,
            params,
        } => {
            // **The surface derives its own parts.** `surface_of` returns a
            // ground that can carry ink, the ink that holds on it, and the
            // raised shade a selected row sits on with the ink still holding.
            // Picking a text colour by eye, or by a brightness test on the
            // theme, is the bug this app has found in a screenshot seven
            // times: ink is a property of the SURFACE and is never inherited.
            let p = crate::recorder_panel::surface_of(face);
            let slot = *slot;
            let chosen = *selected;
            let specs = KEYS_PARAM_SPECS
                .get(chosen)
                .unwrap_or(&KEYS_PARAM_SPECS[0]);
            show_dialog_viewport(
                ctx,
                placement,
                KEYS_NAME,
                Vec2::new(392.0, 520.0),
                Vec2::new(330.0, 320.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &theme(face));
                    ui.painter().rect_filled(ui.max_rect(), 0.0, p.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(PLUGIN_MARGIN))
                        .show(ui, |ui| {
                            // **Two columns, names only, the story on
                            // hover.** Full-width rows with a line of prose
                            // each were right for two sounds and would eat
                            // the whole panel at eight: the grid is for
                            // CHOOSING, and the chosen sound's own line is
                            // printed under it so the prose is never more
                            // than one glance away.
                            let cols = 2usize;
                            let rows = KEYS_PATCH_NAMES.len().div_ceil(cols);
                            for row in 0..rows {
                                ui.horizontal(|ui| {
                                    let w = (ui.available_width() - 6.0) / cols as f32;
                                    for col in 0..cols {
                                        let i = row * cols + col;
                                        let Some((name, about)) =
                                            KEYS_PATCH_NAMES.get(i)
                                        else {
                                            continue;
                                        };
                                        let resp = ui.allocate_response(
                                            egui::vec2(w, 28.0),
                                            egui::Sense::click(),
                                        );
                                        let ground = if i == chosen {
                                            Some(p.raised)
                                        } else if resp.hovered() {
                                            Some(p.hover)
                                        } else {
                                            None
                                        };
                                        if let Some(g) = ground {
                                            ui.painter().rect_filled(resp.rect, 3.0, g);
                                        }
                                        ui.painter().text(
                                            resp.rect.left_center() + egui::vec2(9.0, 0.0),
                                            egui::Align2::LEFT_CENTER,
                                            *name,
                                            bold(12.0),
                                            p.ink,
                                        );
                                        let resp = resp.on_hover_text(*about);
                                        if resp.clicked() {
                                            // **The cell lights up NOW.** Firing
                                            // the action and leaving `selected`
                                            // alone is what made choosing a sound
                                            // look like it had done nothing until
                                            // the window was reopened: the panel
                                            // is drawn from its own state. The
                                            // app re-seeds the knobs behind this,
                                            // because the rack belongs to the
                                            // sound.
                                            *selected = i;
                                            action = Some(
                                                DialogAction::ChooseKeysPatch { slot, index: i },
                                            );
                                            // Choosing does not close the panel:
                                            // the knobs below are half of why it
                                            // is open.
                                            result.close = false;
                                        }
                                    }
                                });
                                ui.add_space(4.0);
                            }
                            // The chosen sound's line, in full.
                            if let Some((_, about)) = KEYS_PATCH_NAMES.get(chosen) {
                                ui.label(
                                    RichText::new(*about)
                                        .font(FontId::new(10.0, fonts::courier_bold()))
                                        .color(p.faint),
                                );
                            }
                            ui.add_space(6.0);

                            // ── the rack. Per SOUND: the list comes from the
                            // patch, so a piano shows an unison and a soundboard
                            // and an electric piano shows a bell and a pickup,
                            // rather than one fixed set with half of it lying.
                            let mut reset_all = false;
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new("SOUND")
                                        .font(FontId::new(10.0, fonts::courier_bold()))
                                        .color(p.faint),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        // The explicit one. A knob resets on a
                                        // double click; the whole sound resets
                                        // here, because "put it back" should
                                        // not be eight gestures.
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new("Initialise")
                                                        .font(bold(11.0))
                                                        .color(p.ink),
                                                )
                                                .fill(p.raised),
                                            )
                                            .on_hover_text(
                                                "every knob of this sound back to the bench's own",
                                            )
                                            .clicked()
                                        {
                                            reset_all = true;
                                        }
                                    },
                                );
                            });
                            ui.add_space(4.0);

                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (knob, (name, about, default)) in
                                        specs.iter().enumerate()
                                    {
                                        let Some(cur) = params.get_mut(knob) else {
                                            continue;
                                        };
                                        let mut v = *cur;
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{name:<11}"))
                                                    .font(bold(11.0))
                                                    .color(p.ink),
                                            );
                                            let resp = ui.add(
                                                egui::Slider::new(&mut v, 0.0..=1.0)
                                                    .show_value(false),
                                            );
                                            // **Double click puts it back.**
                                            // On the knob itself, which is
                                            // where the hand already is.
                                            if resp.double_clicked() {
                                                v = *default;
                                            }
                                            let resp = resp.on_hover_text(format!(
                                                "{about}\ndouble-click to reset"
                                            ));
                                            if resp.changed() || resp.double_clicked() {
                                                *cur = v;
                                                action = Some(DialogAction::SetKeysParam {
                                                    slot,
                                                    knob,
                                                    value: v,
                                                });
                                            }
                                            ui.label(
                                                RichText::new(format!(
                                                    "{:>3.0}%",
                                                    f64::from(v) * 100.0
                                                ))
                                                .font(FontId::new(10.0, fonts::courier_bold()))
                                                .color(if (v - *default).abs() < 1.0e-4 {
                                                    p.faint
                                                } else {
                                                    // Moved off the bench's
                                                    // value, and saying so is
                                                    // the whole reason a
                                                    // reset is findable.
                                                    p.ink
                                                }),
                                            );
                                        });
                                        ui.label(
                                            RichText::new(*about)
                                                .font(FontId::new(9.0, fonts::courier_bold()))
                                                .color(p.faint),
                                        );
                                        ui.add_space(4.0);
                                    }
                                });
                            if reset_all {
                                for (slot_v, spec) in params.iter_mut().zip(specs.iter()) {
                                    *slot_v = spec.2;
                                }
                                action = Some(DialogAction::ResetKeysParams { slot });
                            }
                        });
                },
            )
        }

        Dialog::PatchEditor {
            slot,
            patch,
            group,
            name,
            note,
        } => {
            let t = theme(face);
            let slot = *slot;
            show_dialog_viewport(
                ctx,
                placement,
                "Tangent DX7 - patch",
                Vec2::new(EDITOR_W, EDITOR_H),
                Vec2::new(420.0, 400.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let faint = t.text.gamma_multiply(0.72);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(PLUGIN_MARGIN))
                        .show(ui, |ui| {
                            // ── the name, and where it would be saved ───────
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Name").font(bold(11.0)).color(t.text));
                                let resp = ui.add(
                                    egui::TextEdit::singleline(name)
                                        .font(bold(12.0))
                                        .text_color(t.text)
                                        .char_limit(NAME_LEN)
                                        .desired_width(150.0),
                                );
                                if resp.changed() {
                                    action = Some(DialogAction::SetPatchName(name.clone()));
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add(Button::new(
                                            RichText::new("Save to my bank")
                                                .font(bold(11.0))
                                                .color(t.text),
                                        ))
                                        .clicked()
                                    {
                                        action = Some(DialogAction::SavePatch);
                                    }
                                });
                            });
                            if !note.is_empty() {
                                ui.label(
                                    RichText::new(note.as_str())
                                        .font(bold(PLUGIN_NOTE_PT))
                                        .color(faint),
                                );
                            }
                            ui.add_space(4.0);

                            // ── the algorithm, drawn ────────────────────────
                            //
                            // The one picture in here. Everything else is a
                            // number, which is what making a patch on the
                            // instrument was; but which operator feeds which is
                            // a fact no list of numbers shows.
                            let h = (ui.available_height() * 0.30).clamp(80.0, 150.0);
                            let (rect, _) = ui.allocate_exact_size(
                                Vec2::new(ui.available_width(), h),
                                egui::Sense::hover(),
                            );
                            draw_algorithm(ui.painter(), rect, patch, &t);
                            ui.add_space(4.0);

                            // ── which page ──────────────────────────────────
                            ui.horizontal_wrapped(|ui| {
                                for (i, g) in patch.groups.iter().enumerate() {
                                    if ui
                                        .selectable_label(
                                            *group == i,
                                            RichText::new(&g.title)
                                                .font(bold(12.0))
                                                .color(t.text),
                                        )
                                        .clicked()
                                    {
                                        *group = i;
                                    }
                                }
                            });
                            ui.add_space(2.0);

                            // ── the numbers ─────────────────────────────────
                            //
                            // **Columns, not one long list.** Twenty-one rows
                            // stacked is a window taller than the one behind
                            // it, and a patch is a thing you read across: the
                            // envelope beside the frequency beside the
                            // scaling, all of it at once, the way the chart on
                            // the instrument's own lid is laid out. Scrolling
                            // for the last operator parameter is scrolling
                            // away from the one you are comparing it to.
                            let g = (*group).min(patch.groups.len().saturating_sub(1));
                            let bottom = 30.0;
                            let list_h = (ui.available_height() - bottom).max(60.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let Some(page) = patch.groups.get_mut(g) else {
                                        return;
                                    };
                                    let n = page.params.len();
                                    let per = n.div_ceil(PARAM_COLS);
                                    // The column's own width, taken before any
                                    // of them is entered: inside a column
                                    // `available_width` is the column's, and
                                    // inside a row it is what the row has left.
                                    //
                                    // **Both halves are budgeted, and they add
                                    // up to less than a column.** A label and
                                    // a combo box that between them are wider
                                    // than the column they are in do not clip:
                                    // they widen the whole `Ui`, and the first
                                    // thing to fall off the far side is the
                                    // Done button four rows below.
                                    let col_w =
                                        (ui.available_width() / PARAM_COLS as f32 - 14.0)
                                            .max(60.0);
                                    let label_w = col_w * 0.56;
                                    let field_w = col_w * 0.40;
                                    let mut edited: Option<(usize, i32)> = None;
                                    ui.columns(PARAM_COLS, |cols| {
                                        for (c, col) in cols.iter_mut().enumerate() {
                                            for i in (c * per)..((c + 1) * per).min(n) {
                                                let param = &mut page.params[i];
                                                let before = param.value;
                                                col.horizontal(|ui| {
                                                    ui.add_sized(
                                                        [label_w, 18.0],
                                                        egui::Label::new(
                                                            RichText::new(&param.name)
                                                                .font(bold(11.0))
                                                                .color(t.text),
                                                        ),
                                                    );
                                                    if param.choices.is_empty() {
                                                        // A number, dragged or
                                                        // typed. The
                                                        // instrument's own way
                                                        // in.
                                                        ui.add(
                                                            egui::DragValue::new(
                                                                &mut param.value,
                                                            )
                                                            .range(0..=param.max)
                                                            .speed(0.25),
                                                        );
                                                        if !param.unit.is_empty() {
                                                            ui.label(
                                                                RichText::new(&param.unit)
                                                                    .font(bold(10.0))
                                                                    .color(faint),
                                                            );
                                                        }
                                                    } else {
                                                        // A choice: the panel
                                                        // names it, so a
                                                        // number would be
                                                        // worse.
                                                        let idx = param.value.clamp(
                                                            0,
                                                            param.choices.len() as i32 - 1,
                                                        )
                                                            as usize;
                                                        egui::ComboBox::from_id_salt((g, i))
                                                            .width(field_w)
                                                            .selected_text(
                                                                RichText::new(
                                                                    &param.choices[idx],
                                                                )
                                                                .font(bold(10.0))
                                                                .color(t.text),
                                                            )
                                                            .show_ui(ui, |ui| {
                                                                for (v, label) in param
                                                                    .choices
                                                                    .iter()
                                                                    .enumerate()
                                                                {
                                                                    ui.selectable_value(
                                                                        &mut param.value,
                                                                        v as i32,
                                                                        RichText::new(label)
                                                                            .font(bold(11.0))
                                                                            .color(t.text),
                                                                    );
                                                                }
                                                            });
                                                    }
                                                });
                                                if param.value != before {
                                                    edited = Some((i, param.value));
                                                }
                                            }
                                        }
                                    });
                                    // **Reported as it moves.** A patch editor
                                    // that only applied on OK would make every
                                    // change a guess: what you turn is what you
                                    // hear.
                                    if let Some((index, value)) = edited {
                                        action = Some(DialogAction::SetPatchParam {
                                            group: g,
                                            index,
                                            value,
                                        });
                                    }
                                });

                            ui.add_space((ui.available_height() - 26.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Done").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                                if ui
                                    .add(Button::new(
                                        RichText::new("Patches...").color(t.text),
                                    ))
                                    .clicked()
                                {
                                    // Back to the list, which is where a patch
                                    // to start from comes from.
                                    action = Some(DialogAction::ShowPatches { slot });
                                }
                            });
                        });
                },
            )
        }

        Dialog::PatchPicker {
            slot,
            bank,
            bad_checksum,
            factory,
            voices,
            selected,
            filter,
            focus,
            error,
        } => {
            let t = theme(face);
            let slot = *slot;
            show_dialog_viewport(
                ctx,
                placement,
                "Tangent DX7",
                Vec2::new(PATCH_DIALOG_W, PATCH_DIALOG_H),
                Vec2::new(360.0, 300.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let faint = t.text.gamma_multiply(0.72);
                    let first_frame = *focus;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(PLUGIN_MARGIN))
                        .show(ui, |ui| {
                            // ── which cartridge ─────────────────────────────
                            ui.horizontal(|ui| {
                                // **The buttons' width is reserved before the
                                // name is placed.** A bank can be called
                                // anything, and the one that ships is called
                                // something long: a label sized to its own
                                // text ran straight under "Edit...".
                                // The Factory button is only there when there
                                // is a loaded cartridge to leave, so the room
                                // it takes is conditional too — a permanent
                                // reservation would crop the bank name on the
                                // one row that never has the button.
                                let buttons = BANK_BUTTONS_W
                                    + BANK_NAME_GAP
                                    + if *factory { 0.0 } else { FACTORY_BUTTON_W };
                                let room = (ui.available_width() - buttons).max(40.0);
                                // **The width has to be ALLOCATED, not merely
                                // asked for.** `add_sized` reserves the space
                                // but a `Label` still lays out against the
                                // width the row has left, so `truncate` had
                                // nothing to truncate against and the name ran
                                // under the buttons anyway.
                                //
                                // Room grows with the dialog, which is
                                // resizable — so a wider window shows more of
                                // the name and the ellipsis appears only when
                                // there is genuinely nowhere to put it.
                                ui.allocate_ui_with_layout(
                                    Vec2::new(room, 18.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.add(
                                            egui::Label::new(
                                                RichText::new(if bank.is_empty() {
                                                    "No cartridge loaded".to_owned()
                                                } else {
                                                    format!("Cartridge: {bank}")
                                                })
                                                .font(bold(12.0))
                                                .color(t.text),
                                            )
                                            .truncate(),
                                        );
                                    },
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add(Button::new(
                                            RichText::new("Load .syx...")
                                                .font(bold(11.0))
                                                .color(t.text),
                                        ))
                                        .clicked()
                                    {
                                        action = Some(DialogAction::LoadCartridge { slot });
                                    }
                                    // **The way back, which there was not
                                    // one of.** Loading somebody else's bank
                                    // was a one-way door: the shipped
                                    // cartridge is only reached at startup
                                    // from an empty setting, so getting it
                                    // back meant quitting the app. Offered
                                    // only while there IS a cartridge to
                                    // leave, so the row stays two buttons
                                    // wide on a fresh install.
                                    if !*factory
                                        && ui
                                            .add(Button::new(
                                                RichText::new("Factory")
                                                    .font(bold(11.0))
                                                    .color(t.text),
                                            ))
                                            .clicked()
                                    {
                                        action = Some(DialogAction::UseFactoryCartridge { slot });
                                    }
                                    // **Edit what is playing**, which is the
                                    // only patch anybody wants to start from.
                                    // Beside Load, because between them they
                                    // are the two ways a patch gets here: off
                                    // a cartridge, or out of your own hands.
                                    if ui
                                        .add(Button::new(
                                            RichText::new("Edit...")
                                                .font(bold(11.0))
                                                .color(t.text),
                                        ))
                                        .clicked()
                                    {
                                        action = Some(DialogAction::EditPatch { slot });
                                    }
                                });
                            });
                            // A bad checksum is worth saying and not worth
                            // stopping for: several hundred cartridges in the
                            // wild fail their own and play perfectly.
                            if *bad_checksum && !bank.is_empty() {
                                ui.label(
                                    RichText::new(
                                        "This cartridge failed its own checksum. That is common \
                                         and it still plays.",
                                    )
                                    .font(bold(PLUGIN_NOTE_PT))
                                    .color(faint),
                                );
                            }
                            if !error.is_empty() {
                                ui.label(
                                    RichText::new(error.as_str())
                                        .font(bold(PLUGIN_NOTE_PT))
                                        .color(t.text),
                                );
                            }
                            ui.add_space(4.0);
                            if !voices.is_empty() {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Filter:").font(bold(11.0)).color(t.text),
                                    );
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(filter)
                                            .font(bold(12.0))
                                            .text_color(t.text)
                                            .desired_width(f32::INFINITY),
                                    );
                                    if *focus {
                                        *focus = false;
                                        focus_and_select_all(ui, &resp, filter);
                                    }
                                });
                            } else {
                                *focus = false;
                            }

                            // ── the patches ─────────────────────────────────
                            let bottom_h = 52.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            let visible = visible_patches(voices, filter);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // The built-in patch, always first and never
                                    // filtered: it is the sound a fresh install
                                    // makes, and getting back to it has to not
                                    // depend on still having the cartridge that
                                    // replaced it.
                                    if ui
                                        .selectable_label(
                                            selected.is_none(),
                                            RichText::new(format!("  {BUILTIN_PATCH_ROW}"))
                                                .font(bold(12.0))
                                                .color(t.text),
                                        )
                                        .clicked()
                                        && selected.is_some()
                                    {
                                        *selected = None;
                                        action = Some(DialogAction::ChoosePatch {
                                            slot,
                                            index: usize::MAX,
                                        });
                                    }
                                    for &i in &visible {
                                        let Some(name) = voices.get(i) else { continue };
                                        let resp = ui.selectable_label(
                                            *selected == Some(i),
                                            // Numbered as the instrument does,
                                            // from 1: every DX7 cartridge sheet
                                            // ever printed says A1 through B16,
                                            // and none of them start at zero.
                                            RichText::new(format!("{:>3}. {name}", i + 1))
                                                .font(bold(12.0))
                                                .color(t.text),
                                        );
                                        // One click, one sound. See the variant.
                                        if resp.clicked() {
                                            *selected = Some(i);
                                            action =
                                                Some(DialogAction::ChoosePatch { slot, index: i });
                                        }
                                        if first_frame && *selected == Some(i) {
                                            resp.scroll_to_me(Some(Align::Center));
                                        }
                                    }
                                    if voices.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new(
                                                "Load a DX7 cartridge to get thirty-two more \
                                                 patches. Any .syx bank will do.",
                                            )
                                            .font(bold(11.0))
                                            .color(t.text),
                                        );
                                    } else if visible.is_empty() {
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new(
                                                "Nothing matches that. Clear the filter to see \
                                                 them all.",
                                            )
                                            .font(bold(11.0))
                                            .color(t.text),
                                        );
                                    }
                                });

                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Play while you click: each patch sounds as you pick it.",
                                )
                                .font(bold(PLUGIN_NOTE_PT))
                                .color(faint),
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // One button, and it does not undo anything.
                                // There is no Cancel because there is nothing to
                                // cancel: the patch is already playing, and the
                                // way back is the row at the top of the list.
                                if ui
                                    .add(Button::new(RichText::new("Done").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::PluginPicker {
            plugins,
            selected,
            current,
            open_folders,
            filter,
            focus,
        } => {
            // Themed, unlike the two pickers above it. Those are stock because
            // Qt showed the MIDI dialogs that way (spec §7.1) and the recorder's
            // device picker is their twin; this one has a live text field and
            // two paragraphs of the app's own voice in it, and reads as part of
            // Tangent rather than as a system sheet.
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Load Instrument",
                Vec2::new(PLUGIN_DIALOG_W, PLUGIN_DIALOG_H),
                Vec2::new(460.0, 300.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    // The filter field paints its interior on `extreme_bg_color`
                    // — stock near-black otherwise, a hole punched through a
                    // themed dialog. Same trap as the tuning editor.
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let faint = t.text.gamma_multiply(0.72);
                    // Taken before the field consumes it: the same first frame
                    // that focuses the filter scrolls the loaded plugin into
                    // view, which is what makes marking it worth anything when
                    // it is ninety rows down.
                    let first_frame = *focus;
                    // Read here, not from `ctx`: on the desktop this dialog is
                    // its own viewport with its own input, and the parent
                    // context never sees the key at all. A singleline `TextEdit`
                    // surrenders focus on Enter but does not consume the event
                    // (`filtered_events` copies), so one read serves the field
                    // and the list both.
                    //
                    // Never on the opening frame. Inline there is one context
                    // and one event queue, so the keystroke that opened the
                    // picker is still in it — and the dialog would confirm
                    // itself before it was ever seen.
                    let enter = !first_frame && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let mut confirm_now = false;
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(PLUGIN_MARGIN))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Filter:").font(bold(11.0)).color(t.text));
                                let resp = ui.add(
                                    egui::TextEdit::singleline(filter)
                                        .font(bold(12.0))
                                        .text_color(t.text)
                                        .desired_width(f32::INFINITY),
                                );
                                if *focus {
                                    *focus = false;
                                    focus_and_select_all(ui, &resp, filter);
                                }
                            });

                            let visible = visible_plugins(plugins, filter);
                            // **No INSTALLED plugins**, not no rows. The
                            // built-in is always a row, so counting rows would
                            // hide the note telling somebody where to put a
                            // VST3 from exactly the person who has none.
                            let installed = plugins
                                .iter()
                                .filter(|p| {
                                    !is_builtin(&p.path)
                                })
                                .count();
                            if installed == 0 {
                                // Not an empty box with a greyed button: an
                                // empty list cannot be told apart from a picker
                                // that failed, and the ordinary cause is an
                                // installer that offered AU or VST2 and was
                                // taken up on it.
                                ui.add_space(6.0);
                                ui.label(
                                    RichText::new(vst3_locations())
                                        .font(bold(11.0))
                                        .color(t.text),
                                );
                                ui.add_space(4.0);
                            } else {
                                // The INSTALLED count, both sides of the
                                // slash — `plugins` carries the two
                                // built-in rows at the top, and counting
                                // rows read "6 installed" to somebody
                                // with four VST3s on disk.
                                ui.label(
                                    RichText::new(if filter.trim().is_empty() {
                                        format!("{installed} installed")
                                    } else {
                                        let found = visible
                                            .iter()
                                            .filter(|&&i| {
                                                plugins.get(i).is_some_and(|p| {
                                                    !is_builtin(&p.path)
                                                })
                                            })
                                            .count();
                                        format!("{found} of {installed}")
                                    })
                                    .font(bold(PLUGIN_NOTE_PT))
                                    .color(faint),
                                );
                                ui.add_space(2.0);
                            }
                            // The two notes, the button row, and the space
                            // between them. See `PLUGIN_FOOTER_H` for why the
                            // band is a reservation and not a measurement.
                            let bottom_h = PLUGIN_FOOTER_H + 34.0 + 8.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // The None row first, always present, never
                                    // filtered, and there even when nothing is
                                    // installed: it is not an entry, and
                                    // unloading has to stay reachable from a
                                    // picker whose plugin folder has moved since
                                    // the plugin was loaded.
                                    if ui
                                        .selectable_label(
                                            selected.is_none(),
                                            RichText::new(format!("  {PLUGIN_NONE_ROW}"))
                                                .font(bold(12.0))
                                                .color(t.text),
                                        )
                                        .clicked()
                                    {
                                        *selected = None;
                                    }
                                    let filtering = !filter.trim().is_empty();
                                    let mut seen_folder: Option<String> = None;
                                    let mut folder_open = true;
                                    for &i in &visible {
                                        let Some(entry) = plugins.get(i) else {
                                            continue;
                                        };
                                        // **The vendor is a DROPDOWN.** Every
                                        // folder closed by default — the
                                        // owner's ruling — the heading a
                                        // click toggles, and an active filter
                                        // overrides the collapse: a search
                                        // that found things inside closed
                                        // folders must not read as a search
                                        // that found nothing. The house's
                                        // folder is a folder like the rest —
                                        // first by sort, and dressed in the
                                        // cluster's own colour so the eye
                                        // finds it before it reads anything.
                                        {
                                            let folder = plugin_folder(entry).to_owned();
                                            if seen_folder.as_deref() != Some(folder.as_str()) {
                                                folder_open = filtering
                                                    || open_folders.contains(&folder);
                                                ui.add_space(5.0);
                                                let ink = if folder == LABS_VENDOR {
                                                    labs_ink(ctx, t.bg, faint)
                                                } else {
                                                    faint
                                                };
                                                let head = ui.selectable_label(
                                                    false,
                                                    RichText::new(format!(
                                                        "{} {folder}",
                                                        if folder_open {
                                                            '\u{25BE}'
                                                        } else {
                                                            '\u{25B8}'
                                                        }
                                                    ))
                                                    .font(bold(11.0))
                                                    .color(ink),
                                                );
                                                if head.clicked() {
                                                    if let Some(at) = open_folders
                                                        .iter()
                                                        .position(|f| *f == folder)
                                                    {
                                                        open_folders.remove(at);
                                                    } else {
                                                        open_folders.push(folder.clone());
                                                    }
                                                }
                                                seen_folder = Some(folder);
                                            }
                                            if !folder_open {
                                                continue;
                                            }
                                        }
                                        let resp = ui.selectable_label(
                                            *selected == Some(i),
                                            RichText::new(plugin_row(entry, current.as_deref()))
                                                .font(bold(12.0))
                                                .color(t.text),
                                        );
                                        // `i`, the index into `plugins` — never
                                        // the position in `visible`, which
                                        // changes under the user with every
                                        // keystroke.
                                        if resp.clicked() {
                                            *selected = Some(i);
                                        }
                                        if resp.double_clicked() {
                                            *selected = Some(i);
                                            confirm_now = true;
                                        }
                                        if first_frame && *selected == Some(i) {
                                            // The mark is worth nothing ninety
                                            // rows down a list nobody has
                                            // scrolled.
                                            resp.scroll_to_me(Some(Align::Center));
                                        }
                                    }
                                    if visible.is_empty() && installed > 0 {
                                        ui.add_space(6.0);
                                        ui.label(
                                            RichText::new(
                                                "Nothing matches that. Clear the filter to see \
                                                 them all.",
                                            )
                                            .font(bold(11.0))
                                            .color(t.text),
                                        );
                                    }
                                });

                            // ── what this costs you ─────────────────────────
                            //
                            // Full strength for the warning and faint for the
                            // explanation, because one of them is a thing the
                            // user has to decide about and the other is a thing
                            // they were about to wonder.
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(PLUGIN_TRUST_NOTE)
                                    .font(bold(PLUGIN_NOTE_PT))
                                    .color(t.text),
                            );
                            ui.label(
                                RichText::new(PLUGIN_SCAN_NOTE)
                                    .font(bold(PLUGIN_NOTE_PT))
                                    .color(faint),
                            );

                            // Computed AFTER the list, so it answers this
                            // frame's click rather than the one before it.
                            let choice =
                                plugin_choice(
                                    plugins,
                                    *selected,
                                    current.as_deref(),
                                    filter,
                                    open_folders,
                                );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let load = ui.add_enabled(
                                    choice.is_some(),
                                    Button::new(RichText::new("Load").color(t.text)),
                                );
                                let cancel =
                                    ui.add(Button::new(RichText::new("Cancel").color(t.text)));
                                // Cancel is asked FIRST, and that is not tidiness:
                                // egui activates a focused button with Enter, so
                                // a user who tabbed to Cancel and pressed it
                                // would otherwise trip the global Enter below and
                                // load a plugin by pressing the button that says
                                // don't.
                                if cancel.clicked() {
                                    result.close = true;
                                } else if load.clicked() || confirm_now || enter {
                                    // Enter, a double-click and the button all
                                    // take the value the gate offered, which is
                                    // the point of there being one gate: they
                                    // cannot come to mean different things.
                                    // Escape is the funnel's job.
                                    if let Some(c) = &choice {
                                        action = Some(c.action());
                                        result.close = true;
                                    }
                                }
                            });
                        });
                },
            )
        }

        Dialog::MidiPicker {
            ports,
            selected,
            current,
        } => {
            let stock = stock_style(ctx);
            show_dialog_viewport(
                ctx,
                placement,
                "Select MIDI Input",
                Vec2::new(400.0, 300.0),
                Vec2::new(400.0, 200.0),
                |ui, result| {
                    *ui.style_mut() = stock.clone();
                    let bg = ui.style().visuals.window_fill;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(10))
                        .show(ui, |ui| {
                            ui.label("Select MIDI input port:");
                            if let Some(current) = current.as_deref() {
                                ui.label(format!("Current: {current}"));
                            }
                            ui.add_space(4.0);
                            let bottom_h = 34.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (i, name) in ports.iter().enumerate() {
                                        if ui.selectable_label(*selected == Some(i), name).clicked()
                                        {
                                            *selected = Some(i);
                                        }
                                    }
                                });
                            ui.add_space(6.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("OK").clicked() {
                                    if let Some(i) = *selected {
                                        if let Some(name) = ports.get(i) {
                                            action = Some(DialogAction::ConnectPort(name.clone()));
                                        }
                                    }
                                    result.close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::SaveChanges { what, name } => {
            let stock = stock_style(ctx);
            let what = what.clone();
            let name = name.clone();
            show_dialog_viewport(
                ctx,
                placement,
                "Unsaved changes",
                Vec2::new(470.0, 190.0),
                Vec2::new(360.0, 150.0),
                |ui, result| {
                    *ui.style_mut() = stock.clone();
                    let bg = ui.style().visuals.window_fill;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(format!("Save changes to \u{201c}{name}\u{201d} before {what}?"));
                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new(
                                    "Changes since the last save will be lost otherwise.",
                                )
                                .weak(),
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            // Right to left: the SAFE answer is rightmost,
                            // where the eye lands and the hand goes.
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("Save").clicked() {
                                    action = Some(DialogAction::SaveChangesSave);
                                    result.close = true;
                                }
                                if ui.button("Save As...").clicked() {
                                    action = Some(DialogAction::SaveChangesSaveAs);
                                    result.close = true;
                                }
                                if ui.button("Discard").clicked() {
                                    action = Some(DialogAction::SaveChangesDiscard);
                                    result.close = true;
                                }
                                if ui.button("Cancel").clicked() {
                                    action = Some(DialogAction::SaveChangesCancel);
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::Cheated | Dialog::CheatFun => {
            let words = if matches!(dialog, Dialog::CheatFun) {
                "So now we're just doing it for fun then? Fine."
            } else {
                "That's not fair, you cheated. >:c"
            };
            let stock = stock_style(ctx);
            show_dialog_viewport(
                ctx,
                placement,
                "Tangent",
                Vec2::new(440.0, 170.0),
                Vec2::new(340.0, 140.0),
                |ui, result| {
                    *ui.style_mut() = stock.clone();
                    let bg = ui.style().visuals.window_fill;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(words);
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.button("...").clicked() {
                                    action = Some(DialogAction::AcceptCheat);
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::AudioSetup {
            status,
            buffer,
            rate,
            systems,
            system,
            rates,
            input_channels,
            exposed,
            alias,
            output_path,
            take_summary,
        } => {
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Setup",
                Vec2::new(AUDIO_W, AUDIO_H),
                Vec2::new(AUDIO_W - 60.0, AUDIO_H - 40.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    // Derived from the theme's own ink rather than added to
                    // `DialogTheme`: two shades used by one dialog are not a
                    // theme, and a field nothing else reads is a field the next
                    // theme has to remember to set.
                    let dim = t.text.gamma_multiply(0.62);
                    let warn = Color32::from_rgb(0xE0, 0x9A, 0x3C);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            // The buttons stay put and the body scrolls, which
                            // is the Export dialog's arrangement and for the
                            // same reason: the number of rate rows depends on
                            // the interface, so the panel's content has no
                            // fixed height and its Close button must anyway.
                            let button_row = 34.0;
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height((ui.available_height() - button_row).max(0.0))
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            for (title, side) in
                                [("INPUT", &status.input), ("OUTPUT", &status.output)]
                            {
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new(title).font(bold(11.0)).color(t.text));
                                    // **The device half of "the audio system
                                    // and device".** A button rather than a
                                    // second list: the microphone picker is
                                    // already the device chooser, it is already
                                    // where the revealed channels appear, and a
                                    // copy of that list here would be a copy to
                                    // keep in step with it.
                                    if title == "INPUT" {
                                        ui.with_layout(
                                            Layout::right_to_left(Align::Center),
                                            |ui| {
                                                if ui
                                                    .add(Button::new(
                                                        RichText::new("Change...")
                                                            .font(plain(10.0))
                                                            .color(t.text),
                                                    ))
                                                    .clicked()
                                                {
                                                    action =
                                                        Some(DialogAction::ChooseAudioDevice);
                                                    result.close = true;
                                                }
                                            },
                                        );
                                    }
                                });
                                match side {
                                    Some((name, s)) => {
                                        ui.label(
                                            RichText::new(name).font(plain(11.0)).color(t.text),
                                        );
                                        // **What is RUNNING, and — when they
                                        // differ — what was asked for and WHY
                                        // it was not granted.**
                                        //
                                        // The panel used to print the request,
                                        // so it read 128 while the room ran 512
                                        // and nothing anywhere said so. Naming
                                        // both numbers fixed that and left a
                                        // worse problem: it also named a CAUSE,
                                        // "another app owns this device", which
                                        // is the CoreAudio answer. There,
                                        // another host holding the interface
                                        // really does dictate the size.
                                        //
                                        // On a PipeWire machine it is the GRAPH,
                                        // and no second app need exist. One
                                        // user was told that about a machine
                                        // running nothing else, while the
                                        // number that actually decided it sat
                                        // in a config file the app had never
                                        // mentioned — so the message was not
                                        // merely wrong, it pointed away from
                                        // the only thing that would have helped.
                                        //
                                        // So: say the floor when the floor is
                                        // knowable, and say nothing about cause
                                        // when it is not. An unexplained number
                                        // is a smaller failure than a confident
                                        // wrong explanation.
                                        let real = s.observed_frames.or(s.buffer_frames);
                                        let asked = s.buffer_frames.unwrap_or(0);
                                        let buf = match (real, s.buffer_ms()) {
                                            (Some(f), Some(ms)) if s.honoured() => {
                                                format!("{f} frames  ({ms:.1} ms)")
                                            }
                                            (Some(f), Some(ms)) => match status.graph_floor {
                                                Some(floor) if asked < floor => format!(
                                                    "{f} frames  ({ms:.1} ms)  - asked for \
                                                     {asked}; this machine's audio graph will \
                                                     not serve below {floor}"
                                                ),
                                                _ => format!(
                                                    "{f} frames  ({ms:.1} ms)  - asked for \
                                                     {asked}, and the device gave this instead"
                                                ),
                                            },
                                            // The device chose and did not say
                                            // what it chose — which is what
                                            // cpal's `BufferSize::Default`
                                            // means, and pretending otherwise
                                            // would be inventing a number.
                                            _ => "device default".to_owned(),
                                        };
                                        // The ring behind the period, where
                                        // one exists (WASAPI, ALSA): without
                                        // it the millisecond figure above
                                        // understates output buffering by
                                        // the ring factor. Same wording as
                                        // `OutputInfo::latency_line`, so the
                                        // band and this panel agree.
                                        let buf = match s.buffer_ring {
                                            Some(ring) => {
                                                format!("{buf}  ({ring} frame ring)")
                                            }
                                            None => buf,
                                        };
                                        ui.label(
                                            RichText::new(format!(
                                                "{} Hz   {} ch   {buf}",
                                                s.sample_rate, s.channels
                                            ))
                                            .font(plain(11.0))
                                            .color(t.text),
                                        );
                                    }
                                    None => {
                                        ui.label(
                                            RichText::new("not open")
                                                .font(plain(11.0))
                                                .color(dim),
                                        );
                                    }
                                }
                                ui.add_space(6.0);
                            }

                            if status.rates_disagree() {
                                // Not cosmetic: the writer drains the
                                // instrument's ring at the input's rate while
                                // the engine fills it at the output's.
                                // Two lines. One was 549 points wide against a
                                // 430-point panel — see
                                // `the_audio_status_card_fits_its_own_text`,
                                // which measures it rather than trusting it.
                                for line in [
                                    "The two sides run at different rates.",
                                    "The instrument's audio will drift.",
                                ] {
                                    ui.label(
                                        RichText::new(line).font(bold(11.0)).color(warn),
                                    );
                                }
                                ui.add_space(6.0);
                            }

                            ui.label(
                                RichText::new(match status.round_trip_ms() {
                                    Some(ms) => format!("Round trip, at least  {ms:.1} ms"),
                                    None => "Round trip  -".to_owned(),
                                })
                                .font(bold(11.0))
                                .color(t.text),
                            );
                            // Said out loud, because a latency figure people
                            // trust and should not is worse than none.
                            ui.label(
                                RichText::new(
                                    "buffers only; converters and the driver add more",
                                )
                                .font(plain(10.0))
                                .color(dim),
                            );

                            // ── dropouts ─────────────────────────────────────
                            //
                            // **Drawn only when somebody could actually count.**
                            // `None` is not zero: on a PipeWire desktop the
                            // client-side counter is structurally blind
                            // (pipewire-alsa absorbs the underrun before cpal
                            // hears it), so this number comes from the graph's
                            // own tally where there is one. A session that was
                            // audibly "total noise" once showed a clean panel
                            // because the only counter anybody read could not
                            // fire — rendering silence for "unknown" is how
                            // that stays fixed, and rendering 0 for it is how
                            // it happened.
                            if let Some(n) = status.dropouts {
                                let (text, color) = if n == 0 {
                                    ("no dropouts since this stream opened".to_owned(), dim)
                                } else {
                                    (
                                        format!(
                                            "{n} dropout{} since this stream opened - each \
                                             one is a gap in what was heard and recorded",
                                            if n == 1 { "" } else { "s" }
                                        ),
                                        warn,
                                    )
                                };
                                ui.label(RichText::new(text).font(plain(10.0)).color(color));
                            }

                            // ── who else is in the path ──────────────────────
                            //
                            // **Directly under the round trip, because it is
                            // the explanation for it.** The estimate above is
                            // buffers, and the sentence "converters and the
                            // driver add more" is true and useless on its own:
                            // it names an unknown. These lines name the
                            // knowable part of that unknown, and the reason
                            // they exist at all is that the app once spent a
                            // week measuring itself while another application
                            // sat downstream at unity gain, adding delay and
                            // nothing else to hear.
                            if output_path.worth_saying() {
                                let rate = status
                                    .output
                                    .as_ref()
                                    .map_or(0, |(_, s)| s.sample_rate);
                                if let Some(ms) = output_path.extra_ms(rate) {
                                    // cpal reads neither of these on any
                                    // platform, so this is time the figure
                                    // above has never contained.
                                    ui.label(
                                        RichText::new(format!(
                                            "the device reports {ms:.1} ms more, \
                                             not counted above"
                                        ))
                                        .font(plain(10.0))
                                        .color(dim),
                                    );
                                }
                                if output_path.virtualised {
                                    ui.label(
                                        RichText::new("Output is a VIRTUAL device")
                                            .font(bold(11.0))
                                            .color(warn),
                                    );
                                    // **The maker on its own line, and
                                    // clipped.** "a virtual device" is a fact;
                                    // "Rogue Amoeba" is somewhere to go, so it
                                    // is worth the line. But the string comes
                                    // from the DEVICE and can be any length,
                                    // and a panel 430 points wide cannot
                                    // promise to fit one it has never seen.
                                    // `the_audio_status_card_fits_its_own_text`
                                    // measures the budget; this keeps inside
                                    // it whatever the driver is called.
                                    if !output_path.maker.is_empty() {
                                        ui.label(
                                            RichText::new(clipped(&output_path.maker, MAKER_MAX))
                                                .font(plain(10.0))
                                                .color(warn),
                                        );
                                    }
                                    ui.label(
                                        RichText::new(
                                            "another app is between the mix and the hardware",
                                        )
                                        .font(plain(10.0))
                                        .color(dim),
                                    );
                                } else if output_path.aggregate {
                                    // Not a warning. An aggregate is how
                                    // anyone with two interfaces runs them on
                                    // one clock, and it is named only because
                                    // its drift correction is a cost a single
                                    // device does not have.
                                    ui.label(
                                        RichText::new("Output is an aggregate device")
                                            .font(plain(10.0))
                                            .color(dim),
                                    );
                                }
                            }
                            ui.add_space(10.0);

                            // **What these two rows mean under JACK.** The
                            // server runs one period and one rate for the whole
                            // graph and tells its clients what they are, so both
                            // controls below are still worth SHOWING — they are
                            // what gets used the moment the system goes back to
                            // ALSA — and are not worth pretending about while
                            // JACK is selected. Said rather than greyed out
                            // because this is the saved setting and the engine
                            // falls back to ALSA when no server answered; the
                            // Audio Status line reports the period actually
                            // running either way, and it is the one to believe.
                            let jack_note = (system.as_deref() == Some("JACK")).then_some(
                                "JACK sets this for the whole graph; change it in the server",
                            );

                            // ── the audio system ─────────────────────────────
                            // Drawn even when there is one, and saying so.
                            // cpal compiles in a single host per platform
                            // unless a cargo feature asks for more, so on macOS
                            // and Windows this row is a READOUT — and a readout
                            // that names the driver stack is worth having,
                            // because "ALSA" on a PipeWire machine is the
                            // sentence that explains why the buffer behaves the
                            // way it does. On Linux and FreeBSD, where JACK is
                            // compiled in beside it, the same row is the choice
                            // between that sentence and the graph itself.
                            ui.label(RichText::new("SYSTEM").font(bold(11.0)).color(t.text));
                            if systems.len() <= 1 {
                                ui.label(
                                    RichText::new(match systems.first() {
                                        Some(only) => format!("{only}  (the only one this build has)"),
                                        None => "none reported".to_owned(),
                                    })
                                    .font(plain(11.0))
                                    .color(dim),
                                );
                            } else {
                                ui.horizontal_wrapped(|ui| {
                                    for name in systems.iter() {
                                        let on = system.as_deref() == Some(name.as_str());
                                        if ui.selectable_label(on, name).clicked() && !on {
                                            *system = Some(name.clone());
                                            action = Some(DialogAction::SetAudioSystem(Some(
                                                name.clone(),
                                            )));
                                        }
                                    }
                                });
                                ui.label(
                                    RichText::new("the device list changes with it")
                                        .font(plain(10.0))
                                        .color(dim),
                                );
                            }
                            ui.add_space(10.0);

                            // ── the rate ─────────────────────────────────────
                            ui.label(
                                RichText::new("SAMPLE RATE").font(bold(11.0)).color(t.text),
                            );
                            ui.horizontal_wrapped(|ui| {
                                let mut pick =
                                    |ui: &mut egui::Ui, label: String, val: Option<u32>| {
                                        if ui.selectable_label(*rate == val, label).clicked() {
                                            *rate = val;
                                            action = Some(DialogAction::SetSampleRate(val));
                                        }
                                    };
                                pick(ui, "Device".to_owned(), None);
                                // What the INPUT device actually offers, not a
                                // constant: a rate offered and then refused is
                                // a "could not open" every time somebody tries
                                // the biggest number.
                                for r in rates.iter() {
                                    pick(ui, r.to_string(), Some(*r));
                                }
                            });
                            ui.label(
                                RichText::new(jack_note.unwrap_or(
                                    "what the input offers; the output follows where it can",
                                ))
                                .font(plain(10.0))
                                .color(dim),
                            );
                            ui.add_space(10.0);

                            ui.label(
                                RichText::new("BUFFER SIZE").font(bold(11.0)).color(t.text),
                            );
                            ui.horizontal_wrapped(|ui| {
                                let mut pick = |ui: &mut egui::Ui, label: String, val: Option<u32>| {
                                    if ui.selectable_label(*buffer == val, label).clicked() {
                                        *buffer = val;
                                        action = Some(DialogAction::SetBufferFrames(val));
                                    }
                                };
                                pick(ui, "Device".to_owned(), None);
                                for f in crate::recorder::BUFFER_CHOICES {
                                    pick(ui, f.to_string(), Some(f));
                                }
                            });
                            ui.label(
                                RichText::new(jack_note.unwrap_or(
                                    "both streams reopen; not while a take is rolling",
                                ))
                                .font(plain(10.0))
                                .color(dim),
                            );
                            ui.add_space(10.0);

                            // ── the channels ─────────────────────────────────
                            ui.label(
                                RichText::new("INPUT CHANNELS").font(bold(11.0)).color(t.text),
                            );
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(if exposed.is_empty() {
                                        match *input_channels {
                                            0 => "the whole device".to_owned(),
                                            n => format!("all {n}, as one device"),
                                        }
                                    } else {
                                        // Named, not counted. "3 shown" makes
                                        // somebody open the chooser to find out
                                        // which three.
                                        exposed
                                            .iter()
                                            .map(|&(a, b)| match b {
                                                None => format!("{}", a + 1),
                                                Some(b) => format!("{}/{}", a + 1, b + 1),
                                            })
                                            .collect::<Vec<_>>()
                                            .join(", ")
                                    })
                                    .font(plain(11.0))
                                    .color(t.text),
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    // Offered wherever there is more than one
                                    // input — the owner's ruling: even a plain
                                    // stereo interface must expose its two
                                    // channels one at a time, because a mono
                                    // microphone on input 2 of a two-in box is
                                    // as real as one on input 6 of a rack.
                                    let can = *input_channels >= 2;
                                    if ui
                                        .add_enabled(
                                            can,
                                            Button::new(
                                                RichText::new("Choose...")
                                                    .font(plain(10.0))
                                                    .color(t.text),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action = Some(DialogAction::ChooseInputChannels);
                                        result.close = true;
                                    }
                                });
                            });
                            ui.label(
                                RichText::new(if *input_channels >= 3 {
                                    "each one ticked is its own row in the mic picker"
                                } else {
                                    "for an interface with more than two inputs"
                                })
                                .font(plain(10.0))
                                .color(dim),
                            );

                            ui.add_space(10.0);

                            // ── the short name ───────────────────────────────
                            //
                            // "Scarlett 18i20 USB - input 3" is what the
                            // system calls a channel and a mixer strip is a
                            // hundred points wide. The half worth shortening
                            // is the one that repeats on every channel of the
                            // box, so it is shortened once, here, where the
                            // box is chosen.
                            ui.label(
                                RichText::new("SHORT NAME").font(bold(11.0)).color(t.text),
                            );
                            let before = alias.clone();
                            ui.add(
                                egui::TextEdit::singleline(alias)
                                    .font(plain(11.0))
                                    .text_color(t.text)
                                    .hint_text(
                                        RichText::new(
                                            status
                                                .input
                                                .as_ref()
                                                .map_or("the interface's own name", |d| {
                                                    d.0.as_str()
                                                }),
                                        )
                                        .font(plain(11.0))
                                        .color(dim),
                                    )
                                    .desired_width(f32::INFINITY),
                            );
                            if *alias != before {
                                action = Some(DialogAction::SetInputAlias(alias.clone()));
                            }
                            ui.label(
                                RichText::new(
                                    "what the mixer calls this interface - \"x\" makes \
                                     \"x - 3\"",
                                )
                                .font(plain(10.0))
                                .color(dim),
                            );

                            ui.add_space(10.0);

                            // ── the take report ──────────────────────────────
                            //
                            // **The switch that came back.** It used to be a
                            // "Don't show this again" tick on the take summary
                            // itself, which was a modal box; the report is a
                            // notification in the corner now and the box is
                            // gone, so for a while the setting was read on
                            // every take and writable by nobody — anyone who
                            // had turned the report off was stuck with it off.
                            // It is here because this is the panel about the
                            // recording path, and a take is what that path
                            // makes.
                            //
                            // Phrased the POSITIVE way round, unlike the tick
                            // it replaces: "don't show this again" made sense
                            // on a box you were reading, and makes none in a
                            // list of settings, where every other row says
                            // what it turns on.
                            ui.label(RichText::new("REPORTS").font(bold(11.0)).color(t.text));
                            if ui
                                .checkbox(
                                    take_summary,
                                    RichText::new("Say what each take produced")
                                        .font(plain(11.0))
                                        .color(t.text),
                                )
                                .changed()
                            {
                                action = Some(DialogAction::SetShowTakeSummary(*take_summary));
                            }
                            ui.label(
                                RichText::new("a take with a PROBLEM says so either way")
                                    .font(plain(10.0))
                                    .color(dim),
                            );
                                });

                            ui.add_space(6.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.add(Button::new(RichText::new("Close").color(t.text))).clicked()
                                {
                                    result.close = true;
                                }
                                // The IR wizard's door, beside Close: it is
                                // about the same subject as this panel — the
                                // interface's ins and outs — and it needs
                                // both halves open, which Setup is where you
                                // check.
                                if ui
                                    .add(Button::new(
                                        RichText::new("Capture hardware IR...")
                                            .color(t.text),
                                    ))
                                    .clicked()
                                {
                                    action = Some(DialogAction::OpenIrWizard);
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::InputChannels {
            device,
            channels,
            exposed,
        } => {
            let t = theme(face);
            let n = *channels;
            // Two columns, one row per input. Tall enough for the longer of
            // them — mono has `n` boxes, stereo `n - 1` — and no taller,
            // with a scroll for an interface with more inputs than a dialog
            // can hold.
            let listed = f32::from(n.max(1));
            let h = (190.0 + listed * 24.0).min(CHANNELS_MAX_H);
            show_dialog_viewport(
                ctx,
                placement,
                "Input Channels",
                Vec2::new(CHANNELS_W, h),
                Vec2::new(CHANNELS_W - 60.0, h - 40.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    let dim = t.text.gamma_multiply(0.62);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            ui.label(RichText::new(device.as_str()).font(bold(11.0)).color(t.text));
                            ui.label(
                                RichText::new(format!(
                                    "{n} inputs, numbered as they are on the box"
                                ))
                                .font(plain(10.0))
                                .color(dim),
                            );
                            ui.add_space(8.0);

                            // Ticked boxes are collected and reported once, at
                            // the end of the frame. Reporting from inside the
                            // loop would mean the row after the one that was
                            // just clicked is laid out against a set that no
                            // longer matches what is drawn.
                            let mut want = exposed.clone();
                            let mut toggle = |want: &mut Vec<(u16, Option<u16>)>,
                                              pick: (u16, Option<u16>),
                                              on: bool| {
                                want.retain(|p| *p != pick);
                                if on {
                                    want.push(pick);
                                }
                            };

                            ui.horizontal_top(|ui| {
                                let col = (CHANNELS_W - 60.0) / 2.0;
                                ui.vertical(|ui| {
                                    ui.set_width(col);
                                    ui.label(
                                        RichText::new("MONO").font(bold(11.0)).color(t.text),
                                    );
                                    egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                        .id_salt("mono-inputs")
                                        .max_height(ui.available_height() - 44.0)
                                        .show(ui, |ui| {
                                            for c in 0..n {
                                                let pick = (c, None);
                                                let mut on = exposed.contains(&pick);
                                                if ui
                                                    .checkbox(
                                                        &mut on,
                                                        RichText::new(format!("{}", c + 1))
                                                            .font(plain(11.0))
                                                            .color(t.text),
                                                    )
                                                    .changed()
                                                {
                                                    toggle(&mut want, pick, on);
                                                }
                                            }
                                        });
                                });
                                ui.vertical(|ui| {
                                    ui.set_width(col);
                                    ui.label(
                                        RichText::new("STEREO").font(bold(11.0)).color(t.text),
                                    );
                                    egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                        .id_salt("stereo-pairs")
                                        .max_height(ui.available_height() - 44.0)
                                        .show(ui, |ui| {
                                            // **Every adjacent pair, including
                                            // the odd starts.** 2/3 is a stereo
                                            // source patched across the middle
                                            // of the box because input 1 has
                                            // something else in it, and it is
                                            // as ordinary as 1/2.
                                            for c in 0..n.saturating_sub(1) {
                                                let pick = (c, Some(c + 1));
                                                let mut on = exposed.contains(&pick);
                                                if ui
                                                    .checkbox(
                                                        &mut on,
                                                        RichText::new(format!(
                                                            "{}/{}",
                                                            c + 1,
                                                            c + 2
                                                        ))
                                                        .font(plain(11.0))
                                                        .color(t.text),
                                                    )
                                                    .changed()
                                                {
                                                    toggle(&mut want, pick, on);
                                                }
                                            }
                                        });
                                });
                            });

                            if want != *exposed {
                                want.sort_by_key(|&(a, b)| (a, b.unwrap_or(a)));
                                *exposed = want.clone();
                                action = Some(DialogAction::SetInputChannels(want));
                            }

                            ui.add_space(6.0);
                            ui.label(
                                RichText::new(
                                    "each one ticked is its own row in the mic picker;\n\
                                     the interface itself is always offered",
                                )
                                .font(plain(10.0))
                                .color(dim),
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Close").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::Project { name, subdir } => {
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Project",
                // Taller for the subdirectory tick and its line of fine
                // print: the row went in without the window growing, and the
                // Done button fell off the bottom until somebody resized.
                Vec2::new(340.0, 410.0),
                Vec2::new(340.0, 370.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    let dim = t.text.gamma_multiply(0.62);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            // The name is a FIELD: it stages what the
                            // session should be called, and Save saves that
                            // name — nobody has to meet a panel to name a
                            // song.
                            // The field's WELL is pinned dark so the white
                            // ink reads in every theme — the inherited well
                            // was light under system-light and the name
                            // typed white-on-white.
                            ui.style_mut().visuals.extreme_bg_color =
                                egui::Color32::from_rgb(0x1A, 0x16, 0x12);
                            ui.add(
                                egui::TextEdit::singleline(name)
                                    .font(bold(13.0))
                                    .text_color(egui::Color32::WHITE)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.label(
                                RichText::new(
                                    "the open session - audio takes and the kit write themselves; Save writes the rest, under this name",
                                )
                                .font(plain(9.0))
                                .color(dim),
                            );
                            ui.add_space(8.0);
                            let mut row = |ui: &mut egui::Ui, word: &str, hint: &str| -> bool {
                                let clicked = ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), 24.0),
                                        Button::new(
                                            RichText::new(word).font(plain(11.0)).color(t.text),
                                        ),
                                    )
                                    .clicked();
                                if !hint.is_empty() {
                                    ui.label(RichText::new(hint).font(plain(8.5)).color(dim));
                                }
                                ui.add_space(4.0);
                                clicked
                            };
                            if row(ui, "New", "a fresh session from the default template") {
                                action = Some(DialogAction::ProjectNew);
                            }
                            if row(ui, "Open...", "") {
                                action = Some(DialogAction::ProjectOpen);
                            }
                            if row(ui, "Save", "desk, document and plugin settings, now") {
                                action = Some(DialogAction::ProjectSave {
                                    name: name.clone(),
                                });
                            }
                            if row(ui, "Save As...", "copy the whole session and open the copy") {
                                action = Some(DialogAction::ProjectSaveAs);
                            }
                            // **A checkbox rather than a row**, because it
                            // does not DO anything on click - it changes what
                            // the next Save As does, which is what a tick is
                            // for. Sits under Save As, the thing it changes.
                            {
                                let mut on = *subdir;
                                if ui
                                    .checkbox(
                                        &mut on,
                                        RichText::new("Create Project Subdirectory")
                                            .font(plain(10.0))
                                            .color(t.text),
                                    )
                                    .changed()
                                {
                                    *subdir = on;
                                    action = Some(DialogAction::SetProjectSubdir(on));
                                }
                                ui.label(
                                    RichText::new(
                                        "wrap the project in its own folder - media always \
                                         travels inside it",
                                    )
                                    .font(plain(8.5))
                                    .color(dim),
                                );
                                ui.add_space(4.0);
                            }
                            if row(
                                ui,
                                "Save As Template...",
                                "under Projects/Templates - name it default to replace the default",
                            ) {
                                action = Some(DialogAction::ProjectTemplate);
                            }
                            ui.add_space(4.0);
                            if row(
                                ui,
                                "Export...",
                                "master and/or stems - full-speed render by default",
                            ) {
                                action = Some(DialogAction::ProjectExportOpen);
                            }
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Done").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::ExportProject { spec, sel } => {
            let sel = *sel;
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Export",
                // Taller for the WHEN DONE row, which was added below the
                // window's floor and could not be reached at all.
                Vec2::new(380.0, 446.0),
                Vec2::new(380.0, 396.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    let dim = t.text.gamma_multiply(0.62);
                    // One row idiom: a label, then choice chips; the chosen
                    // one is filled.
                    let choice = |ui: &mut egui::Ui,
                                  label: &str,
                                  options: &[&str],
                                  chosen: usize|
                     -> Option<usize> {
                        let mut picked = None;
                        ui.horizontal(|ui| {
                            ui.add_sized(
                                Vec2::new(96.0, 18.0),
                                egui::Label::new(
                                    RichText::new(label).font(bold(9.5)).color(dim),
                                ),
                            );
                            for (i, o) in options.iter().enumerate() {
                                let text = RichText::new(*o).font(plain(9.5)).color(
                                    if i == chosen { t.bg } else { t.text },
                                );
                                let b = Button::new(text).fill(if i == chosen {
                                    t.text
                                } else {
                                    t.bg
                                });
                                if ui.add(b).clicked() {
                                    picked = Some(i);
                                }
                            }
                        });
                        ui.add_space(2.0);
                        picked
                    };
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            let src = match (spec.master, spec.stems) {
                                (true, false) => 0,
                                (false, true) => 1,
                                _ => 2,
                            };
                            if let Some(i) =
                                choice(ui, "SOURCE", &["Master mix", "Stems", "Both"], src)
                            {
                                spec.master = i != 1;
                                spec.stems = i != 0;
                            }
                            if let Some(range) = sel {
                                let at = usize::from(spec.span.is_some());
                                if let Some(i) = choice(
                                    ui,
                                    "TIME",
                                    &["Entire project", "Time selection"],
                                    at,
                                ) {
                                    spec.span = (i == 1).then_some(range);
                                }
                            } else {
                                // No selection standing in the arrange view:
                                // the whole project is the one honest answer.
                                let _ = choice(ui, "TIME", &["Entire project"], 0);
                                spec.span = None;
                            }
                            let rates: [u32; 4] = [44_100, 48_000, 88_200, 96_000];
                            let at = rates.iter().position(|r| *r == spec.rate).unwrap_or(1);
                            if let Some(i) = choice(
                                ui,
                                "SAMPLE RATE",
                                &["44.1k", "48k", "88.2k", "96k"],
                                at,
                            ) {
                                spec.rate = rates[i];
                            }
                            if let Some(i) = choice(
                                ui,
                                "CHANNELS",
                                &["Stereo", "Mono"],
                                usize::from(spec.mono),
                            ) {
                                spec.mono = i == 1;
                            }
                            {
                                use crate::ports::ExportDepth as D;
                                let at = match spec.depth {
                                    D::Pcm16 => 0,
                                    D::Pcm24 => 1,
                                    D::Float32 => 2,
                                };
                                if let Some(i) = choice(
                                    ui,
                                    "DEPTH",
                                    &["16-bit", "24-bit", "32-float"],
                                    at,
                                ) {
                                    spec.depth = [D::Pcm16, D::Pcm24, D::Float32][i];
                                }
                                ui.label(
                                    RichText::new(match spec.depth {
                                        D::Pcm24 => "the standard: mixes, collaborators, mastering hand-offs",
                                        D::Float32 => "keeps headroom above 0 dB - stems and heavy bounces",
                                        D::Pcm16 => "CD burns and the platforms that ask for it",
                                    })
                                    .font(plain(8.0))
                                    .color(dim),
                                );
                                ui.add_space(2.0);
                            }
                            if let Some(i) = choice(
                                ui,
                                "RENDER",
                                &["Full speed", "Realtime"],
                                usize::from(spec.realtime),
                            ) {
                                spec.realtime = i == 1;
                            }
                            if let Some(i) = choice(
                                ui,
                                "WHEN DONE",
                                &["Stay here", "Open folder"],
                                usize::from(spec.open_when_done),
                            ) {
                                spec.open_when_done = i == 1;
                            }
                            let prim = matches!(spec.primary, crate::ports::ExportFormat::Mp3);
                            if let Some(i) =
                                choice(ui, "PRIMARY", &["wav", "mp3"], usize::from(prim))
                            {
                                spec.primary = if i == 1 {
                                    crate::ports::ExportFormat::Mp3
                                } else {
                                    crate::ports::ExportFormat::Wav
                                };
                            }
                            let sec = match spec.secondary {
                                None => 0,
                                Some(crate::ports::ExportFormat::Wav) => 1,
                                Some(crate::ports::ExportFormat::Mp3) => 2,
                            };
                            if let Some(i) =
                                choice(ui, "SECONDARY", &["off", "wav", "mp3"], sec)
                            {
                                spec.secondary = match i {
                                    1 => Some(crate::ports::ExportFormat::Wav),
                                    2 => Some(crate::ports::ExportFormat::Mp3),
                                    _ => None,
                                };
                            }
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    Vec2::new(96.0, 18.0),
                                    egui::Label::new(
                                        RichText::new("FILE NAME").font(bold(9.5)).color(dim),
                                    ),
                                );
                                ui.style_mut().visuals.extreme_bg_color =
                                    egui::Color32::from_rgb(0x1A, 0x16, 0x12);
                                ui.add(
                                    egui::TextEdit::singleline(&mut spec.name)
                                        .font(plain(10.5))
                                        .text_color(egui::Color32::WHITE)
                                        .desired_width(180.0),
                                );
                            });
                            ui.add_space(2.0);
                            ui.horizontal(|ui| {
                                ui.add_sized(
                                    Vec2::new(96.0, 18.0),
                                    egui::Label::new(
                                        RichText::new("DIRECTORY").font(bold(9.5)).color(dim),
                                    ),
                                );
                                let shown = if spec.dir.is_empty() {
                                    "(project's export folder)"
                                } else {
                                    spec.dir.as_str()
                                };
                                ui.label(
                                    RichText::new(shorten_middle(shown, 30))
                                        .font(plain(9.0))
                                        .color(t.text),
                                );
                                if ui
                                    .add(Button::new(
                                        RichText::new("Browse...").font(plain(9.5)).color(t.text),
                                    ))
                                    .clicked()
                                {
                                    action = Some(DialogAction::ProjectExportBrowse);
                                }
                            });
                            ui.add_space(8.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(
                                        RichText::new("Export").font(bold(11.0)).color(t.text),
                                    ))
                                    .clicked()
                                {
                                    action = Some(DialogAction::ProjectExportRun(spec.clone()));
                                    result.close = true;
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::Track { lane, removable, icon, palette, palette_auto, serial, can_paste } => {
            let t = theme(face);
            let lane = *lane;
            let removable = *removable;
            // An EMPTY slot's menu is a different, much smaller thing: there
            // is no track here to remove, recolour or badge. `removable` is
            // what says a track exists — it is false for the buses too, which
            // is why the paste row asks `can_paste` rather than assuming.
            let empty = !removable;
            let can_paste = *can_paste;
            let current_icon = *icon;
            let palette_now = *palette;
            let palette_auto = *palette_auto;
            let serial = *serial;
            show_dialog_viewport(
                ctx,
                placement,
                "Track",
                // Sized for everything it holds, with the ICON row now in
                // the stack — and the Done row pins to the BOTTOM via a
                // height stretch, so a resize can never push it past the
                // frame again.
                // **Big enough that nothing scrolls.** It held a scroll area
                // whose offset egui remembers by id, so opening the menu,
                // scrolling, closing it and opening it on ANOTHER track came
                // up halfway down somebody else's strip. The content is a
                // handful of rows; the honest fix is a window that fits it.
                // Widened for the icon table's two rows of thirteen.
                Vec2::new(520.0, 648.0),
                Vec2::new(520.0, 648.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    let dim = t.text.gamma_multiply(0.62);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            // **Done owns a fixed seat at the frame's own
                            // corner** — computed from the window, not from
                            // however tall the content ran — and the content
                            // SCROLLS above it. There is no arithmetic left
                            // to get wrong: the button cannot leave.
                            let done_rect = {
                                let full = ui.max_rect();
                                egui::Rect::from_min_size(
                                    egui::Pos2::new(
                                        full.right() - 58.0,
                                        full.bottom() - 24.0,
                                    ),
                                    Vec2::new(56.0, 22.0),
                                )
                            };
                            {
                            // **REMOVE TRACK on top, the owner's order**: it
                            // is the destructive one and the one this menu was
                            // built around; Insert Media sits under it.
                            if removable {
                                if ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), 22.0),
                                        Button::new(
                                            RichText::new("Remove Track")
                                                .font(plain(11.0))
                                                .color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::TrackRemove(lane));
                                }
                                ui.add_space(6.0);
                                // **Copy the whole track**, for an empty slot
                                // to receive. Under Remove because it is the
                                // safe one of the pair and the eye lands on
                                // the destructive row first by design.
                                if ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), 22.0),
                                        Button::new(
                                            RichText::new("Copy Track")
                                                .font(plain(11.0))
                                                .color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::TrackCopy(lane));
                                }
                                ui.add_space(6.0);
                            }
                            // **And an EMPTY slot can receive one.** Greyed
                            // rather than hidden when nothing is held, so the
                            // slot's menu always says what it is for; a row
                            // that appears only sometimes is a row nobody
                            // learns is there.
                            if empty && lane < crate::recorder::CHANNELS {
                                let label = if can_paste {
                                    "Paste Track"
                                } else {
                                    "Paste Track (nothing copied)"
                                };
                                let btn = ui.add_enabled(
                                    can_paste,
                                    Button::new(
                                        RichText::new(label)
                                            .font(plain(11.0))
                                            .color(if can_paste { t.text } else { t.faint }),
                                    )
                                    .min_size(Vec2::new(ui.available_width(), 22.0)),
                                );
                                if btn.clicked() {
                                    action = Some(DialogAction::TrackPaste(lane));
                                }
                                ui.add_space(6.0);
                            }
                            if lane < crate::recorder::CHANNELS {
                                if ui
                                    .add_sized(
                                        Vec2::new(ui.available_width(), 26.0),
                                        Button::new(
                                            RichText::new("Insert Media...")
                                                .font(bold(12.0))
                                                .color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::InsertMedia(lane));
                                }
                                ui.label(
                                    RichText::new(
                                        "At the playhead. Pick several and each one \
                                         gets its own track.",
                                    )
                                    .font(plain(9.0))
                                    .color(dim),
                                );
                                ui.add_space(8.0);
                            }
                            if lane < crate::recorder::CHANNELS {
                                ui.label(
                                    RichText::new("ICON").font(bold(10.0)).color(dim),
                                );
                                // **Every badge in the table, two rows** -
                                // the owner's full kit-room of instruments.
                                // One source of truth: the cells ARE
                                // `recorder_panel::ICONS`, drawn by the same
                                // dispatch the strips use, so the picker can
                                // never show a badge the desk cannot wear.
                                let all = crate::recorder_panel::ICONS;
                                let per_row = all.len().div_ceil(2);
                                for row in all.chunks(per_row) {
                                    ui.horizontal(|ui| {
                                        // Thirteen cells at egui's default
                                        // 8pt gap need 512pt and the frame
                                        // gives 496: the last cell hung off
                                        // the edge. A 4pt gap fits the row
                                        // with air to spare on the right.
                                        ui.spacing_mut().item_spacing.x = 4.0;
                                        for (code, name) in row {
                                            let chosen = current_icon == *code;
                                            let (rect, resp) = ui.allocate_exact_size(
                                                Vec2::new(32.0, 26.0),
                                                egui::Sense::click(),
                                            );
                                            ui.painter().rect_filled(
                                                rect,
                                                3.0,
                                                if chosen {
                                                    t.text
                                                } else {
                                                    egui::Color32::from_black_alpha(40)
                                                },
                                            );
                                            let ink = if chosen { t.bg } else { t.text };
                                            let cell_bg = if chosen { t.text } else { t.bg };
                                            let b = egui::Rect::from_center_size(
                                                rect.center(),
                                                Vec2::splat(22.0),
                                            );
                                            if !crate::recorder_panel::draw_badge(
                                                ui.painter(),
                                                b,
                                                ink,
                                                cell_bg,
                                                *code,
                                            ) {
                                                ui.painter().text(
                                                    rect.center(),
                                                    egui::Align2::CENTER_CENTER,
                                                    *name,
                                                    plain(8.0),
                                                    ink,
                                                );
                                            }
                                            let resp = resp.on_hover_text(*name);
                                            if resp.clicked() {
                                                action = Some(DialogAction::TrackIcon(
                                                    lane, *code,
                                                ));
                                            }
                                        }
                                    });
                                }
                                ui.add_space(8.0);
                            }
                            ui.label(RichText::new("COLOUR").font(bold(10.0)).color(dim));
                            // The swatch grid: hues down, tones across —
                            // the palette overlay's own table.
                            // Only the HAND-PICKABLE part of the table: the
                            // auto-palettes' own colours live past it and are
                            // reached by choosing a palette, never a swatch.
                            let cols = crate::mixer_panel::MANUAL_COLORS
                                / crate::mixer_panel::PALETTE_COLUMNS;
                            // **The gap egui will REALLY put between them.**
                            //
                            // This divided the width by nine assuming a
                            // three-point gap, and `ui.horizontal` inserts
                            // `item_spacing.x` — eight by default, which
                            // nothing here changes. Eight columns of gap at
                            // five points too many is forty points of
                            // overflow on a three-hundred-point row: the last
                            // swatch and a good part of the one before it ran
                            // off the right-hand edge of the dialog, on every
                            // theme, since the grid was written.
                            //
                            // Asked of the ui rather than assumed, so it
                            // stays right if the dialog's spacing ever moves.
                            let gap = ui.spacing().item_spacing.x;
                            let sw = cells_across(ui.available_width(), cols, gap);
                            for row in 0..3 {
                                ui.horizontal(|ui| {
                                    for col in 0..cols {
                                        let idx = match (row, col) {
                                            (0, 0) => 0,
                                            (1, 0) => 1,
                                            (2, 0) => 2,
                                            (r, c) if c >= 1 => 3 + (c - 1) * 3 + r,
                                            _ => 0,
                                        };
                                        let (cr, cg, cb) =
                                            crate::mixer_panel::STRIP_COLORS[idx
                                                .min(crate::mixer_panel::MANUAL_COLORS - 1)];
                                        let color = if idx == 0 {
                                            egui::Color32::from_rgb(0x6B, 0x4A, 0x3A)
                                        } else {
                                            egui::Color32::from_rgb(cr, cg, cb)
                                        };
                                        let (rect, resp) = ui.allocate_exact_size(
                                            Vec2::new(sw, 16.0),
                                            egui::Sense::click(),
                                        );
                                        ui.painter().rect_filled(rect, 2.0, color);
                                        if idx == 0 {
                                            ui.painter().text(
                                                rect.center(),
                                                egui::Align2::CENTER_CENTER,
                                                "auto",
                                                plain(7.0),
                                                t.text,
                                            );
                                        }
                                        if resp.clicked() {
                                            action = Some(DialogAction::TrackColor(
                                                lane, idx as i64,
                                            ));
                                        }
                                    }
                                });
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("AUTO-PALETTE — every unpainted slot")
                                    .font(bold(10.0))
                                    .color(dim),
                            );
                            // **A scroll for the LIST alone, and it starts at
                            // the top every time.** Sixteen palettes plus AUTO
                            // will not fit any sane window, so this one does
                            // scroll — but egui remembers an offset by id
                            // forever, which is how the old whole-dialog
                            // scroll came up halfway down somebody else's
                            // strip. The id carries the lane, and the offset
                            // is reset when the window opens.
                            // **A framed WELL, filling the room that is
                            // left.** A bare scroll area with a fixed height
                            // read as "eight palettes, the last one cut off",
                            // with a huge blank slab under it that looked
                            // like the Done button's own territory. Nothing
                            // said the list moved. So it is a sunken panel
                            // with a border: an edge is what makes a region
                            // read as a region, and it now takes all the
                            // height between the heading and Done.
                            let room = (done_rect.top() - ui.min_rect().bottom() - 14.0)
                                .clamp(120.0, 460.0);
                            egui::Frame::NONE
                                .fill(t.field_bg)
                                .stroke(Stroke::new(1.0, t.text.gamma_multiply(0.45)))
                                .inner_margin(egui::Margin::same(4))
                                .corner_radius(3)
                                .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .id_salt(("track-palettes", lane, serial))
                                .max_height(room)
                                .min_scrolled_height(room)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                            // **AUTO first, and it is the default.** Every
                            // theme names a palette that suits it, so the
                            // ordinary answer is "whichever this theme says".
                            // The eight are all offered under it because
                            // mixing and matching is somebody's business but
                            // theirs — a hand-picked one then stops following
                            // the theme, which is what makes the pick stick.
                            ui.horizontal(|ui| {
                                let lit = palette_auto;
                                if ui
                                    .add_sized(
                                        Vec2::new(64.0, 18.0),
                                        Button::new(
                                            RichText::new(if lit { "> AUTO" } else { "AUTO" })
                                                .font(plain(10.0))
                                                .color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::TrackPalette(PALETTE_AUTO));
                                }
                                ui.label(
                                    RichText::new("follows the theme")
                                        .font(plain(9.0))
                                        .color(dim),
                                );
                            });
                            for (i, pal) in
                                crate::mixer_panel::PALETTES.iter().enumerate()
                            {
                                ui.horizontal(|ui| {
                                    if ui
                                        .add_sized(
                                            Vec2::new(64.0, 18.0),
                                            Button::new(
                                                RichText::new(
                                                    if !palette_auto && palette_now == i as i64 {
                                                        format!("> {}", pal.name)
                                                    } else {
                                                        pal.name.to_owned()
                                                    },
                                                )
                                                .font(plain(10.0))
                                                .color(t.text),
                                            ),
                                        )
                                        .clicked()
                                    {
                                        action =
                                            Some(DialogAction::TrackPalette(i as i64));
                                    }
                                    // Sixteen channels, then the click and the
                                    // master set a little apart — the row is
                                    // the whole desk in miniature, and the two
                                    // that are not channels should not read as
                                    // the seventeenth and eighteenth.
                                    //
                                    // **The gap between them is set to ONE
                                    // and then paid for.** Eighteen swatches
                                    // in a `horizontal` take seventeen gaps
                                    // of whatever `item_spacing.x` is — eight
                                    // by default — which is a hundred and
                                    // thirty-six points nobody budgeted for,
                                    // on a row of about two hundred. The
                                    // miniature desk ran off the right of the
                                    // dialog and took its last few channels
                                    // with it.
                                    ui.spacing_mut().item_spacing.x = 1.0;
                                    // The extra six is `add_space` between the
                                    // channels and the two that are not.
                                    let sw = cells_across(
                                        ui.available_width() - 6.0,
                                        crate::recorder::CHANNELS + 2,
                                        1.0,
                                    )
                                    .clamp(3.0, 12.0);
                                    let mut swatch = |ui: &mut egui::Ui, c: usize| {
                                        let (cr, cg, cb) = crate::mixer_panel::STRIP_COLORS
                                            [c.min(crate::mixer_panel::STRIP_COLORS.len() - 1)];
                                        let (rect, _) = ui.allocate_exact_size(
                                            Vec2::new(sw, 14.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().rect_filled(
                                            rect,
                                            2.0,
                                            egui::Color32::from_rgb(cr, cg, cb),
                                        );
                                    };
                                    for c in 0..crate::recorder::CHANNELS {
                                        swatch(ui, pal.slot(c));
                                    }
                                    ui.add_space(6.0);
                                    swatch(ui, pal.click());
                                    swatch(ui, pal.master());
                                });
                            }
                            });
                            });
                            }
                            if ui
                                .put(
                                    done_rect,
                                    Button::new(RichText::new("Done").color(t.text)),
                                )
                                .clicked()
                            {
                                result.close = true;
                            }
                        });
                },
            )
        }
        Dialog::About => {
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "About Tangent",
                Vec2::new(400.0, 220.0),
                Vec2::new(400.0, 180.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(9))
                        .show(ui, |ui| {
                            ui.vertical_centered(|ui| {
                                ui.label(
                                    RichText::new("Tangent").font(bold(16.0)).color(t.text),
                                );
                                // What 5.0 IS. The line this replaces
                                // described the 1.x chord monitor — "Simple
                                // MIDI Keyboard Monitor" — and sat directly
                                // above a 5.0 version number for the whole of
                                // the beta run.
                                ui.label(
                                    RichText::new("A recording DAW with chord intelligence")
                                        .font(bold(10.0))
                                        .color(t.text),
                                );
                                ui.label(
                                    RichText::new(
                                        "Recorder, 16-channel mixer, arrangement, MIDI editor, \
                                         drum machine, LV2/VST3",
                                    )
                                    .font(bold(8.0))
                                    .color(t.text),
                                );
                                // The author's own site. It was still the old
                                // shambhaline address, carried through the port
                                // from the Python app's spec (docs/spec/
                                // ui-spec.md 244) and never revisited.
                                ui.hyperlink_to(
                                    RichText::new("ganten.neocities.org")
                                        .font(bold(10.0))
                                        .color(t.text)
                                        .underline(),
                                    "https://ganten.neocities.org",
                                );
                                // Where to say a thing broke — the app's one
                                // support surface, so it lives where people
                                // go hunting for one. The gmail address and
                                // not the branded alias, because the alias
                                // reaches the same inbox by forwarding, and
                                // forwarding is one more thing that can rot
                                // without anyone noticing. A click copies the
                                // address rather than opening `mailto:`,
                                // which assumes a configured mail client that
                                // plenty of studio machines do not have.
                                let contact = ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(concat!(
                                                "Something broken? Write to ",
                                                "tangentdaw@gmail.com",
                                            ))
                                            .font(bold(9.0))
                                            .color(t.text),
                                        )
                                        .sense(egui::Sense::click()),
                                    )
                                    .on_hover_text("Click to copy the address");
                                if contact.clicked() {
                                    ui.ctx().copy_text("tangentdaw@gmail.com".to_owned());
                                }
                            });
                            // Stretch, then bottom block.
                            let bottom_h = 62.0;
                            ui.add_space((ui.available_height() - bottom_h).max(0.0));
                            ui.label(
                                RichText::new(concat!("Version ", env!("CARGO_PKG_VERSION")))
                                    .font(bold(8.0))
                                    .color(t.text),
                            );
                            // D-UI-6: one extra 8pt credit line under the version.
                            ui.label(
                                RichText::new(
                                    "Courier Prime © The Courier Prime Project Authors, SIL OFL 1.1",
                                )
                                .font(bold(8.0))
                                .color(t.text),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("OK").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::ColorPick { target, color } => {
            let t = theme(face);
            let target = *target;
            show_dialog_viewport(
                ctx,
                placement,
                color_pick_title(target),
                Vec2::new(320.0, 400.0),
                Vec2::new(280.0, 320.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(9))
                        .show(ui, |ui| {
                            egui::color_picker::color_picker_color32(
                                ui,
                                color,
                                egui::color_picker::Alpha::Opaque,
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("OK").color(t.text)))
                                    .clicked()
                                {
                                    // OK applies + saves; Cancel/close is a no-op (D-UI-7).
                                    action = Some(DialogAction::ApplyColor(target, *color));
                                    result.close = true;
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::Welcome { dont_show_again } => {
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Welcome to Tangent",
                Vec2::new(WELCOME_W, WELCOME_H),
                Vec2::new(WELCOME_W - 60.0, WELCOME_H - 40.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            for (i, line) in WELCOME_LINES.iter().enumerate() {
                                // The header and the signature carry the
                                // weight; the body reads in the plain hand.
                                let font = if i == 0 {
                                    bold(14.0)
                                } else if i == WELCOME_LINES.len() - 1 {
                                    bold(12.0)
                                } else {
                                    FontId::new(12.0, fonts::courier())
                                };
                                ui.label(RichText::new(*line).font(font).color(t.text));
                            }
                            ui.add_space(8.0);
                            let mut hide = *dont_show_again;
                            if ui
                                .checkbox(
                                    &mut hide,
                                    RichText::new("Don't show this again")
                                        .font(bold(11.0))
                                        .color(t.text),
                                )
                                .changed()
                            {
                                *dont_show_again = hide;
                                action = Some(DialogAction::SetShowWelcome(!hide));
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Start playing").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::Routing {
            channel,
            picking,
            hardware,
            channels,
            existing,
        } => {
            let t = theme(face);
            let dim = t.text.gamma_multiply(0.62);
            let ch = *channel;
            let step = *picking;
            show_dialog_viewport(
                ctx,
                placement,
                &format!("Routing - Channel {}", ch + 1),
                Vec2::new(420.0, 420.0),
                Vec2::new(360.0, 300.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.field_bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            let row = |ui: &mut egui::Ui, label: &str, hint: &str, on: bool| -> bool {
                                let clicked = ui
                                    .add_enabled(
                                        on,
                                        Button::new(
                                            RichText::new(label).font(bold(12.0)).color(t.text),
                                        )
                                        .min_size(Vec2::new(ui.available_width(), 24.0)),
                                    )
                                    .clicked();
                                if !hint.is_empty() {
                                    ui.label(RichText::new(hint).font(bold(9.0)).color(dim));
                                }
                                ui.add_space(4.0);
                                clicked
                            };
                            match step {
                                // ── which kind ────────────────────────────
                                None => {
                                    ui.label(
                                        RichText::new("Send this channel somewhere else.")
                                            .font(bold(11.0))
                                            .color(t.text),
                                    );
                                    ui.label(
                                        RichText::new(
                                            "Routing to a channel MOVES it there, through \
                                             whatever is loaded on it. For a copy - a reverb \
                                             feed - use the SEND knob instead.",
                                        )
                                        .font(bold(9.0))
                                        .color(dim),
                                    );
                                    ui.add_space(8.0);
                                    // ── what is already routed, each with a
                                    // way off. Shown FIRST: somebody opening
                                    // this window on a channel that already
                                    // routes is more often here to change
                                    // that than to add another.
                                    if !existing.is_empty() {
                                        ui.label(
                                            RichText::new("Now routing to:")
                                                .font(bold(10.0))
                                                .color(dim),
                                        );
                                        for (at, label, gain) in existing.iter() {
                                            ui.horizontal(|ui| {
                                                ui.label(
                                                    RichText::new(label)
                                                        .font(bold(11.0))
                                                        .color(t.text),
                                                );
                                                // The LEVEL, here rather than
                                                // only on the mixer strip:
                                                // the arrangement header has
                                                // room for the door and not
                                                // for the fader, so a person
                                                // working in that view had no
                                                // way to turn a route down.
                                                let mut g = *gain;
                                                let resp = ui.add(
                                                    egui::Slider::new(&mut g, 0.0..=1.0)
                                                        .show_value(false),
                                                );
                                                // Double-click puts it back to
                                                // unity — the keys knobs'
                                                // exemplar, on the one slider
                                                // that had no way home.
                                                if resp.double_clicked() {
                                                    g = 1.0;
                                                    action = Some(
                                                        DialogAction::SetRouteGain(*at, g),
                                                    );
                                                } else if resp.changed() {
                                                    action = Some(
                                                        DialogAction::SetRouteGain(*at, g),
                                                    );
                                                }
                                                let resp = resp.on_hover_text(
                                                    "double-click to reset",
                                                );
                                                let _ = resp;
                                                ui.label(
                                                    RichText::new(format!(
                                                        "{:>3.0}%",
                                                        g.clamp(0.0, 1.0) * 100.0
                                                    ))
                                                    .font(bold(10.0))
                                                    .color(dim),
                                                );
                                                ui.with_layout(
                                                    Layout::right_to_left(Align::Center),
                                                    |ui| {
                                                        if ui
                                                            .add(Button::new(
                                                                RichText::new("Remove")
                                                                    .font(bold(10.0))
                                                                    .color(t.text),
                                                            ))
                                                            .clicked()
                                                        {
                                                            action = Some(
                                                                DialogAction::RemoveRoute(*at),
                                                            );
                                                        }
                                                    },
                                                );
                                            });
                                        }
                                        ui.add_space(8.0);
                                    }
                                    if row(
                                        ui,
                                        // Not "Sends" — a send is a COPY, and
                                        // this moves the channel. The SND
                                        // knob is the copy, into the effects
                                        // bus, and calling both the same
                                        // thing is how you end up expecting
                                        // to hear the track twice.
                                        "Another Channel",
                                        "moves it there, through whatever is loaded on it",
                                        !channels.is_empty(),
                                    ) {
                                        action = Some(DialogAction::RoutingPick {
                                            channel: ch,
                                            hardware: false,
                                        });
                                    }
                                    if channels.is_empty() {
                                        ui.label(
                                            RichText::new("(no other channel is in use yet)")
                                                .font(bold(9.0))
                                                .color(dim),
                                        );
                                        ui.add_space(4.0);
                                    }
                                    if row(
                                        ui,
                                        "Hardware Outputs",
                                        "out of the interface on its own jacks",
                                        !hardware.is_empty(),
                                    ) {
                                        action = Some(DialogAction::RoutingPick {
                                            channel: ch,
                                            hardware: true,
                                        });
                                    }
                                    if hardware.is_empty() {
                                        ui.label(
                                            RichText::new(
                                                "(this device has no outputs to offer)",
                                            )
                                            .font(bold(9.0))
                                            .color(dim),
                                        );
                                    }
                                }
                                // ── which destination ─────────────────────
                                Some(is_hw) => {
                                    ui.label(
                                        RichText::new(if is_hw {
                                            "Pick an output:"
                                        } else {
                                            "Pick a channel to send to:"
                                        })
                                        .font(bold(11.0))
                                        .color(t.text),
                                    );
                                    ui.add_space(6.0);
                                    egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                        .max_height(ui.available_height() - 40.0)
                                        .show(ui, |ui| {
                                            if is_hw {
                                                for (label, first, stereo) in hardware.iter() {
                                                    // A destination already
                                                    // routed from here is
                                                    // greyed: the rule is no
                                                    // duplicates, and saying
                                                    // so before the click
                                                    // beats refusing after.
                                                    let free =
                                                        !existing.iter().any(|(_, l, _)| l == label);
                                                    if row(ui, label, "", free) {
                                                        action = Some(DialogAction::AddRoute {
                                                            from: ch,
                                                            target: crate::routing::Target::Hardware {
                                                                first: *first,
                                                                stereo: *stereo,
                                                            },
                                                        });
                                                    }
                                                }
                                            } else {
                                                for (idx, name) in channels.iter() {
                                                    let label =
                                                        crate::routing::Target::Channel(*idx)
                                                            .label();
                                                    let shown = if name.is_empty() {
                                                        label.clone()
                                                    } else {
                                                        format!("{label} - {name}")
                                                    };
                                                    let free =
                                                        !existing.iter().any(|(_, l, _)| *l == label);
                                                    if row(ui, &shown, "", free) {
                                                        action = Some(DialogAction::AddRoute {
                                                            from: ch,
                                                            target:
                                                                crate::routing::Target::Channel(
                                                                    *idx,
                                                                ),
                                                        });
                                                    }
                                                }
                                            }
                                        });
                                }
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Close").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                                if step.is_some()
                                    && ui
                                        .add(Button::new(RichText::new("Back").color(t.text)))
                                        .clicked()
                                {
                                    action = Some(DialogAction::OpenRouting(ch));
                                }
                            });
                        });
                },
            )
        }

        Dialog::InsertKeySource { strip, slot, current, channels } => {
            let t = theme(face);
            let dim = t.text.gamma_multiply(0.62);
            let (at, bay, now) = (*strip, *slot, *current);
            show_dialog_viewport(
                ctx,
                placement,
                "Sidechain",
                Vec2::new(420.0, 420.0),
                Vec2::new(360.0, 300.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.field_bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            // The routing picker's row, verbatim: a full-width
                            // button, greyed when it is not a choice.
                            let row = |ui: &mut egui::Ui, label: &str, on: bool| -> bool {
                                let clicked = ui
                                    .add_enabled(
                                        on,
                                        Button::new(
                                            RichText::new(label).font(bold(12.0)).color(t.text),
                                        )
                                        .min_size(Vec2::new(ui.available_width(), 24.0)),
                                    )
                                    .clicked();
                                ui.add_space(4.0);
                                clicked
                            };
                            ui.label(
                                RichText::new("Key this effect from:")
                                    .font(bold(11.0))
                                    .color(t.text),
                            );
                            ui.label(
                                RichText::new(
                                    "A copy of the chosen channel feeds the effect's \
                                     sidechain input. It does not move any audio.",
                                )
                                .font(bold(9.0))
                                .color(dim),
                            );
                            ui.add_space(8.0);
                            egui::ScrollArea::vertical()
                                .scroll_bar_visibility(
                                    egui::scroll_area::ScrollBarVisibility::AlwaysHidden,
                                )
                                .max_height(ui.available_height() - 40.0)
                                .show(ui, |ui| {
                                    // The way off first, greyed when it is
                                    // already the state — the picker must
                                    // never look like a question with no
                                    // no-thanks answer.
                                    if row(ui, "No sidechain", now.is_some()) {
                                        action = Some(DialogAction::SetInsertKeySource {
                                            strip: at,
                                            slot: bay,
                                            from: None,
                                        });
                                    }
                                    for (idx, name) in channels.iter() {
                                        let label =
                                            crate::routing::Target::Channel(*idx).label();
                                        let shown = if name.is_empty() {
                                            label
                                        } else {
                                            format!("{label} - {name}")
                                        };
                                        // The current source is greyed, the
                                        // routing picker's no-duplicates
                                        // grammar: saying "already this" before
                                        // the click beats a click that does
                                        // nothing.
                                        if row(ui, &shown, now != Some(*idx)) {
                                            action = Some(DialogAction::SetInsertKeySource {
                                                strip: at,
                                                slot: bay,
                                                from: Some(*idx),
                                            });
                                        }
                                    }
                                });
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Close").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::IrWizard { state } => {
            let t = theme(face);
            let (lcd, lcd_glass_c) = live_lcd(ctx);
            let st = crate::face::FaceStyle::new(face, lcd, lcd_glass_c);
            let s: &mut IrWizardState = state.as_mut();
            // The auto-advance onto the result page. One line here because
            // the rule itself lives on the state
            // (`IrWizardState::settle_from_live`) where a test can drive the
            // very same code: a predicate written out inside this closure
            // can only be pinned by a copy of itself, and a copy goes on
            // passing after the original has been broken.
            s.settle_from_live();
            show_dialog_viewport(
                ctx,
                placement,
                "IR Wizard",
                Vec2::new(540.0, 560.0),
                Vec2::new(460.0, 430.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.field_bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(14))
                        .show(ui, |ui| {
                            let head = match s.stage {
                                IrStage::Io => "1 / 4  THE LOOP",
                                IrStage::Level => "2 / 4  LEVELS",
                                IrStage::Sweep => "3 / 4  THE SWEEP",
                                IrStage::Result => "4 / 4  THE CAPTURE",
                            };
                            ui.label(RichText::new(head).font(bold(12.0)).color(t.text));
                            ui.add_space(8.0);
                            match s.stage {
                                IrStage::Io => draw_ir_io(ui, &t, s),
                                IrStage::Level => {
                                    draw_ir_level(ui, &t, s, &mut action)
                                }
                                IrStage::Sweep => {
                                    draw_ir_sweep(ui, &t, s, &mut action)
                                }
                                IrStage::Result => {
                                    draw_ir_result(ui, &t, &st, s, &mut action)
                                }
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                match s.stage {
                                    IrStage::Io => {
                                        if ui
                                            .add(Button::new(
                                                RichText::new("Next").color(t.text),
                                            ))
                                            .clicked()
                                        {
                                            s.stage = IrStage::Level;
                                        }
                                    }
                                    IrStage::Level => {
                                        if ui
                                            .add(Button::new(
                                                RichText::new("Next").color(t.text),
                                            ))
                                            .clicked()
                                        {
                                            // The tone must not outlive its
                                            // meter.
                                            if s.tone_on {
                                                s.tone_on = false;
                                                action = Some(DialogAction::IrTone {
                                                    on: false,
                                                    out: s.out_pair(),
                                                    inp: s.in_pair(),
                                                    level_db: s.level_db,
                                                });
                                            }
                                            s.stage = IrStage::Sweep;
                                        }
                                        if ui
                                            .add(Button::new(
                                                RichText::new("Back").color(t.text),
                                            ))
                                            .clicked()
                                        {
                                            if s.tone_on {
                                                s.tone_on = false;
                                                action = Some(DialogAction::IrTone {
                                                    on: false,
                                                    out: s.out_pair(),
                                                    inp: s.in_pair(),
                                                    level_db: s.level_db,
                                                });
                                            }
                                            s.stage = IrStage::Io;
                                        }
                                    }
                                    IrStage::Sweep => {
                                        if !s.live.capturing
                                            && ui
                                                .add(Button::new(
                                                    RichText::new("Back").color(t.text),
                                                ))
                                                .clicked()
                                        {
                                            s.stage = IrStage::Level;
                                        }
                                    }
                                    IrStage::Result => {
                                        if ui
                                            .add(Button::new(
                                                RichText::new("Done").color(t.text),
                                            ))
                                            .clicked()
                                        {
                                            result.close = true;
                                        }
                                        // **Record another IR**, not
                                        // "Retry". It is the same move —
                                        // back to the sweep page with the
                                        // loop and the levels still staged
                                        // — but the word matters: this is
                                        // the button somebody capturing a
                                        // box's whole program list presses
                                        // forty times, and calling it Retry
                                        // says the last one went wrong.
                                        // `record_another` clears the name
                                        // and the latch; nothing else.
                                        if ui
                                            .add(Button::new(
                                                RichText::new("Record another IR")
                                                    .color(t.text),
                                            ))
                                            .on_hover_text(
                                                "back to the sweep with this loop and \
                                                 these levels — capture, save, repeat",
                                            )
                                            .clicked()
                                        {
                                            s.record_another();
                                        }
                                    }
                                }
                                if s.stage != IrStage::Result
                                    && ui
                                        .add(Button::new(
                                            RichText::new("Cancel").color(t.text),
                                        ))
                                        .clicked()
                                {
                                    action = Some(DialogAction::IrCancel);
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::Confirm {
            title,
            body,
            kept,
            choices,
            warn,
        } => {
            let t = theme(face);
            // **`faint`, not `gamma_multiply`.** The quieter line here is the
            // one that says the action is SAFE, and multiplying an ink's
            // gamma is how this app has repeatedly produced text nobody can
            // read on a pale ground. `Surface::faint` blends toward the
            // ground and then walks back to 4.5:1.
            let dim = t.faint;
            // **The warning colour, measured.** `surface_of` moves the ground
            // through `legible_surface` if it landed in the dead zone and
            // then picks the ink that ground can actually carry — so a
            // destructive button reads on all sixteen themes in both modes
            // without any theme having to name a colour for it. Writing the
            // theme's crimson straight onto the button, or picking white text
            // by a brightness test, is the mistake this app has now made
            // seven times.
            let danger = crate::recorder_panel::surface_of(*warn);
            // Sized to the words: three lines of prose at a fixed width plus
            // a button row, and `kept` is usually the longest of the three.
            let lines = 1 + body.len() / 46 + if kept.is_empty() { 0 } else { 1 + kept.len() / 46 };
            let height = (108.0 + lines as f32 * 15.0).clamp(150.0, 340.0);
            show_dialog_viewport(
                ctx,
                placement,
                title,
                Vec2::new(430.0, height),
                Vec2::new(380.0, 140.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(RichText::new(&*body).font(bold(12.0)).color(t.text));
                            if !kept.is_empty() {
                                ui.add_space(8.0);
                                ui.label(RichText::new(&*kept).font(bold(10.5)).color(dim));
                            }
                            ui.add_space((ui.available_height() - 30.0).max(6.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // `choices[0]` is rightmost — see the variant.
                                for choice in choices.iter() {
                                    let button = if choice.destructive {
                                        Button::new(
                                            RichText::new(&choice.label).color(danger.ink),
                                        )
                                        .fill(danger.bg)
                                    } else {
                                        Button::new(RichText::new(&choice.label).color(t.text))
                                    };
                                    if ui.add(button).clicked() {
                                        action = Some(choice.action.clone());
                                        result.close = true;
                                    }
                                }
                                // **Cancel is always here and is always last
                                // in the layout, so it is leftmost** — the
                                // furthest button from where a hand that came
                                // to press the destructive one is already
                                // resting.
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        // **The A button's menu: which lanes this track exposes.**
        //
        // A dialog rather than a popup list, and the reason is the FX half: a
        // compressor publishes forty parameters and a synth two hundred, so
        // this needs a scroll and headings to scroll between. Ticks rather
        // than a picker, because a track shows several at once.
        Dialog::Automation {
            lane,
            who,
            groups,
            open_groups,
        } => {
            let t = theme(face);
            let lane = *lane;
            let dim = t.faint;
            show_dialog_viewport(
                ctx,
                placement,
                &format!("{who} \u{2014} Automation"),
                Vec2::new(430.0, 460.0),
                Vec2::new(360.0, 260.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let plain = |size: f32| FontId::new(size, fonts::courier());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Which lanes this track shows:")
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.add_space(6.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .auto_shrink([false, false])
                                .max_height(ui.available_height() - 40.0)
                                .show(ui, |ui| {
                                    for g in groups.iter() {
                                        let shown =
                                            open_groups.iter().any(|n| *n == g.name);
                                        let lit = g.rows.iter().filter(|r| r.on).count();
                                        // The heading: an arrow, the name, and
                                        // how many of its rows are showing —
                                        // so a shut group still says whether
                                        // anything inside it is on.
                                        let head = format!(
                                            "{} {}{}",
                                            if shown { "\u{25be}" } else { "\u{25b8}" },
                                            g.name,
                                            if lit > 0 {
                                                format!("  ({lit})")
                                            } else {
                                                String::new()
                                            },
                                        );
                                        if ui
                                            .add(
                                                egui::Button::new(
                                                    RichText::new(head)
                                                        .font(bold(12.0))
                                                        .color(t.text),
                                                )
                                                .fill(t.field_bg)
                                                .min_size(Vec2::new(
                                                    ui.available_width(),
                                                    0.0,
                                                )),
                                            )
                                            .clicked()
                                        {
                                            action = Some(
                                                DialogAction::AutomationGroup(
                                                    g.name.clone(),
                                                ),
                                            );
                                        }
                                        if !shown {
                                            ui.add_space(3.0);
                                            continue;
                                        }
                                        for row in &g.rows {
                                            let mark =
                                                if row.on { "[x]" } else { "[ ]" };
                                            let colour =
                                                if row.on { t.text } else { dim };
                                            if ui
                                                .add(
                                                    egui::Button::new(
                                                        RichText::new(format!(
                                                            "  {mark} {}",
                                                            row.label
                                                        ))
                                                        .font(plain(12.0))
                                                        .color(colour),
                                                    )
                                                    .fill(t.bg)
                                                    .min_size(Vec2::new(
                                                        ui.available_width(),
                                                        0.0,
                                                    )),
                                                )
                                                .clicked()
                                            {
                                                action = Some(
                                                    DialogAction::AutomationToggle(
                                                        lane,
                                                        row.key.clone(),
                                                    ),
                                                );
                                            }
                                        }
                                        ui.add_space(6.0);
                                    }
                                    if groups.len() == 1 {
                                        ui.add_space(4.0);
                                        ui.label(
                                            RichText::new(
                                                "No plugin in this track's bays \
                                                 publishes parameters to automate.",
                                            )
                                            .font(plain(11.0))
                                            .color(dim),
                                        );
                                    }
                                });
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(
                                        "Alt-click a lane to add a point.",
                                    )
                                    .font(plain(11.0))
                                    .color(dim),
                                );
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .add(egui::Button::new(
                                                RichText::new("Done").color(t.text),
                                            ))
                                            .clicked()
                                        {
                                            result.close = true;
                                        }
                                    },
                                );
                            });
                        });
                },
            )
        }
        Dialog::SaveTheme {
            name,
            message,
            focus,
        } => {
            let t = theme(face);
            let dim = t.text.gamma_multiply(0.62);
            show_dialog_viewport(
                ctx,
                placement,
                "Save Theme As",
                Vec2::new(430.0, 210.0),
                Vec2::new(380.0, 180.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.field_bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Save the colours you are wearing as:")
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.add_space(6.0);
                            let resp = ui.add(
                                egui::TextEdit::singleline(name)
                                    .font(bold(12.0))
                                    .text_color(t.text)
                                    .desired_width(f32::INFINITY),
                            );
                            if *focus {
                                *focus = false;
                                focus_and_select_all(ui, &resp, name);
                            }
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Saved in the \"user themes\" folder beside your settings. \
                                     Themes there survive Reset Settings to Default.",
                                )
                                .font(bold(9.5))
                                .color(dim),
                            );
                            if let Some(msg) = message.as_deref() {
                                ui.add_space(4.0);
                                ui.label(RichText::new(msg).font(bold(10.5)).color(t.text));
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let ok = !name.trim().is_empty();
                                if ui
                                    .add_enabled(
                                        ok,
                                        Button::new(RichText::new("Save").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::SaveThemeNamed(name.clone()));
                                    // NOT closed here: a failed write reports
                                    // why and keeps the name.
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::ReaStream {
            enabled,
            identifier,
            host,
            focus,
        } => {
            // ALWAYS the light theme — the owner's word ("keep the obs
            // stream menu light"): ivory ground, black text, whatever the
            // rest of the app is wearing.
            let t = theme(Color32::from_rgb(0xE3, 0xD9, 0xBD));
            let dim = t.text.gamma_multiply(0.62);
            show_dialog_viewport(
                ctx,
                placement,
                "Stream to OBS (ReaStream)",
                Vec2::new(470.0, 310.0),
                Vec2::new(410.0, 270.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            let mut on = *enabled;
                            if ui
                                .checkbox(
                                    &mut on,
                                    RichText::new("Send master out over ReaStream")
                                        .font(bold(12.0))
                                        .color(t.text),
                                )
                                .changed()
                            {
                                *enabled = on;
                                action = Some(DialogAction::SetReaStream {
                                    enabled: *enabled,
                                    identifier: identifier.clone(),
                                    host: host.clone(),
                                });
                            }
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new("Identifier (match OBS's ReaStream)")
                                    .font(bold(11.0))
                                    .color(t.text),
                            );
                            let id_resp = ui.add(
                                egui::TextEdit::singleline(identifier)
                                    .font(bold(11.0))
                                    .text_color(t.text)
                                    .desired_width(f32::INFINITY),
                            );
                            if *focus {
                                *focus = false;
                                focus_and_select_all(ui, &id_resp, identifier);
                            }
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Receiver host (127.0.0.1 for OBS on this machine)")
                                    .font(bold(11.0))
                                    .color(t.text),
                            );
                            ui.add(
                                egui::TextEdit::singleline(host)
                                    .font(bold(11.0))
                                    .text_color(t.text)
                                    .desired_width(f32::INFINITY),
                            );
                            ui.add_space(10.0);
                            ui.label(
                                RichText::new(
                                    "In OBS add the ReaStream filter, set it to Receive, and use \
                                     the same identifier. UDP port 58710.",
                                )
                                .font(bold(9.5))
                                .color(dim),
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Done").color(t.text)))
                                    .clicked()
                                {
                                    action = Some(DialogAction::SetReaStream {
                                        enabled: *enabled,
                                        identifier: identifier.clone(),
                                        host: host.clone(),
                                    });
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }
        Dialog::SupporterKey {
            input,
            message,
            installed_as,
            focus,
        } => {
            let t = theme(face);
            show_dialog_viewport(
                ctx,
                placement,
                "Supporter Key",
                Vec2::new(460.0, 310.0),
                Vec2::new(400.0, 270.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            if let Some(name) = installed_as.as_deref() {
                                ui.label(
                                    RichText::new(format!("Supporter: {name}"))
                                        .font(bold(12.0))
                                        .color(t.text),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Tangent is free, and it stays free.")
                                        .font(bold(12.0))
                                        .color(t.text),
                                );
                                ui.label(
                                    RichText::new("A key is a thank-you, nothing more.")
                                        .font(bold(12.0))
                                        .color(t.text),
                                );
                                // The store, said out loud. This dialog asks
                                // for a key and never used to say where one
                                // comes from — the only purchase link lived
                                // in the README, which a person inside the
                                // app is not reading. Still `/l/ivory`: the
                                // slug is the one commercial URL the rename
                                // deliberately left alone, because every key
                                // email ever sent points at it.
                                ui.hyperlink_to(
                                    RichText::new("ganten.gumroad.com/l/ivory")
                                        .font(bold(11.0))
                                        .color(t.text)
                                        .underline(),
                                    "https://ganten.gumroad.com/l/ivory",
                                );
                            }
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Paste your key:")
                                    .font(bold(11.0))
                                    .color(t.text),
                            );
                            let edit = egui::TextEdit::multiline(input)
                                .font(bold(11.0))
                                .text_color(t.text)
                                .desired_width(f32::INFINITY)
                                .desired_rows(5);
                            let resp = ui.add(edit);
                            if *focus {
                                *focus = false;
                                focus_and_select_all(ui, &resp, input);
                            }
                            if let Some(msg) = message.as_deref() {
                                ui.add_space(4.0);
                                ui.label(RichText::new(msg).font(bold(11.0)).color(t.text));
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let ok = !input.trim().is_empty();
                                if ui
                                    .add_enabled(
                                        ok,
                                        Button::new(RichText::new("Activate").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    action =
                                        Some(DialogAction::InstallLicense { key: input.clone() });
                                    // NOT closed here: the app decides, so a bad
                                    // key can report why without losing the paste.
                                }
                                if ui
                                    .add(Button::new(RichText::new("Close").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::TeachChord {
            notes,
            note_names,
            current_label,
            input,
            apply_all_keys,
            focus,
        } => {
            let t = theme(face);
            let notes = notes.clone();
            show_dialog_viewport(
                ctx,
                placement,
                "Teach Chord Name",
                Vec2::new(420.0, 240.0),
                Vec2::new(360.0, 200.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Notes: {note_names}"))
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.label(
                                RichText::new(format!("Detected: {current_label}"))
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.add_space(6.0);
                            ui.label(RichText::new("Name:").font(bold(11.0)).color(t.text));
                            let edit = egui::TextEdit::singleline(input)
                                .font(bold(12.0))
                                .text_color(t.text)
                                .desired_width(f32::INFINITY);
                            let resp = ui.add(edit);
                            if *focus {
                                *focus = false;
                                focus_and_select_all(ui, &resp, input);
                            }
                            ui.add_space(6.0);
                            let mut apply = *apply_all_keys;
                            if ui
                                .checkbox(
                                    &mut apply,
                                    RichText::new("Apply in all keys")
                                        .font(bold(11.0))
                                        .color(t.text),
                                )
                                .changed()
                            {
                                *apply_all_keys = apply;
                            }
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let ok_enabled = !input.trim().is_empty();
                                if ui
                                    .add_enabled(
                                        ok_enabled,
                                        Button::new(RichText::new("OK").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    action = Some(DialogAction::TeachSave {
                                        notes: notes.clone(),
                                        name: input.trim().to_owned(),
                                        apply_all_keys: *apply_all_keys,
                                    });
                                    result.close = true;
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::CorrectChord {
            notes,
            note_names,
            current_label,
            candidates,
            selected,
        } => {
            let t = theme(face);
            let notes = notes.clone();
            show_dialog_viewport(
                ctx,
                placement,
                "Correct Chord Name",
                Vec2::new(470.0, 370.0),
                Vec2::new(400.0, 290.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(format!("Notes: {note_names}"))
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.label(
                                RichText::new(format!("Now reads: {current_label}"))
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Which reading would you rather see?")
                                    .font(bold(11.0))
                                    .color(t.text),
                            );
                            ui.add_space(2.0);
                            // Explain the list's limits up front — these are the
                            // only names the re-ranker can be trained toward.
                            // 4 explanatory lines at 9pt + spacing + buttons.
                            let bottom_h = 90.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for (i, (name, score)) in candidates.iter().enumerate() {
                                        let is_current = current_label == name
                                            || current_label
                                                .strip_prefix(name.as_str())
                                                .is_some_and(|r| r.starts_with('/'));
                                        let text = if is_current {
                                            format!("{name}   (current)")
                                        } else {
                                            name.clone()
                                        };
                                        ui.horizontal(|ui| {
                                            if ui
                                                .selectable_label(
                                                    *selected == Some(i),
                                                    RichText::new(text)
                                                        .font(bold(12.0))
                                                        .color(t.text),
                                                )
                                                .clicked()
                                            {
                                                *selected = Some(i);
                                            }
                                            ui.with_layout(
                                                Layout::right_to_left(Align::Center),
                                                |ui| {
                                                    ui.label(
                                                        RichText::new(format!("{score:.0}"))
                                                            .font(bold(10.0))
                                                            .color(t.text.gamma_multiply(0.55)),
                                                    );
                                                },
                                            );
                                        });
                                    }
                                });
                            ui.add_space(4.0);
                            // Measured, not hand-waved: see
                            // tangent-core/tests/blast_radius.rs.
                            ui.label(
                                RichText::new(
                                    "Tangent learns a general leaning, not this one chord.\n\
                                     One correction changes about 1 chord in 10 overall,\n\
                                     often in unrelated keys. \"Forget Learning\" in Manage\n\
                                     Taught Chords undoes all of it exactly.",
                                )
                                .font(bold(9.0))
                                .color(t.text.gamma_multiply(0.75)),
                            );
                            ui.add_space(4.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let pick = selected.and_then(|i| candidates.get(i));
                                if ui
                                    .add_enabled(
                                        pick.is_some(),
                                        Button::new(RichText::new("Learn").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    if let Some((name, _)) = pick {
                                        action = Some(DialogAction::TrainCorrection {
                                            notes: notes.clone(),
                                            name: name.clone(),
                                        });
                                    }
                                    result.close = true;
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::LearnResult { title, message } => {
            let t = theme(face);
            let title = *title;
            let message = message.clone();
            // Size from the content. These messages range from one line to ten,
            // and a fixed height pushed the OK button off the bottom edge of the
            // longest one — where only Esc could still dismiss it. 12pt Courier
            // Prime rows are ~13.5pt; the authored lines are under 60 chars, so
            // the 446pt content width does not wrap them.
            let lines = message.lines().count().max(1) as f32;
            let height = (lines * 13.5 + 24.0 + 30.0 + 16.0).clamp(150.0, 460.0);
            show_dialog_viewport(
                ctx,
                placement,
                title,
                Vec2::new(470.0, height),
                Vec2::new(400.0, height.min(200.0)),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new(message.as_str())
                                    .font(bold(12.0))
                                    .color(t.text),
                            );
                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("OK").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::ManageTaught { rows, learning } => {
            let t = theme(face);
            let mut to_delete: Option<Vec<u8>> = None;
            let mut forget = false;
            // Snapshot for the closure; the live binding is updated afterwards.
            let learning_view = learning.clone();
            let r = show_dialog_viewport(
                ctx,
                placement,
                "Manage Taught Chords",
                Vec2::new(460.0, 460.0),
                Vec2::new(360.0, 320.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            if rows.is_empty() {
                                ui.label(
                                    RichText::new("No taught chords yet.")
                                        .font(bold(12.0))
                                        .color(t.text),
                                );
                            }
                            // List, then the D-UI-9 learning footer, then the
                            // buttons. Reserve the footer's REAL height (it grows
                            // one 9pt line per non-zero weight, up to seven) —
                            // a fixed guess clips the buttons off the bottom
                            // once training fills every feature in.
                            const FOOTER_LINE_H: f32 = 13.0;
                            let weight_rows = learning_view
                                .weights
                                .iter()
                                .filter(|(_, w)| *w != 0.0)
                                .count() as f32;
                            let footer_h = 8.0 // separator
                                + 16.0 // status line
                                + if learning_view.has_learned {
                                    FOOTER_LINE_H * (1.0 + weight_rows)
                                } else {
                                    FOOTER_LINE_H * 2.0 // the two-line hint
                                };
                            let bottom_h = 34.0 + footer_h + 6.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for row in rows.iter() {
                                        ui.horizontal(|ui| {
                                            if ui
                                                .add(Button::new(
                                                    RichText::new("Delete").color(t.text),
                                                ))
                                                .clicked()
                                            {
                                                to_delete = Some(row.intervals.clone());
                                            }
                                            ui.label(
                                                RichText::new(format!(
                                                    "{}   [{}]",
                                                    row.display_name, row.voicing
                                                ))
                                                .font(bold(12.0))
                                                .color(t.text),
                                            );
                                        });
                                    }
                                });
                            ui.add_space(6.0);
                            ui.separator();
                            ui.label(
                                RichText::new(format!(
                                    "Chord learning: {}  ({} correction{})",
                                    if learning_view.on { "ON" } else { "off" },
                                    learning_view.corrections,
                                    if learning_view.corrections == 1 {
                                        ""
                                    } else {
                                        "s"
                                    },
                                ))
                                .font(bold(11.0))
                                .color(t.text),
                            );
                            if learning_view.has_learned {
                                ui.label(
                                    RichText::new("Learned leanings:")
                                        .font(bold(9.0))
                                        .color(t.text.gamma_multiply(0.75)),
                                );
                                for (label, w) in learning_view.weights.iter() {
                                    if *w == 0.0 {
                                        continue;
                                    }
                                    ui.label(
                                        RichText::new(format!(
                                            "   {label}: {}{:.0}",
                                            if *w > 0.0 { "+" } else { "" },
                                            w
                                        ))
                                        .font(bold(9.0))
                                        .color(t.text.gamma_multiply(0.75)),
                                    );
                                }
                            } else {
                                // Forget leaves the switch armed but the weights
                                // empty. Say outright that readings are stock, or
                                // "ON (0 corrections)" reads as a failed undo.
                                ui.label(
                                    RichText::new(if learning_view.on {
                                        "Nothing learned - chord names are stock.\n\
                                         Use \"Correct Chord Name...\" to teach a leaning."
                                    } else {
                                        "Nothing learned yet. Play a chord, then use\n\
                                         \"Correct Chord Name...\" in the menu."
                                    })
                                    .font(bold(9.0))
                                    .color(t.text.gamma_multiply(0.75)),
                                );
                            }
                            ui.add_space(6.0);
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add(Button::new(RichText::new("Close").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                                if ui
                                    .add_enabled(
                                        learning_view.has_learned,
                                        Button::new(RichText::new("Forget Learning").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    forget = true;
                                }
                            });
                        });
                },
            );
            if let Some(intervals) = to_delete {
                // Reflect the deletion locally so the list updates immediately,
                // and tell the app to persist it.
                rows.retain(|r| r.intervals != intervals);
                action = Some(DialogAction::DeleteOverride { intervals });
            }
            if forget {
                // Same immediate-feedback pattern as Delete: clear the footer
                // locally so the dialog updates now, and let the app persist.
                learning.has_learned = false;
                learning.corrections = 0;
                learning.weights.clear();
                action = Some(DialogAction::ForgetLearning);
            }
            r
        }

        Dialog::CustomTuning {
            name,
            strings,
            prefer_flats,
            focus,
        } => {
            let t = theme(face);
            let flats = *prefer_flats;
            // Asked once, outside the loop: see `highest_open_pitch`. The
            // spinner is clamped to it, so the one refusal a user could hit by
            // dragging is not reachable by dragging at all.
            let ceiling = highest_open_pitch();
            show_dialog_viewport(
                ctx,
                placement,
                "Custom Tuning",
                Vec2::new(TUNING_DIALOG_W, TUNING_DIALOG_H),
                Vec2::new(440.0, 320.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    // Text fields and number spinners paint their interiors on
                    // `extreme_bg_color`, which is stock near-black otherwise:
                    // a row of holes punched through a themed dialog.
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let faint = t.text.gamma_multiply(0.72);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(TUNING_MARGIN))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                ui.label(RichText::new("Name:").font(bold(11.0)).color(t.text));
                                let resp = ui.add(
                                    egui::TextEdit::singleline(name)
                                        .font(bold(12.0))
                                        .text_color(t.text)
                                        .desired_width(f32::INFINITY),
                                );
                                if *focus {
                                    *focus = false;
                                    focus_and_select_all(ui, &resp, name);
                                }
                            });

                            // ── start from a preset ─────────────────────────
                            ui.add_space(6.0);
                            ui.label(
                                RichText::new("Start from:").font(bold(10.0)).color(faint),
                            );
                            ui.horizontal_wrapped(|ui| {
                                for preset in fretboard::TUNINGS {
                                    // Marked while the rows still hold exactly
                                    // this preset's pitches, so "standard
                                    // but…" stops claiming to be standard the
                                    // moment it stops being standard.
                                    let untouched = strings.len() == preset.open.len()
                                        && strings
                                            .iter()
                                            .zip(preset.open.iter())
                                            .all(|(row, p)| row.pitch == *p);
                                    if ui
                                        .selectable_label(
                                            untouched,
                                            RichText::new(preset.name.as_ref())
                                                .font(bold(10.0))
                                                .color(t.text),
                                        )
                                        .clicked()
                                    {
                                        // Pitches ONLY. The name belongs to
                                        // the user: it is typed once while the
                                        // presets are clicked several times
                                        // trying them on, so overwriting it
                                        // would throw away typing that cannot
                                        // be got back.
                                        *strings = rows_for(&preset.open, flats);
                                    }
                                }
                            });

                            // ── how many strings ────────────────────────────
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(format!("Strings: {}", strings.len()))
                                        .font(bold(11.0))
                                        .color(t.text),
                                );
                                ui.add_space(6.0);
                                // Buttons rather than a count field, and the
                                // word "low" in both labels, because the count
                                // alone cannot say WHICH end changes — and
                                // which end changes is the whole difference
                                // between the obvious behaviour and the
                                // annoying one (see `add_low_string`).
                                if ui
                                    .add_enabled(
                                        strings.len() < MAX_STRINGS,
                                        Button::new(
                                            RichText::new("Add low string").color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    add_low_string(strings, flats);
                                }
                                if ui
                                    .add_enabled(
                                        strings.len() > 1,
                                        Button::new(
                                            RichText::new("Remove low string").color(t.text),
                                        ),
                                    )
                                    .clicked()
                                {
                                    remove_low_string(strings);
                                }
                            });

                            // ── the strings themselves ──────────────────────
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(
                                    "Highest string at the top. They are numbered from the \
                                     LOWEST, the way the message below counts them.",
                                )
                                .font(bold(9.0))
                                .color(faint),
                            );
                            // The interval up from the string below. A player
                            // reads a tuning by its gaps — standard is
                            // 5 5 5 4 5 — and a NEGATIVE one is the monotone
                            // break made visible before Create is even
                            // reached. Snapshotted before the rows are drawn
                            // because a row cannot see its neighbour through
                            // an `iter_mut`; one frame of lag on a decoration.
                            let gaps: Vec<Option<i16>> = strings
                                .iter()
                                .enumerate()
                                .map(|(i, row)| {
                                    i.checked_sub(1).map(|below| {
                                        row.pitch as i16 - strings[below].pitch as i16
                                    })
                                })
                                .collect();
                            // The message band, the button row, and the space
                            // between them. See `TUNING_FOOTER_H` for why the
                            // band is a reservation rather than a measurement.
                            let bottom_h = TUNING_FOOTER_H + 34.0 + 8.0;
                            let list_h = (ui.available_height() - bottom_h).max(40.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(list_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // Drawn highest-first, which is how tab and
                                    // every chord chart are read, while the
                                    // numbering still counts from the lowest
                                    // string so it agrees with `TuningError`.
                                    for (i, row) in strings.iter_mut().enumerate().rev() {
                                        ui.horizontal(|ui| {
                                            ui.label(
                                                RichText::new(format!("{:>2}", i + 1))
                                                    .font(bold(11.0))
                                                    .color(faint),
                                            );
                                            let typed = ui.add(
                                                egui::TextEdit::singleline(&mut row.text)
                                                    .font(bold(12.0))
                                                    .text_color(t.text)
                                                    .desired_width(58.0),
                                            );
                                            if typed.changed() {
                                                // A row whose text does not
                                                // parse keeps the last pitch
                                                // that did, so the spinner
                                                // never shows a number nobody
                                                // chose while a name is
                                                // half-typed.
                                                if let Ok(p) = parse_pitch(&row.text) {
                                                    row.pitch = p;
                                                }
                                            }
                                            let dragged = ui.add(
                                                egui::DragValue::new(&mut row.pitch)
                                                    .range(0..=ceiling),
                                            );
                                            if dragged.changed() {
                                                // `confirm` reads the FIELD, so
                                                // a spinner that moved the
                                                // number without rewriting the
                                                // text would build a tuning out
                                                // of a value the user can no
                                                // longer see.
                                                row.text = pitch_name(row.pitch, flats);
                                            }
                                            if let Some(Some(gap)) = gaps.get(i) {
                                                ui.label(
                                                    RichText::new(format!("{gap:+}"))
                                                        .font(bold(10.0))
                                                        // Full strength when it
                                                        // descends: the theme
                                                        // has no error colour,
                                                        // and weight is the one
                                                        // emphasis that works
                                                        // in both themes.
                                                        .color(if *gap < 0 {
                                                            t.text
                                                        } else {
                                                            faint
                                                        }),
                                                );
                                            }
                                        });
                                    }
                                });

                            // ── what is wrong, or what will be made ─────────
                            //
                            // Computed AFTER the rows are drawn, so the message
                            // answers this frame's keystroke rather than the
                            // one before it, and always occupies the same band
                            // whether it is a complaint or a confirmation.
                            let ready = confirm(name, strings);
                            let footer = match &ready {
                                Ok(built) => format!(
                                    "{} strings: {}",
                                    built.strings(),
                                    built
                                        .open
                                        .iter()
                                        .map(|&p| pitch_name(p, flats))
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                ),
                                Err(problem) => problem.message(flats),
                            };
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(footer)
                                    .font(bold(TUNING_FOOTER_PT))
                                    .color(if ready.is_ok() { faint } else { t.text }),
                            );

                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui
                                    .add_enabled(
                                        ready.is_ok(),
                                        Button::new(RichText::new("Use Tuning").color(t.text)),
                                    )
                                    .clicked()
                                {
                                    // Closes only by producing a tuning. A
                                    // disabled button cannot be clicked, so
                                    // this is belt and braces — but the belt is
                                    // that there is exactly one place a
                                    // `Tuning` is built, and it is the same
                                    // value that enabled the button.
                                    if let Ok(built) = &ready {
                                        action = Some(DialogAction::SetCustomTuning(
                                            built.clone(),
                                        ));
                                        result.close = true;
                                    }
                                }
                                if ui
                                    .add(Button::new(RichText::new("Cancel").color(t.text)))
                                    .clicked()
                                {
                                    result.close = true;
                                }
                            });
                        });
                },
            )
        }

        Dialog::Export {
            spec,
            tempo_text,
            remember,
            post_take,
            had_camera,
        } => {
            let t = theme(face);
            let post_take = *post_take;
            // Asked once, outside the drawing, because every greyed control in
            // the dialog is greyed by this one value. See `Rederivable`.
            let can = rederivable(post_take, *had_camera);
            show_dialog_viewport(
                ctx,
                placement,
                // The two entry points mean different things and the title bar
                // is the cheapest place to say which one this is.
                if post_take {
                    "Export This Take"
                } else {
                    "Export"
                },
                Vec2::new(EXPORT_DIALOG_W, EXPORT_DIALOG_H),
                Vec2::new(470.0, 340.0),
                |ui, result| {
                    apply_theme(ui.style_mut(), &t);
                    // The tempo field paints its interior on `extreme_bg_color`
                    // — stock near-black otherwise, a hole punched through a
                    // themed dialog. Same trap as the tuning editor.
                    ui.visuals_mut().extreme_bg_color = t.bg;
                    ui.painter().rect_filled(ui.max_rect(), 0.0, t.bg);
                    let bold = |size: f32| FontId::new(size, fonts::courier_bold());
                    let faint = t.text.gamma_multiply(0.72);
                    egui::Frame::NONE
                        .inner_margin(egui::Margin::same(EXPORT_MARGIN))
                        .show(ui, |ui| {
                            // The footer band and the button row are reserved
                            // first; everything else scrolls. Inline in a
                            // plugin the canvas can be shorter than the
                            // dialog, and Export has to stay reachable there
                            // too — that is the whole reason the funnel exists.
                            let bottom_h = EXPORT_FOOTER_H + 34.0 + 8.0;
                            let body_h = (ui.available_height() - bottom_h).max(60.0);
                            egui::ScrollArea::vertical()
                            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                                .max_height(body_h)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    // ── always written ──────────────────────
                                    //
                                    // No filenames here, unlike the §5 mock:
                                    // the take's name is built by
                                    // `tangent_record::take` and this crate is
                                    // never handed it (see `recorder.rs` on why
                                    // the band is given strings rather than
                                    // computing them). What each file IS is the
                                    // part the dialog can say truthfully.
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "Always written", false);
                                        export_check(ui, &t, true, &mut spec.audio, "Audio");
                                        ui.label(
                                            RichText::new("48 kHz 24-bit stereo .wav")
                                                .font(bold(9.0))
                                                .color(faint),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "", false);
                                        export_check(ui, &t, true, &mut spec.midi, "MIDI");
                                        ui.label(
                                            RichText::new("SMF 1, 960 ppq, tick 0 at take start")
                                                .font(bold(9.0))
                                                .color(faint),
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "", false);
                                        // Live after a take: rewriting the
                                        // tempo meta is a file edit, which is
                                        // the one thing on this dialog that is
                                        // re-derivable without question. Dead
                                        // with no MIDI, because there would be
                                        // no file to write it into.
                                        let tempo_on = can.tempo && spec.midi;
                                        ui.label(
                                            RichText::new("Tempo mark")
                                                .font(bold(11.0))
                                                .color(t.text),
                                        );
                                        ui.add_enabled(
                                            tempo_on,
                                            egui::TextEdit::singleline(tempo_text)
                                                .font(bold(12.0))
                                                .text_color(t.text)
                                                .desired_width(52.0),
                                        );
                                        ui.label(
                                            RichText::new("BPM").font(bold(11.0)).color(t.text),
                                        );
                                    });

                                    // ── video ───────────────────────────────
                                    ui.add_space(6.0);
                                    for (i, m) in VideoMode::ALL.into_iter().enumerate() {
                                        ui.horizontal(|ui| {
                                            export_label(
                                                ui,
                                                &t,
                                                if i == 0 { "Video" } else { "" },
                                                false,
                                            );
                                            export_radio(
                                                ui,
                                                &t,
                                                m == VideoMode::None || VIDEO_EXPORT_READY,
                                                &mut spec.video,
                                                m,
                                                m.label(),
                                            );
                                        });
                                    }
                                    if !VIDEO_EXPORT_READY {
                                        export_para(ui, &t, VIDEO_PENDING_NOTE);
                                    }

                                    // ── what is in the composite ────────────
                                    //
                                    // The camera and display ticks are read by
                                    // the per-source modes too (they are what
                                    // `encoder_count` counts), so they follow
                                    // "is there a video at all" rather than
                                    // "is there a composite". The layout is the
                                    // only control that is about the composite
                                    // specifically.
                                    let video_on = spec.video.wants_video();
                                    ui.add_space(6.0);
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "Composite contains", true);
                                        export_check(
                                            ui,
                                            &t,
                                            video_on && can.camera,
                                            &mut spec.composite.camera,
                                            "Camera",
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "", true);
                                        export_check(
                                            ui,
                                            &t,
                                            video_on,
                                            &mut spec.composite.display,
                                            "Tangent's display",
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "", true);
                                        export_check(
                                            ui,
                                            &t,
                                            video_on,
                                            &mut spec.composite.audio,
                                            "Performance audio",
                                        );
                                    });
                                    // **No layout picker.** A take is the
                                    // window: the same bands in the same
                                    // order, with the camera where the window
                                    // already puts it — full height at the
                                    // top-left of the recorder band. There is
                                    // nothing left to arrange, and five radio
                                    // buttons offering arrangements that no
                                    // longer happen would be five ways to be
                                    // told the video will look like something
                                    // it will not.
                                    //
                                    // `VideoLayout` still exists and is still
                                    // stored; nothing reads it. It comes out
                                    // after the beta.
                                    let _ = &can.layout;
                                    // Said here rather than at the bottom,
                                    // because it is the answer to "why can I
                                    // not click these".
                                    if let Some(why) = can.why {
                                        export_para(ui, &t, why);
                                    }

                                    // ── which panels the display draws ──────
                                    //
                                    // The app's own panel names, not a second
                                    // vocabulary invented for the video. These
                                    // override the live settings FOR THE VIDEO
                                    // ONLY — recording a clean piano-and-chord
                                    // video while keeping the fretboard on
                                    // screen for your own use is the case this
                                    // exists for. `shows_from_settings` seeds
                                    // them.
                                    let shows_on =
                                        video_on && spec.composite.display && can.display;
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "Display shows", true);
                                        export_check(
                                            ui,
                                            &t,
                                            shows_on,
                                            &mut spec.composite.shows.piano,
                                            "Piano",
                                        );
                                        export_check(
                                            ui,
                                            &t,
                                            shows_on,
                                            &mut spec.composite.shows.chord,
                                            "Chord name",
                                        );
                                    });
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "", true);
                                        export_check(
                                            ui,
                                            &t,
                                            shows_on,
                                            &mut spec.composite.shows.fretboard,
                                            "Fretboard",
                                        );
                                        export_check(
                                            ui,
                                            &t,
                                            shows_on,
                                            &mut spec.composite.shows.theory,
                                            "Theory",
                                        );
                                    });

                                    // ── frame size and rate ─────────────────
                                    ui.add_space(6.0);
                                    for (i, chunk) in Resolution::ALL.chunks(2).enumerate() {
                                        ui.horizontal(|ui| {
                                            export_label(
                                                ui,
                                                &t,
                                                if i == 0 { "Resolution" } else { "" },
                                                true,
                                            );
                                            for &r in chunk {
                                                // "Match camera" after a take
                                                // would be matching a camera
                                                // that is not running and whose
                                                // frames were never kept.
                                                let on = video_on
                                                    && (r != Resolution::MatchCamera || can.camera);
                                                export_radio(
                                                    ui,
                                                    &t,
                                                    on,
                                                    &mut spec.resolution,
                                                    r,
                                                    r.label(),
                                                );
                                            }
                                        });
                                    }
                                    ui.horizontal(|ui| {
                                        export_label(ui, &t, "Frame rate", true);
                                        for f in FPS_CHOICES {
                                            export_radio(
                                                ui,
                                                &t,
                                                video_on,
                                                &mut spec.fps,
                                                f,
                                                &format!("{f} fps"),
                                            );
                                        }
                                    });
                                    export_para(ui, &t, encoder_note());
                                });

                            // ── what it costs, or what is wrong ─────────────
                            //
                            // Computed AFTER the body, so the footer answers
                            // this frame's click rather than the one before it,
                            // and occupying the same band either way so the
                            // buttons do not walk under the cursor.
                            let built = export_ready(spec, tempo_text);
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new(estimate_line(spec))
                                    .font(bold(EXPORT_FOOTER_PT))
                                    .color(faint),
                            );
                            if let Err(why) = &built {
                                ui.label(
                                    RichText::new(why.as_str())
                                        .font(bold(EXPORT_FOOTER_PT))
                                        .color(t.text),
                                );
                            }

                            ui.add_space((ui.available_height() - 30.0).max(0.0));
                            ui.horizontal(|ui| {
                                let mut keep = *remember;
                                if ui
                                    .checkbox(
                                        &mut keep,
                                        RichText::new("Use these settings for every take")
                                            .font(bold(11.0))
                                            .color(t.text),
                                    )
                                    .changed()
                                {
                                    *remember = keep;
                                }
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    if ui
                                        .add_enabled(
                                            built.is_ok(),
                                            Button::new(RichText::new("Export").color(t.text)),
                                        )
                                        .clicked()
                                    {
                                        // Closes only by producing a spec, and
                                        // it is the same value that enabled the
                                        // button — `export_ready` is the one
                                        // gate, so nothing invalid can reach
                                        // the app even if the button were
                                        // somehow clicked while greyed.
                                        if let Ok(s) = &built {
                                            action = Some(export_action(*s, *remember));
                                            result.close = true;
                                        }
                                    }
                                    if ui
                                        .add(Button::new(RichText::new("Cancel").color(t.text)))
                                        .clicked()
                                    {
                                        result.close = true;
                                    }
                                });
                            });
                        });
                },
            )
        }
    };

    if result.close {
        *dialog_opt = None;
    }
    // The arm's own ask first: it is a hand on a control, and the theme key
    // is one press that can simply be pressed again. Both on one frame is
    // a T struck mid-drag, which nobody does twice.
    if result.cycle_theme {
        action = action.or(Some(DialogAction::CycleTheme));
    }
    action
}

#[cfg(test)]
mod tests {

    /// **Everything Tangent ships is a sentinel, and nothing else is.**
    ///
    /// [`is_builtin`] is one `starts_with`, and it replaced the same two-way
    /// comparison written out in ten places. What it rests on is that no
    /// filesystem path begins with `@` — so this checks the assumption from
    /// both ends rather than trusting the sentence on [`BUILTIN_PATH`].
    #[test]
    fn every_built_in_is_recognised_and_no_real_path_is() {
        for path in [super::BUILTIN_PATH, super::DRUMPAD_PATH, super::KEYS_PATH] {
            assert!(super::is_builtin(path), "{path} was not recognised");
        }
        for (path, ..) in super::FX_UNITS {
            assert!(super::is_builtin(path), "{path} was not recognised");
        }
        for path in [
            "/Library/Audio/Plug-Ins/VST3/Pianoteq 8.vst3",
            "C:\\Program Files\\Common Files\\VST3\\Vital.vst3",
            "~/tangent-plate.vst3",
            "",
        ] {
            assert!(!super::is_builtin(path), "{path:?} was taken for a built-in");
        }
    }

    /// **The patch list and the settings index are one contract.**
    ///
    /// `settings.keys_patch` is a NUMBER, and the engine turns it back into an
    /// instrument by position. Reordering one list without the other does not
    /// fail to compile and does not fail to run: it silently loads the piano
    /// when the electric piano was asked for. This pins the length and the
    /// order, so the reorder shows up here instead of in somebody's ears.
    #[test]
    fn keys_patches_are_named_in_engine_order() {
        assert_eq!(super::KEYS_PATCH_NAMES.len(), super::KEYS_PATCHES);
        assert_eq!(super::KEYS_PATCH_NAMES[0].0, "Piano 1");
        assert_eq!(super::KEYS_PATCH_NAMES[1].0, "EP Mk.1");
        for (name, about) in super::KEYS_PATCH_NAMES {
            assert!(!name.is_empty() && !about.is_empty());
            // The reference belongs in the description, never the name. See
            // LICENSING.md, and the same rule FX_UNITS follows.
            for claim in ["Rhodes", "Fender", "Wurlitzer", "Steinway", "Mark I"] {
                assert!(!name.contains(claim), "{name} names somebody else's instrument");
            }
        }
    }

    /// **The rows are unique, and their names carry no trademark.**
    ///
    /// The second half is the one that matters and it is not decoration. A row
    /// is the effect's NAME; the hardware it is voiced after belongs in the
    /// description beside it, where naming it is a reference to somebody
    /// else's product rather than a claim about this one. That is the
    /// difference the whole position rests on, it is what
    /// `recorder_panel::Fx::subtitle` has done since the phaser, and it is one
    /// careless edit away from being lost — so it is a test and not a comment.
    #[test]
    fn no_effect_is_named_after_hardware_it_only_refers_to() {
        let marks = [
            "emt", "roland", "space echo", "re-201", "boss", "ce-1", "mxr",
            "phase 90", "140", "fender", "ibanez", "screamer", "vox",
            "cry baby", "cedar",
        ];
        // Both shelves under the one rule: the fixed chain's rows and the
        // suite's insert units, because the suite reuses three of the
        // chain's names and the day someone renames one is the day the rule
        // needs to be standing on both.
        let rows = super::FX_UNITS.iter().chain(crate::faces::SUITE_UNITS.iter());
        for (path, name, what) in rows.clone() {
            let lower = name.to_lowercase();
            for mark in marks {
                assert!(
                    !lower.contains(mark),
                    "the row for {path} is called {name:?}, which carries \
                     {mark:?} — that belongs in the description, not the name"
                );
            }
            assert!(
                name.starts_with("Tangent "),
                "{name:?} does not say whose effect it is"
            );
            assert!(!what.is_empty(), "{name} has nothing under it");
        }
        for (i, (path, name, _)) in super::FX_UNITS.iter().enumerate() {
            for (other, other_name, _) in &super::FX_UNITS[i + 1..] {
                assert_ne!(path, other, "two rows share the path {path}");
                assert_ne!(name, other_name, "two rows share the name {name}");
            }
        }
        // Within the suite the same uniqueness holds; ACROSS the two lists
        // the chorus, echo and plate share on purpose — they are one effect
        // each, reachable from two doors — so cross-list sharing must be
        // exactly that set and no more.
        for (i, (path, name, _)) in crate::faces::SUITE_UNITS.iter().enumerate() {
            for (other, other_name, _) in &crate::faces::SUITE_UNITS[i + 1..] {
                assert_ne!(path, other, "two suite rows share the path {path}");
                assert_ne!(name, other_name, "two suite rows share the name {name}");
            }
        }
        let shared_on_purpose = ["@tangent-chorus", "@tangent-echo", "@tangent-plate"];
        for (path, name, _) in super::FX_UNITS {
            let in_suite = crate::faces::SUITE_UNITS.iter().find(|(p, ..)| *p == path);
            if shared_on_purpose.contains(&path) {
                let (_, suite_name, _) =
                    in_suite.expect("a declared-shared sentinel left the suite");
                assert_eq!(
                    *suite_name, name,
                    "{path} wears two names, one per door"
                );
            } else {
                assert!(
                    in_suite.is_none(),
                    "{path} appeared in both lists without being declared shared"
                );
            }
        }
    }

    /// **A row of cells and its gaps fit the space it was given.**
    ///
    /// The track dialog's colour grid ran off the right-hand edge of the
    /// dialog for as long as it existed, and nothing said so: `ui.horizontal`
    /// inserts `item_spacing.x` between every pair of cells, egui clips what
    /// does not fit, and a clipped swatch looks exactly like a swatch that
    /// was never drawn. One row assumed a three-point gap where there were
    /// eight; the other budgeted eight points for seventeen of them.
    #[test]
    fn a_row_of_swatches_fits_the_space_it_was_given() {
        for available in [120.0_f32, 200.0, 336.0, 900.0] {
            for count in [1_usize, 2, 9, 18, 27] {
                for gap in [0.0_f32, 1.0, 3.0, 8.0] {
                    let w = cells_across(available, count, gap);
                    assert!(w.is_finite() && w >= 0.0, "{w} is not a width");
                    let gaps = (count - 1) as f32 * gap;
                    // **Only when the gaps themselves fit.** Eighteen
                    // eight-point gaps do not go into a hundred and twenty
                    // points however narrow the cells are, and no width this
                    // function could return would make them: that row needs a
                    // smaller GAP, which is what the palette row does. Zero
                    // is the honest answer and the caller's cue.
                    if gaps > available {
                        assert_eq!(w, 0.0, "{count} cells claimed room that is not there");
                        continue;
                    }
                    let used = w * count as f32 + gaps;
                    assert!(
                        used <= available + 0.01,
                        "{count} cells at {w} with a {gap} gap take {used} of {available}"
                    );
                    // ...and it does not waste the space either: a row that
                    // fits in half the width is a grid with a hole in it.
                    assert!(
                        used >= available - 0.01,
                        "{count} cells left {} unused of {available}",
                        available - used
                    );
                }
            }
        }
        // Degenerate asks answer a number rather than a NaN or a negative
        // width, either of which is an inverted rect in the painter.
        assert_eq!(cells_across(100.0, 0, 4.0), 0.0);
        assert_eq!(cells_across(4.0, 27, 8.0), 0.0);
        assert!(cells_across(100.0, 4, -8.0) > 0.0);
    }
    use super::*;

    /// **Every theme dresses the confirmation legibly, warning and all.**
    ///
    /// The destructive button is a NEW SURFACE, and this app has learned the
    /// same lesson about new surfaces seven times: the ink belongs to the
    /// surface and is measured, never inherited from a themed role and never
    /// picked by a brightness threshold. Every one of those seven was found
    /// by a screenshot rather than a test, so here is the test.
    ///
    /// It is also the reason the warning colour needs no theme role of its
    /// own. `master_knob_color` is already per-theme — it is the one control
    /// that can undo every other one, which is exactly what a destructive
    /// button is — and `surface_of` measures whatever that colour turns out
    /// to be, moving the ground out of the 0.12–0.22 dead zone if it landed
    /// there.
    /// **The encoder note tells each platform the truth about its own CPU.**
    ///
    /// Windows was told "runs on Media Foundation, so three 1080p30 streams
    /// cost the CPU almost nothing" — and Windows pipes frames to ffmpeg,
    /// invoked as `-c:v libx264 -preset veryfast`. x264, on the CPU. The one
    /// platform promised the encode was free was the one paying most, and the
    /// warning it needed was already written in the branch next door.
    ///
    /// Asserted as a PROPERTY rather than by string, so rewording the note
    /// does not fail this and changing which platform is hardware does: only
    /// a target that really encodes in hardware may promise "almost nothing".
    #[test]
    fn only_a_hardware_encoder_promises_it_is_free() {
        let note = encoder_note();
        // macOS is the only native/hardware path — see `encode.rs`: "macOS
        // encodes natively: AVFoundation driving VideoToolbox", and "Windows
        // and Linux pipe raw frames into ffmpeg".
        let hardware = cfg!(target_os = "macos");
        let promises_free = note.contains("almost \nnothing")
            || note.contains("almost nothing")
            || note.contains("cost the CPU almost");
        assert_eq!(
            promises_free, hardware,
            "this target {} encode in hardware, and the note says {:?}",
            if hardware { "does" } else { "does NOT" },
            note
        );
        if !hardware {
            assert!(
                note.contains("software encoder"),
                "a software target must say so: {note:?}"
            );
        }
        // And the name of an API this app does not use must not appear.
        assert!(
            !note.contains("Media Foundation"),
            "the note names Media Foundation, which nothing here encodes with"
        );
    }

    #[test]
    fn every_theme_dresses_a_confirmation_legibly_in_both_modes() {
        let mut bad: Vec<String> = Vec::new();
        let mut warns = std::collections::HashSet::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let mode = if dark { "dark" } else { "light" };
                let name = &theme.name;

                // The card itself, from the desk's face like every other
                // floating panel in the app.
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
                let t = theme_of(face);
                let body = crate::recorder_panel::contrast_ratio(t.text, t.bg);
                if body < 4.5 {
                    bad.push(format!("{name} {mode}: body text {body:.2}:1"));
                }
                // The quieter "Kept:" line, which is the one somebody has to
                // read to know the action is SAFE. Held to the same bar as
                // the body: a reassurance nobody can read is not one.
                let kept = crate::recorder_panel::contrast_ratio(t.faint, t.bg);
                if kept < 4.5 {
                    bad.push(format!("{name} {mode}: the kept line {kept:.2}:1"));
                }

                // The destructive button.
                let danger = crate::recorder_panel::surface_of(
                    set.master_knob_color.to_color32(),
                );
                let label = crate::recorder_panel::contrast_ratio(danger.ink, danger.bg);
                if label < 4.5 {
                    bad.push(format!("{name} {mode}: the Reset label {label:.2}:1"));
                }
                // And it has to be TELLABLE from the ordinary buttons beside
                // it, or the warning is decoration.
                //
                // **Measured as ΔE and not as a contrast ratio**, and the
                // first draft of this test got it wrong in exactly the way
                // this app has been caught before. Contrast ratio is
                // luminance ONLY, so the theme's crimson against a mid-grey
                // button scores 1.50:1 on Default — which reads as "these are
                // nearly the same" while they are plainly a red button and a
                // grey one. Every theme fails a 3:1 luminance bar here and
                // every theme passes by an enormous margin perceptually: the
                // measured range is 53 to 91, where 25 is already "obviously
                // a different colour". The bar below is the floor of that
                // range with room to spare.
                //
                // Colour is never the ONLY signal: the destructive button
                // also says what it does ("Reset", "Discard it") and sits
                // furthest from Cancel, so nothing here depends on being able
                // to tell red from grey.
                let apart = crate::recorder_panel::delta_e(danger.bg, t.button_bg);
                if apart < 25.0 {
                    bad.push(format!(
                        "{name} {mode}: the destructive button is only ΔE {apart:.1} from \
                         an ordinary one"
                    ));
                }
                warns.insert((danger.bg.r(), danger.bg.g(), danger.bg.b()));
            }
        }
        assert!(bad.is_empty(), "{} unreadable confirmations: {bad:#?}", bad.len());
        assert!(
            warns.len() > 1,
            "every theme's warning colour came out the same; the role is not being read"
        );
    }

    /// A dialog must centre on the WINDOW, and must refuse to guess when the
    /// window's position is not known yet. The welcome note opens on the first
    /// frame, before the platform has reported the main window's rect; handing
    /// it a rect at (0, 0) made it centre on the corner of the screen, which
    /// looks like a placement bug and is really a "we did not know" bug.

    /// **The Welcome card is the receipt for your settings.**
    ///
    /// The owner's rule, and it needs no special case to hold. A FIRST
    /// install has no settings file, so it comes up on the Default theme,
    /// whose strip face IS the shipped bone — the card is black on ivory and
    /// matches the default right-click menus exactly, which is what a first
    /// launch should look like.
    ///
    /// An update or a reinstall keeps `settings.json` — it lives beside the
    /// config, not inside the bundle — so the theme survives, and the card
    /// comes up wearing it. Somebody who has themed their desk can SEE their
    /// settings were kept before they have clicked anything.
    #[test]
    fn the_welcome_card_wears_the_theme_and_a_fresh_install_is_the_default() {
        // First install: nothing carried over.
        let fresh = crate::settings::Settings::default();
        assert!(fresh.show_welcome, "a first launch does not greet anybody");
        let face = fresh.fx_strip_color.to_color32();
        let card = theme(face);
        assert_eq!(
            (card.bg.r(), card.bg.g(), card.bg.b()),
            (0xE3, 0xD9, 0xBD),
            "a first-install Welcome card is not the shipped bone"
        );
        assert!(
            !crate::recorder_panel::wants_light_ink(card.bg),
            "a first-install Welcome card is not black on ivory"
        );
        // And it is the SAME surface the default right-click menu is, which
        // is the whole of "matches the default theme's context menus".
        let menu = crate::menu::colors(false, face);
        assert_eq!(card.bg, menu.bg, "the card and the menu disagree on the default");
        assert_eq!(card.text, menu.text, "the card and the menu disagree on the ink");

        // An update: the settings file survived, so the theme did.
        let mut kept = crate::settings::Settings::default();
        let mut themed = 0;
        for t in crate::theme::factory().into_iter().skip(1) {
            t.apply(&mut kept);
            let card = theme(kept.fx_strip_color.to_color32());
            if card.bg != Color32::from_rgb(0xE3, 0xD9, 0xBD) {
                themed += 1;
            }
            // Whatever it wears, it has to be readable — the card is a
            // paragraph of text and nothing else.
            let r = crate::recorder_panel::contrast_ratio(card.text, card.bg);
            assert!(r >= 4.5, "{}: the Welcome card reads at {r:.2}:1", t.name);
        }
        assert!(
            themed > 8,
            "only {themed} of the themes changed the Welcome card at all"
        );
    }
    #[test]
    fn a_dialog_centres_on_the_window_or_declines_to_place_itself() {
        let size = Vec2::new(400.0, 300.0);
        let monitor = Some(Vec2::new(2560.0, 1440.0));

        // **Parent unknown, monitor known: the middle of the screen.**
        //
        // This is the welcome card, and it is the only dialog that opens before
        // the main window has a position to centre on. It used to hand back
        // `None` and let the platform decide, which meant the first thing
        // anybody saw was a window placed wherever.
        let p = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor,
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        }
        .position_for(size)
        .expect("a known monitor is enough to place a dialog");
        let centre = p + size * 0.5;
        assert!((centre.x - 1280.0).abs() < 0.5, "off-centre in x: {centre:?}");
        assert!((centre.y - 720.0).abs() < 0.5, "off-centre in y: {centre:?}");

        // Neither: nothing to centre on, so the platform really does decide.
        assert_eq!(
            Placement {
                at: None,
            native_panel_up: false,
            raise: false,
                parent: None,
                monitor: None,
                caps: crate::host::Caps::DESKTOP,
                chrome: None,
            }
            .position_for(size),
            None
        );

        // Parent known: dead centre of the parent.
        let parent = Rect::from_min_size(Pos2::new(600.0, 400.0), Vec2::new(1300.0, 200.0));
        let p = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(parent),
            monitor,
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        }
        .position_for(size)
        .expect("a known parent must yield a position");
        let dialog_centre = p + size * 0.5;
        assert!(
            (dialog_centre.x - parent.center().x).abs() < 0.5,
            "off-centre in x"
        );
        assert!(
            (dialog_centre.y - parent.center().y).abs() < 0.5,
            "off-centre in y"
        );

        // A window near an edge still puts the whole dialog on the monitor —
        // and the dialog really does go to the edge, because the window it
        // belongs to is there. This is the case `believable_parent` must not
        // reject: the parent's middle is still on screen.
        let edge = Rect::from_min_size(Pos2::new(1200.0, 1000.0), Vec2::new(1300.0, 400.0));
        let p = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(edge),
            monitor,
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        }
        .position_for(size)
        .unwrap();
        assert!(p.x >= 0.0 && p.y >= 0.0, "pushed off the top-left: {p:?}");
        assert!(p.x + size.x <= 2560.0 + 0.5, "hangs off the right: {p:?}");
        assert!(p.y + size.y <= 1440.0 + 0.5, "hangs off the bottom: {p:?}");
        assert!(
            (p.x + size.x * 0.5 - edge.center().x).abs() < 0.5,
            "a parent on screen was not centred on: {p:?}"
        );

        // **A window on the display NEXT DOOR is a window.** Its rect
        // comes in virtual-desktop coordinates, where the second monitor
        // starts past the first screen's width, so its centre is nowhere
        // near the box at the origin — and the first draft of
        // `believable_parent` compared it against exactly that box and
        // called it a lie, sending every dialog it owned to the middle of
        // the WRONG screen. It is centred on, like any other parent, and it
        // lands on the screen it belongs to. See `Placement::screen`.
        let mon = Vec2::new(2560.0, 1440.0);
        let corner = (mon - size).to_pos2();
        let next_door =
            Rect::from_min_size(Pos2::new(2600.0, 200.0), Vec2::new(1300.0, 800.0));
        // What the old clamp did with it, which is the fault written out:
        // it pinned the dialog inside the box at the ORIGIN, which is the
        // primary screen — a dialog on a different display from the window
        // it belongs to, every time.
        let onto_primary =
            crate::settings::clamp_to_monitor(next_door.center() - size * 0.5, size, monitor);
        assert!(
            Rect::from_min_size(Pos2::ZERO, mon).contains(onto_primary),
            "this parent is no longer dragged onto the primary, so the test proves nothing"
        );
        let beside = |parent: Rect, at: Option<Pos2>| {
            Placement {
                at,
                native_panel_up: false,
                raise: false,
                parent: Some(parent),
                monitor,
                caps: crate::host::Caps::DESKTOP,
                chrome: None,
            }
            .position_for(size)
            .unwrap()
        };
        let p = beside(next_door, None);
        assert_ne!(
            p, onto_primary,
            "a window on the next screen is still dragged onto the primary"
        );
        assert!(
            (p.x + size.x * 0.5 - next_door.center().x).abs() < 0.5
                && (p.y + size.y * 0.5 - next_door.center().y).abs() < 0.5,
            "a window on the next screen was not centred on: {p:?}"
        );

        // **A rect no desk could hold is still a lie**, and still falls back
        // to the middle of the screen we do know about. This is the hide,
        // the Space change and the display change — a host reporting an
        // origin from nowhere — and it is the case the believability test
        // exists for.
        let nowhere =
            Rect::from_min_size(Pos2::new(40_000.0, 30_000.0), Vec2::new(1300.0, 200.0));
        // And this one really does produce the owner's corner: the clamp,
        // handed a centre far past the screen, answers with `monitor - size`
        // exactly rather than nudging towards it.
        assert_eq!(
            crate::settings::clamp_to_monitor(nowhere.center() - size * 0.5, size, monitor),
            corner,
            "this parent no longer produces the corner, so the test proves nothing"
        );
        let p = beside(nowhere, None);
        assert_ne!(p, corner, "an off-desk parent still corners the dialog");
        let centre = p + size * 0.5;
        assert!(
            (centre.x - mon.x * 0.5).abs() < 0.5 && (centre.y - mon.y * 0.5).abs() < 0.5,
            "an unbelievable parent did not fall back to the middle of the monitor: {p:?}"
        );

        // The anchored road takes both answers: a point measured against a
        // window nobody can see is not a point to hang a menu off, and a
        // point measured against one on the next screen is.
        assert_ne!(
            beside(nowhere, Some(Pos2::new(40.0, 30.0))),
            corner,
            "an off-desk anchor still corners the menu"
        );
        let hung = beside(next_door, Some(Pos2::new(40.0, 30.0)));
        assert!(
            (hung.x - (next_door.left() + 46.0)).abs() < 0.5
                && (hung.y - (next_door.top() + 36.0)).abs() < 0.5,
            "a menu on the next screen did not hang off the point it was given: {hung:?}"
        );
    }

    /// **The screen a dialog is clamped into is the one its parent is on.**
    ///
    /// `monitor` is a size with no origin and the parent is in
    /// virtual-desktop coordinates, so the two only line up on a
    /// single-display desk. `Placement::screen` is what reconciles them, and
    /// this walks a parent around a desk that has a display in each
    /// direction — including the negative ones, which is a monitor hung
    /// above or placed to the left of the primary.
    #[test]
    fn a_dialog_is_clamped_into_the_display_its_parent_is_on() {
        let m = Vec2::new(1920.0, 1080.0);
        let size = Vec2::new(400.0, 300.0);
        let at = |parent: Rect| Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(parent),
            monitor: Some(m),
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        // A window on each of five displays: the primary, and one out in
        // each direction. Each window is well inside its own screen.
        for (what, origin) in [
            ("primary", Vec2::ZERO),
            ("right", Vec2::new(1920.0, 0.0)),
            ("left", Vec2::new(-1920.0, 0.0)),
            ("above", Vec2::new(0.0, -1080.0)),
            ("below", Vec2::new(0.0, 1080.0)),
        ] {
            let screen = Rect::from_min_size(origin.to_pos2(), m);
            let parent = Rect::from_min_size(screen.min + Vec2::new(200.0, 150.0), m * 0.6);
            let p = at(parent).position_for(size).expect("nowhere to put it");
            assert!(
                screen.expand(0.5).contains_rect(Rect::from_min_size(p, size)),
                "the {what} screen's dialog landed at {p:?}, off {screen:?}"
            );
            assert!(
                (p + size * 0.5 - parent.center()).length() < 0.5,
                "the {what} screen's dialog was not centred on its window: {p:?}"
            );
        }
        // And a window shoved against the far edge of the display to the
        // right stays fully ON that display rather than hanging into the gap
        // beyond it — the clamp still clamps, it just clamps to the right
        // box now.
        let screen = Rect::from_min_size(Pos2::new(1920.0, 0.0), m);
        let edge = Rect::from_min_size(Pos2::new(3600.0, 900.0), Vec2::new(230.0, 170.0));
        let p = at(edge).position_for(size).expect("nowhere to put it");
        assert!(
            screen.expand(0.5).contains_rect(Rect::from_min_size(p, size)),
            "a dialog on the right-hand screen's edge hung off it: {p:?}"
        );
    }

    // ── the custom tuning editor ────────────────────────────────────────────

    /// Rows built from what somebody would actually type.
    fn rows(texts: &[&str]) -> Vec<TuningString> {
        texts
            .iter()
            .map(|s| TuningString {
                text: (*s).to_owned(),
                pitch: parse_pitch(s).unwrap_or(0),
            })
            .collect()
    }

    fn pitches(rows: &[TuningString]) -> Vec<u8> {
        rows.iter().map(|r| r.pitch).collect()
    }

    fn preset(name: &str) -> Vec<u8> {
        Tuning::by_name(name)
            .unwrap_or_else(|| panic!("{name} is not in TUNINGS any more"))
            .open
            .to_vec()
    }

    /// A note name has to survive the trip out to the field and back, in the
    /// spelling the app prefers — otherwise the editor would rewrite a string
    /// the user typed into a different one the moment they touched the spinner.
    #[test]
    fn note_names_and_midi_numbers_round_trip_in_the_spelling_the_app_prefers() {
        for prefer_flats in [true, false] {
            for pitch in 0..=127u8 {
                let name = pitch_name(pitch, prefer_flats);
                assert_eq!(
                    parse_pitch(&name),
                    Ok(pitch),
                    "{name} did not come back as {pitch}"
                );
            }
        }
        // The app prefers flats (`ChordDetector::new`, `Settings::default`),
        // and a string has to be spelled the way the piano above it spells the
        // same note. This is the assertion that would fail if the editor grew
        // its own note table.
        assert_eq!(pitch_name(61, true), "Db4");
        assert_eq!(pitch_name(61, false), "C#4");
        // Both spellings are READ whichever is preferred, and case is not a
        // spelling: nobody copying a tuning off a forum post holds shift for it.
        for text in ["Db4", "C#4", "db4", "c#4", " Db4 ", "61"] {
            assert_eq!(parse_pitch(text), Ok(61), "{text:?} was not read as Db4");
        }
        // Standard must read back as the E A D G B E written on every guitar
        // chart, in the octaves a guitarist would write. An off-by-one octave
        // is invisible until somebody who plays reads it.
        let standard: Vec<String> = Tuning::standard()
            .open
            .iter()
            .map(|&p| pitch_name(p, true))
            .collect();
        assert_eq!(standard, ["E2", "A2", "D3", "G3", "B3", "E4"]);
        // Both ends of MIDI, where the octave arithmetic goes negative.
        assert_eq!(pitch_name(0, true), "C-1");
        assert_eq!(parse_pitch("C-1"), Ok(0));
        assert_eq!(parse_pitch("G9"), Ok(127));

        // A parser that can only say "no" makes the user guess which of six
        // things was wrong, so each refusal names its own.
        for (bad, must_mention) in [
            ("", "note name"),
            ("   ", "note name"),
            ("H2", "A to G"),
            ("E", "octave"),
            ("E#b2", "accidental"),
            ("Ex", "octave"),
            ("C-3", "MIDI"),
            ("300", "0 to 127"),
        ] {
            let hint = parse_pitch(bad).expect_err(&format!("{bad:?} was read as a pitch"));
            assert!(
                hint.contains(must_mention),
                "{bad:?} was refused with {hint:?}, which never mentions {must_mention:?}"
            );
        }
    }

    /// Every way a tuning can be refused has to arrive as a REASON. The one
    /// that matters is `NotAscending`: it is not a style rule, it is the
    /// voicing solver's search invariant, and "invalid" would be hiding the
    /// only interesting thing the editor knows.
    #[test]
    fn every_reason_a_tuning_can_be_refused_reaches_the_user_as_a_reason() {
        // Listed one by one on purpose: a variant added to `TuningError` later
        // must be given words here too.
        for e in [
            TuningError::StringCount(0),
            TuningError::StringCount(MAX_STRINGS + 1),
            TuningError::NotAscending {
                string: 2,
                previous: 50,
                found: 45,
            },
            TuningError::PitchRange(120),
            TuningError::EmptyName,
        ] {
            let msg = Problem::Refused(e.clone()).message(true);
            assert!(msg.len() > 20, "{e:?} was answered with {msg:?}");
            assert!(
                !msg.to_lowercase().contains("invalid"),
                "{e:?} was answered with \"invalid\", which is not a reason: {msg:?}"
            );
        }

        let ascending = Problem::Refused(TuningError::NotAscending {
            string: 2,
            previous: 50,
            found: 45,
        })
        .message(true);
        // Note names, not MIDI numbers: the user is looking at a field that
        // says A2. This is the whole reason core's `Display` is not reused for
        // this variant.
        assert!(
            ascending.contains("A2") && ascending.contains("D3"),
            "not in the notation the fields use: {ascending}"
        );
        assert!(
            !ascending.contains("45") && !ascending.contains("50"),
            "raw MIDI numbers leaked into a note-name editor: {ascending}"
        );
        // 1-based from the LOW end, the way `TuningError` counts, so the
        // message and the row numbers on screen point at the same string.
        assert!(
            ascending.contains("String 3") && ascending.contains("string 2"),
            "the wrong string is accused: {ascending}"
        );
        assert!(
            ascending.contains("solver") && ascending.contains("shapes"),
            "the reason is missing, so this reads as a taste rule: {ascending}"
        );

        let too_high = Problem::Refused(TuningError::PitchRange(120)).message(true);
        assert!(too_high.contains("C9"), "which pitch? {too_high}");
        assert!(
            too_high.contains(&pitch_name(highest_open_pitch(), true)),
            "refused without saying what would be accepted: {too_high}"
        );

        let unreadable = Problem::Unreadable {
            string: 3,
            text: "Q9".to_owned(),
            hint: parse_pitch("Q9").expect_err("Q is not a note"),
        }
        .message(true);
        assert!(
            unreadable.contains("String 3") && unreadable.contains("Q9"),
            "{unreadable}"
        );

        let clash = Problem::NameClash {
            preset: "Standard".to_owned(),
        }
        .message(true);
        assert!(clash.contains("Standard"), "{clash}");
        assert!(
            clash.contains("name"),
            "the fix is to rename it, and the message has to say so: {clash}"
        );
    }

    /// A seventh string is a low B added BELOW the low E, not a string stapled
    /// above the top one, and the shipped presets settle it rather than taste:
    /// this asserts the result is `TUNINGS`'s own 7- and 8-string tunings, note
    /// for note.
    #[test]
    fn adding_a_string_grows_the_low_end_like_a_seven_string() {
        let mut r: Vec<TuningString> = Tuning::standard()
            .open
            .iter()
            .map(|&p| TuningString::new(p, true))
            .collect();

        add_low_string(&mut r, true);
        assert_eq!(
            pitches(&r),
            preset("7-String"),
            "the added string is not the low B a 7-string guitar actually has"
        );
        // And it arrives spelled, not as an empty box waiting to be filled in.
        assert_eq!(r[0].text, "B1");
        add_low_string(&mut r, true);
        assert_eq!(pitches(&r), preset("8-String"));

        remove_low_string(&mut r);
        remove_low_string(&mut r);
        assert_eq!(
            pitches(&r),
            preset("Standard"),
            "Add then Remove has to be a round trip, which it only is if both \
             touch the same end"
        );

        // The presets shed from that end too, which is the other half of the
        // argument for it.
        let mut bass: Vec<TuningString> = preset("Bass (5)")
            .iter()
            .map(|&p| TuningString::new(p, true))
            .collect();
        remove_low_string(&mut bass);
        assert_eq!(pitches(&bass), preset("Bass (4)"));

        // The bounds hold even when a caller ignores the disabled buttons, and
        // nothing added below can ever sound ABOVE what it was added under —
        // an "add a string" button that broke the solver's invariant would be
        // the worst possible source of it.
        let mut wide = r.clone();
        for _ in 0..20 {
            add_low_string(&mut wide, true);
        }
        assert_eq!(wide.len(), MAX_STRINGS);
        for pair in wide.windows(2) {
            assert!(
                pair[0].pitch <= pair[1].pitch,
                "adding strings broke the ascending order: {:?}",
                pitches(&wide)
            );
        }
        let mut one = vec![TuningString::new(40, true)];
        remove_low_string(&mut one);
        assert_eq!(one.len(), 1, "a tuning with no strings is not a tuning");
    }

    /// Confirm is one function, and it is the same value that greys the button
    /// out and that builds the `Tuning` — so nothing invalid can reach the app
    /// even if the button were somehow clicked.
    #[test]
    fn confirm_refuses_everything_the_solver_could_not_survive() {
        let good = confirm("My Tuning", &rows(&["E2", "A2", "D3", "G3", "B3", "E4"]))
            .expect("standard tuning under a new name is the plainest custom tuning there is");
        assert_eq!(good.name, "My Tuning");
        assert_eq!(good.open.as_ref(), Tuning::standard().open.as_ref());
        // Whitespace is the user's, not the tuning's.
        assert_eq!(
            confirm("  Padded  ", &rows(&["E2"]))
                .expect("a padded name is a name")
                .name,
            "Padded"
        );
        // A copy of a preset under its own name IS that preset, so it is
        // harmless and must not be refused.
        assert!(
            confirm("Standard", &rows(&["E2", "A2", "D3", "G3", "B3", "E4"])).is_ok(),
            "an unedited copy of a preset was refused"
        );

        let many: Vec<&str> = vec!["E2"; MAX_STRINGS + 1];
        for (why, name, texts) in [
            ("a half-typed pitch", "My Tuning", vec!["E2", "A2", "D3", "E"]),
            ("a name that is only whitespace", "   ", vec!["E2", "A2"]),
            (
                "a preset's name over different pitches",
                "Drop D",
                vec!["E2", "A2"],
            ),
            (
                "a preset's name in lower case, since lookup ignores it",
                "drop d",
                vec!["E2", "A2"],
            ),
            ("strings that do not ascend", "My Tuning", vec!["E2", "D2"]),
            (
                "an open string with no room to fret above it",
                "My Tuning",
                vec!["C9"],
            ),
            ("no strings at all", "My Tuning", vec![]),
            ("more strings than the solver will enumerate", "My Tuning", many),
        ] {
            let problem = confirm(name, &rows(&texts))
                .err()
                .unwrap_or_else(|| panic!("{why} was accepted"));
            let msg = problem.message(true);
            assert!(msg.len() > 20, "{why} was refused with {msg:?}");
            assert!(
                !msg.to_lowercase().contains("invalid"),
                "{why}: \"invalid\" is not a reason: {msg}"
            );
        }
    }

    /// The message under the strings gets a fixed band of the dialog so that
    /// the buttons do not walk up and down under the cursor as you type. That
    /// only works if the LONGEST thing it can say fits inside the band — and
    /// "it should come to about four lines" is exactly the reasoning that once
    /// pushed the D-UI-9 result box's OK button off its bottom edge. So this
    /// measures every message the editor can produce, in the face it uses, at
    /// the size it uses, wrapped to the width it has.
    #[test]
    fn the_longest_refusal_fits_the_space_reserved_for_it() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        let mut messages = vec![
            Problem::NameClash {
                preset: "Half Step Down".to_owned(),
            }
            .message(true),
            Problem::Unreadable {
                string: 12,
                text: "not a note at all".to_owned(),
                hint: parse_pitch("Q").expect_err("Q is not a note"),
            }
            .message(true),
        ];
        for e in [
            TuningError::StringCount(MAX_STRINGS + 1),
            TuningError::NotAscending {
                string: 11,
                previous: 103,
                found: 0,
            },
            TuningError::PitchRange(127),
            TuningError::EmptyName,
        ] {
            messages.push(Problem::Refused(e).message(true));
        }
        // The band holds the confirmation as well as the complaints, and the
        // longest of those is a full twelve strings spelled out.
        let widest = Tuning::custom("wide", (0..MAX_STRINGS as u8).map(|i| i * 8).collect())
            .expect("twelve ascending strings inside the range is a legal tuning");
        messages.push(format!(
            "{} strings: {}",
            widest.strings(),
            widest
                .open
                .iter()
                .map(|&p| pitch_name(p, false))
                .collect::<Vec<_>>()
                .join(" ")
        ));

        // Exactly the width the label is given: the dialog's, less the
        // `Frame` margin on each side.
        let content_w = TUNING_DIALOG_W - 2.0 * f32::from(TUNING_MARGIN);
        let mut worst = (0.0_f32, String::new());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(TUNING_DIALOG_W, TUNING_DIALOG_H),
                )),
                ..Default::default()
            },
            |ctx| {
                crate::shell::viewport_ui(ctx, |ui| {
                    for m in &messages {
                        let galley = ui.painter().layout(
                            m.clone(),
                            FontId::new(TUNING_FOOTER_PT, fonts::courier_bold()),
                            Color32::WHITE,
                            content_w,
                        );
                        if galley.size().y > worst.0 {
                            worst = (galley.size().y, m.clone());
                        }
                    }
                });
            },
        );
        assert!(
            worst.0 <= TUNING_FOOTER_H,
            "{TUNING_FOOTER_H}pt is reserved for the message and the longest one \
             needs {}pt, which would push the buttons off the bottom edge: {:?}",
            worst.0,
            worst.1
        );
    }

    /// The editor has to draw where there is no window manager at all: in a
    /// plugin it is an `Area` in the DAW's canvas, and `show` is the only
    /// funnel that knows the difference. This runs a whole frame of it inline —
    /// text fields, a wrapped row of preset buttons, a scroll area and twelve
    /// spinners sharing one `Area` id — and asserts the editor does nothing on
    /// its own until it is confirmed.
    #[test]
    fn the_editor_draws_inline_in_a_plugin_and_says_nothing_until_it_is_confirmed() {
        let ctx = egui::Context::default();
        // The dialogs name Courier Prime explicitly and epaint PANICS on a
        // font family that is not bound, so the fonts have to be installed
        // exactly as the app installs them.
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        let mut dialog = Some(Dialog::custom_tuning(&Tuning::standard(), true));
        let placement = Placement {
at: None,
                        native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let mut action = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 400.0))),
                ..Default::default()
            },
            |ctx| {
                action = show(ctx, &mut dialog, Color32::from_rgb(0xE3, 0xD9, 0xBD), placement);
            },
        );
        assert!(action.is_none(), "the editor acted without being asked to");

        match dialog {
            Some(Dialog::CustomTuning { name, strings, .. }) => {
                assert_eq!(strings.len(), 6, "it did not open on the live tuning");
                // Opened on a name the app will NOT resolve back to the preset,
                // which is the trap `DialogAction::SetCustomTuning` describes.
                assert_ne!(name, "Standard");
                assert!(
                    confirm(&name, &strings).is_ok(),
                    "the editor opened on a tuning it would refuse to accept: {}",
                    confirm(&name, &strings)
                        .err()
                        .map(|p| p.message(true))
                        .unwrap_or_default()
                );
            }
            _ => panic!("the editor closed itself, or changed into another dialog"),
        }
    }

    // ── the export dialog ───────────────────────────────────────────────────

    use crate::recorder::{Composite, SpecProblem};

    /// The four specs the validator has an opinion about, plus the one it does
    /// not. Built by hand rather than through `Dialog::export`, which clamps.
    fn export_specs() -> [ExportSpec; 4] {
        [
            ExportSpec::default(),
            ExportSpec {
                audio: false,
                midi: false,
                ..Default::default()
            },
            ExportSpec {
                video: VideoMode::Composite,
                composite: Composite {
                    camera: false,
                    display: false,
                    ..Default::default()
                },
                ..Default::default()
            },
            ExportSpec {
                video: VideoMode::Composite,
                composite: Composite {
                    camera: false,
                    display: true,
                    shows: DisplayShows {
                        piano: false,
                        chord: false,
                        fretboard: false,
                        theory: false,
                    },
                    ..Default::default()
                },
                ..Default::default()
            },
        ]
    }

    /// One function greys the button and builds the value, so the two cannot
    /// drift apart — and the words shown are the recorder's own, because a
    /// second wording here would be a second answer to the same question.
    #[test]
    fn the_export_button_is_greyed_exactly_when_the_spec_has_a_problem() {
        for s in export_specs() {
            let built = export_ready(&s, "120");
            assert_eq!(
                built.is_err(),
                s.problem().is_some(),
                "the button and the validator disagree about {s:?}"
            );
            match (s.problem(), built) {
                (Some(p), Err(msg)) => assert_eq!(msg, p.message(), "invented its own words"),
                (None, Ok(out)) => assert_eq!(out, s, "an accepted spec was altered on the way"),
                (p, b) => panic!("{p:?} against {b:?}"),
            }
        }
        // Audio and MIDI default ON and cannot both be off: the owner's brief
        // is that a take produces the audio, the MIDI and the video, and a
        // directory with none of the first two is not a take. Refused, rather
        // than an empty folder appearing on the user's disk.
        let none = ExportSpec {
            audio: false,
            midi: false,
            ..Default::default()
        };
        assert!(ExportSpec::default().audio && ExportSpec::default().midi);
        assert_eq!(
            export_ready(&none, "120").unwrap_err(),
            SpecProblem::NothingToWrite.message()
        );
    }

    /// A finished take can never gain the camera. Those frames are composited
    /// and encoded as the take runs and nothing keeps them (§0: raw retention
    /// is 112 GB per take), and there is no decoder anywhere in this design to
    /// read the finished file back. Offering the tick anyway would be offering
    /// an option that cannot work, which is worse than not offering it.
    #[test]
    fn a_take_recorded_without_a_camera_can_never_gain_one() {
        let can = rederivable(true, false);
        assert!(!can.camera, "the camera controls were left live");
        assert!(!can.layout, "a layout for a camera layer that cannot exist");
        assert!(
            can.tempo && can.display,
            "the two things that ARE re-derivable were greyed with them"
        );
        let why = can.why.expect("greyed with no reason given");
        assert!(
            why.to_lowercase().contains("camera"),
            "the reason never mentions the camera: {why}"
        );

        // And the seed is clamped, so the dialog cannot even OPEN promising a
        // camera video: the spec comes from the settings file or the last take
        // and neither of them knows what this take caught.
        let clamped = can.clamp(ExportSpec {
            video: VideoMode::Both,
            ..Default::default()
        });
        assert!(!clamped.composite.camera);
        assert_eq!(
            clamped.video,
            VideoMode::Composite,
            "a display-only video IS re-derivable - replayed from the recorded .mid"
        );
        assert!(clamped.is_valid());

        // With the display out of it too there is no video left to make, and
        // an empty composite is not a video.
        let nothing_left = can.clamp(ExportSpec {
            video: VideoMode::Composite,
            composite: Composite {
                camera: true,
                display: false,
                ..Default::default()
            },
            ..Default::default()
        });
        assert_eq!(nothing_left.video, VideoMode::None);
        assert!(
            nothing_left.is_valid(),
            "clamping produced a spec the dialog would then refuse: {nothing_left:?}"
        );
    }

    /// The camera half is settled even when there WAS a camera: those frames
    /// were composited in that arrangement as the take ran, so "Camera full
    /// frame, display overlaid" can never become "Side by side".
    #[test]
    fn a_finished_take_cannot_be_given_a_different_layout_even_when_it_had_a_camera() {
        let can = rederivable(true, true);
        assert!(!can.layout, "the layout was left live after the take");
        assert!(!can.camera);
        assert!(can.tempo && can.display);
        let why = can.why.expect("greyed with no reason given");
        assert!(
            why.to_lowercase().contains("layout"),
            "the layout is greyed and nothing says why: {why}"
        );
        // A take that HAD a camera and one that did not are greyed for
        // different reasons, and they are worth saying differently.
        assert_ne!(can.why, rederivable(true, false).why);

        // Before a take nothing has happened yet, so nothing is fixed and
        // there is nothing to explain.
        let before = rederivable(false, false);
        assert_eq!(before, Rederivable::EVERYTHING);
        assert!(before.why.is_none());
        let untouched = ExportSpec {
            video: VideoMode::Both,
            ..Default::default()
        };
        assert_eq!(before.clamp(untouched), untouched);
    }

    /// The field holds TEXT for exactly this reason: a partially-typed number
    /// is not a number, and "9" on the way to "92" must not be rounded up to
    /// `MIN_BPM` under the cursor. And a tempo that is finished and out of
    /// range is REFUSED, not quietly clamped — a field that turned a typed 400
    /// into 300 would be a field lying to somebody who was not watching.
    #[test]
    fn a_half_typed_tempo_never_reaches_the_spec_and_an_impossible_one_is_refused_not_clamped() {
        let spec = ExportSpec {
            tempo_bpm: 120.0,
            ..Default::default()
        };
        for half in ["9", "", " ", "-", "1.", "9e"] {
            assert!(
                export_ready(&spec, half).is_err(),
                "{half:?} was accepted as a tempo"
            );
            assert_eq!(spec.tempo_bpm, 120.0, "{half:?} reached the stored spec");
        }
        for bad in ["19", "19.9", "301", "0", "-40", "1e9", "NaN", "inf"] {
            let e = export_ready(&spec, bad).expect_err("{bad:?} was accepted");
            assert!(e.len() > 20, "{bad:?} was refused with {e:?}");
        }
        // In range is taken exactly, at both ends and with a fraction.
        for (text, want) in [
            ("120", 120.0),
            ("92.5", 92.5),
            (" 88 ", 88.0),
            ("20", MIN_BPM),
            ("300", MAX_BPM),
        ] {
            assert_eq!(
                export_ready(&spec, text).expect("a legal tempo").tempo_bpm,
                want,
                "{text:?}"
            );
        }
        // With no MIDI there is no file to write a tempo mark into, so a stale
        // value in a greyed field must not be able to grey Export out as well
        // — that is a dead end with nothing on screen the user can fix.
        let no_midi = ExportSpec {
            midi: false,
            ..spec
        };
        assert_eq!(
            export_ready(&no_midi, "not a tempo at all")
                .expect("a greyed field refused an export it has no say in")
                .tempo_bpm,
            120.0
        );
    }

    /// Open it, change nothing, commit: the spec has to come back exactly as it
    /// went in. The seeded text is the only copy of the tempo while the dialog
    /// is open, so a constructor that seeded it badly would silently rewrite
    /// the value on the way out.
    #[test]
    fn the_export_dialog_opens_on_the_spec_it_was_given_and_commits_it_unchanged() {
        let spec = ExportSpec {
            tempo_bpm: 92.5,
            ..Default::default()
        };
        match Dialog::export(spec, false, true) {
            Dialog::Export {
                spec: seeded,
                tempo_text,
                remember,
                post_take,
                had_camera,
            } => {
                assert_eq!(seeded, spec, "the spec was altered on the way in");
                assert_eq!(tempo_text, "92.5");
                assert!(!remember, "it must not persist without being asked to");
                assert!(!post_take && had_camera);
                assert_eq!(
                    export_ready(&seeded, &tempo_text).expect("opened on a spec it refuses"),
                    spec,
                    "a round trip through the field changed the tempo"
                );
            }
            _ => panic!("Dialog::export built some other dialog"),
        }
        // A whole number reads as a whole number: a field opening on "120.0"
        // is one every user retypes.
        match Dialog::export(ExportSpec::default(), false, false) {
            Dialog::Export { tempo_text, .. } => assert_eq!(tempo_text, "120"),
            _ => panic!("Dialog::export built some other dialog"),
        }
    }

    /// The tick is the whole difference between "this take" and "every take",
    /// so it decides the action rather than being reported alongside it.
    #[test]
    fn the_remember_tick_is_what_chooses_the_action_that_persists() {
        let spec = ExportSpec {
            fps: 60,
            ..Default::default()
        };
        match export_action(spec, false) {
            DialogAction::SetExport(s) => assert_eq!(s, spec),
            _ => panic!("an unticked box persisted the spec anyway"),
        }
        match export_action(spec, true) {
            DialogAction::SetExportAndRemember(s) => assert_eq!(s, spec),
            _ => panic!("the tick did not reach the action"),
        }
    }

    /// The video panels seed from what is on screen, and the theory band is
    /// one panel however many diagrams are inside it.
    #[test]
    fn the_video_panel_ticks_seed_from_the_panels_that_are_on_screen() {
        let mut s = crate::settings::Settings::default();
        let shows = shows_from_settings(&s);
        assert!(shows.piano && shows.chord, "the panels that are never off");
        assert!(!shows.fretboard && !shows.theory);
        s.show_fretboard = true;
        // Only the Tonnetz. Seeding from one element alone would drop this
        // user's band out of their video without saying anything.
        s.set_theory_views(&crate::theory_panel::Views::of(vec![
            crate::theory_panel::View::Tonnetz,
        ]));
        let shows = shows_from_settings(&s);
        assert!(shows.fretboard && shows.theory);
    }

    /// Disk AND CPU, because "will it fill my disk" and "will it stutter while
    /// I play" are two questions and a byte count answers only one of them.
    #[test]
    fn the_estimate_names_a_rate_and_an_encoder_count() {
        let quiet = estimate_line(&ExportSpec {
            video: VideoMode::None,
            ..Default::default()
        });
        assert!(quiet.contains("MB a minute"), "{quiet}");
        assert!(
            quiet.contains("no video encoder"),
            "audio and MIDI run no encoder at all: {quiet}"
        );
        let busy = estimate_line(&ExportSpec {
            video: VideoMode::Both,
            ..Default::default()
        });
        assert!(busy.contains("3 video encoders"), "{busy}");
        assert!(
            encoder_note().contains("1080p30"),
            "the note never says what a stream costs: {}",
            encoder_note()
        );
    }

    /// The footer gets a fixed band so the buttons do not walk up and down
    /// under the cursor as you click. That only works if the longest thing it
    /// can say fits — and "it should come to about two lines" is exactly the
    /// reasoning that once pushed a dialog's OK button off its own bottom edge.
    /// **The Welcome card fits its own text.**
    ///
    /// It is the first thing anybody sees, and a card whose last line is cut
    /// off — or whose button has been pushed past the bottom edge — is a first
    /// impression nobody recovers from. Measured against the real strings at
    /// the real font, so adding a line fails here rather than in front of a
    /// user.
    #[test]
    fn the_welcome_card_fits_its_own_text() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        let font = FontId::new(12.0, fonts::courier_bold());
        let (mut widest, mut total_h) = (0.0_f32, 0.0_f32);
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                // Laid out through the PAINTER, which is the same route
                // `ui.label` takes — so this measures what will actually be
                // drawn rather than a reconstruction of it.
                // **Including the spacing egui puts BETWEEN the labels.**
                // Thirteen `ui.label` calls are twelve gaps, and leaving them
                // out is how this test passed while the card's last line and
                // its button were being cut off the bottom edge in front of
                // every first-time user.
                let gap = ui.spacing().item_spacing.y;
                for (i, line) in WELCOME_LINES.iter().enumerate() {
                    let galley = ui.painter().layout_no_wrap(
                        (*line).to_owned(),
                        font.clone(),
                        Color32::WHITE,
                    );
                    total_h += galley.rect.height().max(font.size);
                    if i + 1 < WELCOME_LINES.len() {
                        total_h += gap;
                    }
                    widest = widest.max(galley.rect.width());
                }
            });
        });

        // The chrome under the text: a checkbox, the spacing around it, and the
        // button row the layout reserves 30 points for.
        const CHROME_H: f32 = 8.0 + 22.0 + 30.0;
        // Two more gaps the layout adds under the text: one before the
        // checkbox's own row and one before the button row.
        const MARGIN: f32 = 14.0 * 2.0;
        let needed_h = total_h + CHROME_H + MARGIN;
        let needed_w = widest + MARGIN;
        assert!(
            needed_h + WELCOME_SLACK <= WELCOME_H,
            "the welcome card needs {needed_h:.0} points of measurable height plus \
             {WELCOME_SLACK:.0} of slack, and has {WELCOME_H:.0}"
        );
        assert!(
            needed_w <= WELCOME_W,
            "the longest welcome line needs {needed_w:.0} points of width and has {WELCOME_W:.0}"
        );
        // And the hint it exists for is actually in it.
        assert!(
            WELCOME_LINES.iter().any(|l| l.contains("'H'")),
            "the welcome card no longer tells anybody about the help key"
        );
    }

    /// The maker name is the one string on the audio panel that comes from
    /// OUTSIDE, so the clip that bounds it is worth its own test — including
    /// the accented case, which a byte slice would panic on.
    #[test]
    fn a_makers_name_is_cut_to_the_panels_budget_and_never_mid_character() {
        assert_eq!(clipped("Focusrite", 46), "Focusrite");
        // Exactly at the budget is not cut.
        let exact = "M".repeat(46);
        assert_eq!(clipped(&exact, 46), exact);
        // One over is cut, and the result is the budget: 45 kept plus the
        // ellipsis, so the measured width still holds.
        let over = "M".repeat(47);
        let cut = clipped(&over, 46);
        assert_eq!(cut.chars().count(), 46);
        assert!(cut.ends_with('\u{2026}'));
        // Multi-byte, which is the case a byte slice gets wrong.
        let accented = "Rögue Amöeba Softwäre, Incörporated, and then some more";
        let cut = clipped(accented, 20);
        assert_eq!(cut.chars().count(), 20);
        assert!(accented.starts_with(cut.trim_end_matches('\u{2026}')));
    }

    /// **The Audio Status panel fits the longest line it can produce.**
    ///
    /// Its content is generated, not fixed — device names, rates, a warning —
    /// so the thing to measure is the widest line it could ever draw. A panel
    /// whose rate line runs under the buffer buttons is one nobody can read the
    /// number off, which is its entire job.
    ///
    /// **It only started measuring anything today.** Its `#[test]` had drifted
    /// up above the maker-name test's own doc comment, so that function
    /// carried two attributes — legal, and it simply ran twice — while this
    /// one was a plain function nothing called. Every doc in this file that
    /// says the width is measured rather than hoped for was hoping. The
    /// attribute is back where it belongs, and the budget it enforces turns
    /// out to have been kept all along. The trap leaves no trace of itself: a
    /// doubly-attributed test is not an error, and an orphaned function in a
    /// test module is not either.
    #[test]
    fn the_audio_status_card_fits_its_own_text() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        // The longest of each kind, including a device name longer than any
        // real one — interfaces have names like "Scarlett 18i20 USB" and a
        // Continuity camera's is longer still.
        let lines = [
            "Scarlett 18i20 USB (2nd Generation) Digital".to_owned(),
            format!("{} Hz   {} ch   {} frames  ({:.1} ms)", 192_000, 32, 2048, 10.7),
            "The two sides run at different rates.".to_owned(),
            "The instrument's audio will drift.".to_owned(),
            "Round trip, at least  21.3 ms".to_owned(),
            "buffers only; converters and the driver add more".to_owned(),
            "both streams reopen; not while a take is rolling".to_owned(),
            // Setup's own lines. The first is the longest string this panel
            // draws, which is why it is here: it was written to fit and that is
            // a claim, not a fact, until something measures it.
            "what the input offers; the output follows where it can".to_owned(),
            "18 inputs; record the one the piano is in".to_owned(),
            "for an interface with more than two inputs".to_owned(),
            "the device list changes with it".to_owned(),
            "each one ticked is its own row in the mic picker".to_owned(),
            // The short name's caption, and the hint inside its field. Both are
            // the panel's own text at the panel's own size, so both belong in
            // the measurement that decides how wide the panel is.
            "what the mixer calls this interface - \"x\" makes \"x - 3\"".to_owned(),
            "the interface's own name".to_owned(),
            // The output-path lines. The maker is the one string on this
            // panel that comes from OUTSIDE, so it is measured at exactly the
            // length the panel will clip it to — `MAKER_MAX` of the widest
            // character Courier has, which is every character, it being
            // monospaced.
            "the device reports 10.7 ms more, not counted above".to_owned(),
            "Output is a VIRTUAL device".to_owned(),
            "M".repeat(MAKER_MAX),
            "another app is between the mix and the hardware".to_owned(),
            "Output is an aggregate device".to_owned(),
            // The take report's block, which is the switch that came back
            // from the modal box: a heading, a checkbox and its caption.
            "REPORTS".to_owned(),
            "Say what each take produced".to_owned(),
            "a take with a PROBLEM says so either way".to_owned(),
        ];
        let mut widest = 0.0_f32;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                for (i, line) in lines.iter().enumerate() {
                    // The warning and the headings are bold; the readings are
                    // not. Measured at the size each is actually drawn.
                    let (size, family) = match i {
                        2 | 3 | 4 | 19 => (11.0, fonts::courier_bold()),
                        // The captions and the small print, at 10.
                        5..=13 => (10.0, fonts::courier()),
                        // The output-path block: its heading is bold at 11
                        // and everything under it is small print at 10.
                        15 => (11.0, fonts::courier_bold()),
                        14 | 16..=18 | 21 => (10.0, fonts::courier()),
                        _ => (11.0, fonts::courier()),
                    };
                    let g = ui.painter().layout_no_wrap(
                        line.clone(),
                        FontId::new(size, family),
                        Color32::WHITE,
                    );
                    // **A checkbox is wider than its words.** The tick box and
                    // the gap after it are drawn from the same budget as the
                    // label, so a measurement of the label alone would say a
                    // row fits that does not. Only row 20 is one.
                    let furniture = if i == 20 {
                        ui.spacing().icon_width + ui.spacing().icon_spacing
                    } else {
                        0.0
                    };
                    widest = widest.max(g.rect.width() + furniture);
                }
            });
        });

        const MARGIN: f32 = 14.0 * 2.0;
        let needed = widest + MARGIN;
        assert!(
            needed <= AUDIO_W,
            "the audio panel needs {needed:.0} points of width and has {AUDIO_W:.0}"
        );
    }

    #[test]
    fn the_longest_export_refusal_fits_the_space_reserved_for_it() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        // Every refusal the footer can hold, plus the fattest estimate line —
        // the two are painted together, so it is their SUM that has to fit.
        let mut refusals: Vec<String> = export_specs()
            .into_iter()
            .filter_map(|s| export_ready(&s, "120").err())
            .collect();
        for bad in ["", "not a tempo at all, not even close", "400"] {
            refusals.push(
                export_ready(&ExportSpec::default(), bad).expect_err("a bad tempo was accepted"),
            );
        }
        let estimate = estimate_line(&ExportSpec {
            video: VideoMode::Both,
            ..Default::default()
        });

        // Exactly the width the labels are given: the dialog's, less the
        // `Frame` margin on each side.
        let content_w = EXPORT_DIALOG_W - 2.0 * f32::from(EXPORT_MARGIN);
        let mut worst = (0.0_f32, String::new());
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(EXPORT_DIALOG_W, EXPORT_DIALOG_H),
                )),
                ..Default::default()
            },
            |ctx| {
                crate::shell::viewport_ui(ctx, |ui| {
                    let height = |m: &str| {
                        ui.painter()
                            .layout(
                                m.to_owned(),
                                FontId::new(EXPORT_FOOTER_PT, fonts::courier_bold()),
                                Color32::WHITE,
                                content_w,
                            )
                            .size()
                            .y
                    };
                    let base = height(&estimate);
                    for m in &refusals {
                        let total = base + height(m) + ui.spacing().item_spacing.y;
                        if total > worst.0 {
                            worst = (total, m.clone());
                        }
                    }
                });
            },
        );
        assert!(
            worst.0 <= EXPORT_FOOTER_H,
            "{EXPORT_FOOTER_H}pt is reserved for the footer and the estimate plus the \
             longest refusal needs {}pt, which would push Export off the bottom edge: {:?}",
            worst.0,
            worst.1
        );
    }

    /// Like the tuning editor, this dialog has to draw where there is no window
    /// manager at all — in a plugin it is an `Area` in the DAW's canvas, and
    /// `show` is the only funnel that knows the difference. This runs a whole
    /// frame of it inline (a text field, four radio groups, eleven checkboxes
    /// and a scroll area sharing one `Area` id) and asserts it does nothing on
    /// its own until it is confirmed.
    #[test]
    fn the_export_dialog_draws_inline_in_a_plugin_and_says_nothing_until_it_is_confirmed() {
        let ctx = egui::Context::default();
        // The dialogs name Courier Prime explicitly and epaint PANICS on a
        // font family that is not bound.
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        // The post-take case, which is the one with greyed controls in it.
        let mut dialog = Some(Dialog::export(ExportSpec::default(), true, false));
        let placement = Placement {
at: None,
                        native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let mut action = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 420.0))),
                ..Default::default()
            },
            |ctx| {
                action = show(ctx, &mut dialog, Color32::from_rgb(0xE3, 0xD9, 0xBD), placement);
            },
        );
        assert!(action.is_none(), "the dialog exported without being asked");

        match dialog {
            Some(Dialog::Export {
                spec, tempo_text, ..
            }) => assert!(
                export_ready(&spec, &tempo_text).is_ok(),
                "it opened on a spec it would refuse to export: {:?}",
                export_ready(&spec, &tempo_text).err()
            ),
            _ => panic!("the dialog closed itself, or changed into another one"),
        }
    }

    // ── the plugin picker ───────────────────────────────────────────────────

    fn entry(path: &str, name: &str, vendor: &str) -> PluginEntry {
        PluginEntry {
            path: path.to_owned(),
            name: name.to_owned(),
            vendor: vendor.to_owned(),
        }
    }

    /// A believable shelf. Two vendors are known because the app has loaded
    /// those bundles before; the other two are blank, which is the ordinary
    /// case and the whole point of the picker not scanning.
    fn installed() -> Vec<PluginEntry> {
        vec![
            entry(
                "/Library/Audio/Plug-Ins/VST3/Analog Lab V.vst3",
                "Analog Lab V",
                "Arturia",
            ),
            entry(
                "/Users/x/Library/Audio/Plug-Ins/VST3/Pianoteq 8.vst3",
                "Pianoteq 8",
                "Modartt",
            ),
            entry("/Library/Audio/Plug-Ins/VST3/Serum2.vst3", "Serum2", ""),
            entry("/Library/Audio/Plug-Ins/VST3/Vital.vst3", "Vital", ""),
        ]
    }

    /// Every string a frame actually painted.
    ///
    /// "The dialog says where VST3s live" is a claim about what is on screen and
    /// there is no other way to reach it: the help is a label, not a value. So
    /// the frame's shapes are walked and their galleys read.
    fn painted_text(output: &egui::FullOutput) -> String {
        fn walk(shape: &egui::epaint::Shape, out: &mut String) {
            match shape {
                egui::epaint::Shape::Text(t) => {
                    out.push_str(t.galley.text());
                    out.push('\n');
                }
                egui::epaint::Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = String::new();
        for c in &output.shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    /// The picker drawn inline, the way a plugin editor draws it: what ended up
    /// on the screen, and anything it asked the app to do.
    ///
    /// Three frames, not one. An `egui::Area` whose size is not known yet runs a
    /// SIZING PASS — it lays the whole dialog out and paints none of it
    /// (`Area::content_ui` builds that `Ui` with `.invisible()`), and areas
    /// fade in over the frames after. A test that ran one frame and read the
    /// shapes would find a blank screen and conclude the dialog draws nothing,
    /// which is the wrong answer to the right question.
    fn run_picker(dialog: &mut Option<Dialog>) -> (String, Option<DialogAction>) {
        let ctx = egui::Context::default();
        // The themed dialogs name Courier Prime explicitly and epaint PANICS on
        // a font family that is not bound.
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let placement = Placement {
at: None,
                        native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let mut action = None;
        let mut painted = String::new();
        for _ in 0..3 {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 520.0))),
                    ..Default::default()
                },
                |ctx| {
                    if let Some(a) = show(ctx, dialog, Color32::from_rgb(0xE3, 0xD9, 0xBD), placement) {
                        action = Some(a);
                    }
                },
            );
            painted = painted_text(&out);
        }
        (painted, action)
    }

    /// With 112 plugins installed the filter is the dialog, so it has to find
    /// things the way somebody actually types: any case, and by the maker as
    /// well as the product — "arturia" is how you ask for the eleven of them at
    /// once, and the vendor is not in the name.
    #[test]
    fn the_filter_matches_a_name_or_a_vendor_in_any_case_and_an_empty_one_shows_everything() {
        let plugins = installed();
        let names = |f: &str| {
            visible_plugins(&plugins, f)
                .into_iter()
                .map(|i| plugins[i].name.as_str())
                .collect::<Vec<_>>()
        };

        assert_eq!(names(""), ["Analog Lab V", "Pianoteq 8", "Serum2", "Vital"]);
        assert_eq!(names("   "), names(""), "whitespace is not a filter");
        assert_eq!(names("piano"), ["Pianoteq 8"]);
        assert_eq!(names("PIANO"), ["Pianoteq 8"], "the caps lock case");
        assert_eq!(names(" piano "), ["Pianoteq 8"], "a pasted name has spaces");
        assert_eq!(names("arturia"), ["Analog Lab V"], "not matched on vendor");
        assert_eq!(names("MODARTT"), ["Pianoteq 8"]);
        // A blank vendor must not turn into a wildcard. `contains("")` is true
        // of every string, so the empty needle is ruled out first and the two
        // vendorless rows answer for their names alone.
        assert_eq!(names("ital"), ["Vital"]);
        assert!(names("zzz").is_empty());
    }

    /// The bug this dialog is most likely to grow: a row is highlighted, the
    /// user types, the list becomes a different list, and OK commits either the
    /// row that scrolled out of sight or whatever now sits at that position.
    /// The committed value is an ENTRY, resolved against what is on screen.
    #[test]
    fn filtering_after_a_selection_never_commits_a_row_the_user_can_no_longer_see() {
        let plugins = installed();
        // Vital is highlighted; "an" leaves Analog Lab V and Pianoteq 8, and
        // Vital is not among them.
        let stale = plugin_choice(&plugins, Some(3), None, "an", &[]);
        assert_eq!(
            stale, None,
            "Load stayed live over a selection the filter had hidden: {stale:?}"
        );
        // And in particular it is not the filtered list's row 3, nor its row 0
        // — the two shapes the index bug takes.
        assert_ne!(stale, Some(PluginChoice::Load(&plugins[0])));

        // Narrowed to one, the user types four letters and presses Enter. That
        // is how a list of 112 is used, so the single match wins over a stale
        // highlight rather than greying the button out.
        assert_eq!(
            plugin_choice(&plugins, Some(3), None, "piano", &[]),
            Some(PluginChoice::Load(&plugins[1]))
        );
        // A selection the filter still SHOWS is untouched by any of this.
        assert_eq!(
            plugin_choice(&plugins, Some(3), None, "ital", &[]),
            Some(PluginChoice::Load(&plugins[3]))
        );
        // The list is rebuilt from disk on every open, so an index left over
        // from a bundle that has since been deleted selects nothing rather than
        // panicking.
        assert_eq!(plugin_choice(&plugins, Some(99), None, "", &[]), None);
    }

    /// **A collapsed folder hides its rows from Load too.** Select a row,
    /// click its vendor heading shut, and the highlight is gone from the
    /// screen — rule 1's filtered-list bug in a second hat, which the gate
    /// used to miss because only the FILTER was consulted.
    #[test]
    fn collapsing_a_folder_ungates_load_from_its_hidden_selection() {
        let plugins = installed();
        let folder = plugin_folder(&plugins[1]).to_owned();
        assert_eq!(
            plugin_choice(&plugins, Some(1), None, "", &[folder]),
            Some(PluginChoice::Load(&plugins[1])),
            "an open folder's row must stay loadable"
        );
        assert_eq!(
            plugin_choice(&plugins, Some(1), None, "", &[]),
            None,
            "Load stayed live over a selection its collapsed folder hid"
        );
        // Filtering forces the folders open, so the filter path is untouched.
        assert_eq!(
            plugin_choice(&plugins, Some(1), None, "piano", &[]),
            Some(PluginChoice::Load(&plugins[1]))
        );
    }

    /// The None row is a real choice — it is how you get the sound back off a
    /// plugin without quitting — so it carries a value of its own rather than
    /// being "cancel with extra steps".
    #[test]
    fn the_none_row_unloads_the_instrument_that_is_loaded() {
        let plugins = installed();
        let loaded = plugins[1].path.clone();
        let choice = plugin_choice(&plugins, None, Some(&loaded), "", &[])
            .expect("the None row is a choice, not an absence of one");
        assert_eq!(choice, PluginChoice::Unload);
        match choice.action() {
            DialogAction::LoadPlugin { path } => {
                assert_eq!(path, None, "the None row loaded something");
            }
            _ => panic!("the None row produced some other action"),
        }
        // Nothing loaded, nothing typed, nothing highlighted: Load is greyed,
        // because unloading nothing is not an action.
        assert_eq!(plugin_choice(&plugins, None, None, "", &[]), None);
        // One plugin installed and an empty filter also leaves "exactly one
        // match" — and if that rule fired here, the None row could never be
        // reached on a machine with a single instrument.
        let one = vec![plugins[1].clone()];
        assert_eq!(
            plugin_choice(&one, None, Some(&loaded), "", &[]),
            Some(PluginChoice::Unload)
        );
    }

    /// An empty list has an ordinary cause — the installer offered AU or VST2
    /// and the user took the default — and an empty box cannot be told apart
    /// from a picker that failed. So the dialog names the folders it looked in,
    /// and this reads what actually reached the screen.
    #[test]
    fn an_empty_plugin_list_says_where_vst3s_live_instead_of_showing_an_empty_box() {
        // The directories are the platform's, decided by the TARGET, so a
        // cross-built Windows binary still names the Windows folder.
        #[cfg(target_os = "macos")]
        assert!(
            vst3_locations().contains("~/Library/Audio/Plug-Ins/VST3")
                && vst3_locations().contains(" /Library/Audio/Plug-Ins/VST3"),
            "{}",
            vst3_locations()
        );
        #[cfg(target_os = "windows")]
        assert!(
            vst3_locations().contains("%COMMONPROGRAMFILES%\\VST3"),
            "{}",
            vst3_locations()
        );
        #[cfg(target_os = "linux")]
        assert!(
            vst3_locations().contains("~/.vst3") && vst3_locations().contains("/usr/lib/vst3"),
            "{}",
            vst3_locations()
        );
        // FreeBSD names what `scan.rs` walks — `/usr/local/lib/vst3`, never
        // `/usr/lib/vst3`, which ports cannot install to — and says LV2 out
        // loud, because on FreeBSD an empty VST3 list is the ordinary state
        // of a machine whose instruments are all LV2.
        #[cfg(target_os = "freebsd")]
        assert!(
            vst3_locations().contains("~/.vst3")
                && vst3_locations().contains("/usr/local/lib/vst3")
                && !vst3_locations().contains("/usr/lib/vst3")
                && vst3_locations().contains("LV2"),
            "{}",
            vst3_locations()
        );

        let mut dialog = Some(Dialog::plugin_picker(&[], None, false));
        let (painted, action) = run_picker(&mut dialog);
        assert!(action.is_none(), "an empty picker loaded something");
        assert!(
            painted.contains(vst3_locations()),
            "the help never reached the screen: {painted}"
        );
        // The warning is not conditional on there being a list. It is the same
        // dialog and it will be read again the moment a plugin is installed.
        assert!(painted.contains(PLUGIN_TRUST_NOTE), "{painted}");
        // And the None row survives an empty folder, because the plugin that is
        // loaded may be the one whose folder has moved — unloading it is the
        // one thing this picker can still do.
        assert!(painted.contains(PLUGIN_NONE_ROW), "{painted}");
        assert!(dialog.is_some(), "the picker closed itself");
    }

    /// Opening on row one, with nothing marked, makes the user answer "what am
    /// I playing through" by reading 112 rows. The loaded bundle is preselected
    /// and marked, and — because it may be ninety rows down — the mark is only
    /// worth anything if the list opens on it, which is what the `focus` flag
    /// also does. This runs the frame inline, the way a plugin editor draws it.
    #[test]
    fn the_loaded_plugin_opens_preselected_and_marked() {
        let bundles: Vec<(PathBuf, Option<String>, Option<String>)> = [
            "/Library/Audio/Plug-Ins/VST3/Vital.vst3",
            // Lower case on purpose: `discover` sorts paths, so a byte sort
            // would file this after "Vital" and a user would call the list
            // unsorted.
            "/Library/Audio/Plug-Ins/VST3/analog lab v.vst3",
            "/Users/x/Library/Audio/Plug-Ins/VST3/Pianoteq 8.vst3",
        ]
        .iter()
        .map(|p| (PathBuf::from(p), None, None))
        .collect();
        let loaded = "/Users/x/Library/Audio/Plug-Ins/VST3/Pianoteq 8.vst3".to_owned();

        let opened = Dialog::plugin_picker(&bundles, Some(loaded.clone()), false);
        match &opened {
            Dialog::PluginPicker {
                plugins,
                selected,
                current,
                open_folders: _,
                filter,
                focus,
            } => {
                assert_eq!(
                    plugins.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
                    // The house shelf leads, above the sort — instruments
                    // then the suite, the order the folder shows them — and
                    // it is what a fresh install can still choose with no
                    // VST3s on disk.
                    [
                        BUILTIN_NAME,
                        DRUMPAD_NAME,
                        KEYS_NAME,
                        "Tangent EQ",
                        "Tangent Imager",
                        "Tangent Tape",
                        "Tangent Comp",
                        "Tangent Denoise",
                        "Tangent Amp",
                        "Tangent Drive",
                        "Tangent Trem",
                        "Tangent Chorus",
                        "Tangent Echo",
                        "Tangent Plate",
                        "Tangent Wah",
                        "Tangent IR",
                        "analog lab v",
                        "Pianoteq 8",
                        "Vital"
                    ],
                    "sorted the way a byte comparison sorts, not the way a person reads"
                );
                // The name is the stem: nobody wants to read ".vst3" 112 times.
                assert!(plugins.iter().all(|p| !p.name.contains(".vst3")));
                // Nothing was opened, so nothing knows a vendor. This is the
                // assertion that fails the day the picker starts scanning.
                // The built-in is exempt: it was not scanned, it is compiled
                // in, and it is the one entry that can describe itself.
                assert!(plugins
                    .iter()
                    .filter(|p| !is_builtin(&p.path))
                    .all(|p| p.vendor.is_empty()));
                // Seventeen, not one: the house shelf took the first sixteen
                // rows — three instruments and the thirteen-unit suite.
                assert_eq!(*selected, Some(17), "it did not open on what is loaded");
                assert_eq!(current.as_deref(), Some(loaded.as_str()));
                assert!(filter.is_empty() && *focus);
                assert!(
                    plugin_row(&plugins[17], Some(&loaded)).starts_with('\u{2022}'),
                    "the loaded plugin is not marked"
                );
                assert!(
                    plugin_row(&plugins[3], Some(&loaded)).starts_with(' '),
                    "a plugin that is not loaded was marked as if it were"
                );
                // Every row is bare — the folder heading carries the vendor,
                // the house shelf's included.
                assert_eq!(plugin_row(&plugins[18], None), "  Vital");
                assert_eq!(plugin_row(&plugins[0], None), format!("  {BUILTIN_NAME}"));
                // The vendor is a FOLDER heading now, never a row suffix —
                // the owner's ruling. A scanned plugin's row is bare.
                assert_eq!(
                    plugin_row(&entry("/x/A.vst3", "A", "Arturia"), None),
                    "  A"
                );
            }
            _ => panic!("plugin_picker built some other dialog"),
        }

        let mut dialog = Some(opened);
        let (painted, action) = run_picker(&mut dialog);
        assert!(action.is_none(), "the picker loaded a plugin unasked");
        assert!(
            painted.contains("\u{2022} Pianoteq 8"),
            "the mark never reached the screen: {painted}"
        );
        assert!(
            painted.contains(PLUGIN_NONE_ROW) && painted.contains(PLUGIN_SCAN_NOTE),
            "{painted}"
        );
    }

    /// Typing four letters and pressing Enter is how a list of 112 is used, so
    /// the key has to go all the way through the funnel and come out as the
    /// action. What it must NOT do is answer the keystroke that opened the
    /// dialog: inline there is one context and one event queue, and a picker
    /// that confirmed itself before it was read would be a dialog nobody could
    /// see.
    #[test]
    fn enter_loads_the_highlighted_plugin_but_never_on_the_frame_that_opened_the_dialog() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let placement = Placement {
at: None,
                        native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let held_enter = || egui::RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 520.0))),
            events: vec![egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };

        let loaded = "/Users/x/Library/Audio/Plug-Ins/VST3/Pianoteq 8.vst3".to_owned();
        let mut dialog = Some(Dialog::plugin_picker(
            &[
                (
                    PathBuf::from("/Library/Audio/Plug-Ins/VST3/Vital.vst3"),
                    None,
                    None,
                ),
                (PathBuf::from(&loaded), None, None),
            ],
            Some(loaded.clone()),
            false,
        ));

        let mut action = None;
        let _ = ctx.run(held_enter(), |ctx| {
            action = show(ctx, &mut dialog, Color32::from_rgb(0xE3, 0xD9, 0xBD), placement);
        });
        assert!(
            action.is_none() && dialog.is_some(),
            "the picker answered the keystroke that opened it"
        );

        let _ = ctx.run(held_enter(), |ctx| {
            action = show(ctx, &mut dialog, Color32::from_rgb(0xE3, 0xD9, 0xBD), placement);
        });
        match action {
            Some(DialogAction::LoadPlugin { path }) => {
                assert_eq!(path.as_deref(), Some(loaded.as_str()), "it loaded another");
            }
            _ => panic!("Enter did not load the highlighted plugin"),
        }
        assert!(dialog.is_none(), "it loaded a plugin and stayed open");
    }

    /// The name is what is on screen and the path is what is stored. Two
    /// vendors ship a "Piano", plugins are renamed between versions, and a
    /// settings file holding a display name comes back as nothing at all.
    #[test]
    fn confirming_commits_the_bundle_path_and_never_the_display_name() {
        let plugins = installed();
        let choice =
            plugin_choice(&plugins, Some(1), None, "", &[plugin_folder(&plugins[1]).to_owned()]).expect("a highlighted row is a choice");
        assert_eq!(choice, PluginChoice::Load(&plugins[1]));
        match choice.action() {
            DialogAction::LoadPlugin { path } => {
                assert_eq!(path.as_deref(), Some(plugins[1].path.as_str()));
                assert_ne!(path.as_deref(), Some(plugins[1].name.as_str()));
                assert!(
                    path.as_deref().is_some_and(|p| p.ends_with(".vst3")),
                    "what travelled was not a bundle path: {path:?}"
                );
            }
            _ => panic!("Load produced some other action"),
        }
    }

    /// The two notes get a fixed band so the buttons do not walk under the
    /// cursor as the list is filtered. That only works if what they say fits —
    /// and "it comes to about four lines" is exactly the reasoning that once
    /// pushed a dialog's OK button off its own bottom edge.
    #[test]
    fn the_notes_under_the_list_fit_the_space_reserved_for_them() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);

        // Exactly the width the labels are given: the dialog's, less the
        // `Frame` margin on each side.
        let content_w = PLUGIN_DIALOG_W - 2.0 * f32::from(PLUGIN_MARGIN);
        let mut total = 0.0_f32;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(
                    Pos2::ZERO,
                    Vec2::new(PLUGIN_DIALOG_W, PLUGIN_DIALOG_H),
                )),
                ..Default::default()
            },
            |ctx| {
                crate::shell::viewport_ui(ctx, |ui| {
                    let height = |m: &str| {
                        ui.painter()
                            .layout(
                                m.to_owned(),
                                FontId::new(PLUGIN_NOTE_PT, fonts::courier_bold()),
                                Color32::WHITE,
                                content_w,
                            )
                            .size()
                            .y
                    };
                    // Both are painted, every frame, one under the other.
                    total = height(PLUGIN_TRUST_NOTE)
                        + height(PLUGIN_SCAN_NOTE)
                        + ui.spacing().item_spacing.y;
                });
            },
        );
        assert!(
            total <= PLUGIN_FOOTER_H,
            "{PLUGIN_FOOTER_H}pt is reserved for the notes and they need {total}pt, \
             which would push Load off the bottom edge"
        );
    }

    /// The About card is the one place the app says what it IS, and it said
    /// "Simple MIDI Keyboard Monitor" directly above a 5.0 version number for
    /// the whole beta run. Read as painted: the 5.0 sentence, the support
    /// address, the composed version — and no trace of the 1.x line.
    #[test]
    fn about_describes_the_daw_and_names_the_support_address() {
        let mut dialog = Some(Dialog::About);
        let (painted, action) = run_picker(&mut dialog);
        assert!(action.is_none(), "About asked the app to do something");
        assert!(
            painted.contains("A recording DAW with chord intelligence"),
            "{painted}"
        );
        assert!(
            painted.contains("tangentdaw@gmail.com"),
            "the support address never reached the screen: {painted}"
        );
        assert!(
            !painted.contains("Simple MIDI Keyboard Monitor"),
            "the 1.x tagline is back: {painted}"
        );
        // Composed from the crate version, never retyped — the assertion
        // holds across releases precisely because both sides move together.
        assert!(
            painted.contains(concat!("Version ", env!("CARGO_PKG_VERSION"))),
            "{painted}"
        );
    }

    /// The key dialog asks for a key, and until 5.1 never said where one
    /// comes from — the only purchase link lived in the README, which nobody
    /// inside the app is reading. The pointer shows exactly while there is no
    /// key installed, and not to a supporter, who has nothing left to buy.
    #[test]
    fn the_key_dialog_points_at_the_store_until_a_key_is_installed() {
        let mut dialog = Some(Dialog::SupporterKey {
            input: String::new(),
            message: None,
            installed_as: None,
            focus: false,
        });
        let (painted, _) = run_picker(&mut dialog);
        assert!(
            painted.contains("ganten.gumroad.com/l/ivory"),
            "no way to buy a key from the dialog that asks for one: {painted}"
        );

        let mut dialog = Some(Dialog::SupporterKey {
            input: String::new(),
            message: None,
            installed_as: Some("A. Supporter".to_owned()),
            focus: false,
        });
        let (painted, _) = run_picker(&mut dialog);
        assert!(
            !painted.contains("gumroad"),
            "a supporter is being sold a key they already have: {painted}"
        );
    }

    /// Every fill a frame painted, for the test that asks about COLOR the way
    /// [`painted_text`] asks about words.
    fn painted_fills(output: &egui::FullOutput) -> Vec<Color32> {
        fn walk(shape: &egui::epaint::Shape, out: &mut Vec<Color32>) {
            match shape {
                egui::epaint::Shape::Rect(r) => out.push(r.fill),
                egui::epaint::Shape::Vec(v) => {
                    for s in v {
                        walk(s, out);
                    }
                }
                _ => {}
            }
        }
        let mut out = Vec::new();
        for c in &output.shapes {
            walk(&c.shape, &mut out);
        }
        out
    }

    // ── the export window ───────────────────────────────────────────────────

    /// The size this window opened at for years, kept here as the thing the
    /// new one is measured against rather than as anything that still runs.
    const OLD_EXPORT_SIZE: Vec2 = Vec2::new(520.0, 210.0);

    /// **The window is a share of the screen, with a floor and a ceiling.**
    ///
    /// 520 by 210 was a percentage of nothing in particular: comfortable on
    /// the display this app was written on, a postage stamp on a 5K. This is
    /// `main_width`'s rule applied to the one window whose whole content is
    /// a picture — the same share of a laptop and of a 6K, and never bigger
    /// than the screen it opens on.
    #[test]
    fn the_export_window_is_a_share_of_the_screen_between_a_floor_and_a_ceiling() {
        // Nothing measured yet — the first frames on a new machine. The
        // floor, because a guess about an unseen screen is worse.
        assert_eq!(export_window_size(None), EXPORT_MIN);
        assert_eq!(export_window_size(Some(Vec2::ZERO)), EXPORT_MIN);

        let screens = [
            ("a 13-inch laptop", Vec2::new(1280.0, 800.0)),
            ("a 1080p desk", Vec2::new(1920.0, 1080.0)),
            ("the reference display", Vec2::new(2560.0, 1400.0)),
            ("a 5K", Vec2::new(2560.0, 1440.0)),
            ("a 6K XDR", Vec2::new(3008.0, 1692.0)),
        ];
        let mut last = Vec2::ZERO;
        for (name, mon) in screens {
            let size = export_window_size(Some(mon));
            assert!(
                size.x >= EXPORT_MIN.x && size.y >= EXPORT_MIN.y,
                "{name}: {size:?} is under the floor {EXPORT_MIN:?}"
            );
            assert!(
                size.x <= EXPORT_MAX.x && size.y <= EXPORT_MAX.y,
                "{name}: {size:?} is over the ceiling {EXPORT_MAX:?}"
            );
            assert!(
                size.x <= mon.x && size.y <= mon.y,
                "{name}: a {size:?} window does not fit a {mon:?} screen"
            );
            // The point of the whole exercise, measured where the owner
            // asked for it: bigger than the fixed window it replaces in both
            // directions, and a WAVE more than twice the 96 points it used
            // to get. A 13-inch laptop is the tight case — the window there
            // is only 1.7 times the area, and the waveform is still two and
            // a half times as tall, because the growth went where it was
            // wanted rather than into furniture.
            assert!(
                size.x >= OLD_EXPORT_SIZE.x && size.y >= OLD_EXPORT_SIZE.y,
                "{name}: {size:?} is not bigger than {OLD_EXPORT_SIZE:?}"
            );
            let wave = export_layout(Rect::from_min_size(Pos2::ZERO, size)).wave;
            assert!(
                wave.height() >= 2.0 * 96.0,
                "{name}: the wave is {:.0} points, against the 96 it had",
                wave.height()
            );
            // A bigger screen never gets a smaller window.
            assert!(
                size.x >= last.x && size.y >= last.y,
                "{name}: {size:?} is smaller than the screen below it got ({last:?})"
            );
            last = size;
        }

        // **The numbers the commit and the docs quote, pinned.**
        //
        // The commit that shipped this window said "96 to 542 on the 2560 by
        // 1400 reference display", and 542 is the wave at the CEILING — a
        // 620-point window, which needs 1550 points of screen height and the
        // reference display has 1400. The real answer there is 482. Written
        // down as assertions because prose about arithmetic drifts and
        // arithmetic does not.
        let wave_at = |mon: Vec2| {
            let size = export_window_size(Some(mon));
            export_layout(Rect::from_min_size(Pos2::ZERO, size)).wave.height()
        };
        for (name, mon, want) in [
            ("a 13-inch laptop", Vec2::new(1280.0, 800.0), 242.0_f32),
            ("the reference display", Vec2::new(2560.0, 1400.0), 482.0),
            ("a 5K", Vec2::new(2560.0, 1440.0), 498.0),
            ("a 6K XDR, at the ceiling", Vec2::new(3008.0, 1692.0), 542.0),
        ] {
            let got = wave_at(mon);
            assert!(
                (got - want).abs() < 0.5,
                "{name}: the wave is {got:.0} points, not the {want:.0} the docs claim"
            );
        }
        // 542 is the ceiling's wave and nothing beats it, however big the
        // display — which is the half of that claim worth keeping.
        assert!(
            (wave_at(Vec2::new(7680.0, 4320.0)) - 542.0).abs() < 0.5,
            "the ceiling stopped capping the wave"
        );

        // Both limits have to actually BITE somewhere, or they are decoration
        // and the next change to the share will sail past them unnoticed.
        assert_eq!(
            export_window_size(Some(Vec2::new(3008.0, 1692.0))),
            EXPORT_MAX,
            "the ceiling never engages"
        );
        assert_eq!(
            export_window_size(Some(Vec2::new(900.0, 600.0))),
            EXPORT_MIN,
            "the floor never engages"
        );

        // And a canvas too small even for the floor — a plugin editor, where
        // `monitor` is the editor's own pane. The window comes down to fit,
        // and the minimum it may be dragged to comes down with it, because a
        // minimum larger than the size just asked for is a window that
        // cannot be placed.
        let tiny = Vec2::new(400.0, 300.0);
        let size = export_window_size(Some(tiny));
        assert!(size.x < EXPORT_MIN.x && size.y < EXPORT_MIN.y, "{size:?}");
        assert!(size.x <= tiny.x && size.y <= tiny.y, "{size:?} escapes {tiny:?}");
        let least = export_min_size(size);
        assert!(
            least.x <= size.x && least.y <= size.y,
            "the minimum {least:?} is bigger than the size {size:?}"
        );
        // On every real screen the minimum IS the floor.
        assert_eq!(export_min_size(export_window_size(Some(Vec2::new(1920.0, 1080.0)))), EXPORT_MIN);
    }

    /// **One layout, every part — and the wave gets the lion's share.**
    ///
    /// The parts used to be three `allocate_exact_size` calls with a
    /// hard-coded 96-point wave in a 210-point window, which only agreed
    /// with itself at that one size. Now the window follows the monitor, so
    /// the arithmetic has to hold at every size the derivation can produce:
    /// nothing escapes the window, nothing overlaps, the order down the
    /// window is label, wave, bar, and every point the window gains goes to
    /// the wave.
    #[test]
    fn the_export_windows_parts_come_from_one_layout() {
        let sizes = [
            export_window_size(None),
            export_window_size(Some(Vec2::new(1280.0, 800.0))),
            export_window_size(Some(Vec2::new(2560.0, 1400.0))),
            export_window_size(Some(Vec2::new(3008.0, 1692.0))),
            export_window_size(Some(Vec2::new(400.0, 300.0))),
        ];
        for size in sizes {
            // Placed away from the origin on purpose: a layout that only
            // works at (0, 0) is one that has confused a size for a rect.
            let rect = Rect::from_min_size(Pos2::new(37.0, 19.0), size);
            let l = export_layout(rect);
            for (what, r) in [("label", l.label), ("wave", l.wave), ("bar", l.bar)] {
                assert!(
                    rect.expand(0.01).contains_rect(r),
                    "{size:?}: the {what} {r:?} escapes the window {rect:?}"
                );
                assert!(r.width() >= 0.0 && r.height() >= 0.0, "{what} inverted: {r:?}");
            }
            // Down the window in reading order, sharing nothing.
            assert!(
                l.label.bottom() <= l.wave.top() + 0.01,
                "{size:?}: the label runs into the wave"
            );
            assert!(
                l.wave.bottom() <= l.bar.top() + 0.01,
                "{size:?}: the wave runs into the bar"
            );
            // One left edge and one width for all three, which is what lets
            // the bar's fill and the playhead be the same measurement.
            assert!(
                (l.label.left() - l.wave.left()).abs() < 0.01
                    && (l.bar.left() - l.wave.left()).abs() < 0.01
                    && (l.label.width() - l.wave.width()).abs() < 0.01
                    && (l.bar.width() - l.wave.width()).abs() < 0.01,
                "{size:?}: the parts do not share a column"
            );
            // **The wave is the window.** Two thirds is the floor at the
            // smallest size the layout is ever handed; on a real screen it
            // is well over four fifths.
            assert!(
                l.wave.height() >= rect.height() * 2.0 / 3.0,
                "{size:?}: the wave got {:.0} of {:.0} points — the furniture is \
                 taking the room the waveform is the point of",
                l.wave.height(),
                rect.height()
            );
            // And more than twice the 96 points it used to get, on every
            // screen big enough to hold the floor.
            if size.y >= EXPORT_MIN.y {
                assert!(
                    l.wave.height() >= 2.0 * 96.0,
                    "{size:?}: the wave is {:.0} points, no bigger than it was",
                    l.wave.height()
                );
            }
            // The playhead is measured on the WAVE and clamped, because a
            // fraction outside 0..=1 is a rounding error at the end of a
            // render and not a reason to draw off the window.
            assert!((l.playhead(0.0) - l.wave.left()).abs() < 0.01);
            assert!((l.playhead(1.0) - l.wave.right()).abs() < 0.01);
            assert!((l.playhead(0.5) - l.wave.center().x).abs() < 0.01);
            assert!((l.playhead(-3.0) - l.wave.left()).abs() < 0.01);
            assert!((l.playhead(9.0) - l.wave.right()).abs() < 0.01);
        }

        // Squeezed under its own minimum — which the inline shell can do on a
        // very short plugin editor. Nothing inverts and nothing escapes.
        for h in [90.0_f32, 40.0, 10.0, 0.0] {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(200.0, h));
            let l = export_layout(rect);
            for (what, r) in [("label", l.label), ("wave", l.wave), ("bar", l.bar)] {
                assert!(
                    r.height() >= -0.01 && r.width() >= -0.01,
                    "at {h} points the {what} inverted: {r:?}"
                );
            }
            assert!(l.label.bottom() <= l.wave.top() + 0.01, "at {h} points");
            assert!(l.wave.bottom() <= l.bar.top() + 0.01, "at {h} points");
        }
    }

    /// **The export window is readable on every theme, in both modes, or the
    /// build fails.**
    ///
    /// The sweep `shell.rs` runs on the chrome, on the one window that is on
    /// screen for minutes at a time and has just become the biggest thing
    /// this app floats. Every colour is measured on the surface it lands on:
    /// the label on the window's ground, the wave's ink on the WELL it is
    /// drawn in and not on the ground beside it, and the playhead — the one
    /// colour here with an identity of its own, and a luminance of 0.21,
    /// which is inside the dead zone.
    ///
    /// The zero line is deliberately absent: it is `t.text` at a quarter
    /// alpha, a guide rather than ink, and measuring a translucent colour as
    /// though it were opaque would be measuring the wrong thing.
    ///
    /// **Seventy-six dressings, not thirty-eight.** Nineteen factory themes,
    /// two modes each, and TWO faces this window can be handed — the desk's
    /// strip and the band's ground, because it reads whichever one `show`
    /// remembered. The first draft of this test counted the faces and then
    /// wrote thirty-eight in its own prose, which is how the commit that
    /// shipped it under-reported the playhead's failures by half. The counts
    /// are asserted below rather than described, so the prose cannot drift
    /// from them again.
    #[test]
    fn every_theme_dresses_the_export_window() {
        use crate::recorder_panel::contrast_ratio;
        let themes = crate::theme::factory();
        assert!(
            themes.len() >= 19,
            "the sweep expects the nineteen factory themes, found {}",
            themes.len()
        );
        let mut bad: Vec<String> = Vec::new();
        let mut swept = 0;
        let mut moved = 0;
        let mut wells: Vec<Color32> = Vec::new();
        let mut reds = std::collections::HashSet::new();
        for theme in &themes {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                // The two faces `show` can have remembered when the export
                // window reads one: the desk's strip and the band's ground.
                let strip = if dark {
                    set.dark_role("fx_strip").map_or_else(
                        || crate::mixer_panel::shade_face(set.fx_strip_color.to_color32(), 0.88),
                        |c| c.to_color32(),
                    )
                } else {
                    set.fx_strip_color.to_color32()
                };
                let band = crate::recorder_panel::palette(&set).bg;
                let mode = if dark { "dark" } else { "light" };
                for (which, face) in [("strip", strip), ("band", band)] {
                    let t = theme_of(face);
                    let head = playhead_ink(t.field_bg);
                    let mut check = |what: &str, r: f32, floor: f32| {
                        if r < floor {
                            bad.push(format!(
                                "{} {mode} ({which}): {what} {r:.2}:1",
                                theme.name
                            ));
                        }
                    };
                    check("the label on the ground", contrast_ratio(t.text, t.bg), 4.5);
                    check(
                        "the wave on the well",
                        contrast_ratio(t.text, t.field_bg),
                        4.5,
                    );
                    // The wave's box has to be FINDABLE, or the waveform
                    // floats in an unbounded rectangle. The well alone is
                    // not enough — on Porcelain light, Phosphor dark and
                    // Silverface dark it lands within 1.1:1 of the window's
                    // own ground — so the box is drawn with a hairline and
                    // it is the hairline that is measured, from both sides.
                    check(
                        "the wave's edge against the ground",
                        contrast_ratio(t.button_border, t.bg),
                        1.5,
                    );
                    check(
                        "the wave's edge against the well",
                        contrast_ratio(t.button_border, t.field_bg),
                        1.5,
                    );
                    // The non-text floor: this is a one-point marker, not a
                    // word. See `playhead_ink`.
                    check("the playhead on the well", contrast_ratio(head, t.field_bg), 3.0);
                    reds.insert((head.r(), head.g(), head.b()));
                    if head != Color32::from_rgb(0xE8, 0x3A, 0x4E) {
                        moved += 1;
                    }
                    wells.push(t.field_bg);
                    swept += 1;
                }
            }
        }
        // Nineteen themes, two modes, two faces. Named as the product it is
        // so a face added to the list moves the number rather than the
        // comment about it.
        assert_eq!(
            swept,
            themes.len() * 2 * 2,
            "the sweep covered {swept} dressings, not one per theme per mode per face"
        );
        assert_eq!(swept, 76, "the sweep is no longer seventy-six dressings");
        assert!(
            bad.is_empty(),
            "the export window is unreadable:\n  {}",
            bad.join("\n  ")
        );
        // **And it is still a RED playhead.** A walk that ran to the end
        // would leave the marker as the body ink, and nobody would ever see
        // a playhead again — so every answer has to read as red, and the
        // shipped red itself has to survive untouched on the dressings that
        // can carry it (it does on sixty-nine of the seventy-six).
        assert!(
            reds.contains(&(0xE8, 0x3A, 0x4E)),
            "the walk moved the playhead on every theme; it is no longer the \
             colour it was designed as"
        );
        for (r, g, b) in &reds {
            assert!(
                *r > *g + 40 && *r > *b + 20,
                "the playhead came out #{r:02X}{g:02X}{b:02X}, which is not red"
            );
        }
        // **The exact count, because the doc quotes it.** Seven of the
        // seventy-six need the move and sixty-nine do not. A palette change
        // that moves this number is not a bug — it is a fact the prose above
        // `playhead_ink` is claiming, and this is what makes the two argue.
        assert_eq!(
            moved, 7,
            "the shipped red now needs moving on {moved} of {swept} dressings, \
             not the seven `playhead_ink` says"
        );
        // And it IS the house helper, not a walk of its own. `distinct_
        // surface_at` is the rule every other surface here is measured with;
        // hand-rolling a second one is how two surfaces end up disagreeing
        // about the same well.
        for well in wells {
            assert_eq!(
                playhead_ink(well),
                crate::recorder_panel::distinct_surface_at(
                    Color32::from_rgb(0xE8, 0x3A, 0x4E),
                    well,
                    3.0
                ),
                "the playhead stopped using the house helper on {well:?}"
            );
        }
    }

    /// The export window wears the desk's face, not the retired ivory.
    ///
    /// It is not a `Dialog`, so it never sees `show`'s `face` parameter — it
    /// reads the face `show` remembered this frame instead (see
    /// [`remember_face`]). This runs the frame the app actually runs — `show`
    /// first with no dialog open, the export window after — and then asks
    /// what the window's ground was painted with.
    #[test]
    fn the_export_window_wears_the_desks_face_not_ivory() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        // The colors are compared EXACTLY, and an `Area` fades in by
        // multiplying every color it paints — with the test clock standing
        // still the fade never finishes and every fill compares unequal at
        // two-thirds opacity. No animation, no multiplication.
        ctx.style_mut(|s| s.animation_time = 0.0);
        // Before `show` has ever run, the historical ivory — a frame the app
        // cannot produce, because `show` is called unconditionally ahead of
        // the export window every update.
        assert_eq!(live_face(&ctx), FALLBACK_FACE);

        let olive = Color32::from_rgb(0x6B, 0x71, 0x3E);
        let placement = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let p = crate::ports::ExportProgress {
            fraction: 0.5,
            label: "Rendering".to_owned(),
            peaks: vec![(-0.4, 0.4); 16],
        };
        // Three frames for the same reason `run_picker` runs three: the
        // sizing pass paints nothing.
        let mut fills = Vec::new();
        for _ in 0..3 {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 520.0))),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, &mut None, olive, placement);
                    show_export_progress(ctx, &p, placement);
                },
            );
            fills = painted_fills(&out);
        }
        assert_eq!(live_face(&ctx), olive, "`show` did not record the face");
        assert!(
            fills.contains(&theme_of(olive).bg),
            "the export window's ground is not the desk's face"
        );
        assert!(
            !fills.contains(&theme_of(FALLBACK_FACE).bg),
            "ivory is still painted somewhere in the export window"
        );
    }

    /// **The window's size and its position are frozen TOGETHER.**
    ///
    /// A position is only a position for a size: `position_for` centres one
    /// inside the other. Freezing the position alone — which the first draft
    /// did — leaves the window off-centre the moment the size would have
    /// changed under it, which is a monitor change or a host reporting a
    /// different canvas mid-render. And both have to be let go when the
    /// export ends, or the next one opens where and how the last one did.
    #[test]
    fn the_export_windows_size_and_place_are_frozen_together_and_let_go_together() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let at = |monitor: Vec2| Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(Rect::from_min_size(Pos2::new(100.0, 60.0), Vec2::new(900.0, 600.0))),
            monitor: Some(monitor),
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let big = Vec2::new(2560.0, 1400.0);
        let small = Vec2::new(1280.0, 800.0);
        // The two displays really do want different sizes, or this test
        // proves nothing.
        assert_ne!(export_window_size(Some(big)), export_window_size(Some(small)));

        let mut first = None;
        let run = |monitor: Vec2, out: &mut Option<(Vec2, Option<Pos2>)>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| *out = Some(export_geometry(ctx, at(monitor))),
            );
        };
        run(big, &mut first);
        let (size, pos) = first.expect("the first frame answered nothing");
        assert_eq!(size, export_window_size(Some(big)));
        assert_eq!(pos, at(big).position_for(size), "it did not open where it was placed");

        // The monitor changes under a live render. NEITHER moves: the pair
        // was worked out together and is held together.
        for _ in 0..4 {
            let mut got = None;
            run(small, &mut got);
            assert_eq!(
                got,
                Some((size, pos)),
                "the frozen window followed the monitor"
            );
        }
        // Skip two passes — the export ended — and the next one opens fresh,
        // at the size and place of the screen it is started on.
        for _ in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |_ctx| {},
            );
        }
        let mut again = None;
        run(small, &mut again);
        let (size2, pos2) = again.expect("the next export answered nothing");
        assert_eq!(size2, export_window_size(Some(small)), "it reopened at the old size");
        assert_eq!(pos2, at(small).position_for(size2), "it reopened in the old place");
    }

    /// What the app is asking for the one dialog window this frame — read
    /// back through the same one-pass liveness `held_geometry` writes it
    /// with, so a rectangle left behind by a window that has closed reads as
    /// nothing rather than as this window's.
    fn asked_geometry(ctx: &egui::Context) -> Option<(Vec2, Option<Pos2>)> {
        let pass = ctx.cumulative_pass_nr();
        ctx.memory(|m| m.data.get_temp::<Held>(egui::Id::new((DIALOG_ID, "geometry"))))
            .filter(|h| pass.saturating_sub(h.pass) <= 1)
            .map(|h| (h.size, h.pos))
    }

    /// **A plugin window that has been placed is never placed again.**
    ///
    /// The owner, on the bug this pins: "open plugin window --> tab out or
    /// workspace change with it still open --> tab back = the plugin window
    /// is now on the right bottom of your monitor. consistent."
    ///
    /// The mechanism, in three steps and all of them in this file. The
    /// position was recomputed from the parent's rect on every frame; egui
    /// turns a `pos` that has changed into a `ViewportCommand::OuterPosition`
    /// (`ViewportBuilder::patch`); and `clamp_to_monitor` handed a position
    /// well off the screen does not nudge it but returns exactly
    /// `monitor - size`, which IS the bottom-right corner. So one frame with
    /// a stale or off-screen parent — which is what a hide, a Space change or
    /// a display change can produce — moved the window there for good,
    /// because nothing ever asked again afterwards.
    ///
    /// The fix is that the window is placed ONCE, so the frames afterwards
    /// have nothing to say. This drives the real funnel — `show`, the real
    /// `Dialog`, the real `show_dialog_viewport` — and walks the parent
    /// around underneath it, ending with the rect that used to corner it.
    #[test]
    fn a_placed_plugin_window_is_never_moved_again_by_the_parent() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let monitor = Vec2::new(1512.0, 982.0);
        let screen = Rect::from_min_size(Pos2::ZERO, monitor);
        let at = |parent: Rect| Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(parent),
            monitor: Some(monitor),
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let olive = Color32::from_rgb(0x6B, 0x71, 0x3E);
        let frame = |dialog: &mut Option<Dialog>, parent: Rect| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, dialog, olive, at(parent));
                },
            );
        };
        let face = || {
            Some(Dialog::InsertFace {
                strip: 0,
                slot: 0,
                live: Box::default(),
            })
        };

        let home = Rect::from_min_size(Pos2::new(80.0, 60.0), Vec2::new(1240.0, 800.0));
        let mut dialog = face();
        frame(&mut dialog, home);
        let (size, pos) = asked_geometry(&ctx).expect("the face was never placed");
        assert_eq!(
            pos,
            at(home).position_for(size),
            "it did not open centred on the window it belongs to"
        );

        // The parent moves the way a main window moves, and then the way a
        // host that has lost track of it reports one: off the screen
        // entirely. The last of these is the frame that used to corner the
        // window, and this asserts it exactly.
        // `clamp_to_monitor` rounds what it hands back, so the corner it
        // would answer with is rounded too.
        let corner = Pos2::new((monitor.x - size.x).round(), (monitor.y - size.y).round());
        let lying = Rect::from_min_size(Pos2::new(4000.0, 2600.0), Vec2::new(1240.0, 800.0));
        assert_eq!(
            crate::settings::clamp_to_monitor(lying.center() - size * 0.5, size, Some(monitor)),
            corner,
            "this parent no longer corners a dialog, so the test proves nothing"
        );
        for parent in [
            home.translate(Vec2::new(120.0, 40.0)),
            home.translate(Vec2::new(-60.0, 90.0)),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(700.0, 500.0)),
            lying,
            lying,
        ] {
            frame(&mut dialog, parent);
            assert_eq!(
                asked_geometry(&ctx),
                Some((size, pos)),
                "the placed window followed the parent to {parent:?}"
            );
        }
        assert_ne!(pos, Some(corner), "it ended up in the corner after all");

        // **Closed and opened again, it centres afresh.** That is the reveal
        // the whole rule turns on: a window is placed when it is born and
        // never afterwards, and closing one is what makes the next one a
        // birth. Three idle passes, the same liveness `shell::surface` uses.
        let mut shut = None;
        for _ in 0..3 {
            frame(&mut shut, home);
        }
        let mut dialog = face();
        frame(&mut dialog, home);
        let (again_size, again_pos) = asked_geometry(&ctx).expect("it never reopened");
        assert_eq!(
            (again_size, again_pos),
            (size, at(home).position_for(size)),
            "the second opening did not centre on the window again"
        );

        // **A different window under the same slot gets its own rectangle.**
        // Every dialog shares one viewport and one slot in the app, and a
        // face that opens a picker is replaced with no undrawn frame between
        // them — so identity is what the window says it is, not the slot it
        // came through.
        let mut picker = Some(Dialog::plugin_picker(&[], None, false));
        frame(&mut picker, home);
        let (other_size, _) = asked_geometry(&ctx).expect("the picker was never placed");
        assert_ne!(
            other_size, size,
            "the picker inherited the face's window instead of asking for its own"
        );

        // **A window nothing could place yet keeps asking.** The welcome card
        // opens on the first frame of a launch, before the host has said
        // where anything is, and `position_for` answers `None` — which is not
        // a place and must not be held, or the first thing anybody ever sees
        // lands wherever the platform felt like putting it.
        for _ in 0..3 {
            frame(&mut shut, home);
        }
        let blind = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let mut welcome = Some(Dialog::Welcome {
            dont_show_again: false,
        });
        for pass in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, &mut welcome, olive, blind);
                },
            );
            assert_eq!(
                asked_geometry(&ctx).and_then(|(_, pos)| pos),
                None,
                "pass {pass}: a window nobody could place was held unplaced"
            );
        }
        // The host reports a monitor, and the card centres on it at once.
        frame(&mut welcome, home);
        let (card, placed) = asked_geometry(&ctx).expect("the card was never placed");
        assert_eq!(
            placed,
            at(home).position_for(card),
            "the card never took the first answer it was given"
        );
    }

    /// **Two dialogs that share a title do not share a size.**
    ///
    /// Identity was the title alone, and titles are not unique: the chord
    /// game's `Cheated` is called "Tangent", and so is a face whose unit the
    /// suite table has never heard of. Every dialog goes through one viewport
    /// and one slot, and one can replace another with no undrawn frame in
    /// between — so the second window found a rectangle already held under
    /// its own name and wore it, throwing away the size its caller had just
    /// worked out. A 440-by-170 question opened as a 520-point knob panel.
    ///
    /// The fix is that the ask is part of the identity, and this drives the
    /// real funnel to prove it: `show`, the real dialogs, no idle frame.
    #[test]
    fn two_dialogs_that_share_a_title_do_not_share_a_size() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let monitor = Vec2::new(1512.0, 982.0);
        let screen = Rect::from_min_size(Pos2::ZERO, monitor);
        let placement = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: Some(Rect::from_min_size(Pos2::new(80.0, 60.0), Vec2::new(1240.0, 800.0))),
            monitor: Some(monitor),
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let olive = Color32::from_rgb(0x6B, 0x71, 0x3E);
        let frame = |dialog: &mut Option<Dialog>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, dialog, olive, placement);
                },
            );
        };

        // The face first. Its unit is not in the suite table, so the window
        // it opens is called "Tangent" — and it is sized from the screen.
        let mut face = Some(Dialog::InsertFace {
            strip: 0,
            slot: 0,
            live: Box::default(),
        });
        frame(&mut face);
        let (face_size, _) = asked_geometry(&ctx).expect("the face was never placed");
        assert_eq!(face_size, face_window_size("", Some(monitor)));

        // And straight into the other window of that name, with no undrawn
        // frame between them, which is the case the holding is FOR.
        let cheat = Vec2::new(440.0, 170.0);
        assert_ne!(cheat, face_size, "the two windows agree, so this proves nothing");
        let mut cheated = Some(Dialog::Cheated);
        frame(&mut cheated);
        let (size, _) = asked_geometry(&ctx).expect("the second window was never placed");
        assert_eq!(
            size, cheat,
            "a window inherited the size of a different window that shares its title"
        );
    }

    /// **A window on a display that goes away is brought back.**
    ///
    /// Holding the geometry fixed the window that walked to the corner, and
    /// took a rescue away with it: the per-frame clamp used to put a window
    /// back on the screen for free, every frame, as a side effect of
    /// re-placing it. Nothing did it afterwards. Undock the laptop with a
    /// face open on the big display and the face is simply gone — still
    /// open, still taking every event a modal drops, at a coordinate the
    /// remaining screen does not reach.
    ///
    /// The answer is neither to go back to asking every frame nor to leave
    /// it alone: a display change re-VALIDATES. The size is kept, because a
    /// hand may be on the grip and the screen changing is not a reason to
    /// resize under it; the position is clamped back onto the screen that is
    /// left. This asserts both halves, and that the window sits still on
    /// every frame either side of the change.
    #[test]
    fn a_window_left_on_a_display_that_goes_away_is_brought_back() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let big = Vec2::new(3840.0, 2160.0);
        let small = Vec2::new(1512.0, 982.0);
        // No parent: the window centres on the screen, which is where a
        // stranding is plainest — the main window is on the display that
        // went away too, and its rect is the thing least worth trusting on
        // the frame a display changes.
        let at = |monitor: Option<Vec2>| Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor,
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let olive = Color32::from_rgb(0x6B, 0x71, 0x3E);
        let frame = |dialog: &mut Option<Dialog>, monitor: Option<Vec2>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, big)),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, dialog, olive, at(monitor));
                },
            );
        };

        let mut dialog = Some(Dialog::InsertFace {
            strip: 0,
            slot: 0,
            live: Box::default(),
        });
        frame(&mut dialog, Some(big));
        let (size, pos) = asked_geometry(&ctx).expect("the face was never placed");
        let pos = pos.expect("a known monitor is enough to place a window");
        assert!(
            pos.x + size.x > small.x || pos.y + size.y > small.y,
            "this window already fits the small screen, so the test proves nothing"
        );

        // It does not move while the screen stays put — the whole point of
        // holding, and the thing the rescue must not undo.
        for _ in 0..3 {
            frame(&mut dialog, Some(big));
            assert_eq!(
                asked_geometry(&ctx),
                Some((size, Some(pos))),
                "the held window moved with nothing under it changing"
            );
        }

        // **A frame where the host says nothing about a monitor is not a
        // display change.** There is nothing to validate the window against,
        // and one silent frame around a hide or a Space switch is not a
        // reason to move somebody's window — that is the fault this whole
        // mechanism exists to stop, arriving through the new door.
        frame(&mut dialog, None);
        assert_eq!(
            asked_geometry(&ctx),
            Some((size, Some(pos))),
            "a frame with no monitor re-placed the window"
        );

        // The display goes away.
        frame(&mut dialog, Some(small));
        let (after, back) = asked_geometry(&ctx).expect("the face vanished");
        let back = back.expect("the rescue took the position away");
        assert_eq!(
            after, size,
            "the rescue resized a window a hand may be on the grip of"
        );
        assert!(
            Rect::from_min_size(Pos2::ZERO, small)
                .expand(0.5)
                .contains_rect(Rect::from_min_size(back, after)),
            "the window is stranded at {back:?} on a {small:?} screen"
        );

        // And it settles there rather than walking every frame afterwards.
        for _ in 0..3 {
            frame(&mut dialog, Some(small));
            assert_eq!(
                asked_geometry(&ctx),
                Some((after, Some(back))),
                "the rescued window kept being rescued"
            );
        }
    }

    /// **How big a face opens: a share of the screen, floored and capped.**
    ///
    /// The floor is the size each face opened at before this rule existed, so
    /// nothing ever gets smaller; the ceiling is that floor scaled by 1.6 in
    /// both directions, so a face keeps its proportions when the cap bites;
    /// and a laptop lands comfortably between the two. See [`FACE_SIZES`].
    #[test]
    fn a_face_is_sized_from_the_screen_between_its_floor_and_its_ceiling() {
        // Every face that has a size of its own, with the size it used to
        // open at. The floor IS that size, which is the promise being kept.
        let was = [
            ("@tangent-amp", Vec2::new(540.0, 360.0)),
            ("@tangent-eq", Vec2::new(470.0, 470.0)),
            ("@tangent-imager", Vec2::new(340.0, 480.0)),
            ("@tangent-comp", Vec2::new(400.0, 430.0)),
            ("@tangent-tape", Vec2::new(400.0, 360.0)),
        ];
        for (unit, floor) in was {
            // No monitor is no guess: the floor, which is today's size.
            assert_eq!(face_window_size(unit, None), floor, "{unit} with no screen");
            // A tiny screen cannot be given the floor and is not given a
            // window bigger than itself either.
            let tiny = Vec2::new(700.0, 520.0);
            let small = face_window_size(unit, Some(tiny));
            assert!(
                small.x <= tiny.x && small.y <= tiny.y,
                "{unit} opened bigger than a {tiny:?} screen: {small:?}"
            );
            // A laptop: meaningfully bigger in both directions, and still a
            // long way from filling the screen.
            let laptop = Vec2::new(1512.0, 982.0);
            let mid = face_window_size(unit, Some(laptop));
            assert!(
                mid.x > floor.x * 1.2 && mid.y > floor.y * 1.2,
                "{unit} is barely bigger on a laptop: {mid:?} against {floor:?}"
            );
            assert!(
                mid.x < laptop.x * 0.6 && mid.y < laptop.y * 0.7,
                "{unit} eats the laptop's screen: {mid:?}"
            );
            // A big desk display: the ceiling, and the ceiling holds the
            // shape the face was drawn at.
            let desk = face_window_size(unit, Some(Vec2::new(3840.0, 2160.0)));
            assert!(
                (desk.x / desk.y - floor.x / floor.y).abs() < 0.01,
                "{unit} changed shape at the ceiling: {desk:?} against {floor:?}"
            );
            assert!(
                (desk.x / floor.x - 1.6).abs() < 0.01,
                "{unit}'s ceiling is not 1.6 times its floor: {desk:?}"
            );
            // Bigger screens never give back a smaller window.
            let mut last = 0.0_f32;
            for w in [700.0_f32, 1280.0, 1440.0, 1512.0, 1920.0, 2560.0, 3840.0] {
                let got = face_window_size(unit, Some(Vec2::new(w, w * 0.62)));
                assert!(got.x >= last - 0.01, "{unit} shrank at {w}: {got:?}");
                last = got.x;
            }
        }
    }

    /// **The IR face is a drumpad-sized window.**
    ///
    /// It is becoming the same kind of thing — a machine on the left, a file
    /// browser on the right — and this is the contract the lane laying out
    /// those two halves builds against. Checked against
    /// `drumpad_panel::place` itself rather than against a number copied out
    /// of it, so the day the drumpad's own rule moves, this says so.
    #[test]
    fn the_ir_face_opens_in_the_drumpads_size_class() {
        for m in [
            Vec2::new(1280.0, 800.0),
            Vec2::new(1512.0, 982.0),
            Vec2::new(1920.0, 1080.0),
            Vec2::new(2560.0, 1440.0),
        ] {
            let ir = face_window_size("@tangent-ir", Some(m));
            let pad = crate::drumpad_panel::place(Rect::from_min_size(Pos2::ZERO, m), None);
            // The same width, because it is derived the same way: four
            // fifths of the screen, the same floor and the same cap.
            assert!(
                (ir.x - pad.width()).abs() < 0.5,
                "the IR face is {ir:?} where the drumpad is {:?} on a {m:?} screen",
                pad.size()
            );
            // And a height in the same class: a browser wants more rows than
            // a pad grid, never fewer, and never so many that it stops being
            // the same kind of window.
            assert!(
                ir.y >= pad.height() && ir.y <= pad.height() * 1.3,
                "the IR face is {ir:?} where the drumpad is {:?} on a {m:?} screen",
                pad.size()
            );
            // Both halves stay usable: the minimum is a browser's, not a
            // knob panel's.
            let least = face_min_size("@tangent-ir", ir);
            assert!(
                least.x >= 600.0 && least.x < ir.x,
                "the IR face can be dragged down to {least:?}"
            );
        }
        // It is meaningfully bigger than the generic face it used to share a
        // size with, which is the whole reason it is called out.
        let laptop = Some(Vec2::new(1512.0, 982.0));
        assert!(
            face_window_size("@tangent-ir", laptop).x
                > face_window_size("@tangent-tape", laptop).x * 2.0
        );
    }

    /// **A real modal dialog opened mid-render comes out in front of it, on
    /// the desktop path.**
    ///
    /// `shell.rs` proves the ladder on synthetic specs; this proves the two
    /// windows the ladder was WRITTEN for are actually on the right rungs —
    /// that `show_dialog_viewport` marks a question as one and the export
    /// window does not accidentally outrank it. It runs the frame the app
    /// runs, in the app's order: the dialog first, the export window after.
    ///
    /// The level read back is the one `shell::surface` last handed the host,
    /// which is the only per-surface answer available headlessly — every
    /// viewport command in a test run lands on the one root output with no
    /// name on it. It is not a claim about where a window manager put the
    /// windows; that is what the by-hand list on `show_export_progress` is
    /// for.
    #[test]
    fn a_question_opened_during_a_render_outranks_it() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let placement = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor: Some(screen.size()),
            caps: crate::host::Caps::DESKTOP,
            chrome: None,
        };
        let p = crate::ports::ExportProgress {
            fraction: 0.5,
            label: "Rendering".to_owned(),
            peaks: vec![(-0.5, 0.5); 32],
        };
        let level = |id: &str| {
            ctx.memory(|m| {
                m.data
                    .get_temp::<egui::WindowLevel>(egui::Id::new((id, "surface-level")))
            })
        };
        let frame = |dialog: &mut Option<Dialog>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    show(ctx, dialog, Color32::from_rgb(0x6B, 0x71, 0x3E), placement);
                    show_export_progress(ctx, &p, placement);
                },
            );
        };
        // The render on its own: it floats.
        let mut none = None;
        frame(&mut none);
        frame(&mut none);
        assert_eq!(
            level(EXPORT_ID),
            Some(egui::WindowLevel::AlwaysOnTop),
            "the render did not float"
        );
        // A question opens. `SaveChanges` is a question — `Dialog::is_modal`
        // says so — and the app drops every main-window event while it is up.
        // (It used to be `NoMidiInput`, which was never a question at all: it
        // had one OK button, and it is a notification now.)
        let mut asking = Some(Dialog::SaveChanges {
            what: "quitting".to_owned(),
            name: "Song".to_owned(),
        });
        assert!(
            asking.as_ref().is_some_and(Dialog::is_modal),
            "this test is no longer about a question"
        );
        for pass in 0..3 {
            frame(&mut asking);
            assert!(asking.is_some(), "the dialog closed itself on pass {pass}");
            assert_eq!(
                level(DIALOG_ID),
                Some(egui::WindowLevel::AlwaysOnTop),
                "pass {pass}: the question did not float"
            );
            assert_eq!(
                level(EXPORT_ID),
                Some(egui::WindowLevel::Normal),
                "pass {pass}: the render stayed on top of a question"
            );
        }
        // The question is answered. The render comes back up on its own —
        // three frames, the settle `shell::standing_claim` documents.
        for _ in 0..3 {
            frame(&mut none);
        }
        assert_eq!(
            level(EXPORT_ID),
            Some(egui::WindowLevel::AlwaysOnTop),
            "the render never recovered from the question"
        );
    }

    /// **A press on the export window is the export window's.**
    ///
    /// Inline the window is a layer over the piano and the recorder band,
    /// and the app's raw-pointer handlers read `i.pointer` straight off the
    /// input — no layer has ever shielded those, the modal scrim included.
    /// They ask [`crate::shell::inline_rect`] where a surface is instead,
    /// and this is the export window's half of that question. On the desktop
    /// the window has a pointer stream of its own and the answer is false,
    /// which is the honest answer and not a gap.
    #[test]
    fn a_pointer_on_the_inline_export_window_reads_as_on_it() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 620.0));
        let placement = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor: Some(screen.size()),
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let p = crate::ports::ExportProgress {
            fraction: 0.5,
            label: "Rendering".to_owned(),
            peaks: vec![(-0.4, 0.4); 16],
        };
        let mut over_middle = false;
        let mut over_corner = false;
        // Three passes: the rect is recorded at the end of a pass and read
        // on the next, and the pointer has to be delivered with the input.
        for i in 0..3 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![egui::Event::PointerMoved(screen.center())],
                    ..Default::default()
                },
                |ctx| {
                    show_export_progress(ctx, &p, placement);
                    if i > 0 {
                        over_middle = pointer_over_export(ctx);
                    }
                },
            );
        }
        assert!(over_middle, "a press in the middle of the window read as off it");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(screen.min + Vec2::splat(2.0))],
                ..Default::default()
            },
            |ctx| {
                show_export_progress(ctx, &p, placement);
                over_corner = pointer_over_export(ctx);
            },
        );
        assert!(!over_corner, "a press in the canvas corner read as on the window");
        // And it is not the dialog: the export window must never take the
        // one dialog slot, which is what `EXPORT_ID` exists for.
        assert!(!dialog_drawn_inline(&ctx), "the export window took the dialog slot");
    }

    /// **The wizard's choices resolve to exactly what its actions will
    /// carry** — pairs by index with a stale selection falling back rather
    /// than panicking, and the sweep-length table answering in seconds.
    /// This is the spec round-trip: what the combo shows IS what
    /// `IrCapture` asks for.
    #[test]
    fn the_ir_wizards_choices_resolve_to_the_pairs_and_seconds_it_asks_for() {
        let mut s = IrWizardState::default();
        assert_eq!(s.stage, IrStage::Io, "a fresh wizard starts at the loop");
        s.out_pairs = vec![(0, 1), (2, 3), (12, 13)];
        s.in_pairs = vec![(0, 1)];
        s.out_sel = 2;
        assert_eq!(s.out_pair(), (12, 13));
        assert_eq!(s.in_pair(), (0, 1));
        // A selection the device no longer has falls back to the main pair.
        s.in_sel = 7;
        assert_eq!(s.in_pair(), (0, 1));
        for (i, want) in IR_SWEEP_CHOICES.iter().enumerate() {
            s.sweep_choice = i;
            assert!((s.sweep_s() - want).abs() < f64::EPSILON);
        }
        s.sweep_choice = 99;
        assert!((s.sweep_s() - 15.0).abs() < f64::EPSILON, "the fallback is the default");
        // And the default levels are the recipe's: −18 first, 10 s of tail.
        let fresh = IrWizardState::default();
        assert!((fresh.level_db + 18.0).abs() < 0.01);
        assert!((fresh.tail_s - 10.0).abs() < 0.01);
        assert!((fresh.sweep_s() - 15.0).abs() < 0.01);
    }

    /// **The wizard walks the same four stages on its second run as on its
    /// first, and "Record another IR" is a loop you can stay in.**
    ///
    /// The report: "the wizard if followed through end to end including a
    /// plugin save at the end, skips stage 3 if the wizard is used again in
    /// succession". It did, and this is the mechanism. The host keeps the
    /// last finished capture on `IrLive` across a close and reopen — on
    /// purpose, so closing the wizard mid-thought costs nobody their pass —
    /// and a reopened wizard is a fresh `Default` handed that same live
    /// picture. The auto-advance asked only "is there a result and am I on
    /// the sweep page", so the moment the second run reached stage 3 it
    /// jumped to stage 4 and showed the FIRST run's IR.
    ///
    /// `armed` is the fix and this is its pin: a result only moves the
    /// wizard if the wizard is the one that asked for it, AND the host's
    /// capture id has moved since it asked.
    ///
    /// **The walk is driven through the production methods, not a copy of
    /// them.** A headless test cannot press an egui button, so the
    /// temptation is to restate `show`'s predicate here and drive that — and
    /// the first version of this test did exactly that, which is worth
    /// nothing: break `IrWizardState::settle_from_live` back to the version
    /// that skipped stage 3 and a test carrying its own copy goes on
    /// passing. `settle_from_live`, `arm_capture` and `arm_reference` are
    /// methods for this reason, and every stage move below goes through
    /// them. Only the HOST's half is written out here, and it is written as
    /// `desktop::start_ir_capture` and `ir_tick` do it.
    #[test]
    fn the_wizard_walks_the_same_stages_on_every_run_and_record_another_loops() {
        // The host's half, and only the host's half. A pass that reaches the
        // engine counts the capture id up and clears the result; the worker
        // lands the outcome some frames later.
        fn host_started(s: &mut IrWizardState) {
            s.live.capture_id += 1;
            s.live.result = None;
        }
        fn host_finished(s: &mut IrWizardState) {
            s.live.result = Some(crate::ports::IrOutcome {
                seconds: 2.4,
                ..Default::default()
            });
        }

        let walk = |s: &mut IrWizardState| -> Vec<IrStage> {
            let mut seen = vec![s.stage];
            s.settle_from_live();
            s.stage = IrStage::Level;
            seen.push(s.stage);
            s.settle_from_live();
            s.stage = IrStage::Sweep;
            seen.push(s.stage);
            // Reaching the sweep page must NOT move on by itself.
            s.settle_from_live();
            assert_eq!(
                s.stage,
                IrStage::Sweep,
                "the sweep page was skipped before anything was armed"
            );
            s.arm_capture();
            s.settle_from_live();
            assert_eq!(s.stage, IrStage::Sweep, "armed is not captured");
            host_started(s);
            s.settle_from_live();
            assert_eq!(s.stage, IrStage::Sweep, "the wizard advanced with no result");
            host_finished(s);
            s.settle_from_live();
            seen.push(s.stage);
            seen
        };

        // Run one, end to end, including naming and saving.
        let mut first = IrWizardState::default();
        let run1 = walk(&mut first);
        assert_eq!(
            run1,
            vec![IrStage::Io, IrStage::Level, IrStage::Sweep, IrStage::Result]
        );
        first.name = "Holmes Plate".to_owned();

        // Done, then opened again: a fresh state, handed the host's live
        // picture — which still carries run one's result. This is the exact
        // shape that skipped stage 3.
        let carried = first.live.clone();
        assert!(
            carried.result.is_some(),
            "the host was supposed to still be holding the finished capture"
        );
        let mut second = IrWizardState::default();
        second.live = carried;
        let run2 = walk(&mut second);
        assert_eq!(run2, run1, "the second run did not walk the same stages");
        assert!(
            second.name.is_empty(),
            "the second capture arrived carrying the name {:?}",
            second.name
        );

        // And "Record another IR" from the result page is the same loop
        // without leaving the wizard: back to the sweep, loop and levels
        // still staged, name and latch cleared.
        second.name = "Room A".to_owned();
        second.out_sel = 3;
        second.in_sel = 1;
        second.level_db = -6.0;
        second.sweep_choice = 2;
        second.tail_s = 20.0;
        second.record_another();
        assert_eq!(second.stage, IrStage::Sweep);
        assert!(second.armed.is_none());
        assert!(second.name.is_empty(), "the next capture inherited a name");
        assert_eq!((second.out_sel, second.in_sel), (3, 1), "the loop was lost");
        assert!((second.level_db + 6.0).abs() < 0.01, "the level was lost");
        assert!((second.sweep_s() - 50.0).abs() < 0.01, "the sweep was lost");
        assert!((second.tail_s - 20.0).abs() < 0.01, "the tail was lost");
        // Sitting on the sweep page with the previous result still pushed
        // must not bounce back to it.
        second.settle_from_live();
        assert_eq!(
            second.stage,
            IrStage::Sweep,
            "record-another bounced straight back to the old result"
        );

        // **An arm the host REFUSED must not advance onto the last IR.**
        //
        // `start_ir_capture` gives up before it starts anything when no
        // engine is running or the sweep will not generate, and the result
        // on the live push is deliberately sticky, so the wizard sat armed
        // over the PREVIOUS capture and showed it — one click from saving
        // somebody else's measurement under this name. The refused pass
        // never moves the capture id, so the advance never fires.
        let stale = second.live.result.clone();
        assert!(stale.is_some(), "the host is supposed to still hold the last one");
        second.arm_capture();
        for _ in 0..10 {
            second.settle_from_live();
        }
        assert_eq!(
            second.stage,
            IrStage::Sweep,
            "a capture the host refused advanced the wizard onto the previous IR"
        );

        // Arm again, this time with the host actually starting: the loop
        // closes — capture, result, capture, result.
        second.arm_capture();
        host_started(&mut second);
        second.settle_from_live();
        assert_eq!(second.stage, IrStage::Sweep);
        host_finished(&mut second);
        second.settle_from_live();
        assert_eq!(second.stage, IrStage::Result, "the loop did not come round");

        // And the reference pass, which produces nothing to show, disarms:
        // it must not leave the wizard primed to jump when the DUT pass's
        // result lands somewhere else entirely.
        second.record_another();
        second.arm_reference();
        host_started(&mut second);
        host_finished(&mut second);
        second.settle_from_live();
        assert_eq!(
            second.stage,
            IrStage::Sweep,
            "a loop-only reference pass moved the wizard on"
        );
    }

    /// **Every send level the wizard offers is one the engine will honour,
    /// and the row spans a range worth having.**
    ///
    /// The report: "the 2/4 levels section seems to be limited at around
    /// -20 dBFS, so that readout is just hitting a wall". There were two
    /// send levels, −18 and −12, and −18 dBFS of sine reads −21 dBFS RMS —
    /// so the meter showed a number that could not be raised. The send level
    /// is the capture's whole signal-to-noise budget (see
    /// `sweep::recovered_floor_db`), so this pins that the row still holds
    /// the two alignment levels AND reaches somewhere useful.
    #[test]
    fn the_send_levels_span_a_usable_range_and_keep_the_alignment_pair() {
        assert!(
            IR_SEND_CHOICES.windows(2).all(|w| w[0] < w[1]),
            "the send row is not in order"
        );
        for &v in IR_SEND_CHOICES.iter() {
            // `Engine::start_ir_tone` clamps to [−80, 0] and
            // `start_ir_capture` to [−60, 0]: every offered level has to
            // survive both unchanged, or the row lies about what it sends.
            assert!((-60.0..=0.0).contains(&v), "{v} dBFS will be clamped");
        }
        assert!(
            IR_SEND_CHOICES.contains(&-18.0) && IR_SEND_CHOICES.contains(&-12.0),
            "the two alignment levels the references use are gone"
        );
        let top = IR_SEND_CHOICES.last().copied().unwrap_or(-60.0);
        assert!(
            top >= -6.0,
            "the hottest send is {top} dBFS - still a wall for a box that \
             will not clip until -3"
        );
        // The default is still the recipe's first alignment level, and it is
        // one of the offered choices rather than a value nothing can select.
        let fresh = IrWizardState::default();
        assert!(IR_SEND_CHOICES.contains(&fresh.level_db));
    }


    // ── modality ────────────────────────────────────────────────────────────

    /// **A plugin window leaves the desk live; a question freezes it.**
    ///
    /// The classification is `Dialog::is_modal`, whose match has no
    /// wildcard — a NEW variant does not compile until somebody has said
    /// which kind it is, which is stricter than any test. What this pins is
    /// the classification itself, by name: every plugin window says
    /// non-modal, the questions say modal, and a reclassification has to
    /// come here and say so.
    #[test]
    fn plugin_windows_are_not_modal_and_questions_are() {
        let plugin: Vec<(&str, Dialog)> = vec![
            (
                "InsertFace",
                Dialog::InsertFace {
                    strip: 0,
                    slot: 0,
                    live: Box::default(),
                },
            ),
            (
                "KeysPatch",
                Dialog::KeysPatch {
                    slot: 0,
                    selected: 0,
                    params: [0.5; KEYS_PARAMS],
                },
            ),
            (
                "PatchPicker",
                Dialog::patch_picker(0, String::new(), false, true, Vec::new(), None),
            ),
            (
                "PatchEditor",
                Dialog::PatchEditor {
                    slot: 0,
                    patch: crate::ports::PatchEdit::default(),
                    group: 0,
                    name: String::new(),
                    note: String::new(),
                },
            ),
            ("PluginPicker", Dialog::plugin_picker(&[], None, false)),
            (
                "InsertKeySource",
                Dialog::InsertKeySource {
                    strip: 0,
                    slot: 0,
                    current: None,
                    channels: Vec::new(),
                },
            ),
            (
                "IrWizard",
                Dialog::IrWizard {
                    state: Box::default(),
                },
            ),
        ];
        for (name, d) in &plugin {
            assert!(!d.is_modal(), "{name} is a plugin window and must not freeze the desk");
        }
        let questions: Vec<(&str, Dialog)> = vec![
            ("About", Dialog::About),
            ("Cheated", Dialog::Cheated),
            ("CheatFun", Dialog::CheatFun),
            (
                "SaveChanges",
                Dialog::SaveChanges {
                    what: "quitting".to_owned(),
                    name: "Song".to_owned(),
                },
            ),
            (
                "CustomTuning",
                Dialog::custom_tuning(&Tuning::standard(), true),
            ),
        ];
        for (name, d) in &questions {
            assert!(d.is_modal(), "{name} is a question and must wait for its answer");
        }
    }

    /// **Every dialog that asked nothing is gone, and exactly one report is
    /// not.**
    ///
    /// The owner's ruling was to remove every system dialog except the ones
    /// that genuinely ask something — a folder chooser, a destructive
    /// confirmation, an unsaved-changes prompt. Four variants were the whole
    /// of the list that did not: `TakeSummary`, `Notice`, `MidiError` and
    /// `NoMidiInput`, each a child viewport with one OK button in it that
    /// froze the desk until it was found and clicked. What they said now goes
    /// through `App::notify` and `App::notify_raise`.
    ///
    /// **Read out of the source, which is the only way to assert an absence.**
    /// A test cannot name a variant that does not exist without failing to
    /// compile, so it cannot say "this is gone" by mentioning it — the string
    /// is what is left. The `Dialog::` prefix is joined on deliberately:
    /// "Notice" alone appears in half a dozen unrelated comments, and a test
    /// that fails on prose is a test people delete. Joined at RUN time rather
    /// than written out, because this file includes ITSELF — a needle spelled
    /// in full here would find its own literal and fail where nothing is
    /// wrong.
    ///
    /// `LearnResult` is the deliberate exception and is asserted PRESENT, so
    /// that "we kept one" stays a decision somebody made rather than something
    /// that was missed. Its own doc carries the reasoning.
    #[test]
    fn every_ok_only_dialog_is_gone_except_the_one_report() {
        const SRC: &str = include_str!("dialogs.rs");
        let named = |v: &str| SRC.contains(&format!("Dialog::{v}"));
        for retired in ["TakeSummary", "Notice", "MidiError", "NoMidiInput"] {
            assert!(
                !named(retired),
                "Dialog::{retired} is back; a message with one OK button is a notification"
            );
        }
        // And the scan is a real one: run against variants that ARE here it
        // has to find them, or this test would pass on a typo.
        assert!(named("LearnResult") && named("Confirm") && named("SaveChanges"));
        // The report that stayed still works, and is still modal: it is a
        // thing to read, and reading it is what the box is for.
        let report = Dialog::LearnResult {
            title: "Chord Learning",
            message: "Learned. This voicing now reads Cmaj7.".to_owned(),
        };
        assert!(report.is_modal());
    }

    /// **The classification is the only switch.** A plugin dialog goes
    /// through `show` and comes out borderless, non-modal and wearing the
    /// bar; a question comes out decorated and modal. Both facts come from
    /// `is_modal` alone, so an arm of the big match cannot get one of them
    /// wrong on its own — asserted on the inline path, where the layer's
    /// memory says what was drawn.
    #[test]
    fn a_plugin_dialog_wears_the_bar_and_a_question_does_not() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let placement = Placement {
            at: None,
            native_panel_up: false,
            raise: false,
            parent: None,
            monitor: None,
            caps: crate::host::Caps::PLUGIN,
            chrome: None,
        };
        let face = Color32::from_rgb(0xE3, 0xD9, 0xBD);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
        // The theme key inside the Keys panel: carried back as the action,
        // which is what makes T cycle the theme from a plugin window.
        let mut dialog = Some(Dialog::KeysPatch {
            slot: 0,
            selected: 0,
            params: [0.5; KEYS_PARAMS],
        });
        let mut action = None;
        for events in [
            Vec::new(),
            vec![egui::Event::Key {
                key: egui::Key::T,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    action = show(ctx, &mut dialog, face, placement);
                },
            );
        }
        assert!(
            matches!(action, Some(DialogAction::CycleTheme)),
            "T inside the Keys panel did not come back as a theme cycle"
        );
        assert!(dialog.is_some(), "the theme key closed the panel");
        // And the layer is remembered where it was drawn, which is what the
        // app's raw-pointer handlers read to keep a press on the panel off
        // the desk underneath.
        let rect = crate::shell::inline_rect(&ctx, DIALOG_ID).expect("the panel left no rect");
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                events: vec![egui::Event::PointerMoved(rect.center())],
                ..Default::default()
            },
            |ctx| {
                assert!(pointer_over_dialog(ctx), "a pointer on the panel reads as off it");
                assert!(dialog_drawn_inline(ctx));
                let _ = show(ctx, &mut dialog, face, placement);
            },
        );
    }
}

