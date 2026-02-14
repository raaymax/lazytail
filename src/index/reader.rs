use crate::index::checkpoint::CheckpointReader;
use crate::index::column::ColumnReader;
use crate::index::flags::Severity;
use crate::index::meta::{ColumnBit, IndexMeta};
use crate::source::index_dir_for_log;
use std::path::Path;

/// Read-only access to an index's flags and checkpoint columns.
pub struct IndexReader {
    flags: ColumnReader<u32>,
    checkpoints: Option<CheckpointReader>,
}

impl IndexReader {
    /// Open an index for the given log file path. Returns None if no index exists.
    pub fn open(log_path: &Path) -> Option<Self> {
        let idx_dir = index_dir_for_log(log_path);
        let meta = IndexMeta::read_from(idx_dir.join("meta")).ok()?;

        let flags =
            ColumnReader::<u32>::open(idx_dir.join("flags"), meta.entry_count as usize).ok()?;

        let checkpoints = if meta.has_column(ColumnBit::Checkpoints) {
            CheckpointReader::open(idx_dir.join("checkpoints")).ok()
        } else {
            None
        };

        Some(Self { flags, checkpoints })
    }

    /// Get the severity level for a specific line.
    pub fn severity(&self, line_number: usize) -> Severity {
        self.flags
            .get(line_number)
            .map(Severity::from_flags)
            .unwrap_or(Severity::Unknown)
    }

    /// Get the raw flags u32 for a specific line.
    pub fn flags(&self, line_number: usize) -> Option<u32> {
        self.flags.get(line_number)
    }

    /// Number of indexed lines.
    pub fn len(&self) -> usize {
        self.flags.len()
    }

    /// Whether the index is empty.
    pub fn is_empty(&self) -> bool {
        self.flags.is_empty()
    }

    /// Access the checkpoint reader (for severity histogram).
    pub fn checkpoints(&self) -> Option<&CheckpointReader> {
        self.checkpoints.as_ref()
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
            if let Some(f) = self.flags.get(i) {
                if f & mask == want {
                    result.push(i);
                }
            }
        }
        result
    }

    /// Build a boolean bitmap where `true` means the line is a candidate.
    ///
    /// More efficient than `scan_flags` when the caller needs to iterate
    /// lines sequentially and check membership (avoids binary search).
    pub fn candidate_bitmap(&self, mask: u32, want: u32, limit: usize) -> Vec<bool> {
        let count = self.flags.len().min(limit);
        let mut bitmap = vec![false; count];
        for i in 0..count {
            if let Some(f) = self.flags.get(i) {
                bitmap[i] = f & mask == want;
            }
        }
        bitmap
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::column::ColumnWriter;
    use crate::index::flags::*;
    use crate::index::meta::IndexMeta;
    use tempfile::tempdir;

    fn create_test_index(dir: &std::path::Path, flags_data: &[u32]) {
        std::fs::create_dir_all(dir).unwrap();

        let mut meta = IndexMeta::new();
        meta.entry_count = flags_data.len() as u64;
        meta.set_column(crate::index::meta::ColumnBit::Flags);
        meta.write_to(dir.join("meta")).unwrap();

        let mut writer = ColumnWriter::<u32>::create(dir.join("flags")).unwrap();
        writer.push_batch(flags_data).unwrap();
        writer.flush().unwrap();
    }

    // --- flags() ---

    #[test]
    fn test_flags_access() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");
        let flags = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        create_test_index(&idx_dir, &[flags, SEVERITY_INFO, 0]);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), 3).unwrap(),
            checkpoints: None,
        };

        assert_eq!(reader.flags(0), Some(flags));
        assert_eq!(reader.flags(1), Some(SEVERITY_INFO));
        assert_eq!(reader.flags(2), Some(0));
        assert_eq!(reader.flags(3), None);
    }

    // --- len() / is_empty() ---

    #[test]
    fn test_len_and_empty() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");
        create_test_index(&idx_dir, &[0, 0, 0]);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), 3).unwrap(),
            checkpoints: None,
        };

        assert_eq!(reader.len(), 3);
        assert!(!reader.is_empty());
    }

    #[test]
    fn test_empty_index() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");
        create_test_index(&idx_dir, &[]);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), 0).unwrap(),
            checkpoints: None,
        };

        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
    }

    // --- scan_flags() ---

    #[test]
    fn test_scan_flags_json_errors() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");

        let flags_data = vec![
            SEVERITY_ERROR | FLAG_FORMAT_JSON,  // line 0: JSON error
            SEVERITY_INFO | FLAG_FORMAT_JSON,   // line 1: JSON info
            SEVERITY_ERROR,                     // line 2: non-JSON error
            SEVERITY_ERROR | FLAG_FORMAT_JSON,  // line 3: JSON error
            FLAG_FORMAT_JSON,                   // line 4: JSON unknown severity
            SEVERITY_WARN | FLAG_FORMAT_LOGFMT, // line 5: logfmt warn
        ];
        create_test_index(&idx_dir, &flags_data);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), flags_data.len()).unwrap(),
            checkpoints: None,
        };

        let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
        let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let candidates = reader.scan_flags(mask, want, flags_data.len());

        assert_eq!(candidates, vec![0, 3]);
    }

    #[test]
    fn test_scan_flags_json_only() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");

        let flags_data = vec![
            FLAG_FORMAT_JSON | SEVERITY_INFO,
            SEVERITY_ERROR,
            FLAG_FORMAT_JSON | SEVERITY_ERROR,
            FLAG_FORMAT_LOGFMT | SEVERITY_WARN,
        ];
        create_test_index(&idx_dir, &flags_data);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), flags_data.len()).unwrap(),
            checkpoints: None,
        };

        let mask = FLAG_FORMAT_JSON;
        let want = FLAG_FORMAT_JSON;
        let candidates = reader.scan_flags(mask, want, flags_data.len());

        assert_eq!(candidates, vec![0, 2]);
    }

    #[test]
    fn test_scan_flags_with_limit() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");

        let flags_data = vec![
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
            FLAG_FORMAT_JSON,
        ];
        create_test_index(&idx_dir, &flags_data);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), flags_data.len()).unwrap(),
            checkpoints: None,
        };

        let candidates = reader.scan_flags(FLAG_FORMAT_JSON, FLAG_FORMAT_JSON, 2);
        assert_eq!(candidates, vec![0, 1]);
    }

    #[test]
    fn test_scan_flags_no_matches() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");

        let flags_data = vec![SEVERITY_INFO, SEVERITY_WARN, SEVERITY_DEBUG];
        create_test_index(&idx_dir, &flags_data);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), flags_data.len()).unwrap(),
            checkpoints: None,
        };

        let candidates = reader.scan_flags(FLAG_FORMAT_JSON, FLAG_FORMAT_JSON, flags_data.len());
        assert!(candidates.is_empty());
    }

    // --- candidate_bitmap() ---

    #[test]
    fn test_candidate_bitmap() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("test.idx");

        let flags_data = vec![
            SEVERITY_ERROR | FLAG_FORMAT_JSON,
            SEVERITY_INFO | FLAG_FORMAT_JSON,
            SEVERITY_ERROR,
            SEVERITY_ERROR | FLAG_FORMAT_JSON,
        ];
        create_test_index(&idx_dir, &flags_data);

        let reader = IndexReader {
            flags: ColumnReader::<u32>::open(idx_dir.join("flags"), flags_data.len()).unwrap(),
            checkpoints: None,
        };

        let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
        let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
        let bitmap = reader.candidate_bitmap(mask, want, flags_data.len());

        assert_eq!(bitmap, vec![true, false, false, true]);
    }
}
