//! Signal handling infrastructure for graceful shutdown.
//!
//! This module provides flag-based signal handling using `signal-hook::flag`.
//! It supports:
//! - Graceful shutdown on SIGINT/SIGTERM (sets flag, allows cleanup)
//! - Force quit on double Ctrl+C (immediate exit with code 1)

use signal_hook::consts::TERM_SIGNALS;
use signal_hook::flag;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Set up signal handlers for graceful shutdown.
///
/// Returns an `Arc<AtomicBool>` that becomes `true` when a termination signal
/// (SIGINT, SIGTERM) is received. The caller should check this flag periodically
/// and perform cleanup when it becomes true.
///
/// On receiving a second signal while the flag is already true, the process
/// exits immediately with code 1 (force quit behavior).
///
/// # Example
///
/// ```ignore
/// let shutdown_flag = setup_shutdown_handlers()?;
///
/// loop {
///     if shutdown_flag.load(Ordering::SeqCst) {
///         break; // Perform cleanup and exit
///     }
///     // ... do work ...
/// }
/// ```
pub fn setup_shutdown_handlers() -> Result<Arc<AtomicBool>, std::io::Error> {
    let term_now = Arc::new(AtomicBool::new(false));

    for sig in TERM_SIGNALS {
        // First: register conditional shutdown (exits with code 1 on second signal)
        // This only triggers if term_now is already true
        flag::register_conditional_shutdown(*sig, 1, Arc::clone(&term_now))?;

        // Second: set term_now to true, arming the conditional shutdown for next signal
        flag::register(*sig, Arc::clone(&term_now))?;
    }

    Ok(term_now)
}
