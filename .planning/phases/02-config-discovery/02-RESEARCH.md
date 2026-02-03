# Phase 2: Config Discovery - Research

**Researched:** 2026-02-03
**Domain:** Filesystem path traversal, config file discovery
**Confidence:** HIGH

## Summary

Config discovery for lazytail requires walking parent directories from cwd to find `lazytail.yaml`, plus checking the global config at `~/.config/lazytail/config.yaml`. Rust's standard library provides `Path::ancestors()` for efficient upward traversal, and the existing `dirs` crate (v5.0) provides cross-platform config directory resolution.

The implementation is straightforward pure-Rust with no additional dependencies needed. The pattern follows Cargo's hierarchical config discovery: closest file wins for project config, global config serves as fallback layer. Key considerations are proper path canonicalization before traversal and graceful handling of permission errors.

**Primary recommendation:** Use `std::path::Path::ancestors()` for upward traversal combined with `dirs::config_dir()` for global config location. Return a simple struct containing discovered paths, letting Phase 3 handle actual loading and merging.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::path | stdlib | Path manipulation, ancestors() traversal | Zero-dep, stable since Rust 1.28 |
| std::fs | stdlib | File existence checks, canonicalize() | Zero-dep, handles cross-platform |
| dirs | 5.0 | XDG config directory resolution | Already in Cargo.toml, cross-platform |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| std::env | stdlib | Get current working directory | `std::env::current_dir()` as starting point |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| Manual ancestors | walkdir | Overkill - walkdir walks DOWN into dirs, not up to parents |
| Manual discovery | find_config crate | Unnecessary dep - std::path::ancestors() is trivial |
| dirs 5.0 | dirs 6.0 | No compelling reason to upgrade, 5.0 works fine |

**Installation:**
```bash
# No new dependencies needed - dirs already in Cargo.toml
```

## Architecture Patterns

### Recommended Module Structure
```
src/
├── config/
│   ├── mod.rs           # Re-exports, module root
│   └── discovery.rs     # This phase: find config paths
```

### Pattern 1: Discovery Result Struct
**What:** Return a struct with all discovered paths, not just "the config"
**When to use:** Always - caller needs to know what was found for merging logic
**Example:**
```rust
// Source: Cargo's hierarchical config pattern
/// Results of config file discovery
#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    /// Project root directory (where lazytail.yaml was found), if any
    pub project_root: Option<PathBuf>,
    /// Path to project config file (lazytail.yaml), if found
    pub project_config: Option<PathBuf>,
    /// Path to global config file (~/.config/lazytail/config.yaml), if exists
    pub global_config: Option<PathBuf>,
    /// Path where .lazytail/ data directory should be created
    /// Same as project_root when project_config found, None otherwise
    pub data_dir_location: Option<PathBuf>,
}
```

### Pattern 2: Upward Traversal with ancestors()
**What:** Use Path::ancestors() iterator to walk from cwd to root
**When to use:** Finding closest config file upward
**Example:**
```rust
// Source: https://doc.rust-lang.org/std/path/struct.Path.html#method.ancestors
use std::path::Path;

fn find_project_config(start: &Path) -> Option<PathBuf> {
    for ancestor in start.ancestors() {
        let candidate = ancestor.join("lazytail.yaml");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
```

### Pattern 3: Canonicalize First
**What:** Resolve symlinks before traversal
**When to use:** Per user decision - canonicalize, then walk real directories
**Example:**
```rust
// Source: https://doc.rust-lang.org/std/fs/fn.canonicalize.html
use std::fs;
use std::path::PathBuf;

fn discover_from_cwd() -> DiscoveryResult {
    let start = match std::env::current_dir() {
        Ok(cwd) => cwd.canonicalize().unwrap_or(cwd),
        Err(_) => return DiscoveryResult::default(),
    };
    // Walk ancestors of canonical path
    find_configs(&start)
}
```

### Anti-Patterns to Avoid
- **Recursive directory listing:** Don't use walkdir/read_dir - we're going UP not DOWN
- **Hardcoded paths:** Don't assume `/home` or specific paths - use dirs crate
- **Stopping at $HOME:** User decision says traverse to `/`, not stop at home
- **Deep merging in discovery:** Discovery finds paths, Phase 3 does loading/merging

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| XDG config dir | `~/.config` hardcoded | `dirs::config_dir()` | macOS returns `~/Library/Application Support`, Windows different |
| Home directory | `$HOME` env var | `dirs::home_dir()` | Handles Windows `%USERPROFILE%` and edge cases |
| Path parent iteration | Manual `.parent()` loop | `Path::ancestors()` | Built-in, handles edge cases at root |
| Symlink resolution | Manual readlink | `Path::canonicalize()` | Handles chains, cross-platform |

**Key insight:** Rust stdlib covers directory traversal completely. The `dirs` crate handles the one cross-platform edge case (config directory location).

## Common Pitfalls

### Pitfall 1: Canonicalize on Non-Existent Path
**What goes wrong:** `fs::canonicalize()` errors if path doesn't exist
**Why it happens:** Trying to canonicalize before checking existence
**How to avoid:** Only canonicalize the starting directory (cwd), not the config file paths being searched
**Warning signs:** `ErrorKind::NotFound` from canonicalize

### Pitfall 2: Permission Denied Mid-Traversal
**What goes wrong:** User lacks permission to access a parent directory
**Why it happens:** Unusual directory permissions, mounted filesystems
**How to avoid:** Catch `is_file()` errors gracefully, continue to next ancestor
**Warning signs:** Tests passing locally but failing in CI containers
**Example handling:**
```rust
// Don't just check is_file() - it can panic on permission denied
fn safe_is_file(path: &Path) -> bool {
    path.try_exists().unwrap_or(false) && path.is_file()
}
```

### Pitfall 3: Symlink Loops
**What goes wrong:** Symlink chain creates infinite loop
**Why it happens:** Malformed filesystem configuration
**How to avoid:** `canonicalize()` handles this - returns error on loop
**Warning signs:** Hang during discovery (without canonicalize protection)

### Pitfall 4: Empty Path from ancestors()
**What goes wrong:** ancestors() yields empty path `""` for relative paths
**Why it happens:** Relative path like `../foo` ends with `..` then `""`
**How to avoid:** Always start from absolute path (canonicalized cwd)
**Warning signs:** Attempting to join filename to empty path

### Pitfall 5: Case Sensitivity on macOS
**What goes wrong:** Looking for `lazytail.yaml` but file is `LazyTail.yaml`
**Why it happens:** macOS FS is case-insensitive by default
**How to avoid:** User decision specifies exact filename - document it clearly
**Warning signs:** Discovery fails on macOS when file has different casing

## Code Examples

Verified patterns from official sources:

### Complete Discovery Function
```rust
// Source: Cargo config pattern + Rust stdlib docs
use std::env;
use std::path::{Path, PathBuf};

const PROJECT_CONFIG_NAME: &str = "lazytail.yaml";
const GLOBAL_CONFIG_NAME: &str = "config.yaml";
const DATA_DIR_NAME: &str = ".lazytail";

#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    pub project_root: Option<PathBuf>,
    pub project_config: Option<PathBuf>,
    pub global_config: Option<PathBuf>,
}

impl DiscoveryResult {
    /// Path where .lazytail/ data directory should be created
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.project_root.as_ref().map(|r| r.join(DATA_DIR_NAME))
    }

    /// True if any config was found
    pub fn has_config(&self) -> bool {
        self.project_config.is_some() || self.global_config.is_some()
    }
}

/// Discover config file locations.
///
/// Walks from cwd upward looking for lazytail.yaml (closest wins).
/// Also checks for global config at ~/.config/lazytail/config.yaml.
pub fn discover() -> DiscoveryResult {
    let mut result = DiscoveryResult::default();

    // Check global config first (always exists at fixed location if present)
    if let Some(config_dir) = dirs::config_dir() {
        let global_path = config_dir.join("lazytail").join(GLOBAL_CONFIG_NAME);
        if global_path.is_file() {
            result.global_config = Some(global_path);
        }
    }

    // Get starting point - canonicalize to resolve symlinks
    let start = match env::current_dir() {
        Ok(cwd) => cwd.canonicalize().unwrap_or(cwd),
        Err(_) => return result,
    };

    // Walk ancestors looking for project config
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(PROJECT_CONFIG_NAME);
        // Use try_exists for graceful permission handling
        if candidate.try_exists().unwrap_or(false) && candidate.is_file() {
            result.project_root = Some(ancestor.to_path_buf());
            result.project_config = Some(candidate);
            break; // Closest wins
        }
    }

    result
}
```

### Verbose Mode Output
```rust
// For -v flag: show what was searched
pub fn discover_verbose() -> (DiscoveryResult, Vec<PathBuf>) {
    let mut searched = Vec::new();
    let mut result = DiscoveryResult::default();

    // ... same logic but push each candidate to searched vec ...

    (result, searched)
}
```

### Testing with Temp Directories
```rust
// Source: existing LazyTail test pattern in source.rs
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    use std::fs;

    #[test]
    fn test_finds_config_in_current_dir() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");
        fs::write(&config_path, "# test config").unwrap();

        // Change to temp dir for test
        let old_cwd = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        let result = discover();

        env::set_current_dir(old_cwd).unwrap();

        assert_eq!(result.project_root, Some(temp.path().to_path_buf()));
        assert_eq!(result.project_config, Some(config_path));
    }

    #[test]
    fn test_finds_config_in_parent_dir() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();

        let config_path = temp.path().join("lazytail.yaml");
        fs::write(&config_path, "# test config").unwrap();

        let old_cwd = env::current_dir().unwrap();
        env::set_current_dir(&subdir).unwrap();

        let result = discover();

        env::set_current_dir(old_cwd).unwrap();

        assert_eq!(result.project_root, Some(temp.path().to_path_buf()));
    }

    #[test]
    fn test_no_config_returns_defaults() {
        let temp = TempDir::new().unwrap();

        let old_cwd = env::current_dir().unwrap();
        env::set_current_dir(temp.path()).unwrap();

        let result = discover();

        env::set_current_dir(old_cwd).unwrap();

        assert!(result.project_config.is_none());
        assert!(result.project_root.is_none());
        // Note: global_config may or may not exist depending on test env
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual parent() loop | Path::ancestors() | Rust 1.28 (2018) | Cleaner iteration, handles edge cases |
| $HOME env var | dirs crate | ~2018 | Cross-platform, handles Windows/macOS |
| Sync-only file ops | try_exists() (returns Result) | Rust 1.63 (2022) | Better error handling for permissions |

**Deprecated/outdated:**
- `std::env::home_dir()` - Deprecated since Rust 1.29, use `dirs::home_dir()` instead
- Manual `$XDG_CONFIG_HOME` parsing - Use dirs crate which handles this

## Open Questions

Things that couldn't be fully resolved:

1. **Performance of canonicalize() on network filesystems**
   - What we know: canonicalize() can be slow on NFS/network mounts
   - What's unclear: Is this a real concern for config discovery?
   - Recommendation: Proceed with canonicalize(). If performance issues arise, add optional flag to skip it.

2. **Caching across multiple calls**
   - What we know: Discovery is lightweight, mostly stat() calls
   - What's unclear: Will discovery be called repeatedly (e.g., in a loop)?
   - Recommendation: Implement without caching first. If profiling shows it's hot, add OnceCell/LazyLock caching.

## Sources

### Primary (HIGH confidence)
- [Rust std::path::Path documentation](https://doc.rust-lang.org/std/path/struct.Path.html) - ancestors(), canonicalize(), starts_with() methods
- [Rust fs::canonicalize documentation](https://doc.rust-lang.org/std/fs/fn.canonicalize.html) - Symlink resolution behavior
- [dirs crate docs (v5.0.1)](https://docs.rs/dirs/5.0.1/dirs/) - config_dir(), home_dir() return types

### Secondary (MEDIUM confidence)
- [Cargo Configuration Reference](https://doc.rust-lang.org/cargo/reference/config.html) - Hierarchical config discovery pattern
- [Rain's Rust CLI Recommendations](https://rust-cli-recommendations.sunshowers.io/configuration.html) - Directory-scoped config patterns

### Tertiary (LOW confidence)
- None - all findings verified with official documentation

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All stdlib/dirs crate, well-documented
- Architecture: HIGH - Follows established Cargo pattern
- Pitfalls: HIGH - Based on official docs for error conditions

**Research date:** 2026-02-03
**Valid until:** 90 days (stdlib APIs are stable)
