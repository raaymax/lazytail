pub mod file_reader;
#[allow(dead_code)]
pub mod huge_file_reader;
#[allow(dead_code)]
pub mod mmap_reader;
#[allow(dead_code)]
pub mod sparse_index;
pub mod stream_reader;
#[allow(dead_code)]
pub mod tail_buffer;

use anyhow::Result;

/// Trait for reading log lines
pub trait LogReader {
    /// Get total number of lines
    fn total_lines(&self) -> usize;

    /// Get a specific line by index
    fn get_line(&mut self, index: usize) -> Result<Option<String>>;

    /// Reload the source (e.g., for file watching)
    fn reload(&mut self) -> Result<()>;

    /// Append lines for incremental loading (only supported by StreamReader)
    fn append_lines(&mut self, _lines: Vec<String>) {
        // Default: no-op for readers that don't support incremental loading
    }

    /// Mark stream as complete (only supported by StreamReader)
    fn mark_complete(&mut self) {
        // Default: no-op for readers that don't support incremental loading
    }

    /// Check if this is a streaming reader that's still loading
    #[allow(dead_code)]
    fn is_loading(&self) -> bool {
        false // Default: not a streaming reader
    }
}
