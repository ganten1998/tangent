//! The seam between "something hands us a window" and "we draw into it".
//!
//! Three different hosts call into this app and all three hand over an
//! `egui::Context` rather than a `Ui`:
//!
//!   * `eframe::App::update` for the desktop window,
//!   * `Context::show_viewport_immediate` for each child window, and
//!   * `nih_plug_egui`'s editor callback for the VST3 build.
//!
//! Every drawing surface here paints absolutely into `max_rect` and owns its
//! whole window, so the bridge is the same in all three cases and costs
//! nothing: one central panel with no frame, no margin and no fill.
//!
//! Keeping it in one place matters more than it looks. The desktop and plugin
//! builds have to render identically, and the fastest way to end up with two
//! subtly different GUIs is to let each entry point grow its own idea of what
//! the root panel should be.

use crate::host::Caps;
use crate::recorder_panel::{legible_surface, surface_of};
use egui::{Align2, Color32, FontId, Pos2, Rect, Stroke, StrokeKind, Vec2};

/// Run `add` against a `Ui` covering the whole of `ctx`'s viewport.
pub fn viewport_ui<R>(ctx: &egui::Context, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ctx, add)
        .inner
}

// ── the in-house window dress ───────────────────────────────────────────────

/// The dress of a window this app draws its own bar on: the suite faces,
/// the Keys panel, and the pickers they open.
///
/// **Why the app draws these itself.** They are the windows a person works
/// IN while the desk stays live behind them, and an OS title bar is the one
/// thing on screen the theme cannot reach — nineteen themes on the desk and
/// one grey Aqua strip over every face. A bar of our own follows the theme
/// on the same frame the desk does, is the same bar on every platform, and
/// is the same bar in the plugin build's canvas, where there is no OS bar
/// to borrow at all.
///
/// Three colours, each MEASURED. The bar is the body's raised shade made
/// able to carry ink of its own (`legible_surface`); the ink is what
/// `surface_of` puts on THAT ground — never the body's ink handed down,
/// which is the way this app has learned the same lesson from a screenshot
/// seven times; the edge is the body's ink, so the window separates from
/// whatever it is floating over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chrome {
    pub bar: Color32,
    pub ink: Color32,
    pub edge: Color32,
}

impl Chrome {
    /// Dress for a window whose body is `face`.
    pub fn of(face: Color32) -> Chrome {
        let body = surface_of(face);
        let bar = legible_surface(body.raised);
        Chrome {
            bar,
            ink: surface_of(bar).ink,
            edge: body.ink,
        }
    }
}

/// The bar's height. Twenty-four points: a 12-point title with air above
/// and below, and a close cross whose square is a target a hand finds the
/// first time rather than the third.
pub const CHROME_BAR_H: f32 = 24.0;
/// The resize grip's square, in the bottom-right corner.
pub const CHROME_GRIP: f32 = 14.0;

/// Where the chrome's parts are. ONE function for both paths and every
/// test, so the cross that is drawn is the cross that is hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ChromeLayout {
    pub bar: Rect,
    /// Where the title is written: the drag rect less a little air.
    pub title: Rect,
    /// The close cross's target: a full-height square at the bar's right.
    pub close: Rect,
    /// A press here moves the window. The bar WITHOUT the close square —
    /// the two must not overlap, or a press on the cross starts a drag that
    /// swallows the click.
    pub drag: Rect,
    /// The resize grip, only when the window can be resized.
    pub grip: Option<Rect>,
    /// What is left for the body.
    pub body: Rect,
}

pub fn chrome_layout(rect: Rect, resizable: bool) -> ChromeLayout {
    let h = CHROME_BAR_H.min(rect.height());
    let bar = Rect::from_min_size(rect.min, Vec2::new(rect.width(), h));
    let close = Rect::from_min_size(
        Pos2::new((bar.right() - h).max(bar.left()), bar.top()),
        Vec2::splat(h),
    );
    let drag = Rect::from_min_max(bar.min, Pos2::new(close.left(), bar.bottom()));
    let title = Rect::from_min_max(
        Pos2::new(drag.left() + 8.0, drag.top()),
        Pos2::new((drag.right() - 4.0).max(drag.left() + 8.0), drag.bottom()),
    );
    let body = Rect::from_min_max(Pos2::new(rect.left(), bar.bottom()), rect.max);
    let grip = resizable.then(|| {
        Rect::from_min_size(rect.max - Vec2::splat(CHROME_GRIP), Vec2::splat(CHROME_GRIP))
    });
    ChromeLayout {
        bar,
        title,
        close,
        drag,
        grip,
        body,
    }
}

/// What the bar saw this frame. The caller decides what a move MEANS — an
/// OS drag on the desktop, a shift of the layer inline — because that is
/// the one thing the two paths cannot share.
#[derive(Clone, Copy, Default, Debug)]
struct ChromeGesture {
    close: bool,
    cycle_theme: bool,
    /// The TRANSPORT key, pressed in a plugin window. See
    /// [`SurfaceReport::transport`].
    transport: bool,
    /// The primary button went down on the drag rect this frame.
    grab: bool,
    /// How far a held bar moved this frame.
    moved: Vec2,
}

/// The bar, the title, the cross — and what the hand did to them.
fn draw_chrome_bar(
    ui: &mut egui::Ui,
    id: egui::Id,
    title: &str,
    l: &ChromeLayout,
    c: Chrome,
) -> ChromeGesture {
    let mut g = ChromeGesture::default();
    // Interactions before paint, so a hover shows on the frame it happens.
    let close = ui.interact(l.close, id.with("close"), egui::Sense::click());
    // `drag`, not `click_and_drag`: a widget that senses only a drag starts
    // it on the press, with no threshold to cross first, which is what makes
    // an inline layer follow the hand from the first point it moves.
    let drag = ui.interact(l.drag, id.with("drag"), egui::Sense::drag());
    let painter = ui.painter().with_clip_rect(l.bar);
    painter.rect_filled(l.bar, 0.0, c.bar);
    painter.with_clip_rect(l.title).text(
        l.title.left_center(),
        Align2::LEFT_CENTER,
        title,
        FontId::new(12.0, crate::fonts::courier_bold()),
        c.ink,
    );
    if close.hovered() {
        painter.rect_filled(l.close, 0.0, c.ink.gamma_multiply(0.22));
    }
    let x = l.close.shrink(l.close.height() * 0.34);
    let w = if close.hovered() { 2.0_f32 } else { 1.5_f32 };
    painter.line_segment([x.left_top(), x.right_bottom()], Stroke::new(w, c.ink));
    painter.line_segment([x.right_top(), x.left_bottom()], Stroke::new(w, c.ink));
    g.close = close.clicked();
    // **The PRESS, not the drag threshold.** A window that only starts to
    // follow the hand after it has travelled a few points feels stuck to
    // the desk; the borderless main window goes on the press for the same
    // reason.
    g.grab = ui.input(|i| {
        i.pointer.primary_pressed()
            && i.pointer.interact_pos().is_some_and(|p| l.drag.contains(p))
    });
    // Not on the frame the drag STARTS: a drag that begins on the press
    // reports that frame's whole pointer motion as its first delta, and
    // that motion is the hand arriving at the bar, not moving it.
    if drag.dragged_by(egui::PointerButton::Primary) && !drag.drag_started() {
        g.moved = drag.drag_delta();
    }
    // **The keys this window forwards, exactly as the main window reads
    // them** — the keymap is the one table, so the bar does not grow a
    // second idea of what a letter means. Not while a field in this window
    // is being typed into: "t" in the picker's filter is a letter, and the
    // main window has the same rule under `text_entry_active`.
    //
    // Asked ONCE, because `keys::pressed` answers with the single action
    // this frame's press means and two calls would be two answers to one
    // question.
    //
    // **Asked with the transport UNGATED**, and the app re-checks. The
    // table gates Space on the recorder band being shown
    // (`keys::available`), which is state this bar cannot see - asking with
    // the default gates would simply never recognise the key and the
    // forwarding would be a silent no-op. So the bar answers "which key",
    // the app answers "is it allowed", which is where that state lives.
    let key = (!ui.ctx().wants_keyboard_input())
        .then(|| {
            crate::keys::pressed(
                ui.ctx(),
                crate::keys::Gates { recorder_shown: true, ..Default::default() },
            )
        })
        .flatten();
    g.cycle_theme = key == Some(crate::keys::KeyAction::CycleTheme);
    // **And the TRANSPORT**, which is what makes a window you leave open
    // usable. The quantize window is not modal on purpose - "select a clip,
    // press Apply, LISTEN, select another" - and on the desktop it is a real
    // child window, so while it has focus the main one reports `!focused`
    // and `dialog_blocks_keys` swallowed every key including Space. The
    // owner: "The quantize menu doesn't allow playback while it's open."
    g.transport = key == Some(crate::keys::KeyAction::StopRecording);
    g
}

/// The hairline round the window and the grip in its corner, painted AFTER
/// the body so the body's own ground fill cannot cover them. Returns how
/// far a held grip moved this frame.
fn draw_chrome_edge(ui: &mut egui::Ui, id: egui::Id, rect: Rect, l: &ChromeLayout, c: Chrome) -> Vec2 {
    let mut resized = Vec2::ZERO;
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_stroke(rect, 0.0, Stroke::new(1.0_f32, c.edge), StrokeKind::Inside);
    if let Some(grip) = l.grip {
        let r = ui.interact(grip, id.with("grip"), egui::Sense::drag());
        // The start frame's delta is the hand arriving, as on the bar.
        if r.dragged_by(egui::PointerButton::Primary) && !r.drag_started() {
            resized = r.drag_delta();
        }
        if r.hovered() || r.dragged() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeNwSe);
        }
        // Three diagonal hairlines: the grip every desk app draws, in the
        // edge's own ink so it reads as part of the frame.
        for k in [4.0_f32, 8.0, 12.0] {
            painter.line_segment(
                [
                    Pos2::new(grip.right() - k, grip.bottom() - 1.0),
                    Pos2::new(grip.right() - 1.0, grip.bottom() - k),
                ],
                Stroke::new(1.0_f32, c.edge),
            );
        }
    }
    resized
}

/// What an inline window remembers between frames: how far its bar has
/// been dragged and what size the grip left it at.
///
/// Dropped the moment the surface is not drawn for a pass — which is how
/// a window that closed and a different one that opened under the same id
/// come up centred and at their own size, exactly as the desktop's fresh
/// OS window does.
#[derive(Clone, Copy, Default)]
struct InlineChromeState {
    last_pass: u64,
    moved: Vec2,
    size: Option<Vec2>,
}

fn inline_rect_id(id: &str) -> egui::Id {
    egui::Id::new((id, "inline-rect"))
}

/// Where an inline surface was drawn on the last pass, if it was.
///
/// **For the app's raw-pointer handlers, which egui's layers cannot
/// shield.** A widget under a layer is protected by egui's own hit test,
/// but half of this app reads `i.pointer` directly, and a non-modal dialog
/// drawn in the canvas would otherwise hand every press on its face to the
/// mixer underneath it. On the desktop the dialog is another window with a
/// pointer stream of its own and nothing is ever stored, so `None` is the
/// honest answer there too.
pub fn inline_rect(ctx: &egui::Context, id: &str) -> Option<Rect> {
    let pass = ctx.cumulative_pass_nr();
    ctx.memory(|m| m.data.get_temp::<(u64, Rect)>(inline_rect_id(id)))
        .filter(|(last, _)| pass.saturating_sub(*last) <= 1)
        .map(|(_, r)| r)
}

// ── how high a surface sits ─────────────────────────────────────────────────

/// **How high a surface sits among this app's OWN windows.**
///
/// Every child surface here asks to float, and inside one window level the
/// order is by most-recent order-front — `makeKeyAndOrderFront:` on macOS,
/// `SetWindowPos` on Windows, a map request on X11. So whichever surface is
/// revealed or focused LAST is the one in front, and a window that has to
/// stay visible for the length of a render cannot win a race it re-runs
/// every time anything else opens. The export window was losing it to the
/// band's own "Rendering at full speed…" notice, which is born on the same
/// frame and reveals on the next.
///
/// **The rule is an announcement against a question, and it is a LADDER of
/// three, not a flag.** The first draft of this was a flag — one surface
/// said "nothing of ours covers me" and every other surface of the app went
/// down — and that put a modal dialog opened mid-render UNDERNEATH the
/// render. This app enforces modality by dropping every main-window event
/// while a question is open (`App::handle_main_interaction` returns early),
/// so a question pinned under anything is an invisible question with the app
/// frozen behind it: strictly worse than the covering the flag was fixing.
///
/// So a surface that merely ANNOUNCES — a notice, a menu, a tooltip, a
/// plugin window left open — may be held down under one that is being
/// watched. A surface that is ASKING may not, ever. The declaration order
/// below IS the ladder (the `Ord` derive reads it), and a surface floats
/// unless some OTHER surface holds a STRICTLY higher rung.
///
/// **There is no fourth window level to promote into.** `winit` exposes
/// exactly three (`AlwaysOnBottom`, `Normal`, `AlwaysOnTop`) and `egui`
/// passes them straight through, so "put this one higher" is not a thing
/// that can be said. The ladder is therefore expressed from the other end:
/// the highest rung in play keeps the floating level and everything below it
/// is put DOWN to `Normal` while it is up. All three window managers order
/// strictly by level first and front-to-back only WITHIN a level, so a
/// sibling revealed later cannot come out in front however loudly it orders
/// itself forward. It is a rule rather than a race, which is the whole
/// difference: re-asserting the top surface's own level on every sibling
/// reveal would be the same race run once more, and `-[NSWindow setLevel:]`
/// with the level it already has is not documented to reorder anything.
///
/// **What it costs.** A demoted surface shares the main window's level, so a
/// press on the main window can put it behind — which is exactly the case
/// [`SurfaceSpec::raise`] already exists to answer, and the app already asks
/// for a raise on that press. Note which way round that falls now: it is the
/// render that goes down under a question, and the render is the one thing
/// here nobody is waiting on. On Windows the two levels are `HWND_TOPMOST`
/// and `HWND_NOTOPMOST` and behave the same way. On X11 the level is
/// `_NET_WM_STATE_ABOVE`, which a window manager MAY ignore: there a demoted
/// surface can still land in front, and the `_NET_WM_WINDOW_TYPE` hint is
/// the only other lever. That is a WM already ignoring the float every
/// dialog in this app asks for, so it is not a new failure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum SurfaceTier {
    /// **It announces, or it is merely open.** Notices, menus, tooltips, the
    /// suite's plugin windows. What everything here was before there was a
    /// tier, and what everything still is unless it says otherwise — none of
    /// it is a question, and none of it is worth covering a render for.
    #[default]
    Ordinary,
    /// **Nothing that merely announces may cover it while it is up.**
    ///
    /// One customer: the export window, which reports on a render that can
    /// run for minutes and is worth nothing the moment something lands on
    /// top of it. It does NOT outrank a question — see [`Self::Asking`],
    /// which is the whole point of the rung above.
    Uncovered,
    /// **It is asking the user something, and it always comes forward.**
    ///
    /// The modal dialogs. `App::handle_main_interaction` drops every event
    /// on the main window while one is open, so this is the one surface in
    /// the app that must never be behind anything: a question the user
    /// cannot see is an app that has stopped responding for no visible
    /// reason, and this file has fixed that twice already by other means.
    /// A render is worth protecting from an announcement and is not worth
    /// protecting from a question.
    Asking,
}

impl SurfaceTier {
    /// The inline equivalent. A layer has no window level, so the ladder is
    /// egui's own paint order, which — unlike a window level — is absolute:
    /// a later `Area` cannot get in front of one in a higher order.
    ///
    /// **Only the RENDER moves inline, and it moves both ways.** A question
    /// has to stay in `Foreground`, the band every menu and dialog here has
    /// always used, because its own furniture is drawn relative to it: a
    /// `ComboBox` opens its list in `Foreground` and `on_hover_text` draws
    /// in `Tooltip`, so a dialog promoted above those is a dialog whose
    /// dropdowns open behind it and whose hover text never appears. About's
    /// "Click to copy the address" is exactly that, and About is modal.
    ///
    /// So the render takes `Tooltip` — one band above every announcement,
    /// which is all it needs — and STANDS DOWN to `Middle` when a question
    /// is up, which is the same ladder read from the other end. Middle is
    /// where the modal scrim already lives; the render lands above the scrim
    /// rather than under it, on purpose, because dimming the one window that
    /// says the app is busy is the invisible-progress bug again.
    ///
    /// `by_others` is the highest rung anybody ELSE is standing on — the
    /// same answer [`level_for`] is given, so the two paths cannot disagree
    /// about who is on top.
    fn inline_order(self, asked: egui::Order, by_others: Option<Self>) -> egui::Order {
        match self {
            Self::Ordinary | Self::Asking => asked,
            Self::Uncovered if by_others.is_some_and(|other| other > self) => {
                egui::Order::Middle
            }
            Self::Uncovered => egui::Order::Tooltip,
        }
    }
}

fn tier_claim_id() -> egui::Id {
    egui::Id::new("surface-tier-claim")
}

/// The highest rung any of this app's surfaces is standing on, and who is
/// standing on it.
///
/// **One slot, holding the MAXIMUM.** A surface only ever needs to know
/// whether somebody ELSE outranks it, and the slot always holds the highest
/// claim in play, so "the slot is somebody else's" answers it. The case
/// that looks lost — the slot is mine, and I cannot see who is second —
/// cannot matter: anybody above me would have taken the slot from me.
///
/// **Accepted one pass stale, on purpose.** The surfaces of a frame are
/// drawn in whatever order the app happens to call them, and the export
/// window is drawn LAST — after the dialogs, after the notice. A claim good
/// only on the pass it was written would never be seen by the very surfaces
/// it exists to rank against. One frame late is the right answer instead:
/// a claim appears a frame after the surface opens and lapses a frame after
/// it closes, and every surface that makes one repaints continuously.
fn standing_claim(ctx: &egui::Context, pass: u64) -> Option<(SurfaceTier, egui::Id)> {
    ctx.memory(|m| m.data.get_temp::<(u64, SurfaceTier, egui::Id)>(tier_claim_id()))
        .filter(|(p, _, _)| pass.saturating_sub(*p) <= 1)
        .map(|(_, tier, who)| (tier, who))
}

/// **The window level a surface asks for**, given its own rung and the
/// highest rung anybody ELSE is standing on. See [`SurfaceTier`].
///
/// A pure table so the decision can be read and tested without a window
/// manager: everything about the ladder that can be checked headlessly is
/// checked here, and the ordering it produces on a real screen is not
/// something a unit test may claim to have proved.
///
/// Strictly higher, never equal: two surfaces on the same rung must not put
/// each other down, and the app is built so there is only ever one of each
/// rung above `Ordinary` — one export window, one dialog slot. `surface`
/// asserts that rather than trusting it.
fn level_for(
    on_top: bool,
    tier: SurfaceTier,
    by_others: Option<SurfaceTier>,
) -> egui::WindowLevel {
    let outranked = by_others.is_some_and(|other| other > tier);
    if on_top && !outranked {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    }
}

/// **When a surface goes on screen, and which pass counts as its reveal.**
///
/// Returns `(visible, revealing)`. A window is created VISIBLE unless told
/// otherwise, and on Windows a window that exists before anything has been
/// drawn into it shows one frame of undefined back buffer — white, in
/// practice. So everything here is born hidden and shown a pass later, which
/// is what eframe does for the main window (egui#3631, "to fix white flash
/// on startup") and what a child viewport gets none of.
///
/// `born_visible` is the exemption, and it moves the REVEAL forward with the
/// showing rather than dropping it: the level and the focus are creation-time
/// attributes applied to a window the manager has not mapped yet, they do not
/// survive being shown on X11, and a surface that skipped its reveal would
/// come up unable to hear the Escape that closes it. See
/// [`SurfaceSpec::born_visible`] for why anything would want to pay the flash.
///
/// A pure table, because the alternative is proving it through a window
/// manager that no headless test can ask.
fn birth(born_visible: bool, existing: bool, second_pass: bool) -> (bool, bool) {
    let revealing = if born_visible {
        !existing
    } else {
        existing && second_pass
    };
    (existing || born_visible, revealing)
}

/// **A window sized as a share of the screen it opens on.**
///
/// The rule `app::main_width` uses for the main window, said once so the
/// next window to want it does not open-code it again: a percentage of the
/// display, floored so it stays usable on a laptop, capped so it does not
/// eat a 6K, and finally held inside the screen itself — because the floor
/// can outgrow a small display, and a window bigger than the screen it opens
/// on is the bug the 5.0 demo session found in the main window.
///
/// A fixed point count is a percentage of nothing in particular: it is
/// comfortable on the display the app was written on and a postage stamp
/// everywhere else. `monitor` is `None` on the frames before anything has
/// been measured, and the floor is the honest answer there — a guess about
/// a screen nobody has looked at is worse than the smallest size the layout
/// is designed for.
pub fn share_of_screen(monitor: Option<Vec2>, share: Vec2, floor: Vec2, ceiling: Vec2) -> Vec2 {
    debug_assert!(
        floor.x <= ceiling.x && floor.y <= ceiling.y,
        "a floor {floor:?} above the ceiling {ceiling:?} is not a range"
    );
    let Some(m) = monitor.filter(|m| m.x > 0.0 && m.y > 0.0) else {
        return floor;
    };
    let want = Vec2::new(
        (m.x * share.x).clamp(floor.x, ceiling.x),
        (m.y * share.y).clamp(floor.y, ceiling.y),
    );
    crate::settings::clamp_size_to_monitor(want, Some(m))
}

/// A pane that is an OS window on the desktop and a floating layer in a plugin.
///
/// The fields are exactly the union of what the real callers set on a
/// `ViewportBuilder`; nothing is here speculatively. `id` is used for BOTH the
/// viewport id and the inline `Area` id, so the two paths cannot drift apart by
/// one of them being renamed.
pub struct SurfaceSpec<'a> {
    /// Stable identity. Changing one of these strings resets a window's
    /// remembered geometry, so they are written down once and left alone.
    pub id: &'a str,
    pub title: &'a str,
    pub size: Vec2,
    pub min_size: Vec2,
    /// Top-left. Monitor points on the desktop, canvas points inline. None
    /// means centre it: the desktop lets the OS place the window, inline
    /// centres on the canvas.
    pub pos: Option<Pos2>,
    pub decorated: bool,
    pub resizable: bool,
    /// Whether the surface takes key focus. The submenu passes false so it
    /// cannot steal focus from the menu and close it.
    pub takes_focus: bool,
    /// Dim and block everything underneath. Dialogs only, and only inline —
    /// on the desktop modality is enforced by the app dropping input, because
    /// there is nothing to dim.
    pub modal: bool,
    /// Inline stacking. The submenu must sit above the menu.
    pub order: egui::Order,
    /// Whether the surface floats above ordinary windows.
    ///
    /// **True for everything, except while a native file panel is up.** A
    /// modal dialog has to float: this app enforces modality by dropping input,
    /// so a dialog that ends up behind the main window is an app that has
    /// frozen with nothing on screen saying why — that failure has been fixed
    /// twice already. But a floating dialog also floats above the OS's own file
    /// panel, which is parented to the MAIN window, and then "Load bank..."
    /// appears to do nothing at all. So it is a flag rather than a constant,
    /// and the one moment it goes false is the one moment it must.
    pub on_top: bool,
    /// Come to the front NOW, whatever the window manager decided earlier.
    ///
    /// **Set when the user presses the window this one is modal over.** A
    /// modal dialog behind its parent is an app that has stopped responding
    /// with no visible reason why — `handle_main_interaction` drops every
    /// event while one is open — and `AlwaysOnTop` is a request some window
    /// managers ignore. A press on the parent is the moment the user is
    /// wondering where their click went, so it is the moment to answer.
    pub raise: bool,
    /// The `_NET_WM_WINDOW_TYPE` hint, X11 only, ignored everywhere else.
    ///
    /// This is what tells a Linux window manager what the window IS. It
    /// matters most under tiling WMs (i3, sway): a `Normal` resizable window
    /// gets TILED into the layout — which for a modal dialog means it opens
    /// as a random pane UNDERNEATH the floating main window while the app
    /// ignores all input waiting on it, i.e. the app appears frozen. `Dialog`
    /// and `PopupMenu` windows are floated by every WM that reads the hint.
    pub window_type: Option<egui::X11WindowType>,
    /// A bar of our own instead of the OS's — see [`Chrome`]. `Some` makes
    /// the desktop window borderless and draws the same bar on both paths,
    /// so they cannot drift; `None` is every surface that was here before.
    pub chrome: Option<Chrome>,
    /// **How high this one sits among the app's own surfaces** — see
    /// [`SurfaceTier`]. Distinct from `on_top`, which is a yes/no about
    /// floating at all: this is which of two floating surfaces wins, and
    /// `on_top: false` still beats any tier.
    pub tier: SurfaceTier,
    /// **Show the window on the pass it is created**, rather than on the
    /// next one.
    ///
    /// Every other surface here is born hidden and revealed a pass later,
    /// and the comment in `surface` says why: on Windows a window that
    /// exists before anything has been drawn into it shows one frame of
    /// undefined back buffer, white in practice, and eframe dodges exactly
    /// that for the main window (egui#3631, "to fix white flash on
    /// startup"). A menu that flashes white on the way up is worse than a
    /// menu one frame late, so late is the default and stays the default.
    ///
    /// **The exemption is a surface that has to be on screen before the UI
    /// thread stops.** A busy overlay put up in front of a call that blocks
    /// — a device scan, a project load — never gets its second pass: the
    /// block happens inside the same frame, the reveal is queued behind it,
    /// and the one thing that was supposed to explain the freeze is the one
    /// thing that never appeared. There a frame of undefined back buffer is
    /// not the failure; a frozen app with nothing on screen is. So the
    /// surface that has to beat a block says so here, and pays the flash.
    ///
    /// It costs the surface nothing else: `surface` treats the birth pass as
    /// the reveal, so the level and the focus are still asked for once the
    /// window is real, exactly as they are a pass later for everything else.
    pub born_visible: bool,
}

impl Default for SurfaceSpec<'_> {
    fn default() -> Self {
        Self {
            id: "",
            title: "Tangent",
            size: Vec2::ZERO,
            min_size: Vec2::ZERO,
            pos: None,
            decorated: false,
            resizable: false,
            takes_focus: true,
            modal: false,
            on_top: true,
            raise: false,
            order: egui::Order::Foreground,
            window_type: None,
            chrome: None,
            tier: SurfaceTier::Ordinary,
            born_visible: false,
        }
    }
}

/// What the host says happened to a surface after a frame.
///
/// `close` is deliberately one field fed by four sources — a title-bar close,
/// Escape, the body asking, and (inline) a click outside. Callers that treated
/// those separately were the reason Escape worked on one surface and not
/// another.
#[derive(Clone, Copy, Default, Debug, PartialEq)]
pub struct SurfaceReport {
    pub close: bool,
    /// `None` inline: there is no window, so there is no focus to lose. Callers
    /// must not read absence as "unfocused" — that is what would make a plugin
    /// menu close itself on the first frame.
    pub focused: Option<bool>,
    /// A pointer press landed outside this surface's rect this frame. Only ever
    /// true inline; on the desktop, a click elsewhere shows up as focus loss.
    pub pressed_outside: bool,
    /// Whether the pointer is currently over this surface's window. `Some`
    /// only on the desktop, where each viewport has its own pointer stream and
    /// a `PointerGone` marks the cursor leaving. `None` inline.
    ///
    /// This exists because focus is not a safe proxy for attention on Linux:
    /// under i3 a freshly created submenu window takes the focus and the menu
    /// window never gets it back, so "nothing of mine is focused" can be true
    /// while the user's pointer is sitting in the middle of the menu.
    pub pointer_over: Option<bool>,
    /// The theme key was pressed with this surface's bar up. Only a
    /// [`Chrome`] surface ever says so: it is the one whose input the main
    /// window cannot see, so somebody has to carry the key back.
    pub cycle_theme: bool,
    /// **The transport key, pressed in a window wearing the chrome bar** - a
    /// plugin's, or the quantize window. Space, through the one keymap. A
    /// non-modal window is one you are meant to leave open while you work,
    /// and on the desktop it is a real child window - so without this the
    /// main window is unfocused for as long as it is up and nothing can be
    /// played. See `ChromeGesture`, which `draw_chrome_bar` fills.
    pub transport: bool,
}

fn viewport_id(id: &str) -> egui::ViewportId {
    egui::ViewportId::from_hash_of(id)
}

/// Draw `add` into a surface, as an OS window or in the canvas.
///
/// **This is the only place in `tangent-ui` that decides between the two.** The
/// plugin path is not a fallback for calls that fail: in a plugin
/// `show_viewport_immediate` still RUNS, because the context is built with
/// `embed_viewports: true`, so it invokes the closure inline and opens a second
/// `CentralPanel` under the same id as the first — painting garbage over the
/// piano rather than erroring. A hard error would have been kinder. That is why
/// the branch sits ABOVE `viewport_ui` rather than inside it.
///
/// `add` receives the `Ui` and a `&mut bool` it sets to ask for the surface to
/// close (a Cancel button, a menu item chosen). Every other reason to close is
/// folded in here.
pub fn surface(
    ctx: &egui::Context,
    caps: Caps,
    spec: &SurfaceSpec<'_>,
    add: &mut dyn FnMut(&mut egui::Ui, &mut bool),
) -> SurfaceReport {
    let mut report = SurfaceReport::default();
    let mut body_wants_close = false;
    let chrome_id = egui::Id::new((spec.id, "chrome"));
    // **The tier, settled once for both paths.** Read before anything is
    // drawn, because the answer decides the level this surface is BORN at as
    // well as the one it is told to hold — and because the claim has to be
    // stamped whether this surface ends up a window or a layer, or a plugin
    // editor would leave a stale claim holding every desktop dialog down.
    let pass = ctx.cumulative_pass_nr();
    let me = egui::Id::new(spec.id);
    let standing = standing_claim(ctx, pass);
    let by_others = standing.filter(|(_, who)| *who != me).map(|(t, _)| t);
    // **Two surfaces on one rung is the race this exists to end.** The ladder
    // orders DIFFERENT rungs; equal ones leave both floating and hand the
    // answer back to whoever reveals last. The app is built so it cannot
    // happen — one export window, one dialog slot — and this is where that is
    // checked rather than assumed, because the failure it would cause is
    // invisible on a screenshot and only shows up as a window that is
    // sometimes in front.
    debug_assert!(
        spec.tier == SurfaceTier::Ordinary || by_others != Some(spec.tier),
        "two surfaces at {:?} at once: {:?} and {me:?} ({})",
        spec.tier,
        standing.map(|(_, who)| who),
        spec.id
    );
    let want_level = level_for(spec.on_top, spec.tier, by_others);
    // Claimed only while the claim is worth anything, and only by whoever is
    // highest. A surface that has stood down for the OS's file panel
    // (`on_top: false`) is not above anything, and holding the rest of the
    // app at `Normal` on its behalf would cost them their float for no one's
    // benefit. The slot keeps the maximum: a lower rung must not overwrite a
    // higher one that is still up, or a modal dialog would lose its standing
    // to the render window it is asking in front of.
    if spec.tier > SurfaceTier::Ordinary
        && want_level == egui::WindowLevel::AlwaysOnTop
        && standing.is_none_or(|(t, who)| who == me || spec.tier > t)
    {
        ctx.memory_mut(|m| m.data.insert_temp(tier_claim_id(), (pass, spec.tier, me)));
    }
    // **`size` is the BODY's size.** Every caller designed its panel to the
    // points it asked for; a bar of ours on top must be added to the
    // window, not taken out of the panel — taken out, every plugin window
    // came up a bar short, clipped on the desktop and scrolling inline.
    let outer = |s: Vec2| match spec.chrome {
        Some(_) => s + Vec2::new(0.0, CHROME_BAR_H),
        None => s,
    };

    if caps.child_windows {
        // **Born hidden, revealed on the second pass.**
        //
        // A window is created VISIBLE unless told otherwise, and on Windows a
        // window that exists before anything has been drawn into it shows one
        // frame of undefined back buffer — white, in practice. eframe's own
        // main window dodges exactly this by starting hidden and revealing
        // itself after the first frame (egui#3631, and the comment there says
        // "to fix white flash on startup"); a CHILD viewport gets no such
        // treatment, and this app's menu, submenu panel and dialogs are all
        // child viewports that appear and disappear while somebody is using
        // it.
        //
        // A surface that was drawn on the PREVIOUS pass already has a window,
        // so it stays visible; one that was not is being created right now.
        // Consecutive calls differ by exactly one pass — not drawing a surface
        // is what destroys it — so the gap is a reliable "this is new".
        let life = egui::Id::new(("surface-life", spec.id));
        // `(the pass it was born on, the last pass it was drawn on)`. Drawn on
        // the previous pass means the window is still there; a gap means this
        // one is brand new, because not drawing a surface is what destroys it.
        let alive = ctx
            .memory(|m| m.data.get_temp::<(u64, u64)>(life))
            .filter(|(_, last)| pass.saturating_sub(*last) <= 1);
        let born = alive.map_or(pass, |(b, _)| b);
        ctx.memory_mut(|m| m.data.insert_temp(life, (born, pass)));
        let existing = alive.is_some();
        // Whether it is on screen yet, and whether this is the pass that puts
        // it there — see [`birth`]. Either way the reveal happens exactly
        // once, and it is what the level and the focus below hang off.
        let (shown, revealing) = birth(spec.born_visible, existing, pass == born + 1);
        if !existing {
            // The reveal is a second pass, and a menu sitting still would not
            // otherwise ask for one.
            ctx.request_repaint();
        }
        let mut builder = egui::ViewportBuilder::default()
            .with_title(spec.title)
            // A bar of ours under the OS's would be two title bars.
            .with_decorations(spec.decorated && spec.chrome.is_none())
            .with_resizable(spec.resizable)
            .with_window_level(want_level)
            .with_active(spec.takes_focus)
            .with_visible(shown)
            .with_inner_size(outer(spec.size))
            .with_min_inner_size(outer(spec.min_size));
        if !spec.resizable {
            builder = builder.with_max_inner_size(outer(spec.size));
        }
        if let Some(p) = spec.pos {
            builder = builder.with_position(p);
        }
        if let Some(t) = spec.window_type {
            builder = builder.with_window_type(t);
        }

        ctx.show_viewport_immediate(viewport_id(spec.id), builder, |vp, _class| {
            viewport_ui(vp, |ui| {
                let (close_req, esc, focused, pointer_over) = ui.input(|i| {
                    (
                        i.viewport().close_requested(),
                        i.key_pressed(egui::Key::Escape),
                        i.viewport().focused,
                        i.pointer.has_pointer(),
                    )
                });
                report.focused = focused;
                report.pointer_over = Some(pointer_over);
                report.close |= close_req || esc;
                // **The LEVEL is asked for on the reveal, and it is not a focus
                // matter.**
                //
                // `with_window_level` is a creation-time attribute exactly as
                // `with_active` is, and these surfaces are born INVISIBLE — so
                // the level set in the builder is applied to a window the
                // manager has not mapped yet, and on X11 it does not survive
                // being shown. It has to be asked for again once the window is
                // real, the same way focus is.
                //
                // This used to be inside the focus branch below, which meant a
                // surface that deliberately does not take focus never asked at
                // all. The notice was exactly that: `takes_focus: false` so a
                // message arriving mid-edit cannot move the caret, `on_top:
                // true` and no way to say so. It landed in the NORMAL layer —
                // under the Setup window it was reporting on, and under the
                // desktop's own panels. The owner saw a device-change message
                // appear below the panel he had just changed the device in.
                //
                // **And asked for again whenever the answer CHANGES**, which
                // is what a tier needs: a surface that was floating and has
                // been put down under a [`SurfaceTier::Uncovered`] claim must
                // be told so on a frame that is neither its reveal nor a
                // raise. Remembering the last level sent keeps that to one
                // command per transition rather than one every frame.
                let level_id = egui::Id::new((spec.id, "surface-level"));
                let sent = ui
                    .ctx()
                    .memory(|m| m.data.get_temp::<egui::WindowLevel>(level_id));
                if revealing || spec.raise || sent != Some(want_level) {
                    ui.ctx()
                        .send_viewport_cmd(egui::ViewportCommand::WindowLevel(want_level));
                    ui.ctx()
                        .memory_mut(|m| m.data.insert_temp(level_id, want_level));
                }
                // **Focus, separately, and only for surfaces that want it.**
                // `with_active` is creation-time and an invisible window is not
                // a window anybody can focus, so without this a dialog could
                // come up unable to hear Escape — which is the only way some of
                // them close. The submenu passes `takes_focus: false` because
                // stealing it from the menu is what closes the menu.
                if (revealing && spec.takes_focus) || spec.raise {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                let Some(c) = spec.chrome else {
                    add(ui, &mut body_wants_close);
                    return;
                };
                let frame = ui.max_rect();
                let l = chrome_layout(frame, spec.resizable);
                let g = draw_chrome_bar(ui, chrome_id, spec.title, &l, c);
                report.close |= g.close;
                report.cycle_theme |= g.cycle_theme;
                report.transport |= g.transport;
                if g.grab {
                    // **The OS moves the window, from the press.** A
                    // borderless window has no bar its manager knows about,
                    // so the bar we drew has to say "this is one" — the same
                    // `StartDrag` the borderless main window sends from its
                    // own press.
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
                let mut body = ui.new_child(egui::UiBuilder::new().max_rect(l.body));
                body.set_clip_rect(l.body.intersect(ui.clip_rect()));
                add(&mut body, &mut body_wants_close);
                let resized = draw_chrome_edge(ui, chrome_id, frame, &l, c);
                if resized != Vec2::ZERO {
                    // **Resized by asking for a size, not with `BeginResize`.**
                    // winit's macOS backend answers `drag_resize_window` with
                    // `NotSupported` (0.30, `window_delegate.rs`), so on the
                    // platform this ships to first a grip built on it would
                    // do nothing at all. A size request is honoured by every
                    // backend, and the grip is only ever asking for one.
                    let want = (frame.size() + resized).max(outer(spec.min_size));
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::InnerSize(want));
                }
            });
        });
        report.close |= body_wants_close;
        return report;
    }

    // ── In-canvas ──────────────────────────────────────────────────────────
    // An Area needs only a Context, so this works identically in a host-owned
    // window with no window manager anywhere in sight.
    let screen = ctx.content_rect();
    // What the bar and the grip did to this layer on earlier frames — only
    // a chrome surface has either, so only a chrome surface remembers.
    let state_id = egui::Id::new((spec.id, "inline-chrome"));
    let mut state = spec.chrome.map(|_| {
        ctx.memory(|m| m.data.get_temp::<InlineChromeState>(state_id))
            .filter(|s| pass.saturating_sub(s.last_pass) <= 1)
            .unwrap_or_default()
    });
    let asked = state.and_then(|s| s.size).unwrap_or(outer(spec.size));
    let least = outer(spec.min_size);
    let size = Vec2::new(
        asked
            .x
            .min(screen.width() * 0.94)
            .max(least.x.min(screen.width())),
        asked
            .y
            .min(screen.height() * 0.94)
            .max(least.y.min(screen.height())),
    );
    // Clamped rather than trusted: a right-click near the bottom of a short
    // editor would otherwise put the menu mostly off the canvas, and unlike
    // a window there is no desktop for it to hang over. A dragged bar is
    // held to the same rule, for the same reason.
    let clamp_in = |p: Pos2| {
        Pos2::new(
            p.x.clamp(screen.min.x, (screen.max.x - size.x).max(screen.min.x)),
            p.y.clamp(screen.min.y, (screen.max.y - size.y).max(screen.min.y)),
        )
    };
    let rect = match spec.pos {
        Some(p) => Rect::from_min_size(clamp_in(p), size),
        None => Rect::from_center_size(screen.center(), size),
    };
    let rect = match state {
        Some(s) => Rect::from_min_size(clamp_in(rect.min + s.moved), size),
        None => rect,
    };

    if spec.modal {
        // Dim the app behind it. Without this the piano behind a plugin dialog
        // looks live when it is not.
        //
        // **`Middle`, which is also where a stood-down render lands** — see
        // [`SurfaceTier::inline_order`] — and the render is drawn after the
        // dialog, so it comes out ABOVE the dim rather than under it. That is
        // deliberate: a render is not something the question is waiting on,
        // and dimming the one window that says the app is busy is the
        // invisible-progress bug over again. The question itself is above
        // both.
        egui::Area::new(egui::Id::new((spec.id, "scrim")))
            .order(egui::Order::Middle)
            .fixed_pos(screen.min)
            .show(ctx, |ui| {
                ui.painter()
                    .rect_filled(screen, 0.0, Color32::from_black_alpha(150));
                // Swallow clicks so the app underneath cannot be operated
                // through the scrim, which is what makes it genuinely modal.
                ui.allocate_rect(screen, egui::Sense::click_and_drag());
            });
    }

    egui::Area::new(egui::Id::new(spec.id))
        // The ladder, inline: a layer's height is its paint order, and
        // `Order` is absolute in a way a window level is not. See
        // [`SurfaceTier::inline_order`].
        .order(spec.tier.inline_order(spec.order, by_others))
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
            // **A layer eats what lands on it, modal or not.**
            //
            // On the desktop this is free: the surface is another OS window
            // and a press on it never reaches the main window at all. Inline
            // it has to be said, and saying it is not the same as being
            // modal — the export window dims nothing and blocks nothing, and
            // a press on its face still must not play the piano behind it.
            // The scrim used to be the only thing doing this, so a surface
            // that stopped being modal (correctly: it reports, it does not
            // ask) stopped swallowing too.
            //
            // An `Area` on its own does not do it. It registers one widget
            // over `state.size`, which is the size the CONTENT claimed last
            // pass — and a body that only paints (the export waveform is
            // exactly that) claims nothing, so the layer is a hole. This
            // allocation claims the whole surface, which both blocks and
            // gives the `Area` its true size.
            //
            // Allocated FIRST so it is the bottom of its own layer: egui
            // reads a big click-and-drag rect with smaller widgets over it
            // as a background, and hands presses to the widgets. The body's
            // own buttons are registered after this and keep working.
            //
            // It stops everything that goes through egui's hit test, which
            // is every widget in the app and the main background's own
            // `Response`. It does NOT stop the handlers that read
            // `i.pointer` straight off the input — nothing in a layer can,
            // and the scrim never did either. Those ask `inline_rect` where
            // a surface is; see it, and `dialogs::pointer_over_dialog`.
            ui.allocate_rect(rect, egui::Sense::click_and_drag());
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    report.close = true;
                }
                // The body, into whatever rect is its own. SCROLL rather than
                // clip when it does not fit.
                //
                // Every body here paints absolutely into `max_rect`, so handing
                // it a rect smaller than it asked for does not make it smaller
                // — it makes it CUT OFF, with the rows past the bottom edge
                // drawn and unreachable. The menu is about 460 points tall and
                // a plugin editor is often shorter than that, so the last third
                // of it, Reset and About included, simply could not be clicked.
                //
                // On the desktop, and inline whenever the surface fits, this
                // branch is not taken and not one pixel moves.
                let mut body = |ui: &mut egui::Ui| {
                    let room = ui.max_rect();
                    if spec.size.y > room.height() + 0.5 || spec.size.x > room.width() + 0.5 {
                        egui::ScrollArea::both()
                            .auto_shrink([false; 2])
                            .show(ui, |ui| {
                                let natural = Rect::from_min_size(ui.max_rect().min, spec.size);
                                let mut inner =
                                    ui.new_child(egui::UiBuilder::new().max_rect(natural));
                                add(&mut inner, &mut body_wants_close);
                                // Claim the full height so the scrollbar knows
                                // how far there is to go.
                                ui.allocate_rect(natural, egui::Sense::hover());
                            });
                    } else {
                        add(ui, &mut body_wants_close);
                    }
                };
                let Some(c) = spec.chrome else {
                    // Handed exactly its rect, as it always was.
                    body(ui);
                    return;
                };
                // The same bar the desktop window wears, on the same layout;
                // here a move is the layer's to make and a resize is a
                // remembered size, because there is no window manager to ask.
                let l = chrome_layout(rect, spec.resizable);
                let g = draw_chrome_bar(ui, chrome_id, spec.title, &l, c);
                report.close |= g.close;
                report.cycle_theme |= g.cycle_theme;
                report.transport |= g.transport;
                if let Some(s) = state.as_mut() {
                    s.moved += g.moved;
                }
                let mut inner = ui.new_child(egui::UiBuilder::new().max_rect(l.body));
                inner.set_clip_rect(l.body);
                body(&mut inner);
                let resized = draw_chrome_edge(ui, chrome_id, rect, &l, c);
                if let Some(s) = state.as_mut() {
                    if resized != Vec2::ZERO {
                        s.size = Some((size + resized).max(least));
                    }
                }
            });
        });
    if let Some(mut s) = state {
        s.last_pass = pass;
        ctx.memory_mut(|m| m.data.insert_temp(state_id, s));
    }
    // Every inline surface, chrome or not: the app's raw-pointer handlers
    // ask `inline_rect` where a dialog sits, and the answer has to be there
    // on the pass after it was drawn.
    ctx.memory_mut(|m| m.data.insert_temp(inline_rect_id(spec.id), (pass, rect)));

    report.pressed_outside = ctx.input(|i| i.pointer.any_pressed())
        && ctx.pointer_interact_pos().is_none_or(|p| !rect.contains(p));
    report.close |= body_wants_close;
    report
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole point of `Frame::NONE`: the bridge must hand over the entire
    /// viewport with no margin and no inset. A default `CentralPanel` adds
    /// both, which would shift the piano and silently break the fixed-size
    /// geometry the app depends on. Cheaper and more reliable than eyeballing
    /// a screenshot, and it cannot rot.
    #[test]
    fn the_bridge_gives_away_the_whole_viewport() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, egui::vec2(1300.0, 200.0));
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };

        let mut got = None;
        let _ = ctx.run(input, |ctx| {
            viewport_ui(ctx, |ui| got = Some(ui.max_rect()));
        });

        let got = got.expect("the bridge never called its closure");
        assert!(
            (got.min.x - screen.min.x).abs() < 0.5 && (got.min.y - screen.min.y).abs() < 0.5,
            "bridge inset the origin: {got:?} vs {screen:?}"
        );
        assert!(
            (got.width() - screen.width()).abs() < 0.5
                && (got.height() - screen.height()).abs() < 0.5,
            "bridge shrank the drawing area: {got:?} vs {screen:?}"
        );
    }

    /// Returns (what the body was given, what is actually visible, report).
    fn run_inline(screen: Rect, spec: &SurfaceSpec<'_>) -> (Rect, Rect, SurfaceReport) {
        let ctx = egui::Context::default();
        let input = egui::RawInput {
            screen_rect: Some(screen),
            ..Default::default()
        };
        let mut got = Rect::NOTHING;
        let mut visible = Rect::NOTHING;
        let mut report = SurfaceReport::default();
        let _ = ctx.run(input, |ctx| {
            report = surface(ctx, Caps::PLUGIN, spec, &mut |ui, _close| {
                got = ui.max_rect();
                visible = ui.clip_rect();
            });
        });
        (got, visible, report)
    }

    /// The inline surface must land inside the canvas whatever position it is
    /// asked for. A menu opened by a right-click near the bottom edge of a
    /// short plugin editor is the real case: on the desktop it may hang over
    /// the rest of the screen, inline there is nothing to hang over, and an
    /// unclamped menu is one whose lower rows cannot be clicked.
    #[test]
    fn an_inline_surface_stays_inside_the_canvas() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        for pos in [
            Pos2::new(590.0, 195.0),
            Pos2::new(-40.0, -40.0),
            Pos2::new(300.0, 100.0),
        ] {
            let spec = SurfaceSpec {
                id: "test-clamp",
                size: Vec2::new(200.0, 120.0),
                min_size: Vec2::new(120.0, 60.0),
                pos: Some(pos),
                ..Default::default()
            };
            let (_, visible, _) = run_inline(screen, &spec);
            assert!(
                screen.expand(1.0).contains_rect(visible),
                "asked for {pos:?}, visible area {visible:?} is not inside {screen:?}"
            );
        }
    }

    /// A surface bigger than the editor SCROLLS. It must not be shrunk, and it
    /// must not be clipped.
    ///
    /// This test used to assert the opposite — that the body is handed a rect
    /// that fits — and that assertion was the bug. Every body here paints
    /// absolutely into `max_rect`, so a smaller rect does not make a smaller
    /// menu; it makes one whose bottom rows are drawn past the edge and cannot
    /// be clicked. The menu is ~460pt tall and a plugin editor is often less,
    /// so Reset, About and Theory were all unreachable in a plugin.
    #[test]
    fn an_oversized_inline_surface_scrolls_rather_than_clipping() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        let spec = SurfaceSpec {
            id: "test-big",
            size: Vec2::new(900.0, 460.0),
            min_size: Vec2::new(200.0, 100.0),
            ..Default::default()
        };
        let (got, visible, _) = run_inline(screen, &spec);

        // The body gets what it asked for, so nothing it draws falls off.
        assert!(
            got.width() >= spec.size.x - 0.5 && got.height() >= spec.size.y - 0.5,
            "the body was given {got:?}, less than the {:?} it needs",
            spec.size
        );
        // What is on screen still fits on screen.
        assert!(
            screen.expand(1.0).contains_rect(visible),
            "the visible area {visible:?} escapes {screen:?}"
        );
    }

    /// ...and a surface that DOES fit is handed exactly its rect, with no
    /// scroll area in the way. This is every dialog on the desktop and most of
    /// them in a plugin, so it is the path that must not change.
    #[test]
    fn a_surface_that_fits_is_not_wrapped_in_anything() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 400.0));
        let spec = SurfaceSpec {
            id: "test-fits",
            size: Vec2::new(300.0, 200.0),
            min_size: Vec2::new(100.0, 100.0),
            ..Default::default()
        };
        let (got, _, _) = run_inline(screen, &spec);
        assert!(
            (got.width() - 300.0).abs() < 0.5 && (got.height() - 200.0).abs() < 0.5,
            "a surface that fits was resized to {got:?}"
        );
    }

    /// An oversized inline surface must actually SCROLL when the wheel turns.
    ///
    /// "It has a ScrollArea in it" is not the same as "it scrolls": the area
    /// only scrolls if it knows its content is bigger than its viewport, and
    /// the content size here comes from an explicitly allocated rect rather
    /// than from laying widgets out.
    #[test]
    fn an_oversized_inline_surface_really_scrolls() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        let spec = SurfaceSpec {
            id: "test-scroll",
            size: Vec2::new(400.0, 900.0),
            min_size: Vec2::new(400.0, 900.0),
            pos: Some(Pos2::ZERO),
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut body_top = 0.0_f32;
        let mut run = |events: Vec<egui::Event>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    surface(ctx, Caps::PLUGIN, &spec, &mut |ui, _| {
                        body_top = ui.max_rect().min.y;
                    });
                },
            );
            body_top
        };

        run(vec![]);
        let before = run(vec![egui::Event::PointerMoved(Pos2::new(200.0, 100.0))]);
        // Three notches down, over the middle of the surface.
        let after = run(vec![
            egui::Event::PointerMoved(Pos2::new(200.0, 100.0)),
            egui::Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: Vec2::new(0.0, -240.0),
                modifiers: egui::Modifiers::NONE,
            },
        ]);
        run(vec![]);
        let settled = run(vec![]);

        assert!(
            settled < before - 50.0,
            "the wheel moved the content from {before} to {settled} \
             (immediately after the event: {after}) - it is not scrolling"
        );
    }

    /// Inline has no window, so it has no focus. Reporting `Some(false)` would
    /// be read by the menu as "the user clicked away" on the very first frame,
    /// and the menu would shut before it could be used.
    #[test]
    fn inline_reports_no_focus_rather_than_unfocused() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        let spec = SurfaceSpec {
            id: "test-focus",
            size: Vec2::new(100.0, 100.0),
            ..Default::default()
        };
        let (_, _, report) = run_inline(screen, &spec);
        assert_eq!(report.focused, None, "inline invented a focus state");
        assert!(!report.close, "an untouched surface asked to close");
    }

    /// The body's own close request has to survive the trip back, on both
    /// paths. It is how Cancel works.
    #[test]
    fn the_body_can_ask_to_close() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(600.0, 200.0));
        let spec = SurfaceSpec {
            id: "test-close",
            size: Vec2::new(100.0, 100.0),
            ..Default::default()
        };
        let mut report = SurfaceReport::default();
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                report = surface(ctx, Caps::PLUGIN, &spec, &mut |_ui, close| *close = true);
            },
        );
        assert!(report.close, "the body asked to close and was ignored");
    }


    // ── how high a surface sits ─────────────────────────────────────────────

    /// **The ladder's whole rule, as a table.**
    ///
    /// A surface floats unless some OTHER surface is standing on a STRICTLY
    /// higher rung. Three ways to get it wrong, and all three have been in
    /// this file: leave a sibling floating over the render (the notice
    /// landing on the export waveform), put the watched surface down under
    /// its own claim, and — the one that made this a ladder rather than a
    /// flag — put a QUESTION down under the render, which is an invisible
    /// question with the app frozen behind it.
    ///
    /// A unit test on a level is NOT a proof that the windows stack that way
    /// on a screen — the window manager has the last word and no headless
    /// test can ask it. What it does prove is that the right level is asked
    /// for in every combination, which is the half of this that lives in
    /// this crate. The other half is the by-hand list on
    /// `dialogs::show_export_progress`.
    #[test]
    fn the_ladder_puts_down_only_what_is_outranked() {
        use SurfaceTier::{Asking, Ordinary, Uncovered};
        use egui::WindowLevel::{AlwaysOnTop, Normal};
        // The declaration order IS the ladder — `level_for` compares the
        // variants directly — so a variant inserted in the wrong place would
        // silently reorder the whole app.
        assert!(Ordinary < Uncovered && Uncovered < Asking, "the ladder is out of order");
        // Nobody standing anywhere: every floating surface floats, exactly as
        // before there was a tier at all.
        for tier in [Ordinary, Uncovered, Asking] {
            assert_eq!(level_for(true, tier, None), AlwaysOnTop, "{tier:?}");
        }
        // An announcement goes down under a render...
        assert_eq!(level_for(true, Ordinary, Some(Uncovered)), Normal);
        // ...and under a question.
        assert_eq!(level_for(true, Ordinary, Some(Asking)), Normal);
        // **The render goes down under a question.** This is the fix: the
        // first draft demoted everything but the render, and a modal Setup
        // opened mid-export was pinned underneath it with no way up while
        // the app dropped every event waiting on it.
        assert_eq!(
            level_for(true, Uncovered, Some(Asking)),
            Normal,
            "a question was left underneath the render"
        );
        // A question is never put down by anything. There is no rung above
        // it and there must not be.
        assert_eq!(level_for(true, Asking, Some(Uncovered)), AlwaysOnTop);
        assert_eq!(level_for(true, Asking, Some(Ordinary)), AlwaysOnTop);
        // Nothing is put down by its own rung: two on one rung is a state
        // the app does not reach (see the `debug_assert` in `surface`), and
        // if it ever did, demoting both would be the worst possible answer.
        for tier in [Ordinary, Uncovered, Asking] {
            assert_eq!(level_for(true, tier, Some(tier)), AlwaysOnTop, "{tier:?}");
        }
        // A render still outranks the announcements while it is up.
        assert_eq!(level_for(true, Uncovered, Some(Ordinary)), AlwaysOnTop);
        // `on_top: false` beats every rung. It is the one moment a surface
        // must NOT float — the OS's own file panel is up, parented to the
        // main window, and nothing else may get in front of it.
        for tier in [Ordinary, Uncovered, Asking] {
            for other in [None, Some(Ordinary), Some(Uncovered), Some(Asking)] {
                assert_eq!(
                    level_for(false, tier, other),
                    Normal,
                    "{tier:?} floated over a native panel (others: {other:?})"
                );
            }
        }
        // **Inline the ladder is a paint order, and only the render moves.**
        // A question stays in the band its own `ComboBox` lists and its own
        // hover text are drawn in — promote it and its dropdowns open behind
        // it. See `SurfaceTier::inline_order`.
        let fg = egui::Order::Foreground;
        assert_eq!(Asking.inline_order(fg, None), fg, "a question left its own band");
        assert_eq!(Asking.inline_order(fg, Some(Uncovered)), fg);
        assert_eq!(Ordinary.inline_order(fg, None), fg);
        assert_eq!(Ordinary.inline_order(fg, Some(Asking)), fg);
        // The render goes one band up, over every announcement...
        assert_eq!(Uncovered.inline_order(fg, None), egui::Order::Tooltip);
        assert_eq!(Uncovered.inline_order(fg, Some(Ordinary)), egui::Order::Tooltip);
        // ...and one band DOWN when a question is up, which is how a
        // question drawn earlier in the frame still ends up in front of it.
        assert_eq!(
            Uncovered.inline_order(fg, Some(Asking)),
            egui::Order::Middle,
            "the render stayed over a question inline"
        );
        assert!(
            egui::Order::Middle < fg && fg < egui::Order::Tooltip,
            "the inline ladder does not actually stack"
        );
        // Every band it uses can still be clicked in — a question drawn in a
        // band that eats its own presses is the frozen app all over again.
        for order in [egui::Order::Middle, fg, egui::Order::Tooltip] {
            assert!(order.allow_interaction(), "{order:?} cannot be interacted with");
        }
        // An ordinary surface keeps whatever order it asked for — the
        // submenu sits above the menu by asking, and the ladder must not
        // flatten that.
        assert_eq!(
            Ordinary.inline_order(egui::Order::Middle, None),
            egui::Order::Middle
        );
    }

    /// **Nothing this app can open may paint above a cover.**
    ///
    /// A cover — the busy overlay, the launch splash — is the last thing on
    /// screen before the UI thread stops answering, so it has to be over
    /// every surface that can be up while the app works: the export dialog
    /// and every other question, a plugin's face, the pedal window, the menu
    /// and its submenu, and the export render window that stands for the
    /// whole of a bounce. That was stated at two call sites in `desktop.rs`
    /// as `Order::Foreground` and was false in both directions — under the
    /// render window outright, over a dialog only by egui's hash-ordered
    /// sweep of layers that are not `Area`s. It is
    /// [`crate::busy::COVER_ORDER`] now, and this is the guard that ties the
    /// one to the other.
    ///
    /// **Read off `inline_order` rather than off a list**, because a list is
    /// the thing that drifts: a fourth tier added above `Uncovered`, or
    /// `Uncovered` moved up a band, fails here on the next `cargo test`
    /// instead of on somebody's screen during an export. `asked` is swept
    /// over every band a caller can pass in, since `Ordinary` and `Asking`
    /// hand it straight back.
    ///
    /// Revert-check: set `busy::COVER_ORDER` back to `egui::Order::Foreground`
    /// and the `Uncovered` row goes red here, and
    /// `the_busy_cover_paints_over_the_export_render_window` goes red in
    /// `dialogs.rs`.
    #[test]
    fn a_cover_is_painted_above_every_surface_this_app_can_open() {
        use SurfaceTier::{Asking, Ordinary, Uncovered};
        let cover = crate::busy::COVER_ORDER;
        for tier in [Ordinary, Uncovered, Asking] {
            for asked in [
                egui::Order::Background,
                egui::Order::Middle,
                egui::Order::Foreground,
                egui::Order::Tooltip,
            ] {
                for other in [None, Some(Ordinary), Some(Uncovered), Some(Asking)] {
                    let band = tier.inline_order(asked, other);
                    assert!(
                        band < cover,
                        "{tier:?} asking for {asked:?} (others: {other:?}) paints \
                         in {band:?}, which is not under a cover in {cover:?}"
                    );
                }
            }
        }
        // The two bands nothing in this app declares but everything gets for
        // free: `notify`'s column draws in `Foreground`, and egui puts its own
        // hover text in `Tooltip` whether we ask or not.
        assert!(egui::Order::Foreground < cover, "the notice column is over a cover");
        assert!(egui::Order::Tooltip < cover, "a tooltip is over a cover");
    }

    /// Run one `surface` on the desktop path and report the window levels it
    /// asked the host for on that frame.
    ///
    /// `Caps::DESKTOP` takes the window path; with no viewport renderer
    /// attached egui runs the body inline and the commands land on the
    /// root's output, which is exactly the list eframe would hand to winit.
    fn levels_asked_for(ctx: &egui::Context, screen: Rect, spec: &SurfaceSpec<'_>) -> Vec<egui::WindowLevel> {
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                surface(ctx, Caps::DESKTOP, spec, &mut |_ui, _close| {});
            },
        );
        out.viewport_output
            .get(&egui::ViewportId::ROOT)
            .map(|vp| {
                vp.commands
                    .iter()
                    .filter_map(|c| match c {
                        egui::ViewportCommand::WindowLevel(l) => Some(*l),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    /// **The claim reaches the sibling, and it lapses.**
    ///
    /// The end-to-end half: run `surface` the way the app does and read the
    /// level it actually asks the host for. The order matters — the app
    /// draws its dialogs and its notice BEFORE the export window, so a
    /// sibling reads a claim written on the previous pass, which is why
    /// `standing_claim` accepts one that is a pass old.
    ///
    /// It has to lapse too, or an export that finished would hold every
    /// dialog in the app at the ordinary level for the rest of the session.
    #[test]
    fn a_no_cover_claim_reaches_the_next_frames_siblings_and_then_lapses() {
        use egui::WindowLevel::{AlwaysOnTop, Normal};
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let top = SurfaceSpec {
            id: "test-uncovered",
            size: Vec2::new(600.0, 400.0),
            tier: SurfaceTier::Uncovered,
            ..Default::default()
        };
        let sibling = SurfaceSpec {
            id: "test-sibling",
            size: Vec2::new(300.0, 200.0),
            ..Default::default()
        };
        let run = |spec: &SurfaceSpec<'_>| levels_asked_for(&ctx, screen, spec);

        // The claimant asks to float, and stamps the claim.
        assert_eq!(run(&top), vec![AlwaysOnTop], "the claimant did not float");
        // The very next frame, a sibling that asked to float is put down.
        // This is the notice that used to land on the waveform.
        assert_eq!(
            run(&sibling),
            vec![Normal],
            "a sibling still floated over a surface nothing may cover"
        );
        // And with the claimant gone a frame, it comes back up on its own.
        assert_eq!(
            run(&sibling),
            vec![AlwaysOnTop],
            "the claim outlived the surface that made it"
        );
    }

    /// The level a surface last asked the host to put it at.
    ///
    /// `surface` records every level it sends so it can send one only on a
    /// change; reading that back is how a test asks a PARTICULAR surface
    /// what it wanted, when every viewport command in a headless run lands
    /// on the one root output with no name on it.
    fn level_held(ctx: &egui::Context, id: &str) -> Option<egui::WindowLevel> {
        ctx.memory(|m| {
            m.data
                .get_temp::<egui::WindowLevel>(egui::Id::new((id, "surface-level")))
        })
    }

    /// **A question opened mid-render is reachable and on top.**
    ///
    /// The blocker this ladder exists for, run end to end in the order the
    /// app draws things: the dialogs first, the notice next, the export
    /// window last. Setup opens while the render is running and must come
    /// out in FRONT of it — the app drops every main-window event while a
    /// modal dialog is open, so a Setup window pinned under the waveform is
    /// an app frozen with nothing on screen that can answer it.
    ///
    /// And it must keep [`SurfaceSpec::raise`], which is the app's answer to
    /// a press on the main window while a question is waiting. A demoted
    /// surface would still ask for `Focus`, but it would be asking for it at
    /// the ordinary level, under every floating sibling — the raise would
    /// look like it had failed.
    #[test]
    fn a_question_opened_mid_render_comes_out_in_front_of_it() {
        use egui::WindowLevel::{AlwaysOnTop, Normal};
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let render = SurfaceSpec {
            id: "test-render",
            size: Vec2::new(600.0, 400.0),
            tier: SurfaceTier::Uncovered,
            ..Default::default()
        };
        let notice = SurfaceSpec {
            id: "test-notice",
            size: Vec2::new(300.0, 200.0),
            takes_focus: false,
            ..Default::default()
        };
        let setup = SurfaceSpec {
            id: "test-setup",
            size: Vec2::new(500.0, 400.0),
            tier: SurfaceTier::Asking,
            ..Default::default()
        };
        // One frame of the app: the question, then the announcement, then
        // the render — the order `App::update` calls them in, which is the
        // order that makes a claim one pass old the only one a sibling can
        // see.
        let frame = |dialog: Option<&SurfaceSpec<'_>>, msg: bool| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    if let Some(d) = dialog {
                        surface(ctx, Caps::DESKTOP, d, &mut |_ui, _close| {});
                    }
                    if msg {
                        surface(ctx, Caps::DESKTOP, &notice, &mut |_ui, _close| {});
                    }
                    surface(ctx, Caps::DESKTOP, &render, &mut |_ui, _close| {});
                },
            )
        };
        // The render alone: it floats, and it stamps the claim.
        frame(None, false);
        assert_eq!(level_held(&ctx, "test-render"), Some(AlwaysOnTop));
        // A message arrives. It is an announcement, so it goes down and the
        // render stays up. This is the rule working as designed — and the
        // notice is not destroyed by losing, it is merely underneath.
        frame(None, true);
        assert_eq!(
            level_held(&ctx, "test-notice"),
            Some(Normal),
            "an announcement was left floating over the render"
        );
        assert_eq!(level_held(&ctx, "test-render"), Some(AlwaysOnTop));
        // Setup opens. On the pass it is born it reads a claim written by
        // the render, and the ladder — not the reveal order — says it wins.
        // Held for several passes, because a rule that only holds on the
        // frame something opened is the race this replaced.
        for pass in 0..4 {
            frame(Some(&setup), true);
            assert_eq!(
                level_held(&ctx, "test-setup"),
                Some(AlwaysOnTop),
                "pass {pass}: the question did not float"
            );
            assert_eq!(
                level_held(&ctx, "test-render"),
                Some(Normal),
                "pass {pass}: the render stayed on top of a question"
            );
            assert_eq!(level_held(&ctx, "test-notice"), Some(Normal), "pass {pass}");
        }
        // A press on the main window asks the question to come forward, and
        // it is still asking at the FLOATING level — not demoted, which is
        // what would make the raise look like it had done nothing.
        let raised = SurfaceSpec { raise: true, ..setup };
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                surface(ctx, Caps::DESKTOP, &raised, &mut |_ui, _close| {});
                surface(ctx, Caps::DESKTOP, &render, &mut |_ui, _close| {});
            },
        );
        let cmds = &out
            .viewport_output
            .get(&egui::ViewportId::ROOT)
            .expect("no root viewport")
            .commands;
        assert_eq!(
            level_held(&ctx, "test-setup"),
            Some(AlwaysOnTop),
            "a raised question asked for the ordinary level"
        );
        assert!(
            cmds.iter().any(|c| matches!(c, egui::ViewportCommand::Focus)),
            "a raised question never asked for the focus"
        );
        assert!(
            cmds.iter()
                .any(|c| matches!(c, egui::ViewportCommand::WindowLevel(AlwaysOnTop))),
            "a raised question never re-asserted its level"
        );
        // And when the question closes, the render comes back up by itself.
        //
        // **It takes three frames, and that is the documented cost.** The
        // question's claim stays good for one pass after it stops being
        // written (`standing_claim`); on the pass it finally goes stale the
        // announcement is drawn BEFORE the render, so for exactly one frame
        // it reads no claim at all and floats; the render restamps at the
        // end of that same frame and the announcement is back down on the
        // next. One frame at the 30 Hz this window repaints at, and the
        // alternative — every surface re-reading the claim after the frame
        // is drawn — is a second pass for every frame of every render.
        for _ in 0..3 {
            frame(None, true);
        }
        assert_eq!(
            level_held(&ctx, "test-render"),
            Some(AlwaysOnTop),
            "the render never recovered from the question"
        );
        assert_eq!(
            level_held(&ctx, "test-notice"),
            Some(Normal),
            "the announcement climbed back over the render"
        );
    }

    /// **The same answer in the canvas: a question is on top, the render is
    /// over the announcement, and nothing falls through.**
    ///
    /// The plugin path has no window levels at all — a surface is an `Area`
    /// and its height is its paint order — so the ladder has to be checked
    /// separately here. The order the app draws them in is the trap: the
    /// dialog goes first and the export window last, so "whoever is drawn
    /// last is on top" gets exactly the wrong answer, which is the bug this
    /// is pinning.
    #[test]
    fn the_ladder_stacks_the_same_way_in_the_canvas() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
        let render = SurfaceSpec {
            id: "inline-render",
            size: Vec2::new(600.0, 400.0),
            tier: SurfaceTier::Uncovered,
            ..Default::default()
        };
        let notice = SurfaceSpec {
            id: "inline-notice",
            size: Vec2::new(400.0, 300.0),
            takes_focus: false,
            ..Default::default()
        };
        let question = SurfaceSpec {
            id: "inline-question",
            size: Vec2::new(500.0, 350.0),
            tier: SurfaceTier::Asking,
            modal: true,
            ..Default::default()
        };
        // All three are centred, so the middle of the canvas is inside all
        // of them and `layer_id_at` there names whichever is on top.
        let ctx = egui::Context::default();
        let mut top = None;
        let frame = |ctx: &egui::Context, ask: bool, top: &mut Option<egui::LayerId>| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    // `App::update`'s order: the dialog, then the message,
                    // then the export window.
                    if ask {
                        surface(ctx, Caps::PLUGIN, &question, &mut |_ui, _c| {});
                    }
                    surface(ctx, Caps::PLUGIN, &notice, &mut |_ui, _c| {});
                    surface(ctx, Caps::PLUGIN, &render, &mut |_ui, _c| {});
                    *top = ctx.layer_id_at(screen.center());
                },
            );
        };
        // Two passes each way: the claim is read a pass after it is written.
        for _ in 0..2 {
            frame(&ctx, false, &mut top);
        }
        assert_eq!(
            top.map(|l| l.id),
            Some(egui::Id::new(render.id)),
            "an announcement covered the render in the canvas"
        );
        for _ in 0..3 {
            frame(&ctx, true, &mut top);
        }
        assert_eq!(
            top.map(|l| l.id),
            Some(egui::Id::new(question.id)),
            "the render stayed on top of a question in the canvas"
        );
        // And the render is back on top once the question goes — three
        // passes, the same settle the desktop path documents.
        for _ in 0..3 {
            frame(&ctx, false, &mut top);
        }
        assert_eq!(
            top.map(|l| l.id),
            Some(egui::Id::new(render.id)),
            "the render never came back after the question"
        );
    }

    /// **Two surfaces on one rung is caught, not tolerated.**
    ///
    /// The ladder orders DIFFERENT rungs. Two on the same one leave each
    /// other floating and hand the answer back to whoever reveals last,
    /// which is the race the whole thing replaced. The app is built so it
    /// cannot happen — one export window, one dialog slot — and `surface`
    /// says so out loud rather than degrading quietly into the old
    /// behaviour.
    ///
    /// **Debug builds only, and that is the honest shape of it.** The check it
    /// is asserting on is a `debug_assert!`, which is compiled OUT of a release
    /// build - so under `cargo test --release` this test does not panic, and a
    /// `#[should_panic]` that does not panic FAILS. It failed that way on macOS,
    /// Linux and Windows alike the first time anyone ran the suite in release;
    /// the gate never saw it because the gate runs debug. Ignoring it in release
    /// says "this cannot be checked here" instead of reporting a defect that is
    /// not there.
    #[test]
    #[should_panic(expected = "two surfaces at Uncovered at once")]
    #[cfg_attr(
        not(debug_assertions),
        ignore = "the check is a debug_assert!, compiled out of a release build"
    )]
    fn two_surfaces_on_one_rung_is_caught() {
        let ctx = egui::Context::default();
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(900.0, 700.0));
        let one = SurfaceSpec {
            id: "rung-one",
            size: Vec2::new(300.0, 200.0),
            tier: SurfaceTier::Uncovered,
            ..Default::default()
        };
        let two = SurfaceSpec {
            id: "rung-two",
            tier: SurfaceTier::Uncovered,
            ..one
        };
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen),
                ..Default::default()
            },
            |ctx| {
                surface(ctx, Caps::PLUGIN, &one, &mut |_ui, _c| {});
                surface(ctx, Caps::PLUGIN, &two, &mut |_ui, _c| {});
            },
        );
    }

    /// **A surface that has to beat a blocking call is born visible.**
    ///
    /// Everything else here is born hidden and revealed a pass later, to
    /// dodge the white flash of an undrawn back buffer. A busy overlay put
    /// up in front of a call that blocks the UI thread never GETS that
    /// second pass — the block happens inside the same frame — so it needs
    /// the window on screen from the first. See `SurfaceSpec::born_visible`.
    ///
    /// Born visible must still be a real reveal: the level and the focus are
    /// creation-time attributes applied to a window the manager has not
    /// mapped yet, and on X11 they do not survive being shown, so they have
    /// to be asked for again once the window is real. That is what
    /// `revealing` carries, and moving the reveal forward must not drop it.
    #[test]
    fn a_surface_that_cannot_wait_a_pass_is_born_visible_and_still_reveals() {
        // The default, unchanged: hidden on the pass it is created, revealed
        // on the next, and never again.
        assert_eq!(birth(false, false, false), (false, false), "born visible by default");
        assert_eq!(birth(false, true, true), (true, true), "the second pass is the reveal");
        assert_eq!(birth(false, true, false), (true, false), "revealed twice");
        // The escape: on screen from the first pass, and that pass IS the
        // reveal — the level and the focus asked for there rather than on a
        // pass that a blocking call means will never come.
        assert_eq!(birth(true, false, false), (true, true), "the overlay was born hidden");
        // ...and it is still only once. A surface that asked for the level
        // and the focus every frame would fight the user for the focus.
        assert_eq!(birth(true, true, false), (true, false), "revealed every frame");
        assert_eq!(birth(true, true, true), (true, false), "revealed twice");

        // End to end: the reveal work really is done on the first pass.
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1400.0, 900.0));
        let first_pass_commands = |spec: &SurfaceSpec<'_>| {
            let ctx = egui::Context::default();
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    surface(ctx, Caps::DESKTOP, spec, &mut |_ui, _close| {});
                },
            );
            let cmds = out
                .viewport_output
                .get(&egui::ViewportId::ROOT)
                .map(|vp| vp.commands.clone())
                .unwrap_or_default();
            (
                cmds.iter().any(|c| matches!(c, egui::ViewportCommand::Focus)),
                cmds.iter()
                    .any(|c| matches!(c, egui::ViewportCommand::WindowLevel(_))),
            )
        };
        let overlay = SurfaceSpec {
            id: "test-blocking",
            size: Vec2::new(300.0, 200.0),
            born_visible: true,
            ..Default::default()
        };
        let (focused, levelled) = first_pass_commands(&overlay);
        assert!(focused, "the overlay never asked for the focus");
        assert!(levelled, "the overlay never asked for its window level");
        // The ordinary surface does ask for its level on the first pass (it
        // has none recorded yet) but must NOT take the focus there — it is
        // still hidden, and an invisible window is not one anybody can focus.
        let ordinary = SurfaceSpec {
            id: "test-late",
            size: Vec2::new(300.0, 200.0),
            ..Default::default()
        };
        let (focused, _) = first_pass_commands(&ordinary);
        assert!(!focused, "a hidden window was told to take the focus");
    }

    /// **An inline surface eats the clicks that land on it, modal or not.**
    ///
    /// On the desktop a surface is another OS window and this is free. Inline
    /// it is a layer over the desk, and it was the modal scrim doing the
    /// swallowing — so the export window, which correctly stopped being
    /// modal (it reports, it does not ask), stopped swallowing with it and a
    /// press on the waveform reached the piano behind it.
    ///
    /// The proof is egui's own hit test: a widget under the surface must not
    /// be what a click at that point would hit. Which is also the limit of
    /// what any layer can do — the app's raw `i.pointer` handlers see
    /// through every layer there has ever been, the scrim included, and they
    /// ask `inline_rect` instead.
    #[test]
    fn an_inline_surface_swallows_the_presses_that_land_on_it() {
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let spec = SurfaceSpec {
            id: "test-swallow",
            size: Vec2::new(300.0, 200.0),
            // Not modal, which is the whole point: no scrim is drawn.
            modal: false,
            ..Default::default()
        };
        let ctx = egui::Context::default();
        let mut desk = None;
        let mut middle = None;
        let mut corner = None;
        // Two passes: egui hit-tests against the layers of the previous one,
        // so the answer that matters is the second.
        for _ in 0..2 {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    ..Default::default()
                },
                |ctx| {
                    // The desk: one big background widget under everything,
                    // the shape `App::handle_main_interaction` interacts
                    // with the main window as.
                    egui::CentralPanel::default().show(ctx, |ui| {
                        desk = Some(ui.layer_id());
                        ui.interact(
                            screen,
                            egui::Id::new("test-desk"),
                            egui::Sense::click_and_drag(),
                        );
                    });
                    surface(ctx, Caps::PLUGIN, &spec, &mut |_ui, _close| {});
                    middle = ctx.layer_id_at(screen.center());
                    corner = ctx.layer_id_at(screen.min + Vec2::splat(4.0));
                },
            );
        }
        let desk = desk.expect("the desk never drew");
        // Centred, so the middle of the canvas is the middle of the surface.
        let over = middle.expect("nothing at all was under the pointer");
        assert_ne!(
            over, desk,
            "a press in the middle of the surface reached the desk behind it"
        );
        assert_eq!(
            over.id,
            egui::Id::new(spec.id),
            "the surface is not the layer under its own middle"
        );
        // And the desk is still reachable where the surface is not, or this
        // would be a scrim by another name.
        assert_eq!(
            corner,
            Some(desk),
            "the surface swallowed a press nowhere near it"
        );
    }

    /// **A share of the screen, with a floor, a ceiling and a last clamp.**
    ///
    /// The general rule, tested where it lives rather than inside the one
    /// window that happens to use it first. All four parts have to bite:
    /// the share on an ordinary display, the floor on a small one, the
    /// ceiling on a big one, and the clamp on a display so small the floor
    /// itself does not fit — which is not a hypothetical, it is a plugin
    /// editor's canvas.
    #[test]
    fn a_window_takes_its_share_of_the_screen_between_a_floor_and_a_ceiling() {
        let share = Vec2::new(0.46, 0.40);
        let floor = Vec2::new(560.0, 300.0);
        let ceiling = Vec2::new(1200.0, 620.0);
        let at = |m: Vec2| share_of_screen(Some(m), share, floor, ceiling);
        // Nothing measured yet: the floor, because a guess about a screen
        // nobody has looked at is worse than the smallest designed size.
        assert_eq!(share_of_screen(None, share, floor, ceiling), floor);
        assert_eq!(share_of_screen(Some(Vec2::ZERO), share, floor, ceiling), floor);
        // The share itself, on a display where neither limit is near.
        let mid = at(Vec2::new(1920.0, 1080.0));
        assert!((mid.x - 1920.0 * 0.46).abs() < 0.5 && (mid.y - 1080.0 * 0.40).abs() < 0.5);
        // The floor, on a small display.
        assert_eq!(at(Vec2::new(900.0, 600.0)), floor, "the floor never engages");
        // The ceiling, on a big one.
        assert_eq!(at(Vec2::new(3008.0, 1692.0)), ceiling, "the ceiling never engages");
        // And never bigger than the screen, even when that means going under
        // the floor — the bug the 5.0 demo session found in the main window.
        let tiny = Vec2::new(400.0, 300.0);
        let got = at(tiny);
        assert!(got.x <= tiny.x && got.y <= tiny.y, "{got:?} escapes {tiny:?}");
        assert!(got.x < floor.x, "the clamp did not take it under the floor");
        // Monotone: a bigger screen never gets a smaller window.
        let mut last = Vec2::ZERO;
        for h in [600.0_f32, 800.0, 900.0, 1080.0, 1400.0, 1440.0, 1692.0, 2160.0] {
            let got = at(Vec2::new(h * 16.0 / 9.0, h));
            assert!(got.x >= last.x && got.y >= last.y, "{h} gave {got:?} after {last:?}");
            last = got;
        }
    }

    // ── the in-house chrome ─────────────────────────────────────────────────

    /// **One layout, every part.** The bar, the cross, the drag rect, the
    /// grip and the body all come from `chrome_layout`, so what is drawn is
    /// what is hit — and the cross and the drag rect never overlap, because
    /// a press on the cross that also started a drag would swallow the
    /// click.
    #[test]
    fn the_chrome_parts_come_from_one_layout() {
        let rect = Rect::from_min_size(Pos2::new(40.0, 30.0), Vec2::new(400.0, 300.0));
        let l = chrome_layout(rect, true);
        // The bar spans the top at its one height.
        assert!((l.bar.height() - CHROME_BAR_H).abs() < 0.5, "bar {:?}", l.bar);
        assert_eq!(l.bar.min, rect.min);
        assert!((l.bar.width() - rect.width()).abs() < 0.5);
        // The cross is a full-height square at the bar's right end.
        assert!((l.close.width() - l.close.height()).abs() < 0.5, "close {:?}", l.close);
        assert!((l.close.height() - l.bar.height()).abs() < 0.5);
        assert!((l.close.right() - rect.right()).abs() < 0.5);
        assert!(l.bar.contains_rect(l.close));
        // The drag rect is the rest of the bar, sharing nothing with the cross.
        assert!(l.bar.contains_rect(l.drag));
        assert!(
            l.drag.right() <= l.close.left() + 0.01,
            "the drag rect {:?} runs under the cross {:?}",
            l.drag,
            l.close
        );
        assert!(l.drag.contains_rect(l.title), "the title leaves the drag rect");
        // The grip sits in the bottom-right corner, over the body.
        let grip = l.grip.expect("a resizable window has a grip");
        assert!((grip.max - rect.max).length() < 0.5, "grip {grip:?}");
        assert!((grip.width() - CHROME_GRIP).abs() < 0.5);
        assert!(l.body.contains_rect(grip));
        // The body is everything under the bar.
        assert!((l.body.top() - l.bar.bottom()).abs() < 0.5);
        assert_eq!(l.body.max, rect.max);
        assert!((l.body.left() - rect.left()).abs() < 0.5);
        // Not resizable: no grip, same everything else.
        let fixed = chrome_layout(rect, false);
        assert!(fixed.grip.is_none());
        assert_eq!(fixed.bar, l.bar);
        assert_eq!(fixed.body, l.body);
        // Every part is inside the window.
        for r in [l.bar, l.title, l.close, l.drag, grip, l.body] {
            assert!(rect.expand(0.01).contains_rect(r), "{r:?} escapes {rect:?}");
        }
    }

    /// **The bar is readable on every theme, in both modes, or the build
    /// fails.** The sweep `face.rs` runs, on the chrome: the title ink
    /// measured on the bar it is written on, the edge on the body, and the
    /// bar told apart from the body it tops — on BOTH faces a dialog can be
    /// handed, the desk's strip and the band's ground.
    #[test]
    fn every_theme_dresses_the_chrome() {
        use crate::recorder_panel::contrast_ratio;
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
                    let c = Chrome::of(face);
                    let body = legible_surface(face);
                    let mut check = |what: &str, r: f32, floor: f32| {
                        if r < floor {
                            bad.push(format!("{} {mode} ({which}): {what} {r:.2}:1", theme.name));
                        }
                    };
                    check("title ink on the bar", contrast_ratio(c.ink, c.bar), 4.5);
                    check("the edge on the body", contrast_ratio(c.edge, body), 4.5);
                    check("the bar against the body", contrast_ratio(c.bar, body), 1.1);
                    swept += 1;
                }
            }
        }
        assert!(swept >= 19 * 2 * 2, "the sweep covered {swept} dressings");
        assert!(
            bad.is_empty(),
            "the chrome is unreadable:\n  {}",
            bad.join("\n  ")
        );
    }

    /// The bar's controls do what they show: the body sits under the bar,
    /// the theme key is carried back, a drag on the bar moves the window, a
    /// drag on the grip resizes it, and the cross closes it. Inline, which
    /// is the one path a test can move a window on; the desktop path shares
    /// every function but the last line of each gesture.
    #[test]
    fn the_chrome_moves_resizes_cycles_and_closes() {
        let ctx = egui::Context::default();
        crate::fonts::install(&ctx, crate::fonts::FontChoice::default(), None);
        crate::fonts::apply_text_styles(&ctx);
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(800.0, 600.0));
        let at = Pos2::new(100.0, 100.0);
        let size = Vec2::new(300.0, 200.0);
        let spec = SurfaceSpec {
            id: "test-chrome",
            title: "Tangent Comp",
            size,
            min_size: Vec2::new(200.0, 100.0),
            pos: Some(at),
            resizable: true,
            chrome: Some(Chrome::of(Color32::from_rgb(0xE3, 0xD9, 0xBD))),
            ..Default::default()
        };
        let body_rect = std::cell::Cell::new(Rect::NOTHING);
        let run = |events: Vec<egui::Event>| -> SurfaceReport {
            let mut report = SurfaceReport::default();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    report = surface(ctx, Caps::PLUGIN, &spec, &mut |ui, _| {
                        body_rect.set(ui.max_rect());
                    });
                },
            );
            report
        };
        let press = |p: Pos2, down: bool| egui::Event::PointerButton {
            pos: p,
            button: egui::PointerButton::Primary,
            pressed: down,
            modifiers: egui::Modifiers::NONE,
        };

        // The body is handed EXACTLY the size it asked for, under the bar:
        // the bar is added to the window, not taken out of the panel.
        run(vec![]);
        let outer = size + Vec2::new(0.0, CHROME_BAR_H);
        let l = chrome_layout(Rect::from_min_size(at, outer), true);
        assert!(
            (body_rect.get().min - l.body.min).length() < 0.5
                && (body_rect.get().size() - size).length() < 0.5,
            "the body was given {:?}, not its own {size:?} under the bar {:?}",
            body_rect.get(),
            l.body
        );

        // T comes back as a theme cycle, and closes nothing.
        let r = run(vec![egui::Event::Key {
            key: egui::Key::T,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(r.cycle_theme, "the theme key was not carried back");
        assert!(!r.close, "the theme key closed the window");
        assert!(!r.transport, "the theme key was read as a transport");

        // **And SPACE comes back as the transport.** A window drawn with
        // this bar is one you are meant to leave open while you work, and
        // on the desktop it owns the input while it is focused - so without
        // this the main window is unfocused for as long as it is up and
        // nothing can be played. The owner met it on the quantize window:
        // "The quantize menu doesn't allow playback while it's open."
        //
        // The key is asked for with the transport UNGATED, because the
        // table gates Space on the recorder band being shown and this bar
        // cannot see that; the app re-checks. A default-gated ask here
        // would recognise nothing and forward nothing, silently.
        let r = run(vec![egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        }]);
        assert!(r.transport, "Space was not carried back");
        assert!(!r.close, "Space closed the window");
        assert!(!r.cycle_theme, "Space was read as a theme cycle");

        // (A modified Space is refused by `keys::pressed` itself, which
        // returns None for any chord - `RawInput.modifiers` is what it
        // reads, and this harness does not set it, so the case belongs to
        // that function's own tests rather than here.)
        //
        // A frame with no key at all carries nothing.
        let r = run(vec![]);
        assert!(!r.transport && !r.cycle_theme, "an idle frame carried a key");

        // **Not while a field in this window is being TYPED into.** A space
        // in a plugin picker's filter is a space and a `t` is a letter -
        // the same rule the main window keeps under `text_entry_active`.
        // This is the half that makes forwarding safe at all, so it is
        // driven rather than asserted about the source.
        let typed = |events: Vec<egui::Event>| -> SurfaceReport {
            let mut report = SurfaceReport::default();
            let mut text = String::new();
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    report = surface(ctx, Caps::PLUGIN, &spec, &mut |ui, _| {
                        let r = ui.text_edit_singleline(&mut text);
                        r.request_focus();
                    });
                },
            );
            report
        };
        // The first pass puts the focus on; the second presses a key into a
        // field that already has it.
        typed(vec![]);
        for (key, what) in [(egui::Key::Space, "Space"), (egui::Key::T, "the theme key")] {
            let r = typed(vec![egui::Event::Key {
                key,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }]);
            assert!(
                !r.transport && !r.cycle_theme,
                "{what} typed into a field was forwarded as a shortcut"
            );
        }

        // **Both chrome paths carry it.** The harness above runs on
        // `Caps::PLUGIN`, which has no child windows and so exercises only
        // the INLINE bar; the desktop's child-window path is the one the
        // quantize window actually takes. A source scan, because driving a
        // real child viewport needs a window server.
        let src = include_str!("shell.rs");
        let body = &src[..src.find("mod tests {").expect("the test module")];
        assert_eq!(
            body.matches("report.transport |= g.transport;").count(),
            2,
            "one of the two chrome paths drops the transport key"
        );
        assert_eq!(
            body.matches("report.cycle_theme |= g.cycle_theme;").count(),
            2,
            "one of the two chrome paths drops the theme key"
        );

        // A drag on the bar moves the window by what the hand moved.
        let grab = l.drag.center();
        let by = Vec2::new(50.0, 30.0);
        run(vec![egui::Event::PointerMoved(grab), press(grab, true)]);
        run(vec![egui::Event::PointerMoved(grab + by)]);
        run(vec![egui::Event::PointerMoved(grab + by), press(grab + by, false)]);
        run(vec![]);
        let moved = Rect::from_min_size(at + by, outer);
        assert!(
            (body_rect.get().min - chrome_layout(moved, true).body.min).length() < 1.0,
            "the bar was dragged by {by:?} and the body went to {:?}",
            body_rect.get()
        );

        // A drag on the grip grows it.
        let l = chrome_layout(moved, true);
        let grip = l.grip.expect("resizable").center();
        let grow = Vec2::new(40.0, 20.0);
        run(vec![egui::Event::PointerMoved(grip), press(grip, true)]);
        run(vec![egui::Event::PointerMoved(grip + grow)]);
        run(vec![egui::Event::PointerMoved(grip + grow), press(grip + grow, false)]);
        run(vec![]);
        assert!(
            (body_rect.get().width() - l.body.width() - grow.x).abs() < 1.0
                && (body_rect.get().height() - l.body.height() - grow.y).abs() < 1.0,
            "the grip was pulled by {grow:?} and the body became {:?} from {:?}",
            body_rect.get(),
            l.body
        );

        // The cross closes it. Press on one frame, release on the next, as a hand does.
        let l = chrome_layout(Rect::from_min_size(moved.min, outer + grow), true);
        let x = l.close.center();
        run(vec![egui::Event::PointerMoved(x), press(x, true)]);
        let r = run(vec![egui::Event::PointerMoved(x), press(x, false)]);
        assert!(r.close, "the cross did not close the window");
    }
}
