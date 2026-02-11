# LazyTail Roadmap

This is a local planning document for upcoming features and improvements.

---

## Current Status (v0.4.0)

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
- Close tab keybinding (`x` / `Ctrl+W`)
- MCP server support (`lazytail --mcp`)
- MCP tools: `list_sources`, `get_lines`, `get_tail`, `search`, `get_context`
- MCP plain text output format (default) to reduce JSON escaping overhead
- Streaming filter optimization for MCP (grep-like performance on 5GB+ files)
- Config system with `lazytail.yaml` discovery (walk parent directories)
- `lazytail init` and `lazytail config {validate,show}` subcommands
- Project-scoped and global source definitions in config
- Query language basics: `json | field == "value"` syntax in filter input

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
  - [x] Close tab keybinding (`x` or `Ctrl+W`)
  - [ ] Optionally delete source file on close
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
1. **Define AST structs** with serde derives
2. **Build executor** that processes FilterQuery
3. **JSON deserialization** → MCP tools work immediately
4. **Text parser** → UI gets query language later

**Tasks:**
- [ ] Phase 1: Core AST & JSON Interface (MCP)
  - [ ] Define `FilterQuery` and related structs with `#[derive(Deserialize)]`
  - [ ] Implement executor for basic filters (`==`, `!=`, `=~`, `!~`)
  - [ ] JSON parser support (serde_json field extraction)
  - [ ] Wire up to MCP `search` tool as `query` parameter
  - [ ] Tests with JSON input
- [ ] Phase 2: Exclusion & Time Filtering
  - [ ] Implement exclude patterns (critical for noisy logs!)
  - [ ] Timestamp field detection (common field names)
  - [ ] Time range filtering (after/before)
  - [ ] Tests for exclusion and time filtering
- [ ] Phase 3: Aggregation
  - [ ] Implement `count by (field)`
  - [ ] Implement `top N` / limit
  - [ ] Return aggregation results as structured JSON
  - [ ] New MCP tool or extend search response
- [ ] Phase 4: Text Parser (UI)
  - [ ] Lexer for text query syntax
  - [ ] Recursive descent parser → AST
  - [ ] Error messages with position info
  - [ ] UI integration (filter input mode)
- [ ] Phase 5: Advanced Parsers
  - [ ] `logfmt` parser (key=value)
  - [ ] `pattern` parser (extract fields via template)
  - [ ] Nested field access (`user.id`, `request.headers.host`)
- [ ] Phase 6: Polish
  - [ ] Syntax highlighting in filter input
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
- [x] MCP search optimized with streaming filter (tested on 5GB+ files)
- [x] FilterProgress::Complete includes lines_processed for accurate tracking
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
- [ ] Command-based sources (future consideration)
  - Define sources as commands in config: `command: "docker logs -f api"`
  - LazyTail spawns and manages the process
  - Auto-restart on failure?
  - Security implications (arbitrary command execution)
  - Alternative: keep using `cmd | lazytail -n "Name"` pattern
  - Needs more thought on UX and lifecycle management

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

### v0.4.0 ✅ (Released)
**Focus: Source Discovery, Capture, MCP Server & Config System**

Source Discovery & Capture:
- Auto-discover sources from `~/.config/lazytail/data/` and `lazytail.yaml`
- Source capture mode: `cmd | lazytail -n "Name"`
- Active/ended status indicators
- Dynamic tab creation for new sources
- Close tab keybinding (`x` / `Ctrl+W`)

MCP Server Support:
- MCP (Model Context Protocol) server via `--mcp` flag
- Tools: `list_sources`, `get_lines`, `get_tail`, `search`, `get_context`
- Plain text output format (default) to reduce JSON escaping for AI consumption
- MCP enabled by default in all builds
- Streaming filter for grep-like performance on large files
- Tested on 5GB+ log files

Config System:
- `lazytail.yaml` discovery (walk parent directories from CWD)
- Project-scoped (`.lazytail/`) and global (`~/.config/lazytail/`) data directories
- Source definitions with name + path in config
- `lazytail init` to create config interactively
- `lazytail config validate` and `lazytail config show` subcommands
- Query language basics: `json | field == "value"` filter syntax

---

#### MCP Tools Roadmap

**Current Tools (v0.4.0):**
| Tool | Purpose | Status |
|------|---------|--------|
| `list_sources` | Discover available log sources | ✅ Complete |
| `get_lines` | Read lines from position | ✅ Complete |
| `get_tail` | Read last N lines | ✅ Complete |
| `search` | Find pattern matches | ✅ Complete (basic) |
| `get_context` | Get lines around a match | ✅ Complete |

**Common Parameters (all tools except `list_sources`):**
| Parameter | Type | Default | Description |
|-----------|------|---------|-------------|
| `output` | text/json | text | Response format (text reduces escaping for AI) |
| `raw` | bool | false | Keep ANSI escape codes (default strips them) |

**Current `search` Parameters:**
| Parameter | Type | Status |
|-----------|------|--------|
| `file` | PathBuf | ✅ Done |
| `pattern` | String | ✅ Done |
| `mode` | plain/regex | ✅ Done |
| `case_sensitive` | bool | ✅ Done |
| `max_results` | usize | ✅ Done |
| `context_lines` | usize | ✅ Done |
| `exclude` | Vec<String> | ❌ Missing |
| `time_range` | TimeRange | ❌ Missing |
| `query` | FilterQuery | ❌ Missing |

**Planned `search` Enhancements (v0.5.0):**
| Feature | Purpose | Priority |
|---------|---------|----------|
| `exclude` param | Filter out noise (e.g., kscreen spam) | 🔴 High |
| `query` param | Full FilterQuery JSON for field filtering | 🔴 High |
| `time_range` param | Filter by timestamp range | 🟡 Medium |

**Planned New Tools (v0.5.0+):**
| Tool | Purpose | Priority |
|------|---------|----------|
| `summarize` | Log overview: line count, time range, top patterns, top services | 🟡 Medium |
| `search_sources` | Search multiple sources at once, grouped results | 🟡 Medium |
| `aggregate` | Count by field, top N results | 🟡 Medium |

**Internal Improvements Done:**
- ✅ Streaming filter with mmap (grep-like performance)
- ✅ SIMD-accelerated search (memchr/memmem)
- ✅ `lines_searched` tracking in FilterProgress::Complete
- ✅ Single-pass content extraction for matched lines
- ✅ Plain text output format (eliminates JSON escaping explosion for AI consumption)

### v0.5.0 (Next) 🔴 HIGH PRIORITY
**Focus: Query Language & MCP Enhancements**

Query Language Expansion:
- FilterQuery AST with serde derives (JSON interface for MCP)
- Exclusion patterns (critical for noisy logs)
- Time range filtering
- `logfmt` parser support

MCP Enhancements:
- MCP scoped to project root (detect `lazytail.yaml`)
- Filter presets from config available in MCP
- `exclude` parameter for search tool
- `query` parameter for structured field filtering via MCP

### v0.6.0 (Future)
**Focus: Sidecar Index & Combined Sources**

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

### v0.7.0 (Future)
**Focus: Query Language - Time & Aggregation**
- Timestamp field detection and parsing
- Time range filtering (after/before)
- `count by (field)` aggregation
- `top N` limiting
- Multi-source search tool

### v0.8.0 (Future)
**Focus: Query Language - Text Parser (UI)**
- Text query syntax: `json | level == "error"`
- Recursive descent parser
- UI integration with syntax highlighting
- Query history with mode persistence

### v0.9.0 (Future)
**Focus: Log Intelligence**
- `logfmt` and `pattern` parsers
- Nested field access (`user.id`)
- Severity detection and filtering
- JSON formatting in expanded view

### v1.0.0 (Future)
**Focus: Feature Complete & Stable**
- All core features stable and documented
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
