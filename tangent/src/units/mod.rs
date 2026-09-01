//! Standalone DSP kernels for the coming plugin suite.
//!
//! Each unit is a self-contained processor in the house shape — `new(sample_rate)`,
//! an in-place interleaved `process`, a `clear()`, and a params struct of
//! normalised 0..=1 fields with the mapping documented next to the DSP, exactly
//! as `effects.rs` does it. Nothing links to these yet; that is deliberate. The
//! kernels are compiled and tested long before any panel can reach them. One
//! `pub mod` line per unit, so parallel work never collides on this file.

pub mod imager;
