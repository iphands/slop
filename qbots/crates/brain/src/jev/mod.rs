//! # brain::jev — Plan 72's model-driven decision layer (the brain-side half)
//!
//! Everything here is **closed data and pure functions**: no I/O, no async, no serde, no locks.
//! The hosted `/v1/systemone` decision model is reached only through the hub in `crates/qbots`;
//! this crate defines what may be *said* to it ([`state`]) and how its answers are judged.
//!
//! The security boundary is the type system: a [`state::DecisionState`] holds enums, integers,
//! booleans and `&'static str` — nothing a server, map author or player can put on the wire can
//! reach a prompt, because no `String` from the wire can become `&'static` without an explicit,
//! reviewable leak.

pub mod question;
pub mod state;
