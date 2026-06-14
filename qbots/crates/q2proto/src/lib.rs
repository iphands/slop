//! # q2proto — Quake 2 wire codec (protocol 34)
//!
//! Pure, transport-agnostic byte-level codec for the Q2 client/server protocol:
//! `MSG_*` read/write primitives, `usercmd_t` delta encode/decode, InfoString, and
//! connectionless (out-of-band) framing. Filled in by Plan 02.
//!
//! See `AGENTS.md` and `context/plans/02_wire_codec_q2proto.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
