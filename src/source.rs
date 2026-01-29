//! Source discovery and marker management for lazytail.
//!
//! This module handles:
//! - Config directory paths for data and sources
//! - Source discovery from the data directory
//! - PID-based marker files for active source tracking
//! - Collision detection for capture mode

use anyhow::{Context, Result};
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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

/// Check if a process with the given PID is running.
///
/// On Linux, checks if /proc/<pid>/ exists.
pub fn is_pid_running(pid: u32) -> bool {
    Path::new(&format!("/proc/{}", pid)).exists()
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
        if path.extension().map_or(false, |ext| ext == "log") {
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
}
