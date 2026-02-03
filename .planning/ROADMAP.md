# Roadmap: LazyTail Project Configuration & Stream Cleanup

## Overview

This roadmap delivers project-scoped configuration and robust signal handling to LazyTail. The journey starts with signal infrastructure (foundational for all cleanup), builds configuration discovery and loading, integrates config into the application, extends to project-local streams, and finishes with developer experience commands. Each phase builds on the previous, following the dependency chain identified in research.

## Phases

**Phase Numbering:**
- Integer phases (1, 2, 3): Planned milestone work
- Decimal phases (2.1, 2.2): Urgent insertions (marked with INSERTED)

Decimal phases appear between their surrounding integers in numeric order.

- [x] **Phase 1: Signal Infrastructure** - Robust signal handling with cleanup coordination
- [x] **Phase 2: Config Discovery** - Find project root and config files
- [ ] **Phase 3: Config Loading** - Parse YAML and merge configuration layers
- [ ] **Phase 4: Project-Local Streams** - .lazytail/ directory with context-aware capture
- [ ] **Phase 5: Config Commands** - Init, validate, and show commands for developer experience

## Phase Details

### Phase 1: Signal Infrastructure
**Goal**: Application handles termination signals gracefully with proper cleanup
**Depends on**: Nothing (first phase)
**Requirements**: SIG-01, SIG-02, SIG-03, SIG-04
**Success Criteria** (what must be TRUE):
  1. Running `lazytail -n test` then pressing Ctrl+C cleans up stream markers before exit
  2. Running `lazytail -n test` then sending SIGTERM cleans up stream markers before exit
  3. After SIGKILL (kill -9), restarting lazytail detects and cleans stale markers
  4. Double Ctrl+C forces immediate exit without hanging
  5. capture.rs signal handler does not call process::exit() directly
**Plans:** 1 plan

Plans:
- [x] 01-01-PLAN.md — Signal module, capture.rs refactor, stale marker cleanup

### Phase 2: Config Discovery
**Goal**: Application finds project root and config files by walking directory tree
**Depends on**: Phase 1
**Requirements**: DISC-01, DISC-02, DISC-03
**Success Criteria** (what must be TRUE):
  1. Running lazytail in a subdirectory finds lazytail.yaml in parent directories
  2. Running lazytail in a directory with lazytail.yaml recognizes it as project root
  3. Running lazytail without any config file works normally using defaults
  4. Discovery stops at filesystem root (/)
**Plans:** 1 plan

Plans:
- [x] 02-01-PLAN.md — Config discovery module with verbose output integration

### Phase 3: Config Loading
**Goal**: Application parses YAML config and merges multiple configuration sources with clear precedence
**Depends on**: Phase 2
**Requirements**: LOAD-01, LOAD-02, LOAD-03, OPT-01, OPT-02
**Note**: OPT-03, OPT-04, OPT-05 (follow, filter, streams_dir) deferred per CONTEXT.md - only `name` and `sources` for now
**Success Criteria** (what must be TRUE):
  1. lazytail.yaml with `name` and `sources` options parses correctly
  2. Project config overrides global config for name; sources kept in separate groups
  3. Parse errors show file path, line number, and "did you mean" suggestions
  4. Named sources from config appear in side panel
  5. Sources with missing files shown grayed out/disabled
**Plans:** 2 plans

Plans:
- [ ] 03-01-PLAN.md — Config loading infrastructure (types, parser, errors)
- [ ] 03-02-PLAN.md — Integration with main.rs and UI (source categories)

### Phase 4: Project-Local Streams
**Goal**: Streams captured within a project are stored locally in .lazytail/ directory
**Depends on**: Phase 3
**Requirements**: PROJ-01, PROJ-02
**Success Criteria** (what must be TRUE):
  1. Running `lazytail -n test` inside a project creates stream in .lazytail/data/
  2. Running `lazytail -n test` outside any project creates stream in ~/.config/lazytail/data/
  3. Discovery mode shows both project-local and global streams appropriately
  4. .lazytail/ directory created with secure permissions (mode 0700)
**Plans**: TBD

Plans:
- [ ] 04-01: TBD during planning

### Phase 5: Config Commands
**Goal**: Developer experience commands for config initialization, validation, and introspection
**Depends on**: Phase 3
**Requirements**: CMD-01, CMD-02, CMD-03
**Success Criteria** (what must be TRUE):
  1. `lazytail init` creates starter lazytail.yaml with helpful comments
  2. `lazytail config validate` reports config errors without starting the viewer
  3. `lazytail config show` displays effective merged configuration
  4. Init refuses to overwrite existing config without confirmation
**Plans**: TBD

Plans:
- [ ] 05-01: TBD during planning

## Progress

**Execution Order:**
Phases execute in numeric order: 1 -> 2 -> 3 -> 4 -> 5

| Phase | Plans Complete | Status | Completed |
|-------|----------------|--------|-----------|
| 1. Signal Infrastructure | 1/1 | Complete | 2026-02-03 |
| 2. Config Discovery | 1/1 | Complete | 2026-02-03 |
| 3. Config Loading | 0/2 | In Progress | - |
| 4. Project-Local Streams | 0/? | Not started | - |
| 5. Config Commands | 0/? | Not started | - |
