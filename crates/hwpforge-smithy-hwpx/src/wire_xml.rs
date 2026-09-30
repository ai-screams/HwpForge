//! Byte-level helpers for the XML that crosses the HWPX wire, shared by the
//! encoder and the decoder.
//!
//! - attribute values: [`normalize_attr_control_whitespace`] rewrites the
//!   control-whitespace references quick-xml 0.42 writes, and
//!   [`clean_font_name`] cleans font names on both write and read, so a
//!   decode→encode→decode round trip stays a no-op;
//! - whitespace-only text runs: [`preserve_ws_only_text`] marks them before
//!   quick-xml's serde decode would drop them, and [`strip_ws_sentinel`]
//!   removes the mark again.
//!
//! Every function here scans raw XML text; none of them parses XML.

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

/// Comments, CDATA sections and processing instructions: markup whose
/// content is character data, never an element or attribute.
const NON_ELEMENT_MARKUP: [(&[u8], &[u8]); 3] =
    [(b"<!--", b"-->"), (b"<![CDATA[", b"]]>"), (b"<?", b"?>")];

/// Comments and processing instructions, the only such markup a DTD internal
/// subset can hold.
const DTD_NON_DECLARATION_MARKUP: [(&[u8], &[u8]); 2] = [(b"<!--", b"-->"), (b"<?", b"?>")];

/// If `rest` starts one of `kinds`, its length through the closing token (the
/// whole of `rest` when unclosed: the parser then reports the malformed input).
fn markup_len(rest: &[u8], kinds: &[(&[u8], &[u8])]) -> Option<usize> {
    let (open, close) = kinds.iter().find(|(open, _)| rest.starts_with(open))?;
    Some(
        rest[open.len()..]
            .windows(close.len())
            .position(|w| w == *close)
            .map_or(rest.len(), |p| open.len() + p + close.len()),
    )
}

/// XML whitespace (`S` in the XML grammar).
fn is_xml_space(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\r' | '\n')
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
                if let Some(len) = markup_len(&bytes[i..], &NON_ELEMENT_MARKUP) {
                    i += len;
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

/// quick-xml 의 serde 역직렬화는 whitespace-only 텍스트 노드를 무시해
/// `<hp:t> </hp:t>` (공백만 담긴 run — 한컴도 저작하는 유효 wire)가 통째로
/// 사라진다. 문자 참조(`&#32;`)도 unescape 후 판정이라 같은 일을 겪는다
/// (실측). 그래서 파싱 전에 ws-only 콘텐츠 선두에 sentinel `U+E000` 을 붙여
/// 노드를 살리고, [`HxText`](crate::schema::section::HxText) 소비 시점(`strip_ws_sentinel`)에 대칭 제거한다.
/// ws-only 판정은 literal 공백과 공백 문자 참조를 함께 본다
/// ([`is_xml_whitespace`]) — quick-xml 0.42 가 텍스트의 CR 을 `&#13;` 로 쓰므로
/// CR 하나만 든 run 이 이 형태로 온다.
///
/// mixed content(`<hp:t>` 안에 자식 요소가 있는 경우)와 CDATA 는 건드리지
/// 않는다 — 관측된 결함 범위는 순수 ws-only 콘텐츠뿐이다.
pub(crate) fn preserve_ws_only_text(xml: &str) -> std::borrow::Cow<'_, str> {
    let mut result = mark_ws_only_text(xml, "<hp:t");
    if let std::borrow::Cow::Borrowed(_) = result {
        result = mark_ws_only_text(xml, "<t");
    }
    result
}

/// `content` (escape 된 원시 텍스트) 가 XML 공백만 담는지 — literal
/// `' ' '\t' '\r' '\n'` 과 그 문자 참조(`&#32;` `&#9;` `&#13;` `&#10;`, 16진
/// `&#x20;` 등, 앞자리 0 허용). 다른 참조·엔티티(`&amp;` 등)가 하나라도 있으면
/// false 다.
fn is_xml_whitespace(content: &str) -> bool {
    let mut rest = content;
    while let Some(c) = rest.chars().next() {
        if is_xml_space(c) {
            rest = &rest[1..];
            continue;
        }
        let Some(body) = rest.strip_prefix("&#") else { return false };
        let Some(end) = body.find(';') else { return false };
        // Digits only: `from_str_radix`/`parse` would also take a leading `+`.
        // XML CharRef takes a lowercase `x` only (quick-xml agrees).
        let code = match body[..end].strip_prefix('x') {
            Some(hex) if hex.bytes().all(|b| b.is_ascii_hexdigit()) => {
                u32::from_str_radix(hex, 16).ok()
            }
            None if body[..end].bytes().all(|b| b.is_ascii_digit()) => {
                body[..end].parse::<u32>().ok()
            }
            _ => None,
        };
        if !matches!(code, Some(0x20 | 0x09 | 0x0D | 0x0A)) {
            return false;
        }
        rest = &body[end + 1..];
    }
    true
}

/// Length of the document type declaration at the start of `rest`, up to and
/// including its closing `>`: quoted literals and the `[...]` internal subset
/// may hold `>`, `]` and `<hp:t>`-shaped text, and a comment or processing
/// instruction inside the subset is skipped whole (its quotes and brackets are
/// not markup). Runs to the end when unclosed (the parser then reports the
/// malformed input).
fn doctype_len(rest: &[u8]) -> usize {
    let mut quote: Option<u8> = None;
    let mut depth = 0usize;
    let mut i = 0;
    while i < rest.len() {
        let b = rest[i];
        if let Some(q) = quote {
            if b == q {
                quote = None;
            }
            i += 1;
            continue;
        }
        if let Some(len) = markup_len(&rest[i..], &DTD_NON_DECLARATION_MARKUP) {
            i += len;
            continue;
        }
        match b {
            b'"' | b'\'' => quote = Some(b),
            b'[' => depth += 1,
            b']' => depth = depth.saturating_sub(1),
            b'>' if depth == 0 => return i + 1,
            _ => {}
        }
        i += 1;
    }
    rest.len()
}

/// [`preserve_ws_only_text`] 의 실제 스캐너 — `open` 여는-태그 접두로 1패스.
///
/// 주석·CDATA·PI 안의 `<hp:t>` 모양 글자는 요소가 아니므로 건너뛴다 — 표시하면
/// 수식 스크립트 같은 문자 데이터에 U+E000 이 박히고, 주석 안의 가짜 여는 태그가
/// 뒤따르는 진짜 run 을 가린다.
fn mark_ws_only_text<'a>(xml: &'a str, open: &str) -> std::borrow::Cow<'a, str> {
    let close = if open == "<hp:t" { "</hp:t>" } else { "</t>" };
    let mut out: Option<String> = None;
    let mut last = 0usize;
    let mut search = 0usize;
    while let Some(rel) = xml[search..].find('<') {
        let tag_start = search + rel;
        let rest = &xml[tag_start..];
        if let Some(len) = markup_len(rest.as_bytes(), &NON_ELEMENT_MARKUP) {
            search = tag_start + len;
            continue;
        }
        if rest.starts_with("<!DOCTYPE") {
            search = tag_start + doctype_len(rest.as_bytes());
            continue;
        }
        if !rest.starts_with(open) {
            search = tag_start + 1;
            continue;
        }
        let after = &xml[tag_start + open.len()..];
        // `<hp:tab/>` 등 다른 태그 배제: 다음 문자가 '>' 또는 공백(속성)이어야 함.
        let Some(first) = after.chars().next() else { break };
        if first != '>' && !first.is_ascii_whitespace() {
            search = tag_start + open.len();
            continue;
        }
        let Some(gt_rel) = after.find('>') else { break };
        // 자기닫힘 `<hp:t/>` 는 콘텐츠 없음.
        if after[..gt_rel].ends_with('/') {
            search = tag_start + open.len() + gt_rel + 1;
            continue;
        }
        let content_start = tag_start + open.len() + gt_rel + 1;
        let Some(close_rel) = xml[content_start..].find(close) else { break };
        let content = &xml[content_start..content_start + close_rel];
        let ws_only = !content.is_empty() && !content.contains('<') && is_xml_whitespace(content);
        if ws_only {
            let buf = out.get_or_insert_with(|| String::with_capacity(xml.len() + 8));
            buf.push_str(&xml[last..content_start]);
            buf.push('\u{E000}');
            last = content_start;
        }
        search = content_start + close_rel + close.len();
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&xml[last..]);
            std::borrow::Cow::Owned(buf)
        }
        None => std::borrow::Cow::Borrowed(xml),
    }
}

/// 디코더 전처리(`preserve_ws_only_text`)가 ws-only `<hp:t>` 콘텐츠 선두에
/// 붙인 sentinel `U+E000` 을 대칭 제거한다. 조건은 전처리와 정확히 동형 —
/// "선두 U+E000 + 나머지 전부 XML whitespace" 일 때만 1글자 벗긴다.
/// (원본 문서가 그 정확한 형태의 텍스트를 가질 이론적 위험은 PUA 단독+공백
/// run 이라 실사용이 없다 — 전처리 rustdoc 참조.)
pub(crate) fn strip_ws_sentinel(s: &str) -> &str {
    if let Some(rest) = s.strip_prefix('\u{E000}') {
        if !rest.is_empty() && rest.chars().all(is_xml_space) {
            return rest;
        }
    }
    s
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

    // 한컴 판정(VG-1) 입력을 0.42 serde 가 쓰는 형태 그대로 넣는다. 참조가 남으면 한컴에서
    // 스타일 이름이 잘린다. (글꼴 이름·수식 글꼴은 serde 전에 `clean_font_name` 이 정리해
    // 이 함수까지 제어 문자가 오지 않는다 — 공백으로 두면 한컴이 글꼴을 못 찾는다, VG-2·3.)
    // 이것을 실패시키는 것: `&#9;`·`&#10;`·`&#13;` 중 하나의 치환을 빼는 것.
    #[test]
    fn hancom_verdict_inputs_leave_no_references() {
        let cases = [
            (
                r#"<hh:style name="바탕&#10;글" engName="Nor&#9;mal"/>"#,
                r#"<hh:style name="바탕 글" engName="Nor mal"/>"#,
            ),
            (r#"<hh:style name="본&#13;문"/>"#, r#"<hh:style name="본 문"/>"#),
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
}

#[cfg(test)]
mod ws_scanner_tests {
    use super::*;

    // An unclosed comment runs to the end: nothing after `<!--` is markup, so
    // a run-shaped string inside it is not marked and no attribute in it is
    // rewritten (the parser then reports the malformed input).
    // 이것을 실패시키는 것: 닫히지 않은 주석에서 여는 토큰 뒤부터 다시 훑는 것.
    #[test]
    fn an_unclosed_comment_runs_to_the_end() {
        let marked = "<!-- <hp:t> </hp:t>";
        assert_eq!(preserve_ws_only_text(marked), marked);
        let attr = r#"<!-- <a x="A&#9;B"/>"#;
        assert_eq!(normalize_attr_control_whitespace(attr), attr);
    }

    #[test]
    fn preserve_ws_only_text_marks_only_pure_whitespace() {
        let xml = "<hp:run><hp:t>a</hp:t><hp:t> </hp:t><hp:t>  b</hp:t><hp:t/></hp:run>";
        let out = preserve_ws_only_text(xml);
        assert_eq!(
            out,
            "<hp:run><hp:t>a</hp:t><hp:t>\u{E000} </hp:t><hp:t>  b</hp:t><hp:t/></hp:run>"
        );
    }

    #[test]
    fn preserve_ws_only_text_ignores_mixed_and_other_tags() {
        let xml = "<hp:t> <hp:tab/></hp:t><hp:tbl> </hp:tbl>";
        assert_eq!(preserve_ws_only_text(xml), xml, "mixed content and other tags untouched");
    }

    // 이것을 실패시키는 것: 공백이 아닌 참조(`&#65;`)나 escape 된 `&amp;#13;` 까지 공백으로
    // 보는 것 — sentinel 이 붙어 본문 앞에 U+E000 이 남는다.
    #[test]
    fn non_whitespace_references_are_not_marked() {
        for wire in
            ["&#65;", "&amp;#13;", "&#13;x", "&#1;", "&#13", "&#+13;", "&#x+d;", "&#;", "&#X20;"]
        {
            let xml = format!("<hp:t>{wire}</hp:t>");
            assert_eq!(preserve_ws_only_text(&xml), xml, "{wire}");
        }
    }

    // `<hp:t>` inside a comment, CDATA section or processing instruction is
    // character data, not an element: marking it would plant U+E000 in, e.g.,
    // an equation script that no consumer strips.
    // 이것을 실패시키는 것: 스캐너가 주석·CDATA·PI 를 건너뛰지 않는 것.
    #[test]
    fn markup_that_is_not_an_element_is_left_alone() {
        for xml in [
            "<hp:script><![CDATA[<hp:t>&#13;</hp:t>]]></hp:script>",
            "<hp:script><![CDATA[<hp:t> </hp:t>]]></hp:script>",
            "<!-- <hp:t> --><x/>",
            "<?pi <hp:t> </hp:t>?>",
        ] {
            assert_eq!(preserve_ws_only_text(xml), xml, "{xml}");
        }
    }

    // 이것을 실패시키는 것: 주석을 건너뛰지 않는 것 — 주석 안의 가짜 `<hp:t>` 가 진짜
    // `</hp:t>` 까지 먹어 뒤따르는 공백 run 이 표시되지 않는다.
    #[test]
    fn a_fake_tag_in_a_comment_does_not_hide_a_real_run() {
        let xml = "<!-- <hp:t> --><hp:t> </hp:t>";
        assert_eq!(preserve_ws_only_text(xml), "<!-- <hp:t> --><hp:t>\u{E000} </hp:t>");
    }

    // A document type declaration can hold `<hp:t>`-shaped text in an entity
    // value or a quoted literal, and `]` / `>` inside quotes do not end it.
    // 이것을 실패시키는 것: DOCTYPE 을 건너뛰지 않는 것(가짜 `<hp:t>` 가 진짜 run 을 가림),
    // 따옴표 안의 `]>` 에서 끝으로 보는 것, 또는 내부 subset 의 첫 선언 `>` 에서
    // 끝으로 보는 것(둘째 선언의 `<hp:t>` 가 진짜 run 을 가림), 또는 내부 subset 의
    // 주석·PI 안 따옴표·괄호를 구문으로 보는 것(파일 끝까지 DOCTYPE 이 됨).
    #[test]
    fn a_doctype_is_skipped_whole() {
        for doctype in [
            r#"<!DOCTYPE sec [<!ENTITY e "<hp:t>">]>"#,
            r#"<!DOCTYPE sec [<!ENTITY e "]><hp:t>">]>"#,
            r#"<!DOCTYPE sec SYSTEM "x.dtd">"#,
            r#"<!DOCTYPE sec [<!ENTITY a "x"><!ENTITY e "<hp:t>">]>"#,
            r#"<!DOCTYPE sec [<!-- " -->]>"#,
            r#"<!DOCTYPE sec [<!-- ]><hp:t> </hp:t> -->]>"#,
            r#"<!DOCTYPE sec [<?pi ' ] ?>]>"#,
        ] {
            let xml = format!("{doctype}<hp:t> </hp:t>");
            let want = format!("{doctype}<hp:t>\u{E000} </hp:t>");
            assert_eq!(preserve_ws_only_text(&xml), want, "{doctype}");
        }
    }

    // An unclosed declaration runs to the end: nothing after it is marked, and
    // the parser reports the malformed input.
    // 이것을 실패시키는 것: 닫히지 않은 DOCTYPE 에서 끝 대신 중간에서 멈추는 것.
    #[test]
    fn an_unclosed_doctype_runs_to_the_end() {
        let xml = "<!DOCTYPE sec [<hp:t> </hp:t>";
        assert_eq!(preserve_ws_only_text(xml), xml);
    }
}
