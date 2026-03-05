use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use crate::index::checkpoint::CheckpointReader;
use crate::index::column::ColumnReader;
use crate::index::meta::{ColumnBit, IndexMeta};

/// Result of index validation with partial trust support.
pub struct ValidatedIndex {
    /// Number of index entries that can be trusted.
    pub trusted_entries: usize,
    /// Byte offset in the log file up to which the index is valid.
    pub trusted_file_size: u64,
}

/// Validate a columnar index against its log file.
///
/// Returns `None` if the index is completely unusable.
/// Returns `Some(validated)` with the number of entries that can be trusted.
///
/// Validation has two phases:
/// 1. **Structural checks** — instant rejection if column sizes don't match meta
/// 2. **Checkpoint walk** — walks checkpoints from last to first, verifying content
///    hashes against the actual file. Returns partial trust at the first valid checkpoint.
pub fn validate_index(idx_dir: &Path, log_path: &Path, meta: &IndexMeta) -> Option<ValidatedIndex> {
    let file_size = std::fs::metadata(log_path).ok()?.len();

    // File was truncated below indexed range
    if file_size < meta.log_file_size {
        return None;
    }

    let entry_count = meta.entry_count as usize;

    // Empty index
    if entry_count == 0 {
        return Some(ValidatedIndex {
            trusted_entries: 0,
            trusted_file_size: 0,
        });
    }

    // Structural check: offsets column must have at least entry_count entries
    let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), entry_count).ok()?;
    if offsets.len() != entry_count {
        return None;
    }

    // Structural check: first line must start at byte 0
    if offsets.get(0) != Some(0) {
        return None;
    }

    // Structural check: byte before second line must be a newline
    if entry_count >= 2 {
        if let Some(next_offset) = offsets.get(1) {
            if next_offset > 0 {
                let mut file = File::open(log_path).ok()?;
                file.seek(SeekFrom::Start(next_offset - 1)).ok()?;
                let mut buf = [0u8; 1];
                file.read_exact(&mut buf).ok()?;
                if buf[0] != b'\n' {
                    return None;
                }
            }
        }
    }

    // Checkpoint walk (partial trust)
    if meta.has_column(ColumnBit::Checkpoints) {
        if let Ok(ckpt_reader) = CheckpointReader::open(idx_dir.join("checkpoints")) {
            if !ckpt_reader.is_empty() {
                for i in (0..ckpt_reader.len()).rev() {
                    if let Some(cp) = ckpt_reader.get(i) {
                        if cp.line_number == 0 || cp.line_number > meta.entry_count {
                            continue;
                        }

                        // Cross-check: offset in column must match checkpoint
                        let line_idx = (cp.line_number - 1) as usize;
                        if offsets.get(line_idx) != Some(cp.byte_offset) {
                            continue;
                        }

                        // Verify content hash against actual file.
                        // Cap read to meta.log_file_size so appended bytes don't
                        // change the hash when the file has grown.
                        let max_bytes =
                            (meta.log_file_size.saturating_sub(cp.byte_offset)) as usize;
                        if !verify_checkpoint_hash(
                            log_path,
                            cp.byte_offset,
                            cp.content_hash,
                            max_bytes,
                        ) {
                            continue;
                        }

                        // This checkpoint validates
                        let trusted = cp.line_number as usize;
                        let trusted_file_size = if trusted == entry_count {
                            meta.log_file_size
                        } else {
                            find_line_end(log_path, cp.byte_offset)?
                        };

                        return Some(ValidatedIndex {
                            trusted_entries: trusted,
                            trusted_file_size,
                        });
                    }
                }

                // All checkpoints failed
                return None;
            }
        }
    }

    // No checkpoints — trust full entry_count (structural checks passed)
    Some(ValidatedIndex {
        trusted_entries: entry_count,
        trusted_file_size: meta.log_file_size,
    })
}

/// Verify a checkpoint's content hash against the actual log file.
///
/// `max_bytes` caps how many bytes to read from the file (to avoid reading
/// into appended content that wasn't present when the hash was computed).
///
/// Tries two hash methods to support both IndexBuilder (256-byte raw read from file)
/// and LineIndexer (content-only without delimiter, up to 256 bytes) built indexes.
fn verify_checkpoint_hash(
    log_path: &Path,
    byte_offset: u64,
    expected_hash: u64,
    max_bytes: usize,
) -> bool {
    let mut file = match File::open(log_path) {
        Ok(f) => f,
        Err(_) => return false,
    };
    if file.seek(SeekFrom::Start(byte_offset)).is_err() {
        return false;
    }

    let read_len = max_bytes.min(256);
    let mut buf = [0u8; 256];
    let bytes_read = match file.read(&mut buf[..read_len]) {
        Ok(n) => n,
        Err(_) => return false,
    };
    if bytes_read == 0 {
        return false;
    }

    // Method 1: hash all bytes read (matches IndexBuilder's content_hash which
    // hashes up to 256 bytes from the file position, potentially spanning lines)
    let hash_full = xxhash_rust::xxh3::xxh3_64(&buf[..bytes_read]);
    if hash_full == expected_hash {
        return true;
    }

    // Method 2: hash content up to the first newline (matches LineIndexer's
    // content hash which excludes the delimiter)
    let content_end = memchr::memchr(b'\n', &buf[..bytes_read]).unwrap_or(bytes_read);
    let content_end = if content_end > 0 && buf[content_end - 1] == b'\r' {
        content_end - 1
    } else {
        content_end
    };
    if content_end != bytes_read {
        let hash_content = xxhash_rust::xxh3::xxh3_64(&buf[..content_end]);
        if hash_content == expected_hash {
            return true;
        }
    }

    false
}

/// Find the byte offset after the line starting at `offset` (past the newline).
fn find_line_end(log_path: &Path, offset: u64) -> Option<u64> {
    let mut file = File::open(log_path).ok()?;
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buf = [0u8; 8192];
    let mut pos = offset;
    loop {
        let n = file.read(&mut buf).ok()?;
        if n == 0 {
            return Some(pos);
        }
        if let Some(nl) = memchr::memchr(b'\n', &buf[..n]) {
            return Some(pos + nl as u64 + 1);
        }
        pos += n as u64;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::builder::IndexBuilder;
    use crate::index::checkpoint::CheckpointReader;
    use std::io::Write;
    use tempfile::tempdir;

    fn write_log(dir: &Path, name: &str, content: &str) -> std::path::PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, content).unwrap();
        path
    }

    fn build_index(log_path: &Path, idx_dir: &Path) -> IndexMeta {
        IndexBuilder::new().build(log_path, idx_dir).unwrap()
    }

    fn build_index_with_interval(log_path: &Path, idx_dir: &Path, interval: u16) -> IndexMeta {
        IndexBuilder::new()
            .with_checkpoint_interval(interval)
            .build(log_path, idx_dir)
            .unwrap()
    }

    // --- Valid index ---

    #[test]
    fn valid_index_full_trust() {
        let dir = tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = write_log(dir.path(), "test.log", content);
        let idx_dir = dir.path().join("idx");
        let meta = build_index(&log_path, &idx_dir);

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.trusted_entries, 3);
        assert_eq!(v.trusted_file_size, content.len() as u64);
    }

    // --- Inflated entry_count ---

    #[test]
    fn inflated_entry_count_rejected() {
        let dir = tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = write_log(dir.path(), "test.log", content);
        let idx_dir = dir.path().join("idx");
        let mut meta = build_index(&log_path, &idx_dir);

        // Inflate entry_count beyond actual offsets column size
        meta.entry_count = 1000;
        meta.write_to(idx_dir.join("meta")).unwrap();

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_none());
    }

    // --- Corrupt last checkpoint, walk back ---

    #[test]
    fn corrupt_last_checkpoint_partial_trust() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..15 {
            content.push_str(&format!("line number {i}\n"));
        }
        let log_path = write_log(dir.path(), "test.log", &content);
        let idx_dir = dir.path().join("idx");
        let meta = build_index_with_interval(&log_path, &idx_dir, 5);

        // Should have checkpoints at lines 5, 10, and 15 (final)
        let ckpts = CheckpointReader::open(idx_dir.join("checkpoints")).unwrap();
        assert_eq!(ckpts.len(), 3);

        // Corrupt the last checkpoint's content_hash (bytes 16..24 of the 3rd entry)
        let mut data = std::fs::read(idx_dir.join("checkpoints")).unwrap();
        let offset = 2 * 64 + 16; // 3rd entry, content_hash field
        data[offset] ^= 0xFF; // flip bits
        std::fs::write(idx_dir.join("checkpoints"), &data).unwrap();

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_some());
        let v = result.unwrap();
        // Should trust up to checkpoint at line 10 (second checkpoint)
        assert_eq!(v.trusted_entries, 10);
    }

    // --- All checkpoints corrupt ---

    #[test]
    fn all_checkpoints_corrupt_returns_none() {
        let dir = tempdir().unwrap();
        let mut content = String::new();
        for i in 0..15 {
            content.push_str(&format!("line number {i}\n"));
        }
        let log_path = write_log(dir.path(), "test.log", &content);
        let idx_dir = dir.path().join("idx");
        let meta = build_index_with_interval(&log_path, &idx_dir, 5);

        // Corrupt ALL checkpoints' content_hash fields
        let mut data = std::fs::read(idx_dir.join("checkpoints")).unwrap();
        for entry in 0..3 {
            let offset = entry * 64 + 16;
            data[offset] ^= 0xFF;
        }
        std::fs::write(idx_dir.join("checkpoints"), &data).unwrap();

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_none());
    }

    // --- File replaced (content hash mismatch) ---

    #[test]
    fn file_replaced_detected_by_hash() {
        let dir = tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = write_log(dir.path(), "test.log", content);
        let idx_dir = dir.path().join("idx");
        let meta = build_index(&log_path, &idx_dir);

        // Replace file with same-size different content
        let replacement = "XXXX one\nXXXX two\nXXXX three\n";
        assert_eq!(content.len(), replacement.len());
        std::fs::write(&log_path, replacement).unwrap();

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_none());
    }

    // --- No checkpoints (structural checks only) ---

    #[test]
    fn no_checkpoints_trusts_full() {
        let dir = tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = write_log(dir.path(), "test.log", content);
        let idx_dir = dir.path().join("idx");
        let mut meta = build_index(&log_path, &idx_dir);

        // Remove checkpoints column bit from meta
        meta.clear_column(ColumnBit::Checkpoints);
        meta.write_to(idx_dir.join("meta")).unwrap();

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.trusted_entries, 3);
    }

    // --- Empty index ---

    #[test]
    fn empty_index_passes() {
        let dir = tempdir().unwrap();
        let log_path = write_log(dir.path(), "test.log", "");
        let idx_dir = dir.path().join("idx");
        let meta = build_index(&log_path, &idx_dir);

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.trusted_entries, 0);
        assert_eq!(v.trusted_file_size, 0);
    }

    // --- Grown file ---

    #[test]
    fn grown_file_full_trust() {
        let dir = tempdir().unwrap();
        let content = "line one\nline two\nline three\n";
        let log_path = write_log(dir.path(), "test.log", content);
        let idx_dir = dir.path().join("idx");
        let meta = build_index(&log_path, &idx_dir);

        // Append more data
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&log_path)
            .unwrap();
        write!(f, "line four\nline five\n").unwrap();
        drop(f);

        let result = validate_index(&idx_dir, &log_path, &meta);
        assert!(result.is_some());
        let v = result.unwrap();
        assert_eq!(v.trusted_entries, 3);
        assert_eq!(v.trusted_file_size, content.len() as u64);
    }
}
