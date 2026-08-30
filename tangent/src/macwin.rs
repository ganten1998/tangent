//! **Two AppKit facts winit does not surface, and one hook it does not install.**
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

use objc2::runtime::{AnyClass, AnyObject, Bool, Sel};
use objc2::{sel, ClassType};
use objc2_app_kit::{NSEvent, NSView, NSWindow};
use objc2_foundation::NSSize;
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
/// GUI crate: it runs while AppKit owns the thread, and `IvoryApp` is behind a
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
        if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
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
        if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
            let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
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
    let wanted_content = objc2_foundation::NSRect::new(
        objc2_foundation::NSPoint::new(content.origin.x, content.origin.y),
        objc2_foundation::NSSize::new(w, h),
    );
    let mut want = window.frameRectForContentRect(wanted_content);
    want.origin.y = top - want.size.height;
    // SAFETY: main thread, as everything else in this module; `want` is a
    // finite rect derived from the window's own geometry.
    unsafe { window.setFrame_display(want, true) };
    true
}
