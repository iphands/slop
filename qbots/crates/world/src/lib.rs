//! # world — reconstructed map model
//!
//! Parses a Q2 `.bsp` (loose or from a `.pak`) into the collision structures a gamecode
//! bot gets for free via `gi.trace()` but an external client must build itself. Filled in
//! by Plan 05: loader (T1) → collision trace (T2) → PVS (T3) → nav graph (T4).

pub mod bsp;
pub mod pak;

pub use bsp::{Brush, BrushSide, Bsp, Header, Leaf, Lump, Model, Node, Plane, NUM_LUMPS};
pub use pak::Pak;
