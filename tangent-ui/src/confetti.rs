//! A crappy rainbow confetti burst — the owner's words, and the owner's
//! spec: this is celebration, not craft. It plays when a supporter key
//! lands (either the honest way or the Konami way), dims nothing and owns
//! nothing; the caller decides what sits under it.
//!
//! Deterministic on purpose: no `rand` dependency, just a fixed-increment
//! hash off each particle's index, so the same burst can be drawn in a
//! test and asserted about, and two machines throw the same party.

use egui::{Color32, Painter, Pos2, Rect, Vec2};

/// How long the party lasts, seconds. The tail fades, so it reads shorter.
pub const DURATION_S: f32 = 3.2;

const PARTICLES: usize = 140;

/// The rainbow, as an assortment rather than a gradient — a gradient would
/// look designed, and the spec says crappy.
const RAINBOW: [Color32; 7] = [
    Color32::from_rgb(0xe8, 0x3a, 0x3a),
    Color32::from_rgb(0xf0, 0x8c, 0x2a),
    Color32::from_rgb(0xf2, 0xd0, 0x2e),
    Color32::from_rgb(0x3f, 0xc4, 0x4f),
    Color32::from_rgb(0x2e, 0x9c, 0xf2),
    Color32::from_rgb(0x5a, 0x4b, 0xe8),
    Color32::from_rgb(0xb8, 0x4f, 0xe0),
];

/// A cheap deterministic hash: index -> [0, 1). Good enough for a party.
fn h(i: usize, salt: u32) -> f32 {
    let mut x = (i as u32).wrapping_mul(2_654_435_761).wrapping_add(salt.wrapping_mul(40_503));
    x ^= x >> 15;
    x = x.wrapping_mul(2_246_822_519);
    x ^= x >> 13;
    (x & 0xFFFF) as f32 / 65_536.0
}

/// Draw the burst at `t` seconds in. Answers whether the party is still
/// going — `false` means the caller can drop its confetti state and stop
/// asking for repaints.
pub fn draw(painter: &Painter, rect: Rect, t: f32) -> bool {
    if !(0.0..DURATION_S).contains(&t) || !rect.is_positive() {
        return false;
    }
    // The last stretch fades everything together rather than particle by
    // particle: a hard pop-out at the end reads as a glitch, not an ending.
    let fade = ((DURATION_S - t) / 0.8).clamp(0.0, 1.0);
    for i in 0..PARTICLES {
        // Where it enters, when it enters, how fast it falls, how it sways.
        let x0 = h(i, 1);
        let delay = h(i, 2) * 0.9;
        let life = t - delay;
        if life < 0.0 {
            continue;
        }
        let speed = 0.28 + h(i, 3) * 0.5; // screen-heights per second
        let sway = (h(i, 4) - 0.5) * 0.16;
        let wobble = (life * (2.0 + h(i, 5) * 4.0) + h(i, 6) * 6.28).sin();
        let y = life * speed;
        if y > 1.1 {
            continue;
        }
        let x = (x0 + sway * life + wobble * 0.015).rem_euclid(1.0);
        let c = RAINBOW[i % RAINBOW.len()];
        let size = 3.0 + h(i, 7) * 6.0;
        let centre = Pos2::new(
            rect.left() + x * rect.width(),
            rect.top() + y * rect.height(),
        );
        // A "rotating" rectangle on the cheap: the width breathes with the
        // wobble, which is what a tumbling paper scrap looks like from far
        // enough away that nobody is grading the physics.
        let w = size * (0.35 + 0.65 * wobble.abs());
        painter.rect_filled(
            Rect::from_center_size(centre, Vec2::new(w, size)),
            1.0,
            c.gamma_multiply(fade),
        );
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The party ends, and says so.** A caller keeps repainting for as
    /// long as `draw` answers true; an overlay that never says false is a
    /// per-frame repaint for the rest of the session.
    #[test]
    fn the_party_ends_and_says_so() {
        let ctx = egui::Context::default();
        let mut ended = true;
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let r = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
                assert!(draw(ui.painter(), r, 0.1), "the party never started");
                assert!(draw(ui.painter(), r, DURATION_S * 0.9));
                ended = !draw(ui.painter(), r, DURATION_S + 0.01);
            });
        });
        assert!(ended, "the party never ends");
    }
}
