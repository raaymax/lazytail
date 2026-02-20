# LazyTail Roadmap

This is a local planning document for upcoming features and improvements.

---

## Current Status (v0.6.0)

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

**v0.4.0 Features:**
- Source discovery mode (`lazytail` with no args)
- Source capture mode (`cmd | lazytail -n "Name"`)
- Active/ended status indicators for discovered sources
- Directory watcher for dynamic tab creation
- Close tab with confirmation dialog (`x` / `Ctrl+W`)
- MCP server support (`lazytail --mcp`)
- MCP tools: `list_sources`, `get_lines`, `get_tail`, `search`, `get_context`
- Streaming filter optimization for MCP (grep-like performance on 5GB+ files)

**v0.5.0 Features:**
- Config system with `lazytail.yaml` discovery (walk parent directories)
- `lazytail init` and `lazytail config {validate,show}` subcommands
- Project-scoped and global source definitions in config
- Query language: `json | field == "value"` syntax in filter input
- MCP query language integration (JSON and text syntax converge on shared AST)
- MCP plain text output format (default) to reduce JSON escaping overhead
- Display file path in header and `y` to copy source path
- Project-local data directories (`.lazytail/`)

**v0.6.0 Features:**
- Columnar index system with severity detection
- Index-accelerated filtering with bitmap pre-filtering
- Severity-based line coloring (ERROR/WARN/INFO/DEBUG)
- Severity histogram in stats panel
- Line count and file size per source in side panel
- MCP `get_stats` tool with index metadata and severity breakdown
- Incremental index building during capture mode
- O(1) line access via mmap-backed columnar offsets

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
- [x] Show total line count and file size per source in side panel (live-updating as file grows) — ✅ v0.6.0
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

#### Phase 2: Source Discovery ✅
**Goal:** Auto-discover log sources from config directory

**Status:** Complete

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
- [x] Config directory setup
  - [x] Create `~/.config/lazytail/data/` on first run
  - [x] Create `~/.config/lazytail/sources/` on first run
- [x] Source discovery (UI mode)
  - [x] Scan `data/` directory for `.log` files
  - [x] Check `sources/` for active markers (file exists + PID valid)
  - [x] Display discovered sources as tabs
  - [x] Show active/ended status indicator per tab
- [x] Watch for new sources
  - [x] Monitor `data/` directory for new files
  - [x] Add new tabs dynamically when sources appear
- [x] Tab management
  - [x] Close tab keybinding (`x` or `Ctrl+W`) with confirmation dialog
  - [x] Delete ended source files on close (after confirmation)
- [x] Add tests for discovery behavior

**Behavior:**
- `lazytail` (no args) → discover mode, show all sources from config dir
- `lazytail file.log` → explicit mode, show only that file
- `lazytail file1.log file2.log` → explicit mode, show those files

---

#### Phase 3: Source Capture Mode (Tee-like) ✅
**Goal:** Capture stdin to named source, viewable in UI

**Status:** Complete

```bash
# Capture logs from any command
cmd | lazytail -n "API"
lazytail -n "API" <(kubectl logs -f pod)

# Works like:
# cmd | tee ~/.config/lazytail/data/API.log
# + register in sources/ + collision check + header
```

**Tasks:**
- [x] CLI argument parsing
  - [x] `-n <name>` flag for source mode
  - [x] Detect stdin input
- [x] Source mode implementation
  - [x] Name collision detection (check marker + PID validity)
  - [x] Create marker file in `sources/` with PID
  - [x] Print header: `Serving "API" → ~/.config/lazytail/data/API.log`
  - [x] Read stdin line by line
  - [x] Write to log file (append)
  - [x] Echo to stdout (tee behavior)
  - [x] On EOF: remove marker, exit (file persists)
- [x] Signal handling
  - [x] Handle SIGINT/SIGTERM gracefully
  - [x] Clean up marker file on exit
- [x] Error handling
  - [x] Exit with error if name collision
  - [x] Handle write errors gracefully
- [x] Add tests for source mode

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
- [ ] `lazytail -n` should truncate (reset) existing log file by default instead of appending
  - Current behavior: appends to existing log file, accumulating stale data across runs
  - New default: truncate the file on start so each capture session begins fresh
  - Add `--append` / `-a` flag to preserve existing contents (opt-in)
- [ ] `--file <path>` for custom log file location
- [ ] `--max-size <size>` for log rotation
- [ ] Memory-only mode with streaming (no file)
- [ ] Merged chronological view across sources
- [ ] Filter across all tabs simultaneously

---

#### Phase 5: Query Language (LogQL-style) 🔴 HIGHEST PRIORITY
**Goal:** Unified pipeline-based query language for filtering, time ranges, and aggregation - with dual input formats (text for UI, JSON for MCP/LLMs)

**Architecture:**
```
┌─────────────────────┐      ┌─────────────────────┐
│  Text Query (UI)    │      │  JSON Query (MCP)   │
│                     │      │                     │
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

**Key Insight:** MCP tool parameters ARE the query language for LLMs. Design rich structured JSON parameters that compile to the same AST as text queries.

**Text Syntax (for humans):**
```bash
# Field filtering
json | level == "error" | service =~ "api|worker"

# Exclusion (critical for noisy logs)
json | level == "error" | msg !~ "kscreen|systemd"

# Time filtering
json | time > "2024-01-28T10:00:00" | time < "2024-01-28T11:00:00"

# Aggregation
json | level == "error" | count by (service)
json | count by (level) | top 10
```

**JSON Syntax (for MCP/LLMs):**
```json
{
  "parser": "json",
  "filters": [
    {"field": "level", "op": "==", "value": "error"},
    {"field": "service", "op": "=~", "value": "api|worker"}
  ],
  "exclude": [
    {"field": "msg", "pattern": "kscreen|systemd"}
  ],
  "time_range": {
    "field": "timestamp",
    "after": "2024-01-28T10:00:00",
    "before": "2024-01-28T11:00:00"
  },
  "aggregate": {
    "count_by": "service",
    "limit": 10
  }
}
```

**Pipeline Stages:**
| Stage | Text Syntax | JSON Field | Description |
|-------|-------------|------------|-------------|
| Parser | `json`, `logfmt`, `pattern "..."` | `parser` | Extract fields from line |
| Filter | `field == "value"` | `filters[]` | Include matching lines |
| Exclude | `field !~ "pattern"` | `exclude[]` | Remove matching lines |
| Time | `time > "..."` | `time_range` | Filter by timestamp |
| Aggregate | `count by (field)` | `aggregate` | Group and count |
| Limit | `top N` | `aggregate.limit` | Limit results |

**Operators:**
| Operator | Description | Example |
|----------|-------------|---------|
| `==`, `!=` | Equality | `level == "error"` |
| `=~`, `!~` | Regex match/exclude | `msg !~ "kscreen"` |
| `>`, `<`, `>=`, `<=` | Comparison (numeric/time) | `status >= 500` |
| `contains` | Substring match | `msg contains "timeout"` |

**FilterQuery AST (Rust):**
```rust
struct FilterQuery {
    parser: Parser,                    // json, logfmt, pattern, raw
    filters: Vec<FieldFilter>,         // field op value
    exclude: Vec<ExcludePattern>,      // negative filters
    time_range: Option<TimeRange>,     // after/before timestamps
    aggregate: Option<Aggregation>,    // count_by, limit
}

enum Parser {
    Raw,                               // plain text (default)
    Json,                              // parse as JSON
    Logfmt,                            // parse key=value
    Pattern(String),                   // extract via pattern
}

struct FieldFilter {
    field: String,                     // e.g., "level" or "user.id"
    op: Operator,                      // ==, !=, =~, !~, >, <, etc.
    value: Value,                      // string, number, regex
}

struct Aggregation {
    count_by: Option<String>,          // group by field
    limit: Option<usize>,              // top N
}
```

**Implementation Order (MCP-first):**
1. **Define AST structs** with serde derives — ✅ v0.5.0
2. **Build executor** that processes FilterQuery — ✅ v0.5.0
3. **JSON deserialization** → MCP tools work immediately — ✅ v0.5.0
4. **Text parser** → UI gets query language later — ✅ v0.5.0

**Tasks:**
- [x] Phase 1: Core AST & JSON Interface (MCP) — ✅ v0.5.0
  - [x] Define `FilterQuery` and related structs with `#[derive(Deserialize)]`
  - [x] Implement executor for basic filters (`==`, `!=`, `=~`, `!~`)
  - [x] JSON parser support (serde_json field extraction)
  - [x] Wire up to MCP `search` tool as `query` parameter
  - [x] Tests with JSON input
- [x] Phase 2: Exclusion & Time Filtering — ✅ v0.5.0 (partial)
  - [x] Implement exclude patterns (critical for noisy logs!)
  - [ ] Timestamp field detection (common field names)
  - [ ] Time range filtering (after/before)
  - [x] Tests for exclusion filtering
- [ ] Phase 3: Aggregation 🔴 HIGH PRIORITY
  - [ ] Implement `count by (field)` — e.g. `json | level == "error" | count by (service)` → "which service has the most errors?"
  - [ ] Implement `top N` / limit
  - [ ] Multiple group_by fields — e.g. `count by (service, level)`
  - [ ] Return aggregation results as structured JSON (field → count map)
  - [ ] Wire into MCP: either extend `search` response or add dedicated `aggregate` tool
  - [ ] Wire into text query parser (already has AST slots for aggregation)
  - Real-world motivation: currently requires multiple manual queries to answer "which service has the most errors?" or "what's the error distribution by field?" — a single `count by` would replace N manual queries
- [x] Phase 4: Text Parser (UI) — ✅ v0.5.0
  - [x] Lexer for text query syntax
  - [x] Recursive descent parser → AST
  - [x] Error messages with position info
  - [x] UI integration (filter input mode)
- [x] Phase 5: Advanced Parsers — ✅ v0.5.0
  - [x] `logfmt` parser (key=value)
  - [ ] `pattern` parser (extract fields via template)
  - [x] Nested field access (`user.id`, `request.headers.host`)
- [ ] Phase 6: Polish
  - [ ] Syntax highlighting in filter input
  - [ ] LogQL `format` stage — render structured fields into a custom display template
    - Text syntax: `json | format <severity> - <method> <url> - <status>`
    - JSON syntax: `"format": "<severity> - <method> <url> - <status>"`
    - Extracts fields from parsed log line and interpolates into template
    - Unresolved fields render as empty or `<missing>`
    - Useful in TUI for readable views of dense JSON/logfmt lines
    - Useful in MCP for agents requesting specific field projections
  - [ ] Query history with mode
  - [ ] Documentation and examples

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
- [x] Severity stats — ✅ v0.6.0
  - [x] Count per severity level
  - [x] Color-coded display
  - [ ] Click to filter by severity

**Current Status:** ✅ Complete (v0.6.0) — stats panel shows line counts and severity histogram with color-coded display

---

#### Log Format Detection & Severity Parsing ✅
**Goal:** Automatically detect log format and extract severity for highlighting and filtering

**Status:** ✅ Complete (v0.6.0) — columnar index system with byte-level severity detection

**Severity Levels (standardized):**
```
TRACE → DEBUG → INFO → WARN → ERROR → FATAL
```

**Detection Sources:**

| Format | Example | Severity Extraction | Status |
|--------|---------|---------------------|--------|
| JSON | `{"level":"error","msg":"..."}` | Parse `level`, `severity`, `lvl` fields | ✅ v0.6.0 |
| Bracket | `[ERROR] Failed to connect` | Match `[LEVEL]` pattern | ✅ v0.6.0 |
| Prefix | `ERROR: Connection refused` | Match `LEVEL:` pattern | ✅ v0.6.0 |
| Syslog | `<3>Jan 20 10:00:01 app[123]: msg` | Parse priority code | ✅ v0.6.0 |
| Log4j | `2024-01-20 ERROR com.app - msg` | Match known patterns | ✅ v0.6.0 |
| Kubernetes | `E0120 10:00:01.123 file.go:42]` | First char: I/W/E/F | ✅ v0.6.0 |

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
- [x] Format detection — ✅ v0.6.0
  - [x] Detect JSON lines (starts with `{`, valid JSON)
  - [x] Detect common text patterns (bracket, prefix, syslog)
  - [x] Per-line flag detection cached in columnar index
  - [ ] Allow manual override per source (config hints)
- [x] Severity parsing — ✅ v0.6.0
  - [x] JSON: check common fields (`level`, `severity`, `lvl`, `log.level`)
  - [x] Text: byte-level patterns for common formats
  - [x] Normalize to standard levels (TRACE/DEBUG/INFO/WARN/ERROR/FATAL)
  - [x] Handle case variations (error, ERROR, Error)
- [x] Severity highlighting — ✅ v0.6.0
  - [x] Color-code by severity (configurable colors)
  - [x] ERROR/FATAL: red
  - [x] WARN: yellow
  - [x] INFO: default
  - [x] DEBUG/TRACE: dim/gray
- [ ] Severity filtering
  - [ ] Quick filter: show ERROR and above
  - [ ] Keybinding to cycle minimum severity level
  - [x] Combine with text filter via query language
- [x] Severity statistics — ✅ v0.6.0
  - [x] Count per severity level
  - [x] Show in side panel per source
  - [ ] Click to filter by severity
- [x] Add tests for format detection and parsing — ✅ v0.6.0

**Future:**
- [ ] Custom format definitions (regex-based) in config
- [ ] Timestamp parsing from detected format
- [ ] Auto-detect field names for structured logs
- [x] Columnar index with flags, offsets, checkpoints — ✅ v0.6.0
- [x] Index-accelerated filtering with bitmap pre-filtering — ✅ v0.6.0

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

#### Mouse Controls
**Goal:** Expand mouse support beyond scroll — make the UI fully mouse-interactive

**Tasks:**
- [ ] Click to select a log line
- [ ] Click source in side panel to switch tabs
- [ ] Click severity levels in stats panel to filter (when severity stats land)
- [ ] Click-and-drag to select text for copying
- [ ] Right-click context menu (expand, copy, filter by selection)
- [ ] Resize side panel by dragging the divider
- [ ] Double-click to expand/collapse a log line

**Benefits:**
- Lower barrier to entry for non-vim users
- Faster interaction for common actions (tab switching, line selection)
- Expected by users coming from GUI log viewers

---

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

#### Structured Logging & Debug Instrumentation 🔴 HIGH PRIORITY
**Goal:** Add comprehensive logging to debug what LazyTail is doing internally

**Motivation:** Currently difficult to debug issues like:
- Why is filtering slow on this file?
- Which reader implementation is being used?
- Why did the file watcher trigger?
- What's happening during index builds?
- Why is follow mode not working?
- Performance bottlenecks in the event loop

**Logging Framework:**
- Use `tracing` crate (better than `log` for structured context)
- Support multiple output targets (stderr, file, structured JSON)
- Configurable per-module log levels
- Span-based instrumentation for performance tracing

**Key Areas to Instrument:**

1. **File Operations**
   - Reader selection (FileReader vs HugeFileReader vs StreamReader)
   - File watching events (what changed, how many bytes)
   - Index building progress and timing
   - Mmap operations and failures

2. **Filtering**
   - Filter orchestrator decisions (which engine is used)
   - Filter progress (lines scanned, matches found, elapsed time)
   - Streaming filter vs generic filter selection
   - Query parsing and execution

3. **Event Loop**
   - Event types received and processing time
   - Debouncing decisions
   - Frame timing (render, collect, process)
   - Dropped frames / performance issues

4. **MCP Server**
   - Tool invocations (which tool, parameters)
   - Query execution time
   - Result sizes
   - Errors and failures

5. **Capture Mode**
   - Lines captured per second
   - Flush events
   - Signal handling
   - Marker file operations

**Log Levels:**
- `ERROR`: Failures that impact functionality
- `WARN`: Degraded performance, recoverable errors
- `INFO`: Major operations (file opened, filter applied, source added)
- `DEBUG`: Detailed operation info (event types, state transitions)
- `TRACE`: Verbose instrumentation (every line read, every event)

**Configuration:**
```bash
# Enable debug logs for filter module
RUST_LOG=lazytail::filter=debug lazytail app.log

# Enable trace for everything
RUST_LOG=trace lazytail app.log

# Log to file
RUST_LOG=debug lazytail app.log 2> debug.log

# Structured JSON output
RUST_LOG_FORMAT=json RUST_LOG=debug lazytail --mcp
```

**Performance Tracing:**
```rust
use tracing::{info_span, instrument};

#[instrument(skip(reader))]
fn apply_filter(reader: &dyn LogReader, filter: Arc<dyn Filter>) {
    let _span = info_span!("apply_filter", total_lines = reader.total_lines()).entered();
    // ... filtering logic
    // Automatically logs: duration, total_lines, function args
}
```

**Tasks:**
- [ ] Add `tracing` and `tracing-subscriber` dependencies
- [ ] Initialize tracing subscriber in main.rs
  - [ ] Support RUST_LOG env var
  - [ ] Support RUST_LOG_FORMAT (text/json/compact)
  - [ ] Support --log-file flag
- [ ] Instrument core modules
  - [ ] Reader selection and file operations (reader/)
  - [ ] Filter orchestration and execution (filter/)
  - [ ] Event loop and debouncing (main.rs)
  - [ ] Index building (index/builder.rs)
  - [ ] File watching (watcher.rs, dir_watcher.rs)
  - [ ] MCP server (mcp/)
  - [ ] Capture mode (capture.rs)
- [ ] Add span instrumentation for performance-critical paths
  - [ ] Filter execution (per-filter timing)
  - [ ] Index building (progress tracking)
  - [ ] File reading (lines/sec, bytes/sec)
- [ ] Add diagnostic commands
  - [ ] `lazytail --version --verbose` - show build info, feature flags
  - [ ] `lazytail doctor` - check config, permissions, verify setup
- [ ] Document logging in README and troubleshooting guide
  - [ ] How to enable debug logs
  - [ ] Common patterns for debugging issues
  - [ ] Performance profiling with TRACE logs

**Benefits:**
- **Debuggability:** Understand what LazyTail is doing without recompiling
- **Performance analysis:** Find bottlenecks with span timing
- **User support:** Ask users for logs instead of guessing
- **Development:** Faster iteration when debugging issues
- **Production monitoring:** Track MCP server performance

---

## Future Ideas (Backlog)

### Performance & Scalability
- [x] Streaming filter with mmap for large files
- [x] SIMD-accelerated search using memchr/memmem
- [x] Grep-style lazy line counting for case-sensitive search
- [x] MCP search optimized with streaming filter (tested on 5GB+ files)
- [x] FilterProgress::Complete includes lines_processed for accurate tracking
- [x] Columnar index system — ✅ v0.6.0
  - [x] Per-line flags (severity, ANSI, JSON, logfmt, timestamp markers)
  - [x] O(1) line access via mmap-backed offset column
  - [x] Index-accelerated filtering with bitmap pre-filtering
  - [x] Incremental index building during capture mode
  - [x] ~2.5s to index 60M lines (9GB file)
  - [x] ANSI-aware severity detection with memchr-assisted scanning
- [ ] Performance profiling on very large files (100GB+)
- [ ] Optimize ANSI parsing (cache parsed lines?)
- [ ] Benchmark filtering performance
- [ ] Further optimize case-insensitive search

### Features

#### Project-Scoped Instances (lazytail.yaml) ✅
**Goal:** Per-project log sources and configuration, auto-discovered by ancestry

**Status:** Core config system implemented (v0.4.0)

**Discovery Order:**
1. Check current dir and ancestors for `lazytail.yaml`
2. If found → project mode (use `.lazytail/` in that dir)
3. If not found → global mode (`~/.config/lazytail/`)

**Directory Structure:**
```
my-project/
├── lazytail.yaml          # Config (committed to git)
├── .lazytail/             # Data (gitignored)
│   ├── data/              # Captured logs
│   ├── sources/           # Active markers
│   └── history.json       # Project-specific filter history
└── src/
```

**`lazytail.yaml` Example:**
```yaml
# Source definitions (path-based only)
sources:
  - name: Database
    path: /var/log/postgresql/postgresql.log
  - name: App
    path: ./logs/app.log  # Relative to project root
  - name: Nginx
    path: /var/log/nginx/access.log
```

**Benefits:**
- Team shares source definitions via git
- AI assistants (Claude Code) auto-discover project logs
- No pollution of global config
- Project-specific filter history
- Different projects can have different log setups

**Tasks:**
- [x] Config file discovery (walk ancestors for `lazytail.yaml`)
- [x] Parse YAML config with serde
- [x] Create `.lazytail/` directory structure
- [x] Support `path:` sources (watch existing file)
- [x] Relative path resolution from project root
- [ ] Filter presets in config
- [ ] MCP: detect project root and scope sources
- [x] Fallback to global `~/.config/lazytail/` when no project found

---

- [x] Configuration file (`lazytail.yaml` with project + global scope)
  - System-wide and project-scoped log source definitions (name, path)
  - Pre-configured sources appear automatically in discovery mode
  - [ ] Custom source groups/categories
  - [ ] Default filter patterns per source
  - [ ] UI preferences (colors, panel width, default modes)
  - [ ] MCP server settings (enabled tools, access control)
- [ ] `lazytail clear` CLI subcommand
  - Clear all captured log files from the project data directory (`.lazytail/data/` or `~/.config/lazytail/data/`)
  - `lazytail clear` — clear all ended sources
  - `lazytail clear <name>` — clear a specific source by name
  - `lazytail clear --all` — clear all sources including active ones (with confirmation)
  - Respect project scoping (clear project logs when `lazytail.yaml` is present, global otherwise)
  - Confirmation prompt before destructive action (skip with `--yes` / `-y`)
- [ ] `lazytail sources` / `lazytail list` CLI subcommand
  - List all available sources (discovered + config-defined) from the terminal without opening the TUI
  - Show name, path, active/ended status, total lines
  - Useful for scripting, piping into other tools, quick inspection
- [ ] Register sources from CLI and MCP (without piping data)
  - CLI: `lazytail add <name> --path /var/log/app.log` — register an existing log file as a named source
  - MCP: `add_source` tool — let AI agents register sources programmatically (e.g. discover a log path and add it)
  - Currently the only ways to add sources are: pipe via `lazytail -n`, define in `lazytail.yaml`, or place files in data dir
  - This would allow dynamic source management without editing config or restarting captures
- [ ] JSON pretty collapsible viewer in TUI
  - Detect JSON content in log lines automatically
  - Pretty-print with syntax highlighting (keys, values, types)
  - Collapsible/expandable nested objects and arrays (tree-style navigation)
  - Expand/collapse individual nodes with keybindings
  - Integrates with existing line expansion (`Space` to toggle)
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
- [x] Self-update (`lazytail update`) — ✅ v0.7.0
  - Use the `self_update` crate to check GitHub Releases and replace the binary in-place
  - `lazytail update` — check for new version and install if available
  - `lazytail update --check` — check only, don't install (exit code 0 = up to date, 1 = update available)
  - Background update check on TUI startup (non-blocking, cached to `~/.config/lazytail/update_check.json`)
  - Only check once every 24h to avoid API rate limits and startup latency
  - Print subtle notice after TUI exits if update is available (not during — would interfere with ratatui)
  - `--no-update-check` flag and config option (`update_check: false`) to disable automatic checks
  - Respect AUR users: detect if installed via package manager and suggest `yay -S lazytail` instead of self-replacing
  - Feature-gated behind `self-update` cargo feature (included in GitHub release builds, excluded from AUR)
- [ ] TUI colors configuration / theme customization
  - Configurable colors via `lazytail.yaml` (e.g., `theme:` section)
  - Customizable elements: side panel, selected line, status bar, filter input, borders, active/ended indicators
  - Support named colors (`red`, `cyan`) and hex (`#ff5555`)
  - Built-in themes (e.g., default, light, solarized) with option to override individual colors
  - Respect terminal color scheme where possible
- [ ] Keybindings configuration
  - Configurable keybindings via `lazytail.yaml` (e.g., `keybindings:` section)
  - Override default vim-style bindings with custom keys
  - Support modifier keys (`Ctrl`, `Alt`, `Shift`) and key combinations
  - Sensible defaults that work out of the box, customization for power users
- [ ] Merged/chronological view for multiple sources
  - Parse timestamps from all sources
  - Display merged timeline
  - Color-code by source
- [ ] Command-based sources (future consideration)
  - Define sources as commands in config: `command: "docker logs -f api"`
  - LazyTail spawns and manages the process
  - Auto-restart on failure?
  - Security implications (arbitrary command execution)
  - Alternative: keep using `cmd | lazytail -n "Name"` pattern
  - Needs more thought on UX and lifecycle management
- [ ] Tmux-aware capture
  - During `lazytail -n`, detect tmux session via `$TMUX` / `$TMUX_PANE` env vars
  - Store tmux coordinates (session:window.pane) in marker file alongside PID
  - Expose tmux context in `list_sources` response when available
  - No new MCP tools — agent has bash access and can use the info however it sees fit

### Developer Experience
- [ ] Integration tests for full app behavior
- [ ] UI snapshot testing
- [ ] Performance benchmarks in CI
- [x] Release automation improvements — ✅ 2026-02-20
  - [x] Auto-trigger release builds when release-please creates releases
  - [x] Binaries automatically attached to GitHub releases
- [ ] Pre-built binaries for Windows

---

## MCP Tools Roadmap

**Current Tools (v0.6.0):**
| Tool | Purpose | Status |
|------|---------|--------|
| `list_sources` | Discover available log sources | ✅ Complete |
| `get_lines` | Read lines from position | ✅ Complete |
| `get_tail` | Read last N lines | ✅ Complete |
| `search` | Find pattern matches + structured queries | ✅ Complete |
| `get_context` | Get lines around a match | ✅ Complete |
| `get_stats` | Index metadata and severity breakdown | ✅ Complete (v0.6.0) |

**Common Parameters (all tools except `list_sources`):**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `output` | text/json | text | Response format (text reduces escaping for AI) |
| `raw` | bool | false | Keep ANSI escape codes (default strips them) |

**Current `search` Parameters:**
| Parameter | Type | Status |
|-----------|------|--------|
| `source` | String | ✅ Done |
| `pattern` | String | ✅ Done (optional when using `query`) |
| `mode` | plain/regex | ✅ Done |
| `case_sensitive` | bool | ✅ Done |
| `max_results` | usize | ✅ Done |
| `context_lines` | usize | ✅ Done |
| `query` | FilterQuery | ✅ Done (JSON/logfmt field filtering with exclusions) |
| `time_range` | TimeRange | ❌ Missing |

**Planned `search` Enhancements:**
| Feature | Purpose | Priority |
|---------|---------|----------|
| `time_range` param | Filter by timestamp range | 🟡 Medium |
| Search pagination / cursor | When results exceed `max_results` (capped at 1000), there's no cursor or offset to fetch the next page. Currently the only workaround is adding more filters to narrow results. A cursor/offset mechanism would allow iterating through large result sets. | 🟡 Medium |

**Completed `list_sources` Enhancements:**
| Feature | Purpose | Version |
|---------|---------|---------|
| Include total line count per source | Callers shouldn't need a separate call to know source size. Useful for calculating offsets, gauging search scope, etc. | ✅ v0.6.0 (via get_stats) |

**Planned `get_tail` Enhancements:**
| Feature | Purpose | Priority |
|---------|---------|----------|
| `since_line` parameter | Return only lines after a given line number. Enables efficient incremental polling of active sources without re-fetching or deduplicating. Currently monitoring a live source means blind-polling `get_tail` and manually tracking what you've already seen. | 🔴 High |

**Planned `get_lines` Enhancements:**
| Feature | Purpose | Priority |
|---------|---------|----------|
| Negative indexing / "from end" shorthand | Reading last N lines without knowing total_lines first (`get_tail` covers most cases, but minor friction when you need a specific offset from end) | 🟢 Low |

**Completed Tools:**
| Tool | Purpose | Version |
|------|---------|---------|
| `get_stats` | Index metadata, severity breakdown, total lines, file size. Lightweight — reads index metadata only, no content scanning. Helps decide whether to tail or search, and whether a source is healthy. | ✅ v0.6.0 |

**Planned New Tools:**
| Tool | Purpose | Priority |
|------|---------|----------|
| `aggregate` | Count by field, top N results. Answers "which service has the most errors?", "what's the error distribution?" in a single call instead of N manual queries. Should integrate with the query language AST (Phase 3 aggregation) so both text queries and MCP JSON work. | 🔴 High |
| `search_sources` | Search multiple sources at once, grouped results by source name. Essential for cross-service correlation (e.g., "find this request ID across all services"). Doesn't require timestamps or merging — just run the same query across all sources. | 🔴 High |
| `fields` | Sample N lines from a source and return discovered field names, types, and example values. Makes structured queries far more usable — currently consumers must `get_tail` a few lines and visually parse JSON to discover field names before constructing a query. Critical for LLM consumers that can't eyeball the data. | 🔴 High |
| `summarize` | Log overview: time range, top patterns, top services, error rate. Content-analysis based summary. | 🟡 Medium |
| `add_source` | Register an existing log file as a named source. Lets AI agents dynamically add sources without editing config or piping data. CLI equivalent: `lazytail add <name> --path <path>`. | 🟡 Medium |
| `export` | Dump filtered results to a file or return in bulk. Supports query filters, time range, and output format. Useful for "save me all errors from the last hour" workflows. TUI has export in backlog but MCP needs its own path since results are capped at 1000. | 🟢 Low |

**Internal Improvements Done:**
- ✅ Streaming filter with mmap (grep-like performance)
- ✅ SIMD-accelerated search (memchr/memmem)
- ✅ `lines_searched` tracking in FilterProgress::Complete
- ✅ Single-pass content extraction for matched lines
- ✅ Plain text output format (eliminates JSON escaping explosion for AI consumption)

### Upcoming Focus Areas

#### Time Filtering & Aggregation 🔴 HIGH PRIORITY

Already Completed (landed post-v0.4.0):
- ✅ FilterQuery AST with serde derives (JSON interface for MCP)
- ✅ `query` parameter wired into MCP `search` tool
- ✅ Exclusion patterns (`exclude` field in query)
- ✅ `logfmt` parser support
- ✅ Nested field access (`user.id`)
- ✅ Text query parser for UI (`json | level == "error"`)
- ✅ All comparison operators (eq, ne, regex, not_regex, contains, gt, lt, gte, lte)

Remaining:
- Time range filtering (timestamp field detection, after/before)
- Aggregation (`count by (field)`, `top N`)
- Filter presets from config available in MCP

#### Sidecar Index & Combined Sources

Sidecar Index (`.log.idx`):
- Binary index file alongside each captured log
- Store arrival timestamp + byte offset per line
- Append to index in real-time during capture
- Header with validation: file size, mtime, first-4KB hash
- Auto-rebuild on corruption/truncation detection
- Enables time-based operations and merging

Combined Source View:
- Merge multiple sources into single chronological view
- Use sidecar timestamps for captured sources
- Parse timestamps from log content for external files
- Fallback to arrival order for streaming, concatenation for static
- Source-colored lines or `[SOURCE]` prefix
- Filter by source: `source:API`

#### MCP Project Scoping 🟡 MEDIUM PRIORITY

**Goal:** MCP server automatically scopes to the project when `lazytail.yaml` is present

**Design Questions:**
- MCP server should detect `lazytail.yaml` by walking parent directories from CWD (same as TUI)
- When project-scoped: `list_sources` returns project sources + config-defined sources
- When global: `list_sources` returns `~/.config/lazytail/data/` sources (current behavior)
- Sources from `lazytail.yaml` `sources:` definitions should appear alongside captured sources
- Filter presets defined in config should be available via a `list_presets` or similar mechanism
- Consider: should MCP expose both project and global sources, or only project when scoped?

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
