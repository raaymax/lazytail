use crate::app::{App, FilterState};
use crate::filter::cancel::CancelToken;
use crate::filter::engine::FilterEngine;
use crate::filter::{
    query, regex_filter::RegexFilter, streaming_filter, string_filter::StringFilter, Filter,
    FilterMode,
};
use crate::tab::TabState;
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
            Self::trigger_query(tab, &pattern, range);
            return;
        }

        let case_sensitive = mode.is_case_sensitive();
        let is_regex = mode.is_regex();

        // Create new cancel token for this operation
        let cancel = CancelToken::new();
        tab.filter.cancel_token = Some(cancel.clone());

        let receiver = if let Some((start, end)) = range {
            // Incremental filtering (new lines only)
            tab.filter.state = FilterState::Processing { lines_processed: 0 };
            tab.filter.is_incremental = true;

            let filter: Arc<dyn Filter> = if is_regex {
                match RegexFilter::new(&pattern, case_sensitive) {
                    Ok(f) => Arc::new(f),
                    Err(_) => return,
                }
            } else {
                Arc::new(StringFilter::new(&pattern, case_sensitive))
            };

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
                if is_regex {
                    let filter: Arc<dyn Filter> = match RegexFilter::new(&pattern, case_sensitive) {
                        Ok(f) => Arc::new(f),
                        Err(_) => return,
                    };
                    match streaming_filter::run_streaming_filter(path.clone(), filter, cancel) {
                        Ok(rx) => rx,
                        Err(_) => return,
                    }
                } else {
                    // Plain text: use FAST byte-level filter with SIMD
                    match streaming_filter::run_streaming_filter_fast(
                        path.clone(),
                        pattern.as_bytes(),
                        case_sensitive,
                        cancel,
                    ) {
                        Ok(rx) => rx,
                        Err(_) => return,
                    }
                }
            } else {
                // Stdin: use generic filter
                let filter: Arc<dyn Filter> = if is_regex {
                    match RegexFilter::new(&pattern, case_sensitive) {
                        Ok(f) => Arc::new(f),
                        Err(_) => return,
                    }
                } else {
                    Arc::new(StringFilter::new(&pattern, case_sensitive))
                };
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

    /// Trigger a query-based filter (json | ... or logfmt | ...).
    fn trigger_query(tab: &mut TabState, pattern: &str, range: Option<(usize, usize)>) {
        // Parse the query
        let filter_query = match query::parse_query(pattern) {
            Ok(q) => q,
            Err(_) => return,
        };

        // Create QueryFilter
        let query_filter = match query::QueryFilter::new(filter_query) {
            Ok(f) => f,
            Err(_) => return,
        };

        let filter: Arc<dyn Filter> = Arc::new(query_filter);

        // Create new cancel token for this operation
        let cancel = CancelToken::new();
        tab.filter.cancel_token = Some(cancel.clone());

        let receiver = if let Some((start, end)) = range {
            // Incremental
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
            // Full
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
