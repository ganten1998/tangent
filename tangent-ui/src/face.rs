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
//!
//! **Two laws the first shipping build broke, written down here so the next
//! face cannot break them again.** A face draws EVERY row it has, every
//! frame, and folds their asks with [`keep`] — never `or_else`, which stops
//! calling the rows once one of them has spoken and leaves the panel below
//! the hand blinking in and out at pointer rate. And an adjustable is only
//! double-clickable if its widget SENSES clicks: `egui::Slider` does not, so
//! the reset gesture the hover text promises is arranged here, in one place,
//! rather than re-derived by each panel that happens to want it.

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

/// Keep the FIRST wish of the frame — having DRAWN every row that could have
/// made one.
///
/// **Why this exists, and why `or_else` must never be used for it.** A face
/// returns at most one ask per frame, because a hand only does one thing at a
/// time and a queue of asks is a queue of bugs. The obvious spelling of that
/// rule is `out = out.or_else(|| row(ui, ..))` — and it is a trap, because
/// `or_else` does not CALL its closure once something is held. The moment one
/// row acts, every row under it stops being drawn at all.
///
/// That is the whole of the reported "slider glitch". A dragged slider
/// reports a change on the frames the pointer travels and none on the frames
/// it rests, so the panel below the hand alternated between whole and
/// one-row-tall at pointer rate — measured at 129pt and 18pt by
/// `a_dragged_row_never_moves_the_rows_under_it`. It took the double-click
/// with it: egui matches a second press to a first by widget id, and a widget
/// that is absent every other frame never sees two.
///
/// So the argument is evaluated EAGERLY, which is the entire point: the row
/// draws whether or not its wish is the one that gets kept.
pub fn keep<T>(slot: &mut Option<T>, wish: Option<T>) {
    if slot.is_none() {
        *slot = wish;
    }
}

/// Remember which control the hand has just clicked, and hand back the one
/// it clicked before this. Kept in egui's own per-context scratch — never a
/// static, because the plugin build runs several contexts in one process,
/// and never `Settings`, because a widget that reads global state is a widget
/// that cannot be tested.
///
/// EVERY click a face row takes has to pass through here, whether or not the
/// row itself cares about the answer, because the answer is "what did the
/// hand touch last" and a click nobody reported would leave the NEXT row
/// believing the pair before it were its own.
fn note_click(resp: &egui::Response) -> Option<egui::Id> {
    let key = egui::Id::new("face-kit-last-clicked");
    let previous: Option<egui::Id> = resp.ctx.data(|d| d.get_temp(key));
    resp.ctx.data_mut(|d| d.insert_temp(key, resp.id));
    previous
}

/// Did the hand ask THIS control to go home? For a control the hand DRAGS —
/// a slider, a knob, a fader — where clicking twice can mean nothing else.
///
/// **Why it is not simply `double_clicked`.** egui counts double-clicks on
/// the CLOCK, not per widget: the second of two clicks a fifth of a second
/// apart reads as a double on whichever widget took it, even when the first
/// landed on something else entirely. Almost everywhere that is harmless — a
/// list row opens the thing it was going to open anyway. On a face it would
/// throw away a value the hand had just placed on the NEIGHBOURING row, which
/// is the one mistake a reset gesture must never make. So the pair has to
/// have landed on the same control, which is what [`note_click`] is for.
///
/// A triple counts as well: a double refused for following a click elsewhere
/// should land on the retry rather than making somebody pause first.
///
/// **And why only a control the hand drags may wear it.** egui's counter
/// LATCHES. Read it in `InputState`: a release is a double when it lands
/// within `max_double_click_delay` (0.3 s) of the last one, and a TRIPLE
/// when it lands within twice that of the one BEFORE the last — and in a
/// sustained stream both windows keep sliding forward, so from the third
/// release onward every single one is a triple, for as long as the hand
/// keeps going. Above about 3.3 clicks a second this function is therefore
/// true on every click. That is harmless on a slider, which nobody clicks
/// repeatedly, and fatal on anything whose ordinary use IS repeated
/// clicking — those take [`alt_home_gesture`] instead.
///
/// Alt-click is accepted here as well, so that the answer to "how do I put
/// this back" is one gesture for the whole panel and not one per kind of
/// control.
///
/// Crate-visible on purpose: the keys panel and the routing window each hold
/// a copy of the same broken line (`resp.double_clicked()` straight off an
/// `egui::Slider`), and when their lanes come to fix it the answer should be
/// this function and not a third telling of the story. Hand it the response
/// that senses clicks — `resp.interact(Sense::click())` — not the raw one.
pub(crate) fn home_gesture(resp: &egui::Response) -> bool {
    if !resp.clicked() {
        return false;
    }
    let same = note_click(resp) == Some(resp.id);
    resp.ctx.input(|i| i.modifiers.alt)
        || (same && (resp.double_clicked() || resp.triple_clicked()))
}

/// The same ask, for a control whose PRIMARY gesture is the click itself.
///
/// A stepped row is WALKED by clicking it, so there is no second click to
/// spare for a second meaning: see [`home_gesture`] for the arithmetic that
/// turns a brisk walk into an endless triple. A modifier cannot collide with
/// a walk, because the hand has to reach for it deliberately, and alt is the
/// modifier this app already spends on "undo what is here": the key legend
/// teaches `A-CLK` as emptying an insert bay and removing an automation
/// node, so a hand that knows the rack already knows this one.
///
/// Note it does not ask whether the pair landed on the same control, because
/// there is no pair: one click carries the whole gesture. It still reports
/// the click, so that a slider clicked after it cannot mistake the hand's
/// detour for a double of its own.
pub(crate) fn alt_home_gesture(resp: &egui::Response) -> bool {
    if !resp.clicked() {
        return false;
    }
    note_click(resp);
    resp.ctx.input(|i| i.modifiers.alt)
}

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
        // **A slider has to be TOLD to sense clicks before it can be
        // double-clicked.** `egui::Slider` allocates itself with
        // `Sense::drag()` alone, and `Response::double_clicked` is gated on
        // the CLICKED flag, which egui only ever sets for a widget whose
        // sense says it wants clicks. So the `resp.double_clicked()` this
        // row used to ask — and the identical line the keys panel asks —
        // could not be true on any frame, on any theme, on any unit: the
        // hover text has been promising a gesture the widget was deaf to.
        // Re-registering the SAME id with the click sense added is egui's
        // own remedy for exactly this ("use `interact` to sense more things
        // after adding a widget"), and it costs no extra id, so the rows
        // below keep the ids they had.
        //
        // The two responses are not interchangeable: `changed` is a flag the
        // slider stamps on the response object it returned, and the
        // re-interacted one is a fresh object that never saw it. Read each
        // from the one that knows.
        let hand = resp.interact(egui::Sense::click());
        if home_gesture(&hand) {
            out = Some(default);
        } else if resp.changed() {
            out = Some(v);
        }
        hand.on_hover_text("drag to set; double-click or alt-click to reset");
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

/// What a gesture on a stepped row asked for. The row names the WISH and the
/// face maps it onto the parameter — [`CurveAction`]'s bargain, kept for the
/// same reason: the kit knows nothing about ids, and it must know nothing
/// about defaults either. A knob's home is a number the DSP owns and the host
/// pushes, so only the face can spell it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StepAsk {
    /// Walk the choices by this many, wrapping. +1 forward, -1 back.
    By(i32),
    /// Put it back where the DSP says it rests.
    Home,
}

/// One stepped parameter as a row: name, a cycling button wearing the current
/// choice, a step either way from its ends.
///
/// A click on the body cycles forward — the effect panels' own idiom for a
/// mode: fewer targets, and the list is short enough to walk. A right-click
/// steps back, which the hover text has always promised and no code has ever
/// done until now. An ALT-click asks for home.
///
/// **Why alt here, and a double-click on the sliders.** This row's primary
/// gesture is repeated clicking, and egui's click counter is a clock that
/// latches: past about 3.3 clicks a second every release reads as a triple
/// ([`home_gesture`] holds the arithmetic). A row that took a double or a
/// triple for "go home" would therefore reset itself in the middle of being
/// walked — the reset and the walk would be the SAME gesture, which is no
/// gesture at all. A modifier is the one spelling a fast hand cannot produce
/// by accident. The sliders keep their double-click, since a slider has no
/// repeat-click idiom to collide with, and they answer to alt as well, so
/// "put it back" is still one thing to know for the whole panel.
pub fn stepped_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    choice: &str,
) -> Option<StepAsk> {
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
        let body = ui
            .add_sized(
                Vec2::new(w * (1.0 - LABEL_W) - 40.0, 18.0),
                egui::Button::new(
                    egui::RichText::new(choice).font(bold(10.0)).color(st.s.ink),
                )
                .fill(st.s.raised),
            )
            .on_hover_text(
                "click to cycle; right-click steps back; alt-click resets",
            );
        if alt_home_gesture(&body) {
            out = Some(StepAsk::Home);
        } else if body.secondary_clicked() {
            out = Some(StepAsk::By(-1));
        } else if body.clicked() {
            out = Some(StepAsk::By(1));
        }
        let back = ui.add_sized(
            Vec2::new(22.0, 18.0),
            egui::Button::new(
                egui::RichText::new("‹").font(bold(11.0)).color(st.s.faint),
            )
            .fill(st.s.raised),
        );
        if alt_home_gesture(&back) {
            out = Some(StepAsk::Home);
        } else if back.clicked() {
            out = Some(StepAsk::By(-1));
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
///
/// **On the two faults the rows had, and why this widget has neither.** Its
/// id is spelled out (`ui.id().with("eq-curve")`) instead of drawn from the
/// auto counter, so nothing above it in the panel can shift it — the handles
/// were never at risk of the strobe that took the sliders' double-click, and
/// the measurement is in `dragging_a_curve_node_never_moves_the_rows_under_it`.
/// It senses clicks AND drags from the start, so its double-click has always
/// worked. What it does NOT get is a reset: on the curve that gesture is
/// spoken for — a double-click on a node takes the band out and one on empty
/// glass adds a band, which is the control the brief was written around, and
/// no third meaning fits on one gesture. Putting a band back is what the five
/// rows under it are for.
///
/// One rough edge left standing: the first press of a double-click is
/// reported as an ordinary click before egui can know a second is coming, so
/// double-clicking a node cycles its shape once on the way to removing it.
/// The band ends up off either way and the stray shape is invisible under it,
/// which is a smaller price than a hand-tuned click delay.
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

    // ── the hand on a row ───────────────────────────────────────────────

    /// A face of `n` plain continuous rows: the shape `faces::draw` gives
    /// any unit without a hand-built panel, which is most of the suite.
    fn a_face_of(n: usize) -> crate::faces::FaceLive {
        crate::faces::FaceLive {
            face: crate::ports::BuiltinFace {
                path: "@tangent-tape".to_owned(),
                params: (0..n)
                    .map(|i| crate::ports::BuiltinFaceParam {
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
            ..Default::default()
        }
    }

    fn a_test_style() -> FaceStyle {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        FaceStyle::new(p.bg, p.lcd, p.lcd_face)
    }

    /// **The panel under the hand is the SAME panel every frame.**
    ///
    /// The reported fault: drag any slider on a suite face and everything
    /// from the next row down flashes. A face folds its rows with "keep the
    /// first ask", and the obvious spelling of that skips the rows under
    /// whichever one acted — so on the frames the hand moves they are not
    /// drawn at all, and on the frames it rests they are. That strobe is
    /// also why a double-click never landed: a widget that is absent every
    /// other frame cannot see two presses.
    ///
    /// So: press a row, then travel-rest-travel the way a real drag does,
    /// and demand that the ids the face consumes and the rect it fills come
    /// out identical on every one of those frames.
    #[test]
    fn a_dragged_row_never_moves_the_rows_under_it() {
        let ctx = a_test_ctx();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 420.0));
        let st = a_test_style();
        let mut live = a_face_of(5);
        // Mid-track of the FIRST row's slider: the label column ends at
        // 0.34 of the width and the reading column starts at 0.81.
        let mut at = egui::pos2(200.0, 9.0);
        // A gesture that never fired would make every frame agree and prove
        // nothing at all, so the asks are counted as well.
        let mut acted = 0_u32;

        let mut frame =
            |events: Vec<egui::Event>, live: &mut crate::faces::FaceLive| -> (u64, Rect) {
                let mut seen = (0_u64, Rect::NOTHING);
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(screen),
                        events,
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                // A scope so the face's own extent is a rect
                                // somebody can read, and so the auto-id
                                // counter can be sampled either side of it.
                                let mut consumed = 0_u64;
                                let drawn = ui.scope(|ui| {
                                    let before = ui.next_auto_id().value();
                                    let act = crate::faces::draw(ui, &st, live);
                                    consumed =
                                        ui.next_auto_id().value().wrapping_sub(before);
                                    act
                                });
                                seen = (consumed, drawn.response.rect);
                                // The app's optimistic mirror, which is what
                                // makes a RESTING hand report no change.
                                if let Some(crate::faces::FaceAction::Set { id, value }) =
                                    drawn.inner
                                {
                                    acted += 1;
                                    if let Some(p) =
                                        live.face.params.iter_mut().find(|p| p.id == id)
                                    {
                                        p.value = value;
                                    }
                                }
                            });
                    },
                );
                seen
            };

        let _ = frame(Vec::new(), &mut live);
        let _ = frame(
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            &mut live,
        );
        let mut seen: Vec<(u64, Rect)> = Vec::new();
        for step in 0..6 {
            let events = if step % 2 == 0 {
                at.x += 6.0;
                vec![egui::Event::PointerMoved(at)]
            } else {
                Vec::new()
            };
            seen.push(frame(events, &mut live));
        }
        assert!(
            acted >= 3,
            "the slider was never actually dragged, so this proves nothing \
             ({acted} asks over the gesture)"
        );
        let first = seen[0];
        assert!(
            seen.iter().all(|s| *s == first),
            "the face is not the same face frame to frame while a slider is \
             dragged - (auto ids consumed, content rect) per frame: {seen:#?}"
        );
    }

    /// The fold's whole contract: the first wish of the frame is the one
    /// that travels, and the arguments after it were already evaluated —
    /// which is to say already DRAWN.
    #[test]
    fn the_first_wish_of_the_frame_is_the_one_kept() {
        let mut slot = None;
        let mut drawn = 0;
        for wish in [None, Some("freq"), Some("gain")] {
            keep(&mut slot, {
                drawn += 1;
                wish
            });
        }
        assert_eq!(slot, Some("freq"), "the fold kept the wrong ask");
        assert_eq!(drawn, 3, "a row went undrawn because an earlier one acted");
    }

    // ── the home gesture, one control at a time ─────────────────────────

    fn press_at(p: egui::Pos2, down: bool) -> egui::Event {
        press_with(p, down, egui::Modifiers::NONE)
    }

    /// The same press with a modifier held. egui reads the modifier state
    /// off the frame's `RawInput` rather than off the event, so a test that
    /// wants an alt-click has to hold it in BOTH places — which is exactly
    /// what a real backend does.
    fn press_with(p: egui::Pos2, down: bool, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers,
        }
    }

    /// A ready `Context` with the panel fonts in it — the kit prints in
    /// Courier and monospace, and an uninstalled family is a different
    /// layout, so every gesture test dresses itself the same way.
    fn a_test_ctx() -> egui::Context {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        ctx
    }

    /// **Double-click a slider row and it goes home.** The gesture the hover
    /// text has advertised since the kit was written and which no unit has
    /// ever performed: `egui::Slider` senses drags only, and egui refuses
    /// the CLICKED flag — and therefore `double_clicked` — to a widget that
    /// never asked for clicks. Five frames: settle, press, release, press,
    /// release, which is a double-click as the input layer counts one.
    #[test]
    fn a_double_click_puts_a_slider_row_home() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 60.0));
        // The knob is parked well away from the number the DSP calls home,
        // so nothing but the reset could produce it.
        const HOME: f32 = 0.25;
        let at = egui::pos2(200.0, 9.0);

        let frame = |events: Vec<egui::Event>| -> Option<f32> {
            let mut out = None;
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
                            out = slider_row(ui, &st, "DRIVE", "80%", 0.80, HOME);
                        });
                },
            );
            out
        };

        let _ = frame(vec![egui::Event::PointerMoved(at)]);
        let _ = frame(vec![press_at(at, true)]);
        let _ = frame(vec![press_at(at, false)]);
        let _ = frame(vec![press_at(at, true)]);
        assert_eq!(
            frame(vec![press_at(at, false)]),
            Some(HOME),
            "a double-click on a slider row did not ask for the DSP's default"
        );
    }

    /// **And one alt-click does the same thing** — the gesture the stepped
    /// rows had to take instead of the double, tried here so that "put it
    /// back" is one thing to know for the whole panel and the hover text is
    /// telling the truth on both kinds of row. It has to beat the slider's
    /// own click, too: a click anywhere on the track jumps the knob there,
    /// so an alt-click that merely reset AFTER moving would be no reset.
    #[test]
    fn an_alt_click_puts_a_slider_row_home_as_well() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 60.0));
        const HOME: f32 = 0.25;
        // Well down the track from where the knob sits (the label column
        // takes the first 0.34 of the width and the knob is at 0.80), so the
        // click has somewhere wrong to put it.
        let at = egui::pos2(180.0, 9.0);
        let alt = egui::Modifiers::ALT;

        let frame = |events: Vec<egui::Event>,
                     modifiers: egui::Modifiers|
         -> Option<f32> {
            let mut out = None;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    modifiers,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            out = slider_row(ui, &st, "DRIVE", "80%", 0.80, HOME);
                        });
                },
            );
            out
        };

        let _ = frame(vec![egui::Event::PointerMoved(at)], egui::Modifiers::NONE);
        let _ = frame(vec![press_with(at, true, alt)], alt);
        assert_eq!(
            frame(vec![press_with(at, false, alt)], alt),
            Some(HOME),
            "an alt-click on a slider row did not ask for the DSP's default"
        );
    }

    /// **And the drag still owns the hand once it has left the track.**
    /// Teaching the slider to sense clicks changes how egui decides a press
    /// IS a drag — a widget that senses both waits until the pointer has
    /// moved too far to be a click, where a drag-only one is dragging at
    /// once. The latch that carries a drag off the widget is a different
    /// thing again, and the gesture people actually make (grab the knob,
    /// haul it past the end of the track, past the row) has to keep working
    /// or the cure would be worse than the fault.
    #[test]
    fn a_drag_that_leaves_the_track_still_moves_the_knob() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        let mut value = 0.50_f32;
        let frame = |events: Vec<egui::Event>, value: &mut f32| {
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
                            if let Some(v) =
                                slider_row(ui, &st, "DRIVE", "50%", *value, 0.25)
                            {
                                *value = v;
                            }
                        });
                },
            );
        };

        let at = egui::pos2(200.0, 9.0);
        frame(vec![egui::Event::PointerMoved(at)], &mut value);
        frame(vec![press_at(at, true)], &mut value);
        // Off the right end of the track and well below the row: nowhere
        // the widget's own rect reaches.
        for p in [
            egui::pos2(260.0, 20.0),
            egui::pos2(340.0, 60.0),
            egui::pos2(600.0, 150.0),
        ] {
            frame(vec![egui::Event::PointerMoved(p)], &mut value);
        }
        assert!(
            value > 0.99,
            "the drag was dropped when the hand left the track: {value}"
        );
    }

    /// **Alt-click a stepped row and it goes home** — and a plain second
    /// click keeps walking, because on this row the click is the walk. The
    /// row names the wish; only the face knows which position home is. Both
    /// halves of the row wear the gesture, since both are clicked.
    #[test]
    fn an_alt_click_asks_a_stepped_row_for_home() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 60.0));
        // Mid-body of the cycling button: the label column ends at 0.34 of
        // the width and the back-step is the last 22pt.
        let body = egui::pos2(250.0, 9.0);
        let back = egui::pos2(385.0, 9.0);

        let frame = |events: Vec<egui::Event>,
                     modifiers: egui::Modifiers|
         -> Option<StepAsk> {
            let mut out = None;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    modifiers,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            out = stepped_row(ui, &st, "SHAPE", "bell");
                        });
                },
            );
            out
        };
        let none = egui::Modifiers::NONE;
        let alt = egui::Modifiers::ALT;

        let _ = frame(vec![egui::Event::PointerMoved(body)], none);
        let _ = frame(vec![press_at(body, true)], none);
        assert_eq!(
            frame(vec![press_at(body, false)], none),
            Some(StepAsk::By(1)),
            "a single click no longer cycles the row forward"
        );
        // The very next click, well inside the double-click window: the row
        // must go on WALKING, which is the fault this gesture was moved for.
        let _ = frame(vec![press_at(body, true)], none);
        assert_eq!(
            frame(vec![press_at(body, false)], none),
            Some(StepAsk::By(1)),
            "a double-click on a stepped row stopped walking it"
        );
        let _ = frame(vec![press_with(body, true, alt)], alt);
        assert_eq!(
            frame(vec![press_with(body, false, alt)], alt),
            Some(StepAsk::Home),
            "an alt-click on a stepped row did not ask for home"
        );
        let _ = frame(vec![egui::Event::PointerMoved(back)], none);
        let _ = frame(vec![press_with(back, true, alt)], alt);
        assert_eq!(
            frame(vec![press_with(back, false, alt)], alt),
            Some(StepAsk::Home),
            "an alt-click on the back arrow did not ask for home"
        );
    }

    /// **Six clicks in a row still walk six places.** The regression this
    /// pins, and the reason a stepped row cannot wear a click-counted
    /// reset: egui's counter is a clock, and it latches. A release inside
    /// 0.3 s of the last is a double; a release inside 0.6 s of the one
    /// before THAT is a triple; in a sustained stream both stay true, so
    /// the second click is already a double and every click from the third
    /// is a triple, for as long as the hand keeps going. Five clicks a
    /// second is not fast — it is somebody looking for the fifth entry in a
    /// list of seven — and a row that read either as "go home" spent the
    /// whole stream resetting itself instead of walking.
    ///
    /// Three surfaces, because all three are walked by clicking: the
    /// cycling body, the back arrow, and the EQ's band walker, which is the
    /// same row with no engine number behind it. Measured against the code
    /// that wore the gesture: six clicks on the body landed on home (3)
    /// rather than 6, six on the back arrow the same, and the walker — which
    /// drops a `Home` on the floor, having no engine number to go back to —
    /// did not reset but simply STOPPED, frozen on band 2 from its first
    /// click for the whole stream.
    #[test]
    fn a_stepped_row_can_still_be_walked_at_speed() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(470.0, 470.0));
        // A row 470 wide: the label takes 0.34 of it, the body runs from
        // there to 40pt short of the end, and the back arrow is the last
        // 22pt of that — so 250 is the body and 457 the arrow.
        let body = egui::pos2(250.0, 9.0);
        let back = egui::pos2(457.0, 9.0);

        let frame = |t: f64,
                     events: Vec<egui::Event>,
                     modifiers: egui::Modifiers,
                     live: &mut crate::faces::FaceLive| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(t),
                    modifiers,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if let Some(crate::faces::FaceAction::Set { id, value }) =
                                crate::faces::draw(ui, &st, live)
                            {
                                if let Some(p) =
                                    live.face.params.iter_mut().find(|p| p.id == id)
                                {
                                    p.value = value;
                                }
                            }
                        });
                },
            );
        };

        // A fifth of a second between RELEASES, which is what egui dates a
        // click by; the press is the frame before it.
        let stream = |t0: f64,
                      at: egui::Pos2,
                      clicks: usize,
                      live: &mut crate::faces::FaceLive,
                      after: &mut dyn FnMut(&crate::faces::FaceLive)| {
            let none = egui::Modifiers::NONE;
            frame(t0, vec![egui::Event::PointerMoved(at)], none, live);
            for i in 0..clicks {
                let release = t0 + 0.2 * (i + 1) as f64;
                frame(release - 1.0 / 60.0, vec![press_at(at, true)], none, live);
                frame(release, vec![press_at(at, false)], none, live);
                after(live);
            }
        };

        // A seven-position row whose home is the MIDDLE choice, so a reset
        // can never be mistaken for a walk that happened to arrive there.
        let a_seven_way = |start: usize| {
            let mut live = a_face_of(1);
            let p = &mut live.face.params[0];
            p.choices = (0..7).map(|i| format!("s{i}")).collect();
            p.steps = 7;
            p.default = 0.5;
            p.value = start as f32 / 6.0;
            live
        };
        let at_index = |live: &crate::faces::FaceLive| {
            (live.face.params[0].value * 6.0).round() as usize
        };

        for (k, (what, at, start, want)) in [
            ("the cycling body", body, 0_usize, 6_usize),
            ("the back arrow", back, 6, 0),
        ]
        .into_iter()
        .enumerate()
        {
            let mut live = a_seven_way(start);
            // Whole seconds apart, so one block's last click cannot be read
            // as the first of the next.
            stream(10.0 * (k + 1) as f64, at, 6, &mut live, &mut |_| {});
            assert_eq!(
                at_index(&live),
                want,
                "six clicks on {what} did not walk six places (home is 3)"
            );
        }

        // The EQ's walker: three bands, so six clicks is two laps.
        let mut live = a_face_of(15);
        live.face.path = "@tangent-eq".to_owned();
        for band in 0..3 {
            let base = band * 5;
            live.face.params[base].value = 1.0;
            live.face.params[base + 1].choices =
                ["bell", "lo-shelf", "hi-shelf"].map(str::to_owned).to_vec();
            live.face.params[base + 1].steps = 3;
            for k in 1..5 {
                live.face.params[base + k].value = 0.5;
            }
        }
        // The curve takes the first 190pt and 6 more are spent under it, so
        // the walker's row (18pt on 3pt of spacing) has its middle at 208.
        let walker = egui::pos2(250.0, 208.0);
        let mut walked: Vec<usize> = Vec::new();
        stream(40.0, walker, 6, &mut live, &mut |live| {
            walked.push(live.band + 1);
        });
        assert_eq!(
            walked,
            vec![2, 3, 1, 2, 3, 1],
            "the EQ band walker stopped walking under a fast hand"
        );

        // And the walker still owes the tooltip an answer. It is the one
        // stepped row with no engine number behind it, so its home is the
        // first band rather than a pushed default — walk one step off it,
        // then ask.
        let alt = egui::Modifiers::ALT;
        let none = egui::Modifiers::NONE;
        frame(50.0, vec![press_at(walker, true)], none, &mut live);
        frame(50.1, vec![press_at(walker, false)], none, &mut live);
        assert_eq!(live.band, 1, "the walker did not step off band 1");
        frame(52.0, vec![press_with(walker, true, alt)], alt, &mut live);
        frame(52.1, vec![press_with(walker, false, alt)], alt, &mut live);
        assert_eq!(
            live.band, 0,
            "an alt-click on the band walker did not come home to band 1"
        );
    }

    /// **And it is the ENGINE's number that comes back, through the whole
    /// path.** The kit is handed a default by the face, the face reads it
    /// off the pushed parameter, and the parameter carries what the unit's
    /// own spec says — so this drives `faces::draw` rather than the row, and
    /// asserts on the ask the app would forward. Both control kinds, each
    /// with the gesture it actually wears — a double-click on the slider, an
    /// alt-click on the stepped row — since a stepped parameter's home is an
    /// index and a slider's is a number.
    #[test]
    fn the_reset_a_face_asks_for_is_the_pushed_default() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        let mut live = a_face_of(2);
        live.face.params[0].value = 0.80;
        live.face.params[0].default = 0.25;
        // The second row is a three-way whose home is the LAST position, so
        // neither "the first choice" nor "the one it is on" could pass.
        live.face.params[1].choices =
            vec!["off".to_owned(), "half".to_owned(), "full".to_owned()];
        live.face.params[1].steps = 3;
        live.face.params[1].value = 0.0;
        live.face.params[1].default = 1.0;

        // egui dates its clicks, so the test has to hold the clock rather
        // than let two unrelated gestures run into one double.
        let mut clock = 0.0_f64;
        let frame = |events: Vec<egui::Event>,
                         at: f64,
                         modifiers: egui::Modifiers,
                         live: &mut crate::faces::FaceLive|
         -> Option<(u32, f32)> {
            let mut out = None;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(at),
                    modifiers,
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if let Some(crate::faces::FaceAction::Set { id, value }) =
                                crate::faces::draw(ui, &st, live)
                            {
                                out = Some((id, value));
                            }
                        });
                },
            );
            out
        };

        // Rows are 18pt tall on 3pt of spacing, so the second one's middle
        // sits at 30.
        let none = egui::Modifiers::NONE;
        let alt = egui::Modifiers::ALT;
        for (what, at, held, want) in [
            ("the slider row", egui::pos2(200.0, 9.0), none, (0_u32, 0.25_f32)),
            ("the stepped row", egui::pos2(250.0, 30.0), alt, (1, 1.0)),
        ] {
            // A clean second between the two gestures: each is a gesture of
            // its own, not the tail of the last one.
            clock += 1.0;
            let mut tick = |events: Vec<egui::Event>,
                            modifiers: egui::Modifiers,
                            live: &mut crate::faces::FaceLive|
             -> Option<(u32, f32)> {
                clock += 1.0 / 60.0;
                frame(events, clock, modifiers, live)
            };
            let _ = tick(vec![egui::Event::PointerMoved(at)], none, &mut live);
            // The slider wants two clicks and the stepped row one, so the
            // first pair is a plain click either way: on the slider it is
            // the half of a double that does nothing, on the stepped row it
            // is one step forward that the alt-click then undoes.
            let _ = tick(vec![press_at(at, true)], none, &mut live);
            let _ = tick(vec![press_at(at, false)], none, &mut live);
            let _ = tick(vec![press_with(at, true, held)], held, &mut live);
            assert_eq!(
                tick(vec![press_with(at, false, held)], held, &mut live),
                Some(want),
                "{what} did not come home to the pushed default"
            );
        }
    }

    /// **A reset never lands on the row the hand did NOT double-click.**
    /// egui dates its double-clicks rather than owning them: two quick
    /// single clicks on two different rows come back as a double on the
    /// second. Everywhere else that is harmless; here it would silently
    /// throw away the value just placed on a neighbour. So: click one row,
    /// click the next one straight after, and the second must set what was
    /// clicked, never its default.
    #[test]
    fn a_click_on_one_row_cannot_reset_the_next_one() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        let mut live = a_face_of(2);
        for p in live.face.params.iter_mut() {
            p.value = 0.80;
            p.default = 0.25;
        }
        let mut clock = 0.0_f64;
        let mut frame = |events: Vec<egui::Event>,
                         live: &mut crate::faces::FaceLive|
         -> Option<(u32, f32)> {
            clock += 1.0 / 60.0;
            let mut out = None;
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    time: Some(clock),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            if let Some(crate::faces::FaceAction::Set { id, value }) =
                                crate::faces::draw(ui, &st, live)
                            {
                                out = Some((id, value));
                            }
                        });
                },
            );
            out
        };

        let first = egui::pos2(200.0, 9.0);
        let second = egui::pos2(200.0, 30.0);
        let _ = frame(vec![egui::Event::PointerMoved(first)], &mut live);
        let _ = frame(vec![press_at(first, true)], &mut live);
        let _ = frame(vec![press_at(first, false)], &mut live);
        // The very next frames, well inside the double-click window.
        let _ = frame(vec![egui::Event::PointerMoved(second)], &mut live);
        let _ = frame(vec![press_at(second, true)], &mut live);
        let landed = frame(vec![press_at(second, false)], &mut live);
        assert!(
            !matches!(landed, Some((1, v)) if (v - 0.25).abs() < 1.0e-6),
            "clicking one row reset the NEXT one: {landed:?}"
        );
    }

    /// **The curve is the other draggable, and it drags the same rows.**
    /// The EQ's nodes take their id from [`eq_curve`]'s own explicit
    /// `ui.id().with("eq-curve")` rather than the auto counter, so the
    /// widget itself was never at risk of the strobe. The five rows UNDER
    /// it were, and worse than the sliders': a held node reports a move on
    /// every frame, not only the ones the hand travels, so the band's whole
    /// control surface did not flicker — it was simply GONE for as long as
    /// a node was held. That is why the measure here is the panel at REST,
    /// carried through the drag: a gesture on the picture must not take the
    /// rows away.
    #[test]
    fn dragging_a_curve_node_never_moves_the_rows_under_it() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(470.0, 470.0));
        // Three bands of five: on, kind, freq, gain, q — the face's own
        // band-major id plan.
        let mut live = a_face_of(15);
        live.face.path = "@tangent-eq".to_owned();
        for band in 0..3 {
            let base = band * 5;
            live.face.params[base].value = 1.0;
            live.face.params[base + 1].choices =
                ["bell", "lo-shelf", "hi-shelf"].map(str::to_owned).to_vec();
            live.face.params[base + 1].steps = 3;
            for k in 1..5 {
                live.face.params[base + k].value = 0.5;
            }
        }
        // The first node: the middle of the log sweep at 0 dB, which the
        // curve's own mapping puts at the middle of its inner rect.
        // The curve fills the panel width, so 0.5 of the log sweep lands at
        // the middle of its inner rect: (470 - 12) / 2 + 6.
        let mut at = egui::pos2(235.0, 95.0);

        // A gesture that never fired would make every frame agree and prove
        // nothing at all, so the asks are counted as well.
        let mut acted = 0_u32;
        let mut frame =
            |events: Vec<egui::Event>, live: &mut crate::faces::FaceLive| -> (u64, Rect) {
                let mut seen = (0_u64, Rect::NOTHING);
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
                                let mut consumed = 0_u64;
                                let drawn = ui.scope(|ui| {
                                    let before = ui.next_auto_id().value();
                                    let act = crate::faces::draw(ui, &st, live);
                                    consumed =
                                        ui.next_auto_id().value().wrapping_sub(before);
                                    act
                                });
                                seen = (consumed, drawn.response.rect);
                                if let Some(crate::faces::FaceAction::Set { id, value }) =
                                    drawn.inner
                                {
                                    acted += 1;
                                    if let Some(p) =
                                        live.face.params.iter_mut().find(|p| p.id == id)
                                    {
                                        p.value = value;
                                    }
                                }
                            });
                    },
                );
                seen
            };

        let _ = frame(Vec::new(), &mut live);
        // The panel with nobody touching it: the shape every later frame
        // has to keep.
        let at_rest = frame(Vec::new(), &mut live);
        // The press, then a decisive first step: egui postpones the drag on
        // a widget that senses clicks AND drags until the hand has moved
        // too far to be a click, and the grab is only taken while the
        // pointer is still within reach of the node.
        let _ = frame(
            vec![egui::Event::PointerMoved(at), press_at(at, true)],
            &mut live,
        );
        let mut seen: Vec<(u64, Rect)> = vec![at_rest];
        for step in 0..8 {
            let events = if step % 2 == 0 {
                at.y -= if step == 0 { 9.0 } else { 3.0 };
                vec![egui::Event::PointerMoved(at)]
            } else {
                Vec::new()
            };
            seen.push(frame(events, &mut live));
        }
        assert!(
            acted >= 2,
            "the node was never actually dragged, so this proves nothing \
             ({acted} asks over the gesture)"
        );
        assert!(
            seen.iter().all(|s| *s == at_rest),
            "the EQ face is not the face it was at rest while a node is \
             dragged - (auto ids consumed, content rect) per frame: {seen:#?}"
        );
    }
}
