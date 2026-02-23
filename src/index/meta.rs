use std::path::Path;

use anyhow::{bail, Context, Result};

const META_SIZE: usize = 64;
const MAGIC: &[u8; 4] = b"LTIX";
const VERSION: u16 = 1;

/// Identifies which column files are present in an index directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnBit {
    Offsets = 0,
    Lengths = 1,
    Time = 2,
    Flags = 3,
    Templates = 4,
    Checkpoints = 5,
}

/// 64-byte index header with structural metadata.
///
/// Written at `meta` path in the index directory. All fields are
/// little-endian packed manually (no `#[repr(C)]` transmute).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexMeta {
    pub version: u16,
    pub checkpoint_interval: u16,
    pub entry_count: u64,
    pub log_file_size: u64,
    pub columns_present: u64,
    pub flags_schema: u16,
}

impl IndexMeta {
    pub fn new() -> Self {
        Self {
            version: VERSION,
            checkpoint_interval: 100,
            entry_count: 0,
            log_file_size: 0,
            columns_present: 0,
            flags_schema: 1,
        }
    }

    pub fn has_column(&self, col: ColumnBit) -> bool {
        self.columns_present & (1u64 << col as u64) != 0
    }

    pub fn set_column(&mut self, col: ColumnBit) {
        self.columns_present |= 1u64 << col as u64;
    }

    pub fn clear_column(&mut self, col: ColumnBit) {
        self.columns_present &= !(1u64 << col as u64);
    }

    pub fn to_bytes(&self) -> [u8; META_SIZE] {
        let mut buf = [0u8; META_SIZE];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4..6].copy_from_slice(&self.version.to_le_bytes());
        buf[6..8].copy_from_slice(&self.checkpoint_interval.to_le_bytes());
        buf[8..16].copy_from_slice(&self.entry_count.to_le_bytes());
        buf[16..24].copy_from_slice(&self.log_file_size.to_le_bytes());
        buf[24..32].copy_from_slice(&self.columns_present.to_le_bytes());
        buf[32..34].copy_from_slice(&self.flags_schema.to_le_bytes());
        // bytes 34..64 are reserved (zeros)
        buf
    }

    pub fn from_bytes(buf: &[u8; META_SIZE]) -> Result<Self> {
        if &buf[0..4] != MAGIC {
            bail!("invalid index magic: expected LTIX");
        }
        let version = u16::from_le_bytes([buf[4], buf[5]]);
        if version != VERSION {
            bail!("unsupported index version: {version}");
        }
        Ok(Self {
            version,
            checkpoint_interval: u16::from_le_bytes([buf[6], buf[7]]),
            entry_count: u64::from_le_bytes(buf[8..16].try_into().unwrap()),
            log_file_size: u64::from_le_bytes(buf[16..24].try_into().unwrap()),
            columns_present: u64::from_le_bytes(buf[24..32].try_into().unwrap()),
            flags_schema: u16::from_le_bytes([buf[32], buf[33]]),
        })
    }

    pub fn write_to(&self, path: impl AsRef<Path>) -> Result<()> {
        let path = path.as_ref();
        let tmp = path.with_extension("meta.tmp");
        std::fs::write(&tmp, self.to_bytes())
            .with_context(|| format!("writing index meta tmp: {}", tmp.display()))?;
        std::fs::rename(&tmp, path)
            .with_context(|| format!("renaming index meta: {}", path.display()))
    }

    pub fn read_from(path: impl AsRef<Path>) -> Result<Self> {
        let data = std::fs::read(path.as_ref())
            .with_context(|| format!("reading index meta: {}", path.as_ref().display()))?;
        if data.len() < META_SIZE {
            bail!(
                "index meta file too small: {} bytes (expected {META_SIZE})",
                data.len()
            );
        }
        let buf: [u8; META_SIZE] = data[..META_SIZE]
            .try_into()
            .map_err(|_| anyhow::anyhow!("meta slice conversion failed"))?;
        Self::from_bytes(&buf)
    }
}

impl Default for IndexMeta {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip() {
        let mut meta = IndexMeta::new();
        meta.entry_count = 1_000_000;
        meta.log_file_size = 500_000_000;
        meta.checkpoint_interval = 100;
        meta.set_column(ColumnBit::Offsets);
        meta.set_column(ColumnBit::Flags);

        let bytes = meta.to_bytes();
        let restored = IndexMeta::from_bytes(&bytes).unwrap();
        assert_eq!(meta, restored);
    }

    #[test]
    fn invalid_magic() {
        let mut buf = [0u8; META_SIZE];
        buf[0..4].copy_from_slice(b"NOPE");
        let err = IndexMeta::from_bytes(&buf).unwrap_err();
        assert!(err.to_string().contains("invalid index magic"));
    }

    #[test]
    fn bad_version() {
        let mut buf = [0u8; META_SIZE];
        buf[0..4].copy_from_slice(MAGIC);
        buf[4..6].copy_from_slice(&99u16.to_le_bytes());
        let err = IndexMeta::from_bytes(&buf).unwrap_err();
        assert!(err.to_string().contains("unsupported index version"));
    }

    #[test]
    fn column_bit_operations() {
        let mut meta = IndexMeta::new();
        assert!(!meta.has_column(ColumnBit::Offsets));
        assert!(!meta.has_column(ColumnBit::Flags));

        meta.set_column(ColumnBit::Offsets);
        assert!(meta.has_column(ColumnBit::Offsets));
        assert!(!meta.has_column(ColumnBit::Flags));

        meta.set_column(ColumnBit::Flags);
        assert!(meta.has_column(ColumnBit::Offsets));
        assert!(meta.has_column(ColumnBit::Flags));

        meta.clear_column(ColumnBit::Offsets);
        assert!(!meta.has_column(ColumnBit::Offsets));
        assert!(meta.has_column(ColumnBit::Flags));
    }

    #[test]
    fn all_column_bits() {
        let mut meta = IndexMeta::new();
        let bits = [
            ColumnBit::Offsets,
            ColumnBit::Lengths,
            ColumnBit::Time,
            ColumnBit::Flags,
            ColumnBit::Templates,
            ColumnBit::Checkpoints,
        ];
        for bit in &bits {
            meta.set_column(*bit);
        }
        for bit in &bits {
            assert!(meta.has_column(*bit));
        }
        assert_eq!(meta.columns_present, 0b111111);
    }

    #[test]
    fn byte_layout() {
        let meta = IndexMeta {
            version: VERSION,
            checkpoint_interval: 100,
            entry_count: 0x0102030405060708,
            log_file_size: 0,
            columns_present: 0,
            flags_schema: 1,
        };
        let bytes = meta.to_bytes();

        // Magic
        assert_eq!(&bytes[0..4], b"LTIX");
        // Version (LE)
        assert_eq!(&bytes[4..6], &1u16.to_le_bytes());
        // entry_count at offset 8 (LE)
        assert_eq!(&bytes[8..16], &0x0102030405060708u64.to_le_bytes());
    }

    #[test]
    fn write_and_read_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("meta");

        let mut meta = IndexMeta::new();
        meta.entry_count = 42;
        meta.log_file_size = 1024;
        meta.set_column(ColumnBit::Offsets);
        meta.set_column(ColumnBit::Lengths);

        meta.write_to(&path).unwrap();
        let restored = IndexMeta::read_from(&path).unwrap();
        assert_eq!(meta, restored);
    }

    #[test]
    fn truncated_file() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("truncated_meta");

        // Write only 32 bytes (less than META_SIZE)
        std::fs::write(&path, &[0u8; 32]).unwrap();
        let err = IndexMeta::read_from(&path).unwrap_err();
        assert!(err.to_string().contains("too small"));
    }

    #[test]
    fn default_values() {
        let meta = IndexMeta::new();
        assert_eq!(meta.version, VERSION);
        assert_eq!(meta.checkpoint_interval, 100);
        assert_eq!(meta.entry_count, 0);
        assert_eq!(meta.log_file_size, 0);
        assert_eq!(meta.columns_present, 0);
        assert_eq!(meta.flags_schema, 1);
    }

    #[test]
    fn reserved_bytes_are_zero() {
        let meta = IndexMeta::new();
        let bytes = meta.to_bytes();
        // Reserved region: bytes 34..64
        assert!(bytes[34..64].iter().all(|&b| b == 0));
    }
}
