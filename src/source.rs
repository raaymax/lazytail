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

/// Derive the index directory path for a given log file.
/// e.g., `/path/to/myapp.log` → `/path/to/myapp.idx/`
pub fn index_dir_for_log(log_path: &Path) -> PathBuf {
    log_path.with_extension("idx")
}

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

/// Location where a source was discovered
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceLocation {
    /// Source is in project-local .lazytail/data/
    Project,
    /// Source is in global ~/.config/lazytail/data/
    Global,
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
    /// Where the source was discovered (project-local or global)
    pub location: SourceLocation,
}

/// Get the lazytail config directory: ~/.config/lazytail/
///
/// Always uses `~/.config/` regardless of platform for consistency.
/// On macOS, `dirs::config_dir()` returns `~/Library/Application Support/`,
/// but `~/.config/` is more convenient for CLI tools.
pub fn lazytail_dir() -> Option<PathBuf> {
    dirs::home_dir().map(|p| p.join(".config").join("lazytail"))
}

/// Get the data directory path: ~/.config/lazytail/data/
pub fn data_dir() -> Option<PathBuf> {
    lazytail_dir().map(|p| p.join("data"))
}

/// Get the sources directory path: ~/.config/lazytail/sources/
pub fn sources_dir() -> Option<PathBuf> {
    lazytail_dir().map(|p| p.join("sources"))
}

/// Ensure both data and sources directories exist.
#[cfg(test)]
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
        // it just checks if the process exists and we have permission.
        // Returns 0 if process exists and we can signal it.
        // Returns -1 with EPERM if process exists but we lack permission
        // (common on macOS due to sandboxing/hardened runtime).
        // Returns -1 with ESRCH if process doesn't exist.
        unsafe {
            libc::kill(pid_i32, 0) == 0
                || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
        }
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

/// Scan a data directory for log sources.
///
/// Helper function that scans a directory for .log files and returns
/// discovered sources with the specified location.
fn scan_data_directory(
    dir: &Path,
    sources_dir: Option<&Path>,
    location: SourceLocation,
) -> Result<Vec<DiscoveredSource>> {
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut sources = Vec::new();

    for entry in fs::read_dir(dir).context("Failed to read data directory")? {
        let entry = entry?;
        let path = entry.path();

        // Only process .log files
        if path.extension().is_some_and(|ext| ext == "log") {
            if let Some(stem) = path.file_stem() {
                let name = stem.to_string_lossy().to_string();

                // Check status using the provided sources directory
                let status = if let Some(src_dir) = sources_dir {
                    check_source_status_in_dir(&name, src_dir)
                } else {
                    SourceStatus::Ended
                };

                sources.push(DiscoveredSource {
                    name,
                    log_path: path,
                    status,
                    location,
                });
            }
        }
    }

    // Sort by name for consistent ordering
    sources.sort_by(|a, b| a.name.cmp(&b.name));

    Ok(sources)
}

/// Check the status of a source by name in a specific sources directory.
pub fn check_source_status_in_dir(name: &str, sources_dir: &Path) -> SourceStatus {
    let marker_path = sources_dir.join(name);
    if !marker_path.exists() {
        return SourceStatus::Ended;
    }

    match read_marker_pid(&marker_path) {
        Some(pid) if is_pid_running(pid) => SourceStatus::Active,
        _ => SourceStatus::Ended,
    }
}

/// Discover log sources from both project and global data directories.
///
/// Scans both the project-local `.lazytail/data/` (if in a project) and the
/// global `~/.config/lazytail/data/` directories for log sources.
///
/// Project sources appear first in the result. Sources with the same name in
/// both locations are both returned — `SourceLocation` distinguishes them.
pub fn discover_sources_for_context(discovery: &DiscoveryResult) -> Result<Vec<DiscoveredSource>> {
    let mut all_sources = Vec::new();

    // First, scan project data directory if in a project
    if discovery.project_root.is_some() {
        if let Some(project_data) = resolve_data_dir(discovery) {
            let project_sources = resolve_sources_dir(discovery);
            let sources = scan_data_directory(
                &project_data,
                project_sources.as_deref(),
                SourceLocation::Project,
            )?;
            all_sources.extend(sources);
        }
    }

    // Then, scan global data directory
    if let Some(global_data) = data_dir() {
        let global_sources_path = sources_dir();
        let sources = scan_data_directory(
            &global_data,
            global_sources_path.as_deref(),
            SourceLocation::Global,
        )?;
        all_sources.extend(sources);
    }

    Ok(all_sources)
}

/// Create a marker file for the given source name.
///
/// Uses atomic creation to prevent races. Writes the current PID.
#[cfg(test)]
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

/// Create a marker file for the given source name using discovery context.
///
/// Cleans up stale markers from killed processes before creating. Uses atomic
/// creation to prevent races between concurrent capture processes.
pub fn create_marker_for_context(name: &str, discovery: &DiscoveryResult) -> Result<()> {
    let sources =
        resolve_sources_dir(discovery).context("Could not determine sources directory")?;
    create_secure_dir(&sources).context("Failed to create sources directory")?;

    let marker_path = sources.join(name);

    // Clean up stale marker from a killed process (SIGKILL, OOM, etc.)
    if marker_path.exists() {
        match read_marker_pid(&marker_path) {
            Some(pid) if is_pid_running(pid) => {
                anyhow::bail!("Source '{}' is already active (PID {})", name, pid);
            }
            _ => {
                fs::remove_file(&marker_path).context("Failed to remove stale marker")?;
            }
        }
    }

    // Use create_new for atomic creation (fails if another process raced us)
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
#[cfg(test)]
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
/// Only deletes sources in lazytail data directories (global or project-local).
/// Derives the marker directory from the log path's parent structure.
pub fn delete_source(name: &str, log_path: &Path) -> Result<()> {
    // Safety check: only delete files in a lazytail data directory
    let is_in_global = data_dir().is_some_and(|d| log_path.starts_with(&d));
    let is_in_project = log_path
        .parent()
        .and_then(|p| p.file_name())
        .is_some_and(|n| n == "data")
        && log_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.file_name())
            .is_some_and(|n| n == ".lazytail");

    if !is_in_global && !is_in_project {
        anyhow::bail!("Cannot delete source outside data directory");
    }

    // Remove the log file
    if log_path.exists() {
        fs::remove_file(log_path).context("Failed to delete source log file")?;
    }

    // Remove the marker — derive sources dir from the log path's data dir sibling
    if let Some(sources_dir) = log_path
        .parent()
        .and_then(|data_dir| data_dir.parent())
        .map(|root| root.join("sources"))
    {
        let marker_path = sources_dir.join(name);
        if marker_path.exists() {
            let _ = fs::remove_file(marker_path);
        }
    }

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

/// Resolve a source name to its log file path in a specific data directory.
pub fn resolve_source_in(name: &str, data_dir: &Path) -> Result<PathBuf> {
    validate_source_name(name)?;
    let path = data_dir.join(format!("{name}.log"));
    if !path.exists() {
        anyhow::bail!("Source '{name}' not found. Use list_sources to see available sources.");
    }
    Ok(path)
}

/// Resolve a source name to its log file path using discovery context.
///
/// Checks the project-local `.lazytail/data/` first (if in a project),
/// then falls back to the global `~/.config/lazytail/data/`.
pub fn resolve_source_for_context(name: &str, discovery: &DiscoveryResult) -> Result<PathBuf> {
    validate_source_name(name)?;

    // Try project data directory first
    if discovery.project_root.is_some() {
        if let Some(project_data) = resolve_data_dir(discovery) {
            let path = project_data.join(format!("{name}.log"));
            if path.exists() {
                return Ok(path);
            }
        }
    }

    // Fall back to global data directory
    let data = data_dir().context("Could not determine data directory")?;
    resolve_source_in(name, &data)
}

/// Build columnar indexes for discovered sources that don't have one.
pub fn build_missing_indexes(sources: &[DiscoveredSource]) {
    use crate::index::builder::IndexBuilder;

    let missing: Vec<_> = sources
        .iter()
        .filter(|s| !index_dir_for_log(&s.log_path).join("meta").exists())
        .collect();

    if missing.is_empty() {
        return;
    }

    eprintln!(
        "Building indexes for {} source{}...",
        missing.len(),
        if missing.len() == 1 { "" } else { "s" }
    );

    for (i, source) in missing.iter().enumerate() {
        let idx_dir = index_dir_for_log(&source.log_path);
        let file_size = std::fs::metadata(&source.log_path)
            .map(|m| m.len())
            .unwrap_or(0);
        eprintln!(
            "  [{}/{}] Indexing {} ({})...",
            i + 1,
            missing.len(),
            source.name,
            format_bytes(file_size),
        );
        let start = std::time::Instant::now();
        match IndexBuilder::new().build(&source.log_path, &idx_dir) {
            Ok(meta) => {
                eprintln!(
                    "  [{}/{}] Done: {} lines indexed in {:.1?}",
                    i + 1,
                    missing.len(),
                    meta.entry_count,
                    start.elapsed(),
                );
            }
            Err(e) => {
                eprintln!(
                    "  [{}/{}] Warning: failed to build index for {}: {}",
                    i + 1,
                    missing.len(),
                    source.name,
                    e,
                );
            }
        }
    }
}

fn format_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else if bytes < 1024 * 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} GB", bytes as f64 / (1024.0 * 1024.0 * 1024.0))
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
    use std::sync::Mutex;
    use tempfile::TempDir;

    // Mutex to serialize tests that modify the HOME env var
    static HOME_MUTEX: Mutex<()> = Mutex::new(());

    fn with_temp_config<F>(f: F)
    where
        F: FnOnce(&Path),
    {
        let _lock = HOME_MUTEX.lock().unwrap();
        let temp = TempDir::new().unwrap();
        let old_home = env::var("HOME").ok();

        // Set HOME so dirs::home_dir() uses temp dir
        env::set_var("HOME", temp.path());

        f(temp.path());

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
    fn test_resolve_source_in_found() {
        let temp = TempDir::new().unwrap();
        fs::write(temp.path().join("myapp.log"), "data").unwrap();
        let result = resolve_source_in("myapp", temp.path());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), temp.path().join("myapp.log"));
    }

    #[test]
    fn test_resolve_source_in_not_found() {
        let temp = TempDir::new().unwrap();
        let result = resolve_source_in("missing", temp.path());
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("not found"), "error was: {err}");
        assert!(err.contains("list_sources"), "error was: {err}");
    }

    #[test]
    fn test_resolve_source_in_rejects_invalid_name() {
        let temp = TempDir::new().unwrap();
        assert!(resolve_source_in("", temp.path()).is_err());
        assert!(resolve_source_in("../etc/passwd", temp.path()).is_err());
        assert!(resolve_source_in(".hidden", temp.path()).is_err());
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

            // Discover sources (global-only context, no project root)
            let discovery = crate::config::DiscoveryResult {
                project_root: None,
                project_config: None,
                global_config: None,
            };
            let sources = discover_sources_for_context(&discovery).unwrap();

            assert_eq!(sources.len(), 2);

            // Sources should be sorted by name
            assert_eq!(sources[0].name, "api");
            assert_eq!(sources[0].status, SourceStatus::Active);
            assert_eq!(sources[0].location, SourceLocation::Global);

            assert_eq!(sources[1].name, "worker");
            assert_eq!(sources[1].status, SourceStatus::Ended);
            assert_eq!(sources[1].location, SourceLocation::Global);
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

        // On systems with dirs::home_dir(), this should return a path
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

        // On systems with dirs::home_dir(), this should return a path
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

        // Remove marker
        remove_marker_for_context("test", &discovery).unwrap();
        assert!(!marker_path.exists(), "marker should be removed");
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_stale_marker_cleaned_on_create() {
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().to_path_buf();
        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        // Simulate a stale marker from a killed process (PID that doesn't exist)
        let sources = project_root.join(".lazytail").join("sources");
        fs::create_dir_all(&sources).unwrap();
        fs::write(sources.join("test"), "999999999\n").unwrap();

        // Creating a new marker should succeed by cleaning the stale one
        create_marker_for_context("test", &discovery).unwrap();

        // Marker should contain our PID now
        let contents = fs::read_to_string(sources.join("test")).unwrap();
        assert_eq!(contents.trim(), std::process::id().to_string());

        // Cleanup
        remove_marker_for_context("test", &discovery).unwrap();
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_discover_sources_for_context_project_before_global() {
        let _lock = HOME_MUTEX.lock().unwrap();
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();

        let old_home = env::var("HOME").ok();
        env::set_var("HOME", temp.path());

        // Create project data directory
        let project_data = project_root.join(".lazytail").join("data");
        fs::create_dir_all(&project_data).unwrap();

        // Create global data directory
        let global_data = temp.path().join(".config").join("lazytail").join("data");
        fs::create_dir_all(&global_data).unwrap();

        // Create sources in both locations
        fs::write(project_data.join("proj-source.log"), "project").unwrap();
        fs::write(global_data.join("glob-source.log"), "global").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        let sources = discover_sources_for_context(&discovery).unwrap();

        // Should have 2 sources
        assert_eq!(sources.len(), 2);

        // Project source should be first
        assert_eq!(sources[0].name, "proj-source");
        assert_eq!(sources[0].location, SourceLocation::Project);

        // Global source should be second
        assert_eq!(sources[1].name, "glob-source");
        assert_eq!(sources[1].location, SourceLocation::Global);

        if let Some(home) = old_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_discover_sources_for_context_project_shadows_global() {
        let _lock = HOME_MUTEX.lock().unwrap();
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();

        let old_home = env::var("HOME").ok();
        env::set_var("HOME", temp.path());

        // Create project data directory
        let project_data = project_root.join(".lazytail").join("data");
        fs::create_dir_all(&project_data).unwrap();

        // Create global data directory
        let global_data = temp.path().join(".config").join("lazytail").join("data");
        fs::create_dir_all(&global_data).unwrap();

        // Create same-named source in both locations
        fs::write(project_data.join("shared.log"), "project version").unwrap();
        fs::write(global_data.join("shared.log"), "global version").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        let sources = discover_sources_for_context(&discovery).unwrap();

        // Should have 2 sources — same name in both locations, both visible
        assert_eq!(sources.len(), 2);
        assert_eq!(sources[0].name, "shared");
        assert_eq!(sources[0].location, SourceLocation::Project);
        assert_eq!(sources[1].name, "shared");
        assert_eq!(sources[1].location, SourceLocation::Global);

        if let Some(home) = old_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }

    #[test]
    #[ignore] // Slow test - requires temp dir setup
    fn test_discover_sources_for_context_empty_project_dir() {
        let _lock = HOME_MUTEX.lock().unwrap();
        let temp = TempDir::new().unwrap();
        let project_root = temp.path().join("project");
        fs::create_dir_all(&project_root).unwrap();

        let old_home = env::var("HOME").ok();
        env::set_var("HOME", temp.path());

        // Create empty project data directory
        let project_data = project_root.join(".lazytail").join("data");
        fs::create_dir_all(&project_data).unwrap();

        // Create global data directory with a source
        let global_data = temp.path().join(".config").join("lazytail").join("data");
        fs::create_dir_all(&global_data).unwrap();
        fs::write(global_data.join("global.log"), "global").unwrap();

        let discovery = DiscoveryResult {
            project_root: Some(project_root.clone()),
            project_config: Some(project_root.join("lazytail.yaml")),
            global_config: None,
        };

        let sources = discover_sources_for_context(&discovery).unwrap();

        // Should only have global source
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].name, "global");
        assert_eq!(sources[0].location, SourceLocation::Global);

        if let Some(home) = old_home {
            env::set_var("HOME", home);
        } else {
            env::remove_var("HOME");
        }
    }
}
