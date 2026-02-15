# glam - Fast Math Library for Graphics

SIMD-optimized linear algebra library designed for real-time graphics.

## Core Types
- `Vec2`, `Vec3`, `Vec4` - Vectors (f32)
- `Mat2`, `Mat3`, `Mat4` - Matrices (column-major)
- `Quat` - Quaternions for rotations
- `Affine2`, `Affine3` - Affine transformations

## Key Features
- SIMD acceleration (SSE2, SSE3, SSE4.1, AVX, NEON)
- Zero-cost abstractions
- Column-major matrices (matches GPU/Vulkan/OpenGL)
- `bytemuck` integration via feature flag

## Common Operations

### Vectors
```rust
use glam::Vec3;
let a = Vec3::new(1.0, 2.0, 3.0);
let b = Vec3::X; // Unit vector (1, 0, 0)
let c = a + b;
let dot = a.dot(b);
let cross = a.cross(b);
let normalized = a.normalize();
let len = a.length();
```

### Matrices
```rust
use glam::Mat4;
let view = Mat4::look_at_rh(eye, target, up);
let proj = Mat4::perspective_rh(fov, aspect, near, far);
let model = Mat4::from_rotation_translation(rotation, translation);
let mvp = proj * view * model; // Right-to-left multiplication
```

### Transformations
```rust
// Rotation
let rot = Mat4::from_rotation_y(angle_radians);
let quat_rot = Quat::from_rotation_y(angle_radians);

// Translation
let trans = Mat4::from_translation(Vec3::new(x, y, z));

// Scale
let scale = Mat4::from_scale(Vec3::new(sx, sy, sz));

// Combined
let transform = Mat4::from_scale_rotation_translation(scale, rotation, translation);
```

## Performance Tips
- Use `.normalize_or_zero()` instead of checking length first
- Prefer quaternions for rotations (more compact, interpolate better)
- Use `Vec3A` for aligned storage (better SIMD, but 16-byte aligned)
- Matrices are column-major: `mat * vec` transforms vector

## GPU Interop
```rust
// Enable bytemuck feature in Cargo.toml
use bytemuck::{Pod, Zeroable};
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    pos: glam::Vec3,
    normal: glam::Vec3,
}
```

## Right-Handed Coordinate System
- glam provides both `_rh` (right-handed) and `_lh` (left-handed) variants
- Vulkan/OpenGL use right-handed by default
- Use `_rh` functions: `look_at_rh`, `perspective_rh`, etc.

## Const Support
Many operations are `const fn`:
```rust
const ORIGIN: Vec3 = Vec3::ZERO;
const UP: Vec3 = Vec3::Y;
```
