pub mod file_reader;

use anyhow::Result;

/// Trait for reading log lines
pub trait LogReader {
    /// Get total number of lines
    fn total_lines(&self) -> usize;

    /// Get a specific line by index
    fn get_line(&mut self, index: usize) -> Result<Option<String>>;

    /// Get a range of lines
    fn get_lines(&mut self, start: usize, count: usize) -> Result<Vec<String>>;

    /// Reload the source (e.g., for file watching)
    fn reload(&mut self) -> Result<()>;
}
