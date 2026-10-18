//! Tangent Tape's face: a machine in four sections, in the order the signal
//! meets them.
//!
//! **Why sections, and why these four.** A tape machine divides its own
//! labour, and its controls sit where that labour happens: the deck that
//! pulls the tape past the heads (the capstan and the reels, and the speed
//! they run at), the record electronics that put the signal on the tape
//! (the record level, and the bias trim aligned beside it), the tape itself
//! (its noise, its age, its damage), and the reproduce electronics that take
//! the signal back off (the head's low-end contour, the top, the output).
//! TRANSPORT, RECORD, TAPE and PLAYBACK, captioned and in that order, so a
//! hand that knows a machine finds each control where a machine keeps it,
//! and a hand that does not learns the signal's road by reading down.
//!
//! **The order inside each.** Speed leads the transport: it is the format,
//! it re-voices nearly everything else on the panel, and on a deck it is
//! the selector a hand sets before anything else. Wow Range sits directly
//! under Wow because it changes Wow and nothing else, and Rate closes the
//! group because it scales the rotating parts of both. Drive, the record
//! level a hand rides, comes before the trim it is aligned against. Hiss
//! leads the tape, the one a fresh slot engages; Age and Damage open at
//! nothing. And the reproduce chain is in its own order, the output last.
//!
//! **Tape start and stop is pointed at, never held.** The tape spinning up
//! on Play and winding down on Stop is the whole desk's, a setting saved
//! with the project and kept under the varispeed dial's range, and an insert
//! that held a copy of it would be a second switch onto one motor. So the
//! transport closes on a row that says how the setting stands and where it
//! is set, and a click there asks the app to take the hand to it; which
//! window steps aside is the app's to decide. The row is drawn every frame
//! whether or not the setting was pushed, so its arriving moves nothing.
//!
//! **Kinds come from the push, never from the id.** A control the push
//! gives steps is the kit's selector, wearing the words `builtin_fx::choices`
//! pushes; every other is the kit's slider, reading what `builtin_fx::describe`
//! prints. So a kernel that changes a control's kind changes the panel with
//! it, which is the generic rows' own rule. The one choice made by id is
//! Bias's: a trim about the point the machine is aligned at, it draws as
//! `face::centred_row`, marked and caught at its pushed default, which is
//! the kernel's word for aligned and never a number spelled here. A control
//! the panel has no place for yet is still drawn, under the four sections,
//! so the engine can grow ahead of its face and never lose a dial.
//!
//! **The ids are restated** because this crate cannot see the engine's table
//! (the firewall `faces::SUITE_UNITS` documents). `builtin_fx`'s tests hold
//! each one to its symbol at whatever prefix of the table the engine has
//! landed, and the panel draws any prefix: a section whose ids are not
//! pushed yet draws its caption and nothing under it.
//!
//! **The air between the sections is spent out of proven room**, the amp's
//! rule. [`floor_cost`] is everything the panel and its footer must have,
//! the air takes only what the window holds past that, and a row never
//! shrinks. The bypass row and the preset strip arrive from `faces::draw`,
//! as they do for every face, and the floor counts them.

use egui::{FontId, Vec2};

use crate::face::{self, FaceStyle};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;

/// The tape's sentinel path; the engine's `builtin_fx::TAPE_PATH`. The UI's
/// one spelling of it in code: `faces::SUITE_UNITS` keeps its own literal
/// row, so the line under the name can change without touching this file,
/// and a test here holds the two together.
pub const TAPE_PATH: &str = "@tangent-tape";

/// The parameter ids, in the engine's `TAPE_FACE_PARAMS` order: dense, each
/// its row's index, append-only forever. Restated; see the module header.
pub const P_DRIVE: u32 = 0;
pub const P_BUMP: u32 = 1;
pub const P_ROLL: u32 = 2;
/// The wow dial, the slow half of the transport's instability. Its symbol
/// is `wobble`, which is what a saved session carries, whatever the dial is
/// called on the panel.
pub const P_WOBBLE: u32 = 3;
pub const P_HISS: u32 = 4;
pub const P_TRIM: u32 = 5;
pub const P_SPEED: u32 = 6;
pub const P_FLUTTER: u32 = 7;
pub const P_RATE: u32 = 8;
pub const P_WOW_RANGE: u32 = 9;
pub const P_BIAS: u32 = 10;
pub const P_AGE: u32 = 11;
pub const P_DAMAGE: u32 = 12;

/// One past the highest id the panel places: what a dense harness numbers
/// its parameters up to (`face::fit::FACES`), and what `builtin_fx` holds
/// every declared row under.
pub const ID_CEILING: usize = 13;

/// The ids the panel expects as selectors, for the shared sweeps: the two
/// the engine's table gives positions, the four speeds and the two wow
/// ranges. The panel itself reads a control's kind off the push.
pub const STEPPED: &[u32] = &[P_SPEED, P_WOW_RANGE];

/// The four sections, in signal order, each with its controls in the order
/// they are drawn. See the module header for both orders.
pub const SECTIONS: [(&str, &[u32]); 4] = [
    ("TRANSPORT", &[P_SPEED, P_WOBBLE, P_WOW_RANGE, P_FLUTTER, P_RATE]),
    ("RECORD", &[P_DRIVE, P_BIAS]),
    ("TAPE", &[P_HISS, P_AGE, P_DAMAGE]),
    ("PLAYBACK", &[P_BUMP, P_ROLL, P_TRIM]),
];

/// The air between two sections when the window can afford it, and the
/// first thing given up when it cannot: the amp's own figure for the air
/// between its groups.
const GROUP_AIR: f32 = 6.0;

/// Where tape start and stop is set, as the pointer's second line says it:
/// on the band, by the name the band prints on the dial (its `PITCH CONT`),
/// so the word a hand reads here is the word it looks for there.
pub const START_STOP_WHERE: &str = "set on the PITCH CONT dial";

/// What the pointer says to a hand resting on it: what the setting does,
/// where it is kept, and what a click here does.
const START_STOP_HOVER: &str = "the whole desk spins up on Play and winds down on Stop or Pause, saved with the project. It is set under the varispeed dial's range (right-click the band's PITCH CONT dial); click here to go there";

/// **The panel**: four captioned sections in signal order, the transport
/// closing on its pointer to tape start and stop, then any control no
/// section places, all above the footer `faces::draw` appends. Every
/// caption, every pushed control and the pointer are drawn every frame and
/// their asks are folded with [`face::keep`], the kit's law, so a hand on
/// one row never moves the rows under it. At most one ask per frame, the
/// face contract.
pub fn draw_tape(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let air = section_air(ui, live);
    let mut out = None;
    for (k, (title, ids)) in SECTIONS.iter().enumerate() {
        if k > 0 {
            ui.add_space(air);
        }
        caption(ui, st, title);
        for &id in *ids {
            face::keep(&mut out, control(ui, st, live, id));
        }
        // Start and stop is the transport's too, and the desk's: the
        // section's last row points at where it is set.
        if k == 0 {
            face::keep(&mut out, start_stop_pointer(ui, st, live));
        }
    }
    // Whatever the engine has grown that no section places yet, in id
    // order: the generic rows' promise, kept under the sections, so a
    // control that lands before its place on the panel is still a control.
    for p in &live.face.params {
        if !placed(p.id) {
            face::keep(&mut out, control(ui, st, live, p.id));
        }
    }
    out
}

/// One control by id, as its kind draws it: the kit's selector when the
/// push gives it steps, the centred row for Bias, the slider for the rest.
/// A control the push does not carry draws nothing and asks nothing, which
/// is what lets the panel land at any prefix of the engine's table.
fn control(ui: &mut egui::Ui, st: &FaceStyle, live: &FaceLive, id: u32) -> Option<FaceAction> {
    let p = live.face.params.iter().find(|p| p.id == id)?;
    if p.steps >= 2 {
        crate::faces::step_row(ui, st, &live.face, id, None)
    } else if id == P_BIAS {
        face::centred_row(ui, st, &p.name, &p.reading, p.value, p.default)
            .map(|ask| crate::faces::ask_to_action(id, ask))
    } else {
        crate::faces::row(ui, st, &live.face, id)
    }
}

/// Whether one of the four sections draws `id`.
fn placed(id: u32) -> bool {
    SECTIONS.iter().any(|(_, ids)| ids.contains(&id))
}

/// **The transport's pointer to tape start and stop**: the kit's pointer
/// row, reading how the desk's setting stands and where it is set, and a
/// click on it is the ask to be taken there. See the module header for why
/// it is a pointer and not a control.
fn start_stop_pointer(ui: &mut egui::Ui, st: &FaceStyle, live: &FaceLive) -> Option<FaceAction> {
    face::pointer_row(ui, st, &start_stop_line(live.start_stop), START_STOP_WHERE, START_STOP_HOVER)
        .then_some(FaceAction::OpenStartStop)
}

/// The pointer's first line: where tape start and stop stands. Its two times
/// as the range panel prints them, through the setting's own reading, and
/// nothing but the name when nobody pushed it.
pub fn start_stop_line(ss: Option<crate::motor::StartStop>) -> String {
    match ss {
        None => "Start and stop".to_owned(),
        Some(s) if !s.on() => "Start and stop: off".to_owned(),
        Some(s) => format!(
            "Start and stop: {} up, {} down",
            crate::motor::StartStop::reading(s.start_cs()),
            crate::motor::StartStop::reading(s.stop_cs()),
        ),
    }
}

/// One caption line: the section's title in the caption tier, faint on the
/// panel's ground. ONE allocation of exactly one nine-point line, with the
/// title painted into it. Laid out the ordinary way, a caption that shares
/// its line with anything (a figure on the right, say) goes through
/// `ui.horizontal`, which starts every row at `interact_size.y`: six to
/// eight points more than one line, which [`floor_cost`] cannot afford four
/// times over. Allocated at its own height, the line costs the same whatever
/// it carries, and it takes one auto id whatever it carries.
fn caption(ui: &mut egui::Ui, st: &FaceStyle, title: &str) {
    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), caption_height(ui)),
        egui::Sense::hover(),
    );
    ui.painter().text(
        rect.left_top(),
        egui::Align2::LEFT_TOP,
        title,
        FontId::new(9.0, fonts::courier_bold()),
        st.s.faint,
    );
}

/// A caption line's height: one nine-point line, bold or regular, whichever
/// the installed face draws taller. [`caption`] allocates it and
/// [`floor_cost`] counts it, so the two cannot disagree.
fn caption_height(ui: &egui::Ui) -> f32 {
    line(ui, 9.0, fonts::courier_bold()).max(line(ui, 9.0, fonts::courier()))
}

/// How tall one line of `family` at `size` lays out on this `Ui`'s fonts:
/// the `faces_amp::amp_floor_cost` idiom, measured and never typed, because
/// a nine-point line is ten points on one face the app ships and twelve on
/// another.
fn line(ui: &egui::Ui, size: f32, family: egui::FontFamily) -> f32 {
    ui.painter()
        .layout_no_wrap("Xg".to_owned(), FontId::new(size, family), egui::Color32::WHITE)
        .rect
        .height()
}

/// **What the panel may not spend**, in points, on the font and spacing this
/// frame is drawn with: four captions, every pushed control at the height
/// its kind is drawn at, the transport's pointer, and the footer
/// `faces::draw` appends under it. It is exactly the distance from the
/// panel's top to the bottom of the preset strip with no air anywhere, so
/// whatever the room holds past it is air the panel can prove it has.
///
/// Every figure is the one egui sizes the part at. A slider row is the
/// tallest of the kit's row floor, its eleven-point name and the slider's
/// own thickness; a selector row its two buttons, each its text and the
/// theme's padding, set side by side the way egui sets them; the pointer is
/// asked of [`face::pointer_row_height`], the height it is drawn at; and the
/// footer is asked of the two functions that reserve it,
/// [`crate::faces::bypass_foot_height`] stopping at its button and
/// [`crate::faces::preset_foot_height`] opening with the gap under it. A
/// control is counted only when it is pushed, so the panel lands at any
/// prefix of the engine's table with its air worked out for what is there.
fn floor_cost(ui: &egui::Ui, live: &FaceLive) -> f32 {
    let gap = ui.spacing().item_spacing.y;
    let pad_y = ui.spacing().button_padding.y * 2.0;
    let least = face::ROW_H.max(ui.spacing().interact_size.y);
    let bold = |size: f32| line(ui, size, fonts::courier_bold());
    let slider = least
        .max(bold(11.0))
        .max(ui.text_style_height(&egui::TextStyle::Body));
    // **A selector is the kit's stepped row**: its name, the body wearing
    // the choice, then the chevron, side by side. egui hands each part a
    // frame a row's floor tall, centred on the row as it stands when the
    // part arrives, and a part taller than its frame grows down from the
    // frame's top. So a body taller than the floor sets the chevron, the
    // tallest part, half its overhang down, and the row comes out that much
    // taller than its tallest button: half a point on two of the three
    // fonts, a point and a half on the third.
    let beside = |row: f32, part: f32| row.max((row - face::ROW_H) / 2.0 + part.max(face::ROW_H));
    let selector = beside(beside(least, bold(10.0) + pad_y), bold(11.0) + pad_y);
    let rows: f32 = live
        .face
        .params
        .iter()
        .map(|p| (if p.steps >= 2 { selector } else { slider }) + gap)
        .sum();
    SECTIONS.len() as f32 * (caption_height(ui) + gap)
        + rows
        + face::pointer_row_height(ui)
        + gap
        + crate::faces::bypass_foot_height(ui)
        + crate::faces::preset_foot_height(ui)
}

/// The air between two sections: an equal share of whatever the room holds
/// past [`floor_cost`], up to [`GROUP_AIR`], and none at all when it holds
/// nothing past it. Shared among the seams, three for four sections.
fn section_air(ui: &egui::Ui, live: &FaceLive) -> f32 {
    let seams = (SECTIONS.len() - 1) as f32;
    ((ui.available_height() - floor_cost(ui, live)) / seams).clamp(0.0, GROUP_AIR)
}

/// **The word the band prints on the dial the pointer names**, read off the
/// band's own source, where it is private (`recorder_panel`'s
/// `PITCH_LABEL`). The pointer's line and the app's notice are held to it,
/// so both name the dial a hand will find on the band, and a rename there
/// fails their tests by name instead of leaving them pointing at a word the
/// band no longer prints.
#[cfg(test)]
pub(crate) fn the_bands_word_for_its_pitch_dial() -> &'static str {
    const AT: &str = "const PITCH_LABEL: &str = \"";
    let src = include_str!("recorder_panel.rs");
    let from = src.find(AT).map(|at| at + AT.len()).expect("the band no longer declares its PITCH_LABEL");
    let len = src[from..].find('"').expect("the band's PITCH_LABEL never closes its quote");
    &src[from..from + len]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::fit;
    use egui::{Color32, Pos2, Rect};

    fn style() -> FaceStyle {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        FaceStyle::new(p.bg, p.lcd, p.lcd_face)
    }

    /// A tape face pushed `ids`, each named `KNOB {id}` so a test can find
    /// it by the word it paints, the stepped ones (the engine's
    /// [`STEPPED`]) wearing four choices of their own. Synthetic on purpose:
    /// the names, readings and choices are the engine's to print, so a panel
    /// test that spelled them would be a second copy of the engine's table.
    fn a_tape_of(ids: impl IntoIterator<Item = u32>) -> FaceLive {
        FaceLive {
            face: crate::ports::BuiltinFace {
                path: TAPE_PATH.to_owned(),
                params: ids
                    .into_iter()
                    .map(|id| {
                        let stepped = STEPPED.contains(&id);
                        crate::ports::BuiltinFaceParam {
                            id,
                            symbol: format!("p{id}"),
                            name: format!("KNOB {id}"),
                            units: "%".to_owned(),
                            default: 0.25,
                            reading: "50%".to_owned(),
                            choices: if stepped {
                                (0..4).map(|k| format!("pos {id}.{k}")).collect()
                            } else {
                                Vec::new()
                            },
                            steps: if stepped { 4 } else { 0 },
                            value: 0.5,
                        }
                    })
                    .collect(),
                ..Default::default()
            },
            preset_rows: vec![crate::ports::PresetRow {
                name: "a saved sound".to_owned(),
                path: "/p/a.tanfx".to_owned(),
            }],
            ..Default::default()
        }
    }

    /// What one frame of the face showed and asked.
    struct Shown {
        /// The ask the face returned.
        asked: Option<FaceAction>,
        /// Every shape the frame painted, flattened, in paint order.
        shapes: Vec<egui::Shape>,
        /// How many auto ids the face consumed, and the rect it filled.
        consumed: u64,
        took: Rect,
    }

    /// **One frame of the face as its window draws it**, in a window
    /// `window` big: the chain `fit::escape` measures (the in-house title
    /// bar off the top, the panel's ground, the plugin margin, the strap
    /// line and the air under it), dressed in `st`'s surface, with `events`
    /// delivered at `t` seconds while `mods` are held. Positions in the
    /// events and in what comes back are the same screen points.
    fn show(
        ctx: &egui::Context,
        window: Vec2,
        st: &FaceStyle,
        live: &mut FaceLive,
        t: f64,
        mods: egui::Modifiers,
        events: Vec<egui::Event>,
    ) -> Shown {
        let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
        let about = crate::faces::suite_unit(TAPE_PATH).map_or("", |(_, a)| a);
        let mut asked = None;
        let mut consumed = 0_u64;
        let mut took = Rect::NOTHING;
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, room)),
                time: Some(t),
                modifiers: mods,
                events,
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        fit::dress(ui.style_mut(), &st.s);
                        ui.painter().rect_filled(ui.max_rect(), 0.0, st.s.bg);
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::same(fit::MARGIN as i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(about)
                                        .font(FontId::new(10.0, fonts::courier_bold())),
                                );
                                ui.add_space(4.0);
                                let drawn = ui.scope(|ui| {
                                    let before = ui.next_auto_id().value();
                                    let act = crate::faces::draw(ui, st, live);
                                    consumed = ui.next_auto_id().value().wrapping_sub(before);
                                    act
                                });
                                took = drawn.response.rect;
                                asked = drawn.inner;
                            });
                    });
            },
        );
        fn flatten(shape: egui::Shape, into: &mut Vec<egui::Shape>) {
            match shape {
                egui::Shape::Vec(v) => {
                    for s in v {
                        flatten(s, into);
                    }
                }
                s => into.push(s),
            }
        }
        let mut shapes = Vec::new();
        for cs in out.shapes {
            flatten(cs.shape, &mut shapes);
        }
        Shown { asked, shapes, consumed, took }
    }

    /// Every word a frame painted: what it says, where, and in what ink.
    ///
    /// Where is the galley's own rect moved to the shape's origin, which is
    /// where the word is drawn. A galley laid out against its right edge (a
    /// reading in its right-to-left column) or its middle (a name centred in
    /// its cell) starts left of that origin, so a rect built from the origin
    /// and the galley's size would sit a whole width, or half of one, to the
    /// right of the word, and a press aimed at its middle would miss.
    fn words(shapes: &[egui::Shape]) -> Vec<(String, Rect, Color32)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::Text(t) => Some((
                    t.galley.text().to_owned(),
                    t.galley.rect.translate(t.pos.to_vec2()),
                    t.override_text_color
                        .or_else(|| t.galley.job.sections.first().map(|sec| sec.format.color))
                        .unwrap_or(t.fallback_color),
                )),
                _ => None,
            })
            .collect()
    }

    /// Where the one word that says `text` was painted, or a panic naming
    /// everything that was, since a test that found nothing must not pass.
    fn word(shapes: &[egui::Shape], text: &str) -> Rect {
        let all = words(shapes);
        let hits: Vec<Rect> = all.iter().filter(|(w, ..)| w == text).map(|(_, r, _)| *r).collect();
        assert_eq!(
            hits.len(),
            1,
            "{text:?} is painted {} times; the frame says {:?}",
            hits.len(),
            all.iter().map(|(w, ..)| w.as_str()).collect::<Vec<_>>()
        );
        hits[0]
    }

    /// The home marks a frame painted: every one-point vertical hairline,
    /// as its two ends, top first, and its colour.
    fn marks(shapes: &[egui::Shape]) -> Vec<(Pos2, Pos2, Color32)> {
        shapes
            .iter()
            .filter_map(|s| match s {
                egui::Shape::LineSegment { points: [a, b], stroke }
                    if (a.x - b.x).abs() < 1.0e-3 && (stroke.width - 1.0).abs() < 1.0e-3 =>
                {
                    Some(if a.y <= b.y { (*a, *b, stroke.color) } else { (*b, *a, stroke.color) })
                }
                _ => None,
            })
            .collect()
    }

    fn press(at: Pos2, button: egui::PointerButton, down: bool, mods: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton { pos: at, button, pressed: down, modifiers: mods }
    }

    /// **The tape is whole at its own window, on every font, and the window
    /// is the one the app opens.**
    ///
    /// The windows are asked of production rather than restated: the floor
    /// is `face_window_size` with no screen, the ceiling the same on a large
    /// display, the drag minimum is that floor, and the shared sweeps'
    /// restated pair in `fit::FACES` is held to exactly these two, so a
    /// floor moved in `FACE_SIZES` cannot leave the sweeps measuring the old
    /// one. Then the fullest panel the engine can push, with the pointer at
    /// its longest line, through `faces::draw` so the footer is counted,
    /// stays inside both windows on all three fonts.
    ///
    /// Three claims stand on the floor alone. **The section air is whole
    /// there**, on every font: a panel squeezed at the size it opens at is a
    /// floor set too low, and the air is the first thing a short window
    /// takes, so it is the first thing to measure. **The count the air is
    /// worked out from is the face drawn there**: the panel and its footer,
    /// less the air, come to exactly `floor_cost`, so the air is spent out of
    /// room the window really has, and a part the count leaves out or counts
    /// shorter than egui lays it out fails here rather than as a strip
    /// pushed down by points nobody had. **And the preset strip is one line
    /// there**, with a preset saved and with an empty folder, whose
    /// closed menu says "none saved yet", the widest thing it says. The strip
    /// clears its wrap at this width by under half a point on two of the
    /// three fonts (it holds one line at 375.75 points of content and wraps
    /// at 375.5), and a wrap adds a whole line the floor would pay for out of
    /// the air, so a strip that grows a part fails here by name rather than
    /// as air quietly lost.
    #[test]
    fn the_tape_is_whole_at_its_own_window_on_every_font() {
        let st = style();
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let ceiling = crate::dialogs::face_window_size(TAPE_PATH, Some(Vec2::new(3840.0, 2160.0)));
        assert_eq!(fit::drag_minimum(TAPE_PATH), floor, "the tape can be dragged below its floor");
        let &(_, n, stepped, _, opens, grows) = fit::FACES
            .iter()
            .find(|(p, ..)| *p == TAPE_PATH)
            .expect("the shared sweeps do not walk the tape");
        assert_eq!(
            (opens, grows),
            (floor, ceiling),
            "the shared sweeps measure the tape at {opens:?} and {grows:?}; it opens at {floor:?} and grows to {ceiling:?}"
        );
        assert_eq!((n, stepped), (ID_CEILING, STEPPED), "the shared sweeps push the tape a different shape");
        let about = crate::faces::suite_unit(TAPE_PATH).map_or("", |(_, a)| a);
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for (edge, window) in [("opens at", floor), ("grown to", ceiling)] {
                let mut live = fit::a_worst_case_face(TAPE_PATH, ID_CEILING, STEPPED, false);
                let (right, bottom) = fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                if right > fit::SLACK {
                    bad.push(format!("{} {edge} {}x{}: {right:.1} off the right", font.key(), window.x, window.y));
                }
                if bottom > fit::SLACK {
                    bad.push(format!("{} {edge} {}x{}: {bottom:.1} off the bottom", font.key(), window.x, window.y));
                }
            }
            // The air, read where the panel reads it: at the top of the room
            // the face is handed, inside the window it opens at. And the
            // count it is worked out from, held to the face drawn there.
            let mut live = fit::a_worst_case_face(TAPE_PATH, ID_CEILING, STEPPED, false);
            let (mut air, mut cost, mut took) = (f32::NAN, f32::NAN, f32::NAN);
            let _ = fit::escape(&ctx, floor, about, &st.s, &mut |ui| {
                air = section_air(ui, &live);
                cost = floor_cost(ui, &live);
                took = ui
                    .scope(|ui| {
                        let _ = crate::faces::draw(ui, &st, &mut live);
                    })
                    .response
                    .rect
                    .height();
            });
            if air != GROUP_AIR {
                bad.push(format!("{} at the floor: the section air is {air:.2}, not {GROUP_AIR}", font.key()));
            }
            if (took - 3.0 * air - cost).abs() > 0.01 {
                bad.push(format!(
                    "{} at the floor: the panel and its footer take {:.2} points past the air, and floor_cost counts {cost:.2}",
                    font.key(),
                    took - 3.0 * air
                ));
            }
            for saved in [true, false] {
                let mut live = fit::a_worst_case_face(TAPE_PATH, ID_CEILING, STEPPED, false);
                if !saved {
                    live.preset_rows.clear();
                }
                let shown = show(&ctx, floor, &st, &mut live, 1.0, egui::Modifiers::NONE, Vec::new());
                // On one line when the strip's first word and its last share
                // a height: a caption and a button's word do not share a
                // centre (the label is centred in the row before the menu
                // makes it taller, two points higher on every font), but a
                // wrap puts Init a whole line of the strip below PRESET.
                let (caption, init) = (word(&shown.shapes, "PRESET"), word(&shown.shapes, "Init"));
                if init.top() >= caption.bottom() || caption.top() >= init.bottom() {
                    bad.push(format!(
                        "{} at the floor, {}: the preset strip wraps, PRESET at y {:.1} to {:.1} and Init at y {:.1} to {:.1}",
                        font.key(),
                        if saved { "a preset saved" } else { "none saved yet" },
                        caption.top(),
                        caption.bottom(),
                        init.top(),
                        init.bottom()
                    ));
                }
            }
        }
        assert!(bad.is_empty(), "the tape is not whole at its own window:\n  {}", bad.join("\n  "));
    }

    /// **Every control sits in its section, in the panel's order, at every
    /// prefix of the engine's table.**
    ///
    /// The order is the design (see the module header): read down the panel
    /// and the signal's road is read with it. So the walk is the painted one,
    /// word by word, and every caption and control must stand strictly below
    /// the one before it, the transport closing on its pointer to tape start
    /// and stop and the footer's BYPASS last. Three tables, because the
    /// panel lands before the engine is whole: all thirteen ids, the ten the
    /// table holds before Bias is appended, and the six it holds today,
    /// where TRANSPORT holds Wow alone and TAPE holds Hiss alone. A section
    /// whose ids are not pushed yet keeps its caption, so the panel reads the
    /// same with a control missing as with it present.
    #[test]
    fn every_control_sits_in_its_section_in_the_panels_order() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let walk: [(&str, Option<u32>); 19] = [
            ("TRANSPORT", None),
            ("KNOB 6", Some(P_SPEED)),
            ("KNOB 3", Some(P_WOBBLE)),
            ("KNOB 9", Some(P_WOW_RANGE)),
            ("KNOB 7", Some(P_FLUTTER)),
            ("KNOB 8", Some(P_RATE)),
            ("Start and stop", None),
            ("RECORD", None),
            ("KNOB 0", Some(P_DRIVE)),
            ("KNOB 10", Some(P_BIAS)),
            ("TAPE", None),
            ("KNOB 4", Some(P_HISS)),
            ("KNOB 11", Some(P_AGE)),
            ("KNOB 12", Some(P_DAMAGE)),
            ("PLAYBACK", None),
            ("KNOB 1", Some(P_BUMP)),
            ("KNOB 2", Some(P_ROLL)),
            ("KNOB 5", Some(P_TRIM)),
            ("BYPASS", None),
        ];
        for count in [13_u32, 10, 6] {
            let mut live = a_tape_of(0..count);
            let shown = show(&ctx, floor, &st, &mut live, 1.0, egui::Modifiers::NONE, Vec::new());
            let mut last: Option<(&str, f32)> = None;
            let mut drawn = 0;
            for (text, id) in walk {
                if id.is_some_and(|id| id >= count) {
                    assert!(
                        !words(&shown.shapes).iter().any(|(w, ..)| w == text),
                        "with {count} controls pushed the panel still paints {text}"
                    );
                    continue;
                }
                let y = word(&shown.shapes, text).center().y;
                if let Some((above, at)) = last {
                    assert!(
                        y > at,
                        "with {count} controls pushed, {text} (y {y:.1}) is not below {above} (y {at:.1})"
                    );
                }
                last = Some((text, y));
                drawn += 1;
            }
            assert_eq!(drawn, count as usize + 6, "with {count} pushed the walk found {drawn} words");
        }
    }

    /// **The selectors walk the positions the kernel pushes**, and the panel
    /// spells none of them.
    ///
    /// Speed and Wow Range are drawn by the kit's stepped row from what the
    /// push says: its choices for the words and its steps for the arithmetic.
    /// So the fixture's words are the kernel's four speeds and two ranges,
    /// and the asks are measured: a click on "15 ips" steps forward to the
    /// top, a right-click steps back a third, a click at the top wraps to the
    /// bottom, an alt-click goes to the pushed default's position, and a
    /// click on "studio" asks for "deep". A face that kept a list of its own
    /// would paint its words over the kernel's and drift from them the day
    /// the kernel moved.
    #[test]
    fn the_selectors_walk_the_positions_the_kernel_pushes() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let mut live = a_tape_of(0..ID_CEILING as u32);
        for p in live.face.params.iter_mut() {
            if p.id == P_SPEED {
                p.choices = ["3.75 ips", "7.5 ips", "15 ips", "30 ips"].map(str::to_owned).to_vec();
                p.steps = 4;
                p.value = 2.0 / 3.0;
                p.default = 2.0 / 3.0;
            } else if p.id == P_WOW_RANGE {
                p.choices = ["studio", "deep"].map(str::to_owned).to_vec();
                p.steps = 2;
                p.value = 0.0;
                p.default = 0.0;
            }
        }
        let set = |live: &mut FaceLive, id: u32, value: f32| {
            if let Some(p) = live.face.params.iter_mut().find(|p| p.id == id) {
                p.value = value;
            }
        };
        let none = egui::Modifiers::NONE;
        let alt = egui::Modifiers::ALT;
        // A click on the word `text`, a whole second after the last gesture
        // so no two of them can be read as one: the ask the release raised.
        let mut clock = 0.0_f64;
        let mut click = |live: &mut FaceLive, text: &str, button: egui::PointerButton, mods: egui::Modifiers| {
            clock += 1.0;
            let at = word(&show(&ctx, floor, &st, live, clock, none, Vec::new()).shapes, text).center();
            let _ = show(&ctx, floor, &st, live, clock + 0.02, mods, vec![egui::Event::PointerMoved(at)]);
            let _ = show(&ctx, floor, &st, live, clock + 0.04, mods, vec![press(at, button, true, mods)]);
            show(&ctx, floor, &st, live, clock + 0.06, mods, vec![press(at, button, false, mods)]).asked
        };
        let primary = egui::PointerButton::Primary;
        let near = |v: f32, want: f32| (v - want).abs() < 1.0e-6;

        assert!(
            matches!(click(&mut live, "15 ips", primary, none), Some(FaceAction::Set { id: 6, value }) if near(value, 1.0)),
            "a click on 15 ips did not step Speed forward to the top"
        );
        assert!(
            matches!(click(&mut live, "15 ips", egui::PointerButton::Secondary, none), Some(FaceAction::Set { id: 6, value }) if near(value, 1.0 / 3.0)),
            "a right-click on 15 ips did not step Speed back"
        );
        set(&mut live, P_SPEED, 1.0);
        assert!(
            matches!(click(&mut live, "30 ips", primary, none), Some(FaceAction::Set { id: 6, value }) if near(value, 0.0)),
            "a click at the top speed did not wrap to the bottom"
        );
        assert!(
            matches!(click(&mut live, "30 ips", primary, alt), Some(FaceAction::Set { id: 6, value }) if near(value, 2.0 / 3.0)),
            "an alt-click did not take Speed to its pushed default's position"
        );
        assert!(
            matches!(click(&mut live, "studio", primary, none), Some(FaceAction::Set { id: 9, value }) if near(value, 1.0)),
            "a click on studio did not ask for the deep range"
        );
    }

    /// **Only Bias wears a home mark, and it stands at the home the push
    /// gives it.**
    ///
    /// The mark is the one id-based choice on the panel, so it is counted:
    /// exactly one hairline, about the twelve points the rail and its two
    /// proud ends come to, inside the Bias row and on no other, standing at
    /// the middle of the track while the pushed home is the middle. Then the
    /// pushed home moves to a quarter and the mark moves with it, onto the
    /// track's quarter point. The track is found from the row's own handle
    /// pushed to both ends of its travel rather than from the kit's
    /// arithmetic, so the two halves are measured against the picture. A
    /// face that drew the mark at a 0.5 of its own would pass the first half
    /// and fail the second.
    #[test]
    fn only_bias_wears_a_mark_and_it_stands_at_its_home() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let none = egui::Modifiers::NONE;
        let mut live = a_tape_of(0..ID_CEILING as u32);
        let bias = |live: &mut FaceLive, value: f32, home: f32| {
            for p in live.face.params.iter_mut().filter(|p| p.id == P_BIAS) {
                p.value = value;
                p.default = home;
            }
        };
        let row = word(&show(&ctx, floor, &st, &mut live, 1.0, none, Vec::new()).shapes, "KNOB 10").center().y;
        // The track, from the handle: Bias pushed to each end of its travel.
        // egui's handle is a rectangle three quarters as wide as it is tall
        // unless a style asks for a circle, and the kit paints whichever the
        // style says, so either is a handle; nothing else on a row stands
        // taller than it is wide.
        let handle_at = |live: &mut FaceLive, value: f32| -> f32 {
            bias(live, value, 0.5);
            let handles: Vec<Pos2> = show(&ctx, floor, &st, live, 2.0, none, Vec::new())
                .shapes
                .iter()
                .filter_map(|s| match s {
                    egui::Shape::Circle(c) => Some(c.center),
                    egui::Shape::Rect(r)
                        if r.rect.width() < r.rect.height() && r.rect.height() < face::ROW_H =>
                    {
                        Some(r.rect.center())
                    }
                    _ => None,
                })
                .collect();
            handles
                .iter()
                .find(|c| (c.y - row).abs() < face::ROW_H / 2.0)
                .map(|c| c.x)
                .unwrap_or_else(|| panic!("no handle on the Bias row at y {row:.1}; the handles: {handles:?}"))
        };
        let (from, to) = (handle_at(&mut live, 0.0), handle_at(&mut live, 1.0));
        assert!(to - from > 100.0, "the Bias track runs from x {from:.1} to x {to:.1}");

        bias(&mut live, 0.8, 0.5);
        let shown = show(&ctx, floor, &st, &mut live, 3.0, none, Vec::new());
        let found = marks(&shown.shapes);
        assert_eq!(found.len(), 1, "a thirteen-control panel paints {} home marks", found.len());
        let (top, bottom, _) = found[0];
        assert!(
            (bottom.y - top.y - 12.0).abs() <= 1.0,
            "the mark is {:.1} points long, not the rail and its two proud ends",
            bottom.y - top.y
        );
        let (lo, hi) = (row - face::ROW_H / 2.0, row + face::ROW_H / 2.0);
        assert!(
            top.y >= lo && bottom.y <= hi,
            "the mark spans y {:.1} to {:.1}, outside the Bias row's {lo:.1} to {hi:.1}",
            top.y,
            bottom.y
        );
        let middle = from + 0.5 * (to - from);
        assert!(
            (top.x - middle).abs() <= 0.5,
            "with its home pushed to the middle the mark stands at x {:.1}, the track's middle is {middle:.1}",
            top.x
        );

        bias(&mut live, 0.8, 0.25);
        let found = marks(&show(&ctx, floor, &st, &mut live, 4.0, none, Vec::new()).shapes);
        assert_eq!(found.len(), 1, "a moved home paints {} marks", found.len());
        let quarter = from + 0.25 * (to - from);
        assert!(
            (found[0].0.x - quarter).abs() <= 0.5,
            "with its home pushed to a quarter the mark stands at x {:.1}, the track's quarter is {quarter:.1}",
            found[0].0.x
        );
    }

    /// **A dragged tape row never moves the rows under it.**
    ///
    /// The kit's law (`face::keep`), held on this panel because this panel
    /// has more under a hand than any generic face: the captions, the air
    /// between the sections, the pointer and every row below Wow. A face
    /// that skipped drawing anything on the frames a slider spoke would
    /// flash all of it in and out at pointer rate and lose the double-click
    /// with it. So Wow's slider is pressed and dragged travel, rest, travel,
    /// and the auto ids the face consumes and the rect it fills must be
    /// identical on every frame, with the asks counted so a drag that never
    /// landed cannot pass.
    #[test]
    fn a_dragged_tape_row_never_moves_the_rows_under_it() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let mut live = a_tape_of(0..ID_CEILING as u32);
        let none = egui::Modifiers::NONE;
        let first = show(&ctx, floor, &st, &mut live, 1.0, none, Vec::new());
        // Mid-window is mid-track: the label column ends at a third of the
        // row and the reading column starts four fifths along.
        let mut at = Pos2::new(floor.x / 2.0, word(&first.shapes, "KNOB 3").center().y);
        let mut t = 1.0;
        let mut acted = 0_u32;
        let mut frame = |live: &mut FaceLive, events: Vec<egui::Event>| -> (u64, Rect) {
            t += 1.0 / 60.0;
            let shown = show(&ctx, floor, &st, live, t, none, events);
            if let Some(FaceAction::Set { id, value }) = shown.asked {
                acted += 1;
                for p in live.face.params.iter_mut().filter(|p| p.id == id) {
                    p.value = value;
                }
            }
            (shown.consumed, shown.took)
        };
        let _ = frame(&mut live, vec![egui::Event::PointerMoved(at)]);
        let _ = frame(&mut live, vec![press(at, egui::PointerButton::Primary, true, none)]);
        let mut seen: Vec<(u64, Rect)> = Vec::new();
        for step in 0..6 {
            let events = if step % 2 == 0 {
                at.x += 6.0;
                vec![egui::Event::PointerMoved(at)]
            } else {
                Vec::new()
            };
            seen.push(frame(&mut live, events));
        }
        assert!(acted >= 3, "Wow was never actually dragged, so this proves nothing ({acted} asks)");
        assert!(
            seen.iter().all(|s| *s == seen[0]),
            "the tape face is not the same face frame to frame while Wow is dragged: {seen:#?}"
        );
    }

    /// **A control the panel has no place for still gets a row**, under the
    /// four sections and above the footer. The engine is append-only and
    /// grows ahead of its face, so a fourteenth control pushed to a panel
    /// that knows thirteen must not vanish: an automation lane would move a
    /// dial nobody could see. It is drawn as the generic rows would draw it.
    #[test]
    fn a_control_the_panel_has_no_place_for_still_gets_a_row() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let mut live = a_tape_of(0..14);
        let shown = show(&ctx, floor, &st, &mut live, 1.0, egui::Modifiers::NONE, Vec::new());
        let (last, stray, foot) = (
            word(&shown.shapes, "KNOB 5").center().y,
            word(&shown.shapes, "KNOB 13").center().y,
            word(&shown.shapes, "BYPASS").center().y,
        );
        assert!(stray > last, "the unplaced control (y {stray:.1}) is not under the last section's last row (y {last:.1})");
        assert!(stray < foot, "the unplaced control (y {stray:.1}) is not above the footer (y {foot:.1})");
    }

    /// **The home mark reads on every theme, in both modes.**
    ///
    /// The shared ink sweep reads only `Shape::Text`, and the mark is a
    /// line, so it is the one thing on this panel that sweep cannot see. It
    /// wears the rail's own foreground and crosses the rail's fill, with its
    /// two ends proud of the rail on the panel's ground, so it is measured
    /// against both, off the shape stream: the ground under a point is the
    /// last opaque rectangle painted before the mark that holds it. Each pair
    /// must clear 3.0, the floor a reading at rest is held to. Every theme
    /// `theme::factory` returns (thirty-two today), dressed the way the face
    /// window dresses itself, light and dark; and the mark has to be found on
    /// every one of them, or a mark that stopped being drawn would pass.
    #[test]
    fn the_home_mark_reads_on_every_theme_in_both_modes() {
        use crate::recorder_panel::contrast_ratio;
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let themes = crate::theme::factory();
        let mut bad: Vec<String> = Vec::new();
        let mut found = 0;
        for theme in &themes {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mode = if dark { "dark" } else { "light" };
                let mut live = fit::a_worst_case_face(TAPE_PATH, ID_CEILING, STEPPED, false);
                let shown = show(&ctx, floor, &st, &mut live, 1.0, egui::Modifiers::NONE, Vec::new());
                let mut grounds: Vec<(Rect, Color32)> = Vec::new();
                for s in &shown.shapes {
                    match s {
                        egui::Shape::Rect(r) if r.fill.a() == 255 && r.rect.is_positive() => {
                            grounds.push((r.rect, r.fill));
                        }
                        egui::Shape::LineSegment { points: [a, b], stroke }
                            if (a.x - b.x).abs() < 1.0e-3 && (stroke.width - 1.0).abs() < 1.0e-3 =>
                        {
                            found += 1;
                            let under = |at: Pos2| {
                                grounds.iter().rev().find(|(r, _)| r.contains(at)).map_or(st.s.bg, |&(_, c)| c)
                            };
                            let (top, bottom) = if a.y <= b.y { (*a, *b) } else { (*b, *a) };
                            for (what, at) in [
                                ("the rail it crosses", Pos2::new(top.x, (top.y + bottom.y) / 2.0)),
                                ("the ground under its top", top + Vec2::new(0.0, 0.5)),
                                ("the ground under its foot", bottom - Vec2::new(0.0, 0.5)),
                            ] {
                                let got = contrast_ratio(stroke.color, under(at));
                                if got < 3.0 {
                                    bad.push(format!("{} {mode}: the home mark on {what} is {got:.2}:1", theme.name));
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
        assert_eq!(
            found,
            themes.len() * 2,
            "the home mark was found on {found} of {} dressings",
            themes.len() * 2
        );
        assert!(bad.is_empty(), "the home mark does not read:\n  {}", bad.join("\n  "));
    }

    /// The face's path is the suite table's spelling, under the name the
    /// window wears: one sentinel, asserted where both live, the amp's
    /// precedent. A table row that drifted from this const would route the
    /// tape to the generic rows and open it at the generic size.
    #[test]
    fn the_tape_path_is_the_suite_tables_spelling() {
        assert!(
            crate::faces::SUITE_UNITS
                .iter()
                .any(|(p, name, _)| *p == TAPE_PATH && *name == "Tangent Tape"),
            "SUITE_UNITS has no Tangent Tape at {TAPE_PATH}, so its panel would never open"
        );
    }

    /// **A squeezed tape gives up its air and never its rows.**
    ///
    /// The amp's rule, and the reason [`floor_cost`] exists: the panel spends
    /// air only out of room it can prove it has, and when the room is not
    /// there the air goes and nothing else does. So the panel alone, at a
    /// room with plenty to spare and at one far too short for it, differs by
    /// exactly the three seams of air and no more, and the squeezed panel
    /// still holds four captions, thirteen rows at the kit's own height and
    /// the pointer at its own.
    #[test]
    fn a_squeezed_tape_gives_up_its_air_and_never_its_rows() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let mut caption_h = 0.0_f32;
        let mut pointer_h = 0.0_f32;
        let mut height = |h: f32| {
            let mut live = fit::a_worst_case_face(TAPE_PATH, ID_CEILING, STEPPED, false);
            let mut took = 0.0_f32;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(376.0, h))),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            fit::dress(ui.style_mut(), &st.s);
                            caption_h = caption_height(ui);
                            pointer_h = face::pointer_row_height(ui);
                            took = ui
                                .scope(|ui| {
                                    let _ = draw_tape(ui, &st, &mut live);
                                })
                                .response
                                .rect
                                .height();
                        });
                },
            );
            took
        };
        let roomy = height(900.0);
        let squeezed = height(240.0);
        assert!(
            (roomy - squeezed - 3.0 * GROUP_AIR).abs() <= 0.5,
            "the panel gave up {:.2} points between a roomy window and a short one; its air is {:.1}",
            roomy - squeezed,
            3.0 * GROUP_AIR
        );
        assert!(
            squeezed >= 4.0 * caption_h + ID_CEILING as f32 * face::ROW_H + pointer_h,
            "the squeezed panel is {squeezed:.1} points, which cannot hold four captions, {ID_CEILING} rows and the pointer"
        );
    }

    /// **The transport points at tape start and stop, and holds no copy of
    /// it.**
    ///
    /// The setting is the desk's, so the panel carries one row for it and no
    /// control: how it stands, in the setting's own readings, and where it is
    /// set, by the name the band prints on the dial. Nobody pushed it, it is
    /// off, it is on: three first lines, each spelled here because the words
    /// are the pointer's contract, the times through `StartStop::reading`
    /// (trailing zeros trimmed, so ten seconds is "10 s"). Painted, the row
    /// closes the transport, under Rate and over RECORD, and it is the same
    /// row whatever it says: the panel's extent and the auto ids it takes are
    /// identical for the three, so the setting arriving or moving shifts
    /// nothing under the hand. A press on either line is the ask to be taken
    /// there; a press on Rate's slider, directly above it, is Rate's.
    ///
    /// The second line's words are ruled, and both halves of them are held:
    /// the line itself, spelled in full, so a reworded pointer fails here by
    /// name; and the dial in it, against the word the band prints on the
    /// dial, read off the band's own source, so a band that renamed it
    /// would leave this line pointing at a word nobody can find and fail
    /// here by name as well.
    #[test]
    fn the_transport_section_points_at_tape_start_and_stop() {
        use crate::motor::StartStop;
        assert_eq!(start_stop_line(None), "Start and stop");
        assert_eq!(start_stop_line(Some(StartStop::default())), "Start and stop: off");
        assert_eq!(
            start_stop_line(Some(StartStop::new(true, 0.5, 0.7))),
            "Start and stop: 0.5 s up, 0.7 s down"
        );
        assert_eq!(
            start_stop_line(Some(StartStop::new(true, 1.25, 10.0))),
            "Start and stop: 1.25 s up, 10 s down"
        );
        assert_eq!(
            START_STOP_WHERE, "set on the PITCH CONT dial",
            "the pointer's second line is not the ruled one"
        );
        let dial = the_bands_word_for_its_pitch_dial();
        assert_eq!(
            START_STOP_WHERE,
            format!("set on the {dial} dial"),
            "the band prints its dial as {dial:?}, and the pointer says {START_STOP_WHERE:?}"
        );

        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let none = egui::Modifiers::NONE;
        let mut seen: Vec<(String, u64, Rect)> = Vec::new();
        for (k, ss) in [None, Some(StartStop::default()), Some(StartStop::new(true, 0.5, 0.7))]
            .into_iter()
            .enumerate()
        {
            let mut live = a_tape_of(0..ID_CEILING as u32);
            live.start_stop = ss;
            let said = start_stop_line(ss);
            let shown = show(&ctx, floor, &st, &mut live, 1.0 + k as f64, none, Vec::new());
            let (rate, line, sub, record) = (
                word(&shown.shapes, "KNOB 8"),
                word(&shown.shapes, &said),
                word(&shown.shapes, START_STOP_WHERE),
                word(&shown.shapes, "RECORD"),
            );
            assert!(
                rate.bottom() <= line.top() && line.bottom() <= sub.top() && sub.bottom() <= record.top(),
                "{said:?} at y {:.1} to {:.1} and {START_STOP_WHERE:?} at y {:.1} to {:.1} are not between Rate \
                 (ending at {:.1}) and RECORD (starting at {:.1})",
                line.top(),
                line.bottom(),
                sub.top(),
                sub.bottom(),
                rate.bottom(),
                record.top()
            );
            seen.push((said, shown.consumed, shown.took));
        }
        assert!(
            seen.iter().all(|(_, ids, took)| (*ids, *took) == (seen[0].1, seen[0].2)),
            "the panel is not the same panel whatever the pointer says (line, auto ids, extent): {seen:#?}"
        );

        let mut live = a_tape_of(0..ID_CEILING as u32);
        live.start_stop = Some(StartStop::new(true, 0.5, 0.7));
        let shown = show(&ctx, floor, &st, &mut live, 10.0, none, Vec::new());
        let line = word(&shown.shapes, "Start and stop: 0.5 s up, 0.7 s down").center();
        let sub = word(&shown.shapes, START_STOP_WHERE).center();
        // Rate's slider: three fifths across the window is past the middle
        // of its track, off the half it holds, so a press there is a change.
        let rate = Pos2::new(floor.x * 0.6, word(&shown.shapes, "KNOB 8").center().y);
        // A click at `at`, a whole second after the last so no two of them
        // can be read as one: the ask the frames of it raised.
        let mut clock = 10.0_f64;
        let mut click = |live: &mut FaceLive, at: Pos2| -> Vec<Option<FaceAction>> {
            clock += 1.0;
            let mut asked = Vec::new();
            for (dt, events) in [
                (0.02, vec![egui::Event::PointerMoved(at)]),
                (0.04, vec![press(at, egui::PointerButton::Primary, true, none)]),
                (0.06, vec![press(at, egui::PointerButton::Primary, false, none)]),
            ] {
                asked.push(show(&ctx, floor, &st, live, clock + dt, none, events).asked);
            }
            asked
        };
        for (what, at) in [("first line", line), ("second line", sub)] {
            let asked = click(&mut live, at);
            assert_eq!(
                asked.iter().filter(|a| matches!(a, Some(FaceAction::OpenStartStop))).count(),
                1,
                "a press on the pointer's {what} did not ask, once, to be taken to start and stop"
            );
        }
        let asked = click(&mut live, rate);
        assert!(
            !asked.iter().any(|a| matches!(a, Some(FaceAction::OpenStartStop))),
            "a press on Rate's slider asked to be taken to start and stop"
        );
        assert!(
            asked.iter().any(|a| matches!(a, Some(FaceAction::Set { id: P_RATE, .. }))),
            "the press on Rate's slider never landed on it, so it proves nothing"
        );
    }

    /// **A reading committed with Tab never lands Enter or Space on the
    /// pointer.**
    ///
    /// Rate's reading is the last field above the pointer, and a typed
    /// reading commits on Tab, which hands egui's keyboard focus to the next
    /// widget that takes one; egui then presses a focused widget on Enter or
    /// Space. The pointer is a door that closes the window it stands in, it
    /// paints no focus, and Space is the transport's key: in the Tab order,
    /// it is a hand that typed a value, tabbed on, pressed Space to play and
    /// lost the window instead. So the walk is the hand's: click Rate's
    /// reading, type, press Tab, and the typed value must arrive (the field
    /// held the focus, and Tab is what moved it on) with the focus left on
    /// some widget; then Enter, then Space, and neither may ask to be taken
    /// to start and stop.
    #[test]
    fn a_reading_committed_with_tab_never_lands_enter_or_space_on_the_pointer() {
        let st = style();
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let floor = crate::dialogs::face_window_size(TAPE_PATH, None);
        let none = egui::Modifiers::NONE;
        let mut live = a_tape_of(0..ID_CEILING as u32);
        live.start_stop = Some(crate::motor::StartStop::new(true, 0.5, 0.7));
        for p in live.face.params.iter_mut().filter(|p| p.id == P_RATE) {
            p.reading = "77%".to_owned();
        }
        let named = |a: &Option<FaceAction>| match a {
            None => "nothing".to_owned(),
            Some(FaceAction::Set { id, value }) => format!("Set {id} to {value}"),
            Some(FaceAction::Type { id, text }) => format!("Type {text:?} into {id}"),
            Some(FaceAction::OpenStartStop) => "OpenStartStop".to_owned(),
            Some(_) => "another ask".to_owned(),
        };
        let key = |key: egui::Key| egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: none,
        };
        let mut clock = 1.0_f64;
        let mut frame = |live: &mut FaceLive, events: Vec<egui::Event>| -> Option<FaceAction> {
            clock += 1.0 / 60.0;
            show(&ctx, floor, &st, live, clock, none, events).asked
        };
        let reading = word(&show(&ctx, floor, &st, &mut live, 0.5, none, Vec::new()).shapes, "77%").center();
        let mut typed = Vec::new();
        for events in [
            vec![egui::Event::PointerMoved(reading)],
            vec![press(reading, egui::PointerButton::Primary, true, none)],
            vec![press(reading, egui::PointerButton::Primary, false, none)],
            Vec::new(),
            vec![egui::Event::Text("60%".to_owned())],
            vec![key(egui::Key::Tab)],
        ] {
            typed.push(frame(&mut live, events));
        }
        assert!(
            typed.iter().any(|a| matches!(a, Some(FaceAction::Type { id: P_RATE, text }) if text == "60%")),
            "Rate's reading never committed what was typed, so Tab moved nothing and this proves nothing: {:?}",
            typed.iter().map(named).collect::<Vec<_>>()
        );
        assert!(
            ctx.memory(|m| m.focused()).is_some(),
            "Tab left the focus nowhere, so Enter and Space had no widget to press and this proves nothing"
        );
        for (what, events) in [
            ("Enter", vec![key(egui::Key::Enter)]),
            ("Space", vec![key(egui::Key::Space), egui::Event::Text(" ".to_owned())]),
        ] {
            let asked = frame(&mut live, events);
            assert!(
                !matches!(asked, Some(FaceAction::OpenStartStop)),
                "{what} after Rate's reading was committed with Tab pressed the pointer, a door that paints no \
                 focus and closes the window it stands in"
            );
        }
    }
}
