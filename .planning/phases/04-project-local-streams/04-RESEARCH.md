# Phase 4: Project-Local Streams - Research

**Researched:** 2026-02-04
**Domain:** Filesystem directory management, context-aware data storage, source discovery
**Confidence:** HIGH

## Summary

This phase enables project-local stream storage by creating `.lazytail/` directories within project roots (identified by Phase 2/3's `lazytail.yaml` discovery). The capture mode (`lazytail -n <NAME>`) must become context-aware: when run inside a project, streams write to `.lazytail/data/` in the project root; otherwise they fall back to the global `~/.config/lazytail/data/`.

The implementation extends the existing `source.rs` module with project-aware variants of `data_dir()` and `sources_dir()` that consume the `DiscoveryResult` from config discovery. The key architectural insight is that discovery already runs early (before mode dispatch), so capture mode has access to project root information. Directory creation uses `std::fs::DirBuilder` with Unix-specific mode `0o700` for secure permissions.

**Primary recommendation:** Add `project_data_dir()` and `project_sources_dir()` functions to `source.rs` that accept `DiscoveryResult`, falling back to global dirs when no project root exists. Pass the discovery result through to capture mode.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| std::fs::DirBuilder | stdlib | Directory creation with permissions | Zero-dep, supports recursive + mode |
| std::os::unix::fs::DirBuilderExt | stdlib | Unix mode permissions (0o700) | Platform-specific secure creation |
| dirs | 5.0 | Fallback to global config directory | Already in project, cross-platform |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| anyhow | 1.0 | Error context in directory operations | Already used throughout source.rs |
| libc | * | (Indirect via stdlib) Unix permission checks | Already in project for PID checks |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| DirBuilder::mode() | chmod after create | Race condition window, extra syscall |
| Manual permission check | umask manipulation | Process-global state, not thread-safe |
| tempdir for tests | Real directory tests | Tempdir is already used, fits pattern |

**Installation:**
```bash
# No new dependencies needed - all stdlib + existing deps
```

## Architecture Patterns

### Recommended Module Structure
```
src/
├── source.rs           # MODIFY: Add project-aware directory functions
├── capture.rs          # MODIFY: Pass DiscoveryResult, use project dirs
├── config/
│   └── discovery.rs    # EXISTING: DiscoveryResult.data_dir() already exists
└── main.rs             # MODIFY: Pass discovery to capture mode
```

### Pattern 1: Context-Aware Directory Resolution
**What:** Functions that resolve to project-local or global directory based on discovery
**When to use:** All operations that need data/sources directories
**Example:**
```rust
// Source: Existing discovery.rs pattern + extension
use crate::config::DiscoveryResult;

/// Get data directory: project .lazytail/data/ or global ~/.config/lazytail/data/
pub fn resolve_data_dir(discovery: &DiscoveryResult) -> Option<PathBuf> {
    // Prefer project-local if available
    if let Some(project_root) = &discovery.project_root {
        Some(project_root.join(".lazytail").join("data"))
    } else {
        // Fall back to global
        dirs::config_dir().map(|p| p.join("lazytail").join("data"))
    }
}

/// Get sources directory: project .lazytail/sources/ or global
pub fn resolve_sources_dir(discovery: &DiscoveryResult) -> Option<PathBuf> {
    if let Some(project_root) = &discovery.project_root {
        Some(project_root.join(".lazytail").join("sources"))
    } else {
        dirs::config_dir().map(|p| p.join("lazytail").join("sources"))
    }
}
```

### Pattern 2: Secure Directory Creation with Mode 0o700
**What:** Create directories with restricted permissions using DirBuilder
**When to use:** Creating .lazytail/ directory and its subdirectories
**Example:**
```rust
// Source: https://doc.rust-lang.org/std/fs/struct.DirBuilder.html
use std::fs::DirBuilder;
#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;

/// Create directory with secure permissions (0o700).
/// On non-Unix systems, creates with default permissions.
pub fn create_secure_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    builder.mode(0o700);

    builder.create(path)
}
```

### Pattern 3: Discovery Pass-Through to Capture Mode
**What:** Pass DiscoveryResult from main.rs to capture mode
**When to use:** When capture mode needs to know project context
**Example:**
```rust
// In main.rs
if let Some(name) = args.name {
    // ... stdin check ...
    return capture::run_capture_mode(name, &discovery);
}

// In capture.rs
pub fn run_capture_mode(name: String, discovery: &DiscoveryResult) -> Result<()> {
    // Resolve data directory based on context
    let data = resolve_data_dir(discovery)
        .context("Could not determine data directory")?;

    // Create parent directories with secure permissions
    create_secure_dir(&data)?;

    let log_path = data.join(format!("{}.log", name));
    // ... rest of capture logic ...
}
```

### Pattern 4: Discovery Mode Shows Both Project and Global
**What:** Discovery mode scans both project .lazytail/data/ and global data/
**When to use:** When running `lazytail` with no args
**Example:**
```rust
// Modified discover_sources() to accept discovery
pub fn discover_sources(discovery: &DiscoveryResult) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();

    // Scan project-local first (if in project)
    if let Some(project_data) = discovery.data_dir() {
        if project_data.exists() {
            sources.extend(scan_data_dir(&project_data, SourceLocation::Project)?);
        }
    }

    // Then scan global
    if let Some(global_data) = global_data_dir() {
        if global_data.exists() {
            sources.extend(scan_data_dir(&global_data, SourceLocation::Global)?);
        }
    }

    Ok(sources)
}
```

### Anti-Patterns to Avoid
- **Creating .lazytail/ without lazytail.yaml:** Only create when project config exists (user decision [02-01])
- **Exposing raw global functions:** New code should use context-aware functions
- **Forgetting platform-specific code:** Mode 0o700 is Unix-only, use cfg(unix)
- **Hardcoding .lazytail:** Use constant from discovery module (DATA_DIR_NAME already exists)

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Directory permissions | chmod after mkdir | DirBuilder::mode() | Race-free, atomic |
| Config dir location | Hardcode ~/.config | dirs::config_dir() | Already used, cross-platform |
| Parent dir creation | Loop with mkdir | DirBuilder::recursive(true) | Handles all parents with same permissions |
| Project root detection | New traversal code | DiscoveryResult.project_root | Already implemented in Phase 2 |

**Key insight:** Phase 2's discovery already provides everything needed for context detection. This phase is about using that information, not re-implementing discovery.

## Common Pitfalls

### Pitfall 1: Mode 0o700 vs 0o755
**What goes wrong:** Using 0o755 allows group/other read access to potentially sensitive logs
**Why it happens:** 0o755 is common default, feels "safe enough"
**How to avoid:** Always use 0o700 for .lazytail/ - it contains user data
**Warning signs:** Other users on system can read captured logs

### Pitfall 2: Creating .lazytail/ Without lazytail.yaml
**What goes wrong:** .lazytail/ appears in projects that don't use lazytail
**Why it happens:** Creating dir unconditionally on capture, not checking for project
**How to avoid:** Per decision [02-01]: only lazytail.yaml signals project root. If no project config, use global.
**Warning signs:** Random .lazytail/ directories appearing in non-lazytail projects

### Pitfall 3: Windows Mode Ignored Silently
**What goes wrong:** DirBuilder::mode() is Unix-only, silently ignored on Windows
**Why it happens:** No Windows equivalent in stdlib
**How to avoid:** Accept limitation, document in code. Windows has different security model.
**Warning signs:** Directories have default permissions on Windows

### Pitfall 4: Marker/Source Directory Mismatch
**What goes wrong:** Creating marker in global sources/ but log in project data/
**Why it happens:** Functions use different directory resolution logic
**How to avoid:** Always resolve both from same DiscoveryResult
**Warning signs:** Source status shows "ended" when capture is still running

### Pitfall 5: Discovery Not Refreshed After Directory Creation
**What goes wrong:** Dir watcher doesn't see project-local sources
**Why it happens:** DirWatcher only watches one directory path
**How to avoid:** When in project context, watch project .lazytail/data/ instead of global
**Warning signs:** New captures in project don't appear as tabs

## Code Examples

Verified patterns from official sources:

### Secure Directory Creation
```rust
// Source: https://doc.rust-lang.org/std/fs/struct.DirBuilder.html
use std::fs::DirBuilder;
use std::path::Path;
use std::io::Result;

#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;

/// Create directory with mode 0o700 (owner rwx only).
pub fn create_secure_dir(path: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    builder.mode(0o700);

    builder.create(path)
}
```

### Context-Aware Ensure Directories
```rust
// Source: Custom extension of existing ensure_directories()
use crate::config::DiscoveryResult;
use anyhow::{Context, Result};

/// Ensure data and sources directories exist with proper permissions.
/// Uses project-local dirs if in project, otherwise global.
pub fn ensure_directories(discovery: &DiscoveryResult) -> Result<()> {
    let data = resolve_data_dir(discovery)
        .context("Could not determine data directory")?;
    let sources = resolve_sources_dir(discovery)
        .context("Could not determine sources directory")?;

    create_secure_dir(&data)
        .context("Failed to create data directory")?;
    create_secure_dir(&sources)
        .context("Failed to create sources directory")?;

    Ok(())
}
```

### Capture Mode Header Message
```rust
// Show user where data is going
let location = if discovery.project_root.is_some() {
    "project"
} else {
    "global"
};
eprintln!(
    "Serving \"{}\" -> {} ({})",
    name,
    log_path.display(),
    location
);
```

### Dual-Location Discovery
```rust
// Source: Pattern from existing discover_sources()
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLocation {
    Project,
    Global,
}

pub struct DiscoveredSource {
    pub name: String,
    pub log_path: PathBuf,
    pub status: SourceStatus,
    pub location: SourceLocation,  // NEW: track origin
}

pub fn discover_all_sources(discovery: &DiscoveryResult) -> Result<Vec<DiscoveredSource>> {
    let mut sources = Vec::new();

    // Project-local sources (higher priority in display)
    if let Some(project_data) = discovery.data_dir() {
        if project_data.exists() {
            for source in scan_data_dir(&project_data)? {
                sources.push(DiscoveredSource {
                    location: SourceLocation::Project,
                    ..source
                });
            }
        }
    }

    // Global sources
    if let Some(global_data) = global_data_dir() {
        if global_data.exists() {
            for source in scan_data_dir(&global_data)? {
                // Skip if same name exists in project
                if !sources.iter().any(|s| s.name == source.name) {
                    sources.push(DiscoveredSource {
                        location: SourceLocation::Global,
                        ..source
                    });
                }
            }
        }
    }

    Ok(sources)
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| Manual mkdir + chmod | DirBuilder::mode() | Rust 1.6+ | Atomic, race-free |
| Global-only storage | Context-aware storage | This phase | Project isolation |
| Single data_dir() | resolve_data_dir(discovery) | This phase | Location flexibility |

**Deprecated/outdated:**
- Global-only `data_dir()` / `sources_dir()` without context: Still needed for backward compatibility but new code should prefer context-aware variants

## Open Questions

Things that couldn't be fully resolved:

1. **Backward Compatibility: Should existing global streams be visible in projects?**
   - What we know: Decision says "shows both project-local and global appropriately"
   - What's unclear: Does "appropriately" mean separate categories in UI, or merged?
   - Recommendation: Show both, but visually distinguish (like config sources). Project sources first.

2. **Name Collision: Project vs Global with Same Name**
   - What we know: Same source name could exist in both locations
   - What's unclear: Should project shadow global? Show both? Error?
   - Recommendation: Show both with location indicator. Let user decide via selection.

3. **Directory Watcher Scope**
   - What we know: Current dir_watcher watches one directory
   - What's unclear: Should it watch both project and global, or just active context?
   - Recommendation: Watch only the context-appropriate directory (project if in project). Global sources added at startup, not dynamically.

## Sources

### Primary (HIGH confidence)
- [Rust std::fs::DirBuilder](https://doc.rust-lang.org/std/fs/struct.DirBuilder.html) - API for directory creation with permissions
- Existing `src/source.rs` - Current directory management implementation
- Existing `src/config/discovery.rs` - DiscoveryResult with data_dir() method
- Prior decisions from STATE.md [02-01]: Only lazytail.yaml signals project root

### Secondary (MEDIUM confidence)
- [dirs crate docs](https://docs.rs/dirs/5.0) - Confirmed global config_dir() behavior

### Tertiary (LOW confidence)
- None - all findings verified with official documentation and existing codebase

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - All stdlib, well-documented APIs
- Architecture: HIGH - Extends existing patterns from source.rs and discovery.rs
- Pitfalls: HIGH - Based on Phase 2/3 decisions and platform-specific behavior

**Research date:** 2026-02-04
**Valid until:** 90 days (stdlib APIs are stable, architecture extends existing code)
