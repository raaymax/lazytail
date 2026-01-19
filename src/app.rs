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
    /// Current view mode
    pub mode: ViewMode,

    /// Current input mode
    pub input_mode: InputMode,

    /// Input buffer for filter entry
    pub input_buffer: String,

    /// Total number of lines in the source
    pub total_lines: usize,

    /// Indices of lines to display (all lines or filtered results)
    pub line_indices: Vec<usize>,

    /// Current scroll position (index into line_indices)
    pub scroll_position: usize,

    /// Currently selected line (index into line_indices)
    pub selected_line: usize,

    /// Current filter state
    pub filter_state: FilterState,

    /// Should the app quit
    pub should_quit: bool,

    /// Current filter pattern (if any)
    pub filter_pattern: Option<String>,

    /// Follow mode - auto-scroll to latest logs
    pub follow_mode: bool,

    /// Last line number that was filtered (for incremental filtering)
    pub last_filtered_line: usize,

    /// Help overlay visible
    pub show_help: bool,

    /// Skip scroll adjustment on next render (set by mouse scroll)
    skip_scroll_adjustment: bool,

    /// Filter history (up to 50 entries)
    filter_history: Vec<String>,

    /// Current position in filter history (None = not navigating)
    history_index: Option<usize>,
}

impl App {
    pub fn new(total_lines: usize) -> Self {
        let line_indices = (0..total_lines).collect();

        Self {
            mode: ViewMode::Normal,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            total_lines,
            line_indices,
            scroll_position: 0,
            selected_line: 0,
            filter_state: FilterState::Inactive,
            should_quit: false,
            filter_pattern: None,
            follow_mode: false,
            last_filtered_line: 0,
            show_help: false,
            skip_scroll_adjustment: false,
            filter_history: Vec::new(),
            history_index: None,
        }
    }

    /// Get the number of visible lines
    pub fn visible_line_count(&self) -> usize {
        self.line_indices.len()
    }

    /// Scroll down by one line
    pub fn scroll_down(&mut self) {
        if self.selected_line < self.line_indices.len().saturating_sub(1) {
            self.selected_line += 1;
        }
    }

    /// Scroll up by one line
    pub fn scroll_up(&mut self) {
        if self.selected_line > 0 {
            self.selected_line -= 1;
        }
    }

    /// Ensure the selected line is visible in the viewport
    pub fn adjust_scroll(&mut self, viewport_height: usize) {
        // Skip adjustment if mouse scroll just happened (prevents interference)
        if self.skip_scroll_adjustment {
            self.skip_scroll_adjustment = false;
            return;
        }

        // Add some padding at the edges for better UX
        let padding = 3.min(viewport_height / 4);

        // If selection is above viewport, scroll up
        if self.selected_line < self.scroll_position + padding {
            self.scroll_position = self.selected_line.saturating_sub(padding);
        }
        // If selection is below viewport, scroll down
        else if self.selected_line >= self.scroll_position + viewport_height - padding {
            self.scroll_position = self.selected_line + padding + 1 - viewport_height;
        }

        // Ensure scroll position is valid
        let max_scroll = self.line_indices.len().saturating_sub(viewport_height);
        self.scroll_position = self.scroll_position.min(max_scroll);
    }

    /// Scroll down by page
    pub fn page_down(&mut self, page_size: usize) {
        self.selected_line =
            (self.selected_line + page_size).min(self.line_indices.len().saturating_sub(1));
    }

    /// Scroll up by page
    pub fn page_up(&mut self, page_size: usize) {
        self.selected_line = self.selected_line.saturating_sub(page_size);
    }

    /// Mouse scroll down - moves viewport and selection together
    pub fn mouse_scroll_down(&mut self, lines: usize, visible_height: usize) {
        let max_scroll = self.line_indices.len().saturating_sub(visible_height);
        let old_scroll = self.scroll_position;
        self.scroll_position = (self.scroll_position + lines).min(max_scroll);

        // Move selection by the same amount the viewport moved
        let actual_scroll = self.scroll_position - old_scroll;
        if actual_scroll > 0 {
            let max_selection = self.line_indices.len().saturating_sub(1);
            self.selected_line = (self.selected_line + actual_scroll).min(max_selection);
        }

        // Skip scroll adjustment on next render to prevent padding interference
        self.skip_scroll_adjustment = true;
    }

    /// Mouse scroll up - moves viewport and selection together
    pub fn mouse_scroll_up(&mut self, lines: usize, _visible_height: usize) {
        let old_scroll = self.scroll_position;
        self.scroll_position = self.scroll_position.saturating_sub(lines);

        // Move selection by the same amount the viewport moved
        let actual_scroll = old_scroll - self.scroll_position;
        if actual_scroll > 0 {
            self.selected_line = self.selected_line.saturating_sub(actual_scroll);
        }

        // Skip scroll adjustment on next render to prevent padding interference
        self.skip_scroll_adjustment = true;
    }

    /// Apply filter results (for full filtering)
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        let was_filtered = self.mode == ViewMode::Filtered;
        // Remember which actual line was selected before changing filter
        let actual_line_number = self.line_indices.get(self.selected_line).copied();

        self.line_indices = matching_indices;
        self.mode = ViewMode::Filtered;
        self.filter_pattern = Some(pattern);
        self.filter_state = FilterState::Complete {
            matches: self.line_indices.len(),
        };
        self.last_filtered_line = self.total_lines;

        // Preserve selection when updating an existing filter (unless follow mode will handle it)
        if was_filtered && !self.follow_mode {
            // Try to keep selection on the same actual line
            if let Some(line_num) = actual_line_number {
                // Find where this line is in the new filtered results
                if let Some(new_index) = self.line_indices.iter().position(|&l| l == line_num) {
                    self.selected_line = new_index;
                } else {
                    // Line not in new results, try to keep similar position
                    self.selected_line = self
                        .selected_line
                        .min(self.line_indices.len().saturating_sub(1));
                }
            } else {
                self.selected_line = 0;
            }
            // Don't reset scroll_position - let adjust_scroll handle it based on the preserved selection
        } else if !self.follow_mode {
            // New filter - start at the top
            self.selected_line = 0;
            self.scroll_position = 0;
        }
        // If follow mode is active, don't set selection or scroll here - let follow mode handle it
    }

    /// Append incremental filter results (for new logs only)
    pub fn append_filter_results(&mut self, new_matching_indices: Vec<usize>) {
        self.line_indices.extend(new_matching_indices);
        self.filter_state = FilterState::Complete {
            matches: self.line_indices.len(),
        };
        self.last_filtered_line = self.total_lines;
        // Don't change selection - let follow mode or user control it
    }

    /// Clear filter and return to normal view
    pub fn clear_filter(&mut self) {
        // Remember which actual line was selected before clearing filter
        let actual_line_number = self.line_indices.get(self.selected_line).copied();

        self.line_indices = (0..self.total_lines).collect();
        self.mode = ViewMode::Normal;

        // Restore selection to the same actual line number
        if let Some(line_num) = actual_line_number {
            self.selected_line = line_num.min(self.total_lines.saturating_sub(1));
        } else {
            self.selected_line = 0;
        }

        // Don't reset scroll_position - let adjust_scroll handle it
        self.filter_pattern = None;
        self.filter_state = FilterState::Inactive;
    }

    /// Enter filter input mode
    pub fn start_filter_input(&mut self) {
        self.input_mode = InputMode::EnteringFilter;
        self.input_buffer.clear();
    }

    /// Cancel filter input and return to normal mode
    pub fn cancel_filter_input(&mut self) {
        self.input_mode = InputMode::Normal;
        self.input_buffer.clear();
        self.history_index = None;
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

    /// Jump to a specific line number (1-indexed)
    pub fn jump_to_line(&mut self, line_number: usize) {
        if line_number == 0 || self.line_indices.is_empty() {
            return;
        }

        // Convert 1-indexed line number to actual file line index (0-indexed)
        let target_line = line_number.saturating_sub(1);

        // Find the position in line_indices that contains this line number
        if let Some(position) = self.line_indices.iter().position(|&l| l == target_line) {
            self.selected_line = position;
        } else if target_line >= self.total_lines {
            // If line number is beyond total lines, jump to end
            self.selected_line = self.line_indices.len().saturating_sub(1);
        } else {
            // Line exists in file but not in current view (filtered out)
            // Jump to nearest line that exists in view
            let nearest = self
                .line_indices
                .iter()
                .enumerate()
                .min_by_key(|(_, &l)| l.abs_diff(target_line))
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.selected_line = nearest;
        }
    }

    /// Toggle follow mode
    pub fn toggle_follow_mode(&mut self) {
        self.follow_mode = !self.follow_mode;
        if self.follow_mode {
            self.jump_to_end();
        }
    }

    /// Jump to the end of the log
    pub fn jump_to_end(&mut self) {
        if !self.line_indices.is_empty() {
            self.selected_line = self.line_indices.len().saturating_sub(1);
        }
    }

    /// Jump to the beginning of the log
    pub fn jump_to_start(&mut self) {
        self.selected_line = 0;
    }

    /// Apply an event to the application state
    /// This is the central event handler that modifies app state based on events
    pub fn apply_event(&mut self, event: crate::event::AppEvent) {
        use crate::event::AppEvent;

        match event {
            // Navigation events
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

            // Filter input events
            AppEvent::StartFilterInput => self.start_filter_input(),
            AppEvent::FilterInputChar(c) => self.input_char(c),
            AppEvent::FilterInputBackspace => self.input_backspace(),
            AppEvent::FilterInputSubmit => {
                // Save current filter to history before closing
                let pattern = self.input_buffer.clone();
                self.add_to_history(pattern);
                self.cancel_filter_input();
            }
            AppEvent::FilterInputCancel => self.cancel_filter_input(),
            AppEvent::ClearFilter => self.clear_filter(),

            // Filter progress events
            AppEvent::FilterProgress(lines_processed) => {
                self.filter_state = FilterState::Processing {
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
                    let pattern = self.filter_pattern.clone().unwrap_or_default();
                    self.apply_filter(indices, pattern);
                }
                // Follow mode jump will be handled separately in main loop
            }
            AppEvent::FilterError(err) => {
                eprintln!("Filter error: {}", err);
                self.filter_state = FilterState::Inactive;
            }

            // File events
            AppEvent::FileModified {
                new_total,
                old_total: _,
            } => {
                self.total_lines = new_total;
                if self.mode == ViewMode::Normal {
                    self.line_indices = (0..new_total).collect();
                }
                // Incremental filter will be handled by StartFilter event
            }
            AppEvent::FileTruncated { new_total } => {
                eprintln!(
                    "File truncated: {} -> {} lines",
                    self.total_lines, new_total
                );
                // Reset state on truncation
                self.total_lines = new_total;
                self.line_indices = (0..new_total).collect();
                self.mode = ViewMode::Normal;
                self.filter_pattern = None;
                self.filter_state = FilterState::Inactive;
                self.last_filtered_line = 0;
                // Ensure selection is valid
                if self.selected_line >= new_total && new_total > 0 {
                    self.selected_line = new_total - 1;
                } else if new_total == 0 {
                    self.selected_line = 0;
                }
            }
            AppEvent::FileError(err) => {
                eprintln!("File watcher error: {}", err);
            }

            // Mode toggles
            AppEvent::ToggleFollowMode => self.toggle_follow_mode(),
            AppEvent::DisableFollowMode => {
                self.follow_mode = false;
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
                    self.follow_mode = false;
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

    #[test]
    fn test_app_initialization() {
        let app = App::new(100);

        assert_eq!(app.total_lines, 100);
        assert_eq!(app.line_indices.len(), 100);
        assert_eq!(app.selected_line, 0);
        assert_eq!(app.scroll_position, 0);
        assert_eq!(app.mode, ViewMode::Normal);
        assert!(!app.should_quit);
        assert!(!app.follow_mode);
        assert!(app.filter_pattern.is_none());
    }

    #[test]
    fn test_navigation_basic() {
        let mut app = App::new(10);

        // Scroll down
        app.scroll_down();
        assert_eq!(app.selected_line, 1);

        // Scroll down multiple times
        app.scroll_down();
        app.scroll_down();
        assert_eq!(app.selected_line, 3);

        // Scroll up
        app.scroll_up();
        assert_eq!(app.selected_line, 2);

        // Can't scroll below 0
        app.selected_line = 0;
        app.scroll_up();
        assert_eq!(app.selected_line, 0);

        // Can't scroll past end
        app.selected_line = 9;
        app.scroll_down();
        assert_eq!(app.selected_line, 9);
    }

    #[test]
    fn test_page_navigation() {
        let mut app = App::new(100);

        // Page down
        app.page_down(10);
        assert_eq!(app.selected_line, 10);

        // Page up
        app.page_up(5);
        assert_eq!(app.selected_line, 5);

        // Page down past end
        app.selected_line = 95;
        app.page_down(10);
        assert_eq!(app.selected_line, 99); // Last line
    }

    #[test]
    fn test_jump_to_start_end() {
        let mut app = App::new(100);

        // Jump to end
        app.jump_to_end();
        assert_eq!(app.selected_line, 99);

        // Jump to start
        app.jump_to_start();
        assert_eq!(app.selected_line, 0);
    }

    #[test]
    fn test_filter_application() {
        let mut app = App::new(10);
        let matching_indices = vec![1, 3, 5, 7, 9];

        app.apply_filter(matching_indices.clone(), "test".to_string());

        assert_eq!(app.mode, ViewMode::Filtered);
        assert_eq!(app.line_indices, matching_indices);
        assert_eq!(app.filter_pattern, Some("test".to_string()));
        assert_eq!(app.selected_line, 0); // Reset to start
        assert!(matches!(
            app.filter_state,
            FilterState::Complete { matches: 5 }
        ));
    }

    #[test]
    fn test_filter_preserves_selection_on_update() {
        let mut app = App::new(20);

        // Apply initial filter
        app.apply_filter(vec![1, 3, 5, 7, 9], "test".to_string());
        app.selected_line = 2; // Select index 2, which is actual line 5

        // Update filter - line 5 is still in results but at different index
        app.apply_filter(vec![1, 5, 9], "test".to_string());

        // Selection should stay on line 5 (now at index 1)
        assert_eq!(app.selected_line, 1);
        assert_eq!(app.line_indices[app.selected_line], 5);
    }

    #[test]
    fn test_filter_update_when_selected_line_not_in_results() {
        let mut app = App::new(20);

        // Apply initial filter
        app.apply_filter(vec![1, 3, 5, 7, 9], "test".to_string());
        app.selected_line = 2; // Select index 2, which is actual line 5

        // Update filter - line 5 is NOT in new results
        app.apply_filter(vec![1, 3, 7], "test".to_string());

        // Selection should be clamped to valid range (keeps similar position)
        assert_eq!(app.selected_line, 2); // Index 2 is now line 7
        assert_eq!(app.line_indices[app.selected_line], 7);
    }

    #[test]
    fn test_clear_filter_preserves_actual_line() {
        let mut app = App::new(20);

        // Apply filter
        app.apply_filter(vec![2, 5, 10, 15], "test".to_string());

        // Select line at index 2 (actual line 10)
        app.selected_line = 2;

        // Clear filter
        app.clear_filter();

        // Should stay on actual line 10
        assert_eq!(app.selected_line, 10);
        assert_eq!(app.mode, ViewMode::Normal);
        assert!(app.filter_pattern.is_none());
        assert_eq!(app.line_indices.len(), 20);
    }

    #[test]
    fn test_clear_filter_empty_selection() {
        let mut app = App::new(10);

        // Apply filter with no matches
        app.apply_filter(vec![], "nomatch".to_string());

        // Clear filter
        app.clear_filter();

        // Should reset to 0
        assert_eq!(app.selected_line, 0);
        assert_eq!(app.mode, ViewMode::Normal);
    }

    #[test]
    fn test_follow_mode_toggle() {
        let mut app = App::new(10);

        assert!(!app.follow_mode);

        app.toggle_follow_mode();
        assert!(app.follow_mode);

        app.toggle_follow_mode();
        assert!(!app.follow_mode);
    }

    #[test]
    fn test_follow_mode_jumps_to_end() {
        let mut app = App::new(100);
        app.follow_mode = true;

        app.jump_to_end();
        assert_eq!(app.selected_line, 99);
    }

    #[test]
    fn test_filter_input_mode() {
        let mut app = App::new(10);

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
    fn test_input_backspace_empty() {
        let mut app = App::new(10);

        app.start_filter_input();
        app.input_backspace(); // Should not panic on empty input
        assert!(app.get_input().is_empty());
    }

    #[test]
    fn test_append_filter_results() {
        let mut app = App::new(10);

        // Apply initial filter
        app.apply_filter(vec![1, 3, 5], "test".to_string());
        assert_eq!(app.line_indices.len(), 3);

        // Append new results (incremental filtering)
        app.append_filter_results(vec![7, 9]);
        assert_eq!(app.line_indices.len(), 5);
        assert_eq!(app.line_indices, vec![1, 3, 5, 7, 9]);
    }

    #[test]
    fn test_scroll_position_adjustment() {
        let mut app = App::new(100);
        let viewport_height = 20;

        // Scroll near bottom
        app.selected_line = 90;
        app.adjust_scroll(viewport_height);

        // Scroll position should adjust to keep selection visible
        // This ensures selection is visible with padding
        assert!(app.scroll_position <= app.selected_line);
    }

    #[test]
    fn test_empty_file_handling() {
        let app = App::new(0);

        assert_eq!(app.total_lines, 0);
        assert_eq!(app.line_indices.len(), 0);
        assert_eq!(app.selected_line, 0);
    }

    #[test]
    fn test_filter_with_follow_mode() {
        let mut app = App::new(10);
        app.follow_mode = true;

        // Apply filter (follow mode should NOT affect filter application)
        app.apply_filter(vec![1, 3, 5], "test".to_string());

        assert!(app.follow_mode); // Follow mode stays enabled
        assert_eq!(app.mode, ViewMode::Filtered);
    }

    #[test]
    fn test_navigation_bounds_with_filter() {
        let mut app = App::new(100);

        // Apply filter (only 5 lines visible)
        app.apply_filter(vec![10, 20, 30, 40, 50], "test".to_string());

        // Try to scroll past filtered end
        app.selected_line = 4; // Last filtered line
        app.scroll_down();
        assert_eq!(app.selected_line, 4); // Should not go past end

        // Jump to end should go to last filtered line
        app.jump_to_end();
        assert_eq!(app.selected_line, 4);
    }

    #[test]
    fn test_last_filtered_line_tracking() {
        let mut app = App::new(10);

        // Apply filter
        app.apply_filter(vec![1, 3, 5], "test".to_string());

        // last_filtered_line should be updated
        assert_eq!(app.last_filtered_line, 10);
    }

    #[test]
    fn test_help_mode_toggle() {
        use crate::event::AppEvent;

        let mut app = App::new(10);
        assert!(!app.show_help);

        // Show help
        app.apply_event(AppEvent::ShowHelp);
        assert!(app.show_help);

        // Hide help
        app.apply_event(AppEvent::HideHelp);
        assert!(!app.show_help);
    }

    #[test]
    fn test_help_mode_initial_state() {
        let app = App::new(10);
        assert!(!app.show_help); // Help should be hidden initially
    }

    #[test]
    fn test_line_jump_input_mode() {
        let mut app = App::new(10);

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
    fn test_jump_to_line_basic() {
        let mut app = App::new(100);

        // Jump to line 50 (1-indexed)
        app.jump_to_line(50);
        assert_eq!(app.selected_line, 49); // 0-indexed

        // Jump to line 1
        app.jump_to_line(1);
        assert_eq!(app.selected_line, 0);

        // Jump to line 100
        app.jump_to_line(100);
        assert_eq!(app.selected_line, 99);
    }

    #[test]
    fn test_jump_to_line_out_of_bounds() {
        let mut app = App::new(50);

        // Jump to line beyond total lines - should go to end
        app.jump_to_line(200);
        assert_eq!(app.selected_line, 49);

        // Jump to line 0 - should do nothing
        let old_selection = app.selected_line;
        app.jump_to_line(0);
        assert_eq!(app.selected_line, old_selection);
    }

    #[test]
    fn test_jump_to_line_with_filter() {
        let mut app = App::new(100);

        // Apply filter (only lines 10, 30, 50, 70, 90 visible)
        app.apply_filter(vec![10, 30, 50, 70, 90], "test".to_string());

        // Jump to line 31 (1-indexed) which maps to index 30 (0-indexed)
        app.jump_to_line(31);
        assert_eq!(app.selected_line, 1); // Should be at position 1 in filtered results

        // Jump to line 91 (last filtered line)
        app.jump_to_line(91);
        assert_eq!(app.selected_line, 4); // Position 4 in filtered results

        // Jump to line 20 (not in filtered results) - should find nearest
        app.jump_to_line(20);
        // Should jump to nearest visible line (10 or 30)
        let selected_actual_line = app.line_indices[app.selected_line];
        assert!(selected_actual_line == 10 || selected_actual_line == 30);
    }

    #[test]
    fn test_jump_to_line_empty_file() {
        let mut app = App::new(0);

        // Jump to any line in empty file - should do nothing
        app.jump_to_line(1);
        assert_eq!(app.selected_line, 0);
    }

    #[test]
    fn test_line_jump_input_events() {
        use crate::event::AppEvent;

        let mut app = App::new(100);

        // Start line jump input
        app.apply_event(AppEvent::StartLineJumpInput);
        assert!(app.is_entering_line_jump());

        // Add digits
        app.apply_event(AppEvent::LineJumpInputChar('5'));
        app.apply_event(AppEvent::LineJumpInputChar('0'));
        assert_eq!(app.get_input(), "50");

        // Non-digit should not be added
        app.apply_event(AppEvent::LineJumpInputChar('a'));
        assert_eq!(app.get_input(), "50"); // Unchanged

        // Backspace
        app.apply_event(AppEvent::LineJumpInputBackspace);
        assert_eq!(app.get_input(), "5");

        // Submit
        app.apply_event(AppEvent::LineJumpInputSubmit);
        assert!(!app.is_entering_line_jump());
        assert_eq!(app.selected_line, 4); // Line 5 is at index 4
    }

    #[test]
    fn test_line_jump_input_cancel() {
        use crate::event::AppEvent;

        let mut app = App::new(100);
        app.selected_line = 10;

        // Start and enter some input
        app.apply_event(AppEvent::StartLineJumpInput);
        app.apply_event(AppEvent::LineJumpInputChar('5'));
        app.apply_event(AppEvent::LineJumpInputChar('0'));

        // Cancel without jumping
        app.apply_event(AppEvent::LineJumpInputCancel);
        assert!(!app.is_entering_line_jump());
        assert_eq!(app.selected_line, 10); // Should not have moved
        assert_eq!(app.get_input(), ""); // Input buffer cleared
    }

    #[test]
    fn test_line_jump_disables_follow_mode() {
        use crate::event::AppEvent;

        let mut app = App::new(100);
        app.follow_mode = true;

        // Jump to a line
        app.apply_event(AppEvent::StartLineJumpInput);
        app.apply_event(AppEvent::LineJumpInputChar('5'));
        app.apply_event(AppEvent::LineJumpInputChar('0'));
        app.apply_event(AppEvent::LineJumpInputSubmit);

        // Follow mode should be disabled
        assert!(!app.follow_mode);
        assert_eq!(app.selected_line, 49); // Line 50 is at index 49
    }

    #[test]
    fn test_mouse_scroll_down_moves_viewport_and_selection() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Start at top
        app.selected_line = 5;
        app.scroll_position = 0;

        // Scroll down by 3 lines
        app.mouse_scroll_down(3, visible_height);

        // Viewport should move down
        assert_eq!(app.scroll_position, 3);
        // Selection should move down by the same amount
        assert_eq!(app.selected_line, 8);
    }

    #[test]
    fn test_mouse_scroll_down_with_selection_at_top() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Selection at line 2, viewport at 0
        app.selected_line = 2;
        app.scroll_position = 0;

        // Scroll down by 5 lines
        app.mouse_scroll_down(5, visible_height);

        // Viewport moved to 5
        assert_eq!(app.scroll_position, 5);
        // Selection should move down by 5 as well
        assert_eq!(app.selected_line, 7);
    }

    #[test]
    fn test_mouse_scroll_up_moves_viewport_and_selection() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Start scrolled down
        app.selected_line = 25;
        app.scroll_position = 20;

        // Scroll up by 3 lines
        app.mouse_scroll_up(3, visible_height);

        // Viewport should move up
        assert_eq!(app.scroll_position, 17);
        // Selection should move up by the same amount
        assert_eq!(app.selected_line, 22);
    }

    #[test]
    fn test_mouse_scroll_up_with_selection_near_bottom() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Selection at line 39, viewport at 20
        app.selected_line = 39;
        app.scroll_position = 20;

        // Scroll up by 10 lines
        app.mouse_scroll_up(10, visible_height);

        // Viewport moved to 10
        assert_eq!(app.scroll_position, 10);
        // Selection should move up by 10 as well
        assert_eq!(app.selected_line, 29);
    }

    #[test]
    fn test_mouse_scroll_down_at_bottom() {
        let mut app = App::new(50);
        let visible_height = 20;

        // Scroll to near bottom
        app.scroll_position = 30; // Max is 50 - 20 = 30
        app.selected_line = 40;

        // Try to scroll further down
        app.mouse_scroll_down(10, visible_height);

        // Viewport should not scroll past max (stays at 30)
        assert_eq!(app.scroll_position, 30);
        // Since viewport didn't move, selection shouldn't move either
        assert_eq!(app.selected_line, 40);
    }

    #[test]
    fn test_mouse_scroll_up_at_top() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Start at top
        app.scroll_position = 0;
        app.selected_line = 5;

        // Try to scroll further up
        app.mouse_scroll_up(10, visible_height);

        // Should stay at 0
        assert_eq!(app.scroll_position, 0);
        assert_eq!(app.selected_line, 5);
    }

    #[test]
    fn test_mouse_scroll_with_filtered_view() {
        let mut app = App::new(100);
        let visible_height = 10;

        // Apply filter with 20 matching lines
        let filtered_lines: Vec<usize> = (0..20).map(|i| i * 5).collect();
        app.apply_filter(filtered_lines, "test".to_string());

        // Start at top with selection in middle of visible area
        app.selected_line = 5;
        app.scroll_position = 0;

        // Scroll down by 2
        app.mouse_scroll_down(2, visible_height);

        // Viewport should move
        assert_eq!(app.scroll_position, 2);
        // Selection should follow the scroll
        assert_eq!(app.selected_line, 7);
    }

    #[test]
    fn test_mouse_scroll_skips_adjust_scroll() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Position selection at top with some scroll
        app.selected_line = 5;
        app.scroll_position = 5;

        // Mouse scroll down - this would normally trigger adjust_scroll
        // because selection is at top of viewport (within padding zone)
        app.mouse_scroll_down(3, visible_height);

        // Check flag was set
        assert!(app.skip_scroll_adjustment);

        // After mouse scroll: viewport=8, selection=8
        assert_eq!(app.scroll_position, 8);
        assert_eq!(app.selected_line, 8);

        // Call adjust_scroll - it should skip and clear flag
        app.adjust_scroll(visible_height);
        assert!(!app.skip_scroll_adjustment);

        // Scroll position should NOT have changed (no padding adjustment)
        assert_eq!(app.scroll_position, 8);
        assert_eq!(app.selected_line, 8);
    }

    #[test]
    fn test_adjust_scroll_works_normally_without_mouse() {
        let mut app = App::new(100);
        let visible_height = 20;

        // Position selection at top with some scroll
        app.selected_line = 5;
        app.scroll_position = 5;

        // Call adjust_scroll without mouse scroll
        app.adjust_scroll(visible_height);

        // Should apply padding adjustment (selection is at top, should add padding)
        assert_eq!(app.scroll_position, 2); // 5 - 3 (padding)
    }

    #[test]
    fn test_add_to_history() {
        let mut app = App::new(10);

        // Add patterns to history
        app.add_to_history("ERROR".to_string());
        app.add_to_history("WARN".to_string());
        app.add_to_history("INFO".to_string());

        assert_eq!(app.filter_history.len(), 3);
        assert_eq!(app.filter_history[0], "ERROR");
        assert_eq!(app.filter_history[1], "WARN");
        assert_eq!(app.filter_history[2], "INFO");
    }

    #[test]
    fn test_add_to_history_skips_duplicates() {
        let mut app = App::new(10);

        app.add_to_history("ERROR".to_string());
        app.add_to_history("ERROR".to_string()); // Duplicate - should not add

        assert_eq!(app.filter_history.len(), 1);
    }

    #[test]
    fn test_add_to_history_skips_empty() {
        let mut app = App::new(10);

        app.add_to_history("".to_string());

        assert_eq!(app.filter_history.len(), 0);
    }

    #[test]
    fn test_history_limit() {
        let mut app = App::new(10);

        // Add 52 entries to exceed limit of 50
        for i in 0..52 {
            app.add_to_history(format!("pattern{}", i));
        }

        // Should only keep 50 most recent
        assert_eq!(app.filter_history.len(), 50);
        // Oldest should be removed
        assert_eq!(app.filter_history[0], "pattern2");
        assert_eq!(app.filter_history[49], "pattern51");
    }

    #[test]
    fn test_history_up_navigation() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        app.add_to_history("ERROR".to_string());
        app.add_to_history("WARN".to_string());
        app.add_to_history("INFO".to_string());

        // Start filter input
        app.start_filter_input();

        // Navigate up (most recent)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "INFO");
        assert_eq!(app.history_index, Some(2));

        // Navigate up again (older)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "WARN");
        assert_eq!(app.history_index, Some(1));

        // Navigate up again
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "ERROR");
        assert_eq!(app.history_index, Some(0));

        // Try to go up past oldest (should stay)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "ERROR");
        assert_eq!(app.history_index, Some(0));
    }

    #[test]
    fn test_history_down_navigation() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        app.add_to_history("ERROR".to_string());
        app.add_to_history("WARN".to_string());
        app.add_to_history("INFO".to_string());

        app.start_filter_input();

        // Navigate up to oldest
        app.apply_event(AppEvent::HistoryUp);
        app.apply_event(AppEvent::HistoryUp);
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "ERROR");

        // Navigate down (newer)
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "WARN");
        assert_eq!(app.history_index, Some(1));

        // Navigate down again
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "INFO");
        assert_eq!(app.history_index, Some(2));

        // Navigate down past newest (should clear)
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "");
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_history_down_when_not_navigating() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        app.add_to_history("ERROR".to_string());
        app.start_filter_input();

        // Down arrow when not navigating should do nothing
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "");
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_filter_submit_saves_to_history() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        // Start filter and type
        app.start_filter_input();
        app.input_char('E');
        app.input_char('R');
        app.input_char('R');

        // Submit filter
        app.apply_event(AppEvent::FilterInputSubmit);

        // Should be saved to history
        assert_eq!(app.filter_history.len(), 1);
        assert_eq!(app.filter_history[0], "ERR");
    }

    #[test]
    fn test_cancel_filter_resets_history_index() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        app.add_to_history("ERROR".to_string());
        app.start_filter_input();

        // Navigate history
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.history_index, Some(0));

        // Cancel filter
        app.cancel_filter_input();

        // History index should be reset
        assert_eq!(app.history_index, None);
    }

    #[test]
    fn test_history_empty() {
        use crate::event::AppEvent;

        let mut app = App::new(10);

        app.start_filter_input();

        // Try to navigate empty history
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input_buffer, "");
        assert_eq!(app.history_index, None);

        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input_buffer, "");
        assert_eq!(app.history_index, None);
    }
}
