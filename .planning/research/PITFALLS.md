# Domain Pitfalls: Project Configuration and Signal Handling

**Domain:** Rust CLI tool - project-scoped configuration and signal-based cleanup
**Researched:** 2026-02-03
**Project:** LazyTail (terminal log viewer)

## Critical Pitfalls

Mistakes that cause rewrites, data loss, or major issues.

---

### Pitfall 1: Async-Signal-Unsafe Operations in Signal Handlers

**What goes wrong:** Signal handlers call functions that are not async-signal-safe (mutex locks, memory allocation, I/O operations), causing deadlocks, crashes, or undefined behavior.

**Why it happens:** POSIX restricts the set of functions callable inside signal handlers to a very small set. Mutexes, memory allocation/deallocation, and most I/O operations are NOT allowed. Signal handlers can interrupt a thread at any point, including while the thread holds a lock the handler needs.

**Consequences:**
- Deadlock when handler waits for a lock held by interrupted code
- Memory corruption from allocator state inconsistency
- Silent data loss when cleanup operations fail mid-execution
- Undefined behavior that manifests differently across runs

**Warning signs:**
- Cleanup code in signal handler calls `fs::remove_file()`, `println!()`, or acquires any `Mutex`
- Using `std::process::exit()` inside signal handler (calls destructors, which may allocate)
- Signal handler does anything more complex than setting an `AtomicBool`

**Prevention:**
1. Use the "flag and check" pattern: signal handler ONLY sets an `Arc<AtomicBool>`
2. Main loop checks the flag and performs cleanup in normal code path
3. Use `signal_hook::flag::register()` which handles this safely
4. For LazyTail: existing `capture.rs` signal handler already uses this pattern correctly

**Detection:**
- Code review: any function call in signal handler other than atomic operations
- Runtime: hangs after Ctrl+C, especially under load

**Phase mapping:** Address in Phase 1 (signal infrastructure) - foundation for all cleanup

**Confidence:** HIGH - verified via [signal-hook documentation](https://docs.rs/signal-hook) and [Rust CLI Book signals chapter](https://rust-cli.github.io/book/in-depth/signals.html)

---

### Pitfall 2: Race Condition Between Signal and Normal Exit

**What goes wrong:** Signal arrives after cleanup begins but before it completes, or cleanup runs twice (once from signal, once from normal exit path).

**Why it happens:** Signal handlers run concurrently with the interrupted thread. When using `AtomicBool` flags, there's a window where:
- Normal exit starts cleanup
- Signal arrives, sees flag not set, starts its own cleanup
- Both paths race to delete the same files

**Consequences:**
- Double-free errors on resources
- Partial cleanup (one path errors, other doesn't run)
- File system errors from removing already-removed files
- Inconsistent state in marker files

**Warning signs:**
- Cleanup logic exists in both signal handler thread AND normal exit path
- No coordination mechanism between signal handler and main thread cleanup
- Using `std::process::exit(0)` in signal handler (LazyTail's current `capture.rs` does this!)

**Prevention:**
1. Single cleanup path: signal sets flag, main loop handles cleanup before exit
2. Use `AtomicBool` with `compare_exchange` to ensure cleanup runs exactly once
3. Idempotent cleanup operations (removing non-existent file should not error)
4. Remove `std::process::exit()` from signal handler - let main loop exit after cleanup

**Detection:**
- Rapid signal sending (Ctrl+C multiple times quickly) causes errors
- Log file analysis shows duplicate cleanup attempts
- Stale marker files after apparently clean shutdown

**Phase mapping:** Phase 1 - must be solved before adding cleanup operations

**Confidence:** HIGH - verified via [signal-hook docs](https://docs.rs/signal-hook) and [issue discussions](https://github.com/Detegr/rust-ctrlc/issues/6)

---

### Pitfall 3: SIGKILL/SIGSTOP Cannot Be Caught

**What goes wrong:** Design assumes all signals can be handled for cleanup. SIGKILL (`kill -9`) and SIGSTOP cannot be caught, blocked, or ignored - they terminate/stop immediately without running any cleanup.

**Why it happens:** OS kernel handles SIGKILL/SIGSTOP before they reach the process. This is intentional - provides guaranteed way to terminate unresponsive processes.

**Consequences:**
- Orphaned marker files (`.lazytail/sources/<name>`) when process is killed with `-9`
- Orphaned lock files
- Incomplete writes to config/state files
- Users confused why "clean" shutdown leaves files behind

**Warning signs:**
- Documentation claims "cleanup always runs"
- No stale marker detection on startup
- Assuming marker file presence means process is definitely running

**Prevention:**
1. **Always detect stale markers on startup**: Check if PID in marker file is still running
2. **Write to temp file, rename atomically**: Prevents corrupt state files on kill
3. **LazyTail already has this**: `source.rs` checks `is_pid_running()` for markers
4. **Document the limitation**: Users should know `kill -9` leaves orphans

**Detection:**
- Run `kill -9 <pid>` on running process, check for leftover files
- Test recovery: does app handle stale markers gracefully on next run?

**Phase mapping:** Phase 2 (marker management) - expand existing stale detection to all cleanup artifacts

**Confidence:** HIGH - fundamental OS limitation, verified via [signal-hook docs](https://docs.rs/signal-hook) and [Rust CLI Book](https://rust-cli.github.io/book/in-depth/signals.html)

---

### Pitfall 4: Config File Discovery Infinite Loop or Crosses Filesystem Boundaries

**What goes wrong:** Walking up directory tree to find `lazytail.yaml` either:
1. Loops forever (symlink cycles)
2. Crosses filesystem boundaries (slow network mounts, unrelated projects)
3. Finds config in unexpected location (parent of home directory)

**Why it happens:** Naive "walk up until root" algorithms don't account for:
- Symlinks pointing to parent directories
- Mount points (NFS, FUSE, etc.)
- User running from deeply nested path in unrelated project

**Consequences:**
- Hang on startup when traversing network filesystems
- Loading wrong project's config from parent directory
- Unexpected behavior when config found in `/` or `/home`
- Performance issues on slow network mounts

**Warning signs:**
- Simple `loop { parent = path.parent() }` without termination conditions
- No filesystem boundary detection (checking `stat().dev()` changes)
- No maximum depth limit

**Prevention:**
1. **Stop at filesystem boundaries**: Compare `stat().st_dev` between directories
2. **Stop at well-known directories**: `$HOME`, `/`, repository root (`.git`)
3. **Implement maximum depth**: e.g., 32 levels as sanity check
4. **Use canonical paths**: `Path::canonicalize()` resolves symlinks upfront
5. **Consider Git's approach**: `GIT_DISCOVERY_ACROSS_FILESYSTEM` equivalent env var

```rust
// Example: stop at filesystem boundary
fn find_config_up(start: &Path) -> Option<PathBuf> {
    let start_dev = start.metadata().ok()?.dev();
    let mut current = start.to_path_buf();

    for _ in 0..32 {  // Max depth limit
        let config = current.join("lazytail.yaml");
        if config.exists() { return Some(config); }

        let parent = current.parent()?;
        // Stop at filesystem boundary
        if parent.metadata().ok()?.dev() != start_dev { return None; }
        // Stop at home directory
        if Some(&current) == dirs::home_dir().as_ref() { return None; }

        current = parent.to_path_buf();
    }
    None
}
```

**Detection:**
- Test with symlink: `ln -s . loop && cd loop && lazytail`
- Test from NFS mount or slow filesystem
- Test from root directory

**Phase mapping:** Phase 3 (config file discovery) - implement with boundary checks from start

**Confidence:** MEDIUM - synthesized from [Git behavior](https://github.com/astral-sh/uv/issues/7351) and [file traversal best practices](https://docs.python.org/3/library/pathlib.html)

---

## Moderate Pitfalls

Mistakes that cause delays, technical debt, or degraded UX.

---

### Pitfall 5: Memory Ordering Too Weak for Signal Handler Communication

**What goes wrong:** Using `Ordering::Relaxed` for `AtomicBool` in signal handler causes main thread to never see the flag change, or sees it with unbounded delay.

**Why it happens:** `Relaxed` ordering provides no synchronization guarantees. On some architectures (especially ARM), stores may not be visible to other threads for a long time.

**Consequences:**
- Cleanup doesn't trigger despite signal received
- Intermittent failures hard to reproduce (works on x86, fails on ARM)
- App continues running after Ctrl+C

**Prevention:**
1. Use `Ordering::SeqCst` for both store (in handler) and load (in main loop)
2. Alternative: `Release` for store, `Acquire` for load (slightly more efficient)
3. `signal_hook::flag::register()` handles this correctly - prefer it over manual implementation

```rust
// signal_hook does this correctly:
// Store: Ordering::SeqCst (or Release)
// Load: Ordering::SeqCst (or Acquire)
let shutdown = Arc::new(AtomicBool::new(false));
signal_hook::flag::register(SIGTERM, Arc::clone(&shutdown))?;

// In main loop:
if shutdown.load(Ordering::SeqCst) {
    // cleanup
}
```

**Detection:**
- Test on non-x86 hardware (ARM, RISC-V)
- `cargo miri` may catch some ordering bugs

**Phase mapping:** Phase 1 - get ordering right in initial implementation

**Confidence:** HIGH - verified via [std::sync::atomic documentation](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html)

---

### Pitfall 6: YAML Config Parsing with Deprecated serde_yaml

**What goes wrong:** Using `serde_yaml` which is unmaintained, leading to security issues, missing features, or future incompatibility.

**Why it happens:** `serde_yaml` was the standard for years but is now deprecated (as of 2024-03-25). Tutorials and examples still reference it.

**Consequences:**
- No bug fixes or security patches
- Compilation failures with newer Rust versions (future)
- Missing YAML 1.2 features

**Prevention:**
1. Use `serde_yml` (maintained fork) instead of `serde_yaml`
2. Consider `serde_json` if YAML features aren't needed (simpler, more maintained)
3. Pin exact version and monitor for deprecation notices

**Detection:**
- `cargo audit` warnings
- Compilation failures after Rust updates

**Phase mapping:** Phase 3 (config implementation) - choose correct library from start

**Confidence:** MEDIUM - verified via [serde_yaml deprecation discussion](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868)

---

### Pitfall 7: Permission Issues with Project-Local Directories

**What goes wrong:** Creating `.lazytail/` directory in project root fails or creates security issues:
- No write permission in project directory
- Directory created with wrong permissions (too permissive)
- Shared project directories (multiple users) cause conflicts

**Why it happens:**
- Running in read-only directories (CD-ROM, system directories)
- Default umask doesn't restrict permissions enough
- Shared filesystems with group ownership

**Consequences:**
- Startup failure in unexpected locations
- Security: sensitive data readable by other users
- Conflicts when multiple users run in same project directory

**Prevention:**
1. **Graceful degradation**: If can't create `.lazytail/`, warn and continue without project-local config
2. **Explicit permissions**: Create directories with mode 0700 (`fs::DirBuilder::new().mode(0o700)`)
3. **Check umask interaction**: See [Cargo umask vulnerability](https://github.com/rust-lang/cargo/security/advisories/GHSA-j3xp-wfr4-hx87) for lessons learned
4. **Use XDG for sensitive data**: Config in `.lazytail/`, but sensitive runtime data in `$XDG_RUNTIME_DIR`

```rust
use std::os::unix::fs::DirBuilderExt;

fn create_project_dir(project_root: &Path) -> io::Result<PathBuf> {
    let dir = project_root.join(".lazytail");
    std::fs::DirBuilder::new()
        .mode(0o700)  // rwx for owner only
        .create(&dir)?;
    Ok(dir)
}
```

**Detection:**
- Test in `/tmp` (writable) vs `/usr` (read-only)
- Test with restrictive umask: `umask 077 && lazytail`
- Test in shared directory with different users

**Phase mapping:** Phase 2 (project directory creation)

**Confidence:** MEDIUM - synthesized from [XDG spec](https://whitequark.github.io/rust-xdg/xdg/struct.BaseDirectories.html) and [Cargo security advisory](https://github.com/rust-lang/cargo/security/advisories/GHSA-j3xp-wfr4-hx87)

---

### Pitfall 8: Double Ctrl+C Pattern Not Implemented

**What goes wrong:** User presses Ctrl+C, cleanup starts but is slow/hangs, user can't force quit.

**Why it happens:** Only first Ctrl+C is handled; subsequent signals are ignored because handler is already running or flag is already set.

**Consequences:**
- User must resort to `kill -9` (leaves orphaned files)
- Frustrated users
- Lost trust in "graceful shutdown"

**Prevention:**
1. Track signal count: first Ctrl+C starts cleanup, second forces immediate exit
2. Use `signal_hook::flag::register_conditional_shutdown()` for this pattern
3. Display message on first signal: "Cleaning up... press Ctrl+C again to force quit"

```rust
use std::sync::atomic::{AtomicUsize, Ordering};

static SIGNAL_COUNT: AtomicUsize = AtomicUsize::new(0);

// In signal handler:
let count = SIGNAL_COUNT.fetch_add(1, Ordering::SeqCst);
if count >= 1 {
    std::process::abort();  // Immediate termination, no cleanup
}
```

**Detection:**
- Test: start filtering large file, Ctrl+C twice quickly
- Cleanup takes more than 1-2 seconds? User will Ctrl+C again

**Phase mapping:** Phase 1 (signal handling) - implement alongside basic signal handling

**Confidence:** HIGH - verified via [Rust CLI Book](https://rust-cli.github.io/book/in-depth/signals.html) and [signal-hook flag module](https://docs.rs/signal-hook)

---

### Pitfall 9: Cleanup Runs in Drop but App Compiled with panic=abort

**What goes wrong:** Cleanup logic in `Drop` implementations doesn't run when:
- `panic = 'abort'` is set in Cargo.toml
- Signal causes immediate termination
- Static/lazy_static instances never dropped

**Why it happens:** `panic = 'abort'` skips unwinding and destructors. Signal-induced exits may not run destructors either. Lazy static instances only drop at process exit, which may not happen cleanly.

**Consequences:**
- Cleanup code in `impl Drop` silently skipped
- Orphaned temporary files
- Stale lock/marker files

**Prevention:**
1. **Don't rely solely on Drop for critical cleanup**
2. Implement explicit cleanup function called from both signal path and normal exit
3. If using `tempfile` crate, be aware: "TempDir and NamedTempFile both rely on Rust destructors for cleanup"
4. Consider keeping `panic = 'unwind'` (default) for CLI tools where cleanup matters

**Detection:**
- Check Cargo.toml for `panic = 'abort'`
- Test: force panic during execution, check for orphaned files

**Phase mapping:** Phase 1 - decide cleanup strategy before implementing

**Confidence:** HIGH - verified via [tempfile crate docs](https://docs.rs/tempfile/) and [Rust panic documentation](https://doc.rust-lang.org/reference/panic.html)

---

## Minor Pitfalls

Mistakes that cause annoyance but are easily fixable.

---

### Pitfall 10: Config Precedence Confusion

**What goes wrong:** Users confused about which config takes effect when multiple exist (global, project, CLI args).

**Prevention:**
1. Document clear precedence: CLI args > project config > global config > defaults
2. Add `--verbose` or `--debug` flag showing which config loaded
3. Add `lazytail config show` command to display effective config

**Phase mapping:** Phase 3 (config implementation) - plan UX from start

**Confidence:** MEDIUM - common pattern from other tools

---

### Pitfall 11: Config File Not in .gitignore Warning

**What goes wrong:** Users accidentally commit `.lazytail/` directory or project config with sensitive paths/settings.

**Prevention:**
1. On first creation, warn: "Consider adding .lazytail/ to .gitignore"
2. Add `.lazytail/` to project's `.gitignore` automatically (with user confirmation)
3. Store truly sensitive data (API keys, etc.) in XDG directories, not project

**Phase mapping:** Phase 2 (project directory creation)

**Confidence:** LOW - UX preference, not a technical requirement

---

### Pitfall 12: SIGHUP Handling Not Considered

**What goes wrong:** When terminal closes (SSH disconnect, terminal window closed), SIGHUP is sent but not handled, causing abrupt termination without cleanup.

**Prevention:**
1. Register handler for SIGHUP alongside SIGINT/SIGTERM
2. Treat SIGHUP same as SIGTERM (graceful shutdown)
3. `signal_hook` makes this easy: `register(SIGHUP, shutdown_flag.clone())?`

**Detection:**
- SSH into machine, run lazytail, disconnect SSH - check for orphaned files

**Phase mapping:** Phase 1 (signal handling)

**Confidence:** HIGH - verified via [signal-hook documentation](https://docs.rs/signal-hook)

---

## Phase-Specific Warnings

| Phase Topic | Likely Pitfall | Mitigation |
|-------------|---------------|------------|
| Signal infrastructure | Async-signal-unsafe operations | Use flag pattern only |
| Signal infrastructure | Race between signal and exit | Single cleanup path with AtomicBool coordination |
| Signal infrastructure | Memory ordering | Use SeqCst or Release/Acquire |
| Signal infrastructure | Double Ctrl+C not handled | Implement signal counting |
| Marker management | Orphaned files from SIGKILL | Stale marker detection on startup |
| Marker management | Race in marker creation | Atomic file creation with `create_new` |
| Project directory | Permission issues | Graceful degradation + explicit mode |
| Project directory | Security of created files | Mode 0700 for directories |
| Config discovery | Infinite loop/boundary crossing | Filesystem boundary + depth limit |
| Config parsing | Deprecated serde_yaml | Use serde_yml instead |
| Cleanup | Drop not running | Explicit cleanup, not just Drop |

---

## LazyTail-Specific Notes

**Existing good patterns:**
- `capture.rs` already uses `signal_hook` with `AtomicBool` flag pattern
- `source.rs` already has `is_pid_running()` for stale marker detection
- Marker creation uses `create_new` for atomic creation

**Current issues to address:**
- `capture.rs` calls `std::process::exit(0)` in signal handler - should let main loop exit
- Main viewing mode has no signal handling for cleanup
- No project-local config discovery yet

---

## Sources

### HIGH Confidence (Official Documentation)
- [signal-hook crate documentation](https://docs.rs/signal-hook) - Async-signal-safety, flag pattern
- [Rust CLI Book - Signal Handling](https://rust-cli.github.io/book/in-depth/signals.html) - Best practices
- [std::sync::atomic::Ordering](https://doc.rust-lang.org/std/sync/atomic/enum.Ordering.html) - Memory ordering
- [tempfile crate documentation](https://docs.rs/tempfile/) - Cleanup limitations
- [Rust panic reference](https://doc.rust-lang.org/reference/panic.html) - Unwind vs abort

### MEDIUM Confidence (Multiple Sources Agree)
- [XDG BaseDirectories in rust-xdg](https://whitequark.github.io/rust-xdg/xdg/struct.BaseDirectories.html) - Permission requirements
- [Cargo umask security advisory](https://github.com/rust-lang/cargo/security/advisories/GHSA-j3xp-wfr4-hx87) - Permission pitfalls
- [serde_yaml deprecation discussion](https://users.rust-lang.org/t/serde-yaml-deprecation-alternatives/108868) - Library status
- [uv config discovery issue](https://github.com/astral-sh/uv/issues/7351) - Filesystem boundary handling

### LOW Confidence (Single Source/Synthesized)
- Git's `GIT_DISCOVERY_ACROSS_FILESYSTEM` pattern - config discovery boundaries
- Double Ctrl+C UX pattern - user expectation from other tools
