# ADR-016: Renderer Preset Architecture

## Status

Accepted

## Context

Log files come in many structured formats (JSON, logfmt, custom patterns). Users need to see parsed, formatted output rather than raw text. The formatting system needed to support:

- Multiple log formats within the same project
- User-customizable layouts without code changes
- Auto-detection of format per line
- Reasonable defaults out of the box

Alternatives considered:
- **Lua scripting**: Maximum flexibility, but adds a scripting runtime dependency and a learning curve. Overkill for layout definitions that are mostly "extract field X, show it in color Y with width Z."
- **Regex-only**: Simple but can't express field extraction from JSON or logfmt. Would require regex groups for every field, making definitions fragile.
- **Hardcoded formatters**: Fast to implement initially, but every new format requires a code change and rebuild.

## Decision

Use **declarative YAML preset definitions** compiled into `CompiledPreset` at startup. Each preset specifies:

- **Parser**: how to extract fields (`json`, `logfmt`, or `regex` with named capture groups)
- **Detect rules**: patterns to auto-match presets to log lines (filename globs, string/regex content matches)
- **Layout entries**: ordered list of field extractions → styled segments (color, width, alignment, conditional styling)

Presets follow a three-tier priority system:
1. **Inline config presets** (in `lazytail.yaml` `renderers:` section) — highest priority
2. **External file presets** (from `.lazytail/renderers/` or `renderers/` directories)
3. **Builtin presets** (JSON and logfmt defaults compiled into the binary) — lowest priority

Duplicate names use the highest-priority version, allowing users to override builtins.

The `PresetRegistry` holds all compiled presets. Each source can have explicit renderer names assigned via config, or presets are auto-detected per line using detect rules.

## Consequences

**Benefits:**
- Users customize log formatting via YAML without touching Rust code
- Auto-detection selects the right preset per line, supporting mixed-format logs
- Builtin presets provide immediate value for JSON and logfmt without configuration
- Priority system lets users override any builtin behavior

**Trade-offs:**
- YAML compilation at startup adds a small delay (negligible for typical preset counts)
- Declarative definitions can't express arbitrary formatting logic (no conditionals beyond `style_when`)
- Auto-detection runs per line, adding overhead for rendered lines (mitigated by caching)
