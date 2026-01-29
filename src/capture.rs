//! Capture mode for lazytail.
//!
//! When run with `-n <name>`, lazytail acts as a tee-like utility:
//! - Reads from stdin
//! - Writes to ~/.config/lazytail/data/<name>.log
//! - Echoes to stdout
//! - Creates a marker file for source discovery
//! - Cleans up marker on exit (EOF or signal)

use crate::source::{
    check_source_status, create_marker, data_dir, ensure_directories, remove_marker,
    validate_source_name, SourceStatus,
};
use anyhow::{Context, Result};
use signal_hook::consts::{SIGINT, SIGTERM};
use signal_hook::iterator::Signals;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;

/// Run in capture mode: tee stdin to a named log file.
///
/// This function:
/// 1. Validates the source name
/// 2. Checks for name collision (active source with same name)
/// 3. Creates a marker file with the current PID
/// 4. Sets up signal handlers for cleanup
/// 5. Opens/creates the log file
/// 6. Reads stdin line by line, writing to both log file and stdout
/// 7. Cleans up the marker on EOF or signal
pub fn run_capture_mode(name: String) -> Result<()> {
    // 1. Validate name
    validate_source_name(&name)?;

    // 2. Ensure directories exist
    ensure_directories()?;

    // 3. Check for collision
    if check_source_status(&name) == SourceStatus::Active {
        anyhow::bail!(
            "Source '{}' is already active (another process is writing to it)",
            name
        );
    }

    // 4. Create marker file with our PID
    create_marker(&name)?;

    // 5. Setup signal handler for cleanup
    let running = Arc::new(AtomicBool::new(true));
    let name_for_signal = name.clone();
    let running_for_signal = Arc::clone(&running);

    thread::spawn(move || {
        let mut signals = match Signals::new([SIGINT, SIGTERM]) {
            Ok(s) => s,
            Err(_) => return,
        };

        if signals.forever().next().is_some() {
            running_for_signal.store(false, Ordering::SeqCst);
            let _ = remove_marker(&name_for_signal);
            std::process::exit(0);
        }
    });

    // 6. Open/create log file
    let log_path = data_dir()
        .context("Could not determine config directory")?
        .join(format!("{}.log", name));

    let mut log_file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)
        .with_context(|| format!("Failed to open log file: {}", log_path.display()))?;

    // 7. Print header to stderr
    eprintln!("Serving \"{}\" -> {}", name, log_path.display());

    // 8. Tee loop: read stdin, write to file AND stdout
    let stdin = io::stdin();
    let mut stdout = io::stdout();
    let reader = BufReader::new(stdin.lock());

    for line in reader.lines() {
        if !running.load(Ordering::SeqCst) {
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

    // 9. Cleanup on EOF
    remove_marker(&name)?;

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
