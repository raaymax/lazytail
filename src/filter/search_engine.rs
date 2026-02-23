//! Unified search dispatch — picks the fastest filter execution path.
//!
//! Both TUI (`FilterOrchestrator`) and MCP converge here, eliminating
//! duplicated index-acceleration logic. All functions are stateless and
//! return `Result<Receiver<FilterProgress>>`.

use super::cancel::CancelToken;
use super::engine::{FilterEngine, FilterProgress};
use super::{streaming_filter, Filter};
use crate::index::column::ColumnReader;
use crate::index::reader::IndexReader;
use crate::reader::LogReader;
use crate::source::index_dir_for_log;
use anyhow::Result;
use std::path::Path;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};

use crate::filter::query::FilterQuery;

/// Progress interval for reader-based filter operations (report every N lines)
const FILTER_PROGRESS_INTERVAL: usize = 1000;

/// Stateless search dispatch — picks the fastest execution path based on
/// filter type, available index, and range.
pub struct SearchEngine;

impl SearchEngine {
    /// File-backed search. Picks the fastest execution path based on
    /// filter type, available index, and range.
    ///
    /// - `filter`: Pre-built filter (StringFilter, RegexFilter, or QueryFilter)
    /// - `query`: Optional FilterQuery AST — needed for index_mask() acceleration
    /// - `index`: Optional IndexReader — needed for bitmap pre-filtering
    /// - `range`: Optional (start, end) for incremental filtering
    pub fn search_file(
        path: &Path,
        filter: Arc<dyn Filter>,
        query: Option<&FilterQuery>,
        index: Option<&IndexReader>,
        range: Option<(usize, usize)>,
        cancel: CancelToken,
    ) -> Result<Receiver<FilterProgress>> {
        // Try index-accelerated path: query + index available
        let bitmap = query.and_then(|q| q.index_mask()).and_then(|(mask, want)| {
            let reader = index?;
            if reader.is_empty() {
                return None;
            }
            Some(reader.candidate_bitmap(mask, want, reader.len()))
        });

        if let Some((start, end)) = range {
            // Incremental filtering (new lines only)
            let start_byte_offset = {
                let idx_dir = index_dir_for_log(path);
                ColumnReader::<u64>::open(idx_dir.join("offsets"), start + 1)
                    .ok()
                    .and_then(|r| r.get(start))
            };

            streaming_filter::run_streaming_filter_range(
                path.to_path_buf(),
                filter,
                start,
                end,
                start_byte_offset,
                bitmap,
                cancel,
            )
        } else if let Some(bitmap) = bitmap {
            // Index-accelerated full filter
            streaming_filter::run_streaming_filter_indexed(
                path.to_path_buf(),
                filter,
                bitmap,
                cancel,
            )
        } else {
            // Generic full filter
            streaming_filter::run_streaming_filter(path.to_path_buf(), filter, cancel)
        }
    }

    /// Fast path for plain text full-file search (SIMD).
    /// Bypasses Filter trait entirely for maximum performance.
    pub fn search_file_fast(
        path: &Path,
        pattern: &[u8],
        case_sensitive: bool,
        cancel: CancelToken,
    ) -> Result<Receiver<FilterProgress>> {
        streaming_filter::run_streaming_filter_fast(
            path.to_path_buf(),
            pattern,
            case_sensitive,
            cancel,
        )
    }

    /// Stdin/pipe path: uses FilterEngine with shared reader.
    pub fn search_reader(
        reader: Arc<Mutex<dyn LogReader + Send>>,
        filter: Arc<dyn Filter>,
        range: Option<(usize, usize)>,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress> {
        if let Some((start, end)) = range {
            FilterEngine::run_filter_range(
                reader,
                filter,
                FILTER_PROGRESS_INTERVAL,
                start,
                end,
                cancel,
            )
        } else {
            FilterEngine::run_filter(reader, filter, FILTER_PROGRESS_INTERVAL, cancel)
        }
    }
}
