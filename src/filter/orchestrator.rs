use crate::app::{App, FilterState};
use crate::filter::cancel::CancelToken;
use crate::filter::search_engine::SearchEngine;
use crate::filter::{
    query, regex_filter::RegexFilter, string_filter::StringFilter, Filter, FilterMode,
};
use crate::tab::TabState;
use std::sync::Arc;

/// Unified filter orchestration — consolidates all filter trigger paths
/// (full/incremental, file/stdin, plain/regex/query) into one entry point.
///
/// Owns TabState-specific concerns (cancel, state flags, receiver storage)
/// and delegates actual filter execution to `SearchEngine`.
pub struct FilterOrchestrator;

impl FilterOrchestrator {
    /// Trigger a filter on a tab.
    ///
    /// Handles query detection, filter construction, and delegates execution
    /// to `SearchEngine`. The `range` parameter is `Some((start, end))` for
    /// incremental filtering.
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

            let query_filter = match query::QueryFilter::new(filter_query.clone()) {
                Ok(f) => f,
                Err(_) => return,
            };
            let filter: Arc<dyn Filter> = Arc::new(query_filter);

            Self::execute(tab, filter, Some(&filter_query), range);
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

                if let Ok(rx) =
                    SearchEngine::search_file_fast(path, pattern.as_bytes(), case_sensitive, cancel)
                {
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

        Self::execute(tab, filter, None, range);
    }

    /// Set TabState flags and delegate to the appropriate SearchEngine method.
    fn execute(
        tab: &mut TabState,
        filter: Arc<dyn Filter>,
        query: Option<&query::FilterQuery>,
        range: Option<(usize, usize)>,
    ) {
        let cancel = CancelToken::new();
        tab.filter.cancel_token = Some(cancel.clone());

        if range.is_some() {
            tab.filter.state = FilterState::Processing { lines_processed: 0 };
            tab.filter.is_incremental = true;
        } else {
            tab.filter.needs_clear = true;
            tab.filter.state = FilterState::Processing { lines_processed: 0 };
            tab.filter.is_incremental = false;
        }

        let receiver = if let Some(path) = &tab.source_path {
            match SearchEngine::search_file(
                path,
                filter,
                query,
                tab.index_reader.as_ref(),
                range,
                cancel,
            ) {
                Ok(rx) => rx,
                Err(_) => return,
            }
        } else {
            SearchEngine::search_reader(tab.reader.clone(), filter, range, cancel)
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
