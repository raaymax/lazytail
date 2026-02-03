# Requirements

**Project:** LazyTail — Project Configuration & Stream Cleanup
**Version:** v1
**Created:** 2026-02-03

## v1 Requirements

### Signal Handling

- [ ] **SIG-01**: Graceful shutdown on SIGINT/SIGTERM cleans up stream markers before exit
- [ ] **SIG-02**: Fix capture.rs signal handler — remove process::exit(), let main thread handle cleanup
- [ ] **SIG-03**: Stale marker detection on startup — recover from SIGKILL by checking if PID still running
- [ ] **SIG-04**: Double Ctrl+C support — first triggers graceful shutdown, second forces immediate exit

### Config Discovery

- [ ] **DISC-01**: Project root discovery — walk up directories looking for lazytail.yaml or .lazytail/
- [ ] **DISC-02**: Graceful missing config — tool works without config file using defaults
- [ ] **DISC-03**: Filesystem boundary checks — stop at root and $HOME to prevent slow traversal

### Config Loading

- [ ] **LOAD-01**: YAML format support — parse lazytail.yaml using serde-saphyr
- [ ] **LOAD-02**: Hierarchical precedence — CLI args override project config override global config
- [ ] **LOAD-03**: Clear error messages — show file path and line number on parse errors

### Config Options

- [ ] **OPT-01**: `name` option — project display name shown in UI
- [ ] **OPT-02**: `sources` option — file-based source definitions with paths/globs
- [ ] **OPT-03**: `follow` option — default auto-follow mode for new tabs
- [ ] **OPT-04**: `filter` option — default filter pattern applied on startup
- [ ] **OPT-05**: `streams_dir` option — custom location for project streams (default: .lazytail/)

### Project-Local Streams

- [ ] **PROJ-01**: .lazytail/ directory — create project-local directory for stream storage
- [ ] **PROJ-02**: Context-aware capture — `lazytail -n` writes to project .lazytail/ when in project, global otherwise

### Config Commands

- [ ] **CMD-01**: `lazytail init` — generate starter lazytail.yaml with comments
- [ ] **CMD-02**: `lazytail config validate` — parse config and report errors without running
- [ ] **CMD-03**: `lazytail config show` — display effective merged config

## v2 Requirements (Deferred)

- Local override file (lazytail.local.yaml) for personal settings not committed to git
- Environment variable overrides (LAZYTAIL_* prefix)
- Command-based sources in config (capture output of commands)
- Source groups (open multiple related sources with one command)
- JSON Schema for editor autocomplete support

## Out of Scope

- GUI interface — terminal-first tool
- Remote log access — local files and streams only
- TOML config format — YAML chosen for nested structures and multi-line strings
- Wrap option in config — line expansion handles long lines

## Traceability

| Requirement | Phase | Status |
|-------------|-------|--------|
| SIG-01 | — | pending |
| SIG-02 | — | pending |
| SIG-03 | — | pending |
| SIG-04 | — | pending |
| DISC-01 | — | pending |
| DISC-02 | — | pending |
| DISC-03 | — | pending |
| LOAD-01 | — | pending |
| LOAD-02 | — | pending |
| LOAD-03 | — | pending |
| OPT-01 | — | pending |
| OPT-02 | — | pending |
| OPT-03 | — | pending |
| OPT-04 | — | pending |
| OPT-05 | — | pending |
| PROJ-01 | — | pending |
| PROJ-02 | — | pending |
| CMD-01 | — | pending |
| CMD-02 | — | pending |
| CMD-03 | — | pending |

---
*Last updated: 2026-02-03*
