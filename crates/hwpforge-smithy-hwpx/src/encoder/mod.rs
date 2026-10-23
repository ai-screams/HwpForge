//! HWPX encoder pipeline.
//!
//! Submodules handle individual stages:
//! - `header` — [`HwpxStyleStore`] → `header.xml` serialization
//! - `section` — Core `Section` → `section*.xml` serialization
//! - `package` — ZIP assembly (mimetype, metadata, content files)
//! - `references` — manifest id and image reference checks on the package
//!
//! The public entry point is [`HwpxEncoder`], which orchestrates
//! the full pipeline: header → sections → ZIP packaging.

pub(crate) mod chart;
pub(crate) mod header;
pub(crate) mod header_tabs;
mod origins;
pub use origins::EmittedOrigins;
mod object_ids;
pub(crate) mod package;
mod references;
pub(crate) mod section;
pub(crate) use section::table_default_placement;
pub(crate) mod shapes;

/// Escapes XML special characters for text content and double-quoted
/// attribute values.
///
/// `&`, `<`, `>`, and `"` are encoded as `&amp;`, `&lt;`, `&gt;`, and
/// `&quot;`. Single quotes (`'`) are **not** escaped because all HWPX
/// attribute values produced by this encoder use double-quote delimiters.
/// If a future caller places escaped values inside single-quoted XML
/// attributes, `&apos;` escaping must be added.
///
/// Characters XML 1.0 forbids pass through unchanged. Each part that can
/// hold document strings (header, sections, `content.hpf`, master pages,
/// charts) is scanned once it is assembled
/// ([`strip_xml_forbidden_chars`](crate::wire_xml::strip_xml_forbidden_chars)),
/// and the scan reports what it removes as
/// [`EncodeWarning::XmlForbiddenCharsRemoved`]. Removing them here as well
/// would hide them from that scan, so the caller would get no warning.
/// (Font names are the exception: `clean_font_name` drops every control
/// character before serialization; only `hh:font@face` removals are counted.)
/// The preserve-first edits splice escaped text without a part scan, so
/// they clean the caller's text first and check the new fragment
/// ([`sanitize_edit_text`](crate::wire_xml::sanitize_edit_text)).
pub(crate) fn escape_xml(s: &str) -> String {
    // Single-pass: only allocate when a special character is found.
    let mut result = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => result.push_str("&amp;"),
            '<' => result.push_str("&lt;"),
            '>' => result.push_str("&gt;"),
            '"' => result.push_str("&quot;"),
            _ => result.push(ch),
        }
    }
    result
}

/// Strips Unicode code points that are illegal in XML 1.0 character content.
///
/// Removes:
/// - **C0 control characters** (U+0000 – U+001F) except `\t` (U+0009),
///   `\n` (U+000A), and `\r` (U+000D) — U+0000 is not removed
/// - **Unicode non-characters** U+FFFE / U+FFFF, which are explicitly
///   forbidden by the XML 1.0 Character Range production
///   (`#x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD] | [#x10000-#x10FFFF]`)
/// - **Surrogate code points** U+D800 – U+DFFF cannot occur in a
///   well-formed `&str` (Rust enforces valid UTF-8), so they are not
///   explicitly checked but the documentation calls out the rejection
///   contract.
///
/// Read side only: the `content.hpf` metadata decoder applies it to the
/// values it reads. The encoder does not use it — what it writes is
/// cleaned by the part scan described on [`escape_xml`].
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

    #[test]
    fn leaves_forbidden_characters_to_the_part_scan() {
        // The part scan removes and reports them; a removal here would hide
        // them from it (R1-5).
        let input = "a\u{0}\u{0001}b\u{001F}c\u{FFFE}<\u{FFFF}>";
        assert_eq!(escape_xml(input), "a\u{0}\u{0001}b\u{001F}c\u{FFFE}&lt;\u{FFFF}&gt;");
    }

    #[test]
    fn preserves_tab_lf_cr_during_escape() {
        // Hancom HWPX uses literal newlines inside `<hp:t>` for memo body
        // continuation; escape_xml must NOT strip those.
        assert_eq!(escape_xml("line1\nline2\tindent\rfinal"), "line1\nline2\tindent\rfinal");
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
    /// A source-backed unmatched field is carried without inventing an end.
    UnclosedFieldBoundary {
        /// Original section part.
        part: String,
        /// Core location and recovery reason.
        reason: String,
    },
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
    /// XML 1.0 이 금지하는 문자를 part 에서 지움 — TAB·LF·CR 을 뺀 C0 제어
    /// 문자(U+0000–U+0008, U+000B, U+000C, U+000E–U+001F)와 U+FFFE·U+FFFF.
    /// 이 문자는 문자 참조(`&#1;`)로도 쓸 수 없어, 남기면 part 가
    /// well-formed XML 이 아니게 된다 (#198). 같은 `(part, location)` 의
    /// 제거는 한 경고로 합친다.
    XmlForbiddenCharsRemoved {
        /// 패키지 안 part 경로 (예: `Contents/header.xml`).
        part: String,
        /// 문자가 있던 자리 — 속성은 `요소@속성` (`hh:style@engName`),
        /// 텍스트는 그것을 담은 요소 이름 (`hp:t`).
        location: String,
        /// 지운 문자 수.
        count: usize,
    },
    /// Core 에 있던 내용을 HWPX 로 그대로 쓸 수 없다 — 산출물에서 빠지거나
    /// 정확한 기하를 잃는다. 이 변형 하나가 두 경우를 모두 담고 `kind` 로
    /// 가른다: 도형의 회전·반사를 HWPX 가 저장하는 형태(정수 도 각도, 소수
    /// 6자리 행렬 계수)로 쓰면 정확한 위치에서 2 HWPUNIT 넘게 벗어나는 경우는
    /// `shape_emission`, 인코더가 쓸 형태가 없어 뺀 컨트롤·run 내용은 그
    /// 종류 이름.
    ///
    /// 디코드 쪽 [`crate::DecodeWarning::ContentLost`] 와 같은 이름·코드다.
    /// 산출물이 입력의 내용을 잃었으므로 의미 손상이다
    /// ([`Self::is_semantic_loss`]).
    ContentLost {
        /// 손실이 난 패키지 part 이름 (예: `Contents/section0.xml`).
        part: String,
        /// part 안에서 손실이 난 자리의 중첩 경로.
        path: crate::decoder::ParagraphPath,
        /// 무엇을 잃었는지 — `shape_emission`, 또는 빠진 내용의 종류 이름
        /// (컨트롤이면 `Control::kind_name` (`unknown`, `new_number` …), run
        /// 내용이면 `run_content`).
        kind: String,
        /// 사람이 읽는 사유.
        reason: String,
    },
    /// `content.hpf` manifest 의 한 `id` 를 항목 둘 이상이 함께 쓴다 — 예:
    /// 이미지 저장소 키 `header.png` 의 항목 id 가 고정 항목 `header`
    /// (`Contents/header.xml`)와 같아지거나, `a.png`·`a.jpg` 가 둘 다 `a` 가
    /// 된다. `binaryItemIDRef`·`idref` 가 어느 항목을 가리키는지 정해지지
    /// 않으므로 의미 손상이다. 바이트는 바꾸지 않는다.
    PackageIdConflict {
        /// 겹친 manifest id.
        id: String,
        /// 그 id 를 쓰는 항목들의 `href`, manifest 순서.
        hrefs: Vec<String>,
    },
    /// part 의 `binaryItemIDRef` 가 패키지의 어떤 이미지·OLE 항목
    /// (`content.hpf` 에서 `href` 가 `BinData/` 인 항목)의 id 와도 맞지
    /// 않는다 — 예: `from-json` 을 원본 패키지(`--base`) 없이 써서 이미지
    /// 저장소가 비었다. 그림·채우기가 산출물에서 깨지므로 의미 손상이다.
    /// id 가 맞는 항목이 있어도 그 `href` 가 실제로 쓴 ZIP 항목 이름과 다르면
    /// 같다 — 예: 저장소 키 `a\u{1}.png` 는 ZIP 항목 이름에 U+0001 이 남지만
    /// 금지 문자를 지운 `content.hpf` 의 `href` 는 `BinData/a.png` 라, 그
    /// `href` 를 따라가면 데이터가 없다. ZIP 항목 이름은 고치지 않는다(자산이
    /// 말없이 개명된다). 그 `href` 가 마침 다른 자산의 ZIP 항목 이름이어도
    /// 같다 — 예: `a\u{0}.png` 옆의 `/a.png` 는 `BinData/a.png` 로 쓰여, 그
    /// `href` 는 다른 그림을 가리킨다. 항목마다 제 자산의 항목 이름과 대조한다.
    ///
    /// 참조는 **그대로 남긴다**. 한컴은 이런 문서를 경고 없이 열어 원래
    /// 크기의 빈 그림 상자를 그리고, 다시 저장하면 참조를 빈 값으로 쓴다.
    /// 빈 참조는 이 크레이트의 디코더가 그림째 버리므로, 남겨 두는 쪽이
    /// 그림 자리와 참조 이름(나중에 자산을 붙일 열쇠)을 보존한다. 빈 값은
    /// 참조로 보지 않는다. 같은 `(part, reference)` 는 한 경고로 합친다.
    ///
    /// 이 인코더가 쓰는 패키지만 검사한다 — 원본 패키지에 part 만 바꿔
    /// 쓰는 preserve 편집(`fill`·`patch`·구조 편집)은 새 참조도 manifest
    /// 항목도 만들지 않아 입력에 있던 것을 그대로 물려받는다.
    ImageReferenceUnresolved {
        /// 참조가 있는 part 이름 (예: `Contents/header.xml`).
        part: String,
        /// 참조를 담은 자리 — `요소@binaryItemIDRef` (`hc:img@binaryItemIDRef`).
        /// 합친 경고는 처음 본 자리.
        location: String,
        /// 해석되지 않은 `binaryItemIDRef` 값.
        reference: String,
    },
    /// 개체 둘 이상이 같은 Core id(그림·표·수식·묶음·글맵시의 `inst_id`)를
    /// 갖는다 (W0). 각주·미주는 세지 않는다 — 한컴이 여러 주석을 한 instance
    /// id 로 저장한다. 상호 참조가 어느 개체를 가리키는지 정해지지
    /// 않는다. 인코더는 재번호하지 않고 두 값을 그대로 쓴다 — 어느 쪽이 참조
    /// 대상인지는 인코더가 정할 수 없다. 새로 뽑는 id 는 이 값을 피한다.
    ObjectIdConflict {
        /// 겹친 Core id.
        id: u64,
        /// 그 id 를 가진 개체 종류(`picture`·`table`·…), 문서 순서.
        kinds: Vec<String>,
    },
}

/// manifest 가 실리는 part 이름 — [`EncodeWarning::PackageIdConflict`] 의 자리.
const CONTENT_HPF_PART: &str = "Contents/content.hpf";

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
    /// | [`Self::ContentLost`] | 예 | 도형이 정확한 위치에서 벗어나 쓰이거나, 컨트롤·run 내용이 산출물에서 빠진다 |
    /// | [`Self::PackageIdConflict`] | 예 | 참조가 어느 manifest 항목을 가리키는지 정해지지 않는다 |
    /// | [`Self::ImageReferenceUnresolved`] | 예 | 그림·채우기가 가리키는 자산이 패키지에 없다 |
    /// | [`Self::XmlForbiddenCharsRemoved`] | 아니오 | XML 이 담을 수 없는 문자만 지운다 (#198) |
    /// | [`Self::ObjectIdConflict`] | 아니오 | 입력의 id 를 그대로 쓴다 — 모호함은 입력에 이미 있었고 이 인코드가 잃는 것은 없다 |
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
            | Self::NoteRestartIgnored { .. }
            | Self::ContentLost { .. }
            | Self::PackageIdConflict { .. }
            | Self::ImageReferenceUnresolved { .. } => true,
            Self::UnclosedFieldBoundary { .. }
            | Self::LayoutCacheDropped { .. }
            | Self::XmlForbiddenCharsRemoved { .. }
            | Self::ObjectIdConflict { .. } => false,
        }
    }

    /// 이 경고의 안정 코드.
    ///
    /// 하류(ops·convert·CLI)가 자기 매핑표에 없는 변형을 만나면 이 값을
    /// 그대로 보고한다. 기존 변형의 하류 출력 코드는 각 하류의 표가 정한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath};
    ///
    /// let w = EncodeWarning::LayoutCacheDropped {
    ///     path: ParagraphPath::default(),
    ///     reason: "ledger failed".into(),
    /// };
    /// assert_eq!(w.code(), "LAYOUT_CACHE_DROPPED");
    /// ```
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::UnclosedFieldBoundary { .. } => "UNCLOSED_FIELD_BOUNDARY",
            Self::LayoutCacheDropped { .. } => "LAYOUT_CACHE_DROPPED",
            Self::NoteHeadSkipped { .. } => "NOTE_HEAD_SKIPPED",
            Self::TitleMarkSkipped { .. } => "TITLE_MARK_SKIPPED",
            Self::NoteRestartIgnored { .. } => "NOTE_RESTART_IGNORED",
            Self::XmlForbiddenCharsRemoved { .. } => "XML_FORBIDDEN_CHARS_REMOVED",
            Self::ContentLost { .. } => "CONTENT_LOST",
            Self::PackageIdConflict { .. } => "PACKAGE_ID_CONFLICT",
            Self::ImageReferenceUnresolved { .. } => "IMAGE_REFERENCE_UNRESOLVED",
            Self::ObjectIdConflict { .. } => "OBJECT_ID_CONFLICT",
        }
    }

    /// 경고가 가리키는 패키지 part 이름. part 를 싣지 않는 변형은 `None`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath};
    ///
    /// let removed = EncodeWarning::XmlForbiddenCharsRemoved {
    ///     part: "Contents/header.xml".into(),
    ///     location: "hh:style@engName".into(),
    ///     count: 1,
    /// };
    /// assert_eq!(removed.part(), Some("Contents/header.xml"));
    /// let dropped = EncodeWarning::LayoutCacheDropped {
    ///     path: ParagraphPath::default(),
    ///     reason: "ledger failed".into(),
    /// };
    /// assert_eq!(dropped.part(), None);
    /// ```
    #[must_use]
    pub fn part(&self) -> Option<&str> {
        match self {
            Self::LayoutCacheDropped { .. }
            | Self::NoteHeadSkipped { .. }
            | Self::TitleMarkSkipped { .. }
            | Self::NoteRestartIgnored { .. }
            | Self::ObjectIdConflict { .. } => None,
            Self::UnclosedFieldBoundary { part, .. }
            | Self::XmlForbiddenCharsRemoved { part, .. }
            | Self::ContentLost { part, .. }
            | Self::ImageReferenceUnresolved { part, .. } => Some(part),
            Self::PackageIdConflict { .. } => Some(CONTENT_HPF_PART),
        }
    }

    /// 경고가 가리키는 자리를 사람이 읽는 한 줄로.
    ///
    /// 문단 경로를 싣는 변형은 그 경로, `XmlForbiddenCharsRemoved` 는
    /// `part 요소@속성`, `ContentLost` 는 `part:경로`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath, PathSeg};
    ///
    /// let w = EncodeWarning::NoteHeadSkipped {
    ///     path: ParagraphPath(vec![PathSeg::Section(0), PathSeg::BodyParagraph(2)]),
    ///     reason: "titleMark first run".into(),
    /// };
    /// assert_eq!(w.location().as_deref(), Some("section[0].para[2]"));
    /// ```
    #[must_use]
    pub fn location(&self) -> Option<String> {
        match self {
            Self::LayoutCacheDropped { path, .. }
            | Self::NoteHeadSkipped { path, .. }
            | Self::TitleMarkSkipped { path, .. }
            | Self::NoteRestartIgnored { path, .. } => Some(path.to_string()),
            Self::XmlForbiddenCharsRemoved { part, location, .. }
            | Self::ImageReferenceUnresolved { part, location, .. } => {
                Some(format!("{part} {location}"))
            }
            Self::UnclosedFieldBoundary { part, .. } => Some(part.clone()),
            Self::ContentLost { part, path, .. } => Some(format!("{part}:{path}")),
            Self::PackageIdConflict { .. } => Some(format!("{CONTENT_HPF_PART} opf:item@id")),
            // The holders can sit in different parts; the place is the value.
            Self::ObjectIdConflict { id, .. } => Some(format!("object id/instid \"{id}\"")),
        }
    }

    /// 자리([`Self::location`])를 뺀 사람용 문구.
    ///
    /// 자리를 따로 싣는 하류 표(CLI `to-pdf` 의 `location` 열 등)가 문구에
    /// 자리를 두 번 쓰지 않도록 쓴다. 자리까지 한 줄로 필요하면 `Display`.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::{EncodeWarning, ParagraphPath};
    ///
    /// let w = EncodeWarning::LayoutCacheDropped {
    ///     path: ParagraphPath::default(),
    ///     reason: "ledger failed".into(),
    /// };
    /// assert_eq!(w.message(), "layout cache dropped: ledger failed");
    /// ```
    #[must_use]
    pub fn message(&self) -> String {
        match self {
            Self::UnclosedFieldBoundary { reason, .. } => {
                format!("unclosed source field: {reason}")
            }
            Self::LayoutCacheDropped { reason, .. } => format!("layout cache dropped: {reason}"),
            Self::NoteHeadSkipped { reason, .. } => format!("note number head skipped: {reason}"),
            Self::TitleMarkSkipped { reason, .. } => format!("titleMark skipped: {reason}"),
            Self::NoteRestartIgnored { reason, .. } => format!("note restart ignored: {reason}"),
            Self::XmlForbiddenCharsRemoved { count, .. } => {
                format!("removed {count} XML-forbidden character(s)")
            }
            Self::ContentLost { kind, reason, .. } => format!("{kind} lost: {reason}"),
            Self::PackageIdConflict { id, hrefs } => {
                format!("{} manifest items share id \"{id}\": {}", hrefs.len(), hrefs.join(", "))
            }
            Self::ImageReferenceUnresolved { reference, .. } => {
                format!("binaryItemIDRef \"{reference}\" does not resolve to image or OLE data in the package")
            }
            Self::ObjectIdConflict { id, kinds } => {
                format!("{} objects share id {id}: {}", kinds.len(), kinds.join(", "))
            }
        }
    }
}

/// One [`EncodeWarning::XmlForbiddenCharsRemoved`] per `(location, count)`
/// that [`strip_xml_forbidden_chars`](crate::wire_xml::strip_xml_forbidden_chars)
/// reported for `part`.
pub(crate) fn forbidden_char_warnings(
    part: &str,
    sites: Vec<(String, usize)>,
) -> impl Iterator<Item = EncodeWarning> + '_ {
    sites.into_iter().map(move |(location, count)| EncodeWarning::XmlForbiddenCharsRemoved {
        part: part.to_string(),
        location,
        count,
    })
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
            Self::XmlForbiddenCharsRemoved { part, location, count } => {
                write!(f, "removed {count} XML-forbidden character(s) from {location} in {part}")
            }
            Self::ContentLost { part, path, kind, reason } => {
                write!(f, "{kind} lost at {part}:{path}: {reason}")
            }
            Self::PackageIdConflict { id, hrefs } => write!(
                f,
                "{} manifest items share id \"{id}\" in {CONTENT_HPF_PART}: {}",
                hrefs.len(),
                hrefs.join(", ")
            ),
            Self::ImageReferenceUnresolved { part, location, reference } => write!(
                f,
                "binaryItemIDRef \"{reference}\" at {location} in {part} does not resolve to \
                 image or OLE data in the package"
            ),
            Self::UnclosedFieldBoundary { part, reason } => {
                write!(f, "unclosed source field in {part}: {reason}")
            }
            Self::ObjectIdConflict { .. } => f.write_str(&self.message()),
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

    /// 이것을 실패시키는 것: `is_semantic_loss` 에서 `ContentLost` 를 `false`
    /// 갈래로 옮기는 것 — 편집기가 도형이 어긋난 바이트를 내보낸다.
    #[test]
    fn content_lost_is_semantic_loss() {
        let warning = EncodeWarning::ContentLost {
            part: "Contents/section0.xml".into(),
            path: path(),
            kind: "shape_emission".into(),
            reason: "off by 35.08".into(),
        };
        assert!(warning.is_semantic_loss(), "{warning:?}");
        assert_eq!(
            warning.to_string(),
            format!("shape_emission lost at Contents/section0.xml:{}: off by 35.08", path())
        );
    }

    /// W0: a shared Core id is not a semantic loss — the encoder writes the
    /// input's ids unchanged, so an editor does not refuse the document.
    /// 이것을 실패시키는 것: `ObjectIdConflict` 를 `true` 갈래로 옮기기.
    #[test]
    fn object_id_conflict_is_not_semantic_loss() {
        let warning = EncodeWarning::ObjectIdConflict {
            id: 7,
            kinds: vec!["table".into(), "picture".into()],
        };
        assert!(!warning.is_semantic_loss());
        assert_eq!(warning.message(), "2 objects share id 7: table, picture");
    }

    /// 이것을 실패시키는 것: 표본 목록에서 한 변형을 빼는 것 — 고유 코드 수가
    /// 갈래 수와 어긋난다.
    #[test]
    fn samples_cover_every_code_once() {
        let samples = crate::decoder::warning_samples::encode();
        let codes: std::collections::BTreeSet<&str> =
            samples.iter().map(EncodeWarning::code).collect();
        assert_eq!(codes.len(), samples.len(), "one sample per code");
        assert_eq!(
            codes.into_iter().collect::<Vec<_>>(),
            [
                "CONTENT_LOST",
                "IMAGE_REFERENCE_UNRESOLVED",
                "LAYOUT_CACHE_DROPPED",
                "NOTE_HEAD_SKIPPED",
                "NOTE_RESTART_IGNORED",
                "OBJECT_ID_CONFLICT",
                "PACKAGE_ID_CONFLICT",
                "TITLE_MARK_SKIPPED",
                "UNCLOSED_FIELD_BOUNDARY",
                "XML_FORBIDDEN_CHARS_REMOVED",
            ]
        );
    }

    /// 이것을 실패시키는 것: `message` 가 자리를 다시 넣는 것 (자리 없는 문구가
    /// 깨진다).
    #[test]
    fn message_leaves_the_location_out() {
        for w in crate::decoder::warning_samples::encode() {
            let loc = w.location().unwrap();
            assert!(!w.message().contains(&loc), "{w:?}: {}", w.message());
        }
    }

    /// 이것을 실패시키는 것: `is_semantic_loss` 에서 `ContentLost` 를 `false`
    /// 갈래로 옮기는 것 — 편집기가 빠진 내용을 담은 바이트를 내보낸다.
    #[test]
    fn content_lost_for_left_out_content_is_semantic_loss() {
        let warning = EncodeWarning::ContentLost {
            part: "Contents/section0.xml".into(),
            path: path(),
            kind: "unknown".into(),
            reason: "no HWPX form".into(),
        };
        assert!(warning.is_semantic_loss(), "{warning:?}");
        assert_eq!(warning.code(), "CONTENT_LOST");
        assert_eq!(warning.part(), Some("Contents/section0.xml"));
        assert_eq!(warning.location().as_deref(), Some("Contents/section0.xml:section[0].para[3]"));
        assert_eq!(warning.message(), "unknown lost: no HWPX form");
        assert_eq!(
            warning.to_string(),
            "unknown lost at Contents/section0.xml:section[0].para[3]: no HWPX form"
        );
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
                    EncodeWarning::UnclosedFieldBoundary { reason, .. }
                    | EncodeWarning::LayoutCacheDropped { reason, .. }
                    | EncodeWarning::NoteHeadSkipped { reason, .. }
                    | EncodeWarning::TitleMarkSkipped { reason, .. }
                    | EncodeWarning::NoteRestartIgnored { reason, .. }
                    | EncodeWarning::ContentLost { reason, .. } => reason.clone(),
                    EncodeWarning::XmlForbiddenCharsRemoved { location, .. } => location.clone(),
                    EncodeWarning::PackageIdConflict { id, .. } => id.clone(),
                    EncodeWarning::ImageReferenceUnresolved { reference, .. } => reference.clone(),
                    EncodeWarning::ObjectIdConflict { id, .. } => id.to_string(),
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
    /// HWP5→HWPX 변환(기본 carry)과 PDF 재생 파이프라인이 켠다. 편집 표면은
    /// 켜지 않는다 — 편집한 문단의 캐시를 무검증 방출하면 `layout_carry` 의
    /// "이미 있으면 스킵" 안전장치가 fail-open 이 된다. 영-core 토큰 사이에서
    /// 시작하는 줄은 줄 경계([`hwpforge_core::layout::LineBoundary`])로 그
    /// 토큰 앞에 놓고, 정확히 놓을 수 없거나 줄 시작이 늘지 않는 문단은
    /// 캐시를 버린다(경고) — 접힌 `[0,0]` 캐시는 쓰지 않는다(S22).
    ///
    /// 좌표 주의: HWPX 디코더가 승격한 캐시의 `textpos` 는 **보이는-텍스트
    /// 좌표로 정규화**돼 있다 (선행 컨트롤 8유닛 차감) — 방출값도 원본
    /// wire 의 스트림 좌표와 다르다.
    pub emit_layout_cache: bool,
    /// 작성 시각(`CreateDateTime`)이 비어 있는 메모에 쓸 시각, Unix 초.
    /// `None`(기본)이면 epoch(`1970-01-01T00:00:00Z`). 인코더는 시계를 읽지
    /// 않는다 — 같은 문서는 늘 같은 바이트(W0). 사용자가 새로 쓰는 문서의
    /// 진입점(`from-json` 등)이 현재 시각을 넣는다.
    pub memo_create_time: Option<u64>,
    /// Crate-internal (the carry's cache source, `layout_carry`): with
    /// `emit_layout_cache`, a paragraph whose cache cannot be placed gets a
    /// marker cache — one line at `textpos` [`DROPPED_CACHE_MARK`] whose
    /// `vertpos` is an ordinal within its section — so the carry can find
    /// that paragraph; the warning that says why is returned beside the
    /// output under that ordinal
    /// ([`HwpxEncoder::encode_reporting_dropped_caches`]).
    pub(crate) mark_dropped_caches: bool,
}

/// `textpos` of the marker line [`EncodeOptions::mark_dropped_caches`]
/// writes — no real line starts there.
pub(crate) const DROPPED_CACHE_MARK: u32 = u32::MAX;

impl EncodeOptions {
    /// 캐시 방출 여부를 설정한다 (기본 `false`).
    #[must_use]
    pub fn with_emit_layout_cache(mut self, emit: bool) -> Self {
        self.emit_layout_cache = emit;
        self
    }

    /// 작성 시각이 빈 메모에 쓸 시각(Unix 초)을 정한다.
    ///
    /// # Examples
    ///
    /// ```
    /// use hwpforge_smithy_hwpx::EncodeOptions;
    ///
    /// let options = EncodeOptions::default().with_memo_create_time(1_700_000_000);
    /// assert_eq!(options.memo_create_time, Some(1_700_000_000));
    /// ```
    #[must_use]
    pub fn with_memo_create_time(mut self, unix_secs: u64) -> Self {
        self.memo_create_time = Some(unix_secs);
        self
    }
}

/// Review r1 HIGH-2: a table cell zone must name a border fill the package
/// writes (looked up by id) — JSON or Markdown can author any number, and
/// the package would point at nothing. Every written table is checked
/// ([`Document::for_each_table`] — picture captions too, review r2 HIGH).
/// 이것을 실패시키는 것: 이 검사 삭제(`unknown_cell_zone_fill_is_refused`).
fn refuse_unknown_cell_zone_fills(
    document: &Document<Validated>,
    style_store: &HwpxStyleStore,
) -> HwpxResult<()> {
    let written =
        |id: u32| crate::header_references::HeaderReferences::border_fill(style_store, id).is_ok();
    // Tables in a memo's anchor runs are not written (the anchor keeps its
    // text only, `build_memo_anchor_xml`), so they are not checked (review r3
    // M3). 이것을 실패시키는 것: 앵커 표 제외 삭제(`unknown_cell_zone_fill_is_refused`
    // 의 메모 앵커 안 999 성공).
    let mut dropped: std::collections::HashSet<*const hwpforge_core::table::Table> =
        std::collections::HashSet::new();
    for section in document.sections() {
        let tops = section
            .paragraphs
            .iter()
            .chain(section.headers.iter().chain(section.footers.iter()).flat_map(|h| &h.paragraphs))
            .chain(section.master_pages.iter().flatten().flat_map(|m| &m.paragraphs));
        for p in tops {
            memo_anchor_tables(p, &mut dropped);
        }
    }
    let mut missing = None;
    document.for_each_table(|table| {
        if dropped.contains(&std::ptr::from_ref(table)) {
            return;
        }
        if let Some(zone) = table.cell_zones.iter().find(|z| !written(z.border_fill_id)) {
            missing.get_or_insert(zone.border_fill_id);
        }
    });
    match missing {
        Some(id) => Err(crate::error::HwpxError::InvalidStructure {
            detail: format!("table cell zone border fill {id} is not in the style store"),
        }),
        None => Ok(()),
    }
}

/// The tables inside memo anchor runs reachable from `p` (run picture
/// captions entered too) — content the HWPX encoder does not write.
fn memo_anchor_tables(
    p: &hwpforge_core::paragraph::Paragraph,
    out: &mut std::collections::HashSet<*const hwpforge_core::table::Table>,
) {
    use hwpforge_core::control::Control;
    use hwpforge_core::run::RunContent;
    p.for_each_paragraph(|q| {
        for run in &q.runs {
            match &run.content {
                RunContent::Control(c) => {
                    if let Control::Memo { anchor_runs, .. } = c.as_ref() {
                        for anchor in anchor_runs {
                            anchor.for_each_table(|t| {
                                out.insert(std::ptr::from_ref(t));
                            });
                        }
                    }
                }
                RunContent::Image(img) => {
                    for cp in img.caption.iter().flat_map(|c| &c.paragraphs) {
                        memo_anchor_tables(cp, out);
                    }
                }
                _ => {}
            }
        }
    });
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
/// (e.g. `"BinData/image1.png"`) are matched against the store keys by
/// stem (`image1`), the id the manifest gives each image. A picture or fill
/// whose image is not in the store keeps its XML reference, with no binary
/// data, and [`Self::encode_with_diagnostics`] reports it as
/// [`EncodeWarning::ImageReferenceUnresolved`].
#[derive(Debug, Clone, Copy)]
pub struct HwpxEncoder;

impl HwpxEncoder {
    /// Encodes an edited Core document, retaining trusted base line caches
    /// for unchanged paragraphs outside an edit's flow tail and host chain.
    ///
    /// `base` is the original HWPX package. Paragraph content, referenced
    /// styles and container/page settings are compared in emitted XML. Lists with
    /// changed lengths retain only their identical prefix; paragraph ids are
    /// not used as identities. Caches supplied in `document` are ignored:
    /// only decoded base caches may pass through the encoder's token ledger.
    /// `options.emit_layout_cache` is overridden by this selective policy.
    /// Images come from `image_store`, as in [`Self::encode_with_diagnostics`].
    /// When the complete typed document (except caches), styles and images are
    /// unchanged, returns the original package byte for byte. Original values
    /// absent from Core are then retained along with the caches; cache validity
    /// is not re-certified. Regenerated output always uses the layout proof.
    ///
    /// The outcome distinguishes intentional edit invalidation from missing
    /// base correspondence and ledger failures on otherwise unchanged caches.
    /// Base decoder content losses are also reported, including containers
    /// whose paragraphs never reached Core.
    ///
    /// # Errors
    ///
    /// Returns an error if the base cannot be decoded, the prepared Core
    /// document cannot be validated, or encoding/package I/O fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use hwpforge_smithy_hwpx::{EncodeOptions, HwpxDecoder, HwpxEncoder};
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let base = std::fs::read("original.hwpx")?;
    /// let decoded = HwpxDecoder::decode(&base)?;
    /// let edited = decoded.document.validate()?;
    /// let result = HwpxEncoder::encode_with_base(
    ///     &base, &edited, &decoded.style_store, &decoded.image_store,
    ///     EncodeOptions::default(),
    /// )?;
    /// std::fs::write("edited.hwpx", result.bytes)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn encode_with_base(
        base: &[u8],
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<EncodeOutcome> {
        let base = crate::decoder::HwpxEditBase::read(base)?;
        Self::encode_with_edit_base(&base, document, style_store, image_store, options)
    }

    /// Encode against an immutable base already decoded for this operation.
    /// Source claims are reverified against original bytes at output, including
    /// when the complete original package can be returned unchanged.
    ///
    /// # Errors
    /// Returns provenance, layout-analysis and encoding errors as
    /// [`Self::encode_with_base`] does.
    pub fn encode_with_edit_base(
        base: &crate::decoder::HwpxEditBase,
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<EncodeOutcome> {
        let decoded = base.decoded();
        let base = base.bytes();
        crate::HwpxDecoder::verify_source_provenance(document.sections(), decoded)?;
        if crate::layout_edit::unchanged(decoded, document, style_store, image_store)? {
            // 이것을 실패시키는 것: Core만 비교하고 스타일/이미지/메타데이터를
            // 무시하거나 이 분기에서 새로 인코딩한 XML에 원본 캐시를 붙이기.
            // No token positions or layout inputs move: even unsupported XML
            // and its dependencies remain byte-identical. Caller caches are
            // never read. memo_create_time only supplies times to newly written
            // memos; this branch writes no memo (or any other package entry).
            // Validate ZIP limits also for entries the Core decoder ignores.
            crate::patch::RawPackage::read(base)?;
            let warnings = decoded
                .warnings
                .iter()
                .cloned()
                .filter_map(|warning| match warning {
                    crate::DecodeWarning::ContentLost { part, path, kind, reason } => {
                        Some(EncodeWarning::ContentLost {
                            part,
                            path,
                            kind,
                            reason: format!(
                                "base decode: {reason}; original package bytes retained"
                            ),
                        })
                    }
                    crate::DecodeWarning::UnclosedFieldBoundary { part, reason } => {
                        Some(EncodeWarning::UnclosedFieldBoundary { part, reason })
                    }
                    _ => None,
                })
                .collect();
            return Ok(EncodeOutcome { bytes: base.to_vec(), warnings });
        }
        let (prepared, mut warnings, output) = crate::layout_edit::prepare(
            base,
            decoded,
            document,
            style_store,
            image_store,
            options,
        )?;
        warnings.extend(decoded.warnings.iter().cloned().filter_map(|warning| match warning {
            crate::DecodeWarning::LayoutCacheDropped { path, reason } => {
                Some(EncodeWarning::LayoutCacheDropped {
                    path,
                    reason: format!("base decode: {reason}"),
                })
            }
            // A container the base decoder cannot carry may own paragraphs
            // absent from Core altogether. Keep that provenance loss visible.
            crate::DecodeWarning::ContentLost { part, path, kind, reason } => {
                Some(EncodeWarning::ContentLost {
                    part,
                    path,
                    kind,
                    reason: format!("base decode: {reason}"),
                })
            }
            _ => None,
        }));
        let source = crate::layout_carry::cache_source_with_options(
            &prepared,
            style_store,
            image_store,
            options,
        )?;
        let mut outcome = crate::layout_carry::carry_prepared(&output.bytes, source)?;
        outcome.bytes = crate::patch::RawPackage::preserve_opaque(base, outcome.bytes)?;
        outcome.warnings.splice(0..0, warnings);
        Ok(outcome)
    }

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
        Self::encode_reporting_dropped_caches(document, style_store, image_store, options)
            .map(|(outcome, _)| outcome)
    }

    /// Encode actual output bytes and record the Core origin of each emitted
    /// paragraph. Use these origins with `layout_edit::PackageAnalysis` and
    /// `ParagraphCorrespondence`; synthetic paragraphs have no trusted origin.
    pub fn encode_with_origins(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<(EncodeOutcome, EmittedOrigins)> {
        let scope = origins::Scope::enter();
        let outcome = Self::encode_with_diagnostics(document, style_store, image_store, options)?;
        Ok((outcome, scope.finish()))
    }

    /// [`Self::encode_with_diagnostics`], plus, for
    /// [`EncodeOptions::mark_dropped_caches`], the warning of each marked
    /// cache by section index and marker ordinal.
    pub(crate) fn encode_reporting_dropped_caches(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<(EncodeOutcome, Vec<Vec<EncodeWarning>>)> {
        crate::HwpxDecoder::verify_document_source(document)?;
        refuse_unknown_cell_zone_fills(document, style_store)?;
        // W0: ids drawn for objects without a Core id come from this encode's
        // own allocator, which skips every Core id. 이것을 실패시키는 것: 스코프를
        // 열지 않기(전 인코드 panic) · 예약 생략(`drawn_ids_skip_core_ids`).
        let references = crate::header_references::HeaderReferences::build(style_store)?;
        references.validate(style_store, true)?;
        let border_ids = 1..=style_store.border_fill_count() as u32;
        crate::header_references::HeaderReferences::validate_body_borders(document, border_ids)?;
        let mut context = section::DocumentEncodeContext::for_document(document, image_store);
        context.references = Some(references);
        // Core already follows the decoder's manifest-resolved spine order.
        // Number output parts from that order; never reconstruct original filenames.
        let sections = document.sections();
        let sec_cnt = sections.len() as u32;

        context.warnings =
            hwpforge_core::field_inventory::FieldInventory::from_sections(document.sections())
                .warnings()
                .iter()
                .filter_map(|w| {
                    if let hwpforge_core::ValidationError::InvalidFieldBoundary {
                        section_index,
                        ..
                    } = w
                    {
                        Some(EncodeWarning::UnclosedFieldBoundary {
                            part: format!("Contents/section{section_index}.xml"),
                            reason: w.to_string(),
                        })
                    } else {
                        None
                    }
                })
                .collect();
        let section_warning_start = context.warnings.len();
        // All parts share one document asset/diagnostic context.
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
            let mut result = encode_section_with_note_counters(
                section,
                i,
                options,
                &mut note_counters,
                &mut context,
            )?;
            origins::finish_part(&format!("Contents/section{i}.xml"), &mut result.xml)?;
            for (part, xml) in &mut result.master_pages {
                origins::finish_part(part, xml)?;
            }
            section_results.push(result);
        }
        // The single output-supplementation boundary: header references, body
        // borders/zones, and every emitted body ID have now been resolved using
        // the ORIGINAL tables, with only the font-only new-document char/para 0
        // exception in HeaderReferences::output_ids. Keep that resolver in context;
        // generated definitions cannot validate any other explicit reference.
        // 이것을 실패시키는 것: 보충을 원본 검증 앞으로 이동(fix6_empty_definition_matrix).
        let mut generated;
        let style_store = if style_store.char_shape_count() == 0
            || style_store.para_shape_count() == 0
            || style_store.font_count() == 0
        {
            generated = style_store.clone();
            if generated.char_shape_count() == 0 {
                // Font references are group-local positions; the first is 0,
                // regardless of the public definition's decorative id label.
                let shape = crate::HwpxCharShape::default();
                generated.push_char_shape(shape);
            }
            if generated.para_shape_count() == 0 {
                generated.push_para_shape(crate::HwpxParaShape::default());
            }
            if generated.font_count() == 0 {
                generated.push_font(crate::HwpxFont::new(0, "함초롬바탕", "HANGUL"));
            }
            &generated
        } else {
            style_store
        };
        // Absent authoring values and the narrowly scoped new-document char/para 0
        // references request defaults. All other explicit IDs were checked above.
        let mut needed = std::collections::BTreeSet::new();
        if document.sections().iter().any(|s| s.page_border_fills.is_none()) {
            needed.insert(1);
        }
        if style_store.iter_char_shapes().any(|c| c.border_fill_id.is_none()) {
            needed.insert(2);
        }
        document.for_each_table(|table| {
            if table.border_fill_id.is_none() {
                needed.insert(3);
            }
        });
        needed.retain(|id| *id as usize > style_store.border_fill_count());
        let mut generated_borders;
        let style_store = if !needed.is_empty() {
            generated_borders = style_store.clone();
            // Fill every intervening position, never emit sparse default labels.
            let last = *needed.last().expect("nonempty needed borders");
            for id in style_store.border_fill_count() as u32 + 1..=last {
                let mut fill = if id == 3 {
                    crate::style_store::HwpxBorderFill::default_table_border()
                } else if id == 2 {
                    crate::style_store::HwpxBorderFill::default_char_background()
                } else {
                    crate::style_store::HwpxBorderFill::default_page_border()
                };
                fill.id = id;
                generated_borders.push_border_fill(fill);
            }
            &generated_borders
        } else {
            style_store
        };
        // Only now build the output header, including missing font groups.
        let begin_num = sections.first().and_then(|s| s.begin_num.as_ref());
        let header_xml = encode_header(style_store, sec_cnt, begin_num)?;
        // #198: a font face loses its forbidden characters (with the other
        // controls) to `clean_font_name` before serialization, so the part
        // scan below cannot see them; they are counted here instead.
        let mut header_sites = Vec::new();
        let face_removed = style_store
            .iter_fonts()
            .map(|f| {
                f.face_name.chars().filter(|&c| crate::wire_xml::is_xml_forbidden_char(c)).count()
            })
            .sum();
        crate::wire_xml::tally_removed(&mut header_sites, "hh:font@face", face_removed);
        let (header_xml, scanned) = crate::wire_xml::strip_xml_forbidden_chars(&header_xml);
        for (location, count) in scanned {
            crate::wire_xml::tally_removed(&mut header_sites, &location, count);
        }
        // Preserve the established header-before-section diagnostic order even
        // though original body references must be checked before header output.
        // 이것을 실패시키는 것: extend로 되돌림(each_removal_is_reported_with_its_part_and_location).
        context.warnings.splice(
            section_warning_start..section_warning_start,
            forbidden_char_warnings("Contents/header.xml", header_sites),
        );

        let dropped_caches: Vec<Vec<EncodeWarning>> =
            section_results.iter_mut().map(|r| std::mem::take(&mut r.dropped_caches)).collect();
        // W0: two written objects with one Core id keep it, and are reported
        // — counted from the ids the encoder actually wrote (review W0 r3).
        // 이것을 실패시키는 것: 이 경고 삭제(`shared_core_id_is_kept_and_reported`).
        for (id, kinds) in object_ids::conflicts(&object_ids::written()) {
            context.warnings.push(EncodeWarning::ObjectIdConflict {
                id,
                kinds: kinds.into_iter().map(str::to_string).collect(),
            });
        }

        let (section_xmls, master_pages): (Vec<_>, Vec<_>) =
            section_results.into_iter().map(|r| (r.xml, r.master_pages)).unzip();
        let master_pages: Vec<_> = master_pages.into_iter().flatten().collect();
        let charts = std::mem::take(&mut context.charts);
        let embedded_oles = std::mem::take(&mut context.embedded_oles);
        let images = std::mem::take(&mut context.images);

        // Step 4: Package into ZIP with images, charts, master pages, and
        // embedded-chart OLE blobs. Document.metadata flows into content.hpf
        // <opf:metadata> (Wave 12o Phase 1).
        let manifest = package::manifest_items(
            section_xmls.len(),
            &images.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
            &master_pages.iter().map(|(path, _)| path.clone()).collect::<Vec<_>>(),
            &embedded_oles.iter().map(|(id, _)| id.clone()).collect::<Vec<_>>(),
        );
        // C4: the manifest the writer is about to write, and the parts it
        // writes, checked as they are (the manifest read back from its text).
        // #198: `content.hpf` is scanned for characters XML 1.0 forbids
        // before it is read back, so the checks read the text that is written.
        let rendered = package::render_content_hpf(document.metadata(), &manifest);
        let (content_hpf, sites) = crate::wire_xml::strip_xml_forbidden_chars(&rendered);
        context.warnings.extend(forbidden_char_warnings("Contents/content.hpf", sites));
        let written = references::read_manifest(&content_hpf)?;
        context.warnings.extend(references::manifest_id_conflicts(&written));
        let mut parts: Vec<(String, &str)> = vec![("Contents/header.xml".to_string(), &header_xml)];
        parts.extend(
            section_xmls
                .iter()
                .enumerate()
                .map(|(i, xml)| (format!("Contents/section{i}.xml"), xml.as_str())),
        );
        parts.extend(master_pages.iter().map(|(path, xml)| (path.clone(), xml.as_str())));
        let parts: Vec<(&str, &str)> = parts.iter().map(|(n, x)| (n.as_str(), *x)).collect();
        // A binary item resolves only when its `href` names the entry written
        // for its own asset: a key with a character XML forbids keeps it in
        // the entry name, while the scanned `content.hpf` writes the `href`
        // without it. Each written item is compared with the entry of the
        // manifest item it was rendered from (one `<opf:item>` per item, in
        // order) — not with the set of all entries, where another asset may
        // own that name (`a\u{0}.png` beside `a.png`). Non-binary
        // items drop out: `unresolved_references` looks only at `BinData/`.
        // Embedded-chart OLE ids are the encoder's own `ole{n}`, which the
        // part-name validation accepts, so `image_entry_name` of the
        // `{id}.ole` path is `ole_entry_name(id)`.
        let resolvable: Vec<(String, String)> = manifest
            .iter()
            .zip(&written)
            .filter(|(item, (_, href))| {
                item.href
                    .strip_prefix("BinData/")
                    .is_some_and(|path| *href == package::image_entry_name(path))
            })
            .map(|(_, written_item)| written_item.clone())
            .collect();
        context.warnings.extend(references::unresolved_references(&resolvable, &parts)?);
        let (bytes, package_warnings) = PackageWriter::write_hwpx_with_content_hpf(
            &content_hpf,
            &header_xml,
            &section_xmls,
            &images,
            &charts,
            &master_pages,
            &embedded_oles,
        )?;
        context.warnings.extend(package_warnings);
        Ok((EncodeOutcome { bytes, warnings: context.warnings }, dropped_caches))
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
        Alignment, CharShadow, CharShapeIndex, Color, EmbossType, EngraveType, FontIndex, HwpUnit,
        LineSpacingType, OutlineType, ParaShapeIndex, StrikeoutShape, UnderlineType,
        VerticalPosition,
    };

    use crate::style_store::{HwpxCharShape, HwpxFont, HwpxFontRef, HwpxParaShape};

    #[test]
    fn fix6_empty_definition_sentinel_matrix() {
        // 이것을 실패시키는 것: 출력 기본 정의를 원본 검사 전에 보충하기,
        // 종류별 lookup 생략, 새 문서 char/para 0 예외 제거·확대, 내장/sentinel 거부, 시험 행 누락.
        use hwpforge_core::section::PageBorderFillEntry;
        use hwpforge_core::table::{CellZone, Table, TableCell, TableRow};
        use hwpforge_foundation::{BorderFillIndex, HeadingType, StyleIndex};
        const LANGS: [&str; 7] =
            ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
        // Explicit allowlists are field-specific: paragraph/zone border 0 has
        // no no-border contract; char/table/cell/page 0 does. MAX is not a tab ID.
        let rows: [(&str, &[u32]); 20] = [
            ("HANGUL", &[]),
            ("LATIN", &[]),
            ("HANJA", &[]),
            ("JAPANESE", &[]),
            ("OTHER", &[]),
            ("SYMBOL", &[]),
            ("USER", &[]),
            ("border-char", &[0]),
            ("border-para", &[]),
            ("border-table", &[0]),
            ("border-cell", &[0]),
            ("border-zone", &[]),
            ("border-page", &[0]),
            ("charPr", &[0]),
            ("paraPr", &[0]),
            ("tabPr", &[0, 1, 2]),
            ("numbering", &[0]),
            ("outline", &[0]),
            ("bullet", &[0]),
            ("style", &[]),
        ];
        let covered: std::collections::BTreeSet<_> = rows.iter().map(|r| r.0).collect();
        assert_eq!(
            covered,
            [
                "HANGUL",
                "LATIN",
                "HANJA",
                "JAPANESE",
                "OTHER",
                "SYMBOL",
                "USER",
                "border-char",
                "border-para",
                "border-table",
                "border-cell",
                "border-zone",
                "border-page",
                "charPr",
                "paraPr",
                "tabPr",
                "numbering",
                "outline",
                "bullet",
                "style"
            ]
            .into_iter()
            .collect()
        );
        for (kind, allowed) in rows {
            for id in [0, 1, 2, 3, u32::MAX] {
                let mut store = HwpxStyleStore::new();
                // All other tables are valid. Multiple source fonts expose the
                // old missing-language clone bug for 0, 1, 2 and 3 alike.
                for lang in LANGS.into_iter().filter(|lang| *lang != kind) {
                    for font_id in 0..=3 {
                        store.push_font(HwpxFont::new(font_id, "바탕", lang));
                    }
                }
                let mut c = HwpxCharShape::default();
                if let Some(axis) = LANGS.iter().position(|lang| *lang == kind) {
                    let f = &mut c.font_ref;
                    let axes = [
                        &mut f.hangul,
                        &mut f.latin,
                        &mut f.hanja,
                        &mut f.japanese,
                        &mut f.other,
                        &mut f.symbol,
                        &mut f.user,
                    ];
                    *axes.into_iter().nth(axis).unwrap() = FontIndex::new(id as usize);
                }
                if kind == "border-char" {
                    c.border_fill_id = Some(id);
                }
                if kind != "charPr" {
                    store.push_char_shape(c);
                }
                let mut p = HwpxParaShape::default();
                match kind {
                    "border-para" => p.border_fill_id = Some(BorderFillIndex::new(id as usize)),
                    "tabPr" => p.tab_pr_id_ref = id,
                    "numbering" | "outline" | "bullet" => {
                        p.heading_type = match kind {
                            "numbering" => HeadingType::Number,
                            "outline" => HeadingType::Outline,
                            _ => HeadingType::Bullet,
                        };
                        p.heading_id_ref = id;
                    }
                    _ => {}
                }
                if kind != "paraPr" {
                    store.push_para_shape(p);
                }
                let cs = CharShapeIndex::new(if kind == "charPr" { id as usize } else { 0 });
                let ps = ParaShapeIndex::new(if kind == "paraPr" { id as usize } else { 0 });
                let text = Paragraph::with_runs(vec![Run::text("x", cs)], ps);
                let mut cell = TableCell::new(vec![text], HwpUnit::new(1000).unwrap());
                if kind == "border-cell" {
                    cell.border_fill_id = Some(id);
                }
                let mut table = Table::new(vec![TableRow::new(vec![cell])]);
                if kind == "border-table" {
                    table.border_fill_id = Some(id);
                }
                if kind == "border-zone" {
                    // The r5 counterexample must not generate table border 3.
                    table.border_fill_id = Some(0);
                    table.cell_zones = vec![CellZone::new(0, 0, 0, 0, id)];
                }
                let mut paragraph = Paragraph::with_runs(vec![Run::table(table, cs)], ps);
                if kind == "style" {
                    paragraph.style_id = Some(StyleIndex::new(id as usize));
                }
                let mut section = Section::with_paragraphs(vec![paragraph], PageSettings::a4());
                if kind == "border-page" {
                    section.page_border_fills = Some(vec![PageBorderFillEntry {
                        border_fill_id: id,
                        ..Default::default()
                    }]);
                }
                let mut doc = Document::new();
                doc.add_section(section);
                let result =
                    HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new());
                assert_eq!(result.is_ok(), allowed.contains(&id), "{kind}/{id}: {result:?}");
                if let Err(error) = result {
                    let expected = if kind == "border-zone" {
                        "cell zone"
                    } else if kind.starts_with("border-") {
                        "borderFill"
                    } else if LANGS.contains(&kind) {
                        kind
                    } else if kind == "outline" {
                        "numbering"
                    } else {
                        kind
                    };
                    assert!(error.to_string().contains(expected), "{kind}/{id}: {error}");
                }
            }
        }
        // paraHead has a wire-ID inheritance sentinel even with no charPr
        // definitions. Test both numbering and bullet owners independently.
        for owner in ["numbering", "bullet"] {
            for id in [0, 1, 2, 3, u32::MAX] {
                let mut store = HwpxStyleStore::new();
                let mut head = crate::style_store::fidelity::HeadMetrics::generated(0);
                head.char_pr_id_ref = id;
                let map = if owner == "numbering" {
                    &mut store.header_fidelity.numbering
                } else {
                    &mut store.header_fidelity.bullet
                };
                map.insert(1, [(0, head)].into_iter().collect());
                let refs = crate::header_references::HeaderReferences::build(&store).unwrap();
                for output in [false, true] {
                    assert_eq!(
                        refs.validate(&store, output).is_ok(),
                        id == u32::MAX,
                        "{owner}/{id}/{output}"
                    );
                }
            }
        }
    }

    // 이것을 실패시키는 것: 60% 줄 간격의 빈 첫 문단에 0번 글자 모양 carrier를 합성하기.
    #[test]
    fn empty_section_carrier_keeps_character_shape_at_sixty_percent_spacing() {
        use hwpforge_core::control::{Control, NewNumberKind};
        let mut store = HwpxStyleStore::with_default_fonts("바탕");
        for _ in 0..9 {
            store.push_char_shape(HwpxCharShape::default());
        }
        store.push_para_shape(HwpxParaShape { line_spacing: 60, ..Default::default() });
        let mut document = Document::new();
        document.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(
                    Control::NewNumber { kind: NewNumberKind::Page, number: 1 },
                    CharShapeIndex::new(8),
                )],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let bytes =
            HwpxEncoder::encode(&document.validate().unwrap(), &store, &ImageStore::new()).unwrap();
        let mut reader = crate::PackageReader::new(&bytes).unwrap();
        let xml = reader.read_section_xml(0).unwrap();
        assert!(xml.contains(r#"<hp:run charPrIDRef="8"><hp:secPr"#), "{xml}");
        let header = reader.read_header_xml().unwrap();
        assert!(header.contains(r#"type="PERCENT" value="60""#), "{header}");
    }

    #[test]
    fn fix5_memo_anchor_uses_fallible_output_ids() {
        // 이것을 실패시키는 것: 메모 앵커의 내부 0을 출력 0으로 쓰기,
        // 없는 인덱스까지 출력해 성공시키기. 희소 단독 ID와 충돌하는 유효 ID 모두 대조.
        use hwpforge_core::Control;
        for with_black in [false, true] {
            for anchor_index in [0, 99] {
                let mut store = HwpxStyleStore::with_default_fonts("바탕");
                store.push_char_shape(HwpxCharShape {
                    text_color: Color::from_rgb(255, 0, 0),
                    ..Default::default()
                });
                if with_black {
                    store.push_char_shape(HwpxCharShape { ..Default::default() });
                }
                store.push_para_shape(HwpxParaShape::default());
                let cs = CharShapeIndex::new(0);
                let ps = ParaShapeIndex::new(0);
                let memo = Control::memo_with_anchor(
                    vec![Paragraph::with_runs(vec![Run::text("body", cs)], ps)],
                    vec![Run::text("anchor", CharShapeIndex::new(anchor_index))],
                );
                let mut document = Document::new();
                document.add_section(Section::with_paragraphs(
                    vec![Paragraph::with_runs(vec![Run::control(memo, cs)], ps)],
                    PageSettings::a4(),
                ));
                let result =
                    HwpxEncoder::encode(&document.validate().unwrap(), &store, &ImageStore::new());
                if anchor_index == 99 {
                    assert!(result.unwrap_err().to_string().contains("charPr"));
                    continue;
                }
                let bytes = result.unwrap();
                let xml = crate::PackageReader::new(&bytes).unwrap().read_section_xml(0).unwrap();
                assert!(
                    xml.contains(r#"<hp:run charPrIDRef="0"><hp:t>anchor</hp:t></hp:run>"#),
                    "{xml}"
                );
                let decoded = HwpxDecoder::decode(&bytes).unwrap();
                let control = decoded.document.sections()[0].paragraphs[0]
                    .runs
                    .iter()
                    .find_map(|r| match r.content.as_control() {
                        Some(Control::Memo { anchor_runs, .. }) => Some(anchor_runs),
                        _ => None,
                    })
                    .expect("folded memo");
                assert_eq!(
                    decoded.style_store.char_shape(control[0].char_shape_id).unwrap().text_color,
                    Color::from_rgb(255, 0, 0)
                );
            }
        }
    }

    #[test]
    fn fix6_empty_definition_matrix() {
        // 이것을 실패시키는 것: 각 정의 조회 생략/출력 기본 정의 선보충, char/para 0 예외 제거, 행 누락.
        // tabPr 0·1·2는 정의 없이도 유효한 내장 값이다. 일반 tabPr 3과 별도로 확인한다.
        use hwpforge_core::{BulletDef, NumberingDef, TabDef, Table, TableCell, TableRow};
        use hwpforge_foundation::{BorderFillIndex, HeadingType, StyleIndex};
        use std::collections::BTreeSet;
        let cases = [
            ("font:HANGUL", 0),
            ("font:LATIN", 0),
            ("font:HANJA", 0),
            ("font:JAPANESE", 0),
            ("font:OTHER", 0),
            ("font:SYMBOL", 0),
            ("font:USER", 0),
            ("border:char", 2),
            ("border:para", 1),
            ("border:table", 3),
            ("border:cell", 3),
            ("border:zone", 3),
            ("border:page", 1),
            ("charPr", 0),
            ("paraPr", 0),
            ("style", 0),
            ("tabPr", 3),
            ("numbering", 1),
            ("bullet", 1),
        ];
        // Independent inventory: removing/duplicating any case must fail, including
        // one language or one border host (not merely the header definition kind).
        let required: BTreeSet<_> = [
            "font:HANGUL",
            "font:LATIN",
            "font:HANJA",
            "font:JAPANESE",
            "font:OTHER",
            "font:SYMBOL",
            "font:USER",
            "border:char",
            "border:para",
            "border:table",
            "border:cell",
            "border:zone",
            "border:page",
            "charPr",
            "paraPr",
            "style",
            "tabPr",
            "numbering",
            "bullet",
        ]
        .into_iter()
        .collect();
        assert_eq!(cases.len(), required.len());
        assert_eq!(cases.iter().map(|(kind, _)| *kind).collect::<BTreeSet<_>>(), required);
        let mut failures = Vec::new();
        for (kind, id) in cases {
            for defined in [false, true] {
                let mut store = HwpxStyleStore::new();
                for lang in ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
                    if defined || kind.strip_prefix("font:") != Some(lang) {
                        store.push_font(crate::HwpxFont::new(0, "바탕", lang));
                    }
                }
                let mut char_shape = HwpxCharShape::default();
                let mut para_shape = HwpxParaShape::default();
                match kind {
                    "border:char" => char_shape.border_fill_id = Some(id),
                    "border:para" => {
                        para_shape.border_fill_id = Some(BorderFillIndex::new(id as usize))
                    }
                    "tabPr" => para_shape.tab_pr_id_ref = id,
                    "numbering" | "bullet" => {
                        para_shape.heading_type = if kind == "numbering" {
                            HeadingType::Number
                        } else {
                            HeadingType::Bullet
                        };
                        para_shape.heading_id_ref = id;
                    }
                    _ => {}
                }
                if defined || kind != "charPr" {
                    store.push_char_shape(char_shape);
                }
                if defined || kind != "paraPr" {
                    store.push_para_shape(para_shape);
                }
                if defined {
                    if kind.starts_with("border:") {
                        let mut fill = crate::style_store::HwpxBorderFill::default_page_border();
                        fill.id = id;
                        for _ in 0..id {
                            store.push_border_fill(fill.clone());
                        }
                    }
                    match kind {
                        "style" => {
                            store.push_style(crate::HwpxStyle::new(
                                0, "PARA", "base", "base", 0, 0, 0, 1042, 0,
                            ));
                        }
                        "tabPr" => {
                            for tab in TabDef::defaults() {
                                store.push_tab(tab);
                            }
                            store.push_tab(TabDef {
                                id,
                                auto_tab_left: false,
                                auto_tab_right: false,
                                stops: vec![],
                            });
                        }
                        "numbering" => {
                            store.push_numbering(NumberingDef::default_outline());
                        }
                        "bullet" => {
                            store.push_bullet(BulletDef {
                                id,
                                bullet_char: "•".into(),
                                checked_char: None,
                                use_image: false,
                                para_head: NumberingDef::default_outline().levels.remove(0),
                            });
                        }
                        _ => {}
                    }
                }
                let cs = CharShapeIndex::new(0);
                let ps = ParaShapeIndex::new(0);
                let mut cell = TableCell::new(
                    vec![Paragraph::with_runs(vec![Run::text("x", cs)], ps)],
                    HwpUnit::new(1000).unwrap(),
                );
                if kind == "border:cell" {
                    cell.border_fill_id = Some(id);
                }
                let mut table = Table::new(vec![TableRow::new(vec![cell])]);
                if kind == "border:table" {
                    table.border_fill_id = Some(id);
                }
                if kind == "border:zone" {
                    table.cell_zones.push(hwpforge_core::table::CellZone::new(0, 0, 0, 0, id));
                }
                let mut paragraph = Paragraph::with_runs(vec![Run::table(table, cs)], ps);
                if kind == "style" {
                    paragraph.style_id = Some(StyleIndex::new(0));
                }
                let mut section = Section::with_paragraphs(vec![paragraph], PageSettings::a4());
                if kind == "border:page" {
                    section.page_border_fills =
                        Some(vec![hwpforge_core::section::PageBorderFillEntry {
                            border_fill_id: id,
                            ..Default::default()
                        }]);
                }
                let mut doc = Document::new();
                doc.add_section(section);
                let output =
                    HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new());
                let expected_kind = if kind.starts_with("font:") {
                    "font"
                } else if kind == "border:zone" {
                    "cell zone"
                } else if kind.starts_with("border:") {
                    "borderFill"
                } else {
                    kind
                };
                match (defined || matches!(kind, "charPr" | "paraPr"), output) {
                    (true, Ok(bytes)) => {
                        HwpxDecoder::decode(&bytes).unwrap();
                    }
                    (false, Err(error)) if error.to_string().contains(expected_kind) => {}
                    (_, result) => failures
                        .push(format!("{kind}/{id} defined={defined}: {:?}", result.map(|_| ()))),
                }
            }
        }
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        // 이것을 실패시키는 것: tabPr 내장 ID를 일반 참조처럼 거부.
        for id in [0, 1, 2] {
            let mut store = HwpxStyleStore::with_default_fonts("바탕");
            store.push_char_shape(HwpxCharShape::default());
            store.push_para_shape(HwpxParaShape { tab_pr_id_ref: id, ..Default::default() });
            let refs = crate::header_references::HeaderReferences::build(&store).unwrap();
            refs.validate(&store, true).unwrap();
        }
    }

    #[test]
    fn fix6_empty_latin_fonts_cannot_be_repaired_by_output_cloning() {
        // 이것을 실패시키는 것: validate(output=true)에 복제 글꼴 합치기 또는 font 조회 생략.
        for id in [1, 2, 3] {
            for latin_ids in [vec![], vec![0], vec![id]] {
                let mut store = HwpxStyleStore::new();
                for lang in ["HANGUL", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
                    for font_id in 0..=3 {
                        store.push_font(crate::HwpxFont::new(font_id, "바탕", lang));
                    }
                }
                for &font_id in &latin_ids {
                    store.push_font(crate::HwpxFont::new(font_id, "Latin", "LATIN"));
                }
                let mut shape = HwpxCharShape::default();
                shape.font_ref.latin = hwpforge_foundation::FontIndex::new(id as usize);
                store.push_char_shape(shape);
                store.push_para_shape(HwpxParaShape::default());
                let mut doc = Document::new();
                doc.add_section(Section::with_paragraphs(
                    vec![Paragraph::with_runs(
                        vec![Run::text("x", CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    PageSettings::a4(),
                ));
                let refs = crate::header_references::HeaderReferences::build(&store).unwrap();
                assert_eq!(refs.validate(&store, true).is_ok(), (id as usize) < latin_ids.len());
                let output =
                    HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new());
                if (id as usize) < latin_ids.len() {
                    let decoded = HwpxDecoder::decode(&output.unwrap()).unwrap();
                    assert_eq!(
                        decoded.style_store.iter_char_shapes().next().unwrap().font_ref.latin.get(),
                        id as usize
                    );
                } else {
                    assert!(output.unwrap_err().to_string().contains("font LATIN"));
                }
            }
        }
    }

    #[test]
    fn fix5_empty_border_table_rejects_explicit_default_ids() {
        // 이것을 실패시키는 것: 원본 빈 표를 생성 정의 1·2·3으로 대체해 명시 참조를 통과시킴.
        use hwpforge_core::section::PageBorderFillEntry;
        use hwpforge_core::{Table, TableCell, TableRow};
        for kind in ["table", "cell", "page", "char", "para"] {
            for id in [1, 2, 3] {
                let mut store = HwpxStyleStore::with_default_fonts("바탕");
                let mut char_shape = HwpxCharShape::default();
                let mut para_shape = HwpxParaShape::default();
                if kind == "char" {
                    char_shape.border_fill_id = Some(id);
                }
                if kind == "para" {
                    para_shape.border_fill_id =
                        Some(hwpforge_foundation::BorderFillIndex::new(id as usize));
                }
                store.push_char_shape(char_shape);
                store.push_para_shape(para_shape);
                let cs = CharShapeIndex::new(0);
                let ps = ParaShapeIndex::new(0);
                let text = Paragraph::with_runs(vec![Run::text("x", cs)], ps);
                let mut cell = TableCell::new(vec![text], HwpUnit::new(1000).unwrap());
                if kind == "cell" {
                    cell.border_fill_id = Some(id);
                }
                let mut table = Table::new(vec![TableRow::new(vec![cell])]);
                if kind == "table" {
                    table.border_fill_id = Some(id);
                }
                let mut section = Section::with_paragraphs(
                    vec![Paragraph::with_runs(vec![Run::table(table, cs)], ps)],
                    PageSettings::a4(),
                );
                if kind == "page" {
                    section.page_border_fills = Some(vec![PageBorderFillEntry {
                        border_fill_id: id,
                        ..Default::default()
                    }]);
                }
                let mut doc = Document::new();
                doc.add_section(section);
                let error =
                    HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new())
                        .unwrap_err();
                assert!(error.to_string().contains("borderFill"), "{kind}/{id}: {error}");
            }
        }
    }

    #[test]
    fn fix5_only_absent_values_generate_border_defaults() {
        // 이것을 실패시키는 것: 빈 표의 None 기본 정의 생성 누락 또는 불필요한 3 생성.
        for table_needed in [false, true] {
            let mut store = HwpxStyleStore::with_default_fonts("바탕");
            store.push_char_shape(HwpxCharShape::default());
            store.push_para_shape(HwpxParaShape::default());
            let cs = CharShapeIndex::new(0);
            let ps = ParaShapeIndex::new(0);
            let text = Paragraph::with_runs(vec![Run::text("x", cs)], ps);
            let paragraph = if table_needed {
                Paragraph::with_runs(
                    vec![Run::table(
                        hwpforge_core::Table::new(vec![hwpforge_core::TableRow::new(vec![
                            hwpforge_core::TableCell::new(vec![text], HwpUnit::new(1000).unwrap()),
                        ])]),
                        cs,
                    )],
                    ps,
                )
            } else {
                text
            };
            let mut doc = Document::new();
            doc.add_section(Section::with_paragraphs(vec![paragraph], PageSettings::a4()));
            let bytes =
                HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let ids: Vec<_> = decoded.style_store.iter_border_fills().map(|f| f.id).collect();
            assert_eq!(ids, if table_needed { vec![1, 2, 3] } else { vec![1, 2] });
        }
    }

    #[test]
    fn fix4_nested_body_borders_and_generated_defaults() {
        // 이것을 실패시키는 것: encoder 본문 검증 삭제, 중첩 컨테이너 순회 누락,
        // 원본 오류를 출력 기본값으로 수복, sparse store의 생성 기본 정의 누락.
        use hwpforge_core::section::{HeaderFooter, MasterPage, PageBorderFillEntry};
        use hwpforge_core::{Control, Table, TableCell, TableRow};
        use hwpforge_foundation::ApplyPageType;
        let cs = CharShapeIndex::new(0);
        let ps = ParaShapeIndex::new(0);
        let size = HwpUnit::new(1000).unwrap();
        let text = || Paragraph::with_runs(vec![Run::text("x", cs)], ps);
        for site in ["body", "nested", "header", "footer", "master", "textbox", "memo", "caption"] {
            for (kind, id) in [
                ("table", 99),
                ("cell", 99),
                ("page", 99),
                ("table", 1),
                ("table", 0),
                ("cell", 0),
                ("page", 0),
            ] {
                let mut store = HwpxStyleStore::with_default_fonts("바탕");
                store.push_char_shape(HwpxCharShape::default());
                store.push_para_shape(HwpxParaShape::default());
                let mut fill = crate::style_store::HwpxBorderFill::default_page_border();
                fill.id = 7;
                store.push_border_fill(fill);
                let mut cell = TableCell::new(vec![text()], size);
                if kind == "cell" {
                    cell.border_fill_id = Some(id);
                }
                let mut table = Table::new(vec![TableRow::new(vec![cell])]);
                if kind == "table" {
                    table.border_fill_id = Some(id);
                }
                let mut payload = Paragraph::with_runs(vec![Run::table(table, cs)], ps);
                payload = match site {
                    "nested" => Paragraph::with_runs(
                        vec![Run::table(
                            Table::new(vec![TableRow::new(vec![TableCell::new(
                                vec![payload],
                                size,
                            )])]),
                            cs,
                        )],
                        ps,
                    ),
                    "textbox" => Paragraph::with_runs(
                        vec![Run::control(Control::text_box(vec![payload], size, size), cs)],
                        ps,
                    ),
                    "memo" => Paragraph::with_runs(
                        vec![Run::control(
                            Control::memo_with_anchor(vec![payload], vec![Run::text("anchor", cs)]),
                            cs,
                        )],
                        ps,
                    ),
                    "caption" => {
                        let mut image = hwpforge_core::image::Image::new(
                            "BinData/a.png",
                            size,
                            size,
                            hwpforge_core::image::ImageFormat::Png,
                        );
                        image.caption = Some(hwpforge_core::caption::Caption::new(
                            vec![payload],
                            hwpforge_core::caption::CaptionSide::Bottom,
                        ));
                        Paragraph::with_runs(vec![Run::image(image, cs)], ps)
                    }
                    _ => payload,
                };
                let mut section = Section::with_paragraphs(vec![text()], PageSettings::a4());
                match site {
                    "header" => {
                        section.headers.push(HeaderFooter::new(vec![payload], ApplyPageType::Both))
                    }
                    "footer" => {
                        section.footers.push(HeaderFooter::new(vec![payload], ApplyPageType::Both))
                    }
                    "master" => {
                        section.master_pages =
                            Some(vec![MasterPage::new(ApplyPageType::Both, vec![payload])])
                    }
                    _ => section.paragraphs.push(payload),
                }
                if kind == "page" {
                    section.page_border_fills = Some(vec![PageBorderFillEntry {
                        border_fill_id: id,
                        ..Default::default()
                    }]);
                }
                let mut document = Document::new();
                document.add_section(section);
                let mut images = ImageStore::new();
                images.insert("a.png", vec![0x89, b'P', b'N', b'G']);
                let result = HwpxEncoder::encode(&document.validate().unwrap(), &store, &images);
                if id == 99 {
                    assert!(result.unwrap_err().to_string().contains(kind), "{site}/{kind}");
                } else {
                    let decoded = HwpxDecoder::decode(&result.unwrap()).unwrap();
                    if kind != "page" {
                        assert!(decoded.style_store.border_fill(1).is_ok());
                    }
                    assert!(decoded.style_store.border_fill(2).is_ok());
                    assert!(decoded.style_store.border_fill(1).is_ok());
                }
            }
        }
    }

    /// Review r1 HIGH-2: encoding refuses a cell zone whose border fill the
    /// store does not have; a known one encodes.
    /// 이것을 실패시키는 것: 사전 검사 삭제.
    #[test]
    fn unknown_cell_zone_fill_is_refused() {
        use hwpforge_core::caption::{Caption, CaptionSide};
        use hwpforge_core::image::{Image, ImageFormat};
        use hwpforge_core::table::{CellZone, Table, TableCell, TableRow};
        let encode = |with_7: bool, in_picture_caption: bool, fill: u32, table_fill| {
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(HwpxCharShape::default());
            store.push_para_shape(HwpxParaShape::default());
            if with_7 {
                let mut bf = crate::style_store::HwpxBorderFill::default_page_border();
                bf.id = 7;
                store.push_border_fill(bf);
            }
            let text = || {
                Paragraph::with_runs(
                    vec![Run::text("x", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )
            };
            let mut table = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![text()],
                HwpUnit::new(1000).unwrap(),
            )])])
            .with_cell_zones(vec![CellZone::new(0, 0, 0, 0, fill)]);
            table.border_fill_id = table_fill;
            let run = if in_picture_caption {
                let mut image = Image::new(
                    "BinData/a.png",
                    HwpUnit::new(1000).unwrap(),
                    HwpUnit::new(1000).unwrap(),
                    ImageFormat::Png,
                );
                image.caption = Some(Caption::new(
                    vec![Paragraph::with_runs(
                        vec![Run::table(table, CharShapeIndex::new(0))],
                        ParaShapeIndex::new(0),
                    )],
                    CaptionSide::Bottom,
                ));
                Run::image(image, CharShapeIndex::new(0))
            } else {
                Run::table(table, CharShapeIndex::new(0))
            };
            let mut doc = Document::new();
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))],
                PageSettings::a4(),
            ));
            let mut images = ImageStore::new();
            images.insert("a.png", vec![0x89, b'P', b'N', b'G']);
            HwpxEncoder::encode(&doc.validate().unwrap(), &store, &images)
        };
        let refused = |r: HwpxResult<Vec<u8>>| matches!(r, Err(crate::error::HwpxError::InvalidStructure { ref detail }) if detail.contains("cell zone"));
        assert!(encode(true, false, 1, None).is_ok());
        assert!(refused(encode(true, false, 999, None)));
        // Review r2 HIGH: a table in a picture caption is checked too.
        assert!(encode(true, true, 1, None).is_ok());
        assert!(refused(encode(true, true, 999, None)));
        // 이것을 실패시키는 것: 빈 표 zone 1·2·3 허용 또는 표 Some(0)일 때 zone 검사 생략.
        for id in [1, 2, 3] {
            assert!(refused(encode(false, false, id, None)));
        }
        assert!(refused(encode(false, false, 3, Some(0))));
        assert!(refused(encode(false, false, 4, None)));
        // Review r3 M3: a table in a memo anchor is not written, so its zone
        // is not checked; the same table in the memo body is.
        let memo = |in_anchor: bool| {
            let table = Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![Paragraph::with_runs(
                    vec![Run::text("x", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                HwpUnit::new(1000).unwrap(),
            )])])
            .with_cell_zones(vec![CellZone::new(0, 0, 0, 0, 999)]);
            let holder = Run::table(table, CharShapeIndex::new(0));
            let text = || Run::text("a", CharShapeIndex::new(0));
            let (content, anchor) = if in_anchor {
                // The reviewed case: the table sits in a text box in the anchor.
                let text_box = hwpforge_core::control::Control::text_box(
                    vec![Paragraph::with_runs(vec![holder], ParaShapeIndex::new(0))],
                    HwpUnit::new(1000).unwrap(),
                    HwpUnit::new(1000).unwrap(),
                );
                let boxed = Run::control(text_box, CharShapeIndex::new(0));
                (
                    vec![Paragraph::with_runs(vec![text()], ParaShapeIndex::new(0))],
                    vec![text(), boxed],
                )
            } else {
                (vec![Paragraph::with_runs(vec![holder], ParaShapeIndex::new(0))], vec![text()])
            };
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(HwpxCharShape::default());
            store.push_para_shape(HwpxParaShape::default());
            let mut doc = Document::new();
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::control(
                        hwpforge_core::control::Control::memo_with_anchor(content, anchor),
                        CharShapeIndex::new(0),
                    )],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            ));
            HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new())
        };
        assert!(memo(true).is_ok());
        assert!(refused(memo(false)));
    }

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
        // 이것을 실패시키는 것: 합성 fixture에서 LATIN 등 원본 언어별 글꼴 누락.
        let mut store = HwpxStyleStore::with_default_fonts("함초롬돋움");
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
            shadow: CharShadow::DEFAULT,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
            ..Default::default()
        });
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Left,
            margin_left: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO),
            margin_right: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO),
            indent: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO),
            spacing_before: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO),
            spacing_after: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::ZERO),
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

    // 이것을 실패시키는 것: `ImageStore.images` 를 `HashMap` 으로 되돌리는 것
    // (아래 ZIP·manifest 순서 assert 가 실패한다: 12개 key 가 정렬 순서로 나올 확률은 1/12!.
    //  같은 프로세스 안에서는 같은 맵이 같은 순서를 내므로 `first == second` 는 이 변이를 잡지 못하고,
    //  실행 간 재현성에 대한 보조 확인일 뿐이다)
    #[test]
    fn bindata_order_in_manifest_and_zip_follows_image_key_order() {
        use std::io::Read;
        let (doc, store) = minimal_doc_and_store();
        let mut images = ImageStore::new();
        for i in (1..=12).rev() {
            images.insert(format!("image{i}.png"), vec![i as u8; 4]);
        }
        let mut expected: Vec<String> = (1..=12).map(|i| format!("image{i}.png")).collect();
        expected.sort();

        let first = HwpxEncoder::encode(&doc, &store, &images).unwrap();
        let second = HwpxEncoder::encode(&doc, &store, &images).unwrap();
        assert_eq!(first, second, "same input must give identical bytes");

        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(first)).unwrap();
        let zip_order: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().expect("ZIP entry name").to_string())
            .filter(|n| n.starts_with("BinData/"))
            .collect();
        let want: Vec<String> = expected.iter().map(|k| format!("BinData/{k}")).collect();
        assert_eq!(zip_order, want, "ZIP BinData entries must follow key order");

        let mut hpf = String::new();
        zip.by_name("Contents/content.hpf").unwrap().read_to_string(&mut hpf).unwrap();
        let manifest_order: Vec<&str> = hpf
            .split("href=\"")
            .skip(1)
            .filter_map(|r| r.split('"').next())
            .filter(|h| h.starts_with("BinData/"))
            .collect();
        assert_eq!(manifest_order, want.iter().map(String::as_str).collect::<Vec<_>>());
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

        // All seven original language groups are preserved.
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
            id: 0,
            face_name: "함초롬돋움".into(),
            lang: "HANGUL".into(),
            font_type: None,
            substitute: None,
            type_info: None,
        });
        store.push_font(HwpxFont {
            id: 0,
            face_name: "Arial".into(),
            lang: "LATIN".into(),
            font_type: None,
            substitute: None,
            type_info: None,
        });
        // 이것을 실패시키는 것: 명시 참조하는 나머지 5언어의 원본 글꼴 누락.
        for lang in ["HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"] {
            store.push_font(HwpxFont::new(0, "함초롬돋움", lang));
        }
        store.push_char_shape(HwpxCharShape {
            font_ref: HwpxFontRef {
                hangul: FontIndex::new(0),
                latin: FontIndex::new(0),
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
            shadow: CharShadow::DEFAULT,
            emboss_type: EmbossType::None,
            engrave_type: EngraveType::None,
            ..Default::default()
        });
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape {
            alignment: Alignment::Justify,
            margin_left: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(200).unwrap()),
            margin_right: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(100).unwrap()),
            indent: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(300).unwrap()),
            spacing_before: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(150).unwrap()),
            spacing_after: hwpforge_core::ParagraphMeasure::HwpUnit(HwpUnit::new(50).unwrap()),
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

        // All seven original language groups are preserved.
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
        assert_eq!(ps.margin_left.as_hwp_unit().unwrap().as_i32(), 200);
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

    // ── 9. Empty style store keeps the new-document defaults ─

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

        // 이것을 실패시키는 것: 새 문서의 빈 char/para 표 + 0 예외 제거.
        let bytes = HwpxEncoder::encode(&validated, &store, &ImageStore::new()).unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(decoded.document.sections()[0].paragraphs[0].text_content(), "text");
    }

    #[test]
    fn quickstart_font_only_document_roundtrips() {
        // 이것을 실패시키는 것: 빈 charPr 또는 paraPr의 0 예외 제거.
        let style_store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        assert_eq!(style_store.char_shape_count(), 0);
        assert_eq!(style_store.para_shape_count(), 0);
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("안녕하세요, HwpForge!", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &style_store, &ImageStore::new())
            .unwrap();
        let decoded = HwpxDecoder::decode(&bytes).unwrap();
        let paragraph = &decoded.document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.text_content(), "안녕하세요, HwpForge!");
        assert_eq!(paragraph.para_shape_id.get(), 0);
        assert_eq!(paragraph.runs[0].char_shape_id.get(), 0);
        assert_eq!(decoded.style_store.char_shape_count(), 1);
        assert_eq!(decoded.style_store.para_shape_count(), 1);
    }

    #[test]
    fn synthetic_char_fonts_reference_output_definitions() {
        // 이것을 실패시키는 것: 합성 fontRef를 0으로 고정, 언어별 최소 ID 대신
        // 첫 ID 선택, 누락 언어에 첫 그룹 대신 전체 글꼴의 최소 ID 선택.
        const LANGS: [&str; 7] =
            ["HANGUL", "LATIN", "HANJA", "JAPANESE", "OTHER", "SYMBOL", "USER"];
        let all_seven: Vec<_> = LANGS.iter().map(|&lang| (lang, 7)).collect();
        let distinct: Vec<_> = LANGS
            .iter()
            .enumerate()
            .flat_map(|(i, &lang)| [(lang, 19 + i as u32), (lang, 7 + i as u32)])
            .collect();
        let missing = vec![("LATIN", 19), ("LATIN", 7), ("HANGUL", 2)];
        for fonts in [all_seven, distinct, missing, vec![]] {
            let mut store = HwpxStyleStore::new();
            for (lang, id) in fonts {
                store.push_font(HwpxFont::new(id, "바탕", lang));
            }
            assert_eq!(store.char_shape_count(), 0);
            assert_eq!(store.para_shape_count(), 0);
            let mut doc = Document::new();
            doc.add_section(Section::with_paragraphs(
                vec![Paragraph::with_runs(
                    vec![Run::text("새 문서", CharShapeIndex::new(0))],
                    ParaShapeIndex::new(0),
                )],
                PageSettings::a4(),
            ));
            let bytes =
                HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
            let xml = crate::PackageReader::new(&bytes).unwrap().read_header_xml().unwrap();
            let head: crate::schema::header::HxHead = crate::decoder::xml_from_str(&xml).unwrap();
            let refs = head.ref_list.unwrap();
            let groups = refs.fontfaces.unwrap().groups;
            let chars = refs.char_properties.unwrap().items;
            assert_eq!(chars.len(), 1);
            for shape in chars {
                let f = shape.font_ref.unwrap();
                for (lang, id) in LANGS
                    .into_iter()
                    .zip([f.hangul, f.latin, f.hanja, f.japanese, f.other, f.symbol, f.user])
                {
                    let group = groups.iter().find(|group| group.lang == lang).unwrap();
                    assert!(group.fonts.iter().any(|font| font.id == id), "{lang}: {id}");
                    assert_eq!(Some(id), group.fonts.iter().map(|font| font.id).min(), "{lang}");
                }
            }
            let decoded = HwpxDecoder::decode(&bytes).unwrap();
            let paragraph = &decoded.document.sections()[0].paragraphs[0];
            assert_eq!(paragraph.text_content(), "새 문서");
            assert_eq!(paragraph.para_shape_id.get(), 0);
            assert_eq!(paragraph.runs[0].char_shape_id.get(), 0);
            assert_eq!(decoded.style_store.char_shape_count(), 1);
            assert_eq!(decoded.style_store.para_shape_count(), 1);
        }
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

/// The five serde call sites each normalize their own output: a Core string
/// with tab, LF, CR and CRLF reaches every one of them in one document. A
/// bookmark name carries it to the section, memo and header/footer sites
/// (font names are cleaned before serialization, so they cannot).
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

    fn para(runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(0))
    }

    fn encode_parts() -> (String, String) {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        // 이것을 실패시키는 것: 합성 fixture의 명시적 charPr/paraPr 0 정의 누락.
        store.push_char_shape(crate::HwpxCharShape::default());
        store.push_para_shape(crate::HwpxParaShape::default());
        store.push_font(HwpxFont::new(7, format!(" 글꼴{RAW} "), "HANGUL"));
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
            vec![para(vec![Run::control(Control::bookmark(&format!("MEMO{RAW}")), cs)])],
            vec![Run::text("앵커", cs)],
        );
        let body = para(vec![
            Run::text("본문", cs),
            Run::control(Control::bookmark(&format!("BODY{RAW}")), cs),
            Run::control(equation_with_font(&format!(" EQ{RAW} ")), cs),
            // A shape inside a group is written by `shapes.rs` `serialize_with_root`,
            // not by the section serializer.
            Run::control(
                Control::Group {
                    frame: hwpforge_core::ObjectFrame::new(),
                    children: vec![
                        hwpforge_core::control::GroupChild::Shape(connect),
                        hwpforge_core::control::GroupChild::Shape(plain_line),
                    ],
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
            Control::bookmark(&format!("HEAD{RAW}")),
            cs,
        )])]));
        let mut doc = Document::new();
        doc.add_section(section);
        let doc = doc.validate().expect("valid document");

        read_parts(&HwpxEncoder::encode(&doc, &store, &ImageStore::new()).expect("encode"))
    }

    /// `(header.xml, section0.xml)` of an encoded package.
    fn read_parts(bytes: &[u8]) -> (String, String) {
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
        let mut read = |name: &str| {
            let mut s = String::new();
            zip.by_name(name).expect(name).read_to_string(&mut s).expect("utf-8");
            s
        };
        (read("Contents/header.xml"), read("Contents/section0.xml"))
    }

    /// The value of the first `attr="…"` (a whole attribute name, preceded by
    /// a space) whose value starts with `prefix`.
    fn attr_value<'a>(xml: &'a str, attr: &str, prefix: &str) -> &'a str {
        let needle = format!(" {attr}=\"");
        let start = xml
            .match_indices(&needle)
            .map(|(i, _)| i + needle.len())
            .find(|&start| xml[start..].starts_with(prefix))
            .unwrap_or_else(|| panic!("{attr}=\"{prefix}… not in output"));
        let end = start + xml[start..].find('"').expect("closing quote");
        &xml[start..end]
    }

    /// Every start tag, read with quote awareness so a `>` inside a value does
    /// not end the tag, is free of control-character references.
    fn assert_no_control_refs_in_start_tags(part: &str, xml: &str) {
        let mut tag_start: Option<usize> = None;
        let mut quote: Option<char> = None;
        for (i, c) in xml.char_indices() {
            match (tag_start, quote) {
                (None, _) if c == '<' => tag_start = Some(i),
                (Some(_), Some(q)) if c == q => quote = None,
                (Some(_), None) if c == '"' || c == '\'' => quote = Some(c),
                (Some(start), None) if c == '>' => {
                    let tag = &xml[start..=i];
                    for r in ["&#9;", "&#10;", "&#13;"] {
                        assert!(!tag.contains(r), "{part}: {r} left in {tag}");
                    }
                    tag_start = None;
                }
                _ => {}
            }
        }
    }

    // 이것을 실패시키는 것: 다섯 호출 지점 중 하나라도 정규화 호출을 빼는 것 — 그 part 의 어느
    // 시작 태그에 문자 참조가 남는다.
    #[test]
    fn no_control_reference_is_left_in_any_start_tag() {
        let (header, section) = encode_parts();
        assert_no_control_refs_in_start_tags("header.xml", &header);
        assert_no_control_refs_in_start_tags("section0.xml", &section);
    }

    // 이것을 실패시키는 것: `encoder/header.rs` 의 정규화 호출을 빼는 것.
    #[test]
    fn header_xml_style_attrs_are_normalized() {
        let (header, _) = encode_parts();
        assert_eq!(attr_value(&header, "name", "A"), "A B C D");
        assert_eq!(attr_value(&header, "engName", "E"), "E F", "CRLF folds to one space");
    }

    // Font names go further than the attribute normalization: control
    // characters are removed (not turned into spaces) and the ends are trimmed,
    // so Hancom can match an installed font. With a space left in, `함초롬바탕 `
    // leaves the font box empty and `HancomEQN ` breaks the equation (0.41 did
    // the same).
    // 이것을 실패시키는 것: `header.rs`·`equation.rs` 에서 `clean_font_name` 을 빼는 것.
    #[test]
    fn font_names_lose_control_characters_and_edge_spaces() {
        let (header, section) = encode_parts();
        assert_eq!(attr_value(&header, "face", "글꼴"), "글꼴ABCD");
        assert_eq!(attr_value(&section, "font", "EQ"), "EQABCD");
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
    }

    // 텍스트 `hp:t` 와 수식 `hp:script` 는 0.42 escape(`&#13;`) 그대로 둔다 — 한컴이 이것을
    // 0.41 의 literal CR 과 같게 표시하고 저장한다. 속성은 decode 하면 공백으로 돌아온다.
    // 이것을 실패시키는 것: `encoder/header.rs` 정규화 호출을 빼는 것(decode 값이 제어 문자가 됨),
    // 또는 정규화를 텍스트 구간까지 넓히는 것(`&#13;` 이 사라짐).
    #[test]
    fn normalized_output_decodes_to_spaces() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        // 이것을 실패시키는 것: 합성 fixture의 명시적 charPr/paraPr 0 정의 누락.
        store.push_char_shape(crate::HwpxCharShape::default());
        store.push_para_shape(crate::HwpxParaShape::default());
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
        let (_, section) = read_parts(&bytes);
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
    // 이것을 실패시키는 것: 디코더의 ws-only 판정(`is_ws_only_wire_text`)을 literal 공백만으로
    // 되돌리는 것 — run 이 경고 없이 사라진다.
    #[test]
    fn a_run_holding_only_a_cr_survives_the_round_trip() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        // 이것을 실패시키는 것: 합성 fixture의 명시적 charPr/paraPr 0 정의 누락.
        store.push_char_shape(crate::HwpxCharShape::default());
        store.push_para_shape(crate::HwpxParaShape::default());
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

    /// The other serde attributes a Core string reaches (found by filling every
    /// string of every repo fixture with control whitespace), beyond the ones
    /// the call-site tests above already pin.
    // 이것을 실패시키는 것: 이 속성들 중 하나를 serde 가 아닌 경로(정규화를 거치지 않는)로 옮기는 것,
    // 또는 header·section 정규화 호출을 빼는 것.
    #[test]
    fn remaining_census_attrs_are_normalized() {
        use crate::style_store::HwpxParaShape;
        use hwpforge_core::image::Image;

        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        // 이것을 실패시키는 것: 합성 fixture의 명시적 charPr/paraPr 0 정의 누락.
        store.push_char_shape(crate::HwpxCharShape::default());
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
        let (header, section) = read_parts(&bytes);
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

#[cfg(test)]
mod context_tests;

pub(crate) mod standalone;
