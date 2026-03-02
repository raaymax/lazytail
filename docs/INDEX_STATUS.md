# Index System Status

## Completed
- [x] Flag bitmask constants + Severity enum (flags.rs)
- [x] detect_flags() line heuristics (flags.rs)
- [x] ColumnWriter<T> / ColumnReader<T> (column.rs)
- [x] IndexMeta 64-byte header (meta.rs)
- [x] Checkpoint 64-byte entries (checkpoint.rs)
- [x] Optimized detect_flags_bytes() (flags.rs)
- [x] IndexBuilder bulk build (builder.rs)
- [x] LineIndexer capture-time indexing (builder.rs)

## Remaining Integration Tasks
- [x] Wire LineIndexer into capture.rs (capture mode -n flag)
- [x] Wire IndexBuilder into source discovery (build index for existing files)
- [ ] Add index-aware LogReader (read lines via offset column instead of sparse index)
- [x] TUI: severity-based line coloring from flags column
- [x] TUI: severity histogram from checkpoint counts
- [x] MCP: expose index stats via tools
- [ ] Config: format hints in lazytail.yaml (json/logfmt/plain)
- [ ] Auto-detection: sample first 20 lines, infer format
- [ ] Incremental rebuild: detect log file truncation/rotation
- [ ] Template detection (bits 16-31) — future phase
