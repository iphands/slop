//! Codec decode errors.

use std::fmt;

/// Errors returned by [`crate::Reader`] reads.
///
/// The C reference (`MSG_Read*` in `movemsg.c`) silently returns `-1` (or 0) on
/// overrun and sets an `overflowed` flag, then keeps parsing. This Rust port instead
/// returns `Err` so a truncated frame can be dropped cleanly rather than mis-parsed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// Not enough bytes remaining for the requested read.
    Eof,
    /// A decoded value was outside its valid range (e.g. a dir index ≥ 162).
    Invalid(&'static str),
    /// A `clc_move` checksum does not match what the server would compute for the
    /// packet's sequence (`sv_user.c:711-722`) — a live server would silently
    /// ignore such a packet.
    ChecksumMismatch,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Eof => write!(f, "unexpected end of message"),
            DecodeError::Invalid(what) => write!(f, "{what} out of range"),
            DecodeError::ChecksumMismatch => write!(f, "clc_move checksum mismatch"),
        }
    }
}

impl std::error::Error for DecodeError {}
