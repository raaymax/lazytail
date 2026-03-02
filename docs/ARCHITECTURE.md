# LazyTail Architecture

## Overview

LazyTail is a terminal-based log viewer written in Rust. It provides live filtering, multi-tab viewing, source discovery, capture mode, a web UI, and an MCP server for AI assistant integration.

The application operates in six distinct modes:

1. **TUI mode** — Interactive terminal UI for viewing log files and stdin
2. **Web mode** (`web`) — HTTP server with browser-based log viewing
3. **Discovery mode** — Auto-discovers sources from project/global data directories
4. **Capture mode** (`-n`) — Tee-like stdin-to-file with source tracking
5. **MCP server mode** (`--mcp`) — Model Context Protocol server for programmatic log access
6. **Subcommand mode** — CLI subcommands (`init`, `config`, `bench`, `theme`)

## Module Map

```
src/
  main.rs           Core event loop, mode dispatch, CLI definition (~1200 lines)
  lib.rs            Library crate interface (config, index, source, theme)
  ansi.rs           ANSI escape sequence stripping
  log_source.rs     Domain-only source state (LogSource, FilterConfig, LineRateTracker)
  signal.rs         Flag-based SIGINT/SIGTERM handling
  source.rs         Source discovery, PID markers, data directory management
  capture.rs        Capture mode implementation
  history.rs        Filter history persistence (~/.config/lazytail/history.json)
  session.rs        Session persistence (last-opened source per project)

  app/
    mod.rs           Top-level application state (App struct, InputMode, ViewMode)
    tab.rs           Per-tab TUI state (TabState) wrapping LogSource
    viewport.rs      Vim-style viewport with anchor-based scrolling
    event.rs         AppEvent enum (all possible state transitions)

  reader/
    mod.rs           LogReader trait
    file_reader.rs   Sparse-indexed file reader (O(1) memory)
    stream_reader.rs Stdin/pipe buffering reader
    combined_reader.rs Multi-source chronological merging via index timestamps
    sparse_index.rs  Sparse line offset index
    mmap_reader.rs   Memory-mapped reader (experimental)
    huge_file_reader.rs  Large file reader (experimental)
    tail_buffer.rs   Tail buffer (experimental)

  filter/
    mod.rs           Filter trait, FilterMode, FilterHistoryEntry
    orchestrator.rs  FilterOrchestrator — unified filter dispatch entry point
    search_engine.rs SearchEngine — stateless search dispatch (picks fastest path)
    engine.rs        FilterEngine — background thread coordination
    streaming_filter.rs  mmap + memchr grep-like filtering
    string_filter.rs Plain text substring filter
    regex_filter.rs  Regex-based filter
    query.rs         Structured query language (json/logfmt field filtering)
    aggregation.rs   Grouped query results (count by field, top N)
    cancel.rs        CancelToken for cooperative cancellation
    parallel_engine.rs  Parallel filtering (experimental)

  handlers/
    mod.rs           Handler module exports
    input.rs         Keyboard/mouse input -> AppEvent mapping
    filter.rs        FilterProgress -> AppEvent mapping
    file_events.rs   File modification -> AppEvent mapping

  renderer/
    mod.rs           PresetRegistry — compiled rendering presets
    preset.rs        Compiled preset types
    detect.rs        Auto-detection of log format
    field.rs         Field extraction from log lines
    format.rs        Segment formatting
    segment.rs       Styled segment types
    builtin.rs       Built-in preset definitions

  theme/
    mod.rs           Theme struct, color parsing
    loader.rs        YAML theme loading, multi-format import

  tui/
    mod.rs           ratatui rendering coordinator
    log_view.rs      Main log content rendering
    side_panel.rs    Source tree panel
    status_bar.rs    Status bar rendering
    help.rs          Help overlay (keyboard shortcuts)
    aggregation_view.rs  Aggregation result rendering

  watcher/
    mod.rs           Watcher module exports
    file.rs          File change detection via notify/inotify
    dir.rs           Directory watcher for dynamic source discovery

  web/
    mod.rs           HTTP server with embedded SPA for browser-based log viewing
    index.html       Embedded single-page application

  index/
    mod.rs           Columnar index module exports
    builder.rs       Index writer (flags, offsets, checkpoints)
    reader.rs        IndexReader — read-only access to flags and checkpoints
    column.rs        ColumnReader/ColumnWriter for typed columnar storage
    checkpoint.rs    Checkpoint and SeverityCounts types
    flags.rs         Severity enum, format flags, line classification
    meta.rs          Index metadata (entry count, column bits)
    lock.rs          Advisory flock-based write lock

  config/
    mod.rs           Config module exports
    discovery.rs     lazytail.yaml discovery (parent dir walk)
    loader.rs        YAML config loading and merging
    types.rs         Config and Source types
    error.rs         Config error types

  cli/
    mod.rs           Subcommand definitions
    init.rs          `lazytail init` command
    bench.rs         `lazytail bench` — filter performance benchmarking
    config.rs        `lazytail config validate/show` commands
    theme.rs         `lazytail theme import/list` commands
    update.rs        `lazytail update` command (feature-gated: self-update)

  cache/
    mod.rs           Cache module
    line_cache.rs    LRU cache for line content
    ansi_cache.rs    LRU cache for parsed ANSI sequences

  mcp/              (feature-gated: "mcp")
    mod.rs           MCP server entry point (tokio + rmcp)
    tools.rs         6 MCP tools implementation
    types.rs         MCP request/response types
    format.rs        Output formatting for MCP responses
    ansi.rs          ANSI stripping for MCP output

  update/            (feature-gated: "self-update")
    mod.rs           Types, cache I/O, version comparison
    checker.rs       GitHub release checking with 24h cache
    installer.rs     Binary download and replacement
    detection.rs     Package manager detection (pacman/dpkg/brew/path)
```

## Core Architecture

### Event-Driven Loop

The application follows a **render-collect-process** cycle in `main.rs`:

```
loop {
    1. Render current state (ratatui)
    2. Check debounced filter triggers
    3. Refresh source status for discovered sources
    4. Check directory watcher for new sources
    5. Collect events from:
       - File watchers (notify/inotify)
       - Filter progress channels
       - Stream readers (stdin/pipe)
       - User input (keyboard/mouse via crossterm)
    6. Process events through App::apply_event()
    7. Handle side effects (trigger filters, follow mode jumps)
}
```

Input handlers (`handlers/`) never mutate `App` directly. They return `Vec<AppEvent>` which are processed centrally. This separation ensures predictable state transitions and makes the system testable.

See [ADR-001: Event-Driven Architecture](adr/001-event-driven-architecture.md).

### State Hierarchy

```
App
  tabs: Vec<TabState>           One per open file/source/pipe
  combined_tabs: [Option; 5]    Per-category combined ($all) tabs
  active_tab: usize             Index into tabs
  active_combined: Option<...>  Which combined tab is active
  input_mode: InputMode         Normal, EnteringFilter, EnteringLineJump,
                                ZPending, SourcePanel, ConfirmClose
  input_buffer: String          Shared input for filter/line-jump
  filter_history: Vec<...>      Persistent across sessions
  source_panel: ...             Tree navigation state
  preset_registry: Arc<...>     Compiled rendering presets
  theme: Theme                  UI color scheme
  source_renderer_map: ...      Source name → renderer preset names
  pending_filter_at: Option<..> Debounced filter trigger time

TabState                              TUI adapter state
  source: LogSource              Domain core (shared across adapters)
  viewport: Viewport                  Scroll/selection state
  expansion: ExpansionState           Expanded/collapsed lines
  watcher: Option<FileWatcher>        inotify file watcher
  is_combined: bool                   Whether this is a combined view
  stream_writer: ...                  Stdin background reader handle
  stream_receiver: ...                Stdin message channel
  config_source_type: Option<...>     Source type from config
  aggregation_view: ...               Aggregation table navigation state

LogSource                        Domain-only state (log_source.rs)
  reader: Arc<Mutex<dyn LogReader>>   File or stream reader
  index_reader: Option<IndexReader>   Columnar index (severity, flags)
  filter: FilterConfig                Active filter, cancel token, receiver
  line_indices: Vec<usize>            Current visible line indices
  total_lines: usize                  Total lines in source
  mode: ViewMode                      Normal, Filtered, or Aggregation
  follow_mode: bool                   Auto-scroll on new content
  source_path: Option<PathBuf>        File path (None for stdin)
  source_status: Option<SourceStatus> Active/Ended for discovered sources
  rate_tracker: LineRateTracker       Sliding-window ingestion rate
  aggregation_result: Option<...>     Grouped query result (count by)
  renderer_names: Vec<String>         Renderer preset names for this source
```

Each tab is fully independent. Domain state lives in `LogSource` which is shared by TUI, Web, and MCP adapters. Adapter-specific state (viewport, expansion, watchers) stays on `TabState`.

See [ADR-014: Hexagonal Architecture — LogSource Extraction](adr/014-hexagonal-log-source-state.md).

### Filter Pipeline

Filtering runs in background threads to keep the UI responsive. `FilterOrchestrator` is the entry point for TUI and Web adapters, and delegates search dispatch to `SearchEngine` (shared with MCP):

```
User types pattern (TUI) / POST /api/filter (Web) / MCP search
  -> FilterOrchestrator::trigger(&mut source, pattern, mode, range)
  -> cancel previous filter (CancelToken)
  -> detect filter type:
       Query syntax (json | ...):  parse FilterQuery AST, optional index acceleration
       Plain text:                 StringFilter
       Regex:                      RegexFilter
  -> choose execution backend:
       File + plain text (full):   streaming_filter_fast (mmap + SIMD memmem)
       File + regex/query (full):  streaming_filter (mmap + per-line matching)
       File + any (incremental):   streaming_filter_range (byte offset from index)
       Stdin + any:                FilterEngine (shared reader with Mutex)
       Index available + query:    candidate_bitmap pre-filter -> streaming_filter_indexed
  -> background thread sends FilterProgress via channel:
       PartialResults { matches, lines_processed }  (batched, every 50k lines)
       Complete { matches, lines_processed }
       Error(String)
  -> main loop collects via try_recv()
  -> App::apply_event() merges results into line_indices
```

For files, the streaming filter uses **mmap + memchr** for grep-like performance. Plain text case-sensitive search uses a grep-style algorithm: find pattern occurrences first, then lazily determine line numbers only near matches.

See [ADR-002: mmap-Based Streaming Filter](adr/002-mmap-streaming-filter.md).

### File Reading

`FileReader` uses **sparse indexing** to provide memory-efficient random access:

- Stores byte offsets for every 10,000th line (not every line)
- To read line N: seek to nearest indexed position, scan forward
- Memory: ~120KB for 100M lines (vs ~800MB with full indexing)

See [ADR-003: Sparse Indexing for File Reader](adr/003-sparse-index-file-reader.md).

### Viewport

The `Viewport` struct implements vim-style scrolling:

- **Anchor-based**: tracks a file line number, stable across filter changes
- **Edge padding** (scrolloff=3): selection stays within comfort zone
- **Binary search resolution**: on filter change, finds nearest line to anchor
- Supports: `j/k` (move), `Ctrl+E/Y` (viewport scroll), `zz/zt/zb` (position), `G/gg` (jump)

See [ADR-005: Vim-Style Viewport Navigation](adr/005-vim-style-viewport.md).

### Source Discovery and Capture

LazyTail uses a two-tier storage model:

- **Project-local**: `.lazytail/data/` and `.lazytail/sources/` (next to `lazytail.yaml`)
- **Global**: `~/.config/lazytail/data/` and `~/.config/lazytail/sources/`

**Capture mode** (`cmd | lazytail -n NAME`):
1. Validates source name
2. Creates PID marker file in `sources/` directory
3. Sets up signal handlers for cleanup
4. Tee loop: reads stdin, writes to `data/NAME.log`, echoes to stdout
5. Removes marker on exit (EOF or signal)

**Discovery mode** (`lazytail` with no args):
1. Walks parent directories looking for `lazytail.yaml`
2. Scans project and global data directories for `.log` files
3. Checks PID markers to determine Active/Ended status
4. Opens directory watcher for live source appearance

Project sources shadow global sources with the same name.

See [ADR-006: PID-Based Source Tracking](adr/006-pid-source-tracking.md) and [ADR-007: Config Discovery](adr/007-config-discovery.md).

### Signal Handling

Uses `signal-hook::flag` for safe, flag-based signal handling:

- First SIGINT/SIGTERM: sets `AtomicBool` flag, main loop checks and performs cleanup
- Second signal while flag is set: immediate exit (code 1)
- No `process::exit()` in signal handler, ensuring destructors run

See [ADR-008: Flag-Based Signal Handling](adr/008-flag-based-signals.md).

### Web Server

The `lazytail web` subcommand starts an HTTP server with an embedded single-page application. It reuses `LogSource` and `FilterOrchestrator` from the core domain, adding only web-specific concerns (HTTP routing, JSON serialization, SSE polling).

Key endpoints:
- `GET /api/sources` - list sources with severity counts and filter state
- `GET /api/lines` - paginated line content with per-line severity
- `GET /api/events` - long-polling for state changes (25-second timeout)
- `POST /api/filter` - trigger filter via `FilterOrchestrator::trigger`
- `POST /api/filter/clear` - cancel and clear filter
- `POST /api/follow` - toggle follow mode
- `POST /api/source/close` - close a source tab

The web adapter uses the same `TabState` as TUI for file watching and filter progress polling, but only consumes the `LogSource` portion for API responses. Severity data flows from `IndexReader` through to JSON responses automatically.

### MCP Server

Feature-gated behind `mcp` (enabled by default). Uses `rmcp` crate with tokio runtime and stdio transport.

Provides 6 tools:
- `list_sources` - discover available log sources
- `search` - pattern search with regex/plain text and structured queries
- `get_lines` - read specific line ranges with per-line severity
- `get_tail` - fetch recent lines with per-line severity
- `get_context` - get lines around a specific line number with per-line severity
- `get_stats` - columnar index statistics including severity counts

When a columnar index is available, `LineInfo` responses include a `severity` field (trace/debug/info/warn/error/fatal). Text output format: `[L{n}] [{severity}] {content}`.

See [ADR-010: MCP Server Integration](adr/010-mcp-server.md).

### Renderer / Preset System

The renderer system (`src/renderer/`) provides configurable structured log formatting. Presets define how log lines are parsed and displayed:

1. **Preset compilation**: Raw YAML definitions (`lazytail.yaml` `renderers:` section) are compiled into `CompiledPreset` at startup
2. **Auto-detection**: Each preset includes detection patterns (regex or string match) to automatically select the right preset for a log line
3. **Field extraction**: Parsers extract named fields from JSON, logfmt, or regex-captured groups
4. **Segment formatting**: Extracted fields are arranged into styled segments (color, alignment, truncation)
5. **Builtin presets**: Default presets for common log formats ship with the binary

The `PresetRegistry` holds all compiled presets (user definitions take priority over builtins). Each source can have explicit renderer names assigned via config, or presets are auto-detected per line.

### Theme System

The theme system (`src/theme/`) provides color scheme support:

- **Theme struct**: Maps UI elements to ratatui colors (foreground, background, highlight, severity levels, etc.)
- **YAML themes**: Themes are defined as YAML files and stored in `~/.config/lazytail/themes/` or project-local `.lazytail/themes/`
- **Multi-format import**: `lazytail theme import` converts color schemes from Windows Terminal (.json), Alacritty (.toml), Ghostty (.conf), and iTerm2 (.itermcolors) into LazyTail's YAML format
- **Theme resolution**: Project theme > global theme > built-in default
- **Color parsing**: Supports named colors, `#rrggbb` hex, `#rgb` shorthand, and `"default"` for terminal default

### SearchEngine

`SearchEngine` (`src/filter/search_engine.rs`) is the unified, stateless search dispatch module. Both `FilterOrchestrator` (TUI) and MCP converge here, eliminating duplicated index-acceleration logic.

Given a filter, path, optional index, and optional range, `SearchEngine` picks the fastest execution path:
- **File + plain text (full scan)**: `streaming_filter_fast` (mmap + SIMD memmem)
- **File + regex/query (full scan)**: `streaming_filter` (mmap + per-line matching)
- **File + index available + query**: `candidate_bitmap` pre-filter → `streaming_filter_indexed`
- **File + any (incremental)**: `streaming_filter_range` (byte offset from index)
- **Stdin + any**: `FilterEngine` (shared reader with Mutex)

All functions are stateless and return `Result<Receiver<FilterProgress>>`.

### Aggregation

The aggregation system (`src/filter/aggregation.rs`) computes grouped counts from query results:

- Triggered by `count by (field1, field2)` syntax in the query language
- Optional `top N` limiting for large cardinality fields
- Each `AggregationGroup` contains key-value pairs, count, and source line indices
- Drill-down: selecting a group switches to a filtered view of its constituent lines
- UI rendered via `tui/aggregation_view.rs` as a navigable list

### Session Persistence

`session.rs` remembers the last-opened source per project context:

- Stores per-project entries in `~/.config/lazytail/session.json`
- On launch, restores the previously active source (selects its tab)
- Uses the project root path as context key (or a global key for non-project usage)
- Caps stored entries at 100 to prevent unbounded growth

### Combined / Merged View

`CombinedReader` (`src/reader/combined_reader.rs`) merges lines from multiple sources in chronological order:

- Implements `LogReader` so it works transparently with the existing rendering and filter infrastructure
- Uses timestamps from the columnar index to interleave lines
- Each `MergedLine` references a source ID and file line number
- Tabs with `is_combined: true` use a `CombinedReader` instead of a single-file reader

## Key Dependencies

| Crate | Purpose |
|-------|---------|
| `ratatui` | Terminal UI framework |
| `crossterm` | Terminal input/output |
| `notify` | File system watching (inotify/kqueue) |
| `memmap2` | Memory-mapped file access for filtering |
| `memchr` | SIMD-accelerated byte search |
| `regex` | Regular expression filtering |
| `serde` / `serde_json` | Serialization for config, history, JSON logs |
| `serde-saphyr` | YAML parsing for lazytail.yaml |
| `clap` | CLI argument parsing |
| `signal-hook` | Flag-based signal handling |
| `lru` | LRU cache for line content and ANSI parsing |
| `rayon` | Parallel iteration (experimental) |
| `tiny_http` | Lightweight HTTP server for web mode |
| `colored` | CLI colored output |
| `strsim` | Fuzzy matching for typo suggestions |
| `xxhash-rust` | Content hashing |
| `unicode-width` | Text width calculation |
| `libc` | Unix-specific operations (flock) |
| `tokio` | Async runtime for MCP server (optional) |
| `rmcp` | MCP protocol implementation (optional) |
| `self_update` | GitHub release checking and binary replacement (optional) |

## Data Flow Diagrams

### File Viewing

```
File on disk
  -> FileWatcher (notify/inotify) detects changes
  -> FileReader.reload() rebuilds sparse index
  -> AppEvent::FileModified { new_total, old_total }
  -> If filtered: trigger incremental filter on new lines only
  -> If follow_mode: jump to end
  -> Render via ratatui
```

### Live Filter Preview

```
User types in filter input
  -> AppEvent::FilterInputChar(c)
  -> App updates input_buffer
  -> Cancel any in-progress filter (CancelToken)
  -> Schedule debounced filter (500ms)
  -> After debounce: trigger_live_filter_preview()
  -> Background thread sends PartialResults
  -> UI shows results incrementally
  -> On submit (Enter): final filter, add to history
  -> On cancel (Esc): restore original position via filter.origin_line
```

### Tab Close Flow

```
User presses 'x' or Ctrl+W
  -> AppEvent::CloseCurrentTab
  -> App stores (tab_index, tab_name) in pending_close_tab
  -> InputMode::ConfirmClose (shows dialog)
  -> User presses 'y': AppEvent::ConfirmCloseTab
     -> Verifies tab still matches by name (guards against reordering)
     -> If Ended discovered source: deletes source file
     -> Removes tab, adjusts active_tab
  -> User presses 'n'/Esc: AppEvent::CancelCloseTab
     -> Restores previous InputMode
```
