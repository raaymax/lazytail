# ADR-024: @ts Virtual Field and Time-Based Queries

## Status

Accepted

## Context

LazyTail's columnar index stores arrival timestamps (epoch milliseconds recording when each line was captured) in the `time` column. Users need to filter logs by time windows ("last 5 minutes", "between 2pm and 3pm") but the existing query system (`json | level == "error"`) only operates on log content fields parsed from the line itself. The timestamps in the index are orthogonal to any timestamp field inside the log line -- a log line's `timestamp` field records when the event occurred at the source, while the index's arrival time records when LazyTail ingested it.

Without a mechanism to query index timestamps, users had to visually scan or rely on content-based timestamp fields, which are format-dependent and require parsing every line.

## Decision

Introduce `@ts` as a **virtual field** in the query language that maps to the index's arrival timestamp column rather than any field in the log line content. The `@` prefix signals that this is metadata about the line, not data from the line.

**Query syntax integration:**

- TUI text syntax: `json | @ts >= "now-5m"` or `json | @ts >= "now-1h" | @ts < "now-30m" | level == "error"`
- MCP JSON queries: `@ts` filters can appear in the `filters` array alongside content filters, or in the dedicated `ts_filters` array on `FilterQuery`

**Parse-time separation via `partition_ts_filters()`:**

At parse time, `FilterQuery::partition_ts_filters()` splits filters into two groups: `@ts` entries move to `ts_filters`, everything else stays in `filters`. This separation is critical because the two kinds of filters execute at completely different layers -- `@ts` filters run against the index bitmap before any log content is read, while content filters require parsing each candidate line.

**Time value resolution (`src/filter/query/time.rs`):**

- Relative expressions: `now`, `now-5m`, `now-1h30m`, `now-2d12h`, `now+1h`. Compound offsets are supported with units `s`, `m`, `h`, `d`, `w`. Resolved at query time via `resolve_relative_time()`.
- Absolute timestamps: ISO 8601 / RFC 3339 (`2024-01-15T10:30:00Z`, `2024-01-15T10:30:00.123+05:30`), space-separated datetime (`2024-01-15 10:30:00`), epoch seconds (10-digit), epoch milliseconds (13-digit). Parsed via `parse_timestamp()`.
- Only comparison operators are allowed: `>`, `<`, `>=`, `<=`. Equality (`==`) and regex operators are rejected with an error since exact timestamp matching is rarely useful.

**`TsBounds` struct:**

`TsBounds::from_filters()` resolves all `@ts` filter values into epoch milliseconds and stores them as `Vec<(Operator, EpochMillis)>`. The `matches(ts)` method checks a single timestamp against all conditions with AND logic. Returns an error if a filter value cannot be resolved or uses an unsupported operator.

**Bitmap application in `SearchEngine` (`src/filter/search_engine.rs`):**

1. `SearchEngine::search_file()` first builds an optional bitmap from `index_mask()` (format flags, severity).
2. If the query has `@ts` filters, it iterates the index's timestamp column and builds a `ts_bitmap` where each entry is `ts_bounds.matches(ts)`.
3. The `ts_bitmap` is AND-ed with any existing index bitmap (from severity/format flags). If no prior bitmap exists, the `ts_bitmap` becomes the sole bitmap.
4. If `@ts` filters are present but no index is available, the search bails with an error: `"@ts filters require an index, but this source has none"`.
5. If `@ts` filters are present but the resulting bitmap is empty (index has 0 entries), an empty result set is returned immediately -- no streaming filter runs.
6. When `@ts` filters are active, the search uses the range-limited path (`run_streaming_filter_range`) capped to the indexed line count, ensuring lines beyond the index (which have no timestamps) are excluded.

**Arrival timestamp visibility:**

The `include_ts` parameter on MCP tools and the `t` key in the TUI toggle display of arrival timestamps on each line, helping users understand what `@ts` values correspond to which log lines.

## Consequences

**Benefits:**
- Time filtering is very fast: bitmap scan over the index's timestamp column, O(1) per line with no log content parsing required
- Composes naturally with existing filters -- `json | @ts >= "now-5m" | level == "error"` first narrows by time (bitmap), then by content
- Relative time expressions (`now-5m`) make ad-hoc queries convenient without knowing exact timestamps
- Shared `FilterQuery` AST means both TUI and MCP get `@ts` support automatically
- The `@` prefix convention leaves room for future virtual fields (e.g., `@line`, `@source`)

**Trade-offs:**
- `@ts` is the arrival time, not the timestamp inside the log line. If LazyTail ingests a batch of historical logs, `@ts` reflects the ingestion time, not when the events originally occurred. This distinction must be clear to users.
- Sources without a columnar index (e.g., plain stdin without capture mode) cannot use `@ts` -- the query fails with an explicit error rather than silently ignoring the filter.
- Relative time values are resolved once at query construction time. A long-running filter does not re-resolve `now` as time passes, so `now-5m` means "5 minutes before the query started", not a sliding window.
- The bitmap is built by iterating the full timestamp column linearly. For very large indexes this is still fast (sequential memory access), but a future optimization could use sorted-order binary search to find range boundaries in O(log n).
