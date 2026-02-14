# ADR-003: Sparse Indexing for File Reader

## Status

Accepted

## Context

A log viewer needs random access to any line in a file. The naive approach is to store byte offsets for every line, but this uses O(n) memory: ~800MB for 100M lines (8 bytes per offset).

Options considered:
1. **Full line index** - store offset for every line (O(n) memory)
2. **Sparse index** - store offset for every Nth line, scan forward from nearest
3. **No index** - scan from beginning every time (too slow for random access)
4. **mmap with line counting** - mmap entire file, scan for newlines on each access

## Decision

`FileReader` uses a **sparse index** that stores byte offsets for every 10,000th line:

- On file open: single scan to build sparse index and count total lines
- On line access: seek to nearest indexed position, scan forward (at most 9,999 lines)
- On reload (file change): rebuild the entire sparse index

Memory usage: ~120 bytes for 100M lines (10 entries x 12 bytes each).

The `SparseIndex` stores `(line_number, byte_offset)` pairs and provides a `locate(line_num) -> (offset, skip_count)` method that returns the nearest indexed position and how many lines to skip.

## Consequences

**Benefits:**
- O(1) memory regardless of file size (only ~10 index entries for typical files)
- Acceptable random access latency (scan at most 10,000 lines from nearest index point)
- Simple implementation with `BufReader::seek()` + `read_line()`
- Works with any file encoding (line-by-line scanning)

**Trade-offs:**
- Random access requires scanning up to `interval` lines (not O(1) time)
- Full rescan on reload (acceptable since file changes trigger reload anyway)
- Not optimal for huge files with frequent random access (but the streaming filter bypasses this entirely for filtering)
- `BufReader::seek()` clears the internal buffer, which is correct but means no read-ahead caching
