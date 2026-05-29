//! Shared helpers for inline HWPX text markup.
//!
//! HWPX uses mixed content inside `<hp:t>` for certain characters:
//! line breaks, tabs, and non-breaking spaces are encoded as child elements
//! instead of plain text. Core keeps text as a `String`, so the smithy layer
//! is responsible for translating between the plain-text view and the HWPX
//! mixed-content view.

use hwpforge_core::inline::{InlineSegment, InlineText};

use crate::encoder::escape_xml;

/// Returns `true` when a plain-text payload requires inline HWPX child
/// elements inside `<hp:t>`.
pub(crate) fn requires_inline_text_markup(text: &str) -> bool {
    text.chars().any(|ch| matches!(ch, '\n' | '\t' | '\u{00A0}' | '\u{001F}'))
}

/// Encodes plain-text content into the inner XML for an `<hp:t>` element.
///
/// The mapping is intentionally narrow and lossless for the characters that
/// Core can currently represent:
///
/// - `\n` → `<hp:lineBreak/>`
/// - `\t` → `<hp:tab/>`
/// - `U+00A0` → `<hp:nbSpace/>`
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

    for ch in text.chars() {
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
