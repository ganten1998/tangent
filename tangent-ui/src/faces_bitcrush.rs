//! Tangent Bitcrush's face: a window onto the stair, and the knobs under it.
//!
//! The glass at the top draws what the converter is doing to a tone RIGHT
//! NOW: two cycles of a 440 Hz sine in the quiet tier, and the same sine
//! through the dialled clock and grid in the bright one. RATE widens the
//! treads, BITS deepens the risers, and DRIVE swings the tone across more of
//! the grid: the three knobs that change the shape, shown as the shape.
//! MIX and OUTPUT are levels, not shapes, and stay off the picture so the
//! stair is never a stair of something the knob did not do.
//!
//! The picture uses the kernel's maps, restated here because this crate
//! cannot see the engine's (the firewall `faces::SUITE_UNITS` documents);
//! `builtin_fx`'s tests pin the two copies equal. Under the glass, every
//! parameter is a kit row in id order, the dither a two-position row, so
//! automation, typing a value and double-click-home all work the way they
//! do on every other face. The bypass row and the preset strip arrive from
//! `faces::draw`, as they do for every face.

use egui::{Pos2, Rect, Stroke, Vec2};

use crate::face::{self, lcd_glass, lcd_ink, lcd_text, FaceStyle, LcdTier};
use crate::faces::{FaceAction, FaceLive};

/// The bitcrusher's sentinel path; the engine's `builtin_fx::BITCRUSH_PATH`.
pub const BITCRUSH_PATH: &str = "@tangent-bitcrush";

/// The kernel's BITS sweep, restated. See the module header.
pub const BITS_RANGE: (f32, f32) = (1.0, 24.0);
/// The kernel's RATE sweep, restated; the top of the travel is "full".
pub const RATE_HZ: (f32, f32) = (200.0, 48_000.0);
/// The kernel's DRIVE sweep, restated.
pub const DRIVE_DB: (f32, f32) = (-24.0, 24.0);

/// The parameter ids, in the engine's table order.
const P_BITS: u32 = 0;
const P_RATE: u32 = 1;
const P_DRIVE: u32 = 2;
const P_DITHER: u32 = 5;

/// The tone on the glass, the rate it is drawn at, and how much of it.
const TONE_HZ: f32 = 440.0;
const DRAW_RATE: f32 = 48_000.0;
const CYCLES: f32 = 2.0;
/// Where on its cycle the tone starts: a third of the way up, so a clock
/// slow enough to hold one value across the whole picture holds a value
/// that is not zero, and the reader sees a held level rather than nothing.
const START_PHASE: f32 = std::f32::consts::FRAC_PI_3;
/// The tone's level on the glass at unity drive: most of full scale, with
/// room above it for the stair to round up into.
const TONE_LEVEL: f32 = 0.8;

/// How tall the glass is drawn, and the least it gives up when the window is
/// short. The rows under it are the controls and never shrink; the picture
/// is the part that can.
const GLASS_H: f32 = 64.0;
const GLASS_MIN_H: f32 = 36.0;

/// The tone, sampled at [`DRAW_RATE`] across [`CYCLES`], and the same samples
/// through the converter at the dialled knobs, `(clean, crushed)`, both in
/// full-scale units. The clock and the grid are the kernel's: a fractional
/// phase accumulator that takes a sample whenever it reaches one, and a
/// mid-tread round to `2^(1 − bits)`, out of circuit at the top of the BITS
/// travel. Pure, so the picture can be tested without drawing it.
pub fn staircase(bits_v: f32, rate_v: f32, drive_v: f32) -> (Vec<f32>, Vec<f32>) {
    let n = (DRAW_RATE / TONE_HZ * CYCLES).round() as usize;
    let bits = BITS_RANGE.0 + (BITS_RANGE.1 - BITS_RANGE.0) * bits_v.clamp(0.0, 1.0);
    let grid = (bits < BITS_RANGE.1).then(|| (1.0 - bits).exp2());
    let step = if rate_v >= 1.0 {
        1.0
    } else {
        let hz = RATE_HZ.0 * (RATE_HZ.1 / RATE_HZ.0).powf(rate_v.max(0.0));
        (hz / DRAW_RATE).min(1.0)
    };
    let drive = 10f32.powf((DRIVE_DB.0 + (DRIVE_DB.1 - DRIVE_DB.0) * drive_v.clamp(0.0, 1.0)) / 20.0);
    let mut clean = Vec::with_capacity(n);
    let mut crushed = Vec::with_capacity(n);
    let mut phase = 1.0f32;
    let mut held = 0.0f32;
    for i in 0..n {
        let t = i as f32 / DRAW_RATE;
        let x = TONE_LEVEL * (std::f32::consts::TAU * TONE_HZ * t + START_PHASE).sin();
        if phase >= 1.0 - 1.0e-6 {
            phase -= 1.0;
            let s = x * drive;
            held = grid.map_or(s, |q| (s / q).round() * q);
        }
        phase += step;
        clean.push(x);
        crushed.push(held);
    }
    (clean, crushed)
}

fn value(live: &FaceLive, id: u32) -> Option<f32> {
    live.face.params.iter().find(|p| p.id == id).map(|p| p.value)
}

fn reading(live: &FaceLive, id: u32) -> &str {
    live.face
        .params
        .iter()
        .find(|p| p.id == id)
        .map_or("", |p| p.reading.as_str())
}

/// Paint the glass into `r`: the clean tone quiet, the crushed one bright,
/// and the two readings that shape it in the corner.
fn paint_glass(painter: &egui::Painter, r: Rect, st: &FaceStyle, live: &FaceLive) {
    lcd_glass(painter, r, st);
    let inner = r.shrink(4.0);
    if !inner.is_positive() {
        return;
    }
    let (clean, crushed) = staircase(
        value(live, P_BITS).unwrap_or(1.0),
        value(live, P_RATE).unwrap_or(1.0),
        value(live, P_DRIVE).unwrap_or(0.5),
    );
    let n = clean.len().max(2);
    let x_at = |i: usize| inner.left() + inner.width() * i as f32 / (n - 1) as f32;
    // Full scale fills the glass; anything driven past it rides the edge
    // rather than leaving the picture.
    let y_at = |v: f32| inner.center().y - v.clamp(-1.0, 1.0) * inner.height() * 0.5;
    let quiet = lcd_ink(st, LcdTier::Label);
    let bright = lcd_ink(st, LcdTier::Primary);
    // The zero line, then the tone, then the stair over both.
    painter.line_segment(
        [Pos2::new(inner.left(), y_at(0.0)), Pos2::new(inner.right(), y_at(0.0))],
        Stroke::new(1.0_f32, lcd_ink(st, LcdTier::Secondary).gamma_multiply(0.35)),
    );
    let tone: Vec<Pos2> = clean.iter().enumerate().map(|(i, v)| Pos2::new(x_at(i), y_at(*v))).collect();
    painter.add(egui::Shape::line(tone, Stroke::new(1.0_f32, quiet)));
    // The stair: a tread to the next sample, then a riser to its value.
    let mut stair = Vec::with_capacity(crushed.len() * 2);
    for (i, v) in crushed.iter().enumerate() {
        let y = y_at(*v);
        if i > 0 {
            stair.push(Pos2::new(x_at(i), stair.last().map_or(y, |p: &Pos2| p.y)));
        }
        stair.push(Pos2::new(x_at(i), y));
    }
    painter.add(egui::Shape::line(stair, Stroke::new(1.5_f32, bright)));
    // The two readings that shape the stair, in the glass's own digits.
    let label = format!("{}  {}", reading(live, P_BITS), reading(live, P_RATE));
    let size = face::fit_mono(inner, &label, 10.0);
    lcd_text(painter, inner.left_top(), egui::Align2::LEFT_TOP, &label, size, LcdTier::Primary, st);
}

/// How tall the glass may be in what is left of `ui`: what the rows and
/// the footer will need is set aside first, so the glass only spends what
/// remains: the same bargain the amp strikes, with the picture as the part
/// that gives.
fn glass_height(ui: &egui::Ui, live: &FaceLive) -> f32 {
    let gap = ui.spacing().item_spacing.y;
    let line = ui.text_style_height(&egui::TextStyle::Body);
    let rows = live.face.params.len() as f32 * (face::ROW_H.max(line) + gap);
    let footer = crate::faces::preset_foot_height(ui) + 2.0 * line + gap;
    (ui.available_height() - rows - footer - 6.0).clamp(GLASS_MIN_H, GLASS_H)
}

/// The whole face above the footer: the glass, then every knob as a row.
pub fn draw_bitcrush(ui: &mut egui::Ui, st: &FaceStyle, live: &mut FaceLive) -> Option<FaceAction> {
    let h = glass_height(ui, live);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), h), egui::Sense::hover());
    paint_glass(ui.painter(), rect, st, live);
    ui.add_space(6.0);
    let mut out = None;
    let ids: Vec<(u32, bool)> = live.face.params.iter().map(|p| (p.id, p.steps >= 2 || p.id == P_DITHER)).collect();
    for (id, stepped) in ids {
        face::keep(
            &mut out,
            if stepped {
                crate::faces::step_row(ui, st, &live.face, id, None)
            } else {
                crate::faces::row(ui, st, &live.face, id)
            },
        );
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::face::fit;

    /// **The stair is the kernel's stair**: at one bit the crushed tone
    /// takes only the levels {−1, 0, +1}; at the top of BITS and RATE it IS
    /// the tone; at a 12 kHz clock it holds each value for four samples of
    /// the 48 kHz drawing; and driving it up spreads it across more levels.
    #[test]
    fn the_stair_follows_the_bits_the_clock_and_the_drive() {
        let (clean, wire) = staircase(1.0, 1.0, 0.5);
        assert_eq!(clean, wire, "the wire's stair is not the tone");
        let (_, one) = staircase(0.0, 1.0, 0.5);
        assert!(one.iter().all(|v| [-1.0, 0.0, 1.0].contains(v)), "one bit drew a fourth level");
        let rate_12k = (12_000.0f32 / RATE_HZ.0).ln() / (RATE_HZ.1 / RATE_HZ.0).ln();
        let (_, held) = staircase(1.0, rate_12k, 0.5);
        let mut runs = Vec::new();
        let mut len = 1;
        for w in held.windows(2) {
            if w[0] == w[1] {
                len += 1;
            } else {
                runs.push(len);
                len = 1;
            }
        }
        assert!(runs.iter().all(|r| *r == 4), "a 12 kHz clock held for {runs:?}");
        let levels = |drive: f32| {
            let (_, s) = staircase(3.0 / 23.0, 1.0, drive);
            let mut v: Vec<i64> = s.iter().map(|x| (x * 1_000.0).round() as i64).collect();
            v.sort_unstable();
            v.dedup();
            v.len()
        };
        assert!(levels(0.75) > levels(0.5), "driving the tone used no more of the grid");
        // A clock slower than the picture holds one non-zero value.
        let (_, slow) = staircase(1.0, 0.0, 0.5);
        assert!(slow.iter().all(|v| *v == slow[0]) && slow[0] != 0.0);
    }

    /// **The face paints inside the room it is given, at every width a
    /// window can be dragged to, and the glass gives way before a row
    /// does.** The shelf's fit sweeps (`face::fit::FACES`) measure the
    /// whole window; this is the bitcrusher's own, across the widths and a
    /// window short enough to squeeze the glass to its floor.
    #[test]
    fn the_bitcrusher_paints_within_its_bounds() {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let ctx = fit::a_ctx(crate::fonts::FontChoice::default());
        let about = crate::faces::suite_unit(BITCRUSH_PATH).map(|(_, a)| a).expect("a suite unit");
        for w in [320.0f32, 400.0, 520.0, 640.0, 900.0] {
            for h in [440.0f32, 520.0, 704.0] {
                let mut live = fit::a_worst_case_face(BITCRUSH_PATH, 6, &[5], false);
                let (right, bottom) = fit::escape(&ctx, Vec2::new(w, h), about, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                assert!(right <= fit::SLACK, "at {w}x{h} the face ran {right:.1} off the right");
                assert!(bottom <= fit::SLACK, "at {w}x{h} the face ran {bottom:.1} off the bottom");
            }
        }
        // And at the size it OPENS at, the picture is whole on every font:
        // a glass squeezed at its own window is a floor set too low.
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            let live = fit::a_worst_case_face(BITCRUSH_PATH, 6, &[5], false);
            let mut got = 0.0;
            let _ = fit::escape(&ctx, Vec2::new(400.0, 440.0), about, &st.s, &mut |ui| {
                got = glass_height(ui, &live);
            });
            assert_eq!(got, GLASS_H, "on {} the glass opens squeezed to {got}", font.key());
        }
    }
}
