/// Input mode for user interaction
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    EnteringFilter,
    EnteringLineJump,
    /// Waiting for second key after 'z' (for zz, zt, zb commands)
    ZPending,
    /// Source panel is focused for tree navigation
    SourcePanel,
    /// Waiting for user to confirm tab close
    ConfirmClose,
}

/// Manages text input state: buffer, cursor position, and input mode.
#[derive(Debug)]
pub struct InputController {
    /// Input buffer for filter/line-jump entry
    pub buffer: String,

    /// Cursor position within input buffer (byte offset)
    pub cursor: usize,

    /// Current input mode
    pub mode: InputMode,
}

impl InputController {
    pub fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            mode: InputMode::Normal,
        }
    }

    /// Add a character at cursor position
    pub fn input_char(&mut self, c: char) {
        if self.cursor >= self.buffer.len() {
            self.buffer.push(c);
        } else {
            self.buffer.insert(self.cursor, c);
        }
        self.cursor += c.len_utf8();
    }

    /// Remove the character before the cursor
    pub fn input_backspace(&mut self) {
        if self.cursor > 0 {
            let mut prev_boundary = self.cursor - 1;
            while prev_boundary > 0 && !self.buffer.is_char_boundary(prev_boundary) {
                prev_boundary -= 1;
            }
            self.buffer.remove(prev_boundary);
            self.cursor = prev_boundary;
        }
    }

    /// Move cursor left by one character
    pub fn cursor_left(&mut self) {
        if self.cursor > 0 {
            let mut prev_boundary = self.cursor - 1;
            while prev_boundary > 0 && !self.buffer.is_char_boundary(prev_boundary) {
                prev_boundary -= 1;
            }
            self.cursor = prev_boundary;
        }
    }

    /// Move cursor right by one character
    pub fn cursor_right(&mut self) {
        if self.cursor < self.buffer.len() {
            let mut next_boundary = self.cursor + 1;
            while next_boundary < self.buffer.len() && !self.buffer.is_char_boundary(next_boundary)
            {
                next_boundary += 1;
            }
            self.cursor = next_boundary;
        }
    }

    /// Move cursor to the beginning of input
    pub fn cursor_home(&mut self) {
        self.cursor = 0;
    }

    /// Move cursor to the end of input
    pub fn cursor_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    /// Get the current cursor position
    pub fn get_cursor_position(&self) -> usize {
        self.cursor
    }

    /// Get the current input buffer content
    pub fn get_input(&self) -> &str {
        &self.buffer
    }

    /// Clear the buffer and reset cursor
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.cursor = 0;
    }

    /// Check if currently entering filter input
    pub fn is_entering_filter(&self) -> bool {
        self.mode == InputMode::EnteringFilter
    }

    /// Check if currently entering line jump input
    pub fn is_entering_line_jump(&self) -> bool {
        self.mode == InputMode::EnteringLineJump
    }

    /// Set buffer content and move cursor to end (used by history navigation)
    pub fn set_content(&mut self, content: String) {
        self.buffer = content;
        self.cursor = self.buffer.len();
    }
}
