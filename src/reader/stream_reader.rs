use super::LogReader;
use anyhow::Result;

/// In-memory reader for non-seekable streams (pipes, process substitution, etc.)
/// Reads the entire input into memory for random access
pub struct StreamReader {
    /// All lines stored in memory
    lines: Vec<String>,
}

impl StreamReader {
    /// Create a new StreamReader from any readable source
    pub fn from_reader<R: std::io::Read>(mut reader: R) -> Result<Self> {
        let mut content = String::new();
        reader.read_to_string(&mut content)?;

        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();

        Ok(Self { lines })
    }
}

impl LogReader for StreamReader {
    fn total_lines(&self) -> usize {
        self.lines.len()
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        Ok(self.lines.get(index).cloned())
    }

    fn reload(&mut self) -> Result<()> {
        // Streams can't be reloaded - they're consumed on first read
        // Just return Ok to avoid errors
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn test_stream_reader_basic() {
        let data = "Line 1\nLine 2\nLine 3\n";
        let cursor = Cursor::new(data);
        let mut reader = StreamReader::from_reader(cursor).unwrap();

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0).unwrap(), Some("Line 1".to_string()));
        assert_eq!(reader.get_line(1).unwrap(), Some("Line 2".to_string()));
        assert_eq!(reader.get_line(2).unwrap(), Some("Line 3".to_string()));
        assert_eq!(reader.get_line(3).unwrap(), None);
    }

    #[test]
    fn test_stream_reader_no_trailing_newline() {
        let data = "Line 1\nLine 2\nLine 3";
        let cursor = Cursor::new(data);
        let mut reader = StreamReader::from_reader(cursor).unwrap();

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(2).unwrap(), Some("Line 3".to_string()));
    }

    #[test]
    fn test_stream_reader_empty() {
        let data = "";
        let cursor = Cursor::new(data);
        let mut reader = StreamReader::from_reader(cursor).unwrap();

        assert_eq!(reader.total_lines(), 0);
        assert_eq!(reader.get_line(0).unwrap(), None);
    }

    #[test]
    fn test_stream_reader_single_line() {
        let data = "Single line";
        let cursor = Cursor::new(data);
        let mut reader = StreamReader::from_reader(cursor).unwrap();

        assert_eq!(reader.total_lines(), 1);
        assert_eq!(reader.get_line(0).unwrap(), Some("Single line".to_string()));
    }

    #[test]
    fn test_stream_reader_reload_is_noop() {
        let data = "Line 1\nLine 2";
        let cursor = Cursor::new(data);
        let mut reader = StreamReader::from_reader(cursor).unwrap();

        // Reload should not fail
        assert!(reader.reload().is_ok());
        // Content should still be there
        assert_eq!(reader.total_lines(), 2);
    }
}
