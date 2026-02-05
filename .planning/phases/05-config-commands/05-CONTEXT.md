# Phase 5: Config Commands - Context

**Gathered:** 2026-02-04
**Status:** Ready for planning

<domain>
## Phase Boundary

Developer experience commands for config initialization, validation, and introspection. Three commands: `lazytail init`, `lazytail config validate`, and `lazytail config show`. These are CLI utilities that don't start the TUI viewer.

**Important scope change:** Config loading does NOT merge project and global configs. Closest config wins completely (project config if exists, else global config). This affects how `config show` displays results.

</domain>

<decisions>
## Implementation Decisions

### Init command behavior
- Creates minimal lazytail.yaml with commented examples for sources
- Project name auto-detected from current directory name
- Requires `--force` flag to overwrite existing config (no interactive prompt)
- Also creates `.lazytail/` data directory alongside the config file
- Only operates in current directory (no path argument)

### Output format & style
- `config show` displays path at top ("Using: /path/to/lazytail.yaml") then content
- Colored output for keys, values, errors (respects NO_COLOR environment variable)
- `config validate` is quiet on success (exit 0, no output)
- Validation errors go to stderr with exit 1

### Command structure
- `init` is top-level: `lazytail init`
- validate/show are under config: `lazytail config validate`, `lazytail config show`
- No aliases (no `cfg` shorthand)
- Commands use auto-discovery only (no `--config` flag to specify path)

### Error & edge cases
- `config show` with no config: show default configuration values
- `config validate` with no config: exit 1 with "No config found to validate"
- Validate reports errors only (no warnings for unused keys)
- Validate checks that source files referenced in config actually exist

### Claude's Discretion
- Exact YAML comment style and template wording
- Error message formatting
- Color choices for output

</decisions>

<specifics>
## Specific Ideas

- "Closest config wins" — no merging between project and global config
- Should feel like standard CLI tools (git init, npm init patterns)
- Quiet success for validate enables use in CI pipelines

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 05-config-commands*
*Context gathered: 2026-02-04*
