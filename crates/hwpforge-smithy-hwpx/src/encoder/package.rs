//! HWPX ZIP package writer.
//!
//! Creates the ZIP archive structure required by the HWPX format.
//! Mirrors the [`crate::decoder::package`] module for symmetry.

use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

use crate::error::{HwpxError, HwpxResult};

// ── HWPX constants ───────────────────────────────────────────────

/// The mimetype value written as the first (stored) entry.
const MIMETYPE: &[u8] = b"application/hwp+zip";

/// All 15 HWPX namespace URI declarations.
///
/// Used by header and section XML writers when constructing root elements.
pub(crate) const XMLNS_DECLS: &str = concat!(
    r#" xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app""#,
    r#" xmlns:hp="http://www.hancom.co.kr/hwpml/2011/paragraph""#,
    r#" xmlns:hp10="http://www.hancom.co.kr/hwpml/2016/paragraph""#,
    r#" xmlns:hs="http://www.hancom.co.kr/hwpml/2011/section""#,
    r#" xmlns:hc="http://www.hancom.co.kr/hwpml/2011/core""#,
    r#" xmlns:hh="http://www.hancom.co.kr/hwpml/2011/head""#,
    r#" xmlns:hhs="http://www.hancom.co.kr/hwpml/2011/history""#,
    r#" xmlns:hm="http://www.hancom.co.kr/hwpml/2011/master-page""#,
    r#" xmlns:hpf="http://www.hancom.co.kr/schema/2011/hpf""#,
    r#" xmlns:dc="http://purl.org/dc/elements/1.1/""#,
    r#" xmlns:opf="http://www.idpf.org/2007/opf/""#,
    r#" xmlns:ooxmlchart="http://www.hancom.co.kr/hwpml/2016/ooxmlchart""#,
    r#" xmlns:hwpunitchar="http://www.hancom.co.kr/hwpml/2016/HwpUnitChar""#,
    r#" xmlns:epub="http://www.idpf.org/2007/ops""#,
    r#" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0""#,
);

// ── Template XML constants ───────────────────────────────────────

/// HCF version descriptor. Note: `tagetApplication` is an intentional typo
/// preserved from the official format for compatibility.
/// The namespace URI matches the official 한글 output.
const VERSION_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="0" micro="5" buildNumber="0" os="1" xmlVersion="1.4" application="Hancom Office Hangul" appVersion="12, 0, 0, 0"/>"##;

/// META-INF/container.xml pointing to the content package file.
const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:hpf="http://www.hancom.co.kr/schema/2011/hpf"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;

/// META-INF/manifest.xml (empty manifest).
const MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;

/// Application settings with default caret position.
const SETTINGS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ha:HWPApplicationSetting xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"><ha:CaretPosition listIDRef="0" paraIDRef="0" pos="0"/></ha:HWPApplicationSetting>"#;

// ── content.hpf generator ────────────────────────────────────────

/// Generates the OPF content manifest listing header, sections, and images.
///
/// Matches the structure produced by 한글: full namespace declarations,
/// metadata section, header + sections + settings + images in manifest,
/// and header + sections in spine (images are NOT in spine).
fn generate_content_hpf(section_count: usize, image_paths: &[String]) -> String {
    let mut manifest_items = String::from(
        r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#,
    );
    let mut spine_refs = String::from(r#"<opf:itemref idref="header" linear="yes"/>"#);

    for i in 0..section_count {
        use std::fmt::Write as _;
        write!(
            manifest_items,
            r#"<opf:item id="section{i}" href="Contents/section{i}.xml" media-type="application/xml"/>"#,
        )
        .expect("write to String is infallible");
        write!(spine_refs, r#"<opf:itemref idref="section{i}" linear="yes"/>"#)
            .expect("write to String is infallible");
    }

    // settings in manifest (not in spine)
    manifest_items
        .push_str(r#"<opf:item id="settings" href="settings.xml" media-type="application/xml"/>"#);

    // Image entries in manifest (not in spine).
    // The `id` must match the `binaryItemIDRef` in section XML (filename stem, no extension).
    // `isEmbeded="1"` (intentional typo matching 한글's output) marks the binary as embedded.
    for path in image_paths {
        use std::fmt::Write as _;
        let media_type = guess_image_media_type(path);
        // Strip extension: "test_image.png" → "test_image"
        let stem = match path.rfind('.') {
            Some(pos) => &path[..pos],
            None => path.as_str(),
        };
        write!(
            manifest_items,
            r#"<opf:item id="{stem}" href="BinData/{path}" media-type="{media_type}" isEmbeded="1"/>"#,
        )
        .expect("write to String is infallible");
    }

    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<opf:package{xmlns} version="" unique-identifier="" id="">"#,
            r#"<opf:metadata>"#,
            r#"<opf:title/>"#,
            r#"<opf:language>ko</opf:language>"#,
            r#"</opf:metadata>"#,
            r#"<opf:manifest>{manifest_items}</opf:manifest>"#,
            r#"<opf:spine>{spine_refs}</opf:spine>"#,
            r#"</opf:package>"#,
        ),
        xmlns = XMLNS_DECLS,
        manifest_items = manifest_items,
        spine_refs = spine_refs,
    )
}

/// Guesses the MIME type for an image based on file extension.
fn guess_image_media_type(path: &str) -> &'static str {
    let lower = path.to_ascii_lowercase();
    if lower.ends_with(".png") {
        "image/png"
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        "image/jpeg"
    } else if lower.ends_with(".gif") {
        "image/gif"
    } else if lower.ends_with(".bmp") {
        "image/bmp"
    } else if lower.ends_with(".wmf") {
        "image/x-wmf"
    } else if lower.ends_with(".emf") {
        "image/x-emf"
    } else {
        "application/octet-stream"
    }
}

// ── PackageWriter ────────────────────────────────────────────────

/// Writes a valid HWPX ZIP archive.
///
/// Assembles header XML, section XMLs, and optional binary data (images)
/// into the standard HWPX ZIP structure. The resulting bytes can be
/// written directly to a `.hwpx` file.
pub(crate) struct PackageWriter;

impl PackageWriter {
    /// Assembles header XML, section XMLs, and optional images into a HWPX ZIP.
    ///
    /// # Arguments
    ///
    /// * `header_xml` — Serialized `Contents/header.xml` content.
    /// * `section_xmls` — Serialized section XML strings (`Contents/section0.xml`, etc.).
    /// * `images` — Pairs of `(filename, data)` written under `BinData/`.
    ///
    /// # Errors
    ///
    /// Returns [`HwpxError::Zip`] if the ZIP writer fails at any stage.
    pub fn write_hwpx(
        header_xml: &str,
        section_xmls: &[String],
        images: &[(String, Vec<u8>)],
    ) -> HwpxResult<Vec<u8>> {
        let buf: Vec<u8> = Vec::new();
        let cursor = Cursor::new(buf);
        let mut zip = ZipWriter::new(cursor);

        let stored_opts =
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
        let deflate_opts =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);

        // 1. mimetype — STORED, must be first entry (OPF convention)
        zip.start_file("mimetype", stored_opts).map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(MIMETYPE).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 2. version.xml
        zip.start_file("version.xml", deflate_opts).map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(VERSION_XML.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 3. META-INF/container.xml
        zip.start_file("META-INF/container.xml", deflate_opts)
            .map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(CONTAINER_XML.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 4. META-INF/manifest.xml
        zip.start_file("META-INF/manifest.xml", deflate_opts)
            .map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(MANIFEST_XML.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 5. Contents/content.hpf (dynamic manifest)
        zip.start_file("Contents/content.hpf", deflate_opts)
            .map_err(|e| HwpxError::Zip(e.to_string()))?;
        let image_paths: Vec<String> = images.iter().map(|(path, _)| path.clone()).collect();
        let content_hpf = generate_content_hpf(section_xmls.len(), &image_paths);
        zip.write_all(content_hpf.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 6. settings.xml
        zip.start_file("settings.xml", deflate_opts).map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(SETTINGS_XML.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 7. Contents/header.xml
        zip.start_file("Contents/header.xml", deflate_opts)
            .map_err(|e| HwpxError::Zip(e.to_string()))?;
        zip.write_all(header_xml.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;

        // 8. Contents/section*.xml
        for (i, section_xml) in section_xmls.iter().enumerate() {
            zip.start_file(format!("Contents/section{i}.xml"), deflate_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(section_xml.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        // 9. BinData/* — images stored uncompressed (already compressed formats)
        for (path, data) in images {
            zip.start_file(format!("BinData/{path}"), stored_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(data).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        let cursor = zip.finish().map_err(|e| HwpxError::Zip(e.to_string()))?;
        Ok(cursor.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use zip::ZipArchive;

    /// Minimal header XML for testing.
    const MINIMAL_HEADER: &str =
        r#"<?xml version="1.0" encoding="UTF-8"?><head version="1.4" secCnt="1"></head>"#;

    /// Minimal section XML for testing.
    const MINIMAL_SECTION: &str = r#"<?xml version="1.0" encoding="UTF-8"?><sec></sec>"#;

    /// Helper: write a minimal HWPX and return the raw bytes.
    fn write_minimal(sections: &[String]) -> Vec<u8> {
        PackageWriter::write_hwpx(MINIMAL_HEADER, sections, &[]).unwrap()
    }

    /// Helper: open a ZipArchive from raw bytes.
    fn open_zip(bytes: &[u8]) -> ZipArchive<Cursor<&[u8]>> {
        ZipArchive::new(Cursor::new(bytes)).unwrap()
    }

    // ── Test 1: mimetype is first stored entry ───────────────────

    #[test]
    fn mimetype_is_first_stored_entry() {
        let sections = vec![MINIMAL_SECTION.to_string()];
        let bytes = write_minimal(&sections);
        let mut archive = open_zip(&bytes);

        // First entry by index must be "mimetype"
        let entry = archive.by_index(0).unwrap();
        assert_eq!(entry.name(), "mimetype");
        assert_eq!(
            entry.compression(),
            CompressionMethod::Stored,
            "mimetype must be STORED, not DEFLATED"
        );
    }

    // ── Test 2: all required files exist ─────────────────────────

    #[test]
    fn all_required_files_exist_in_zip() {
        let sections = vec![MINIMAL_SECTION.to_string()];
        let bytes = write_minimal(&sections);
        let archive = open_zip(&bytes);

        let names: Vec<&str> = archive.file_names().collect();
        let required = [
            "mimetype",
            "version.xml",
            "META-INF/container.xml",
            "META-INF/manifest.xml",
            "Contents/content.hpf",
            "settings.xml",
            "Contents/header.xml",
            "Contents/section0.xml",
        ];
        for path in &required {
            assert!(names.contains(path), "missing required entry: {path}");
        }
    }

    // ── Test 3: version.xml preserves tagetApplication typo ──────

    #[test]
    fn version_xml_has_taget_typo() {
        assert!(
            VERSION_XML.contains("tagetApplication"),
            "must preserve intentional typo 'tagetApplication'"
        );
        assert!(!VERSION_XML.contains("targetApplication"), "must NOT contain corrected spelling");
    }

    // ── Test 4: content.hpf lists all sections ───────────────────

    #[test]
    fn content_hpf_lists_all_sections() {
        // Single section
        let hpf1 = generate_content_hpf(1, &[]);
        assert!(hpf1.contains(r#"id="section0""#));
        assert!(hpf1.contains(r#"idref="section0""#));
        assert!(!hpf1.contains(r#"id="section1""#));

        // Three sections
        let hpf3 = generate_content_hpf(3, &[]);
        for i in 0..3 {
            assert!(hpf3.contains(&format!(r#"id="section{i}""#)), "manifest missing section{i}");
            assert!(hpf3.contains(&format!(r#"idref="section{i}""#)), "spine missing section{i}");
        }
        assert!(!hpf3.contains(r#"id="section3""#));
    }

    // ── Test: content.hpf includes image entries ────────────────

    #[test]
    fn content_hpf_includes_images() {
        let images = vec!["photo.jpg".to_string(), "logo.png".to_string()];
        let hpf = generate_content_hpf(1, &images);
        // id must match binaryItemIDRef (filename stem, no extension)
        assert!(hpf.contains(r#"id="photo""#), "missing photo manifest entry");
        assert!(hpf.contains(r#"href="BinData/photo.jpg""#), "missing image href");
        assert!(hpf.contains(r#"media-type="image/jpeg""#), "missing jpeg media type");
        assert!(hpf.contains(r#"isEmbeded="1""#), "missing isEmbeded attribute");
        assert!(hpf.contains(r#"id="logo""#), "missing logo manifest entry");
        assert!(hpf.contains(r#"href="BinData/logo.png""#), "missing image href");
        assert!(hpf.contains(r#"media-type="image/png""#), "missing png media type");
        // Images should NOT be in spine
        assert!(!hpf.contains(r#"idref="image0""#), "images should not be in spine");
    }

    // ── Test 5: empty header XML succeeds ────────────────────────

    #[test]
    fn write_empty_header_succeeds() {
        let result = PackageWriter::write_hwpx("", &[MINIMAL_SECTION.to_string()], &[]);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        let archive = open_zip(&bytes);
        assert!(archive.file_names().any(|n| n == "Contents/header.xml"));
    }

    // ── Test 6: multi-section creates multiple entries ───────────

    #[test]
    fn multi_section_creates_multiple_entries() {
        let sections: Vec<String> = (0..3).map(|i| format!(r#"<sec>section{i}</sec>"#)).collect();
        let bytes = PackageWriter::write_hwpx(MINIMAL_HEADER, &sections, &[]).unwrap();
        let mut archive = open_zip(&bytes);

        for i in 0..3 {
            let path = format!("Contents/section{i}.xml");
            let mut entry = archive.by_name(&path).unwrap();
            let mut content = String::new();
            entry.read_to_string(&mut content).unwrap();
            assert!(content.contains(&format!("section{i}")), "section{i} content mismatch");
        }
    }

    // ── Test 7: generated ZIP is decodable by PackageReader ──────

    #[test]
    fn generated_zip_is_decodable() {
        use crate::decoder::package::PackageReader;

        let sections = vec![MINIMAL_SECTION.to_string()];
        let bytes = write_minimal(&sections);

        let mut reader = PackageReader::new(&bytes).unwrap();
        assert_eq!(reader.section_count(), 1);

        let header = reader.read_header_xml().unwrap();
        assert_eq!(header, MINIMAL_HEADER);

        let section = reader.read_section_xml(0).unwrap();
        assert_eq!(section, MINIMAL_SECTION);
    }

    // ── Test 8: zero sections succeeds ───────────────────────────

    #[test]
    fn write_zero_sections_succeeds() {
        let result = PackageWriter::write_hwpx(MINIMAL_HEADER, &[], &[]);
        assert!(result.is_ok());
        let bytes = result.unwrap();
        let archive = open_zip(&bytes);
        let names: Vec<&str> = archive.file_names().collect();
        // No section entries
        assert!(!names.iter().any(|n| n.starts_with("Contents/section")));
    }

    // ── Test 9: large section count ──────────────────────────────

    #[test]
    fn large_section_count() {
        let sections: Vec<String> = (0..100).map(|i| format!(r#"<sec>s{i}</sec>"#)).collect();
        let bytes = PackageWriter::write_hwpx(MINIMAL_HEADER, &sections, &[]).unwrap();
        let archive = open_zip(&bytes);

        let section_entries = archive
            .file_names()
            .filter(|n| n.starts_with("Contents/section") && n.ends_with(".xml"))
            .count();
        assert_eq!(section_entries, 100);
    }

    // ── Test 10: XMLNS_DECLS has all 15 namespaces ───────────────

    #[test]
    fn xmlns_decls_has_all_15_namespaces() {
        let expected = [
            r#"xmlns:ha="#,
            r#"xmlns:hp="#,
            r#"xmlns:hp10="#,
            r#"xmlns:hs="#,
            r#"xmlns:hc="#,
            r#"xmlns:hh="#,
            r#"xmlns:hhs="#,
            r#"xmlns:hm="#,
            r#"xmlns:hpf="#,
            r#"xmlns:dc="#,
            r#"xmlns:opf="#,
            r#"xmlns:ooxmlchart="#,
            r#"xmlns:hwpunitchar="#,
            r#"xmlns:epub="#,
            r#"xmlns:config="#,
        ];
        for ns in &expected {
            assert!(XMLNS_DECLS.contains(ns), "missing namespace declaration: {ns}");
        }
    }

    // ── Test 11 (bonus): images written to BinData/ ──────────────

    #[test]
    fn images_written_to_bindata() {
        let image_data = vec![0xFFu8, 0xD8, 0xFF, 0xE0]; // fake JPEG header
        let images = vec![("photo.jpg".to_string(), image_data.clone())];
        let bytes =
            PackageWriter::write_hwpx(MINIMAL_HEADER, &[MINIMAL_SECTION.to_string()], &images)
                .unwrap();

        let mut archive = open_zip(&bytes);
        let mut entry = archive.by_name("BinData/photo.jpg").unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Stored, "images should be STORED");
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, image_data);
    }
}
