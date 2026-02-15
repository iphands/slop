# Project slop

A place to house random ai assisted experiments. Cool stuff may migrate away from here into standalone projects.

## Existing projects

### rt-rs (GUI Ray Tracer)
A real-time Vulkan ray tracer using NVIDIA RTX hardware acceleration. Renders a Cornell Box scene with a glass sphere in a GUI window.

**Location**: `/home/iphands/prog/slop/rt/rt-rs`

**Build and Run**:
```bash
cd rt/rt-rs
cargo build
cargo run
```

**Architecture**:
- `src/main.rs` - Application entry point with winit event loop
- `src/render.rs` - Ray tracing command recording and frame rendering
- `src/scene.rs` - Scene state and animation (sphere movement, lighting)
- `src/vk/` - Modular Vulkan wrapper components:
  - `instance.rs` - Vulkan instance with RTX extensions
  - `device.rs` - Logical device and queue setup
  - `accel.rs` - BLAS and TLAS acceleration structures
  - `pipeline.rs` - Ray tracing pipeline and shader binding table
  - `buffer.rs` - GPU buffer creation and staging
  - `descriptor.rs` - Descriptor sets for shader resources
  - `swapchain.rs` - Swapchain management
- `src/shaders/` - GLSL ray tracing shaders (*.rgen, *.rmiss, *.rchit, *.rint) and precompiled SPIR-V (*.spv)

**Shaders**:
Shaders are written in GLSL with ray tracing extensions and manually compiled to SPIR-V using glslc. The .spv files are embedded at compile time using `include_bytes!` in `pipeline.rs`.

To recompile shaders:
```bash
cd rt/rt-rs/src/shaders
glslc -fshader-stage=rgen raygen.rgen -o raygen.spv
glslc -fshader-stage=rmiss miss.rmiss -o miss.spv
glslc -fshader-stage=rchit closesthit_opaque.rchit -o closesthit_opaque.spv
glslc -fshader-stage=rchit closesthit_glass.rchit -o closesthit_glass.spv
glslc -fshader-stage=rint sphere.rint -o sphere.spv
glslc -fshader-stage=rmiss shadow_miss.rmiss -o shadow_miss.spv
```

**Controls**:
- Arrow keys: Move light in XZ plane
- PgUp/PgDn: Move light in Y
- R/G/B: Toggle light color channels
- +/-: Adjust light intensity
- [/]: Decrease/increase max bounces (0-31)
- Escape: Quit

### ascii-rt-glm5 (ASCII Ray Tracer)
A Vulkan-accelerated ray tracer that renders to the terminal using ASCII art with half-block characters for 2x vertical resolution.

**Location**: `/home/iphands/prog/slop/rt/ascii-rt-glm5`

**Build and Run**:
```bash
cd rt/ascii-rt-glm5
cargo build
cargo run              # Interactive mode
cargo run -- --debug   # Render 10 frames to debug/ directory
```

**Debug Output**:
Debug mode saves frames to `debug/frame_XXX.txt`. View with:
```bash
./view_debug.sh 0      # View frame 0
cat debug/frame_000.txt
```

**Architecture**:
- `src/main.rs` - Application entry with interactive and debug modes
- `src/lib.rs` - Library exports
- `src/renderer.rs` - Ray tracing renderer and ASCII conversion
- `src/scene.rs` - Scene setup (Cornell box)
- `src/terminal.rs` - Terminal display handling with crossterm
- `src/vulkan.rs` - Vulkan initialization and testing

**Controls** (Interactive Mode):
- Up/Down arrows: Adjust light height
- Left/Right arrows: Adjust number of bounces
- [ / ]: Camera zoom
- R: Reset to defaults
- Space: Pause (allows text selection)
- Q or Escape: Quit

## Development Notes

**Vulkan Requirements**:
Both projects require:
- Linux (tested on Gentoo/Ubuntu 22.04+)
- NVIDIA RTX GPU
- Vulkan SDK 1.2+ with ray tracing extensions
- The `rt-rs` project specifically requires RTX hardware support

**Testing**:
Run tests from the project root:
```bash
cd rt/ascii-rt-glm5
cargo test
```

**Clean Build**:
```bash
cargo clean
```

## Key Dependencies

**rt-rs**:
- `ash` (0.38) - Vulkan bindings with linked feature
- `glam` (0.29) - SIMD math library
- `winit` (0.30) - Windowing
- `bytemuck` - POD casts for GPU data

**ascii-rt-glm5**:
- `ash` (0.38) - Vulkan bindings
- `gpu-allocator` (0.27) - GPU memory management
- `nalgebra` (0.33) - Linear algebra
- `crossterm` (0.28) - Terminal handling
- `rayon` (1.10) - Parallel rendering fallback
