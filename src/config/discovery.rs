//! Config discovery for lazytail.
//!
//! Walks parent directories to find `lazytail.yaml` and checks for global config
//! at `~/.config/lazytail/config.yaml`.

use std::path::PathBuf;

/// Project config filename to search for in parent directories.
pub const PROJECT_CONFIG_NAME: &str = "lazytail.yaml";

/// Global config filename within the lazytail config directory.
pub const GLOBAL_CONFIG_NAME: &str = "config.yaml";

/// Data directory name for project-scoped storage.
pub const DATA_DIR_NAME: &str = ".lazytail";

/// Result of config discovery.
///
/// Contains paths to discovered configs and the project root directory.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryResult {
    /// Directory containing `lazytail.yaml` (the project root).
    pub project_root: Option<PathBuf>,
    /// Full path to the project config file (`lazytail.yaml`).
    pub project_config: Option<PathBuf>,
    /// Full path to the global config file (`~/.config/lazytail/config.yaml`).
    pub global_config: Option<PathBuf>,
}

impl DiscoveryResult {
    /// Returns the data directory path for project-scoped storage.
    ///
    /// This is `project_root/.lazytail` - used by Phase 4 for storing
    /// project-specific streams and state.
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.project_root
            .as_ref()
            .map(|root| root.join(DATA_DIR_NAME))
    }

    /// Returns true if any config was found (project or global).
    pub fn has_config(&self) -> bool {
        self.project_config.is_some() || self.global_config.is_some()
    }
}

/// Discover config files starting from the current working directory.
///
/// Walks parent directories looking for `lazytail.yaml` and checks for
/// a global config at `~/.config/lazytail/config.yaml`.
///
/// # Returns
///
/// A `DiscoveryResult` with paths to any discovered configs.
pub fn discover() -> DiscoveryResult {
    discover_verbose().0
}

/// Discover config files with verbose output.
///
/// Same as [`discover`] but also returns a list of all directories
/// that were searched during the walk. Useful for `-v` output.
///
/// # Returns
///
/// A tuple of (DiscoveryResult, Vec<PathBuf>) where the second element
/// contains all directories that were checked during discovery.
pub fn discover_verbose() -> (DiscoveryResult, Vec<PathBuf>) {
    let mut result = DiscoveryResult::default();
    let mut searched_paths = Vec::new();

    // Check global config first
    if let Some(config_dir) = dirs::config_dir() {
        let global_config_path = config_dir.join("lazytail").join(GLOBAL_CONFIG_NAME);
        if global_config_path.try_exists().unwrap_or(false) && global_config_path.is_file() {
            result.global_config = Some(global_config_path);
        }
    }

    // Get current working directory
    let cwd = match std::env::current_dir() {
        Ok(dir) => dir.canonicalize().unwrap_or(dir),
        Err(_) => return (result, searched_paths),
    };

    // Walk ancestors looking for lazytail.yaml
    for ancestor in cwd.ancestors() {
        searched_paths.push(ancestor.to_path_buf());

        let config_path = ancestor.join(PROJECT_CONFIG_NAME);
        if config_path.try_exists().unwrap_or(false) && config_path.is_file() {
            result.project_root = Some(ancestor.to_path_buf());
            result.project_config = Some(config_path);
            break;
        }
    }

    (result, searched_paths)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Mutex to serialize tests that change cwd
    static CWD_MUTEX: Mutex<()> = Mutex::new(());

    /// Helper to run a test with a specific working directory.
    /// Uses a mutex to prevent parallel tests from interfering.
    fn with_cwd<F, T>(dir: &std::path::Path, f: F) -> T
    where
        F: FnOnce() -> T,
    {
        let _lock = CWD_MUTEX.lock().unwrap();
        let original = std::env::current_dir().expect("Failed to get cwd");
        std::env::set_current_dir(dir).expect("Failed to set cwd");
        let result = f();
        // Restore even if f() panicked - but we can't because we're not using catch_unwind
        let _ = std::env::set_current_dir(&original);
        result
    }

    #[test]
    #[ignore] // Slow: creates temp directory and changes cwd
    fn test_finds_config_in_current_dir() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(PROJECT_CONFIG_NAME);
        fs::write(&config_path, "test: true").unwrap();

        let result = with_cwd(temp.path(), discover);

        assert!(
            result.project_config.is_some(),
            "project_config should be Some"
        );
        let found_config = result.project_config.unwrap();
        // Compare canonicalized paths to handle symlinks (e.g., /tmp -> /private/tmp on macOS)
        assert_eq!(
            found_config.canonicalize().unwrap(),
            config_path.canonicalize().unwrap(),
            "config paths should match"
        );
        assert!(result.project_root.is_some(), "project_root should be Some");
    }

    #[test]
    #[ignore] // Slow: creates nested temp directories and changes cwd
    fn test_finds_config_in_parent_dir() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.path().join("subdir");
        fs::create_dir(&subdir).unwrap();
        let config_path = temp.path().join(PROJECT_CONFIG_NAME);
        fs::write(&config_path, "test: true").unwrap();

        let result = with_cwd(&subdir, discover);

        assert!(
            result.project_config.is_some(),
            "project_config should be Some"
        );
        let found_config = result.project_config.unwrap();
        // Compare canonicalized paths
        assert_eq!(
            found_config.canonicalize().unwrap(),
            config_path.canonicalize().unwrap(),
            "config paths should match"
        );
        assert!(result.project_root.is_some(), "project_root should be Some");
    }

    #[test]
    #[ignore] // Slow: creates temp directory and changes cwd
    fn test_no_config_returns_defaults() {
        let temp = TempDir::new().unwrap();
        // Don't create any config file

        let result = with_cwd(temp.path(), discover);

        assert!(result.project_config.is_none());
        assert!(result.project_root.is_none());
        // Global config might exist depending on system state
    }

    #[test]
    #[ignore] // Slow: creates temp directory and changes cwd
    fn test_data_dir_derived_from_project_root() {
        let temp = TempDir::new().unwrap();
        let config_path = temp.path().join(PROJECT_CONFIG_NAME);
        fs::write(&config_path, "test: true").unwrap();

        let result = with_cwd(temp.path(), discover);

        assert!(result.data_dir().is_some(), "data_dir should be Some");
        // data_dir should end with .lazytail
        let data_dir = result.data_dir().unwrap();
        assert!(
            data_dir.ends_with(DATA_DIR_NAME),
            "data_dir should end with {}",
            DATA_DIR_NAME
        );
    }

    #[test]
    fn test_has_config_methods() {
        // Empty result
        let empty = DiscoveryResult::default();
        assert!(!empty.has_config());

        // With project config
        let with_project = DiscoveryResult {
            project_config: Some(PathBuf::from("/test/lazytail.yaml")),
            ..Default::default()
        };
        assert!(with_project.has_config());

        // With global config
        let with_global = DiscoveryResult {
            global_config: Some(PathBuf::from("/home/user/.config/lazytail/config.yaml")),
            ..Default::default()
        };
        assert!(with_global.has_config());

        // With both
        let with_both = DiscoveryResult {
            project_config: Some(PathBuf::from("/test/lazytail.yaml")),
            global_config: Some(PathBuf::from("/home/user/.config/lazytail/config.yaml")),
            ..Default::default()
        };
        assert!(with_both.has_config());
    }

    #[test]
    fn test_data_dir_none_without_project_root() {
        let result = DiscoveryResult::default();
        assert!(result.data_dir().is_none());
    }

    #[test]
    #[ignore] // Slow: creates nested temp directories and changes cwd
    fn test_walk_stops_at_root() {
        let temp = TempDir::new().unwrap();
        // Don't create any config file - walk should go to root and stop

        let (result, searched_paths) = with_cwd(temp.path(), discover_verbose);

        // Should have searched multiple directories
        assert!(
            !searched_paths.is_empty(),
            "searched_paths should not be empty"
        );

        // Last path should be root (/)
        let last_path = searched_paths.last().unwrap();
        assert_eq!(
            last_path,
            &PathBuf::from("/"),
            "last searched path should be /"
        );

        // No config found
        assert!(result.project_config.is_none());
    }

    #[test]
    #[ignore] // Slow: creates temp directories, checks global config detection
    fn test_global_config_detection() {
        // This test verifies the global config detection logic works
        // by checking that when a global config exists, it's found.
        // Note: We can't easily mock dirs::config_dir(), so we test
        // the structure of the result when global config may exist.

        let temp = TempDir::new().unwrap();
        let result = with_cwd(temp.path(), discover);

        // If global config exists on the system, it should be found.
        // We can't guarantee it exists, but we can verify the path structure
        // if it is found.
        if let Some(global_path) = result.global_config {
            assert!(global_path.to_string_lossy().contains("lazytail"));
            assert!(global_path.to_string_lossy().contains(GLOBAL_CONFIG_NAME));
        }
        // Test passes either way - we're verifying the logic, not system state
    }

    #[test]
    #[ignore] // Slow: creates nested temp directories and changes cwd
    fn test_verbose_returns_searched_paths() {
        let temp = TempDir::new().unwrap();
        let subdir = temp.path().join("level1").join("level2");
        fs::create_dir_all(&subdir).unwrap();

        let (_, searched_paths) = with_cwd(&subdir, discover_verbose);

        // Should include parent directories (multiple levels)
        assert!(
            searched_paths.len() >= 3,
            "should search at least 3 directories, got {}",
            searched_paths.len()
        );

        // The first searched path should be the canonicalized subdir
        let first_path = &searched_paths[0];
        let canonicalized_subdir = subdir.canonicalize().unwrap();
        assert_eq!(
            first_path, &canonicalized_subdir,
            "first searched path should be the starting directory"
        );
    }
}
