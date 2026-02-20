//! Config types for lazytail.
//!
//! Defines structures for parsing and representing configuration files.

use serde::Deserialize;
use std::path::PathBuf;

/// Raw config file structure (used for parsing).
///
/// This struct directly mirrors the YAML config file structure.
/// Unknown fields are rejected with an error.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawConfig {
    /// Project name (optional).
    pub name: Option<String>,
    /// List of log sources.
    #[serde(default)]
    pub sources: Vec<RawSource>,
    /// Whether to check for updates on TUI startup (default: true).
    #[serde(default)]
    pub update_check: Option<bool>,
}

/// Raw source from config file.
///
/// Represents a log source with a name and path as written in the config file.
/// Paths are not yet expanded (tilde not resolved).
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawSource {
    /// Display name for this source.
    pub name: String,
    /// Path to the log file (may contain tilde).
    pub path: PathBuf,
}

/// Validated source with expanded path and existence check.
///
/// After loading, paths are expanded (tilde resolved) and existence is checked.
#[derive(Debug, Clone)]
pub struct Source {
    /// Display name for this source.
    pub name: String,
    /// Expanded path to the log file.
    pub path: PathBuf,
    /// Whether the file exists at load time.
    pub exists: bool,
}

/// Merged config from global and project files.
///
/// Contains all sources from both global (~/.config/lazytail/config.yaml)
/// and project (lazytail.yaml) config files, kept in separate groups.
#[derive(Debug, Clone, Default)]
pub struct Config {
    /// Project name from project config (if present).
    pub name: Option<String>,
    /// Sources defined in the project config.
    pub project_sources: Vec<Source>,
    /// Sources defined in the global config.
    pub global_sources: Vec<Source>,
    /// Whether to check for updates on TUI startup (from global config).
    pub update_check: Option<bool>,
}

impl Config {
    /// Returns true if any sources are defined (project or global).
    #[cfg(test)]
    pub fn has_sources(&self) -> bool {
        !self.project_sources.is_empty() || !self.global_sources.is_empty()
    }
}
