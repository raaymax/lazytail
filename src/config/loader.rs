//! Config loading for lazytail.
//!
//! Loads and validates YAML config files with path expansion.

use std::path::Path;

use crate::config::discovery::DiscoveryResult;
use crate::config::error::ConfigError;
use crate::config::types::Config;

/// Load config from discovered config files.
///
/// Returns an empty Config if no config files exist (graceful degradation).
pub fn load(_discovery: &DiscoveryResult) -> Result<Config, ConfigError> {
    // TODO: Implement in Task 3
    Ok(Config::default())
}

/// Expand tilde in path to home directory.
#[allow(dead_code)]
fn expand_path(_path: &Path) -> std::path::PathBuf {
    // TODO: Implement in Task 3
    _path.to_path_buf()
}
