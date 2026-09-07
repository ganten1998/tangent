//! Keyboard shortcuts, and the card that lists them.
//!
//! Two things make this more than a match on `Key`.
//!
//! The bindings live in ONE table, and the help card is rendered FROM that
//! table. A shortcut that works but is not listed, or is listed but does not
//! work, is worse than no shortcut at all, and the only way to guarantee
//! neither happens is to have one source. Adding a binding is one line and the
//! card updates itself.
//!
//! And the card is drawn IN THE CANVAS rather than in a child window. Every
//! other surface in this app is an OS viewport, which a VST3 editor cannot
//! create — so this is the first piece of UI that already works in both hosts,
//! and the shape the rest will move to (docs/PLUGIN-PLAN.md).

use egui::{Align2, Color32, FontId, Key, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// What a keypress asks the app to do. Deliberately not the same enum as
/// `MenuAction`: these are verbs a key can mean, and several have no menu row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyAction {
    ToggleHelp,
    ToggleKeytoggle,
    ClearNotes,
    ToggleFretboard,
    ToggleCameraPane,
    CycleClef,
    ToggleNoteNames,
    /// One of the theory band's elements, by its number key.
    ToggleTheoryElement(usize),
    /// Wear the next theme. See `theme::factory` and the user folder.
    CycleTheme,
    ToggleDarkMode,
    ToggleDetection,
    ToggleBorderless,
    CycleFont,
    ShowAbout,
    ShowSupporterKey,
    TeachChordName,
    CorrectChordName,
    ManageTaughtChords,
    ToggleChordLearning,
    ToggleNotePreference,
    /// §5: START a take.
    ///
    /// Separate from stopping, which is the owner's call and the right one: one
    /// key that does both means the key that begins a performance is the same
    /// one that ends it, and the difference between the two is a state you
    /// cannot see from the bench. Enter starts, Space stops.
    ///
    /// The first binding here that is not always live — see [`Gates`].
    StartRecording,
    /// §5: STOP a take. Space, because that is what every transport in every
    /// DAW the user already owns is bound to.
    StopRecording,
    /// Raise the recorder band, or put it away.
    ///
    /// **Menu-only: no key.** `V` was it and retired as unused clutter — see
    /// the note beside `W` in `BINDINGS`, which the two letters share.
    /// `the_menu_only_actions_are_in_no_row_of_the_table` holds this true.
    ToggleRecorder,
    /// Swap the piano and the bands under it for the mixer, and back.
    ToggleMixer,
    /// The drumpad, floating over whichever view is open.
    ///
    /// A POPUP, not a Tab stop: the pads are an instrument you raise over
    /// the surface you are working on, exactly as the MIDI editor rises
    /// over the arrangement. Gated on the band being open, because the
    /// engine lives and dies with the band and a drumpad with no engine is
    /// sixteen buttons that do nothing.
    ///
    /// **Menu-only: no key**, on the same grounds as [`KeyAction::ToggleRecorder`].
    ToggleDrumpad,
    /// Fill the screen, and come back.
    ///
    /// Gated on [`Gates::window_sizing`] for the same reason every other
    /// window action is: a plugin editor lives inside somebody's DAW and has no
    /// business taking their display.
    ToggleFullscreen,
    /// Menus and dialogs inline, for a screen reader — the same switch as
    /// the menu row, reachable without a mouse.
    ///
    /// The keyboard door matters more here than anywhere else on this card:
    /// the menu row it mirrors opens from a RIGHT-CLICK, and the person the
    /// switch exists for is exactly the person least able to aim one.
    ///
    /// Gated on [`Gates::window_sizing`] because the handler follows the
    /// menu row's semantics, and those hand back `child_windows` when the
    /// mode goes off — a grant a plugin editor must never receive.
    ToggleReaderMode,
    TransposeUp,
    TransposeDown,
    CloseHelp,
}

/// What the app is currently OFFERING a key.
///
/// A struct passed into `pressed` and `draw_help` rather than an `if` around
/// the call in `app.rs`, and that is the whole point of it. This module's one
/// rule is that the bindings and the card come from a single table; filtering
/// at the call site puts the binding on one side of a condition and its
/// description on the other, and the two drift apart the first time somebody
/// edits one of them. Going through here, a key that cannot fire is also a key
/// the card does not mention, by construction.
///
/// `Default` is "nothing extra is open", which is the conservative direction: a
/// gate that nobody remembered to wire up costs a shortcut rather than firing
/// one nobody asked for.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Gates {
    /// The Recorder band is showing.
    ///
    /// Space toggles recording, and it is gated on the band being OPEN rather
    /// than on the host merely being able to record: a user who has never
    /// opened the recorder must not start a take by pressing the widest key on
    /// the keyboard while reaching for something else. Opening the band is the
    /// consent, and it is one right-click away.
    ///
    /// Note for whoever wires this up: the band owns a take-name text field. A
    /// space typed into it must not also hit the transport, so `app.rs` has to
    /// keep `pressed` away from a frame where a text field holds keyboard
    /// focus — the same reasoning that already keeps it away from dialogs and
    /// the open menu.
    pub recorder_shown: bool,
    /// This host can record at all — `Caps::capture_devices`.
    ///
    /// Gates the key that OPENS the band, which obviously cannot be gated on
    /// the band being open. False in a plugin and in a Minimal build, where
    /// there is no recorder to show and the menu category is absent too.
    pub recorder_available: bool,
    /// This host owns its window — `Caps::window_sizing`.
    ///
    /// Gates fullscreen. A plugin editor lives inside somebody's DAW and has
    /// no business taking their display.
    pub window_sizing: bool,
    /// This session is being read out loud — `TangentApp::reader_mode()`,
    /// whoever decided so, the saved setting or this session's detection.
    ///
    /// Not a gate on any binding: it changes what the card ADVERTISES. In
    /// reader mode egui takes Tab to walk the controls before this module's
    /// table ever sees it, so the Tab row is a lie there and F6 is the view
    /// cycle's real key. Carried in `Gates` rather than read at the call
    /// site so the one struct keeps feeding both `pressed` and the card —
    /// the module's cannot-disagree invariant.
    pub reader_mode: bool,
}

/// Whether a binding is live right now.
///
/// The single gate. `pressed` and `draw_help` both come through here, so the
/// card cannot advertise a dead key and a live key cannot go unlisted.
fn available(action: KeyAction, gates: Gates) -> bool {
    match action {
        KeyAction::StartRecording | KeyAction::StopRecording | KeyAction::ToggleDrumpad => {
            gates.recorder_shown
        }
        KeyAction::ToggleRecorder => gates.recorder_available,
        KeyAction::ToggleFullscreen => gates.window_sizing,
        // The same gate as the menu row it mirrors — critical, because the
        // handler grants `child_windows` back when the mode turns off, and a
        // plugin editor must never receive that grant.
        KeyAction::ToggleReaderMode => gates.window_sizing,
        _ => true,
    }
}

/// (key, label, what it does, whether it appears on the card).
/// The card is generated from this, so the two cannot disagree.
/// `H` as well as `F1`, because macOS takes F1 for screen brightness unless
/// "use F1, F2 etc. as standard function keys" is on, and it is off by default.
/// An app never sees the keypress at all, so a help key that only answers to F1
/// is a help key most Mac users cannot press.
const BINDINGS: &[(Key, &str, KeyAction, bool)] = &[
    (Key::H, "H", KeyAction::ToggleHelp, true),
    (Key::F1, "F1", KeyAction::ToggleHelp, false),
    (Key::K, "K", KeyAction::ToggleKeytoggle, true),
    (Key::R, "R", KeyAction::ClearNotes, true),
    (Key::G, "G", KeyAction::ToggleFretboard, true),
    // **No `W` here, and no `V` anywhere.**
    //
    // `W` toggled a second camera pane beside the theory diagrams; the camera
    // has one home now, the full-height preview at the recorder band's
    // top-left, which is the inset a recording carries. That pane's code is
    // still here and switched off — it comes out after the beta — but nothing
    // reaches it. The letter did NOT stay free, though, and the note that said
    // it did outlived the truth: `arrangement_input` and `midi_editor_input`
    // both read a bare `W` as "fit", and both cards list it. It is a per-view
    // key, which is why it is not in this global table — and why binding it
    // here would quietly take "fit" away from two views.
    //
    // `V` was to be the recorder VIEW and never shipped, retired with the
    // drumpad's key as unused clutter: both bands are opened from the menu,
    // and a letter that duplicates a menu item nobody presses is a row on the
    // help card that teaches nothing. `KeyAction::ToggleRecorder` and
    // `KeyAction::ToggleDrumpad` therefore appear in no row of this table and
    // are reached only through `menu.rs`. `V` is genuinely free; `W` is not.
    // **U I O, in that order, are the sheet music.** Not one of them is the
    // first letter of what it does, and there was no letter left that was:
    // `S` is the supporter key, `C` the chord detector, `N` the chord namer.
    // Three keys next to each other under one hand beat three unrelated
    // letters that each ALMOST stand for something.
    (Key::U, "U", KeyAction::ToggleNoteNames, true),
    (Key::I, "I", KeyAction::CycleClef, true),
    // **The number row is the theory band.** One key per element, in the order
    // `View::ALL` lists them, and pressing one removes that element or puts it
    // back at the RIGHT-HAND END — so the same four keys both choose what is
    // showing and arrange it. Listed as a single row rather than four, because
    // four rows saying "circle of fifths", "Tonnetz"... is a help card that
    // teaches the names of things instead of the mechanism.
    (Key::Num1, "1-4", KeyAction::ToggleTheoryElement(1), true),
    // 2, 3 and 4 are unlisted ALIASES of the row above in the help card's
    // sense: they are four keys doing one job, and four rows naming four
    // diagrams would teach the names instead of the mechanism.
    (Key::Num2, "2", KeyAction::ToggleTheoryElement(2), false),
    (Key::Num3, "3", KeyAction::ToggleTheoryElement(3), false),
    (Key::Num4, "4", KeyAction::ToggleTheoryElement(4), false),
    // **T is THEMES.** It used to turn every theory diagram on at once,
    // which is not a cycle: the second press did nothing, and the owner said
    // so. The 1-4 keys already toggle the diagrams one at a time, so the
    // letter went to the thing people actually flick through - sixteen
    // themes plus whatever is in the user folder.
    (Key::T, "T", KeyAction::CycleTheme, true),
    (Key::C, "C", KeyAction::ToggleDetection, true),
    (Key::D, "D", KeyAction::ToggleDarkMode, true),
    (Key::B, "B", KeyAction::ToggleBorderless, true),
    (Key::F, "F", KeyAction::CycleFont, true),
    (Key::A, "A", KeyAction::ShowAbout, true),
    (Key::S, "S", KeyAction::ShowSupporterKey, true),
    // The teach block. `N` for name is the one that matters: naming a voicing
    // is what this app is FOR, and reaching it took a right-click, a scan down
    // a long menu, a click, and then a click into the field before a single
    // letter could be typed.
    (Key::N, "N", KeyAction::TeachChordName, true),
    (Key::E, "E", KeyAction::CorrectChordName, true),
    (Key::M, "M", KeyAction::ManageTaughtChords, true),
    (Key::L, "L", KeyAction::ToggleChordLearning, true),
    (Key::P, "P", KeyAction::ToggleNotePreference, true),
    // Fullscreen. `Z` for zoom, which is what the green button does and what
    // macOS calls it; `F` is long since taken by the font cycle, and rebinding
    // that to fill the screen would be a nasty surprise mid-practice.
    //
    // `F11` is the same key everywhere else in computing and is listed as an
    // unlisted alias, exactly as `F1` is for help — on macOS an app never sees
    // F11 unless the user has turned on "use function keys as standard", so a
    // fullscreen bound only to it would be one most Mac users cannot press.
    (Key::Z, "Z", KeyAction::ToggleFullscreen, true),
    (Key::F11, "F11", KeyAction::ToggleFullscreen, false),
    // **J is the reader-mode switch, and it was free.** No global row took
    // it and neither view handler reads a bare `J` — checked the same way
    // `W`'s note above demands. It sits beside the other window switches on
    // the card because that is what it is: where the menus and dialogs
    // live. The key matters more than most on this card — the menu row it
    // mirrors opens from a right-click, and the person this switch exists
    // for is exactly the person least able to aim one.
    (Key::J, "J", KeyAction::ToggleReaderMode, true),
    // The arrows move the notes you are HOLDING, which is a thing you do while
    // still holding them — so they had to be keys reachable without letting
    // go, and the arrow keys are the only ones that are.
    (Key::ArrowUp, "Up", KeyAction::TransposeUp, true),
    (Key::ArrowDown, "Down", KeyAction::TransposeDown, true),
    // **Tab, and it was free.** Nothing in this app cycled a view before the
    // mixer, because every surface was a band you turn on; the mixer is a
    // place you go, and Tab is what a program with more than one screen uses
    // to go there.
    (Key::Tab, "Tab", KeyAction::ToggleMixer, true),
    // **F6 is the same key, and it is the one that still works in
    // accessibility mode.** There, Tab walks the controls — that is what a
    // screen-reader user presses it for and egui takes it before this table
    // ever sees it — so the view cycle needs a key of its own. F6 is what
    // every other program with panes uses for exactly this, which makes it
    // the one alias somebody might already know. LISTED, unlike F11, and
    // the difference is the audience: F11 duplicates a key the card already
    // tells the truth about, while in reader mode the Tab row above is a
    // lie — Tab walks the controls there — and an unlisted F6 left the view
    // cycle with no key the card was honest about. Its row wears its own
    // words rather than Tab's; see `card_rows`.
    (Key::F6, "F6", KeyAction::ToggleMixer, true),
    // §5: one button, not arm-then-record, and Space because that is what every
    // transport in every DAW the user already owns is bound to.
    //
    // LAST of the listed rows on purpose. These are the only GATED ones, so
    // they are the only ones that can appear and disappear; at the end of the
    // table they take the end of the card with them, instead of reflowing
    // every row below them each time the band opens. "Space" is also not a
    // single letter, which is why the key column is measured rather than
    // assumed — see `key_col_w`.
    (Key::Enter, "Enter", KeyAction::StartRecording, true),
    (Key::Space, "Space", KeyAction::StopRecording, true),
    // Escape only closes the card; it is not worth a row of its own.
    (Key::Escape, "Esc", KeyAction::CloseHelp, false),
];

impl KeyAction {
    /// The action with any per-key payload flattened away.
    ///
    /// Two bindings are the same CONTROL when their families match, which is
    /// how four number keys can share one row on the help card without the
    /// "every bound key is described" rule losing its teeth for anything else.
    fn family(self) -> KeyAction {
        match self {
            KeyAction::ToggleTheoryElement(_) => KeyAction::ToggleTheoryElement(1),
            other => other,
        }
    }
}

/// One line of help text per binding, written where the binding is defined so
/// they cannot drift apart.
fn describe(a: KeyAction) -> &'static str {
    match a {
        KeyAction::ToggleHelp => "hold for this card",
        KeyAction::ToggleKeytoggle => "keytoggle: click the piano or the neck to place notes",
        KeyAction::ClearNotes => "clear every note you placed",
        KeyAction::ToggleFretboard => "guitar view",
        KeyAction::ToggleMixer => "cycle theory / arrange / mixer",
        KeyAction::ToggleCameraPane => "camera",
        KeyAction::CycleClef => "clef",
        KeyAction::ToggleNoteNames => "note names",
        // The row reads `1-4`, not `1`, because the four keys are one control.
        KeyAction::ToggleTheoryElement(_) => "theory panel on/off (1-4)",
        KeyAction::CycleTheme => "cycle themes",
        KeyAction::ToggleDarkMode => "dark mode",
        KeyAction::ToggleDetection => "chord detection",
        KeyAction::ToggleBorderless => "window border",
        KeyAction::CycleFont => "cycle the typeface",
        KeyAction::ShowAbout => "about",
        KeyAction::ShowSupporterKey => "enter a supporter key",
        KeyAction::TeachChordName => "name the chord you are holding",
        KeyAction::CorrectChordName => "correct what it reads",
        KeyAction::ManageTaughtChords => "chords you have taught",
        KeyAction::ToggleChordLearning => "chord learning",
        KeyAction::ToggleNotePreference => "sharps or flats",
        KeyAction::ToggleDrumpad => "the drumpad",
        KeyAction::StartRecording => "record the arrangement",
        KeyAction::StopRecording => "play / stop (to cursor)",
        KeyAction::ToggleRecorder => "the recorder",
        KeyAction::ToggleFullscreen => "fill the screen",
        KeyAction::ToggleReaderMode => "screen reader mode (menus inline)",
        KeyAction::TransposeUp => "transpose up a semitone",
        KeyAction::TransposeDown => "transpose down a semitone",
        KeyAction::CloseHelp => "close this card",
    }
}

/// The rows the card shows, in table order.
///
/// Shared with `pressed` through `available`, which is what makes "listed but
/// dead" and "live but unlisted" the same impossible thing rather than two
/// separate bugs to remember.
/// Which surface the card describes. **The keys change with the view**: a
/// key the arrangement steals (S, G, M) is described by what it does THERE,
/// and each view appends the gestures that have no key at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HelpView {
    Piano,
    Mixer,
    Arrange,
    MidiEdit,
}

/// Whether a binding belongs on `view`'s card — the owner's rule: each
/// view's H lists only the keys that DO something on that page. The piano
/// is the app's home and keeps every binding; the other surfaces keep the
/// window's own switches, the transport, the popups, and whatever they
/// stole for themselves. A key that only changes something the view is
/// covering — keytoggle, the staff, the theory band, the teach block —
/// is not on their cards, because pressing it there shows you nothing.
fn on_card(view: HelpView, a: KeyAction) -> bool {
    use KeyAction as K;
    if matches!(view, HelpView::Piano) {
        return true;
    }
    let global = matches!(
        a,
        K::ToggleHelp
            | K::ToggleDarkMode
            | K::ToggleBorderless
            | K::CycleFont
            | K::ShowAbout
            | K::ToggleFullscreen
            | K::ToggleReaderMode
            | K::ToggleMixer
            | K::StartRecording
            | K::StopRecording
    );
    global
        || match view {
            HelpView::Piano => true,
            // The supporter card rises over the mixer exactly as it does
            // over the piano; everything else on this card is gestures.
            // The mixer keeps the split key and the metronome — see the
            // arrangement's row below, which now shares the second.
            HelpView::Mixer => matches!(a, K::ShowSupporterKey | K::ManageTaughtChords),
            // The three the arrangement stole, wearing their meanings HERE.
            HelpView::Arrange => matches!(
                a,
                K::ShowSupporterKey | K::ToggleFretboard | K::ManageTaughtChords
            ),
            // The editor's magnet and its triplet grid.
            HelpView::MidiEdit => matches!(a, K::ToggleFretboard | K::ToggleNotePreference),
        }
}

/// The binds that mean the same thing on EVERY surface — the card's
/// GENERAL section. Everything else is the view's own.
fn is_general(a: KeyAction) -> bool {
    use KeyAction as K;
    matches!(
        a,
        K::ToggleHelp
            | K::ToggleDarkMode
            | K::ToggleBorderless
            | K::CycleFont
            | K::ShowAbout
            | K::ToggleFullscreen
            // Reader mode moves the menus and dialogs of EVERY surface, so
            // its key belongs in the section that speaks for all of them.
            | K::ToggleReaderMode
            | K::ToggleMixer
            | K::StartRecording
            | K::StopRecording
    )
}

/// A section heading row: an empty key column, drawn bold by the card.
fn heading(word: &'static str) -> (&'static str, &'static str) {
    ("", word)
}

fn card_rows(gates: Gates, view: HelpView) -> Vec<(&'static str, &'static str)> {
    let describe_on = |&(_, label, action, _): &(
        egui::Key,
        &'static str,
        KeyAction,
        bool,
    )|
     -> (&'static str, &'static str) {
            // **F6 wears its own words, not Tab's.** The two keys share
            // `ToggleMixer`, but this row exists for one situation: in
            // reader mode egui takes Tab to walk the controls, so the Tab
            // row above it is a lie there and F6 is the key that still
            // cycles the views. Repeating "cycle theory / arrange / mixer"
            // would document the alias and explain nothing.
            if label == "F6" {
                return (label, "the view cycle in reader mode - Tab walks controls there");
            }
            let desc = match (view, action) {
                (HelpView::Arrange, KeyAction::ShowSupporterKey) => {
                    "split the clip at the playhead"
                }
                (HelpView::Arrange, KeyAction::ToggleFretboard) => "snap to grid (the magnet)",
                (HelpView::Arrange | HelpView::Mixer, KeyAction::ManageTaughtChords) => {
                    "metronome on/off"
                }
                (HelpView::MidiEdit, KeyAction::ToggleFretboard) => {
                    "snap to grid (the editor's magnet)"
                }
                (HelpView::MidiEdit, KeyAction::ToggleNotePreference) => "triplet grid on/off",
                _ => describe(action),
            };
            (label, desc)
        };
    // GENERAL first — the binds that work everywhere — then the view's
    // own panel, delineated, the owner's rule.
    let mut rows: Vec<(&'static str, &'static str)> = vec![heading("GENERAL")];
    rows.extend(
        BINDINGS
            .iter()
            .filter(|&&(key, _, a, shown)| {
                // **`Z` is not fullscreen inside the MIDI editor.** It is
                // general, so it was listed on every card as "fullscreen" —
                // and with the pointer over the editor `midi_editor_input`
                // takes bare `Z` for `me_fit_clip` while the fullscreen
                // action is swallowed. The editor's own section below says
                // what it really does; listing it twice, once wrongly, is
                // the failure this file's header names.
                if matches!(view, HelpView::MidiEdit)
                    && matches!(a, KeyAction::ToggleFullscreen)
                {
                    return false;
                }
                // **In reader mode the Tab row is a lie**, and this card is
                // being read to exactly the audience with the least means of
                // discovering that: egui takes Tab to walk the controls
                // before the table ever sees it. The row comes off; F6's
                // row below carries the view cycle, in its own words.
                if gates.reader_mode && key == Key::Tab {
                    return false;
                }
                shown && available(a, gates) && is_general(a)
            })
            .map(describe_on),
    );
    // **Undo and redo, which were on no card at all.** They are chords, so
    // they cannot live in `BINDINGS` — that table is single keys — and being
    // in neither place meant the two most-pressed shortcuts in any editor
    // went undocumented on every surface.
    rows.push(("C-S", "save the project"));
    rows.push(("C-R", "record the arrangement - a take may be rolling"));
    rows.push(("C-Z", "undo"));
    rows.push(("C-S-Z", "redo"));
    rows.push(heading(match view {
        HelpView::Piano => "PIANO PANEL",
        HelpView::Mixer => "MIXER PANEL",
        HelpView::Arrange => "ARRANGE PANEL",
        HelpView::MidiEdit => "MIDI EDITOR PANEL",
    }));
    rows.extend(
        BINDINGS
            .iter()
            .filter(|&&(key, _, a, shown)| {
                // The same reader-mode drop as the GENERAL section above, so
                // the rule survives Tab's action ever moving off the general
                // list: no card advertises Tab while egui is eating it.
                if gates.reader_mode && key == Key::Tab {
                    return false;
                }
                shown && available(a, gates) && !is_general(a) && on_card(view, a)
            })
            .map(describe_on),
    );
    match view {
        // The view keys, each card naming only the OTHER surfaces.
        HelpView::Piano => rows.extend([
            ("S-2", "shift+2: the arrangement"),
            ("S-3", "shift+3: the mixer"),
        ]),
        HelpView::Mixer => rows.extend([
            ("R-CLK", "channel: the track menu · loaded insert: delete / replace"),
            ("2-CLK", "rename a channel · reset a fader"),
            ("S-ARM", "shift+arm: this lane alone - every other arm stands down"),
            ("A-CLK", "alt-click an insert: empty the bay"),
            ("S-CLK", "shift-click an insert: bypass it - the plugin stays"),
            ("DEL", "remove the selected channel"),
            ("C-T", "ctrl+T: a new track"),
            ("C-F", "ctrl+F: group the selection into a folder"),
            ("C-C/V/X", "copy the track · paste onto an empty slot · cut"),
            ("FLDR", "the folder glyph: hide or show its tracks"),
            ("NOTE", "the \u{266a} at a track's foot: its MIDI to the theory view"),
            ("S-1", "shift+1: the theory view"),
            ("S-2", "shift+2: the arrangement"),
        ]),
        HelpView::Arrange => rows.extend([
            ("DRAG", "move a clip · edges trim · corners fade"),
            // **Cmd, not shift.** The owner moved the creation gesture onto
            // the command key and unbound shift from it entirely; the card is
            // the only place in the app that says what the gesture IS, so a
            // row left behind here would be the app teaching a chord it no
            // longer answers. Spelled "cmd-drag" like the editor's own C-DRG
            // row, which is the same finger.
            ("C-DRG", "cmd-drag a lane: draw a MIDI item"),
            // "MIDI or audio", because it said "a MIDI clip" long after
            // audio stretch shipped — the row understated the feature while
            // C-G below overstated its own, in the opposite direction.
            ("S-EDG", "shift-drag an edge: stretch the clip, MIDI or audio"),
            // **The automation and tempo lanes, which were on no card at
            // all** — a whole gesture set discoverable only by clicking.
            // One rule each way, shift puts a node down and alt takes one
            // away, on both kinds of lane — which is why these rows say "a
            // lane" and "a node" rather than naming the two surfaces. The
            // ramp toggle is the one thing a tempo point has that an
            // automation node does not, so it alone gets a row of its own.
            ("S-CLK", "shift-click a lane: drop a node · a node: add it · an insert bay: bypass it"),
            ("S-PT", "shift-click a tempo point: ramp or step"),
            ("DIAL", "an automation row's dial: the lane's base · 2-click resets"),
            ("2-CLK", "double-click a MIDI item: open its editor"),
            ("DEL", "delete the selected clips · nodes · track"),
            ("A-CLK", "alt-click: empty an insert bay · remove a node"),
            ("R-CLK", "empty room: the app menu · header: the track menu · loaded insert: delete / replace"),
            ("WHEEL", "zoom at the playhead - click a bar, wheel zooms there"),
            ("A-WHL", "alt: move the viewport"),
            ("A-DRG", "alt-drag empty room: move the viewport"),
            ("CA-WHL", "ctrl+alt: grid finer or coarser"),
            ("NAV", "corner box: drag left/right scrolls · up/down zooms"),
            ("-/=", "zoom out / in, in steps, around the playhead"),
            ("W", "fit the biggest clip - empty: the shipped zoom"),
            ("S-ARM", "shift+arm: this lane alone - every other arm stands down"),
            ("C-T", "ctrl+T: a new track"),
            ("C-C/V/X", "copy · paste · cut the selected clips - or nodes"),
            ("C-F", "group the selection into a folder"),
            // "MIDI clips", because that is the truth: `arr_glue_selection`
            // filters to MIDI and refuses everything else — welding audio
            // means rendering, a different operation with a different cost.
            ("C-G", "glue the selected MIDI clips into one"),
            // C-S-N, not C-N: the handler requires shift, and only audio
            // comes up — a note's velocity is a performance, not a level.
            ("C-S-N", "normalise the selected audio clips"),
            ("C-M", "the master row on/off"),
            ("ESC", "drop the time selection"),
            ("S-1", "shift+1: the theory view"),
            ("S-3", "shift+3: the mixer"),
        ]),
        HelpView::MidiEdit => rows.extend([
            ("DRAG", "draw a note · body moves the selection · edge resizes"),
            ("S-EDG", "shift-drag a note's edge: stretch the selection to the grid"),
            ("S-CLK", "shift-click: add to the selection"),
            ("C-DRG", "cmd-drag empty room: rubber-band select"),
            ("R-CLK", "erase a note · empty room: the app menu"),
            ("DEL", "delete the selection"),
            ("Q", "quantise the selection to the grid"),
            ("-/=", "zoom out / in, in steps, around the playhead"),
            ("W", "fit the clip"),
            ("C-A", "select every note"),
            ("C-D", "duplicate one grid step later"),
            ("ARROWS", "nudge: left/right a step · up/down a semitone"),
            ("Z", "fit the clip back in the window"),
            ("N", "draw mode"),
            ("O", "swing grid on/off"),
            ("C-G", "glue the selected notes"),
            ("C-C/V", "copy · paste notes"),
            ("ESC", "close the editor"),
            ("WHEEL", "zoom at the playhead - click a bar, wheel zooms there"),
            ("A-WHL", "alt: move the viewport (the editor's own)"),
            ("C-WHL", "ctrl: walk the piano roll up and down"),
            ("CA-WHL", "ctrl+alt: grid finer or coarser"),
        ]),
    }
    rows
}

/// Which action a frame's keypresses ask for, if any.
///
/// Returns at most one, because two shortcuts firing on one frame is never
/// what anybody meant. Modifiers are REQUIRED to be absent: Cmd-R and Ctrl-R
/// belong to the OS and the browser habit, and swallowing them would be rude.
///
/// `gates` says which bindings are live; it is a parameter rather than a filter
/// in `app.rs` so the card cannot disagree with the keyboard. See [`Gates`].
/// Is this action bound to a key at all?
///
/// For `menu.rs`, which deleted a dozen rows on the grounds that a key already
/// does the job. That justification is only true while the binding exists, and
/// a binding is one line in a table that nothing else would miss — so the menu
/// asserts the key is here, and this is how it asks.
///
/// Matched by `family`, so `ToggleTheoryElement(1)` answers for all four
/// number keys.
pub fn binding_for_test(action: KeyAction) -> bool {
    BINDINGS.iter().any(|(_, _, a, _)| a.family() == action.family())
}

pub fn pressed(ctx: &egui::Context, gates: Gates) -> Option<KeyAction> {
    ctx.input(|i| {
        if i.modifiers.any() {
            return None;
        }
        BINDINGS
            .iter()
            .filter(|&&(_, _, a, _)| {
                !matches!(a, KeyAction::ToggleHelp | KeyAction::CloseHelp) && available(a, gates)
            })
            .find(|(key, ..)| i.key_pressed(*key))
            .map(|&(_, _, action, _)| action)
    })
}

/// How far the card is out, 0.0 (hidden) to 1.0 (fully down).
///
/// The help key is HELD rather than toggled: press and read, let go and it is
/// gone, with no state to get stuck in and nothing to dismiss. That only works
/// if it moves, though — a card that blinks in and out looks like a glitch,
/// while one that slides looks like it was always there waiting.
///
/// `animate_bool_with_time` does the easing and asks for the repaints, but the
/// app's own cadence is 50ms (20fps), which would make this visibly steppy, so
/// an animation in progress asks for the next frame immediately.
/// How far the shortcut card is down, 0 to 1.
///
/// `allowed` is not a nicety. The key is read from `InputState::keys_down`,
/// which is built before any widget runs, so keyboard focus does not filter
/// it — typing an `h` into the supporter-key field slid the card down over
/// the app while the letter also went into the box. The desktop escaped that
/// only because a dialog there is a separate OS window and the unfocused root
/// gets `WindowFocused(false)`, which clears `keys_down`.
///
/// Still animates when not allowed, so a card that is already down slides back
/// up rather than vanishing.
pub fn help_progress(ctx: &egui::Context, allowed: bool) -> f32 {
    let held = allowed
        && ctx.input(|i| {
            !i.modifiers.any()
                && BINDINGS
                    .iter()
                    .filter(|(_, _, a, _)| *a == KeyAction::ToggleHelp)
                    .any(|(key, ..)| i.key_down(*key))
        });
    let t = ctx.animate_bool_with_time(egui::Id::new("tangent-help-card"), held, 0.16);
    if t > 0.0 && t < 1.0 {
        ctx.request_repaint();
    }
    t
}

/// Where the card goes, how big its text is, and how many columns it uses.
///
/// Split out so it can be TESTED, because the bug here is invisible in code and
/// glaring on screen: sized from the window height alone, the card grew taller
/// than the app the moment the list did, and the first and last shortcuts were
/// cut off by the window edges.
///
/// Columns matter as much as type size. This window is very WIDE and very SHORT
/// (1300x332 usually, 650x166 at the smallest with everything on), so ten
/// shortcuts in one column do not fit at any legible size while two columns fit
/// comfortably. Using the shape of the window beats shrinking the type until
/// nobody can read it.
/// Width of the key column, which is no longer a constant.
///
/// It was a flat `size * 3.2` while every listed key was one character wide.
/// Space is the first that is not: "Space" set in Courier at ~0.62 em per glyph
/// is 3.1 em, so it would have run into its own description with a fifth of a
/// character to spare — which on screen reads as a rendering fault rather than
/// a layout one. Measured from the widest label instead, with the old value
/// kept as a floor so a card of single letters is pixel-identical to before.
///
/// One function, called by both `layout` and `draw_help`, because a card sized
/// on one number and painted with another is the bug this replaces.
fn key_col_w(size: f32, widest_key: usize) -> f32 {
    (size * 3.2).max(size * (widest_key as f32 * 0.62 + 1.0))
}

/// One column count's font size and card size: `(size, width, height)`.
///
/// Split out of [`layout`] so the test that checks "we chose the biggest
/// arrangement that fits" can measure the alternatives with the SAME
/// arithmetic. A second copy in the test would agree with this one only by
/// accident, and would go on agreeing after this one changed.
fn measure_card(
    rect: Rect,
    rows: usize,
    widest: usize,
    widest_key: usize,
    cols: usize,
) -> (f32, f32, f32) {
    let per_col = rows.max(1).div_ceil(cols);
    let by_height = rect.height() * 0.92 / (per_col as f32 * 1.75 + 3.36);
    // The floor is 6.0 rather than 8.0, and the gap between them is an
    // anti-clipping fallback rather than slack: 22 rows at 650x125 want three
    // columns of eight, which needs 125.4 points of a 120-point card, and there
    // is no legible four-column layout at 650 wide to escape into. Small and
    // complete beats crisp and cut off.
    let size = by_height.min(rect.height() * 0.075).clamp(6.0, 20.0);
    let pad = size * 1.4;
    let col_w = key_col_w(size, widest_key) + widest as f32 * size * 0.62;
    let w = col_w * cols as f32 + pad * 2.0 + pad * (cols - 1) as f32;
    let h = per_col as f32 * size * 1.75 + pad * 2.4;
    (size, w, h)
}

fn layout(rect: Rect, rows: usize, widest: usize, widest_key: usize) -> (Rect, f32, usize) {
    let rows = rows.max(1);
    // The BIGGEST text that fits, not the first arrangement that does.
    //
    // This used to return on the first fitting column count, which was fine
    // while the font floor was 8.0: a one-column layout that did not fit simply
    // failed the height test and the loop moved on. Lowering the floor to 6.0
    // (so a very full card shrinks rather than clipping) broke that, because a
    // one-column layout then always "fits" — at 6pt — and the loop returned it
    // even where three columns at 13pt were available. The card went from
    // readable to a thread of tiny text in a large window.
    let measure = |cols: usize| measure_card(rect, rows, widest, widest_key, cols);

    let mut best: Option<(usize, f32, f32, f32)> = None;
    // Up to SIX columns now: the arrangement's card carries the gesture
    // rows — fifty-plus of them once the global chords are listed — and a
    // short window cannot hold them any other way at a legible size. It was
    // five until Ctrl+R joined Ctrl+S on the GENERAL block and the card
    // stopped fitting 650x125, which `the_card_fits_the_window_at_every_size
    // _and_row_count` reported to the point: "51 rows in 5 col(s) need
    // 125.58 of 119.99". A column is cheap; a row with nowhere to be is a
    // shortcut nobody can look up.
    for cols in 1..=6usize {
        let (size, w, h) = measure(cols);
        if w <= rect.width() * 0.94 && h <= rect.height() {
            if best.is_none_or(|(_, bs, ..)| size > bs) {
                best = Some((cols, size, w, h));
            }
        }
    }
    // Nothing fits at all — a window shorter than any legible card. Take the
    // widest arrangement and let the clamp below trim it, which clips inside
    // the window rather than painting outside it.
    let (cols, size, w, h) = best.unwrap_or_else(|| {
        let (size, w, h) = measure(6);
        (6, size, w, h)
    });
    // Never flush against the window edges: a card touching them reads as
    // clipped even when it is not, and exact-equality containment is a float
    // coin toss.
    let card = Vec2::new(w.min(rect.width() * 0.94), h.min(rect.height() * 0.96));
    (
        Rect::from_min_size(rect.center() - card * 0.5, card),
        size,
        cols,
    )
}

/// The help card, drawn over the app.
///
/// Centred, sized to its own content, and painted directly rather than through
/// a `Window` so it cannot be dragged off, cannot be resized into nothing, and
/// looks identical in a plugin editor where there is no window manager at all.
///
/// `gates` reaches the card for one reason only: a shortcut the app would
/// ignore must not be advertised. It goes through the same `available` the
/// keyboard does, so the card and the binding cannot disagree about Space.
pub fn draw_help(
    painter: &Painter,
    rect: Rect,
    dark: bool,
    face: Color32,
    t: f32,
    gates: Gates,
    view: HelpView,
) {
    let rows = card_rows(gates, view);
    let widest = rows.iter().map(|(_, d)| d.len()).max().unwrap_or(20);
    // Counted in `chars`, not bytes: the labels are ASCII today and a "⌫" one
    // day would otherwise be measured as three characters wide.
    let widest_key = rows.iter().map(|(l, _)| l.chars().count()).max().unwrap_or(1);
    let (resting, size, cols) = layout(rect, rows.len(), widest, widest_key);
    // Slide down from above the top edge. At t = 0 the card sits entirely off
    // the top, so the first frame of the animation is genuinely invisible
    // rather than a flash of a half-drawn card.
    let t = t.clamp(0.0, 1.0);
    let travel = resting.bottom() - rect.top();
    let card_rect = resting.translate(egui::vec2(0.0, -(1.0 - t) * travel));

    let row_h = size * 1.75;
    let pad = size * 1.4;
    let key_w = key_col_w(size, widest_key);
    let per_col = rows.len().div_ceil(cols);
    let col_w = (card_rect.width() - pad * 2.0) / cols as f32;
    let font = FontId::new(size, crate::fonts::courier());
    let bold = FontId::new(size, crate::fonts::courier_bold());

    // Dim what is behind it, so the card reads as modal without being one.
    // The dimming fades with the slide, or the app would go dark before the
    // card had arrived to explain why.
    painter.rect_filled(rect, 0.0, Color32::from_black_alpha((150.0 * t) as u8));

    // The desk's own face, through the one derivation every floating panel
    // shares — it was the frozen pair, reachable by no theme.
    let _ = dark;
    let p = crate::recorder_panel::surface_of(face);
    let (bg, fg, dim, edge) = (p.bg, p.ink, p.faint, p.edge);
    painter.rect_filled(card_rect, 4.0, bg);
    painter.rect_stroke(
        card_rect,
        4.0,
        Stroke::new(1.0_f32, edge),
        StrokeKind::Middle,
    );

    for (i, (label, desc)) in rows.iter().enumerate() {
        let x = card_rect.left() + pad + (i / per_col) as f32 * col_w;
        let y = card_rect.top() + pad * 1.2 + (i % per_col) as f32 * row_h;
        if label.is_empty() {
            // A SECTION heading: bold, full ink, no key column — the
            // delineation between the general binds and the view's own.
            painter.text(Pos2::new(x, y), Align2::LEFT_TOP, desc, bold.clone(), fg);
            painter.line_segment(
                [
                    Pos2::new(x, y + row_h * 0.78),
                    Pos2::new(x + col_w - pad, y + row_h * 0.78),
                ],
                Stroke::new(1.0_f32, edge),
            );
            continue;
        }
        painter.text(Pos2::new(x, y), Align2::LEFT_TOP, label, bold.clone(), fg);
        painter.text(
            Pos2::new(x + key_w, y),
            Align2::LEFT_TOP,
            desc,
            font.clone(),
            dim,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The two menu-only actions are in no row of `BINDINGS`, and the
    /// comments that say so stay true.**
    ///
    /// `V` and `W` were retired as unused clutter and the table's prose says
    /// so in three places. Prose drifts — the `V` paragraph outlived its row
    /// and sat there explaining a binding that no longer existed, above rows
    /// it had nothing to do with. This is the cheap assertion that stops the
    /// claim from going stale again, and it fails loudly in the one case that
    /// matters: somebody binds a key and forgets the comments.
    #[test]
    fn the_menu_only_actions_are_in_no_row_of_the_table() {
        for action in [KeyAction::ToggleRecorder, KeyAction::ToggleDrumpad] {
            assert!(
                !binding_for_test(action),
                "{action:?} has a key now — update the note beside `W` in \
                 BINDINGS, both variants' docs, and `on_card`, which \
                 deliberately does not list either"
            );
        }
        // And the two letters the note speaks for. `W` is not free: the
        // arrangement and the MIDI editor both read a bare `W` as "fit" and
        // both cards say so, so a global row claiming it would take the
        // gesture away from two views without anything failing.
        for (key, why) in [
            (Key::W, "the arrangement and the MIDI editor read bare `W` as fit"),
            (Key::V, "`V` was retired from this table on purpose"),
        ] {
            assert!(
                !BINDINGS.iter().any(|&(k, ..)| k == key),
                "{key:?} is bound globally now — {why}"
            );
        }
    }

    /// The source the drift tests read the handlers back from.
    const APP_SRC: &str = include_str!("app.rs");

    /// The body of `fn {name}` in `app.rs` — from its header to the next
    /// method at the same indent. A crude slice, kept because it is honest
    /// about what it is: these tests read code as text, and a parser would
    /// be rigour spent where drift is cheap to catch textually.
    fn handler_body(name: &str) -> &'static str {
        let start = APP_SRC
            .find(&format!("fn {name}("))
            .unwrap_or_else(|| panic!("{name} is gone; this test reads the wrong thing"));
        let rest = &APP_SRC[start..];
        let end = rest[10..].find("\n    fn ").map_or(rest.len(), |e| e + 10);
        &rest[..end]
    }

    /// A card label to the `Key::` idents it stands for. `None` is a
    /// GESTURE row — a mouse verb with no key to check.
    fn keys_of(label: &str) -> Option<Vec<&'static str>> {
        Some(match label {
            "-/=" => vec!["Minus", "Equals"],
            "W" => vec!["W"],
            "Q" => vec!["Q"],
            "Z" => vec!["Z"],
            "N" => vec!["N"],
            "O" => vec!["O"],
            "DEL" => vec!["Delete"],
            "ESC" => vec!["Escape"],
            "ARROWS" => vec!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown"],
            "C-A" => vec!["A"],
            "C-D" => vec!["D"],
            "C-G" => vec!["G"],
            "C-F" => vec!["F"],
            "C-S-N" => vec!["N"],
            "C-M" => vec!["M"],
            "C-T" => vec!["T"],
            "C-C/V" => vec!["C", "V"],
            "C-C/V/X" => vec!["C", "V", "X"],
            // Ctrl/Cmd+S. The BARE `S` is the supporter key and lives in
            // `BINDINGS`; this row vouches for the chord `paint` reads, and
            // the two cannot collide because `pressed` refuses any modified
            // press outright.
            "C-S" => vec!["S"],
            // Ctrl/Cmd+R. The bare `R` is Clear Notes and lives in
            // `BINDINGS`; this row vouches for the chord `paint` reads.
            "C-R" => vec!["R"],
            // The view chords. Dispatched from `fn paint` rather than from
            // any view's own handler — the old version of this test filed
            // every one of them under "gesture" and checked nothing.
            "C-Z" | "C-S-Z" => vec!["Z"],
            "S-1" => vec!["Num1", "Exclamationmark"],
            "S-2" => vec!["Num2"],
            "S-3" => vec!["Num3"],
            _ => return None,
        })
    }

    /// The labels every card carries that `fn paint` dispatches for all
    /// views at once: undo/redo and the shift view keys. They sit outside
    /// the panel section, so the section boundary must not exempt them.
    const PAINT_CHORDS: &[&str] = &["C-Z", "C-S-Z", "S-1", "S-2", "S-3"];

    /// The modifier words a label promises. "chord" is command-or-ctrl,
    /// which is how every handler spells it.
    fn mods_of(label: &str) -> Vec<&'static str> {
        if label.starts_with("C-S-") {
            vec!["chord", "shift"]
        } else if label.starts_with("C-") {
            vec!["chord"]
        } else if label.starts_with("S-") {
            vec!["shift"]
        } else {
            Vec::new()
        }
    }

    /// Line numbers of `hay` where `Key::{k}` appears as a whole ident on a
    /// CODE line. Comment lines are skipped — prose about a key must not
    /// vouch for it — and the ident boundary matters: the old substring
    /// check let `Key::Tab` answer for `Key::T`.
    fn key_lines(hay: &str, k: &str) -> Vec<usize> {
        let needle = format!("Key::{k}");
        hay.lines()
            .enumerate()
            .filter(|(_, line)| !line.trim_start().starts_with("//"))
            .filter(|(_, line)| {
                line.match_indices(&needle).any(|(at, _)| {
                    line[at + needle.len()..]
                        .chars()
                        .next()
                        .is_none_or(|c| !c.is_alphanumeric() && c != '_')
                })
            })
            .map(|(i, _)| i)
            .collect()
    }

    /// Whether some read of `Key::{k}` in `hay` sits in a stretch of code
    /// that mentions every modifier word the label promises.
    ///
    /// Line-windowed and textual on purpose — the same bargain the body
    /// slice makes. The window reaches twelve lines UP because a guard like
    /// `if !i.modifiers.shift { None } else if ...` stands at the head of a
    /// cascade and the last key of the cascade sits eleven lines under it.
    fn key_read_with_mods(hay: &str, k: &str, mods: &[&'static str]) -> bool {
        let lines: Vec<&str> = hay.lines().collect();
        key_lines(hay, k).into_iter().any(|i| {
            let window = lines[i.saturating_sub(12)..(i + 3).min(lines.len())].join("\n");
            mods.iter().all(|m| match *m {
                "chord" => window.contains("command") || window.contains("ctrl"),
                other => window.contains(other),
            })
        })
    }

    /// Every distinct `Key::` ident a body's CODE lines read.
    fn key_idents(hay: &str) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in hay.lines() {
            if line.trim_start().starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(at) = rest.find("Key::") {
                let tail = &rest[at + 5..];
                let ident: String = tail
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !ident.is_empty() && !out.contains(&ident) {
                    out.push(ident);
                }
                rest = tail;
            }
        }
        out
    }

    /// **Every KEY row on a view's card names a key the app really reads,
    /// with the modifiers the label promises.**
    ///
    /// This module's header promises one source, so a shortcut cannot work
    /// unlisted or be listed and not work. `BINDINGS` is what this module
    /// dispatches; the per-view sections are prose, describing keys that
    /// `arrangement_input`, `midi_editor_input` and `fn paint` (the mixer's
    /// own block, and the view chords every surface shares) handle. Prose
    /// drifts, so it is read back against the handlers. The first version
    /// of this test checked substrings in two handlers and skipped the
    /// modifiers; "C-N" passed for years while the handler demanded shift.
    #[test]
    fn every_key_row_names_a_key_the_view_handles() {
        let gates = Gates {
            recorder_shown: true,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        for (view, handler) in [
            // The piano's own section is all `BINDINGS`; only its shared
            // chord rows have a handler to be read against.
            (HelpView::Piano, None),
            // The mixer's DEL and C-T live inline in `fn paint`, inside
            // the `mixer_rect` block — there is no named handler to slice,
            // so the whole of `paint` stands for it.
            (HelpView::Mixer, Some("paint")),
            (HelpView::Arrange, Some("arrangement_input")),
            (HelpView::MidiEdit, Some("midi_editor_input")),
        ] {
            let rows = card_rows(gates, view);
            // The GENERAL rows above the second heading are dispatched by
            // this module from `BINDINGS`, not by the view — except the
            // chords, which no section boundary excuses.
            let panel = rows
                .iter()
                .enumerate()
                .filter(|(_, (l, _))| l.is_empty())
                .nth(1)
                .map_or(rows.len(), |(i, _)| i);
            for (i, (label, desc)) in rows.iter().enumerate() {
                let Some(keys) = keys_of(label) else { continue };
                let hay = if PAINT_CHORDS.contains(label) {
                    handler_body("paint")
                } else if i > panel {
                    match handler {
                        Some(h) => handler_body(h),
                        None => continue,
                    }
                } else {
                    continue;
                };
                for k in &keys {
                    assert!(
                        key_read_with_mods(hay, k, &mods_of(label)),
                        "{view:?} offers {label:?} ({desc}), and no read of \
                         Key::{k} beside the modifiers the label names exists \
                         in the handler"
                    );
                }
            }
        }
    }

    /// **The other direction: every key a surface's handler reads is on
    /// that surface's card.** The old test only read the card against the
    /// code, so a gesture that shipped without a row — which is how the
    /// whole automation set went undocumented — passed in silence. This is
    /// the assertion that catches the NEXT one.
    #[test]
    fn every_key_a_surface_reads_is_on_its_card() {
        let gates = Gates {
            recorder_shown: true,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        // Keys that are deliberately read and deliberately not rows, each
        // with its reason written where the failure will point.
        let spoken_for: &[(&str, &str)] = &[
            // The laptop half of the same key, answering wherever Delete
            // does — an alias, like F1 for help, not a second shortcut.
            ("Backspace", "DEL's unlisted alias"),
        ];
        for (name, views) in [
            ("arrangement_input", vec![HelpView::Arrange]),
            ("midi_editor_input", vec![HelpView::MidiEdit]),
            // `paint` serves every surface at once: the mixer's inline
            // block plus the shared view chords, so every card's labels
            // may vouch for it.
            (
                "paint",
                vec![
                    HelpView::Piano,
                    HelpView::Mixer,
                    HelpView::Arrange,
                    HelpView::MidiEdit,
                ],
            ),
        ] {
            let hay = handler_body(name);
            let carded: Vec<&str> = views
                .iter()
                .flat_map(|v| card_rows(gates, *v))
                .filter_map(|(l, _)| keys_of(l))
                .flatten()
                .collect();
            for ident in key_idents(hay) {
                if spoken_for.iter().any(|(k, _)| *k == ident) {
                    continue;
                }
                assert!(
                    carded.contains(&ident.as_str()),
                    "{name} reads Key::{ident} and no card row spells it — \
                     add the row, or add the key to `spoken_for` with its \
                     reason"
                );
            }
        }
    }

    /// **No card shows one key twice.** Two rows with the same key column is
    /// either a bind described twice or two binds fighting, and both look
    /// like a typo until somebody presses it.
    #[test]
    fn a_card_never_lists_the_same_key_twice() {
        let gates = Gates {
            recorder_shown: true,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        for view in [
            HelpView::Piano,
            HelpView::Mixer,
            HelpView::Arrange,
            HelpView::MidiEdit,
        ] {
            let rows = card_rows(gates, view);
            let mut seen: Vec<&str> = Vec::new();
            for (label, desc) in &rows {
                if label.is_empty() {
                    continue; // a heading
                }
                assert!(
                    !seen.contains(label),
                    "{view:?} lists {label:?} twice; the second says {desc:?}"
                );
                seen.push(label);
            }
        }
    }

    /// **Tab reaches the app, and egui does not eat it.**
    ///
    /// Worth a test rather than a look, because Tab is the one key in this
    /// table that a UI toolkit has its own designs on: egui reads it in
    /// `Memory::begin_pass` to move focus between widgets. It only READS it —
    /// the event is left in the queue — but "only reads it" is a fact about a
    /// version of a dependency, and this is the assertion that notices if that
    /// ever stops being true.
    #[test]
    fn tab_reaches_the_mixer_rather_than_egui() {
        let ctx = egui::Context::default();
        let mut got = None;
        let input = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::Tab,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let _ = ctx.run(input, |ctx| {
            got = pressed(ctx, Gates::default());
        });
        // A key nobody argues about, through the same harness, so a failure
        // above is about Tab and not about this test.
        let mut control = None;
        let g = egui::RawInput {
            events: vec![egui::Event::Key {
                key: Key::G,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }],
            ..Default::default()
        };
        let ctx2 = egui::Context::default();
        let _ = ctx2.run(g, |ctx| {
            control = pressed(ctx, Gates::default());
        });
        assert_eq!(control, Some(KeyAction::ToggleFretboard), "the harness is wrong");
        assert_eq!(
            got,
            Some(KeyAction::ToggleMixer),
            "Tab did not reach the app"
        );
    }

    /// The card is generated from the binding table, so a shortcut that works
    /// but is not listed cannot exist. This asserts the table itself is sane.
    #[test]
    fn every_binding_is_unique_and_described() {
        let mut keys: Vec<Key> = Vec::new();
        for &(key, label, action, _) in BINDINGS {
            assert!(!keys.contains(&key), "{label} is bound twice");
            keys.push(key);
            assert!(!label.is_empty());
            assert!(!describe(action).is_empty(), "{label} has no description");
        }
        // Everything the card shows is a real binding, and every binding the
        // card hides is deliberate.
        // Hidden rows are the ALTERNATES: Esc, and F1 as a second help key.
        // Every hidden one must duplicate an action that is shown, or a
        // shortcut exists that the card never mentions.
        let shown: Vec<KeyAction> = BINDINGS
            .iter()
            .filter(|(.., s)| *s)
            .map(|&(_, _, a, _)| a)
            .collect();
        // A gated binding must be a LISTED one, or the gate hides a shortcut
        // the card was never going to mention in the first place.
        for &(_, label, action, _) in BINDINGS {
            if !available(action, Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }) {
                panic!("{label} is dead even with every gate open");
            }
            if !available(action, Gates::default()) {
                // The ACTION has to be listed, not this particular key. An
                // alias is unlisted by design — `F1` for help, `F11` for
                // fullscreen — and what the rule is protecting against is a
                // gate that hides something the card never mentioned at all.
                assert!(
                    shown.contains(&action),
                    "{label} is gated and its action appears nowhere on the card"
                );
            }
        }
        for &(_, label, action, is_shown) in BINDINGS {
            if !is_shown && action != KeyAction::CloseHelp {
                // By FAMILY, not by exact action. The number row is four keys
                // doing one job — the card names the job once, `1-4`, because
                // four rows naming four diagrams would teach their names
                // instead of the mechanism.
                assert!(
                    shown.iter().any(|a| a.family() == action.family()),
                    "{label} is bound but never listed"
                );
            }
        }
    }

    /// **Each view's card lists only the keys that work on that page.** A
    /// card that advertises the clef cycle over the mixer is teaching a key
    /// that shows you nothing there — and the overlapping letters (S, G, M,
    /// P) must wear the meaning they have THERE, not the piano's.
    #[test]
    fn each_views_card_is_its_own_page() {
        let gates = Gates {
            recorder_shown: true,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        let card = |view: HelpView| card_rows(gates, view);
        let has = |rows: &[(&str, &str)], key: &str| rows.iter().any(|(k, _)| *k == key);
        let desc_of = |rows: &[(&str, &str)], key: &str| -> String {
            rows.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, d)| (*d).to_owned())
                .unwrap_or_default()
        };
        fn desc<'a>(rows: &[(&'a str, &'a str)], key: &str) -> &'a str {
            rows.iter()
                .find(|(k, _)| *k == key)
                .map(|(_, d)| *d)
                .unwrap_or_default()
        }

        // The piano is the home: every shown, live binding is on its card.
        let piano = card(HelpView::Piano);
        let shown = BINDINGS
            .iter()
            .filter(|&&(_, _, a, s)| s && available(a, gates))
            .count();
        // Plus the prose rows every card carries — the two view keys, SAVE,
        // RECORD, and the two undo/redo chords, none of which are in
        // `BINDINGS` because that table is single keys — and the TWO section
        // headings.
        assert_eq!(piano.len(), shown + 8, "the piano card dropped a live key");

        for (view, name) in [
            (HelpView::Mixer, "mixer"),
            (HelpView::Arrange, "arrange"),
            (HelpView::MidiEdit, "editor"),
        ] {
            let rows = card(view);
            // The piano-only keys are gone: keytoggle, the staff pair, the
            // theory row, transpose, clear, and the teach block.
            //
            // `N` is NOT in this list, because it is not one answer for every
            // view: on the mixer and the arrangement it is the teach key and
            // dead, and in the MIDI editor it is DRAW MODE — the editor takes
            // bare `N` for itself, and the teach action is swallowed there
            // exactly as the magnet and the triplet grid are.
            for dead in ["K", "U", "I", "1-4", "T", "C", "Up", "Down", "R", "E", "L"] {
                assert!(
                    !has(&rows, dead),
                    "{name}: `{dead}` is on the card but does nothing there"
                );
            }
            if matches!(view, HelpView::MidiEdit) {
                assert_eq!(
                    desc_of(&rows, "N"),
                    "draw mode",
                    "the editor's N should be its own"
                );
            } else {
                assert!(!has(&rows, "N"), "{name}: `N` is dead here");
            }
            // The window's own switches stay everywhere.
            for live in ["H", "D", "F", "Z", "Tab", "Enter", "Space"] {
                assert!(has(&rows, live), "{name}: `{live}` went missing");
            }
        }

        // The overlapping letters wear their local meanings.
        let arrange = card(HelpView::Arrange);
        assert!(desc(&arrange, "S").contains("split"), "S on arrange: {:?}", desc(&arrange, "S"));
        assert!(desc(&arrange, "G").contains("snap"));
        assert!(desc(&arrange, "M").contains("metronome"));
        assert!(!has(&arrange, "P"), "P does nothing on the arrangement");
        // The automation and tempo lanes have rows now, and they must keep
        // saying what the one-hand rules say: shift drops a node, a tempo
        // point's shift is the ramp, and glue is the MIDI-only truth.
        assert!(
            desc(&arrange, "S-CLK").contains("node"),
            "the arrange card lost the automation gestures: {:?}",
            desc(&arrange, "S-CLK")
        );
        assert!(
            desc(&arrange, "S-PT").contains("ramp"),
            "the arrange card lost the tempo ramp: {:?}",
            desc(&arrange, "S-PT")
        );
        assert!(
            desc(&arrange, "C-G").contains("MIDI"),
            "C-G glues MIDI only, and the card stopped saying so: {:?}",
            desc(&arrange, "C-G")
        );

        let editor = card(HelpView::MidiEdit);
        assert!(desc(&editor, "G").contains("snap"));
        assert!(desc(&editor, "P").contains("triplet"));
        assert!(!has(&editor, "S"), "S is the arrangement's split, not the editor's");
        assert!(!has(&editor, "M"), "M is not an editor key");

        let mixer = card(HelpView::Mixer);
        assert!(desc(&mixer, "S").contains("supporter"), "S on the mixer is the supporter card");
        assert!(!has(&mixer, "G"), "G's guitar band is hidden under the mixer");
        // **M is the metronome here too**, not the taught-chords dialog. The
        // mixer is the other page a take is set up from, and reaching for the
        // click is a thing people do there; teaching a chord is not.
        assert!(
            desc(&mixer, "M").contains("metronome"),
            "M on the mixer should be the metronome, not {:?}",
            desc(&mixer, "M")
        );
    }

    /// **F6 is listed, and it tells the reader-mode truth.** In
    /// accessibility mode egui takes Tab to walk the controls — that is
    /// what a screen-reader user presses it for — so a card whose only
    /// view-cycle row was Tab lied to exactly the audience with the least
    /// means of discovering the key that works. F6's row must be on every
    /// card, beside a Tab row that keeps the ordinary meaning, and it must
    /// say something Tab's row does not — the same action twice with one
    /// description is a duplicate that teaches nothing.
    #[test]
    fn f6_is_on_every_card_and_names_reader_mode() {
        let full = Gates {
            recorder_shown: true,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        for gates in [Gates::default(), full] {
            for view in [
                HelpView::Piano,
                HelpView::Mixer,
                HelpView::Arrange,
                HelpView::MidiEdit,
            ] {
                let rows = card_rows(gates, view);
                let of = |key: &str| -> String {
                    rows.iter()
                        .find(|(k, _)| *k == key)
                        .map(|(_, d)| (*d).to_owned())
                        .unwrap_or_else(|| panic!("{view:?} has no {key} row"))
                };
                let f6 = of("F6");
                assert!(
                    f6.contains("reader") && f6.contains("Tab"),
                    "{view:?}: F6's row no longer explains reader mode: {f6:?}"
                );
                let tab = of("Tab");
                assert!(
                    tab.contains("cycle"),
                    "{view:?}: Tab's row lost the view cycle: {tab:?}"
                );
                assert_ne!(f6, tab, "{view:?}: F6 merely repeats Tab's row");
            }
        }
    }

    /// **In reader mode the card stops advertising Tab and keeps F6.** Tab
    /// is eaten by egui there — it walks the controls, which is what a
    /// screen-reader user presses it for — so a card listing it is telling
    /// a lie to exactly the audience with the least means of discovering
    /// that. F6's row, which wears the reader-mode words, must survive on
    /// every card; outside reader mode both rows stand as before.
    #[test]
    fn reader_mode_swaps_the_tab_row_for_f6_on_every_card() {
        for view in [
            HelpView::Piano,
            HelpView::Mixer,
            HelpView::Arrange,
            HelpView::MidiEdit,
        ] {
            let reading = card_rows(
                Gates { reader_mode: true, ..Gates::default() },
                view,
            );
            assert!(
                !reading.iter().any(|(k, _)| *k == "Tab"),
                "{view:?}: the card advertises Tab in reader mode, where egui eats it"
            );
            let f6 = reading
                .iter()
                .find(|(k, _)| *k == "F6")
                .unwrap_or_else(|| panic!("{view:?}: reader mode lost the F6 row"));
            assert!(
                f6.1.contains("reader"),
                "{view:?}: F6's row stopped naming reader mode: {:?}",
                f6.1
            );
            // And with the mode off, the ordinary card: both keys listed.
            let plain = card_rows(Gates::default(), view);
            assert!(
                plain.iter().any(|(k, _)| *k == "Tab"),
                "{view:?}: Tab went missing outside reader mode"
            );
            assert!(plain.iter().any(|(k, _)| *k == "F6"));
        }
    }

    /// **The reader-mode switch has a key, and only where the window is
    /// ours.** The menu row it mirrors opens from a right-click — the one
    /// gesture the person this switch exists for is least able to aim — so
    /// J is the keyboard door. Gated on `window_sizing` because the
    /// handler hands `child_windows` back when the mode turns off, a grant
    /// a plugin editor must never receive.
    #[test]
    fn j_toggles_reader_mode_only_where_the_window_is_ours() {
        let ours = Gates { window_sizing: true, ..Gates::default() };
        assert_eq!(press(Key::J, ours), Some(KeyAction::ToggleReaderMode));
        assert_eq!(
            press(Key::J, Gates::default()),
            None,
            "J fired inside a host whose windows are not ours to regrant"
        );
        // The card follows the same gate, listed under GENERAL: advertised
        // exactly where it fires, on every view.
        for view in [
            HelpView::Piano,
            HelpView::Mixer,
            HelpView::Arrange,
            HelpView::MidiEdit,
        ] {
            let rows = card_rows(ours, view);
            let j = rows
                .iter()
                .find(|(k, _)| *k == "J")
                .unwrap_or_else(|| panic!("{view:?}: no J row"));
            assert!(
                j.1.contains("reader"),
                "{view:?}: J's description lost its meaning: {:?}",
                j.1
            );
            assert!(
                !card_rows(Gates::default(), view).iter().any(|(k, _)| *k == "J"),
                "{view:?}: the card advertises J where it cannot fire"
            );
        }
    }

    /// Letters that are ordinary shortcuts must not fire with a modifier held.
    /// Cmd-R and Ctrl-R belong to the OS, and swallowing them would be rude.
    #[test]
    fn a_modifier_suppresses_every_shortcut() {
        let ctx = egui::Context::default();
        for mods in [
            egui::Modifiers::COMMAND,
            egui::Modifiers::CTRL,
            egui::Modifiers::ALT,
            egui::Modifiers::SHIFT,
        ] {
            let mut input = egui::RawInput::default();
            input.modifiers = mods;
            input.events.push(egui::Event::Key {
                key: Key::R,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: mods,
            });
            let mut got = Some(KeyAction::ClearNotes);
            // Every gate open, so this is testing the modifier and nothing else.
            let _ = ctx.run(input, |ctx| {
                got = pressed(ctx, Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() });
            });
            assert_eq!(got, None, "R fired with {mods:?} held");
        }
    }

    /// Press one key, with the gates in a given state, and see what comes back.
    fn press(key: Key, gates: Gates) -> Option<KeyAction> {
        let ctx = egui::Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        });
        let mut got = None;
        let _ = ctx.run(input, |ctx| got = pressed(ctx, gates));
        got
    }

    #[test]
    fn a_bare_key_does_fire() {
        assert_eq!(
            press(Key::K, Gates::default()),
            Some(KeyAction::ToggleKeytoggle)
        );
    }

    /// §5: a user who has never opened the Recorder must not start a take by
    /// pressing the widest key on the keyboard.
    ///
    /// Space is the one binding that is not always live, and it is gated on the
    /// BAND being open rather than on the host being able to capture: opening
    /// the band is the consent, and it is one right-click away. Ungated, the
    /// first thing a stray thumb does in a piano app is begin recording the
    /// room.
    #[test]
    fn space_does_nothing_until_the_recorder_band_is_open() {
        assert_eq!(
            press(Key::Space, Gates::default()),
            None,
            "Space started a take on a recorder the user has never opened"
        );
        assert_eq!(
            press(Key::Space, Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }),
            Some(KeyAction::StopRecording)
        );
        // And Enter, which is the other half of the same gate. **Two keys, not
        // one that toggles**: the key that begins a performance must not be the
        // key that ends it, because the difference between the two is a state
        // you cannot read from the bench.
        assert_eq!(press(Key::Enter, Gates::default()), None);
        assert_eq!(
            press(Key::Enter, Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }),
            Some(KeyAction::StartRecording)
        );
        // The gate is Space's alone: nothing else changes with it.
        for (key, want) in [
            (Key::K, KeyAction::ToggleKeytoggle),
            (Key::G, KeyAction::ToggleFretboard),
            (Key::U, KeyAction::ToggleNoteNames),
            (Key::I, KeyAction::CycleClef),
        ] {
            for gates in [Gates::default(), Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }] {
                assert_eq!(press(key, gates), Some(want), "{key:?} under {gates:?}");
            }
        }
    }

    /// The card is built through the same gate the keyboard is, so it cannot
    /// advertise a key that would do nothing. "Listed but dead" and "live but
    /// unlisted" are the same bug told two ways, and one `available` kills both.
    #[test]
    fn the_card_does_not_list_space_while_the_recorder_is_hidden() {
        // `recorder_available` is held CONSTANT and only `recorder_shown`
        // varies, or this measures two changes and calls it one.
        let can_record = Gates {
            recorder_shown: false,
            recorder_available: true,
            window_sizing: true,
            ..Gates::default()
        };
        let hidden = card_rows(can_record, HelpView::Piano);
        for gone in ["Space", "Enter", "W"] {
            assert!(
                !hidden.iter().any(|(label, _)| *label == gone),
                "the card offers {gone}, a shortcut the app would ignore"
            );
        }
        let shown = card_rows(
            Gates {
                recorder_shown: true,
                ..can_record
            },
            HelpView::Piano,
        );
        // TWO rows now: starting and stopping are separate keys — the key
        // which begins a performance is not the key that ends it. (The
        // drumpad's W retired with the recorder's V as unused clutter.)
        assert_eq!(
            shown.len(),
            hidden.len() + 2,
            "opening the band added something other than Enter and Space"
        );
        assert!(
            shown
                .iter()
                .any(|(l, d)| *l == "Space" && *d == describe(KeyAction::StopRecording)),
            "Space did not appear with the band"
        );
        // And every listed row is a live binding, in both states.
        for gates in [Gates::default(), Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }] {
            for (label, _) in card_rows(gates, HelpView::Piano) {
                // The view-key gesture rows are card prose, not bindings.
                if matches!(label, "S-1" | "S-2" | "S-3" | "C-S" | "C-R" | "C-Z" | "C-S-Z")
                    || label.is_empty()
                {
                    // The view-key gesture rows and the section headings
                    // have no table entry to cross-check.
                    continue;
                }
                let (_, _, action, _) = BINDINGS
                    .iter()
                    .find(|(_, l, ..)| *l == label)
                    .unwrap_or_else(|| panic!("{label} is on the card but not in the table"));
                assert!(available(*action, gates), "{label} is listed but dead");
            }
        }
    }

    /// Space is the first listed key that is not one character wide, and the
    /// key column used to be a constant three-and-a-bit characters. Set in
    /// Courier that leaves "Space" a fifth of a glyph clear of its own
    /// description, which reads on screen as a broken renderer.
    #[test]
    fn the_key_column_is_wide_enough_for_the_longest_label_it_shows() {
        let size = 14.0_f32;
        // ~0.62 em per glyph is what the card's own column maths assumes.
        let glyph = size * 0.62;
        for label in card_rows(
            Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() },
            HelpView::Arrange,
        )
            .iter()
            .map(|(l, _)| *l)
        {
            let text = label.chars().count() as f32 * glyph;
            assert!(
                key_col_w(size, label.chars().count()) >= text + glyph * 0.8,
                "{label} has no gap before its description"
            );
        }
        // A card of single letters is unchanged: the old constant is the floor.
        assert_eq!(key_col_w(size, 1), size * 3.2);
    }

    /// The card must fit inside the window it is drawn over, at every size the
    /// app can be, or the shortcut list is the thing you cannot read.
    /// It has to FIT. The first version sized itself from the window height
    /// alone, so the card outgrew the app the moment another shortcut was added
    /// and the top and bottom rows were cut off by the window edges. "It did
    /// not panic" was all the old test checked, which is how that reached a
    /// screenshot.
    #[test]
    fn the_card_fits_the_window_at_every_size_and_row_count() {
        // The real maximum, taken through the gate rather than off the table:
        // the card is longest with every binding live.
        let shown = card_rows(
            Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() },
            HelpView::Arrange,
        )
        .len();
        for w in [650.0_f32, 975.0, 1300.0, 1950.0, 2600.0] {
            for h in [w / 8.667, w / 8.667 + 50.0, w / 8.667 + 182.0, 90.0] {
                let rect = Rect::from_min_size(Pos2::new(7.0, 11.0), Vec2::new(w, h));
                for rows in [1, shown, shown + 4, 20] {
                    // 5 is "Space", the widest key label the card carries.
                    let (card, size, cols) = layout(rect, rows, 46, 5);
                    // ALWAYS: nothing is ever painted outside the app.
                    //
                    // The floor is 6.0 rather than 8.0, and the gap between
                    // them is the anti-clipping fallback rather than slack.
                    // `by_height` only asks for less than 8 when 8 genuinely
                    // does not fit: 22 rows at 650x125 want three columns of
                    // eight, which needs 125.4 points of a 120-point card, and
                    // there is no legible four-column layout at 650 wide to
                    // escape into. Small and complete beats crisp and cut off.
                    assert!(size >= 6.0, "text shrank below legible at {w}x{h}");
                    // And it is the BEST arrangement available, not merely a
                    // working one. This is the invariant that broke when the
                    // font floor dropped to 6.0: a one-column layout suddenly
                    // "fit" at 6pt and was returned in windows where three
                    // columns at 13pt were sitting right there.
                    for other in 1..=3usize {
                        let (alt_size, aw, ah) = measure_card(rect, rows, 46, 5, other);
                        if aw <= rect.width() * 0.94 && ah <= rect.height() {
                            assert!(
                                alt_size <= size + 1e-3,
                                "{other} columns fits at {alt_size} and we chose \
                                 {size} at {w}x{h} with {rows} rows"
                            );
                        }
                    }
                    assert!(
                        rect.contains_rect(card),
                        "card {card:?} escapes window {rect:?}, {rows} rows"
                    );
                    // AND wherever there is room, every row has somewhere to be.
                    // The exception is real rather than a fudge: the smallest
                    // this app goes is 50% with the chord strip and the guitar
                    // view both off, 650x75, and no number of legible rows fits
                    // in 75 points. It clips there rather than escaping, and the
                    // window is one keypress from being bigger.
                    if h >= 120.0 {
                        let used = rows.div_ceil(cols) as f32 * size * 1.75 + size * 1.4 * 1.2;
                        assert!(
                            used <= card.height() + 0.5,
                            "{rows} rows in {cols} col(s) need {used} of {} at {w}x{h}",
                            card.height()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_card_paints_without_panicking() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        for w in [650.0_f32, 1300.0, 2600.0] {
            let rect = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, w / 8.667 + 50.0));
            let _ = ctx.run(Default::default(), |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    // Both gate states: they are two different row counts, and
                    // the shorter one is the card most users will ever see.
                    for gates in [Gates::default(), Gates { recorder_shown: true, recorder_available: true, window_sizing: true, ..Gates::default() }] {
                        for view in [HelpView::Piano, HelpView::Mixer, HelpView::Arrange] {
                            for t in [0.0, 0.01, 0.5, 0.999, 1.0] {
                                draw_help(ui.painter(), rect, false, Color32::from_rgb(0xE3, 0xD9, 0xBD), t, gates, view);
                                draw_help(ui.painter(), rect, true, Color32::from_rgb(0xE3, 0xD9, 0xBD), t, gates, view);
                            }
                        }
                        // Out of range must not escape the window either.
                        draw_help(ui.painter(), rect, false, Color32::from_rgb(0xE3, 0xD9, 0xBD), -1.0, gates, HelpView::Piano);
                        draw_help(ui.painter(), rect, true, Color32::from_rgb(0xE3, 0xD9, 0xBD), 2.0, gates, HelpView::Piano);
                    }
                });
            });
        }
    }
}

// ── the code ───────────────────────────────────────────────────────────────

/// The Konami sequence, watched for on every install: up up down down left
/// right left right B A Enter, with nothing else in between. The owner's
/// spec, verbatim, including what it unlocks — see the cheat dialog in the
/// app. A wrong key resets the walk; a wrong key that happens to be the
/// sequence's own first step counts as that first step, or two Ups in a
/// row before the Downs would silently poison the attempt.
#[derive(Default)]
pub struct Konami {
    at: usize,
}

const KONAMI: [egui::Key; 11] = [
    egui::Key::ArrowUp,
    egui::Key::ArrowUp,
    egui::Key::ArrowDown,
    egui::Key::ArrowDown,
    egui::Key::ArrowLeft,
    egui::Key::ArrowRight,
    egui::Key::ArrowLeft,
    egui::Key::ArrowRight,
    egui::Key::B,
    egui::Key::A,
    egui::Key::Enter,
];

impl Konami {
    /// How deep the walk stands - the caller consumes the frame's normal
    /// key dispatch once the letters start (B toggles borderless and A
    /// opens About; a cheat that opens About mid-code is not a cheat).
    pub fn progress(&self) -> usize {
        self.at
    }

    /// Feed one pressed key; answers true on the press that completes the
    /// code. The walk resets after completion, though the caller's
    /// once-per-install latch means that hardly matters.
    pub fn feed(&mut self, key: egui::Key) -> bool {
        if key == KONAMI[self.at] {
            self.at += 1;
            if self.at == KONAMI.len() {
                self.at = 0;
                return true;
            }
            return false;
        }
        // The restart rule: a mismatch may still BE the first step (an Up
        // after three Ups is still two Ups deep, but this walk only ever
        // needs the one-step lookback because no prefix of the code repeats
        // past its head).
        self.at = usize::from(key == KONAMI[0]);
        false
    }
}

#[cfg(test)]
mod konami_tests {
    use super::*;

    #[test]
    fn the_code_lands_only_when_entered_exactly() {
        use egui::Key::*;
        let mut k = Konami::default();
        let full = [
            ArrowUp, ArrowUp, ArrowDown, ArrowDown, ArrowLeft, ArrowRight,
            ArrowLeft, ArrowRight, B, A, Enter,
        ];
        // Clean entry lands on the final press and no earlier one.
        for (i, key) in full.iter().enumerate() {
            assert_eq!(k.feed(*key), i == full.len() - 1, "step {i}");
        }
        // Anything else in between resets the walk.
        let mut k = Konami::default();
        for key in [ArrowUp, ArrowUp, ArrowDown] {
            assert!(!k.feed(key));
        }
        assert!(!k.feed(Space));
        for (i, key) in full.iter().enumerate() {
            assert_eq!(k.feed(*key), i == full.len() - 1, "post-reset step {i}");
        }
        // An extra Up mid-walk falls back to being the first step, so the
        // classic fat-finger of three Ups still recovers.
        let mut k = Konami::default();
        assert!(!k.feed(ArrowUp));
        assert!(!k.feed(ArrowUp));
        assert!(!k.feed(ArrowUp)); // wrong (wanted Down) - but IS step one
        for (i, key) in full.iter().enumerate().skip(1) {
            assert_eq!(k.feed(*key), i == full.len() - 1, "recovery step {i}");
        }
    }
}
