use std::fs::File;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{bail, Context, Result};
use memchr::memchr;
use memmap2::Mmap;

use super::checkpoint::{Checkpoint, CheckpointReader, CheckpointWriter, SeverityCounts};
use super::column::ColumnWriter;
use super::flags::{
    detect_flags_bytes, SEVERITY_DEBUG, SEVERITY_ERROR, SEVERITY_FATAL, SEVERITY_INFO,
    SEVERITY_MASK, SEVERITY_TRACE, SEVERITY_WARN,
};
use super::lock::IndexWriteLock;
use super::meta::{ColumnBit, IndexMeta};

const BATCH: usize = 1024;

pub fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn add_severity(counts: &mut SeverityCounts, severity: u32) {
    match severity {
        SEVERITY_TRACE => counts.trace += 1,
        SEVERITY_DEBUG => counts.debug += 1,
        SEVERITY_INFO => counts.info += 1,
        SEVERITY_WARN => counts.warn += 1,
        SEVERITY_ERROR => counts.error += 1,
        SEVERITY_FATAL => counts.fatal += 1,
        _ => counts.unknown += 1,
    }
}

fn content_hash(data: &[u8], offset: usize, len: usize) -> u64 {
    let end = data.len().min(offset + len);
    if offset >= data.len() {
        return 0;
    }
    xxhash_rust::xxh3::xxh3_64(&data[offset..end])
}

/// Bulk index builder: produces all column files from an existing log file via mmap.
pub struct IndexBuilder {
    checkpoint_interval: u16,
}

impl IndexBuilder {
    pub fn new() -> Self {
        Self {
            checkpoint_interval: 100,
        }
    }

    #[allow(dead_code)]
    pub fn with_checkpoint_interval(mut self, interval: u16) -> Self {
        self.checkpoint_interval = interval;
        self
    }

    pub fn build(self, log_path: &Path, index_dir: &Path) -> Result<IndexMeta> {
        let Some(_lock) = IndexWriteLock::try_acquire(index_dir)? else {
            bail!("index is being written by another process, skipping");
        };

        std::fs::create_dir_all(index_dir)
            .with_context(|| format!("creating index dir: {}", index_dir.display()))?;

        let file = File::open(log_path)
            .with_context(|| format!("opening log file: {}", log_path.display()))?;
        let file_meta = file.metadata()?;
        let file_size = file_meta.len();

        // Handle empty file
        if file_size == 0 {
            let mut meta = IndexMeta::new();
            meta.checkpoint_interval = self.checkpoint_interval;
            meta.log_file_size = 0;
            meta.entry_count = 0;
            self.set_columns_present(&mut meta);
            meta.write_to(index_dir.join("meta"))?;

            // Create empty column files
            ColumnWriter::<u64>::create(index_dir.join("offsets"))?;
            ColumnWriter::<u32>::create(index_dir.join("lengths"))?;
            ColumnWriter::<u32>::create(index_dir.join("flags"))?;
            ColumnWriter::<u64>::create(index_dir.join("time"))?;
            CheckpointWriter::create(index_dir.join("checkpoints"))?;

            return Ok(meta);
        }

        let mmap = unsafe {
            Mmap::map(&file).with_context(|| format!("mmap log file: {}", log_path.display()))?
        };
        let data = &mmap[..];

        let mut off_writer = ColumnWriter::<u64>::create(index_dir.join("offsets"))?;
        let mut len_writer = ColumnWriter::<u32>::create(index_dir.join("lengths"))?;
        let mut flg_writer = ColumnWriter::<u32>::create(index_dir.join("flags"))?;
        let mut tim_writer = ColumnWriter::<u64>::create(index_dir.join("time"))?;
        let mut ckpt_writer = CheckpointWriter::create(index_dir.join("checkpoints"))?;

        let now = now_millis();
        let interval = self.checkpoint_interval as u64;

        let mut off_buf = [0u64; BATCH];
        let mut len_buf = [0u32; BATCH];
        let mut flg_buf = [0u32; BATCH];
        let mut tim_buf = [0u64; BATCH];
        let mut batch_idx = 0;

        let mut line_count: u64 = 0;
        let mut severity_counts = SeverityCounts::default();
        let mut pos: usize = 0;
        let mut last_line_start: usize = 0;

        while pos < data.len() {
            let line_start = pos;
            last_line_start = line_start;
            let line_end = match memchr(b'\n', &data[pos..]) {
                Some(offset) => pos + offset,
                None => data.len(), // last line without trailing newline
            };

            // CRLF handling: exclude \r from line content
            let content_end = if line_end > line_start
                && line_end <= data.len()
                && line_end > 0
                && data[line_end - 1] == b'\r'
            {
                line_end - 1
            } else {
                line_end
            };

            let line = &data[line_start..content_end];
            let flags = detect_flags_bytes(line);

            off_buf[batch_idx] = line_start as u64;
            len_buf[batch_idx] = line.len() as u32;
            flg_buf[batch_idx] = flags;
            tim_buf[batch_idx] = now;
            batch_idx += 1;

            add_severity(&mut severity_counts, flags & SEVERITY_MASK);
            line_count += 1;

            // Flush batch when full
            if batch_idx == BATCH {
                off_writer.push_batch(&off_buf)?;
                len_writer.push_batch(&len_buf)?;
                flg_writer.push_batch(&flg_buf)?;
                tim_writer.push_batch(&tim_buf)?;
                batch_idx = 0;
            }

            // Write checkpoint at interval boundaries
            if interval > 0 && line_count.is_multiple_of(interval) {
                let hash = content_hash(data, line_start, 256);
                ckpt_writer.push(&Checkpoint {
                    line_number: line_count,
                    byte_offset: line_start as u64,
                    content_hash: hash,
                    index_timestamp: now,
                    severity_counts,
                })?;
            }

            pos = if line_end < data.len() {
                line_end + 1 // skip the \n
            } else {
                data.len()
            };
        }

        // Flush remaining batch
        if batch_idx > 0 {
            off_writer.push_batch(&off_buf[..batch_idx])?;
            len_writer.push_batch(&len_buf[..batch_idx])?;
            flg_writer.push_batch(&flg_buf[..batch_idx])?;
            tim_writer.push_batch(&tim_buf[..batch_idx])?;
        }

        // Final checkpoint if last line wasn't on a boundary
        if line_count > 0 && (interval == 0 || !line_count.is_multiple_of(interval)) {
            let hash = content_hash(data, last_line_start, 256);
            ckpt_writer.push(&Checkpoint {
                line_number: line_count,
                byte_offset: last_line_start as u64,
                content_hash: hash,
                index_timestamp: now,
                severity_counts,
            })?;
        }

        off_writer.flush()?;
        len_writer.flush()?;
        flg_writer.flush()?;
        tim_writer.flush()?;
        ckpt_writer.flush()?;

        let mut meta = IndexMeta::new();
        meta.checkpoint_interval = self.checkpoint_interval;
        meta.entry_count = line_count;
        meta.log_file_size = file_size;
        self.set_columns_present(&mut meta);
        meta.write_to(index_dir.join("meta"))?;

        Ok(meta)
    }

    fn set_columns_present(&self, meta: &mut IndexMeta) {
        meta.set_column(ColumnBit::Offsets);
        meta.set_column(ColumnBit::Lengths);
        meta.set_column(ColumnBit::Time);
        meta.set_column(ColumnBit::Flags);
        meta.set_column(ColumnBit::Checkpoints);
    }
}

impl Default for IndexBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Capture-time incremental indexer: accepts lines one at a time.
///
/// `push_line` expects raw bytes including the trailing delimiter (`\n` or `\r\n`).
/// The delimiter is detected per-line, so mixed LF/CRLF files are handled correctly.
/// The last line of a file may omit the delimiter entirely.
pub struct LineIndexer {
    _lock: Option<IndexWriteLock>,
    offset_writer: ColumnWriter<u64>,
    length_writer: ColumnWriter<u32>,
    flags_writer: ColumnWriter<u32>,
    time_writer: ColumnWriter<u64>,
    checkpoint_writer: CheckpointWriter,
    checkpoint_interval: u16,
    line_count: u64,
    current_offset: u64,
    severity_counts: SeverityCounts,
    last_line_offset: u64,
    last_content_hash: u64,
}

impl LineIndexer {
    pub fn create(index_dir: &Path) -> Result<Self> {
        let lock = IndexWriteLock::try_acquire(index_dir)?;
        if lock.is_none() {
            eprintln!(
                "Warning: index directory is locked by another process, proceeding without lock"
            );
        }

        std::fs::create_dir_all(index_dir)
            .with_context(|| format!("creating index dir: {}", index_dir.display()))?;

        Ok(Self {
            _lock: lock,
            offset_writer: ColumnWriter::create(index_dir.join("offsets"))?,
            length_writer: ColumnWriter::create(index_dir.join("lengths"))?,
            flags_writer: ColumnWriter::create(index_dir.join("flags"))?,
            time_writer: ColumnWriter::create(index_dir.join("time"))?,
            checkpoint_writer: CheckpointWriter::create(index_dir.join("checkpoints"))?,
            checkpoint_interval: 100,
            line_count: 0,
            current_offset: 0,
            severity_counts: SeverityCounts::default(),
            last_line_offset: 0,
            last_content_hash: 0,
        })
    }

    pub fn resume(index_dir: &Path) -> Result<Self> {
        let lock = IndexWriteLock::try_acquire(index_dir)?;
        if lock.is_none() {
            eprintln!(
                "Warning: index directory is locked by another process, proceeding without lock"
            );
        }

        let meta = IndexMeta::read_from(index_dir.join("meta"))?;

        // Restore cumulative severity counts and last content hash from the last checkpoint
        let last_cp = CheckpointReader::open(index_dir.join("checkpoints"))?.last();
        let (severity_counts, last_content_hash) = match last_cp {
            Some(cp) => (cp.severity_counts, cp.content_hash),
            None => (SeverityCounts::default(), 0),
        };

        Ok(Self {
            _lock: lock,
            offset_writer: ColumnWriter::truncate_and_open(
                index_dir.join("offsets"),
                meta.entry_count as usize,
            )?,
            length_writer: ColumnWriter::truncate_and_open(
                index_dir.join("lengths"),
                meta.entry_count as usize,
            )?,
            flags_writer: ColumnWriter::truncate_and_open(
                index_dir.join("flags"),
                meta.entry_count as usize,
            )?,
            time_writer: ColumnWriter::truncate_and_open(
                index_dir.join("time"),
                meta.entry_count as usize,
            )?,
            checkpoint_writer: CheckpointWriter::truncate_and_open(
                index_dir.join("checkpoints"),
                meta.entry_count,
            )?,
            checkpoint_interval: meta.checkpoint_interval,
            line_count: meta.entry_count,
            current_offset: meta.log_file_size,
            severity_counts,
            last_line_offset: meta.log_file_size,
            last_content_hash,
        })
    }

    /// Index a raw line including its trailing delimiter (`\n`, `\r\n`, or none).
    ///
    /// The content stored in the index excludes the delimiter — only the
    /// meaningful bytes are hashed and their length recorded.
    pub fn push_line(&mut self, raw: &[u8], timestamp: u64) -> Result<()> {
        // Strip trailing delimiter to get content
        let (content, raw_len) = if raw.ends_with(b"\r\n") {
            (&raw[..raw.len() - 2], raw.len())
        } else if raw.ends_with(b"\n") {
            (&raw[..raw.len() - 1], raw.len())
        } else {
            (raw, raw.len())
        };

        let flags = detect_flags_bytes(content);
        let line_offset = self.current_offset;
        let hash = xxhash_rust::xxh3::xxh3_64(content);

        self.offset_writer.push(line_offset)?;
        self.length_writer.push(content.len() as u32)?;
        self.flags_writer.push(flags)?;
        self.time_writer.push(timestamp)?;

        add_severity(&mut self.severity_counts, flags & SEVERITY_MASK);
        self.line_count += 1;
        self.last_line_offset = line_offset;
        self.last_content_hash = hash;
        self.current_offset += raw_len as u64;

        let interval = self.checkpoint_interval as u64;
        if interval > 0 && self.line_count.is_multiple_of(interval) {
            self.checkpoint_writer.push(&Checkpoint {
                line_number: self.line_count,
                byte_offset: line_offset,
                content_hash: hash,
                index_timestamp: timestamp,
                severity_counts: self.severity_counts,
            })?;
        }

        Ok(())
    }

    fn build_meta(&self) -> IndexMeta {
        let mut meta = IndexMeta::new();
        meta.checkpoint_interval = self.checkpoint_interval;
        meta.entry_count = self.line_count;
        meta.log_file_size = self.current_offset;
        meta.set_column(ColumnBit::Offsets);
        meta.set_column(ColumnBit::Lengths);
        meta.set_column(ColumnBit::Time);
        meta.set_column(ColumnBit::Flags);
        meta.set_column(ColumnBit::Checkpoints);
        meta
    }

    /// Flush column buffers and write meta so readers can pick up new offsets.
    /// Call periodically during capture to keep the TUI's columnar offsets current.
    pub fn sync(&mut self, index_dir: &Path) -> Result<()> {
        self.offset_writer.flush()?;
        self.length_writer.flush()?;
        self.flags_writer.flush()?;
        self.time_writer.flush()?;
        self.checkpoint_writer.flush()?;

        self.build_meta().write_to(index_dir.join("meta"))?;

        Ok(())
    }

    pub fn finish(mut self, index_dir: &Path) -> Result<IndexMeta> {
        // Final checkpoint if last line wasn't on a boundary
        let interval = self.checkpoint_interval as u64;
        if self.line_count > 0 && (interval == 0 || !self.line_count.is_multiple_of(interval)) {
            self.checkpoint_writer.push(&Checkpoint {
                line_number: self.line_count,
                byte_offset: self.last_line_offset,
                content_hash: self.last_content_hash,
                index_timestamp: now_millis(),
                severity_counts: self.severity_counts,
            })?;
        }

        self.offset_writer.flush()?;
        self.length_writer.flush()?;
        self.flags_writer.flush()?;
        self.time_writer.flush()?;
        self.checkpoint_writer.flush()?;

        let meta = self.build_meta();
        meta.write_to(index_dir.join("meta"))?;

        Ok(meta)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::checkpoint::CheckpointReader;
    use crate::index::column::ColumnReader;
    use crate::index::flags::*;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_log(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        let mut f = File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path
    }

    // --- IndexBuilder tests ---

    #[test]
    fn build_empty_file() {
        let dir = tempdir().unwrap();
        let log = write_log(dir.path(), "empty.log", "");
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 0);
        assert_eq!(meta.log_file_size, 0);
        assert!(meta.has_column(ColumnBit::Offsets));
        assert!(meta.has_column(ColumnBit::Flags));
    }

    #[test]
    fn build_single_line() {
        let dir = tempdir().unwrap();
        let log = write_log(dir.path(), "single.log", "2024-01-01 ERROR boom\n");
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 1);

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 1).unwrap();
        assert_eq!(offsets.get(0), Some(0));

        let lengths = ColumnReader::<u32>::open(idx_dir.join("lengths"), 1).unwrap();
        assert_eq!(lengths.get(0), Some(21)); // "2024-01-01 ERROR boom" = 21 bytes

        let flags = ColumnReader::<u32>::open(idx_dir.join("flags"), 1).unwrap();
        let f = flags.get(0).unwrap();
        assert_eq!(f & SEVERITY_MASK, SEVERITY_ERROR);
        assert_ne!(f & FLAG_HAS_TIMESTAMP, 0);
    }

    #[test]
    fn build_multiple_lines() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..10 {
            content.push_str(&format!("line {i}\n"));
        }
        let log = write_log(dir.path(), "multi.log", &content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 10);

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 10).unwrap();
        assert_eq!(offsets.len(), 10);
        assert_eq!(offsets.get(0), Some(0));
        // "line 0\n" = 7 bytes, so line 1 starts at 7
        assert_eq!(offsets.get(1), Some(7));

        let lengths = ColumnReader::<u32>::open(idx_dir.join("lengths"), 10).unwrap();
        assert_eq!(lengths.get(0), Some(6)); // "line 0" = 6 bytes
    }

    #[test]
    fn build_json_lines() {
        let dir = tempdir().unwrap();
        let content = r#"{"level":"error","msg":"fail"}
{"level":"info","msg":"ok"}
{"level":"warn","msg":"slow"}
"#;
        let log = write_log(dir.path(), "json.log", content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 3);

        let flags = ColumnReader::<u32>::open(idx_dir.join("flags"), 3).unwrap();
        for i in 0..3 {
            let f = flags.get(i).unwrap();
            assert_ne!(f & FLAG_FORMAT_JSON, 0, "line {i} should be JSON");
        }
    }

    #[test]
    fn build_logfmt_lines() {
        let dir = tempdir().unwrap();
        let content = "ts=2024-01-01 level=error msg=fail\nts=2024-01-01 level=info msg=ok\n";
        let log = write_log(dir.path(), "logfmt.log", content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 2);

        let flags = ColumnReader::<u32>::open(idx_dir.join("flags"), 2).unwrap();
        for i in 0..2 {
            let f = flags.get(i).unwrap();
            assert_ne!(f & FLAG_FORMAT_LOGFMT, 0, "line {i} should be logfmt");
        }
    }

    #[test]
    fn build_mixed_format() {
        let dir = tempdir().unwrap();
        let content = r#"{"level":"error","msg":"json line"}
ts=2024-01-01 level=info msg=logfmt
just a plain text line
"#;
        let log = write_log(dir.path(), "mixed.log", content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 3);

        let flags = ColumnReader::<u32>::open(idx_dir.join("flags"), 3).unwrap();
        assert_ne!(flags.get(0).unwrap() & FLAG_FORMAT_JSON, 0);
        assert_ne!(flags.get(1).unwrap() & FLAG_FORMAT_LOGFMT, 0);
        assert_eq!(
            flags.get(2).unwrap() & (FLAG_FORMAT_JSON | FLAG_FORMAT_LOGFMT),
            0
        );
    }

    #[test]
    fn build_severity_counts() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        // 5 lines per checkpoint (interval=5), write 10 lines to get 2 checkpoints
        for i in 0..5 {
            content.push_str(&format!("2024-01-01 ERROR error line {i}\n"));
        }
        for i in 0..5 {
            content.push_str(&format!("2024-01-01 INFO info line {i}\n"));
        }
        let log = write_log(dir.path(), "sev.log", &content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new()
            .with_checkpoint_interval(5)
            .build(&log, &idx_dir)
            .unwrap();
        assert_eq!(meta.entry_count, 10);

        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 2);

        let cp1 = ckpts.get(0).unwrap();
        assert_eq!(cp1.severity_counts.error, 5);
        assert_eq!(cp1.severity_counts.info, 0);

        let cp2 = ckpts.get(1).unwrap();
        assert_eq!(cp2.severity_counts.error, 5); // cumulative
        assert_eq!(cp2.severity_counts.info, 5);
    }

    #[test]
    fn build_checkpoint_interval() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..25 {
            content.push_str(&format!("line {i}\n"));
        }
        let log = write_log(dir.path(), "ckpt.log", &content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new()
            .with_checkpoint_interval(10)
            .build(&log, &idx_dir)
            .unwrap();
        assert_eq!(meta.entry_count, 25);

        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 3); // at line 10, 20, and final at 25

        assert_eq!(ckpts.get(0).unwrap().line_number, 10);
        assert_eq!(ckpts.get(1).unwrap().line_number, 20);
        assert_eq!(ckpts.get(2).unwrap().line_number, 25);
    }

    #[test]
    fn build_crlf() {
        let dir = tempdir().unwrap();
        let content = "2024-01-01 ERROR line one\r\n2024-01-01 INFO line two\r\n";
        let log = write_log(dir.path(), "crlf.log", content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 2);

        let lengths = ColumnReader::<u32>::open(idx_dir.join("lengths"), 2).unwrap();
        // "2024-01-01 ERROR line one" = 25 bytes (without \r\n)
        assert_eq!(lengths.get(0), Some(25));
        // "2024-01-01 INFO line two" = 24 bytes (without \r\n)
        assert_eq!(lengths.get(1), Some(24));
    }

    #[test]
    fn build_meta_columns_present() {
        let dir = tempdir().unwrap();
        let log = write_log(dir.path(), "col.log", "test\n");
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert!(meta.has_column(ColumnBit::Offsets));
        assert!(meta.has_column(ColumnBit::Lengths));
        assert!(meta.has_column(ColumnBit::Time));
        assert!(meta.has_column(ColumnBit::Flags));
        assert!(meta.has_column(ColumnBit::Checkpoints));
    }

    #[test]
    #[ignore] // slow: 10K+ lines
    fn build_large_file() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..10_000 {
            content.push_str(&format!(
                "2024-01-01T10:00:00Z INFO request id={i} status=200\n"
            ));
        }
        let log = write_log(dir.path(), "large.log", &content);
        let idx_dir = dir.path().join("idx");

        let meta = IndexBuilder::new().build(&log, &idx_dir).unwrap();
        assert_eq!(meta.entry_count, 10_000);

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 10_000).unwrap();
        assert_eq!(offsets.len(), 10_000);

        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 100); // 10000 / 100 default interval
    }

    #[test]
    fn build_final_checkpoint_has_totals() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        // 7 lines with interval 5: checkpoint at 5, final at 7
        for _ in 0..3 {
            content.push_str("2024-01-01 ERROR fail\n");
        }
        for _ in 0..4 {
            content.push_str("2024-01-01 INFO ok\n");
        }
        let log = write_log(dir.path(), "totals.log", &content);
        let idx_dir = dir.path().join("idx");

        IndexBuilder::new()
            .with_checkpoint_interval(5)
            .build(&log, &idx_dir)
            .unwrap();

        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 2); // at line 5 + final at 7

        let last = ckpts.last().unwrap();
        assert_eq!(last.line_number, 7);
        assert_eq!(last.severity_counts.error, 3);
        assert_eq!(last.severity_counts.info, 4);
    }

    // --- LineIndexer tests ---

    #[test]
    fn indexer_push_lines() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("idx");

        let mut indexer = LineIndexer::create(&idx_dir).unwrap();
        let lines: Vec<&[u8]> = vec![
            b"2024-01-01 ERROR boom\n",
            b"2024-01-01 INFO started\n",
            b"2024-01-01 WARN slow\n",
            b"plain line\n",
            b"2024-01-01 DEBUG verbose", // last line, no delimiter
        ];
        let now = now_millis();
        for line in &lines {
            indexer.push_line(line, now).unwrap();
        }
        let meta = indexer.finish(&idx_dir).unwrap();

        assert_eq!(meta.entry_count, 5);

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 5).unwrap();
        assert_eq!(offsets.get(0), Some(0));
        // "2024-01-01 ERROR boom\n" = 22 raw bytes, so line 1 starts at 22
        assert_eq!(offsets.get(1), Some(22));

        let lengths = ColumnReader::<u32>::open(idx_dir.join("lengths"), 5).unwrap();
        assert_eq!(lengths.get(0), Some(21)); // content without \n
        assert_eq!(lengths.get(4), Some(24)); // no delimiter to strip

        let flags = ColumnReader::<u32>::open(idx_dir.join("flags"), 5).unwrap();
        assert_eq!(flags.get(0).unwrap() & SEVERITY_MASK, SEVERITY_ERROR);
        assert_eq!(flags.get(1).unwrap() & SEVERITY_MASK, SEVERITY_INFO);
        assert_eq!(flags.get(2).unwrap() & SEVERITY_MASK, SEVERITY_WARN);
        assert_eq!(flags.get(3).unwrap() & SEVERITY_MASK, SEVERITY_UNKNOWN);
        assert_eq!(flags.get(4).unwrap() & SEVERITY_MASK, SEVERITY_DEBUG);
    }

    #[test]
    fn indexer_resume() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("idx");
        let now = now_millis();

        // Phase 1: create and push 3 lines
        let mut indexer = LineIndexer::create(&idx_dir).unwrap();
        for line in &[b"line one\n" as &[u8], b"line two\n", b"line three\n"] {
            indexer.push_line(line, now).unwrap();
        }
        let meta1 = indexer.finish(&idx_dir).unwrap();
        assert_eq!(meta1.entry_count, 3);

        // Phase 2: resume and push 2 more
        let mut indexer = LineIndexer::resume(&idx_dir).unwrap();
        for line in &[b"line four\n" as &[u8], b"line five\n"] {
            indexer.push_line(line, now).unwrap();
        }
        let meta2 = indexer.finish(&idx_dir).unwrap();
        assert_eq!(meta2.entry_count, 5);

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 5).unwrap();
        assert_eq!(offsets.len(), 5);
        // All 5 entries should be readable
        for i in 0..5 {
            assert!(offsets.get(i).is_some(), "offset {i} should exist");
        }
    }

    #[test]
    fn indexer_resume_preserves_severity_counts() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("idx");
        let now = now_millis();

        // Phase 1: 3 ERROR lines
        let mut indexer = LineIndexer::create(&idx_dir).unwrap();
        indexer.checkpoint_interval = 10; // no interval checkpoint in either phase
        for _ in 0..3 {
            indexer.push_line(b"2024-01-01 ERROR fail\n", now).unwrap();
        }
        indexer.finish(&idx_dir).unwrap();

        // Phase 2: resume, add 2 INFO lines
        let mut indexer = LineIndexer::resume(&idx_dir).unwrap();
        for _ in 0..2 {
            indexer.push_line(b"2024-01-01 INFO ok\n", now).unwrap();
        }
        indexer.finish(&idx_dir).unwrap();

        // Last checkpoint should have cumulative totals from both phases
        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        let last = ckpts.last().unwrap();
        assert_eq!(last.severity_counts.error, 3);
        assert_eq!(last.severity_counts.info, 2);
        assert_eq!(last.line_number, 5);
    }

    #[test]
    fn indexer_checkpoint_written() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("idx");
        let now = now_millis();

        let mut indexer = LineIndexer::create(&idx_dir).unwrap();
        indexer.checkpoint_interval = 5;

        for i in 0..12 {
            indexer
                .push_line(format!("line {i}\n").as_bytes(), now)
                .unwrap();
        }
        indexer.finish(&idx_dir).unwrap();

        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 3); // at line 5, 10, and final at 12
        assert_eq!(ckpts.get(0).unwrap().line_number, 5);
        assert_eq!(ckpts.get(1).unwrap().line_number, 10);
        assert_eq!(ckpts.get(2).unwrap().line_number, 12);
    }

    #[test]
    fn indexer_push_crlf_lines() {
        let dir = tempdir().unwrap();
        let idx_dir = dir.path().join("idx");

        let mut indexer = LineIndexer::create(&idx_dir).unwrap();
        indexer
            .push_line(b"2024-01-01 ERROR boom\r\n", now_millis())
            .unwrap();
        indexer
            .push_line(b"2024-01-01 INFO ok\r\n", now_millis())
            .unwrap();
        let meta = indexer.finish(&idx_dir).unwrap();

        assert_eq!(meta.entry_count, 2);

        let lengths = ColumnReader::<u32>::open(idx_dir.join("lengths"), 2).unwrap();
        assert_eq!(lengths.get(0), Some(21)); // "2024-01-01 ERROR boom" without \r\n
        assert_eq!(lengths.get(1), Some(18)); // "2024-01-01 INFO ok" without \r\n

        let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 2).unwrap();
        assert_eq!(offsets.get(0), Some(0));
        assert_eq!(offsets.get(1), Some(23)); // 21 content + 2 delimiter
    }
}
