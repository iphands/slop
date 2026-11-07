//! # qbots::jev — Plan 72's I/O side: the prompt renderer, and (T4/T5) the `/v1/systemone`
//! client and the process-wide decision hub.
//!
//! The brain crate owns the closed vocabulary (`brain::jev`); this module turns it into bytes.

// Plan 72 builds this module bottom-up across T2–T5 and wires it into the bot loop in T7/T8;
// until then the binary has no caller, so rustc would flag every item as dead. Remove this
// allow in T8 (tracked in the Plan 72 tracker's Follow-ups) — anything still unused then is
// genuinely dead.
#![allow(dead_code)]

pub mod bucket;
pub mod hub;
pub mod net;
pub mod render;
pub mod transport;
pub mod wire;

#[cfg(test)]
mod tests_hub;
#[cfg(test)]
mod tests_net;
