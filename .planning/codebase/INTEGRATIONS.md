# External Integrations

**Analysis Date:** 2026-02-03

## APIs & External Services

**Model Context Protocol (MCP) Server:**
- Service: MCP 1.0 compatible server for AI assistant integration
- What it's used for: Exposing log analysis tools to Claude, Codex, Gemini, and other MCP-compatible AI assistants
  - Search logs by pattern
  - Analyze log file metadata
  - Extract structured data from unstructured logs
  - SDK/Client: `rmcp` 0.1 (Rust MCP library)
  - Transport: stdio-based (piped input/output to AI client)
  - Activation: `lazytail --mcp` CLI flag
  - Implementation: `src/mcp/mod.rs`, `src/mcp/tools.rs`, `src/mcp/types.rs`

## Data Storage

**File System Only:**
- Primary input: Local log files on disk
- Streaming: stdin for piped input (no file persistence for stdin)
- Access method: Memory-mapped file I/O via `memmap2` for O(1) random line access
- File watching: File system events via `notify` (inotify on Linux, FSEvents on macOS)

**Session State Storage:**
- Location: `~/.config/lazytail/` (platform-aware via `dirs` crate)
- Contents: Filter history, application state
- Format: JSON (via `serde_json`)
- Implementation: `src/history.rs`, `src/source.rs`

**No Databases:**
- Not used - all log data processed in-memory or via memory mapping
- No persistent database integration

**No Cloud Storage:**
- Not used - purely local file processing

**Caching:**
- Internal LRU cache for expansion state and viewport data
- Cache: `lru` crate 0.12
- Files affected: `src/app.rs`, `src/tab.rs`

## Authentication & Identity

**None:**
- No user authentication required
- No API keys needed for core functionality
- MCP server: Uses stdio transport (authentication delegated to parent AI client application)

## Monitoring & Observability

**Error Tracking:**
- Not integrated - errors propagated via `anyhow::Result`
- Logging: stderr only (eprintln! macros in MCP server startup)
- Implementation: `src/mcp/mod.rs` logs server startup messages to stderr

**Logs:**
- Terminal UI: Status bar messages for user feedback
- Debug: No logging framework integrated; could use `log` crate for future enhancements
- MCP Server: Basic stderr output for server lifecycle events

## CI/CD & Deployment

**Hosting:**
- Self-hosted binaries (GitHub Releases)
- Arch Linux AUR (community-maintained)
- Installation script: `install.sh` (downloads from GitHub Releases)

**CI Pipeline:**
- GitHub Actions (`.github/workflows/ci.yml`)
- Test platforms: Ubuntu, macOS
- Release workflow: `.github/workflows/release.yml` (GitHub Releases)
- PR workflow: `.github/workflows/release-pr.yml`

**Distribution Channels:**
- GitHub Releases (binary artifacts)
- Cargo crates.io (source package)
- Arch Linux AUR (PKGBUILD)

## Environment Configuration

**Required env vars:**
- None for basic operation
- Optional for development:
  - `CARGO_TERM_COLOR=always` (set in CI workflows)

**CLI Flags (instead of env vars):**
```
--mcp                    # Run as MCP server (optional feature)
--log-file <PATH>        # Specify log file to open
--source <NAME>          # Specify source to open (from ~/.config/lazytail/)
```

**Secrets location:**
- No secrets or API keys used in application
- MCP transport: Secrets managed by parent AI client (not by LazyTail)

## Webhooks & Callbacks

**Incoming:**
- None - no HTTP server or webhook listener

**Outgoing:**
- None - no external service calls

## File System Integration

**Input Paths:**
- `<file>` - Any readable file path (log files)
- stdin - Piped input from other programs
- Discovered sources: `~/.local/share/lazytail/sources/` (UNIX) / `%APPDATA%\lazytail\sources\` (Windows)

**Output:**
- Terminal rendering only (no file output)
- History stored to: `~/.config/lazytail/history.json`

**File Watching Implementation:**
- Linux: inotify via `notify` crate
- macOS: FSEvents via `notify` crate
- Watch targets: Single file or directory
- Watch events: Modify, Create, Remove, Rename
- Implementation: `src/watcher.rs`, `src/dir_watcher.rs`

## Third-Party Service Dependencies

**Platform Libraries:**
- libc (non-Linux): C library bindings for Darwin/BSD syscalls
- Windows: Windows API bindings (via crossterm transitive dependency)

**No Third-Party Integrations:**
- Slack, Discord, monitoring tools: Not integrated
- Cloud log aggregation (CloudWatch, Datadog, etc.): Not integrated
- Email notifications: Not integrated
- Remote log sources: Not supported

---

*Integration audit: 2026-02-03*
