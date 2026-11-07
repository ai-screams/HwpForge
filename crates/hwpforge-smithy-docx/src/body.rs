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
    document::Document, image::ImageStore, page::PageSettings, paragraph::Paragraph, run::Run,
    section::Section,
};
use hwpforge_foundation::ParaShapeIndex;
use std::{collections::HashSet, fmt, rc::Rc};

const MAX_PARAGRAPHS: usize = 1_000_000;
const MAX_RUNS: usize = 4_000_000;

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
            scan(&bytes, part, root, false, &mut package.text_budget, &mut warnings)?.1
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
    let paragraphs =
        scan_formatted(&bytes, &main, &mut package.text_budget, &mut warnings, &mut resolver)?;
    let registry = resolver.registry;
    let mut document = Document::new();
    document.add_section(Section::with_paragraphs(paragraphs, PageSettings::default()));
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
    Body,
    Paragraph,
    Run,
    ParaProperties,
    RunProperties,
    MarkProperties,
    NumberProperties,
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
struct Body<'a> {
    part: &'a str,
    main: bool,
    root: &'a str,
    root_matches: bool,
    body_seen: bool,
    stack: Vec<Frame>,
    fields: Vec<Field>,
    paragraphs: Vec<Paragraph>,
    paragraph: Option<Paragraph>,
    run: Option<(String, bool)>,
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
) -> Result<(Vec<Paragraph>, bool)> {
    scan_inner(bytes, part, root, main, budget, warnings, None)
}
fn scan_formatted(
    bytes: &[u8],
    part: &str,
    budget: &mut TextBudget,
    warnings: &mut Vec<DocxWarning>,
    resolver: &mut Resolver,
) -> Result<Vec<Paragraph>> {
    Ok(scan_inner(bytes, part, "document", true, budget, warnings, Some(resolver))?.0)
}
fn scan_inner(
    bytes: &[u8],
    part: &str,
    root: &str,
    main: bool,
    budget: &mut TextBudget,
    warnings: &mut Vec<DocxWarning>,
    resolver: Option<&mut Resolver>,
) -> Result<(Vec<Paragraph>, bool)> {
    let mut body = Body {
        part,
        main,
        root,
        root_matches: false,
        body_seen: false,
        stack: Vec::new(),
        fields: Vec::new(),
        paragraphs: Vec::new(),
        paragraph: None,
        run: None,
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
    // Core requires a paragraph even when all source objects were excluded.
    if main && body.paragraphs.is_empty() {
        admit_count(part, "#body", "paragraphs", 0, MAX_PARAGRAPHS)?;
        body.paragraph = Some(Paragraph::new(ParaShapeIndex::new(0)));
        body.push_run(String::new(), "#body")?;
        body.finish_paragraph()?;
    }
    Ok((body.paragraphs, body.root_matches))
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
            if self.root == "settings"
                && self.root_matches
                && self.stack.len() == 1
                && !(w && element.name == "trackRevisions")
            {
                self.warn(
                    Kind::DocumentSettingsOmitted,
                    &location,
                    || "Document setting is not applied",
                    "document_settings",
                    "ignore",
                )?;
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
        let location = &frame.location;
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
                    admit_count(
                        self.part,
                        location,
                        "paragraphs",
                        self.paragraphs.len(),
                        MAX_PARAGRAPHS,
                    )?;
                    self.direct_para = Delta::default();
                    self.direct_run = Delta::default();
                    self.run_style = None;
                    self.mark = None;
                    self.mark_style = None;
                    self.paragraph_style = None;
                    self.paragraph = Some(Paragraph::new(ParaShapeIndex::new(0)));
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
                        let loss = if attribute(element, W_NS.0, "type") == Some("page") {
                            "page_break"
                        } else {
                            "column_break"
                        };
                        self.warn(
                            Kind::BreakOmitted,
                            location,
                            || "Break discarded; paragraph is not split",
                            loss,
                            "omit",
                        )?;
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
                "rPr" | "pPr" | "sectPr" => {
                    if element.name != "sectPr" {
                        crate::format::container_attributes(
                            element,
                            self.part,
                            &location.to_string(),
                            self.warnings,
                        )?;
                    }
                    match element.name.as_str() {
                        "rPr" => frame.node = Node::RunProperties,
                        "pPr" => frame.node = Node::ParaProperties,
                        _ => self.warn(
                            Kind::FormattingOmitted,
                            location,
                            || "Section and page settings use Core defaults",
                            "section_layout",
                            "use_defaults",
                        )?,
                    }
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
    fn append(&mut self, text: &str, location: &Location) -> Result<()> {
        if !text.is_empty() {
            for field in self.fields.iter_mut().rev().take_while(|f| f.result) {
                field.has_result = true;
            }
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
        Ok(())
    }
    fn finish_paragraph(&mut self) -> Result<()> {
        let mut paragraph = self
            .paragraph
            .take()
            .ok_or_else(|| DocxError::invalid(self.part, "p", "missing paragraph"))?;
        self.resolver
            .as_mut()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing resolver"))?
            .finish(
                &mut paragraph,
                self.paragraph_style,
                &self.direct_para,
                self.mark.as_ref().map(|d| (d, self.mark_style)),
                self.part,
                self.warnings,
            )?;
        self.paragraphs.push(paragraph);
        Ok(())
    }
    fn property(&mut self, e: &Element, frame: &mut Frame) -> Result<()> {
        let parent = &self
            .stack
            .last()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing parent frame"))?
            .node;
        let w = e.namespace_id == W_NS.0;
        if w && self
            .stack
            .last()
            .ok_or_else(|| DocxError::invalid(self.part, "body", "missing parent frame"))?
            .location
            .name
            .as_ref()
            == "sectPr"
        {
            crate::format::structural(e, self.part)?;
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
                if keep {
                    self.push_run(text, &frame.location)?;
                }
            }
            Node::Paragraph => {
                if self
                    .paragraph
                    .as_ref()
                    .ok_or_else(|| DocxError::invalid(self.part, "p", "missing paragraph"))?
                    .runs
                    .is_empty()
                {
                    self.push_run(String::new(), &frame.location)?;
                }
                self.finish_paragraph()?;
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
