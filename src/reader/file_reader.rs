use super::LogReader;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Lazy file reader that indexes line positions for efficient random access
pub struct FileReader {
    /// Path to the file
    path: PathBuf,

    /// File handle for reading
    file: File,

    /// Index storing byte offset for each line start
    /// line_index[i] = byte offset where line i starts
    line_index: Vec<u64>,

    /// Total number of lines
    total_lines: usize,
}

impl FileReader {
    /// Create a new FileReader and build the line index
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)
            .context(format!("Failed to open file: {}", path.display()))?;

        let mut reader = Self {
            path,
            file,
            line_index: Vec::new(),
            total_lines: 0,
        };

        reader.build_index()?;
        Ok(reader)
    }

    /// Build the line index by scanning through the file
    fn build_index(&mut self) -> Result<()> {
        self.file.seek(SeekFrom::Start(0))?;

        let mut buf_reader = BufReader::new(&self.file);
        let mut current_offset = 0u64;
        let mut line_buffer = String::new();

        // First line starts at offset 0
        self.line_index.push(0);

        loop {
            line_buffer.clear();
            let bytes_read = buf_reader.read_line(&mut line_buffer)?;

            if bytes_read == 0 {
                // End of file
                break;
            }

            current_offset += bytes_read as u64;

            // If we read a line and haven't reached EOF, there's another line
            if line_buffer.ends_with('\n') || line_buffer.ends_with('\r') {
                self.line_index.push(current_offset);
            }
        }

        // Calculate total lines (index has N+1 entries for N lines)
        self.total_lines = self.line_index.len().saturating_sub(1);

        // Reopen file for future seeks (BufReader consumed the file)
        self.file = File::open(&self.path)?;

        Ok(())
    }

    /// Read a specific line by seeking to its position
    fn read_line_at_offset(&mut self, offset: u64) -> Result<String> {
        self.file.seek(SeekFrom::Start(offset))?;
        let mut reader = BufReader::new(&self.file);
        let mut line = String::new();
        reader.read_line(&mut line)?;

        // Reopen file for future seeks
        self.file = File::open(&self.path)?;

        // Remove trailing newline
        if line.ends_with('\n') {
            line.pop();
            if line.ends_with('\r') {
                line.pop();
            }
        }

        Ok(line)
    }

    /// Get the file path
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl LogReader for FileReader {
    fn total_lines(&self) -> usize {
        self.total_lines
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        if index >= self.total_lines {
            return Ok(None);
        }

        let offset = self.line_index[index];
        let line = self.read_line_at_offset(offset)?;
        Ok(Some(line))
    }

    fn get_lines(&mut self, start: usize, count: usize) -> Result<Vec<String>> {
        let mut lines = Vec::new();
        let end = (start + count).min(self.total_lines);

        for i in start..end {
            if let Some(line) = self.get_line(i)? {
                lines.push(line);
            }
        }

        Ok(lines)
    }

    fn reload(&mut self) -> Result<()> {
        self.line_index.clear();
        self.total_lines = 0;
        self.file = File::open(&self.path)?;
        self.build_index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_file_reader() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Line 1")?;
        writeln!(temp_file, "Line 2")?;
        writeln!(temp_file, "Line 3")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "Line 2");
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3");
        assert!(reader.get_line(3)?.is_none());

        Ok(())
    }
}
