# ADR-020: SearchEngine Extraction

## Status

Accepted

## Context

Filter dispatch logic — deciding whether to use SIMD, index bitmaps, byte offset ranges, or reader-based filtering — was duplicated between `FilterOrchestrator` (TUI) and `mcp/tools.rs` (MCP). Adding a new index-acceleration path (e.g., timestamp range filtering) required changes in both places, with the risk of feature gaps between adapters.

The hexagonal architecture analysis (see `docs/HEXAGONAL_ANALYSIS.md`) identified this as the highest-value extraction: centralizing dispatch logic so that index features become single-point changes.

## Decision

Extract the dispatch logic into `SearchEngine` (`src/filter/search_engine.rs`), a stateless struct with two entry points:

- `search_file()`: For file-backed sources. Picks the fastest path based on filter type, available index, query AST, and range:
  1. Index-accelerated + incremental range → `streaming_filter_range` with bitmap
  2. Index-accelerated full scan → `streaming_filter_indexed` with bitmap
  3. Generic full scan → `streaming_filter`
  4. SIMD fast path (plain text) → bypasses `Filter` trait entirely

- `search_reader()`: For stdin/pipe sources using `FilterEngine` with shared reader mutex

Both return `Receiver<FilterProgress>`, keeping the API uniform across all callers.

`FilterOrchestrator` (TUI/Web) delegates to `SearchEngine` after building the filter and managing `TabState` fields (cancel token, receiver, state transitions). MCP calls `SearchEngine` directly.

## Consequences

**Benefits:**
- Single point for all dispatch decisions — adding a new acceleration path (e.g., timestamp range) requires changing only `SearchEngine`
- Both TUI and MCP get the same optimizations automatically
- Stateless design makes the module easy to test in isolation
- Clear separation: `FilterOrchestrator` manages state, `SearchEngine` manages dispatch

**Trade-offs:**
- `FilterOrchestrator` still exists as a thin wrapper for state management — not fully eliminated
- MCP's synchronous usage pattern collects results from the receiver inline, which works but isn't the most natural API for synchronous callers
