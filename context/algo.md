# Algorithms & Techniques

## Ray Tracing

### Basic Ray-Sphere Intersection
```rust
// Ray: origin + t * direction
// Sphere: |point - center|² = radius²
// Solve: |origin + t*dir - center|² = r²

fn ray_sphere_intersect(
    ray_origin: Vec3,
    ray_dir: Vec3,
    sphere_center: Vec3,
    radius: f32
) -> Option<f32> {
    let oc = ray_origin - sphere_center;
    let a = ray_dir.dot(ray_dir);
    let b = 2.0 * oc.dot(ray_dir);
    let c = oc.dot(oc) - radius * radius;
    let discriminant = b * b - 4.0 * a * c;

    if discriminant < 0.0 {
        None
    } else {
        Some((-b - discriminant.sqrt()) / (2.0 * a))
    }
}
```

### Ray-Triangle Intersection (Möller-Trumbore)
Fast algorithm using barycentric coordinates:
```rust
fn ray_triangle_intersect(
    ray_origin: Vec3,
    ray_dir: Vec3,
    v0: Vec3, v1: Vec3, v2: Vec3
) -> Option<f32> {
    let edge1 = v1 - v0;
    let edge2 = v2 - v0;
    let h = ray_dir.cross(edge2);
    let a = edge1.dot(h);

    if a.abs() < EPSILON { return None; } // Ray parallel to triangle

    let f = 1.0 / a;
    let s = ray_origin - v0;
    let u = f * s.dot(h);

    if u < 0.0 || u > 1.0 { return None; }

    let q = s.cross(edge1);
    let v = f * ray_dir.dot(q);

    if v < 0.0 || u + v > 1.0 { return None; }

    let t = f * edge2.dot(q);
    if t > EPSILON { Some(t) } else { None }
}
```

### Acceleration Structures

**BVH (Bounding Volume Hierarchy)**
- Tree of axis-aligned bounding boxes (AABBs)
- Build: Top-down (split at median) or bottom-up
- Traverse: Test ray-AABB, recurse into children if hit
- Good for static scenes, O(log n) traversal

**Vulkan Ray Tracing**
- BLAS (Bottom-Level Acceleration Structure): Geometry (triangles, AABBs)
- TLAS (Top-Level Acceleration Structure): Instance transforms
- GPU builds and traverses acceleration structures
- Use `VK_KHR_acceleration_structure` + `VK_KHR_ray_tracing_pipeline`

### Lighting Models

**Lambertian (Diffuse)**
```rust
let light_dir = (light_pos - hit_point).normalize();
let intensity = normal.dot(light_dir).max(0.0);
let color = albedo * light_color * intensity;
```

**Phong Specular**
```rust
let reflect_dir = reflect(-light_dir, normal);
let view_dir = (camera_pos - hit_point).normalize();
let spec = reflect_dir.dot(view_dir).max(0.0).powf(shininess);
let specular = light_color * spec;
```

**Physically-Based (Cook-Torrance)**
- Fresnel term: Schlick approximation
- Normal distribution: GGX/Trowbridge-Reitz
- Geometry term: Smith's method
- Energy conservation: diffuse + specular ≤ 1

### Recursive Ray Tracing
```rust
fn trace_ray(ray: Ray, depth: u32, max_depth: u32) -> Color {
    if depth >= max_depth { return Color::BLACK; }

    if let Some(hit) = scene.intersect(ray) {
        let reflected_ray = Ray {
            origin: hit.point,
            direction: reflect(ray.direction, hit.normal),
        };
        let reflected_color = trace_ray(reflected_ray, depth + 1, max_depth);

        // Combine local shading + reflections
        hit.material.color * 0.8 + reflected_color * 0.2
    } else {
        skybox_color(ray.direction)
    }
}
```

## Optimization Techniques

### SIMD Vectorization
- Process 4+ rays simultaneously (SSE/AVX)
- Use SIMD-friendly math libraries (glam)
- Align data to 16/32 bytes
- AoS (Array of Structs) → SoA (Struct of Arrays) for better SIMD

### Spatial Partitioning
- **Grid**: Uniform subdivision, O(1) lookup, memory intensive
- **Octree**: Adaptive subdivision, better for clustered geometry
- **KD-Tree**: Binary space partitioning, good for ray tracing
- **BVH**: Best all-around for ray tracing (used in production)

### Early Ray Termination
- Track accumulated opacity/alpha
- Stop when opacity ≥ 1.0 (fully opaque)
- Reduces unnecessary bounces/intersections

### Coherent Rays
- Group rays with similar directions
- Better cache locality in acceleration structures
- Tile-based rendering (process tiles of pixels together)

## Fast Inverse Square Root
Classic optimization for `1/sqrt(x)` (less relevant with hardware sqrt):
```rust
fn fast_inv_sqrt(x: f32) -> f32 {
    let i = x.to_bits();
    let i = 0x5f3759df - (i >> 1);
    let y = f32::from_bits(i);
    y * (1.5 - 0.5 * x * y * y) // Newton iteration
}
```
Modern hardware: Use `x.sqrt().recip()` or `1.0 / x.sqrt()` (compiler optimizes)

## Cornell Box Setup
Classic test scene for global illumination:
- 5 walls: floor, ceiling, left (red), right (green), back (white)
- Light source: Area light on ceiling
- Objects: 1-2 boxes or spheres
- Camera: Looking into box from front opening
- Tests: Color bleeding, soft shadows, ambient occlusion
