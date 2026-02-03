# Feature Landscape: CLI Project Configuration

**Domain:** CLI tool project-scoped configuration
**Researched:** 2026-02-03
**Confidence:** HIGH (verified against multiple authoritative sources)

## Table Stakes

Features users expect from CLI tools with project configuration. Missing these means the implementation feels incomplete or broken.

| Feature | Why Expected | Complexity | Notes |
|---------|--------------|------------|-------|
| Config file auto-discovery | All major CLI tools (Cargo, ESLint, Docker Compose) search current directory first | Low | Walk up directories until config found or root reached |
| Hierarchical precedence | Git, Cargo, npm establish the pattern: project overrides global | Low | Standard is: CLI args > env vars > project > global |
| YAML format support | Human-readable, comment-friendly, widely understood by developers | Low | YAML chosen in PROJECT.md; good choice per ecosystem research |
| Graceful missing config | Tool works without config file (uses defaults or CLI args) | Low | Never error just because config doesn't exist |
| Clear error messages | Users expect helpful feedback on syntax/validation errors | Medium | Include file path, line number, and what went wrong |
| Global config location | XDG compliance: `~/.config/lazytail/` or `$XDG_CONFIG_HOME/lazytail/` | Low | Already established in LazyTail for stream storage |
| Project config in repo root | Developers expect `lazytail.yaml` at project root, not hidden in subdirs | Low | Single canonical location simplifies discovery |
| Backwards compatibility | Existing global streams in `~/.config/lazytail/data/` must keep working | Medium | Migration path if format changes |

## Differentiators

Features that would set LazyTail apart. Not strictly expected, but valued when present.

| Feature | Value Proposition | Complexity | Notes |
|---------|-------------------|------------|-------|
| Config validation command (`lazytail config validate`) | Catch errors before runtime; integrates with CI/CD | Low | Just parse and report errors without starting TUI |
| Config introspection (`lazytail config show`) | Debug which config is active and what values are set | Low | Show merged config from all sources |
| Local override file (`lazytail.local.yaml`) | Personal overrides not committed to git (like `.env.local`) | Low | Mention in generated `.gitignore` |
| JSON Schema for editor support | Autocomplete and inline validation in VS Code/editors | Medium | Publish to SchemaStore for broad adoption |
| Environment variable overrides | CI/CD friendly, standard pattern from Cargo | Medium | `LAZYTAIL_*` prefix for config keys |
| Config init command (`lazytail init`) | Generate starter `lazytail.yaml` with comments | Low | Include common patterns and documentation |
| Named source definitions | Define log sources by name in config, open by name | Medium | Core value prop for project config |
| Source groups | Group related sources (e.g., "backend", "frontend") | Medium | Open multiple tabs with one command |
| Watch paths for auto-discovery | Config specifies directories to monitor for new logs | High | Requires directory watcher integration |

## Anti-Features

Features to explicitly NOT build. Common mistakes in this domain.

| Anti-Feature | Why Avoid | What to Do Instead |
|--------------|-----------|-------------------|
| Cascading config search (ESLint legacy style) | Complexity, confusion, ESLint deprecated this in favor of flat config | Single project root config, no directory cascading |
| Config in `package.json` or `Cargo.toml` | Coupling to specific ecosystems, parsing complexity | Dedicated `lazytail.yaml` file |
| Global config that affects project behavior silently | Unexpected behavior when running in different environments | Clear precedence, show effective config |
| Auto-creating config files | Pollutes projects, confusing for users | Explicit `lazytail init` command only |
| Complex merging semantics | Hard to reason about final config state | Replace strategy (project replaces global) except for additive arrays |
| Config file in hidden directory (`.lazytail/config.yaml`) | Separates data dir from config file location, confusing | Config at root (`lazytail.yaml`), data in `.lazytail/` |
| Multiple config formats (YAML + TOML + JSON) | Maintenance burden, inconsistent behavior | YAML only for config files |
| Mandatory config file | Tool should work without any config | All config optional, sensible defaults |
| Storing secrets in config file | Security risk, git exposure | Use environment variables for sensitive data |
| Silent config errors | Users don't know why tool behaves unexpectedly | Fail fast with clear error message on parse failure |

## Feature Dependencies

```
Config Discovery (must have first)
    |
    v
Config Parsing (YAML + validation)
    |
    +---> Source Definitions (depends on parsing)
    |         |
    |         +---> Source Groups (depends on source definitions)
    |         |
    |         +---> Watch Paths (depends on source definitions)
    |
    +---> Local Override File (depends on parsing + discovery)
    |
    +---> Environment Overrides (depends on parsing)

Config Commands (independent, but need parsing)
    |
    +---> lazytail config validate
    +---> lazytail config show
    +---> lazytail init
```

**Key ordering:**
1. Config discovery must come before any config-dependent features
2. Source definitions are the core value; everything else builds on them
3. Config commands can be added incrementally after basic parsing works

## Config Content Separation

Based on research, here's what belongs where:

### Project Config (`lazytail.yaml`)

Content that should be version-controlled and shared across team:

```yaml
# Log source definitions
sources:
  backend:
    path: logs/backend.log
    filter: "level:error"  # optional default filter

  frontend:
    path: logs/frontend.log

  docker:
    command: "docker logs -f app"  # stream from command

# Source groups
groups:
  all: [backend, frontend, docker]

# Watch directories for new logs
watch:
  - logs/
  - /var/log/myapp/
```

### Local Override (`lazytail.local.yaml`)

Content that varies per developer machine:

```yaml
# Personal preferences that override project config
sources:
  backend:
    path: /custom/path/backend.log  # different path on this machine

# Additional local-only sources not in project config
sources:
  debug:
    path: ~/debug.log
```

### Global Config (`~/.config/lazytail/config.yaml`)

User-wide defaults:

```yaml
# UI preferences
ui:
  theme: dark
  scrolloff: 5

# Default behaviors
defaults:
  follow: true
  case_sensitive: false
```

### Environment Variables

For CI/CD and sensitive data:

```bash
LAZYTAIL_SOURCES_BACKEND_PATH=/ci/logs/backend.log
LAZYTAIL_DEFAULT_FILTER="level:error"
```

## MVP Recommendation

For MVP (first milestone), prioritize:

1. **Config file auto-discovery** - Find `lazytail.yaml` in current or parent directories
2. **YAML parsing with validation** - Parse config, fail gracefully with clear errors
3. **Named source definitions** - Core value: define sources by name, open with `lazytail open backend`
4. **Hierarchical precedence** - Project config overrides global defaults
5. **Graceful degradation** - Tool works without any config file

Defer to post-MVP:

- **`lazytail init` command**: Nice to have, not blocking adoption
- **Local override files**: Add after basic config works
- **Source groups**: Enhancement after single sources work
- **Watch paths**: Complex feature, save for later
- **JSON Schema**: Polish feature, not core functionality
- **Environment variable overrides**: Add when CI/CD use cases emerge

## Sources

**High Confidence (Official Documentation):**
- [Cargo Configuration Reference](https://doc.rust-lang.org/cargo/reference/config.html) - Hierarchical config, environment overrides, precedence rules
- [Docker Compose Documentation](https://docs.docker.com/compose/intro/compose-application-model/) - File discovery, naming conventions
- [XDG Base Directory Specification](https://specifications.freedesktop.org/basedir/latest/) - Standard config locations
- [ESLint Flat Config Migration](https://eslint.org/docs/latest/use/configure/migration-guide) - Why cascading config was deprecated
- [Git Config Documentation](https://git-scm.com/docs/git-config) - Three-tier config model (system/global/local)

**Medium Confidence (Verified with Multiple Sources):**
- [Configuration Format Comparison](https://schoenwald.aero/posts/2025-05-03_configuration-format-comparison/) - YAML vs TOML vs JSON tradeoffs
- [CLI Best Practices](https://hackmd.io/@arturtamborski/cli-best-practices) - Config layering patterns
- [Command Line Interface Guidelines](https://clig.dev/) - Error message best practices
- [Prettier Configuration](https://prettier.io/docs/configuration) - Intentional lack of global config for consistency

**Low Confidence (WebSearch only, needs validation):**
- None - all critical claims verified with official sources

---

*Research conducted: 2026-02-03*
