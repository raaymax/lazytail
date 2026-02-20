pub mod cancel;
pub mod engine;
pub mod orchestrator;
#[allow(dead_code)]
pub mod parallel_engine;
pub mod query;
pub mod regex_filter;
pub mod search_engine;
pub mod streaming_filter;
pub mod string_filter;

/// Trait for extensible filtering
pub trait Filter: Send + Sync {
    fn matches(&self, line: &str) -> bool;
}

use serde::{Deserialize, Serialize};

/// Filter mode for switching between plain text and regex filtering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FilterMode {
    Plain { case_sensitive: bool },
    Regex { case_sensitive: bool },
}

impl Default for FilterMode {
    fn default() -> Self {
        FilterMode::Plain {
            case_sensitive: false,
        }
    }
}

impl FilterMode {
    /// Create a new plain text filter mode (case-insensitive by default)
    #[allow(dead_code)] // Public API for external use and tests
    pub fn plain() -> Self {
        FilterMode::Plain {
            case_sensitive: false,
        }
    }

    /// Create a new regex filter mode (case-insensitive by default)
    #[allow(dead_code)] // Public API for external use and tests
    pub fn regex() -> Self {
        FilterMode::Regex {
            case_sensitive: false,
        }
    }

    /// Toggle between Plain and Regex modes, preserving case sensitivity
    pub fn toggle_mode(&mut self) {
        *self = match *self {
            FilterMode::Plain { case_sensitive } => FilterMode::Regex { case_sensitive },
            FilterMode::Regex { case_sensitive } => FilterMode::Plain { case_sensitive },
        };
    }

    /// Toggle case sensitivity within the current mode
    pub fn toggle_case_sensitivity(&mut self) {
        match self {
            FilterMode::Plain { case_sensitive } => *case_sensitive = !*case_sensitive,
            FilterMode::Regex { case_sensitive } => *case_sensitive = !*case_sensitive,
        }
    }

    /// Check if current mode is regex
    pub fn is_regex(&self) -> bool {
        matches!(self, FilterMode::Regex { .. })
    }

    /// Check if current mode is case sensitive
    pub fn is_case_sensitive(&self) -> bool {
        match self {
            FilterMode::Plain { case_sensitive } | FilterMode::Regex { case_sensitive } => {
                *case_sensitive
            }
        }
    }

    /// Get display label for the filter prompt
    pub fn prompt_label(&self) -> &'static str {
        match self {
            FilterMode::Plain {
                case_sensitive: false,
            } => "Filter",
            FilterMode::Plain {
                case_sensitive: true,
            } => "Filter [Aa]",
            FilterMode::Regex {
                case_sensitive: false,
            } => "Regex",
            FilterMode::Regex {
                case_sensitive: true,
            } => "Regex [Aa]",
        }
    }
}

/// A filter history entry that stores both the pattern and the mode
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FilterHistoryEntry {
    pub pattern: String,
    pub mode: FilterMode,
}

impl FilterHistoryEntry {
    /// Create a new history entry
    pub fn new(pattern: String, mode: FilterMode) -> Self {
        Self { pattern, mode }
    }

    /// Check if this entry matches another (same pattern and mode)
    pub fn matches(&self, other: &FilterHistoryEntry) -> bool {
        self.pattern == other.pattern && self.mode == other.mode
    }
}

#[cfg(test)]
mod filter_history_entry_tests {
    use super::*;

    #[test]
    fn test_new_entry() {
        let entry = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        assert_eq!(entry.pattern, "error");
        assert!(!entry.mode.is_regex());
    }

    #[test]
    fn test_entry_with_regex_mode() {
        let entry = FilterHistoryEntry::new("err.*".to_string(), FilterMode::regex());
        assert_eq!(entry.pattern, "err.*");
        assert!(entry.mode.is_regex());
    }

    #[test]
    fn test_entry_preserves_case_sensitivity() {
        let mode = FilterMode::Regex {
            case_sensitive: true,
        };
        let entry = FilterHistoryEntry::new("Error".to_string(), mode);
        assert!(entry.mode.is_case_sensitive());
    }

    #[test]
    fn test_matches_same_pattern_and_mode() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        assert!(entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_pattern() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("warn".to_string(), FilterMode::plain());
        assert!(!entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_mode() {
        let entry1 = FilterHistoryEntry::new("error".to_string(), FilterMode::plain());
        let entry2 = FilterHistoryEntry::new("error".to_string(), FilterMode::regex());
        assert!(!entry1.matches(&entry2));
    }

    #[test]
    fn test_matches_different_case_sensitivity() {
        let entry1 = FilterHistoryEntry::new(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: false,
            },
        );
        let entry2 = FilterHistoryEntry::new(
            "error".to_string(),
            FilterMode::Plain {
                case_sensitive: true,
            },
        );
        assert!(!entry1.matches(&entry2));
    }
}

#[cfg(test)]
mod filter_mode_tests {
    use super::*;

    #[test]
    fn test_default_is_plain_case_insensitive() {
        let mode = FilterMode::default();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_plain_constructor() {
        let mode = FilterMode::plain();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_regex_constructor() {
        let mode = FilterMode::regex();
        assert!(mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_plain_to_regex() {
        let mut mode = FilterMode::plain();
        mode.toggle_mode();
        assert!(mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_regex_to_plain() {
        let mut mode = FilterMode::regex();
        mode.toggle_mode();
        assert!(!mode.is_regex());
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_mode_preserves_case_sensitivity() {
        let mut mode = FilterMode::Plain {
            case_sensitive: true,
        };
        mode.toggle_mode();
        assert!(mode.is_regex());
        assert!(mode.is_case_sensitive());

        mode.toggle_mode();
        assert!(!mode.is_regex());
        assert!(mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_case_sensitivity_plain() {
        let mut mode = FilterMode::plain();
        assert!(!mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_toggle_case_sensitivity_regex() {
        let mut mode = FilterMode::regex();
        assert!(!mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(mode.is_case_sensitive());

        mode.toggle_case_sensitivity();
        assert!(!mode.is_case_sensitive());
    }

    #[test]
    fn test_prompt_label_plain() {
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };
        assert_eq!(mode.prompt_label(), "Filter");

        let mode = FilterMode::Plain {
            case_sensitive: true,
        };
        assert_eq!(mode.prompt_label(), "Filter [Aa]");
    }

    #[test]
    fn test_prompt_label_regex() {
        let mode = FilterMode::Regex {
            case_sensitive: false,
        };
        assert_eq!(mode.prompt_label(), "Regex");

        let mode = FilterMode::Regex {
            case_sensitive: true,
        };
        assert_eq!(mode.prompt_label(), "Regex [Aa]");
    }

    #[test]
    fn test_filter_mode_clone() {
        let mode1 = FilterMode::Regex {
            case_sensitive: true,
        };
        let mode2 = mode1;
        assert_eq!(mode1, mode2);
    }
}

// ============================================================================
// Index-accelerated filter integration tests
// ============================================================================
//
// These tests verify the full pipeline: create log file -> build index ->
// run query through indexed path -> compare with non-indexed path.

#[cfg(test)]
mod index_filter_integration_tests {
    use super::cancel::CancelToken;
    use super::engine::FilterProgress;
    use super::query::{self, QueryFilter};
    use super::streaming_filter;
    use super::Filter;
    use lazytail::index::builder::IndexBuilder;
    use lazytail::index::reader::IndexReader;
    use std::io::Write;
    use std::sync::Arc;
    use tempfile::tempdir;

    /// Write lines to a temp file and return its path.
    fn write_log_file(dir: &std::path::Path, name: &str, lines: &[&str]) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        for line in lines {
            writeln!(f, "{}", line).unwrap();
        }
        f.flush().unwrap();
        path
    }

    /// Build an index for a log file, returning the index directory.
    fn build_index(log_path: &std::path::Path) -> std::path::PathBuf {
        let idx_dir = log_path.with_extension("idx");
        IndexBuilder::new()
            .with_checkpoint_interval(10)
            .build(log_path, &idx_dir)
            .unwrap();
        idx_dir
    }

    /// Collect all matching line indices from a FilterProgress receiver.
    fn collect_matches(rx: std::sync::mpsc::Receiver<FilterProgress>) -> Vec<usize> {
        let mut all = Vec::new();
        for progress in rx {
            match progress {
                FilterProgress::PartialResults { matches, .. } => all.extend(matches),
                FilterProgress::Complete { matches, .. } => {
                    all.extend(matches);
                    break;
                }
                FilterProgress::Error(e) => panic!("Filter error: {}", e),
                FilterProgress::Processing(_) => {}
            }
        }
        all
    }

    #[test]
    fn test_indexed_json_query_matches_unindexed() {
        let dir = tempdir().unwrap();
        let lines = &[
            r#"{"level":"error","msg":"connection refused","service":"api"}"#,
            r#"{"level":"info","msg":"request completed","service":"api"}"#,
            "2024-01-01 ERROR plain text error line",
            r#"{"level":"error","msg":"timeout","service":"worker"}"#,
            r#"{"level":"debug","msg":"loading config","service":"api"}"#,
            "INFO: another plain text line",
            r#"{"level":"warn","msg":"slow query","service":"db"}"#,
            r#"{"level":"error","msg":"disk full","service":"storage"}"#,
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        // Parse query
        let filter_query = query::parse_query("json | level == \"error\"").unwrap();

        // Build bitmap from index
        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        // Run indexed filter
        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        // Run non-indexed filter
        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        // Results must be identical
        assert_eq!(indexed_results, regular_results);
        // And correct: lines 0, 3, 7 are JSON with level=error
        assert_eq!(indexed_results, vec![0, 3, 7]);

        // Cleanup
        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_indexed_logfmt_query_matches_unindexed() {
        let dir = tempdir().unwrap();
        let lines = &[
            "level=error msg=\"connection refused\" service=api",
            "level=info msg=\"request completed\" service=api",
            "2024-01-01 ERROR plain text error line",
            "level=error msg=\"timeout\" service=worker",
            "level=debug msg=\"loading config\" service=api",
            r#"{"level":"error","msg":"json error"}"#,
            "level=warn msg=\"slow query\" service=db",
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        let filter_query = query::parse_query("logfmt | level == error").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        assert_eq!(indexed_results, regular_results);
        // Lines 0, 3 are logfmt with level=error
        assert_eq!(indexed_results, vec![0, 3]);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_indexed_json_no_level_filter() {
        // Query with non-level field — index only filters by format (JSON), not severity
        let dir = tempdir().unwrap();
        let lines = &[
            r#"{"level":"error","msg":"fail","service":"api"}"#,
            "plain text line",
            r#"{"level":"info","msg":"ok","service":"api"}"#,
            r#"{"level":"warn","msg":"slow","service":"db"}"#,
            "another plain text line",
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        let filter_query = query::parse_query("json | service == \"api\"").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        assert_eq!(indexed_results, regular_results);
        // Lines 0, 2 have service=api (line 3 is service=db)
        assert_eq!(indexed_results, vec![0, 2]);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_indexed_json_only_no_filters() {
        // "json" with no filters — should match all valid JSON lines
        let dir = tempdir().unwrap();
        let lines = &[
            r#"{"level":"error","msg":"fail"}"#,
            "plain text line",
            r#"{"level":"info","msg":"ok"}"#,
            "2024 ERROR plain",
            r#"{"msg":"no level"}"#,
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        let filter_query = query::parse_query("json").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        assert_eq!(indexed_results, regular_results);
        // Lines 0, 2, 4 are valid JSON
        assert_eq!(indexed_results, vec![0, 2, 4]);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_indexed_filter_with_multiple_conditions() {
        let dir = tempdir().unwrap();
        let lines = &[
            r#"{"level":"error","msg":"timeout","service":"api"}"#,
            r#"{"level":"error","msg":"disk full","service":"storage"}"#,
            r#"{"level":"info","msg":"ok","service":"api"}"#,
            r#"{"level":"error","msg":"connection refused","service":"api"}"#,
            "plain text ERROR line",
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        let filter_query =
            query::parse_query("json | level == \"error\" | service == \"api\"").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        assert_eq!(indexed_results, regular_results);
        // Lines 0, 3 are JSON with level=error AND service=api
        assert_eq!(indexed_results, vec![0, 3]);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_no_index_falls_back_correctly() {
        // When no index exists, IndexReader::open returns None
        let dir = tempdir().unwrap();
        let log_path = write_log_file(
            dir.path(),
            "no_index.log",
            &[
                r#"{"level":"error","msg":"fail"}"#,
                r#"{"level":"info","msg":"ok"}"#,
            ],
        );

        // No index built — IndexReader::open should return None
        assert!(IndexReader::open(&log_path).is_none());

        // Filter still works through non-indexed path
        let filter_query = query::parse_query("json | level == \"error\"").unwrap();
        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let results = collect_matches(rx);
        assert_eq!(results, vec![0]);
    }

    #[test]
    fn test_index_mask_does_not_drop_matches() {
        // Critical correctness test: the indexed path must NEVER produce
        // fewer matches than the non-indexed path. This could happen if
        // the flag detection disagrees with the JSON parser.
        let dir = tempdir().unwrap();
        let lines = &[
            // Standard JSON with explicit level
            r#"{"level":"error","msg":"standard"}"#,
            // JSON with ERROR in message (not level field) — severity detection
            // in flags may pick up ERROR keyword, but level field is "info"
            r#"{"level":"info","msg":"ERROR in message text"}"#,
            // JSON with mixed case level
            r#"{"level":"Error","msg":"mixed case"}"#,
            // JSON with no level field at all
            r#"{"msg":"no level field but has ERROR keyword"}"#,
            // Whitespace-prefixed JSON
            r#"  {"level":"error","msg":"indented"}"#,
        ];
        let log_path = write_log_file(dir.path(), "test.log", lines);
        let idx_dir = build_index(&log_path);

        let filter_query = query::parse_query("json | level == \"error\"").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        // Indexed results must be a superset of (or equal to) regular results.
        // The index may let through false positives (which filter.matches removes),
        // but must never miss a true match.
        assert_eq!(indexed_results, regular_results);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_index_shorter_than_file_appended_lines() {
        // Simulate a file that grew after the index was built
        let dir = tempdir().unwrap();
        let initial_lines = &[
            r#"{"level":"error","msg":"first"}"#,
            r#"{"level":"info","msg":"second"}"#,
        ];
        let log_path = write_log_file(dir.path(), "test.log", initial_lines);
        let idx_dir = build_index(&log_path);

        // Append more lines to the file after index was built
        {
            let mut f = std::fs::OpenOptions::new()
                .append(true)
                .open(&log_path)
                .unwrap();
            writeln!(f, r#"{{"level":"error","msg":"appended"}}"#).unwrap();
            writeln!(f, "plain text line").unwrap();
            f.flush().unwrap();
        }

        let filter_query = query::parse_query("json | level == \"error\"").unwrap();

        let (mask, want) = filter_query.index_mask().unwrap();
        let reader = IndexReader::open(&log_path).unwrap();
        // Index only covers 2 lines, file has 4
        assert_eq!(reader.len(), 2);
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());
        assert_eq!(bitmap.len(), 2);

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx_indexed = streaming_filter::run_streaming_filter_indexed(
            log_path.clone(),
            filter.clone(),
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let indexed_results = collect_matches(rx_indexed);

        let rx_regular =
            streaming_filter::run_streaming_filter(log_path, filter, CancelToken::new()).unwrap();
        let regular_results = collect_matches(rx_regular);

        // Indexed filter falls back to checking lines past bitmap
        assert_eq!(indexed_results, regular_results);
        // Lines 0 and 2 match (line 2 was appended after index)
        assert_eq!(indexed_results, vec![0, 2]);

        let _ = std::fs::remove_dir_all(&idx_dir);
    }

    #[test]
    fn test_empty_file_with_index() {
        let dir = tempdir().unwrap();
        let log_path = dir.path().join("empty.log");
        std::fs::write(&log_path, "").unwrap();

        // Build index on empty file
        let idx_dir = log_path.with_extension("idx");
        IndexBuilder::new().build(&log_path, &idx_dir).unwrap();

        let reader = IndexReader::open(&log_path).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());

        let filter_query = query::parse_query("json | level == \"error\"").unwrap();
        let (mask, want) = filter_query.index_mask().unwrap();
        let bitmap = reader.candidate_bitmap(mask, want, reader.len());
        assert!(bitmap.is_empty());

        let query_filter = QueryFilter::new(filter_query).unwrap();
        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        let rx = streaming_filter::run_streaming_filter_indexed(
            log_path,
            filter,
            bitmap,
            CancelToken::new(),
        )
        .unwrap();
        let results = collect_matches(rx);
        assert!(results.is_empty());

        let _ = std::fs::remove_dir_all(&idx_dir);
    }
}
