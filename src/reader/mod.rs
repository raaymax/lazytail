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
}

/// Extension trait for stream-based readers that support incremental loading.
///
/// Only implemented by `StreamReader` — `FileReader` does not implement this.
/// Tab stores an optional `Box<dyn StreamableReader>` for stream-specific operations.
pub trait StreamableReader: LogReader + Send {
    /// Append lines for incremental loading
    fn append_lines(&mut self, lines: Vec<String>);

    /// Mark the stream as complete (no more data will arrive)
    fn mark_complete(&mut self);

    /// Check if this stream is still loading
    #[allow(dead_code)]
    fn is_loading(&self) -> bool;
}
