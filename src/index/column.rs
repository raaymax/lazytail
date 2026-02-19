use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::marker::PhantomData;
use std::path::Path;

use anyhow::{Context, Result};
use memmap2::Mmap;

/// Trait for fixed-size column elements with little-endian serialization.
pub trait ColumnElement: Copy + Default + 'static {
    const SIZE: usize;
    fn write_le(&self, buf: &mut [u8]);
    fn read_le(buf: &[u8]) -> Self;
}

impl ColumnElement for u16 {
    const SIZE: usize = 2;
    fn write_le(&self, buf: &mut [u8]) {
        buf[..2].copy_from_slice(&self.to_le_bytes());
    }
    fn read_le(buf: &[u8]) -> Self {
        u16::from_le_bytes([buf[0], buf[1]])
    }
}

impl ColumnElement for u32 {
    const SIZE: usize = 4;
    fn write_le(&self, buf: &mut [u8]) {
        buf[..4].copy_from_slice(&self.to_le_bytes());
    }
    fn read_le(buf: &[u8]) -> Self {
        u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]])
    }
}

impl ColumnElement for u64 {
    const SIZE: usize = 8;
    fn write_le(&self, buf: &mut [u8]) {
        buf[..8].copy_from_slice(&self.to_le_bytes());
    }
    fn read_le(buf: &[u8]) -> Self {
        u64::from_le_bytes([
            buf[0], buf[1], buf[2], buf[3], buf[4], buf[5], buf[6], buf[7],
        ])
    }
}

/// Append-only writer for a typed column file.
pub struct ColumnWriter<T: ColumnElement> {
    writer: BufWriter<File>,
    _phantom: PhantomData<T>,
}

impl<T: ColumnElement> ColumnWriter<T> {
    /// Create a new column file, truncating any existing content.
    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let file = File::create(path.as_ref())
            .with_context(|| format!("creating column file: {}", path.as_ref().display()))?;
        Ok(Self {
            writer: BufWriter::new(file),
            _phantom: PhantomData,
        })
    }

    /// Open an existing column file for appending.
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .open(path.as_ref())
            .with_context(|| {
                format!(
                    "opening column file for append: {}",
                    path.as_ref().display()
                )
            })?;
        Ok(Self {
            writer: BufWriter::new(file),
            _phantom: PhantomData,
        })
    }

    /// Append a single value.
    pub fn push(&mut self, value: T) -> Result<()> {
        let mut buf = [0u8; 8];
        value.write_le(&mut buf[..T::SIZE]);
        self.writer.write_all(&buf[..T::SIZE])?;
        Ok(())
    }

    /// Append multiple values.
    pub fn push_batch(&mut self, values: &[T]) -> Result<()> {
        let mut buf = [0u8; 8];
        for value in values {
            value.write_le(&mut buf[..T::SIZE]);
            self.writer.write_all(&buf[..T::SIZE])?;
        }
        Ok(())
    }

    /// Flush buffered writes to disk.
    pub fn flush(&mut self) -> Result<()> {
        self.writer.flush()?;
        Ok(())
    }
}

/// Mmap-based zero-copy reader for a typed column file.
pub struct ColumnReader<T: ColumnElement> {
    mmap: Option<Mmap>,
    entry_count: usize,
    _phantom: PhantomData<T>,
}

impl<T: ColumnElement> ColumnReader<T> {
    /// Open a column file and mmap it. Clamps to `min(expected, file_entries)`.
    pub fn open(path: impl AsRef<Path>, expected_entries: usize) -> Result<Self> {
        let file = File::open(path.as_ref())
            .with_context(|| format!("opening column file: {}", path.as_ref().display()))?;
        let metadata = file.metadata()?;
        let file_size = metadata.len() as usize;

        if file_size == 0 {
            return Ok(Self {
                mmap: None,
                entry_count: 0,
                _phantom: PhantomData,
            });
        }

        let mmap = unsafe { Mmap::map(&file)? };
        let file_entries = mmap.len() / T::SIZE;
        let entry_count = expected_entries.min(file_entries);

        Ok(Self {
            mmap: Some(mmap),
            entry_count,
            _phantom: PhantomData,
        })
    }

    /// Read entry at `index`, returning `None` if out of bounds.
    pub fn get(&self, index: usize) -> Option<T> {
        if index >= self.entry_count {
            return None;
        }
        let mmap = self.mmap.as_ref()?;
        let offset = index * T::SIZE;
        Some(T::read_le(&mmap[offset..offset + T::SIZE]))
    }

    /// Return a raw byte slice for entries `[start..end)`.
    pub fn raw_slice(&self, start: usize, end: usize) -> Option<&[u8]> {
        if start > end || end > self.entry_count {
            return None;
        }
        let mmap = self.mmap.as_ref()?;
        let byte_start = start * T::SIZE;
        let byte_end = end * T::SIZE;
        Some(&mmap[byte_start..byte_end])
    }

    pub fn len(&self) -> usize {
        self.entry_count
    }

    pub fn is_empty(&self) -> bool {
        self.entry_count == 0
    }

    pub fn iter(&self) -> ColumnIter<'_, T> {
        ColumnIter {
            reader: self,
            index: 0,
        }
    }
}

pub struct ColumnIter<'a, T: ColumnElement> {
    reader: &'a ColumnReader<T>,
    index: usize,
}

impl<T: ColumnElement> Iterator for ColumnIter<'_, T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let value = self.reader.get(self.index)?;
        self.index += 1;
        Some(value)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let remaining = self.reader.entry_count.saturating_sub(self.index);
        (remaining, Some(remaining))
    }
}

impl<T: ColumnElement> ExactSizeIterator for ColumnIter<'_, T> {}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // --- ColumnElement trait ---

    #[test]
    fn u16_element_roundtrip() {
        let mut buf = [0u8; 2];
        for val in [0u16, 1, 255, 1000, u16::MAX] {
            val.write_le(&mut buf);
            assert_eq!(u16::read_le(&buf), val);
        }
    }

    #[test]
    fn u32_element_roundtrip() {
        let mut buf = [0u8; 4];
        for val in [0u32, 1, 255, 70_000, u32::MAX] {
            val.write_le(&mut buf);
            assert_eq!(u32::read_le(&buf), val);
        }
    }

    #[test]
    fn u64_element_roundtrip() {
        let mut buf = [0u8; 8];
        for val in [0u64, 1, u32::MAX as u64 + 1, u64::MAX] {
            val.write_le(&mut buf);
            assert_eq!(u64::read_le(&buf), val);
        }
    }

    #[test]
    fn endianness_u16() {
        let mut buf = [0u8; 2];
        0x0102u16.write_le(&mut buf);
        assert_eq!(buf, [0x02, 0x01]); // little-endian
    }

    #[test]
    fn endianness_u32() {
        let mut buf = [0u8; 4];
        0x01020304u32.write_le(&mut buf);
        assert_eq!(buf, [0x04, 0x03, 0x02, 0x01]);
    }

    #[test]
    fn endianness_u64() {
        let mut buf = [0u8; 8];
        0x0102030405060708u64.write_le(&mut buf);
        assert_eq!(buf, [0x08, 0x07, 0x06, 0x05, 0x04, 0x03, 0x02, 0x01]);
    }

    // --- Write + Read ---

    #[test]
    fn write_read_u32() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("flags.col");

        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push(42).unwrap();
        writer.push(100).unwrap();
        writer.push(0).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 3).unwrap();
        assert_eq!(reader.len(), 3);
        assert_eq!(reader.get(0), Some(42));
        assert_eq!(reader.get(1), Some(100));
        assert_eq!(reader.get(2), Some(0));
        assert_eq!(reader.get(3), None);
    }

    #[test]
    fn write_read_u64() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("offsets.col");

        let mut writer = ColumnWriter::<u64>::create(&path).unwrap();
        writer.push(0).unwrap();
        writer.push(1024).unwrap();
        writer.push(u64::MAX).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u64>::open(&path, 3).unwrap();
        assert_eq!(reader.len(), 3);
        assert_eq!(reader.get(0), Some(0));
        assert_eq!(reader.get(1), Some(1024));
        assert_eq!(reader.get(2), Some(u64::MAX));
    }

    #[test]
    fn write_read_u16() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("templates.col");

        let mut writer = ColumnWriter::<u16>::create(&path).unwrap();
        writer.push(0).unwrap();
        writer.push(1234).unwrap();
        writer.push(u16::MAX).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u16>::open(&path, 3).unwrap();
        assert_eq!(reader.len(), 3);
        assert_eq!(reader.get(0), Some(0));
        assert_eq!(reader.get(1), Some(1234));
        assert_eq!(reader.get(2), Some(u16::MAX));
    }

    #[test]
    fn empty_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("empty.col");

        // Create an empty file
        let writer = ColumnWriter::<u32>::create(&path).unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 100).unwrap();
        assert_eq!(reader.len(), 0);
        assert!(reader.is_empty());
        assert_eq!(reader.get(0), None);
    }

    #[test]
    fn partial_file_clamped() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("partial.col");

        // Write 3 entries but tell reader to expect 100
        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push(1).unwrap();
        writer.push(2).unwrap();
        writer.push(3).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 100).unwrap();
        assert_eq!(reader.len(), 3); // clamped to actual file size
        assert_eq!(reader.get(0), Some(1));
        assert_eq!(reader.get(2), Some(3));
        assert_eq!(reader.get(3), None);
    }

    #[test]
    fn expected_less_than_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("excess.col");

        // Write 5 entries but tell reader to expect only 2
        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        for i in 0..5u32 {
            writer.push(i).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 2).unwrap();
        assert_eq!(reader.len(), 2); // clamped to expected
        assert_eq!(reader.get(0), Some(0));
        assert_eq!(reader.get(1), Some(1));
        assert_eq!(reader.get(2), None);
    }

    #[test]
    fn batch_write() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("batch.col");

        let values: Vec<u32> = (0..100).collect();
        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push_batch(&values).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 100).unwrap();
        assert_eq!(reader.len(), 100);
        assert_eq!(reader.get(0), Some(0));
        assert_eq!(reader.get(50), Some(50));
        assert_eq!(reader.get(99), Some(99));
    }

    #[test]
    fn large_dataset() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("large.col");

        let count = 10_000u32;
        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        for i in 0..count {
            writer.push(i).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, count as usize).unwrap();
        assert_eq!(reader.len(), count as usize);
        assert_eq!(reader.get(0), Some(0));
        assert_eq!(reader.get(5000), Some(5000));
        assert_eq!(reader.get(9999), Some(9999));
        assert_eq!(reader.get(10_000), None);
    }

    #[test]
    fn out_of_bounds() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("oob.col");

        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push(42).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 1).unwrap();
        assert_eq!(reader.get(0), Some(42));
        assert_eq!(reader.get(1), None);
        assert_eq!(reader.get(usize::MAX), None);
    }

    #[test]
    fn iterator() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("iter.col");

        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        for i in 0..5u32 {
            writer.push(i * 10).unwrap();
        }
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 5).unwrap();
        let collected: Vec<u32> = reader.iter().collect();
        assert_eq!(collected, vec![0, 10, 20, 30, 40]);
        assert_eq!(reader.iter().len(), 5);
    }

    #[test]
    fn raw_slice() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("raw.col");

        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push(0x01020304).unwrap();
        writer.push(0x05060708).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 2).unwrap();
        let slice = reader.raw_slice(0, 2).unwrap();
        assert_eq!(slice.len(), 8);

        // First entry: 0x01020304 in LE = [04, 03, 02, 01]
        assert_eq!(&slice[0..4], &[0x04, 0x03, 0x02, 0x01]);

        // Invalid ranges
        assert!(reader.raw_slice(1, 0).is_none()); // start > end
        assert!(reader.raw_slice(0, 3).is_none()); // end > entry_count
    }

    #[test]
    fn open_append() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("append.col");

        // Write initial entries
        let mut writer = ColumnWriter::<u32>::create(&path).unwrap();
        writer.push(1).unwrap();
        writer.push(2).unwrap();
        writer.flush().unwrap();
        drop(writer);

        // Append more entries
        let mut writer = ColumnWriter::<u32>::open(&path).unwrap();
        writer.push(3).unwrap();
        writer.push(4).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let reader = ColumnReader::<u32>::open(&path, 4).unwrap();
        assert_eq!(reader.len(), 4);
        assert_eq!(reader.get(0), Some(1));
        assert_eq!(reader.get(1), Some(2));
        assert_eq!(reader.get(2), Some(3));
        assert_eq!(reader.get(3), Some(4));
    }
}
