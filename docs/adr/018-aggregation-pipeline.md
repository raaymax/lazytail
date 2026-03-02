# ADR-018: Aggregation Pipeline

## Status

Accepted

## Context

Users analyzing structured logs frequently need to answer questions like "how many errors per service?" or "which status codes appear most?" The existing filter pipeline produced a flat list of matching lines, requiring users to mentally group results.

The aggregation system needed to:
- Integrate with the existing query language (`json | level == "error"`)
- Reuse existing field extraction (JSON and logfmt parsers)
- Support drill-down from aggregated groups back to individual lines
- Work within the TUI's event-driven architecture

## Decision

Add **post-filter aggregation** via `count by (field1, field2)` syntax appended to query expressions, with optional `top N` limiting.

The aggregation pipeline:
1. User enters a query with aggregation clause: `json | level == "error" | count by (service) top 10`
2. The filter pipeline runs as normal, producing `matching_indices`
3. When filtering completes, `AggregationResult::compute()` processes the matched lines:
   - Extracts group-by field values using the query's parser (JSON/logfmt)
   - Accumulates counts and line indices per group in a HashMap
   - Sorts groups by count descending
   - Applies optional `top N` limit
4. The view switches to `ViewMode::Aggregation`, rendering groups as a navigable list
5. Selecting a group triggers drill-down: switches to a filtered view showing only that group's lines

The `Aggregation` struct is part of the shared `FilterQuery` AST in `query.rs`, so both TUI and MCP can parse and use it.

## Consequences

**Benefits:**
- Natural extension of the query language — no new UI mode to learn
- Reuses existing parser infrastructure (JSON/logfmt field extraction)
- Drill-down preserves context: users can inspect individual lines behind a count
- Shared AST means MCP gets aggregation support automatically

**Trade-offs:**
- In-memory grouping: all matched lines must be read to extract field values. For very large result sets, this can be slow.
- Only `count by` is supported — no `sum`, `avg`, or other aggregation types
- Drill-down saves and restores aggregation state, adding complexity to `FilterConfig`
