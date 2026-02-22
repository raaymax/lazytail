use std::fs::{File, OpenOptions};
use std::path::Path;

use anyhow::{bail, Result};

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

    /// Acquire, blocking until available. Used by capture mode (only one capture per name
    /// is allowed by the marker check, so this should never actually block in practice).
    pub fn acquire(index_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(index_dir)?;
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(index_dir.join(Self::LOCK_FILE))?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            let ret = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) };
            if ret != 0 {
                bail!("failed to acquire index write lock");
            }
        }
        Ok(Self { _file: file })
    }
}

#[cfg(unix)]
impl Drop for IndexWriteLock {
    fn drop(&mut self) {
        use std::os::unix::io::AsRawFd;
        unsafe {
            libc::flock(self._file.as_raw_fd(), libc::LOCK_UN);
        }
    }
}
