use crate::filter::query;
use crate::filter::{FilterHistoryEntry, FilterMode};
use crate::history;
use crate::source::{self, SourceStatus};
use crate::tab::TabState;
#[cfg(test)]
use std::path::PathBuf;
use std::time::Instant;

/// Maximum number of filter history entries to keep
const MAX_HISTORY_ENTRIES: usize = 50;

/// Represents the current view mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Normal,
    Filtered,
}

/// Source type for categorizing tabs in the tree view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceType {
    /// Sources from project lazytail.yaml config
    ProjectSource,
    /// Sources from global config.yaml
    GlobalSource,
    /// Discovered sources from -n capture mode
    Global,
    /// Files passed as CLI arguments
    File,
    /// Stdin or pipe input
    Pipe,
}

/// Selection state for the source panel tree
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeSelection {
    /// A category header is selected
    Category(SourceType),
    /// An item within a category (category type, index within that category)
    Item(SourceType, usize),
}

/// State for the source panel tree navigation
#[derive(Debug)]
pub struct SourcePanelState {
    /// Currently selected tree item
    pub selection: Option<TreeSelection>,
    /// Whether each category is expanded: [ProjectSource, GlobalSource, Global, Files, Pipes]
    pub expanded: [bool; 5],
}

impl Default for SourcePanelState {
    fn default() -> Self {
        Self {
            selection: None,
            expanded: [true, true, true, true, true], // All expanded by default
        }
    }
}

/// Filter state tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterState {
    #[default]
    Inactive,
    Processing {
        lines_processed: usize,
    },
    Complete {
        matches: usize,
    },
}

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

    /// Query syntax validation error (None = valid or not query syntax, Some = invalid query)
    pub query_error: Option<String>,

    /// Side panel width
    pub side_panel_width: u16,

    /// Time when pending filter should be triggered (for debouncing)
    pub pending_filter_at: Option<Instant>,

    /// Source panel tree state
    pub source_panel: SourcePanelState,

    /// Tab pending close confirmation: (index, name) for identity verification
    pub pending_close_tab: Option<(usize, String)>,

    /// Input mode to restore when cancelling close confirmation
    confirm_return_mode: InputMode,
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
        assert!(
            !tabs.is_empty(),
            "App must be created with at least one tab"
        );
        Self {
            tabs,
            active_tab: 0,
            input_mode: InputMode::Normal,
            input_buffer: String::new(),
            input_cursor: 0,
            should_quit: false,
            show_help: false,
            filter_history: history::load_history(),
            history_index: None,
            current_filter_mode: FilterMode::default(),
            regex_error: None,
            query_error: None,
            side_panel_width: 32,
            pending_filter_at: None,
            source_panel: SourcePanelState::default(),
            pending_close_tab: None,
            confirm_return_mode: InputMode::Normal,
        }
    }

    /// Get a reference to the active tab
    ///
    /// # Panics
    /// Panics if there are no tabs (should never happen as App requires at least one tab)
    pub fn active_tab(&self) -> &TabState {
        debug_assert!(!self.tabs.is_empty(), "No tabs available");
        &self.tabs[self.active_tab]
    }

    /// Get a mutable reference to the active tab
    ///
    /// # Panics
    /// Panics if there are no tabs (should never happen as App requires at least one tab)
    pub fn active_tab_mut(&mut self) -> &mut TabState {
        debug_assert!(!self.tabs.is_empty(), "No tabs available");
        &mut self.tabs[self.active_tab]
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

    /// Add a new tab
    pub fn add_tab(&mut self, tab: TabState) {
        self.tabs.push(tab);
    }

    /// Close a tab by index
    ///
    /// If the last tab is closed, sets should_quit to true.
    /// If the tab is a discovered source with Ended status, deletes the source file.
    pub fn close_tab(&mut self, index: usize) {
        if self.tabs.len() <= 1 {
            // Don't close the last tab - quit instead
            self.should_quit = true;
            return;
        }

        if index < self.tabs.len() {
            let tab = &self.tabs[index];

            // If this is an ended discovered source, delete it
            if tab.source_status == Some(SourceStatus::Ended) {
                if let Some(ref path) = tab.source_path {
                    let _ = source::delete_source(&tab.name, path);
                }
            }

            self.tabs.remove(index);

            // Adjust active_tab if needed
            if self.active_tab >= self.tabs.len() {
                self.active_tab = self.tabs.len() - 1;
            } else if self.active_tab > index {
                self.active_tab -= 1;
            }
        }
    }

    // === Source Panel Methods ===

    /// Get tabs grouped by source type, returning (type, vec of global tab indices)
    pub fn tabs_by_category(&self) -> [(SourceType, Vec<usize>); 5] {
        let mut result = [
            (SourceType::ProjectSource, Vec::new()),
            (SourceType::GlobalSource, Vec::new()),
            (SourceType::Global, Vec::new()),
            (SourceType::File, Vec::new()),
            (SourceType::Pipe, Vec::new()),
        ];

        for (idx, tab) in self.tabs.iter().enumerate() {
            match tab.source_type() {
                SourceType::ProjectSource => result[0].1.push(idx),
                SourceType::GlobalSource => result[1].1.push(idx),
                SourceType::Global => result[2].1.push(idx),
                SourceType::File => result[3].1.push(idx),
                SourceType::Pipe => result[4].1.push(idx),
            }
        }

        result
    }

    /// Find global tab index from category and in-category index
    fn find_tab_index(&self, category: SourceType, idx: usize) -> Option<usize> {
        self.tabs
            .iter()
            .enumerate()
            .filter(|(_, t)| t.source_type() == category)
            .nth(idx)
            .map(|(i, _)| i)
    }

    /// Focus the source panel for tree navigation
    fn focus_source_panel(&mut self) {
        self.input_mode = InputMode::SourcePanel;

        // Initialize selection to current tab if not set
        if self.source_panel.selection.is_none() && !self.tabs.is_empty() {
            let tab = &self.tabs[self.active_tab];
            let stype = tab.source_type();
            let idx = self.tabs[..self.active_tab]
                .iter()
                .filter(|t| t.source_type() == stype)
                .count();
            self.source_panel.selection = Some(TreeSelection::Item(stype, idx));
        }
    }

    /// Navigate tree selection up or down
    fn source_panel_navigate(&mut self, delta: i32) {
        // Build flat list of navigable items
        let categories = self.tabs_by_category();
        let mut items: Vec<TreeSelection> = Vec::new();

        for (cat, tab_indices) in &categories {
            if tab_indices.is_empty() {
                continue; // Skip empty categories
            }
            items.push(TreeSelection::Category(*cat));
            let cat_idx = *cat as usize;
            if self.source_panel.expanded[cat_idx] {
                for i in 0..tab_indices.len() {
                    items.push(TreeSelection::Item(*cat, i));
                }
            }
        }

        if items.is_empty() {
            return;
        }

        // Find current position
        let current_pos = self
            .source_panel
            .selection
            .as_ref()
            .and_then(|sel| items.iter().position(|x| x == sel))
            .unwrap_or(0);

        // Calculate new position (no wrapping)
        let new_pos = (current_pos as i32 + delta)
            .max(0)
            .min(items.len() as i32 - 1) as usize;

        self.source_panel.selection = Some(items[new_pos].clone());
    }

    /// Toggle expand/collapse on the selected category
    fn toggle_category_expand(&mut self) {
        if let Some(TreeSelection::Category(cat)) = self.source_panel.selection {
            let idx = cat as usize;
            self.source_panel.expanded[idx] = !self.source_panel.expanded[idx];
        }
    }

    /// Select a source from the panel (switch to that tab)
    fn select_source_from_panel(&mut self) {
        if let Some(TreeSelection::Item(cat, idx)) = self.source_panel.selection {
            if let Some(tab_idx) = self.find_tab_index(cat, idx) {
                self.active_tab = tab_idx;
                self.input_mode = InputMode::Normal;
            }
        }
    }

    // === Close Confirmation Methods ===

    /// Request closing a tab with confirmation dialog
    fn request_close_tab(&mut self, tab_index: usize) {
        if tab_index < self.tabs.len() {
            let tab_name = self.tabs[tab_index].name.clone();
            self.pending_close_tab = Some((tab_index, tab_name));
            self.confirm_return_mode = self.input_mode;
            self.input_mode = InputMode::ConfirmClose;
        }
    }

    /// Confirm and execute the pending tab close
    fn confirm_pending_close(&mut self) {
        if let Some((tab_index, expected_name)) = self.pending_close_tab.take() {
            let return_mode = self.confirm_return_mode;
            self.input_mode = return_mode;

            // Verify the tab at this index still matches (guards against tab reordering)
            if tab_index < self.tabs.len() && self.tabs[tab_index].name == expected_name {
                self.close_tab(tab_index);
            }

            // Fix source panel selection if returning to source panel
            if return_mode == InputMode::SourcePanel {
                self.fix_source_panel_selection();
            }
        }
    }

    /// Cancel the pending tab close and return to previous mode
    fn cancel_pending_close(&mut self) {
        self.pending_close_tab = None;
        self.input_mode = self.confirm_return_mode;
    }

    /// Fix source panel selection after a tab is closed
    fn fix_source_panel_selection(&mut self) {
        if let Some(TreeSelection::Item(cat, idx)) = self.source_panel.selection {
            let cat_count = self.tabs.iter().filter(|t| t.source_type() == cat).count();
            if cat_count == 0 {
                self.source_panel.selection = None;
            } else if idx >= cat_count {
                self.source_panel.selection = Some(TreeSelection::Item(cat, cat_count - 1));
            }
        }
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
    pub fn mouse_scroll_down(&mut self, lines: usize) {
        self.active_tab_mut().mouse_scroll_down(lines);
    }

    /// Mouse scroll up
    pub fn mouse_scroll_up(&mut self, lines: usize) {
        self.active_tab_mut().mouse_scroll_up(lines);
    }

    /// Viewport scroll down (Ctrl+E) - scroll viewport without moving selection
    pub fn viewport_down(&mut self) {
        self.active_tab_mut().viewport_down();
    }

    /// Viewport scroll up (Ctrl+Y) - scroll viewport without moving selection
    pub fn viewport_up(&mut self) {
        self.active_tab_mut().viewport_up();
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

    /// Merge partial filter results (for immediate display while filtering continues)
    pub fn merge_partial_filter_results(
        &mut self,
        new_indices: Vec<usize>,
        lines_processed: usize,
    ) {
        let tab = self.active_tab_mut();

        // Check if we need to clear old results (new filter started)
        // This is deferred from trigger_filter to prevent blink
        if tab.filter.needs_clear {
            tab.mode = ViewMode::Filtered;
            tab.line_indices.clear();
            tab.filter.needs_clear = false;
        } else if tab.mode == ViewMode::Normal {
            // Switch to filtered mode if this is the first partial result
            tab.mode = ViewMode::Filtered;
            tab.line_indices.clear();
        }

        // Merge new indices (they should already be sorted)
        // Since we process from end to start, new indices may need to be inserted at the beginning
        let is_first_result = tab.line_indices.is_empty();
        if is_first_result {
            tab.line_indices = new_indices;
            // Jump to end to show newest results first (we process from end of file)
            tab.viewport.jump_to_end(&tab.line_indices);
        } else {
            // Count items that will be prepended (items smaller than current first item)
            // This is needed to adjust scroll_position since it's an index that becomes
            // stale when items are inserted before it
            let first_existing = tab.line_indices[0];
            let prepended_count = new_indices
                .iter()
                .filter(|&&idx| idx < first_existing)
                .count();

            // Merge sorted arrays
            let mut merged = Vec::with_capacity(tab.line_indices.len() + new_indices.len());
            let mut i = 0;
            let mut j = 0;

            while i < tab.line_indices.len() && j < new_indices.len() {
                if tab.line_indices[i] <= new_indices[j] {
                    merged.push(tab.line_indices[i]);
                    i += 1;
                } else {
                    merged.push(new_indices[j]);
                    j += 1;
                }
            }

            // Add remaining elements
            merged.extend_from_slice(&tab.line_indices[i..]);
            merged.extend_from_slice(&new_indices[j..]);

            tab.line_indices = merged;

            // Adjust scroll_position to account for prepended items
            // This keeps the view stable - the same content stays visible
            tab.viewport.adjust_scroll_for_prepend(prepended_count);
        }

        // Update filter state with lines processed for progress display
        tab.filter.state = FilterState::Processing { lines_processed };
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
        tab.filter.origin_line = Some(current_line);
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

        // Limit history size
        if self.filter_history.len() > MAX_HISTORY_ENTRIES {
            self.filter_history.remove(0);
        }

        // Reset history navigation
        self.history_index = None;

        // Persist history to disk
        history::save_history(&self.filter_history);
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
        // Also validate query syntax
        self.validate_query();

        if !self.current_filter_mode.is_regex() || self.input_buffer.is_empty() {
            self.regex_error = None;
            return;
        }

        // Skip regex validation if this is query syntax
        if query::is_query_syntax(&self.input_buffer) {
            self.regex_error = None;
            return;
        }

        match regex::Regex::new(&self.input_buffer) {
            Ok(_) => self.regex_error = None,
            Err(e) => self.regex_error = Some(e.to_string()),
        }
    }

    /// Validate the current input as a query (if it looks like query syntax)
    /// Sets query_error to None if valid or not query syntax, Some(error) if invalid
    pub fn validate_query(&mut self) {
        if !query::is_query_syntax(&self.input_buffer) {
            self.query_error = None;
            return;
        }

        match query::parse_query(&self.input_buffer) {
            Ok(filter_query) => {
                // Also validate the filter (e.g., regex patterns)
                match query::QueryFilter::new(filter_query) {
                    Ok(_) => self.query_error = None,
                    Err(e) => self.query_error = Some(e),
                }
            }
            Err(e) => self.query_error = Some(e.message),
        }
    }

    /// Check if the current filter input is valid (regex or query)
    pub fn is_regex_valid(&self) -> bool {
        self.regex_error.is_none() && self.query_error.is_none()
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
            // MouseScrollDown/MouseScrollUp handled directly in main.rs process_event()
            AppEvent::MouseScrollDown(_) | AppEvent::MouseScrollUp(_) => {}
            AppEvent::ViewportDown => self.viewport_down(),
            AppEvent::ViewportUp => self.viewport_up(),

            // Tab navigation events
            AppEvent::SelectTab(index) => self.select_tab(index),
            AppEvent::CloseCurrentTab => {
                let idx = self.active_tab;
                self.request_close_tab(idx);
            }
            AppEvent::CloseSelectedTab => {
                if let Some(TreeSelection::Item(cat, idx)) = self.source_panel.selection.clone() {
                    if let Some(tab_idx) = self.find_tab_index(cat, idx) {
                        self.request_close_tab(tab_idx);
                    }
                }
            }
            AppEvent::ConfirmCloseTab => self.confirm_pending_close(),
            AppEvent::CancelCloseTab => self.cancel_pending_close(),

            // Source panel events
            AppEvent::FocusSourcePanel => self.focus_source_panel(),
            AppEvent::UnfocusSourcePanel => {
                self.input_mode = InputMode::Normal;
            }
            AppEvent::SourcePanelUp => self.source_panel_navigate(-1),
            AppEvent::SourcePanelDown => self.source_panel_navigate(1),
            AppEvent::ToggleCategoryExpand => self.toggle_category_expand(),
            AppEvent::SelectSource => self.select_source_from_panel(),

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
                self.active_tab_mut().filter.origin_line = None;
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
                self.active_tab_mut().filter.state = FilterState::Processing { lines_processed };
            }
            AppEvent::FilterPartialResults {
                matches,
                lines_processed,
            } => {
                // Merge partial results for immediate display
                self.merge_partial_filter_results(matches, lines_processed);
            }
            AppEvent::FilterComplete {
                indices,
                incremental,
            } => {
                if incremental {
                    self.append_filter_results(indices);
                } else {
                    let pattern = self.active_tab().filter.pattern.clone().unwrap_or_default();
                    self.apply_filter(indices, pattern);
                }
                // Follow mode jump will be handled separately in main loop
            }
            AppEvent::FilterError(err) => {
                eprintln!("Filter error: {}", err);
                self.active_tab_mut().filter.state = FilterState::Inactive;
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

                // Cancel any in-progress filter
                if let Some(ref cancel) = tab.filter.cancel_token {
                    cancel.cancel();
                }

                // Reset state on truncation
                tab.total_lines = new_total;
                tab.line_indices = (0..new_total).collect();
                tab.mode = ViewMode::Normal;

                // Fully reset filter state
                tab.filter.pattern = None;
                tab.filter.state = FilterState::Inactive;
                tab.filter.last_filtered_line = 0;
                tab.filter.cancel_token = None;
                tab.filter.receiver = None;
                tab.filter.needs_clear = false;
                tab.filter.is_incremental = false;

                // Reset viewport to valid position
                let new_anchor = if new_total > 0 { new_total - 1 } else { 0 };
                tab.viewport.jump_to_line(new_anchor);

                // Sync old fields from viewport
                tab.selected_line = new_anchor.min(new_total.saturating_sub(1));
                tab.scroll_position = 0;
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

            // Line expansion events
            AppEvent::ToggleLineExpansion => {
                self.active_tab_mut().toggle_expansion();
            }
            AppEvent::CollapseAll => {
                self.active_tab_mut().collapse_all();
            }

            // StartFilter: mark that results need clearing when first results arrive
            AppEvent::StartFilter { incremental, .. } => {
                if !incremental {
                    // Defer clearing until first results arrive (prevents blink)
                    let tab = self.active_tab_mut();
                    tab.filter.needs_clear = true;
                    tab.filter.state = FilterState::Processing { lines_processed: 0 };
                }
                // Actual filter execution is handled in main loop
            }

            // Stream events are handled directly in main loop, not here
            AppEvent::StreamData { .. } | AppEvent::StreamComplete => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::AppEvent;
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

        // Direct selection
        app.select_tab(1);
        assert_eq!(app.active_tab, 1);

        app.select_tab(2);
        assert_eq!(app.active_tab, 2);

        app.select_tab(0);
        assert_eq!(app.active_tab, 0);

        // Invalid selection (out of bounds)
        app.select_tab(10);
        assert_eq!(app.active_tab, 0); // Unchanged
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
        app.select_tab(1);
        assert_eq!(app.active_tab().mode, ViewMode::Normal);
        assert!(app.active_tab().filter.pattern.is_none());

        // Tab 0 should still be filtered
        app.select_tab(0);
        assert_eq!(app.active_tab().mode, ViewMode::Filtered);
    }

    #[test]
    fn test_navigation_basic() {
        let temp_file = create_temp_log_file(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Starts at end in follow mode
        assert_eq!(app.active_tab().selected_line, 9);

        // Jump to start first
        app.jump_to_start();
        assert_eq!(app.active_tab().selected_line, 0);

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

        // Jump to start first (starts at end in follow mode)
        app.jump_to_start();

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
        assert_eq!(app.active_tab().filter.pattern, Some("error".to_string()));
    }

    #[test]
    fn test_clear_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_filter(vec![0, 2], "error".to_string());
        app.clear_filter();

        assert_eq!(app.active_tab().mode, ViewMode::Normal);
        assert!(app.active_tab().filter.pattern.is_none());
    }

    #[test]
    fn test_follow_mode_toggle() {
        let temp_file = create_temp_log_file(&["1", "2", "3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Follow mode enabled by default
        assert!(app.active_tab().follow_mode);

        // Toggle off
        app.toggle_follow_mode();
        assert!(!app.active_tab().follow_mode);

        // Toggle back on
        app.toggle_follow_mode();
        assert!(app.active_tab().follow_mode);
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
        let initial_len = app.filter_history.len();

        // Add patterns to history (use unique names to avoid conflicts with persisted history)
        app.add_to_history("TEST_HIST_ERROR_12345".to_string(), FilterMode::plain());
        app.add_to_history("TEST_HIST_WARN_12345".to_string(), FilterMode::plain());
        app.add_to_history("TEST_HIST_INFO_12345".to_string(), FilterMode::plain());

        assert_eq!(app.filter_history.len(), initial_len + 3);
    }

    #[test]
    fn test_add_to_history_skips_duplicates() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        let initial_len = app.filter_history.len();

        app.add_to_history("ERROR_DUP_TEST".to_string(), FilterMode::plain());
        app.add_to_history("ERROR_DUP_TEST".to_string(), FilterMode::plain()); // Duplicate - should not add

        assert_eq!(app.filter_history.len(), initial_len + 1);
    }

    #[test]
    fn test_add_to_history_same_pattern_different_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        let initial_len = app.filter_history.len();

        app.add_to_history("error_mode_test".to_string(), FilterMode::plain());
        app.add_to_history("error_mode_test".to_string(), FilterMode::regex()); // Different mode - should add

        assert_eq!(app.filter_history.len(), initial_len + 2);
        assert!(!app.filter_history[initial_len].mode.is_regex());
        assert!(app.filter_history[initial_len + 1].mode.is_regex());
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

        app.apply_event(AppEvent::SelectTab(1));
        assert_eq!(app.active_tab, 1);

        app.apply_event(AppEvent::SelectTab(0));
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

    #[test]
    fn test_toggle_line_expansion_event() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Jump to start (starts at end in follow mode)
        app.apply_event(AppEvent::JumpToStart);

        // Initially no lines expanded
        assert!(app.active_tab().expansion.expanded_lines.is_empty());

        // Toggle expansion via event
        app.apply_event(AppEvent::ToggleLineExpansion);
        assert!(app.active_tab().is_line_expanded(0));

        // Toggle again
        app.apply_event(AppEvent::ToggleLineExpansion);
        assert!(!app.active_tab().is_line_expanded(0));
    }

    #[test]
    fn test_collapse_all_event() {
        use crate::event::AppEvent;

        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        // Jump to start (starts at end in follow mode)
        app.apply_event(AppEvent::JumpToStart);

        // Expand some lines
        app.apply_event(AppEvent::ToggleLineExpansion);
        app.apply_event(AppEvent::ScrollDown);
        app.apply_event(AppEvent::ToggleLineExpansion);

        assert_eq!(app.active_tab().expansion.expanded_lines.len(), 2);

        // Collapse all
        app.apply_event(AppEvent::CollapseAll);
        assert!(app.active_tab().expansion.expanded_lines.is_empty());
    }

    #[test]
    fn test_close_tab_request_sets_mode_and_stores_index() {
        let file1 = create_temp_log_file(&["line1"]);
        let file2 = create_temp_log_file(&["line2"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        assert_eq!(app.input_mode, InputMode::Normal);
        app.apply_event(AppEvent::CloseCurrentTab);

        assert_eq!(app.input_mode, InputMode::ConfirmClose);
        assert!(app.pending_close_tab.is_some());
        let (idx, name) = app.pending_close_tab.as_ref().unwrap();
        assert_eq!(*idx, 0);
        assert_eq!(*name, app.tabs[0].name);
    }

    #[test]
    fn test_confirm_close_tab_closes_and_restores_mode() {
        let file1 = create_temp_log_file(&["line1"]);
        let file2 = create_temp_log_file(&["line2"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        app.apply_event(AppEvent::CloseCurrentTab);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.input_mode, InputMode::ConfirmClose);

        app.apply_event(AppEvent::ConfirmCloseTab);
        assert_eq!(app.tabs.len(), 1);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.pending_close_tab.is_none());
    }

    #[test]
    fn test_cancel_close_tab_restores_mode_without_closing() {
        let file1 = create_temp_log_file(&["line1"]);
        let file2 = create_temp_log_file(&["line2"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        app.apply_event(AppEvent::CloseCurrentTab);
        assert_eq!(app.input_mode, InputMode::ConfirmClose);

        app.apply_event(AppEvent::CancelCloseTab);
        assert_eq!(app.tabs.len(), 2);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert!(app.pending_close_tab.is_none());
    }

    #[test]
    fn test_confirm_close_verifies_tab_identity() {
        let file1 = create_temp_log_file(&["line1"]);
        let file2 = create_temp_log_file(&["line2"]);
        let file3 = create_temp_log_file(&["line3"]);
        let mut app = App::new(
            vec![
                file1.path().to_path_buf(),
                file2.path().to_path_buf(),
                file3.path().to_path_buf(),
            ],
            false,
        )
        .unwrap();

        // Request close on tab 1
        app.active_tab = 1;
        let original_name = app.tabs[1].name.clone();
        app.apply_event(AppEvent::CloseCurrentTab);
        assert_eq!(app.pending_close_tab.as_ref().unwrap().1, original_name);

        // Simulate the tab at index 1 being replaced (name mismatch)
        app.tabs[1].name = "different_name".to_string();

        // Confirm should NOT close because name doesn't match
        app.apply_event(AppEvent::ConfirmCloseTab);
        assert_eq!(app.tabs.len(), 3);
    }
}
