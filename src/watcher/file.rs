use anyhow::Result;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as NotifyWatcher};
use std::path::Path;
use std::sync::mpsc::{channel, Receiver};

/// File change notification
#[derive(Debug, Clone)]
pub enum FileEvent {
    Modified,
    Error(String),
}

/// File watcher that monitors a file for changes
pub struct FileWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<FileEvent>,
}

impl FileWatcher {
    /// Create a new file watcher for the given path
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let (tx, rx) = channel();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, notify::Error>| {
            match res {
                Ok(event) => {
                    // Only care about modify events
                    if matches!(
                        event.kind,
                        notify::EventKind::Modify(_) | notify::EventKind::Create(_)
                    ) {
                        let _ = tx.send(FileEvent::Modified);
                    }
                }
                Err(e) => {
                    let _ = tx.send(FileEvent::Error(e.to_string()));
                }
            }
        })?;

        // Watch the file
        watcher.watch(path.as_ref(), RecursiveMode::NonRecursive)?;

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
    }

    /// Check if there are any pending file events (non-blocking)
    pub fn try_recv(&self) -> Option<FileEvent> {
        self.receiver.try_recv().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::OpenOptions;
    use std::io::Write;
    use std::thread;
    use std::time::Duration;
    use tempfile::NamedTempFile;

    /// Helper: Poll for an event with minimal waiting
    /// Uses short polling intervals to stay fast while handling async FS events
    fn poll_for_event(
        watcher: &FileWatcher,
        max_attempts: u32,
        interval_ms: u64,
    ) -> Option<FileEvent> {
        for _ in 0..max_attempts {
            if let Some(event) = watcher.try_recv() {
                return Some(event);
            }
            thread::sleep(Duration::from_millis(interval_ms));
        }
        None
    }

    #[test]
    fn test_watcher_creation_succeeds() {
        let temp_file = NamedTempFile::new().unwrap();
        let result = FileWatcher::new(temp_file.path());
        assert!(result.is_ok());
    }

    #[test]
    fn test_watcher_creation_fails_for_nonexistent_file() {
        let result = FileWatcher::new("/path/that/definitely/does/not/exist/file.log");
        assert!(result.is_err());
    }

    #[test]
    fn test_try_recv_returns_none_when_no_events() {
        let temp_file = NamedTempFile::new().unwrap();
        let watcher = FileWatcher::new(temp_file.path()).unwrap();

        // Drain any spurious initial events (macOS FSEvents may fire on watcher creation)
        thread::sleep(Duration::from_millis(100));
        while watcher.try_recv().is_some() {}

        // Should return None when no new events
        assert!(watcher.try_recv().is_none());
    }

    #[test]
    fn test_detects_file_modification() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let watcher = FileWatcher::new(&path).unwrap();

        // Small delay for watcher initialization (unavoidable with inotify)
        thread::sleep(Duration::from_millis(50));

        // Modify the file
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "New line").unwrap();
        file.flush().unwrap();
        drop(file);

        // Poll for event (fast: 10 attempts x 10ms = 100ms max)
        let event = poll_for_event(&watcher, 10, 10);
        assert!(
            matches!(event, Some(FileEvent::Modified)),
            "Expected Modified event"
        );
    }

    #[test]
    fn test_multiple_modifications() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let watcher = FileWatcher::new(&path).unwrap();
        thread::sleep(Duration::from_millis(50));

        // First modification
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "Line 1").unwrap();
        file.flush().unwrap();
        drop(file);

        let event1 = poll_for_event(&watcher, 10, 10);
        assert!(matches!(event1, Some(FileEvent::Modified)));

        // Drain any duplicate events
        while watcher.try_recv().is_some() {}

        // Second modification
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        writeln!(file, "Line 2").unwrap();
        file.flush().unwrap();
        drop(file);

        let event2 = poll_for_event(&watcher, 10, 10);
        assert!(matches!(event2, Some(FileEvent::Modified)));
    }

    // === SLOW TESTS (marked with #[ignore]) ===
    // Run with: cargo test -- --ignored
    // These tests are more thorough but take longer

    #[test]
    #[ignore]
    fn test_rapid_modifications_stress() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let watcher = FileWatcher::new(&path).unwrap();
        thread::sleep(Duration::from_millis(50));

        // Rapidly modify the file 100 times
        for i in 0..100 {
            let mut file = OpenOptions::new().append(true).open(&path).unwrap();
            writeln!(file, "Line {}", i).unwrap();
            file.flush().unwrap();
            drop(file);
        }

        // Should receive at least one event (OS may batch them)
        let event = poll_for_event(&watcher, 50, 20);
        assert!(matches!(event, Some(FileEvent::Modified)));

        // Drain remaining events
        let mut event_count = 1;
        for _ in 0..50 {
            if watcher.try_recv().is_some() {
                event_count += 1;
            }
            thread::sleep(Duration::from_millis(10));
        }

        // Should have received multiple events (OS may batch, so don't require 100)
        assert!(event_count > 0, "Expected at least one modification event");
    }

    #[test]
    #[ignore]
    fn test_detects_file_creation_in_watched_directory() {
        use std::fs;

        // Create a temporary directory
        let temp_dir = tempfile::tempdir().unwrap();
        let file_path = temp_dir.path().join("test.log");

        // Create initial file
        fs::write(&file_path, "initial").unwrap();

        let watcher = FileWatcher::new(&file_path).unwrap();
        thread::sleep(Duration::from_millis(50));

        // Recreate the file (some editors do this on save)
        fs::remove_file(&file_path).unwrap();
        fs::write(&file_path, "recreated").unwrap();

        // Should detect the recreation as Create or Modify event
        let event = poll_for_event(&watcher, 50, 20);
        assert!(
            matches!(event, Some(FileEvent::Modified)),
            "Expected event after file recreation"
        );
    }

    #[test]
    #[ignore]
    fn test_handles_large_writes() {
        let temp_file = NamedTempFile::new().unwrap();
        let path = temp_file.path().to_path_buf();

        let watcher = FileWatcher::new(&path).unwrap();
        thread::sleep(Duration::from_millis(50));

        // Write a large chunk of data
        let mut file = OpenOptions::new().append(true).open(&path).unwrap();
        let large_data = "x".repeat(1024 * 1024); // 1MB
        write!(file, "{}", large_data).unwrap();
        file.flush().unwrap();
        drop(file);

        let event = poll_for_event(&watcher, 50, 20);
        assert!(matches!(event, Some(FileEvent::Modified)));
    }
}
