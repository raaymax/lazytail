# Phase 3: Config Loading - Research

**Researched:** 2026-02-03
**Domain:** YAML configuration parsing, error reporting, config merging
**Confidence:** HIGH

## Summary

This phase involves parsing YAML configuration files with strict validation, providing helpful error messages with typo suggestions, and merging global/project configs while keeping source groups separate. The Rust ecosystem has mature solutions for all these requirements.

The core stack is **serde-saphyr** for YAML parsing (a modern, maintained replacement for the unmaintained serde-yaml), **strsim** for "did you mean" suggestions via string similarity, and standard serde `#[serde(deny_unknown_fields)]` for strict parsing. Error formatting will use a custom approach inspired by Cargo diagnostics rather than pulling in a full diagnostic framework like miette (overkill for config errors).

**Primary recommendation:** Use serde-saphyr with `deny_unknown_fields`, implement custom error wrapper with line/column info from serde-saphyr's `Location`, and use strsim's `jaro_winkler` for typo suggestions.

## Standard Stack

The established libraries/tools for this domain:

### Core
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| serde-saphyr | 0.0.17 | YAML parsing with serde | Modern replacement for unmaintained serde-yaml, panic-free, direct struct parsing |
| serde | 1.0 | Serialization framework | Already in project, `deny_unknown_fields` for strict parsing |
| strsim | 0.11 | String similarity for typo suggestions | Provides jaro-winkler (used by clap for "did you mean") |

### Supporting
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| thiserror | 1.0 | Error type derivation | Clean error types with Display impl |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| serde-saphyr | serde_yml | Fork of serde-yaml, but serde-saphyr is more actively maintained and has better error location support |
| miette | Custom formatting | Miette is full diagnostic framework; overkill for config errors, simpler to format manually |
| figment | Manual merge | Figment adds complexity; our merge is simple (two files, separate groups) |

**Installation:**
```bash
cargo add serde-saphyr strsim thiserror
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── config/
│   ├── mod.rs           # Public API: load(), ConfigError, Config types
│   ├── discovery.rs     # Existing: find config file paths
│   ├── loader.rs        # NEW: Read and parse YAML files
│   ├── types.rs         # NEW: Config structs (ProjectConfig, GlobalConfig, Source)
│   └── error.rs         # NEW: ConfigError with formatting, suggestions
```

### Pattern 1: Layered Config with Separate Groups
**What:** Parse global and project configs separately, merge sources into distinct groups
**When to use:** When sources must remain grouped by origin (global vs project)
**Example:**
```rust
// Source: Custom pattern based on decisions
#[derive(Debug, Clone)]
pub struct Config {
    /// Project name from project config (if any)
    pub name: Option<String>,
    /// Sources from project config (lazytail.yaml)
    pub project_sources: Vec<Source>,
    /// Sources from global config (~/.config/lazytail/config.yaml)
    pub global_sources: Vec<Source>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub name: String,
    pub path: PathBuf,
}

// Merge by keeping groups separate
pub fn load(discovery: &DiscoveryResult) -> Result<Config, ConfigError> {
    let project = discovery.project_config.as_ref()
        .map(|p| load_file::<ProjectConfig>(p))
        .transpose()?;
    let global = discovery.global_config.as_ref()
        .map(|p| load_file::<GlobalConfig>(p))
        .transpose()?;

    Ok(Config {
        name: project.as_ref().and_then(|p| p.name.clone()),
        project_sources: project.map(|p| p.sources).unwrap_or_default(),
        global_sources: global.map(|g| g.sources).unwrap_or_default(),
    })
}
```

### Pattern 2: Strict Parsing with Typo Suggestions
**What:** Use `deny_unknown_fields` and catch errors to add suggestions
**When to use:** Every config struct to catch typos
**Example:**
```rust
// Source: serde docs + strsim
use strsim::jaro_winkler;

const KNOWN_FIELDS: &[&str] = &["name", "path"];
const SUGGESTION_THRESHOLD: f64 = 0.8;

fn suggest_field(unknown: &str, known: &[&str]) -> Option<&str> {
    known.iter()
        .map(|&k| (k, jaro_winkler(unknown, k)))
        .filter(|(_, score)| *score >= SUGGESTION_THRESHOLD)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap())
        .map(|(field, _)| field)
}
```

### Pattern 3: Error Wrapper with Location
**What:** Wrap serde-saphyr errors to add context and suggestions
**When to use:** All config parsing operations
**Example:**
```rust
// Source: serde-saphyr docs
use serde_saphyr::Error as YamlError;

#[derive(Debug)]
pub struct ConfigError {
    pub message: String,
    pub location: Option<Location>,
    pub suggestion: Option<String>,
    pub file_path: PathBuf,
}

impl ConfigError {
    pub fn from_yaml(err: YamlError, path: PathBuf) -> Self {
        // serde-saphyr provides location via err.location()
        // Extract unknown field name from message to generate suggestion
        Self {
            message: err.to_string(),
            location: extract_location(&err),
            suggestion: extract_unknown_field(&err.to_string())
                .and_then(|f| suggest_field(&f, KNOWN_FIELDS)),
            file_path: path,
        }
    }
}
```

### Anti-Patterns to Avoid
- **Silently ignoring unknown fields:** Always use `deny_unknown_fields` to catch typos
- **Generic error messages:** Include file path, line number, and suggestions
- **Parsing then merging sources by name:** Keep project/global sources in separate groups
- **Using figment for simple two-file merge:** Adds complexity; manual merge is simpler

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| String similarity | Custom edit distance | strsim::jaro_winkler | Tested algorithm, handles edge cases |
| YAML parsing | Custom parser | serde-saphyr | Complex format, error locations, anchor support |
| Error location extraction | Regex on error messages | serde-saphyr's Location API | Built-in support for line/column |

**Key insight:** The YAML spec is complex (anchors, multi-line strings, type inference). Never hand-roll parsing.

## Common Pitfalls

### Pitfall 1: Missing deny_unknown_fields
**What goes wrong:** Typos like `fillter` silently ignored, user never knows
**Why it happens:** Serde's default is permissive
**How to avoid:** Add `#[serde(deny_unknown_fields)]` to ALL config structs
**Warning signs:** Config changes have no effect

### Pitfall 2: Path Expansion Not Handled
**What goes wrong:** `~/logs/app.log` doesn't expand, file not found
**Why it happens:** YAML doesn't expand shell variables or tilde
**How to avoid:** Use `dirs::home_dir()` and handle `~` manually before PathBuf
**Warning signs:** Paths with `~` or `$HOME` fail

### Pitfall 3: serde-yaml vs serde-saphyr Confusion
**What goes wrong:** Using unmaintained serde-yaml instead of serde-saphyr
**Why it happens:** serde-yaml appears in older docs and has more downloads
**How to avoid:** Always use serde-saphyr; serde-yaml is unmaintained as of 2023
**Warning signs:** Import is `serde_yaml` not `serde_saphyr`

### Pitfall 4: Swallowing Parse Errors
**What goes wrong:** Invalid YAML causes silent failure, empty config
**Why it happens:** Using `.ok()` or `unwrap_or_default()` on parse results
**How to avoid:** Propagate errors to debug source; always show parse failures
**Warning signs:** Config file exists but sources don't appear

### Pitfall 5: File Existence vs Parse Errors Conflated
**What goes wrong:** "Config not found" when file exists but has syntax error
**Why it happens:** Both errors bubble up as "couldn't load config"
**How to avoid:** Check file existence first, then parse; distinct error types
**Warning signs:** User sees wrong error message for syntax errors

## Code Examples

Verified patterns from official sources:

### YAML Parsing with serde-saphyr
```rust
// Source: https://docs.rs/serde-saphyr
use serde::Deserialize;
use serde_saphyr::from_str;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    name: Option<String>,
    #[serde(default)]
    sources: Vec<Source>,
}

fn load_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, ConfigError> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| ConfigError::io(e, path.to_path_buf()))?;
    serde_saphyr::from_str(&content)
        .map_err(|e| ConfigError::from_yaml(e, path.to_path_buf()))
}
```

### String Similarity for Suggestions
```rust
// Source: https://docs.rs/strsim/0.11
use strsim::jaro_winkler;

fn suggest_similar<'a>(input: &str, candidates: &[&'a str]) -> Option<&'a str> {
    const THRESHOLD: f64 = 0.8;

    candidates.iter()
        .map(|&c| (c, jaro_winkler(input, c)))
        .filter(|(_, score)| *score >= THRESHOLD)
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(candidate, _)| candidate)
}

// Usage in error handling
if let Some(suggestion) = suggest_similar("fillter", &["filter", "follow", "name"]) {
    // suggestion = "filter"
}
```

### Cargo-Style Error Formatting
```rust
// Source: Inspired by Cargo error output
impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "error: {}", self.message)?;
        writeln!(f, "  --> {}:{}", self.file_path.display(),
            self.location.map(|l| format!("{}:{}", l.line, l.column))
                .unwrap_or_else(|| "?:?".to_string()))?;
        if let Some(ref suggestion) = self.suggestion {
            writeln!(f, "  |")?;
            writeln!(f, "  = help: did you mean `{}`?", suggestion)?;
        }
        Ok(())
    }
}
```

### Tilde Expansion for Paths
```rust
// Source: Standard pattern, dirs crate
fn expand_path(path: &Path) -> PathBuf {
    if let Ok(stripped) = path.strip_prefix("~") {
        if let Some(home) = dirs::home_dir() {
            return home.join(stripped);
        }
    }
    path.to_path_buf()
}
```

### Source Path Validation
```rust
// Source: Custom pattern per decisions
#[derive(Debug, Clone)]
pub struct ValidatedSource {
    pub name: String,
    pub path: PathBuf,
    pub exists: bool,
}

impl Source {
    pub fn validate(self) -> ValidatedSource {
        let expanded = expand_path(&self.path);
        let exists = expanded.try_exists().unwrap_or(false);
        ValidatedSource {
            name: self.name,
            path: expanded,
            exists,
        }
    }
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| serde-yaml | serde-saphyr | 2023+ | serde-yaml unmaintained; serde-saphyr actively developed |
| serde_yaml import | serde_saphyr import | 2023+ | Different crate, different import |
| Manual error parsing | Location API | serde-saphyr | Built-in line/column tracking |

**Deprecated/outdated:**
- **serde-yaml:** Unmaintained since 2023, use serde-saphyr instead
- **yaml-rust:** Low-level, no serde integration, use serde-saphyr

## Open Questions

Things that couldn't be fully resolved:

1. **Exact error message format from serde-saphyr's deny_unknown_fields**
   - What we know: Returns "unknown field `X`" style message
   - What's unclear: Exact format for extracting field name programmatically
   - Recommendation: Parse error message string to extract unknown field name for suggestions

2. **serde-saphyr's Spanned type integration with deny_unknown_fields**
   - What we know: serde-saphyr provides `Spanned<T>` for location tracking
   - What's unclear: Whether Spanned works seamlessly with deny_unknown_fields errors
   - Recommendation: Use error's Location API, not Spanned, for error location

## Sources

### Primary (HIGH confidence)
- [serde-saphyr GitHub](https://github.com/bourumir-wyngs/serde-saphyr) - Version 0.0.17, API, error handling
- [serde-saphyr docs.rs](https://docs.rs/serde-saphyr) - Location struct, from_str API
- [strsim docs.rs](https://docs.rs/strsim/0.11) - jaro_winkler algorithm
- [serde container-attrs](https://serde.rs/container-attrs.html) - deny_unknown_fields documentation

### Secondary (MEDIUM confidence)
- [rust-suggestions GitHub](https://github.com/Techcable/rust-suggestions) - Clap's suggestion algorithm approach
- [figment docs.rs](https://docs.rs/figment/latest/figment/) - Merge patterns (not using, but informed design)

### Tertiary (LOW confidence)
- [miette documentation](https://docs.rs/miette) - Diagnostic formatting inspiration (not using directly)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - serde-saphyr well-documented, strsim mature
- Architecture: HIGH - Simple patterns, existing discovery module to extend
- Pitfalls: HIGH - Common issues well-documented in serde/YAML communities

**Research date:** 2026-02-03
**Valid until:** 2026-03-03 (30 days - stable libraries)
