# bytemuck - Safe Type Casting

Zero-cost, compile-time checked casting between types with compatible memory layouts.

## Core Traits

### Pod (Plain Old Data)
- Can be safely copied byte-for-byte
- No padding bytes with unspecified values
- All bit patterns valid

### Zeroable
- Type can be safely zeroed (all bytes = 0)
- Includes types like integers, floats, arrays

## Common Usage

### Derive Macros
```rust
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct Vertex {
    position: [f32; 3],
    normal: [f32; 3],
    uv: [f32; 2],
}
```

### Casting Functions
```rust
use bytemuck::{cast, cast_slice, bytes_of};

// Single value to bytes
let vertex = Vertex { ... };
let bytes: &[u8] = bytes_of(&vertex);

// Array to bytes
let vertices = [vertex1, vertex2, vertex3];
let bytes: &[u8] = cast_slice(&vertices);

// Cast between types (same size)
let u32_val: u32 = cast(f32_val); // Reinterpret bits
```

### GPU Buffer Upload
```rust
// Upload vertex data to GPU
let vertex_data = vec![vertex1, vertex2, vertex3];
let bytes = bytemuck::cast_slice(&vertex_data);

unsafe {
    let data_ptr = device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty())?;
    std::ptr::copy_nonoverlapping(
        bytes.as_ptr(),
        data_ptr as *mut u8,
        bytes.len()
    );
    device.unmap_memory(memory);
}
```

### Uniform Buffers
```rust
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct CameraUBO {
    view_proj: [[f32; 4]; 4],  // Mat4 as column-major array
    position: [f32; 3],
    _padding: f32,  // Align to 16 bytes
}

// Upload to UBO
camera_buffer.store(bytemuck::bytes_of(&camera_ubo), device);
```

## Alignment Requirements

### GLSL/Vulkan Layout Rules (std140/std430)
```rust
#[repr(C, align(16))]  // Force 16-byte alignment
#[derive(Copy, Clone, Pod, Zeroable)]
struct UniformData {
    value1: f32,        // offset 0
    _pad1: [f32; 3],    // padding to 16
    matrix: [[f32; 4]; 4],  // offset 16, column-major
    vec3: [f32; 3],     // offset 80
    _pad2: f32,         // padding to multiple of 16
}
```

### Common Padding Patterns
- `Vec3` in GLSL = 16 bytes (add 1 f32 padding in Rust)
- `Mat4` = 64 bytes (4 Vec4s, each 16 bytes)
- Arrays: Each element aligned to 16 bytes in std140

## Integration with Math Libraries

### glam + bytemuck
```rust
// Enable in Cargo.toml: glam = { version = "0.29", features = ["bytemuck"] }
use glam::{Vec3, Mat4};
use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone)]
struct Transform {
    matrix: Mat4,  // glam::Mat4 is Pod
    position: Vec3,
    _padding: f32,
}

unsafe impl Pod for Transform {}
unsafe impl Zeroable for Transform {}
```

## Safety Notes
- **`#[repr(C)]` required** for deterministic layout
- Padding must be explicit for GPU alignment
- No references/pointers (not Pod)
- No `bool` (use `u32` instead for GPU)
- Column-major matrices for Vulkan/OpenGL

## Checked Conversions
```rust
use bytemuck::{try_cast, PodCastError};

match try_cast::<u32, f32>(value) {
    Ok(float_val) => process(float_val),
    Err(PodCastError::AlignmentMismatch) => eprintln!("Bad alignment"),
    _ => {}
}
```

## Common Patterns

### Reading Binary Data
```rust
let bytes: &[u8] = file.read_to_end()?;
let header: &FileHeader = bytemuck::from_bytes(&bytes[0..16]);
let data: &[Vertex] = bytemuck::cast_slice(&bytes[16..]);
```

### Zero Initialization
```rust
let mut buffer: [Vertex; 100] = bytemuck::Zeroable::zeroed();
```
