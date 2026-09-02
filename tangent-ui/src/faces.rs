//! The four suite faces, drawn from the face kit.
//!
//! One `draw` for all of them: the path in the pushed [`BuiltinFace`] picks
//! the layout, the kit supplies every part, and what a face returns is a
//! wish (`FaceAction`) the app maps onto requests. Nothing here touches
//! `Settings`, the engine, or a disk — a face is a picture of pushed data
//! and a stream of small asks, which is what lets the same code sit in the
//! desktop and (one day) the plugin build unchanged.

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
pub const SUITE_UNITS: [(&str, &str, &str); 4] = [
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
}

/// What a face asked for. `Set` speaks the unit's own stable ids; the
/// preset wishes carry only names and paths — the app owns every request.
pub enum FaceAction {
    Set { id: u32, value: f32 },
    SavePreset { name: String },
    LoadPreset { path: String },
    DeletePreset { path: String },
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
/// stepping wraps.
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
    stepped_row(ui, st, name_override.unwrap_or(&p.name), label).map(|delta| {
        let next = (at as i32 + delta).rem_euclid(steps as i32) as usize;
        FaceAction::Set {
            id,
            value: next as f32 / (steps - 1) as f32,
        }
    })
}

/// The preset strip every face wears at its foot: the saved sounds, a name
/// to save under, and the two buttons. The host scans the folder and pushes
/// the rows; sharing a preset IS sharing the file, which is the point.
fn preset_strip(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let mut action = None;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let bold = |size: f32| egui::FontId::new(size, crate::fonts::courier_bold());
        ui.label(
            egui::RichText::new("PRESET")
                .font(bold(10.0))
                .color(st.s.faint),
        );
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
            .width(120.0);
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
            .desired_width(110.0);
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

/// The whole face: glass on top where a unit has a picture, rows under it,
/// presets at the foot. Returns at most one ask per frame — a hand only
/// does one thing at a time, and a queue of asks is a queue of bugs.
pub fn draw(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let path = live.face.path.clone();
    let mut action = match path.as_str() {
        "@tangent-eq" => draw_eq(ui, st, live),
        "@tangent-imager" => draw_imager(ui, st, live),
        "@tangent-comp" => draw_comp(ui, st, live),
        // The tape, the trim, and anything the suite grows before its face
        // does: every continuous parameter as a row, in id order.
        _ => {
            let mut out = None;
            let ids: Vec<u32> = live.face.params.iter().map(|p| p.id).collect();
            for id in ids {
                out = out.or_else(|| row(ui, st, &live.face, id));
            }
            out
        }
    };
    action = action.or_else(|| preset_strip(ui, st, live));
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
        out = out.or_else(|| row(ui, st, &live.face, id));
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
        out = out.or_else(|| row(ui, st, &live.face, id));
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
    if stepped_row(ui, st, "Band", &format!("{}", band + 1)).is_some() {
        live.band = (band + 1) % EQ_BANDS;
    }
    out = out.or_else(|| step_row(ui, st, &live.face, base, None));
    out = out.or_else(|| step_row(ui, st, &live.face, base + 1, Some("Shape")));
    out = out.or_else(|| row(ui, st, &live.face, base + 2));
    out = out.or_else(|| row(ui, st, &live.face, base + 3));
    out = out.or_else(|| row(ui, st, &live.face, base + 4));
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
        // And it falls once the input stops arriving.
        let before = there;
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
}
