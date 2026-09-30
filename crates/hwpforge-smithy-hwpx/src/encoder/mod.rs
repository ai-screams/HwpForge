//! HWPX encoder pipeline.
//!
//! Submodules handle individual stages:
//! - `header` — [`HwpxStyleStore`] → `header.xml` serialization
//! - `section` — Core `Section` → `section*.xml` serialization
//! - `package` — ZIP assembly (mimetype, metadata, content files)
//!
//! The public entry point is [`HwpxEncoder`], which orchestrates
//! the full pipeline: header → sections → ZIP packaging.

pub(crate) mod chart;
pub(crate) mod header;
pub(crate) mod header_tabs;
pub(crate) mod package;
pub(crate) mod section;
pub(crate) mod shapes;

/// Cleans a font name before it is written (`hh:font@face`,
/// `hp:equation@font`): control characters are removed and the ends are
/// trimmed; spaces inside the name are kept (`Times New Roman`).
///
/// Hancom matches the name against installed fonts exactly, and neither a
/// control character nor a space at either end counts toward a name it can
/// match: with a space in place of the control character (what
/// [`normalize_attr_control_whitespace`] would write, and what Hancom reads
/// from 0.41's literal control character) `함초롬바탕 ` leaves the font box
/// empty and `HancomEQN ` breaks the equation, and ` 함초롬바탕` does the
/// same. Cleaning the name lets it match. The decoder reads names through
/// this function too, so decode→encode→decode stays a no-op for edits. No
/// warning is raised (the change is recorded in the changelog).
pub(crate) fn clean_font_name(name: &str) -> String {
    let without_controls: String = name.chars().filter(|c| !c.is_control()).collect();
    without_controls.trim().to_string()
}

/// Rewrites the control-whitespace character references that quick-xml's
/// serde serializer writes in attribute values (`&#9;`, `&#10;`, `&#13;`) as
/// plain spaces.
///
/// quick-xml 0.42 escapes tab, LF and CR inside attribute values; 0.41 wrote
/// them literally, and an XML parser reads a literal one as a space
/// (end-of-line handling folds CR LF into one LF, then attribute-value
/// normalization turns each tab or LF into a space). Hancom reads the
/// references instead and keeps the raw character: a style name `본&#13;문`
/// shows only "본". Writing the space directly gives Hancom the value it
/// read from 0.41 output (`본 문`, measured by re-saving in Hancom), so no
/// warning is raised.
///
/// This restores 0.41's reading; it does not make every such value usable.
/// A font name with a space at either end still matches no installed font,
/// so the font list (`hh:font@face`) and equation font (`hp:equation@font`)
/// are cleaned before serialization by [`clean_font_name`]. The text-art font
/// (`hp:textartPr@fontName`) is written by its own escape path and is not
/// cleaned here; text-art attributes are left as 0.41 wrote them (their line
/// breaks in the `text` attribute are issue #199).
///
/// `&#13;&#10;` (one line end) becomes one space; after that each remaining
/// reference becomes one space. Only quoted attribute values inside start
/// tags change. Text, comments, CDATA sections and processing instructions
/// are left as they are, which keeps `&#13;` in `hp:t` and `hp:script`
/// text as 0.42 writes it. A user string `&#10;` is serialized as
/// `&amp;#10;` and is not a reference, so it is untouched.
///
/// The input must be quick-xml serde output: it relies on every `<` in
/// text and attribute values being escaped, so an unescaped `<` starts
/// markup. It is not a general XML rewriter.
pub(crate) fn normalize_attr_control_whitespace(xml: &str) -> std::borrow::Cow<'_, str> {
    if !xml.contains("&#") {
        return std::borrow::Cow::Borrowed(xml);
    }
    // All matching is on bytes: `i` may sit inside a multi-byte UTF-8
    // sequence, and slicing `xml` there would panic. Slices of `xml` are
    // taken only at ASCII `&` and after ASCII `;`, which are char boundaries.
    let bytes = xml.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    let mut in_tag = false;
    let mut quote: Option<u8> = None;
    while i < bytes.len() {
        if !in_tag {
            if bytes[i] == b'<' {
                let rest = &bytes[i..];
                let skip: Option<(usize, &[u8])> = if rest.starts_with(b"<!--") {
                    Some((4, b"-->"))
                } else if rest.starts_with(b"<![CDATA[") {
                    Some((9, b"]]>"))
                } else if rest.starts_with(b"<?") {
                    Some((2, b"?>"))
                } else {
                    None
                };
                if let Some((open, close)) = skip {
                    i = bytes[i + open..]
                        .windows(close.len())
                        .position(|w| w == close)
                        .map_or(bytes.len(), |p| i + open + p + close.len());
                    continue;
                }
                in_tag = true;
            }
            i += 1;
            continue;
        }
        match quote {
            None => {
                if bytes[i] == b'"' || bytes[i] == b'\'' {
                    quote = Some(bytes[i]);
                } else if bytes[i] == b'>' {
                    in_tag = false;
                }
                i += 1;
            }
            Some(q) if bytes[i] == q => {
                quote = None;
                i += 1;
            }
            Some(_) => {
                let rest = &bytes[i..];
                let len = if rest.starts_with(b"&#13;&#10;") {
                    10
                } else if rest.starts_with(b"&#9;") {
                    4
                } else if rest.starts_with(b"&#10;") || rest.starts_with(b"&#13;") {
                    5
                } else {
                    0
                };
                if len == 0 {
                    i += 1;
                    continue;
                }
                let buf = out.get_or_insert_with(|| String::with_capacity(xml.len()));
                buf.push_str(&xml[copied..i]);
                buf.push(' ');
                i += len;
                copied = i;
            }
        }
    }
    match out {
        None => std::borrow::Cow::Borrowed(xml),
        Some(mut buf) => {
            buf.push_str(&xml[copied..]);
            std::borrow::Cow::Owned(buf)
        }
    }
}

/// Escapes XML special characters in text content **and** strips Unicode
/// code points illegal in XML 1.0 character content.
///
/// Combines two responsibilities in a single pass:
///
/// 1. **Metacharacter escaping** — `&`, `<`, `>`, and `"` are encoded as
///    `&amp;`, `&lt;`, `&gt;`, and `&quot;`. Single quotes (`'`) are
///    **not** escaped because all HWPX attribute values produced by this
///    encoder use double-quote delimiters. If a future caller places
///    escaped values inside single-quoted XML attributes, `&apos;`
///    escaping must be added.
///
/// 2. **Illegal-character strip** — Wave 12n leftover hardening (#87)
///    promoted the previously metadata-only `sanitize_xml_text` strip
///    to apply at every emit surface. The same `\x01..=\x08 | \x0B |
///    \x0C | \x0E..=\x1F | U+FFFE | U+FFFF` ranges are dropped here so
///    no caller can accidentally inject parser-fatal bytes through the
///    50+ direct uses of `escape_xml` scattered throughout
///    `encoder::section` / `encoder::header` / `encoder::shapes`.
///
/// The standalone [`sanitize_xml_text`] remains available for callers
/// that only want the strip step (e.g. text routed through other
/// escape paths). [`escape_xml_text_safe`] is the explicit-name
/// convenience wrapper used by metadata; it is now equivalent to
/// `escape_xml` for the strip+escape sequence but preserves the
/// historical naming.
pub(crate) fn escape_xml(s: &str) -> String {
    // Single-pass: only allocate when a special character is found.
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            '\t' | '\n' | '\r' => result.push(ch),
            '\u{0001}'..='\u{0008}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000E}'..='\u{001F}'
            | '\u{FFFE}'
            | '\u{FFFF}' => { /* strip — XML 1.0 illegal range */ }
            _ => result.push(ch),
        }
    }
    result
}

/// Strips Unicode code points that are illegal in XML 1.0 character content.
///
/// Removes:
/// - **C0 control characters** (U+0000 – U+001F) except `\t` (U+0009),
///   `\n` (U+000A), and `\r` (U+000D)
/// - **Unicode non-characters** U+FFFE / U+FFFF, which are explicitly
///   forbidden by the XML 1.0 Character Range production
///   (`#x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`)
/// - **Surrogate code points** U+D800 – U+DFFF cannot occur in a
///   well-formed `&str` (Rust enforces valid UTF-8), so they are not
///   explicitly checked but the documentation calls out the rejection
///   contract.
///
/// This is a **separate** stage from [`escape_xml`]: escaping only handles
/// metacharacters that have meaning inside well-formed XML, while this
/// sanitizer rejects bytes that the parser would reject *before* any
/// escaping applied. Apply this first when user-controlled string values
/// flow into XML text content (e.g. document metadata).
///
/// Wave 12o architect review S1: separating concerns prevents the common
/// foot-gun where `escape_xml` produces well-formed-looking output that a
/// strict downstream parser still rejects.
pub(crate) fn sanitize_xml_text(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\t' | '\n' | '\r' => result.push(ch),
            '\u{0001}'..='\u{0008}'
            | '\u{000B}'
            | '\u{000C}'
            | '\u{000E}'..='\u{001F}'
            | '\u{FFFE}'
            | '\u{FFFF}' => { /* strip */ }
            _ => result.push(ch),
        }
    }
    result
}

/// Convenience: sanitize then escape. Used by metadata writers where
/// values flow straight from user-controlled `Metadata` fields into XML
/// text content.
pub(crate) fn escape_xml_text_safe(s: &str) -> String {
    escape_xml(&sanitize_xml_text(s))
}

/// Returns `true` if the URL uses a safe scheme for hyperlinks.
///
/// Only `http://`, `https://`, `mailto:`, and empty URLs are accepted.
/// Dangerous schemes like `javascript:`, `data:`, and `file:` are rejected
/// to prevent XSS and local file access when the HWPX is rendered in a
/// web-based viewer.
pub(crate) fn is_safe_url(url: &str) -> bool {
    let lower = url.to_ascii_lowercase();
    lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("mailto:")
        || url.is_empty()
}

/// Detects an explicit URL scheme per the RFC 3986 grammar
/// (`ALPHA *( ALPHA / DIGIT / "+" / "-" / "." ) ":"`).
///
/// Returns the scheme substring when `url` begins with one, or `None` when
/// `url` is schemeless (a bare domain like `www.go.kr`). A bare `host:port`
/// (digits after the colon, e.g. `example.com:8080`) is intentionally **not**
/// treated as a scheme, since the colon there denotes a port.
fn explicit_scheme(url: &str) -> Option<&str> {
    let colon = url.find(':')?;
    let scheme = &url[..colon];
    let mut chars = scheme.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return None,
    }
    if !chars.all(|c| c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.') {
        return None;
    }
    let rest = &url[colon + 1..];
    // `scheme://...` (authority form) is unambiguously a scheme.
    if rest.starts_with("//") {
        return Some(scheme);
    }
    // `host:port` — everything up to the next `/` is digits → it is a port,
    // not a scheme, so the whole thing is a schemeless bare URL.
    let port_part = rest.split('/').next().unwrap_or("");
    if !port_part.is_empty() && port_part.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(scheme)
}

/// Normalizes a hyperlink URL for safe embedding in HWPX.
///
/// - Empty URLs and URLs already using a safe scheme (`http://`, `https://`,
///   `mailto:`) pass through unchanged.
/// - Schemeless URLs (bare domains such as `www.motie.go.kr` or
///   `example.com:8080/path`) are normalized by prepending `http://`, matching
///   how 한글 treats schemeless hyperlinks. This prevents a single schemeless
///   link from aborting the conversion of an entire document.
/// - URLs with an explicit but unsafe scheme (`javascript:`, `data:`, `file:`,
///   …) are rejected (returns `None`) to preserve the XSS / local-file
///   boundary enforced by [`is_safe_url`].
pub(crate) fn normalize_hyperlink_url(url: &str) -> Option<String> {
    if is_safe_url(url) {
        return Some(url.to_string());
    }
    match explicit_scheme(url) {
        // Explicit scheme that is not in the safe allowlist → reject.
        Some(_) => None,
        // Schemeless bare URL → normalize to an http:// web link.
        None => Some(format!("http://{url}")),
    }
}

/// Sanitizes a filename for safe use as a ZIP archive entry.
///
/// Strips leading slashes and rejects `..` path components to prevent
/// path traversal attacks (CWE-22) when the ZIP is extracted.
pub(crate) fn sanitize_zip_entry_name(name: &str) -> String {
    name.split('/').filter(|c| !c.is_empty() && *c != "..").collect::<Vec<_>>().join("/")
}

#[cfg(test)]
mod sanitize_xml_text_tests {
    use super::sanitize_xml_text;

    #[test]
    fn allows_tab_lf_cr() {
        assert_eq!(sanitize_xml_text("a\tb\nc\rd"), "a\tb\nc\rd");
    }

    #[test]
    fn strips_c0_controls_except_tab_lf_cr() {
        // U+0001 .. U+0008, U+000B, U+000C, U+000E .. U+001F all stripped.
        let input = "x\u{0001}y\u{0008}z\u{000B}w\u{000C}v\u{000E}u\u{001F}t";
        assert_eq!(sanitize_xml_text(input), "xyzwvut");
    }

    #[test]
    fn strips_non_characters_fffe_ffff() {
        assert_eq!(sanitize_xml_text("a\u{FFFE}b\u{FFFF}c"), "abc");
    }

    #[test]
    fn preserves_korean_text() {
        assert_eq!(sanitize_xml_text("안녕하세요 Wave 12o"), "안녕하세요 Wave 12o");
    }

    #[test]
    fn preserves_xml_metachars() {
        // Sanitization does NOT escape — that's escape_xml's job.
        assert_eq!(sanitize_xml_text("<a&b>"), "<a&b>");
    }
}

#[cfg(test)]
mod escape_xml_tests {
    use super::escape_xml;

    #[test]
    fn empty_string() {
        assert_eq!(escape_xml(""), "");
    }

    #[test]
    fn no_special_chars() {
        let input = "Hello World 123";
        assert_eq!(escape_xml(input), input);
    }

    #[test]
    fn all_special_chars() {
        assert_eq!(escape_xml("<>&\""), "&lt;&gt;&amp;&quot;");
    }

    #[test]
    fn mixed_content() {
        assert_eq!(escape_xml("a < b & c"), "a &lt; b &amp; c");
    }

    #[test]
    fn ampersand_first() {
        // Ampersand must be replaced first to avoid double-escaping
        assert_eq!(escape_xml("&<"), "&amp;&lt;");
    }

    #[test]
    fn korean_text_unchanged() {
        let input = "안녕하세요 테스트";
        assert_eq!(escape_xml(input), input);
    }

    #[test]
    fn url_with_ampersand() {
        assert_eq!(escape_xml("https://example.com?a=1&b=2"), "https://example.com?a=1&amp;b=2");
    }

    // ── Wave 12n leftover #87 — C0 / illegal-char strip integrated ──

    /// `escape_xml` now also strips XML 1.0-illegal control characters so
    /// the 50+ direct callers across encoder/section, encoder/header,
    /// and encoder/shapes do not need to be individually audited.
    #[test]
    fn strips_c0_controls_in_addition_to_escape() {
        let input = "a\u{0001}b\u{0008}c\u{000B}d";
        assert_eq!(escape_xml(input), "abcd");
    }

    #[test]
    fn preserves_tab_lf_cr_during_escape() {
        // Hancom HWPX uses literal newlines inside `<hp:t>` for memo body
        // continuation; escape_xml must NOT strip those.
        assert_eq!(escape_xml("line1\nline2\tindent\rfinal"), "line1\nline2\tindent\rfinal");
    }

    #[test]
    fn strips_non_characters_alongside_metachar_escape() {
        let input = "x\u{FFFE}<\u{FFFF}>";
        assert_eq!(escape_xml(input), "x&lt;&gt;");
    }
}

#[cfg(test)]
mod is_safe_url_tests {
    use super::is_safe_url;

    #[test]
    fn http_allowed() {
        assert!(is_safe_url("http://example.com"));
    }

    #[test]
    fn https_allowed() {
        assert!(is_safe_url("https://example.com/path?q=1"));
    }

    #[test]
    fn mailto_allowed() {
        assert!(is_safe_url("mailto:user@example.com"));
    }

    #[test]
    fn empty_allowed() {
        assert!(is_safe_url(""));
    }

    #[test]
    fn javascript_rejected() {
        assert!(!is_safe_url("javascript:alert(1)"));
    }

    #[test]
    fn javascript_mixed_case_rejected() {
        assert!(!is_safe_url("JaVaScRiPt:alert(1)"));
    }

    #[test]
    fn data_uri_rejected() {
        assert!(!is_safe_url("data:text/html,<script>alert(1)</script>"));
    }

    #[test]
    fn file_uri_rejected() {
        assert!(!is_safe_url("file:///etc/passwd"));
    }

    #[test]
    fn ftp_rejected() {
        assert!(!is_safe_url("ftp://example.com"));
    }

    #[test]
    fn bare_path_rejected() {
        assert!(!is_safe_url("/etc/passwd"));
    }
}

#[cfg(test)]
mod normalize_hyperlink_url_tests {
    use super::normalize_hyperlink_url;

    #[test]
    fn http_passes_through() {
        assert_eq!(
            normalize_hyperlink_url("http://example.com").as_deref(),
            Some("http://example.com")
        );
    }

    #[test]
    fn https_passes_through() {
        assert_eq!(
            normalize_hyperlink_url("https://example.com/path?q=1").as_deref(),
            Some("https://example.com/path?q=1")
        );
    }

    #[test]
    fn mailto_passes_through() {
        assert_eq!(
            normalize_hyperlink_url("mailto:user@example.com").as_deref(),
            Some("mailto:user@example.com")
        );
    }

    #[test]
    fn empty_passes_through() {
        assert_eq!(normalize_hyperlink_url("").as_deref(), Some(""));
    }

    #[test]
    fn bare_domain_gets_http_prefix() {
        // The real-world corpus case: 한글 stores schemeless government domains.
        assert_eq!(
            normalize_hyperlink_url("www.motie.go.kr").as_deref(),
            Some("http://www.motie.go.kr")
        );
    }

    #[test]
    fn bare_domain_without_www_gets_http_prefix() {
        assert_eq!(normalize_hyperlink_url("motie.go.kr").as_deref(), Some("http://motie.go.kr"));
    }

    #[test]
    fn bare_domain_with_path_gets_http_prefix() {
        assert_eq!(
            normalize_hyperlink_url("www.kotra.or.kr/opengallery").as_deref(),
            Some("http://www.kotra.or.kr/opengallery")
        );
    }

    #[test]
    fn host_with_port_is_treated_as_bare_url() {
        // The colon here is a port separator, not a scheme.
        assert_eq!(
            normalize_hyperlink_url("example.com:8080/path").as_deref(),
            Some("http://example.com:8080/path")
        );
    }

    #[test]
    fn javascript_scheme_rejected() {
        assert_eq!(normalize_hyperlink_url("javascript:alert(1)"), None);
    }

    #[test]
    fn data_uri_rejected() {
        assert_eq!(normalize_hyperlink_url("data:text/html,<script>"), None);
    }

    #[test]
    fn file_uri_rejected() {
        assert_eq!(normalize_hyperlink_url("file:///etc/passwd"), None);
    }

    #[test]
    fn ftp_scheme_rejected() {
        // ftp is an explicit scheme outside the safe allowlist.
        assert_eq!(normalize_hyperlink_url("ftp://example.com"), None);
    }
}

#[cfg(test)]
mod sanitize_zip_tests {
    use super::sanitize_zip_entry_name;

    #[test]
    fn normal_path_unchanged() {
        assert_eq!(sanitize_zip_entry_name("BinData/logo.png"), "BinData/logo.png");
    }

    #[test]
    fn strips_dotdot() {
        assert_eq!(sanitize_zip_entry_name("../../../etc/passwd"), "etc/passwd");
    }

    #[test]
    fn strips_leading_slash() {
        assert_eq!(sanitize_zip_entry_name("/absolute/path.png"), "absolute/path.png");
    }

    #[test]
    fn strips_empty_components() {
        assert_eq!(sanitize_zip_entry_name("a//b///c"), "a/b/c");
    }

    #[test]
    fn dotdot_in_middle() {
        assert_eq!(sanitize_zip_entry_name("a/../b/file.txt"), "a/b/file.txt");
    }

    #[test]
    fn single_filename() {
        assert_eq!(sanitize_zip_entry_name("file.png"), "file.png");
    }
}

use std::path::Path;

use hwpforge_core::document::{Document, Validated};
use hwpforge_core::image::ImageStore;

use crate::error::{HwpxError, HwpxResult};
use crate::style_store::HwpxStyleStore;

use self::header::encode_header;
use self::package::PackageWriter;
use self::section::encode_section_with_note_counters;

// ── HwpxEncoder ─────────────────────────────────────────────────
/// 인코드 중 표면화된 비치명 경고 (W1b — §1g v5 변경 2).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum EncodeWarning {
    /// 문단 캐시가 방출되지 않음 — 방출 wire map 구축 실패, 좌표
    /// 역변환 실패(축약점 모호 등), 또는 cache-inadmissible 방출
    /// (차트 재배치·스킵된 컨트롤 등).
    LayoutCacheDropped {
        /// 해당 문단의 중첩 경로.
        path: crate::decoder::ParagraphPath,
        /// 드롭 사유.
        reason: String,
    },
    /// 각주/미주 번호 머리(autoNum)를 안전하게 주입할 수 없어 생략함 —
    /// 예: 첫 문단이 titleMark run 으로 시작 (TOC "첫 run" 불변식 및
    /// 전체-run 치환 키와 충돌해 안전한 삽입 지점이 없다).
    NoteHeadSkipped {
        /// 해당 note 의 중첩 경로.
        path: crate::decoder::ParagraphPath,
        /// 생략 사유.
        reason: String,
    },
    /// TOC 제목 표식(`<hp:titleMark>`)을 부착하지 못함 — heading 문단의
    /// 첫 run 이 전체-run 치환 placeholder(하이퍼링크/필드)라 부착하면
    /// 치환 키가 어긋나 내부 마커가 최종 XML 로 유출된다.
    TitleMarkSkipped {
        /// 해당 문단의 중첩 경로.
        path: crate::decoder::ParagraphPath,
        /// 생략 사유.
        reason: String,
    },
    /// 후속 섹션의 명시적 각주/미주 시작 번호를 반영하지 못함 — 구역별
    /// 재시작(`ON_SECTION` numbering 정책)은 아직 typed carry 되지 않는다.
    NoteRestartIgnored {
        /// 해당 섹션의 경로.
        path: crate::decoder::ParagraphPath,
        /// 무시 사유.
        reason: String,
    },
}

impl EncodeWarning {
    /// 이 경고가 **의미 손상**인지 — 즉 산출 바이트가 입력의 의미를 잃었는지.
    ///
    /// # 계약
    ///
    /// 재인코드 편집기([`crate::stamp`] 스탬퍼, [`crate::cell_edit`] 셀 편집,
    /// 그리고 앞으로 추가될 restyle 연산)는 preserve-first 다 — 인코드가
    /// 의미 손상 경고를 내면 **바이트를 방출하지 않고 거부**해야 한다
    /// (fail-closed). admission 게이트는 Core 비교라 wire 단계에서 일어난
    /// 손상을 보지 못하므로, 이 경고가 유일한 신호다.
    ///
    /// **이 메서드가 그 집합의 유일한 정의다.** 편집기마다 손으로 적은
    /// 변형 목록을 두지 말고 여기를 호출한다. 내부 `match` 는 와일드카드
    /// 없이 전 변형을 나열하므로, 새 변형이 추가되면 컴파일러가 분류를
    /// 강제한다 (같은 크레이트라 `#[non_exhaustive]` 가 막지 않는다).
    ///
    /// # 현재 분류
    ///
    /// | 변형 | 의미 손상 | 사유 |
    /// | -- | -- | -- |
    /// | [`Self::NoteHeadSkipped`] | 예 | 각주/미주 가시 번호가 산출물에서 사라진다 |
    /// | [`Self::TitleMarkSkipped`] | 예 | TOC 제목 표식이 붙지 않아 목차가 어긋난다 |
    /// | [`Self::NoteRestartIgnored`] | 예 | 구역별 재시작 번호가 반영되지 않아 번호가 틀어진다 |
    /// | [`Self::LayoutCacheDropped`] | 아니오 | 줄 조판 캐시는 문서 의미가 아니라 렌더 입력이다 (아래) |
    ///
    /// # `LayoutCacheDropped` 가 비-의미 손상인 이유
    ///
    /// "렌더러가 알아서 재생성하니까" 가 아니다 — 재생성하는 렌더러와 하지
    /// 않는 렌더러가 갈린다.
    ///
    /// - **한컴**은 문서를 열 때 직접 조판하므로 캐시가 없어도 같은 지면을 만든다.
    /// - **HwpForge 자체 PDF 렌더러는 조판을 하지 않는다.** 저장된 줄 조판 캐시를
    ///   그대로 재생하는 구조라, 캐시가 빠진 문서는 PDF 단계에서 `MISSING_LAYOUT_CACHE`
    ///   / `NO_RENDERABLE_CACHE` 로 **렌더가 실패**한다.
    ///
    /// 그래도 비-의미 손상인 이유는, 그 실패가 **렌더 파이프라인의 가용성**
    /// 문제로 드러날 뿐 문서가 뜻하는 바는 그대로이기 때문이다 — 본문·각주
    /// 번호·TOC 표식 어느 것도 사라지지 않고, 조판할 수 있는 소비자(한컴)는
    /// 원본과 같은 결과를 얻는다. 편집기가 바이트를 거부해야 하는 사유는
    /// "열어 보니 뜻이 달라졌다" 이지 "우리 렌더러가 캐시를 필요로 한다" 가
    /// 아니므로, 이 변형은 fail-closed 대상에서 제외한다.
    #[must_use]
    pub fn is_semantic_loss(&self) -> bool {
        match self {
            Self::NoteHeadSkipped { .. }
            | Self::TitleMarkSkipped { .. }
            | Self::NoteRestartIgnored { .. } => true,
            Self::LayoutCacheDropped { .. } => false,
        }
    }
}

/// 경고 목록을 `(의미 손상, 그 외)` 로 가른다 — **양쪽 모두 원래 순서 유지**.
///
/// [`EncodeWarning::is_semantic_loss`] 가 분류의 유일한 정의이고, 이 함수는
/// 편집기가 그 분류로 목록을 쪼갤 때 쓰는 유일한 경로다 — 순서 계약을 한
/// 곳에만 두려는 것이다 (fail-closed 오류가 두 벡터를 그대로 싣는다).
pub fn partition_semantic_loss(
    warnings: Vec<EncodeWarning>,
) -> (Vec<EncodeWarning>, Vec<EncodeWarning>) {
    warnings.into_iter().partition(EncodeWarning::is_semantic_loss)
}

/// preserve-first 편집기의 인코드 결과를 성공/fail-closed 로 가른다.
///
/// 의미 손상 경고가 하나라도 있으면 `Err((의미 손상, 그 외))` — 호출자가
/// 각자의 오류형(`StamperError::SemanticLoss` ·
/// `CellEditError::SemanticLoss`)으로 감싼다. 없으면
/// `Ok((바이트, 비의미 경고))` — **R2 HIGH 1**: 과거엔 이 지점에서 비의미
/// 경고를 버리고 바이트만 남겼다.
///
/// 편집기마다 복제하지 않고 한 곳에 둔다 (분류의 정의는
/// [`EncodeWarning::is_semantic_loss`] 하나뿐이라는 계약의 연장).
pub(crate) fn split_successful_encode(
    outcome: EncodeOutcome,
) -> Result<(Vec<u8>, Vec<EncodeWarning>), (Vec<EncodeWarning>, Vec<EncodeWarning>)> {
    let (semantic, others) = partition_semantic_loss(outcome.warnings);
    if semantic.is_empty() {
        Ok((outcome.bytes, others))
    } else {
        Err((semantic, others))
    }
}

impl std::fmt::Display for EncodeWarning {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::LayoutCacheDropped { path, reason } => {
                write!(f, "layout cache dropped at {path}: {reason}")
            }
            Self::NoteHeadSkipped { path, reason } => {
                write!(f, "note number head skipped at {path}: {reason}")
            }
            Self::TitleMarkSkipped { path, reason } => {
                write!(f, "titleMark skipped at {path}: {reason}")
            }
            Self::NoteRestartIgnored { path, reason } => {
                write!(f, "note restart ignored at {path}: {reason}")
            }
        }
    }
}

#[cfg(test)]
mod is_semantic_loss_tests {
    use super::EncodeWarning;
    use crate::decoder::{ParagraphPath, PathSeg};

    fn path() -> ParagraphPath {
        ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(3)])
    }

    #[test]
    fn note_and_title_mark_warnings_are_semantic_loss() {
        for warning in [
            EncodeWarning::NoteHeadSkipped { path: path(), reason: "titleMark first run".into() },
            EncodeWarning::TitleMarkSkipped {
                path: path(),
                reason: "placeholder first run".into(),
            },
            EncodeWarning::NoteRestartIgnored { path: path(), reason: "ON_SECTION".into() },
        ] {
            assert!(warning.is_semantic_loss(), "expected semantic loss: {warning:?}");
        }
    }

    #[test]
    fn layout_cache_dropped_is_not_semantic_loss() {
        let warning =
            EncodeWarning::LayoutCacheDropped { path: path(), reason: "ledger failed".into() };
        assert!(!warning.is_semantic_loss(), "layout cache is regenerable: {warning:?}");
    }

    #[test]
    fn partition_keeps_original_order_within_each_side() {
        let cache = |n: &str| EncodeWarning::LayoutCacheDropped { path: path(), reason: n.into() };
        let note = |n: &str| EncodeWarning::NoteHeadSkipped { path: path(), reason: n.into() };
        // Interleaved so a partition that sorted or reversed would be caught.
        let input = vec![cache("c1"), note("n1"), cache("c2"), note("n2"), note("n3")];

        let (semantic, others) = super::partition_semantic_loss(input);

        let reasons = |ws: &[EncodeWarning]| {
            ws.iter()
                .map(|w| match w {
                    EncodeWarning::LayoutCacheDropped { reason, .. }
                    | EncodeWarning::NoteHeadSkipped { reason, .. }
                    | EncodeWarning::TitleMarkSkipped { reason, .. }
                    | EncodeWarning::NoteRestartIgnored { reason, .. } => reason.clone(),
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(reasons(&semantic), ["n1", "n2", "n3"]);
        assert_eq!(reasons(&others), ["c1", "c2"]);
        assert!(semantic.iter().all(EncodeWarning::is_semantic_loss));
        assert!(!others.iter().any(EncodeWarning::is_semantic_loss));
    }
}

/// [`HwpxEncoder::encode_with_diagnostics`] 의 결과 — 산출 바이트 +
/// 전체 typed 경고 (permissive 경로: 캐시 드롭이 있어도 성공).
#[derive(Debug)]
pub struct EncodeOutcome {
    /// HWPX ZIP 바이트.
    pub bytes: Vec<u8>,
    /// 인코드 경고 (캐시 드롭 등).
    pub warnings: Vec<EncodeWarning>,
}

/// Encoder behavior options.
///
/// [`Default`] 는 현행 인코더 동작 그대로다 (출력 바이트 불변).
/// `#[non_exhaustive]` — 외부에서는 [`EncodeOptions::default`] 후
/// 세터로 조정한다.
#[derive(Debug, Clone, Copy, Default)]
#[non_exhaustive]
pub struct EncodeOptions {
    /// `true` 면 [`Paragraph::layout_cache`](hwpforge_core::paragraph::Paragraph::layout_cache)
    /// 를 `<hp:linesegarray>` 로 방출한다. 기본 `false`.
    ///
    /// ⚠️ 이 opt-in 은 **PDF 재생/비교 파이프라인 전용**이다 (HWP5→HWPX
    /// convert carry). 편집 표면은 절대 켜지 않는다 — 승격된 캐시를
    /// 무검증 방출하면 `layout_carry` 의 "이미 있으면 스킵" 안전장치가
    /// fail-open 이 된다. 또한 과거 convert 가 HWP5 lineseg 를 carry 했다가
    /// 한컴에서 다중행 텍스트 겹침을 일으켜 제거한 이력이 있다 — 이
    /// 산출물은 한컴 재개봉 용도가 아니다.
    ///
    /// 좌표 주의: HWPX 디코더가 승격한 캐시의 `textpos` 는 **보이는-텍스트
    /// 좌표로 정규화**돼 있다 (선행 컨트롤 8유닛 차감) — 방출값도 원본
    /// wire 의 스트림 좌표와 다르다.
    pub emit_layout_cache: bool,
}

impl EncodeOptions {
    /// 캐시 방출 여부를 설정한다 (기본 `false`).
    #[must_use]
    pub fn with_emit_layout_cache(mut self, emit: bool) -> Self {
        self.emit_layout_cache = emit;
        self
    }
}

/// Encodes Core documents to HWPX format (ZIP + XML).
///
/// This is the reverse of [`crate::HwpxDecoder`]: it takes a validated
/// document and an [`HwpxStyleStore`] and produces a valid HWPX archive.
///
/// # Round-trip
///
/// ```no_run
/// use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
///
/// let bytes = std::fs::read("input.hwpx").unwrap();
/// let result = HwpxDecoder::decode(&bytes).unwrap();
/// let validated = result.document.validate().unwrap();
/// let output = HwpxEncoder::encode(&validated, &result.style_store, &result.image_store).unwrap();
/// std::fs::write("output.hwpx", &output).unwrap();
/// ```
///
/// # Image Binary Support
///
/// The encoder embeds binary image data from [`ImageStore`] into
/// `BinData/` entries in the ZIP archive. Image paths in the document
/// (e.g. `"BinData/image1.png"`) are matched against the store keys.
/// Images not found in the store are silently skipped (XML reference
/// only, no binary data).
#[derive(Debug, Clone, Copy)]
pub struct HwpxEncoder;

impl HwpxEncoder {
    /// Encodes a validated document with its style store and images to HWPX bytes.
    ///
    /// The returned bytes form a valid ZIP archive that can be written
    /// to a `.hwpx` file or decoded back with [`crate::HwpxDecoder`].
    ///
    /// # Pipeline
    ///
    /// 1. Serialize `HwpxStyleStore` → `header.xml`
    /// 2. Serialize each section → `section{N}.xml`
    /// 3. Collect image binaries from `ImageStore`
    /// 4. Package into ZIP with metadata files + BinData/
    ///
    /// # Errors
    ///
    /// - [`HwpxError::XmlSerialize`] if quick-xml serialization fails
    /// - [`HwpxError::InvalidStructure`] if table nesting exceeds limits
    /// - [`HwpxError::Zip`] if ZIP archive creation fails
    ///
    /// # Warnings are discarded
    ///
    /// 이 편의 API 는 [`EncodeWarning`] (각주 번호 머리 생략·titleMark 보류
    /// 등 의미 경고)을 **폐기한다**. 사용자-노출 write 표면은 반드시
    /// [`Self::encode_with_diagnostics`] 로 경고를 표면화할 것 — CLI/MCP
    /// `convert`·`from-json`·`restyle` 이 그 경로다.
    pub fn encode(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
    ) -> HwpxResult<Vec<u8>> {
        Self::encode_with_options(document, style_store, image_store, EncodeOptions::default())
    }

    /// [`Self::encode`] 에 동작 옵션([`EncodeOptions`])을 더한 변형.
    ///
    /// `EncodeOptions::default()` 를 넘기면 [`Self::encode`] 와 바이트
    /// 단위로 동일한 출력을 낸다.
    ///
    /// [`Self::encode`] 와 동일하게 의미 [`EncodeWarning`] 은 **폐기**된다
    /// (`emit_layout_cache` 요청 중 캐시 드롭만 오류로 승격) — 경고 보존
    /// 경로는 [`Self::encode_with_diagnostics`].
    pub fn encode_with_options(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<Vec<u8>> {
        let outcome = Self::encode_with_diagnostics(document, style_store, image_store, options)?;
        // W1b (§1g v5 변경 2): emit_layout_cache 요청 중 캐시 드롭은 무음
        // 성공 금지 — 데이터 손실을 진단 없이 넘기지 않는다 (breaking,
        // 0.14.0). 경고 보존 경로는 `encode_with_diagnostics`.
        if options.emit_layout_cache {
            // 독립 리뷰 Low: first() 는 미래 variant 가 앞에 끼는 순간
            // 데이터-손실 신호를 삼킨다 — find 로 전수 검색.
            if let Some(EncodeWarning::LayoutCacheDropped { path, reason }) = outcome
                .warnings
                .iter()
                .find(|w| matches!(w, EncodeWarning::LayoutCacheDropped { .. }))
            {
                return Err(crate::error::HwpxError::LayoutCacheDropped {
                    path: path.to_string(),
                    reason: reason.clone(),
                });
            }
        }
        Ok(outcome.bytes)
    }

    /// [`Self::encode_with_options`] 의 진단 보존 변형 — 캐시 드롭이
    /// 있어도 성공하고 typed 경고([`EncodeWarning`])를 함께 반환한다
    /// (convert carry 파이프라인이 경고를 `ConvertWarning` 으로 전파).
    pub fn encode_with_diagnostics(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<EncodeOutcome> {
        let sections = document.sections();
        let sec_cnt = sections.len() as u32;

        // Step 1: Encode header
        let begin_num = sections.first().and_then(|s| s.begin_num.as_ref());
        let header_xml = encode_header(style_store, sec_cnt, begin_num)?;

        // Step 2: Encode sections (each produces XML + chart + masterpage entries)
        // chart_offset / masterpage_offset / embedded_ole_offset track global
        // indices across sections to avoid duplicate filenames or item ids in
        // the ZIP archive and content.hpf manifest.
        let mut chart_offset = 0usize;
        let mut masterpage_offset = 0usize;
        let mut embedded_ole_offset = 0usize;
        let mut section_results = Vec::with_capacity(sections.len());
        // 각주/미주 autoNum 번호 — 문서 전역 연속 (한컴 기본 CONTINUOUS,
        // F7 실측: 종류별 순번 캐시). 첫 섹션 begin_num(= header
        // <hh:beginNum> 병합 실값)만 문서 시작에서 1회 반영한다. 후속
        // 섹션의 begin_num 은 **무시**되고 (합성 1 — 비기본 실값이면
        // NoteRestartIgnored 경고), 본문 NewNumber 재시작은 walk 가 처리.
        let mut note_counters = crate::encoder::section::NoteNumbering::from_document_begin(
            sections.first().and_then(|s| s.begin_num.as_ref()),
        );
        for (i, section) in sections.iter().enumerate() {
            let result = encode_section_with_note_counters(
                section,
                i,
                chart_offset,
                masterpage_offset,
                embedded_ole_offset,
                options,
                &mut note_counters,
            )?;
            chart_offset += result.charts.len();
            masterpage_offset += result.master_pages.len();
            embedded_ole_offset += result.embedded_oles.len();
            section_results.push(result);
        }
        let mut warnings: Vec<EncodeWarning> = Vec::new();
        for r in &mut section_results {
            warnings.append(&mut r.warnings);
        }

        // Single move-consuming pass: extract all four fields without cloning
        // (previously xml/charts/embedded_oles were `.iter().clone()`d).
        // Push/extend order preserves the original per-section ordering.
        type SectionParts =
            (Vec<String>, Vec<(String, String)>, Vec<(String, Vec<u8>)>, Vec<(String, String)>);
        let (section_xmls, charts, embedded_oles, master_pages): SectionParts = section_results
            .into_iter()
            .fold(Default::default(), |(mut xmls, mut charts, mut oles, mut mps), r| {
                xmls.push(r.xml);
                charts.extend(r.charts);
                oles.extend(r.embedded_oles);
                mps.extend(r.master_pages);
                (xmls, charts, oles, mps)
            });

        // Step 3: Collect image binaries
        let images: Vec<(String, Vec<u8>)> =
            image_store.iter().map(|(key, data)| (key.to_string(), data.to_vec())).collect();

        // Step 4: Package into ZIP with images, charts, master pages, and
        // embedded-chart OLE blobs. Document.metadata flows into content.hpf
        // <opf:metadata> (Wave 12o Phase 1).
        let bytes = PackageWriter::write_hwpx(
            document.metadata(),
            &header_xml,
            &section_xmls,
            &images,
            &charts,
            &master_pages,
            &embedded_oles,
        )?;
        Ok(EncodeOutcome { bytes, warnings })
    }

    /// Encodes a validated document and writes it to a file.
    ///
    /// Convenience wrapper around [`encode`](Self::encode) +
    /// [`std::fs::write`].
    ///
    /// # Errors
    ///
    /// Returns [`HwpxError::Io`] if the file cannot be written, or any
    /// error from [`encode`](Self::encode).
    ///
    /// [`encode`](Self::encode) 와 동일하게 **[`EncodeWarning`] 을 폐기**
    /// 한다 — 경고 보존이 필요하면 [`Self::encode_with_diagnostics`] 후
    /// 직접 파일로 쓸 것.
    pub fn encode_file(
        path: impl AsRef<Path>,
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
    ) -> HwpxResult<()> {
        let bytes = Self::encode(document, style_store, image_store)?;
        std::fs::write(path.as_ref(), bytes).map_err(HwpxError::Io)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HwpxDecoder;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::Section;
    use hwpforge_core::PageSettings;
    use hwpforge_foundation::{
        Alignment, CharShapeIndex, Color, EmbossType, EngraveType, FontIndex, HwpUnit,
        LineSpacingType, OutlineType, ParaShapeIndex, ShadowType, StrikeoutShape, UnderlineType,
        VerticalPosition,
    };

    use crate::style_store::{HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape};

    /// R2 HIGH 1 — the defect was that a *successful* encode kept only the
    /// bytes. These four cases pin the split both editors now share.
    ///
    /// The encoder cannot be driven to emit a non-semantic warning through
    /// `set_cells`/`stamp` (the only one, `LayoutCacheDropped`, needs
    /// `EncodeOptions::emit_layout_cache`, which preserve-first editors must
    /// never set), so the outcome is constructed directly here. That keeps
    /// the assertion about the code under test rather than about which
    /// fixture happens to trip the encoder.
    mod split_successful_encode {
        use super::*;

        fn path() -> crate::decoder::ParagraphPath {
            crate::decoder::ParagraphPath(vec![crate::decoder::PathSeg::Section(0)])
        }

        fn cache_dropped(reason: &str) -> EncodeWarning {
            EncodeWarning::LayoutCacheDropped { path: path(), reason: reason.to_string() }
        }

        fn semantic(reason: &str) -> EncodeWarning {
            EncodeWarning::NoteHeadSkipped { path: path(), reason: reason.to_string() }
        }

        #[test]
        fn a_successful_encode_carries_its_nonsemantic_warnings() {
            let outcome = EncodeOutcome {
                bytes: vec![1, 2, 3],
                warnings: vec![cache_dropped("first"), cache_dropped("second")],
            };

            let (bytes, warnings) =
                split_successful_encode(outcome).expect("no semantic loss means success");

            assert_eq!(bytes, vec![1, 2, 3]);
            assert_eq!(
                warnings,
                vec![cache_dropped("first"), cache_dropped("second")],
                "non-semantic warnings must survive the success path, in encoder order"
            );
        }

        #[test]
        fn a_clean_encode_carries_an_empty_list() {
            let outcome = EncodeOutcome { bytes: vec![7], warnings: Vec::new() };

            let (bytes, warnings) = split_successful_encode(outcome).expect("success");

            assert_eq!(bytes, vec![7]);
            assert!(warnings.is_empty());
        }

        #[test]
        fn semantic_loss_fails_closed_and_keeps_both_groups() {
            let outcome = EncodeOutcome {
                bytes: vec![9],
                warnings: vec![cache_dropped("kept"), semantic("titleMark"), cache_dropped("too")],
            };

            let (loss, others) =
                split_successful_encode(outcome).expect_err("semantic loss must fail closed");

            assert_eq!(loss, vec![semantic("titleMark")]);
            assert_eq!(
                others,
                vec![cache_dropped("kept"), cache_dropped("too")],
                "the non-semantic remainder rides along with the refusal, in order"
            );
        }

        #[test]
        fn fail_closed_produces_no_bytes_at_all() {
            let outcome =
                EncodeOutcome { bytes: vec![1, 2, 3], warnings: vec![semantic("titleMark")] };

            // The `Err` variant has no byte channel, so a caller cannot
            // reach the output of an encode that lost meaning.
            assert!(split_successful_encode(outcome).is_err());
        }
    }

    /// Creates a minimal validated document + style store for testing.
    fn minimal_doc_and_store() -> (Document<Validated>, HwpxStyleStore) {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef::default(),
            height: HwpUnit::new(1000).unwrap(),
            text_color: Color::BLACK,
            shade_color: None,
            bold: false,
            italic: false,
            underline_type: UnderlineType::None,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            vertical_position: VerticalPosition::Normal,
            outline_type: OutlineType::None,
            shadow_type: ShadowType::None,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
            ..Default::default()
        });
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Left,
            margin_left: HwpUnit::ZERO,
            margin_right: HwpUnit::ZERO,
            indent: HwpUnit::ZERO,
            spacing_before: HwpUnit::ZERO,
            spacing_after: HwpUnit::ZERO,
            line_spacing: 160,
            line_spacing_type: LineSpacingType::Percentage,
            ..Default::default()
        });

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("안녕하세요", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();
        (validated, store)
    }

    // ── 1. Basic encode produces valid ZIP ──────────────────────

    #[test]
    fn encode_produces_valid_zip() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // Must be a valid ZIP (starts with PK magic bytes)
        assert_eq!(&bytes[0..2], b"PK", "output must be a ZIP archive");
        assert!(bytes.len() > 100, "ZIP too small: {} bytes", bytes.len());
    }

    // ── 2. Full encode → decode roundtrip ──────────────────────

    #[test]
    fn encode_decode_roundtrip() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // Decode the encoded output
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        // Document structure preserved
        assert_eq!(decoded.document.sections().len(), 1);
        let section = &decoded.document.sections()[0];
        assert_eq!(section.paragraphs.len(), 1);
        assert_eq!(section.paragraphs[0].runs[0].content.as_text(), Some("안녕하세요"),);

        // Style store preserved (fonts expanded to 7 language groups: 1 × 7 = 7)
        assert_eq!(decoded.style_store.font_count(), 7);
        let font = decoded.style_store.font(FontIndex::new(0)).unwrap();
        assert_eq!(font.face_name, "함초롬돋움");
        assert_eq!(font.lang, "HANGUL");

        assert_eq!(decoded.style_store.char_shape_count(), store.char_shape_count());
        let cs = decoded.style_store.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 1000);
        assert!(!cs.bold);

        assert_eq!(decoded.style_store.para_shape_count(), store.para_shape_count());
        let ps = decoded.style_store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Left);
        assert_eq!(ps.line_spacing, 160);
    }

    // ── 3. Multi-section roundtrip ─────────────────────────────

    #[test]
    fn multi_section_roundtrip() {
        let (_, store) = minimal_doc_and_store();

        let mut doc = Document::new();
        for i in 0..3 {
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::text(format!("Section {i}"), CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            ));
        }
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        assert_eq!(decoded.document.sections().len(), 3);
        for i in 0..3 {
            let text =
                decoded.document.sections()[i].paragraphs[0].runs[0].content.as_text().unwrap();
            assert_eq!(text, &format!("Section {i}"));
        }
    }

    // ── 4. Page settings roundtrip ─────────────────────────────

    #[test]
    fn page_settings_roundtrip() {
        let (_, store) = minimal_doc_and_store();

        let custom_ps = PageSettings {
            width: HwpUnit::new(59528).unwrap(),
            height: HwpUnit::new(84188).unwrap(),
            margin_left: HwpUnit::new(8504).unwrap(),
            margin_right: HwpUnit::new(8504).unwrap(),
            margin_top: HwpUnit::new(5668).unwrap(),
            margin_bottom: HwpUnit::new(4252).unwrap(),
            header_margin: HwpUnit::new(4252).unwrap(),
            footer_margin: HwpUnit::new(4252).unwrap(),
            ..PageSettings::a4()
        };

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("Content", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            custom_ps,
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        let decoded_ps = &decoded.document.sections()[0].page_settings;
        assert_eq!(decoded_ps.width.as_i32(), 59528);
        assert_eq!(decoded_ps.height.as_i32(), 84188);
        assert_eq!(decoded_ps.margin_left.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_right.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_top.as_i32(), 5668);
        assert_eq!(decoded_ps.margin_bottom.as_i32(), 4252);
    }

    // ── 5. Table roundtrip ─────────────────────────────────────

    #[test]
    fn table_roundtrip() {
        use hwpforge_core::table::{Table, TableCell, TableRow};

        let (_, store) = minimal_doc_and_store();

        let cell1 = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("A", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(5000).unwrap(),
        );
        let cell2 = TableCell::new(
            vec![Paragraph::with_runs(
                vec![Run::text("B", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            HwpUnit::new(5000).unwrap(),
        );
        let table = Table::new(vec![TableRow::new(vec![cell1, cell2])]);

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        let run = &decoded.document.sections()[0].paragraphs[0].runs[0];
        let t = run.content.as_table().unwrap();
        assert_eq!(t.rows.len(), 1);
        assert_eq!(t.rows[0].cells.len(), 2);
        assert_eq!(t.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(), Some("A"),);
        assert_eq!(t.rows[0].cells[1].paragraphs[0].runs[0].content.as_text(), Some("B"),);
    }

    // ── 6. Rich styles roundtrip ───────────────────────────────

    #[test]
    fn rich_styles_roundtrip() {
        let mut store = HwpxStyleStore::new();
        store.push_font(HwpxFont {
            id: 0, face_name: "함초롬돋움".into(), lang: "HANGUL".into()
        });
        store.push_font(HwpxFont { id: 0, face_name: "Arial".into(), lang: "LATIN".into() });
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef {
                hangul: FontIndex::new(0),
                latin: FontIndex::new(1),
                ..Default::default()
            },
            height: HwpUnit::new(2400).unwrap(),
            text_color: Color::from_rgb(255, 0, 0),
            shade_color: None,
            bold: true,
            italic: true,
            underline_type: UnderlineType::Bottom,
            underline_color: None,
            strikeout_shape: StrikeoutShape::None,
            strikeout_color: None,
            vertical_position: VerticalPosition::Normal,
            outline_type: OutlineType::None,
            shadow_type: ShadowType::None,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
            ..Default::default()
        });
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Justify,
            margin_left: HwpUnit::new(200).unwrap(),
            margin_right: HwpUnit::new(100).unwrap(),
            indent: HwpUnit::new(300).unwrap(),
            spacing_before: HwpUnit::new(150).unwrap(),
            spacing_after: HwpUnit::new(50).unwrap(),
            line_spacing: 200,
            line_spacing_type: LineSpacingType::Percentage,
            ..Default::default()
        });

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("Bold+Italic", CharShapeIndex::new(0)),
                    Run::text("Normal", CharShapeIndex::new(1)),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();

        // Fonts: expanded to 7 language groups (1+1+1×5 = 7)
        assert_eq!(decoded.style_store.font_count(), 7);
        assert_eq!(decoded.style_store.font(FontIndex::new(0)).unwrap().face_name, "함초롬돋움");
        assert_eq!(decoded.style_store.font(FontIndex::new(1)).unwrap().face_name, "Arial");

        // Rich char shape
        let cs = decoded.style_store.char_shape(CharShapeIndex::new(0)).unwrap();
        assert_eq!(cs.height.as_i32(), 2400);
        assert_eq!(cs.text_color, Color::from_rgb(255, 0, 0));
        assert!(cs.bold);
        assert!(cs.italic);
        assert_eq!(cs.underline_type, UnderlineType::Bottom);

        // Para shape
        let ps = decoded.style_store.para_shape(ParaShapeIndex::new(0)).unwrap();
        assert_eq!(ps.alignment, Alignment::Justify);
        assert_eq!(ps.margin_left.as_i32(), 200);
        assert_eq!(ps.line_spacing, 200);
    }

    // ── 7. encode_file roundtrip ───────────────────────────────

    #[test]
    fn encode_file_roundtrip() {
        let (doc, store) = minimal_doc_and_store();

        let dir = std::env::temp_dir().join("hwpforge_test_encode_file");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test_output.hwpx");

        HwpxEncoder::encode_file(&path, &doc, &store, &ImageStore::new()).unwrap();

        // Decode the file
        let decoded = HwpxDecoder::decode_file(&path).unwrap();
        assert_eq!(decoded.document.sections().len(), 1);
        assert_eq!(
            decoded.document.sections()[0].paragraphs[0].runs[0].content.as_text(),
            Some("안녕하세요"),
        );

        // Cleanup
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── 8. encode_file error on bad path ───────────────────────

    #[test]
    fn encode_file_bad_path() {
        let (doc, store) = minimal_doc_and_store();
        let err = HwpxEncoder::encode_file(
            "/nonexistent/dir/test.hwpx",
            &doc,
            &store,
            &ImageStore::new(),
        )
        .unwrap_err();
        assert!(matches!(err, HwpxError::Io(_)));
    }

    // ── 9. Empty style store produces valid output ─────────────

    #[test]
    fn empty_style_store_encode() {
        let store = HwpxStyleStore::new();
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("text", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let validated = doc.validate().unwrap();

        // Should still produce a valid ZIP (no style data, but valid structure)
        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        assert_eq!(&bytes[0..2], b"PK");
    }

    // ── 10. Encoded output is decodable ────────────────────────

    #[test]
    fn encoded_output_is_decodable_by_decoder() {
        let (doc, store) = minimal_doc_and_store();
        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).unwrap();

        // The key test: the decoder accepts encoder output
        let result = HwpxDecoder::decode(&bytes);
        assert!(result.is_ok(), "Decoder failed on encoder output: {:?}", result.err());
    }
}

#[cfg(test)]
mod attr_control_whitespace_tests {
    use super::normalize_attr_control_whitespace as norm;
    use std::borrow::Cow;

    // 이것을 실패시키는 것: 속성 구간에서 `&#9;`·`&#10;`·`&#13;` 중 하나라도 치환을 빼는 것,
    // 또는 텍스트 구간까지 치환하는 것(`t&#13;u` 가 바뀜).
    #[test]
    fn attr_refs_become_spaces_and_text_is_untouched() {
        assert_eq!(
            norm(r#"<a x="1&#9;2&#10;3&#13;4">t&#13;u</a>"#),
            r#"<a x="1 2 3 4">t&#13;u</a>"#
        );
    }

    // 이것을 실패시키는 것: `&#13;&#10;` 쌍 규칙을 빼는 것(공백 둘이 됨).
    #[test]
    fn crlf_pair_folds_to_one_space() {
        assert_eq!(norm(r#"<a x="A&#13;&#10;B"/>"#), r#"<a x="A B"/>"#);
    }

    // 이것을 실패시키는 것: 쌍 규칙을 `&#10;&#13;` 에도 적용하는 것(0.41 에서도 줄 끝 둘).
    #[test]
    fn lf_cr_order_stays_two_spaces() {
        assert_eq!(norm(r#"<a x="A&#10;&#13;B"/>"#), r#"<a x="A  B"/>"#);
    }

    // 이것을 실패시키는 것: `&amp;` 뒤의 `#10;` 를 참조로 보는 매칭(예: `#10;` 만 찾기).
    // 진짜 참조 `&#9;` 를 같이 넣어 앞의 `contains("&#")` 조기 반환을 지나게 한다.
    #[test]
    fn escaped_ampersand_is_not_a_reference() {
        assert_eq!(norm(r#"<a x="&amp;#10;&#9;"/>"#), r#"<a x="&amp;#10; "/>"#);
    }

    // 이것을 실패시키는 것: 따옴표를 만날 때마다 상태를 뒤집는 것(`'` 에서 값이 끝난 것으로 봄).
    #[test]
    fn quote_closes_only_on_its_opener() {
        assert_eq!(norm(r#"<a x="O'Reilly&#10;X"/>"#), r#"<a x="O'Reilly X"/>"#);
        assert_eq!(norm(r#"<a x='a"b&#13;c'/>"#), r#"<a x='a"b c'/>"#);
    }

    // 이것을 실패시키는 것: `&xml[i..]` 처럼 `&str` 을 임의 바이트에서 자르는 것(한글에서 panic).
    #[test]
    fn multibyte_values_do_not_split_chars() {
        assert_eq!(
            norm(r#"<a x="함초롬&#13;바탕" y="가&#13;&#10;나">본&#13;문</a>"#),
            r#"<a x="함초롬 바탕" y="가 나">본&#13;문</a>"#
        );
    }

    // 이것을 실패시키는 것: 주석·CDATA·PI 를 "`<` 부터 첫 `>`" 로 판별하는 것 —
    // 그 안의 `"` 가 따옴표 상태를 열어 뒤따르는 텍스트의 참조까지 바꾼다.
    #[test]
    fn comment_cdata_pi_and_end_tags_are_skipped() {
        for src in [
            r#"<!-- x="a&#13;b" -->t&#13;u"#,
            r#"<![CDATA[x="a&#13;b"]]>t&#13;u"#,
            r#"<?pi x="a&#13;b"?>t&#13;u"#,
            r#"<a></a>t&#13;u"#,
        ] {
            assert!(matches!(norm(src), Cow::Borrowed(s) if s == src), "{src}");
        }
        assert_eq!(
            norm(r#"<!-- " --><a x="A&#9;B"/>"#),
            r#"<!-- " --><a x="A B"/>"#,
            "a quote inside a comment must not open a value"
        );
    }

    // 이것을 실패시키는 것: 시작 태그의 `>` 에서 태그 상태를 닫지 않는 것 — 텍스트의 `"` 가
    // 값을 여는 것으로 읽혀 텍스트의 `&#13;` 이 바뀐다(serde 는 텍스트의 `"` 를 escape 하지 않음).
    #[test]
    fn quotes_in_text_do_not_open_a_value() {
        let src = r#"<a x="1">say "a&#13;b"</a>"#;
        assert!(matches!(norm(src), Cow::Borrowed(s) if s == src));
    }

    // 한컴 판정(VG-1·2·3) 입력을 0.42 serde 가 쓰는 형태 그대로 넣는다. 참조가 남으면 한컴에서
    // 스타일 이름 잘림·글꼴 칸 빈칸·수식 깨짐이 난다.
    // 이것을 실패시키는 것: `&#9;`·`&#10;`·`&#13;` 중 하나의 치환을 빼는 것.
    #[test]
    fn hancom_verdict_inputs_leave_no_references() {
        let cases = [
            (
                r#"<hh:style name="바탕&#10;글" engName="Nor&#9;mal"/>"#,
                r#"<hh:style name="바탕 글" engName="Nor mal"/>"#,
            ),
            (r#"<hh:style name="본&#13;문"/>"#, r#"<hh:style name="본 문"/>"#),
            (r#"<hh:font face="함초롬바탕&#13;"/>"#, r#"<hh:font face="함초롬바탕 "/>"#),
            (r#"<hp:equation font="HancomEQN&#10;"/>"#, r#"<hp:equation font="HancomEQN "/>"#),
        ];
        for (src, want) in cases {
            assert_eq!(norm(src), want);
        }
    }

    // 이것을 실패시키는 것: 바뀐 것이 없어도 `Owned` 로 새로 할당하는 것.
    #[test]
    fn unchanged_input_is_borrowed() {
        for src in ["<a x=\"1\">t</a>", "<a x=\"&amp;\">&#13;</a>", ""] {
            assert!(matches!(norm(src), Cow::Borrowed(_)), "{src}");
        }
    }
}

/// The five serde call sites each normalize their own output: a Core string
/// with tab, LF, CR and CRLF reaches every one of them in one document.
#[cfg(test)]
mod attr_control_whitespace_call_site_tests {
    use super::HwpxEncoder;
    use crate::style_store::{HwpxFont, HwpxStyle, HwpxStyleStore};
    use crate::HwpxDecoder;
    use hwpforge_core::control::{Control, ShapePoint};
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::{HeaderFooter, Section};
    use hwpforge_core::{Document, PageSettings};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use std::io::Read;

    const RAW: &str = "A\tB\nC\rD";
    const RAW_CRLF: &str = "E\r\nF";

    fn equation_with_font(font: &str) -> Control {
        let mut eq = Control::equation("a+b");
        if let Control::Equation { font: f, .. } = &mut eq {
            *f = font.to_string();
        }
        eq
    }

    /// A bookmark name reaches each serde call site as a plain attribute
    /// (font names are cleaned before serde, so they cannot carry the test).
    fn bookmark(name: &str) -> Control {
        Control::bookmark(name)
    }

    fn para(runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(0))
    }

    fn encode_parts() -> (String, String) {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_font(HwpxFont::new(7, format!("글꼴{RAW}"), "HANGUL"));
        store.push_style(HwpxStyle::new(0, "PARA", RAW, RAW_CRLF, 0, 0, 0, 1042, 0));
        let cs = CharShapeIndex::new(0);

        let mut connect = Control::connect_line(ShapePoint::new(0, 0), ShapePoint::new(1000, 500))
            .expect("non-degenerate");
        if let Control::ConnectLine { connect_type, .. } = &mut connect {
            *connect_type = format!("CT{RAW}");
        }
        let plain_line =
            Control::connect_line(ShapePoint::new(0, 500), ShapePoint::new(2000, 1000))
                .expect("non-degenerate");
        let memo = Control::memo_with_anchor(
            vec![para(vec![Run::control(bookmark(&format!("MEMO{RAW}")), cs)])],
            vec![Run::text("앵커", cs)],
        );
        let body = para(vec![
            Run::text("본문", cs),
            Run::control(bookmark(&format!("BODY{RAW}")), cs),
            Run::control(equation_with_font(&format!("EQ{RAW}")), cs),
            // A shape inside a group is written by `shapes.rs` `serialize_with_root`,
            // not by the section serializer.
            Run::control(
                Control::Group {
                    children: vec![connect, plain_line],
                    width: HwpUnit::new(2000).unwrap(),
                    height: HwpUnit::new(1000).unwrap(),
                    placement: None,
                    inst_id: None,
                },
                cs,
            ),
            Run::control(memo, cs),
        ]);
        let mut section = Section::with_paragraphs(vec![body], PageSettings::a4());
        section.headers.push(HeaderFooter::all_pages(vec![para(vec![Run::control(
            bookmark(&format!("HEAD{RAW}")),
            cs,
        )])]));
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("valid document");

        let bytes = HwpxEncoder::encode(&doc, &store, &ImageStore::new()).expect("encode");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        let mut read = |name: &str| {
            let mut s = String::new();
            zip.by_name(name).expect(name).read_to_string(&mut s).expect("utf-8");
            s
        };
        (read("Contents/header.xml"), read("Contents/section0.xml"))
    }

    /// Returns the value of the first `attr="…"` whose value starts with `prefix`.
    fn attr_value<'a>(xml: &'a str, attr: &str, prefix: &str) -> &'a str {
        let needle = format!("{attr}=\"{prefix}");
        let start =
            xml.find(&needle).unwrap_or_else(|| panic!("{needle} not in output")) + attr.len() + 2;
        let end = start + xml[start..].find('"').expect("closing quote");
        &xml[start..end]
    }

    fn assert_no_control_refs_in_start_tags(part: &str, xml: &str) {
        let mut rest = xml;
        while let Some(lt) = rest.find('<') {
            let gt = lt + rest[lt..].find('>').expect("tag end");
            let tag = &rest[lt..=gt];
            for r in ["&#9;", "&#10;", "&#13;"] {
                assert!(!tag.contains(r), "{part}: {r} left in {tag}");
            }
            rest = &rest[gt + 1..];
        }
    }

    // 이것을 실패시키는 것: `encoder/header.rs` 의 정규화 호출을 빼는 것.
    #[test]
    fn header_xml_style_attrs_are_normalized() {
        let (header, _) = encode_parts();
        assert_eq!(attr_value(&header, "name", "A"), "A B C D");
        assert_eq!(attr_value(&header, "engName", "E"), "E F", "CRLF folds to one space");
        assert_no_control_refs_in_start_tags("header.xml", &header);
    }

    // Font names go further than the D3 normalization: control characters are
    // removed (not turned into spaces) and the ends are trimmed, so Hancom can
    // match an installed font. With a space left in, `함초롬바탕 ` leaves the
    // font box empty and `HancomEQN ` breaks the equation (0.41 did the same).
    // 이것을 실패시키는 것: `header.rs`·`equation.rs` 에서 `clean_font_name` 을 빼는 것.
    #[test]
    fn font_names_lose_control_characters_and_edge_spaces() {
        let (header, section) = encode_parts();
        assert_eq!(attr_value(&header, "face", "글꼴"), "글꼴ABCD");
        assert_eq!(attr_value(&section, "font", "EQ"), "EQABCD");
    }

    // 이것을 실패시키는 것: 제어 문자만 지우고 앞뒤를 자르지 않는 것, 또는 가운데 공백까지
    // 지우는 것.
    #[test]
    fn clean_font_name_keeps_inner_spaces() {
        for (raw, want) in [
            ("함초롬바탕\r", "함초롬바탕"),
            ("함초롬\r바탕", "함초롬바탕"),
            ("HancomEQN\n", "HancomEQN"),
            ("  Times New Roman\t ", "Times New Roman"),
            ("\r\n", ""),
            ("함초롬바탕", "함초롬바탕"),
        ] {
            assert_eq!(super::clean_font_name(raw), want, "{raw:?}");
        }
    }

    // 이것을 실패시키는 것: `encoder/section.rs` 의 정규화 호출을 빼는 것.
    #[test]
    fn section_body_bookmark_name_is_normalized() {
        let (_, section) = encode_parts();
        assert_eq!(attr_value(&section, "name", "BODY"), "BODYA B C D");
    }

    // 이것을 실패시키는 것: `encoder/shapes.rs` `serialize_with_root` 의 정규화 호출을 빼는 것.
    #[test]
    fn grouped_connect_line_type_is_normalized() {
        let (_, section) = encode_parts();
        assert_eq!(attr_value(&section, "type", "CT"), "CTA B C D");
    }

    // 이것을 실패시키는 것: `encoder/section/memo.rs` 의 정규화 호출을 빼는 것.
    #[test]
    fn memo_body_bookmark_name_is_normalized() {
        let (_, section) = encode_parts();
        assert_eq!(attr_value(&section, "name", "MEMO"), "MEMOA B C D");
    }

    // 이것을 실패시키는 것: `encoder/section/header_footer.rs` 의 정규화 호출을 빼는 것.
    #[test]
    fn header_footer_bookmark_name_is_normalized() {
        let (_, section) = encode_parts();
        assert_eq!(attr_value(&section, "name", "HEAD"), "HEADA B C D");
        assert_no_control_refs_in_start_tags("section0.xml", &section);
    }

    // 텍스트 `hp:t` 와 수식 `hp:script` 는 0.42 escape(`&#13;`) 그대로 둔다 — 한컴이 0.41 의
    // literal CR 과 같게 다룬다(VG-4·5·6·7). 속성은 decode 하면 공백으로 돌아온다.
    // 이것을 실패시키는 것: `encoder/header.rs` 정규화 호출을 빼는 것(decode 값이 제어 문자가 됨),
    // 또는 정규화를 텍스트 구간까지 넓히는 것(`&#13;` 이 사라짐).
    #[test]
    fn normalized_output_decodes_to_spaces() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_style(HwpxStyle::new(0, "PARA", RAW, RAW_CRLF, 0, 0, 0, 1042, 0));
        let cs = CharShapeIndex::new(0);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![para(vec![
                Run::text("가\r나", cs),
                Run::control(Control::equation("a+b\r\n+c"), cs),
            ])],
            PageSettings::a4(),
        ));
        let bytes =
            HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
        let mut section = String::new();
        zip::ZipArchive::new(std::io::Cursor::new(bytes.clone()))
            .unwrap()
            .by_name("Contents/section0.xml")
            .unwrap()
            .read_to_string(&mut section)
            .unwrap();
        assert!(section.contains("<hp:t>가&#13;나</hp:t>"), "text keeps the 0.42 escape");
        assert!(section.contains("a+b&#13;\n+c</hp:script>"), "script keeps the 0.42 escape");
        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        let style = decoded.style_store.iter_styles().next().expect("one style");
        assert_eq!((style.name.as_str(), style.eng_name.as_str()), ("A B C D", "E F"));
        let text = decoded.document.sections()[0].paragraphs[0].text_content();
        assert_eq!(text, "가\r나", "text keeps its CR (Hancom reads &#13; like 0.41's literal CR)");
    }

    /// quick-xml 0.42 writes a run holding only a CR as `<hp:t>&#13;</hp:t>`;
    /// reading it back must keep the run (0.41 wrote a literal CR, which a
    /// parser reads as LF, so it survived as `"\n"`).
    // 이것을 실패시키는 것: 디코더의 ws-only 판정(`is_xml_whitespace`)을 literal 공백만으로
    // 되돌리는 것 — run 이 경고 없이 사라진다.
    #[test]
    fn a_run_holding_only_a_cr_survives_the_round_trip() {
        let store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        let cs = CharShapeIndex::new(0);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![para(vec![Run::text("가", cs), Run::text("\r", cs), Run::text("나", cs)])],
            PageSettings::a4(),
        ));
        let bytes =
            HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).expect("decode");
        let texts: Vec<String> = decoded.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .filter_map(|r| r.content.plain_text().map(|c| c.into_owned()))
            .collect();
        assert_eq!(texts, vec!["가", "\r", "나"]);
    }

    /// Census attributes that Core strings reach (plan §4.3), other than the
    /// five the call-site tests above already pin.
    // 이것을 실패시키는 것: 이 속성들 중 하나를 serde 가 아닌 경로(정규화를 거치지 않는)로 옮기는 것,
    // 또는 header·section 정규화 호출을 빼는 것.
    #[test]
    fn remaining_census_attrs_are_normalized() {
        use crate::style_store::HwpxParaShape;
        use hwpforge_core::image::Image;

        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_font(HwpxFont::new(7, "글꼴", format!("LANG{RAW}")));
        store.push_style(HwpxStyle::new(0, format!("TY{RAW}"), "s", "s", 0, 0, 0, 1042, 0));
        store
            .push_para_shape(HwpxParaShape { line_wrap: format!("LW{RAW}"), ..Default::default() });
        let cs = CharShapeIndex::new(0);
        let mut compose = Control::compose(format!("CX{RAW}"));
        if let Control::Compose { circle_type, compose_type, .. } = &mut compose {
            *circle_type = format!("CI{RAW}");
            *compose_type = format!("CO{RAW}");
        }
        let body = para(vec![
            Run::control(Control::bookmark(&format!("BM{RAW}")), cs),
            Run::control(compose, cs),
            Run::image(
                Image::from_path(
                    format!("BinData/IM{RAW}.png"),
                    HwpUnit::new(1000).unwrap(),
                    HwpUnit::new(1000).unwrap(),
                ),
                cs,
            ),
        ]);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![body], PageSettings::a4()));
        let mut images = ImageStore::new();
        images.insert(format!("BinData/IM{RAW}.png"), vec![0x89, b'P', b'N', b'G']);
        let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &images).expect("encode");
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        let mut read = |name: &str| {
            let mut s = String::new();
            zip.by_name(name).expect(name).read_to_string(&mut s).expect("utf-8");
            s
        };
        let (header, section) = (read("Contents/header.xml"), read("Contents/section0.xml"));
        assert_eq!(attr_value(&header, "lang", "LANG"), "LANGA B C D");
        assert_eq!(attr_value(&header, "type", "TY"), "TYA B C D");
        assert_eq!(attr_value(&header, "lineWrap", "LW"), "LWA B C D");
        assert_eq!(attr_value(&section, "name", "BM"), "BMA B C D");
        assert_eq!(attr_value(&section, "composeText", "CX"), "CXA B C D");
        assert_eq!(attr_value(&section, "circleType", "CI"), "CIA B C D");
        assert_eq!(attr_value(&section, "composeType", "CO"), "COA B C D");
        assert_eq!(attr_value(&section, "binaryItemIDRef", "IM"), "IMA B C D");
        assert_no_control_refs_in_start_tags("header.xml", &header);
        assert_no_control_refs_in_start_tags("section0.xml", &section);
    }
}
