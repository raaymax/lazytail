use crate::filter::{FilterHistoryEntry, FilterMode};
use crate::tab::TabState;
#[cfg(test)]
use std::path::PathBuf;

/// Represents the current view mode
#[derive(Debug, Clone, PartialEq)]
pub enum ViewMode {
    Normal,
    Filtered,
}

/// Filter state tracking
#[derive(Debug, Clone, PartialEq)]
pub enum FilterState {
    Inactive,
    Processing { progress: usize },
    Complete { matches: usize },
}

/// Input mode for user interaction
#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    EnteringFilter,
    EnteringLineJump,
    /// Waiting for second key after 'z' (for zz, zt, zb commands)
    ZPending,
}

/// Main application state
pub struct App {
    /// All open tabs
    pub tabs: Vec<TabState>,

    /// Currently active tab index
    pub active_tab: usize,

    /// Current input mode
    pub input_mode: InputMode,

    /// Input buffer for filter entry
    pub input_buffer: String,

    /// Cursor position within input buffer (byte offset)
    pub input_cursor: usize,

    /// Should the app quit
    pub should_quit: bool,

    /// Help overlay visible
    pub show_help: bool,

    /// Filter history (up to 50 entries)
    filter_history: Vec<FilterHistoryEntry>,

    /// Current position in filter history (None = not navigating)
    history_index: Option<usize>,

    /// Current filter mode for input (Plain or Regex, with case sensitivity)
    pub current_filter_mode: FilterMode,

    /// Regex validation error (None = valid or plain mode, Some = invalid regex)
    pub regex_error: Option<String>,

    /// Side panel width
    pub side_panel_width: u16,
}

impl App {
    #[cfg(test)]
    pub fn new(files: Vec<PathBuf>, watch: bool) -> anyhow::Result<Self> {
        let mut tabs = Vec::new();
        for file in files {
            tabs.push(TabState::new(file, watch)?);
        }

        Ok(Self::with_tabs(tabs))
    }

    /// Create an App with pre-created tabs
    pub fn with_tabs(tabs: Vec<TabState>) -> Self {
        Self {
            tabs,
            active_tab: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            should_quit: false,
            show_help: false,
            filter_history: Vec::new(),
            history_index: None,
            current_filter_mode: FilterMode::default(),
            regex_error: None,
            side_panel_width: 32,
        }
    }

    /// Get a reference to the active tab
    pub fn active_tab(&self) -> &TabState {
        &self.tabs[self.active_tab]
    }

    /// Get a mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> &mut TabState {
        &mut self.tabs[self.active_tab]
    }

    /// Switch to the next tab
    pub fn next_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = (self.active_tab + 1) % self.tabs.len();
        }
    }

    /// Switch to the previous tab
    pub fn prev_tab(&mut self) {
        if !self.tabs.is_empty() {
            self.active_tab = if self.active_tab == 0 {
                self.tabs.len() - 1
            } else {
                self.active_tab - 1
            };
        }
    }

    /// Switch to a specific tab by index
    pub fn select_tab(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_tab = index;
        }
    }

    /// Get the number of tabs
    pub fn tab_count(&self) -> usize {
        self.tabs.len()
    }

    // === Delegated methods for backward compatibility ===

    /// Scroll down by one line
    pub fn scroll_down(&mut self) {
        self.active_tab_mut().scroll_down();
    }

    /// Scroll up by one line
    pub fn scroll_up(&mut self) {
        self.active_tab_mut().scroll_up();
    }

    /// Scroll down by page
    pub fn page_down(&mut self, page_size: usize) {
        self.active_tab_mut().page_down(page_size);
    }

    /// Scroll up by page
    pub fn page_up(&mut self, page_size: usize) {
        self.active_tab_mut().page_up(page_size);
    }

    /// Mouse scroll down
    pub fn mouse_scroll_down(&mut self, lines: usize, visible_height: usize) {
        self.active_tab_mut()
            .mouse_scroll_down(lines, visible_height);
    }

    /// Mouse scroll up
    pub fn mouse_scroll_up(&mut self, lines: usize, visible_height: usize) {
        self.active_tab_mut().mouse_scroll_up(lines, visible_height);
    }

    /// Apply filter results
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        self.active_tab_mut()
            .apply_filter(matching_indices, pattern);
    }

    /// Append incremental filter results
    pub fn append_filter_results(&mut self, new_matching_indices: Vec<usize>) {
        self.active_tab_mut()
            .append_filter_results(new_matching_indices);
    }

    /// Clear filter
    pub fn clear_filter(&mut self) {
        self.active_tab_mut().clear_filter();
    }

    /// Enter filter input mode
    pub fn start_filter_input(&mut self) {
        self.input_mode = InputMode::EnteringFilter;
        self.input_buffer.clear();
        self.input_cursor = 0;

        // Save current line as filter origin (for restoring on Esc)
        let tab = self.active_tab_mut();
        let current_line = tab.viewport.selected_line();
        tab.filter_origin_line = Some(current_line);
    }

    /// Cancel filter input and return to normal mode
    pub fn cancel_filter_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.input_cursor = 0;
        self.history_index = None;
        // Note: filter_origin_line is NOT cleared here - clear_filter() uses it
        // to restore the original position when Esc is pressed
    }

    /// Add filter pattern to history (called on filter submit)
    pub fn add_to_history(&mut self, pattern: String, mode: FilterMode) {
        if pattern.is_empty() {
            return;
        }

        let entry = FilterHistoryEntry::new(pattern, mode);

        // Don't add if it's the same as the last entry (same pattern AND mode)
        if let Some(last) = self.filter_history.last() {
            if last.matches(&entry) {
                return;
            }
        }

        // Add to history
        self.filter_history.push(entry);

        // Limit history to 50 entries
        if self.filter_history.len() > 50 {
            self.filter_history.remove(0);
        }

        // Reset history navigation
        self.history_index = None;
    }

    /// Navigate up in filter history (older entries)
    pub fn history_up(&mut self) {
        if self.filter_history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            None => {
                // First time navigating - save current input and go to most recent
                Some(self.filter_history.len() - 1)
            }
            Some(idx) => {
                // Already navigating - go to older entry
                if idx > 0 {
                    Some(idx - 1)
                } else {
                    Some(idx) // At oldest, stay there
                }
            }
        };

        self.history_index = new_index;
        if let Some(idx) = new_index {
            let entry = &self.filter_history[idx];
            self.input_buffer = entry.pattern.clone();
            self.input_cursor = self.input_buffer.len();
            self.current_filter_mode = entry.mode;
            self.validate_regex();
        }
    }

    /// Navigate down in filter history (newer entries)
    pub fn history_down(&mut self) {
        if self.filter_history.is_empty() {
            return;
        }

        let new_index = match self.history_index {
            None => None, // Not navigating, do nothing
            Some(idx) => {
                if idx < self.filter_history.len() - 1 {
                    Some(idx + 1)
                } else {
                    // At newest entry, go back to empty input
                    None
                }
            }
        };

        self.history_index = new_index;
        if let Some(idx) = new_index {
            let entry = &self.filter_history[idx];
            self.input_buffer = entry.pattern.clone();
            self.input_cursor = self.input_buffer.len();
            self.current_filter_mode = entry.mode;
            self.validate_regex();
        } else {
            // Back to empty
            self.input_buffer.clear();
            self.input_cursor = 0;
            // Keep current mode when clearing (don't reset)
            self.validate_regex();
        }
    }

    /// Add a character to the input buffer at cursor position
    pub fn input_char(&mut self, c: char) {
        if self.input_cursor >= self.input_buffer.len() {
            // Cursor at end - append
            self.input_buffer.push(c);
        } else {
            // Insert at cursor position
            self.input_buffer.insert(self.input_cursor, c);
        }
        self.input_cursor += c.len_utf8();
        self.validate_regex();
    }

    /// Remove the character before the cursor
    pub fn input_backspace(&mut self) {
        if self.input_cursor > 0 {
            // Find the character boundary before cursor
            let mut prev_boundary = self.input_cursor - 1;
            while prev_boundary > 0 && !self.input_buffer.is_char_boundary(prev_boundary) {
                prev_boundary -= 1;
            }
            self.input_buffer.remove(prev_boundary);
            self.input_cursor = prev_boundary;
        }
        self.validate_regex();
    }

    /// Move cursor left by one character
    pub fn cursor_left(&mut self) {
        if self.input_cursor > 0 {
            // Find the previous character boundary
            let mut prev_boundary = self.input_cursor - 1;
            while prev_boundary > 0 && !self.input_buffer.is_char_boundary(prev_boundary) {
                prev_boundary -= 1;
            }
            self.input_cursor = prev_boundary;
        }
    }

    /// Move cursor right by one character
    pub fn cursor_right(&mut self) {
        if self.input_cursor < self.input_buffer.len() {
            // Find the next character boundary
            let mut next_boundary = self.input_cursor + 1;
            while next_boundary < self.input_buffer.len()
                && !self.input_buffer.is_char_boundary(next_boundary)
            {
                next_boundary += 1;
            }
            self.input_cursor = next_boundary;
        }
    }

    /// Move cursor to the beginning of input
    pub fn cursor_home(&mut self) {
        self.input_cursor = 0;
    }

    /// Move cursor to the end of input
    pub fn cursor_end(&mut self) {
        self.input_cursor = self.input_buffer.len();
    }

    /// Get the current cursor position
    pub fn get_cursor_position(&self) -> usize {
        self.input_cursor
    }

    /// Get the current input buffer content
    pub fn get_input(&self) -> &str {
        &self.input_buffer
    }

    /// Validate the current input as a regex (if in regex mode)
    /// Sets regex_error to None if valid, Some(error) if invalid
    pub fn validate_regex(&mut self) {
        if !self.current_filter_mode.is_regex() || self.input_buffer.is_empty() {
            self.regex_error = None;
            return;
        }

        match regex::Regex::new(&self.input_buffer) {
            Ok(_) => self.regex_error = None,
            Err(e) => self.regex_error = Some(e.to_string()),
        }
    }

    /// Check if the current regex input is valid
    pub fn is_regex_valid(&self) -> bool {
        self.regex_error.is_none()
    }

    /// Check if currently entering filter input
    pub fn is_entering_filter(&self) -> bool {
        self.input_mode == InputMode::EnteringFilter
    }

    /// Enter line jump input mode
    pub fn start_line_jump_input(&mut self) {
        self.input_mode = InputMode::EnteringLineJump;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Cancel line jump input and return to normal mode
    pub fn cancel_line_jump_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.input_cursor = 0;
    }

    /// Check if currently entering line jump input
    pub fn is_entering_line_jump(&self) -> bool {
        self.input_mode == InputMode::EnteringLineJump
    }

    /// Jump to a specific line number
    pub fn jump_to_line(&mut self, line_number: usize) {
        self.active_tab_mut().jump_to_line(line_number);
    }

    /// Toggle follow mode
    pub fn toggle_follow_mode(&mut self) {
        self.active_tab_mut().toggle_follow_mode();
    }

    /// Jump to the end of the log
    pub fn jump_to_end(&mut self) {
        self.active_tab_mut().jump_to_end();
    }

    /// Jump to the beginning of the log
    pub fn jump_to_start(&mut self) {
        self.active_tab_mut().jump_to_start();
    }

    /// Apply an event to the application state
    /// This is the central event handler that modifies app state based on events
    pub fn apply_event(&mut self, event: crate::event::AppEvent) {
        use crate::event::AppEvent;

        match event {
            // Navigation events - delegate to active tab
            AppEvent::ScrollDown => self.scroll_down(),
            AppEvent::ScrollUp => self.scroll_up(),
            AppEvent::PageDown(page_size) => self.page_down(page_size),
            AppEvent::PageUp(page_size) => self.page_up(page_size),
            AppEvent::JumpToStart => self.jump_to_start(),
            AppEvent::JumpToEnd => self.jump_to_end(),
            AppEvent::MouseScrollDown(_lines) => {
                // Mouse scroll events will be handled in main loop with visible_height
            }
            AppEvent::MouseScrollUp(_lines) => {
                // Mouse scroll events will be handled in main loop with visible_height
            }

            // Tab navigation events
            AppEvent::NextTab => self.next_tab(),
            AppEvent::PrevTab => self.prev_tab(),
            AppEvent::SelectTab(index) => self.select_tab(index),

            // Filter input events
            AppEvent::StartFilterInput => self.start_filter_input(),
            AppEvent::FilterInputChar(c) => self.input_char(c),
            AppEvent::FilterInputBackspace => self.input_backspace(),
            AppEvent::FilterInputSubmit => {
                // Save current filter to history before closing
                let pattern = self.input_buffer.clone();
                let mode = self.current_filter_mode;
                self.add_to_history(pattern, mode);
                // Clear origin line - user is committing to the filtered position
                self.active_tab_mut().filter_origin_line = None;
                self.cancel_filter_input();
            }
            AppEvent::FilterInputCancel => self.cancel_filter_input(),
            AppEvent::ClearFilter => self.clear_filter(),
            AppEvent::ToggleFilterMode => {
                self.current_filter_mode.toggle_mode();
                self.validate_regex();
            }
            AppEvent::ToggleCaseSensitivity => self.current_filter_mode.toggle_case_sensitivity(),
            AppEvent::CursorLeft => self.cursor_left(),
            AppEvent::CursorRight => self.cursor_right(),
            AppEvent::CursorHome => self.cursor_home(),
            AppEvent::CursorEnd => self.cursor_end(),

            // Filter progress events
            AppEvent::FilterProgress(lines_processed) => {
                self.active_tab_mut().filter_state = FilterState::Processing {
                    progress: lines_processed,
                };
            }
            AppEvent::FilterComplete {
                indices,
                incremental,
            } => {
                if incremental {
                    self.append_filter_results(indices);
                } else {
                    let pattern = self.active_tab().filter_pattern.clone().unwrap_or_default();
                    self.apply_filter(indices, pattern);
                }
                // Follow mode jump will be handled separately in main loop
            }
            AppEvent::FilterError(err) => {
                eprintln!("Filter error: {}", err);
                self.active_tab_mut().filter_state = FilterState::Inactive;
            }

            // File events
            AppEvent::FileModified {
                new_total,
                old_total: _,
            } => {
                let tab = self.active_tab_mut();
                tab.total_lines = new_total;
                if tab.mode == ViewMode::Normal {
                    tab.line_indices = (0..new_total).collect();
                }
                // Incremental filter will be handled by StartFilter event
            }
            AppEvent::FileTruncated { new_total } => {
                let tab = self.active_tab_mut();
                eprintln!("File truncated: {} -> {} lines", tab.total_lines, new_total);
                // Reset state on truncation
                tab.total_lines = new_total;
                tab.line_indices = (0..new_total).collect();
                tab.mode = ViewMode::Normal;
                tab.filter_pattern = None;
                tab.filter_state = FilterState::Inactive;
                tab.last_filtered_line = 0;
                // Ensure selection is valid
                if tab.selected_line >= new_total && new_total > 0 {
                    tab.selected_line = new_total - 1;
                } else if new_total == 0 {
                    tab.selected_line = 0;
                }
            }

            // Mode toggles
            AppEvent::ToggleFollowMode => self.toggle_follow_mode(),
            AppEvent::DisableFollowMode => {
                self.active_tab_mut().follow_mode = false;
            }

            // System events
            AppEvent::Quit => {
                self.should_quit = true;
            }

            // Help mode
            AppEvent::ShowHelp => {
                self.show_help = true;
            }
            AppEvent::HideHelp => {
                self.show_help = false;
            }

            // Line jump events
            AppEvent::StartLineJumpInput => self.start_line_jump_input(),
            AppEvent::LineJumpInputChar(c) => {
                // Only allow digits in line jump input
                if c.is_ascii_digit() {
                    self.input_char(c);
                }
            }
            AppEvent::LineJumpInputBackspace => self.input_backspace(),
            AppEvent::LineJumpInputSubmit => {
                // Parse the input and jump to the line
                if let Ok(line_num) = self.input_buffer.parse::<usize>() {
                    self.jump_to_line(line_num);
                    // Disable follow mode when explicitly jumping to a line
                    self.active_tab_mut().follow_mode = false;
                }
                self.cancel_line_jump_input();
            }
            AppEvent::LineJumpInputCancel => self.cancel_line_jump_input(),

            // Filter history navigation
            AppEvent::HistoryUp => self.history_up(),
            AppEvent::HistoryDown => self.history_down(),

            // View positioning (vim z commands)
            AppEvent::EnterZMode => {
                self.input_mode = InputMode::ZPending;
            }
            AppEvent::ExitZMode => {
                self.input_mode = InputMode::Normal;
            }
            AppEvent::CenterView => {
                self.active_tab_mut().center_view();
            }
            AppEvent::ViewToTop => {
                self.active_tab_mut().view_to_top();
            }
            AppEvent::ViewToBottom => {
                self.active_tab_mut().view_to_bottom();
            }

            // Future events - not yet implemented
            AppEvent::StartFilter { .. } => {
                // Will be handled in main loop to trigger background filter
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_log_file(lines: &[&str]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_app_initialization() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.active_tab, 0);
        assert_eq!(app.active_tab().total_lines, 3);
        assert!(!app.should_quit);
        assert!(!app.show_help);
    }

    #[test]
    fn test_multiple_tabs() {
        let file1 = create_temp_log_file(&["line1", "line2"]);
        let file2 = create_temp_log_file(&["a", "b", "c"]);
        let file3 = create_temp_log_file(&["x"]);

        let app = App::new(
            vec![
                file1.path().to_path_buf(),
                file2.path().to_path_buf(),
                file3.path().to_path_buf(),
            ],
            false,
        )
        .unwrap();

        assert_eq!(app.tabs.len(), 3);
        assert_eq!(app.tabs[0].total_lines, 2);
        assert_eq!(app.tabs[1].total_lines, 3);
        assert_eq!(app.tabs[2].total_lines, 1);
    }

    #[test]
    fn test_tab_navigation() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);
        let file3 = create_temp_log_file(&["c"]);

        let mut app = App::new(
            vec![
                file1.path().to_path_buf(),
                file2.path().to_path_buf(),
                file3.path().to_path_buf(),
            ],
            false,
        )
        .unwrap();

        assert_eq!(app.active_tab, 0);

        app.next_tab();
        assert_eq!(app.active_tab, 1);

        app.next_tab();
        assert_eq!(app.active_tab, 2);

        // Wrap around
        app.next_tab();
        assert_eq!(app.active_tab, 0);

        // Previous tab
        app.prev_tab();
        assert_eq!(app.active_tab, 2);

        // Direct selection
        app.select_tab(1);
        assert_eq!(app.active_tab, 1);

        // Invalid selection (out of bounds)
        app.select_tab(10);
        assert_eq!(app.active_tab, 1); // Unchanged
    }

    #[test]
    fn test_per_tab_state_isolation() {
        let file1 = create_temp_log_file(&["error", "info", "error"]);
        let file2 = create_temp_log_file(&["debug", "warn"]);

        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        // Apply filter to tab 0
        app.apply_filter(vec![0, 2], "error".to_string());
        assert_eq!(app.active_tab().mode, ViewMode::Filtered);

        // Switch to tab 1
        app.next_tab();
        assert_eq!(app.active_tab().mode, ViewMode::Normal);
        assert!(app.active_tab().filter_pattern.is_none());

        // Tab 0 should still be filtered
        app.prev_tab();
        assert_eq!(app.active_tab().mode, ViewMode::Filtered);
    }

    #[test]
    fn test_navigation_basic() {
        let temp_file = create_temp_log_file(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Scroll down
        app.scroll_down();
        assert_eq!(app.active_tab().selected_line, 1);

        // Scroll up
        app.scroll_up();
        assert_eq!(app.active_tab().selected_line, 0);

        // Can't scroll below 0
        app.scroll_up();
        assert_eq!(app.active_tab().selected_line, 0);

        // Jump to end
        app.jump_to_end();
        assert_eq!(app.active_tab().selected_line, 9);
    }

    #[test]
    fn test_page_navigation() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Page down
        app.page_down(10);
        assert_eq!(app.active_tab().selected_line, 10);

        // Page up
        app.page_up(5);
        assert_eq!(app.active_tab().selected_line, 5);
    }

    #[test]
    fn test_filter_application() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        let matching_indices = vec![0, 2];

        app.apply_filter(matching_indices.clone(), "error".to_string());

        assert_eq!(app.active_tab().mode, ViewMode::Filtered);
        assert_eq!(app.active_tab().line_indices, matching_indices);
        assert_eq!(app.active_tab().filter_pattern, Some("error".to_string()));
    }

    #[test]
    fn test_clear_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_filter(vec![0, 2], "error".to_string());
        app.clear_filter();

        assert_eq!(app.active_tab().mode, ViewMode::Normal);
        assert!(app.active_tab().filter_pattern.is_none());
    }

    #[test]
    fn test_follow_mode_toggle() {
        let temp_file = create_temp_log_file(&["1", "2", "3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert!(!app.active_tab().follow_mode);

        app.toggle_follow_mode();
        assert!(app.active_tab().follow_mode);

        app.toggle_follow_mode();
        assert!(!app.active_tab().follow_mode);
    }

    #[test]
    fn test_filter_input_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Enter filter mode
        app.start_filter_input();
        assert!(app.is_entering_filter());
        assert!(app.get_input().is_empty());

        // Type some input
        app.input_char('t');
        app.input_char('e');
        app.input_char('s');
        app.input_char('t');
        assert_eq!(app.get_input(), "test");

        // Backspace
        app.input_backspace();
        assert_eq!(app.get_input(), "tes");

        // Cancel input
        app.cancel_filter_input();
        assert!(!app.is_entering_filter());
        assert!(app.get_input().is_empty());
    }

    #[test]
    fn test_help_mode_toggle() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        assert!(!app.show_help);

        // Show help
        app.apply_event(AppEvent::ShowHelp);
        assert!(app.show_help);

        // Hide help
        app.apply_event(AppEvent::HideHelp);
        assert!(!app.show_help);
    }

    #[test]
    fn test_line_jump_input_mode() {
        let temp_file = create_temp_log_file(&["1", "2", "3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Start line jump input
        app.start_line_jump_input();
        assert!(app.is_entering_line_jump());
        assert_eq!(app.get_input(), "");

        // Cancel line jump input
        app.cancel_line_jump_input();
        assert!(!app.is_entering_line_jump());
        assert_eq!(app.get_input(), "");
    }

    #[test]
    fn test_add_to_history() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Add patterns to history
        app.add_to_history("ERROR".to_string(), FilterMode::plain());
        app.add_to_history("WARN".to_string(), FilterMode::plain());
        app.add_to_history("INFO".to_string(), FilterMode::plain());

        assert_eq!(app.filter_history.len(), 3);
    }

    #[test]
    fn test_add_to_history_skips_duplicates() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.add_to_history("ERROR".to_string(), FilterMode::plain());
        app.add_to_history("ERROR".to_string(), FilterMode::plain()); // Duplicate - should not add

        assert_eq!(app.filter_history.len(), 1);
    }

    #[test]
    fn test_add_to_history_same_pattern_different_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.add_to_history("error".to_string(), FilterMode::plain());
        app.add_to_history("error".to_string(), FilterMode::regex()); // Different mode - should add

        assert_eq!(app.filter_history.len(), 2);
        assert!(!app.filter_history[0].mode.is_regex());
        assert!(app.filter_history[1].mode.is_regex());
    }

    #[test]
    fn test_history_navigation() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.add_to_history("ERROR".to_string(), FilterMode::plain());
        app.add_to_history("WARN".to_string(), FilterMode::regex());
        app.add_to_history("INFO".to_string(), FilterMode::plain());

        // Start filter input
        app.start_filter_input();

        // Navigate up (most recent - INFO with plain mode)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "INFO");
        assert!(!app.current_filter_mode.is_regex());

        // Navigate up again (older - WARN with regex mode)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "WARN");
        assert!(app.current_filter_mode.is_regex());

        // Navigate down (back to INFO with plain mode)
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "INFO");
        assert!(!app.current_filter_mode.is_regex());
    }

    #[test]
    fn test_history_navigation_restores_case_sensitivity() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Add entries with different case sensitivity settings
        app.add_to_history(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: false,
            },
        );
        app.add_to_history(
            "Error".to_string(),
            FilterMode::Plain {
                case_sensitive: true,
            },
        );

        app.start_filter_input();

        // Navigate to most recent (case sensitive)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "Error");
        assert!(app.current_filter_mode.is_case_sensitive());

        // Navigate to older (case insensitive)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "error");
        assert!(!app.current_filter_mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_filter_mode() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Default is plain mode
        assert!(!app.current_filter_mode.is_regex());

        // Toggle to regex
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.current_filter_mode.is_regex());

        // Toggle back to plain
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.current_filter_mode.is_regex());
    }

    #[test]
    fn test_toggle_case_sensitivity() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Default is case insensitive
        assert!(!app.current_filter_mode.is_case_sensitive());

        // Toggle to case sensitive
        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(app.current_filter_mode.is_case_sensitive());

        // Toggle back to case insensitive
        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(!app.current_filter_mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_preserves_case_sensitivity() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Set case sensitive
        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(app.current_filter_mode.is_case_sensitive());
        assert!(!app.current_filter_mode.is_regex());

        // Toggle to regex - should preserve case sensitivity
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.current_filter_mode.is_regex());
        assert!(app.current_filter_mode.is_case_sensitive());

        // Toggle back to plain - should still be case sensitive
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.current_filter_mode.is_regex());
        assert!(app.current_filter_mode.is_case_sensitive());
    }

    #[test]
    fn test_regex_validation_valid_regex() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Switch to regex mode
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.current_filter_mode.is_regex());

        // Type a valid regex
        app.apply_event(AppEvent::FilterInputChar('e'));
        app.apply_event(AppEvent::FilterInputChar('r'));
        app.apply_event(AppEvent::FilterInputChar('r'));
        app.apply_event(AppEvent::FilterInputChar('.'));
        app.apply_event(AppEvent::FilterInputChar('*'));

        assert!(app.is_regex_valid());
        assert!(app.regex_error.is_none());
    }

    #[test]
    fn test_regex_validation_invalid_regex() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Switch to regex mode
        app.apply_event(AppEvent::ToggleFilterMode);

        // Type an invalid regex (unclosed bracket)
        app.apply_event(AppEvent::FilterInputChar('['));
        app.apply_event(AppEvent::FilterInputChar('a'));

        assert!(!app.is_regex_valid());
        assert!(app.regex_error.is_some());
    }

    #[test]
    fn test_regex_validation_clears_on_backspace() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Switch to regex mode and type invalid regex
        app.apply_event(AppEvent::ToggleFilterMode);
        app.apply_event(AppEvent::FilterInputChar('['));
        assert!(!app.is_regex_valid());

        // Backspace to remove the bracket
        app.apply_event(AppEvent::FilterInputBackspace);
        assert!(app.is_regex_valid());
    }

    #[test]
    fn test_regex_validation_not_checked_in_plain_mode() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Stay in plain mode (default)
        assert!(!app.current_filter_mode.is_regex());

        // Type what would be an invalid regex
        app.apply_event(AppEvent::FilterInputChar('['));
        app.apply_event(AppEvent::FilterInputChar('a'));

        // Should still be valid (not checked in plain mode)
        assert!(app.is_regex_valid());
        assert!(app.regex_error.is_none());
    }

    #[test]
    fn test_regex_validation_on_mode_toggle() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Type invalid regex in plain mode (not validated)
        app.apply_event(AppEvent::FilterInputChar('['));
        assert!(app.is_regex_valid());

        // Switch to regex mode - now it should be invalid
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.is_regex_valid());
        assert!(app.regex_error.is_some());

        // Switch back to plain mode - should be valid again
        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.is_regex_valid());
    }

    #[test]
    fn test_tab_events() {
        use crate::event::AppEvent;

        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);

        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        assert_eq!(app.active_tab, 0);

        app.apply_event(AppEvent::NextTab);
        assert_eq!(app.active_tab, 1);

        app.apply_event(AppEvent::PrevTab);
        assert_eq!(app.active_tab, 0);

        app.apply_event(AppEvent::SelectTab(1));
        assert_eq!(app.active_tab, 1);
    }

    #[test]
    fn test_cursor_starts_at_zero() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        assert_eq!(app.get_cursor_position(), 0);
    }

    #[test]
    fn test_cursor_moves_with_input() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        assert_eq!(app.get_cursor_position(), 1);
        assert_eq!(app.get_input(), "a");

        app.apply_event(AppEvent::FilterInputChar('b'));
        assert_eq!(app.get_cursor_position(), 2);
        assert_eq!(app.get_input(), "ab");
    }

    #[test]
    fn test_cursor_left_right() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));
        assert_eq!(app.get_cursor_position(), 3);

        // Move left
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 2);

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 1);

        // Move right
        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);
    }

    #[test]
    fn test_cursor_at_boundaries() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));

        // Move left to start
        app.apply_event(AppEvent::CursorLeft);
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 0);

        // Try to move left past start (should stay at 0)
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 0);

        // Move right to end
        app.apply_event(AppEvent::CursorRight);
        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);

        // Try to move right past end (should stay at end)
        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);
    }

    #[test]
    fn test_cursor_home_end() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));

        // Move cursor to middle
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 2);

        // Home goes to start
        app.apply_event(AppEvent::CursorHome);
        assert_eq!(app.get_cursor_position(), 0);

        // End goes to end
        app.apply_event(AppEvent::CursorEnd);
        assert_eq!(app.get_cursor_position(), 3);
    }

    #[test]
    fn test_insert_at_cursor() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('c'));
        // Cursor is at end: "ac|"

        // Move cursor left
        app.apply_event(AppEvent::CursorLeft);
        // Cursor is now: "a|c"

        // Insert 'b' at cursor
        app.apply_event(AppEvent::FilterInputChar('b'));
        assert_eq!(app.get_input(), "abc");
        assert_eq!(app.get_cursor_position(), 2); // "ab|c"
    }

    #[test]
    fn test_backspace_at_cursor() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));
        // "abc|"

        // Move cursor to middle
        app.apply_event(AppEvent::CursorLeft);
        // "ab|c"

        // Backspace removes 'b'
        app.apply_event(AppEvent::FilterInputBackspace);
        assert_eq!(app.get_input(), "ac");
        assert_eq!(app.get_cursor_position(), 1); // "a|c"
    }

    #[test]
    fn test_backspace_at_start() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));

        // Move to start
        app.apply_event(AppEvent::CursorHome);
        assert_eq!(app.get_cursor_position(), 0);

        // Backspace at start does nothing
        app.apply_event(AppEvent::FilterInputBackspace);
        assert_eq!(app.get_input(), "a");
        assert_eq!(app.get_cursor_position(), 0);
    }

    #[test]
    fn test_cursor_with_unicode() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        // Type "日本" (Japanese characters, 3 bytes each in UTF-8)
        app.apply_event(AppEvent::FilterInputChar('日'));
        app.apply_event(AppEvent::FilterInputChar('本'));
        assert_eq!(app.get_input(), "日本");
        assert_eq!(app.get_cursor_position(), 6); // 3 bytes * 2 chars

        // Move left one character
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 3); // After first character

        // Insert at cursor
        app.apply_event(AppEvent::FilterInputChar('語'));
        assert_eq!(app.get_input(), "日語本");
    }

    #[test]
    fn test_history_sets_cursor_to_end() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Add to history
        app.add_to_history("error".to_string(), FilterMode::plain());

        // Start filter input
        app.start_filter_input();
        assert_eq!(app.get_cursor_position(), 0);

        // Navigate up to history
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "error");
        assert_eq!(app.get_cursor_position(), 5); // Cursor at end
    }
}
