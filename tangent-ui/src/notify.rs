//! Notifications: the app's important messages, said in the corner of the
//! window it is already looking at.
//!
//! **An AREA, never a window, and that is the whole reason this exists.**
//! Every message surface in the app so far is a child VIEWPORT — a real OS
//! window — and they fight each other for z-order. Each one is `AlwaysOnTop`,
//! macOS orders within a level by whichever was raised last, and so the
//! "Rendering..." notice lands in FRONT of the export waveform it is a
//! progress report for, simply because it was revealed second. A notification
//! is the shortest-lived surface in the app and would lose that fight most
//! often, so it does not enter it: it is painted inside the main window on
//! `Order::Foreground`, above everything drawn there and below nothing. It
//! also never takes focus — a message arriving mid-edit that moved the caret
//! out of the field somebody was typing in would be worse than no message.
//!
//! **The clock is a parameter.** `tangent-ui` has no engine dependencies and
//! nothing here may reach for wall time, so ageing is driven by egui's own
//! `ctx.input(|i| i.time)` — monotonic, supplied by the frame — handed in from
//! outside. That is also what makes the whole module testable headless: a test
//! advances the clock by hand and never opens a window.
//!
//! Two ways in, because the app has two kinds of caller. [`show`] does the
//! whole job against a `Context` (its own Area, its own hit test, its own
//! click-eating); [`draw`] is the pure painter for a surface that already owns
//! a `Painter` and a rect, and hands back where every box landed so the caller
//! can do the hit test itself. Neither asks for a repaint — see [`Shown`].

use crate::fonts;
use crate::recorder_panel::{distinct_surface_at, surface_of, REC_RED};
use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};

// ── the clock ───────────────────────────────────────────────────────────────

/// How long an ordinary message stays up, seconds.
///
/// The owner asked for "approx 3-5 seconds". Four is the middle of that, and
/// about two readings of a one-line message.
pub const LIFE_S: f64 = 4.0;

/// How long an ERROR stays up. The top of the owner's range: an error is the
/// message most likely to be worth re-reading, and the one whose text is
/// longest.
pub const LIFE_ERROR_S: f64 = 5.0;

/// The tail, seconds. A message that vanishes between two frames reads as a
/// glitch; one that leaves reads as an ending.
pub const FADE_S: f64 = 0.45;

/// The most one [`Notifications::tick`] may age the stack, seconds.
///
/// **A message must not expire during a stall it was never drawn in.** The UI
/// thread blocks for whole seconds in this app on purpose — a saved instrument
/// warming up, a camera opening, an export starting — and egui's frame time is
/// wall time, so the tick after one of those hands over a delta long enough to
/// retire the entire stack unseen. Clamped, a stall costs the stack one frame
/// of ageing instead of its whole life.
const MAX_STEP: f64 = 0.5;

/// How many messages stand whole at once.
///
/// **A burst of device errors must not paper the desk.** Four fills roughly a
/// third of a short window and still leaves the newest arrival obvious; past
/// four the oldest is sent into its tail early rather than the column growing.
pub const VISIBLE: usize = 4;

// ── the box ─────────────────────────────────────────────────────────────────

/// Inset from the window's top-left corner.
const MARGIN: f32 = 10.0;
/// Between two boxes in the column.
const GAP: f32 = 6.0;
const PAD_X: f32 = 10.0;
const PAD_Y: f32 = 8.0;
/// The kind stripe down the left edge.
const ACCENT_W: f32 = 3.0;
const RADIUS: f32 = 4.0;

/// The column's width, fixed rather than fitted.
///
/// Fitted to the text, a stack of four would be a staircase of four different
/// widths that redraws itself as each one retires. One width is a column, and
/// a column is a thing the eye can ignore once it has read it.
pub const WIDTH: f32 = 300.0;

/// The narrowest the column may be squeezed before it simply hangs off a very
/// small editor. A plugin window can be 240 points wide.
const MIN_WIDTH: f32 = 160.0;

const TEXT_PT: f32 = 11.0;

/// The most rows a box will grow to.
///
/// **Wrap and grow, then stop.** Several of the real messages are multi-line
/// and one of them is about ten; at this width that message is a box taller
/// than the piano band, in the corner, over the thing it is describing. So the
/// box grows to fit up to five rows and elides the rest with an ellipsis — and
/// [`Note::text`] still holds every word, so a caller that wants the whole
/// thing (a log, a notice, a copy button) has it. [`Placed::elided`] says when
/// that happened, which is the caller's cue that there is more to show.
pub const MAX_ROWS: usize = 5;

// ── kinds ───────────────────────────────────────────────────────────────────

/// What a message is about, which decides its accent and how long it stays.
///
/// **Append-only.** Callers match on it and a settings file may one day name
/// one; a new kind goes at the END.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Something happened. Most messages.
    Info,
    /// Something the user asked for finished.
    Success,
    /// Something failed. Lives longer — see [`LIFE_ERROR_S`].
    Error,
}

impl Kind {
    /// How long a message of this kind stays up, seconds.
    pub fn life(self) -> f64 {
        match self {
            Self::Info | Self::Success => LIFE_S,
            Self::Error => LIFE_ERROR_S,
        }
    }

    /// The word a screen reader leads with, so a message that is a WARNING is
    /// not read out in the same breath as one that is a receipt.
    pub fn spoken(self) -> &'static str {
        match self {
            Self::Info => "Notice",
            Self::Success => "Done",
            Self::Error => "Error",
        }
    }
}

/// How far into its life a message of `kind` is: 1.0 whole, 0.0 gone.
///
/// A free function rather than only a method because this arithmetic is the
/// whole behaviour of the module, and a test should be able to ask it
/// directly, with no queue and no clock.
pub fn fade_at(age: f64, kind: Kind) -> f32 {
    let life = kind.life();
    if age >= life {
        return 0.0;
    }
    if age <= life - FADE_S {
        return 1.0;
    }
    (((life - age) / FADE_S) as f32).clamp(0.0, 1.0)
}

// ── the queue ───────────────────────────────────────────────────────────────

/// A handle to one message, stable for its life.
///
/// Opaque: it is worth equality and nothing else, which is all a caller needs
/// to say "the click was for THAT one".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteId(u64);

/// One message.
#[derive(Debug, Clone)]
pub struct Note {
    pub id: NoteId,
    pub kind: Kind,
    /// The message, WHOLE. The box may show less of it — see [`MAX_ROWS`].
    pub text: String,
    /// Seconds this message has been up. Paused while the pointer is on it, so
    /// it does not vanish out from under somebody reading it.
    pub age: f64,
}

impl Note {
    /// 1.0 whole, sliding to 0.0 through the tail.
    pub fn fade(&self) -> f32 {
        fade_at(self.age, self.kind)
    }

    /// Already in its tail: on its way out, and no longer counted against
    /// [`VISIBLE`].
    ///
    /// Inclusive at the boundary, because that boundary is a value [`cap`] sets
    /// EXACTLY when it retires a message early: at `>` a retired message tested
    /// as whole, so a burst of five stacked five and the cap did nothing.
    ///
    /// [`cap`]: Notifications::cap
    pub fn leaving(&self) -> bool {
        self.age >= self.kind.life() - FADE_S
    }

    pub fn expired(&self) -> bool {
        self.age >= self.kind.life()
    }

    /// The line a screen reader is given for this box.
    pub fn spoken(&self) -> String {
        format!("{}. {}", self.kind.spoken(), self.text)
    }
}

/// The stack of messages currently up.
///
/// Holds no clock of its own: [`tick`](Self::tick) is handed the time and works
/// out its own delta, so the same queue ages identically in a window and in a
/// test.
#[derive(Debug, Default, Clone)]
pub struct Notifications {
    /// Oldest first. Painted in reverse — see [`Self::stack`].
    items: Vec<Note>,
    next: u64,
    /// The time of the last tick, so a delta can be derived rather than asked
    /// for. `None` until the first tick, which therefore ages nothing: a queue
    /// pushed to before the first frame must not lose a message to whatever
    /// egui's clock happened to already read.
    last: Option<f64>,
    /// What the pointer was on when the stack was last drawn. Honoured by the
    /// NEXT tick, because hover is only known once the boxes have been laid
    /// out and that happens after the ageing — one frame of lag, invisible.
    held: Option<NoteId>,
}

impl Notifications {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a message up, and hand back its handle.
    ///
    /// **The same text twice is one message, re-aged.** Device errors repeat —
    /// the same failure arrives once per buffer until something changes — and a
    /// column of four identical lines says nothing the first one did not.
    pub fn push(&mut self, kind: Kind, text: impl Into<String>) -> NoteId {
        let text = text.into();
        if let Some(n) = self
            .items
            .iter_mut()
            .find(|n| n.kind == kind && n.text == text)
        {
            n.age = 0.0;
            return n.id;
        }
        let id = NoteId(self.next);
        self.next = self.next.wrapping_add(1);
        self.items.push(Note {
            id,
            kind,
            text,
            age: 0.0,
        });
        self.cap();
        id
    }

    pub fn info(&mut self, text: impl Into<String>) -> NoteId {
        self.push(Kind::Info, text)
    }

    pub fn success(&mut self, text: impl Into<String>) -> NoteId {
        self.push(Kind::Success, text)
    }

    pub fn error(&mut self, text: impl Into<String>) -> NoteId {
        self.push(Kind::Error, text)
    }

    /// Age the stack to `now` and retire whatever ran out.
    ///
    /// `now` is egui's own frame time in seconds — `ctx.input(|i| i.time)` —
    /// which is monotonic and does not need a clock this crate is not allowed
    /// to have.
    ///
    /// **Answers whether anything is still up**, which is the repaint signal: a
    /// caller that stops repainting while this is true freezes the stack
    /// half-faded on screen. Asking for the repaint is the CALLER's job — a
    /// painter that reaches back into the context to demand another frame is
    /// how an overlay becomes a permanent 60 Hz redraw nobody can find.
    pub fn tick(&mut self, now: f64) -> bool {
        let dt = self.last.map_or(0.0, |prev| (now - prev).clamp(0.0, MAX_STEP));
        self.last = Some(now);
        let held = self.held;
        for n in &mut self.items {
            if Some(n.id) != held {
                n.age += dt;
            }
        }
        self.items.retain(|n| !n.expired());
        !self.items.is_empty()
    }

    /// Note which box the pointer is on, if any. The next [`tick`](Self::tick)
    /// leaves that one's clock alone.
    pub fn hold(&mut self, id: Option<NoteId>) {
        self.held = id;
    }

    /// Take a message down NOW. What a click does: the user has said they are
    /// finished with it, and a fade would be the app disagreeing.
    pub fn dismiss(&mut self, id: NoteId) {
        self.items.retain(|n| n.id != id);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn get(&self, id: NoteId) -> Option<&Note> {
        self.items.iter().find(|n| n.id == id)
    }

    /// The queue as it is held: oldest first.
    pub fn notes(&self) -> &[Note] {
        &self.items
    }

    /// The stack in PAINTING order: newest first, top of the column down. Also
    /// the order a hit test must walk, since the newest is the one nearest the
    /// corner.
    pub fn stack(&self) -> impl Iterator<Item = &Note> {
        self.items.iter().rev()
    }

    /// Hold the column to [`VISIBLE`].
    ///
    /// The overflow is sent into its TAIL rather than deleted, so the oldest
    /// slides out under the arrival that displaced it instead of blinking away
    /// — and then a hard ceiling one past the cap, because fifty errors in a
    /// second would otherwise build a column of fifty ghosts all fading at
    /// once.
    fn cap(&mut self) {
        let mut whole = 0;
        for n in self.items.iter_mut().rev() {
            if n.leaving() {
                continue;
            }
            whole += 1;
            if whole > VISIBLE {
                // Never rewound: a message already halfway through its life
                // keeps what it has spent and only loses the rest.
                n.age = n.age.max(n.kind.life() - FADE_S);
            }
        }
        while self.items.len() > VISIBLE + 1 {
            self.items.remove(0);
        }
    }
}

// ── the dress ───────────────────────────────────────────────────────────────

/// How far a notification has to stand off whatever it landed on.
///
/// A touch past the 1.25 a meter's well answers to: a well only has to be
/// findable, and this is an object with words on it that arrived over the top
/// of something else.
const SEPARATION: f32 = 1.35;

/// What an accent has to read at on the card. Not letter contrast — the stripe
/// is a mark, not a word — but well past "you can see where it ends", because
/// it is the only thing that says which KIND of message this is.
const ACCENT_FLOOR: f32 = 3.0;

/// The app's one green, fixed in every theme like the record red beside it: a
/// receipt that changed colour with the palette would have to be learnt twice.
/// Pushed off the card at draw time — see [`Style::of`] — where a green theme
/// would swallow it.
const METER_GREEN: Color32 = Color32::from_rgb(0x3D, 0xC0, 0x5A);

/// Everything a notification is made of, derived from the face it lands on.
///
/// **The face is whatever is UNDER the top-left corner**, which is not one
/// thing: the piano and theory band in some views, the arrangement in others,
/// the desk's strip in a plugin editor. So the caller names it and the card is
/// built from it, rather than the card picking a colour and hoping.
#[derive(Debug, Clone, Copy)]
pub struct Style {
    pub bg: Color32,
    pub ink: Color32,
    /// Secondary ink. Not used by the box itself; carried so a caller drawing a
    /// count or a hint beside the stack has the matching quiet ink.
    pub faint: Color32,
    pub edge: Color32,
    pub info: Color32,
    pub success: Color32,
    pub error: Color32,
}

impl Style {
    /// Dress a notification for the surface it will land on.
    pub fn of(face: Color32) -> Self {
        // The card is that ground MOVED until you can see where it ends —
        // `distinct_surface_at` is the same question a meter's well asks, and
        // handing it the ground as its own starting colour is the honest way to
        // say "this colour, but off the thing behind it". `surface_of` then
        // makes it writable, which on the 0.12–0.22 dead zone means moving the
        // SURFACE again rather than choosing a braver ink.
        let card = distinct_surface_at(face, face, SEPARATION);
        let s = surface_of(card);
        Self {
            bg: s.bg,
            ink: s.ink,
            faint: s.faint,
            edge: s.edge,
            // Info has no hue of its own. Its stripe is the card's quiet ink,
            // which reads as "these are just words" beside a stripe that is
            // red — and it is derived from the card, so it is right on all
            // nineteen palettes without any of them naming it.
            info: s.faint,
            success: distinct_surface_at(METER_GREEN, s.bg, ACCENT_FLOOR),
            error: distinct_surface_at(REC_RED, s.bg, ACCENT_FLOOR),
        }
    }

    pub fn accent(&self, kind: Kind) -> Color32 {
        match kind {
            Kind::Info => self.info,
            Kind::Success => self.success,
            Kind::Error => self.error,
        }
    }
}

// ── painting ────────────────────────────────────────────────────────────────

/// Where one notification landed, and what shape it came out.
#[derive(Debug, Clone, Copy)]
pub struct Placed {
    pub id: NoteId,
    pub kind: Kind,
    pub rect: Rect,
    /// 1.0 whole, 0.0 gone. Everything in the box is multiplied by it.
    pub fade: f32,
    /// Rows the text actually took, after wrapping.
    pub rows: usize,
    /// The message did not fit in [`MAX_ROWS`] and was cut with an ellipsis.
    /// The whole text is still in [`Note::text`].
    pub elided: bool,
}

/// What one frame of the stack covered.
#[derive(Debug, Default, Clone)]
pub struct Drawn {
    /// Newest first, top of the column down — the order a hit test walks.
    pub boxes: Vec<Placed>,
}

impl Drawn {
    /// Which box a point landed in, if any.
    ///
    /// **`None` means the click was never ours**, which is the answer a caller
    /// needs before it lets the press through to the arrangement underneath.
    pub fn hit(&self, pos: Pos2) -> Option<NoteId> {
        self.boxes
            .iter()
            .find(|p| p.rect.contains(pos))
            .map(|p| p.id)
    }

    /// Everything the stack covered, as one rect. `Rect::NOTHING` when nothing
    /// was drawn.
    pub fn covered(&self) -> Rect {
        self.boxes
            .iter()
            .fold(Rect::NOTHING, |acc, p| acc.union(p.rect))
    }
}

/// Draw the stack into the top-left of `area` and say where every box landed.
///
/// Pure: it reads the queue and paints, and does not age anything, dismiss
/// anything or ask for a repaint. A caller on this path does its own
/// [`Notifications::tick`], its own hit test through [`Drawn::hit`], and its
/// own [`Notifications::hold`] for the hover pause.
///
/// Boxes that would run off the bottom of `area` are not drawn at all. A plugin
/// editor can be shorter than four stacked messages, and half a box clipped at
/// the window's edge is worse than one message fewer.
pub fn draw(painter: &Painter, area: Rect, notes: &Notifications, st: &Style) -> Drawn {
    let mut out = Drawn::default();
    if !area.is_positive() {
        return out;
    }
    let w = (area.width() - MARGIN * 2.0).clamp(MIN_WIDTH, WIDTH);
    let wrap = w - PAD_X * 2.0 - ACCENT_W;
    if wrap <= 0.0 {
        return out;
    }
    let font = FontId::new(fonts::px(TEXT_PT), fonts::courier_bold());
    let mut y = area.top() + MARGIN;
    for n in notes.stack() {
        // Laid out with no colour of its own so the same galley can be painted
        // at whatever the tail has faded the ink to this frame.
        let mut job =
            egui::text::LayoutJob::simple(n.text.clone(), font.clone(), Color32::PLACEHOLDER, wrap);
        job.wrap.max_rows = MAX_ROWS;
        job.wrap.overflow_character = Some('…');
        let galley = painter.layout_job(job);
        let h = galley.size().y + PAD_Y * 2.0;
        let rect = Rect::from_min_size(Pos2::new(area.left() + MARGIN, y), Vec2::new(w, h));
        if rect.bottom() > area.bottom() - MARGIN {
            break;
        }
        let fade = n.fade();
        let dim = |c: Color32| c.gamma_multiply(fade);
        painter.rect_filled(rect, RADIUS, dim(st.bg));
        // The stripe carries the kind. Rounded on the left only, or its square
        // corners would sit outside the card's own curve.
        let r = RADIUS as u8;
        painter.rect_filled(
            Rect::from_min_max(rect.min, Pos2::new(rect.left() + ACCENT_W, rect.bottom())),
            CornerRadius {
                nw: r,
                sw: r,
                ne: 0,
                se: 0,
            },
            dim(st.accent(n.kind)),
        );
        painter.rect_stroke(
            rect,
            RADIUS,
            Stroke::new(1.0_f32, dim(st.edge)),
            StrokeKind::Inside,
        );
        painter.galley(
            Pos2::new(rect.left() + ACCENT_W + PAD_X, rect.top() + PAD_Y),
            galley.clone(),
            dim(st.ink),
        );
        out.boxes.push(Placed {
            id: n.id,
            kind: n.kind,
            rect,
            fade,
            rows: galley.rows.len(),
            elided: galley.elided,
        });
        y = rect.bottom() + GAP;
    }
    out
}

// ── the whole job ───────────────────────────────────────────────────────────

/// The Area's id. One per context: there is one stack in one corner.
const AREA_ID: &str = "tangent-notifications";

/// What [`show`] did this frame.
#[derive(Debug, Default, Clone)]
pub struct Shown {
    /// **Honour this or the stack freezes mid-fade.** True while anything is
    /// still up. `show` does not call `request_repaint` itself, because an
    /// overlay that quietly pins the app at the display's refresh rate is a
    /// battery cost nobody can trace back to a message that left four seconds
    /// ago — the caller owns that decision and can see it in its own code.
    pub wants_repaint: bool,
    /// The message a click took down. Returned so the caller knows the press
    /// was SPENT and must not also reach whatever is underneath — though the
    /// Area has already eaten it, so this is a report rather than a duty.
    pub dismissed: Option<NoteId>,
    /// Where the boxes landed, newest first.
    pub boxes: Vec<Placed>,
}

/// Age the stack, paint it into the top-left of `area`, and handle the click.
///
/// The whole job, for the app: it ticks against egui's own clock, puts the
/// column on `Order::Foreground` inside the main window (never a viewport,
/// never focus-taking), pauses the countdown under the pointer, eats a click on
/// a box and takes that message down.
///
/// `face` is whatever the top-left corner is over — see [`Style::of`].
pub fn show(ctx: &egui::Context, area: Rect, notes: &mut Notifications, face: Color32) -> Shown {
    let now = ctx.input(|i| i.time);
    let mut out = Shown {
        wants_repaint: notes.tick(now),
        ..Default::default()
    };
    if notes.is_empty() {
        notes.hold(None);
        return out;
    }
    let st = Style::of(face);
    let mut hovered = None;
    let mut clicked = None;
    egui::Area::new(egui::Id::new(AREA_ID))
        .order(egui::Order::Foreground)
        .fixed_pos(area.min)
        .movable(false)
        // Clamped by `draw` to the rect it was given, which is the window and
        // not the screen; egui's own constraint would fight it.
        .constrain(false)
        // The tail is this module's, timed in the seconds the owner asked for.
        // egui's own fade-in would multiply into it on the first frames and
        // make a fresh message look like a leaving one.
        .fade_in(false)
        .show(ctx, |ui| {
            let drawn = draw(ui.painter(), area, notes, &st);
            for p in &drawn.boxes {
                let r = ui.interact(p.rect, egui::Id::new((AREA_ID, p.id)), egui::Sense::click());
                if let Some(n) = notes.get(p.id) {
                    // Spoken, because the box is the only place the message is
                    // now: a reader that cannot find it never hears it at all.
                    let spoken = n.spoken();
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken)
                    });
                }
                if r.hovered() {
                    hovered = Some(p.id);
                }
                if r.clicked() {
                    clicked = Some(p.id);
                }
                // **The Area has to OWN these rects.** Its size comes from what
                // was laid into it, and `interact` lays nothing: without this
                // the layer is zero-sized, egui decides the pointer is over the
                // panel underneath, and one click both dismisses the message
                // AND lands on the arrangement behind it.
                ui.expand_to_include_rect(p.rect);
            }
            out.boxes = drawn.boxes;
        });
    notes.hold(hovered);
    if let Some(id) = clicked {
        notes.dismiss(id);
        out.dismissed = Some(id);
        // One more frame: the column above the gap has to close up.
        out.wants_repaint = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder_panel::contrast_ratio;

    const SCREEN: Rect = Rect {
        min: Pos2::new(0.0, 0.0),
        max: Pos2::new(900.0, 620.0),
    };

    /// A context with the app's own fonts in it, which is what the box measures
    /// its text with.
    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        fonts::apply_text_styles(&ctx);
        ctx
    }

    /// Sixty frames a second, honestly.
    ///
    /// **Not one long jump**: `tick` clamps its delta at [`MAX_STEP`] so a
    /// stalled UI thread cannot retire the stack unseen, which means a test
    /// that ticks straight from 0 to 2 advances the clock by half a second and
    /// then quietly asserts the wrong thing.
    fn advance(n: &mut Notifications, t: &mut f64, secs: f64) {
        for _ in 0..(secs * 60.0).round() as i32 {
            *t += 1.0 / 60.0;
            n.tick(*t);
        }
    }

    /// Run `f` with a painter, headless. No window, no window manager.
    fn with_painter<R>(f: impl FnOnce(&Painter) -> R) -> R {
        let ctx = ctx();
        let mut out = None;
        // `ctx.run` wants `FnMut`, so the once-only body is handed over rather
        // than called in place.
        let mut once = Some(f);
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                ..Default::default()
            },
            |ctx| {
                let p = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("notify-test"),
                ));
                if let Some(body) = once.take() {
                    out = Some(body(&p));
                }
            },
        );
        out.expect("the frame never ran")
    }

    /// **Readable on every theme, in both modes, or the build fails.** The
    /// sweep `face.rs` and `shell.rs` run, on the notification: its ink and its
    /// quiet ink measured on the card they are printed on, the card measured
    /// against the THREE faces it can land in the corner of — the piano band,
    /// the record band and the desk's strip — its edge against its own ground,
    /// and every kind's accent on the card.
    ///
    /// The accents include the two fixed-meaning colours, which is exactly
    /// where a theme could break them silently: no palette may repaint the
    /// record red, so the only question a theme can answer wrongly is whether
    /// it is still visible.
    #[test]
    fn every_theme_dresses_a_notification() {
        let themes = crate::theme::factory();
        assert!(
            themes.len() >= 19,
            "the sweep expects the nineteen factory themes, found {}",
            themes.len()
        );
        let mut bad: Vec<String> = Vec::new();
        let mut swept = 0;
        for theme in &themes {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let strip = if dark {
                    set.dark_role("fx_strip").map_or_else(
                        || crate::mixer_panel::shade_face(set.fx_strip_color.to_color32(), 0.88),
                        |c| c.to_color32(),
                    )
                } else {
                    set.fx_strip_color.to_color32()
                };
                let band = crate::recorder_panel::palette(&set).bg;
                let keys = crate::piano::themed_bg(&set);
                let mode = if dark { "dark" } else { "light" };
                for (which, face) in [("strip", strip), ("band", band), ("keys", keys)] {
                    let st = Style::of(face);
                    let mut check = |what: &str, r: f32, floor: f32| {
                        if r < floor {
                            bad.push(format!("{} {mode} ({which}): {what} {r:.2}:1", theme.name));
                        }
                    };
                    check("the message on the card", contrast_ratio(st.ink, st.bg), 4.5);
                    check(
                        "the quiet ink on the card",
                        contrast_ratio(st.faint, st.bg),
                        4.5,
                    );
                    // The whole reason the card is derived and not picked: it
                    // has to be seen ON the thing it arrived over.
                    check(
                        "the card on what it lands on",
                        contrast_ratio(st.bg, face),
                        SEPARATION,
                    );
                    check("the edge on the card", contrast_ratio(st.edge, st.bg), 1.5);
                    for kind in [Kind::Info, Kind::Success, Kind::Error] {
                        check(
                            &format!("the {kind:?} accent"),
                            contrast_ratio(st.accent(kind), st.bg),
                            ACCENT_FLOOR,
                        );
                    }
                    swept += 1;
                }
            }
        }
        assert!(swept >= 19 * 2 * 3, "the sweep covered {swept} dressings");
        assert!(
            bad.is_empty(),
            "a notification is unreadable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// The owner's "approx 3-5 seconds", held to the second: an ordinary
    /// message is still up at 3.9 and gone at 4.1, an error outlives it, and
    /// the tail only ever gets thinner — a fade that went back up would read as
    /// the message arriving twice.
    #[test]
    fn a_message_ages_out_and_only_ever_fades() {
        assert!(fade_at(3.9, Kind::Info) > 0.0, "gone before its time");
        assert_eq!(fade_at(4.1, Kind::Info), 0.0, "still up past its time");
        assert_eq!(fade_at(0.0, Kind::Info), 1.0);
        assert_eq!(fade_at(LIFE_S - FADE_S, Kind::Info), 1.0, "faded early");
        // The error gets the top of the range, and is still whole where an
        // ordinary message has already gone.
        assert!(fade_at(4.1, Kind::Error) > 0.0);
        assert_eq!(fade_at(5.01, Kind::Error), 0.0);
        for kind in [Kind::Info, Kind::Success, Kind::Error] {
            let mut prev = f32::INFINITY;
            for step in 0..=700 {
                let f = fade_at(f64::from(step) * 0.01, kind);
                assert!(f <= prev + 1e-6, "{kind:?} brightened at {step}");
                assert!((0.0..=1.0).contains(&f), "{kind:?} fade {f} at {step}");
                prev = f;
            }
        }
    }

    /// The queue's own clock, driven a frame at a time: still up at 3.9
    /// seconds, gone by 4.2, and it says whether anything is left — which is
    /// the repaint signal the caller honours.
    #[test]
    fn the_queue_ages_itself_and_says_when_it_is_done() {
        let mut n = Notifications::new();
        // Pushed BEFORE the first tick: the first tick must age it by nothing,
        // whatever egui's clock already read.
        let id = n.info("saved");
        let mut t = 1_000.0_f64;
        assert!(n.tick(t), "the first tick emptied the queue");
        assert_eq!(n.get(id).map(|x| x.age), Some(0.0));
        advance(&mut n, &mut t, 3.9);
        assert_eq!(n.len(), 1, "gone before its four seconds were up");
        advance(&mut n, &mut t, 0.3);
        assert!(n.is_empty(), "a four-second message outlived four seconds");
        assert!(!n.tick(t + 1.0 / 60.0), "an empty queue wants repaints");
        // An ERROR outlives an ordinary message by the owner's second.
        let mut n = Notifications::new();
        n.error("no such device");
        let mut t = 0.0;
        n.tick(t);
        advance(&mut n, &mut t, 4.2);
        assert_eq!(n.len(), 1, "the error went at four seconds");
        advance(&mut n, &mut t, 1.0);
        assert!(n.is_empty(), "the error outlived five seconds");
        // A stall must not retire a message that was never drawn.
        let mut n = Notifications::new();
        n.error("no such device");
        n.tick(0.0);
        n.tick(30.0);
        assert_eq!(n.len(), 1, "a stall ate the message");
    }

    /// The pointer holds the clock: a message somebody is still reading does
    /// not go while they read it, and starts running again when they leave.
    #[test]
    fn hovering_holds_the_clock() {
        let mut n = Notifications::new();
        let id = n.info("hold me");
        let mut t = 0.0_f64;
        n.tick(t);
        n.hold(Some(id));
        advance(&mut n, &mut t, 10.0);
        assert_eq!(n.len(), 1, "held, and it went anyway");
        assert!(n.get(id).is_some_and(|x| x.age < 0.001));
        n.hold(None);
        advance(&mut n, &mut t, 10.0);
        assert!(n.is_empty(), "let go, and it stayed");
    }

    /// **A burst of device errors must not paper the desk.** Past four whole
    /// messages the oldest is sent into its tail, and the ceiling one past that
    /// keeps a hundred arrivals from becoming a hundred ghosts.
    #[test]
    fn the_stack_retires_the_oldest_rather_than_growing() {
        let mut n = Notifications::new();
        let ids: Vec<_> = (0..4).map(|i| n.info(format!("m{i}"))).collect();
        assert_eq!(n.len(), 4);
        assert!(n.notes().iter().all(|x| !x.leaving()), "retired too early");
        let fifth = n.info("m4");
        assert_eq!(n.len(), 5, "the fifth displaced instead of stacking");
        assert!(
            n.get(ids[0]).is_some_and(Note::leaving),
            "the oldest stayed whole"
        );
        assert!(n.get(fifth).is_some_and(|x| !x.leaving()));
        assert_eq!(
            n.notes().iter().filter(|x| !x.leaving()).count(),
            VISIBLE,
            "more than four whole messages"
        );
        // And the ceiling.
        for i in 5..100 {
            n.info(format!("m{i}"));
        }
        assert!(
            n.len() <= VISIBLE + 1,
            "a burst built a column of {}",
            n.len()
        );
        assert!(n.get(ids[0]).is_none(), "the first message never left");
    }

    /// The same failure arriving once per buffer is one message, re-aged, not
    /// four copies of itself.
    #[test]
    fn the_same_message_twice_is_one_message() {
        let mut n = Notifications::new();
        let first = n.error("Input device disappeared");
        let mut t = 0.0;
        n.tick(t);
        advance(&mut n, &mut t, 2.0);
        assert!(n.get(first).is_some_and(|x| x.age > 1.9));
        let again = n.error("Input device disappeared");
        assert_eq!(again, first, "a new handle for the same message");
        assert_eq!(n.len(), 1);
        assert_eq!(
            n.get(first).map(|x| x.age),
            Some(0.0),
            "the clock did not restart"
        );
        // A different KIND of the same words is a different message: "Done" and
        // "Failed" can share a sentence.
        n.info("Input device disappeared");
        assert_eq!(n.len(), 2);
    }

    /// The column: top-left, newest at the top, one width, inside the window.
    #[test]
    fn the_column_stacks_down_from_the_corner() {
        let mut n = Notifications::new();
        let a = n.info("first");
        let b = n.error("second");
        let c = n.success("third");
        let st = Style::of(Color32::from_rgb(0x2A, 0x24, 0x1E));
        let d = with_painter(|p| draw(p, SCREEN, &n, &st));
        assert_eq!(d.boxes.len(), 3);
        assert_eq!(
            d.boxes.iter().map(|p| p.id).collect::<Vec<_>>(),
            vec![c, b, a],
            "the newest is not nearest the corner"
        );
        for (i, p) in d.boxes.iter().enumerate() {
            assert!((p.rect.left() - MARGIN).abs() < 0.5, "box {i} left {p:?}");
            assert!((p.rect.width() - WIDTH).abs() < 0.5, "box {i} width {p:?}");
            assert!(SCREEN.contains_rect(p.rect), "box {i} escapes the window");
            if i > 0 {
                let above = d.boxes[i - 1].rect;
                assert!(
                    p.rect.top() > above.bottom(),
                    "box {i} overlaps the one above"
                );
                assert!((p.rect.top() - above.bottom() - GAP).abs() < 0.5);
            }
        }
        assert!((d.boxes[0].rect.top() - MARGIN).abs() < 0.5);
        assert!((d.covered().width() - WIDTH).abs() < 0.5);
    }

    /// **A click that lands on a box belongs to the box.** The caller needs the
    /// answer before it lets the press through to the arrangement, so a miss
    /// has to be a definite miss.
    #[test]
    fn the_hit_test_finds_the_box_it_landed_on() {
        let mut n = Notifications::new();
        let a = n.info("first");
        let b = n.error("second");
        let st = Style::of(Color32::from_rgb(0xE3, 0xD9, 0xBD));
        let d = with_painter(|p| draw(p, SCREEN, &n, &st));
        assert_eq!(d.boxes.len(), 2);
        // Newest first: box 0 is `b`.
        assert_eq!(d.hit(d.boxes[0].rect.center()), Some(b));
        assert_eq!(d.hit(d.boxes[1].rect.center()), Some(a));
        // A corner is inside; the gap between two boxes, the desk beside the
        // column and anything off the window are not.
        assert_eq!(d.hit(d.boxes[0].rect.min + Vec2::splat(0.5)), Some(b));
        let gap = Pos2::new(
            d.boxes[0].rect.center().x,
            (d.boxes[0].rect.bottom() + d.boxes[1].rect.top()) * 0.5,
        );
        assert_eq!(d.hit(gap), None, "the gap between boxes swallowed a click");
        assert_eq!(d.hit(Pos2::new(WIDTH + MARGIN * 4.0, 40.0)), None);
        assert_eq!(d.hit(Pos2::new(20.0, SCREEN.bottom() - 20.0)), None);
        assert_eq!(d.hit(Pos2::new(-10.0, -10.0)), None);
    }

    /// **A ten-line message does not become a full-screen box.** It wraps, it
    /// grows to five rows, and then it is cut with an ellipsis — and the whole
    /// text is still in the queue for whoever wants all of it.
    #[test]
    fn a_long_message_is_capped_rather_than_filling_the_window() {
        let long = (1..=10)
            .map(|i| format!("Line {i}: the interface reported a fault on this channel"))
            .collect::<Vec<_>>()
            .join("\n");
        let mut n = Notifications::new();
        let id = n.error(long.clone());
        let st = Style::of(Color32::from_rgb(0x1A, 0x1A, 0x1A));
        let d = with_painter(|p| draw(p, SCREEN, &n, &st));
        let p = d.boxes[0];
        assert!(p.elided, "a ten-line message came out whole");
        assert!(p.rows <= MAX_ROWS, "{} rows", p.rows);
        assert!(
            p.rect.height() < SCREEN.height() * 0.25,
            "the box is {} of a {} window",
            p.rect.height(),
            SCREEN.height()
        );
        assert_eq!(n.get(id).map(|x| x.text.as_str()), Some(long.as_str()));
        // Four of them still stack inside the window rather than running off
        // the bottom of it.
        for i in 0..4 {
            n.error(format!("{long}\n{i}"));
        }
        let d = with_painter(|p| draw(p, SCREEN, &n, &st));
        assert!(!d.boxes.is_empty());
        for b in &d.boxes {
            assert!(SCREEN.contains_rect(b.rect), "a long box escaped: {b:?}");
        }
    }

    /// A window too short for the column drops the boxes it cannot draw whole,
    /// rather than clipping one at the edge.
    #[test]
    fn a_short_window_drops_boxes_instead_of_clipping_them() {
        let mut n = Notifications::new();
        for i in 0..4 {
            n.info(format!("message {i}"));
        }
        let short = Rect::from_min_size(Pos2::ZERO, Vec2::new(320.0, 90.0));
        let st = Style::of(Color32::from_rgb(0x88, 0x88, 0x88));
        let d = with_painter(|p| draw(p, short, &n, &st));
        assert!(d.boxes.len() < 4, "four boxes fitted in ninety points");
        for b in &d.boxes {
            assert!(
                short.contains_rect(b.rect),
                "clipped rather than dropped: {b:?}"
            );
        }
    }

    /// Nothing to say, nothing drawn, nothing broken — including at the sizes a
    /// window briefly is while it is being made.
    #[test]
    fn an_empty_stack_draws_nothing_and_does_not_panic() {
        let n = Notifications::new();
        let st = Style::of(Color32::from_rgb(0x2A, 0x24, 0x1E));
        let d = with_painter(|p| draw(p, SCREEN, &n, &st));
        assert!(d.boxes.is_empty());
        assert_eq!(d.hit(Pos2::new(20.0, 20.0)), None);
        assert_eq!(d.covered(), Rect::NOTHING);
        let mut busy = Notifications::new();
        busy.info("something");
        for r in [
            Rect::NOTHING,
            Rect::from_min_size(Pos2::ZERO, Vec2::ZERO),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(4.0, 4.0)),
            Rect::from_min_size(Pos2::ZERO, Vec2::new(2000.0, 1.0)),
        ] {
            let d = with_painter(|p| draw(p, r, &busy, &st));
            for b in &d.boxes {
                assert!(r.contains_rect(b.rect), "{b:?} escapes {r:?}");
            }
        }
        // And an empty queue is happy to be ticked forever.
        let mut n = Notifications::new();
        for i in 0..10 {
            assert!(!n.tick(f64::from(i)));
        }
        n.dismiss(NoteId(99));
        n.clear();
    }

    /// **The Area takes the click, and the click takes the message down.**
    /// Driven at the event level through a real context, because "it does not
    /// fall through" is a claim about egui's layers and not about arithmetic.
    #[test]
    fn a_click_on_a_box_dismisses_it_through_the_area() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        let id = notes.error("Input device disappeared");
        let face = Color32::from_rgb(0x2A, 0x24, 0x1E);
        let run = |events: Vec<egui::Event>, notes: &mut Notifications| -> Shown {
            let mut out = Shown::default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(SCREEN),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    out = show(ctx, SCREEN, notes, face);
                },
            );
            out
        };
        // A frame to lay it out and learn where it is.
        let first = run(vec![], &mut notes);
        assert!(first.wants_repaint, "a live message wants repaints");
        assert_eq!(first.boxes.len(), 1);
        let at = first.boxes[0].rect.center();
        // Move onto it, then press and release.
        let moved = run(vec![egui::Event::PointerMoved(at)], &mut notes);
        assert!(moved.dismissed.is_none());
        let press = |pressed: bool| egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let click = run(
            vec![egui::Event::PointerMoved(at), press(true), press(false)],
            &mut notes,
        );
        assert_eq!(click.dismissed, Some(id), "the click missed its own box");
        assert!(notes.is_empty(), "dismissed and still in the queue");
        // Nothing up: nothing wants a repaint, and nothing panics.
        let quiet = run(vec![], &mut notes);
        assert!(!quiet.wants_repaint);
        assert!(quiet.boxes.is_empty());
    }

    /// A click well away from the column is not ours, and the message stays.
    #[test]
    fn a_click_elsewhere_leaves_the_message_alone() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        notes.info("still here");
        let face = Color32::from_rgb(0xE3, 0xD9, 0xBD);
        let away = Pos2::new(SCREEN.right() - 40.0, SCREEN.bottom() - 40.0);
        let press = |pressed: bool| egui::Event::PointerButton {
            pos: away,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        let mut out = Shown::default();
        for _ in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(SCREEN),
                    events: vec![egui::Event::PointerMoved(away), press(true), press(false)],
                    ..Default::default()
                },
                |ctx| {
                    out = show(ctx, SCREEN, &mut notes, face);
                },
            );
        }
        assert!(
            out.dismissed.is_none(),
            "a click on the desk took a message down"
        );
        assert_eq!(notes.len(), 1);
    }
}
