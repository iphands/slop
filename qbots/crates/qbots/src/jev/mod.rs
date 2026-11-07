//! # qbots::jev — Plan 72's I/O side: the prompt renderer, and (T4/T5) the `/v1/systemone`
//! client and the process-wide decision hub.
//!
//! The brain crate owns the closed vocabulary (`brain::jev`); this module turns it into bytes.

pub mod bucket;
pub mod hub;
pub mod net;
pub mod probe;
pub mod render;
pub mod startup;
pub mod transport;
pub mod wire;

#[cfg(test)]
mod tests_hub;
#[cfg(test)]
mod tests_net;
