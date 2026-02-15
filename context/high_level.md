# High Level Dependencies Overview

## Graphics & Rendering

### Vulkan Bindings
- **ash** - Low-level Vulkan bindings for Rust
  - Used in: `rt-rs`, `ascii-rt-glm5`
  - Pros: Direct Vulkan API access, minimal overhead, type-safe wrappers, supports all Vulkan features
  - Cons: Verbose, requires manual memory/resource management, steep learning curve
  - Alternatives: `vulkano` (higher-level, safer but more restrictive), `wgpu` (cross-platform abstraction)

### Math Libraries
- **glam** - Fast SIMD-optimized math library
  - Used in: `rt-rs`
  - Pros: Excellent performance (SIMD), minimal dependencies, simple API, small binary size
  - Cons: Less features than nalgebra, f32-focused

- **nalgebra** - Comprehensive linear algebra library
  - Used in: `ascii-rt-glm5`
  - Pros: Feature-rich, generic over scalar types, supports complex operations (decompositions, etc.)
  - Cons: Slower than glam for simple operations, larger compile times
  - Note: Choose `glam` for real-time graphics, `nalgebra` for scientific/general-purpose math

## Windowing & Terminal

### Window Management
- **winit** - Cross-platform window creation and event handling
  - Used in: `rt-rs`
  - Pros: Mature, widely used, good OS integration, supports all major platforms
  - Alternatives: `sdl2`, `glutin`

- **ash-window** - Helper for creating Vulkan surfaces from winit windows
  - Used in: `rt-rs`
  - Integrates winit windows with ash/Vulkan

### Terminal Control
- **crossterm** - Cross-platform terminal manipulation
  - Used in: `ascii-rt-glm5`
  - Pros: Pure Rust, works on Windows/Unix/macOS, async support, feature-rich
  - Alternatives: `termion` (Unix-only), `console` (similar features)

## Memory & Performance

### GPU Memory Management
- **gpu-allocator** - Vulkan/DX12/Metal memory allocator
  - Used in: `ascii-rt-glm5`
  - Pros: Handles complex GPU memory allocation strategies, reduces fragmentation
  - Based on AMD's Vulkan Memory Allocator (VMA)

### Data Casting
- **bytemuck** - Safe, zero-cost casting between data types
  - Used in: `rt-rs`
  - Pros: No runtime overhead, compile-time safety checks, works with GPU buffers
  - Essential for uploading data to GPU (vertices, uniforms, etc.)

### Parallel Processing
- **rayon** - Data parallelism library
  - Used in: `ascii-rt-glm5`
  - Pros: Easy parallel iterators, work-stealing scheduler, excellent CPU utilization
  - Use for: CPU-based ray tracing, image processing, batch operations
