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

    // Access active tab state
    let tab = app.active_tab();

    // If in filtered mode and file grew, trigger incremental filter
    if tab.mode == ViewMode::Filtered && new_total > old_total {
        if let Some(ref pattern) = tab.filter.pattern {
            let start_line = tab.filter.last_filtered_line;
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
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_temp_log_file(lines: &[&str]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_file_truncated() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        let events = process_file_modification(50, 100, &app);
        assert!(events.contains(&AppEvent::FileTruncated { new_total: 50 }));
    }

    #[test]
    fn test_file_grew_normal_mode() {
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

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
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let mut app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        app.apply_filter(vec![0, 5, 10], "test".to_string());
        app.active_tab_mut().filter.last_filtered_line = 100;

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
        let lines: Vec<&str> = (0..100).map(|_| "line").collect();
        let temp_file = create_temp_log_file(&lines);
        let app = App::new(vec![temp_file.path().to_path_buf()], false).unwrap();

        let events = process_file_modification(100, 100, &app);
        assert!(events.contains(&AppEvent::FileModified {
            new_total: 100,
            old_total: 100
        }));
    }
}
