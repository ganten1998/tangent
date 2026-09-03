//! The four suite faces, drawn from the face kit.
//!
//! One `draw` for all of them: the path in the pushed [`BuiltinFace`] picks
//! the layout, the kit supplies every part, and what a face returns is a
//! wish (`FaceAction`) the app maps onto requests. Nothing here touches
//! `Settings`, the engine, or a disk — a face is a picture of pushed data
//! and a stream of small asks, which is what lets the same code sit in the
//! desktop and (one day) the plugin build unchanged.
//!
//! **That last clause is why the IR browser is shaped the way it is.** It
//! walks folders, but it never opens one: the face writes where it wants to
//! stand into `FaceLive::ir_dir` and the app lists it, exactly as the
//! drumpad's media explorer is listed by `app::refresh_media`. Path
//! arithmetic — a parent, a join, an extension — is string work and stays
//! here; anything that has to ask the filesystem a question is on the other
//! side of the push.

use egui::{Rect, Vec2};

use crate::face::{
    self, correlation_meter, eq_curve, gr_meter, slider_row, stepped_row, vectorscope,
    CurveAction, CurveNode, FaceStyle,
};
use crate::ports::{BuiltinFace, BuiltinFaceParam, PresetRow};

/// The suite's units as the UI knows them: sentinel path, the name on the
/// panel, and the line under it. The engine's `builtin_fx::UNITS` is the
/// same list; the PATH is the contract between them, exactly as
/// `KEYS_PATCH_NAMES` documents for the keyboard.
pub const SUITE_UNITS: [(&str, &str, &str); 13] = [
    ("@tangent-eq", "Tangent EQ", "three bands you can hold in your hand"),
    (
        "@tangent-imager",
        "Tangent Imager",
        "width that folds back to mono without apology",
    ),
    (
        "@tangent-tape",
        "Tangent Tape",
        "an honest quarter-inch machine at its one speed",
    ),
    (
        "@tangent-comp",
        "Tangent Comp",
        "dials, a needle, and nothing to hide behind",
    ),
    // The second shelf-filling: nine more kernels, registered in one wave.
    // Same naming law as `dialogs::FX_UNITS` — the NAME is Tangent's, the
    // hardware a unit is voiced after lives in the line under it — and the
    // chorus, echo and plate REUSE that table's sentinels and rows exactly,
    // because they are the fixed chain's own effects made seatable.
    (
        "@tangent-denoise",
        "Tangent Denoise",
        "learns your noise, then takes exactly what you dial out",
    ),
    (
        "@tangent-amp",
        "Tangent Amp",
        "warm and clean, solved from the blackface schematic",
    ),
    (
        "@tangent-drive",
        "Tangent Drive",
        "the green pedal's mid hump, oversampled",
    ),
    (
        "@tangent-trem",
        "Tangent Trem",
        "a neon lamp flashing at a photocell",
    ),
    (
        "@tangent-chorus",
        "Tangent Chorus",
        "true stereo, after the Boss CE-1",
    ),
    (
        "@tangent-echo",
        "Tangent Echo",
        "warbling saturation, after the RE-201 Space Echo",
    ),
    (
        "@tangent-plate",
        "Tangent Plate",
        "legendary plate, after the EMT-140",
    ),
    (
        "@tangent-wah",
        "Tangent Wah",
        "a touch filter that chases how hard you play",
    ),
    (
        "@tangent-ir",
        "Tangent IR",
        "plays any impulse response you can capture",
    ),
];

/// The face a sentinel names, if it is one of the suite's.
pub fn suite_unit(path: &str) -> Option<(&'static str, &'static str)> {
    SUITE_UNITS
        .iter()
        .find(|(p, _, _)| *p == path)
        .map(|(_, name, about)| (*name, *about))
}

/// Everything a face draws this frame, refreshed by the app before the
/// dialog shows: the pushed control surface, the meters, the tap-derived
/// pictures, and the face's own small gesture state.
#[derive(Default)]
pub struct FaceLive {
    pub face: BuiltinFace,
    /// The comp's needle, engine max-hold per frame, positive dB.
    pub gr_db: f32,
    /// Recent stereo frames off the tap, oldest first, for the goniometer.
    pub scope: Vec<(f32, f32)>,
    /// The goniometer's normalisation, from the ring's own recent peak.
    pub scope_gain: f32,
    /// Windowed correlation off the tap ring.
    pub correlation: f32,
    /// The analyzer's spectrum on the kit's log grid, dB, peak-held.
    pub analyzer_db: Vec<f32>,
    /// Presets on disk for this unit, pushed by the host.
    pub preset_rows: Vec<PresetRow>,
    /// The save-as field.
    pub preset_name: String,
    /// Which curve node a drag took.
    pub drag: Option<usize>,
    /// The EQ's band under the rows, chosen by the last gesture.
    pub band: usize,
    /// The amp face's pedal chips: each pedal's short label, empty for an
    /// empty pedal slot. `None` when the face's seat carries no pedalboard
    /// — the amp on a folder's extra rack, the master or a bus. Filled by
    /// the app each frame, like every other field here, because tenants
    /// live in settings and a face reads only what it is pushed.
    pub pedals: Option<[String; crate::recorder::AMP_PEDALS]>,
    /// **Where the IR browser is standing.** The face WRITES this — walking
    /// into a folder and walking back out is the whole of the gesture — and
    /// the app reads it to know what to list. Empty until the app has put
    /// the IRs folder in it, which is what makes that folder the root a
    /// fresh panel opens on.
    ///
    /// A wish left in the live state rather than a `FaceAction`: the walk
    /// changes nothing outside this panel, so there is nothing for the app
    /// to *do* about it beyond listing the new folder, and the save-as field
    /// beside it (`preset_name`) has always been written by the face and
    /// read by the app the same way.
    pub ir_dir: String,
    /// The IRs folder itself, pushed: `Settings::irs_root`. The browser
    /// opens here, the IRs chip walks back here, and it is what the header
    /// abbreviates paths against.
    pub ir_root: String,
    /// What [`ir_dir`](Self::ir_dir) holds, pushed: folders first, then the
    /// impulses this build can read. See [`is_ir_file`].
    pub ir_entries: Vec<IrEntry>,
    /// Which directory `ir_entries` describes. The app compares it against
    /// `ir_dir` to know whether the listing it holds is still the answer —
    /// the cache key riding with the data, so a walk costs one `read_dir`
    /// and a resting panel costs none.
    pub ir_listed: String,
    /// The shelf generation `ir_entries` was read at, so a capture that
    /// lands in the folder being browsed shows up without a walk. The app
    /// bumps its own counter whenever the host rescans.
    pub ir_gen: u64,
    /// The first row shown — the browser's own scroll.
    pub ir_scroll: usize,
    /// Wheel intent the browser has not spent yet. See `app::WheelSteps`:
    /// one mouse notch is one event, a trackpad flick is thirty, and both
    /// have to mean the same amount of walking.
    pub ir_wheel: crate::app::WheelSteps,
    /// **The row menu a secondary click opened**, or nothing.
    ///
    /// Held here rather than raised as a [`FaceAction`], for the reason
    /// [`ir_dir`](Self::ir_dir) is: opening a menu changes nothing outside
    /// this panel, so there is nothing for the app to *do* about it. The
    /// three verbs it offers are the asks; the menu itself is a gesture, and
    /// gestures live in the face's own state.
    pub ir_menu: Option<IrRowMenu>,
    /// **What Paste would put here — a PATH, pushed like everything else.**
    ///
    /// Copy remembers a file; it never reads one. So the clipboard holds the
    /// path of the impulse that was copied and nothing else: no samples, no
    /// system clipboard. Two reasons, and both are this module's own law.
    /// A face never asks the filesystem a question, so it cannot hold a
    /// file's bytes honestly; and a path is what PASTE needs anyway, because
    /// the copy itself is a host act. It also survives the walk that is the
    /// whole point of the gesture — copy an impulse out of somebody else's
    /// library, walk home, paste — which a selection tied to the visible
    /// listing would not.
    ///
    /// Empty means nothing has been copied. `dialogs::ir_clip` is where it
    /// actually lives, for the lifetime the face does not have.
    pub ir_clip: String,
    /// **Scroll the loaded impulse into view on the next listing that holds
    /// it.**
    ///
    /// The way back from the wizard sets this. Somebody who has just
    /// captured, saved and asked to go back to the panel is going back to
    /// hear the thing they made, and a browser that lands on row zero of
    /// forty makes them hunt for it — the file is highlighted, three
    /// screens down. It is a wish rather than a scroll position because the
    /// answer is not known yet on the frame it is made: the host has still
    /// to bind the file and push its path back, and the folder has still to
    /// be listed. So it stands until it can be honoured, and any gesture in
    /// the browser — a walk, a wheel, a press — cancels it, because by then
    /// the hand has said where it wants to be.
    pub ir_reveal: bool,
}

/// One row of the IR browser: a folder to walk into, or an impulse to load.
///
/// [`crate::drumpad_panel::MediaEntry`] with the duration kept — the shelf
/// this browser replaces printed how long each impulse ran, and a browser
/// meant to be an archival tool should not know less about a file than the
/// list it grew out of. Zero means nobody has measured it, which is the
/// honest answer for a folder the host's own scan has not reached.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct IrEntry {
    pub name: String,
    pub dir: bool,
    pub seconds: f32,
}

/// **The browser's open row menu: what the secondary click landed on.**
///
/// The mixer's insert menu is the shape this follows — a small set of rows
/// drawn over the panel in the same frame, hit-tested before anything under
/// them, dismissed by a press anywhere else — because that is how this app
/// already answers a secondary click and a second idiom would be a second
/// thing to learn. What it adds is the two fields under `entry`, and they
/// are here because a browser's list MOVES: a capture landing in the folder
/// on screen re-lists it, and the wheel scrolls it. A menu that remembered
/// only a row number would offer "Delete" over one file's name and delete
/// whichever file had slid into that row.
///
/// So the menu carries what it was opened over and the folder it was opened
/// in, and every verb checks both before it fires. Disagreement is not an
/// error to report — the listing simply moved — so the menu closes and the
/// hand tries again, over a row it can see.
#[derive(Debug, Clone, PartialEq)]
pub struct IrRowMenu {
    /// The entry the click landed on, as an index into
    /// [`FaceLive::ir_entries`] — absolute, not a row on screen.
    ///
    /// `None` for a press on the list's empty space, and that case is not a
    /// mistake to swallow: Paste has to be reachable in a folder with
    /// nothing in it yet, which is exactly the folder somebody makes to put
    /// a copied impulse into.
    pub entry: Option<usize>,
    /// The name that entry carried when the menu opened.
    pub name: String,
    /// The folder the browser was standing in when it opened.
    pub dir: String,
    /// Where the press landed — the menu's anchor.
    pub at: egui::Pos2,
}

/// Whether a file name is an impulse this build can load.
///
/// The convolver hands its path to `wavread`, which reads RIFF/WAVE and
/// nothing else, so the browser lists `.wav` and nothing else. One spelling,
/// here, because the app does the scanning and the face does the drawing and
/// a filter that disagreed between them would show files that cannot load.
pub fn is_ir_file(name: &str) -> bool {
    std::path::Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// What a face asked for. `Set` speaks the unit's own stable ids; the
/// preset wishes carry only names and paths — the app owns every request.
pub enum FaceAction {
    Set { id: u32, value: f32 },
    SavePreset { name: String },
    LoadPreset { path: String },
    /// The IR face's magic wand: open the capture wizard.
    OpenWizard,
    /// Load this impulse file into the bay.
    PickIr { path: String },
    /// The amp face's pedal chip: open the picker over pedal `pedal`'s
    /// seat. The face knows only the ordinal; the app owns the seat math.
    OpenPedal { pedal: usize },
    /// **The IR face's name field was clicked: raise the system file
    /// chooser and load whatever comes back.** Appended at the END, this
    /// enum's law.
    ///
    /// The shelf below only lists what is in `Tangent Media/IRs`, which is
    /// where the wizard saves. Everybody else's impulse libraries live
    /// wherever they were unzipped, and the name field is the obvious place
    /// to press to change what is loaded — so it is now the door to the
    /// ordinary chooser, exactly as every other file in this app is picked.
    BrowseIr,
    // ── the browser's row menu ──────────────────────────────────────────
    // Three verbs, appended at the END, this enum's law. They are asks and
    // not acts: a face does not touch a disk, so what leaves here is the
    // path and the wish, and the host owns every byte that moves.
    /// **Delete this impulse from disk.** `loaded` says it is the one the
    /// bay is playing, which the confirmation the app raises has to be able
    /// to say out loud.
    ///
    /// The face never deletes, and it does not even ask the host directly:
    /// this rides into a `Dialog::Confirm` first. The app has exactly one
    /// answer for a gesture that cannot be taken back, and an impulse is
    /// squarely in that class — a capture is a session with hardware (a
    /// plate, a cabinet, a room, somebody else's amp) that may never be set
    /// up again, and unlike a preset there is nothing on screen it could be
    /// rebuilt from.
    DeleteIr { path: String, loaded: bool },
    /// **Remember this impulse for Paste.** A path and nothing else — see
    /// [`FaceLive::ir_clip`], which is where the answer comes back in.
    ///
    /// Nothing is read from disk here and nothing is written: Copy is the
    /// cheapest verb in the menu and the only one that cannot fail. That is
    /// also why it is offered ANYWHERE the browser can walk, including deep
    /// inside somebody else's library — remembering where a file is asks
    /// nobody's permission.
    CopyIr { path: String },
    /// **Put a copy of the remembered impulse in this folder.**
    ///
    /// The folder, not the row: a paste lands BESIDE what is already there,
    /// and aiming it at a file would raise the question of replacing that
    /// file — a question with no good answer at the end of a right-click.
    /// The source is [`FaceLive::ir_clip`] and the app supplies it, because
    /// the app is where the clipboard lives.
    PasteIr { into: String },
}

fn param(face: &BuiltinFace, id: u32) -> Option<&BuiltinFaceParam> {
    face.params.iter().find(|p| p.id == id)
}

/// One continuous parameter as a kit row. Returns the ask.
fn row(ui: &mut egui::Ui, st: &FaceStyle, face: &BuiltinFace, id: u32) -> Option<FaceAction> {
    let p = param(face, id)?;
    slider_row(ui, st, &p.name, &p.reading, p.value, p.default)
        .map(|value| FaceAction::Set { id, value })
}

/// One stepped parameter as a kit row: the button wears the current choice,
/// stepping wraps, and a double-click lands on the position the DSP calls
/// home — `p.default`, pushed with the parameter, never a number spelled
/// here.
fn step_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    face: &BuiltinFace,
    id: u32,
    name_override: Option<&str>,
) -> Option<FaceAction> {
    let p = param(face, id)?;
    let steps = p.choices.len().max(2);
    let at = ((p.value * (steps - 1) as f32).round() as usize).min(steps - 1);
    let label = p
        .choices
        .get(at)
        .map(String::as_str)
        .unwrap_or(&p.reading);
    stepped_row(ui, st, name_override.unwrap_or(&p.name), label).map(|ask| {
        let next = match ask {
            face::StepAsk::By(delta) => (at as i32 + delta).rem_euclid(steps as i32) as usize,
            face::StepAsk::Home => {
                ((p.default * (steps - 1) as f32).round() as usize).min(steps - 1)
            }
        };
        FaceAction::Set {
            id,
            value: next as f32 / (steps - 1) as f32,
        }
    })
}

/// The preset strip every face wears at its foot: the saved sounds, a name
/// to save under, and the two buttons. The host scans the folder and pushes
/// the rows; sharing a preset IS sharing the file, which is the point.
///
/// **Every width here is a SHARE of the row, and the row wraps.** This strip
/// used to be four fixed numbers — a caption, a 120-point menu, a 110-point
/// field and a button — which add up to about three hundred and thirty
/// points however narrow the panel is. The Imager's own window is 340 wide
/// and the plugin margin takes twenty-four of those, so the Save button has
/// sat off the right-hand edge of that panel since the day it shipped, and
/// every other face loses it the moment the window is dragged in past ~354.
/// That is the "menus cut off on the right" the owner reported, and it was
/// never about the Imager or the Amp in particular: it was about the strip
/// they both wear.
///
/// Two things fix it and both are needed. The shares — [`preset_parts`],
/// which is where they live now — keep the strip on ONE line at every width
/// a face is opened at, and `horizontal_wrapped` makes the failure mode at
/// any width below that a SECOND LINE rather than a part that is simply not
/// there: an item that will not fit the rest of the row starts a new one
/// instead of running off the end. A layout that cannot overflow is worth
/// more than a layout whose arithmetic happens to be right today.
///
/// The shares moved out of this function so the one panel that has to
/// RESERVE room for the strip can ask the same arithmetic how tall it is
/// going to be rather than guessing. See [`preset_foot_height`].
fn preset_strip(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let mut action = None;
    ui.add_space(PRESET_LEAD);
    let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());
    let pad = ui.spacing().button_padding.x * 2.0;
    let PresetParts { captioned, combo_w, name_w, .. } =
        preset_parts(ui, ui.available_width());
    ui.horizontal_wrapped(|ui| {
        if captioned {
            ui.label(
                egui::RichText::new("PRESET")
                    .font(bold(10.0))
                    .color(st.s.faint),
            );
        }
        // `ComboBox::width` sizes the TEXT, and the arrow and the frame's
        // padding are added on top of it, so the share has to have that
        // taken back off or the menu is wider than the room it was given.
        let icon = ui.spacing().icon_width + ui.spacing().icon_spacing;
        let picked = egui::ComboBox::from_id_salt("face-presets")
            .selected_text(
                egui::RichText::new(if live.preset_rows.is_empty() {
                    "none saved yet"
                } else {
                    "load…"
                })
                .font(bold(10.0))
                .color(st.s.ink),
            )
            .width((combo_w - icon - pad).max(24.0));
        picked.show_ui(ui, |ui| {
            for row_ in &live.preset_rows {
                if ui
                    .selectable_label(
                        false,
                        egui::RichText::new(&row_.name).font(bold(10.0)),
                    )
                    .clicked()
                {
                    action = Some(FaceAction::LoadPreset {
                        path: row_.path.clone(),
                    });
                }
            }
        });
        let edit = egui::TextEdit::singleline(&mut live.preset_name)
            .hint_text("name…")
            .font(bold(10.0))
            .desired_width((name_w - pad).max(24.0));
        ui.add(edit);
        let can_save = !live.preset_name.trim().is_empty();
        if ui
            .add_enabled(
                can_save,
                egui::Button::new(
                    egui::RichText::new("Save").font(bold(10.0)).color(st.s.ink),
                )
                .fill(st.s.raised),
            )
            .on_hover_text("into the Presets folder, shareable as the file")
            .clicked()
        {
            action = Some(FaceAction::SavePreset {
                name: live.preset_name.trim().to_owned(),
            });
        }
    });
    action
}

/// The narrowest the preset menu may be drawn: enough for "load…" and the
/// arrow beside it, which is the whole of what it has to say when it is
/// closed.
const PRESET_MIN_COMBO: f32 = 62.0;
/// And the narrowest the save-as field may be. Below this it stops being a
/// field somebody can read their own typing back out of.
const PRESET_MIN_NAME: f32 = 54.0;
/// What the two of them and the gap between them cost together — the figure
/// the caption is weighed against.
const PRESET_MIN_PARTS: f32 = PRESET_MIN_COMBO + PRESET_MIN_NAME;

/// The space the strip opens with, so it reads as a footer rather than as
/// one more thing stacked on the last control above it.
const PRESET_LEAD: f32 = 6.0;

/// egui's own margin around a `TextEdit`'s text — `Margin::symmetric(4, 2)`,
/// which it adds to the width the caller asked for and to the row height.
/// Named here because the strip has to know what its own field will cost
/// before it draws one.
const TEXT_EDIT_MARGIN: egui::Vec2 = egui::vec2(8.0, 4.0);

/// **What the preset strip is made of at a given width, and how tall that
/// makes it.**
///
/// One function for the drawing and for the RESERVING, because the two have
/// to agree. Most faces stack down their panel and the strip lands wherever
/// the rows above it left the cursor, so how tall it is is nobody's
/// business; the IR panel is the exception, because it fills the rect it is
/// handed and has to keep a foot back before it can lay anything else out.
///
/// That foot used to be `IR_FOOT_H`, a flat thirty-two points — the same
/// four-fixed-numbers disease [`preset_strip`] was itself cured of, and it
/// failed the same way. Below about two hundred and ninety-six points the
/// row wraps to a second line, and a second line is a whole row the
/// reservation never had: the strip ran out of the bottom of the window,
/// and the browser it was supposed to sit under ended up underneath IT.
///
/// So it is measured. Every width below is the width actually handed to a
/// widget, and the line count is the greedy walk `horizontal_wrapped` does:
/// an item that will not fit the rest of the row starts a new one.
struct PresetParts {
    /// Whether the "PRESET" caption is affordable at this width.
    captioned: bool,
    combo_w: f32,
    name_w: f32,
    /// One row of it, from the font and the style rather than a number.
    line_h: f32,
    /// How many rows `horizontal_wrapped` will put the parts on.
    lines: usize,
}

fn preset_parts(ui: &egui::Ui, w: f32) -> PresetParts {
    let font = egui::FontId::new(10.0, crate::fonts::courier_bold());
    // The fixed parts are MEASURED, not guessed: a button is its text plus
    // the theme's own padding, and both of those move with the font and the
    // style. Guessing them is how a strip gets to be 330 wide.
    let word = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), font.clone(), egui::Color32::WHITE)
            .rect
    };
    let pad = ui.spacing().button_padding.x * 2.0;
    let gap = ui.spacing().item_spacing.x;
    let save = word("Save");
    let save_w = save.width() + pad;
    let caption_w = word("PRESET").width();
    // The caption is the one part that is pure decoration, so it is the
    // first thing the budget spends: a narrow strip says what it is by
    // being a menu and a name field, which nobody mistakes for anything.
    let captioned = w >= caption_w + gap + PRESET_MIN_PARTS;
    let fixed = save_w + gap * 2.0 + if captioned { caption_w + gap } else { 0.0 };
    let free = (w - fixed).max(PRESET_MIN_PARTS);
    // The menu takes a little over half of what is left; the field takes
    // the rest. Both stop growing well before the panel does — a preset
    // menu three hundred points wide is not more useful than one at a
    // hundred and sixty, and the empty run to its right is what tells the
    // eye the strip is a footer rather than the panel's subject.
    let combo_w = (free * 0.52).clamp(PRESET_MIN_COMBO, 160.0);
    let name_w = (free - combo_w).clamp(PRESET_MIN_NAME, 160.0);
    // **A row is as tall as the tallest thing on it**, and each of the three
    // is measured the way egui itself sizes it. The menu is the tall one and
    // not for an obvious reason: `ComboBox` lays its arrow out at
    // `icon_width` and takes the greater of that and its own text, so a
    // ten-point word in a twelve-by-four theme rides on a fourteen-point
    // icon. The Save button is its text and the padding; the name field is
    // its text and egui's own margin; and nothing on the row may come out
    // under what the style says a thing you can hit must measure.
    let row_h = save.height();
    let least = ui.spacing().interact_size.y;
    let pad_y = ui.spacing().button_padding.y * 2.0;
    let combo_h = (row_h.max(ui.spacing().icon_width) + pad_y).max(least);
    let button_h = (row_h + pad_y).max(least);
    let line_h = combo_h.max(button_h).max(row_h + TEXT_EDIT_MARGIN.y);
    // The walk, in the order the widgets are added. The field's own margin
    // is added back on because `desired_width` sizes the TEXT inside it,
    // which is why the share had it taken off in the first place.
    let mut lines = 1usize;
    let mut used = 0.0_f32;
    for item in [
        captioned.then_some(caption_w),
        Some(combo_w),
        Some((name_w - pad).max(24.0) + TEXT_EDIT_MARGIN.x),
        Some(save_w),
    ]
    .into_iter()
    .flatten()
    {
        if used > 0.0 && used + gap + item > w {
            lines += 1;
            used = item;
        } else {
            used += if used > 0.0 { gap + item } else { item };
        }
    }
    PresetParts { captioned, combo_w, name_w, line_h, lines }
}

/// **The room a panel must keep back for the preset strip**, when it is the
/// kind of panel that has to keep any back at all.
///
/// The strip's own lead and lines, plus the gap the allocation ABOVE it
/// leaves: a panel that hands its body to `allocate_rect` gets the cursor
/// put one `item_spacing` below that rect, and the strip starts from there.
/// Every number in it comes off the live `Ui`, so a bigger font or a roomier
/// theme moves the reservation with it instead of silently overrunning it.
pub fn preset_foot_height(ui: &egui::Ui) -> f32 {
    let p = preset_parts(ui, ui.available_width());
    let gap = ui.spacing().item_spacing.y;
    gap + PRESET_LEAD + p.lines as f32 * p.line_h + (p.lines as f32 - 1.0) * gap
}

/// The whole face: glass on top where a unit has a picture, rows under it,
/// presets at the foot. Returns at most one ask per frame — a hand only
/// does one thing at a time, and a queue of asks is a queue of bugs.
pub fn draw(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let path = live.face.path.clone();
    let mut action = match path.as_str() {
        "@tangent-eq" => draw_eq(ui, st, live),
        "@tangent-imager" => draw_imager(ui, st, live),
        "@tangent-comp" => draw_comp(ui, st, live),
        "@tangent-ir" => draw_ir(ui, st, live),
        crate::faces_amp::AMP_PATH => crate::faces_amp::draw_amp(ui, st, live),
        // The tape, the trim, and anything the suite grows before its face
        // does: every parameter as a row, in id order — sliders for the
        // continuous ones, the cycling button for anything stepped, so a
        // toggle like the denoise's Learn or the drive's Symmetry wears its
        // words instead of pretending to be a fader. The amp and the IR
        // route here DELIBERATELY for now: their real faces (a tolex panel,
        // an IR browser) are other lanes' work, and generic rows are the
        // honest floor under them until those land.
        _ => {
            let mut out = None;
            let ids: Vec<(u32, bool)> = live
                .face
                .params
                .iter()
                .map(|p| (p.id, p.steps >= 2))
                .collect();
            for (id, stepped) in ids {
                face::keep(
                    &mut out,
                    if stepped {
                        step_row(ui, st, &live.face, id, None)
                    } else {
                        row(ui, st, &live.face, id)
                    },
                );
            }
            out
        }
    };
    face::keep(&mut action, preset_strip(ui, st, live));
    action
}

fn draw_comp(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    // The needle first: the face IS the reduction; the knobs serve it.
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 58.0),
        egui::Sense::hover(),
    );
    gr_meter(ui.painter(), rect, st, live.gr_db);
    ui.add_space(6.0);
    let mut out = None;
    for id in 0..=6 {
        face::keep(&mut out, row(ui, st, &live.face, id));
    }
    out
}

fn draw_imager(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    // The circle, square, centred; the correlation bar under it — the two
    // meters the brief named, and no others.
    let side = ui.available_width().min(210.0);
    ui.vertical_centered(|ui| {
        let (rect, _) =
            ui.allocate_exact_size(Vec2::new(side, side), egui::Sense::hover());
        vectorscope(ui.painter(), rect, st, &live.scope, live.scope_gain);
    });
    ui.add_space(4.0);
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 26.0),
        egui::Sense::hover(),
    );
    correlation_meter(ui.painter(), rect, st, live.correlation);
    ui.add_space(6.0);
    let mut out = None;
    for id in 0..=2 {
        face::keep(&mut out, row(ui, st, &live.face, id));
    }
    out
}

/// The EQ's ids, band-major: `band * 5 + {on, kind, freq, gain, q}`.
const EQ_KNOBS: u32 = 5;
const EQ_BANDS: usize = 3;

fn draw_eq(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), 190.0),
        egui::Sense::hover(),
    );
    // The handles: every band, on or off — an off band draws hollow at its
    // parked spot, and any gesture on it is first of all "wake up".
    let curve_rect = Rect::from_min_size(rect.min, rect.size());
    let mut nodes = Vec::with_capacity(EQ_BANDS);
    let mut on = [false; EQ_BANDS];
    for b in 0..EQ_BANDS {
        let base = b as u32 * EQ_KNOBS;
        let v = |k: u32| param(&live.face, base + k).map(|p| p.value).unwrap_or(0.5);
        on[b] = v(0) >= 0.5;
        let kind_at = {
            let p = param(&live.face, base + 1);
            let steps = p.map(|p| p.choices.len()).unwrap_or(6).max(2);
            ((v(1) * (steps - 1) as f32).round() as usize).min(steps - 1)
        };
        // The glyphs mirror NodeKind::ALL's order; a band's badge is its
        // shape, so the eye can read the curve's recipe at a glance.
        const GLYPHS: [&str; 6] = ["●", "⌐", "¬", "/", "\\", "○"];
        nodes.push(CurveNode {
            freq_hz: face::CURVE_LO_HZ
                * (face::CURVE_HI_HZ / face::CURVE_LO_HZ).powf(v(2)),
            gain_db: (v(3) * 2.0 - 1.0) * face::CURVE_DB,
            glyph: if on[b] { GLYPHS[kind_at.min(5)] } else { "·" },
        });
    }
    let curve = eq_curve(
        ui,
        st,
        curve_rect,
        &nodes,
        &live.face.curve_db,
        if live.analyzer_db.is_empty() {
            None
        } else {
            Some(&live.analyzer_db)
        },
        &mut live.drag,
    );
    let mut out = None;
    if let Some(act) = curve {
        out = match act {
            CurveAction::Move { node, freq_hz, gain_db } => {
                live.band = node;
                let base = node as u32 * EQ_KNOBS;
                // One ask per frame: the freq this frame, the gain rides
                // the next — a drag is many frames, and the pair converges
                // in two. Waking a sleeping band wins the frame instead.
                if !on[node] {
                    Some(FaceAction::Set { id: base, value: 1.0 })
                } else {
                    let f = (freq_hz.max(1.0) / face::CURVE_LO_HZ).ln()
                        / (face::CURVE_HI_HZ / face::CURVE_LO_HZ).ln();
                    let g = (gain_db / face::CURVE_DB + 1.0) * 0.5;
                    let pf = param(&live.face, base + 2).map(|p| p.value).unwrap_or(0.5);
                    let want_f = f.clamp(0.0, 1.0);
                    if (want_f - pf).abs() > 0.002 {
                        Some(FaceAction::Set { id: base + 2, value: want_f })
                    } else {
                        Some(FaceAction::Set {
                            id: base + 3,
                            value: g.clamp(0.0, 1.0),
                        })
                    }
                }
            }
            CurveAction::Turn { node, steps } => {
                live.band = node;
                let base = node as u32 * EQ_KNOBS;
                let q = param(&live.face, base + 4).map(|p| p.value).unwrap_or(0.5);
                Some(FaceAction::Set {
                    id: base + 4,
                    value: (q + steps * 0.03).clamp(0.0, 1.0),
                })
            }
            CurveAction::Cycle { node } => {
                live.band = node;
                let base = node as u32 * EQ_KNOBS;
                if on[node] {
                    // The ask the control was specified around: a tap walks
                    // the filter shapes.
                    let p = param(&live.face, base + 1);
                    let steps = p.map(|p| p.choices.len()).unwrap_or(6).max(2);
                    let at = p
                        .map(|p| {
                            ((p.value * (steps - 1) as f32).round() as usize)
                                .min(steps - 1)
                        })
                        .unwrap_or(0);
                    let next = (at + 1) % steps;
                    Some(FaceAction::Set {
                        id: base + 1,
                        value: next as f32 / (steps - 1) as f32,
                    })
                } else {
                    Some(FaceAction::Set { id: base, value: 1.0 })
                }
            }
            CurveAction::Remove { node } => {
                live.band = node;
                Some(FaceAction::Set {
                    id: node as u32 * EQ_KNOBS,
                    value: 0.0,
                })
            }
            CurveAction::Add { freq_hz, .. } => {
                // Wake the nearest sleeping band and park it where the hand
                // pointed; with all three awake the tap chose the nearest
                // awake one instead, so this only fires with one to spare.
                let sleeping = (0..EQ_BANDS).find(|b| !on[*b]);
                sleeping.map(|b| {
                    live.band = b;
                    let base = b as u32 * EQ_KNOBS;
                    let f = (freq_hz.max(1.0) / face::CURVE_LO_HZ).ln()
                        / (face::CURVE_HI_HZ / face::CURVE_LO_HZ).ln();
                    // Park first, wake next frame: the wake is the visible
                    // half, so it goes last and the park never flashes at
                    // the old spot.
                    let pf = param(&live.face, base + 2).map(|p| p.value).unwrap_or(0.5);
                    if (f.clamp(0.0, 1.0) - pf).abs() > 0.002 {
                        FaceAction::Set { id: base + 2, value: f.clamp(0.0, 1.0) }
                    } else {
                        FaceAction::Set { id: base, value: 1.0 }
                    }
                })
            }
        };
    }
    ui.add_space(6.0);
    // The rows serve the chosen band: its switch, its shape, and its three
    // numbers — with a band walker on top so the panel never needs fifteen
    // rows to say five things.
    let band = live.band.min(EQ_BANDS - 1);
    let base = band as u32 * EQ_KNOBS;
    // The walker is a VIEW, not a parameter: it steps either way like any
    // other stepped row, but there is no engine number behind it. Home
    // therefore cannot mean "what the DSP says" — it means the first band,
    // which is a real answer and the one the row's hover text is promising.
    // A row that silently dropped the gesture would be a row whose tooltip
    // lied, and this walker is the only stepped row in the suite with
    // nobody behind it to catch the ask.
    match stepped_row(ui, st, "Band", &format!("{}", band + 1)) {
        Some(face::StepAsk::By(delta)) => {
            live.band = (band as i32 + delta).rem_euclid(EQ_BANDS as i32) as usize;
        }
        Some(face::StepAsk::Home) => live.band = 0,
        None => {}
    }
    face::keep(&mut out, step_row(ui, st, &live.face, base, None));
    face::keep(&mut out, step_row(ui, st, &live.face, base + 1, Some("Shape")));
    face::keep(&mut out, row(ui, st, &live.face, base + 2));
    face::keep(&mut out, row(ui, st, &live.face, base + 3));
    face::keep(&mut out, row(ui, st, &live.face, base + 4));
    out
}

// ── the IR panel ────────────────────────────────────────────────────────────
// Two halves, modelled on the drumpad: the machine on the left, the browser
// on the right. The owner asked for it in those words — "a whole half window
// modeled just like the drums" — and the shape below is deliberately the
// drumpad's, down to the split, the header, the UP chip and the row height,
// so the two panels read as one family and neither has to be learned twice.

/// The drumpad's own split, reused so the two panels line up. See
/// `drumpad_panel::Layout::new`, which spells the same number inline.
const IR_SPLIT: f32 = 0.52;
/// The gap between the parts, and their inset from the panel edge — the
/// drumpad's `GAP`.
const IR_GAP: f32 = 6.0;
/// The browser's row height and the hairline between rows. The drumpad's
/// numbers: "a fifth taller than the first cut — the owner squinted at 17".
const IR_ROW_H: f32 = 20.0;
const IR_ROW_GAP: f32 = 1.0;
/// The browser's header, where the chips and the path live.
const IR_HEAD_H: f32 = 20.0;
/// The UP chip, and the one that walks home to the IRs folder.
const IR_UP_W: f32 = 24.0;
const IR_HOME_W: f32 = 34.0;
/// The loaded-impulse readout: the LCD bar the name field has always been.
const IR_NAME_H: f32 = 26.0;
/// The Wizard button at the foot of the left half.
const IR_WIZARD_H: f32 = 24.0;
/// **The least the foot may be**, whatever the strip says it wants.
///
/// The strip's height is measured — [`preset_foot_height`] — and a
/// measurement taken from a `Ui` that has not been given a real style yet
/// can come back tiny. A floor under it costs nothing on a panel of any
/// usable height and stops a foot of two points from ever being reserved.
const IR_FOOT_MIN: f32 = 26.0;
/// Narrower than this and the two halves stop being halves: the browser
/// goes UNDER the controls instead of beside them, and takes the whole width
/// while it is there.
///
/// The number is the drumpad's own floor less a little: `drumpad_panel::place`
/// never opens narrower than six hundred points, so a panel that has been
/// dragged well under that is not a drumpad-sized window any more and
/// pretending otherwise buys a hundred-and-eighty-point column of truncated
/// filenames. Stacking gives the same list the FULL width instead, which is
/// what a browser actually wants — impulse names are long and there are
/// never so many that three more rows beat readable ones. Losing the
/// browser entirely at small sizes was never an option: it is the panel's
/// whole point.
const IR_SIDE_BY_SIDE: f32 = 520.0;

/// Every part of the IR panel, from one function.
///
/// The drumpad's `Layout` bargain: the painter, the hit test and the tests
/// all read the same rects, so the header cannot drift off the pane and the
/// rows cannot drift out from under the fingers that press them. Nothing
/// below bakes in a size — the panel is laid out inside whatever rect it is
/// handed, at every width the window can be dragged to.
pub struct IrLayout {
    pub panel: Rect,
    /// Everything above the preset strip.
    pub body: Rect,
    /// The preset strip's room at the foot, which `draw` fills.
    pub foot: Rect,
    /// The left half: the loaded impulse, its controls, the Wizard.
    pub left: Rect,
    /// The LCD bar that names what is loaded — and summons the system file
    /// chooser, which is still the road to an impulse living far away.
    pub name: Rect,
    /// Where the kit rows go.
    pub controls: Rect,
    pub wizard: Rect,
    /// The browser: the pane, its header, the two chips, the list.
    pub browse: Rect,
    pub browse_head: Rect,
    pub browse_up: Rect,
    pub browse_home: Rect,
    pub browse_list: Rect,
    /// Whether the browser is beside the controls or under them.
    pub stacked: bool,
}

impl IrLayout {
    /// The layout for the `Ui` the panel is about to be drawn into: the rect
    /// it has been given, and a foot measured from the preset strip that is
    /// going to land in it.
    ///
    /// The one door `draw_ir` and its tests both go through, so the foot the
    /// layout reserves and the foot the strip actually fills are the same
    /// number by construction rather than by two people agreeing.
    pub fn for_ui(ui: &egui::Ui) -> Self {
        Self::new(ui.available_rect_before_wrap(), preset_foot_height(ui))
    }

    /// `foot` is what the preset strip will cost — see [`preset_foot_height`],
    /// which is where that number comes from on every live path. Passed in
    /// rather than looked up so this stays a pure function of two rectangles
    /// and can be reasoned about at sizes no window ever opens at.
    pub fn new(rect: Rect, foot: f32) -> Self {
        let nothing = Self {
            panel: rect,
            body: Rect::NOTHING,
            foot: Rect::NOTHING,
            left: Rect::NOTHING,
            name: Rect::NOTHING,
            controls: Rect::NOTHING,
            wizard: Rect::NOTHING,
            browse: Rect::NOTHING,
            browse_head: Rect::NOTHING,
            browse_up: Rect::NOTHING,
            browse_home: Rect::NOTHING,
            browse_list: Rect::NOTHING,
            stacked: false,
        };
        // **An infinite rect is a real answer here.** `available_rect_before_wrap`
        // reports unbounded room to a Ui inside a scroll area, and a panel
        // laid out in one of those would put its foot at infinity and draw
        // nothing anybody could press. Falling back to a workable size is
        // what a layout owes its caller; refusing is not.
        let rect = Rect::from_min_size(
            rect.min,
            egui::vec2(
                if rect.width().is_finite() { rect.width() } else { 640.0 },
                if rect.height().is_finite() { rect.height() } else { 460.0 },
            ),
        );
        if !rect.is_positive() || rect.width() < 80.0 || rect.height() < 80.0 {
            return nothing;
        }
        // The strip at the foot is the panel's, not the browser's: it spans
        // the whole width the way it does on every other face, so the two
        // halves above it stop at its top edge.
        //
        // Capped at a quarter of the panel, because a foot bigger than that
        // is a panel with no body left: on a window dragged down to nothing
        // there is no arrangement that works and the browser is the part
        // worth keeping. Every size a face can actually be opened at is far
        // above that cap.
        let foot_h = foot.max(IR_FOOT_MIN).min(rect.height() * 0.25);
        let body = Rect::from_min_max(
            rect.min,
            egui::pos2(rect.right(), rect.bottom() - foot_h),
        );
        let foot = Rect::from_min_max(
            egui::pos2(rect.left(), body.bottom()),
            rect.max,
        );
        let stacked = rect.width() < IR_SIDE_BY_SIDE;
        let (left, browse) = if stacked {
            // The controls take exactly what they need — the readout, two
            // rows, the Wizard — and the browser takes the rest, so the
            // list is the part that grows with the window.
            let need = (IR_NAME_H + IR_GAP + IR_STACKED_CONTROLS + IR_GAP + IR_WIZARD_H)
                .min(body.height() * 0.62);
            let split = body.top() + need;
            (
                Rect::from_min_max(body.min, egui::pos2(body.right(), split)),
                Rect::from_min_max(
                    egui::pos2(body.left(), split + IR_GAP),
                    body.max,
                ),
            )
        } else {
            let split = body.left() + body.width() * IR_SPLIT;
            (
                Rect::from_min_max(body.min, egui::pos2(split - IR_GAP * 0.5, body.bottom())),
                Rect::from_min_max(
                    egui::pos2(split + IR_GAP * 0.5, body.top()),
                    body.max,
                ),
            )
        };
        // ── the left half ───────────────────────────────────────────────
        let name = Rect::from_min_size(
            left.min,
            egui::vec2(left.width(), IR_NAME_H.min(left.height())),
        );
        let wizard_top = (left.bottom() - IR_WIZARD_H).max(name.bottom() + IR_GAP);
        let wizard = Rect::from_min_max(
            egui::pos2(left.left(), wizard_top),
            egui::pos2(left.right(), left.bottom()),
        );
        let controls = Rect::from_min_max(
            egui::pos2(left.left(), name.bottom() + IR_GAP),
            egui::pos2(left.right(), (wizard.top() - IR_GAP).max(name.bottom() + IR_GAP)),
        );
        // ── the browser ─────────────────────────────────────────────────
        let browse_head = Rect::from_min_size(
            browse.min,
            egui::vec2(browse.width(), IR_HEAD_H.min(browse.height())),
        );
        let browse_up = Rect::from_min_size(
            browse_head.min,
            egui::vec2(IR_UP_W.min(browse_head.width()), browse_head.height()),
        );
        let browse_home = Rect::from_min_size(
            egui::pos2(browse_up.right() + 2.0, browse_head.top()),
            egui::vec2(
                IR_HOME_W.min((browse_head.right() - browse_up.right() - 2.0).max(0.0)),
                browse_head.height(),
            ),
        );
        let browse_list = Rect::from_min_max(
            egui::pos2(browse.left(), browse_head.bottom() + 2.0),
            browse.max,
        );
        Self {
            panel: rect,
            body,
            foot,
            left,
            name,
            controls,
            wizard,
            browse,
            browse_head,
            browse_up,
            browse_home,
            browse_list,
            stacked,
        }
    }

    /// The rows the list has room for, top down — however many fit. Row 0
    /// here is entry `ir_scroll` there, exactly as the drumpad's explorer
    /// scrolls its window over a listing the app holds.
    pub fn browse_rows(&self) -> Vec<Rect> {
        if !self.browse_list.is_positive() {
            return Vec::new();
        }
        let step = IR_ROW_H + IR_ROW_GAP;
        let n = ((self.browse_list.height() / step).floor() as usize).min(256);
        (0..n)
            .map(|i| {
                Rect::from_min_size(
                    egui::pos2(
                        self.browse_list.left(),
                        self.browse_list.top() + i as f32 * step,
                    ),
                    egui::vec2(self.browse_list.width(), IR_ROW_H),
                )
            })
            .collect()
    }

    /// **Where an open row menu's rows go**, anchored at the press and
    /// clamped inside the browser pane.
    ///
    /// `mixer_panel::fx_menu_rows`'s bargain, on a different panel: a menu
    /// that overhung its pane would be a menu whose press lands on somebody
    /// else's control — here, the kit rows in the other half — and one that
    /// ran off the bottom of the window would have a row nobody can reach.
    /// So the whole stack is pushed back inside the pane rather than drawn
    /// where the pointer happened to be.
    ///
    /// A pane with no room for the stack answers `Rect::NOTHING`, and the
    /// browser closes the menu rather than drawing one that cannot be
    /// pressed: a menu you can see and cannot dismiss is worse than none.
    pub fn menu_rows(&self, at: egui::Pos2) -> Vec<Rect> {
        let none = vec![Rect::NOTHING; IR_MENU_ROWS];
        if !self.browse.is_positive() {
            return none;
        }
        let n = IR_MENU_ROWS as f32;
        let total = IR_MENU_ROW_H * n + IR_MENU_GAP * (n - 1.0);
        let pane = self.browse.shrink(3.0);
        if pane.width() < 60.0 || pane.height() < total {
            return none;
        }
        let w = IR_MENU_W.min(pane.width());
        let left = at.x.clamp(pane.left(), pane.right() - w);
        let top = at.y.clamp(pane.top(), pane.bottom() - total);
        (0..IR_MENU_ROWS)
            .map(|i| {
                Rect::from_min_size(
                    egui::pos2(left, top + (IR_MENU_ROW_H + IR_MENU_GAP) * i as f32),
                    egui::vec2(w, IR_MENU_ROW_H),
                )
            })
            .collect()
    }
}

/// The row menu's own numbers: a pressing height rather than a reading one
/// (the mixer's insert menu makes the same move, and for the same reason —
/// these are targets, not text), a hairline between rows, and a width wide
/// enough for `Paste "a-long-impulse-name.wav"` without being so wide it
/// cannot sit inside a stacked panel's browser.
const IR_MENU_ROW_H: f32 = 20.0;
const IR_MENU_GAP: f32 = 1.0;
const IR_MENU_W: f32 = 168.0;

/// What the two kit rows need when the panel is stacked rather than split.
/// `slider_row` draws at eighteen points and `ui.horizontal` adds its own
/// spacing; two of them plus that is what the number is.
const IR_STACKED_CONTROLS: f32 = 48.0;

/// **The quiet ink for a row that has a FILL under it.**
///
/// `faint` is a ratio of ink to ground, held at 4.5:1 against the panel it
/// was derived from — and a highlighted row is not that ground. `raised` and
/// `hover` are steps AWAY from the panel in the direction the ink came from,
/// so every step costs `faint` contrast: on Mono, Aurora and Moss it lands
/// at 2.95:1, under the floor a caption has to clear, and the duration at
/// the right of the loaded impulse's own row — the one row a person is most
/// likely to be reading — is the thing that goes blind.
///
/// The header chips answer the same question by moving the SURFACE: a dead
/// chip loses its raised ground and prints on the well instead. A row cannot
/// do that, because its fill is what SAYS it is the loaded one or the one
/// under the pointer. So the ink moves instead, walking back toward full
/// strength by the least it takes — `recorder_panel::hold_ink`'s move,
/// against a caption's floor rather than a body's, and against the surface
/// the words are actually landing on rather than the one they were derived
/// from.
fn row_faint(st: &FaceStyle, on: egui::Color32) -> egui::Color32 {
    /// A duration at the end of a row is a caption, not body text.
    const FLOOR: f32 = 3.0;
    if crate::recorder_panel::contrast_ratio(st.s.faint, on) >= FLOOR {
        return st.s.faint;
    }
    // Toward `ink`, which is guaranteed to clear both fills — `surface_of`
    // holds `raised` and `hover` at 4.5:1 against it — so this terminates on
    // something readable however far it has to walk.
    for k in 1u8..=12 {
        let t = f32::from(k) / 12.0;
        let mix = |a: u8, b: u8| {
            (f32::from(a) + (f32::from(b) - f32::from(a)) * t) as u8
        };
        let out = egui::Color32::from_rgb(
            mix(st.s.faint.r(), st.s.ink.r()),
            mix(st.s.faint.g(), st.s.ink.g()),
            mix(st.s.faint.b(), st.s.ink.b()),
        );
        if crate::recorder_panel::contrast_ratio(out, on) >= FLOOR {
            return out;
        }
    }
    st.s.ink
}

/// Where the browser walks when the UP chip is pressed.
///
/// A pure string answer, so the walk can be tested without a disk: the
/// parent of where we stand, and where we stand when there is no parent —
/// the root of a volume has nowhere further up, and a chip that emptied the
/// path would be a chip that lost the browser.
pub fn ir_parent(dir: &str) -> String {
    std::path::Path::new(dir)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|| dir.to_owned())
}

/// **Whether a path is the IRs folder or somewhere inside it.**
///
/// The line the row menu's two writing verbs are drawn on, and the whole of
/// this panel's answer to "the browser can walk anywhere, so these verbs can
/// be aimed at a file outside the IRs folder".
///
/// **Read anywhere, write only at home.** The browser walks the whole disk
/// on purpose — "you load any irs there" — so at any moment it may be
/// standing in a library somebody bought, unzipped and has one copy of. Copy
/// is fine there: remembering where a file is changes nothing. Delete and
/// Paste are not, and the reason is not that they are risky in general but
/// that they would be this app doing them: a DAW that can erase a file it
/// did not create, out of a folder it was never given, is a DAW nobody can
/// leave pointed at their sample drive. The confirmation in front of Delete
/// guards against a mis-aimed click; it does not grant a mandate.
///
/// The IRs folder is where the Wizard writes and where the panel opens, so
/// it is the one place the app already owns files in — and the road from a
/// foreign library into it is exactly the pair of verbs left live
/// everywhere: copy there, walk home, paste. Nothing a person can reasonably
/// want to do is out of reach; it just goes through their own folder.
///
/// String work, so it stays on this side of the push with the rest of the
/// path arithmetic. `Path::starts_with` compares COMPONENTS, which is what
/// makes `IRs-old` not a child of `IRs`; a `str::starts_with` here would
/// have quietly said it was.
pub fn ir_under_root(path: &str, root: &str) -> bool {
    if path.is_empty() || root.is_empty() {
        // Before the app has said where home is, nothing is home. A default
        // of "yes" here would arm Delete on the first frame of a panel that
        // has not been told anything yet.
        return false;
    }
    std::path::Path::new(path).starts_with(std::path::Path::new(root))
}

/// How many rows the browser's context menu has. Named because the layout,
/// the painter and the tests all count to it.
pub const IR_MENU_ROWS: usize = 3;

/// One row of that menu: the word on it, whether it can be pressed, and the
/// sentence it says on hover.
///
/// **A row that cannot act is drawn dead, never hidden.** The Track menu's
/// own rule, in its own words: "a row that appears only sometimes is a row
/// nobody learns is there". A menu whose length changed with where the
/// browser was standing would teach nobody that Paste exists, and the one
/// question a person actually has — *why can I not delete this?* — is
/// answered by the row itself rather than by nothing at all.
#[derive(Debug, Clone, PartialEq)]
pub struct IrMenuRow {
    pub label: String,
    pub live: bool,
    pub tip: String,
    /// What pressing it asks for. `None` on a dead row.
    pub ask: Option<IrMenuAsk>,
}

/// What a live menu row does. Resolved with the row so the painter never
/// re-derives a path the words were chosen against.
#[derive(Debug, Clone, PartialEq)]
pub enum IrMenuAsk {
    Delete { path: String, loaded: bool },
    Copy { path: String },
    Paste { into: String },
}

/// **The three rows of an open menu, resolved.**
///
/// One function for the words, the enablement and the ask, because the three
/// have to agree: a row that said "Delete" and was live while its ask
/// resolved to nothing would be a menu row that does nothing, and a row
/// greyed for one reason while its hover text gave another is worse than no
/// hover text. The painter draws exactly what comes back and the tests press
/// exactly what comes back.
///
/// `loaded` is the impulse the bay is playing, so Delete can carry the fact
/// the confirmation needs.
pub fn ir_menu_rows(live: &FaceLive, menu: &IrRowMenu, loaded: &str) -> Vec<IrMenuRow> {
    // **The row the menu was opened over, and only if it is still that
    // row.** See [`IrRowMenu`]: the listing moves under an open menu.
    let file = menu
        .entry
        .and_then(|i| live.ir_entries.get(i))
        .filter(|e| !e.dir && e.name == menu.name);
    let target = file.map(|e| {
        std::path::Path::new(&menu.dir)
            .join(&e.name)
            .to_string_lossy()
            .into_owned()
    });
    let home = ir_under_root(&menu.dir, &live.ir_root);
    let dead = |label: &str, tip: &str| IrMenuRow {
        label: label.to_owned(),
        live: false,
        tip: tip.to_owned(),
        ask: None,
    };
    let delete = match (&target, home) {
        (Some(path), true) => IrMenuRow {
            label: "Delete".to_owned(),
            live: true,
            tip: format!("{path}\n\nremove this file from the disk — there is no undo"),
            ask: Some(IrMenuAsk::Delete {
                path: path.clone(),
                loaded: path == loaded,
            }),
        },
        (Some(_), false) => dead(
            "Delete",
            "Tangent only deletes inside its own IRs folder — copy this\n\
             impulse home first if you want to manage it here",
        ),
        (None, _) => dead("Delete", "right-click an impulse to delete it"),
    };
    let copy = match &target {
        Some(path) => IrMenuRow {
            label: "Copy".to_owned(),
            live: true,
            tip: format!("{path}\n\nremember this file, so Paste can put a copy of it\n\
                          in another folder"),
            ask: Some(IrMenuAsk::Copy { path: path.clone() }),
        },
        None => dead("Copy", "right-click an impulse to copy it"),
    };
    let held = std::path::Path::new(&live.ir_clip)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let paste = match (held.is_empty(), home) {
        (false, true) => IrMenuRow {
            label: format!("Paste “{held}”"),
            live: true,
            // **The folder is NAMED**, not called "this folder". Paste
            // always lands where the browser is STANDING, and the menu can
            // be opened over a folder row — so "this folder" is exactly the
            // word somebody would read as the one they right-clicked.
            tip: format!(
                "{}\n\nput a copy in {} — a name already there is never\n\
                 written over: the copy is numbered and the app says which\n\
                 name it got",
                live.ir_clip,
                ir_where(&menu.dir, &live.ir_root)
            ),
            ask: Some(IrMenuAsk::Paste { into: menu.dir.clone() }),
        },
        (true, _) => dead("Paste", "nothing copied yet — Copy an impulse first"),
        (false, false) => dead(
            "Paste",
            "Tangent only writes inside its own IRs folder — walk home\n\
             with the IRs chip and paste there",
        ),
    };
    vec![delete, copy, paste]
}

/// The path the header prints: inside the IRs folder it is written relative
/// to it, because "Cabs/Marshall" says more in the room a header has than
/// the forty characters of absolute path in front of it. Outside, the whole
/// path is the only honest answer — that is the case where a hand needs to
/// know exactly where it has walked to.
pub fn ir_where(dir: &str, root: &str) -> String {
    if dir == root {
        return "IRs".to_owned();
    }
    if !root.is_empty() {
        if let Ok(rel) = std::path::Path::new(dir).strip_prefix(root) {
            return format!("IRs/{}", rel.to_string_lossy());
        }
    }
    dir.to_owned()
}

/// The IR panel: the loaded impulse and its controls on the left, a browser
/// that walks the whole disk on the right.
///
/// The old face was a readout, a flat shelf of whatever the host had found
/// one level down inside `Tangent Media/IRs`, and a name field that opened
/// the system chooser for everything else. The owner's ruling: "it's a
/// legitimate archival tool if you can get it right with the tail... the
/// impulses option is a whole half window modeled just like the drums." So
/// the shelf becomes a real browser with the drumpad's shape and the
/// drumpad's manners — walk into a folder, walk back out with the chip,
/// press a file to LOAD it, the wheel steps the listing — rooted at the IRs
/// folder but free to go anywhere, because "you load any irs there".
///
/// The name bar keeps its click. A browser is the ordinary road; the system
/// chooser is still the short one to a library on another volume, and the
/// two do not compete.
fn draw_ir(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let mut out = None;
    let l = IrLayout::for_ui(ui);
    if !l.body.is_positive() {
        return None;
    }
    // Everything above the foot is drawn by hand into `l`; taking the body
    // in one allocation is what keeps this panel from fighting the layout
    // above it.
    ui.allocate_rect(l.body, egui::Sense::hover());
    let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());

    // What is loaded, said plainly on the glass.
    let loaded = live
        .face
        .extra
        .iter()
        .find(|(k, _)| k == "ir_path")
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    // **The name field is a button.** Pressing it raises the system file
    // chooser, because a bought or downloaded impulse library lives wherever
    // it was unzipped and walking there one folder at a time is the long
    // way round. The bar keeps its LCD look — it is still a readout of what
    // is loaded — and only gains a click, a hover cursor and a hover
    // sentence saying so.
    if l.name.is_positive() {
        face::lcd_glass(ui.painter(), l.name, st);
        let shown = if loaded.is_empty() {
            "no impulse loaded — click to load one".to_owned()
        } else {
            std::path::Path::new(&loaded)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or(loaded.clone())
        };
        face::lcd_text(
            ui.painter(),
            l.name.left_center() + Vec2::new(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            &shown,
            face::fit_mono(l.name.shrink(6.0), &shown, 11.0),
            face::LcdTier::Primary,
            st,
        );
        // The sentence says the panel closes, because it does: the chooser
        // is parented to the main window and this face floats above it, so
        // the face steps aside for the answer. Said out loud rather than
        // left as a surprise — see `dialogs::IR_BROWSE`.
        let hit = ui
            .interact(l.name, ui.id().with("ir-name"), egui::Sense::click())
            .on_hover_cursor(egui::CursorIcon::PointingHand);
        let hit = if loaded.is_empty() {
            hit.on_hover_text(
                "load an impulse response from anywhere on disk\n\
                 (this panel steps aside while the chooser is up)",
            )
        } else {
            hit.on_hover_text(format!(
                "{loaded}\n\nclick to load a different impulse\n\
                 (this panel steps aside while the chooser is up)"
            ))
        };
        if hit.clicked() {
            out = Some(FaceAction::BrowseIr);
        }
    }

    // The unit's own two knobs, in a child of the left half so the kit rows
    // lay themselves out exactly as they do on every other face.
    if l.controls.is_positive() {
        let mut sub = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(l.controls)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        for id in 0..=1 {
            face::keep(&mut out, row(&mut sub, st, &live.face, id));
        }
    }

    // The wand at the foot of the left half: capturing your own is the
    // point of the folder this browser opens on.
    if l.wizard.is_positive()
        && ui
            .put(
                l.wizard,
                egui::Button::new(
                    egui::RichText::new("Wizard…").font(bold(10.0)).color(st.s.ink),
                )
                .fill(st.s.raised),
            )
            .on_hover_text("capture an impulse from hardware, level-staged and honest")
            .clicked()
    {
        out = Some(FaceAction::OpenWizard);
    }

    face::keep(&mut out, draw_ir_browser(ui, st, live, &l, &loaded));
    // **The cursor is put on the foot LAST, and that is the whole of it.**
    //
    // Claiming the body up front is not enough, because `Ui::put` — which
    // is how the Wizard button is drawn into its rect — advances the cursor
    // after the child, and `Layout::advance_after_rects` SETS the cursor to
    // the child's bottom rather than taking the further of the two. The
    // Wizard sits at the foot of the LEFT HALF, and whenever the panel is
    // stacked the left half ends well above the body does. So the cursor
    // came back up into the middle of the pane and the preset strip drew
    // itself on top of the browser, over the ".." chip, eating its clicks —
    // at every width the face actually opens at, because the face opens
    // stacked.
    //
    // Re-asserting it here means no widget drawn above can move it, whatever
    // order they are drawn in and whatever egui does with the cursor on the
    // way: the last word belongs to the layout.
    ui.advance_cursor_after_rect(l.body);
    out
}

/// The right half: where we stand, the two chips, and the rows.
fn draw_ir_browser(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &mut FaceLive,
    l: &IrLayout,
    loaded: &str,
) -> Option<FaceAction> {
    if !l.browse.is_positive() {
        return None;
    }
    let mut out = None;
    let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());
    // **Where the rows came from, taken before a chip can move it.** The
    // entries below describe the folder the panel opened the frame in, and a
    // row's path is that folder plus its name. Reading `live.ir_dir` again
    // further down would join a row onto a folder the UP chip had already
    // walked out of, on the one frame between the press and the app's next
    // listing — a browser that loads the wrong file once in a while.
    let dir = live.ir_dir.clone();
    // The pane is a WELL: the surface `surface_of` derives by stepping away
    // from the ink, so a list drawn on it reads at least as well as the
    // panel around it on every theme. The sweep test measures it rather than
    // trusting that sentence.
    ui.painter().rect_filled(l.browse, 2.0, st.s.well);
    ui.painter().rect_stroke(
        l.browse,
        2.0,
        egui::Stroke::new(1.0, st.s.edge),
        egui::StrokeKind::Inside,
    );

    // ── the header: up, home, and where we stand ────────────────────────
    // **A chip that cannot go anywhere says so, and it says it by sitting
    // FLAT.** At a volume's root there is no up, and standing in the IRs
    // folder there is nowhere to go home to; a chip that looked live and did
    // nothing would be the same lie the amp's clipped pedals were.
    //
    // The dead chip loses its raised ground rather than dimming its word,
    // and that is the theme law rather than a preference: `faint` on
    // `raised` measures 2.95:1 on Mono, Aurora and Moss, which is under the
    // floor a caption has to clear. When the ink cannot clear the surface
    // the SURFACE moves — so it does, onto the well, where the same word
    // reads. It is also the drumpad's own idiom for the same idea: an empty
    // pad sits visibly flatter.
    let chip =
        |ui: &mut egui::Ui, r: Rect, word: &str, id: &str, tip: &str, live: bool| -> bool {
            if !r.is_positive() {
                return false;
            }
            if live {
                ui.painter().rect_filled(r, 2.0, st.s.raised);
            }
            ui.painter().text(
                r.center(),
                egui::Align2::CENTER_CENTER,
                word,
                bold(10.0),
                if live { st.s.ink } else { st.s.faint },
            );
            if !live {
                return false;
            }
            ui.interact(r, ui.id().with(id), egui::Sense::click())
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(tip)
                .clicked()
        };
    let at_root = live.ir_dir == live.ir_root;
    let parent = ir_parent(&live.ir_dir);
    if chip(
        ui,
        l.browse_up,
        "..",
        "ir-up",
        "up one folder — the browser goes anywhere on disk",
        parent != live.ir_dir,
    ) {
        live.ir_dir = parent;
        live.ir_scroll = 0;
        // A chip is a gesture like any other, so it spends the reveal — see
        // [`FaceLive::ir_reveal`]. Without this a wish the folder could not
        // honour stands, and the browser scrolls itself somewhere a frame
        // after the hand said where it wanted to be.
        live.ir_reveal = false;
    }
    if chip(
        ui,
        l.browse_home,
        "IRs",
        "ir-home",
        "back to the IRs folder — where the Wizard saves and where this opens",
        !at_root && !live.ir_root.is_empty(),
    ) {
        live.ir_dir = live.ir_root.clone();
        live.ir_scroll = 0;
        live.ir_reveal = false;
    }
    let where_r = Rect::from_min_max(
        egui::pos2(l.browse_home.right() + 6.0, l.browse_head.top()),
        l.browse_head.max,
    );
    if where_r.is_positive() {
        let word = ir_where(&live.ir_dir, &live.ir_root);
        ui.painter().text(
            where_r.left_center(),
            egui::Align2::LEFT_CENTER,
            crate::drumpad_panel::cut_text(ui.painter(), where_r.width(), &word, &bold(10.0)),
            bold(10.0),
            st.s.faint,
        );
        // Cut for the room, whole on the hover: an archival tool has to be
        // able to say exactly where a file came from.
        ui.interact(where_r, ui.id().with("ir-where"), egui::Sense::hover())
            .on_hover_text(&live.ir_dir);
    }

    // ── the wheel walks the listing ─────────────────────────────────────
    // Three rows a NOTCH of accrued intent, the drumpad explorer's own feel:
    // a mouse notch arrives as one big event and a trackpad flick as thirty
    // small ones, and `WheelSteps` is what makes the two mean the same
    // amount of walking.
    let rows = l.browse_rows();
    let over = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|p| l.browse.contains(p));
    let delta = if over {
        ui.ctx().input(|i| i.raw_scroll_delta.y)
    } else {
        0.0
    };
    let steps = live.ir_wheel.steps(delta);
    // The last page, never past it: scrolling into an empty list is the
    // classic way a browser looks broken with nothing wrong underneath.
    let max_scroll = live.ir_entries.len().saturating_sub(rows.len().max(1));
    if steps != 0 {
        live.ir_scroll = (live.ir_scroll as isize - 3 * steps as isize)
            .clamp(0, max_scroll as isize) as usize;
    }
    live.ir_scroll = live.ir_scroll.min(max_scroll);
    // **A wheel step closes an open menu.** The words on it name a row, and
    // scrolling slides a different row under them — see [`IrRowMenu`]. The
    // stale check further down would catch it, but catching it here is what
    // stops the menu from sitting over a list that has visibly moved.
    if steps != 0 {
        live.ir_menu = None;
        live.ir_reveal = false;
    }

    // ── the impulse the way back came here to show ──────────────────────
    // **Scroll the loaded impulse into view, once, when it can be found.**
    // See [`FaceLive::ir_reveal`]: somebody who has just captured and saved
    // has come back to hear it, and a folder of forty with the new one three
    // screens down is a hunt. It is honoured LATE — after the scroll clamp
    // above and before the rows are drawn — because the answer only exists
    // once the host has bound the file and listed the folder, which is one
    // or two frames after the wish was made.
    if live.ir_reveal && !loaded.is_empty() && !rows.is_empty() {
        let here = std::path::Path::new(loaded).parent();
        if here == Some(std::path::Path::new(&dir)) {
            let name = std::path::Path::new(loaded)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            if let Some(at) = live.ir_entries.iter().position(|e| !e.dir && e.name == name) {
                // The row lands on screen wherever it already was if it was
                // already showing, and at the top of the window otherwise:
                // a reveal that always scrolled would jump a list somebody
                // is already looking at the right part of.
                if at < live.ir_scroll || at >= live.ir_scroll + rows.len() {
                    live.ir_scroll = at.min(max_scroll);
                }
                live.ir_reveal = false;
            }
        }
    }

    // ── the rows ────────────────────────────────────────────────────────
    if live.ir_entries.is_empty() {
        if let Some(r) = rows.first() {
            ui.painter().text(
                egui::pos2(r.left() + 5.0, r.center().y),
                egui::Align2::LEFT_CENTER,
                crate::drumpad_panel::cut_text(
                    ui.painter(),
                    r.width() - 10.0,
                    if at_root {
                        "no impulses here yet — the Wizard fills this folder"
                    } else {
                        "no impulses in this folder"
                    },
                    &bold(10.0),
                ),
                bold(10.0),
                st.s.faint,
            );
        }
    }
    // **The walk is decided after the loop, never inside it.** Two reasons,
    // and both have bitten this app before. A row that changed `ir_dir`
    // mid-list would leave the rows under it joined onto a folder they did
    // not come from; and returning early would stop DRAWING them on the
    // frame the press landed, which is the strobe
    // `face::a_dragged_row_never_moves_the_rows_under_it` was written for —
    // a widget that is absent every other frame cannot be double-clicked
    // and looks like a list that flinches.
    let mut walk_to: Option<String> = None;
    // **Nothing under an open menu answers a press.**
    //
    // The alternative is to draw the menu on top and trust egui to resolve
    // the overlap, and that is a bet this panel does not need to take: the
    // rows are still DRAWN (the strobe rule above), they simply stop being
    // interactive while a menu names one of them. It also happens to be the
    // right behaviour — a click on the list with a menu up means "not that,
    // then", not "load this one instead".
    let menu_up = live.ir_menu.is_some();
    // **The list's own ground, registered BEFORE the rows.** egui hands a
    // press to the last widget registered over the point, so a background
    // added after the rows would eat every one of their clicks. It is here
    // for the secondary press on EMPTY space, which is not a mistake to
    // swallow: Paste has to be reachable in a folder with nothing in it.
    let ground = (!menu_up).then(|| {
        ui.interact(l.browse_list, ui.id().with("ir-list"), egui::Sense::click())
    });
    let mut open_menu: Option<IrRowMenu> = None;
    for (i, r) in rows.iter().enumerate() {
        let Some(e) = live.ir_entries.get(live.ir_scroll + i) else {
            break;
        };
        let path = std::path::Path::new(&dir)
            .join(&e.name)
            .to_string_lossy()
            .into_owned();
        let here = !e.dir && path == loaded;
        let hit = (!menu_up)
            .then(|| ui.interact(*r, ui.id().with(("ir-row", i)), egui::Sense::click()));
        let hovered = hit.as_ref().is_some_and(egui::Response::hovered);
        // **The row's own surface, named once**: the loaded impulse wears
        // the raised ground, the row under the pointer the hover shade, and
        // everything else is the pane it is drawn on. Both the fill and the
        // ink below read this, because ink is a property of the surface it
        // lands on and never of the row it came from.
        let under = if here {
            st.s.raised
        } else if hovered {
            st.s.hover
        } else {
            st.s.well
        };
        if here || hovered {
            ui.painter().rect_filled(*r, 1.5, under);
        }
        // A folder wears the pointing triangle the drumpad's explorer uses,
        // so the two lists are read the same way. The duration sits at the
        // right end where a column of them lines up; a folder has none and
        // a file nobody has measured says nothing rather than "0.0s".
        let tail = if e.dir || e.seconds <= 0.0 {
            String::new()
        } else {
            format!("{:.1}s", e.seconds)
        };
        let quiet = row_faint(st, under);
        let tail_w = if tail.is_empty() {
            0.0
        } else {
            ui.painter()
                .layout_no_wrap(tail.clone(), bold(10.0), quiet)
                .rect
                .width()
                + 8.0
        };
        let word = if e.dir {
            format!("\u{25B8} {}", e.name)
        } else {
            e.name.clone()
        };
        ui.painter().text(
            egui::pos2(r.left() + 5.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            crate::drumpad_panel::cut_text(
                ui.painter(),
                (r.width() - 10.0 - tail_w).max(0.0),
                &word,
                &bold(11.0),
            ),
            bold(11.0),
            st.s.ink,
        );
        if !tail.is_empty() {
            ui.painter().text(
                egui::pos2(r.right() - 5.0, r.center().y),
                egui::Align2::RIGHT_CENTER,
                &tail,
                bold(10.0),
                quiet,
            );
        }
        // A menu is up, so this row was drawn and nothing more — the rest of
        // the body is the interaction it does not have this frame.
        let Some(hit) = hit else { continue };
        let hit = hit.on_hover_cursor(egui::CursorIcon::PointingHand);
        // **A folder is not promised the three verbs**, because two of them
        // are dead over one: an impulse can be deleted and copied and a
        // folder cannot. Saying so here rather than letting the menu say it
        // — the menu still opens, and still explains itself — because a
        // tooltip that offers what the next click will grey out is the sort
        // of small lie that makes a whole panel feel untrustworthy.
        let hit = hit.on_hover_text(if e.dir {
            format!("{}\n\nclick to walk in\n\nright-click for the impulse menu", e.name)
        } else {
            format!(
                "{path}\n\nclick to load this impulse\n\nright-click to delete, copy \
                 or paste"
            )
        });
        // **The secondary press opens the menu over THIS row**, carrying the
        // name and the folder it was opened in so the verbs cannot be handed
        // a different file later. See [`IrRowMenu`].
        // **And the Delete key, on the row under the pointer.**
        //
        // The owner asked for it beside the menu — "delete key should work
        // too" — and the row under the pointer is the right target for the
        // same reason the right-click's is: this browser has no selection to
        // aim at. A press LOADS, so a selection would either fight the load
        // or shadow it; the pointer is already how the user says which file
        // they mean, and hovering one is the gesture they are making when
        // they reach for the key.
        //
        // It goes through the same `DeleteIr` the menu's row does, so it
        // lands on the same confirmation with the same file named in it —
        // which is what makes a key that deletes safe to offer at all. Dead
        // over a folder and outside the IRs folder, exactly as the menu row
        // is; `ir_menu_rows` decides that once and this asks it rather than
        // re-deriving the rule.
        if !e.dir
            && hit.hovered()
            && ui.input(|inp| {
                inp.key_pressed(egui::Key::Delete) || inp.key_pressed(egui::Key::Backspace)
            })
            && ir_under_root(&dir, &live.ir_root)
        {
            out = Some(FaceAction::DeleteIr {
                path: path.clone(),
                loaded: here,
            });
        }
        if hit.secondary_clicked() {
            open_menu = Some(IrRowMenu {
                entry: Some(live.ir_scroll + i),
                name: e.name.clone(),
                dir: dir.clone(),
                at: hit.interact_pointer_pos().unwrap_or_else(|| r.left_top()),
            });
        } else if hit.clicked() {
            if e.dir {
                walk_to = Some(path);
            } else if !here {
                // **A browser, not a picker.** One press loads; there is no
                // OK button to hunt for, and the readout above says what
                // took.
                out = Some(FaceAction::PickIr { path });
                // And the press spends the reveal: the loaded impulse is
                // about to be the one under the pointer.
                live.ir_reveal = false;
            }
        }
    }
    // The empty space below the rows answers the same press, with no file
    // under it: Delete and Copy come up dead and say why, and Paste — the
    // one verb that is about the FOLDER — is live.
    if let Some(g) = ground {
        if open_menu.is_none() && g.secondary_clicked() {
            open_menu = Some(IrRowMenu {
                entry: None,
                name: String::new(),
                dir: dir.clone(),
                at: g
                    .interact_pointer_pos()
                    .unwrap_or_else(|| l.browse_list.left_top()),
            });
        }
    }
    let fresh = open_menu.is_some();
    if let Some(m) = open_menu {
        live.ir_menu = Some(m);
        // A hand that has started aiming at rows does not need the panel
        // scrolling itself somewhere else underneath it.
        live.ir_reveal = false;
    }
    if let Some(into) = walk_to {
        live.ir_dir = into;
        live.ir_scroll = 0;
        live.ir_reveal = false;
    }
    // More below? A quiet count, so a long folder says so — the drumpad's
    // own marker, for the same reason.
    let shown = live.ir_scroll + rows.len();
    if live.ir_entries.len() > shown {
        ui.painter().text(
            egui::pos2(l.browse.right() - 4.0, l.browse.bottom() - 3.0),
            egui::Align2::RIGHT_BOTTOM,
            format!("+{}", live.ir_entries.len() - shown),
            bold(8.0),
            st.s.faint,
        );
    }
    // Last, so it is on top of everything it covers — and registered last,
    // which is what makes its presses beat the pane under it.
    face::keep(&mut out, draw_ir_menu(ui, st, live, l, loaded, fresh));
    out
}

/// **The row menu: three verbs over the browser.**
///
/// Drawn and hit-tested here rather than through egui's own context menu,
/// because this app hand-draws its menus — the mixer's insert menu is rows
/// painted over the strip, the arrangement's is a dialog — and a popup with
/// egui's stock frame inside a themed face would be the one surface in the
/// panel wearing somebody else's paint.
///
/// **Every colour is measured on the surface it lands on.** The stack sits
/// on `raised` (the ground the header chips already use, held at 4.5:1
/// against `ink` by `surface_of`), a live row under the pointer moves to
/// `hover`, and a DEAD row's word takes [`row_faint`] against whichever of
/// the two it is actually on — the same walk the browser's own durations
/// take, and for the same reason: `faint` measures 2.95:1 on `raised` on
/// Mono, Aurora and Moss, which is under the floor a caption has to clear.
fn draw_ir_menu(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &mut FaceLive,
    l: &IrLayout,
    loaded: &str,
    fresh: bool,
) -> Option<FaceAction> {
    let Some(menu) = live.ir_menu.clone() else {
        return None;
    };
    // **A menu whose folder has moved is not this menu.** The chips above
    // still walk while it is open, and the words on a menu opened in
    // `IRs/Cabs` mean nothing standing in `IRs`. Closing is the whole
    // answer: nothing was done, and the hand can press again on a row it
    // can see.
    if menu.dir != live.ir_dir {
        live.ir_menu = None;
        return None;
    }
    let rects = l.menu_rows(menu.at);
    if rects.iter().any(|r| !r.is_positive()) {
        // No room to draw it, so no menu — see `IrLayout::menu_rows`.
        live.ir_menu = None;
        return None;
    }
    let rows = ir_menu_rows(live, &menu, loaded);
    let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());
    let bounds = rects
        .iter()
        .fold(Rect::NOTHING, |a, r| a.union(*r))
        .expand(2.0);
    ui.painter().rect_filled(bounds, 2.0, st.s.raised);
    ui.painter().rect_stroke(
        bounds,
        2.0,
        egui::Stroke::new(1.0_f32, st.s.edge),
        egui::StrokeKind::Inside,
    );
    let mut out = None;
    let mut close = false;
    for (i, (r, row)) in rects.iter().zip(rows.iter()).enumerate() {
        let hit = ui.interact(*r, ui.id().with(("ir-menu", i)), egui::Sense::click());
        let on = if row.live && hit.hovered() {
            st.s.hover
        } else {
            st.s.raised
        };
        if row.live && hit.hovered() {
            ui.painter().rect_filled(*r, 1.5, on);
        }
        ui.painter().text(
            egui::pos2(r.left() + 6.0, r.center().y),
            egui::Align2::LEFT_CENTER,
            crate::drumpad_panel::cut_text(
                ui.painter(),
                (r.width() - 12.0).max(0.0),
                &row.label,
                &bold(11.0),
            ),
            bold(11.0),
            if row.live { st.s.ink } else { row_faint(st, on) },
        );
        let hit = if row.live {
            hit.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            hit
        };
        let hit = hit.on_hover_text(&row.tip);
        // A DEAD row swallows its press and stays put, which is deliberate:
        // it is the one row somebody has a question about, and closing the
        // menu out from under the sentence that answers it would mean
        // opening it again to read the rest.
        if hit.clicked() {
            if let Some(ask) = row.ask.clone() {
                close = true;
                out = Some(match ask {
                    IrMenuAsk::Delete { path, loaded } => {
                        FaceAction::DeleteIr { path, loaded }
                    }
                    IrMenuAsk::Copy { path } => FaceAction::CopyIr { path },
                    IrMenuAsk::Paste { into } => FaceAction::PasteIr { into },
                });
            }
        }
    }
    // **A press anywhere else dismisses it**, the mixer's manner. Read off
    // the raw pointer rather than a scrim widget, because a scrim big enough
    // to catch every press would also have to sit under the menu and over
    // the rows, and this panel has just gone to some trouble to have no
    // ambiguous overlaps in it at all.
    //
    // **Never on the frame the menu opened**, which is belt to the clamp's
    // braces rather than the thing holding the trousers up. The press that
    // opens a menu is itself a press, and it arrives with the release in the
    // same frame often enough — so the rule above would be asked whether
    // that press landed away from a menu that did not exist when it was
    // made.
    //
    // It never has, and cannot: `IrLayout::menu_rows` clamps the stack with
    // `at.clamp(pane.min, pane.max - size)`, so whenever the clamp moves the
    // stack it moves it far enough that the far edge reaches the pointer, and
    // the opening press is by construction inside the pane. An earlier note
    // here claimed the opposite — that a corner right-click lands outside the
    // menu it just summoned — and it is worth correcting rather than
    // deleting, because the guard reads as load-bearing and is not.
    // `the_ir_parts_come_from_one_layout`'s neighbour pins the containment
    // from five corners of the pane.
    if !close && !fresh {
        let away = ui
            .ctx()
            .pointer_interact_pos()
            .is_none_or(|p| !bounds.contains(p));
        if away && ui.input(|i| i.pointer.any_click()) {
            close = true;
        }
    }
    if close {
        live.ir_menu = None;
    }
    out
}

// ── the analyzer ────────────────────────────────────────────────────────────

/// Update a peak-held spectrum from the tap ring, on the kit's log grid.
///
/// A ~43 Hz-resolution Hann window at 48 k: coarse under 100 Hz and honest
/// everywhere the EQ's decisions live. Hand-rolled radix-2, the same forty
/// lines `stretch.rs` keeps for the same licensing reason. Peaks fall at
/// ~20 dB/s so a transient stays readable for a beat without smearing the
/// picture.
pub fn analyzer_tick(
    ring: &crate::tapring::TapRing,
    held_db: &mut Vec<f32>,
    dt: f32,
) {
    const N: usize = 2048;
    const FLOOR_DB: f32 = -90.0;
    if held_db.len() != face::CURVE_POINTS {
        held_db.clear();
        held_db.resize(face::CURVE_POINTS, FLOOR_DB);
    }
    let decay = 20.0 * dt.clamp(0.0, 0.25);
    for v in held_db.iter_mut() {
        *v = (*v - decay).max(FLOOR_DB);
    }
    if ring.frames() < N {
        return;
    }
    let mut re = [0.0f32; N];
    let mut im = [0.0f32; N];
    for i in 0..N {
        // Hann, because a rectangle's leakage floor buries quiet partials —
        // the bench's hardest-won lesson, applied to the picture too.
        let w = 0.5 - 0.5 * (std::f32::consts::TAU * i as f32 / N as f32).cos();
        re[i] = ring.mono_back(N - 1 - i) * w;
    }
    fft(&mut re, &mut im);
    let rate = ring.rate().max(8_000.0);
    // Log-grid bins: each grid point takes the loudest FFT bin in its span,
    // which is what a peak analyzer is.
    let half = N / 2;
    let to_db = |p: f32| 10.0 * (p.max(1e-12)).log10();
    // Single-sided amplitude, normalised for the window's coherent gain.
    let norm = 2.0 / (N as f32 * 0.5);
    for (g, out) in held_db.iter_mut().enumerate() {
        let f0 = face::CURVE_LO_HZ
            * (face::CURVE_HI_HZ / face::CURVE_LO_HZ)
                .powf(g as f32 / (face::CURVE_POINTS - 1) as f32);
        let f1 = face::CURVE_LO_HZ
            * (face::CURVE_HI_HZ / face::CURVE_LO_HZ)
                .powf((g + 1) as f32 / (face::CURVE_POINTS - 1) as f32);
        let b0 = ((f0 / rate * N as f32) as usize).clamp(1, half - 1);
        let b1 = ((f1 / rate * N as f32) as usize).clamp(b0, half - 1);
        let mut peak = 0.0f32;
        for b in b0..=b1 {
            let a = (re[b] * re[b] + im[b] * im[b]).sqrt() * norm;
            peak = peak.max(a);
        }
        let db = to_db(peak * peak).max(FLOOR_DB);
        if db > *out {
            *out = db;
        }
    }
}

/// In-place radix-2, power-of-two length. The forty lines, kept here rather
/// than a crate for the reason `stretch.rs` wrote down: a dependency costs a
/// THIRD-PARTY-LICENSES diff at release time and this does not.
fn fft(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    debug_assert!(n.is_power_of_two());
    let mut j = 0usize;
    for i in 0..n {
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
        let mut m = n >> 1;
        while m >= 1 && j & m != 0 {
            j ^= m;
            m >>= 1;
        }
        j |= m;
    }
    let mut len = 2;
    while len <= n {
        let ang = -std::f32::consts::TAU / len as f32;
        let (ws, wc) = ang.sin_cos();
        for start in (0..n).step_by(len) {
            let mut wr = 1.0f32;
            let mut wi = 0.0f32;
            for k in 0..len / 2 {
                let a = start + k;
                let b = a + len / 2;
                let tr = re[b] * wr - im[b] * wi;
                let ti = re[b] * wi + im[b] * wr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nwr = wr * wc - wi * ws;
                wi = wr * ws + wi * wc;
                wr = nwr;
            }
        }
        len <<= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The analyzer's transform finds a tone where it is and nowhere else —
    /// the ruler is only as honest as its marks.
    #[test]
    fn the_analyzer_fft_finds_a_tone_at_its_own_bin() {
        const N: usize = 2048;
        let mut re = [0.0f32; N];
        let mut im = [0.0f32; N];
        let bin = 100;
        for i in 0..N {
            re[i] = (std::f32::consts::TAU * bin as f32 * i as f32 / N as f32).cos();
        }
        fft(&mut re, &mut im);
        let mag =
            |b: usize| (re[b] * re[b] + im[b] * im[b]).sqrt();
        let peak = mag(bin);
        assert!((peak - N as f32 / 2.0).abs() < 1.0, "peak {peak}");
        for b in [10usize, 50, 200, 500] {
            assert!(mag(b) < peak * 1e-3, "leak at bin {b}: {}", mag(b));
        }
    }

    /// A tone fed through the whole analyzer lands on the grid point that
    /// owns its frequency, at about its true level, and decays when it
    /// stops — peak-hold, not paint-forever.
    #[test]
    fn the_analyzer_hears_a_tone_then_lets_it_fall() {
        let mut ring = crate::tapring::TapRing::new(48_000.0);
        let hz = 1_000.0;
        let frames: Vec<f32> = (0..4096)
            .flat_map(|i| {
                let s =
                    0.5 * (std::f32::consts::TAU * hz * i as f32 / 48_000.0).sin();
                [s, s]
            })
            .collect();
        ring.tick(&frames);
        let mut held = Vec::new();
        analyzer_tick(&ring, &mut held, 0.016);
        let grid_at = |f: f32| {
            ((f / face::CURVE_LO_HZ).ln()
                / (face::CURVE_HI_HZ / face::CURVE_LO_HZ).ln()
                * (face::CURVE_POINTS - 1) as f32)
                .round() as usize
        };
        let at = grid_at(hz);
        let there = held[at.saturating_sub(1)..=(at + 1).min(held.len() - 1)]
            .iter()
            .fold(f32::MIN, |a, b| a.max(*b));
        // 0.5 amplitude is about -6 dB; the Hann and binning cost a little.
        assert!(there > -12.0, "the tone read {there} dB");
        let far = held[grid_at(8_000.0)];
        assert!(far < there - 30.0, "8 kHz read {far} dB against {there}");
        // And it falls once the input stops arriving — silence pushes the
        // tone out of the window, and the hold decays instead of repainting.
        let before = there;
        ring.tick(&vec![0.0f32; 4096 * 2]);
        for _ in 0..10 {
            analyzer_tick(&ring, &mut held, 0.1);
        }
        let after = held[at];
        assert!(after <= before - 15.0, "held {before} → {after}, no decay");
    }

    /// The suite table and the face router agree with themselves.
    #[test]
    fn every_suite_unit_routes_to_a_face() {
        for (path, name, about) in SUITE_UNITS {
            let (n, a) = suite_unit(path).expect("routes");
            assert_eq!(n, name);
            assert_eq!(a, about);
            assert!(!about.is_empty());
        }
        assert!(suite_unit("@tangent-nonsense").is_none());
    }

    // ── the panels fit the rooms they are given ─────────────────────────

    fn a_test_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        ctx
    }

    fn a_test_style() -> FaceStyle {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        FaceStyle::new(p.bg, p.lcd, p.lcd_face)
    }

    /// A face of `n` plain rows, with everything the four hand-built panels
    /// read pushed into it — a saved preset, a full pedalboard, a folder of
    /// impulses — so a width sweep exercises the widest thing each face can
    /// ever be asked to draw rather than its emptiest.
    fn a_full_face(path: &str, n: usize) -> FaceLive {
        FaceLive {
            face: BuiltinFace {
                path: path.to_owned(),
                params: (0..n)
                    .map(|i| BuiltinFaceParam {
                        id: i as u32,
                        symbol: format!("p{i}"),
                        name: format!("KNOB {i}"),
                        units: "%".to_owned(),
                        default: 0.25,
                        reading: "50%".to_owned(),
                        choices: Vec::new(),
                        steps: 0,
                        value: 0.5,
                    })
                    .collect(),
                ..Default::default()
            },
            // The longest label a pedal chip can carry — `pedal_label` cuts
            // at ten characters, so this is the worst case by construction.
            pedals: Some(std::array::from_fn(|_| "compresso\u{2026}".to_owned())),
            preset_rows: vec![PresetRow {
                name: "a saved sound".to_owned(),
                path: "/p/a.tanfx".to_owned(),
            }],
            preset_name: "typed".to_owned(),
            ir_root: "/m/IRs".to_owned(),
            ir_dir: "/m/IRs".to_owned(),
            ir_entries: a_folder(),
            ..Default::default()
        }
    }

    /// **The style the faces are actually drawn under.**
    ///
    /// `dialogs::apply_theme` widens `button_padding` to the twelve-by-four
    /// every dialog in this app wears, and that number decides where the
    /// preset strip wraps: at egui's own four-by-one it holds one line down
    /// to about two hundred and twenty points, and at the dialog's it gives
    /// up around two hundred and ninety-six. Both are swept, because the
    /// point of measuring the foot rather than naming it is that the answer
    /// follows the style rather than a width somebody wrote down once.
    fn padded(ctx: &egui::Context, pad: Vec2) {
        ctx.style_mut(|s| s.spacing.button_padding = pad);
    }

    /// The layout the IR panel will actually be drawn with in a `w` × `h`
    /// window — the same door `draw_ir` goes through, so a test that presses
    /// at `l.browse_up.center()` presses the chip that is really there.
    ///
    /// It has to be this rather than `IrLayout::new(rect, ..)` with a foot
    /// of the test's own choosing: the foot is measured off the live `Ui`,
    /// and a test that guessed it would be testing a panel the app never
    /// draws.
    fn ir_layout(ctx: &egui::Context, size: Vec2) -> IrLayout {
        let mut out = None;
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), size)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| out = Some(IrLayout::for_ui(ui)));
            },
        );
        out.expect("the panel was never laid out")
    }

    /// A folder with both kinds of row in it, one of them named far longer
    /// than any column could hold.
    fn a_folder() -> Vec<IrEntry> {
        let mut v = vec![
            IrEntry { name: "Cabs".to_owned(), dir: true, seconds: 0.0 },
            IrEntry { name: "Halls".to_owned(), dir: true, seconds: 0.0 },
        ];
        v.push(IrEntry {
            name: "a-genuinely-absurd-impulse-response-file-name-from-somebody-\
                   elses-library-2024-remaster.wav"
                .to_owned(),
            dir: false,
            seconds: 12.75,
        });
        for i in 0..30 {
            v.push(IrEntry {
                name: format!("cab-{i:02}.wav"),
                dir: false,
                seconds: 0.5,
            });
        }
        v
    }

    /// Draw `body` into a `w` × `h` panel and report the right-hand edge of
    /// the space it took and of the furthest thing it painted.
    ///
    /// Both numbers matter and they fail differently. `min_rect` catches a
    /// row that asked for more room than the panel has — the layout fault —
    /// and the shape sweep catches a widget that was allocated correctly and
    /// then painted past its allocation, which is what a button whose text
    /// will not fit does. egui CLIPS the second kind, so it leaves no trace
    /// at all in the picture except the part that is missing.
    fn extent(
        ctx: &egui::Context,
        w: f32,
        h: f32,
        body: &mut dyn FnMut(&mut egui::Ui),
    ) -> (Rect, Rect) {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(w, h));
        let mut took = Rect::NOTHING;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        took = ui.scope(&mut *body).response.rect;
                    });
            },
        );
        let mut painted = Rect::NOTHING;
        for cs in &out.shapes {
            let b = cs.shape.visual_bounding_rect();
            if b.is_positive() && b.max.is_finite() {
                painted = painted.union(b);
            }
        }
        (took, painted)
    }

    /// **Nothing runs off the right-hand edge, at any width the window can
    /// be.** The owner's report was "menus that are cut off in them (cut off
    /// on the right side): Imager, Amp", and both were the same shape of
    /// fault: a row built out of fixed numbers instead of shares of the room
    /// it was handed. The preset strip wanted a flat 330 points — more than
    /// the Imager's whole 340-point window less its margins — and the amp's
    /// seven pedal chips wanted about 480, because a button cannot be sized
    /// below its own text. Every other face wore the same strip and lost the
    /// Save button the moment its window was dragged in.
    ///
    /// So the sweep is every face, from the narrowest a dialog can be
    /// dragged to (the 320-point floor, less the plugin margin) out to a
    /// width nobody will ever use, and it asks the two questions separately.
    #[test]
    fn no_face_runs_off_the_right_of_the_rect_it_is_given() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let mut bad: Vec<String> = Vec::new();
        for (path, n) in [
            ("@tangent-eq", 15usize),
            ("@tangent-imager", 3),
            ("@tangent-comp", 7),
            ("@tangent-amp", 9),
            ("@tangent-ir", 2),
            ("@tangent-tape", 8),
        ] {
            for w in [200.0f32, 296.0, 316.0, 376.0, 446.0, 516.0, 900.0, 1400.0] {
                let mut live = a_full_face(path, n);
                let (took, painted) = extent(&ctx, w, 900.0, &mut |ui| {
                    let _ = draw(ui, &st, &mut live);
                });
                // A point of slack: egui justifies a widget to a fractional
                // width and rounds the galley it lays out inside it, so a
                // sub-pixel overhang is arithmetic, not a cut-off menu. The
                // faults this pins ran to eighty points and more.
                if took.right() > w + 1.0 {
                    bad.push(format!("{path} at {w}: took {:.1}", took.right()));
                }
                if painted.right() > w + 1.0 {
                    bad.push(format!("{path} at {w}: painted to {:.1}", painted.right()));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "content escapes the panel:\n  {}",
            bad.join("\n  ")
        );
    }

    // ── the IR panel ────────────────────────────────────────────────────

    /// **Every part of the IR panel comes from one layout**, so the header
    /// cannot drift off its pane, the UP chip cannot drift out of the
    /// header, and a row cannot drift out from under the finger pressing it.
    /// `shell::the_chrome_parts_come_from_one_layout`'s bargain, on the
    /// panel the drumpad's shape was borrowed for.
    #[test]
    fn the_ir_parts_come_from_one_layout() {
        for (w, h, foot) in [
            (1160.0f32, 700.0f32, 30.0f32), // the widest the drumpad ever opens
            (1000.0, 620.0, 30.0),          // a drumpad-sized window
            (620.0, 420.0, 30.0),
            (520.0, 360.0, 30.0), // the last width that still splits
            (519.0, 360.0, 30.0), // and the first that stacks
            (400.0, 300.0, 30.0),
            (296.0, 300.0, 56.0), // the narrowest a dialog can be dragged to,
            (296.0, 300.0, 30.0), // where the strip wraps and where it does not
            (200.0, 200.0, 56.0),
            (200.0, 200.0, 0.0), // a foot nobody measured: the floor holds
        ] {
            let rect = Rect::from_min_size(egui::pos2(17.0, 23.0), Vec2::new(w, h));
            let l = IrLayout::new(rect, foot);
            let at = format!("{w}x{h} foot {foot}");
            assert!(l.body.is_positive(), "{at}: no body");
            // The panel divides into a body and a foot, with nothing lost
            // between them — the preset strip's room is the foot.
            assert!((l.body.bottom() - l.foot.top()).abs() < 0.01, "{at}");
            assert!((l.foot.bottom() - rect.bottom()).abs() < 0.01, "{at}");
            assert!(l.foot.height() > 0.0, "{at}: no room for the presets");
            // The two halves share the body and never each other.
            assert!(l.body.expand(0.01).contains_rect(l.left), "{at}: left");
            assert!(l.body.expand(0.01).contains_rect(l.browse), "{at}: browse");
            // **An open row menu never leaves the browser pane.** Summoned
            // from any of the pane's four corners and from its middle, at
            // every size the panel can be dragged to — `mixer_panel`'s rule
            // that a menu row overhanging its pane is a press that lands on
            // somebody else's control, which here means the kit rows in the
            // other half. A pane too small for the stack answers
            // `Rect::NOTHING`, and the browser closes the menu rather than
            // drawing one nobody can dismiss.
            for corner in [
                l.browse.left_top(),
                l.browse.right_top(),
                l.browse.left_bottom(),
                l.browse.right_bottom(),
                l.browse.center(),
            ] {
                let stack = l.menu_rows(corner);
                assert_eq!(stack.len(), IR_MENU_ROWS, "{at}: wrong number of verbs");
                if stack.iter().all(|r| !r.is_positive()) {
                    continue;
                }
                for (i, r) in stack.iter().enumerate() {
                    assert!(
                        l.browse.expand(0.5).contains_rect(*r),
                        "{at}: menu row {i} summoned at {corner:?} escaped to {r:?}"
                    );
                }
            }
            assert!(
                !l.left.intersects(l.browse),
                "{at}: the halves overlap - {:?} and {:?}",
                l.left,
                l.browse
            );
            if l.stacked {
                assert!(l.browse.top() >= l.left.bottom(), "{at}: not stacked");
            } else {
                assert!(l.browse.left() >= l.left.right(), "{at}: not beside");
            }
            // The left half's three parts, in order, inside it.
            for (what, r) in [("name", l.name), ("controls", l.controls), ("wizard", l.wizard)] {
                assert!(
                    l.left.expand(0.01).contains_rect(r),
                    "{at}: {what} {r:?} escapes the left half {:?}",
                    l.left
                );
            }
            assert!(l.name.top() <= l.controls.top() + 0.01, "{at}");
            assert!(l.controls.bottom() <= l.wizard.top() + 0.01, "{at}");
            // The browser's parts, inside the pane, in order.
            for (what, r) in [
                ("head", l.browse_head),
                ("up", l.browse_up),
                ("home", l.browse_home),
                ("list", l.browse_list),
            ] {
                assert!(
                    l.browse.expand(0.01).contains_rect(r),
                    "{at}: {what} {r:?} escapes the pane {:?}",
                    l.browse
                );
            }
            assert!(l.browse_head.contains_rect(l.browse_up), "{at}: up left the header");
            assert!(l.browse_head.contains_rect(l.browse_home), "{at}: home left it");
            assert!(!l.browse_up.intersects(l.browse_home), "{at}: the chips overlap");
            assert!(l.browse_list.top() >= l.browse_head.bottom(), "{at}");
            // And every row is inside the list, in order, all the same size.
            let rows = l.browse_rows();
            for (i, r) in rows.iter().enumerate() {
                assert!(
                    l.browse_list.expand(0.01).contains_rect(*r),
                    "{at}: row {i} {r:?} escapes the list {:?}",
                    l.browse_list
                );
                assert!((r.height() - IR_ROW_H).abs() < 0.01, "{at}: row {i} is short");
                if i > 0 {
                    assert!(r.top() > rows[i - 1].bottom(), "{at}: rows {i} overlap");
                }
            }
            // Nothing at all leaves the panel it was handed.
            for r in [
                l.body, l.foot, l.left, l.name, l.controls, l.wizard, l.browse,
                l.browse_head, l.browse_up, l.browse_home, l.browse_list,
            ] {
                assert!(rect.expand(0.01).contains_rect(r), "{at}: {r:?} escapes {rect:?}");
            }
        }
    }

    /// A drumpad-sized panel has a real browser in it: a list somebody can
    /// read, not two rows and a scrollbar.
    #[test]
    fn a_drumpad_sized_panel_lists_a_folder_whole() {
        let l = IrLayout::new(
            Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(1000.0, 620.0)),
            32.0,
        );
        assert!(!l.stacked, "a wide panel should split");
        assert!(l.browse.width() > 380.0, "the browser is a sliver: {:?}", l.browse);
        assert!(l.browse_rows().len() >= 20, "only {} rows", l.browse_rows().len());
        // And a panel dragged well under the drumpad's own floor still has a
        // browser — the whole width of one, stacked under the controls.
        let narrow = IrLayout::new(
            Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 360.0)),
            32.0,
        );
        assert!(narrow.stacked, "a narrow panel should stack");
        assert!(
            (narrow.browse.width() - 400.0).abs() < 0.01,
            "the stacked browser is not the full width: {:?}",
            narrow.browse
        );
        assert!(narrow.browse_rows().len() >= 6, "only {} rows", narrow.browse_rows().len());
    }

    /// An unbounded rect — what a Ui inside a scroll area offers — still
    /// lays out. A panel whose foot is at infinity draws nothing.
    #[test]
    fn an_infinite_rect_still_lays_out() {
        let l = IrLayout::new(
            Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                Vec2::new(f32::INFINITY, f32::INFINITY),
            ),
            32.0,
        );
        assert!(l.body.is_positive());
        assert!(l.body.width().is_finite() && l.body.height().is_finite());
        assert!(!l.browse_rows().is_empty());
    }

    /// The path readouts: at home it says so, inside the folder it says
    /// where inside, and outside it tells the whole truth.
    #[test]
    fn the_browser_says_where_it_is_standing() {
        assert_eq!(ir_where("/m/IRs", "/m/IRs"), "IRs");
        assert_eq!(ir_where("/m/IRs/Cabs", "/m/IRs"), "IRs/Cabs");
        assert_eq!(ir_where("/elsewhere/lib", "/m/IRs"), "/elsewhere/lib");
        // Up walks out of the tree and stops at the volume's root rather
        // than emptying the path, which would lose the browser.
        assert_eq!(ir_parent("/m/IRs/Cabs"), "/m/IRs");
        assert_eq!(ir_parent("/m"), "/");
        assert_eq!(ir_parent("/"), "/");
    }

    /// The filter is the WAV reader's own reach and nothing wider: a file
    /// the browser offers has to be a file the convolver can load.
    #[test]
    fn the_browser_lists_only_what_the_reader_can_read() {
        assert!(is_ir_file("hall.wav"));
        assert!(is_ir_file("HALL.WAV"));
        assert!(is_ir_file("a.b.c.Wav"));
        assert!(!is_ir_file("hall.aiff"));
        assert!(!is_ir_file("hall.mp3"));
        assert!(!is_ir_file("hall"));
        assert!(!is_ir_file("wav"));
    }

    /// Press once at `at`, and report what the face asked for. Three frames,
    /// because a click is a press and a release and egui needs the widget to
    /// have been laid out before either.
    fn press(
        ctx: &egui::Context,
        st: &FaceStyle,
        live: &mut FaceLive,
        size: Vec2,
        at: egui::Pos2,
    ) -> Option<FaceAction> {
        press_with(ctx, st, live, size, at, egui::PointerButton::Primary)
    }

    /// The same, with the button named — the secondary press is a whole
    /// gesture on this panel now, and it has to be driven the way the mouse
    /// drives it rather than by writing `live.ir_menu` from a test.
    fn press_with(
        ctx: &egui::Context,
        st: &FaceStyle,
        live: &mut FaceLive,
        size: Vec2,
        at: egui::Pos2,
        button: egui::PointerButton,
    ) -> Option<FaceAction> {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let mut asked = None;
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if let Some(a) = draw(ui, st, live) {
                                asked = Some(a);
                            }
                        });
                },
            );
        }
        asked
    }


    /// Hover `at` and press `key`, and answer what the face asked for. Two
    /// frames: egui hit-tests against the previous frame's layout, so the
    /// move has to land before the key does.
    fn key_over(
        ctx: &egui::Context,
        st: &FaceStyle,
        live: &mut FaceLive,
        size: Vec2,
        at: egui::Pos2,
        key: egui::Key,
    ) -> Option<FaceAction> {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let mut asked = None;
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                },
            ],
        ] {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if let Some(a) = draw(ui, st, live) {
                                asked = Some(a);
                            }
                        });
                },
            );
        }
        asked
    }

    /// **The Delete key deletes the impulse under the pointer**, and it asks
    /// for it exactly the way the menu row does.
    ///
    /// The owner asked for it beside the right-click: "delete key should work
    /// too". The row under the pointer is the target because this browser has
    /// no selection to aim at — a press LOADS, so a selection would either
    /// fight the load or shadow it — and the pointer is already how the user
    /// says which file they mean.
    ///
    /// What makes a key that deletes safe to offer is that it goes through the
    /// same `DeleteIr` the menu does, so it lands on the same confirmation
    /// with the same file named in it. The three things it must NOT do are
    /// each pinned below: no folder, nothing outside the IRs folder, and
    /// nothing when the pointer is not on a row at all.
    #[test]
    fn the_delete_key_removes_the_impulse_under_the_pointer() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();

        // Rows 0 and 1 are folders; row 2 is the first impulse.
        let mut live = a_full_face("@tangent-ir", 2);
        assert!(!live.ir_entries[2].dir, "the fixture's row 2 is not a file");
        let name = live.ir_entries[2].name.clone();
        let want = format!("/m/IRs/{name}");
        let asked = key_over(
            &ctx,
            &st,
            &mut live,
            size,
            rows[2].center(),
            egui::Key::Delete,
        );
        match asked {
            Some(FaceAction::DeleteIr { path, loaded }) => {
                assert_eq!(path, want, "the key deleted a different row");
                assert!(!loaded, "the fixture's row 2 is not the loaded impulse");
            }
            other => panic!("Delete over an impulse asked for something else: {}", other.is_some()),
        }

        // Backspace too: the key that means "delete" on a laptop.
        let mut live = a_full_face("@tangent-ir", 2);
        assert!(matches!(
            key_over(&ctx, &st, &mut live, size, rows[2].center(), egui::Key::Backspace),
            Some(FaceAction::DeleteIr { .. })
        ));

        // **A FOLDER is not an impulse.** The menu draws its Delete row dead
        // over one; the key is dead over one for the same reason.
        let mut live = a_full_face("@tangent-ir", 2);
        assert!(live.ir_entries[0].dir);
        assert!(
            key_over(&ctx, &st, &mut live, size, rows[0].center(), egui::Key::Delete)
                .is_none(),
            "the key deleted a folder"
        );

        // **And nothing outside the IRs folder.** Read anywhere, write only
        // at home — `ir_under_root`'s rule, which the key asks rather than
        // re-derives.
        let mut live = a_full_face("@tangent-ir", 2);
        live.ir_dir = "/elsewhere/library".to_owned();
        assert!(
            key_over(&ctx, &st, &mut live, size, rows[2].center(), egui::Key::Delete)
                .is_none(),
            "the key erased a file out of somebody else's library"
        );

        // **And nothing at all when the pointer is not on a row.** A key that
        // deleted whatever was loaded whenever the panel had focus would be a
        // different and much worse feature.
        let mut live = a_full_face("@tangent-ir", 2);
        assert!(
            key_over(
                &ctx,
                &st,
                &mut live,
                size,
                l.browse_head.center(),
                egui::Key::Delete
            )
            .is_none(),
            "the key acted with the pointer off the list"
        );
    }

    /// **The browser opens on the IRs folder, walks anywhere, and comes
    /// back.** The owner's two sentences about it — "you load any irs
    /// there" and "it defaults in the tangent media irs folder though" — are
    /// both promises about walking, so this drives the panel with real
    /// presses: into a folder, out with the UP chip, and home with the chip
    /// beside it.
    #[test]
    fn the_browser_walks_out_of_the_irs_folder_and_back() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        // The panel opens where the app put it: the IRs folder.
        assert_eq!(live.ir_dir, "/m/IRs");
        // Row 0 is the first folder — the listing is folders then files.
        assert!(live.ir_entries[0].dir);
        let _ = press(&ctx, &st, &mut live, size, rows[0].center());
        assert_eq!(live.ir_dir, "/m/IRs/Cabs", "walking in went nowhere");
        assert_eq!(live.ir_scroll, 0, "the scroll came along into the new folder");
        // Up: out of the folder, then out of the IRs tree entirely, which
        // is the whole of "you load any irs there".
        let _ = press(&ctx, &st, &mut live, size, l.browse_up.center());
        assert_eq!(live.ir_dir, "/m/IRs");
        let _ = press(&ctx, &st, &mut live, size, l.browse_up.center());
        assert_eq!(live.ir_dir, "/m", "up would not leave the IRs folder");
        assert_eq!(ir_where(&live.ir_dir, &live.ir_root), "/m");
        // And the chip beside it walks home from wherever that got to.
        let _ = press(&ctx, &st, &mut live, size, l.browse_home.center());
        assert_eq!(live.ir_dir, "/m/IRs", "the IRs chip did not go home");
    }

    /// **Pressing an impulse LOADS it.** A browser, not a picker: there is
    /// no OK button, and the ask that comes back names the file's whole
    /// path, joined onto the folder the browser is standing in.
    #[test]
    fn pressing_an_impulse_loads_it_and_the_name_bar_still_browses() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        // Row 2 is the first file; rows 0 and 1 are the folders.
        assert!(!live.ir_entries[2].dir);
        let want = format!("/m/IRs/{}", live.ir_entries[2].name);
        match press(&ctx, &st, &mut live, size, rows[2].center()) {
            Some(FaceAction::PickIr { path }) => assert_eq!(path, want),
            other => panic!(
                "pressing an impulse asked for {:?}",
                other.map(|a| std::mem::discriminant(&a))
            ),
        }
        assert_eq!(live.ir_dir, "/m/IRs", "loading a file walked somewhere");
        // The name bar keeps its own road to the system chooser — the way
        // to a library on another volume, which no amount of walking beats.
        match press(&ctx, &st, &mut live, size, l.name.center()) {
            Some(FaceAction::BrowseIr) => {}
            other => panic!(
                "the name bar asked for {:?}",
                other.map(|a| std::mem::discriminant(&a))
            ),
        }
        // And the Wizard is still one press away.
        match press(&ctx, &st, &mut live, size, l.wizard.center()) {
            Some(FaceAction::OpenWizard) => {}
            other => panic!(
                "the wand asked for {:?}",
                other.map(|a| std::mem::discriminant(&a))
            ),
        }
    }

    // ── the browser's row menu ──────────────────────────────────────────

    /// One ordinary frame with nothing pressed — the panel drawing itself.
    fn settle(ctx: &egui::Context, st: &FaceStyle, live: &mut FaceLive, size: Vec2) {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let _ = draw(ui, st, live);
                    });
            },
        );
    }

    /// Open the menu with a real secondary press on row `row`, and hand back
    /// the rects its three verbs are drawn into.
    ///
    /// Through the same door the app goes through — a pointer event, the
    /// production `draw`, and then `IrLayout::menu_rows` with the anchor the
    /// PANEL stored — so a test that presses `rects[0].center()` presses the
    /// row that is really there.
    fn open_menu_on(
        ctx: &egui::Context,
        st: &FaceStyle,
        live: &mut FaceLive,
        size: Vec2,
        at: egui::Pos2,
    ) -> Vec<Rect> {
        let asked = press_with(ctx, st, live, size, at, egui::PointerButton::Secondary);
        assert!(
            asked.is_none(),
            "opening a menu is not an ask; the panel acted on the secondary press"
        );
        let menu = live
            .ir_menu
            .clone()
            .expect("the secondary press opened no menu");
        ir_layout(ctx, size).menu_rows(menu.at)
    }

    /// **The secondary press opens the menu over the row it landed on, and
    /// each verb acts on THAT file.**
    ///
    /// The owner's second ask — "the picker needs a right click to
    /// delete/copy/paste ir" — driven as events rather than by writing the
    /// menu into the live state, because the interesting half is the aiming:
    /// a browser's list scrolls and re-lists under an open menu, so a row
    /// number is not an identity. The menu carries the name and the folder
    /// and every verb is resolved against them.
    #[test]
    fn a_right_click_opens_the_menu_and_every_verb_acts_on_that_row() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        // Rows 0 and 1 are the folders; row 2 is the first impulse.
        assert!(!live.ir_entries[2].dir);
        let name = live.ir_entries[2].name.clone();
        let want = format!("/m/IRs/{name}");

        // **The menu opened by a press the clamp cannot reach.**
        //
        // `menu_rows` clamps the stack into `browse.shrink(3.0)`, and the
        // rows it is summoned from span the whole of `browse_list` — so a
        // secondary press in the outermost three points of the browser lands
        // on a row while the menu it opens sits up to three points away from
        // the pointer. "A press anywhere else dismisses it" is then TRUE of
        // the press that opened it, and the menu appears and vanishes in one
        // frame. `!fresh` is what stops that, and it is reachable only there
        // and only when the press and its release arrive together — which is
        // what a quick right-click looks like at sixty frames a second.
        {
            let at = egui::pos2(l.browse_list.right(), l.browse_list.top() + 4.0);
            let stack = l.menu_rows(at);
            let bounds = stack.iter().fold(Rect::NOTHING, |a, r| a.union(*r)).expand(2.0);
            assert!(
                !bounds.contains(at),
                "the fixture no longer reproduces the gap between the row and the \
                 clamped menu, so the guard below is inert here and this proves nothing"
            );
            let mut edge = a_full_face("@tangent-ir", 2);
            let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
            // One quiet frame first, so the rows have been laid out and there
            // is something for the press to land on.
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let _ = draw(ui, &st, &mut edge);
                        });
                },
            );
            // Then the whole click in ONE frame, which is the case the guard
            // is for: three separate frames put the release on a frame where
            // the menu is no longer fresh.
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events: vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Secondary,
                            pressed: true,
                            modifiers: Default::default(),
                        },
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Secondary,
                            pressed: false,
                            modifiers: Default::default(),
                        },
                    ],
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let _ = draw(ui, &st, &mut edge);
                        });
                },
            );
            assert!(
                edge.ir_menu.is_some(),
                "a right-click at the edge of the browser opened a menu and dismissed \
                 it in the same frame"
            );
        }

        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        {
            let menu = live.ir_menu.clone().expect("no menu");
            assert_eq!(menu.entry, Some(2), "the menu aimed at the wrong row");
            assert_eq!(menu.name, name, "the menu did not remember the file");
            assert_eq!(menu.dir, "/m/IRs", "the menu did not remember the folder");
        }
        // The whole stack sits inside the browser pane, wherever it was
        // summoned — `mixer_panel::fx_menu_rows`'s rule.
        for (i, r) in menu_rects.iter().enumerate() {
            assert!(
                l.browse.expand(0.5).contains_rect(*r),
                "menu row {i} escaped the pane: {r:?} against {:?}",
                l.browse
            );
        }

        // COPY: a path, and nothing read from disk.
        match press(&ctx, &st, &mut live, size, menu_rects[1].center()) {
            Some(FaceAction::CopyIr { path }) => assert_eq!(path, want),
            other => panic!("Copy asked for {}", named(other.as_ref())),
        }
        assert!(live.ir_menu.is_none(), "the menu stayed up after a verb");

        // DELETE, on the same row: the whole path, and the fact the
        // confirmation needs — this one is not the impulse in the bay.
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        match press(&ctx, &st, &mut live, size, menu_rects[0].center()) {
            Some(FaceAction::DeleteIr { path, loaded }) => {
                assert_eq!(path, want);
                assert!(!loaded, "an impulse nobody loaded was called loaded");
            }
            other => panic!("Delete asked for {}", named(other.as_ref())),
        }

        // And DELETE over the row the bay IS playing says so, because the
        // question in front of it has to be able to.
        live.face.extra = vec![("ir_path".to_owned(), want.clone())];
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        match press(&ctx, &st, &mut live, size, menu_rects[0].center()) {
            Some(FaceAction::DeleteIr { path, loaded }) => {
                assert_eq!(path, want);
                assert!(loaded, "the loaded impulse was not reported as loaded");
            }
            other => panic!("Delete asked for {}", named(other.as_ref())),
        }

        // PASTE is about the FOLDER, not the row: it lands beside what is
        // there and cannot name a file to write over.
        live.ir_clip = "/elsewhere/library/plate.wav".to_owned();
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        match press(&ctx, &st, &mut live, size, menu_rects[2].center()) {
            Some(FaceAction::PasteIr { into }) => assert_eq!(into, "/m/IRs"),
            other => panic!("Paste asked for {}", named(other.as_ref())),
        }
    }

    /// A name for whatever came back, for a panic message. `FaceAction` has
    /// no `Debug` and does not want one — it carries paths.
    fn named(a: Option<&FaceAction>) -> String {
        match a {
            None => "nothing".to_owned(),
            Some(FaceAction::PickIr { path }) => format!("PickIr({path})"),
            Some(FaceAction::DeleteIr { path, loaded }) => {
                format!("DeleteIr({path}, loaded={loaded})")
            }
            Some(FaceAction::CopyIr { path }) => format!("CopyIr({path})"),
            Some(FaceAction::PasteIr { into }) => format!("PasteIr({into})"),
            Some(FaceAction::BrowseIr) => "BrowseIr".to_owned(),
            Some(FaceAction::OpenWizard) => "OpenWizard".to_owned(),
            Some(_) => "some other ask".to_owned(),
        }
    }

    /// **A press on the empty space below the rows opens the menu too**, with
    /// no file under it: Delete and Copy come up dead and say why, and Paste
    /// — the one verb that is about the folder — is live.
    ///
    /// Not a nicety. Copy an impulse, make a folder for it, walk in: there is
    /// no row to right-click, and a menu that only opened over rows would put
    /// Paste out of reach in the one place somebody has just walked to in
    /// order to use it.
    #[test]
    fn the_empty_space_below_the_rows_still_answers_the_right_click() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        live.ir_entries.clear();
        live.ir_clip = "/elsewhere/library/plate.wav".to_owned();
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        // Row 4 of an empty listing is empty space by construction.
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[4].center());
        let menu = live.ir_menu.clone().expect("no menu");
        assert_eq!(menu.entry, None, "empty space claimed to be a row");
        let verbs = ir_menu_rows(&live, &menu, "");
        assert!(!verbs[0].live, "Delete was live with no file under it");
        assert!(!verbs[1].live, "Copy was live with no file under it");
        assert!(verbs[2].live, "Paste was dead in the folder it is for");
        assert!(
            verbs[0].tip.contains("right-click an impulse"),
            "the dead Delete row does not say what it wants: {:?}",
            verbs[0].tip
        );
        match press(&ctx, &st, &mut live, size, menu_rects[2].center()) {
            Some(FaceAction::PasteIr { into }) => assert_eq!(into, "/m/IRs"),
            other => panic!("Paste asked for {}", named(other.as_ref())),
        }
        // And the two dead rows swallow their press: nothing is asked for,
        // and the menu stays up so the sentence explaining them can be read.
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[4].center());
        assert!(
            press(&ctx, &st, &mut live, size, menu_rects[0].center()).is_none(),
            "a dead Delete row acted"
        );
        assert!(live.ir_menu.is_some(), "a dead row dismissed the menu");
    }

    /// **Read anywhere, write only at home.**
    ///
    /// The browser walks the whole disk, so the two writing verbs have to
    /// answer for where they are aimed. Copy is live everywhere — remembering
    /// a path changes nothing — and Delete and Paste are live only inside the
    /// IRs folder, which is the only place this app has any business writing.
    /// See [`ir_under_root`], which carries the argument.
    #[test]
    fn delete_and_paste_stop_at_the_edge_of_the_irs_folder() {
        // The line itself, componentwise: a sibling whose name merely starts
        // the same way is NOT inside.
        assert!(ir_under_root("/m/IRs", "/m/IRs"));
        assert!(ir_under_root("/m/IRs/Cabs/Marshall", "/m/IRs"));
        assert!(!ir_under_root("/m/IRs-old", "/m/IRs"));
        assert!(!ir_under_root("/m", "/m/IRs"));
        assert!(!ir_under_root("/elsewhere/library", "/m/IRs"));
        // And before the app has said where home is, nothing is home.
        assert!(!ir_under_root("/m/IRs", ""));

        let mut live = a_full_face("@tangent-ir", 2);
        live.ir_clip = "/m/IRs/plate.wav".to_owned();
        let over_a_file = |live: &FaceLive, dir: &str| IrRowMenu {
            entry: Some(2),
            name: live.ir_entries[2].name.clone(),
            dir: dir.to_owned(),
            at: egui::pos2(0.0, 0.0),
        };

        let home = ir_menu_rows(&live, &over_a_file(&live, "/m/IRs"), "");
        assert!(home[0].live && home[1].live && home[2].live, "all three sleep at home");

        // Standing in somebody else's library: Copy still works, and it is
        // the road home — copy there, walk back, paste.
        live.ir_dir = "/elsewhere/library".to_owned();
        let away = ir_menu_rows(&live, &over_a_file(&live, "/elsewhere/library"), "");
        assert!(!away[0].live, "Delete was offered outside the IRs folder");
        assert!(away[1].live, "Copy was refused outside the IRs folder");
        assert!(!away[2].live, "Paste was offered outside the IRs folder");
        for row in [&away[0], &away[2]] {
            assert!(
                row.tip.contains("IRs folder"),
                "a dead row does not say why: {:?}",
                row.tip
            );
        }
    }

    /// **Pasting onto a name that is already there is OFFERED, not refused —
    /// and the ask cannot express an overwrite.**
    ///
    /// The two failures the owner ruled out are a silent overwrite and a
    /// cryptic refusal, and the shape of the ask is what rules the first one
    /// out for good: [`FaceAction::PasteIr`] carries a FOLDER. There is no
    /// field in it that could name the file to write over, so no arm on the
    /// far side can be written that does. What the host does with the
    /// collision — number the copy and say which name it got, exactly as
    /// `desktop::save_ir` already does for two captures named the same thing
    /// — is the other half, and it is stated on `dialogs::ir_paste_ask`.
    #[test]
    fn pasting_onto_a_name_already_in_the_folder_is_offered_not_refused() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        // The clipboard holds a file whose name is ALREADY in this folder.
        let clash = live.ir_entries[3].name.clone();
        assert!(!live.ir_entries[3].dir);
        live.ir_clip = format!("/elsewhere/library/{clash}");
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[3].center());
        let menu = live.ir_menu.clone().expect("no menu");
        let verbs = ir_menu_rows(&live, &menu, "");
        assert!(
            verbs[2].live,
            "Paste refused a name already in the folder: {:?}",
            verbs[2].tip
        );
        assert!(
            verbs[2].label.contains(&clash),
            "the Paste row does not name what it would put here: {:?}",
            verbs[2].label
        );
        match press(&ctx, &st, &mut live, size, menu_rects[2].center()) {
            // The folder, and only the folder.
            Some(FaceAction::PasteIr { into }) => assert_eq!(into, "/m/IRs"),
            other => panic!("Paste asked for {}", named(other.as_ref())),
        }
    }

    /// **The menu cannot be handed a different file from the one it names.**
    ///
    /// A capture landing in the folder on screen re-lists it and the wheel
    /// scrolls it, both while a menu is open. The menu remembers the name and
    /// the folder; when either stops matching it closes, because nothing was
    /// done and the hand can press again on a row it can see.
    #[test]
    fn a_menu_over_a_listing_that_moved_closes_rather_than_aiming_somewhere_else() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());

        // The row the menu names is now a DIFFERENT file — the shape a
        // re-listing takes when a capture lands ahead of it in the sort.
        live.ir_entries[2].name = "something-else.wav".to_owned();
        let stale = live.ir_menu.clone().expect("the menu vanished on its own");
        assert!(
            ir_menu_rows(&live, &stale, "")[0].ask.is_none(),
            "Delete resolved against a row whose file had changed"
        );
        assert!(
            press(&ctx, &st, &mut live, size, menu_rects[0].center()).is_none(),
            "a menu over a moved listing still deleted something"
        );

        // **A folder change with no press at all still closes it.** The
        // walk below closes it twice over — the press lands outside the menu
        // AND the folder moves — so the folder check has to be cornered on
        // its own, or a rule this panel depends on is guarded by nothing.
        // It is not a hypothetical road: the app owns `ir_dir` too, and
        // `refresh_ir_browser` writes it.
        let _ = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        live.ir_dir = "/m/IRs/Cabs".to_owned();
        settle(&ctx, &st, &mut live, size);
        assert!(
            live.ir_menu.is_none(),
            "the menu survived the browser moving to another folder"
        );
        live.ir_dir = "/m/IRs".to_owned();

        // And a walk out from under it closes it outright: the words on a
        // menu opened in one folder mean nothing standing in another.
        let menu_rects = open_menu_on(&ctx, &st, &mut live, size, rows[2].center());
        let _ = press(&ctx, &st, &mut live, size, l.browse_up.center());
        assert_eq!(live.ir_dir, "/m");
        assert!(live.ir_menu.is_none(), "the menu survived the walk");
        // Pressing where its rows WERE is now a press on the list again —
        // which is the point: the menu is gone, so nothing it offered can
        // still be reached. Whatever the row under the pointer does, it is
        // not one of the three verbs.
        let after = press(&ctx, &st, &mut live, size, menu_rects[1].center());
        assert!(
            !matches!(
                after,
                Some(FaceAction::DeleteIr { .. })
                    | Some(FaceAction::CopyIr { .. })
                    | Some(FaceAction::PasteIr { .. })
            ),
            "a menu closed by a walk still ran a verb: {}",
            named(after.as_ref())
        );
    }

    /// **The way back from the wizard lands with the new impulse in view.**
    ///
    /// `FaceLive::ir_reveal` is a wish rather than a scroll position because
    /// the answer does not exist yet on the frame it is made: the host has
    /// still to bind the file and list the folder. So it stands until it can
    /// be honoured — and then honours itself once, and only for a file that
    /// is actually in the folder on screen.
    #[test]
    fn the_way_back_scrolls_the_impulse_it_came_to_show_into_view() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let rows = l.browse_rows();
        // Something far enough down the folder to be off the first page.
        let at = live.ir_entries.len() - 1;
        let name = live.ir_entries[at].name.clone();
        assert!(at >= rows.len(), "the fixture is too short to test a scroll");
        let path = format!("/m/IRs/{name}");
        live.face.extra = vec![("ir_path".to_owned(), path)];
        live.ir_reveal = true;
        assert_eq!(live.ir_scroll, 0);

        // One ordinary frame is all it takes, once the listing holds it.
        let _ = press(&ctx, &st, &mut live, size, l.browse.left_bottom());
        assert!(!live.ir_reveal, "the wish was not spent");
        assert!(
            live.ir_scroll > 0 && at >= live.ir_scroll && at < live.ir_scroll + rows.len(),
            "row {at} is not on screen: scroll {} of {} rows",
            live.ir_scroll,
            rows.len()
        );

        // A wish aimed at a file this folder does NOT hold stands rather than
        // scrolling somewhere arbitrary — the file may be about to arrive.
        let mut live = a_full_face("@tangent-ir", 2);
        live.face.extra = vec![("ir_path".to_owned(), "/other/place/x.wav".to_owned())];
        live.ir_reveal = true;
        let _ = press(&ctx, &st, &mut live, size, l.browse.left_bottom());
        assert!(live.ir_reveal, "a wish that could not be honoured was thrown away");
        assert_eq!(live.ir_scroll, 0, "the browser scrolled to a file it does not hold");
    }

    /// Draw one frame of the panel with `live` as it stands and hand back
    /// every rectangle filled and every run of text painted: the fill's
    /// colour and rect, and each text's colour and where it landed.
    ///
    /// **The sweep below reads this rather than the palette**, and that is
    /// the whole point of it. A theme test that measures `row_faint(&st,
    /// st.s.raised)` is measuring `row_faint`, which nobody broke; it goes on
    /// passing after the painter has been changed to write `st.s.faint`
    /// straight onto the button. Asking the picture what colour it actually
    /// is, is the only version of this test that can fail for the reason it
    /// was written.
    fn painted(
        ctx: &egui::Context,
        st: &FaceStyle,
        live: &mut FaceLive,
        size: Vec2,
    ) -> (Vec<(Rect, egui::Color32)>, Vec<(Rect, egui::Color32)>) {
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        let _ = draw(ui, st, live);
                    });
            },
        );
        let mut fills = Vec::new();
        let mut texts = Vec::new();
        for cs in &out.shapes {
            match &cs.shape {
                egui::Shape::Rect(r) if r.fill != egui::Color32::TRANSPARENT => {
                    fills.push((r.rect, r.fill));
                }
                egui::Shape::Text(t) => {
                    let r = Rect::from_min_size(t.pos, t.galley.rect.size());
                    texts.push((
                        r,
                        t.override_text_color.unwrap_or(t.fallback_color),
                    ));
                }
                _ => {}
            }
        }
        (fills, texts)
    }

    /// **Every theme dresses the row menu, in both modes — measured off the
    /// picture.**
    ///
    /// The menu is new SURFACE: a stack over the browser's well with live
    /// words, dead words and a hover shade on it. This app's most repeated
    /// fault, seven times now, is ink inherited from a role instead of
    /// measured on the ground it lands on, and every one of those seven was
    /// found in a SCREENSHOT rather than by a test — because the tests were
    /// measuring the helper that would have got it right.
    ///
    /// So this one draws the menu and reads the shapes back: for every run of
    /// text inside the stack, the fill actually painted under it, and the
    /// contrast between the two. A dead row is a caption and clears 3:1; a
    /// live one is body text and clears 4.5:1.
    #[test]
    fn every_theme_dresses_the_row_menu_in_both_modes() {
        let ctx = a_test_ctx();
        let size = Vec2::new(1000.0, 620.0);
        let l = ir_layout(&ctx, size);
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mode = if dark { "dark" } else { "light" };
                let at = format!("{} {mode}", theme.name);

                // A menu over a FOLDER row: Delete and Copy come up dead
                // (a folder is not an impulse) and Paste is live, so one
                // frame carries both kinds of word.
                let mut live = a_full_face("@tangent-ir", 2);
                live.ir_clip = "/elsewhere/plate.wav".to_owned();
                let rows = l.browse_rows();
                live.ir_menu = Some(IrRowMenu {
                    entry: Some(0),
                    name: live.ir_entries[0].name.clone(),
                    dir: live.ir_dir.clone(),
                    at: rows[0].center(),
                });
                let verbs = ir_menu_rows(
                    &live,
                    live.ir_menu.as_ref().expect("the fixture set a menu"),
                    "",
                );
                assert!(
                    verbs.iter().any(|v| v.live) && verbs.iter().any(|v| !v.live),
                    "the fixture stopped carrying both a live and a dead row"
                );
                let stack = l
                    .menu_rows(rows[0].center())
                    .iter()
                    .fold(Rect::NOTHING, |a, r| a.union(*r));
                let (fills, texts) = painted(&ctx, &st, &mut live, size);

                // The stack has to be FINDABLE over the pane it covers, or a
                // menu is three words floating on a list. Read off the fill
                // that was actually painted at the stack's own rectangle.
                let ground = fills
                    .iter()
                    .filter(|(r, _)| r.contains_rect(stack.shrink(1.0)) && r.width() < l.browse.width())
                    .map(|(_, c)| *c)
                    .next_back();
                match ground {
                    None => bad.push(format!("{at}: the menu painted no ground of its own")),
                    Some(c) => {
                        let over = crate::recorder_panel::contrast_ratio(c, st.s.well);
                        if over < 1.1 {
                            bad.push(format!("{at}: the menu is {over:.2}:1 on the pane"));
                        }
                    }
                }
                let ground = ground.unwrap_or(st.s.raised);

                // And every word inside it, against that ground. The floor
                // is a caption's, because the dead rows are the ones somebody
                // stops to read and they are the quietest thing here.
                let mut words = 0;
                for (r, ink) in &texts {
                    // A run with no width is a galley that laid out nothing,
                    // which is a row with no word on it however honest its
                    // colour is. Counted out, so the tally below notices.
                    if !stack.expand(1.0).contains_rect(*r) || r.width() < 1.0 {
                        continue;
                    }
                    words += 1;
                    let ratio = crate::recorder_panel::contrast_ratio(*ink, ground);
                    if ratio < 3.0 {
                        bad.push(format!("{at}: a menu word is {ratio:.2}:1 on its ground"));
                    }
                }
                if words < IR_MENU_ROWS {
                    bad.push(format!("{at}: only {words} of three verbs were painted"));
                }
                // The live words are body text and owe the full bar. Read
                // from the palette, because THAT is what the painter writes
                // for a live row — and the picture above has already proved
                // it is the palette the painter is reading.
                let live_ink = crate::recorder_panel::contrast_ratio(st.s.ink, ground);
                if live_ink < 4.5 {
                    bad.push(format!("{at}: a live verb is {live_ink:.2}:1"));
                }
                let hovered = crate::recorder_panel::contrast_ratio(st.s.ink, st.s.hover);
                if hovered < 4.5 {
                    bad.push(format!("{at}: a live verb under the pointer is {hovered:.2}:1"));
                }
                // Dead has to READ as dead beside live, or greying it out
                // says nothing at all.
                let apart =
                    crate::recorder_panel::contrast_ratio(row_faint(&st, ground), st.s.ink);
                if apart < 1.15 {
                    bad.push(format!("{at}: a dead verb is only {apart:.2}:1 from a live one"));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the row menu is unreadable on some theme:\n  {}",
            bad.join("\n  ")
        );
    }

    /// An empty folder and a filename nobody could have meant both lay out
    /// inside the panel — on all FOUR sides.
    ///
    /// The long name is the one that would run off the right if it were
    /// printed whole, and the bottom edge is checked here and nowhere else
    /// because this is the only face that fills the rect it is given rather
    /// than stacking down it: its foot has to be enough for the preset strip
    /// that lands in it — see `preset_foot_height` — and a foot a point short
    /// would push the strip off the bottom of the window.
    #[test]
    fn an_empty_folder_and_an_absurd_filename_both_lay_out() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        for entries in [Vec::new(), a_folder()] {
            for (w, h) in [(1000.0f32, 620.0f32), (620.0, 420.0), (296.0, 300.0)] {
                let mut live = a_full_face("@tangent-ir", 2);
                live.ir_entries = entries.clone();
                let (took, painted) = extent(&ctx, w, h, &mut |ui| {
                    let _ = draw(ui, &st, &mut live);
                });
                let panel = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(w, h))
                    .expand(1.0);
                assert!(panel.contains_rect(took), "{w}x{h}: took {took:?}");
                assert!(panel.contains_rect(painted), "{w}x{h}: painted {painted:?}");
            }
        }
    }



    /// **The preset strip can never land inside the browser.**
    ///
    /// Three lanes built this panel at once and each assumed another had
    /// done the sizing, and this is where it showed: `draw_ir` allocates the
    /// body up front so the cursor lands on the foot, and then draws the
    /// Wizard with `ui.put`, which advances the cursor AFTER its child —
    /// `Layout::advance_after_rects` sets the cursor to that child's bottom
    /// rather than taking the further of the two. The Wizard sits at the
    /// foot of the LEFT HALF, so on every stacked width — which is every
    /// width the face actually opens at — the cursor came back up into the
    /// middle of the pane and the strip drew itself across the browser.
    ///
    /// The contract is the cursor, so that is what this asks, at a walk of
    /// widths either side of the split rather than at one size: after the
    /// panel has drawn, the next thing to be laid out starts at the foot.
    #[test]
    fn the_preset_strip_never_lands_inside_the_browser_at_any_width() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let mut bad: Vec<String> = Vec::new();
        let mut stacked = 0;
        for pad in [Vec2::new(4.0, 1.0), Vec2::new(12.0, 4.0)] {
            padded(&ctx, pad);
            for w in [
                200.0f32, 260.0, 296.0, 340.0, 400.0, 460.0, 519.0, 520.0, 560.0,
                700.0, 900.0, 1160.0,
            ] {
                for h in [300.0f32, 420.0, 620.0, 700.0] {
                    let mut live = a_full_face("@tangent-ir", 2);
                    let mut seen = None;
                    let _ = ctx.run(
                        egui::RawInput {
                            screen_rect: Some(Rect::from_min_size(
                                egui::pos2(0.0, 0.0),
                                Vec2::new(w, h),
                            )),
                            ..Default::default()
                        },
                        |ctx| {
                            egui::CentralPanel::default()
                                .frame(egui::Frame::NONE)
                                .show(ctx, |ui| {
                                    let l = IrLayout::for_ui(ui);
                                    let _ = draw_ir(ui, &st, &mut live);
                                    seen = Some((l, ui.cursor().top()));
                                });
                        },
                    );
                    let (l, cursor) = seen.expect("the panel never drew");
                    if l.stacked {
                        stacked += 1;
                    }
                    if cursor < l.body.bottom() - 0.01 {
                        bad.push(format!(
                            "{pad:?} {w}x{h}: the strip starts at {cursor:.1}, \
                             {:.1} points inside a pane that ends at {:.1}",
                            l.body.bottom() - cursor,
                            l.body.bottom(),
                        ));
                    }
                }
            }
        }
        assert!(stacked > 0, "no width in the sweep stacks, so this proves nothing");
        assert!(
            bad.is_empty(),
            "the preset strip is drawn over the browser:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **And the ".." chip still takes a press with the strip under it.**
    ///
    /// The cost of the fault above, said the way the owner would meet it: a
    /// strip drawn across the header is a strip whose own widgets are on
    /// top of the chip, and egui gives a click to the last thing registered
    /// at the point — so the browser lost the one control that gets it back
    /// out of a folder. A stacked panel, which is what the face opens as.
    #[test]
    fn the_up_chip_still_walks_out_of_a_folder_with_the_strip_below_it() {
        let ctx = a_test_ctx();
        padded(&ctx, Vec2::new(12.0, 4.0));
        let st = a_test_style();
        for size in [
            Vec2::new(400.0, 420.0),
            Vec2::new(460.0, 620.0),
            Vec2::new(519.0, 500.0),
        ] {
            let l = ir_layout(&ctx, size);
            assert!(l.stacked, "{size:?} is not a stacked panel");
            let mut live = a_full_face("@tangent-ir", 2);
            live.ir_dir = "/m/IRs/Cabs".to_owned();
            let _ = press(&ctx, &st, &mut live, size, l.browse_up.center());
            assert_eq!(
                live.ir_dir, "/m/IRs",
                "{size:?}: the UP chip took no press — \
                 something is drawn on top of it"
            );
        }
    }

    /// **The foot reserves what the strip actually takes.**
    ///
    /// `IR_FOOT_H` was a flat thirty-two points, guessed once from a strip
    /// on one line under one style — the same four-fixed-numbers disease
    /// `preset_strip` had already been cured of. A strip that wraps needs
    /// half again as much, and what it does not get it takes: the whole face
    /// runs off the bottom of the window it was handed.
    ///
    /// So this sweeps into the widths where the row really does wrap, under
    /// both styles, and asserts that it does — a sweep that never wrapped
    /// would pass against the bug it is meant to indict.
    #[test]
    fn the_foot_reserves_what_the_preset_strip_actually_takes() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let mut bad: Vec<String> = Vec::new();
        let mut wrapped = 0;
        for pad in [Vec2::new(4.0, 1.0), Vec2::new(12.0, 4.0)] {
            padded(&ctx, pad);
            for w in [
                160.0f32, 180.0, 200.0, 220.0, 240.0, 260.0, 280.0, 296.0, 300.0,
                340.0, 400.0, 520.0, 700.0, 1000.0,
            ] {
                let h = 620.0;
                // What the layout reserves, against what the strip really
                // takes when it is drawn at that width. The reservation
                // starts one `item_spacing` above the strip, because that is
                // where the body's own allocation leaves the cursor.
                let mut seen = None;
                let mut live = a_full_face("@tangent-ir", 2);
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(
                            egui::pos2(0.0, 0.0),
                            Vec2::new(w, h),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                let reserved = preset_foot_height(ui);
                                let lines = preset_parts(ui, ui.available_width()).lines;
                                let gap = ui.spacing().item_spacing.y;
                                let drew = ui
                                    .scope(|ui| {
                                        let _ = preset_strip(ui, &st, &mut live);
                                    })
                                    .response
                                    .rect
                                    .height();
                                seen = Some((reserved, lines, gap + drew));
                            });
                    },
                );
                let (reserved, lines, took_foot) = seen.expect("the strip never drew");
                if lines > 1 {
                    wrapped += 1;
                }
                if reserved < took_foot - 0.01 {
                    bad.push(format!(
                        "{pad:?} {w} wide, strip on {lines}: reserved {reserved:.1} \
                         for a foot that takes {took_foot:.1}"
                    ));
                }
                let mut live = a_full_face("@tangent-ir", 2);
                let (took, painted) = extent(&ctx, w, h, &mut |ui| {
                    let _ = draw(ui, &st, &mut live);
                });
                let panel =
                    Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(w, h)).expand(1.0);
                if !panel.contains_rect(took) {
                    bad.push(format!(
                        "{pad:?} {w} wide, strip on {lines}: took to {:.1} of {h}",
                        took.bottom()
                    ));
                }
                if !panel.contains_rect(painted) {
                    bad.push(format!(
                        "{pad:?} {w} wide, strip on {lines}: painted to {:.1} of {h}",
                        painted.bottom()
                    ));
                }
            }
        }
        assert!(
            wrapped > 0,
            "no width in the sweep wraps the strip, so this proves nothing"
        );
        assert!(
            bad.is_empty(),
            "the face runs out of the window it was given:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **The list does not flinch when it is pressed.**
    ///
    /// `face::a_dragged_row_never_moves_the_rows_under_it` is the same
    /// proof for the kit rows, and it exists because the obvious spelling of
    /// "keep the first ask" stops drawing everything under whichever widget
    /// acted. A browser that dropped its lower rows on the frame a press
    /// landed would strobe under every click, and — worse — no row could
    /// ever see a second press, because a widget that is absent every other
    /// frame is a widget nothing can be double-clicked on.
    ///
    /// So: count what gets painted inside the list on a resting frame, then
    /// on the press and the release, and demand the three agree.
    #[test]
    fn the_list_does_not_flinch_on_the_frame_it_is_pressed() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let size = Vec2::new(1000.0, 620.0);
        let mut live = a_full_face("@tangent-ir", 2);
        let l = ir_layout(&ctx, size);
        let at = l.browse_rows()[4].center();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let mut counts = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(at)],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Default::default(),
            }],
            vec![egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: Default::default(),
            }],
        ] {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            let _ = draw(ui, &st, &mut live);
                        });
                },
            );
            // The WORDS in the list, not every shape in it: the row under
            // the pointer grows a highlight on the frames it is hovered,
            // and that appearing is the panel working rather than flinching.
            // A row that stopped being drawn takes its name with it.
            counts.push(
                out.shapes
                    .iter()
                    .filter(|cs| {
                        let b = cs.shape.visual_bounding_rect();
                        matches!(cs.shape, egui::Shape::Text(_))
                            && b.is_positive()
                            && l.browse_list.contains_rect(b)
                    })
                    .count(),
            );
        }
        assert!(
            counts.windows(2).all(|w| w[0] == w[1]),
            "the list flinched under the press: {counts:?}"
        );
        assert!(counts[0] > 10, "nothing was drawn to flinch: {counts:?}");
    }

    /// **The wheel walks the listing, and stops at the ends.** The drumpad
    /// explorer's own feel, from the drumpad's own accrual: three rows a
    /// NOTCH of intent, so one mouse notch and a thirty-event trackpad flick
    /// mean the same amount of walking rather than the flick throwing the
    /// list end to end.
    #[test]
    fn the_wheel_walks_the_listing_and_stops_at_both_ends() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        // A short panel, so a listing of thirty-three has somewhere to go.
        let size = Vec2::new(1000.0, 260.0);
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let l = ir_layout(&ctx, size);
        let over = l.browse_list.center();
        let mut live = a_full_face("@tangent-ir", 2);
        let rows = l.browse_rows().len();
        assert!(
            live.ir_entries.len() > rows,
            "the folder fits the pane, so there is nothing to scroll"
        );
        let turn = |live: &mut FaceLive, delta: f32, n: usize| {
            for _ in 0..n {
                let _ = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events: vec![
                            egui::Event::PointerMoved(over),
                            egui::Event::MouseWheel {
                                unit: egui::MouseWheelUnit::Point,
                                delta: egui::vec2(0.0, delta),
                                modifiers: Default::default(),
                            },
                        ],
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                let _ = draw(ui, &st, live);
                            });
                    },
                );
            }
        };
        // One notch, one step of three rows — a wheel that walked per EVENT
        // would have thrown the list to its end on the flick below.
        turn(&mut live, -40.0, 1);
        assert_eq!(live.ir_scroll, 3, "one notch did not walk three rows");
        // A flick: thirty small events worth three notches in total.
        turn(&mut live, -4.0, 30);
        assert_eq!(live.ir_scroll, 12, "the flick walked {} rows", live.ir_scroll);
        // Down to the end and no further: a browser scrolled into empty
        // space is a browser that looks broken with nothing wrong under it.
        turn(&mut live, -40.0, 40);
        assert_eq!(live.ir_scroll, live.ir_entries.len() - rows);
        // And back to the top, which is where it stops.
        turn(&mut live, 40.0, 60);
        assert_eq!(live.ir_scroll, 0);
    }

}
