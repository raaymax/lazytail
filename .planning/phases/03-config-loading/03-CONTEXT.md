# Phase 3: Config Loading - Context

**Gathered:** 2026-02-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Parse YAML config files and merge configuration sources. Config contains project name and named sources (name + file path). Global and project configs merge with sources kept in separate groups. Errors display in a debug log source within the viewer.

</domain>

<decisions>
## Implementation Decisions

### Config file structure
- Sources defined as list with names:
  ```yaml
  sources:
    - name: api
      path: /var/log/api.log
  ```
- Only two top-level concerns for now: `name` (project name) and `sources`
- No per-source options (filter, follow, etc.) — just name and path
- Strict parsing: unknown keys cause errors (catches typos like 'fillter')

### Merge precedence
- Global (~/.config/lazytail/config.yaml) and project (lazytail.yaml) configs both load
- Sources from both configs merge, kept in **separate groups** (no name collisions possible)
- Project name comes from project config if present
- No CLI-to-config overrides needed (config only has name and sources)

### Source groups display
- UI shows separate sections: "Project Sources" and "Global Sources"
- Sources grouped visually, not prefixed

### Error handling
- On any config error: open viewer with debug mode forced
- Errors appear in a **debug log source** visible in the viewer
- Error format should be expandable to Cargo-style rendering in the viewer (line numbers, context, colors)
- "Did you mean" suggestions for typos on unknown fields
- Missing source paths: warn at load, disable source in panel (grayed out, can't select)

### Named sources behavior
- Sources selected from UI side panel, not CLI arguments
- MCP can open sources by name
- Config sources always shown in side panel on startup
- Sources with missing files shown disabled/grayed out
- File size shown in stats panel, not in source list

### Debug log source
- LazyTail's own debug/error output goes to a dedicated log source
- Visible with `--debug` flag (or forced on config errors)
- Provides immediate visibility into parse errors and runtime issues

</decisions>

<specifics>
## Specific Ideas

- Cargo-style error formatting as inspiration (colored, with line indicators, "did you mean" suggestions)
- Viewer always opens, even on errors — debug source shows what went wrong
- Separate sections for project vs global sources in side panel (like grouped tabs)

</specifics>

<deferred>
## Deferred Ideas

- Per-source options (filter, follow, parser settings) — future enhancement
- Grouped config structure (project:, defaults: sections) — revisit if config grows
- CLI argument to open named sources directly (`lazytail api`) — could be added later

</deferred>

---

*Phase: 03-config-loading*
*Context gathered: 2026-02-03*
