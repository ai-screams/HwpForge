//! OPC selection over Core's bounded package. Content is consumed once, lazily.

use crate::{
    error::{quote, DocxError, Result},
    xml::{self, Element, NamespaceId, TextBudget, XmlEvent, REL_NS, TYPE_NS},
};
use hwpforge_core::zip_package::{normalize_part_name, open_package, Package, PackageLimits};
use std::{
    cell::{OnceCell, RefCell},
    collections::{hash_map::RandomState, BTreeMap, BTreeSet, HashMap},
    hash::{BuildHasher, Hasher},
    rc::Rc,
};

const CONTENT_TYPES: &str = "[Content_Types].xml";
const RELS_TYPE: &str = "application/vnd.openxmlformats-package.relationships+xml";
const ROOT_RELS: &str = "_rels/.rels";
const OFFICE_REL: &str = "http://schemas.openxmlformats.org/officeDocument/2006/relationships/";
const MAIN_TYPE: &str =
    "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml";

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum RelationshipKind {
    OfficeDocument,
    Styles,
    Numbering,
    Settings,
    Header,
    Footer,
    Footnotes,
    Endnotes,
    Comments,
    Theme,
    Image,
    Other(String),
}
impl RelationshipKind {
    fn parse(uri: &str) -> Self {
        match uri.strip_prefix(OFFICE_REL) {
            Some("officeDocument") => Self::OfficeDocument,
            Some("styles") => Self::Styles,
            Some("numbering") => Self::Numbering,
            Some("settings") => Self::Settings,
            Some("header") => Self::Header,
            Some("footer") => Self::Footer,
            Some("footnotes") => Self::Footnotes,
            Some("endnotes") => Self::Endnotes,
            Some("comments") => Self::Comments,
            Some("theme") => Self::Theme,
            Some("image") => Self::Image,
            _ => Self::Other(uri.into()),
        }
    }
    fn singleton(&self) -> bool {
        matches!(
            self,
            Self::OfficeDocument
                | Self::Styles
                | Self::Numbering
                | Self::Settings
                | Self::Footnotes
                | Self::Endnotes
                | Self::Comments
                | Self::Theme
        )
    }
    fn expected_type(&self) -> Option<&'static str> {
        match self {
            Self::OfficeDocument => Some(MAIN_TYPE),
            Self::Styles => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml")
            }
            Self::Numbering => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml")
            }
            Self::Settings => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml")
            }
            Self::Header => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml")
            }
            Self::Footer => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml")
            }
            Self::Footnotes => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml")
            }
            Self::Endnotes => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.endnotes+xml")
            }
            Self::Comments => {
                Some("application/vnd.openxmlformats-officedocument.wordprocessingml.comments+xml")
            }
            Self::Theme => Some("application/vnd.openxmlformats-officedocument.theme+xml"),
            Self::Image | Self::Other(_) => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) id: usize,
    cache: Rc<PartCache>,
    pub(crate) name: Rc<str>,
    /// Declared type, even when the target entry is missing or opaque.
    pub(crate) content_type: Option<Rc<str>>,
}
// Clones from both raw Target and canonical aliases share these cells. No
// canonical name is hashed or scanned on a repeated relationship selection.
#[derive(Debug, Default, PartialEq, Eq)]
struct PartCache {
    index: OnceCell<std::result::Result<usize, SelectionFailure>>,
    types: RefCell<BTreeMap<Option<MimeRule>, Option<SelectionFailure>>>,
}
impl Part {
    fn new(id: usize, name: Rc<str>, content_type: Option<Rc<str>>) -> Self {
        Self { id, name, content_type, cache: Rc::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SelectionFailure {
    Missing,
    Ambiguous,
    Opaque,
    MissingType,
    Mismatch(&'static str),
    Macro,
}
impl SelectionFailure {
    pub(crate) fn error(self, part: &Part) -> DocxError {
        let name = quote(&part.name);
        match self {
            Self::Missing => DocxError::MissingPart { part: name },
            Self::Ambiguous => DocxError::AmbiguousPart { part: name },
            Self::Opaque => DocxError::OpaquePart { part: name },
            Self::MissingType => DocxError::MissingContentType { part: name },
            Self::Mismatch(expected) => DocxError::ContentTypeMismatch {
                part: name,
                actual: quote(part.content_type.as_deref().unwrap()),
                expected,
            },
            Self::Macro => DocxError::invalid(
                &name,
                "#content-type",
                format!(
                    "macroEnabled content type '{}' is forbidden",
                    quote(part.content_type.as_deref().unwrap())
                ),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Internal(Part),
    External(String),
}
#[derive(Debug, Clone)]
pub(crate) struct Relationship {
    pub(crate) id: String,
    pub(crate) kind: RelationshipKind,
    pub(crate) target: Target,
}

// Hash the lowercase spelling once. Integer buckets avoid rehashing long names
// when the table grows; exact spelling checks still resolve full hash collisions.
#[derive(Debug)]
struct NameIndex {
    buckets: HashMap<u64, Vec<(String, Option<usize>)>>,
    hasher: RandomState,
}
impl NameIndex {
    fn new(package: &Package<'_>) -> Self {
        let mut result = Self { buckets: HashMap::new(), hasher: RandomState::new() };
        for (index, entry) in package.entries().iter().enumerate() {
            #[cfg(test)]
            selection_work(|c| c.entry_bytes += entry.path.len());
            let key = entry.path.to_ascii_lowercase();
            let hash = result.hash(&key);
            let bucket = result.buckets.entry(hash).or_default();
            if let Some((_, value)) = bucket.iter_mut().find(|(name, _)| name_eq(name, &key)) {
                // Core rejects ordinary case collisions, but accepts mimetype
                // variants for Hancom. DOCX must not choose one arbitrarily.
                *value = None;
            } else {
                bucket.push((key, Some(index)));
            }
        }
        result
    }
    fn hash(&self, key: &str) -> u64 {
        #[cfg(test)]
        selection_work(|c| c.hash_bytes += key.len());
        self.hasher.hash_one(key)
    }
    fn get(&self, name: &str) -> std::result::Result<Option<usize>, SelectionFailure> {
        #[cfg(test)]
        selection_work(|c| {
            c.lookups += 1;
            c.lookup_bytes += name.len();
        });
        let key = name.to_ascii_lowercase();
        let found = self
            .buckets
            .get(&self.hash(&key))
            .and_then(|bucket| bucket.iter().find(|(name, _)| name_eq(name, &key)));
        match found {
            None => Ok(None),
            Some((_, Some(index))) => Ok(Some(*index)),
            Some((_, None)) => Err(SelectionFailure::Ambiguous),
        }
    }
}
fn name_eq(left: &str, right: &str) -> bool {
    #[cfg(test)]
    selection_work(|c| c.path_bytes += compared_bytes(left.as_bytes(), right.as_bytes(), false));
    left == right
}

#[derive(Debug)]
pub(crate) struct DocxPackage<'a> {
    package: Package<'a>,
    consumed: BTreeSet<usize>,
    names: NameIndex,
    pub(crate) main: Part,
    /// Main-part relationships in XML order, including unknown kinds and externals.
    pub(crate) relationships: Vec<Relationship>,
    /// Continue this budget when walking content parts.
    pub(crate) text_budget: TextBudget,
}

impl<'a> DocxPackage<'a> {
    pub(crate) fn open(bytes: &'a [u8]) -> Result<Self> {
        if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
            return Err(DocxError::invalid(
                "#package",
                "#signature",
                "encrypted or legacy binary format; expected ZIP",
            ));
        }
        let package = open_package(bytes, PackageLimits::default())
            .map_err(|source| DocxError::Package { part: "#package".into(), source })?;
        let names = NameIndex::new(&package);
        let mut result = Self {
            package,
            consumed: BTreeSet::new(),
            names,
            main: Part::new(usize::MAX, Rc::from(""), None),
            relationships: Vec::new(),
            text_budget: TextBudget::default(),
        };
        let types = {
            let bytes = result.read_part(CONTENT_TYPES)?;
            ContentTypes::parse(&bytes, &mut result.text_budget)?
        };
        let root = {
            let index = result.index(ROOT_RELS)?;
            check_content_type(ROOT_RELS, types.get(ROOT_RELS), Some(MimeRule::Exact(RELS_TYPE)))?;
            let bytes = result.read_index(ROOT_RELS, index)?;
            parse_relationships(&bytes, ROOT_RELS, &mut result.text_budget)?
        };
        let candidates: Vec<_> =
            root.into_iter().filter(|r| r.kind == RelationshipKind::OfficeDocument).collect();
        if candidates.len() != 1 {
            return Err(DocxError::invalid(
                ROOT_RELS,
                "Relationship",
                format!(
                    "officeDocument relationships observed {}, required 1 (limit 1)",
                    candidates.len()
                ),
            ));
        }
        let mut paths = ResolvedPaths::default();
        let relationship =
            Self::resolve("", ROOT_RELS, &candidates[0], &types, &mut paths, &mut HashMap::new())?;
        result.main = match relationship.target {
            Target::Internal(part) => part,
            Target::External(target) => {
                return Err(DocxError::invalid(
                    ROOT_RELS,
                    "Relationship",
                    format!(
                        "external officeDocument target '{}' cannot be read as a part",
                        quote(target)
                    ),
                ))
            }
        };
        result
            .select(&result.main, &RelationshipKind::OfficeDocument)
            .map_err(|e| e.error(&result.main))?;
        let rels_name = relationship_part_name(&result.main.name);
        if let Some(index) = result.find_index(&rels_name)? {
            if result.package.is_opaque(index) {
                return Err(DocxError::OpaquePart { part: quote(&rels_name) });
            }
            check_content_type(
                &rels_name,
                types.get(&rels_name),
                Some(MimeRule::Exact(RELS_TYPE)),
            )?;
            let bytes = result.read_index(&rels_name, index)?;
            let relationships = parse_relationships(&bytes, &rels_name, &mut result.text_budget)?;
            let mut singletons = BTreeSet::new();
            let mut targets = HashMap::new();
            for relationship in relationships {
                if relationship.kind.singleton() && !singletons.insert(relationship.kind.clone()) {
                    return Err(DocxError::invalid(
                        &rels_name,
                        "Relationship",
                        format!("{:?} relationships observed 2, limit 1", relationship.kind),
                    ));
                }
                result.relationships.push(Self::resolve(
                    &result.main.name,
                    &rels_name,
                    &relationship,
                    &types,
                    &mut paths,
                    &mut targets,
                )?);
            }
        }
        Ok(result)
    }

    fn find_index(&self, name: &str) -> Result<Option<usize>> {
        self.names.get(name).map_err(|_| DocxError::AmbiguousPart { part: quote(name) })
    }

    fn part_index(&self, part: &Part) -> std::result::Result<usize, SelectionFailure> {
        *part.cache.index.get_or_init(|| match self.find_index(&part.name) {
            Err(_) => Err(SelectionFailure::Ambiguous),
            Ok(None) => Err(SelectionFailure::Missing),
            Ok(Some(index)) if self.package.is_opaque(index) => Err(SelectionFailure::Opaque),
            Ok(Some(index)) => Ok(index),
        })
    }

    fn select(
        &self,
        part: &Part,
        kind: &RelationshipKind,
    ) -> std::result::Result<usize, SelectionFailure> {
        let index = self.part_index(part)?;
        content_type_failure(part, kind).map_or(Ok(index), Err)
    }

    fn index(&self, name: &str) -> Result<usize> {
        let index =
            self.find_index(name)?.ok_or_else(|| DocxError::MissingPart { part: quote(name) })?;
        if self.package.is_opaque(index) {
            return Err(DocxError::OpaquePart { part: quote(name) });
        }
        Ok(index)
    }

    fn read_part(&mut self, name: &str) -> Result<Vec<u8>> {
        let index = self.index(name)?;
        self.read_index(name, index)
    }

    fn read_index(&mut self, name: &str, index: usize) -> Result<Vec<u8>> {
        // Mark before decompression: a failed read must never be retried either.
        if !self.consumed.insert(index) {
            return Err(DocxError::invalid(
                name,
                "#part",
                "part already read (attempts observed 2, limit 1)",
            ));
        }
        self.package.read(index).map_err(|source| DocxError::Package { part: quote(name), source })
    }

    /// The mandatory main part is always fatal on selection or read failure.
    pub(crate) fn read_main(&mut self) -> Result<Vec<u8>> {
        let main = self.main.clone();
        self.read_internal(&main, &RelationshipKind::OfficeDocument)
    }

    /// The relationship owns its type. Core read errors are always fatal.
    pub(crate) fn read_relationship(&mut self, relationship: &Relationship) -> Result<Vec<u8>> {
        match &relationship.target {
            Target::Internal(part) => self.read_internal(part, &relationship.kind),
            Target::External(target) => Err(DocxError::invalid(
                target,
                "Relationship",
                "external target cannot be read as a package part",
            )),
        }
    }

    fn read_internal(&mut self, part: &Part, kind: &RelationshipKind) -> Result<Vec<u8>> {
        let index = self.select(part, kind).map_err(|e| e.error(part))?;
        self.read_index(&part.name, index)
    }

    /// Check type even for shared targets; successful reads are indexed by ZIP entry.
    pub(crate) fn relationship_index(
        &self,
        relationship: &Relationship,
    ) -> std::result::Result<Option<usize>, SelectionFailure> {
        match &relationship.target {
            Target::Internal(part) => self.select(part, &relationship.kind).map(Some),
            Target::External(_) => Ok(None),
        }
    }

    fn resolve(
        source: &str,
        location: &str,
        raw: &RawRelationship,
        types: &ContentTypes,
        paths: &mut ResolvedPaths,
        targets: &mut HashMap<String, Part>,
    ) -> Result<Relationship> {
        let target = if raw.external {
            Target::External(raw.target.clone())
        } else {
            Target::Internal(paths.resolve(source, location, &raw.target, targets, types)?)
        };
        Ok(Relationship { id: raw.id.clone(), kind: raw.kind.clone(), target })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum MimeRule {
    Exact(&'static str),
    Image,
    Macro,
}
impl MimeRule {
    fn label(self) -> &'static str {
        match self {
            Self::Exact(value) => value,
            Self::Image => "image/*",
            Self::Macro => "macroEnabled",
        }
    }
}

/// MIME type/subtype are ASCII-insensitive; parameter spelling stays exact.
/// Main content types with parameters are unsupported: no parameter semantics are implemented.
fn mime_matches(actual: &str, rule: MimeRule) -> bool {
    #[cfg(test)]
    selection_work(|c| c.mime_calls += 1);
    let (media, parameters) = actual.split_once(';').unwrap_or((actual, ""));
    #[cfg(test)]
    selection_work(|c| c.mime_bytes += media.len() + usize::from(media.len() < actual.len()));
    match rule {
        MimeRule::Exact(expected) => {
            let (expected_media, expected_parameters) =
                expected.split_once(';').unwrap_or((expected, ""));
            mime_eq(media.as_bytes(), expected_media.as_bytes()) && {
                #[cfg(test)]
                selection_work(|c| {
                    c.mime_bytes +=
                        compared_bytes(parameters.as_bytes(), expected_parameters.as_bytes(), false)
                });
                parameters == expected_parameters
            }
        }
        MimeRule::Image => {
            media.get(..6).is_some_and(|prefix| mime_eq(prefix.as_bytes(), b"image/"))
        }
        MimeRule::Macro => media.as_bytes().windows(12).any(|word| mime_eq(word, b"macroEnabled")),
    }
}

fn mime_eq(actual: &[u8], expected: &[u8]) -> bool {
    #[cfg(test)]
    selection_work(|c| c.mime_bytes += compared_bytes(actual, expected, true));
    actual.eq_ignore_ascii_case(expected)
}

// Logical byte visits at the production comparison sites, not SIMD instructions
// or allocator/probe overhead. Unequal lengths require no payload comparison.
#[cfg(test)]
fn compared_bytes(actual: &[u8], expected: &[u8], ascii_case: bool) -> usize {
    if actual.len() != expected.len() {
        return 0;
    }
    actual
        .iter()
        .zip(expected)
        .position(|(a, b)| if ascii_case { !a.eq_ignore_ascii_case(b) } else { a != b })
        .map_or(actual.len(), |index| index + 1)
}

fn check_content_type(name: &str, actual: Option<&str>, expected: Option<MimeRule>) -> Result<()> {
    let actual = actual.ok_or_else(|| DocxError::MissingContentType { part: quote(name) })?;
    if let Some(expected) = expected {
        if !mime_matches(actual, expected) {
            return Err(DocxError::ContentTypeMismatch {
                part: quote(name),
                actual: quote(actual),
                expected: expected.label(),
            });
        }
    }
    Ok(())
}

fn content_type_failure(part: &Part, kind: &RelationshipKind) -> Option<SelectionFailure> {
    let rule = if *kind == RelationshipKind::Image {
        Some(MimeRule::Image)
    } else {
        kind.expected_type().map(MimeRule::Exact)
    };
    *part.cache.types.borrow_mut().entry(rule).or_insert_with(|| {
        #[cfg(test)]
        selection_work(|c| c.type_checks += 1);
        let Some(actual) = part.content_type.as_deref() else {
            return Some(SelectionFailure::MissingType);
        };
        if *kind == RelationshipKind::OfficeDocument && mime_matches(actual, MimeRule::Macro) {
            return Some(SelectionFailure::Macro);
        }
        rule.filter(|rule| !mime_matches(actual, *rule))
            .map(|rule| SelectionFailure::Mismatch(rule.label()))
    })
}

#[cfg(test)]
fn validate_content_type(part: &Part, kind: &RelationshipKind) -> Result<()> {
    content_type_failure(part, kind).map_or(Ok(()), |e| Err(e.error(part)))
}

fn relationship_part_name(source: &str) -> String {
    match source.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/_rels/{name}.rels"),
        None => format!("_rels/{source}.rels"),
    }
}

// Derived path work has a package-wide 64 MiB ceiling, including first-time
// aliases. Four times source+Target is an admission weight for canonical path
// derivation, not a count of every byte visit or ZIP candidate comparison. A first
// selection lowercases/hashes its canonical name once (length <= source+Target);
// it is covered by this weight. ZIP name indexing separately visits entry names
// once; metadata performs a fixed number of name lookups. Hash collision checks
// remain subject to the randomized hash table's expected-cost guarantee.
const MAX_RESOLVED_PATH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct ResolvedPaths {
    next_id: usize,
    canonical: HashMap<u64, Vec<Part>>,
    hasher: RandomState,
    bytes: usize,
    #[cfg(test)]
    resolutions: usize,
    #[cfg(test)]
    allocated_bytes: usize,
    #[cfg(test)]
    hash_bytes: usize,
}
impl ResolvedPaths {
    fn resolve(
        &mut self,
        source: &str,
        location: &str,
        target: &str,
        targets: &mut HashMap<String, Part>,
        types: &ContentTypes,
    ) -> Result<Part> {
        #[cfg(test)]
        {
            self.hash_bytes += target.len();
        }
        if let Some(part) = targets.get(target) {
            return Ok(part.clone());
        }
        // Charge before even splitting the source. A new alias also pays for its
        // canonical work; repeated raw Targets only hash their own spelling.
        let cost = source.len().saturating_add(target.len()).saturating_mul(4);
        let observed = self.bytes.saturating_add(cost);
        if observed > MAX_RESOLVED_PATH_BYTES {
            return Err(DocxError::Limit {
                part: quote(location),
                element: "Relationship".into(),
                resource: "resolved path processing bytes",
                observed,
                limit: MAX_RESOLVED_PATH_BYTES,
            });
        }
        self.bytes = observed;
        #[cfg(test)]
        {
            self.resolutions += 1;
        }
        let components = target_components(source, location, target)?;
        // Hash/compare borrowed components, before allocating a joined path. Case and
        // dot-segment aliases (including missing entries) share the first spelling.
        let mut hasher = self.hasher.build_hasher();
        for component in &components {
            #[cfg(test)]
            {
                self.hash_bytes += component.len() + 1;
            }
            for byte in component.bytes() {
                hasher.write_u8(byte.to_ascii_lowercase());
            }
            hasher.write_u8(b'/');
        }
        let bucket = self.canonical.entry(hasher.finish()).or_default();
        let existing = bucket.iter().find(|name| {
            let mut stored = name.name.split('/');
            components
                .iter()
                .all(|part| stored.next().is_some_and(|old| old.eq_ignore_ascii_case(part)))
                && stored.next().is_none()
        });
        let name = if let Some(name) = existing {
            name.clone()
        } else {
            #[cfg(test)]
            let length = components.iter().map(|part| part.len()).sum::<usize>()
                + components.len().saturating_sub(1);
            #[cfg(test)]
            {
                self.allocated_bytes += length;
            }
            let name = components.join("/");
            normalize_part_name(&name)
                .map_err(|source| DocxError::Package { part: quote(location), source })?;
            let name: Rc<str> = Rc::from(name);
            let part = Part::new(self.next_id, name.clone(), types.shared(&name).cloned());
            self.next_id += 1;
            bucket.push(part.clone());
            part
        };
        targets.insert(target.to_owned(), name.clone());
        Ok(name)
    }
}

#[cfg(test)]
fn resolve_target(source: &str, location: &str, target: &str) -> Result<Rc<str>> {
    ResolvedPaths::default()
        .resolve(source, location, target, &mut HashMap::new(), &ContentTypes::default())
        .map(|part| part.name)
}

/// Resolve a part URI without percent decoding or changing its spelling.
fn target_components<'a>(source: &'a str, location: &str, target: &'a str) -> Result<Vec<&'a str>> {
    let invalid = |detail: &str| {
        DocxError::invalid(
            location,
            "Relationship",
            format!("target '{}': {detail}", quote(target)),
        )
    };
    if target.is_empty() || target.starts_with("//") {
        return Err(invalid("expected a part URI, not an empty or network path"));
    }
    if target.split('/').next().unwrap_or_default().contains(':') {
        return Err(invalid("URI schemes and absolute drive paths are forbidden"));
    }
    if matches!(target.rsplit('/').next(), Some("." | "..")) {
        return Err(invalid("target identifies a directory, not a part"));
    }
    let bytes = target.as_bytes();
    for (index, byte) in bytes.iter().copied().enumerate() {
        if byte == b'%' {
            if !bytes
                .get(index + 1..index + 3)
                .is_some_and(|hex| hex.iter().all(u8::is_ascii_hexdigit))
            {
                return Err(invalid("invalid percent escape"));
            }
        } else if !(byte.is_ascii_alphanumeric() || b"-._~!$&'()*+,;=:@/".contains(&byte)) {
            return Err(invalid("invalid relative URI character"));
        }
    }
    let mut components: Vec<_> = if target.starts_with('/') {
        Vec::new()
    } else {
        source.rsplit_once('/').map(|(dir, _)| dir.split('/').collect()).unwrap_or_default()
    };
    for component in target.strip_prefix('/').unwrap_or(target).split('/') {
        match component {
            "." => {}
            ".." => {
                if components.pop().is_none() {
                    return Err(invalid("path escapes package root"));
                }
            }
            value => components.push(value),
        }
    }
    Ok(components)
}

/// Only the two flat OPC metadata vocabularies use this visitor.
fn metadata(
    bytes: &[u8],
    part: &str,
    (namespace_id, namespace): (NamespaceId, &str),
    root: &str,
    budget: &mut TextBudget,
    mut record: impl FnMut(Element) -> Result<()>,
) -> Result<()> {
    let mut depth = 0;
    xml::walk(bytes, part, budget, |event| {
        match event {
            XmlEvent::Start(element) => {
                depth += 1;
                if element.namespace_id != namespace_id {
                    return Err(DocxError::invalid(
                        part,
                        &element.name,
                        format!(
                            "namespace '{}' does not match '{namespace}'",
                            quote(element.namespace)
                        ),
                    ));
                }
                match depth {
                    1 if element.name == root => {},
                    2 => record(element)?,
                    _ => return Err(DocxError::invalid(part, &element.name, format!("unexpected metadata element at depth {depth}; expected {root} with direct children"))),
                }
            }
            XmlEvent::End => depth -= 1,
            XmlEvent::Text(value) => {
                if !value.trim().is_empty() {
                    return Err(DocxError::invalid(part, root, "unexpected metadata text"));
                }
            }
        }
        Ok(())
    })
}
fn required<'a>(part: &str, element: &'a Element, name: &str) -> Result<&'a str> {
    element.attribute(name).filter(|value| !value.is_empty()).ok_or_else(|| {
        DocxError::invalid(
            part,
            &element.name,
            format!("attribute '{}' observed missing/empty, required 1", quote(name)),
        )
    })
}
#[derive(Debug)]
struct RawRelationship {
    id: String,
    kind: RelationshipKind,
    target: String,
    external: bool,
}
fn parse_relationships(
    bytes: &[u8],
    part: &str,
    budget: &mut TextBudget,
) -> Result<Vec<RawRelationship>> {
    let mut result = Vec::new();
    let mut ids = BTreeSet::new();
    metadata(bytes, part, REL_NS, "Relationships", budget, |element| {
        if element.name != "Relationship" {
            return Err(DocxError::invalid(part, &element.name, "expected Relationship"));
        }
        let id = required(part, &element, "Id")?;
        if !ids.insert(id.to_owned()) {
            return Err(DocxError::invalid(
                part,
                &element.name,
                format!("duplicate relationship Id '{}' (observed 2, limit 1)", quote(id)),
            ));
        }
        let uri = required(part, &element, "Type")?;
        xml::reject_strict(part, &element.name, uri)?;
        let target = required(part, &element, "Target")?;
        let external = match element.attribute("TargetMode") {
            None | Some("Internal") => false,
            Some("External") => true,
            Some(value) => {
                return Err(DocxError::invalid(
                    part,
                    &element.name,
                    format!("TargetMode '{}'; expected Internal or External", quote(value)),
                ))
            }
        };
        result.push(RawRelationship {
            id: id.into(),
            kind: RelationshipKind::parse(uri),
            target: target.into(),
            external,
        });
        Ok(())
    })?;
    Ok(result)
}
#[derive(Default)]
struct ContentTypes {
    #[cfg(test)]
    lowercase_bytes: std::cell::Cell<usize>,
    overrides: BTreeMap<String, Rc<str>>,
    defaults: BTreeMap<String, Rc<str>>,
}
impl ContentTypes {
    fn parse(bytes: &[u8], budget: &mut TextBudget) -> Result<Self> {
        let mut result = Self::default();
        metadata(bytes, CONTENT_TYPES, TYPE_NS, "Types", budget, |element| {
            let mime = required(CONTENT_TYPES, &element, "ContentType")?;
            let previous = match element.name.as_str() {
                "Override" => {
                    let name = required(CONTENT_TYPES, &element, "PartName")?;
                    let name = name.strip_prefix('/').ok_or_else(|| {
                        DocxError::invalid(
                            CONTENT_TYPES,
                            "Override",
                            format!("PartName '{}'; expected leading '/'", quote(name)),
                        )
                    })?;
                    normalize_part_name(name).map_err(|source| DocxError::Package {
                        part: CONTENT_TYPES.into(),
                        source,
                    })?;
                    result.overrides.insert(name.to_ascii_lowercase(), mime.into())
                }
                "Default" => result.defaults.insert(
                    required(CONTENT_TYPES, &element, "Extension")?.to_ascii_lowercase(),
                    mime.into(),
                ),
                _ => {
                    return Err(DocxError::invalid(
                        CONTENT_TYPES,
                        &element.name,
                        "expected Override or Default",
                    ))
                }
            };
            if previous.is_some() {
                return Err(DocxError::invalid(
                    CONTENT_TYPES,
                    &element.name,
                    "duplicate content type definition (observed 2, limit 1)",
                ));
            }
            Ok(())
        })?;
        Ok(result)
    }
    fn get(&self, name: &str) -> Option<&str> {
        self.shared(name).map(AsRef::as_ref)
    }
    fn lookup_key(&self, value: &str) -> String {
        #[cfg(test)]
        self.lowercase_bytes.set(self.lowercase_bytes.get() + value.len());
        value.to_ascii_lowercase()
    }
    fn shared(&self, name: &str) -> Option<&Rc<str>> {
        self.overrides.get(&self.lookup_key(name)).or_else(|| {
            name.rsplit('/')
                .next()
                .and_then(|last| last.rsplit_once('.'))
                .and_then(|(_, extension)| self.defaults.get(&self.lookup_key(extension)))
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SelectionWork {
    pub(crate) lookups: usize,
    pub(crate) entry_bytes: usize,
    pub(crate) hash_bytes: usize,
    pub(crate) path_bytes: usize,
    pub(crate) lookup_bytes: usize,
    pub(crate) type_checks: usize,
    pub(crate) mime_calls: usize,
    pub(crate) mime_bytes: usize,
}
#[cfg(test)]
thread_local! {
    static SELECTION_WORK: std::cell::Cell<SelectionWork> = const { std::cell::Cell::new(SelectionWork { entry_bytes: 0, hash_bytes: 0, lookups: 0, path_bytes: 0, lookup_bytes: 0, type_checks: 0, mime_calls: 0, mime_bytes: 0 }) };
}
#[cfg(test)]
fn selection_work(update: impl FnOnce(&mut SelectionWork)) {
    SELECTION_WORK.with(|cell| {
        let mut work = cell.get();
        update(&mut work);
        cell.set(work);
    });
}
#[cfg(test)]
pub(crate) fn take_selection_work() -> SelectionWork {
    SELECTION_WORK.with(|cell| cell.replace(SelectionWork::default()))
}
