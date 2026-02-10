# Technology Stack

**Analysis Date:** 2026-02-03

## Languages

**Primary:**
- Rust 2021 Edition - Terminal-based log viewer application

## Runtime

**Environment:**
- Rust stable toolchain (1.70+)
- Linux (primary), macOS (supported)

**Package Manager:**
- Cargo 1.x
- Lockfile: `Cargo.lock` (present)

## Frameworks & Core Libraries

**TUI Rendering:**
- `ratatui` 0.29 - Terminal UI framework with widget system (box layouts, text rendering, input handling)
- `crossterm` 0.28 - Cross-platform terminal I/O and event handling (keyboard, mouse, raw mode)

**File Watching & Streaming:**
- `notify` 7.0 - File system event notifications (inotify on Linux, FSEvents on macOS)

**Text Processing:**
- `regex` 1.11 - Pattern matching for log filtering
- `unicode-width` 0.2 - Unicode character width calculation for TUI rendering
- `ansi-to-tui` 7.0 - ANSI color code parsing and conversion to ratatui text styles

**Data & Configuration:**
- `serde` 1.0 with derive feature - Serialization/deserialization framework
- `serde_json` 1.0 - JSON parsing and generation (for history, config, MCP responses)
- `clap` 4.5 with derive feature - CLI argument parsing and help generation

**Concurrency & Performance:**
- `rayon` 1.10 - Data parallelization for parallel filtering operations
- `signal-hook` 0.3 - POSIX signal handling (graceful shutdown)
- `crossbeam-*` (transitive) - Lock-free concurrency utilities via rayon/notify

**Memory & Optimization:**
- `memmap2` 0.9 - Memory-mapped file access for O(1) random line access
- `memchr` 2.x - SIMD-accelerated byte search in log lines
- `lru` 0.12 - LRU cache for line expansion state and viewport rendering

**Error Handling & Utilities:**
- `anyhow` 1.0 - Ergonomic error handling with context chaining
- `dirs` 5.0 - Platform-aware config directory resolution

**MCP (Model Context Protocol) Server - Optional Feature:**
- `tokio` 1.x with full features - Async runtime for MCP server
- `rmcp` 0.1 with server and transport-io features - MCP protocol implementation and stdio transport
- `schemars` 0.8 - JSON Schema generation for MCP tool definitions

**Development & Testing:**
- `tempfile` 3.0 - Temporary file/directory handling in tests

## Build & Compilation

**Configuration Files:**
- `Cargo.toml` - Package manifest with feature flags and dependencies
- `Cargo.lock` - Locked dependency versions for reproducible builds

**Build Features:**
```toml
default = ["mcp"]           # MCP server enabled by default
mcp = ["tokio", "rmcp", "schemars"]  # Feature flag for MCP functionality
```

**Platform-Specific Dependencies:**
- `libc` 0.2 (non-Linux only) - C library bindings for platform-specific operations

## Configuration

**Environment:**
- No env vars required for basic operation
- MCP server mode activated via `--mcp` CLI flag (not env var)

**Build Configuration:**
- Rust edition: 2021
- MSRV (Minimum Supported Rust Version): 1.70+

## Platform Requirements

**Development:**
- Rust toolchain (stable channel)
- `cargo` package manager
- Standard C development tools (for compiling C dependencies like libc)

**Runtime - Linux:**
- Linux kernel with inotify support (for file watching)
- Terminal with UTF-8 support
- No external dependencies; statically linked

**Runtime - macOS:**
- macOS 10.12+ (Darwin kernel with FSEvents)
- Terminal with UTF-8 support
- No external dependencies; statically linked

## CI/CD

**Testing Platform:**
- GitHub Actions workflow (`.github/workflows/ci.yml`)
- Tested on: Ubuntu Latest, macOS Latest
- Rust version: stable

**Build Targets:**
- x86_64-unknown-linux-gnu
- x86_64-apple-darwin
- aarch64-apple-darwin

**Build Outputs:**
- `target/<target>/release/lazytail` - Optimized binary (used for releases)

---

*Stack analysis: 2026-02-03*
