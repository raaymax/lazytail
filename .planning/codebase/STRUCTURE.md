# Codebase Structure

**Analysis Date:** 2026-02-03

## Directory Layout

```
/home/raay/Workspace/LazyTail/
├── src/                    # Source code
│   ├── app.rs             # Global app state (App, ViewMode, FilterState, InputMode)
│   ├── main.rs            # Event loop, CLI entry point, mode detection
│   ├── tab.rs             # Per-tab state (TabState, FilterConfig, ExpandMode)
│   ├── viewport.rs        # Vim-style scroll/selection management (Viewport, ResolvedView)
│   ├── event.rs           # AppEvent enum (all possible state changes)
│   ├── history.rs         # Filter history persistence
│   ├── source.rs          # Source discovery, PID-based status tracking
│   ├── capture.rs         # Capture mode (-n flag): tee-like stdin to file
│   ├── watcher.rs         # File watching with notify crate
│   ├── dir_watcher.rs     # Directory watching for source discovery mode
│   ├── cache/             # Line caching for display optimization
│   │   ├── mod.rs
│   │   ├── line_cache.rs  # Caches expanded/wrapped lines for rendering
│   │   └── ansi_cache.rs  # ANSI color sequence caching
│   ├── reader/            # Log source abstraction
│   │   ├── mod.rs         # LogReader trait
│   │   ├── file_reader.rs # Sparse-indexed file access (default, O(1) seek)
│   │   ├── stream_reader.rs # Buffered stdin/pipe reading
│   │   ├── sparse_index.rs # Sparse line offset index (10k line intervals)
│   │   ├── mmap_reader.rs # Memory-mapped file reader (unused)
│   │   ├── huge_file_reader.rs # Large file handler (unused)
│   │   └── tail_buffer.rs # Ring buffer for tail mode (unused)
│   ├── filter/            # Filtering logic and engines
│   │   ├── mod.rs         # Filter trait, FilterMode enum, FilterHistoryEntry
│   │   ├── engine.rs      # FilterEngine: background thread filtering with progress reporting
│   │   ├── streaming_filter.rs # Direct file reading for fast path (SIMD byte search)
│   │   ├── string_filter.rs # Plain text matching (case-insensitive with simd find)
│   │   ├── regex_filter.rs # Regex matching via `regex` crate
│   │   ├── query.rs       # Query syntax parser (json | field > value, logfmt | ...)
│   │   ├── cancel.rs      # CancelToken for aborting in-flight filters
│   │   └── parallel_engine.rs # Parallel filtering (unused, rayon based)
│   ├── handlers/          # Event-to-state converters (pure functions, no mutation)
│   │   ├── mod.rs
│   │   ├── input.rs       # Keyboard/mouse input → AppEvent conversion
│   │   ├── filter.rs      # FilterProgress → AppEvent conversion
│   │   └── file_events.rs # File watcher events → AppEvent conversion
│   ├── ui/                # Terminal rendering
│   │   └── mod.rs         # ratatui UI code (side panel, log view, status bar, help)
│   └── mcp/               # Model Context Protocol server (optional feature)
│       ├── mod.rs         # MCP server setup and stdio transport
│       ├── tools.rs       # MCP tools: read_file, search_file
│       └── types.rs       # MCP type definitions
├── Cargo.toml             # Package manifest
├── Cargo.lock             # Dependency lock
├── CLAUDE.md              # Claude instructions (conventions, architecture notes)
├── .github/               # GitHub config (workflows, templates)
└── .planning/codebase/    # GSD codebase analysis documents
    ├── ARCHITECTURE.md    # This file
    └── STRUCTURE.md       # Directory structure and file locations
```

## Directory Purposes

**src/:**
- Purpose: All Rust source code
- Contains: Modules, main binary, library definitions
- Key files: `main.rs` (entry), `app.rs` (state), `event.rs` (events)

**src/reader/:**
- Purpose: Log source abstraction with multiple implementations
- Contains: `LogReader` trait and implementations for files/streams
- Key files: `mod.rs` (trait), `file_reader.rs` (default sparse-indexed impl), `stream_reader.rs` (pipes/stdin)

**src/filter/:**
- Purpose: Filtering engines and implementations
- Contains: Filter trait, mode definitions, background processing, query language
- Key files: `mod.rs` (Filter trait), `engine.rs` (background thread), `streaming_filter.rs` (fast path), `query.rs` (query syntax)

**src/handlers/:**
- Purpose: Event conversion (no state mutation)
- Contains: Input handlers, filter progress handlers, file event handlers
- Key files: `input.rs` (keyboard/mouse), `filter.rs` (filter progress), `file_events.rs` (file watcher)

**src/ui/:**
- Purpose: Terminal rendering with ratatui
- Contains: Layout logic, styling, text rendering
- Key files: `mod.rs` (all UI logic)

**src/cache/:**
- Purpose: Performance optimization for line display
- Contains: Caches for expanded lines and ANSI color sequences
- Key files: `line_cache.rs`, `ansi_cache.rs`

**src/mcp/:**
- Purpose: Model Context Protocol server for AI integration
- Contains: Server setup, tools (read_file, search_file), type definitions
- Key files: `mod.rs` (server), `tools.rs` (MCP tools), `types.rs` (schemas)

## Key File Locations

**Entry Points:**
- `src/main.rs`: Binary entry point with CLI parsing and main event loop

**Configuration:**
- `src/app.rs`: Application state structure and defaults
- `Cargo.toml`: Dependencies, feature flags, package metadata

**Core Logic:**
- `src/app.rs`: State mutations via `App::apply_event()`
- `src/tab.rs`: Per-tab state encapsulation
- `src/viewport.rs`: Vim-style viewport calculations

**Testing:**
- `src/*/mod.rs`, `src/*/*.rs`: Tests in `#[cfg(test)] mod tests` blocks
- No dedicated test directory; tests colocated with modules

**Data Persistence:**
- `src/history.rs`: Filter history saved to disk via serde_json
- `~/.config/lazytail/data/`: Captured log files (created by capture mode)
- `~/.config/lazytail/sources/`: PID marker files for active source tracking

## Naming Conventions

**Files:**
- Lowercase snake_case: `file_reader.rs`, `string_filter.rs`, `apply_selection_style`
- Module markers: `mod.rs` for directory modules
- Feature gates: `#[cfg(feature = "mcp")]` for optional modules

**Functions:**
- Lowercase snake_case: `handle_input_event()`, `trigger_filter()`, `apply_event()`
- Prefix style: Event handlers start with `handle_` (e.g., `handle_filter_input_mode()`)
- Factory pattern: `new()`, `with_*()` for constructors
- Query methods: `is_*()` for boolean returns, `get_*()` for owned returns

**Variables:**
- Lowercase snake_case: `active_tab`, `filter_receiver`, `scroll_position`
- Prefix for booleans: `is_loading`, `should_quit`, `has_filter`
- Prefix for optionals: `maybe_path`, `pending_filter_at`
- Postfix for collection indices: `tab_idx`, `line_indices`

**Types:**
- PascalCase: `App`, `TabState`, `FilterMode`, `AppEvent`
- Enum variants: PascalCase: `Active`, `Ended`, `Normal`, `Filtered`
- Traits: PascalCase: `Filter`, `LogReader`, `Backend`

**Constants:**
- SCREAMING_SNAKE_CASE: `DEFAULT_EDGE_PADDING`, `MAX_HISTORY_ENTRIES`, `FILTER_PROGRESS_INTERVAL`

## Where to Add New Code

**New Feature (e.g., bookmark lines):**
- Primary code: Add variant to `AppEvent` in `src/event.rs`
- Handler: Create `src/handlers/bookmarks.rs` or add to `src/handlers/input.rs`
- State: Add field to `TabState` in `src/tab.rs` or `App` in `src/app.rs`
- UI: Add rendering in `src/ui/mod.rs`
- Tests: Add `#[cfg(test)] mod tests` in relevant file

**New Filter Type (e.g., glob patterns):**
- Trait implementation: Create `src/filter/glob_filter.rs`, implement `Filter` trait
- Integration: Add variant to `FilterMode` or create new mode type
- Engine: Update `trigger_filter()` in `src/main.rs` to choose new filter type
- Tests: Colocate tests in `glob_filter.rs`

**New Reader Implementation (e.g., gzip files):**
- Trait implementation: Create `src/reader/gzip_reader.rs`, implement `LogReader` trait
- Integration: Update `TabState::new()` or add new constructor
- File detection: Add file type check in `src/tab.rs` before choosing reader
- Tests: Colocate tests in `gzip_reader.rs`

**New Command Mode (like capture/discovery):**
- Mode detection: Add condition in `fn main()` after `Args` parsing
- Handler: Create new function (e.g., `run_special_mode()`)
- State: Use existing `App` or create minimal state
- Integration: Return from CLI entry point with appropriate mode

**New Keyboard Command:**
- Handler: Add case to relevant function in `src/handlers/input.rs`
- Event: Add variant to `AppEvent` in `src/event.rs`
- State mutation: Add handler in `App::apply_event()` in `src/app.rs`
- UI feedback: Update status bar or help in `src/ui/mod.rs`

**New MCP Tool:**
- Tool definition: Add to `src/mcp/tools.rs`
- Type schema: Add to `src/mcp/types.rs`
- Implementation: Reference file reader or filter engine
- Server registration: Update MCP server in `src/mcp/mod.rs`

## Special Directories

**target/:**
- Purpose: Cargo build artifacts
- Generated: Yes (by cargo build)
- Committed: No

**~/.config/lazytail/data/:**
- Purpose: Captured log files (-n mode)
- Generated: Yes (by capture mode)
- Committed: No (user-specific)

**~/.config/lazytail/sources/:**
- Purpose: PID marker files for active source tracking
- Generated: Yes (by capture mode)
- Committed: No (user-specific)

## Module Dependency Graph

```
main.rs
├── app.rs (state)
├── event.rs (events)
├── handlers/ (event conversion)
│   ├── input.rs
│   ├── filter.rs
│   └── file_events.rs
├── ui/mod.rs (rendering)
├── reader/ (abstraction)
│   ├── file_reader.rs
│   ├── stream_reader.rs
│   └── ...
├── filter/ (filtering)
│   ├── engine.rs
│   ├── streaming_filter.rs
│   ├── string_filter.rs
│   ├── regex_filter.rs
│   └── query.rs
├── tab.rs (per-tab state)
├── viewport.rs (scroll/selection)
├── watcher.rs (file watching)
├── dir_watcher.rs (directory watching)
├── source.rs (discovery & status)
├── capture.rs (capture mode)
├── history.rs (persistence)
├── cache/ (optimization)
└── mcp/ (optional: MCP server)
```

---

*Structure analysis: 2026-02-03*
