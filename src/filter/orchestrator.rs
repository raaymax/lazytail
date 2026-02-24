use crate::app::{App, FilterState};
use crate::filter::cancel::CancelToken;
use crate::filter::search_engine::SearchEngine;
use crate::filter::{
    query, regex_filter::RegexFilter, string_filter::StringFilter, Filter, FilterMode,
};
use crate::log_source::LogSource;
use std::sync::Arc;

/// Unified filter orchestration — consolidates all filter trigger paths
/// (full/incremental, file/stdin, plain/regex/query) into one entry point.
///
/// Owns LogSource-specific concerns (cancel, state flags, receiver storage)
/// and delegates actual filter execution to `SearchEngine`.
pub struct FilterOrchestrator;

impl FilterOrchestrator {
    /// Trigger a filter on a source.
    ///
    /// Handles query detection, filter construction, and delegates execution
    /// to `SearchEngine`. The `range` parameter is `Some((start, end))` for
    /// incremental filtering.
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

        // Query mode: user explicitly selected via Tab cycling
        if mode.is_query() {
            let mut filter_query = match query::parse_query(&pattern) {
                Ok(q) => q,
                Err(_) => return,
            };

            // Extract aggregation clause before building the filter
            if let Some(agg) = filter_query.aggregate.take() {
                source.filter.pending_aggregation = Some((agg, filter_query.parser.clone()));
            } else {
                source.filter.pending_aggregation = None;
            }

            let query_filter = match query::QueryFilter::new(filter_query.clone()) {
                Ok(f) => f,
                Err(_) => return,
            };
            let filter: Arc<dyn Filter> = Arc::new(query_filter);

            Self::execute(source, filter, Some(&filter_query), range);
            return;
        }

        // Non-query filters clear any pending aggregation
        source.filter.pending_aggregation = None;

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

                if let Ok(rx) =
                    SearchEngine::search_file_fast(path, pattern.as_bytes(), case_sensitive, cancel)
                {
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

        Self::execute(source, filter, None, range);
    }

    /// Set LogSource flags and delegate to the appropriate SearchEngine method.
    fn execute(
        source: &mut LogSource,
        filter: Arc<dyn Filter>,
        query: Option<&query::FilterQuery>,
        range: Option<(usize, usize)>,
    ) {
        let cancel = CancelToken::new();
        source.filter.cancel_token = Some(cancel.clone());

        if range.is_some() {
            source.filter.state = FilterState::Processing { lines_processed: 0 };
            source.filter.is_incremental = true;
        } else {
            source.filter.needs_clear = true;
            source.filter.state = FilterState::Processing { lines_processed: 0 };
            source.filter.is_incremental = false;
        }

        let receiver = if let Some(path) = &source.source_path {
            match SearchEngine::search_file(
                path,
                filter,
                query,
                source.index_reader.as_ref(),
                range,
                cancel,
            ) {
                Ok(rx) => rx,
                Err(_) => return,
            }
        } else {
            SearchEngine::search_reader(source.reader.clone(), filter, range, cancel)
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
