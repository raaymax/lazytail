# LazyTail Roadmap

This is a local planning document for upcoming features and improvements.

---

## Current Status (v0.3.0)

**Core Features Complete:**
- Lazy file reading with indexed line positions
- Live filtering with background processing
- File watching and auto-reload
- Follow mode (tail -f style)
- Filter history with arrow key navigation
- ANSI color support
- Vim-style line jumping (`:123`)
- Vim-style z commands (zz, zt, zb)
- Mouse scroll support
- Help overlay (`?` key)
- Event-based architecture

**v0.2.0 Features:**
- Multi-tab support with side panel UI
- Stdin support (`cmd | lazytail`)
- Multiple file arguments (`lazytail a.log b.log`)
- Per-tab state (filter, scroll, follow mode)
- Tab navigation (Tab, Shift+Tab, 1-9)
- AUR package available

**v0.3.0 Features:**
- Regex filter mode (Tab to toggle)
- Case sensitivity toggle (Alt+C)
- Filter history with mode persistence
- Expandable log entries (Space to toggle, c to collapse)
- Persistent filter history to disk
- Stats panel (line counts)
- Filter progress percentage display
- Streaming filter with SIMD search (memmem) for better performance
- Grep-style search for case-sensitive patterns

---

## Upcoming Features & Improvements

### 🔴 HIGH PRIORITY

#### Phase 1: Multi-Tab Support (CLI Arguments) ✅
**Goal:** View multiple log files in tabs within single UI instance

**Status:** Complete (v0.2.0)

```bash
lazytail api.log worker.log db.log
# Opens UI with side panel showing all sources
```

**UI Layout:**
```
┌──────────────┬──────────────────────────────────────────────────────┐
│ Sources      │ [log content]                                        │
│──────────────│                                                      │
│ > api.log    │ 10:00:01 INFO  Starting server...                   │
│   worker.log │ 10:00:02 DEBUG Connected to DB                      │
│   db.log     │ 10:00:03 INFO  Listening on :8080                   │
│              │ 10:00:04 ERROR Connection refused                   │
│──────────────│ 10:00:05 INFO  GET /health 200                      │
│ Severity     │                                                      │
│──────────────│                                                      │
│ ○ FATAL    0 │                                                      │
│ ● ERROR   12 │                                                      │
│ ○ WARN    45 │                                                      │
│ ○ INFO   892 │                                                      │
│ ○ DEBUG   45 │                                                      │
│──────────────│──────────────────────────────────────────────────────│
│ [Bookmarks]  │ Filter: _                    Showing 12/1183 ⟳ 45%   │
└──────────────┴──────────────────────────────────────────────────────┘

Status bar (right-aligned indicators):
- "Showing X/Y" - filtered count / total count
- "⟳ 45%" - filter processing progress (hidden when idle)
- "●" - follow mode active indicator

Two-panel layout:
- Left:   Source list, severity filter, bookmarks (future)
- Right:  Log content + filter input
```

**Side Panel Design:**
- Left panel shows all available sources
- Tree structure ready for future organization (folders, groups)
- Active source highlighted with `>`
- Shows indicators: `*` for unsaved filter, `●` for active/live source
- Panel can be toggled hidden/visible (e.g., `Ctrl+B`)
- Future: Bookmarks section at bottom for project-scoped quick access

**Tasks:**
- [x] Multi-tab state management
  - [x] Add `Vec<TabState>` to App (selection, filter, scroll, follow mode per tab)
  - [x] Track active tab index
  - [x] Refactor single-file state into `TabState` struct
- [x] Side panel UI component
  - [x] Render source list on left
  - [x] Highlight active source
  - [x] Show status indicators (active/ended, filter active, follow mode)
  - [ ] Toggle panel visibility keybinding
  - [ ] Configurable panel width
- [x] Tab navigation keybindings
  - [x] `Tab` / `Shift+Tab` to cycle sources
  - [x] `1-9` for direct source access
  - [ ] Arrow keys to navigate panel when focused
  - [x] Show keybindings in help overlay
- [x] File watching for multiple files
  - [x] Watch all open files simultaneously
  - [x] Update correct tab on file change
- [x] CLI argument handling
  - [x] Accept multiple file paths
  - [x] Validate all files exist before starting
- [x] Backward compatibility
  - [x] Single file still works: `lazytail file.log`
- [x] Add tests for multi-tab behavior

**Future Side Panel Enhancements:**
- [ ] Tree structure with collapsible groups
- [ ] Drag-and-drop reordering
- [ ] Bookmarks section (per UI instance / project scope)
  - Save frequently used file combinations
  - Quick switch between "projects"
  - Persist bookmarks to config file
- [ ] Search/filter within source list

**Use Cases:**
```bash
# Compare multiple services
lazytail api.log worker.log scheduler.log

# System logs
lazytail /var/log/syslog /var/log/auth.log

# Multiple container logs (pre-captured)
lazytail pod1.log pod2.log pod3.log
```

---

#### Phase 2: Source Discovery
**Goal:** Auto-discover log sources from config directory

```bash
lazytail              # No args → discover sources from ~/.config/lazytail/data/
lazytail api.log      # Explicit file → single tab (backward compatible)
```

**Directory Structure:**
```
~/.config/lazytail/
├── data/             # Log files (auto-discovered)
│   ├── API.log
│   ├── Worker.log
│   └── DB.log
└── sources/          # Active source markers
    ├── API           # Contains PID, indicates source is live
    └── Worker
```

**Tasks:**
- [ ] Config directory setup
  - [ ] Create `~/.config/lazytail/data/` on first run
  - [ ] Create `~/.config/lazytail/sources/` on first run
- [ ] Source discovery (UI mode)
  - [ ] Scan `data/` directory for `.log` files
  - [ ] Check `sources/` for active markers (file exists + PID valid)
  - [ ] Display discovered sources as tabs
  - [ ] Show active/ended status indicator per tab
- [ ] Watch for new sources
  - [ ] Monitor `data/` directory for new files
  - [ ] Add new tabs dynamically when sources appear
- [ ] Tab management
  - [ ] Close tab keybinding (e.g., `x` or `Ctrl+W`)
  - [ ] Optionally delete source file on close
- [ ] Add tests for discovery behavior

**Behavior:**
- `lazytail` (no args) → discover mode, show all sources from config dir
- `lazytail file.log` → explicit mode, show only that file
- `lazytail file1.log file2.log` → explicit mode, show those files

---

#### Phase 3: Source Capture Mode (Tee-like)
**Goal:** Capture stdin to named source, viewable in UI

```bash
# Capture logs from any command
cmd | lazytail -n "API"
lazytail -n "API" <(kubectl logs -f pod)

# Works like:
# cmd | tee ~/.config/lazytail/data/API.log
# + register in sources/ + collision check + header
```

**Tasks:**
- [ ] CLI argument parsing
  - [ ] `-n <name>` flag for source mode
  - [ ] Detect stdin input
- [ ] Source mode implementation
  - [ ] Name collision detection (check marker + PID validity)
  - [ ] Create marker file in `sources/` with PID
  - [ ] Print header: `Serving "API" → ~/.config/lazytail/data/API.log`
  - [ ] Read stdin line by line
  - [ ] Write to log file (append)
  - [ ] Echo to stdout (tee behavior)
  - [ ] On EOF: remove marker, exit (file persists)
- [ ] Signal handling
  - [ ] Handle SIGINT/SIGTERM gracefully
  - [ ] Clean up marker file on exit
- [ ] Error handling
  - [ ] Exit with error if name collision
  - [ ] Handle write errors gracefully
- [ ] Add tests for source mode

**Full Workflow:**
```bash
# Terminal 1: Capture API logs
kubectl logs -f api-pod | lazytail -n "API"

# Terminal 2: Capture worker logs
kubectl logs -f worker-pod | lazytail -n "Worker"

# Terminal 3: View everything
lazytail
# Shows tabs: [API] [Worker]
# API marked as "active", Worker marked as "active"

# Kill Terminal 1
# UI shows: API now marked as "ended", history still available
```

---

#### Future Enhancements (Post-Phase 3)
- [ ] `--file <path>` for custom log file location
- [ ] `--max-size <size>` for log rotation
- [ ] Memory-only mode with streaming (no file)
- [ ] Merged chronological view across sources
- [ ] Filter across all tabs simultaneously

---

#### Phase 4: Advanced Filter Modes
**Goal:** Add regex filtering and case sensitivity with intuitive mode switching

**UX Design:**
```
┌─────────────────────────────────────────────────────────────┐
│ Plain text mode (default):                                  │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ Filter: error                              [Tab: Regex] │ │
│ └─────────────────────────────────────────────────────────┘ │
│ Frame color: default (e.g., white/gray)                     │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Regex mode:                                                 │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ Regex: error|warn|fatal                    [Tab: Plain] │ │
│ └─────────────────────────────────────────────────────────┘ │
│ Frame color: distinct (e.g., cyan/magenta)                  │
└─────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────┐
│ Invalid regex (visual feedback):                            │
│ ┌─────────────────────────────────────────────────────────┐ │
│ │ Regex: error[                              [Tab: Plain] │ │
│ └─────────────────────────────────────────────────────────┘ │
│ Frame color: red (indicates error)                          │
└─────────────────────────────────────────────────────────────┘
```

**Behavior:**
- `Tab` while in filter input: toggles between plain text and regex mode
- Filter panel frame color changes to indicate current mode
- Invalid regex: frame turns red, filter not applied until valid
- Reopening filter (`/`) restores last used mode
- History stores mode per entry, navigating history switches mode automatically
- Case sensitivity toggle available in both modes

**Filter Mode States:**
```
FilterMode {
    Plain { case_sensitive: bool },
    Regex { case_sensitive: bool },
}
```

**History Entry:**
```
FilterHistoryEntry {
    pattern: String,
    mode: FilterMode,
}
```

**Keybindings (while in filter input):**
- `Tab` - Toggle between Plain/Regex mode
- `Ctrl+I` - Toggle case sensitivity
- `Up/Down` - Navigate history (mode switches automatically)
- `Enter` - Apply filter
- `Esc` - Cancel

**Visual Indicators:**
| Mode | Frame Color | Label |
|------|-------------|-------|
| Plain (case-insensitive) | Default | `Filter:` |
| Plain (case-sensitive) | Default | `Filter [Aa]:` |
| Regex (case-insensitive) | Cyan | `Regex:` |
| Regex (case-sensitive) | Cyan | `Regex [Aa]:` |
| Regex (invalid) | Red | `Regex:` |

**Tasks:**
- [x] Filter mode enum and state
  - [x] Create `FilterMode` enum (Plain, Regex)
  - [x] Add case_sensitive flag to each mode
  - [x] Store current mode in App/Tab state
  - [x] Persist mode when closing filter input
- [x] Filter input UI changes
  - [x] Tab key toggles mode while in filter input
  - [x] Different frame colors per mode
  - [x] Show mode indicator in prompt (Filter: vs Regex:)
  - [x] Show case sensitivity indicator [Aa]
  - [x] Red frame for invalid regex
- [x] History with mode support
  - [x] Update FilterHistoryEntry to include mode
  - [x] When navigating history, switch to stored mode
  - [ ] Display history entries with mode indicator
- [x] Regex validation
  - [x] Validate regex on each keystroke
  - [x] Show visual error state (red frame)
  - [x] Don't apply filter until regex is valid
  - [ ] Show error message in status bar (optional)
- [x] Case sensitivity
  - [x] Alt+C toggles case sensitivity
  - [x] Update StringFilter to respect flag
  - [x] Update RegexFilter to respect flag (regex::RegexBuilder)
- [x] Integration
  - [x] Wire up to existing FilterEngine
  - [x] Ensure background filtering works with both modes
  - [x] Handle mode in filter re-application on file change
- [x] Tests
  - [x] Unit tests for mode switching
  - [x] Tests for history mode restoration
  - [x] Tests for regex validation
  - [x] Tests for case sensitivity
- [x] Documentation
  - [x] Update help overlay with new keybindings
  - [x] Update README

**Current Status:** ✅ Complete

---

### 🟡 MEDIUM PRIORITY

#### Expandable Log Entries ✅
**Goal:** Open/expand log entries to view full content (long lines, JSON properties)

**Status:** Implemented - Space to toggle, 'c' to collapse all

**Use Cases:**
- View truncated long lines in full
- Pretty-print JSON log entries
- Inspect multi-line stack traces
- Copy full content of a log entry

**UI Behavior:**
```
Normal view (collapsed):
┌─────────────────────────────────────────────────────────┐
│ 142  2024-01-20 10:00:01 {"level":"error","msg":"Fai...│
│ 143  2024-01-20 10:00:02 Starting worker process       │
│ 144  2024-01-20 10:00:03 Connection established        │
└─────────────────────────────────────────────────────────┘

Expanded view (press Enter or 'o' on line 142):
┌─────────────────────────────────────────────────────────┐
│ 142  2024-01-20 10:00:01 {"level":"error","msg":"Fai...│
│ ┌─────────────────────────────────────────────────────┐ │
│ │ {                                                   │ │
│ │   "level": "error",                                 │ │
│ │   "msg": "Failed to connect to database",          │ │
│ │   "error": "connection refused",                   │ │
│ │   "host": "db.example.com",                        │ │
│ │   "port": 5432,                                    │ │
│ │   "retry_count": 3                                 │ │
│ │ }                                                   │ │
│ └─────────────────────────────────────────────────────┘ │
│ 143  2024-01-20 10:00:02 Starting worker process       │
│ 144  2024-01-20 10:00:03 Connection established        │
└─────────────────────────────────────────────────────────┘

Raw expanded view (for non-JSON long lines):
┌─────────────────────────────────────────────────────────┐
│ 142  2024-01-20 10:00:01 Very long log message that ...│
│ ┌─────────────────────────────────────────────────────┐ │
│ │ Very long log message that contains a lot of       │ │
│ │ information and spans multiple lines when fully    │ │
│ │ displayed without truncation so you can read the   │ │
│ │ entire content of the log entry.                   │ │
│ └─────────────────────────────────────────────────────┘ │
│ 143  2024-01-20 10:00:02 Starting worker process       │
└─────────────────────────────────────────────────────────┘
```

**Tasks:**
- [x] Expand/collapse single entry
  - [x] Keybinding: `Space` to toggle expand
  - [x] Word-wrap long lines in expanded view
  - [x] Visual background to distinguish expanded content
- [ ] JSON detection and formatting
  - [ ] Auto-detect JSON content in log line
  - [ ] Pretty-print with indentation
  - [ ] Syntax highlighting for JSON (keys, values, types)
- [x] Multiple expanded entries
  - [x] Allow multiple entries expanded simultaneously
  - [x] Collapse all keybinding (`c`)
- [ ] Scrolling within expanded content
  - [ ] Handle very large expanded content (huge JSON)
  - [ ] Nested scrolling or pagination
- [ ] Copy expanded content
  - [ ] `y` to yank/copy expanded content to clipboard
- [x] Add tests

**Display Modes (per entry):**
- **Raw**: Word-wrapped full text (default for non-JSON)
- **JSON**: Pretty-printed with syntax highlighting
- **Auto**: Detect format and choose appropriate mode

**Future:**
- [ ] Collapsible JSON nodes (expand/collapse nested objects)
- [ ] Table view for structured logs
- [ ] Custom formatters for known log formats

---

#### Stats Panel (Left Column)
**Goal:** Show log statistics in the left panel below the source list

**UI Layout:**
```
┌──────────────────────┬─────────────────────────────────────────┐
│ Sources              │ [log content]                           │
│──────────────────────│                                         │
│ > api.log            │ 142  INFO  Starting server...          │
│   worker.log         │ 143  DEBUG Connected to database       │
│   db.log             │ 144  ERROR Failed to connect      ← red│
│                      │                                         │
│──────────────────────│                                         │
│ Stats                │                                         │
│──────────────────────│                                         │
│ Lines:      1,234    │                                         │
│ Filtered:     892    │                                         │
│                      │                                         │
│ ERROR          12    │                                         │
│ WARN           45    │                                         │
│ INFO          892    │                                         │
│ DEBUG         285    │                                         │
└──────────────────────┴─────────────────────────────────────────┘
```

**Features:**
- Total line count and filtered count
- Severity breakdown with counts (requires severity detection)
- Updates in real-time as file changes or filter applied
- Clickable severity levels to quick-filter (future)

**Tasks:**
- [x] Stats panel UI component
  - [x] Render below source list in left panel
  - [x] Show total lines / filtered lines
  - [ ] Collapsible section
- [x] Basic stats tracking
  - [x] Line counts per tab
  - [x] Update on file reload
  - [x] Update on filter change
- [ ] Severity stats (after severity detection is implemented)
  - [ ] Count per severity level
  - [ ] Color-coded display
  - [ ] Click to filter by severity

**Current Status:** Basic stats (line counts) implemented. Severity stats pending.

**Dependencies:** Severity stats require Log Format Detection feature

---

#### Log Format Detection & Severity Parsing
**Goal:** Automatically detect log format and extract severity for highlighting and filtering

**Severity Levels (standardized):**
```
TRACE → DEBUG → INFO → WARN → ERROR → FATAL
```

**Detection Sources:**

| Format | Example | Severity Extraction |
|--------|---------|---------------------|
| JSON | `{"level":"error","msg":"..."}` | Parse `level`, `severity`, `lvl` fields |
| Bracket | `[ERROR] Failed to connect` | Match `[LEVEL]` pattern |
| Prefix | `ERROR: Connection refused` | Match `LEVEL:` pattern |
| Syslog | `<3>Jan 20 10:00:01 app[123]: msg` | Parse priority code |
| Log4j | `2024-01-20 ERROR com.app - msg` | Match known patterns |
| Kubernetes | `E0120 10:00:01.123 file.go:42]` | First char: I/W/E/F |

**UI Integration (Left Panel):**
```
┌──────────────┬──────────────────────────────────────────────────────┐
│ Sources      │ [log content]                                        │
│──────────────│                                                      │
│ > api.log    │ 142  INFO  Starting server...                       │
│   worker.log │ 143  DEBUG Connected to database                    │
│   db.log     │ 144  ERROR Failed to authenticate             ← red │
│              │ 145  WARN  Retry attempt 2/3                  ← yel │
│──────────────│ 146  INFO  Request processed                        │
│ Severity     │                                                      │
│──────────────│                                                      │
│ ○ FATAL    0 │                                                      │
│ ● ERROR   12 │ ← active filter                                      │
│ ○ WARN    45 │                                                      │
│ ○ INFO   892 │                                                      │
│ ○ DEBUG  234 │                                                      │
│──────────────│──────────────────────────────────────────────────────│
│ [Bookmarks]  │ Filter: database              Showing 12/1183 ⟳ 100% │
└──────────────┴──────────────────────────────────────────────────────┘
```

**Severity Section Features:**
- Severity levels with counts (from current source)
- Toggle filtering: click/select to show only that level and above
- `●` indicates active filter, `○` indicates inactive
- Counts update as text filter changes
- Keybinding to cycle severity filter (e.g., `s` to cycle through levels)

**Tasks:**
- [ ] Format detection
  - [ ] Detect JSON lines (starts with `{`, valid JSON)
  - [ ] Detect common text patterns (bracket, prefix, syslog)
  - [ ] Cache detected format per source (don't re-detect every line)
  - [ ] Allow manual override per source
- [ ] Severity parsing
  - [ ] JSON: check common fields (`level`, `severity`, `lvl`, `log.level`)
  - [ ] Text: regex patterns for common formats
  - [ ] Normalize to standard levels (TRACE/DEBUG/INFO/WARN/ERROR/FATAL)
  - [ ] Handle case variations (error, ERROR, Error)
- [ ] Severity highlighting
  - [ ] Color-code by severity (configurable colors)
  - [ ] ERROR/FATAL: red
  - [ ] WARN: yellow
  - [ ] INFO: default
  - [ ] DEBUG/TRACE: dim/gray
- [ ] Severity filtering
  - [ ] Quick filter: show ERROR and above
  - [ ] Keybinding to cycle minimum severity level
  - [ ] Combine with text filter (e.g., filter "database" + ERROR)
- [ ] Severity statistics
  - [ ] Count per severity level
  - [ ] Show in side panel per source
  - [ ] Click to filter by severity
- [ ] Add tests for format detection and parsing

**Future:**
- [ ] Custom format definitions (regex-based)
- [ ] Timestamp parsing from detected format
- [ ] Auto-detect field names for structured logs

---

#### Persist Filter History to Disk ✅
**Goal:** Save filter history between sessions

**Tasks:**
- [x] Add history file path (~/.config/lazytail/history.json)
- [x] Load history on startup
- [x] Save history after each filter submission
- [x] Handle file read/write errors gracefully
- [x] Add tests for persistence

**Current Status:** ✅ Complete

**Benefits:**
- Persistent workflow across sessions
- Better UX for repeated log analysis

---

### 🟢 LOW PRIORITY

#### Search Highlighting
**Goal:** Highlight filter matches in displayed text

**Tasks:**
- [ ] Detect filter pattern in rendered lines
- [ ] Apply highlight style to matching substrings
- [ ] Handle case sensitivity in highlighting
- [ ] Support regex pattern highlighting
- [ ] Add tests with mock rendering
- [ ] Make highlight colors configurable

**Benefits:**
- Visual feedback for matches
- Easier to spot relevant content
- Common feature in log viewers

---

#### Structured Logging
**Goal:** Replace eprintln! with proper logging framework

**Tasks:**
- [ ] Add env_logger or tracing dependency
- [ ] Replace eprintln! calls with log macros
- [ ] Add log levels (debug, info, warn, error)
- [ ] Document RUST_LOG usage in README
- [ ] Add logging to troubleshooting section

**Benefits:**
- Better debugging experience
- Controllable verbosity
- Production-ready error reporting

---

## Future Ideas (Backlog)

### Performance & Scalability
- [x] Streaming filter with mmap for large files
- [x] SIMD-accelerated search using memchr/memmem
- [x] Grep-style lazy line counting for case-sensitive search
- [ ] Performance profiling on very large files (100GB+)
- [ ] Optimize ANSI parsing (cache parsed lines?)
- [ ] Benchmark filtering performance
- [ ] Further optimize case-insensitive search

### Features
- [ ] JSON log parsing and formatted view
  - Detect JSON lines automatically
  - Pretty-print JSON in dedicated view mode
  - Filter by JSON field values
- [ ] Multiple display modes
  - Raw view (current)
  - Compact view (truncate long lines)
  - JSON formatted view
  - Table view (for structured logs)
- [ ] Bookmarks (mark lines for quick navigation)
- [ ] Export filtered results to file
- [ ] Copy selected line to clipboard
- [ ] Timestamp parsing and time-based filtering
  - Detect common timestamp formats
  - Filter by time range
  - Jump to specific timestamp
- [ ] Theme customization
- [ ] Merged/chronological view for multiple sources
  - Parse timestamps from all sources
  - Display merged timeline
  - Color-code by source

### Developer Experience
- [ ] Integration tests for full app behavior
- [ ] UI snapshot testing
- [ ] Performance benchmarks in CI
- [ ] Release automation improvements
- [ ] Pre-built binaries for Windows

---

## Release Planning

### v0.2.0 ✅ (Released)
**Focus: Multi-Tab Support**
- Multiple files as CLI arguments: `lazytail a.log b.log c.log`
- Side panel UI with source list (tree-structure ready)
- Navigation (`Tab`, `Shift+Tab`, `1-9`)
- Per-tab state (filter, scroll, follow mode)
- File watching for all open files
- Stdin support: `cmd | lazytail`

### v0.3.0 ✅ (Released)
**Focus: Advanced Filter Modes**
- Tab to switch between Plain/Regex filter modes
- Visual mode indicator (different frame colors)
- Case sensitivity toggle (Alt+C)
- History stores mode per entry
- Mode switches automatically when navigating history
- Invalid regex visual feedback (red frame)
- Expandable log entries (Space to toggle, c to collapse all)
- Default follow mode on file open
- Stats panel (line counts)
- Persistent filter history to disk

### v0.4.0 (Next)
**Focus: Source Discovery & Capture**
- Auto-discover sources from `~/.config/lazytail/data/`
- Source capture mode: `cmd | lazytail -n "Name"`
- Active/ended status indicators
- Dynamic tab creation for new sources

### v0.5.0 (Future)
**Focus: Search & Highlighting**
- Search highlighting in results
- Filter across all tabs simultaneously

### v0.6.0 (Future)
**Focus: Log Intelligence**
- JSON log parsing and formatted view in expanded entries
- Timestamp parsing and time-based filtering
- Severity detection and filtering

### v1.0.0 (Future)
**Focus: Feature Complete & Stable**
- All core features stable and documented
- Merged chronological view
- Performance optimizations
- Comprehensive test coverage

---

## Development Workflow

### Before Starting a Feature
1. Update this roadmap with detailed tasks
2. Consider impact on existing tests
3. Plan for backward compatibility
4. Review CLAUDE.md for implementation guidance

### During Development
1. Write tests first (TDD when appropriate)
2. Run pre-commit checks frequently
3. Keep commits focused and atomic
4. Update documentation as you go

### Before Completion
1. All tests pass (cargo test)
2. Clippy clean (cargo clippy -- -D warnings)
3. Formatted (cargo fmt)
4. Documentation updated
5. Roadmap updated to mark task complete

---

## Notes

- This roadmap is a living document - update as priorities change
- Focus on one major feature at a time
- Keep production stability as top priority
- User feedback will shape future direction
