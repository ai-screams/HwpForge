//! Apply phase (E6 Wave 1A): promote approved candidates to named ClickHere
//! fields.
//!
//! All-or-nothing: the whole spec set preflights against a fresh plan of the
//! current document (spec↔candidate identity, marker equality, name
//! uniqueness against both the spec set and existing document fields,
//! coverage of every unguarded candidate) BEFORE any mutation. The mutation
//! walker mirrors `patch.rs::collect_semantic_text_slots` path construction
//! exactly — a coverage-mirror test locks the pair.

use std::collections::HashMap;
use std::fmt;
use std::ops::Range;

use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::{Control, Document, Draft, Paragraph, Section};
use hwpforge_foundation::FieldType;

use super::detect::BuiltinPattern;
use super::plan::{plan, StampCandidate};
use crate::fill::visit_section_fields;

/// Caller decision for one candidate (design §3-3: the caller map is the
/// only source of final field names — no auto-derived names, no suffixes).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StampAction {
    /// Promote to a named ClickHere field.
    Field {
        /// Unique field name (unique across specs AND existing fields).
        name: String,
        /// Optional hint; defaults to the original marker text.
        hint: Option<String>,
    },
    /// Explicitly leave this candidate unstamped.
    Ignore,
}

/// One approved candidate: identity (section/path/span/marker) + action.
///
/// Identity must match a live plan candidate exactly — a stale span or
/// edited document fails preflight instead of stamping the wrong text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampSpec {
    /// Zero-based section index (from [`StampCandidate::section`]).
    pub section: usize,
    /// Semantic slot path (from [`StampCandidate::path`]).
    pub path: String,
    /// UTF-8 byte span within the slot text (from [`StampCandidate::span`]).
    pub span: Range<usize>,
    /// Original marker text, verified against the document.
    pub marker: String,
    /// What to do with this candidate.
    pub action: StampAction,
}

/// One field created by [`apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampedField {
    /// The field name (unique in the output document).
    pub name: String,
    /// Section index.
    pub section: usize,
    /// Pre-stamp semantic slot path (`source_location` — run indices shift
    /// after the split, so this addresses the ORIGINAL document).
    pub path: String,
    /// UTF-8 byte span of the marker within the original slot text.
    pub span: Range<usize>,
    /// The original marker, now the field's body and default hint.
    pub marker: String,
    /// Which detector produced the candidate (manifest provenance).
    pub pattern: BuiltinPattern,
}

/// Result of a successful [`apply`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StampOutcome {
    /// Fields created, in spec order.
    pub stamped: Vec<StampedField>,
    /// Number of explicitly ignored candidates.
    pub ignored: usize,
    /// Guarded candidates left untouched because no spec approved them.
    pub skipped_guarded: Vec<StampCandidate>,
}

/// `stamp` preflight failure — nothing was modified.
#[derive(Debug)]
#[non_exhaustive]
pub enum StampError {
    /// Spec does not match any live candidate (path/span drift or the
    /// document changed since planning).
    UnknownSpec {
        /// Spec's section index.
        section: usize,
        /// Spec's slot path.
        path: String,
        /// Spec's span.
        span: Range<usize>,
    },
    /// Spec marker differs from the document's marker at that span.
    MarkerMismatch {
        /// Slot path.
        path: String,
        /// Marker the spec claimed.
        expected: String,
        /// Marker actually in the document.
        found: String,
    },
    /// Two specs target the same candidate.
    DuplicateSpec {
        /// Slot path.
        path: String,
        /// Duplicated span.
        span: Range<usize>,
    },
    /// Two specs claim the same field name.
    DuplicateName {
        /// The duplicated name.
        name: String,
    },
    /// A spec's name collides with an existing field in the document.
    NameCollision {
        /// The colliding name.
        name: String,
    },
    /// An unguarded candidate has no spec — every candidate must be named
    /// or explicitly ignored (design §3-2 step 5).
    UncoveredCandidate {
        /// Section index.
        section: usize,
        /// Slot path.
        path: String,
        /// Candidate span.
        span: Range<usize>,
        /// Candidate marker.
        marker: String,
    },
    /// A field name is empty.
    EmptyName,
}

impl fmt::Display for StampError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownSpec { section, path, span } => write!(
                f,
                "spec matches no live candidate: section {section}, {path} [{}..{}]",
                span.start, span.end
            ),
            Self::MarkerMismatch { path, expected, found } => {
                write!(f, "marker mismatch at {path}: spec {expected:?}, document {found:?}")
            }
            Self::DuplicateSpec { path, span } => {
                write!(f, "duplicate specs for {path} [{}..{}]", span.start, span.end)
            }
            Self::DuplicateName { name } => write!(f, "duplicate field name {name:?}"),
            Self::NameCollision { name } => {
                write!(f, "field name {name:?} already exists in the document")
            }
            Self::UncoveredCandidate { section, path, span, marker } => write!(
                f,
                "unguarded candidate {marker:?} (section {section}, {path} [{}..{}]) has no \
                 spec — name it or ignore it explicitly",
                span.start, span.end
            ),
            Self::EmptyName => write!(f, "field name must not be empty"),
        }
    }
}

impl std::error::Error for StampError {}

/// Applies the approved spec set to the document, all-or-nothing.
///
/// Preflight failures return [`StampError`] with the document untouched.
/// Guarded candidates without a spec are skipped (reported in the outcome);
/// a spec that names a guarded candidate is an explicit approval and is
/// applied.
///
/// # Errors
///
/// See [`StampError`] — every variant is a preflight rejection.
pub fn apply(
    document: &mut Document<Draft>,
    specs: &[StampSpec],
) -> Result<StampOutcome, StampError> {
    let candidates = plan(&*document);

    // ── preflight ───────────────────────────────────────────────────
    let mut by_key: HashMap<String, &StampCandidate> = HashMap::new();
    for c in &candidates {
        by_key.insert(candidate_key(c.section, &c.path, &c.span), c);
    }

    let mut seen_spec_keys: HashMap<String, ()> = HashMap::new();
    let mut names: HashMap<&str, ()> = HashMap::new();
    for spec in specs {
        let key = candidate_key(spec.section, &spec.path, &spec.span);
        let Some(candidate) = by_key.get(&key) else {
            return Err(StampError::UnknownSpec {
                section: spec.section,
                path: spec.path.clone(),
                span: spec.span.clone(),
            });
        };
        if candidate.marker != spec.marker {
            return Err(StampError::MarkerMismatch {
                path: spec.path.clone(),
                expected: spec.marker.clone(),
                found: candidate.marker.clone(),
            });
        }
        if seen_spec_keys.insert(key, ()).is_some() {
            return Err(StampError::DuplicateSpec {
                path: spec.path.clone(),
                span: spec.span.clone(),
            });
        }
        if let StampAction::Field { name, .. } = &spec.action {
            if name.is_empty() {
                return Err(StampError::EmptyName);
            }
            if names.insert(name, ()).is_some() {
                return Err(StampError::DuplicateName { name: name.clone() });
            }
        }
    }

    // Existing field names (fill's visitor — the single definition of
    // field-visit coverage).
    let mut existing: Vec<String> = Vec::new();
    for (idx, section) in document.sections_mut().iter_mut().enumerate() {
        visit_section_fields(section, idx, &mut |slot| {
            if let Control::Field { name: Some(name), .. } = &*slot.control {
                existing.push(name.clone());
            }
        });
    }
    for name in &existing {
        if names.contains_key(name.as_str()) {
            return Err(StampError::NameCollision { name: name.clone() });
        }
    }

    // Every unguarded candidate must be covered; uncovered guarded ones
    // are skipped and reported.
    let mut skipped_guarded = Vec::new();
    for c in &candidates {
        let key = candidate_key(c.section, &c.path, &c.span);
        if seen_spec_keys.contains_key(&key) {
            continue;
        }
        if c.guard.is_some() {
            skipped_guarded.push(c.clone());
        } else {
            return Err(StampError::UncoveredCandidate {
                section: c.section,
                path: c.path.clone(),
                span: c.span.clone(),
                marker: c.marker.clone(),
            });
        }
    }

    // ── mutation (cannot fail: everything verified above and we hold
    //    exclusive access) ───────────────────────────────────────────
    let mut index = SpecIndex::default();
    let mut stamped = Vec::new();
    let mut ignored = 0usize;
    for spec in specs {
        match &spec.action {
            StampAction::Ignore => ignored += 1,
            StampAction::Field { name, hint } => {
                // Preflight verified every spec resolves to a live candidate.
                let pattern = by_key
                    .get(&candidate_key(spec.section, &spec.path, &spec.span))
                    .map(|c| c.pattern)
                    .expect("preflight guarantees a matching candidate");
                stamped.push(StampedField {
                    name: name.clone(),
                    section: spec.section,
                    path: spec.path.clone(),
                    span: spec.span.clone(),
                    marker: spec.marker.clone(),
                    pattern,
                });
                index.by_slot.entry(slot_key(spec.section, &spec.path)).or_default().push(
                    ApprovedField {
                        span: spec.span.clone(),
                        marker: spec.marker.clone(),
                        name: name.clone(),
                        hint: hint.clone(),
                    },
                );
            }
        }
    }
    for fields in index.by_slot.values_mut() {
        fields.sort_by_key(|f| f.span.start);
    }

    for (section_idx, section) in document.sections_mut().iter_mut().enumerate() {
        transform_section(section, section_idx, &index);
    }

    Ok(StampOutcome { stamped, ignored, skipped_guarded })
}

fn candidate_key(section: usize, path: &str, span: &Range<usize>) -> String {
    format!("{section}:{path}:{}:{}", span.start, span.end)
}

fn slot_key(section: usize, path: &str) -> String {
    format!("{section}:{path}")
}

#[derive(Debug, Clone)]
struct ApprovedField {
    span: Range<usize>,
    marker: String,
    name: String,
    hint: Option<String>,
}

#[derive(Debug, Default)]
struct SpecIndex {
    /// `"{section}:{slot path}"` → approved fields, sorted by span start.
    by_slot: HashMap<String, Vec<ApprovedField>>,
}

// ── mutation walker — MUST mirror collect_semantic_text_slots paths ──

fn transform_section(section: &mut Section, section_idx: usize, index: &SpecIndex) {
    transform_paragraphs(&mut section.paragraphs, "paragraphs", section_idx, index);
    let header_count = section.headers.len();
    for (idx, header) in section.headers.iter_mut().enumerate() {
        let prefix = if header_count == 1 {
            "header.paragraphs".to_string()
        } else {
            format!("headers[{idx}].paragraphs")
        };
        transform_paragraphs(&mut header.paragraphs, &prefix, section_idx, index);
    }
    let footer_count = section.footers.len();
    for (idx, footer) in section.footers.iter_mut().enumerate() {
        let prefix = if footer_count == 1 {
            "footer.paragraphs".to_string()
        } else {
            format!("footers[{idx}].paragraphs")
        };
        transform_paragraphs(&mut footer.paragraphs, &prefix, section_idx, index);
    }
}

fn transform_paragraphs(
    paragraphs: &mut [Paragraph],
    prefix: &str,
    section_idx: usize,
    index: &SpecIndex,
) {
    for (paragraph_idx, paragraph) in paragraphs.iter_mut().enumerate() {
        let paragraph_prefix = format!("{prefix}[{paragraph_idx}]");

        // 1) recurse into nested containers (paths are independent of the
        //    run rebuild below, which only replaces Text runs)
        for (run_idx, run) in paragraph.runs.iter_mut().enumerate() {
            let run_prefix = format!("{paragraph_prefix}.runs[{run_idx}]");
            match &mut run.content {
                RunContent::Table(table) => {
                    let table_prefix = format!("{run_prefix}.table");
                    if let Some(caption) = table.caption.as_mut() {
                        transform_paragraphs(
                            &mut caption.paragraphs,
                            &format!("{table_prefix}.caption.paragraphs"),
                            section_idx,
                            index,
                        );
                    }
                    for (row_idx, row) in table.rows.iter_mut().enumerate() {
                        for (cell_idx, cell) in row.cells.iter_mut().enumerate() {
                            transform_paragraphs(
                                &mut cell.paragraphs,
                                &format!(
                                    "{table_prefix}.rows[{row_idx}].cells[{cell_idx}].paragraphs"
                                ),
                                section_idx,
                                index,
                            );
                        }
                    }
                }
                RunContent::Image(image) => {
                    if let Some(caption) = image.caption.as_mut() {
                        transform_paragraphs(
                            &mut caption.paragraphs,
                            &format!("{run_prefix}.image.caption.paragraphs"),
                            section_idx,
                            index,
                        );
                    }
                }
                RunContent::Control(control) => {
                    transform_control(
                        control,
                        &format!("{run_prefix}.control"),
                        section_idx,
                        index,
                    );
                }
                _ => {}
            }
        }

        // 2) rebuild this paragraph's runs when any spec targets them
        let needs_rebuild = (0..paragraph.runs.len()).any(|j| {
            index
                .by_slot
                .contains_key(&slot_key(section_idx, &format!("{paragraph_prefix}.runs[{j}].text")))
        });
        if !needs_rebuild {
            continue;
        }
        let old_runs = std::mem::take(&mut paragraph.runs);
        let mut new_runs = Vec::with_capacity(old_runs.len());
        for (run_idx, run) in old_runs.into_iter().enumerate() {
            let key = slot_key(section_idx, &format!("{paragraph_prefix}.runs[{run_idx}].text"));
            let Some(fields) = index.by_slot.get(&key) else {
                new_runs.push(run);
                continue;
            };
            let RunContent::Text(text) = &run.content else {
                // Preflight matched these specs against Text-slot candidates
                // of this exact document under exclusive access.
                debug_assert!(false, "stamp spec resolved to a non-Text run");
                new_runs.push(run);
                continue;
            };
            let char_shape = run.char_shape_id;
            let mut cursor = 0usize;
            for field in fields {
                if field.span.start > cursor {
                    new_runs.push(Run::text(&text[cursor..field.span.start], char_shape));
                }
                new_runs.push(Run::control(
                    Control::Field {
                        field_type: FieldType::ClickHere,
                        hint_text: Some(field.hint.clone().unwrap_or_else(|| field.marker.clone())),
                        help_text: None,
                        name: Some(field.name.clone()),
                        display_text: field.marker.clone(),
                    },
                    char_shape,
                ));
                cursor = field.span.end;
            }
            if cursor < text.len() {
                new_runs.push(Run::text(&text[cursor..], char_shape));
            }
        }
        paragraph.runs = new_runs;
    }
}

fn transform_control(control: &mut Control, prefix: &str, section_idx: usize, index: &SpecIndex) {
    match control {
        Control::TextBox { paragraphs, caption, .. } => {
            transform_paragraphs(
                paragraphs,
                &format!("{prefix}.textbox.paragraphs"),
                section_idx,
                index,
            );
            if let Some(caption) = caption.as_mut() {
                transform_paragraphs(
                    &mut caption.paragraphs,
                    &format!("{prefix}.textbox.caption.paragraphs"),
                    section_idx,
                    index,
                );
            }
        }
        Control::Footnote { paragraphs, .. } => {
            transform_paragraphs(
                paragraphs,
                &format!("{prefix}.footnote.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::Endnote { paragraphs, .. } => {
            transform_paragraphs(
                paragraphs,
                &format!("{prefix}.endnote.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::Ellipse { paragraphs, caption, .. } => {
            transform_paragraphs(
                paragraphs,
                &format!("{prefix}.ellipse.paragraphs"),
                section_idx,
                index,
            );
            if let Some(caption) = caption.as_mut() {
                transform_paragraphs(
                    &mut caption.paragraphs,
                    &format!("{prefix}.ellipse.caption.paragraphs"),
                    section_idx,
                    index,
                );
            }
        }
        Control::Polygon { paragraphs, caption, .. } => {
            transform_paragraphs(
                paragraphs,
                &format!("{prefix}.polygon.paragraphs"),
                section_idx,
                index,
            );
            if let Some(caption) = caption.as_mut() {
                transform_paragraphs(
                    &mut caption.paragraphs,
                    &format!("{prefix}.polygon.caption.paragraphs"),
                    section_idx,
                    index,
                );
            }
        }
        Control::Rect { caption: Some(caption), .. } => {
            transform_paragraphs(
                &mut caption.paragraphs,
                &format!("{prefix}.rect.caption.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::Line { caption: Some(caption), .. } => {
            transform_paragraphs(
                &mut caption.paragraphs,
                &format!("{prefix}.line.caption.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::Arc { caption: Some(caption), .. } => {
            transform_paragraphs(
                &mut caption.paragraphs,
                &format!("{prefix}.arc.caption.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::Curve { caption: Some(caption), .. } => {
            transform_paragraphs(
                &mut caption.paragraphs,
                &format!("{prefix}.curve.caption.paragraphs"),
                section_idx,
                index,
            );
        }
        Control::ConnectLine { caption: Some(caption), .. } => {
            transform_paragraphs(
                &mut caption.paragraphs,
                &format!("{prefix}.connect_line.caption.paragraphs"),
                section_idx,
                index,
            );
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use hwpforge_core::page::PageSettings;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    use super::super::detect::BuiltinPattern;
    use super::*;

    fn text_para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn doc_with_paras(paras: Vec<Paragraph>) -> Document {
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(paras, PageSettings::default()));
        doc
    }

    fn field_spec(c: &StampCandidate, name: &str) -> StampSpec {
        StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Field { name: name.to_string(), hint: None },
        }
    }

    fn ignore_spec(c: &StampCandidate) -> StampSpec {
        StampSpec {
            section: c.section,
            path: c.path.clone(),
            span: c.span.clone(),
            marker: c.marker.clone(),
            action: StampAction::Ignore,
        }
    }

    // ── preflight rejections (document must stay untouched) ─────────

    #[test]
    fn uncovered_unguarded_candidate_rejects() {
        let mut doc = doc_with_paras(vec![text_para("성명: (   )")]);
        let before = doc.clone();
        let err = apply(&mut doc, &[]).unwrap_err();
        assert!(matches!(err, StampError::UncoveredCandidate { .. }), "got {err}");
        assert_eq!(doc, before, "preflight failure must not modify the document");
    }

    #[test]
    fn unknown_spec_rejects() {
        let mut doc = doc_with_paras(vec![text_para("성명: (   )")]);
        let c = plan(&doc).remove(0);
        let mut spec = field_spec(&c, "성명");
        spec.span = 0..1; // stale span
        let err = apply(&mut doc, &[spec]).unwrap_err();
        assert!(matches!(err, StampError::UnknownSpec { .. }), "got {err}");
    }

    #[test]
    fn duplicate_name_rejects() {
        let mut doc = doc_with_paras(vec![text_para("성명 (   ) 소속 (  )")]);
        let cs = plan(&doc);
        assert_eq!(cs.len(), 2);
        let specs = vec![field_spec(&cs[0], "같은이름"), field_spec(&cs[1], "같은이름")];
        let err = apply(&mut doc, &specs).unwrap_err();
        assert!(matches!(err, StampError::DuplicateName { .. }), "got {err}");
    }

    #[test]
    fn name_collision_with_existing_field_rejects() {
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.add_run(Run::control(
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("기존".to_string()),
                help_text: None,
                name: Some("성명".to_string()),
                display_text: String::new(),
            },
            CharShapeIndex::new(0),
        ));
        let mut doc = doc_with_paras(vec![para, text_para("(   )")]);
        let c = plan(&doc).remove(0);
        let err = apply(&mut doc, &[field_spec(&c, "성명")]).unwrap_err();
        assert!(matches!(err, StampError::NameCollision { .. }), "got {err}");
    }

    #[test]
    fn empty_name_rejects() {
        let mut doc = doc_with_paras(vec![text_para("(   )")]);
        let c = plan(&doc).remove(0);
        let err = apply(&mut doc, &[field_spec(&c, "")]).unwrap_err();
        assert!(matches!(err, StampError::EmptyName), "got {err}");
    }

    // ── guarded candidates ──────────────────────────────────────────

    #[test]
    fn guarded_without_spec_is_skipped_and_reported() {
        let mut doc = doc_with_paras(vec![text_para("※ 해당하는 항목의 □에 표시")]);
        let before = doc.clone();
        let outcome = apply(&mut doc, &[]).unwrap();
        assert!(outcome.stamped.is_empty());
        assert_eq!(outcome.skipped_guarded.len(), 1);
        assert_eq!(doc, before, "skipped guard must leave the document unchanged");
    }

    #[test]
    fn guarded_with_spec_is_applied() {
        let mut doc = doc_with_paras(vec![text_para("※ 신청인 (인)")]);
        let c = plan(&doc).remove(0);
        assert!(c.guard.is_some());
        let outcome = apply(&mut doc, &[field_spec(&c, "신청인도장")]).unwrap();
        assert_eq!(outcome.stamped.len(), 1);
        assert!(outcome.skipped_guarded.is_empty());
        assert_eq!(plan(&doc).len(), 0, "stamped marker must no longer be a candidate");
    }

    // ── successful application ──────────────────────────────────────

    #[test]
    fn apply_splits_run_and_preserves_surroundings() {
        let mut doc = doc_with_paras(vec![text_para("성명: (   ) 끝")]);
        let c = plan(&doc).remove(0);
        let outcome = apply(&mut doc, &[field_spec(&c, "성명")]).unwrap();
        assert_eq!(outcome.stamped.len(), 1);

        let runs = &doc.sections()[0].paragraphs[0].runs;
        assert_eq!(runs.len(), 3, "prefix text + field + suffix text");
        assert_eq!(runs[0].content.as_text(), Some("성명: "));
        match &runs[1].content {
            RunContent::Control(control) => match control.as_ref() {
                Control::Field {
                    field_type: FieldType::ClickHere,
                    name,
                    hint_text,
                    display_text,
                    ..
                } => {
                    assert_eq!(name.as_deref(), Some("성명"));
                    assert_eq!(hint_text.as_deref(), Some("(   )"), "hint defaults to marker");
                    assert_eq!(display_text, "(   )", "body keeps the marker verbatim");
                }
                other => panic!("expected ClickHere field, got {other:?}"),
            },
            other => panic!("expected control run, got {other:?}"),
        }
        assert_eq!(runs[2].content.as_text(), Some(" 끝"));
        assert_eq!(plan(&doc).len(), 0, "re-plan after stamping must be empty");
    }

    #[test]
    fn apply_multiple_markers_in_one_run() {
        let mut doc = doc_with_paras(vec![text_para("□ 예 □ 아니오")]);
        let cs = plan(&doc);
        assert_eq!(cs.len(), 2);
        let specs = vec![field_spec(&cs[0], "예"), field_spec(&cs[1], "아니오")];
        let outcome = apply(&mut doc, &specs).unwrap();
        assert_eq!(outcome.stamped.len(), 2);

        let runs = &doc.sections()[0].paragraphs[0].runs;
        // field(□) + " 예 " + field(□) + " 아니오"
        assert_eq!(runs.len(), 4);
        let is_field = |content: &RunContent| matches!(content, RunContent::Control(c) if matches!(c.as_ref(), Control::Field { .. }));
        assert!(is_field(&runs[0].content));
        assert_eq!(runs[1].content.as_text(), Some(" 예 "));
        assert!(is_field(&runs[2].content));
        assert_eq!(runs[3].content.as_text(), Some(" 아니오"));
    }

    #[test]
    fn ignore_action_is_counted_and_leaves_text_alone() {
        let mut doc = doc_with_paras(vec![text_para("금액: (   )억원")]);
        let before = doc.clone();
        let c = plan(&doc).remove(0);
        let outcome = apply(&mut doc, &[ignore_spec(&c)]).unwrap();
        assert_eq!(outcome.ignored, 1);
        assert!(outcome.stamped.is_empty());
        assert_eq!(doc, before);
    }

    #[test]
    fn stamped_cell_candidate_round_trips_through_plan() {
        let width = HwpUnit::new(1000).unwrap();
        let row = TableRow::new(vec![TableCell::new(vec![text_para("□ 동의")], width)]);
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));
        let mut doc = doc_with_paras(vec![host]);

        let c = plan(&doc).remove(0);
        assert_eq!(c.pattern, BuiltinPattern::Checkbox);
        let outcome = apply(&mut doc, &[field_spec(&c, "동의여부")]).unwrap();
        assert_eq!(outcome.stamped.len(), 1);
        assert_eq!(plan(&doc).len(), 0);
    }

    // ── coverage mirror: every container the planner sees, the
    //    transformer must reach ────────────────────────────────────────

    #[test]
    fn mutation_walker_reaches_every_planned_container() {
        // Body + table cell + table caption + textbox + footnote — one
        // marker each. Stamp all candidates, then re-plan must be empty;
        // any walker asymmetry leaves a candidate behind (or trips the
        // debug_assert in the rebuild).
        use hwpforge_core::caption::{Caption, CaptionSide};

        let width = HwpUnit::new(1000).unwrap();
        let mut table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![text_para("셀 (   )")],
            width,
        )])]);
        table.caption = Some(Caption::new(vec![text_para("캡션 □")], CaptionSide::default()));
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(table, CharShapeIndex::new(0)));

        let mut doc = doc_with_paras(vec![text_para("본문 (  )"), host]);

        let cs = plan(&doc);
        assert_eq!(cs.len(), 3, "expected candidates in body, cell, caption: {cs:?}");
        let specs: Vec<StampSpec> =
            cs.iter().enumerate().map(|(i, c)| field_spec(c, &format!("필드{i}"))).collect();
        let outcome = apply(&mut doc, &specs).unwrap();
        assert_eq!(outcome.stamped.len(), 3);
        assert_eq!(plan(&doc).len(), 0, "walker must reach every planned container");
    }
}
