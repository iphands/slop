//! # q2proto — Quake 2 wire codec (protocol 34)
//!
//! Pure, transport-agnostic byte-level codec for the Q2 client/server protocol, ported
//! from yquake2 `src/common/movemsg.c`. No async, no sockets — just correct byte
//! shuffling, validated by round-trip tests.
//!
//! - [`Reader`] / [`Writer`]: the `MSG_Read*` / `MSG_Write*` primitives (LE scalars,
//!   strings, fixed-point coords, compressed angles/dirs).
//! - [`bytedirs`]: the 162-entry vertex-normal table for direction compression.
//!
//! Opcode tables, `usercmd_t` delta, InfoString, and connectionless framing land in
//! later tasks of Plan 02. See `AGENTS.md` and `context/plans/02_wire_codec_q2proto.md`.

pub mod bytedirs;
pub mod error;
pub mod reader;
pub mod writer;

pub use bytedirs::{BYTEDIRS, NUM_VERTEX_NORMALS};
pub use error::DecodeError;
pub use reader::Reader;
pub use writer::Writer;
