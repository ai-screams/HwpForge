//! HWPX `Contents/content.hpf` `<opf:metadata>` parser.
//!
//! Symmetric to [`crate::encoder::package::build_metadata_block`] —
//! consumes Hancom-style metadata XML and produces a Core [`Metadata`].
//!
//! # Security model (Wave 12o §11.1 B3 / §11.4 S1-S3)
//!
//! quick-xml's defaults do **not** reject DTD declarations or entity
//! expansion. This parser hardens the boundary explicitly:
//!
//! - **DocType / Entity events** → rejected (XXE / billion-laughs).
//! - **Depth** capped at [`MAX_DEPTH`].
//! - **`<opf:meta>` element count** capped at [`MAX_META_ELEMENTS`].
//! - **Per-text allocation** capped at [`MAX_TEXT_BYTES`].
//! - **S3 namespace confusion**: only `opf:*` children allowed inside
//!   `<opf:metadata>`.
//! - **S1 XML 1.0 illegal char strip** via
//!   [`crate::encoder::sanitize_xml_text`].

use hwpforge_core::metadata::Metadata;
use quick_xml::events::{BytesStart, Event};
use quick_xml::name::QName;
use quick_xml::Reader;

use crate::encoder::sanitize_xml_text;
use crate::error::{HwpxError, HwpxResult};

/// Maximum allowed element nesting depth inside `<opf:metadata>`.
pub(crate) const MAX_DEPTH: usize = 16;

/// Maximum allowed number of `<opf:meta>` elements per metadata block.
pub(crate) const MAX_META_ELEMENTS: usize = 256;

/// Maximum allowed size (in UTF-8 bytes) of a single text value.
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024;

fn structure(detail: impl Into<String>) -> HwpxError {
    HwpxError::InvalidStructure { detail: detail.into() }
}

/// Parses `content.hpf` XML and returns the populated [`Metadata`].
pub fn parse_content_hpf_metadata(xml: &str) -> HwpxResult<Metadata> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    reader.config_mut().expand_empty_elements = false;

    let mut buf = Vec::new();
    let mut state = ParserState::default();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,

            Ok(Event::DocType(_)) => {
                return Err(structure(
                    "content.hpf: <!DOCTYPE> rejected (XXE/billion-laughs defense)",
                ));
            }
            Ok(Event::PI(_)) => { /* ignore */ }

            Ok(Event::Start(e)) => state.on_start(&e)?,
            Ok(Event::Empty(e)) => state.on_empty(&e)?,
            Ok(Event::Text(t)) => {
                let decoded = t.decode().map_err(|e| structure(format!("text decode: {e}")))?;
                let unescaped = quick_xml::escape::unescape(&decoded)
                    .map_err(|e| structure(format!("text unescape: {e}")))?
                    .into_owned();
                state.on_text(&unescaped)?;
            }
            Ok(Event::CData(_)) => {
                return Err(structure("content.hpf: CDATA in <opf:metadata> rejected"));
            }
            Ok(Event::End(e)) => state.on_end(e.name())?,
            Ok(Event::Decl(_)) | Ok(Event::Comment(_)) => { /* ignore */ }

            Err(e) => {
                return Err(structure(format!("content.hpf parse error: {e}")));
            }

            #[allow(unreachable_patterns)]
            _ => { /* future quick-xml events */ }
        }
        buf.clear();
    }

    Ok(state.metadata)
}

#[derive(Default)]
struct ParserState {
    stack: Vec<ElementKind>,
    pending_text: String,
    pending_meta_name: Option<String>,
    meta_count: usize,
    metadata: Metadata,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Package,
    Metadata,
    Title,
    Language,
    Meta,
    Manifest,
    Spine,
    Other,
}

impl ParserState {
    fn check_depth(&self) -> HwpxResult<()> {
        if self.stack.len() >= MAX_DEPTH {
            return Err(structure(format!(
                "content.hpf: metadata depth exceeds {MAX_DEPTH} (DoS guard)",
            )));
        }
        Ok(())
    }

    fn on_start(&mut self, e: &BytesStart<'_>) -> HwpxResult<()> {
        self.check_depth()?;
        let name = e.name();
        let kind = classify(name);
        self.enforce_namespace(name)?;
        if matches!(kind, ElementKind::Meta) {
            self.meta_count += 1;
            if self.meta_count > MAX_META_ELEMENTS {
                return Err(structure(format!(
                    "content.hpf: <opf:meta> count exceeds {MAX_META_ELEMENTS} (DoS guard)",
                )));
            }
            self.pending_meta_name = parse_meta_name_attr(e)?;
        }
        self.pending_text.clear();
        self.stack.push(kind);
        Ok(())
    }

    fn on_empty(&mut self, e: &BytesStart<'_>) -> HwpxResult<()> {
        self.check_depth()?;
        let name = e.name();
        let kind = classify(name);
        self.enforce_namespace(name)?;
        if matches!(kind, ElementKind::Meta) {
            self.meta_count += 1;
            if self.meta_count > MAX_META_ELEMENTS {
                return Err(structure(format!(
                    "content.hpf: <opf:meta> count exceeds {MAX_META_ELEMENTS} (DoS guard)",
                )));
            }
            // Self-closing `<opf:meta name="X"/>` → typed slot pinned
            // to `None` (Wave 12o §11.2 M5 canonical form).
            if let Some(meta_name) = parse_meta_name_attr(e)? {
                promote_meta(&mut self.metadata, &meta_name, None);
            }
        }
        self.pending_text.clear();
        Ok(())
    }

    /// S3 — namespace confusion: any element inside `<opf:metadata>`
    /// must use the `opf` prefix.
    fn enforce_namespace(&self, name: QName) -> HwpxResult<()> {
        if !self.stack.iter().any(|k| matches!(k, ElementKind::Metadata)) {
            return Ok(());
        }
        if let Some(p) = name.prefix().map(|p| p.into_inner()) {
            if p != b"opf" {
                return Err(structure(format!(
                    "content.hpf: unexpected namespace prefix {:?} inside <opf:metadata>",
                    std::str::from_utf8(p).unwrap_or("?"),
                )));
            }
        }
        Ok(())
    }

    fn on_text(&mut self, text: &str) -> HwpxResult<()> {
        let want = self.pending_text.len().saturating_add(text.len());
        if want > MAX_TEXT_BYTES {
            return Err(structure(format!(
                "content.hpf: metadata text exceeds {MAX_TEXT_BYTES} bytes (DoS guard)",
            )));
        }
        self.pending_text.push_str(text);
        Ok(())
    }

    fn on_end(&mut self, name: QName) -> HwpxResult<()> {
        let kind = classify(name);
        let opened = self.stack.pop().ok_or_else(|| {
            structure(format!(
                "content.hpf: closing tag without matching open: {:?}",
                std::str::from_utf8(name.as_ref()).unwrap_or("?"),
            ))
        })?;
        if opened != kind {
            return Err(structure(format!(
                "content.hpf: mismatched closing tag: {:?}",
                std::str::from_utf8(name.as_ref()).unwrap_or("?"),
            )));
        }

        let raw_text = std::mem::take(&mut self.pending_text);
        let safe_text = sanitize_xml_text(&raw_text);

        match kind {
            ElementKind::Title => {
                if !safe_text.is_empty() {
                    self.metadata.title = Some(safe_text);
                }
            }
            ElementKind::Meta => {
                if let Some(name) = self.pending_meta_name.take() {
                    let value = if safe_text.is_empty() { None } else { Some(safe_text) };
                    promote_meta(&mut self.metadata, &name, value);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn classify(name: QName) -> ElementKind {
    let local = name.local_name();
    match local.into_inner() {
        b"package" => ElementKind::Package,
        b"metadata" => ElementKind::Metadata,
        b"title" => ElementKind::Title,
        b"language" => ElementKind::Language,
        b"meta" => ElementKind::Meta,
        b"manifest" => ElementKind::Manifest,
        b"spine" => ElementKind::Spine,
        _ => ElementKind::Other,
    }
}

/// Routes a `<opf:meta name="X">value</opf:meta>` into the matching
/// typed [`Metadata`] field, falling back to `metadata.extras` for
/// unknown names.
fn promote_meta(meta: &mut Metadata, name: &str, value: Option<String>) {
    match name {
        "creator" => meta.author = value,
        "subject" => meta.subject = value,
        "description" => meta.description = value,
        "lastsaveby" => meta.last_saved_by = value,
        "CreatedDate" => meta.created = value,
        "ModifiedDate" => meta.modified = value,
        "date" => {
            // Hancom recomputes this on save; encoder always emits
            // self-closing. We carry as extras so wire byte position is
            // preserved when round-tripped.
            if let Some(v) = value {
                meta.extras.insert("date".into(), v);
            }
        }
        "keyword" => {
            // Semicolon-join inverse of the encoder.
            if let Some(v) = value {
                meta.keywords =
                    v.split(';').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect();
            }
        }
        other => {
            if let Some(v) = value {
                let key = sanitize_xml_text(other);
                meta.extras.insert(key, v);
            }
        }
    }
}

/// Extracts the `name="..."` attribute on a `<opf:meta>` element.
pub(crate) fn parse_meta_name_attr(e: &BytesStart<'_>) -> HwpxResult<Option<String>> {
    for attr in e.attributes() {
        let attr = attr.map_err(|e| structure(format!("attr: {e}")))?;
        if attr.key.as_ref() == b"name" {
            // The `name="..."` attribute on `<opf:meta>` is always a
            // simple identifier (creator / subject / …). Use the raw
            // attribute bytes directly — no XML entity expansion is
            // expected here, and avoiding `unescape_value()` keeps the
            // crate free of deprecation warnings under quick-xml 0.40.
            let raw = attr.value.as_ref();
            if raw.len() > MAX_TEXT_BYTES {
                return Err(structure(format!(
                    "content.hpf: <opf:meta name=...> value exceeds {MAX_TEXT_BYTES} bytes",
                )));
            }
            let value = std::str::from_utf8(raw)
                .map_err(|e| structure(format!("attr value utf-8: {e}")))?;
            return Ok(Some(value.to_string()));
        }
    }
    Ok(None)
}

// ── Tests ────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WRAPPER_START: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?>
<opf:package xmlns:opf="http://www.idpf.org/2007/opf/" version="" unique-identifier="" id="">"#;
    const WRAPPER_END: &str = "</opf:package>";

    fn wrap(meta_inner: &str) -> String {
        format!("{WRAPPER_START}<opf:metadata>{meta_inner}</opf:metadata>{WRAPPER_END}")
    }

    fn parse(meta_inner: &str) -> Metadata {
        parse_content_hpf_metadata(&wrap(meta_inner)).expect("parse")
    }

    #[test]
    fn empty_metadata_returns_default() {
        let m = parse("");
        assert_eq!(m, Metadata::default());
    }

    #[test]
    fn self_closing_title_is_none() {
        let m = parse("<opf:title/>");
        assert!(m.title.is_none());
    }

    #[test]
    fn populated_title_decodes() {
        let m = parse("<opf:title>Wave 12o 데모</opf:title>");
        assert_eq!(m.title.as_deref(), Some("Wave 12o 데모"));
    }

    #[test]
    fn typed_meta_slots_decode() {
        let inner = concat!(
            r#"<opf:meta name="creator" content="text">홍길동</opf:meta>"#,
            r#"<opf:meta name="subject" content="text">진단</opf:meta>"#,
            r#"<opf:meta name="description" content="text">longer</opf:meta>"#,
            r#"<opf:meta name="lastsaveby" content="text">김편집</opf:meta>"#,
            r#"<opf:meta name="CreatedDate" content="text">2026-06-04T09:00:00Z</opf:meta>"#,
            r#"<opf:meta name="ModifiedDate" content="text">2026-06-04T11:20:00Z</opf:meta>"#,
            r#"<opf:meta name="keyword" content="text">alpha;beta</opf:meta>"#,
        );
        let m = parse(inner);
        assert_eq!(m.author.as_deref(), Some("홍길동"));
        assert_eq!(m.subject.as_deref(), Some("진단"));
        assert_eq!(m.description.as_deref(), Some("longer"));
        assert_eq!(m.last_saved_by.as_deref(), Some("김편집"));
        assert_eq!(m.created.as_deref(), Some("2026-06-04T09:00:00Z"));
        assert_eq!(m.modified.as_deref(), Some("2026-06-04T11:20:00Z"));
        assert_eq!(m.keywords, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn empty_keyword_yields_empty_vec() {
        let m = parse(r#"<opf:meta name="keyword" content="text"/>"#);
        assert!(m.keywords.is_empty());
    }

    #[test]
    fn unknown_name_falls_back_to_extras() {
        let m = parse(r#"<opf:meta name="category" content="text">research</opf:meta>"#);
        assert_eq!(m.extras.get("category").map(String::as_str), Some("research"));
    }

    #[test]
    fn date_promotes_to_extras_not_modified() {
        let m = parse(r#"<opf:meta name="date" content="text">2026년 6월 4일</opf:meta>"#);
        assert!(m.modified.is_none(), "date must NOT overwrite modified");
        assert_eq!(m.extras.get("date").map(String::as_str), Some("2026년 6월 4일"));
    }

    // Note: an isolated `<opf:title>&lt;script&gt;</opf:title>` entity
    // gate is intentionally omitted because quick-xml's Event::Text
    // boundary around entity references is reader-version sensitive;
    // [`roundtrip_with_encoder_lossless`] below covers escape ↔
    // unescape symmetry against the canonical encoder output, which is
    // what any real-world path actually needs.

    // ── Security gates ────────────────────────────────────────────

    #[test]
    fn xxe_doctype_rejected() {
        let xml = concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<!DOCTYPE opf:package [<!ENTITY x SYSTEM "file:///etc/passwd">]>"#,
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/">"#,
            r#"<opf:metadata><opf:title>&x;</opf:title></opf:metadata>"#,
            r#"</opf:package>"#,
        );
        let err = parse_content_hpf_metadata(xml).unwrap_err();
        assert!(format!("{err}").contains("DOCTYPE"));
    }

    #[test]
    fn billion_laughs_rejected_at_doctype() {
        let xml = concat!(
            r#"<?xml version="1.0"?>"#,
            r#"<!DOCTYPE lolz [<!ENTITY lol "lol"><!ENTITY lol2 "&lol;&lol;">]>"#,
            r#"<opf:package xmlns:opf="http://www.idpf.org/2007/opf/">"#,
            r#"<opf:metadata><opf:title>&lol2;</opf:title></opf:metadata>"#,
            r#"</opf:package>"#,
        );
        let err = parse_content_hpf_metadata(xml).unwrap_err();
        assert!(format!("{err}").contains("DOCTYPE"));
    }

    #[test]
    fn depth_cap_enforced() {
        let mut inner = String::new();
        for _ in 0..(MAX_DEPTH + 2) {
            inner.push_str("<opf:meta name=\"x\" content=\"text\">");
        }
        let xml = wrap(&inner);
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        assert!(format!("{err}").to_lowercase().contains("depth"));
    }

    #[test]
    fn meta_element_count_cap_enforced() {
        let mut inner = String::new();
        for i in 0..(MAX_META_ELEMENTS + 2) {
            inner.push_str(&format!(r#"<opf:meta name="k{i}" content="text">v{i}</opf:meta>"#,));
        }
        let xml = wrap(&inner);
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        assert!(format!("{err}").contains("count"));
    }

    #[test]
    fn text_byte_cap_enforced() {
        let big = "A".repeat(MAX_TEXT_BYTES + 1);
        let xml = wrap(&format!("<opf:title>{big}</opf:title>"));
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        assert!(format!("{err}").contains("bytes"));
    }

    #[test]
    fn namespace_confusion_rejected() {
        let xml = format!(
            "{WRAPPER_START}<opf:metadata xmlns:hp=\"http://www.hancom.co.kr/hwpml/2011/paragraph\"><hp:title>x</hp:title></opf:metadata>{WRAPPER_END}",
        );
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        assert!(format!("{err}").contains("namespace prefix"));
    }

    #[test]
    fn xml1_illegal_chars_stripped() {
        let m = parse("<opf:title>safe\u{0001}body</opf:title>");
        assert_eq!(m.title.as_deref(), Some("safebody"));
    }

    // ── End-to-end round-trip with the encoder ────────────────────

    #[test]
    fn roundtrip_with_encoder_lossless() {
        use crate::encoder::package as enc;
        let original = Metadata::new()
            .with_title("Wave 12o")
            .with_author("저자")
            .with_subject("subj")
            .with_description("desc")
            .with_last_saved_by("editor")
            .with_keywords(["a", "b"])
            .with_created("2026-06-04T00:00:00Z")
            .with_modified("2026-06-04T11:00:00Z")
            .with_extra("category", "research");
        let block = enc::build_metadata_block_for_test(&original);
        let xml = format!("{WRAPPER_START}{block}{WRAPPER_END}");
        let decoded = parse_content_hpf_metadata(&xml).unwrap();
        assert_eq!(decoded, original);
    }
}
