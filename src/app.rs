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

    /// Should the app quit
    pub should_quit: bool,

    /// Help overlay visible
    pub show_help: bool,

    /// Filter history (up to 50 entries)
    filter_history: Vec<String>,

    /// Current position in filter history (None = not navigating)
    history_index: Option<usize>,

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
            should_quit: false,
            show_help: false,
            filter_history: Vec::new(),
            history_index: None,
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

        // Save current line as filter origin (for restoring on Esc)
        let tab = self.active_tab_mut();
        let current_line = tab.viewport.selected_line();
        tab.filter_origin_line = Some(current_line);
    }

    /// Cancel filter input and return to normal mode
    pub fn cancel_filter_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.history_index = None;
        // Note: filter_origin_line is NOT cleared here - clear_filter() uses it
        // to restore the original position when Esc is pressed
    }

    /// Add filter pattern to history (called on filter submit)
    pub fn add_to_history(&mut self, pattern: String) {
        if pattern.is_empty() {
            return;
        }

        // Don't add if it's the same as the last entry
        if let Some(last) = self.filter_history.last() {
            if last == &pattern {
                return;
            }
        }

        // Add to history
        self.filter_history.push(pattern);

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
            self.input_buffer = self.filter_history[idx].clone();
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
            self.input_buffer = self.filter_history[idx].clone();
        } else {
            // Back to empty
            self.input_buffer.clear();
        }
    }

    /// Add a character to the input buffer
    pub fn input_char(&mut self, c: char) {
        self.input_buffer.push(c);
    }

    /// Remove the last character from the input buffer
    pub fn input_backspace(&mut self) {
        self.input_buffer.pop();
    }

    /// Get the current input buffer content
    pub fn get_input(&self) -> &str {
        &self.input_buffer
    }

    /// Check if currently entering filter input
    pub fn is_entering_filter(&self) -> bool {
        self.input_mode == InputMode::EnteringFilter
    }

    /// Enter line jump input mode
    pub fn start_line_jump_input(&mut self) {
        self.input_mode = InputMode::EnteringLineJump;
        self.input_buffer.clear();
    }

    /// Cancel line jump input and return to normal mode
    pub fn cancel_line_jump_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
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
                self.add_to_history(pattern);
                // Clear origin line - user is committing to the filtered position
                self.active_tab_mut().filter_origin_line = None;
                self.cancel_filter_input();
            }
            AppEvent::FilterInputCancel => self.cancel_filter_input(),
            AppEvent::ClearFilter => self.clear_filter(),

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
        app.add_to_history("ERROR".to_string());
        app.add_to_history("WARN".to_string());
        app.add_to_history("INFO".to_string());

        assert_eq!(app.filter_history.len(), 3);
    }

    #[test]
    fn test_add_to_history_skips_duplicates() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.add_to_history("ERROR".to_string());
        app.add_to_history("ERROR".to_string()); // Duplicate - should not add

        assert_eq!(app.filter_history.len(), 1);
    }

    #[test]
    fn test_history_navigation() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.add_to_history("ERROR".to_string());
        app.add_to_history("WARN".to_string());
        app.add_to_history("INFO".to_string());

        // Start filter input
        app.start_filter_input();

        // Navigate up (most recent)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "INFO");

        // Navigate up again (older)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "WARN");

        // Navigate down
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "INFO");
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
}
