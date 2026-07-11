//! # world — reconstructed map model
//!
//! Parses a Q2 `.bsp` (loose or from a `.pak`) into the collision structures a gamecode
//! bot gets for free via `gi.trace()` but an external client must build itself. Filled in
//! by Plan 05: loader (T1) → collision trace (T2) → PVS (T3) → nav graph (T4).

pub mod bsp;
pub mod build;
pub mod collision;
pub mod mapcache;
pub mod navgraph;
pub mod navmesh;
pub mod pak;
pub mod vis;

pub use bsp::{
    Brush, BrushSide, Bsp, BspEntity, Header, Leaf, Lump, Model, Node, Plane as BspPlane,
    SpawnPoint, NUM_LUMPS,
};
pub use build::{
    cached_map_nav, check_spawn_connectivity, generate_map_nav, spacing_subdir, MapNavBuild,
    GRID_SPACING, JUMP_SPACING,
};
pub use collision::{
    water_channel_world, CollisionModel, Trace, CONTENTS_LAVA, CONTENTS_SLIME, CONTENTS_SOLID,
    CONTENTS_WATER, CONTENTS_WINDOW, MASK_SOLID, MASK_WATER,
};
pub use mapcache::{load as load_mapcache, save as save_mapcache, Fingerprint};
pub use navgraph::{
    walkable_stair, EdgeKind, NavGraph, RideInfo, HULL_MAXS, HULL_MINS, STAIR_MAX, STEP,
};
pub use navmesh::{Heightfield, NavMesh, VoxelParams};
pub use pak::Pak;
pub use vis::Pvs;
