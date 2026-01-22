use crate::app::{FilterState, ViewMode};
use crate::filter::engine::FilterProgress;
use crate::reader::{file_reader::FileReader, stream_reader::StreamReader, LogReader};
use crate::viewport::Viewport;
use crate::watcher::FileWatcher;
use anyhow::{Context, Result};
use std::fs::File;
use std::path::PathBuf;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// Per-tab state for viewing a single log source
pub struct TabState {
    /// Display name (filename)
    pub name: String,
    /// Current view mode
    pub mode: ViewMode,
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
    /// Current filter pattern (if any)
    pub filter_pattern: Option<String>,
    /// Follow mode - auto-scroll to latest logs
    pub follow_mode: bool,
    /// Last line number that was filtered (for incremental filtering)
    pub last_filtered_line: usize,
    /// Per-tab reader
    pub reader: Arc<Mutex<dyn LogReader + Send>>,
    /// Per-tab file watcher
    pub watcher: Option<FileWatcher>,
    /// Per-tab filter receiver
    pub filter_receiver: Option<Receiver<FilterProgress>>,
    /// Whether the current filter operation is incremental
    pub is_incremental_filter: bool,
    /// Viewport for anchor-based scroll/selection management
    pub viewport: Viewport,
    /// Original line when filter mode started (for restoring on Esc)
    pub filter_origin_line: Option<usize>,
}

impl TabState {
    /// Create a new tab from a file path
    pub fn new(path: PathBuf, watch: bool) -> Result<Self> {
        // Check file type to determine if it's a regular file or pipe/FIFO
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Failed to get metadata: {}", path.display()))?;
        let is_regular_file = metadata.file_type().is_file();

        // Open the file
        let file =
            File::open(&path).with_context(|| format!("Failed to open: {}", path.display()))?;

        let (reader, watcher): (Arc<Mutex<dyn LogReader + Send>>, Option<FileWatcher>) =
            if is_regular_file {
                // Regular file - close this handle and use FileReader (which opens its own)
                drop(file);
                let file_reader = FileReader::new(&path)?;
                let watcher = if watch {
                    FileWatcher::new(&path).ok()
                } else {
                    None
                };
                (Arc::new(Mutex::new(file_reader)), watcher)
            } else {
                // Pipe/FIFO - use the already-open handle with StreamReader
                let stream_reader = StreamReader::from_reader(file)
                    .with_context(|| format!("Failed to read stream: {}", path.display()))?;
                // No file watching for streams
                (Arc::new(Mutex::new(stream_reader)), None)
            };

        let total_lines = reader.lock().unwrap().total_lines();

        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        let line_indices = (0..total_lines).collect();

        Ok(Self {
            name,
            mode: ViewMode::Normal,
            total_lines,
            line_indices,
            scroll_position: 0,
            selected_line: 0,
            filter_state: FilterState::Inactive,
            filter_pattern: None,
            follow_mode: false,
            last_filtered_line: 0,
            reader,
            watcher,
            filter_receiver: None,
            is_incremental_filter: false,
            viewport: Viewport::new(0),
            filter_origin_line: None,
        })
    }

    /// Create a new tab from stdin
    pub fn from_stdin() -> Result<Self> {
        let stdin = std::io::stdin();
        let stream_reader =
            StreamReader::from_reader(stdin.lock()).context("Failed to read from stdin")?;

        let total_lines = stream_reader.total_lines();
        let line_indices = (0..total_lines).collect();

        Ok(Self {
            name: "<stdin>".to_string(),
            mode: ViewMode::Normal,
            total_lines,
            line_indices,
            scroll_position: 0,
            selected_line: 0,
            filter_state: FilterState::Inactive,
            filter_pattern: None,
            follow_mode: false,
            last_filtered_line: 0,
            reader: Arc::new(Mutex::new(stream_reader)),
            watcher: None,
            filter_receiver: None,
            is_incremental_filter: false,
            viewport: Viewport::new(0),
            filter_origin_line: None,
        })
    }

    /// Get the number of visible lines
    pub fn visible_line_count(&self) -> usize {
        self.line_indices.len()
    }

    /// Sync old fields from viewport (for backward compatibility during migration)
    fn sync_from_viewport(&mut self) {
        // Find the index of viewport's anchor_line in line_indices
        let anchor_line = self.viewport.selected_line();
        if let Ok(idx) = self.line_indices.binary_search(&anchor_line) {
            self.selected_line = idx;
        } else {
            // If not found exactly, find nearest
            self.selected_line = self
                .line_indices
                .iter()
                .position(|&l| l >= anchor_line)
                .unwrap_or(self.line_indices.len().saturating_sub(1));
        }
    }

    /// Scroll down by one line
    pub fn scroll_down(&mut self) {
        self.viewport.move_selection(1, &self.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll up by one line
    pub fn scroll_up(&mut self) {
        self.viewport.move_selection(-1, &self.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll down by page
    pub fn page_down(&mut self, page_size: usize) {
        self.viewport
            .move_selection(page_size as i32, &self.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll up by page
    pub fn page_up(&mut self, page_size: usize) {
        self.viewport
            .move_selection(-(page_size as i32), &self.line_indices);
        self.sync_from_viewport();
    }

    /// Mouse scroll down - moves viewport and selection together
    pub fn mouse_scroll_down(&mut self, lines: usize, _visible_height: usize) {
        self.viewport
            .scroll_with_selection(lines as i32, &self.line_indices);
        self.sync_from_viewport();
    }

    /// Mouse scroll up - moves viewport and selection together
    pub fn mouse_scroll_up(&mut self, lines: usize, _visible_height: usize) {
        self.viewport
            .scroll_with_selection(-(lines as i32), &self.line_indices);
        self.sync_from_viewport();
    }

    /// Apply filter results (for full filtering)
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        // Capture screen offset BEFORE changing line_indices
        let screen_offset = self.viewport.get_screen_offset(&self.line_indices);

        self.line_indices = matching_indices;
        self.mode = ViewMode::Filtered;
        self.filter_pattern = Some(pattern);
        self.filter_state = FilterState::Complete {
            matches: self.line_indices.len(),
        };
        self.last_filtered_line = self.total_lines;

        // If we have an origin line (from when filtering started), select nearest match
        // while preserving screen position
        if let Some(origin) = self.filter_origin_line {
            if !self.line_indices.is_empty() {
                // Find the match nearest to origin
                let nearest_line = self.find_nearest_line(origin);
                // Jump to it while keeping same screen offset
                self.viewport.jump_to_line_at_offset(
                    nearest_line,
                    screen_offset,
                    &self.line_indices,
                );
            }
        }
        // Otherwise viewport's anchor_line will find nearest automatically via resolve()

        // Sync old fields from viewport
        self.sync_from_viewport();
    }

    /// Find the line in line_indices nearest to target
    fn find_nearest_line(&self, target: usize) -> usize {
        if self.line_indices.is_empty() {
            return target;
        }

        // Binary search to find insertion point
        match self.line_indices.binary_search(&target) {
            Ok(_) => target, // Exact match
            Err(pos) => {
                // Find closest between pos-1 and pos
                if pos == 0 {
                    self.line_indices[0]
                } else if pos >= self.line_indices.len() {
                    self.line_indices[self.line_indices.len() - 1]
                } else {
                    let before = self.line_indices[pos - 1];
                    let after = self.line_indices[pos];
                    if target - before <= after - target {
                        before
                    } else {
                        after
                    }
                }
            }
        }
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
        self.line_indices = (0..self.total_lines).collect();
        self.mode = ViewMode::Normal;
        self.filter_pattern = None;
        self.filter_state = FilterState::Inactive;

        // Restore to origin line if set (where user was before filtering)
        if let Some(origin) = self.filter_origin_line.take() {
            self.viewport.jump_to_line(origin);
        } else {
            // Preserve screen offset - keep selection at same position on screen
            self.viewport.preserve_screen_offset(&self.line_indices);
        }
        self.sync_from_viewport();
    }

    /// Jump to a specific line number (1-indexed)
    pub fn jump_to_line(&mut self, line_number: usize) {
        if line_number == 0 || self.line_indices.is_empty() {
            return;
        }

        // Convert 1-indexed line number to actual file line index (0-indexed)
        let target_line = line_number.saturating_sub(1);

        // Use viewport's jump_to_line - it handles finding nearest if not in view
        self.viewport.jump_to_line(target_line);
        self.sync_from_viewport();
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
        self.viewport.jump_to_end(&self.line_indices);
        self.sync_from_viewport();
    }

    /// Jump to the beginning of the log
    pub fn jump_to_start(&mut self) {
        self.viewport.jump_to_start(&self.line_indices);
        self.sync_from_viewport();
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
    fn test_tab_creation() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        assert_eq!(tab.total_lines, 3);
        assert_eq!(tab.selected_line, 0);
        assert_eq!(tab.scroll_position, 0);
        assert_eq!(tab.mode, ViewMode::Normal);
        assert!(!tab.follow_mode);
    }

    #[test]
    fn test_tab_name_extraction() {
        let temp_file = create_temp_log_file(&["line1"]);
        let tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Name should be extracted from the filename
        assert!(!tab.name.is_empty());
    }

    #[test]
    fn test_navigation() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3", "line4", "line5"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Scroll down
        tab.scroll_down();
        assert_eq!(tab.selected_line, 1);

        // Scroll up
        tab.scroll_up();
        assert_eq!(tab.selected_line, 0);

        // Can't scroll above 0
        tab.scroll_up();
        assert_eq!(tab.selected_line, 0);

        // Jump to end
        tab.jump_to_end();
        assert_eq!(tab.selected_line, 4);

        // Jump to start
        tab.jump_to_start();
        assert_eq!(tab.selected_line, 0);
    }

    #[test]
    fn test_filter_application() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        tab.apply_filter(vec![0, 2], "error".to_string());

        assert_eq!(tab.mode, ViewMode::Filtered);
        assert_eq!(tab.line_indices, vec![0, 2]);
        assert_eq!(tab.filter_pattern, Some("error".to_string()));
    }

    #[test]
    fn test_clear_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        tab.apply_filter(vec![0, 2], "error".to_string());
        tab.clear_filter();

        assert_eq!(tab.mode, ViewMode::Normal);
        assert_eq!(tab.line_indices.len(), 4);
        assert!(tab.filter_pattern.is_none());
    }

    #[test]
    fn test_follow_mode() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        assert!(!tab.follow_mode);

        tab.toggle_follow_mode();
        assert!(tab.follow_mode);
        assert_eq!(tab.selected_line, 2); // Should jump to end

        tab.toggle_follow_mode();
        assert!(!tab.follow_mode);
    }

    #[test]
    fn test_page_navigation() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        tab.page_down(20);
        assert_eq!(tab.selected_line, 20);

        tab.page_up(10);
        assert_eq!(tab.selected_line, 10);
    }

    #[test]
    fn test_jump_to_line() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3", "line4", "line5"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to line 3 (1-indexed)
        tab.jump_to_line(3);
        assert_eq!(tab.selected_line, 2); // 0-indexed

        // Jump to line 1
        tab.jump_to_line(1);
        assert_eq!(tab.selected_line, 0);

        // Jump beyond total lines
        tab.jump_to_line(100);
        assert_eq!(tab.selected_line, 4); // Last line
    }

    #[test]
    fn test_mouse_scroll() {
        let lines: Vec<&str> = (0..50).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Position selection at line 5 using proper method
        tab.page_down(5);
        assert_eq!(tab.selected_line, 5);

        // Mouse scroll down - moves both viewport and selection together
        tab.mouse_scroll_down(3, 20);
        assert_eq!(tab.selected_line, 8); // 5 + 3

        // Mouse scroll up
        tab.mouse_scroll_up(2, 20);
        assert_eq!(tab.selected_line, 6); // 8 - 2
    }
}
