# ADR-015: Columnar Index System

## Status

Accepted

## Context

LazyTail needed per-line metadata (severity level, timestamps, byte offsets) for features like severity-based coloring, severity histograms, and accelerated filtering. The index had to support:

- Concurrent reading while a capture process writes new lines
- Incremental updates (append-only during capture)
- Selective column loading (only read flags when filtering by severity)
- Low memory overhead for large files (millions of lines)

Alternatives considered:
- **SQLite**: Too heavy for a CLI tool; adds a runtime dependency and write-ahead log complexity. Overkill for append-only integer columns.
- **Single binary file**: All columns interleaved per row. Simpler, but forces reading all columns even when only flags are needed. Poor cache locality for column-scan queries.
- **Parquet/Arrow**: Designed for analytics workloads with compression and encoding schemes. Too complex for our fixed-width integer columns and append-only write pattern.

## Decision

Use a **per-column binary file** format where each column is stored as a separate append-only file of fixed-width little-endian values:

- `flags` (u32): severity level, format flags, line classification bits
- `offsets` (u64): byte offset of each line in the log file
- `lengths` (u32): byte length of each line
- `time` (u64): extracted timestamp per line
- `checkpoints`: periodic severity count snapshots for histograms

A 64-byte `IndexMeta` header tracks which columns are present (via `ColumnBit` flags) and the entry count.

`IndexReader` copies column data into owned `Vec<T>` at open time rather than keeping mmap handles alive. This makes readers immune to SIGBUS when a concurrent writer truncates and re-creates a column file.

`IndexWriteLock` (`lock.rs`) uses `flock(2)` to prevent concurrent index builds. The lock is automatically released if the holding process crashes.

## Consequences

**Benefits:**
- Selective column loading: filtering by severity only reads the `flags` file
- Append-only writes: capture mode appends entries without rebuilding
- Type safety: `ColumnReader<T>` and `ColumnWriter<T>` enforce fixed-size encoding via the `ColumnElement` trait
- Reader isolation: copying data at open time eliminates SIGBUS risk from concurrent writers
- Simple format: no compression, no encoding — just fixed-width integers. Easy to debug with `hexdump`

**Trade-offs:**
- Multiple files per index directory (one per column) increases filesystem overhead
- Copying data at open time uses more memory than mmap (but bounded by file size)
- No built-in compression — index size is proportional to line count × column width
- Lock file mechanism depends on `flock(2)` semantics (Unix-only)
