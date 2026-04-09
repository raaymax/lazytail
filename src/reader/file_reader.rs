use super::sparse_index::SparseIndex;
use super::LogReader;
use crate::index::column::ColumnReader;
use crate::index::meta::IndexMeta;
use crate::index::validate::validate_index;
use crate::source::index_dir_for_log;
use anyhow::{Context, Result};
use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// Default sparse index interval (index every 10,000 lines)
const DEFAULT_INDEX_INTERVAL: usize = 10_000;

/// Lazy file reader with two-tier line access:
///
/// 1. **Columnar offsets (O(1))**: When a columnar index exists, the mmap-backed
///    offsets column provides direct byte offset for every indexed line — zero scanning.
/// 2. **Sparse index (fallback)**: For files without an index, or for tail lines
///    beyond the indexed range (file grew after index was built), falls back to
///    sparse sampling (every Nth line) with forward scanning.
pub struct FileReader {
    /// Path to the file
    path: PathBuf,

    /// Buffered reader for efficient reading
    /// Using BufReader with seek() clears the buffer, making random access safe
    reader: BufReader<File>,

    /// Sparse index storing byte offsets for every Nth line (fallback for unindexed files/tails)
    sparse_index: SparseIndex,

    /// Byte offset up to which the file has been scanned (for incremental reload)
    scanned_up_to: u64,

    /// Mmap-backed offsets from the columnar index — O(1) access for indexed lines
    columnar_offsets: Option<ColumnReader<u64>>,

    /// Number of lines covered by columnar_offsets
    indexed_lines: usize,

    /// Last line read (via any path) — enables O(1) sequential reads.
    /// When the next get_line(N+1) follows get_line(N), the reader is already
    /// positioned at the right byte offset so no seek is needed.
    last_read_line: Option<usize>,
}

impl FileReader {
    /// Create a new FileReader and build the sparse line index.
    ///
    /// If a columnar index exists, seeds the sparse index from its offsets column
    /// (essentially instant). Otherwise falls back to scanning the file.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        Self::with_interval(path, DEFAULT_INDEX_INTERVAL)
    }

    /// Create a new FileReader with a custom index interval
    pub fn with_interval<P: AsRef<Path>>(path: P, interval: usize) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).context(format!("Failed to open file: {}", path.display()))?;

        let mut reader = Self {
            path,
            reader: BufReader::new(file),
            sparse_index: SparseIndex::new(interval),
            scanned_up_to: 0,
            columnar_offsets: None,
            indexed_lines: 0,
            last_read_line: None,
        };

        if !reader.try_seed_from_index() {
            reader.build_index()?;
        }
        Ok(reader)
    }

    /// Try to load the columnar index's offsets column for O(1) line access.
    /// Uses `validate_index()` for structural and checkpoint-based validation
    /// with partial trust support. Returns true if successful.
    fn try_seed_from_index(&mut self) -> bool {
        let idx_dir = index_dir_for_log(&self.path);
        let meta = match IndexMeta::read_from(idx_dir.join("meta")) {
            Ok(m) => m,
            Err(_) => return false,
        };

        let validated = match validate_index(&idx_dir, &self.path, &meta) {
            Some(v) => v,
            None => return false,
        };

        let indexed_lines = validated.trusted_entries;
        if indexed_lines == 0 {
            self.sparse_index.set_total_lines(0);
            return true;
        }

        let offsets = match ColumnReader::<u64>::open(idx_dir.join("offsets"), indexed_lines) {
            Ok(r) if r.len() == indexed_lines => r,
            _ => return false,
        };

        // Keep the mmap-backed offsets column for O(1) line access.
        // For lines beyond the trusted range, sparse index handles the tail.
        self.columnar_offsets = Some(offsets);
        self.indexed_lines = indexed_lines;

        // If the file extends beyond the trusted region, scan only the new tail
        let file_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if file_size > validated.trusted_file_size {
            if self
                .scan_tail(indexed_lines, validated.trusted_file_size)
                .is_err()
            {
                return false;
            }
        } else {
            self.sparse_index.set_total_lines(indexed_lines);
            self.scanned_up_to = validated.trusted_file_size;
        }

        true
    }

    /// Refresh columnar offsets from the index that capture is building in real-time.
    /// Re-mmaps the offsets column if the index has grown since we last loaded it.
    ///
    /// Uses lightweight structural checks (not full checkpoint validation) since
    /// during live capture the indexer is trusted. The sequential read path also
    /// verifies position against columnar offsets to catch orphan gaps.
    fn try_refresh_columnar_offsets(&mut self) {
        let idx_dir = index_dir_for_log(&self.path);
        let meta = match IndexMeta::read_from(idx_dir.join("meta")) {
            Ok(m) => m,
            Err(_) => return,
        };

        let new_indexed = meta.entry_count as usize;
        if new_indexed <= self.indexed_lines {
            return; // No new indexed lines
        }

        // Sanity check: log file must be at least as large as meta claims
        let file_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        if file_size < meta.log_file_size {
            return;
        }

        // Re-mmap the offsets column with the updated size
        let offsets = match ColumnReader::<u64>::open(idx_dir.join("offsets"), new_indexed) {
            Ok(r) if r.len() == new_indexed => r,
            _ => return,
        };

        // Verify offsets match actual file content by cross-checking against
        // the lengths column. For a valid index, the line at offset[i] must have
        // content length == lengths[i]. Sample a few to keep it cheap.
        {
            use std::io::{BufRead, BufReader, Seek, SeekFrom};
            let lengths_col = match ColumnReader::<u32>::open(idx_dir.join("lengths"), new_indexed)
            {
                Ok(r) if r.len() == new_indexed => r,
                _ => return,
            };
            let mut f = match std::fs::File::open(&self.path) {
                Ok(f) => BufReader::new(f),
                Err(_) => return,
            };
            let samples = [0, new_indexed / 2, new_indexed.saturating_sub(1)];
            for i in samples {
                if i >= new_indexed {
                    continue;
                }
                if let (Some(offset), Some(expected_len)) = (offsets.get(i), lengths_col.get(i)) {
                    if offset >= file_size {
                        return;
                    }
                    if f.seek(SeekFrom::Start(offset)).is_err() {
                        return;
                    }
                    let mut buf = Vec::new();
                    if f.read_until(b'\n', &mut buf).is_err() {
                        return;
                    }
                    // Strip trailing \n and \r for content length comparison
                    if buf.last() == Some(&b'\n') {
                        buf.pop();
                    }
                    if buf.last() == Some(&b'\r') {
                        buf.pop();
                    }
                    let content_len = buf.len();
                    if content_len != expected_len as usize {
                        return;
                    }
                }
            }
        }
        if self.indexed_lines > 0 {
            let last_old = self
                .columnar_offsets
                .as_ref()
                .and_then(|c| c.get(self.indexed_lines - 1));
            let first_new = offsets.get(self.indexed_lines);
            if let (Some(old), Some(new)) = (last_old, first_new) {
                if new <= old {
                    return; // Not monotonically increasing
                }
            }
        }

        self.columnar_offsets = Some(offsets);
        self.indexed_lines = new_indexed;

        // Advance scanned_up_to so scan_tail only covers the unindexed remainder
        if meta.log_file_size > self.scanned_up_to {
            self.scanned_up_to = meta.log_file_size;
            self.sparse_index.set_total_lines(new_indexed);
        }
    }

    /// Scan only the tail of the file (from `start_offset`) to count new lines
    /// and extend the sparse index. `base_lines` is the number of lines already indexed.
    fn scan_tail(&mut self, base_lines: usize, start_offset: u64) -> Result<()> {
        self.reader.seek(SeekFrom::Start(start_offset))?;

        let mut buf = [0u8; 64 * 1024];
        let mut line_count = base_lines;
        let mut file_offset = start_offset;
        let mut last_byte_was_newline = true;
        let interval = self.sparse_index.interval();

        loop {
            let bytes_read = self.reader.read(&mut buf)?;
            if bytes_read == 0 {
                break;
            }

            let chunk = &buf[..bytes_read];
            for pos in memchr::memchr_iter(b'\n', chunk) {
                line_count += 1;
                if line_count.is_multiple_of(interval) {
                    self.sparse_index
                        .append(line_count, file_offset + pos as u64 + 1);
                }
            }

            last_byte_was_newline = chunk[bytes_read - 1] == b'\n';
            file_offset += bytes_read as u64;
        }

        if file_offset > start_offset && !last_byte_was_newline {
            line_count += 1;
        }

        self.sparse_index.set_total_lines(line_count);
        self.scanned_up_to = file_offset;
        Ok(())
    }

    /// Build the sparse line index by scanning through the file
    ///
    /// Uses raw byte scanning with memchr for ~10x speedup over read_line(),
    /// since we only need newline byte offsets, not line content or UTF-8 validation.
    fn build_index(&mut self) -> Result<()> {
        self.reader.seek(SeekFrom::Start(0))?;
        self.sparse_index.clear();

        let mut buf = [0u8; 64 * 1024];
        let mut line_count = 0usize;
        let mut file_offset = 0u64;
        let mut last_byte_was_newline = true; // treat start-of-file as "after newline"
        let interval = self.sparse_index.interval();

        loop {
            let bytes_read = self.reader.read(&mut buf)?;
            if bytes_read == 0 {
                break;
            }

            let chunk = &buf[..bytes_read];
            for pos in memchr::memchr_iter(b'\n', chunk) {
                line_count += 1;
                if line_count.is_multiple_of(interval) {
                    self.sparse_index
                        .append(line_count, file_offset + pos as u64 + 1);
                }
            }

            last_byte_was_newline = chunk[bytes_read - 1] == b'\n';
            file_offset += bytes_read as u64;
        }

        // Count final line if file doesn't end with newline
        if file_offset > 0 && !last_byte_was_newline {
            line_count += 1;
        }

        self.sparse_index.set_total_lines(line_count);
        self.scanned_up_to = file_offset;
        Ok(())
    }

    /// Read a specific line. Detects sequential access first (no seek needed),
    /// then falls back to columnar offsets (O(1) seek) or sparse index (seek + scan).
    fn read_line_at(&mut self, line_num: usize) -> Result<Option<String>> {
        if line_num >= self.sparse_index.total_lines() {
            return Ok(None);
        }

        // Sequential access fast path: if we just read line N-1 (common in render loops),
        // the reader is already positioned at line N — just read, no seek needed.
        // For indexed lines, verify position matches columnar offset to catch orphan gaps
        // (where the file has unindexed bytes between consecutive indexed lines).
        if self.last_read_line == Some(line_num.wrapping_sub(1)) {
            let position_ok = if line_num < self.indexed_lines {
                self.columnar_offsets
                    .as_ref()
                    .and_then(|c| c.get(line_num))
                    .is_none_or(|expected| {
                        self.reader
                            .stream_position()
                            .is_ok_and(|pos| pos == expected)
                    })
            } else {
                true
            };

            if position_ok {
                match read_line_lossy(&mut self.reader)? {
                    Some(line) => {
                        self.last_read_line = Some(line_num);
                        return Ok(Some(line));
                    }
                    None => {
                        self.last_read_line = None;
                        return Ok(None);
                    }
                }
            }
        }

        // Columnar path: O(1) direct seek via mmap-backed offsets
        if line_num < self.indexed_lines {
            if let Some(offset) = self.columnar_offsets.as_ref().and_then(|c| c.get(line_num)) {
                self.reader.seek(SeekFrom::Start(offset))?;
                match read_line_lossy(&mut self.reader)? {
                    Some(line) => {
                        self.last_read_line = Some(line_num);
                        return Ok(Some(line));
                    }
                    None => {
                        self.last_read_line = None;
                        return Ok(None);
                    }
                }
            }
        }

        // Columnar tail path: line is beyond indexed range but we can use the last
        // indexed offset as a starting point instead of falling through to sparse index
        // (which may scan from the start of the file if it has no entries for the tail).
        if let Some(ref col) = self.columnar_offsets {
            if self.indexed_lines > 0 {
                if let Some(last_offset) = col.get(self.indexed_lines - 1) {
                    // Seek to the last indexed line and skip forward
                    self.reader.seek(SeekFrom::Start(last_offset))?;
                    let skip = line_num - (self.indexed_lines - 1);
                    for _ in 0..skip {
                        if read_line_lossy(&mut self.reader)?.is_none() {
                            self.last_read_line = None;
                            return Ok(None);
                        }
                    }
                    match read_line_lossy(&mut self.reader)? {
                        Some(line) => {
                            self.last_read_line = Some(line_num);
                            return Ok(Some(line));
                        }
                        None => {
                            self.last_read_line = None;
                            return Ok(None);
                        }
                    }
                }
            }
        }

        // Sparse index path: locate nearest entry + forward scan
        let (offset, skip) = self.sparse_index.locate(line_num);
        self.reader.seek(SeekFrom::Start(offset))?;

        for _ in 0..skip {
            if read_line_lossy(&mut self.reader)?.is_none() {
                self.last_read_line = None;
                return Ok(None);
            }
        }

        match read_line_lossy(&mut self.reader)? {
            Some(line) => {
                self.last_read_line = Some(line_num);
                Ok(Some(line))
            }
            None => {
                self.last_read_line = None;
                Ok(None)
            }
        }
    }

    /// Get memory usage of the index in bytes
    #[cfg(test)]
    pub fn index_memory_usage(&self) -> usize {
        self.sparse_index.memory_usage()
    }

    /// Whether this reader has loaded columnar offsets for O(1) line access.
    #[cfg(test)]
    #[allow(dead_code)] // Used by lib tests (healing_tests), not bin tests
    pub(crate) fn has_columnar_offsets(&self) -> bool {
        self.columnar_offsets.is_some()
    }

    /// Number of lines covered by the columnar index.
    #[cfg(test)]
    #[allow(dead_code)] // Used by lib tests (healing_tests), not bin tests
    pub(crate) fn columnar_line_count(&self) -> usize {
        self.indexed_lines
    }
}

/// Strip trailing newline (and optional carriage return) from a line buffer.
fn trim_newline(buf: &mut String) {
    if buf.ends_with('\n') {
        buf.pop();
        if buf.ends_with('\r') {
            buf.pop();
        }
    }
}

/// Read a line from a buffered reader, tolerating invalid UTF-8 and binary content.
///
/// Uses `read_until(b'\n')` into a byte buffer and converts via
/// `String::from_utf8_lossy`, replacing invalid bytes with U+FFFD.
/// Control characters (0x00-0x1F) other than tab are stripped to prevent
/// them from corrupting TUI layout calculations.
/// Returns `Ok(None)` on EOF (0 bytes read).
fn read_line_lossy(reader: &mut BufReader<File>) -> Result<Option<String>> {
    let mut buf = Vec::new();
    let n = reader.read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Ok(None);
    }
    let mut line = String::from_utf8_lossy(&buf).into_owned();
    trim_newline(&mut line);
    // Strip control characters that break TUI rendering.
    // Keep: \t (expand_tabs), \x1b (ANSI ESC), \x07 (BEL, used as OSC terminator)
    if line.bytes().any(|b| b < 0x20 && b != b'\t' && b != b'\x1b' && b != b'\x07') {
        line = line
            .chars()
            .filter(|&c| c >= ' ' || c == '\t' || c == '\x1b' || c == '\x07')
            .collect();
    }
    Ok(Some(line))
}

impl LogReader for FileReader {
    fn total_lines(&self) -> usize {
        self.sparse_index.total_lines()
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        self.read_line_at(index)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn reload(&mut self) -> Result<()> {
        let new_size = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);

        // Nothing changed — skip the reload entirely
        if new_size == self.scanned_up_to {
            return Ok(());
        }

        let file = File::open(&self.path)?;
        self.reader = BufReader::new(file);
        self.last_read_line = None;

        if new_size >= self.scanned_up_to {
            // File grew — refresh columnar offsets from the index that
            // capture is building in real-time, then scan only the unindexed tail.
            // SAFETY: columnar_offsets mmap is protected from concurrent truncation
            // by IndexWriteLock — only one writer runs at a time, and a truncating
            // writer would change the log file size, triggering the shrink branch below.
            self.try_refresh_columnar_offsets();
            let old_lines = self.sparse_index.total_lines();
            self.scan_tail(old_lines, self.scanned_up_to)
        } else {
            // File was truncated — columnar offsets are now invalid
            self.columnar_offsets = None;
            self.indexed_lines = 0;
            self.build_index()
        }
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
        // Control characters (including null) are stripped for TUI safety
        assert_eq!(reader.get_line(1)?.unwrap(), "Null: containsnull");
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

    // Tests specific to sparse indexing behavior

    #[test]
    fn test_sparse_index_with_small_interval() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 25 lines with interval of 10
        for i in 0..25 {
            writeln!(temp_file, "Line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = FileReader::with_interval(temp_file.path(), 10)?;

        assert_eq!(reader.total_lines(), 25);

        // Test access before first index entry
        assert_eq!(reader.get_line(0)?.unwrap(), "Line 0");
        assert_eq!(reader.get_line(5)?.unwrap(), "Line 5");
        assert_eq!(reader.get_line(9)?.unwrap(), "Line 9");

        // Test access at index entry
        assert_eq!(reader.get_line(10)?.unwrap(), "Line 10");

        // Test access between index entries
        assert_eq!(reader.get_line(15)?.unwrap(), "Line 15");

        // Test access at second index entry
        assert_eq!(reader.get_line(20)?.unwrap(), "Line 20");

        // Test access after last index entry
        assert_eq!(reader.get_line(24)?.unwrap(), "Line 24");

        Ok(())
    }

    #[test]
    fn test_sparse_index_memory_efficiency() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 100,000 lines
        for i in 0..100_000 {
            writeln!(temp_file, "Line number {} with some padding text", i)?;
        }
        temp_file.flush()?;

        // With default interval (10,000), should have ~10 index entries
        let reader = FileReader::new(temp_file.path())?;

        // Memory should be minimal - roughly 10 entries * 12 bytes = 120 bytes
        // Plus struct overhead. Should be under 1KB for sure.
        let memory = reader.index_memory_usage();
        assert!(
            memory < 1024,
            "Index memory {} bytes should be under 1KB",
            memory
        );

        Ok(())
    }

    #[test]
    fn test_random_access_across_index_boundaries() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        // Write 50 lines with interval of 10
        for i in 0..50 {
            writeln!(temp_file, "Content at line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = FileReader::with_interval(temp_file.path(), 10)?;

        // Access lines in random order crossing index boundaries
        assert_eq!(reader.get_line(45)?.unwrap(), "Content at line 45");
        assert_eq!(reader.get_line(5)?.unwrap(), "Content at line 5");
        assert_eq!(reader.get_line(25)?.unwrap(), "Content at line 25");
        assert_eq!(reader.get_line(10)?.unwrap(), "Content at line 10");
        assert_eq!(reader.get_line(35)?.unwrap(), "Content at line 35");
        assert_eq!(reader.get_line(0)?.unwrap(), "Content at line 0");
        assert_eq!(reader.get_line(49)?.unwrap(), "Content at line 49");

        Ok(())
    }

    #[test]
    fn test_interval_of_one_behaves_like_full_index() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;

        for i in 0..20 {
            writeln!(temp_file, "Line {}", i)?;
        }
        temp_file.flush()?;

        let mut reader = FileReader::with_interval(temp_file.path(), 1)?;

        assert_eq!(reader.total_lines(), 20);
        for i in 0..20 {
            assert_eq!(reader.get_line(i)?.unwrap(), format!("Line {}", i));
        }

        Ok(())
    }

    // Tests for columnar offsets (O(1) line access)

    #[test]
    fn test_columnar_offsets_direct_access() -> Result<()> {
        use crate::index::builder::IndexBuilder;
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Write a log file
        {
            let mut f = File::create(&log_path)?;
            for i in 0..100 {
                writeln!(f, "Line number {}", i)?;
            }
            f.flush()?;
        }

        // Build a columnar index for it
        let idx_dir = index_dir_for_log(&log_path);
        IndexBuilder::new().build(&log_path, &idx_dir)?;

        // Open FileReader — should use columnar offsets
        let mut reader = FileReader::new(&log_path)?;

        // Verify it loaded the columnar offsets
        assert!(reader.columnar_offsets.is_some());
        assert_eq!(reader.indexed_lines, 100);

        // Verify random access works correctly via direct path
        assert_eq!(reader.total_lines(), 100);
        assert_eq!(reader.get_line(0)?.unwrap(), "Line number 0");
        assert_eq!(reader.get_line(50)?.unwrap(), "Line number 50");
        assert_eq!(reader.get_line(99)?.unwrap(), "Line number 99");
        assert!(reader.get_line(100)?.is_none());

        Ok(())
    }

    #[test]
    fn test_columnar_offsets_with_file_growth() -> Result<()> {
        use crate::index::builder::IndexBuilder;
        use crate::source::index_dir_for_log;
        use std::fs::OpenOptions;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Write initial content
        {
            let mut f = File::create(&log_path)?;
            for i in 0..50 {
                writeln!(f, "Original line {}", i)?;
            }
            f.flush()?;
        }

        // Build index for the initial content
        let idx_dir = index_dir_for_log(&log_path);
        IndexBuilder::new().build(&log_path, &idx_dir)?;

        // Append more lines (beyond the index)
        {
            let mut f = OpenOptions::new().append(true).open(&log_path)?;
            for i in 50..80 {
                writeln!(f, "New line {}", i)?;
            }
            f.flush()?;
        }

        // Open FileReader — should use columnar offsets for 0..50 and sparse for 50..80
        let mut reader = FileReader::new(&log_path)?;

        assert!(reader.columnar_offsets.is_some());
        assert_eq!(reader.indexed_lines, 50);
        assert_eq!(reader.total_lines(), 80);

        // Indexed lines (O(1) path)
        assert_eq!(reader.get_line(0)?.unwrap(), "Original line 0");
        assert_eq!(reader.get_line(49)?.unwrap(), "Original line 49");

        // Tail lines beyond index (sparse fallback)
        assert_eq!(reader.get_line(50)?.unwrap(), "New line 50");
        assert_eq!(reader.get_line(79)?.unwrap(), "New line 79");
        assert!(reader.get_line(80)?.is_none());

        Ok(())
    }

    #[test]
    fn test_columnar_offsets_invalidated_on_truncation() -> Result<()> {
        use crate::index::builder::IndexBuilder;
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Write and index
        {
            let mut f = File::create(&log_path)?;
            for i in 0..50 {
                writeln!(f, "Line {}", i)?;
            }
            f.flush()?;
        }
        let idx_dir = index_dir_for_log(&log_path);
        IndexBuilder::new().build(&log_path, &idx_dir)?;

        let mut reader = FileReader::new(&log_path)?;
        assert!(reader.columnar_offsets.is_some());
        assert_eq!(reader.indexed_lines, 50);

        // Truncate the file (simulate log rotation)
        {
            let mut f = File::create(&log_path)?;
            writeln!(f, "Fresh line 0")?;
            writeln!(f, "Fresh line 1")?;
            f.flush()?;
        }

        // Reload should invalidate columnar offsets
        reader.reload()?;
        assert!(reader.columnar_offsets.is_none());
        assert_eq!(reader.indexed_lines, 0);
        assert_eq!(reader.total_lines(), 2);
        assert_eq!(reader.get_line(0)?.unwrap(), "Fresh line 0");
        assert_eq!(reader.get_line(1)?.unwrap(), "Fresh line 1");

        Ok(())
    }

    #[test]
    /// Regression test: capture appends to existing file but builds index
    /// from offset 0. The TUI's refresh should reject these broken offsets.
    #[test]
    fn test_broken_index_from_appended_capture_rejected_on_refresh() -> Result<()> {
        use crate::index::builder::LineIndexer;
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Phase 1: Write original content with VARYING line lengths
        // (no index — previous capture left only file)
        {
            let mut f = File::create(&log_path)?;
            for i in 0..50 {
                // Varying lengths so offsets don't accidentally align
                let padding = "x".repeat(i * 3);
                writeln!(f, "OLD {}{}", i, padding)?;
            }
            f.flush()?;
        }

        // Open reader — no index, sparse index only
        let mut reader = FileReader::new(&log_path)?;
        assert!(reader.columnar_offsets.is_none());
        assert_eq!(reader.get_line(0)?.unwrap(), "OLD 0");

        // Phase 2: Append new content with DIFFERENT lengths
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&log_path)?;
            for i in 0..20 {
                writeln!(f, r#"{{"ts":"2024-01-01","level":"info","i":{}}}"#, i)?;
            }
            f.flush()?;
        }

        // Phase 3: Build index with offset 0 (THE BUG — not calling set_current_offset)
        let idx_dir = index_dir_for_log(&log_path);
        let mut indexer = LineIndexer::create(&idx_dir)?;
        for i in 0..20 {
            let raw = format!("{{\"ts\":\"2024-01-01\",\"level\":\"info\",\"i\":{}}}\n", i);
            indexer.push_line(raw.as_bytes(), 1700000000000 + i * 1000)?;
        }
        indexer.finish(&idx_dir)?;

        // Reload reader — the broken index should be rejected by newline check
        reader.reload()?;

        assert!(
            reader.columnar_offsets.is_none(),
            "broken index should be rejected on refresh"
        );
        // Content still accessible via sparse index
        assert_eq!(reader.get_line(0)?.unwrap(), "OLD 0");

        Ok(())
    }

    /// Verify that a correctly built index for appended content is accepted.
    #[test]
    fn test_correct_appended_index_accepted_on_refresh() -> Result<()> {
        use crate::index::builder::LineIndexer;
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Phase 1: Write original content (no index)
        let old_size;
        {
            let mut f = File::create(&log_path)?;
            for i in 0..50 {
                let padding = "x".repeat(i * 3);
                writeln!(f, "OLD {}{}", i, padding)?;
            }
            f.flush()?;
            old_size = std::fs::metadata(&log_path)?.len();
        }

        // Open reader — no index
        let mut reader = FileReader::new(&log_path)?;
        assert!(reader.columnar_offsets.is_none());

        // Phase 2: Append new content
        {
            let mut f = std::fs::OpenOptions::new().append(true).open(&log_path)?;
            for i in 0..20 {
                writeln!(f, r#"{{"ts":"2024-01-01","level":"info","i":{}}}"#, i)?;
            }
            f.flush()?;
        }

        // Phase 3: Build index with CORRECT base offset
        let idx_dir = index_dir_for_log(&log_path);
        let mut indexer = LineIndexer::create(&idx_dir)?;
        indexer.set_current_offset(old_size);
        for i in 0..20 {
            let raw = format!("{{\"ts\":\"2024-01-01\",\"level\":\"info\",\"i\":{}}}\n", i);
            indexer.push_line(raw.as_bytes(), 1700000000000 + i * 1000)?;
        }
        indexer.finish(&idx_dir)?;

        // Reload reader — the correct index should be accepted
        reader.reload()?;

        assert!(
            reader.columnar_offsets.is_some(),
            "correctly offset index should be accepted"
        );
        assert_eq!(reader.indexed_lines, 20);

        Ok(())
    }

    fn test_stale_index_rejected_on_file_replacement() -> Result<()> {
        use crate::index::builder::IndexBuilder;
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir()?;
        let log_path = dir.path().join("test.log");

        // Write original content and build index
        {
            let mut f = File::create(&log_path)?;
            for i in 0..50 {
                writeln!(f, "Original line {}", i)?;
            }
            f.flush()?;
        }
        let idx_dir = index_dir_for_log(&log_path);
        IndexBuilder::new().build(&log_path, &idx_dir)?;

        // Replace the file with DIFFERENT content (same or larger size)
        // This simulates deleting and recreating the log file
        {
            let mut f = File::create(&log_path)?;
            for i in 0..100 {
                writeln!(f, "Completely different content line {}", i)?;
            }
            f.flush()?;
        }

        // Open reader — stale index should be rejected, content should be correct
        let mut reader = FileReader::new(&log_path)?;
        assert!(
            reader.columnar_offsets.is_none(),
            "stale index should be rejected"
        );
        assert_eq!(reader.total_lines(), 100);
        assert_eq!(
            reader.get_line(0)?.unwrap(),
            "Completely different content line 0"
        );
        assert_eq!(
            reader.get_line(99)?.unwrap(),
            "Completely different content line 99"
        );

        Ok(())
    }

    #[test]
    fn test_binary_file_does_not_error() -> Result<()> {
        let mut temp_file = NamedTempFile::new()?;
        // Write binary content with invalid UTF-8 bytes interspersed with newlines
        temp_file.write_all(b"hello\n")?;
        temp_file.write_all(b"\x00\x80\xff\xfe\n")?;
        temp_file.write_all(b"valid line\n")?;
        temp_file.write_all(b"\xc0\xc1\xf5\xf6\xf7\n")?;
        temp_file.flush()?;

        let mut reader = FileReader::new(temp_file.path())?;
        assert_eq!(reader.total_lines(), 4);

        // All lines should be readable without errors
        assert_eq!(reader.get_line(0)?.unwrap(), "hello");
        // Invalid UTF-8 bytes are replaced with U+FFFD
        let line1 = reader.get_line(1)?.unwrap();
        assert!(
            line1.contains('\u{FFFD}'),
            "Expected replacement chars in binary line, got: {:?}",
            line1
        );
        assert_eq!(reader.get_line(2)?.unwrap(), "valid line");
        let line3 = reader.get_line(3)?.unwrap();
        assert!(
            line3.contains('\u{FFFD}'),
            "Expected replacement chars in binary line, got: {:?}",
            line3
        );

        Ok(())
    }
}
