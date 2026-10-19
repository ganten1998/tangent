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
//! resizes. A sweep test at the bottom dresses the kit in every factory
//! theme, both modes, and fails the build on the first layer that
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
/// reaches into `Settings` itself, because a widget that reads global state
/// is a widget that cannot be tested by handing it colours.
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

/// **The app's own mark, drawn.** `assets/icon-small-square.svg` is a
/// triad — an equilateral triangle stroked with a filled circle at each
/// vertex — and on its 512 viewBox the geometry is vertices `(256,150)`,
/// `(136,356)`, `(376,356)`, node radius 82, stroke width 48. Those nine
/// numbers, scaled to `r`, so it stays the icon rather than a thing that
/// resembles it. It takes the ink it is handed and no colour of its own:
/// the icon file's plate and ink are a dark icon's, and stamping them on
/// a themed panel is the "ink inherited from somewhere else" fault this
/// repo has shipped seven times. Here beside the rest of the face
/// vocabulary so the next panel that wants a badge finds one.
pub fn triad_badge(painter: &Painter, r: Rect, ink: Color32) {
    if !r.is_positive() {
        return;
    }
    let scale = r.width().min(r.height()) / 512.0;
    let at = |x: f32, y: f32| r.center() + Vec2::new(x - 256.0, y - 256.0) * scale;
    let nodes = [at(256.0, 150.0), at(136.0, 356.0), at(376.0, 356.0)];
    let stroke = Stroke::new(48.0 * scale, ink);
    painter.line_segment([nodes[0], nodes[1]], stroke);
    painter.line_segment([nodes[1], nodes[2]], stroke);
    painter.line_segment([nodes[2], nodes[0]], stroke);
    for n in nodes {
        painter.circle_filled(n, 82.0 * scale, ink);
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
    painter.rect_stroke(r, 2.0, Stroke::new(1.0_f32, st.s.edge), StrokeKind::Inside);
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
    painter.text(at, align, text, FontId::new(size, fonts::courier()), lcd_ink(st, tier));
}

/// The floor a quiet word has to clear on whatever it is printed on. A
/// caption is not the story, but a caption nobody can read is not a caption.
const QUIET_FLOOR: f32 = 3.0;

/// **A quiet word, held readable on the ground it is actually printed on.**
///
/// `Surface::faint` is a ratio of the panel's ink to the panel's GROUND, so
/// it clears its floor there by construction and nowhere else. A raised
/// button is a step back toward the ink — that step is what makes it read as
/// raised — so every point of relief it gains costs the quiet ink contrast.
/// On Mono a `faint` word on `raised` measures 2.95:1, which is under the
/// floor, and Aurora and Moss do the same thing.
///
/// The dead-zone law says the SURFACE moves when the ink cannot clear it, and
/// where the surface is free to move that is still the answer: the IR
/// browser's dead header chips drop their raised ground rather than dimming
/// their word, and look flatter for it, which is what they are. A LIVE
/// control cannot take that road — a button that gave up its ground to stay
/// readable has stopped saying it is a button — so here the INK moves
/// instead, walking back toward the panel's own ink until it clears.
/// Hierarchy is sacrificed before legibility, in that order, which is the
/// same bargain [`FaceStyle::new`] already settles for the glass tiers.
///
/// On every theme where `faint` already clears, this returns it unchanged and
/// not one pixel moves.
pub fn quiet_ink(st: &FaceStyle, on: Color32) -> Color32 {
    let mut c = st.s.faint;
    let mut t = 0.0_f32;
    while contrast_ratio(c, on) < QUIET_FLOOR && t < 1.0 {
        t += 0.05;
        c = lerp(st.s.faint, st.s.ink, t);
    }
    c
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

/// How tall a kit row is drawn.
///
/// Named rather than spelled three times, because it is not only the number
/// [`slider_row`] and [`stepped_row`] hand to `add_sized` — it is what a
/// panel has to know to work out whether its rows will fit the window it
/// was given. `faces_amp::amp_floor_cost` is the first caller that needs it
/// from outside, and asking a panel to re-guess this figure is how a layout
/// comes to be built out of numbers that were true once.
///
/// A floor rather than a height: `add_sized` takes the LARGER of what it is
/// handed and what the widget's own galley needs, so a font whose eleven
/// point line runs taller than this makes the row taller with it. Anything
/// budgeting against this must take the same maximum.
pub const ROW_H: f32 = 18.0;

/// The narrowest the stepped row's back chevron is drawn — a floor under a
/// measurement, not a width. What it actually costs is its glyph plus the
/// theme's button padding, which is wider than this on every style the app
/// ships; see [`stepped_row`] for what happened when the row subtracted a
/// guess instead of asking.
const BACK_W: f32 = 22.0;

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
/// static, because one process runs several contexts (the compositor's
/// offscreen one beside the window's, and every test its own), and never
/// `Settings`, because a widget that reads global state is a widget that
/// cannot be tested.
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

/// What a slider row asked for: the knob's own value when the hand moved
/// it, or the words typed into its reading.
#[derive(Clone, Debug, PartialEq)]
pub enum SliderAsk {
    /// Put the knob here, normalised.
    Value(f32),
    /// **The reading was typed into and committed** - "-12", "440", "1.5k",
    /// "35%", "off". The row cannot spell what that is on the knob's own
    /// scale: every reading is printed on the HOST side by the unit's own
    /// mapping, so the face and the sound can never disagree, and the same
    /// rule means only the host can read one back. The text travels up.
    Type(String),
}

/// How much of a drag a slider takes while Shift is held: a quarter, the
/// ratio the Analog and sampler dials already turn at for their own fine
/// drag (`FINE_PER_PT / DRAG_PER_PT`), so "shift for fine" is one feel
/// across the suite.
pub const FINE_RATIO: f32 = 0.25;

/// Where a press-and-drag on a slider stands, kept in egui's per-widget
/// scratch for the length of the gesture. See [`fine_slider`].
#[derive(Clone, Copy)]
struct SliderDrag {
    /// The pointer's x on the previous frame of the gesture.
    last_x: f32,
    /// Shift has been held at some point in this gesture, so the knob has
    /// parted from the pointer and stays on the RELATIVE rule until the
    /// button is let go.
    parted: bool,
}

/// **The kit's slider: egui's own look, with Shift for a fine drag.**
/// Returns the response and, on any frame the hand moved it, the new value.
///
/// `egui::Slider` maps the pointer's x straight onto the value, which is
/// what makes a click anywhere on the track jump the knob there - and also
/// why no plugin face had a fine drag: a quarter-rate drag under an
/// absolute mapping snaps the knob back under the pointer the moment the
/// rate changes, and the owner's "they also don't reduce slider sens when
/// holding shift" was the whole of the kit's sliders. So the widget is the
/// kit's own, drawn from the same style fields egui's reads, and it keeps
/// two rules:
///
/// * Unshifted it is egui's: the knob goes where the pointer is, on the
///   press and for as long as the button is held.
/// * Shift PARTS the knob from the pointer. From then until the release
///   the knob moves by the pointer's travel - a quarter of it while Shift
///   is down ([`FINE_RATIO`]), all of it once Shift is up - and never
///   snaps back under the hand. A press made with Shift already down
///   parts them at once, so a fine adjustment starts on the knob where it
///   stands rather than wherever the press landed.
///
/// An alt-press moves nothing, because an alt-click is the reset gesture
/// and a reset that first knocked the knob to the press point would show
/// as a flinch. The double-click reset is [`home_gesture`]'s, unchanged.
///
/// **`centre` is a home the plain drag is caught at**, and marked on the
/// rail: a [`centred_row`]'s, `None` on every other row, which is then
/// exactly the slider it always was. Only the unparted rule catches. A fine
/// step is about a five-hundredth of the travel per point, far inside
/// [`DETENT`], so a catch on the relative rule would put the knob back on
/// its home every frame and a fine drag started there could never leave it:
/// the varispeed dial learned that first, and its catch is a plain drag's
/// alone for the same reason.
fn fine_slider(
    ui: &mut egui::Ui,
    value: f32,
    default: f32,
    centre: Option<f32>,
) -> (egui::Response, Option<f32>) {
    let thickness = ui
        .text_style_height(&egui::TextStyle::Body)
        .max(ui.spacing().interact_size.y);
    let size = Vec2::new(ui.spacing().slider_width, thickness);
    let resp = ui.allocate_response(size, egui::Sense::click_and_drag());
    let rect = resp.rect;
    let radius = rect.height() / 2.5;
    let track = rect.x_range().shrink(radius);
    let key = resp.id.with("face-slider-drag");
    let mut out = None;
    let (shift, alt, pressed_now) =
        ui.input(|i| (i.modifiers.shift, i.modifiers.alt, i.pointer.primary_pressed()));
    if home_gesture(&resp) {
        out = Some(default);
        ui.data_mut(|d| d.remove::<SliderDrag>(key));
    } else if resp.is_pointer_button_down_on() && !alt {
        if let Some(p) = resp.interact_pointer_pos() {
            let held: Option<SliderDrag> = ui.data(|d| d.get_temp(key));
            let mut drag = match held {
                Some(d) if !pressed_now => d,
                _ => SliderDrag { last_x: p.x, parted: shift },
            };
            let dx = p.x - drag.last_x;
            drag.last_x = p.x;
            drag.parted |= shift;
            let next = if drag.parted {
                let rate = if shift { FINE_RATIO } else { 1.0 };
                value + dx / track.span().max(1.0) * rate
            } else {
                let at = egui::remap_clamp(p.x, track, 0.0..=1.0);
                centre.filter(|c| (at - c).abs() <= DETENT).unwrap_or(at)
            };
            ui.data_mut(|d| d.insert_temp(key, drag));
            let next = next.clamp(0.0, 1.0);
            if (next - value).abs() > 1.0e-6 {
                out = Some(next);
            }
        }
    } else {
        ui.data_mut(|d| d.remove::<SliderDrag>(key));
    }
    // Painted exactly as egui paints its own, from the same style fields,
    // so a face wears the same slider it did.
    if ui.is_rect_visible(rect) {
        let shown = out.unwrap_or(value).clamp(0.0, 1.0);
        let visuals = ui.style().interact(&resp);
        let widgets = &ui.visuals().widgets;
        let rail_radius = (ui.spacing().slider_rail_height / 2.0).max(0.0);
        let rail = Rect::from_min_max(
            egui::pos2(rect.left(), rect.center().y - rail_radius),
            egui::pos2(rect.right(), rect.center().y + rail_radius),
        );
        let corner = widgets.inactive.corner_radius;
        ui.painter().rect_filled(rail, corner, widgets.inactive.bg_fill);
        let center = egui::pos2(egui::lerp(track, shown), rail.center().y);
        if ui.visuals().slider_trailing_fill {
            let mut trailing = rail;
            trailing.max.x = center.x + corner.nw as f32;
            ui.painter()
                .rect_filled(trailing, corner, ui.visuals().selection.bg_fill);
        }
        // **The home, marked where the hand can find it**: a hairline across
        // the rail, two points proud of it either side, in the rail's own
        // foreground, so it reads wherever the rail does and asks no colour
        // of its own. The handle covers it at home, which is the look at
        // rest; anywhere else it says where home is.
        if let Some(c) = centre {
            let x = egui::lerp(track, c.clamp(0.0, 1.0));
            ui.painter().line_segment(
                [egui::pos2(x, rail.top() - 2.0), egui::pos2(x, rail.bottom() + 2.0)],
                Stroke::new(1.0_f32, widgets.inactive.fg_stroke.color),
            );
        }
        match ui.style().visuals.handle_shape {
            egui::style::HandleShape::Circle => {
                ui.painter().add(egui::epaint::CircleShape {
                    center,
                    radius: radius + visuals.expansion,
                    fill: visuals.bg_fill,
                    stroke: visuals.fg_stroke,
                });
            }
            egui::style::HandleShape::Rect { aspect_ratio } => {
                let v = Vec2::new(radius * aspect_ratio, radius) + Vec2::splat(visuals.expansion);
                ui.painter().rect(
                    Rect::from_center_size(center, 2.0 * v),
                    visuals.corner_radius,
                    visuals.bg_fill,
                    visuals.fg_stroke,
                    StrokeKind::Inside,
                );
            }
        }
    }
    (resp, out)
}

/// **The reading, which is also a field.** Drawn as the dimmed-or-inked
/// label it always was; a click on it opens the same words as a text box,
/// selected whole so the first key replaces them, and Enter or Tab commits
/// what was typed. Escape, or the focus going elsewhere, puts the label
/// back with nothing changed - the band's rule for its own numeric cells:
/// only Enter commits, because a field that applied whatever it held when
/// the hand wandered off would apply the fumble too.
///
/// Returns the committed text; the row hands it up as [`SliderAsk::Type`]
/// because the number's scale is the host's. The owner: "none of our
/// plugins currently have typing enabled fields."
fn reading_field(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    reading: &str,
    at_rest: bool,
    width: f32,
) -> Option<String> {
    let key = ui.id().with(("face-reading", name));
    let edit_id = key.with("edit");
    let font = FontId::new(10.0, fonts::courier());
    let typing: Option<String> = ui.data(|d| d.get_temp(key));
    let Some(mut text) = typing else {
        let label = ui
            .add(
                egui::Label::new(
                    egui::RichText::new(reading)
                        // **`FontFamily::Monospace` draws NOTHING in this
                        // app.** `egui` is depended on with
                        // `default-features = false`, so its Monospace
                        // family arrives EMPTY and a galley in it lays out
                        // at zero by zero; every reading on every face was
                        // invisible for exactly that reason once. The
                        // named Courier family is the one that exists.
                        .font(font.clone())
                        .color(if at_rest { st.s.faint } else { st.s.ink }),
                )
                .sense(egui::Sense::click()),
            )
            .on_hover_text("click to type a value");
        if label.clicked() {
            ui.data_mut(|d| d.insert_temp(key, reading.to_owned()));
            // Selected whole, so the first key replaces the reading rather
            // than appending to it - what every DAW's field does.
            let mut state = egui::TextEdit::load_state(ui.ctx(), edit_id).unwrap_or_default();
            state.cursor.set_char_range(Some(egui::text::CCursorRange::two(
                egui::text::CCursor::new(0),
                egui::text::CCursor::new(reading.chars().count()),
            )));
            egui::TextEdit::store_state(ui.ctx(), edit_id, state);
            ui.memory_mut(|m| m.request_focus(edit_id));
        }
        return None;
    };
    let resp = ui.add(
        egui::TextEdit::singleline(&mut text)
            .id(edit_id)
            .font(font)
            .desired_width(width.max(24.0)),
    );
    let (commit, abandon) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::Enter) || i.key_pressed(egui::Key::Tab),
            i.key_pressed(egui::Key::Escape),
        )
    });
    if resp.lost_focus() || abandon {
        ui.data_mut(|d| d.remove::<String>(key));
        return (commit && !abandon).then_some(text);
    }
    ui.data_mut(|d| d.insert_temp(key, text));
    None
}

/// One continuous parameter as a row: name, slider, reading. Returns what
/// the hand asked for - see [`SliderAsk`]. The kit's contract, in one
/// place: double-click or alt-click puts the knob back to `default` (never
/// a UI constant - the caller passes the DSP's own number), Shift makes the
/// drag fine, the reading is typed into by clicking it, the hover says so,
/// and the reading dims when the knob is at rest, all exactly as the
/// keyboard panel does.
pub fn slider_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    reading: &str,
    value: f32,
    default: f32,
) -> Option<SliderAsk> {
    slider_row_body(
        ui,
        st,
        name,
        reading,
        value,
        default,
        None,
        "drag to set; shift for fine; double-click or alt-click to reset; click the reading to type",
    )
}

/// How far either side of a centred row's home a plain drag is caught: a
/// sixtieth of the travel, the share the varispeed dial's own catch takes
/// of its default range (half a percent of thirty, `varispeed::DETENT`).
/// About two and a half points either side on the tape's floor: wide enough
/// to hit, narrow enough that nobody aiming at a real value is snapped off.
pub const DETENT: f32 = 1.0 / 60.0;

/// **One continuous parameter that is a trim about its own home**, as a
/// row: the kit's slider row with a mark at `default` and a catch there.
/// Everything else is [`slider_row`]'s contract, unchanged.
///
/// A trim about the point the DSP calls home wants the varispeed dial's
/// laws on a kit row: a home a hand can see, a plain drag that lands on it
/// when it comes within [`DETENT`], and a reading signed about it, which is
/// the host's to print. The home is the pushed default and never a number
/// spelled here, so a unit that moves its home moves the mark and the catch
/// with it. Shift's fine drag is never caught (see [`fine_slider`]); the
/// double-click and the alt-click go home as on every row, and so does a
/// typed reading that names it.
pub fn centred_row(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    reading: &str,
    value: f32,
    default: f32,
) -> Option<SliderAsk> {
    slider_row_body(
        ui,
        st,
        name,
        reading,
        value,
        default,
        Some(default),
        "drag to set, it catches at home; shift for fine, which does not catch; double-click or alt-click to reset; click the reading to type",
    )
}

/// The one body [`slider_row`] and [`centred_row`] are drawn by: the name,
/// the slider, the reading. `centre` is the home a centred row catches and
/// marks, `None` on a plain row; `hover` is what the handle says it does,
/// which differs between the two by exactly the catch.
#[allow(clippy::too_many_arguments)]
fn slider_row_body(
    ui: &mut egui::Ui,
    st: &FaceStyle,
    name: &str,
    reading: &str,
    value: f32,
    default: f32,
    centre: Option<f32>,
    hover: &str,
) -> Option<SliderAsk> {
    let mut out = None;
    ui.horizontal(|ui| {
        let w = ui.available_width();
        let bold = |size: f32| FontId::new(size, fonts::courier_bold());
        ui.add_sized(
            Vec2::new(w * LABEL_W, ROW_H),
            egui::Label::new(
                egui::RichText::new(name).font(bold(11.0)).color(st.s.ink),
            ),
        );
        ui.spacing_mut().slider_width = w * (1.0 - LABEL_W - READING_W) - 16.0;
        let (hand, moved) = fine_slider(ui, value, default, centre);
        if let Some(v) = moved {
            out = Some(SliderAsk::Value(v));
        }
        hand.on_hover_text(hover);
        let at_rest = (value - default).abs() < 0.5 / 100.0;
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(text) = reading_field(ui, st, name, reading, at_rest, w * READING_W) {
                out = Some(SliderAsk::Type(text));
            }
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
            Vec2::new(w * LABEL_W, ROW_H),
            egui::Label::new(
                egui::RichText::new(name).font(bold(11.0)).color(st.s.ink),
            ),
        );
        // **The forty this row used to subtract was a guess, and it stopped
        // being right the day the dialogs took Qt's button padding.**
        //
        // The chevron beside the body is `add_sized(22)`, and `add_sized`
        // only ASKS: egui sizes a button from its own galley plus the
        // theme's padding and takes the LARGER of that and what it was
        // handed. `dialogs::apply_theme` sets that padding to twelve points
        // a side, so the chevron is about thirty-one wide however small a
        // number this row hands it — and forty was never going to cover
        // thirty-one plus two gaps. Every stepped row on the shelf ran
        // seven or eight points off the right of its panel at EVERY width,
        // the EQ's three band-type rows most visibly, and it was invisible
        // for the same reason the preset strip's Save button was: egui
        // clips rather than overdraws.
        //
        // So both parts are measured. The chevron is asked what it costs,
        // the gaps are the style's own, and the body takes the rest — and
        // the body's word is CUT to that rest, because a button is no
        // narrower than its own text and "high shelf" on a dragged-in EQ is
        // wider than the room. The whole choice is on the hover, where the
        // gesture text already was.
        let gap = ui.spacing().item_spacing.x;
        let word = |text: &str, f: FontId| {
            ui.painter()
                .layout_no_wrap(text.to_owned(), f, Color32::WHITE)
                .rect
                .width()
        };
        let pad = ui.spacing().button_padding.x * 2.0;
        let back_w = (word("\u{2039}", bold(11.0)) + pad).max(BACK_W);
        let body_w = (w * (1.0 - LABEL_W) - back_w - gap * 2.0).max(0.0);
        let body = ui
            .add_sized(
                Vec2::new(body_w, ROW_H),
                egui::Button::new(
                    egui::RichText::new(crate::drumpad_panel::cut_text(
                        ui.painter(),
                        (body_w - pad).max(0.0),
                        choice,
                        &bold(10.0),
                    ))
                    .font(bold(10.0))
                    .color(st.s.ink),
                )
                .fill(st.s.raised),
            )
            .on_hover_text(format!(
                "{choice} - click to cycle; right-click steps back; \
                 alt-click resets"
            ));
        if alt_home_gesture(&body) {
            out = Some(StepAsk::Home);
        } else if body.secondary_clicked() {
            out = Some(StepAsk::By(-1));
        } else if body.clicked() {
            out = Some(StepAsk::By(1));
        }
        // The chevron is quieter than the word beside it on purpose — the
        // click on the body is the gesture, this is the way back — but it
        // is printed on the RAISED ground, not the panel's, so its ink is
        // held to that. See [`quiet_ink`].
        let back = ui.add_sized(
            Vec2::new(back_w, ROW_H),
            egui::Button::new(
                egui::RichText::new("‹")
                    .font(bold(11.0))
                    .color(quiet_ink(st, st.s.raised)),
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

/// The hair between [`pointer_row`]'s two lines, so the second reads as the
/// first one's footnote and not as a line of its own.
const POINTER_GAP: f32 = 1.0;

/// How tall [`pointer_row`] is drawn: its two lines and the hair between
/// them, never less than a kit row. One function for drawing and for
/// reserving, so a panel that budgets its height asks this rather than
/// guessing (the `preset_foot_height` bargain).
pub fn pointer_row_height(ui: &egui::Ui) -> f32 {
    let line = |f: FontId| {
        ui.painter()
            .layout_no_wrap("Xg".to_owned(), f, Color32::WHITE)
            .rect
            .height()
    };
    ROW_H.max(
        line(FontId::new(10.0, fonts::courier_bold()))
            + POINTER_GAP
            + line(FontId::new(9.0, fonts::courier())),
    )
}

/// **A row that takes the hand to a setting that lives somewhere else.**
/// What the setting is now on the first line, where it lives on the second,
/// a chevron at the end, and the whole row one target. It owns nothing: the
/// panel says what the lines read and what a click means, which is the
/// primary button's. Every click it takes, whichever button made it, goes
/// through [`note_click`] like every click a face row takes, so the slider
/// clicked next cannot pair it with a click of its own and read a reset.
///
/// One exact allocation, [`pointer_row_height`] tall and as wide as the
/// panel, with both lines and the chevron painted into it, so the row is the
/// same size and takes the same one auto id whatever its lines say: a
/// setting that changes under it moves nothing below. Each line is cut to
/// the room left of the chevron and never wraps. There is no hover fill,
/// because ink on the hover ground is a pair no sweep measures; the pointing
/// hand and the chevron say it is a door.
///
/// **It never takes the keyboard's focus**: `Sense::CLICK`, not
/// `Sense::click()`, which adds `FOCUSABLE` (the notices' rule, in
/// `notify`). A reading above it commits on Tab, Tab hands the focus to the
/// next widget that takes one, and egui presses a focused widget on Enter or
/// Space, the transport's key. The row paints no focus, so a hand that typed
/// a value, tabbed on and pressed Space would have walked through a door it
/// could not see and lost the window it was typing in. So Tab passes it by.
/// A screen reader still finds it and presses it, as the button it is,
/// named by its two lines. Nothing is taken from the keyboard: the row holds
/// nothing, and the setting it leads to is set where it lives.
pub fn pointer_row(ui: &mut egui::Ui, st: &FaceStyle, line: &str, sub: &str, hover: &str) -> bool {
    let (rect, resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), pointer_row_height(ui)),
        egui::Sense::CLICK,
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        let first = FontId::new(10.0, fonts::courier_bold());
        let second = FontId::new(9.0, fonts::courier());
        // The stepped row's chevron family, the other way: this one leads
        // out of the panel rather than back through a list.
        let chevron = painter.text(
            rect.right_center(),
            Align2::RIGHT_CENTER,
            "\u{203A}",
            FontId::new(11.0, fonts::courier_bold()),
            st.s.faint,
        );
        let room = (chevron.left() - rect.left() - ui.spacing().item_spacing.x).max(0.0);
        let top = painter.text(
            rect.left_top(),
            Align2::LEFT_TOP,
            crate::drumpad_panel::cut_text(painter, room, line, &first),
            first,
            st.s.ink,
        );
        painter.text(
            egui::pos2(rect.left(), top.bottom() + POINTER_GAP),
            Align2::LEFT_TOP,
            crate::drumpad_panel::cut_text(painter, room, sub, &second),
            second,
            st.s.faint,
        );
    }
    let enabled = resp.enabled();
    resp.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, format!("{line}, {sub}"))
    });
    let resp = resp
        .on_hover_text(hover)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    // **Every button's click is reported**, not only the one that opens the
    // door: egui keeps one click clock for all of them, so a right-click
    // here is the first half of a double for the primary click after it,
    // and a click left unreported would hand the slider clicked next the
    // pair from before it. The tape's pointer tells the hand to right-click
    // the band's dial, which makes a stray right-click here the likely slip.
    let went = resp.clicked();
    if went
        || resp.secondary_clicked()
        || [egui::PointerButton::Middle, egui::PointerButton::Extra1, egui::PointerButton::Extra2]
            .into_iter()
            .any(|b| resp.clicked_by(b))
    {
        note_click(&resp);
    }
    went
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
            Stroke::new(1.0_f32, lcd_ink(st, LcdTier::Label)),
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
    let _mid = inner.center().x;
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
        Stroke::new(1.0_f32, lcd_ink(st, LcdTier::Label)),
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
    /// **A plain tap on a node: make it the one the rows below serve.**
    /// Appended at the END, and it changes nothing about the sound — which
    /// is the whole point, since the gesture it takes over from did.
    Pick { node: usize },
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
            Stroke::new(1.0_f32, lcd_ink(st, LcdTier::Label).gamma_multiply(0.55)),
        );
        let short = if hz >= 1_000.0 {
            format!("{:.0}k", hz / 1_000.0)
        } else {
            format!("{hz:.0}")
        };
        // **The end labels are held on the glass, and the word is MEASURED
        // to hold them there.** The sweep runs edge to edge by construction
        // — `hz_to_x` puts 20 Hz on `inner.left()` and 20 kHz on
        // `inner.right()` — so a CENTRED word on either of those lines hangs
        // half of itself off the end, and the glass is inset only six. "20k"
        // is three characters wide and painted 2.3 points past the panel at
        // EVERY width the window can be dragged to, which is the stepped
        // row's fault over again and invisible for the same reason: egui
        // clips rather than overdraws, so the picture just quietly lost the
        // k.
        //
        // The half that stayed inside was the worse half. Ink is a property
        // of the surface it lands on, and a word that is not WHOLLY on the
        // glass is a word on the dialog's ground wearing the glass's ink:
        // `LcdTier::Label` is walked to 1.6 against the glass in
        // `FaceStyle::new` and against nothing else, and on Sandstone it
        // measures 1.57 on the panel and 1.97 on the glass it was meant for.
        // Both sweeps caught the same overhang from opposite ends.
        //
        // So the label is laid out and its centre held inside the ends,
        // which moves nothing that already fit and cannot be wrong for a
        // font this was never sized against — the three the app ships differ
        // by a point a character. Max-then-min rather than `clamp`, which
        // asserts when its bounds cross, and a panel narrower than one label
        // crosses them.
        let half = painter
            .layout_no_wrap(short.clone(), FontId::new(label_size, fonts::courier()), Color32::WHITE)
            .rect
            .width()
            * 0.5;
        lcd_text(
            &painter,
            egui::pos2(
                x.max(inner.left() + half).min(inner.right() - half),
                inner.bottom(),
            ),
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
            Stroke::new(1.0_f32, ink),
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
                Stroke::new(3.0_f32, lcd_ink(st, LcdTier::Primary).gamma_multiply(0.22)),
            );
        }
        for i in 1..n {
            painter.line_segment(
                [pt(i - 1), pt(i)],
                Stroke::new(1.4_f32, lcd_ink(st, LcdTier::Primary)),
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
    // **The dragged handle is drawn AT THE HAND, not where the engine last
    // said it was.**
    //
    // Everything else on this glass comes from `nodes`, which the face built
    // from a mirror of the unit's parameters — and that mirror is a frame
    // old by construction: this frame's gesture becomes a `FaceAction`, the
    // app writes it after the frame returns, the host pushes the new value,
    // and the face sees it NEXT frame. So a handle painted from the mirror
    // trails the pointer by one frame for the whole gesture, every gesture.
    //
    // The remedy is the ordinary immediate-mode one: while this widget owns
    // the drag it already knows where the hand is, so it paints there and
    // lets the mirror catch up underneath. The CURVE still trails — that
    // response is computed by the unit and cannot be known here — but the
    // thing the eye is actually tracking stops lagging, which is most of
    // what "not smooth" was.
    let held = drag.and_then(|n| pointer.map(|p| (n, p)));
    for (i, nd) in nodes.iter().enumerate() {
        let at = match held {
            Some((n, p)) if n == i => p,
            _ => egui::pos2(hz_to_x(inner, nd.freq_hz), db_to_y(inner, nd.gain_db)),
        };
        let hot = hovered_node == Some(i) || *drag == Some(i);
        painter.circle_filled(at, if hot { 6.0 } else { 5.0 }, st.lcd_face);
        painter.circle_stroke(
            at,
            if hot { 6.0 } else { 5.0 },
            Stroke::new(1.5_f32, lcd_ink(st, LcdTier::Primary)),
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
    // **BY THE PRIMARY BUTTON, spelled out.** egui decides a drag from the
    // pointer's travel and does NOT ask which button is down
    // (`Sense::click_and_drag` plus any button), so the bare `dragged()` this
    // used to call meant a right-press that wandered two pixels MOVED the
    // node instead of cycling it — the exact hand a right-click gesture
    // invites. `dragged_by` asks.
    use egui::PointerButton as Btn;
    // **The grab is taken on the PRESS, not when egui admits it is a drag.**
    //
    // A widget that senses clicks as well as drags does not report
    // `drag_started` until the hand has travelled far enough not to be a
    // click — and this one only grabbed the node under the pointer at that
    // moment, by which time a quick hand is already past `NODE_GRAB` and
    // `hovered_node` is None. So a decisive flick grabbed NOTHING and the
    // node sat still, while a slow careful one worked: the difference
    // between the two is the hand, not the intent, which is a control that
    // punishes confidence. Measured while writing the test below — the first
    // step of 15 px landed outside the 12 px radius and the drag produced no
    // asks at all.
    //
    // The press is the moment the hand chose a node, so that is when it is
    // remembered. Released clears it, and the click gestures below still
    // read the same latch on their way out.
    if ui.input(|i| i.pointer.primary_pressed()) {
        *drag = hovered_node;
    }
    if resp.dragged_by(Btn::Primary) {
        if let (Some(node), Some(p)) = (*drag, pointer) {
            action = Some(CurveAction::Move {
                node,
                freq_hz: x_to_hz(inner, p.x),
                gain_db: y_to_db(inner, p.y),
            });
        }
    }
    if action.is_none() {
        // **The shape walk is the SECONDARY button now**, at the owner's
        // word: "right click now cycling types". It was the primary click,
        // which cost this widget the one rough edge its own note admitted —
        // egui reports the first press of a double-click as an ordinary
        // click before it can know a second is coming, so double-clicking a
        // node to remove it cycled the shape once on the way past. Moving
        // the walk to the other button retires that entirely rather than
        // trading it for a hand-tuned click delay.
        if resp.secondary_clicked() {
            if let Some(node) = hovered_node {
                action = Some(CurveAction::Cycle { node });
            }
        } else if resp.double_clicked() {
            action = match hovered_node {
                Some(node) => Some(CurveAction::Remove { node }),
                None => pointer.map(|p| CurveAction::Add {
                    freq_hz: x_to_hz(inner, p.x),
                    gain_db: y_to_db(inner, p.y),
                }),
            };
        } else if resp.clicked() {
            // And the freed primary tap SELECTS: the five rows under the
            // glass serve one band at a time, and until now the only way to
            // choose which was the walker. A tap on a handle is what
            // everybody tries first and it used to change the sound.
            if let Some(node) = hovered_node {
                action = Some(CurveAction::Pick { node });
            }
        }
    }
    // Last, so a click that lands on the release frame still sees the node
    // the press took.
    if ui.input(|i| i.pointer.primary_released()) {
        *drag = None;
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
    painter.circle_stroke(c, radius, Stroke::new(1.0_f32, lcd_ink(st, LcdTier::Label)));
    let axis = lcd_ink(st, LcdTier::Label).gamma_multiply(0.6);
    painter.line_segment(
        [c - Vec2::new(0.0, radius), c + Vec2::new(0.0, radius)],
        Stroke::new(1.0_f32, axis),
    );
    painter.line_segment(
        [c - Vec2::new(radius, 0.0), c + Vec2::new(radius, 0.0)],
        Stroke::new(1.0_f32, axis),
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
            (1.3_f32, hot)
        } else if i >= n / 2 {
            (1.0_f32, mid)
        } else {
            (1.0_f32, dim)
        };
        painter.line_segment([prev, at], Stroke::new(w, ink));
        prev = at;
    }
}

/// The shared harness for "does this panel stay inside the window a person
/// can put it at", and for "does its ink read on the surface it landed on".
///
/// Both questions are asked of every face, and the amp asks the first one of
/// itself as well, so the apparatus lives here rather than in one panel's
/// test module. Both are answered off the SHAPE STREAM — what egui was
/// actually told to paint — because that is the only place the answer is
/// not a restatement of the code being tested. A layout that returns the
/// right rectangle and then paints outside it looks perfect to every
/// assertion made against its own arithmetic.
#[cfg(test)]
pub(crate) mod fit {
    use super::*;

    /// The plugin margin `dialogs` puts inside every face window.
    ///
    /// Spelled here rather than read, because `dialogs::PLUGIN_MARGIN` is
    /// private to that module and this crate's firewall gives no other road
    /// to it. It is the one number in this harness that can drift: if the
    /// dialogs ever change their margin these sweeps go on measuring the old
    /// one, and they will do it in the safe direction — a bigger real margin
    /// makes a green sweep still green, a smaller one makes it optimistic.
    /// Whoever changes `PLUGIN_MARGIN` changes this with it.
    pub const MARGIN: f32 = 12.0;

    /// Dress egui's own widgets the way the face window does before it draws
    /// a face.
    ///
    /// `dialogs::apply_theme` is private to that module, so this restates
    /// the derivation rather than calling it — and it is a derivation, not a
    /// palette: a button's ground is the surface's `raised`, a typed field's
    /// is its `well`, the page is `bg` and the text is `ink`, all read off
    /// the same [`Surface`] the face itself is handed. Both halves of this
    /// harness need it and for different reasons. The fit sweeps need it
    /// because the app's button padding is twelve by four against egui's own
    /// six by three, and padding is width; the ink sweep needs it because a
    /// combo box paints its own frame, and measuring the panel's ink against
    /// egui's stock grey would be measuring a window nobody has.
    pub fn dress(style: &mut egui::Style, s: &Surface) {
        style.spacing.button_padding = Vec2::new(12.0, 4.0);
        for wv in [
            &mut style.visuals.widgets.inactive,
            &mut style.visuals.widgets.hovered,
            &mut style.visuals.widgets.active,
            &mut style.visuals.widgets.open,
        ] {
            wv.bg_fill = s.raised;
            wv.weak_bg_fill = s.raised;
            wv.bg_stroke = Stroke::new(1.0_f32, s.edge);
            wv.fg_stroke = Stroke::new(1.0_f32, s.ink);
            wv.expansion = 0.0;
        }
        style.visuals.widgets.hovered.bg_fill = s.hover;
        style.visuals.widgets.hovered.weak_bg_fill = s.hover;
        style.visuals.widgets.noninteractive.bg_fill = s.bg;
        style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, s.ink);
        style.visuals.window_fill = s.bg;
        style.visuals.extreme_bg_color = s.well;
        style.visuals.override_text_color = None;
    }

    /// One frame of a face, drawn the way `dialogs`' face window draws it,
    /// into a window `window` points across.
    ///
    /// The chain is the real one and each link costs height: a face is a
    /// PLUGIN window, so it wears `shell`'s own title bar rather than the
    /// OS's and loses [`crate::shell::CHROME_BAR_H`] off the top; then the
    /// plugin margin on all four sides; then the unit's strap-line and the
    /// air under it, which `dialogs` prints above every face. A sweep that
    /// handed the face the whole window would be measuring a panel nobody
    /// ever sees.
    ///
    /// Returns how far the face escaped to the right and below the room it
    /// was given — zero or less when it stayed inside.
    pub fn escape(
        ctx: &egui::Context,
        window: Vec2,
        about: &str,
        s: &Surface,
        body: &mut dyn FnMut(&mut egui::Ui),
    ) -> (f32, f32) {
        let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), room);
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
                        dress(ui.style_mut(), s);
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::same(MARGIN as i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(about)
                                        .font(FontId::new(10.0, fonts::courier_bold())),
                                );
                                ui.add_space(4.0);
                                took = ui.scope(&mut *body).response.rect;
                            });
                    });
            },
        );
        // **The allocation is one question and what got PAINTED is the
        // other.** A row that asked for more room than the panel has is a
        // layout fault and shows up in `took`; a widget allocated correctly
        // and then drawn past its allocation — which is what a button whose
        // text will not fit does — shows up only in the shapes. egui CLIPS
        // the second kind, so it leaves no trace in the picture at all
        // except the part that is missing, which is exactly why the owner
        // had to report it rather than a test catching it.
        let mut painted = Rect::NOTHING;
        for cs in &out.shapes {
            let b = cs.shape.visual_bounding_rect();
            if b.is_positive() && b.max.is_finite() {
                painted = painted.union(b);
            }
        }
        (
            painted.right().max(took.right()) - room.x,
            painted.bottom().max(took.bottom()) - room.y,
        )
    }

    /// Every word one frame of a face painted, each with the surface it
    /// actually landed on.
    ///
    /// The answer comes off the shape stream and not off the code: egui is
    /// handed shapes in paint order, so the ground under a word is the LAST
    /// opaque rectangle painted before it that contains it, and the panel's
    /// own ground when nothing does. That is the only way to ask the
    /// question the theme law actually poses — ink is a property of the
    /// surface it lands on, never of the role it was fetched from — because
    /// a test that reads the palette instead is a test that agrees with
    /// whatever the drawing code does.
    ///
    /// Semi-transparent fills are skipped rather than composited: the kit
    /// paints its surfaces opaque, and a blend guessed here would be a
    /// second implementation of something egui already owns.
    pub fn ink_on_its_surface(
        ctx: &egui::Context,
        window: Vec2,
        about: &str,
        s: &Surface,
        body: &mut dyn FnMut(&mut egui::Ui),
    ) -> Vec<(Color32, Color32, String)> {
        let ground = s.bg;
        let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), room)),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ctx, |ui| {
                        dress(ui.style_mut(), s);
                        egui::Frame::NONE
                            .inner_margin(egui::Margin::same(MARGIN as i8))
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(about)
                                        .font(FontId::new(10.0, fonts::courier_bold())),
                                );
                                ui.add_space(4.0);
                                body(ui);
                            });
                    });
            },
        );
        let mut surfaces: Vec<(Rect, Color32)> = Vec::new();
        let mut words: Vec<(Color32, Color32, String)> = Vec::new();
        fn walk(
            shape: &egui::Shape,
            surfaces: &mut Vec<(Rect, Color32)>,
            words: &mut Vec<(Color32, Color32, String)>,
            ground: Color32,
        ) {
            match shape {
                egui::Shape::Vec(v) => {
                    for s in v {
                        walk(s, surfaces, words, ground);
                    }
                }
                egui::Shape::Rect(r) => {
                    if r.fill.a() == 255 && r.rect.is_positive() && r.rect.max.is_finite() {
                        surfaces.push((r.rect, r.fill));
                    }
                }
                egui::Shape::Text(t) => {
                    let box_ = Rect::from_min_size(t.pos, t.galley.rect.size());
                    let under = surfaces
                        .iter()
                        .rev()
                        .find(|(r, _)| r.contains_rect(box_))
                        .map_or(ground, |&(_, c)| c);
                    let ink = t.override_text_color.or_else(|| {
                        t.galley.job.sections.first().map(|s| s.format.color)
                    });
                    if let Some(ink) = ink {
                        if ink.a() == 255 {
                            words.push((ink, under, t.galley.text().to_owned()));
                        }
                    }
                }
                _ => {}
            }
        }
        for cs in &out.shapes {
            walk(&cs.shape, &mut surfaces, &mut words, ground);
        }
        words
    }

    /// A face with everything a panel can be asked to draw pushed into it:
    /// the real parameter count from the engine's own spec table, the
    /// longest name and reading any unit prints, a full pedalboard wearing
    /// the longest label a chip can carry, a saved preset, and a folder of
    /// impulses. A sweep run against an EMPTY face measures the one picture
    /// that was never going to overflow.
    pub fn a_worst_case_face(
        path: &str,
        params: usize,
        stepped: &[u32],
        pedals: bool,
    ) -> crate::faces::FaceLive {
        crate::faces::FaceLive {
            face: crate::ports::BuiltinFace {
                path: path.to_owned(),
                params: (0..params as u32)
                    .map(|i| crate::ports::BuiltinFaceParam {
                        id: i,
                        symbol: format!("p{i}"),
                        // "Detector HPF" and "high shelf" are the longest
                        // name and the longest reading in `builtin_fx`'s
                        // tables today. Every row wears them, which no real
                        // unit does — a floor under the real panels, not a
                        // picture of one.
                        name: "Detector HPF".to_owned(),
                        units: "Hz".to_owned(),
                        default: 0.25,
                        reading: "high shelf".to_owned(),
                        choices: if stepped.contains(&i) {
                            vec!["high shelf".to_owned(), "Listening".to_owned()]
                        } else {
                            Vec::new()
                        },
                        steps: i32::from(stepped.contains(&i)) * 2,
                        value: 0.5,
                    })
                    .collect(),
                curve_db: vec![0.0; CURVE_POINTS],
                notes: crate::faces_tape::longest_notes(),
                ..Default::default()
            },
            // The last seat is left EMPTY on purpose: an empty chip wears
            // the quiet ink where a filled one wears the panel's, so a
            // board of seven filled chips is a board with one of the two
            // inks never drawn — and it is the quiet one, which is the one
            // that has a floor to miss.
            pedals: pedals.then(|| {
                std::array::from_fn(|i| {
                    if i + 1 == crate::recorder::AMP_PEDALS {
                        String::new()
                    } else {
                        "compresso\u{2026}".to_owned()
                    }
                })
            }),
            preset_rows: vec![crate::ports::PresetRow {
                name: "a saved sound".to_owned(),
                path: "/p/a.tanfx".to_owned(),
            }],
            preset_name: "typed".to_owned(),
            ir_root: "/m/IRs".to_owned(),
            ir_dir: "/m/IRs".to_owned(),
            ir_entries: (0..40)
                .map(|i| crate::faces::IrEntry {
                    name: format!("cab-{i:02}.wav"),
                    dir: false,
                    seconds: 0.5,
                })
                .collect(),
            // The longest line the tape's pointer can say, both times at
            // their longest reading. Only the tape draws it.
            start_stop: Some(crate::motor::StartStop::new(true, 9.99, 9.99)),
            ..Default::default()
        }
    }

    /// **A sentinel no unit is spelled**, so `faces::draw` takes its generic
    /// arm by definition: every parameter a row, in id order. The kit's
    /// generic-row tests ask this. The tape stood in for it until it grew a
    /// panel of its own, and every real unit is a candidate for one, so a
    /// test that borrows a real unit's path for "the generic rows" is a test
    /// that goes on measuring them only until that unit's panel lands.
    pub const GENERIC_ROWS: &str = "@tangent-generic-rows";

    /// One face's shape as this harness needs it: the sentinel, how many
    /// parameters the engine declares for it, which of those are stepped,
    /// whether its seat carries a pedalboard, and the two windows it lives
    /// between. Named because the tuple has six parts and an unnamed
    /// six-tuple is a shape nobody can read at the call site.
    pub type FaceShape = (&'static str, usize, &'static [u32], bool, Vec2, Vec2);

    /// Every suite face, with the parameter count and the stepped ids the
    /// engine's `builtin_fx` spec tables actually declare, the window it
    /// OPENS at (`dialogs::FACE_SIZES`' floor, which is the size it gets on
    /// any display too small to earn more) and the window it can grow to.
    ///
    /// The counts are restated rather than imported because `tangent-ui`
    /// cannot see the `tangent` crate — the firewall that keeps a plugin
    /// build from linking the engine. `faces::every_suite_unit_routes_to_a_face`
    /// already pins the paths against the same table; this pins the shapes.
    pub const FACES: &[FaceShape] = &[
        ("@tangent-eq", 15, &[0, 1, 5, 6, 10, 11], false, Vec2::new(470.0, 470.0), Vec2::new(752.0, 752.0)),
        ("@tangent-imager", 3, &[], false, Vec2::new(340.0, 480.0), Vec2::new(544.0, 768.0)),
        // The tape's count is its panel's id ceiling, the analog's bargain
        // below: every id the four sections place, whether or not the
        // engine has appended it yet, so the shelf is swept at the panel's
        // fullest. `builtin_fx` pins the ceiling and the stepped ids against
        // the engine's table, and `faces_tape` pins the two windows against
        // `dialogs::FACE_SIZES`.
        (
            crate::faces_tape::TAPE_PATH,
            crate::faces_tape::ID_CEILING,
            crate::faces_tape::STEPPED,
            false,
            Vec2::new(400.0, 520.0),
            Vec2::new(640.0, 832.0),
        ),
        ("@tangent-comp", 7, &[], false, Vec2::new(400.0, 430.0), Vec2::new(640.0, 688.0)),
        ("@tangent-denoise", 6, &[0, 5], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-amp", 9, &[0, 5], true, Vec2::new(540.0, 360.0), Vec2::new(864.0, 576.0)),
        ("@tangent-drive", 4, &[3], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-trem", 3, &[], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-chorus", 5, &[], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-echo", 4, &[], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-plate", 4, &[], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-wah", 4, &[], false, Vec2::new(400.0, 360.0), Vec2::new(640.0, 576.0)),
        ("@tangent-ir", 2, &[], false, Vec2::new(600.0, 400.0), Vec2::new(1160.0, 620.0)),
        ("@tangent-bitcrush", 7, &[5, 6], false, Vec2::new(400.0, 440.0), Vec2::new(640.0, 704.0)),
        ("@tangent-insert", 4, &[3], false, Vec2::new(440.0, 480.0), Vec2::new(704.0, 768.0)),
        // The analog's ids are sparse (0..85 with gaps between the blocks),
        // and this harness numbers its parameters densely, so the count is
        // the id ceiling: every dial the panel draws finds a pushed value,
        // and the gaps are ids the panel never asks for. The stepped list
        // is the kernel's own (`crate::faces_analog::STEPPED`).
        (
            crate::faces_analog::ANALOG_PATH,
            crate::faces_analog::ID_CEILING,
            crate::faces_analog::STEPPED,
            false,
            Vec2::new(900.0, 560.0),
            Vec2::new(1400.0, 840.0),
        ),
        // The sampler's ids are dense 0..16, so the ceiling IS the count.
        (
            crate::faces_sampler::SAMPLER_PATH,
            crate::faces_sampler::ID_CEILING,
            crate::faces_sampler::STEPPED,
            false,
            Vec2::new(800.0, 620.0),
            Vec2::new(1280.0, 992.0),
        ),
    ];

    /// The smallest a face may be DRAGGED to, asked of the production
    /// function itself rather than restated here.
    ///
    /// It used to be a copy - a flat 320 by 300 for every face but the IR -
    /// and a copy is how a test goes on certifying a number the app has
    /// stopped using. `face_min_size` now answers with each face's own floor,
    /// and this asks it, so the two cannot drift again. The infinity is the
    /// "no window to be smaller than" argument: what comes back is the floor.
    pub fn drag_minimum(path: &str) -> Vec2 {
        crate::dialogs::face_min_size(path, Vec2::splat(f32::INFINITY))
    }

    /// A context with one of the three fonts a person can choose installed.
    /// The font is an axis of this question and not a detail: a nine-point
    /// caption is ten points tall on Courier Prime and twelve on JetBrains
    /// Mono, and the amp's whole margin at its own window was five.
    pub fn a_ctx(font: fonts::FontChoice) -> egui::Context {
        let ctx = egui::Context::default();
        fonts::install(&ctx, font, None);
        fonts::apply_text_styles(&ctx);
        ctx
    }

    pub const FONTS: [fonts::FontChoice; 3] = [
        fonts::FontChoice::Courier,
        fonts::FontChoice::Terminess,
        fonts::FontChoice::JetBrains,
    ];

    /// A point of slack, because egui justifies a widget to a fractional
    /// width and rounds the galley it lays out inside it. A sub-pixel
    /// overhang is arithmetic; the faults these sweeps pin ran to a hundred.
    pub const SLACK: f32 = 1.0;

    // ── looking at a face ───────────────────────────────────────────────
    //
    // **A picture, because the ink law has only ever been broken in ways a
    // rendering shows and a test does not.** Nine times, every one of them
    // found by somebody looking at a screenshot. The sweeps above measure
    // contrast off the shape stream and they are worth keeping, but they
    // cannot see a node drawn under a veil, a waveform that came out flat,
    // or a panel that is technically legible and unreadable.
    //
    // `composite.rs`'s `mod shot` photographs the COMPOSITE through a GPU,
    // and its `frame_painting` is private to that file, so a face — which
    // is a window and not part of the video — cannot be reached that way.
    // `busy.rs` solved the same problem for the overlay with a software
    // rasteriser, and this is that method lifted out of one panel's test
    // module so any face can use it: every triangle egui would have sent to
    // the card, including the text, which arrives as triangles sampling the
    // font atlas like anything else.

    /// How many device pixels a point becomes in a shot. Two, so a glyph is
    /// rasterised at the size it is shown rather than magnified.
    pub const SHOT_SCALE: f32 = 2.0;

    /// **One frame of a face, rasterised.** Returns `(width, height, rgba)`.
    ///
    /// The chain is [`escape`]'s, exactly — the chrome bar, the plugin
    /// margin, the strap-line — so what comes back is the window a person
    /// actually sees and not a panel drawn in a vacuum.
    ///
    /// Two `ctx.run` passes with the font atlas ACCUMULATED across both:
    /// the second pass returns only a partial delta (a patch at an offset),
    /// so reading the atlas from it alone gives a fragment and every glyph
    /// then samples its neighbour. That is what made `busy.rs`'s first
    /// version render text as blobs, and it is the one non-obvious line in
    /// here.
    pub fn shoot(
        ctx: &egui::Context,
        window: Vec2,
        about: &str,
        s: &Surface,
        body: &mut dyn FnMut(&mut egui::Ui),
    ) -> (usize, usize, Vec<u8>) {
        ctx.set_pixels_per_point(SHOT_SCALE);
        let room = Vec2::new(window.x, window.y - crate::shell::CHROME_BAR_H);
        let w = (room.x * SHOT_SCALE).round() as usize;
        let h = (room.y * SHOT_SCALE).round() as usize;
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), room);
        let mut atlas: Option<(usize, usize, Vec<f32>)> = None;
        let soak = |d: &egui::TexturesDelta, atlas: &mut Option<(usize, usize, Vec<f32>)>| {
            for (id, img) in &d.set {
                if *id != egui::TextureId::Managed(0) {
                    continue;
                }
                let egui::ImageData::Color(c) = &img.image;
                let cov: Vec<f32> = c.pixels.iter().map(|p| f32::from(p.a()) / 255.0).collect();
                match (img.pos, atlas.as_mut()) {
                    (Some([px, py]), Some((aw, _, ref mut a))) => {
                        for row in 0..c.size[1] {
                            for col in 0..c.size[0] {
                                let dst = (py + row) * *aw + px + col;
                                if dst < a.len() {
                                    a[dst] = cov[row * c.size[0] + col];
                                }
                            }
                        }
                    }
                    _ => *atlas = Some((c.size[0], c.size[1], cov)),
                }
            }
        };
        let run = |body: &mut dyn FnMut(&mut egui::Ui)| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE)
                        .show(ctx, |ui| {
                            dress(ui.style_mut(), s);
                            ui.painter().rect_filled(screen, 0.0, s.bg);
                            egui::Frame::NONE
                                .inner_margin(egui::Margin::same(MARGIN as i8))
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(about)
                                            .font(FontId::new(10.0, fonts::courier_bold()))
                                            .color(s.faint),
                                    );
                                    ui.add_space(4.0);
                                    ui.scope(&mut *body);
                                });
                        });
                },
            )
        };
        let first = run(body);
        soak(&first.textures_delta, &mut atlas);
        let out = run(body);
        soak(&out.textures_delta, &mut atlas);

        // The ground first, so a shot of a panel that paints nothing is the
        // panel's own colour rather than black.
        let mut buf = vec![[s.bg.r(), s.bg.g(), s.bg.b()]; w * h];
        for prim in &ctx.tessellate(out.shapes, SHOT_SCALE) {
            let egui::epaint::Primitive::Mesh(m) = &prim.primitive else {
                continue;
            };
            for tri in m.indices.chunks_exact(3) {
                // **The tessellator's vertices are in POINTS, and this
                // buffer is in device pixels.** `Context::tessellate` takes
                // a pixels-per-point only to size its feathering; it does
                // not scale what it emits, and the renderers multiply in
                // their own vertex shader. Rasterising the points straight
                // into a buffer twice their size drew the whole panel into
                // its top-left QUARTER, which the first shot of the sampler
                // shows perfectly — a picture that looks like a layout bug
                // and is a harness bug.
                let v: Vec<_> = tri
                    .iter()
                    .map(|&i| {
                        let mut vert = m.vertices[i as usize];
                        vert.pos = egui::pos2(vert.pos.x * SHOT_SCALE, vert.pos.y * SHOT_SCALE);
                        vert
                    })
                    .collect();
                let x0 = v.iter().map(|p| p.pos.x).fold(f32::MAX, f32::min);
                let x1 = v.iter().map(|p| p.pos.x).fold(f32::MIN, f32::max);
                let y0 = v.iter().map(|p| p.pos.y).fold(f32::MAX, f32::min);
                let y1 = v.iter().map(|p| p.pos.y).fold(f32::MIN, f32::max);
                let area = |a: egui::epaint::Vertex, b: egui::epaint::Vertex, px: f32, py: f32| {
                    (b.pos.x - a.pos.x) * (py - a.pos.y) - (b.pos.y - a.pos.y) * (px - a.pos.x)
                };
                let denom = area(v[0], v[1], v[2].pos.x, v[2].pos.y);
                if denom.abs() < 1e-9 {
                    continue;
                }
                let py0 = y0.floor().max(0.0) as usize;
                let py1 = y1.ceil().min(h as f32 - 1.0).max(0.0) as usize;
                let px0 = x0.floor().max(0.0) as usize;
                let px1 = x1.ceil().min(w as f32 - 1.0).max(0.0) as usize;
                for py in py0..=py1 {
                    for px in px0..=px1 {
                        let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                        let w0 = area(v[1], v[2], fx, fy) / denom;
                        let w1 = area(v[2], v[0], fx, fy) / denom;
                        let w2 = area(v[0], v[1], fx, fy) / denom;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                            continue;
                        }
                        let col = |f: fn(&egui::epaint::Vertex) -> f32| {
                            w0 * f(&v[0]) + w1 * f(&v[1]) + w2 * f(&v[2])
                        };
                        let (r, g, b, mut a) = (
                            col(|p| f32::from(p.color.r())),
                            col(|p| f32::from(p.color.g())),
                            col(|p| f32::from(p.color.b())),
                            col(|p| f32::from(p.color.a())) / 255.0,
                        );
                        // Text and shapes both arrive as textured triangles;
                        // a shape samples the atlas's white pixel at (0, 0)
                        // and text samples a glyph.
                        let is_glyph = col(|p| p.uv.x) > 1e-4 || col(|p| p.uv.y) > 1e-4;
                        if let (true, Some((aw, ah, px_a))) =
                            (is_glyph, atlas.as_ref().map(|a| (a.0, a.1, &a.2)))
                        {
                            // BILINEAR, because a glyph is a coverage map and
                            // point-sampling one turns letters into blocks.
                            let fu = (col(|p| p.uv.x) * aw as f32 - 0.5).clamp(0.0, aw as f32 - 1.0);
                            let fv = (col(|p| p.uv.y) * ah as f32 - 0.5).clamp(0.0, ah as f32 - 1.0);
                            let (u0, v0) = (fu.floor() as usize, fv.floor() as usize);
                            let (u1, v1) = ((u0 + 1).min(aw - 1), (v0 + 1).min(ah - 1));
                            let (tu, tv) = (fu - u0 as f32, fv - v0 as f32);
                            let at = |x: usize, y: usize| px_a[y * aw + x];
                            let top = at(u0, v0) * (1.0 - tu) + at(u1, v0) * tu;
                            let bot = at(u0, v1) * (1.0 - tu) + at(u1, v1) * tu;
                            a *= top * (1.0 - tv) + bot * tv;
                        }
                        if a <= 0.002 {
                            continue;
                        }
                        let d = &mut buf[py * w + px];
                        for (i, c) in [r, g, b].into_iter().enumerate() {
                            d[i] = (c * a + f32::from(d[i]) * (1.0 - a))
                                .round()
                                .clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            }
        }
        let mut rgba = Vec::with_capacity(w * h * 4);
        for p in &buf {
            rgba.extend_from_slice(&[p[0], p[1], p[2], 255]);
        }
        (w, h, rgba)
    }

    /// A minimal RGBA PNG, so this needs no image crate — `composite.rs`'s
    /// own writer, which is where the shots that already exist come from.
    /// PNG rather than the BMP `busy.rs` writes because the Read tool
    /// displays a PNG and nothing has to be converted before a person (or
    /// the agent that made the picture) can look at it.
    pub fn write_png(path: &std::path::Path, rgba: &[u8], w: usize, h: usize) {
        fn crc(bytes: &[u8]) -> u32 {
            let mut c = 0xffff_ffffu32;
            for b in bytes {
                c ^= u32::from(*b);
                for _ in 0..8 {
                    c = if c & 1 != 0 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
                }
            }
            !c
        }
        fn chunk(out: &mut Vec<u8>, kind: &[u8; 4], data: &[u8]) {
            out.extend_from_slice(&(data.len() as u32).to_be_bytes());
            let mut body = kind.to_vec();
            body.extend_from_slice(data);
            out.extend_from_slice(&body);
            out.extend_from_slice(&crc(&body).to_be_bytes());
        }
        /// Stored (uncompressed) deflate blocks inside a zlib wrapper.
        fn zlib_stored(raw: &[u8]) -> Vec<u8> {
            let mut z = vec![0x78, 0x01];
            let mut a = 1u32;
            let mut b = 0u32;
            for byte in raw {
                a = (a + u32::from(*byte)) % 65521;
                b = (b + a) % 65521;
            }
            for (i, part) in raw.chunks(65_535).enumerate() {
                let last = u8::from((i + 1) * 65_535 >= raw.len());
                z.push(last);
                z.extend_from_slice(&(part.len() as u16).to_le_bytes());
                z.extend_from_slice(&(!(part.len() as u16)).to_le_bytes());
                z.extend_from_slice(part);
            }
            z.extend_from_slice(&((b << 16) | a).to_be_bytes());
            z
        }
        let mut raw = Vec::with_capacity(w * h * 4 + h);
        for y in 0..h {
            raw.push(0);
            let at = y * w * 4;
            raw.extend_from_slice(&rgba[at..at + w * 4]);
        }
        let mut png = vec![0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&(w as u32).to_be_bytes());
        ihdr.extend_from_slice(&(h as u32).to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        chunk(&mut png, b"IHDR", &ihdr);
        chunk(&mut png, b"IDAT", &zlib_stored(&raw));
        chunk(&mut png, b"IEND", &[]);
        std::fs::write(path, png).expect("write the png");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **The suite is dressed by every theme or the build fails.** The same
    /// sweep the loudness meter runs: every factory theme, both
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
    /// any unit without a hand-built panel, which is most of the suite. On
    /// [`fit::GENERIC_ROWS`], which no unit is, so the rows stay generic
    /// whichever unit grows a panel next.
    fn a_face_of(n: usize) -> crate::faces::FaceLive {
        crate::faces::FaceLive {
            face: crate::ports::BuiltinFace {
                path: fit::GENERIC_ROWS.to_owned(),
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
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
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

        let frame = |events: Vec<egui::Event>| -> Option<SliderAsk> {
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
            Some(SliderAsk::Value(HOME)),
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
         -> Option<SliderAsk> {
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
            Some(SliderAsk::Value(HOME)),
            "an alt-click on a slider row did not ask for the DSP's default"
        );
    }

    /// **A centred row catches a plain drag at its home and lets a fine one
    /// through.** A trim about the point the machine is aligned at is one a
    /// hand drags back to that point, so a plain press within [`DETENT`] of
    /// home lands ON it, exactly, and one ten points away is left where it
    /// was put. Shift's fine drag is never caught: it moves about a
    /// five-hundredth of the travel per point, far inside the catch, so a
    /// catch on it would snap the knob home on every frame and a fine drag
    /// that reached home could never leave it, which is the varispeed dial's
    /// lesson on a kit row. The double-click still goes home, and the home
    /// is marked: one hairline where `default` sits on the track, and none
    /// on a plain row. And the catch is the width its doc promises, a
    /// sixtieth of the travel: probed half a point inside its edge and half
    /// a point outside, either side of home, with the sixtieth spelled here
    /// rather than read off [`DETENT`], so a catch three times as wide or
    /// two fifths as wide fails where the presses a point and a half and ten
    /// points out cannot tell.
    ///
    /// The geometry is the row's own at 400 wide: the label takes 136 and
    /// the item gap 8, so the slider runs from 144 to 316 and its track,
    /// inset by the handle's radius, from 151.2 to 308.8, which puts a home
    /// of 0.5 at x 230 and the catch's edges 2.63 points either side. A
    /// press at 231.5 is 0.5095 of the track and one at 240 is 0.5635. The
    /// gestures stand whole seconds apart, because egui dates its
    /// double-clicks and two of these must never pair.
    ///
    /// Revert-checks performed: with the catch added to the parted branch the
    /// fine drag is caught at home and stalls there, and this goes red on the
    /// fine drag; with the catch taken off the plain branch, red on the first
    /// press.
    #[test]
    fn a_centred_row_catches_at_home_and_lets_a_fine_drag_through() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 60.0));
        const HOME: f32 = 0.5;
        // One frame of one row at `t` seconds: what it asked for, and the x
        // of every vertical hairline it painted.
        let frame = |t: f64,
                     events: Vec<egui::Event>,
                     modifiers: egui::Modifiers,
                     value: f32,
                     centred: bool|
         -> (Option<SliderAsk>, Vec<f32>) {
            let mut out = None;
            let painted = ctx.run(
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
                            out = if centred {
                                centred_row(ui, &st, "BIAS", "aligned", value, HOME)
                            } else {
                                slider_row(ui, &st, "BIAS", "aligned", value, HOME)
                            };
                        });
                },
            );
            fn walk(shape: &egui::Shape, marks: &mut Vec<f32>) {
                match shape {
                    egui::Shape::Vec(v) => {
                        for s in v {
                            walk(s, marks);
                        }
                    }
                    egui::Shape::LineSegment { points, .. }
                        if (points[0].x - points[1].x).abs() < 1.0e-3 =>
                    {
                        marks.push(points[0].x);
                    }
                    _ => {}
                }
            }
            let mut marks = Vec::new();
            for cs in &painted.shapes {
                walk(&cs.shape, &mut marks);
            }
            (out, marks)
        };
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers::SHIFT;
        let step = 1.0 / 60.0;

        // ── a plain press a point and a half from home lands on it ──────
        let near = egui::pos2(231.5, 9.0);
        let _ = frame(1.0, vec![egui::Event::PointerMoved(near)], none, 0.8, true);
        let (asked, _) = frame(1.0 + step, vec![press_at(near, true)], none, 0.8, true);
        assert_eq!(
            asked,
            Some(SliderAsk::Value(HOME)),
            "a plain press inside the catch did not land exactly on home"
        );
        let _ = frame(1.0 + 2.0 * step, vec![press_at(near, false)], none, HOME, true);

        // ── and one ten points away is a real value, left where it is put
        let far = egui::pos2(240.0, 9.0);
        let _ = frame(3.0, vec![egui::Event::PointerMoved(far)], none, 0.8, true);
        match frame(3.0 + step, vec![press_at(far, true)], none, 0.8, true).0 {
            Some(SliderAsk::Value(v)) => assert!(
                (v - 0.5635).abs() < 1.0e-3 && v != HOME,
                "a press outside the catch asked {v}, wanted 0.5635 and not home"
            ),
            other => panic!("a press outside the catch asked {other:?}"),
        }
        let _ = frame(3.0 + 2.0 * step, vec![press_at(far, false)], none, 0.5635, true);

        // ── a fine drag walks straight through home ─────────────────────
        // From 0.6, leftward in eight-point steps, the value fed back each
        // frame as a face's is. Each step is a quarter of 8 / 157.6 of the
        // travel, so the seventh lands at 0.511 and the eighth at 0.498:
        // inside the catch on both sides of home, and never on it.
        let mut value = 0.6_f32;
        let mut at = egui::pos2(260.0, 9.0);
        let _ = frame(5.0, vec![egui::Event::PointerMoved(at)], shift, value, true);
        let (pressed, _) = frame(5.0 + step, vec![press_with(at, true, shift)], shift, value, true);
        assert_eq!(pressed, None, "a press with Shift held moved the knob");
        let mut caught = Vec::new();
        let mut t = 5.0 + step;
        for _ in 0..10 {
            t += step;
            at.x -= 8.0;
            if let (Some(SliderAsk::Value(v)), _) =
                frame(t, vec![egui::Event::PointerMoved(at)], shift, value, true)
            {
                if v == HOME {
                    caught.push(v);
                }
                value = v;
            }
        }
        let _ = frame(t + step, vec![press_with(at, false, shift)], shift, value, true);
        assert!(caught.is_empty(), "a fine drag was caught at home {} times", caught.len());
        assert!(
            value < HOME,
            "ten fine steps from 0.6 did not take the knob past home: it stands at {value}"
        );

        // ── a double-click still goes home ──────────────────────────────
        let away = egui::pos2(290.0, 9.0);
        let _ = frame(8.0, vec![egui::Event::PointerMoved(away)], none, 0.8, true);
        let _ = frame(8.0 + step, vec![press_at(away, true)], none, 0.8, true);
        let _ = frame(8.0 + 2.0 * step, vec![press_at(away, false)], none, 0.8, true);
        let _ = frame(8.0 + 3.0 * step, vec![press_at(away, true)], none, 0.8, true);
        assert_eq!(
            frame(8.0 + 4.0 * step, vec![press_at(away, false)], none, 0.8, true).0,
            Some(SliderAsk::Value(HOME)),
            "a double-click on a centred row did not ask for its home"
        );

        // ── and home is marked, on the centred row alone ────────────────
        let (_, marks) = frame(10.0, Vec::new(), none, 0.8, true);
        assert_eq!(marks.len(), 1, "a centred row painted {} home marks", marks.len());
        assert!(
            (marks[0] - 230.0).abs() <= 0.5,
            "the home mark stands at x {}, wanted 230",
            marks[0]
        );
        let (_, plain) = frame(11.0, Vec::new(), none, 0.8, false);
        assert!(plain.is_empty(), "a plain slider row painted a home mark at {plain:?}");

        // ── and the catch is a sixtieth of the travel, either side ──────
        // The width the doc promises, spelled here rather than read off
        // DETENT, so a catch of any other size fails: half a point inside
        // its edge on either side of home lands on home, and half a point
        // outside it is a value of its own. A second between the presses,
        // so no two of them pair into a double.
        let catch = (308.8 - 151.2) / 60.0;
        for (k, (off, caught)) in [(catch - 0.5, true), (-(catch - 0.5), true), (catch + 0.5, false), (-(catch + 0.5), false)]
            .into_iter()
            .enumerate()
        {
            let t = 12.0 + k as f64;
            let at = egui::pos2(230.0 + off, 9.0);
            let _ = frame(t, vec![egui::Event::PointerMoved(at)], none, 0.8, true);
            let asked = frame(t + step, vec![press_at(at, true)], none, 0.8, true).0;
            let _ = frame(t + 2.0 * step, vec![press_at(at, false)], none, 0.8, true);
            let want = (at.x - 151.2) / (308.8 - 151.2);
            match asked {
                Some(SliderAsk::Value(v)) if caught => assert_eq!(
                    v, HOME,
                    "a press {off:+.2} points from home, inside a catch a sixtieth of the travel wide, asked {v}"
                ),
                Some(SliderAsk::Value(v)) => assert!(
                    v != HOME && (v - want).abs() < 1.0e-3,
                    "a press {off:+.2} points from home, outside a catch a sixtieth of the travel wide, asked {v}, \
                     wanted {want}"
                ),
                other => panic!("a press {off:+.2} points from home asked {other:?}"),
            }
        }
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
                            if let Some(SliderAsk::Value(v)) =
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

    /// **Shift makes a slider drag a quarter as far.** Forty points of
    /// travel unshifted, then the same forty with Shift held, on one row
    /// whose value is fed back each frame as a face's is: the second move
    /// is `FINE_RATIO` of the first, and a press made with Shift down does
    /// not first knock the knob to the press point.
    #[test]
    fn shift_makes_a_slider_drag_a_quarter_as_far() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        let mut value = 0.50_f32;
        let frame = |events: Vec<egui::Event>, modifiers: egui::Modifiers, value: &mut f32| {
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
                            if let Some(SliderAsk::Value(v)) =
                                slider_row(ui, &st, "DRIVE", "50%", *value, 0.25)
                            {
                                *value = v;
                            }
                        });
                },
            );
        };
        let none = egui::Modifiers::NONE;
        let shift = egui::Modifiers::SHIFT;
        let a = egui::pos2(200.0, 9.0);
        let b = egui::pos2(240.0, 9.0);
        let c = egui::pos2(280.0, 9.0);
        // Unshifted: press (the knob jumps under the hand), drag forty.
        frame(vec![egui::Event::PointerMoved(a)], none, &mut value);
        frame(vec![press_at(a, true)], none, &mut value);
        let v0 = value;
        frame(vec![egui::Event::PointerMoved(b)], none, &mut value);
        let full = value - v0;
        frame(vec![press_at(b, false)], none, &mut value);
        assert!(full > 0.05, "an unshifted forty-point drag moved the knob by {full}");
        // Shifted: the press moves nothing, the same forty moves a quarter.
        let v1 = value;
        frame(vec![press_with(b, true, shift)], shift, &mut value);
        assert!(
            (value - v1).abs() < 1.0e-6,
            "a press with Shift held knocked the knob from {v1} to {value}"
        );
        frame(vec![egui::Event::PointerMoved(c)], shift, &mut value);
        let fine = value - v1;
        frame(vec![press_with(c, false, shift)], shift, &mut value);
        assert!(
            (fine - full * FINE_RATIO).abs() < 0.01,
            "the shifted drag moved {fine} against {full} unshifted; wanted a quarter"
        );
    }

    /// **The reading is a field: click it, type, Enter, and the words go
    /// up.** The row cannot say what "-6" is on the knob's scale, so the
    /// ask is the text itself - `SliderAsk::Type` - and the label opens
    /// selected whole, so the first key replaces the reading rather than
    /// appending to it.
    #[test]
    fn a_reading_typed_into_hands_up_the_words() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 60.0));
        let frame = |events: Vec<egui::Event>| -> Option<SliderAsk> {
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
                            out = slider_row(ui, &st, "DRIVE", "80%", 0.80, 0.25);
                        });
                },
            );
            out
        };
        // The reading sits at the right-hand end of the row.
        let on_reading = egui::pos2(392.0, 9.0);
        let _ = frame(vec![]);
        let _ = frame(vec![egui::Event::PointerMoved(on_reading)]);
        let _ = frame(vec![press_at(on_reading, true)]);
        let _ = frame(vec![press_at(on_reading, false)]);
        // The box is up and focused: what is typed replaces the selection.
        let _ = frame(vec![]);
        assert!(
            ctx.memory(|m| m.focused()).is_some(),
            "clicking the reading opened no field to type into"
        );
        let _ = frame(vec![egui::Event::Text("-6".to_owned())]);
        let enter = egui::Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        assert_eq!(
            frame(vec![enter]),
            Some(SliderAsk::Type("-6".to_owned())),
            "Enter did not hand up the typed words"
        );
        // And the label is back, with nothing pending.
        assert_eq!(frame(vec![]), None);
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
        // Five stops, and it wraps at the fifth: the face grew two bands at
        // the owner's ask and this row is how they are reached from the
        // keyboard. Six steps from band one is 2,3,4,5,1,2 — a sequence that
        // is wrong in a DIFFERENT place if the walker stalls (repeats) than
        // if the count regresses (wraps early), which is why the whole run
        // is compared rather than the last value.
        assert_eq!(
            walked,
            vec![2, 3, 4, 5, 1, 2],
            "the EQ band walker stopped walking under a fast hand"
        );

        // And the walker still owes the tooltip an answer. It is the one
        // stepped row with no engine number behind it, so its home is the
        // first band rather than a pushed default — walk one step off it,
        // then ask.
        let alt = egui::Modifiers::ALT;
        let none = egui::Modifiers::NONE;
        // **Home first, so this half does not inherit the last one's
        // finishing post.** The run above walks a fixed six steps, so where
        // it stops depends on how many bands there are — it landed on band 1
        // with three and band 2 with five, and this assertion went red for
        // the band COUNT changing rather than for anything it is about.
        // What it is about is the step and the alt-click, and both are
        // measured from a known place.
        live.band = 0;
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

    /// **A click on a pointer row cannot turn the next slider click into a
    /// reset**, whichever button made it.
    ///
    /// The pointer is a door, not a control, so its click means nothing to
    /// the row itself; it is reported to [`note_click`] anyway, because the
    /// answer that function keeps is "what did the hand touch last", and a
    /// click nobody reported leaves the next row believing the pair before
    /// it was its own. So: click a slider, click the pointer over it, click
    /// the slider again, a frame apart, so the last release lands inside
    /// egui's double-click window of the pointer's and its triple-click
    /// window of the slider's first. egui's clock reads it as a double or a
    /// triple, and the test asks egui so, or it would prove nothing; only the
    /// kit's memory of the pointer keeps it from being the slider's, and the
    /// slider from going home. Twice, from a fresh start each time: with the
    /// pointer clicked by the primary button, which opens the door, and by
    /// the secondary, which opens nothing and still runs egui's one clock for
    /// every button.
    #[test]
    fn a_click_on_a_pointer_row_cannot_turn_the_next_slider_click_into_a_reset() {
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(400.0, 200.0));
        let primary = egui::PointerButton::Primary;
        for button in [primary, egui::PointerButton::Secondary] {
            let ctx = a_test_ctx();
            let mut clock = 0.0_f64;
            // One frame of the panel: the pointer, then the slider under it,
            // at 0.8 with its home at 0.25. What each asked, how tall the
            // pointer was drawn, so the presses can find the slider under it,
            // and whether egui read a primary release this frame as a double
            // or a triple.
            let mut frame = |events: Vec<egui::Event>| -> (bool, Option<SliderAsk>, f32, bool) {
                clock += 1.0 / 60.0;
                let mut out = (false, None, 0.0, false);
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
                                let tall = pointer_row_height(ui);
                                let went = pointer_row(ui, &st, "Start and stop: off", "set somewhere else", "go there");
                                let asked = slider_row(ui, &st, "KNOB", "80%", 0.80, 0.25);
                                let paired = ui.input(|i| {
                                    i.pointer.button_double_clicked(primary) || i.pointer.button_triple_clicked(primary)
                                });
                                out = (went, asked, tall, paired);
                            });
                    },
                );
                out
            };

            let (_, _, tall, _) = frame(Vec::new());
            let pointer = egui::pos2(60.0, tall / 2.0);
            // Rows sit one item spacing apart, and the middle of the slider
            // is the middle of its track, well off the 0.8 it holds, so a
            // press there is a change the row reports.
            let slider = egui::pos2(230.0, tall + 3.0 + ROW_H / 2.0);
            let mut went = false;
            let mut paired = false;
            let mut last: Vec<Option<SliderAsk>> = Vec::new();
            for (k, (at, by)) in [(slider, primary), (pointer, button), (slider, primary)].into_iter().enumerate() {
                let mut asks = Vec::new();
                for events in [
                    vec![egui::Event::PointerMoved(at)],
                    vec![egui::Event::PointerButton { pos: at, button: by, pressed: true, modifiers: egui::Modifiers::NONE }],
                    vec![egui::Event::PointerButton { pos: at, button: by, pressed: false, modifiers: egui::Modifiers::NONE }],
                ] {
                    let (pressed, asked, _, pair) = frame(events);
                    went |= pressed && k == 1;
                    paired |= pair && k == 2;
                    asks.push(asked);
                }
                last = asks;
            }
            if button == primary {
                assert!(went, "the pointer was never clicked, so this proves nothing");
            }
            assert!(
                paired,
                "egui did not read the last click as a double or a triple after a {button:?} click on the pointer, \
                 so nothing could have reset and this proves nothing"
            );
            assert!(
                last.iter().flatten().any(|a| matches!(a, SliderAsk::Value(v) if (v - 0.5).abs() < 0.05)),
                "the last click never landed on the slider, so this proves nothing: {last:?}"
            );
            assert!(
                !last.iter().flatten().any(|a| matches!(a, SliderAsk::Value(v) if (v - 0.25).abs() < 1.0e-6)),
                "a {button:?} click on the pointer made the slider's next click a reset: {last:?}"
            );
        }
    }

    /// **A pointer row paints both its lines inside the row it takes, and
    /// cuts them to the room left of its chevron**, on every font the app
    /// ships.
    ///
    /// The row is one exact allocation of [`pointer_row_height`] and its
    /// words are painted into it, not laid out, so nothing in egui stops a
    /// line from running past the allocation: a height that forgot the
    /// second line, or a second line set lower than the height counts it,
    /// paints over whatever is under the row, and no layout test sees it.
    /// So the allocation is held to the height asked of it, and every word
    /// the row paints (its two lines and its chevron, each at the rect its
    /// galley really covers) is held inside that allocation, on all three
    /// fonts. And the lines are cut, never wrapped: at a width that holds
    /// the longest line the tape can say they are painted whole, and at one
    /// far too narrow for it both are cut and end left of the chevron.
    #[test]
    fn a_pointer_row_paints_both_its_lines_inside_itself_on_every_font() {
        let st = a_test_style();
        let (line, sub) = ("Start and stop: 9.99 s up, 9.99 s down", "set on the PITCH CONT dial");
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for width in [400.0_f32, 120.0] {
                let (mut took, mut tall) = (Rect::NOTHING, f32::NAN);
                let out = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(width, 100.0))),
                        ..Default::default()
                    },
                    |ctx| {
                        egui::CentralPanel::default()
                            .frame(egui::Frame::NONE)
                            .show(ctx, |ui| {
                                tall = pointer_row_height(ui);
                                took = ui
                                    .scope(|ui| {
                                        let _ = pointer_row(ui, &st, line, sub, "go there");
                                    })
                                    .response
                                    .rect;
                            });
                    },
                );
                let at = format!("{} at {width} wide", font.key());
                if (took.height() - tall).abs() > 1.0e-3 {
                    bad.push(format!("{at}: the row took {:.2} points, not the {tall:.2} it asked for", took.height()));
                }
                fn walk(shape: &egui::Shape, into: &mut Vec<(String, Rect)>) {
                    match shape {
                        egui::Shape::Vec(v) => {
                            for s in v {
                                walk(s, into);
                            }
                        }
                        egui::Shape::Text(t) => {
                            into.push((t.galley.text().to_owned(), t.galley.rect.translate(t.pos.to_vec2())));
                        }
                        _ => {}
                    }
                }
                let mut words = Vec::new();
                for cs in &out.shapes {
                    walk(&cs.shape, &mut words);
                }
                for (w, r) in &words {
                    if r.top() < took.top() - 1.0e-3
                        || r.bottom() > took.bottom() + 1.0e-3
                        || r.left() < took.left() - 1.0e-3
                        || r.right() > took.right() + 1.0e-3
                    {
                        bad.push(format!("{at}: {w:?} is painted at {r:?}, outside the row's {took:?}"));
                    }
                }
                let Some(chevron) = words.iter().find(|(w, _)| w == "\u{203A}").map(|(_, r)| *r) else {
                    bad.push(format!("{at}: no chevron is painted"));
                    continue;
                };
                let mut lines: Vec<&(String, Rect)> = words.iter().filter(|(w, _)| w != "\u{203A}").collect();
                lines.sort_by(|a, b| a.1.top().total_cmp(&b.1.top()));
                let [(first, first_at), (second, second_at)] = lines.as_slice() else {
                    bad.push(format!("{at}: the row paints {} lines, not two", lines.len()));
                    continue;
                };
                if width >= 400.0 {
                    if (first.as_str(), second.as_str()) != (line, sub) {
                        bad.push(format!("{at}: room for both lines, and it painted {first:?} and {second:?}"));
                    }
                } else {
                    if first == line || second == sub {
                        bad.push(format!("{at}: no room for the lines, and it painted {first:?} over {second:?}, not both cut"));
                    }
                    for (w, r) in [(first, first_at), (second, second_at)] {
                        if r.right() > chevron.left() + 1.0e-3 {
                            bad.push(format!("{at}: {w:?} ends at {:.1}, under the chevron at {:.1}", r.right(), chevron.left()));
                        }
                    }
                }
            }
        }
        assert!(bad.is_empty(), "the pointer row does not hold its words:\n  {}", bad.join("\n  "));
    }


    /// **A double-click puts the whole band back where it shipped — the
    /// HANDLE as well as the curve.**
    ///
    /// It used to set the on-switch to zero and nothing else, so the wave
    /// flattened and the node stayed wherever the hand had left it: hollow,
    /// silent, and floating twelve dB above a curve it was no longer part
    /// of. The owner: "dbl click reset node should make the node jump back
    /// to its default position, right now only the wave moves down when
    /// reset, leaving the node stranded above/below."
    ///
    /// Every knob, not only the two that make the position, because "reset"
    /// has one meaning and a band left on a shape and a width somebody
    /// dialled in is not reset. The values come from the pushed parameters'
    /// own `default`, so the face never holds a second opinion about what
    /// shipped.
    ///
    /// Revert-check performed: send `Set { id: base, value: 0.0 }` alone
    /// again and this goes red on the gain, which is the axis the owner
    /// could see the node stranded on.
    #[test]
    fn a_double_click_puts_the_whole_band_back_where_it_shipped() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(470.0, 470.0));
        let mut live = a_face_of(25);
        live.face.path = "@tangent-eq".to_owned();
        for band in 0..5 {
            let base = band * 5;
            live.face.params[base].value = f32::from(u8::from(band == 0));
            live.face.params[base].default = 0.0;
            live.face.params[base + 1].choices =
                ["bell", "lo-shelf", "hi-shelf"].map(str::to_owned).to_vec();
            live.face.params[base + 1].steps = 3;
            for k in 1..5 {
                live.face.params[base + k].value = 0.5;
                live.face.params[base + k].default = 0.5;
            }
        }
        // Band one, dialled well away from where it shipped: up in level and
        // along in frequency, which is what a used EQ looks like.
        live.face.params[2].value = 0.72;
        live.face.params[3].value = 0.86;
        live.face.params[4].value = 0.19;

        let press = |p: egui::Pos2, down: bool| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        };
        let frame = |events: Vec<egui::Event>, live: &mut crate::faces::FaceLive| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE).show(
                        ctx,
                        |ui| {
                            let act = crate::faces::draw(ui, &st, live);
                            let mut apply = |id: u32, value: f32| {
                                if let Some(p) =
                                    live.face.params.iter_mut().find(|p| p.id == id)
                                {
                                    p.value = value;
                                }
                            };
                            match act {
                                Some(crate::faces::FaceAction::Set { id, value }) => {
                                    apply(id, value);
                                }
                                Some(crate::faces::FaceAction::SetMany { sets }) => {
                                    for (id, value) in sets {
                                        apply(id, value);
                                    }
                                }
                                _ => {}
                            }
                        },
                    );
                },
            );
        };
        // Where the handle actually is, from the numbers above: the curve's
        // own mapping over its inner rect.
        let inner = Rect::from_min_size(egui::pos2(6.0, 6.0), Vec2::new(458.0, 178.0));
        let at = egui::pos2(
            hz_to_x(inner, CURVE_LO_HZ * (CURVE_HI_HZ / CURVE_LO_HZ).powf(0.72)),
            db_to_y(inner, (0.86 * 2.0 - 1.0) * CURVE_DB),
        );
        frame(Vec::new(), &mut live);
        frame(vec![egui::Event::PointerMoved(at)], &mut live);
        // Two clicks in consecutive frames, which is well inside egui's
        // double-click window.
        for _ in 0..2 {
            frame(vec![press(at, true)], &mut live);
            frame(vec![press(at, false)], &mut live);
        }
        let value = |id: u32| -> f32 {
            live.face.params.iter().find(|p| p.id == id).map_or(-1.0, |p| p.value)
        };
        assert_eq!(value(0), 0.0, "the double-click did not take the band out");
        assert!(
            (value(3) - 0.5).abs() < 1e-6,
            "the node is still stranded at gain {:.2} over a curve it left",
            value(3)
        );
        assert!(
            (value(2) - 0.5).abs() < 1e-6,
            "the node did not go home along the frequency axis: {:.2}",
            value(2)
        );
        assert!(
            (value(4) - 0.5).abs() < 1e-6,
            "the width somebody dialled in survived a reset: {:.2}",
            value(4)
        );
    }

    /// **The three things the owner asked of the EQ, measured through
    /// `faces::draw` rather than argued about.**
    ///
    /// He asked for two of them and the third fell out of reading the code
    /// for the first: "the eq needs treatment as well, it is not smooth
    /// dragging those nodes around. Additionally, add 2 more nodes for a 5
    /// total with right click now cycling types."
    ///
    /// 1. **A drag moves BOTH numbers.** It used to send the frequency
    ///    whenever it disagreed with the hand by more than 0.002 of the
    ///    knob's travel — about one pixel on this glass — and the gain only
    ///    once it agreed, which on a diagonal drag is never while the hand
    ///    is moving. So the handle tracked X and refused Y. That is the
    ///    whole of "not smooth", and it is why the assertion is that the
    ///    GAIN moved: the frequency always did.
    /// 2. **The secondary button walks the shapes and the primary does
    ///    not.** Moving the walk off the primary click also retires the
    ///    rough edge `eq_curve`'s own note admitted — egui reports the first
    ///    press of a double-click as a click, so removing a band used to
    ///    cycle its shape on the way past.
    /// 3. **A right-DRAG does not move the node.** egui decides a drag from
    ///    travel and never asks which button is down, so the bare
    ///    `dragged()` this used to call meant a right-press that wandered
    ///    two pixels moved the node instead of cycling it — which is the
    ///    exact hand a right-click gesture invites.
    /// 4. **And the grab survives a FLICK.** The node under the pointer is
    ///    remembered on the press rather than when egui admits the gesture
    ///    is a drag — by that moment a decisive hand is already outside the
    ///    12-point grab radius and the old code grabbed nothing at all. The
    ///    first step of the drag below is 15 points for exactly that reason:
    ///    it is a flick, and it used to produce no asks whatsoever.
    ///
    /// Revert-checks, all performed:
    ///
    /// * put the `(want_f - pf).abs() > 0.002` branch back → (1) goes red
    ///   with the gain unmoved;
    /// * swap `secondary_clicked` for `clicked` → (2) goes red both ways;
    /// * take the grab at `drag_started_by` again → (1) goes red, because
    ///   the flick grabs nothing;
    /// * (3) needs BOTH of its guards undone — loosen the latch to
    ///   `any_pressed()` AND drop `dragged_by(Btn::Primary)`. Either one
    ///   alone still refuses a right-drag, which is not a vacuous test but a
    ///   deliberate second lock: the latch is what actually holds today, and
    ///   the button gate is what keeps holding if somebody moves the latch.
    ///   Said out loud because a revert-check that passes is worth nothing
    ///   unless the reason it passes is written down.
    #[test]
    fn the_curve_moves_on_the_left_and_changes_shape_on_the_right() {
        let ctx = a_test_ctx();
        let st = a_test_style();
        let screen = Rect::from_min_size(egui::pos2(0.0, 0.0), Vec2::new(470.0, 470.0));
        // Five bands of five now — the face's own band-major id plan, grown
        // by two. Only the first is woken: a sleeping band answers every
        // gesture with "wake up" instead of the thing being measured.
        let mut live = a_face_of(25);
        live.face.path = "@tangent-eq".to_owned();
        for band in 0..5 {
            let base = band * 5;
            live.face.params[base].value = f32::from(u8::from(band == 0));
            live.face.params[base + 1].choices =
                ["bell", "lo-shelf", "hi-shelf"].map(str::to_owned).to_vec();
            live.face.params[base + 1].steps = 3;
            for k in 1..5 {
                live.face.params[base + k].value = 0.5;
            }
        }
        let home = egui::pos2(235.0, 95.0);

        let press = |p: egui::Pos2, down: bool, button: egui::PointerButton| {
            egui::Event::PointerButton {
                pos: p,
                button,
                pressed: down,
                modifiers: egui::Modifiers::NONE,
            }
        };
        let frame = |events: Vec<egui::Event>,
                         live: &mut crate::faces::FaceLive| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().frame(egui::Frame::NONE).show(
                        ctx,
                        |ui| {
                            let act = crate::faces::draw(ui, &st, live);
                            let mut apply = |id: u32, value: f32| {
                                if let Some(p) =
                                    live.face.params.iter_mut().find(|p| p.id == id)
                                {
                                    p.value = value;
                                }
                            };
                            match act {
                                Some(crate::faces::FaceAction::Set { id, value }) => {
                                    apply(id, value);
                                }
                                Some(crate::faces::FaceAction::SetPair {
                                    id_a,
                                    value_a,
                                    id_b,
                                    value_b,
                                }) => {
                                    apply(id_a, value_a);
                                    apply(id_b, value_b);
                                }
                                _ => {}
                            }
                        },
                    );
                },
            );
        };
        let value = |live: &crate::faces::FaceLive, id: u32| -> f32 {
            live.face.params.iter().find(|p| p.id == id).map_or(-1.0, |p| p.value)
        };

        // ── 1. a diagonal drag moves the frequency AND the gain ──────────
        let (was_f, was_g) = (value(&live, 2), value(&live, 3));
        let mut at = home;
        // Settle, then press with the pointer already there in the SAME
        // frame: egui takes the grab on the press and only for a pointer it
        // already knows the position of.
        frame(Vec::new(), &mut live);
        frame(
            vec![
                egui::Event::PointerMoved(at),
                press(at, true, egui::PointerButton::Primary),
            ],
            &mut live,
        );
        // Decisively past egui's click/drag threshold on the FIRST step, then
        // a real hand's smaller ones - and diagonal, which is the case that
        // was broken.
        for step in 0..6 {
            let d = if step == 0 { 11.0 } else { 4.0 };
            at += Vec2::new(d, -d);
            frame(vec![egui::Event::PointerMoved(at)], &mut live);
        }
        frame(vec![press(at, false, egui::PointerButton::Primary)], &mut live);
        let (now_f, now_g) = (value(&live, 2), value(&live, 3));
        assert!(
            now_f > was_f + 0.005,
            "the drag did not move the frequency at all: {was_f} -> {now_f}"
        );
        assert!(
            now_g > was_g + 0.005,
            "the drag moved the frequency and NOT the gain, which is the \
             one-ask-per-frame throttle back: gain {was_g} -> {now_g}"
        );

        // ── 2. the buttons: right walks the shape, left does not ─────────
        // Park the handle back in the middle rather than computing where the
        // drag left it: the arithmetic that would find it is the arithmetic
        // under test, so a test that used it could agree with a broken curve.
        for p in live.face.params.iter_mut() {
            if p.id == 2 || p.id == 3 {
                p.value = 0.5;
            }
        }
        let node_at = home;
        frame(Vec::new(), &mut live);
        let kind_before = value(&live, 1);
        frame(vec![egui::Event::PointerMoved(node_at)], &mut live);
        frame(vec![press(node_at, true, egui::PointerButton::Primary)], &mut live);
        frame(vec![press(node_at, false, egui::PointerButton::Primary)], &mut live);
        assert_eq!(
            value(&live, 1),
            kind_before,
            "a LEFT click on a node still changes its shape - the walk was \
             supposed to move to the other button"
        );
        frame(vec![press(node_at, true, egui::PointerButton::Secondary)], &mut live);
        frame(vec![press(node_at, false, egui::PointerButton::Secondary)], &mut live);
        assert_ne!(
            value(&live, 1),
            kind_before,
            "a RIGHT click on a node did not walk its shape"
        );

        // ── 3. and a right-DRAG leaves it where it is ────────────────────
        let (held_f, held_g) = (value(&live, 2), value(&live, 3));
        let mut at = node_at;
        frame(vec![press(at, true, egui::PointerButton::Secondary)], &mut live);
        for _ in 0..6 {
            at += Vec2::new(9.0, -9.0);
            frame(vec![egui::Event::PointerMoved(at)], &mut live);
        }
        frame(vec![press(at, false, egui::PointerButton::Secondary)], &mut live);
        assert_eq!(
            (value(&live, 2), value(&live, 3)),
            (held_f, held_g),
            "a right-drag MOVED the node: egui decides a drag from travel and \
             not from the button, so the gesture has to say which one"
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
                                // **A curve drag sends a PAIR now** — one
                                // gesture carrying a frequency and a gain —
                                // so a counter that only knew `Set` counted
                                // zero, and this test's own "the node was
                                // never actually dragged" guard is what
                                // caught it. Both shapes are applied here,
                                // exactly as the app applies them.
                                let mut apply = |id: u32, value: f32| {
                                    if let Some(p) =
                                        live.face.params.iter_mut().find(|p| p.id == id)
                                    {
                                        p.value = value;
                                    }
                                };
                                match drawn.inner {
                                    Some(crate::faces::FaceAction::Set { id, value }) => {
                                        acted += 1;
                                        apply(id, value);
                                    }
                                    Some(crate::faces::FaceAction::SetPair {
                                        id_a,
                                        value_a,
                                        id_b,
                                        value_b,
                                    }) => {
                                        acted += 1;
                                        apply(id_a, value_a);
                                        apply(id_b, value_b);
                                    }
                                    _ => {}
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

    // ── the panels stay inside their windows ────────────────────────────────

    /// **Every face fits the window it opens at, on every font, at both ends
    /// of the size it is allowed to be.**
    ///
    /// The owner's report was "menus that are cut off in them (cut off on the
    /// right side): Imager, Amp". The shared cause was the preset strip, built
    /// out of four fixed numbers that came to more than the Imager's whole
    /// window; the amp had a second one of its own in seven pedal chips that
    /// could not be sized below their own text. Both are fixed, and this is
    /// the sweep that says so for the WHOLE shelf rather than the two panels
    /// somebody happened to open — thirteen faces, not the six the first sweep
    /// reached, and the seven that route through the generic rows are exactly
    /// the ones nobody had ever measured.
    ///
    /// Three things this asks that the first sweep did not, each of which
    /// found something:
    ///
    /// * **The window is the real one.** Not a bare width, but the size
    ///   `dialogs::FACE_SIZES` opens that unit at, less the in-house title
    ///   bar, the plugin margin and the strap-line above the panel. A face
    ///   handed its whole window is a face measured in a room it never gets.
    /// * **Downwards as well as sideways.** There is no scroll area under a
    ///   face on the desktop path, so a panel taller than its window is drawn
    ///   and unreachable, which is the same failure the owner reported wearing
    ///   a different direction. The amp ran four points past its own window
    ///   here, with nothing dragged, and nothing said so.
    /// * **All three fonts, dressed as the app dresses them.** A nine-point
    ///   caption is ten points tall on Courier Prime and twelve on JetBrains
    ///   Mono, and `dialogs::apply_theme` gives every button twelve points of
    ///   padding a side against egui's own six — so the amp's four points
    ///   past became twenty, and the EQ's band-type rows ran eight off the
    ///   right at EVERY width. A sweep run on stock egui in the default font
    ///   is a sweep of a window nobody has. See [`fit::dress`].
    #[test]
    fn every_face_fits_the_window_it_opens_at() {
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for &(path, n, stepped, pedals, floor, ceiling) in fit::FACES {
                let about = crate::faces::suite_unit(path).map_or("", |(_, a)| a);
                for (edge, window) in [("opens at", floor), ("grown to", ceiling)] {
                    let mut live = fit::a_worst_case_face(path, n, stepped, pedals);
                    let (right, bottom) = fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                        let _ = crate::faces::draw(ui, &st, &mut live);
                    });
                    if right > fit::SLACK {
                        bad.push(format!(
                            "{path} on {} {edge} {}x{}: {right:.1} off the right",
                            font.key(),
                            window.x,
                            window.y
                        ));
                    }
                    if bottom > fit::SLACK {
                        bad.push(format!(
                            "{path} on {} {edge} {}x{}: {bottom:.1} off the bottom",
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
            "a face is drawn outside its own window:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **Dragged to the smallest a face may be, nothing runs off the SIDE —
    /// and exactly three faces still run off the BOTTOM.**
    ///
    /// `dialogs::face_min_size` answers a flat 320 by 300 for every face but
    /// the IR browser, on the stated ground that "320 by 300 is what the knob
    /// rows need". Sideways that is now true of all thirteen, and it is what
    /// the preset strip's shares bought: at 296 points of content the strip
    /// folds rather than losing its Save button, and the amp's chips cut their
    /// word rather than pushing the board off the edge.
    ///
    /// **Downwards it is true now, and it was not.** Against a flat 320 by 300
    /// the EQ's fifteen rows wanted between 114 and 129 points more than the
    /// window held depending on the font, the Imager's goniometer 97 to 99 and
    /// the amp's board 65 to 94 — drawn, clipped and unreachable, because the
    /// desktop path puts no scroll area under a face. `face_min_size` now
    /// answers with each face's OWN floor, the size its rows were laid out in,
    /// so the list below is empty and a face that overflows at its minimum is
    /// a face whose layout outgrew its floor. Do not add it back to the list:
    /// raise that face's floor in `FACE_SIZES`, which is the number the list
    /// used to be standing in for.
    #[test]
    fn the_smallest_a_face_can_be_dragged_to_holds_it_sideways() {
        /// Empty, and it should stay that way: see the note above.
        const TOO_TALL: [&str; 0] = [];
        let set = crate::settings::Settings::default();
        let p = crate::recorder_panel::palette(&set);
        let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
        let mut bad: Vec<String> = Vec::new();
        for font in fit::FONTS {
            let ctx = fit::a_ctx(font);
            for &(path, n, stepped, pedals, _, _) in fit::FACES {
                let about = crate::faces::suite_unit(path).map_or("", |(_, a)| a);
                let window = fit::drag_minimum(path);
                let mut live = fit::a_worst_case_face(path, n, stepped, pedals);
                let (right, bottom) = fit::escape(&ctx, window, about, &st.s, &mut |ui| {
                    let _ = crate::faces::draw(ui, &st, &mut live);
                });
                if right > fit::SLACK {
                    bad.push(format!(
                        "{path} on {}: {right:.1} off the right at {}x{}",
                        font.key(),
                        window.x,
                        window.y
                    ));
                }
                let over = bottom > fit::SLACK;
                if over != TOO_TALL.contains(&path) {
                    bad.push(format!(
                        "{path} on {}: {} off the bottom at {}x{}, and the list {}",
                        font.key(),
                        if over {
                            format!("{bottom:.1}")
                        } else {
                            "nothing".to_owned()
                        },
                        window.x,
                        window.y,
                        if over { "does not say so" } else { "says it should be" }
                    ));
                }
            }
        }
        assert!(
            bad.is_empty(),
            "the drag minimum no longer holds what it is recorded as holding:\n  {}",
            bad.join("\n  ")
        );
    }

    // ── the ink lands on a surface that can carry it ────────────────────────

    /// **Every word a face prints is measured against the surface it actually
    /// landed on**: every panel the shelf sweeps, every factory theme, both
    /// modes.
    ///
    /// This replaces a sweep that read as coverage and was not. That one
    /// walked the themes, built a `FaceStyle` and compared pairs of palette
    /// colours to each other — `ink` against `well`, `faint` against `well` —
    /// and never called the panel it was named after. It passed with the
    /// browser's rows printed on the panel ground and its dead chips printing
    /// `faint` on `raised`, which is the exact fault its own comment said it
    /// existed to prevent. A test that cannot fail against the code it names
    /// is worse than no test, because the next person reads it as proof.
    ///
    /// So this one asks the picture instead. It draws the face, walks the
    /// shape stream in paint order, and for every word finds the last opaque
    /// rectangle painted UNDER it — which is the surface, whatever the code
    /// believed it was. Ink is recognised by its colour, and each role carries
    /// the floor it has always carried: reading contrast for the panel's own
    /// ink, the caption floor for `faint`, the cluster's three tiers for the
    /// glass. Move a word onto a ground that cannot hold it, or move the
    /// ground out from under a word, and this goes red; that is the whole
    /// point, and `every_theme_dresses_a_face` above still guards the palette
    /// the roles are derived from.
    ///
    /// **The vacuity guard is the important line.** Classifying by colour
    /// means a word wearing something unrecognised is skipped rather than
    /// failed — egui's own stock widget text is in this stream too, and the
    /// app's `dialogs::apply_theme` is private to that module, so it cannot be
    /// spoken for here. A skip that grew to cover everything is precisely how
    /// the sweep this replaces became empty, so the count is asserted: if a
    /// face ever stops offering words this can recognise, the build says so
    /// rather than quietly passing.
    #[test]
    fn every_face_prints_its_ink_on_the_surface_it_lands_on() {
        let ctx = fit::a_ctx(fonts::FontChoice::default());
        let mut bad: Vec<String> = Vec::new();
        /// The fewest words a face may print and still be something this
        /// sweep is evidence about. See the assertion at the foot.
        const WORDS_FLOOR: usize = 8;
        let mut thinnest: Vec<String> = Vec::new();
        for theme in crate::theme::factory() {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let p = crate::recorder_panel::palette(&set);
                let st = FaceStyle::new(p.bg, p.lcd, p.lcd_face);
                let mode = if dark { "dark" } else { "light" };
                // Ink the kit and the faces choose, and the floor each has
                // always had to clear. `faint` is a caption and asks the
                // caption floor; the glass tiers ask the cluster's own.
                let roles: [(&str, Color32, f32); 6] = [
                    ("the panel's ink", st.s.ink, 4.5),
                    ("a reading at rest", st.s.faint, 3.0),
                    ("the primary digits", lcd_ink(&st, LcdTier::Primary), 3.0),
                    ("the secondary digits", lcd_ink(&st, LcdTier::Secondary), 2.0),
                    ("a grid label", lcd_ink(&st, LcdTier::Label), 1.6),
                    ("the correlation red", CORR_RED, 1.6),
                ];
                for &(path, n, stepped, pedals, floor, _) in fit::FACES {
                    let about = crate::faces::suite_unit(path).map_or("", |(_, a)| a);
                    let mut live = fit::a_worst_case_face(path, n, stepped, pedals);
                    let words = fit::ink_on_its_surface(&ctx, floor, about, &st.s, &mut |ui| {
                        let _ = crate::faces::draw(ui, &st, &mut live);
                    });
                    let mut seen = 0usize;
                    for (ink, ground, text) in words {
                        let Some(&(what, _, need)) =
                            roles.iter().find(|(_, c, _)| *c == ink)
                        else {
                            continue;
                        };
                        seen += 1;
                        let got = contrast_ratio(ink, ground);
                        if got < need {
                            bad.push(format!(
                                "{} {mode} {path}: {what} on what it landed on is \
                                 {got:.2}:1, wants {need:.1} ({text:?})",
                                theme.name
                            ));
                        }
                    }
                    if seen < WORDS_FLOOR {
                        thinnest.push(format!("{path}: {seen}"));
                    }
                }
            }
        }
        assert!(
            bad.is_empty(),
            "a face prints ink its own surface cannot carry:\n  {}",
            bad.join("\n  ")
        );
        // **The guard against this sweep going the way of the one it
        // replaces.** The smallest panel on the shelf is the tremolo, three
        // rows and the preset strip, and it offers ten recognised words; a
        // face that drops under eight has either lost its rows or started
        // printing them in a colour this cannot account for, and either way
        // the sweep has stopped being evidence about that face. Fail rather
        // than pass quietly, which is exactly what the sweep this replaces
        // did for the whole of its short life.
        thinnest.sort();
        thinnest.dedup();
        assert!(
            thinnest.is_empty(),
            "the sweep no longer recognises enough of what these faces print \
             to be evidence about them:\n  {}",
            thinnest.join("\n  ")
        );
    }
}
