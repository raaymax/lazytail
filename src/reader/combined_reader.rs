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
}

impl CombinedReader {
    pub fn new(sources: Vec<SourceEntry>) -> Self {
        let mut reader = Self {
            sources,
            merged: Vec::new(),
        };
        reader.build_merged();
        reader
    }

    /// Rebuild the merged line list from all sources, sorted by timestamp.
    fn build_merged(&mut self) {
        self.merged.clear();

        for (source_id, source) in self.sources.iter().enumerate() {
            for line in 0..source.total_lines {
                let timestamp = source
                    .index_reader
                    .as_ref()
                    .and_then(|ir| ir.get_timestamp(line))
                    .unwrap_or(0);

                self.merged.push(MergedLine {
                    source_id,
                    file_line: line,
                    timestamp,
                });
            }
        }

        // Stable sort: ties broken by source order, then line order within source
        self.merged.sort_by_key(|m| m.timestamp);
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
        let mut reader = self.sources[m.source_id].reader.lock().unwrap();
        reader.get_line(m.file_line)
    }

    fn reload(&mut self) -> Result<()> {
        // Reload each source reader and refresh index readers
        for source in &mut self.sources {
            let mut reader = source.reader.lock().unwrap();
            reader.reload()?;
            source.total_lines = reader.total_lines();
            drop(reader);

            if let (Some(ref mut ir), Some(ref path)) =
                (&mut source.index_reader, &source.source_path)
            {
                ir.refresh(path);
            }
        }
        self.build_merged();
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
}
