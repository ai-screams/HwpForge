//! Semantic HWP5 IR and parser-only audit contracts.
//!
//! This module intentionally does **not** parse HWP5 bytes yet. It defines the
//! typed target that later parser work should populate after package and record
//! interpretation has been stabilized by fixture-driven research.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use hwpforge_foundation::Index;

use crate::{Hwp5BinDataRecordSummary, Hwp5BinDataStream, Hwp5PackageEntry};

fn is_false(value: &bool) -> bool {
    !*value
}

/// Marker for section-local semantic identifiers.
pub struct Hwp5SemanticSectionMarker;

/// Stable identifier for a semantic section local to one HWP5 semantic document.
pub type Hwp5SemanticSectionId = Index<Hwp5SemanticSectionMarker>;

/// Marker for paragraph-local semantic identifiers.
pub struct Hwp5SemanticParagraphMarker;

/// Stable identifier for a semantic paragraph local to one HWP5 semantic document.
pub type Hwp5SemanticParagraphId = Index<Hwp5SemanticParagraphMarker>;

/// Marker for control-local semantic identifiers.
pub struct Hwp5SemanticControlMarker;

/// Stable identifier for a semantic control node local to one HWP5 semantic document.
pub type Hwp5SemanticControlId = Index<Hwp5SemanticControlMarker>;

/// Marker for unresolved-item semantic identifiers.
pub struct Hwp5SemanticUnresolvedMarker;

/// Stable identifier for an unresolved semantic item local to one HWP5 semantic document.
pub type Hwp5SemanticUnresolvedId = Index<Hwp5SemanticUnresolvedMarker>;

/// Semantic HWP5 document assembled after package and record interpretation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticDocument {
    /// Package-level metadata kept outside Core projection.
    pub package_meta: Hwp5SemanticPackageMeta,
    /// Interpreted DocInfo content.
    pub doc_info: Hwp5SemanticDocInfo,
    /// Section-level semantic content.
    pub sections: Vec<Hwp5SemanticSection>,
    /// Inter-control relationships that should survive projection.
    pub control_graph: Vec<Hwp5SemanticControlEdge>,
    /// Explicitly preserved unresolved or unsupported facts.
    pub unresolved: Vec<Hwp5UnresolvedItem>,
}

impl Hwp5SemanticDocument {
    /// Builds an empty semantic document with package metadata.
    #[must_use]
    pub fn new(package_meta: Hwp5SemanticPackageMeta) -> Self {
        Self {
            package_meta,
            doc_info: Hwp5SemanticDocInfo::default(),
            sections: Vec::new(),
            control_graph: Vec::new(),
            unresolved: Vec::new(),
        }
    }

    /// Produces a parser-only audit snapshot from the semantic IR.
    ///
    /// The snapshot is intentionally simpler than the document itself so CLI
    /// tools can compare parser output against controlled fixture expectations
    /// before any Core/HWPX projection is involved.
    #[must_use]
    pub fn parser_audit_snapshot(&self) -> Hwp5ParserAuditSnapshot {
        let mut audit = ParserAuditAccumulator::default();
        accumulate_unresolved_container_counts(
            &self.unresolved,
            &mut audit.unresolved_container_counts,
        );

        let section_summaries: Vec<Hwp5ParserAuditSection> = self
            .sections
            .iter()
            .map(|section| build_section_audit_summary(section, &self.unresolved, &mut audit))
            .collect();

        Hwp5ParserAuditSnapshot {
            version: self.package_meta.version.clone(),
            section_count: self.sections.len(),
            paragraph_count: audit.total_paragraph_count,
            control_count: audit.total_control_count,
            unresolved_count: self.unresolved.len(),
            container_counts: to_container_counts(audit.container_counts),
            paragraph_container_counts: to_container_counts(audit.paragraph_container_counts),
            control_container_counts: to_container_counts(audit.control_container_counts),
            unresolved_container_counts: to_optional_container_counts(
                audit.unresolved_container_counts,
            ),
            control_counts: to_control_counts(audit.control_counts),
            container_owner_counts: to_container_owner_counts(audit.container_owner_counts),
            container_control_counts: to_container_control_counts(audit.container_control_counts),
            sections: section_summaries,
        }
    }

    /// Returns graph-integrity issues discovered in the semantic IR.
    ///
    /// The semantic graph is intentionally local to one HWP5 document, so
    /// identifiers must be unique across the entire document and all graph
    /// references must resolve to existing nodes.
    #[must_use]
    pub fn graph_integrity_issues(&self) -> Vec<Hwp5SemanticGraphIntegrityIssue> {
        let mut issues: Vec<Hwp5SemanticGraphIntegrityIssue> = Vec::new();
        let graph_index = collect_duplicate_id_issues(self, &mut issues);
        collect_paragraph_integrity_issues(self, &graph_index.control_ids, &mut issues);
        collect_control_anchor_issues(self, &graph_index.paragraph_ids, &mut issues);
        collect_control_edge_issues(self, &graph_index.control_ids, &mut issues);
        collect_unresolved_section_issues(self, &graph_index.section_ids, &mut issues);

        issues.sort();
        issues.dedup();
        issues
    }

    /// Returns `true` when every semantic-local identifier is unique and all
    /// graph references resolve to existing semantic nodes.
    #[must_use]
    pub fn graph_is_coherent(&self) -> bool {
        self.graph_integrity_issues().is_empty()
    }
}

#[derive(Default)]
struct ParserAuditAccumulator {
    total_paragraph_count: usize,
    total_control_count: usize,
    container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    paragraph_container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    control_container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    unresolved_container_counts: BTreeMap<Option<Hwp5SemanticContainerKind>, usize>,
    control_counts: BTreeMap<Hwp5SemanticControlKind, usize>,
    container_owner_counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
    container_control_counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
}

#[derive(Default)]
struct GraphIntegrityIndex {
    section_ids: BTreeSet<Hwp5SemanticSectionId>,
    paragraph_ids: BTreeSet<Hwp5SemanticParagraphId>,
    control_ids: BTreeSet<Hwp5SemanticControlId>,
    unresolved_ids: BTreeSet<Hwp5SemanticUnresolvedId>,
}

#[derive(Default)]
struct SectionAuditAccumulator {
    container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    paragraph_container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    control_container_counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
    unresolved_container_counts: BTreeMap<Option<Hwp5SemanticContainerKind>, usize>,
    control_counts: BTreeMap<Hwp5SemanticControlKind, usize>,
    container_owner_counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
    container_control_counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
}

fn collect_duplicate_id_issues(
    document: &Hwp5SemanticDocument,
    issues: &mut Vec<Hwp5SemanticGraphIntegrityIssue>,
) -> GraphIntegrityIndex {
    let mut index = GraphIntegrityIndex::default();

    for section in &document.sections {
        if !index.section_ids.insert(section.section_id) {
            issues.push(Hwp5SemanticGraphIntegrityIssue::DuplicateSectionId {
                section_id: section.section_id,
            });
        }

        for paragraph in &section.paragraphs {
            if !index.paragraph_ids.insert(paragraph.paragraph_id) {
                issues.push(Hwp5SemanticGraphIntegrityIssue::DuplicateParagraphId {
                    paragraph_id: paragraph.paragraph_id,
                });
            }
        }

        for control in &section.controls {
            if !index.control_ids.insert(control.node_id) {
                issues.push(Hwp5SemanticGraphIntegrityIssue::DuplicateControlId {
                    control_id: control.node_id,
                });
            }
        }
    }

    for item in &document.unresolved {
        if !index.unresolved_ids.insert(item.item_id) {
            issues.push(Hwp5SemanticGraphIntegrityIssue::DuplicateUnresolvedId {
                unresolved_id: item.item_id,
            });
        }
    }

    index
}

fn collect_paragraph_integrity_issues(
    document: &Hwp5SemanticDocument,
    control_ids: &BTreeSet<Hwp5SemanticControlId>,
    issues: &mut Vec<Hwp5SemanticGraphIntegrityIssue>,
) {
    for section in &document.sections {
        for paragraph in &section.paragraphs {
            if paragraph.inline_text_summary() != paragraph.text {
                issues.push(Hwp5SemanticGraphIntegrityIssue::ParagraphTextSummaryMismatch {
                    paragraph_id: paragraph.paragraph_id,
                });
            }

            if paragraph.inline_control_ids() != paragraph.control_ids {
                issues.push(Hwp5SemanticGraphIntegrityIssue::ParagraphControlInventoryMismatch {
                    paragraph_id: paragraph.paragraph_id,
                });
            }

            for &control_id in &paragraph.control_ids {
                if !control_ids.contains(&control_id) {
                    issues.push(Hwp5SemanticGraphIntegrityIssue::DanglingParagraphControlRef {
                        paragraph_id: paragraph.paragraph_id,
                        control_id,
                    });
                }
            }

            if let Some(owner_control_id) = paragraph.owner_control_id {
                if !control_ids.contains(&owner_control_id) {
                    issues.push(
                        Hwp5SemanticGraphIntegrityIssue::DanglingParagraphOwnerControlRef {
                            paragraph_id: paragraph.paragraph_id,
                            control_id: owner_control_id,
                        },
                    );
                }
            }
        }
    }
}

fn collect_control_anchor_issues(
    document: &Hwp5SemanticDocument,
    paragraph_ids: &BTreeSet<Hwp5SemanticParagraphId>,
    issues: &mut Vec<Hwp5SemanticGraphIntegrityIssue>,
) {
    for section in &document.sections {
        for control in &section.controls {
            if let Some(anchor_paragraph_id) = control.anchor_paragraph_id {
                if !paragraph_ids.contains(&anchor_paragraph_id) {
                    issues.push(
                        Hwp5SemanticGraphIntegrityIssue::DanglingControlAnchorParagraphRef {
                            control_id: control.node_id,
                            paragraph_id: anchor_paragraph_id,
                        },
                    );
                }
            }
        }
    }
}

fn collect_control_edge_issues(
    document: &Hwp5SemanticDocument,
    control_ids: &BTreeSet<Hwp5SemanticControlId>,
    issues: &mut Vec<Hwp5SemanticGraphIntegrityIssue>,
) {
    for edge in &document.control_graph {
        if !control_ids.contains(&edge.from_node_id) {
            issues.push(Hwp5SemanticGraphIntegrityIssue::DanglingControlEdgeFrom {
                from_node_id: edge.from_node_id,
                to_node_id: edge.to_node_id,
            });
        }
        if !control_ids.contains(&edge.to_node_id) {
            issues.push(Hwp5SemanticGraphIntegrityIssue::DanglingControlEdgeTo {
                from_node_id: edge.from_node_id,
                to_node_id: edge.to_node_id,
            });
        }
    }
}

fn collect_unresolved_section_issues(
    document: &Hwp5SemanticDocument,
    section_ids: &BTreeSet<Hwp5SemanticSectionId>,
    issues: &mut Vec<Hwp5SemanticGraphIntegrityIssue>,
) {
    for item in &document.unresolved {
        if let Some(section_id) = item.section_id {
            if !section_ids.contains(&section_id) {
                issues.push(Hwp5SemanticGraphIntegrityIssue::DanglingUnresolvedSectionRef {
                    unresolved_id: item.item_id,
                    section_id,
                });
            }
        }
    }
}

fn build_section_audit_summary(
    section: &Hwp5SemanticSection,
    unresolved: &[Hwp5UnresolvedItem],
    audit: &mut ParserAuditAccumulator,
) -> Hwp5ParserAuditSection {
    let mut section_audit = SectionAuditAccumulator::default();
    let control_kind_by_id: BTreeMap<Hwp5SemanticControlId, Hwp5SemanticControlKind> =
        section.controls.iter().map(|control| (control.node_id, control.kind.clone())).collect();

    audit.total_paragraph_count += section.paragraphs.len();
    audit.total_control_count += section.controls.len();

    for paragraph in &section.paragraphs {
        accumulate_paragraph_audit_counts(
            paragraph,
            &control_kind_by_id,
            audit,
            &mut section_audit,
        );
    }

    for control in &section.controls {
        accumulate_control_audit_counts(control, audit, &mut section_audit);
    }

    let section_unresolved_items: Vec<&Hwp5UnresolvedItem> =
        collect_section_unresolved_items(unresolved, section.section_id);
    accumulate_section_unresolved_counts(&section_unresolved_items, &mut section_audit);

    Hwp5ParserAuditSection {
        index: section.index,
        paragraph_count: section.paragraphs.len(),
        control_count: section.controls.len(),
        unresolved_count: section_unresolved_items.len(),
        container_counts: to_container_counts(section_audit.container_counts),
        paragraph_container_counts: to_container_counts(section_audit.paragraph_container_counts),
        control_container_counts: to_container_counts(section_audit.control_container_counts),
        unresolved_container_counts: to_optional_container_counts(
            section_audit.unresolved_container_counts,
        ),
        control_counts: to_control_counts(section_audit.control_counts),
        container_owner_counts: to_container_owner_counts(section_audit.container_owner_counts),
        container_control_counts: to_container_control_counts(
            section_audit.container_control_counts,
        ),
    }
}

fn accumulate_unresolved_container_counts(
    unresolved_items: &[Hwp5UnresolvedItem],
    counts: &mut BTreeMap<Option<Hwp5SemanticContainerKind>, usize>,
) {
    for unresolved in unresolved_items {
        let terminal_kind: Option<Hwp5SemanticContainerKind> =
            unresolved.container.as_ref().map(Hwp5SemanticContainerPath::terminal_kind).cloned();
        *counts.entry(terminal_kind).or_insert(0) += 1;
    }
}

fn accumulate_paragraph_audit_counts(
    paragraph: &Hwp5SemanticParagraph,
    control_kind_by_id: &BTreeMap<Hwp5SemanticControlId, Hwp5SemanticControlKind>,
    audit: &mut ParserAuditAccumulator,
    section_audit: &mut SectionAuditAccumulator,
) {
    let terminal_kind: Hwp5SemanticContainerKind = paragraph.container.terminal_kind().clone();
    *audit.container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *section_audit.container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *audit.paragraph_container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *section_audit.paragraph_container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;

    if let Some(owner_control_id) = paragraph.owner_control_id {
        if let Some(owner_kind) = control_kind_by_id.get(&owner_control_id) {
            *audit
                .container_owner_counts
                .entry((terminal_kind.clone(), owner_kind.clone()))
                .or_insert(0) += 1;
            *section_audit
                .container_owner_counts
                .entry((terminal_kind, owner_kind.clone()))
                .or_insert(0) += 1;
        }
    }
}

fn accumulate_control_audit_counts(
    control: &Hwp5SemanticControlNode,
    audit: &mut ParserAuditAccumulator,
    section_audit: &mut SectionAuditAccumulator,
) {
    let terminal_kind: Hwp5SemanticContainerKind = control.container.terminal_kind().clone();
    *audit.container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *section_audit.container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *audit.control_container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *section_audit.control_container_counts.entry(terminal_kind.clone()).or_insert(0) += 1;
    *audit.control_counts.entry(control.kind.clone()).or_insert(0) += 1;
    *section_audit.control_counts.entry(control.kind.clone()).or_insert(0) += 1;
    *audit
        .container_control_counts
        .entry((terminal_kind.clone(), control.kind.clone()))
        .or_insert(0) += 1;
    *section_audit
        .container_control_counts
        .entry((terminal_kind, control.kind.clone()))
        .or_insert(0) += 1;
}

fn collect_section_unresolved_items(
    unresolved: &[Hwp5UnresolvedItem],
    section_id: Hwp5SemanticSectionId,
) -> Vec<&Hwp5UnresolvedItem> {
    unresolved.iter().filter(|item| item.section_id == Some(section_id)).collect()
}

fn accumulate_section_unresolved_counts(
    unresolved_items: &[&Hwp5UnresolvedItem],
    section_audit: &mut SectionAuditAccumulator,
) {
    for unresolved in unresolved_items {
        let terminal_kind: Option<Hwp5SemanticContainerKind> =
            unresolved.container.as_ref().map(Hwp5SemanticContainerPath::terminal_kind).cloned();
        *section_audit.unresolved_container_counts.entry(terminal_kind).or_insert(0) += 1;
    }
}

/// Typed graph-integrity issue found in a semantic HWP5 document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Hwp5SemanticGraphIntegrityIssue {
    /// Two semantic sections reused the same document-local section identifier.
    DuplicateSectionId {
        /// Reused semantic section identifier.
        section_id: Hwp5SemanticSectionId,
    },
    /// Two semantic paragraphs reused the same document-local paragraph identifier.
    DuplicateParagraphId {
        /// Reused semantic paragraph identifier.
        paragraph_id: Hwp5SemanticParagraphId,
    },
    /// Two semantic controls reused the same document-local control identifier.
    DuplicateControlId {
        /// Reused semantic control identifier.
        control_id: Hwp5SemanticControlId,
    },
    /// Two unresolved items reused the same document-local unresolved identifier.
    DuplicateUnresolvedId {
        /// Reused semantic unresolved identifier.
        unresolved_id: Hwp5SemanticUnresolvedId,
    },
    /// A paragraph referenced a control identifier that does not exist.
    DanglingParagraphControlRef {
        /// Paragraph holding the broken reference.
        paragraph_id: Hwp5SemanticParagraphId,
        /// Missing control identifier.
        control_id: Hwp5SemanticControlId,
    },
    /// A paragraph subtree owner referenced a control identifier that does not exist.
    DanglingParagraphOwnerControlRef {
        /// Paragraph holding the broken owner reference.
        paragraph_id: Hwp5SemanticParagraphId,
        /// Missing owner control identifier.
        control_id: Hwp5SemanticControlId,
    },
    /// A paragraph's inline text summary diverged from its flattened `text`.
    ParagraphTextSummaryMismatch {
        /// Paragraph with the mismatched text summary.
        paragraph_id: Hwp5SemanticParagraphId,
    },
    /// A paragraph's inline control inventory diverged from `control_ids`.
    ParagraphControlInventoryMismatch {
        /// Paragraph with the mismatched control inventory.
        paragraph_id: Hwp5SemanticParagraphId,
    },
    /// A control referenced an anchor paragraph identifier that does not exist.
    DanglingControlAnchorParagraphRef {
        /// Control with the broken anchor reference.
        control_id: Hwp5SemanticControlId,
        /// Missing paragraph identifier.
        paragraph_id: Hwp5SemanticParagraphId,
    },
    /// A graph edge pointed from a missing control node.
    DanglingControlEdgeFrom {
        /// Missing source control identifier.
        from_node_id: Hwp5SemanticControlId,
        /// Target control identifier recorded on the edge.
        to_node_id: Hwp5SemanticControlId,
    },
    /// A graph edge pointed to a missing control node.
    DanglingControlEdgeTo {
        /// Source control identifier recorded on the edge.
        from_node_id: Hwp5SemanticControlId,
        /// Missing target control identifier.
        to_node_id: Hwp5SemanticControlId,
    },
    /// An unresolved item referenced a missing section identifier.
    DanglingUnresolvedSectionRef {
        /// Unresolved item with the broken section reference.
        unresolved_id: Hwp5SemanticUnresolvedId,
        /// Missing section identifier.
        section_id: Hwp5SemanticSectionId,
    },
}

/// Package-level metadata retained by the semantic IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticPackageMeta {
    /// HWP5 version string observed in the `FileHeader`.
    pub version: String,
    /// Whether main streams are compressed at the document level.
    pub compressed: bool,
    /// Raw package entry inventory.
    pub package_entries: Vec<Hwp5PackageEntry>,
    /// Parsed `DocInfo/BinData` references.
    pub bin_data_records: Vec<Hwp5BinDataRecordSummary>,
    /// Package `/BinData/*` stream inventory.
    pub bin_data_streams: Vec<Hwp5BinDataStream>,
}

/// Interpreted DocInfo content carried by the semantic IR.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticDocInfo {
    /// Observed font face names in canonical order.
    pub font_faces: Vec<String>,
    /// Named style references.
    pub named_styles: Vec<Hwp5SemanticNamedStyleRef>,
    /// Character-shape identifiers referenced by semantic content.
    pub char_shape_ids: Vec<u16>,
    /// Paragraph-shape identifiers referenced by semantic content.
    pub para_shape_ids: Vec<u16>,
}

/// Named style metadata preserved from DocInfo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticNamedStyleRef {
    /// Zero-based style identifier when known.
    pub style_id: Option<u16>,
    /// Human-readable style name from HWP5.
    pub name: String,
    /// Linked paragraph-shape identifier.
    pub para_shape_id: Option<u16>,
    /// Linked character-shape identifier.
    pub char_shape_id: Option<u16>,
}

/// Semantic section content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticSection {
    /// Stable semantic-local section identifier.
    pub section_id: Hwp5SemanticSectionId,
    /// Zero-based section index.
    pub index: usize,
    /// Section-level page definition metadata when the parser recovered it.
    pub page_def: Option<Hwp5SemanticPageDefSummary>,
    /// Semantic paragraphs reconstructed inside the section.
    pub paragraphs: Vec<Hwp5SemanticParagraph>,
    /// Controls anchored to the section.
    pub controls: Vec<Hwp5SemanticControlNode>,
}

/// Minimal page-definition payload preserved on a semantic section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticPageDefSummary {
    /// Page width in HWPUNIT.
    pub width: u32,
    /// Page height in HWPUNIT.
    pub height: u32,
    /// Left margin in HWPUNIT.
    pub margin_left: u32,
    /// Right margin in HWPUNIT.
    pub margin_right: u32,
    /// Top margin in HWPUNIT.
    pub margin_top: u32,
    /// Bottom margin in HWPUNIT.
    pub margin_bottom: u32,
    /// Header margin in HWPUNIT.
    pub header_margin: u32,
    /// Footer margin in HWPUNIT.
    pub footer_margin: u32,
    /// Gutter in HWPUNIT.
    pub gutter: u32,
    /// Whether the section page is landscape.
    pub landscape: bool,
}

/// Semantic paragraph reconstructed from HWP5 records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticParagraph {
    /// Stable semantic-local paragraph identifier.
    pub paragraph_id: Hwp5SemanticParagraphId,
    /// Zero-based paragraph index inside the section.
    pub paragraph_index: usize,
    /// Semantic container path.
    pub container: Hwp5SemanticContainerPath,
    /// Semantic control subtree owner when the paragraph belongs to a nested control.
    pub owner_control_id: Option<Hwp5SemanticControlId>,
    /// Ordered inline content observed inside the paragraph.
    ///
    /// This preserves `text -> control -> text` sequencing for future nested
    /// control work while keeping the first semantic slice lightweight.
    pub inline_items: Vec<Hwp5SemanticInlineItem>,
    /// Flattened text content currently known for the paragraph.
    pub text: String,
    /// Named style identifier, when available.
    pub style_id: Option<u16>,
    /// Character-shape run count preserved for audit.
    pub char_shape_run_count: usize,
    /// Control-node identifiers anchored to this paragraph.
    pub control_ids: Vec<Hwp5SemanticControlId>,
}

impl Hwp5SemanticParagraph {
    /// Returns the flattened text represented by the ordered inline items.
    #[must_use]
    pub fn inline_text_summary(&self) -> String {
        let mut summary = String::new();
        for item in &self.inline_items {
            if let Hwp5SemanticInlineItem::Text { text } = item {
                summary.push_str(text);
            }
        }
        summary
    }

    /// Returns paragraph-local control identifiers in inline order.
    #[must_use]
    pub fn inline_control_ids(&self) -> Vec<Hwp5SemanticControlId> {
        self.inline_items
            .iter()
            .filter_map(|item| match item {
                Hwp5SemanticInlineItem::Control { control_id } => Some(*control_id),
                Hwp5SemanticInlineItem::Text { .. } => None,
            })
            .collect()
    }
}

/// Ordered inline paragraph content preserved by the semantic IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Hwp5SemanticInlineItem {
    /// Plain text segment in paragraph order.
    Text {
        /// Text segment content.
        text: String,
    },
    /// Inline control reference in paragraph order.
    Control {
        /// Semantic control identifier.
        control_id: Hwp5SemanticControlId,
    },
}

/// Semantic control node reconstructed from `CtrlHeader` and child records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticControlNode {
    /// Stable node identifier within one semantic document.
    pub node_id: Hwp5SemanticControlId,
    /// Canonical semantic control kind.
    pub kind: Hwp5SemanticControlKind,
    /// Minimal structured payload preserved for known controls.
    pub payload: Hwp5SemanticControlPayload,
    /// Semantic container path.
    pub container: Hwp5SemanticContainerPath,
    /// Original four-byte control literal when known.
    pub literal_ctrl_id: Option<String>,
    /// Semantic-local paragraph identifier that anchors this control when known.
    pub anchor_paragraph_id: Option<Hwp5SemanticParagraphId>,
    /// Confidence assigned to this interpretation.
    pub confidence: Hwp5SemanticConfidence,
    /// Free-form notes for non-lossless reconstruction details.
    pub notes: Vec<String>,
}

/// Minimal structured payload for a semantic control node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Hwp5SemanticControlPayload {
    /// No structured payload has been attached yet.
    None,
    /// Minimal image payload for paragraph-local picture controls.
    Image(Hwp5SemanticImagePayload),
    /// Minimal line payload preserved for visible non-image GSO controls.
    Line(Hwp5SemanticLinePayload),
    /// Minimal polygon payload preserved for visible non-image GSO controls.
    Polygon(Hwp5SemanticPolygonPayload),
    /// Minimal pure-rectangle evidence preserved without projection support.
    Rect(Hwp5SemanticRectPayload),
    /// Minimal OLE-backed object payload preserved without chart semantics.
    OleObject(Hwp5SemanticOlePayload),
    /// Table-level summary proven by the current parser.
    Table(Hwp5SemanticTablePayload),
}

/// Minimal image asset payload preserved on a semantic image control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hwp5SemanticImagePayload {
    /// 1-based HWP5 binary item identifier.
    pub binary_data_id: u16,
    /// Storage key inside the HWP5 package and Core `ImageStore`.
    pub storage_name: String,
    /// Document-relative path used by Core image runs.
    pub package_path: String,
    /// Image format inferred from the HWP5 `BinData` extension.
    pub format: Hwp5SemanticImageFormat,
    /// Display width in HWPUNIT when known.
    pub width_hwp: Option<i32>,
    /// Display height in HWPUNIT when known.
    pub height_hwp: Option<i32>,
}

/// Minimal point payload preserved for non-image GSO controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticShapePoint {
    /// Horizontal coordinate in raw HWPUNIT.
    pub x: i32,
    /// Vertical coordinate in raw HWPUNIT.
    pub y: i32,
}

/// Minimal line payload preserved on a semantic line control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticLinePayload {
    /// Horizontal offset from the paragraph anchor in HWPUNIT.
    pub x_hwp: i32,
    /// Vertical offset from the paragraph anchor in HWPUNIT.
    pub y_hwp: i32,
    /// Bounding-box width in HWPUNIT.
    pub width_hwp: u32,
    /// Bounding-box height in HWPUNIT.
    pub height_hwp: u32,
    /// Start point in local shape coordinates.
    pub start: Hwp5SemanticShapePoint,
    /// End point in local shape coordinates.
    pub end: Hwp5SemanticShapePoint,
}

/// Minimal polygon payload preserved on a semantic polygon control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticPolygonPayload {
    /// Horizontal offset from the paragraph anchor in HWPUNIT.
    pub x_hwp: i32,
    /// Vertical offset from the paragraph anchor in HWPUNIT.
    pub y_hwp: i32,
    /// Bounding-box width in HWPUNIT.
    pub width_hwp: u32,
    /// Bounding-box height in HWPUNIT.
    pub height_hwp: u32,
    /// Ordered polygon vertices in local shape coordinates.
    pub points: Vec<Hwp5SemanticShapePoint>,
}

/// Minimal pure-rectangle evidence preserved on a semantic rect control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticRectPayload {
    /// Horizontal offset from the paragraph anchor in HWPUNIT.
    pub x_hwp: i32,
    /// Vertical offset from the paragraph anchor in HWPUNIT.
    pub y_hwp: i32,
    /// Bounding-box width in HWPUNIT.
    pub width_hwp: u32,
    /// Bounding-box height in HWPUNIT.
    pub height_hwp: u32,
}

/// Supported semantic image format hints inferred from HWP5 metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Hwp5SemanticImageFormat {
    /// PNG bitmap.
    Png,
    /// JPEG bitmap.
    Jpeg,
    /// GIF bitmap.
    Gif,
    /// BMP bitmap.
    Bmp,
    /// WMF vector image.
    Wmf,
    /// EMF vector image.
    Emf,
    /// Unrecognized extension preserved verbatim.
    Unknown(String),
}

/// Minimal embedded-object payload preserved on a semantic OLE control.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticOlePayload {
    /// 1-based HWP5 binary item identifier.
    pub binary_data_id: u16,
    /// Storage key inside the HWP5 package.
    pub storage_name: String,
    /// Document-relative package path.
    pub package_path: String,
    /// Embedded object extent width in HWPUNIT when known.
    pub extent_width_hwp: Option<i32>,
    /// Embedded object extent height in HWPUNIT when known.
    pub extent_height_hwp: Option<i32>,
}

/// Minimal table summary preserved on a semantic control node.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticTablePayload {
    /// Declared number of rows.
    pub rows: u16,
    /// Declared number of columns.
    pub cols: u16,
    /// Parsed cell count currently available from the parser.
    pub cell_count: usize,
    /// Page break policy recovered from the HWP5 table body record.
    pub page_break: Hwp5SemanticTablePageBreak,
    /// Whether the table repeats its header row.
    pub repeat_header: bool,
    /// Number of leading header rows observed on table cells.
    pub header_row_count: u16,
    /// Cell spacing recovered from the HWP5 table body record in HWPUNIT16.
    pub cell_spacing_hwp: i16,
    /// Optional table-level border/fill reference.
    pub border_fill_id: Option<u16>,
    /// Distinct cell-level border/fill references observed in parsed cells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub distinct_cell_border_fill_ids: Vec<u16>,
    /// Distinct positive cell heights observed in parsed cells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub distinct_cell_heights_hwp: Vec<i32>,
    /// Distinct positive cell widths observed in parsed cells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub distinct_cell_widths_hwp: Vec<i32>,
    /// Structural table width inferred from the first logical row when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structural_width_hwp: Option<i32>,
    /// Row-local max positive cell heights, preserved as structural evidence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_max_cell_heights_hwp: Vec<i32>,
    /// Cell-scoped evidence recovered from parsed table cells.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cells: Vec<Hwp5SemanticTableCellEvidence>,
}

/// Semantic page-break policy preserved for an HWP5 table body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hwp5SemanticTablePageBreak {
    /// Do not split the table across pages.
    None,
    /// Split at cell boundaries.
    Cell,
    /// Split at table boundaries.
    Table,
    /// Unknown raw value preserved for audit/debugging.
    Unknown(u8),
}

/// Semantic vertical alignment preserved for an HWP5 table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Hwp5SemanticTableCellVerticalAlign {
    /// Align cell content to the top edge.
    Top,
    /// Center cell content vertically.
    Center,
    /// Align cell content to the bottom edge.
    Bottom,
    /// Unknown raw value preserved for audit/debugging.
    Unknown(u8),
}

impl Hwp5SemanticTableCellVerticalAlign {
    /// Returns a stable audit key for notes and snapshot output.
    #[must_use]
    pub fn audit_key(&self) -> String {
        match self {
            Self::Top => "top".to_string(),
            Self::Center => "center".to_string(),
            Self::Bottom => "bottom".to_string(),
            Self::Unknown(raw) => format!("unknown-{raw}"),
        }
    }
}

/// Semantic cell margin preserved from an HWP5 table cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Hwp5SemanticTableCellMargin {
    /// Left margin in HWPUNIT16.
    pub left_hwp: i16,
    /// Right margin in HWPUNIT16.
    pub right_hwp: i16,
    /// Top margin in HWPUNIT16.
    pub top_hwp: i16,
    /// Bottom margin in HWPUNIT16.
    pub bottom_hwp: i16,
}

/// Semantic cell-scoped evidence preserved for table audit and parity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Hwp5SemanticTableCellEvidence {
    /// Zero-based column index.
    pub column: u16,
    /// Zero-based row index.
    pub row: u16,
    /// Horizontal span.
    pub col_span: u16,
    /// Vertical span.
    pub row_span: u16,
    /// Whether this cell is marked as belonging to a title/header row.
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_header: bool,
    /// Optional border/fill reference.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub border_fill_id: Option<u16>,
    /// Optional positive cell height in HWPUNIT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height_hwp: Option<i32>,
    /// Optional positive cell width in HWPUNIT.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width_hwp: Option<i32>,
    /// Cell-local inner margin in HWPUNIT16.
    pub margin_hwp: Hwp5SemanticTableCellMargin,
    /// Cell content vertical alignment.
    pub vertical_align: Hwp5SemanticTableCellVerticalAlign,
}

impl Hwp5SemanticTablePageBreak {
    /// Returns a stable audit key for notes and snapshot output.
    #[must_use]
    pub fn audit_key(&self) -> String {
        match self {
            Self::None => "none".to_string(),
            Self::Cell => "cell".to_string(),
            Self::Table => "table".to_string(),
            Self::Unknown(raw) => format!("unknown-{raw}"),
        }
    }
}

/// Edge in the semantic control graph.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticControlEdge {
    /// Source control node identifier.
    pub from_node_id: Hwp5SemanticControlId,
    /// Target control node identifier.
    pub to_node_id: Hwp5SemanticControlId,
    /// Meaning of the relationship.
    pub kind: Hwp5SemanticControlEdgeKind,
}

/// Relationship kind between semantic control nodes.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Hwp5SemanticControlEdgeKind {
    /// Parent-child nesting relationship.
    Contains,
    /// Anchor relationship between a wrapper and anchored object.
    Anchors,
    /// Fallback relationship, for example chart XML and OLE fallback.
    FallbacksTo,
    /// Relationship not yet canonicalized.
    Unknown(String),
}

/// Semantic container path preserved for nested control reconstruction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5SemanticContainerPath {
    /// Ordered semantic container segments from outermost to innermost.
    pub segments: Vec<Hwp5SemanticContainerKind>,
}

impl Hwp5SemanticContainerPath {
    /// Creates a new container path from explicit segments.
    ///
    /// Empty input is normalized to `[Body]` so the path always has a stable
    /// terminal semantic meaning.
    #[must_use]
    pub fn new(segments: Vec<Hwp5SemanticContainerKind>) -> Self {
        if segments.is_empty() {
            return Self { segments: vec![Hwp5SemanticContainerKind::Body] };
        }
        Self { segments }
    }

    /// Returns the innermost semantic container kind.
    #[must_use]
    pub fn terminal_kind(&self) -> &Hwp5SemanticContainerKind {
        self.segments.last().unwrap_or(&Hwp5SemanticContainerKind::Body)
    }

    /// Returns a stable slash-separated key for audit output.
    #[must_use]
    pub fn audit_key(&self) -> String {
        self.segments.iter().map(Hwp5SemanticContainerKind::audit_key).collect::<Vec<_>>().join("/")
    }
}

/// Semantic container segment kinds currently required by fixture-driven work.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Hwp5SemanticContainerKind {
    /// Main section body.
    Body,
    /// Header subtree rooted at `header/subList`.
    HeaderSubList,
    /// Footer subtree rooted at `footer/subList`.
    FooterSubList,
    /// Table-cell subtree rooted at `tc/subList`.
    TableCellSubList,
    /// Textbox subtree rooted at `rect/drawText/subList`.
    TextBoxSubList,
    /// Footnote subtree.
    FootnoteSubList,
    /// Endnote subtree.
    EndnoteSubList,
    /// Semantic container not yet canonicalized.
    Unknown(String),
}

impl Hwp5SemanticContainerKind {
    /// Returns a stable lowercase key for audit output.
    #[must_use]
    pub fn audit_key(&self) -> String {
        match self {
            Self::Body => "body".to_string(),
            Self::HeaderSubList => "header/subList".to_string(),
            Self::FooterSubList => "footer/subList".to_string(),
            Self::TableCellSubList => "table/tc/subList".to_string(),
            Self::TextBoxSubList => "rect/drawText/subList".to_string(),
            Self::FootnoteSubList => "footnote/subList".to_string(),
            Self::EndnoteSubList => "endnote/subList".to_string(),
            Self::Unknown(value) => format!("unknown/{value}"),
        }
    }
}

/// Canonical semantic control kinds currently required by the parser plan.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Hwp5SemanticControlKind {
    /// Body text paragraph container.
    Paragraph,
    /// Table control.
    Table,
    /// Image or picture object.
    Image,
    /// Line shape object.
    Line,
    /// Pure rectangle shape object.
    Rect,
    /// Polygon shape object.
    Polygon,
    /// Header control.
    Header,
    /// Footer control.
    Footer,
    /// Textbox or draw-text shape.
    TextBox,
    /// Page number or auto-numbering control.
    PageNumber,
    /// Chart object.
    Chart,
    /// OLE-backed embedded object.
    OleObject,
    /// Unsupported or unknown control.
    Unknown(String),
}

/// Confidence assigned to a semantic interpretation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Hwp5SemanticConfidence {
    /// Proven directly by fixture evidence and stable parser behavior.
    High,
    /// Supported by partial evidence but still needs more fixture coverage.
    Medium,
    /// Kept only as a placeholder or unresolved interpretation.
    Low,
}

/// Explicit unresolved or unsupported semantic fact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5UnresolvedItem {
    /// Stable identifier within one semantic document.
    pub item_id: Hwp5SemanticUnresolvedId,
    /// Semantic-local section identifier when the item belongs to a section.
    pub section_id: Option<Hwp5SemanticSectionId>,
    /// Semantic container path for the unresolved item when known.
    pub container: Option<Hwp5SemanticContainerPath>,
    /// Why the item remained unresolved.
    pub reason: Hwp5UnresolvedReason,
    /// Free-form human-readable detail.
    pub detail: String,
    /// Confidence that the unresolved classification itself is correct.
    pub confidence: Hwp5SemanticConfidence,
    /// Raw tag identifier when present.
    pub raw_tag_id: Option<u16>,
    /// Original four-byte control literal when present.
    pub raw_ctrl_id: Option<String>,
}

/// Reason category for an unresolved semantic item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub enum Hwp5UnresolvedReason {
    /// The tag was unknown to the current registry.
    UnknownTag,
    /// The control literal was unknown to the current registry.
    UnknownControlId,
    /// The payload was known but intentionally left opaque.
    OpaquePayload,
    /// Projection to Core/HWPX is not supported yet.
    UnsupportedProjection,
    /// Version gate is known to exist but not resolved yet.
    MissingVersionGate,
    /// A placeholder for evidence that is still too weak.
    LowConfidenceInterpretation,
}

/// Parser-only audit snapshot derived from the semantic IR.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditSnapshot {
    /// HWP5 version being audited.
    pub version: String,
    /// Number of semantic sections.
    pub section_count: usize,
    /// Number of semantic paragraphs.
    pub paragraph_count: usize,
    /// Number of semantic controls.
    pub control_count: usize,
    /// Number of unresolved items.
    pub unresolved_count: usize,
    /// Legacy aggregate counts by terminal container kind.
    ///
    /// This mixed view combines paragraphs and controls only. Newer callers
    /// should prefer the split container counts below so structural
    /// observability is not lost.
    pub container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate paragraph counts by terminal container kind.
    pub paragraph_container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate control counts by terminal container kind.
    pub control_container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate unresolved counts by terminal container kind when known.
    pub unresolved_container_counts: Vec<Hwp5ParserAuditOptionalContainerCount>,
    /// Aggregate counts by control kind.
    pub control_counts: Vec<Hwp5ParserAuditControlCount>,
    /// Aggregate paragraph counts by `(terminal container kind, owner control kind)`.
    pub container_owner_counts: Vec<Hwp5ParserAuditContainerOwnerCount>,
    /// Aggregate counts by `(terminal container kind, control kind)`.
    pub container_control_counts: Vec<Hwp5ParserAuditContainerControlCount>,
    /// Section-level audit summaries.
    pub sections: Vec<Hwp5ParserAuditSection>,
}

/// Section-level parser-only audit summary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditSection {
    /// Zero-based section index.
    pub index: usize,
    /// Number of semantic paragraphs in the section.
    pub paragraph_count: usize,
    /// Number of semantic controls in the section.
    pub control_count: usize,
    /// Number of unresolved items assigned to the section.
    pub unresolved_count: usize,
    /// Legacy aggregate counts by terminal container kind for the section.
    ///
    /// This mixed view combines paragraphs and controls only. Newer callers
    /// should prefer the split container counts below so structural
    /// observability is not lost.
    pub container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate paragraph counts by terminal container kind for the section.
    pub paragraph_container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate control counts by terminal container kind for the section.
    pub control_container_counts: Vec<Hwp5ParserAuditContainerCount>,
    /// Aggregate unresolved counts by terminal container kind for the section when known.
    pub unresolved_container_counts: Vec<Hwp5ParserAuditOptionalContainerCount>,
    /// Aggregate counts by control kind for the section.
    pub control_counts: Vec<Hwp5ParserAuditControlCount>,
    /// Aggregate paragraph counts by `(terminal container kind, owner control kind)` for the section.
    pub container_owner_counts: Vec<Hwp5ParserAuditContainerOwnerCount>,
    /// Aggregate counts by `(terminal container kind, control kind)` for the section.
    pub container_control_counts: Vec<Hwp5ParserAuditContainerControlCount>,
}

/// Aggregate count keyed by semantic container kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditContainerCount {
    /// Container kind being counted.
    pub kind: Hwp5SemanticContainerKind,
    /// Number of semantic items anchored to this terminal container.
    pub count: usize,
}

/// Aggregate count keyed by an optional semantic container kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditOptionalContainerCount {
    /// Container kind being counted, or `None` when the unresolved item was not
    /// anchored to any known semantic container.
    pub kind: Option<Hwp5SemanticContainerKind>,
    /// Number of semantic items anchored to this terminal container.
    pub count: usize,
}

/// Aggregate count keyed by semantic control kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditControlCount {
    /// Control kind being counted.
    pub kind: Hwp5SemanticControlKind,
    /// Number of semantic controls of this kind.
    pub count: usize,
}

/// Aggregate paragraph-owner count keyed by `(terminal container kind, owner control kind)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditContainerOwnerCount {
    /// Terminal container kind for the semantic paragraphs.
    pub container: Hwp5SemanticContainerKind,
    /// Owner control kind for those paragraphs.
    pub owner_kind: Hwp5SemanticControlKind,
    /// Number of semantic paragraphs owned by the control kind inside the container.
    pub count: usize,
}

/// Aggregate count keyed by `(terminal container kind, semantic control kind)`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Hwp5ParserAuditContainerControlCount {
    /// Terminal container kind for the semantic controls.
    pub container: Hwp5SemanticContainerKind,
    /// Control kind being counted inside the container.
    pub kind: Hwp5SemanticControlKind,
    /// Number of semantic controls of this kind inside the container.
    pub count: usize,
}

fn to_container_counts(
    counts: BTreeMap<Hwp5SemanticContainerKind, usize>,
) -> Vec<Hwp5ParserAuditContainerCount> {
    counts.into_iter().map(|(kind, count)| Hwp5ParserAuditContainerCount { kind, count }).collect()
}

fn to_optional_container_counts(
    counts: BTreeMap<Option<Hwp5SemanticContainerKind>, usize>,
) -> Vec<Hwp5ParserAuditOptionalContainerCount> {
    counts
        .into_iter()
        .map(|(kind, count)| Hwp5ParserAuditOptionalContainerCount { kind, count })
        .collect()
}

fn to_control_counts(
    counts: BTreeMap<Hwp5SemanticControlKind, usize>,
) -> Vec<Hwp5ParserAuditControlCount> {
    counts.into_iter().map(|(kind, count)| Hwp5ParserAuditControlCount { kind, count }).collect()
}

fn to_container_owner_counts(
    counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
) -> Vec<Hwp5ParserAuditContainerOwnerCount> {
    counts
        .into_iter()
        .map(|((container, owner_kind), count)| Hwp5ParserAuditContainerOwnerCount {
            container,
            owner_kind,
            count,
        })
        .collect()
}

fn to_container_control_counts(
    counts: BTreeMap<(Hwp5SemanticContainerKind, Hwp5SemanticControlKind), usize>,
) -> Vec<Hwp5ParserAuditContainerControlCount> {
    counts
        .into_iter()
        .map(|((container, kind), count)| Hwp5ParserAuditContainerControlCount {
            container,
            kind,
            count,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    fn inline_text(text: &str) -> Hwp5SemanticInlineItem {
        Hwp5SemanticInlineItem::Text { text: text.to_string() }
    }

    fn inline_control(control_id: Hwp5SemanticControlId) -> Hwp5SemanticInlineItem {
        Hwp5SemanticInlineItem::Control { control_id }
    }

    fn sample_page_def(landscape: bool) -> Hwp5SemanticPageDefSummary {
        Hwp5SemanticPageDefSummary {
            width: 59_528,
            height: 84_188,
            margin_left: 8_504,
            margin_right: 8_504,
            margin_top: 5_668,
            margin_bottom: 4_252,
            header_margin: 4_252,
            footer_margin: 4_252,
            gutter: 0,
            landscape,
        }
    }

    #[test]
    fn empty_container_path_defaults_to_body() {
        let path = Hwp5SemanticContainerPath::new(Vec::new());

        assert_eq!(path.terminal_kind(), &Hwp5SemanticContainerKind::Body);
        assert_eq!(path.audit_key(), "body");
    }

    #[test]
    fn parser_audit_snapshot_counts_nested_controls_by_container() {
        let package_meta = Hwp5SemanticPackageMeta {
            version: "5.1.1.0".to_string(),
            compressed: true,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
        };

        let mut document = Hwp5SemanticDocument::new(package_meta);
        document.sections.push(Hwp5SemanticSection {
            section_id: Hwp5SemanticSectionId::new(0),
            index: 0,
            page_def: None,
            paragraphs: vec![
                Hwp5SemanticParagraph {
                    paragraph_id: Hwp5SemanticParagraphId::new(0),
                    paragraph_index: 0,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::Body,
                    ]),
                    owner_control_id: None,
                    inline_items: vec![
                        inline_text("본문"),
                        inline_control(Hwp5SemanticControlId::new(0)),
                    ],
                    text: "본문".to_string(),
                    style_id: Some(1),
                    char_shape_run_count: 1,
                    control_ids: vec![Hwp5SemanticControlId::new(0)],
                },
                Hwp5SemanticParagraph {
                    paragraph_id: Hwp5SemanticParagraphId::new(1),
                    paragraph_index: 1,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    owner_control_id: Some(Hwp5SemanticControlId::new(0)),
                    inline_items: vec![
                        inline_text("머리말"),
                        inline_control(Hwp5SemanticControlId::new(1)),
                    ],
                    text: "머리말".to_string(),
                    style_id: None,
                    char_shape_run_count: 1,
                    control_ids: vec![Hwp5SemanticControlId::new(1)],
                },
            ],
            controls: vec![
                Hwp5SemanticControlNode {
                    node_id: Hwp5SemanticControlId::new(0),
                    kind: Hwp5SemanticControlKind::Header,
                    payload: Hwp5SemanticControlPayload::None,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    literal_ctrl_id: Some("head".to_string()),
                    anchor_paragraph_id: Some(Hwp5SemanticParagraphId::new(0)),
                    confidence: Hwp5SemanticConfidence::High,
                    notes: Vec::new(),
                },
                Hwp5SemanticControlNode {
                    node_id: Hwp5SemanticControlId::new(1),
                    kind: Hwp5SemanticControlKind::Image,
                    payload: Hwp5SemanticControlPayload::None,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    literal_ctrl_id: Some("gso ".to_string()),
                    anchor_paragraph_id: Some(Hwp5SemanticParagraphId::new(1)),
                    confidence: Hwp5SemanticConfidence::Medium,
                    notes: Vec::new(),
                },
            ],
        });
        document.unresolved.push(Hwp5UnresolvedItem {
            item_id: Hwp5SemanticUnresolvedId::new(0),
            section_id: Some(Hwp5SemanticSectionId::new(0)),
            container: Some(Hwp5SemanticContainerPath::new(vec![
                Hwp5SemanticContainerKind::TextBoxSubList,
            ])),
            reason: Hwp5UnresolvedReason::OpaquePayload,
            detail: "textbox payload".to_string(),
            confidence: Hwp5SemanticConfidence::Low,
            raw_tag_id: Some(0x54),
            raw_ctrl_id: Some("gso ".to_string()),
        });

        let snapshot = document.parser_audit_snapshot();

        assert_eq!(snapshot.section_count, 1);
        assert_eq!(snapshot.paragraph_count, 2);
        assert_eq!(snapshot.control_count, 2);
        assert_eq!(snapshot.unresolved_count, 1);
        assert_eq!(
            snapshot.container_counts,
            vec![
                Hwp5ParserAuditContainerCount { kind: Hwp5SemanticContainerKind::Body, count: 1 },
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::HeaderSubList,
                    count: 3,
                },
            ]
        );
        assert_eq!(
            snapshot.paragraph_container_counts,
            vec![
                Hwp5ParserAuditContainerCount { kind: Hwp5SemanticContainerKind::Body, count: 1 },
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::HeaderSubList,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            snapshot.control_container_counts,
            vec![Hwp5ParserAuditContainerCount {
                kind: Hwp5SemanticContainerKind::HeaderSubList,
                count: 2,
            }]
        );
        assert_eq!(
            snapshot.unresolved_container_counts,
            vec![Hwp5ParserAuditOptionalContainerCount {
                kind: Some(Hwp5SemanticContainerKind::TextBoxSubList),
                count: 1,
            }]
        );
        assert_eq!(
            snapshot.control_counts,
            vec![
                Hwp5ParserAuditControlCount { kind: Hwp5SemanticControlKind::Image, count: 1 },
                Hwp5ParserAuditControlCount { kind: Hwp5SemanticControlKind::Header, count: 1 },
            ]
        );
        assert_eq!(
            snapshot.container_control_counts,
            vec![
                Hwp5ParserAuditContainerControlCount {
                    container: Hwp5SemanticContainerKind::HeaderSubList,
                    kind: Hwp5SemanticControlKind::Image,
                    count: 1,
                },
                Hwp5ParserAuditContainerControlCount {
                    container: Hwp5SemanticContainerKind::HeaderSubList,
                    kind: Hwp5SemanticControlKind::Header,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            snapshot.sections[0].paragraph_container_counts,
            snapshot.paragraph_container_counts,
        );
        assert_eq!(
            snapshot.sections[0].control_container_counts,
            snapshot.control_container_counts,
        );
    }

    #[test]
    fn parser_audit_snapshot_distinguishes_header_text_and_nested_textbox_content() {
        let package_meta = Hwp5SemanticPackageMeta {
            version: "5.1.1.0".to_string(),
            compressed: true,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
        };

        let mut document = Hwp5SemanticDocument::new(package_meta);
        document.sections.push(Hwp5SemanticSection {
            section_id: Hwp5SemanticSectionId::new(0),
            index: 0,
            page_def: None,
            paragraphs: vec![
                Hwp5SemanticParagraph {
                    paragraph_id: Hwp5SemanticParagraphId::new(0),
                    paragraph_index: 0,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    owner_control_id: Some(Hwp5SemanticControlId::new(0)),
                    inline_items: vec![
                        inline_text("머리말 텍스트"),
                        inline_control(Hwp5SemanticControlId::new(0)),
                    ],
                    text: "머리말 텍스트".to_string(),
                    style_id: None,
                    char_shape_run_count: 1,
                    control_ids: vec![Hwp5SemanticControlId::new(0)],
                },
                Hwp5SemanticParagraph {
                    paragraph_id: Hwp5SemanticParagraphId::new(1),
                    paragraph_index: 1,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::TextBoxSubList,
                    ]),
                    owner_control_id: Some(Hwp5SemanticControlId::new(1)),
                    inline_items: vec![
                        inline_text("글상자 내부 텍스트"),
                        inline_control(Hwp5SemanticControlId::new(2)),
                    ],
                    text: "글상자 내부 텍스트".to_string(),
                    style_id: None,
                    char_shape_run_count: 2,
                    control_ids: vec![Hwp5SemanticControlId::new(2)],
                },
            ],
            controls: vec![
                Hwp5SemanticControlNode {
                    node_id: Hwp5SemanticControlId::new(0),
                    kind: Hwp5SemanticControlKind::Image,
                    payload: Hwp5SemanticControlPayload::None,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    literal_ctrl_id: Some("gso ".to_string()),
                    anchor_paragraph_id: Some(Hwp5SemanticParagraphId::new(0)),
                    confidence: Hwp5SemanticConfidence::High,
                    notes: Vec::new(),
                },
                Hwp5SemanticControlNode {
                    node_id: Hwp5SemanticControlId::new(1),
                    kind: Hwp5SemanticControlKind::TextBox,
                    payload: Hwp5SemanticControlPayload::None,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::Body,
                    ]),
                    literal_ctrl_id: Some("gso ".to_string()),
                    anchor_paragraph_id: None,
                    confidence: Hwp5SemanticConfidence::High,
                    notes: Vec::new(),
                },
                Hwp5SemanticControlNode {
                    node_id: Hwp5SemanticControlId::new(2),
                    kind: Hwp5SemanticControlKind::Image,
                    payload: Hwp5SemanticControlPayload::None,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::TextBoxSubList,
                    ]),
                    literal_ctrl_id: Some("gso ".to_string()),
                    anchor_paragraph_id: Some(Hwp5SemanticParagraphId::new(1)),
                    confidence: Hwp5SemanticConfidence::Medium,
                    notes: Vec::new(),
                },
            ],
        });
        document.unresolved.push(Hwp5UnresolvedItem {
            item_id: Hwp5SemanticUnresolvedId::new(0),
            section_id: Some(Hwp5SemanticSectionId::new(0)),
            container: Some(Hwp5SemanticContainerPath::new(vec![
                Hwp5SemanticContainerKind::TextBoxSubList,
            ])),
            reason: Hwp5UnresolvedReason::OpaquePayload,
            detail: "textbox shape payload".to_string(),
            confidence: Hwp5SemanticConfidence::Low,
            raw_tag_id: Some(0x54),
            raw_ctrl_id: Some("gso ".to_string()),
        });
        document.unresolved.push(Hwp5UnresolvedItem {
            item_id: Hwp5SemanticUnresolvedId::new(1),
            section_id: None,
            container: None,
            reason: Hwp5UnresolvedReason::MissingVersionGate,
            detail: "global ambiguity".to_string(),
            confidence: Hwp5SemanticConfidence::Low,
            raw_tag_id: None,
            raw_ctrl_id: None,
        });

        let snapshot = document.parser_audit_snapshot();

        assert_eq!(
            snapshot.paragraph_container_counts,
            vec![
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::HeaderSubList,
                    count: 1,
                },
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::TextBoxSubList,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            snapshot.control_container_counts,
            vec![
                Hwp5ParserAuditContainerCount { kind: Hwp5SemanticContainerKind::Body, count: 1 },
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::HeaderSubList,
                    count: 1,
                },
                Hwp5ParserAuditContainerCount {
                    kind: Hwp5SemanticContainerKind::TextBoxSubList,
                    count: 1,
                },
            ]
        );
        assert_eq!(
            snapshot.unresolved_container_counts,
            vec![
                Hwp5ParserAuditOptionalContainerCount { kind: None, count: 1 },
                Hwp5ParserAuditOptionalContainerCount {
                    kind: Some(Hwp5SemanticContainerKind::TextBoxSubList),
                    count: 1,
                },
            ]
        );
        assert_eq!(
            snapshot.container_control_counts,
            vec![
                Hwp5ParserAuditContainerControlCount {
                    container: Hwp5SemanticContainerKind::Body,
                    kind: Hwp5SemanticControlKind::TextBox,
                    count: 1,
                },
                Hwp5ParserAuditContainerControlCount {
                    container: Hwp5SemanticContainerKind::HeaderSubList,
                    kind: Hwp5SemanticControlKind::Image,
                    count: 1,
                },
                Hwp5ParserAuditContainerControlCount {
                    container: Hwp5SemanticContainerKind::TextBoxSubList,
                    kind: Hwp5SemanticControlKind::Image,
                    count: 1,
                },
            ]
        );
        assert_eq!(snapshot.sections[0].unresolved_count, 1);
        assert_eq!(
            snapshot.sections[0].unresolved_container_counts,
            vec![Hwp5ParserAuditOptionalContainerCount {
                kind: Some(Hwp5SemanticContainerKind::TextBoxSubList),
                count: 1,
            }]
        );
    }

    #[test]
    fn paragraph_inline_items_preserve_mixed_content_order() {
        let control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(9);
        let paragraph = Hwp5SemanticParagraph {
            paragraph_id: Hwp5SemanticParagraphId::new(3),
            paragraph_index: 0,
            container: Hwp5SemanticContainerPath::new(vec![
                Hwp5SemanticContainerKind::TextBoxSubList,
            ]),
            owner_control_id: Some(control_id),
            inline_items: vec![
                inline_text("글상자 시작."),
                inline_control(control_id),
                inline_text("글상자 끝."),
            ],
            text: "글상자 시작.글상자 끝.".to_string(),
            style_id: None,
            char_shape_run_count: 2,
            control_ids: vec![control_id],
        };

        assert_eq!(paragraph.inline_text_summary(), "글상자 시작.글상자 끝.");
        assert_eq!(paragraph.inline_control_ids(), vec![control_id]);
    }

    #[test]
    fn graph_integrity_reports_paragraph_summary_mismatches() {
        let paragraph_id: Hwp5SemanticParagraphId = Hwp5SemanticParagraphId::new(3);
        let control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(7);
        let document = Hwp5SemanticDocument {
            package_meta: Hwp5SemanticPackageMeta {
                version: "5.1.1.0".to_string(),
                compressed: true,
                package_entries: Vec::new(),
                bin_data_records: Vec::new(),
                bin_data_streams: Vec::new(),
            },
            doc_info: Hwp5SemanticDocInfo::default(),
            sections: vec![Hwp5SemanticSection {
                section_id: Hwp5SemanticSectionId::new(0),
                index: 0,
                page_def: None,
                paragraphs: vec![Hwp5SemanticParagraph {
                    paragraph_id,
                    paragraph_index: 0,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::Body,
                    ]),
                    owner_control_id: None,
                    inline_items: vec![inline_text("inline"), inline_control(control_id)],
                    text: "summary".to_string(),
                    style_id: None,
                    char_shape_run_count: 1,
                    control_ids: vec![],
                }],
                controls: vec![Hwp5SemanticControlNode {
                    node_id: control_id,
                    kind: Hwp5SemanticControlKind::Table,
                    payload: Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
                        rows: 2,
                        cols: 3,
                        cell_count: 4,
                        page_break: Hwp5SemanticTablePageBreak::Cell,
                        repeat_header: true,
                        header_row_count: 1,
                        cell_spacing_hwp: 120,
                        border_fill_id: Some(8),
                        distinct_cell_border_fill_ids: vec![3, 7],
                        distinct_cell_heights_hwp: vec![282, 1281],
                        distinct_cell_widths_hwp: vec![6236, 12472],
                        structural_width_hwp: Some(18708),
                        row_max_cell_heights_hwp: vec![1281, 282],
                        cells: vec![Hwp5SemanticTableCellEvidence {
                            column: 1,
                            row: 0,
                            col_span: 1,
                            row_span: 1,
                            is_header: true,
                            border_fill_id: Some(7),
                            height_hwp: Some(1281),
                            width_hwp: Some(12472),
                            margin_hwp: Hwp5SemanticTableCellMargin {
                                left_hwp: 4251,
                                right_hwp: 5669,
                                top_hwp: 2834,
                                bottom_hwp: 1417,
                            },
                            vertical_align: Hwp5SemanticTableCellVerticalAlign::Center,
                        }],
                    }),
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::Body,
                    ]),
                    literal_ctrl_id: Some("tbl ".to_string()),
                    anchor_paragraph_id: Some(paragraph_id),
                    confidence: Hwp5SemanticConfidence::High,
                    notes: Vec::new(),
                }],
            }],
            control_graph: Vec::new(),
            unresolved: Vec::new(),
        };

        assert_eq!(
            document.graph_integrity_issues(),
            vec![
                Hwp5SemanticGraphIntegrityIssue::ParagraphTextSummaryMismatch { paragraph_id },
                Hwp5SemanticGraphIntegrityIssue::ParagraphControlInventoryMismatch { paragraph_id },
            ]
        );
    }

    #[test]
    fn semantic_ids_keep_graph_references_coherent() {
        let header_control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(10);
        let image_control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(11);
        let header_paragraph_id: Hwp5SemanticParagraphId = Hwp5SemanticParagraphId::new(20);
        let section_id: Hwp5SemanticSectionId = Hwp5SemanticSectionId::new(30);
        let unresolved_id: Hwp5SemanticUnresolvedId = Hwp5SemanticUnresolvedId::new(40);

        let document = Hwp5SemanticDocument {
            package_meta: Hwp5SemanticPackageMeta {
                version: "5.1.1.0".to_string(),
                compressed: true,
                package_entries: Vec::new(),
                bin_data_records: Vec::new(),
                bin_data_streams: Vec::new(),
            },
            doc_info: Hwp5SemanticDocInfo::default(),
            sections: vec![Hwp5SemanticSection {
                section_id,
                index: 0,
                page_def: Some(sample_page_def(true)),
                paragraphs: vec![Hwp5SemanticParagraph {
                    paragraph_id: header_paragraph_id,
                    paragraph_index: 0,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ]),
                    owner_control_id: Some(header_control_id),
                    inline_items: vec![
                        inline_text("머리말"),
                        inline_control(header_control_id),
                        inline_control(image_control_id),
                    ],
                    text: "머리말".to_string(),
                    style_id: None,
                    char_shape_run_count: 1,
                    control_ids: vec![header_control_id, image_control_id],
                }],
                controls: vec![
                    Hwp5SemanticControlNode {
                        node_id: header_control_id,
                        kind: Hwp5SemanticControlKind::Header,
                        payload: Hwp5SemanticControlPayload::None,
                        container: Hwp5SemanticContainerPath::new(vec![
                            Hwp5SemanticContainerKind::HeaderSubList,
                        ]),
                        literal_ctrl_id: Some("head".to_string()),
                        anchor_paragraph_id: Some(header_paragraph_id),
                        confidence: Hwp5SemanticConfidence::High,
                        notes: Vec::new(),
                    },
                    Hwp5SemanticControlNode {
                        node_id: image_control_id,
                        kind: Hwp5SemanticControlKind::Image,
                        payload: Hwp5SemanticControlPayload::None,
                        container: Hwp5SemanticContainerPath::new(vec![
                            Hwp5SemanticContainerKind::HeaderSubList,
                        ]),
                        literal_ctrl_id: Some("gso ".to_string()),
                        anchor_paragraph_id: Some(header_paragraph_id),
                        confidence: Hwp5SemanticConfidence::Medium,
                        notes: Vec::new(),
                    },
                ],
            }],
            control_graph: vec![Hwp5SemanticControlEdge {
                from_node_id: header_control_id,
                to_node_id: image_control_id,
                kind: Hwp5SemanticControlEdgeKind::Contains,
            }],
            unresolved: vec![Hwp5UnresolvedItem {
                item_id: unresolved_id,
                section_id: Some(section_id),
                container: Some(Hwp5SemanticContainerPath::new(vec![
                    Hwp5SemanticContainerKind::HeaderSubList,
                ])),
                reason: Hwp5UnresolvedReason::LowConfidenceInterpretation,
                detail: "header payload".to_string(),
                confidence: Hwp5SemanticConfidence::Low,
                raw_tag_id: None,
                raw_ctrl_id: Some("head".to_string()),
            }],
        };

        let paragraph_ids: BTreeSet<Hwp5SemanticParagraphId> = document
            .sections
            .iter()
            .flat_map(|section| section.paragraphs.iter().map(|paragraph| paragraph.paragraph_id))
            .collect();
        let control_ids: BTreeSet<Hwp5SemanticControlId> = document
            .sections
            .iter()
            .flat_map(|section| section.controls.iter().map(|control| control.node_id))
            .collect();
        let section_ids: BTreeSet<Hwp5SemanticSectionId> =
            document.sections.iter().map(|section| section.section_id).collect();

        assert!(document.graph_is_coherent());
        assert!(document.graph_integrity_issues().is_empty());
        assert!(document.sections.iter().flat_map(|section| section.paragraphs.iter()).all(
            |paragraph| paragraph
                .control_ids
                .iter()
                .all(|control_id| control_ids.contains(control_id))
        ));
        assert!(document.sections.iter().flat_map(|section| section.controls.iter()).all(
            |control| {
                control
                    .anchor_paragraph_id
                    .is_none_or(|paragraph_id| paragraph_ids.contains(&paragraph_id))
            }
        ));
        assert!(document.control_graph.iter().all(|edge| {
            control_ids.contains(&edge.from_node_id) && control_ids.contains(&edge.to_node_id)
        }));
        assert!(document.unresolved.iter().all(|item| item
            .section_id
            .is_none_or(|item_section_id| section_ids.contains(&item_section_id))));
    }

    #[test]
    fn graph_integrity_reports_duplicate_and_dangling_typed_ids() {
        let duplicate_control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(7);
        let known_paragraph_id: Hwp5SemanticParagraphId = Hwp5SemanticParagraphId::new(3);
        let missing_paragraph_id: Hwp5SemanticParagraphId = Hwp5SemanticParagraphId::new(99);
        let missing_control_id: Hwp5SemanticControlId = Hwp5SemanticControlId::new(88);
        let known_section_id: Hwp5SemanticSectionId = Hwp5SemanticSectionId::new(1);
        let missing_section_id: Hwp5SemanticSectionId = Hwp5SemanticSectionId::new(42);
        let duplicate_unresolved_id: Hwp5SemanticUnresolvedId = Hwp5SemanticUnresolvedId::new(5);

        let document = Hwp5SemanticDocument {
            package_meta: Hwp5SemanticPackageMeta {
                version: "5.1.1.0".to_string(),
                compressed: true,
                package_entries: Vec::new(),
                bin_data_records: Vec::new(),
                bin_data_streams: Vec::new(),
            },
            doc_info: Hwp5SemanticDocInfo::default(),
            sections: vec![Hwp5SemanticSection {
                section_id: known_section_id,
                index: 0,
                page_def: None,
                paragraphs: vec![Hwp5SemanticParagraph {
                    paragraph_id: known_paragraph_id,
                    paragraph_index: 0,
                    container: Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::Body,
                    ]),
                    owner_control_id: Some(duplicate_control_id),
                    inline_items: vec![
                        inline_text("본문"),
                        inline_control(duplicate_control_id),
                        inline_control(missing_control_id),
                    ],
                    text: "본문".to_string(),
                    style_id: None,
                    char_shape_run_count: 1,
                    control_ids: vec![duplicate_control_id, missing_control_id],
                }],
                controls: vec![
                    Hwp5SemanticControlNode {
                        node_id: duplicate_control_id,
                        kind: Hwp5SemanticControlKind::TextBox,
                        payload: Hwp5SemanticControlPayload::None,
                        container: Hwp5SemanticContainerPath::new(vec![
                            Hwp5SemanticContainerKind::Body,
                        ]),
                        literal_ctrl_id: Some("gso ".to_string()),
                        anchor_paragraph_id: Some(known_paragraph_id),
                        confidence: Hwp5SemanticConfidence::High,
                        notes: Vec::new(),
                    },
                    Hwp5SemanticControlNode {
                        node_id: duplicate_control_id,
                        kind: Hwp5SemanticControlKind::Image,
                        payload: Hwp5SemanticControlPayload::None,
                        container: Hwp5SemanticContainerPath::new(vec![
                            Hwp5SemanticContainerKind::TextBoxSubList,
                        ]),
                        literal_ctrl_id: Some("gso ".to_string()),
                        anchor_paragraph_id: Some(missing_paragraph_id),
                        confidence: Hwp5SemanticConfidence::Medium,
                        notes: Vec::new(),
                    },
                ],
            }],
            control_graph: vec![
                Hwp5SemanticControlEdge {
                    from_node_id: duplicate_control_id,
                    to_node_id: missing_control_id,
                    kind: Hwp5SemanticControlEdgeKind::Contains,
                },
                Hwp5SemanticControlEdge {
                    from_node_id: missing_control_id,
                    to_node_id: duplicate_control_id,
                    kind: Hwp5SemanticControlEdgeKind::Anchors,
                },
            ],
            unresolved: vec![
                Hwp5UnresolvedItem {
                    item_id: duplicate_unresolved_id,
                    section_id: Some(known_section_id),
                    container: None,
                    reason: Hwp5UnresolvedReason::OpaquePayload,
                    detail: "known".to_string(),
                    confidence: Hwp5SemanticConfidence::Low,
                    raw_tag_id: None,
                    raw_ctrl_id: None,
                },
                Hwp5UnresolvedItem {
                    item_id: duplicate_unresolved_id,
                    section_id: Some(missing_section_id),
                    container: None,
                    reason: Hwp5UnresolvedReason::MissingVersionGate,
                    detail: "missing".to_string(),
                    confidence: Hwp5SemanticConfidence::Low,
                    raw_tag_id: None,
                    raw_ctrl_id: None,
                },
            ],
        };

        assert!(!document.graph_is_coherent());
        assert_eq!(
            document.graph_integrity_issues(),
            vec![
                Hwp5SemanticGraphIntegrityIssue::DuplicateControlId {
                    control_id: duplicate_control_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DuplicateUnresolvedId {
                    unresolved_id: duplicate_unresolved_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DanglingParagraphControlRef {
                    paragraph_id: known_paragraph_id,
                    control_id: missing_control_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DanglingControlAnchorParagraphRef {
                    control_id: duplicate_control_id,
                    paragraph_id: missing_paragraph_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DanglingControlEdgeFrom {
                    from_node_id: missing_control_id,
                    to_node_id: duplicate_control_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DanglingControlEdgeTo {
                    from_node_id: duplicate_control_id,
                    to_node_id: missing_control_id,
                },
                Hwp5SemanticGraphIntegrityIssue::DanglingUnresolvedSectionRef {
                    unresolved_id: duplicate_unresolved_id,
                    section_id: missing_section_id,
                },
            ]
        );
    }
}
