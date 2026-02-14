# ADR-009: Multi-Tab Model with Independent State

## Status

Accepted

## Context

Users often need to view multiple log files simultaneously (e.g., application log + error log, or logs from multiple services). The viewer needs a model for organizing multiple sources.

Options considered:
1. **Single view** - one file at a time, switch between them
2. **Split panes** - multiple files visible simultaneously (like tmux)
3. **Independent tabs** - each tab has its own complete state
4. **Shared state with views** - one state object, multiple views into it

## Decision

Each tab (`TabState`) is **fully independent** with its own:
- `reader: Arc<Mutex<dyn LogReader>>` - file or stream reader
- `watcher: Option<FileWatcher>` - file change detection
- `viewport: Viewport` - scroll position and selection
- `filter: FilterTabState` - active filter, cancel token, progress channel
- `line_indices: Vec<usize>` - currently visible lines
- `mode: ViewMode` - Normal or Filtered
- `follow_mode: bool` - auto-scroll
- `expansion: ExpansionState` - expanded lines

Tabs are stored in `App.tabs: Vec<TabState>` with `App.active_tab: usize` pointing to the current tab. The side panel groups tabs by source type (Project, Global, File, Pipe) in a tree view.

Inactive tabs continue processing in the background:
- File modifications update `total_lines` and `line_indices` directly
- Filter progress is applied directly to the tab's state
- Follow mode jumps happen for inactive tabs if enabled

## Consequences

**Benefits:**
- Switching tabs is instant (all state is pre-computed)
- Filtering one tab doesn't affect others
- Each tab's filter can run independently in its own background thread
- Simple mental model: each tab behaves exactly like a standalone viewer

**Trade-offs:**
- Memory scales linearly with open tabs (each has its own reader, index, viewport)
- Background processing for inactive tabs adds some CPU overhead
- No shared filter across tabs (users must re-enter filters per tab)
- Source panel tree view adds UI complexity for organizing many tabs
