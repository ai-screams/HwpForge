//! Shared, bounded ZIP package input for codecs and source verification.
//!
//! This layer knows package paths and ZIP framing, not HWPX XML or mimetypes.
//! No raw archive/entry reader escapes it: every read counts actual output.
//! Errors use existing `CoreError::InvalidStructure`: context `ZIP` denotes
//! framing/I/O errors, and `ZIP package` denotes names or resource limits.

mod directory;

use std::collections::BTreeSet;
use std::io::{Cursor, Read};

use zip::{CompressionMethod, ZipArchive};

use crate::error::{CoreError, CoreResult};

/// Maximum actual decompressed bytes in one entry (50 MiB).
pub const MAX_ENTRY_SIZE: u64 = 50 * 1024 * 1024;
/// Maximum actual decompressed bytes read from one package (500 MiB).
pub const MAX_TOTAL_SIZE: u64 = 500 * 1024 * 1024;
/// Maximum physical central-directory records, before name merging.
pub const MAX_ENTRIES: usize = 10_000;

/// Optional tighter limits; values above the defaults cannot relax safety.
#[derive(Debug, Clone, Copy)]
pub struct PackageLimits {
    /// Per-entry decompressed bytes.
    pub entry: u64,
    /// Cumulative decompressed bytes, including repeated reads.
    pub total: u64,
    /// Physical directory records.
    pub entries: usize,
}
impl Default for PackageLimits {
    fn default() -> Self {
        Self { entry: MAX_ENTRY_SIZE, total: MAX_TOTAL_SIZE, entries: MAX_ENTRIES }
    }
}

/// Validated entry metadata, retaining physical ZIP order and directory records.
#[derive(Debug, Clone)]
pub struct PackageEntry {
    /// Decoded, validated name (directories retain their trailing slash).
    pub path: String,
    /// Advertised size; never used to allocate decompression buffers.
    pub size: u64,
    /// Advertised compressed size.
    pub compressed_size: u64,
    /// Original compression method for package-preserving rewrites.
    pub compression: CompressionMethod,
}

/// Validated package whose content can only be read with enforced budgets.
#[derive(Debug)]
pub struct Package<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    entries: Vec<PackageEntry>,
    limits: PackageLimits,
    total_read: u64,
}

pub(super) fn zip_error(reason: impl Into<String>) -> CoreError {
    CoreError::InvalidStructure { context: "ZIP".into(), reason: reason.into() }
}
pub(super) fn invalid_structure(reason: impl Into<String>) -> CoreError {
    CoreError::InvalidStructure { context: "ZIP package".into(), reason: reason.into() }
}

/// Validate a package part name without silently removing path components.
///
/// # Errors
/// Rejects empty, dot, parent, backslash, absolute and Windows drive paths.
pub fn normalize_part_name(name: &str) -> CoreResult<&str> {
    let drive = name.as_bytes().get(1) == Some(&b':')
        && name.as_bytes().first().is_some_and(u8::is_ascii_alphabetic);
    if drive
        || name.contains('\\')
        || name.split('/').any(|part| part.is_empty() || part == "." || part == "..")
    {
        return Err(invalid_structure(format!("invalid package part name '{name}'")));
    }
    Ok(name)
}

/// The single ZIP package read entrance, shared by all library consumers.
///
/// Validates framing, physical entry count, original/effective/decoded name
/// uniqueness, part names and empty directory records before exposing metadata.
/// Decompression is lazy, with actual byte limits enforced by [`Package::read`].
///
/// # Errors
/// Rejects ambiguous/invalid ZIP directories, unsafe names and resource limits.
pub fn open_package(bytes: &[u8], limits: PackageLimits) -> CoreResult<Package<'_>> {
    let limits = PackageLimits {
        entry: limits.entry.min(MAX_ENTRY_SIZE),
        total: limits.total.min(MAX_TOTAL_SIZE),
        entries: limits.entries.min(MAX_ENTRIES),
    };
    let archive = directory::open(bytes, limits.entries)?;
    let mut entries = Vec::with_capacity(archive.len());
    let mut keys = BTreeSet::new();
    let metadata = archive.metadata();
    for (index, name) in archive.file_names().enumerate() {
        let path = name.map_err(|e| zip_error(e.to_string()))?.into_owned();
        let directory = path.strip_suffix('/');
        let key = normalize_part_name(directory.unwrap_or(&path))?;
        if !keys.insert(key.to_string()) {
            return Err(invalid_structure(format!("duplicate decoded ZIP entry name '{key}'")));
        }
        let entry = metadata.entry(index).map_err(|e| zip_error(e.to_string()))?;
        if directory.is_some() && entry.size() != 0 {
            return Err(invalid_structure(format!("nonempty ZIP directory entry '{path}'")));
        }
        entries.push(PackageEntry {
            path,
            size: entry.size(),
            compressed_size: entry.compressed_size(),
            compression: entry.compression(),
        });
    }
    Ok(Package { archive, entries, limits, total_read: 0 })
}

impl Package<'_> {
    /// Validated metadata in physical order.
    pub fn entries(&self) -> &[PackageEntry] {
        &self.entries
    }

    /// Read an entry, bounded by actual output rather than header sizes.
    ///
    /// # Errors
    /// Returns existing Core errors for ZIP read failures or exceeded budgets.
    pub fn read(&mut self, index: usize) -> CoreResult<Vec<u8>> {
        if self.total_read > self.limits.total {
            return Err(invalid_structure(format!(
                "total decompressed data ({} bytes) exceeds limit of {}",
                self.total_read, self.limits.total,
            )));
        }
        let entry =
            self.entries.get(index).ok_or_else(|| zip_error("entry index out of bounds"))?;
        let path = &entry.path;
        let file = self.archive.by_index(index).map_err(|e| zip_error(e.to_string()))?;
        let mut data = Vec::new();
        // Allow one overflow probe byte, bounded by both remaining budgets.
        let limit = self.limits.entry.min(self.limits.total - self.total_read);
        let result = file.take(limit + 1).read_to_end(&mut data);
        // Partial output consumes the budget even when reading or validation fails.
        self.total_read = self.total_read.saturating_add(data.len() as u64);
        result.map_err(|e| zip_error(format!("read '{path}': {e}")))?;
        if data.len() as u64 > self.limits.entry {
            return Err(invalid_structure(format!(
                "entry '{path}' decompressed to {} bytes, exceeds limit of {}",
                data.len(),
                self.limits.entry,
            )));
        }
        if self.total_read > self.limits.total {
            return Err(invalid_structure(format!(
                "total decompressed data ({} bytes) exceeds limit of {}",
                self.total_read, self.limits.total,
            )));
        }
        Ok(data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    fn make_zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// 이것을 실패시키는 것: 단일 엔트리 상한 오류 전에 읽은 바이트의 누적을 생략하기.
    #[test]
    fn zip9_fix4_entry_failure_exhausts_total_budget() {
        let bytes = make_zip(&[("a", b"ab"), ("b", b"x")]);
        let mut package =
            open_package(&bytes, PackageLimits { entry: 1, total: 1, entries: 2 }).unwrap();
        assert!(package.read(0).is_err());
        let err = package.read(1).expect_err("failed a must also exhaust b's budget");
        assert!(matches!(err, CoreError::InvalidStructure { context, reason }
            if context == "ZIP package"
                && reason == "total decompressed data (2 bytes) exceeds limit of 1"));
        assert_eq!(package.total_read, 2);
    }

    /// 이것을 실패시키는 것: 실패한 같은 엔트리의 반복 읽기를 누적하지 않거나 초과 후 다시 해제하기.
    #[test]
    fn zip9_fix4_repeated_entry_failures_accumulate() {
        let bytes = make_zip(&[("a", b"ab")]);
        let mut package =
            open_package(&bytes, PackageLimits { entry: 1, total: 5, entries: 1 }).unwrap();
        for expected in [2, 4, 6] {
            assert!(package.read(0).is_err());
            assert_eq!(package.total_read, expected);
        }
        for _ in 0..2 {
            assert!(package.read(0).is_err());
            assert_eq!(package.total_read, 6, "no further output after exceeding total");
        }
    }

    /// 이것을 실패시키는 것: 읽기 한도에서 남은 누적 예산을 무시하기.
    #[test]
    fn zip9_fix4_read_is_capped_by_remaining_total() {
        let bytes = make_zip(&[("a", b"ab"), ("b", b"0123456789")]);
        let mut package =
            open_package(&bytes, PackageLimits { entry: 32, total: 3, entries: 2 }).unwrap();
        assert_eq!(package.read(0).unwrap(), b"ab");
        let err = package.read(1).expect_err("only one byte plus the overflow probe remains");
        assert!(matches!(err, CoreError::InvalidStructure { reason, .. }
            if reason == "total decompressed data (4 bytes) exceeds limit of 3"));
        assert_eq!(package.total_read, 4);
        assert!(package.read(0).is_err());
        assert_eq!(package.total_read, 4);
    }

    /// 이것을 실패시키는 것: read_to_end 오류를 누적 갱신 전에 반환하기.
    #[test]
    fn zip9_fix4_repeated_crc_failures_accumulate() {
        let mut bytes = make_zip(&[("a", b"ab")]);
        // Stored payload follows the 30-byte local header, filename and extra field.
        let name_len = u16::from_le_bytes(bytes[26..28].try_into().unwrap()) as usize;
        let extra_len = u16::from_le_bytes(bytes[28..30].try_into().unwrap()) as usize;
        bytes[30 + name_len + extra_len] ^= 1;
        let mut package =
            open_package(&bytes, PackageLimits { entry: 8, total: 5, entries: 1 }).unwrap();
        for expected in [2, 4] {
            let err = package.read(0).expect_err("CRC failure after reading the payload");
            assert!(matches!(err, CoreError::InvalidStructure { context, reason }
                if context == "ZIP" && reason.contains("Invalid checksum")));
            assert_eq!(package.total_read, expected);
        }
        assert!(package.read(0).is_err());
        assert_eq!(package.total_read, 6);
        assert!(package.read(0).is_err());
        assert_eq!(package.total_read, 6);
    }

    /// 이것을 실패시키는 것: 정확한 상한을 초과로 취급하거나 초과 이후 빈 엔트리를 허용하기.
    #[test]
    fn zip9_fix4_exact_budget_and_empty_entry_boundaries() {
        let bytes = make_zip(&[("a", b"ab"), ("empty", b"")]);
        let mut package =
            open_package(&bytes, PackageLimits { entry: 2, total: 4, entries: 2 }).unwrap();
        for _ in 0..2 {
            assert_eq!(package.read(0).unwrap(), b"ab");
        }
        assert!(package.read(1).unwrap().is_empty());
        assert_eq!(package.total_read, 4);
        assert!(package.read(0).is_err());
        assert_eq!(package.total_read, 5);
        assert!(package.read(1).is_err());
        assert_eq!(package.total_read, 5);
    }

    /// 이것을 실패시키는 것: 0바이트 예산에서 빈 엔트리를 거부하거나 초과 감지용 1바이트를 빼기.
    #[test]
    fn zip9_fix4_zero_budget_still_checks_for_output() {
        let bytes = make_zip(&[("a", b"ab"), ("empty", b"")]);
        let mut package =
            open_package(&bytes, PackageLimits { entry: 0, total: 0, entries: 2 }).unwrap();
        assert!(package.read(1).unwrap().is_empty());
        assert!(package.read(0).is_err());
        assert_eq!(package.total_read, 1);
        assert!(package.read(1).is_err());
        assert_eq!(package.total_read, 1);
    }
}
