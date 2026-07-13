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
pub mod crc;
pub mod crc_tables;
pub mod entitystate;
pub mod error;
pub mod frame;
pub mod infostring;
pub mod oob;
pub mod ops;
pub mod playerstate;
pub mod reader;
pub mod usercmd;
pub mod writer;

pub use bytedirs::{BYTEDIRS, NUM_VERTEX_NORMALS};
pub use crc::{block_sequence_crc_byte, crc_block};
pub use entitystate::EntityState;
pub use error::DecodeError;
pub use frame::{parse_frame, parse_packet_entities, Frame, FrameRing};
pub use infostring::InfoString;
pub use oob::{is_oob, oob_payload, tokenize, write_oob, OOB_MARKER, OOB_PREFIX};
pub use ops::{ClcOp, SvcOp, PROTOCOL_VERSION, UPDATE_BACKUP, UPDATE_MASK};
pub use playerstate::{PlayerState, PmoveState, MAX_STATS, PM_FREEZE};
pub use reader::Reader;
pub use usercmd::{build_clc_move, Usercmd};
pub use writer::Writer;
