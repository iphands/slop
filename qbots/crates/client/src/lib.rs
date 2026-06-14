//! # client — connection + frame loop
//!
//! One tokio task per bot: the Q2 connect handshake, netchan, server-frame parsing, and
//! the per-frame `clc_move` heartbeat. Builds on `q2proto`. Filled in by Plans 03–04.
//!
//! See `AGENTS.md` and `context/plans/03_connection_client.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
