//! Source discovery and marker management for lazytail.
//!
//! This module handles:
//! - Config directory paths for data and sources
//! - Source discovery from the data directory
//! - PID-based marker files for active source tracking
//! - Collision detection for capture mode

use crate::config::DiscoveryResult;
use anyhow::{Context, Result};
use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::DirBuilderExt;

/// Status of a discovered source
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceStatus {
    /// Source is actively being written to (marker exists, PID running)
    Active,
    /// Source has ended (no marker or PID not running)
    Ended,
}

/// A discovered log source
#[derive(Debug, Clone)]
pub struct DiscoveredSource {
    /// Display name (filename without .log extension)
    pub name: String,
    /// Full path to the log file
    pub log_path: PathBuf,
    /// Whether the source is currently active
    pub status: SourceStatus,
}

/// Get the data directory path: ~/.config/lazytail/data/
pub fn data_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("lazytail").join("data"))
}

/// Get the sources directory path: ~/.config/lazytail/sources/
pub fn sources_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("lazytail").join("sources"))
}

/// Ensure both data and sources directories exist.
pub fn ensure_directories() -> Result<()> {
    if let Some(data) = data_dir() {
        fs::create_dir_all(&data).context("Failed to create data directory")?;
    }
    if let Some(sources) = sources_dir() {
        fs::create_dir_all(&sources).context("Failed to create sources directory")?;
    }
    Ok(())
}

/// Create a directory with secure permissions (mode 0700 on Unix).
///
/// Creates the directory and all parent directories if they don't exist.
/// On Unix systems, the directory is created with mode 0700 (owner read/write/execute only).
/// On other platforms, the directory is created with default permissions.
pub fn create_secure_dir(path: &Path) -> std::io::Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);

    #[cfg(unix)]
    builder.mode(0o700);

    builder.create(path)
}

/// Resolve the data directory path based on discovery context.
///
/// If inside a project (discovery has project_root), returns `project_root/.lazytail/data`.
/// Otherwise, falls back to the global data directory `~/.config/lazytail/data`.
pub fn resolve_data_dir(discovery: &DiscoveryResult) -> Option<PathBuf> {
    if let Some(ref project_root) = discovery.project_root {
        Some(project_root.join(".lazytail").join("data"))
    } else {
        data_dir()
    }
}

/// Resolve the sources directory path based on discovery context.
///
/// If inside a project (discovery has project_root), returns `project_root/.lazytail/sources`.
/// Otherwise, falls back to the global sources directory `~/.config/lazytail/sources`.
pub fn resolve_sources_dir(discovery: &DiscoveryResult) -> Option<PathBuf> {
    if let Some(ref project_root) = discovery.project_root {
        Some(project_root.join(".lazytail").join("sources"))
    } else {
        sources_dir()
    }
}

/// Ensure directories exist for the given discovery context.
///
/// Creates data and sources directories using secure permissions.
/// Uses project-local directories if in a project, global directories otherwise.
pub fn ensure_directories_for_context(discovery: &DiscoveryResult) -> Result<()> {
    if let Some(data) = resolve_data_dir(discovery) {
        create_secure_dir(&data)
            .with_context(|| format!("Failed to create data directory: {}", data.display()))?;
    }
    if let Some(sources) = resolve_sources_dir(discovery) {
        create_secure_dir(&sources).with_context(|| {
            format!("Failed to create sources directory: {}", sources.display())
        })?;
    }
    Ok(())
}

/// Check if a process with the given PID is running.
///
/// On Linux, checks if /proc/<pid>/ exists.
/// On macOS/BSD, uses kill(pid, 0) to check process existence.
pub fn is_pid_running(pid: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new(&format!("/proc/{}", pid)).exists()
    }
    #[cfg(not(target_os = "linux"))]
    {
        // PIDs that don't fit in i32 are invalid
        let Ok(pid_i32) = i32::try_from(pid) else {
            return false;
        };
        // PID 0 and negative PIDs have special meaning in kill()
        if pid_i32 <= 0 {
            return false;
        }
        // SAFETY: kill with signal 0 doesn't actually send a signal,
        // it just checks if the process exists and we have permission
        unsafe { libc::kill(pid_i32, 0) == 0 }
    }
}

/// Read the PID from a marker file.
fn read_marker_pid(marker_path: &Path) -> Option<u32> {
    let mut file = File::open(marker_path).ok()?;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    contents.trim().parse().ok()
}

/// Check the status of a source by name.
///
/// Returns `Active` if the marker exists and the PID is running,
/// otherwise returns `Ended`.
pub fn check_source_status(name: &str) -> SourceStatus {
    let Some(sources) = sources_dir() else {
        return SourceStatus::Ended;
    };

    let marker_path = sources.join(name);
    if !marker_path.exists() {
        return SourceStatus::Ended;
    }

    match read_marker_pid(&marker_path) {
        Some(pid) if is_pid_running(pid) => SourceStatus::Active,
        _ => SourceStatus::Ended,
    }
}

/// Discover all log sources from the data directory.
///
/// Scans ~/.config/lazytail/data/ for .log files and checks
/// their status against the sources/ markers.
pub fn discover_sources() -> Result<Vec<DiscoveredSource>> {
    let Some(data) = data_dir() else {
        return Ok(Vec::new());
    };

    if !data.exists() {
        return Ok(Vec::new());
    }

    let mut sources = Vec::new();

    for entry in fs::read_dir(&data).context("Failed to read data directory")? {
        let entry = entry?;
        let path = entry.path();

        // Only process .log files
        if path.extension().is_some_and(|ext| ext == "log") {
            if let Some(stem) = path.file_stem() {
                let name = stem.to_string_lossy().to_string();
                let status = check_source_status(&name);

                sources.push(DiscoveredSource {
                    name,
                    log_path: path,
                    status,
                });
            }
        }
    }

    // Sort by name for consistent ordering
    sources.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(sources)
}

/// Create a marker file for the given source name.
///
/// Uses atomic creation to prevent races. Writes the current PID.
pub fn create_marker(name: &str) -> Result<()> {
    let sources = sources_dir().context("Could not determine config directory")?;
    fs::create_dir_all(&sources).context("Failed to create sources directory")?;

    let marker_path = sources.join(name);

    // Use create_new for atomic creation (fails if exists)
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .context("Failed to create marker (source may already be active)")?;

    writeln!(file, "{}", std::process::id())?;
    file.flush()?;

    Ok(())
}

/// Check the status of a source by name using discovery context.
///
/// Returns `Active` if the marker exists and the PID is running,
/// otherwise returns `Ended`.
pub fn check_source_status_for_context(name: &str, discovery: &DiscoveryResult) -> SourceStatus {
    let Some(sources) = resolve_sources_dir(discovery) else {
        return SourceStatus::Ended;
    };

    let marker_path = sources.join(name);
    if !marker_path.exists() {
        return SourceStatus::Ended;
    }

    match read_marker_pid(&marker_path) {
        Some(pid) if is_pid_running(pid) => SourceStatus::Active,
        _ => SourceStatus::Ended,
    }
}

/// Create a marker file for the given source name using discovery context.
///
/// Uses atomic creation to prevent races. Writes the current PID.
pub fn create_marker_for_context(name: &str, discovery: &DiscoveryResult) -> Result<()> {
    let sources =
        resolve_sources_dir(discovery).context("Could not determine sources directory")?;
    create_secure_dir(&sources).context("Failed to create sources directory")?;

    let marker_path = sources.join(name);

    // Use create_new for atomic creation (fails if exists)
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&marker_path)
        .context("Failed to create marker (source may already be active)")?;

    writeln!(file, "{}", std::process::id())?;
    file.flush()?;

    Ok(())
}

/// Remove a marker file for the given source name using discovery context.
pub fn remove_marker_for_context(name: &str, discovery: &DiscoveryResult) -> Result<()> {
    let Some(sources) = resolve_sources_dir(discovery) else {
        return Ok(());
    };

    let marker_path = sources.join(name);
    if marker_path.exists() {
        fs::remove_file(&marker_path).context("Failed to remove marker")?;
    }

    Ok(())
}

/// Remove a marker file for the given source name.
pub fn remove_marker(name: &str) -> Result<()> {
    let Some(sources) = sources_dir() else {
        return Ok(());
    };

    let marker_path = sources.join(name);
    if marker_path.exists() {
        fs::remove_file(&marker_path).context("Failed to remove marker")?;
    }

    Ok(())
}

/// Delete a source (log file and marker).
///
/// Only deletes sources in the lazytail data directory.
pub fn delete_source(name: &str, log_path: &Path) -> Result<()> {
    // Safety check: only delete files in our data directory
    let Some(data) = data_dir() else {
        anyhow::bail!("Could not determine data directory");
    };

    if !log_path.starts_with(&data) {
        anyhow::bail!("Cannot delete source outside data directory");
    }

    // Remove the log file
    if log_path.exists() {
        fs::remove_file(log_path).context("Failed to delete source log file")?;
    }

    // Remove the marker if it exists (cleanup stale markers)
    let _ = remove_marker(name);

    Ok(())
}

/// Remove marker files for processes that are no longer running.
///
/// Called at startup to recover from SIGKILL scenarios where the capture
/// process was killed without cleanup. Errors are logged to stderr but
/// don't prevent startup.
pub fn cleanup_stale_markers() {
    let Some(sources) = sources_dir() else {
        return;
    };

    if !sources.exists() {
        return;
    }

    let entries = match fs::read_dir(&sources) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("Warning: Could not read sources directory: {}", e);
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };
        let path = entry.path();

        // Skip non-files
        if !path.is_file() {
            continue;
        }

        // Read PID from marker
        if let Some(pid) = read_marker_pid(&path) {
            // Only remove if process is definitely not running
            if !is_pid_running(pid) {
                // Remove silently - user doesn't need to know
                let _ = fs::remove_file(&path);
            }
        }
    }
}

/// Validate a source name for use in capture mode.
pub fn validate_source_name(name: &str) -> Result<()> {
    if name.is_empty() {
        anyhow::bail!("Source name cannot be empty");
    }
    if name.contains('/') || name.contains('\\') || name.contains('\0') {
        anyhow::bail!("Source name cannot contain path separators");
    }
    if name.len() > 255 {
        anyhow::bail!("Source name too long (max 255 characters)");
    }
    if name.starts_with('.') {
        anyhow::bail!("Source name cannot start with a dot");
    }
    // Check for other problematic characters
    if name.contains(':') || name.contains('*') || name.contains('?') {
        anyhow::bail!("Source name contains invalid characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::env;
    use tempfile::TempDir;

    fn with_temp_config<F>(f: F)
    where
        F: FnOnce(&Path),
    {
        let temp = TempDir::new().unwrap();
        let old_home = env::var("HOME").ok();

        // Set HOME to temp dir so dirs::config_dir() uses it
        env::set_var("HOME", temp.path());

        f(temp.path());

        // Restore HOME
        if let Some(home) = old_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }

    #[test]
    fn test_is_pid_running_self() {
        // Current process should be running
        assert!(is_pid_running(std::process::id()));
    }

    #[test]
    fn test_is_pid_running_nonexistent() {
        // Very high PID should not exist
        assert!(!is_pid_running(u32::MAX));
    }

    #[test]
    fn test_validate_source_name_valid() {
        assert!(validate_source_name("myapp").is_ok());
        assert!(validate_source_name("my-app").is_ok());
        assert!(validate_source_name("my_app_123").is_ok());
        assert!(validate_source_name("API").is_ok());
    }

    #[test]
    fn test_validate_source_name_invalid() {
        assert!(validate_source_name("").is_err());
        assert!(validate_source_name("path/name").is_err());
        assert!(validate_source_name(".hidden").is_err());
        assert!(validate_source_name("name:with:colons").is_err());
        assert!(validate_source_name("name*star").is_err());
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_ensure_directories() {
        with_temp_config(|_| {
            ensure_directories().unwrap();

            let data = data_dir().unwrap();
            let sources = sources_dir().unwrap();

            assert!(data.exists());
            assert!(sources.exists());
        });
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_marker_creation_and_removal() {
        with_temp_config(|_| {
            ensure_directories().unwrap();

            // Create marker
            create_marker("test").unwrap();

            // Marker should exist
            let marker_path = sources_dir().unwrap().join("test");
            assert!(marker_path.exists());

            // Read PID
            let pid = read_marker_pid(&marker_path).unwrap();
            assert_eq!(pid, std::process::id());

            // Status should be Active (our PID is running)
            assert_eq!(check_source_status("test"), SourceStatus::Active);

            // Remove marker
            remove_marker("test").unwrap();
            assert!(!marker_path.exists());

            // Status should be Ended
            assert_eq!(check_source_status("test"), SourceStatus::Ended);
        });
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_discover_sources() {
        with_temp_config(|_| {
            ensure_directories().unwrap();

            let data = data_dir().unwrap();

            // Create some test log files
            fs::write(data.join("api.log"), "test").unwrap();
            fs::write(data.join("worker.log"), "test").unwrap();
            fs::write(data.join("notlog.txt"), "test").unwrap(); // Should be ignored

            // Create a marker for api
            create_marker("api").unwrap();

            // Discover sources
            let sources = discover_sources().unwrap();

            assert_eq!(sources.len(), 2);

            // Sources should be sorted by name
            assert_eq!(sources[0].name, "api");
            assert_eq!(sources[0].status, SourceStatus::Active);

            assert_eq!(sources[1].name, "worker");
            assert_eq!(sources[1].status, SourceStatus::Ended);
        });
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_create_secure_dir() {
        let temp = TempDir::new().unwrap();
        let secure_path = temp.path().join("secure_dir");

        create_secure_dir(&secure_path).unwrap();

        assert!(secure_path.exists());
        assert!(secure_path.is_dir());

        // On Unix, check permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let metadata = fs::metadata(&secure_path).unwrap();
            let mode = metadata.permissions().mode();
            // Check that the last 9 bits (rwx for user/group/other) are 0o700
            assert_eq!(mode & 0o777, 0o700, "directory should have mode 0700");
        }
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_create_secure_dir_recursive() {
        let temp = TempDir::new().unwrap();
        let nested_path = temp.path().join("level1").join("level2").join("level3");

        create_secure_dir(&nested_path).unwrap();

        assert!(nested_path.exists());
        assert!(nested_path.is_dir());
    }

    #[test]
    fn test_resolve_data_dir_with_project() {
        let project_root = PathBuf::from("/test/project");
        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        let result = resolve_data_dir(&discovery);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/test/project/.lazytail/data")
        );
    }

    #[test]
    fn test_resolve_data_dir_without_project() {
        let discovery = DiscoveryResult::default();

        // Without project root, should fall back to global data_dir()
        let result = resolve_data_dir(&discovery);

        // On systems with dirs::config_dir(), this should return a path
        // containing "lazytail/data"
        if let Some(path) = result {
            assert!(path.to_string_lossy().contains("lazytail"));
            assert!(path.to_string_lossy().contains("data"));
        }
    }

    #[test]
    fn test_resolve_sources_dir_with_project() {
        let project_root = PathBuf::from("/test/project");
        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        let result = resolve_sources_dir(&discovery);
        assert!(result.is_some());
        assert_eq!(
            result.unwrap(),
            PathBuf::from("/test/project/.lazytail/sources")
        );
    }

    #[test]
    fn test_resolve_sources_dir_without_project() {
        let discovery = DiscoveryResult::default();

        // Without project root, should fall back to global sources_dir()
        let result = resolve_sources_dir(&discovery);

        // On systems with dirs::config_dir(), this should return a path
        // containing "lazytail/sources"
        if let Some(path) = result {
            assert!(path.to_string_lossy().contains("lazytail"));
            assert!(path.to_string_lossy().contains("sources"));
        }
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_ensure_directories_for_context_project() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().to_path_buf();
        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        ensure_directories_for_context(&discovery).unwrap();

        let data_dir = project_root.join(".lazytail").join("data");
        let sources_dir = project_root.join(".lazytail").join("sources");

        assert!(data_dir.exists(), "data directory should exist");
        assert!(sources_dir.exists(), "sources directory should exist");

        // On Unix, check permissions
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let data_mode = fs::metadata(&data_dir).unwrap().permissions().mode();
            let sources_mode = fs::metadata(&sources_dir).unwrap().permissions().mode();
            assert_eq!(data_mode & 0o777, 0o700, "data dir should have mode 0700");
            assert_eq!(
                sources_mode & 0o777,
                0o700,
                "sources dir should have mode 0700"
            );
        }
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_marker_for_context() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().to_path_buf();
        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        // Create marker
        create_marker_for_context("test", &discovery).unwrap();

        // Marker should exist
        let marker_path = project_root.join(".lazytail").join("sources").join("test");
        assert!(marker_path.exists(), "marker should exist");

        // Status should be Active
        assert_eq!(
            check_source_status_for_context("test", &discovery),
            SourceStatus::Active
        );

        // Remove marker
        remove_marker_for_context("test", &discovery).unwrap();
        assert!(!marker_path.exists(), "marker should be removed");

        // Status should be Ended
        assert_eq!(
            check_source_status_for_context("test", &discovery),
            SourceStatus::Ended
        );
    }
}
