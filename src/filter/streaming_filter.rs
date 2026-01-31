//! Streaming filter for grep-like performance on large files
//!
//! Instead of using per-line random access (which requires seek+scan for each line),
//! this filter streams through the file sequentially using mmap and memchr.
//! This is how grep achieves its speed.

use super::cancel::CancelToken;
use super::engine::FilterProgress;
use super::Filter;
use anyhow::Result;
use memchr::memmem;
use memmap2::Mmap;
use std::fs::File;
use std::path::Path;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use std::thread;

/// Batch size for sending partial results
const BATCH_SIZE: usize = 50_000;

/// How often to check for cancellation (every N lines)
const CANCEL_CHECK_INTERVAL: usize = 10_000;

/// Run a streaming filter on a file (grep-like performance)
///
/// This is MUCH faster than the per-line reader approach because:
/// 1. Sequential memory access (cache-friendly)
/// 2. No seeking - just scan through the mmap
/// 3. memchr for fast newline detection
/// 4. Zero-copy line access
/// 5. No memory allocation for line positions
pub fn run_streaming_filter<P>(
    path: P,
    filter: Arc<dyn Filter>,
    cancel: CancelToken,
) -> Result<Receiver<FilterProgress>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = channel();
    let path = path.as_ref().to_path_buf();

    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stream_filter_impl(&path, filter, tx.clone(), cancel)
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = tx.send(FilterProgress::Error(e.to_string()));
            }
            Err(_) => {
                let _ = tx.send(FilterProgress::Error(
                    "Streaming filter thread panicked".to_string(),
                ));
            }
        }
    });

    Ok(rx)
}

/// Internal implementation - streams through file in one pass
fn stream_filter_impl(
    path: &Path,
    filter: Arc<dyn Filter>,
    tx: Sender<FilterProgress>,
    cancel: CancelToken,
) -> Result<()> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 {
        tx.send(FilterProgress::Complete {
            matches: vec![],
            lines_processed: 0,
        })?;
        return Ok(());
    }

    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut all_matches = Vec::new();
    let mut batch_matches = Vec::new();
    let mut line_idx = 0usize;
    let mut pos = 0usize;

    // Stream through file, processing each line
    while pos < data.len() {
        // Check cancellation less frequently for performance
        if line_idx.is_multiple_of(CANCEL_CHECK_INTERVAL) && cancel.is_cancelled() {
            return Ok(());
        }

        // Find end of line using SIMD-accelerated memchr
        let line_end = memchr::memchr(b'\n', &data[pos..])
            .map(|offset| pos + offset)
            .unwrap_or(data.len());

        // Handle Windows line endings
        let content_end = if line_end > pos && data.get(line_end.saturating_sub(1)) == Some(&b'\r')
        {
            line_end - 1
        } else {
            line_end
        };

        // Check if line matches (skip invalid UTF-8)
        if let Ok(line) = std::str::from_utf8(&data[pos..content_end]) {
            if filter.matches(line) {
                batch_matches.push(line_idx);
            }
        }

        line_idx += 1;
        pos = line_end + 1;

        // Send batch update periodically
        if line_idx.is_multiple_of(BATCH_SIZE) && !batch_matches.is_empty() {
            all_matches.extend(batch_matches.iter().copied());

            let _ = tx.send(FilterProgress::PartialResults {
                matches: std::mem::take(&mut batch_matches),
                lines_processed: line_idx,
            });
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    // Add remaining matches
    all_matches.extend(batch_matches);
    tx.send(FilterProgress::Complete {
        matches: all_matches,
        lines_processed: line_idx,
    })?;

    Ok(())
}

/// Fast byte-level filter for plain text patterns (no UTF-8 overhead)
/// Uses SIMD-accelerated memmem for substring search
pub fn run_streaming_filter_fast<P>(
    path: P,
    pattern: &[u8],
    case_sensitive: bool,
    cancel: CancelToken,
) -> Result<Receiver<FilterProgress>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = channel();
    let path = path.as_ref().to_path_buf();
    let pattern = pattern.to_vec();

    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            if case_sensitive {
                // Case-sensitive: use grep-style search (find pattern, then count lines)
                stream_filter_grep_style(&path, &pattern, tx.clone(), cancel)
            } else {
                // Case-insensitive: must check each line (need to lowercase)
                stream_filter_fast_impl(&path, &pattern, false, tx.clone(), cancel)
            }
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = tx.send(FilterProgress::Error(e.to_string()));
            }
            Err(_) => {
                let _ = tx.send(FilterProgress::Error(
                    "Streaming filter thread panicked".to_string(),
                ));
            }
        }
    });

    Ok(rx)
}

/// Grep-style search: find pattern first, then determine line number lazily
/// This is faster when matches are sparse because we only count lines near matches
fn stream_filter_grep_style(
    path: &Path,
    pattern: &[u8],
    tx: Sender<FilterProgress>,
    cancel: CancelToken,
) -> Result<()> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 || pattern.is_empty() {
        tx.send(FilterProgress::Complete {
            matches: vec![],
            lines_processed: 0,
        })?;
        return Ok(());
    }

    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    // SIMD-accelerated pattern finder
    let finder = memmem::Finder::new(pattern);

    let mut matches = Vec::new();
    let mut search_pos = 0usize;

    // Track line counting progress to avoid recounting
    let mut counted_up_to_pos = 0usize;
    let mut counted_up_to_line = 0usize;
    let mut last_matched_line = usize::MAX;
    let mut check_count = 0usize;

    // Search for pattern occurrences
    while let Some(match_offset) = finder.find(&data[search_pos..]) {
        check_count += 1;
        if check_count.is_multiple_of(10000) && cancel.is_cancelled() {
            return Ok(());
        }

        let abs_pos = search_pos + match_offset;

        // Count lines from last counted position to this match
        // This is lazy - we only count lines in regions that have matches
        let line_num = if abs_pos >= counted_up_to_pos {
            let additional_lines =
                memchr::memchr_iter(b'\n', &data[counted_up_to_pos..abs_pos]).count();
            let line = counted_up_to_line + additional_lines;

            // Update our counting checkpoint to end of this line
            let line_end = memchr::memchr(b'\n', &data[abs_pos..])
                .map(|o| abs_pos + o + 1)
                .unwrap_or(data.len());
            counted_up_to_pos = line_end;
            counted_up_to_line = line + 1;

            line
        } else {
            // Match is before our checkpoint (shouldn't happen with forward search)
            // Fall back to counting from start
            memchr::memchr_iter(b'\n', &data[..abs_pos]).count()
        };

        // Only record each line once
        if line_num != last_matched_line {
            matches.push(line_num);
            last_matched_line = line_num;
        }

        // Move past this match
        search_pos = abs_pos + 1;
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    // Count total lines for progress (fast single pass)
    let total_lines = if counted_up_to_pos < data.len() {
        counted_up_to_line
            + memchr::memchr_iter(b'\n', &data[counted_up_to_pos..]).count()
            + if data.last() != Some(&b'\n') { 1 } else { 0 }
    } else {
        counted_up_to_line
    };

    let _ = tx.send(FilterProgress::PartialResults {
        matches: vec![],
        lines_processed: total_lines,
    });

    tx.send(FilterProgress::Complete {
        matches,
        lines_processed: total_lines,
    })?;
    Ok(())
}

/// Fast implementation using SIMD memmem - for case-insensitive search
fn stream_filter_fast_impl(
    path: &Path,
    pattern: &[u8],
    _case_sensitive: bool,
    tx: Sender<FilterProgress>,
    cancel: CancelToken,
) -> Result<()> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 || pattern.is_empty() {
        tx.send(FilterProgress::Complete {
            matches: vec![],
            lines_processed: 0,
        })?;
        return Ok(());
    }

    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    // For case-insensitive, we need lowercase pattern
    let lower_pattern: Vec<u8> = pattern.iter().map(|b| b.to_ascii_lowercase()).collect();
    let finder = memmem::Finder::new(&lower_pattern);

    let mut all_matches = Vec::new();
    let mut batch_matches = Vec::new();
    let mut line_idx = 0usize;
    let mut pos = 0usize;

    // Reusable buffer for lowercase conversion (avoid allocation per line)
    let mut lower_buf: Vec<u8> = Vec::with_capacity(4096);

    while pos < data.len() {
        // Check cancellation less frequently
        if line_idx.is_multiple_of(CANCEL_CHECK_INTERVAL) && cancel.is_cancelled() {
            return Ok(());
        }

        // Find end of line
        let line_end = memchr::memchr(b'\n', &data[pos..])
            .map(|offset| pos + offset)
            .unwrap_or(data.len());

        let content_end = if line_end > pos && data.get(line_end.saturating_sub(1)) == Some(&b'\r')
        {
            line_end - 1
        } else {
            line_end
        };

        let line_bytes = &data[pos..content_end];

        // Reuse buffer for lowercase conversion
        lower_buf.clear();
        lower_buf.extend(line_bytes.iter().map(|b| b.to_ascii_lowercase()));

        if finder.find(&lower_buf).is_some() {
            batch_matches.push(line_idx);
        }

        line_idx += 1;
        pos = line_end + 1;

        // Send batch update periodically
        if line_idx.is_multiple_of(BATCH_SIZE) && !batch_matches.is_empty() {
            all_matches.extend(batch_matches.iter().copied());

            let _ = tx.send(FilterProgress::PartialResults {
                matches: std::mem::take(&mut batch_matches),
                lines_processed: line_idx,
            });
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    all_matches.extend(batch_matches);
    tx.send(FilterProgress::Complete {
        matches: all_matches,
        lines_processed: line_idx,
    })?;

    Ok(())
}

/// Run streaming filter on a range of a file (for incremental filtering)
pub fn run_streaming_filter_range<P>(
    path: P,
    filter: Arc<dyn Filter>,
    start_line: usize,
    end_line: usize,
    cancel: CancelToken,
) -> Result<Receiver<FilterProgress>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = channel();
    let path = path.as_ref().to_path_buf();

    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stream_filter_range_impl(&path, filter, start_line, end_line, tx.clone(), cancel)
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = tx.send(FilterProgress::Error(e.to_string()));
            }
            Err(_) => {
                let _ = tx.send(FilterProgress::Error(
                    "Streaming filter thread panicked".to_string(),
                ));
            }
        }
    });

    Ok(rx)
}

/// Internal implementation for range filtering
fn stream_filter_range_impl(
    path: &Path,
    filter: Arc<dyn Filter>,
    start_line: usize,
    end_line: usize,
    tx: Sender<FilterProgress>,
    cancel: CancelToken,
) -> Result<()> {
    let file = File::open(path)?;
    let metadata = file.metadata()?;

    if metadata.len() == 0 || start_line >= end_line {
        tx.send(FilterProgress::Complete {
            matches: vec![],
            lines_processed: 0,
        })?;
        return Ok(());
    }

    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut all_matches = Vec::new();
    let mut batch_matches = Vec::new();
    let mut line_idx = 0usize;
    let mut pos = 0usize;
    let mut lines_in_range = 0usize;

    while pos < data.len() && line_idx < end_line {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let line_end = memchr::memchr(b'\n', &data[pos..])
            .map(|offset| pos + offset)
            .unwrap_or(data.len());

        // Only process lines in range
        if line_idx >= start_line {
            let content_end =
                if line_end > pos && data.get(line_end.saturating_sub(1)) == Some(&b'\r') {
                    line_end - 1
                } else {
                    line_end
                };

            if let Ok(line) = std::str::from_utf8(&data[pos..content_end]) {
                if filter.matches(line) {
                    batch_matches.push(line_idx);
                }
            }

            lines_in_range += 1;

            if lines_in_range.is_multiple_of(BATCH_SIZE) && !batch_matches.is_empty() {
                all_matches.extend(batch_matches.iter().copied());

                if cancel.is_cancelled() {
                    return Ok(());
                }

                let _ = tx.send(FilterProgress::PartialResults {
                    matches: std::mem::take(&mut batch_matches),
                    lines_processed: lines_in_range,
                });
            }
        }

        line_idx += 1;
        pos = line_end + 1;
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    all_matches.extend(batch_matches);
    tx.send(FilterProgress::Complete {
        matches: all_matches,
        lines_processed: lines_in_range,
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::filter::string_filter::StringFilter;
    use crate::filter::Filter;
    use std::io::Write;
    use tempfile::NamedTempFile;

    fn create_test_file(lines: &[&str]) -> NamedTempFile {
        let mut file = NamedTempFile::new().unwrap();
        for line in lines {
            writeln!(file, "{}", line).unwrap();
        }
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_streaming_filter_basic() {
        let file = create_test_file(&[
            "ERROR: first error",
            "INFO: some info",
            "ERROR: second error",
            "DEBUG: debug",
            "ERROR: third error",
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert_eq!(indices, vec![0, 2, 4]);
    }

    #[test]
    fn test_streaming_filter_no_matches() {
        let file = create_test_file(&["INFO: first", "INFO: second", "DEBUG: third"]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert!(indices.is_empty());
    }

    #[test]
    fn test_streaming_filter_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert!(indices.is_empty());
    }

    #[test]
    fn test_streaming_filter_range() {
        let file = create_test_file(&[
            "ERROR: 0", "ERROR: 1", "INFO: 2", "ERROR: 3", "ERROR: 4", "INFO: 5", "ERROR: 6",
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        // Filter only lines 2-5
        let rx = run_streaming_filter_range(path, filter, 2, 6, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert_eq!(indices, vec![3, 4]); // Lines 3 and 4 match within range 2-6
    }

    #[test]
    fn test_streaming_filter_case_insensitive() {
        let file = create_test_file(&["ERROR: caps", "error: lower", "Error: mixed"]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("error", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert_eq!(indices, vec![0, 1, 2]); // All match case-insensitively
    }

    #[test]
    fn test_fast_filter_case_sensitive() {
        let file = create_test_file(&[
            "ERROR: first",
            "error: second",
            "Error: third",
            "INFO: fourth",
        ]);
        let path = file.path().to_path_buf();

        let rx = run_streaming_filter_fast(path, b"ERROR", true, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert_eq!(indices, vec![0]); // Only exact match
    }

    #[test]
    fn test_fast_filter_case_insensitive() {
        let file = create_test_file(&[
            "ERROR: first",
            "error: second",
            "Error: third",
            "INFO: fourth",
        ]);
        let path = file.path().to_path_buf();

        let rx = run_streaming_filter_fast(path, b"error", false, CancelToken::new()).unwrap();

        let mut result = None;
        while let Ok(progress) = rx.recv() {
            if let FilterProgress::Complete {
                matches: indices, ..
            } = progress
            {
                result = Some(indices);
                break;
            }
        }

        let indices = result.expect("Should complete");
        assert_eq!(indices, vec![0, 1, 2]); // All ERROR/error/Error match
    }
}
