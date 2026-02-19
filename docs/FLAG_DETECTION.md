# Flag Detection Design

*Per-line metadata detection for the columnar index system.*

---

## Problem

`detect_flags(line) -> u32` is called for every line during index building — both inline during capture and when bulk-building indexes for existing files. At 60M lines, even small per-line costs compound. The current implementation makes 4-10 separate passes over each line's prefix.

---

## Bulk Build Pipeline

The index builder's inner loop has three steps:

```
for each line in mmap:
    1. Find newline       memchr(b'\n')          SIMD, ~15-20 GB/s
    2. Detect flags       detect_flags_bytes()    scalar + memchr hybrid
    3. Write 4 columns    batched BufWriter       sequential I/O
```

### Cost breakdown (9 GB file, 60M lines, 150 bytes avg)

| Step | Method | Time | % of total |
|------|--------|------|------------|
| Newline detection | `memchr(b'\n')` SIMD | ~0.5s | ~20% |
| Flag detection | hybrid (see below) | ~1.4s | ~55% |
| Column writes | batched BufWriter | ~0.3s | ~12% |
| Mmap page faults | OS | ~0.3s | ~12% |
| **Total** | | **~2.5s** | |

Newline detection is essentially free — `memchr` uses AVX2/NEON, scanning 32 bytes per cycle. For a 150-byte line that's ~5 cycles (~1.7ns) to find the `\n`.

Flag detection is the bottleneck at ~55% of total time.

### Why NOT fuse newline detection with flag detection

Tempting to scan byte-by-byte for `\n` AND flags in one loop. But this is slower:

```
memchr SIMD:     32 bytes/cycle, ~0.1 cycles/byte
scalar per-byte: 1 byte/cycle, ~3-5 cycles/byte (flag check branches)
```

For a 150-byte line:
- Separate: memchr(150 bytes) + flags(120 bytes) = ~5 + 350 = **355 cycles**
- Fused: scalar(150 bytes × 4 cycles) = **600 cycles** (worse)

The data stays in L1 cache between the memchr pass and the flag pass, so the "second read" of the first 120 bytes is essentially free (~1 cycle/load from L1).

---

## memchr-Assisted Detection (Recommended Default)

Use the `memchr` crate's SIMD internals for candidate scanning, then scalar verification at candidate positions. This is the default for mixed log streams — no format assumptions.

### How it works

Most flag detections reduce to "find a marker byte, then verify context":

| Flag | SIMD scan | Scalar verify |
|------|-----------|---------------|
| ANSI | `memchr(0x1B, line)` | (none needed — 0x1B is definitive) |
| Logfmt | `memchr(b'=', prefix)` | check key chars before, value after |
| Timestamp | `memchr2(b'-', b':', first_30)` | verify YYYY- or HH:MM:SS digit pattern |
| JSON | (no scan — check `prefix[trimmed_start]`) | (none — `{` is sufficient) |
| Severity | scalar first-byte dispatch | verify keyword + word boundary |

Severity stays scalar because the prefix is only 80 bytes — SIMD setup cost doesn't amortize.

### Per-line cost

| Detection | Method | Cycles |
|-----------|--------|--------|
| JSON | single byte check | ~1 |
| ANSI | `memchr(0x1B)` full line | ~10 |
| Logfmt | `memchr(b'=')` 120 bytes + verify | ~12 |
| Timestamp | `memchr2(b'-', b':')` 30 bytes + verify | ~8 |
| Severity | scalar dispatch over 80 bytes | ~40 |
| **Total** | | **~70 cycles (~23ns)** |

vs current multi-pass: ~300+ cycles (~100ns). **~4x faster, zero unsafe code.**

### API

```rust
pub fn detect_flags_bytes(bytes: &[u8]) -> u32 { /* memchr-assisted impl */ }

pub fn detect_flags(line: &str) -> u32 {
    detect_flags_bytes(line.as_bytes())
}
```

The `&[u8]` entry point avoids UTF-8 validation for bulk building from mmap.

---

## Severity: First-Match-in-Text

The single-pass approach finds the first severity keyword scanning left-to-right. This is more correct than the current priority-order (fatal > error > warn > ...) because the actual severity is the first keyword:

```
"2024-01-01 INFO processing error count=5"

Priority-order (current):  scans for "error" → finds it → returns ERROR  ← WRONG
First-match (single-pass): hits "INFO" first → returns INFO               ← CORRECT
```

At each word boundary, dispatch on the first byte (case-folded via `| 0x20`):

```rust
match bytes[i] | 0x20 {
    b'f' if remaining >= 5 => check "fatal" + word_end → FATAL
    b'e' if remaining >= 5 => check "error" + word_end → ERROR
    b'w' if remaining >= 4 => check "warning" or "warn" + word_end → WARN
    b'i' if remaining >= 4 => check "info" + word_end → INFO
    b'd' if remaining >= 5 => check "debug" + word_end → DEBUG
    b't' if remaining >= 5 => check "trace" + word_end → TRACE
    _ => continue
}
```

Keyword verification uses `u32` loads instead of byte-by-byte:

```rust
fn eq_ci4(hay: &[u8], needle: &[u8; 4]) -> bool {
    let h = u32::from_le_bytes([hay[0]|0x20, hay[1]|0x20, hay[2]|0x20, hay[3]|0x20]);
    let n = u32::from_le_bytes(*needle);
    h == n && (hay.len() <= 4 || !hay[4].is_ascii_alphabetic())
}
```

One compare instruction for 4 bytes instead of 4 separate comparisons.

### ANSI: skip-in-place

Instead of stripping ANSI to a temporary buffer then re-scanning, skip CSI sequences inline. When `0x1B` is hit during severity scan, advance past the CSI sequence. Word-boundary check naturally works because the cursor jumps over the `m` terminator.

---

## CPU Cache Utilization

### Read side — already optimal

- Sequential mmap access → hardware prefetcher keeps pages hot
- `memchr` reads the line, flag detection re-reads first 120 bytes → L1 hit
- Working set: current mmap pages (~4-8KB) → well within L1

### Write side — needs batching

**Problem:** Four `push()` calls per line hit four different BufWriter buffers. Each call has function overhead (bounds check, copy, advance pointer) = ~5-10 cycles. Four calls = ~20-40 cycles per line.

**Fix:** Batch accumulate into contiguous arrays, flush periodically:

```rust
const BATCH: usize = 1024;
let mut off_buf = [0u64; BATCH];  // 8 KB  ─┐
let mut len_buf = [0u32; BATCH];  // 4 KB   │ 24 KB total
let mut flg_buf = [0u32; BATCH];  // 4 KB   │ fits in L1 cache
let mut tim_buf = [0u64; BATCH];  // 8 KB  ─┘
let mut idx = 0;

while pos < data.len() {
    let line_end = memchr::memchr(b'\n', &data[pos..])...;
    let line = &data[pos..line_end];

    off_buf[idx] = pos as u64;         // sequential writes to
    len_buf[idx] = line.len() as u32;  // contiguous memory —
    flg_buf[idx] = detect_flags_bytes(line); // same cache line
    tim_buf[idx] = now;                // hit ~8 times in a row
    idx += 1;

    if idx == BATCH {
        offsets_writer.push_batch(&off_buf)?;  // one memcpy
        lengths_writer.push_batch(&len_buf)?;
        flags_writer.push_batch(&flg_buf)?;
        time_writer.push_batch(&tim_buf)?;
        idx = 0;
    }
    pos = line_end + 1;
}
```

**Why this is better:**
- Writes are contiguous: `off_buf[0], [1], [2]...` hit the same cache line ~8 times (64B line / 8B per u64) before advancing
- One `write_all` per 1024 entries instead of 1024 separate calls
- 24 KB batch buffers fit entirely in L1 (48-64KB typical)
- Per-line write cost: ~20-40 cycles → **~5 cycles**

---

## Can We Process 32 Bytes at Once?

Yes, but the practical benefit depends on the detection type.

### Where SIMD helps (via memchr)

`memchr` internally uses AVX2/NEON to compare 32 bytes per cycle. We get this for free:

```
memchr(0x1B, line)           → 32 bytes/cycle, finds ANSI in ~10 cycles
memchr(b'=', prefix)         → 32 bytes/cycle, finds logfmt candidate in ~5 cycles
memchr2(b'-', b':', prefix)  → 32 bytes/cycle, finds timestamp candidate in ~3 cycles
```

### Where SIMD doesn't help

**Severity (80-byte prefix):** 80 bytes = 2.5 AVX2 loads. SIMD could find candidate start bytes ('e', 'w', 'i', 'd', 't', 'f') in ~10 cycles, but we still need scalar verification (word boundary + full keyword match) at each hit. Total SIMD approach: ~25 cycles. Scalar dispatch approach: ~40 cycles. The ~15 cycle gap isn't worth raw intrinsics on a 80-byte buffer.

**Raw AVX2 single-pass (theoretical):**

```
Load 32 bytes from prefix
    → cmpeq 0x1B              → ANSI candidates
    → cmpeq '='               → logfmt candidates
    → cmpeq ':'               → timestamp candidates
    → OR with 0x20, cmpeq 'e' → severity candidates
    → ...6 more comparisons
    → OR all masks → movemask → bitmask of all interesting positions
    → if bitmask == 0, skip entire 32-byte chunk
```

4 loads × ~10 cycles = ~40 cycles for all detections at once. But:
- Requires `unsafe` + `#[cfg(target_arch)]` + fallback for non-AVX2
- ~500 lines of platform-specific intrinsics
- Saves ~30 cycles/line over the memchr hybrid
- At 60M lines: saves ~0.6s (from ~1.4s to ~0.8s)

Not worth the complexity. The memchr hybrid gets 80% of the SIMD benefit with safe, portable code.

---

## Format-Aware Detection (Opt-In)

For uniform-format files, a `format` config flag enables specialized fast paths that skip irrelevant checks. This is opt-in — the default is the generic memchr-assisted detector for mixed streams.

### Configuration

```yaml
# lazytail.yaml
sources:
  - name: api-logs
    command: docker logs api
    format: json          # opt-in: skip logfmt, ANSI, timestamp pattern checks
    severity_key: level   # optional: override default "level" key
```

### Format-specific fast paths

**JSON** (`format: json`):
```
Knows: always FLAG_FORMAT_JSON, never logfmt
Severity: memmem::find(b"\"level\":\"") → one byte check (~8ns)
Timestamp: always FLAG_HAS_TIMESTAMP
ANSI: skip
```

**Logfmt** (`format: logfmt`):
```
Knows: always FLAG_FORMAT_LOGFMT, never JSON
Severity: memmem::find(b"level=") → one byte check (~8ns)
Timestamp: memmem::find(b"ts=") or b"time="
ANSI: skip
```

**Plain text** (`format: plain` or default):
```
Full generic memchr-assisted detection
```

### Auto-detection (future)

Sample first 20 lines, infer format, switch to fast path. Re-check periodically (every checkpoint interval) to handle format changes mid-file.

---

## Theoretical Limits

**Memory bandwidth ceiling:** ~20 GB/s. At 150 bytes/line: 133M lines/sec. We achieve ~43M lines/sec with the memchr hybrid (~23ns/line). Gap: ~3x.

**Where the gap goes:**
- Flag detection CPU: ~2x (scalar severity scan over 80 bytes)
- Write overhead: ~0.5x (batched writes, amortized syscalls)
- mmap fault overhead: ~0.5x (OS kernel, TLB misses)

**Closing the gap further requires:** raw SIMD for severity detection (~0.5s saved) or format-aware paths that skip most checks (~1s saved). Both are diminishing returns since I/O costs (page faults, column writes) establish a floor of ~0.6-0.8s regardless of detection speed.

---

## Implementation Order

1. **Rewrite `detect_flags`** as memchr-assisted `detect_flags_bytes(&[u8])` with `detect_flags(&str)` wrapper. Scalar first-byte dispatch for severity, `memchr` for ANSI/logfmt/timestamp.
2. **Add `push_batch`** to ColumnWriter (already exists) and use batched writes in the builder.
3. **Benchmark** with `cargo bench` on real log files (JSON, logfmt, plain text, ANSI-colored, mixed).
4. **Add format hints** in config if benchmarks show detection is the bottleneck (vs I/O).
5. Wire into bulk builder and capture path.
