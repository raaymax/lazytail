//! Index integrity and self-healing tests.
//!
//! These tests assert CORRECT behavior. Tests that fail reveal bugs that
//! need fixing. The index should be bullet-proof: detect corruption,
//! reject bad state, and self-heal where possible.

use std::fs;
use std::io::Write;
use std::path::PathBuf;

use tempfile::TempDir;

use crate::index::builder::{now_millis, IndexBuilder, LineIndexer};
use crate::index::column::{ColumnReader, ColumnWriter};
use crate::index::meta::{ColumnBit, IndexMeta};
use crate::index::validate::validate_index;
use crate::reader::file_reader::FileReader;
use crate::reader::LogReader;
use crate::source::index_dir_for_log;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_lines(count: usize) -> String {
    make_lines_offset(0, count)
}

fn make_lines_offset(start: usize, count: usize) -> String {
    let mut s = String::new();
    for i in start..start + count {
        s.push_str(&format!("line {i}\n"));
    }
    s
}

struct HealingFixture {
    dir: TempDir,
}

impl HealingFixture {
    fn new(content: &str) -> Self {
        let dir = TempDir::new().unwrap();
        fs::write(dir.path().join("test.log"), content).unwrap();
        Self { dir }
    }

    fn log_path(&self) -> PathBuf {
        self.dir.path().join("test.log")
    }

    fn idx_dir(&self) -> PathBuf {
        index_dir_for_log(&self.log_path())
    }

    fn write_log(&self, content: &str) {
        fs::write(self.log_path(), content).unwrap();
    }

    fn append_to_log(&self, extra: &str) {
        let mut f = fs::OpenOptions::new()
            .append(true)
            .open(self.log_path())
            .unwrap();
        f.write_all(extra.as_bytes()).unwrap();
        f.flush().unwrap();
    }

    fn build_index(&self) -> IndexMeta {
        IndexBuilder::new()
            .build(&self.log_path(), &self.idx_dir())
            .unwrap()
    }

    fn build_index_with_interval(&self, interval: u16) -> IndexMeta {
        IndexBuilder::new()
            .with_checkpoint_interval(interval)
            .build(&self.log_path(), &self.idx_dir())
            .unwrap()
    }

    fn validate(&self, meta: &IndexMeta) -> Option<crate::index::validate::ValidatedIndex> {
        validate_index(&self.idx_dir(), &self.log_path(), meta)
    }

    fn open_reader(&self) -> FileReader {
        FileReader::new(&self.log_path()).unwrap()
    }

    fn assert_lines(&self, reader: &mut FileReader, expected: &[String]) {
        assert_eq!(reader.total_lines(), expected.len());
        for (i, exp) in expected.iter().enumerate() {
            let got = reader
                .get_line(i)
                .unwrap_or_else(|e| panic!("get_line({i}) failed: {e}"))
                .unwrap_or_else(|| panic!("get_line({i}) returned None"));
            assert_eq!(&got, exp, "mismatch at line {i}");
        }
    }

    /// Simulate the capture.rs restart pattern: validate → resume_at
    fn resume_from_validated(&self) -> LineIndexer {
        let idx_dir = self.idx_dir();
        let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
        let validated = self.validate(&meta).unwrap();
        // Use actual file size (not trusted_file_size) so current_offset
        // accounts for orphan bytes beyond the trusted region.
        let actual_file_size = fs::metadata(self.log_path()).unwrap().len();
        LineIndexer::resume_at(
            &idx_dir,
            validated.trusted_entries as u64,
            actual_file_size,
            meta.checkpoint_interval,
        )
        .unwrap()
    }
}

// ===========================================================================
// Golden path tests (should always pass)
// ===========================================================================

#[test]
fn golden_path_clean_build_and_read() {
    let fix = HealingFixture::new(&make_lines(100));
    let meta = fix.build_index();
    assert_eq!(meta.entry_count, 100);

    let mut reader = fix.open_reader();
    assert!(reader.has_columnar_offsets());
    assert_eq!(reader.columnar_line_count(), 100);
    assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 0");
    assert_eq!(reader.get_line(99).unwrap().unwrap(), "line 99");
}

#[test]
fn golden_path_clean_resume() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    let content_5 = make_lines(5);
    fix.write_log(&content_5);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_5.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    let extra = make_lines_offset(5, 5);
    fix.append_to_log(&extra);
    let mut indexer = LineIndexer::resume(&idx_dir).unwrap();
    for line in extra.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    let mut reader = fix.open_reader();
    let expected: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
    fix.assert_lines(&mut reader, &expected);
    assert!(reader.has_columnar_offsets());
    assert_eq!(reader.columnar_line_count(), 10);
}

#[test]
fn golden_path_orphan_detection_and_repair() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    // Push 10, sync, push 5 orphans, kill
    let content_10 = make_lines(10);
    fix.write_log(&content_10);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_10.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    let orphans = make_lines_offset(10, 5);
    fix.append_to_log(&orphans);
    for line in orphans.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // Validate detects orphans
    let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    let validated = fix.validate(&meta).unwrap();
    assert_eq!(validated.trusted_entries, 10);

    // Repair: re-index orphan lines starting from trusted_file_size
    // (orphans live at trusted_file_size..actual_file_size in the file)
    let mut indexer = LineIndexer::resume_at(
        &idx_dir,
        validated.trusted_entries as u64,
        validated.trusted_file_size,
        meta.checkpoint_interval,
    )
    .unwrap();
    for line in orphans.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // All 15 lines readable via columnar path
    let mut reader = fix.open_reader();
    let expected: Vec<String> = (0..15).map(|i| format!("line {i}")).collect();
    fix.assert_lines(&mut reader, &expected);
    assert!(reader.has_columnar_offsets());
    assert_eq!(reader.columnar_line_count(), 15);

    // Offsets are monotonically increasing
    let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), 15).unwrap();
    for i in 1..15 {
        assert!(
            offsets.get(i).unwrap() > offsets.get(i - 1).unwrap(),
            "offsets must be monotonically increasing at {i}"
        );
    }
}

// ===========================================================================
// Corruption detection tests
// ===========================================================================

#[test]
fn detect_corrupt_offset_at_checkpoint() {
    let fix = HealingFixture::new(&make_lines(5));
    let meta = fix.build_index_with_interval(5);

    let offsets_path = fix.idx_dir().join("offsets");
    let mut data = fs::read(&offsets_path).unwrap();
    data[4 * 8] ^= 0xFF;
    data[4 * 8 + 1] ^= 0xFF;
    fs::write(&offsets_path, &data).unwrap();

    assert!(fix.validate(&meta).is_none(), "corrupt offset must be rejected");
}

#[test]
fn detect_file_replaced_same_length() {
    let original = make_lines(5);
    let fix = HealingFixture::new(&original);
    let meta = fix.build_index();

    let mut replacement = String::new();
    for i in 0..5 {
        replacement.push_str(&format!("XXXX {i}\n"));
    }
    while replacement.len() < original.len() {
        replacement.push(' ');
    }
    replacement.truncate(original.len());
    fix.write_log(&replacement);

    assert!(fix.validate(&meta).is_none(), "replaced content must be rejected");

    let reader = fix.open_reader();
    assert!(!reader.has_columnar_offsets());
}

#[test]
fn detect_file_truncation() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();

    let mut new_content = String::new();
    for i in 0..5 {
        new_content.push_str(&format!("line {i}\n"));
    }
    for i in 0..3 {
        new_content.push_str(&format!("new {i}\n"));
    }
    fix.write_log(&new_content);

    assert!(fix.validate(&meta).is_none(), "truncated file must be rejected");

    let reader = fix.open_reader();
    assert!(!reader.has_columnar_offsets());
    assert_eq!(reader.total_lines(), 8);
}

#[test]
fn detect_nonzero_first_offset() {
    let fix = HealingFixture::new(&make_lines(5));
    let meta = fix.build_index();

    let offsets_path = fix.idx_dir().join("offsets");
    let mut data = fs::read(&offsets_path).unwrap();
    data[0] = 0x01;
    fs::write(&offsets_path, &data).unwrap();

    assert!(fix.validate(&meta).is_none());
}

#[test]
fn detect_wrong_newline_before_offset_1() {
    let fix = HealingFixture::new(&make_lines(5));
    let meta = fix.build_index();

    let offsets_path = fix.idx_dir().join("offsets");
    let mut data = fs::read(&offsets_path).unwrap();
    let offset1_pos = 8;
    let current = u64::from_le_bytes(data[offset1_pos..offset1_pos + 8].try_into().unwrap());
    let wrong = current - 1;
    data[offset1_pos..offset1_pos + 8].copy_from_slice(&wrong.to_le_bytes());
    fs::write(&offsets_path, &data).unwrap();

    assert!(fix.validate(&meta).is_none());
}

#[test]
fn detect_inflated_entry_count() {
    let fix = HealingFixture::new(&make_lines(5));
    let mut meta = fix.build_index();
    meta.entry_count = 500;
    meta.write_to(fix.idx_dir().join("meta")).unwrap();

    assert!(fix.validate(&meta).is_none());
}

#[test]
fn detect_missing_offsets_file() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();
    fs::remove_file(fix.idx_dir().join("offsets")).unwrap();

    assert!(fix.validate(&meta).is_none());

    let reader = fix.open_reader();
    assert!(!reader.has_columnar_offsets());
    assert_eq!(reader.total_lines(), 10);
}

#[test]
fn detect_zero_byte_offsets() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();
    fs::write(fix.idx_dir().join("offsets"), b"").unwrap();

    assert!(fix.validate(&meta).is_none());
}

#[test]
fn detect_empty_log_with_stale_index() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();
    fix.write_log("");

    assert!(fix.validate(&meta).is_none());
}

#[cfg(unix)]
#[test]
fn detect_symlink_retarget() {
    let dir = TempDir::new().unwrap();
    let log1 = dir.path().join("real1.log");
    let log2 = dir.path().join("real2.log");
    let link = dir.path().join("current.log");

    fs::write(&log1, &make_lines(10)).unwrap();
    fs::write(&log2, &make_lines_offset(100, 10)).unwrap();

    std::os::unix::fs::symlink(&log1, &link).unwrap();
    let idx_dir = index_dir_for_log(&link);
    let meta = IndexBuilder::new().build(&link, &idx_dir).unwrap();

    fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(&log2, &link).unwrap();

    assert!(validate_index(&idx_dir, &link, &meta).is_none());

    let mut reader = FileReader::new(&link).unwrap();
    assert!(!reader.has_columnar_offsets());
    assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 100");
}

#[test]
fn detect_index_dir_is_regular_file() {
    let fix = HealingFixture::new(&make_lines(5));
    fs::write(&fix.idx_dir(), "not a directory").unwrap();

    let reader = fix.open_reader();
    assert!(!reader.has_columnar_offsets());
    assert_eq!(reader.total_lines(), 5);
}

#[test]
fn partial_checkpoint_record_ignored() {
    let fix = HealingFixture::new(&make_lines(15));
    let meta = fix.build_index_with_interval(5);

    let ckpt_path = fix.idx_dir().join("checkpoints");
    let mut ckpt_data = fs::read(&ckpt_path).unwrap();
    ckpt_data.extend_from_slice(&[0xAB; 20]);
    fs::write(&ckpt_path, &ckpt_data).unwrap();

    let result = fix.validate(&meta);
    assert!(result.is_some());
    assert_eq!(result.unwrap().trusted_entries, 15);
}

#[test]
fn offsets_longer_than_meta_clamped() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();

    let offsets_path = fix.idx_dir().join("offsets");
    let mut writer = ColumnWriter::<u64>::open(&offsets_path).unwrap();
    for i in 0..5u64 {
        writer.push(9999 + i).unwrap();
    }
    writer.flush().unwrap();
    drop(writer);

    let result = fix.validate(&meta);
    assert!(result.is_some());
    assert_eq!(result.unwrap().trusted_entries, 10);
}

#[test]
fn no_sync_before_kill_creates_fresh_index() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    let content = make_lines(10);
    fix.write_log(&content);

    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    assert!(!idx_dir.join("meta").exists());

    let mut indexer2 = LineIndexer::create(&idx_dir).unwrap();
    for line in content.lines() {
        indexer2
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer2.finish(&idx_dir).unwrap();

    let mut reader = fix.open_reader();
    assert!(reader.has_columnar_offsets());
    let expected: Vec<String> = (0..10).map(|i| format!("line {i}")).collect();
    fix.assert_lines(&mut reader, &expected);
}

// ===========================================================================
// Bug-catching tests: these assert CORRECT behavior.
// If they fail, there's a bug to fix.
// ===========================================================================

// ---------------------------------------------------------------------------
// Resume over orphan gap must produce correct offsets.
//
// After a kill leaves orphans in the log, resume_at should account for
// the orphan bytes so that new lines get correct file offsets.
// ---------------------------------------------------------------------------

#[test]
fn resume_over_orphan_gap_produces_correct_offsets() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    // Push 10 lines, sync, push 5 orphans, kill
    let content_10 = make_lines(10);
    fix.write_log(&content_10);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_10.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    let orphans = make_lines_offset(10, 5);
    fix.append_to_log(&orphans);
    for line in orphans.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // Resume at trusted point
    let mut indexer = fix.resume_from_validated();

    // Append NEW lines to the log (after the orphans)
    let new_lines = make_lines_offset(100, 5);
    fix.append_to_log(&new_lines);

    for line in new_lines.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // The new lines' offsets must point to their actual file positions,
    // not into the orphan region.
    let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), meta.entry_count as usize).unwrap();

    // Read the actual file to find where "line 100" really starts
    let file_content = fs::read_to_string(fix.log_path()).unwrap();
    let actual_pos_of_line_100 = file_content.find("line 100\n").unwrap() as u64;

    let recorded_offset_10 = offsets.get(10).unwrap();
    assert_eq!(
        recorded_offset_10, actual_pos_of_line_100,
        "offset[10] must point to actual file position of 'line 100', \
         not into orphan region"
    );
}

// ---------------------------------------------------------------------------
// Live capture: FileReader reload must validate before using new offsets.
//
// During live capture, try_refresh_columnar_offsets re-mmaps the offsets
// column. It must validate the index, and the sequential read fast path
// must verify position against columnar offsets to catch orphan gaps.
// ---------------------------------------------------------------------------

#[test]
fn live_capture_reload_must_not_use_wrong_offsets() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    // Build clean index for 10 lines
    let content_10 = make_lines(10);
    fix.write_log(&content_10);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_10.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    let mut reader = fix.open_reader();
    assert!(reader.has_columnar_offsets());

    // Simulate killed capture that left orphans, then correct resume
    let orphans = make_lines_offset(10, 5);
    fix.append_to_log(&orphans);

    // Resume with actual file size (correct capture.rs behavior)
    let mut indexer = fix.resume_from_validated();

    // Append new lines (after orphans) and push with correct offsets
    let new_lines = make_lines_offset(100, 5);
    fix.append_to_log(&new_lines);
    for line in new_lines.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    // FileReader reloads — must read correct content for line 10
    reader.reload().unwrap();

    // The file has orphan lines between indexed regions:
    //   lines 0-9 (indexed), orphans 10-14 (unindexed), new 100-104 (indexed as 10-14)
    // The reader should use columnar offset for indexed line 10, pointing to "line 100"
    let line10 = reader.get_line(10).unwrap().unwrap();
    assert_eq!(
        line10, "line 100",
        "after reload, line 10 must be 'line 100' (the indexed line), \
         not 'line 10' (orphan content from sequential read)"
    );
}

// ---------------------------------------------------------------------------
// validate_index must check ALL column sizes, not just offsets.
//
// If lengths/flags/time columns are truncated but offsets is intact,
// the index is structurally inconsistent and must be rejected.
// ---------------------------------------------------------------------------

#[test]
fn validate_must_check_all_column_sizes() {
    let fix = HealingFixture::new(&make_lines(20));
    let meta = fix.build_index_with_interval(10);

    // Truncate lengths column to 12 entries while offsets has 20
    let lengths_path = fix.idx_dir().join("lengths");
    let lengths_data = fs::read(&lengths_path).unwrap();
    fs::write(&lengths_path, &lengths_data[..12 * 4]).unwrap();

    let result = fix.validate(&meta);
    assert!(
        result.is_none(),
        "validate must reject index when column sizes are inconsistent"
    );
}

// ---------------------------------------------------------------------------
// Missing checkpoints file must NOT cause trust-all fallthrough.
//
// If meta says checkpoints are present but the file is gone, that's
// corruption — not "no checkpoints were configured".
// ---------------------------------------------------------------------------

#[test]
fn missing_checkpoints_file_must_not_trust_all() {
    let fix = HealingFixture::new(&make_lines(10));
    let meta = fix.build_index();
    assert!(meta.has_column(ColumnBit::Checkpoints));

    fs::remove_file(fix.idx_dir().join("checkpoints")).unwrap();

    let result = fix.validate(&meta);
    assert!(
        result.is_none(),
        "missing checkpoints file (when meta says present) must be rejected, \
         not silently trusted"
    );
}

// ---------------------------------------------------------------------------
// Bit rot between checkpoints must be detected.
//
// A corrupt offset at a non-checkpoint line should be caught, not silently
// used to read wrong file content.
// ---------------------------------------------------------------------------

#[test]
fn bit_rot_between_checkpoints_must_be_detected() {
    let content = make_lines(20);
    let fix = HealingFixture::new(&content);
    let _meta = fix.build_index_with_interval(10);

    // Corrupt offset for line 5 (between checkpoints at 10 and 20)
    let offsets_path = fix.idx_dir().join("offsets");
    let mut data = fs::read(&offsets_path).unwrap();
    data[5 * 8] ^= 0x01;
    fs::write(&offsets_path, &data).unwrap();

    // Either validate should reject, OR FileReader should detect and
    // fall back to sparse. The reader must NOT return wrong content.
    let mut reader = fix.open_reader();
    let line5 = reader.get_line(5).unwrap().unwrap();
    assert_eq!(
        line5, "line 5",
        "reader must return correct content even if offset is corrupt"
    );
}

// ---------------------------------------------------------------------------
// Multiple crash-restart cycles must not silently corrupt the index.
//
// After two kills without repair, the offset error compounds. The index
// must either be rejected by validation or produce correct reads.
// ---------------------------------------------------------------------------

#[test]
fn multiple_kills_must_not_silently_corrupt() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    // Round 1: push 10, sync, push 3 orphans, kill
    let content_10 = make_lines(10);
    fix.write_log(&content_10);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_10.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    let orphans_r1 = make_lines_offset(10, 3);
    fix.append_to_log(&orphans_r1);
    for line in orphans_r1.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // Round 2: buggy resume over orphan gap, push new lines, sync, kill
    let mut indexer = fix.resume_from_validated();

    let new_r2 = make_lines_offset(100, 5);
    fix.append_to_log(&new_r2);
    for line in new_r2.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    let orphans_r2 = make_lines_offset(200, 3);
    fix.append_to_log(&orphans_r2);
    for line in orphans_r2.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // After two crashes: FileReader must return correct data.
    // Either validation rejects the bad index (sparse fallback reads correctly),
    // or the offsets are correct in the first place.
    let _meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    let mut reader = fix.open_reader();

    // Lines 0-9 must always be correct
    for i in 0..10 {
        assert_eq!(
            reader.get_line(i).unwrap().unwrap(),
            format!("line {i}"),
        );
    }

    // If the reader loaded columnar offsets, the new lines must be correct too
    if reader.has_columnar_offsets() {
        let line10 = reader.get_line(10).unwrap().unwrap();
        assert_eq!(
            line10, "line 100",
            "if columnar offsets are used, they must be correct"
        );
    }
}

// ---------------------------------------------------------------------------
// Orphan gap + external data appended: offsets must be correct.
// ---------------------------------------------------------------------------

#[test]
fn orphan_gap_plus_external_append_offsets_correct() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    let content_10 = make_lines(10);
    fix.write_log(&content_10);
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for line in content_10.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    let orphans = make_lines_offset(10, 3);
    fix.append_to_log(&orphans);
    for line in orphans.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // External process appends data
    fix.append_to_log(&"=== ROTATION MARKER ===\n".repeat(100));

    // Resume and push new lines
    let mut indexer = fix.resume_from_validated();
    let new_lines = make_lines_offset(500, 5);
    fix.append_to_log(&new_lines);
    for line in new_lines.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // New line offsets must point to correct file positions
    let file_content = fs::read_to_string(fix.log_path()).unwrap();
    let actual_pos = file_content.find("line 500\n").unwrap() as u64;

    let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    let offsets = ColumnReader::<u64>::open(idx_dir.join("offsets"), meta.entry_count as usize).unwrap();
    let recorded = offsets.get(10).unwrap();

    assert_eq!(
        recorded, actual_pos,
        "offset[10] must point to actual file position of 'line 500'"
    );
}

// ---------------------------------------------------------------------------
// CRLF/LF mismatch: offsets must match actual file positions.
//
// If push_line receives \n-terminated lines but the file has \r\n,
// the offset drift must be detected and rejected.
// ---------------------------------------------------------------------------

#[test]
fn crlf_lf_mismatch_must_produce_correct_offsets_or_be_rejected() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    // Write file with CRLF
    let mut crlf_content = String::new();
    for i in 0..10 {
        crlf_content.push_str(&format!("line {i}\r\n"));
    }
    fix.write_log(&crlf_content);

    // Push with LF only (mismatch)
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    for i in 0..10 {
        indexer
            .push_line(format!("line {i}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // FileReader must return correct content for ALL lines.
    // Either offsets are correct, or validation rejects and sparse reads correctly.
    let mut reader = fix.open_reader();
    for i in 0..10 {
        assert_eq!(
            reader.get_line(i).unwrap().unwrap(),
            format!("line {i}"),
            "line {i} must have correct content regardless of CRLF/LF mismatch"
        );
    }
}

// ---------------------------------------------------------------------------
// Partial line push: missing delimiter must not cause silent offset drift.
// ---------------------------------------------------------------------------

#[test]
fn partial_line_push_must_not_cause_silent_drift() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    let content = make_lines(5);
    fix.write_log(&content);

    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    // Push line 0 WITHOUT delimiter
    indexer.push_line(b"line 0", now).unwrap();
    // Push remaining lines normally
    for i in 1..5 {
        indexer
            .push_line(format!("line {i}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // FileReader must return correct content for ALL lines.
    let mut reader = fix.open_reader();
    for i in 0..5 {
        assert_eq!(
            reader.get_line(i).unwrap().unwrap(),
            format!("line {i}"),
            "line {i} must be correct despite partial line push"
        );
    }
}

// ---------------------------------------------------------------------------
// External interleaved writes: indexer offset tracking diverges from file.
// The index must either account for external writes or be rejected.
// ---------------------------------------------------------------------------

#[test]
fn external_interleaved_writes_must_not_return_wrong_content() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    fix.write_log("");
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();

    fix.append_to_log("line 0\n");
    indexer.push_line(b"line 0\n", now).unwrap();

    // External write between captures
    fix.append_to_log("EXTERNAL: noise\n");

    fix.append_to_log("line 1\n");
    indexer.push_line(b"line 1\n", now).unwrap();

    fix.append_to_log("EXTERNAL: more noise\n");

    fix.append_to_log("line 2\n");
    indexer.push_line(b"line 2\n", now).unwrap();

    indexer.finish(&idx_dir).unwrap();

    // The reader must not silently return external noise for "line 1".
    // Either: offsets are correct, or validation rejects the index.
    let mut reader = fix.open_reader();

    // If columnar offsets are loaded, they must point to the right content
    if reader.has_columnar_offsets() {
        assert_eq!(
            reader.get_line(1).unwrap().unwrap(),
            "line 1",
            "columnar read must return 'line 1', not external noise"
        );
    } else {
        // Sparse fallback sees all lines in the file (including external)
        // which is correct — it's the actual file content
        assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 0");
    }
}

// ---------------------------------------------------------------------------
// Resume with mismatched checkpoint_interval: meta must be consistent.
// ---------------------------------------------------------------------------

#[test]
fn resume_with_mismatched_interval_must_be_consistent() {
    let fix = HealingFixture::new(&make_lines(10));
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    fix.build_index_with_interval(5);

    let extra = make_lines_offset(10, 10);
    fix.append_to_log(&extra);

    let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    assert_eq!(meta.checkpoint_interval, 5);

    // Resume with wrong interval
    let mut indexer = LineIndexer::resume_at(
        &idx_dir,
        meta.entry_count,
        meta.log_file_size,
        7,
    )
    .unwrap();
    for line in extra.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    // Regardless of interval mismatch, all lines must read correctly
    let mut reader = fix.open_reader();
    assert!(reader.has_columnar_offsets());
    let expected: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
    fix.assert_lines(&mut reader, &expected);
}

// ---------------------------------------------------------------------------
// Multiple syncs with orphan accumulation: recovery must work.
// ---------------------------------------------------------------------------

#[test]
fn multiple_syncs_with_orphan_recovery() {
    let fix = HealingFixture::new("");
    let idx_dir = fix.idx_dir();
    let now = now_millis();

    let all_content = make_lines(30);
    fix.write_log(&all_content);

    let mut indexer = LineIndexer::create(&idx_dir).unwrap();

    // Sync at 10
    for line in make_lines(10).lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    // Sync at 20
    for line in make_lines_offset(10, 10).lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.sync(&idx_dir).unwrap();

    // Push 10 orphans and kill
    for line in make_lines_offset(20, 10).lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // Validate and repair
    let meta = IndexMeta::read_from(idx_dir.join("meta")).unwrap();
    assert_eq!(meta.entry_count, 20);
    let validated = fix.validate(&meta).unwrap();
    assert_eq!(validated.trusted_entries, 20);

    // Repair: re-index orphan lines starting from trusted_file_size
    let mut indexer = LineIndexer::resume_at(
        &idx_dir,
        validated.trusted_entries as u64,
        validated.trusted_file_size,
        meta.checkpoint_interval,
    )
    .unwrap();
    for line in make_lines_offset(20, 10).lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    indexer.finish(&idx_dir).unwrap();

    let mut reader = fix.open_reader();
    let expected: Vec<String> = (0..30).map(|i| format!("line {i}")).collect();
    fix.assert_lines(&mut reader, &expected);
    assert!(reader.has_columnar_offsets());
    assert_eq!(reader.columnar_line_count(), 30);
}

// ---------------------------------------------------------------------------
// Create over existing index then killed: must not leave corrupt state.
// ---------------------------------------------------------------------------

#[test]
fn create_over_existing_then_killed() {
    let fix = HealingFixture::new(&make_lines(20));
    fix.build_index();

    let new_content = make_lines_offset(100, 5);
    fix.write_log(&new_content);

    let idx_dir = fix.idx_dir();
    let mut indexer = LineIndexer::create(&idx_dir).unwrap();
    let now = now_millis();
    for line in new_content.lines() {
        indexer
            .push_line(format!("{line}\n").as_bytes(), now)
            .unwrap();
    }
    drop(indexer);

    // FileReader must not use the stale gen1 index to read gen2 content
    let mut reader = fix.open_reader();
    assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 100");
}

// ===========================================================================
// Manual edit scenarios: user modifies log file outside of lazytail
// ===========================================================================

// ---------------------------------------------------------------------------
// Content-only edit preserving line lengths (e.g., sed 's/error/ERROR/')
// between checkpoints. Offsets remain valid, newline positions unchanged,
// but the content is different. Must be detected.
// ---------------------------------------------------------------------------

#[test]
fn manual_edit_same_length_between_checkpoints_must_be_detected() {
    // 20 lines, checkpoints at 10 and 20.
    // Edit line 5 content without changing its length.
    let mut content = String::new();
    for i in 0..20 {
        content.push_str(&format!("line {:04}\n", i)); // fixed-width: "line 0005\n" = 10 bytes
    }
    let fix = HealingFixture::new(&content);
    let meta = fix.build_index_with_interval(10);

    // Edit line 5: "line 0005" → "LINE 0005" (same length)
    let edited = content.replace("line 0005", "LINE 0005");
    assert_eq!(edited.len(), content.len());
    fix.write_log(&edited);

    // Reader must not silently return old content via columnar path.
    // Either validation rejects, or the reader detects the mismatch.
    let mut reader = fix.open_reader();
    let line5 = reader.get_line(5).unwrap().unwrap();
    assert_eq!(
        line5, "LINE 0005",
        "reader must return edited content, not stale indexed content"
    );
}

// ---------------------------------------------------------------------------
// Edit that changes line length, shifting all subsequent offsets.
// e.g., "line 5\n" (7 bytes) → "line 5 EDITED\n" (15 bytes)
// All offsets after line 5 are now wrong.
// ---------------------------------------------------------------------------

#[test]
fn manual_edit_changes_line_length_must_invalidate_index() {
    let fix = HealingFixture::new(&make_lines(20));
    let meta = fix.build_index_with_interval(10);

    // Replace line 5 with a longer version
    let original = fs::read_to_string(fix.log_path()).unwrap();
    let edited = original.replace("line 5\n", "line 5 THIS LINE WAS EDITED BY USER\n");
    assert_ne!(edited.len(), original.len());
    fix.write_log(&edited);

    // Validation must reject (file grew but content shifted)
    // OR reader must return correct content for ALL lines
    let mut reader = fix.open_reader();
    for i in 0..20 {
        let got = reader.get_line(i).unwrap().unwrap();
        if i == 5 {
            assert_eq!(
                got, "line 5 THIS LINE WAS EDITED BY USER",
                "edited line must show new content"
            );
        } else {
            assert_eq!(
                got,
                format!("line {i}"),
                "line {i} must be correct after mid-file edit"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Lines inserted in the middle of the file.
// Line count changes, all offsets after insertion point shift.
// ---------------------------------------------------------------------------

#[test]
fn manual_insert_lines_in_middle_must_invalidate_index() {
    let fix = HealingFixture::new(&make_lines(20));
    let meta = fix.build_index_with_interval(10);

    // Insert 3 lines after line 5
    let original = fs::read_to_string(fix.log_path()).unwrap();
    let insertion = "INSERTED LINE A\nINSERTED LINE B\nINSERTED LINE C\n";
    let edited = original.replace("line 5\n", &format!("line 5\n{insertion}"));
    fix.write_log(&edited);

    // Reader must return correct content for ALL lines (now 23 lines)
    let mut reader = fix.open_reader();
    assert_eq!(reader.total_lines(), 23, "should see 23 lines after insertion");
    assert_eq!(reader.get_line(5).unwrap().unwrap(), "line 5");
    assert_eq!(reader.get_line(6).unwrap().unwrap(), "INSERTED LINE A");
    assert_eq!(reader.get_line(7).unwrap().unwrap(), "INSERTED LINE B");
    assert_eq!(reader.get_line(8).unwrap().unwrap(), "INSERTED LINE C");
    assert_eq!(reader.get_line(9).unwrap().unwrap(), "line 6");
}

// ---------------------------------------------------------------------------
// Lines deleted from the middle of the file.
// File shrinks, offsets beyond deletion point are wrong.
// ---------------------------------------------------------------------------

#[test]
fn manual_delete_lines_in_middle_must_invalidate_index() {
    let fix = HealingFixture::new(&make_lines(20));
    let meta = fix.build_index_with_interval(10);

    // Delete lines 5-7
    let original = fs::read_to_string(fix.log_path()).unwrap();
    let edited = original
        .replace("line 5\n", "")
        .replace("line 6\n", "")
        .replace("line 7\n", "");
    fix.write_log(&edited);

    // File is smaller → validation should reject
    assert!(
        fix.validate(&meta).is_none(),
        "deleted lines must cause validation rejection (file smaller)"
    );

    // Reader must return correct content (17 lines)
    let mut reader = fix.open_reader();
    assert_eq!(reader.total_lines(), 17);
    assert_eq!(reader.get_line(4).unwrap().unwrap(), "line 4");
    assert_eq!(reader.get_line(5).unwrap().unwrap(), "line 8");
}

// ---------------------------------------------------------------------------
// File replaced atomically via mv (new inode, same path).
// Reader holds old fd, index on disk may be stale or rebuilt.
// On reload, reader must pick up the new file content.
// ---------------------------------------------------------------------------

#[test]
fn atomic_file_replace_while_open_must_show_new_content() {
    let fix = HealingFixture::new(&make_lines(10));
    fix.build_index();

    let mut reader = fix.open_reader();
    assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 0");

    // Atomic replace: write new content to a temp file, then rename over the original
    let tmp_path = fix.log_path().with_extension("new");
    let new_content = make_lines_offset(500, 5);
    fs::write(&tmp_path, &new_content).unwrap();
    fs::rename(&tmp_path, fix.log_path()).unwrap();

    // Rebuild index for new content
    fix.build_index();

    // After reload, reader must see new content
    reader.reload().unwrap();
    assert_eq!(reader.total_lines(), 5, "should see 5 lines after replace");
    assert_eq!(
        reader.get_line(0).unwrap().unwrap(),
        "line 500",
        "must read new file content after atomic replace"
    );
}

// ---------------------------------------------------------------------------
// truncate -s 0 then rewrite while reader is open.
// The file is zeroed then new content appears.
// ---------------------------------------------------------------------------

#[test]
fn truncate_to_zero_and_rewrite_while_open() {
    let fix = HealingFixture::new(&make_lines(20));
    fix.build_index();

    let mut reader = fix.open_reader();
    assert!(reader.has_columnar_offsets());
    assert_eq!(reader.get_line(0).unwrap().unwrap(), "line 0");

    // Truncate to 0 then write new content (simulates: > logfile; new_process >> logfile)
    fix.write_log("");
    let new_content = make_lines_offset(300, 8);
    fix.write_log(&new_content);

    // After reload, must see new content
    reader.reload().unwrap();
    assert_eq!(reader.total_lines(), 8, "should see 8 lines after truncate+rewrite");
    assert_eq!(
        reader.get_line(0).unwrap().unwrap(),
        "line 300",
        "must read new content after truncate and rewrite"
    );
}

// ---------------------------------------------------------------------------
// sed -i edits the file (creates new inode on most systems).
// Reader's open fd still reads old inode. Index may be stale.
// On reload, must detect inode change and re-read.
// ---------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn sed_i_style_edit_detected_on_reload() {
    let fix = HealingFixture::new(&make_lines(10));
    fix.build_index();

    let mut reader = fix.open_reader();
    assert_eq!(reader.get_line(3).unwrap().unwrap(), "line 3");

    // Simulate sed -i: write new content to temp, rename over original (new inode)
    let tmp_path = fix.log_path().with_extension("sedtmp");
    let original = fs::read_to_string(fix.log_path()).unwrap();
    let edited = original.replace("line 3", "LINE_THREE");
    fs::write(&tmp_path, &edited).unwrap();
    fs::rename(&tmp_path, fix.log_path()).unwrap();

    // After reload, must see edited content
    reader.reload().unwrap();
    assert_eq!(
        reader.get_line(3).unwrap().unwrap(),
        "LINE_THREE",
        "must detect inode change and read new content"
    );
}

// ---------------------------------------------------------------------------
// User appends to log file externally (echo >> logfile) while lazytail
// has an index. The appended content must be visible after reload.
// ---------------------------------------------------------------------------

#[test]
fn external_append_visible_after_reload() {
    let fix = HealingFixture::new(&make_lines(10));
    fix.build_index();

    let mut reader = fix.open_reader();
    assert_eq!(reader.total_lines(), 10);

    // External append
    fix.append_to_log("EXTERNAL LINE 1\nEXTERNAL LINE 2\n");

    reader.reload().unwrap();
    assert_eq!(reader.total_lines(), 12, "should see 12 lines after external append");
    assert_eq!(
        reader.get_line(10).unwrap().unwrap(),
        "EXTERNAL LINE 1",
    );
    assert_eq!(
        reader.get_line(11).unwrap().unwrap(),
        "EXTERNAL LINE 2",
    );
}

// ---------------------------------------------------------------------------
// User overwrites the beginning of the file but keeps same size
// (e.g., dd if=/dev/zero bs=1 count=20 conv=notrunc of=logfile).
// First offset is still 0, but content is garbage. Must be detected.
// ---------------------------------------------------------------------------

#[test]
fn overwrite_beginning_same_size_must_be_detected() {
    let content = make_lines(10);
    let fix = HealingFixture::new(&content);
    let meta = fix.build_index();

    // Overwrite first 20 bytes with zeros (corrupts first two lines)
    let mut data = fs::read(fix.log_path()).unwrap();
    for i in 0..20 {
        data[i] = 0;
    }
    fs::write(fix.log_path(), &data).unwrap();

    // Validation must reject (content hash mismatch at checkpoint)
    assert!(
        fix.validate(&meta).is_none(),
        "overwritten beginning must be detected by checkpoint hash"
    );

    // Reader must not return garbage via columnar path
    let mut reader = fix.open_reader();
    assert!(
        !reader.has_columnar_offsets(),
        "reader must fall back to sparse after detecting corruption"
    );
}
