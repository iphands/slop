# crossterm - Cross-Platform Terminal Manipulation

Pure Rust library for terminal control on Windows, Linux, and macOS.

## Core Features
- Cursor movement and visibility control
- Color and text styling
- Terminal size and resize events
- Raw mode for interactive applications
- Keyboard input with modifiers
- Mouse events

## Basic Setup

### Raw Mode (Interactive Apps)
```rust
use crossterm::{
    terminal::{enable_raw_mode, disable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use std::io::stdout;

// Enter raw mode + alternate screen
enable_raw_mode()?;
execute!(stdout(), EnterAlternateScreen)?;

// ... your app ...

// Cleanup
execute!(stdout(), LeaveAlternateScreen)?;
disable_raw_mode()?;
```

### Cursor Control
```rust
use crossterm::cursor::{Hide, Show, MoveTo, MoveToColumn};
use crossterm::execute;

execute!(
    stdout(),
    Hide,                    // Hide cursor
    MoveTo(0, 0),           // Move to top-left
    MoveToColumn(5)         // Move to column 5
)?;
```

## Output & Styling

### Colors
```rust
use crossterm::style::{Color, SetForegroundColor, SetBackgroundColor, ResetColor};

execute!(
    stdout(),
    SetForegroundColor(Color::Red),
    SetBackgroundColor(Color::Black)
)?;
print!("Red text on black");
execute!(stdout(), ResetColor)?;

// 256 colors
SetForegroundColor(Color::AnsiValue(196));

// RGB
SetForegroundColor(Color::Rgb { r: 255, g: 0, b: 0 });
```

### Efficient Rendering
```rust
use std::io::{Write, stdout};
use crossterm::{cursor::MoveTo, QueueableCommand};

let mut stdout = stdout();

// Queue multiple commands (more efficient)
stdout
    .queue(MoveTo(0, 0))?
    .queue(SetForegroundColor(Color::Green))?;
print!("Text");
stdout.flush()?; // Flush once
```

## Input Handling

### Keyboard Events
```rust
use crossterm::event::{read, Event, KeyCode, KeyEvent, KeyModifiers};

loop {
    match read()? {
        Event::Key(KeyEvent { code, modifiers, .. }) => {
            match code {
                KeyCode::Char('q') => break,
                KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => break,
                KeyCode::Up => handle_up(),
                KeyCode::Esc => handle_escape(),
                _ => {}
            }
        }
        Event::Resize(width, height) => handle_resize(width, height),
        _ => {}
    }
}
```

### Non-Blocking Input
```rust
use crossterm::event::{poll, read};
use std::time::Duration;

// Check if event available without blocking
if poll(Duration::from_millis(100))? {
    match read()? {
        Event::Key(key_event) => handle_key(key_event),
        _ => {}
    }
}
```

## Terminal Size
```rust
use crossterm::terminal::size;

let (cols, rows) = size()?;
println!("Terminal: {} columns x {} rows", cols, rows);
```

## Patterns for Terminal Apps

### Full-Screen Renderer
```rust
use std::io::{Write, stdout};
use crossterm::{cursor, terminal, execute, queue};

struct TerminalApp {
    stdout: std::io::Stdout,
}

impl TerminalApp {
    fn init() -> Result<Self> {
        let mut stdout = stdout();
        terminal::enable_raw_mode()?;
        execute!(stdout, terminal::EnterAlternateScreen, cursor::Hide)?;
        Ok(Self { stdout })
    }

    fn render(&mut self, content: &str) -> Result<()> {
        queue!(self.stdout, cursor::MoveTo(0, 0))?;
        write!(self.stdout, "{}", content)?;
        self.stdout.flush()?;
        Ok(())
    }

    fn cleanup(&mut self) -> Result<()> {
        execute!(
            self.stdout,
            cursor::Show,
            terminal::LeaveAlternateScreen
        )?;
        terminal::disable_raw_mode()?;
        Ok(())
    }
}
```

## Performance Tips
- Use `queue!()` instead of `execute!()` for batching
- Batch writes and flush once per frame
- Clear only changed regions, not entire screen
- Use alternate screen for full-screen apps
- Disable line buffering in raw mode for immediate input

## Unicode & ANSI
- Supports full Unicode (UTF-8)
- ANSI escape sequences handled internally
- Half-block characters (▀▄█) useful for double vertical resolution
- Box-drawing characters (│─┌┐└┘) for borders
