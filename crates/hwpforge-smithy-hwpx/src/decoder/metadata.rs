//! HWPX `Contents/content.hpf` `<opf:metadata>` parser.
//!
//! Symmetric to [`crate::encoder::package::build_metadata_block`] —
//! consumes Hancom-style metadata XML and produces a Core [`Metadata`].
//!
//! # Security model
//!
//! quick-xml's defaults do **not** reject DTD declarations or entity
//! expansion. This parser hardens the boundary explicitly:
//!
//! - **DocType** → rejected (XXE / billion-laughs).
//! - **Depth** capped at [`MAX_DEPTH`].
//! - **`<opf:meta>` element count** capped at [`MAX_META_ELEMENTS`].
//! - **Per-text allocation** capped at [`MAX_TEXT_BYTES`].
//! - **Namespace confusion**: only `opf:*` children allowed inside
//!   `<opf:metadata>`.
//! - **XML 1.0 illegal char strip** via
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

/// Parses metadata and reports discarded modeled values.
pub(crate) fn parse_content_hpf_metadata(xml: &str) -> HwpxResult<(Metadata, Vec<DroppedField>)> {
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
                state.on_text(&t);
            }
            Ok(Event::GeneralRef(reference)) => {
                match crate::wire_xml::resolve_general_ref(&reference) {
                    Ok(value) => state.on_text(&value),
                    Err(_) => {
                        if let Some(element) = state.stack.last_mut() {
                            element.invalid_reference = true;
                            element.text.clear();
                        }
                    }
                }
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

    Ok((state.metadata, state.dropped))
}

#[derive(Debug)]
pub(crate) struct DroppedField {
    pub field: String,
    pub reason: String,
}

#[derive(Default)]
struct ParserState {
    stack: Vec<Element>,
    dropped: Vec<DroppedField>,
    meta_count: usize,
    metadata: Metadata,
}

#[derive(Default)]
struct Element {
    kind: ElementKind,
    name: Option<String>,
    text: String,
    bytes: usize,
    name_bytes: usize,
    invalid_reference: bool,
}

impl Element {
    fn collects_text(&self) -> bool {
        self.kind == ElementKind::Title || (self.kind == ElementKind::Meta && self.name.is_some())
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum ElementKind {
    Package,
    Metadata,
    Title,
    Language,
    Meta,
    Manifest,
    Spine,
    #[default]
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
        let mut element = Element { kind, ..Element::default() };
        if kind == ElementKind::Meta {
            self.meta_count += 1;
            if self.meta_count > MAX_META_ELEMENTS {
                return Err(structure(format!(
                    "content.hpf: <opf:meta> count exceeds {MAX_META_ELEMENTS} (DoS guard)",
                )));
            }
            Self::parse_meta_name_attr(e, &mut element)?;
        }
        self.stack.push(element);
        Ok(())
    }

    fn on_empty(&mut self, e: &BytesStart<'_>) -> HwpxResult<()> {
        self.on_start(e)?;
        self.on_end(e.name())
    }

    /// Reads the name without copying oversized attributes; disposal happens at element end.
    fn parse_meta_name_attr(e: &BytesStart<'_>, element: &mut Element) -> HwpxResult<()> {
        for attr in e.attributes() {
            let attr = attr.map_err(|e| structure(format!("attr: {e}")))?;
            if attr.key.as_ref() == "name" {
                element.name_bytes = attr.value.len();
                if element.name_bytes <= MAX_TEXT_BYTES {
                    element.name = Some(attr.value.to_string());
                }
                break;
            }
        }
        Ok(())
    }

    /// The sole disposal gate for modeled values, regardless of the loss cause.
    fn discard_element(&mut self, element: &Element) -> bool {
        let field = match element.kind {
            ElementKind::Title => "title".to_string(),
            ElementKind::Meta if element.name_bytes > MAX_TEXT_BYTES => "meta@name".into(),
            ElementKind::Meta => match &element.name {
                Some(name) => format!(
                    "meta[name={}]",
                    sanitize_xml_text(name).chars().take(64).collect::<String>()
                ),
                None => return false,
            },
            _ => return false,
        };
        let bytes = element.bytes.max(element.name_bytes);
        let reason = if element.invalid_reference {
            "metadata value contains an unresolved reference".to_string()
        } else if bytes > MAX_TEXT_BYTES {
            format!("metadata value has {bytes} bytes, exceeds {MAX_TEXT_BYTES} bytes")
        } else {
            return false;
        };
        self.dropped.push(DroppedField { field, reason });
        true
    }

    /// namespace confusion: any element inside `<opf:metadata>`
    /// must use the `opf` prefix.
    ///
    /// The prior implementation used `if let Some(p) = name.prefix()` which only
    /// triggered when a prefix was present. Bare `<title>` / `<meta>`
    /// elements (no prefix) bypassed the gate entirely, so a hostile
    /// `content.hpf` could shadow the `<opf:title>` slot with an
    /// unprefixed sibling and still pass the typed-name router. Reject
    /// any non-`opf` prefix, including the absent-prefix case.
    fn enforce_namespace(&self, name: QName) -> HwpxResult<()> {
        if !self.stack.iter().any(|k| matches!(k.kind, ElementKind::Metadata)) {
            return Ok(());
        }
        match name.prefix().map(|p| p.into_inner()) {
            Some("opf") => Ok(()),
            Some(p) => Err(structure(format!(
                "content.hpf: unexpected namespace prefix {p:?} inside <opf:metadata>",
            ))),
            None => Err(structure(format!(
                "content.hpf: missing `opf:` prefix on {:?} inside <opf:metadata>",
                name.local_name().into_inner(),
            ))),
        }
    }

    fn on_text(&mut self, text: &str) {
        let Some(element) = self.stack.last_mut() else { return };
        element.bytes = element.bytes.saturating_add(text.len());
        if !element.collects_text()
            || element.bytes > MAX_TEXT_BYTES
            || element.invalid_reference
            || element.name_bytes > MAX_TEXT_BYTES
        {
            element.text.clear();
            return;
        }
        let need = element.text.len() + text.len();
        let cap = element.text.capacity();
        if need > cap {
            let target = cap.saturating_mul(2).max(need).min(MAX_TEXT_BYTES);
            element.text.reserve_exact(target - element.text.len());
        }
        element.text.push_str(text);
    }

    fn on_end(&mut self, name: QName) -> HwpxResult<()> {
        let kind = classify(name);
        let opened = self.stack.pop().ok_or_else(|| {
            structure(format!(
                "content.hpf: closing tag without matching open: {:?}",
                name.as_ref(),
            ))
        })?;
        if opened.kind != kind {
            return Err(structure(format!(
                "content.hpf: mismatched closing tag: {:?}",
                name.as_ref(),
            )));
        }

        if self.discard_element(&opened) || !opened.collects_text() {
            return Ok(());
        }
        let safe_text = sanitize_xml_text(&opened.text);

        match kind {
            ElementKind::Title => {
                if !safe_text.is_empty() {
                    self.metadata.title = Some(safe_text);
                }
            }
            ElementKind::Meta => {
                if let Some(name) = opened.name {
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
        "package" => ElementKind::Package,
        "metadata" => ElementKind::Metadata,
        "title" => ElementKind::Title,
        "language" => ElementKind::Language,
        "meta" => ElementKind::Meta,
        "manifest" => ElementKind::Manifest,
        "spine" => ElementKind::Spine,
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
        parse_content_hpf_metadata(&wrap(meta_inner)).expect("parse").0
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
        let m = parse("<opf:title>메타데이터 데모</opf:title>");
        assert_eq!(m.title.as_deref(), Some("메타데이터 데모"));
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

    // 이것을 실패시키는 것: 바이트 경계 >= 변경, GeneralRef 무시/빈 문자열, 필드명 보고 삭제.
    #[test]
    fn value_and_name_byte_boundaries() {
        for n in [MAX_TEXT_BYTES, MAX_TEXT_BYTES + 1] {
            for attribute in [false, true] {
                let big = "x".repeat(n);
                let extra = if attribute {
                    format!(r#"<opf:meta name="{big}">v</opf:meta>"#)
                } else {
                    format!(
                        r#"<opf:meta name="description">{}&#xAC00;</opf:meta>"#,
                        "x".repeat(n - 3)
                    )
                };
                let xml = wrap(&format!(
                    r#"<opf:title>A &amp; B &#xAC00; C</opf:title><opf:meta name="creator">A &amp; B &#xAC00; C</opf:meta>{extra}"#
                ));
                let (m, dropped) = parse_content_hpf_metadata(&xml).unwrap();
                assert_eq!(m.title.as_deref(), Some("A & B 가 C"));
                assert_eq!(m.author.as_deref(), Some("A & B 가 C"));
                assert_eq!(dropped.len(), usize::from(n > MAX_TEXT_BYTES));
                if n > MAX_TEXT_BYTES {
                    assert!(dropped[0].reason.contains(&n.to_string()));
                    assert_eq!(
                        dropped[0].field,
                        if attribute { "meta@name" } else { "meta[name=description]" }
                    );
                    assert!(m.description.is_none());
                    assert!(m.extras.is_empty());
                } else if attribute {
                    assert_eq!(m.extras.get(&big).unwrap(), "v");
                } else {
                    assert_eq!(m.description.unwrap().len(), n);
                }
            }
        }
    }

    // 이것을 실패시키는 것: 제목 폐기 누락 또는 모델링하지 않는 언어의 손실 보고.
    #[test]
    fn title_and_unmodeled_language_limits() {
        let (m, lost) = parse_content_hpf_metadata(&wrap(&format!(
            "<opf:title>{}</opf:title>",
            "x".repeat(MAX_TEXT_BYTES + 1)
        )))
        .unwrap();
        assert!(m.title.is_none());
        assert_eq!(lost[0].field, "title");
        let (_, lost) = parse_content_hpf_metadata(&wrap(&format!(
            r#"<opf:meta name="{}"/><opf:language>{}</opf:language>"#,
            "x".repeat(MAX_TEXT_BYTES + 1),
            "x".repeat(MAX_TEXT_BYTES + 1)
        )))
        .unwrap();
        assert_eq!(lost.len(), 1);
        assert_eq!(lost[0].field, "meta@name");
        let (m, lost) = parse_content_hpf_metadata(&wrap(&format!(
            r#"<opf:meta name="{}">{}&amp;tail</opf:meta><opf:meta name="creator">ok</opf:meta>"#,
            "x".repeat(MAX_TEXT_BYTES + 1),
            "x".repeat(MAX_TEXT_BYTES + 1)
        )))
        .unwrap();
        assert_eq!(lost.len(), 1);
        assert_eq!(m.author.as_deref(), Some("ok"));
    }

    // 이것을 실패시키는 것: 조각마다 정확 크기 할당, 상한 없는 배증, 넘친 뒤 수집, 할당 횟수 상한의 +1 제거.
    #[test]
    fn fragmented_buffer_growth_is_bounded() {
        let mut state = ParserState::default();
        state.on_start(&BytesStart::new("opf:title")).unwrap();
        let mut allocations = 0;
        for _ in 0..MAX_TEXT_BYTES {
            let before = state.stack[0].text.capacity();
            state.on_text("&");
            allocations += usize::from(before != state.stack[0].text.capacity());
            assert!(state.stack[0].text.capacity() <= MAX_TEXT_BYTES);
        }
        assert!(
            allocations <= MAX_TEXT_BYTES.ilog2() as usize + 1,
            "capacity changes: {allocations}"
        );
        let mut uneven = ParserState::default();
        uneven.on_start(&BytesStart::new("opf:title")).unwrap();
        uneven.on_text(&"x".repeat(MAX_TEXT_BYTES / 2 + 1));
        uneven.on_text(&"x".repeat(MAX_TEXT_BYTES / 2 - 1));
        assert_eq!(uneven.stack[0].text.len(), MAX_TEXT_BYTES);
        assert!(uneven.stack[0].text.capacity() <= MAX_TEXT_BYTES);
        state.on_text("&tail");
        assert!(state.stack[0].text.is_empty());
        assert_eq!(state.stack[0].bytes, MAX_TEXT_BYTES + "&tail".len());
    }

    // 이것을 실패시키는 것: 참조 오류의 전체 전파, 자식 시작 때 부모 상태 초기화,
    // 하나의 요소에 중복 손실 보고.
    #[test]
    fn element_loss_is_isolated_and_reported_once() {
        for bad in [
            "&nbsp;".to_string(),
            "&#0;".into(),
            "&#xD800;".into(),
            "&#x110000;".into(),
            "x".repeat(MAX_TEXT_BYTES + 1),
        ] {
            for child in [
                "",
                "<opf:x/>",
                "<opf:x>child</opf:x>",
                "<opf:meta name=\"subject\">kept</opf:meta>",
            ] {
                for tag in [
                    "opf:title".to_string(),
                    "opf:meta name=\"description\"".into(),
                    format!("opf:meta name=\"{}\"", "x".repeat(MAX_TEXT_BYTES + 1)),
                ] {
                    let close = tag.split(' ').next().unwrap();
                    let (m, lost) = parse_content_hpf_metadata(&wrap(&format!(
                        "<{tag}>{bad}{child}tail</{close}><opf:meta name=\"creator\">ok</opf:meta>"
                    )))
                    .unwrap();
                    assert!(m.title.is_none());
                    assert!(m.description.is_none());
                    assert_eq!(m.author.as_deref(), Some("ok"));
                    if child.contains("subject") {
                        assert_eq!(m.subject.as_deref(), Some("kept"));
                    }
                    assert_eq!(lost.len(), 1, "{tag}: {lost:?}");
                }
            }
        }
    }

    // 이것을 실패시키는 것: 경고 이름 정리 또는 길이 제한 삭제.
    #[test]
    fn warning_name_is_sanitized_and_truncated() {
        let name = format!("bad\u{1}{}", "가".repeat(100));
        let (_, lost) = parse_content_hpf_metadata(&wrap(&format!(
            "<opf:meta name=\"{name}\">&nbsp;</opf:meta>"
        )))
        .unwrap();
        assert_eq!(lost[0].field, format!("meta[name=bad{}]", "가".repeat(61)));
    }

    // 이것을 실패시키는 것: 모델링하지 않는 요소에 참조 손실 경고 추가.
    #[test]
    fn unmodeled_reference_loss_is_silent() {
        let (_, lost) = parse_content_hpf_metadata(&wrap(
            "<opf:language>&nbsp;</opf:language><opf:x>&nbsp;</opf:x>",
        ))
        .unwrap();
        assert!(lost.is_empty());
    }

    // 이것을 실패시키는 것: 저장하지 않는 요소의 텍스트 수집 재개.
    #[test]
    fn unmodeled_text_does_not_allocate() {
        for tag in ["opf:language", "opf:x", "opf:meta"] {
            let mut state = ParserState::default();
            state.on_start(&BytesStart::new(tag)).unwrap();
            state.on_text("ignored");
            assert_eq!(state.stack[0].bytes, 7);
            assert_eq!(state.stack[0].text.capacity(), 0);
            state.on_end(QName(tag)).unwrap();
            assert!(state.dropped.is_empty());
        }
    }

    #[test]
    fn namespace_confusion_rejected() {
        let xml = format!(
            "{WRAPPER_START}<opf:metadata xmlns:hp=\"http://www.hancom.co.kr/hwpml/2011/paragraph\"><hp:title>x</hp:title></opf:metadata>{WRAPPER_END}",
        );
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        assert!(format!("{err}").contains("namespace prefix"));
    }

    /// Unprefixed elements inside `<opf:metadata>` (e.g. `<title>` without the
    /// `opf:` prefix) used to bypass the typed-name router. Reject
    /// them so a hostile / malformed `content.hpf` cannot shadow the
    /// typed slots.
    #[test]
    fn unprefixed_element_inside_metadata_rejected() {
        let xml =
            format!("{WRAPPER_START}<opf:metadata><title>x</title></opf:metadata>{WRAPPER_END}");
        let err = parse_content_hpf_metadata(&xml).unwrap_err();
        let msg = format!("{err}");
        assert!(
            msg.contains("missing `opf:` prefix") || msg.contains("opf"),
            "expected unprefixed reject, got: {msg}",
        );
    }

    /// A Hancom-authored `content.hpf` that emits a populated `<opf:meta name="date">`
    /// must round-trip through the encoder without the value being
    /// dropped at the typed-collision guard. The encoder now emits
    /// `extras["date"]` at the canonical 9-slot position rather than
    /// (a) always self-closing and (b) silently dropping the carry.
    #[test]
    fn date_extras_roundtrip_preserves_value() {
        use crate::encoder::package as enc;
        let xml = format!(
            "{WRAPPER_START}<opf:metadata><opf:meta name=\"date\" content=\"text\">2026년 6월 4일 목요일 오후 4:47:38</opf:meta></opf:metadata>{WRAPPER_END}",
        );
        let decoded = parse_content_hpf_metadata(&xml).unwrap().0;
        assert_eq!(
            decoded.extras.get("date").map(String::as_str),
            Some("2026년 6월 4일 목요일 오후 4:47:38"),
        );
        let block = enc::build_metadata_block_for_test(&decoded);
        assert!(
            block.contains(
                r#"<opf:meta name="date" content="text">2026년 6월 4일 목요일 오후 4:47:38</opf:meta>"#
            ),
            "encoder must emit extras['date'] at canonical slot, got: {block}",
        );
        // And the second decode must observe the same value (no drift).
        let xml2 = format!("{WRAPPER_START}{block}{WRAPPER_END}");
        let decoded2 = parse_content_hpf_metadata(&xml2).unwrap().0;
        assert_eq!(decoded2.extras.get("date"), decoded.extras.get("date"));
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
            .with_title("메타데이터")
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
        let decoded = parse_content_hpf_metadata(&xml).unwrap().0;
        assert_eq!(decoded, original);
    }
}
