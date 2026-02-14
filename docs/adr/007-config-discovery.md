# ADR-007: Config Discovery via Parent Directory Walk

## Status

Accepted

## Context

LazyTail needs to find its configuration to determine which sources to show and where to store captured data. The tool must work both in project-specific contexts (monorepo, service directory) and globally.

Options considered:
1. **Fixed path only** - always `~/.config/lazytail/config.yaml`
2. **Parent directory walk** - search upward for `lazytail.yaml` (like `.gitignore`)
3. **Environment variable** - `LAZYTAIL_CONFIG=/path/to/config.yaml`
4. **CLI flag only** - `--config /path/to/config.yaml`

## Decision

We use a **two-tier config system** with parent directory walk:

1. **Project config**: walk from CWD upward looking for `lazytail.yaml`. The directory containing it becomes the "project root".
2. **Global config**: check `~/.config/lazytail/config.yaml`.

Both are loaded and merged. Project sources and global sources appear in separate categories in the UI.

The discovery also determines **data storage location**:
- In a project: `.lazytail/data/` and `.lazytail/sources/` next to `lazytail.yaml`
- Outside a project: `~/.config/lazytail/data/` and `~/.config/lazytail/sources/`

On macOS, we always use `~/.config/` instead of `~/Library/Application Support/` for consistency with other CLI tools.

The `DiscoveryResult` struct carries both paths and is passed to capture mode, source resolution, and MCP tools so they all use the same context.

## Consequences

**Benefits:**
- Works intuitively: `cd` into a project and `lazytail` finds its config
- Supports both project-scoped and global sources simultaneously
- Project sources shadow global sources with the same name (least surprise)
- `lazytail init` can create the config in the current directory

**Trade-offs:**
- Directory walk adds startup latency (negligible: walking parent dirs is fast)
- Two config files to maintain for users who want both project and global sources
- The `.lazytail/` directory should be added to `.gitignore` (contains local data)
