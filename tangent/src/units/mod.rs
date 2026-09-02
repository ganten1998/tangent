//! Standalone DSP kernels for the coming plugin suite.
//!
//! Each unit is one file, proven by its own tests before anything reaches it
//! from a UI — so nothing in here may assume an engine, a window or a
//! settings file. One `pub mod` line per unit, and only that line, so two
//! units landing at once never collide in this file.

pub mod eq;
