# ADR-026: Index Validation with Partial Trust

## Status

Accepted

## Context

LazyTail's columnar index (ADR-015) is built during capture and stored alongside log files. Several failure scenarios can leave the index stale or corrupt:

- **Application crash**: capture terminates before flushing the final checkpoint, leaving entry counts or column sizes inconsistent.
- **Log file truncation**: an external process truncates the log file, causing indexed byte offsets to point beyond the end of the file.
- **File replacement**: the log file is replaced with different content of similar or identical size, making checkpoint content hashes invalid even though structural sizes appear correct.
- **Wrong base offset**: capture restarts and re-indexes from byte 0 on a file that already has content, producing offsets that point to the wrong lines.

Blindly trusting a corrupt index causes wrong query results, incorrect severity statistics, or panics from out-of-bounds reads. However, fully rebuilding the index on every mismatch would be slow for large files, and the index builder lives in capture mode --- the viewer cannot rebuild it.

Alternatives considered:
- **Full re-read validation**: read every line offset and compare against the log file. Accurate, but O(n) in file size and too slow for multi-gigabyte logs.
- **Ignore corruption**: treat the index as always valid. Leads to panics and wrong results.
- **Automatic rebuild in the viewer**: the viewer would need to embed the full indexing pipeline, violating separation of concerns between capture and viewing.

## Decision

Implement checkpoint-based validation in `src/index/validate.rs`. The `validate_index()` function runs when `IndexReader::open()` is called and when `IndexReader::stats()` computes statistics. It validates the index against the log file in two phases:

### Phase 1: Structural checks (instant rejection)

These are O(1) or O(n) with sampling and catch gross corruption:

1. **File size bounds**: the actual log file must be at least as large as `meta.log_file_size`. A truncated file is rejected immediately.
2. **Column size consistency**: the offsets, lengths, flags, and time column files must contain at least `meta.entry_count` entries. An inflated entry count is caught here.
3. **First offset validity**: the first offset must be byte 0, or the byte before it must be a newline (supporting appended captures that start mid-file).
4. **Monotonic offsets**: all offsets must be strictly increasing and within file bounds.
5. **Newline boundary sampling**: for indexes under 100k lines, every offset is checked to ensure the preceding byte is a newline. For larger indexes, 1-in-1000 offsets are sampled. This catches wrong-base-offset bugs where offsets point into the middle of lines.

### Phase 2: Checkpoint walk (partial trust)

If checkpoints are present, the validator walks them from last to first:

1. Cross-checks the checkpoint's `byte_offset` against the offsets column.
2. Reads up to 256 bytes from the log file at the checkpoint's byte offset and computes an xxh3 hash.
3. Compares the hash against the checkpoint's `content_hash`. Two hash methods are tried to support both `IndexBuilder` (raw 256-byte read) and `LineIndexer` (content-only, excluding delimiter) built indexes.
4. The first checkpoint that validates establishes the trust boundary. All entries up to that checkpoint's `line_number` are trusted.

If all checkpoints fail verification, the index is rejected entirely.

If no checkpoints column exists (older indexes), structural checks alone determine trust --- the full entry count is trusted if structural checks pass.

### Return value and degraded mode

`validate_index()` returns `Option<ValidatedIndex>` containing `trusted_entries` and `trusted_file_size`. When validation fails (returns `None`):

- `IndexReader::open()` returns `None` --- the source operates without an index. Line viewing and text filtering still work. Severity stats, bitmap-accelerated filtering, and `@ts` queries are unavailable.
- The TUI detects the missing reader when an index directory exists and shows `"Index is corrupt --- restart capture to fix"` in the status bar (`src/app/tab.rs`).
- MCP `get_stats` returns `has_index: false` and falls back to the actual file size from disk metadata.

When validation succeeds with partial trust (fewer entries than `meta.entry_count`), `IndexReader::stats()` limits column reads to `trusted_entries`, providing accurate statistics for the validated portion.

### No automatic rebuild

The viewer does not attempt to rebuild or repair the index. The user must restart capture (`lazytail -n NAME`) to regenerate it. This keeps the viewer simple and avoids the viewer needing to embed the full indexing pipeline.

## Consequences

**Benefits:**
- Fast validation: structural checks are O(1) metadata reads; checkpoint walk reads only a few 256-byte samples from the log file. Even multi-gigabyte files validate in milliseconds.
- Partial trust: if the last checkpoint is corrupt but earlier ones are valid, the index is still usable for the validated portion, avoiding full rejection of a mostly-good index.
- Graceful degradation: a corrupt index never causes panics or wrong results --- the source simply operates without index features.
- Detects subtle corruption: content hashing catches file replacement even when the new file has the same size as the original.
- Catches wrong-base-offset bugs: newline boundary sampling rejects indexes where offsets point into the middle of lines.

**Trade-offs:**
- Sampled validation for large indexes (over 100k lines) means some corruption patterns between sample points could go undetected. The checkpoint content hash provides a second layer of defense.
- No automatic repair means users must manually restart capture to fix a corrupt index.
- Two hash methods in `verify_checkpoint_hash` add complexity to support both `IndexBuilder` and `LineIndexer` checkpoint formats.
- Partial trust only works at checkpoint granularity (default every 100 lines) --- corruption between checkpoints may include some invalid entries in the trusted range.
