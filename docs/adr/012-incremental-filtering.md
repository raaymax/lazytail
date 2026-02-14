# ADR-012: Incremental Filtering on File Growth

## Status

Accepted

## Context

Log files grow continuously. When a filter is active, new lines appended to the file need to be checked against the filter pattern. Re-filtering the entire file on every append would be wasteful.

Options considered:
1. **Full re-filter** - re-run filter on entire file when it changes
2. **Incremental filter** - only filter new lines (from `last_filtered_line` to `new_total`)
3. **Ignore new lines** - only show results from the initial filter run

## Decision

We use **incremental filtering**: when a file grows while a filter is active, only the new lines (from `tab.filter.last_filtered_line` to the new total) are filtered.

The flow:
1. `FileWatcher` detects modification
2. `FileReader.reload()` rebuilds sparse index, revealing new total
3. If `tab.filter.pattern.is_some()` and `new_total > tab.filter.last_filtered_line`:
   - `trigger_filter()` is called with `start_line` and `end_line` parameters
   - Streaming filter scans only the new range
   - Results are appended to existing `line_indices` via `append_filter_results()`
4. `last_filtered_line` is updated to `new_total`

The `FilterEngine` and `streaming_filter` both support range parameters (`start_line`, `end_line`) for this purpose.

File **truncation** (new total < old total) resets all filter state and returns to normal mode, since line indices become invalid.

## Consequences

**Benefits:**
- Constant-time filtering for append-only growth (only new lines are scanned)
- Users see filtered results update in real-time as logs are written
- Follow mode + filter works together (new matching lines appear at the end)

**Trade-offs:**
- Only handles append-only growth; truncation triggers full reset
- Incremental results are appended, not merged (assumes new lines have higher indices)
- `last_filtered_line` tracking adds state that must be reset on filter clear or change
