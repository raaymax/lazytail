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

## TODO — By Priority

### HIGH

#### Query Language — Aggregation (Phase 3)
- [ ] `count by (field)` — e.g. `json | level == "error" | count by (service)`
- [ ] `top N` / limit
- [ ] Multiple `group_by` fields — e.g. `count by (service, level)`
- [ ] Return results as structured JSON (field → count map)
- [ ] Wire into MCP (`aggregate` tool or extend `search` response)
- [ ] Wire into text query parser (AST slots already exist)

#### Query Language — Time Filtering
- [ ] Timestamp field detection (common field names)
- [ ] Time range filtering (after/before)

#### MCP — New Tools
- [ ] **`aggregate`** — count by field, top N. Single call replaces N manual queries
- [ ] **`search_sources`** — search all sources at once, grouped by source name. Cross-service correlation
- [ ] **`fields`** — sample N lines, return field names/types/examples. Critical for LLM consumers

#### Structured Logging & Debug Instrumentation
- [ ] Add `tracing` + `tracing-subscriber` dependencies
- [ ] Initialize subscriber (RUST_LOG, RUST_LOG_FORMAT, --log-file)
- [ ] Instrument: reader/, filter/, main.rs event loop, index/builder, watcher, mcp/, capture
- [ ] Span instrumentation for perf-critical paths (filter timing, index building, read throughput)
- [ ] `lazytail --version --verbose` — build info, feature flags
- [ ] `lazytail doctor` — check config, permissions, verify setup

---

### MEDIUM

#### Explicit Query Filter Mode
- [ ] Add `Query` variant to `FilterMode` enum
- [ ] `Tab` cycles: Plain → Regex → Query → Plain
- [ ] Remove heuristic auto-detection from filter dispatch
- [ ] Update prompt label, frame color, help text
- [ ] Update filter history serialization
- [ ] Tests for mode cycling and dispatch

#### Query Language — Polish (Phase 6)
- [ ] Syntax highlighting in filter input
- [ ] `format` stage — `json | format <severity> - <method> <url> - <status>`
- [ ] Query history with mode
- [ ] Filter history prefix matching (zsh-style `Up`/`Down` with typed prefix)
- [ ] `pattern` parser (extract fields via template)
- [ ] Documentation and examples

#### Capture Mode Enhancements
- [ ] Truncate log file by default on `lazytail -n` (add `--append`/`-a` to keep old behavior)
- [ ] Session ID per capture run (UUID in marker + log boundary marker + filter by session)
- [ ] `--file <path>` for custom log file location
- [ ] `--max-size <size>` for log rotation

#### CLI Subcommands
- [ ] **`lazytail sources`** — list sources (name, path, status, lines, size; `--json`)
- [ ] **`lazytail search <pattern> [source]`** — CLI grep with lazytail engines (plain/regex/query, `--count`, `--context N`, color, SIMD)
- [ ] **`lazytail tail [source]`** — tail source (`-f` follow, `-n 50`, `--filter`)
- [ ] **`lazytail clear`** — clear ended sources (by name, `--all`, `--yes`/`-y`)
- [ ] **`lazytail add <name> --path <path>`** — register existing file as source
- [ ] **`lazytail rm <name>`** — unregister source (`--delete-data`, `--ended`)
- All respect project scoping (`lazytail.yaml` → `.lazytail/`, otherwise global)

#### MCP — Enhancements
- [ ] `time_range` param for `search`
- [ ] Search pagination / cursor (offset for results > 1000)
- [ ] **`summarize`** tool — time range, top patterns, error rate
- [ ] **`add_source`** tool — register file as source from AI agents
- [ ] MCP project scoping (detect `lazytail.yaml`, scope `list_sources`)
- [ ] Filter presets from config available in MCP

#### Severity Filtering (TUI)
- [ ] Quick filter: show ERROR and above
- [ ] Keybinding to cycle minimum severity level
- [ ] Click severity in stats panel to filter

#### Side Panel Enhancements
- [ ] Toggle panel visibility keybinding
- [ ] Configurable panel width
- [ ] Tree structure with collapsible groups
- [ ] Search/filter within source list

#### Config Enhancements
- [ ] Filter presets in config
- [ ] Custom source groups/categories
- [ ] Default filter patterns per source
- [ ] UI preferences (colors, panel width, default modes)
- [ ] MCP server settings (enabled tools, access control)
- [ ] Allow manual severity format override per source

---

### LOW

#### Mouse Controls
- [x] Click to select a log line ✅
- [x] Click source in side panel to switch tabs ✅
- [ ] Click severity levels to filter
- [ ] Click-and-drag to select text for copying
- [ ] Right-click context menu (expand, copy, filter by selection)
- [ ] Resize side panel by dragging divider
- [ ] Double-click to expand/collapse a log line

#### Multi-Line Selection
- [ ] `V` enters visual/selection mode, `Esc` exits
- [ ] `Shift+Up`/`Shift+Down` or `Shift+j`/`Shift+k` to extend selection
- [ ] `Shift+Click` to select range, click-and-drag for mouse selection
- [ ] Selected lines highlighted with distinct background
- [ ] `y` copies all selected lines to clipboard (raw content, newline-separated)
- [ ] Status bar indicator ("3 lines selected")

#### Search Highlighting
- [ ] Highlight filter matches in displayed text
- [ ] Handle case sensitivity and regex patterns
- [ ] Configurable highlight colors

#### Copy to Clipboard (`y`) — partially done ✅
- [x] Context-aware: copy selected line in log view, source path in side panel ✅
- [x] Full raw line content (ANSI-stripped) ✅
- [x] Visual feedback (status bar: "Copied: ...") ✅
- [x] OSC 52 escape sequence (works over SSH/tmux) ✅
- [ ] Fallback to xclip/xsel/wl-copy/pbcopy
- [ ] Copy expanded content when line is expanded

#### JSON Pretty Viewer in TUI
- [ ] Auto-detect JSON in log lines
- [ ] Pretty-print with syntax highlighting
- [ ] Collapsible/expandable nested objects (tree navigation)
- [ ] Integrates with line expansion (`Space`)

#### Expandable Entries — Remaining
- [ ] Scrolling within expanded content (huge JSON)
- [ ] Collapsible JSON nodes (nested objects)

#### Filter UI Polish
- [ ] Display history entries with mode indicator
- [ ] Show regex error message in status bar

#### MCP — Low Priority
- [ ] `get_lines` negative indexing / "from end" shorthand
- [ ] **`export`** tool — dump filtered results to file in bulk

#### TUI Customization
- [ ] Theme/colors config via `lazytail.yaml` (named + hex colors, built-in themes)
- [ ] Keybindings config via `lazytail.yaml`

---

### BACKLOG / IDEAS

- [ ] Search Result Bitmap Cache — persist filter results as Roaring Bitmaps in `.lazytail/idx/{source}/search/`. Repeat searches skip scanning. Extend (not invalidate) on file append. AND/OR bitmaps for compound queries. Transparent fallback to `streaming_filter` on cache miss. See `.planning/ROADMAP.md` Phase 6
- [ ] Compressed file support (read `.gz`/`.zst`/`.lz4`, capture-time compression, index compression)
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
- [ ] Performance profiling on 100GB+ files
- [ ] Optimize ANSI parsing (cache parsed lines)
- [ ] Benchmark filtering performance
- [ ] Further optimize case-insensitive search
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
