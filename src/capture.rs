//! Capture mode for lazytail.
//!
//! When run with `-n <name>`, lazytail acts as a tee-like utility:
//! - Reads from stdin
//! - Writes to project-local or global data directory based on context
//! - Echoes to stdout
//! - Creates a marker file for source discovery
//! - Cleans up marker on exit (EOF or signal)

use crate::config::DiscoveryResult;
use crate::index::builder::{now_millis, LineIndexer};
use crate::signal::setup_shutdown_handlers;
use crate::source::{
    create_marker_for_context, ensure_directories_for_context, index_dir_for_log,
    remove_marker_for_context, resolve_data_dir, validate_source_name,
};
use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::Ordering;

/// Run in capture mode: tee stdin to a named log file.
///
/// This function:
/// 1. Validates the source name
/// 2. Ensures directories exist using discovery context
/// 3. Checks for name collision (active source with same name)
/// 4. Creates a marker file with the current PID
/// 5. Sets up signal handlers for cleanup
/// 6. Opens/creates the log file
/// 7. Reads stdin line by line, writing to both log file and stdout
/// 8. Cleans up the marker on EOF or signal
///
/// The discovery context determines where files are stored:
/// - If inside a project (lazytail.yaml found): `.lazytail/data/`
/// - Otherwise: `~/.config/lazytail/data/`
pub fn run_capture_mode(name: String, discovery: &DiscoveryResult) -> Result<()> {
    // 1. Validate name
    validate_source_name(&name)?;

    // 2. Ensure directories exist using context
    ensure_directories_for_context(discovery)?;

    // 3. Create marker file with our PID (cleans stale markers, rejects active sources)
    create_marker_for_context(&name, discovery)?;

    // 5. Setup signal handlers (flag-based, supports double Ctrl+C for force quit)
    let shutdown_flag = setup_shutdown_handlers()?;

    // 6. Open/create log file
    let log_path = resolve_data_dir(discovery)
        .context("Could not determine data directory")?
        .join(format!("{}.log", name));

    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

    // 7. Print header to stderr showing storage location
    let location = if discovery.project_root.is_some() {
        "project"
    } else {
        "global"
    };
    eprintln!(
        "Serving \"{}\" -> {} ({})",
        name,
        log_path.display(),
        location
    );

    // 8. Create or resume indexer for columnar index
    let idx_dir = index_dir_for_log(&log_path);
    let mut indexer = if idx_dir.join("meta").exists() {
        LineIndexer::resume(&idx_dir)
            .with_context(|| format!("Failed to resume index at {}", idx_dir.display()))?
    } else {
        LineIndexer::create(&idx_dir)
            .with_context(|| format!("Failed to create index at {}", idx_dir.display()))?
    };

    // 9. Tee loop: read stdin, write to file AND stdout
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let mut reader = BufReader::new(stdin.lock());
    let mut line_buf = String::new();
    let mut last_sync = std::time::Instant::now();

    loop {
        // Check for shutdown signal
        if shutdown_flag.load(Ordering::SeqCst) {
            break;
        }

        line_buf.clear();
        match reader.read_line(&mut line_buf) {
            Ok(0) => break, // EOF
            Ok(_) => {
                // Write raw bytes to log file (already includes \n)
                if let Err(e) = log_file.write_all(line_buf.as_bytes()) {
                    eprintln!("Error writing to log file: {}", e);
                    break;
                }
                if let Err(e) = log_file.flush() {
                    eprintln!("Error flushing log file: {}", e);
                    break;
                }

                // Index the raw line (delimiter auto-detected)
                let ts = now_millis();
                if let Err(e) = indexer.push_line(line_buf.as_bytes(), ts) {
                    eprintln!("Warning: failed to index line: {}", e);
                }

                // Periodically sync index to disk so the TUI can pick up columnar offsets
                if last_sync.elapsed() >= std::time::Duration::from_millis(500) {
                    last_sync = std::time::Instant::now();
                    if let Err(e) = indexer.sync(&idx_dir) {
                        eprintln!("Warning: failed to sync index: {}", e);
                    }
                }

                // Echo to stdout (ignore errors - stdout might be closed)
                let _ = stdout.write_all(line_buf.as_bytes());
                let _ = stdout.flush();
            }
            Err(e) => {
                eprintln!("Error reading from stdin: {}", e);
                break;
            }
        }
    }

    // 10. Finalize index before cleanup
    if let Err(e) = indexer.finish(&idx_dir) {
        eprintln!("Warning: failed to finalize index: {}", e);
    }

    // 11. Cleanup on EOF or signal - always reached (no process::exit in signal handler)
    remove_marker_for_context(&name, discovery)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::source::validate_source_name;

    #[test]
    fn test_validate_name() {
        assert!(validate_source_name("valid").is_ok());
        assert!(validate_source_name("valid-name").is_ok());
        assert!(validate_source_name("valid_name_123").is_ok());

        assert!(validate_source_name("").is_err());
        assert!(validate_source_name("bad/name").is_err());
        assert!(validate_source_name(".hidden").is_err());
    }
}
