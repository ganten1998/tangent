//! Tangent's user interface, shared by the desktop app and the VST3 plugin.
//!
//! Everything here draws with `egui` and nothing here knows what is hosting it.
//! See `Cargo.toml` for why the absent dependencies matter more than the
//! present ones.

/// Focus, keyboard reach and screen-reader semantics for surfaces made of one
/// `Painter`. See the module for why every panel needed a second enumerator.
pub mod a11y;
pub mod app;
/// Automation curves: a control's value written down as a shape over time.
pub mod automation;
/// The busy overlay: what covers a mid-session stall, and says what it is.
pub mod busy;
pub mod chord_strip;
pub mod dialogs;
pub mod face;
pub mod faces;
pub mod faces_amp;
/// The drumpad's document: the kit, saved beside the arrangement.
pub mod drumpad;
/// The drumpad: sixteen sample pads in a popup that floats over any view.
pub mod drumpad_panel;
pub mod fonts;
pub mod fretboard_panel;
pub mod host;
pub mod keys;
pub mod menu;
pub mod midi_event;
pub mod piano;
pub mod ports;
pub mod preset;
pub mod recorder;
pub mod scope;
/// The mixer view: every source in the app, drawn as a channel strip.
pub mod mixer_panel;
pub mod recorder_panel;
pub mod arrangement;
pub mod smf_read;
pub mod arrangement_panel;
pub mod midi_editor;
pub mod settings;
/// Musical time: the tempo map, the meter map, and the two conversions
/// everything else in the app used to do with a multiply.
pub mod tempo;
pub mod varispeed;
pub mod confetti;
pub mod splash;
pub mod staff;
pub mod shell;
pub mod routing;
pub mod theme;
pub mod theory_panel;
pub mod tapring;

/// The version the About box reports.
///
/// Named rather than incidental: `env!("CARGO_PKG_VERSION")` inside `dialogs`
/// now reads THIS crate's version, not the binary's. They are the same string
/// because both take `version.workspace = true`, and this constant is where
/// that coupling is written down so it cannot quietly stop being true.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
