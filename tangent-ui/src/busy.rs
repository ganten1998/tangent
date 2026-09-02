//! The busy overlay: what the window says while the UI thread is gone.
//!
//! **The launch splash is not this.** `splash.rs` covers the seconds before
//! the app is usable and deliberately carries no status line — by the time
//! anybody reads it the wordmark IS the answer, and that decision stands. This
//! covers a stall in the MIDDLE of a session: a project opening, somebody
//! else's plugin constructor, eight tracks being pasted, a folder of samples
//! being decoded. The user is already at work and already has a theme, so this
//! one is themed, and it SAYS what is happening — an app that stops answering
//! for four seconds with nothing on screen has crashed as far as anybody
//! watching can tell, which is also the reading macOS gives it when the window
//! server paints its spinning wheel over the top.
//!
//! **It cannot animate while the operation runs, and that is the constraint
//! the whole design is bent around.** Every stall this covers happens ON the
//! UI thread: the frame that paints the overlay is frame N and the block
//! happens inside frame N+1, so what is on screen for the whole of the wait is
//! ONE FROZEN FRAME of this drawing. Every part therefore has to read
//! correctly standing still. A spinner stopped mid-turn is indistinguishable
//! from the hang it was drawn to explain, which is why there is no spinner
//! here: the indeterminate meter is an evenly hatched track with no leading
//! edge, true at every phase and claiming no percentage at any of them. Motion
//! is a BONUS for the operations that do come back between chunks — inserts
//! load one bay per frame, a decode reports per file — and never the thing
//! carrying the message.
//!
//! **The mark is one triangle of the lattice, not the wordmark.** Reusing
//! `splash::draw` was the first thought and it is the wrong drawing twice
//! over: it fills its rect with the app's own opaque black and inks itself in
//! two fixed colours, because a splash is up before anybody has seen the
//! theme — neither is true here — and it needs some two hundred points of
//! width before it will draw at all. Factoring the lattice out of it would
//! mean editing `splash.rs`, and the launch path is not something to disturb
//! for a decoration. So this draws the smallest honest crop of the same
//! lattice, in the card's own measured ink. It is also the right SIZE of
//! statement: the seven-letter wordmark means "the app is starting", and
//! repeating it mid-session would say something that is not true.
//!
//! A pure painter over a given rect, like every other band in this crate. No
//! viewport, no window, nothing the plugin build cannot do.

use crate::face::fit_mono;
use crate::fonts;
use crate::recorder_panel::{
    contrast_ratio, distinct_surface_at, legible_surface, surface_of, Surface,
};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::time::{Duration, Instant};

// ── what is being waited on ─────────────────────────────────────────────────

/// How far along an operation is, when it knows.
///
/// Three cases because callers really do come in three kinds. The insert
/// loader and a multi-file decode count ITEMS and can say "7 of 12"; an
/// offline render knows a share of a timeline and nothing countable; a plugin
/// constructor knows only that it has been called. Making the third one spell
/// itself out as [`Progress::Indeterminate`] rather than pass a fabricated 0.5
/// is the whole point of the enum.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Progress {
    /// Working, with no honest number to show. Drawn as a hatched track: no
    /// leading edge, so it implies no percentage even standing still.
    Indeterminate,
    /// A share of the whole, 0..=1. Clamped, not trusted.
    Fraction(f32),
    /// `done` of `total` countable things. `done` is clamped to `total`,
    /// because "13 of 12" is a caller's arithmetic bug and not something to
    /// put in front of a user; a `total` of 0 has no fraction and no reading,
    /// so it draws as indeterminate rather than dividing by zero.
    Steps { done: usize, total: usize },
}

impl Progress {
    /// The share of the bar to fill, or `None` when there is nothing honest to
    /// fill it with.
    pub fn fraction(self) -> Option<f32> {
        match self {
            Progress::Indeterminate => None,
            Progress::Fraction(f) => Some(if f.is_finite() { f.clamp(0.0, 1.0) } else { 0.0 }),
            Progress::Steps { done, total } => {
                (total > 0).then(|| done.min(total) as f32 / total as f32)
            }
        }
    }

    /// The words beside the bar. `None` means the bar stands alone, which is
    /// half of what makes an indeterminate meter obviously not a measurement.
    pub fn reading(self) -> Option<String> {
        match self {
            Progress::Indeterminate => None,
            Progress::Fraction(_) => self.fraction().map(|f| format!("{:.0}%", f * 100.0)),
            Progress::Steps { done, total } => {
                (total > 0).then(|| format!("{} of {total}", done.min(total)))
            }
        }
    }

    /// The WIDEST reading this operation will ever show.
    ///
    /// The cluster's rule, borrowed: digits are sized on the widest string a
    /// field can hold rather than on the one it holds now, or the number
    /// resizes under the eye every time it climbs. See [`crate::face::fit_mono`].
    pub fn widest(self) -> Option<String> {
        match self {
            Progress::Indeterminate => None,
            Progress::Fraction(_) => Some("100%".to_owned()),
            Progress::Steps { total, .. } => (total > 0).then(|| format!("{total} of {total}")),
        }
    }
}

/// The shortest an overlay stays on screen once it has appeared.
///
/// `splash.rs` met this first (`SPLASH_MIN`, 600 ms): an overlay that appears
/// and vanishes inside two frames is a glitch, not a loading screen. Shorter
/// than the splash's figure on purpose — this one interrupts somebody
/// mid-gesture and the desk it covers is already drawn behind it, so it has
/// less to justify and should be gone sooner.
pub const MIN_VISIBLE: Duration = Duration::from_millis(400);

/// How long [`Busy::deferred`] waits before showing itself at all.
///
/// The other half of the flicker problem: an operation that turns out to be
/// instant should never draw an overlay, not even for the minimum above. Under
/// this long nobody perceives a wait, so nobody needs telling about one.
pub const SHOW_AFTER: Duration = Duration::from_millis(120);

/// One thing the app is waiting on, and the timing that keeps it from
/// flickering.
///
/// **Which constructor, by the shape of the stall:**
///
/// * [`Busy::new`] — the caller KNOWS it is about to block, and blocks in one
///   go: opening a project, instantiating a plugin, decoding an import,
///   starting an offline render. Nothing can be measured after the fact
///   because nothing runs after the fact, so it goes up on the frame BEFORE
///   the call and the delay is zero.
/// * [`Busy::deferred`] — usually instant, occasionally not: pasting tracks,
///   duplicating a lane, an edit that rebuilds a clip's peaks. Most of the
///   time the operation finishes inside [`SHOW_AFTER`] and the overlay is
///   never painted at all, which is better than any overlay could be.
///
/// The fields are public so a chunked operation can rewrite its `detail` and
/// `progress` each frame without rebuilding the struct; `started` is not,
/// because the timing below is only honest if it is stamped once.
#[derive(Clone, Debug)]
pub struct Busy {
    /// What is happening, in the user's words: "Loading project", "Loading
    /// Serum", "Pasting 8 tracks". A sentence fragment, not a status code.
    pub label: String,
    /// The item currently under the knife — a file name, a plugin name, a
    /// track. Optional because plenty of stalls are one indivisible thing
    /// whose name is already the label.
    pub detail: Option<String>,
    /// `None` draws no meter at all: for a cover that is up for a known, brief
    /// moment, where even a hatch would be noise. Everything else should say
    /// [`Progress::Indeterminate`] rather than nothing.
    pub progress: Option<Progress>,
    /// How long to wait before appearing. Zero for a known block.
    pub delay: Duration,
    /// How long to stay once it has appeared. See [`MIN_VISIBLE`].
    pub min_visible: Duration,
    started: Instant,
}

impl Busy {
    /// An overlay for a stall the caller already knows it is about to cause.
    pub fn new(label: impl Into<String>) -> Self {
        Busy {
            label: label.into(),
            detail: None,
            progress: None,
            delay: Duration::ZERO,
            min_visible: MIN_VISIBLE,
            started: Instant::now(),
        }
    }

    /// An overlay that appears only if the operation is still going after
    /// [`SHOW_AFTER`].
    pub fn deferred(label: impl Into<String>) -> Self {
        Busy {
            delay: SHOW_AFTER,
            ..Busy::new(label)
        }
    }

    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    pub fn with_progress(mut self, progress: Progress) -> Self {
        self.progress = Some(progress);
        self
    }

    pub fn with_delay(mut self, delay: Duration) -> Self {
        self.delay = delay;
        self
    }

    pub fn with_min_visible(mut self, min_visible: Duration) -> Self {
        self.min_visible = min_visible;
        self
    }

    /// How long since the operation began.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// Whether to paint it this frame.
    pub fn is_visible(&self) -> bool {
        visible_after(self.elapsed(), self.delay)
    }

    /// Whether an overlay whose work has FINISHED may be dropped now.
    ///
    /// Ask this only once the operation is done; while it is still running the
    /// overlay stays up regardless of any of this.
    pub fn may_close(&self) -> bool {
        may_close_after(self.elapsed(), self.delay, self.min_visible)
    }
}

/// Whether an overlay is on screen this far into its operation.
pub fn visible_after(elapsed: Duration, delay: Duration) -> bool {
    elapsed >= delay
}

/// Whether a finished overlay may be taken down.
///
/// Two ways out, and they are not the same one. Either it never appeared — the
/// work beat the delay, which is the case the delay exists for — or it
/// appeared and has served its minimum. The arithmetic is in WALL TIME and not
/// in frames on purpose: during the block there ARE no frames, and an overlay
/// that owed the screen a count of paints it could never make would still be
/// sitting there after the app came back.
pub fn may_close_after(elapsed: Duration, delay: Duration, min_visible: Duration) -> bool {
    elapsed < delay || elapsed >= delay.saturating_add(min_visible)
}

// ── the dress ───────────────────────────────────────────────────────────────

/// How far the app goes behind the card, as a contrast ratio against its own
/// undimmed self.
///
/// A ratio rather than an alpha, because an alpha is not a dimming: 30% black
/// over ivory is a clear veil and 30% black over the near-black desk every
/// dark theme ships is nothing at all. This is the amount of CHANGE the eye
/// has to be able to see; the alpha that delivers it is solved for below.
const SCRIM_TARGET: f32 = 2.2;
/// The lightest and heaviest veil allowed. The floor keeps a pale desk from
/// getting a scrim so thin it reads as a rendering fault; the cap keeps the
/// work visible underneath, which is the whole reason this is a scrim and not
/// a full-screen fill.
const SCRIM_MIN_A: f32 = 0.10;
const SCRIM_MAX_A: f32 = 0.72;
/// How far the card has to stand off the dimmed desk behind it.
const CARD_FLOOR: f32 = 1.6;
/// And how far the meter's empty track has to stand off the card.
///
/// The panel's own well is "a step away from the ink", which is the right
/// look and has nowhere to go on the extremes: on Phosphor's near-black card
/// a recessed well is 1.09:1 from the card and on Porcelain's near-white one
/// it is 1.08:1 — a meter whose empty half is invisible, so a bar at 10% reads
/// as a bar at 100%. Where the well cannot recede, `distinct_surface_at` lifts
/// it instead.
const TRACK_FLOOR: f32 = 1.35;

/// Everything the overlay is made of, derived from the desk's own face.
///
/// One colour in, because that is all a painter over an arbitrary window can
/// honestly be told, and it is what every other surface in this app derives
/// itself from. Each value below is MEASURED on the thing it lands on and
/// never inherited: the ink is what `surface_of` puts on the CARD, not what
/// the desk happens to wear — the lesson this codebase has now learned from a
/// screenshot more times than it has learned anything else.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dress {
    /// Painted over the whole window. Translucent — the work must still be
    /// visible underneath, or the overlay reads as "your session is gone".
    pub scrim: Color32,
    /// What the scrim leaves the desk looking like, opaque, for measurement.
    /// The card is told apart from THIS and not from the raw face, because
    /// this is what is actually behind the card.
    pub veiled: Color32,
    pub card: Color32,
    pub ink: Color32,
    /// The detail line: quieter by weight, still held to reading contrast.
    pub faint: Color32,
    pub edge: Color32,
    /// The meter's well, and the fill that runs along it.
    pub track: Color32,
    pub fill: Color32,
}

impl Dress {
    /// Dress an overlay over a desk whose face is `face`.
    pub fn of(face: Color32) -> Dress {
        let (veil, alpha) = veil_of(face);
        let veiled = over(face, veil, alpha);
        // The card keeps the app's OWN panel colour and is pushed off the
        // veiled desk only if it fails to separate from it. Deriving it from
        // the veiled ground instead would throw the theme away at exactly the
        // moment somebody is staring at the screen with nothing else to look
        // at.
        let card = legible_surface(distinct_surface_at(surface_of(face).bg, veiled, CARD_FLOOR));
        let s: Surface = surface_of(card);
        let track = distinct_surface_at(s.well, card, TRACK_FLOOR);
        Dress {
            scrim: Color32::from_rgba_unmultiplied(
                veil.r(),
                veil.g(),
                veil.b(),
                (alpha * 255.0).round().clamp(0.0, 255.0) as u8,
            ),
            veiled,
            card,
            ink: s.ink,
            faint: s.faint,
            edge: s.edge,
            track,
            // **The fill is ink, not a colour of its own.** A progress bar is
            // a reading and this app draws its readings in ink; a fifth
            // meaning of colour on a surface that already carries record red,
            // meter green and the channel palette would be one too many. It
            // still goes through the measurement, because the track it runs
            // along is a surface like any other.
            fill: distinct_surface_at(s.ink, track, 3.0),
        }
    }
}

/// Composite `veil` at `alpha` over `base`.
///
/// Straight sRGB byte arithmetic, which is close enough to what the compositor
/// does with a premultiplied `Color32` to design against — and it is the same
/// function the alpha is CHOSEN with below, so the number that gets measured
/// is the number that gets painted.
fn over(base: Color32, veil: Color32, alpha: f32) -> Color32 {
    let a = alpha.clamp(0.0, 1.0);
    let mix = |b: u8, v: u8| (f32::from(b) * (1.0 - a) + f32::from(v) * a).round() as u8;
    Color32::from_rgb(
        mix(base.r(), veil.r()),
        mix(base.g(), veil.g()),
        mix(base.b(), veil.b()),
    )
}

/// Which way the scrim veils, and how heavily.
///
/// **Both directions are measured, exactly as `wants_light_ink` measures both
/// inks and for the same reason.** A black scrim is the conventional answer
/// and it is the wrong one half the time: black over the near-black desk is
/// not a veil, and an app that dims by nothing at all looks as though it
/// stopped painting rather than as though it is thinking. So the veil goes
/// whichever way the ground can still travel, and then only as far as it has
/// to — the smallest alpha that reaches [`SCRIM_TARGET`], so a pale desk gets
/// a firm dim and a dark one gets a light haze and both read as the same
/// gesture.
fn veil_of(face: Color32) -> (Color32, f32) {
    let mut best: Option<(Color32, f32)> = None;
    let mut fallback = (Color32::BLACK, 0.0_f32);
    for veil in [Color32::BLACK, Color32::WHITE] {
        for step in 0..=35_u8 {
            let a = SCRIM_MIN_A + f32::from(step) * 0.02;
            if a > SCRIM_MAX_A {
                break;
            }
            if contrast_ratio(over(face, veil, a), face) >= SCRIM_TARGET {
                if best.is_none_or(|(_, best_a)| a < best_a) {
                    best = Some((veil, a));
                }
                break;
            }
        }
        // Kept in case neither direction reaches the target: a mid grey desk
        // can be veiled either way, but nothing stops a user theme picking a
        // colour where one direction runs out of room first.
        let r = contrast_ratio(over(face, veil, SCRIM_MAX_A), face);
        if r > fallback.1 {
            fallback = (veil, r);
        }
    }
    best.unwrap_or((fallback.0, SCRIM_MAX_A))
}

// ── the layout ──────────────────────────────────────────────────────────────

/// Air between the card and the edge of the window.
const MARGIN: f32 = 16.0;
/// The card's nominal width, and the narrowest it may be squeezed to before
/// the overlay gives up and shows the scrim alone. A plugin editor is a small
/// window and this has to be sane inside one.
const CARD_W: f32 = 300.0;
const CARD_MIN_W: f32 = 160.0;
/// The card's inner margin.
const PAD: f32 = 18.0;
/// The lattice band's height, the gap under it, and the two text rows.
const MARK_H: f32 = 38.0;
const GAP: f32 = 12.0;
const LABEL_H: f32 = 20.0;
const DETAIL_GAP: f32 = 4.0;
const DETAIL_H: f32 = 16.0;
/// The meter row: tall enough for the reading beside the bar, with the bar
/// itself centred in it.
const METER_ROW_H: f32 = 14.0;
const METER_H: f32 = 10.0;
/// The card's corner, and the meter's.
const CARD_R: f32 = 4.0;

/// Where every part of the overlay is.
///
/// ONE function, so the card, the mark, the two text rows and the meter cannot
/// drift apart — the rule `shell::chrome_layout` keeps for the window bar,
/// kept here for the same reason: what a test measures has to be what the
/// painter draws.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Layout {
    /// The whole window. The scrim covers it.
    pub scrim: Rect,
    pub card: Rect,
    /// The lattice band. `None` when the window is too short to spare it.
    pub mark: Option<Rect>,
    pub label: Rect,
    pub detail: Option<Rect>,
    /// The meter's track. `None` when the caller gave no progress at all.
    pub meter: Option<Rect>,
    /// Where "7 of 12" is written. `None` for an indeterminate meter, which is
    /// the other half of what makes it obviously not a measurement.
    pub reading: Option<Rect>,
}

/// Lay the overlay out over `rect`, or `None` when there is no room for a card
/// at all — in which case the caller still paints the scrim, because a dimmed
/// window is a truthful thing to show even when nothing will fit on it.
pub fn layout(rect: Rect, busy: &Busy) -> Option<Layout> {
    if !rect.is_positive() {
        return None;
    }
    let w = CARD_W.min(rect.width() - MARGIN * 2.0);
    if w < CARD_MIN_W {
        return None;
    }
    let inner = w - PAD * 2.0;

    let wants_detail = busy.detail.as_deref().is_some_and(|d| !d.trim().is_empty());
    let widest = busy.progress.and_then(Progress::widest);
    let wants_meter = busy.progress.is_some();

    // **Rows are given up in order of how much they carry.** The mark is
    // decoration; the detail is the second half of a sentence whose first half
    // is the label; the label and the meter are the message and are never
    // dropped. A short window loses the picture, not the point.
    let height = |mark: bool, detail: bool| {
        PAD * 2.0
            + LABEL_H
            + if mark { MARK_H + GAP } else { 0.0 }
            + if detail { DETAIL_GAP + DETAIL_H } else { 0.0 }
            + if wants_meter { GAP + METER_ROW_H } else { 0.0 }
    };
    let room = rect.height() - MARGIN * 2.0;
    let (mut want_mark, mut want_detail) = (true, wants_detail);
    if height(want_mark, want_detail) > room {
        want_mark = false;
    }
    if height(want_mark, want_detail) > room {
        want_detail = false;
    }
    if height(want_mark, want_detail) > room {
        return None;
    }

    let card = Rect::from_center_size(
        rect.center(),
        Vec2::new(w, height(want_mark, want_detail)),
    );
    let left = card.left() + PAD;
    let mut y = card.top() + PAD;
    let row = |y: f32, h: f32| Rect::from_min_size(Pos2::new(left, y), Vec2::new(inner, h));

    let mark = want_mark.then(|| {
        let r = row(y, MARK_H);
        y += MARK_H + GAP;
        r
    });
    let label = row(y, LABEL_H);
    y += LABEL_H;
    let detail = want_detail.then(|| {
        y += DETAIL_GAP;
        let r = row(y, DETAIL_H);
        y += DETAIL_H;
        r
    });
    let (meter, reading) = if wants_meter {
        y += GAP;
        let full = row(y, METER_ROW_H);
        let bar_y = full.center().y - METER_H * 0.5;
        match widest {
            // The digits take a fixed column at the right and the bar takes
            // the rest, so the bar does not change length as the number does —
            // the same reason the reading is sized on its widest form.
            Some(_) => {
                let rw = (inner * 0.28).clamp(44.0, 96.0).min(inner * 0.5);
                let bar = Rect::from_min_size(
                    Pos2::new(full.left(), bar_y),
                    Vec2::new((inner - rw - GAP).max(1.0), METER_H),
                );
                let read = Rect::from_min_max(Pos2::new(full.right() - rw, full.top()), full.max);
                (Some(bar), Some(read))
            }
            None => (
                Some(Rect::from_min_size(
                    Pos2::new(full.left(), bar_y),
                    Vec2::new(inner, METER_H),
                )),
                None,
            ),
        }
    } else {
        (None, None)
    };

    Some(Layout {
        scrim: rect,
        card,
        mark,
        label,
        detail,
        meter,
        reading,
    })
}

// ── the drawing ─────────────────────────────────────────────────────────────

/// Node radius and stub length as fractions of the lattice spacing.
///
/// `splash.rs`'s own figures, restated rather than shared: they are private
/// there, and the launch path is not worth editing for a decoration. They are
/// the theory band's numbers too, which is what makes all three drawings one
/// lattice at three moments rather than three things that resemble each other.
const NODE_R: f32 = 0.30;
const STUB: f32 = 0.62;

/// How long the indeterminate hatch takes to travel one period, in seconds.
/// Slow: it is a texture that happens to drift, not a thing to be watched.
const HATCH_PERIOD: f64 = 1.6;
/// The hatch's spacing, in points.
const HATCH_STEP: f32 = 9.0;

/// Draw the overlay over `rect`, on a desk whose face is `face`.
///
/// `time` is the host's running clock in seconds (`ctx.input(|i| i.time)`) and
/// drives the only motion here. **Nothing depends on it being fresh**: the
/// hatch is correct at every phase, so a frozen frame — which is exactly what
/// a blocking operation leaves on the screen — is a correct drawing and not a
/// stopped animation.
pub fn draw(painter: &Painter, rect: Rect, face: Color32, busy: &Busy, time: f64) {
    if !rect.is_positive() {
        return;
    }
    let d = Dress::of(face);
    let p = painter.with_clip_rect(rect);
    p.rect_filled(rect, 0.0, d.scrim);

    let Some(l) = layout(rect, busy) else {
        return;
    };
    p.rect_filled(l.card, CARD_R, d.card);
    p.rect_stroke(
        l.card,
        CARD_R,
        Stroke::new(1.0_f32, d.edge),
        StrokeKind::Inside,
    );

    if let Some(mark) = l.mark {
        draw_mark(&p, mark, d.ink);
    }

    let label_font = FontId::new(14.0, fonts::courier_bold());
    p.with_clip_rect(l.label).text(
        l.label.center(),
        Align2::CENTER_CENTER,
        cut(&p, l.label.width(), &busy.label, &label_font),
        label_font.clone(),
        d.ink,
    );
    if let (Some(r), Some(text)) = (l.detail, busy.detail.as_deref()) {
        let font = FontId::new(11.0, fonts::courier());
        p.with_clip_rect(r).text(
            r.center(),
            Align2::CENTER_CENTER,
            cut(&p, r.width(), text, &font),
            font,
            d.faint,
        );
    }
    if let (Some(bar), Some(progress)) = (l.meter, busy.progress) {
        draw_meter(&p, &l, bar, &d, progress, time);
    }
}

/// One triangle of the Tonnetz, cropped by its band.
///
/// Three nodes and the lines out of all six lattice directions, with the ones
/// that lead nowhere running off the edge — the crop that makes it read as a
/// piece of an infinite lattice rather than as a triangle somebody drew.
fn draw_mark(painter: &Painter, rect: Rect, ink: Color32) {
    if !rect.is_positive() {
        return;
    }
    let a = (rect.height() * 0.66).min(rect.width() * 0.30);
    if a < 8.0 {
        return;
    }
    let h = a * 0.866_025_4; // equilateral: a * sqrt(3)/2
    let r = a * NODE_R;
    let c = rect.center();
    // Hung off the triangle's own centroid, so the band is balanced rather
    // than the top row being.
    let top = Pos2::new(c.x, c.y - h / 3.0);
    let nodes = [
        Pos2::new(top.x - a * 0.5, top.y),
        Pos2::new(top.x + a * 0.5, top.y),
        Pos2::new(top.x, top.y + h),
    ];
    let dirs = [
        Vec2::new(a, 0.0),
        Vec2::new(-a, 0.0),
        Vec2::new(a * 0.5, h),
        Vec2::new(-a * 0.5, h),
        Vec2::new(a * 0.5, -h),
        Vec2::new(-a * 0.5, -h),
    ];
    let p = painter.with_clip_rect(rect);
    let line = Stroke::new(1.0_f32, ink.gamma_multiply(0.45));
    for from in nodes {
        for dir in dirs {
            let to = from + dir;
            let real = nodes.iter().any(|n| (*n - to).length() < a * 0.25);
            if real {
                p.line_segment([from, to], line);
            } else {
                p.line_segment([from, from + dir * STUB], line);
            }
        }
    }
    // Nodes last, filled with the card under them, so the circles trim every
    // line back to their own edge without any arithmetic here — `splash.rs`'s
    // trick and the theory band's.
    for n in nodes {
        p.circle_stroke(n, r, Stroke::new(1.2_f32, ink));
    }
}

/// The meter: a filled bar with its reading, or a hatched track with neither.
fn draw_meter(painter: &Painter, l: &Layout, bar: Rect, d: &Dress, progress: Progress, time: f64) {
    painter.rect_filled(bar, 1.0, d.track);
    match progress.fraction() {
        Some(f) => {
            let filled = Rect::from_min_max(
                bar.min,
                Pos2::new(bar.left() + bar.width() * f, bar.bottom()),
            );
            if filled.width() > 0.5 {
                painter.rect_filled(filled, 1.0, d.fill);
            }
            if let (Some(r), Some(text), Some(widest)) =
                (l.reading, progress.reading(), progress.widest())
            {
                // Monospace, sized on the widest reading this operation can
                // ever produce: the cluster's rule, so the digits do not
                // resize under the eye as the count climbs.
                let font = FontId::new(fit_mono(r, &widest, 11.0), fonts::courier_bold());
                painter
                    .with_clip_rect(r)
                    .text(r.right_center(), Align2::RIGHT_CENTER, text, font, d.ink);
            }
        }
        None => {
            // **No leading edge.** An indeterminate bar with a chunk in it
            // claims a percentage the caller does not have — and claims a
            // different one depending on which frame it happened to freeze on.
            // An even hatch says "working" and says nothing else, at every
            // phase, including the one it stops on.
            let p = painter.with_clip_rect(bar);
            let h = bar.height();
            let drift = ((time / HATCH_PERIOD).rem_euclid(1.0) as f32) * HATCH_STEP;
            let n = ((bar.width() + h * 2.0) / HATCH_STEP).ceil().max(0.0) as i32;
            for k in 0..=n {
                let x = bar.left() - h + drift + k as f32 * HATCH_STEP;
                p.line_segment(
                    [Pos2::new(x, bar.bottom()), Pos2::new(x + h, bar.top())],
                    Stroke::new(1.5_f32, d.fill),
                );
            }
        }
    }
}

/// `text`, cut with an ellipsis to fit `width`.
///
/// The strings this draws are file names and plugin names: they come from
/// outside and have no length. Cut by MEASURING rather than by counting
/// characters, because the card's width is what they have to fit inside and a
/// character count is only a guess at that.
fn cut(painter: &Painter, width: f32, text: &str, font: &FontId) -> String {
    let measure = |t: &str| {
        painter
            .layout_no_wrap(t.to_owned(), font.clone(), Color32::WHITE)
            .rect
            .width()
    };
    if measure(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    for ch in text.chars() {
        out.push(ch);
        if measure(&format!("{out}\u{2026}")) > width {
            out.pop();
            break;
        }
    }
    format!("{out}\u{2026}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A desk face to draw against when the theme is not what is under test.
    const FACE: Color32 = Color32::from_rgb(0x3a, 0x2a, 0x2a);

    fn ctx() -> egui::Context {
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        ctx
    }

    /// It must draw at every size a window can be, including the degenerate
    /// ones and the small ones a plugin editor really is — an overlay that
    /// panics turns a slow operation into a crashed one.
    #[test]
    fn it_draws_at_every_size_without_panicking() {
        let ctx = ctx();
        let shapes = [
            None,
            Some(Progress::Indeterminate),
            Some(Progress::Fraction(0.0)),
            Some(Progress::Fraction(1.0)),
            Some(Progress::Steps { done: 7, total: 12 }),
            Some(Progress::Steps { done: 0, total: 0 }),
        ];
        for (w, h) in [
            (0.0_f32, 0.0_f32),
            (1.0, 1.0),
            (140.0, 90.0),
            (320.0, 120.0),
            (640.0, 480.0),
            (3840.0, 2160.0),
        ] {
            for shape in shapes {
                let mut busy = Busy::new("Loading project").with_detail("kick 07.wav");
                busy.progress = shape;
                let r = Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h));
                let _ = ctx.run(Default::default(), |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        for t in [0.0_f64, 0.4, 12.7] {
                            draw(ui.painter(), r, FACE, &busy, t);
                        }
                    });
                });
            }
        }
    }

    /// **One layout, every part.** The card, the mark, both text rows, the bar
    /// and the reading all come from `layout`, so what a test measures is what
    /// the painter draws — the rule `shell::chrome_layout` keeps, kept here.
    #[test]
    fn the_overlay_parts_come_from_one_layout() {
        let rect = Rect::from_min_size(Pos2::new(30.0, 20.0), egui::vec2(900.0, 620.0));
        let busy = Busy::new("Loading project")
            .with_detail("Serum")
            .with_progress(Progress::Steps { done: 7, total: 12 });
        let l = layout(rect, &busy).expect("a full-size window fits a card");

        assert_eq!(l.scrim, rect, "the scrim is the whole window");
        // Centred, and inside the window with its margin.
        assert!((l.card.center() - rect.center()).length() < 0.5, "{:?}", l.card);
        assert!(
            rect.shrink(MARGIN - 0.5).contains_rect(l.card),
            "the card {:?} escapes its margin in {rect:?}",
            l.card
        );

        let mark = l.mark.expect("a tall window keeps the mark");
        let detail = l.detail.expect("a detail line was given");
        let meter = l.meter.expect("a progress was given");
        let reading = l.reading.expect("k of n is a reading");

        // Every part inside the card, inset by the card's own padding.
        for r in [mark, l.label, detail, meter, reading] {
            assert!(l.card.contains_rect(r), "{r:?} escapes the card {:?}", l.card);
            assert!(r.left() >= l.card.left() + PAD - 0.01, "{r:?} breaks the pad");
            assert!(r.right() <= l.card.right() - PAD + 0.01, "{r:?} breaks the pad");
        }
        // The stack, in order, nothing overlapping.
        assert!(mark.bottom() <= l.label.top() + 0.01, "the mark runs into the label");
        assert!(l.label.bottom() <= detail.top() + 0.01, "the label runs into the detail");
        assert!(detail.bottom() <= meter.top() + 0.01, "the detail runs into the meter");
        assert!(mark.top() >= l.card.top() + PAD - 0.01);
        assert!(meter.bottom() <= l.card.bottom() - PAD + 0.01);
        // The bar and its digits share a row and share no pixels: a bar that
        // ran under the number would make the number unreadable at 100%.
        assert!(
            meter.right() <= reading.left() + 0.01,
            "the bar {meter:?} runs under the reading {reading:?}"
        );
        assert!((reading.right() - (l.card.right() - PAD)).abs() < 0.01);
        assert!((meter.height() - METER_H).abs() < 0.01);

        // A window with no room for a card lays out nothing, and the caller
        // still has a scrim to paint.
        assert!(layout(Rect::from_min_size(Pos2::ZERO, egui::vec2(120.0, 400.0)), &busy).is_none());
        assert!(layout(Rect::NOTHING, &busy).is_none());
    }

    /// Determinate, indeterminate and no-progress at all each lay out, and
    /// each says the right thing by what it OMITS: no meter without a
    /// progress, and no digits beside a meter that cannot measure.
    #[test]
    fn every_shape_of_progress_lays_out() {
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));

        let bare = Busy::new("Saving");
        let l = layout(rect, &bare).expect("fits");
        assert!(l.meter.is_none() && l.reading.is_none() && l.detail.is_none());

        let vague = Busy::new("Loading Serum").with_progress(Progress::Indeterminate);
        let l = layout(rect, &vague).expect("fits");
        let bar = l.meter.expect("an indeterminate meter still has a track");
        assert!(l.reading.is_none(), "an indeterminate meter claims no number");
        assert!(
            (bar.width() - (l.card.width() - PAD * 2.0)).abs() < 0.01,
            "with no digits beside it the track takes the whole row"
        );

        for p in [
            Progress::Fraction(0.42),
            Progress::Steps { done: 7, total: 12 },
        ] {
            let busy = Busy::new("Pasting 8 tracks").with_progress(p);
            let l = layout(rect, &busy).expect("fits");
            let bar = l.meter.expect("a determinate meter has a track");
            assert!(l.reading.is_some(), "a determinate meter shows its number");
            assert!(bar.width() < l.card.width() - PAD * 2.0);
        }

        // Steps that count nothing cannot measure, so they lay out as the
        // indeterminate case rather than dividing by zero.
        let none = Busy::new("Loading").with_progress(Progress::Steps { done: 0, total: 0 });
        assert!(layout(rect, &none).expect("fits").reading.is_none());

        // A short window gives up the mark, then the detail, and never the
        // label or the meter.
        let full = Busy::new("Loading project")
            .with_detail("Serum")
            .with_progress(Progress::Indeterminate);
        let short = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 150.0));
        let l = layout(short, &full).expect("a short window still fits the message");
        assert!(l.mark.is_none(), "the picture goes first");
        assert!(l.detail.is_some(), "and the detail outlives it");
        assert!(l.meter.is_some(), "the meter is the message and never goes");
        let shorter = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 120.0));
        let l = layout(shorter, &full).expect("fits");
        assert!(l.mark.is_none() && l.detail.is_none(), "the detail goes second");
        assert!(l.meter.is_some());
        // And below that there is no honest card at all, only the scrim.
        assert!(layout(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 100.0)), &full).is_none());
    }

    /// The readings, in the words a person reads them in.
    #[test]
    fn k_of_n_reads_as_words_and_a_fraction() {
        let steps = |done, total| Progress::Steps { done, total };
        assert_eq!(steps(7, 12).reading().as_deref(), Some("7 of 12"));
        assert_eq!(steps(0, 1).reading().as_deref(), Some("0 of 1"));
        // A caller's arithmetic bug is not something to show a user.
        assert_eq!(steps(13, 12).reading().as_deref(), Some("12 of 12"));
        assert!((steps(13, 12).fraction().expect("clamped") - 1.0).abs() < 1e-6);
        assert!((steps(3, 12).fraction().expect("known") - 0.25).abs() < 1e-6);
        // Nothing to count: no reading, no fraction, no panic.
        assert_eq!(steps(0, 0).reading(), None);
        assert_eq!(steps(0, 0).fraction(), None);
        assert_eq!(steps(0, 0).widest(), None);

        assert_eq!(Progress::Fraction(0.5).reading().as_deref(), Some("50%"));
        assert_eq!(Progress::Fraction(0.0).reading().as_deref(), Some("0%"));
        assert_eq!(Progress::Fraction(1.0).reading().as_deref(), Some("100%"));
        // Out of range, and not a number at all, both land somewhere sane.
        assert_eq!(Progress::Fraction(4.0).reading().as_deref(), Some("100%"));
        assert_eq!(Progress::Fraction(-1.0).reading().as_deref(), Some("0%"));
        assert_eq!(Progress::Fraction(f32::NAN).reading().as_deref(), Some("0%"));
        assert_eq!(Progress::Indeterminate.reading(), None);

        // The widest form is never narrower than the reading it sizes, which
        // is the whole reason it exists.
        assert_eq!(steps(3, 12).widest().as_deref(), Some("12 of 12"));
        assert_eq!(Progress::Fraction(0.1).widest().as_deref(), Some("100%"));
        for done in 0..=12 {
            let p = steps(done, 12);
            let (r, w) = (p.reading().expect("a reading"), p.widest().expect("a widest"));
            assert!(
                w.chars().count() >= r.chars().count(),
                "{r} is wider than the {w} the digits were sized on"
            );
        }
    }

    /// **The two timings, and the flicker each one forbids.**
    ///
    /// Pure arithmetic on purpose: the wiring cannot be asked to sleep, and
    /// this is the whole of what stops a 30 ms paste from strobing the screen.
    #[test]
    fn the_delay_and_the_minimum_forbid_a_flicker() {
        let ms = Duration::from_millis;
        let (d, m) = (SHOW_AFTER, MIN_VISIBLE);

        // Nothing is painted before the delay is up.
        assert!(!visible_after(ms(0), d));
        assert!(!visible_after(ms(119), d));
        assert!(visible_after(ms(120), d));
        // Work that beat the delay closes at once: it was never on screen, so
        // it owes the screen nothing.
        assert!(may_close_after(ms(0), d, m));
        assert!(may_close_after(ms(119), d, m));
        // Work that finished just after it appeared owes the full minimum from
        // the moment it appeared — not from the moment it started.
        assert!(!may_close_after(ms(120), d, m));
        assert!(!may_close_after(ms(519), d, m));
        assert!(may_close_after(ms(520), d, m));
        // A zero delay is up on the first painted frame and still owes the
        // minimum from that frame.
        assert!(visible_after(Duration::ZERO, Duration::ZERO));
        assert!(!may_close_after(ms(399), Duration::ZERO, m));
        assert!(may_close_after(m, Duration::ZERO, m));
        // A minimum of zero is a legal thing to ask for and closes at once.
        assert!(may_close_after(ms(0), Duration::ZERO, Duration::ZERO));

        // The two constructors carry the two settings, and nothing else.
        let now = Busy::new("Loading project");
        assert_eq!(now.delay, Duration::ZERO);
        assert_eq!(now.min_visible, MIN_VISIBLE);
        assert!(now.is_visible(), "a known block is up on the next frame");
        let later = Busy::deferred("Pasting 8 tracks");
        assert_eq!(later.delay, SHOW_AFTER);
        assert_eq!(later.min_visible, MIN_VISIBLE);
        assert_eq!(later.label, "Pasting 8 tracks");
        assert_eq!(
            Busy::new("x").with_delay(ms(50)).with_min_visible(ms(60)).delay,
            ms(50)
        );
    }

    /// **The sweep: every factory theme, both modes, on both faces a window
    /// can be handed** — the desk's strip and the band's ground. The label ink
    /// and the detail ink measured on the CARD they land on, the meter's fill
    /// on its own track, the card told apart from the dimmed desk, and the
    /// dimming itself measured against the desk it dims. The pattern
    /// `face::every_theme_dresses_a_face` and `shell::every_theme_dresses_the_chrome`
    /// set, for the same reason: every one of the colour bugs this app has
    /// shipped was found by a screenshot, and this is the screenshot as a test.
    #[test]
    fn every_theme_dresses_the_overlay() {
        let themes = crate::theme::factory();
        assert!(
            themes.len() >= 19,
            "the sweep expects the nineteen factory themes, found {}",
            themes.len()
        );
        let mut bad: Vec<String> = Vec::new();
        let mut swept = 0;
        let (mut darkened, mut lightened) = (0, 0);
        for theme in &themes {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                // The desk's face, exactly as `TangentApp::panel_face` resolves it.
                let strip = if dark {
                    set.dark_role("fx_strip").map_or_else(
                        || crate::mixer_panel::shade_face(set.fx_strip_color.to_color32(), 0.88),
                        |c| c.to_color32(),
                    )
                } else {
                    set.fx_strip_color.to_color32()
                };
                let band = crate::recorder_panel::palette(&set).bg;
                let mode = if dark { "dark" } else { "light" };
                for (which, face) in [("strip", strip), ("band", band)] {
                    let d = Dress::of(face);
                    if crate::recorder_panel::luminance(d.veiled) < crate::recorder_panel::luminance(face) {
                        darkened += 1;
                    } else {
                        lightened += 1;
                    }
                    let mut check = |what: &str, r: f32, floor: f32| {
                        if r < floor {
                            bad.push(format!("{} {mode} ({which}): {what} {r:.2}:1", theme.name));
                        }
                    };
                    check("the label on the card", contrast_ratio(d.ink, d.card), 4.5);
                    check("the detail on the card", contrast_ratio(d.faint, d.card), 4.5);
                    check("the fill on the track", contrast_ratio(d.fill, d.track), 3.0);
                    check("the track on the card", contrast_ratio(d.track, d.card), 1.1);
                    check("the card's edge", contrast_ratio(d.edge, d.card), 1.6);
                    // The card must stand off the desk it is floating over —
                    // the DIMMED desk, which is what is actually behind it.
                    check("the card on the dimmed desk", contrast_ratio(d.card, d.veiled), 1.5);
                    // And the dimming has to be a dimming. This is the one
                    // that a black scrim silently fails on every dark theme.
                    check("the scrim over the desk", contrast_ratio(d.veiled, face), 2.15);
                    // Translucent, always: the work stays visible under it.
                    if d.scrim.a() == 0 || d.scrim.a() == 255 {
                        bad.push(format!(
                            "{} {mode} ({which}): the scrim is not translucent (a={})",
                            theme.name,
                            d.scrim.a()
                        ));
                    }
                    swept += 1;
                }
            }
        }
        assert!(swept >= 19 * 2 * 2, "the sweep covered {swept} dressings");
        // **Both directions really happen.** The whole reason the veil is
        // measured rather than assumed is that the shipped themes need both:
        // the pale ones dim and the near-black ones haze. If this ever comes
        // out one-sided the measurement has collapsed into a constant and the
        // dark themes are back to a scrim nobody can see.
        assert!(
            darkened > 0 && lightened > 0,
            "the scrim went one way on every theme: {darkened} darkened, {lightened} lightened"
        );
        assert!(
            bad.is_empty(),
            "the busy overlay is unreadable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// A plugin name and a file path are strings from outside with no length,
    /// and the card is a fixed width. Both rows are cut to fit rather than
    /// running out over the scrim.
    #[test]
    fn a_long_label_and_a_long_detail_stay_inside_the_card() {
        let ctx = ctx();
        let long_label = "Loading a plugin with an unreasonably long name indeed, and then some";
        let long_detail =
            "/Users/somebody/Music/Samples/Big Library/Drums/Kicks/kick 07 processed final.wav";
        let busy = Busy::new(long_label)
            .with_detail(long_detail)
            .with_progress(Progress::Steps { done: 7, total: 12 });
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(900.0, 600.0));
        let l = layout(rect, &busy).expect("fits");
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                let p = ui.painter();
                let width = |t: &str, f: &FontId| {
                    p.layout_no_wrap(t.to_owned(), f.clone(), Color32::WHITE)
                        .rect
                        .width()
                };
                let label_font = FontId::new(14.0, fonts::courier_bold());
                let cut_label = cut(p, l.label.width(), &busy.label, &label_font);
                assert!(cut_label.ends_with('\u{2026}'), "{cut_label} was not cut");
                assert!(
                    width(&cut_label, &label_font) <= l.label.width() + 0.5,
                    "the label overflows its row"
                );
                let detail_font = FontId::new(11.0, fonts::courier());
                let detail = busy.detail.as_deref().expect("a detail");
                let r = l.detail.expect("a detail row");
                let cut_detail = cut(p, r.width(), detail, &detail_font);
                assert!(cut_detail.ends_with('\u{2026}'), "{cut_detail} was not cut");
                assert!(
                    width(&cut_detail, &detail_font) <= r.width() + 0.5,
                    "the detail overflows its row"
                );
                // A string that already fits is left alone — no ellipsis on a
                // name that never needed one.
                assert_eq!(cut(p, 400.0, "Serum", &detail_font), "Serum");
                // And the digits shrink to their column rather than escaping
                // it, which is what `fit_mono` is for.
                let read = l.reading.expect("a reading");
                let widest = busy.progress.and_then(Progress::widest).expect("a widest");
                let size = fit_mono(read, &widest, 11.0);
                let font = FontId::new(size, fonts::courier_bold());
                assert!(
                    width(&widest, &font) <= read.width() + 0.5,
                    "the widest reading overflows its column"
                );
                draw(p, rect, FACE, &busy, 0.0);
            });
        });
    }
}

