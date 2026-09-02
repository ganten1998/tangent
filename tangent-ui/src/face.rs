//! The face kit: one set of parts for every built-in effect's panel.
//!
//! **Why a kit and not four panels.** The suite's promise is that Tangent EQ,
//! Tangent Imager, Tangent Tape and Tangent Comp read as one family — the
//! recorder's panel grammar, the timecode's glass, the theme laws — and four
//! faces built by hand converge on that only by discipline. Built from this
//! file they cannot diverge: the frame, the rows, the glass and the meters
//! are the same functions, and a face is only its contents. When the eight
//! legacy effects are re-exposed as inserts one day, their faces are near
//! free for the same reason.
//!
//! **The laws this file answers to** (all inherited from `recorder_panel`,
//! never re-derived): ink is a property of the SURFACE, measured, never
//! guessed from a theme; a surface that cannot carry ink MOVES
//! (`legible_surface`); fixed-meaning colours stay fixed whatever the theme
//! does. The glass is the timecode cluster's own idiom — `lcd` digits on
//! `lcd_face`, `FontId::monospace`, brightness tiers 1.0 / 0.55 / 0.45,
//! sized against the widest reading a field can show so nothing ever
//! resizes. A sweep test at the bottom dresses the kit in all sixteen
//! factory themes, both modes, and fails the build on the first layer that
//! cannot be read.

use egui::{Align2, Color32, FontId, Painter, Rect, Stroke, StrokeKind, Vec2};

use crate::fonts;
use crate::recorder_panel::{contrast_ratio, distinct_surface, luminance, surface_of, Surface};

/// Everything a face needs to dress itself: the dialog surface, and the
/// cluster's glass. Built once per frame by whoever opens the face, from the
/// same settings colours the timecode reads, and handed down — the kit never
/// reaches into `Settings` itself, because the plugin build shares these
/// panels and a widget that reads global state is a widget that cannot be
/// tested by handing it colours.
#[derive(Clone, Copy)]
pub struct FaceStyle {
    pub s: Surface,
    /// The digits and the trace. `scope_draw` role.
    pub lcd: Color32,
    /// The glass they sit on: the `meter_bg` role, pushed off the panel by
    /// `distinct_surface` — the loudness meter's own cure for the theme
    /// whose glass happens to be the panel's colour, where an un-nudged
    /// meter is an invisible rectangle somebody has to know is there.
    pub lcd_face: Color32,
    /// The three tiers, resolved once: see `lcd_ink`.
    tiers: [Color32; 3],
}

/// A straight-line blend, channel by channel. Used only to place a tier
/// between the glass and the digits; anything perceptual goes through the
/// measured laws instead.
fn lerp(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let ch = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(ch(a.r(), b.r()), ch(a.g(), b.g()), ch(a.b(), b.b()))
}

impl FaceStyle {
    pub fn new(face: Color32, lcd: Color32, lcd_face: Color32) -> Self {
        let s = surface_of(face);
        let mut glass = distinct_surface(lcd_face, s.bg);
        // **When the digits cannot clear the glass, the GLASS moves** — the
        // dead-zone law, applied to the cluster: one shipping theme's pair
        // never reaches reading contrast at any brightness, and brightening
        // ink that is already at full is not an option. Walk the glass away
        // from the digits' luminance until the pair reads AND the glass is
        // still findable on the panel.
        let away = if luminance(lcd) > luminance(glass) {
            Color32::BLACK
        } else {
            Color32::WHITE
        };
        let mut guard = 0;
        while (contrast_ratio(lcd, glass) < 3.0
            || contrast_ratio(glass, s.bg) < 1.25)
            && guard < 40
        {
            glass = lerp(glass, away, 0.06);
            guard += 1;
        }
        // **The tiers are the timecode's, HELD legible.** Raw gamma is what
        // the cluster prints, and on most themes that is exactly what these
        // resolve to — but a theme whose glass pair runs dim would fade the
        // secondary tier into the glass (the sweep test caught two shipping
        // themes doing it). So each tier starts at its blend and walks back
        // toward the full digit colour until it clears its floor: hierarchy
        // is sacrificed before legibility, in that order.
        let tier = |blend: f32, floor: f32| -> Color32 {
            let mut t = blend;
            let mut c = lerp(glass, lcd, t);
            while contrast_ratio(c, glass) < floor && t < 1.0 {
                t += 0.05;
                c = lerp(glass, lcd, t);
            }
            c
        };
        FaceStyle {
            s,
            lcd,
            lcd_face: glass,
            tiers: [tier(1.0, 3.0), tier(0.62, 2.0), tier(0.50, 1.6)],
        }
    }
}

/// The three brightnesses the cluster prints at. The hierarchy IS the
/// readability: the value is the story, the caption is not.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum LcdTier {
    /// The reading itself.
    Primary,
    /// The secondary reading beside it.
    Secondary,
    /// Captions, separators, grid labels.
    Label,
}

impl LcdTier {
    fn index(self) -> usize {
        match self {
            LcdTier::Primary => 0,
            LcdTier::Secondary => 1,
            LcdTier::Label => 2,
        }
    }
}

/// The glass: fill plus a hairline inside it, exactly as the timecode draws
/// its own (`draw_timecode_cluster`).
pub fn lcd_glass(painter: &Painter, r: Rect, st: &FaceStyle) {
    painter.rect_filled(r, 2.0, st.lcd_face);
    painter.rect_stroke(r, 2.0, Stroke::new(1.0, st.s.edge), StrokeKind::Inside);
}

/// Ink at a tier — resolved at `FaceStyle::new`, held legible there.
pub fn lcd_ink(st: &FaceStyle, tier: LcdTier) -> Color32 {
    st.tiers[tier.index()]
}

/// Print on the glass. Monospace on purpose — the cluster's own reasoning:
/// these are readouts, and monospace is what makes the digits line up.
pub fn lcd_text(
    painter: &Painter,
    at: egui::Pos2,
    align: Align2,
    text: &str,
    size: f32,
    tier: LcdTier,
    st: &FaceStyle,
) {
    painter.text(at, align, text, FontId::monospace(size), lcd_ink(st, tier));
}

/// The size at which `widest` fits `r` — the cluster's rule: size against
/// the widest reading a field can ever show, so the digits never resize as
/// the value moves. Monospace advance is ~0.60 of the point size.
pub fn fit_mono(r: Rect, widest: &str, nominal: f32) -> f32 {
    let n = widest.chars().count().max(1) as f32;
    let by_width = r.width() / (n * 0.60);
    nominal.min(by_width).min(r.height()).max(5.0)
}

// ── rows ────────────────────────────────────────────────────────────────────

/// How wide the label column and the reading column sit, as fractions of the
/// row — the effect panels' own split (label left of 0.40, reading right of
/// 0.86), which is also the fader rows'.
const LABEL_W: f32 = 0.34;
const READING_W: f32 = 0.19;

/// One continuous parameter as a row: name, slider, reading. Returns the new
/// value when the hand moved it. The kit's contract, in one place:
/// double-click puts the knob back to `default` (never a UI constant — the
/// caller passes the DSP's own number), the hover says so, and the reading
/// dims when the knob is at rest, all exactly as the keyboard panel does.
pub fn slider_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    reading: &str,
    value: f32,
    default: f32,
) -> Option<f32> {
    let mut out = None;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let bold = |size: f32| FontId::new(size, fonts::courier_bold());
        ui.add_sized(
            Vec2::new(w * LABEL_W, 18.0),
            egui::Label::new(
                egui::RichText::new(name).font(bold(11.0)).color(st.s.ink),
            ),
        );
        let mut v = value;
        ui.spacing_mut().slider_width = w * (1.0 - LABEL_W - READING_W) - 16.0;
        let resp = ui.add(egui::Slider::new(&mut v, 0.0..=1.0).show_value(false));
        if resp.double_clicked() {
            out = Some(default);
        } else if resp.changed() {
            out = Some(v);
        }
        resp.on_hover_text("double-click to reset");
        let at_rest = (value - default).abs() < 0.5 / 100.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(reading)
                    .font(FontId::monospace(10.0))
                    .color(if at_rest { st.s.faint } else { st.s.ink }),
            );
        });
    });
    out
}

/// One stepped parameter as a row: name, a cycling button wearing the current
/// choice, a step either way from its ends. Returns +1/-1 when stepped.
/// A click on the body cycles forward — the effect panels' own idiom for a
/// mode: fewer targets, and the list is short enough to walk.
pub fn stepped_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    choice: &str,
) -> Option<i32> {
    let mut out = None;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let bold = |size: f32| FontId::new(size, fonts::courier_bold());
        ui.add_sized(
            Vec2::new(w * LABEL_W, 18.0),
            egui::Label::new(
                egui::RichText::new(name).font(bold(11.0)).color(st.s.ink),
            ),
        );
        if ui
            .add_sized(
                Vec2::new(w * (1.0 - LABEL_W) - 40.0, 18.0),
                egui::Button::new(
                    egui::RichText::new(choice).font(bold(10.0)).color(st.s.ink),
                )
                .fill(st.s.raised),
            )
            .on_hover_text("click to cycle; right-click steps back")
            .clicked()
        {
            out = Some(1);
        }
        let back = ui.add_sized(
            Vec2::new(22.0, 18.0),
            egui::Button::new(
                egui::RichText::new("‹").font(bold(11.0)).color(st.s.faint),
            )
            .fill(st.s.raised),
        );
        if back.clicked() {
            out = Some(-1);
        }
    });
    out
}

// ── meters ──────────────────────────────────────────────────────────────────

/// Correlation's red: the fixed-meaning family (record red, limiter red) —
/// out-of-phase is a warning on every theme, so no theme may repaint it.
pub const CORR_RED: Color32 = Color32::from_rgb(0xC8, 0x37, 0x2E);

/// A gain-reduction readout on the glass: scale labels, a bar growing DOWN
/// from zero (reduction is a taking-away, and the bar should look like one),
/// and the figure printed at the cluster's primary tier.
pub fn gr_meter(painter: &Painter, r: Rect, st: &FaceStyle, gr_db: f32) {
    lcd_glass(painter, r, st);
    let pad = (r.height() * 0.14).clamp(3.0, 10.0);
    let inner = r.shrink(pad);
    // The scale: 0 at the left edge, 24 dB of travel. Sized on the widest
    // caption so the labels never move.
    const FULL_DB: f32 = 24.0;
    let label_size = fit_mono(
        Rect::from_min_size(inner.min, Vec2::new(inner.width() * 0.16, inner.height() * 0.3)),
        "-24",
        10.0,
    );
    for db in [0.0_f32, 6.0, 12.0, 18.0, 24.0] {
        let x = inner.left() + inner.width() * (db / FULL_DB);
        painter.line_segment(
            [
                egui::pos2(x, inner.top()),
                egui::pos2(x, inner.top() + inner.height() * 0.18),
            ],
            Stroke::new(1.0, lcd_ink(st, LcdTier::Label)),
        );
        lcd_text(
            painter,
            egui::pos2(x, inner.top() + inner.height() * 0.22),
            Align2::CENTER_TOP,
            &format!("{db:.0}"),
            label_size,
            LcdTier::Label,
            st,
        );
    }
    // The bar. Instantaneous is unreadable; the caller hands the ballistic
    // value (the engine's read-and-reset max per frame is already a hold).
    let clamped = gr_db.clamp(0.0, FULL_DB);
    let bar_h = (inner.height() * 0.24).max(3.0);
    let bar = Rect::from_min_max(
        egui::pos2(inner.left(), inner.bottom() - bar_h),
        egui::pos2(
            inner.left() + inner.width() * (clamped / FULL_DB),
            inner.bottom(),
        ),
    );
    painter.rect_filled(bar, 1.0, lcd_ink(st, LcdTier::Primary));
    // And the figure, where the eye lands.
    lcd_text(
        painter,
        egui::pos2(inner.right(), inner.top()),
        Align2::RIGHT_TOP,
        &format!("-{clamped:>4.1} dB"),
        fit_mono(inner, "-24.0 dB", inner.height() * 0.34),
        LcdTier::Primary,
        st,
    );
}

/// A correlation readout on the glass: -1 to +1, a bar from the centre, the
/// negative side in the warning red. `c` outside the range is clamped — a
/// meter must never lie further than its scale.
pub fn correlation_meter(painter: &Painter, r: Rect, st: &FaceStyle, c: f32) {
    lcd_glass(painter, r, st);
    let pad = (r.height() * 0.18).clamp(3.0, 8.0);
    let inner = r.shrink(pad);
    let mid = inner.center().x;
    let c = c.clamp(-1.0, 1.0);
    let label_size = fit_mono(
        Rect::from_min_size(inner.min, Vec2::new(inner.width() * 0.12, inner.height())),
        "+1",
        9.0,
    );
    lcd_text(
        painter,
        egui::pos2(inner.left(), inner.center().y),
        Align2::LEFT_CENTER,
        "-1",
        label_size,
        LcdTier::Label,
        st,
    );
    lcd_text(
        painter,
        egui::pos2(inner.right(), inner.center().y),
        Align2::RIGHT_CENTER,
        "+1",
        label_size,
        LcdTier::Label,
        st,
    );
    let track = Rect::from_min_max(
        egui::pos2(inner.left() + inner.width() * 0.12, inner.top()),
        egui::pos2(inner.right() - inner.width() * 0.12, inner.bottom()),
    );
    let mid = track.center().x;
    // The centre mark: always there, so zero has a place to be.
    painter.line_segment(
        [egui::pos2(mid, track.top()), egui::pos2(mid, track.bottom())],
        Stroke::new(1.0, lcd_ink(st, LcdTier::Label)),
    );
    let x = mid + track.width() * 0.5 * c;
    let bar_h = (track.height() * 0.4).max(3.0);
    let (a, b) = if c >= 0.0 { (mid, x) } else { (x, mid) };
    let bar = Rect::from_min_max(
        egui::pos2(a, track.center().y - bar_h * 0.5),
        egui::pos2(b.max(a + 1.0), track.center().y + bar_h * 0.5),
    );
    painter.rect_filled(
        bar,
        1.0,
        if c < 0.0 {
            CORR_RED
        } else {
            lcd_ink(st, LcdTier::Primary)
        },
    );
}

// ── readings ────────────────────────────────────────────────────────────────

/// Hertz, with the band's human rounding: whole below a kilohertz, one
/// decimal above it.
pub fn fmt_hz(hz: f32) -> String {
    if hz < 1_000.0 {
        format!("{hz:.0} Hz")
    } else {
        format!("{:.1} kHz", hz / 1_000.0)
    }
}

pub fn fmt_db(db: f32) -> String {
    format!("{db:+.1} dB")
}

pub fn fmt_pct(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}

pub fn fmt_ms(ms: f32) -> String {
    if ms < 10.0 {
        format!("{ms:.1} ms")
    } else if ms < 1_000.0 {
        format!("{ms:.0} ms")
    } else {
        format!("{:.2} s", ms / 1_000.0)
    }
}

// ── the curve display ───────────────────────────────────────────────────────

/// The frequency band every curve widget speaks: the audible decades, log.
pub const CURVE_LO_HZ: f32 = 20.0;
pub const CURVE_HI_HZ: f32 = 20_000.0;
/// The gain window, symmetric.
pub const CURVE_DB: f32 = 24.0;
/// How many points the response is sampled at, log-spaced edge to edge.
/// The HOST computes the curve with the audio path's own math and hands it
/// over as data — the display must never re-derive the filter, or the
/// picture and the sound can disagree.
pub const CURVE_POINTS: usize = 256;

/// x within `r` for a frequency, log-mapped.
pub fn hz_to_x(r: Rect, hz: f32) -> f32 {
    let t = (hz.max(1.0) / CURVE_LO_HZ).ln() / (CURVE_HI_HZ / CURVE_LO_HZ).ln();
    r.left() + r.width() * t.clamp(0.0, 1.0)
}

/// The frequency at an x within `r`.
pub fn x_to_hz(r: Rect, x: f32) -> f32 {
    let t = ((x - r.left()) / r.width().max(1.0)).clamp(0.0, 1.0);
    CURVE_LO_HZ * (CURVE_HI_HZ / CURVE_LO_HZ).powf(t)
}

/// y within `r` for a gain, +CURVE_DB at the top.
pub fn db_to_y(r: Rect, db: f32) -> f32 {
    let t = (db.clamp(-CURVE_DB, CURVE_DB) + CURVE_DB) / (2.0 * CURVE_DB);
    r.bottom() - r.height() * t
}

/// The gain at a y within `r`.
pub fn y_to_db(r: Rect, y: f32) -> f32 {
    let t = ((r.bottom() - y) / r.height().max(1.0)).clamp(0.0, 1.0);
    t * 2.0 * CURVE_DB - CURVE_DB
}

/// One node as the curve draws it.
pub struct CurveNode {
    pub freq_hz: f32,
    pub gain_db: f32,
    /// A one-character badge for the filter kind, printed over the handle.
    pub glyph: &'static str,
}

/// What a gesture on the curve asked for. The widget names the wish; the
/// face maps it onto parameters — the kit knows nothing about ids.
pub enum CurveAction {
    /// A node is being dragged: here is where it wants to be.
    Move { node: usize, freq_hz: f32, gain_db: f32 },
    /// The wheel turned over a node: sharpen or widen it.
    Turn { node: usize, steps: f32 },
    /// A tap on a node, no travel: cycle its kind — the ask the whole
    /// control was specified around.
    Cycle { node: usize },
    /// A double-click on a node: take it out.
    Remove { node: usize },
    /// A double-click on empty curve: put a node here.
    Add { freq_hz: f32, gain_db: f32 },
}

/// How close a pointer must be to own a node, in points.
const NODE_GRAB: f32 = 12.0;

/// The frequency grid, the response, the analyzer behind it, the handles.
///
/// `analyzer_db` (optional) is drawn FIRST, faint, filled to the floor — the
/// spectrum behind the curve. `curve_db` is the response on the same
/// `CURVE_POINTS` log grid. `drag` is the caller's latch: which node the
/// press took, cleared on release — held by the caller because the widget is
/// immediate-mode and owns nothing between frames.
#[allow(clippy::too_many_arguments)]
pub fn eq_curve(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    r: Rect,
    nodes: &[CurveNode],
    curve_db: &[f32],
    analyzer_db: Option<&[f32]>,
    drag: &mut Option<usize>,
) -> Option<CurveAction> {
    let painter = ui.painter_at(r);
    lcd_glass(&painter, r, st);
    let inner = r.shrink2(Vec2::new(6.0, 4.0));

    // The grid: decade lines and the dB rules, all in the label tier — the
    // frequencies drawn behind, in the cluster's own dress, per the brief.
    let label_size = fit_mono(
        Rect::from_min_size(inner.min, Vec2::new(34.0, 12.0)),
        "20k",
        9.0,
    );
    for hz in [
        20.0_f32, 50.0, 100.0, 200.0, 500.0, 1_000.0, 2_000.0, 5_000.0, 10_000.0, 20_000.0,
    ] {
        let x = hz_to_x(inner, hz);
        painter.line_segment(
            [egui::pos2(x, inner.top()), egui::pos2(x, inner.bottom() - 11.0)],
            Stroke::new(1.0, lcd_ink(st, LcdTier::Label).gamma_multiply(0.55)),
        );
        let short = if hz >= 1_000.0 {
            format!("{:.0}k", hz / 1_000.0)
        } else {
            format!("{hz:.0}")
        };
        lcd_text(
            &painter,
            egui::pos2(x, inner.bottom()),
            Align2::CENTER_BOTTOM,
            &short,
            label_size,
            LcdTier::Label,
            st,
        );
    }
    for db in [-18.0_f32, -12.0, -6.0, 0.0, 6.0, 12.0, 18.0] {
        let y = db_to_y(inner, db);
        let ink = if db == 0.0 {
            lcd_ink(st, LcdTier::Secondary)
        } else {
            lcd_ink(st, LcdTier::Label).gamma_multiply(0.55)
        };
        painter.line_segment(
            [egui::pos2(inner.left() + 20.0, y), egui::pos2(inner.right(), y)],
            Stroke::new(1.0, ink),
        );
        lcd_text(
            &painter,
            egui::pos2(inner.left(), y),
            Align2::LEFT_CENTER,
            &format!("{db:+.0}"),
            label_size,
            LcdTier::Label,
            st,
        );
    }

    // The analyzer, behind everything that moves: a filled ridge in the
    // scope's own understatement, so the response always reads on top.
    if let Some(spec) = analyzer_db {
        let floor = inner.bottom() - 11.0;
        let fill = lcd_ink(st, LcdTier::Primary).gamma_multiply(0.16);
        let n = spec.len().max(2);
        for (i, &db) in spec.iter().enumerate() {
            let x0 = inner.left() + inner.width() * i as f32 / n as f32;
            let x1 = inner.left() + inner.width() * (i + 1) as f32 / n as f32;
            // The analyzer's own window: -90 dB at the floor, 0 at the top.
            let t = ((db + 90.0) / 90.0).clamp(0.0, 1.0);
            let y = floor - (floor - inner.top()) * t;
            painter.rect_filled(
                Rect::from_min_max(egui::pos2(x0, y), egui::pos2(x1, floor)),
                0.0,
                fill,
            );
        }
    }

    // The response: glow under, core over — the scope's trace treatment.
    if curve_db.len() >= 2 {
        let n = curve_db.len();
        let pt = |i: usize| {
            egui::pos2(
                inner.left() + inner.width() * i as f32 / (n - 1) as f32,
                db_to_y(inner, curve_db[i]),
            )
        };
        for i in 1..n {
            painter.line_segment(
                [pt(i - 1), pt(i)],
                Stroke::new(3.0, lcd_ink(st, LcdTier::Primary).gamma_multiply(0.22)),
            );
        }
        for i in 1..n {
            painter.line_segment(
                [pt(i - 1), pt(i)],
                Stroke::new(1.4, lcd_ink(st, LcdTier::Primary)),
            );
        }
    }

    // The handles, over everything.
    let pointer = ui.ctx().pointer_latest_pos();
    let hovered_node = pointer.filter(|p| r.contains(*p)).and_then(|p| {
        nodes
            .iter()
            .enumerate()
            .map(|(i, nd)| {
                let at = egui::pos2(hz_to_x(inner, nd.freq_hz), db_to_y(inner, nd.gain_db));
                (i, at.distance(p))
            })
            .filter(|(_, d)| *d <= NODE_GRAB)
            .min_by(|a, b| a.1.total_cmp(&b.1))
            .map(|(i, _)| i)
    });
    for (i, nd) in nodes.iter().enumerate() {
        let at = egui::pos2(hz_to_x(inner, nd.freq_hz), db_to_y(inner, nd.gain_db));
        let hot = hovered_node == Some(i) || *drag == Some(i);
        painter.circle_filled(at, if hot { 6.0 } else { 5.0 }, st.lcd_face);
        painter.circle_stroke(
            at,
            if hot { 6.0 } else { 5.0 },
            Stroke::new(1.5, lcd_ink(st, LcdTier::Primary)),
        );
        lcd_text(
            &painter,
            at + Vec2::new(0.0, -8.0),
            Align2::CENTER_BOTTOM,
            nd.glyph,
            9.0,
            if hot { LcdTier::Primary } else { LcdTier::Secondary },
            st,
        );
    }

    // The gestures. One interact region for the whole glass; the latch in
    // `drag` keeps a moving hand on the node it took, however far it leaves.
    let resp = ui.interact(r, ui.id().with("eq-curve"), egui::Sense::click_and_drag());
    let mut action = None;
    if resp.drag_started() {
        *drag = hovered_node;
    }
    if resp.dragged() {
        if let (Some(node), Some(p)) = (*drag, pointer) {
            action = Some(CurveAction::Move {
                node,
                freq_hz: x_to_hz(inner, p.x),
                gain_db: y_to_db(inner, p.y),
            });
        }
    }
    if resp.drag_stopped() {
        *drag = None;
    }
    if action.is_none() {
        if resp.double_clicked() {
            action = match hovered_node {
                Some(node) => Some(CurveAction::Remove { node }),
                None => pointer.map(|p| CurveAction::Add {
                    freq_hz: x_to_hz(inner, p.x),
                    gain_db: y_to_db(inner, p.y),
                }),
            };
        } else if resp.clicked() {
            if let Some(node) = hovered_node {
                action = Some(CurveAction::Cycle { node });
            }
        }
    }
    if action.is_none() {
        if let Some(node) = hovered_node {
            let wheel = ui.input(|i| i.raw_scroll_delta.y);
            if wheel.abs() > 0.0 {
                action = Some(CurveAction::Turn { node, steps: wheel / 40.0 });
            }
        }
    }
    action
}

// ── the vectorscope ─────────────────────────────────────────────────────────

/// The full-circle goniometer: L/R rotated forty-five degrees, so a mono
/// signal stands up straight and pure side lies flat. `frames` is recent
/// interleaved-decoded stereo, oldest first; `gain` is the caller's
/// normalisation (the tap ring keeps a peak for exactly this).
pub fn vectorscope(painter: &Painter, r: Rect, st: &FaceStyle, frames: &[(f32, f32)], gain: f32) {
    lcd_glass(painter, r, st);
    let radius = r.width().min(r.height()) * 0.5 - 6.0;
    let c = r.center();
    // The circle, and the axes: M up, S across, the diagonals are the
    // channels themselves.
    painter.circle_stroke(c, radius, Stroke::new(1.0, lcd_ink(st, LcdTier::Label)));
    let axis = lcd_ink(st, LcdTier::Label).gamma_multiply(0.6);
    painter.line_segment(
        [c - Vec2::new(0.0, radius), c + Vec2::new(0.0, radius)],
        Stroke::new(1.0, axis),
    );
    painter.line_segment(
        [c - Vec2::new(radius, 0.0), c + Vec2::new(radius, 0.0)],
        Stroke::new(1.0, axis),
    );
    let d = radius * std::f32::consts::FRAC_1_SQRT_2;
    lcd_text(painter, c + Vec2::new(-d, -d), Align2::RIGHT_BOTTOM, "L", 9.0, LcdTier::Label, st);
    lcd_text(painter, c + Vec2::new(d, -d), Align2::LEFT_BOTTOM, "R", 9.0, LcdTier::Label, st);

    if frames.len() < 2 {
        return;
    }
    // The trace: older is fainter, drawn first; the newest fifth carries the
    // glow. Points are the rotation x=(l-r), y=(l+r), clamped INTO the
    // circle rather than clipped out of it — a hot signal bends against the
    // rim, it does not vanish.
    let scale = radius * std::f32::consts::FRAC_1_SQRT_2 * gain;
    let plot = |l: f32, r_: f32| {
        let mut x = (l - r_) * scale;
        let mut y = (l + r_) * scale;
        let len = (x * x + y * y).sqrt();
        if len > radius {
            let k = radius / len;
            x *= k;
            y *= k;
        }
        egui::pos2(c.x + x, c.y - y)
    };
    let n = frames.len();
    let bright_from = n.saturating_sub(n / 5);
    let dim = lcd_ink(st, LcdTier::Primary).gamma_multiply(0.10);
    let mid = lcd_ink(st, LcdTier::Primary).gamma_multiply(0.28);
    let hot = lcd_ink(st, LcdTier::Primary);
    let mut prev = plot(frames[0].0, frames[0].1);
    for (i, &(l, r_)) in frames.iter().enumerate().skip(1) {
        let at = plot(l, r_);
        let (w, ink) = if i >= bright_from {
            (1.3, hot)
        } else if i >= n / 2 {
            (1.0, mid)
        } else {
            (1.0, dim)
        };
        painter.line_segment([prev, at], Stroke::new(w, ink));
        prev = at;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The suite is dressed by every theme or the build fails.** The same
    /// sweep the loudness meter runs: all sixteen factory themes, both
    /// modes, every layer of the kit asserted against the surface it is
    /// actually drawn on. A face built from the kit inherits this proof.
    #[test]
    fn every_theme_dresses_a_face() {
        let mut bad: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mode = if dark { "dark" } else { "light" };
                let mut check = |what: &str, fg: Color32, bg: Color32, floor: f32| {
                    let r = contrast_ratio(fg, bg);
                    if r < floor {
                        bad.push(format!("{} {mode}: {what} {r:.2}:1", theme.name));
                    }
                };
                // The panel's own text.
                check("row names", st.s.ink, st.s.bg, 4.5);
                check("readings at rest", st.s.faint, st.s.bg, 4.5);
                // A button must be findable, then its label readable.
                check("the raised ground", st.s.raised, st.s.bg, 1.1);
                check("ink on raised", st.s.ink, st.s.raised, 4.5);
                // The glass and every tier printed on it. Primary earns
                // reading contrast; the label tier only needs to be found.
                check("the glass on the panel", st.lcd_face, st.s.bg, 1.25);
                check("primary digits", lcd_ink(&st, LcdTier::Primary), st.lcd_face, 3.0);
                check(
                    "secondary digits",
                    lcd_ink(&st, LcdTier::Secondary),
                    st.lcd_face,
                    2.0,
                );
                check("grid labels", lcd_ink(&st, LcdTier::Label), st.lcd_face, 1.6);
                // The warning red must read on the glass on every theme —
                // fixed meaning, fixed colour, so this is the one place a
                // theme could break it silently.
                check("the correlation red", CORR_RED, st.lcd_face, 1.6);
            }
        }
        assert!(
            bad.is_empty(),
            "a face layer is unreadable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// The cluster's rule, held: a reading sized on the widest text never
    /// exceeds the room it was given, and never collapses below legibility.
    #[test]
    fn fitted_mono_stays_inside_its_box() {
        let r = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(80.0, 20.0));
        let size = fit_mono(r, "-24.0 dB", 14.0);
        assert!(size * 0.60 * 8.0 <= r.width() + 0.5, "overflows: {size}");
        assert!(size >= 5.0);
        let tiny = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(6.0, 4.0));
        assert!(fit_mono(tiny, "000|0|000", 12.0) >= 5.0, "collapsed below the floor");
    }

    /// The curve's coordinate mappings invert each other, edge to edge —
    /// the drag math and the drawing math are the same functions, so this
    /// is the whole correctness story of node placement.
    #[test]
    fn the_curve_mappings_round_trip() {
        let r = Rect::from_min_size(egui::pos2(10.0, 20.0), Vec2::new(300.0, 140.0));
        for hz in [20.0_f32, 55.0, 440.0, 1_000.0, 9_800.0, 20_000.0] {
            let back = x_to_hz(r, hz_to_x(r, hz));
            assert!(
                (back / hz - 1.0).abs() < 0.01,
                "{hz} Hz came back as {back}"
            );
        }
        for db in [-24.0_f32, -6.0, 0.0, 3.0, 24.0] {
            let back = y_to_db(r, db_to_y(r, db));
            assert!((back - db).abs() < 0.1, "{db} dB came back as {back}");
        }
        // The edges pin the window: left is 20 Hz, right is 20 kHz.
        assert!((x_to_hz(r, r.left()) - CURVE_LO_HZ).abs() < 0.5);
        assert!((x_to_hz(r, r.right()) - CURVE_HI_HZ).abs() < 50.0);
        assert!((y_to_db(r, r.top()) - CURVE_DB).abs() < 0.1);
        assert!((y_to_db(r, r.bottom()) + CURVE_DB).abs() < 0.1);
    }

    /// The readings speak the band's language.
    #[test]
    fn readings_round_like_the_band() {
        assert_eq!(fmt_hz(480.0), "480 Hz");
        assert_eq!(fmt_hz(1_200.0), "1.2 kHz");
        assert_eq!(fmt_db(3.42), "+3.4 dB");
        assert_eq!(fmt_db(-12.0), "-12.0 dB");
        assert_eq!(fmt_pct(0.5), "50%");
        assert_eq!(fmt_ms(2.5), "2.5 ms");
        assert_eq!(fmt_ms(120.0), "120 ms");
        assert_eq!(fmt_ms(1_200.0), "1.20 s");
    }
}
