# ADR-008: Flag-Based Signal Handling

## Status

Accepted

## Context

Capture mode reads from stdin and writes to a file. On SIGINT/SIGTERM, it must clean up marker files before exiting. Calling `process::exit()` inside a signal handler skips destructors and cleanup code.

Options considered:
1. **`process::exit()` in handler** - immediate exit, skips cleanup
2. **Flag-based** (`signal-hook::flag`) - set atomic flag, main loop checks and cleans up
3. **Async signal handling** - tokio signal streams
4. **Custom signal handler** - raw signal handling with `sigaction`

## Decision

We use `signal-hook::flag` for **cooperative, flag-based signal handling**:

```rust
let term_now = Arc::new(AtomicBool::new(false));

for sig in TERM_SIGNALS {
    // Second signal while flag is set: force exit (code 1)
    flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term_now))?;
    // First signal: set flag
    flag::register(*sig, Arc::clone(&term_now))?;
}
```

The capture mode's tee loop checks `shutdown_flag.load(Ordering::SeqCst)` on each line read. When set, it breaks out of the loop and runs normal cleanup (removing the marker file).

**Double Ctrl+C** triggers an immediate exit (code 1) via `register_conditional_shutdown`, providing a force-quit escape hatch.

## Consequences

**Benefits:**
- Cleanup code always runs on first signal (marker files are removed)
- No `process::exit()` in signal handlers, so all destructors execute
- Double Ctrl+C provides force-quit if cleanup hangs
- Simple API: just check an `AtomicBool` in the main loop

**Trade-offs:**
- Cleanup is not instantaneous (waits for current line read to complete)
- Only useful for capture mode; TUI mode handles terminal restore differently (crossterm's raw mode cleanup)
