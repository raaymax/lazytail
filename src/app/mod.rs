pub mod event;
pub mod filter_controller;
pub mod input_controller;
pub mod source_panel;
pub mod tab;
pub mod tab_manager;
pub mod viewport;

pub use event::AppEvent;
pub use filter_controller::FilterController;
pub use input_controller::{InputController, InputMode};
pub use source_panel::SourcePanelController;
pub use tab::{StreamMessage, TabState};
pub use tab_manager::TabManager;

use crate::filter_orchestrator::FilterOrchestrator;
use crate::renderer::PresetRegistry;
use std::collections::HashMap;
#[cfg(test)]
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Lightweight rectangle for storing layout areas (avoids ratatui dependency in app module)
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutRect {
    pub x: u16,
    pub y: u16,
    pub width: u16,
    pub height: u16,
}

impl LayoutRect {
    /// Check if a point is inside the inner content area (excluding 1px borders on all sides)
    fn contains_inner(&self, column: u16, row: u16) -> bool {
        column > self.x
            && column < self.x + self.width.saturating_sub(1)
            && row > self.y
            && row < self.y + self.height.saturating_sub(1)
    }

    /// Convert a terminal row to inner content row (0-indexed, relative to content start)
    fn inner_row(&self, row: u16) -> usize {
        (row - self.y - 1) as usize
    }
}

/// Cached layout areas from the most recent render pass.
/// Used by mouse click handling to resolve click targets.
#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAreas {
    /// The sources list area in the side panel (top portion)
    pub side_panel_sources: LayoutRect,
    /// The main log content area
    pub log_view: LayoutRect,
}

/// Represents the current view mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    Normal,
    Filtered,
    Aggregation,
}

/// Source type for categorizing tabs in the tree view
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(usize)]
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

impl SourceType {
    /// Convert array index back to SourceType.
    pub fn from_index(idx: usize) -> Self {
        match idx {
            0 => SourceType::ProjectSource,
            1 => SourceType::GlobalSource,
            2 => SourceType::Global,
            3 => SourceType::File,
            4 => SourceType::Pipe,
            _ => panic!("invalid SourceType index: {}", idx),
        }
    }
}

/// Selection state for the source panel tree
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TreeSelection {
    /// Per-category combined view entry
    CombinedForCategory(SourceType),
    /// A category header is selected
    Category(SourceType),
    /// An item within a category (category type, index within that category)
    Item(SourceType, usize),
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

/// Main application state
pub struct App {
    /// Tab management (tabs, combined views, active tab)
    pub tab_mgr: TabManager,

    /// Text input state (buffer, cursor, mode)
    pub input: InputController,

    /// Filter validation, debouncing, and history
    pub filter: FilterController,

    /// Source panel tree navigation
    pub panel: SourcePanelController,

    /// Should the app quit
    pub should_quit: bool,

    /// Help overlay scroll offset (None = hidden, Some(n) = visible at offset n)
    pub help_scroll_offset: Option<usize>,

    /// Tab pending close confirmation: (index, name) for identity verification
    pub pending_close_tab: Option<(usize, String)>,

    /// Input mode to restore when cancelling close confirmation
    confirm_return_mode: InputMode,

    /// Temporary status message shown in the status bar
    pub status_message: Option<(String, Instant)>,

    /// Transient flag: set per event batch when a StartFilter event is present.
    /// Suppresses follow-mode jump on FileModified (the filter restart handles positioning).
    pub has_start_filter_in_batch: bool,

    /// Startup timestamp for measuring time-to-first-render
    pub startup_time: Option<Instant>,

    /// Elapsed time to first render (printed after terminal restore)
    pub first_render_elapsed: Option<Duration>,

    /// Verbose mode (-v flag)
    pub verbose: bool,

    /// Cached layout areas from the most recent render pass
    pub layout: LayoutAreas,

    /// Preset registry for rendering structured log lines
    pub preset_registry: Arc<PresetRegistry>,

    /// Color theme for UI rendering
    pub theme: crate::theme::Theme,

    /// Map from source name to renderer preset names (from config).
    /// Used to assign renderers to dynamically discovered sources.
    pub source_renderer_map: HashMap<String, Vec<String>>,

    /// Warning popup — shown as overlay, dismissed on any key
    pub warning_popup: Option<String>,
}

impl App {
    #[cfg(test)]
    pub fn new(files: Vec<PathBuf>, watch: bool) -> anyhow::Result<Self> {
        let mut tabs = Vec::new();
        for file in files {
            tabs.push(TabState::new(file, watch)?);
        }

        Ok(Self::with_tabs(
            tabs,
            Arc::new(PresetRegistry::new(Vec::new())),
        ))
    }

    /// Create an App with pre-created tabs
    pub fn with_tabs(tabs: Vec<TabState>, preset_registry: Arc<PresetRegistry>) -> Self {
        assert!(
            !tabs.is_empty(),
            "App must be created with at least one tab"
        );
        Self {
            tab_mgr: TabManager::new(tabs),
            input: InputController::new(),
            filter: FilterController::new(),
            panel: SourcePanelController::new(),
            should_quit: false,
            help_scroll_offset: None,
            pending_close_tab: None,
            confirm_return_mode: InputMode::Normal,
            status_message: None,
            has_start_filter_in_batch: false,
            startup_time: None,
            first_render_elapsed: None,
            verbose: false,
            layout: LayoutAreas::default(),
            preset_registry,
            theme: crate::theme::Theme::dark(),
            source_renderer_map: HashMap::new(),
            warning_popup: None,
        }
    }

    // === Delegation methods for backward compatibility ===

    /// Get a reference to the active tab
    pub fn active_tab(&self) -> &TabState {
        self.tab_mgr.active_tab()
    }

    /// Get a mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> &mut TabState {
        self.tab_mgr.active_tab_mut()
    }

    /// Switch to a specific tab by index
    pub fn select_tab(&mut self, index: usize) {
        self.tab_mgr.select_tab(index);
        self.check_index_warning();
    }

    /// Show a warning popup if the active tab has a broken index.
    fn check_index_warning(&mut self) {
        if self.tab_mgr.tab_count() > 0 {
            if let Some(ref warning) = self.active_tab().source.index_warning {
                self.warning_popup = Some(warning.clone());
                return;
            }
        }
        self.warning_popup = None;
    }

    /// Map a sidebar shortcut number (0-based) to the real tab index.
    pub fn tab_index_for_shortcut(&self, shortcut: usize) -> Option<usize> {
        self.tab_mgr.tab_index_for_shortcut(shortcut)
    }

    /// Get the number of tabs
    pub fn tab_count(&self) -> usize {
        self.tab_mgr.tab_count()
    }

    /// Add a new tab
    pub fn add_tab(&mut self, tab: TabState) {
        self.tab_mgr.add_tab(tab);
        self.check_index_warning();
    }

    /// Close a tab by index
    pub fn close_tab(&mut self, index: usize) {
        let should_quit = self.tab_mgr.close_tab(index);
        if should_quit {
            self.should_quit = true;
        }
    }

    /// Get the current input buffer content
    pub fn get_input(&self) -> &str {
        self.input.get_input()
    }

    /// Get the current cursor position
    pub fn get_cursor_position(&self) -> usize {
        self.input.get_cursor_position()
    }

    /// Check if the current filter input is valid (regex and query)
    pub fn is_regex_valid(&self) -> bool {
        self.filter.is_valid()
    }

    /// Check if currently entering filter input
    pub fn is_entering_filter(&self) -> bool {
        self.input.is_entering_filter()
    }

    /// Check if currently entering line jump input
    pub fn is_entering_line_jump(&self) -> bool {
        self.input.is_entering_line_jump()
    }

    // === Source Panel Methods ===

    /// Focus the source panel for tree navigation
    fn focus_source_panel(&mut self) {
        self.input.mode = InputMode::SourcePanel;

        if self.panel.state.selection.is_none() {
            if let Some(cat) = self.tab_mgr.active_combined {
                self.panel.state.selection = Some(TreeSelection::CombinedForCategory(cat));
            } else if !self.tab_mgr.tabs.is_empty() {
                let tab = &self.tab_mgr.tabs[self.tab_mgr.active];
                let stype = tab.source_type();
                let idx = self.tab_mgr.tabs[..self.tab_mgr.active]
                    .iter()
                    .filter(|t| t.source_type() == stype)
                    .count();
                self.panel.state.selection = Some(TreeSelection::Item(stype, idx));
            }
        }
    }

    /// Build a flat list of navigable tree items (categories + expanded sources)
    pub fn build_source_tree_items(&self) -> Vec<TreeSelection> {
        let categories = self.tab_mgr.tabs_by_category();
        let mut items: Vec<TreeSelection> = Vec::new();

        for (cat, tab_indices) in &categories {
            if tab_indices.is_empty() {
                continue;
            }
            items.push(TreeSelection::Category(*cat));
            let cat_idx = *cat as usize;
            if self.panel.state.expanded[cat_idx] {
                if self.tab_mgr.combined[cat_idx].is_some() {
                    items.push(TreeSelection::CombinedForCategory(*cat));
                }
                for i in 0..tab_indices.len() {
                    items.push(TreeSelection::Item(*cat, i));
                }
            }
        }

        items
    }

    /// Select a source from the panel (switch to that tab)
    fn select_source_from_panel(&mut self) {
        match self.panel.state.selection {
            Some(TreeSelection::CombinedForCategory(cat)) => {
                self.tab_mgr.select_combined_tab(cat);
                self.input.mode = InputMode::Normal;
            }
            Some(TreeSelection::Item(cat, idx)) => {
                if let Some(tab_idx) = self.tab_mgr.find_tab_index(cat, idx) {
                    self.tab_mgr.active = tab_idx;
                    self.tab_mgr.active_combined = None;
                    self.input.mode = InputMode::Normal;
                }
            }
            _ => {}
        }
    }

    /// Copy the selected source's file path to clipboard via OSC 52
    fn copy_source_path(&mut self) {
        let tab_idx = if let Some(TreeSelection::Item(cat, idx)) = self.panel.state.selection {
            self.tab_mgr.find_tab_index(cat, idx)
        } else {
            None
        };

        if let Some(tab_idx) = tab_idx {
            if let Some(path) = &self.tab_mgr.tabs[tab_idx].source.source_path {
                let path_str = path.display().to_string();
                let encoded = base64_encode(path_str.as_bytes());
                print!("\x1b]52;c;{}\x07", encoded);
                self.status_message = Some((format!("Copied: {}", path_str), Instant::now()));
            }
        }
    }

    /// Copy the selected line's content (ANSI-stripped) to clipboard via OSC 52
    fn copy_selected_line(&mut self) {
        let tab = self.active_tab_mut();
        if tab.source.line_indices.is_empty() {
            return;
        }

        let file_line_number = match tab.source.line_indices.get(tab.selected_line) {
            Some(&n) => n,
            None => return,
        };

        let content = {
            let mut reader = match tab.source.reader.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            reader.get_line(file_line_number).ok().flatten()
        };

        if let Some(raw) = content {
            let clean = crate::ansi::strip_ansi(&raw);
            let encoded = base64_encode(clean.as_bytes());
            print!("\x1b]52;c;{}\x07", encoded);

            let display = if clean.is_empty() {
                "Copied: (empty line)".to_string()
            } else if clean.len() > 60 {
                format!("Copied: {}...", &clean[..clean.floor_char_boundary(57)])
            } else {
                format!("Copied: {}", clean)
            };
            self.status_message = Some((display, Instant::now()));
        }
    }

    // === Close Confirmation Methods ===

    /// Request closing a tab with confirmation dialog
    fn request_close_tab(&mut self, tab_index: usize) {
        if tab_index < self.tab_mgr.tabs.len() {
            let tab_name = self.tab_mgr.tabs[tab_index].source.name.clone();
            self.pending_close_tab = Some((tab_index, tab_name));
            self.confirm_return_mode = self.input.mode;
            self.input.mode = InputMode::ConfirmClose;
        }
    }

    /// Confirm and execute the pending tab close
    fn confirm_pending_close(&mut self) {
        if let Some((tab_index, expected_name)) = self.pending_close_tab.take() {
            let return_mode = self.confirm_return_mode;
            self.input.mode = return_mode;

            if tab_index < self.tab_mgr.tabs.len()
                && self.tab_mgr.tabs[tab_index].source.name == expected_name
            {
                self.close_tab(tab_index);
            }

            if return_mode == InputMode::SourcePanel {
                self.fix_source_panel_selection();
            }
        }
    }

    /// Cancel the pending tab close and return to previous mode
    fn cancel_pending_close(&mut self) {
        self.pending_close_tab = None;
        self.input.mode = self.confirm_return_mode;
    }

    /// Fix source panel selection after a tab is closed
    fn fix_source_panel_selection(&mut self) {
        let tabs = &self.tab_mgr.tabs;
        self.panel.fix_selection_after_close(|cat| {
            tabs.iter().filter(|t| t.source_type() == cat).count()
        });
    }

    // === Delegated scroll/navigation methods ===

    pub fn scroll_down(&mut self) {
        self.active_tab_mut().scroll_down();
    }

    pub fn scroll_up(&mut self) {
        self.active_tab_mut().scroll_up();
    }

    pub fn page_down(&mut self, page_size: usize) {
        self.active_tab_mut().page_down(page_size);
    }

    pub fn page_up(&mut self, page_size: usize) {
        self.active_tab_mut().page_up(page_size);
    }

    pub fn mouse_scroll_down(&mut self, lines: usize) {
        self.active_tab_mut().mouse_scroll_down(lines);
    }

    pub fn mouse_scroll_up(&mut self, lines: usize) {
        self.active_tab_mut().mouse_scroll_up(lines);
    }

    pub fn viewport_down(&mut self) {
        self.active_tab_mut().viewport_down();
    }

    pub fn viewport_up(&mut self) {
        self.active_tab_mut().viewport_up();
    }

    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        self.active_tab_mut()
            .apply_filter(matching_indices, pattern);
    }

    pub fn append_filter_results(&mut self, new_matching_indices: Vec<usize>) {
        self.active_tab_mut()
            .append_filter_results(new_matching_indices);
    }

    pub fn merge_partial_filter_results(
        &mut self,
        new_indices: Vec<usize>,
        lines_processed: usize,
    ) {
        self.active_tab_mut()
            .merge_partial_filter_results(new_indices, lines_processed);
    }

    fn aggregation_drill_down(&mut self) {
        let tab = self.active_tab_mut();
        let selected = tab.aggregation_view.selected_row;

        if let Some(result) = tab.source.aggregation_result.take() {
            if let Some(group) = result.groups.get(selected) {
                let drill_pattern = group
                    .key
                    .iter()
                    .map(|(name, value)| format!("{} == \"{}\"", name, value))
                    .collect::<Vec<_>>()
                    .join(" & ");

                tab.source.filter.drill_down_pattern = tab.source.filter.pattern.clone();
                tab.source.line_indices = group.line_indices.clone();
                tab.source.mode = ViewMode::Filtered;
                tab.source.filter.pattern = Some(drill_pattern);
                tab.source.filter.state = FilterState::Complete {
                    matches: tab.source.line_indices.len(),
                };
                tab.source.filter.drill_down_aggregation = Some(result);
                let indices = tab.source.line_indices.clone();
                tab.viewport.jump_to_start(&indices);
            } else {
                tab.source.aggregation_result = Some(result);
            }
        }
    }

    fn aggregation_back(&mut self) {
        let tab = self.active_tab_mut();

        if let Some(result) = tab.source.filter.drill_down_aggregation.take() {
            tab.source.filter.pattern = tab.source.filter.drill_down_pattern.take();
            tab.source.aggregation_result = Some(result);
            tab.source.mode = ViewMode::Aggregation;
        } else {
            tab.clear_filter();
        }
    }

    fn maybe_compute_aggregation(&mut self) {
        let tab = self.active_tab_mut();
        if let Some((ref agg, ref parser)) = tab.source.filter.pending_aggregation {
            let agg = agg.clone();
            let parser = parser.clone();
            let indices = tab.source.line_indices.clone();
            let mut reader = match tab.source.reader.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            let result = crate::filter::aggregation::AggregationResult::compute(
                &mut *reader,
                &indices,
                &agg,
                &parser,
            );
            drop(reader);
            let tab = self.active_tab_mut();
            tab.source.aggregation_result = Some(result);
            tab.source.mode = ViewMode::Aggregation;
            tab.aggregation_view = tab::AggregationViewState::default();
        }
    }

    pub fn clear_filter(&mut self) {
        self.active_tab_mut().clear_filter();
    }

    /// Trigger live filter preview based on current input.
    pub fn trigger_filter_preview(&mut self) {
        let pattern = self.get_input().to_string();
        let mode = self.filter.current_mode;

        if !pattern.is_empty() && self.is_regex_valid() {
            let tab = self.active_tab_mut();
            tab.source.filter.pattern = Some(pattern.clone());
            tab.source.filter.mode = mode;
            if let Err(e) = FilterOrchestrator::trigger(&mut tab.source, pattern, mode, None) {
                self.status_message = Some((e, Instant::now()));
                self.active_tab_mut().source.filter.state = FilterState::Inactive;
            }
        } else {
            self.clear_filter();
            self.active_tab_mut().source.filter.receiver = None;
        }
    }

    /// Enter filter input mode
    pub fn start_filter_input(&mut self) {
        self.input.mode = InputMode::EnteringFilter;
        self.input.clear();

        let tab = self.active_tab_mut();
        let current_line = tab.viewport.selected_line();
        tab.source.filter.origin_line = Some(current_line);
    }

    /// Cancel filter input and return to normal mode
    pub fn cancel_filter_input(&mut self) {
        self.input.mode = InputMode::Normal;
        self.input.clear();
        self.filter.reset_history_index();
    }

    /// Enter line jump input mode
    pub fn start_line_jump_input(&mut self) {
        self.input.mode = InputMode::EnteringLineJump;
        self.input.clear();
    }

    /// Cancel line jump input and return to normal mode
    pub fn cancel_line_jump_input(&mut self) {
        self.input.mode = InputMode::Normal;
        self.input.clear();
    }

    pub fn jump_to_line(&mut self, line_number: usize) {
        self.active_tab_mut().jump_to_line(line_number);
    }

    pub fn toggle_follow_mode(&mut self) {
        self.active_tab_mut().toggle_follow_mode();
    }

    pub fn jump_to_end(&mut self) {
        self.active_tab_mut().jump_to_end();
    }

    pub fn jump_to_start(&mut self) {
        self.active_tab_mut().jump_to_start();
    }

    /// Apply an event to the application state.
    ///
    /// Central event dispatcher — delegates to concern-focused handler methods.
    /// Note: most state changes flow through this method, but some mutations
    /// happen outside it by design: inactive tab updates (FileModified on
    /// background tabs), combined-view refresh (main loop), and stream data
    /// appending (main loop). These bypasses exist because the event targets
    /// a tab other than the active one, or because the data arrives outside
    /// the event channel.
    pub fn apply_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;

        match event {
            // Navigation
            AppEvent::ScrollDown
            | AppEvent::ScrollUp
            | AppEvent::PageDown(_)
            | AppEvent::PageUp(_)
            | AppEvent::JumpToStart
            | AppEvent::JumpToEnd
            | AppEvent::MouseScrollDown(_)
            | AppEvent::MouseScrollUp(_)
            | AppEvent::ViewportDown
            | AppEvent::ViewportUp => self.handle_navigation_event(event),

            // Tab management
            AppEvent::SelectTab(_)
            | AppEvent::CloseCurrentTab
            | AppEvent::CloseSelectedTab
            | AppEvent::ConfirmCloseTab
            | AppEvent::CancelCloseTab => self.handle_tab_event(event),

            // Source panel
            AppEvent::FocusSourcePanel
            | AppEvent::UnfocusSourcePanel
            | AppEvent::SourcePanelUp
            | AppEvent::SourcePanelDown
            | AppEvent::ToggleCategoryExpand
            | AppEvent::SelectSource
            | AppEvent::CopySourcePath
            | AppEvent::CopySelectedLine => self.handle_source_panel_event(event),

            // Filter input
            AppEvent::StartFilterInput
            | AppEvent::FilterInputChar(_)
            | AppEvent::FilterInputBackspace
            | AppEvent::FilterInputSubmit
            | AppEvent::FilterInputCancel
            | AppEvent::ClearFilter
            | AppEvent::ToggleFilterMode
            | AppEvent::ToggleCaseSensitivity
            | AppEvent::CursorLeft
            | AppEvent::CursorRight
            | AppEvent::CursorHome
            | AppEvent::CursorEnd
            | AppEvent::StartFilter { .. } => self.handle_filter_input_event(event),

            // Filter progress
            AppEvent::FilterProgress(_)
            | AppEvent::FilterPartialResults { .. }
            | AppEvent::FilterComplete { .. }
            | AppEvent::FilterError(_) => self.handle_filter_progress_event(event),

            // File events
            AppEvent::FileModified { .. } | AppEvent::FileTruncated { .. } => {
                self.handle_file_event(event)
            }

            // Help overlay
            AppEvent::ShowHelp
            | AppEvent::HideHelp
            | AppEvent::ScrollHelpDown
            | AppEvent::ScrollHelpUp => self.handle_help_event(event),

            // Line jump
            AppEvent::StartLineJumpInput
            | AppEvent::LineJumpInputChar(_)
            | AppEvent::LineJumpInputBackspace
            | AppEvent::LineJumpInputSubmit
            | AppEvent::LineJumpInputCancel => self.handle_line_jump_event(event),

            // Filter history
            AppEvent::HistoryUp | AppEvent::HistoryDown => self.handle_history_event(event),

            // View positioning (vim z commands)
            AppEvent::EnterZMode
            | AppEvent::ExitZMode
            | AppEvent::CenterView
            | AppEvent::ViewToTop
            | AppEvent::ViewToBottom => self.handle_view_position_event(event),

            // Mode toggles
            AppEvent::ToggleFollowMode => self.toggle_follow_mode(),
            AppEvent::DisableFollowMode => {
                self.active_tab_mut().source.follow_mode = false;
            }
            AppEvent::ToggleRawMode => {
                let tab = self.active_tab_mut();
                tab.source.raw_mode = !tab.source.raw_mode;
            }
            AppEvent::ToggleLineWrap => {
                let tab = self.active_tab_mut();
                tab.source.line_wrap = !tab.source.line_wrap;
            }
            AppEvent::ToggleTimestamps => {
                let tab = self.active_tab_mut();
                tab.source.show_timestamps = !tab.source.show_timestamps;
            }

            // Line expansion
            AppEvent::ToggleLineExpansion => self.active_tab_mut().toggle_expansion(),
            AppEvent::CollapseAll => self.active_tab_mut().collapse_all(),

            // Aggregation
            AppEvent::AggregationDown
            | AppEvent::AggregationUp
            | AppEvent::AggregationJumpToStart
            | AppEvent::AggregationJumpToEnd
            | AppEvent::AggregationDrillDown
            | AppEvent::AggregationBack => self.handle_aggregation_event(event),

            // Combined view
            AppEvent::RefreshCombinedView => {
                if let Some(cat) = self.tab_mgr.active_combined {
                    self.tab_mgr.refresh_combined_tab(cat);
                    let cat_idx = cat as usize;
                    if let Some(ref mut tab) = self.tab_mgr.combined[cat_idx] {
                        tab.source.mode = ViewMode::Normal;
                        let indices = tab.source.line_indices.clone();
                        tab.viewport.jump_to_end(&indices);
                    }
                    self.status_message =
                        Some(("Combined view refreshed".to_string(), Instant::now()));
                }
            }

            // Mouse
            AppEvent::MouseClick { column, row } => self.handle_mouse_click(column, row),

            // System
            AppEvent::DismissWarning => self.warning_popup = None,
            AppEvent::Quit => self.should_quit = true,

            // Stream events are handled directly in main loop
            AppEvent::StreamData { .. } | AppEvent::StreamComplete => {}
        }
    }

    // === Event handler methods (delegated from apply_event) ===

    fn handle_navigation_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::ScrollDown => self.scroll_down(),
            AppEvent::ScrollUp => self.scroll_up(),
            AppEvent::PageDown(size) => self.page_down(size),
            AppEvent::PageUp(size) => self.page_up(size),
            AppEvent::JumpToStart => self.jump_to_start(),
            AppEvent::JumpToEnd => self.jump_to_end(),
            AppEvent::MouseScrollDown(lines) => self.mouse_scroll_down(lines),
            AppEvent::MouseScrollUp(lines) => self.mouse_scroll_up(lines),
            AppEvent::ViewportDown => self.viewport_down(),
            AppEvent::ViewportUp => self.viewport_up(),
            _ => {}
        }
    }

    fn handle_tab_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::SelectTab(shortcut) => {
                if let Some(tab_idx) = self.tab_index_for_shortcut(shortcut) {
                    self.select_tab(tab_idx);
                }
            }
            AppEvent::CloseCurrentTab => {
                if self.tab_mgr.active_combined.is_none() {
                    let idx = self.tab_mgr.active;
                    self.request_close_tab(idx);
                }
            }
            AppEvent::CloseSelectedTab => match self.panel.state.selection.clone() {
                Some(TreeSelection::CombinedForCategory(_)) => {}
                Some(TreeSelection::Item(cat, idx)) => {
                    if let Some(tab_idx) = self.tab_mgr.find_tab_index(cat, idx) {
                        self.request_close_tab(tab_idx);
                    }
                }
                _ => {}
            },
            AppEvent::ConfirmCloseTab => self.confirm_pending_close(),
            AppEvent::CancelCloseTab => self.cancel_pending_close(),
            _ => {}
        }
    }

    fn handle_source_panel_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::FocusSourcePanel => self.focus_source_panel(),
            AppEvent::UnfocusSourcePanel => self.input.mode = InputMode::Normal,
            AppEvent::SourcePanelUp => {
                let items = self.build_source_tree_items();
                self.panel.navigate(-1, &items);
            }
            AppEvent::SourcePanelDown => {
                let items = self.build_source_tree_items();
                self.panel.navigate(1, &items);
            }
            AppEvent::ToggleCategoryExpand => self.panel.toggle_category_expand(),
            AppEvent::SelectSource => self.select_source_from_panel(),
            AppEvent::CopySourcePath => self.copy_source_path(),
            AppEvent::CopySelectedLine => self.copy_selected_line(),
            _ => {}
        }
    }

    fn handle_filter_input_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::StartFilterInput => self.start_filter_input(),
            AppEvent::FilterInputChar(c) => {
                self.input.input_char(c);
                self.filter.validate_regex(&self.input.buffer);
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.filter.schedule_debounce();
            }
            AppEvent::FilterInputBackspace => {
                self.input.input_backspace();
                self.filter.validate_regex(&self.input.buffer);
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.filter.schedule_debounce();
            }
            AppEvent::FilterInputSubmit => {
                self.filter.pending_at = None;
                let pattern = self.input.buffer.clone();
                let mode = self.filter.current_mode;
                if !pattern.is_empty() && self.is_regex_valid() {
                    let tab = self.active_tab_mut();
                    tab.source.filter.pattern = Some(pattern.clone());
                    tab.source.filter.mode = mode;
                    if let Err(e) =
                        FilterOrchestrator::trigger(&mut tab.source, pattern.clone(), mode, None)
                    {
                        self.status_message = Some((e, Instant::now()));
                        self.active_tab_mut().source.filter.state = FilterState::Inactive;
                    }
                }
                self.filter.add_to_history(pattern, mode);
                self.active_tab_mut().source.filter.origin_line = None;
                self.cancel_filter_input();
            }
            AppEvent::FilterInputCancel => {
                self.filter.pending_at = None;
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.cancel_filter_input();
            }
            AppEvent::ClearFilter => {
                self.filter.pending_at = None;
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.active_tab_mut().source.filter.receiver = None;
                self.clear_filter();
            }
            AppEvent::ToggleFilterMode => {
                self.filter.current_mode.cycle_mode();
                self.filter.validate_regex(&self.input.buffer);
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.filter.schedule_debounce();
            }
            AppEvent::ToggleCaseSensitivity => {
                self.filter.current_mode.toggle_case_sensitivity();
                FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
                self.filter.schedule_debounce();
            }
            AppEvent::CursorLeft => self.input.cursor_left(),
            AppEvent::CursorRight => self.input.cursor_right(),
            AppEvent::CursorHome => self.input.cursor_home(),
            AppEvent::CursorEnd => self.input.cursor_end(),
            AppEvent::StartFilter { pattern, range, .. } => {
                let mode = self.filter.current_mode;
                let tab = self.active_tab_mut();
                tab.source.filter.pattern = Some(pattern.clone());
                tab.source.filter.mode = mode;
                if let Err(e) = FilterOrchestrator::trigger(&mut tab.source, pattern, mode, range) {
                    self.status_message = Some((e, Instant::now()));
                    self.active_tab_mut().source.filter.state = FilterState::Inactive;
                }
            }
            _ => {}
        }
    }

    fn handle_filter_progress_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::FilterProgress(lines_processed) => {
                self.active_tab_mut().source.filter.state =
                    FilterState::Processing { lines_processed };
            }
            AppEvent::FilterPartialResults {
                matches,
                lines_processed,
            } => {
                self.merge_partial_filter_results(matches, lines_processed);
                self.maybe_compute_aggregation();
            }
            AppEvent::FilterComplete {
                indices,
                incremental,
            } => {
                if incremental {
                    self.append_filter_results(indices);
                } else {
                    let pattern = self
                        .active_tab()
                        .source
                        .filter
                        .pattern
                        .clone()
                        .unwrap_or_default();
                    self.apply_filter(indices, pattern);
                }
                self.maybe_compute_aggregation();
                if self.active_tab().source.follow_mode
                    && self.active_tab().source.mode != ViewMode::Aggregation
                {
                    self.jump_to_end();
                }
            }
            AppEvent::FilterError(ref err) => {
                self.status_message = Some((format!("Filter error: {}", err), Instant::now()));
                self.active_tab_mut().source.filter.state = FilterState::Inactive;
            }
            _ => {}
        }
    }

    fn handle_file_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::FileModified { new_total, .. } => {
                let tab = self.active_tab_mut();
                tab.source.total_lines = new_total;
                tab.source.rate_tracker.record(new_total);
                if tab.source.mode == ViewMode::Normal {
                    let old = tab.source.line_indices.len();
                    if new_total > old {
                        tab.source.line_indices.extend(old..new_total);
                    }
                }
                if let (Some(ref mut ir), Some(ref path)) =
                    (&mut tab.source.index_reader, &tab.source.source_path)
                {
                    ir.refresh(path);
                }
                let should_jump = self.active_tab().source.follow_mode
                    && self.active_tab().source.mode == ViewMode::Normal
                    && !self.has_start_filter_in_batch;
                if should_jump {
                    self.jump_to_end();
                }
            }
            AppEvent::FileTruncated { new_total } => {
                eprintln!(
                    "File truncated: {} -> {} lines",
                    self.active_tab().source.total_lines,
                    new_total
                );
                self.active_tab_mut().reset_after_truncation(new_total);
            }
            _ => {}
        }
    }

    fn handle_help_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::ShowHelp => self.help_scroll_offset = Some(0),
            AppEvent::HideHelp => self.help_scroll_offset = None,
            AppEvent::ScrollHelpDown => {
                if let Some(offset) = &mut self.help_scroll_offset {
                    *offset = offset.saturating_add(1);
                }
            }
            AppEvent::ScrollHelpUp => {
                if let Some(offset) = &mut self.help_scroll_offset {
                    *offset = offset.saturating_sub(1);
                }
            }
            _ => {}
        }
    }

    fn handle_line_jump_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::StartLineJumpInput => self.start_line_jump_input(),
            AppEvent::LineJumpInputChar(c) => {
                if c.is_ascii_digit() {
                    self.input.input_char(c);
                }
            }
            AppEvent::LineJumpInputBackspace => self.input.input_backspace(),
            AppEvent::LineJumpInputSubmit => {
                if let Ok(line_num) = self.input.buffer.parse::<usize>() {
                    self.jump_to_line(line_num);
                    self.active_tab_mut().source.follow_mode = false;
                }
                self.cancel_line_jump_input();
            }
            AppEvent::LineJumpInputCancel => self.cancel_line_jump_input(),
            _ => {}
        }
    }

    fn handle_history_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::HistoryUp => {
                if let Some((pattern, _mode)) = self.filter.history_up() {
                    self.input.set_content(pattern);
                }
                self.filter.validate_regex(&self.input.buffer);
            }
            AppEvent::HistoryDown => {
                if let Some((pattern, _mode)) = self.filter.history_down() {
                    self.input.set_content(pattern);
                } else if self.filter.pending_at.is_some() || !self.input.buffer.is_empty() {
                    // Back to empty when navigating past newest
                    self.input.clear();
                    self.filter.validate_regex(&self.input.buffer);
                }
                self.filter.validate_regex(&self.input.buffer);
            }
            _ => {}
        }
        FilterOrchestrator::cancel(&mut self.active_tab_mut().source);
        self.filter.schedule_debounce();
    }

    fn handle_view_position_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::EnterZMode => self.input.mode = InputMode::ZPending,
            AppEvent::ExitZMode => self.input.mode = InputMode::Normal,
            AppEvent::CenterView => self.active_tab_mut().center_view(),
            AppEvent::ViewToTop => self.active_tab_mut().view_to_top(),
            AppEvent::ViewToBottom => self.active_tab_mut().view_to_bottom(),
            _ => {}
        }
    }

    fn handle_aggregation_event(&mut self, event: event::AppEvent) {
        use event::AppEvent;
        match event {
            AppEvent::AggregationDown => {
                let tab = self.active_tab_mut();
                if let Some(ref result) = tab.source.aggregation_result {
                    let max = result.groups.len().saturating_sub(1);
                    if tab.aggregation_view.selected_row < max {
                        tab.aggregation_view.selected_row += 1;
                    }
                }
                self.active_tab_mut().aggregation_view.ensure_visible();
            }
            AppEvent::AggregationUp => {
                let tab = self.active_tab_mut();
                tab.aggregation_view.selected_row =
                    tab.aggregation_view.selected_row.saturating_sub(1);
                tab.aggregation_view.ensure_visible();
            }
            AppEvent::AggregationJumpToStart => {
                let tab = self.active_tab_mut();
                tab.aggregation_view.selected_row = 0;
                tab.aggregation_view.scroll_offset = 0;
            }
            AppEvent::AggregationJumpToEnd => {
                let tab = self.active_tab_mut();
                if let Some(ref result) = tab.source.aggregation_result {
                    tab.aggregation_view.selected_row = result.groups.len().saturating_sub(1);
                }
                self.active_tab_mut().aggregation_view.ensure_visible();
            }
            AppEvent::AggregationDrillDown => self.aggregation_drill_down(),
            AppEvent::AggregationBack => self.aggregation_back(),
            _ => {}
        }
    }

    /// Handle a mouse click at the given terminal coordinates
    fn handle_mouse_click(&mut self, column: u16, row: u16) {
        if self.help_scroll_offset.is_some() {
            self.help_scroll_offset = None;
            return;
        }

        match self.input.mode {
            InputMode::ConfirmClose
            | InputMode::EnteringFilter
            | InputMode::EnteringLineJump
            | InputMode::ZPending => return,
            _ => {}
        }

        let sp = self.layout.side_panel_sources;
        let lv = self.layout.log_view;

        if sp.contains_inner(column, row) {
            let inner_row = sp.inner_row(row);
            let items = self.build_source_tree_items();

            if inner_row < items.len() {
                match &items[inner_row] {
                    TreeSelection::CombinedForCategory(cat) => {
                        self.tab_mgr.select_combined_tab(*cat);
                        self.input.mode = InputMode::Normal;
                    }
                    TreeSelection::Category(cat) => {
                        let idx = *cat as usize;
                        self.panel.state.expanded[idx] = !self.panel.state.expanded[idx];
                    }
                    TreeSelection::Item(cat, idx) => {
                        if let Some(tab_idx) = self.tab_mgr.find_tab_index(*cat, *idx) {
                            self.tab_mgr.active = tab_idx;
                            self.tab_mgr.active_combined = None;
                            self.input.mode = InputMode::Normal;
                        }
                    }
                }
            }
            return;
        }

        if lv.contains_inner(column, row) {
            let inner_row = lv.inner_row(row);
            let tab = self.active_tab_mut();
            let scroll_pos = tab.viewport.scroll_position();
            let target_index = scroll_pos + inner_row;

            if target_index < tab.source.line_indices.len() {
                let file_line = tab.source.line_indices[target_index];
                tab.select_line(file_line);
                tab.source.follow_mode = false;
            }

            if self.input.mode == InputMode::SourcePanel {
                self.input.mode = InputMode::Normal;
            }
        }
    }
}

/// Minimal base64 encoder for OSC 52 clipboard
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut result = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0] as u32;
        let b1 = chunk.get(1).copied().unwrap_or(0) as u32;
        let b2 = chunk.get(2).copied().unwrap_or(0) as u32;
        let n = (b0 << 16) | (b1 << 8) | b2;
        result.push(ALPHABET[(n >> 18 & 0x3F) as usize] as char);
        result.push(ALPHABET[(n >> 12 & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            result.push(ALPHABET[(n >> 6 & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
        if chunk.len() > 2 {
            result.push(ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            result.push('=');
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::FilterMode;
    use event::AppEvent;
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

        assert_eq!(app.tab_mgr.tabs.len(), 1);
        assert_eq!(app.tab_mgr.active, 0);
        assert_eq!(app.active_tab().source.total_lines, 3);
        assert!(!app.should_quit);
        assert!(app.help_scroll_offset.is_none());
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

        assert_eq!(app.tab_mgr.tabs.len(), 3);
        assert_eq!(app.tab_mgr.tabs[0].source.total_lines, 2);
        assert_eq!(app.tab_mgr.tabs[1].source.total_lines, 3);
        assert_eq!(app.tab_mgr.tabs[2].source.total_lines, 1);
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

        assert_eq!(app.tab_mgr.active, 0);

        app.select_tab(1);
        assert_eq!(app.tab_mgr.active, 1);

        app.select_tab(2);
        assert_eq!(app.tab_mgr.active, 2);

        app.select_tab(0);
        assert_eq!(app.tab_mgr.active, 0);

        // Invalid selection (out of bounds)
        app.select_tab(10);
        assert_eq!(app.tab_mgr.active, 0);
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

        app.apply_filter(vec![0, 2], "error".to_string());
        assert_eq!(app.active_tab().source.mode, ViewMode::Filtered);

        app.select_tab(1);
        assert_eq!(app.active_tab().source.mode, ViewMode::Normal);
        assert!(app.active_tab().source.filter.pattern.is_none());

        app.select_tab(0);
        assert_eq!(app.active_tab().source.mode, ViewMode::Filtered);
    }

    #[test]
    fn test_navigation_basic() {
        let temp_file = create_temp_log_file(&["1", "2", "3", "4", "5", "6", "7", "8", "9", "10"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert_eq!(app.active_tab().selected_line, 9);

        app.jump_to_start();
        assert_eq!(app.active_tab().selected_line, 0);

        app.scroll_down();
        assert_eq!(app.active_tab().selected_line, 1);

        app.scroll_up();
        assert_eq!(app.active_tab().selected_line, 0);

        app.scroll_up();
        assert_eq!(app.active_tab().selected_line, 0);

        app.jump_to_end();
        assert_eq!(app.active_tab().selected_line, 9);
    }

    #[test]
    fn test_page_navigation() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.jump_to_start();

        app.page_down(10);
        assert_eq!(app.active_tab().selected_line, 10);

        app.page_up(5);
        assert_eq!(app.active_tab().selected_line, 5);
    }

    #[test]
    fn test_filter_application() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        let matching_indices = vec![0, 2];

        app.apply_filter(matching_indices.clone(), "error".to_string());

        assert_eq!(app.active_tab().source.mode, ViewMode::Filtered);
        assert_eq!(app.active_tab().source.line_indices, matching_indices);
        assert_eq!(
            app.active_tab().source.filter.pattern,
            Some("error".to_string())
        );
    }

    #[test]
    fn test_clear_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_filter(vec![0, 2], "error".to_string());
        app.clear_filter();

        assert_eq!(app.active_tab().source.mode, ViewMode::Normal);
        assert!(app.active_tab().source.filter.pattern.is_none());
    }

    #[test]
    fn test_follow_mode_toggle() {
        let temp_file = create_temp_log_file(&["1", "2", "3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert!(app.active_tab().source.follow_mode);

        app.toggle_follow_mode();
        assert!(!app.active_tab().source.follow_mode);

        app.toggle_follow_mode();
        assert!(app.active_tab().source.follow_mode);
    }

    #[test]
    fn test_filter_input_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        assert!(app.is_entering_filter());
        assert!(app.get_input().is_empty());

        app.input.input_char('t');
        app.input.input_char('e');
        app.input.input_char('s');
        app.input.input_char('t');
        assert_eq!(app.get_input(), "test");

        app.input.input_backspace();
        assert_eq!(app.get_input(), "tes");

        app.cancel_filter_input();
        assert!(!app.is_entering_filter());
        assert!(app.get_input().is_empty());
    }

    #[test]
    fn test_help_mode_toggle() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();
        assert!(app.help_scroll_offset.is_none());

        app.apply_event(AppEvent::ShowHelp);
        assert!(app.help_scroll_offset.is_some());

        app.apply_event(AppEvent::HideHelp);
        assert!(app.help_scroll_offset.is_none());
    }

    #[test]
    fn test_line_jump_input_mode() {
        let temp_file = create_temp_log_file(&["1", "2", "3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_line_jump_input();
        assert!(app.is_entering_line_jump());
        assert_eq!(app.get_input(), "");

        app.cancel_line_jump_input();
        assert!(!app.is_entering_line_jump());
        assert_eq!(app.get_input(), "");
    }

    #[test]
    fn test_add_to_history() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter
            .add_to_history("TEST_HIST_ERROR_12345".to_string(), FilterMode::plain());
        app.filter
            .add_to_history("TEST_HIST_WARN_12345".to_string(), FilterMode::plain());
        app.filter
            .add_to_history("TEST_HIST_INFO_12345".to_string(), FilterMode::plain());

        // History is private, so we test via history navigation
        // 3 entries added successfully if we can navigate through them
        app.start_filter_input();
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "TEST_HIST_INFO_12345");
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "TEST_HIST_WARN_12345");
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "TEST_HIST_ERROR_12345");
    }

    #[test]
    fn test_add_to_history_skips_duplicates() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter
            .add_to_history("ERROR_DUP_TEST".to_string(), FilterMode::plain());
        app.filter
            .add_to_history("ERROR_DUP_TEST".to_string(), FilterMode::plain());

        // Should only have 1 entry - navigating up once then again should stay on same
        app.start_filter_input();
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "ERROR_DUP_TEST");
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "ERROR_DUP_TEST"); // stays on same (only 1 entry)
    }

    #[test]
    fn test_add_to_history_same_pattern_different_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter
            .add_to_history("error_mode_test".to_string(), FilterMode::plain());
        app.filter
            .add_to_history("error_mode_test".to_string(), FilterMode::regex());

        // 2 entries with different modes
        app.start_filter_input();
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "error_mode_test");
        assert!(app.filter.current_mode.is_regex());
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "error_mode_test");
        assert!(!app.filter.current_mode.is_regex());
    }

    #[test]
    fn test_history_navigation() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter
            .add_to_history("ERROR".to_string(), FilterMode::plain());
        app.filter
            .add_to_history("WARN".to_string(), FilterMode::regex());
        app.filter
            .add_to_history("INFO".to_string(), FilterMode::plain());

        app.start_filter_input();

        // Navigate up (most recent - INFO with plain mode)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input.buffer, "INFO");
        assert!(!app.filter.current_mode.is_regex());

        // Navigate up again (older - WARN with regex mode)
        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input.buffer, "WARN");
        assert!(app.filter.current_mode.is_regex());

        // Navigate down (back to INFO with plain mode)
        app.apply_event(AppEvent::HistoryDown);
        assert_eq!(app.input.buffer, "INFO");
        assert!(!app.filter.current_mode.is_regex());
    }

    #[test]
    fn test_history_navigation_restores_case_sensitivity() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter.add_to_history(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: false,
            },
        );
        app.filter.add_to_history(
            "Error".to_string(),
            FilterMode::Plain {
                case_sensitive: true,
            },
        );

        app.start_filter_input();

        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input.buffer, "Error");
        assert!(app.filter.current_mode.is_case_sensitive());

        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.input.buffer, "error");
        assert!(!app.filter.current_mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_filter_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert!(!app.filter.current_mode.is_regex());
        assert!(!app.filter.current_mode.is_query());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_regex());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_query());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.filter.current_mode.is_regex());
        assert!(!app.filter.current_mode.is_query());
    }

    #[test]
    fn test_toggle_case_sensitivity() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert!(!app.filter.current_mode.is_case_sensitive());

        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(app.filter.current_mode.is_case_sensitive());

        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(!app.filter.current_mode.is_case_sensitive());
    }

    #[test]
    fn test_cycle_mode_case_sensitivity_behavior() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::ToggleCaseSensitivity);
        assert!(app.filter.current_mode.is_case_sensitive());
        assert!(!app.filter.current_mode.is_regex());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_regex());
        assert!(app.filter.current_mode.is_case_sensitive());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_query());
        assert!(!app.filter.current_mode.is_case_sensitive());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.filter.current_mode.is_regex());
        assert!(!app.filter.current_mode.is_query());
        assert!(!app.filter.current_mode.is_case_sensitive());
    }

    #[test]
    fn test_regex_validation_valid_regex() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_regex());

        app.apply_event(AppEvent::FilterInputChar('e'));
        app.apply_event(AppEvent::FilterInputChar('r'));
        app.apply_event(AppEvent::FilterInputChar('r'));
        app.apply_event(AppEvent::FilterInputChar('.'));
        app.apply_event(AppEvent::FilterInputChar('*'));

        assert!(app.is_regex_valid());
        assert!(app.filter.regex_error.is_none());
    }

    #[test]
    fn test_regex_validation_invalid_regex() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::ToggleFilterMode);

        app.apply_event(AppEvent::FilterInputChar('['));
        app.apply_event(AppEvent::FilterInputChar('a'));

        assert!(!app.is_regex_valid());
        assert!(app.filter.regex_error.is_some());
    }

    #[test]
    fn test_regex_validation_clears_on_backspace() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::ToggleFilterMode);
        app.apply_event(AppEvent::FilterInputChar('['));
        assert!(!app.is_regex_valid());

        app.apply_event(AppEvent::FilterInputBackspace);
        assert!(app.is_regex_valid());
    }

    #[test]
    fn test_regex_validation_not_checked_in_plain_mode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        assert!(!app.filter.current_mode.is_regex());

        app.apply_event(AppEvent::FilterInputChar('['));
        app.apply_event(AppEvent::FilterInputChar('a'));

        assert!(app.is_regex_valid());
        assert!(app.filter.regex_error.is_none());
    }

    #[test]
    fn test_regex_validation_on_mode_toggle() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::FilterInputChar('['));
        assert!(app.is_regex_valid());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_regex());
        assert!(!app.is_regex_valid());
        assert!(app.filter.regex_error.is_some());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(app.filter.current_mode.is_query());
        assert!(app.filter.regex_error.is_none());
        assert!(app.filter.query_error.is_some());

        app.apply_event(AppEvent::ToggleFilterMode);
        assert!(!app.filter.current_mode.is_regex());
        assert!(!app.filter.current_mode.is_query());
        assert!(app.is_regex_valid());
    }

    #[test]
    fn test_tab_events() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);

        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        assert_eq!(app.tab_mgr.active, 0);

        app.apply_event(AppEvent::SelectTab(1));
        assert_eq!(app.tab_mgr.active, 1);

        app.apply_event(AppEvent::SelectTab(0));
        assert_eq!(app.tab_mgr.active, 0);

        app.apply_event(AppEvent::SelectTab(1));
        assert_eq!(app.tab_mgr.active, 1);
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
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));
        assert_eq!(app.get_cursor_position(), 3);

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 2);

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 1);

        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);
    }

    #[test]
    fn test_cursor_at_boundaries() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));

        app.apply_event(AppEvent::CursorLeft);
        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 0);

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 0);

        app.apply_event(AppEvent::CursorRight);
        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);

        app.apply_event(AppEvent::CursorRight);
        assert_eq!(app.get_cursor_position(), 2);
    }

    #[test]
    fn test_cursor_home_end() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 2);

        app.apply_event(AppEvent::CursorHome);
        assert_eq!(app.get_cursor_position(), 0);

        app.apply_event(AppEvent::CursorEnd);
        assert_eq!(app.get_cursor_position(), 3);
    }

    #[test]
    fn test_insert_at_cursor() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('c'));

        app.apply_event(AppEvent::CursorLeft);

        app.apply_event(AppEvent::FilterInputChar('b'));
        assert_eq!(app.get_input(), "abc");
        assert_eq!(app.get_cursor_position(), 2);
    }

    #[test]
    fn test_backspace_at_cursor() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));
        app.apply_event(AppEvent::FilterInputChar('b'));
        app.apply_event(AppEvent::FilterInputChar('c'));

        app.apply_event(AppEvent::CursorLeft);

        app.apply_event(AppEvent::FilterInputBackspace);
        assert_eq!(app.get_input(), "ac");
        assert_eq!(app.get_cursor_position(), 1);
    }

    #[test]
    fn test_backspace_at_start() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('a'));

        app.apply_event(AppEvent::CursorHome);
        assert_eq!(app.get_cursor_position(), 0);

        app.apply_event(AppEvent::FilterInputBackspace);
        assert_eq!(app.get_input(), "a");
        assert_eq!(app.get_cursor_position(), 0);
    }

    #[test]
    fn test_cursor_with_unicode() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.start_filter_input();
        app.apply_event(AppEvent::FilterInputChar('日'));
        app.apply_event(AppEvent::FilterInputChar('本'));
        assert_eq!(app.get_input(), "日本");
        assert_eq!(app.get_cursor_position(), 6);

        app.apply_event(AppEvent::CursorLeft);
        assert_eq!(app.get_cursor_position(), 3);

        app.apply_event(AppEvent::FilterInputChar('語'));
        assert_eq!(app.get_input(), "日語本");
    }

    #[test]
    fn test_history_sets_cursor_to_end() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.filter
            .add_to_history("error".to_string(), FilterMode::plain());

        app.start_filter_input();
        assert_eq!(app.get_cursor_position(), 0);

        app.apply_event(AppEvent::HistoryUp);
        assert_eq!(app.get_input(), "error");
        assert_eq!(app.get_cursor_position(), 5);
    }

    #[test]
    fn test_toggle_line_expansion_event() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::JumpToStart);

        assert!(app.active_tab().expansion.expanded_lines.is_empty());

        app.apply_event(AppEvent::ToggleLineExpansion);
        assert!(app.active_tab().is_line_expanded(0));

        app.apply_event(AppEvent::ToggleLineExpansion);
        assert!(!app.active_tab().is_line_expanded(0));
    }

    #[test]
    fn test_collapse_all_event() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::JumpToStart);

        app.apply_event(AppEvent::ToggleLineExpansion);
        app.apply_event(AppEvent::ScrollDown);
        app.apply_event(AppEvent::ToggleLineExpansion);

        assert_eq!(app.active_tab().expansion.expanded_lines.len(), 2);

        app.apply_event(AppEvent::CollapseAll);
        assert!(app.active_tab().expansion.expanded_lines.is_empty());
    }

    #[test]
    fn test_copy_selected_line_sets_status_message() {
        let temp_file = create_temp_log_file(&["hello world", "second line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::JumpToStart);
        assert_eq!(app.active_tab().selected_line, 0);

        app.apply_event(AppEvent::CopySelectedLine);
        assert!(app.status_message.is_some());
        let (msg, _) = app.status_message.as_ref().unwrap();
        assert!(msg.contains("Copied:"));
        assert!(msg.contains("hello world"));
    }

    #[test]
    fn test_copy_selected_line_noop_on_empty() {
        let temp_file = create_temp_log_file(&[]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::CopySelectedLine);
        assert!(app.status_message.is_none());
    }

    #[test]
    fn test_copy_selected_line_strips_ansi() {
        let temp_file = create_temp_log_file(&["\x1b[31mred text\x1b[0m"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::CopySelectedLine);
        assert!(app.status_message.is_some());
        let (msg, _) = app.status_message.as_ref().unwrap();
        assert!(msg.contains("red text"));
        assert!(!msg.contains("\x1b"));
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

        assert_eq!(app.input.mode, InputMode::Normal);
        app.apply_event(AppEvent::CloseCurrentTab);

        assert_eq!(app.input.mode, InputMode::ConfirmClose);
        assert!(app.pending_close_tab.is_some());
        let (idx, name) = app.pending_close_tab.as_ref().unwrap();
        assert_eq!(*idx, 0);
        assert_eq!(*name, app.tab_mgr.tabs[0].source.name);
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
        assert_eq!(app.tab_mgr.tabs.len(), 2);
        assert_eq!(app.input.mode, InputMode::ConfirmClose);

        app.apply_event(AppEvent::ConfirmCloseTab);
        assert_eq!(app.tab_mgr.tabs.len(), 1);
        assert_eq!(app.input.mode, InputMode::Normal);
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
        assert_eq!(app.input.mode, InputMode::ConfirmClose);

        app.apply_event(AppEvent::CancelCloseTab);
        assert_eq!(app.tab_mgr.tabs.len(), 2);
        assert_eq!(app.input.mode, InputMode::Normal);
        assert!(app.pending_close_tab.is_none());
    }

    #[test]
    fn test_build_source_tree_items_returns_correct_items() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);
        let file3 = create_temp_log_file(&["c"]);
        let app = App::new(
            vec![
                file1.path().to_path_buf(),
                file2.path().to_path_buf(),
                file3.path().to_path_buf(),
            ],
            false,
        )
        .unwrap();

        let items = app.build_source_tree_items();
        assert_eq!(items.len(), 4);
        assert_eq!(items[0], TreeSelection::Category(SourceType::File));
        assert_eq!(items[1], TreeSelection::Item(SourceType::File, 0));
        assert_eq!(items[2], TreeSelection::Item(SourceType::File, 1));
        assert_eq!(items[3], TreeSelection::Item(SourceType::File, 2));
    }

    #[test]
    fn test_build_source_tree_items_respects_collapsed() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        app.panel.state.expanded[SourceType::File as usize] = false;
        let items = app.build_source_tree_items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0], TreeSelection::Category(SourceType::File));
    }

    #[test]
    fn test_mouse_click_side_panel_selects_tab() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        assert_eq!(app.tab_mgr.active, 0);

        app.layout.side_panel_sources = LayoutRect {
            x: 0,
            y: 0,
            width: 32,
            height: 20,
        };

        app.apply_event(AppEvent::MouseClick { column: 5, row: 3 });

        assert_eq!(app.tab_mgr.active, 1);
        assert_eq!(app.input.mode, InputMode::Normal);
    }

    #[test]
    fn test_mouse_click_side_panel_toggles_category() {
        let file1 = create_temp_log_file(&["a"]);
        let mut app = App::new(vec![file1.path().to_path_buf()], false).unwrap();

        app.layout.side_panel_sources = LayoutRect {
            x: 0,
            y: 0,
            width: 32,
            height: 20,
        };

        assert!(app.panel.state.expanded[SourceType::File as usize]);
        app.apply_event(AppEvent::MouseClick { column: 5, row: 1 });
        assert!(!app.panel.state.expanded[SourceType::File as usize]);

        app.apply_event(AppEvent::MouseClick { column: 5, row: 1 });
        assert!(app.panel.state.expanded[SourceType::File as usize]);
    }

    #[test]
    fn test_mouse_click_log_view_selects_line() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3", "line4", "line5"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::JumpToStart);

        app.layout.log_view = LayoutRect {
            x: 32,
            y: 0,
            width: 80,
            height: 20,
        };

        app.apply_event(AppEvent::MouseClick { column: 40, row: 3 });

        assert_eq!(app.active_tab().selected_line, 2);
        assert!(!app.active_tab().source.follow_mode);
    }

    #[test]
    fn test_mouse_click_dismisses_help() {
        let temp_file = create_temp_log_file(&["line"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_event(AppEvent::ShowHelp);
        assert!(app.help_scroll_offset.is_some());

        app.apply_event(AppEvent::MouseClick {
            column: 10,
            row: 10,
        });
        assert!(app.help_scroll_offset.is_none());
    }

    #[test]
    fn test_mouse_click_ignored_during_filter_input() {
        let temp_file = create_temp_log_file(&["line1", "line2"]);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.layout.log_view = LayoutRect {
            x: 32,
            y: 0,
            width: 80,
            height: 20,
        };

        app.apply_event(AppEvent::StartFilterInput);
        assert_eq!(app.input.mode, InputMode::EnteringFilter);

        app.apply_event(AppEvent::MouseClick { column: 40, row: 3 });
        assert_eq!(app.input.mode, InputMode::EnteringFilter);
    }

    #[test]
    fn test_mouse_click_ignored_during_confirm_close() {
        let file1 = create_temp_log_file(&["line1"]);
        let file2 = create_temp_log_file(&["line2"]);
        let mut app = App::new(
            vec![file1.path().to_path_buf(), file2.path().to_path_buf()],
            false,
        )
        .unwrap();

        app.layout.side_panel_sources = LayoutRect {
            x: 0,
            y: 0,
            width: 32,
            height: 20,
        };

        app.apply_event(AppEvent::CloseCurrentTab);
        assert_eq!(app.input.mode, InputMode::ConfirmClose);

        let active_before = app.tab_mgr.active;
        app.apply_event(AppEvent::MouseClick { column: 5, row: 3 });
        assert_eq!(app.input.mode, InputMode::ConfirmClose);
        assert_eq!(app.tab_mgr.active, active_before);
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

        app.tab_mgr.active = 1;
        let original_name = app.tab_mgr.tabs[1].source.name.clone();
        app.apply_event(AppEvent::CloseCurrentTab);
        assert_eq!(app.pending_close_tab.as_ref().unwrap().1, original_name);

        app.tab_mgr.tabs[1].source.name = "different_name".to_string();

        app.apply_event(AppEvent::ConfirmCloseTab);
        assert_eq!(app.tab_mgr.tabs.len(), 3);
    }

    #[test]
    fn test_tab_index_for_shortcut_same_type() {
        let file1 = create_temp_log_file(&["a"]);
        let file2 = create_temp_log_file(&["b"]);
        let file3 = create_temp_log_file(&["c"]);

        let app = App::new(
            vec![
                file1.path().to_path_buf(),
                file2.path().to_path_buf(),
                file3.path().to_path_buf(),
            ],
            false,
        )
        .unwrap();

        assert_eq!(app.tab_index_for_shortcut(0), Some(0));
        assert_eq!(app.tab_index_for_shortcut(1), Some(1));
        assert_eq!(app.tab_index_for_shortcut(2), Some(2));
        assert_eq!(app.tab_index_for_shortcut(3), None);
    }

    #[test]
    fn test_tab_index_for_shortcut_mixed_types() {
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

        app.tab_mgr.tabs[1].config_source_type = Some(SourceType::ProjectSource);
        app.tab_mgr.tabs[2].config_source_type = Some(SourceType::GlobalSource);

        assert_eq!(app.tab_index_for_shortcut(0), Some(1));
        assert_eq!(app.tab_index_for_shortcut(1), Some(2));
        assert_eq!(app.tab_index_for_shortcut(2), Some(0));
        assert_eq!(app.tab_index_for_shortcut(3), None);
    }

    #[test]
    fn test_select_tab_uses_shortcut_mapping() {
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

        app.tab_mgr.tabs[1].config_source_type = Some(SourceType::ProjectSource);

        app.apply_event(AppEvent::SelectTab(0));
        assert_eq!(app.tab_mgr.active, 1);

        app.apply_event(AppEvent::SelectTab(1));
        assert_eq!(app.tab_mgr.active, 0);

        app.apply_event(AppEvent::SelectTab(2));
        assert_eq!(app.tab_mgr.active, 2);
    }
}
