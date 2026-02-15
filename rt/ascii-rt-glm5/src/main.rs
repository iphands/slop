//! ASCII Raytracer - A Vulkan-accelerated ray tracer that renders to the terminal
//!
//! Controls:
//! - Up/Down arrows: Adjust light height
//! - Left/Right arrows: Adjust number of bounces
//! - R: Reset to defaults
//! - Q or Escape: Quit
//!
//! Usage:
//!   ascii_rt_glm5           - Run interactive mode
//!   ascii_rt_glm5 --debug   - Render 10 frames to ./debug/frame_XXX.txt files

use ascii_rt_glm5::renderer::Renderer;
use ascii_rt_glm5::scene::Scene;
use ascii_rt_glm5::terminal::{TerminalDisplay, parse_key_event, Action};
use std::time::{Duration, Instant};
use std::fs;
use std::path::Path;

fn main() {
    // Check for --debug flag
    let args: Vec<String> = std::env::args().collect();
    if args.iter().any(|a| a == "--debug" || a == "-d") {
        run_debug_mode();
        return;
    }

    println!("ASCII Raytracer - Initializing...");
    println!("This may take a moment to render the first frame.\n");

    // Try to initialize Vulkan (optional - will use CPU fallback)
    match ascii_rt_glm5::vulkan::test_vulkan() {
        Ok(msg) => eprintln!("GPU: {}", msg),
        Err(e) => eprintln!("GPU: {} (using CPU rendering)", e),
    }

    // Set up terminal display
    let mut terminal = match TerminalDisplay::new() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("Failed to initialize terminal: {}", e);
            std::process::exit(1);
        }
    };

    // Initialize renderer and scene
    // Use 2x height for half-block rendering (2 vertical pixels per character)
    let (width, height) = terminal.get_size();
    let renderer_width = width.max(10);
    let renderer_height = (height * 2).max(10);

    // Create debug directory if it doesn't exist
    let _ = std::fs::create_dir_all("debug");

    // Write dimensions to file for debugging
    let dim_info = format!(
        "Terminal get_size: {}x{}\nRenderer: {}x{}\nOutput rows: {}\n",
        width, height,
        renderer_width, renderer_height,
        (renderer_height + 1) / 2
    );
    let _ = std::fs::write("debug/interactive_dims.txt", &dim_info);

    let mut renderer = Renderer::new(renderer_width, renderer_height);
    let mut scene = Scene::cornell_box();

    // Animation and timing
    let mut time = 0.0f32;
    let mut last_frame = Instant::now();
    let frame_time = Duration::from_millis(100); // ~10 FPS for smooth animation
    let mut paused = false;

    // Frame counter for debugging
    let mut frame_count = 0u32;
    let max_debug_frames = 3;

    // Main loop
    'main_loop: loop {
        // Check for terminal resize
        if terminal.check_resize() {
            let (width, height) = terminal.get_size();
            renderer.resize(width.max(10), (height * 2).max(10));
        }

        // Handle input
        match terminal.poll_input(Duration::from_millis(16)) {
            Ok(Some(key_event)) => {
                match parse_key_event(key_event) {
                    Action::Quit => break 'main_loop,
                    Action::LightUp => {
                        scene.adjust_light_height(0.1);
                    }
                    Action::LightDown => {
                        scene.adjust_light_height(-0.1);
                    }
                    Action::BouncesUp => {
                        scene.adjust_bounces(1);
                    }
                    Action::BouncesDown => {
                        scene.adjust_bounces(-1);
                    }
                    Action::Reset => {
                        scene = Scene::cornell_box();
                    }
                    Action::Pause => {
                        paused = !paused;
                    }
                    Action::CameraForward => {
                        renderer.get_camera_mut().adjust_distance(0.2);
                    }
                    Action::CameraBack => {
                        renderer.get_camera_mut().adjust_distance(-0.2);
                    }
                    Action::None => {}
                }
            }
            Ok(None) => {}
            Err(e) => {
                eprintln!("Input error: {}", e);
            }
        }

        // Skip rendering when paused (allows text selection)
        if paused {
            continue;
        }

        // Throttle rendering
        if last_frame.elapsed() < frame_time {
            continue;
        }
        last_frame = Instant::now();

        // Update scene animation (1.5x speed)
        time += 0.050;
        scene.update_sphere(time);

        // Render frame
        renderer.render(&scene);

        // Convert to ASCII with half-block for 2x vertical resolution
        let ascii_output = renderer.to_ascii_halfblock();

        // Save first few frames for debugging
        if frame_count < max_debug_frames {
            let filename = format!("debug/interactive_frame_{:03}.txt", frame_count);
            let _ = std::fs::write(&filename, &ascii_output);

            // Also check line lengths
            let line_lengths: Vec<usize> = ascii_output.lines().map(|l| l.len()).collect();
            let dim_info = format!(
                "Frame {} dimensions:\nTerminal: {}x{}\nRenderer: {}x{}\nOutput lines: {}\nFirst 5 line lengths: {:?}\n",
                frame_count,
                width, height,
                renderer_width, renderer_height,
                line_lengths.len(),
                &line_lengths[..5.min(line_lengths.len())]
            );
            let _ = std::fs::write("debug/interactive_line_lengths.txt", &dim_info);

            frame_count += 1;
        }

        // Build status line
        let status = format!(
            "Light Y: {:.2} | Bounces: {} | [↑↓] Light  [←→] Bounces  [[]] Zoom  [R]eset  [SPACE] Pause  [Q]uit",
            scene.light_position.y,
            scene.max_bounces
        );

        // Render to terminal
        if let Err(e) = terminal.render(&ascii_output, &status) {
            if e.kind() == std::io::ErrorKind::BrokenPipe {
                break;
            }
            eprintln!("Render error: {}", e);
        }
    }

    println!("\nThanks for using ASCII Raytracer!");
}

/// Debug mode: Render 10 frames to files in ./debug/ directory
fn run_debug_mode() {
    println!("ASCII Raytracer - Debug Mode");
    println!("Rendering 10 frames to ./debug/ directory...\n");

    // Create debug directory
    let debug_dir = Path::new("debug");
    if !debug_dir.exists() {
        if let Err(e) = fs::create_dir_all(debug_dir) {
            eprintln!("Failed to create debug directory: {}", e);
            std::process::exit(1);
        }
    }

    // Try to get terminal size, fall back to fixed size
    let (width, height) = match crossterm::terminal::size() {
        Ok((w, h)) => {
            let w = w as usize;
            let h = (h.saturating_sub(2)) as usize; // Leave room for status
            println!("Terminal size: {}x{} (renderer will be {}x{})", w, h, w, h * 2);
            (w.max(10), (h * 2).max(10))
        }
        Err(e) => {
            println!("Could not get terminal size ({}), using default 120x72", e);
            (120, 72)
        }
    };

    let mut renderer = Renderer::new(width, height);
    let mut scene = Scene::cornell_box();

    // Render 10 frames
    for frame in 0..10 {
        let time = frame as f32 * 0.05;
        scene.update_sphere(time);
        renderer.render(&scene);

        let ascii_output = renderer.to_ascii_halfblock();

        let filename = format!("debug/frame_{:03}.txt", frame);
        match fs::write(&filename, &ascii_output) {
            Ok(_) => println!("Wrote {}", filename),
            Err(e) => eprintln!("Failed to write {}: {}", filename, e),
        }
    }

    println!("\nDebug frames saved to ./debug/");
    println!("View with: cat debug/frame_000.txt");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_renderer_creation() {
        let renderer = Renderer::new(80, 48); // 2x height for half-block
        assert_eq!(renderer.to_ascii().lines().count(), 48);
    }

    #[test]
    fn test_scene_creation() {
        let scene = Scene::cornell_box();
        assert!(scene.max_bounces >= 1);
    }
}
