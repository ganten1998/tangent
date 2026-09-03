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
/// Two things fix it and both are needed. The shares below keep the strip on
/// ONE line at every width a face is opened at, and `horizontal_wrapped`
/// makes the failure mode at any width below that a SECOND LINE rather than
/// a part that is simply not there — an item that will not fit the rest of
/// the row starts a new one instead of running off the end. A layout that
/// cannot overflow is worth more than a layout whose arithmetic happens to
/// be right today.
fn preset_strip(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let mut action = None;
    ui.add_space(6.0);
    let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());
    // The two fixed parts are MEASURED, not guessed: a button is its text
    // plus the theme's own padding, and both of those move with the font
    // and the style. Guessing them is how a strip gets to be 330 wide.
    let word_w = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_owned(), bold(10.0), egui::Color32::WHITE)
            .rect
            .width()
    };
    let pad = ui.spacing().button_padding.x * 2.0;
    let gap = ui.spacing().item_spacing.x;
    let save_w = word_w("Save") + pad;
    let caption_w = word_w("PRESET");
    let w = ui.available_width();
    // The caption is the one part that is pure decoration, so it is the
    // first thing the budget spends: a narrow strip says what it is by
    // being a menu and a name field, which nobody mistakes for anything.
    let captioned = w >= caption_w + gap + PRESET_MIN_PARTS;
    let fixed =
        save_w + gap * 2.0 + if captioned { caption_w + gap } else { 0.0 };
    let free = (w - fixed).max(PRESET_MIN_PARTS);
    // The menu takes a little over half of what is left; the field takes
    // the rest. Both stop growing well before the panel does — a preset
    // menu three hundred points wide is not more useful than one at a
    // hundred and sixty, and the empty run to its right is what tells the
    // eye the strip is a footer rather than the panel's subject.
    let combo_w = (free * 0.52).clamp(PRESET_MIN_COMBO, 160.0);
    let name_w = (free - combo_w).clamp(PRESET_MIN_NAME, 160.0);
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
/// What the preset strip needs under the panel — its own `add_space`, a row
/// of buttons, and the descender room a text field asks for.
const IR_FOOT_H: f32 = 32.0;
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
    pub fn new(rect: Rect) -> Self {
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
        let foot_h = IR_FOOT_H.min(rect.height() * 0.25);
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
}

/// What the two kit rows need when the panel is stacked rather than split.
/// `slider_row` draws at eighteen points and `ui.horizontal` adds its own
/// spacing; two of them plus that is what the number is.
const IR_STACKED_CONTROLS: f32 = 48.0;

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
    let l = IrLayout::new(ui.available_rect_before_wrap());
    if !l.body.is_positive() {
        return None;
    }
    // Everything above the foot is drawn by hand into `l`; taking the body
    // in one allocation is what leaves the cursor on the preset strip's own
    // line and keeps this panel from fighting the layout above it.
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
    for (i, r) in rows.iter().enumerate() {
        let Some(e) = live.ir_entries.get(live.ir_scroll + i) else {
            break;
        };
        let path = std::path::Path::new(&dir)
            .join(&e.name)
            .to_string_lossy()
            .into_owned();
        let here = !e.dir && path == loaded;
        let hit = ui.interact(*r, ui.id().with(("ir-row", i)), egui::Sense::click());
        if here || hit.hovered() {
            ui.painter()
                .rect_filled(*r, 1.5, if here { st.s.raised } else { st.s.hover });
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
        let tail_w = if tail.is_empty() {
            0.0
        } else {
            ui.painter()
                .layout_no_wrap(tail.clone(), bold(10.0), st.s.faint)
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
                st.s.faint,
            );
        }
        let hit = hit.on_hover_cursor(egui::CursorIcon::PointingHand);
        let hit = hit.on_hover_text(if e.dir {
            format!("{}\n\nclick to walk in", e.name)
        } else {
            format!("{path}\n\nclick to load this impulse")
        });
        if hit.clicked() {
            if e.dir {
                walk_to = Some(path);
            } else if !here {
                // **A browser, not a picker.** One press loads; there is no
                // OK button to hunt for, and the readout above says what
                // took.
                out = Some(FaceAction::PickIr { path });
            }
        }
    }
    if let Some(into) = walk_to {
        live.ir_dir = into;
        live.ir_scroll = 0;
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
        for (w, h) in [
            (1160.0f32, 700.0f32), // the widest the drumpad ever opens
            (1000.0, 620.0),       // a drumpad-sized window
            (620.0, 420.0),
            (520.0, 360.0), // the last width that still splits
            (519.0, 360.0), // and the first that stacks
            (400.0, 300.0),
            (296.0, 300.0), // the narrowest a dialog can be dragged to
            (200.0, 200.0),
        ] {
            let rect = Rect::from_min_size(egui::pos2(17.0, 23.0), Vec2::new(w, h));
            let l = IrLayout::new(rect);
            let at = format!("{w}x{h}");
            assert!(l.body.is_positive(), "{at}: no body");
            // The panel divides into a body and a foot, with nothing lost
            // between them — the preset strip's room is the foot.
            assert!((l.body.bottom() - l.foot.top()).abs() < 0.01, "{at}");
            assert!((l.foot.bottom() - rect.bottom()).abs() < 0.01, "{at}");
            assert!(l.foot.height() > 0.0, "{at}: no room for the presets");
            // The two halves share the body and never each other.
            assert!(l.body.expand(0.01).contains_rect(l.left), "{at}: left");
            assert!(l.body.expand(0.01).contains_rect(l.browse), "{at}: browse");
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
        let l = IrLayout::new(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            Vec2::new(1000.0, 620.0),
        ));
        assert!(!l.stacked, "a wide panel should split");
        assert!(l.browse.width() > 380.0, "the browser is a sliver: {:?}", l.browse);
        assert!(l.browse_rows().len() >= 20, "only {} rows", l.browse_rows().len());
        // And a panel dragged well under the drumpad's own floor still has a
        // browser — the whole width of one, stacked under the controls.
        let narrow =
            IrLayout::new(Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 360.0)));
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
        let l = IrLayout::new(Rect::from_min_size(
            egui::pos2(0.0, 0.0),
            Vec2::new(f32::INFINITY, f32::INFINITY),
        ));
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
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), size);
        let mut asked = None;
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
        let l = IrLayout::new(Rect::from_min_size(egui::pos2(0.0, 0.0), size));
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
        let l = IrLayout::new(Rect::from_min_size(egui::pos2(0.0, 0.0), size));
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

    /// An empty folder and a filename nobody could have meant both lay out
    /// inside the panel — on all FOUR sides.
    ///
    /// The long name is the one that would run off the right if it were
    /// printed whole, and the bottom edge is checked here and nowhere else
    /// because this is the only face that fills the rect it is given rather
    /// than stacking down it: its foot is a number (`IR_FOOT_H`) that has to
    /// be enough for the preset strip that lands in it, and a foot that was
    /// a point short would push the strip off the bottom of the window.
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
        let l = IrLayout::new(Rect::from_min_size(egui::pos2(0.0, 0.0), size));
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
        let l = IrLayout::new(screen);
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

    /// **The browser is legible on every theme, in both modes, or the build
    /// fails.** `face::every_theme_dresses_a_face`'s sweep, extended to the
    /// three surfaces the browser added: the well the list is drawn on, the
    /// raised chip in its header, and the row the pointer is over. Ink is
    /// measured on the surface it actually lands on, never inherited — the
    /// law seven separate bugs in this app were found breaking.
    #[test]
    fn the_ir_browser_wears_every_theme_legibly() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mode = if dark { "dark" } else { "light" };
                let mut check = |what: &str, fg: egui::Color32, bg: egui::Color32, floor: f32| {
                    let r = crate::recorder_panel::contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // The pane has to be findable on the panel before anything
                // written on it can be read.
                check("the browser well", st.s.well, st.s.bg, 1.1);
                check("a row on the well", st.s.ink, st.s.well, 4.5);
                check("the path on the well", st.s.faint, st.s.well, 3.0);
                // The loaded row and the row under the pointer are surfaces
                // of their own, and the same ink lands on both.
                check("the loaded row", st.s.ink, st.s.raised, 4.5);
                check("the hovered row", st.s.ink, st.s.hover, 4.5);
                check("the loaded row on the well", st.s.raised, st.s.well, 1.1);
                check("the hovered row on the well", st.s.hover, st.s.well, 1.1);
                // A live header chip wears the raised ground and the panel
                // ink; a chip with nowhere to go loses the ground and prints
                // `faint` straight onto the well, because `faint` on
                // `raised` misses the caption floor on three shipping themes
                // and the answer to that is to move the SURFACE.
                check("a header chip", st.s.raised, st.s.well, 1.1);
                check("a chip with nowhere to go", st.s.faint, st.s.well, 3.0);
            }
        }
        assert!(
            bad.is_empty(),
            "the IR browser goes blind:\n  {}",
            bad.join("\n  ")
        );
    }
}
