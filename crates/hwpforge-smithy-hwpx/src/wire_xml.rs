//! Byte-level helpers for the XML that crosses the HWPX wire, shared by the
//! encoder and the decoder.
//!
//! - attribute values: [`normalize_attr_control_whitespace`] rewrites the
//!   control-whitespace references quick-xml 0.42 writes, and
//!   [`clean_font_name`] cleans font names on both write and read, so a
//!   decode→encode→decode round trip stays a no-op;
//! - text-art line breaks: [`textart_text_to_wire`] and
//!   [`textart_text_from_wire`] convert between Core's `\r\n` and Hancom's
//!   `␍␊` in `hp:textart@text`;
//! - whitespace-only text nodes: [`preserve_ws_only_text`] marks them before
//!   quick-xml's serde decode would drop them, and [`PreservedText::as_str`]
//!   removes the mark again.
//!
//! The whitespace scanner also resolves namespace declarations on start tags.

use std::borrow::Cow;

/// How Hancom writes one TextArt line break inside `<hp:textart text="…">`:
/// the visible pair `␍␊` (U+240D U+240A). A raw CR/LF would not survive —
/// XML attribute-value normalization reads it back as a space. Core carries
/// the break as `\r\n`, the form the HWP5 record uses (issue #199).
pub(crate) const TEXTART_LINE_BREAK: &str = "\u{240D}\u{240A}";

/// Core → wire: every line break — `\r\n`, a lone `\r` or a lone `\n` —
/// becomes one [`TEXTART_LINE_BREAK`] pair.
pub(crate) fn textart_text_to_wire(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                chars.next_if_eq(&'\n');
                out.push_str(TEXTART_LINE_BREAK);
            }
            '\n' => out.push_str(TEXTART_LINE_BREAK),
            _ => out.push(ch),
        }
    }
    out
}

/// Wire → Core: each [`TEXTART_LINE_BREAK`] pair becomes `\r\n`. A lone
/// `␍` or `␊` is a symbol the author typed and stays as it is.
///
/// A file from another writer may carry the break as a character reference
/// (`&#10;`, `&#13;`) that reaches here as a raw `\n` or `\r`; it is folded
/// to `\r\n` too, so the decoded text has one form and a
/// decode→encode→decode round trip leaves it unchanged.
pub(crate) fn textart_text_from_wire(text: &str) -> String {
    textart_text_to_wire(text).replace(TEXTART_LINE_BREAK, "\r\n")
}

/// Body `<hp:t>` line breaks, excluding attributes, scripts and TextArt.
/// Encoding folds CRLF only within the supplied text fragment. Decoding
/// replaces CR one-for-one: the original wire map still counts both units
/// of a referenced CRLF. Never normalize the XML before building that map.
pub(crate) fn body_text_line_breaks(text: &str, fold_crlf: bool) -> Cow<'_, str> {
    if !text.contains('\r') {
        return Cow::Borrowed(text);
    }
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\r' {
            if fold_crlf {
                chars.next_if_eq(&'\n');
            }
            out.push('\n');
        } else {
            out.push(ch);
        }
    }
    Cow::Owned(out)
}

/// Whether encoding this body fragment changes its UTF-16 coordinate length.
/// Keep cache admission tied to the same rule that writes the body.
pub(crate) fn body_text_line_breaks_change_utf16_len(text: &str) -> bool {
    body_text_line_breaks(text, true).encode_utf16().count() != text.encode_utf16().count()
}

/// Cleans a font name before it is written (`hh:font@face`,
/// `hp:equation@font`): control characters are removed and ASCII spaces
/// are trimmed from the ends; spaces inside the name are kept
/// (`Times New Roman`). Other whitespace at the ends (U+3000, NBSP) is kept:
/// only ASCII spaces and control characters were checked in Hancom.
///
/// Hancom matches the name against installed fonts exactly, and neither a
/// control character nor a space at either end counts toward a name it can
/// match: with a space in place of the control character (what
/// [`normalize_attr_control_whitespace`] would write, and what Hancom reads
/// from 0.41's literal control character) `함초롬바탕 ` leaves the font box
/// empty and `HancomEQN ` breaks the equation, and ` 함초롬바탕` does the
/// same. Cleaning the name lets it match. The decoder reads names through
/// this function too, so decode→encode→decode stays a no-op for edits. No
/// warning is raised; the release notes record the change.
pub(crate) fn clean_font_name(name: &str) -> String {
    name.trim_matches(|c: char| c.is_control() || c == ' ')
        .chars()
        .filter(|c| !c.is_control())
        .collect()
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

/// The references [`normalize_attr_control_whitespace`] rewrites, each to one
/// space. The CR LF pair comes first so that it becomes one space, not two.
const CONTROL_WS_REFS: [&[u8]; 4] = [b"&#13;&#10;", b"&#9;", b"&#10;", b"&#13;"];

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
/// cleaned here. Line breaks in `hp:textart@text` are converted by
/// [`textart_text_to_wire`].
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
pub(crate) fn normalize_attr_control_whitespace(xml: &str) -> Cow<'_, str> {
    if !xml.contains("&#") {
        return Cow::Borrowed(xml);
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
                let Some(reference) = CONTROL_WS_REFS.iter().find(|r| bytes[i..].starts_with(r))
                else {
                    i += 1;
                    continue;
                };
                let buf = out.get_or_insert_with(|| String::with_capacity(xml.len()));
                buf.push_str(&xml[copied..i]);
                buf.push(' ');
                i += reference.len();
                copied = i;
            }
        }
    }
    match out {
        None => Cow::Borrowed(xml),
        Some(mut buf) => {
            buf.push_str(&xml[copied..]);
            Cow::Owned(buf)
        }
    }
}

/// Whether XML 1.0 forbids `c` anywhere in a document: the C0 controls
/// other than TAB, LF and CR, and U+FFFE/U+FFFF (the `Char` production;
/// a surrogate cannot occur in a `str`). Not even a character reference
/// (`&#1;`) may stand for one, so the only way to keep a part well-formed
/// is to leave the character out.
///
/// U+0000 is included: the XML grammar forbids it like the other controls.
pub(crate) fn is_xml_forbidden_char(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// Adds `count` removals at `location`, merging with an earlier entry for
/// the same location so the list stays in first-seen order.
pub(crate) fn tally_removed(sites: &mut Vec<(String, usize)>, location: &str, count: usize) {
    if count == 0 {
        return;
    }
    match sites.iter_mut().find(|(seen, _)| seen == location) {
        Some((_, total)) => *total += count,
        None => sites.push((location.to_string(), count)),
    }
}

/// Whether an edit surface must drop `c` from a caller's text before writing
/// it as `<hp:t>` content: every [`is_xml_forbidden_char`] except U+001F,
/// which the inline-text encoder writes as `<hp:fwSpace/>` and so can carry.
pub(crate) fn is_edit_unrepresentable_char(c: char) -> bool {
    c != '\u{1F}' && is_xml_forbidden_char(c)
}

/// Removes the characters [`is_edit_unrepresentable_char`] rejects from one
/// caller-supplied text value and reports their removal.
///
/// Every edit surface (fill, patch, insert-para, set-cell, stamp) passes the
/// caller's value through this once, at its entry, and uses the returned text
/// everywhere after that — for the bytes it writes, for the "did the value
/// change" comparison and for any expected state it verifies against. A
/// removal adds one [`EncodeWarning::XmlForbiddenCharsRemoved`] for
/// `(part, location)`; a value with nothing to remove is returned unchanged
/// and adds nothing.
///
/// [`EncodeWarning::XmlForbiddenCharsRemoved`]: crate::EncodeWarning::XmlForbiddenCharsRemoved
pub(crate) fn sanitize_edit_text(
    text: &str,
    part: &str,
    location: &str,
    warnings: &mut Vec<crate::EncodeWarning>,
) -> String {
    let (cleaned, count) = clean_edit_text(text);
    if count > 0 {
        warnings.push(crate::EncodeWarning::XmlForbiddenCharsRemoved {
            part: part.to_string(),
            location: location.to_string(),
            count,
        });
    }
    cleaned
}

/// The removal half of [`sanitize_edit_text`], for a surface that learns the
/// value's location only after using the cleaned value: the cleaned text and
/// how many characters left it.
pub(crate) fn clean_edit_text(text: &str) -> (String, usize) {
    let count = text.chars().filter(|&c| is_edit_unrepresentable_char(c)).count();
    if count == 0 {
        return (text.to_string(), 0);
    }
    (text.chars().filter(|&c| !is_edit_unrepresentable_char(c)).collect(), count)
}

/// Copies `s` into `out` without its forbidden characters; returns how many
/// were left out.
fn push_allowed(out: &mut String, s: &str) -> usize {
    let mut removed = 0;
    for c in s.chars() {
        if is_xml_forbidden_char(c) {
            removed += 1;
        } else {
            out.push(c);
        }
    }
    removed
}

/// Length of the start or end tag at the front of `rest` (which starts with
/// `<`), through the `>` that is not inside a quoted attribute value; the
/// whole of `rest` when unclosed.
fn tag_len(rest: &[u8]) -> usize {
    let mut quote: Option<u8> = None;
    for (i, &b) in rest.iter().enumerate().skip(1) {
        match quote {
            Some(q) if b == q => quote = None,
            Some(_) => {}
            None if b == b'"' || b == b'\'' => quote = Some(b),
            None if b == b'>' => return i + 1,
            None => {}
        }
    }
    rest.len()
}

/// Removes every character [`is_xml_forbidden_char`] rejects from a whole
/// XML part, and reports where they were: `(location, count)` per location,
/// in first-seen order. A location is `element@attribute` for an attribute
/// value (`hh:style@engName`) and the enclosing element's name for text
/// (`hp:t`), including a comment, CDATA section or processing instruction
/// inside it; content outside the root element is `(top level)`.
///
/// quick-xml's serde serializer writes these characters as they are, and
/// so would any string spliced into a part unescaped. Scanning the part
/// after it is assembled covers every such path at once, which a per-field
/// strip at the Core→schema boundary cannot promise for the hundreds of
/// string fields the schema has.
///
/// The input must be a part this encoder assembled: every `<` in text and
/// attribute values is escaped, so an unescaped `<` starts markup. It is not
/// a general XML rewriter. Text after an end tag is placed in the parent
/// element, so the element stack must balance, as it does in serde output.
pub(crate) fn strip_xml_forbidden_chars(xml: &str) -> (Cow<'_, str>, Vec<(String, usize)>) {
    if !xml.chars().any(is_xml_forbidden_char) {
        return (Cow::Borrowed(xml), Vec::new());
    }
    let bytes = xml.as_bytes();
    let mut out = String::with_capacity(xml.len());
    let mut sites: Vec<(String, usize)> = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < xml.len() {
        let parent = open.last().copied().unwrap_or("(top level)");
        // Every boundary below is an ASCII `<` or `>` (or the end), so each
        // slice of `xml` starts and ends on a char boundary.
        let end = if bytes[i] != b'<' {
            xml[i..].find('<').map_or(xml.len(), |p| i + p)
        } else if let Some(len) = markup_len(&bytes[i..], &NON_ELEMENT_MARKUP) {
            i + len
        } else {
            let end = i + tag_len(&bytes[i..]);
            push_tag(&xml[i..end], &mut out, &mut open, &mut sites);
            i = end;
            continue;
        };
        let removed = push_allowed(&mut out, &xml[i..end]);
        tally_removed(&mut sites, parent, removed);
        i = end;
    }
    (Cow::Owned(out), sites)
}

/// Copies one start or end tag into `out` without its forbidden characters,
/// keeping the element stack of [`strip_xml_forbidden_chars`] in step.
fn push_tag<'a>(
    tag: &'a str,
    out: &mut String,
    open: &mut Vec<&'a str>,
    sites: &mut Vec<(String, usize)>,
) {
    if tag.starts_with("</") {
        open.pop();
        let removed = push_allowed(out, tag);
        tally_removed(sites, open.last().copied().unwrap_or("(top level)"), removed);
        return;
    }
    let name_end = tag[1..]
        .find(|c: char| is_xml_space(c) || c == '/' || c == '>')
        .map_or(tag.len(), |p| p + 1);
    let name = &tag[1..name_end];
    out.push_str(&tag[..name_end]);
    // `seg` is where the text before the next `=` began: after the name or
    // after the last closing quote, so its last word is the attribute name.
    let mut seg = name_end;
    let mut value: Option<(char, &str, usize)> = None;
    for (offset, c) in tag[name_end..].char_indices() {
        let at = name_end + offset;
        match value {
            Some((q, attr, removed)) if c == q => {
                tally_removed(sites, &format!("{name}@{attr}"), removed);
                value = None;
                seg = at + 1;
                out.push(c);
            }
            Some((q, attr, removed)) if is_xml_forbidden_char(c) => {
                value = Some((q, attr, removed + 1));
            }
            Some(_) => out.push(c),
            None if c == '"' || c == '\'' => {
                let before = tag[seg..at].trim_end().trim_end_matches('=');
                let attr = before.split_whitespace().last().unwrap_or("");
                value = Some((c, attr, 0));
                out.push(c);
            }
            None if is_xml_forbidden_char(c) => tally_removed(sites, name, 1),
            None => out.push(c),
        }
    }
    if !tag.ends_with("/>") {
        open.push(name);
    }
}

/// The mark for blank entries; a source entry starting with this character
/// is escaped by prepending one more, so even repeated marks are reversible.
const WS_SENTINEL: char = '\u{E000}';

/// Marks whitespace-only character data directly inside paragraph-namespace
/// `t` elements (or unqualified `t`), including gaps around children.
///
/// The serde decoder skips blank mixed-content entries independently of the
/// reader's text-trimming setting. A mark keeps each entry alive; both Core
/// emission and the wire ledger remove it with [`PreservedText::as_str`].
/// Consecutive text, character references and CDATA are one entry, even across
/// comments and processing instructions, so they receive at most one mark.
///
/// Hancom writes spaces before `fwSpace` and after inline elements as document
/// characters. The `<hp:t>` parent is the distinction: all its character data
/// is text, including XML whitespace; indentation between paragraphs, runs or
/// other structural elements is not. Guessing from newlines or the presence of
/// children would erase valid text in pretty-printed documents too.
///
/// Source entries starting with the mark are escaped, including references
/// and CDATA. Serde accepts `t` by local name even in other namespaces, so
/// escaping covers those entries too; only paragraph text gets blank marks.
/// Namespace errors are returned before any partially rewritten XML is used.
pub(crate) fn preserve_ws_only_text(xml: &str) -> Result<Cow<'_, str>, quick_xml::Error> {
    preserve_ws_only_text_counted(xml, &mut NamespaceWork::default())
}

// Counts actual URI input traversed by the classifier, not elapsed time or an
// estimate from document size. No counters are stored in production builds.
#[derive(Default, Debug)]
struct NamespaceWork {
    #[cfg(test)]
    uri_bytes: usize,
    #[cfg(test)]
    classifications: usize,
    #[cfg(test)]
    binding_probes: usize,
}

const PARAGRAPH_NS: &str = "http://www.hancom.co.kr/hwpml/2011/paragraph";

impl NamespaceWork {
    fn classify(&mut self, uri: &str) -> Result<bool, quick_xml::Error> {
        #[cfg(test)]
        {
            self.uri_bytes += uri.len();
            self.classifications += 1;
        }
        Ok(quick_xml::escape::unescape(uri)? == PARAGRAPH_NS)
    }

    fn probe(&mut self) {
        #[cfg(test)]
        {
            self.binding_probes += 1;
        }
    }
}

fn preserve_ws_only_text_counted<'a>(
    xml: &'a str,
    work: &mut NamespaceWork,
) -> Result<Cow<'a, str>, quick_xml::Error> {
    use quick_xml::events::BytesStart;
    use quick_xml::name::{Namespace, NamespaceError, NamespaceResolver, PrefixDeclaration};

    let mut namespaces = NamespaceResolver::default();
    let namespace_limit = crate::decoder::XML_MAX_NAMESPACE_BINDINGS;
    // One extra slot is for our implicit binding, not a source declaration.
    // 이것을 실패시키는 것: +1 생략 — 유효한 원문 128개 바인딩을 거부한다.
    namespaces.set_max_namespace_bindings(namespace_limit + 1);
    namespaces.add(PrefixDeclaration::Named("hp"), Namespace(PARAGRAPH_NS))?;
    // Cache declaration classifications with the same scope as the resolver.
    // Only prefixes are compared at element lookup; URI bytes are never hashed,
    // compared or unescaped again. The binding count is bounded above by 131.
    let mut bindings = vec![
        (Some("xml".to_owned()), false),
        (Some("xmlns".to_owned()), false),
        (Some("hp".to_owned()), true),
    ];
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut at = 0;
    let mut parents = Vec::new();
    while at < xml.len() {
        let (end, blank, first) = text_group(xml, at);
        let (is_text, preserve_blank, _) = parents.last().copied().unwrap_or((false, false, 0));
        // 이것을 실패시키는 것: 원문 선두 표식 이스케이프 생략 또는 URI 제한으로
        // 외부 네임스페이스의 serde `t` 텍스트를 이스케이프하지 않기.
        if is_text && PreservedText::needs_mark(blank && preserve_blank, first) {
            let buf = out.get_or_insert_with(|| String::with_capacity(xml.len()));
            buf.push_str(&xml[copied..at]);
            buf.push(WS_SENTINEL);
            copied = at;
        }
        at = end;
        if at == xml.len() {
            break;
        }
        let rest = &xml[at..];
        if rest.starts_with("<!DOCTYPE") {
            at += doctype_len(rest.as_bytes());
            continue;
        }
        let end = at + tag_len(rest.as_bytes());
        let tag = &xml[at..end];
        if tag.starts_with("</") {
            if let Some((_, _, bindings_before)) = parents.pop() {
                namespaces.pop();
                bindings.truncate(bindings_before);
            }
        } else {
            let name_end = tag[1..]
                .find(|c: char| is_xml_space(c) || c == '/' || c == '>')
                .map_or(tag.len(), |p| p + 1);
            let content = tag[1..].trim_end_matches('>').trim_end_matches('/');
            let start = BytesStart::from_content(content, name_end - 1);
            // 이것을 실패시키는 것: 내부 여유 슬롯을 오류의 제한값으로 보고하기.
            namespaces.push(&start).map_err(|error| match error {
                NamespaceError::TooManyBindings(_) => {
                    NamespaceError::TooManyBindings(namespace_limit)
                }
                other => other,
            })?;
            let bindings_before = bindings.len();
            // Mirror push's attribute iteration, including its malformed-attribute
            // stop rule. The downstream XML parser diagnoses malformed attributes.
            for attr in start.attributes().with_checks(false) {
                let Ok(attr) = attr else { break };
                if let Some(prefix) = attr.key.as_namespace_binding() {
                    let prefix = match prefix {
                        PrefixDeclaration::Default => None,
                        // Resolver validates these but does not allocate a binding.
                        // 이것을 실패시키는 것: 예약 접두사 재선언을 캐시에 누적하기.
                        PrefixDeclaration::Named("xml" | "xmlns") => continue,
                        PrefixDeclaration::Named(prefix) => Some(prefix.to_owned()),
                    };
                    // 이것을 실패시키는 것: 분류를 요소 조회로 이동하여 URI를 재처리하기,
                    // 참조 해석 생략, 또는 default/named 빈 URI를 같은 뜻으로 취급하기.
                    let paragraph =
                        work.classify(&attr.value)? || (prefix.is_none() && attr.value.is_empty());
                    bindings.push((prefix, paragraph));
                }
            }
            let (local, prefix) = start.name().decompose();
            let prefix = prefix.map(|p| p.into_inner());
            let is_text = local.as_ref() == "t";
            let paragraph = bindings
                .iter()
                .rev()
                .find(|(bound, _)| {
                    work.probe();
                    bound.as_deref() == prefix
                })
                .map_or(prefix.is_none(), |(_, paragraph)| *paragraph);
            // 이것을 실패시키는 것: 빈 요소의 xmlns 범위를 다음 형제로 누출하기.
            if tag.ends_with("/>") {
                namespaces.pop();
                bindings.truncate(bindings_before);
            } else {
                parents.push((is_text, is_text && paragraph, bindings_before));
            }
        }
        at = end;
    }
    match out {
        Some(mut buf) => {
            buf.push_str(&xml[copied..]);
            Ok(Cow::Owned(buf))
        }
        None => Ok(Cow::Borrowed(xml)),
    }
}

/// Finds one serde character-data entry, stopping at an element or declaration.
/// Comments and processing instructions contribute no characters; CDATA is
/// literal text (its `&#32;` is not a character reference).
fn text_group(xml: &str, mut at: usize) -> (usize, bool, Option<char>) {
    let mut first = None;
    let mut blank = true;
    let mut nonempty = false;
    while at < xml.len() {
        let rest = &xml[at..];
        if let Some(len) = markup_len(rest.as_bytes(), &NON_ELEMENT_MARKUP) {
            if let Some(cdata) = rest[..len].strip_prefix("<![CDATA[") {
                let content = cdata.strip_suffix("]]>").unwrap_or(cdata);
                // 이것을 실패시키는 것: CDATA 선두 표식을 읽지 않거나, 앞선
                // 텍스트가 있는데 CDATA의 선두로 덮어쓰기.
                if first.is_none() {
                    first = content.chars().next();
                }
                nonempty |= !content.is_empty();
                blank &= content.chars().all(is_xml_space);
            }
            at += len;
        } else if rest.starts_with('<') {
            break;
        } else {
            let len = rest.find('<').unwrap_or(rest.len());
            nonempty = true;
            // Only the first character matters for escaping. Unescape just a
            // leading reference, not the whole entry (later entities may be
            // unknown to this scanner and are diagnosed by serde).
            if first.is_none() {
                first = if rest.starts_with('&') {
                    rest[..len].find(';').and_then(|end| {
                        quick_xml::escape::unescape(&rest[..=end]).ok()?.chars().next()
                    })
                } else {
                    rest.chars().next()
                };
            }
            blank &= is_ws_only_wire_text(&rest[..len]);
            at += len;
        }
    }
    (at, nonempty && blank, first)
}

/// Whether escaped element content holds only XML whitespace: literal
/// `' ' '\t' '\r' '\n'` or references to them (`&#32;` `&#9;` `&#13;` `&#10;`,
/// or hexadecimal `&#x20;`, leading zeros allowed). Any other reference or
/// entity (`&amp;`, …), or markup, makes it false.
fn is_ws_only_wire_text(content: &str) -> bool {
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

/// Character data in the preprocessed XML representation, never a raw Core string.
/// The private payload prevents inline parts and `HxText::new` from bypassing
/// the same escaping rule. Deserialize accepts only the scanner's wire form;
/// consumers get source text exclusively through `as_str`.
#[derive(Debug, Clone, serde::Deserialize, PartialEq)]
#[serde(transparent)]
pub(crate) struct PreservedText(String);

impl PreservedText {
    fn needs_mark(blank: bool, first: Option<char>) -> bool {
        blank || first == Some(WS_SENTINEL)
    }

    pub(crate) fn new(source: String) -> Self {
        let blank = !source.is_empty() && source.chars().all(is_xml_space);
        // 이것을 실패시키는 것: 생성자에서 원문을 그대로 저장하기.
        if Self::needs_mark(blank, source.chars().next()) {
            Self(format!("{WS_SENTINEL}{source}"))
        } else {
            Self(source)
        }
    }

    /// Removes exactly one transport mark; the returned source is never
    /// passed through this operation again, even when it looks like an escape.
    pub(crate) fn as_str(&self) -> &str {
        let s = self.0.as_str();
        if let Some(rest) = s.strip_prefix(WS_SENTINEL) {
            // 이것을 실패시키는 것: 이중 표식 복원 생략 또는 모든 선두 표식 제거.
            if rest.starts_with(WS_SENTINEL) || (!rest.is_empty() && rest.chars().all(is_xml_space))
            {
                return rest;
            }
        }
        s
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

    // 이것을 실패시키는 것: 바뀐 것이 없어도 `Owned` 로 새로 할당하는 것.
    #[test]
    fn unchanged_input_is_borrowed() {
        for src in ["<a x=\"1\">t</a>", "<a x=\"&amp;\">&#13;</a>", ""] {
            assert!(matches!(norm(src), Cow::Borrowed(_)), "{src}");
        }
    }
}

#[cfg(test)]
mod clean_font_name_tests {
    use super::clean_font_name;

    // 이것을 실패시키는 것: 제어 문자만 지우고 앞뒤를 자르지 않는 것, 가운데 공백까지 지우는 것,
    // 가운데 제어 문자를 남기는 것, 또는 앞뒤 자르기를 유니코드 공백 전체로 넓히는 것.
    #[test]
    fn control_characters_go_and_only_edge_spaces_are_trimmed() {
        for (raw, want) in [
            ("함초롬바탕\r", "함초롬바탕"),
            ("함초롬\r바탕", "함초롬바탕"),
            ("HancomEQN\n", "HancomEQN"),
            ("  Times New Roman\t ", "Times New Roman"),
            (" \r 윤명조440", "윤명조440"),
            ("Times \u{1}New", "Times New"),
            ("\r\n", ""),
            ("함초롬바탕", "함초롬바탕"),
            ("\u{3000}명조\u{a0}", "\u{3000}명조\u{a0}"),
        ] {
            assert_eq!(clean_font_name(raw), want, "{raw:?}");
        }
    }
}

#[cfg(test)]
mod ws_scanner_tests {
    use super::*;

    fn preserve_ws_only_text(xml: &str) -> Cow<'_, str> {
        super::preserve_ws_only_text(xml).expect("namespace scan")
    }

    // 이것을 실패시키는 것: 선두 원문 표식 이스케이프 생략, CDATA/참조의 첫 문자
    // 누락, 이전 첫 문자 덮어쓰기, 또는 두 표식 복원 시 전부 제거하기.
    #[test]
    fn source_marks_are_reversibly_escaped_across_character_data() {
        for (wire, decoded) in [
            ("\u{E000} ", "\u{E000} "),
            ("&#xE000; ", "\u{E000} "),
            ("&#57344; ", "\u{E000} "),
            ("&#x000e000; ", "\u{E000} "),
            ("<![CDATA[\u{E000} ]]>", "\u{E000} "),
            ("<!--x--><?pi x?><![CDATA[]]>&#xE000; ", "\u{E000} "),
            ("\u{E000}<!--x--><![CDATA[ ]]>&#32;", "\u{E000}  "),
            ("<![CDATA[\u{E000}]]><!--x-->끝", "\u{E000}끝"),
            ("\u{E000}", "\u{E000}"),
            ("\u{E000}\u{E000} ", "\u{E000}\u{E000} "),
            ("&#xE000;&#xE000;&#xE000;끝", "\u{E000}\u{E000}\u{E000}끝"),
            ("&#xE000;끝", "\u{E000}끝"),
            ("&amp;#xE000; ", "&#xE000; "),
            ("x<![CDATA[\u{E000} ]]>", "x\u{E000} "),
            ("<![CDATA[x]]>&#xE000; ", "x\u{E000} "),
            (" <!--x-->&#xE000; ", " \u{E000} "),
        ] {
            // Foreign/unknown t is still accepted by serde's local-name mapping.
            for tag in ["t", "hp:t", "q:t xmlns:q='urn:foreign'", "z:t"] {
                let end = tag.split_whitespace().next().unwrap();
                let xml = format!("<{tag}>{wire}</{end}>");
                let marked = preserve_ws_only_text(&xml);
                let want = if decoded.starts_with(WS_SENTINEL) {
                    format!("<{tag}>{WS_SENTINEL}{wire}</{end}>")
                } else {
                    xml.clone()
                };
                assert_eq!(marked, want, "{wire:?}");
                let hx: crate::schema::section::HxText =
                    crate::decoder::xml_from_str(&marked).unwrap();
                assert_eq!(hx.text(), decoded, "{wire:?}");
            }
        }
        let script = "<hp:script>\u{E000} </hp:script>";
        assert_eq!(preserve_ws_only_text(script), script);
    }

    // 이것을 실패시키는 것: 접두사 철자 비교, URI 무시, local name 무시,
    // 무접두 호환 삭제, 미선언 접두사 허용, 닫는/빈 요소에서 xmlns 범위 복구 생략,
    // 또는 참조로 쓴 URI를 원문 바이트로 비교하기.
    #[test]
    fn namespace_uri_local_name_and_scope_select_blank_text() {
        let ns = "http://www.hancom.co.kr/hwpml/2011/paragraph";
        for (xml, count) in [
            (format!("<q:t xmlns:q='{ns}'>  <q:fwSpace/>끝</q:t>"), 1),
            (format!("<t xmlns='{ns}'>  <fwSpace/>끝</t>"), 1),
            ("<t>  <fwSpace/>끝</t>".into(), 1),
            ("<hp:t>  <hp:fwSpace/>끝</hp:t>".into(), 1),
            ("<q:t>  <q:fwSpace/>끝</q:t>".into(), 0),
            ("<hp:t xmlns:hp='urn:foreign'>  <hp:fwSpace/>끝</hp:t>".into(), 0),
            ("<t xmlns='urn:foreign'>  <fwSpace/>끝</t>".into(), 0),
            (format!("<q:tbl xmlns:q='{ns}'> </q:tbl>"), 0),
            (
                format!(
                    "<r xmlns:q='{ns}'><x xmlns:q='urn:foreign'><q:t> </q:t></x><q:t> </q:t></r>"
                ),
                1,
            ),
            (format!("<r xmlns:q='{ns}'><x xmlns:q='urn:foreign'/><q:t> </q:t></r>"), 1),
            (format!("<r xmlns='{ns}'><x xmlns='urn:foreign'><t> </t></x><t> </t></r>"), 1),
            ("<r xmlns='urn:foreign'><t xmlns=''> </t><t> </t></r>".into(), 1),
            (format!("<q:t xmlns:q='{}'> </q:t>", ns.replace("paragraph", "paragr&#x61;ph")), 1),
        ] {
            let marked = preserve_ws_only_text(&xml);
            assert_eq!(marked.matches(WS_SENTINEL).count(), count, "{xml}");
            assert_eq!(marked.replace(WS_SENTINEL, ""), xml);
        }
    }

    // 이것을 실패시키는 것: 네임스페이스 오류를 무시하고 부분 수정 XML을 반환하기.
    #[test]
    fn namespace_errors_do_not_return_partially_marked_xml() {
        for binding in ["xmlns:xml='urn:invalid'", "xmlns:q='urn:&unknown;'"] {
            let xml = format!("<r><t> </t><q:t {binding}> </q:t></r>");
            assert!(super::preserve_ws_only_text(&xml).is_err());
        }
    }

    // 이것을 실패시키는 것: 선언의 URI 해석을 매 요소 조회 때 반복하기.
    // 입력 K배 긴 URI와 K개 요소에서 실제 해석 바이트/접두사 조회를 센다.
    #[test]
    fn namespace_uri_work_is_linear_in_declaration_and_element_bytes() {
        let mut totals = Vec::new();
        for k in [1_000, 2_000, 4_000] {
            let uri = format!("urn:{}", "&#97;".repeat(k));
            let xml = format!("<r xmlns:q='{uri}'>{}</r>", "<q:x/>".repeat(k));
            let mut work = NamespaceWork::default();
            assert_eq!(preserve_ws_only_text_counted(&xml, &mut work).unwrap(), xml);
            assert_eq!(work.classifications, 1);
            assert_eq!(work.uri_bytes, 4 + 5 * k);
            assert_eq!(work.binding_probes, k + 4);
            totals.push(work.uri_bytes + work.binding_probes);
        }
        assert_eq!(totals, [6_008, 12_008, 24_008]);
    }

    // 이것을 실패시키는 것: 캐시를 접두사별 전역 값으로 두어 재선언/해제를
    // 덮어쓰기, 닫는/빈 요소의 범위 복구 생략, 또는 선언 없이 URI를 재분류하기.
    #[test]
    fn namespace_cache_restores_shadowed_and_undeclared_bindings() {
        let ns = PARAGRAPH_NS.replace("paragraph", "paragr&#97;ph");
        let xml = format!(
            "<r xmlns:q='{ns}' xmlns='{ns}'>\
             <q:t> </q:t><q:t xmlns:q=''> </q:t><q:t> </q:t>\
             <x xmlns:q='urn:foreign'><q:t> </q:t></x><q:t> </q:t>\
             <x xmlns:q='urn:other'/><q:t> </q:t>\
             <x xmlns='urn:foreign'><t xmlns=''> </t><t> </t></x><t> </t>\
             <xml:t> </xml:t><xmlns:t> </xmlns:t></r>"
        );
        let mut work = NamespaceWork::default();
        let out = preserve_ws_only_text_counted(&xml, &mut work).unwrap();
        assert_eq!(out.matches(WS_SENTINEL).count(), 6);
        assert_eq!(out.replace(WS_SENTINEL, ""), xml);
        assert_eq!(work.classifications, 7);
        assert_eq!(work.uri_bytes, 2 * ns.len() + 2 * "urn:foreign".len() + "urn:other".len());
    }

    // 이것을 실패시키는 것: resolver가 저장하지 않는 예약 xml 재선언을
    // 캐시에 쌓아, 긴 형제 요소열의 조회량이 다시 제곱으로 증가하기.
    #[test]
    fn namespace_cache_excludes_reserved_redeclarations() {
        for k in [1_000, 2_000, 4_000] {
            let start = "<r xmlns:xml='http://www.w3.org/XML/1998/namespace'>";
            let xml = format!("{}{}{}", start.repeat(k), "<t> </t>".repeat(k), "</r>".repeat(k));
            let mut work = NamespaceWork::default();
            let out = preserve_ws_only_text_counted(&xml, &mut work).unwrap();
            assert_eq!(out.matches(WS_SENTINEL).count(), k);
            assert_eq!(work.classifications, 0);
            assert_eq!(work.binding_probes, 6 * k);
        }
    }

    // An unclosed comment runs to the end for both scanners that share
    // `markup_len`: nothing after `<!--` is marked or rewritten (the parser
    // then reports the malformed input).
    // 이것을 실패시키는 것: 닫히지 않은 주석에서 여는 토큰 뒤부터 다시 훑는 것.
    #[test]
    fn an_unclosed_comment_hides_the_rest_from_both_scanners() {
        let marked = "<!-- <hp:t> </hp:t>";
        assert_eq!(preserve_ws_only_text(marked), marked);
        let attr = r#"<!-- <a x="A&#9;B"/>"#;
        assert_eq!(normalize_attr_control_whitespace(attr), attr);
    }

    // 이것을 실패시키는 것: 공백 판정을 빼거나 느슨하게 하는 것 — `a`·`  b` run 에도 표시가
    // 붙는다. 또는 빈 run 을 공백 전용으로 치는 것.
    #[test]
    fn preserve_ws_only_text_marks_only_pure_whitespace() {
        let xml =
            "<hp:run><hp:t>a</hp:t><hp:t> </hp:t><hp:t>  b</hp:t><hp:t></hp:t><hp:t/></hp:run>";
        let out = preserve_ws_only_text(xml);
        assert_eq!(
            out,
            "<hp:run><hp:t>a</hp:t><hp:t>\u{E000} </hp:t><hp:t>  b</hp:t><hp:t></hp:t><hp:t/></hp:run>"
        );
    }

    // An element whose name only starts like the text element (`<hp:tbl>`)
    // is not read as text; spaces directly inside `<hp:t>` are characters.
    // 이것을 실패시키는 것: 요소 이름 뒤 글자 검사를 빼는 것 — `<hp:tbl>` 을 run 으로 읽어
    // 첫 `</hp:t>` 까지 건너뛰므로 그 안의 진짜 공백 run 이 표시되지 않는다.
    #[test]
    fn preserve_ws_only_text_distinguishes_text_from_structural_spaces() {
        let xml = "<hp:t> <hp:tab/></hp:t><hp:tbl> </hp:tbl>";
        assert_eq!(preserve_ws_only_text(xml), "<hp:t>\u{E000} <hp:tab/></hp:t><hp:tbl> </hp:tbl>");
        assert_eq!(
            preserve_ws_only_text("<hp:tbl><hp:t> </hp:t></hp:tbl>"),
            "<hp:tbl><hp:t>\u{E000} </hp:t></hp:tbl>"
        );
    }

    // 이것을 실패시키는 것: 선두·중간·꼬리 중 한 구간을 건너뛰거나, `hp:t` 만 보호하기.
    #[test]
    fn mixed_text_marks_each_blank_entry_in_both_spellings() {
        let xml =
            "<hp:t>  <hp:fwSpace/> &#32;<hp:tab/>끝<hp:lineBreak/>   </hp:t><t> <fwSpace/> </t>";
        assert_eq!(
            preserve_ws_only_text(xml),
            "<hp:t>\u{E000}  <hp:fwSpace/>\u{E000} &#32;<hp:tab/>끝<hp:lineBreak/>\u{E000}   </hp:t><t>\u{E000} <fwSpace/>\u{E000} </t>"
        );
    }

    // 이것을 실패시키는 것: 주석에서 글자 구간을 끊기, 또는 CDATA 를 무시하기.
    #[test]
    fn adjacent_character_data_gets_one_mark_only_when_all_blank() {
        for (body, blank) in [
            (" <!--x--> <![CDATA[ ]]> &#32;<?pi x?>", true),
            ("<![CDATA[  ]]>", true),
            (" <!--x-->끝", false),
            (" <![CDATA[끝]]>", false),
            ("<![CDATA[&#32;]]>", false),
            ("<!--x--><?pi x?><![CDATA[]]>", false),
        ] {
            let xml = format!("<hp:t>{body}<hp:fwSpace/></hp:t>");
            let mark = if blank { "\u{E000}" } else { "" };
            assert_eq!(
                preserve_ws_only_text(&xml),
                format!("<hp:t>{mark}{body}<hp:fwSpace/></hp:t>")
            );
        }
    }

    // 이것을 실패시키는 것: 부모가 `<t>` 인지 확인하지 않기, 또는 속성 안 `>` 에서 태그를 끊기.
    #[test]
    fn indentation_and_child_payloads_are_not_text_entries() {
        let xml =
            "<sec>\n <p> <run> <hp:t attr='>'> <hp:tab> </hp:tab> </hp:t> </run> </p>\n</sec>";
        assert_eq!(preserve_ws_only_text(xml), "<sec>\n <p> <run> <hp:t attr='>'>\u{E000} <hp:tab> </hp:tab>\u{E000} </hp:t> </run> </p>\n</sec>");
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

#[cfg(test)]
mod forbidden_char_tests {
    use super::strip_xml_forbidden_chars;
    use std::borrow::Cow;

    fn strip(xml: &str) -> (String, Vec<(String, usize)>) {
        let (out, sites) = strip_xml_forbidden_chars(xml);
        (out.into_owned(), sites)
    }

    fn site(location: &str, count: usize) -> (String, usize) {
        (location.to_string(), count)
    }

    #[test]
    fn an_attribute_value_is_located_as_element_at_attribute() {
        // 이것을 실패시키는 것: 따옴표 안 금지 문자를 `{name}@{attr}` 대신 요소 이름으로 집계하는 것.
        let (out, sites) = strip("<a x=\"1\" y=\"p\u{1}q\"/>");
        assert_eq!(out, r#"<a x="1" y="pq"/>"#);
        assert_eq!(sites, vec![site("a@y", 1)]);
    }

    #[test]
    fn text_after_a_child_closes_belongs_to_the_parent() {
        // 이것을 실패시키는 것: 끝 태그에서 `open.pop()` 을 빼는 것.
        let (out, sites) = strip("<p><t>a\u{1}</t>b\u{2}</p>");
        assert_eq!(out, "<p><t>a</t>b</p>");
        assert_eq!(sites, vec![site("t", 1), site("p", 1)]);
    }

    #[test]
    fn a_self_closing_element_is_not_a_parent() {
        // 이것을 실패시키는 것: `/>` 로 끝난 태그도 스택에 올리는 것.
        let (_, sites) = strip("<p><br/>x\u{1}</p>");
        assert_eq!(sites, vec![site("p", 1)]);
    }

    #[test]
    fn the_two_noncharacters_and_nul_are_removed() {
        // 이것을 실패시키는 것: `is_xml_forbidden_char` 에서 U+FFFE·U+FFFF·U+0000 중 하나를 빼는 것.
        let (out, sites) = strip("<t>a\u{FFFE}b\u{FFFF}c\u{0}d</t>");
        assert_eq!(out, "<t>abcd</t>");
        assert_eq!(sites, vec![site("t", 3)]);
    }

    #[test]
    fn tab_lf_cr_are_kept_and_the_input_is_borrowed() {
        // 이것을 실패시키는 것: `is_xml_forbidden_char` 범위에 TAB·LF·CR 을 넣는 것.
        let xml = "<t a=\"\t\n\r\">\t\n\r</t>";
        let (out, sites) = strip_xml_forbidden_chars(xml);
        assert!(matches!(out, Cow::Borrowed(_)));
        assert!(sites.is_empty());
    }

    #[test]
    fn removals_merge_per_location_in_first_seen_order() {
        // 이것을 실패시키는 것: `tally_removed` 가 같은 location 을 합치지 않고 새로 push 하는 것.
        let (_, sites) =
            strip("<r><t>\u{1}</t><s n=\"\u{1}\"/><t>\u{1}\u{1}</t><s n=\"\u{1}\"/></r>");
        assert_eq!(sites, vec![site("t", 3), site("s@n", 2)]);
    }

    #[test]
    fn markup_inside_an_element_belongs_to_that_element() {
        // 이것을 실패시키는 것: 주석·CDATA 구간을 금지 문자 검사 없이 그대로 복사하는 것.
        let (out, sites) = strip("<t><!--c\u{1}--><![CDATA[d\u{1}]]></t>");
        assert_eq!(out, "<t><!--c--><![CDATA[d]]></t>");
        assert_eq!(sites, vec![site("t", 2)]);
    }

    #[test]
    fn a_quote_of_the_other_kind_does_not_end_the_value() {
        // 이것을 실패시키는 것: 여는 따옴표와 다른 종류의 따옴표에서도 값을 닫는 것.
        let (_, sites) = strip("<a x=\"it's\u{1}\"/>");
        assert_eq!(sites, vec![site("a@x", 1)]);
    }
}

#[cfg(test)]
mod edit_text_tests {
    use super::sanitize_edit_text;
    use crate::EncodeWarning;

    fn run(text: &str) -> (String, Vec<EncodeWarning>) {
        let mut warnings = Vec::new();
        let out = sanitize_edit_text(text, "Contents/section0.xml", "field 'f'", &mut warnings);
        (out, warnings)
    }

    fn removed(count: usize) -> Vec<EncodeWarning> {
        vec![EncodeWarning::XmlForbiddenCharsRemoved {
            part: "Contents/section0.xml".to_string(),
            location: "field 'f'".to_string(),
            count,
        }]
    }

    #[test]
    fn nul_and_the_other_controls_are_removed_and_counted_once() {
        // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 시작하게 하는 것 (NUL 이 남는다).
        assert_eq!(run("A\u{0}B\u{1}C\u{FFFF}D"), ("ABCD".to_string(), removed(3)));
    }

    #[test]
    fn u001f_tab_lf_cr_stay_because_the_inline_encoder_carries_them() {
        // 이것을 실패시키는 것: `is_edit_unrepresentable_char` 에서 U+001F 제외를 빼는 것,
        // 또는 `sanitize_edit_text` 의 `count > 0` 가드를 빼는 것 (지운 것 없는 값이 경고가 된다).
        assert_eq!(run("A\u{1F}B\tC\nD\rE"), ("A\u{1F}B\tC\nD\rE".to_string(), Vec::new()));
    }

    #[test]
    fn a_value_of_only_forbidden_characters_becomes_empty() {
        assert_eq!(run("\u{1}\u{2}"), (String::new(), removed(2)));
    }
}

#[cfg(test)]
mod sentinel_invariant_tests {
    use super::*;
    use crate::schema::section::{HxText, HxTextPart};
    use proptest::prelude::*;

    fn decode_text(xml: &str) -> HxText {
        crate::decoder::xml_from_str(&preserve_ws_only_text(xml).unwrap()).unwrap()
    }

    fn assert_plain_round_trip(source: &str) {
        use hwpforge_core::{Document, ImageStore, PageSettings, Paragraph, Run, Section};
        use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};

        let text = HxText::new(source);
        assert_eq!(text.text(), source);
        // Exercise the text-only serde boundary directly, as permitted for tests
        // by .clippy.toml; HxText has no attributes to normalize here.
        #[allow(clippy::disallowed_methods)]
        let xml = quick_xml::se::to_string_with_root("hp:t", &text).unwrap();
        assert!(!xml.contains("<hp:tab"));
        // Decimal/hex references must enter the same transport representation
        // as literal characters. No inline child can bypass HxText::serialize.
        for xml in [xml.clone(), xml.replace(WS_SENTINEL, "&#xE000;")] {
            let decoded = decode_text(&xml);
            assert_eq!(decoded.text(), source);
            assert_eq!(decoded, text);
        }

        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text(source, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let styles = crate::style_store_for_preset("default").unwrap();
        let bytes =
            crate::HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new())
                .unwrap();
        let decoded = crate::HwpxDecoder::decode(&bytes).unwrap();
        assert_eq!(decoded.document.sections()[0].paragraphs[0].text_content(), source);
        let bytes = crate::HwpxEncoder::encode(
            &decoded.document.clone().validate().unwrap(),
            &decoded.style_store,
            &decoded.image_store,
        )
        .unwrap();
        assert_eq!(crate::HwpxDecoder::decode(&bytes).unwrap().document, decoded.document);
    }

    proptest! {
        #![proptest_config(ProptestConfig { failure_persistence: None, ..ProptestConfig::with_cases(64) })]

        // 이것을 실패시키는 것: HxText::new 원문 이스케이프 생략 — 인라인 없는
        // &#xE000;&#xE000;끝 경로가 재인코딩 시 표식을 하나 잃는다.
        #[test]
        fn repeated_marks_without_inline_round_trip(count in 2usize..65) {
            for count in [2, count] {
                assert_plain_round_trip(&format!("{}끝", "\u{E000}".repeat(count)));
            }
        }

        // 이것을 실패시키는 것: 표식만 있는 원문을 빈 문자열로 보거나,
        // 되돌릴 때 선두 표식을 하나가 아닌 전부 제거하기.
        #[test]
        fn marks_only_round_trip(count in 1usize..65) {
            for count in [1, count] {
                assert_plain_round_trip(&"\u{E000}".repeat(count));
            }
        }

        // 이것을 실패시키는 것: 이미 이스케이프된 꼴의 원문은 재이스케이프하지
        // 않기, 원문에 포함된 문자참조 글자열을 참조로 풀기, 또는 이중 복원.
        #[test]
        fn escape_shaped_source_round_trip(
            count in 1usize..33,
            tail in prop::collection::vec(prop::sample::select(vec![
                " ", "끝", "&#xE000;", "&#57344;", "&", "<", "\u{E000}", "😀",
            ]), 0..24),
        ) {
            for suffix in [" ".repeat(count), format!("\u{E000}{}", tail.concat())] {
                assert_plain_round_trip(&format!("\u{E000}{suffix}"));
            }
        }

        // 이것을 실패시키는 것: 인라인 텍스트 생성·역직렬화가 전용 타입을
        // 우회하거나, 속성 탭의 Plain 투영에서 표식을 복원하지/두 번 복원하기.
        #[test]
        fn inline_parts_share_the_text_invariant(count in 1usize..33, spaces in 1usize..16) {
            assert_plain_round_trip(&" ".repeat(spaces));
            let source = format!("{}{}", "\u{E000}".repeat(count), " ".repeat(spaces));
            let text = HxText { parts: vec![
                HxTextPart::Text(PreservedText::new(source.clone())),
                HxTextPart::Tab { width: Some(1200), leader: Some(1), tab_type: Some(2) },
                HxTextPart::Text(PreservedText::new(source.clone())),
            ] };
            let xml = format!("<hp:t>{source}<hp:tab width='1200' leader='1' type='2'/>{source}</hp:t>");
            let decoded = decode_text(&xml);
            prop_assert_eq!(&decoded, &text);
            let expected = format!("{source}\t{source}");
            prop_assert_eq!(text.text(), expected.as_str());
            prop_assert_eq!(decoded.to_run_content().plain_text().unwrap().into_owned(), expected);
        }
    }
}
