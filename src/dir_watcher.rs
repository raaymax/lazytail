//! Directory watcher for detecting new log files in the data directory.
//!
//! Uses the notify crate to watch ~/.config/lazytail/data/ for new .log files.

use anyhow::{Context, Result};
use notify::{
    event::{CreateKind, ModifyKind, RemoveKind},
    Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher,
};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, TryRecvError};

/// Events from the directory watcher
#[derive(Debug, Clone)]
pub enum DirEvent {
    /// A new .log file was created
    NewFile(PathBuf),
    /// A .log file was removed
    FileRemoved(PathBuf),
}

/// Watches a directory for new .log files
pub struct DirectoryWatcher {
    _watcher: RecommendedWatcher,
    receiver: Receiver<DirEvent>,
}

impl DirectoryWatcher {
    /// Create a new directory watcher for the given path.
    ///
    /// Only notifies about .log files being created or removed.
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let (tx, rx) = channel();
        let path_buf = path.as_ref().to_path_buf();

        let mut watcher = notify::recommended_watcher(move |res: Result<Event, _>| {
            if let Ok(event) = res {
                match event.kind {
                    EventKind::Create(CreateKind::File)
                    | EventKind::Modify(ModifyKind::Name(_)) => {
                        // File created or renamed
                        for path in event.paths {
                            if path.extension().map_or(false, |ext| ext == "log") {
                                let _ = tx.send(DirEvent::NewFile(path));
                            }
                        }
                    }
                    EventKind::Remove(RemoveKind::File) => {
                        // File removed
                        for path in event.paths {
                            if path.extension().map_or(false, |ext| ext == "log") {
                                let _ = tx.send(DirEvent::FileRemoved(path));
                            }
                        }
                    }
                    _ => {}
                }
            }
        })
        .context("Failed to create directory watcher")?;

        // Watch the directory (non-recursive since we only care about data/)
        watcher
            .watch(&path_buf, RecursiveMode::NonRecursive)
            .context("Failed to watch directory")?;

        Ok(Self {
            _watcher: watcher,
            receiver: rx,
        })
    }

    /// Try to receive a directory event without blocking.
    pub fn try_recv(&self) -> Option<DirEvent> {
        match self.receiver.try_recv() {
            Ok(event) => Some(event),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::thread;
    use std::time::Duration;
    use tempfile::TempDir;

    #[test]
    #[ignore] // Slow test - involves file system operations
    fn test_directory_watcher_detects_new_file() {
        let temp = TempDir::new().unwrap();
        let watcher = DirectoryWatcher::new(temp.path()).unwrap();

        // Give the watcher time to initialize
        thread::sleep(Duration::from_millis(100));

        // Create a .log file
        fs::write(temp.path().join("test.log"), "test").unwrap();

        // Give the watcher time to detect the file
        thread::sleep(Duration::from_millis(200));

        // Should receive a NewFile event
        let event = watcher.try_recv();
        assert!(matches!(event, Some(DirEvent::NewFile(_))));
    }

    #[test]
    #[ignore] // Slow test - involves file system operations
    fn test_directory_watcher_ignores_non_log_files() {
        let temp = TempDir::new().unwrap();
        let watcher = DirectoryWatcher::new(temp.path()).unwrap();

        // Give the watcher time to initialize
        thread::sleep(Duration::from_millis(100));

        // Create a non-.log file
        fs::write(temp.path().join("test.txt"), "test").unwrap();

        // Give the watcher time to detect (or not detect) the file
        thread::sleep(Duration::from_millis(200));

        // Should NOT receive any event
        let event = watcher.try_recv();
        assert!(event.is_none());
    }
}
