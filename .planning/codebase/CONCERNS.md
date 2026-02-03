# Codebase Concerns

**Analysis Date:** 2026-02-03

## Tech Debt

**Mutex Poisoning - Reader Lock Assertions (Critical)**
- Issue: Multiple locations use `.expect("Reader lock poisoned")` assuming the filter thread won't panic. If a panic occurs in the filter thread, the main UI thread will crash when trying to acquire the lock.
- Files: `src/filter/engine.rs:325,352`, `src/filter/parallel_engine.rs:116,155,202`, `src/main.rs:549-550`
- Impact: Single unexpected panic in filter thread will crash entire application
- Fix approach: Replace `expect()` with `unwrap_or_else()` to handle poisoned locks gracefully, or use alternative synchronization primitives like `parking_lot::Mutex` which don't poison

**Large Files with Excessive Line Counting (Performance)**
- Issue: `app.rs` (1765 lines) and `filter/query.rs` (1415 lines) are significantly large, making them harder to maintain and test
- Files: `src/app.rs`, `src/filter/query.rs`
- Impact: Harder to identify and fix bugs in state management and query logic
- Fix approach: Extract independent concerns from `app.rs` (e.g., move filter history and validation to separate module); break down query parsing into smaller functions

**Multiple Unwrap/Expect Calls in Production Code**
- Issue: 468 total occurrences of `unwrap()`, `expect()`, `panic!()` in src. Many are in test code, but production paths exist where errors should be handled gracefully.
- Files: Throughout, examples: `src/source.rs:239,288,290,291`, `src/tab.rs:277,299`, `src/ui/mod.rs:334`
- Impact: Unexpected runtime panics in edge cases (e.g., file I/O failures, config directory issues)
- Fix approach: Convert critical paths from unwrap to proper error handling using `?` operator and Result propagation

**Lock Contention in Shared Reader Pattern**
- Issue: Filter operations lock the shared reader mutex during batch processing, blocking the UI thread. While mitigated with brief locks and partial results, under high filter load the UI can still stutter.
- Files: `src/filter/engine.rs:350-361` (process_filter_shared), `src/main.rs:547-559` (file reload)
- Impact: Responsiveness degradation during large file filtering on slower systems
- Fix approach: Consider owned reader approach where possible (already exists as `run_filter_owned`); use reader thread-local copies for filters

**Unsafe Code in Memory-Mapped Files (Medium Risk)**
- Issue: Multiple `unsafe { Mmap::map(&file)? }` calls without explicit lifetime management documentation
- Files: `src/filter/streaming_filter.rs` (lines 84,184,274,380,451), `src/reader/mmap_reader.rs`, `src/reader/huge_file_reader.rs`, `src/mcp/tools.rs`
- Impact: If file handle is closed before mmap is dropped, undefined behavior; potential memory corruption
- Fix approach: Add explicit documentation of mmap lifetime guarantees; verify file handles remain open throughout mmap scope (currently correct but fragile)

---

## Known Bugs

**Filter State Inconsistency During Rapid Filter Changes**
- Symptoms: User rapidly toggles filter mode (plain/regex) + types pattern. UI flickers or shows stale results. Filter may show intermediate results that don't match final pattern.
- Trigger: Type filter pattern, press mode toggle repeatedly, very quick rapid keystrokes
- Files: `src/main.rs:854-867` (live filter debounce), `src/app.rs:398-462` (merge_partial_filter_results)
- Workaround: Press Escape to cancel and start over with a new filter
- Current mitigation: Debounce of 500ms and cancellation of in-progress filters, but edge case remains

**File Truncation Edge Case**
- Symptoms: If file is truncated while filter is in progress, UI may render incorrect line numbers or crash
- Trigger: Run filter on large log, truncate file in background, filter completes
- Files: `src/app.rs:848-878`, `src/main.rs:888-894`
- Current handling: Cancels filter and resets tab state, but timing window exists before cancel takes effect
- Fix approach: Add atomic flag for truncation detection before rendering filtered results

**Viewport Anchor Loss During Incremental Filtering**
- Symptoms: When new lines are appended to file during filtering, user's selection anchor may jump unexpectedly
- Trigger: Large file with active filter + file continues to grow. Partial results arrive, prepend items to list.
- Files: `src/app.rs:425-432` (adjust_scroll_for_prepend), `src/viewport.rs:85-103` (anchor resolution)
- Impact: Selection jumps when new matches found before current position
- Fix approach: Improve anchor persistence during prepended matches, test with continuously-growing logs

---

## Security Considerations

**Untrusted ANSI Code Processing**
- Risk: ANSI escape sequences in log files could exploit terminal or rendering libraries
- Current mitigation: Using `ansi-to-tui` crate which sanitizes ANSI codes for ratatui
- Recommendation: Audit `ansi-to-tui` dependency regularly; consider allowlist approach for ANSI codes

**File Path Traversal in Discovery Mode**
- Risk: Source discovery reads from `~/.config/lazytail/data/`. If symlinks or relative paths are exploited, could read arbitrary files
- Files: `src/source.rs` (discover_sources, check_source_status)
- Current mitigation: Uses `dirs::data_dir()` which resolves to user's actual config directory
- Recommendation: Validate discovered paths don't escape data directory; reject symlinks in discovery

**MCP Server Stdin Transport (If Enabled)**
- Risk: `lazytail --mcp` reads commands from stdio. If multiple processes write to stdio, malformed input could crash server
- Files: `src/mcp/mod.rs`
- Current mitigation: Uses `rmcp` library which handles JSON-RPC parsing
- Recommendation: When MCP is enabled, validate all inputs are well-formed JSON-RPC; add request size limits

---

## Performance Bottlenecks

**Large File Viewport Resolution**
- Problem: `Viewport::resolve()` uses binary search to find anchor line in filtered view. With millions of filtered matches, repeated calls during every render add cost.
- Files: `src/viewport.rs:57-100`
- Cause: Caching could be more aggressive; currently invalidated on each resolve call
- Current state: Cache exists but invalidated frequently
- Improvement path: Use persistent cache with selective invalidation only when line_indices change, not on every resolve

**Filter Progress Events Flood**
- Problem: Filter sends partial results frequently. With large files and small batch sizes, this can cause excessive channel sends and UI re-renders.
- Files: `src/filter/streaming_filter.rs:123-130` (BATCH_SIZE=50,000)
- Cause: Each partial result triggers viewport resolution and UI render
- Improvement path: Coalesce progress updates; send partial results less frequently (every 100k lines instead of 50k)

**Regex Compilation on Every Keystroke**
- Problem: During filter input, regex is re-compiled and validated on every character entered (even with debounce)
- Files: `src/app.rs:659-680` (validate_regex), `src/main.rs:854-867` (debounce at 500ms)
- Current state: Debounce mitigates most impact, but complex regexes still recompile repeatedly
- Improvement path: Cache last valid regex; only recompile if pattern actually changes

**Reader Seek Performance on Large Files**
- Problem: `FileReader` uses binary search with seeks to find lines. On very large files (>1GB), seek overhead accumulates
- Files: `src/reader/file_reader.rs` (get_line implementation)
- Current mitigation: Sparse index caching to reduce seek count
- Improvement path: Pre-allocate larger sparse index chunks; consider sequential read optimization for sequential access patterns

---

## Fragile Areas

**Tab State Synchronization (Complex)**
- Files: `src/app.rs:85-129` (App state), `src/tab.rs:72-104` (TabState), `src/viewport.rs:19-48` (Viewport)
- Why fragile: Multiple sources of truth for position: `TabState::selected_line`, `TabState::scroll_position`, `Viewport::anchor_line`, `Viewport::scroll_position`. These must stay in sync through navigation, filtering, file changes, and truncation.
- Safe modification: Always use viewport methods for navigation; sync legacy `selected_line`/`scroll_position` after viewport changes; add assertions to verify consistency
- Test coverage: Good coverage of individual operations, but insufficient coverage of state transitions (filter->normal->filter)

**Filter Result Merging Logic (Algorithmic Complexity)**
- Files: `src/app.rs:398-462` (merge_partial_filter_results)
- Why fragile: Merges sorted arrays of indices while adjusting viewport scroll position for prepended items. Two-pointer merge with dynamic scroll adjustment is error-prone.
- Safe modification: Add extensive test coverage for prepended matches, edge cases (empty results, duplicate indices); add invariant assertions
- Test coverage: No specific tests for merge with prepended items, only basic filter apply tests

**File Truncation Handling**
- Files: `src/app.rs:848-878` (FileTruncated event), `src/main.rs:888-894` (process_event)
- Why fragile: Must reset filter state, cancel in-progress operations, adjust viewport, and sync legacy fields. Subtle race: filter completion event may arrive after truncation event, corrupting state.
- Safe modification: Use sequence numbers or timestamps for events to prevent out-of-order processing; test truncation during active filter
- Test coverage: No integration tests for truncation scenarios

**Streaming Reader Background Thread**
- Files: `src/tab.rs:180-220` (spawn_stream_reader), `src/main.rs:692-732` (collect_stream_events)
- Why fragile: Separate thread reads stdin/pipe and sends lines via channel. UI thread must handle disconnect gracefully. Panics in background thread don't poison channel but silently stop sending.
- Safe modification: Add monitoring for unexpected thread exits; use crossbeam-channel for better error semantics; test pipe closure scenarios
- Test coverage: No tests for premature pipe closure or background thread panics

---

## Scaling Limits

**In-Memory Line Index Storage**
- Current capacity: Stores all matching line indices in `Vec<usize>`. With 1GB log file (10M lines), filtered 1M matches = 8MB just for indices.
- Limit: At 10M filtered matches, would need ~80MB. Beyond this, memory pressure increases.
- Scaling path: Implement lazy/lazy index loading; store ranges instead of individual indices; support paged index on disk

**Filter Channel Backlog**
- Current design: Filter thread sends progress updates to channel; UI processes them in main loop at ~100ms poll interval
- Limit: If filter produces results faster than UI can render (e.g., 50k matches per 100ms poll), channel backlog grows unbounded
- Scaling path: Use bounded channels with backpressure; drop intermediate partial results if UI can't keep up

**Mmap Size Limits**
- Current approach: Memory-maps entire file for fast searching
- Limit: Files >4GB may cause memory pressure on 32-bit systems; on 64-bit systems practically unlimited but can exhaust virtual memory
- Scaling path: Support multi-mmap for large files (process in chunks); fall back to streaming filter for files >2GB

**Viewport Rendering with Large Result Sets**
- Current approach: Renders entire visible window every frame using cached index lookup
- Limit: With 1M+ filtered results, repeated viewport resolution searches become bottleneck (binary search on 1M items per render = 20 iterations)
- Scaling path: Improve viewport caching; implement hierarchical index structure for faster searches

---

## Dependencies at Risk

**regex Crate - Potential ReDoS**
- Risk: User-provided regex patterns could cause catastrophic backtracking (ReDoS attack)
- Impact: User enters `(a+)+b` pattern; filter thread spins at 100% CPU, blocking UI
- Current mitigation: None - regex is compiled but not restricted
- Migration plan: Add pattern complexity check before compilation; consider separate timeout thread for regex compilation; document unsafe patterns

**memmap2 Crate - File Descriptor Leaks**
- Risk: If mmap drop is delayed or skipped, file descriptors leak
- Impact: Long filtering sessions could exhaust fd limits (ulimit -n)
- Current mitigation: RAII should ensure cleanup, but no explicit verification
- Migration plan: Add FD monitoring in tests; add explicit mmap.flush() before important operations

**ratatui - Terminal State Recovery**
- Risk: Terminal panic or crash leaves terminal in alternate screen mode with raw mode enabled
- Impact: Terminal becomes unusable until manual `reset` command
- Current mitigation: Uses proper cleanup in main() defer block
- Recommendation: Test abnormal termination scenarios; add signal handlers for SIGTERM/SIGKILL to ensure cleanup

---

## Missing Critical Features

**No Persistent Search Queries**
- Problem: Query filters (json | ... syntax) aren't persisted to history like plain filters. User must re-type complex queries.
- Blocks: Power users can't build a library of useful queries
- Potential fix: Store query history separately with metadata about result counts

**No Session Recovery**
- Problem: If app crashes or terminal hangs, user loses scroll position and active filters
- Blocks: Users of large continuous logs must re-navigate to position of interest
- Potential fix: Auto-save tab state to disk; restore on startup (with --no-restore flag)

**No Incremental Save for Captured Sources**
- Problem: When using `lazytail -n API` capture mode with long-running processes, sources must be re-captured from scratch if app restarts
- Blocks: Can't reliably tail continuously-updated sources
- Potential fix: Support append mode for sources; track file offsets

---

## Test Coverage Gaps

**File Truncation Scenarios**
- What's not tested: Truncation during active filter, truncation of partially-viewed file, multiple truncations in sequence
- Files: `src/app.rs:848-878`, tests in `src/app.rs:965+` don't include truncation
- Risk: Regression in truncation handling could silently corrupt state
- Priority: High - truncation is well-defined but fragile

**State Transitions**
- What's not tested: Filter -> Normal -> Filtered transitions with viewport anchor preservation
- Files: `src/viewport.rs`, `src/app.rs`
- Risk: Anchor loss on view mode changes
- Priority: High - core UX feature

**Streaming Reader Edge Cases**
- What's not tested: Premature pipe closure, slow producer (reader blocks), background thread panic
- Files: `src/tab.rs:180-220`, `src/main.rs:692-732`
- Risk: Silent data loss or incomplete UI state
- Priority: Medium - affects pipe input users

**Large File Performance**
- What's not tested: Filter performance on files >100MB, viewport resolution with 1M+ matches
- Current tests: Mostly <1MB synthetic data
- Risk: Unknown performance degradation in production
- Priority: Medium - helps identify scaling issues before users hit them

**Regex Validation**
- What's not tested: Complex regexes that cause ReDoS, extremely long patterns (>10KB), invalid UTF-8 in pattern
- Files: `src/app.rs:659-680`
- Risk: Crash or hang on pathological input
- Priority: Low-Medium - edge case but user-controlled

---

## Architectural Concerns

**Mixed Synchronization Primitives**
- Issue: Uses both standard `Mutex` and manual lock patterns with `.expect()`. No consistent error handling policy.
- Files: Throughout src/
- Impact: Inconsistent behavior on lock poisoning; potential for missed edge cases
- Recommendation: Standardize on one approach - either use `parking_lot::Mutex` (no poisoning) or handle poisoning uniformly with recovery logic

**Event Loop Complexity**
- Issue: Main loop in `run_app_with_discovery()` coordinates multiple event sources (file watcher, filter progress, stream reader, input) with complex state transitions
- Files: `src/main.rs:430-510`
- Impact: Difficult to reason about event ordering; subtle race conditions possible
- Recommendation: Consider event sourcing pattern with event log; add sequence numbers to all events for replay/debugging

**Implicit State Synchronization**
- Issue: Multiple `sync` operations happen implicitly (viewport anchors, filter state transitions, tab selection) without explicit notification
- Files: Various handler functions in `src/handlers/`
- Impact: Easy to miss state updates; inconsistent state visible to renderer
- Recommendation: Implement explicit state invalidation; add debug assertions for state consistency checks
