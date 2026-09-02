//! Standalone DSP kernels for the plugin suite.
//!
//! Each unit is one insert's whole signal path in a single struct: a
//! `new(sample_rate)`, an in-place `process` over interleaved stereo, and a
//! `clear`. Nothing here is reachable from any UI yet, on purpose — the hosts
//! that will own these are not written, and until they are the units exist to
//! compile and to keep their tests honest. One `pub mod` line per unit.

pub mod tape;
