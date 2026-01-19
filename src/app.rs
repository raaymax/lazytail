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

    /// Apply filter results (for full filtering)
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        let was_filtered = self.mode == ViewMode::Filtered;
        let old_selection = self.selected_line;

        self.line_indices = matching_indices;
        self.mode = ViewMode::Filtered;
        self.filter_pattern = Some(pattern);
        self.filter_state = FilterState::Complete {
            matches: self.line_indices.len(),
        };
        self.last_filtered_line = self.total_lines;

        // Preserve selection position when updating an existing filter (unless follow mode will handle it)
        if was_filtered && !self.follow_mode {
            // Try to keep selection at the same position, but clamp to new bounds
            self.selected_line = old_selection.min(self.line_indices.len().saturating_sub(1));
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
        let mut app = App::new(10);

        // Apply initial filter
        app.apply_filter(vec![1, 3, 5, 7, 9], "test".to_string());
        app.selected_line = 2; // Select line 5 (index 2 in filtered view)

        // Update filter with new matches
        app.apply_filter(vec![1, 3, 5, 7, 9, 11], "test".to_string());

        // Selection should be preserved
        assert_eq!(app.selected_line, 2);
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
}
