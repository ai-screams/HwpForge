//! Shared helpers for inline HWPX text markup.
//!
//! HWPX uses mixed content inside `<hp:t>` for certain characters:
//! line breaks, tabs, and non-breaking spaces are encoded as child elements
//! instead of plain text. Core keeps text as a `String`, so the smithy layer
//! is responsible for translating between the plain-text view and the HWPX
//! mixed-content view.

use crate::encoder::escape_xml;

/// Returns `true` when a plain-text payload requires inline HWPX child
/// elements inside `<hp:t>`.
pub(crate) fn requires_inline_text_markup(text: &str) -> bool {
    text.chars().any(|ch| matches!(ch, '\n' | '\t' | '\u{00A0}'))
}

/// Encodes plain-text content into the inner XML for an `<hp:t>` element.
///
/// The mapping is intentionally narrow and lossless for the characters that
/// Core can currently represent:
///
/// - `\n` → `<hp:lineBreak/>`
/// - `\t` → `<hp:tab/>`
/// - `U+00A0` → `<hp:nbSpace/>`
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_inline_text_markup_detects_tab_and_breaks() {
        assert!(requires_inline_text_markup("left\tright"));
        assert!(requires_inline_text_markup("line1\nline2"));
        assert!(requires_inline_text_markup("a\u{00A0}b"));
        assert!(!requires_inline_text_markup("plain text"));
    }

    #[test]
    fn encode_inline_text_xml_emits_mixed_content_tokens() {
        assert_eq!(
            encode_inline_text_xml("line 1\tline 2\nnext\u{00A0}keep"),
            "line 1<hp:tab/>line 2<hp:lineBreak/>next<hp:nbSpace/>keep"
        );
    }

    #[test]
    fn build_text_element_xml_handles_empty_and_special_text() {
        assert_eq!(build_text_element_xml(""), "<hp:t/>");
        assert_eq!(build_text_element_xml("LEFT\tRIGHT"), "<hp:t>LEFT<hp:tab/>RIGHT</hp:t>");
    }
}
