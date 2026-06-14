//! # client — connection + frame loop
//!
//! One tokio task per bot: the Q2 connect handshake, netchan, server-frame parsing, and
//! the per-frame `clc_move` heartbeat. Builds on `q2proto`.
//!
//! See `AGENTS.md` and `context/plans/completed/03_connection_client.md`.

pub mod conn;
pub mod netchan;
pub mod parse;
pub mod userinfo;

pub use conn::{run, Conn, ConnState};
pub use netchan::Netchan;
pub use parse::{parse_message, ConfigStrings, ServerData, SvcEvent, MAX_CONFIGSTRINGS};
pub use userinfo::Userinfo;
