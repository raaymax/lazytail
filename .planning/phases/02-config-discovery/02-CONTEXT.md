# Phase 2: Config Discovery - Context

**Gathered:** 2026-02-03
**Status:** Ready for planning

<domain>
## Phase Boundary

Find project root and config files by walking the directory tree upward. Return discovered paths for config loading (Phase 3). Discovery determines WHERE configs are, not WHAT they contain.

</domain>

<decisions>
## Implementation Decisions

### Discovery Markers
- Only `lazytail.yaml` signals project root (not `.lazytail/` directory)
- Single canonical filename: `lazytail.yaml` only (not `.yml` variants or hidden files)
- No recognition of other project markers like `.git/` — only explicit lazytail config matters
- When `lazytail.yaml` is found, `.lazytail/` data directory auto-created on first capture (Phase 4 implements storage, but discovery establishes the location)

### Search Boundaries
- Search upward from cwd until filesystem root (`/`)
- No depth limit — traverse until root or config found
- No special handling for containers/chroot — works naturally with container's `/`
- Canonicalize paths first (resolve symlinks), then walk real directories

### Multi-Config Handling
- Closest config wins — first `lazytail.yaml` found walking up from cwd
- Global config exists at `~/.config/lazytail/config.yaml` as base layer
- Precedence (highest to lowest): CLI args > project config > global config > defaults
- Shallow merge: top-level keys override entirely (no deep merging of nested structures)

### Fallback Behavior
- No config found = use defaults silently (works out of the box)
- No `--no-config` flag — discovery always runs
- No `--config PATH` flag — config must be in discoverable locations
- Verbose mode (`-v`) shows discovery search path for debugging

### Claude's Discretion
- Internal data structures for discovery results
- Error handling for permission-denied directories during walk
- Caching strategy if discovery is called multiple times

</decisions>

<specifics>
## Specific Ideas

- Discovery should feel instant — users won't notice it happening
- Keep the public API simple: "give me config paths" — caller doesn't need to know traversal details

</specifics>

<deferred>
## Deferred Ideas

None — discussion stayed within phase scope

</deferred>

---

*Phase: 02-config-discovery*
*Context gathered: 2026-02-03*
