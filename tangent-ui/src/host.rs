//! What the thing hosting us can actually do.
//!
//! The desktop binary owns a window, opens its own MIDI devices, raises native
//! panels and can spawn more windows. Not every app this crate builds may do
//! all of that: screen-reader mode and fullscreen give the child windows up, a
//! Minimal build links no capture code, and a test builds the app in a bare
//! egui context that owns nothing outside it ([`Caps::NONE`]).
//!
//! `Caps` is how the shared GUI asks, instead of assuming. It is deliberately
//! plain data rather than a trait: every field is a fact about the host that
//! the UI needs at a branch point, and a struct of bools can be built in a test
//! for a host that does not exist yet.
//!
//! The rule for adding a field: it must name a CAPABILITY, not a host. Code
//! that says `if caps.child_windows` still reads correctly whatever turned the
//! windows off; code that says `if is_desktop` has to be revisited every time.

/// What this host permits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Caps {
    /// The UI may open additional OS windows.
    ///
    /// Off in screen-reader mode and in fullscreen, where every menu and
    /// dialog is drawn in the canvas instead, and in [`Caps::NONE`].
    ///
    /// **False does not mean "the calls fail".** In a context that embeds its
    /// viewports, which a bare `egui::Context` does,
    /// `egui::Context::show_viewport_immediate` still RUNS: it invokes the
    /// closure inline and opens a second `CentralPanel` under the same id as
    /// the first, painting garbage over the piano. A hard error would have been
    /// kinder. Anything that would have been a window has to be drawn in the
    /// canvas instead, and this is the flag that decides.
    pub child_windows: bool,
    /// The UI may set its own size by sending viewport commands. This also
    /// gates the borderless toggle, the geometry write-back and the offscreen
    /// rescue: everything that treats the window as ours. Off only in
    /// [`Caps::NONE`], whose context has no window to send them to.
    pub window_sizing: bool,
    /// The UI chooses its own MIDI input. Off only in [`Caps::NONE`], which
    /// has no device list to offer.
    pub midi_ports: bool,
    /// Settings changes may be written to `~/.config/tangent/`. Off in every
    /// test that builds an app it must not let near that file, which is what
    /// [`Caps::NONE`] and the tests' own `QUIET` sets are for.
    pub persist_global_settings: bool,
    /// The UI may open a camera or an audio input.
    ///
    /// False in a Minimal build, where the code to do it is not linked at all,
    /// and in [`Caps::NONE`]. **The Recorder band's height must be exactly zero
    /// when this is false**: `fit_bands` shrinks the piano to make room for any
    /// band with a non-zero height, including one that will never be painted.
    pub capture_devices: bool,
    /// The UI may create take directories and write media outside the config
    /// directory.
    ///
    /// Deliberately separate from `persist_global_settings`, which is about one
    /// specific shared file. Conflating "shares settings.json" with "may write
    /// a 2 GB video into the user's Movies folder" is exactly the mistake this
    /// struct's own rule warns against.
    pub write_user_files: bool,
    /// A "Choose folder..." button is a meaningful thing to offer: this host
    /// may raise a native panel at all. Off only in [`Caps::NONE`], where there
    /// is no panel to raise and nothing would drain the request.
    pub native_file_dialogs: bool,
}

impl Caps {
    /// The standalone app: it can do everything.
    pub const DESKTOP: Caps = Caps {
        child_windows: true,
        window_sizing: true,
        midi_ports: true,
        persist_global_settings: true,
        capture_devices: true,
        write_user_files: true,
        native_file_dialogs: true,
    };

    /// **Nothing at all**: no window of its own, no viewport command, no MIDI
    /// port, no settings file, no capture device, no user file and no native
    /// panel.
    ///
    /// What the test harnesses build an app with, so a test reaches nothing
    /// outside its egui context. With `child_windows` off every menu and
    /// dialog draws inline in that context, as they do in screen-reader mode
    /// and in fullscreen, which is where a test can see them. No build
    /// constructs it.
    pub const NONE: Caps = Caps {
        child_windows: false,
        window_sizing: false,
        midi_ports: false,
        persist_global_settings: false,
        capture_devices: false,
        write_user_files: false,
        native_file_dialogs: false,
    };

    /// The standalone, built **without** the recorder (`--no-default-features`).
    ///
    /// Everything the chord display needs and nothing that opens a device or
    /// writes a file the user did not ask for. This is a real shipped variant,
    /// not a debug mode — see `docs/RECORDER-PLAN.md` §12a.
    ///
    /// The point of it is **not** binary size. Measured, the whole recorder and
    /// plugin-host dependency set is about 50 KB. The point is that a Full build
    /// must carry `com.apple.security.cs.disable-library-validation` and
    /// `allow-jit` in order to host plugins at all, plus camera and microphone
    /// usage strings — and someone who wants a chord display should not have to
    /// run a binary that can load arbitrary third-party code. A Minimal build
    /// ships **no entitlements**, exactly as 3.0.0 does.
    ///
    /// It differs from `DESKTOP` in exactly the two capabilities whose code is
    /// absent, and in nothing else: it still owns a real window, still opens its
    /// own MIDI device, still raises native dialogs.
    pub const MINIMAL: Caps = Caps {
        capture_devices: false,
        write_user_files: false,
        ..Caps::DESKTOP
    };
}

impl Default for Caps {
    /// Desktop, because that is the host every existing caller has and a
    /// default that silently disabled things would be a bug generator.
    fn default() -> Self {
        Self::DESKTOP
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **`NONE` withholds everything**, because the test harness leans on
    /// every part of that: no window to command, no device to open, no panel
    /// to raise, and above all no settings file to write. A field that went
    /// true here would be every test built on it reaching for that thing.
    #[test]
    fn none_withholds_every_capability() {
        assert_eq!(Caps::default(), Caps::DESKTOP);
        let n = Caps::NONE;
        assert!(!n.child_windows && !n.window_sizing, "a test owns no window");
        assert!(!n.midi_ports, "a test opens no MIDI device");
        assert!(!n.persist_global_settings, "a test writes no settings file");
        assert!(
            !n.capture_devices && !n.write_user_files,
            "a test opens no camera and writes no take"
        );
        assert!(!n.native_file_dialogs, "a test raises no native panel");
    }

    /// A Minimal build is the desktop MINUS the two things whose code is not
    /// linked, and nothing else. Stated as a test because the tempting mistake
    /// is to make Minimal a cut-down app — it is not; it is the same app without
    /// a recorder.
    #[test]
    fn minimal_gives_up_the_recorder_and_nothing_else() {
        let m = Caps::MINIMAL;
        assert!(!m.capture_devices && !m.write_user_files);
        assert!(
            m.child_windows && m.window_sizing && m.midi_ports,
            "Minimal still owns a real window and a real MIDI device"
        );
        assert!(
            m.persist_global_settings && m.native_file_dialogs,
            "nothing else is taken away"
        );
    }

    /// Writing a take is opening a device's worth of trust. A build that may
    /// capture but may not write has nowhere to put a recording; a build that
    /// may write but not capture has nothing to put there. Neither is a
    /// configuration anyone wants, and a `..Caps::DESKTOP` spread makes it easy
    /// to create one by editing a single field.
    #[test]
    fn capture_and_writing_files_travel_together() {
        for c in [Caps::DESKTOP, Caps::NONE, Caps::MINIMAL, Caps::default()] {
            assert_eq!(
                c.capture_devices, c.write_user_files,
                "{c:?} can do one half of recording and not the other"
            );
        }
    }
}

/// What the HOST can see about a gesture, which the toolkit will not say.
///
/// `egui::ViewportInfo` has no live-resize field, and winit 0.30 emits no
/// event for `windowWillStartLiveResize:`. Without this the app has to INFER
/// a drag from the size going still, which is what the old frame-counting
/// settle was — and inference cannot tell a hand on an edge from a tiling
/// window manager placing the window, because both look like "the size
/// changed and nobody here asked for it".
///
/// **All-false is the correct answer for every host that cannot say**:
/// Windows, Linux. Nothing here may be a platform type; the
/// firewall keeps this crate from seeing winit at all, so the desktop binary
/// fills this in and hands it across.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Gesture {
    /// A hand is on a resize edge of the MAIN window right now.
    pub live_resize: bool,
    /// A mouse button is down somewhere in the session. Covers a title-bar
    /// drag, where the geometry changes only after the window has moved.
    pub mouse_down: bool,
    /// **`live_resize` is a real answer, not a default.**
    ///
    /// This is what makes "a size change nobody asked for" mean "a window
    /// manager placed us". False on any host that cannot distinguish the two
    /// — and there, only a REFUSED size request may conclude anything, because
    /// on a host where `live_resize` is permanently false an ordinary edge
    /// drag is indistinguishable from a tiler and would otherwise disable the
    /// aspect snap for the whole session.
    pub resize_known: bool,
    /// **`mouse_down` is a real answer, not a default.** Appended at the END.
    ///
    /// Same bargain as [`Self::resize_known`], and needed for the same reason
    /// in the other direction: `mouse_down` is FALSE on every host that
    /// cannot say, so "the hand has let go" and "nobody can tell me" are the
    /// same bit. Anything that ENDS a gesture on it would therefore end every
    /// gesture immediately on Windows and on Linux. Only a host that sets
    /// this may be believed when it says the button is up.
    pub mouse_known: bool,
}

impl Gesture {
    /// A hand is on the window somehow — dragging an edge or the title bar.
    pub fn active(self) -> bool {
        self.live_resize || self.mouse_down
    }
}
