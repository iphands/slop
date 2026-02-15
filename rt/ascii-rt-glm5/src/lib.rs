//! ASCII Raytracer using Vulkan Ray Tracing
//!
//! This library implements a Vulkan-based ray tracer that renders a Cornell box scene
//! with a glass sphere to ASCII output for terminal display.

pub mod vulkan;
pub mod scene;
pub mod renderer;
pub mod terminal;

pub use scene::Scene;
pub use renderer::Renderer;
pub use terminal::TerminalDisplay;

/// Maximum number of ray bounces
pub const MAX_BOUNCES: u32 = 10;

/// Default number of ray bounces
pub const DEFAULT_BOUNCES: u32 = 3;

/// UTF-8 character gradient from dark to light (extended Unicode block characters)
pub const ASCII_GRADIENT: &str = " ·∙:;░▒▓█";
