//! **What the OS keeps of the screen, and therefore what is left for a window.**
//!
//! Every band's height in this app is a fixed fraction of the window's WIDTH,
//! so the layout is a one-parameter family: choose a width and the height is
//! decided for you. Nothing checked that decided height against the display
//! until a tester opened Tangent on a laptop and found the bottom of the
//! window under the taskbar, with no drag that could bring it back — the
//! window was exactly as tall as the layout said it must be.
//!
//! Fixing that needs one number the window system does not hand out: how much
//! of the screen's height a window may actually occupy once the OS has taken
//! its taskbar, its Dock and its menu bar. `egui`'s `monitor_size` is the RAW
//! display, taskbar included.
//!
//! **A fraction rather than a height, which is what makes it DPI-proof.** The
//! OS answers these questions in physical pixels; egui lays out in logical
//! points; the ratio between them is a scale factor this module would
//! otherwise have to ask for and get right. A ratio of two physical numbers
//! has the scale factor top and bottom, so it cancels — and the caller
//! multiplies it by the logical height egui already knows. There is no scale
//! factor anywhere in this file, deliberately.
//!
//! Unknown is `None`, never a guess: the caller leaves the window alone rather
//! than resizing it by a number nobody measured.

/// The share of the primary display's height a window may occupy, 0..=1.
///
/// `None` on any platform or in any situation where the honest answer is not
/// available — the caller must treat that as "do not touch the size".
#[cfg(windows)]
pub fn usable_height_fraction() -> Option<f32> {
    use windows_sys::Win32::Foundation::RECT;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetSystemMetrics, SystemParametersInfoW, SM_CYSCREEN, SPI_GETWORKAREA,
    };
    // SAFETY: `SystemParametersInfoW` with `SPI_GETWORKAREA` writes one `RECT`
    // through the pointer it is given and reads nothing else; the RECT is
    // fully initialised by the call and only read when it reports success.
    unsafe {
        let mut rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        };
        let ok = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            std::ptr::addr_of_mut!(rect).cast(),
            0,
        );
        if ok == 0 {
            return None;
        }
        let work = (rect.bottom - rect.top) as f32;
        let screen = GetSystemMetrics(SM_CYSCREEN) as f32;
        fraction(work, screen)
    }
}

/// macOS keeps the menu bar always and the Dock unless it is hidden;
/// `visibleFrame` is AppKit's own answer to the same question.
#[cfg(target_os = "macos")]
pub fn usable_height_fraction() -> Option<f32> {
    let mtm = objc2_foundation::MainThreadMarker::new()?;
    let screen = objc2_app_kit::NSScreen::mainScreen(mtm)?;
    let full = screen.frame().size.height as f32;
    let visible = screen.visibleFrame().size.height as f32;
    fraction(visible, full)
}

/// **X11 and Wayland: unknown, on purpose.**
///
/// There is a `_NET_WORKAREA` property, and it is a property of the ROOT
/// window of one X11 screen — meaningless under Wayland, wrong under most
/// multi-head setups, and absent under a tiling manager that reserves nothing.
/// A tiler also places windows itself, so a size chosen here is overruled a
/// frame later; that fight is one this app has already had once (see
/// `wm_overrode_size`). Returning `None` leaves the size alone, which is the
/// behaviour a tiling user wants anyway.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn usable_height_fraction() -> Option<f32> {
    None
}

/// The shared sanity check: a share of the screen, or nothing.
///
/// Clamped at both ends. A fraction over 1 means the work area is somehow
/// taller than the screen it is on and the numbers are not to be trusted; a
/// tiny one would shrink the window to a slot, and no real desktop reserves
/// more than about a third of its height.
#[cfg(any(windows, target_os = "macos"))]
fn fraction(usable: f32, whole: f32) -> Option<f32> {
    if !usable.is_finite() || !whole.is_finite() || usable <= 0.0 || whole <= 0.0 {
        return None;
    }
    let f = usable / whole;
    if !(0.5..=1.0).contains(&f) {
        return None;
    }
    Some(f)
}

#[cfg(test)]
mod tests {
    /// The guard is the point of the function, so it is what is tested: the
    /// platform call itself cannot be faked and is checked by running the app.
    #[cfg(any(windows, target_os = "macos"))]
    #[test]
    fn a_fraction_is_a_share_of_a_screen_or_it_is_nothing() {
        use super::fraction;
        // A taskbar's worth.
        assert_eq!(fraction(1040.0, 1080.0), Some(1040.0 / 1080.0));
        // No reservation at all is a legitimate answer.
        assert_eq!(fraction(1080.0, 1080.0), Some(1.0));
        // Nonsense in every direction is None, never a number.
        assert_eq!(fraction(1200.0, 1080.0), None, "taller than the screen");
        assert_eq!(fraction(200.0, 1080.0), None, "most of the screen gone");
        assert_eq!(fraction(0.0, 1080.0), None);
        assert_eq!(fraction(1040.0, 0.0), None);
        assert_eq!(fraction(f32::NAN, 1080.0), None);
        assert_eq!(fraction(1040.0, f32::INFINITY), None);
    }

    /// Every platform answers the question without panicking, whatever it
    /// answers. Cheap, and it is the only thing a test can assert about a
    /// call into the window server.
    #[test]
    fn asking_is_always_safe() {
        if let Some(f) = super::usable_height_fraction() {
            assert!((0.5..=1.0).contains(&f), "fraction out of band: {f}");
        }
    }
}
