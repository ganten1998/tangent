//! The amp's face: a panel of kit rows and a pedalboard of seven chips.
//!
//! Drawn entirely from the face kit, like the suite faces, and answering to
//! the same laws: the surface derives its parts, ink is measured, and the
//! panel is a picture of pushed data returning small asks. The art is
//! deliberately ORIGINAL — kit surfaces, the cluster's glass, Tangent's own
//! captions. The circuit it models is named referentially in docs and
//! tooltips only; no script logo, no model-name plate, no borrowed knob art
//! (see the IR-pack legal brief for the naming rule this follows).
//!
//! The layout reads like the hardware's control plan without copying its
//! dress: a bright switch and four dials on the top strip, the tremolo
//! group beside them — the panel word is "Vibrato" because that is what the
//! era's panels said, and the tooltip tells the truth that it is amplitude
//! tremolo — and the pedalboard underfoot as a row of seven chips, each
//! wearing its tenant's short name or a plus. A chip is a doorway, not a
//! editor: clicking one asks the app to open the picker over that pedal's
//! own seat (`recorder::amp_pedal_seat`), and the pedal's face then opens
//! by the same road every insert's does.

use crate::face::{self, slider_row, stepped_row, FaceStyle, StepAsk};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;
use crate::recorder::AMP_PEDALS;

/// The amp's sentinel path. The string is the contract with the engine's
/// `builtin_fx::AMP_PATH`, exactly as `SUITE_UNITS` documents for the other
/// four — the two crates cannot share the const across the firewall, so a
/// test on each side pins the spelling.
pub const AMP_PATH: &str = "@tangent-amp";

/// A pedal chip at full height, and the shortest it may be squeezed to and
/// still be a target a hand can hit. Thirty is the drawn height; the floor
/// is still four points taller than a kit row, which is the smallest thing
/// this panel asks anybody to click.
const CHIP_H: f32 = 30.0;
const CHIP_MIN_H: f32 = 22.0;

/// The air between the panel's three groups when the room can afford it —
/// under the channel strip, and under the vibrato group before the board.
const GROUP_AIR: f32 = 6.0;
const BOARD_AIR: f32 = 8.0;
/// And the hair between the board's caption and its chips, which is the
/// first thing to go because it separates a word from the thing it names.
const CAPTION_AIR: f32 = 2.0;

/// A pedal chip's label, from the tenant's stored path.
///
/// Sentinels shed their `@tangent-` prefix — the chip row is all Tangent's
/// own units in the seeded shape, and seven chips saying "tangent-" seven
/// times is a row nobody can scan. A real bundle shows its file stem. Both
/// are cut to what a chip can carry; the full name lives on the hover.
pub fn pedal_label(path: &str) -> String {
    let name = path
        .strip_prefix("@tangent-")
        .map(str::to_owned)
        .unwrap_or_else(|| {
            std::path::Path::new(path)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| path.to_owned())
        });
    // Ten characters holds every seeded pedal whole and keeps a long VST3
    // name from stretching its chip; the ellipsis says there is more.
    if name.chars().count() > 10 {
        let cut: String = name.chars().take(9).collect();
        format!("{cut}…")
    } else {
        name
    }
}

/// The panel: the amp's rows, then the pedalboard. At most one ask per
/// frame, the face contract.
pub fn draw_amp(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &mut FaceLive,
) -> Option<FaceAction> {
    let mut out = None;
    let bold = |size: f32| egui::FontId::new(size, fonts::courier_bold());
    let caption = |ui: &mut egui::Ui, text: &str| {
        ui.label(egui::RichText::new(text).font(bold(9.0)).color(st.s.faint));
    };

    // **What the panel can afford, asked before it spends anything.**
    //
    // The preset strip's lesson, at the other end of the same panel: a
    // fixed number is a promise the room may not be able to keep. This one
    // ran the other way — DOWN rather than right — and so nobody saw it.
    // Nine rows, three captions and a board at thirty points, plus the
    // preset strip under them, come to more than the amp's window holds:
    // the window floors at 540 by 360, and once the in-house title bar, the
    // plugin margin and the strap-line above have taken theirs the panel is
    // left with about two hundred and ninety. It ran four points past that
    // on Courier Prime and Terminess and twenty on JetBrains Mono, where a
    // nine-point caption is two points taller. The save row was clipped
    // along its bottom edge at the amp's OWN size, on every font, with
    // nothing dragged — the same complaint the owner made about the right
    // edge, wearing the other one.
    //
    // So the decoration is spent out of room the panel can prove it has.
    // The air goes first, because air carries no information; then the
    // chips come down to [`CHIP_MIN_H`]. What never moves is a ROW — a knob
    // that shrinks with the window is a knob nobody can set — and what is
    // never spent is the footer, because `faces::draw` appends the preset
    // strip under every face and this panel is the only thing standing
    // between the strip and the bottom of the window.
    let give = (ui.available_height() - amp_floor_cost(ui)).max(0.0);
    // Both spacers, then the chips: the order is the order they are given
    // up in, and `give` is consumed as it is handed out.
    let mut left = give;
    let mut spend = |want: f32| {
        let got = want.min(left);
        left -= got;
        got
    };
    let group_air = spend(GROUP_AIR);
    let board_air = spend(BOARD_AIR);
    let caption_air = spend(CAPTION_AIR);
    let chip_h = CHIP_MIN_H + spend(CHIP_H - CHIP_MIN_H);

    // ── the top strip: bright, volume, the stack ────────────────────────────
    caption(ui, "CHANNEL");
    face::keep(&mut out, toggle_row(ui, st, live, 0));
    for id in 1..=4 {
        face::keep(&mut out, param_row(ui, st, live, id));
    }
    ui.add_space(group_air);

    // ── the vibrato group ───────────────────────────────────────────────────
    // The panel word is the era's; that it is amplitude tremolo is the
    // module doc's to tell, not a knob's.
    caption(ui, "VIBRATO");
    face::keep(&mut out, toggle_row(ui, st, live, 5));
    for id in 6..=8 {
        face::keep(&mut out, param_row(ui, st, live, id));
    }
    ui.add_space(board_air);

    // ── the pedalboard ──────────────────────────────────────────────────────
    match &live.pedals {
        Some(pedals) => {
            caption(ui, "PEDALS - in order, before the amp");
            ui.add_space(caption_air);
            let gap = ui.spacing().item_spacing.x;
            let w = crate::dialogs::cells_across(ui.available_width(), AMP_PEDALS, gap);
            // **A chip is only as narrow as its own text lets it be.**
            // `cells_across` divides the row honestly, and then `add_sized`
            // hands the button a size it is free to ignore: egui sizes a
            // button from its galley plus the theme's padding, and takes the
            // LARGER of that and what it was asked for. Seven ten-character
            // labels therefore need about four hundred and eighty points
            // whatever the panel is, so the board ran off the right-hand
            // edge of any amp window narrower than that — which is every amp
            // window the moment it is dragged in, and the amp half of the
            // owner's "cut off on the right". Cutting the WORD to the room
            // is the fix; the full name is on the hover, where it already
            // was. `horizontal_wrapped` is the floor under that: if the room
            // is too small even for an ellipsis, the board takes a second
            // line rather than running off the end.
            let room = (w - ui.spacing().button_padding.x * 2.0 - 2.0).max(0.0);
            let cut: Vec<String> = pedals
                .iter()
                .map(|t| {
                    crate::drumpad_panel::cut_text(
                        ui.painter(),
                        room,
                        if t.is_empty() { "+" } else { t.as_str() },
                        &bold(9.0),
                    )
                })
                .collect();
            ui.horizontal_wrapped(|ui| {
                for (pedal, tenant) in pedals.iter().enumerate() {
                    let empty = tenant.is_empty();
                    let chip = ui.add_sized(
                        egui::Vec2::new(w, chip_h),
                        egui::Button::new(
                            egui::RichText::new(&cut[pedal])
                                .font(bold(9.0))
                                // An empty seat wears the quiet ink, and a
                                // chip is RAISED, so that ink is measured
                                // against the chip and not against the
                                // panel — `faint` on `raised` is under the
                                // caption floor on three shipping themes.
                                .color(if empty {
                                    face::quiet_ink(st, st.s.raised)
                                } else {
                                    st.s.ink
                                }),
                        )
                        .fill(st.s.raised),
                    );
                    let chip = chip.on_hover_text(if empty {
                        format!("pedal {}: choose an effect", pedal + 1)
                    } else {
                        format!("pedal {}: {tenant} - click to swap", pedal + 1)
                    });
                    if chip.clicked() && out.is_none() {
                        out = Some(FaceAction::OpenPedal { pedal });
                    }
                }
            });
        }
        // The amp is on the master, a bus or a folder's extra rack: no
        // pedalboard there in v1, and the panel says so rather than
        // drawing seven chips that go nowhere.
        None => caption(ui, "PEDALS - on channel strips only"),
    }
    out
}

/// How many rows and captions the panel above the board is made of: five in
/// the channel group, four in the vibrato group, and a caption over each of
/// the three groups. Counted here so the budget below and the panel above
/// cannot drift apart.
const AMP_ROWS: f32 = 9.0;
const AMP_CAPTIONS: f32 = 3.0;

/// **What the amp may not spend**, in points, on the font and spacing this
/// frame is being drawn with.
///
/// Nine rows and three captions at the sizes they are actually drawn, a
/// board squeezed to [`CHIP_MIN_H`], and the footer under it. Everything is
/// MEASURED and nothing is counted in points typed here: `add_sized` takes
/// the larger of what it is handed and what the galley needs, so a row is
/// only [`face::ROW_H`] tall on a font whose eleven-point line fits inside
/// it, a nine-point caption is ten points on Courier Prime and twelve on
/// JetBrains Mono, and a button's padding is the dialogs' twelve by four
/// rather than egui's own six by three. Three captions and the strap-line
/// are eight points of difference between one font and another, which is
/// why this panel missed its window by four on one font and twenty on
/// another rather than by the same amount on all three.
///
/// **Why the footer is in here at all.** `faces::draw` appends the preset
/// strip under every face, so the strip is drawn into whatever this panel
/// leaves behind and has no way to argue for itself. Its cost is measured
/// the way `preset_strip` measures its own parts, for the same reason: a
/// button is its text plus the theme's padding, and both move with the
/// style. If the strip ever grows a second row this figure is wrong and
/// `the_amp_fits_its_own_window_on_every_font` says so.
fn amp_floor_cost(ui: &egui::Ui) -> f32 {
    let gap = ui.spacing().item_spacing.y;
    let line = |size: f32| {
        ui.painter()
            .layout_no_wrap(
                "Xg".to_owned(),
                egui::FontId::new(size, fonts::courier_bold()),
                egui::Color32::WHITE,
            )
            .rect
            .height()
    };
    let row = face::ROW_H.max(line(11.0)) + gap;
    let caption = line(9.0) + gap;
    let footer = 6.0
        + ui.spacing()
            .interact_size
            .y
            .max(line(10.0) + ui.spacing().button_padding.y * 2.0)
        + gap;
    AMP_ROWS * row + AMP_CAPTIONS * caption + CHIP_MIN_H + gap + footer
}

/// One continuous parameter as a kit row, by id.
fn param_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &FaceLive,
    id: u32,
) -> Option<FaceAction> {
    let p = live.face.params.iter().find(|p| p.id == id)?;
    slider_row(ui, st, &p.name, &p.reading, p.value, p.default)
        .map(|value| FaceAction::Set { id, value })
}

/// One toggle as a stepped kit row wearing its choice.
fn toggle_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    live: &FaceLive,
    id: u32,
) -> Option<FaceAction> {
    let p = live.face.params.iter().find(|p| p.id == id)?;
    let on = p.value >= 0.5;
    let label = p
        .choices
        .get(usize::from(on))
        .map(String::as_str)
        .unwrap_or(if on { "on" } else { "off" });
    stepped_row(ui, st, &p.name, label).map(|ask| FaceAction::Set {
        id,
        value: match ask {
            // A switch has two positions; either step is the other one.
            StepAsk::By(_) => {
                if on {
                    0.0
                } else {
                    1.0
                }
            }
            // Home is the engine's own number, pushed with the parameter.
            StepAsk::Home => p.default,
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The chip labels: sentinels shed the house prefix, bundles keep their
    /// stem, and nothing overflows a chip.
    #[test]
    fn pedal_chips_wear_short_honest_names() {
        assert_eq!(pedal_label("@tangent-comp"), "comp");
        assert_eq!(pedal_label("@tangent-wah"), "wah");
        assert_eq!(pedal_label("/x/plugs/Pro-Q 4.vst3"), "Pro-Q 4");
        let long = pedal_label("/x/FabFilter Saturn 2 Deluxe.vst3");
        assert!(long.chars().count() <= 10, "{long}");
        assert!(long.ends_with('…'), "{long}");
    }

    /// **The pedalboard fits the panel, at every width the panel can be.**
    ///
    /// The owner's report was "menus that are cut off in them (cut off on
    /// the right side): Imager, Amp", and this is the amp's half of it.
    /// `cells_across` divided the row honestly all along; what nobody
    /// noticed is that `add_sized` only ASKS. egui sizes a button from its
    /// own galley plus the theme's padding and takes the larger of that and
    /// the size it was handed, so seven ten-character labels claimed about
    /// four hundred and eighty points however narrow the window was, and the
    /// last chips were quietly clipped away. The word is cut to the room
    /// now, so the row's width follows the panel's.
    ///
    /// The widths swept start at the narrowest a dialog can be dragged to —
    /// the 320-point floor, less the plugin margin either side.
    #[test]
    fn the_pedalboard_fits_the_panel_at_every_width() {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let mut bad: Vec<String> = Vec::new();
        for w in [296.0f32, 316.0, 400.0, 516.0, 700.0, 1200.0] {
            let mut live = FaceLive {
                face: crate::ports::BuiltinFace {
                    path: AMP_PATH.to_owned(),
                    params: (0..9)
                        .map(|i| crate::ports::BuiltinFaceParam {
                            id: i,
                            symbol: format!("p{i}"),
                            name: format!("KNOB {i}"),
                            units: "%".to_owned(),
                            default: 0.5,
                            reading: "50%".to_owned(),
                            choices: vec!["off".to_owned(), "on".to_owned()],
                            steps: 0,
                            value: 0.5,
                        })
                        .collect(),
                    ..Default::default()
                },
                // The longest a chip can ever wear: `pedal_label` cuts at
                // ten characters, so this is the worst case by construction.
                pedals: Some(std::array::from_fn(|_| "compresso\u{2026}".to_owned())),
                ..Default::default()
            };
            let screen =
                egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::Vec2::new(w, 900.0));
            let mut took = f32::MIN;
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            took = ui
                                .scope(|ui| {
                                    let _ = draw_amp(ui, &st, &mut live);
                                })
                                .response
                                .rect
                                .right();
                        });
                },
            );
            // The allocation is one question and what got PAINTED is the
            // other: egui clips the second kind away, which is exactly why
            // this shipped unseen.
            let mut painted = f32::MIN;
            for cs in &out.shapes {
                let b = cs.shape.visual_bounding_rect();
                if b.is_positive() && b.right().is_finite() {
                    painted = painted.max(b.right());
                }
            }
            // A point of slack for the fractional widths egui justifies to;
            // the fault this pins ran to a hundred and sixty.
            if took > w + 1.0 {
                bad.push(format!("at {w}: took {took:.1}"));
            }
            if painted > w + 1.0 {
                bad.push(format!("at {w}: painted to {painted:.1}"));
            }
        }
        assert!(bad.is_empty(), "the amp overflows:\n  {}", bad.join("\n  "));
    }

    /// The face's path const matches the suite table row that routes to it —
    /// one spelling, asserted where both live.
    #[test]
    fn the_amp_path_is_the_suite_tables_spelling() {
        assert!(
            crate::faces::SUITE_UNITS.iter().any(|(p, ..)| *p == AMP_PATH),
            "the amp is not in SUITE_UNITS, so no face would ever open"
        );
    }

    /// **The amp is whole at its OWN window, on every font, with a board and
    /// without one.**
    ///
    /// The owner named this panel: "menus that are cut off in them (cut off
    /// on the right side): Imager, Amp". The sideways half of that was the
    /// preset strip and the pedal chips, and it is fixed — at 540 by 360, the
    /// size `dialogs::FACE_SIZES` opens this face at, the panel now has
    /// twelve points of air on its right on all three fonts.
    ///
    /// What that fix did not touch, and what this test is really for, is the
    /// other direction. There is no scroll area under a face on the desktop
    /// path, so a panel taller than its window is drawn and unreachable —
    /// the same complaint wearing a different edge. Nine rows, three captions
    /// and a board came to more than the window held once the strap-line and
    /// the in-house title bar had taken theirs: four points more on Courier
    /// Prime and Terminess, twenty on JetBrains Mono, where a nine-point
    /// caption is two points taller. The save row was clipped along its
    /// bottom edge at the amp's own size, on every font, with nothing
    /// dragged.
    ///
    /// Both seats are swept because they are different panels: a bay on a
    /// channel strip gets seven chips, and one on the master, a bus or a
    /// folder's extra rack gets a line of text saying why it does not.
    #[test]
    fn the_amp_is_whole_at_its_own_window_on_every_font() {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let about = crate::faces::suite_unit(AMP_PATH).map_or("", |(_, a)| a);
        let mut bad: Vec<String> = Vec::new();
        for font in face::fit::FONTS {
            let ctx = face::fit::a_ctx(font);
            for board in [true, false] {
                for (edge, window) in [
                    ("opens at", egui::Vec2::new(540.0, 360.0)),
                    ("grown to", egui::Vec2::new(864.0, 576.0)),
                ] {
                    let mut live = face::fit::a_worst_case_face(AMP_PATH, 9, &[0, 5], board);
                    let (right, bottom) =
                        face::fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                            // Through `faces::draw` rather than `draw_amp`,
                            // because the strip it appends under every face
                            // is the thing this panel must leave room for —
                            // and the amp alone, measured alone, would pass
                            // while the window still clipped the save row.
                            let _ = crate::faces::draw(ui, &st, &mut live);
                        });
                    let seat = if board { "on a channel" } else { "off a channel" };
                    if right > face::fit::SLACK {
                        bad.push(format!(
                            "{} {seat} {edge} {}x{}: {right:.1} off the right",
                            font.key(),
                            window.x,
                            window.y
                        ));
                    }
                    if bottom > face::fit::SLACK {
                        bad.push(format!(
                            "{} {seat} {edge} {}x{}: {bottom:.1} off the bottom",
                            font.key(),
                            window.x,
                            window.y
                        ));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the amp is drawn outside its own window:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **A row never gives up its height so the decoration can keep its
    /// own.** [`amp_floor_cost`] lets the panel spend air it can prove it
    /// has, and the order it gives things up in is the whole design: the two
    /// spacers, then the hair under the board's caption, then the chips down
    /// to [`CHIP_MIN_H`] — and never a row, because a knob that shrinks with
    /// the window is a knob nobody can set.
    ///
    /// So: squeezed to a window far too short for it, the panel's rows are
    /// still `face::ROW_H` apart and its chips are still tall enough to hit,
    /// and given a window with room to spare nothing has moved at all.
    #[test]
    fn a_squeezed_amp_gives_up_its_air_and_never_its_rows() {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let ctx = face::fit::a_ctx(fonts::FontChoice::default());
        // The height of the panel at a window with room to spare, and at one
        // with none at all. The difference is the decoration and nothing else.
        let height = |h: f32| {
            let mut live = face::fit::a_worst_case_face(AMP_PATH, 9, &[0, 5], true);
            let mut took = 0.0_f32;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::pos2(0.0, 0.0),
                        egui::Vec2::new(516.0, h),
                    )),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            face::fit::dress(ui.style_mut(), &st.s);
                            took = ui
                                .scope(|ui| {
                                    let _ = draw_amp(ui, &st, &mut live);
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
        // Everything the panel can give up: both spacers, the caption hair,
        // and the chips' travel from full height down to their floor.
        let air = GROUP_AIR + BOARD_AIR + CAPTION_AIR + (CHIP_H - CHIP_MIN_H);
        assert!(
            squeezed < roomy,
            "a squeezed amp gave nothing up: {squeezed} at a short window, \
             {roomy} at a tall one"
        );
        assert!(
            roomy - squeezed <= air + 1.0,
            "the amp gave up {:.1} points, which is more than its decoration \
             is worth ({air:.1}) — a row has started shrinking",
            roomy - squeezed
        );
        // And the rows themselves are untouched: nine rows at the kit's own
        // height still fit inside what is left after the captions and the
        // board at its floor.
        assert!(
            squeezed >= AMP_ROWS * face::ROW_H,
            "the squeezed panel is {squeezed} points, which cannot hold nine \
             rows at {}",
            face::ROW_H
        );
    }
}
