//! Structural paragraph editing (E4): preserve-first byte-splice insert/delete
//! of top-level `<hp:p>` paragraphs.
//!
//! Scope (E4 = paragraphs only; table-row editing is the separate E4b epic):
//! insert-after / insert-before / delete of **top-level body-flow**
//! paragraphs. Paragraphs nested inside table cells, text boxes, footnotes,
//! or other containers are an explicit non-goal — they are addressed by
//! neither this module nor E5 `read`/`outline` (which are top-level too).
//!
//! Safety is a **closed set of fail-closed rejections** (warning-first): a
//! delete that would strand a reference, or lose a hard page/column break,
//! is refused rather than silently corrupting the document.
//!
//! This module owns the conservative reference scan; precise reference
//! resolution (which id is referenced from where, to allow deleting an
//! unreferenced bookmark) is a documented follow-up.

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::paragraph::Paragraph;
use serde_json::Value;

use crate::decoder::{HwpxDecoder, HwpxDocument};
use crate::patch::{collect_direct_child_outer_spans, find_root_span, section_path, RawPackage};
use crate::stamp::{admission_compare, check_zip_carry, encode_hwpx, StamperError};

/// Addresses a **top-level body-flow** paragraph for structural editing.
///
/// Index-based: paragraph `index` within section `section`, counting only the
/// section's direct-child `<hp:p>` elements. Paragraphs nested inside table
/// cells / text boxes / footnotes are not addressable (E4 non-goal). The
/// index is a positional locator — it goes stale after a structural edit, so
/// a batch is resolved against one pristine snapshot. Name-anchor resolution
/// (bookmark / heading) is layered on at the CLI/MCP surface via E5.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ParagraphLocator {
    /// Zero-based section index.
    pub section: usize,
    /// Zero-based top-level paragraph index within the section.
    pub index: usize,
}

/// Fail-closed reasons a structural edit is refused. No bytes are produced on
/// any error (warning-first: refuse rather than silently corrupt).
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum StructuralEditError {
    /// Decode / encode failure.
    Codec(String),
    /// Input is not round-trip-safe (admission gate): editing it byte-for-byte
    /// cannot be verified.
    NotRoundTripSafe {
        /// Which store diverged (`document` / `style_store` / `image_store`).
        component: String,
        /// First differing path.
        diff_path: String,
    },
    /// The encoder does not carry some ZIP entries (closed-world check).
    UncarriedZipEntries {
        /// Entry paths not preserved.
        entries: Vec<String>,
    },
    /// Section index out of range.
    SectionOutOfRange {
        /// Requested section.
        section: usize,
        /// Number of sections.
        available: usize,
    },
    /// Paragraph index out of range for the section.
    ParagraphOutOfRange {
        /// Section index.
        section: usize,
        /// Requested paragraph.
        index: usize,
        /// Number of top-level paragraphs in the section.
        available: usize,
    },
    /// The same paragraph was targeted twice in one batch.
    DuplicateTarget {
        /// Section index.
        section: usize,
        /// Paragraph index.
        index: usize,
    },
    /// Deleting this paragraph would strand a reference (conservative rule 1):
    /// it carries a bookmark, cross-reference, or an identifiable object
    /// (footnote / endnote / equation / group / text-art / image / table)
    /// that something elsewhere may point at.
    ReferenceStranded {
        /// Section index.
        section: usize,
        /// Paragraph index.
        index: usize,
    },
    /// Deleting this paragraph would silently lose a hard page or column
    /// break (formatting intent, distinct from reference integrity).
    HardBreakLoss {
        /// Section index.
        section: usize,
        /// Paragraph index.
        index: usize,
    },
    /// The edit would leave a section with zero paragraphs.
    EmptySection {
        /// Section index.
        section: usize,
    },
    /// The target paragraph carries the section properties (`<hp:secPr>` — page
    /// setup, start numbers). In HWPX these live inside the section's first
    /// paragraph; deleting it would silently reset the section layout. Deleting
    /// such a paragraph is a Wave 1 non-goal (moving `secPr` to the next
    /// paragraph is a follow-up).
    SectionPropertiesParagraph {
        /// Section index.
        section: usize,
        /// Paragraph index (the `secPr` carrier).
        index: usize,
    },
    /// The wire `<hp:p>` count does not match the decoded paragraph count, so
    /// index→byte-span mapping is unsafe.
    SpanCountMismatch {
        /// Section index.
        section: usize,
        /// Decoded paragraph count.
        decoded: usize,
        /// Wire `<hp:p>` element count.
        wire: usize,
    },
    /// Post-edit self-verification failed: the re-decoded output does not equal
    /// the declared delta.
    DeltaMismatch {
        /// Diagnostic detail.
        detail: String,
    },
}

impl std::fmt::Display for StructuralEditError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Codec(m) => write!(f, "codec: {m}"),
            Self::NotRoundTripSafe { component, diff_path } => {
                write!(f, "input not round-trip-safe ({component} at {diff_path})")
            }
            Self::UncarriedZipEntries { entries } => {
                write!(f, "uncarried ZIP entries: {}", entries.join(", "))
            }
            Self::SectionOutOfRange { section, available } => {
                write!(f, "section {section} out of range ({available} sections)")
            }
            Self::ParagraphOutOfRange { section, index, available } => write!(
                f,
                "paragraph {index} out of range in section {section} ({available} paragraphs)"
            ),
            Self::DuplicateTarget { section, index } => {
                write!(f, "paragraph {section}:{index} targeted twice")
            }
            Self::ReferenceStranded { section, index } => write!(
                f,
                "deleting paragraph {section}:{index} would strand a reference (bookmark/cross-ref/object)"
            ),
            Self::HardBreakLoss { section, index } => write!(
                f,
                "deleting paragraph {section}:{index} would lose a hard page/column break"
            ),
            Self::EmptySection { section } => {
                write!(f, "edit would leave section {section} empty")
            }
            Self::SectionPropertiesParagraph { section, index } => write!(
                f,
                "paragraph {section}:{index} carries the section properties (secPr) and cannot be deleted"
            ),
            Self::SpanCountMismatch { section, decoded, wire } => write!(
                f,
                "section {section}: decoded {decoded} paragraphs but {wire} <hp:p> elements"
            ),
            Self::DeltaMismatch { detail } => write!(f, "self-verify failed: {detail}"),
        }
    }
}

impl std::error::Error for StructuralEditError {}

fn map_admission(error: StamperError) -> StructuralEditError {
    match error {
        StamperError::NotRoundTripSafe { component, diff_path } => {
            StructuralEditError::NotRoundTripSafe { component, diff_path }
        }
        StamperError::UncarriedZipEntries { entries } => {
            StructuralEditError::UncarriedZipEntries { entries }
        }
        other => StructuralEditError::Codec(other.to_string()),
    }
}

/// Byte-splice structural editor (preserve-first, admission-gated).
#[derive(Debug)]
pub struct HwpxStructuralEditor;

impl HwpxStructuralEditor {
    /// Deletes top-level paragraphs, all-or-nothing, preserving every other
    /// byte of the package exactly.
    ///
    /// Pipeline: input admission gate → per-target rejection scan
    /// (reference / hard-break) → declared-delta build + `validate()` →
    /// byte-splice the affected section XML → re-decode and reverse-delta
    /// self-verify. Any failure produces no bytes.
    ///
    /// # Errors
    ///
    /// See [`StructuralEditError`].
    pub fn delete_paragraphs(
        base: &[u8],
        targets: &[ParagraphLocator],
    ) -> Result<Vec<u8>, StructuralEditError> {
        // ── input admission: base must round-trip so we can verify ──
        let d0 =
            HwpxDecoder::decode(base).map_err(|e| StructuralEditError::Codec(e.to_string()))?;
        let e0 = encode_hwpx(&d0).map_err(map_admission)?;
        let d1 = HwpxDecoder::decode(&e0).map_err(|e| StructuralEditError::Codec(e.to_string()))?;
        admission_compare(&d0, &d1).map_err(map_admission)?;
        check_zip_carry(base, &e0).map_err(map_admission)?;

        let sections = d0.document.sections();

        // ── preflight: ranges, duplicates, rejection rules ──
        let mut seen = std::collections::BTreeSet::new();
        for t in targets {
            let section =
                sections.get(t.section).ok_or(StructuralEditError::SectionOutOfRange {
                    section: t.section,
                    available: sections.len(),
                })?;
            let para = section.paragraphs.get(t.index).ok_or(
                StructuralEditError::ParagraphOutOfRange {
                    section: t.section,
                    index: t.index,
                    available: section.paragraphs.len(),
                },
            )?;
            if !seen.insert((t.section, t.index)) {
                return Err(StructuralEditError::DuplicateTarget {
                    section: t.section,
                    index: t.index,
                });
            }
            if scan_paragraph_references(para).has_reference_material() {
                return Err(StructuralEditError::ReferenceStranded {
                    section: t.section,
                    index: t.index,
                });
            }
            if para.page_break || para.column_break {
                return Err(StructuralEditError::HardBreakLoss {
                    section: t.section,
                    index: t.index,
                });
            }
        }

        // ── declared delta: base document minus the targets ──
        let mut expected = d0.document.clone();
        remove_targets(&mut expected, targets);
        // Core invariants do not ride along the byte-splice path, so assert
        // them explicitly on the declared result. Empty-section is checked
        // precisely (which section) before the generic validate().
        for (idx, section) in expected.sections().iter().enumerate() {
            if section.paragraphs.is_empty() {
                return Err(StructuralEditError::EmptySection { section: idx });
            }
        }
        expected
            .clone()
            .validate()
            .map_err(|e| StructuralEditError::Codec(format!("validate: {e}")))?;

        // ── byte-splice each affected section's XML ──
        let mut package =
            RawPackage::read(base).map_err(|e| StructuralEditError::Codec(e.to_string()))?;
        let mut by_section: std::collections::BTreeMap<usize, Vec<usize>> =
            std::collections::BTreeMap::new();
        for t in targets {
            by_section.entry(t.section).or_default().push(t.index);
        }
        for (section_idx, mut indices) in by_section {
            let path = section_path(section_idx);
            let xml = package
                .read_text_entry(&path)
                .map_err(|e| StructuralEditError::Codec(e.to_string()))?;
            let spans = paragraph_spans(&xml)?;
            if spans.len() != sections[section_idx].paragraphs.len() {
                return Err(StructuralEditError::SpanCountMismatch {
                    section: section_idx,
                    decoded: sections[section_idx].paragraphs.len(),
                    wire: spans.len(),
                });
            }
            // Section properties (<hp:secPr>) live inside a paragraph (the
            // first) in HWPX; cutting that paragraph would silently reset the
            // section layout. Refuse — the self-verify would catch it, but a
            // clear rejection beats an opaque delta mismatch.
            for &idx in &indices {
                if xml[spans[idx].clone()].contains("<hp:secPr") {
                    return Err(StructuralEditError::SectionPropertiesParagraph {
                        section: section_idx,
                        index: idx,
                    });
                }
            }
            // Cut in descending order so earlier byte offsets stay valid.
            indices.sort_unstable_by(|a, b| b.cmp(a));
            let mut out = xml;
            for idx in indices {
                let span = spans[idx].clone();
                out.replace_range(span, "");
            }
            package.replace_text_entry(&path, out);
        }
        let bytes = package.write().map_err(|e| StructuralEditError::Codec(e.to_string()))?;

        // ── reverse-delta self-verify: output ≡ declared delta ──
        let d2 =
            HwpxDecoder::decode(&bytes).map_err(|e| StructuralEditError::Codec(e.to_string()))?;
        let expected_doc = HwpxDocument {
            document: expected,
            style_store: d0.style_store.clone(),
            image_store: d0.image_store.clone(),
        };
        admission_compare(&d2, &expected_doc).map_err(|e| match e {
            StamperError::NotRoundTripSafe { component, diff_path } => {
                StructuralEditError::DeltaMismatch {
                    detail: format!("{component} diverges at {diff_path}"),
                }
            }
            other => StructuralEditError::DeltaMismatch { detail: other.to_string() },
        })?;

        Ok(bytes)
    }
}

/// Collects the outer byte spans of a section's direct-child `<hp:p>` elements.
fn paragraph_spans(xml: &str) -> Result<Vec<std::ops::Range<usize>>, StructuralEditError> {
    let root =
        find_root_span(xml, b"hs:sec").map_err(|e| StructuralEditError::Codec(e.to_string()))?;
    collect_direct_child_outer_spans(xml, root, b"hp:p")
        .map_err(|e| StructuralEditError::Codec(e.to_string()))
}

/// Removes the targeted paragraphs from `document` (descending per section so
/// indices stay valid).
fn remove_targets(document: &mut Document<Draft>, targets: &[ParagraphLocator]) {
    let mut by_section: std::collections::BTreeMap<usize, Vec<usize>> =
        std::collections::BTreeMap::new();
    for t in targets {
        by_section.entry(t.section).or_default().push(t.index);
    }
    let sections = document.sections_mut();
    for (section_idx, mut indices) in by_section {
        if let Some(section) = sections.get_mut(section_idx) {
            indices.sort_unstable_by(|a, b| b.cmp(a));
            for idx in indices {
                if idx < section.paragraphs.len() {
                    section.paragraphs.remove(idx);
                }
            }
        }
    }
}

/// Result of scanning a paragraph subtree for reference material that makes a
/// structural delete unsafe under the **conservative** rule 1 (dangling-ref).
///
/// The scan is an exhaustive `serde_json` structure walk: serde visits every
/// field of the paragraph and everything nested under it — captions, group
/// children, table cells, and container paragraphs (text box / footnote /
/// endnote / ellipse / polygon / memo bodies) — so **no container can be
/// missed**. A typed walker would risk overlooking one of the ~15 nested
/// container sites; for a safety rule where a miss means silent corruption,
/// provable completeness is worth more than type-elegance.
///
/// The conservative rule needs only *presence*; measuring which id is
/// referenced by whom (to allow deleting a bookmark that nothing points at)
/// is the precise follow-up.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ReferenceScan {
    /// A `Control::Bookmark` is present somewhere in the subtree.
    pub bookmark: bool,
    /// A `Control::CrossRef` is present somewhere in the subtree.
    pub cross_ref: bool,
    /// A non-null `inst_id` is present — i.e. an identifiable object
    /// (footnote / endnote / equation / group / text-art / image / table)
    /// that a cross-reference elsewhere could point at.
    pub object_id: bool,
}

impl ReferenceScan {
    /// `true` when deleting this paragraph could strand a reference and the
    /// conservative rule must refuse.
    pub fn has_reference_material(&self) -> bool {
        self.bookmark || self.cross_ref || self.object_id
    }
}

/// Scans a paragraph subtree for conservative-rule reference material.
///
/// Fail-closed: a serialization failure (practically impossible for owned
/// `Paragraph` data) yields `object_id = true` so the caller refuses rather
/// than assuming the paragraph is safe to delete.
pub(crate) fn scan_paragraph_references(paragraph: &Paragraph) -> ReferenceScan {
    let mut scan = ReferenceScan::default();
    match serde_json::to_value(paragraph) {
        Ok(value) => walk_value(&value, &mut scan),
        Err(_) => scan.object_id = true, // unknown ⇒ reject (fail-closed)
    }
    scan
}

fn walk_value(value: &Value, scan: &mut ReferenceScan) {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                // Control is externally tagged: a bookmark serializes as
                // `{"Bookmark": {..}}`, a cross-ref as `{"CrossRef": {..}}`.
                // These PascalCase keys are enum discriminants and never
                // collide with the snake_case struct field names.
                match key.as_str() {
                    "Bookmark" => scan.bookmark = true,
                    "CrossRef" => scan.cross_ref = true,
                    "inst_id" if !child.is_null() => scan.object_id = true,
                    _ => {}
                }
                walk_value(child, scan);
                if scan.bookmark && scan.cross_ref && scan.object_id {
                    return; // saturated — nothing more to learn
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_value(item, scan);
                if scan.bookmark && scan.cross_ref && scan.object_id {
                    return;
                }
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::control::{Control, RefTarget};
    use hwpforge_core::{ObjectId, Paragraph, Run, Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, RefContentType, RefType};

    fn para() -> Paragraph {
        Paragraph::new(ParaShapeIndex::new(0))
    }

    fn with_control(control: Control) -> Paragraph {
        let mut p = para();
        p.runs.push(Run::control(control, CharShapeIndex::new(0)));
        p
    }

    #[test]
    fn plain_text_paragraph_has_no_reference_material() {
        let mut p = para();
        p.runs.push(Run::text("본문", CharShapeIndex::new(0)));
        let scan = scan_paragraph_references(&p);
        assert!(!scan.has_reference_material());
        assert_eq!(scan, ReferenceScan::default());
    }

    #[test]
    fn bookmark_is_detected() {
        let scan = scan_paragraph_references(&with_control(Control::bookmark("장시작")));
        assert!(scan.bookmark);
        assert!(scan.has_reference_material());
    }

    #[test]
    fn footnote_inst_id_is_detected() {
        let ctrl = Control::footnote_with_id(4242, vec![para()]);
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(scan.object_id);
        assert!(scan.has_reference_material());
    }

    #[test]
    fn footnote_without_inst_id_is_not_object_id() {
        // A footnote with inst_id = None is self-contained; nothing can
        // reference it, so it is not blocking on the object-id axis.
        let ctrl = Control::footnote(vec![para()]);
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(!scan.object_id);
    }

    #[test]
    fn reference_nested_in_group_children_is_detected() {
        // Serde recurses into Group children — the fill.rs walker's Group gap
        // cannot cause a miss here.
        let inner = Control::footnote_with_id(7, vec![para()]);
        let group = Control::Group {
            children: vec![inner],
            inst_id: None,
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            horz_offset: 0,
            vert_offset: 0,
        };
        let scan = scan_paragraph_references(&with_control(group));
        assert!(scan.object_id, "footnote hidden inside a group must be seen");
    }

    #[test]
    fn reference_nested_in_table_cell_is_detected() {
        let mut cell_para = para();
        cell_para.runs.push(Run::control(Control::bookmark("셀책갈피"), CharShapeIndex::new(0)));
        let cell = TableCell::new(vec![cell_para], HwpUnit::new(1000).unwrap());
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let mut p = para();
        p.runs.push(Run::table(table, CharShapeIndex::new(0)));
        let scan = scan_paragraph_references(&p);
        assert!(scan.bookmark, "bookmark inside a table cell must be seen");
    }

    #[test]
    fn cross_ref_is_detected() {
        let ctrl = Control::CrossRef {
            target: RefTarget::Object(ObjectId::new(9)),
            ref_type: RefType::Table,
            content_type: RefContentType::Number,
            as_hyperlink: false,
            display_text: String::new(),
        };
        let scan = scan_paragraph_references(&with_control(ctrl));
        assert!(scan.cross_ref);
        assert!(scan.has_reference_material());
    }

    // -- W2: delete_paragraphs (byte-splice) ------------------------------

    fn fixture(rel: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/structural")
            .join(rel);
        std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
    }

    fn body_texts(bytes: &[u8]) -> Vec<String> {
        let decoded = HwpxDecoder::decode(bytes).expect("decode");
        decoded.document.sections()[0]
            .paragraphs
            .iter()
            .map(|p| p.text_content().trim().to_string())
            .collect()
    }

    fn loc(section: usize, index: usize) -> ParagraphLocator {
        ParagraphLocator { section, index }
    }

    #[test]
    fn delete_middle_paragraph_preserves_others() {
        let base = fixture("plain_paragraphs.hwpx");
        let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]).expect("delete");
        assert_eq!(
            body_texts(&out),
            vec!["첫째 문단입니다.", "셋째 문단입니다.", "넷째 문단입니다."]
        );
    }

    #[test]
    fn delete_batch_removes_all_targets() {
        let base = fixture("plain_paragraphs.hwpx");
        let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1), loc(0, 3)])
            .expect("delete");
        assert_eq!(body_texts(&out), vec!["첫째 문단입니다.", "셋째 문단입니다."]);
    }

    #[test]
    fn delete_rejects_section_properties_paragraph() {
        // Paragraph 0 carries <hp:secPr>; deleting it would reset the section
        // layout, so it is refused with a clear error (not an opaque delta
        // mismatch). Confirms the self-verify's finding is surfaced cleanly.
        let base = fixture("plain_paragraphs.hwpx");
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 0)]),
            Err(StructuralEditError::SectionPropertiesParagraph { section: 0, index: 0 })
        ));
    }

    #[test]
    fn delete_preserves_non_section_entries_byte_for_byte() {
        let base = fixture("plain_paragraphs.hwpx");
        let out = HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]).expect("delete");
        let before = RawPackage::read(&base).unwrap();
        let after = RawPackage::read(&out).unwrap();
        // Every entry except the edited section XML is byte-identical.
        for path in before.entry_paths() {
            if path == "Contents/section0.xml" {
                continue;
            }
            assert_eq!(
                before.read_text_entry(path).ok(),
                after.read_text_entry(path).ok(),
                "entry {path} must be untouched"
            );
        }
    }

    #[test]
    fn delete_rejects_out_of_range() {
        let base = fixture("plain_paragraphs.hwpx");
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 99)]),
            Err(StructuralEditError::ParagraphOutOfRange { .. })
        ));
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(9, 0)]),
            Err(StructuralEditError::SectionOutOfRange { .. })
        ));
    }

    #[test]
    fn delete_rejects_duplicate_target() {
        let base = fixture("plain_paragraphs.hwpx");
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1), loc(0, 1)]),
            Err(StructuralEditError::DuplicateTarget { .. })
        ));
    }

    #[test]
    fn delete_rejects_emptying_the_section() {
        let base = fixture("plain_paragraphs.hwpx");
        let all = [loc(0, 0), loc(0, 1), loc(0, 2), loc(0, 3)];
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &all),
            Err(StructuralEditError::EmptySection { section: 0 })
        ));
    }

    #[test]
    fn delete_rejects_reference_bearing_paragraph() {
        let base = fixture("crossref_para.hwpx");
        // p0 carries a footnote (inst_id), p2 a cross-ref — both blocking.
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 0)]),
            Err(StructuralEditError::ReferenceStranded { .. })
        ));
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 2)]),
            Err(StructuralEditError::ReferenceStranded { .. })
        ));
    }

    #[test]
    fn delete_rejects_hard_break_paragraph() {
        let base = fixture("page_break.hwpx");
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(&base, &[loc(0, 1)]),
            Err(StructuralEditError::HardBreakLoss { .. })
        ));
    }

    #[test]
    fn delete_rejects_non_hwpx_bytes() {
        assert!(matches!(
            HwpxStructuralEditor::delete_paragraphs(b"not a zip", &[loc(0, 0)]),
            Err(StructuralEditError::Codec(_))
        ));
    }
}
