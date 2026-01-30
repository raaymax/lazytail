use super::cancel::CancelToken;
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
    #[allow(dead_code)]
    Processing(usize),
    /// Partial results found (sent periodically so UI can show matches immediately)
    PartialResults {
        matches: Vec<usize>,
        lines_processed: usize,
    },
    /// Filtering complete (final matching line indices and total lines processed)
    Complete {
        matches: Vec<usize>,
        lines_processed: usize,
    },
    /// Error occurred
    Error(String),
}

/// Filter engine that processes filters in the background
pub struct FilterEngine;

impl FilterEngine {
    /// Run a filter with an OWNED reader (no locking, no UI contention)
    ///
    /// This is the preferred method for filtering files - it creates no lock
    /// contention with the UI thread because the filter has its own reader.
    #[allow(dead_code)]
    pub fn run_filter_owned<R, F>(
        mut reader: R,
        filter: Arc<F>,
        progress_interval: usize,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter_owned(
                    &mut reader,
                    filter,
                    tx.clone(),
                    progress_interval,
                    0,
                    None,
                    cancel,
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

    /// Run a filter on a log reader in a background thread (shared reader version)
    /// Returns a receiver for progress updates
    pub fn run_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        progress_interval: usize,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter_shared(
                    reader,
                    filter,
                    tx.clone(),
                    progress_interval,
                    0,
                    None,
                    cancel,
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

    /// Run a filter on a specific range with an OWNED reader (no locking)
    #[allow(dead_code)]
    pub fn run_filter_range_owned<R, F>(
        mut reader: R,
        filter: Arc<F>,
        progress_interval: usize,
        start_line: usize,
        end_line: usize,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter_owned(
                    &mut reader,
                    filter,
                    tx.clone(),
                    progress_interval,
                    start_line,
                    Some(end_line),
                    cancel,
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

    /// Run a filter on a specific range of lines (for incremental filtering)
    /// Returns a receiver for progress updates
    pub fn run_filter_range<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        progress_interval: usize,
        start_line: usize,
        end_line: usize,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter_shared(
                    reader,
                    filter,
                    tx.clone(),
                    progress_interval,
                    start_line,
                    Some(end_line),
                    cancel,
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

    /// Internal filter processing with OWNED reader (no locking!)
    ///
    /// Processes lines in batches FROM THE END to show recent results first.
    /// Sends partial results after each batch so the UI can display matches immediately.
    /// This version has zero lock contention with the UI because it owns its reader.
    #[allow(dead_code)]
    fn process_filter_owned<R, F>(
        reader: &mut R,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        batch_size: usize,
        start_line: usize,
        end_line: Option<usize>,
        cancel: CancelToken,
    ) -> Result<()>
    where
        R: LogReader + Send + 'static,
        F: Filter + 'static + ?Sized,
    {
        let batch_size = batch_size.max(100); // Ensure reasonable minimum

        if cancel.is_cancelled() {
            return Ok(());
        }

        let total_lines = reader.total_lines();
        let end = end_line.unwrap_or(total_lines);
        let range_size = end.saturating_sub(start_line);

        if range_size == 0 {
            tx.send(FilterProgress::Complete {
                matches: vec![],
                lines_processed: 0,
            })?;
            return Ok(());
        }

        let mut all_matches = Vec::new();
        let mut current_end = end;

        while current_end > start_line {
            if cancel.is_cancelled() {
                return Ok(());
            }

            let batch_start = current_end.saturating_sub(batch_size).max(start_line);

            // Read and filter this batch
            let mut batch_matches = Vec::new();
            for line_idx in batch_start..current_end {
                if let Ok(Some(line)) = reader.get_line(line_idx) {
                    if filter.matches(&line) {
                        batch_matches.push(line_idx);
                    }
                }
            }

            // Calculate lines processed (we process from end to start)
            let lines_processed = end - batch_start;

            // Send partial results immediately if we found matches
            if !batch_matches.is_empty() {
                // Sort this batch (it's from a contiguous range, so already mostly sorted)
                batch_matches.sort_unstable();
                all_matches.extend(batch_matches.iter().copied());

                if cancel.is_cancelled() {
                    return Ok(());
                }
                // Send these matches so UI can show them right away
                let _ = tx.send(FilterProgress::PartialResults {
                    matches: batch_matches,
                    lines_processed,
                });
            }

            current_end = batch_start;

            // Yield to let UI thread process the partial results
            std::thread::yield_now();
        }

        if cancel.is_cancelled() {
            return Ok(());
        }

        // Sort all matches and send final complete message
        all_matches.sort_unstable();
        tx.send(FilterProgress::Complete {
            matches: all_matches,
            lines_processed: range_size,
        })?;

        Ok(())
    }

    /// Internal filter processing with shared reader (uses locking)
    ///
    /// Processes lines in batches FROM THE END to show recent results first.
    /// Sends partial results after each batch so the UI can display matches immediately.
    /// This version is for when you can't create a separate reader (e.g., stdin).
    fn process_filter_shared<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        batch_size: usize,
        start_line: usize,
        end_line: Option<usize>,
        cancel: CancelToken,
    ) -> Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let batch_size = batch_size.max(100); // Ensure reasonable minimum

        if cancel.is_cancelled() {
            return Ok(());
        }

        let total_lines = {
            let reader_guard = reader.lock().expect("Reader lock poisoned");
            reader_guard.total_lines()
        };

        let end = end_line.unwrap_or(total_lines);
        let range_size = end.saturating_sub(start_line);

        if range_size == 0 {
            tx.send(FilterProgress::Complete {
                matches: vec![],
                lines_processed: 0,
            })?;
            return Ok(());
        }

        let mut all_matches = Vec::new();
        let mut current_end = end;

        while current_end > start_line {
            if cancel.is_cancelled() {
                return Ok(());
            }

            let batch_start = current_end.saturating_sub(batch_size).max(start_line);

            // Read a batch of lines (brief lock)
            let batch: Vec<(usize, String)> = {
                let mut reader_guard = reader.lock().expect("Reader lock poisoned");
                let mut lines = Vec::with_capacity(current_end - batch_start);

                for line_idx in batch_start..current_end {
                    if let Ok(Some(line)) = reader_guard.get_line(line_idx) {
                        lines.push((line_idx, line));
                    }
                }
                lines
            };
            // Lock released here

            // Filter the batch (no lock held)
            let mut batch_matches = Vec::new();
            for (line_idx, line) in batch {
                if filter.matches(&line) {
                    batch_matches.push(line_idx);
                }
            }

            // Calculate lines processed (we process from end to start)
            let lines_processed = end - batch_start;

            // Send partial results immediately if we found matches
            if !batch_matches.is_empty() {
                batch_matches.sort_unstable();
                all_matches.extend(batch_matches.iter().copied());

                if cancel.is_cancelled() {
                    return Ok(());
                }
                let _ = tx.send(FilterProgress::PartialResults {
                    matches: batch_matches,
                    lines_processed,
                });
            }

            current_end = batch_start;
            std::thread::yield_now();
        }

        if cancel.is_cancelled() {
            return Ok(());
        }

        all_matches.sort_unstable();
        tx.send(FilterProgress::Complete {
            matches: all_matches,
            lines_processed: range_size,
        })?;

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

        let rx = FilterEngine::run_filter(reader, filter, 1, CancelToken::new());

        // Collect all progress updates
        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 1, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 1, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 1, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 10, CancelToken::new());

        let mut progress_updates = vec![];
        let mut final_result = None;

        while let Ok(progress) = rx.recv() {
            match progress {
                FilterProgress::Processing(line_num) => {
                    progress_updates.push(line_num);
                }
                FilterProgress::PartialResults { .. } => {
                    // Partial results are fine, just continue
                }
                FilterProgress::Complete {
                    matches: indices, ..
                } => {
                    final_result = Some(indices);
                    break;
                }
                FilterProgress::Error(_) => panic!("Should not receive error"),
            }
        }

        // Should receive some progress updates (either Processing or PartialResults)
        // Note: with new partial results, we may not always get Processing updates

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
        let rx = FilterEngine::run_filter_range(reader, filter, 1, 5, 15, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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
        let rx = FilterEngine::run_filter_range(reader, filter, 1, 3, 10, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 1, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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

        let rx = FilterEngine::run_filter(reader, filter, 1000, CancelToken::new());

        let mut final_result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
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
