use super::viewport::Viewport;
use crate::app::{FilterState, SourceType, ViewMode};
use crate::config;
use crate::index::reader::IndexReader;
use crate::log_source::calculate_index_size;
use crate::reader::{
    file_reader::FileReader, stream_reader::StreamReader, LogReader, StreamableReader,
};
use crate::source::{
    check_source_status, check_source_status_in_dir, DiscoveredSource, SourceLocation,
};
use crate::watcher::FileWatcher;
use anyhow::{Context, Result};
use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

// Re-export FilterConfig so existing `use crate::tab::FilterConfig` still works
pub use crate::log_source::FilterConfig;
// Re-export LogSource for convenience
pub use crate::log_source::LogSource;

/// Batch size for sending lines from background reader
const STREAM_BATCH_SIZE: usize = 10_000;

/// Messages sent from the background stream reader thread
#[derive(Debug)]
pub enum StreamMessage {
    /// A batch of lines has been read
    Lines(Vec<String>),
    /// Reading is complete
    Complete,
    /// An error occurred while reading
    Error(String),
}

/// Mode for expanding log entries
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum ExpandMode {
    #[default]
    Multiple, // Allow multiple expanded entries
    Single, // Only one expanded at a time
}

/// Line expansion state for a tab
#[derive(Default)]
pub struct ExpansionState {
    /// Set of expanded line numbers (file line numbers, not indices)
    pub expanded_lines: HashSet<usize>,
    /// Mode for expanding (Multiple or Single)
    pub mode: ExpandMode,
}

/// Per-tab state for viewing a single log source.
///
/// Contains a `LogSource` (domain core) plus TUI-specific state
/// (viewport, expansion, watcher, stream handling).
pub struct TabState {
    /// Domain core: reader, filter, line indices, source metadata
    pub source: LogSource,
    /// Current scroll position (synced from viewport for compatibility)
    pub scroll_position: usize,
    /// Currently selected line index (synced from viewport for compatibility)
    pub selected_line: usize,
    /// Per-tab file watcher
    pub watcher: Option<FileWatcher>,
    /// Viewport for anchor-based scroll/selection management
    pub viewport: Viewport,
    /// Line expansion state
    pub expansion: ExpansionState,
    /// Stream writer handle for stream-specific operations (append, mark_complete).
    /// Only set for stdin/pipe tabs. Uses `StreamableReader` trait (ISP).
    stream_writer: Option<Arc<Mutex<dyn StreamableReader>>>,
    /// Receiver for background stream loading (pipes/stdin)
    pub stream_receiver: Option<Receiver<StreamMessage>>,
    /// Source type from config (ProjectSource or GlobalSource)
    pub config_source_type: Option<SourceType>,
}

impl TabState {
    /// Check if a line is expanded (test helper)
    #[cfg(test)]
    pub fn is_line_expanded(&self, file_line_number: usize) -> bool {
        self.expansion.expanded_lines.contains(&file_line_number)
    }

    /// Get the file path for this tab (None for stdin/pipe tabs).
    pub fn file_path(&self) -> Option<&std::path::Path> {
        self.source.source_path.as_deref()
    }

    /// Get the source type for this tab (ProjectSource, GlobalSource, Global, File, or Pipe)
    pub fn source_type(&self) -> SourceType {
        // Config source type takes precedence
        if let Some(source_type) = self.config_source_type {
            return source_type;
        }
        if self.source.source_status.is_some() {
            SourceType::Global
        } else if self.source.source_path.is_some() {
            SourceType::File
        } else {
            SourceType::Pipe
        }
    }

    /// Create a new tab from a file path
    pub fn new(path: PathBuf, watch: bool) -> Result<Self> {
        // Check file type to determine if it's a regular file or pipe/FIFO
        let metadata = std::fs::metadata(&path)
            .with_context(|| format!("Failed to get metadata: {}", path.display()))?;
        let is_regular_file = metadata.file_type().is_file();

        // Open the file
        let file =
            File::open(&path).with_context(|| format!("Failed to open: {}", path.display()))?;

        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| path.to_string_lossy().to_string());

        if is_regular_file {
            // Regular file - open reader + index directly
            drop(file);
            let file_reader = FileReader::new(&path)?;
            let index_reader = IndexReader::open(&path);
            let file_size = Some(metadata.len());
            let index_size = index_reader
                .as_ref()
                .and_then(|_| calculate_index_size(&path));

            let watcher = if watch {
                FileWatcher::new(&path).ok()
            } else {
                None
            };

            let total_lines = file_reader.total_lines();
            let line_indices = (0..total_lines).collect();
            let selected_line = total_lines.saturating_sub(1);

            Ok(Self {
                source: LogSource {
                    name,
                    source_path: Some(path),
                    mode: ViewMode::Normal,
                    total_lines,
                    line_indices,
                    follow_mode: true,
                    reader: Arc::new(Mutex::new(file_reader)),
                    filter: FilterConfig::default(),
                    source_status: None,
                    disabled: false,
                    file_size,
                    index_reader,
                    index_size,
                },
                scroll_position: 0,
                selected_line,
                watcher,
                viewport: Viewport::new(selected_line),
                expansion: ExpansionState::default(),
                stream_writer: None,
                stream_receiver: None,
                config_source_type: None,
            })
        } else {
            // Pipe/FIFO - use background loading for immediate UI
            let stream_reader = Arc::new(Mutex::new(StreamReader::new_incremental()));
            let reader: Arc<Mutex<dyn LogReader + Send>> = stream_reader.clone();
            let stream_writer: Arc<Mutex<dyn StreamableReader>> = stream_reader;

            // Spawn background thread to read from pipe
            let (tx, rx) = mpsc::channel();
            spawn_stream_reader(file, tx);

            Ok(Self {
                source: LogSource {
                    name,
                    source_path: None,
                    mode: ViewMode::Normal,
                    total_lines: 0,
                    line_indices: Vec::new(),
                    follow_mode: true,
                    reader,
                    filter: FilterConfig::default(),
                    source_status: None,
                    disabled: false,
                    file_size: None,
                    index_reader: None,
                    index_size: None,
                },
                scroll_position: 0,
                selected_line: 0,
                watcher: None,
                viewport: Viewport::new(0),
                expansion: ExpansionState::default(),
                stream_writer: Some(stream_writer),
                stream_receiver: Some(rx),
                config_source_type: None,
            })
        }
    }

    /// Create a new tab from stdin (with background loading)
    pub fn from_stdin() -> Result<Self> {
        let stream_reader = Arc::new(Mutex::new(StreamReader::new_incremental()));
        let reader: Arc<Mutex<dyn LogReader + Send>> = stream_reader.clone();
        let stream_writer: Arc<Mutex<dyn StreamableReader>> = stream_reader;

        // Spawn background thread to read from stdin
        let (tx, rx) = mpsc::channel();
        spawn_stream_reader(std::io::stdin(), tx);

        Ok(Self {
            source: LogSource {
                name: "<stdin>".to_string(),
                source_path: None,
                mode: ViewMode::Normal,
                total_lines: 0,
                line_indices: Vec::new(),
                follow_mode: true,
                reader,
                filter: FilterConfig::default(),
                source_status: None,
                disabled: false,
                file_size: None,
                index_reader: None,
                index_size: None,
            },
            scroll_position: 0,
            selected_line: 0,
            watcher: None,
            viewport: Viewport::new(0),
            expansion: ExpansionState::default(),
            stream_writer: Some(stream_writer),
            stream_receiver: Some(rx),
            config_source_type: None,
        })
    }

    /// Create a new tab from a discovered source
    pub fn from_discovered_source(source: DiscoveredSource, watch: bool) -> Result<Self> {
        let file_reader = FileReader::new(&source.log_path)?;
        let index_reader = IndexReader::open(&source.log_path);
        let file_size = std::fs::metadata(&source.log_path).map(|m| m.len()).ok();
        let index_size = index_reader
            .as_ref()
            .and_then(|_| calculate_index_size(&source.log_path));

        let watcher = if watch {
            FileWatcher::new(&source.log_path).ok()
        } else {
            None
        };

        let total_lines = file_reader.total_lines();
        let line_indices = (0..total_lines).collect();
        let selected_line = total_lines.saturating_sub(1);

        Ok(Self {
            source: LogSource {
                name: source.name,
                source_path: Some(source.log_path),
                mode: ViewMode::Normal,
                total_lines,
                line_indices,
                follow_mode: true,
                reader: Arc::new(Mutex::new(file_reader)),
                filter: FilterConfig::default(),
                source_status: Some(source.status),
                disabled: false,
                file_size,
                index_reader,
                index_size,
            },
            scroll_position: 0,
            selected_line,
            watcher,
            viewport: Viewport::new(selected_line),
            expansion: ExpansionState::default(),
            stream_writer: None,
            stream_receiver: None,
            config_source_type: match source.location {
                SourceLocation::Project => Some(SourceType::ProjectSource),
                SourceLocation::Global => None,
            },
        })
    }

    /// Create a new tab from a config source.
    ///
    /// If the source file doesn't exist, creates a disabled placeholder tab.
    /// Otherwise creates a normal file tab with the config source type set.
    pub fn from_config_source(
        source: &config::Source,
        source_type: SourceType,
        watch: bool,
    ) -> Result<Self> {
        // If source doesn't exist, create disabled placeholder tab
        if !source.exists {
            return Self::disabled_source(source.name.clone(), source.path.clone(), source_type);
        }

        // Create normal file tab
        let file_reader = FileReader::new(&source.path)?;
        let index_reader = IndexReader::open(&source.path);
        let file_size = std::fs::metadata(&source.path).map(|m| m.len()).ok();
        let index_size = index_reader
            .as_ref()
            .and_then(|_| calculate_index_size(&source.path));

        let watcher = if watch {
            FileWatcher::new(&source.path).ok()
        } else {
            None
        };

        let total_lines = file_reader.total_lines();
        let line_indices = (0..total_lines).collect();
        let selected_line = total_lines.saturating_sub(1);

        Ok(Self {
            source: LogSource {
                name: source.name.clone(),
                source_path: Some(source.path.clone()),
                mode: ViewMode::Normal,
                total_lines,
                line_indices,
                follow_mode: true,
                reader: Arc::new(Mutex::new(file_reader)),
                filter: FilterConfig::default(),
                source_status: None,
                disabled: false,
                file_size,
                index_reader,
                index_size,
            },
            scroll_position: 0,
            selected_line,
            watcher,
            viewport: Viewport::new(selected_line),
            expansion: ExpansionState::default(),
            stream_writer: None,
            stream_receiver: None,
            config_source_type: Some(source_type),
        })
    }

    /// Create a disabled tab for a missing source (shown grayed out in UI).
    fn disabled_source(name: String, path: PathBuf, source_type: SourceType) -> Result<Self> {
        // Use an empty stream reader as a placeholder (no stream_writer needed)
        let stream_reader = StreamReader::new_incremental();
        let reader: Arc<Mutex<dyn LogReader + Send>> = Arc::new(Mutex::new(stream_reader));

        Ok(Self {
            source: LogSource {
                name,
                source_path: Some(path),
                mode: ViewMode::Normal,
                total_lines: 0,
                line_indices: Vec::new(),
                follow_mode: false,
                reader,
                filter: FilterConfig::default(),
                source_status: None,
                disabled: true,
                file_size: None,
                index_reader: None,
                index_size: None,
            },
            scroll_position: 0,
            selected_line: 0,
            watcher: None,
            viewport: Viewport::new(0),
            expansion: ExpansionState::default(),
            stream_writer: None,
            stream_receiver: None,
            config_source_type: Some(source_type),
        })
    }

    /// Refresh source status for discovered sources.
    ///
    /// Checks if the source process is still running and updates the status.
    /// Only affects tabs created from discovered sources (source_status is Some).
    /// Derives the sources directory from the log file path (sibling of data dir).
    pub fn refresh_source_status(&mut self) {
        if self.source.source_status.is_some() {
            // Derive the sources dir from the log file's data dir:
            // source_path is like .lazytail/data/foo.log → sources dir is .lazytail/sources/
            let status = self
                .source
                .source_path
                .as_ref()
                .and_then(|p| p.parent()) // .lazytail/data/
                .and_then(|data_dir| data_dir.parent()) // .lazytail/
                .map(|base| base.join("sources"))
                .filter(|sources_dir| sources_dir.exists())
                .map(|sources_dir| check_source_status_in_dir(&self.source.name, &sources_dir))
                .unwrap_or_else(|| check_source_status(&self.source.name));
            self.source.source_status = Some(status);
        }
    }

    /// Append lines from background stream loading
    pub fn append_stream_lines(&mut self, lines: Vec<String>) {
        let old_total = self.source.total_lines;
        let new_lines_count = lines.len();

        // Add lines via the StreamableReader handle
        if let Some(ref writer) = self.stream_writer {
            let mut writer = writer.lock().unwrap();
            writer.append_lines(lines);
        }

        // Update total lines
        self.source.total_lines = old_total + new_lines_count;

        // In normal mode, add new line indices
        if self.source.mode == ViewMode::Normal {
            self.source
                .line_indices
                .extend(old_total..old_total + new_lines_count);
        }

        // If in follow mode, jump to end
        if self.source.follow_mode && new_lines_count > 0 {
            self.jump_to_end();
        }
    }

    /// Mark stream loading as complete
    pub fn mark_stream_complete(&mut self) {
        if let Some(ref writer) = self.stream_writer {
            let mut writer = writer.lock().unwrap();
            writer.mark_complete();
        }
        // Clear the receiver since we won't need it anymore
        self.stream_receiver = None;
    }

    /// Get the number of visible lines
    pub fn visible_line_count(&self) -> usize {
        self.source.line_indices.len()
    }

    /// Sync old fields from viewport (for backward compatibility during migration)
    fn sync_from_viewport(&mut self) {
        // Find the index of viewport's anchor_line in line_indices
        let anchor_line = self.viewport.selected_line();
        if let Ok(idx) = self.source.line_indices.binary_search(&anchor_line) {
            self.selected_line = idx;
        } else {
            // If not found exactly, find nearest
            self.selected_line = self
                .source
                .line_indices
                .iter()
                .position(|&l| l >= anchor_line)
                .unwrap_or(self.source.line_indices.len().saturating_sub(1));
        }
    }

    /// Scroll down by one line
    pub fn scroll_down(&mut self) {
        self.viewport.move_selection(1, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll up by one line
    pub fn scroll_up(&mut self) {
        self.viewport.move_selection(-1, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll down by page
    pub fn page_down(&mut self, page_size: usize) {
        // Clamp to i32::MAX to prevent overflow (page_size > 2^31 is unrealistic anyway)
        let delta = page_size.min(i32::MAX as usize) as i32;
        self.viewport
            .move_selection(delta, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Scroll up by page
    pub fn page_up(&mut self, page_size: usize) {
        // Clamp to i32::MAX to prevent overflow (page_size > 2^31 is unrealistic anyway)
        let delta = page_size.min(i32::MAX as usize) as i32;
        self.viewport
            .move_selection(-delta, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Mouse scroll down - moves viewport and selection together
    pub fn mouse_scroll_down(&mut self, lines: usize) {
        self.viewport
            .scroll_with_selection(lines as i32, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Mouse scroll up - moves viewport and selection together
    pub fn mouse_scroll_up(&mut self, lines: usize) {
        self.viewport
            .scroll_with_selection(-(lines as i32), &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Viewport scroll down (Ctrl+E) - scroll viewport without moving selection
    pub fn viewport_down(&mut self) {
        self.viewport.move_viewport(1, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Viewport scroll up (Ctrl+Y) - scroll viewport without moving selection
    pub fn viewport_up(&mut self) {
        self.viewport.move_viewport(-1, &self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Apply filter results (for full filtering)
    pub fn apply_filter(&mut self, matching_indices: Vec<usize>, pattern: String) {
        // Capture screen offset BEFORE changing line_indices
        let screen_offset = self.viewport.get_screen_offset(&self.source.line_indices);

        if self.source.filter.needs_clear {
            // Orchestrator set needs_clear but no partials arrived — Complete has all matches
            self.source.line_indices = matching_indices;
            self.source.filter.needs_clear = false;
        } else if matches!(self.source.filter.state, FilterState::Processing { .. }) {
            // Partials were received (they consumed needs_clear) — extend with final batch
            self.source.line_indices.extend(matching_indices);
        } else {
            // Direct call (no orchestrator, no partials) — replace
            self.source.line_indices = matching_indices;
        }
        self.source.mode = ViewMode::Filtered;
        self.source.filter.pattern = Some(pattern);
        self.source.filter.state = FilterState::Complete {
            matches: self.source.line_indices.len(),
        };
        self.source.filter.last_filtered_line = self.source.total_lines;

        // If we have an origin line (from when filtering started), select nearest match
        // while preserving screen position
        if let Some(origin) = self.source.filter.origin_line {
            if !self.source.line_indices.is_empty() {
                // Find the match nearest to origin
                let nearest_line = self.find_nearest_line(origin);
                // Jump to it while keeping same screen offset
                self.viewport.jump_to_line_at_offset(
                    nearest_line,
                    screen_offset,
                    &self.source.line_indices,
                );
            }
        }
        // Otherwise viewport's anchor_line will find nearest automatically via resolve()

        // Sync old fields from viewport
        self.sync_from_viewport();
    }

    /// Find the line in line_indices nearest to target
    fn find_nearest_line(&self, target: usize) -> usize {
        if self.source.line_indices.is_empty() {
            return target;
        }

        // Binary search to find insertion point
        match self.source.line_indices.binary_search(&target) {
            Ok(_) => target, // Exact match
            Err(pos) => {
                // Find closest between pos-1 and pos
                if pos == 0 {
                    self.source.line_indices[0]
                } else if pos >= self.source.line_indices.len() {
                    self.source.line_indices[self.source.line_indices.len() - 1]
                } else {
                    let before = self.source.line_indices[pos - 1];
                    let after = self.source.line_indices[pos];
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
        self.source.line_indices.extend(new_matching_indices);
        self.source.filter.state = FilterState::Complete {
            matches: self.source.line_indices.len(),
        };
        self.source.filter.last_filtered_line = self.source.total_lines;
        // Don't change selection - let follow mode or user control it
    }

    /// Clear filter and return to normal view
    pub fn clear_filter(&mut self) {
        self.source.line_indices = (0..self.source.total_lines).collect();
        self.source.mode = ViewMode::Normal;
        self.source.filter.pattern = None;
        self.source.filter.state = FilterState::Inactive;

        // Restore to origin line if set (where user was before filtering)
        if let Some(origin) = self.source.filter.origin_line.take() {
            self.viewport.jump_to_line(origin);
        } else {
            // Preserve screen offset - keep selection at same position on screen
            self.viewport
                .preserve_screen_offset(&self.source.line_indices);
        }
        self.sync_from_viewport();
    }

    /// Jump to a specific line number (1-indexed)
    pub fn jump_to_line(&mut self, line_number: usize) {
        if line_number == 0 || self.source.line_indices.is_empty() {
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
        self.source.follow_mode = !self.source.follow_mode;
        if self.source.follow_mode {
            self.jump_to_end();
        }
    }

    /// Jump to the end of the log
    pub fn jump_to_end(&mut self) {
        self.viewport.jump_to_end(&self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Jump to the beginning of the log
    pub fn jump_to_start(&mut self) {
        self.viewport.jump_to_start(&self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Center the current selection on screen (zz)
    pub fn center_view(&mut self) {
        self.viewport.center(&self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Move current selection to top of viewport (zt)
    pub fn view_to_top(&mut self) {
        self.viewport.anchor_to_top(&self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Move current selection to bottom of viewport (zb)
    pub fn view_to_bottom(&mut self) {
        self.viewport.anchor_to_bottom(&self.source.line_indices);
        self.sync_from_viewport();
    }

    /// Toggle expansion state of the currently selected line
    pub fn toggle_expansion(&mut self) {
        if self.source.line_indices.is_empty() {
            return;
        }

        // Get the actual file line number (not the index into line_indices)
        let file_line_number = self.source.line_indices[self.selected_line];

        if self.expansion.expanded_lines.contains(&file_line_number) {
            // Collapse this line
            self.expansion.expanded_lines.remove(&file_line_number);
        } else {
            // Expand this line
            if self.expansion.mode == ExpandMode::Single {
                // In single mode, collapse all other lines first
                self.expansion.expanded_lines.clear();
            }
            self.expansion.expanded_lines.insert(file_line_number);
        }
    }

    /// Collapse all expanded lines
    pub fn collapse_all(&mut self) {
        self.expansion.expanded_lines.clear();
    }

    /// Handle a file modification event (works for both active and inactive tabs).
    ///
    /// Updates total_lines, line_indices, and triggers incremental filtering if needed.
    pub fn apply_file_modification(&mut self, new_total: usize) {
        self.source.total_lines = new_total;

        if self.source.mode == ViewMode::Normal {
            self.source.line_indices = (0..new_total).collect();
        }

        // If tab has an active filter, trigger incremental filtering
        if let Some(pattern) = self.source.filter.pattern.clone() {
            if new_total > self.source.filter.last_filtered_line {
                let mode = self.source.filter.mode;
                let range = Some((self.source.filter.last_filtered_line, new_total));
                crate::filter::orchestrator::FilterOrchestrator::trigger(
                    &mut self.source,
                    pattern,
                    mode,
                    range,
                );
            }
        }
    }

    /// Apply a filter event directly to this tab (works for both active and inactive tabs).
    ///
    /// Returns `true` if the filter operation completed (receiver should be cleared).
    pub fn apply_filter_event(&mut self, event: &super::event::AppEvent) -> bool {
        use super::event::AppEvent;

        match event {
            AppEvent::FilterProgress(lines_processed) => {
                self.source.filter.state = FilterState::Processing {
                    lines_processed: *lines_processed,
                };
                false
            }
            AppEvent::FilterComplete {
                indices,
                incremental,
            } => {
                if *incremental {
                    self.append_filter_results(indices.clone());
                } else {
                    let pattern = self.source.filter.pattern.clone().unwrap_or_default();
                    self.apply_filter(indices.clone(), pattern);
                }
                if self.source.follow_mode {
                    self.jump_to_end();
                }
                true
            }
            AppEvent::FilterError(err) => {
                eprintln!("Filter error: {}", err);
                self.source.filter.state = FilterState::Inactive;
                true
            }
            _ => false,
        }
    }
}

/// Spawn a background thread to read from a stream and send batches of lines
fn spawn_stream_reader<R: std::io::Read + Send + 'static>(reader: R, tx: Sender<StreamMessage>) {
    thread::spawn(move || {
        let buf_reader = BufReader::new(reader);
        let mut batch = Vec::with_capacity(STREAM_BATCH_SIZE);

        for line in buf_reader.lines() {
            match line {
                Ok(line) => {
                    batch.push(line);
                    if batch.len() >= STREAM_BATCH_SIZE {
                        if tx
                            .send(StreamMessage::Lines(std::mem::take(&mut batch)))
                            .is_err()
                        {
                            // Receiver dropped, stop reading
                            return;
                        }
                        batch = Vec::with_capacity(STREAM_BATCH_SIZE);
                    }
                }
                Err(e) => {
                    // Send any remaining lines before error
                    if !batch.is_empty() {
                        let _ = tx.send(StreamMessage::Lines(std::mem::take(&mut batch)));
                    }
                    let _ = tx.send(StreamMessage::Error(e.to_string()));
                    return;
                }
            }
        }

        // Send any remaining lines
        if !batch.is_empty() {
            let _ = tx.send(StreamMessage::Lines(batch));
        }
        let _ = tx.send(StreamMessage::Complete);
    });
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

        assert_eq!(tab.source.total_lines, 3);
        assert_eq!(tab.selected_line, 2); // Starts at end in follow mode
        assert_eq!(tab.source.mode, ViewMode::Normal);
        assert!(tab.source.follow_mode); // Follow mode enabled by default
    }

    #[test]
    fn test_tab_name_extraction() {
        let temp_file = create_temp_log_file(&["line1"]);
        let tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Name should be extracted from the filename
        assert!(!tab.source.name.is_empty());
    }

    #[test]
    fn test_navigation() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3", "line4", "line5"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Starts at end (line 4) in follow mode
        assert_eq!(tab.selected_line, 4);

        // Jump to start first
        tab.jump_to_start();
        assert_eq!(tab.selected_line, 0);

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
    }

    #[test]
    fn test_filter_application() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        tab.apply_filter(vec![0, 2], "error".to_string());

        assert_eq!(tab.source.mode, ViewMode::Filtered);
        assert_eq!(tab.source.line_indices, vec![0, 2]);
        assert_eq!(tab.source.filter.pattern, Some("error".to_string()));
    }

    #[test]
    fn test_clear_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        tab.apply_filter(vec![0, 2], "error".to_string());
        tab.clear_filter();

        assert_eq!(tab.source.mode, ViewMode::Normal);
        assert_eq!(tab.source.line_indices.len(), 4);
        assert!(tab.source.filter.pattern.is_none());
    }

    #[test]
    fn test_follow_mode() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Follow mode is now enabled by default
        assert!(tab.source.follow_mode);
        assert_eq!(tab.selected_line, 2); // Starts at end

        // Toggle off
        tab.toggle_follow_mode();
        assert!(!tab.source.follow_mode);

        // Toggle back on - should jump to end
        tab.toggle_follow_mode();
        assert!(tab.source.follow_mode);
        assert_eq!(tab.selected_line, 2);
    }

    #[test]
    fn test_page_navigation() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start first (starts at end in follow mode)
        tab.jump_to_start();
        assert_eq!(tab.selected_line, 0);

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

        // Jump to start first (starts at end in follow mode)
        tab.jump_to_start();

        // Position selection at line 5
        tab.page_down(5);
        assert_eq!(tab.selected_line, 5);

        // Mouse scroll down - moves both viewport and selection together
        tab.mouse_scroll_down(3);
        assert_eq!(tab.selected_line, 8); // 5 + 3

        // Mouse scroll up
        tab.mouse_scroll_up(2);
        assert_eq!(tab.selected_line, 6); // 8 - 2
    }

    #[test]
    fn test_toggle_expansion() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start (starts at end in follow mode)
        tab.jump_to_start();

        // Initially no lines are expanded
        assert!(!tab.is_line_expanded(0));
        assert!(tab.expansion.expanded_lines.is_empty());

        // Expand line 0
        tab.toggle_expansion();
        assert!(tab.is_line_expanded(0));
        assert_eq!(tab.expansion.expanded_lines.len(), 1);

        // Toggle again - should collapse
        tab.toggle_expansion();
        assert!(!tab.is_line_expanded(0));
        assert!(tab.expansion.expanded_lines.is_empty());
    }

    #[test]
    fn test_multiple_expanded_lines() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3", "line4", "line5"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start (starts at end in follow mode)
        tab.jump_to_start();

        // Default is Multiple mode
        assert_eq!(tab.expansion.mode, ExpandMode::Multiple);

        // Expand line 0
        tab.toggle_expansion();
        assert!(tab.is_line_expanded(0));

        // Move to line 2 and expand
        tab.scroll_down();
        tab.scroll_down();
        tab.toggle_expansion();
        assert!(tab.is_line_expanded(2));

        // Both should be expanded
        assert!(tab.is_line_expanded(0));
        assert!(tab.is_line_expanded(2));
        assert_eq!(tab.expansion.expanded_lines.len(), 2);
    }

    #[test]
    fn test_single_expand_mode() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start (starts at end in follow mode)
        tab.jump_to_start();

        // Switch to single mode
        tab.expansion.mode = ExpandMode::Single;

        // Expand line 0
        tab.toggle_expansion();
        assert!(tab.is_line_expanded(0));

        // Move to line 1 and expand
        tab.scroll_down();
        tab.toggle_expansion();

        // Only line 1 should be expanded now (single mode collapses others)
        assert!(!tab.is_line_expanded(0));
        assert!(tab.is_line_expanded(1));
        assert_eq!(tab.expansion.expanded_lines.len(), 1);
    }

    #[test]
    fn test_collapse_all() {
        let temp_file = create_temp_log_file(&["line1", "line2", "line3"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start (starts at end in follow mode)
        tab.jump_to_start();

        // Expand multiple lines
        tab.toggle_expansion(); // Line 0
        tab.scroll_down();
        tab.toggle_expansion(); // Line 1
        tab.scroll_down();
        tab.toggle_expansion(); // Line 2

        assert_eq!(tab.expansion.expanded_lines.len(), 3);

        // Collapse all
        tab.collapse_all();
        assert!(tab.expansion.expanded_lines.is_empty());
        assert!(!tab.is_line_expanded(0));
        assert!(!tab.is_line_expanded(1));
        assert!(!tab.is_line_expanded(2));
    }

    #[test]
    fn test_expansion_survives_filter() {
        let temp_file = create_temp_log_file(&["error", "info", "error", "debug"]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Jump to start (starts at end in follow mode)
        tab.jump_to_start();

        // Expand line 0 (file line 0)
        tab.toggle_expansion();
        assert!(tab.is_line_expanded(0));

        // Apply filter - should keep expanded_lines (stores file line numbers)
        tab.apply_filter(vec![0, 2], "error".to_string());

        // Line 0 should still be marked as expanded
        assert!(tab.is_line_expanded(0));

        // Clear filter
        tab.clear_filter();

        // Still expanded
        assert!(tab.is_line_expanded(0));
    }

    #[test]
    fn test_expansion_with_empty_file() {
        let temp_file = create_temp_log_file(&[]);
        let mut tab = TabState::new(temp_file.path().to_path_buf(), false).unwrap();

        // Toggle expansion on empty file should not panic
        tab.toggle_expansion();
        assert!(tab.expansion.expanded_lines.is_empty());
    }
}
