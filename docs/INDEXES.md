# Columnar Index Design

*Design document for LazyTail's persistent per-line index system.*

---

## Overview

LazyTail builds persistent columnar indexes alongside log files to enable sub-millisecond metadata queries without parsing log content. Each "column" is a separate file containing a dense typed array — one entry per log line. Columns are built lazily (on demand) or inline during capture, and persist across sessions.

**Design principles:**
- Columnar: each field is a separate file (dense typed array)
- Lazy: columns materialize only when first needed
- Append-only: capture mode appends to columns in real-time
- Zero-copy: columns are mmap'd for direct access
- SIMD-friendly: contiguous typed arrays load directly into vector registers

---

## Directory Layout

```
.lazytail/idx/{source_name}/
  meta          64 bytes     header with structural info
  checkpoints   [Checkpoint; M]  64B per entry, one per 100K lines
  offsets       [u64; N]     8B/line   byte offset into log file
  lengths       [u32; N]     4B/line   line length in bytes
  time          [u64; N]     8B/line   arrival/parsed timestamp (epoch millis)
  flags         [u32; N]     4B/line   metadata bitmask
  templates     [u16; N]     2B/line   Drain cluster ID (future)
```

Dense column files are raw typed arrays with no framing. Line `i`'s flags = `flags_mmap[i]`. Missing file = column not yet built. The `checkpoints` file is sparse (one entry per 100K lines) and handles validation, partial rebuild, and cumulative stats.

---

## Meta File (64 bytes)

```
offset  size  field
0       4     magic: [u8; 4] = b"LTIX"
4       2     version: u16 = 1
6       2     checkpoint_interval: u16    — in thousands (e.g., 100 = every 100K lines)
8       8     entry_count: u64            — total indexed lines
16      8     log_file_size: u64          — expected log file size
24      8     columns_present: u64        — bitmask of which column files exist
32      2     flags_schema: u16           — bit layout version (reindex when changed)
34      30    reserved: [u8; 30]
```

**`columns_present` bitmask:**
```
bit 0: offsets
bit 1: lengths
bit 2: time
bit 3: flags
bit 4: templates
bit 5: checkpoints
bits 6-63: reserved
```

Fast way to know what's available without filesystem stat calls per column.

## Checkpoints File

Sparse column — one entry per `checkpoint_interval` lines (default: 100K). Each entry is 64 bytes:

```
offset  size  field
0       8     line_number: u64                — checkpoint position
8       8     byte_offset: u64                — log file position at this line
16      8     content_hash: u64               — rolling xxhash of log content up to this point
24      8     index_timestamp: u64            — when this checkpoint was written (epoch millis)
32      28    severity_counts: [u32; 7]       — cumulative: unknown/trace/debug/info/warn/error/fatal
60      4     reserved: u32
```

For 60M lines at 100K interval = **600 entries = 38 KB**.

### Granular Stale Detection

Instead of a single hash in meta, checkpoints provide per-segment validation:

```rust
for cp in checkpoints.iter().rev() {
    let actual = xxhash(&log_mmap[..cp.byte_offset]);
    if actual == cp.content_hash {
        // Valid up to here — rebuild only from cp.line_number forward
        return RebuildFrom(cp.line_number);
    }
}
// Nothing matches — full rebuild
```

Catches cases a single hash misses: log rotation with identical first 4KB, mid-file edits, partial truncation.

### Partial Rebuild

If checkpoint at line 5M is valid but 5.1M isn't, rebuild columns from line 5M onward. On a 60M-line file: **12x faster** than full rebuild.

Also handles interrupted index builds — resume from last written checkpoint.

### Instant Approximate Stats

Last checkpoint has cumulative severity counts. Combine with a short scan past the last checkpoint:

```
"How many errors total?"
  → last checkpoint (line 59.9M): error_count = 142,857
  → scan flags for remaining ~100K lines: +23 errors
  → answer: 142,880
  → scanned 400KB instead of 240MB
```

### Incremental Append

Log file grew since last index:

1. Verify last checkpoint's `content_hash` — file identity unchanged
2. Scan only new lines past `meta.entry_count`
3. Append to each existing column file
4. Write new checkpoints as intervals are crossed
5. Update `meta.entry_count` and `meta.log_file_size`

---

## Flags Bitmask (u32)

```
bits 0-2   (3 bits):  severity
                        0 = unknown
                        1 = trace
                        2 = debug
                        3 = info
                        4 = warn
                        5 = error
                        6 = fatal

bit  3     (1 bit):   format_json         — line is valid JSON
bit  4     (1 bit):   format_logfmt       — line is logfmt
bit  5     (1 bit):   has_ansi            — line contains ANSI escape codes
bit  6     (1 bit):   has_timestamp       — timestamp detected in content
bit  7     (1 bit):   has_trace_id        — trace/request ID detected
bit  8     (1 bit):   is_empty            — empty or whitespace-only line
bit  9     (1 bit):   is_multiline_cont   — continuation of previous line
bits 10-15 (6 bits):  reserved

bits 16-31 (16 bits): template_id         — Drain cluster ID (0 = unclassified)
                                            supports up to 65535 templates
```

**Filtering via AND:**

```rust
// "All JSON ERROR lines" — no log content parsed
let mask = SEVERITY_MASK | FLAG_FORMAT_JSON;
let want = SEVERITY_ERROR | FLAG_FORMAT_JSON;
if entry_flags & mask == want { /* match */ }
```

**SIMD scanning (AVX2):**

Dense `[u32]` array loads directly into 256-bit registers. 8 flags checked per instruction, no gather needed.

```rust
// AVX2: 8 flags per cycle
let mask = _mm256_set1_epi32(SEVERITY_MASK | FLAG_FORMAT_JSON);
let want = _mm256_set1_epi32(SEVERITY_ERROR | FLAG_FORMAT_JSON);

for chunk in flags_mmap.chunks_exact(8) {
    let v = _mm256_loadu_si256(chunk.as_ptr() as *const __m256i);
    let masked = _mm256_and_si256(v, mask);
    let hits = _mm256_cmpeq_epi32(masked, want);
    let bitmask = _mm256_movemask_epi8(hits);
    if bitmask != 0 { /* extract matching positions */ }
}
```

---

## Timestamps

**During capture (`lazytail -n`):** Arrival timestamp — `SystemTime::now()` per line. Free, monotonic, consistent across sources.

**For existing files:** Parse timestamp from log content during index build. Fall back to 0 if unparseable.

**Properties:** Monotonically increasing (for captured sources) enables binary search for time-range queries: O(log n) to find the start position, then linear scan.

---

## Column Build Strategy

### During Capture (`lazytail -n`)

Columns built inline as lines are written. Cost per line:

| Column | Cost | Why |
|--------|------|-----|
| `offsets` | Zero | Byte position known at write time |
| `lengths` | Zero | Line length known at write time |
| `time` | Zero | Single `SystemTime::now()` call |
| `flags` | ~50-100ns | Inspect content: starts with `{`? has ANSI? severity? |

All four are essentially free — the line is already in memory for tee-to-stdout.

```rust
// Capture write path (pseudocode)
let now = SystemTime::now().epoch_millis();
let offset = log_file.stream_position();
let len = line.len() as u32;
let flags = detect_flags(&line);  // format, severity, ansi

writeln!(log_file, "{}", line)?;
offsets_file.write_all(&offset.to_le_bytes())?;
lengths_file.write_all(&len.to_le_bytes())?;
time_file.write_all(&now.to_le_bytes())?;
flags_file.write_all(&flags.to_le_bytes())?;
```

### For Existing Files (On Demand)

Columns build lazily — only when a query first needs them:

| User action | Columns triggered |
|-------------|-------------------|
| Just scroll through file | None (use current SparseIndex) |
| First text filter (`/error`) | None (streaming mmap+memchr, no index needed) |
| First structured query (`json \| level == "error"`) | `flags` |
| First time-range query | `time` |
| MCP `stats` call | `time` (first + last entry only) |
| Pattern clustering | `flags` + `templates` |
| Read matched line content | `offsets` + `lengths` |

**Single-pass multi-column build:** When multiple columns are needed, build them in one sequential scan of the log file:

```rust
for (i, line) in mmap_lines(log_path).enumerate() {
    if need_flags { flags_writer.write(detect_flags(line))?; }
    if need_time  { time_writer.write(parse_timestamp(line))?; }
    // ... other columns
}
```

One pass over the log, N column files written. Never scan the file twice.

### Incremental Update

When a log file grows (file watcher detects change):

1. Read `meta.entry_count` — lines already indexed
2. Verify last checkpoint's `content_hash` — file identity unchanged
3. Scan only new lines (seek to `offsets[entry_count - 1] + lengths[entry_count - 1]`)
4. Append to each existing column file
5. Write new checkpoints as intervals are crossed
6. Update `meta.entry_count` and `meta.log_file_size`

---

## Query Execution

### Metadata-Only Queries

Queries on flags/time never touch log content:

```
"severity distribution"
  → mmap flags (240MB for 60M lines)
  → single pass: bucket by bits 0-2
  → result in ~6ms

"errors in last 5 minutes"
  → mmap time → binary search for cutoff position
  → mmap flags from cutoff → scan with severity mask
  → touches only the tail of both columns

"top 10 templates"
  → mmap flags (240MB)
  → count by bits 16-31
  → result in ~6ms
```

### Content Queries (Pre-Filtered)

For queries that need field values (`count by (service)`):

```
json | level == "error" | count by (service)
  → mmap flags → find lines where severity=error AND format=json
  → only 2% of lines match (e.g., 1.2M out of 60M)
  → mmap offsets+lengths for those 1.2M lines
  → read and parse JSON for matched lines only
  → 50x less content parsing than full scan
```

### Text Search

Plain text search (`/connection timeout`) does not benefit from the index — the current mmap+memchr streaming filter is already near-optimal (~2-4 GB/s). The index doesn't change this path.

---

## Performance Estimates (60M lines)

### Index Sizes

| Column | Entry size | Total size |
|--------|-----------|------------|
| `flags` | 4 bytes | 240 MB |
| `time` | 8 bytes | 480 MB |
| `offsets` | 8 bytes | 480 MB |
| `lengths` | 4 bytes | 240 MB |
| `templates` | 2 bytes | 120 MB |
| **All columns** | **26 bytes** | **1.56 GB** |

Log file at ~150 bytes/line avg: ~9 GB. Index is ~17% of log size.

### Query Speed

| Query | Columns touched | Data scanned | Estimated time |
|-------|----------------|--------------|----------------|
| Severity distribution | `flags` | 240 MB | ~6 ms |
| Errors in last 5 min | `time` + `flags` | <100 MB (tail only) | ~2 ms |
| Template top-10 | `flags` | 240 MB | ~6 ms |
| Timeline histogram | `time` + `flags` | 720 MB | ~18 ms |
| MCP `stats` overview | `meta` only | 64 bytes | <0.01 ms |
| Count errors by service | `flags` + content | 240 MB + matched lines | ~50-100 ms |

### vs Alternatives

| Approach | "Severity distribution" on 60M lines |
|----------|---------------------------------------|
| **LazyTail columnar index** | **~6 ms** |
| lnav SQLite virtual table | ~8-12 s |
| DuckDB on CSV (cold) | ~2-4 s |
| DuckDB on Parquet (warm) | ~50-100 ms |
| ripgrep "ERROR" (text match, false positives) | ~1.5 s |

---

## Competitive Analysis

**vs lnav (SQLite):** 100-1000x faster for metadata queries. SQLite evaluates each row through its VM interpreter (~200ns/row). We scan a dense u32 array at memory bandwidth.

**vs Loki/Elastic/ClickHouse:** Different category — they need infrastructure. We're a local tool with zero setup, zero ETL. For the "understand these logs" workflow, faster to first answer.

**vs DuckDB:** Closest analytical competitor. But DuckDB has no real-time tailing, no incremental index, no capture pipeline, re-scans on every query.

**vs ripgrep:** Text search is equivalent (both mmap+SIMD). Structured queries are impossible with ripgrep — `rg "ERROR"` catches false positives in message content. Our flags give semantic filtering.

**Unique advantage:** Index builds inline during capture at zero marginal cost. Every other tool re-scans from scratch or requires a separate ingestion step.

---

## Extensibility

Adding a new column = adding a new file. No migration, no schema versioning:

```
templates   [u16; N]    2B/line     Drain cluster ID
field_hash  [u64; N]    8B/line     hash of extracted field values (future)
labels      variable    ?B/line     extracted key-value pairs (future, needs design)
```

If a column file doesn't exist, that feature isn't available yet — graceful degradation. Existing columns are never modified by adding new ones.

---

## File Format Conventions

- **Byte order:** Little-endian throughout (matches x86/ARM64, no conversion on common hardware)
- **Alignment:** Column files are naturally aligned by entry size (u32 files are 4-byte aligned, u64 files are 8-byte aligned). No padding between entries.
- **Mmap safety:** Files are append-only. Readers must not read past `meta.entry_count × entry_size`. Writers append then update `meta.entry_count` last (acts as a commit barrier).

---

## Integration with Existing Architecture

### SparseIndex Coexistence

The existing `SparseIndex` (one entry per 10K lines, in-memory, rebuilt on open) continues to work for basic viewing. The columnar index is an acceleration layer on top:

| Scenario | What's used |
|----------|-------------|
| Open file, scroll | SparseIndex (no columnar index needed) |
| Text filter (`/error`) | Streaming mmap+memchr (no index needed) |
| Structured query | Columnar `flags` (built on demand if missing) |
| Read matched line | Columnar `offsets`+`lengths` if available, else SparseIndex |

When the `offsets` column exists, it provides true O(1) line access (direct seek) instead of SparseIndex's O(1) amortized (seek + scan up to 10K lines). The SparseIndex is never explicitly replaced — it's just bypassed when a faster path exists.

### Streaming Filter Integration

The current streaming filter (mmap+memchr) remains the fast path for text search. The index enhances structured queries:

```
User enters: json | level == "error" | msg contains "timeout"

Without index:
  → streaming filter scans all lines
  → parse JSON on every line to check level and msg
  → O(N) JSON parses

With flags index:
  → scan flags: severity=error AND format=json
  → collect matching line numbers (eliminates ~98% of lines)
  → parse JSON only on survivors to check msg contains "timeout"
  → O(0.02N) JSON parses
```

### MCP Tool Integration

| MCP Tool | Index usage |
|----------|-------------|
| `list_sources` | `meta.entry_count` for line count (no file scan) |
| `search` with `query` | `flags` for pre-filtering, `offsets`+`lengths` for content |
| `get_tail` | `offsets`+`lengths` for O(1) access to last N lines |
| `get_lines` | `offsets`+`lengths` for O(1) random access |
| `stats` (planned) | `meta` + last checkpoint for instant severity counts |
| `aggregate` (planned) | `flags` for metadata aggregation, pre-filter for field aggregation |
| `log_cluster` (planned) | `templates` column for instant pattern counts |

### Capture Mode Changes

The capture write path (`capture.rs`) gains index writers alongside the log file writer:

```
Current:  stdin → log_file (+ tee to stdout)
Proposed: stdin → log_file + offsets + lengths + time + flags (+ tee to stdout)
```

Four additional sequential writes per line, all small (4-8 bytes each). OS write buffering absorbs this — the actual I/O cost is negligible compared to the existing `writeln!` + `flush()`.

---

## Scope Boundaries

**What gets indexed:**
- Files in `.lazytail/data/` (captured sources) — always indexed during capture
- Files referenced by `lazytail.yaml` `sources:` config — indexed on demand
- Files passed as CLI arguments — indexed on demand

**What does NOT get indexed:**
- Stdin/pipe sources (`StreamReader`) — data is in memory, no file to index
- Temporary/transient sources

**Index lifecycle:**
- When a source is deleted (user closes ended tab with confirmation), delete its index directory too
- `lazytail clear` should clear indexes alongside log files
- Index directory is always gitignored (lives under `.lazytail/`)

---

## Error Handling

### Partial Builds

If an index build is interrupted (crash, Ctrl+C, OOM):
- Column files may have fewer entries than `meta.entry_count` — detected by comparing file size vs `entry_count × entry_size`
- Recovery: truncate column files to the shortest one's entry count, update `meta.entry_count`, resume from there
- Checkpoints enable resuming without full rescan

### Concurrent Access

Multiple processes may interact with the same index (TUI viewing, MCP serving, capture writing):
- **Multiple readers:** Safe — mmap read is inherently concurrent
- **Single writer + readers:** Safe if readers don't read past `meta.entry_count`. Writer appends to columns then updates `entry_count` last.
- **Multiple writers:** Not supported. Only one capture process per source (existing PID-based marker prevents this).
- **Concurrent builds:** Use a lock file (`.lazytail/idx/{source}/build.lock`) to prevent two processes from building the same column simultaneously. Second process waits or skips.

---

## Implementation Phases

### Phase 1: Capture-Time Index (Foundation)

Build `offsets`, `lengths`, `time`, and `flags` inline during `lazytail -n` capture. This is the cheapest entry point — all data is already available in the capture write path.

**Scope:**
- Index writer module (append-only column file writer)
- `meta` file creation and updates
- `flags` detection: `is_json` (starts with `{`), `has_ansi` (contains `\x1b[`), basic severity (match common patterns)
- Wire into `capture.rs` write loop
- Checkpoint writing at intervals

**Result:** Captured sources get indexes for free. No query integration yet — just building and persisting the data.

### Phase 2: Index Reader + MCP Integration

Mmap-based column reader. Wire into MCP tools for instant metadata queries.

**Scope:**
- Index reader module (mmap column files, expose typed slices)
- `meta` validation and stale detection via checkpoints
- MCP `list_sources` uses `meta.entry_count` for line count
- MCP `search` uses `flags` for pre-filtering structured queries
- MCP `stats` tool using checkpoint cumulative counts

**Result:** MCP consumers see dramatically faster structured queries on captured sources.

### Phase 3: On-Demand Building for Existing Files

Background index builder for files opened without capture. Lazy column materialization.

**Scope:**
- Background builder thread (similar to current `build_index()`)
- Single-pass multi-column building
- Progress reporting (reuse existing `FilterProgress` channel pattern)
- Incremental update on file growth
- Partial rebuild via checkpoint validation

**Result:** All file sources get indexes, not just captured ones. First query pays the build cost, subsequent queries are instant.

### Phase 4: TUI Integration

Wire indexes into the TUI for severity highlighting, severity filtering in side panel, and timeline visualization.

**Scope:**
- Severity-based line coloring using `flags`
- Severity counts in stats panel using checkpoint data
- Severity filter toggle in side panel
- Pre-filtering structured queries in filter input

**Result:** The TUI becomes severity-aware with zero per-frame parsing cost.

---

## Search Result Bitmap Cache

A persistent cache of filter results stored as [Roaring Bitmaps](https://roaringbitmap.org/) — one file per search term, co-located with the columnar index.

### Motivation

Plain-text search (`/connection timeout`) runs mmap+memchr at ~2–4 GB/s — already fast. But for the same query run repeatedly (TUI re-filter, MCP repeated calls, compound queries), re-scanning 52M lines every time wastes cycles. A Roaring Bitmap makes repeat lookups sub-millisecond and enables bitwise compound queries for free.

For a 52M-line file a raw bitset would be 6.5 MB. A Roaring Bitmap is much smaller when matches are sparse (e.g. 1 000 hits → ~10 KB) and falls back to dense chunks automatically when matches are dense.

### Directory Layout

```
.lazytail/idx/{source_name}/
  meta
  checkpoints
  offsets
  lengths
  time
  flags
  templates
  search/                          ← new sub-directory
    {term_hash}.rbm                  Roaring Bitmap file (roaring crate serialization)
    {term_hash}.meta                 64-byte sidecar: canonical term + line_count_at_build
```

`search/` lives inside the existing index directory — same lifecycle, same gitignore, same deletion on source cleanup.

### Cache Key

`{term_hash}` = `xxhash64(canonical_term)` formatted as 16 hex chars.

**Canonical form rules:**
- Plain text: the raw search string as-is
- Regex: the source string of the compiled regex (not flags — anchoring, case are part of the string)
- Query (`json | level == "error"`): serialised `FilterQuery` AST (deterministic, not the raw input string)

The `.meta` sidecar stores the original human-readable term for debug/introspection and `line_count_at_build` for append detection.

### Sidecar Format (64 bytes)

```
offset  size  field
0       4     magic: [u8; 4] = b"LTSB"
4       2     version: u16 = 1
6       2     term_len: u16
8       8     line_count_at_build: u64    — meta.entry_count when bitmap was written
16      8     file_size_at_build: u64     — log file size when bitmap was written
24      8     built_at: u64              — epoch millis
32      32    term: [u8; 32]             — canonical term, truncated/zero-padded
```

### Cache Lookup Logic

```
FilterOrchestrator::trigger()
  → check search/{term_hash}.meta
      missing?            → cache miss, run streaming_filter, write bitmap
      line_count matches meta.entry_count AND file_size matches?
                          → cache hit, deserialize .rbm, return line numbers instantly
      file_size > at_build (append only)?
                          → partial hit: load bitmap, run streaming_filter on new lines only,
                            extend bitmap, update sidecar
      file_size changed in other way?
                          → stale, delete .rbm + .meta, run full streaming_filter, write new bitmap
```

### Append Extension

Because log files are append-only in the common case, the existing bitmap stays valid for lines `0..line_count_at_build`. Only new lines need scanning:

```rust
// Pseudocode
let old_bitmap = load_rbm(term_hash);              // valid for 0..old_count
let new_matches = streaming_filter(term, old_count..new_count);  // scan only tail
old_bitmap.extend(new_matches);                    // O(new_lines) not O(all_lines)
save_rbm(term_hash, &old_bitmap);
update_meta(term_hash, new_count, new_file_size);
```

For a 52M-line file where 1M new lines arrived since last search: scan 1M lines instead of 52M — **52× faster**.

### Compound Queries

With multiple bitmaps already on disk:

```
"error" bitmap  AND  "timeout" bitmap  →  bitwise AND, no file scan
"error" bitmap  OR   "warn" bitmap     →  bitwise OR,  no file scan
NOT "debug" bitmap                     →  bitwise NOT, no file scan
```

The `roaring` crate implements `BitAnd`, `BitOr`, `BitXor`, `Sub` directly on `RoaringBitmap`.

### columns_present Bitmask Update

```
bit 6: search_cache_dir    — search/ subdirectory exists with at least one .rbm
```

### Rust Crate

```toml
[dependencies]
roaring = "0.10"
```

Serialization: `bitmap.serialize_into(&mut file)` / `RoaringBitmap::deserialize_from(&file)` — the native roaring format, portable across platforms.

### Scope

- **In scope:** plain-text and regex searches on file sources
- **In scope:** query-syntax searches after the flags pre-filter stage
- **Out of scope:** stdin/stream sources (no file to cache alongside)
- **Out of scope:** case-insensitive variants (treated as separate cache entries via canonical form)

---

## Open Questions

1. **Labels / extracted field values:** Variable-length data doesn't fit the fixed-size column model. Options: separate auxiliary file with offset column, dictionary encoding, or defer to content parsing with flags pre-filtering. Needs design.

2. **Template ID assignment:** Drain clustering produces template IDs, but these aren't stable across rebuilds. Do we need deterministic template IDs? Does it matter if they change when the index is rebuilt?

3. **Compression:** Column files could be compressed (LZ4/zstd frame compression per 64KB block). Would reduce disk footprint 2-5x at cost of decompression during scan. Worth it for `time` column (likely very compressible due to monotonic values)? Probably premature for v1.

4. **Non-captured sources with no timestamps:** For existing files where timestamp parsing fails, the `time` column is all zeros. Should we skip building it entirely? Use line number as a proxy?

5. **Concurrent access:** TUI and MCP server may both read the index simultaneously while capture appends to it. Mmap handles concurrent read safely. Append + read needs care — reader must not read past `meta.entry_count`. Does `entry_count` need to be atomic / memory-mapped itself?

6. **Checkpoint interval tuning:** 100K lines is a guess. Smaller intervals = finer stale detection and faster partial rebuild, but more checkpoint entries and more frequent hashing during build. Profile to find the sweet spot.

7. **Flag bitmaps: migrate or layer?** The `flags` dense array and Roaring Bitmaps serve opposite query directions and should coexist rather than one replacing the other. The dense `[u32; N]` array is optimal for per-line lookup (O(1) direct indexing, used for display/rendering) and SIMD multi-flag checks in a single op. Roaring Bitmaps are optimal for "give me all line numbers with property X" queries. The decision: keep the dense flags column as source of truth, and add pre-built flag bitmaps as a derived layer in `search/` using reserved names (`flags:severity_error.rbm`, `flags:format_json.rbm`, etc.). Key difference from text search bitmaps: flag bitmaps can be written **during capture** at zero marginal cost since flag values are known at write time, rather than being built lazily on first query. The template ID field (bits 16–31, up to 65 535 values) is unsuitable for per-value bitmaps and stays in the dense column regardless.
