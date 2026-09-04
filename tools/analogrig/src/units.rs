//! The one oversampler, reached the way `main.rs` reaches the synth: the one
//! true copy of `units/drive.rs`, by `#[path]`. The analog module names it
//! as `crate::units::drive`, and this file is what makes that path exist in
//! the bench crate as well as the app.
//!
//! A file of its own rather than an inline `mod units { .. }` in main.rs:
//! for an inline module the compiler resolves `#[path]` from `src/units/`,
//! and a directory that does not exist cannot be walked through with `..`.
#![allow(dead_code)]

#[path = "../../../tangent/src/units/drive.rs"]
pub mod drive;
