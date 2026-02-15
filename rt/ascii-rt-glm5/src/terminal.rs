//! Terminal display and input handling

use crossterm::{
    event::{self, Event, KeyCode, KeyEvent},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use std::io::{self, BufWriter, Stdout, Write, stdout};
use std::time::{Duration, Instant};

/// Terminal display handler with buffered output
pub struct TerminalDisplay {
    width: u16,
    height: u16,
    last_resize_check: Instant,
    buffer: BufWriter<Stdout>,
}

impl TerminalDisplay {
    pub fn new() -> io::Result<Self> {
        // Enter alternate screen first to get accurate dimensions
        let mut stdout = stdout();
        execute!(stdout, EnterAlternateScreen)?;
        terminal::enable_raw_mode()?;

        // Clear the alternate screen to ensure a clean slate
        execute!(stdout, terminal::Clear(terminal::ClearType::All))?;

        // Small delay to let terminal process the escape codes
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Get terminal size after entering alternate screen
        let (width, height) = terminal::size()?;

        let adjusted_height = height.saturating_sub(2); // Leave room for status line

        Ok(Self {
            width,
            height: adjusted_height,
            last_resize_check: Instant::now(),
            buffer: BufWriter::new(stdout),
        })
    }

    pub fn get_size(&self) -> (usize, usize) {
        (self.width as usize, self.height as usize)
    }

    /// Check if terminal has been resized
    pub fn check_resize(&mut self) -> bool {
        if self.last_resize_check.elapsed() < Duration::from_millis(100) {
            return false;
        }
        self.last_resize_check = Instant::now();

        if let Ok((new_width, new_height)) = terminal::size() {
            let new_height = new_height.saturating_sub(2);
            if new_width != self.width || new_height != self.height {
                self.width = new_width;
                self.height = new_height;
                return true;
            }
        }
        false
    }

    /// Render ASCII content to terminal with line-by-line positioning
    /// This prevents long lines from corrupting cursor position
    pub fn render(&mut self, content: &str, status: &str) -> io::Result<()> {
        // Hide cursor and disable line wrap to prevent flickering
        // \x1b[?25l = hide cursor, \x1b[?7l = disable line wrap
        write!(self.buffer, "\x1b[?25l\x1b[?7l")?;

        // Write each line with explicit cursor positioning
        // This ensures even if a line is too long, it won't corrupt subsequent lines
        for (i, line) in content.lines().enumerate() {
            // Position cursor at start of line (row i+1, column 1)
            write!(self.buffer, "\x1b[{};1H{}", i + 1, line)?;
        }

        // Clear from cursor to end of screen (removes leftover from larger frames)
        write!(self.buffer, "\x1b[J")?;

        // Position status line and clear that line
        let status_row = content.lines().count() + 1;
        write!(self.buffer, "\x1b[{};1H\x1b[K{}", status_row, status)?;

        // Show cursor and re-enable line wrap
        // \x1b[?25h = show cursor, \x1b[?7h = enable line wrap
        write!(self.buffer, "\x1b[?25h\x1b[?7h")?;

        // Flush all writes
        self.buffer.flush()?;

        Ok(())
    }

    /// Check for keyboard input
    pub fn poll_input(&self, timeout: Duration) -> io::Result<Option<KeyEvent>> {
        if event::poll(timeout)? {
            if let Event::Key(key_event) = event::read()? {
                return Ok(Some(key_event));
            }
        }
        Ok(None)
    }

    /// Get current width
    pub fn width(&self) -> usize {
        self.width as usize
    }

    /// Get current height
    pub fn height(&self) -> usize {
        self.height as usize
    }
}

impl Drop for TerminalDisplay {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
        // Flush buffer before leaving alternate screen
        let _ = self.buffer.flush();
        let _ = execute!(stdout(), LeaveAlternateScreen);
    }
}

/// Key actions for the raytracer
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Action {
    None,
    Quit,
    LightUp,
    LightDown,
    BouncesUp,
    BouncesDown,
    Reset,
    Pause,
    CameraForward,
    CameraBack,
}

/// Parse keyboard input into actions
pub fn parse_key_event(event: KeyEvent) -> Action {
    match event.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Up => Action::LightUp,
        KeyCode::Down => Action::LightDown,
        KeyCode::Right => Action::BouncesUp,
        KeyCode::Left => Action::BouncesDown,
        KeyCode::Char('r') => Action::Reset,
        KeyCode::Char(' ') => Action::Pause,
        KeyCode::Char('[') => Action::CameraBack,
        KeyCode::Char(']') => Action::CameraForward,
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn test_parse_key_event_quit() {
        let event = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::Quit);
    }

    #[test]
    fn test_parse_key_event_escape() {
        let event = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::Quit);
    }

    #[test]
    fn test_parse_key_event_light_up() {
        let event = KeyEvent::new(KeyCode::Up, KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::LightUp);
    }

    #[test]
    fn test_parse_key_event_light_down() {
        let event = KeyEvent::new(KeyCode::Down, KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::LightDown);
    }

    #[test]
    fn test_parse_key_event_bounces_up() {
        let event = KeyEvent::new(KeyCode::Right, KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::BouncesUp);
    }

    #[test]
    fn test_parse_key_event_bounces_down() {
        let event = KeyEvent::new(KeyCode::Left, KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::BouncesDown);
    }

    #[test]
    fn test_parse_key_event_reset() {
        let event = KeyEvent::new(KeyCode::Char('r'), KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::Reset);
    }

    #[test]
    fn test_parse_key_event_none() {
        let event = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::empty());
        assert_eq!(parse_key_event(event), Action::None);
    }
}
