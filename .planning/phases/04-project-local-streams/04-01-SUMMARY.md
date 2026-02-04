---
phase: 04-project-local-streams
plan: 01
subsystem: infra
tags: [directory-resolution, capture-mode, permissions, context-aware]

# Dependency graph
requires:
  - phase: 02-config-discovery
    provides: DiscoveryResult with project_root for context detection
  - phase: 03-config-loading
    provides: Config loading infrastructure
provides:
  - Context-aware directory resolution functions
  - Secure directory creation with 0700 permissions on Unix
  - Project-local capture mode support
  - Capture output showing storage location (project/global)
affects: [04-02, discovery-mode, project-scoped-streams]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - Context-aware functions accept DiscoveryResult
    - Secure directory creation pattern with DirBuilderExt
    - Backward-compatible function pairs (legacy + context-aware)

key-files:
  created: []
  modified:
    - src/source.rs
    - src/capture.rs
    - src/main.rs

key-decisions:
  - "Added _for_context variants instead of modifying existing functions for backward compatibility"
  - "Secure permissions (0700) applied only on Unix via cfg(unix)"
  - "Location indicator (project/global) shown in capture header for user awareness"

patterns-established:
  - "Context-aware function pattern: accept DiscoveryResult, use resolve_*_dir(discovery)"
  - "Secure directory creation: create_secure_dir uses DirBuilder with mode(0o700)"

# Metrics
duration: 8min
completed: 2026-02-04
---

# Phase 04 Plan 01: Context-Aware Directory Resolution Summary

**Context-aware directory functions for project-local stream storage using DiscoveryResult**

## Performance

- **Duration:** 8 min
- **Started:** 2026-02-04T10:15:00Z
- **Completed:** 2026-02-04T10:23:00Z
- **Tasks:** 2
- **Files modified:** 3

## Accomplishments
- Added context-aware directory resolution (resolve_data_dir, resolve_sources_dir)
- Implemented secure directory creation with 0700 Unix permissions
- Updated capture mode to use project-local or global directories based on context
- Added location indicator (project/global) to capture output for user visibility

## Task Commits

Each task was committed atomically:

1. **Task 1: Add context-aware directory functions to source.rs** - `2bb6982` (feat)
2. **Task 2: Integrate context-aware capture mode** - `f1250bb` (feat)

## Files Created/Modified
- `src/source.rs` - Added create_secure_dir, resolve_data_dir, resolve_sources_dir, ensure_directories_for_context, and context-aware marker functions with tests
- `src/capture.rs` - Updated run_capture_mode to accept DiscoveryResult and use context-aware functions
- `src/main.rs` - Updated capture mode call to pass discovery result

## Decisions Made
- Added `_for_context` function variants to preserve backward compatibility with existing code
- Used cfg(unix) conditional compilation for mode 0700 permissions (Windows uses defaults)
- Added location indicator "(project)" or "(global)" to capture header output for user clarity

## Deviations from Plan
None - plan executed exactly as written.

## Issues Encountered
None - all changes compiled and tested successfully.

## Next Phase Readiness
- Context-aware capture mode ready for use
- Plan 02 will update discovery mode to also use context-aware directories
- Pre-existing test failures in app::tests::test_add_to_history* remain (unrelated to this work)

---
*Phase: 04-project-local-streams*
*Completed: 2026-02-04*
