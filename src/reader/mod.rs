pub mod file_reader;
pub mod stream_reader;

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
