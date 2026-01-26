use super::Filter;
use crate::reader::LogReader;
use anyhow::Result;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Filter progress update
#[derive(Debug, Clone)]
pub enum FilterProgress {
    /// Currently processing (lines processed so far)
    Processing(usize),
    /// Filtering complete (matching line indices)
    Complete(Vec<usize>),
    /// Error occurred
    Error(String),
}

/// Filter engine that processes filters in the background
pub struct FilterEngine;

impl FilterEngine {
    /// Run a filter on a log reader in a background thread
    /// Returns a receiver for progress updates
    pub fn run_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        progress_interval: usize,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter(reader, filter, tx.clone(), progress_interval, 0, None)
            }));

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let _ = tx.send(FilterProgress::Error(e.to_string()));
                }
                Err(_) => {
                    let _ = tx.send(FilterProgress::Error("Filter thread panicked".to_string()));
                }
            }
        });

        rx
    }

    /// Run a filter on a specific range of lines (for incremental filtering)
    /// Returns a receiver for progress updates
    pub fn run_filter_range<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        progress_interval: usize,
        start_line: usize,
        end_line: usize,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter(
                    reader,
                    filter,
                    tx.clone(),
                    progress_interval,
                    start_line,
                    Some(end_line),
                )
            }));

            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    let _ = tx.send(FilterProgress::Error(e.to_string()));
                }
                Err(_) => {
                    let _ = tx.send(FilterProgress::Error("Filter thread panicked".to_string()));
                }
            }
        });

        rx
    }

    /// Internal filter processing
    fn process_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        progress_interval: usize,
        start_line: usize,
        end_line: Option<usize>,
    ) -> Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        // Hold lock for entire batch to avoid lock/unlock overhead per line
        let mut reader_guard = reader
            .lock()
            .expect("Reader lock poisoned - thread panicked");

        let total_lines = reader_guard.total_lines();
        let end = end_line.unwrap_or(total_lines);
        let mut matching_indices = Vec::new();

        for line_idx in start_line..end {
            // Get the line
            let line = match reader_guard.get_line(line_idx)? {
                Some(line) => line,
                None => continue,
            };

            // Check if it matches
            if filter.matches(&line) {
                matching_indices.push(line_idx);
            }

            // Send progress update periodically
            if line_idx % progress_interval == 0 {
                tx.send(FilterProgress::Processing(line_idx))?;
            }
        }

        // Release lock before sending final results
        drop(reader_guard);

        // Send final results
        tx.send(FilterProgress::Complete(matching_indices))?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::string_filter::StringFilter;

    /// Mock LogReader for testing
    struct MockLogReader {
        lines: Vec<String>,
    }

    impl MockLogReader {
        fn new(lines: Vec<String>) -> Self {
            Self { lines }
        }
    }

    impl LogReader for MockLogReader {
        fn total_lines(&self) -> usize {
            self.lines.len()
        }

        fn get_line(&mut self, index: usize) -> Result<Option<String>> {
            Ok(self.lines.get(index).cloned())
        }

        fn reload(&mut self) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_filter_all_matching() {
        let lines = vec![
            "ERROR: First error".to_string(),
            "INFO: Some info".to_string(),
            "ERROR: Second error".to_string(),
            "DEBUG: Debug info".to_string(),
            "ERROR: Third error".to_string(),
        ];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));

        let rx = FilterEngine::run_filter(reader, filter, 1);

        // Collect all progress updates
        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices, vec![0, 2, 4]);
    }

    #[test]
    fn test_filter_no_matches() {
        let lines = vec![
            "INFO: First message".to_string(),
            "INFO: Second message".to_string(),
            "INFO: Third message".to_string(),
        ];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));

        let rx = FilterEngine::run_filter(reader, filter, 1);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert!(indices.is_empty());
    }

    #[test]
    fn test_filter_all_lines_match() {
        let lines = vec![
            "ERROR: First".to_string(),
            "ERROR: Second".to_string(),
            "ERROR: Third".to_string(),
        ];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));

        let rx = FilterEngine::run_filter(reader, filter, 1);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices, vec![0, 1, 2]);
    }

    #[test]
    fn test_filter_empty_reader() {
        let lines: Vec<String> = vec![];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));

        let rx = FilterEngine::run_filter(reader, filter, 1);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert!(indices.is_empty());
    }

    #[test]
    fn test_filter_progress_updates() {
        let lines: Vec<String> = (0..100)
            .map(|i| {
                if i % 10 == 0 {
                    format!("ERROR: Line {}", i)
                } else {
                    format!("INFO: Line {}", i)
                }
            })
            .collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));

        let rx = FilterEngine::run_filter(reader, filter, 10);

        let mut progress_updates = vec![];
        let mut final_result = None;

        while let Ok(progress) = rx.recv() {
            match progress {
                FilterProgress::Processing(line_num) => {
                    progress_updates.push(line_num);
                }
                FilterProgress::Complete(indices) => {
                    final_result = Some(indices);
                    break;
                }
                FilterProgress::Error(_) => panic!("Should not receive error"),
            }
        }

        // Should receive multiple progress updates
        assert!(!progress_updates.is_empty());

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices, vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
    }

    #[test]
    fn test_filter_range() {
        let lines: Vec<String> = (0..20)
            .map(|i| {
                if i % 2 == 0 {
                    format!("EVEN: Line {}", i)
                } else {
                    format!("ODD: Line {}", i)
                }
            })
            .collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("EVEN", false));

        // Filter only lines 5-15
        let rx = FilterEngine::run_filter_range(reader, filter, 1, 5, 15);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        // Lines 5-14, only even numbers: 6, 8, 10, 12, 14
        assert_eq!(indices, vec![6, 8, 10, 12, 14]);
    }

    #[test]
    fn test_filter_range_start_only() {
        let lines: Vec<String> = (0..10)
            .map(|i| {
                if i >= 5 {
                    format!("TARGET: Line {}", i)
                } else {
                    format!("OTHER: Line {}", i)
                }
            })
            .collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("TARGET", false));

        // Filter from line 3 to end
        let rx = FilterEngine::run_filter_range(reader, filter, 1, 3, 10);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices, vec![5, 6, 7, 8, 9]);
    }

    #[test]
    fn test_case_sensitive_filter() {
        let lines = vec![
            "ERROR: Uppercase".to_string(),
            "error: Lowercase".to_string(),
            "Error: Mixed case".to_string(),
        ];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", true));

        let rx = FilterEngine::run_filter(reader, filter, 1);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices, vec![0]); // Only the uppercase ERROR
    }

    #[test]
    fn test_large_file_simulation() {
        // Simulate a large file with 10,000 lines
        let lines: Vec<String> = (0..10000)
            .map(|i| {
                if i % 100 == 0 {
                    format!("MARKER: Line {}", i)
                } else {
                    format!("Normal: Line {}", i)
                }
            })
            .collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("MARKER", false));

        let rx = FilterEngine::run_filter(reader, filter, 1000);

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete(indices) = progress {
                final_result = Some(indices);
                break;
            }
        }

        let indices = final_result.expect("Should receive Complete message");
        assert_eq!(indices.len(), 100); // Should find 100 markers
        assert_eq!(indices[0], 0);
        assert_eq!(indices[99], 9900);
    }
}
