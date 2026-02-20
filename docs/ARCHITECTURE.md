# LazyTail Architecture

## Overview

LazyTail is a terminal-based log viewer written in Rust. It provides live filtering, multi-tab viewing, source discovery, capture mode, and an MCP server for AI assistant integration.

The application operates in five distinct modes:

1. **TUI mode** - Interactive terminal UI for viewing log files and stdin
2. **Discovery mode** - Auto-discovers sources from project/global data directories
3. **Capture mode** (`-n`) - Tee-like stdin-to-file with source tracking
4. **MCP server mode** (`--mcp`) - Model Context Protocol server for programmatic log access
5. **Subcommand mode** - CLI subcommands (`init`, `config`, `update`)

## Module Map

```
src/
  main.rs           Core event loop, mode dispatch, CLI definition
  app.rs            Top-level application state (App struct)
  tab.rs            Per-tab state (TabState) with reader, watcher, filter
  viewport.rs       Vim-style viewport with anchor-based scrolling
  event.rs          AppEvent enum (all possible state transitions)
  signal.rs         Flag-based SIGINT/SIGTERM handling

  reader/
    mod.rs           LogReader trait
    file_reader.rs   Sparse-indexed file reader (O(1) memory)
    stream_reader.rs Stdin/pipe buffering reader
    sparse_index.rs  Sparse line offset index
    mmap_reader.rs   Memory-mapped reader (experimental)
    huge_file_reader.rs  Large file reader (experimental)
    tail_buffer.rs   Tail buffer (experimental)

  filter/
    mod.rs           Filter trait, FilterMode, FilterHistoryEntry
    engine.rs        FilterEngine - background thread coordination
    streaming_filter.rs  mmap + memchr grep-like filtering
    string_filter.rs Plain text substring filter
    regex_filter.rs  Regex-based filter
    query.rs         Structured query language (json/logfmt field filtering)
    cancel.rs        CancelToken for cooperative cancellation
    parallel_engine.rs  Parallel filtering (experimental)

  handlers/
    mod.rs           Handler module exports
    input.rs         Keyboard/mouse input -> AppEvent mapping
    filter.rs        FilterProgress -> AppEvent mapping
    file_events.rs   File modification -> AppEvent mapping

  ui/
    mod.rs           ratatui rendering (side panel, log view, status bar, help)

  config/
    mod.rs           Config module exports
    discovery.rs     lazytail.yaml discovery (parent dir walk)
    loader.rs        YAML config loading and merging
    types.rs         Config and Source types
    error.rs         Config error types

  source.rs         Source discovery, PID markers, data directory management
  capture.rs        Capture mode implementation
  watcher.rs        File change detection via notify/inotify
  dir_watcher.rs    Directory watcher for dynamic source discovery
  history.rs        Filter history persistence (~/.config/lazytail/history.json)
  cache/
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

  cmd/
    mod.rs           Subcommand definitions
    init.rs          `lazytail init` command
    config.rs        `lazytail config validate/show` commands
    update.rs        `lazytail update` command (feature-gated: self-update)
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
  tabs: Vec<TabState>         One per open file/source/pipe
  active_tab: usize           Index into tabs
  input_mode: InputMode       Normal, EnteringFilter, ZPending, SourcePanel, ConfirmClose
  input_buffer: String        Shared input for filter/line-jump
  filter_history: Vec<...>    Persistent across sessions
  source_panel: ...           Tree navigation state

TabState
  reader: Arc<Mutex<dyn LogReader>>   File or stream reader
  watcher: Option<FileWatcher>        inotify file watcher
  viewport: Viewport                  Scroll/selection state
  filter: FilterTabState              Active filter, cancel token, receiver
  line_indices: Vec<usize>            Current visible line indices
  mode: ViewMode                      Normal or Filtered
  follow_mode: bool                   Auto-scroll on new content
  expansion: ExpansionState           Expanded/collapsed lines
  source_status: Option<SourceStatus> Active/Ended for discovered sources
```

Each tab is fully independent with its own reader, watcher, viewport, and filter state.

### Filter Pipeline

Filtering runs in background threads to keep the UI responsive:

```
User types pattern
  -> debounce (500ms)
  -> cancel previous filter (CancelToken)
  -> choose filter strategy:
       File + plain text:  streaming_filter_fast (mmap + SIMD memmem)
       File + regex:       streaming_filter (mmap + per-line regex)
       File + query:       streaming_filter (mmap + JSON/logfmt parsing)
       Stdin:              FilterEngine (shared reader with Mutex)
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

### MCP Server

Feature-gated behind `mcp` (enabled by default). Uses `rmcp` crate with tokio runtime and stdio transport.

Provides 6 tools:
- `list_sources` - discover available log sources
- `search` - pattern search with regex/plain text and structured queries
- `get_lines` - read specific line ranges
- `get_tail` - fetch recent lines
- `get_context` - get lines around a specific line number
- `get_stats` - index metadata and severity breakdown

See [ADR-010: MCP Server Integration](adr/010-mcp-server.md).

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
