use crate::app::{FilterState, ViewMode};
use crate::filter::cancel::CancelToken;
use crate::filter::engine::FilterProgress;
use crate::filter::FilterMode;
use crate::index::reader::IndexReader;
use crate::reader::LogReader;
use crate::source::SourceStatus;
use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

/// Calculate the total size of all files in the index directory
pub(crate) fn calculate_index_size(log_path: &Path) -> Option<u64> {
    let index_dir = crate::source::index_dir_for_log(log_path);
    if !index_dir.exists() || !index_dir.is_dir() {
        return None;
    }

    let mut total_size = 0u64;
    let entries = std::fs::read_dir(&index_dir).ok()?;

    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                total_size += metadata.len();
            }
        }
    }

    Some(total_size)
}

/// Filter-related state for a source
#[derive(Default)]
pub struct FilterConfig {
    /// Current filter state (Inactive, Processing, Complete)
    pub state: FilterState,
    /// Current filter pattern (if any)
    pub pattern: Option<String>,
    /// Filter mode (Plain or Regex, with case sensitivity)
    pub mode: FilterMode,
    /// Channel receiver for filter progress updates
    pub receiver: Option<Receiver<FilterProgress>>,
    /// Cancellation token for the current filter operation
    pub cancel_token: Option<CancelToken>,
    /// Whether current filter operation is incremental
    pub is_incremental: bool,
    /// Last line number that was filtered (for incremental filtering)
    pub last_filtered_line: usize,
    /// Original line when filter started (for restoring on Esc)
    pub origin_line: Option<usize>,
    /// Flag to clear results when first partial results arrive (prevents blink)
    pub needs_clear: bool,
}

/// Domain-only state for a log source, shared across TUI/Web/MCP adapters.
///
/// Contains all the core data needed for log viewing: reader, index,
/// filter state, line indices, and source metadata. Does NOT contain
/// adapter-specific state like viewport, expansion, or watchers.
pub struct LogSourceState {
    /// Display name (filename or source name)
    pub name: String,
    /// Source file path (None for stdin)
    pub source_path: Option<PathBuf>,
    /// Current view mode (Normal or Filtered)
    pub mode: ViewMode,
    /// Total number of lines in the source
    pub total_lines: usize,
    /// Indices of lines to display (all lines or filtered results)
    pub line_indices: Vec<usize>,
    /// Follow mode - auto-scroll to latest logs
    pub follow_mode: bool,
    /// Per-source reader
    pub reader: Arc<Mutex<dyn LogReader + Send>>,
    /// Filter configuration and state
    pub filter: FilterConfig,
    /// Source status for discovered sources (Active/Ended)
    pub source_status: Option<SourceStatus>,
    /// Whether this source is disabled (file doesn't exist)
    pub disabled: bool,
    /// File size in bytes (None for stdin/pipes without a file path)
    pub file_size: Option<u64>,
    /// Columnar index reader for severity coloring and stats (None if no index)
    pub index_reader: Option<IndexReader>,
    /// Index directory size in bytes (None if no index)
    pub index_size: Option<u64>,
}

#[allow(dead_code)]
impl LogSourceState {
    /// Get the number of visible lines
    pub fn visible_line_count(&self) -> usize {
        self.line_indices.len()
    }

    /// Get the file path for this source (None for stdin/pipe).
    pub fn file_path(&self) -> Option<&Path> {
        self.source_path.as_deref()
    }
}
