use crate::app::{FilterState, ViewMode};
use crate::filter::engine::FilterProgress;
use crate::reader::{file_reader::FileReader, stream_reader::StreamReader, LogReader};
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
    /// Full path to the file (used for tooltips, title displays)
    #[allow(dead_code)]
    pub path: PathBuf,
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
    /// Skip scroll adjustment on next render (set by mouse scroll)
    pub skip_scroll_adjustment: bool,
    /// Per-tab reader
    pub reader: Arc<Mutex<dyn LogReader + Send>>,
    /// Per-tab file watcher
    pub watcher: Option<FileWatcher>,
    /// Per-tab filter receiver
    pub filter_receiver: Option<Receiver<FilterProgress>>,
    /// Whether the current filter operation is incremental
    pub is_incremental_filter: bool,
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
            path,
            mode: ViewMode::Normal,
            total_lines,
            line_indices,
            scroll_position: 0,
            selected_line: 0,
            filter_state: FilterState::Inactive,
            filter_pattern: None,
            follow_mode: false,
            last_filtered_line: 0,
            skip_scroll_adjustment: false,
            reader,
            watcher,
            filter_receiver: None,
            is_incremental_filter: false,
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
            path: PathBuf::from("-"),
            mode: ViewMode::Normal,
            total_lines,
            line_indices,
            scroll_position: 0,
            selected_line: 0,
            filter_state: FilterState::Inactive,
            filter_pattern: None,
            follow_mode: false,
            last_filtered_line: 0,
            skip_scroll_adjustment: false,
            reader: Arc::new(Mutex::new(stream_reader)),
            watcher: None,
            filter_receiver: None,
            is_incremental_filter: false,
        })
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

        // Mouse scroll down
        tab.selected_line = 5;
        tab.scroll_position = 0;
        tab.mouse_scroll_down(3, 20);
        assert_eq!(tab.scroll_position, 3);
        assert_eq!(tab.selected_line, 8);

        // Mouse scroll up
        tab.mouse_scroll_up(2, 20);
        assert_eq!(tab.scroll_position, 1);
        assert_eq!(tab.selected_line, 6);
    }
}
