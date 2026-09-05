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

/// **What the synth's demo test names beside it, stood in for.**
/// `synth.rs`'s ignored `dx7_demo_wav` reaches `super::super::Cartridge`:
/// two levels up from its test module is `crate::dx7`, which in the app is
/// the directory holding the real reader and in this bench is this file —
/// so the bench's TEST build had been broken since the crate rename on
/// 2026-08-28 without anyone running it here (the app's own `cargo test`
/// covered the keys tests meanwhile). The real reader drags
/// the editor and the factory bank in behind it, none of which a bench
/// that strikes keys needs, so this is a shape with the three fields the
/// test reads and a `load` that says no. The test is ignored in the app
/// as well; it exists to be run by hand with a bank on disk.
#[cfg(test)]
pub struct Cartridge {
    pub name: String,
    pub checksum_ok: bool,
    pub voices: Vec<voice::Voice>,
}

#[cfg(test)]
impl Cartridge {
    pub fn load(_path: &std::path::Path) -> Result<Self, String> {
        Err("the keys bench carries no cartridge reader; run this demo in the app".to_owned())
    }
}
