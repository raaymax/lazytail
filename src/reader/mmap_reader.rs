use super::sparse_index::SparseIndex;
use super::LogReader;
use anyhow::{Context, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Default sparse index interval (index every 10,000 lines)
const DEFAULT_INDEX_INTERVAL: usize = 10_000;

/// Memory-mapped file reader for efficient access to large log files
///
/// Uses mmap for zero-copy file access combined with sparse indexing
/// for memory-efficient line lookup. The OS handles paging, so only
/// accessed regions consume physical memory.
///
/// Benefits over BufReader-based FileReader:
/// - Zero-copy access (returns &str slices into mapped memory)
/// - OS-managed page cache (automatic eviction of cold data)
/// - Better performance for random access patterns
///
/// Tradeoffs:
/// - Requires file to be valid UTF-8
/// - File must be reopened and remapped on modification
pub struct MmapReader {
    /// Path to the file
    path: PathBuf,

    /// Memory-mapped file content
    mmap: Mmap,

    /// Sparse index for line lookup
    sparse_index: SparseIndex,

    /// Size of the file when mapped
    file_size: u64,
}

impl MmapReader {
    /// Create a new MmapReader and build the sparse line index
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_interval(path, DEFAULT_INDEX_INTERVAL)
    }

    /// Create a new MmapReader with a custom index interval
    pub fn with_interval<P: AsRef<Path>>(path: P, interval: usize) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).context(format!("Failed to open file: {}", path.display()))?;

        let metadata = file.metadata()?;
        let file_size = metadata.len();

        // Handle empty files
        if file_size == 0 {
            return Ok(Self {
                path,
                mmap: unsafe { Mmap::map(&file)? },
                sparse_index: SparseIndex::new(interval),
                file_size: 0,
            });
        }

        let mmap = unsafe { Mmap::map(&file)? };

        let mut reader = Self {
            path,
            mmap,
            sparse_index: SparseIndex::new(interval),
            file_size,
        };

        reader.build_index()?;
        Ok(reader)
    }

    /// Build the sparse line index by scanning through the memory-mapped file
    fn build_index(&mut self) -> Result<()> {
        self.sparse_index.clear();

        if self.file_size == 0 {
            return Ok(());
        }

        let data = &self.mmap[..];
        let interval = self.sparse_index.interval();

        let mut line_count = 0usize;
        let mut pos = 0usize;

        while pos < data.len() {
            // Find next newline using memchr for speed
            let next_newline = memchr::memchr(b'\n', &data[pos..]);

            match next_newline {
                Some(offset) => {
                    line_count += 1;
                    let line_end = pos + offset + 1; // Include the newline

                    // Index every `interval` lines
                    if line_count.is_multiple_of(interval) {
                        self.sparse_index.append(line_count, line_end as u64);
                    }

                    pos = line_end;
                }
                None => {
                    // No more newlines - check if there's remaining content (line without newline)
                    if pos < data.len() {
                        line_count += 1;
                    }
                    break;
                }
            }
        }

        self.sparse_index.set_total_lines(line_count);
        Ok(())
    }

    /// Get a line as a string slice (zero-copy when possible)
    fn get_line_slice(&self, line_num: usize) -> Option<&str> {
        if line_num >= self.sparse_index.total_lines() || self.file_size == 0 {
            return None;
        }

        let data = &self.mmap[..];
        let (offset, skip) = self.sparse_index.locate(line_num);

        // Start from the indexed position
        let mut pos = offset as usize;

        // Skip lines to reach target
        for _ in 0..skip {
            let next_newline = memchr::memchr(b'\n', &data[pos..])?;
            pos = pos + next_newline + 1;
        }

        // Find the end of target line
        let start = pos;
        let end = memchr::memchr(b'\n', &data[start..])
            .map(|offset| start + offset)
            .unwrap_or(data.len());

        // Handle Windows line endings
        let end = if end > start && data.get(end.saturating_sub(1)) == Some(&b'\r') {
            end - 1
        } else {
            end
        };

        // Convert to string, handling invalid UTF-8 gracefully
        std::str::from_utf8(&data[start..end]).ok()
    }

    /// Get memory usage of the index in bytes
    #[cfg(test)]
    pub fn index_memory_usage(&self) -> usize {
        self.sparse_index.memory_usage()
    }

    /// Get the file size
    #[cfg(test)]
    pub fn file_size(&self) -> u64 {
        self.file_size
    }
}

impl LogReader for MmapReader {
    fn total_lines(&self) -> usize {
        self.sparse_index.total_lines()
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        // Convert from &str to owned String for API compatibility
        Ok(self.get_line_slice(index).map(|s| s.to_string()))
    }

    fn reload(&mut self) -> Result<()> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        self.file_size = metadata.len();

        if self.file_size == 0 {
            self.sparse_index.clear();
            return Ok(());
        }

        self.mmap = unsafe { Mmap::map(&file)? };
        self.build_index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_mmap_reader_basic() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Line 1")?;
        writeln!(temp_file, "Line 2")?;
        writeln!(temp_file, "Line 3")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "Line 2");
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3");
        assert!(reader.get_line(3)?.is_none());

        Ok(())
    }

    #[test]
    fn test_mmap_empty_file() -> Result<()> {
        let temp_file = NamedTempFile::new()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 0);
        assert!(reader.get_line(0)?.is_none());

        Ok(())
    }

    #[test]
    fn test_mmap_file_with_empty_lines() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "First line")?;
        writeln!(temp_file)?; // Empty line
        writeln!(temp_file, "Third line")?;
        writeln!(temp_file)?; // Empty line
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 4);
        assert_eq!(reader.get_line(0)?.unwrap(), "First line");
        assert_eq!(reader.get_line(1)?.unwrap(), "");
        assert_eq!(reader.get_line(2)?.unwrap(), "Third line");
        assert_eq!(reader.get_line(3)?.unwrap(), "");

        Ok(())
    }

    #[test]
    fn test_mmap_unicode_content() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "English: Hello World")?;
        writeln!(temp_file, "日本語: こんにちは世界")?;
        writeln!(temp_file, "Emoji: 🎉🚀✨")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "English: Hello World");
        assert_eq!(reader.get_line(1)?.unwrap(), "日本語: こんにちは世界");
        assert_eq!(reader.get_line(2)?.unwrap(), "Emoji: 🎉🚀✨");

        Ok(())
    }

    #[test]
    fn test_mmap_ansi_codes() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "\x1b[31mRed text\x1b[0m")?;
        writeln!(temp_file, "\x1b[1;32mBold green\x1b[0m")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 2);
        assert_eq!(reader.get_line(0)?.unwrap(), "\x1b[31mRed text\x1b[0m");
        assert_eq!(reader.get_line(1)?.unwrap(), "\x1b[1;32mBold green\x1b[0m");

        Ok(())
    }

    #[test]
    fn test_mmap_mixed_line_endings() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Unix line\n")?;
        temp_file.write_all(b"Windows line\r\n")?;
        temp_file.write_all(b"Another Unix\n")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Unix line");
        assert_eq!(reader.get_line(1)?.unwrap(), "Windows line");
        assert_eq!(reader.get_line(2)?.unwrap(), "Another Unix");

        Ok(())
    }

    #[test]
    fn test_mmap_no_trailing_newline() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Line 1\n")?;
        temp_file.write_all(b"Line 2\n")?;
        temp_file.write_all(b"Line 3 no newline")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "Line 2");
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3 no newline");

        Ok(())
    }

    #[test]
    fn test_mmap_reload() -> Result<()> {
        use std::fs::OpenOptions;

        let mut temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();
        writeln!(temp_file, "Initial line 1")?;
        writeln!(temp_file, "Initial line 2")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(&path)?;
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
    fn test_mmap_sparse_index_with_small_interval() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        for i in 0..25 {
            writeln!(temp_file, "Line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = MmapReader::with_interval(temp_file.path(), 10)?;

        assert_eq!(reader.total_lines(), 25);

        // Test access across index boundaries
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 0");
        assert_eq!(reader.get_line(9)?.unwrap(), "Line 9");
        assert_eq!(reader.get_line(10)?.unwrap(), "Line 10");
        assert_eq!(reader.get_line(15)?.unwrap(), "Line 15");
        assert_eq!(reader.get_line(24)?.unwrap(), "Line 24");

        Ok(())
    }

    #[test]
    fn test_mmap_random_access() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        for i in 0..50 {
            writeln!(temp_file, "Content at line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = MmapReader::with_interval(temp_file.path(), 10)?;

        // Access lines in random order
        assert_eq!(reader.get_line(45)?.unwrap(), "Content at line 45");
        assert_eq!(reader.get_line(5)?.unwrap(), "Content at line 5");
        assert_eq!(reader.get_line(25)?.unwrap(), "Content at line 25");
        assert_eq!(reader.get_line(10)?.unwrap(), "Content at line 10");
        assert_eq!(reader.get_line(0)?.unwrap(), "Content at line 0");
        assert_eq!(reader.get_line(49)?.unwrap(), "Content at line 49");

        Ok(())
    }

    #[test]
    fn test_mmap_memory_efficiency() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 100,000 lines
        for i in 0..100_000 {
            writeln!(temp_file, "Line number {} with some padding", i)?;
        }
        temp_file.flush()?;

        let reader = MmapReader::new(temp_file.path())?;

        // Index memory should be minimal
        let memory = reader.index_memory_usage();
        assert!(
            memory < 1024,
            "Index memory {} bytes should be under 1KB",
            memory
        );

        Ok(())
    }

    #[test]
    fn test_mmap_very_long_line() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let long_line = "x".repeat(10000);
        writeln!(temp_file, "{}", long_line)?;
        writeln!(temp_file, "Short line")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 2);
        assert_eq!(reader.get_line(0)?.unwrap().len(), 10000);
        assert_eq!(reader.get_line(1)?.unwrap(), "Short line");

        Ok(())
    }

    #[test]
    fn test_mmap_out_of_bounds() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Only line")?;
        temp_file.flush()?;

        let mut reader = MmapReader::new(temp_file.path())?;

        assert!(reader.get_line(0)?.is_some());
        assert!(reader.get_line(1)?.is_none());
        assert!(reader.get_line(100)?.is_none());

        Ok(())
    }
}
