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
//! **One Area per BOX, not one for the column.** An Area's layer is a
//! rectangle and `Context::layer_id_at` hands the pointer to the topmost layer
//! whose rectangle covers it — so a single Area sized to the whole column owns
//! the six-point gaps between the boxes as surely as it owns the boxes, and a
//! click aimed at the arrangement through one of those gaps lands nowhere at
//! all. The column is therefore four small layers with air between them.
//!
//! **The clock is a parameter.** `tangent-ui` has no engine dependencies and
//! nothing here may reach for wall time, so ageing is driven by egui's own
//! `ctx.input(|i| i.time)` — monotonic, supplied by the frame — handed in from
//! outside. That is also what makes the whole module testable headless: a test
//! advances the clock by hand and never opens a window.
//!
//! # Three shapes, one queue
//!
//! The app's messages are not all the same animal, and a queue that only knows
//! the first one serves the other two by accident:
//!
//! - **Terminal.** "Exported to ~/Music", "Pasted 3 tracks". It happened, it is
//!   over, it appears and ages out. [`Notifications::push`] and its three
//!   sugar methods.
//! - **Level-triggered.** "No MIDI input ports found", "the input device
//!   disappeared". Not an event: a CONDITION, true until it stops being true,
//!   and re-checked every frame by a caller that has no idea whether it has
//!   said this before. [`Notifications::raise`] and [`Notifications::lower`],
//!   with [`Notifications::insist`] for the caller that is not re-checking at
//!   all — a menu row, a button — and must be answered the once it asks.
//! - **Progressive.** "Scanning plugins... 40 of 112" — one message whose TEXT
//!   changes. [`Notifications::update`], or simply raising the same key again
//!   with new words.
//!
//! **A condition is raised once per episode.** `raise` remembers the key, so
//! re-raising a standing condition hands back the same handle and changes
//! nothing but the words: it cannot stack copies, it cannot restart a clock,
//! and — the one that matters — it cannot resurrect a message the user has
//! DISMISSED. Clicking "the input device disappeared" away means it is gone
//! until the device comes back and the caller
//! [`lower`](Notifications::lower)s the condition; without that memory the
//! message reappears on the very next frame and the click reads as broken.
//!
//! **Unless the user ASKS again, which is not the same as the app repeating
//! itself.** A dismissal answers one asking of the question, and both MIDI
//! conditions are asked by a press rather than by a per-frame check — so
//! choosing MIDI Input from the menu after dismissing "No MIDI input ports
//! found" got no answer of any kind, for the rest of the session. That door
//! is [`Notifications::insist`]: same key, same standing message, but the
//! mute goes and the words appear, because a request the user made by hand
//! has to produce something.
//!
//! **But only the USER's click means that.** The other way a standing message
//! can leave is the cap taking its slot for a newer arrival, and for a while
//! the queue could not tell the two apart: a dead input device evicted by four
//! receipts was silenced for the rest of the session, which is the one thing a
//! fault message must never be. A dismissal is remembered; an eviction is not,
//! and the condition puts itself back the moment there is room.
//!
//! **And that is the answer to "a message with no natural expiry".** A raised
//! message is STANDING: it has no clock at all and leaves only when it is
//! dismissed, when its condition is lowered, or when the cap needs its slot.
//! There is deliberately no `Kind` or lifetime that means "forever", because a
//! message with no clock and no owner is one nothing in the app can ever take
//! down. The no-expiry lifetime exists, and the only door to it is a condition
//! that can also end it.
//!
//! Two ways out, because the app has two kinds of caller. [`show`] does the
//! whole job against a `Context` (its own Areas, its own hit test, its own
//! click-eating); [`draw`] is the pure painter for a surface that already owns
//! a `Painter` and a rect, and hands back where every box landed so the caller
//! can do the hit test itself. Neither asks for a repaint — see [`Shown`].

use crate::fonts;
use crate::recorder_panel::{
    contrast_ratio, distinct_surface_at, surface_of, wants_light_ink, REC_RED,
};
use egui::{Color32, CornerRadius, FontId, Painter, Pos2, Rect, Stroke, StrokeKind, Vec2};
use std::collections::BTreeSet;
use std::sync::Arc;

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

/// How long an ANSWER stays up — the explanatory message a press asks for and
/// nothing re-checks. See [`Notifications::insist`].
///
/// **These used to stand for ever**, because `insist` borrowed `raise`'s note
/// and a raised note has no clock: it is a CONDITION, and a condition ends
/// when it stops being true. An answer is not a condition. Nothing was ever
/// going to lower "That is a MIDI part, not a sample", so it sat in the
/// corner until somebody clicked it — which is one more thing to do in the
/// middle of whatever the message interrupted. The owner, testing: "Make the
/// helper notifs also fade on their own after a set interval. These interfere
/// with testing."
///
/// Ten rather than the error's five: these are the app's longest messages, two
/// sentences of explanation rather than a receipt, and a reader who has just
/// been told why the thing they tried did not work deserves to finish the
/// sentence. Asking again restarts it.
pub const LIFE_ANSWER_S: f64 = 10.0;

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

/// The narrowest box worth drawing, and therefore the narrowest window that
/// gets a column at all.
///
/// **Squeezing is not the same as hanging off the edge.** The column is
/// squeezed toward this on a small editor — a plugin window can be 240 points
/// wide — but a window with less than `MIN_WIDTH + 2 * MARGIN` of room has no
/// box that fits inside it, and the honest answer there is no box: one that
/// runs off the right edge is a message the user can only half read, over the
/// controls it was covering.
pub const MIN_WIDTH: f32 = 160.0;

/// The narrowest window that gets a column.
pub const MIN_WINDOW_W: f32 = MIN_WIDTH + MARGIN * 2.0;

/// A point the top box in `area` is certain to cover.
///
/// **The caller has to know what the column will land ON**, because the card
/// is derived from that face and there is no one answer — the record band in
/// the ordinary view, the desk's strip or the arrangement's lane field in a
/// DAW view, the keyboard's own ground where a letterboxed window shows it.
/// Working that out means hit-testing the app's own bands against a point,
/// and the point has to be the one this module actually paints over or the
/// card is dressed for a surface it is not on. So it is answered here, from
/// the same [`MARGIN`] `plan` lays out with, rather than being a `10.0`
/// written down twice.
pub fn probe(area: Rect) -> Pos2 {
    area.min + Vec2::splat(MARGIN + 1.0)
}

const TEXT_PT: f32 = 11.0;

/// The most rows a box will grow to.
///
/// **Wrap and grow, then stop.** Several of the real messages are multi-line
/// and one of them is about ten; at this width that message is a box taller
/// than the piano band, in the corner, over the thing it is describing. So the
/// box grows to fit up to five rows and elides the rest with an ellipsis — and
/// [`Note::text`] still holds every word, so a caller that wants the whole
/// thing (a log, a notice, a copy button) has it. [`Placed::elided`] says when
/// that happened, which is the caller's cue that there is more to show, and
/// [`Shown::dismissed`] hands back the WHOLE note on the frame a click takes
/// it down — the elision would be worth nothing if the only copy of the full
/// text died with the box the user clicked.
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

/// How far into a `life`-second message `age` is: 1.0 whole, 0.0 gone.
///
/// A free function rather than only a method because this arithmetic is the
/// whole behaviour of the module, and a test should be able to ask it
/// directly, with no queue and no clock. It takes the life rather than the
/// kind because a message's life is no longer only its kind's: the cap can
/// shorten one, and [`Notifications::lower`] gives a standing message a tail
/// it never had.
///
/// **The tail's start is written as an ADDITION, and that is not a
/// stylistic choice.** [`Note::retire`] sets a life of `age + FADE_S`, and
/// asking `age <= life - FADE_S` back afterwards runs the subtraction on the
/// other side of the rounding: `(age + FADE_S) - FADE_S` comes out a hair
/// ABOVE `age` at most ages, so a message the cap had just retired reported
/// itself WHOLE for one more frame and five of them stood where the rule says
/// four. Written this way the comparison recomputes the very same expression
/// `retire` stored, with the same operands in the same order, so IEEE
/// arithmetic makes the two exactly equal and the boundary lands where it was
/// put. [`Note::leaving`] carries the mirror of this and must keep it.
pub fn fade_within(age: f64, life: f64) -> f32 {
    if age >= life {
        return 0.0;
    }
    if age + FADE_S <= life {
        return 1.0;
    }
    (((life - age) / FADE_S) as f32).clamp(0.0, 1.0)
}

/// [`fade_within`] for a message living its kind's own life.
pub fn fade_at(age: f64, kind: Kind) -> f32 {
    fade_within(age, kind.life())
}

// ── the queue ───────────────────────────────────────────────────────────────

/// A handle to one message, stable for its life.
///
/// Opaque: it is worth equality and nothing else, which is all a caller needs
/// to say "the click was for THAT one".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct NoteId(u64);

/// One message.
///
/// The fields that decide when it leaves are private, because they are an
/// invariant rather than a setting: a message's clock is set by the door it
/// came in through — [`Notifications::push`] or [`Notifications::raise`] — and
/// a caller that could reach in and blank a life would have the unkillable
/// message this module exists to prevent.
#[derive(Debug, Clone)]
pub struct Note {
    pub id: NoteId,
    pub kind: Kind,
    /// The message, WHOLE. The box may show less of it — see [`MAX_ROWS`].
    pub text: String,
    /// Seconds since this message last became new. Paused while the pointer is
    /// on it, so it does not vanish out from under somebody reading it.
    pub age: f64,
    /// Seconds it has been up in TOTAL, across every refresh. This is what
    /// stops a repeat from renewing a message forever — see [`Note::refresh`].
    stood: f64,
    /// The age it runs out at. `None` is STANDING: a message raised against a
    /// condition, which has no clock and leaves only when the condition is
    /// lowered, the user dismisses it, or the cap needs its slot.
    life: Option<f64>,
    /// The condition it was raised against, if it was raised at all.
    key: Option<String>,
}

impl Note {
    fn brief(id: NoteId, kind: Kind, text: String) -> Self {
        Self {
            id,
            kind,
            text,
            age: 0.0,
            stood: 0.0,
            life: Some(kind.life()),
            key: None,
        }
    }

    fn standing(id: NoteId, kind: Kind, text: String, key: String) -> Self {
        Self {
            id,
            kind,
            text,
            age: 0.0,
            stood: 0.0,
            life: None,
            key: Some(key),
        }
    }

    /// An ANSWER: a message keyed to a condition like a standing one, and
    /// given a clock like an ordinary one, because it is the reply to a press
    /// rather than a report of a state. See [`LIFE_ANSWER_S`].
    fn answered(id: NoteId, kind: Kind, text: String, key: String) -> Self {
        Self {
            life: Some(LIFE_ANSWER_S),
            ..Self::standing(id, kind, text, key)
        }
    }

    /// The age this message runs out at, or `None` if it is standing.
    pub fn expires_at(&self) -> Option<f64> {
        self.life
    }

    /// No natural expiry: raised against a condition, and it goes when the
    /// condition does.
    pub fn is_standing(&self) -> bool {
        self.life.is_none()
    }

    /// The condition this message was raised against.
    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    /// Total seconds up, across every refresh.
    pub fn stood(&self) -> f64 {
        self.stood
    }

    /// 1.0 whole, sliding to 0.0 through the tail. A standing message is
    /// always whole: it is not on its way anywhere.
    pub fn fade(&self) -> f32 {
        self.life.map_or(1.0, |life| fade_within(self.age, life))
    }

    /// Already in its tail: on its way out, and no longer counted against
    /// [`VISIBLE`].
    ///
    /// Inclusive at the boundary, because that boundary is a value [`cap`] sets
    /// EXACTLY when it retires a message early: at `>` a retired message tested
    /// as whole, so a burst of five stacked five and the cap did nothing.
    ///
    /// And written as `age + FADE_S >= life` rather than `age >= life -
    /// FADE_S`, which is the same inequality on paper and a different one in
    /// floats — see [`fade_within`], which carries the argument.
    ///
    /// [`cap`]: Notifications::cap
    pub fn leaving(&self) -> bool {
        self.life.is_some_and(|life| self.age + FADE_S >= life)
    }

    pub fn expired(&self) -> bool {
        self.life.is_some_and(|life| self.age >= life)
    }

    /// Send this message into its tail NOW, wherever it had got to.
    ///
    /// The one way a STANDING message can be made to leave: it is given the
    /// tail it never had, starting from the age it has already reached. Never
    /// rewound — `min` only ever shortens — so a message already inside its
    /// tail keeps the ending it was in the middle of.
    fn retire(&mut self) {
        let end = self.age + FADE_S;
        self.life = Some(self.life.map_or(end, |life| life.min(end)));
    }

    /// Put the clock back to the start — what an identical message arriving
    /// again does.
    ///
    /// **Bounded, or a repeat is an unkillable message.** The same failure
    /// arrives once per buffer until something changes, and a refresh that
    /// always took means the message never expired and the click that
    /// dismissed it was undone before the next frame drew. So a message may be
    /// refreshed during its FIRST life and no longer: past that a duplicate is
    /// ignored and it ages out on the clock it already has, which caps any
    /// repeated message at twice its life however long the fault goes on. A
    /// caller that wants the message to STAND while its condition holds says
    /// so with [`Notifications::raise`], which is the honest way to ask.
    fn refresh(&mut self) {
        if self.stood >= self.kind.life() {
            return;
        }
        self.renew();
    }

    /// The clock back to the start with NO bound, for a caller that holds the
    /// handle.
    ///
    /// **Owning the handle is the difference.** [`Self::refresh`] is bounded
    /// because its caller found this message by its TEXT and may have no idea
    /// it is already up — that is a duplicate arriving, and a duplicate must
    /// not be able to keep a message alive forever. An
    /// [`update`](Notifications::update) is the message's own author saying
    /// something new about a job still running, and a progress report for a
    /// scan that takes a minute would otherwise die forty seconds in with the
    /// scan still going. It cannot become the unkillable message either: a
    /// dismissal takes the note out of the queue entirely, and there is no
    /// lookup by id that can bring one back, so the very next update answers
    /// `false` and the caller has been told to stop.
    ///
    /// A standing message has no clock, so this is not something that can be
    /// done to it.
    fn renew(&mut self) {
        if self.life.is_none() {
            return;
        }
        self.age = 0.0;
        self.life = Some(self.kind.life());
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
    /// The conditions currently RAISED, whether or not their message is still
    /// on screen.
    ///
    /// It outlives the note, because a condition is true whether or not
    /// anything is being said about it. These are code-authored condition
    /// names, one per condition currently true, so the set is small and static
    /// in a way user text never would be. Only [`Self::lower`] — the condition
    /// genuinely ending — forgets a key; [`Self::insist`] adds one, because a
    /// user asking by hand is an episode of the condition too.
    raised: BTreeSet<String>,
    /// The conditions whose message the USER took down.
    ///
    /// **This is the memory that makes a dismissal stick, and it is a separate
    /// set for a reason that cost a real fault.** A standing message can leave
    /// the queue two ways: the user clicks it, or [`Self::cap`] retires it to
    /// keep the column to four. One of those is a decision and the other is
    /// housekeeping, and while `raised` alone carried both, a dead input
    /// device evicted by a burst of receipts went silent FOREVER — every later
    /// `raise` found the key standing, found no note to update, and answered
    /// `None` for the rest of the session. So only what the user chose lands
    /// here, and only that suppresses a re-raise; an eviction leaves the
    /// condition able to say itself again the moment there is room to. Cleared
    /// with the condition, so a fault that comes back after genuinely clearing
    /// is news again — and cleared by [`Self::insist`], because a user who
    /// asks the question a second time is owed the answer a second time.
    ///
    /// **Only a key that is still RAISED may be muted.** See [`Self::dismiss`]:
    /// a note lives on through its fade after its condition is lowered, so a
    /// click during that tail was writing a mute nothing would ever clear.
    muted: BTreeSet<String>,
}

impl Notifications {
    pub fn new() -> Self {
        Self::default()
    }

    fn mint(&mut self) -> NoteId {
        let id = NoteId(self.next);
        self.next = self.next.wrapping_add(1);
        id
    }

    /// Put a TERMINAL message up, and hand back its handle.
    ///
    /// For something that happened and is over: a receipt, a count, a path.
    /// It appears and it ages out.
    ///
    /// **The same text twice is one message, refreshed.** A column of four
    /// identical lines says nothing the first one did not. How far a refresh
    /// goes is [`Note::refresh`]'s business, and it is bounded — a caller
    /// whose "event" is really a CONDITION being re-checked belongs on
    /// [`Self::raise`], and this door will not pretend otherwise by keeping
    /// its message up forever.
    ///
    /// **What that bound does NOT buy, said plainly.** The bound holds one
    /// NOTE to twice its life; it cannot hold a CALLER to anything. A caller
    /// that pushes the same error every frame gets a note, watches it expire
    /// at twice its life, and then mints a fresh one on the very next frame —
    /// so a minute of per-frame `error(...)` is six successive messages, each
    /// dismissable and none of them lasting, rather than one message that
    /// cannot be got rid of. That is deliberate: the alternative is a memory
    /// of every text ever pushed, keyed by words the user's own filenames and
    /// device names end up in, and it would silence a receipt somebody really
    /// did earn twice. **A caller that re-checks a condition every frame must
    /// use [`Self::raise`]**, which says the thing once, lets a dismissal
    /// stick, and takes itself down when the condition clears. This door is
    /// for things that HAPPENED.
    pub fn push(&mut self, kind: Kind, text: impl Into<String>) -> NoteId {
        let text = text.into();
        let id = if let Some(n) = self
            .items
            .iter_mut()
            .find(|n| n.kind == kind && n.text == text)
        {
            n.refresh();
            n.id
        } else {
            let id = self.mint();
            self.items.push(Note::brief(id, kind, text));
            id
        };
        // **Both paths, or the cap is a suggestion.** Returning early from the
        // duplicate branch left five whole messages standing where four is the
        // rule: the fifth's refresh made it whole again and nothing counted.
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

    /// Say that a CONDITION is true, once, however often you say it.
    ///
    /// For the messages that are not events: "No MIDI input ports found", "the
    /// input device disappeared". The caller re-checks its condition every
    /// frame and calls this every time it holds; `key` names the condition so
    /// this queue can tell "still true" from "true again".
    ///
    /// The message is STANDING — no clock — and leaves when the condition is
    /// [`lower`](Self::lower)ed, when the user dismisses it, or when the cap
    /// needs its slot. Re-raising a standing condition never stacks a copy and
    /// never restarts anything; it only updates the words if they have changed,
    /// which is how a PROGRESSIVE message ("Scanning plugins... 40 of 112")
    /// lives here too.
    ///
    /// **`None` means the condition is standing with nothing on screen for
    /// it**, and the caller must not treat that as a failure and try again —
    /// it is already saying so every frame, which is the shape this door is
    /// built for. Two things produce it, and the difference is the whole point
    /// of [`Self::muted`]: the user DISMISSED the message, in which case it
    /// stays down until the condition genuinely clears; or the column is full
    /// of whole messages and this one is waiting for a slot, in which case it
    /// comes back by itself on the frame one frees. What cannot happen any
    /// more is the third case, where a message the cap tidied away was never
    /// seen again.
    ///
    /// **So this door is only for a caller that will ask again next frame.**
    /// An event-driven one — a menu row, a Connect button — asks once, and
    /// `None` reaches it as "the app did nothing at all". Those callers want
    /// [`Self::insist`], which cannot answer `None`.
    pub fn raise(
        &mut self,
        key: impl Into<String>,
        kind: Kind,
        text: impl Into<String>,
    ) -> Option<NoteId> {
        let key = key.into();
        if self.raised.contains(&key) {
            let text = text.into();
            // Newest first, as everything else in this module reads the
            // stack: lowering a condition leaves its message mid-tail with the
            // key still on it, so a condition that goes bad again straight
            // away has two notes wearing that key for half a second and the
            // live one is the later.
            if let Some(n) = self
                .items
                .iter_mut()
                .rev()
                .find(|n| n.key.as_deref() == Some(key.as_str()))
            {
                if n.text != text {
                    n.text = text;
                }
                return Some(n.id);
            }
            // Standing, and nothing on screen saying so. The user's own
            // dismissal is the one thing that keeps it that way.
            if self.muted.contains(&key) {
                return None;
            }
            // **Only into a free slot.** Re-minting regardless would put the
            // new message at the top, where the cap would immediately retire
            // whichever standing message is now oldest — and five conditions
            // would rotate through four slots twice a second, forever. Waiting
            // for a slot means the fifth fault appears the moment one of the
            // four is lowered or clicked away, which is what "the cap needs
            // its slot" was always supposed to mean.
            if !self.has_room() {
                return None;
            }
            let id = self.mint();
            self.items.push(Note::standing(id, kind, text, key));
            self.cap();
            return Some(id);
        }
        let id = self.mint();
        self.items
            .push(Note::standing(id, kind, text.into(), key.clone()));
        self.raised.insert(key);
        self.cap();
        Some(id)
    }

    /// Whether a message could stand WHOLE right now without putting the
    /// column over [`VISIBLE`]. What [`Self::cap`] is about to enforce, asked
    /// before rather than after.
    fn has_room(&self) -> bool {
        self.items.iter().filter(|n| !n.leaving()).count() < VISIBLE
    }

    /// Say a condition NOW, because the user just asked to be told.
    ///
    /// **The door [`Self::raise`] cannot be, and the reason both exist.**
    /// `raise` is built for a caller that re-checks its condition every frame:
    /// it answers `None` while the message is muted or the column is full,
    /// and that is right, because such a caller is already saying so on the
    /// next frame and every frame after. Both of this app's MIDI conditions
    /// have callers of the OTHER shape — a menu row, a picker's Connect
    /// button — which are EVENT-driven and ask exactly once. For them `None`
    /// is not "already covered", it is silence: the user picked MIDI Input
    /// from the menu, having dismissed "No MIDI input ports found" an hour
    /// ago, and the app did nothing whatsoever. Forever, because only
    /// [`Self::lower`] forgot a mute and nothing was ever going to lower a
    /// condition that never stopped being true.
    ///
    /// **An explicit request must always produce an answer**, so this one
    /// does: the mute goes, because the user has just asked the same question
    /// again and a dismissal answers the LAST asking of it; and the message
    /// is minted whether or not there is a free slot, letting [`Self::cap`]
    /// retire the oldest as it does for any other arrival. That is safe here
    /// in a way it would not be in `raise`: this is one message per press,
    /// not five conditions rotating through four slots sixty times a second.
    ///
    /// Everything else is `raise`'s: it is one message however often it is
    /// asked for, and [`Self::lower`] still ends it early when the condition
    /// genuinely clears. A note already on screen for this key takes the new
    /// words and keeps its identity — a note on its way OUT does not, because
    /// handing an explicit request back a box that is already fading is the
    /// same silence in slower motion.
    ///
    /// **Except the clock, which is the one thing this does NOT borrow.** A
    /// raised message stands because a condition is a state and states end
    /// when they end; an answer is a reply to a press, and nothing on earth
    /// was ever going to lower "That is a MIDI part, not a sample". So an
    /// answer ages out on its own after [`LIFE_ANSWER_S`], and asking again
    /// restarts it.
    pub fn insist(
        &mut self,
        key: impl Into<String>,
        kind: Kind,
        text: impl Into<String>,
    ) -> NoteId {
        let key = key.into();
        let text = text.into();
        self.muted.remove(&key);
        self.raised.insert(key.clone());
        // Newest first and never one that is leaving, for the reason `raise`
        // reads the stack backwards: a condition lowered a moment ago still
        // has a note wearing its key while its tail runs.
        if let Some(n) = self
            .items
            .iter_mut()
            .rev()
            .find(|n| n.key.as_deref() == Some(key.as_str()) && !n.leaving())
        {
            if n.text != text {
                n.text = text;
            }
            // **Asking again restarts the clock**, and gives one to a note
            // that was raised as a condition and is now being ANSWERED. A
            // press that gets the same box back three seconds from its end
            // has been answered in name only.
            n.age = 0.0;
            n.life = Some(LIFE_ANSWER_S);
            return n.id;
        }
        let id = self.mint();
        self.items.push(Note::answered(id, kind, text, key));
        self.cap();
        id
    }

    /// The condition has cleared: take its message down.
    ///
    /// It LEAVES rather than blinking out — the same tail everything else gets
    /// — and the key is forgotten, so the next time the condition goes bad the
    /// message is raised again as new. Answers whether the condition was
    /// actually standing.
    pub fn lower(&mut self, key: &str) -> bool {
        // The dismissal goes with the condition, always — the user said they
        // were finished with THIS episode of it, not with the next one.
        self.muted.remove(key);
        if !self.raised.remove(key) {
            return false;
        }
        for n in &mut self.items {
            if n.key.as_deref() == Some(key) {
                n.retire();
            }
        }
        true
    }

    /// Whether this condition is currently raised — true even if its message
    /// has been dismissed, because the condition is what "raised" means.
    pub fn is_raised(&self, key: &str) -> bool {
        self.raised.contains(key)
    }

    /// Whether the USER took this condition's message down, which is the one
    /// thing that keeps [`Self::raise`] quiet while the condition holds.
    pub fn is_muted(&self, key: &str) -> bool {
        self.muted.contains(key)
    }

    /// Change a message's words in PLACE.
    ///
    /// **A progress report is one message, not forty.** "Scanning plugins... 40
    /// of 112" pushed once a file would stack copies until the cap ate the
    /// column and then flicker between the survivors; the same message saying
    /// something new is what actually happened, so that is what this does.
    ///
    /// **`false` means the message is no longer in the queue, and there are
    /// two ways for that to be true.** Usually it is the user: they clicked
    /// the box away, and a report nobody wants to see is a report that should
    /// stop being written. But the cap can take it as well — a burst past
    /// `VISIBLE + 1` drops what is already leaving and then the oldest, and a
    /// progress report that has been sitting still while fifty errors landed
    /// on top of it can be that oldest. So `false` is "it is gone", not "the
    /// user is finished with it", and a caller that wants to distinguish the
    /// two has to ask [`Self::is_empty`] or look at what else it has been
    /// pushing. **What `false` no longer means is "refused".** The clock used
    /// to be put back through [`Note::refresh`], whose bound is about
    /// DUPLICATES, so a scan that ran longer than one lifetime had its report
    /// expire underneath it at eight seconds and every later update was
    /// reporting into a message nobody could see. A caller holding a handle is
    /// the message's author, not a duplicate arriving, so the clock goes back
    /// every call — see [`Note::renew`] for why that is not the unkillable
    /// message. A scan of any length is one line that counts, and it takes
    /// something outside the scan to end it.
    ///
    /// The clock goes back even when the words have not changed, because a
    /// scan stuck on one slow plugin for six seconds is still running and its
    /// report should still be on screen. A standing message has no clock to
    /// touch and only takes the new words.
    pub fn update(&mut self, id: NoteId, text: impl Into<String>) -> bool {
        let text = text.into();
        let Some(n) = self.items.iter_mut().find(|n| n.id == id) else {
            return false;
        };
        if n.text != text {
            n.text = text;
        }
        n.renew();
        // A refresh can make a message whole that the cap had already retired,
        // which is the same hole the duplicate push had: every door that can
        // put a message back on its feet counts the column again.
        self.cap();
        true
    }

    /// Age the stack to `now` and retire whatever ran out.
    ///
    /// `now` is egui's own frame time in seconds — `ctx.input(|i| i.time)` —
    /// which is monotonic and does not need a clock this crate is not allowed
    /// to have.
    ///
    /// **Answers whether anything is still MOVING**, which is the repaint
    /// signal: a caller that stops repainting while this is true freezes the
    /// stack half-faded on screen. A STANDING message is not motion — it sits
    /// at full strength until something takes it down — so it does not pin the
    /// app at the display's refresh rate for as long as a device stays
    /// unplugged. Asking for the repaint is the CALLER's job either way: a
    /// painter that reaches back into the context to demand another frame is
    /// how an overlay becomes a permanent 60 Hz redraw nobody can find.
    pub fn tick(&mut self, now: f64) -> bool {
        let dt = self.last.map_or(0.0, |prev| (now - prev).clamp(0.0, MAX_STEP));
        self.last = Some(now);
        let held = self.held;
        for n in &mut self.items {
            if Some(n.id) != held {
                n.age += dt;
                n.stood += dt;
            }
        }
        self.items.retain(|n| !n.expired());
        self.items.iter().any(|n| n.life.is_some())
    }

    /// Note which box the pointer is on, if any. The next [`tick`](Self::tick)
    /// leaves that one's clock alone.
    pub fn hold(&mut self, id: Option<NoteId>) {
        self.held = id;
    }

    /// Take a message down NOW, and hand it back.
    ///
    /// What a click does: the user has said they are finished with it, and a
    /// fade would be the app disagreeing.
    ///
    /// **It returns the note, whole.** The box elides a long message on
    /// purpose and this queue is the only place the rest of it lives, so a
    /// caller that wants to route the full text somewhere — a log, a details
    /// panel, the clipboard — has exactly one frame to be handed it, and this
    /// is that frame. Looking it up afterwards finds nothing, which is what
    /// "taken down" means.
    ///
    /// A message raised against a condition stays dismissed: the key is
    /// MUTED here, and re-raising will not put it back until the condition
    /// clears. Only [`Self::lower`] and [`Self::insist`] forget a mute — see
    /// [`Self::muted`] for why that memory is not the same one as `raised`.
    ///
    /// **Only a dismissal of a condition that is still STANDING may mute it,
    /// and that qualifier cost the same fault twice.** [`Self::lower`] retires
    /// a note rather than deleting it — the tail is how a message leaves — so
    /// for the half-second after a condition clears there is still a box on
    /// screen wearing its key, and it is still clickable. Muting on that click
    /// wrote a mute for a key nothing had raised, and since only `lower`
    /// forgets a mute and the condition was already lowered, the NEXT episode
    /// of that fault was silenced before it happened: the device came back,
    /// went again, and said nothing at all. So the key is consulted first.
    /// Clicking a message on its way out means "I have read it", which is what
    /// the fade was already saying; it cannot mean "and do not tell me next
    /// time", because there is no condition left for next time to be about.
    pub fn dismiss(&mut self, id: NoteId) -> Option<Note> {
        let i = self.items.iter().position(|n| n.id == id)?;
        let note = self.items.remove(i);
        if let Some(key) = note.key.as_deref().filter(|k| self.raised.contains(*k)) {
            self.muted.insert(key.to_owned());
        }
        Some(note)
    }

    /// Start over: every message down and every condition forgotten.
    ///
    /// The conditions go too, because this is the "none of that applies any
    /// more" call — a project closing, a device set torn down — and anything
    /// still true will raise itself again on its next check.
    pub fn clear(&mut self) {
        self.items.clear();
        self.raised.clear();
        self.muted.clear();
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
    /// once. The ceiling drops something already LEAVING before it touches a
    /// message that is still whole, so a burst of receipts cannot silently eat
    /// a standing device error underneath it.
    fn cap(&mut self) {
        let mut whole = 0;
        for n in self.items.iter_mut().rev() {
            if n.leaving() {
                continue;
            }
            whole += 1;
            if whole > VISIBLE {
                n.retire();
            }
        }
        while self.items.len() > VISIBLE + 1 {
            let i = self.items.iter().position(Note::leaving).unwrap_or(0);
            self.items.remove(i);
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

/// How many steps the card may take away from the face before it gives up and
/// takes white or black. See [`dress_on`].
const CARD_STEPS: u16 = 24;

/// The app's one green, fixed in every theme like the record red beside it: a
/// receipt that changed colour with the palette would have to be learnt twice.
/// Pushed off the card at draw time — see [`Style::of`] — where a green theme
/// would swallow it.
///
/// **The eighth copy of this literal in the tree**, and it should be the
/// import that [`REC_RED`] already is. There is no public home for it yet:
/// `recorder_panel` keeps two private ones (`PLAY_GREEN`, `LED_GREEN`), the
/// drumpad a third, and the mixer and arrangement spell it inline. Promoting
/// one of those to `pub` is a one-line change in a file this lane may not
/// touch, so it is named here and left for whoever does.
const METER_GREEN: Color32 = Color32::from_rgb(0x3D, 0xC0, 0x5A);

/// `t` of the way from `a` to `b`.
fn blend(a: Color32, b: Color32, t: f32) -> Color32 {
    let f = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t) as u8;
    Color32::from_rgb(f(a.r(), b.r()), f(a.g(), b.g()), f(a.b(), b.b()))
}

/// The card's own surface: the face moved until it is BOTH separate from that
/// face and writable, measured at the END.
///
/// **Composing the two helpers does not compose their guarantees.**
/// `distinct_surface_at(face, face, SEPARATION)` moves the ground until you can
/// see where the card ends; `surface_of` then makes it writable, and on the
/// 0.12–0.22 dead zone that means moving the SURFACE — in whichever direction
/// the closer ink asks for, which knows nothing about the face and is quite
/// happy to walk the card straight back onto it. The card that came out was
/// legible and invisible, and the sweep never caught it because it measured
/// the intermediate colour rather than the one that gets painted.
///
/// So the two rules are checked TOGETHER, on the finished surface, and the
/// card keeps walking away from the face until both hold. It terminates: the
/// last step is white on a dark face or black on a pale one, and those carry
/// ink at better than 15:1 and stand off the face they were chosen for at
/// better than 4:1 — worst case a mid grey right on the ink crossover, which
/// is still three times the separation asked for. Walking from the NEAR end
/// also means a face that never had the problem gets exactly the colour it
/// always got.
fn dress_on(face: Color32) -> crate::recorder_panel::Surface {
    let start = distinct_surface_at(face, face, SEPARATION);
    // Away from the face is the direction its own ink came from: a dark face
    // is left toward white, a pale one toward black.
    let away = if wants_light_ink(face) {
        Color32::WHITE
    } else {
        Color32::BLACK
    };
    let mut last = surface_of(start);
    for step in 0..=CARD_STEPS {
        let t = f32::from(step) / f32::from(CARD_STEPS);
        let s = surface_of(blend(start, away, t));
        if contrast_ratio(s.bg, face) >= SEPARATION {
            return s;
        }
        last = s;
    }
    last
}

/// Everything a notification is made of, derived from the face it lands on.
///
/// **The face is whatever is UNDER the top-left corner**, which is not one
/// thing: the piano and theory band in some views, the arrangement's field in
/// others. So the caller names it and the card is built from it, rather than
/// the card picking a colour and hoping.
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
        let s = dress_on(face);
        Self {
            bg: s.bg,
            ink: s.ink,
            faint: s.faint,
            edge: s.edge,
            // Info has no hue of its own. Its stripe is the card's quiet ink,
            // which reads as "these are just words" beside a stripe that is
            // red — and it is derived from the card, so it is right on all
            // thirty-two palettes without any of them naming it.
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

/// One box, measured and placed but not yet painted.
///
/// Laying out and painting used to be one pass, which is fine for a painter
/// that owns the whole column and impossible for [`show`]: an Area has to be
/// created AT the box's position, and that position is only known once every
/// box above it has been measured.
struct Laid {
    placed: Placed,
    galley: Arc<egui::Galley>,
}

/// Measure the column into the top-left of `area`.
///
/// `layout` is handed in rather than taken from a painter because the two
/// callers have different halves of egui: [`draw`] has a `Painter`, [`show`]
/// has only a `Context` and needs the answer before it can make the layers to
/// paint into.
fn plan(
    layout: &dyn Fn(egui::text::LayoutJob) -> Arc<egui::Galley>,
    area: Rect,
    notes: &Notifications,
) -> Vec<Laid> {
    let mut out = Vec::new();
    if !area.is_positive() {
        return out;
    }
    // Squeezed toward `MIN_WIDTH`, never past the window's own edge: a box
    // that hangs off the right of a 120-point editor is a message you can only
    // half read, over the controls it is covering.
    let w = (area.width() - MARGIN * 2.0).min(WIDTH);
    if w < MIN_WIDTH {
        return out;
    }
    let wrap = w - PAD_X * 2.0 - ACCENT_W;
    let font = FontId::new(fonts::px(TEXT_PT), fonts::courier_bold());
    let mut y = area.top() + MARGIN;
    for n in notes.stack() {
        // Laid out with no colour of its own so the same galley can be painted
        // at whatever the tail has faded the ink to this frame.
        let mut job =
            egui::text::LayoutJob::simple(n.text.clone(), font.clone(), Color32::PLACEHOLDER, wrap);
        job.wrap.max_rows = MAX_ROWS;
        job.wrap.overflow_character = Some('…');
        let galley = layout(job);
        let h = galley.size().y + PAD_Y * 2.0;
        let rect = Rect::from_min_size(Pos2::new(area.left() + MARGIN, y), Vec2::new(w, h));
        if rect.bottom() > area.bottom() - MARGIN {
            break;
        }
        out.push(Laid {
            placed: Placed {
                id: n.id,
                kind: n.kind,
                rect,
                fade: n.fade(),
                rows: galley.rows.len(),
                elided: galley.elided,
            },
            galley,
        });
        y = rect.bottom() + GAP;
    }
    out
}

/// Paint one measured box.
fn paint(painter: &Painter, l: &Laid, st: &Style) {
    let Laid { placed, galley } = l;
    let rect = placed.rect;
    let dim = |c: Color32| c.gamma_multiply(placed.fade);
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
        dim(st.accent(placed.kind)),
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
    let laid = plan(&|job| painter.layout_job(job), area, notes);
    let mut out = Drawn::default();
    for l in &laid {
        paint(painter, l, st);
        out.boxes.push(l.placed);
    }
    out
}

// ── the whole job ───────────────────────────────────────────────────────────

/// The Areas' id root. One layer per SLOT in the column rather than one per
/// message: egui never prunes `Memory::areas`, so a layer id derived from a
/// `NoteId` would leave one dead `AreaState` behind per message shown, forever,
/// on a list that `layer_id_at` walks on every pointer move. Five slots is
/// five layers for the life of the app.
///
/// The WIDGET inside each one is still keyed by the note, so a column that
/// shifts up between a press and its release cannot land the click on whatever
/// slid into that slot.
const AREA_ID: &str = "tangent-notifications";

/// The id of the clickable box for one message.
///
/// **Tagged apart from the slot on purpose.** An `Id` is a hash, and a `usize`
/// hashes exactly as a `u64` newtype does, so `(AREA_ID, 1usize)` — slot one's
/// own layer — and `(AREA_ID, NoteId(1))` were the same id: the box for the
/// second message claimed its neighbouring layer's identity, and egui caught it
/// as a widget that changed layer mid-frame.
fn box_id(id: NoteId) -> egui::Id {
    egui::Id::new((AREA_ID, "note", id))
}

/// What [`show`] did this frame.
#[derive(Debug, Default, Clone)]
pub struct Shown {
    /// **Honour this or the stack freezes mid-fade.** True while anything is
    /// still moving. `show` does not call `request_repaint` itself, because an
    /// overlay that quietly pins the app at the display's refresh rate is a
    /// battery cost nobody can trace back to a message that left four seconds
    /// ago — the caller owns that decision and can see it in its own code. A
    /// STANDING message does not set it: see [`Notifications::tick`].
    pub wants_repaint: bool,
    /// The message a click took down, WHOLE.
    ///
    /// The note itself and not its id, because by the time the caller sees this
    /// the message is out of the queue and looking the id up finds nothing —
    /// and the box the user clicked may well have been showing an elided five
    /// rows of it. This is the one frame on which the full text can still be
    /// routed somewhere: a log, a details panel, the clipboard.
    pub dismissed: Option<Note>,
    /// Where the boxes landed, newest first.
    pub boxes: Vec<Placed>,
}

/// Age the stack, paint it into the top-left of `area`, and handle the click.
///
/// The whole job, for the app: it ticks against egui's own clock, puts each box
/// on `Order::Foreground` inside the main window (never a viewport, never
/// focus-taking), pauses the countdown under the pointer, eats a click on a box
/// and takes that message down.
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
    let laid = plan(&|job| ctx.fonts_mut(|f| f.layout_job(job)), area, notes);
    let mut hovered = None;
    let mut clicked = None;
    for (slot, l) in laid.iter().enumerate() {
        let rect = l.placed.rect;
        egui::Area::new(egui::Id::new((AREA_ID, slot)))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .movable(false)
            // Clamped by `plan` to the rect it was given, which is the window
            // and not the screen; egui's own constraint would fight it.
            .constrain(false)
            // The tail is this module's, timed in the seconds the owner asked
            // for. egui's own fade-in would multiply into it on the first
            // frames and make a fresh message look like a leaving one.
            .fade_in(false)
            .show(ctx, |ui| {
                paint(ui.painter(), l, &st);
                // **The Area has to OWN this rect.** Its size comes from what
                // was laid into it, and `interact` lays nothing: without this
                // the layer is zero-sized, egui decides the pointer is over the
                // panel underneath, and one click both dismisses the message
                // AND lands on the arrangement behind it. It owns THIS box and
                // nothing else — the gap below it belongs to whatever is under
                // the column.
                ui.expand_to_include_rect(rect);
                // `Sense::CLICK` and not `Sense::click()`, which is
                // `CLICK | FOCUSABLE`: a message is not a control, it must
                // never join the Tab order, and a box that can hold keyboard
                // focus for the four seconds before it evaporates is a caret
                // the user has to go and find again.
                let r = ui.interact(rect, box_id(l.placed.id), egui::Sense::CLICK);
                if let Some(n) = notes.get(l.placed.id) {
                    // Spoken, because the box is the only place the message is
                    // now: a reader that cannot find it never hears it at all.
                    let spoken = n.spoken();
                    r.widget_info(|| {
                        egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &spoken)
                    });
                }
                if r.hovered() {
                    hovered = Some(l.placed.id);
                }
                if r.clicked() {
                    clicked = Some(l.placed.id);
                }
            });
        out.boxes.push(l.placed);
    }
    notes.hold(hovered);
    if let Some(id) = clicked {
        out.dismissed = notes.dismiss(id);
        // One more frame: the column above the gap has to close up.
        out.wants_repaint = true;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::recorder_panel::{legible_surface, INK_DARK, INK_LIGHT};

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

    /// Every rule a dressed notification answers to, measured on the FINAL
    /// colours: the card against the face it landed on, and everything printed
    /// on the card against the card.
    ///
    /// **The card is measured, not the colour it was on the way to.** `Style`
    /// used to hand `distinct_surface_at`'s answer to `surface_of`, and the
    /// sweep checked the first of those against the face while the second was
    /// what got painted — so a face whose card fell in the dead zone had it
    /// walked back onto that face and nothing said a word.
    fn dressing_faults(face: Color32, st: &Style) -> Vec<String> {
        let mut bad = Vec::new();
        let mut check = |what: &str, r: f32, floor: f32| {
            if r < floor {
                bad.push(format!("{what} {r:.2}:1"));
            }
        };
        check("the message on the card", contrast_ratio(st.ink, st.bg), 4.5);
        check(
            "the quiet ink on the card",
            contrast_ratio(st.faint, st.bg),
            4.5,
        );
        // The whole reason the card is derived and not picked: it has to be
        // seen ON the thing it arrived over.
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
        bad
    }

    /// **Readable on every theme, in both modes, or the build fails.** The
    /// sweep `face.rs` and `shell.rs` run, on the notification: its ink and its
    /// quiet ink measured on the card they are printed on, the card measured
    /// against the FOUR faces it can land in the corner of — the piano band,
    /// the record band, the desk's strip and the arrangement's lane field —
    /// its edge against its own ground, and every kind's accent on the card.
    ///
    /// The arrangement field is here because it is the most likely thing under
    /// the top-left corner in the view the owner works in, and it was the one
    /// ground the sweep did not sample.
    ///
    /// The accents include the two fixed-meaning colours, which is exactly
    /// where a theme could break them silently: no palette may repaint the
    /// record red, so the only question a theme can answer wrongly is whether
    /// it is still visible.
    #[test]
    fn every_theme_dresses_a_notification() {
        let themes = crate::theme::factory();
        assert!(
            themes.len() >= 32,
            "the sweep expects the thirty-two factory themes, found {}",
            themes.len()
        );
        let mut bad: Vec<String> = Vec::new();
        let mut swept = 0;
        for theme in &themes {
            for dark in [false, true] {
                let mut set = crate::settings::Settings::default();
                theme.apply(&mut set);
                set.dark_mode = dark;
                let role = |role: &str, light: crate::settings::Rgb| {
                    let c = set.dark_role_if(dark, role).unwrap_or(light);
                    Color32::from_rgb(c.r, c.g, c.b)
                };
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
                // The arrangement's LANE FIELD, derived the way
                // `arrangement_panel::ink` derives it from the same two roles:
                // the timecode well lifted in dark mode or carried toward the
                // strip in light, then through the dead-zone guard. Mirrored
                // rather than called because that derivation wants a whole
                // `ArrangeView` to exist; `the_card_stands_off_any_face`
                // below is what actually guarantees the card against grounds
                // this one only samples.
                let well = role("meter_bg", set.meter_bg_color);
                let field = legible_surface(if dark {
                    // `shade(well, 1.55)`: a fraction of the headroom, so the
                    // hue survives.
                    blend(well, Color32::WHITE, 0.55 * 0.28)
                } else {
                    blend(well, role("fx_strip", set.fx_strip_color), 0.46)
                });
                let mode = if dark { "dark" } else { "light" };
                for (which, face) in [
                    ("strip", strip),
                    ("band", band),
                    ("keys", keys),
                    ("field", field),
                ] {
                    let st = Style::of(face);
                    for fault in dressing_faults(face, &st) {
                        bad.push(format!("{} {mode} ({which}): {fault}", theme.name));
                    }
                    swept += 1;
                }
            }
        }
        assert!(swept >= 32 * 2 * 4, "the sweep covered {swept} dressings");
        assert!(
            bad.is_empty(),
            "a notification is unreadable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// **Both rules hold at the END, on any face at all.**
    ///
    /// The four theme faces are four samples of a rule that has to hold for
    /// every colour the corner can land on — a hand-picked strip, a wallpaper
    /// nobody has chosen yet — and the failure this catches lives in a narrow
    /// band of luminance that a sample can miss entirely. So: every grey, and
    /// a coarse grid of every hue.
    #[test]
    fn the_card_stands_off_any_face() {
        let mut bad: Vec<String> = Vec::new();
        let mut swept = 0usize;
        let mut faces: Vec<Color32> = (0..=255).map(Color32::from_gray).collect();
        for r in (0..=255).step_by(17) {
            for g in (0..=255).step_by(17) {
                for b in (0..=255).step_by(17) {
                    faces.push(Color32::from_rgb(r, g, b));
                }
            }
        }
        for face in faces {
            let st = Style::of(face);
            swept += 1;
            for fault in dressing_faults(face, &st) {
                if bad.len() < 12 {
                    bad.push(format!("face {face:?}: {fault}"));
                }
            }
        }
        assert!(swept > 4000, "the sweep covered {swept} faces");
        assert!(
            bad.is_empty(),
            "the card does not hold on every face:\n  {}",
            bad.join("\n  ")
        );
        // And the walk ends somewhere sane rather than at white on everything:
        // a face that never had the problem keeps the colour it always had.
        let plain = Color32::from_rgb(0x2A, 0x24, 0x1E);
        assert_eq!(
            Style::of(plain).bg,
            surface_of(distinct_surface_at(plain, plain, SEPARATION)).bg,
            "a face that composed fine was moved anyway"
        );
        // The end of the walk really is a floor: white and black are already
        // legible, so `surface_of` leaves them alone, and each stands off
        // every face that would send the walk its way. The worst of those is a
        // mid grey sitting on the ink crossover, and it still clears the
        // separation three times over.
        let mut worst = f32::INFINITY;
        for g in 0..=255 {
            let face = Color32::from_gray(g);
            let away = if wants_light_ink(face) {
                Color32::WHITE
            } else {
                Color32::BLACK
            };
            assert_eq!(legible_surface(away), away, "the walk ends somewhere unwritable");
            worst = worst.min(contrast_ratio(away, face));
        }
        assert!(worst > 4.0, "the end of the walk only reaches {worst:.2}:1");
        for end in [Color32::WHITE, Color32::BLACK] {
            assert!(contrast_ratio(end, INK_LIGHT).max(contrast_ratio(end, INK_DARK)) > 15.0);
        }
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
        // A message with NO clock is still held to the cap: five raised
        // conditions are five whole messages, which is one too many, and a
        // standing message cannot be waited out.
        let mut n = Notifications::new();
        let first = n.raise("c0", Kind::Error, "condition 0").unwrap();
        for i in 1..5 {
            n.raise(format!("c{i}"), Kind::Error, format!("condition {i}"));
        }
        assert_eq!(
            n.notes().iter().filter(|x| !x.leaving()).count(),
            VISIBLE,
            "five standing messages stood whole"
        );
        assert!(
            n.get(first).is_some_and(Note::leaving),
            "the standing message the cap retired never left"
        );
        let mut t = 0.0;
        n.tick(t);
        advance(&mut n, &mut t, 1.0);
        assert!(n.get(first).is_none(), "a retired standing message stayed");
    }

    /// The same failure arriving once per buffer is one message, refreshed —
    /// but only for so long.
    ///
    /// **A repeat must not be an unkillable message.** Refreshing without a
    /// bound meant a caller that re-raised its condition every frame had a note
    /// that never expired and came straight back the moment it was clicked
    /// away; `raise` is the door for that shape, and this one holds any
    /// repeated message to twice its own life.
    #[test]
    fn the_same_message_twice_is_one_message_but_not_forever() {
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
        // Now the fault that never stops. Pushed every frame for a minute, it
        // still leaves — inside twice its life, not sixty seconds later.
        let mut n = Notifications::new();
        let id = n.error("Input device disappeared");
        let mut t = 0.0;
        n.tick(t);
        let mut gone_at = None;
        for frame in 0..3_600 {
            t += 1.0 / 60.0;
            n.tick(t);
            n.error("Input device disappeared");
            if n.get(id).is_none() && gone_at.is_none() {
                gone_at = Some(f64::from(frame) / 60.0);
                break;
            }
        }
        let gone_at = gone_at.expect("a repeated message never expired");
        assert!(
            gone_at <= LIFE_ERROR_S * 2.0 + 0.1,
            "a repeated message stood {gone_at:.2}s"
        );
        assert!(gone_at >= LIFE_ERROR_S, "a repeat cut it short at {gone_at:.2}s");
        // And the cap holds on the duplicate path, which is where it did not.
        // Five messages leave the oldest LEAVING; pushing that one again makes
        // it whole, and the branch that refreshed it used to return before the
        // cap could count — five whole messages where the rule says four.
        let mut n = Notifications::new();
        for i in 0..5 {
            n.info(format!("m{i}"));
        }
        assert!(n.notes()[0].leaving(), "the probe needs a leaving message");
        n.info("m0");
        assert_eq!(
            n.notes().iter().filter(|x| !x.leaving()).count(),
            VISIBLE,
            "a duplicate refreshed its way past the cap"
        );
    }

    /// **A condition is said once, and a dismissal sticks.**
    ///
    /// The caller re-checks "is there a MIDI input port" every frame and says
    /// so every frame. One message stands. The user clicks it away and it
    /// stays away, however many more times the condition is re-raised — until
    /// the port comes back, the condition is lowered, and the next failure is
    /// news again.
    #[test]
    fn a_condition_stands_once_and_a_dismissal_sticks() {
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        let id = n
            .raise("midi-in", Kind::Error, "No MIDI input ports found")
            .expect("the first raise said nothing");
        for _ in 0..600 {
            t += 1.0 / 60.0;
            n.tick(t);
            assert_eq!(
                n.raise("midi-in", Kind::Error, "No MIDI input ports found"),
                Some(id),
                "re-raising minted a second message"
            );
        }
        assert_eq!(n.len(), 1, "ten seconds of re-raising stacked copies");
        assert!(
            n.get(id).is_some_and(Note::is_standing),
            "a condition aged out while it was still true"
        );
        // The user takes it down.
        let taken = n.dismiss(id).expect("dismiss lost the note");
        assert_eq!(taken.text, "No MIDI input ports found");
        for _ in 0..600 {
            t += 1.0 / 60.0;
            n.tick(t);
            assert_eq!(
                n.raise("midi-in", Kind::Error, "No MIDI input ports found"),
                None,
                "a dismissed message came straight back"
            );
        }
        assert!(n.is_empty(), "the dismissed message returned");
        assert!(n.is_raised("midi-in"), "the condition forgot itself");
        // The port comes back: the condition clears, and the message may be
        // news again next time.
        assert!(n.lower("midi-in"));
        assert!(!n.is_raised("midi-in"));
        assert!(!n.lower("midi-in"), "lowering twice claimed to do something");
        let again = n
            .raise("midi-in", Kind::Error, "No MIDI input ports found")
            .expect("a cleared condition stayed silent");
        assert_ne!(again, id, "the same handle came back from the dead");
        assert_eq!(n.len(), 1);
        // Lowering a live one gives it a tail rather than blinking it out.
        assert!(n.lower("midi-in"));
        assert!(
            n.get(again).is_some_and(Note::leaving),
            "a lowered message vanished between frames"
        );
        advance(&mut n, &mut t, 1.0);
        assert!(n.is_empty(), "a lowered message never finished leaving");
        // A condition that goes bad again while its last message is still
        // leaving: two notes wear the key for half a second, and the one the
        // caller is handed has to be the LIVE one.
        let old = n.raise("dev", Kind::Error, "gone").unwrap();
        assert!(n.lower("dev"));
        let fresh = n.raise("dev", Kind::Error, "gone again").unwrap();
        assert_ne!(fresh, old, "the raise reused the message it just lowered");
        assert_eq!(
            n.raise("dev", Kind::Error, "gone again, still"),
            Some(fresh),
            "a re-raise reached back past the live message"
        );
        assert_eq!(
            n.get(fresh).map(|x| x.text.as_str()),
            Some("gone again, still")
        );
        assert!(n.get(old).is_some_and(Note::leaving), "the old one stopped leaving");
        advance(&mut n, &mut t, 1.0);
        assert_eq!(n.len(), 1, "the lowered message came back");
        n.clear();
        // `clear` is the start-over call: conditions go too, so anything still
        // true raises again.
        n.raise("midi-in", Kind::Error, "No MIDI input ports found");
        n.clear();
        assert!(!n.is_raised("midi-in"));
        assert!(n
            .raise("midi-in", Kind::Error, "No MIDI input ports found")
            .is_some());
    }

    /// **A dismissal silences a condition; the CAP must not.**
    ///
    /// The two used to be the same act. `raise` remembered a key and refused
    /// to mint a second message for it, which is exactly right for a message
    /// the user clicked away — and catastrophic for one the cap tidied away to
    /// make room for four receipts: a dead input device went quiet for the
    /// rest of the session, and every later `raise` answered `None` with
    /// nothing on screen to say why. So the eviction leaves the condition able
    /// to speak, and only the user's own click does not.
    #[test]
    fn the_cap_may_take_a_conditions_slot_but_never_its_voice() {
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        let dev = n
            .raise("dev", Kind::Error, "The input device disappeared")
            .expect("the fault said nothing at all");
        // Four receipts land on top of it. None of them is a fault; all of
        // them are newer; the cap gives them the column.
        for i in 0..VISIBLE {
            n.info(format!("Exported take {i}"));
        }
        assert!(
            n.get(dev).is_some_and(Note::leaving),
            "the cap did not take the standing message's slot"
        );
        advance(&mut n, &mut t, 1.0);
        assert!(n.get(dev).is_none(), "the evicted message never finished leaving");
        assert!(n.is_raised("dev"), "the condition forgot itself");
        assert!(!n.is_muted("dev"), "an eviction was recorded as the user's choice");
        // While the column is still full it waits rather than rotating: five
        // standing messages through four slots twice a second is a strobe, not
        // a report.
        assert_eq!(
            n.raise("dev", Kind::Error, "The input device disappeared"),
            None,
            "the fault shouldered a receipt out of a full column"
        );
        assert_eq!(n.len(), VISIBLE, "the column grew past the cap");
        // The receipts age out, and the fault — still true, still said every
        // frame — is back.
        advance(&mut n, &mut t, 5.0);
        let again = n
            .raise("dev", Kind::Error, "The input device disappeared")
            .expect("a fault the cap tidied away was silenced forever");
        assert_ne!(again, dev, "the evicted note came back from the dead");
        assert_eq!(n.len(), 1);
        // The user's own click is the other thing entirely, and it still
        // sticks for as long as the condition holds.
        assert!(n.dismiss(again).is_some());
        assert!(n.is_muted("dev"), "a dismissal was not remembered");
        for _ in 0..600 {
            t += 1.0 / 60.0;
            n.tick(t);
            assert_eq!(
                n.raise("dev", Kind::Error, "The input device disappeared"),
                None,
                "a dismissed condition came straight back"
            );
        }
        // ...and only until it clears. Then the next failure is news.
        assert!(n.lower("dev"));
        assert!(!n.is_muted("dev"), "the mute outlived its condition");
        assert!(n
            .raise("dev", Kind::Error, "The input device disappeared")
            .is_some());
    }

    /// **A request the user made by hand is always answered.**
    ///
    /// `raise` goes quiet while a key is muted, which is exactly right for the
    /// caller it was built for — one that re-checks its condition every frame
    /// and is therefore already saying so. Both MIDI callers are the other
    /// shape: a menu row and a picker's Connect button, each asking once. So
    /// dismissing "No MIDI input ports found" and then choosing MIDI Input
    /// from the menu produced nothing at all — no message, no picker, no
    /// answer — and went on producing nothing for the rest of the session,
    /// because the only thing that forgot a mute was a `lower` nothing was
    /// ever going to call for a condition that never stopped being true.
    /// **An answer leaves on its own; a condition still waits to be told.**
    ///
    /// The owner, testing: "Make the helper notifs also fade on their own
    /// after a set interval. These interfere with testing." The helper
    /// messages are the ones [`Notifications::insist`] puts up — "That is a
    /// MIDI part, not a sample", "No MIDI track on the desk" — and they stood
    /// for ever because `insist` borrowed `raise`'s clockless note. Nothing
    /// was ever going to lower them: they are replies to a press, not reports
    /// of a state, and the only way out was a click in the middle of whatever
    /// the message had just interrupted.
    ///
    /// Both halves matter. A raised CONDITION must not pick up a clock from
    /// this — an input device that has genuinely vanished is the one message
    /// that must not quietly leave — so the two are pinned side by side under
    /// the same clock.
    ///
    /// Revert check performed: mint an answer with `Note::standing` again and
    /// the first half fails with the box still up after a minute.
    #[test]
    fn an_answer_ages_out_but_a_condition_stands() {
        let mut n = Notifications::new();
        n.tick(0.0);
        let answer = n.insist("clip-drag-is-midi", Kind::Info, "That is a MIDI part");
        let condition = n
            .raise("midi-port", Kind::Error, "The MIDI device went away")
            .expect("a fresh condition was refused");

        // Walked rather than jumped: `MAX_STEP` clamps one tick's ageing, so
        // a single leap to the far side ages the stack by half a second and
        // proves nothing.
        let mut t = 0.0;
        let mut gone_at = None;
        while t < 60.0 {
            t += 0.25;
            n.tick(t);
            if gone_at.is_none() && n.get(answer).is_none() {
                gone_at = Some(t);
            }
        }
        let gone_at = gone_at.expect("the answer never left");
        assert!(
            gone_at >= LIFE_ANSWER_S,
            "the answer left after {gone_at}s, inside its {LIFE_ANSWER_S}s"
        );
        assert!(
            gone_at <= LIFE_ANSWER_S + FADE_S + 1.0,
            "the answer took {gone_at}s to leave"
        );
        assert!(
            n.get(condition).is_some(),
            "a standing condition was aged out with the answers"
        );

        // And asking again restarts it rather than handing back a box that
        // is three seconds from its end.
        let mut n = Notifications::new();
        n.tick(0.0);
        let first = n.insist("clip-drag-is-midi", Kind::Info, "That is a MIDI part");
        for k in 1..=16 {
            n.tick(f64::from(k) * 0.5);
        }
        assert_eq!(
            n.insist("clip-drag-is-midi", Kind::Info, "That is a MIDI part"),
            first,
            "a second asking minted a second message"
        );
        let mut t = 8.0;
        while t < 8.0 + LIFE_ANSWER_S - 1.0 {
            t += 0.25;
            n.tick(t);
        }
        assert!(
            n.get(first).is_some(),
            "asking again did not restart the clock"
        );
    }

    #[test]
    fn an_explicit_request_is_answered_however_often_it_was_dismissed() {
        let mut n = Notifications::new();
        n.tick(0.0);
        let first = n.insist("midi-in", Kind::Error, "No MIDI input ports found");
        assert!(n.is_raised("midi-in"), "an insisted condition forgot itself");
        // **And it has a CLOCK**, which is the one thing an answer does not
        // borrow from the condition it is keyed like. Nothing was ever going
        // to lower this message, so without one it sat in the corner until
        // somebody clicked it — see `LIFE_ANSWER_S`.
        assert_eq!(
            n.get(first).and_then(|n| n.expires_at()),
            Some(LIFE_ANSWER_S),
            "an answer was left standing for ever"
        );
        // Asking again while it stands is one message with new words, exactly
        // as `raise` would have it.
        assert_eq!(
            n.insist("midi-in", Kind::Error, "No MIDI input ports found, still"),
            first,
            "a second asking minted a second message"
        );
        assert_eq!(n.len(), 1);
        assert_eq!(
            n.get(first).map(|x| x.text.as_str()),
            Some("No MIDI input ports found, still")
        );

        // The user takes it down. `raise` must stay quiet — that is its whole
        // contract and the reason a dismissal feels like a dismissal.
        assert!(n.dismiss(first).is_some());
        assert!(n.is_muted("midi-in"), "a dismissal was not remembered");
        assert_eq!(
            n.raise("midi-in", Kind::Error, "No MIDI input ports found"),
            None,
            "a re-check spoke over a dismissal"
        );

        // ...and the user asks again, by hand. That has to say something.
        let again = n.insist("midi-in", Kind::Error, "No MIDI input ports found");
        assert_ne!(again, first, "the dismissed message came back from the dead");
        assert!(!n.is_muted("midi-in"), "the mute survived the asking");
        assert!(
            n.get(again).is_some_and(|x| !x.leaving()),
            "the answer to an explicit request was not on screen"
        );
        assert_eq!(
            n.get(again).and_then(|x| x.expires_at()),
            Some(LIFE_ANSWER_S),
            "the second answer was left standing for ever"
        );

        // And a full column is not an answer either. `raise` waits for a slot
        // because five conditions rotating through four of them is a strobe;
        // one press is not a strobe, so it takes a slot the way any arrival
        // does and the cap retires the oldest.
        n.clear();
        for i in 0..VISIBLE {
            n.info(format!("Exported take {i}"));
        }
        let crowded = n.insist("midi-in", Kind::Error, "No MIDI input ports found");
        assert!(
            n.get(crowded).is_some_and(|x| !x.leaving()),
            "the answer to an explicit request waited for a slot that a menu \
             press will never come back for"
        );
        // Still the condition's own message, so the condition still ends it.
        assert!(n.lower("midi-in"));
        assert!(!n.is_raised("midi-in"));
    }

    /// **A message dismissed on its way OUT must not mute the next episode.**
    ///
    /// `lower` retires the note rather than deleting it — the tail is how
    /// everything here leaves — so for half a second after a condition clears
    /// there is still a box on screen wearing its key, and it is still
    /// clickable. Muting on that click wrote a mute for a key nothing had
    /// raised, and since only `lower` forgets a mute and the condition had
    /// already been lowered, the mute outlived everything: the device came
    /// back, went again, and said nothing at all.
    #[test]
    fn a_click_on_a_leaving_message_does_not_silence_the_next_one() {
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        let id = n
            .raise("dev", Kind::Error, "The input device disappeared")
            .expect("the fault said nothing at all");
        // The device comes back: the condition clears and the box starts to go.
        assert!(n.lower("dev"));
        assert!(!n.is_raised("dev"));
        assert!(
            n.get(id).is_some_and(Note::leaving),
            "a lowered message vanished between frames"
        );
        // The user clicks the fading box — "I have read it", which the fade
        // was already saying. It cannot mean "and not next time", because
        // there is no condition left for next time to be about.
        assert!(n.dismiss(id).is_some(), "the leaving box was not clickable");
        assert!(
            !n.is_muted("dev"),
            "dismissing a message whose condition had already cleared muted a \
             key nothing had raised"
        );
        // So the next episode is news, and says itself.
        advance(&mut n, &mut t, 1.0);
        let next = n.raise("dev", Kind::Error, "The input device disappeared");
        assert!(
            next.is_some(),
            "the next time the device went, the app said nothing"
        );
        // And a dismissal of a STANDING message still sticks, which is the
        // half of this that must not be lost.
        assert!(n.dismiss(next.expect("the fault said nothing")).is_some());
        assert!(n.is_muted("dev"), "a real dismissal stopped being remembered");
        assert_eq!(
            n.raise("dev", Kind::Error, "The input device disappeared"),
            None
        );
    }

    /// **`false` from `update` means gone, and the user is only one of the
    /// ways to be gone.**
    ///
    /// The doc said in bold that it meant the user took the message down. The
    /// cap can take it too — the hard ceiling drops what is leaving and then
    /// the oldest — so a progress report sitting still under a burst of device
    /// errors is exactly the note that disappears without anybody clicking
    /// anything.
    #[test]
    fn the_cap_can_be_the_reason_an_update_finds_nothing() {
        let mut n = Notifications::new();
        n.tick(0.0);
        let scan = n.info("Scanning plugins... 1 of 112");
        assert!(
            n.update(scan, "Scanning plugins... 2 of 112"),
            "the report was not there"
        );
        // A burst past the hard ceiling. Nobody clicked anything.
        for i in 0..20 {
            n.error(format!("input {i} went away"));
        }
        assert!(n.get(scan).is_none(), "the ceiling kept the progress note");
        assert!(
            !n.update(scan, "Scanning plugins... 40 of 112"),
            "update claimed to have reached a message the cap had dropped"
        );
    }

    /// **The cap's boundary survives a float round trip.**
    ///
    /// `retire` sets a life of `age + FADE_S` and `leaving` asks the question
    /// back. Asked as `age >= life - FADE_S` the subtraction does not return
    /// where it started — `(age + FADE_S) - FADE_S` lands a hair ABOVE `age`
    /// at about one age in twenty — so the message the cap had just retired
    /// reported itself WHOLE for one more frame and five stood where the rule
    /// says four. It only ever held at age zero, which is exactly where the
    /// cap's own test had always driven it.
    #[test]
    fn a_message_retired_mid_life_is_leaving_on_that_very_frame() {
        // The cap's real customer: a STANDING message, which has no clock of
        // its own, so `retire` sets the life outright and the round trip is
        // the whole of the arithmetic.
        for step in 0..20_000 {
            let age = f64::from(step) * 0.0007;
            let mut n = Note::standing(NoteId(0), Kind::Error, "x".to_owned(), "k".to_owned());
            n.age = age;
            n.stood = age;
            assert!(!n.leaving(), "a standing message was leaving at {age}");
            n.retire();
            assert!(n.leaving(), "retired at {age} and still reads whole");
            assert!(!n.expired(), "retiring at {age} killed it outright");
            assert_eq!(n.fade(), 1.0, "retired at {age} and jumped mid-fade");
        }
        // And a message that HAS a clock, retired before its own tail begins —
        // where `retire` shortens the life it was already carrying.
        for step in 0..5_000 {
            let age = f64::from(step) * 0.0007;
            if age >= LIFE_S - FADE_S {
                break;
            }
            let mut n = Note::brief(NoteId(0), Kind::Info, "x".to_owned());
            n.age = age;
            n.stood = age;
            n.retire();
            assert!(n.leaving(), "a brief message retired at {age} reads whole");
            assert!(!n.expired(), "retiring at {age} killed it outright");
            assert_eq!(n.fade(), 1.0, "retired at {age} and jumped mid-fade");
        }
        // And the same thing where it actually bites: a column capped after a
        // few frames of ageing rather than on the frame it was filled.
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        for i in 0..VISIBLE {
            n.info(format!("m{i}"));
        }
        // Four frames at sixty a second — an age the old subtraction lied
        // about.
        advance(&mut n, &mut t, 4.0 / 60.0);
        n.info("the fifth");
        assert_eq!(
            n.notes().iter().filter(|x| !x.leaving()).count(),
            VISIBLE,
            "five whole messages stood where the cap allows four"
        );
    }

    /// **The `push` door is honest about what its bound does and does not
    /// buy.**
    ///
    /// One NOTE is held to twice its life. The CALLER is not held to anything,
    /// and cannot be: this door finds its message by the words, and a memory
    /// of every text ever pushed would be keyed on the user's own filenames
    /// and would silence a receipt somebody really did earn twice. So a fault
    /// pushed every frame for a minute is a succession of messages, each one
    /// dismissable and none of them permanent — never a column, and never one
    /// box that cannot be got rid of. A caller who wants the message to STAND
    /// says so with `raise`, and this asserts the difference rather than
    /// describing it.
    #[test]
    fn a_per_frame_push_is_successive_messages_rather_than_one_unkillable_one() {
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        let mut minted = 0usize;
        let mut current: Option<NoteId> = None;
        for _ in 0..3_600 {
            t += 1.0 / 60.0;
            n.tick(t);
            let id = n.error("The input device disappeared");
            if current != Some(id) {
                minted += 1;
                current = Some(id);
            }
            assert_eq!(n.len(), 1, "a per-frame push stacked a column");
        }
        // Sixty seconds at twice an error's five is six, and the number is
        // asserted rather than bounded loosely because it IS the behaviour the
        // doc promises.
        assert_eq!(
            minted, 6,
            "sixty seconds of pushing the same fault minted {minted} messages"
        );
        // And whichever one is up right now goes when it is clicked.
        let id = n.error("The input device disappeared");
        assert!(n.dismiss(id).is_some());
        assert!(n.is_empty(), "a pushed message survived being dismissed");
        // The honest door for the same caller: one message, for as long as it
        // is true.
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        let first = n
            .raise("dev", Kind::Error, "The input device disappeared")
            .unwrap();
        for _ in 0..3_600 {
            t += 1.0 / 60.0;
            n.tick(t);
            assert_eq!(
                n.raise("dev", Kind::Error, "The input device disappeared"),
                Some(first),
                "the condition was re-minted"
            );
        }
        assert_eq!(n.len(), 1);
    }

    /// **A progress report outlives the scan it is reporting on.**
    ///
    /// `update` used to put the clock back through the DUPLICATE bound, which
    /// is a rule about the same words arriving twice and has nothing to say
    /// about a job still running: a plugin scan that took thirty seconds had
    /// its report expire underneath it at eight, and every count after that
    /// went into a message nobody could see while `update` still answered
    /// `true`. A caller holding the handle is the message's author, so the
    /// clock goes back every call — and `false` now means the one thing it
    /// should: the user took it down.
    #[test]
    fn a_progress_report_outlives_the_scan_it_is_reporting() {
        let mut n = Notifications::new();
        let mut t = 0.0_f64;
        n.tick(t);
        let id = n.info("Scanning plugins... 0 of 112");
        for i in 1..=112 {
            advance(&mut n, &mut t, 30.0 / 112.0);
            assert!(
                n.update(id, format!("Scanning plugins... {i} of 112")),
                "the report died at {i} of 112, {t:.1}s in"
            );
            assert_eq!(n.len(), 1, "the report stacked a copy at {i}");
        }
        assert!(t > LIFE_S * 6.0, "the scan was not long enough to prove it");
        assert_eq!(
            n.get(id).map(|x| x.text.as_str()),
            Some("Scanning plugins... 112 of 112")
        );
        // A stall on one slow plugin is still a scan that is running: the
        // words stop changing and the report stays up.
        for _ in 0..3 {
            advance(&mut n, &mut t, 2.0);
            assert!(
                n.update(id, "Scanning plugins... 112 of 112"),
                "a report whose words stopped moving expired"
            );
        }
        assert_eq!(n.len(), 1);
        // Let go of it and it goes, on the ordinary clock — this is not the
        // message with no owner the module refuses to have.
        advance(&mut n, &mut t, LIFE_S + 0.5);
        assert!(n.is_empty(), "a report nobody is updating any more stayed up");
        // `false`, and the only thing it means.
        let id = n.info("Rendering... 0%");
        assert!(n.update(id, "Rendering... 10%"));
        assert!(n.dismiss(id).is_some());
        assert!(
            !n.update(id, "Rendering... 20%"),
            "a dismissed report was still being written to"
        );
    }

    /// **A standing message is not motion.** It sits there at full strength, so
    /// it must not pin the app at sixty frames a second for as long as a device
    /// stays unplugged — but the moment it is given a tail, it wants its frames
    /// back.
    #[test]
    fn a_standing_message_does_not_ask_for_repaints() {
        let mut n = Notifications::new();
        let id = n.raise("dev", Kind::Error, "The input device disappeared");
        let mut t = 0.0;
        assert!(!n.tick(t), "a standing message asked for a repaint");
        advance(&mut n, &mut t, 5.0);
        assert_eq!(n.len(), 1, "a standing message aged out");
        assert_eq!(n.get(id.unwrap()).map(Note::fade), Some(1.0));
        // A brief one beside it does want them.
        n.info("saved");
        t += 1.0 / 60.0;
        assert!(n.tick(t), "a live message wants repaints");
        // And so does the standing one, once it is leaving.
        advance(&mut n, &mut t, 5.0);
        assert!(!n.tick(t), "only the standing one is left, and it is still");
        n.lower("dev");
        t += 1.0 / 60.0;
        assert!(n.tick(t), "a message mid-fade stopped asking for frames");
    }

    /// **A progress report is one message whose words change.**
    ///
    /// A hundred and twelve plugins scanned is one line that counts up, not a
    /// hundred and twelve lines, and not four lines flickering as the cap eats
    /// them. Both doors do it: `update` by handle, and re-raising the same
    /// condition with new words.
    #[test]
    fn a_progressive_message_changes_in_place() {
        let mut n = Notifications::new();
        let id = n
            .raise("scan", Kind::Info, "Scanning plugins... 0 of 112")
            .unwrap();
        let mut t = 0.0_f64;
        for i in 1..=112 {
            t += 1.0 / 60.0;
            n.tick(t);
            assert_eq!(
                n.raise("scan", Kind::Info, format!("Scanning plugins... {i} of 112")),
                Some(id),
                "the report was re-minted at {i}"
            );
            assert_eq!(n.len(), 1, "the report stacked a copy at {i}");
        }
        assert_eq!(
            n.get(id).map(|x| x.text.as_str()),
            Some("Scanning plugins... 112 of 112")
        );
        // The other door: by handle, on a terminal message, which treats new
        // words as news and gives itself its clock back.
        let mut n = Notifications::new();
        let id = n.info("Rendering... 0%");
        let mut t = 0.0_f64;
        n.tick(t);
        advance(&mut n, &mut t, 2.0);
        assert!(n.update(id, "Rendering... 50%"));
        assert_eq!(n.len(), 1);
        assert!(n.get(id).is_some_and(|x| x.age < 0.001), "the words changed and the clock did not");
        assert_eq!(n.get(id).map(|x| x.text.as_str()), Some("Rendering... 50%"));
        // And an update that puts a retired message back on its feet is
        // counted, the same as a duplicate push is.
        let mut n = Notifications::new();
        let ids: Vec<_> = (0..5).map(|i| n.info(format!("m{i}"))).collect();
        assert!(n.notes()[0].leaving(), "the probe needs a leaving message");
        assert!(n.update(ids[0], "m0, now with news"));
        assert_eq!(
            n.notes().iter().filter(|x| !x.leaving()).count(),
            VISIBLE,
            "an update refreshed its way past the cap"
        );
        // Dismissed, the caller is told rather than left talking to itself.
        let mut n = Notifications::new();
        let id = n.info("Rendering... 0%");
        n.tick(0.0);
        n.dismiss(id);
        assert!(!n.update(id, "Rendering... 90%"), "updated a dead message");
        assert!(n.is_empty());
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
        assert!(!d.boxes.is_empty(), "ninety points fitted no box at all");
        for b in &d.boxes {
            assert!(
                short.contains_rect(b.rect),
                "clipped rather than dropped: {b:?}"
            );
        }
    }

    /// **A box never hangs off the right edge of its window.**
    ///
    /// `MIN_WIDTH` is a squeeze, not a promise the window is that wide: at 160
    /// points plus two 10-point margins the column needs 180, and every window
    /// narrower than that used to get a box running off the side of it. The
    /// test that should have caught it asserted "every box drawn is inside the
    /// area" over a window where no box was drawn at all, which is true of
    /// nothing.
    #[test]
    fn a_narrow_window_gets_no_column_rather_than_one_hanging_off_it() {
        assert!(
            MIN_WIDTH > PAD_X * 2.0 + ACCENT_W,
            "the narrowest box has no room for its own text"
        );
        let mut n = Notifications::new();
        n.info("saved");
        let st = Style::of(Color32::from_rgb(0x2A, 0x24, 0x1E));
        let mut drew_at = None;
        for w in 0_u16..=400 {
            let w = f32::from(w);
            let area = Rect::from_min_size(Pos2::ZERO, Vec2::new(w, 400.0));
            let d = with_painter(|p| draw(p, area, &n, &st));
            for b in &d.boxes {
                assert!(
                    area.contains_rect(b.rect),
                    "a {w}-point window drew a box at {:?}",
                    b.rect
                );
                assert!(
                    b.rect.width() >= MIN_WIDTH,
                    "a {w}-point window drew a {}-point box",
                    b.rect.width()
                );
            }
            if !d.boxes.is_empty() && drew_at.is_none() {
                drew_at = Some(w);
            }
            assert_eq!(
                d.boxes.is_empty(),
                w < MIN_WINDOW_W,
                "a {w}-point window disagreed about whether it has room"
            );
        }
        assert_eq!(drew_at, Some(MIN_WINDOW_W));
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
            assert!(
                d.boxes.is_empty(),
                "{r:?} is not a window a message fits in, and one was drawn"
            );
        }
        // And an empty queue is happy to be ticked forever.
        let mut n = Notifications::new();
        for i in 0..10 {
            assert!(!n.tick(f64::from(i)));
        }
        assert!(n.dismiss(NoteId(99)).is_none());
        n.clear();
    }

    /// A frame of the real thing, driven at the event level.
    fn run(
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        notes: &mut Notifications,
        under: Option<&std::cell::Cell<bool>>,
    ) -> Shown {
        let mut out = Shown::default();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(SCREEN),
                events,
                ..Default::default()
            },
            |ctx| {
                if let Some(under) = under {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let r = ui.interact(SCREEN, egui::Id::new("the-desk"), egui::Sense::CLICK);
                        if r.clicked() {
                            under.set(true);
                        }
                    });
                }
                out = show(ctx, SCREEN, notes, Color32::from_rgb(0x2A, 0x24, 0x1E));
            },
        );
        out
    }

    fn press_at(at: Pos2, pressed: bool) -> egui::Event {
        egui::Event::PointerButton {
            pos: at,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        }
    }

    fn click_at(at: Pos2) -> Vec<egui::Event> {
        vec![
            egui::Event::PointerMoved(at),
            press_at(at, true),
            press_at(at, false),
        ]
    }

    /// **The Area takes the click, and the click takes the message down.**
    /// Driven at the event level through a real context, because "it does not
    /// fall through" is a claim about egui's layers and not about arithmetic.
    ///
    /// And the click hands back the message WHOLE: the box showed five elided
    /// rows of a ten-line fault, the queue no longer has it, and the caller
    /// that wants to log the rest gets exactly this one frame to be handed it.
    #[test]
    fn a_click_on_a_box_dismisses_it_and_hands_it_back_whole() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        let long = (1..=10)
            .map(|i| format!("Line {i}: the interface reported a fault on this channel"))
            .collect::<Vec<_>>()
            .join("\n");
        let id = notes.error(long.clone());
        // A frame to lay it out and learn where it is.
        let first = run(&ctx, vec![], &mut notes, None);
        assert!(first.wants_repaint, "a live message wants repaints");
        assert_eq!(first.boxes.len(), 1);
        assert!(first.boxes[0].elided, "the box showed the whole ten lines");
        let at = first.boxes[0].rect.center();
        // Move onto it, then press and release.
        let moved = run(&ctx, vec![egui::Event::PointerMoved(at)], &mut notes, None);
        assert!(moved.dismissed.is_none());
        let click = run(&ctx, click_at(at), &mut notes, None);
        let taken = click.dismissed.expect("the click missed its own box");
        assert_eq!(taken.id, id);
        assert_eq!(taken.text, long, "the full text died with the box");
        assert_eq!(taken.kind, Kind::Error);
        assert!(notes.is_empty(), "dismissed and still in the queue");
        assert!(notes.get(id).is_none(), "the note the caller was handed is still in the queue");
        // Nothing up: nothing wants a repaint, and nothing panics.
        let quiet = run(&ctx, vec![], &mut notes, None);
        assert!(!quiet.wants_repaint);
        assert!(quiet.boxes.is_empty());
    }

    /// A click well away from the column is not ours, and the message stays.
    #[test]
    fn a_click_elsewhere_leaves_the_message_alone() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        notes.info("still here");
        let away = Pos2::new(SCREEN.right() - 40.0, SCREEN.bottom() - 40.0);
        let mut out = Shown::default();
        for _ in 0..2 {
            out = run(&ctx, click_at(away), &mut notes, None);
        }
        assert!(
            out.dismissed.is_none(),
            "a click on the desk took a message down"
        );
        assert_eq!(notes.len(), 1);
    }

    /// **A click that hits no box reaches what is underneath.**
    ///
    /// One Area sized to the whole column owned the six-point gaps between the
    /// boxes as surely as it owned the boxes: `layer_id_at` handed the pointer
    /// to the notification layer, `Drawn::hit` reported a miss, and the press
    /// aimed at the arrangement through the gap landed on nothing at all. Two
    /// probes, because they fail differently: the layer the pointer belongs to
    /// (what `rect_contains_pointer` and `is_pointer_over_area` ask) and a real
    /// widget underneath actually being clicked.
    #[test]
    fn a_click_in_the_gap_reaches_what_is_underneath() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        notes.info("first");
        notes.error("second");
        let under = std::cell::Cell::new(false);
        let first = run(&ctx, vec![], &mut notes, Some(&under));
        assert_eq!(first.boxes.len(), 2);
        let (top, below) = (first.boxes[0].rect, first.boxes[1].rect);
        let gap = Pos2::new(top.center().x, (top.bottom() + below.top()) * 0.5);
        assert!(
            !top.contains(gap) && !below.contains(gap),
            "the probe is not in the gap"
        );
        // The pointer in the gap belongs to the panel, not to a notification.
        let notify_layers: Vec<egui::LayerId> = (0..=VISIBLE)
            .map(|slot| {
                egui::LayerId::new(egui::Order::Foreground, egui::Id::new((AREA_ID, slot)))
            })
            .collect();
        let _ = run(&ctx, vec![egui::Event::PointerMoved(gap)], &mut notes, Some(&under));
        let at_gap = ctx.layer_id_at(gap);
        assert!(
            at_gap.is_none_or(|l| !notify_layers.contains(&l)),
            "the gap belongs to {at_gap:?}, a notification layer"
        );
        // And the click gets there.
        under.set(false);
        let missed = run(&ctx, click_at(gap), &mut notes, Some(&under));
        assert!(missed.dismissed.is_none(), "a gap click dismissed a message");
        assert!(under.get(), "the click in the gap reached nothing");
        // The box itself still belongs to the notification, and the desk does
        // not also get that press.
        under.set(false);
        let hit = run(&ctx, click_at(top.center()), &mut notes, Some(&under));
        assert!(hit.dismissed.is_some(), "a click on a box missed it");
        assert!(!under.get(), "one click both dismissed and hit the desk");
    }

    /// **A message never takes the keyboard.**
    ///
    /// `Sense::click()` is `CLICK | FOCUSABLE` in egui 0.33, which puts every
    /// box in the Tab order: a notification that evaporates four seconds later
    /// is not a control, and one holding the caret while somebody types in a
    /// filter field is the exact thing this module's header promises not to do.
    #[test]
    fn a_notification_never_takes_the_keyboard() {
        let ctx = ctx();
        let mut notes = Notifications::new();
        notes.info("first");
        notes.error("second");
        let first = run(&ctx, vec![], &mut notes, None);
        assert_eq!(first.boxes.len(), 2);
        let ids: Vec<egui::Id> = first
            .boxes
            .iter()
            .map(|p| box_id(p.id))
            .collect();
        let tab = egui::Event::Key {
            key: egui::Key::Tab,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::default(),
        };
        for _ in 0..6 {
            let _ = run(&ctx, vec![tab.clone()], &mut notes, None);
            let focused = ctx.memory(|m| m.focused());
            assert!(
                focused.is_none_or(|f| !ids.contains(&f)),
                "Tab landed the caret on a notification"
            );
        }
        // Clicking one does not put the caret there either.
        let at = first.boxes[0].rect.center();
        let _ = run(&ctx, click_at(at), &mut notes, None);
        let focused = ctx.memory(|m| m.focused());
        assert!(
            focused.is_none_or(|f| !ids.contains(&f)),
            "a click put the caret on a notification"
        );
    }
}
