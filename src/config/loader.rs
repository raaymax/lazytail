//! Config loading for lazytail.
//!
//! Loads and validates YAML config files with path expansion.

use std::fs;
use std::path::{Path, PathBuf};

use crate::config::discovery::DiscoveryResult;
use crate::config::error::ConfigError;
use crate::config::types::{Config, RawConfig, RawSource, Source};

/// Config loaded from a single file (for config commands).
///
/// Unlike [`Config`] which has `project_sources`/`global_sources`, this has a single
/// `sources` list. Used by `config validate` and `config show` where "closest config
/// wins completely" - we load ONLY the winning config file, not merge both.
#[derive(Debug, Default)]
pub struct SingleFileConfig {
    /// Project name (optional).
    pub name: Option<String>,
    /// List of log sources from this config file.
    pub sources: Vec<Source>,
}

/// Expand tilde in path to home directory.
///
/// Handles the following cases:
/// - `~/foo` -> `/home/user/foo`
/// - `/absolute/path` -> unchanged
/// - `relative/path` -> unchanged
pub fn expand_path(path: &Path) -> PathBuf {
    let path_str = path.to_string_lossy();

    // Handle tilde expansion
    if let Some(rest) = path_str.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    } else if path_str == "~" {
        if let Some(home) = dirs::home_dir() {
            return home;
        }
    }

    path.to_path_buf()
}

/// Load and parse a YAML config file.
///
/// Returns the parsed RawConfig or a ConfigError with location and suggestions.
fn load_file(path: &Path) -> Result<RawConfig, ConfigError> {
    // Read file content
    let content = fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_path_buf(),
        source: e,
    })?;

    // Parse YAML with serde-saphyr
    serde_saphyr::from_str(&content)
        .map_err(|e| ConfigError::from_saphyr_error(path.to_path_buf(), e))
}

/// Validate and expand paths in raw sources.
///
/// Expands tilde paths and checks file existence.
fn validate_sources(raw: Vec<RawSource>) -> Vec<Source> {
    raw.into_iter()
        .map(|raw_source| {
            let expanded_path = expand_path(&raw_source.path);
            let exists = expanded_path.try_exists().unwrap_or(false);
            Source {
                name: raw_source.name,
                path: expanded_path,
                exists,
            }
        })
        .collect()
}

/// Load config from a single file (closest-wins semantics for config commands).
///
/// Unlike [`load`] which merges project and global configs for the TUI,
/// this loads only one config file and returns its contents directly.
/// Used by `config validate` and `config show` commands.
pub fn load_single_file(path: &Path) -> Result<SingleFileConfig, ConfigError> {
    let raw = load_file(path)?;
    Ok(SingleFileConfig {
        name: raw.name,
        sources: validate_sources(raw.sources),
    })
}

/// Load config from discovered config files.
///
/// Merges project and global configs:
/// - Name comes from project config (if present)
/// - Sources are kept in separate groups (project_sources, global_sources)
///
/// Returns an empty Config if no config files exist (graceful degradation).
pub fn load(discovery: &DiscoveryResult) -> Result<Config, ConfigError> {
    let mut config = Config::default();
    let mut theme_raw: Option<crate::theme::RawThemeConfig> = None;

    // Load global config if it exists (loaded first so project can override)
    if let Some(global_path) = &discovery.global_config {
        let raw = load_file(global_path)?;
        config.global_sources = validate_sources(raw.sources);
        config.update_check = raw.update_check;
        theme_raw = raw.theme;
        // Note: global name is ignored, project name takes precedence
    }

    // Load project config if it exists
    if let Some(project_path) = &discovery.project_config {
        let raw = load_file(project_path)?;
        config.name = raw.name;
        config.project_sources = validate_sources(raw.sources);
        // Project theme overrides global theme (full override, not merge)
        if raw.theme.is_some() {
            theme_raw = raw.theme;
        }
    }

    // Resolve theme
    config.theme = crate::theme::loader::resolve_theme(&theme_raw, &[])?;

    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn test_expand_path_tilde() {
        let path = Path::new("~/logs/app.log");
        let expanded = expand_path(path);

        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home.join("logs/app.log"));
        } else {
            // If no home dir, path should be unchanged
            assert_eq!(expanded.to_string_lossy(), "~/logs/app.log");
        }
    }

    #[test]
    fn test_expand_path_tilde_only() {
        let path = Path::new("~");
        let expanded = expand_path(path);

        if let Some(home) = dirs::home_dir() {
            assert_eq!(expanded, home);
        } else {
            assert_eq!(expanded.to_string_lossy(), "~");
        }
    }

    #[test]
    fn test_expand_path_absolute() {
        let path = Path::new("/var/log/app.log");
        let expanded = expand_path(path);
        assert_eq!(expanded, PathBuf::from("/var/log/app.log"));
    }

    #[test]
    fn test_expand_path_relative() {
        let path = Path::new("logs/app.log");
        let expanded = expand_path(path);
        assert_eq!(expanded, PathBuf::from("logs/app.log"));
    }

    #[test]
    #[ignore] // Slow: creates temp directory
    fn test_load_empty_discovery() {
        // No config files exist
        let discovery = DiscoveryResult::default();
        let config = load(&discovery).unwrap();

        assert!(config.name.is_none());
        assert!(config.project_sources.is_empty());
        assert!(config.global_sources.is_empty());
        assert!(!config.has_sources());
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_project_config() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");

        fs::write(
            &config_path,
            r#"
name: "Test Project"
sources:
  - name: api
    path: /var/log/api.log
  - name: web
    path: ~/logs/web.log
"#,
        )
        .unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path),
            global_config: None,
        };

        let config = load(&discovery).unwrap();

        assert_eq!(config.name, Some("Test Project".to_string()));
        assert_eq!(config.project_sources.len(), 2);
        assert_eq!(config.project_sources[0].name, "api");
        assert_eq!(
            config.project_sources[0].path,
            PathBuf::from("/var/log/api.log")
        );
        assert_eq!(config.project_sources[1].name, "web");
        // Tilde should be expanded
        if let Some(home) = dirs::home_dir() {
            assert_eq!(config.project_sources[1].path, home.join("logs/web.log"));
        }
        assert!(config.global_sources.is_empty());
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_minimal_config() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");

        // Minimal valid config - just name
        fs::write(&config_path, "name: Minimal\n").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path),
            global_config: None,
        };

        let config = load(&discovery).unwrap();

        assert_eq!(config.name, Some("Minimal".to_string()));
        assert!(config.project_sources.is_empty());
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_empty_yaml() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");

        // Empty YAML file (or just whitespace/comments)
        fs::write(&config_path, "# Empty config\n").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path),
            global_config: None,
        };

        let config = load(&discovery).unwrap();

        // Empty YAML should result in empty config (all None/empty)
        assert!(config.name.is_none());
        assert!(config.project_sources.is_empty());
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_unknown_field_error() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");

        // Config with unknown field
        fs::write(
            &config_path,
            r#"
nam: "Typo"
sources: []
"#,
        )
        .unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path.clone()),
            global_config: None,
        };

        let result = load(&discovery);
        assert!(result.is_err());

        let error = result.unwrap_err();
        let display = error.to_string();

        // Should contain the path
        assert!(display.contains(&config_path.to_string_lossy().to_string()));

        // Should mention the unknown field
        assert!(display.contains("nam"));

        // Should suggest "name"
        assert!(display.contains("name"));
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_load_both_configs() {
        let temp = TempDir::new().unwrap();
        let project_config_path = temp.path().join("lazytail.yaml");
        let global_dir = temp.path().join("global");
        fs::create_dir(&global_dir).unwrap();
        let global_config_path = global_dir.join("config.yaml");

        fs::write(
            &project_config_path,
            r#"
name: "Project Name"
sources:
  - name: project-log
    path: /var/log/project.log
"#,
        )
        .unwrap();

        fs::write(
            &global_config_path,
            r#"
name: "Global Name (should be ignored)"
sources:
  - name: global-log
    path: /var/log/global.log
"#,
        )
        .unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(project_config_path),
            global_config: Some(global_config_path),
        };

        let config = load(&discovery).unwrap();

        // Project name takes precedence
        assert_eq!(config.name, Some("Project Name".to_string()));

        // Sources are kept separate
        assert_eq!(config.project_sources.len(), 1);
        assert_eq!(config.project_sources[0].name, "project-log");

        assert_eq!(config.global_sources.len(), 1);
        assert_eq!(config.global_sources[0].name, "global-log");

        assert!(config.has_sources());
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_source_existence_check() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");
        let existing_log = temp.path().join("existing.log");

        // Create one file that exists
        fs::write(&existing_log, "log content").unwrap();

        fs::write(
            &config_path,
            format!(
                r#"
sources:
  - name: existing
    path: {}
  - name: missing
    path: /nonexistent/path/file.log
"#,
                existing_log.display()
            ),
        )
        .unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path),
            global_config: None,
        };

        let config = load(&discovery).unwrap();

        assert_eq!(config.project_sources.len(), 2);

        // First source exists
        assert!(config.project_sources[0].exists);
        assert_eq!(config.project_sources[0].name, "existing");

        // Second source doesn't exist
        assert!(!config.project_sources[1].exists);
        assert_eq!(config.project_sources[1].name, "missing");
    }

    #[test]
    #[ignore] // Slow: creates temp directory
    fn test_load_missing_file() {
        let temp = TempDir::new().unwrap();
        let nonexistent = temp.path().join("nonexistent.yaml");

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(nonexistent.clone()),
            global_config: None,
        };

        let result = load(&discovery);
        assert!(result.is_err());

        match result.unwrap_err() {
            ConfigError::Io { path, .. } => {
                assert_eq!(path, nonexistent);
            }
            _ => panic!("Expected Io error"),
        }
    }

    #[test]
    #[ignore] // Slow: creates temp directory and files
    fn test_invalid_yaml_syntax() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join("lazytail.yaml");

        // Invalid YAML syntax
        fs::write(&config_path, "name: [\ninvalid yaml").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(temp.path().to_path_buf()),
            project_config: Some(config_path),
            global_config: None,
        };

        let result = load(&discovery);
        assert!(result.is_err());

        match result.unwrap_err() {
            ConfigError::Parse { .. } => {}
            e => panic!("Expected Parse error, got: {:?}", e),
        }
    }
}
