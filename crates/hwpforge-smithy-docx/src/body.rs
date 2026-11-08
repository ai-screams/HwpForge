//! One event pass for safety and selected body text; no XML tree is retained.
use crate::format::{attr, Definitions, Delta, PropertyState, Resolver, StyleKey};
use crate::{
    error::{quote, Result},
    package::{DocxPackage, RelationshipKind, SelectionFailure, Target},
    warning::{record, Kind},
    xml::{self, Element, NamespaceId, TextBudget, XmlEvent, MC_NS, M_NS, R_NS, W_NS},
    DocxDecoded, DocxError, DocxWarning,
};
use hwpforge_core::{
    document::Document,
    image::ImageStore,
    paragraph::Paragraph,
    run::{Run, RunContent},
    section::Section,
};
use hwpforge_foundation::ParaShapeIndex;
mod layout;
use layout::{Break, DecodedSection, Layout, Origin, Relations};
use std::{
    collections::{HashMap, HashSet},
    fmt,
    rc::Rc,
};

const MAX_PARAGRAPHS: usize = 1_000_000;
const MAX_RUNS: usize = 4_000_000;

fn empty_text_runs(runs: &[Run]) -> bool {
    runs.iter().all(|run| matches!(&run.content, RunContent::Text(text) if text.is_empty()))
}

fn admit_count(
    part: &str,
    location: &(impl fmt::Display + ?Sized),
    resource: &'static str,
    current: usize,
    limit: usize,
) -> Result<usize> {
    let observed = current.saturating_add(1);
    if observed > limit {
        return Err(DocxError::Limit {
            part: quote(part),
            element: quote(location),
            resource,
            observed,
            limit,
        });
    }
    Ok(observed)
}

fn root_for(kind: &RelationshipKind) -> Option<&'static str> {
    use RelationshipKind::*;
    match kind {
        OfficeDocument => Some("document"),
        Styles => Some("styles"),
        Numbering => Some("numbering"),
        Settings => Some("settings"),
        Header => Some("hdr"),
        Footer => Some("ftr"),
        Footnotes => Some("footnotes"),
        Endnotes => Some("endnotes"),
        Comments => Some("comments"),
        Theme => Some("theme"),
        Image | Other(_) => None,
    }
}

pub(crate) fn decode(bytes: &[u8]) -> Result<DocxDecoded> {
    let mut package = DocxPackage::open(bytes)?;
    let mut warnings = Vec::new();
    let mut seen = HashSet::from([package.main.id]);
    let main = package.main.name.clone();
    let mut definitions = Definitions::default();
    let mut relations = Relations::default();
    let mut valid_roots = HashMap::new();
    let mut suppress_before_page_break = false;
    // Moving the relationships avoids cloning all input strings or borrowing the
    // package through a read. Shared ZIP targets are scanned only once.
    let mut relationships = std::mem::take(&mut package.relationships);
    relationships.sort_by_key(|r| match r.kind {
        RelationshipKind::Theme => 0,
        RelationshipKind::Styles => 1,
        RelationshipKind::Numbering => 2,
        _ => 3,
    });
    for relation in relationships {
        let part = match &relation.target {
            Target::Internal(part) => part.name.as_ref(),
            Target::External(_) => main.as_ref(),
        };
        let Some(root) = root_for(&relation.kind) else {
            record(&mut warnings, Kind::OptionalPartOmitted, "uninterpreted_part", "omit", || {
                (
                    Some(part),
                    Some(format!("relationship:{}", quote(&relation.id))),
                    "Relationship content is not interpreted",
                )
            })?;
            continue;
        };
        // Only absent parts are omitted. Selected WordprocessingML
        // must remain readable for change detection, including ancillary content.
        match package.relationship_index(&relation) {
            Ok(Some(_)) => {
                let Target::Internal(internal) = &relation.target else { unreachable!() };
                if !seen.insert(internal.id) {
                    if valid_roots.get(&internal.id) == Some(&root) {
                        relations.admit(&relation, &main)?;
                    }
                    continue;
                }
            }
            Ok(None) if relation.kind == RelationshipKind::Theme => {
                record(
                    &mut warnings,
                    Kind::OptionalPartOmitted,
                    "uninterpreted_part",
                    "omit",
                    || (Some(part), Some("theme"), "External theme is not fetched"),
                )?;
                continue;
            }
            Ok(None) => {
                return Err(DocxError::invalid(
                    part,
                    format!("relationship:{}", quote(&relation.id)),
                    "external WordprocessingML part cannot be checked for tracked changes",
                ));
            }
            Err(SelectionFailure::Missing) => {
                let Target::Internal(internal) = &relation.target else { unreachable!() };
                record(&mut warnings, Kind::OptionalPartOmitted, "optional_part", "omit", || {
                    (
                        Some(part),
                        Some(format!("relationship:{}", quote(&relation.id))),
                        SelectionFailure::Missing.error(internal),
                    )
                })?;
                continue;
            }
            Err(error) => {
                let Target::Internal(internal) = &relation.target else { unreachable!() };
                return Err(error.error(internal));
            }
        }
        let bytes = package.read_relationship(&relation)?;
        let root_matches = if matches!(
            relation.kind,
            RelationshipKind::Theme | RelationshipKind::Styles | RelationshipKind::Numbering
        ) {
            definitions.read(&bytes, part, root, &mut package.text_budget, &mut warnings)?
        } else {
            let scanned = scan(&bytes, part, root, false, &mut package.text_budget, &mut warnings)?;
            suppress_before_page_break |= scanned.2;
            scanned.1
        };
        if !root_matches {
            record(&mut warnings, Kind::OptionalPartOmitted, "optional_part", "omit", || {
                (
                    Some(part),
                    Some("#root"),
                    "WordprocessingML root does not match relationship kind",
                )
            })?;
        } else {
            if let Target::Internal(internal) = &relation.target {
                valid_roots.insert(internal.id, root);
            }
            relations.admit(&relation, &main)?;
            let loss = match relation.kind {
                RelationshipKind::Numbering => {
                    Some((Kind::FormattingOmitted, "list_definitions", "omit"))
                }
                RelationshipKind::Header => Some((Kind::ContentOmitted, "header", "omit")),
                RelationshipKind::Footer => Some((Kind::ContentOmitted, "footer", "omit")),
                RelationshipKind::Footnotes | RelationshipKind::Endnotes => {
                    Some((Kind::ContentOmitted, "notes", "omit"))
                }
                RelationshipKind::Comments => Some((Kind::ContentOmitted, "comments", "omit")),
                _ => None,
            };
            if let Some((kind, loss, action)) = loss {
                record(&mut warnings, kind, loss, action, || {
                    (
                        Some(part),
                        Some("#root"),
                        if relation.kind == RelationshipKind::Numbering {
                            "Numbering markers and list semantics are not applied; level paragraph formatting is retained"
                        } else {
                            "Part was checked for safety; its content is not applied"
                        },
                    )
                })?;
            }
        }
    }
    let mut resolver = Resolver::new(definitions, &mut warnings)?;
    let bytes = package.read_main()?;
    let sections = scan_formatted(
        &bytes,
        &main,
        &mut package.text_budget,
        &mut warnings,
        &mut resolver,
        &relations,
        suppress_before_page_break,
    )?;
    let registry = resolver.registry;
    let mut document = Document::new();
    for section in sections {
        debug_assert_eq!(section.origins.len(), section.core.paragraphs.len());
        debug_assert!(section.content_width.as_i32() > 0 && section.content_height.as_i32() > 0);
        document.add_section(section.core);
    }
    Ok(DocxDecoded { document, registry, image_store: ImageStore::new(), warnings })
}

pub(crate) fn tracked(name: &str) -> bool {
    matches!(
        name,
        "ins"
            | "del"
            | "moveFrom"
            | "moveTo"
            | "moveFromRangeStart"
            | "moveFromRangeEnd"
            | "moveToRangeStart"
            | "moveToRangeEnd"
            | "pPrChange"
            | "rPrChange"
            | "sectPrChange"
            | "tblPrChange"
            | "tblPrExChange"
            | "tblGridChange"
            | "trPrChange"
            | "tcPrChange"
            | "cellIns"
            | "cellDel"
            | "cellMerge"
            | "numberingChange"
            | "customXmlInsRangeStart"
            | "customXmlInsRangeEnd"
            | "customXmlDelRangeStart"
            | "customXmlDelRangeEnd"
            | "customXmlMoveFromRangeStart"
            | "customXmlMoveFromRangeEnd"
            | "customXmlMoveToRangeStart"
            | "customXmlMoveToRangeEnd"
    )
}

fn attribute<'a>(element: &'a Element, ns: NamespaceId, name: &str) -> Option<&'a str> {
    element
        .attributes
        .iter()
        .find(|(id, key, _)| *id == ns && key == name)
        .map(|(_, _, value)| value.as_str())
}

#[derive(Default)]
enum Node {
    #[default]
    Other,
    Compatibility {
        omitted: bool,
    },
    Body,
    Paragraph,
    Run,
    ParaProperties,
    RunProperties,
    MarkProperties,
    NumberProperties,
    SectionProperties,
    Columns,
    SectionProperty(PropertyState),
    Property(PropertyState),
    Text {
        preserve: bool,
    },
    SimpleField,
    Sdt,
    Alternate {
        fallback: bool,
    },
}
// Store input-local identity, not a formatted diagnostic. Cloning a location
// shares the name; quoting and formatting happen only when a diagnostic is kept.
#[derive(Clone)]
struct Location {
    name: Rc<str>,
    ordinal: usize,
}
impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}#{}", self.name, self.ordinal)
    }
}
struct Frame {
    node: Node,
    blocked: bool,
    in_body: bool,
    // Properties suppress text but still classify nested list/section/mark formatting.
    properties: bool,
    location: Location,
}
struct Field {
    simple: bool,
    result: bool,
    has_result: bool,
    location: Location,
}
struct TrailingBreak {
    base: Delta,
    first: bool,
    mark: Option<hwpforge_foundation::CharShapeIndex>,
    split_at: (Location, usize),
}

struct Body<'a> {
    part: &'a str,
    main: bool,
    root: &'a str,
    root_matches: bool,
    suppress_before_page_break: bool,
    body_seen: bool,
    final_section_seen: bool,
    stack: Vec<Frame>,
    fields: Vec<Field>,
    paragraphs: Vec<Paragraph>,
    sections: Vec<DecodedSection>,
    origins: Vec<Origin>,
    breaks: Vec<(usize, Break)>,
    // At most two output shapes await their final boundary: the last fragment,
    // plus its predecessor when an empty tail may move to the next paragraph.
    pending_shapes: Vec<(usize, Delta)>,
    trailing_break: Option<TrailingBreak>,
    spacing_neighbor: Option<(Option<StyleKey>, bool)>,
    split_at: Option<(Location, usize)>,
    boundary_input: Option<(hwpforge_foundation::CharShapeIndex, bool)>,
    source_paragraphs: usize,
    paragraph_top_level: bool,
    output_paragraphs: usize,
    layout: Option<(Layout, Location)>,
    paragraph_layout: Option<(Layout, Location)>,
    relations: Option<&'a Relations>,
    paragraph: Option<Paragraph>,
    run: Option<(String, bool)>,
    run_diagnosed: bool,
    run_split: bool,
    diagnose_runs: Vec<bool>,
    resolver: Option<&'a mut Resolver>,
    direct_para: Delta,
    direct_run: Delta,
    mark: Option<Delta>,
    mark_style: Option<StyleKey>,
    paragraph_style: Option<StyleKey>,
    run_style: Option<StyleKey>,
    runs: usize,
    ordinal: usize,
    warnings: &'a mut Vec<DocxWarning>,
}

fn scan(
    bytes: &[u8],
    part: &str,
    root: &str,
    main: bool,
    budget: &mut TextBudget,
    warnings: &mut Vec<DocxWarning>,
) -> Result<(Vec<DecodedSection>, bool, bool)> {
    scan_inner(bytes, part, root, main, budget, warnings, None)
}
fn scan_formatted(
    bytes: &[u8],
    part: &str,
    budget: &mut TextBudget,
    warnings: &mut Vec<DocxWarning>,
    resolver: &mut Resolver,
    relations: &Relations,
    suppress_before_page_break: bool,
) -> Result<Vec<DecodedSection>> {
    Ok(scan_inner(
        bytes,
        part,
        "document",
        true,
        budget,
        warnings,
        Some((resolver, relations, suppress_before_page_break)),
    )?
    .0)
}
fn scan_inner(
    bytes: &[u8],
    part: &str,
    root: &str,
    main: bool,
    budget: &mut TextBudget,
    warnings: &mut Vec<DocxWarning>,
    resources: Option<(&mut Resolver, &Relations, bool)>,
) -> Result<(Vec<DecodedSection>, bool, bool)> {
    let (resolver, relations, suppress_before_page_break) =
        resources.map_or((None, None, false), |(r, i, suppress)| (Some(r), Some(i), suppress));
    let mut body = Body {
        part,
        main,
        root,
        root_matches: false,
        suppress_before_page_break,
        body_seen: false,
        final_section_seen: false,
        stack: Vec::new(),
        fields: Vec::new(),
        paragraphs: Vec::new(),
        sections: Vec::new(),
        origins: Vec::new(),
        breaks: Vec::new(),
        trailing_break: None,
        pending_shapes: Vec::new(),
        spacing_neighbor: None,
        split_at: None,
        boundary_input: None,
        source_paragraphs: 0,
        paragraph_top_level: false,
        output_paragraphs: 0,
        layout: None,
        paragraph_layout: None,
        relations,
        paragraph: None,
        run: None,
        run_diagnosed: false,
        run_split: false,
        diagnose_runs: Vec::new(),
        resolver,
        direct_para: Delta::default(),
        direct_run: Delta::default(),
        mark: None,
        mark_style: None,
        paragraph_style: None,
        run_style: None,
        runs: 0,
        ordinal: 0,
        warnings,
    };
    xml::walk(bytes, part, budget, |event| match event {
        XmlEvent::Start(element) => body.start(element),
        XmlEvent::Text(text) => body.text(text),
        XmlEvent::End => body.end(),
    })?;
    if let Some(field) = body.fields.first() {
        return Err(DocxError::invalid(part, &field.location, "unclosed field begin"));
    }
    if main && !body.body_seen {
        return Err(DocxError::invalid(part, "document", "missing body"));
    }
    if main && (!body.paragraphs.is_empty() || !body.final_section_seen) {
        body.close_section(Layout::default(), Location { name: Rc::from("body"), ordinal: 0 })?;
    }
    Ok((body.sections, body.root_matches, body.suppress_before_page_break))
}

impl Body<'_> {
    fn warn<R: std::fmt::Display>(
        &mut self,
        kind: Kind,
        location: &(impl fmt::Display + ?Sized),
        reason: impl FnOnce() -> R,
        loss: &'static str,
        action: &'static str,
    ) -> Result<()> {
        record(self.warnings, kind, loss, action, || {
            (Some(self.part), Some(quote(location)), reason())
        })
    }
    fn visible(&self) -> bool {
        self.fields.iter().all(|f| f.result)
    }
    fn start(&mut self, element: Element) -> Result<()> {
        self.ordinal += 1;
        let location = Location { name: Rc::from(element.name.as_str()), ordinal: self.ordinal };
        let w = element.namespace_id == W_NS.0;
        // This check precedes every output-selection decision, even in ancillary parts.
        if w && tracked(&element.name) {
            return Err(DocxError::TrackedChange {
                part: quote(self.part),
                location: quote(&location),
                element: quote(&element.name),
            });
        }
        if self.stack.is_empty() {
            self.root_matches = w && element.name == self.root;
            if self.main && !self.root_matches {
                return Err(DocxError::invalid(
                    self.part,
                    &location,
                    "expected WordprocessingML document root",
                ));
            }
        }
        let mut frame = Frame {
            node: Node::Other,
            blocked: self.stack.last().is_some_and(|f| f.blocked),
            in_body: self.stack.last().is_some_and(|f| f.in_body),
            properties: self.stack.last().is_some_and(|f| f.properties),
            location: location.clone(),
        };
        if !self.main {
            if self.root == "settings" && self.root_matches {
                if self.stack.len() == 1 {
                    if w && element.name == "compat" {
                        frame.node = Node::Compatibility { omitted: false };
                    } else if w
                        && matches!(
                            element.name.as_str(),
                            "mirrorMargins" | "gutterAtTop" | "rtlGutter"
                        )
                    {
                        layout::warn(
                            self.warnings,
                            self.part,
                            &location,
                            "binding_layout",
                            "omit",
                            "Document-wide mirrored or relocated binding margins are not applied",
                        )?;
                    } else if !(w && element.name == "trackRevisions") {
                        self.warn(
                            Kind::DocumentSettingsOmitted,
                            &location,
                            || "Document setting is not applied",
                            "document_settings",
                            "ignore",
                        )?;
                    }
                } else if let Some(parent) = self.stack.last_mut() {
                    if let Node::Compatibility { omitted } = &mut parent.node {
                        if w && element.name == "suppressSpBfAfterPgBrk" {
                            match attr(&element, "val") {
                                None | Some("1" | "true" | "on") => self.suppress_before_page_break = true,
                                Some("0" | "false" | "off") => self.suppress_before_page_break = false,
                                Some(_) => self.warn(Kind::UnsupportedValue, &location,
                                    || "Invalid suppressSpBfAfterPgBrk boolean; previous value retained",
                                    "document_settings", "inherit_or_default")?,
                            }
                        } else if !*omitted {
                            *omitted = true;
                            let at = parent.location.clone();
                            self.warn(
                                Kind::DocumentSettingsOmitted,
                                &at,
                                || "Unsupported compatibility settings are not applied",
                                "document_settings",
                                "ignore",
                            )?;
                        }
                    }
                }
            }
            self.stack.push(frame);
            return Ok(());
        }
        if w && element.name == "body" && self.stack.len() == 1 {
            if self.body_seen {
                return Err(DocxError::invalid(self.part, &location, "multiple body elements"));
            }
            self.body_seen = true;
            frame.node = Node::Body;
            frame.in_body = true;
            self.attributes(&element, &location)?;
        } else if self.stack.is_empty() {
            self.attributes(&element, &location)?;
        } else {
            // A content control only exposes its direct sdtContent child.
            if self.stack.last().is_some_and(|f| matches!(f.node, Node::Sdt))
                && !(w && element.name == "sdtContent")
            {
                frame.blocked = true;
            }
            if let Some(parent) = self.stack.last_mut() {
                if let Node::Alternate { fallback } = &mut parent.node {
                    if element.namespace_id == MC_NS.0 && element.name == "Fallback" && !*fallback {
                        *fallback = true;
                        if !frame.blocked {
                            self.warn(
                                Kind::AlternateContentSelected,
                                &location,
                                || "Using the first Fallback without feature negotiation",
                                "alternate_content_selection",
                                "use_fallback",
                            )?;
                        }
                    } else {
                        frame.blocked = true;
                    }
                }
            }
            if frame.properties {
                self.property(&element, &mut frame)?;
            }
            if !frame.blocked {
                if !frame.in_body {
                    self.warn(
                        Kind::ContentOmitted,
                        &location,
                        || "Content outside body is excluded",
                        "outside_body",
                        "omit",
                    )?;
                    frame.blocked = true;
                } else {
                    self.semantic(&element, &mut frame)?;
                }
            }
        }
        self.stack.push(frame);
        Ok(())
    }
    fn semantic(&mut self, element: &Element, frame: &mut Frame) -> Result<()> {
        let location_value = frame.location.clone();
        let location = &location_value;
        let w = element.namespace_id == W_NS.0;
        if element.namespace_id == MC_NS.0 {
            match element.name.as_str() {
                "AlternateContent" => frame.node = Node::Alternate { fallback: false },
                "Fallback"
                    if self
                        .stack
                        .last()
                        .is_some_and(|f| matches!(f.node, Node::Alternate { .. })) => {}
                _ => {
                    frame.blocked = true;
                    self.warn(
                        Kind::ContentOmitted,
                        location,
                        || "Unsupported compatibility branch",
                        "unknown_element",
                        "omit",
                    )?;
                }
            }
        } else if element.namespace_id == M_NS.0 {
            self.warn(
                Kind::ContentOmitted,
                location,
                || "Equation content is excluded",
                "math",
                "omit",
            )?;
            frame.blocked = true;
        } else if w {
            match element.name.as_str() {
                "p" => {
                    if self.paragraph.is_some() {
                        return Err(DocxError::invalid(
                            self.part,
                            location,
                            "nested body paragraph",
                        ));
                    }
                    let moved_break = self.move_trailing_break()?;
                    self.output_paragraphs = admit_count(
                        self.part,
                        location,
                        "paragraphs",
                        self.output_paragraphs,
                        MAX_PARAGRAPHS,
                    )?;
                    self.source_paragraphs += 1;
                    self.paragraph_top_level =
                        self.stack.last().is_some_and(|f| matches!(f.node, Node::Body));
                    self.breaks.clear();
                    self.split_at = None;
                    self.diagnose_runs.clear();
                    self.run_diagnosed = false;
                    self.boundary_input = None;
                    self.direct_para = Delta::default();
                    self.direct_run = Delta::default();
                    self.run_style = None;
                    self.mark = None;
                    self.mark_style = None;
                    self.paragraph_style = None;
                    let mut paragraph = Paragraph::new(ParaShapeIndex::new(0));
                    paragraph.page_break = moved_break;
                    self.paragraph = Some(paragraph);
                    frame.node = Node::Paragraph;
                }
                "r" => {
                    if self.paragraph.is_none() || self.run.is_some() {
                        return Err(DocxError::invalid(
                            self.part,
                            location,
                            "run requires a paragraph and cannot nest",
                        ));
                    }
                    self.direct_run = Delta::default();
                    self.run_style = None;
                    self.run = Some((String::new(), self.visible()));
                    self.run_diagnosed = false;
                    self.run_split = false;
                    frame.node = Node::Run;
                }
                "t" => {
                    if self.run.is_none() {
                        return Err(DocxError::invalid(self.part, location, "text requires a run"));
                    }
                    frame.node = Node::Text {
                        preserve: attribute(element, NamespaceId::XML, "space") == Some("preserve"),
                    };
                }
                "tab" => self.append("\t", location)?,
                "cr" | "br" => match attribute(element, W_NS.0, "type") {
                    Some("page" | "column") if element.name == "br" => {
                        self.split(attribute(element, W_NS.0, "type") == Some("column"), location)?;
                    }
                    None | Some("textWrapping") => self.append("\n", location)?,
                    Some(_) => self.warn(
                        Kind::ValueApproximated,
                        location,
                        || "Unknown break type discarded",
                        "unsupported_value",
                        "omit",
                    )?,
                },
                "noBreakHyphen" => self.append("\u{2011}", location)?,
                "softHyphen" => self.append("\u{00ad}", location)?,
                "sectPr" => self.begin_layout(element, frame)?,
                "rPr" | "pPr" => {
                    crate::format::container_attributes(
                        element,
                        self.part,
                        &location.to_string(),
                        self.warnings,
                    )?;
                    frame.node = if element.name == "rPr" {
                        Node::RunProperties
                    } else {
                        Node::ParaProperties
                    };
                    frame.blocked = true;
                    frame.properties = true;
                }
                "fldSimple" => {
                    self.begin_field(true, location)?;
                    frame.node = Node::SimpleField;
                }
                "fldChar" => {
                    self.field_char(attribute(element, W_NS.0, "fldCharType"), location)?
                }
                "instrText" => {
                    if self.fields.is_empty() {
                        self.warn(
                            Kind::FieldInstructionOmitted,
                            location,
                            || "Orphan field instruction discarded",
                            "field_function",
                            "use_saved_result",
                        )?;
                    }
                    frame.blocked = true;
                }
                "hyperlink" => self.warn(
                    Kind::LinkTargetOmitted,
                    location,
                    || "Hyperlink target discarded; run text retained",
                    "hyperlink_target",
                    "keep_text",
                )?,
                "smartTag" | "customXml" | "dir" | "bdo" | "customXmlPr" | "smartTagPr" => {
                    self.warn(
                        Kind::StructuredWrapperFlattened,
                        location,
                        || "Structured wrapper meaning and properties are not applied",
                        "structured_wrapper",
                        "keep_content",
                    )?;
                    frame.blocked = matches!(element.name.as_str(), "customXmlPr" | "smartTagPr");
                    // Attributes describe the same discarded wrapper semantics.
                    return Ok(());
                }
                "sdt" => {
                    self.warn(
                        Kind::ContentControlFlattened,
                        location,
                        || "Lock, binding and check state discarded",
                        "content_control_function",
                        "keep_content",
                    )?;
                    frame.node = Node::Sdt;
                }
                "sdtContent" if self.stack.last().is_some_and(|f| matches!(f.node, Node::Sdt)) => {}
                "lastRenderedPageBreak" | "proofErr" | "bookmarkStart" | "bookmarkEnd" => {
                    frame.blocked = true;
                }
                name => {
                    if name == "tbl" {
                        self.spacing_neighbor = None;
                    }
                    let loss = match name {
                        "tbl" => "table",
                        "drawing" | "pict" => "image_shape",
                        "object" | "altChunk" => "embedded_object",
                        "footnoteReference" | "endnoteReference" => "notes",
                        "commentReference" | "commentRangeStart" | "commentRangeEnd" => "comments",
                        "sym" => "symbol_character",
                        _ => "unknown_element",
                    };
                    self.warn(
                        Kind::ContentOmitted,
                        location,
                        || format!("Unsupported element '{}' excluded", quote(name)),
                        loss,
                        "omit",
                    )?;
                    frame.blocked = true;
                }
            }
        } else {
            self.warn(
                Kind::ContentOmitted,
                location,
                || format!("Unsupported element '{}' excluded", quote(&element.name)),
                "unknown_element",
                "omit",
            )?;
            frame.blocked = true;
        }
        if !frame.blocked {
            self.attributes(element, location)?;
        }
        Ok(())
    }
    fn attributes(&mut self, element: &Element, location: &Location) -> Result<()> {
        for (ns, name, value) in &element.attributes {
            let known = (*ns == W_NS.0
                && (matches!(
                    name.as_str(),
                    "rsidR" | "rsidRPr" | "rsidRDefault" | "rsidP" | "rsidDel" | "rsidSect"
                ) || matches!(
                    (element.name.as_str(), name.as_str()),
                    ("br", "type")
                        | ("fldSimple", "instr" | "fldLock" | "dirty")
                        | ("fldChar", "fldCharType" | "fldLock" | "dirty")
                        | (
                            "hyperlink",
                            "anchor" | "history" | "docLocation" | "tgtFrame" | "tooltip"
                        )
                )))
                || (*ns == R_NS.0 && element.name == "hyperlink" && name == "id");
            if *ns == NamespaceId::XML && name == "space" && element.name == "t" {
                if !matches!(value.as_str(), "preserve" | "default") {
                    self.warn(
                        Kind::ValueApproximated,
                        location,
                        || "Unknown xml:space value; source whitespace retained",
                        "unsupported_value",
                        "preserve_source",
                    )?;
                }
            } else if !known {
                self.warn(
                    Kind::AttributeOmitted,
                    location,
                    || format!("Attribute '{}' is not applied", quote(name)),
                    "unsupported_attribute",
                    "ignore",
                )?;
            }
        }
        Ok(())
    }
    fn mark_field_result(&mut self) {
        for field in self.fields.iter_mut().rev().take_while(|f| f.result) {
            field.has_result = true;
        }
    }
    fn append(&mut self, text: &str, location: &Location) -> Result<()> {
        if !text.is_empty() {
            self.mark_field_result();
        }
        if !self.visible() {
            return Ok(());
        }
        let run = self.run.as_mut().ok_or_else(|| {
            DocxError::invalid(self.part, location, "text control requires a run")
        })?;
        run.0.push_str(text);
        run.1 = true;
        Ok(())
    }
    fn text(&mut self, text: String) -> Result<()> {
        let frame = self
            .stack
            .last()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing parent frame"))?;
        if !self.main || frame.blocked {
            return Ok(());
        }
        let location = frame.location.clone();
        if let Node::Text { preserve } = frame.node {
            if self.visible()
                && !preserve
                && (text.starts_with([' ', '\t', '\r', '\n'])
                    || text.ends_with([' ', '\t', '\r', '\n']))
            {
                self.warn(
                    Kind::WhitespaceApproximated,
                    &location,
                    || "Boundary whitespace without xml:space='preserve' retained",
                    "whitespace_semantics",
                    "preserve_source",
                )?;
            }
            self.append(&text, &location)?;
        } else if !text.trim().is_empty() {
            self.warn(
                Kind::ContentOmitted,
                &location,
                || "Text outside a WordprocessingML text element excluded",
                "unplaced_text",
                "omit",
            )?;
        }
        Ok(())
    }
    fn begin_field(&mut self, simple: bool, location: &Location) -> Result<()> {
        self.warn(
            Kind::FieldInstructionOmitted,
            location,
            || "Field instruction discarded; saved result retained",
            "field_function",
            "use_saved_result",
        )?;
        if !self.fields.is_empty() {
            self.warn(
                Kind::NestedFieldFlattened,
                location,
                || "Nested field functionality discarded",
                "nested_field_function",
                "use_saved_result",
            )?;
        }
        // Complex fields are not XML-nested; use the XML depth cap for this stack too.
        admit_count(self.part, location, "field nesting", self.fields.len(), xml::MAX_DEPTH)?;
        self.fields.push(Field {
            simple,
            result: simple,
            has_result: false,
            location: location.clone(),
        });
        Ok(())
    }
    fn finish_field(&mut self, simple: bool, location: &Location) -> Result<()> {
        let field = self
            .fields
            .last()
            .filter(|f| f.simple == simple)
            .ok_or_else(|| DocxError::invalid(self.part, location, "unmatched field end"))?;
        if !field.has_result {
            let first = field.location.clone();
            self.warn(
                Kind::FieldResultMissing,
                &first,
                || "Field has no saved result; empty text used",
                "field_result",
                "use_empty_result",
            )?;
        }
        self.fields.pop();
        Ok(())
    }
    fn field_char(&mut self, kind: Option<&str>, location: &Location) -> Result<()> {
        if let Some((text, keep)) = &mut self.run {
            if text.is_empty() {
                *keep = false;
            }
        }
        match kind {
            Some("begin") => self.begin_field(false, location),
            Some("separate") => {
                let field =
                    self.fields.last_mut().filter(|f| !f.simple && !f.result).ok_or_else(|| {
                        DocxError::invalid(
                            self.part,
                            location,
                            "unmatched or repeated field separate",
                        )
                    })?;
                field.result = true;
                Ok(())
            }
            Some("end") => self.finish_field(false, location),
            _ => Err(DocxError::invalid(
                self.part,
                location,
                "missing or unknown field character type",
            )),
        }
    }
    fn push_run(&mut self, text: String, location: &(impl fmt::Display + ?Sized)) -> Result<()> {
        self.runs = admit_count(self.part, location, "runs", self.runs, MAX_RUNS)?;
        let id = self
            .resolver
            .as_mut()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing resolver"))?
            .run_input(self.direct_run.clone(), self.run_style)?;
        self.paragraph
            .as_mut()
            .ok_or_else(|| DocxError::invalid(self.part, "p", "missing paragraph"))?
            .add_run(Run::text(text, id));
        self.diagnose_runs.push(!self.run_diagnosed);
        self.run_diagnosed = true;
        Ok(())
    }
    fn warn_split(&mut self, at: &(Location, usize)) -> Result<()> {
        let count = self.warnings.len();
        self.warn(Kind::ParagraphSplitApproximated, &at.0, || "Paragraph split; list grouping, indentation, keep and paragraph-end effects are approximated", "paragraph_split", "split")?;
        // Classification waits for the final fragment count; retain source order.
        if self.warnings.len() > count {
            let warning = self.warnings.pop().unwrap();
            self.warnings.insert(at.1, warning);
        }
        Ok(())
    }
    fn move_trailing_break(&mut self) -> Result<bool> {
        let Some(pending) = self.trailing_break.take() else { return Ok(false) };
        self.pending_shapes.pop();
        let shape = &mut self.pending_shapes.last_mut().unwrap().1;
        *shape = if pending.first {
            pending.base
        } else {
            Resolver::fragment_shape(&pending.base, false, true)
        };
        self.paragraphs.pop();
        self.origins.pop();
        self.output_paragraphs -= 1;
        let previous = self.paragraphs.last_mut().unwrap();
        previous.end_char_shape = pending.mark;
        Ok(true)
    }
    fn finalize_shapes(&mut self, keep: usize) -> Result<()> {
        let ready = self.pending_shapes.len().saturating_sub(keep);
        for (index, delta) in self.pending_shapes.drain(..ready) {
            // The only output paragraph shape assignment: all restoration,
            // fragment derivation and boundary spacing edits precede this point.
            self.paragraphs[index].para_shape_id =
                self.resolver.as_mut().unwrap().para_shape(&delta)?;
        }
        Ok(())
    }
    fn finish_paragraph(&mut self) -> Result<()> {
        let mut paragraph = self
            .paragraph
            .take()
            .ok_or_else(|| DocxError::invalid(self.part, "p", "missing paragraph"))?;
        let moved_break = paragraph.page_break;
        let mut base = self
            .resolver
            .as_mut()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing resolver"))?
            .finish(
                (&mut paragraph, &self.diagnose_runs),
                self.paragraph_style,
                &self.direct_para,
                self.mark.as_ref().map(|d| (d, self.mark_style)),
                self.part,
                self.warnings,
            )?;
        paragraph.page_break |= moved_break;
        let resolver = self.resolver.as_mut().unwrap();
        let style = resolver.paragraph_style(self.paragraph_style);
        let contextual = base.para.contextual == Some(true);
        // Word has a separate suppressSpBfAfterPgBrk option (off by default).
        // Preserve both sides at forced page starts, including approximated columns;
        // this is a compatibility-based choice, not a Word/Hancom layout measurement.
        // Test the final flag AFTER restoring the moved trailing break.
        if paragraph.page_break {
            if self.suppress_before_page_break {
                base.para.before = Some(0);
            }
        } else if let Some((previous_style, previous_contextual)) = self.spacing_neighbor {
            if let Some((_, previous)) = self.pending_shapes.last_mut() {
                let same_style = previous_style == style;
                Resolver::spacing_boundary(
                    previous,
                    &mut base,
                    (same_style && previous_contextual, same_style && contextual),
                );
            }
        }
        self.spacing_neighbor = Some((style, contextual));
        let total = self.breaks.len() + 1;
        let trailing =
            self.breaks.last().is_some_and(|(start, _)| empty_text_runs(&paragraph.runs[*start..]));
        if let Some(at) = self.split_at.take() {
            if !trailing || total > 2 {
                self.warn_split(&at)?;
            }
            if trailing {
                self.trailing_break = Some(TrailingBreak {
                    base: base.clone(),
                    first: total == 2,
                    mark: paragraph.end_char_shape,
                    split_at: at,
                });
            }
        }
        let mut start = 0;
        let run_count = paragraph.runs.len();
        let mut runs = std::mem::take(&mut paragraph.runs).into_iter();
        for fragment in 0..total {
            let end = self.breaks.get(fragment).map_or(run_count, |b| b.0);
            let mut p = paragraph.clone();
            p.runs = runs.by_ref().take(end - start).collect();
            // Empty source runs still participate in source diagnostics, but an
            // empty output fragment has exactly one run at its final effective shape.
            if empty_text_runs(&p.runs) {
                if let Some(last) = p.runs.pop() {
                    p.runs.clear();
                    p.runs.push(last);
                }
            }
            start = end;
            let cause = fragment.checked_sub(1).map(|i| self.breaks[i].1);
            let shape = if total > 1 {
                Resolver::fragment_shape(&base, fragment == 0, fragment + 1 == total)
            } else {
                base.clone()
            };
            if total > 1 {
                p.page_break = if fragment == 0 { paragraph.page_break } else { cause.is_some() };
                if fragment + 1 != total {
                    p.end_char_shape = None;
                }
                if fragment != 0 {
                    p.heading_level = None;
                }
            }
            self.origins.push(Origin {
                paragraph: self.source_paragraphs,
                list_item: self.source_paragraphs,
                fragment,
                cause,
            });
            self.pending_shapes.push((self.paragraphs.len(), shape));
            self.paragraphs.push(p);
            self.finalize_shapes(if trailing { 2 } else { 1 })?;
        }
        Ok(())
    }
    fn begin_layout(&mut self, e: &Element, frame: &mut Frame) -> Result<()> {
        frame.blocked = true;
        frame.properties = true;
        frame.node = Node::SectionProperty(PropertyState::Excluded);
        let valid_parent =
            self.stack.last().is_some_and(|f| matches!(f.node, Node::ParaProperties | Node::Body));
        if !valid_parent || (self.paragraph.is_some() && !self.paragraph_top_level) {
            layout::warn(
                self.warnings,
                self.part,
                &frame.location,
                "nested_section",
                "keep_content",
                "Nested section settings excluded; body content retained",
            )?;
            return Ok(());
        }
        if self.layout.is_some() || self.paragraph_layout.is_some() {
            return Err(DocxError::invalid(
                self.part,
                &frame.location,
                "duplicate section settings",
            ));
        }
        for (ns, key, _) in &e.attributes {
            if !(*ns == W_NS.0
                && matches!(key.as_str(), "rsidR" | "rsidRPr" | "rsidSect" | "rsidDel"))
            {
                layout::warn(
                    self.warnings,
                    self.part,
                    &frame.location,
                    "page_attribute",
                    "omit",
                    "Unsupported section attribute excluded",
                )?;
            }
        }
        self.layout = Some((Layout::default(), frame.location.clone()));
        frame.node = Node::SectionProperties;
        Ok(())
    }
    fn split(&mut self, column: bool, at: &Location) -> Result<()> {
        if self.run.is_none() {
            return Err(DocxError::invalid(self.part, at, "break requires a run"));
        }
        self.mark_field_result();
        if !self.visible() {
            return Ok(());
        }
        self.run_split = true;
        self.output_paragraphs =
            admit_count(self.part, at, "paragraphs", self.output_paragraphs, MAX_PARAGRAPHS)?;
        let text = std::mem::take(&mut self.run.as_mut().unwrap().0);
        if !text.is_empty() {
            self.push_run(text, at)?;
        }
        self.boundary_input = Some((
            self.resolver.as_mut().unwrap().run_input(self.direct_run.clone(), self.run_style)?,
            !self.run_diagnosed,
        ));
        let start = self.breaks.last().map_or(0, |b| b.0);
        let runs = &self.paragraph.as_ref().unwrap().runs[start..];
        if !runs.is_empty() && empty_text_runs(runs) {
            // A preceding empty source run does not own this break's character shape.
            self.push_run(String::new(), at)?;
        }
        self.ensure_fragment_run(at)?;
        if self.run_diagnosed {
            if let Some((id, _)) = self.boundary_input {
                self.boundary_input = Some((id, false));
            }
        }
        if self.breaks.is_empty() {
            self.split_at = Some((at.clone(), self.warnings.len()));
        }
        if column {
            self.warn(
                Kind::ColumnBreakApproximated,
                at,
                || "Column break uses a page break in single-column output",
                "column_break",
                "page_break",
            )?;
        }
        self.breaks.push((
            self.paragraph.as_ref().unwrap().runs.len(),
            if column { Break::Column } else { Break::Page },
        ));
        Ok(())
    }
    fn ensure_fragment_run(&mut self, at: &(impl fmt::Display + ?Sized)) -> Result<()> {
        let start = self.breaks.last().map_or(0, |b| b.0);
        if self.paragraph.as_ref().unwrap().runs.len() == start {
            if let Some((id, diagnose)) = self.boundary_input {
                self.runs = admit_count(self.part, at, "runs", self.runs, MAX_RUNS)?;
                self.paragraph.as_mut().unwrap().add_run(Run::text("", id));
                self.diagnose_runs.push(diagnose);
                self.run_diagnosed = true;
                self.boundary_input = Some((id, false));
            } else {
                self.push_run(String::new(), at)?;
            }
        }
        Ok(())
    }
    fn close_section(&mut self, layout: Layout, at: Location) -> Result<()> {
        let (page, mut references, content_width, content_height) =
            layout.finish(self.part, &at, self.warnings)?;
        if let Some(previous) = self.sections.last() {
            references.inherit_defaults(previous.references);
        }
        if self.paragraphs.is_empty() {
            self.output_paragraphs =
                admit_count(self.part, &at, "paragraphs", self.output_paragraphs, MAX_PARAGRAPHS)?;
            self.source_paragraphs += 1;
            self.direct_para = Delta::default();
            self.direct_run = Delta::default();
            self.paragraph_style = None;
            self.run_style = None;
            self.mark = None;
            self.mark_style = None;
            self.breaks.clear();
            self.diagnose_runs.clear();
            self.run_diagnosed = false;
            self.boundary_input = None;
            self.paragraph = Some(Paragraph::new(ParaShapeIndex::new(0)));
            self.ensure_fragment_run(&at)?;
            self.finish_paragraph()?;
        }
        if let Some(pending) = self.trailing_break.take() {
            if pending.first {
                self.warn_split(&pending.split_at)?;
            }
        }
        self.finalize_shapes(0)?;
        self.spacing_neighbor = None;
        self.sections.push(DecodedSection {
            core: Section::with_paragraphs(std::mem::take(&mut self.paragraphs), page),
            origins: std::mem::take(&mut self.origins),
            references,
            content_width,
            content_height,
        });
        Ok(())
    }
    fn property(&mut self, e: &Element, frame: &mut Frame) -> Result<()> {
        let parent = &self
            .stack
            .last()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing parent frame"))?
            .node;
        let w = e.namespace_id == W_NS.0;
        if matches!(parent, Node::SectionProperties | Node::Columns | Node::SectionProperty(_)) {
            let state = match parent {
                Node::SectionProperties => {
                    let layout = &mut self.layout.as_mut().unwrap().0;
                    let state = layout.read(
                        e,
                        self.part,
                        &frame.location,
                        self.relations,
                        self.warnings,
                    )?;
                    if w && e.name == "cols" {
                        frame.node = Node::Columns;
                        return Ok(());
                    }
                    state
                }
                Node::Columns if w && e.name == "col" => {
                    self.layout.as_mut().unwrap().0.approximate_columns(
                        self.part,
                        &frame.location,
                        self.warnings,
                    )?;
                    PropertyState::Excluded
                }
                Node::SectionProperty(PropertyState::Excluded) => PropertyState::Excluded,
                _ => layout::exclude(self.part, &frame.location, self.warnings)?,
            };
            frame.node = Node::SectionProperty(state);
            return Ok(());
        }
        if w && e.name == "sectPr" && matches!(parent, Node::ParaProperties) {
            return self.begin_layout(e, frame);
        }
        if let Node::Property(state) = parent {
            frame.node = Node::Property(state.child(
                e,
                self.part,
                &frame.location.to_string(),
                self.warnings,
            )?);
            return Ok(());
        }
        if w && matches!(e.name.as_str(), "rPr" | "pPr") {
            crate::format::container_attributes(
                e,
                self.part,
                &frame.location.to_string(),
                self.warnings,
            )?;
        }
        let character = matches!(parent, Node::RunProperties | Node::MarkProperties);
        let target = match parent {
            Node::ParaProperties | Node::NumberProperties => &mut self.direct_para,
            Node::RunProperties => &mut self.direct_run,
            Node::MarkProperties => self.mark.get_or_insert_with(Delta::default),
            _ => return Ok(()),
        };
        if w && e.name == "rPr" && matches!(parent, Node::ParaProperties) {
            frame.node = Node::MarkProperties;
            self.mark.get_or_insert_with(Delta::default);
            return Ok(());
        }
        if w && e.name == "numPr" && matches!(parent, Node::ParaProperties) {
            frame.node = Node::NumberProperties;
        }
        if w && e.name == "pStyle" && matches!(parent, Node::ParaProperties) {
            self.paragraph_style = attr(e, "val")
                .map(|id| {
                    self.resolver
                        .as_ref()
                        .ok_or_else(|| DocxError::invalid(self.part, "pStyle", "missing resolver"))?
                        .definitions
                        .style_key(id, self.part, "pStyle")
                })
                .transpose()?;
        }
        if w && e.name == "rStyle" && character {
            let style = attr(e, "val")
                .map(|id| {
                    self.resolver
                        .as_ref()
                        .ok_or_else(|| DocxError::invalid(self.part, "rStyle", "missing resolver"))?
                        .definitions
                        .style_key(id, self.part, "rStyle")
                })
                .transpose()?;
            if matches!(parent, Node::MarkProperties) {
                self.mark_style = style;
            } else {
                self.run_style = style;
            }
        }
        let state = self
            .resolver
            .as_mut()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing resolver"))?
            .property(
                target,
                e,
                character,
                self.part,
                &frame.location.to_string(),
                self.warnings,
            )?;
        if !matches!(frame.node, Node::NumberProperties) {
            frame.node = Node::Property(state);
        }
        Ok(())
    }
    fn end(&mut self) -> Result<()> {
        let frame = self
            .stack
            .pop()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing closing frame"))?;
        match frame.node {
            Node::Run => {
                let (text, keep) = self
                    .run
                    .take()
                    .ok_or_else(|| DocxError::invalid(self.part, "r", "missing run"))?;
                if keep && (!text.is_empty() || !self.run_split) {
                    self.push_run(text, &frame.location)?;
                }
            }
            Node::Paragraph => {
                self.ensure_fragment_run(&frame.location)?;
                self.finish_paragraph()?;
                if let Some((layout, at)) = self.paragraph_layout.take() {
                    self.close_section(layout, at)?;
                }
            }
            Node::SectionProperties => {
                if let Some((layout, at)) = self.layout.take() {
                    if self.paragraph.is_some() {
                        self.paragraph_layout = Some((layout, at));
                    } else {
                        self.final_section_seen = true;
                        self.close_section(layout, at)?;
                    }
                }
            }
            Node::SimpleField => self.finish_field(true, &frame.location)?,
            Node::Alternate { fallback: false } if !frame.blocked => self.warn(
                Kind::ContentOmitted,
                &frame.location,
                || "AlternateContent has no Fallback; content excluded",
                "alternate_content",
                "omit",
            )?,
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod format_tests;

#[cfg(test)]
mod layout_tests;

#[cfg(test)]
mod line_spacing_tests;
