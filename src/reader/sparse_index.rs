/// Sparse index entry - one per `interval` lines
#[derive(Clone, Copy, Debug)]
struct IndexEntry {
    /// Line number (supports up to 4 billion lines)
    line_number: u32,
    /// Byte offset in the file where this line starts
    byte_offset: u64,
}

/// Sparse line index with configurable interval
///
/// Instead of storing byte offset for every line (O(n) memory),
/// we store only every Nth line (O(n/interval) memory).
/// For 100M lines with interval=10000, this uses ~120KB instead of ~800MB.
#[derive(Debug)]
pub struct SparseIndex {
    /// Indexed entries (one per `interval` lines)
    entries: Vec<IndexEntry>,
    /// Lines between index entries (default: 10,000)
    interval: usize,
    /// Total number of lines in the file
    total_lines: usize,
    /// Byte offset at end of last indexed region (for incremental indexing)
    last_indexed_offset: u64,
}

impl SparseIndex {
    /// Create a new sparse index with the given interval
    pub fn new(interval: usize) -> Self {
        Self {
            entries: Vec::new(),
            interval: interval.max(1), // Minimum interval of 1
            total_lines: 0,
            last_indexed_offset: 0,
        }
    }

    /// Get the indexing interval
    pub fn interval(&self) -> usize {
        self.interval
    }

    /// Get total number of lines
    pub fn total_lines(&self) -> usize {
        self.total_lines
    }

    /// Set total lines (called after scanning)
    pub fn set_total_lines(&mut self, total: usize) {
        self.total_lines = total;
    }

    /// Find byte offset for a line number
    /// Returns (nearest_indexed_offset, lines_to_skip)
    pub fn locate(&self, line: usize) -> (u64, usize) {
        if self.entries.is_empty() || line == 0 {
            return (0, line);
        }

        // Find which chunk this line belongs to
        let chunk = line / self.interval;

        // Get the offset from the entry (or 0 if before first entry)
        if chunk == 0 {
            (0, line)
        } else if chunk <= self.entries.len() {
            let entry = &self.entries[chunk - 1];
            let skip = line - entry.line_number as usize;
            (entry.byte_offset, skip)
        } else {
            // Line is beyond indexed region, use last entry
            if let Some(last) = self.entries.last() {
                let skip = line - last.line_number as usize;
                (last.byte_offset, skip)
            } else {
                (0, line)
            }
        }
    }

    /// Append a new index entry (called every `interval` lines during scanning)
    pub fn append(&mut self, line_number: usize, byte_offset: u64) {
        self.entries.push(IndexEntry {
            line_number: line_number as u32,
            byte_offset,
        });
        self.last_indexed_offset = byte_offset;
    }

    /// Get the byte offset where incremental indexing should resume
    #[cfg(test)]
    pub fn last_indexed_offset(&self) -> u64 {
        self.last_indexed_offset
    }

    /// Get the line number of the last indexed entry
    #[cfg(test)]
    pub fn last_indexed_line(&self) -> usize {
        self.entries
            .last()
            .map(|e| e.line_number as usize)
            .unwrap_or(0)
    }

    /// Clear the index (for reload after truncation)
    pub fn clear(&mut self) {
        self.entries.clear();
        self.total_lines = 0;
        self.last_indexed_offset = 0;
    }

    /// Get memory usage in bytes (approximate)
    #[cfg(test)]
    pub fn memory_usage(&self) -> usize {
        self.entries.len() * std::mem::size_of::<IndexEntry>() + std::mem::size_of::<Self>()
    }

    /// Get number of index entries
    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_sparse_index() {
        let index = SparseIndex::new(1000);
        assert_eq!(index.interval(), 1000);
        assert_eq!(index.total_lines(), 0);
        assert_eq!(index.entry_count(), 0);
    }

    #[test]
    fn test_minimum_interval() {
        let index = SparseIndex::new(0);
        assert_eq!(index.interval(), 1); // Should be clamped to 1
    }

    #[test]
    fn test_locate_empty_index() {
        let index = SparseIndex::new(100);
        let (offset, skip) = index.locate(50);
        assert_eq!(offset, 0);
        assert_eq!(skip, 50);
    }

    #[test]
    fn test_locate_line_zero() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);
        let (offset, skip) = index.locate(0);
        assert_eq!(offset, 0);
        assert_eq!(skip, 0);
    }

    #[test]
    fn test_locate_before_first_entry() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);

        // Line 50 is before first entry (line 100)
        let (offset, skip) = index.locate(50);
        assert_eq!(offset, 0);
        assert_eq!(skip, 50);
    }

    #[test]
    fn test_locate_at_entry() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);
        index.append(200, 2000);

        // Exactly at entry
        let (offset, skip) = index.locate(100);
        assert_eq!(offset, 1000);
        assert_eq!(skip, 0);
    }

    #[test]
    fn test_locate_between_entries() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);
        index.append(200, 2000);

        // Line 150 is between entries at 100 and 200
        let (offset, skip) = index.locate(150);
        assert_eq!(offset, 1000);
        assert_eq!(skip, 50);
    }

    #[test]
    fn test_locate_after_last_entry() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);
        index.append(200, 2000);

        // Line 250 is after last entry
        let (offset, skip) = index.locate(250);
        assert_eq!(offset, 2000);
        assert_eq!(skip, 50);
    }

    #[test]
    fn test_append_updates_last_offset() {
        let mut index = SparseIndex::new(100);
        assert_eq!(index.last_indexed_offset(), 0);

        index.append(100, 1000);
        assert_eq!(index.last_indexed_offset(), 1000);
        assert_eq!(index.last_indexed_line(), 100);

        index.append(200, 2000);
        assert_eq!(index.last_indexed_offset(), 2000);
        assert_eq!(index.last_indexed_line(), 200);
    }

    #[test]
    fn test_clear() {
        let mut index = SparseIndex::new(100);
        index.append(100, 1000);
        index.set_total_lines(150);

        index.clear();

        assert_eq!(index.entry_count(), 0);
        assert_eq!(index.total_lines(), 0);
        assert_eq!(index.last_indexed_offset(), 0);
    }

    #[test]
    fn test_memory_usage() {
        let mut index = SparseIndex::new(100);
        let base_usage = index.memory_usage();

        // Add some entries
        for i in 1..=10 {
            index.append(i * 100, i as u64 * 1000);
        }

        let with_entries = index.memory_usage();
        // Should have grown by approximately 10 * size_of::<IndexEntry>()
        assert!(with_entries > base_usage);
        assert_eq!(
            with_entries - base_usage,
            10 * std::mem::size_of::<IndexEntry>()
        );
    }

    #[test]
    fn test_large_line_numbers() {
        let mut index = SparseIndex::new(10_000);

        // Simulate 100M lines with interval 10K = 10K entries
        for i in 1..=100 {
            let line = i * 10_000;
            let offset = i as u64 * 100_000; // ~100 bytes per line avg
            index.append(line, offset);
        }

        index.set_total_lines(1_000_000);

        // Locate line 555_555
        let (offset, skip) = index.locate(555_555);
        // Should use entry at line 550_000 (entry index 54)
        assert_eq!(offset, 55 * 100_000);
        assert_eq!(skip, 555_555 - 550_000);
    }

    #[test]
    fn test_entry_count() {
        let mut index = SparseIndex::new(100);
        assert_eq!(index.entry_count(), 0);

        index.append(100, 1000);
        assert_eq!(index.entry_count(), 1);

        index.append(200, 2000);
        assert_eq!(index.entry_count(), 2);
    }
}
