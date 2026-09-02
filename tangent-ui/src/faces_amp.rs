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

use crate::face::{slider_row, stepped_row, FaceStyle};
use crate::faces::{FaceAction, FaceLive};
use crate::fonts;
use crate::recorder::AMP_PEDALS;

/// The amp's sentinel path. The string is the contract with the engine's
/// `builtin_fx::AMP_PATH`, exactly as `SUITE_UNITS` documents for the other
/// four — the two crates cannot share the const across the firewall, so a
/// test on each side pins the spelling.
pub const AMP_PATH: &str = "@tangent-amp";

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

    // ── the top strip: bright, volume, the stack ────────────────────────────
    caption(ui, "CHANNEL");
    out = out.or_else(|| toggle_row(ui, st, live, 0));
    for id in 1..=4 {
        out = out.or_else(|| param_row(ui, st, live, id));
    }
    ui.add_space(6.0);

    // ── the vibrato group ───────────────────────────────────────────────────
    // The panel word is the era's; that it is amplitude tremolo is the
    // module doc's to tell, not a knob's.
    caption(ui, "VIBRATO");
    out = out.or_else(|| toggle_row(ui, st, live, 5));
    for id in 6..=8 {
        out = out.or_else(|| param_row(ui, st, live, id));
    }
    ui.add_space(8.0);

    // ── the pedalboard ──────────────────────────────────────────────────────
    match &live.pedals {
        Some(pedals) => {
            caption(ui, "PEDALS - in order, before the amp");
            ui.add_space(2.0);
            let gap = ui.spacing().item_spacing.x;
            let w = crate::dialogs::cells_across(ui.available_width(), AMP_PEDALS, gap);
            ui.horizontal(|ui| {
                for (pedal, tenant) in pedals.iter().enumerate() {
                    let empty = tenant.is_empty();
                    let label = if empty { "+" } else { tenant.as_str() };
                    let chip = ui.add_sized(
                        egui::Vec2::new(w, 30.0),
                        egui::Button::new(
                            egui::RichText::new(label)
                                .font(bold(9.0))
                                .color(if empty { st.s.faint } else { st.s.ink }),
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
    stepped_row(ui, st, &p.name, label).map(|_| FaceAction::Set {
        id,
        // A switch has two positions; either step is the other one.
        value: if on { 0.0 } else { 1.0 },
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

    /// The face's path const matches the suite table row that routes to it —
    /// one spelling, asserted where both live.
    #[test]
    fn the_amp_path_is_the_suite_tables_spelling() {
        assert!(
            crate::faces::SUITE_UNITS.iter().any(|(p, ..)| *p == AMP_PATH),
            "the amp is not in SUITE_UNITS, so no face would ever open"
        );
    }
}
