# Hexagonal Architecture Analysis

## The Problem

Three interfaces (TUI, MCP, Web UI) each independently wire together readers, indexes, and filters. Adding an index feature means touching `FilterOrchestrator` (TUI), `mcp/tools.rs` (MCP), and the Web UI. The core domain operations — "open a log source", "search with index acceleration", "get line content" — were duplicated across adapters rather than expressed once.

> **Update:** Phases 1–2 from the recommended approach below have been implemented. `LogSource` (in `src/log_source.rs`) and `SearchEngine` (in `src/filter/search_engine.rs`) now exist. `FilterOrchestrator` delegates search dispatch to `SearchEngine`, and all three adapters share the `LogSource` domain struct via `TabState.source`. The coupling map below reflects the state before these changes.

## Current Coupling Map

```
                 ┌──────────────────────────────────────────┐
                 │               ADAPTERS                    │
                 │                                           │
                 │  TUI (app.rs + tab.rs)                    │
                 │    ├─ FileReader::new()          direct   │
                 │    ├─ StreamReader::new()         direct   │
                 │    ├─ IndexReader::open()         direct   │
                 │    ├─ ColumnReader::<u64>::open() direct   │
                 │    ├─ index_dir_for_log()         direct   │
                 │    └─ FilterOrchestrator::trigger()        │
                 │         ├─ streaming_filter::*     direct   │
                 │         ├─ FilterEngine::*         direct   │
                 │         ├─ ColumnReader::<u64>     direct   │
                 │         └─ index_dir_for_log()     direct   │
                 │                                           │
                 │  MCP (mcp/tools.rs)                       │
                 │    ├─ FileReader::new()          direct   │
                 │    ├─ IndexReader::open()         direct   │
                 │    ├─ IndexMeta::read_from()      direct   │
                 │    ├─ ColumnReader::<u32>::open() direct   │
                 │    ├─ CheckpointReader::open()    direct   │
                 │    ├─ index_dir_for_log()         direct   │
                 │    ├─ streaming_filter::*         direct   │
                 │    └─ query::QueryFilter::new()   direct   │
                 │                                           │
                 │  Web UI (future)                          │
                 │    └─ ... same thing again?               │
                 └──────────────────────────────────────────┘
```

Every adapter reaches through to implementation details: index directory layout, column file names, which streaming_filter variant to call, how to combine index bitmaps with filters. This is the root cause of the problem.

## What Specifically Gets Duplicated

### 1. Index-accelerated query dispatch

**FilterOrchestrator** (orchestrator.rs:48-103):
```rust
if let Some((mask, want)) = filter_query.index_mask() {
    if let Some(ref index_reader) = tab.index_reader {
        let bitmap = index_reader.candidate_bitmap(mask, want, index_reader.len());
        streaming_filter::run_streaming_filter_indexed(path, filter, bitmap, cancel)
    }
}
```

**MCP** (tools.rs:412-459):
```rust
let bitmap = query.index_mask().and_then(|(mask, want)| {
    let reader = IndexReader::open(path)?;
    Some(reader.candidate_bitmap(mask, want, reader.len()))
});
if let Some(bitmap) = bitmap {
    streaming_filter::run_streaming_filter_indexed(path, filter, bitmap, cancel)
}
```

Same logic, two places. Adding a new index-acceleration path (e.g., timestamp range filtering) means changing both.

### 2. Byte offset lookup for incremental filtering

**FilterOrchestrator** (orchestrator.rs:68-73 and 169-173):
```rust
let start_byte_offset = {
    let idx_dir = index_dir_for_log(path);
    ColumnReader::<u64>::open(idx_dir.join("offsets"), start + 1)
        .ok()
        .and_then(|r| r.get(start))
};
```

This appears **twice** in the orchestrator alone (once for query path, once for dispatch path). MCP doesn't do incremental filtering yet, but the Web UI will need it.

### 3. Index stats gathering

**MCP** (tools.rs:477-539) directly reads meta, flags columns, and checkpoint columns to build stats. The TUI reads `IndexReader` for severity and `calculate_index_size()` for display. Neither goes through a unified stats interface.

### 4. Reader creation + index association

**TabState::new()** (tab.rs:194-204):
```rust
let file_reader = FileReader::new(&path)?;
let index_reader = IndexReader::open(&path);
let index_size = calculate_index_size(&path);
```

**MCP get_lines_impl** (tools.rs:234):
```rust
let mut reader = FileReader::new(path)?;
```

MCP creates readers without indexes. TabState creates readers with indexes. There's no single "open a log source" operation.

## Proposed Architecture: LogStore Facade

Adapters shouldn't know about `FileReader`, `IndexReader`, `ColumnReader`, `streaming_filter`, or index directory layout. They should interact with a **LogStore** that encapsulates all of that.

```
                 ┌─────────────────────────────────────┐
                 │             ADAPTERS                  │
                 │  TUI    MCP    Web UI                │
                 │   │      │       │                   │
                 │   └──────┼───────┘                   │
                 │          │                           │
                 │    LogSource + SearchEngine           │
                 │          │                           │
                 └──────────┼───────────────────────────┘
                            │
                 ┌──────────┼───────────────────────────┐
                 │          │      DOMAIN CORE           │
                 │   ┌──────┴──────┐                    │
                 │   │  LogSource  │ (per-source state)  │
                 │   │  ├ reader   │                    │
                 │   │  ├ index    │                    │
                 │   │  └ meta     │                    │
                 │   └─────────────┘                    │
                 │          │                           │
                 │   ┌──────┴──────┐                    │
                 │   │SearchEngine │ (filter dispatch)   │
                 │   │  ├ index-accelerated             │
                 │   │  ├ streaming (mmap)              │
                 │   │  ├ range (incremental)           │
                 │   │  └ in-memory (stdin)             │
                 │   └─────────────┘                    │
                 │                                      │
                 │   Readers / Indexes / Filters         │
                 │   (implementation details, private)   │
                 └──────────────────────────────────────┘
```

### LogSource: Single Entry Point for a Log File

```rust
/// A log source with associated index (if available).
/// This is the domain object that adapters work with.
pub struct LogSource {
    path: PathBuf,
    reader: FileReader,         // private
    index: Option<IndexReader>, // private
    meta: Option<IndexMeta>,    // private
}

impl LogSource {
    /// Open a log source. Automatically loads index if available.
    pub fn open(path: &Path) -> Result<Self>;

    // --- Reading ---
    pub fn total_lines(&self) -> usize;
    pub fn get_line(&mut self, index: usize) -> Result<Option<String>>;
    pub fn get_lines(&mut self, range: Range<usize>) -> Vec<(usize, String)>;

    // --- Index info (adapters never touch IndexReader directly) ---
    pub fn severity(&self, line: usize) -> Severity;
    pub fn has_index(&self) -> bool;
    pub fn index_stats(&self) -> Option<IndexStats>;

    // --- Expose for SearchEngine ---
    pub fn index(&self) -> Option<&IndexReader>;
    pub fn path(&self) -> &Path;
}
```

### SearchEngine: Unified Filter Dispatch

Extract the dispatch logic from both `FilterOrchestrator` and `mcp/tools.rs::query_impl` into one place:

```rust
/// Unified search dispatch. Picks the fastest execution path
/// based on available indexes, filter type, and source type.
pub struct SearchEngine;

impl SearchEngine {
    /// Run a search, automatically using index acceleration when available.
    pub fn search(
        path: &Path,
        filter: Arc<dyn Filter>,
        index: Option<&IndexReader>,
        query: Option<&FilterQuery>,
        range: Option<Range<usize>>,
    ) -> Result<Receiver<FilterProgress>>;

    /// Fast path: plain text search with optional SIMD.
    pub fn search_text(
        path: &Path,
        pattern: &[u8],
        case_sensitive: bool,
    ) -> Result<Receiver<FilterProgress>>;
}
```

This is where all the "should I use SIMD?", "should I use bitmap?", "should I use byte offsets?" decisions live — **once**.

### What Each Adapter Becomes

**TUI (FilterOrchestrator)**:
```rust
// Before: 150 lines of dispatch logic
// After:
pub fn trigger(tab: &mut TabState, pattern: String, mode: FilterMode, range: ...) {
    let filter = build_filter(&pattern, mode)?;
    let query = parse_query_if_applicable(&pattern);
    tab.filter.receiver = Some(
        SearchEngine::search(
            tab.source.path(),
            filter,
            tab.source.index(),
            query.as_ref(),
            range,
        )?
    );
}
```

**MCP (query_impl)**:
```rust
// Before: 60 lines of index loading + dispatch
// After:
pub fn query_impl(path: &Path, query: FilterQuery, ...) -> String {
    let source = LogSource::open(path)?;
    let filter = QueryFilter::new(query)?;
    let rx = SearchEngine::search(
        source.path(), filter, source.index(), Some(&query), None
    )?;
    collect_and_format(rx)
}
```

**MCP (get_stats_impl)**:
```rust
// Before: 60 lines reaching into IndexMeta, ColumnReader, CheckpointReader
// After:
pub fn get_stats_impl(path: &Path, ...) -> String {
    let source = LogSource::open(path)?;
    let stats = source.index_stats(); // All the details are inside
    format_stats(stats)
}
```

## Difficulty Assessment

### Easy (can do incrementally)

**1. Extract SearchEngine from FilterOrchestrator** — Low effort

The dispatch logic in `orchestrator.rs:25-220` already has the right shape. The main blocker is that it takes `&mut TabState` instead of primitive args. Refactoring to take `(path, reader, index, filter, range)` is straightforward and doesn't change behavior.

Steps:
- Create `src/filter/search_engine.rs` with a function that takes path + filter + index + range
- Have `FilterOrchestrator::trigger()` call it (thin wrapper managing TabState fields)
- Have MCP's `query_impl` and `search_impl` call it
- Delete duplicate dispatch code from MCP

Estimated scope: ~200 lines moved/refactored, 0 behavior changes.

**2. Create IndexStats abstraction** — Low effort

`mcp/tools.rs:477-539` (get_stats_impl) reads meta, flags, and checkpoints manually. Wrapping this into `IndexReader::stats() -> IndexStats` is pure extraction. The TUI could use the same for its status bar.

Estimated scope: ~50 lines new struct + method, ~60 lines simplified in MCP.

**3. Hide index_dir_for_log()** — Low effort

Currently called from: `tab.rs`, `orchestrator.rs` (x2), `mcp/tools.rs`, `reader/file_reader.rs`. All callers want the same thing: "give me index data for this log path." Moving it inside `IndexReader::open()` and `LogSource::open()` removes it from adapter code entirely.

Estimated scope: ~10 call sites updated.

### Medium (needs care but doable)

**4. Create LogSource facade** — Medium effort

This combines FileReader + IndexReader + metadata into one object. Challenges:
- TabState currently holds `reader`, `index_reader`, `index_size`, `file_size` separately
- These are initialized at different times (reader first, index after)
- TabState also holds stream-specific state that LogSource shouldn't know about

LogSource handles **file-backed sources only**. Stream (stdin/pipe) sources stay as-is — they don't have indexes anyway.

The tricky part: `reader` is behind `Arc<Mutex<>>` because FilterEngine needs shared access for stdin filtering. LogSource could expose `reader()` returning the Arc, or SearchEngine could take ownership of the search path.

Estimated scope: ~300-400 lines refactored across tab.rs, app.rs, UI code.

**5. Make FilterOrchestrator adapter-agnostic** — Medium effort

Currently `FilterOrchestrator::trigger()` takes `&mut TabState` and directly modifies `tab.filter.receiver`, `tab.filter.cancel_token`, `tab.filter.state`, etc. This is TUI-specific — MCP doesn't have TabState.

Refactoring to return a `Receiver<FilterProgress>` (letting the caller decide what to do with it) would make it usable by all adapters. But the cancellation token management and state transitions need to move to the caller.

Estimated scope: ~150 lines in orchestrator + ~50 lines in each adapter.

### Hard (significant effort)

**6. True crate separation** — High effort

Splitting into `lazytail-core`, `lazytail-tui`, `lazytail-mcp` workspace crates. This would:
- Force clean dependency boundaries (compile-time enforcement)
- Allow the Web UI to depend on `lazytail-core` only
- Make it impossible for adapters to reach into internals

But it requires:
- Moving ~15K lines into a core crate
- Resolving circular dependencies (e.g., `FilterOrchestrator` imports from `app.rs`)
- Feature flags for optional components
- CI changes for workspace builds

Could be done after steps 1-5 have cleaned up the internal boundaries.

## Recommended Approach: Incremental, Inside-Out

Don't try to do all of this at once. The following sequence minimizes risk and delivers value at each step:

### Phase 1: SearchEngine extraction (1-2 sessions)

1. Create `src/filter/search_engine.rs`
2. Move dispatch logic from `FilterOrchestrator::dispatch()` and the query path
3. Make it take `(path, filter, index, query, range)` — no TabState dependency
4. Update `FilterOrchestrator::trigger()` to call SearchEngine
5. Update MCP's `search_impl` and `query_impl` to call SearchEngine
6. Delete duplicate code from MCP

**Result**: Index-accelerated search logic exists in exactly one place. Adding a new index feature (e.g., timestamp range) means changing SearchEngine only.

### Phase 2: LogSource facade (1-2 sessions)

1. Create `src/log_source.rs` combining FileReader + IndexReader + metadata
2. Add convenience methods: `severity()`, `has_index()`, `index_stats()`
3. Update TabState to use LogSource for file-backed tabs
4. Update MCP to use LogSource instead of raw FileReader + IndexReader
5. Hide `index_dir_for_log()` inside LogSource/IndexReader

**Result**: Opening a log source is a single operation. Index details are encapsulated.

### Phase 3: Clean adapter boundaries (1 session)

1. Make FilterOrchestrator return `Receiver<FilterProgress>` instead of mutating TabState
2. Have TUI code handle the TabState mutation
3. MCP and Web UI get the same interface as TUI

**Result**: All three adapters use the same domain API surface.

### Phase 4 (optional): Crate separation

Only if the team grows or the project needs it. With phases 1-3 done, the internal boundaries are clean enough that crate extraction becomes mostly mechanical.

## What NOT To Do

- **Don't create a `trait LogStore`** with dynamic dispatch for the facade. Use a concrete struct — there's only one implementation, and trait objects add complexity without value here.
- **Don't try to abstract over stdin vs file at the facade level.** Stdin sources are fundamentally different (no path, no index, in-memory). Keep them separate. LogSource is for file-backed sources only.
- **Don't refactor the Filter trait or FilterProgress enum.** These are already well-abstracted and shared correctly between TUI and MCP.
- **Don't move UI/rendering code.** The rendering layer is already cleanly separated in `src/ui/`.

## Impact on Adding Index Features

**Today** (adding e.g. timestamp range index):
1. Add column to `IndexBuilder`
2. Add reader method to `IndexReader`
3. Update `FilterOrchestrator` (TUI path) to use it
4. Update `mcp/tools.rs` (MCP path) to use it
5. Update Web UI to use it
6. Hope you didn't miss a code path

**After Phase 1-2** (same feature):
1. Add column to `IndexBuilder`
2. Add reader method to `IndexReader`
3. Update `SearchEngine` to use it — **done**
4. All three interfaces automatically get the acceleration

That's the core value proposition: **index features become single-point changes**.

## What's Already Good

Don't lose sight of what's working:

- **Filter trait** — Simple, clean, all implementations interchangeable
- **LogReader trait** — Good ISP with StreamableReader separation
- **FilterProgress enum** — Both TUI and MCP consume it identically
- **FilterQuery AST** — Shared between TUI and MCP, new operators work everywhere
- **Event-driven core loop** — Clean separation of event collection vs processing
- **UI rendering** — Already fully separated in `src/ui/`
- **streaming_filter functions** — Well-designed, just need to be called from one place instead of two
