//! ZIP package reader for HWPX files.
//!
//! [`PackageReader`] wraps a `ZipArchive` and provides safe access
//! to the files inside an HWPX document archive.

use std::io::{Cursor, Read};

use zip::ZipArchive;

use crate::error::{HwpxError, HwpxResult};

// ── Safety limits ────────────────────────────────────────────────

/// Maximum decompressed size of a single entry (50 MB).
pub(crate) const MAX_ENTRY_SIZE: u64 = 50 * 1024 * 1024;

/// Maximum total decompressed size across all entries (500 MB).
pub(crate) const MAX_TOTAL_SIZE: u64 = 500 * 1024 * 1024;

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

// ── PackageReader ────────────────────────────────────────────────

/// Reader for HWPX ZIP archives.
///
/// Validates structure and provides access to individual XML files
/// within the archive. Enforces safety limits on decompressed data
/// to prevent ZIP bomb attacks.
pub struct PackageReader<'a> {
    archive: ZipArchive<Cursor<&'a [u8]>>,
    /// Decoded names in archive order. zip 9's `by_name` only matches raw
    /// UTF-8 bytes, so CP437 entries must be read through their index.
    paths: Vec<String>,
    index_by_path: std::collections::BTreeMap<String, usize>,
    pub(crate) order: super::package_order::PackageOrder,
    /// Cumulative bytes decompressed so far.
    total_read: u64,
}

/// Metadata about a single ZIP entry inside an HWPX package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageEntryInfo {
    /// Full archive path (for example, `Contents/section0.xml`).
    pub path: String,
    /// Uncompressed size reported by the ZIP entry header.
    pub size: u64,
    /// Compressed size reported by the ZIP entry header.
    pub compressed_size: u64,
}

impl<'a> PackageReader<'a> {
    /// Opens an HWPX archive from raw bytes.
    ///
    /// Validates:
    /// - The bytes form a valid ZIP archive
    /// - The entry count is within safety limits
    /// - A `mimetype` file exists with an accepted value
    pub fn new(bytes: &'a [u8]) -> HwpxResult<Self> {
        let directory = super::package_directory::preflight(bytes, MAX_ENTRIES)?;
        let cursor = Cursor::new(bytes);
        let archive = directory.open(cursor)?;

        let mut paths = Vec::new();
        let mut index_by_path = std::collections::BTreeMap::new();
        let mut keys = std::collections::BTreeSet::new();
        let metadata = archive.metadata();
        for (index, name) in archive.file_names().enumerate() {
            let path = name.map_err(|e| HwpxError::Zip(e.to_string()))?.into_owned();
            // Empty directory records are ZIP metadata, not package parts.
            // Hancom fixtures contain these. Only the final directory marker
            // is removed; every actual component still uses the shared policy.
            let directory = path.strip_suffix('/');
            let key = crate::package_path::normalize_part_name(directory.unwrap_or(&path))?;
            // 이것을 실패시키는 것: 디코딩/정규 키가 같은 파일·디렉터리를 병합하기.
            if !keys.insert(key.to_string()) {
                return Err(HwpxError::InvalidStructure {
                    detail: format!("duplicate decoded ZIP entry name '{key}'"),
                });
            }
            if directory.is_some() {
                let entry = metadata.entry(index).map_err(|e| HwpxError::Zip(e.to_string()))?;
                // 이것을 실패시키는 것: 후행 /만 붙여 데이터 파트의 검사를 우회하기.
                if entry.size() != 0 {
                    return Err(HwpxError::InvalidStructure {
                        detail: format!("nonempty ZIP directory entry '{path}'"),
                    });
                }
                continue;
            }
            // Keep physical archive indices even when directory records were skipped.
            index_by_path.insert(path.clone(), index);
            paths.push(path);
        }
        let mut reader =
            Self { archive, paths, index_by_path, order: Default::default(), total_read: 0 };
        reader.validate_mimetype()?;
        let mut optional = |path: &str| {
            if reader.index_by_path.contains_key(path) {
                reader.read_entry(path).map(Some)
            } else {
                Ok(None)
            }
        };
        let content = optional(super::package_order::CONTENT)?;
        let container = optional(super::package_order::CONTAINER)?;
        reader.order = super::package_order::PackageOrder::resolve(
            &reader.paths,
            content.as_deref(),
            container.as_deref(),
        );

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

    /// Returns a section in manifest-resolved spine reading order.
    pub fn read_section_xml(&mut self, index: usize) -> HwpxResult<String> {
        let path = self
            .section_paths()
            .get(index)
            .cloned()
            .ok_or_else(|| HwpxError::MissingFile { path: format!("spine section {index}") })?;
        self.read_entry(&path)
    }

    /// Archive paths of sections in document reading order. Unsupported package
    /// structures use file-number fallback with decoder ContentLost warnings.
    pub fn section_paths(&self) -> &[String] {
        &self.order.sections
    }

    /// Returns the number of sections in document reading order.
    pub fn section_count(&self) -> usize {
        self.section_paths().len()
    }

    /// Returns metadata for every entry in the archive.
    ///
    /// Entries are returned in ZIP order so callers can compare package
    /// structure directly against a fixture.
    pub fn list_entries(&mut self) -> HwpxResult<Vec<PackageEntryInfo>> {
        let mut entries = Vec::with_capacity(self.archive.len());
        for index in 0..self.archive.len() {
            let file = self.archive.by_index(index).map_err(|e| HwpxError::Zip(e.to_string()))?;
            entries.push(PackageEntryInfo {
                path: file.name().map_err(|e| HwpxError::Zip(e.to_string()))?.into_owned(),
                size: file.size(),
                compressed_size: file.compressed_size(),
            });
        }
        Ok(entries)
    }

    /// Reads an arbitrary archive entry as UTF-8 text.
    ///
    /// This is primarily useful for package-census tooling that needs raw
    /// access to files such as `Contents/content.hpf`.
    pub fn read_text_entry(&mut self, path: &str) -> HwpxResult<String> {
        self.read_entry(path)
    }

    /// Reads all `Contents/masterpage*.xml` entries from the archive.
    ///
    /// Returns a map from masterpage index to XML content.
    /// E.g., `{0: "<masterPage>...</masterPage>"}` for `Contents/masterpage0.xml`.
    pub fn read_masterpage_xmls(&mut self) -> HwpxResult<std::collections::HashMap<usize, String>> {
        let mp_paths: Vec<(usize, String)> = self
            .paths
            .iter()
            .filter_map(|name| {
                let stripped = name.strip_prefix("Contents/masterpage")?;
                let idx_str = stripped.strip_suffix(".xml")?;
                let idx: usize = idx_str.parse().ok()?;
                Some((idx, name.to_string()))
            })
            .collect();

        let mut result = std::collections::HashMap::new();
        for (idx, path) in mp_paths {
            let xml = self.read_entry(&path)?;
            result.insert(idx, xml);
        }
        Ok(result)
    }

    /// Reads all `Chart/*.xml` entries from the archive into a map.
    ///
    /// Each entry's full path (e.g. `"Chart/chart1.xml"`) becomes the key,
    /// and the XML string becomes the value.
    pub fn read_chart_xmls(&mut self) -> HwpxResult<std::collections::HashMap<String, String>> {
        let chart_paths: Vec<String> = self
            .paths
            .iter()
            .filter(|name| name.starts_with("Chart/") && name.ends_with(".xml"))
            .map(|s| s.to_string())
            .collect();

        let mut map = std::collections::HashMap::new();
        for path in chart_paths {
            let xml = self.read_entry(&path)?;
            map.insert(path, xml);
        }
        Ok(map)
    }

    /// Reads all `BinData/*` entries from the archive into an
    /// [`hwpforge_core::image::ImageStore`].
    ///
    /// Each entry's filename (without the `BinData/` prefix) becomes the
    /// key in the store, and the raw bytes become the value.
    ///
    /// Part names were validated before any payload was read; stripping the
    /// common prefix preserves their uniqueness.
    pub fn read_all_bindata(&mut self) -> HwpxResult<hwpforge_core::image::ImageStore> {
        let bindata_paths: Vec<String> = self
            .paths
            .iter()
            .filter(|name| name.starts_with("BinData/") && name.len() > "BinData/".len())
            .map(|s| s.to_string())
            .collect();

        let mut store = hwpforge_core::image::ImageStore::new();

        for path in bindata_paths {
            let data = self.read_binary_entry(&path)?;
            let raw_key = path.strip_prefix("BinData/").unwrap_or(&path);
            let key = crate::package_path::normalize_part_name(raw_key)?;
            store.insert(key, data);
        }

        Ok(store)
    }

    /// Reads a single entry from the archive as raw bytes.
    ///
    /// Similar to [`read_entry`] but returns `Vec<u8>` instead of `String`.
    pub(crate) fn read_binary_entry(&mut self, path: &str) -> HwpxResult<Vec<u8>> {
        let index = self.entry_index(path)?;
        let file = self
            .archive
            .by_index(index)
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
        let index = self.entry_index(path)?;
        let file = self
            .archive
            .by_index(index)
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

    fn entry_index(&self, path: &str) -> HwpxResult<usize> {
        self.index_by_path
            .get(path)
            .copied()
            .ok_or_else(|| HwpxError::MissingFile { path: path.to_string() })
    }
}

impl std::fmt::Debug for PackageReader<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PackageReader")
            .field("entries", &self.archive.len())
            .field("sections", &self.section_count())
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

    /// Build raw-name fixtures independently of zip's filename encoding policy.
    fn raw_names_zip(entries: &[(&[u8], &[u8])], utf8_flag: bool) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        writer.start_file("mimetype", opts).expect("mimetype");
        writer.write_all(b"application/hwp+zip").expect("mimetype payload");
        for (index, (name, payload)) in entries.iter().enumerate() {
            writer.start_file(format!("{index:0width$}", width = name.len()), opts).expect("entry");
            writer.write_all(payload).expect("payload");
        }
        let mut bytes = writer.finish().expect("finish").into_inner();
        let offsets: Vec<_> = {
            let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("archive");
            (1..archive.len())
                .map(|i| {
                    let file = archive.by_index(i).expect("entry");
                    (file.header_start() as usize, file.central_header_start() as usize)
                })
                .collect()
        };
        for ((name, _), (local, central)) in entries.iter().zip(offsets) {
            for (start, name_offset, flag_offset) in [(local, 30, 6), (central, 46, 8)] {
                bytes[start + name_offset..start + name_offset + name.len()].copy_from_slice(name);
                let flags = if utf8_flag { 1u16 << 11 } else { 0 };
                bytes[start + flag_offset..start + flag_offset + 2]
                    .copy_from_slice(&flags.to_le_bytes());
            }
        }
        bytes
    }

    /// 이것을 실패시키는 것: 디코딩한 CP437 이름을 zip 9 by_name()에 다시 전달하기.
    #[test]
    fn zip9_cp437_parts_are_read_by_decoded_name() {
        let bytes = raw_names_zip(
            &[
                (b"BinData/caf\x82.png", b"image"),
                (b"Chart/caf\x82.xml", b"<chart/>"),
                (b"Contents/masterpage0.xml", b"<master/>"),
            ],
            false,
        );
        let mut reader = PackageReader::new(&bytes).expect("package");
        assert_eq!(reader.read_binary_entry("BinData/café.png").expect("binary"), b"image");
        assert_eq!(
            reader.read_all_bindata().expect("images").get("café.png"),
            Some(b"image".as_slice())
        );
        assert_eq!(reader.read_chart_xmls().expect("charts")["Chart/café.xml"], "<chart/>");
        assert_eq!(reader.read_masterpage_xmls().expect("masters")[&0], "<master/>");
        assert_eq!(reader.list_entries().expect("entries")[1].path, "BinData/café.png");
    }

    /// 이것을 실패시키는 것: UTF-8 플래그 없는 유효 UTF-8을 CP437로 다시 해석하기.
    #[test]
    fn zip9_valid_utf8_without_flag_uses_utf8() {
        let bytes = raw_names_zip(&[("BinData/한글.png".as_bytes(), b"image")], false);
        let mut reader = PackageReader::new(&bytes).expect("package");
        assert_eq!(
            reader.read_all_bindata().expect("images").get("한글.png"),
            Some(b"image".as_slice())
        );
    }

    /// 이것을 실패시키는 것: 잘못된 UTF-8을 U+FFFD로 치환해 서로 다른 파트를 합치기.
    #[test]
    fn zip9_invalid_utf8_with_flag_falls_back_to_cp437() {
        let bytes = raw_names_zip(&[(b"BinData/caf\x82.png", b"image")], true);
        let mut reader = PackageReader::new(&bytes).expect("package");
        assert_eq!(
            reader.read_all_bindata().expect("images").get("café.png"),
            Some(b"image".as_slice())
        );
        assert!(matches!(
            reader.read_binary_entry("BinData/caf�.png"),
            Err(HwpxError::MissingFile { .. })
        ));
    }

    /// 이것을 실패시키는 것: Unicode Path extra의 유효 이름 대신 원래 raw 이름으로 조회하기,
    /// 또는 CRC가 다른 extra를 신뢰하기.
    #[test]
    fn zip9_unicode_path_extra_uses_validated_name_for_part_lookup() {
        for valid_crc in [true, false] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            writer.start_file("mimetype", SimpleFileOptions::default()).expect("mimetype");
            writer.write_all(b"application/hwp+zip").expect("payload");
            let mut options = zip::write::FullFileOptions::default();
            // CRC32 of the original ASCII name "BinData/legacy.png".
            let crc = if valid_crc { 0x328f_7257u32 } else { 0 };
            let mut extra = vec![1];
            extra.extend_from_slice(&crc.to_le_bytes());
            extra.extend_from_slice("BinData/한글.png".as_bytes());
            options.add_extra_field(0x7075, extra, true).expect("Unicode Path extra");
            writer.start_file("BinData/legacy.png", options).expect("entry");
            writer.write_all(b"image").expect("image");
            let bytes = writer.finish().expect("finish").into_inner();
            let mut reader = PackageReader::new(&bytes).expect("package");
            let (present, absent) = if valid_crc {
                ("BinData/한글.png", "BinData/legacy.png")
            } else {
                ("BinData/legacy.png", "BinData/한글.png")
            };
            assert_eq!(reader.read_binary_entry(present).expect("resolved name"), b"image");
            assert!(matches!(reader.read_binary_entry(absent), Err(HwpxError::MissingFile { .. })));
            assert_eq!(reader.list_entries().expect("entries")[1].path, present);
        }
    }

    /// 이것을 실패시키는 것: 디코딩 이름 충돌 검사 삭제(뒤 파트로 덮어쓰게 됨).
    #[test]
    fn zip9_rejects_ambiguous_decoded_names_before_reading_mimetype() {
        let bytes = raw_names_zip(
            &[(b"BinData/caf\x82.png", b"first"), ("BinData/café.png".as_bytes(), b"second")],
            false,
        );
        let err = PackageReader::new(&bytes).expect_err("ambiguous name");
        assert!(matches!(err, HwpxError::InvalidStructure { detail }
            if detail == "duplicate decoded ZIP entry name 'BinData/café.png'"));
    }

    fn with_zip64_footer(mut bytes: Vec<u8>) -> Vec<u8> {
        let end = bytes.len() - 22;
        let mut eocd = bytes.split_off(end);
        let count = u16::from_le_bytes(eocd[10..12].try_into().unwrap()) as u64;
        let size = u32::from_le_bytes(eocd[12..16].try_into().unwrap()) as u64;
        let offset = u32::from_le_bytes(eocd[16..20].try_into().unwrap()) as u64;
        bytes.extend_from_slice(b"PK\x06\x06");
        bytes.extend_from_slice(&44u64.to_le_bytes());
        bytes.extend_from_slice(&[45, 0, 45, 0]);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&size.to_le_bytes());
        bytes.extend_from_slice(&offset.to_le_bytes());
        bytes.extend_from_slice(b"PK\x06\x07");
        bytes.extend_from_slice(&0u32.to_le_bytes());
        bytes.extend_from_slice(&(end as u64).to_le_bytes());
        bytes.extend_from_slice(&1u32.to_le_bytes());
        eocd[8..12].fill(0xff);
        eocd[12..20].fill(0xff);
        bytes.extend(eocd);
        bytes
    }

    /// 이것을 실패시키는 것: 병합된 len()만 상한에 대거나 ZIP64 경로에서 검사를 생략하기.
    #[test]
    fn zip9_fix1_physical_duplicate_count_is_capped_before_merging() {
        let entries = vec![(b"unused".as_slice(), b"".as_slice()); 10_001];
        let bytes = raw_names_zip(&entries, false);
        for bytes in [bytes.clone(), with_zip64_footer(bytes)] {
            assert_eq!(ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 2);
            assert!(
                matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
                if detail == "archive has 10002 entries, exceeds limit of 10000")
            );
        }
    }

    /// 이것을 실패시키는 것: 원본 중복 검사 삭제(상한 이하 중복도 병합하면 안 됨).
    #[test]
    fn zip9_fix1_rejects_original_duplicates_under_limit() {
        let bytes = raw_names_zip(&[(b"unused", b"first"), (b"unused", b"last")], false);
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
            if detail == "duplicate original ZIP entry name"));
    }

    fn unicode_alias_zip(alias: &str) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file("mimetype", SimpleFileOptions::default()).unwrap();
        writer.write_all(b"application/hwp+zip").unwrap();
        let mut options = zip::write::FullFileOptions::default();
        let mut extra = vec![1];
        extra.extend_from_slice(&0x328f_7257u32.to_le_bytes()); // CRC32("BinData/legacy.png")
        extra.extend_from_slice(alias.as_bytes());
        options.add_extra_field(0x7075, extra, true).unwrap();
        writer.start_file("BinData/legacy.png", options).unwrap();
        writer.write_all(b"application/hwp+zip").unwrap();
        writer.finish().unwrap().into_inner()
    }

    /// 이것을 실패시키는 것: 원본 이름 검사를 유효 이름 검사로 대체하기.
    #[test]
    fn zip9_fix1_original_duplicates_with_distinct_unicode_names_are_rejected() {
        let mut bytes = unicode_alias_zip("renamed1");
        let end = bytes.len() - 22;
        let mut eocd = bytes.split_off(end);
        let archive = ZipArchive::new(Cursor::new([bytes.as_slice(), &eocd].concat())).unwrap();
        let record_start = archive.metadata().entry(1).unwrap().central_header_start() as usize;
        let mut record = bytes[record_start..].to_vec();
        let alias = record.windows(8).position(|s| s == b"renamed1").unwrap();
        record[alias..alias + 8].copy_from_slice(b"renamed2");
        let old_size = u32::from_le_bytes(eocd[12..16].try_into().unwrap());
        eocd[8..12].copy_from_slice(&[3, 0, 3, 0]);
        eocd[12..16].copy_from_slice(&(old_size + record.len() as u32).to_le_bytes());
        bytes.extend(record);
        bytes.extend(eocd);
        // zip sees distinct effective keys, despite identical original names.
        assert_eq!(ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 3);
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
            if detail == "duplicate original ZIP entry name"));
    }

    /// 이것을 실패시키는 것: 물리 수와 병합 후 수의 대조 삭제.
    #[test]
    fn zip9_fix1_unicode_extra_cannot_merge_into_mimetype() {
        let bytes = unicode_alias_zip("mimetype");
        assert_eq!(ZipArchive::new(Cursor::new(&bytes)).unwrap().len(), 1);
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
            if detail == "duplicate effective ZIP entry name"));
    }

    /// 이것을 실패시키는 것: BinData만 검사하거나 '..'/'.'/빈 성분/역슬래시/절대 경로 조건 삭제.
    #[test]
    fn zip9_fix1_rejects_noncanonical_parts_before_payload_reads() {
        for name in [
            "BinData/../a.png",
            "Contents/./section0.xml",
            "Chart//a.xml",
            "META-INF//",
            "Contents\\section0.xml",
            "/Contents/header.xml",
            "C:/header.xml",
            "C:header.xml",
            "..",
            ".",
            "//server/share/file",
        ] {
            let bytes = raw_names_zip(&[(name.as_bytes(), b"unread")], false);
            assert!(
                matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
                if detail.starts_with("invalid package part name ")),
                "{name}"
            );
        }
        let bytes =
            raw_names_zip(&[(b"BinData/a.png", b"first"), (b"BinData/../a.png", b"last")], false);
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { .. })));
        // The effective Unicode name must pass the same policy.
        assert!(matches!(
            PackageReader::new(&unicode_alias_zip("Contents/../a.xml")),
            Err(HwpxError::InvalidStructure { .. })
        ));
    }

    /// 이것을 실패시키는 것: 빈 디렉터리까지 파트로 거부하거나 디렉터리를 뺀 인덱스로 읽기.
    #[test]
    fn zip9_fix1_empty_directory_markers_are_not_parts() {
        let bytes = raw_names_zip(
            &[
                (b"Contents/", b""),
                (b"BinData/folder/", b""),
                (b"BinData/folder/a.png", b"image"),
                (b"Contents/section0.xml", b"<sec/>"),
            ],
            false,
        );
        let mut reader = PackageReader::new(&bytes).unwrap();
        assert_eq!(reader.read_section_xml(0).unwrap(), "<sec/>");
        assert_eq!(
            reader.read_all_bindata().unwrap().get("folder/a.png"),
            Some(b"image".as_slice())
        );
        assert!(!reader.paths.iter().any(|p| p.ends_with('/')));
        assert!(matches!(
            reader.read_binary_entry("Contents/"),
            Err(HwpxError::MissingFile { .. })
        ));
    }

    /// 이것을 실패시키는 것: 디렉터리 표시는 내용·정규 키 충돌·내부 성분 검사에서 제외하기.
    #[test]
    fn zip9_fix1_directory_markers_cannot_hide_parts() {
        let bytes = raw_names_zip(&[(b"BinData/a.png/", b"image")], false);
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
            if detail == "nonempty ZIP directory entry 'BinData/a.png/'"));
        for entries in [
            vec![(b"a/".as_slice(), b"".as_slice()), (b"a", b"file")],
            vec![(b"a/../", b"")],
            vec![(b"a//", b"")],
            vec![(b"/", b"")],
        ] {
            let bytes = raw_names_zip(&entries, false);
            assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { .. })));
        }
    }

    /// 이것을 실패시키는 것: EOCD 선언 개수만 검사해 추가 물리 레코드를 숨기기.
    #[test]
    fn zip9_fix1_forged_count_cannot_hide_physical_records() {
        let entries = vec![(b"unused".as_slice(), b"".as_slice()); 10_001];
        let mut bytes = raw_names_zip(&entries, false);
        let end = bytes.len() - 22;
        bytes[end + 8..end + 10].copy_from_slice(&1u16.to_le_bytes());
        bytes[end + 10..end + 12].copy_from_slice(&1u16.to_le_bytes());
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::InvalidStructure { detail })
            if detail.contains("10001 entries")));
    }

    /// 이것을 실패시키는 것: ZIP64/주석/접두 바이트가 있으면 정상 ZIP도 일괄 거부하기.
    #[test]
    fn zip9_fix1_accepts_zip64_comments_and_prefixes() {
        let bytes = raw_names_zip(&[(b"BinData/folder/a.png", b"image")], false);
        for mut bytes in [bytes.clone(), with_zip64_footer(bytes)] {
            let end = bytes.len() - 22;
            let comment = b"comment PK\x05\x06";
            bytes[end + 20..end + 22].copy_from_slice(&(comment.len() as u16).to_le_bytes());
            bytes.extend_from_slice(comment);
            for prefix in [b"".as_slice(), b"prefix".as_slice()] {
                let bytes = [prefix, &bytes].concat();
                let mut reader = PackageReader::new(&bytes).unwrap();
                assert_eq!(
                    reader.read_all_bindata().unwrap().get("folder/a.png"),
                    Some(b"image".as_slice())
                );
            }
        }
    }

    /// 이것을 실패시키는 것: zip의 첫 CDFH 대신 EOCD-size로 디렉터리를 선택하기.
    #[test]
    fn zip9_fix1_rejects_different_directory_interpretations() {
        let mut bytes = raw_names_zip(&[], false);
        let end = bytes.len() - 22;
        let eocd = bytes.split_off(end);
        let start = u32::from_le_bytes(eocd[16..20].try_into().unwrap()) as usize;
        let record = bytes[start..].to_vec();
        bytes.extend(record);
        bytes.extend(eocd);
        assert!(super::super::package_directory::preflight(&bytes, MAX_ENTRIES).is_err());
        assert!(matches!(PackageReader::new(&bytes), Err(HwpxError::Zip(detail))
            if detail == "invalid ZIP central directory framing"));
    }

    /// 이것을 실패시키는 것: 경계/개수/디스크 검증을 삭제해 잘린 디렉터리나 오프셋을 신뢰하기.
    #[test]
    fn zip9_fix1_rejects_malformed_directory_framing() {
        let bytes = raw_names_zip(&[(b"unused", b"payload")], false);
        let end = bytes.len() - 22;
        let start = u32::from_le_bytes(bytes[end + 16..end + 20].try_into().unwrap()) as usize;
        for (at, value) in [
            (end + 4, 1),
            (end + 6, 1),
            (end + 8, 1),
            (end + 10, 3),
            (end + 12, 0xff),
            (end + 16, 0xff),
            (end + 20, 1),
            (start, 0),
            (start + 28, 0xff),
        ] {
            let mut bad = bytes.clone();
            bad[at] = value;
            assert!(super::super::package_directory::preflight(&bad, MAX_ENTRIES).is_err());
            assert!(matches!(PackageReader::new(&bad), Err(HwpxError::Zip(_))), "offset {at}");
        }
        let mut bad = bytes.clone();
        bad[end + 8..end + 12].copy_from_slice(&[1, 0, 1, 0]);
        assert!(super::super::package_directory::preflight(&bad, MAX_ENTRIES).is_err());
        assert!(matches!(PackageReader::new(&bad), Err(HwpxError::Zip(_))));
        let zip64 = with_zip64_footer(bytes);
        for at in [
            end + 4,
            end + 16,
            end + 20,
            end + 24,
            end + 40,
            end + 48,
            end + 60,
            end + 64,
            end + 72,
        ] {
            let mut bad = zip64.clone();
            bad[at] = 0xff;
            assert!(super::super::package_directory::preflight(&bad, MAX_ENTRIES).is_err());
            assert!(
                matches!(PackageReader::new(&bad), Err(HwpxError::Zip(_))),
                "ZIP64 offset {at}"
            );
        }
        for length in [0, 21, end + 5, zip64.len() - 1] {
            assert!(PackageReader::new(&zip64[..length]).is_err());
        }
    }

    /// 이것을 실패시키는 것: 엔트리 수 검사 삭제/후이동 또는 >를 >=로 바꾸기.
    #[test]
    fn zip9_decoder_entry_count_limit_precedes_payload_reads() {
        for count in [MAX_ENTRIES, MAX_ENTRIES + 1] {
            let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
            let opts =
                SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
            for index in 0..count {
                writer.start_file(index.to_string(), opts).expect("entry");
            }
            let bytes = writer.finish().expect("finish").into_inner();
            let err = PackageReader::new(&bytes).expect_err("missing mimetype or entry cap");
            if count == MAX_ENTRIES {
                assert!(matches!(err, HwpxError::MissingFile { path } if path == MIMETYPE_PATH));
            } else {
                assert!(matches!(err, HwpxError::InvalidStructure { detail }
                    if detail == "archive has 10001 entries, exceeds limit of 10000"));
            }
        }
    }

    /// 이것을 실패시키는 것: 텍스트/바이너리 take(cap + 1) 또는 크기 검사 삭제.
    #[test]
    fn zip9_decoder_caps_high_ratio_text_and_binary_entries() {
        let payload = "x".repeat(MAX_ENTRY_SIZE as usize + 1024);
        let bytes = make_hwpx_zip("application/hwp+zip", &payload, &[]);
        let mut archive = ZipArchive::new(Cursor::new(&bytes)).expect("archive");
        let file = archive.by_name(HEADER_PATH).expect("header");
        assert!(file.size() / file.compressed_size() > 100);
        for binary in [false, true] {
            let mut reader = PackageReader::new(&bytes).expect("package");
            let err = if binary {
                reader.read_binary_entry(HEADER_PATH).expect_err("cap")
            } else {
                reader.read_header_xml().expect_err("cap")
            };
            assert!(matches!(err, HwpxError::InvalidStructure { detail }
                if detail == format!("entry '{HEADER_PATH}' decompressed to {} bytes, exceeds limit of {MAX_ENTRY_SIZE}", MAX_ENTRY_SIZE + 1)));
        }
    }

    /// 이것을 실패시키는 것: 누적 예산 검사 삭제 또는 경계에서 > 를 >= 로 바꾸기.
    #[test]
    fn zip9_decoder_total_budget_includes_text_and_binary_reads() {
        let bytes = make_hwpx_zip("application/hwp+zip", "ab", &[]);
        for binary in [false, true] {
            let mut reader = PackageReader::new(&bytes).expect("package");
            reader.total_read = MAX_TOTAL_SIZE - 2;
            if binary {
                assert_eq!(reader.read_binary_entry(HEADER_PATH).expect("exact cap"), b"ab");
            } else {
                assert_eq!(reader.read_header_xml().expect("exact cap"), "ab");
            }
            assert_eq!(reader.total_read, MAX_TOTAL_SIZE);
            let err = if binary {
                reader.read_binary_entry(HEADER_PATH).expect_err("total")
            } else {
                reader.read_header_xml().expect_err("total")
            };
            assert!(matches!(err, HwpxError::InvalidStructure { detail }
                if detail == format!("total decompressed data ({} bytes) exceeds limit of {MAX_TOTAL_SIZE}", MAX_TOTAL_SIZE + 2)));
        }
    }
}
