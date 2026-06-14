//! # client — connection + frame loop
//!
//! One tokio task per bot: the Q2 connect handshake, netchan, server-frame parsing, and
//! the per-frame `clc_move` heartbeat. Builds on `q2proto`.
//!
//! See `AGENTS.md` and `context/plans/completed/03_connection_client.md`.

pub mod netchan;
pub mod userinfo;

pub use netchan::Netchan;
pub use userinfo::Userinfo;
