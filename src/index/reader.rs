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

    /// Access the checkpoint reader (for severity histogram).
    pub fn checkpoints(&self) -> Option<&CheckpointReader> {
        self.checkpoints.as_ref()
    }
}
