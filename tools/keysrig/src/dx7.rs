//! The DX7, reached the way `main.rs` reaches the keys: the one true copy,
//! by `#[path]`. Only the three files the synth needs — the cartridge reader
//! and the editor stay in the app, because this bench strikes keys and
//! measures, it does not load banks.
//!
//! A file of its own rather than an inline `mod dx7 { .. }` in main.rs: for
//! an inline module the compiler resolves `#[path]` from `src/dx7/`, and a
//! directory that does not exist cannot be walked through with `..`.
#![allow(dead_code)]

#[path = "../../../tangent/src/dx7/algorithms.rs"]
pub mod algorithms;
#[path = "../../../tangent/src/dx7/synth.rs"]
pub mod synth;
#[path = "../../../tangent/src/dx7/voice.rs"]
pub mod voice;
