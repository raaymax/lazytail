use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::Result;

/// Advisory exclusive write lock for an index directory.
/// Backed by flock(2) — automatically released if the holding process is killed.
pub struct IndexWriteLock {
    _file: File,
}

impl IndexWriteLock {
    const LOCK_FILE: &'static str = "writer.lock";

    /// Try to acquire without blocking. Returns `None` if another writer holds it.
    pub fn try_acquire(index_dir: &Path) -> Result<Option<Self>> {
        std::fs::create_dir_all(index_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(index_dir.join(Self::LOCK_FILE))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
            if ret != 0 {
                return Ok(None);
            }
        }
        Ok(Some(Self { _file: file }))
    }
}
// No explicit Drop needed — closing the File fd automatically releases the flock.
