//! Shared helpers for inline HWPX text markup.
//!
//! HWPX uses mixed content inside `<hp:t>` for certain characters:
//! line breaks, tabs, and non-breaking spaces are encoded as child elements
//! instead of plain text. Core keeps text as a `String`, so the smithy layer
//! is responsible for translating between the plain-text view and the HWPX
//! mixed-content view.

use hwpforge_core::inline::{InlineSegment, InlineText};

use crate::encoder::escape_xml;

/// Checks that replacing this mixed-content slot can reproduce every child.
/// Prefix spelling and zero attributes are required, including namespace
/// declarations. The allow-list intentionally excludes all other events.
/// Returns the first rejected event's reason for the patch diagnostic.
pub(crate) fn validate_recreatable_inline_markup(
    t_qualified_name: &str,
    inner: &str,
) -> Result<(), String> {
    use quick_xml::{events::Event, Reader};
    let prefix = t_qualified_name.split_once(':').map(|(prefix, _)| prefix);
    let mut reader = Reader::from_str(inner);
    loop {
        match reader.read_event() {
            Ok(Event::Empty(element)) => {
                let name = element.name();
                let (local, child_prefix) = name.decompose();
                if child_prefix.map(|p| p.into_inner()) != prefix
                    || !matches!(local.as_ref(), "lineBreak" | "nbSpace" | "fwSpace" | "tab")
                {
                    return Err(name.as_ref().to_owned());
                }
                if element.attributes().next().is_some() {
                    return Err(format!("attribute on <{}>", name.as_ref()));
                }
            }
            Ok(Event::Text(_) | Event::GeneralRef(_) | Event::CData(_)) => {}
            Ok(Event::Eof) => return Ok(()),
            Ok(Event::Start(element)) => {
                return Err(format!("start tag <{}>", element.name().as_ref()));
            }
            Ok(Event::Comment(_)) => return Err("comment".into()),
            Ok(Event::PI(_)) => return Err("pi".into()),
            Ok(Event::Decl(_)) => return Err("decl".into()),
            Ok(Event::DocType(_)) => return Err("doctype".into()),
            _ => return Err("invalid XML".into()),
        }
    }
}

/// Returns `true` when a plain-text payload requires inline HWPX child
/// elements inside `<hp:t>`.
pub(crate) fn requires_inline_text_markup(text: &str) -> bool {
    text.chars().any(|ch| matches!(ch, '\r' | '\n' | '\t' | '\u{00A0}' | '\u{001F}'))
}

/// Encodes plain-text content into the inner XML for an `<hp:t>` element.
///
/// Body line breaks follow `wire_xml::body_text_line_breaks`; other
/// supported inline characters keep their existing mapping:
///
/// - `\n` → `<hp:lineBreak/>`
/// - `\t` → `<hp:tab/>`
/// - `U+00A0` → `<hp:nbSpace/>`
/// - `\r` / `\r\n` → one `<hp:lineBreak/>` within this fragment
/// - `U+001F` → `<hp:fwSpace/>` (mirrors the HWP5 wire control byte for the
///   "fixed-width space" so the round-trip through Core is lossless)
pub(crate) fn encode_inline_text_xml(text: &str) -> String {
    if text.is_empty() {
        return String::new();
    }

    let mut encoded = String::new();
    let mut plain = String::new();

    let flush_plain = |plain: &mut String, encoded: &mut String| {
        if plain.is_empty() {
            return;
        }
        encoded.push_str(&escape_xml(plain));
        plain.clear();
    };

    for ch in crate::wire_xml::body_text_line_breaks(text, true).chars() {
        match ch {
            '\n' => {
                flush_plain(&mut plain, &mut encoded);
                encoded.push_str("<hp:lineBreak/>");
            }
            '\t' => {
                flush_plain(&mut plain, &mut encoded);
                encoded.push_str("<hp:tab/>");
            }
            '\u{00A0}' => {
                flush_plain(&mut plain, &mut encoded);
                encoded.push_str("<hp:nbSpace/>");
            }
            '\u{001F}' => {
                flush_plain(&mut plain, &mut encoded);
                encoded.push_str("<hp:fwSpace/>");
            }
            _ => plain.push(ch),
        }
    }

    flush_plain(&mut plain, &mut encoded);
    encoded
}

/// Wraps plain-text content in an `<hp:t>` element, emitting mixed content
/// when necessary.
pub(crate) fn build_text_element_xml(text: &str) -> String {
    if text.is_empty() {
        "<hp:t/>".to_string()
    } else {
        format!("<hp:t>{}</hp:t>", encode_inline_text_xml(text))
    }
}

/// Wraps an [`InlineText`] in an `<hp:t>` element.
///
/// Differs from [`build_text_element_xml`] in that
/// [`InlineSegment::Tab`] segments emit
/// `<hp:tab width="..." leader="..." type="..."/>` with the raw HWP5
/// attribute integers preserved (Hancom uses raw numbers for inline
/// `<hp:tab>` even though the header-level `<hh:tabItem>` uses enum
/// strings). Plain text segments still go through the standard
/// character-sentinel encoding (`\n` / NBSP / fwSpace).
pub(crate) fn build_inline_text_element_xml(it: &InlineText) -> String {
    if it.segments.is_empty() {
        return "<hp:t/>".to_string();
    }
    let mut body = String::new();
    for seg in &it.segments {
        match seg {
            InlineSegment::Plain(s) => body.push_str(&encode_inline_text_xml(s)),
            InlineSegment::Tab(attr) => {
                body.push_str(&format!(
                    r#"<hp:tab width="{}" leader="{}" type="{}"/>"#,
                    attr.width.as_i32(),
                    attr.leader,
                    attr.tab_type,
                ));
            }
            // `InlineSegment` is `#[non_exhaustive]` — fall back to
            // attribute-less emission for any future variant a caller
            // may add before this encoder learns about it.
            _ => body.push_str("<hp:t/>"),
        }
    }
    format!("<hp:t>{body}</hp:t>")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_inline_text_markup_detects_tab_and_breaks() {
        assert!(requires_inline_text_markup("left\tright"));
        assert!(requires_inline_text_markup("line1\nline2"));
        assert!(requires_inline_text_markup("line1\rline2"));
        assert!(requires_inline_text_markup("a\u{00A0}b"));
        assert!(requires_inline_text_markup("a\u{001F}b"));
        assert!(!requires_inline_text_markup("plain text"));
    }

    #[test]
    fn encode_inline_text_xml_emits_mixed_content_tokens() {
        assert_eq!(
            encode_inline_text_xml("line 1\tline 2\nnext\u{00A0}keep\u{001F}fw"),
            "line 1<hp:tab/>line 2<hp:lineBreak/>next<hp:nbSpace/>keep<hp:fwSpace/>fw"
        );
    }

    #[test]
    fn build_text_element_xml_handles_empty_and_special_text() {
        assert_eq!(build_text_element_xml(""), "<hp:t/>");
        assert_eq!(build_text_element_xml("LEFT\tRIGHT"), "<hp:t>LEFT<hp:tab/>RIGHT</hp:t>");
        assert_eq!(
            build_text_element_xml("FWLEFT\u{001F}FWRIGHT"),
            "<hp:t>FWLEFT<hp:fwSpace/>FWRIGHT</hp:t>"
        );
    }
}

#[cfg(test)]
mod recreation_tests {
    use super::validate_recreatable_inline_markup;

    // 이것을 실패시키는 것: Empty 허용 이름/허용 열거 삭제, 접두사·속성 검사 삭제,
    // 접두사 대소문자 비교 완화, Text·GeneralRef·CData 허용 삭제, Eof 거부,
    // Start·End·Comment·PI·Decl·DocType·파싱 오류 허용.
    #[test]
    fn f1_recreation_event_table() {
        for (name, inner) in [
            ("hp:t", "a<hp:lineBreak/>   b<hp:nbSpace/><hp:fwSpace/><hp:tab/>"),
            ("t", "a<lineBreak/>   b<nbSpace/><fwSpace/><tab/>"),
            ("x:t", "<x:lineBreak/>"),
            ("hp:t", "<![CDATA[a < b]]><hp:lineBreak/>&amp;&#13;&#x20;"),
            ("hp:t", ""),
        ] {
            assert!(validate_recreatable_inline_markup(name, inner).is_ok(), "{name}: {inner}");
        }
        for inner in [
            "<hp:tab width=\"0\" leader=\"0\" type=\"0\"/>",
            "<hp:tab width=\"2056\" leader=\"0\" type=\"1\"/>",
            "<hp:lineBreak customMode=\"0\"/>",
            "<hp:nbSpace xml:space=\"preserve\"/>",
            "<hp:fwSpace xmlns=\"\"/>",
            "<hp:tab xmlns:hp=\"other\"/>",
            "<x:lineBreak/>",
            "<HP:lineBreak/>",
            "<lineBreak/>",
            "<hp:lineBreak></hp:lineBreak>",
            "</hp:t>",
            "<hp:markpenBegin/>",
            "<hp:markpenEnd/>",
            "<hp:titleMark/>",
            "<hp:unknown/>",
            "<hp:LineBreak/>",
            "<hp:lineBreak/><!--comment-->",
            "<hp:lineBreak/><?pi test?>",
            "<?xml version=\"1.0\"?><hp:lineBreak/>",
            "<!DOCTYPE t><hp:lineBreak/>",
            "<hp:tab broken/>",
            "<hp:tab",
        ] {
            assert!(validate_recreatable_inline_markup("hp:t", inner).is_err(), "{inner}");
        }
        assert!(validate_recreatable_inline_markup("t", "<hp:lineBreak/>").is_err());
    }

    // 이것을 실패시키는 것: 각 거부 사유를 다른 이름으로 바꾸기,
    // 첫 comment를 건너뛰어 뒤의 PI를 원인으로 보고하기.
    #[test]
    fn f1_rejection_reports_the_first_cause() {
        for (inner, reason) in [
            ("<hp:unknown/>", "hp:unknown"),
            ("<x:lineBreak/>", "x:lineBreak"),
            ("<hp:tab width=\"0\"/>", "attribute on <hp:tab>"),
            ("<hp:tab broken/>", "attribute on <hp:tab>"),
            ("<hp:lineBreak></hp:lineBreak>", "start tag <hp:lineBreak>"),
            ("a<hp:lineBreak/><!--keep--><?pi test?>b", "comment"),
            ("<hp:lineBreak/><?pi test?>", "pi"),
            ("<?xml version=\"1.0\"?><hp:lineBreak/>", "decl"),
            ("<!DOCTYPE t><hp:lineBreak/>", "doctype"),
            ("<hp:tab", "invalid XML"),
            ("</hp:t>", "invalid XML"),
        ] {
            assert_eq!(
                validate_recreatable_inline_markup("hp:t", inner),
                Err(reason.to_owned()),
                "{inner}"
            );
        }
    }
}
