# Phase 1: Signal Infrastructure - Research

**Researched:** 2026-02-03
**Domain:** Unix signal handling, graceful shutdown, TUI cleanup coordination
**Confidence:** HIGH

## Summary

This phase implements graceful shutdown infrastructure for a ratatui/crossterm TUI application. The research confirms that LazyTail already uses `signal-hook` (v0.3) in `capture.rs`, but the current implementation has issues: it calls `process::exit()` directly from the signal handler thread, bypassing main thread cleanup. The existing codebase also has working PID-based marker detection in `source.rs` that can be leveraged for stale marker recovery.

The standard approach for Rust TUI applications combines: (1) signal-hook's `flag` module for atomic flag-based signal detection, (2) the `register_conditional_shutdown` pattern for double Ctrl+C force-exit, and (3) coordinated cleanup through the main event loop rather than signal handler threads. The key insight is that crossterm's raw mode intercepts Ctrl+C as a key event, so the application already handles it via the input system - signal handling is primarily needed for the capture mode and SIGTERM.

**Primary recommendation:** Refactor capture.rs to use flag-based signal detection (not `process::exit()`), add `register_conditional_shutdown` for double Ctrl+C support, and implement stale marker cleanup at startup using the existing `is_pid_running()` function.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| signal-hook | 0.3.x | Unix signal handling | Already in Cargo.toml; widest community support, safe abstractions |
| crossterm | 0.28.x | Terminal I/O, raw mode, key events | Already in Cargo.toml; handles Ctrl+C as KeyEvent in raw mode |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| signal-hook::flag | 0.3.x | Atomic flag-based signal detection | Double Ctrl+C pattern, non-blocking signal checks |
| signal-hook::consts::TERM_SIGNALS | 0.3.x | Standard termination signals array | Registering handlers for SIGINT, SIGTERM, SIGHUP, etc. |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| signal-hook | ctrlc crate | ctrlc is simpler but doesn't work with crossterm raw mode; signal-hook already in deps |
| Signals::iterator | flag module | Iterator blocks; flags are non-blocking and fit existing event loop |
| Manual PID check | pidlock/fslock crates | Extra dependency; existing `is_pid_running()` already works well |

**Installation:**
```bash
# Already present in Cargo.toml:
signal-hook = "0.3"
```

No new dependencies needed.

## Architecture Patterns

### Recommended Project Structure
```
src/
├── signal.rs           # NEW: Signal handling module
├── capture.rs          # MODIFY: Use signal.rs, remove process::exit()
├── source.rs           # MODIFY: Add cleanup_stale_markers() function
└── main.rs             # MODIFY: Register signal handlers, check stale markers at startup
```

### Pattern 1: Flag-Based Signal Detection (Double Ctrl+C)
**What:** Use `register_conditional_shutdown` + `register` to arm immediate exit on second signal
**When to use:** Any graceful shutdown scenario where user might send multiple signals
**Example:**
```rust
// Source: https://docs.rs/signal-hook/0.3.17/signal_hook/flag/fn.register_conditional_shutdown.html
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;

fn setup_signal_handlers() -> Result<Arc<AtomicBool>, std::io::Error> {
    let term_now = Arc::new(AtomicBool::new(false));

    for sig in TERM_SIGNALS {
        // First: register conditional shutdown (does nothing while term_now is false)
        flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term_now))?;
        // Second: set term_now to true, arming the above for next signal
        flag::register(*sig, Arc::clone(&term_now))?;
    }

    Ok(term_now)
}
```

### Pattern 2: Coordinated Cleanup via Main Loop
**What:** Signal handlers set flags; main loop checks flags and performs cleanup
**When to use:** TUI applications where cleanup requires terminal state restoration
**Example:**
```rust
// In main loop (run_app_with_discovery):
if shutdown_flag.load(Ordering::SeqCst) {
    // Cleanup resources
    cleanup_markers();
    break;  // Exit loop, let caller restore terminal
}
```

### Pattern 3: Stale Marker Recovery at Startup
**What:** Check marker files for dead PIDs before starting UI
**When to use:** Applications with marker/lock files that might be orphaned by SIGKILL
**Example:**
```rust
// Source: Existing pattern in source.rs
pub fn cleanup_stale_markers() -> Result<()> {
    let sources = sources_dir().ok_or_else(|| anyhow::anyhow!("No sources dir"))?;

    for entry in std::fs::read_dir(&sources)? {
        let path = entry?.path();
        if let Some(pid) = read_marker_pid(&path) {
            if !is_pid_running(pid) {
                // Stale marker - remove silently
                let _ = std::fs::remove_file(&path);
            }
        }
    }
    Ok(())
}
```

### Anti-Patterns to Avoid
- **Calling `process::exit()` from signal handlers:** Bypasses RAII cleanup, leaves terminal in raw mode
- **Using `ctrlc` crate with crossterm raw mode:** Raw mode intercepts Ctrl+C as KeyEvent; ctrlc handler never fires
- **Blocking signal iterators in TUI apps:** Blocks the event loop; use non-blocking flag checks instead
- **Deleting markers without PID verification:** Race condition if process is starting up

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| Double Ctrl+C detection | Manual signal counting | `register_conditional_shutdown` | Handles edge cases, thread-safe, well-tested |
| PID liveness check | Shell out to `ps` | `is_pid_running()` (already exists) | Cross-platform, no subprocess overhead |
| Signal registration | Raw `libc::signal` | signal-hook crate | Signal-safety, proper handler chaining |
| Timeout for cleanup | Manual timer thread | `std::time::Instant` + main loop check | No extra threads, simpler |

**Key insight:** The signal-hook crate exists specifically because signal handling is notoriously difficult to get right. The flag module solves the common patterns without exposing signal-safety hazards.

## Common Pitfalls

### Pitfall 1: process::exit() in Signal Handler
**What goes wrong:** Terminal left in raw mode, cursor hidden, alternate screen active
**Why it happens:** Signal handlers run asynchronously; exit() skips Drop impls
**How to avoid:** Set atomic flag, let main loop exit and run cleanup
**Warning signs:** Terminal broken after Ctrl+C; need to run `reset`

### Pitfall 2: Blocking on Signal Iterator in Event Loop
**What goes wrong:** UI freezes, events not processed
**Why it happens:** `Signals::forever()` blocks until signal arrives
**How to avoid:** Use `flag::register()` with atomic bool, check in existing event loop
**Warning signs:** Application unresponsive between signals

### Pitfall 3: Wrong Registration Order for Double Ctrl+C
**What goes wrong:** First signal immediately exits instead of arming
**Why it happens:** `register()` fires before `register_conditional_shutdown` checked
**How to avoid:** Register conditional shutdown BEFORE register
**Warning signs:** Never get chance for graceful shutdown

### Pitfall 4: Race in Stale Marker Cleanup
**What goes wrong:** Delete marker while process is starting up
**Why it happens:** PID check happens before process writes marker
**How to avoid:** Only clean markers where PID is definitely dead (not just not found)
**Warning signs:** "Source already active" errors on restart

### Pitfall 5: Forgetting SIGTERM
**What goes wrong:** systemctl stop / docker stop hangs then force-kills
**Why it happens:** Only handling SIGINT (Ctrl+C), not SIGTERM
**How to avoid:** Use `TERM_SIGNALS` constant which includes both
**Warning signs:** Clean shutdown only works with Ctrl+C, not kill

## Code Examples

Verified patterns from official sources:

### Setup Double Ctrl+C with Graceful Shutdown
```rust
// Source: signal-hook documentation
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;

/// Setup signal handlers for graceful shutdown with double-signal force exit.
/// Returns a flag that becomes true when shutdown is requested.
pub fn setup_shutdown_handlers() -> Result<Arc<AtomicBool>, std::io::Error> {
    let shutdown_requested = Arc::new(AtomicBool::new(false));

    for sig in TERM_SIGNALS {
        // When terminated by a second term signal, exit with code 1.
        // This does nothing on first signal (shutdown_requested is false).
        flag::register_conditional_shutdown(*sig, 1, Arc::clone(&shutdown_requested))?;

        // This "arms" the above by setting shutdown_requested to true.
        // Order matters: conditional_shutdown must be registered first.
        flag::register(*sig, Arc::clone(&shutdown_requested))?;
    }

    Ok(shutdown_requested)
}
```

### Check Shutdown Flag in Event Loop
```rust
// Pattern for existing run_app_with_discovery function
fn run_app_with_discovery<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    shutdown_flag: &AtomicBool,
    // ... other params
) -> Result<()> {
    loop {
        // Check for shutdown signal (non-blocking)
        if shutdown_flag.load(Ordering::SeqCst) {
            // Perform any cleanup before exiting
            // Terminal restoration happens in caller
            break;
        }

        // ... existing event loop code ...

        if app.should_quit {
            break;
        }
    }
    Ok(())
}
```

### Cleanup Stale Markers at Startup
```rust
// Add to source.rs
use std::fs;

/// Remove marker files for processes that are no longer running.
/// Called at startup to recover from SIGKILL scenarios.
pub fn cleanup_stale_markers() -> Result<()> {
    let Some(sources) = sources_dir() else {
        return Ok(());  // No sources dir, nothing to clean
    };

    if !sources.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(&sources)? {
        let entry = entry?;
        let path = entry.path();

        // Skip non-files
        if !path.is_file() {
            continue;
        }

        // Read PID from marker
        if let Some(pid) = read_marker_pid(&path) {
            // Only remove if process is definitely not running
            if !is_pid_running(pid) {
                // Remove silently - user doesn't need to know
                let _ = fs::remove_file(&path);
            }
        }
    }

    Ok(())
}
```

### Refactored Capture Mode Signal Handler
```rust
// Refactored capture.rs - no process::exit()
use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;

pub fn run_capture_mode(name: String) -> Result<()> {
    // ... validation, marker creation ...

    // Setup signal handler that sets flag instead of exiting
    let running = Arc::new(AtomicBool::new(true));
    let term_requested = Arc::new(AtomicBool::new(false));

    for sig in TERM_SIGNALS {
        // Set term_requested when signal received
        flag::register(*sig, Arc::clone(&term_requested))?;
    }

    // ... file setup ...

    // Tee loop with flag check
    for line in reader.lines() {
        // Check for termination signal
        if term_requested.load(Ordering::SeqCst) {
            break;
        }

        // ... existing line processing ...
    }

    // Cleanup happens here, not in signal handler
    remove_marker(&name)?;

    Ok(())
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| `ctrlc` crate | signal-hook `flag` module | ~2020 | Better integration with event loops |
| process::exit() in handlers | Flag + main loop exit | Always best practice | Proper cleanup via RAII |
| Signals::iterator in thread | Non-blocking flag checks | ~2019 | Better for TUI apps |

**Deprecated/outdated:**
- Using `ctrlc` with crossterm: Does not work in raw mode; crossterm intercepts Ctrl+C as KeyEvent

## Open Questions

Things that couldn't be fully resolved:

1. **Exact timeout threshold for "cleanup taking too long" message**
   - What we know: User decision says ~2 seconds
   - What's unclear: Should this be configurable? What's the right default?
   - Recommendation: Start with 2 seconds hardcoded, can make configurable later if needed

2. **Hint message wording for force-quit**
   - What we know: Should combine with timeout message
   - What's unclear: Exact wording that fits Unix philosophy (terse but clear)
   - Recommendation: "Cleanup timeout, press Ctrl+C again to force exit"

## Sources

### Primary (HIGH confidence)
- signal-hook 0.3.x docs - `flag` module, `register_conditional_shutdown`
- LazyTail source code - existing capture.rs, source.rs patterns

### Secondary (MEDIUM confidence)
- [Rust CLI book - Signal handling](https://rust-cli.github.io/book/in-depth/signals.html) - General patterns
- [signal-hook GitHub](https://github.com/vorner/signal-hook) - Double Ctrl+C pattern
- [ratatui.rs recipes](https://ratatui.rs/recipes/apps/terminal-and-event-handler/) - Terminal cleanup patterns

### Tertiary (LOW confidence)
- [DEV.to - Handling Ctrl+C with crossterm](https://dev.to/plecos/handling-ctrl-c-while-using-crossterm-1kil) - Confirms crossterm intercepts Ctrl+C

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - signal-hook already in use, well-documented patterns
- Architecture: HIGH - patterns verified against multiple official sources
- Pitfalls: HIGH - derived from official docs and known issues

**Research date:** 2026-02-03
**Valid until:** 2026-05-03 (stable domain, 90 days)
