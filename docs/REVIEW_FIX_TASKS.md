# Review Fix Tasks

Performance-focused fixes and improvements identified during the columnar index deep review.

---

## P0: Direct Performance Wins

- [x] **FileReader: use offsets column directly instead of sparse index sampling**

  `FileReader` now keeps the full mmap-backed `ColumnReader<u64>` for O(1) direct seek to any indexed line. Falls back to sparse index for tail lines beyond the indexed range. For 50 visible lines per frame, worst case dropped from ~500K lines scanned to zero. *(commit: feat-columnar-index)*

- [ ] **Range filter: seek to start offset instead of scanning from byte 0**

  `run_streaming_filter_range()` counts newlines from the start of the file to reach `start_line`. For incremental filtering on a 1GB file where only the last 1000 lines are new, it still scans ~1GB of newlines just to find the starting position. The offsets column (or FileReader's index) knows the exact byte offset — seek directly.

- [ ] **Extend index acceleration to incremental (range) filters**

  The orchestrator gates index use with `range.is_none()`. When a file grows and incremental filtering triggers, the bitmap is never consulted. The bitmap can be sliced to `[start..end]` trivially. Without this, every file growth event on a query-filtered view does a full content scan of the new lines.

- [ ] **Eliminate `all_matches` duplication in streaming filters**

  Every streaming filter accumulates both `batch_matches` (sent via `PartialResults`) and `all_matches` (sent in `Complete`). The `Complete` payload re-sends everything already delivered as partials, doubling peak memory for the match list. For a filter matching 1M lines, that's two copies of 1M `usize` values (~16 MB wasted). Send only the final batch in `Complete`, or send an empty sentinel.

---

## P1: Index Utilization

- [ ] **Severity-only filter mode from flags column (no content scan)**

  A "show only errors" command could populate `line_indices` purely from the flags column without touching file content — near-instant for any file size. Currently requires typing `json | level == "error"` which parses every candidate line's JSON. The flags column already has severity per line; a keybinding (e.g. `s` to cycle severity levels) that builds `line_indices` from `scan_flags()` would be orders of magnitude faster than content-based filtering.

- [ ] **Extend index acceleration to plain text severity searches**

  The index-accelerated path is gated to `is_query_syntax()`. A plain text search for `"error"` does a full file scan even though the index knows which lines have `Severity::Error`. For files where errors are 0.1% of lines, pre-filtering by severity flag before content scan is a 1000x reduction in lines checked. Requires heuristic matching of the search pattern against known severity keywords.

- [ ] **Severity-based navigation via `scan_flags()`**

  The index has per-line severity flags enabling O(n) scan on a dense u32 array for "next error" / "previous error" jumps. No keybinding exists — users must visually scan or type a filter. `]e`/`[e` (or similar) would use `scan_flags()` which is already implemented and tested but has zero production callers.

- [ ] **Refresh IndexReader on file modification**

  `IndexReader` copies flags + checkpoints into owned `Vec`s once at tab open time and is never updated. For live-tailed captured sources, new lines get `Severity::Unknown` (no coloring), the stats histogram goes stale, and index-accelerated filtering ignores new lines (bitmap shorter than file). On `FileModified` events, re-open the index to pick up new flags from the capture process.

- [ ] **Cache IndexReader in MCP server**

  `query_impl()` calls `IndexReader::open(path)` on every request — re-reading meta, mmapping flags, copying the entire flags column into a `Vec` each time. The TUI caches it in `TabState`. For rapid MCP queries on a large index, this is significant repeated allocation. Cache per source path with a staleness check on `meta.entry_count`.

---

## P2: Stop Computing What Nothing Reads

- [ ] **Stop writing the `time` column (or start using it)**

  Both builders write an 8-byte timestamp per line that no code path ever reads. For a 1M-line log, that's 8 MB of disk I/O and storage with zero consumers. The timestamp stored is the wall-clock indexing time, not the log line's timestamp, limiting future utility. Either remove the write or change it to store parsed log timestamps (which would enable time-range filtering — a planned roadmap feature). Until time filtering lands, stop paying the cost.

- [ ] **Stop writing the `lengths` column (or wire it into FileReader)**

  Both builders write a 4-byte length per line that no code path reads. For a 1M-line log, that's 4 MB wasted. The value would enable `read_exact()` with pre-allocated buffers instead of `read_line()` with dynamic allocation — but only if FileReader uses direct offsets (P0 task above). If the offsets task lands, wire lengths in too. Otherwise, stop writing them.

---

## P3: Dead Code Removal

- [ ] **Remove `parallel_engine.rs` (504 lines)**

  Entire module is `#[allow(dead_code)]` with zero production callers. Superseded by the streaming filter architecture. The planned aggregation feature will use a streaming accumulator pattern (angle-grinder's `MultiGrouper`), not parallel chunk-based filtering. Removing it cuts 504 lines of maintenance surface.

- [ ] **Remove `FilterEngine::run_filter_owned` and `run_filter_range_owned` (~140 lines)**

  Both are `#[allow(dead_code)]`, never called. Owned-reader optimization path that was never wired in. No planned feature needs owned reader transfer — stdin uses `Arc<Mutex<R>>`, files use mmap streaming.

- [ ] **Remove vestigial `_case_sensitive` parameter from `stream_filter_fast_impl`**

  The parameter is accepted but ignored — the function always does case-insensitive search. The caller already dispatches case-sensitive to `stream_filter_grep_style`. Misleading signature.

---

## P4: Correctness / UX

- [ ] **Surface filter errors instead of silent swallowing in FilterOrchestrator**

  Every `Err(_) => return` in the orchestrator silently drops errors. Malformed queries, invalid regex — the user gets no feedback, the filter just silently doesn't run. Return errors through the channel so the UI can display them in the status bar.

- [ ] **Preserve severity background on expanded lines**

  When a line is expanded (wrapped), only `EXPANDED_BG` or selection styling applies. An expanded error line loses its red background — the severity context disappears on exactly the interaction that should show more detail. Blend severity tint into the expanded background.

- [ ] **Wire `LineIndexer::resume()` into capture mode**

  Capture mode always calls `LineIndexer::create()` which truncates all column files. If capture restarts for the same source (log file is append-mode), the log grows across runs but the index resets from zero. `resume()` exists and is tested — wire it in when the log file already has an index, so the index stays in sync with the full log content.

- [ ] **Update INDEX_STATUS.md to reflect completed work**

  Three items marked `[ ]` are actually done: severity-based line coloring, severity histogram in stats panel, MCP `get_stats` tool. Stale docs create confusion about what's implemented.

---

## P5: Code Quality

- [ ] **Unify `detect_severity_single_pass()` and `detect_severity_scalar()`**

  Near-identical keyword matching logic duplicated across two functions — same `match bytes[i] | 0x20` arms, same `eq_ci_word` calls. Only difference is ANSI escape skipping. Unify with a generic function that takes a skip-ANSI flag or callback. Reduces maintenance surface and risk of the two paths diverging.

- [ ] **Use checkpoint history for severity trend sparkline in stats panel**

  Checkpoints are written every 100 lines with cumulative severity counts — intermediate checkpoints encode the severity distribution over time (error rate spikes, quiet periods). The UI only reads `checkpoints().last()` for totals. Rendering a sparkline from checkpoint deltas is free data already on disk, giving users immediate visual insight into when errors cluster.
