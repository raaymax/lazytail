//! Config validate and show commands for lazytail.
//!
//! Provides config introspection and validation for CI pipelines and developer debugging.

use crate::config;
use colored::Colorize;
use std::path::PathBuf;

/// Get the effective config path (closest wins: project > global).
///
/// Config commands use "closest config wins completely" semantics:
/// - If project config exists (lazytail.yaml found in cwd or parent), use it exclusively
/// - Otherwise, fall back to global config (~/.config/lazytail/config.yaml)
/// - If neither exists, return None
fn effective_config_path() -> Option<PathBuf> {
    let discovery = config::discover();
    // Project config wins completely if it exists
    discovery.project_config.or(discovery.global_config)
}

/// Validate the effective config file.
///
/// Follows Unix conventions:
/// - Exit 0 with no output on success (quiet success)
/// - Exit 1 with error message to stderr on failure
///
/// Validates:
/// - YAML syntax
/// - Known field names (typo detection)
/// - Source file existence
pub fn validate() -> Result<(), i32> {
    // Find config to validate (closest wins)
    let config_path = match effective_config_path() {
        Some(path) => path,
        None => {
            eprintln!("error: No config found to validate");
            return Err(1);
        }
    };

    // Load ONLY the winning config file
    match config::load_single_file(&config_path) {
        Ok(cfg) => {
            // Check source file existence
            let mut has_errors = false;
            for source in &cfg.sources {
                if !source.exists {
                    eprintln!(
                        "error: Source '{}' file not found: {}",
                        source.name,
                        source.path.display()
                    );
                    has_errors = true;
                }
            }
            if has_errors {
                Err(1)
            } else {
                // Quiet success - just exit 0
                Ok(())
            }
        }
        Err(e) => {
            // Use existing Cargo-style error formatting from ConfigError
            eprintln!("{}", e);
            Err(1)
        }
    }
}

/// Show the effective configuration.
///
/// Displays:
/// - Which config file is being used ("Using: path")
/// - Config name (if set)
/// - Sources list with paths and existence status
///
/// When no config exists, shows defaults message.
/// Respects NO_COLOR environment variable via the colored crate.
pub fn show() -> Result<(), i32> {
    let config_path = effective_config_path();

    match config_path {
        Some(path) => {
            // Load ONLY the winning config file
            match config::load_single_file(&path) {
                Ok(cfg) => {
                    println!("Using: {}", path.display().to_string().dimmed());
                    println!();
                    show_config(&cfg);
                    Ok(())
                }
                Err(e) => {
                    eprintln!("{}", e);
                    Err(1)
                }
            }
        }
        None => {
            println!("{}", "No config found. Using defaults.".dimmed());
            println!();
            // Show empty/default state
            println!("{}: {}", "name".cyan(), "(not set)".dimmed());
            println!();
            println!("{}", "(no sources defined)".dimmed());
            Ok(())
        }
    }
}

/// Display the config contents with colored output.
fn show_config(cfg: &config::SingleFileConfig) {
    // Name
    if let Some(name) = &cfg.name {
        println!("{}: {}", "name".cyan(), name.green());
    } else {
        println!("{}: {}", "name".cyan(), "(not set)".dimmed());
    }

    // Sources (single list - closest config wins, no merge)
    if !cfg.sources.is_empty() {
        println!();
        println!("{}:", "sources".cyan());
        for source in &cfg.sources {
            show_source(source);
        }
    } else {
        println!();
        println!("{}", "(no sources defined)".dimmed());
    }
}

/// Display a single source with colored output.
fn show_source(source: &config::Source) {
    let status = if source.exists {
        String::new()
    } else {
        format!(" {}", "(not found)".red())
    };
    println!("  - {}: {}", "name".blue(), source.name.green());
    println!(
        "    {}: {}{}",
        "path".blue(),
        source.path.display().to_string().yellow(),
        status
    );
}
