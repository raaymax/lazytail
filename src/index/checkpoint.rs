use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use anyhow::{Context, Result};
use memmap2::Mmap;

const CHECKPOINT_SIZE: usize = 64;

/// Cumulative severity counts up to a checkpoint position.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SeverityCounts {
    pub unknown: u32,
    pub trace: u32,
    pub debug: u32,
    pub info: u32,
    pub warn: u32,
    pub error: u32,
    pub fatal: u32,
}

/// A 64-byte checkpoint entry for granular validation and approximate stats.
///
/// Written once per `checkpoint_interval` lines. Contains the log position,
/// a content hash (computed externally), and cumulative severity counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Checkpoint {
    pub line_number: u64,
    pub byte_offset: u64,
    pub content_hash: u64,
    pub index_timestamp: u64,
    pub severity_counts: SeverityCounts,
    // 4 bytes reserved (implicit zeros)
}

impl Checkpoint {
    pub fn to_bytes(self) -> [u8; CHECKPOINT_SIZE] {
        let mut buf = [0u8; CHECKPOINT_SIZE];
        buf[0..8].copy_from_slice(&self.line_number.to_le_bytes());
        buf[8..16].copy_from_slice(&self.byte_offset.to_le_bytes());
        buf[16..24].copy_from_slice(&self.content_hash.to_le_bytes());
        buf[24..32].copy_from_slice(&self.index_timestamp.to_le_bytes());
        buf[32..36].copy_from_slice(&self.severity_counts.unknown.to_le_bytes());
        buf[36..40].copy_from_slice(&self.severity_counts.trace.to_le_bytes());
        buf[40..44].copy_from_slice(&self.severity_counts.debug.to_le_bytes());
        buf[44..48].copy_from_slice(&self.severity_counts.info.to_le_bytes());
        buf[48..52].copy_from_slice(&self.severity_counts.warn.to_le_bytes());
        buf[52..56].copy_from_slice(&self.severity_counts.error.to_le_bytes());
        buf[56..60].copy_from_slice(&self.severity_counts.fatal.to_le_bytes());
        // bytes 60..64 reserved (zeros)
        buf
    }

    pub fn from_bytes(buf: &[u8; CHECKPOINT_SIZE]) -> Self {
        Self {
            line_number: u64::from_le_bytes(buf[0..8].try_into().unwrap()),
            byte_offset: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            content_hash: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            index_timestamp: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            severity_counts: SeverityCounts {
                unknown: u32::from_le_bytes(buf[32..36].try_into().unwrap()),
                trace: u32::from_le_bytes(buf[36..40].try_into().unwrap()),
                debug: u32::from_le_bytes(buf[40..44].try_into().unwrap()),
                info: u32::from_le_bytes(buf[44..48].try_into().unwrap()),
                warn: u32::from_le_bytes(buf[48..52].try_into().unwrap()),
                error: u32::from_le_bytes(buf[52..56].try_into().unwrap()),
                fatal: u32::from_le_bytes(buf[56..60].try_into().unwrap()),
            },
        }
    }
}

/// Append-only writer for checkpoint entries.
pub struct CheckpointWriter {
    writer: BufWriter<File>,
}

impl CheckpointWriter {
    /// Create a new checkpoint file, truncating any existing content.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::create(path.as_ref())
            .with_context(|| format!("creating checkpoint file: {}", path.as_ref().display()))?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    /// Open an existing checkpoint file for appending.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .open(path.as_ref())
            .with_context(|| {
                format!(
                    "opening checkpoint file for append: {}",
                    path.as_ref().display()
                )
            })?;
        Ok(Self {
            writer: BufWriter::new(file),
        })
    }

    /// Append a checkpoint entry.
    pub fn push(&mut self, checkpoint: &Checkpoint) -> Result<()> {
        self.writer.write_all(&checkpoint.to_bytes())?;
        Ok(())
    }

    /// Flush buffered writes to disk.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

/// Mmap-based reader for checkpoint entries.
pub struct CheckpointReader {
    mmap: Option<Mmap>,
    entry_count: usize,
}

impl CheckpointReader {
    /// Open and mmap a checkpoint file. Entry count is derived from file size.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::open(path.as_ref())
            .with_context(|| format!("opening checkpoint file: {}", path.as_ref().display()))?;
        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        if file_size == 0 {
            return Ok(Self {
                mmap: None,
                entry_count: 0,
            });
        }

        let mmap = unsafe { Mmap::map(&file)? };
        let entry_count = mmap.len() / CHECKPOINT_SIZE;

        Ok(Self {
            mmap: Some(mmap),
            entry_count,
        })
    }

    /// Read checkpoint at `index`, returning `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<Checkpoint> {
        if index >= self.entry_count {
            return None;
        }
        let mmap = self.mmap.as_ref()?;
        let offset = index * CHECKPOINT_SIZE;
        let buf: [u8; CHECKPOINT_SIZE] = mmap[offset..offset + CHECKPOINT_SIZE].try_into().unwrap();
        Some(Checkpoint::from_bytes(&buf))
    }

    /// Return the last checkpoint, or `None` if empty.
    pub fn last(&self) -> Option<Checkpoint> {
        if self.entry_count == 0 {
            return None;
        }
        self.get(self.entry_count - 1)
    }

    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    pub fn iter(&self) -> CheckpointIter<'_> {
        CheckpointIter {
            reader: self,
            index: 0,
        }
    }
}

pub struct CheckpointIter<'a> {
    reader: &'a CheckpointReader,
    index: usize,
}

impl Iterator for CheckpointIter<'_> {
    type Item = Checkpoint;

    fn next(&mut self) -> Option<Checkpoint> {
        let value = self.reader.get(self.index)?;
        self.index += 1;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.reader.entry_count.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl ExactSizeIterator for CheckpointIter<'_> {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn sample_checkpoint(line: u64) -> Checkpoint {
        Checkpoint {
            line_number: line,
            byte_offset: line * 150,
            content_hash: 0xDEAD_BEEF_CAFE_BABE,
            index_timestamp: 1_700_000_000_000,
            severity_counts: SeverityCounts {
                unknown: 1000,
                trace: 500,
                debug: 2000,
                info: 5000,
                warn: 300,
                error: 50,
                fatal: 2,
            },
        }
    }

    #[test]
    fn checkpoint_roundtrip() {
        let cp = sample_checkpoint(100_000);
        let bytes = cp.to_bytes();
        let restored = Checkpoint::from_bytes(&bytes);
        assert_eq!(cp, restored);
    }

    #[test]
    fn byte_layout() {
        let cp = Checkpoint {
            line_number: 0x0102030405060708,
            byte_offset: 0,
            content_hash: 0,
            index_timestamp: 0,
            severity_counts: SeverityCounts::default(),
        };
        let bytes = cp.to_bytes();
        // line_number at offset 0, little-endian
        assert_eq!(&bytes[0..8], &0x0102030405060708u64.to_le_bytes());
        // reserved at offset 60..64 should be zero
        assert_eq!(&bytes[60..64], &[0, 0, 0, 0]);
    }

    #[test]
    fn severity_counts_layout() {
        let cp = Checkpoint {
            line_number: 0,
            byte_offset: 0,
            content_hash: 0,
            index_timestamp: 0,
            severity_counts: SeverityCounts {
                unknown: 1,
                trace: 2,
                debug: 3,
                info: 4,
                warn: 5,
                error: 6,
                fatal: 7,
            },
        };
        let bytes = cp.to_bytes();
        assert_eq!(u32::from_le_bytes(bytes[32..36].try_into().unwrap()), 1); // unknown
        assert_eq!(u32::from_le_bytes(bytes[36..40].try_into().unwrap()), 2); // trace
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 3); // debug
        assert_eq!(u32::from_le_bytes(bytes[44..48].try_into().unwrap()), 4); // info
        assert_eq!(u32::from_le_bytes(bytes[48..52].try_into().unwrap()), 5); // warn
        assert_eq!(u32::from_le_bytes(bytes[52..56].try_into().unwrap()), 6); // error
        assert_eq!(u32::from_le_bytes(bytes[56..60].try_into().unwrap()), 7); // fatal
    }

    #[test]
    fn write_and_read() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoints");

        let cp1 = sample_checkpoint(0);
        let cp2 = sample_checkpoint(100_000);

        let mut writer = CheckpointWriter::create(&path).unwrap();
        writer.push(&cp1).unwrap();
        writer.push(&cp2).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = CheckpointReader::open(&path).unwrap();
        assert_eq!(reader.len(), 2);
        assert_eq!(reader.get(0), Some(cp1));
        assert_eq!(reader.get(1), Some(cp2));
        assert_eq!(reader.get(2), None);
    }

    #[test]
    fn empty_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty_checkpoints");

        let writer = CheckpointWriter::create(&path).unwrap();
        drop(writer);

        let reader = CheckpointReader::open(&path).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
        assert_eq!(reader.get(0), None);
        assert_eq!(reader.last(), None);
    }

    #[test]
    fn last() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoints_last");

        let cp1 = sample_checkpoint(0);
        let cp2 = sample_checkpoint(100_000);
        let cp3 = sample_checkpoint(200_000);

        let mut writer = CheckpointWriter::create(&path).unwrap();
        writer.push(&cp1).unwrap();
        writer.push(&cp2).unwrap();
        writer.push(&cp3).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = CheckpointReader::open(&path).unwrap();
        assert_eq!(reader.last(), Some(cp3));
    }

    #[test]
    fn iterator() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoints_iter");

        let checkpoints: Vec<Checkpoint> = (0..5).map(|i| sample_checkpoint(i * 100_000)).collect();

        let mut writer = CheckpointWriter::create(&path).unwrap();
        for cp in &checkpoints {
            writer.push(cp).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let reader = CheckpointReader::open(&path).unwrap();
        let collected: Vec<Checkpoint> = reader.iter().collect();
        assert_eq!(collected, checkpoints);
        assert_eq!(reader.iter().len(), 5);
    }

    #[test]
    fn append_to_existing() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("checkpoints_append");

        let mut writer = CheckpointWriter::create(&path).unwrap();
        writer.push(&sample_checkpoint(0)).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let mut writer = CheckpointWriter::open(&path).unwrap();
        writer.push(&sample_checkpoint(100_000)).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = CheckpointReader::open(&path).unwrap();
        assert_eq!(reader.len(), 2);
        assert_eq!(reader.get(0).unwrap().line_number, 0);
        assert_eq!(reader.get(1).unwrap().line_number, 100_000);
    }
}
