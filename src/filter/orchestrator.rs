use crate::app::{App, FilterState};
use crate::filter::cancel::CancelToken;
use crate::filter::engine::{FilterEngine, FilterProgress};
use crate::filter::{
    query, regex_filter::RegexFilter, streaming_filter, string_filter::StringFilter, Filter,
    FilterMode,
};
use crate::tab::TabState;
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
    /// Trigger a filter on a tab.
    ///
    /// Handles query detection, streaming vs generic filter, full vs range (incremental).
    /// The `range` parameter is `Some((start, end))` for incremental filtering.
    pub fn trigger(
        tab: &mut TabState,
        pattern: String,
        mode: FilterMode,
        range: Option<(usize, usize)>,
    ) {
        // Cancel any previous filter operation
        if let Some(ref cancel) = tab.filter.cancel_token {
            cancel.cancel();
        }

        // Check for query syntax (json | ... or logfmt | ...)
        if query::is_query_syntax(&pattern) {
            let filter_query = match query::parse_query(&pattern) {
                Ok(q) => q,
                Err(_) => return,
            };

            // Try index-accelerated path: full filter + file source + index available
            if range.is_none() {
                if let Some(path) = &tab.source_path {
                    if let Some((mask, want)) = filter_query.index_mask() {
                        if let Some(ref index_reader) = tab.index_reader {
                            let bitmap =
                                index_reader.candidate_bitmap(mask, want, index_reader.len());

                            let query_filter = match query::QueryFilter::new(filter_query) {
                                Ok(f) => f,
                                Err(_) => return,
                            };
                            let filter: Arc<dyn Filter> = Arc::new(query_filter);

                            let cancel = CancelToken::new();
                            tab.filter.cancel_token = Some(cancel.clone());
                            tab.filter.needs_clear = true;
                            tab.filter.state = FilterState::Processing { lines_processed: 0 };
                            tab.filter.is_incremental = false;

                            if let Ok(rx) = streaming_filter::run_streaming_filter_indexed(
                                path.clone(),
                                filter,
                                bitmap,
                                cancel,
                            ) {
                                tab.filter.receiver = Some(rx);
                            }
                            return;
                        }
                    }
                }
            }

            // Fallback: no index or incremental — use standard dispatch
            let query_filter = match query::QueryFilter::new(filter_query) {
                Ok(f) => f,
                Err(_) => return,
            };
            let filter: Arc<dyn Filter> = Arc::new(query_filter);
            Self::dispatch(tab, filter, range);
            return;
        }

        let case_sensitive = mode.is_case_sensitive();
        let is_regex = mode.is_regex();

        // For full file + plain text, use the FAST byte-level SIMD path
        if range.is_none() && !is_regex {
            if let Some(path) = &tab.source_path {
                let cancel = CancelToken::new();
                tab.filter.cancel_token = Some(cancel.clone());
                tab.filter.needs_clear = true;
                tab.filter.state = FilterState::Processing { lines_processed: 0 };
                tab.filter.is_incremental = false;

                if let Ok(rx) = streaming_filter::run_streaming_filter_fast(
                    path.clone(),
                    pattern.as_bytes(),
                    case_sensitive,
                    cancel,
                ) {
                    tab.filter.receiver = Some(rx);
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

        Self::dispatch(tab, filter, range);
    }

    /// Dispatch a filter to the appropriate execution backend.
    ///
    /// Shared by all filter types (plain, regex, query). Handles:
    /// - Range (incremental) vs full filtering
    /// - File (streaming) vs stdin (generic engine)
    fn dispatch(tab: &mut TabState, filter: Arc<dyn Filter>, range: Option<(usize, usize)>) {
        let cancel = CancelToken::new();
        tab.filter.cancel_token = Some(cancel.clone());

        let receiver: Receiver<FilterProgress> = if let Some((start, end)) = range {
            // Incremental filtering (new lines only)
            tab.filter.state = FilterState::Processing { lines_processed: 0 };
            tab.filter.is_incremental = true;

            if let Some(path) = &tab.source_path {
                match streaming_filter::run_streaming_filter_range(
                    path.clone(),
                    filter,
                    start,
                    end,
                    cancel,
                ) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            } else {
                FilterEngine::run_filter_range(
                    tab.reader.clone(),
                    filter,
                    FILTER_PROGRESS_INTERVAL,
                    start,
                    end,
                    cancel,
                )
            }
        } else {
            // Full filtering
            tab.filter.needs_clear = true;
            tab.filter.state = FilterState::Processing { lines_processed: 0 };
            tab.filter.is_incremental = false;

            if let Some(path) = &tab.source_path {
                match streaming_filter::run_streaming_filter(path.clone(), filter, cancel) {
                    Ok(rx) => rx,
                    Err(_) => return,
                }
            } else {
                FilterEngine::run_filter(
                    tab.reader.clone(),
                    filter,
                    FILTER_PROGRESS_INTERVAL,
                    cancel,
                )
            }
        };

        tab.filter.receiver = Some(receiver);
    }

    /// Trigger live filter preview based on current input.
    ///
    /// Validates input, then either triggers a filter or clears if empty/invalid.
    pub fn trigger_preview(app: &mut App) {
        let pattern = app.get_input().to_string();
        let mode = app.current_filter_mode;

        if !pattern.is_empty() && app.is_regex_valid() {
            let tab = app.active_tab_mut();
            tab.filter.pattern = Some(pattern.clone());
            tab.filter.mode = mode;
            Self::trigger(tab, pattern, mode, None);
        } else {
            app.clear_filter();
            app.active_tab_mut().filter.receiver = None;
        }
    }

    /// Cancel any in-progress filter on a tab.
    pub fn cancel(tab: &mut TabState) {
        if let Some(ref cancel) = tab.filter.cancel_token {
            cancel.cancel();
        }
    }
}
