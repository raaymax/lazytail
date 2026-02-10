//! Capture mode for lazytail.
//!
//! When run with `-n <name>`, lazytail acts as a tee-like utility:
//! - Reads from stdin
//! - Writes to project-local or global data directory based on context
//! - Echoes to stdout
//! - Creates a marker file for source discovery
//! - Cleans up marker on exit (EOF or signal)

use crate::config::DiscoveryResult;
use crate::signal::setup_shutdown_handlers;
use crate::source::{
    check_source_status_for_context, create_marker_for_context, ensure_directories_for_context,
    remove_marker_for_context, resolve_data_dir, validate_source_name, SourceStatus,
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

    // 3. Check for collision
    if check_source_status_for_context(&name, discovery) == SourceStatus::Active {
        anyhow::bail!(
            "Source '{}' is already active (another process is writing to it)",
            name
        );
    }

    // 4. Create marker file with our PID
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

    // 8. Tee loop: read stdin, write to file AND stdout
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        // Check for shutdown signal
        if shutdown_flag.load(Ordering::SeqCst) {
            break;
        }

        match line {
            Ok(line) => {
                // Write to log file
                if let Err(e) = writeln!(log_file, "{}", line) {
                    eprintln!("Error writing to log file: {}", e);
                    break;
                }
                if let Err(e) = log_file.flush() {
                    eprintln!("Error flushing log file: {}", e);
                    break;
                }

                // Echo to stdout (ignore errors - stdout might be closed)
                let _ = writeln!(stdout, "{}", line);
                let _ = stdout.flush();
            }
            Err(e) => {
                eprintln!("Error reading from stdin: {}", e);
                break;
            }
        }
    }

    // 9. Cleanup on EOF or signal - always reached (no process::exit in signal handler)
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
