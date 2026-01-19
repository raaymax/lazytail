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
