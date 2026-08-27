//! What a screen reader and a keyboard can reach on a surface made of one
//! `Painter`.
//!
//! # The problem this solves
//!
//! Every instrument surface in this app is a free function — `draw(painter,
//! rect, view)` paired with `hit_test(rect, view, pos)` — and not a single
//! `egui::Widget` among them. That is a deliberate and good decision: it is
//! what lets the same code paint a desktop window, a plugin editor and an
//! offscreen video frame. It also means egui knows nothing is there. No
//! focus, no tab order, and — with AccessKit switched on — a window a screen
//! reader reads as one empty rectangle.
//!
//! # The shape of the answer
//!
//! A panel already ENUMERATES its controls; `hit_test` is that enumeration
//! with a point in its hand. So each panel gains a second function that does
//! the same walk without a point and hands back what it found. Those become
//! invisible `egui` widgets laid over the painted ones, allocated with
//! [`egui::Sense::focusable_noninteractive`].
//!
//! **That sense is the whole trick.** It senses no clicks and no drags, so
//! every existing pointer path is untouched — the panels go on reading
//! `ctx.input(|i| i.pointer …)` exactly as they did, and nothing this module
//! adds can steal a gesture. What it DOES give is a tab stop and an
//! `egui::Response`, and `Response::widget_info` turns that into an AccessKit
//! node for free. Focus order, keyboard navigation and the screen reader all
//! come from one allocation.
//!
//! # What a node says
//!
//! A label and a value, in the words a person uses. `-4.5 dB`, not `0.596`;
//! `bar 17 beat 1`, not `36.4 seconds`. A readout that has to be decoded is a
//! readout that has not been made accessible, only made noisy.

use egui::{Color32, Id, Painter, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui, Vec2};

/// What kind of thing a node is, in the terms a screen reader has words for.
#[derive(Debug, Clone, PartialEq)]
pub enum Kind {
    /// Does something when pressed.
    Button,
    /// On or off, and says which.
    Toggle(bool),
    /// A continuous control — a fader, a knob, a dial.
    ///
    /// `now` is what it reads; `min` and `max` are what it can reach. All
    /// three in the control's OWN units, so a screen reader announcing a
    /// percentage is announcing the number on the panel.
    Slider { now: f64, min: f64, max: f64 },
    /// Something to be read and not operated: the chord name, the staff, the
    /// neck, a clip on a lane.
    ///
    /// **The half of this that is not about controls at all.** A theory app a
    /// blind student cannot read the notes from has been made operable and
    /// not useful.
    Readout,
}

/// One thing on a painted surface that a keyboard can reach and a screen
/// reader can say.
#[derive(Debug, Clone)]
pub struct Node {
    /// Stable across frames, so focus survives a repaint. Built from the
    /// panel's own name and the control's identity, never from its position —
    /// a node keyed by where it is loses focus the moment the window is
    /// resized, which is now something that happens.
    pub id: Id,
    pub rect: Rect,
    pub kind: Kind,
    /// What it is. "Track 3 Guitar volume", "Record", "Staff, treble".
    pub label: String,
    /// What it reads, already in words. `None` for a control whose label is
    /// the whole story.
    pub value: Option<String>,
    pub enabled: bool,
}

impl Node {
    pub fn button(id: Id, rect: Rect, label: impl Into<String>) -> Self {
        Self {
            id,
            rect,
            kind: Kind::Button,
            label: label.into(),
            value: None,
            enabled: true,
        }
    }

    pub fn toggle(id: Id, rect: Rect, label: impl Into<String>, on: bool) -> Self {
        Self {
            id,
            rect,
            kind: Kind::Toggle(on),
            label: label.into(),
            value: None,
            enabled: true,
        }
    }

    pub fn slider(
        id: Id,
        rect: Rect,
        label: impl Into<String>,
        now: f64,
        min: f64,
        max: f64,
        reads: impl Into<String>,
    ) -> Self {
        Self {
            id,
            rect,
            kind: Kind::Slider { now, min, max },
            label: label.into(),
            value: Some(reads.into()),
            enabled: true,
        }
    }

    pub fn readout(id: Id, rect: Rect, label: impl Into<String>, reads: impl Into<String>) -> Self {
        Self {
            id,
            rect,
            kind: Kind::Readout,
            label: label.into(),
            value: Some(reads.into()),
            enabled: true,
        }
    }

    #[must_use]
    pub fn enabled(mut self, yes: bool) -> Self {
        self.enabled = yes;
        self
    }

    /// Everything a screen reader says about this node, in order.
    ///
    /// Its own function because it is what the tests assert against: a node
    /// whose spoken form is wrong is not visibly wrong anywhere else.
    pub fn spoken(&self) -> String {
        let mut out = self.label.clone();
        if let Some(v) = &self.value {
            out.push_str(", ");
            out.push_str(v);
        }
        match self.kind {
            Kind::Toggle(on) => {
                out.push_str(if on { ", on" } else { ", off" });
            }
            Kind::Button | Kind::Slider { .. } | Kind::Readout => {}
        }
        if !self.enabled {
            out.push_str(", unavailable");
        }
        out
    }
}

/// Lay the nodes over a painted surface and hand back what egui made of them.
///
/// The returned responses are in the order given, so a caller can map an index
/// back to whatever it built the node from.
///
/// **Allocation order IS tab order**, which is why panels emit their nodes in
/// reading order rather than in whatever order the drawing code happens to
/// visit them.
pub fn install(ui: &Ui, nodes: &[Node]) -> Vec<Response> {
    nodes
        .iter()
        .map(|node| {
            let r = ui.interact(node.rect, node.id, Sense::focusable_noninteractive());
            r.widget_info(|| info(node));
            r
        })
        .collect()
}

fn info(node: &Node) -> egui::WidgetInfo {
    use egui::{WidgetInfo, WidgetType};
    match node.kind {
        Kind::Button => WidgetInfo::labeled(WidgetType::Button, node.enabled, node.spoken()),
        Kind::Toggle(on) => {
            WidgetInfo::selected(WidgetType::Checkbox, node.enabled, on, node.spoken())
        }
        // The VALUE goes in as a number as well as in the spoken label: a
        // screen reader that supports value ranges will say "40 percent" and
        // let the user step it, and one that does not still reads the words.
        Kind::Slider { now, .. } => WidgetInfo::slider(node.enabled, now, node.spoken()),
        Kind::Readout => WidgetInfo::labeled(WidgetType::Label, node.enabled, node.spoken()),
    }
}

/// How far a slider node moves for one press of an arrow key, as a fraction
/// of its travel. Shift is [`FINE_STEP`].
pub const STEP: f32 = 0.05;
/// The fine step, matching the Shift-drag the mouse already has.
pub const FINE_STEP: f32 = 0.01;

/// **The focus ring: two rings, and neither of them is themed.**
///
/// A single ring in a themed accent is the eighth instance of the mistake
/// this app has now made seven times — a colour picked once and then asked to
/// be legible against every surface it can land on. A focus ring lands on
/// ALL of them: the near-black meter well, the ivory keys, sixteen palettes
/// of channel colour, the wine of the band. There is no single colour that
/// contrasts with all of it, and the dead zone at 0.12–0.22 luminance is
/// exactly where several of those grounds sit.
///
/// So the ring does not try. It is drawn twice, once in each of the two inks
/// this app has — near-black inside, ivory outside — and whichever one the
/// ground is not, wins. That is how the platforms solve it too, and it makes
/// the ring's legibility independent of the theme rather than something that
/// has to be re-verified on every palette anyone ever adds.
///
/// `accent` is the theme's own colour and is drawn BETWEEN the two, where it
/// is decoration: it makes the ring look like it belongs to the app without
/// ever being the thing that has to be visible.
pub fn draw_focus_ring(painter: &Painter, rect: Rect, accent: Color32) {
    // Outside the control rather than on it, so the ring never covers the
    // thing it is pointing at — a fader whose value is hidden under its own
    // focus ring is worse off than one with no ring.
    let r = rect.expand(2.0);
    let ring = |inset: f32, colour: Color32, width: f32| {
        painter.rect_stroke(
            r.expand(inset),
            2.0,
            Stroke::new(width, colour),
            StrokeKind::Middle,
        );
    };
    ring(0.0, crate::recorder_panel::INK_DARK, 1.0);
    ring(1.0, accent, 1.0);
    ring(2.0, crate::recorder_panel::INK_LIGHT, 1.0);
}

/// Where the ring will be drawn, for a test that wants to know it is inside
/// the surface it belongs to.
pub fn focus_ring_bounds(rect: Rect) -> Rect {
    rect.expand(4.0)
}

/// The keyboard gesture a focused node just received, if any.
///
/// Read once per frame and routed by the app, so every panel answers the same
/// keys: Space and Enter press a thing, the arrows move a value, Home and End
/// take it to its ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gesture {
    /// Space or Enter.
    Activate,
    /// An arrow, in the direction that RAISES the value, with `fine` set when
    /// Shift was held.
    Increase { fine: bool },
    Decrease { fine: bool },
    Least,
    Most,
}

impl Gesture {
    /// What the focused node should do about this frame's keys.
    ///
    /// **Both axes raise and lower the same value.** Up and Right increase;
    /// Down and Left decrease. A fader is vertical and a dial is round, and
    /// asking somebody to remember which of the two a given control is before
    /// they can nudge it is the kind of correctness nobody wants.
    pub fn read(input: &egui::InputState) -> Option<Self> {
        use egui::Key;
        let fine = input.modifiers.shift;
        // Modified presses belong to the app's own shortcuts, not here — the
        // one exception being Shift, which is this app's established "finer"
        // and cannot be a shortcut on its own.
        if input.modifiers.command || input.modifiers.alt || input.modifiers.ctrl {
            return None;
        }
        if input.key_pressed(Key::Space) || input.key_pressed(Key::Enter) {
            return Some(Self::Activate);
        }
        if input.key_pressed(Key::ArrowUp) || input.key_pressed(Key::ArrowRight) {
            return Some(Self::Increase { fine });
        }
        if input.key_pressed(Key::ArrowDown) || input.key_pressed(Key::ArrowLeft) {
            return Some(Self::Decrease { fine });
        }
        if input.key_pressed(Key::Home) {
            return Some(Self::Least);
        }
        if input.key_pressed(Key::End) {
            return Some(Self::Most);
        }
        None
    }

    /// Apply this gesture to a normalised value, answering the new one.
    ///
    /// Normalised because that is what every control in this app is underneath
    /// — a fader is a curve over 0..1, a knob is a fraction of a sweep — and
    /// the alternative is a step size per control that nobody keeps in step.
    pub fn nudged(self, now: f32) -> f32 {
        let step = |fine: bool| if fine { FINE_STEP } else { STEP };
        match self {
            Self::Increase { fine } => (now + step(fine)).clamp(0.0, 1.0),
            Self::Decrease { fine } => (now - step(fine)).clamp(0.0, 1.0),
            Self::Least => 0.0,
            Self::Most => 1.0,
            Self::Activate => now,
        }
    }
}

/// A rect's centre, for a panel that routes a keyboard press through its own
/// `hit_test` rather than growing a second way to say what a control does.
///
/// **The point of this is that there is one path and not two.** A keyboard
/// that reached the controls by its own route would drift from the mouse's
/// the first time either changed, and the drift would be silent: the thing
/// still works with a mouse, so nobody looks.
pub fn press_point(rect: Rect) -> Pos2 {
    rect.center()
}

/// How much room a focus ring needs around a control, so a panel laying out
/// its own controls can leave it.
pub const RING_SLACK: Vec2 = Vec2::splat(4.0);

#[cfg(test)]
mod tests {
    use super::*;

    fn r() -> Rect {
        Rect::from_min_size(Pos2::new(10.0, 10.0), Vec2::new(40.0, 20.0))
    }

    /// A node says its label, then its value, then its state.
    #[test]
    fn a_node_says_what_it_is_and_what_it_reads() {
        let id = Id::new("x");
        assert_eq!(
            Node::slider(id, r(), "Track 3 Guitar volume", 0.6, 0.0, 1.0, "-4.5 dB").spoken(),
            "Track 3 Guitar volume, -4.5 dB"
        );
        assert_eq!(Node::button(id, r(), "Record").spoken(), "Record");
        assert_eq!(
            Node::toggle(id, r(), "Track 3 mute", true).spoken(),
            "Track 3 mute, on"
        );
        assert_eq!(
            Node::toggle(id, r(), "Track 3 mute", false).spoken(),
            "Track 3 mute, off"
        );
        assert_eq!(
            Node::readout(id, r(), "Chord", "C minor 7, first inversion").spoken(),
            "Chord, C minor 7, first inversion"
        );
        // A control that cannot be used says so, rather than being silently
        // read out as if it could.
        assert_eq!(
            Node::button(id, r(), "Undo").enabled(false).spoken(),
            "Undo, unavailable"
        );
    }

    /// Both axes move the value the same way.
    #[test]
    fn the_arrows_agree_about_which_way_is_up() {
        let up = Gesture::Increase { fine: false };
        let down = Gesture::Decrease { fine: false };
        assert!((up.nudged(0.5) - 0.55).abs() < 1e-6);
        assert!((down.nudged(0.5) - 0.45).abs() < 1e-6);
        // Shift is finer, matching the Shift-drag the mouse already has.
        assert!((Gesture::Increase { fine: true }.nudged(0.5) - 0.51).abs() < 1e-6);
        // And nothing leaves the rails.
        assert!((up.nudged(0.99) - 1.0).abs() < 1e-6);
        assert!((down.nudged(0.01) - 0.0).abs() < 1e-6);
        assert!((Gesture::Least.nudged(0.7) - 0.0).abs() < 1e-6);
        assert!((Gesture::Most.nudged(0.7) - 1.0).abs() < 1e-6);
        // Activate is not a nudge.
        assert!((Gesture::Activate.nudged(0.7) - 0.7).abs() < 1e-6);
    }

    /// The ring sits OUTSIDE the control, never over it.
    #[test]
    fn the_focus_ring_does_not_cover_what_it_points_at() {
        let bounds = focus_ring_bounds(r());
        assert!(bounds.contains_rect(r()), "the ring does not surround the control");
        assert!(
            bounds.min.x < r().min.x && bounds.max.x > r().max.x,
            "the ring is drawn on the control rather than around it"
        );
    }
}
