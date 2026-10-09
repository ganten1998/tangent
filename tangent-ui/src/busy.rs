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
//! # The two-phase sequence, which is the whole mechanism
//!
//! **The caller announces on one frame and acts on the next, and nothing here
//! is driven by a wall clock.** That follows straight from the law above: a
//! synchronous stall produces no frames, so a clock started at the moment the
//! overlay was created has advanced by exactly one frame interval when the
//! block begins and by the whole of the stall when it ends. An overlay that
//! decided whether to appear by asking that clock would decide it at a moment
//! when the answer is always "not yet", and would then be asked again only
//! after the operation it was drawn for was over. So the gate is
//! [`Busy::on_screen`], which is stamped by [`Busy::draw`] itself and means
//! what it says: the overlay has been painted, so the frame carrying it has
//! gone out, so the blocking call may now be made.
//!
//! ```ignore
//! // Frame N — announce. Nothing blocking happens on this pass.
//! self.busy = Some(Busy::new("Loading project").with_detail(name));
//! ctx.request_repaint();
//!
//! // Frame N+2, at the top — the announcement is on screen AND a frame has
//! // gone out that no longer carries whatever asked. See
//! // `Busy::outlived_the_asking_frame`, which is why this is not N+1.
//! if self.busy.as_ref().is_some_and(Busy::outlived_the_asking_frame) {
//!     self.open_project(&path);          // the block, in front of the user
//!     self.busy_done = true;
//! }
//!
//! // Every frame, at the BOTTOM, after the desk has drawn itself.
//! if let Some(busy) = &mut self.busy {
//!     let up = busy.draw(&painter, ctx.content_rect(), face, time);
//!     self.app.set_busy_up(up);          // the host's own flag; see below
//!     if self.busy_done && busy.may_close() {
//!         self.busy = None;
//!     } else {
//!         ctx.request_repaint();         // nothing else is asking for frames
//!     }
//! }
//! ```
//!
//! `reconcile_camera` in `desktop.rs` is the same handshake for the same
//! reason — "announce first, act next frame", around an `open_camera` that
//! blocks for up to four seconds — and `painted_once` there is the first-frame
//! form of it. This is that pattern with a picture attached.
//!
//! **It is a picture, and the host owns everything else.** It paints; it does
//! not consume input, and with a [`Painter`] it could not. Two duties come
//! with using it, both the host's:
//!
//! * Tell the app it is covered, the way `set_splash_up` does, so nothing
//!   underneath opens a dialog or takes a click while a cover is up.
//!   [`Busy::draw`] returns whether it painted precisely so there is a value
//!   to hand over.
//! * Swallow the frame's input for a CHUNKED operation, where frames really do
//!   arrive and clicks really do land on the desk underneath an overlay that
//!   says the app is busy. During a single block there is no input to swallow
//!   and a different hazard instead: the clicks queue in the window server and
//!   are delivered when the thread comes back, at whatever is under the
//!   pointer by then.
//!
//! **And it has a way out.** [`MAX_SILENT`] is the cap `splash.rs` carries as
//! `SPLASH_MAX`, for the reason stated there: a thing that never answers is
//! exactly the case where nobody may be left staring at a cover with no way
//! through. It is measured on silence rather than on total time, so a render
//! counting up may take all afternoon and a constructor that has said nothing
//! for twenty seconds gives the desk back.
//!
//! **And it does not stop the beachball.** Past roughly two seconds without a
//! run loop the macOS window server declares the process unresponsive and
//! paints the spinning wheel over whatever is there, this overlay included.
//! Nothing drawn from the blocked thread can prevent that, because the thread
//! that would draw it is the one that is gone. What this buys is that the
//! frozen frame underneath the wheel SAYS what is happening instead of being a
//! half-drawn desk. The only real fixes for a long block are the two that keep
//! frames coming — chunk the work so a slice runs per frame, or move it to a
//! worker and report from the UI thread — and both of those turn the overlay
//! into what it is best at, which is the case where it can actually animate.
//!
//! A pure painter over a given rect, like every other band in this crate: the
//! only state it keeps is when it was first painted and when it last had
//! something new to say, both stamped by its own [`Busy::draw`]. No viewport,
//! no window, nothing the plugin build cannot do.

use crate::face::fit_mono;
use crate::fonts;
use crate::recorder_panel::{
    contrast_ratio, distinct_surface_at, legible_surface, surface_of, Surface,
};
use egui::{Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::time::{Duration, Instant};

// ── which band a cover paints in ────────────────────────────────────────────

/// **The band every COVER of this app paints in, and the one place it is
/// decided.**
///
/// A cover is a drawing that says the app is working: this overlay, and
/// `splash.rs`'s wordmark at launch. Both used to name their own layer at
/// their own call site in `desktop.rs` — `LayerId::new(Order::Foreground,
/// Id::new("tangent-busy"))`, and the same line again with "tangent-splash" —
/// and both call sites carried a comment claiming the drawing therefore
/// covered "everything including any dialog that opened underneath it". It
/// did not, and the owner found it: "the export loading bar comes up behind
/// the export menu itself".
///
/// **What is above `Foreground`, which is what that claim missed.** egui's
/// bands are `Background < Middle < Foreground < Tooltip < Debug`, and paint
/// order is absolute across them. This app puts surfaces in three of them.
/// `shell::SurfaceTier::inline_order` is the table: a modal dialog and a
/// plugin window take `Foreground`, a modal's scrim takes `Middle`, and the
/// **export render window takes `Tooltip`** — one band ABOVE a cover, on
/// purpose, because nothing that merely announces may hide a render. `menu.rs`
/// opens its submenu panel in `Tooltip` for the same reason, and egui's own
/// hover text lands there whether we ask or not.
///
/// So a cover on `Foreground` was under the render window outright, and above
/// a dialog only by accident: a layer that is not an `Area` is missing from
/// `Areas::order`, and `GraphicLayers::drain` sweeps those up after the ones
/// that are — a branch egui itself annotates "we don't think we end up here in
/// normal situations", and one that iterates a hash map, so two covers would
/// have come out in whichever order the hasher felt like. An accident that
/// holds today is not a rule.
///
/// **Which windows have to be under it, and why the rule is here.** Every
/// surface this app can have open while it works: the export dialog and every
/// other modal question, a plugin's own face, the pedal window, the menu and
/// its submenu, the notification column (`notify` draws in `Foreground`
/// inside the main window), and the export render window that is up for the
/// whole of a bounce — `announce("Writing files", …)` is raised while that
/// window is still on screen, which is the pairing the owner was looking at.
/// A cover is the last thing painted before the UI thread stops answering, so
/// it is the one drawing in the app that must be above all of them; it is not
/// a question and nothing is waiting on it, so nothing is lost by it being
/// there. `Debug` is the only band above `Tooltip`, and the app paints nothing
/// else in it — egui's own debug drawing lands there only with the debug style
/// options on, which is a developer looking at a diagnostic and welcome to it.
///
/// The rule lives in `busy.rs` rather than at the call sites because that is
/// what "stated once" means: a host asks for a layer by name and cannot spell
/// a band at all, and `desktop.rs`'s own `a_cover_takes_its_band_from_one_place`
/// holds it to that. `shell.rs`'s
/// `a_cover_is_painted_above_every_surface_this_app_can_open` reads the answer
/// straight off `SurfaceTier`, so a fourth tier added there with a higher band
/// fails on the next `cargo test` rather than on somebody's screen.
pub const COVER_ORDER: egui::Order = egui::Order::Debug;

/// The layer a cover paints on, named for which cover it is.
///
/// `name` is the layer's identity and nothing else — `"tangent-busy"`,
/// `"tangent-splash"`. The BAND is [`COVER_ORDER`]'s to decide, which is the
/// whole point of asking through here.
pub fn cover_layer(name: &str) -> egui::LayerId {
    egui::LayerId::new(COVER_ORDER, egui::Id::new(name))
}

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
/// less to justify and should be gone sooner. Counted from the frame it was
/// PAINTED on and not from the one it was created on, which are the same frame
/// for a known block and are not for a [`Busy::deferred`] one.
pub const MIN_VISIBLE: Duration = Duration::from_millis(400);

/// How long [`Busy::deferred`] waits before showing itself at all.
///
/// The other half of the flicker problem, and it is only meaningful for an
/// operation that keeps painting: under this long nobody perceives a wait, so
/// nobody needs telling about one. **A delay cannot help a single blocking
/// call** — see the law at the top of the module and the note on
/// [`Busy::deferred`] — so this is a chunked operation's setting alone.
pub const SHOW_AFTER: Duration = Duration::from_millis(120);

/// How long an overlay may sit there with nothing new to say before it gives
/// the desk back.
///
/// `splash.rs` carries the same idea as `SPLASH_MAX` and states the reason
/// better than this could: "the cap is the important half — a device that
/// never answers is exactly the case where the user must not be left staring
/// at a wordmark with no way through". Measured here on SILENCE rather than on
/// total time, because unlike a launch these operations are not all short: an
/// offline render that is counting up is answering and may honestly take all
/// afternoon, while a plugin constructor that has said nothing for twenty
/// seconds is not coming back. What counts as speaking is a changed `progress`
/// or `detail`, noticed by [`Busy::draw`] rather than reported by the caller —
/// an overlay with neither can never speak, so for that one the cap runs from
/// the first paint, which is right: a static cover IS the case this guards.
///
/// **It is only an escape for the operations that can still paint.** A single
/// block that never returns takes the whole app with it and no overlay can
/// outlive the thread that draws it — the other half of why the honest answer
/// to a long block is a worker.
pub const MAX_SILENT: Duration = Duration::from_secs(20);

/// One thing the app is waiting on, and the timing that keeps it from
/// flickering.
///
/// **Which constructor, by the shape of the stall:**
///
/// * [`Busy::new`] — the caller KNOWS it is about to block, and blocks in one
///   go: opening a project, instantiating a plugin, decoding an import,
///   starting an offline render. It goes up on the frame BEFORE the call, and
///   [`Busy::on_screen`] is what says that frame has happened.
/// * [`Busy::deferred`] — an operation that comes back between chunks and is
///   usually over before anybody notices: an export rendering a slice per
///   frame, the insert loader taking a bay per frame, a multi-file decode.
///   Frames arrive, so the wall clock genuinely advances, so a delay can
///   genuinely hide the short ones.
///
/// There is deliberately no third constructor for "might be slow, one blocking
/// call", because there cannot be one: the only frame on which such an overlay
/// could be painted is the frame before the call, which is before anybody
/// knows whether it will be slow. A caller in that position decides from what
/// it is about to do — eight tracks announce, one does not — and a caller that
/// cannot decide should chunk the work instead.
///
/// The fields are public so a chunked operation can rewrite its `detail` and
/// `progress` each frame without rebuilding the struct; the stamps are not,
/// because the timing below is only honest if they are made by the paint
/// itself.
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
    /// How long to wait before appearing. Zero for a known block, which is the
    /// only honest figure there. See [`SHOW_AFTER`].
    pub delay: Duration,
    /// How long to stay once it has appeared. See [`MIN_VISIBLE`].
    pub min_visible: Duration,
    /// How long it may stay silent before it leaves. See [`MAX_SILENT`].
    pub cap: Duration,
    started: Instant,
    /// When [`Busy::draw`] first painted it, which is the only moment anybody
    /// can say it was on screen.
    shown: Option<Instant>,
    /// How many frames have actually carried it. See
    /// [`Busy::outlived_the_asking_frame`], which is the only reader and
    /// where the reason lives. Counted rather than timed, for the reason the
    /// module header gives about clocks during a stall.
    paints: u32,
    /// When it last said something new, and what that was — the cap measures
    /// the gap since.
    spoke_at: Option<Instant>,
    spoke_progress: Option<Progress>,
    spoke_detail: Option<String>,
}

impl Busy {
    /// An overlay for a stall the caller already knows it is about to cause.
    ///
    /// Announce it, paint it, and make the blocking call on the next frame,
    /// once [`Busy::on_screen`] says the announcement is really up.
    pub fn new(label: impl Into<String>) -> Self {
        Busy {
            label: label.into(),
            detail: None,
            progress: None,
            delay: Duration::ZERO,
            min_visible: MIN_VISIBLE,
            cap: MAX_SILENT,
            started: Instant::now(),
            shown: None,
            paints: 0,
            spoke_at: None,
            spoke_progress: None,
            spoke_detail: None,
        }
    }

    /// An overlay for a CHUNKED operation, which appears only if the operation
    /// is still going after [`SHOW_AFTER`].
    ///
    /// **Not for a single blocking call**, which is the one thing a delay
    /// cannot survive: the clock it is measured against stops for the whole of
    /// the block, so the overlay would be asked "are you due yet?" once, one
    /// frame after being made, and next asked after the operation was already
    /// over. Use [`Busy::new`] there and pay the frame.
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

    pub fn with_cap(mut self, cap: Duration) -> Self {
        self.cap = cap;
        self
    }

    /// How long since the operation was announced.
    ///
    /// Fine for a log line and for a chunked operation's own arithmetic. It is
    /// NOT what decides whether the overlay is up: see the module's law.
    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    /// **Is the announcement actually on the screen?**
    ///
    /// The first half of the two-phase gate. A caller with a blocking call
    /// waits for [`Busy::outlived_the_asking_frame`], which is this and one
    /// frame more; this one on its own answers the narrower question — has
    /// anything of this overlay ever been presented — which is what the cap,
    /// the minimum and the host's own "did it appear at all" branches ask.
    ///
    /// It goes true the moment [`Busy::draw`] paints, which is why WHERE the
    /// two are called from matters: `draw` belongs at the bottom of the frame,
    /// after the desk, and this belongs at the top of the next one. eframe
    /// presents when `update` returns, so a caller that keeps that order is
    /// asking about a frame that really has reached the screen — the same
    /// reasoning `painted_once` in `desktop.rs` is built on. Asking further
    /// down the SAME frame it was painted in would answer yes about pixels
    /// nobody has seen yet.
    ///
    /// A chunked caller does not gate its work on this; it does a slice every
    /// frame and lets the overlay appear when it is due.
    pub fn on_screen(&self) -> bool {
        self.shown.is_some()
    }

    /// **Has a frame gone out that no longer carries whatever ASKED for the
    /// work?**
    ///
    /// [`Busy::on_screen`] is true one frame too early for a blocking call,
    /// and the owner found the gap: "the export loading bar comes up behind
    /// the export menu itself". The Export dialog is a window of its own on
    /// the desktop, and the frame that announces the render is the same frame
    /// that dialog was last drawn in — the app closes it while that frame is
    /// being built, but a viewport is destroyed by NOT being drawn, which can
    /// only happen on the next pass. Block on the next pass instead and that
    /// pass never finishes, so the window is never destroyed, and the frozen
    /// picture the user stares at for the length of the render is the cover
    /// with the dialog still sitting on top of it.
    ///
    /// No layer can fix that: on the desktop the two are different OS
    /// windows, and [`COVER_ORDER`] only orders what is inside one of them.
    /// What fixes it is one more PRESENTED frame — the first one that is the
    /// cover and no longer the thing that asked — and this is the count of
    /// them. Two paints, because the first is by definition the announcing
    /// frame; nothing is timed, for the reason the module header gives about
    /// clocks during a stall.
    ///
    /// It costs one frame, once, in front of an operation that is about to
    /// take seconds — the whole reason a cover exists.
    ///
    /// **It cannot hang.** The host paints every frame the overlay is up and
    /// asks for the next one, so the second paint is the very next pass; the
    /// silence cap is measured in tens of seconds and cannot fire between
    /// two consecutive frames. The one case that stalls this — a window with
    /// no area at all, where nothing ever paints — stalls
    /// [`Busy::on_screen`] identically and is what `chore_ready`'s `None`
    /// arm is for.
    pub fn outlived_the_asking_frame(&self) -> bool {
        self.paints >= 2
    }

    /// How long it has been on screen, or `None` if it never has.
    pub fn shown_for(&self) -> Option<Duration> {
        self.shown.map(|t| t.elapsed())
    }

    /// How long since it last had something new to say, or `None` if it has
    /// never been painted.
    pub fn silent_for(&self) -> Option<Duration> {
        self.spoke_at.map(|t| t.elapsed())
    }

    /// Whether to paint it this frame. [`Busy::draw`] asks this itself, so a
    /// caller only needs it to decide something else.
    pub fn is_visible(&self) -> bool {
        visible_at(self.elapsed(), self.silent_for(), self.delay, self.cap)
    }

    /// Whether an overlay whose work has FINISHED may be dropped now.
    ///
    /// Ask this only once the operation is done; while it is still running the
    /// overlay stays up regardless of any of this.
    pub fn may_close(&self) -> bool {
        may_close_after(self.shown_for(), self.min_visible)
    }
}

/// Whether an overlay is on screen, given how long ago it was announced and
/// how long it has been silent.
///
/// Two gates and they answer different questions. The delay is the flicker
/// guard for a chunked operation and is measured from the announcement,
/// because that is when the work started. The cap is the escape and is
/// measured from the last thing the overlay had to say, because that is the
/// last evidence anything is still happening — `None` means it has never been
/// painted, so there is nothing to have gone quiet.
pub fn visible_at(
    elapsed: Duration,
    silent_for: Option<Duration>,
    delay: Duration,
    cap: Duration,
) -> bool {
    elapsed >= delay && !capped_after(silent_for, cap)
}

/// Whether an overlay that last spoke `silent_for` ago has outstayed its
/// welcome. See [`MAX_SILENT`].
pub fn capped_after(silent_for: Option<Duration>, cap: Duration) -> bool {
    silent_for.is_some_and(|quiet| quiet >= cap)
}

/// Whether a finished overlay may be taken down.
///
/// Two ways out, and they are not the same one. Either it never appeared — the
/// work beat the delay, which is the case the delay exists for, and an overlay
/// nobody saw owes the screen nothing — or it appeared and has served its
/// minimum from the frame it appeared ON. Counting from the paint rather than
/// from the announcement is what makes the minimum mean what it says: a
/// deferred overlay that went up 120 ms late would otherwise owe 120 ms less
/// than one that went up at once.
pub fn may_close_after(shown_for: Option<Duration>, min_visible: Duration) -> bool {
    shown_for.is_none_or(|up| up >= min_visible)
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
/// And how far the fill has to stand off that track, so the meter's two halves
/// are told apart at a glance. WCAG's figure for a graphical object, which is
/// what a bar is.
const FILL_FLOOR: f32 = 3.0;
/// How far the lattice's LINES have to stand off the card.
///
/// Lower than the ink's floor on purpose — they are a rule and not a word, and
/// the drawing wants them quieter than the nodes they join — but a floor all
/// the same. This was the one colour in the module picked by arithmetic
/// (`ink.gamma_multiply(0.45)`), which is not even the arithmetic it looked
/// like: `gamma_multiply` scales a PREMULTIPLIED colour, so what it returns is
/// a translucent ink whose measured luminance is not the luminance of anything
/// that ends up on screen. Measured on the card like everything else now.
const LATTICE_FLOOR: f32 = 2.0;

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
    /// The lattice's lines. The nodes are drawn in `ink`; these are the app's
    /// own hairline, so the mark is one drawing at two weights rather than a
    /// shape with an alpha on it. See [`LATTICE_FLOOR`].
    pub lattice: Color32,
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
            // The hairline `surface_of` already derives — ink stepped toward
            // the ground — held to its own floor in case a face ever puts it
            // too close to the card to see.
            lattice: distinct_surface_at(s.edge, card, LATTICE_FLOOR),
            track,
            // **The fill is ink, not a colour of its own.** A progress bar is
            // a reading and this app draws its readings in ink; a fifth
            // meaning of colour on a surface that already carries record red,
            // meter green and the channel palette would be one too many. It
            // still goes through the measurement, because the track it runs
            // along is a surface like any other.
            fill: distinct_surface_at(s.ink, track, FILL_FLOOR),
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
const CARD_W: f32 = 440.0;
const CARD_MIN_W: f32 = 160.0;
/// The card's inner margin.
const PAD: f32 = 24.0;
/// The lattice band's height, the gap under it, and the two text rows.
const MARK_H: f32 = 68.0;
const GAP: f32 = 16.0;
const LABEL_H: f32 = 28.0;
const DETAIL_GAP: f32 = 5.0;
const DETAIL_H: f32 = 21.0;
/// The meter row: tall enough for the reading beside the bar, with the bar
/// itself centred in it.
const METER_ROW_H: f32 = 19.0;
const METER_H: f32 = 13.0;
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

impl Busy {
    /// Draw the overlay over `rect`, on a desk whose face is `face`, and say
    /// whether anything was painted.
    ///
    /// **The visibility gate is in HERE and not at the call site**, because a
    /// gate a caller has to remember is a gate that gets copied without it —
    /// an overlay that paints unconditionally never hides, and the delay,
    /// the cap and every word about flicker above become decoration. Ask it
    /// every frame and let it decide.
    ///
    /// The answer is what the host hands to its own "something is covering the
    /// desk" flag — `set_splash_up`'s shape — because this paints and does
    /// nothing else; see the module doc on what the host still owes.
    ///
    /// It takes `&mut self` for the two stamps nothing else can make honestly:
    /// the first paint, which is what [`Busy::on_screen`] answers with, and the
    /// last time `progress` or `detail` changed, which is what [`MAX_SILENT`]
    /// measures its silence from. Both are facts about the PAINT, so the paint
    /// is what records them.
    ///
    /// `time` is the host's running clock in seconds (`ctx.input(|i| i.time)`)
    /// and drives the only motion here. **Nothing depends on it being fresh**:
    /// the hatch is correct at every phase, so a frozen frame — which is
    /// exactly what a blocking operation leaves on the screen — is a correct
    /// drawing and not a stopped animation.
    pub fn draw(&mut self, painter: &Painter, rect: Rect, face: Color32, time: f64) -> bool {
        if !rect.is_positive() || !self.is_visible() {
            return false;
        }
        let now = Instant::now();
        // Counted before anything can return early below, because this is the
        // number `outlived_the_asking_frame` reads and a paint that happened
        // is a paint that happened.
        self.paints = self.paints.saturating_add(1);
        if self.shown.is_none() {
            self.shown = Some(now);
            self.spoke_at = Some(now);
            self.spoke_progress = self.progress;
            self.spoke_detail.clone_from(&self.detail);
        } else if self.progress != self.spoke_progress || self.detail != self.spoke_detail {
            // Something new on screen is the only evidence available that the
            // operation is still alive, and it is evidence the caller cannot
            // forge by accident: it has to have changed what it is showing.
            self.spoke_progress = self.progress;
            self.spoke_detail.clone_from(&self.detail);
            self.spoke_at = Some(now);
        }
        paint(painter, rect, face, self, time);
        true
    }

    /// **The same cover, painted again somewhere else** — a child window
    /// the operating system keeps in front of the main one, where a cover
    /// painted in the main viewport alone lands BEHIND the window the user
    /// is looking at. Draws exactly what `draw` last drew and mutates
    /// nothing: `draw` keeps the clock, this is its mirror, and a mirror of
    /// a cover that is not yet showing shows nothing either.
    pub fn paint_over(&self, painter: &Painter, rect: Rect, face: Color32, time: f64) {
        if rect.is_positive() && self.is_visible() {
            paint(painter, rect, face, self, time);
        }
    }
}

/// The drawing itself, once the decision to draw has been made.
fn paint(painter: &Painter, rect: Rect, face: Color32, busy: &Busy, time: f64) {
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
        draw_mark(&p, mark, d.ink, d.lattice, d.card);
    }

    let label_font = FontId::new(19.0, fonts::courier_bold());
    p.with_clip_rect(l.label).text(
        l.label.center(),
        Align2::CENTER_CENTER,
        cut(&p, l.label.width(), &busy.label, &label_font),
        label_font.clone(),
        d.ink,
    );
    if let (Some(r), Some(text)) = (l.detail, busy.detail.as_deref()) {
        let font = FontId::new(14.5, fonts::courier());
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
///
/// Two inks, both measured on the card: `ink` for the nodes, `lattice` for the
/// lines, which is the app's own hairline and therefore a weight the theme
/// already decided rather than a number picked here.
fn draw_mark(painter: &Painter, rect: Rect, ink: Color32, lattice: Color32, card: Color32) {
    if !rect.is_positive() {
        return;
    }
    // **Sized from the whole drawing, stubs included, not from the triangle.**
    //
    // The lattice lines run PAST the nodes: a stub reaches `STUB` of a
    // spacing beyond the node it leaves, in six directions. So the mark is
    // wider and taller than the triangle that names it - about 2.24 spacings
    // across (half a spacing to a base node, plus a whole one along the
    // horizontal stub) and about 1.94 down (the apex's upward stub to the
    // base's downward one). Sizing `a` from the triangle alone asked for 45
    // points of height and drew 58, and `with_clip_rect` below took the
    // difference off the top and both sides - which is what the owner saw.
    const SPAN_W: f32 = 2.24;
    const SPAN_H: f32 = 1.94;
    let a = (rect.height() / SPAN_H).min(rect.width() / SPAN_W);
    if a < 8.0 {
        return;
    }
    let h = a * 0.866_025_4; // equilateral: a * sqrt(3)/2
    let r = a * NODE_R;
    let c = rect.center();
    // **Point UP, and the three nodes spell the name.** T at the apex, A at
    // the bottom left, N at the bottom right - the owner's arrangement, and
    // the reason the triangle points the other way from `splash.rs`'s pair of
    // rows. Hung off the triangle's own centroid (which sits a third of the
    // height above the base) so the band is balanced rather than the base
    // being.
    let base_y = c.y + h / 3.0;
    let nodes = [
        Pos2::new(c.x, base_y - h),
        Pos2::new(c.x - a * 0.5, base_y),
        Pos2::new(c.x + a * 0.5, base_y),
    ];
    const LETTERS: [char; 3] = ['T', 'A', 'N'];
    let dirs = [
        Vec2::new(a, 0.0),
        Vec2::new(-a, 0.0),
        Vec2::new(a * 0.5, h),
        Vec2::new(-a * 0.5, h),
        Vec2::new(a * 0.5, -h),
        Vec2::new(-a * 0.5, -h),
    ];
    let p = painter.with_clip_rect(rect);
    let line = Stroke::new(1.0_f32, lattice);
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
    // **Nodes last, and genuinely FILLED with the card** - which is what makes
    // the ring trim every lattice line back to its own edge with no arithmetic
    // here, `splash.rs`'s trick and the theory band's. The old code only
    // stroked, so the lines ran on through the middle of each node; harmless
    // when the node was empty, and impossible now that a letter lives there.
    //
    // The letter is sized from the node itself rather than from a constant, so
    // it stays inside the ring at every size the band can be squeezed to. 1.15
    // is the largest multiple of the radius at which a capital in this face
    // still clears the stroke on both sides.
    let glyph = FontId::new((r * 1.15).max(6.0), fonts::courier_bold());
    for (n, ch) in nodes.into_iter().zip(LETTERS) {
        p.circle_filled(n, r, card);
        p.circle_stroke(n, r, Stroke::new(1.2_f32, ink));
        p.text(n, Align2::CENTER_CENTER, ch, glyph.clone(), ink);
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
///
/// **Measured by halving, not by growing.** Adding one character at a time
/// laid out the string once per character kept, and this runs twice a frame on
/// the screen that is up because the machine is already struggling. A prefix of
/// a no-wrap line never gets narrower as it gets longer, so the fit is
/// monotonic and the same answer comes out of a halving search in
/// `log2(chars)` probes — seven for an eighty character path instead of the
/// forty-odd it used to take. The answer is the same one either way: the
/// LONGEST prefix that still fits with the ellipsis on the end. The bounds walk
/// char boundaries, so a multi-byte name is never sliced through the middle of
/// a character.
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
    // Where every character ENDS, so `ends[k - 1]` is the length of the first
    // `k` characters and `k == 0` is the empty prefix.
    let ends: Vec<usize> = text
        .char_indices()
        .skip(1)
        .map(|(i, _)| i)
        .chain(std::iter::once(text.len()))
        .collect();
    let with_ellipsis = |k: usize| {
        let cut_at = if k == 0 { 0 } else { ends[k - 1] };
        format!("{}\u{2026}", &text[..cut_at])
    };
    // `lo` fits, `hi` does not: the whole string is already too wide and an
    // ellipsis only makes it wider, so `hi` starts out true by the test above.
    // `lo` starts at the ellipsis alone, which is kept even when it does not
    // fit — a row too narrow for one glyph has nothing better to show, and the
    // painter clips it.
    let (mut lo, mut hi) = (0_usize, ends.len());
    while hi - lo > 1 {
        let mid = lo + (hi - lo) / 2;
        if measure(&with_ellipsis(mid)) <= width {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    with_ellipsis(lo)
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

    /// One frame, drawn and presented, answering what `draw` answered — the
    /// smallest form of the host's own loop.
    fn paint_once(ctx: &egui::Context, busy: &mut Busy, rect: Rect) -> bool {
        let mut painted = false;
        let _ = ctx.run(Default::default(), |ctx| {
            egui::CentralPanel::default().show(ctx, |ui| {
                painted = busy.draw(ui.painter(), rect, FACE, 0.0);
            });
        });
        painted
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
                            busy.draw(ui.painter(), r, FACE, t);
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

        // **A short window sheds in a fixed ORDER, and the order is the
        // contract - not the heights it happens to shed at.** This used to
        // assert at three hand-picked heights and broke the moment the card
        // was made bigger, which taught it nothing about the rule it was
        // guarding. So walk every height instead and check the sequence.
        let full = Busy::new("Loading project")
            .with_detail("Serum")
            .with_progress(Progress::Indeterminate);
        let (mut saw_mark, mut saw_detail, mut saw_gone) = (false, false, false);
        for h in (60..=400).rev() {
            let r = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, h as f32));
            let Some(l) = layout(r, &full) else {
                saw_gone = true;
                continue;
            };
            assert!(
                !saw_gone,
                "a card came back at {h} pt after the overlay had given up"
            );
            // The label and the meter are the message: neither may ever go
            // while there is a card at all.
            assert!(l.meter.is_some(), "the meter went at {h} pt");
            assert!(l.label.height() > 0.0, "the label went at {h} pt");
            if l.mark.is_some() {
                assert!(!saw_mark, "the picture came back at {h} pt");
                assert!(l.detail.is_some(), "the detail went before the picture");
            } else {
                saw_mark = true;
            }
            if l.detail.is_none() {
                saw_detail = true;
            } else {
                assert!(!saw_detail, "the detail came back at {h} pt");
            }
        }
        assert!(saw_mark, "the picture never went, so nothing was tested");
        assert!(saw_detail, "the detail never went");
        assert!(saw_gone, "the overlay never gave up, so its floor is untested");
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

    /// **The three timings, and what each one forbids.**
    ///
    /// Pure arithmetic on purpose: the wiring cannot be asked to sleep, and
    /// this is the whole of what stops a 30 ms paste from strobing the screen,
    /// what keeps a deferred overlay off the screen for an operation that
    /// beats it, and what gives the desk back when nothing ever answers.
    #[test]
    fn the_delay_and_the_minimum_forbid_a_flicker() {
        let ms = Duration::from_millis;
        let (d, m, cap) = (SHOW_AFTER, MIN_VISIBLE, MAX_SILENT);
        let due = |elapsed, silent| visible_at(elapsed, silent, d, cap);

        // Nothing is painted before the delay is up.
        assert!(!due(ms(0), None));
        assert!(!due(ms(119), None));
        assert!(due(ms(120), None));
        // Work that never appeared closes at once: it owes the screen nothing.
        assert!(may_close_after(None, m));
        // Work that appeared owes the full minimum FROM THE PAINT, not from
        // the moment it was announced — the two differ by the delay.
        assert!(!may_close_after(Some(ms(0)), m));
        assert!(!may_close_after(Some(ms(399)), m));
        assert!(may_close_after(Some(m), m));
        // A minimum of zero is a legal thing to ask for and closes at once.
        assert!(may_close_after(Some(ms(0)), Duration::ZERO));

        // **The cap, measured on silence.** An overlay that has never been
        // painted has not gone quiet; one that has been talking is answering;
        // one that has said nothing for the cap is not coming back, and the
        // desk goes back to whoever is sitting in front of it.
        assert!(!capped_after(None, cap));
        assert!(!capped_after(Some(cap - ms(1)), cap));
        assert!(capped_after(Some(cap), cap));
        assert!(due(Duration::from_secs(600), Some(ms(80))));
        assert!(!due(Duration::from_secs(600), Some(cap)));

        // The two constructors carry their settings, and nothing else.
        let now = Busy::new("Loading project");
        assert_eq!(now.delay, Duration::ZERO);
        assert_eq!(now.min_visible, MIN_VISIBLE);
        assert_eq!(now.cap, MAX_SILENT);
        assert!(now.is_visible(), "a known block is up on the next frame");
        assert!(!now.on_screen(), "and not before it has been painted");
        let later = Busy::deferred("Exporting");
        assert_eq!(later.delay, SHOW_AFTER);
        assert_eq!(later.min_visible, MIN_VISIBLE);
        assert_eq!(later.label, "Exporting");
        assert!(!later.is_visible(), "a deferred overlay waits out its delay");
        let tuned = Busy::new("x")
            .with_delay(ms(50))
            .with_min_visible(ms(60))
            .with_cap(Duration::from_secs(600));
        assert_eq!((tuned.delay, tuned.min_visible, tuned.cap), (ms(50), ms(60), Duration::from_secs(600)));
    }

    /// **The handshake: painted first, blocked second.**
    ///
    /// The law the module is built on, as a test. A caller with one blocking
    /// call announces on frame N and acts two frames later, and the only
    /// honest signal that those frames happened is that the overlay was
    /// PAINTED — a wall clock cannot say so, because during the block there
    /// is no clock being read. `desktop.rs` keeps the same handshake around
    /// `open_camera` and the same first-frame gate in `painted_once`.
    ///
    /// **Why two and not one**, which is the half the owner reported: the
    /// announcing frame still carries whatever asked for the work, and on the
    /// desktop that is a window of its own which is destroyed by not being
    /// drawn — on the pass that blocks. See
    /// [`Busy::outlived_the_asking_frame`].
    ///
    /// Revert-check: make that method answer `self.paints >= 1` and the
    /// second assertion below goes red.
    #[test]
    fn the_block_waits_for_the_frame_that_announces_it() {
        let ctx = ctx();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));

        let mut busy = Busy::new("Loading project");
        assert!(!busy.on_screen(), "nothing has been painted, so nothing may block");
        assert!(busy.shown_for().is_none() && busy.silent_for().is_none());
        assert!(paint_once(&ctx, &mut busy, rect), "a known block paints at once");
        assert!(busy.on_screen(), "the announcement is up");
        assert!(
            !busy.outlived_the_asking_frame(),
            "the frame that announced the block also carried the window that \
             asked for it, so the block may not run yet"
        );
        assert!(paint_once(&ctx, &mut busy, rect), "still up on the next frame");
        assert!(
            busy.outlived_the_asking_frame(),
            "a frame has gone out that is the cover and nothing else; now the \
             block may run"
        );
        assert!(
            !busy.may_close(),
            "an operation that turned out to be instant still owes its minimum"
        );

        // A deferred overlay is not due, paints nothing, says nothing was
        // painted, and — never having been seen — closes the moment its work
        // is done.
        let mut later = Busy::deferred("Exporting");
        assert!(!paint_once(&ctx, &mut later, rect));
        assert!(!later.on_screen());
        assert!(!later.outlived_the_asking_frame());
        assert!(later.may_close());
    }

    /// **An overlay that is not due paints NOTHING.**
    ///
    /// The gate lives inside `draw` rather than at the call site, so this
    /// counts the shapes that reach the screen rather than trusting the
    /// return value: a caller that copied a snippet without a guard would
    /// otherwise get an overlay that never hides, and every word about
    /// flicker in this module would be decoration.
    #[test]
    fn an_overlay_that_is_not_due_paints_nothing() {
        let ctx = ctx();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let shapes = |busy: Option<&mut Busy>| {
            let mut busy = busy;
            ctx.run(Default::default(), |ctx| {
                let p = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    egui::Id::new("busy-under-test"),
                ));
                if let Some(b) = busy.as_deref_mut() {
                    b.draw(&p, rect, FACE, 0.0);
                }
            })
            .shapes
            .len()
        };
        let bare = shapes(None);
        let mut not_due = Busy::deferred("Pasting 8 tracks");
        assert_eq!(
            shapes(Some(&mut not_due)),
            bare,
            "an overlay before its delay put shapes on the screen"
        );
        let mut capped = Busy::new("Loading Serum").with_cap(Duration::ZERO);
        assert!(paint_once(&ctx, &mut capped, rect), "the first paint is what starts the clock");
        assert_eq!(
            shapes(Some(&mut capped)),
            bare,
            "a capped overlay went on owning the screen"
        );
        let mut due = Busy::new("Loading project");
        assert!(shapes(Some(&mut due)) > bare, "a due overlay drew nothing");
    }

    /// **Speaking is what resets the cap, and only speaking.**
    ///
    /// A changed count or a changed file name is evidence the operation is
    /// alive; repainting the same frame is not, or the cap would never fire on
    /// the exact case it exists for — an overlay frozen at "3 of 12" forever.
    #[test]
    fn an_overlay_that_keeps_talking_is_never_capped() {
        let ctx = ctx();
        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut busy = Busy::new("Decoding").with_progress(Progress::Steps { done: 0, total: 9 });
        assert!(paint_once(&ctx, &mut busy, rect));
        assert_eq!(busy.spoke_progress, Some(Progress::Steps { done: 0, total: 9 }));
        let first = busy.spoke_at.expect("the first paint stamps it");

        busy.progress = Some(Progress::Steps { done: 1, total: 9 });
        busy.detail = Some("hat 3.wav".to_owned());
        assert!(paint_once(&ctx, &mut busy, rect));
        let spoke = busy.spoke_at.expect("news moves the stamp");
        assert!(spoke >= first);
        assert_eq!(busy.spoke_progress, Some(Progress::Steps { done: 1, total: 9 }));
        assert_eq!(busy.spoke_detail.as_deref(), Some("hat 3.wav"));

        assert!(paint_once(&ctx, &mut busy, rect));
        assert_eq!(
            busy.spoke_at,
            Some(spoke),
            "repainting the same thing counted as saying something"
        );
    }

    /// **The sweep: every factory theme, both modes, on both faces a window
    /// can be handed** — the desk's strip and the band's ground. The label ink
    /// and the detail ink measured on the CARD they land on, the meter's fill
    /// on its own track, the card told apart from the dimmed desk, and the
    /// dimming itself measured against the desk it dims.
    ///
    /// **Every floor here is the module's OWN constant**, not a rounder number
    /// underneath it. A sweep that asserts 1.1 where the code promises 1.35
    /// passes while two thirds of the promise has already been lost, which is
    /// the sort of green that costs a screenshot to discover. The pattern
    /// `face::every_theme_dresses_a_face` and `shell::every_theme_dresses_the_chrome`
    /// set, for the same reason: every one of the colour bugs this app has
    /// shipped was found by a screenshot, and this is the screenshot as a test.
    #[test]
    fn every_theme_dresses_the_overlay() {
        let themes = crate::theme::factory();
        assert!(
            themes.len() >= 32,
            "the sweep expects the thirty-two factory themes, found {}",
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
                    check("the fill on the track", contrast_ratio(d.fill, d.track), FILL_FLOOR);
                    check("the track on the card", contrast_ratio(d.track, d.card), TRACK_FLOOR);
                    check("the card's edge", contrast_ratio(d.edge, d.card), 1.6);
                    // The lattice's LINES, which used to be the one colour
                    // here picked by arithmetic instead of measured.
                    check("the lattice on the card", contrast_ratio(d.lattice, d.card), LATTICE_FLOOR);
                    // The card must stand off the desk it is floating over —
                    // the DIMMED desk, which is what is actually behind it.
                    check("the card on the dimmed desk", contrast_ratio(d.card, d.veiled), CARD_FLOOR);
                    // And the dimming has to be a dimming. This is the one
                    // that a black scrim silently fails on every dark theme.
                    check("the scrim over the desk", contrast_ratio(d.veiled, face), SCRIM_TARGET);
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
        assert!(swept >= 32 * 2 * 2, "the sweep covered {swept} dressings");
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
                // **The cut is the LONGEST prefix that fits**, which is
                // the property the halving search has to keep and the one a
                // broken bound would quietly lose: one more character must
                // overflow. Asserted rather than assumed, because the cheap
                // wrong answer here looks exactly like the right one.
                let kept = cut_label.trim_end_matches('\u{2026}');
                let next = busy.label[kept.len()..].chars().next().expect("it was cut");
                assert!(
                    width(&format!("{kept}{next}\u{2026}"), &label_font) > l.label.width(),
                    "the label was cut shorter than it had to be"
                );
            });
        });
    }
}


#[cfg(test)]
mod shot {
    use super::*;

    /// **Render the overlay to a file, so a human can look at it.**
    ///
    /// Not a test of anything: an eye. The band is a pure painter, so the real
    /// drawing code can be tessellated and rasterised here with no window and
    /// no GPU — every triangle egui would have sent to the card, including the
    /// text, which arrives as triangles sampling the font atlas like anything
    /// else. Writes a BMP because that needs no encoder; convert with `sips`.
    ///
    ///     cargo test -p tangent-ui --lib shot -- --ignored --nocapture
    #[test]
    #[ignore]
    fn draw_it() {
        const SCALE: f32 = 2.0;
        const W: usize = 1200;
        const H: usize = 520;
        let ctx = egui::Context::default();
        fonts::install(&ctx, fonts::FontChoice::default(), None);
        // Lay out AND build the font atlas at the render scale, so a glyph is
        // rasterised at the size it is shown rather than magnified.
        ctx.set_pixels_per_point(SCALE);

        let rect = Rect::from_min_size(Pos2::ZERO, egui::vec2(W as f32 / SCALE, H as f32 / SCALE));
        let mut busy = Busy::new("Loading project").with_detail("Serum - 3 of 8");
        busy = busy.with_progress(Progress::Steps { done: 3, total: 8 });

        // **Two passes, and the atlas is accumulated across both.** The
        // first builds it; the second returns only a PARTIAL delta (a patch at
        // an offset), so reading the atlas from the second pass alone gives a
        // fragment and every glyph then samples its neighbour. That is what
        // made the first version of this render text as blobs.
        let mut atlas: Option<(usize, usize, Vec<f32>)> = None;
        let soak = |d: &egui::TexturesDelta, atlas: &mut Option<(usize, usize, Vec<f32>)>| {
            for (id, img) in &d.set {
                if *id != egui::TextureId::Managed(0) {
                    continue;
                }
                let egui::ImageData::Color(c) = &img.image;
                let cov: Vec<f32> = c.pixels.iter().map(|p| f32::from(p.a()) / 255.0).collect();
                match (img.pos, atlas.as_mut()) {
                    // A patch into the atlas we already hold.
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
                    // A whole new atlas.
                    _ => *atlas = Some((c.size[0], c.size[1], cov)),
                }
            }
        };
        let first = ctx.run(Default::default(), |_| {});
        soak(&first.textures_delta, &mut atlas);
        let out = ctx.run(Default::default(), |c| {
            let p = Painter::new(c.clone(), egui::LayerId::debug(), rect);
            // A plausible desk under the scrim, so the veil reads honestly.
            p.rect_filled(rect, 0.0, Color32::from_rgb(0x2b, 0x2f, 0x35));
            for i in 0..14 {
                let y = 18.0 + i as f32 * 26.0;
                p.rect_filled(
                    Rect::from_min_size(Pos2::new(24.0, y), egui::vec2(W as f32 / SCALE - 48.0, 16.0)),
                    2.0,
                    Color32::from_rgb(0x3a, 0x3f, 0x47),
                );
            }
            busy.draw(&p, rect, Color32::from_rgb(0x2b, 0x2f, 0x35), 0.0);
        });

        soak(&out.textures_delta, &mut atlas);
        let prims = ctx.tessellate(out.shapes, SCALE);
        let mut buf = vec![[0u8; 3]; W * H];
        for prim in &prims {
            let egui::epaint::Primitive::Mesh(m) = &prim.primitive else { continue };
            for tri in m.indices.chunks_exact(3) {
                let v: Vec<_> = tri.iter().map(|&i| m.vertices[i as usize]).collect();
                let (x0, x1) = (v.iter().map(|p| p.pos.x).fold(f32::MAX, f32::min),
                                v.iter().map(|p| p.pos.x).fold(f32::MIN, f32::max));
                let (y0, y1) = (v.iter().map(|p| p.pos.y).fold(f32::MAX, f32::min),
                                v.iter().map(|p| p.pos.y).fold(f32::MIN, f32::max));
                let area = |a: egui::epaint::Vertex, b: egui::epaint::Vertex, px: f32, py: f32| {
                    (b.pos.x - a.pos.x) * (py - a.pos.y) - (b.pos.y - a.pos.y) * (px - a.pos.x)
                };
                let denom = area(v[0], v[1], v[2].pos.x, v[2].pos.y);
                if denom.abs() < 1e-9 { continue; }
                for py in (y0.floor().max(0.0) as usize)..=(y1.ceil().min(H as f32 - 1.0) as usize) {
                    for px in (x0.floor().max(0.0) as usize)..=(x1.ceil().min(W as f32 - 1.0) as usize) {
                        let (fx, fy) = (px as f32 + 0.5, py as f32 + 0.5);
                        let w0 = area(v[1], v[2], fx, fy) / denom;
                        let w1 = area(v[2], v[0], fx, fy) / denom;
                        let w2 = area(v[0], v[1], fx, fy) / denom;
                        if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 { continue; }
                        let col = |f: fn(&egui::epaint::Vertex) -> f32| w0 * f(&v[0]) + w1 * f(&v[1]) + w2 * f(&v[2]);
                        let (r, g, b, mut a) = (
                            col(|p| p.color.r() as f32), col(|p| p.color.g() as f32),
                            col(|p| p.color.b() as f32), col(|p| p.color.a() as f32) / 255.0);
                        // Text and shapes both arrive as textured triangles; a
                        // shape samples the atlas's white pixel, text samples a glyph.
                        // WHITE_UV is (0,0): a plain shape, not a glyph.
                        let is_glyph = col(|p| p.uv.x) > 1e-4 || col(|p| p.uv.y) > 1e-4;
                        if let (true, Some((aw, ah, ref px_a))) = (is_glyph, atlas.as_ref().map(|a| (a.0, a.1, &a.2))) {
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
                        if a <= 0.002 { continue; }
                        let d = &mut buf[py * W + px];
                        for (i, c) in [r, g, b].into_iter().enumerate() {
                            d[i] = (c * a + d[i] as f32 * (1.0 - a)).round().clamp(0.0, 255.0) as u8;
                        }
                    }
                }
            }
        }
        // BMP: 24-bit, bottom-up, rows padded to 4 bytes.
        let pad = (4 - (W * 3) % 4) % 4;
        let size = 54 + (W * 3 + pad) * H;
        let mut f = Vec::with_capacity(size);
        f.extend_from_slice(b"BM");
        f.extend_from_slice(&(size as u32).to_le_bytes());
        f.extend_from_slice(&0u32.to_le_bytes());
        f.extend_from_slice(&54u32.to_le_bytes());
        f.extend_from_slice(&40u32.to_le_bytes());
        f.extend_from_slice(&(W as i32).to_le_bytes());
        f.extend_from_slice(&(H as i32).to_le_bytes());
        f.extend_from_slice(&1u16.to_le_bytes());
        f.extend_from_slice(&24u16.to_le_bytes());
        f.extend_from_slice(&[0u8; 24]);
        for y in (0..H).rev() {
            for x in 0..W {
                let p = buf[y * W + x];
                f.extend_from_slice(&[p[2], p[1], p[0]]);
            }
            f.extend(std::iter::repeat(0u8).take(pad));
        }
        let path = std::env::var("SHOT").unwrap_or_else(|_| "/tmp/busy.bmp".into());
        std::fs::write(&path, f).unwrap();
        println!("wrote {path}");
    }
}
