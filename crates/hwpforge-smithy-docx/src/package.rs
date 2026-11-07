//! OPC selection over Core's bounded package. Content is consumed once, lazily.

use crate::{
    error::{quote, DocxError, Result},
    xml::{self, Element, NamespaceId, TextBudget, XmlEvent, REL_NS, TYPE_NS},
};
use hwpforge_core::zip_package::{normalize_part_name, open_package, Package, PackageLimits};
use std::{
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
    pub(crate) name: Rc<str>,
    /// Declared type, even when the target entry is missing or opaque.
    pub(crate) content_type: Option<Rc<str>>,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    Internal(Part),
    External(String),
}
#[derive(Debug)]
pub(crate) struct Relationship {
    pub(crate) id: String,
    pub(crate) kind: RelationshipKind,
    pub(crate) target: Target,
}

#[derive(Debug)]
pub(crate) struct DocxPackage<'a> {
    package: Package<'a>,
    consumed: BTreeSet<usize>,
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
        let mut result = Self {
            package,
            consumed: BTreeSet::new(),
            main: Part { name: Rc::from(""), content_type: None },
            relationships: Vec::new(),
            text_budget: TextBudget::default(),
        };
        let types = {
            let bytes = result.read_part(CONTENT_TYPES)?;
            ContentTypes::parse(&bytes, &mut result.text_budget)?
        };
        let root = {
            result.index(ROOT_RELS)?;
            check_content_type(ROOT_RELS, types.get(ROOT_RELS), Some(MimeRule::Exact(RELS_TYPE)))?;
            let bytes = result.read_part(ROOT_RELS)?;
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
        let relationship = Self::resolve("", ROOT_RELS, &candidates[0], &types, &mut paths)?;
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
        result.index(&result.main.name)?;
        validate_content_type(&result.main, &RelationshipKind::OfficeDocument)?;
        let rels_name = relationship_part_name(&result.main.name);
        if result.find_index(&rels_name)?.is_some() {
            result.index(&rels_name)?;
            check_content_type(
                &rels_name,
                types.get(&rels_name),
                Some(MimeRule::Exact(RELS_TYPE)),
            )?;
            let bytes = result.read_part(&rels_name)?;
            let relationships = parse_relationships(&bytes, &rels_name, &mut result.text_budget)?;
            let mut singletons = BTreeSet::new();
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
                )?);
            }
        }
        Ok(result)
    }

    fn find_index(&self, name: &str) -> Result<Option<usize>> {
        let mut matches = self
            .package
            .entries()
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.path.eq_ignore_ascii_case(name));
        let first = matches.next().map(|(index, _)| index);
        // Core permits case variants of mimetype for other container formats.
        if matches.next().is_some() {
            return Err(DocxError::AmbiguousPart { part: quote(name) });
        }
        Ok(first)
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

    /// Move returned bytes to the XML walk or image consumer, then release them.
    /// Shared targets must reuse the interpreted result, never reread the ZIP.
    /// Pass the owning relationship kind (OfficeDocument for `main`). Missing,
    /// opaque, missing-type and type-mismatch errors remain distinct so callers
    /// can choose fatal errors or local exclusion according to content role.
    pub(crate) fn read_target(
        &mut self,
        target: &Target,
        kind: &RelationshipKind,
    ) -> Result<Vec<u8>> {
        match target {
            Target::Internal(part) => {
                self.index(&part.name)?;
                validate_content_type(part, kind)?;
                self.read_part(&part.name)
            }
            Target::External(target) => Err(DocxError::invalid(
                target,
                "Relationship",
                "external target cannot be read as a package part",
            )),
        }
    }

    fn resolve(
        source: &str,
        location: &str,
        raw: &RawRelationship,
        types: &ContentTypes,
        paths: &mut ResolvedPaths,
    ) -> Result<Relationship> {
        let target = if raw.external {
            Target::External(raw.target.clone())
        } else {
            let name = paths.resolve(source, location, &raw.target)?;
            Target::Internal(Part { content_type: types.shared(&name).cloned(), name })
        };
        Ok(Relationship { id: raw.id.clone(), kind: raw.kind.clone(), target })
    }
}

#[derive(Clone, Copy)]
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
    let (media, parameters) = actual.split_once(';').unwrap_or((actual, ""));
    match rule {
        MimeRule::Exact(expected) => {
            let (expected_media, expected_parameters) =
                expected.split_once(';').unwrap_or((expected, ""));
            media.eq_ignore_ascii_case(expected_media) && parameters == expected_parameters
        }
        MimeRule::Image => {
            media.get(..6).is_some_and(|prefix| prefix.eq_ignore_ascii_case("image/"))
        }
        MimeRule::Macro => {
            media.as_bytes().windows(12).any(|word| word.eq_ignore_ascii_case(b"macroEnabled"))
        }
    }
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

fn validate_content_type(part: &Part, kind: &RelationshipKind) -> Result<()> {
    if *kind == RelationshipKind::OfficeDocument
        && part.content_type.as_deref().is_some_and(|mime| mime_matches(mime, MimeRule::Macro))
    {
        return Err(DocxError::invalid(
            &part.name,
            "#content-type",
            format!(
                "macroEnabled content type '{}' is forbidden",
                quote(part.content_type.as_deref().unwrap())
            ),
        ));
    }
    let expected = if *kind == RelationshipKind::Image {
        Some(MimeRule::Image)
    } else {
        kind.expected_type().map(MimeRule::Exact)
    };
    check_content_type(&part.name, part.content_type.as_deref(), expected)
}

fn relationship_part_name(source: &str) -> String {
    match source.rsplit_once('/') {
        Some((directory, name)) => format!("{directory}/_rels/{name}.rels"),
        None => format!("_rels/{source}.rels"),
    }
}

// Like expanded XML text, derived path payload gets a package-wide 64 MiB ceiling.
// It is independent of ZIP bytes: short Targets can expand a long source directory.
const MAX_RESOLVED_PATH_BYTES: usize = 64 * 1024 * 1024;

#[derive(Default)]
struct ResolvedPaths {
    // Source keys occur once, Target keys are bounded by admitted XML attribute bytes.
    raw: HashMap<String, HashMap<String, Rc<str>>>,
    canonical: HashMap<u64, Vec<Rc<str>>>,
    hasher: RandomState,
    bytes: usize,
    #[cfg(test)]
    resolutions: usize,
    #[cfg(test)]
    allocated_bytes: usize,
}
impl ResolvedPaths {
    fn resolve(&mut self, source: &str, location: &str, target: &str) -> Result<Rc<str>> {
        if let Some(name) = self.raw.get(source).and_then(|targets| targets.get(target)) {
            return Ok(Rc::clone(name));
        }
        #[cfg(test)]
        {
            self.resolutions += 1;
        }
        let components = target_components(source, location, target)?;
        // Hash/compare borrowed components, before allocating a joined path. Case and
        // dot-segment aliases (including missing entries) share the first spelling.
        let mut hasher = self.hasher.build_hasher();
        for component in &components {
            for byte in component.bytes() {
                hasher.write_u8(byte.to_ascii_lowercase());
            }
            hasher.write_u8(b'/');
        }
        let bucket = self.canonical.entry(hasher.finish()).or_default();
        let existing = bucket.iter().find(|name| {
            let mut stored = name.split('/');
            components
                .iter()
                .all(|part| stored.next().is_some_and(|old| old.eq_ignore_ascii_case(part)))
                && stored.next().is_none()
        });
        let name = if let Some(name) = existing {
            Rc::clone(name)
        } else {
            let length = components.iter().map(|part| part.len()).sum::<usize>()
                + components.len().saturating_sub(1);
            let observed = self.bytes.saturating_add(length);
            if observed > MAX_RESOLVED_PATH_BYTES {
                return Err(DocxError::Limit {
                    part: quote(location),
                    element: "Relationship".into(),
                    resource: "resolved path UTF-8 bytes",
                    observed,
                    limit: MAX_RESOLVED_PATH_BYTES,
                });
            }
            #[cfg(test)]
            {
                self.allocated_bytes += length;
            }
            let name = components.join("/");
            normalize_part_name(&name)
                .map_err(|source| DocxError::Package { part: quote(location), source })?;
            let name: Rc<str> = Rc::from(name);
            self.bytes = observed;
            bucket.push(Rc::clone(&name));
            name
        };
        if !self.raw.contains_key(source) {
            self.raw.insert(source.to_owned(), HashMap::new());
        }
        self.raw.get_mut(source).unwrap().insert(target.to_owned(), Rc::clone(&name));
        Ok(name)
    }
}

#[cfg(test)]
fn resolve_target(source: &str, location: &str, target: &str) -> Result<Rc<str>> {
    ResolvedPaths::default().resolve(source, location, target)
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
    fn shared(&self, name: &str) -> Option<&Rc<str>> {
        self.overrides.get(&name.to_ascii_lowercase()).or_else(|| {
            name.rsplit('/')
                .next()
                .and_then(|last| last.rsplit_once('.'))
                .and_then(|(_, extension)| self.defaults.get(&extension.to_ascii_lowercase()))
        })
    }
}

#[cfg(test)]
mod tests;
