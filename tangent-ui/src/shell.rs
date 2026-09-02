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
    // the desk; the popouts learned this and they go on the press too.
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
    // **The theme key, exactly as the main window reads it** — the keymap
    // is the one table, so the bar does not grow a second idea of what T
    // means. Not while a field in this window is being typed into: "t" in
    // the picker's filter is a letter, and the main window has the same
    // rule under `text_entry_active`.
    g.cycle_theme = !ui.ctx().wants_keyboard_input()
        && crate::keys::pressed(ui.ctx(), crate::keys::Gates::default())
            == Some(crate::keys::KeyAction::CycleTheme);
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
/// **There is no fourth level to promote into.** `winit` exposes exactly
/// three (`AlwaysOnBottom`, `Normal`, `AlwaysOnTop`) and `egui` passes them
/// straight through, so "put this one higher" is not a thing that can be
/// said. The tier is therefore expressed from the other end: the surface
/// that must not be covered keeps the floating level, and every OTHER
/// surface of this app is put DOWN to `Normal` for as long as that claim is
/// held. All three window managers order strictly by level first and
/// front-to-back only WITHIN a level, so a sibling revealed later cannot
/// come out in front however loudly it orders itself forward. It is a rule
/// rather than a race, which is the whole difference: re-asserting the
/// export window's own level on every sibling reveal would be the same race
/// run once more, and `-[NSWindow setLevel:]` with the level it already has
/// is not documented to reorder anything.
///
/// **What it costs.** A demoted surface shares the main window's level, so a
/// press on the main window can put it behind — which is exactly the case
/// [`SurfaceSpec::raise`] already exists to answer, and the app already asks
/// for a raise on that press. On Windows the two levels are `HWND_TOPMOST`
/// and `HWND_NOTOPMOST` and behave the same way. On X11 the level is
/// `_NET_WM_STATE_ABOVE`, which a window manager MAY ignore: there a demoted
/// surface can still land in front, and the `_NET_WM_WINDOW_TYPE` hint is
/// the only other lever. That is a WM already ignoring the float every
/// dialog in this app asks for, so it is not a new failure.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SurfaceTier {
    /// An ordinary floating surface: dialogs, menus, notices, the suite's
    /// plugin windows. What everything here was before there was a tier.
    #[default]
    Floating,
    /// **Nothing of this app's may cover it while it is up.**
    ///
    /// One customer: the export window, which reports on a render that can
    /// run for minutes and is worth nothing the moment something lands on
    /// top of it.
    Uncovered,
}

impl SurfaceTier {
    /// The inline equivalent. A layer has no window level, so the tier is
    /// egui's own paint order: [`egui::Order::Tooltip`] is the band above
    /// the `Foreground` every menu and dialog here uses, and unlike a
    /// window level it is absolute — a later `Area` cannot get in front of
    /// one in a higher order, so inline the claim needs no bookkeeping at
    /// all.
    fn inline_order(self, asked: egui::Order) -> egui::Order {
        match self {
            Self::Floating => asked,
            Self::Uncovered => egui::Order::Tooltip,
        }
    }
}

fn tier_claim_id() -> egui::Id {
    egui::Id::new("surface-uncovered-claim")
}

/// Who holds the [`SurfaceTier::Uncovered`] claim, if anyone.
///
/// One slot, because the claim is a statement about ALL of this app's
/// surfaces and two of them cannot both be the one nothing may cover.
///
/// **Accepted one pass stale, on purpose.** The surfaces of a frame are
/// drawn in whatever order the app happens to call them, and the export
/// window is drawn LAST — after the dialogs, after the notice. A claim good
/// only on the pass it was written would never be seen by the very surfaces
/// it exists to hold down. One frame late is the right answer instead: the
/// claim appears a frame after the export window opens and lapses a frame
/// after it closes, and this window repaints at 30 Hz.
fn uncovered_claim(ctx: &egui::Context, pass: u64) -> Option<egui::Id> {
    ctx.memory(|m| m.data.get_temp::<(u64, egui::Id)>(tier_claim_id()))
        .filter(|(p, _)| pass.saturating_sub(*p) <= 1)
        .map(|(_, who)| who)
}

/// **The window level a surface asks for**, given what it wants and whether
/// somebody else is holding the no-cover claim. See [`SurfaceTier`].
///
/// A pure table so the decision can be read and tested without a window
/// manager: everything about the tier that can be checked headlessly is
/// checked here, and the ordering it produces on a real screen is not
/// something a unit test may claim to have proved.
fn level_for(on_top: bool, tier: SurfaceTier, held_by_other: bool) -> egui::WindowLevel {
    if on_top && (tier == SurfaceTier::Uncovered || !held_by_other) {
        egui::WindowLevel::AlwaysOnTop
    } else {
        egui::WindowLevel::Normal
    }
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
    /// panel, which is parented to the MAIN window, and then "Load .syx..."
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
            tier: SurfaceTier::Floating,
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
    let held_by_other = uncovered_claim(ctx, pass).is_some_and(|who| who != me);
    let want_level = level_for(spec.on_top, spec.tier, held_by_other);
    // Claimed only while the claim is worth anything. A surface that has
    // stood down for the OS's file panel (`on_top: false`) is not above
    // anything, and holding the rest of the app at `Normal` on its behalf
    // would cost them their float for no one's benefit.
    if spec.tier == SurfaceTier::Uncovered && want_level == egui::WindowLevel::AlwaysOnTop {
        ctx.memory_mut(|m| m.data.insert_temp(tier_claim_id(), (pass, me)));
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
        // The one pass on which it goes from hidden to shown.
        let revealing = existing && pass == born + 1;
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
            .with_visible(existing)
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
                if g.grab {
                    // **The OS moves the window, from the press.** A
                    // borderless window has no bar its manager knows about,
                    // so the bar we drew has to say "this is one" — the same
                    // `StartDrag` the popouts send from their own press.
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
        // The tier, inline: a layer's height is its paint order, and
        // `Order` is absolute in a way a window level is not. See
        // [`SurfaceTier::inline_order`].
        .order(spec.tier.inline_order(spec.order))
        .fixed_pos(rect.min)
        .show(ctx, |ui| {
            ui.set_clip_rect(rect);
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

    /// **The tier's whole rule, as a table.**
    ///
    /// One surface may say "nothing of ours covers me"; everything else goes
    /// down to the ordinary level while it does. The two ways to get this
    /// wrong are to leave a sibling floating (the bug: the notice landing on
    /// the export waveform) and to put the claimant itself down along with
    /// the rest.
    ///
    /// A unit test on a level is NOT a proof that the windows stack that way
    /// on a screen — the window manager has the last word and no headless
    /// test can ask it. What it does prove is that the right level is asked
    /// for in every combination, which is the half of this that lives in
    /// this crate.
    #[test]
    fn the_uncovered_tier_puts_every_other_surface_down_and_never_itself() {
        use egui::WindowLevel::{AlwaysOnTop, Normal};
        // Nobody claiming: every floating surface floats, exactly as before
        // the tier existed.
        assert_eq!(level_for(true, SurfaceTier::Floating, false), AlwaysOnTop);
        assert_eq!(level_for(true, SurfaceTier::Uncovered, false), AlwaysOnTop);
        // Somebody else claiming: the ordinary surfaces go down...
        assert_eq!(level_for(true, SurfaceTier::Floating, true), Normal);
        // ...and the claimant does not, which is the point. Two claimants at
        // once is not a state this app can reach, but neither of them may be
        // put down by the other's claim if it ever does.
        assert_eq!(level_for(true, SurfaceTier::Uncovered, true), AlwaysOnTop);
        // `on_top: false` still wins over any tier. It is the one moment a
        // surface must NOT float — the OS's own file panel is up, parented
        // to the main window, and nothing else can get in front of it.
        for tier in [SurfaceTier::Floating, SurfaceTier::Uncovered] {
            for held in [false, true] {
                assert_eq!(
                    level_for(false, tier, held),
                    Normal,
                    "{tier:?} floated over a native panel (claim held: {held})"
                );
            }
        }
        // Inline the tier is a paint order, and Tooltip really is above the
        // Foreground every menu and dialog here uses.
        assert_eq!(
            SurfaceTier::Floating.inline_order(egui::Order::Foreground),
            egui::Order::Foreground
        );
        assert_eq!(
            SurfaceTier::Uncovered.inline_order(egui::Order::Foreground),
            egui::Order::Tooltip
        );
        assert!(
            egui::Order::Tooltip > egui::Order::Foreground,
            "the inline tier does not actually paint above the dialogs"
        );
        // A floating surface keeps whatever order it asked for — the submenu
        // sits above the menu by asking, and the tier must not flatten that.
        assert_eq!(
            SurfaceTier::Floating.inline_order(egui::Order::Middle),
            egui::Order::Middle
        );
    }

    /// **The claim reaches the sibling, and it lapses.**
    ///
    /// The end-to-end half: run `surface` the way the app does and read the
    /// level it actually asks the host for. The order matters — the app
    /// draws its dialogs and its notice BEFORE the export window, so a
    /// sibling reads a claim written on the previous pass, which is why
    /// `uncovered_claim` accepts one that is a pass old.
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
        // What level was asked for on this frame. `Caps::DESKTOP` takes the
        // window path; with no viewport renderer attached egui runs the body
        // inline and the commands land on the root's output, which is
        // exactly the list eframe would hand to winit.
        let run = |spec: &SurfaceSpec<'_>| -> Vec<egui::WindowLevel> {
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
        };

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
        let mut run = |events: Vec<egui::Event>| -> SurfaceReport {
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
