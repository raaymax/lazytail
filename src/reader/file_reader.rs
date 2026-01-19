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
        let file = File::open(&path).context(format!("Failed to open file: {}", path.display()))?;

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

        // Calculate total lines
        // If the last line didn't end with a newline, we still have that line
        if let Some(&last_index) = self.line_index.last() {
            if last_index < current_offset {
                // We read content after the last newline - that's a line too
                self.total_lines = self.line_index.len();
            } else {
                // Normal case: index has N+1 entries for N lines
                self.total_lines = self.line_index.len().saturating_sub(1);
            }
        } else {
            self.total_lines = 0;
        }

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
    fn test_file_reader_basic() -> Result<()> {
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

    #[test]
    fn test_empty_file() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 0);
        assert!(reader.get_line(0)?.is_none());

        Ok(())
    }

    #[test]
    fn test_file_with_empty_lines() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "First line")?;
        writeln!(temp_file)?; // Empty line
        writeln!(temp_file, "Third line")?;
        writeln!(temp_file)?; // Empty line
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 4);
        assert_eq!(reader.get_line(0)?.unwrap(), "First line");
        assert_eq!(reader.get_line(1)?.unwrap(), "");
        assert_eq!(reader.get_line(2)?.unwrap(), "Third line");
        assert_eq!(reader.get_line(3)?.unwrap(), "");

        Ok(())
    }

    #[test]
    fn test_unicode_content() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "English: Hello World")?;
        writeln!(temp_file, "日本語: こんにちは世界")?;
        writeln!(temp_file, "Русский: Привет мир")?;
        writeln!(temp_file, "العربية: مرحبا بالعالم")?;
        writeln!(temp_file, "Emoji: 🎉🚀✨🔥")?;
        writeln!(temp_file, "Mixed: Hello 世界 🌍")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 6);
        assert_eq!(reader.get_line(0)?.unwrap(), "English: Hello World");
        assert_eq!(reader.get_line(1)?.unwrap(), "日本語: こんにちは世界");
        assert_eq!(reader.get_line(2)?.unwrap(), "Русский: Привет мир");
        assert_eq!(reader.get_line(3)?.unwrap(), "العربية: مرحبا بالعالم");
        assert_eq!(reader.get_line(4)?.unwrap(), "Emoji: 🎉🚀✨🔥");
        assert_eq!(reader.get_line(5)?.unwrap(), "Mixed: Hello 世界 🌍");

        Ok(())
    }

    #[test]
    fn test_ansi_color_codes() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "\x1b[31mRed text\x1b[0m")?;
        writeln!(temp_file, "\x1b[1;32mBold green\x1b[0m")?;
        writeln!(temp_file, "\x1b[44mBlue background\x1b[0m")?;
        writeln!(temp_file, "Normal text")?;
        writeln!(
            temp_file,
            "\x1b[33;1mYellow\x1b[0m mixed \x1b[36mCyan\x1b[0m"
        )?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 5);
        // ANSI codes should be preserved as-is
        assert_eq!(reader.get_line(0)?.unwrap(), "\x1b[31mRed text\x1b[0m");
        assert_eq!(reader.get_line(1)?.unwrap(), "\x1b[1;32mBold green\x1b[0m");
        assert_eq!(
            reader.get_line(2)?.unwrap(),
            "\x1b[44mBlue background\x1b[0m"
        );
        assert_eq!(reader.get_line(3)?.unwrap(), "Normal text");
        assert_eq!(
            reader.get_line(4)?.unwrap(),
            "\x1b[33;1mYellow\x1b[0m mixed \x1b[36mCyan\x1b[0m"
        );

        Ok(())
    }

    #[test]
    fn test_very_long_line() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let long_line = "x".repeat(10000); // 10,000 character line
        writeln!(temp_file, "{}", long_line)?;
        writeln!(temp_file, "Short line")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 2);
        // Should handle long lines without truncation
        assert_eq!(reader.get_line(0)?.unwrap().len(), 10000);
        assert_eq!(reader.get_line(1)?.unwrap(), "Short line");

        Ok(())
    }

    #[test]
    fn test_mixed_line_endings() -> Result<()> {
        use std::io::Write;

        let mut temp_file = NamedTempFile::new()?;
        // Write with different line endings
        temp_file.write_all(b"Unix line\n")?;
        temp_file.write_all(b"Windows line\r\n")?;
        temp_file.write_all(b"Another Unix\n")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Unix line");
        assert_eq!(reader.get_line(1)?.unwrap(), "Windows line");
        assert_eq!(reader.get_line(2)?.unwrap(), "Another Unix");

        Ok(())
    }

    #[test]
    fn test_no_trailing_newline() -> Result<()> {
        use std::io::Write;

        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Line 1\n")?;
        temp_file.write_all(b"Line 2\n")?;
        temp_file.write_all(b"Line 3 no newline")?; // No trailing newline
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "Line 2");
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3 no newline");

        Ok(())
    }

    #[test]
    fn test_special_characters() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Tab:\there")?;
        writeln!(temp_file, "Null: contains\0null")?;
        writeln!(temp_file, "Backslash: \\")?;
        writeln!(temp_file, "Quote: \"test\"")?;
        writeln!(temp_file, "Apostrophe: 'test'")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 5);
        assert_eq!(reader.get_line(0)?.unwrap(), "Tab:\there");
        assert!(reader.get_line(1)?.unwrap().contains('\0'));
        assert_eq!(reader.get_line(2)?.unwrap(), "Backslash: \\");
        assert_eq!(reader.get_line(3)?.unwrap(), "Quote: \"test\"");
        assert_eq!(reader.get_line(4)?.unwrap(), "Apostrophe: 'test'");

        Ok(())
    }

    #[test]
    fn test_reload_with_new_content() -> Result<()> {
        use std::fs::OpenOptions;

        let mut temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();
        writeln!(temp_file, "Initial line 1")?;
        writeln!(temp_file, "Initial line 2")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(&path)?;
        assert_eq!(reader.total_lines(), 2);

        // Append more lines
        let mut file = OpenOptions::new().append(true).open(&path)?;
        writeln!(file, "New line 3")?;
        writeln!(file, "New line 4")?;
        file.flush()?;
        drop(file);

        // Reload
        reader.reload()?;
        assert_eq!(reader.total_lines(), 4);
        assert_eq!(reader.get_line(2)?.unwrap(), "New line 3");
        assert_eq!(reader.get_line(3)?.unwrap(), "New line 4");

        Ok(())
    }

    #[test]
    fn test_reload_after_truncation() -> Result<()> {
        use std::fs::File;

        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();
        drop(temp_file);

        // Write initial content
        let mut file = File::create(&path)?;
        writeln!(file, "Line 1")?;
        writeln!(file, "Line 2")?;
        writeln!(file, "Line 3")?;
        writeln!(file, "Line 4")?;
        file.flush()?;
        drop(file);

        let mut reader = FileReader::new(&path)?;
        assert_eq!(reader.total_lines(), 4);

        // Truncate file to fewer lines
        let mut file = File::create(&path)?;
        writeln!(file, "New line 1")?;
        writeln!(file, "New line 2")?;
        file.flush()?;
        drop(file);

        // Reload
        reader.reload()?;
        assert_eq!(reader.total_lines(), 2);
        assert_eq!(reader.get_line(0)?.unwrap(), "New line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "New line 2");
        assert!(reader.get_line(2)?.is_none());

        Ok(())
    }

    #[test]
    fn test_large_file_indexing() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 1000 lines
        for i in 0..1000 {
            writeln!(temp_file, "Line number {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 1000);
        // Test random access
        assert_eq!(reader.get_line(0)?.unwrap(), "Line number 0");
        assert_eq!(reader.get_line(499)?.unwrap(), "Line number 499");
        assert_eq!(reader.get_line(999)?.unwrap(), "Line number 999");
        assert!(reader.get_line(1000)?.is_none());

        Ok(())
    }

    #[test]
    fn test_out_of_bounds_access() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Only line")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 1);
        assert!(reader.get_line(0)?.is_some());
        assert!(reader.get_line(1)?.is_none());
        assert!(reader.get_line(100)?.is_none());
        assert!(reader.get_line(usize::MAX)?.is_none());

        Ok(())
    }

    #[test]
    fn test_whitespace_only_lines() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Normal line")?;
        writeln!(temp_file, "   ")?; // Spaces only
        writeln!(temp_file, "\t\t")?; // Tabs only
        writeln!(temp_file, " \t ")?; // Mixed whitespace
        writeln!(temp_file)?; // Empty
        writeln!(temp_file, "End")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 6);
        assert_eq!(reader.get_line(0)?.unwrap(), "Normal line");
        assert_eq!(reader.get_line(1)?.unwrap(), "   ");
        assert_eq!(reader.get_line(2)?.unwrap(), "\t\t");
        assert_eq!(reader.get_line(3)?.unwrap(), " \t ");
        assert_eq!(reader.get_line(4)?.unwrap(), "");
        assert_eq!(reader.get_line(5)?.unwrap(), "End");

        Ok(())
    }

    #[test]
    fn test_unicode_line_boundaries() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Test that multi-byte UTF-8 characters at line boundaries work correctly
        writeln!(temp_file, "End with emoji 🎉")?;
        writeln!(temp_file, "🚀 Start with emoji")?;
        writeln!(temp_file, "中文字符在行尾")?;
        writeln!(temp_file, "行首有中文字符")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 4);
        assert_eq!(reader.get_line(0)?.unwrap(), "End with emoji 🎉");
        assert_eq!(reader.get_line(1)?.unwrap(), "🚀 Start with emoji");
        assert_eq!(reader.get_line(2)?.unwrap(), "中文字符在行尾");
        assert_eq!(reader.get_line(3)?.unwrap(), "行首有中文字符");

        Ok(())
    }

    #[test]
    fn test_complex_ansi_sequences() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Complex ANSI sequences from real-world logs
        writeln!(temp_file, "\x1b[38;5;214m[INFO]\x1b[0m Processing request")?;
        writeln!(
            temp_file,
            "\x1b]8;;https://example.com\x1b\\Link\x1b]8;;\x1b\\"
        )?; // Hyperlink
        writeln!(temp_file, "\x1b[1m\x1b[4m\x1b[31mBold Underline Red\x1b[0m")?; // Multiple styles
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        // Should preserve all ANSI sequences
        assert!(reader.get_line(0)?.unwrap().contains("\x1b[38;5;214m"));
        assert!(reader.get_line(1)?.unwrap().contains("\x1b]8;;"));
        assert!(reader.get_line(2)?.unwrap().contains("\x1b[1m"));

        Ok(())
    }
}
