use crate::index::checkpoint::{Checkpoint, CheckpointReader, SeverityCounts};
use crate::index::column::ColumnReader;
use crate::index::flags::Severity;
use crate::index::meta::{ColumnBit, IndexMeta};
use crate::source::index_dir_for_log;
use std::path::Path;

/// Aggregated index statistics for a log file.
pub struct IndexStats {
    pub indexed_lines: u64,
    pub log_file_size: u64,
    pub columns: Vec<String>,
    pub severity_counts: Option<SeverityCounts>,
    /// Approximate ingestion rate in lines per second (from checkpoint timestamps).
    pub lines_per_second: Option<f64>,
}

/// Read-only access to an index's flags and checkpoint columns.
///
/// Data is copied into owned memory at open time so the reader is immune
/// to the underlying column files being truncated by a concurrent writer
/// (e.g. capture mode re-creating the index). Without this, mmap-backed
/// readers would SIGBUS when the file shrinks underneath them.
pub struct IndexReader {
    flags: Vec<u32>,
    checkpoints: Vec<Checkpoint>,
    timestamps: Vec<u64>,
}

impl IndexReader {
    /// Open an index for the given log file path. Returns None if no index exists.
    ///
    /// Copies flags and checkpoint data into owned memory, then drops the mmaps.
    pub fn open(log_path: &Path) -> Option<Self> {
        let idx_dir = index_dir_for_log(log_path);
        let meta = IndexMeta::read_from(idx_dir.join("meta")).ok()?;

        let col_reader =
            ColumnReader::<u32>::open(idx_dir.join("flags"), meta.entry_count as usize).ok()?;
        let flags: Vec<u32> = col_reader.iter().collect();
        drop(col_reader);

        let checkpoints = if meta.has_column(ColumnBit::Checkpoints) {
            CheckpointReader::open(idx_dir.join("checkpoints"))
                .ok()
                .map(|r| {
                    let v: Vec<Checkpoint> = r.iter().collect();
                    v
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        let timestamps = if meta.has_column(ColumnBit::Time) {
            ColumnReader::<u64>::open(idx_dir.join("time"), meta.entry_count as usize)
                .ok()
                .map(|col| {
                    let v: Vec<u64> = col.iter().collect();
                    v
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };

        Some(Self {
            flags,
            checkpoints,
            timestamps,
        })
    }

    /// Refresh flags and checkpoints from disk if the index has grown.
    /// Called periodically to pick up new data written by capture's `sync()`.
    pub fn refresh(&mut self, log_path: &Path) {
        let idx_dir = index_dir_for_log(log_path);
        let meta = match IndexMeta::read_from(idx_dir.join("meta")).ok() {
            Some(m) => m,
            None => return,
        };

        let new_count = meta.entry_count as usize;
        if new_count <= self.flags.len() {
            return; // No new data
        }

        if let Ok(col_reader) = ColumnReader::<u32>::open(idx_dir.join("flags"), new_count) {
            self.flags = col_reader.iter().collect();
        }

        if meta.has_column(ColumnBit::Checkpoints) {
            if let Ok(r) = CheckpointReader::open(idx_dir.join("checkpoints")) {
                self.checkpoints = r.iter().collect();
            }
        }

        if meta.has_column(ColumnBit::Time) {
            if let Ok(col) = ColumnReader::<u64>::open(idx_dir.join("time"), new_count) {
                self.timestamps = col.iter().collect();
            }
        }
    }

    /// Get the severity level for a specific line.
    pub fn severity(&self, line_number: usize) -> Severity {
        self.flags
            .get(line_number)
            .copied()
            .map(Severity::from_flags)
            .unwrap_or(Severity::Unknown)
    }

    /// Get the arrival timestamp (ms since epoch) for a specific line.
    pub fn get_timestamp(&self, line_number: usize) -> Option<u64> {
        self.timestamps.get(line_number).copied()
    }

    /// Get the raw flags u32 for a specific line.
    pub fn flags(&self, line_number: usize) -> Option<u32> {
        self.flags.get(line_number).copied()
    }

    /// Number of indexed lines.
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Access the checkpoint data (for severity histogram).
    pub fn checkpoints(&self) -> &[Checkpoint] {
        &self.checkpoints
    }

    /// Compute live severity counts from the flags column.
    /// Unlike checkpoint-based counts, this reflects every indexed line immediately.
    pub fn severity_counts(&self) -> SeverityCounts {
        use crate::index::flags::SEVERITY_MASK;
        let mut counts = SeverityCounts::default();
        for &f in &self.flags {
            match f & SEVERITY_MASK {
                1 => counts.trace += 1,
                2 => counts.debug += 1,
                3 => counts.info += 1,
                4 => counts.warn += 1,
                5 => counts.error += 1,
                6 => counts.fatal += 1,
                _ => counts.unknown += 1,
            }
        }
        counts
    }

    /// Collect line indices where `(flags & mask) == want`.
    ///
    /// This is the core pre-filtering primitive: scan the dense flags column
    /// and return only the line numbers that match the given bitmask pattern.
    /// For example, to find all JSON ERROR lines:
    ///
    /// ```ignore
    /// let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
    /// let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
    /// let candidates = reader.scan_flags(mask, want, total_lines);
    /// ```
    pub fn scan_flags(&self, mask: u32, want: u32, limit: usize) -> Vec<usize> {
        let count = self.flags.len().min(limit);
        let mut result = Vec::new();
        for i in 0..count {
            if self.flags[i] & mask == want {
                result.push(i);
            }
        }
        result
    }

    /// Build a boolean bitmap where `true` means the line is a candidate.
    ///
    /// More efficient than `scan_flags` when the caller needs to iterate
    /// lines sequentially and check membership (avoids binary search).
    pub fn candidate_bitmap(&self, mask: u32, want: u32, limit: usize) -> Vec<bool> {
        self.flags[..self.flags.len().min(limit)]
            .iter()
            .map(|&f| f & mask == want)
            .collect()
    }

    /// Gather aggregated index statistics from the index directory.
    ///
    /// Reads meta + checkpoint data to produce a summary. Returns `None`
    /// if the index directory doesn't exist or meta cannot be read.
    pub fn stats(log_path: &Path) -> Option<IndexStats> {
        use crate::index::flags::SEVERITY_MASK;

        let idx_dir = index_dir_for_log(log_path);
        let meta = IndexMeta::read_from(idx_dir.join("meta")).ok()?;

        // Validate index against actual file — reject stale indexes
        let file_size = std::fs::metadata(log_path).ok()?.len();
        if file_size < meta.log_file_size {
            return None;
        }
        if meta.entry_count >= 2 && meta.has_column(ColumnBit::Offsets) {
            let offsets =
                ColumnReader::<u64>::open(idx_dir.join("offsets"), meta.entry_count as usize)
                    .ok()?;
            if offsets.get(0) != Some(0) {
                return None;
            }
            if let Some(next_offset) = offsets.get(1) {
                if next_offset > 0 {
                    let mut file = std::fs::File::open(log_path).ok()?;
                    use std::io::{Read, Seek, SeekFrom};
                    file.seek(SeekFrom::Start(next_offset - 1)).ok()?;
                    let mut buf = [0u8; 1];
                    file.read_exact(&mut buf).ok()?;
                    if buf[0] != b'\n' {
                        return None;
                    }
                }
            }
        }

        let column_names = [
            (ColumnBit::Offsets, "offsets"),
            (ColumnBit::Lengths, "lengths"),
            (ColumnBit::Time, "time"),
            (ColumnBit::Flags, "flags"),
            (ColumnBit::Checkpoints, "checkpoints"),
        ];
        let columns: Vec<String> = column_names
            .iter()
            .filter(|(bit, _)| meta.has_column(*bit))
            .map(|(_, name)| name.to_string())
            .collect();

        // Compute severity counts from the flags column (live, not checkpoint-delayed)
        let severity_counts = if meta.has_column(ColumnBit::Flags) {
            ColumnReader::<u32>::open(idx_dir.join("flags"), meta.entry_count as usize)
                .ok()
                .map(|col| {
                    let mut counts = SeverityCounts::default();
                    for f in col.iter() {
                        match f & SEVERITY_MASK {
                            1 => counts.trace += 1,
                            2 => counts.debug += 1,
                            3 => counts.info += 1,
                            4 => counts.warn += 1,
                            5 => counts.error += 1,
                            6 => counts.fatal += 1,
                            _ => counts.unknown += 1,
                        }
                    }
                    counts
                })
        } else {
            None
        };

        // Compute approximate rate from first and last checkpoint timestamps
        let lines_per_second = if meta.has_column(ColumnBit::Checkpoints) {
            Self::rate_from_checkpoints(&idx_dir)
        } else {
            None
        };

        Some(IndexStats {
            indexed_lines: meta.entry_count,
            log_file_size: meta.log_file_size,
            columns,
            severity_counts,
            lines_per_second,
        })
    }

    /// Compute a recent ingestion rate from the last two checkpoint timestamps.
    fn rate_from_checkpoints(idx_dir: &Path) -> Option<f64> {
        let reader = CheckpointReader::open(idx_dir.join("checkpoints")).ok()?;
        let checkpoints: Vec<Checkpoint> = reader.iter().collect();
        let len = checkpoints.len();
        if len < 2 {
            return None;
        }
        let prev = &checkpoints[len - 2];
        let last = &checkpoints[len - 1];
        let dt_ms = last.index_timestamp.saturating_sub(prev.index_timestamp);
        if dt_ms == 0 {
            return None;
        }
        let dn = last.line_number.saturating_sub(prev.line_number) as f64;
        let dt_secs = dt_ms as f64 / 1000.0;
        Some(dn / dt_secs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::column::ColumnWriter;
    use crate::index::flags::*;

    fn reader_from(flags_data: &[u32]) -> IndexReader {
        IndexReader {
            flags: flags_data.to_vec(),
            checkpoints: Vec::new(),
            timestamps: Vec::new(),
        }
    }

    // --- flags() ---

    #[test]
    fn test_flags_access() {
        let flags = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let reader = reader_from(&[flags, SEVERITY_INFO, 0]);

        assert_eq!(reader.flags(0), Some(flags));
        assert_eq!(reader.flags(1), Some(SEVERITY_INFO));
        assert_eq!(reader.flags(2), Some(0));
        assert_eq!(reader.flags(3), None);
    }

    // --- len() / is_empty() ---

    #[test]
    fn test_len_and_empty() {
        let reader = reader_from(&[0, 0, 0]);
        assert_eq!(reader.len(), 3);
        assert!(!reader.is_empty());
    }

    #[test]
    fn test_empty_index() {
        let reader = reader_from(&[]);
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    // --- scan_flags() ---

    #[test]
    fn test_scan_flags_json_errors() {
        let flags_data = vec![
            SEVERITY_ERROR | FLAG_FORMAT_JSON,  // line 0: JSON error
            SEVERITY_INFO | FLAG_FORMAT_JSON,   // line 1: JSON info
            SEVERITY_ERROR,                     // line 2: non-JSON error
            SEVERITY_ERROR | FLAG_FORMAT_JSON,  // line 3: JSON error
            FLAG_FORMAT_JSON,                   // line 4: JSON unknown severity
            SEVERITY_WARN | FLAG_FORMAT_LOGFMT, // line 5: logfmt warn
        ];
        let reader = reader_from(&flags_data);

        let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
        let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let candidates = reader.scan_flags(mask, want, flags_data.len());

        assert_eq!(candidates, vec![0, 3]);
    }

    #[test]
    fn test_scan_flags_json_only() {
        let flags_data = vec![
            FLAG_FORMAT_JSON | SEVERITY_INFO,
            SEVERITY_ERROR,
            FLAG_FORMAT_JSON | SEVERITY_ERROR,
            FLAG_FORMAT_LOGFMT | SEVERITY_WARN,
        ];
        let reader = reader_from(&flags_data);

        let mask = FLAG_FORMAT_JSON;
        let want = FLAG_FORMAT_JSON;
        let candidates = reader.scan_flags(mask, want, flags_data.len());

        assert_eq!(candidates, vec![0, 2]);
    }

    #[test]
    fn test_scan_flags_with_limit() {
        let flags_data = vec![
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
        ];
        let reader = reader_from(&flags_data);

        let candidates = reader.scan_flags(FLAG_FORMAT_JSON, FLAG_FORMAT_JSON, 2);
        assert_eq!(candidates, vec![0, 1]);
    }

    #[test]
    fn test_scan_flags_no_matches() {
        let flags_data = vec![SEVERITY_INFO, SEVERITY_WARN, SEVERITY_DEBUG];
        let reader = reader_from(&flags_data);

        let candidates = reader.scan_flags(FLAG_FORMAT_JSON, FLAG_FORMAT_JSON, flags_data.len());
        assert!(candidates.is_empty());
    }

    // --- candidate_bitmap() ---

    #[test]
    fn test_candidate_bitmap() {
        let flags_data = vec![
            SEVERITY_ERROR | FLAG_FORMAT_JSON,
            SEVERITY_INFO | FLAG_FORMAT_JSON,
            SEVERITY_ERROR,
            SEVERITY_ERROR | FLAG_FORMAT_JSON,
        ];
        let reader = reader_from(&flags_data);

        let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
        let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let bitmap = reader.candidate_bitmap(mask, want, flags_data.len());

        assert_eq!(bitmap, vec![true, false, false, true]);
    }

    // --- stats() stale index validation ---

    /// Helper: create a log file and its index directory with offsets + flags columns.
    fn create_indexed_log(dir: &Path, content: &str) -> std::path::PathBuf {
        let log_path = dir.join("test.log");
        std::fs::write(&log_path, content).unwrap();

        let idx_dir = index_dir_for_log(&log_path);
        std::fs::create_dir_all(&idx_dir).unwrap();

        let lines: Vec<&str> = content.split('\n').collect();
        // Don't count trailing empty split
        let line_count = if content.ends_with('\n') {
            lines.len() - 1
        } else {
            lines.len()
        };

        // Write offsets column
        let mut offsets = ColumnWriter::<u64>::create(idx_dir.join("offsets")).unwrap();
        let mut offset = 0u64;
        for i in 0..line_count {
            offsets.push(offset).unwrap();
            offset += lines[i].len() as u64 + 1; // +1 for newline
        }
        drop(offsets);

        // Write flags column
        let mut flags = ColumnWriter::<u32>::create(idx_dir.join("flags")).unwrap();
        for _ in 0..line_count {
            flags.push(SEVERITY_INFO).unwrap();
        }
        drop(flags);

        // Write meta
        let mut meta = IndexMeta::new();
        meta.entry_count = line_count as u64;
        meta.log_file_size = content.len() as u64;
        meta.set_column(ColumnBit::Offsets);
        meta.set_column(ColumnBit::Flags);
        meta.write_to(idx_dir.join("meta")).unwrap();

        log_path
    }

    #[test]
    fn test_stats_valid_index() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = create_indexed_log(dir.path(), content);

        let stats = IndexReader::stats(&log_path);
        assert!(stats.is_some());
        let stats = stats.unwrap();
        assert_eq!(stats.indexed_lines, 3);
    }

    #[test]
    fn test_stats_rejects_stale_index_after_file_shrink() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = create_indexed_log(dir.path(), content);

        // Replace log with shorter content — index is now stale
        std::fs::write(&log_path, "short\n").unwrap();

        assert!(IndexReader::stats(&log_path).is_none());
    }

    #[test]
    fn test_stats_rejects_stale_index_after_content_change() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = create_indexed_log(dir.path(), content);

        // Replace log with same-size content but no newline at the indexed offset.
        // Original has '\n' at byte 8; replacement has 'X' there.
        let replacement = "X".repeat(content.len() - 1) + "\n";
        assert_eq!(replacement.len(), content.len());
        std::fs::write(&log_path, &replacement).unwrap();

        assert!(IndexReader::stats(&log_path).is_none());
    }

    #[test]
    fn test_stats_accepts_grown_file() {
        let dir = tempfile::tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = create_indexed_log(dir.path(), content);

        // Append more data — file grew, but index is still valid for existing lines
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        write!(f, "line four\n").unwrap();
        drop(f);

        let stats = IndexReader::stats(&log_path);
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().indexed_lines, 3);
    }
}
