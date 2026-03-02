# ADR-019: Combined / Merged Source View

## Status

Accepted

## Context

Users working with microservices often need to correlate logs across multiple services. Opening each service in a separate tab requires manual context-switching. A unified chronological view of all sources would let users see the full timeline of events across services.

The merge mechanism needed to:
- Interleave lines from multiple sources by timestamp
- Work transparently with existing filter, viewport, and rendering infrastructure
- Handle sources with and without timestamp indexes
- Avoid deadlocks when sharing readers with individual source tabs

## Decision

Implement `CombinedReader` as a `LogReader` trait implementation that merges lines from multiple sources in timestamp order.

**Merge strategy:**
- Collect all lines from all sources as `MergedLine { source_id, file_line, timestamp }`
- Extract timestamps from the columnar index when available (`IndexReader::get_timestamp`)
- Sources without indexes default to timestamp 0 (sorted to the beginning)
- Stable sort by timestamp preserves source order for lines with equal timestamps

**LogReader transparency:** Because `CombinedReader` implements `LogReader`, the existing viewport, filter pipeline, and renderer work without special cases. Tabs with `is_combined: true` use a `CombinedReader` instead of a single-file reader.

**Lock ordering:** `CombinedReader` holds `Arc<Mutex<dyn LogReader>>` references shared with individual source tabs. Lock ordering is always outer → inner (combined reader → source reader) to prevent deadlocks.

**Source attribution:** `source_info()` method maps virtual line numbers back to their origin source, enabling source-prefixed rendering and source-specific renderer selection.

## Consequences

**Benefits:**
- Unified timeline across services without manual correlation
- Transparent integration: filters, viewport navigation, and line expansion all work on combined views
- Source attribution preserves context (which service produced each line)

**Trade-offs:**
- Rebuilding the merged index on source changes (new lines appended) requires re-collecting and re-sorting all lines
- Sources without indexes lack timestamps and cluster at the beginning of the merged view
- Shared reader locks may cause brief render stalls if a filter thread holds a source lock
- Memory grows linearly with total line count across all sources (one `MergedLine` per line)
