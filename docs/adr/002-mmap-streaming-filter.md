# ADR-002: mmap-Based Streaming Filter

## Status

Accepted

## Context

Filtering large log files line-by-line using the `LogReader` trait requires seek + scan for each line, which is slow for large files. We needed grep-like performance for interactive filtering where users expect sub-second results on multi-gigabyte files.

Options considered:
1. **Per-line reader access** - seek to each line via sparse index, read, filter
2. **Full file read into memory** - load entire file, split lines, filter
3. **mmap + sequential scan** - memory-map file, scan linearly with SIMD

## Decision

We use **memory-mapped files with SIMD-accelerated scanning** (`streaming_filter.rs`):

- `memmap2::Mmap` for zero-copy file access
- `memchr::memchr` for SIMD-accelerated newline detection
- `memchr::memmem::Finder` for SIMD-accelerated pattern matching

Three filter strategies are selected based on the pattern type:

| Pattern | Strategy | Why |
|---------|----------|-----|
| Plain text, case-sensitive | `stream_filter_grep_style` | Find pattern first, count lines lazily near matches |
| Plain text, case-insensitive | `stream_filter_fast_impl` | Line-by-line scan with lowercase buffer reuse |
| Regex / Query | `stream_filter_impl` | Per-line UTF-8 conversion + regex/query matching |

The grep-style search (`stream_filter_grep_style`) is the fastest path: it searches for pattern occurrences across the entire file using SIMD memmem, then lazily determines line numbers only near matches. This avoids counting lines in regions without matches.

Results are sent as `FilterProgress::PartialResults` in batches of 50,000 lines, allowing the UI to display matches incrementally while filtering continues.

For **stdin/pipe** sources (no file path), the generic `FilterEngine` with shared `Arc<Mutex<LogReader>>` is used instead.

## Consequences

**Benefits:**
- Grep-like performance: sequential memory access, cache-friendly, SIMD-accelerated
- Zero-copy line access via mmap
- No memory allocation for line positions during scan
- Incremental results via batched `PartialResults`

**Trade-offs:**
- Requires file path (not applicable for stdin; falls back to `FilterEngine`)
- mmap may fail on very large files or low-memory systems
- Case-insensitive search requires a per-line lowercase buffer (still faster than per-line seek)
- Separate code paths for file vs stdin filtering
