//! Tangent's user interface, drawn by the desktop app.
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
/// Identity on the desk: tracks, racks and bays named by what they are, not
/// by where they sit. Pure; see the module for why positions had to go.
pub mod desk_id;
/// The desk's shape worked out once: drawn order, ranks, classes, folders
/// and clip counts, so every view is O(visible).
pub mod desk_index;
/// The frozen 32-channel seat map and its one road to identity.
pub mod desk_legacy;
/// The desk as the list of tracks that exist, by identity: rows, the
/// specials, plugins by bay, routes, keys. Pure until W6 makes it live.
pub mod desk_model;
/// Format 2: the one desk reader and writer for settings and documents,
/// and the one switch every format-2 side effect reads.
pub mod desk_format;
/// Which way audio flows between the desk's strips, and the one question a
/// route or send picker asks of it: would this close a loop.
pub mod desk_topology;
pub mod dialogs;
pub mod face;
pub mod faces;
pub mod faces_amp;
pub mod faces_analog;
pub mod faces_sampler;
/// The drumpad's document: the kit, saved beside the arrangement.
pub mod drumpad;
/// The drumpad: sixteen sample pads in a popup that floats over any view.
pub mod drumpad_panel;
pub mod fonts;
pub mod fretboard_panel;
pub mod host;
/// The instrument icon set: raw coverage, tinted by the caller.
pub mod icons;
pub mod keys;
pub mod menu;
pub mod midi_event;
/// The app's important messages, said in the corner of the window rather than
/// in a child window that has to win a z-order fight to be seen.
pub mod notify;
pub mod piano;
pub mod peaks;
pub mod ports;
pub mod preset;
pub mod quantize;
pub mod recorder;
pub mod analyzer;
pub mod scope;
/// The mixer view: every source in the app, drawn as a channel strip.
pub mod mixer_panel;
pub mod recorder_panel;
pub mod arrangement;
pub mod smf_read;
/// Writing arrangement notes back out as a `.mid`. The mirror of
/// [`smf_read`], hand-rolled behind the same firewall and for the same
/// reason — see its header for why `tangent-record`'s writer is unreachable
/// and would be the wrong shape even if it were not.
pub mod smf_write;
pub mod arrangement_panel;
pub mod midi_editor;
pub mod settings;
/// Every desk this app ever wrote, lifted onto the frozen 32-wide shape:
/// the era lifts, their provenance and the legacy global-shelf map.
pub mod desk_lift;
/// The era corpus: one literal fixture per desk shape the app has written,
/// and the scans that keep the lifts off the live constants. Test-only.
#[cfg(test)]
mod era_corpus;
/// The phrase scan over every surface that says what a supporter key does.
/// Test-only.
#[cfg(test)]
mod copy_scan;
/// The ban scan over every crate's shipped code: no live code names the
/// fixed desk's width or what grew from it. Test-only.
#[cfg(test)]
mod ban_scan;
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
/// Documents of a known size for the perf probes, and the probes that need
/// nothing but a document. Test-only; see the module header.
#[cfg(test)]
pub(crate) mod perf_fixtures;

/// The version the About box reports.
///
/// Named rather than incidental: `env!("CARGO_PKG_VERSION")` inside `dialogs`
/// now reads THIS crate's version, not the binary's. They are the same string
/// because both take `version.workspace = true`, and this constant is where
/// that coupling is written down so it cannot quietly stop being true.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
