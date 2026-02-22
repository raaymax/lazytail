# Compression Strategies for LazyTail

*Design document for compression of log files, indexes, and network transfers.*

---

## Overview

LazyTail handles large log files (multi-GB) and builds substantial indexes. Compression can dramatically reduce storage and network costs while maintaining fast random access. This document explores compression strategies across three domains:

1. **Index compression** — columnar data (timestamps, offsets, lengths, flags)
2. **Log file compression** — the raw log content itself
3. **Network transfer compression** — MCP queries, results, and index metadata

---

## Index Compression

### Current Index Size (Uncompressed)

For a 6 GB log file with 52M lines:

| Column | Entry Size | Total Size | Description |
|--------|-----------|------------|-------------|
| `offsets` | 8 bytes (u64) | 397 MB | Byte position of each line |
| `time` | 8 bytes (u64) | 397 MB | Timestamp (epoch milliseconds) |
| `lengths` | 4 bytes (u32) | 199 MB | Line length in bytes |
| `flags` | 4 bytes (u32) | 199 MB | Metadata bitmask |
| `checkpoints` | 64 bytes | 32 MB | Sparse (every 100K lines) |
| `meta` | 64 bytes | 64 bytes | Header |
| **TOTAL** | **26 bytes/line** | **1.20 GB** | 20% of log size |

**Problem:** Index overhead is significant (20% of log size). For a 100 GB log, the index would be ~20 GB.

---

### Option 1: Delta Encoding + Bit Packing ⭐ **Recommended**

**Concept:** Store the first value, then store deltas (differences) using only the bits needed.

#### Timestamps (Monotonic)

```rust
// Instead of:
[1740000000000, 1740000000100, 1740000000250, ...]  // 64 bits each

// Store as:
base: 1740000000000  // 8 bytes
deltas: [100, 150, 80, 95, ...]  // Pack into 14-24 bits each
```

**Bit width auto-detection:**
```rust
// During index build
let max_delta = deltas.iter().max();
let bit_width = 64 - max_delta.leading_zeros();  // Minimal bits needed
```

**For typical logs:**
- High-volume: deltas < 10ms → 14 bits per entry
- Medium-volume: deltas < 1s → 20 bits per entry
- Low-volume: deltas < 16s → 24 bits per entry

**Access pattern:**
```rust
fn get_timestamp(&self, line: u64) -> u64 {
    let bit_offset = line * self.bit_width as u64;
    let byte_offset = (bit_offset / 8) as usize;
    let bit_shift = (bit_offset % 8) as u8;

    let delta = extract_bits(&self.packed[byte_offset..], bit_shift, self.bit_width);
    self.base + delta
}
```

**Performance:**
- ✅ True O(1) access (bit shifting + masking)
- ✅ Overhead: ~5-10ns vs direct array access
- ✅ Cache-friendly (smaller = fewer cache misses)

#### Offsets (Monotonic)

Same strategy as timestamps. Line byte offsets grow monotonically.

**For typical logs (avg line length 120 bytes):**
- Deltas fit in 8-12 bits (max line length ~4KB)
- With occasional long lines: 16-20 bits

#### Expected Savings

| Column | Current | Delta+BitPack | Savings |
|--------|---------|---------------|---------|
| `offsets` | 397 MB | 150 MB (20 bits) | 62% |
| `time` | 397 MB | 130 MB (20 bits) | 67% |
| `lengths` | 199 MB | 60 MB (10 bits) | 70% |
| `flags` | 199 MB | 199 MB* | 0% |
| **TOTAL** | **1192 MB** | **539 MB** | **55%** |

*Flags: Not monotonic, no benefit from delta encoding.

**Implementation complexity:** Medium (~300 lines of code)

---

### Option 2: Frame-Based LZ4 Compression

**Concept:** Divide column into frames (e.g., 8192 values), compress each frame independently, build frame index.

```
Frame layout:
┌─────────────────────────────────────┐
│ Frame 0: [values 0..8191]     → compressed → 32 KB
│ Frame 1: [values 8192..16383] → compressed → 28 KB
│ ...
└─────────────────────────────────────┘

Frame Index: [(offset: 0, size: 32KB), (offset: 32KB, size: 28KB), ...]
```

**For 52M lines:**
- 52M / 8192 = 6,348 frames
- Frame index: 6,348 × 16 bytes = ~102 KB overhead
- Compression ratio: ~3-5x for monotonic data
- 397 MB → ~80-130 MB

**Access pattern:**
```rust
fn get_offset(&self, line: u64) -> u64 {
    let frame_id = line / FRAME_SIZE;
    let frame_offset = line % FRAME_SIZE;

    // O(1) frame lookup
    let compressed_frame = &self.frames[frame_id];

    // Decompress frame (~20μs for 64KB with LZ4)
    let decompressed = lz4_decompress(compressed_frame);
    decompressed[frame_offset]
}
```

**Performance:**
- ⚠️ Near-O(1) but with decompression cost
- LZ4 decompress: ~3 GB/s → **~20μs per frame** (64 KB)
- Can cache recently decompressed frames (LRU)

**Expected savings:**
- 67-80% reduction on top of delta encoding
- Combined: 1192 MB → ~270 MB (77% total savings)

**Trade-off:** Extra 70% savings for 20μs overhead per cold access.

**Use case:** Optional for MCP server (queries are infrequent), skip for TUI (needs instant scrolling).

---

### Option 3: Dictionary Encoding for Flags

**Concept:** Store unique flag values once, reference by index.

```rust
struct DictColumn {
    dict: Vec<u32>,       // Unique flag values
    indices: Vec<u16>,    // Index per line (if <65K unique values)
}
```

**Effectiveness depends on cardinality:**
- **Low cardinality (1K unique):** 199 MB → 4 KB (dict) + 104 MB (indices) = **48% savings**
- **High cardinality (100K unique):** 199 MB → 400 KB (dict) + 104 MB (indices) = **~47% savings**
- **Very high cardinality (>1M unique):** Minimal savings, not worth complexity

**For typical logs:** Flags have moderate cardinality (format + severity + features = ~100-10K unique combinations). Expected savings: **30-50%**.

**Performance:** ✅ True O(1), same speed as array access

---

### Hybrid Recommendation

**Phase 1: Delta + Bit Packing** (High value, medium effort)
- Implement for `time` and `offsets` columns
- 55% index size reduction (1.2 GB → 539 MB)
- Maintains O(1) access (~10ns overhead)

**Phase 2: Optional Frame Compression** (Extra savings, adds latency)
- Add `--index-compression=lz4` flag for capture mode
- MCP server can use it (tolerates 20μs latency)
- TUI skips it (needs instant access)
- Extra 70% reduction (539 MB → 270 MB)

**Phase 3: Dictionary Encoding for Flags** (Evaluate after Phase 1)
- Profile actual flag cardinality on real logs
- Only implement if cardinality is low (<10K unique)

---

## Log File Compression

### Benchmark Results (6 GB systemd journal)

Using frame-based LZ4 compression (1000 lines per frame):

```
Original size:       5.98 GB
Compressed size:     643 MB
Compression ratio:   9.52x
Space saved:         89.5%
Compression speed:   1.57 GB/s
```

**Why such high compression?**
- Systemd journals have highly repetitive structure
- Same field names repeated millions of times (`_SYSTEMD_UNIT=`, `MESSAGE=`, etc.)
- Many identical or nearly-identical messages
- Timestamps differ by only a few characters
- LZ4 excels at finding and referencing repeated patterns

**Other log types:**
- **Application logs (text):** 3-5x compression
- **JSON logs:** 5-10x compression (field names repeat)
- **Mixed format logs:** 2-4x compression

---

### Option 1: Transparent Filesystem Compression 🚀 **Zero-code option**

**Linux — btrfs with zstd:**
```bash
# Enable compression on existing directory
sudo btrfs property set ~/.config/lazytail/data compression zstd

# Or mount with compression
sudo mount -o remount,compress=zstd /dev/sda1
```

**Linux — zfs with LZ4:**
```bash
zfs set compression=lz4 tank/lazytail
```

**macOS — APFS compression:**
```bash
# Compress specific files
ditto --compress /source/log /dest/log.compressed
```

**Windows — NTFS compression:**
```powershell
compact /c /s:C:\lazytail\data
```

**Benefits:**
- ✅ Works **today** with zero code changes
- ✅ Transparent to LazyTail (mmap still works!)
- ✅ OS handles compression/decompression automatically
- ✅ 5-10x compression ratio (similar to frame compression)
- ✅ Minimal CPU overhead (< 5%)

**Trade-offs:**
- ⚠️ User must configure filesystem (not automatic)
- ⚠️ Performance varies by filesystem implementation
- ⚠️ Not portable (compressed on one machine, not automatically on another)

**Verdict:** Best immediate solution. Document in README with instructions per OS.

---

### Option 2: Frame-Based Native Compression ⭐ **Best long-term option**

**Concept:** Compress logs in independent frames during capture, decompress on-demand.

```rust
// Capture mode with compression
struct CompressedLogWriter {
    frame_buffer: Vec<u8>,       // Accumulate lines
    frame_size: usize,            // Lines per frame (e.g., 1000)
    compressed_file: File,        // Append compressed frames
    frame_index: Vec<u64>,        // Byte offset of each frame
}
```

**Write path (capture mode):**
```rust
fn write_line(&mut self, line: &[u8]) -> Result<()> {
    self.frame_buffer.extend_from_slice(line);
    self.frame_buffer.push(b'\n');
    self.lines_in_frame += 1;

    if self.lines_in_frame >= self.frame_size {
        // Compress frame
        let compressed = lz4_compress(&self.frame_buffer);
        let offset = self.compressed_file.stream_position()?;
        self.compressed_file.write_all(&compressed)?;

        // Update frame index
        self.frame_index.push(offset);

        // Reset buffer
        self.frame_buffer.clear();
        self.lines_in_frame = 0;
    }

    Ok(())
}
```

**Read path:**
```rust
fn get_line(&self, line_num: u64) -> Result<String> {
    let frame_id = line_num / FRAME_SIZE;
    let line_in_frame = line_num % FRAME_SIZE;

    // Check LRU cache first
    if let Some(frame) = self.frame_cache.get(frame_id) {
        return Ok(frame.lines[line_in_frame].clone());
    }

    // Cache miss: load and decompress frame
    let offset = self.frame_index[frame_id];
    let compressed = self.read_frame_at(offset)?;
    let decompressed = lz4_decompress(&compressed)?;  // ~50μs for 64KB

    // Cache decompressed frame (keep ~10 frames cached)
    let frame = parse_lines(decompressed);
    self.frame_cache.put(frame_id, frame.clone());

    Ok(frame.lines[line_in_frame].clone())
}
```

**Performance:**
- ✅ Random access: O(1) frame lookup + 50μs decompress
- ✅ Sequential reading: Decompress once per 1000 lines
- ✅ Follow mode: Append new frames as data arrives
- ✅ LRU cache: Hot frames stay decompressed (scrolling is instant)
- ⚠️ Cold access: 50μs overhead (noticeable for fast TUI scrolling)

**Expected savings:**
- **Systemd journals:** 9x compression (6 GB → 644 MB)
- **Application logs:** 3-5x compression
- **JSON logs:** 5-10x compression

**Trade-offs:**
- ✅ Huge space savings (70-90%)
- ✅ Works well with LRU cache for TUI
- ⚠️ Adds complexity to capture and read paths
- ⚠️ Only benefits newly captured logs (existing logs unaffected)
- ⚠️ Need careful tuning of frame size and cache size

**Implementation complexity:** High (~800 lines of code, careful cache management)

---

### Option 3: Hybrid Hot/Cold Storage

Keep recent data uncompressed, compress old data:

```
.lazytail/data/mylog/
  mylog.log          ← Last 100K lines (uncompressed, fast)
  mylog.lz4.000      ← Lines 0-99,999 (compressed)
  mylog.lz4.001      ← Lines 100K-199K (compressed)
  ...
```

**Aging policy:**
```rust
// When active log reaches threshold
if log.line_count() >= 100_000 {
    // Compress old data
    compress_range(&log, 0..99_000, "mylog.lz4.001")?;

    // Keep recent 1K lines in hot log
    truncate_log(&log, 99_000)?;

    // Update index offsets
    update_index_offsets(&index, 99_000)?;
}
```

**Benefits:**
- ✅ Follow mode stays fast (recent data uncompressed)
- ✅ 90% of file is compressed (older data)
- ✅ Can archive old frames to S3/cold storage
- ✅ Automatic aging policy

**Trade-offs:**
- ⚠️ More complex file management
- ⚠️ Need to handle multi-file reads
- ⚠️ Index must track which file each line is in

---

### Option 4: Archive Format (.lzt)

Add `lazytail compress` command for historical/archival logs:

```bash
# Compress old logs into portable archive
lazytail compress old-logs/*.log --output archive-2024.lzt

# Still searchable/viewable
lazytail view archive-2024.lzt
lazytail search archive-2024.lzt --query "level == error"
```

**.lzt file format:**
```
┌─────────────────────────────┐
│ Header (256 bytes)          │
│  - Magic: "LTZA" (4 bytes)  │
│  - Version: u16             │
│  - Compression: u8          │
│  - Index offset: u64        │
│  - Data offset: u64         │
│  - Reserved: 233 bytes      │
├─────────────────────────────┤
│ Embedded Index              │
│  - Meta (64 bytes)          │
│  - Checkpoints (variable)   │
│  - Column indices (offsets) │
├─────────────────────────────┤
│ Frame Index                 │
│  - [frame_offset: u64; N]   │
├─────────────────────────────┤
│ Compressed Log Data         │
│  - Frame 0 (LZ4/Zstd)       │
│  - Frame 1 (LZ4/Zstd)       │
│  - ...                      │
└─────────────────────────────┘
```

**Properties:**
- ✅ Single portable file (can move/share easily)
- ✅ Self-contained index (no separate `.lazytail/idx/` directory)
- ✅ 5-10x compression (index + log)
- ✅ Read-only (safe for archival)
- ✅ Still searchable with LazyTail TUI and MCP

**Use case:**
- Archive old logs for long-term storage
- Share log files with colleagues (single file, compressed)
- Cloud storage / backup (10x smaller)

---

### Compression Strategy Recommendation

**Immediate (Phase 0):**
- Document filesystem compression in README
- Users can enable btrfs/zfs compression today (zero code changes)
- 5-10x savings, transparent

**Phase 1: Frame-Compressed Capture Mode**
- Implement native frame compression for `lazytail -n`
- Only newly captured logs benefit
- 9x compression for systemd journals, 3-10x for others
- LRU cache keeps TUI fast

**Phase 2: Archive Format**
- Add `lazytail compress` command
- Convert old logs to `.lzt` format
- Perfect for long-term storage and sharing

**Skip for now:**
- Hybrid hot/cold storage (complexity not justified)
- Compressing existing logs in-place (use filesystem compression instead)

---

## Network Transfer Compression

### Use Case: MCP Protocol

LazyTail's MCP server transfers data over stdin/stdout (JSON-RPC). Common transfers:

1. **Index metadata** — source list, line counts, severity stats
2. **Query results** — matching line numbers, line content
3. **Log chunks** — tail/head/range requests

### Current Overhead

**Example: `search` tool with 10K matches:**
```json
{
  "matches": [
    {"line": 12345, "content": "2024-01-01 ERROR ..."},
    {"line": 12389, "content": "2024-01-01 ERROR ..."},
    ...  // 10K entries
  ]
}
```

**Size:** ~5-10 MB JSON (if avg line length is 500 bytes)

---

### Option 1: Transport-Layer Compression

MCP protocol supports compression at the transport layer (stdio, SSE, HTTP):

**stdio transport with gzip:**
```rust
// Server wraps stdout with gzip encoder
let stdout = io::stdout();
let compressed = GzipEncoder::new(stdout, Compression::default());
let writer = BufWriter::new(compressed);
```

**Benefits:**
- ✅ Transparent (no protocol changes)
- ✅ Works for all MCP tools automatically
- ✅ 5-10x compression for JSON responses
- ⚠️ Adds latency (~1-5ms for compression)

**Supported by rmcp crate:** Check if `rmcp` supports transparent compression.

---

### Option 2: Binary Protocol for Large Transfers

Instead of JSON for large results, use binary format:

**Query results in binary:**
```rust
// Instead of JSON array of objects
struct BinarySearchResult {
    match_count: u32,                // 4 bytes
    line_numbers: Vec<u64>,          // 8 bytes × N
    content_offsets: Vec<u32>,       // 4 bytes × N (offset into content blob)
    content_blob: Vec<u8>,           // Concatenated line content
}
```

**Size comparison for 10K matches:**
- **JSON:** ~5-10 MB (verbose)
- **Binary:** ~50 KB (line numbers) + ~5 MB (content) = ~5 MB
- **Binary + LZ4:** ~500 KB (10x smaller)

**Trade-off:** Breaks JSON-RPC compatibility. MCP clients expect JSON.

**Verdict:** Not recommended unless MCP spec adds binary support.

---

### Option 3: Chunked Streaming for Large Results

Instead of sending all 10K matches at once, stream in chunks:

```json
// First chunk
{"type": "partial", "matches": [...1000 matches...], "total": 10000}

// Second chunk
{"type": "partial", "matches": [...1000 matches...], "total": 10000}

// Final chunk
{"type": "complete", "matches": [...], "total": 10000}
```

**Benefits:**
- ✅ Client can start displaying results immediately
- ✅ Lower memory usage (don't buffer full result)
- ✅ Can combine with compression per chunk
- ⚠️ Requires MCP client to handle streaming

**Compatibility:** Check if MCP protocol supports streaming responses.

---

### Option 4: Columnar Result Format

Send results in columnar format (like the index):

```json
{
  "line_numbers": [12345, 12389, 12401, ...],  // Array of u64
  "content": [
    "2024-01-01 ERROR ...",
    "2024-01-01 ERROR ...",
    ...
  ]
}
```

Then compress the JSON with gzip:
- Line numbers: highly compressible (sorted, small deltas)
- Content: compressible (repeated patterns)

**Expected compression:** 10-20x for line numbers, 3-5x for content.

**Verdict:** Combine with Option 1 (transport-layer compression) for best results.

---

### Network Compression Recommendation

**Phase 1: Transport-Layer Compression**
- Enable gzip/zstd on MCP stdio transport
- Automatic for all tools
- 5-10x compression on JSON responses
- Check `rmcp` crate support

**Phase 2: Columnar Result Format**
- Restructure large results (search, get_lines) to columnar JSON
- Better compression ratios (line numbers compress 20x)
- Compatible with JSON-RPC

**Future: Binary Protocol**
- Wait for MCP spec to support binary transfers
- Or add LazyTail-specific binary mode (non-standard)

---

## Combined Savings Estimate

For a 6 GB log file with 52M lines:

| Component | Current | Optimized | Strategy | Savings |
|-----------|---------|-----------|----------|---------|
| **Log file** | 6.0 GB | 644 MB | Frame compression (9x) | 89% |
| **Index** | 1.2 GB | 539 MB | Delta + bit packing | 55% |
| **Index** | 1.2 GB | 270 MB | + Frame compression | 77% |
| **Network (10K results)** | 5 MB | 500 KB | Columnar + gzip | 90% |

**Total storage (log + index):**
- Current: 7.2 GB
- Optimized (delta): 6.6 GB (8% savings)
- Optimized (delta + log frames): 1.18 GB (84% savings) ⭐
- Optimized (delta + log frames + index frames): 914 MB (87% savings)

**Key insight:** Log compression dominates savings. Index compression is secondary but still valuable for very large indexes.

---

## Implementation Roadmap

### Phase 0: Documentation (Immediate)
- Document filesystem compression in README
- Provide setup instructions for btrfs/zfs/NTFS compression
- Users can enable today for 5-10x log compression

### Phase 1: Index Compression (Medium effort, high value)
- Implement delta + bit packing for `time` and `offsets` columns
- Auto-detect bit width during index build
- 55% index size reduction (1.2 GB → 539 MB)
- ~300 lines of code

### Phase 2: Frame-Compressed Capture (High effort, very high value)
- Implement frame-based LZ4 compression for `lazytail -n`
- LRU frame cache for read path
- 9x log compression (6 GB → 644 MB)
- ~800 lines of code

### Phase 3: Archive Format (Medium effort, niche value)
- Add `lazytail compress` command
- Design `.lzt` file format
- Useful for long-term archival and sharing
- ~400 lines of code

### Phase 4: Network Compression (Low effort, medium value)
- Enable transport-layer compression in MCP server
- Restructure large results to columnar JSON
- 5-10x network transfer savings
- ~100 lines of code

---

## Performance Considerations

### CPU Overhead

**LZ4 compression:**
- Speed: 1.5-2.0 GB/s (single-threaded)
- Decompression: 3-4 GB/s
- CPU usage: ~5-10% during capture
- Verdict: Negligible overhead

**Delta + bit packing:**
- Speed: Limited by memory bandwidth (~10 GB/s)
- Overhead: 5-10ns per access
- Verdict: Negligible overhead

### Memory Usage

**Frame cache (10 frames, 1000 lines each):**
- Uncompressed: 10 × 64 KB = 640 KB
- Negligible compared to TUI rendering buffers

**Index decompression:**
- If using frame compression: 64 KB per frame
- Keep 10 frames cached: 640 KB
- Acceptable overhead

### Disk I/O

**Compressed logs:**
- Read amplification: 9x fewer bytes read from disk
- Decompression in CPU: Much faster than disk I/O
- Net win: ~5-10x faster cold reads (SSD), ~20-50x faster (HDD)

**Compressed indexes:**
- Random access pattern benefits from smaller size
- More data fits in page cache
- Net win: ~2-5x faster queries

---

## Open Questions

1. **Frame size tuning:** Should frame size be configurable or auto-tuned based on log characteristics?
   - Small frames (100 lines): Lower latency, worse compression
   - Large frames (10K lines): Better compression, higher latency
   - Recommendation: 1000 lines (good balance)

2. **Compression algorithm:** LZ4 vs Zstd vs Snappy?
   - LZ4: Fastest decompression (3-4 GB/s), moderate compression (3-5x)
   - Zstd: Better compression (5-10x), slower decompression (1-2 GB/s)
   - Snappy: Fastest compression, weakest ratio (2-3x)
   - Recommendation: LZ4 for default, Zstd as optional (--compression=zstd)

3. **Backward compatibility:** How to handle old uncompressed logs after implementing compression?
   - Option 1: Support both formats (detect on open)
   - Option 2: Migrate command (lazytail migrate --compress)
   - Recommendation: Option 1 (auto-detect)

4. **Index compression opt-in:** Should compressed indexes be opt-in or default?
   - Delta + bit packing: Minimal overhead, default ON
   - Frame compression: 20μs overhead, opt-in for MCP, OFF for TUI
   - Recommendation: Smart default (ON for MCP, OFF for TUI)

5. **Compression level:** Should compression level be configurable?
   - LZ4 has levels 1-9 (default: 1, fastest)
   - Higher levels: Better compression, slower
   - Recommendation: Default level 1, allow `--compression-level=N`

6. **Network protocol:** When MCP adds binary support, should we switch immediately?
   - Pro: 10x smaller transfers
   - Con: Breaking change for clients
   - Recommendation: Wait for MCP spec, or add opt-in binary mode
