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
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DecodeError::Eof => write!(f, "unexpected end of message"),
            DecodeError::Invalid(what) => write!(f, "{what} out of range"),
        }
    }
}

impl std::error::Error for DecodeError {}
