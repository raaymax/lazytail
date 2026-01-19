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
        self.selected_line = (self.selected_line + page_size)
            .min(self.line_indices.len().saturating_sub(1));
    }

    /// Scroll up by page
    pub fn page_up(&mut self, page_size: usize) {
        self.selected_line = self.selected_line.saturating_sub(page_size);
    }

    /// Get the actual line number for the currently selected line
    pub fn get_selected_line_number(&self) -> Option<usize> {
        self.line_indices.get(self.selected_line).copied()
    }

    /// Apply filter results (for full filtering)
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        let was_filtered = self.mode == ViewMode::Filtered;
        let old_selection = self.selected_line;

        self.line_indices = matching_indices;
        self.mode = ViewMode::Filtered;
        self.filter_pattern = Some(pattern);
        self.filter_state = FilterState::Complete { matches: self.line_indices.len() };
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
        self.filter_state = FilterState::Complete { matches: self.line_indices.len() };
        self.last_filtered_line = self.total_lines;
        // Don't change selection - let follow mode or user control it
    }

    /// Clear filter and return to normal view
    pub fn clear_filter(&mut self) {
        self.line_indices = (0..self.total_lines).collect();
        self.mode = ViewMode::Normal;
        self.selected_line = 0;
        self.scroll_position = 0;
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
