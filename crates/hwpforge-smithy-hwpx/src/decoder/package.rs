//! ZIP package reader for HWPX files.
//!
//! [`PackageReader`] wraps a `ZipArchive` and provides safe access
//! to the files inside an HWPX document archive.

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::error::{HwpxError, HwpxResult};

// ── Safety limits ────────────────────────────────────────────────

/// Maximum decompressed size of a single entry (50 MB).
const MAX_ENTRY_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum total decompressed size across all entries (500 MB).
const MAX_TOTAL_SIZE: u64 = 500 * 1024 * 1024;

/// Maximum number of entries in the archive.
const MAX_ENTRIES: usize = 10_000;

// ── HWPX constants ───────────────────────────────────────────────

/// Accepted mimetype values (first entry in ZIP, uncompressed).
const ACCEPTED_MIMETYPES: &[&str] =
    &["application/hwp+zip", "application/haansofthwp+zip", "application/vnd.hancom.hwp+zip"];

/// Path to the mimetype file inside the ZIP.
const MIMETYPE_PATH: &str = "mimetype";

/// Path to the header XML inside the ZIP.
const HEADER_PATH: &str = "Contents/header.xml";

/// Prefix for section XML files inside the ZIP.
const SECTION_PREFIX: &str = "Contents/section";

/// Suffix for section XML files inside the ZIP.
const SECTION_SUFFIX: &str = ".xml";

// ── PackageReader ────────────────────────────────────────────────

/// Reader for HWPX ZIP archives.
///
/// Validates structure and provides access to individual XML files
/// within the archive. Enforces safety limits on decompressed data
/// to prevent ZIP bomb attacks.
pub struct PackageReader {
    archive: ZipArchive<Cursor<Vec<u8>>>,
    section_count: usize,
    /// Cumulative bytes decompressed so far.
    total_read: u64,
}

impl PackageReader {
    /// Opens an HWPX archive from raw bytes.
    ///
    /// Validates:
    /// - The bytes form a valid ZIP archive
    /// - The entry count is within safety limits
    /// - A `mimetype` file exists with an accepted value
    pub fn new(bytes: &[u8]) -> HwpxResult<Self> {
        let cursor = Cursor::new(bytes.to_vec());
        let archive = ZipArchive::new(cursor).map_err(|e| HwpxError::Zip(e.to_string()))?;

        if archive.len() > MAX_ENTRIES {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "archive has {} entries, exceeds limit of {}",
                    archive.len(),
                    MAX_ENTRIES,
                ),
            });
        }

        // Count section files
        let section_count = archive
            .file_names()
            .filter(|name| name.starts_with(SECTION_PREFIX) && name.ends_with(SECTION_SUFFIX))
            .count();

        let mut reader = Self { archive, section_count, total_read: 0 };

        // Validate mimetype
        reader.validate_mimetype()?;

        Ok(reader)
    }

    /// Validates the `mimetype` file in the archive.
    fn validate_mimetype(&mut self) -> HwpxResult<()> {
        let content = self.read_entry(MIMETYPE_PATH)?;
        let trimmed = content.trim();

        if !ACCEPTED_MIMETYPES.contains(&trimmed) {
            return Err(HwpxError::InvalidMimetype { actual: trimmed.to_string() });
        }

        Ok(())
    }

    /// Returns the raw XML content of `Contents/header.xml`.
    pub fn read_header_xml(&mut self) -> HwpxResult<String> {
        self.read_entry(HEADER_PATH)
    }

    /// Returns the raw XML content of `Contents/section{index}.xml`.
    ///
    /// Sections are zero-indexed: section 0, section 1, etc.
    pub fn read_section_xml(&mut self, index: usize) -> HwpxResult<String> {
        let path = format!("{}{}{}", SECTION_PREFIX, index, SECTION_SUFFIX);
        self.read_entry(&path)
    }

    /// Returns the number of section files found in the archive.
    pub fn section_count(&self) -> usize {
        self.section_count
    }

    /// Reads all `BinData/*` entries from the archive into an [`ImageStore`].
    ///
    /// Each entry's filename (without the `BinData/` prefix) becomes the
    /// key in the store, and the raw bytes become the value.
    pub fn read_all_bindata(&mut self) -> HwpxResult<hwpforge_core::image::ImageStore> {
        let bindata_paths: Vec<String> = self
            .archive
            .file_names()
            .filter(|name| name.starts_with("BinData/") && name.len() > "BinData/".len())
            .map(|s| s.to_string())
            .collect();

        let mut store = hwpforge_core::image::ImageStore::new();

        for path in bindata_paths {
            let data = self.read_binary_entry(&path)?;
            let key = path.strip_prefix("BinData/").unwrap_or(&path);
            store.insert(key, data);
        }

        Ok(store)
    }

    /// Reads a single entry from the archive as raw bytes.
    ///
    /// Similar to [`read_entry`] but returns `Vec<u8>` instead of `String`.
    fn read_binary_entry(&mut self, path: &str) -> HwpxResult<Vec<u8>> {
        let file = self
            .archive
            .by_name(path)
            .map_err(|_| HwpxError::MissingFile { path: path.to_string() })?;

        let hint = file.size().min(MAX_ENTRY_SIZE) as usize;
        let mut limited = file.take(MAX_ENTRY_SIZE + 1);

        let mut buf = Vec::with_capacity(hint);
        std::io::Read::read_to_end(&mut limited, &mut buf)
            .map_err(|e| HwpxError::Zip(format!("read '{}': {}", path, e)))?;

        if buf.len() as u64 > MAX_ENTRY_SIZE {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "entry '{}' decompressed to {} bytes, exceeds limit of {}",
                    path,
                    buf.len(),
                    MAX_ENTRY_SIZE,
                ),
            });
        }

        self.total_read += buf.len() as u64;
        if self.total_read > MAX_TOTAL_SIZE {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "total decompressed data ({} bytes) exceeds limit of {}",
                    self.total_read, MAX_TOTAL_SIZE,
                ),
            });
        }

        Ok(buf)
    }

    /// Reads a single entry from the archive as a UTF-8 string.
    ///
    /// Uses `Read::take()` to enforce the per-entry size limit regardless
    /// of what the ZIP central directory reports (defense against ZIP bombs).
    fn read_entry(&mut self, path: &str) -> HwpxResult<String> {
        let file = self
            .archive
            .by_name(path)
            .map_err(|_| HwpxError::MissingFile { path: path.to_string() })?;

        // Use take() to enforce actual decompressed size limit.
        // file.size() comes from the ZIP header and can be spoofed,
        // so we cap the reader itself to MAX_ENTRY_SIZE + 1 bytes.
        let hint = file.size().min(MAX_ENTRY_SIZE) as usize;
        let mut limited = file.take(MAX_ENTRY_SIZE + 1);

        let mut buf = String::with_capacity(hint);
        limited
            .read_to_string(&mut buf)
            .map_err(|e| HwpxError::Zip(format!("read '{}': {}", path, e)))?;

        if buf.len() as u64 > MAX_ENTRY_SIZE {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "entry '{}' decompressed to {} bytes, exceeds limit of {}",
                    path,
                    buf.len(),
                    MAX_ENTRY_SIZE,
                ),
            });
        }

        // Enforce cumulative budget
        self.total_read += buf.len() as u64;
        if self.total_read > MAX_TOTAL_SIZE {
            return Err(HwpxError::InvalidStructure {
                detail: format!(
                    "total decompressed data ({} bytes) exceeds limit of {}",
                    self.total_read, MAX_TOTAL_SIZE,
                ),
            });
        }

        Ok(buf)
    }
}

impl std::fmt::Debug for PackageReader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageReader")
            .field("entries", &self.archive.len())
            .field("sections", &self.section_count)
            .field("total_read", &self.total_read)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use zip::write::SimpleFileOptions;
    use zip::ZipWriter;

    /// Helper: creates a minimal valid HWPX ZIP in memory.
    fn make_hwpx_zip(mimetype: &str, header_xml: &str, sections: &[&str]) -> Vec<u8> {
        let buf = Vec::new();
        let mut zip = ZipWriter::new(Cursor::new(buf));
        let opts = SimpleFileOptions::default();

        // mimetype must be first entry, stored (not compressed)
        let stored =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        zip.start_file("mimetype", stored).unwrap();
        zip.write_all(mimetype.as_bytes()).unwrap();

        // header.xml
        zip.start_file("Contents/header.xml", opts).unwrap();
        zip.write_all(header_xml.as_bytes()).unwrap();

        // section files
        for (i, content) in sections.iter().enumerate() {
            let path = format!("Contents/section{}.xml", i);
            zip.start_file(&path, opts).unwrap();
            zip.write_all(content.as_bytes()).unwrap();
        }

        zip.finish().unwrap().into_inner()
    }

    const MINIMAL_HEADER: &str =
        r#"<?xml version="1.0" encoding="UTF-8"?><head version="1.4" secCnt="1"></head>"#;

    const MINIMAL_SECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><sec></sec>"#;

    // ── Construction ─────────────────────────────────────────────

    #[test]
    fn new_valid_hwpx() {
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let reader = PackageReader::new(&bytes).unwrap();
        assert_eq!(reader.section_count(), 1);
    }

    #[test]
    fn new_alternative_mimetype() {
        let bytes =
            make_hwpx_zip("application/haansofthwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        assert!(PackageReader::new(&bytes).is_ok());
    }

    #[test]
    fn new_vnd_mimetype() {
        let bytes =
            make_hwpx_zip("application/vnd.hancom.hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        assert!(PackageReader::new(&bytes).is_ok());
    }

    #[test]
    fn new_not_a_zip() {
        let err = PackageReader::new(b"not a zip file").unwrap_err();
        assert!(matches!(err, HwpxError::Zip(_)));
    }

    #[test]
    fn new_wrong_mimetype() {
        let bytes = make_hwpx_zip("application/pdf", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let err = PackageReader::new(&bytes).unwrap_err();
        match err {
            HwpxError::InvalidMimetype { actual } => {
                assert_eq!(actual, "application/pdf");
            }
            _ => panic!("expected InvalidMimetype, got: {err:?}"),
        }
    }

    #[test]
    fn new_empty_zip_missing_mimetype() {
        let buf = Vec::new();
        let zip = ZipWriter::new(Cursor::new(buf));
        let bytes = zip.finish().unwrap().into_inner();
        let err = PackageReader::new(&bytes).unwrap_err();
        assert!(matches!(err, HwpxError::MissingFile { .. }));
    }

    // ── Reading entries ──────────────────────────────────────────

    #[test]
    fn read_header_xml() {
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let mut reader = PackageReader::new(&bytes).unwrap();
        let xml = reader.read_header_xml().unwrap();
        assert!(xml.contains("head"));
    }

    #[test]
    fn read_section_xml_index_0() {
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let mut reader = PackageReader::new(&bytes).unwrap();
        let xml = reader.read_section_xml(0).unwrap();
        assert!(xml.contains("sec"));
    }

    #[test]
    fn read_section_xml_out_of_range() {
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let mut reader = PackageReader::new(&bytes).unwrap();
        let err = reader.read_section_xml(99).unwrap_err();
        assert!(matches!(err, HwpxError::MissingFile { .. }));
    }

    #[test]
    fn multiple_sections() {
        let s0 = r#"<sec>section0</sec>"#;
        let s1 = r#"<sec>section1</sec>"#;
        let s2 = r#"<sec>section2</sec>"#;
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[s0, s1, s2]);
        let mut reader = PackageReader::new(&bytes).unwrap();
        assert_eq!(reader.section_count(), 3);
        assert!(reader.read_section_xml(0).unwrap().contains("section0"));
        assert!(reader.read_section_xml(1).unwrap().contains("section1"));
        assert!(reader.read_section_xml(2).unwrap().contains("section2"));
    }

    // ── Debug impl ───────────────────────────────────────────────

    #[test]
    fn debug_impl() {
        let bytes = make_hwpx_zip("application/hwp+zip", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        let reader = PackageReader::new(&bytes).unwrap();
        let dbg = format!("{reader:?}");
        assert!(dbg.contains("PackageReader"));
        assert!(dbg.contains("sections: 1"));
    }

    // ── Mimetype trimming ────────────────────────────────────────

    #[test]
    fn mimetype_with_trailing_whitespace() {
        let bytes = make_hwpx_zip("application/hwp+zip  \n", MINIMAL_HEADER, &[MINIMAL_SECTION]);
        assert!(PackageReader::new(&bytes).is_ok());
    }
}
