use crate::app::{App, ViewMode};
use crate::event::AppEvent;

/// Process file modification after reload has occurred
/// This is called from main.rs after the file has been reloaded
pub fn process_file_modification(new_total: usize, old_total: usize, app: &App) -> Vec<AppEvent> {
    let mut events = Vec::new();

    // Detect file truncation
    if new_total < old_total {
        events.push(AppEvent::FileTruncated { new_total });
        return events;
    }

    // File grew or stayed same size
    events.push(AppEvent::FileModified {
        new_total,
        old_total,
    });

    // If in filtered mode and file grew, trigger incremental filter
    if app.mode == ViewMode::Filtered && new_total > old_total {
        if let Some(ref pattern) = app.filter_pattern {
            let start_line = app.last_filtered_line;
            if start_line < new_total {
                events.push(AppEvent::StartFilter {
                    pattern: pattern.clone(),
                    incremental: true,
                    range: Some((start_line, new_total)),
                });
            }
        }
    }

    events
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_file_truncated() {
        let app = App::new(100);
        let events = process_file_modification(50, 100, &app);
        assert!(events.contains(&AppEvent::FileTruncated { new_total: 50 }));
    }

    #[test]
    fn test_file_grew_normal_mode() {
        let app = App::new(100);
        let events = process_file_modification(150, 100, &app);
        assert!(events.contains(&AppEvent::FileModified {
            new_total: 150,
            old_total: 100
        }));
        // Should not trigger filter in normal mode
        assert!(!events
            .iter()
            .any(|e| matches!(e, AppEvent::StartFilter { .. })));
    }

    #[test]
    fn test_file_grew_filtered_mode() {
        let mut app = App::new(100);
        app.apply_filter(vec![0, 5, 10], "test".to_string());
        app.last_filtered_line = 100;

        let events = process_file_modification(150, 100, &app);

        assert!(events.contains(&AppEvent::FileModified {
            new_total: 150,
            old_total: 100
        }));

        // Should trigger incremental filter
        let has_filter = events.iter().any(|e| {
            matches!(
                e,
                AppEvent::StartFilter {
                    pattern,
                    incremental: true,
                    range: Some((100, 150))
                } if pattern == "test"
            )
        });
        assert!(has_filter);
    }

    #[test]
    fn test_file_same_size() {
        let app = App::new(100);
        let events = process_file_modification(100, 100, &app);
        assert!(events.contains(&AppEvent::FileModified {
            new_total: 100,
            old_total: 100
        }));
    }
}
