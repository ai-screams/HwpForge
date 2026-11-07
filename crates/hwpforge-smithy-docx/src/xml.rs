//! A bounded event walk; no document tree or format interpretation lives here.

use crate::error::{quote, DocxError, Result};
use quick_xml::{
    events::{BytesRef, Event},
    name::QName,
    reader::Reader,
};
use std::{
    collections::{hash_map::RandomState, HashMap, HashSet},
    hash::BuildHasher,
    rc::Rc,
};

pub(crate) const MAX_DEPTH: usize = 128;
pub(crate) const MAX_ATTRIBUTES: usize = 256;
pub(crate) const MAX_VALUE_BYTES: usize = 1024 * 1024;
pub(crate) const MAX_TEXT_BYTES: usize = 64 * 1024 * 1024;
const STRICT_PREFIX: &str = "http://purl.oclc.org/ooxml/";

pub(crate) fn reject_strict(part: &str, element: &str, uri: &str) -> Result<()> {
    if uri.starts_with(STRICT_PREFIX) {
        return Err(DocxError::invalid(
            part,
            element,
            format!("Strict OOXML URI '{}'; expected Transitional OOXML", quote(uri)),
        ));
    }
    Ok(())
}

fn check(
    part: &str,
    element: &str,
    resource: &'static str,
    observed: usize,
    limit: usize,
) -> Result<()> {
    if observed > limit {
        return Err(DocxError::Limit {
            part: quote(part),
            element: quote(element),
            resource,
            observed,
            limit,
        });
    }
    Ok(())
}

/// Share one counter across all XML parts, including excluded content.
#[derive(Debug, Default)]
pub(crate) struct TextBudget {
    bytes: usize,
    #[cfg(test)]
    namespace_hash_bytes: usize,
    #[cfg(test)]
    namespace_comparison_bytes: usize,
    #[cfg(test)]
    prefix_lookups: usize,
}
impl TextBudget {
    pub(crate) fn add(&mut self, part: &str, element: &str, bytes: usize) -> Result<()> {
        let observed = self.bytes.saturating_add(bytes);
        check(part, element, "expanded UTF-8 text bytes", observed, MAX_TEXT_BYTES)?;
        self.bytes = observed;
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Element {
    // URI text is retained only for diagnostics; identity uses the walk-local ID.
    pub(crate) namespace: Rc<str>,
    pub(crate) namespace_id: NamespaceId,
    pub(crate) name: String,
    pub(crate) attributes: Vec<(NamespaceId, String, String)>,
}
impl Element {
    /// OPC attributes are unqualified even when their elements have a prefix.
    pub(crate) fn attribute(&self, name: &str) -> Option<&str> {
        self.attributes
            .iter()
            .find(|(ns, key, _)| *ns == NamespaceId::NONE && key == name)
            .map(|(_, _, value)| value.as_str())
    }
}

#[derive(Debug)]
pub(crate) enum XmlEvent {
    Start(Element),
    End,
    /// Character data including references and CDATA; comments/PIs do not split it.
    Text(String),
}

/// Identity is valid only within one walk; the predefined IDs are stable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct NamespaceId(usize);
impl NamespaceId {
    pub(crate) const NONE: Self = Self(0);
    pub(crate) const XML: Self = Self(1);
    const XMLNS: Self = Self(2);
}
const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
pub(crate) const REL_NS: (NamespaceId, &str) =
    (NamespaceId(3), "http://schemas.openxmlformats.org/package/2006/relationships");
pub(crate) const TYPE_NS: (NamespaceId, &str) =
    (NamespaceId(4), "http://schemas.openxmlformats.org/package/2006/content-types");

pub(crate) const W_NS: (NamespaceId, &str) =
    (NamespaceId(5), "http://schemas.openxmlformats.org/wordprocessingml/2006/main");
pub(crate) const R_NS: (NamespaceId, &str) =
    (NamespaceId(6), "http://schemas.openxmlformats.org/officeDocument/2006/relationships");
pub(crate) const MC_NS: (NamespaceId, &str) =
    (NamespaceId(7), "http://schemas.openxmlformats.org/markup-compatibility/2006");
pub(crate) const M_NS: (NamespaceId, &str) =
    (NamespaceId(8), "http://schemas.openxmlformats.org/officeDocument/2006/math");

pub(crate) const A_NS: (NamespaceId, &str) =
    (NamespaceId(9), "http://schemas.openxmlformats.org/drawingml/2006/main");

// Hash only on registration, then verify exact content within that hash bucket.
// Keeping the hash as the map key also avoids rehashing long URIs as the map grows.
struct Namespaces {
    uris: Vec<Rc<str>>,
    by_hash: HashMap<u64, Vec<NamespaceId>>,
    hasher: RandomState,
    bindings: HashMap<String, Vec<NamespaceId>>,
    levels: Vec<Vec<String>>,
}
impl Default for Namespaces {
    fn default() -> Self {
        let hasher = RandomState::new();
        let uris: Vec<Rc<str>> =
            ["", XML_NS, XMLNS_NS, REL_NS.1, TYPE_NS.1, W_NS.1, R_NS.1, MC_NS.1, M_NS.1, A_NS.1]
                .into_iter()
                .map(Rc::from)
                .collect();
        let mut by_hash: HashMap<u64, Vec<NamespaceId>> = HashMap::new();
        for (index, uri) in uris.iter().enumerate() {
            by_hash.entry(hasher.hash_one(uri.as_ref())).or_default().push(NamespaceId(index));
        }
        Self { uris, by_hash, hasher, bindings: HashMap::new(), levels: Vec::new() }
    }
}

fn namespace_uri_equal(left: &str, right: &str, _budget: &mut TextBudget) -> bool {
    #[cfg(test)]
    if left.len() == right.len() {
        // Instrument only; the production equality below is also the test verdict.
        let common = left.bytes().zip(right.bytes()).take_while(|(a, b)| a == b).count();
        _budget.namespace_comparison_bytes += (common + 1).min(left.len());
    }
    left == right
}

impl Namespaces {
    fn register(
        &mut self,
        part: &str,
        element: &str,
        value: String,
        budget: &mut TextBudget,
    ) -> Result<NamespaceId> {
        #[cfg(test)]
        {
            budget.namespace_hash_bytes += value.len();
        }
        let hash = self.hasher.hash_one(value.as_str());
        self.register_hashed(part, element, value, hash, budget)
    }

    fn register_hashed(
        &mut self,
        part: &str,
        element: &str,
        value: String,
        hash: u64,
        budget: &mut TextBudget,
    ) -> Result<NamespaceId> {
        let bucket = self.by_hash.entry(hash).or_default();
        for &id in bucket.iter() {
            if namespace_uri_equal(&self.uris[id.0], &value, budget) {
                return Ok(id);
            }
        }
        reject_strict(part, element, &value)?;
        let id = NamespaceId(self.uris.len());
        self.uris.push(Rc::from(value));
        bucket.push(id);
        Ok(id)
    }

    fn push(&mut self) {
        self.levels.push(Vec::new());
    }
    fn pop(&mut self) {
        for prefix in self.levels.pop().unwrap() {
            let stack = self.bindings.get_mut(&prefix).unwrap();
            stack.pop();
            if stack.is_empty() {
                self.bindings.remove(&prefix);
            }
        }
    }
    fn add(
        &mut self,
        part: &str,
        element: &str,
        prefix: &str,
        value: String,
        budget: &mut TextBudget,
    ) -> Result<()> {
        let id = self.register(part, element, value, budget)?;
        if (prefix == "xml") != (id == NamespaceId::XML)
            || prefix == "xmlns"
            || id == NamespaceId::XMLNS
        {
            return Err(DocxError::invalid(part, element, "invalid reserved namespace binding"));
        }
        if !prefix.is_empty() && id == NamespaceId::NONE {
            return Err(DocxError::invalid(
                part,
                element,
                "prefixed namespace declaration must not be empty",
            ));
        }
        self.bindings.entry(prefix.into()).or_default().push(id);
        self.levels.last_mut().unwrap().push(prefix.into());
        Ok(())
    }
    fn lookup(&self, prefix: &str, _budget: &mut TextBudget) -> Option<NamespaceId> {
        self.bindings.get(prefix).and_then(|stack| {
            stack
                .last()
                .inspect(|_| {
                    // Count inspected binding entries, not calls (a miss inspects none).
                    #[cfg(test)]
                    {
                        _budget.prefix_lookups += 1;
                    }
                })
                .copied()
        })
    }
    fn resolve(
        &self,
        part: &str,
        element: &str,
        name: QName<'_>,
        attribute: bool,
        budget: &mut TextBudget,
    ) -> Result<NamespaceId> {
        let prefix = name.prefix();
        if attribute && prefix.is_none() {
            return Ok(NamespaceId::NONE);
        }
        let prefix = prefix.map(|p| p.into_inner()).unwrap_or("");
        if prefix == "xml" {
            return Ok(NamespaceId::XML);
        }
        if let Some(id) = self.lookup(prefix, budget) {
            return Ok(id);
        }
        if prefix.is_empty() {
            return Ok(NamespaceId::NONE);
        }
        Err(DocxError::invalid(
            part,
            element,
            format!("unbound namespace prefix '{}'", quote(prefix)),
        ))
    }
}

// XML 1.0 fifth edition [4]/[4a], excluding ':' (Namespaces 1.0 NCName).
fn ncname_start(ch: char) -> bool {
    matches!(ch, 'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}'
        | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}' | '\u{370}'..='\u{37D}'
        | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}'
        | '\u{FDF0}'..='\u{FFFD}' | '\u{10000}'..='\u{EFFFF}')
}
fn ncname(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(ncname_start)
        && chars.all(|ch| {
            ncname_start(ch)
                || matches!(ch, '-' | '.' | '0'..='9' | '\u{B7}'
            | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
        })
}
fn check_qname(part: &str, element: &str, name: &str) -> Result<()> {
    for identifier in name.split(':') {
        crate::identifier::check(identifier, part, element)?;
    }
    let valid = match name.split_once(':') {
        Some((prefix, local)) => ncname(prefix) && ncname(local),
        None => ncname(name),
    };
    if !valid {
        return Err(DocxError::invalid(part, element, format!("invalid QName '{}'", quote(name))));
    }
    Ok(())
}

fn reference(part: &str, element: &str, raw: &str) -> Result<char> {
    if let Some(ch) = BytesRef::new(raw)
        .resolve_char_ref()
        .map_err(|e| DocxError::invalid(part, element, quote(e)))?
    {
        return Ok(ch);
    }
    quick_xml::escape::resolve_xml_entity(raw).and_then(|s| s.chars().next())
        .ok_or_else(|| DocxError::invalid(part, element, format!("unsupported entity '&{};'; only numeric and five predefined references are allowed", quote(raw))))
}

/// XML 1.0 normalization over a borrowed slice, without a result buffer.
/// Only numeric and five predefined references are allowed: their UTF-8 output
/// is no longer than their spelling. CRLF contracts and all other chars retain
/// their byte length (attribute whitespace becomes one ASCII space).
fn normalized_chars(
    raw: &str,
    attribute: bool,
    part: &str,
    element: &str,
    mut emit: impl FnMut(char) -> Result<()>,
) -> Result<()> {
    let mut rest = raw;
    while let Some(ch) = rest.chars().next() {
        rest = &rest[ch.len_utf8()..];
        let value = match ch {
            '&' if attribute => {
                let end = rest.find(';').ok_or_else(|| {
                    DocxError::invalid(part, element, "unterminated entity reference")
                })?;
                let value = reference(part, element, &rest[..end])?;
                rest = &rest[end + 1..];
                value
            }
            '\r' => {
                rest = rest.strip_prefix('\n').unwrap_or(rest);
                if attribute {
                    ' '
                } else {
                    '\n'
                }
            }
            '\n' | '\t' if attribute => ' ',
            ch => ch,
        };
        emit(value)?;
    }
    Ok(())
}

/// This admission function accepts only borrowed input and scalar counters.
/// Both limits are decided before any normalized value is allocated.
fn admit(
    raw: &str,
    attribute: bool,
    pending: usize,
    part: &str,
    element: &str,
    budget: &mut TextBudget,
) -> Result<usize> {
    let mut length = 0usize;
    normalized_chars(raw, attribute, part, element, |ch| {
        legal_characters(part, element, ch.encode_utf8(&mut [0; 4]))?;
        length = length.saturating_add(ch.len_utf8());
        Ok(())
    })?;
    check(part, element, "value UTF-8 bytes", pending.saturating_add(length), MAX_VALUE_BYTES)?;
    budget.add(part, element, length)?;
    Ok(length)
}

fn normalized(
    raw: &str,
    attribute: bool,
    length: usize,
    part: &str,
    element: &str,
) -> Result<String> {
    let mut value = String::with_capacity(length);
    normalized_chars(raw, attribute, part, element, |ch| {
        value.push(ch);
        Ok(())
    })?;
    Ok(value)
}

fn legal_characters(part: &str, element: &str, value: &str) -> Result<()> {
    if let Some(ch) = value.chars().find(|ch| !matches!(*ch, '\u{9}' | '\u{A}' | '\u{D}' | '\u{20}'..='\u{D7FF}' | '\u{E000}'..='\u{FFFD}' | '\u{10000}'..='\u{10FFFF}')) {
        return Err(DocxError::invalid(part, element, format!("invalid XML 1.0 character U+{:04X}", ch as u32)));
    }
    Ok(())
}

// Geometric growth stays within the already checked single-value ceiling.
fn reserve_value(value: &mut String, additional: usize) {
    let needed = value.len() + additional;
    if needed > value.capacity() {
        let capacity = needed.max(value.capacity().saturating_mul(2)).min(MAX_VALUE_BYTES);
        value.reserve_exact(capacity - value.len());
    }
}

fn append(
    part: &str,
    element: &str,
    pending: &mut String,
    raw: &str,
    budget: &mut TextBudget,
) -> Result<()> {
    let length = admit(raw, false, pending.len(), part, element, budget)?;
    reserve_value(pending, length);
    normalized_chars(raw, false, part, element, |ch| {
        pending.push(ch);
        Ok(())
    })
}

// XML declarations have a closed grammar, unlike arbitrary element attributes.
fn declaration_syntax(raw: &str, part: &str, location: &str) -> Result<()> {
    let invalid = || {
        DocxError::invalid(part, location, "invalid XML declaration; expected version='1.0', optional encoding and standalone in order")
    };
    let mut rest = raw.strip_prefix("xml").ok_or_else(invalid)?;
    let mut order = 0;
    while !rest.is_empty() {
        let trimmed = rest.trim_start_matches([' ', '\t', '\r', '\n']);
        if trimmed.len() == rest.len() {
            return Err(invalid());
        }
        rest = trimmed;
        if rest.is_empty() {
            break;
        }
        let end = rest.find(['=', ' ', '\t', '\r', '\n']).ok_or_else(invalid)?;
        let key = &rest[..end];
        rest = rest[end..].trim_start_matches([' ', '\t', '\r', '\n']);
        rest =
            rest.strip_prefix('=').ok_or_else(invalid)?.trim_start_matches([' ', '\t', '\r', '\n']);
        let delimiter =
            rest.chars().next().filter(|ch| matches!(ch, '\'' | '"')).ok_or_else(invalid)?;
        rest = &rest[1..];
        let end = rest.find(delimiter).ok_or_else(invalid)?;
        let value = &rest[..end];
        rest = &rest[end + 1..];
        order = match (order, key) {
            (0, "version") if value == "1.0" => 1,
            (1, "encoding") => {
                if !value.bytes().next().is_some_and(|ch| ch.is_ascii_alphabetic())
                    || !value.bytes().all(|ch| ch.is_ascii_alphanumeric() || b"._-".contains(&ch))
                {
                    return Err(invalid());
                }
                if !value.eq_ignore_ascii_case("UTF-8") {
                    return Err(DocxError::UnsupportedEncoding {
                        part: quote(part),
                        encoding: quote(value),
                    });
                }
                2
            }
            (1 | 2, "standalone") if matches!(value, "yes" | "no") => 3,
            _ => return Err(invalid()),
        };
    }
    if order == 0 {
        return Err(invalid());
    }
    Ok(())
}

#[derive(PartialEq)]
enum DocumentState {
    Prolog,
    Root,
    Epilog,
}

/// Visit starts/ends and complete text values. The callback owns semantic state.
/// Empty elements become a start/end pair. Comments/PIs do not split text.
/// Errors abort the walk, including errors returned by the visitor.
pub(crate) fn walk(
    bytes: &[u8],
    part: &str,
    budget: &mut TextBudget,
    mut visit: impl FnMut(XmlEvent) -> Result<()>,
) -> Result<()> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        return Err(DocxError::UnsupportedEncoding {
            part: quote(part),
            encoding: "UTF-16".into(),
        });
    }
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    reader.config_mut().check_comments = true;
    let mut stack: Vec<String> = Vec::new();
    let mut state = DocumentState::Prolog;
    let mut first_event = true;
    let mut namespaces = Namespaces::default();
    let mut pending = String::new();
    loop {
        let location = stack.last().map(String::as_str).unwrap_or("#document");
        let event =
            reader.read_event().map_err(|e| DocxError::invalid(part, location, quote(e)))?;
        // Outside the root only literal XML whitespace, comments and PIs are allowed.
        match (&state, &event) {
            (_, Event::Decl(_)) if !first_event => {
                return Err(DocxError::invalid(part, location, "XML declaration must be first"))
            }
            (
                DocumentState::Prolog | DocumentState::Epilog,
                Event::CData(_) | Event::GeneralRef(_),
            ) => {
                return Err(DocxError::invalid(
                    part,
                    location,
                    "reference or CDATA outside document element",
                ))
            }
            (DocumentState::Prolog | DocumentState::Epilog, Event::Text(text)) => {
                if !text.as_ref().chars().all(|ch| matches!(ch, ' ' | '\t' | '\r' | '\n')) {
                    return Err(DocxError::invalid(
                        part,
                        location,
                        "text outside document element",
                    ));
                }
                first_event = false;
                continue;
            }
            _ => {}
        }
        first_event = false;
        match &event {
            Event::Decl(declaration) => {
                declaration_syntax(declaration.as_ref(), part, location)?;
            }
            Event::PI(pi) => {
                if !ncname(pi.target()) || pi.target().eq_ignore_ascii_case("xml") {
                    return Err(DocxError::invalid(
                        part,
                        location,
                        format!("invalid PI target NCName '{}'", quote(pi.target())),
                    ));
                }
            }
            Event::Text(text) => append(part, location, &mut pending, text.as_ref(), budget)?,
            Event::CData(text) => append(part, location, &mut pending, text.as_ref(), budget)?,
            Event::GeneralRef(reference) => {
                let value = self::reference(part, location, reference.as_ref())?;
                let mut buffer = [0; 4];
                let raw = value.encode_utf8(&mut buffer);
                legal_characters(part, location, raw)?;
                check(
                    part,
                    location,
                    "value UTF-8 bytes",
                    pending.len().saturating_add(raw.len()),
                    MAX_VALUE_BYTES,
                )?;
                budget.add(part, location, raw.len())?;
                reserve_value(&mut pending, raw.len());
                pending.push_str(raw);
            }
            Event::DocType(_) => {
                return Err(DocxError::invalid(
                    part,
                    location,
                    "DTD is forbidden (observed 1, limit 0)",
                ))
            }
            Event::Start(_) | Event::End(_) | Event::Eof => {
                if !pending.is_empty() {
                    visit(XmlEvent::Text(std::mem::take(&mut pending)))?;
                }
                match event {
                    Event::Start(start) => {
                        // Reader already checks UTF-8 and matching end-tag spelling.
                        // Validate the start QName once; equal closing names need no second pass.
                        check_qname(part, start.local_name().as_ref(), start.name().into_inner())?;
                        let name = start.local_name().as_ref().to_owned();
                        check(part, &name, "XML depth", stack.len() + 1, MAX_DEPTH)?;
                        if stack.is_empty() {
                            if state == DocumentState::Epilog {
                                return Err(DocxError::invalid(
                                    part,
                                    &name,
                                    "document elements observed 2, limit 1",
                                ));
                            }
                            state = DocumentState::Root;
                        }
                        // First count and admit every attribute, including declarations.
                        // No namespace is copied/registered until this entire pass succeeds.
                        let mut raw_attributes = Vec::new();
                        for (index, attr) in start.attributes().enumerate() {
                            check(part, &name, "attributes", index + 1, MAX_ATTRIBUTES)?;
                            let attr =
                                attr.map_err(|e| DocxError::invalid(part, &name, quote(e)))?;
                            // Includes xmlns:prefix, so an empty/invalid prefix is never registered.
                            check_qname(part, &name, attr.key.into_inner())?;
                            let length = admit(&attr.value, true, 0, part, &name, budget)?;
                            raw_attributes.push((attr, length));
                        }
                        namespaces.push();
                        let mut values = Vec::new();
                        for (attr, length) in &raw_attributes {
                            let value = normalized(&attr.value, true, *length, part, &name)?;
                            if let Some(binding) = attr.key.as_namespace_binding() {
                                let prefix = match binding {
                                    quick_xml::name::PrefixDeclaration::Default => "",
                                    quick_xml::name::PrefixDeclaration::Named(prefix) => prefix,
                                };
                                namespaces.add(part, &name, prefix, value, budget)?;
                            } else {
                                values.push((attr.key, value));
                            }
                        }
                        let ns = namespaces.resolve(part, &name, start.name(), false, budget)?;
                        let mut attributes = Vec::new();
                        let mut expanded = HashSet::new();
                        for (key, value) in values {
                            let local = key.local_name().into_inner();
                            let ns = namespaces.resolve(part, &name, key, true, budget)?;
                            if !expanded.insert((ns, local)) {
                                return Err(DocxError::invalid(
                                    part,
                                    &name,
                                    format!("duplicate expanded attribute '{}'", quote(local)),
                                ));
                            }
                            attributes.push((ns, local.to_owned(), value));
                        }
                        stack.push(name.clone());
                        visit(XmlEvent::Start(Element {
                            namespace: Rc::clone(&namespaces.uris[ns.0]),
                            namespace_id: ns,
                            name,
                            attributes,
                        }))?;
                    }
                    Event::End(_) => {
                        stack.pop();
                        namespaces.pop();
                        if stack.is_empty() {
                            state = DocumentState::Epilog;
                        }
                        visit(XmlEvent::End)?;
                    }
                    Event::Eof => {
                        if state != DocumentState::Epilog {
                            return Err(DocxError::invalid(
                                part,
                                location,
                                "missing or unclosed document element",
                            ));
                        }
                        return Ok(());
                    }
                    _ => unreachable!(),
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
