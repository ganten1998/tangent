//! **Three AppKit facts winit does not surface, and one hook it does not
//! install.**
//!
//! The app's layout is aspect-locked by construction: every band's height is a
//! fixed fraction of the window's width, so there is exactly one height per
//! width and every other height is empty ground. Keeping a window on that
//! curve has, until now, meant watching the size until it stopped moving and
//! then teleporting the window onto the curve — which cannot track a hand by
//! construction, and which fights a tiling window manager forever because the
//! tiler re-tiles and the correction fires again.
//!
//! Two things fix that, and neither is reachable from `tangent-ui`:
//!
//! 1. **`-[NSWindow inLiveResize]`** answers "is a hand on our edge right
//!    now", which is the difference between a user dragging and a window
//!    manager placing us. `egui::ViewportInfo` has no such field and winit
//!    0.30 emits no event for `windowWillStartLiveResize:`.
//! 2. **`windowWillResize:toSize:`** is asked by AppKit for every intermediate
//!    size during a live resize, and whatever it returns is what the window
//!    becomes. Answering it puts the OS itself on the aspect curve, so there
//!    is no wrong size to correct and no correction to see.
//!
//! `NSWindow.contentAspectRatio` would be the obvious API and is dead here:
//! winit's own `windowWillStartLiveResize:` calls `setContentResizeIncrements`,
//! which zeroes the aspect at the instant a drag begins.
//!
//! **Why a class patch rather than our own delegate.** The window's delegate
//! belongs to winit and drives its whole event loop; replacing it breaks the
//! app. winit's `WinitWindowDelegate` does not implement
//! `windowWillResize:toSize:` at all, so the method can simply be ADDED to the
//! class — no swizzling, nothing displaced. If a future winit implements it,
//! `class_addMethod` returns false, the hook is reported as unavailable, and
//! the app's own correction stays armed.
//!
//! # The third fact: "above the main window" and "above every other program
//! on the machine" are one request to winit and two to AppKit
//!
//! 3. **`-[NSWindow addChildWindow:ordered:]`** is what AppKit has for "keep
//!    this window above THAT one". winit has no word for it.
//!
//! A face — a plugin's editor, the IR wizard, a notice — has to stay in front
//! of the window it belongs to, so `shell.rs` asks for
//! `egui::WindowLevel::AlwaysOnTop` for every surface with `on_top`. winit
//! answers that with `kCGFloatingWindowLevel`, and a window LEVEL is a fact
//! about the whole machine, not about this app: level 3 sits above every
//! level-0 window in every process. So the face stayed in front of the
//! owner's terminal as well — and the main window, still at level 0, did not.
//! The report, verbatim: "a the window pushes the MAIN window behind you
//! while IT stays in front of you (you as in the one in the terminal
//! window)". The requirement was only ever "in front of the MAIN window,
//! without taking exclusivity over input"; outranking every other program was
//! never asked for and is what a level buys.
//!
//! A CHILD window is the request that was actually meant. Ordered
//! `NSWindowAbove`, AppKit keeps it above its parent, moves it with the
//! parent, orders it out when the parent is hidden or minimised and brings it
//! back with it — and none of that is a level, so the pair outranks nothing
//! outside this process. Switch to the terminal and both Tangent windows go
//! behind it together; switch back and both return together. Key status is
//! untouched, so the "no exclusivity over input" half still holds: a single
//! click into the main window still moves the dial under the cursor, and the
//! face stays drawn on top, un-clicked.
//!
//! **Why the adoption happens HERE, from an enumeration, and not at a call
//! site.** There is no call site to put it at. The faces are `egui` child
//! viewports, made by winit inside `eframe`, and `tangent-ui` has nothing but
//! `WindowLevel` to say. The VST3 editor windows are made by
//! `tangent_host::editor` with AppKit directly, and that module's own docs
//! record why it could not do this itself: "nothing hands this code the main
//! window to be a child *of*". This module is handed the main window every
//! frame, so it reaches both from the other side — `NSApp.windows` is the one
//! list that has all of them on it.
//!
//! # Safety
//!
//! The callback runs inside AppKit's nested event-tracking run loop, on the
//! main thread, while the user's mouse button is down. A Rust panic unwinding
//! across the Objective-C boundary is undefined behaviour, and a crash there
//! is a crash mid-drag. So the callback allocates nothing, locks nothing,
//! logs nothing, and calls no fallible API without a total fallback: every
//! path that cannot answer returns the size AppKit proposed, unchanged.

#![cfg(target_os = "macos")]

use std::sync::atomic::{AtomicPtr, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{define_class, msg_send, sel, ClassType, DefinedClass, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSEvent, NSFloatingWindowLevel, NSModalResponse, NSModalResponseOK,
    NSNormalWindowLevel, NSOpenPanel, NSOpenSavePanelDelegate, NSTextField, NSView, NSWindow,
    NSWindowLevel, NSWindowOrderingMode,
};
use objc2_foundation::{
    MainThreadMarker, NSDictionary, NSError, NSLocalizedDescriptionKey, NSPoint, NSRect, NSSize,
    NSString, NSURL,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

/// What the host can see, for [`tangent_ui::host::Gesture`] and the aspect lock.
#[derive(Debug, Default, Clone, Copy)]
pub struct Facts {
    pub gesture: tangent_ui::host::Gesture,
    pub os_aspect_lock: bool,
}

/// The window the hook is allowed to touch.
///
/// Every winit window in the process shares the delegate CLASS, so the patch
/// reaches the detached recorder, the fretboard, chord and theory viewports
/// and the Welcome card too. This pointer is the only thing that keeps their
/// resizes untouched, so it is compared before anything else happens.
static TARGET: AtomicPtr<NSWindow> = AtomicPtr::new(std::ptr::null_mut());

/// The height curve, as a table the callback can read without allocating.
static CURVE: AtomicPtr<Curve> = AtomicPtr::new(std::ptr::null_mut());

/// The table the CURVE pointer held one generation ago.
///
/// **This is what stops the rebuild being a leak.** The callback may be
/// holding the pointer it read a moment ago, so a table cannot be freed the
/// instant it is replaced — but keeping every one of them forever is a leak
/// of thirty kilobytes per rebuild, and `refresh_curve` runs every frame. One
/// generation of delay is enough: by the time a table reaches here and is
/// displaced again, two whole swaps have happened on this thread and nothing
/// can still be reading it.
static RETIRED: AtomicPtr<Curve> = AtomicPtr::new(std::ptr::null_mut());

/// How many times the table has been rebuilt. Read by `IVORY_PACE_PROBE`.
///
/// A number rather than a belief: the comment that used to sit on the leak
/// asserted "a handful of times in a session", and nothing measured it.
pub static REBUILDS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

thread_local! {
    /// **A strong reference, so `TARGET` can never dangle.**
    ///
    /// The obvious mistake here is to store the raw pointer alone; then the
    /// window can be released and the callback compares against freed memory.
    static WINDOW: std::cell::RefCell<Option<objc2::rc::Retained<NSWindow>>> =
        const { std::cell::RefCell::new(None) };
    /// Whether the class patch has been attempted, and what it answered.
    static HOOKED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Content height at content width `w_min + i`, in whole points.
///
/// A table rather than a call, because the callback cannot reach back into the
/// GUI crate: it runs while AppKit owns the thread, and `TangentApp` is behind a
/// `&mut self` the caller is already holding.
struct Curve {
    w_min: u32,
    h: Box<[f32]>,
}

/// The widest window this table covers. Beyond it the callback declines.
const W_MAX: u32 = 8192;

impl Curve {
    fn clamp_w(&self, w: f32) -> f32 {
        let hi = self.w_min + self.h.len().saturating_sub(1) as u32;
        (w.round().max(self.w_min as f32)).min(hi as f32)
    }

    fn height_at(&self, w: f32) -> f32 {
        let i = (self.clamp_w(w) as u32).saturating_sub(self.w_min) as usize;
        self.h.get(i).copied().unwrap_or(0.0)
    }

    /// **The point on the curve nearest the rectangle being asked for.**
    ///
    /// For a corner drag both dimensions move at once, so there is no single
    /// input to follow — and following just one of them is what made corners
    /// feel awful: drag a corner downward, the width barely moves, the window
    /// barely resizes, the cursor runs away, and then a stray pixel sideways
    /// snaps the height a long way.
    ///
    /// Minimising the distance to the proposed rectangle instead moves the
    /// answer smoothly as the cursor moves, in any direction, and reduces to
    /// "follow that edge" when only one dimension has changed. Scanned rather
    /// than solved because the curve is a truncation staircase, seeded from
    /// both single-dimension answers so the scan starts near the minimum.
    fn nearest(&self, want_w: f32, want_h: f32) -> f32 {
        if self.h.is_empty() {
            return self.w_min as f32;
        }
        let cost = |i: usize| {
            let w = (self.w_min + i as u32) as f32;
            let dw = w - want_w;
            let dh = self.h[i] - want_h;
            dw * dw + dh * dh
        };
        let seeds = [
            (self.clamp_w(want_w) as u32).saturating_sub(self.w_min) as usize,
            (self.width_for_height(want_h) as u32).saturating_sub(self.w_min) as usize,
        ];
        let last = self.h.len() - 1;
        let mut best = seeds[0].min(last);
        let mut best_c = cost(best);
        for seed in seeds {
            let seed = seed.min(last);
            let lo = seed.saturating_sub(64);
            let hi = (seed + 64).min(last);
            for i in lo..=hi {
                let c = cost(i);
                if c < best_c {
                    best_c = c;
                    best = i;
                }
            }
        }
        (self.w_min + best as u32) as f32
    }

    /// The width whose height is closest to `target`.
    ///
    /// A seeded scan rather than a binary search: the height curve truncates
    /// per band and clamps the theory term, so it is not something to assume
    /// monotone. The seed gets within a point or two and the scan settles it
    /// exactly, which is better than the ratio-solve-then-remeasure the app's
    /// own arithmetic uses — that only approximates the truncation staircase.
    fn width_for_height(&self, target: f32) -> f32 {
        if self.h.is_empty() {
            return self.w_min as f32;
        }
        let seed = match self.h.partition_point(|&h| h < target) {
            i if i >= self.h.len() => self.h.len() - 1,
            i => i,
        };
        let lo = seed.saturating_sub(16);
        let hi = (seed + 16).min(self.h.len() - 1);
        let mut best = seed;
        let mut best_d = f32::INFINITY;
        for i in lo..=hi {
            let d = (self.h[i] - target).abs();
            if d < best_d {
                best_d = d;
                best = i;
            }
        }
        (self.w_min + best as u32) as f32
    }
}

/// **Called once a frame from the desktop app.**
///
/// Resolves the window, installs the hook on first success, refreshes the
/// curve when the layout's answer has moved, and reports what the OS can see.
pub fn publish(
    frame: &eframe::Frame,
    height_at: &dyn Fn(f32) -> f32,
    w_floor: f32,
) -> Facts {
    let Some(window) = resolve(frame) else {
        return Facts::default();
    };
    refresh_curve(height_at, w_floor);
    let hooked = install(&window);
    adopt_floating(&window);
    guard_child_frames(&window);
    Facts {
        gesture: tangent_ui::host::Gesture {
            live_resize: window.inLiveResize(),
            // A global check, not an egui one: during a native window drag
            // AppKit does not deliver mouse events to the content view at all,
            // so egui believes nothing is pressed.
            mouse_down: NSEvent::pressedMouseButtons() != 0,
            // The whole point of this module: `live_resize` is a real answer
            // here, so "a size nobody asked for" may be read as a window
            // manager. Everywhere else that inference is unsafe.
            resize_known: true,
            // And so is `mouse_down` above: `NSEvent::pressedMouseButtons`
            // is a global read that AppKit keeps answering through a native
            // window drag, which is the one gesture egui cannot see the end
            // of. See `host::Gesture::mouse_known`.
            mouse_known: true,
        },
        os_aspect_lock: hooked,
    }
}

/// The `NSWindow` behind eframe's frame, cached and kept alive.
fn resolve(frame: &eframe::Frame) -> Option<objc2::rc::Retained<NSWindow>> {
    if let Some(w) = WINDOW.with(|w| w.borrow().clone()) {
        return Some(w);
    }
    let handle = frame.window_handle().ok()?;
    let RawWindowHandle::AppKit(h) = handle.as_raw() else {
        return None;
    };
    // SAFETY: eframe hands us a live `NSView` pointer for the window it is
    // drawing, valid for as long as that window exists.
    let view: &NSView = unsafe { h.ns_view.cast::<NSView>().as_ref() };
    let window = view.window()?;
    TARGET.store(
        objc2::rc::Retained::as_ptr(&window).cast_mut(),
        Ordering::Release,
    );
    WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
    Some(window)
}

/// Rebuild the table when the layout's answer has moved.
///
/// Probes the live function at four widths and compares against the table's
/// own entries, so a band added later cannot be forgotten — unlike a
/// hand-written fingerprint of whichever settings fields are believed to
/// matter today.
fn refresh_curve(height_at: &dyn Fn(f32) -> f32, w_floor: f32) {
    let w_min = (w_floor.max(1.0) as u32).min(W_MAX - 1);
    let cur = CURVE.load(Ordering::Acquire);
    let stale = if cur.is_null() {
        true
    } else {
        // SAFETY: `CURVE` only ever holds a leaked `Box<Curve>`, never freed.
        let c = unsafe { &*cur };
        c.w_min != w_min
            || [w_min, w_min + 200, 1300, 2560].iter().any(|&w| {
                let w = w.clamp(c.w_min, c.w_min + c.h.len().saturating_sub(1) as u32);
                (c.height_at(w as f32) - height_at(w as f32)).abs() > 0.5
            })
    };
    if !stale {
        return;
    }
    let h: Box<[f32]> = (w_min..=W_MAX).map(|w| height_at(w as f32)).collect();
    let next = Box::into_raw(Box::new(Curve { w_min, h }));
    REBUILDS.fetch_add(1, Ordering::Relaxed);
    // **Swap, retire, and free the one before that.**
    //
    // The callback may still hold the pointer it read a moment ago, so the
    // displaced table cannot be freed here — but it must be freed eventually,
    // because this function runs every frame and thirty kilobytes a frame is
    // four megabytes a second at the rate this window now draws. One
    // generation of delay costs one spare table and bounds the whole thing.
    let prev = CURVE.swap(next, Ordering::Release);
    let older = RETIRED.swap(prev, Ordering::Release);
    if !older.is_null() {
        // SAFETY: `older` was displaced from CURVE a full generation ago, on
        // this same thread, so no reader can still be holding it.
        drop(unsafe { Box::from_raw(older) });
    }
}

/// Add `windowWillResize:toSize:` to winit's delegate class, once.
///
/// Answers whether the hook is live — which is what the app uses to decide
/// whether to keep its own correction armed.
fn install(window: &NSWindow) -> bool {
    if let Some(done) = HOOKED.with(std::cell::Cell::get) {
        return done;
    }
    // **The OS-side lock follows the app's own switch.** With
    // `ASPECT_SNAP` off the app makes no correction, and leaving AppKit
    // answering `windowWillResize:` would keep the window on the aspect
    // anyway — a free aspect that is not free, and impossible to explain.
    if !tangent_ui::settings::ASPECT_SNAP {
        HOOKED.with(|h| h.set(Some(false)));
        return false;
    }
    if std::env::var("IVORY_ASPECT_HOOK").as_deref() == Ok("0") {
        HOOKED.with(|h| h.set(Some(false)));
        return false;
    }
    let ok = (|| {
        let delegate = window.delegate()?;
        let raw: *const AnyObject = objc2::rc::Retained::as_ptr(&delegate).cast();
        // SAFETY: a live delegate object; `class` reads its isa pointer.
        let cls: &AnyClass = unsafe { (*raw).class() };
        // **Refuse anything that is not the class we studied.** A future winit
        // that renames its delegate, or implements this selector itself, must
        // not be patched blind.
        if cls.name().to_str().ok()? != "WinitWindowDelegate" {
            return None;
        }
        // SAFETY: `will_resize` has exactly the signature the encoding
        // describes, and `Imp` is `extern "C-unwind"` — declaring the callback
        // `extern "C"` would compile and then be wrong at the ABI boundary.
        let added = unsafe {
            objc2::ffi::class_addMethod(
                (cls as *const AnyClass).cast_mut().cast(),
                sel!(windowWillResize:toSize:),
                std::mem::transmute::<
                    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut NSWindow, NSSize) -> NSSize,
                    unsafe extern "C-unwind" fn(),
                >(will_resize),
                c"{CGSize=dd}@:@{CGSize=dd}".as_ptr(),
            )
        };
        if !added.as_bool() {
            return None;
        }
        // AppKit caches which selectors a delegate responds to when the
        // delegate is set, and winit set this one long before we patched its
        // class. Re-setting the same object forces that cache to be rebuilt.
        // Harmless: winit holds the strong reference, the property is weak.
        unsafe { window.setDelegate(Some(&delegate)) };
        Some(())
    })()
    .is_some();
    HOOKED.with(|h| h.set(Some(ok)));
    ok
}

/// **The hook. A table lookup and nothing else.**
///
/// # Safety
///
/// Runs on the main thread inside AppKit's nested event-tracking loop with the
/// mouse button down. Nothing here allocates, locks, logs, panics, or calls
/// back into the app; every path that cannot answer returns `proposed`.
unsafe extern "C-unwind" fn will_resize(
    _this: *mut AnyObject,
    _cmd: Sel,
    sender: *mut NSWindow,
    proposed: NSSize,
) -> NSSize {
    // Not our window. Every other winit window shares this delegate class, so
    // this is what keeps the detached recorder and the theory viewports free.
    if sender.is_null() || !std::ptr::eq(sender, TARGET.load(Ordering::Acquire)) {
        return proposed;
    }
    let curve = CURVE.load(Ordering::Acquire);
    if curve.is_null() {
        return proposed;
    }
    // SAFETY: `CURVE` holds a leaked `Box<Curve>`; `sender` is a live window.
    let (curve, win) = unsafe { (&*curve, &*sender) };

    // **Only ever constrain a HAND on the edge.**
    //
    // AppKit asks this delegate for programmatic frame changes as well as for
    // live resizes — which means every placement a tiling window manager
    // makes comes through here too. Answering those put the window back on
    // our aspect, the tiler re-placed it, and the two fought: exactly the
    // wild jumping this hook was meant to end, moved from the app into the
    // callback. A window manager's geometry is not ours to argue with; the
    // app's own `main_wm_managed` latch says the same thing one level up.
    if !win.inLiveResize() {
        return proposed;
    }

    // **Chrome measured, never assumed.** It is about 32 points with
    // decorations and exactly 0 in borderless mode, which is toggleable at
    // runtime — so a hardcoded number drifts by a title bar per drag.
    let frame = win.frame();
    let content = win.contentRectForFrameRect(frame);
    let chrome_w = frame.size.width - content.size.width;
    let chrome_h = frame.size.height - content.size.height;
    if !chrome_w.is_finite() || !chrome_h.is_finite() {
        return proposed;
    }
    let want_w = proposed.width - chrome_w;
    let want_h = proposed.height - chrome_h;

    // Whichever edge moved leads, measured against the window's CURRENT
    // content rect — AppKit gives us both, so unlike the app's own arithmetic
    // there is nothing to remember and nothing to get out of step.
    // **Which edge leads — and a corner is not a close call between two.**
    //
    // This asked "did the height change more than the width", which is right
    // for a single edge and wrong for a CORNER: dragging one moves both, so
    // the answer flipped between width-led and height-led from frame to
    // frame and the window oscillated instead of tracking. Hence the corner
    // drag being horrific while the edges were fine.
    //
    // So the height leads only when the width did not move at all — a pure
    // top or bottom edge. Everything else, corners included, follows the
    // width, which is the dimension this layout is actually a function of.
    let dw = (want_w - content.size.width).abs();
    let dh = (want_h - content.size.height).abs();
    let w = if dw >= 0.5 && dh >= 0.5 {
        // A CORNER: both edges moved, so there is nothing to "follow".
        curve.nearest(want_w as f32, want_h as f32)
    } else if dw < 0.5 && dh >= 0.5 {
        // A pure top or bottom edge: the height leads and the width follows.
        curve.width_for_height(want_h as f32)
    } else {
        // A pure side, and the do-nothing case: the width leads.
        curve.clamp_w(want_w as f32)
    };
    let h = curve.height_at(w);
    if !w.is_finite() || !h.is_finite() || h <= 0.0 {
        return proposed;
    }
    NSSize::new(f64::from(w) + chrome_w, f64::from(h) + chrome_h)
}

// ───────────────────────────────────────────────────────────────────────────
// Above the main window, and no higher
// ───────────────────────────────────────────────────────────────────────────

/// Where a window sits in this app's own hierarchy, as [`adopt_floating`]
/// finds it. Three states and not a `bool`, because "somebody else's child"
/// and "nobody's child" are opposite answers: one is hands-off, the other is
/// the whole job.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Parentage {
    /// `-[NSWindow parentWindow]` is nil.
    Orphan,
    /// The main window is its parent, so we adopted it on an earlier frame.
    Ours,
    /// Somebody else's child — a sheet's owner, a plugin's own panel.
    Theirs,
}

/// What one window looks like to [`adopt_floating`], and everything the
/// decision is allowed to depend on.
///
/// Split out from AppKit on purpose: the decision is the part that can be
/// wrong in a way nobody notices, and it is the only part a test can reach
/// without a window server. See [`verdict`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct WindowFacts {
    /// `-[NSWindow level]`.
    level: NSWindowLevel,
    /// `-[NSWindow parentWindow]`, read against the main window.
    parent: Parentage,
    /// `-[NSWindow isSheet]` or a non-nil `sheetParent`.
    sheet: bool,
    /// This IS the main window.
    is_main: bool,
    /// `-[NSWindow isVisible]`.
    visible: bool,
    /// This window has just BECOME `-[NSApplication keyWindow]`.
    ///
    /// **The arrival and not the state, which is the whole difference.** A
    /// click is an event; "is key" is a condition that then holds for
    /// minutes. Acting on the condition would mean re-seating the focused
    /// face to the front on every frame after it, which would quietly undo
    /// every promotion the app made in the meantime — the export window
    /// coming back up a rung under a closing dialog, most of all. Acting on
    /// the arrival puts a clicked window in front once and then leaves the
    /// ordering to whoever asks for it next.
    key_arrived: bool,
    /// It is already the frontmost of the main window's children, so there is
    /// nothing to bring forward. `childWindows` is back-to-front, so this is
    /// its `lastObject`.
    topmost_child: bool,
}

/// What to do with one window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Verdict {
    /// Not ours to touch.
    Leave,
    /// Floating and unclaimed: adopt it, then put it down.
    Adopt,
    /// Ours already, and either floating AGAIN — `shell.rs` promoting it a
    /// rung — or key while something else is in front of it, which is a click
    /// AppKit did not act on. Either way: pull it out of the child list and
    /// put it back at the top, which is the only thing that moves a child.
    Reseat,
}

/// **Which windows this app is allowed to adopt, as a pure function.**
///
/// Everything here is an exclusion but the last line, and each exclusion is a
/// way of getting it wrong that would not look like a bug in a screenshot.
///
/// **The native file panels are excluded by the LEVEL test, and the reason is
/// not the one it is natural to assume.** `NSOpenPanel` and `NSSavePanel`
/// look like they ought to run at `NSModalPanelWindowLevel` — they are
/// `NSPanel`s and they are modal — and they do not. Measured on this machine
/// with a twenty-line AppKit program that opens both and prints
/// `-[NSWindow level]`: a fresh panel, a panel shown modeless (which is the
/// path `rfd` takes, having no parent window to attach to) and a panel shown
/// as a sheet are all at `NSNormalWindowLevel`, level 0. So they are below
/// the floating level, not above it, and "exactly floating" passes over them
/// either way — but the margin is one rung down rather than five up, which is
/// worth knowing before somebody widens this to `> NSNormalWindowLevel` and
/// silently adopts the owner's Open dialog into the main window.
///
/// **`Ours` and not floating is `Leave`, and that is the tier ladder.**
/// `shell.rs` ranks its surfaces by putting the LOWER one DOWN to `Normal`
/// and leaving the higher one floating — there being no fourth window level
/// to promote into. So the top rung is the only rung that is ever floating,
/// the only rung that is ever adopted, and a surface that has been stood down
/// under a modal question is passed over here exactly as it was before any of
/// this existed. When the question closes and the render is promoted back,
/// that promotion is a level change this function CAN see, which is what
/// [`Verdict::Reseat`] is for: the level command used to do the reordering by
/// itself, and once the level is being taken away again it has to be done by
/// hand.
///
/// **And the same for a click, because AppKit does not do it.** Measured:
/// once two windows are children of one parent, their order relative to each
/// other is frozen at the order they were adopted in. A click on the back one
/// makes it the key window and leaves it exactly where it was;
/// `makeKeyAndOrderFront:`, `orderFront:` and `orderWindow:relativeTo:` all
/// return without moving it. That is a real behaviour lost to the child
/// relationship — two floating siblings used to reorder on a click like any
/// other pair of windows — so it is put back here: a child that is KEY and is
/// not the frontmost child is a click AppKit declined to act on, and it gets
/// re-seated.
///
/// The cost of that, stated plainly: a demoted surface can now be clicked
/// back in front of the one that outranks it, which a window level would not
/// have allowed. It takes a deliberate click ON the lower surface to do it,
/// the higher one is one click from returning, and the app's own `raise`
/// still comes through as a level change and re-seats. Weighed against a face
/// that will not come forward when it is clicked, which happens every time
/// two of them overlap, this is the better failure.
fn verdict(w: WindowFacts) -> Verdict {
    // The parent cannot be its own child. There is no other thing this could
    // mean and AppKit is not the place to find out what it does with it.
    if w.is_main {
        return Verdict::Leave;
    }
    // A sheet is already bound to a window by a stronger relationship than
    // this one, and AppKit positions and orders it itself.
    if w.sheet {
        return Verdict::Leave;
    }
    // Somebody else's child is somebody else's arrangement; re-parenting it
    // would silently break whatever that arrangement was for.
    if w.parent == Parentage::Theirs {
        return Verdict::Leave;
    }
    // **Born hidden stays hidden.** `shell.rs` creates every surface
    // invisible and reveals it a pass later, on purpose — a window that
    // exists before anything has been drawn into it shows one frame of
    // undefined back buffer. `addChildWindow:` orders the child IN, measured:
    // a window that answered `isVisible` NO before the call answers YES
    // after. So adopting one early would put that undrawn frame on the screen
    // and undo the whole reason for the delay. It is visible next pass and
    // adopted then.
    if !w.visible {
        return Verdict::Leave;
    }
    if w.level == NSFloatingWindowLevel {
        return if w.parent == Parentage::Ours {
            Verdict::Reseat
        } else {
            Verdict::Adopt
        };
    }
    // Ours, just clicked, and still behind something: the click case above.
    if w.parent == Parentage::Ours && w.key_arrived && !w.topmost_child {
        return Verdict::Reseat;
    }
    // Not floating and not clicked: a window that never asked to be in front,
    // one of ours that has been put down under a higher tier, or one of ours
    // already sitting where it belongs. None of the three wants anything.
    Verdict::Leave
}

/// Whether the adoption is armed. `TANGENT_CHILD_WINDOWS=0` disarms it.
///
/// The same escape hatch `IVORY_ASPECT_HOOK` is: this is AppKit behaviour on
/// one person's machine and the honest position is that a rebuild should not
/// be the only way to back it out. Cached rather than read per frame, because
/// `std::env::var` allocates and this runs in the draw loop.
fn adoption_armed() -> bool {
    thread_local! {
        static ARMED: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
    }
    ARMED.with(|a| {
        if let Some(known) = a.get() {
            return known;
        }
        let armed = std::env::var("TANGENT_CHILD_WINDOWS").as_deref() != Ok("0");
        a.set(Some(armed));
        armed
    })
}

/// **Make this app's floating windows children of the main window instead.**
///
/// Called once a frame from [`publish`], because the level is not something
/// that is set once: `shell.rs` sends it again on every reveal, on every
/// raise and on every change of rung, and each of those has to be caught and
/// converted. Enumerating `NSApp.windows` is a handful of pointer reads on a
/// list that is four or five long, which is why it can afford to be a sweep
/// rather than a notification.
///
/// # The order of operations is the safety property
///
/// The risk in this whole change is a window that gets its level taken away
/// and does NOT get the child relationship in exchange: that window falls
/// behind the main window and cannot be got back, which is the exact bug the
/// floating level was introduced to fix and is worse than the one being fixed
/// here. So the adoption happens FIRST, its result is read back from AppKit
/// rather than assumed, and the level is only lowered once `parentWindow` has
/// actually come back as the main window. A failed adoption therefore leaves
/// the window floating — today's behaviour, unchanged, in front of the
/// terminal and all — rather than sinking it. The failure is impossible to
/// make worse rather than merely unlikely, which is the property worth having
/// because nobody is watching for it.
///
/// # Re-seating is remove-then-re-add, and nothing else works
///
/// Measured, because it is not what the API suggests. With two windows
/// adopted into one parent, `orderFront:`, `makeKeyAndOrderFront:` and
/// `orderWindow:NSWindowAbove relativeTo:0` all leave their order exactly as
/// it was — a child's z-order relative to its siblings is fixed at the moment
/// it is adopted. Re-adding it without removing it first does nothing either;
/// AppKit sees a child it already has. `removeChildWindow:` followed by
/// `addChildWindow:ordered:NSWindowAbove` is the one sequence that moves it,
/// and it does not disturb key status.
///
/// The window is parentless for the width of those two calls, and the two
/// re-seats are in different states while it is. A window re-seated because
/// it floated AGAIN is still at the floating level in the gap, so that gap is
/// the pre-fix state and not a sunken window. A window re-seated because it
/// was CLICKED was put down to `NSNormalWindowLevel` when it was adopted, so
/// for those two calls it is an ordinary orphan that the main window could
/// cover — an earlier draft of this paragraph claimed the floating level for
/// both cases, and it was only ever true of one. What makes the second safe
/// is not the level but the gap: both calls happen inside one turn of the
/// main thread with no run-loop iteration between them, so there is no moment
/// at which anything can be ordered into it.
///
/// # What this changes that was not asked for
///
/// A surface that has been adopted and is LATER put down a rung by
/// `shell.rs` stays a child, so it stays above the main window instead of
/// being coverable by it. `shell.rs` records that coverability as a cost of
/// the ladder ("a press on the main window can put it behind"), not as a
/// goal, and the ladder's actual intent — the render never covers a question,
/// an announcement never covers the render — is preserved by `Reseat` pulling
/// each newly promoted surface back to the front of the sibling group.
fn adopt_floating(main: &NSWindow) {
    if !adoption_armed() {
        return;
    }
    // SAFETY-adjacent: `MainThreadMarker::new` is the check, not an
    // assertion. `publish` is only ever called from eframe's draw, which is
    // the main thread, and if that ever stops being true this returns rather
    // than touching AppKit from the wrong one.
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    let main_ptr: *const NSWindow = main;
    let is_main = |w: &NSWindow| std::ptr::eq(std::ptr::from_ref(w), main_ptr);
    // **The key window as an EDGE.** Held as a bare address that is compared
    // and never dereferenced, so a window released since the last frame is
    // harmless: the worst an address reused by a different window can do is
    // spend or skip one re-seat.
    thread_local! {
        static LAST_KEY: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
    }
    let key = app.keyWindow();
    let key_addr = key
        .as_ref()
        .map_or(0, |k| objc2::rc::Retained::as_ptr(k) as usize);
    let key_is_new = key_addr != 0 && key_addr != LAST_KEY.with(std::cell::Cell::get);
    LAST_KEY.with(|k| k.set(key_addr));
    // `childWindows` is back-to-front, so the LAST one is the frontmost.
    let front_child = main.childWindows().and_then(|c| c.lastObject());
    let windows = app.windows();
    for i in 0..windows.count() {
        let window = windows.objectAtIndex(i);
        let parent = match window.parentWindow() {
            None => Parentage::Orphan,
            Some(p) if is_main(&p) => Parentage::Ours,
            Some(_) => Parentage::Theirs,
        };
        let same = |other: &Option<objc2::rc::Retained<NSWindow>>| {
            other
                .as_ref()
                .is_some_and(|o| std::ptr::eq(objc2::rc::Retained::as_ptr(o), &*window))
        };
        let facts = WindowFacts {
            level: window.level(),
            parent,
            // Both, because they answer at different moments: `isSheet` is
            // the style, `sheetParent` the live attachment.
            sheet: window.isSheet() || window.sheetParent().is_some(),
            is_main: is_main(&window),
            visible: window.isVisible(),
            key_arrived: key_is_new && same(&key),
            topmost_child: same(&front_child),
        };
        match verdict(facts) {
            Verdict::Leave => continue,
            // The only thing that moves a child among its siblings. See the
            // section above — every gentler ordering call was measured doing
            // nothing at all.
            Verdict::Reseat => main.removeChildWindow(&window),
            Verdict::Adopt => {}
        }
        // SAFETY: two live `NSWindow`s on the main thread, neither of which
        // is the other and neither of which is currently parented. objc2
        // marks this unsafe for the cycle a window adopted into itself would
        // make; `verdict` and the `removeChildWindow` above are what rule
        // that out.
        unsafe { main.addChildWindow_ordered(&window, NSWindowOrderingMode::Above) };
        // **Read the answer back before spending the level on it.** This one
        // line is the difference between a failed adoption leaving the window
        // where it already was and a failed adoption sinking it behind the
        // main window with no way back.
        if window.parentWindow().is_some_and(|p| is_main(&p)) {
            window.setLevel(NSNormalWindowLevel);
        }
    }
}

/// How many times a child window was put back where it was. Diagnostics,
/// like `REBUILDS`: a number that climbs on somebody's machine says the
/// window manager is fighting, which is the thing to know before anything is
/// changed.
pub static REPAIRS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// After this many consecutive repairs the new frame is accepted. A window
/// AppKit insists on moving — one it deems off-screen, say — would otherwise
/// be moved twice a frame for ever; three is enough to undo a jump and not
/// enough to be a fight.
const MAX_REPAIRS: u8 = 3;

/// **A plugin's window moves only under a hand or with its parent.**
///
/// The owner: "if I open a plugin window and tab to another workspace/out,
/// the plugin window should NOT spawn back at the bottom right of my
/// screen. It currently moves there." The editors are adopted as children of
/// the main window (`adopt_floating`), and somewhere between a Space switch
/// and the return AppKit puts a child down somewhere nobody put it. Which
/// call does it could not be watched from here, and a rule that names the
/// call would be wrong on the next macOS anyway. So the rule is about the
/// window instead: a child's frame is remembered, and a frame that changes
/// while no mouse button is down, the parent has not moved and the window is
/// not in a live resize is a move nobody asked for — and it is put back.
/// The window's own resize is not a move: only the origin is guarded, and
/// only while the size stands.
///
/// Bounded by [`MAX_REPAIRS`], read back through [`REPAIRS`], and keyed by
/// bare address the way `adopt_floating` keys its edge — an address reused
/// by a new window is one wrong anchor for one frame.
fn guard_child_frames(main: &NSWindow) {
    use objc2_foundation::NSRect;
    use std::cell::{Cell, RefCell};
    use std::collections::HashMap;
    if !adoption_armed() {
        return;
    }
    thread_local! {
        static LAST_MAIN: Cell<Option<NSRect>> = const { Cell::new(None) };
        static ANCHORS: RefCell<HashMap<usize, (NSRect, u8)>> = RefCell::new(HashMap::new());
    }
    let main_now = main.frame();
    let main_moved = LAST_MAIN.with(|m| {
        let moved = m.get().is_some_and(|prev| !same_rect(prev, main_now));
        m.set(Some(main_now));
        moved
    });
    let hand = NSEvent::pressedMouseButtons() != 0;
    let Some(children) = main.childWindows() else {
        ANCHORS.with(|a| a.borrow_mut().clear());
        return;
    };
    let mut seen: Vec<usize> = Vec::new();
    for i in 0..children.count() {
        let window = children.objectAtIndex(i);
        let addr = objc2::rc::Retained::as_ptr(&window) as usize;
        seen.push(addr);
        let now = window.frame();
        let busy = hand || main_moved || window.inLiveResize();
        ANCHORS.with(|a| {
            let mut a = a.borrow_mut();
            match a.get_mut(&addr) {
                None => {
                    a.insert(addr, (now, 0));
                }
                Some((anchor, repairs)) => {
                    if busy {
                        *anchor = now;
                        *repairs = 0;
                    } else if unrequested_move(*anchor, now) {
                        if *repairs < MAX_REPAIRS {
                            window.setFrameOrigin(anchor.origin);
                            *repairs += 1;
                            REPAIRS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        } else {
                            *anchor = now;
                            *repairs = 0;
                        }
                    } else {
                        *anchor = now;
                    }
                }
            }
        });
    }
    ANCHORS.with(|a| a.borrow_mut().retain(|k, _| seen.contains(k)));
}

fn same_rect(a: objc2_foundation::NSRect, b: objc2_foundation::NSRect) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 0.5;
    close(a.origin.x, b.origin.x)
        && close(a.origin.y, b.origin.y)
        && close(a.size.width, b.size.width)
        && close(a.size.height, b.size.height)
}

/// A frame that moved without resizing: the one shape a Space switch
/// leaves and a hand does not, since a hand is a pressed button and a resize
/// changes the size.
fn unrequested_move(anchor: objc2_foundation::NSRect, now: objc2_foundation::NSRect) -> bool {
    let close = |x: f64, y: f64| (x - y).abs() < 0.5;
    close(anchor.size.width, now.size.width)
        && close(anchor.size.height, now.size.height)
        && !(close(anchor.origin.x, now.origin.x) && close(anchor.origin.y, now.origin.y))
}

#[cfg(test)]
mod frame_guard_tests {
    use super::*;
    use objc2_foundation::{NSPoint, NSRect, NSSize};

    fn rect(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(NSPoint::new(x, y), NSSize::new(w, h))
    }

    /// The rule as a table: a jump is a move nobody asked for, a resize is
    /// not, a sub-pixel wobble is not, and the same frame is not.
    #[test]
    fn a_jump_without_a_resize_is_a_move_nobody_asked_for() {
        let home = rect(100.0, 200.0, 640.0, 480.0);
        assert!(unrequested_move(home, rect(1800.0, 0.0, 640.0, 480.0)));
        assert!(!unrequested_move(home, rect(100.0, 200.0, 800.0, 480.0)), "a resize is not a move");
        assert!(!unrequested_move(home, rect(90.0, 200.0, 800.0, 600.0)), "a resize from the top-left is not a move");
        assert!(!unrequested_move(home, rect(100.3, 199.8, 640.0, 480.0)), "a wobble under a pixel is not a move");
        assert!(!unrequested_move(home, home));
        assert!(same_rect(home, rect(100.2, 200.0, 640.1, 480.0)));
    }
}

#[cfg(test)]
mod adopt_tests {
    use super::{verdict, Parentage, Verdict, WindowFacts};
    use objc2_app_kit::{
        NSFloatingWindowLevel, NSModalPanelWindowLevel, NSNormalWindowLevel, NSWindowLevel,
    };

    /// An ordinary visible window of this app at `level`, owned by nobody
    /// and not holding the keyboard.
    fn at(level: NSWindowLevel) -> WindowFacts {
        WindowFacts {
            level,
            parent: Parentage::Orphan,
            sheet: false,
            is_main: false,
            visible: true,
            key_arrived: false,
            topmost_child: false,
        }
    }

    /// The case the whole filter exists for: a face that asked for
    /// `AlwaysOnTop`, which winit spelt as the floating level.
    #[test]
    fn a_floating_face_is_adopted() {
        assert_eq!(verdict(at(NSFloatingWindowLevel)), Verdict::Adopt);
    }

    /// **The file panel, which the owner explicitly keeps.** `rfd` shows it
    /// with no parent to attach to, so it is a plain modeless window and the
    /// LEVEL is the only thing that excludes it — measured at
    /// `NSNormalWindowLevel`, not the modal panel level it looks like it
    /// should be at. Widen the level test and this is what breaks.
    #[test]
    fn the_native_file_panel_is_left_alone() {
        assert_eq!(verdict(at(NSNormalWindowLevel)), Verdict::Leave);
        assert_eq!(verdict(at(NSModalPanelWindowLevel)), Verdict::Leave);
    }

    /// A window cannot be its own child, and the main window is the parent.
    #[test]
    fn the_main_window_is_never_adopted_into_itself() {
        for level in [NSNormalWindowLevel, NSFloatingWindowLevel] {
            let main = WindowFacts { is_main: true, ..at(level) };
            assert_eq!(verdict(main), Verdict::Leave, "level {level}");
        }
    }

    /// A sheet is bound to its window by a stronger relationship than this
    /// one and AppKit orders it itself.
    #[test]
    fn a_sheet_keeps_its_own_owner() {
        let sheet = WindowFacts { sheet: true, ..at(NSFloatingWindowLevel) };
        assert_eq!(verdict(sheet), Verdict::Leave);
    }

    /// Somebody else's arrangement stays somebody else's.
    #[test]
    fn another_windows_child_is_not_re_parented() {
        let theirs = WindowFacts { parent: Parentage::Theirs, ..at(NSFloatingWindowLevel) };
        assert_eq!(verdict(theirs), Verdict::Leave);
    }

    /// The idempotence that lets this run every frame: once a window is ours
    /// and down at the normal level there is nothing left to do to it.
    #[test]
    fn an_adopted_window_already_put_down_is_left_alone() {
        let settled = WindowFacts { parent: Parentage::Ours, ..at(NSNormalWindowLevel) };
        assert_eq!(verdict(settled), Verdict::Leave);
    }

    /// A surface `shell.rs` has promoted back up a rung — the export window
    /// when the modal question above it closes, or any surface the app has
    /// asked to `raise`. The level command is the only signal that it
    /// happened, and it has to become a re-seat.
    #[test]
    fn an_adopted_window_floated_again_is_reseated() {
        let promoted = WindowFacts { parent: Parentage::Ours, ..at(NSFloatingWindowLevel) };
        assert_eq!(verdict(promoted), Verdict::Reseat);
    }

    /// **A click that AppKit will not act on.** Once two windows are children
    /// of one parent their order is frozen at the order they were adopted in,
    /// so clicking the back one makes it key and leaves it behind. Putting it
    /// back is the only reason this predicate looks at key status at all.
    #[test]
    fn a_clicked_child_that_is_still_behind_is_reseated() {
        let clicked = WindowFacts {
            parent: Parentage::Ours,
            key_arrived: true,
            topmost_child: false,
            ..at(NSNormalWindowLevel)
        };
        assert_eq!(verdict(clicked), Verdict::Reseat);
    }

    /// And the same child once it is where it belongs — otherwise this would
    /// tear the child list apart and rebuild it on every single frame the
    /// user had a face focused.
    #[test]
    fn a_clicked_child_already_in_front_is_left_alone() {
        let settled = WindowFacts {
            parent: Parentage::Ours,
            key_arrived: true,
            topmost_child: true,
            ..at(NSNormalWindowLevel)
        };
        assert_eq!(verdict(settled), Verdict::Leave);
    }

    /// The key window that is NOT one of ours — the file panel, most of all.
    /// Key status alone must never be enough to touch a window.
    #[test]
    fn a_key_window_we_do_not_own_is_left_alone() {
        let panel = WindowFacts { key_arrived: true, ..at(NSNormalWindowLevel) };
        assert_eq!(verdict(panel), Verdict::Leave);
        let main = WindowFacts { key_arrived: true, is_main: true, ..at(NSNormalWindowLevel) };
        assert_eq!(verdict(main), Verdict::Leave);
    }

    /// Born hidden, revealed a pass later. Adopting early would order it in
    /// and put the undrawn frame on the screen.
    #[test]
    fn a_window_that_has_not_been_revealed_yet_waits() {
        let unborn = WindowFacts { visible: false, ..at(NSFloatingWindowLevel) };
        assert_eq!(verdict(unborn), Verdict::Leave);
    }

    /// winit puts a window at `CGShieldingWindowLevel() + 1` for exclusive
    /// fullscreen. Far above floating, and not ours to argue with.
    #[test]
    fn a_fullscreen_shielding_level_is_not_floating() {
        assert_eq!(verdict(at(NSWindowLevel::MAX)), Verdict::Leave);
    }

    /// **A child AppKit has ordered out is left where AppKit put it.**
    ///
    /// This is the `visible` test doing a second job, and it is worth a name
    /// of its own because the case is not the one the test above covers. That
    /// one is a window not yet born; this one is a face that has been open for
    /// half an hour and has gone off screen WITH its parent — the app hidden,
    /// or the user away on another Space. `addChildWindow:` orders a hidden
    /// child in, measured, so a sweep that touched one now would drag it back
    /// onto a screen its parent is not on.
    ///
    /// It matters more than it did, because AppKit is now the ONLY thing
    /// positioning these windows: `dialogs::held_geometry` stopped the app
    /// re-asserting an absolute position on every frame, and a face's place
    /// is whatever the parent-relative arrangement says it is. Two things
    /// moving one window was the whole disease.
    #[test]
    fn a_child_ordered_out_with_its_parent_is_left_alone() {
        for level in [NSNormalWindowLevel, NSFloatingWindowLevel] {
            let away = WindowFacts {
                parent: Parentage::Ours,
                visible: false,
                ..at(level)
            };
            assert_eq!(verdict(away), Verdict::Leave, "level {level}");
            // And the same window on the frame the app comes back, before
            // AppKit has finished ordering things in: a key arrival is not a
            // reason to touch a window that is not on the screen yet.
            let arriving = WindowFacts { key_arrived: true, ..away };
            assert_eq!(verdict(arriving), Verdict::Leave, "level {level}, arriving");
        }
    }

    /// **The tab-away-and-back sequence, as the predicate sees it.**
    ///
    /// The owner's report was a face that walked to the bottom-right corner
    /// on exactly this sequence. The corner was the app's own doing and is
    /// fixed in `dialogs.rs`; what this pins is the other half of the story —
    /// that the sweep does NOTHING across the whole sequence, so a face's
    /// position is AppKit's parent-relative arrangement from the moment it is
    /// adopted until it closes.
    ///
    /// A frame in the middle of this that came back `Adopt` or `Reseat` would
    /// be `removeChildWindow:` and `addChildWindow:` on a settled window, and
    /// re-adoption is the one operation in this module that can reorder a
    /// window a user has arranged. This is the numbered by-hand checklist
    /// written down as facts.
    #[test]
    fn a_face_is_not_touched_across_a_tab_away_and_back() {
        // Adopted and put down, sitting in front of the desk as the only
        // child, holding the keyboard.
        let settled = WindowFacts {
            parent: Parentage::Ours,
            topmost_child: true,
            ..at(NSNormalWindowLevel)
        };
        let sequence = [
            ("open and settled", settled),
            // Tab away: the app resigns active and `keyWindow` goes nil, so
            // nothing arrives. The window is still on screen behind the
            // other program.
            ("tabbed away", settled),
            // A Space change or Cmd-H orders parent and child out together.
            ("ordered out with the parent", WindowFacts { visible: false, ..settled }),
            // Back: AppKit orders them in and hands the keyboard to whichever
            // window had it, which is this one — and it is already the front
            // child, so there is nothing to bring forward.
            ("back, and key again", WindowFacts { key_arrived: true, ..settled }),
            ("settled again", settled),
        ];
        for (when, facts) in sequence {
            assert_eq!(verdict(facts), Verdict::Leave, "{when}");
        }
    }
}

// ───────────────────────────────────────────────────────────────────────────
// Borderless + maximised
// ───────────────────────────────────────────────────────────────────────────

/// **Maximised, borderless, and covering the whole screen.**
///
/// "Maximised" on every platform means *the work area* — the screen minus
/// whatever the shell reserves: the taskbar on Windows, the panel on Linux,
/// the menu bar and the Dock here. That is right for a window with a title
/// bar to grab, and it is wrong for a borderless one: taking the frame off
/// was the request to use the whole display, and the app kept a strip of
/// empty desktop along two edges that nothing could be dragged into.
///
/// The owner: "when window state is maximized ... and borderless is turned
/// on, the window scales up to cover the empty space created by the taskbar,
/// and that nothing is cutoff anywhere."
///
/// **Two things, and both are needed.** Setting the frame to the screen's
/// full rectangle is not enough on macOS — the menu bar is drawn ABOVE every
/// ordinary window, so the top of the layout would be behind it: covered, not
/// cut off, but indistinguishable to the eye and worse than not growing at
/// all. So the presentation options auto-hide the menu bar and the Dock for
/// as long as the window is in this state, which is what a borderless
/// full-screen game does and what makes the frame honest.
///
/// Reversed by [`uncover_screen`] the moment either condition stops holding.
/// Both are idempotent and both are driven by a flag on the caller, so
/// neither runs per frame: `setFrame` during AppKit's own zoom animation
/// fights it.
///
/// Returns whether the window was found and moved.
pub fn cover_screen(frame: &eframe::Frame) -> bool {
    let Some(window) = resolve(frame) else {
        return false;
    };
    let Some(screen) = window.screen() else {
        return false;
    };
    // The FULL frame, not `visibleFrame`: the latter is the work area, which
    // is exactly the rectangle being escaped.
    let full = screen.frame();
    // SAFETY: `sharedApplication` on the main thread, which is the only
    // thread eframe draws from and therefore the only one that reaches here.
    unsafe {
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            app.setPresentationOptions(
                objc2_app_kit::NSApplicationPresentationOptions::AutoHideDock
                    | objc2_app_kit::NSApplicationPresentationOptions::AutoHideMenuBar,
            );
        }
        window.setFrame_display(full, true);
    }
    true
}

/// Give the menu bar and the Dock back, and let the window be an ordinary
/// maximised one again.
///
/// The frame is NOT put back here on purpose: leaving the state means either
/// the window is no longer maximised — in which case AppKit has already
/// restored the size it remembers — or borderless has just been switched
/// off, and the size correction that follows a decoration change is already
/// on its way from the app's own resize pass. Two things moving one window
/// is how a window ends up flickering between two sizes.
pub fn uncover_screen() {
    // SAFETY: as `cover_screen`; main thread only.
    unsafe {
        if let Some(mtm) = MainThreadMarker::new() {
            let app = NSApplication::sharedApplication(mtm);
            app.setPresentationOptions(
                objc2_app_kit::NSApplicationPresentationOptions::Default,
            );
        }
    }
}

/// **The content rectangle, in points** — what the app actually gets to draw
/// in, with any title bar already subtracted.
pub fn content_size(frame: &eframe::Frame) -> Option<(f64, f64)> {
    let window = resolve(frame)?;
    let content = window.contentRectForFrameRect(window.frame());
    Some((content.size.width, content.size.height))
}

/// **Keep the content the size it was, across a decoration change.**
///
/// Adding or removing a title bar changes what a window CONTAINS without
/// changing what it occupies: AppKit keeps the frame and moves the boundary,
/// so the drawable area grows or shrinks by about 28 points. On a window the
/// app is free to resize that is invisible, because the aspect snap puts it
/// straight back. Under a tiling manager — AeroSpace here — the app is not
/// free to resize anything, the snap is latched off on purpose, and the
/// mismatch simply stays: a strip of window the layout does not cover on the
/// way out of borderless, and a title bar sitting over the top of the picture
/// on the way back in. The owner: "toggling on and off borderless adds black
/// strip and when border is readded, it's drawn over."
///
/// Setting the FRAME from the content rectangle is the fix, and it is the
/// opposite of what the app was doing: it asks for the frame that gives back
/// the content size that was already there, so the drawable area is invariant
/// and only the window's outside edge moves. A tiling manager may then re-tile
/// it, which is its right and its business — the app is no longer arguing
/// about the size, only about how much of it is drawable.
pub fn keep_content_size(frame: &eframe::Frame, w: f64, h: f64) -> bool {
    let Some(window) = resolve(frame) else {
        return false;
    };
    if !(w.is_finite() && h.is_finite() && w > 1.0 && h > 1.0) {
        return false;
    }
    let now = window.frame();
    let content = window.contentRectForFrameRect(now);
    // Already right — do not touch it. A `setFrame` during a tiler's own
    // placement is the fight this whole module exists to avoid.
    if (content.size.width - w).abs() < 0.5 && (content.size.height - h).abs() < 0.5 {
        return false;
    }
    // The window's TOP edge stays put, which is what a title bar appearing or
    // disappearing looks like to a person: the picture does not jump.
    let top = now.origin.y + now.size.height;
    let wanted_content = NSRect::new(
        NSPoint::new(content.origin.x, content.origin.y),
        NSSize::new(w, h),
    );
    let mut want = window.frameRectForContentRect(wanted_content);
    want.origin.y = top - want.size.height;
    // SAFETY: main thread, as everything else in this module; `want` is a
    // finite rect derived from the window's own geometry.
    unsafe { window.setFrame_display(want, true) };
    true
}

// ───────────────────────────────────────────────────────────────────────────
// The save panel
// ───────────────────────────────────────────────────────────────────────────

/// **What is about to be written under the typed name**, so the panel can
/// refuse a name that is already taken. The standard save panel's "Replace?"
/// was this question; the answer here is never yes, because nothing this
/// app saves through a panel is something it should write over blind — a
/// project folder holds takes, a kit holds samples.
#[derive(Debug, Clone, Copy)]
pub enum Taken {
    /// A FOLDER named for the typed name — a project. `strip` is the list
    /// of extensions a hand may have typed out of habit and the caller will
    /// remove (`.tangent`), so `Bossa.tangent` and `Bossa` collide alike.
    Folder {
        noun: &'static str,
        strip: &'static [&'static str],
    },
    /// A file `<name>.<ext>`, the extension appended by the caller when the
    /// hand did not type it.
    File { ext: &'static str, noun: &'static str },
    /// Nothing to refuse: the caller honours the name alone and overwrites
    /// on purpose (a template).
    Never,
}

impl Taken {
    /// Why `dir/<name>` cannot be written, said to the user; `None` when it
    /// can. `name` is already [`clean`].
    fn problem(self, dir: &std::path::Path, name: &str) -> Option<String> {
        match self {
            Taken::Never => None,
            Taken::Folder { noun, strip } => {
                let stem = strip
                    .iter()
                    .find_map(|ext| {
                        name.strip_suffix(&format!(".{ext}")).filter(|s| !s.is_empty())
                    })
                    .unwrap_or(name);
                dir.join(stem).exists().then(|| {
                    format!(
                        "A {noun} called \u{201c}{stem}\u{201d} is already here. \
                         Choose another name, or another folder."
                    )
                })
            }
            Taken::File { ext, noun } => {
                let file = if std::path::Path::new(name)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case(ext))
                {
                    name.to_owned()
                } else {
                    format!("{name}.{ext}")
                };
                dir.join(&file).exists().then(|| {
                    format!(
                        "A {noun} called \u{201c}{file}\u{201d} is already here. \
                         Choose another name, or another folder."
                    )
                })
            }
        }
    }
}

/// A typed name as a file name: no control characters, no path separators,
/// no leading or trailing space — the same cleaning `rename_project` gives
/// the Project window's field, so a name is one thing everywhere.
fn clean(typed: &str) -> String {
    typed
        .chars()
        .filter(|c| !c.is_control() && *c != '/' && *c != ':')
        .collect::<String>()
        .trim()
        .to_owned()
}

/// What a save panel is asked for. See [`save_panel`].
pub struct SaveAsk<'a> {
    /// The line above the browser: "Save the project", "Save Kit".
    pub title: &'a str,
    /// What the name field opens with.
    pub suggested: &'a str,
    /// The folder the panel opens on. `panel_start` makes it exist.
    pub start: &'a std::path::Path,
    /// What the caller will write under the name, for the refusal.
    pub taken: Taken,
}

struct CheckIvars {
    field: objc2::rc::Retained<NSTextField>,
    taken: Taken,
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - The class does not implement `Drop`.
    #[unsafe(super(NSObject))]
    // Main-thread-only because `NSOpenSavePanelDelegate` requires it, and
    // the panel only ever calls it from the thread it runs on.
    #[thread_kind = MainThreadOnly]
    // No fixed `#[name]`, for the reason tangent-record's camera delegate
    // gives: this crate can be loaded twice into one process.
    #[ivars = CheckIvars]
    struct SaveCheck;

    unsafe impl NSObjectProtocol for SaveCheck {}

    unsafe impl NSOpenSavePanelDelegate for SaveCheck {
        // The raw shape of an `error:` method — `BOOL` back, `NSError **`
        // out — because `define_class!` in this objc2 has no `error:_`
        // sugar for a protocol method the way `extern_methods!` does.
        #[unsafe(method(panel:validateURL:error:))]
        fn validate(
            &self,
            _sender: &AnyObject,
            url: &NSURL,
            error: *mut *mut NSError,
        ) -> Bool {
            match self.refusal(url) {
                None => Bool::YES,
                Some(text) => {
                    if !error.is_null() {
                        // Autoreleased, as the convention wants: the caller
                        // owns nothing it did not retain.
                        let err = refusal(&text);
                        // SAFETY: `error` is the out-pointer AppKit hands a
                        // validate method, checked non-null above.
                        unsafe { *error = objc2::rc::Retained::autorelease_return(err) };
                    }
                    Bool::NO
                }
            }
        }
    }
);

impl SaveCheck {
    fn new(mtm: MainThreadMarker, ivars: CheckIvars) -> objc2::rc::Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        // SAFETY: `init` on a freshly allocated NSObject subclass whose ivars
        // have been set, which is exactly the initialiser contract.
        unsafe { msg_send![super(this), init] }
    }

    /// Why the Save button must not go through, or `None`.
    fn refusal(&self, folder: &NSURL) -> Option<String> {
        let iv = self.ivars();
        let name = clean(&iv.field.stringValue().to_string());
        if name.is_empty() {
            return Some("Type a name to save under.".to_owned());
        }
        let dir = std::path::PathBuf::from(folder.path()?.to_string());
        iv.taken.problem(&dir, &name)
    }
}

/// An `NSError` the panel shows as a sheet and stays open behind.
fn refusal(text: &str) -> objc2::rc::Retained<NSError> {
    let message = NSString::from_str(text);
    let value: &AnyObject = &message;
    // SAFETY: a `static` from AppKit, read only.
    let key: &NSString = unsafe { NSLocalizedDescriptionKey };
    let info = NSDictionary::<NSString, AnyObject>::from_slices(&[key], &[value]);
    // SAFETY: the dictionary is keyed by the string constants the method
    // documents, and the domain is this app's own.
    unsafe {
        NSError::errorWithDomain_code_userInfo(
            &NSString::from_str("org.codeberg.ganten1998.tangent"),
            1,
            Some(&info),
        )
    }
}

/// **Where to save, asked with an OPEN panel.** `Some(folder/name)` when the
/// hand pressed Save; `None` on Cancel.
///
/// **Why not `NSSavePanel`.** On the owner's machine the save panel refuses
/// any folder under `~/Documents` — the app's whole media root — and opens
/// at home instead, silently. Measured on 2026-09-05 with the programs in
/// `tools/panelprobe`, photographing the panel rather than trusting its
/// `directoryURL`, which reads back whatever was last set while the panel
/// shows something else:
///
/// * `~/Documents`, `~/Desktop`, `~/Music`, a folder under Desktop, under
///   Downloads, under Music: all shown as asked;
/// * a folder under `~/Documents`, new or old, with the folder at 700 or
///   755, given as a path, a `..` path, a firmlink path, a file-reference
///   URL or a symlink from elsewhere: home, every time, with the delegate
///   reporting the move at 280 ms;
/// * saying the directory again — 30 ms on, 1 s on, 2 s on — changes the
///   property and not the panel;
/// * and `NSOpenPanel` in choose-a-folder mode, on the very same folder,
///   opens IN it, listing the projects.
///
/// Neither TCC (the app holds the Documents grant, requirement matching)
/// nor iCloud (Desktop & Documents is not relocating the folder) explains
/// it, and nothing in the unified log names it. The open panels this app
/// already raises — Open Project, Load Kit — have never misbehaved, which is
/// the owner's own report. So the save panel is one of those: a folder
/// chooser with a "Save As" field in its accessory view, "Save" on the
/// button, and a delegate that refuses an empty name or one that is taken
/// (the standard panel's "Replace?", answered no — see [`Taken`]). The
/// answer is `<chosen folder>/<typed name>`, the shape the callers already
/// expect from `rfd`'s save panel, extension handling included.
///
/// Run as a sheet on the main window and then `runModal`, exactly the pair
/// `rfd` uses for the pickers that work.
pub fn save_panel(frame: &eframe::Frame, ask: &SaveAsk<'_>) -> Option<std::path::PathBuf> {
    let mtm = MainThreadMarker::new()?;
    let parent = resolve(frame);
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseDirectories(true);
    panel.setCanChooseFiles(false);
    panel.setCanCreateDirectories(true);
    panel.setAllowsMultipleSelection(false);
    panel.setPrompt(Some(&NSString::from_str("Save")));
    panel.setMessage(Some(&NSString::from_str(ask.title)));
    if let Some(start) = ask.start.to_str() {
        let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(start), true);
        panel.setDirectoryURL(Some(&url));
    }
    // The name field, in the accessory view, disclosed from the start: an
    // open panel has no name field of its own.
    let accessory = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(480.0, 40.0)),
    );
    let label = NSTextField::labelWithString(&NSString::from_str("Save As:"), mtm);
    label.setFrameOrigin(NSPoint::new(10.0, 10.0));
    label.setFrameSize(NSSize::new(70.0, 22.0));
    let field = NSTextField::initWithFrame(
        NSTextField::alloc(mtm),
        NSRect::new(NSPoint::new(85.0, 8.0), NSSize::new(380.0, 24.0)),
    );
    field.setStringValue(&NSString::from_str(ask.suggested));
    field.setEditable(true);
    field.setBezeled(true);
    accessory.addSubview(&label);
    accessory.addSubview(&field);
    panel.setAccessoryView(Some(&accessory));
    panel.setAccessoryViewDisclosed(true);
    let check = SaveCheck::new(
        mtm,
        CheckIvars {
            field: field.clone(),
            taken: ask.taken,
        },
    );
    // SAFETY: a delegate of the protocol the panel asks for, kept alive
    // (`check`) for as long as the panel runs, and taken off again below.
    unsafe { panel.setDelegate(Some(ProtocolObject::from_ref(&*check))) };
    let response: NSModalResponse = match parent {
        Some(window) => {
            let done = block2::StackBlock::new(|_: NSModalResponse| {});
            panel.beginSheetModalForWindow_completionHandler(&window, &done);
            // The name selected for typing over, as the save panel does.
            let _ = panel.makeFirstResponder(Some(&field));
            panel.runModal()
        }
        None => panel.runModal(),
    };
    // SAFETY: as above; the panel outlives nothing here, but a weak
    // delegate is still tidier taken off than left dangling.
    unsafe { panel.setDelegate(None) };
    if response != NSModalResponseOK {
        return None;
    }
    let folder = std::path::PathBuf::from(panel.URL()?.path()?.to_string());
    let name = clean(&field.stringValue().to_string());
    if name.is_empty() {
        return None;
    }
    Some(folder.join(name))
}

#[cfg(test)]
mod save_panel_tests {
    use super::{clean, Taken};

    fn scratch(tag: &str) -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "tangent-save-panel-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("scratch");
        root
    }

    /// **A name that is taken is refused with the reason, and one that is
    /// free goes through** — for a project folder, with or without the
    /// extension a hand types out of habit, and for a file with its
    /// extension appended or already there.
    #[test]
    fn a_taken_name_is_refused_and_a_free_one_is_not() {
        let root = scratch("taken");
        std::fs::create_dir_all(root.join("Bossa")).expect("a project");
        std::fs::write(root.join("Room.kit"), b"kit").expect("a kit");
        let project = Taken::Folder {
            noun: "project",
            strip: &["tangent", "tan"],
        };
        assert!(project.problem(&root, "Bossa").is_some_and(|m| m.contains("Bossa")));
        assert!(project.problem(&root, "Bossa.tangent").is_some(), "the typed extension hid the folder");
        assert!(project.problem(&root, "Nocturne").is_none());
        let kit = Taken::File {
            ext: "kit",
            noun: "kit",
        };
        assert!(kit.problem(&root, "Room").is_some_and(|m| m.contains("Room.kit")));
        assert!(kit.problem(&root, "Room.kit").is_some());
        assert!(kit.problem(&root, "room.KIT").is_some(), "the extension is not case-blind");
        assert!(kit.problem(&root, "Hall").is_none());
        assert!(Taken::Never.problem(&root, "Bossa").is_none());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_typed_name_is_cleaned_the_way_the_project_window_cleans_one() {
        assert_eq!(clean("  My Song  "), "My Song");
        assert_eq!(clean("a/b:c\u{7}"), "abc");
        assert_eq!(clean("   "), "");
    }
}

/// **Tell macOS this process is doing work that cannot be scheduled late.**
///
/// Nothing in this app has ever said so. With no declared activity the system
/// is free to apply App Nap to it — priority reduction, TIMER THROTTLING and
/// I/O throttling, per Apple's own Energy Efficiency Guide — and to coalesce
/// its timers with everybody else's. The CoreAudio IO thread is real-time and
/// safe from that, but nothing else here is: the take writer and the capture
/// ring drain both poll on a four-millisecond loop (`record::POLL`) at default
/// QoS, and a coalesced four-millisecond timer is a ring that overruns.
///
/// `NSActivityUserInitiatedAllowingIdleSystemSleep | NSActivityLatencyCritical`
/// and deliberately not plain `NSActivityUserInitiated`: the plain one carries
/// `NSActivityIdleSystemSleepDisabled`, which would stop the Mac idle-sleeping
/// for as long as Tangent is open. That is a visible change to somebody's
/// machine and it is not what is being asked for here — what is being asked
/// for is to not be napped and not be coalesced.
///
/// **OFF unless `TANGENT_ACTIVITY=1`.** This ships as an experiment, not a
/// behaviour change: with it unset the process behaves exactly as every build
/// before it, on every macOS version, so the build carries it without
/// carrying its risk. Run it from a terminal to try it:
///
/// ```text
/// TANGENT_ACTIVITY=1 /Applications/Tangent.app/Contents/MacOS/tangent
/// ```
///
/// The token is leaked on purpose: an activity ends when its token is
/// released, and this one is meant to last as long as the process.
pub fn hold_latency_critical() {
    if std::env::var("TANGENT_ACTIVITY").as_deref() != Ok("1") {
        return;
    }
    use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
    // `NSActivityUserInitiated` is `0x00FFFFFF | NSActivityIdleSystemSleepDisabled`,
    // and the "allowing idle system sleep" spelling is that with the sleep bit
    // masked back out — so it is exactly the low three bytes. Written as the
    // literal the headers define rather than assembled from the flag set,
    // because the flag this must NOT carry is the one it would be assembled
    // from.
    const USER_INITIATED_ALLOWING_IDLE_SLEEP: u64 = 0x00FF_FFFF;
    const LATENCY_CRITICAL: u64 = 0xFF00_0000_00;
    let options =
        NSActivityOptions(USER_INITIATED_ALLOWING_IDLE_SLEEP | LATENCY_CRITICAL);
    let reason = NSString::from_str("Tangent is running an audio engine");
    let token = unsafe {
        NSProcessInfo::processInfo().beginActivityWithOptions_reason(options, &reason)
    };
    std::mem::forget(token);
    log::info!(
        "TANGENT_ACTIVITY=1: told macOS this process is latency-critical \
         (App Nap and timer coalescing off; idle sleep still allowed)"
    );
}
