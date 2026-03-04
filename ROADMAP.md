# LazyTail Roadmap

---

## What's Shipped

| Version | Highlights |
|---------|-----------|
| **v0.2.0** | Multi-tab support, side panel, stdin support, Tab/Shift+Tab/1-9 navigation, AUR package |
| **v0.3.0** | Regex filter (Tab toggle), case sensitivity (Alt+C), expandable entries (Space/c), filter history persistence, stats panel, SIMD streaming filter |
| **v0.4.0** | Source discovery (`lazytail` no args), capture mode (`-n`), active/ended indicators, dir watcher, close tab (x/Ctrl+W), MCP server (6 tools), streaming filter for MCP |
| **v0.5.0** | Config system (`lazytail.yaml`), project scoping (`.lazytail/`), query language (`json \| level == "error"`), MCP query integration, logfmt parser, plain text MCP output, `y` to copy source path |
| **v0.6.0** | Columnar index, severity detection & coloring, bitmap-accelerated filtering, severity histogram, line count/file size in side panel, MCP `get_stats`, incremental indexing, O(1) mmap line access |
| **v0.7.0** | Self-update (`lazytail update`) |
| **post-v0.7.0** | Scrollable help overlay (j/k navigation), MCP `get_tail` `since_line`, copy line to clipboard (`y` + OSC 52), mouse click (side panel tab switch, log line select, category expand/collapse) |

---

## TODO — By Feature

### Web Viewer

#### Bug: Stops Loading at ~700k Lines 🔴
- [ ] Web viewer (`lazytail web`) stops rendering/loading logs at approximately 700k lines
- [ ] Investigate whether the issue is in the HTTP API response, the SPA frontend, or browser memory limits
- [ ] Fix to support full log files (should match TUI capability)

#### Feature Parity with TUI 🔴

**Already in Web:**
- [x] Multi-source with categories, dynamic discovery, status indicators (active/ended)
- [x] Filtering: plain, regex, query syntax (auto-detected), case sensitivity toggle
- [x] Incremental filtering (range-filter on file growth)
- [x] Filter progress display (processing/complete/match count)
- [x] Follow mode toggle (`f` key + checkbox)
- [x] Keyboard nav: `j`/`k`, `g`/`G`, `PageUp`/`PageDown`, `Ctrl+U`/`Ctrl+D`, `/` to focus filter
- [x] Tab switching: `Tab`/`Shift+Tab`, `1`-`9` direct jump
- [x] Close/delete source with safety checks
- [x] File watching with auto-reload
- [x] Severity counts per source (in API response)
- [x] Virtual scrolling for performance
- [x] Real-time updates via SSE long-polling

**Missing — Rendering & Display:**
- [ ] ANSI color rendering (web strips all ANSI — TUI renders colors inline)
- [ ] Severity coloring on log lines (TUI colors line number background by severity; web has severity data but doesn't render it)
- [ ] Severity bar chart / histogram in side panel (TUI shows colored bars with counts)
- [ ] Line ingestion rate display (TUI shows lines/s or lines/min during active streaming)
- [ ] Index file size display in stats panel
- [ ] Source file size display per tab (TUI shows "512B", "2.3MB" inline)

**Missing — Line Expansion:**
- [ ] `Space` to toggle expand/collapse selected line
- [ ] `c` to collapse all expanded lines
- [ ] Word-wrapped expanded content with indented continuation lines
- [ ] Expanded line background styling

**Missing — Navigation:**
- [ ] Line selection highlight (TUI has bold + dark gray background on selected line)
- [ ] `Ctrl+E`/`Ctrl+Y` viewport scroll (scroll without moving selection)
- [ ] `zz`/`zt`/`zb` center/top/bottom selection on screen
- [ ] `:N` jump to specific line number
- [ ] Scrolloff edge padding (TUI keeps selection 3 lines from edge)
- [ ] `J`/`K` source cycling (web has it but uses uppercase — verify parity with TUI `Tab` in source panel)

**Missing — Clipboard:**
- [ ] `y` to copy selected line content (OSC 52 / fallback)
- [ ] `y` in source panel to copy source file path
- [ ] Status message feedback ("Copied: ...")

**Missing — UI Components:**
- [ ] Help overlay (`?` key) with scrollable keybinding reference
- [ ] Close confirmation dialog (TUI shows source name, deletion warning, quit warning)
- [ ] Source panel focus mode with `j`/`k` tree navigation, `Space` expand/collapse categories, `Enter` to select
- [ ] Status bar context-aware help hints (changes based on current mode)
- [ ] Filter mode visual indicators: border color per mode (white=plain, cyan=regex, magenta=query, red=invalid)
- [ ] "Filter [Aa]" label showing case sensitivity state
- [ ] Filter history navigation (`↑`/`↓` in filter input to browse previous patterns)

**Missing — Modes & Input:**
- [ ] `z`-pending mode (two-key chord: `zz`, `zt`, `zb`)
- [ ] Source panel as distinct input mode (TUI: `Tab` toggles panel focus, `Esc` returns)
- [ ] Confirm close mode (TUI: `y`/`Enter` confirm, `n`/`Esc` cancel)

**Missing — Mouse:**
- [ ] Click to select a specific log line (with selection highlight)
- [ ] Click source panel category headers to expand/collapse
- [ ] Mouse scroll coalescing (TUI batches rapid scroll events)

**Missing — Misc:**
- [ ] `q` / `Ctrl+C` to quit (web has no quit concept, but could close tab/window)
- [ ] Side panel active filter indicator (`*` cyan) per tab
- [ ] Side panel follow mode indicator (`F` green) per tab
- [ ] Side panel loading indicator (`⟳` magenta) for streaming sources
- [ ] File path in title bar (TUI shows full source path in log view title)

---

### Query Language

#### Explicit Filter Mode 🟡
- [ ] Add `Query` variant to `FilterMode` enum
- [ ] `Tab` cycles: Plain → Regex → Query → Plain
- [ ] Remove heuristic auto-detection from filter dispatch
- [ ] Update prompt label, frame color, help text
- [ ] Update filter history serialization
- [ ] Tests for mode cycling and dispatch

#### Additional Aggregation Types 🔴
- [x] `count by (field)` — e.g. `json | level == "error" | count by (service)` ✅
- [x] `top N` / limit ✅
- [x] Multiple `group_by` fields — e.g. `count by (service, level)` ✅
- [x] Return results as structured JSON (field → count map) ✅
- [x] Wire into MCP (extend `search` response with `aggregate` in query) ✅
- [x] Wire into text query parser (`count by (fields) | top N`) ✅
- [x] TUI aggregation view with j/k navigation and drill-down ✅
- [ ] `avg(field) by (fields)` — average of numeric field grouped by others (e.g. `json | avg(latency) by (service)`)
- [ ] `sum(field) by (fields)` — total of numeric field (e.g. `json | sum(processed) by (service)`)
- [ ] `min(field) by (fields)` / `max(field) by (fields)` — extremes with drill-down to actual line
- [ ] `p50(field)` / `p90(field)` / `p99(field) by (fields)` — percentiles (e.g. `json | p99(latency) by (service)`)
- [ ] `rate(interval)` — count per time window (e.g. `json | level == "error" | rate(1m)`) — requires timestamp parsing
- [ ] `count_distinct(field) by (fields)` — unique value count (e.g. `json | count_distinct(user.id) by (service)`)
- [ ] `histogram(field, bucket_size)` — bucket numeric field into ranges (e.g. `json | histogram(latency, 100)`)

#### Time Filtering 🔴
- [ ] Timestamp field detection (common field names)
- [ ] Time range filtering (after/before)

#### Polish 🟡
- [ ] Syntax highlighting in filter input
- [ ] Autocomplete for field names (sample lines, extract field names, offer completions after `|` or on Tab)
- [ ] `format` stage — `json | format <severity> - <method> <url> - <status>`
- [ ] Query history with mode
- [ ] Filter history prefix matching (zsh-style `Up`/`Down` with typed prefix)
- [ ] `pattern` parser (extract fields via template)
- [ ] Documentation and examples

---

### MCP Server

#### New Tools 🔴
- [x] **`aggregate`** — count by field, top N via `search` query `aggregate` param ✅
- [ ] **`search_sources`** — search all sources at once, grouped by source name. Cross-service correlation
- [ ] **`fields`** — sample N lines, return field names/types/examples. Critical for LLM consumers

#### Enhancements 🟡
- [ ] `time_range` param for `search`
- [ ] Search pagination / cursor (offset for results > 1000)
- [ ] **`summarize`** tool — time range, top patterns, error rate
- [ ] **`add_source`** tool — register file as source from AI agents
- [ ] MCP project scoping (detect `lazytail.yaml`, scope `list_sources`)
- [ ] Filter presets from config available in MCP

#### Low Priority
- [ ] `get_lines` negative indexing / "from end" shorthand
- [ ] **`export`** tool — dump filtered results to file in bulk

---

### TUI Interaction

#### Help Overlay 🔴
- [ ] Fix: help overlay renders as a floating box that overlaps both side panel and log content, leaving fragments of both visible around edges — should be a full-pane overlay or clipped to the log content area
- [ ] The `↓` scroll indicator in the title (`Help ↓`) is too subtle — add a visible scrollbar or clearer scroll affordance

#### Mouse Controls 🟢
- [x] Click to select a log line ✅
- [x] Click source in side panel to switch tabs ✅
- [ ] Click severity levels to filter
- [ ] Click-and-drag to select text for copying
- [ ] Right-click context menu (expand, copy, filter by selection)
- [ ] Resize side panel by dragging divider
- [ ] Double-click to expand/collapse a log line

#### Multi-Line Selection 🟢
- [ ] `V` enters visual/selection mode, `Esc` exits
- [ ] `Shift+Up`/`Shift+Down` or `Shift+j`/`Shift+k` to extend selection
- [ ] `Shift+Click` to select range, click-and-drag for mouse selection
- [ ] Selected lines highlighted with distinct background
- [ ] `y` copies all selected lines to clipboard (raw content, newline-separated)
- [ ] Status bar indicator ("3 lines selected")

#### Search Highlighting 🟢
- [ ] Highlight filter matches in displayed text
- [ ] Handle case sensitivity and regex patterns
- [ ] Configurable highlight colors

#### Severity Filtering 🟡
- [ ] Quick filter: show ERROR and above
- [ ] Keybinding to cycle minimum severity level
- [ ] Click severity in stats panel to filter

#### Filter UI Polish 🟢
- [ ] Save current input as draft when navigating history with arrow keys (so user can arrow back down to restore)
- [ ] Display history entries with mode indicator
- [ ] Show regex error message in status bar

---

### Clipboard & Copy

- [x] Context-aware: copy selected line in log view, source path in side panel ✅
- [x] Full raw line content (ANSI-stripped) ✅
- [x] Visual feedback (status bar: "Copied: ...") ✅
- [x] OSC 52 escape sequence (works over SSH/tmux) ✅
- [ ] Fallback to xclip/xsel/wl-copy/pbcopy 🟢
- [ ] Copy expanded content when line is expanded 🟢

---

### JSON & Line Expansion

#### JSON Pretty Viewer 🟢
- [ ] Auto-detect JSON in log lines
- [ ] Pretty-print with syntax highlighting
- [ ] Collapsible/expandable nested objects (tree navigation)
- [ ] Integrates with line expansion (`Space`)

#### AI Conversation JSONL Viewer 🟢
- [ ] Detect JSONL files with `role`/`content` fields (OpenAI, Anthropic, generic chat formats)
- [ ] Render as conversation: role labels (User/Assistant/System), indented message bubbles
- [ ] Syntax-highlight code blocks within messages
- [ ] Collapse/expand individual messages
- [ ] Filter by role (`json | role == "assistant"`)
- [ ] Useful for inspecting LLM training data, API logs, chat transcripts

#### Expandable Entries — Remaining 🟢
- [ ] Fix: expanding a JSON line just shows raw JSON word-wrapped over multiple lines — should pretty-print with indentation and syntax highlighting instead of plain wrap 🔴
- [ ] Fix: expanding a line near the bottom of the screen shows empty lines when the expansion doesn't fit — viewport should auto-scroll up so the expanded content is visible 🔴
- [ ] Scrolling within expanded content — expanded views (especially pretty-printed JSON) can exceed screen height; need internal scroll, viewport clipping, and sensible max-height with scroll indicators
- [ ] Collapsible JSON nodes (nested objects)

---

### Side Panel

- [ ] Fix: launching with no args lands on `global-source` which shows an empty pane (0 lines) — new users see a blank screen; should default to the first source with content, or show welcome/instruction text 🔴
- [ ] Fix: selected empty/ended source is invisible — grayed-out dim text has no visible selection highlight, making it unclear which tab is active 🔴
- [ ] Replace numeric tab indicators (`1`, `2`, `3`...) in side panel with a text-based cursor/focus indicator — `>` already marks the active tab, need a distinct indicator for the focused row during tree navigation (e.g., `▸`, `→`, reverse highlight without numbers) so selection is visible without relying solely on background color 🟡
- [ ] Preview log source content while navigating the side panel — show a live preview of the highlighted source before switching to it 🟡
- [ ] Toggle panel visibility keybinding 🟡
- [ ] Configurable panel width 🟡
- [ ] Tree structure with collapsible groups 🟡
- [ ] Search/filter within source list 🟡

---

### Capture Mode

- [ ] Keybinding to rebuild/reindex the current source's columnar index (useful when index is stale or corrupted) 🟡
- [ ] Truncate log file by default on `lazytail -n` (add `--append`/`-a` to keep old behavior) 🟡
- [ ] Session ID per capture run (UUID in marker + log boundary marker + filter by session) 🟡
- [ ] `--file <path>` for custom log file location 🟡
- [ ] `--max-size <size>` for log rotation 🟡
- [ ] Display captured logs formatted with rendering presets in the terminal during `cmd | lazytail -n name` (apply preset formatting to passthrough output) 🟡
- [ ] `--raw` flag to bypass rendering preset formatting and output unmodified log lines during capture 🟡

---

### CLI Subcommands 🟡

- [ ] **`lazytail sources`** — list sources (name, path, status, lines, size; `--json`)
- [ ] **`lazytail search <pattern> [source]`** — CLI grep with lazytail engines (plain/regex/query, `--count`, `--context N`, color, SIMD)
- [ ] **`lazytail tail [source]`** — tail source (`-f` follow, `-n 50`, `--filter`)
- [ ] **`lazytail clear`** — clear ended sources (by name, `--all`, `--yes`/`-y`)
- [ ] **`lazytail add <name> --path <path>`** — register existing file as source
- [ ] **`lazytail rm <name>`** — unregister source (`--delete-data`, `--ended`)
- All respect project scoping (`lazytail.yaml` → `.lazytail/`, otherwise global)

---

### Configuration

- [ ] Filter presets in config 🟡
- [ ] Custom source groups/categories 🟡
- [ ] Default filter patterns per source 🟡
- [ ] UI preferences (colors, panel width, default modes) 🟡
- [ ] MCP server settings (enabled tools, access control) 🟡
- [ ] Allow manual severity format override per source 🟡
- [ ] Theme/colors config via `lazytail.yaml` (named + hex colors, built-in themes) 🟢
- [ ] Theme and color overrides should support customizing the app background color (not just text/severity colors) 🟢
- [ ] Theme-aware rendering presets — preset styles resolve colors from `theme.palette` instead of fixed ANSI names, so changing themes also affects log line formatting 🟢
- [ ] Keybindings config via `lazytail.yaml` 🟢
- [ ] In-app configuration UI — TUI overlay with two sections: Global (`~/.config/lazytail/config.yaml`) and Project (`lazytail.yaml`), for editing theme, display options, and per-source settings without manually editing config files 🟢

---

### Observability & Debugging 🔴

- [ ] Add `tracing` + `tracing-subscriber` dependencies
- [ ] Initialize subscriber (RUST_LOG, RUST_LOG_FORMAT, --log-file)
- [ ] Instrument: reader/, filter/, main.rs event loop, index/builder, watcher, mcp/, capture
- [ ] Span instrumentation for perf-critical paths (filter timing, index building, read throughput)
- [ ] `lazytail --version --verbose` — build info, feature flags
- [ ] `lazytail doctor` — check config, permissions, verify setup

---

### Performance & Scalability

- [ ] Search Result Bitmap Cache — persist filter results as Roaring Bitmaps in `.lazytail/idx/{source}/search/`. Repeat searches skip scanning. Extend (not invalidate) on file append. AND/OR bitmaps for compound queries. Transparent fallback to `streaming_filter` on cache miss
- [ ] Compressed file support (read `.gz`/`.zst`/`.lz4`, capture-time compression, index compression)
- [ ] Performance profiling on 100GB+ files
- [ ] Optimize ANSI parsing (cache parsed lines)
- [ ] Benchmark filtering performance
- [ ] Further optimize case-insensitive search

---

### Backlog / Ideas

- [ ] Store per-file indexes in `.lazytail/` (project) or `~/.config/lazytail/` (global) instead of next to the log file — avoids polluting working directories with `.idx/` folders when opening files directly
- [ ] Memory-only mode with streaming (no file)
- [ ] Merged chronological view across sources (timestamp parsing + source-colored lines)
- [ ] Filter across all tabs simultaneously
- [ ] Command-based sources in config (`command: "docker logs -f api"`)
- [ ] Tmux-aware capture (store session:window.pane in marker, expose in `list_sources`)
- [ ] Bookmarks (mark lines for quick navigation)
- [ ] Export filtered results to file
- [ ] Multiple display modes (raw, compact, JSON, table)
- [ ] Custom format definitions (regex-based) in config
- [ ] Timestamp parsing from detected format
- [ ] Auto-detect field names for structured logs
- [ ] Drag-and-drop reordering in side panel
- [ ] Bookmarks section in side panel (per project, persist to config)
- [ ] Arrow keys to navigate panel when focused
- [ ] Collapsible stats section
- [ ] Sidecar index (`.log.idx`) — arrival timestamp + byte offset per line, real-time append during capture
- [ ] Combined source view — merge sources chronologically using sidecar timestamps
- [ ] When opening files as arguments (`lazytail file.log`), still show all discovered sources in the side panel under a separate "Args" category — currently only the file arg and global sources appear, hiding project/captured sources
- [ ] Integration tests for full app behavior
- [ ] UI snapshot testing
- [ ] Performance benchmarks in CI
- [ ] Pre-built binaries for Windows

---

## Design Notes

### Query Language Architecture
```
┌─────────────────────┐      ┌─────────────────────┐
│  Text Query (UI)    │      │  JSON Query (MCP)   │
│ json | level=="err" │      │ {"parser":"json",   │
│                     │      │  "filters":[...]}   │
└──────────┬──────────┘      └──────────┬──────────┘
           │  parse                     │  deserialize
           ▼                            ▼
      ┌────────────────────────────────────┐
      │         FilterQuery (AST)          │
      └──────────────────┬─────────────────┘
                         │  execute
                         ▼
                  ┌─────────────┐
                  │   Results   │
                  └─────────────┘
```

Both TUI and MCP converge on shared `FilterQuery` AST. Adding operators/parsers works in both automatically.

### Sidecar Index Design
- Binary `.log.idx` alongside each captured log
- Arrival timestamp + byte offset per line
- Header: file size, mtime, first-4KB hash (corruption detection)
- Auto-rebuild on truncation
- Enables time-based operations and source merging

### Compressed Files — Key Tradeoffs
- Compressed files lose O(1) random line access
- Block compression (zstd seekable) can preserve random access
- Streaming filter (mmap + SIMD) incompatible with compressed data
- Prioritize read support first, write compression second

---

## Development Workflow

### Before Starting a Feature
1. Update this roadmap with detailed tasks
2. Consider impact on existing tests
3. Plan for backward compatibility

### Before Completion
1. All tests pass (`cargo test`)
2. Clippy clean (`cargo clippy`)
3. Formatted (`cargo fmt`)
4. Documentation updated
5. Roadmap updated to mark task complete
