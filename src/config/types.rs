//! Config types for lazytail.
//!
//! Defines structures for parsing and representing configuration files.

use serde::Deserialize;
use std::collections::HashMap;
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
    /// Rendering preset definitions.
    #[serde(default)]
    pub renderers: Vec<RawRendererDef>,
    /// Theme configuration (name or custom struct).
    #[serde(default)]
    pub theme: Option<crate::theme::RawThemeConfig>,
}

/// Raw renderer definition from config file.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawRendererDef {
    pub name: String,
    pub detect: Option<RawDetectDef>,
    pub regex: Option<String>,
    pub layout: Vec<RawLayoutEntryDef>,
}

/// Raw detect rules for a renderer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawDetectDef {
    pub parser: Option<String>,
    pub filename: Option<String>,
}

/// Style value: either a single string or a list of strings (compound style).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum StyleValue {
    Single(String),
    List(Vec<String>),
}

/// Raw layout entry for a renderer.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RawLayoutEntryDef {
    pub field: Option<String>,
    pub literal: Option<String>,
    pub style: Option<StyleValue>,
    pub width: Option<usize>,
    pub format: Option<String>,
    pub style_map: Option<HashMap<String, String>>,
    pub max_width: Option<usize>,
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
    /// Path to the log file (may contain tilde). Optional for metadata-only sources.
    pub path: Option<PathBuf>,
    /// List of renderer preset names to use for this source.
    #[serde(default)]
    pub renderers: Vec<String>,
}

/// Validated source with expanded path and existence check.
///
/// After loading, paths are expanded (tilde resolved) and existence is checked.
#[derive(Debug, Clone)]
pub struct Source {
    /// Display name for this source.
    pub name: String,
    /// Expanded path to the log file (None for metadata-only sources).
    pub path: Option<PathBuf>,
    /// Whether the file exists at load time.
    pub exists: bool,
    /// Renderer preset names assigned to this source.
    pub renderer_names: Vec<String>,
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
    /// Raw renderer definitions (passed through to renderer compilation).
    pub renderers: Vec<RawRendererDef>,
    /// Resolved theme.
    pub theme: crate::theme::Theme,
}

impl Config {
    /// Returns true if any sources are defined (project or global).
    #[cfg(test)]
    pub fn has_sources(&self) -> bool {
        !self.project_sources.is_empty() || !self.global_sources.is_empty()
    }
}
