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

        Some(Self { flags, checkpoints })
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
    }

    /// Get the severity level for a specific line.
    pub fn severity(&self, line_number: usize) -> Severity {
        self.flags
            .get(line_number)
            .copied()
            .map(Severity::from_flags)
            .unwrap_or(Severity::Unknown)
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
        let idx_dir = index_dir_for_log(log_path);
        let meta = IndexMeta::read_from(idx_dir.join("meta")).ok()?;

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

        let severity_counts = if meta.has_column(ColumnBit::Checkpoints) {
            CheckpointReader::open(idx_dir.join("checkpoints"))
                .ok()
                .and_then(|cr| cr.last())
                .map(|cp| cp.severity_counts)
        } else {
            None
        };

        Some(IndexStats {
            indexed_lines: meta.entry_count,
            log_file_size: meta.log_file_size,
            columns,
            severity_counts,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::flags::*;

    fn reader_from(flags_data: &[u32]) -> IndexReader {
        IndexReader {
            flags: flags_data.to_vec(),
            checkpoints: Vec::new(),
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
}
