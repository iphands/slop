//! # world — reconstructed map model
//!
//! Parses a `.bsp` into a collision tree (trace + point-contents), a PVS query, and an
//! auto-generated navigation graph — the `gi.trace()` / world knowledge a gamecode bot
//! gets for free but an external client must build itself. Filled in by Plan 05.
//!
//! See `AGENTS.md` and `context/plans/05_world_model.md`.

#[cfg(test)]
mod tests {
    #[test]
    fn sanity() {
        assert_eq!(2 + 2, 4);
    }
}
