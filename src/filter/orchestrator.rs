use crate::app::{App, FilterState};
use crate::filter::cancel::CancelToken;
use crate::filter::engine::{FilterEngine, FilterProgress};
use crate::filter::{
    query, regex_filter::RegexFilter, streaming_filter, string_filter::StringFilter, Filter,
    FilterMode,
};
use crate::index::column::ColumnReader;
use crate::source::index_dir_for_log;
use crate::log_source::LogSource;
use std::sync::mpsc::Receiver;
use std::sync::Arc;

/// Progress interval for filter operations (report every N lines)
const FILTER_PROGRESS_INTERVAL: usize = 1000;

/// Unified filter orchestration — consolidates all filter trigger paths
/// (full/incremental, file/stdin, plain/regex/query) into one entry point.
///
/// Both TUI and MCP converge on the same `FilterQuery` AST, so adding a new
/// query operator or parser to `src/filter/query.rs` automatically works in
/// both paths with zero changes here.
pub struct FilterOrchestrator;

impl FilterOrchestrator {
    /// Trigger a filter on a source.
    ///
    /// Handles query detection, streaming vs generic filter, full vs range (incremental).
    /// The `range` parameter is `Some((start, end))` for incremental filtering.
    pub fn trigger(
        source: &mut LogSource,
        pattern: String,
        mode: FilterMode,
        range: Option<(usize, usize)>,
    ) {
        // Cancel any previous filter operation
        if let Some(ref cancel) = source.filter.cancel_token {
            cancel.cancel();
        }

        // Check for query syntax (json | ... or logfmt | ...)
        if query::is_query_syntax(&pattern) {
            let filter_query = match query::parse_query(&pattern) {
                Ok(q) => q,
                Err(_) => return,
            };

            // Try index-accelerated path: file source + index available
            if let Some(path) = &source.source_path {
                if let Some((mask, want)) = filter_query.index_mask() {
                    if let Some(ref index_reader) = source.index_reader {
                        let bitmap = index_reader.candidate_bitmap(mask, want, index_reader.len());

                        let query_filter = match query::QueryFilter::new(filter_query) {
                            Ok(f) => f,
                            Err(_) => return,
                        };
                        let filter: Arc<dyn Filter> = Arc::new(query_filter);

                        let cancel = CancelToken::new();
                        source.filter.cancel_token = Some(cancel.clone());

                        if let Some((start, end)) = range {
                            // Index-accelerated incremental filter
                            source.filter.state = FilterState::Processing { lines_processed: 0 };
                            source.filter.is_incremental = true;

                            let start_byte_offset = {
                                let idx_dir = index_dir_for_log(path);
                                ColumnReader::<u64>::open(idx_dir.join("offsets"), start + 1)
                                    .ok()
                                    .and_then(|r| r.get(start))
                            };

                            if let Ok(rx) = streaming_filter::run_streaming_filter_range(
                                path.clone(),
                                filter,
                                start,
                                end,
                                start_byte_offset,
                                Some(bitmap),
                                cancel,
                            ) {
                                source.filter.receiver = Some(rx);
                            }
                        } else {
                            // Index-accelerated full filter
                            source.filter.needs_clear = true;
                            source.filter.state = FilterState::Processing { lines_processed: 0 };
                            source.filter.is_incremental = false;

                            if let Ok(rx) = streaming_filter::run_streaming_filter_indexed(
                                path.clone(),
                                filter,
                                bitmap,
                                cancel,
                            ) {
                                source.filter.receiver = Some(rx);
                            }
                        }
                        return;
                    }
                }
            }

            // Fallback: no index — use standard dispatch
            let query_filter = match query::QueryFilter::new(filter_query) {
                Ok(f) => f,
                Err(_) => return,
            };
            let filter: Arc<dyn Filter> = Arc::new(query_filter);
            Self::dispatch(source, filter, range);
            return;
        }

        let case_sensitive = mode.is_case_sensitive();
        let is_regex = mode.is_regex();

        // For full file + plain text, use the FAST byte-level SIMD path
        if range.is_none() && !is_regex {
            if let Some(path) = &source.source_path {
                let cancel = CancelToken::new();
                source.filter.cancel_token = Some(cancel.clone());
                source.filter.needs_clear = true;
                source.filter.state = FilterState::Processing { lines_processed: 0 };
                source.filter.is_incremental = false;

                if let Ok(rx) = streaming_filter::run_streaming_filter_fast(
                    path.clone(),
                    pattern.as_bytes(),
                    case_sensitive,
                    cancel,
                ) {
                    source.filter.receiver = Some(rx);
                }
                return;
            }
        }

        // Build the appropriate filter
        let filter: Arc<dyn Filter> = if is_regex {
            match RegexFilter::new(&pattern, case_sensitive) {
                Ok(f) => Arc::new(f),
                Err(_) => return,
            }
        } else {
            Arc::new(StringFilter::new(&pattern, case_sensitive))
        };

        Self::dispatch(source, filter, range);
    }

    /// Dispatch a filter to the appropriate execution backend.
    ///
    /// Shared by all filter types (plain, regex, query). Handles:
    /// - Range (incremental) vs full filtering
    /// - File (streaming) vs stdin (generic engine)
    fn dispatch(
        source: &mut LogSource,
        filter: Arc<dyn Filter>,
        range: Option<(usize, usize)>,
    ) {
        let cancel = CancelToken::new();
        source.filter.cancel_token = Some(cancel.clone());

        let receiver: Receiver<FilterProgress> = if let Some((start, end)) = range {
            // Incremental filtering (new lines only)
            source.filter.state = FilterState::Processing { lines_processed: 0 };
            source.filter.is_incremental = true;

            if let Some(path) = &source.source_path {
                // Look up byte offset for start_line from columnar index
                let start_byte_offset = {
                    let idx_dir = index_dir_for_log(path);
                    ColumnReader::<u64>::open(idx_dir.join("offsets"), start + 1)
                        .ok()
                        .and_then(|r| r.get(start))
                };

                match streaming_filter::run_streaming_filter_range(
                    path.clone(),
                    filter,
                    start,
                    end,
                    start_byte_offset,
                    None,
                    cancel,
                ) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            } else {
                FilterEngine::run_filter_range(
                    source.reader.clone(),
                    filter,
                    FILTER_PROGRESS_INTERVAL,
                    start,
                    end,
                    cancel,
                )
            }
        } else {
            // Full filtering
            source.filter.needs_clear = true;
            source.filter.state = FilterState::Processing { lines_processed: 0 };
            source.filter.is_incremental = false;

            if let Some(path) = &source.source_path {
                match streaming_filter::run_streaming_filter(path.clone(), filter, cancel) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            } else {
                FilterEngine::run_filter(
                    source.reader.clone(),
                    filter,
                    FILTER_PROGRESS_INTERVAL,
                    cancel,
                )
            }
        };

        source.filter.receiver = Some(receiver);
    }

    /// Trigger live filter preview based on current input.
    ///
    /// Validates input, then either triggers a filter or clears if empty/invalid.
    pub fn trigger_preview(app: &mut App) {
        let pattern = app.get_input().to_string();
        let mode = app.current_filter_mode;

        if !pattern.is_empty() && app.is_regex_valid() {
            let tab = app.active_tab_mut();
            tab.source.filter.pattern = Some(pattern.clone());
            tab.source.filter.mode = mode;
            Self::trigger(&mut tab.source, pattern, mode, None);
        } else {
            app.clear_filter();
            app.active_tab_mut().source.filter.receiver = None;
        }
    }

    /// Cancel any in-progress filter on a source.
    pub fn cancel(source: &mut LogSource) {
        if let Some(ref cancel) = source.filter.cancel_token {
            cancel.cancel();
        }
    }
}
