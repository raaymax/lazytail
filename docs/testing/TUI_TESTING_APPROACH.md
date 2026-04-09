# TUI Testing Approach

Manual TUI testing via tmux — enabling AI agents to interact with the full LazyTail terminal UI directly.

**Important: Do NOT write test scripts.** Test the TUI manually by sending keystrokes and reading screen captures interactively. Each test is a sequence of send-keys + capture-pane calls made directly from the agent, not a bash script. This keeps testing adaptive — you can react to unexpected state, investigate issues, and adjust your approach in real time.

## Why tmux?

LazyTail is a ratatui-based TUI. Standard unit tests cover domain logic, but verifying the interactive experience — navigation, filter input, aggregation rendering, side panel behavior — requires driving the real application in a real terminal.

tmux gives us a feedback loop:

1. **Start** the app in a tmux session (use an existing one or create with `tmux new-session -d`)
2. **Send keystrokes** via `tmux send-keys`
3. **Capture the screen** via `tmux capture-pane -p` (returns plain text)
4. **Read the output**, decide what to do next
5. **Repeat** — adapt based on what you see

Zero changes to LazyTail required. Works with the release binary as-is.

## Prerequisites

- `tmux` (tested with 3.5+)
- Built binary: `./target/release/lazytail`
- Test data: `test_logs.jsonl` (30 JSON lines) and/or captured sources in `.lazytail/data/`

## Core Primitives

### Session Management

```bash
SESSION="tui-test"

# Start app in detached session with fixed dimensions
tmux new-session -d -s $SESSION -x 120 -y 35 "./target/release/lazytail test_logs.jsonl"
sleep 1  # wait for startup

# Clean up
tmux kill-session -t $SESSION 2>/dev/null
```

Fixed dimensions (`-x 120 -y 35`) ensure consistent layout across runs.

### Sending Input

```bash
# Simple keys
tmux send-keys -t $SESSION j          # press j
tmux send-keys -t $SESSION Enter       # press Enter
tmux send-keys -t $SESSION Escape      # press Escape

# Literal text (bypasses key interpretation)
tmux send-keys -t $SESSION -l 'error'  # type "error" character by character

# Control sequences
tmux send-keys -t $SESSION C-e         # Ctrl+E
tmux send-keys -t $SESSION C-w         # Ctrl+W
```

### Capturing Screen State

```bash
# Capture entire pane as plain text
OUT=$(tmux capture-pane -t $SESSION -p)

# Check content
echo "$OUT" | grep "Matches: 386"
echo "$OUT" | tail -4    # status bar area
echo "$OUT" | head -3    # header area
```

### Verifying State

Don't write assertion functions — just read the captured output and check it yourself:

```bash
# Capture and check visually
tmux capture-pane -t $SESSION -p | tail -5    # status bar area
tmux capture-pane -t $SESSION -p | head -3    # header area
tmux capture-pane -t $SESSION -p | grep "Matches:"  # specific content
```

## Gotchas and Lessons Learned

### 1. Filter Mode Persists

The filter mode (Plain / Regex / Query) persists across filter sessions. You cannot blindly Tab twice to reach Query mode — you must detect the current mode first.

The filter bar shows the *next* mode after Tab: `Tab: Plain` means you are currently in **Query** mode.

```bash
# Smart helper: Tab until we're in Query mode
enter_query_mode() {
    tmux send-keys -t $SESSION /; sleep 0.2
    for i in 1 2 3; do
        OUT=$(tmux capture-pane -t $SESSION -p)
        if echo "$OUT" | grep -q "Tab: Plain"; then
            break  # "Tab: Plain" = currently in Query mode
        fi
        tmux send-keys -t $SESSION Tab; sleep 0.2
    done
}
```

### 2. Timing Matters

- **Startup**: 1-1.5s for the app to render initial state
- **Source switching**: 0.5-1s after pressing a number key
- **Filter debounce**: ~300ms for live preview, but query filters on large files need 1-2s
- **Aggregation**: 1.5-2s for results to render after Enter
- **Escape sequences**: 0.3s between consecutive Escapes

When in doubt, add more sleep. Flaky tests from insufficient wait time are the most common failure.

### 3. zsh Glob Characters

`?` is a glob character in zsh. When sending it via tmux:

```bash
# This fails in zsh:
#   tmux send-keys -t $SESSION ?

# This works:
tmux send-keys -t $SESSION '?'
```

### 4. Quotes in Filter Input

Double quotes arrive correctly via `tmux send-keys -l`. Both of these work:

```bash
tmux send-keys -t $SESSION -l '"error"'
tmux send-keys -t $SESSION -l 'json | level == "error"'
```

### 5. Source Discovery

When launched without arguments, LazyTail discovers all captured sources (project `.lazytail/data/` + global `~/.config/lazytail/data/`). When launched with a file argument, only that file plus global sources appear. The first source (often `global-source` or the first project source) is selected by default — use number keys (`1`-`9`) to switch.

### 6. Screen Parsing

`tmux capture-pane -p` returns text with box-drawing characters (`│`, `┌`, `└`, etc.). These are UTF-8 and work fine with grep. The content area is bounded by box borders, so log lines appear between `│` delimiters.

### 7. Color and Selection Limitations

`tmux capture-pane -p` captures **text only** — all color information is lost. This has several implications:

- **Log line selection is invisible.** The selected line is highlighted with a background color in the real terminal, but in captured text it looks identical to other lines. The only way to verify selection position is via the status bar (`Line N/M`).
- **Side panel cursor is invisible.** When the side panel is focused (after pressing `Tab`), the cursor position is rendered with a background highlight. In captured text, you cannot tell which row is focused. The `>` marker shows the *active tab* (e.g., `3> fast`), but when navigating the tree with `j`/`k`, the focused row has no text-based indicator — only a color highlight.
- **Severity colors are invisible.** Log lines are colored by severity level (error = red, warn = yellow, etc.) in the real terminal. Captured text shows only the level text (`error`, `warn`).
- **Filter mode border colors are invisible.** The filter input border changes color by mode (white = plain, cyan = regex, magenta = query, red = invalid). In captured text, you can only detect the mode via the `Tab: <next_mode>` label text.
- **Side panel selected item overflows.** The *selected* source row's text extends beyond the sidebar border into the content area, showing full details (e.g., `109K · 15MB` instead of truncated `109K · 1`). Non-selected items remain clipped to panel width. In captured text, the overflow text is present but blends with content area text since the background highlight is invisible.

For testing, always rely on **text-based indicators** rather than visual styling:
- Status bar `Line N/M` for selection position
- `>` marker for active tab in side panel
- `*` marker for active filter
- `F` marker for follow mode
- `Tab: Plain/Regex/Query` label for filter mode
- Header title for full source name and path

## Testing Workflow

The recommended workflow for a release test:

1. **Start tmux session** with fixed dimensions for consistent layout
2. **Launch lazytail** with appropriate test data (discovery mode, specific files, or live streams)
3. **Exercise each feature area** interactively: navigate, filter, switch tabs, toggle modes
4. **Capture and inspect** screen output after each action
5. **Record findings** — note any rendering issues, incorrect behavior, or crashes
6. **Start live streams** (`while true; do echo ...; sleep 0.5; done | lazytail -n test-stream`) in separate tmux windows to test follow mode, incremental filtering, and `$all` combined view
7. **Test MCP tools** in parallel using the MCP server connection (if available)

Don't try to automate this into reusable scripts. The value is in adaptive, exploratory testing where you react to what you see and investigate unexpected behavior. Scripts become brittle and mask real issues behind timing workarounds.

## What to Test

Key areas for a release test:

- **Source discovery**: project/global/captured sections, `$all` virtual sources, active (●) vs ended (○)
- **Navigation**: j/k, gg/G, :N line jump, PageUp/Down, Ctrl+E/Y viewport scroll, zz/zt/zb
- **Filtering**: plain text, regex, query mode (`json | level == "error"`), `@ts` time queries, live preview on streams
- **Aggregation**: `count by (field)`, drill-down with Enter, back with Esc
- **Tabs**: 1-9 switching, x/Ctrl+W close with confirmation dialog
- **Side panel**: Tab focus, tree navigation, Space expand/collapse, Enter select, y copy path
- **View modes**: t (timestamps), r (raw), w (wrap), f (follow), Space (expand line), c (collapse all)
- **Live streams**: follow mode, incremental filtering, ingestion rate display
- **Combined view**: `$all` merging multiple streams chronologically
- **Edge cases**: empty files, binary files, very large files (6M+ lines), corrupt indexes
- **MCP tools**: all 6 tools, `@ts` filtering, `include_ts`, `since_line`, aggregation, string-encoded params
- **Performance**: filter speed on large files, startup time, `lazytail bench`
