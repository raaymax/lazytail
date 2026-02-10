# LazyTail

## What This Is

LazyTail is a terminal-based log viewer for developers who work with multiple log streams. It provides multi-tab viewing, vim-style navigation, real-time filtering, and an MCP server for AI-assisted log analysis.

## Core Value

Fast, keyboard-driven log exploration across multiple sources with live updates.

## Requirements

### Validated

- ✓ Multi-tab log viewing with independent state per tab — existing
- ✓ Vim-style navigation (hjkl, gg/G, Ctrl+d/u, search) — existing
- ✓ Real-time filtering with string, regex, and query syntax — existing
- ✓ Background filtering with progress indication — existing
- ✓ File watching for live log updates — existing
- ✓ Stdin/pipe streaming support — existing
- ✓ Line expansion for long lines — existing
- ✓ Global stream capture (`cmd | lazytail -n "name"`) stored in ~/.config/lazytail/data/ — existing
- ✓ MCP server for AI tool integration — existing
- ✓ Source discovery mode to browse captured streams — existing

### Active

- [ ] Stream cleanup on exit/SIGTERM — stream files should be cleaned up so names can be reused
- [ ] Project-scoped log configuration — `lazytail.yaml` config file defines project sources
- [ ] Project-local stream storage — `.lazytail/` folder for project-specific streams
- [ ] Dual storage support — both global (~/.config/lazytail/) and project-local (.lazytail/) coexist

### Out of Scope

- GUI interface — terminal-first tool
- Log aggregation/shipping — this is a viewer, not a collector
- Remote log access — local files and streams only

## Context

LazyTail is a brownfield project with an established architecture:
- Event-driven main loop (render-collect-process)
- Trait-based reader abstraction (`LogReader`) for files and streams
- Channel-based communication for background filtering
- Existing stream capture stores data in `~/.config/lazytail/data/`

Current stream handling in `src/main.rs` creates files but doesn't clean them up on exit. Signal handling exists via `signal-hook` crate but isn't connected to stream cleanup.

## Constraints

- **Tech stack**: Rust with ratatui/crossterm — established, no changes
- **Config format**: YAML for lazytail.yaml — human-readable, familiar to developers
- **Backwards compatibility**: Global streams in ~/.config/lazytail/ must continue working

## Key Decisions

| Decision | Rationale | Outcome |
|----------|-----------|---------|
| YAML for project config | Human-readable, familiar format | — Pending |
| .lazytail/ for project streams | Mirrors .git/ pattern, keeps project state local | — Pending |
| Both global and project storage | Flexibility for different use cases | — Pending |

---
*Last updated: 2026-02-03 after initialization*
