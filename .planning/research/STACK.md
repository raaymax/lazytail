# Technology Stack: Project-Scoped Configuration

**Project:** LazyTail
**Milestone:** Project-scoped configuration and signal handling
**Researched:** 2026-02-03
**Overall Confidence:** HIGH

## Executive Summary

This research covers adding project-level configuration, signal handling for cleanup, and project-local directories to LazyTail. The existing stack (Rust 2021, ratatui, crossterm, signal-hook 0.3, serde, dirs) is solid. Key additions are a YAML parser for config files and potentially the `directories` crate for more structured path handling.

**Key finding:** The Rust YAML ecosystem is in flux. `serde_yaml` and `serde_yml` are both archived. The recommended path is `serde-saphyr` (actively maintained, panic-free, performant) or consider TOML instead of YAML for simpler configs.

---

## Recommended Stack Additions

### Configuration File Parsing

| Library | Version | Purpose | Confidence |
|---------|---------|---------|------------|
| **serde-saphyr** | 0.0.17 | YAML parsing | HIGH |

**Why serde-saphyr:**
- Actively maintained (last release: Feb 1, 2026)
- ~35k downloads/month, growing adoption
- Panic-free parsing (critical for CLI tools handling user-provided config)
- Type-driven parsing solves "Norway problem" (no ambiguous type inference)
- Direct deserialization without intermediate tree (memory efficient)
- Built on saphyr-parser, pure Rust (no unsafe libyaml bindings)
- 834+ passing tests including full yaml-test-suite

**Why NOT the alternatives:**

| Alternative | Why Not |
|-------------|---------|
| `serde_yaml` | Archived March 2024, no longer maintained |
| `serde_yml` | Fork of serde_yaml, archived September 2025 |
| `config-rs` | Good for layered config but cannot write config back; overkill for single file |
| TOML | Valid alternative, but YAML better for nested structures and multi-line strings (log patterns) |

**Installation:**
```toml
[dependencies]
serde-saphyr = "0.0"
serde = { version = "1.0", features = ["derive"] }  # Already present
```

**Usage pattern:**
```rust
use serde::{Deserialize, Serialize};
use serde_saphyr;

#[derive(Debug, Deserialize, Serialize)]
struct LazyTailConfig {
    streams: Vec<StreamConfig>,
    #[serde(default)]
    default_filter: Option<String>,
}

fn load_config(path: &Path) -> Result<LazyTailConfig> {
    let content = std::fs::read_to_string(path)?;
    let config: LazyTailConfig = serde_saphyr::from_str(&content)?;
    Ok(config)
}
```

---

### Signal Handling

| Library | Version | Purpose | Confidence |
|---------|---------|---------|------------|
| **signal-hook** | 0.4.3 | Unix signal handling | HIGH |

**Status:** Already in Cargo.toml at 0.3.x. Recommend updating to 0.4.3.

**Why signal-hook (continue using):**
- Most widely supported signal handling library in Rust ecosystem
- Already used in capture.rs for SIGINT/SIGTERM handling
- Last release: Jan 24, 2026 (actively maintained)
- Provides iterator, flag, and low-level patterns
- Recommended by official Rust CLI book

**Current usage in LazyTail is correct:**
```rust
// capture.rs - existing pattern
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;

thread::spawn(move || {
    let mut signals = Signals::new([SIGINT, SIGTERM]).ok()?;
    if signals.forever().next().is_some() {
        // cleanup and exit
    }
});
```

**For main app cleanup (new):**
```rust
use signal_hook::flag;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

let term = Arc::new(AtomicBool::new(false));
flag::register(signal_hook::consts::SIGTERM, Arc::clone(&term))?;
flag::register(signal_hook::consts::SIGINT, Arc::clone(&term))?;

// In main loop:
if term.load(Ordering::Relaxed) {
    // Graceful shutdown: save state, cleanup
    break;
}
```

**Alternatives considered:**

| Alternative | Why Not |
|-------------|---------|
| `ctrlc` | Simpler but less flexible; signal-hook already in use |
| `tokio::signal` | Only if going async; LazyTail is sync |

---

### Directory Handling

| Library | Version | Purpose | Confidence |
|---------|---------|---------|------------|
| **dirs** | 5.0 | Platform config paths | HIGH |
| **directories** | 6.0.0 | Project-specific paths | MEDIUM |

**Current:** LazyTail uses `dirs` crate for `config_dir()`. This is sufficient.

**For project-local directories (.lazytail/):**

No additional crate needed. Project-local directories follow a simple pattern:

```rust
fn find_project_config() -> Option<PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let config = dir.join("lazytail.yaml");
        if config.exists() {
            return Some(config);
        }
        let dotdir = dir.join(".lazytail");
        if dotdir.is_dir() {
            return Some(dotdir.join("config.yaml"));
        }
        if !dir.pop() {
            return None;
        }
    }
}
```

**Recommended directory conventions:**

| Scope | Path | Purpose |
|-------|------|---------|
| Project root | `lazytail.yaml` | Simple single-file config |
| Project directory | `.lazytail/` | Config + local streams |
| User global | `~/.config/lazytail/` | Global defaults, history |

**Why NOT directories crate:**
- `dirs` already handles global paths
- Project-local paths don't need platform abstraction (always relative to cwd)
- Adding `directories` provides no benefit for this use case

---

## Version Update Recommendation

Update `signal-hook` from 0.3 to 0.4:

```toml
# Cargo.toml change
signal-hook = "0.4"  # was "0.3"
```

**Breaking changes in 0.4:** None significant for current usage pattern.

---

## Configuration File Format Decision

**Recommendation: YAML (lazytail.yaml)**

| Factor | YAML | TOML |
|--------|------|------|
| Nested structures | Cleaner | Verbose with [[]] |
| Multi-line strings | Native | Requires escaping |
| Rust ecosystem | Recovering (serde-saphyr) | Mature (toml crate) |
| User familiarity | High (K8s, Docker, etc.) | Medium |
| Type ambiguity | Solved by serde-saphyr | N/A |

**Example lazytail.yaml:**
```yaml
# Project streams to auto-load
streams:
  - name: api
    path: ./logs/api.log
    follow: true
  - name: db
    command: "docker logs -f postgres"

# Default filter patterns
default_filter: "ERROR|WARN"

# UI preferences
ui:
  scrolloff: 5
  single_expand: true
```

**Alternative if YAML concerns persist:** TOML is a valid fallback with mature ecosystem (`toml` crate). Consider if config structure remains simple.

---

## Summary: Dependencies to Add

```toml
[dependencies]
# NEW: YAML config parsing (panic-free, actively maintained)
serde-saphyr = "0.0"

# UPDATE: Latest signal-hook
signal-hook = "0.4"

# EXISTING (no changes needed)
serde = { version = "1.0", features = ["derive"] }
dirs = "5.0"
```

---

## Sources

### HIGH Confidence (Official Docs)
- [signal-hook docs.rs](https://docs.rs/signal-hook) - v0.4.3, patterns
- [signal-hook lib.rs](https://lib.rs/crates/signal-hook) - v0.4.3, last updated Jan 24, 2026
- [directories lib.rs](https://lib.rs/crates/directories) - v6.0.0, Jan 12, 2025
- [Rust CLI Book: Signals](https://rust-cli.github.io/book/in-depth/signals.html) - Official patterns

### MEDIUM Confidence (Verified with Multiple Sources)
- [serde-saphyr lib.rs](https://lib.rs/crates/serde-saphyr) - v0.0.17, Feb 1, 2026
- [serde-saphyr GitHub](https://github.com/bourumir-wyngs/serde-saphyr) - Features, panic-free claims
- [Rain's Rust CLI Recommendations](https://rust-cli-recommendations.sunshowers.io/configuration.html) - Directory conventions
- [config-rs lib.rs](https://lib.rs/crates/config) - v0.15.19, Nov 12, 2025

### LOW Confidence (WebSearch, Single Source)
- Community forum discussions on YAML ecosystem state
- Relative adoption numbers may shift

---

## Open Questions

1. **YAML vs TOML final decision** - Implementation should prototype both if stakeholder preference differs from recommendation
2. **Config write-back** - If config generation/modification needed, neither serde-saphyr nor config-rs support this well; may need yaml-edit or manual approaches
3. **Windows signal handling** - signal-hook works but Windows doesn't have true SIGTERM; test on Windows if cross-platform important
