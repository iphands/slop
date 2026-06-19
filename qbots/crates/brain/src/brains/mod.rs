//! # brain::brains — the brain plugin layer (Plan 23)
//!
//! `core` defines the `trait Brain` contract + shared I/O types. The `BrainKind` enum +
//! `build_brain` factory (Plan 23 T4) select an implementation at startup, exactly mirroring
//! the nav layer's `NavMode` / `build_navigator`.

pub mod core;

pub use core::{Brain, BrainConfig, BrainContext, BrainMap, BrainOutput};
