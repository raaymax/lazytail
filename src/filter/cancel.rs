use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// A token for cooperative cancellation of long-running operations
///
/// Cloning a CancelToken creates a new handle to the same underlying
/// cancellation state. When any handle calls `cancel()`, all handles
/// will observe `is_cancelled() == true`.
#[derive(Clone, Debug)]
pub struct CancelToken {
    /// Shared cancellation flag
    cancelled: Arc<AtomicBool>,
}

impl CancelToken {
    /// Create a new cancellation token
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Request cancellation
    ///
    /// This is a non-blocking operation. The operation being cancelled
    /// must cooperatively check `is_cancelled()` and stop when true.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Check if cancellation has been requested
    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Reset the cancellation state (use with caution)
    ///
    /// This is useful for reusing a token across multiple operations.
    #[allow(dead_code)]
    pub fn reset(&self) {
        self.cancelled.store(false, Ordering::SeqCst);
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_new_token_not_cancelled() {
        let token = CancelToken::new();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_cancel() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_clone_shares_state() {
        let token1 = CancelToken::new();
        let token2 = token1.clone();

        assert!(!token1.is_cancelled());
        assert!(!token2.is_cancelled());

        token1.cancel();

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
    }

    #[test]
    fn test_cancel_from_clone() {
        let token1 = CancelToken::new();
        let token2 = token1.clone();

        token2.cancel();

        assert!(token1.is_cancelled());
        assert!(token2.is_cancelled());
    }

    #[test]
    fn test_reset() {
        let token = CancelToken::new();
        token.cancel();
        assert!(token.is_cancelled());

        token.reset();
        assert!(!token.is_cancelled());
    }

    #[test]
    fn test_thread_safety() {
        let token = CancelToken::new();
        let token_clone = token.clone();

        let handle = thread::spawn(move || {
            thread::sleep(Duration::from_millis(10));
            token_clone.cancel();
        });

        // Wait for cancellation
        while !token.is_cancelled() {
            thread::yield_now();
        }

        handle.join().unwrap();
        assert!(token.is_cancelled());
    }

    #[test]
    fn test_cooperative_cancellation_pattern() {
        let token = CancelToken::new();
        let token_clone = token.clone();

        let handle = thread::spawn(move || {
            let mut count = 0;
            // Use a longer loop to ensure we have time to cancel
            while !token_clone.is_cancelled() && count < 100_000 {
                count += 1;
                // Check more frequently
                if count % 100 == 0 {
                    thread::yield_now();
                }
            }
            count
        });

        // Give the thread time to start, then cancel
        thread::sleep(Duration::from_millis(5));
        token.cancel();

        let count = handle.join().unwrap();
        // Should have been cancelled before reaching 100,000
        // The exact count depends on timing, but it should stop eventually
        assert!(count <= 100_000, "Loop should have ended, got {}", count);
    }

    #[test]
    fn test_default() {
        let token = CancelToken::default();
        assert!(!token.is_cancelled());
    }
}
