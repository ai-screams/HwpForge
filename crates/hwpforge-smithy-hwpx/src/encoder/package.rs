//! HWPX ZIP package writer.
//!
//! Creates the ZIP archive structure required by the HWPX format.
//! Mirrors the [`crate::decoder::package`] module for symmetry.

use std::borrow::Cow;
use std::io::{Cursor, Write};

use hwpforge_core::metadata::Metadata;
use zip::write::SimpleFileOptions;
use zip::CompressionMethod;
use zip::ZipWriter;

use crate::encoder::{escape_xml, EncodeWarning};
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
const VERSION_XML: &str = r##"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hv:HCFVersion xmlns:hv="http://www.hancom.co.kr/hwpml/2011/version" tagetApplication="WORDPROCESSOR" major="5" minor="1" micro="1" buildNumber="0" os="10" xmlVersion="1.5" application="Hancom Office Hangul" appVersion="12.0.0.0"/>"##;

/// META-INF/container.xml pointing to the content package file.
const CONTAINER_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ocf:container xmlns:ocf="urn:oasis:names:tc:opendocument:xmlns:container" xmlns:hpf="http://www.hancom.co.kr/schema/2011/hpf"><ocf:rootfiles><ocf:rootfile full-path="Contents/content.hpf" media-type="application/hwpml-package+xml"/></ocf:rootfiles></ocf:container>"#;

/// META-INF/manifest.xml (empty manifest).
const MANIFEST_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><odf:manifest xmlns:odf="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"/>"#;

/// Application settings with default caret position.
const SETTINGS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><ha:HWPApplicationSetting xmlns:ha="http://www.hancom.co.kr/hwpml/2011/app" xmlns:config="urn:oasis:names:tc:opendocument:xmlns:config:1.0"><ha:CaretPosition listIDRef="0" paraIDRef="0" pos="0"/></ha:HWPApplicationSetting>"#;

// ── content.hpf generator ────────────────────────────────────────

/// One `<opf:item>` of the `content.hpf` manifest.
///
/// [`manifest_items`] builds the whole list once and [`render_content_hpf`]
/// turns it into `content.hpf`. The encoder checks that rendered text — read
/// back with an XML reader, the way a consumer sees the ids — and
/// [`PackageWriter::write_hwpx_with_content_hpf`] writes the same text, so
/// the check and the written manifest cannot disagree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ManifestItem {
    /// `id` attribute — what `binaryItemIDRef` / `idref` name.
    pub id: String,
    /// `href` attribute — the ZIP entry the item points at.
    pub href: String,
    /// `media-type` attribute.
    pub media_type: &'static str,
    /// `isEmbeded` attribute (Hancom's spelling), absent for XML parts.
    pub is_embeded: Option<&'static str>,
    /// Whether the item is also listed in `<opf:spine>`.
    pub in_spine: bool,
}

impl ManifestItem {
    fn xml_part(id: String, href: String, in_spine: bool) -> Self {
        Self { id, href, media_type: "application/xml", is_embeded: None, in_spine }
    }
}

/// The manifest items for a package, in the order 한글 writes them: header,
/// sections, settings, images, master pages, embedded-chart OLE blobs.
///
/// Images and OLE blobs are not in the spine. Chart XML parts are not in the
/// manifest at all (한글 expects `Chart/*.xml` in the ZIP only).
pub(crate) fn manifest_items(
    section_count: usize,
    image_paths: &[String],
    masterpage_paths: &[String],
    embedded_ole_ids: &[String],
) -> Vec<ManifestItem> {
    let mut items =
        vec![ManifestItem::xml_part("header".to_string(), "Contents/header.xml".to_string(), true)];
    for i in 0..section_count {
        items.push(ManifestItem::xml_part(
            format!("section{i}"),
            format!("Contents/section{i}.xml"),
            true,
        ));
    }
    // settings in manifest (not in spine)
    items.push(ManifestItem::xml_part("settings".to_string(), "settings.xml".to_string(), false));

    // Image entries (not in spine). The `id` must match the
    // `binaryItemIDRef` in section XML (filename stem, no extension).
    // `isEmbeded="1"` (intentional typo matching 한글's output) marks the
    // binary as embedded.
    for path in image_paths {
        // Strip extension: "test_image.png" → "test_image"
        let stem = match path.rfind('.') {
            Some(pos) => &path[..pos],
            None => path.as_str(),
        };
        items.push(ManifestItem {
            id: stem.to_string(),
            href: format!("BinData/{path}"),
            media_type: guess_image_media_type(path),
            is_embeded: Some("1"),
            in_spine: false,
        });
    }

    // Master page entries in manifest and spine.
    for path in masterpage_paths {
        // path is like "Contents/masterpage0.xml" — extract id "masterpage0"
        let stem =
            path.strip_prefix("Contents/").unwrap_or(path).strip_suffix(".xml").unwrap_or(path);
        items.push(ManifestItem::xml_part(stem.to_string(), path.clone(), true));
    }

    // Embedded-chart OLE blobs (Wave 4c). 한글 expects each `BinData/{id}.ole`
    // to be listed with `media-type="application/ole"` and `isEmbeded="0"`
    // (truth output from Hancom uses the "0" form; the typo `isEmbeded`
    // matches Hancom's own attribute name).
    for ole_id in embedded_ole_ids {
        items.push(ManifestItem {
            id: ole_id.clone(),
            href: format!("BinData/{ole_id}.ole"),
            media_type: "application/ole",
            is_embeded: Some("0"),
            in_spine: false,
        });
    }
    items
}

/// Generates the OPF content manifest listing header, sections, and images.
///
/// Matches the structure produced by 한글: full namespace declarations,
/// metadata section, header + sections + settings + images in manifest,
/// and header + sections in spine (images are NOT in spine).
///
/// **Wave 12o**: The `metadata` argument populates the `<opf:metadata>`
/// block. All slots Hancom emits (title/language/creator/subject/
/// description/lastsaveby/CreatedDate/ModifiedDate/date/keyword) are
/// **always** emitted to preserve byte-parity with Hancom output —
/// `None` values self-close, populated values are XML-escaped (the
/// package writer's part scan then removes characters XML 1.0 forbids). Unknown entries in `metadata.extras` are emitted in
/// `BTreeMap` (alphabetical) order so output is deterministic.
#[cfg(test)]
fn generate_content_hpf(
    metadata: &Metadata,
    section_count: usize,
    image_paths: &[String],
    chart_paths: &[String],
    masterpage_paths: &[String],
    embedded_ole_ids: &[String],
) -> String {
    // NOTE: Chart XML files are NOT listed in the manifest.
    let _ = chart_paths;
    render_content_hpf(
        metadata,
        &manifest_items(section_count, image_paths, masterpage_paths, embedded_ole_ids),
    )
}

/// Writes `content.hpf` from metadata and an already built manifest list.
///
/// `id` and `href` are attribute-escaped: an image store key is caller data,
/// and a raw `&` or `"` in it would either break the part or turn into a
/// different id once parsed (`he&#97;der` would read back as `header`).
pub(crate) fn render_content_hpf(metadata: &Metadata, items: &[ManifestItem]) -> String {
    use std::fmt::Write as _;

    let mut manifest_items = String::new();
    let mut spine_refs = String::new();
    for item in items {
        let ManifestItem { id, href, media_type, is_embeded, in_spine } = item;
        let id = escape_xml(id);
        let href = escape_xml(href);
        write!(manifest_items, r#"<opf:item id="{id}" href="{href}" media-type="{media_type}""#)
            .expect("write to String is infallible");
        if let Some(flag) = is_embeded {
            write!(manifest_items, r#" isEmbeded="{flag}""#)
                .expect("write to String is infallible");
        }
        manifest_items.push_str("/>");
        if *in_spine {
            write!(spine_refs, r#"<opf:itemref idref="{id}" linear="yes"/>"#)
                .expect("write to String is infallible");
        }
    }

    let metadata_block = build_metadata_block(metadata);

    format!(
        concat!(
            r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>"#,
            r#"<opf:package{xmlns} version="" unique-identifier="" id="">"#,
            r#"{metadata_block}"#,
            r#"<opf:manifest>{manifest_items}</opf:manifest>"#,
            r#"<opf:spine>{spine_refs}</opf:spine>"#,
            r#"</opf:package>"#,
        ),
        xmlns = XMLNS_DECLS,
        metadata_block = metadata_block,
        manifest_items = manifest_items,
        spine_refs = spine_refs,
    )
}

/// Builds the `<opf:metadata>` block matching Hancom byte-parity.
///
/// Slot order is fixed (title, language, creator, subject, description,
/// lastsaveby, CreatedDate, ModifiedDate, date, keyword, extras). `None`
/// typed slots self-close (`<opf:title/>` and
/// `<opf:meta name="..." content="text"/>`). Populated slots use
/// `<opf:title>value</opf:title>` / `<opf:meta name="..."
/// content="text">value</opf:meta>` form. The `date` slot is intentionally
/// left empty so Hancom recomputes the human-readable Korean locale string
/// on save (Wave 12o §11.3 Q2). Keywords join with `";"` (Q3).
fn build_metadata_block(meta: &Metadata) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(384);
    out.push_str("<opf:metadata>");

    // <opf:title>
    match &meta.title {
        Some(t) => {
            let _ = write!(out, "<opf:title>{}</opf:title>", escape_xml(t));
        }
        None => out.push_str("<opf:title/>"),
    }

    out.push_str("<opf:language>ko</opf:language>");

    push_meta(&mut out, "creator", meta.author.as_deref());
    push_meta(&mut out, "subject", meta.subject.as_deref());
    push_meta(&mut out, "description", meta.description.as_deref());
    push_meta(&mut out, "lastsaveby", meta.last_saved_by.as_deref());
    push_meta(&mut out, "CreatedDate", meta.created.as_deref());
    push_meta(&mut out, "ModifiedDate", meta.modified.as_deref());

    // date — Hancom recomputes on save for most flows, but if a decoder
    //         (HWPX → HWPX round-trip) parked a non-empty value in
    //         extras["date"], honor it at the canonical 9-slot position
    //         rather than dropping it through the typed-collision guard
    //         below (Codex(architect) Wave 12o-fixup §Top-2, Option A
    //         수정형). `creator`/`subject`/... stay reserved for the
    //         typed `Metadata` fields.
    push_meta(&mut out, "date", meta.extras.get("date").map(String::as_str));

    // keyword — semicolon-joined into a single element.
    if meta.keywords.is_empty() {
        out.push_str(r#"<opf:meta name="keyword" content="text"/>"#);
    } else {
        let joined = meta.keywords.join(";");
        let _ = write!(
            out,
            r#"<opf:meta name="keyword" content="text">{}</opf:meta>"#,
            escape_xml(&joined),
        );
    }

    // extras — BTreeMap ordering (alphabetical). Slot guard: reject any
    // key that collides with a typed slot (defense in depth — decoder
    // promotes known names to typed fields).
    //
    // Codex(architect) Wave 12o-fixup §Top-2: `date` is intentionally
    // NOT in the reserved list — extras["date"] is the canonical
    // carry slot for the Hancom display-date string and is already
    // emitted at the fixed 9-slot position above. Including it here
    // would cause a duplicate `<opf:meta name="date">` element and
    // break 9-slot byte parity.
    for (k, v) in &meta.extras {
        if matches!(
            k.as_str(),
            "creator"
                | "subject"
                | "description"
                | "lastsaveby"
                | "CreatedDate"
                | "ModifiedDate"
                | "keyword"
        ) {
            continue;
        }
        if k == "date" {
            continue;
        }
        let safe_key = escape_xml(k);
        let safe_val = escape_xml(v);
        if v.is_empty() {
            let _ = write!(out, r#"<opf:meta name="{safe_key}" content="text"/>"#);
        } else {
            let _ =
                write!(out, r#"<opf:meta name="{safe_key}" content="text">{safe_val}</opf:meta>"#,);
        }
    }

    out.push_str("</opf:metadata>");
    out
}

/// Test hook: exposes [`build_metadata_block`] across module boundaries
/// so the decoder round-trip gates can exercise the canonical encoder
/// output without duplicating the format string.
#[cfg(test)]
pub(crate) fn build_metadata_block_for_test(meta: &Metadata) -> String {
    build_metadata_block(meta)
}

fn push_meta(out: &mut String, name: &'static str, value: Option<&str>) {
    use std::fmt::Write as _;
    match value {
        Some(v) => {
            let _ = write!(
                out,
                r#"<opf:meta name="{name}" content="text">{}</opf:meta>"#,
                escape_xml(v),
            );
        }
        None => {
            let _ = write!(out, r#"<opf:meta name="{name}" content="text"/>"#);
        }
    }
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

/// The ZIP entry an image stored under `path` is written to.
pub(crate) fn image_entry_name(path: &str) -> String {
    format!("BinData/{}", super::sanitize_zip_entry_name(path))
}

/// The ZIP entry an embedded-chart OLE blob `ole_id` is written to.
pub(crate) fn ole_entry_name(ole_id: &str) -> String {
    format!("BinData/{}.ole", super::sanitize_zip_entry_name(ole_id))
}

// ── PackageWriter ────────────────────────────────────────────────

/// Writes a valid HWPX ZIP archive.
///
/// Assembles header XML, section XMLs, and optional binary data (images)
/// into the standard HWPX ZIP structure. The resulting bytes can be
/// written directly to a `.hwpx` file.
pub(crate) struct PackageWriter;

impl PackageWriter {
    /// [`Self::write_hwpx_with_content_hpf`] with the manifest built from the
    /// same inputs (unit tests of the writer).
    #[cfg(test)]
    pub fn write_hwpx(
        metadata: &Metadata,
        header_xml: &str,
        section_xmls: &[String],
        images: &[(String, Vec<u8>)],
        charts: &[(String, String)],
        master_pages: &[(String, String)],
        embedded_oles: &[(String, Vec<u8>)],
    ) -> HwpxResult<(Vec<u8>, Vec<EncodeWarning>)> {
        let manifest = manifest_items(
            section_xmls.len(),
            &images.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
            &master_pages.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
            &embedded_oles.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        );
        Self::write_hwpx_with_content_hpf(
            &render_content_hpf(metadata, &manifest),
            header_xml,
            section_xmls,
            images,
            charts,
            master_pages,
            embedded_oles,
        )
    }

    /// Assembles header XML, section XMLs, and optional images into a HWPX ZIP.
    ///
    /// # Arguments
    ///
    /// * `content_hpf` — `Contents/content.hpf`, from [`render_content_hpf`]
    ///   over [`manifest_items`] of the same parts, images and OLE blobs.
    /// * `header_xml` — Serialized `Contents/header.xml` content.
    /// * `section_xmls` — Serialized section XML strings (`Contents/section0.xml`, etc.).
    /// * `images` — Pairs of `(filename, data)` written under `BinData/`.
    ///
    /// The charts are scanned here for characters XML 1.0 forbids, which are
    /// removed and returned as
    /// [`EncodeWarning::XmlForbiddenCharsRemoved`] (#198) — no earlier stage
    /// scans them. The header, section and master page parts arrive already
    /// scanned (each when it is assembled), and so does `content_hpf`: the
    /// encoder scans it before reading it back for the manifest checks, so
    /// the text checked is the text written.
    ///
    /// # Errors
    ///
    /// Returns [`HwpxError::Zip`] if the ZIP writer fails at any stage.
    pub fn write_hwpx_with_content_hpf(
        content_hpf: &str,
        header_xml: &str,
        section_xmls: &[String],
        images: &[(String, Vec<u8>)],
        charts: &[(String, String)],
        master_pages: &[(String, String)],
        embedded_oles: &[(String, Vec<u8>)],
    ) -> HwpxResult<(Vec<u8>, Vec<EncodeWarning>)> {
        let mut warnings = Vec::new();
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
            zip.start_file(image_entry_name(path), stored_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(data).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        // 10. Contents/masterpage*.xml — master page files (deflated)
        for (path, xml) in master_pages {
            zip.start_file(path.as_str(), deflate_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(xml.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        // 11. Chart/*.xml — OOXML chart files (deflated)
        for (path, xml) in charts {
            zip.start_file(path.as_str(), deflate_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            let xml = scan_part(path, xml, &mut warnings);
            zip.write_all(xml.as_bytes()).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        // 12. BinData/{ole_id}.ole — embedded-chart OLE blobs (Wave 4c).
        // Store uncompressed, matching how image binaries are written: the
        // payload is already a tightly-packed OLE2 compound file, and
        // 한글 reads it raw via `binaryItemIDRef` from content.hpf.
        for (ole_id, data) in embedded_oles {
            zip.start_file(ole_entry_name(ole_id), stored_opts)
                .map_err(|e| HwpxError::Zip(e.to_string()))?;
            zip.write_all(data).map_err(|e| HwpxError::Zip(e.to_string()))?;
        }

        let cursor = zip.finish().map_err(|e| HwpxError::Zip(e.to_string()))?;
        Ok((cursor.into_inner(), warnings))
    }
}

/// `xml` without the characters XML 1.0 forbids; each removal is added to
/// `warnings` under `part`.
fn scan_part<'a>(part: &str, xml: &'a str, warnings: &mut Vec<EncodeWarning>) -> Cow<'a, str> {
    let (clean, sites) = crate::wire_xml::strip_xml_forbidden_chars(xml);
    warnings.extend(super::forbidden_char_warnings(part, sites));
    clean
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
        PackageWriter::write_hwpx(
            &Metadata::default(),
            MINIMAL_HEADER,
            sections,
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap()
        .0
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
        let meta = Metadata::default();

        // Single section
        let hpf1 = generate_content_hpf(&meta, 1, &[], &[], &[], &[]);
        assert!(hpf1.contains(r#"id="section0""#));
        assert!(hpf1.contains(r#"idref="section0""#));
        assert!(!hpf1.contains(r#"id="section1""#));

        // Three sections
        let hpf3 = generate_content_hpf(&meta, 3, &[], &[], &[], &[]);
        for i in 0..3 {
            assert!(hpf3.contains(&format!(r#"id="section{i}""#)), "manifest missing section{i}");
            assert!(hpf3.contains(&format!(r#"idref="section{i}""#)), "spine missing section{i}");
        }
        assert!(!hpf3.contains(r#"id="section3""#));
    }

    /// The whole manifest and spine for every kind of item, as a literal.
    ///
    /// 이것을 실패시키는 것: manifest 항목의 순서(header·section·settings·
    /// 이미지·바탕쪽·OLE)나 한 항목의 속성(`isEmbeded` 값, spine 포함 여부)을
    /// 바꾸는 것.
    #[test]
    fn content_hpf_manifest_and_spine_for_every_item_kind() {
        let hpf = generate_content_hpf(
            &Metadata::default(),
            2,
            &["image1.png".to_string(), "photo.jpg".to_string()],
            &["Chart/chart0.xml".to_string()],
            &["Contents/masterpage0.xml".to_string()],
            &["ole1".to_string()],
        );
        let manifest = hpf.split("<opf:manifest>").nth(1).unwrap();
        assert_eq!(
            manifest,
            concat!(
                r#"<opf:item id="header" href="Contents/header.xml" media-type="application/xml"/>"#,
                r#"<opf:item id="section0" href="Contents/section0.xml" media-type="application/xml"/>"#,
                r#"<opf:item id="section1" href="Contents/section1.xml" media-type="application/xml"/>"#,
                r#"<opf:item id="settings" href="settings.xml" media-type="application/xml"/>"#,
                r#"<opf:item id="image1" href="BinData/image1.png" media-type="image/png" isEmbeded="1"/>"#,
                r#"<opf:item id="photo" href="BinData/photo.jpg" media-type="image/jpeg" isEmbeded="1"/>"#,
                r#"<opf:item id="masterpage0" href="Contents/masterpage0.xml" media-type="application/xml"/>"#,
                r#"<opf:item id="ole1" href="BinData/ole1.ole" media-type="application/ole" isEmbeded="0"/>"#,
                r#"</opf:manifest>"#,
                r#"<opf:spine>"#,
                r#"<opf:itemref idref="header" linear="yes"/>"#,
                r#"<opf:itemref idref="section0" linear="yes"/>"#,
                r#"<opf:itemref idref="section1" linear="yes"/>"#,
                r#"<opf:itemref idref="masterpage0" linear="yes"/>"#,
                r#"</opf:spine></opf:package>"#,
            )
        );
    }

    // ── Test: content.hpf includes image entries ────────────────

    #[test]
    fn content_hpf_includes_images() {
        let images = vec!["photo.jpg".to_string(), "logo.png".to_string()];
        let hpf = generate_content_hpf(&Metadata::default(), 1, &images, &[], &[], &[]);
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
        let result = PackageWriter::write_hwpx(
            &Metadata::default(),
            "",
            &[MINIMAL_SECTION.to_string()],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(result.is_ok());
        let (bytes, _) = result.unwrap();
        let archive = open_zip(&bytes);
        assert!(archive.file_names().any(|n| n == "Contents/header.xml"));
    }

    // ── Test 6: multi-section creates multiple entries ───────────

    #[test]
    fn multi_section_creates_multiple_entries() {
        let sections: Vec<String> = (0..3).map(|i| format!(r#"<sec>section{i}</sec>"#)).collect();
        let bytes = PackageWriter::write_hwpx(
            &Metadata::default(),
            MINIMAL_HEADER,
            &sections,
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap()
        .0;
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
        let result = PackageWriter::write_hwpx(
            &Metadata::default(),
            MINIMAL_HEADER,
            &[],
            &[],
            &[],
            &[],
            &[],
        );
        assert!(result.is_ok());
        let (bytes, _) = result.unwrap();
        let archive = open_zip(&bytes);
        let names: Vec<&str> = archive.file_names().collect();
        // No section entries
        assert!(!names.iter().any(|n| n.starts_with("Contents/section")));
    }

    // ── Test 9: large section count ──────────────────────────────

    #[test]
    fn large_section_count() {
        let sections: Vec<String> = (0..100).map(|i| format!(r#"<sec>s{i}</sec>"#)).collect();
        let bytes = PackageWriter::write_hwpx(
            &Metadata::default(),
            MINIMAL_HEADER,
            &sections,
            &[],
            &[],
            &[],
            &[],
        )
        .unwrap()
        .0;
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
        let bytes = PackageWriter::write_hwpx(
            &Metadata::default(),
            MINIMAL_HEADER,
            &[MINIMAL_SECTION.to_string()],
            &images,
            &[],
            &[],
            &[],
        )
        .unwrap()
        .0;

        let mut archive = open_zip(&bytes);
        let mut entry = archive.by_name("BinData/photo.jpg").unwrap();
        assert_eq!(entry.compression(), CompressionMethod::Stored, "images should be STORED");
        let mut buf = Vec::new();
        entry.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, image_data);
    }

    // ── Wave 12o Phase 1 — metadata emit gates ──────────────────

    /// Default Metadata: every slot self-closes; Hancom byte parity rules
    /// (always 9 slots + always `content="text"` on `<opf:meta>`).
    #[test]
    fn metadata_default_emits_all_self_closing_slots() {
        let hpf = generate_content_hpf(&Metadata::default(), 1, &[], &[], &[], &[]);
        assert!(hpf.contains("<opf:title/>"), "title must self-close when None");
        assert!(hpf.contains("<opf:language>ko</opf:language>"));
        for name in [
            "creator",
            "subject",
            "description",
            "lastsaveby",
            "CreatedDate",
            "ModifiedDate",
            "date",
            "keyword",
        ] {
            let expected = format!(r#"<opf:meta name="{name}" content="text"/>"#);
            assert!(
                hpf.contains(&expected),
                "missing self-closing slot for {name}: looked for {expected}"
            );
        }
    }

    /// Populated Metadata: every typed field surfaces in the matching slot
    /// with content="text" intact.
    #[test]
    fn metadata_populated_emits_typed_values() {
        let meta = Metadata::new()
            .with_title("Wave 12o 데모")
            .with_author("홍길동")
            .with_subject("자동 필드 진단")
            .with_description("longer description")
            .with_last_saved_by("김편집")
            .with_keywords(["alpha", "beta"])
            .with_created("2026-06-04T09:00:00Z")
            .with_modified("2026-06-04T11:20:00Z");
        let hpf = generate_content_hpf(&meta, 1, &[], &[], &[], &[]);
        assert!(hpf.contains("<opf:title>Wave 12o 데모</opf:title>"));
        assert!(hpf.contains(r#"<opf:meta name="creator" content="text">홍길동</opf:meta>"#));
        assert!(
            hpf.contains(r#"<opf:meta name="subject" content="text">자동 필드 진단</opf:meta>"#)
        );
        assert!(hpf.contains(
            r#"<opf:meta name="description" content="text">longer description</opf:meta>"#
        ));
        assert!(hpf.contains(r#"<opf:meta name="lastsaveby" content="text">김편집</opf:meta>"#));
        assert!(hpf.contains(
            r#"<opf:meta name="CreatedDate" content="text">2026-06-04T09:00:00Z</opf:meta>"#
        ));
        assert!(hpf.contains(
            r#"<opf:meta name="ModifiedDate" content="text">2026-06-04T11:20:00Z</opf:meta>"#
        ));
        // date is always self-closing (Hancom recomputes on save).
        assert!(hpf.contains(r#"<opf:meta name="date" content="text"/>"#));
        // keywords semicolon-joined into a single element (Q3 architect ans).
        assert!(hpf.contains(r#"<opf:meta name="keyword" content="text">alpha;beta</opf:meta>"#));
    }

    /// `extras` entries emit in BTreeMap (alphabetical) order so encoder
    /// output is byte-stable for round-trip diffs. Typed-slot collisions
    /// (e.g. extras key "creator") are dropped — typed field is authoritative.
    #[test]
    fn metadata_extras_emit_alphabetical_and_skip_typed_collisions() {
        let meta = Metadata::new()
            .with_extra("zeta", "z")
            .with_extra("alpha", "a")
            .with_extra("creator", "should-not-overwrite-typed"); // typed-slot collision
        let hpf = generate_content_hpf(&meta, 1, &[], &[], &[], &[]);
        // creator extra is dropped — typed `author` (None) controls the slot.
        assert!(hpf.contains(r#"<opf:meta name="creator" content="text"/>"#));
        assert!(!hpf.contains("should-not-overwrite-typed"));
        // alphabetical: alpha before zeta
        let pos_alpha = hpf.find(r#"<opf:meta name="alpha""#).expect("alpha extra emitted");
        let pos_zeta = hpf.find(r#"<opf:meta name="zeta""#).expect("zeta extra emitted");
        assert!(pos_alpha < pos_zeta, "extras must emit in BTreeMap (alphabetical) order");
        assert!(hpf.contains(r#"<opf:meta name="alpha" content="text">a</opf:meta>"#));
        assert!(hpf.contains(r#"<opf:meta name="zeta" content="text">z</opf:meta>"#));
    }

    /// XML metacharacters in metadata values must be escaped, not raw.
    /// Prevents XML injection through user-controlled metadata.
    #[test]
    fn metadata_escapes_xml_metacharacters() {
        let meta = Metadata::new().with_title("<script>&amp;</script>");
        let hpf = generate_content_hpf(&meta, 1, &[], &[], &[], &[]);
        assert!(hpf.contains("&lt;script&gt;&amp;amp;&lt;/script&gt;"));
        // Raw `<script>` must NOT leak through.
        assert!(!hpf.contains("<script>"));
    }

    /// C0 control characters (other than tab/LF/CR) must not reach the
    /// written `content.hpf`: XML 1.0 rejects them entirely. The encoder
    /// scans `content.hpf` right after rendering it, removes them and reports
    /// where.
    /// 이것을 실패시키는 것: 인코더가 `render_content_hpf` 직후에 하는 `content.hpf` 스캔을 빼는 것.
    #[test]
    fn metadata_strips_xml1_illegal_chars() {
        use hwpforge_core::run::Run;
        use hwpforge_core::section::Section;
        use hwpforge_core::{Document, PageSettings, Paragraph};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

        let meta = Metadata::new().with_title("safe\u{0001}\u{0008}body\u{000B}more\u{FFFE}");
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("body", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        doc.set_metadata(meta);
        let mut styles = crate::style_store::HwpxStyleStore::with_default_fonts("함초롬돋움");
        styles.push_char_shape(crate::style_store::HwpxCharShape::default());
        styles.push_para_shape(crate::style_store::HwpxParaShape::default());
        let outcome = crate::HwpxEncoder::encode_with_diagnostics(
            &doc.validate().unwrap(),
            &styles,
            &hwpforge_core::image::ImageStore::new(),
            crate::EncodeOptions::default(),
        )
        .unwrap();
        let mut hpf = String::new();
        open_zip(&outcome.bytes)
            .by_name("Contents/content.hpf")
            .unwrap()
            .read_to_string(&mut hpf)
            .unwrap();
        // The control chars are gone, leaving the legitimate text.
        assert!(hpf.contains("<opf:title>safebodymore</opf:title>"), "{hpf}");
        let removed: Vec<_> = outcome
            .warnings
            .iter()
            .filter(|w| matches!(w, EncodeWarning::XmlForbiddenCharsRemoved { .. }))
            .collect();
        assert_eq!(
            removed,
            [&EncodeWarning::XmlForbiddenCharsRemoved {
                part: "Contents/content.hpf".to_string(),
                location: "opf:title".to_string(),
                count: 4,
            }]
        );
    }
}
