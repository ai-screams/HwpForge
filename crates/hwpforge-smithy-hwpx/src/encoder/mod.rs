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
use self::section::encode_section;

// ── HwpxEncoder ─────────────────────────────────────────────────

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
    pub fn encode_with_options(
        document: &Document<Validated>,
        style_store: &HwpxStyleStore,
        image_store: &ImageStore,
        options: EncodeOptions,
    ) -> HwpxResult<Vec<u8>> {
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
        for (i, section) in sections.iter().enumerate() {
            let result = encode_section(
                section,
                i,
                chart_offset,
                masterpage_offset,
                embedded_ole_offset,
                options,
            )?;
            chart_offset += result.charts.len();
            masterpage_offset += result.master_pages.len();
            embedded_ole_offset += result.embedded_oles.len();
            section_results.push(result);
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
        PackageWriter::write_hwpx(
            document.metadata(),
            &header_xml,
            &section_xmls,
            &images,
            &charts,
            &master_pages,
            &embedded_oles,
        )
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
