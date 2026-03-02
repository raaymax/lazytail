use crate::app::FilterState;
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
    ///
    /// Returns `Err` with a user-facing message if the filter could not be started
    /// (invalid regex, bad query syntax, file I/O failure, etc.).
    pub fn trigger(
        source: &mut LogSource,
        pattern: String,
        mode: FilterMode,
        range: Option<(usize, usize)>,
    ) -> Result<(), String> {
        // Cancel any previous filter operation
        if let Some(ref cancel) = source.filter.cancel_token {
            cancel.cancel();
        }

        // Query mode: user explicitly selected via Tab cycling
        if mode.is_query() {
            let mut filter_query =
                query::parse_query(&pattern).map_err(|e| format!("query parse error: {}", e))?;

            // Extract aggregation clause before building the filter
            if let Some(agg) = filter_query.aggregate.take() {
                source.filter.pending_aggregation = Some((agg, filter_query.parser.clone()));
            } else {
                source.filter.pending_aggregation = None;
            }

            let query_filter = query::QueryFilter::new(filter_query.clone())
                .map_err(|e| format!("query filter error: {}", e))?;
            let filter: Arc<dyn Filter> = Arc::new(query_filter);

            Self::execute(source, filter, Some(&filter_query), range)?;
            return Ok(());
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

                let rx = SearchEngine::search_file_fast(
                    path,
                    pattern.as_bytes(),
                    case_sensitive,
                    cancel,
                )
                .map_err(|e| format!("filter I/O error: {}", e))?;
                source.filter.receiver = Some(rx);
                return Ok(());
            }
        }

        // Build the appropriate filter
        let filter: Arc<dyn Filter> = if is_regex {
            let f = RegexFilter::new(&pattern, case_sensitive)
                .map_err(|e| format!("invalid regex: {}", e))?;
            Arc::new(f)
        } else {
            Arc::new(StringFilter::new(&pattern, case_sensitive))
        };

        Self::execute(source, filter, None, range)?;
        Ok(())
    }

    /// Set LogSource flags and delegate to the appropriate SearchEngine method.
    fn execute(
        source: &mut LogSource,
        filter: Arc<dyn Filter>,
        query: Option<&query::FilterQuery>,
        range: Option<(usize, usize)>,
    ) -> Result<(), String> {
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
            SearchEngine::search_file(
                path,
                filter,
                query,
                source.index_reader.as_ref(),
                range,
                cancel,
            )
            .map_err(|e| format!("filter I/O error: {}", e))?
        } else {
            SearchEngine::search_reader(source.reader.clone(), filter, range, cancel)
        };

        source.filter.receiver = Some(receiver);
        Ok(())
    }

    /// Cancel any in-progress filter on a source.
    pub fn cancel(source: &mut LogSource) {
        if let Some(ref cancel) = source.filter.cancel_token {
            cancel.cancel();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::engine::FilterProgress;
    use crate::log_source::LogSource;
    use crate::test_utils::MockLogReader;
    use std::sync::Mutex;

    fn make_source(lines: Vec<&str>) -> LogSource {
        let lines: Vec<String> = lines.into_iter().map(String::from).collect();
        let total = lines.len();
        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        LogSource::new("test".into(), reader).with_lines(total)
    }

    /// Drain all filter progress messages and collect matching line indices.
    fn collect_matches(source: &mut LogSource) -> Vec<usize> {
        let rx = source.filter.receiver.take().expect("no receiver");
        let mut all = Vec::new();
        while let Ok(msg) = rx.recv() {
            match msg {
                FilterProgress::PartialResults { matches, .. } => all.extend(matches),
                FilterProgress::Complete { matches, .. } => {
                    all.extend(matches);
                    break;
                }
                FilterProgress::Error(e) => panic!("unexpected filter error: {}", e),
                _ => {}
            }
        }
        all.sort_unstable();
        all
    }

    #[test]
    fn plain_text_filter_finds_matches() {
        let mut source = make_source(vec!["ERROR: fail", "INFO: ok", "ERROR: boom"]);
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, "error".into(), mode, None).unwrap();

        assert!(matches!(
            source.filter.state,
            FilterState::Processing { .. }
        ));
        assert!(source.filter.receiver.is_some());

        let matches = collect_matches(&mut source);
        assert_eq!(matches, vec![0, 2]);
    }

    #[test]
    fn regex_filter_finds_matches() {
        let mut source = make_source(vec!["line 42", "line 7", "line 100"]);
        let mode = FilterMode::Regex {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, r"line \d{2,}".into(), mode, None).unwrap();

        let matches = collect_matches(&mut source);
        assert_eq!(matches, vec![0, 2]);
    }

    #[test]
    fn invalid_regex_returns_error() {
        let mut source = make_source(vec!["test"]);
        let mode = FilterMode::Regex {
            case_sensitive: false,
        };

        let result = FilterOrchestrator::trigger(&mut source, "[invalid".into(), mode, None);

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.contains("invalid regex"), "got: {}", err);
    }

    #[test]
    fn invalid_query_returns_error() {
        let mut source = make_source(vec!["test"]);
        let mode = FilterMode::Query {};

        let result =
            FilterOrchestrator::trigger(&mut source, "not a valid query |||".into(), mode, None);

        assert!(result.is_err());
    }

    #[test]
    fn cancel_previous_filter_on_retrigger() {
        let mut source = make_source(vec!["a", "b", "c"]);
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, "a".into(), mode, None).unwrap();
        let first_cancel = source.filter.cancel_token.clone().unwrap();

        FilterOrchestrator::trigger(&mut source, "b".into(), mode, None).unwrap();
        assert!(first_cancel.is_cancelled());
    }

    #[test]
    fn incremental_filter_sets_is_incremental() {
        let mut source = make_source(vec!["a", "b", "c", "d", "e"]);
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, "a".into(), mode, Some((3, 5))).unwrap();

        assert!(source.filter.is_incremental);
        assert!(matches!(
            source.filter.state,
            FilterState::Processing { .. }
        ));
    }

    #[test]
    fn full_filter_clears_needs_clear() {
        let mut source = make_source(vec!["a", "b"]);
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, "a".into(), mode, None).unwrap();

        assert!(source.filter.needs_clear);
        assert!(!source.filter.is_incremental);
    }

    #[test]
    fn cancel_stops_filter() {
        let mut source = make_source(vec!["test"]);
        let mode = FilterMode::Plain {
            case_sensitive: false,
        };

        FilterOrchestrator::trigger(&mut source, "test".into(), mode, None).unwrap();
        let token = source.filter.cancel_token.clone().unwrap();

        FilterOrchestrator::cancel(&mut source);
        assert!(token.is_cancelled());
    }
}
