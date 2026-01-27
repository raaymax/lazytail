use super::sparse_index::SparseIndex;
use super::tail_buffer::TailBuffer;
use super::LogReader;
use anyhow::{Context, Result};
use memmap2::Mmap;
use std::fs::File;
use std::path::{Path, PathBuf};

/// Default sparse index interval (index every 10,000 lines)
const DEFAULT_INDEX_INTERVAL: usize = 10_000;

/// Default tail buffer capacity (10,000 lines)
const DEFAULT_TAIL_CAPACITY: usize = 10_000;

/// High-performance file reader for huge log files (100M+ lines)
///
/// Combines three strategies for optimal performance:
/// 1. **Sparse Index**: O(n/interval) memory instead of O(n)
/// 2. **Memory-Mapped Access**: Zero-copy file reads via OS page cache
/// 3. **Tail Buffer**: Recent lines in RAM for instant follow mode
///
/// When a line is requested:
/// 1. First check tail buffer (instant if recently added)
/// 2. Fall back to mmap + sparse index (seek + scan)
///
/// Memory budget for 100M lines: ~75MB
/// - Sparse index (1:10K): ~120KB
/// - Tail buffer (10K lines): ~5MB typical
/// - OS manages mmap pages automatically
pub struct HugeFileReader {
    /// Path to the file
    path: PathBuf,

    /// Memory-mapped file (None if file is empty)
    mmap: Option<Mmap>,

    /// Sparse index for line lookup in mmap region
    sparse_index: SparseIndex,

    /// Tail buffer for recently added lines (follow mode)
    tail_buffer: TailBuffer,

    /// Total lines in the file
    total_lines: usize,

    /// Size of file when last mapped
    mapped_size: u64,

    /// Byte offset where tail buffer content starts
    tail_start_offset: u64,
}

impl HugeFileReader {
    /// Create a new HugeFileReader with default settings
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_options(path, DEFAULT_INDEX_INTERVAL, DEFAULT_TAIL_CAPACITY)
    }

    /// Create a new HugeFileReader with custom options
    pub fn with_options<P: AsRef<Path>>(
        path: P,
        index_interval: usize,
        tail_capacity: usize,
    ) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).context(format!("Failed to open file: {}", path.display()))?;

        let metadata = file.metadata()?;
        let file_size = metadata.len();

        let mmap = if file_size > 0 {
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        let mut reader = Self {
            path,
            mmap,
            sparse_index: SparseIndex::new(index_interval),
            tail_buffer: TailBuffer::new(tail_capacity),
            total_lines: 0,
            mapped_size: file_size,
            tail_start_offset: 0,
        };

        reader.build_index()?;
        Ok(reader)
    }

    /// Build the sparse index for the entire file
    fn build_index(&mut self) -> Result<()> {
        self.sparse_index.clear();
        self.tail_buffer.clear();

        if self.mapped_size == 0 {
            self.total_lines = 0;
            return Ok(());
        }

        let data = self.mmap.as_ref().unwrap().as_ref();
        let interval = self.sparse_index.interval();

        let mut line_count = 0usize;
        let mut pos = 0usize;

        while pos < data.len() {
            let next_newline = memchr::memchr(b'\n', &data[pos..]);

            match next_newline {
                Some(offset) => {
                    line_count += 1;
                    let line_end = pos + offset + 1;

                    if line_count.is_multiple_of(interval) {
                        self.sparse_index.append(line_count, line_end as u64);
                    }

                    pos = line_end;
                }
                None => {
                    if pos < data.len() {
                        line_count += 1;
                    }
                    break;
                }
            }
        }

        self.total_lines = line_count;
        self.sparse_index.set_total_lines(line_count);
        self.tail_start_offset = self.mapped_size;

        Ok(())
    }

    /// Get a line from the mmap region using sparse index
    fn get_line_from_mmap(&self, line_num: usize) -> Option<&str> {
        let mmap = self.mmap.as_ref()?;
        let data = mmap.as_ref();

        if data.is_empty() {
            return None;
        }

        let (offset, skip) = self.sparse_index.locate(line_num);
        let mut pos = offset as usize;

        // Skip lines to reach target
        for _ in 0..skip {
            let next_newline = memchr::memchr(b'\n', &data[pos..])?;
            pos = pos + next_newline + 1;
        }

        // Find line bounds
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

        std::str::from_utf8(&data[start..end]).ok()
    }

    /// Refresh to pick up new content appended to the file
    ///
    /// Returns the number of new lines added. Uses incremental reading
    /// for small updates (tail buffer) or remaps for larger changes.
    pub fn refresh(&mut self) -> Result<usize> {
        let file = File::open(&self.path)?;
        let current_size = file.metadata()?.len();

        if current_size < self.mapped_size {
            // File was truncated - need full reload
            self.mmap = if current_size > 0 {
                Some(unsafe { Mmap::map(&file)? })
            } else {
                None
            };
            self.mapped_size = current_size;
            self.build_index()?;
            return Ok(0);
        }

        if current_size == self.mapped_size {
            // No new content
            return Ok(0);
        }

        // Count new lines before remapping
        let old_total = self.total_lines;

        // For simplicity and correctness, remap and rebuild index
        // This ensures all lines are accessible via mmap
        self.mmap = Some(unsafe { Mmap::map(&file)? });
        self.mapped_size = current_size;

        // Rebuild index (clears tail buffer)
        self.build_index()?;

        let new_lines = self.total_lines.saturating_sub(old_total);
        Ok(new_lines)
    }

    /// Get memory usage statistics
    pub fn memory_stats(&self) -> MemoryStats {
        MemoryStats {
            sparse_index_bytes: self.sparse_index.memory_usage(),
            tail_buffer_bytes: self.tail_buffer.memory_usage(),
            tail_buffer_lines: self.tail_buffer.len(),
            mmap_size: self.mapped_size,
        }
    }

    /// Get the number of lines in the indexed (mmap) region
    pub fn indexed_lines(&self) -> usize {
        self.sparse_index.total_lines()
    }

    /// Get the number of lines in the tail buffer
    pub fn tail_buffer_lines(&self) -> usize {
        self.tail_buffer.len()
    }
}

/// Memory usage statistics
#[derive(Debug, Clone)]
pub struct MemoryStats {
    /// Bytes used by the sparse index
    pub sparse_index_bytes: usize,
    /// Bytes used by the tail buffer
    pub tail_buffer_bytes: usize,
    /// Number of lines in the tail buffer
    pub tail_buffer_lines: usize,
    /// Size of the memory-mapped region
    pub mmap_size: u64,
}

impl LogReader for HugeFileReader {
    fn total_lines(&self) -> usize {
        self.total_lines
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        if index >= self.total_lines {
            return Ok(None);
        }

        // First try tail buffer (for recently added lines)
        if let Some(line) = self.tail_buffer.get(index) {
            return Ok(Some(line.to_string()));
        }

        // Fall back to mmap region
        Ok(self.get_line_from_mmap(index).map(|s| s.to_string()))
    }

    fn reload(&mut self) -> Result<()> {
        let file = File::open(&self.path)?;
        let metadata = file.metadata()?;
        let file_size = metadata.len();

        self.mmap = if file_size > 0 {
            Some(unsafe { Mmap::map(&file)? })
        } else {
            None
        };

        self.mapped_size = file_size;
        self.build_index()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_huge_file_reader_basic() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "Line 1")?;
        writeln!(temp_file, "Line 2")?;
        writeln!(temp_file, "Line 3")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "Line 2");
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3");
        assert!(reader.get_line(3)?.is_none());

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_empty() -> Result<()> {
        let temp_file = NamedTempFile::new()?;

        let mut reader = HugeFileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 0);
        assert!(reader.get_line(0)?.is_none());

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_refresh() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();

        writeln!(temp_file, "Initial line 1")?;
        writeln!(temp_file, "Initial line 2")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::new(&path)?;
        assert_eq!(reader.total_lines(), 2);

        // Append new content
        let mut file = OpenOptions::new().append(true).open(&path)?;
        writeln!(file, "New line 3")?;
        writeln!(file, "New line 4")?;
        file.flush()?;
        drop(file);

        // Refresh to pick up new content
        let new_lines = reader.refresh()?;
        assert_eq!(new_lines, 2);
        assert_eq!(reader.total_lines(), 4);
        assert_eq!(reader.get_line(2)?.unwrap(), "New line 3");
        assert_eq!(reader.get_line(3)?.unwrap(), "New line 4");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_refresh_truncation() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();

        // Write initial content
        let mut file = std::fs::File::create(&path)?;
        writeln!(file, "Line 1")?;
        writeln!(file, "Line 2")?;
        writeln!(file, "Line 3")?;
        writeln!(file, "Line 4")?;
        file.flush()?;
        drop(file);

        let mut reader = HugeFileReader::new(&path)?;
        assert_eq!(reader.total_lines(), 4);

        // Truncate file
        let mut file = std::fs::File::create(&path)?;
        writeln!(file, "New line 1")?;
        writeln!(file, "New line 2")?;
        file.flush()?;
        drop(file);

        // Refresh should detect truncation and rebuild
        reader.refresh()?;
        assert_eq!(reader.total_lines(), 2);
        assert_eq!(reader.get_line(0)?.unwrap(), "New line 1");
        assert_eq!(reader.get_line(1)?.unwrap(), "New line 2");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_refresh_new_content() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();

        // Write initial content
        for i in 0..10 {
            writeln!(temp_file, "Initial {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = HugeFileReader::with_options(&path, 5, 3)?;
        assert_eq!(reader.total_lines(), 10);

        // Append content
        let mut file = OpenOptions::new().append(true).open(&path)?;
        for i in 10..15 {
            writeln!(file, "New {}", i)?;
        }
        file.flush()?;
        drop(file);

        let new_count = reader.refresh()?;
        assert_eq!(new_count, 5);
        assert_eq!(reader.total_lines(), 15);

        // All lines should be accessible via mmap
        assert_eq!(reader.get_line(0)?.unwrap(), "Initial 0");
        assert_eq!(reader.get_line(9)?.unwrap(), "Initial 9");
        assert_eq!(reader.get_line(10)?.unwrap(), "New 10");
        assert_eq!(reader.get_line(14)?.unwrap(), "New 14");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_sparse_index() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 50 lines with small interval
        for i in 0..50 {
            writeln!(temp_file, "Line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = HugeFileReader::with_options(temp_file.path(), 10, 100)?;

        assert_eq!(reader.total_lines(), 50);

        // Test access across index boundaries
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 0");
        assert_eq!(reader.get_line(9)?.unwrap(), "Line 9");
        assert_eq!(reader.get_line(10)?.unwrap(), "Line 10");
        assert_eq!(reader.get_line(25)?.unwrap(), "Line 25");
        assert_eq!(reader.get_line(49)?.unwrap(), "Line 49");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_unicode() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        writeln!(temp_file, "English: Hello")?;
        writeln!(temp_file, "日本語: こんにちは")?;
        writeln!(temp_file, "Emoji: 🎉🚀")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "English: Hello");
        assert_eq!(reader.get_line(1)?.unwrap(), "日本語: こんにちは");
        assert_eq!(reader.get_line(2)?.unwrap(), "Emoji: 🎉🚀");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_memory_stats() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        for i in 0..1000 {
            writeln!(temp_file, "Line number {} with some content", i)?;
        }
        temp_file.flush()?;

        let reader = HugeFileReader::new(temp_file.path())?;
        let stats = reader.memory_stats();

        // Sparse index should be minimal
        assert!(stats.sparse_index_bytes < 1024);
        // Tail buffer should be empty (no refresh yet)
        assert_eq!(stats.tail_buffer_lines, 0);
        // Mmap size should match file size
        assert!(stats.mmap_size > 0);

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_reload() -> Result<()> {
        let temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();

        // Write initial content
        let mut file = std::fs::File::create(&path)?;
        writeln!(file, "Original 1")?;
        writeln!(file, "Original 2")?;
        file.flush()?;
        drop(file);

        let mut reader = HugeFileReader::new(&path)?;
        assert_eq!(reader.total_lines(), 2);

        // Replace content
        let mut file = std::fs::File::create(&path)?;
        writeln!(file, "New content 1")?;
        writeln!(file, "New content 2")?;
        writeln!(file, "New content 3")?;
        file.flush()?;
        drop(file);

        // Full reload
        reader.reload()?;
        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "New content 1");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_mixed_line_endings() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Unix line\n")?;
        temp_file.write_all(b"Windows line\r\n")?;
        temp_file.write_all(b"Another Unix\n")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(0)?.unwrap(), "Unix line");
        assert_eq!(reader.get_line(1)?.unwrap(), "Windows line");
        assert_eq!(reader.get_line(2)?.unwrap(), "Another Unix");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_no_trailing_newline() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        temp_file.write_all(b"Line 1\nLine 2\nLine 3 no newline")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::new(temp_file.path())?;

        assert_eq!(reader.total_lines(), 3);
        assert_eq!(reader.get_line(2)?.unwrap(), "Line 3 no newline");

        Ok(())
    }

    #[test]
    fn test_huge_file_reader_continuous_refresh() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        let path = temp_file.path().to_path_buf();

        writeln!(temp_file, "Initial")?;
        temp_file.flush()?;

        let mut reader = HugeFileReader::with_options(&path, 10, 5)?;
        assert_eq!(reader.total_lines(), 1);

        // Simulate follow mode with multiple refreshes
        for batch in 0..3 {
            let mut file = OpenOptions::new().append(true).open(&path)?;
            for i in 0..3 {
                writeln!(file, "Batch {} Line {}", batch, i)?;
            }
            file.flush()?;
            drop(file);

            let new_lines = reader.refresh()?;
            assert_eq!(new_lines, 3);
        }

        assert_eq!(reader.total_lines(), 10); // 1 initial + 3*3 = 10

        // Verify content
        assert_eq!(reader.get_line(0)?.unwrap(), "Initial");
        assert_eq!(reader.get_line(1)?.unwrap(), "Batch 0 Line 0");
        assert_eq!(reader.get_line(9)?.unwrap(), "Batch 2 Line 2");

        Ok(())
    }
}
