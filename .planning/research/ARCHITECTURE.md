# Architecture Patterns: Project-Scoped Configuration

**Domain:** CLI tool configuration integration
**Researched:** 2026-02-03
**Confidence:** HIGH (based on existing codebase analysis + Rust ecosystem patterns)

## Executive Summary

This document outlines how project-scoped configuration should integrate with LazyTail's existing event-driven architecture. The key insight is that configuration loading must happen **before** the main event loop but **after** CLI argument parsing, creating a clear initialization phase that feeds into the existing flow.

## Current Architecture (Baseline)

```
main()
  |
  +-- Args::parse() via clap
  |
  +-- Mode detection (MCP / Capture / Discovery / Normal)
  |
  +-- TabState creation (reads files, sets up watchers)
  |
  +-- App::with_tabs() construction
  |
  +-- Terminal setup (raw mode, alternate screen)
  |
  +-- run_app() event loop
        |
        +-- Render phase
        +-- Event collection (file, filter, input)
        +-- Event processing via App::apply_event()
```

**Key observation:** Configuration currently only exists in two places:
1. **CLI arguments** (parsed by clap in `main()`)
2. **Global state** (`~/.config/lazytail/` for history, data, sources)

There is no project-local configuration layer.

## Recommended Architecture

### Component Boundaries

| Component | Responsibility | Communicates With |
|-----------|---------------|-------------------|
| `Config` (new) | Load, merge, and provide configuration values | `main()` startup, `App` construction |
| `ConfigLoader` (new) | Find and parse config files (project + global) | Filesystem, `Config` |
| `Args` (existing) | Parse CLI arguments | `main()`, `Config` (for overrides) |
| `App` (existing) | Runtime state, event handling | Uses `Config` values during construction |
| `TabState` (existing) | Per-tab state | May read project-local streams from `.lazytail/` |

### Data Flow

```
Startup Flow (NEW)
==================

1. Parse CLI args (clap)
       |
       v
2. Discover project root (walk up from cwd looking for lazytail.yaml)
       |
       v
3. Load configuration layers:
       - defaults (hardcoded)
       - global config (~/.config/lazytail/config.yaml)
       - project config (./lazytail.yaml or .lazytail/config.yaml)
       - environment variables (LAZYTAIL_*)
       - CLI arguments (highest priority)
       |
       v
4. Resolve paths (.lazytail/ directory location)
       |
       v
5. Mode detection with config awareness
       |
       v
6. TabState creation (using config values)
       |
       v
7. App::with_tabs() with Config reference
       |
       v
8. Event loop (unchanged)
```

### Precedence Order (Low to High)

| Priority | Source | Example |
|----------|--------|---------|
| 1 (lowest) | Hardcoded defaults | `follow_mode: true` |
| 2 | Global config file | `~/.config/lazytail/config.yaml` |
| 3 | Project config file | `./lazytail.yaml` |
| 4 | Environment variables | `LAZYTAIL_FOLLOW_MODE=false` |
| 5 (highest) | CLI arguments | `--no-follow` |

**Rationale:** This follows the [12-factor app](https://12factor.net/config) convention and matches patterns from tools like ripgrep, bat, and cargo.

## Patterns to Follow

### Pattern 1: Config Discovery via Parent Walk

**What:** Start from current working directory, walk up parent directories until finding `lazytail.yaml` or filesystem root.

**When:** At startup, before tab creation.

**Example:**
```rust
fn find_project_root() -> Option<PathBuf> {
    let mut current = std::env::current_dir().ok()?;
    loop {
        // Check for project marker files
        if current.join("lazytail.yaml").exists() {
            return Some(current);
        }
        if current.join(".lazytail").is_dir() {
            return Some(current);
        }
        // Walk up
        if !current.pop() {
            return None; // Reached filesystem root
        }
    }
}
```

**Why this pattern:**
- Matches behavior of `.gitignore`, `.eslintrc`, `Cargo.toml`
- User can run `lazytail` from any subdirectory
- Clear "project boundary" semantics

### Pattern 2: Layered Config with serde

**What:** Define a config struct with `Option<T>` fields, deserialize from multiple sources, merge with explicit precedence.

**When:** During config loading phase.

**Example:**
```rust
#[derive(Debug, Default, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Default follow mode for new tabs
    pub follow_mode: Option<bool>,

    /// Default filter mode
    pub filter_mode: Option<FilterModeConfig>,

    /// Project-local streams directory
    pub streams_dir: Option<PathBuf>,

    /// Sources to auto-open
    pub sources: Option<Vec<SourceConfig>>,
}

impl Config {
    /// Merge another config, taking values from `other` where present
    pub fn merge(self, other: Config) -> Config {
        Config {
            follow_mode: other.follow_mode.or(self.follow_mode),
            filter_mode: other.filter_mode.or(self.filter_mode),
            streams_dir: other.streams_dir.or(self.streams_dir),
            sources: other.sources.or(self.sources),
        }
    }
}
```

**Why this pattern:**
- Clear precedence without complex logic
- Type-safe configuration
- `Option<T>` distinguishes "not set" from "explicitly set to default"

### Pattern 3: CLI Args Override Config

**What:** Parse CLI args, then use them to override config values.

**When:** After config loading, before app construction.

**Example:**
```rust
// In main()
let args = Args::parse();
let config = ConfigLoader::load()?;

// CLI args override config
let follow_mode = if args.no_follow {
    false
} else {
    config.follow_mode.unwrap_or(true)
};
```

**Why this pattern:**
- Explicit, predictable behavior
- User can always override project config via CLI
- Matches Unix convention (flags override config files)

### Pattern 4: Lazy Project Directory Creation

**What:** Don't create `.lazytail/` directory until actually needed (first capture to project).

**When:** During capture mode when `-n` flag used with project context.

**Example:**
```rust
fn ensure_project_streams_dir(project_root: &Path) -> Result<PathBuf> {
    let streams_dir = project_root.join(".lazytail").join("streams");
    if !streams_dir.exists() {
        fs::create_dir_all(&streams_dir)?;
    }
    Ok(streams_dir)
}
```

**Why this pattern:**
- Avoid polluting projects that don't use lazytail features
- Create on first write, not on first read
- Matches `.git/` behavior

## Anti-Patterns to Avoid

### Anti-Pattern 1: Config in App State

**What:** Storing raw Config struct in App and reading it during event handling.

**Why bad:**
- Config values rarely change at runtime
- Adds unnecessary complexity to hot path
- Makes testing harder

**Instead:** Resolve all config values during startup, pass resolved values to constructors.

### Anti-Pattern 2: Implicit Config Path Changes

**What:** Changing effective project root based on which file is opened.

**Why bad:**
- Confusing behavior when opening files from different directories
- Config becomes non-deterministic
- User can't predict which `.lazytail/` applies

**Instead:** Lock project root at startup based on cwd, never change it.

### Anti-Pattern 3: Config Hot Reload

**What:** Watching config files for changes and applying them at runtime.

**Why bad (for this project):**
- LazyTail is not long-running daemon
- Adds complexity to event loop
- Config changes mid-session could cause confusion

**Instead:** Require restart to apply config changes. Clear and predictable.

### Anti-Pattern 4: Environment-Dependent Defaults

**What:** Different default values based on OS or environment.

**Why bad:**
- Surprising cross-platform behavior
- Harder to document and test
- User confusion when sharing configs

**Instead:** Same defaults everywhere, document platform-specific recommendations.

## Directory Structure Recommendation

```
project/
  lazytail.yaml           # Project config (optional)
  .lazytail/              # Project-local data (created on demand)
    streams/              # Project-local captured streams
      api.log
      worker.log
    sources/              # PID markers for active streams
      api
      worker

~/.config/lazytail/       # Global config (existing)
  config.yaml             # Global config file (new)
  history.json            # Filter history (existing)
  data/                   # Global captured streams (existing)
    api.log
  sources/                # Global PID markers (existing)
    api
```

### Why Two Levels?

| Level | Use Case | Example |
|-------|----------|---------|
| Global (`~/.config/lazytail/`) | Shared across all projects, persistent | Long-running services, personal preferences |
| Project (`.lazytail/`) | Project-specific, ephemeral | Development servers, test runs |

### Config File: Root vs Directory

**Recommendation:** Support both patterns:
1. `lazytail.yaml` in project root (simple, visible)
2. `.lazytail/config.yaml` (hidden, grouped with data)

Check in order: `lazytail.yaml` first, then `.lazytail/config.yaml`. First found wins.

**Rationale:**
- `lazytail.yaml` is easier to discover, good for simple configs
- `.lazytail/config.yaml` keeps project root clean, good for complex setups
- Matches patterns from other tools (e.g., `.prettierrc` vs `.prettier/config`)

## Integration Points with Existing Code

### main.rs Changes

```
BEFORE:
  Args::parse() -> mode detection -> TabState::new() -> App::with_tabs()

AFTER:
  Args::parse() -> Config::load() -> mode detection -> TabState::new(config) -> App::with_tabs()
```

Key change: `Config::load()` inserted between arg parsing and mode detection.

### source.rs Changes

New functions needed:
- `project_data_dir(project_root: &Path)` -> `.lazytail/streams/`
- `project_sources_dir(project_root: &Path)` -> `.lazytail/sources/`
- `discover_project_sources(project_root: &Path)` -> project-local streams

Existing functions unchanged (they handle global scope).

### tab.rs Changes

`TabState::from_discovered_source()` needs to know whether source is global or project-local (for correct cleanup behavior).

### capture.rs Changes

`run_capture_mode()` needs config to determine:
- Write to project `.lazytail/streams/` or global `~/.config/lazytail/data/`
- Create project marker or global marker

### Signal Handling Integration

Signal handling (SIGINT, SIGTERM) needs to:
1. Cancel any active filter operations (existing: `CancelToken`)
2. Clean up PID markers (existing: `remove_marker()`)
3. **NEW:** Distinguish project vs global markers

```rust
// In signal handler or cleanup
fn cleanup_markers(config: &Config) {
    // Clean up any markers we created
    for marker in &active_markers {
        if marker.is_project_local {
            source::remove_project_marker(&config.project_root, &marker.name);
        } else {
            source::remove_marker(&marker.name);
        }
    }
}
```

## Build Order (Dependencies)

Suggested implementation order based on dependencies:

```
Phase 1: Config Foundation
  1. Config struct definition
  2. ConfigLoader (file reading, YAML parsing)
  3. Config precedence merging
  4. Project root discovery

Phase 2: CLI Integration
  5. Args -> Config override logic
  6. main.rs startup flow changes
  7. Environment variable support

Phase 3: Project-Local Streams
  8. source.rs project-local functions
  9. capture.rs project awareness
  10. Discovery mode project sources

Phase 4: Cleanup
  11. Signal handler updates
  12. Tab close cleanup (project vs global)
```

**Why this order:**
- Phase 1 is foundational, no runtime changes
- Phase 2 integrates with existing flow, testable incrementally
- Phase 3 adds new functionality on stable base
- Phase 4 handles edge cases after main features work

## Config File Schema (Recommended)

```yaml
# lazytail.yaml

# Display settings
follow_mode: true           # Auto-scroll to new logs
filter_mode: plain          # plain | regex
case_sensitive: false       # Filter case sensitivity

# Project streams
streams_dir: .lazytail/streams   # Where to store captured streams

# Auto-open sources (optional)
sources:
  - name: api
    path: .lazytail/streams/api.log
  - name: app
    path: ./logs/app.log

# UI settings
side_panel_width: 32
```

## Testing Strategy

### Unit Tests

- `Config::merge()` precedence
- `find_project_root()` directory walking
- Config deserialization from YAML

### Integration Tests

- Config loading with real files
- CLI override behavior
- Project vs global source discovery

### Manual Testing

- Run from project subdirectory
- Multiple config layers
- Missing config file handling

## Sources

- [Rust CLI Book: Config Files](https://rust-cli.github.io/book/in-depth/config-files.html)
- [Cargo Configuration Hierarchy](https://doc.rust-lang.org/cargo/reference/config.html)
- [Rain's Rust CLI Recommendations](https://rust-cli-recommendations.sunshowers.io/configuration.html)
- [config-rs crate](https://github.com/rust-cli/config-rs)
- [Hierarchical Configuration in Rust](https://steezeburger.com/2023/03/rust-hierarchical-configuration/)
- [signal-hook crate](https://docs.rs/signal-hook)
- [Tokio Graceful Shutdown](https://tokio.rs/tokio/topics/shutdown)
- [bat config patterns](https://github.com/sharkdp/bat)
- [ripgrep config discussion](https://github.com/BurntSushi/ripgrep/issues/196)
