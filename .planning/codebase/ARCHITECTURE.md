# Architecture

**Analysis Date:** 2026-02-03

## Pattern Overview

**Overall:** Event-driven render-collect-process loop with multi-threaded filtering backend and pluggable reader abstraction.

**Key Characteristics:**
- Central event hub (`App::apply_event()`) in `src/app.rs` processes all state changes
- Non-blocking background threads for filtering and file watching
- Vim-style viewport with anchor-based selection/scrolling
- Trait-based reader abstraction supporting both file access and streaming inputs
- Channel-based progress reporting for long-running operations

## Layers

**Presentation Layer:**
- Purpose: Render the terminal UI and collect user input
- Location: `src/ui/mod.rs`, `src/handlers/input.rs`
- Contains: ratatui rendering logic, input event handlers
- Depends on: `App` state for rendering data, crossterm for terminal control
- Used by: main loop in `src/main.rs`

**State Management Layer:**
- Purpose: Hold mutable application state and apply events
- Location: `src/app.rs`, `src/tab.rs`
- Contains: `App` (global state), `TabState` (per-tab state), `InputMode`, `ViewMode`, filter history
- Depends on: readers, watchers, viewport for state
- Used by: main event loop, event handlers

**Event Flow Layer:**
- Purpose: Dispatch events without mutating state directly
- Location: `src/event.rs`, `src/handlers/` (input, file_events, filter)
- Contains: `AppEvent` enum defining all state changes, specialized handlers for each event category
- Depends on: App/Tab state for context (read-only), event definitions
- Used by: main loop collects events, processes_event() applies them

**Backend Worker Layer:**
- Purpose: Perform I/O and heavy computation in background threads
- Location: `src/filter/`, `src/reader/`, `src/watcher.rs`, `src/dir_watcher.rs`
- Contains: `FilterEngine`, `StreamingFilter`, file watchers, directory watcher
- Depends on: cancellation tokens for synchronization
- Used by: main loop collects results from channels

**Data Access Layer:**
- Purpose: Abstract different log source types with unified interface
- Location: `src/reader/mod.rs` (trait), implementations in `src/reader/*.rs`
- Contains: `LogReader` trait, `FileReader` (sparse-indexed), `StreamReader` (buffering), deprecated readers
- Depends on: file system, stdio
- Used by: filters, UI for reading lines

## Data Flow

**User Input → State Update:**

1. User presses key
2. `crossterm::event::read()` blocks until event available
3. `collect_input_events()` coalesces mouse scroll and reads key
4. `handlers::input::handle_input_event()` converts to `AppEvent` (no state mutation)
5. `process_event()` applies event via `App::apply_event()`
6. If filter-related: schedule debounced filter with `pending_filter_at`
7. Next loop cycle triggers `trigger_filter()` → spawns `FilterEngine` thread
8. Filter thread sends progress via channel, main loop collects via `collect_filter_progress()`
9. `handlers::filter::handle_filter_progress()` converts to `FilterComplete` event
10. Event applied to state, UI re-renders with new filtered results

**File Modification → Display Update:**

1. `notify` crate detects file change (via inotify on Linux)
2. `watcher::FileWatcher::try_recv()` returns `FileEvent::Modified`
3. `collect_file_events()` calls `reader.reload()` to re-index file
4. If file grew and active tab: generates `FileModified` event with new total lines
5. If follow mode enabled: `App` jumps to end
6. If active filter exists: triggers incremental `FilterEngine` with only new lines

**Stream Data (stdin/pipe) → Display Update:**

1. `TabState::from_stdin()` spawns background thread with `StreamReader`
2. Thread reads batches from stdin via `stream_receiver`
3. Thread sends `StreamMessage::Lines` via channel
4. `collect_stream_events()` receives in main loop, calls `tab.append_stream_lines()`
5. Adds lines to reader and updates total_lines
6. If filter active: incremental filter on new batch

**State Management:**

State flows through `App` → `TabState` → `Viewport` + filter state + reader:
- `App::active_tab` controls which tab is displayed
- `TabState::line_indices` holds filtered or full line numbers
- `Viewport` manages scroll position and selection anchor (stable across filter changes)
- `TabState::reader` locked during filter operations (filter thread holds lock for duration)

## Key Abstractions

**LogReader Trait:**
- Purpose: Unified interface for different line source types
- Examples: `FileReader` (sparse-indexed random access), `StreamReader` (buffered streaming)
- Pattern: Mock-friendly interface with `total_lines()`, `get_line()`, `reload()` methods
- Location: `src/reader/mod.rs`

**Filter Trait:**
- Purpose: Extensible filtering logic (string, regex, query)
- Examples: `StringFilter`, `RegexFilter`, `QueryFilter`
- Pattern: Single method `matches(&str) -> bool` for maximum flexibility
- Location: `src/filter/mod.rs`

**Viewport:**
- Purpose: Manage selection and scroll with vim-style behavior (anchor-based, scrolloff padding)
- Examples: Used in every `TabState` to resolve display position from filtered line indices
- Pattern: Stable anchor line provides UI consistency during filtering
- Location: `src/viewport.rs`

**TabState:**
- Purpose: Encapsulate per-source state (reader, filter, viewport, expansion)
- Examples: `from_stdin()` for pipes, `new()` for files, `from_discovered_source()` for capture sources
- Pattern: Independent state allows multi-tab with isolated filters
- Location: `src/tab.rs`

## Entry Points

**Main Binary Entry:**
- Location: `src/main.rs` (fn main())
- Triggers: CLI argument parsing, mode detection (normal/capture/discovery/MCP)
- Responsibilities: Terminal setup, tab creation, main loop invocation

**Event Loop:**
- Location: `src/main.rs` (fn run_app_with_discovery())
- Triggers: Continuously until `app.should_quit = true`
- Responsibilities:
  1. Render current state
  2. Collect events from file watchers, filters, stdin, keyboard/mouse
  3. Apply events to state
  4. Check for debounced filters

**Filter Start:**
- Location: `src/main.rs` (fn trigger_filter())
- Triggers: User submits filter pattern or typing debounce expires
- Responsibilities:
  1. Cancel any in-flight filter
  2. Detect query syntax vs plain/regex
  3. Choose streaming filter (for files) or generic filter (for pipes/stdin)
  4. Spawn background thread, return receiver

**File Reload:**
- Location: `src/main.rs` (fn collect_file_events())
- Triggers: File watcher detects modification
- Responsibilities:
  1. Call `reader.reload()` to re-index
  2. Detect growth or truncation
  3. For active tab: trigger incremental filter if needed
  4. For inactive tab: apply updates directly

## Error Handling

**Strategy:** Graceful degradation with error logging. No panic on recoverable errors.

**Patterns:**

- **File I/O errors:** Logged to stderr, filter continues or shows empty results
- **Filter regex compilation:** Validation in input handler, `regex_error` field displays error
- **Reader lock contention:** Timeout in filter thread catches poisoned locks, sends error event
- **Watcher setup:** Optional (`ok()` discarded), tab works without watching
- **Stream read errors:** Logged, stream marked complete, tab remains viewable

**Flow example (filter error):**
```rust
// In FilterEngine thread
match reader.get_line(i) {
    Ok(Some(line)) => { /* process */ }
    Ok(None) => { /* line missing */ }
    Err(e) => {
        tx.send(FilterProgress::Error(e.to_string()));
        return;
    }
}
```

## Cross-Cutting Concerns

**Logging:**
- Pattern: Direct `eprintln!()` to stderr (no structured logging)
- Used for: Watcher errors, filter errors, stream errors
- Location: Scattered throughout handlers and main loop

**Validation:**
- Input validation: In handlers before state application
- Regex validation: `RegexFilter::new()` fails gracefully, error stored in `app.regex_error`
- Query validation: `query::parse_query()` and `QueryFilter::new()` both fail

**Authentication:**
- Not applicable (file-based, single-user TUI)

**Cancellation:**
- Pattern: `CancelToken` (Arc<AtomicBool>) checked in filter inner loops
- Used for: Aborting long-running filters when user types new filter or presses Esc
- Location: `src/filter/cancel.rs`

**Concurrency:**
- Reader: `Arc<Mutex<dyn LogReader>>` for shared access between UI and filter threads
- Channels: `crossterm::event` (OS integration), `std::sync::mpsc` (custom channels)
- Threads: One-off spawns for filters, one long-lived for stdin readers per tab

---

*Architecture analysis: 2026-02-03*
