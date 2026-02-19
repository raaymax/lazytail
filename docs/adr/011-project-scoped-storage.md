# ADR-011: Project-Scoped vs Global Source Storage

## Status

Accepted

## Context

When using capture mode (`cmd | lazytail -n NAME`), the log data and marker files need to be stored somewhere. Different projects may have sources with the same name (e.g., "api", "worker"), and users may want project-specific sources isolated from global ones.

Options considered:
1. **Global only** - all sources in `~/.config/lazytail/data/`
2. **Project only** - all sources in `.lazytail/data/` (requires project config)
3. **Two-tier** - project-local when in a project, global otherwise

## Decision

We use a **two-tier storage model** determined by the config discovery context:

**Inside a project** (directory tree containing `lazytail.yaml`):
- Data: `<project_root>/.lazytail/data/NAME.log`
- Markers: `<project_root>/.lazytail/sources/NAME`

**Outside a project**:
- Data: `~/.config/lazytail/data/NAME.log`
- Markers: `~/.config/lazytail/sources/NAME`

Discovery mode shows both tiers, with project sources appearing first. **Project sources shadow global sources** with the same name (if "api" exists in both, only the project-local version is shown).

Directories are created with `mode 0700` on Unix for security (source files may contain sensitive log data).

The `DiscoveryResult` struct is threaded through all source operations to ensure consistent context:
- `resolve_data_dir(discovery)` - where to store/find log files
- `resolve_sources_dir(discovery)` - where to store/find markers
- `create_marker_for_context(name, discovery)` - create marker in correct tier
- `resolve_source_for_context(name, discovery)` - find source, project first

## Consequences

**Benefits:**
- Projects are isolated: `cd project-a && lazytail` shows only project-a sources
- Global sources are always available as fallback
- Shadowing prevents name collisions between projects
- `.lazytail/` can be gitignored per project

**Trade-offs:**
- Users must understand the two-tier model (mitigated by the source panel showing categories)
- `.lazytail/` directory appears in the project root (should be gitignored)
- Source deletion only works for sources in the global data directory (safety check in `delete_source()`)
