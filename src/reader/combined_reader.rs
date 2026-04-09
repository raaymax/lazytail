use crate::index::flags::Severity;
use crate::index::reader::IndexReader;
use crate::reader::LogReader;
use anyhow::Result;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// A source contributing lines to the combined view.
pub struct SourceEntry {
    pub name: String,
    pub reader: Arc<Mutex<dyn LogReader + Send>>,
    pub index_reader: Option<IndexReader>,
    pub source_path: Option<PathBuf>,
    pub total_lines: usize,
    pub renderer_names: Vec<String>,
}

/// A merged line reference: which source, which line in that source.
#[derive(Clone, Copy)]
pub struct MergedLine {
    pub source_id: usize,
    pub file_line: usize,
    pub timestamp: u64,
}

/// A reader that merges lines from multiple sources in chronological order.
///
/// Implements `LogReader` so it works transparently with the existing
/// rendering pipeline, viewport, and filter infrastructure.
///
/// # Locking
///
/// `CombinedReader` shares `Arc<Mutex<dyn LogReader>>` handles with the
/// original source tabs. When the combined tab renders, the outer
/// `CombinedReader` mutex is held while `get_line()` acquires the inner
/// source reader mutex. This is safe from deadlocks because lock ordering
/// is always outer→inner (never reversed). However, if a filter thread
/// holds a source reader lock, `get_line()` will block until the filter
/// releases it, which may cause brief render stalls.
pub struct CombinedReader {
    sources: Vec<SourceEntry>,
    merged: Vec<MergedLine>,
    /// Previous total_lines per source, for incremental append on reload.
    prev_totals: Vec<usize>,
}

impl CombinedReader {
    pub fn new(sources: Vec<SourceEntry>) -> Self {
        let prev_totals = sources.iter().map(|s| s.total_lines).collect();
        let mut reader = Self {
            sources,
            merged: Vec::new(),
            prev_totals,
        };
        reader.build_merged();
        reader
    }

    /// Get the timestamp for a line, carrying forward the last known timestamp
    /// from the same source when the index hasn't caught up yet.
    fn get_timestamp(source: &SourceEntry, line: usize, last_ts: &mut u64) -> u64 {
        let ts = source
            .index_reader
            .as_ref()
            .and_then(|ir| ir.get_timestamp(line))
            .unwrap_or(*last_ts);
        *last_ts = ts;
        ts
    }

    /// Rebuild the merged line list from all sources, sorted by timestamp.
    fn build_merged(&mut self) {
        self.merged.clear();

        for (source_id, source) in self.sources.iter().enumerate() {
            let mut last_ts = 0u64;
            for line in 0..source.total_lines {
                let timestamp = Self::get_timestamp(source, line, &mut last_ts);
                self.merged.push(MergedLine {
                    source_id,
                    file_line: line,
                    timestamp,
                });
            }
        }

        // Deterministic sort: timestamp first, then source order, then line order
        self.merged
            .sort_by_key(|m| (m.timestamp, m.source_id, m.file_line));
    }

    /// Sort key for deterministic ordering.
    #[inline]
    fn sort_key(m: &MergedLine) -> (u64, usize, usize) {
        (m.timestamp, m.source_id, m.file_line)
    }

    /// Append only new lines from sources that grew since the last reload.
    ///
    /// Collects new lines into a small temp vec, sorts it, then merges into
    /// the already-sorted `merged` list. For K new lines into N existing:
    /// - Fast path (all new lines >= last existing): O(K log K) — just append
    /// - Small K path (interleaving, K small): O(K × (log N + shift)) — binary insert
    /// - Large K path (interleaving, K large): O(N + K) — full merge
    ///
    /// The small-K path avoids allocating a second N-element vec, which would
    /// otherwise thrash CPU caches for large N (e.g. 66M entries = 1.6 GB).
    fn append_new_lines(&mut self) {
        let mut new_lines = Vec::new();
        for (source_id, source) in self.sources.iter().enumerate() {
            let prev = self.prev_totals[source_id];
            if source.total_lines > prev {
                // Carry forward the last known timestamp from this source
                // so new lines without index data sort near their true position.
                let mut last_ts = if prev > 0 {
                    source
                        .index_reader
                        .as_ref()
                        .and_then(|ir| ir.get_timestamp(prev - 1))
                        .unwrap_or(0)
                } else {
                    0
                };
                for line in prev..source.total_lines {
                    let timestamp = Self::get_timestamp(source, line, &mut last_ts);
                    new_lines.push(MergedLine {
                        source_id,
                        file_line: line,
                        timestamp,
                    });
                }
            }
        }

        if new_lines.is_empty() {
            return;
        }

        new_lines.sort_by_key(Self::sort_key);

        // Fast path: if all new lines sort after all existing lines, just append.
        // This is the common case for append-only logs with monotonic timestamps.
        let can_append = self.merged.is_empty()
            || Self::sort_key(new_lines.first().unwrap())
                >= Self::sort_key(self.merged.last().unwrap());

        if can_append {
            self.merged.extend(new_lines);
        } else if new_lines.len() <= 64 {
            // Small K: binary-search insert each new line in place.
            // For live logs, new lines typically insert near the end so the
            // element shift is tiny. This avoids a full O(N) merge + 2×N
            // allocation that would thrash CPU caches for large merged vecs.
            // Process in reverse so earlier insertions don't shift later positions.
            for line in new_lines.into_iter().rev() {
                let key = Self::sort_key(&line);
                let pos = self.merged.partition_point(|m| Self::sort_key(m) <= key);
                self.merged.insert(pos, line);
            }
        } else {
            // Large K: merge two sorted sequences
            let old = std::mem::take(&mut self.merged);
            self.merged.reserve(old.len() + new_lines.len());

            let mut i = 0;
            let mut j = 0;
            while i < old.len() && j < new_lines.len() {
                if Self::sort_key(&old[i]) <= Self::sort_key(&new_lines[j]) {
                    self.merged.push(old[i]);
                    i += 1;
                } else {
                    self.merged.push(new_lines[j]);
                    j += 1;
                }
            }
            self.merged.extend_from_slice(&old[i..]);
            self.merged.extend_from_slice(&new_lines[j..]);
        }
    }

    /// Get source info for a virtual line index (for rendering source prefix).
    pub fn source_info(
        &self,
        virtual_idx: usize,
        source_colors: &[ratatui::style::Color],
    ) -> Option<(&str, ratatui::style::Color)> {
        let m = self.merged.get(virtual_idx)?;
        let name = &self.sources[m.source_id].name;
        let color = source_colors[m.source_id % source_colors.len()];
        Some((name, color))
    }

    /// Get the renderer_names for the source that owns a given virtual line.
    pub fn renderer_names(&self, virtual_idx: usize) -> &[String] {
        let Some(m) = self.merged.get(virtual_idx) else {
            return &[];
        };
        &self.sources[m.source_id].renderer_names
    }

    /// Get the arrival timestamp (epoch ms) for a virtual line.
    pub fn timestamp(&self, virtual_idx: usize) -> Option<u64> {
        let m = self.merged.get(virtual_idx)?;
        if m.timestamp > 0 {
            Some(m.timestamp)
        } else {
            None
        }
    }

    /// Get severity for a virtual line from the originating source's IndexReader.
    pub fn severity(&self, virtual_idx: usize) -> Severity {
        let Some(m) = self.merged.get(virtual_idx) else {
            return Severity::Unknown;
        };
        self.sources[m.source_id]
            .index_reader
            .as_ref()
            .map(|ir| ir.severity(m.file_line))
            .unwrap_or(Severity::Unknown)
    }
}

impl LogReader for CombinedReader {
    fn total_lines(&self) -> usize {
        self.merged.len()
    }

    fn get_line(&mut self, index: usize) -> Result<Option<String>> {
        let Some(m) = self.merged.get(index).copied() else {
            return Ok(None);
        };
        let mut reader = match self.sources[m.source_id].reader.lock() {
            Ok(guard) => guard,
            Err(poisoned) => poisoned.into_inner(),
        };
        reader.get_line(m.file_line)
    }

    fn reload(&mut self) -> Result<()> {
        // Reload each source reader and refresh index readers.
        // Individual source failures (e.g. deleted file) are skipped gracefully.
        let mut any_truncated = false;
        let mut index_gained = false;
        for (i, source) in self.sources.iter_mut().enumerate() {
            let mut reader = match source.reader.lock() {
                Ok(guard) => guard,
                Err(poisoned) => poisoned.into_inner(),
            };
            if let Err(_e) = reader.reload() {
                continue;
            }
            self.prev_totals[i] = source.total_lines;
            source.total_lines = reader.total_lines();
            drop(reader);

            if source.total_lines < self.prev_totals[i] {
                any_truncated = true;
            }

            if let Some(ref mut ir) = source.index_reader {
                if let Some(ref path) = source.source_path {
                    ir.refresh(path);
                }
            } else if let Some(ref path) = source.source_path {
                // Index didn't exist when combined tab was created — retry.
                if let Some(ir) = IndexReader::open(path) {
                    source.index_reader = Some(ir);
                    index_gained = true;
                }
            }
        }

        // If any source was truncated, or a new index appeared (meaning lines
        // that previously had no timestamp can now be positioned correctly),
        // do a full rebuild.
        if any_truncated || index_gained {
            self.build_merged();
        } else {
            self.append_new_lines();
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::reader::stream_reader::StreamReader;
    use crate::reader::StreamableReader;

    fn make_source(name: &str, lines: Vec<&str>) -> SourceEntry {
        let mut reader = StreamReader::new_incremental();
        reader.append_lines(lines.into_iter().map(|s| s.to_string()).collect());
        reader.mark_complete();
        let total_lines = reader.total_lines();
        SourceEntry {
            name: name.to_string(),
            reader: Arc::new(Mutex::new(reader)),
            index_reader: None,
            source_path: None,
            total_lines,
            renderer_names: Vec::new(),
        }
    }

    #[test]
    fn test_combined_reader_total_lines() {
        let sources = vec![
            make_source("a", vec!["a1", "a2", "a3"]),
            make_source("b", vec!["b1", "b2"]),
        ];
        let reader = CombinedReader::new(sources);
        assert_eq!(reader.total_lines(), 5);
    }

    #[test]
    fn test_combined_reader_get_line() {
        let sources = vec![
            make_source("a", vec!["a1", "a2"]),
            make_source("b", vec!["b1"]),
        ];
        let mut reader = CombinedReader::new(sources);

        // Without timestamps, all get timestamp 0 — order is stable (a lines first, then b)
        assert_eq!(reader.get_line(0).unwrap(), Some("a1".to_string()));
        assert_eq!(reader.get_line(1).unwrap(), Some("a2".to_string()));
        assert_eq!(reader.get_line(2).unwrap(), Some("b1".to_string()));
        assert_eq!(reader.get_line(3).unwrap(), None);
    }

    #[test]
    fn test_combined_reader_source_info() {
        let colors = [ratatui::style::Color::Cyan, ratatui::style::Color::Green];
        let sources = vec![
            make_source("api", vec!["line1"]),
            make_source("web", vec!["line2"]),
        ];
        let reader = CombinedReader::new(sources);

        let (name, color) = reader.source_info(0, &colors).unwrap();
        assert_eq!(name, "api");
        assert_eq!(color, ratatui::style::Color::Cyan);
        let (name, color) = reader.source_info(1, &colors).unwrap();
        assert_eq!(name, "web");
        assert_eq!(color, ratatui::style::Color::Green);
        assert!(reader.source_info(2, &colors).is_none());
    }

    #[test]
    fn test_combined_reader_empty_sources() {
        let sources = vec![make_source("empty", vec![])];
        let reader = CombinedReader::new(sources);
        assert_eq!(reader.total_lines(), 0);
    }

    #[test]
    fn test_combined_reader_reload() {
        let sources = vec![make_source("a", vec!["a1"])];
        let mut reader = CombinedReader::new(sources);
        assert_eq!(reader.total_lines(), 1);
        // reload should not error
        reader.reload().unwrap();
        assert_eq!(reader.total_lines(), 1);
    }

    #[test]
    fn test_append_new_lines_binary_insert() {
        // Test the binary insert path: new lines interleave with existing.
        let sources = vec![
            make_source("a", vec!["a1", "a2"]),
            make_source("b", vec!["b1"]),
        ];
        let mut reader = CombinedReader::new(sources);

        // Set up a merged list with known timestamps
        reader.merged.clear();
        reader.merged.push(MergedLine {
            source_id: 0,
            file_line: 0,
            timestamp: 10,
        });
        reader.merged.push(MergedLine {
            source_id: 0,
            file_line: 1,
            timestamp: 30,
        });
        reader.merged.push(MergedLine {
            source_id: 1,
            file_line: 0,
            timestamp: 50,
        });

        // Insert a line with timestamp 20 — goes between positions 0 and 1
        let new_line = MergedLine {
            source_id: 1,
            file_line: 1,
            timestamp: 20,
        };
        let key = CombinedReader::sort_key(&new_line);
        let pos = reader
            .merged
            .partition_point(|m| CombinedReader::sort_key(m) <= key);
        reader.merged.insert(pos, new_line);

        assert_eq!(reader.merged.len(), 4);
        assert_eq!(reader.merged[0].timestamp, 10);
        assert_eq!(reader.merged[1].timestamp, 20);
        assert_eq!(reader.merged[2].timestamp, 30);
        assert_eq!(reader.merged[3].timestamp, 50);
    }

    #[test]
    fn test_timestamp_carry_forward_for_unindexed_lines() {
        // Source "a" has timestamps for lines 0-1 but not line 2 (index lagging).
        // Source "b" has timestamps for all lines.
        // Line a:2 should carry forward a's last known timestamp (200), not get 0.
        let mut source_a = make_source("a", vec!["a1", "a2", "a3"]);
        source_a.index_reader = Some(IndexReader::with_timestamps(&[100, 200])); // only 2 of 3 indexed

        let mut source_b = make_source("b", vec!["b1", "b2"]);
        source_b.index_reader = Some(IndexReader::with_timestamps(&[150, 250]));

        let mut reader = CombinedReader::new(vec![source_a, source_b]);

        // Expected order by timestamp: a1(100), b1(150), a2(200), a3(200 carry), b2(250)
        assert_eq!(reader.total_lines(), 5);
        assert_eq!(reader.get_line(0).unwrap(), Some("a1".to_string())); // ts=100
        assert_eq!(reader.get_line(1).unwrap(), Some("b1".to_string())); // ts=150
        assert_eq!(reader.get_line(2).unwrap(), Some("a2".to_string())); // ts=200
        assert_eq!(reader.get_line(3).unwrap(), Some("a3".to_string())); // ts=200 (carried)
        assert_eq!(reader.get_line(4).unwrap(), Some("b2".to_string())); // ts=250
    }

    #[test]
    fn test_no_index_lines_sort_to_beginning() {
        // Source without any index — all lines get timestamp 0, sort stably at start.
        let source_a = make_source("a", vec!["a1", "a2"]);
        let mut source_b = make_source("b", vec!["b1"]);
        source_b.index_reader = Some(IndexReader::with_timestamps(&[100]));

        let mut reader = CombinedReader::new(vec![source_a, source_b]);

        // a lines (ts=0) come first, then b1 (ts=100)
        assert_eq!(reader.get_line(0).unwrap(), Some("a1".to_string()));
        assert_eq!(reader.get_line(1).unwrap(), Some("a2".to_string()));
        assert_eq!(reader.get_line(2).unwrap(), Some("b1".to_string()));
    }

    #[test]
    fn test_interleaved_timestamps_merge_correctly() {
        // Two sources with interleaved timestamps should merge in timestamp order.
        let mut source_a = make_source("a", vec!["a1", "a2", "a3"]);
        source_a.index_reader = Some(IndexReader::with_timestamps(&[10, 30, 50]));

        let mut source_b = make_source("b", vec!["b1", "b2", "b3"]);
        source_b.index_reader = Some(IndexReader::with_timestamps(&[20, 40, 60]));

        let mut reader = CombinedReader::new(vec![source_a, source_b]);

        assert_eq!(reader.total_lines(), 6);
        assert_eq!(reader.get_line(0).unwrap(), Some("a1".to_string())); // ts=10
        assert_eq!(reader.get_line(1).unwrap(), Some("b1".to_string())); // ts=20
        assert_eq!(reader.get_line(2).unwrap(), Some("a2".to_string())); // ts=30
        assert_eq!(reader.get_line(3).unwrap(), Some("b2".to_string())); // ts=40
        assert_eq!(reader.get_line(4).unwrap(), Some("a3".to_string())); // ts=50
        assert_eq!(reader.get_line(5).unwrap(), Some("b3".to_string())); // ts=60
    }

    #[test]
    fn test_reload_picks_up_new_index() {
        use crate::index::column::ColumnWriter;
        use crate::index::meta::{ColumnBit, IndexMeta};
        use crate::source::index_dir_for_log;

        let dir = tempfile::tempdir().unwrap();

        // Create two log files — source_a gets an index, source_b starts without one.
        let log_a = dir.path().join("a.log");
        let log_b = dir.path().join("b.log");
        std::fs::write(&log_a, "a1\na2\n").unwrap();
        std::fs::write(&log_b, "b1\nb2\n").unwrap();

        // Build index for source_a with timestamps [200, 400]
        {
            let idx = index_dir_for_log(&log_a);
            std::fs::create_dir_all(&idx).unwrap();

            let mut offsets = ColumnWriter::<u64>::create(idx.join("offsets")).unwrap();
            offsets.push(0u64).unwrap();
            offsets.push(3u64).unwrap();
            drop(offsets);

            let mut flags = ColumnWriter::<u32>::create(idx.join("flags")).unwrap();
            flags.push(0u32).unwrap();
            flags.push(0u32).unwrap();
            drop(flags);

            let mut time = ColumnWriter::<u64>::create(idx.join("time")).unwrap();
            time.push(200u64).unwrap();
            time.push(400u64).unwrap();
            drop(time);

            let mut meta = IndexMeta::new();
            meta.entry_count = 2;
            meta.log_file_size = 6;
            meta.set_column(ColumnBit::Offsets);
            meta.set_column(ColumnBit::Flags);
            meta.set_column(ColumnBit::Time);
            meta.write_to(idx.join("meta")).unwrap();
        }

        // Create sources — source_b has no index yet.
        let source_a = SourceEntry {
            name: "a".into(),
            reader: Arc::new(Mutex::new(
                crate::reader::file_reader::FileReader::new(&log_a).unwrap(),
            )),
            index_reader: IndexReader::open(&log_a),
            source_path: Some(log_a.clone()),
            total_lines: 2,
            renderer_names: Vec::new(),
        };
        let source_b = SourceEntry {
            name: "b".into(),
            reader: Arc::new(Mutex::new(
                crate::reader::file_reader::FileReader::new(&log_b).unwrap(),
            )),
            index_reader: None, // no index yet
            source_path: Some(log_b.clone()),
            total_lines: 2,
            renderer_names: Vec::new(),
        };

        let mut reader = CombinedReader::new(vec![source_a, source_b]);

        // Before index: b lines have ts=0, sort before a lines (ts=200,400)
        assert_eq!(reader.get_line(0).unwrap(), Some("b1".to_string())); // ts=0
        assert_eq!(reader.get_line(1).unwrap(), Some("b2".to_string())); // ts=0
        assert_eq!(reader.get_line(2).unwrap(), Some("a1".to_string())); // ts=200
        assert_eq!(reader.get_line(3).unwrap(), Some("a2".to_string())); // ts=400

        // Now create index for source_b with timestamps [100, 300]
        {
            let idx = index_dir_for_log(&log_b);
            std::fs::create_dir_all(&idx).unwrap();

            let mut offsets = ColumnWriter::<u64>::create(idx.join("offsets")).unwrap();
            offsets.push(0u64).unwrap();
            offsets.push(3u64).unwrap();
            drop(offsets);

            let mut flags = ColumnWriter::<u32>::create(idx.join("flags")).unwrap();
            flags.push(0u32).unwrap();
            flags.push(0u32).unwrap();
            drop(flags);

            let mut time = ColumnWriter::<u64>::create(idx.join("time")).unwrap();
            time.push(100u64).unwrap();
            time.push(300u64).unwrap();
            drop(time);

            let mut meta = IndexMeta::new();
            meta.entry_count = 2;
            meta.log_file_size = 6;
            meta.set_column(ColumnBit::Offsets);
            meta.set_column(ColumnBit::Flags);
            meta.set_column(ColumnBit::Time);
            meta.write_to(idx.join("meta")).unwrap();
        }

        // Reload — should discover the new index and rebuild with correct ordering.
        reader.reload().unwrap();

        // After index: b1(100), a1(200), b2(300), a2(400)
        assert_eq!(reader.get_line(0).unwrap(), Some("b1".to_string())); // ts=100
        assert_eq!(reader.get_line(1).unwrap(), Some("a1".to_string())); // ts=200
        assert_eq!(reader.get_line(2).unwrap(), Some("b2".to_string())); // ts=300
        assert_eq!(reader.get_line(3).unwrap(), Some("a2".to_string())); // ts=400
    }
}
