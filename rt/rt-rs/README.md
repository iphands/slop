# RTX Ray Tracer

A minimal Rust-based ray tracer that leverages NVIDIA RTX hardware via Vulkan Ray Tracing (KHR) extensions.

## Project Structure

```
rt-rs/
├── Cargo.toml
├── src/
│   ├── main.rs           # Main application entry point
│   └── vk/               # Vulkan components
│       ├── instance.rs   # Vulkan instance and validation layers
│       ├── device.rs     # Logical device + queues
│       ├── accel.rs      # BLAS + TLAS creation
│       ├── pipeline.rs   # Ray tracing pipeline setup
│       ├── buffer.rs     # GPU buffer creation / staging
│       └── descriptor.rs # Descriptor sets for shaders
├── shaders/              # Shader source files
│   ├── raygen.rgen       # Ray generation shader
│   ├── miss.rmiss        # Miss shader
│   └── closesthit.rchit  # Closest hit shader
└── assets/
    └── meshes/           # Example triangle meshes (obj)
```

## Features Implemented

- [x] Project structure and Cargo.toml
- [x] Vulkan instance initialization with RTX extensions
- [x] Device and queue setup
- [x] Acceleration structure framework
- [ ] Ray tracing pipeline setup
- [ ] Shader compilation and loading
- [ ] Ray dispatch logic
- [ ] Image output functionality
- [ ] Testing with minimal scene

## Building and Running

To build the project:
```bash
cargo build
```

To run:
```bash
cargo run
```

## Requirements

- Linux (tested on Ubuntu 22.04+)
- NVIDIA RTX GPU
- Rust toolchain (stable)
- Vulkan SDK 1.2+ with RT extensions

## Dependencies

- `ash` – Vulkan bindings
- `shaderc` – GLSL → SPIR-V compilation (not currently used, will be added later)
- `glam` – SIMD-friendly math
- `bytemuck` – POD casts
- `winit` – windowing (optional)
- `image` – save output images

## References

- [Vulkan Ray Tracing Tutorial (KHR)](https://gpuopen.com/vulkan-ray-tracing/)
- [Ash Crate](https://github.com/MaikKlein/ash)
- [Shaderc-rs](https://crates.io/crates/shaderc)
- [GLM-like math in Rust: glam](https://crates.io/crates/glam)