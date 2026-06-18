//! Navmesh backend — walkable-polygon navigation built by **voxelizing the collision
//! model** (Recast-style), as an alternative representation to the waypoint graph.
//!
//! Pipeline (built once, cached): heightfield (walkable spans) → regions → contours →
//! convex polygons + portals → polygon A* + funnel. Unlike the waypoint graph, navigation
//! quality here is independent of sampling density: `cell_size` is a build-time resolution
//! knob, not a runtime navigation knob.
//!
//! This module keeps `world` glam-free — geometry is `[f32; 3]`/`[f32; 2]` like the rest of
//! the crate; the brain converts to `glam::Vec3` at the steering boundary.

pub mod heightfield;

pub use heightfield::{Heightfield, VoxelParams};
