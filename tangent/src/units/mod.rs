//! Standalone DSP kernels for the coming plugin suite.
//!
//! Each unit is one insert's whole signal path in a single struct: a
//! `new(sample_rate)`, an in-place `process` over interleaved stereo, and a
//! `clear` — proven by its own tests before anything reaches it from a UI.
//! Nothing in here may assume an engine, a window or a settings file; the
//! hosts that will own these are not written yet, and until they are the
//! units exist to compile and to keep their tests honest. One `pub mod`
//! line per unit, and only that line, so two units landing at once never
//! collide in this file.

pub mod comp;
pub mod convolver;
pub mod eq;
pub mod imager;
pub mod sweep;
pub mod tape;
