# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build Commands

```bash
cargo build                       # Debug build
cargo build --release             # Optimized release build
cargo test                        # Run fast tests only
cargo test -- --include-ignored   # Run all tests including slow ones
cargo test -- --ignored           # Run only slow/integration tests
cargo clippy                      # Lint checks
cargo fmt                         # Format code
cargo fmt -- --check              # Check formatting
cargo run -- test.log             # Run with a log file
```

Tests marked with `#[ignore]` are slow integration tests involving file system operations.

## Architecture

LazyTail is a terminal-based log viewer written in Rust using ratatui for the TUI.

### Event-Driven Core Loop (main.rs)

The application follows a render-collect-process cycle:
1. Render current state with ratatui
2. Check pending debounced filters, refresh source status, check directory watcher
3. Collect events from: file watcher, filter threads, user input
4. Process events via `App::apply_event()` to update state (all side-effects handled there)
5. Repeat until quit

`main.rs` (~1200 lines) handles terminal setup, event collection, and delegates all state changes to `App::apply_event()`. The `process_event()` function is a single-line passthrough.

### State Structure

- **App** (app/mod.rs): Top-level state decomposed into sub-controllers: `TabManager` (tabs/combined views), `InputController` (mode/buffer/cursor), `FilterController` (validation/debouncing/history), `SourcePanelController` (source tree), plus preset registry and theme
- **TabState** (app/tab.rs): Per-tab state including source, watcher, viewport, expansion
- **LogSource** (log_source.rs): Domain state for a log source — reader, index, filter config, line indices, rate tracker, aggregation result, renderer names, view modes (raw/wrap/timestamps). Shared across TUI/Web/MCP adapters.
- **Viewport** (app/viewport.rs): Vim-style scrolling with selection anchor and scrolloff padding

### Key Modules

- **app/**: Sub-modules: `event.rs` (AppEvent enum), `filter_controller.rs` (FilterController — debounce, validation, history), `input_controller.rs` (InputController, InputMode), `source_panel.rs` (SourcePanelController — tree navigation), `tab_manager.rs` (TabManager — tab collection, combined views, active tab)
- **reader/**: `LogReader` trait (4 methods: `total_lines`, `get_line`, `reload`, `as_any`) with `FileReader` (lazy O(1) line access via sparse index, lossy UTF-8 for binary tolerance), `StreamReader` (stdin buffering), and `CombinedReader` (multi-source chronological merging via index timestamps). `StreamableReader` trait extends `LogReader` with stream-specific methods (`append_lines`, `mark_complete`, `is_loading`) — only `StreamReader` implements it. This follows ISP: `FileReader` only implements `LogReader`.
- **filter/**: `Filter` trait with `StringFilter` and `RegexFilter`. `FilterEngine` runs filtering in background thread, sends progress via channel. `streaming_filter` provides mmap-based grep-like performance. `query/` directory implements field-based filtering (JSON/logfmt with text parser for `json | level == "error"` syntax) — `ast.rs` (FilterQuery AST), `parser.rs` (text parser), `filter.rs` (QueryFilter), `time.rs` (@ts time-based filtering). `search_engine` provides `SearchEngine` — stateless unified search dispatch (picks fastest execution path based on filter type, index, and range). `aggregation` provides grouped query results (`count by (field)` with `top N`).
- **filter_orchestrator.rs**: `FilterOrchestrator` — the unified entry point for all filter trigger paths (top-level module, not inside `filter/`)
- **handlers/**: Input, filter progress, and file event handlers
- **tui/**: ratatui rendering — `log_view.rs` (main log content), `side_panel.rs` (source tree), `status_bar.rs`, `help.rs` (keyboard shortcut overlay), `aggregation_view.rs` (grouped query results)
- **watcher/**: File watching (`file.rs` via notify/inotify) and directory watching (`dir.rs` for dynamic source discovery)
- **source.rs**: Source discovery, marker files (PID-based active/ended tracking), stale marker cleanup
- **log_source.rs**: `LogSource` struct (domain state shared across TUI/Web/MCP), `FilterConfig`, `LineRateTracker` (sliding-window ingestion rate)
- **capture.rs**: Capture mode (`-n` flag) — tee-like stdin-to-file with signal handling and incremental indexing
- **config/**: Config system — `discovery.rs` (lazytail.yaml walk parent dirs), `loader.rs` (YAML loading), `types.rs` (config structs), `error.rs`. Project-scoped vs global
- **cli/**: CLI subcommand definitions — `init.rs`, `config.rs`, `bench.rs` (filter performance benchmarking), `theme.rs` (theme import/list), `update.rs` (feature-gated: self-update)
- **renderer/**: Rendering preset system for structured log lines — `preset.rs` (compiled presets), `detect.rs` (auto-detection), `field.rs` (field extraction), `format.rs` (segment formatting), `segment.rs` (styled segments), `builtin.rs` (built-in presets)
- **theme/**: Color scheme support — `mod.rs` (color parsing, theme struct), `loader.rs` (YAML loading, import from Windows Terminal/Alacritty/Ghostty/iTerm2)
- **signal.rs**: Flag-based signal handling for SIGINT/SIGTERM (no `process::exit` in handler)
- **history.rs**: Filter history persistence to disk (`~/.config/lazytail/history.json`)
- **session.rs**: Session persistence — remembers last-opened source per project context
- **ansi.rs**: ANSI escape sequence stripping (regex-based)
- **parsing.rs**: Logfmt parser (`parse_logfmt`)
- **text_wrap.rs**: Line wrapping logic for TUI display
- **lib.rs**: Library crate interface exposing `config`, `filter`, `index`, `parsing`, `reader`, `renderer`, `source`, `text_wrap`, `theme`
- **index/**: Columnar index system — `builder.rs`, `reader.rs`, `column.rs`, `checkpoint.rs`, `flags.rs`, `meta.rs`, `lock.rs` (advisory flock-based write lock), `validate.rs` (index integrity verification with partial trust)
- **mcp/**: MCP server for AI assistant integration — 6 tools (list_sources, search, get_lines, get_tail, get_context, get_stats). `tools/` subdirectory with `context.rs`, `lines.rs`, `search.rs`, `stats.rs`, `response.rs`. Also `format.rs`, `types.rs`, `ansi.rs`
- **web/**: HTTP server with embedded SPA for browser-based log viewing (`lazytail web`) — `handlers.rs`, `state.rs`, `index.html`
- **update/**: Self-update feature (feature-gated: `self-update`) — GitHub release checking with 24h cache, binary installer, package manager detection (pacman/dpkg/brew), nightly build support

### Filter Flow

1. User enters pattern in filter input mode (debounced live preview via `FilterController::schedule_debounce()`)
2. `FilterOrchestrator::trigger()` detects filter type (plain/regex/query) and dispatches:
   - File sources: `streaming_filter` (mmap-based, SIMD for plain text)
   - Stdin sources: `FilterEngine` (reader-based)
   - Query syntax (`json | ...`): parsed via shared `FilterQuery` AST → `QueryFilter`
3. Background thread sends `FilterProgress` updates through channel
4. `App::apply_event()` receives updates, populates `TabState::line_indices`
5. Incremental filtering: `TabState::apply_file_modification()` triggers range-only filtering when file grows
6. Both active and inactive tabs use the same code path (`TabState::apply_filter_event()`)

**Shared Query AST (TUI + MCP):** Both TUI and MCP converge on `FilterQuery` in `src/filter/query/ast.rs`. Adding a new operator, parser, or field comparison works in both automatically.

### Multi-Tab Model

Each tab is independent with its own reader, watcher, viewport, and filter state. Tabs can be files or stdin. Side panel shows all tabs with status.

### Close Confirmation

Closing a tab (`x` / `Ctrl+W`) triggers a confirmation dialog (`InputMode::ConfirmClose`):
- `y` / `Enter` confirms, `n` / `Esc` cancels
- Dialog shows context: file deletion warning for ended sources, quit warning for last tab
- `pending_close_tab` stores the tab index; `confirm_return_mode` restores the previous input mode after confirm/cancel

### Line Expansion

Long lines can be expanded/collapsed for better readability:
- `Space` toggles expansion of the selected line
- `c` collapses all expanded lines
- Expansion state stored in `TabState::expansion` (ExpansionState struct)
- Supports single-expand mode (only one line expanded at a time) or multi-expand mode

### Viewport Navigation

Vim-style viewport commands:
- `Ctrl+E` / `Ctrl+Y`: Scroll viewport down/up, selection moves with scroll
- `zz` / `zt` / `zb`: Center/top/bottom selection on screen
- Edge padding (scrolloff) keeps selection away from screen edges during normal navigation

## Design Principles

Follow **SOLID** principles where practical:

- **Single Responsibility**: Each module/struct owns one concern. `FilterOrchestrator` owns filter dispatch, `App::apply_event()` owns state transitions, `main.rs` owns event collection and terminal I/O. Don't let modules accumulate unrelated responsibilities.
- **Open/Closed**: Extend behavior by adding new types, not modifying existing code. New filter types implement the `Filter` trait. New query operators/parsers extend `FilterQuery` in `query.rs` — both TUI and MCP pick them up automatically. New event types get an arm in `apply_event()` without touching the main loop.
- **Liskov Substitution**: All `Filter` implementors (`StringFilter`, `RegexFilter`, `QueryFilter`) are interchangeable via `Arc<dyn Filter>`. All `LogReader` implementors work with `FilterEngine` and `streaming_filter`.
- **Interface Segregation**: `LogReader` has only the 4 methods every reader needs (`total_lines`, `get_line`, `reload`, `as_any`). Stream-specific operations (`append_lines`, `mark_complete`) live on the separate `StreamableReader` trait — `FileReader` doesn't implement it.
- **Dependency Inversion**: Filter infrastructure depends on the `Filter` trait, not concrete types. Reader infrastructure depends on `LogReader`, not `FileReader`/`StreamReader` directly. `FilterOrchestrator` builds `Arc<dyn Filter>` and passes it to engines that only know the trait.

## Conventions

- Uses **Conventional Commits** for changelog generation: `feat:`, `fix:`, `docs:`, `refactor:`, etc.
- Fast tests run by default; slow tests marked `#[ignore]`
- Event handling centralized in `App::apply_event()` — all side-effects (debounce, cancellation, follow-mode jumps) live there, not in main.rs
- Filter orchestration goes through `FilterOrchestrator` (`src/filter_orchestrator.rs`) — new filter types need changes in `filter/` and the orchestrator
- `LogReader` and `StreamableReader` are separate traits (ISP) — file readers don't carry stream baggage
- Never mention AI usage in code or documentation
- Do not add AI co-author lines to commits
- Always run `cargo fmt`, `cargo clippy`, and `cargo test` before committing
- TUI testing: use tmux manually (send-keys + capture-pane), do NOT write test scripts. See [docs/testing/TUI_TESTING_APPROACH.md](docs/testing/TUI_TESTING_APPROACH.md)
