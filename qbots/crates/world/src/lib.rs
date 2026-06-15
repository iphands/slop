//! # world — reconstructed map model
//!
//! Parses a Q2 `.bsp` (loose or from a `.pak`) into the collision structures a gamecode
//! bot gets for free via `gi.trace()` but an external client must build itself. Filled in
//! by Plan 05: loader (T1) → collision trace (T2) → PVS (T3) → nav graph (T4).

pub mod bsp;
pub mod collision;
pub mod pak;
pub mod vis;

pub use bsp::{
    Brush, BrushSide, Bsp, Header, Leaf, Lump, Model, Node, Plane as BspPlane, NUM_LUMPS,
};
pub use collision::{
    CollisionModel, Trace, CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_SOLID, CONTENTS_WATER,
    CONTENTS_WINDOW, MASK_SOLID, MASK_WATER,
};
pub use pak::Pak;
pub use vis::Pvs;
