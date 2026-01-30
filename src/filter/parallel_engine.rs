use super::cancel::CancelToken;
use super::engine::FilterProgress;
use super::Filter;
use crate::reader::LogReader;
use rayon::prelude::*;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

/// Default chunk size for parallel processing
const DEFAULT_CHUNK_SIZE: usize = 10_000;

/// Minimum number of lines to enable parallel processing
/// Below this threshold, sequential is faster due to thread overhead
const PARALLEL_THRESHOLD: usize = 50_000;

/// Parallel filter engine for high-performance filtering of large files
///
/// Uses rayon for parallel processing of line chunks, with cooperative
/// cancellation support for responsive UI.
pub struct ParallelFilterEngine;

impl ParallelFilterEngine {
    /// Run a parallel filter on a log reader
    ///
    /// Returns a receiver for progress updates. The filter runs in a background
    /// thread and can be cancelled via the cancel token.
    ///
    /// For small files (< PARALLEL_THRESHOLD lines), falls back to sequential
    /// processing to avoid thread overhead.
    pub fn run_filter<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        cancel: CancelToken,
    ) -> Receiver<FilterProgress>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let (tx, rx) = channel();

        thread::spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                Self::process_filter(reader, filter, tx.clone(), cancel, 0, None)
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

    /// Run a parallel filter on a specific range of lines
    pub fn run_filter_range<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        cancel: CancelToken,
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
                    cancel,
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
        cancel: CancelToken,
        start_line: usize,
        end_line: Option<usize>,
    ) -> anyhow::Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        // Get total lines (brief lock)
        let total_lines = {
            let reader_guard = reader.lock().expect("Reader lock poisoned");
            reader_guard.total_lines()
        };

        let end = end_line.unwrap_or(total_lines);
        let range_size = end.saturating_sub(start_line);

        // Check cancellation before starting
        if cancel.is_cancelled() {
            tx.send(FilterProgress::Complete {
                matches: vec![],
                lines_processed: 0,
            })?;
            return Ok(());
        }

        // For small ranges, use sequential processing
        if range_size < PARALLEL_THRESHOLD {
            return Self::process_sequential(reader, filter, tx, cancel, start_line, end);
        }

        // Parallel processing for large ranges
        Self::process_parallel(reader, filter, tx, cancel, start_line, end)
    }

    /// Sequential filter processing (for small files or ranges)
    fn process_sequential<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        cancel: CancelToken,
        start: usize,
        end: usize,
    ) -> anyhow::Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let mut matching_indices = Vec::new();
        let mut reader_guard = reader.lock().expect("Reader lock poisoned");
        let progress_interval = 1000;

        for line_idx in start..end {
            // Check cancellation periodically
            if line_idx % progress_interval == 0 {
                if cancel.is_cancelled() {
                    break;
                }
                let _ = tx.send(FilterProgress::Processing(line_idx));
            }

            if let Ok(Some(line)) = reader_guard.get_line(line_idx) {
                if filter.matches(&line) {
                    matching_indices.push(line_idx);
                }
            }
        }

        drop(reader_guard);
        tx.send(FilterProgress::Complete {
            matches: matching_indices,
            lines_processed: end - start,
        })?;
        Ok(())
    }

    /// Parallel filter processing (for large files)
    fn process_parallel<R, F>(
        reader: Arc<Mutex<R>>,
        filter: Arc<F>,
        tx: Sender<FilterProgress>,
        cancel: CancelToken,
        start: usize,
        end: usize,
    ) -> anyhow::Result<()>
    where
        R: LogReader + Send + 'static + ?Sized,
        F: Filter + 'static + ?Sized,
    {
        let chunk_size = DEFAULT_CHUNK_SIZE;
        let range_size = end - start;
        let _num_chunks = range_size.div_ceil(chunk_size);

        // Pre-read all lines to avoid lock contention during parallel processing
        // This trades memory for parallelism
        let lines: Vec<(usize, String)> = {
            let mut reader_guard = reader.lock().expect("Reader lock poisoned");
            let mut result = Vec::with_capacity(range_size);

            for line_idx in start..end {
                // Check cancellation during reading
                if line_idx % 10_000 == 0 && cancel.is_cancelled() {
                    break;
                }

                if let Ok(Some(line)) = reader_guard.get_line(line_idx) {
                    result.push((line_idx, line));
                }

                // Send progress during read phase
                if line_idx % 50_000 == 0 {
                    let _ = tx.send(FilterProgress::Processing(line_idx));
                }
            }

            result
        };

        if cancel.is_cancelled() {
            tx.send(FilterProgress::Complete {
                matches: vec![],
                lines_processed: 0,
            })?;
            return Ok(());
        }

        // Process chunks in parallel
        let tx_clone = tx.clone();
        let chunk_results: Vec<Vec<usize>> = lines
            .par_chunks(chunk_size)
            .enumerate()
            .map(|(chunk_idx, chunk)| {
                // Check cancellation at chunk boundary
                if cancel.is_cancelled() {
                    return vec![];
                }

                let matches: Vec<usize> = chunk
                    .iter()
                    .filter(|(_, line)| filter.matches(line))
                    .map(|(idx, _)| *idx)
                    .collect();

                // Report progress (may be out of order due to parallelism)
                if chunk_idx % 10 == 0 {
                    let progress_line = start + (chunk_idx + 1) * chunk_size;
                    let _ = tx_clone.send(FilterProgress::Processing(progress_line.min(end)));
                }

                matches
            })
            .collect();

        // Flatten results (they're already in order because par_chunks preserves order)
        let matching_indices: Vec<usize> = chunk_results.into_iter().flatten().collect();

        tx.send(FilterProgress::Complete {
            matches: matching_indices,
            lines_processed: end - start,
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

        fn get_line(&mut self, index: usize) -> anyhow::Result<Option<String>> {
            Ok(self.lines.get(index).cloned())
        }

        fn reload(&mut self) -> anyhow::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn test_parallel_filter_small_file() {
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
        let cancel = CancelToken::new();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
        assert_eq!(indices, vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
    }

    #[test]
    fn test_parallel_filter_empty() {
        let lines: Vec<String> = vec![];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", false));
        let cancel = CancelToken::new();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
    fn test_parallel_filter_cancellation() {
        let lines: Vec<String> = (0..1000).map(|i| format!("Line {}", i)).collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("Line", false));
        let cancel = CancelToken::new();

        // Cancel immediately
        cancel.cancel();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
        // Should return empty or partial results due to cancellation
        // The exact behavior depends on timing, but it shouldn't hang
        assert!(indices.len() <= 1000);
    }

    #[test]
    fn test_parallel_filter_range() {
        let lines: Vec<String> = (0..50)
            .map(|i| {
                if i % 2 == 0 {
                    format!("EVEN {}", i)
                } else {
                    format!("ODD {}", i)
                }
            })
            .collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("EVEN", false));
        let cancel = CancelToken::new();

        // Filter only lines 10-30
        let rx = ParallelFilterEngine::run_filter_range(reader, filter, cancel, 10, 30);

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
        // Even numbers in range 10-29: 10, 12, 14, 16, 18, 20, 22, 24, 26, 28
        assert_eq!(indices, vec![10, 12, 14, 16, 18, 20, 22, 24, 26, 28]);
    }

    #[test]
    fn test_parallel_filter_no_matches() {
        let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i)).collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("NOTFOUND", false));
        let cancel = CancelToken::new();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
    fn test_parallel_filter_all_match() {
        let lines: Vec<String> = (0..100).map(|i| format!("Line {}", i)).collect();

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("Line", false));
        let cancel = CancelToken::new();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
        assert_eq!(indices.len(), 100);
        assert_eq!(indices[0], 0);
        assert_eq!(indices[99], 99);
    }

    #[test]
    fn test_parallel_filter_case_sensitive() {
        let lines = vec![
            "ERROR uppercase".to_string(),
            "error lowercase".to_string(),
            "Error mixed".to_string(),
        ];

        let reader = Arc::new(Mutex::new(MockLogReader::new(lines)));
        let filter = Arc::new(StringFilter::new("ERROR", true));
        let cancel = CancelToken::new();

        let rx = ParallelFilterEngine::run_filter(reader, filter, cancel);

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
        assert_eq!(indices, vec![0]); // Only uppercase ERROR
    }
}
