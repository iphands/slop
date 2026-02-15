# nalgebra - General-Purpose Linear Algebra

Comprehensive linear algebra library with focus on correctness and features over raw performance.

## Core Types
- `Vector<T, D>` - Generic vectors (dimension D, scalar type T)
- `Matrix<T, R, C>` - Generic matrices (R rows, C columns)
- Type aliases: `Vector3<f32>`, `Matrix4<f64>`, etc.
- `Point3<T>` - Affine points (distinguish from vectors)
- `UnitQuaternion<T>` - Unit quaternions for rotations
- `Isometry3<T>` - Rigid body transformations (rotation + translation)

## Common Operations

### Vectors & Points
```rust
use nalgebra::{Vector3, Point3};
let v = Vector3::new(1.0, 2.0, 3.0);
let p = Point3::new(x, y, z);
let normalized = v.normalize();
let dot = v.dot(&other);
let cross = v.cross(&other);
```

### Matrices
```rust
use nalgebra::Matrix4;
let identity = Matrix4::identity();
let perspective = Matrix4::new_perspective(aspect, fov, near, far);
let look_at = Matrix4::look_at_rh(&eye, &target, &up);

// Matrix operations
let inv = matrix.try_inverse().unwrap();
let transpose = matrix.transpose();
let det = matrix.determinant();
```

### Transformations
```rust
use nalgebra::{Isometry3, UnitQuaternion, Translation3};

// Isometry = rotation + translation (preserves distances)
let iso = Isometry3::from_parts(
    Translation3::new(x, y, z),
    UnitQuaternion::from_euler_angles(roll, pitch, yaw)
);

// Apply transformation
let transformed_point = iso * point;
```

## Generic Programming
```rust
use nalgebra::{Vector, Scalar};
fn my_function<T: Scalar>(v: &Vector3<T>) -> T {
    // Works with f32, f64, complex numbers, etc.
}
```

## Const Generics
Recent versions use const generics for dimensions:
```rust
use nalgebra::SVector;
let v: SVector<f32, 3> = SVector::new(1.0, 2.0, 3.0);
```

## Decompositions
```rust
// SVD, QR, LU, Cholesky, Eigenvalue, etc.
let svd = matrix.svd(true, true);
let qr = matrix.qr();
let lu = matrix.lu();
```

## Performance Considerations
- More compile time overhead than glam
- Generic code can be slower without proper optimization
- Use `#[inline]` for hot paths
- For real-time graphics, consider `glam` instead
- Excellent for scientific computing, physics simulations

## vs glam
- **nalgebra**: Feature-rich, generic, slower compilation, scientific use
- **glam**: Fast, SIMD-optimized, graphics-focused, minimal features
- Choice: Use glam for graphics, nalgebra for general-purpose math
