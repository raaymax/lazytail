//! LogSource — domain facade combining FileReader + IndexReader + metadata.
//!
//! File-backed sources only. Stdin/pipe tabs don't get a LogSource —
//! they continue using `StreamReader` directly.

use crate::index::reader::{IndexReader, IndexStats};
use crate::reader::file_reader::FileReader;
use crate::reader::LogReader;
use crate::source::index_dir_for_log;
use anyhow::Result;
use std::path::{Path, PathBuf};

/// Calculate the total size of all files in the index directory.
fn calculate_index_size(log_path: &Path) -> Option<u64> {
    let index_dir = index_dir_for_log(log_path);
    if !index_dir.exists() || !index_dir.is_dir() {
        return None;
    }

    let mut total_size = 0u64;
    let entries = std::fs::read_dir(&index_dir).ok()?;

    for entry in entries.flatten() {
        if let Ok(metadata) = entry.metadata() {
            if metadata.is_file() {
                total_size += metadata.len();
            }
        }
    }

    Some(total_size)
}

/// Combined file reader + index reader + metadata for a file-backed log source.
pub struct LogSource {
    path: PathBuf,
    reader: FileReader,
    index: Option<IndexReader>,
    file_size: u64,
    index_size: Option<u64>,
}

impl LogSource {
    /// Open a log source from a file path.
    ///
    /// Initializes the file reader, attempts to load the columnar index,
    /// and gathers file/index metadata.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        let metadata = std::fs::metadata(&path)?;
        let file_size = metadata.len();

        let reader = FileReader::new(&path)?;
        let index = IndexReader::open(&path);
        let index_size = index.as_ref().and_then(|_| calculate_index_size(&path));

        Ok(Self {
            path,
            reader,
            index,
            file_size,
            index_size,
        })
    }

    // --- Reader delegation ---

    pub fn total_lines(&self) -> usize {
        self.reader.total_lines()
    }

    pub fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        self.reader.get_line(index)
    }

    pub fn reload(&mut self) -> Result<()> {
        self.reader.reload()
    }

    /// Get the underlying FileReader (for wrapping in Arc<Mutex<dyn LogReader>>).
    pub fn into_reader(self) -> FileReader {
        self.reader
    }

    /// Decompose into parts for TabState construction.
    ///
    /// Returns (FileReader, Option<IndexReader>, file_size, index_size).
    pub fn into_parts(self) -> (FileReader, Option<IndexReader>, u64, Option<u64>) {
        (self.reader, self.index, self.file_size, self.index_size)
    }

    // --- Index access ---

    pub fn has_index(&self) -> bool {
        self.index.is_some()
    }

    pub fn index(&self) -> Option<&IndexReader> {
        self.index.as_ref()
    }

    /// Take ownership of the IndexReader (for TabState construction).
    pub fn take_index(&mut self) -> Option<IndexReader> {
        self.index.take()
    }

    /// Reload the index from disk.
    pub fn reload_index(&mut self) {
        self.index = IndexReader::open(&self.path);
        self.index_size = self
            .index
            .as_ref()
            .and_then(|_| calculate_index_size(&self.path));
    }

    pub fn index_stats(&self) -> Option<IndexStats> {
        IndexReader::stats(&self.path)
    }

    // --- Metadata ---

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn file_size(&self) -> u64 {
        self.file_size
    }

    pub fn index_size(&self) -> Option<u64> {
        self.index_size
    }
}
