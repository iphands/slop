# winit - Cross-Platform Window Management

Pure Rust window creation and event handling for Windows, macOS, Linux, Android, iOS, and Web.

## Core Concepts
- `EventLoop` - Runs the main event loop
- `Window` - Represents a native window
- `WindowAttributes` - Window configuration (title, size, etc.)
- `ApplicationHandler` - Trait for handling application events

## Basic Setup (Modern 0.30+)

### Application Structure
```rust
use winit::application::ApplicationHandler;
use winit::event::{WindowEvent, KeyEvent};
use winit::event_loop::{EventLoop, ActiveEventLoop};
use winit::window::{Window, WindowId};

struct App {
    window: Option<Window>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        let attrs = Window::default_attributes()
            .with_title("My App")
            .with_inner_size(winit::dpi::LogicalSize::new(800, 600));
        self.window = Some(event_loop.create_window(attrs).unwrap());
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => {
                // Render frame
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let event_loop = EventLoop::new().unwrap();
    let mut app = App { window: None };
    event_loop.run_app(&mut app).unwrap();
}
```

## Window Configuration

### Window Attributes
```rust
use winit::window::Window;
use winit::dpi::{LogicalSize, PhysicalSize};

let attrs = Window::default_attributes()
    .with_title("My Window")
    .with_inner_size(LogicalSize::new(1920, 1080))
    .with_resizable(true)
    .with_transparent(false)
    .with_decorations(true)
    .with_fullscreen(None);
```

### DPI Handling
```rust
// Logical size (DPI-independent)
let logical_size = LogicalSize::new(800, 600);

// Physical size (actual pixels)
let physical_size: PhysicalSize<u32> = window.inner_size();

// Scale factor
let scale = window.scale_factor(); // e.g., 2.0 on retina displays
```

## Event Handling

### Keyboard Input
```rust
use winit::event::{KeyEvent, ElementState};
use winit::keyboard::{PhysicalKey, KeyCode};

WindowEvent::KeyboardInput {
    event: KeyEvent {
        physical_key: PhysicalKey::Code(key_code),
        state: ElementState::Pressed,
        ..
    },
    ..
} => {
    match key_code {
        KeyCode::Escape => event_loop.exit(),
        KeyCode::ArrowUp => move_up(),
        KeyCode::KeyW => move_forward(),
        _ => {}
    }
}
```

### Mouse Input
```rust
WindowEvent::CursorMoved { position, .. } => {
    let x = position.x;
    let y = position.y;
    handle_mouse_move(x, y);
}

WindowEvent::MouseInput { state, button, .. } => {
    if state == ElementState::Pressed {
        match button {
            MouseButton::Left => handle_left_click(),
            _ => {}
        }
    }
}
```

### Window Resize
```rust
WindowEvent::Resized(physical_size) => {
    // Update renderer/swapchain
    renderer.resize(physical_size.width, physical_size.height);
}
```

## Integration with Graphics APIs

### Vulkan (via ash-window)
```rust
use ash_window;
use raw_window_handle::{HasDisplayHandle, HasWindowHandle};

let display_handle = window.display_handle().unwrap().as_raw();
let window_handle = window.window_handle().unwrap().as_raw();

// Create Vulkan surface
let surface = unsafe {
    ash_window::create_surface(&entry, &instance, display_handle, window_handle, None)?
};
```

## Redraw Loop
```rust
// Continuous rendering
WindowEvent::RedrawRequested => {
    render_frame();
    window.request_redraw(); // Request next frame
}

// In resumed() or after initial setup:
window.request_redraw(); // Kick off rendering
```

## Performance Tips
- Use `WindowEvent::RedrawRequested` for rendering
- Call `window.request_redraw()` to schedule next frame
- Don't render in other events (MainEventsCleared, etc.)
- Handle resize events to recreate swapchains
- Use `physical_size` for graphics APIs (actual pixels)

## Platform-Specific Notes
- **Windows**: Supports DPI awareness, Win32 API backend
- **macOS**: Cocoa backend, supports Retina displays
- **Linux**: X11 and Wayland support
- **Web**: WASM support via web-sys
