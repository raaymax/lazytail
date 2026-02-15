# Deep Review: Columnar Index Feature

## Columns We Compute But Never Read

| Column | Written By | Size per 1M lines | Read By | Verdict |
|--------|-----------|-------------------|---------|---------|
| `time` | Both builders | 8 MB | **Nobody** | Dead weight — stores wall-clock indexing time, not log timestamps |
| `lengths` | Both builders | 4 MB | **Nobody** | Dead weight — `FileReader` uses `read_line()`, never pre-allocates from known length |
| `content_hash` (in checkpoints) | Both builders | 8 bytes/checkpoint | **Nobody** | Computed via xxh3, never validated for staleness detection |
| `index_timestamp` (in checkpoints) | Both builders | 8 bytes/checkpoint | **Nobody** | Written, never consumed |

**Total waste**: 12 MB per million lines of pure I/O + storage with zero consumers.

---

## Flags We Detect But Never Use

| Flag | Bit | Set By `detect_flags_bytes`? | Consumed? |
|------|-----|------------------------------|-----------|
| `FLAG_HAS_TRACE_ID` | 7 | **No** — constant defined, zero detection logic | Never |
| `FLAG_IS_MULTILINE_CONT` | 9 | **No** — constant defined, zero detection logic | Never |
| Template ID (bits 16-31) | 16-31 | **No** — accessors exist, never called during indexing | Never |
| `FLAG_HAS_TIMESTAMP` | 6 | Yes | Never — detected and stored but nothing reads it |
| `FLAG_HAS_ANSI` | 5 | Yes | Only internally (to pick ANSI-aware severity scanner) — never surfaced |
| `FLAG_IS_EMPTY` | 8 | Yes | Only in `index_mask()` — used to skip empty lines during query filtering |
| `FLAG_FORMAT_JSON` | 3 | Yes | Only in `index_mask()` — pre-filters non-JSON lines for `json \|` queries |
| `FLAG_FORMAT_LOGFMT` | 4 | Yes | Only in `index_mask()` — pre-filters non-logfmt lines for `logfmt \|` queries |

Three flag constants are complete dead code (`TRACE_ID`, `MULTILINE_CONT`, template ID). Two flags are detected but never surfaced to the user (`TIMESTAMP`, `ANSI`).

---

## Where the Index Is Underutilized

### 1. FileReader samples offsets instead of using them directly

The offsets column stores the **exact byte offset of every line**. `FileReader::try_seed_from_index()` samples 1 in 10,000 offsets to build a sparse index, then for every `get_line()` call, seeks to the nearest sparse entry and scans forward up to 9,999 lines. The full offsets column is mmap-backed — O(1) random access to any line is already on disk, we just throw it away.

**Impact**: Every `get_line()` does unnecessary forward scanning. For the UI rendering ~50 visible lines per frame, this means up to 50 × 9,999 = ~500K lines scanned per render in the worst case.

### 2. IndexReader is never refreshed after tab creation

`IndexReader` copies flags + checkpoints into owned `Vec`s once at tab open time. When a capture process appends lines and index entries concurrently:
- New lines get `Severity::Unknown` (no background coloring)
- Stats panel histogram shows stale counts
- Index-accelerated filtering ignores new lines (bitmap shorter than file)

For the primary use case (live tailing a captured source), severity data goes stale immediately.

### 3. Range filtering scans from byte 0

`run_streaming_filter_range()` counts newlines from the start of the file to reach `start_line`, even though the offsets column knows the exact byte position. For incremental filtering on a 1GB file where only the last 1000 lines are new, it still scans ~1GB of newlines to find the starting position.

### 4. Index acceleration is query-only

The index-accelerated path is gated to `is_query_syntax()` (i.e., `json | ...` / `logfmt | ...`). Plain text and regex filters never consult the index. A search for `"error"` does a full file scan even though the index already knows which lines have `Severity::Error`. For files where errors are 0.1% of lines, this is a 1000x missed opportunity.

### 5. Index acceleration skips incremental (range) filters

The orchestrator gates index use with `range.is_none()`. When a file grows and incremental filtering triggers, the bitmap is never consulted — the range filter does a full content scan of the new lines without pre-filtering. The bitmap could trivially be sliced to `[start..end]`.

### 6. Checkpoints have temporal data we ignore

Checkpoints are written every 100 lines with **cumulative** severity counts. This means intermediate checkpoints encode the severity distribution over time — error rate spikes, quiet periods, etc. The UI only reads `checkpoints().last()` for totals. A sparkline/trend visualization is free data sitting on disk.

### 7. MCP reopens IndexReader on every query

`query_impl()` in `mcp/tools.rs` calls `IndexReader::open(path)` per request — re-reading meta, mmapping flags, copying into a `Vec`. The TUI caches it in `TabState`. MCP has no caching, so every structured query pays the full open+copy cost.

### 8. MCP tools don't carry severity metadata

`get_lines`, `get_tail`, and `get_context` return `LineInfo { line_number, content }` — no severity. When an AI assistant fetches log lines, it has to infer severity from content. The index already has this per line.

### 9. No severity-based navigation

The index has per-line severity flags enabling O(1) "next error" / "previous error" jumps via `scan_flags()`. No keybinding exists for this. Users must visually scan or type a filter query.

### 10. No severity-only filter mode

A "show only errors" command could populate `line_indices` purely from the flags column without touching file content. This would be near-instant for any file size. Currently the only way is `json | level == "error"` which requires a structured query and full content parsing of candidate lines.

---

## Dead Code

| Item | Location | Lines |
|------|----------|-------|
| `parallel_engine.rs` | `filter/parallel_engine.rs` | **504 lines** — entire module `#[allow(dead_code)]`, zero callers |
| `FilterEngine::run_filter_owned` + `run_filter_range_owned` | `filter/engine.rs` | ~140 lines — `#[allow(dead_code)]`, never called |
| `scan_flags()` on IndexReader | `index/reader.rs` | Public method, zero production callers (only test usage) |
| `IndexBuilder::with_checkpoint_interval()` | `index/builder.rs` | `#[allow(dead_code)]`, only test usage |
| `LineIndexer::resume()` | `index/builder.rs` | `#[allow(dead_code)]`, only test usage — capture always calls `create()` |
| `detect_flags()` (str version) | `index/flags.rs` | Thin wrapper, only test callers — production uses `detect_flags_bytes()` |
| `Severity::to_bits()` | `index/flags.rs` | Test-only, no production callers |
| `IndexMeta::clear_column()` | `index/meta.rs` | Test-only |
| `_case_sensitive` param in `stream_filter_fast_impl` | `filter/streaming_filter.rs` | Vestigial — always called for case-insensitive, parameter ignored |

That's **~650+ lines** of dead code.

---

## Architectural Issues

### 1. Silent error swallowing in FilterOrchestrator

Every `Err(_) => return` in the orchestrator (lines 43, 57, 83, 117) silently drops errors. Malformed queries, invalid regex — the user gets no feedback, just a filter that silently doesn't run.

### 2. `all_matches` duplication in streaming filters

Every streaming filter accumulates both `batch_matches` (sent via `PartialResults`) and `all_matches` (sent in `Complete`). The `Complete` payload contains ALL matches including those already sent as partials. This doubles memory usage for the match list. The test helper `collect_matches` adds both partial and complete matches, which would double-count.

### 3. Capture mode always truncates the index

`LineIndexer::create()` truncates all column files. If capture restarts for the same source (log file opened in append mode), the log grows but the index resets from zero. The `resume()` method exists and is tested but never wired in.

### 4. INDEX_STATUS.md is stale

Three items marked incomplete are actually done (severity coloring, severity histogram, MCP get_stats). The doc doesn't reflect reality.

### 5. Severity detection duplicated

`detect_severity_single_pass()` and `detect_severity_scalar()` contain near-identical keyword matching logic — same `match bytes[i] | 0x20` arms, same `eq_ci_word` calls. Only difference is ANSI skip behavior. Could be unified with a skip callback.

### 6. Expanded lines lose severity background

When a line is expanded (wrapped), only `EXPANDED_BG` or selection styling applies. An expanded error line loses its red background — the severity context disappears on the interaction that should show more detail.

---

## Summary: What the Index Could Be But Isn't Yet

The index computes and stores a lot of per-line metadata, but the codebase treats it mostly as an optional optimization for structured queries. The full potential would be:

| Capability | Current State | Full Potential |
|-----------|---------------|----------------|
| Line access | Sparse index, scan forward ≤9999 lines | O(1) direct seek via offsets column |
| Severity coloring | Static snapshot from tab creation | Live-updating as capture appends |
| Severity filtering | Only via `json \| level == "error"` | Instant flag-only filter, keybinding |
| Severity navigation | None | `]e`/`[e` jump to next/prev error |
| Range filter start | Scan from byte 0 | Seek to exact offset |
| Plain text + index | Never combined | Pre-filter by severity for `"error"` searches |
| Checkpoint trends | Only last checkpoint used | Sparkline/trend over time |
| MCP line metadata | Content only | Content + severity per line |
| MCP index caching | Re-open per request | Cached reader |
| Stale data columns | `time`, `lengths` always written | Stop writing or start using |
