---
phase: 04-project-local-streams
plan: 02
subsystem: discovery
tags: [discovery-mode, dual-location, source-location, context-aware]

# Dependency graph
requires:
  - phase: 04-project-local-streams
    plan: 01
    provides: Context-aware directory resolution functions
provides:
  - SourceLocation enum for tracking source origin
  - discover_sources_for_context for dual-location discovery
  - Project source shadowing over global sources
  - Context-aware directory watching
affects: [ui-display, source-management, future-location-indicators]

# Tech tracking
tech-stack:
  added: []
  patterns:
    - SourceLocation enum for origin tracking
    - Dual-location scanning with shadowing
    - Context parameter propagation through call stack

key-files:
  created: []
  modified:
    - src/source.rs
    - src/main.rs

key-decisions:
  - "SourceLocation enum with Project and Global variants for source origin tracking"
  - "Project sources shadow global sources with same name (project takes precedence)"
  - "scan_data_directory helper function extracted to avoid code duplication"
  - "watched_location parameter passed through call chain for correct location assignment"

patterns-established:
  - "Dual-location discovery: scan project first, then global, skip duplicates"
  - "Context-aware watcher: watch directory matches context (project or global)"

# Metrics
duration: 7min
completed: 2026-02-04
---

# Phase 04 Plan 02: Dual-Location Discovery Summary

**Context-aware discovery mode scanning both project-local and global data directories**

## Files Modified

### src/source.rs
- Added `SourceLocation` enum with `Project` and `Global` variants
- Added `location` field to `DiscoveredSource` struct
- Created `scan_data_directory` helper function for code reuse
- Added `check_source_status_in_dir` for location-specific status checks
- Added `discover_sources_for_context` to scan both project and global directories
- Updated existing `discover_sources` to set `location: SourceLocation::Global`
- Added tests for dual-location discovery and shadowing behavior

### src/main.rs
- Updated `run_discovery_mode` to accept `DiscoveryResult` parameter
- Replaced `discover_sources` with `discover_sources_for_context`
- Replaced `ensure_directories` with `ensure_directories_for_context`
- Updated directory watcher to watch context-appropriate directory
- Added `watched_location` parameter to `run_app_with_discovery`
- Newly discovered sources via watcher use correct location

## Decisions Made

1. **SourceLocation enum**: Simple two-variant enum (Project, Global) for tracking where sources were discovered

2. **Project shadows global**: When a source with the same name exists in both locations, only the project-local version is returned in discovery results

3. **Helper function extraction**: `scan_data_directory` contains the core scanning logic, reused by both `discover_sources` and `discover_sources_for_context`

4. **Context propagation**: The `watched_location` parameter is passed through the call chain so that newly discovered files during runtime get the correct location

## Deviations from Plan

None - plan executed exactly as written.

## Test Results

- All new source module tests pass (when run single-threaded due to env var modifications)
- Existing tests pass
- Pre-existing test failures in app::tests::test_add_to_history* (unrelated to this work)

## Next Phase Readiness

Phase 04 is now complete. The foundation for project-local streams is established:
- Capture mode stores to project-local directories (04-01)
- Discovery mode finds sources from both locations (04-02)
- Sources are tagged with their origin location

Future work can build on this to:
- Display location indicators in the UI
- Implement project-scoped filters
- Add source management features (delete, archive)
