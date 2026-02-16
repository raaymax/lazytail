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

    // SAFETY: File handle remains valid for mmap lifetime. Read-only access.
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

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
            let _ = tx.send(FilterProgress::PartialResults {
                matches: std::mem::take(&mut batch_matches),
                lines_processed: line_idx,
            });
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    // Send remaining unsent matches in Complete (partials already delivered the rest)
    tx.send(FilterProgress::Complete {
        matches: batch_matches,
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

    // SAFETY: File handle remains valid for mmap lifetime. Read-only access.
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

    // SAFETY: File handle remains valid for mmap lifetime. Read-only access.
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    // For case-insensitive, we need lowercase pattern
    let lower_pattern: Vec<u8> = pattern.iter().map(|b| b.to_ascii_lowercase()).collect();
    let finder = memmem::Finder::new(&lower_pattern);

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
            let _ = tx.send(FilterProgress::PartialResults {
                matches: std::mem::take(&mut batch_matches),
                lines_processed: line_idx,
            });
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    tx.send(FilterProgress::Complete {
        matches: batch_matches,
        lines_processed: line_idx,
    })?;

    Ok(())
}

/// Run streaming filter on a range of a file (for incremental filtering)
///
/// If `start_byte_offset` is provided, the filter skips directly to that byte position
/// instead of scanning newlines from byte 0 to reach `start_line`. This avoids an O(n)
/// scan of the entire file prefix for every incremental filter invocation.
///
/// If `bitmap` is provided, only lines where `bitmap[line_idx]` is true are checked
/// with the content filter. Lines past the bitmap length are always checked.
pub fn run_streaming_filter_range<P>(
    path: P,
    filter: Arc<dyn Filter>,
    start_line: usize,
    end_line: usize,
    start_byte_offset: Option<u64>,
    bitmap: Option<Vec<bool>>,
    cancel: CancelToken,
) -> Result<Receiver<FilterProgress>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = channel();
    let path = path.as_ref().to_path_buf();

    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stream_filter_range_impl(
                &path,
                filter,
                start_line,
                end_line,
                start_byte_offset,
                bitmap.as_deref(),
                tx.clone(),
                cancel,
            )
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
///
/// When `start_byte_offset` is `Some`, jumps directly to that byte position and begins
/// processing at `line_idx = start_line`. Otherwise falls back to scanning from byte 0.
///
/// When `bitmap` is `Some`, only candidate lines (where `bitmap[line_idx]` is true) are
/// checked with the content filter. Lines past the bitmap are always checked.
#[allow(clippy::too_many_arguments)]
fn stream_filter_range_impl(
    path: &Path,
    filter: Arc<dyn Filter>,
    start_line: usize,
    end_line: usize,
    start_byte_offset: Option<u64>,
    bitmap: Option<&[bool]>,
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

    // SAFETY: File handle remains valid for mmap lifetime. Read-only access.
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut batch_matches = Vec::new();
    let mut lines_in_range = 0usize;

    // If we have a known byte offset for start_line, jump directly to it.
    // This avoids an O(file_size) newline scan just to find the starting position.
    let (mut pos, mut line_idx) = if let Some(offset) = start_byte_offset {
        let offset = offset as usize;
        if offset > data.len() {
            tx.send(FilterProgress::Complete {
                matches: vec![],
                lines_processed: 0,
            })?;
            return Ok(());
        }
        (offset, start_line)
    } else {
        (0, 0)
    };

    while pos < data.len() && line_idx < end_line {
        if cancel.is_cancelled() {
            return Ok(());
        }

        let line_end = memchr::memchr(b'\n', &data[pos..])
            .map(|offset| pos + offset)
            .unwrap_or(data.len());

        // Only process lines in range
        if line_idx >= start_line {
            // Check bitmap: skip non-candidate lines.
            // Lines past the bitmap are always candidates (index shorter than file).
            let is_candidate = bitmap
                .map(|b| b.get(line_idx).copied().unwrap_or(true))
                .unwrap_or(true);

            if is_candidate {
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
            }

            lines_in_range += 1;

            if lines_in_range.is_multiple_of(BATCH_SIZE) && !batch_matches.is_empty() {
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

    tx.send(FilterProgress::Complete {
        matches: batch_matches,
        lines_processed: lines_in_range,
    })?;

    Ok(())
}

/// Run an index-accelerated streaming filter on a file.
///
/// Uses the columnar index to pre-filter lines by flags (format, severity),
/// then only runs `filter.matches()` on candidate lines. For structured queries
/// like `json | level == "error"`, this eliminates ~98% of JSON/logfmt parsing.
///
/// The `bitmap` parameter is a boolean slice where `bitmap[i] == true` means
/// line `i` is a candidate that should be checked with the content filter.
/// Lines past the bitmap length are also checked (handles index shorter than file).
pub fn run_streaming_filter_indexed<P>(
    path: P,
    filter: Arc<dyn Filter>,
    bitmap: Vec<bool>,
    cancel: CancelToken,
) -> Result<Receiver<FilterProgress>>
where
    P: AsRef<Path> + Send + 'static,
{
    let (tx, rx) = channel();
    let path = path.as_ref().to_path_buf();

    thread::spawn(move || {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            stream_filter_indexed_impl(&path, filter, &bitmap, tx.clone(), cancel)
        }));

        match result {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                let _ = tx.send(FilterProgress::Error(e.to_string()));
            }
            Err(_) => {
                let _ = tx.send(FilterProgress::Error(
                    "Index-accelerated filter thread panicked".to_string(),
                ));
            }
        }
    });

    Ok(rx)
}

/// Internal implementation for index-accelerated filtering.
///
/// Iterates lines sequentially (mmap + memchr), but only calls `filter.matches()`
/// on lines where `bitmap[line_idx]` is true. Non-candidate lines are skipped
/// with just a newline scan (no content parsing).
fn stream_filter_indexed_impl(
    path: &Path,
    filter: Arc<dyn Filter>,
    bitmap: &[bool],
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

    // SAFETY: File handle remains valid for mmap lifetime. Read-only access.
    let mmap = unsafe { Mmap::map(&file)? };
    let data = &mmap[..];

    let mut batch_matches = Vec::new();
    let mut line_idx = 0usize;
    let mut pos = 0usize;

    while pos < data.len() {
        if line_idx.is_multiple_of(CANCEL_CHECK_INTERVAL) && cancel.is_cancelled() {
            return Ok(());
        }

        // Find end of line using SIMD-accelerated memchr
        let line_end = memchr::memchr(b'\n', &data[pos..])
            .map(|offset| pos + offset)
            .unwrap_or(data.len());

        // Check bitmap: if line is within bitmap and not a candidate, skip it.
        // Lines past the bitmap are always checked (index may be shorter than file).
        let is_candidate = line_idx >= bitmap.len() || bitmap[line_idx];

        if is_candidate {
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
        }

        line_idx += 1;
        pos = line_end + 1;

        if line_idx.is_multiple_of(BATCH_SIZE) && !batch_matches.is_empty() {
            let _ = tx.send(FilterProgress::PartialResults {
                matches: std::mem::take(&mut batch_matches),
                lines_processed: line_idx,
            });
        }
    }

    if cancel.is_cancelled() {
        return Ok(());
    }

    tx.send(FilterProgress::Complete {
        matches: batch_matches,
        lines_processed: line_idx,
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

    /// Collect all matches from both PartialResults and Complete messages.
    fn collect_matches(rx: Receiver<FilterProgress>) -> Vec<usize> {
        let mut all = Vec::new();
        while let Ok(progress) = rx.recv() {
            match progress {
                FilterProgress::PartialResults { matches, .. } => {
                    all.extend(matches);
                }
                FilterProgress::Complete { matches, .. } => {
                    all.extend(matches);
                    return all;
                }
                _ => {}
            }
        }
        panic!("Channel closed without Complete");
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
        let indices = collect_matches(rx);
        assert_eq!(indices, vec![0, 2, 4]);
    }

    #[test]
    fn test_streaming_filter_no_matches() {
        let file = create_test_file(&["INFO: first", "INFO: second", "DEBUG: third"]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_streaming_filter_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_streaming_filter_range() {
        let file = create_test_file(&[
            "ERROR: 0", "ERROR: 1", "INFO: 2", "ERROR: 3", "ERROR: 4", "INFO: 5", "ERROR: 6",
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        // Filter only lines 2-5 (no byte offset, no bitmap — scans from start)
        let rx =
            run_streaming_filter_range(path, filter, 2, 6, None, None, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert_eq!(indices, vec![3, 4]); // Lines 3 and 4 match within range 2-6
    }

    #[test]
    fn test_streaming_filter_range_with_byte_offset() {
        let file = create_test_file(&[
            "ERROR: 0", "ERROR: 1", "INFO: 2", "ERROR: 3", "ERROR: 4", "INFO: 5", "ERROR: 6",
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));

        // Compute byte offset for line 2:
        // "ERROR: 0\n" = 9 bytes (offset 0)
        // "ERROR: 1\n" = 9 bytes (offset 9)
        // "INFO: 2\n"  starts at offset 18
        let start_byte_offset = Some(18u64);

        let rx = run_streaming_filter_range(
            path,
            filter,
            2,
            6,
            start_byte_offset,
            None,
            CancelToken::new(),
        )
        .unwrap();
        let indices = collect_matches(rx);
        // Same results as without byte offset: lines 3 and 4 match within range 2-6
        assert_eq!(indices, vec![3, 4]);
    }

    #[test]
    fn test_streaming_filter_range_byte_offset_matches_scan() {
        // Verify that byte offset path produces identical results to scan-from-zero
        let lines = &[
            "INFO: first line",
            "ERROR: something bad",
            "DEBUG: some debug",
            "ERROR: another error",
            "WARN: a warning",
            "ERROR: third error",
            "INFO: last line",
        ];
        let file = create_test_file(lines);
        let path = file.path().to_path_buf();

        // Compute byte offset for line 3 by summing prior line lengths
        let offset: u64 = lines[..3].iter().map(|l| l.len() as u64 + 1).sum(); // +1 for \n

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));

        // Without byte offset (scan from 0)
        let rx_scan = run_streaming_filter_range(
            path.clone(),
            filter.clone(),
            3,
            7,
            None,
            None,
            CancelToken::new(),
        )
        .unwrap();
        let scan_result = collect_matches(rx_scan);

        // With byte offset (direct seek)
        let rx_seek =
            run_streaming_filter_range(path, filter, 3, 7, Some(offset), None, CancelToken::new())
                .unwrap();
        let seek_result = collect_matches(rx_seek);

        assert_eq!(scan_result, seek_result);
        assert_eq!(seek_result, vec![3, 5]); // Lines 3 and 5 contain "ERROR"
    }

    #[test]
    fn test_streaming_filter_range_with_bitmap() {
        // Bitmap pre-filters which lines to check within the range
        let file = create_test_file(&[
            "ERROR: 0", // line 0
            "ERROR: 1", // line 1
            "INFO: 2",  // line 2
            "ERROR: 3", // line 3 — candidate, matches
            "ERROR: 4", // line 4 — NOT candidate (bitmap=false), skipped
            "INFO: 5",  // line 5 — candidate, no match
            "ERROR: 6", // line 6
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        // Bitmap: lines 3 and 5 are candidates, line 4 is not
        let bitmap = vec![true, true, true, true, false, true, true];

        let rx =
            run_streaming_filter_range(path, filter, 2, 6, None, Some(bitmap), CancelToken::new())
                .unwrap();
        let result = collect_matches(rx);

        // Without bitmap: would match lines 3, 4. With bitmap: line 4 is skipped.
        assert_eq!(result, vec![3]);
    }

    #[test]
    fn test_streaming_filter_range_bitmap_shorter_than_range() {
        // Bitmap only covers first 4 lines; lines 4+ should be checked (past bitmap)
        let file = create_test_file(&[
            "ERROR: 0", "INFO: 1", "INFO: 2", "INFO: 3", "ERROR: 4", "ERROR: 5",
        ]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        // Bitmap covers lines 0-3 only; lines 4-5 are past the bitmap → always checked
        let bitmap = vec![true, false, false, false];

        let rx =
            run_streaming_filter_range(path, filter, 3, 6, None, Some(bitmap), CancelToken::new())
                .unwrap();
        let result = collect_matches(rx);

        // Line 3: bitmap[3]=false → skipped
        // Lines 4,5: past bitmap → checked, both match "ERROR"
        assert_eq!(result, vec![4, 5]);
    }

    #[test]
    fn test_streaming_filter_case_insensitive() {
        let file = create_test_file(&["ERROR: caps", "error: lower", "Error: mixed"]);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("error", false));
        let rx = run_streaming_filter(path, filter, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
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
        let indices = collect_matches(rx);
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
        let indices = collect_matches(rx);
        assert_eq!(indices, vec![0, 1, 2]); // All ERROR/error/Error match
    }

    // ========================================================================
    // Index-accelerated filter tests
    // ========================================================================

    #[test]
    fn test_indexed_filter_skips_non_candidates() {
        // File with mixed content: JSON and non-JSON lines
        let file = create_test_file(&[
            r#"{"level":"error","msg":"first"}"#, // line 0: JSON error - candidate
            "INFO: plain text log",               // line 1: not JSON - skip
            r#"{"level":"info","msg":"second"}"#, // line 2: JSON info - skip
            r#"{"level":"error","msg":"third"}"#, // line 3: JSON error - candidate
            "DEBUG: another plain line",          // line 4: not JSON - skip
        ]);
        let path = file.path().to_path_buf();

        // Bitmap: only lines 0 and 3 are candidates (JSON error lines)
        let bitmap = vec![true, false, false, true, false];

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("error", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        // Only line 0 and 3 were checked; line 0 and 3 contain "error"
        assert_eq!(indices, vec![0, 3]);
    }

    #[test]
    fn test_indexed_filter_all_candidates() {
        // When all lines are candidates, behaves like regular filter
        let file = create_test_file(&["ERROR: first", "INFO: second", "ERROR: third"]);
        let path = file.path().to_path_buf();

        let bitmap = vec![true, true, true]; // All candidates

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn test_indexed_filter_no_candidates() {
        let file = create_test_file(&["ERROR: first", "ERROR: second", "ERROR: third"]);
        let path = file.path().to_path_buf();

        let bitmap = vec![false, false, false]; // No candidates

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_indexed_filter_bitmap_shorter_than_file() {
        // Bitmap only covers first 2 lines, remaining lines should be checked
        let file = create_test_file(&["ERROR: 0", "INFO: 1", "ERROR: 2", "ERROR: 3"]);
        let path = file.path().to_path_buf();

        let bitmap = vec![true, false]; // Only covers lines 0-1

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        // Line 0 is candidate and matches, line 1 skipped,
        // lines 2,3 past bitmap so checked, both match
        assert_eq!(indices, vec![0, 2, 3]);
    }

    #[test]
    fn test_indexed_filter_empty_bitmap() {
        // Empty bitmap = all lines are checked (no index available)
        let file = create_test_file(&["ERROR: 0", "INFO: 1", "ERROR: 2"]);
        let path = file.path().to_path_buf();

        let bitmap = vec![]; // Empty

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert_eq!(indices, vec![0, 2]);
    }

    #[test]
    fn test_indexed_filter_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let path = file.path().to_path_buf();

        let bitmap = vec![];
        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("ERROR", false));
        let rx = run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indices = collect_matches(rx);
        assert!(indices.is_empty());
    }

    #[test]
    fn test_indexed_filter_same_results_as_regular() {
        // The indexed filter should produce the same results as the regular filter
        // when bitmap is all-true
        let lines = &[
            r#"{"level":"error","msg":"fail"}"#,
            "plain text line",
            r#"{"level":"info","msg":"ok"}"#,
            r#"not json at all"#,
            r#"{"level":"error","msg":"timeout"}"#,
        ];
        let file = create_test_file(lines);
        let path = file.path().to_path_buf();

        let filter: Arc<dyn Filter> = Arc::new(StringFilter::new("error", false));

        // Regular filter
        let rx_regular =
            run_streaming_filter(path.clone(), filter.clone(), CancelToken::new()).unwrap();
        let regular_result = collect_matches(rx_regular);

        // Indexed filter with all-true bitmap
        let bitmap = vec![true; lines.len()];
        let rx_indexed =
            run_streaming_filter_indexed(path, filter, bitmap, CancelToken::new()).unwrap();
        let indexed_result = collect_matches(rx_indexed);

        assert_eq!(regular_result, indexed_result);
    }
}
