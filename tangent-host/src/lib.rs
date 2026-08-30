//! Tangent's plugin host.
//!
//! Separate from `tangent-record` because it is the only part of the product that
//! loads foreign code into the process, and because it is the feature's
//! critical path — see `docs/RECORDER-PLAN.md` §8 and §12.
//!
//! **VST3, and MIT.** That combination became possible on 2025-10-29, when
//! Steinberg relicensed the VST3 SDK from GPLv3-or-proprietary to MIT with VST
//! 3.8. The bindings here are `vst3` 0.3.0 (MIT OR Apache-2.0), which ships
//! pre-generated bindings and therefore needs no libclang, no SDK download and
//! no C toolchain — so the existing `cargo xwin` cross-build survives.
//!
//! Four parts: [`scan`] finds modules and reads what they claim to be,
//! [`instance`] turns one of those claims into a running instrument,
//! [`editor`] opens that instrument's own UI in a window so its sound can be
//! changed, and [`state`] is how the change survives quitting the app.

pub mod editor;
pub mod instance;
/// LV2 hosting, beside VST3. Only where an LV2 ecosystem exists — see the
/// dependency note in `Cargo.toml`. A target gate, not a feature gate.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub mod lv2;
/// An LV2 plugin's own editor window, embedded in one of ours. Separate from
/// `lv2` because a plugin and its UI are two different shared objects that need
/// not both exist.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub mod lv2ui;
pub mod ready;
pub mod scan;
pub mod state;

pub use editor::{Editor, EditorError, EditorHandle};
pub use instance::{
    Bus, Control, Instance, LatencyHandle, Note, ParamPoint, Rendered, Setup,
};
pub use ready::{Policy, Readiness, State as ReadyState};
pub use scan::{discover, discover_in, search_paths, vendor_of, ClassInfo, Module};
pub use state::{ParamInfo, StateHandle, MAX_STATE_BYTES};
