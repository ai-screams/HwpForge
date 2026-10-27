//! Immutable XML source evidence shared by decoding and preservation.
//! Evidence is obtained from actual source bytes, never a deserialized flag.
use crate::node_path::CorePath;
use quick_xml::{events::Event, Reader};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

thread_local! {
    static SOURCE_WORK: std::cell::Cell<(usize, usize, usize)> = const { std::cell::Cell::new((0, 0, 0)) };
}
/// Cumulative thread-local (XML parses, XML bytes, provenance work units).
#[doc(hidden)]
pub fn source_work() -> (usize, usize, usize) {
    SOURCE_WORK.get()
}
/// Account for evidence attachment / canonical source lookups.
#[doc(hidden)]
pub fn record_provenance_work(units: usize) {
    SOURCE_WORK.set((SOURCE_WORK.get().0, SOURCE_WORK.get().1, SOURCE_WORK.get().2 + units));
}

/// One element in an original XML part.
#[derive(Debug, Clone, Default)]
#[non_exhaustive]
pub struct XmlElement {
    name: String,
    qualified_name: String,
    id_value: Option<Range<usize>>,
    outer: Range<usize>,
    inner: Option<Range<usize>>,
    children: Vec<usize>,
    parent: usize,
}
impl XmlElement {
    /// Original qualified name, including the authored namespace prefix.
    pub fn qualified_name(&self) -> &str {
        &self.qualified_name
    }
    /// Raw `id` attribute value range, excluding its quotes. Recorded during
    /// tokenization, so editors need not locate attributes in source bytes.
    pub fn id_value(&self) -> Option<Range<usize>> {
        self.id_value.clone()
    }
    /// Parent element identity (zero at the root).
    pub fn parent(&self) -> usize {
        self.parent
    }
    /// Local element name.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Original outer byte range.
    pub fn outer(&self) -> Range<usize> {
        self.outer.clone()
    }
    /// Original inner byte range; absent for an empty element.
    pub fn inner(&self) -> Option<Range<usize>> {
        self.inner.clone()
    }
    /// Child element identities in source order.
    pub fn children(&self) -> &[usize] {
        &self.children
    }
}
/// Parsed source bytes, retaining element identities and verified open starts.
#[derive(Debug, Default)]
#[non_exhaustive]
pub struct XmlSource {
    elements: Vec<XmlElement>,
    unclosed: HashMap<usize, String>,
    digest: [u8; 32],
}
/// A source claim, not a validation capability. The HWPX verifier must check
/// its original address against the caller's independent base SourceMap.
/// Core checks local pairing and relocation; it cannot interpret HWPX decoding.
#[derive(Clone)]
#[non_exhaustive]
pub struct UnclosedOrigin {
    path: CorePath,
    id: String,
    source_path: CorePath,
    part: String,
    range: Range<usize>,
    digest: [u8; 32],
    package: Arc<[u8]>,
}
impl std::fmt::Debug for UnclosedOrigin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnclosedOrigin")
            .field("path", &self.path)
            .field("id", &self.id)
            .field("source_path", &self.source_path)
            .field("part", &self.part)
            .field("range", &self.range)
            .field("digest", &self.digest)
            .field("package_bytes", &self.package.len())
            .finish()
    }
}
impl UnclosedOrigin {
    /// Original address before edit-owned index translations.
    pub fn source_path(&self) -> &CorePath {
        &self.source_path
    }
    /// Original package part.
    pub fn part(&self) -> &str {
        &self.part
    }
    /// Original fieldBegin element, not an enclosing run.
    pub fn range(&self) -> Range<usize> {
        self.range.clone()
    }
    /// SHA-256 of the complete original section.
    pub fn digest(&self) -> &[u8; 32] {
        &self.digest
    }
    /// Retained bytes shared by claims from a package. These never select the
    /// verification base; the format verifier uses an independent decoder context.
    pub fn package(&self) -> &[u8] {
        &self.package
    }
    #[cfg(test)]
    pub(crate) fn with_package(&self, package: Arc<[u8]>) -> Option<Self> {
        Self::with_package_batch(std::slice::from_ref(self), package).pop()
    }

    /// Re-read the independent original package for this verification operation.
    /// Each referenced part is parsed once within the call; no parsed evidence
    /// persists across calls. Checks fieldBegin, ID, exact range and no matching end.
    pub fn verify_package_batch(origins: &[Self], package: &[u8]) -> bool {
        Self::package_matches(origins, package).into_iter().all(|valid| valid)
    }

    pub(crate) fn with_package_batch(origins: &[Self], package: Arc<[u8]>) -> Vec<Self> {
        origins
            .iter()
            .zip(Self::package_matches(origins, &package))
            .filter(|(_, valid)| *valid)
            .map(|(origin, _)| {
                let mut result = origin.clone();
                result.package = package.clone();
                result
            })
            .collect()
    }

    fn package_matches(origins: &[Self], package: &[u8]) -> Vec<bool> {
        let Ok(mut archive) = crate::zip_package::open_package(package, Default::default()) else {
            return vec![false; origins.len()];
        };
        let indices: HashMap<_, _> = archive
            .entries()
            .iter()
            .enumerate()
            .map(|(index, entry)| (entry.path.clone(), index))
            .collect();
        let mut parts = HashMap::new();
        for origin in origins {
            parts.entry(origin.part.as_str()).or_insert_with(|| {
                let index = *indices.get(&origin.part)?;
                if archive.is_opaque(index) {
                    return None;
                }
                let xml = String::from_utf8(archive.read(index).ok()?).ok()?;
                let (_, source) = XmlSource::read(&xml).ok()?;
                let starts: HashSet<_> = source
                    .unclosed
                    .iter()
                    .map(|(i, id)| {
                        let range = &source.elements[*i].outer;
                        (id.clone(), range.start, range.end)
                    })
                    .collect();
                Some((source.digest, starts))
            });
        }
        origins
            .iter()
            .map(|origin| {
                record_provenance_work(1);
                parts.get(origin.part.as_str()).and_then(Option::as_ref).is_some_and(
                    |(digest, starts)| {
                        digest == &origin.digest
                            && starts.contains(&(
                                origin.id.clone(),
                                origin.range.start,
                                origin.range.end,
                            ))
                    },
                )
            })
            .collect()
    }

    pub(crate) fn relocate(&mut self, path: CorePath) {
        self.path = path;
    }
    /// Source-backed Core address.
    pub fn path(&self) -> &CorePath {
        &self.path
    }
    /// Authored field instance identifier.
    pub fn id(&self) -> &str {
        &self.id
    }
}
/// Non-serialized validation evidence, excluded from semantic equality.
#[derive(Debug, Clone, Default)]
pub(crate) struct RecoveryStamp(pub(crate) Option<UnclosedOrigin>);
impl PartialEq for RecoveryStamp {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}
impl XmlSource {
    /// Read original XML once, returning an identity-annotated decoder view.
    /// The annotations are private decoder transport and are never encoded.
    ///
    /// # Errors
    /// Rejects malformed XML and source-authored decoder identity attributes.
    pub fn read(xml: &str) -> Result<(String, Self), String> {
        SOURCE_WORK.set((
            SOURCE_WORK.get().0 + 1,
            SOURCE_WORK.get().1 + xml.len(),
            SOURCE_WORK.get().2,
        ));
        let mut source = Self {
            elements: vec![XmlElement::default()],
            digest: Sha256::digest(xml.as_bytes()).into(),
            ..Self::default()
        };
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);
        let mut stack = Vec::<usize>::new();
        let mut inserted = Vec::new();
        let mut starts: HashMap<(usize, String), Vec<usize>> = HashMap::new();
        let mut ends = HashSet::new();
        let mut parents = vec![0];
        loop {
            let event = reader.read_event().map_err(|e| e.to_string())?;
            let end = reader.buffer_position() as usize;
            match event {
                Event::Start(e) | Event::Empty(e) => {
                    let empty = xml.as_bytes().get(end.saturating_sub(2)) == Some(&b'/');
                    let start = end
                        .checked_sub(e.len() + if empty { 3 } else { 2 })
                        .ok_or("invalid XML source span")?;
                    let id = source.elements.len();
                    let parent = stack.last().copied().unwrap_or(0);
                    source.elements[parent].children.push(id);
                    parents.push(parent);
                    let name = e.local_name().as_ref().to_string();
                    let flow = stack
                        .iter()
                        .rev()
                        .find(|i| source.elements[**i].name == "p")
                        .map_or(0, |i| parents[*i]);
                    let mut field_id = None;
                    let mut id_value = None;
                    for attr in e.attributes() {
                        let attr = attr.map_err(|e| e.to_string())?;
                        if attr.key.as_ref() == "id" {
                            // Reader::from_str/read_event and raw attributes borrow
                            // the original UTF-8 input. No second XML scan is needed.
                            if let std::borrow::Cow::Borrowed(raw) = &attr.value {
                                let offset = (raw.as_ptr() as usize)
                                    .checked_sub(xml.as_ptr() as usize)
                                    .filter(|offset| *offset + raw.len() <= xml.len())
                                    .ok_or("attribute does not borrow the original XML")?;
                                id_value = Some(offset..offset + raw.len());
                            }
                        }
                        if attr.key.as_ref() == "_hwpforge_origin" {
                            return Err("reserved decoder origin attribute in source".into());
                        }
                        if (name == "fieldBegin" && attr.key.as_ref() == "id")
                            || (name == "fieldEnd" && attr.key.as_ref() == "beginIDRef")
                        {
                            field_id = Some(
                                attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                    .map_err(|e| e.to_string())?
                                    .into_owned(),
                            );
                        }
                    }
                    if let Some(field_id) = field_id.filter(|s| !s.is_empty()) {
                        if name == "fieldBegin" {
                            starts.entry((flow, field_id)).or_default().push(id);
                        } else {
                            ends.insert((flow, field_id));
                        }
                    }
                    source.elements.push(XmlElement {
                        name,
                        qualified_name: e.name().as_ref().to_owned(),
                        id_value,
                        parent,
                        outer: start..end,
                        inner: (!empty).then_some(end..end),
                        children: Vec::new(),
                    });
                    inserted.push((
                        start + 1 + e.name().as_ref().len(),
                        format!(" _hwpforge_origin=\"{id}\""),
                    ));
                    if !empty {
                        stack.push(id);
                    }
                }
                Event::End(e) => {
                    if let Some(id) = stack.pop() {
                        source.elements[id].outer.end = end;
                        if let Some(inner) = &mut source.elements[id].inner {
                            inner.end = end - e.len() - 3;
                        }
                    }
                }
                Event::Eof => break,
                _ => {}
            }
        }
        if !stack.is_empty() {
            return Err("unclosed XML element".into());
        }
        for ((flow, field_id), ids) in starts {
            if ids.len() == 1 && !ends.contains(&(flow, field_id.clone())) {
                source.unclosed.insert(ids[0], field_id);
            }
        }
        let mut annotated = String::with_capacity(xml.len() + inserted.len() * 30);
        let mut offset = 0;
        for (at, attribute) in inserted {
            annotated.push_str(&xml[offset..at]);
            annotated.push_str(&attribute);
            offset = at;
        }
        annotated.push_str(&xml[offset..]);
        Ok((annotated, source))
    }
    /// All elements in source order, including the identity-zero sentinel.
    pub fn elements(&self) -> &[XmlElement] {
        &self.elements
    }
    /// Original element by decoder identity. Identity zero denotes no source.
    pub fn element(&self, id: usize) -> Option<&XmlElement> {
        (id != 0).then(|| self.elements.get(id)).flatten()
    }
    /// A direct child by local name and ordinal.
    pub fn child(&self, id: usize, name: &str, index: usize) -> usize {
        self.element(id)
            .into_iter()
            .flat_map(|e| &e.children)
            .filter(|id| self.elements[**id].name == name)
            .nth(index)
            .copied()
            .unwrap_or(0)
    }
    /// Describe an unmatched XML start. This is NOT authority to accept it:
    /// there are no package bytes yet, and the claimed Core address must be
    /// verified by the HWPX decoder against that package before output.
    /// `part` is the decoder-resolved package path, independent of Core order.
    pub fn unclosed_origin(&self, id: usize, path: CorePath, part: &str) -> Option<UnclosedOrigin> {
        self.unclosed.get(&id).map(|field_id| UnclosedOrigin {
            part: part.into(),
            source_path: path.clone(),
            path,
            id: field_id.clone(),
            range: self.elements[id].outer.clone(),
            digest: self.digest,
            package: Arc::from([]),
        })
    }
}

#[cfg(test)]
mod verification_tests {
    use super::*;
    use std::io::{Cursor, Write};

    fn package(xml: &str) -> Arc<[u8]> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file("Contents/section0.xml", zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(xml.as_bytes()).unwrap();
        Arc::from(zip.finish().unwrap().into_inner())
    }

    /// 이것을 실패시키는 것: 원본 증거 경로에서 공용 ZIP 이름 검사를 우회하기.
    #[test]
    fn source_verification_rejects_parent_part() {
        let xml = "<sec><p><run><fieldBegin id=\"open\"/></run></p></sec>";
        let (_, source) = XmlSource::read(xml).unwrap();
        let origin = source
            .unclosed_origin(4, CorePath::new(0, "paragraphs[0].runs[0]"), "Contents/section0.xml")
            .unwrap();
        assert!(UnclosedOrigin::verify_package_batch(std::slice::from_ref(&origin), &package(xml)));
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in [("Contents/section0.xml", xml), ("Contents/../unused.xml", "x")] {
            zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
            zip.write_all(data.as_bytes()).unwrap();
        }
        let bytes = zip.finish().unwrap().into_inner();
        assert!(!UnclosedOrigin::verify_package_batch(std::slice::from_ref(&origin), &bytes));
        assert!(origin.with_package(Arc::from(bytes)).is_none());
    }

    /// 이것을 실패시키는 것: 원본 ZIP·파트·SHA-256·fieldBegin 범위·ID·짝 없음 중 검사 생략.
    #[test]
    fn original_bytes_verify_every_source_claim_component() {
        let xml = "<sec><p><run><fieldBegin id=\"open\"/></run></p></sec>";
        let (_, source) = XmlSource::read(xml).unwrap();
        let origin = source
            .unclosed_origin(4, CorePath::new(0, "paragraphs[0].runs[0]"), "Contents/section0.xml")
            .unwrap();
        let bytes = package(xml);
        assert!(origin.with_package(bytes.clone()).is_some());
        assert!(UnclosedOrigin::verify_package_batch(std::slice::from_ref(&origin), &bytes));
        assert!(origin.with_package(Arc::from([])).is_none());
        for change in ["part", "digest", "range", "id"] {
            let mut claim = origin.clone();
            match change {
                "part" => claim.part = "Contents/section1.xml".into(),
                "digest" => claim.digest[0] ^= 1,
                "range" => claim.range.start += 1,
                "id" => claim.id = "other".into(),
                _ => unreachable!(),
            }
            assert!(claim.with_package(bytes.clone()).is_none(), "{change}");
            assert!(!UnclosedOrigin::verify_package_batch(&[claim], &bytes), "{change}");
        }
        let closed = xml.replace("</run>", "<fieldEnd beginIDRef=\"open\"/></run>");
        let mut claim = origin.clone();
        claim.digest = Sha256::digest(closed.as_bytes()).into();
        assert!(claim.with_package(package(&closed)).is_none());
        assert!(!UnclosedOrigin::verify_package_batch(&[claim], &package(&closed)));
        // 이것을 실패시키는 것: fieldBegin이 아닌 같은 범위의 요소도 시작으로 인정.
        let other = xml.replace("fieldBegin", "otherBegin");
        let mut claim = origin.clone();
        claim.digest = Sha256::digest(other.as_bytes()).into();
        assert!(!UnclosedOrigin::verify_package_batch(&[claim], &package(&other)));
    }
}
