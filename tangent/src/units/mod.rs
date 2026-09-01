//! Standalone DSP kernels for the coming plugin suite.
//!
//! Each unit is a complete, tested processor with the same buffer contract as
//! the fixed chain in `effects.rs` — in place, interleaved, stride =
//! `channels`, first two channels only — but no seat in that chain and no
//! panel. That is deliberate: the DSP is proven here, alone, before anything
//! is wired to reach it.

pub mod comp;
