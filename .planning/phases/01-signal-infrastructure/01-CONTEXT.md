# Phase 1: Signal Infrastructure - Context

**Gathered:** 2026-02-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Application handles termination signals gracefully with proper cleanup coordination. This includes Ctrl+C (SIGINT), SIGTERM, recovery from SIGKILL, and interrupt handling during operations. No new features — just robust signal handling for what already exists.

</domain>

<decisions>
## Implementation Decisions

### Shutdown Feedback
- Silent exit on graceful shutdown — no message unless needed
- If cleanup exceeds ~2 seconds, show timeout message to stderr
- Exit code 0 for successful graceful shutdown

### Force-Exit Behavior
- Double Ctrl+C forces immediate exit — no delay window
- If cleanup is taking time, hint that another Ctrl+C will force-quit (combined with timeout message)
- Force-exit skips all cleanup — exit immediately, leave cleanup for next run
- Exit code 1 for force-quit (non-zero indicates abnormal termination)

### Stale Marker Recovery
- Silent cleanup — user doesn't need to know about recovered state
- Cleanup happens at startup, before showing UI
- If cleanup fails (permissions, etc.), warn to stderr and continue startup

### Interrupt Semantics
- Ctrl+C always exits the app, regardless of current mode or operation
- Esc cancels operations (filter, input mode) — returns to unfiltered view
- No prompts for unsaved state — just exit, state is ephemeral

### Claude's Discretion
- Stale marker detection method (PID check, lock files, etc.)
- Exact timeout threshold before showing message
- Implementation of hint message wording

</decisions>

<specifics>
## Specific Ideas

- "Esc should kill the filter and go back to unfiltered view" — not partial results
- Unix philosophy: silent by default, only speak when something is wrong or slow

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 01-signal-infrastructure*
*Context gathered: 2026-02-03*
