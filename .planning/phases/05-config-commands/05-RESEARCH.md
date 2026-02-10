# Phase 5: Config Commands - Research

**Researched:** 2026-02-05
**Domain:** CLI subcommand architecture, YAML generation, colored output
**Confidence:** HIGH

## Summary

This phase adds three developer experience commands to lazytail: `lazytail init`, `lazytail config validate`, and `lazytail config show`. These are non-TUI utilities following the established patterns of tools like `git init` and `npm init`.

The research confirms the existing codebase provides a solid foundation. The config module already handles discovery, loading, and validation with rich error messages. The main architectural work is restructuring `main.rs` to handle subcommands alongside the default file-viewing mode, and creating a new `cmd/` module for command implementations.

**Primary recommendation:** Use clap's derive API with nested subcommands. Keep the default behavior (no subcommand = view files/discovery mode) and add `init` as top-level with `config` containing `validate` and `show` subcommands.

## Standard Stack

### Core (Already in Cargo.toml)
| Library | Version | Purpose | Why Standard |
|---------|---------|---------|--------------|
| clap | 4.5 | CLI argument parsing | Already used, derive API supports subcommands |
| serde-saphyr | 0.0 | YAML parsing | Already used for config loading |
| dirs | 5.0 | Platform config paths | Already used for global config |
| anyhow | 1.0 | Error handling | Already used throughout codebase |
| thiserror | 2.0 | Error types | Already used in config/error.rs |

### New Dependency
| Library | Version | Purpose | When to Use |
|---------|---------|---------|-------------|
| colored | 2.7+ | Terminal coloring | Output for keys/values in `config show`, errors in validation |

### Alternatives Considered
| Instead of | Could Use | Tradeoff |
|------------|-----------|----------|
| colored | termcolor | termcolor has Windows console support, but colored is simpler and project already uses crossterm for TUI which handles Windows |
| colored | owo-colors | owo-colors is faster/smaller, but colored is more established and has built-in NO_COLOR support |

**Installation:**
```bash
cargo add colored
```

## Architecture Patterns

### Recommended Project Structure
```
src/
├── cmd/                     # NEW: CLI command implementations
│   ├── mod.rs               # Subcommand dispatch
│   ├── init.rs              # lazytail init
│   └── config.rs            # lazytail config {validate,show}
├── config/                  # EXISTING: Config parsing and discovery
│   ├── mod.rs               # Exports
│   ├── discovery.rs         # Config file discovery (unchanged)
│   ├── loader.rs            # YAML loading (unchanged)
│   ├── error.rs             # Error types (unchanged)
│   └── types.rs             # Config structs (unchanged)
└── main.rs                  # MODIFIED: Add subcommand handling
```

### Pattern 1: Clap Nested Subcommands with Default Behavior

The key challenge is maintaining backward compatibility: `lazytail file.log` must work while adding `lazytail init` and `lazytail config show`.

**What:** Use an optional subcommand enum that allows fallthrough to existing behavior
**When to use:** When adding subcommands to an existing CLI that has default file-based behavior

**Example:**
```rust
// Source: clap derive tutorial and Rain's Rust CLI recommendations
use clap::{Parser, Subcommand, Args};

#[derive(Parser)]
#[command(name = "lazytail")]
#[command(version, about)]
struct Cli {
    /// Optional subcommand
    #[command(subcommand)]
    command: Option<Commands>,

    // Existing Args fields (files, --no-watch, -n, etc.)
    // These only apply when no subcommand is given
    #[arg(value_name = "FILE")]
    files: Vec<PathBuf>,

    #[arg(long = "no-watch")]
    no_watch: bool,
    // ... other existing args
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new lazytail.yaml config file
    Init(InitArgs),

    /// Config introspection commands
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
}

#[derive(Args)]
struct InitArgs {
    /// Overwrite existing config file
    #[arg(long)]
    force: bool,
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Validate the config file
    Validate,
    /// Show effective configuration
    Show,
}
```

### Pattern 2: Closest Config Wins (No Merging)

**What:** When discovering config, project config takes complete precedence over global
**When to use:** Per user decision - simpler mental model than merging

**Example:**
```rust
// In discovery or loading - return the "winning" config
pub fn discover_effective_config() -> Option<PathBuf> {
    let discovery = config::discover();

    // Project config wins completely if it exists
    if discovery.project_config.is_some() {
        discovery.project_config
    } else {
        discovery.global_config
    }
}
```

### Pattern 3: Quiet Success for CI

**What:** Validation command produces no output on success, only sets exit code
**When to use:** For commands used in CI/CD pipelines

**Example:**
```rust
// Source: Standard Unix convention
pub fn run_validate() -> Result<(), i32> {
    let discovery = config::discover();

    // No config is an error
    let config_path = match discover_effective_config(&discovery) {
        Some(path) => path,
        None => {
            eprintln!("error: No config found to validate");
            return Err(1);
        }
    };

    // Try loading - errors are reported by ConfigError
    match config::load_file(&config_path) {
        Ok(_config) => {
            // Quiet success - just exit 0
            Ok(())
        }
        Err(e) => {
            eprintln!("{}", e);
            Err(1)
        }
    }
}
```

### Pattern 4: Colored Output with NO_COLOR Respect

**What:** Use colored crate for pretty output, automatically respect NO_COLOR
**When to use:** For `config show` and error messages

**Example:**
```rust
// Source: colored crate documentation
use colored::Colorize;

pub fn show_config(path: &Path, config: &Config) {
    // Header - shows which config is active
    println!("Using: {}", path.display().to_string().dimmed());
    println!();

    // Key-value display
    if let Some(name) = &config.name {
        println!("{}: {}", "name".cyan(), name.green());
    }

    if !config.sources.is_empty() {
        println!("{}:", "sources".cyan());
        for source in &config.sources {
            println!("  - {}: {}",
                "name".blue(),
                source.name.green());
            println!("    {}: {}",
                "path".blue(),
                source.path.display().to_string().yellow());
        }
    }
}
```

### Anti-Patterns to Avoid

- **Interactive prompts in init:** User decided `--force` flag instead of prompts. Avoids stdin handling complexity.
- **Merging configs:** User explicitly decided "closest wins" - don't add merge logic.
- **Custom color handling:** Don't check `NO_COLOR` manually - colored crate handles it.
- **Verbose validation success:** Don't print "Config is valid!" - quiet success is the Unix way.

## Don't Hand-Roll

Problems that look simple but have existing solutions:

| Problem | Don't Build | Use Instead | Why |
|---------|-------------|-------------|-----|
| NO_COLOR handling | Manual env var check | colored crate | Handles CLICOLOR_FORCE, NO_COLOR, CLICOLOR precedence correctly |
| YAML generation with comments | String concatenation | Literal string with `include_str!` or inline const | Comments aren't part of YAML data model; simpler to use template |
| Subcommand parsing | Match on first arg | clap subcommand derive | Handles help, version, errors consistently |
| Tilde expansion | Custom regex | Existing `loader::expand_path()` | Already implemented in codebase |
| Error formatting | println! to stderr | Existing `ConfigError::format_cargo_style()` | Already implemented with Cargo-like format |

**Key insight:** The config module already handles most of the heavy lifting. The commands are thin wrappers that orchestrate discovery, loading, and output formatting.

## Common Pitfalls

### Pitfall 1: Breaking Existing CLI Behavior
**What goes wrong:** Adding subcommands breaks `lazytail file.log` usage
**Why it happens:** clap requires subcommand when defined
**How to avoid:** Use `Option<Commands>` - when None, fall through to existing file-viewing logic
**Warning signs:** Integration tests for file viewing start failing

### Pitfall 2: Confusing `--config` Flag Desire
**What goes wrong:** Implementing `--config` flag to specify config path
**Why it happens:** Natural assumption from other CLI tools
**How to avoid:** User decided against this - only auto-discovery is supported
**Warning signs:** Adding path arguments to validate/show subcommands

### Pitfall 3: Config Show Without Config
**What goes wrong:** Showing an error when no config exists
**Why it happens:** Treating absence of config as error
**How to avoid:** Per user decision - show defaults when no config exists
**Warning signs:** `config show` returning non-zero exit code when no config

### Pitfall 4: Init in Wrong Directory Pattern
**What goes wrong:** Creating config file in unexpected location
**Why it happens:** Not using current working directory consistently
**How to avoid:** `init` always creates in `$PWD`, not in discovered project root
**Warning signs:** Init creating file in parent directory that has existing config

### Pitfall 5: Not Validating Source Existence
**What goes wrong:** Validation passes but sources don't exist
**Why it happens:** Only validating YAML syntax, not semantic validity
**How to avoid:** User decided validate should check source file existence
**Warning signs:** Config passes validation but lazytail viewer shows missing sources

## Code Examples

Verified patterns from official sources and codebase:

### Config File Template (for init)
```yaml
# lazytail.yaml - Log viewer configuration
# Generated by: lazytail init

# Project name (optional, defaults to directory name)
name: {project_name}

# Log sources to display in the viewer
# sources:
#   - name: api           # Display name shown in tabs
#     path: /var/log/api.log
#   - name: worker
#     path: ~/logs/worker.log
```

### YAML Writing (No Serde for Comments)
```rust
// YAML comments aren't in the data model - write as template string
fn write_config_template(project_name: &str) -> String {
    format!(
        r#"# lazytail.yaml - Log viewer configuration
# Generated by: lazytail init

# Project name (optional, defaults to directory name)
name: {project_name}

# Log sources to display in the viewer
# sources:
#   - name: api           # Display name shown in tabs
#     path: /var/log/api.log
#   - name: worker
#     path: ~/logs/worker.log
"#,
        project_name = project_name
    )
}
```

### Init Command Implementation
```rust
// Source: Codebase patterns from capture.rs and source.rs
use std::fs;
use std::path::PathBuf;
use crate::config::discovery::{PROJECT_CONFIG_NAME, DATA_DIR_NAME};
use crate::source::create_secure_dir;

pub fn run_init(force: bool) -> Result<(), i32> {
    let cwd = std::env::current_dir().map_err(|e| {
        eprintln!("error: Cannot determine current directory: {}", e);
        1
    })?;

    let config_path = cwd.join(PROJECT_CONFIG_NAME);
    let data_dir = cwd.join(DATA_DIR_NAME);

    // Check for existing config
    if config_path.exists() && !force {
        eprintln!("error: {} already exists", config_path.display());
        eprintln!("hint: Use --force to overwrite");
        return Err(1);
    }

    // Auto-detect project name from directory
    let project_name = cwd
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "my-project".to_string());

    // Write config file
    let content = write_config_template(&project_name);
    fs::write(&config_path, content).map_err(|e| {
        eprintln!("error: Failed to write config: {}", e);
        1
    })?;

    // Create .lazytail/ data directory
    create_secure_dir(&data_dir).map_err(|e| {
        eprintln!("error: Failed to create data directory: {}", e);
        1
    })?;

    println!("Created {} and {}/",
        PROJECT_CONFIG_NAME,
        DATA_DIR_NAME);

    Ok(())
}
```

### Main.rs Subcommand Dispatch
```rust
// In main(), after parsing Args
fn main() -> Result<()> {
    let cli = Cli::parse();

    // Handle subcommands first
    if let Some(command) = cli.command {
        return match command {
            Commands::Init(args) => {
                cmd::init::run(args.force)
                    .map_err(|code| std::process::exit(code))
            }
            Commands::Config { action } => {
                match action {
                    ConfigAction::Validate => cmd::config::validate(),
                    ConfigAction::Show => cmd::config::show(),
                }
                .map_err(|code| std::process::exit(code))
            }
        };
    }

    // No subcommand - existing file viewing / discovery logic
    // ... rest of current main() ...
}
```

## State of the Art

| Old Approach | Current Approach | When Changed | Impact |
|--------------|------------------|--------------|--------|
| clap builder API | clap derive API | clap 3.x (2022) | Simpler, type-safe argument handling |
| Manual color escapes | colored/termcolor crates | Established practice | NO_COLOR support built-in |
| config-rs merging | Closest-config-wins | User decision | Simpler mental model |

**Deprecated/outdated:**
- **serde-yaml**: Unmaintained, project already uses serde-saphyr
- **Manual NO_COLOR checking**: colored crate handles environment variable precedence

## Open Questions

Things that couldn't be fully resolved:

1. **Color choices for output**
   - What we know: colored crate provides full palette, NO_COLOR is respected automatically
   - What's unclear: Exact color assignments for keys vs values vs paths
   - Recommendation: Use consistent scheme - cyan for keys, green for values, yellow for paths, dimmed for headers. Claude's discretion per CONTEXT.md.

2. **Default config content when no config exists**
   - What we know: `config show` should show defaults per user decision
   - What's unclear: Whether to show a full template with comments or minimal default values
   - Recommendation: Show minimal output indicating defaults: "No config found. Using defaults." followed by effective default values.

## Sources

### Primary (HIGH confidence)
- Clap derive tutorial: https://docs.rs/clap/latest/clap/_derive/_tutorial/index.html
- Colored crate: https://github.com/colored-rs/colored
- Existing codebase: `src/config/*.rs`, `src/source.rs`, `src/capture.rs`, `src/main.rs`

### Secondary (MEDIUM confidence)
- Rain's Rust CLI recommendations: https://rust-cli-recommendations.sunshowers.io/handling-arguments.html

### Tertiary (LOW confidence)
- WebSearch patterns for YAML template generation (common approach verified in codebase)

## Metadata

**Confidence breakdown:**
- Standard stack: HIGH - Dependencies already in Cargo.toml, only adding colored
- Architecture: HIGH - Follows established patterns from clap docs and existing codebase
- Pitfalls: HIGH - Based on user decisions in CONTEXT.md and codebase analysis

**Research date:** 2026-02-05
**Valid until:** 30 days (stable domain, no fast-moving dependencies)
