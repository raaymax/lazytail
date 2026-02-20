# ADR-014: Hexagonal Architecture — LogSourceState Extraction

## Status

Accepted

## Context

LazyTail has three consumer interfaces (adapters) for log data:

- **TUI** — interactive terminal UI via ratatui
- **Web** — HTTP server with embedded SPA (`lazytail web`)
- **MCP** — Model Context Protocol server for AI assistants (`lazytail --mcp`)

Before this change, `TabState` was a monolithic struct with 28+ fields mixing domain concerns (reader, filter state, line indices, index reader) with TUI-specific concerns (viewport, expansion, file watcher, stream handles). This caused several problems:

1. **Duplicated filter dispatch.** The Web adapter reimplemented ~135 lines of filter dispatch logic (`trigger_filter_for_tab` + `trigger_query_filter_for_tab`) that duplicated `FilterOrchestrator`. The Web version lacked index acceleration, creating a feature gap.
2. **No severity data in Web/MCP.** The columnar index (severity per line, severity counts from checkpoints) was only wired into the TUI renderer. Web and MCP had no access path.
3. **Unclear ownership boundaries.** New contributors couldn't tell which fields were domain state vs adapter state, making it easy to accidentally couple adapter logic to domain internals.
4. **Testing friction.** Testing filter or index behavior required constructing a full `TabState` with viewport and watcher fields that were irrelevant to the test.

## Decision

Extract domain-only state into a new `LogSourceState` struct (`src/source_state.rs`). `TabState` embeds it as `pub source: LogSourceState` and keeps only TUI adapter fields.

### LogSourceState (domain core)

```rust
pub struct LogSourceState {
    pub name: String,
    pub source_path: Option<PathBuf>,
    pub mode: ViewMode,
    pub total_lines: usize,
    pub line_indices: Vec<usize>,
    pub follow_mode: bool,
    pub reader: Arc<Mutex<dyn LogReader + Send>>,
    pub filter: FilterConfig,
    pub source_status: Option<SourceStatus>,
    pub disabled: bool,
    pub file_size: Option<u64>,
    pub index_reader: Option<IndexReader>,
    pub index_size: Option<u64>,
}
```

### TabState (TUI adapter)

```rust
pub struct TabState {
    pub source: LogSourceState,        // domain core
    pub viewport: Viewport,            // TUI: scroll/selection
    pub expansion: ExpansionState,     // TUI: expanded lines
    pub watcher: Option<FileWatcher>,  // TUI: file change detection
    stream_writer: ...,                // TUI: stdin background reader
    pub stream_receiver: ...,          // TUI: stdin message channel
    pub config_source_type: ...,       // TUI: config-defined source type
}
```

### FilterOrchestrator operates on LogSourceState

```rust
// Before:
pub fn trigger(tab: &mut TabState, pattern, mode, range)

// After:
pub fn trigger(source: &mut LogSourceState, pattern, mode, range)
```

All three adapters call the same entry point:
- **TUI**: `FilterOrchestrator::trigger(&mut tab.source, ...)`
- **Web**: `FilterOrchestrator::trigger(&mut tab.source, ...)` — replaces 135 lines of duplicated dispatch
- **MCP**: continues with its own synchronous path (no `LogSourceState` instance), but shares the same `FilterQuery` AST and index acceleration primitives

### Severity wired through LogSourceState

Since `LogSourceState` owns `index_reader: Option<IndexReader>`, all adapters can access severity data:
- **TUI**: `tab.source.index_reader.as_ref().map(|ir| ir.severity(line))` (unchanged)
- **Web**: `LineRow` gains `severity: Option<&'static str>`, `SourceView` gains `severity_counts`
- **MCP**: `LineInfo` gains `severity: Option<String>`, text format shows `[L{n}] [{severity}] {content}`

## Alternatives Considered

### Deref<Target=LogSourceState> on TabState

Would allow `tab.field` instead of `tab.source.field`, reducing diff size. Rejected because implicit delegation obscures ownership — it becomes unclear whether a field lives on `TabState` or `LogSourceState`, defeating the purpose of the separation.

### Trait-based ports (FilterPort, IndexPort, SourcePort)

Extracting traits for all domain operations. Deferred — the concrete types work well today. Traits add value when we need mock implementations for testing or alternative backends (e.g., remote log sources). The current struct extraction is a prerequisite for traits and can be extended later without breaking changes.

### Web/MCP use LogSourceState directly (not via TabState)

Web and MCP could instantiate `LogSourceState` without wrapping in `TabState`. Not done yet because the Web adapter reuses `TabState`'s file watcher and filter progress polling. A future change could create a lighter `WebSourceState` wrapper if needed.

## Consequences

### Benefits

- **Single filter dispatch path.** Web uses `FilterOrchestrator::trigger` instead of its own reimplementation. Adding a new filter type (e.g., Lucene syntax) requires changes only in `filter/` — all three adapters pick it up automatically.
- **Severity available everywhere.** Web API returns per-line severity and aggregate severity counts. MCP returns severity in `get_lines`, `get_tail`, and `get_context` responses.
- **Clear domain boundary.** `LogSourceState` documents exactly which fields are domain state. Adapter-specific additions go on the adapter wrapper.
- **Test-friendly.** Domain logic can be tested by constructing `LogSourceState` alone, without viewport or watcher setup.

### Trade-offs

- **Field access is one level deeper.** All consumer code changed from `tab.field` to `tab.source.field`. This was a large mechanical diff (~200 edits across 6 files) but each edit was trivial.
- **Re-exports needed.** `tab.rs` re-exports `FilterConfig` and `LogSourceState` for backward compatibility with existing `use crate::tab::FilterConfig` imports.
- **Web still uses TabState.** The web adapter wraps `LogSourceState` in `TabState` for file watcher reuse. This is pragmatic but means web carries unused TUI fields (viewport, expansion).

### Migration

All changes are internal refactoring — no public API, CLI, or configuration changes. The 489-test suite passes unchanged (tests updated mechanically for field path changes).
