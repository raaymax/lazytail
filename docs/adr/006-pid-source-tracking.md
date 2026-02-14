# ADR-006: PID-Based Source Tracking

## Status

Accepted

## Context

LazyTail's capture mode (`cmd | lazytail -n NAME`) writes stdin to a file. The TUI needs to know whether a source is actively being written to (live) or has ended. This affects UI indicators and determines whether to delete source files on tab close.

Options considered:
1. **Lock files** (flock/fcntl) - OS-level file locking
2. **PID marker files** - write PID to a file, check if process is alive
3. **Socket-based** - capture process listens on a socket
4. **File modification time** - consider "active" if recently modified

## Decision

We use **PID marker files** in a `sources/` directory alongside the `data/` directory:

- **Capture mode creates** `sources/NAME` containing the PID of the capture process
- **TUI checks** `sources/NAME` exists and whether the PID is still running
  - Linux: checks `/proc/<pid>/` existence
  - macOS/BSD: uses `kill(pid, 0)` with EPERM handling
- **Capture mode removes** the marker on clean exit (EOF or signal)
- **Stale marker cleanup**: on startup, scan all markers and remove those with dead PIDs (handles SIGKILL/OOM scenarios)

Collision detection: if a marker exists with a running PID, `create_marker_for_context()` rejects the new capture with an error. Uses `OpenOptions::create_new(true)` for atomic creation.

## Consequences

**Benefits:**
- Simple, filesystem-based, no daemon or socket needed
- Works across terminals (viewer and capture can be in different sessions)
- Automatic stale cleanup handles ungraceful termination (SIGKILL, OOM)
- Atomic marker creation prevents race conditions between concurrent captures

**Trade-offs:**
- PID reuse is theoretically possible (extremely unlikely in practice for the time between capture end and TUI check)
- Requires filesystem access to the sources directory
- Status polling: TUI refreshes source status each render cycle (cheap: just a stat + file read)
- Stale markers accumulate if lazytail is never run after a crash (cleaned on next startup)
