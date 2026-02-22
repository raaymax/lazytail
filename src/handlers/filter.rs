use crate::app::AppEvent;
use crate::filter::engine::FilterProgress;

/// Handle filter progress messages and return corresponding app events
/// Does not mutate app state directly - returns events to be processed
pub fn handle_filter_progress(progress: FilterProgress, is_incremental: bool) -> Vec<AppEvent> {
    match progress {
        FilterProgress::Processing(lines_processed) => {
            vec![AppEvent::FilterProgress(lines_processed)]
        }
        FilterProgress::PartialResults {
            matches,
            lines_processed,
        } => {
            vec![AppEvent::FilterPartialResults {
                matches,
                lines_processed,
            }]
        }
        FilterProgress::Complete {
            matches: matching_indices,
            ..
        } => {
            vec![AppEvent::FilterComplete {
                indices: matching_indices,
                incremental: is_incremental,
            }]
        }
        FilterProgress::Error(err) => {
            vec![AppEvent::FilterError(err)]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_filter_processing() {
        let progress = FilterProgress::Processing(100);
        let events = handle_filter_progress(progress, false);
        assert_eq!(events, vec![AppEvent::FilterProgress(100)]);
    }

    #[test]
    fn test_filter_complete_full() {
        let progress = FilterProgress::Complete {
            matches: vec![1, 5, 10],
            lines_processed: 100,
        };
        let events = handle_filter_progress(progress, false);
        assert_eq!(
            events,
            vec![AppEvent::FilterComplete {
                indices: vec![1, 5, 10],
                incremental: false
            }]
        );
    }

    #[test]
    fn test_filter_complete_incremental() {
        let progress = FilterProgress::Complete {
            matches: vec![100, 105],
            lines_processed: 200,
        };
        let events = handle_filter_progress(progress, true);
        assert_eq!(
            events,
            vec![AppEvent::FilterComplete {
                indices: vec![100, 105],
                incremental: true
            }]
        );
    }

    #[test]
    fn test_filter_error() {
        let progress = FilterProgress::Error("Test error".to_string());
        let events = handle_filter_progress(progress, false);
        assert_eq!(
            events,
            vec![AppEvent::FilterError("Test error".to_string())]
        );
    }
}
