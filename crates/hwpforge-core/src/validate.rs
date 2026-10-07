//! Document validation logic (crate-private).
//!
//! This module contains the validation rules for transitioning a
//! `Document<Draft>` to `Document<Validated>`. It is not part of the
//! public API; it is called exclusively by `Document<Draft>::validate()`.

use crate::control::{Control, GroupChild};
use crate::error::ValidationError;
use crate::run::RunContent;
use crate::section::Section;
use hwpforge_foundation::ApplyPageType;

#[derive(Clone, Copy)]
struct RunValidationContext {
    section_index: usize,
    paragraph_index: usize,
    run_index: usize,
}

/// Validates the document structure.
///
/// # Rules
///
/// 1. At least 1 section
/// 2. Every section has at least 1 paragraph
/// 3. Every paragraph has at least 1 run
/// 4. Every table has at least 1 row with at least 1 cell — except that a
///    row may be empty when row spans from earlier rows cover it completely
///    (requires the whole table to tile a well-formed logical grid; see
///    [`crate::table::grid::TableGrid`])
/// 5. Every table cell has at least 1 paragraph
/// 6. `col_span >= 1`, `row_span >= 1`
/// 7. TextBox controls have at least 1 paragraph
/// 8. Footnote controls have at least 1 paragraph
/// 9. A master page with a special scope applies to both page types
/// 10. A header/footer anchor points at an existing body paragraph
/// 11. A rectangle's or text box's corner ratio is at most 50
///
/// Errors carry precise location context.
pub(crate) fn validate_sections(sections: &[Section]) -> Result<(), ValidationError> {
    if sections.is_empty() {
        return Err(ValidationError::EmptyDocument);
    }

    for (si, section) in sections.iter().enumerate() {
        if section.paragraphs.is_empty() {
            return Err(ValidationError::EmptySection { section_index: si });
        }

        for (pi, paragraph) in section.paragraphs.iter().enumerate() {
            if paragraph.runs.is_empty() {
                return Err(ValidationError::EmptyParagraph {
                    section_index: si,
                    paragraph_index: pi,
                });
            }

            for (ri, run) in paragraph.runs.iter().enumerate() {
                validate_run_content(
                    &run.content,
                    RunValidationContext { section_index: si, paragraph_index: pi, run_index: ri },
                )?;
            }
        }

        // 이것을 실패시키는 것: 이 검사를 지우기, 또는 `<` 를 `<=` 로
        // (`header_footer_anchor_*` 두 테스트 — 마지막 문단 index 는 통과,
        // 문단 수와 같은 index 는 거부).
        for (kind, list) in [("header", &section.headers), ("footer", &section.footers)] {
            for (index, hf) in list.iter().enumerate() {
                if let Some(anchor) = hf.anchor_paragraph {
                    if anchor >= section.paragraphs.len() {
                        return Err(ValidationError::HeaderFooterAnchorOutOfRange {
                            section_index: si,
                            kind,
                            index,
                            anchor_paragraph: anchor,
                            paragraph_count: section.paragraphs.len(),
                        });
                    }
                }
            }
        }

        // §19.5 1c (review r1): the 0–50 corner ratio is checked wherever
        // the encoder writes a shape — body, cells, text boxes, notes,
        // captions, group children, headers/footers, master pages.
        // 이것을 실패시키는 것: 중첩 순회를 지우기(칸·글상자·머리말·바탕쪽의 51
        // 이 통과 — `corner_ratio_is_checked_in_nested_places`) / `> 50` 을
        // `>= 50` 으로(`corner_ratio_50_accepted`).
        let lists = std::iter::once(&section.paragraphs)
            .chain(section.headers.iter().chain(section.footers.iter()).map(|hf| &hf.paragraphs))
            .chain(section.master_pages.iter().flatten().map(|mp| &mp.paragraphs));
        for paragraphs in lists {
            for (pi, paragraph) in paragraphs.iter().enumerate() {
                for (ri, run) in paragraph.runs.iter().enumerate() {
                    if let Some(ratio) = run_corner_ratio_over_50(run) {
                        return Err(ValidationError::InvalidCornerRatio {
                            section_index: si,
                            paragraph_index: pi,
                            run_index: ri,
                            ratio,
                        });
                    }
                }
            }
        }

        // 이것을 실패시키는 것: 이 검사를 지우기, 또는 `Both` 비교를 빼고
        // `special` 만 보기 (`master_page_special_scope_*` 두 테스트).
        for (mi, master) in section.master_pages.iter().flatten().enumerate() {
            if master.special.is_some() && master.apply_page_type != ApplyPageType::Both {
                return Err(ValidationError::InvalidMasterPageScope {
                    section_index: si,
                    master_page_index: mi,
                });
            }
        }
    }

    Ok(())
}

/// The first corner ratio above 50 in `run`: its own rectangle or text
/// box, a group's shapes, and every paragraph nested in it — picture
/// captions included, which the Core paragraph walk does not enter but the
/// HWPX writer does (review r2).
///
/// (A group picture's caption is already in that walk.)
///
/// 이것을 실패시키는 것: 그림 run 의 캡션 / 중첩 문단 안 그림의 캡션을
/// 건너뛰기(`corner_ratio_is_checked_in_nested_places`).
fn run_corner_ratio_over_50(run: &crate::run::Run) -> Option<u8> {
    fn caption_ratio(image: &crate::image::Image) -> Option<u8> {
        image
            .caption
            .as_ref()?
            .paragraphs
            .iter()
            .flat_map(|p| &p.runs)
            .find_map(run_corner_ratio_over_50)
    }
    fn control_ratio(control: &Control) -> Option<u8> {
        match control {
            Control::Rect { style: Some(style), .. }
            | Control::TextBox { style: Some(style), .. } => style.corner_ratio.filter(|&r| r > 50),
            Control::Group { children, .. } => children.iter().find_map(|child| match child {
                crate::control::GroupChild::Shape(shape) => control_ratio(shape),
                crate::control::GroupChild::Picture(_) => None,
            }),
            _ => None,
        }
    }
    fn own_ratio(run: &crate::run::Run) -> Option<u8> {
        match &run.content {
            RunContent::Control(control) => control_ratio(control),
            RunContent::Image(image) => caption_ratio(image),
            _ => None,
        }
    }
    let mut found = own_ratio(run);
    run.walk_paragraphs(&mut |nested| {
        if found.is_none() {
            found = nested.runs.iter().find_map(own_ratio);
        }
    });
    found
}

/// Validates a single run's content recursively.
fn validate_run_content(
    content: &RunContent,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    match content {
        RunContent::Table(table) => validate_table_run(table, ctx)?,
        RunContent::Control(control) => validate_control_run(control.as_ref(), ctx)?,
        RunContent::Text(_) | RunContent::InlineText(_) | RunContent::Image(_) => {}
    }
    Ok(())
}

fn validate_table_run(
    table: &crate::table::Table,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    if table.rows.is_empty() {
        return Err(ValidationError::EmptyTable {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
        });
    }

    // An empty row is legal only when row spans from earlier rows cover it
    // completely (native formats keep the row element with no cells). Grid
    // derivation succeeding implies every empty row is fully covered; for
    // tables whose cells do not tile a well-formed grid the historical
    // rejection stands.
    // The zero-width guard keeps tables with no cells at all (degenerate
    // grids like 1×0) on the historical rejection path.
    let empty_rows_covered = table.rows.iter().any(|row| row.cells.is_empty())
        && crate::table::grid::TableGrid::from_table(table)
            .is_ok_and(|grid| grid.dimensions().1 > 0);

    let mut seen_non_header_row: bool = false;
    for (row_i, row) in table.rows.iter().enumerate() {
        validate_table_row(row, ctx, row_i, &mut seen_non_header_row, empty_rows_covered)?;
    }

    Ok(())
}

fn validate_table_row(
    row: &crate::table::TableRow,
    ctx: RunValidationContext,
    row_i: usize,
    seen_non_header_row: &mut bool,
    empty_rows_covered: bool,
) -> Result<(), ValidationError> {
    if row.cells.is_empty() && !empty_rows_covered {
        return Err(ValidationError::EmptyTableRow {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            row_index: row_i,
        });
    }

    if row.is_header {
        if *seen_non_header_row {
            return Err(ValidationError::NonLeadingTableHeaderRow {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
                row_index: row_i,
            });
        }
    } else {
        *seen_non_header_row = true;
    }

    for (cell_i, cell) in row.cells.iter().enumerate() {
        validate_table_cell(cell, ctx, row_i, cell_i)?;
    }

    Ok(())
}

fn validate_table_cell(
    cell: &crate::table::TableCell,
    ctx: RunValidationContext,
    row_i: usize,
    cell_i: usize,
) -> Result<(), ValidationError> {
    if cell.col_span == 0 {
        return Err(ValidationError::InvalidSpan {
            field: "col_span",
            value: 0,
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            row_index: row_i,
            cell_index: cell_i,
        });
    }
    if cell.row_span == 0 {
        return Err(ValidationError::InvalidSpan {
            field: "row_span",
            value: 0,
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            row_index: row_i,
            cell_index: cell_i,
        });
    }
    if cell.paragraphs.is_empty() {
        return Err(ValidationError::EmptyTableCell {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            row_index: row_i,
            cell_index: cell_i,
        });
    }

    Ok(())
}

fn validate_control_run(
    control: &Control,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    match control {
        Control::TextBox { paragraphs, .. } => validate_control_paragraphs(
            paragraphs.is_empty(),
            ValidationError::EmptyTextBox {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
            },
        ),
        Control::Footnote { paragraphs, .. } => validate_control_paragraphs(
            paragraphs.is_empty(),
            ValidationError::EmptyFootnote {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
            },
        ),
        Control::Endnote { paragraphs, .. } => validate_control_paragraphs(
            paragraphs.is_empty(),
            ValidationError::EmptyEndnote {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
            },
        ),
        Control::Line { .. } => Ok(()),
        Control::Rect { width, height, .. } => {
            validate_shape_dimensions(width.as_i32(), height.as_i32(), "Rect", ctx)
        }
        Control::Ellipse { width, height, .. } => {
            validate_shape_dimensions(width.as_i32(), height.as_i32(), "Ellipse", ctx)
        }
        Control::Polygon { vertices, width, height, .. } => {
            validate_polygon_control(vertices.len(), width.as_i32(), height.as_i32(), ctx)
        }
        Control::Chart { data, width, height, .. } => {
            validate_chart_control(data, width.as_i32(), height.as_i32(), ctx)
        }
        Control::EmbeddedChart { chart_xml, ole_bytes, width, height, .. } => {
            validate_embedded_chart_control(
                chart_xml,
                ole_bytes,
                width.as_i32(),
                height.as_i32(),
                ctx,
            )
        }
        Control::Equation { script, width, height, .. } => {
            validate_equation_control(script, width.as_i32(), height.as_i32(), ctx)
        }
        Control::TextArt { width, height, .. } => {
            validate_shape_dimensions(width.as_i32(), height.as_i32(), "TextArt", ctx)
        }
        Control::Hyperlink { .. }
        | Control::Unknown { .. }
        | Control::Dutmal { .. }
        | Control::Compose { .. }
        | Control::Arc { .. }
        | Control::Curve { .. }
        | Control::ConnectLine { .. }
        | Control::Bookmark { .. }
        | Control::CrossRef { .. }
        | Control::Field { .. }
        | Control::Memo { .. }
        | Control::IndexMark { .. } => Ok(()),
        // Wave 12n: targeted invariant checks for new variants. Architect review
        // medium: do not let new variants slide through with a blanket `Ok(())`.
        Control::UnknownSummary { token, .. } => validate_unknown_summary(token, ctx),
        Control::DateCodeField { .. }
        | Control::InlinePageNumber { .. }
        | Control::PathField { .. } => Ok(()),
        // W2 (2026-08-12): 새 번호 지정 — number 는 wire 차원에서 전 u16 이
        // 유효 (Codex 결함 8: 근거 없는 clamp 금지). kind Unknown 은 인코더가
        // 스킵하므로 validation 통과.
        Control::NewNumber { .. } => Ok(()),
        // W3 (2026-08-12): 감추기 — 6 bool 전 조합이 유효 (빈 mask 도
        // corpus 실측 존재, 렌더 no-op).
        Control::PageHiding { .. } => Ok(()),
        // Group (묶음 객체): recursively validate each child (nested groups +
        // per-child dimension/empty checks at the same chokepoint). Only
        // shape-family controls are legitimate children; non-shape variants
        // are rejected so a malformed builder caller can't smuggle a
        // hyperlink/field/memo into a `<hp:container>`.
        //
        // A picture child must name its image and have a size: an empty path
        // is reported as `InvalidGroupChild` (there is no picture to draw), a
        // width or height of zero or less as `InvalidShapeDimension`.
        Control::Group { children, .. } => {
            let invalid_child = || ValidationError::InvalidGroupChild {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
            };
            for child in children {
                match child {
                    GroupChild::Picture(image) => {
                        if image.path.is_empty() {
                            return Err(invalid_child());
                        }
                        if image.width.as_i32() <= 0 || image.height.as_i32() <= 0 {
                            return Err(ValidationError::InvalidShapeDimension {
                                section_index: ctx.section_index,
                                paragraph_index: ctx.paragraph_index,
                                run_index: ctx.run_index,
                                shape_type: "Picture",
                            });
                        }
                    }
                    GroupChild::Shape(control) => {
                        if !is_group_child_allowed(control) {
                            return Err(invalid_child());
                        }
                        validate_control_run(control, ctx)?;
                    }
                }
            }
            Ok(())
        }
    }
}

/// Returns `true` if `control` is a drawing-shape variant permitted as a
/// shape child of [`Control::Group`]. Groups may nest the geometric shapes,
/// OLE objects and further groups (pictures are [`GroupChild::Picture`]) — but not text-flow controls
/// (hyperlink/field/memo/bookmark/cross-ref/index-mark/page-number).
fn is_group_child_allowed(control: &Control) -> bool {
    matches!(
        control,
        Control::Line { .. }
            | Control::Rect { .. }
            | Control::Ellipse { .. }
            | Control::Arc { .. }
            | Control::Polygon { .. }
            | Control::Curve { .. }
            | Control::ConnectLine { .. }
            | Control::TextBox { .. }
            | Control::EmbeddedChart { .. }
            | Control::Equation { .. }
            | Control::TextArt { .. }
            | Control::Group { .. }
    )
}

/// Rejects an `UnknownSummary` whose token is empty or does not start with
/// `$` — both are nonsensical for an HWPX SUMMERY `Command` and would
/// emit garbage XML.
fn validate_unknown_summary(token: &str, ctx: RunValidationContext) -> Result<(), ValidationError> {
    if token.is_empty() || !token.starts_with('$') {
        Err(ValidationError::InvalidSummaryToken {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            token: token.to_string(),
        })
    } else {
        Ok(())
    }
}

fn validate_control_paragraphs(
    is_empty: bool,
    err: ValidationError,
) -> Result<(), ValidationError> {
    if is_empty {
        Err(err)
    } else {
        Ok(())
    }
}

fn validate_shape_dimensions(
    width: i32,
    height: i32,
    shape_type: &'static str,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    if width == 0 || height == 0 {
        return Err(ValidationError::InvalidShapeDimension {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            shape_type,
        });
    }

    Ok(())
}

fn validate_polygon_control(
    vertex_count: usize,
    width: i32,
    height: i32,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    if vertex_count < 3 {
        return Err(ValidationError::InvalidPolygon {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
            vertex_count,
        });
    }

    validate_shape_dimensions(width, height, "Polygon", ctx)
}

fn validate_chart_control(
    data: &crate::chart::ChartData,
    width: i32,
    height: i32,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    if data.has_no_series() {
        return Err(ValidationError::EmptyChartData {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
        });
    }

    if let crate::chart::ChartData::Category { categories, .. } = data {
        if categories.is_empty() {
            return Err(ValidationError::EmptyCategoryLabels {
                section_index: ctx.section_index,
                paragraph_index: ctx.paragraph_index,
                run_index: ctx.run_index,
            });
        }
    }

    if let crate::chart::ChartData::Xy { series } = data {
        for s in series {
            if s.x_values.len() != s.y_values.len() {
                return Err(ValidationError::MismatchedSeriesLengths {
                    section_index: ctx.section_index,
                    paragraph_index: ctx.paragraph_index,
                    run_index: ctx.run_index,
                    series_name: s.name.clone(),
                    x_len: s.x_values.len(),
                    y_len: s.y_values.len(),
                });
            }
        }
    }

    validate_shape_dimensions(width, height, "Chart", ctx)
}

fn validate_embedded_chart_control(
    chart_xml: &str,
    ole_bytes: &[u8],
    width: i32,
    height: i32,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    // Wave 4c passthrough: chart XML and OLE bytes are opaque blobs we
    // do not introspect. Treat empty payload as the same failure mode as
    // EmptyChartData / EmptyEquation so downstream encoders never have to
    // emit an empty <hp:chart>/<hp:ole> pair.
    if chart_xml.is_empty() || ole_bytes.is_empty() {
        return Err(ValidationError::EmptyChartData {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
        });
    }

    validate_shape_dimensions(width, height, "EmbeddedChart", ctx)
}

fn validate_equation_control(
    script: &str,
    width: i32,
    height: i32,
    ctx: RunValidationContext,
) -> Result<(), ValidationError> {
    if script.is_empty() {
        return Err(ValidationError::EmptyEquation {
            section_index: ctx.section_index,
            paragraph_index: ctx.paragraph_index,
            run_index: ctx.run_index,
        });
    }

    validate_shape_dimensions(width, height, "Equation", ctx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object_id::ObjectId;
    use crate::page::PageSettings;
    use crate::paragraph::Paragraph;
    use crate::run::Run;
    use crate::section::{MasterPage, MasterPageSpecial, Section};
    use crate::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, VerticalAlign};

    fn text_run(s: &str) -> Run {
        Run::text(s, CharShapeIndex::new(0))
    }

    fn simple_paragraph() -> Paragraph {
        Paragraph::with_runs(vec![text_run("text")], ParaShapeIndex::new(0))
    }

    fn simple_section() -> Section {
        Section::with_paragraphs(vec![simple_paragraph()], PageSettings::a4())
    }

    fn simple_cell() -> TableCell {
        TableCell::new(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap())
    }

    // === Rule 9: master page special scope ===

    fn section_with_master(apply: ApplyPageType, special: Option<MasterPageSpecial>) -> Section {
        let mut section = simple_section();
        let ok = MasterPage::new(ApplyPageType::Both, vec![simple_paragraph()]);
        let mut checked = MasterPage::new(apply, vec![simple_paragraph()]);
        checked.special = special;
        section.master_pages = Some(vec![ok, checked]);
        section
    }

    // 이것을 실패시키는 것: Rule 9 검사를 지우기.
    #[test]
    fn master_page_special_scope_requires_both() {
        let section = section_with_master(ApplyPageType::Even, Some(MasterPageSpecial::LastPage));
        assert_eq!(
            validate_sections(&[section]),
            Err(ValidationError::InvalidMasterPageScope { section_index: 0, master_page_index: 1 })
        );
        assert_eq!(
            ValidationError::InvalidMasterPageScope { section_index: 0, master_page_index: 1 }
                .code() as u32,
            2021
        );
    }

    // 이것을 실패시키는 것: `apply_page_type != Both` 조건을 빼고 `special` 만
    // 보기 (special + Both 를 거부), 또는 `special.is_some()` 을 빼기
    // (special 없는 Odd 를 거부).
    #[test]
    fn master_page_special_scope_accepts_both_and_plain_even_odd() {
        let special = Some(MasterPageSpecial::OptionalPage { page_number: 3 });
        assert_eq!(validate_sections(&[section_with_master(ApplyPageType::Both, special)]), Ok(()));
        assert_eq!(validate_sections(&[section_with_master(ApplyPageType::Odd, None)]), Ok(()));
    }

    // === Rule 10: header/footer anchor in range ===

    fn section_with_anchored_footer(paragraphs: usize, anchor: usize) -> Section {
        let mut section = Section::with_paragraphs(
            (0..paragraphs).map(|_| simple_paragraph()).collect(),
            PageSettings::a4(),
        );
        section.headers.push(crate::section::HeaderFooter::all_pages(vec![simple_paragraph()]));
        let mut footer = crate::section::HeaderFooter::all_pages(vec![simple_paragraph()]);
        footer.anchor_paragraph = Some(anchor);
        section.footers.push(footer);
        section
    }

    // 이것을 실패시키는 것: Rule 10 검사를 지우기, 또는 `>=` 를 `>` 로.
    #[test]
    fn header_footer_anchor_equal_to_paragraph_count_rejected() {
        let err = validate_sections(&[section_with_anchored_footer(3, 3)]).unwrap_err();
        assert_eq!(
            err,
            ValidationError::HeaderFooterAnchorOutOfRange {
                section_index: 0,
                kind: "footer",
                index: 0,
                anchor_paragraph: 3,
                paragraph_count: 3,
            }
        );
        assert_eq!(err.code() as u32, 2022);
    }

    // 이것을 실패시키는 것: 비교를 `anchor + 1 >= len` 으로 당기기(마지막
    // 문단 거부).
    #[test]
    fn header_footer_anchor_on_last_paragraph_accepted() {
        assert_eq!(validate_sections(&[section_with_anchored_footer(3, 2)]), Ok(()));
    }

    // === Rule 11: corner ratio ≤ 50 ===

    fn section_with_rect_ratio(ratio: u8) -> Section {
        let mut rect =
            Control::rect(HwpUnit::new(1000).unwrap(), HwpUnit::new(1000).unwrap()).unwrap();
        if let Control::Rect { style, .. } = &mut rect {
            *style = Some(crate::control::ShapeStyle {
                corner_ratio: Some(ratio),
                ..Default::default()
            });
        }
        Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(rect, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )
    }

    #[test]
    fn corner_ratio_above_50_rejected() {
        let err = validate_sections(&[section_with_rect_ratio(51)]).unwrap_err();
        assert_eq!(
            err,
            ValidationError::InvalidCornerRatio {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                ratio: 51
            }
        );
        assert_eq!(err.code(), crate::error::CoreErrorCode::InvalidShapeDimension);
    }

    /// A ratio above 50 is refused wherever the shape sits — a table cell,
    /// a text box's text, a group, a header, a footer, a master page, a
    /// picture's caption (review r2) — and
    /// 50 passes there too (review r1: only top-level runs were checked, so
    /// a nested 51 reached the HWPX writer).
    #[test]
    fn corner_ratio_is_checked_in_nested_places() {
        use crate::control::GroupChild;
        use crate::section::{HeaderFooter, MasterPage};
        use crate::table::{Table, TableCell, TableRow};
        let rect_run = |ratio: u8| section_with_rect_ratio(ratio).paragraphs[0].runs[0].clone();
        let para = |run: Run| Paragraph::with_runs(vec![run], ParaShapeIndex::new(0));
        let plain = || para(Run::text("x", CharShapeIndex::new(0)));
        let unit = HwpUnit::new(1000).unwrap();
        let captioned = move |r: u8| {
            crate::image::Image::new("BinData/a.png", unit, unit, crate::image::ImageFormat::Png)
                .with_caption(crate::caption::Caption::new(
                    vec![para(rect_run(r))],
                    crate::caption::CaptionSide::Bottom,
                ))
        };
        let places: Vec<(&str, Box<dyn Fn(u8) -> Section>)> = vec![
            (
                "cell",
                Box::new(move |r| {
                    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![para(rect_run(r))],
                        unit,
                    )])]);
                    Section::with_paragraphs(
                        vec![para(Run::table(table, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
            (
                "text box",
                Box::new(move |r| {
                    let tb = Control::text_box(vec![para(rect_run(r))], unit, unit);
                    Section::with_paragraphs(
                        vec![para(Run::control(tb, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
            (
                "group",
                Box::new(move |r| {
                    let RunContent::Control(rect) = rect_run(r).content else { unreachable!() };
                    let group = Control::Group {
                        children: vec![GroupChild::Shape(*rect)],
                        width: unit,
                        height: unit,
                        placement: None,
                        inst_id: None,
                    };
                    Section::with_paragraphs(
                        vec![para(Run::control(group, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
            (
                "header",
                Box::new(move |r| {
                    let mut s = Section::with_paragraphs(vec![plain()], PageSettings::a4());
                    s.headers.push(HeaderFooter::all_pages(vec![para(rect_run(r))]));
                    s
                }),
            ),
            (
                "footer",
                Box::new(move |r| {
                    let mut s = Section::with_paragraphs(vec![plain()], PageSettings::a4());
                    s.footers.push(HeaderFooter::all_pages(vec![para(rect_run(r))]));
                    s
                }),
            ),
            (
                "master page",
                Box::new(move |r| {
                    let mut s = Section::with_paragraphs(vec![plain()], PageSettings::a4());
                    s.master_pages = Some(vec![MasterPage::new(
                        hwpforge_foundation::ApplyPageType::Both,
                        vec![para(rect_run(r))],
                    )]);
                    s
                }),
            ),
            (
                "picture caption",
                Box::new(move |r| {
                    let image = captioned(r);
                    Section::with_paragraphs(
                        vec![para(Run::image(image, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
            (
                "caption of a picture in a cell",
                Box::new(move |r| {
                    let cell = TableCell::new(
                        vec![para(Run::image(captioned(r), CharShapeIndex::new(0)))],
                        unit,
                    );
                    let table = Table::new(vec![TableRow::new(vec![cell])]);
                    Section::with_paragraphs(
                        vec![para(Run::table(table, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
            (
                "caption of a group picture",
                Box::new(move |r| {
                    let group = Control::Group {
                        children: vec![GroupChild::Picture(captioned(r))],
                        width: unit,
                        height: unit,
                        placement: None,
                        inst_id: None,
                    };
                    Section::with_paragraphs(
                        vec![para(Run::control(group, CharShapeIndex::new(0)))],
                        PageSettings::a4(),
                    )
                }),
            ),
        ];
        for (name, build) in &places {
            assert!(
                matches!(
                    validate_sections(&[build(51)]),
                    Err(ValidationError::InvalidCornerRatio { ratio: 51, .. })
                ),
                "{name}"
            );
            assert_eq!(validate_sections(&[build(50)]), Ok(()), "{name}");
        }
    }

    #[test]
    fn corner_ratio_50_accepted() {
        assert_eq!(validate_sections(&[section_with_rect_ratio(50)]), Ok(()));
    }

    // === Rule 1: At least 1 section ===

    #[test]
    fn empty_sections_rejected() {
        let result = validate_sections(&[]);
        assert_eq!(result, Err(ValidationError::EmptyDocument));
    }

    #[test]
    fn one_section_accepted() {
        let result = validate_sections(&[simple_section()]);
        assert!(result.is_ok());
    }

    // === Rule 2: Every section has at least 1 paragraph ===

    #[test]
    fn empty_section_rejected() {
        let sections = vec![Section::new(PageSettings::a4())];
        let result = validate_sections(&sections);
        assert_eq!(result, Err(ValidationError::EmptySection { section_index: 0 }));
    }

    #[test]
    fn second_empty_section_reports_index_1() {
        let sections = vec![simple_section(), Section::new(PageSettings::a4())];
        let result = validate_sections(&sections);
        assert_eq!(result, Err(ValidationError::EmptySection { section_index: 1 }));
    }

    // === Rule 3: Every paragraph has at least 1 run ===

    #[test]
    fn empty_paragraph_rejected() {
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::new(ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyParagraph { section_index: 0, paragraph_index: 0 })
        );
    }

    #[test]
    fn second_empty_paragraph_reports_correct_index() {
        let sections = vec![Section::with_paragraphs(
            vec![simple_paragraph(), Paragraph::new(ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyParagraph { section_index: 0, paragraph_index: 1 })
        );
    }

    // === Rule 4: Tables have at least 1 row ===

    #[test]
    fn empty_table_rejected() {
        let table_run = Run::table(Table::new(vec![]), CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTable { section_index: 0, paragraph_index: 0, run_index: 0 })
        );
    }

    // === Rule 4b: Table rows have at least 1 cell ===

    fn table_sections(table: Table) -> Vec<Section> {
        vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )]
    }

    #[test]
    fn fully_covered_empty_row_accepted() {
        // Native forms keep a row entry even when row spans from above cover
        // the entire row (the wire keeps an empty row element); validation
        // must accept the truthful representation instead of forcing
        // projections to invent phantom cells.
        let covered =
            TableCell::with_span(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap(), 1, 2);
        let table = Table::new(vec![TableRow::new(vec![covered]), TableRow::new(vec![])]);
        assert!(validate_sections(&table_sections(table)).is_ok());
    }

    #[test]
    fn uncovered_empty_row_still_rejected() {
        let table = Table::new(vec![TableRow::new(vec![simple_cell()]), TableRow::new(vec![])]);
        assert_eq!(
            validate_sections(&table_sections(table)),
            Err(ValidationError::EmptyTableRow {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 1,
            })
        );
    }

    #[test]
    fn empty_row_in_ragged_table_still_rejected() {
        // Grid derivation fails for ragged tables, so the covered-row
        // relaxation must not apply.
        let table = Table::new(vec![
            TableRow::new(vec![simple_cell(), simple_cell()]),
            TableRow::new(vec![simple_cell()]),
            TableRow::new(vec![]),
        ]);
        assert_eq!(
            validate_sections(&table_sections(table)),
            Err(ValidationError::EmptyTableRow {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 2,
            })
        );
    }

    #[test]
    fn empty_table_row_rejected() {
        let table = Table::new(vec![TableRow::new(vec![])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTableRow {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 0,
            })
        );
    }

    // === Rule 5: Table cells have at least 1 paragraph ===

    #[test]
    fn empty_table_cell_rejected() {
        let cell = TableCell::new(vec![], HwpUnit::from_mm(50.0).unwrap());
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::EmptyTableCell {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 0,
                cell_index: 0,
            })
        );
    }

    // === Rule 6: Spans >= 1 ===

    #[test]
    fn zero_col_span_rejected() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            0, // invalid
            1,
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidSpan { field: "col_span", .. })));
    }

    #[test]
    fn zero_row_span_rejected() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            1,
            0, // invalid
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidSpan { field: "row_span", .. })));
    }

    // === Rule 7: TextBox has paragraphs ===

    #[test]
    fn empty_text_box_rejected() {
        let ctrl = Control::TextBox {
            paragraphs: vec![],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            placement: None,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyTextBox { .. })));
    }

    // === Rule 8: Footnote has paragraphs ===

    #[test]
    fn empty_footnote_rejected() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyFootnote { .. })));
    }

    // === Happy paths ===

    #[test]
    fn valid_table_accepted() {
        let table = Table::new(vec![TableRow::new(vec![simple_cell()])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_text_box_accepted() {
        let ctrl = Control::TextBox {
            paragraphs: vec![simple_paragraph()],
            width: HwpUnit::from_mm(80.0).unwrap(),
            height: HwpUnit::from_mm(40.0).unwrap(),
            placement: None,
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_footnote_accepted() {
        let ctrl = Control::Footnote { inst_id: None, paragraphs: vec![simple_paragraph()] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn hyperlink_always_valid() {
        let ctrl =
            Control::Hyperlink { text: "link".to_string(), url: "https://example.com".to_string() };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn unknown_control_always_valid() {
        let ctrl = Control::Unknown { tag: "x".to_string(), data: None };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn multiple_valid_sections() {
        let sections = vec![simple_section(), simple_section(), simple_section()];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn large_span_values_accepted() {
        let cell = TableCell::with_span(
            vec![simple_paragraph()],
            HwpUnit::from_mm(50.0).unwrap(),
            100, // large but valid
            50,
        );
        let table = Table::new(vec![TableRow::new(vec![cell])]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn non_leading_table_header_row_rejected() {
        let header_cell = TableCell::new(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap());
        let data_cell = TableCell::new(vec![simple_paragraph()], HwpUnit::from_mm(50.0).unwrap());
        let table = Table::new(vec![
            TableRow::new(vec![data_cell]),
            TableRow::new(vec![header_cell]).with_header(true),
        ]);
        let table_run = Run::table(table, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![table_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];

        let result = validate_sections(&sections);
        assert_eq!(
            result,
            Err(ValidationError::NonLeadingTableHeaderRow {
                section_index: 0,
                paragraph_index: 0,
                run_index: 0,
                row_index: 1,
            })
        );
    }

    // === Rule 9: Endnote has paragraphs ===

    #[test]
    fn empty_endnote_rejected() {
        let ctrl = Control::Endnote { inst_id: None, paragraphs: vec![] };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyEndnote { .. })));
    }

    #[test]
    fn valid_endnote_accepted() {
        let ctrl = Control::Endnote {
            inst_id: Some(ObjectId::new(999)),
            paragraphs: vec![simple_paragraph()],
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Rule 10: Polygon has at least 3 vertices ===

    #[test]
    fn polygon_zero_vertices_rejected() {
        let ctrl = Control::Polygon {
            vertices: vec![],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidPolygon { vertex_count: 0, .. })));
    }

    #[test]
    fn polygon_two_vertices_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![ShapePoint { x: 0, y: 0 }, ShapePoint { x: 100, y: 100 }],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::InvalidPolygon { vertex_count: 2, .. })));
    }

    #[test]
    fn polygon_three_vertices_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Rule 11: Shape dimensions (Ellipse and Polygon only) ===

    #[test]
    fn ellipse_zero_width_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::new(0).unwrap(), // invalid
            height: HwpUnit::from_mm(30.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Ellipse", .. })
        ));
    }

    #[test]
    fn ellipse_zero_height_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::new(0).unwrap(), // invalid
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Ellipse", .. })
        ));
    }

    #[test]
    fn polygon_zero_width_rejected() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::new(0).unwrap(), // invalid
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Polygon", .. })
        ));
    }

    #[test]
    fn line_zero_height_accepted() {
        use crate::control::ShapePoint;
        // Lines can have zero height (horizontal line)
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 0 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::new(0).unwrap(), // valid for lines
            placement: None,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn line_zero_width_accepted() {
        use crate::control::ShapePoint;
        // Lines can have zero width (vertical line)
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 0, y: 1000 },
            width: HwpUnit::new(0).unwrap(), // valid for lines
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_line_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Line {
            start: ShapePoint { x: 0, y: 0 },
            end: ShapePoint { x: 1000, y: 500 },
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(25.0).unwrap(),
            placement: None,
            caption: None,
            style: None,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_ellipse_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Ellipse {
            center: ShapePoint { x: 500, y: 500 },
            axis1: ShapePoint { x: 1000, y: 500 },
            axis2: ShapePoint { x: 500, y: 1000 },
            width: HwpUnit::from_mm(40.0).unwrap(),
            height: HwpUnit::from_mm(30.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn valid_polygon_accepted() {
        use crate::control::ShapePoint;
        let ctrl = Control::Polygon {
            vertices: vec![
                ShapePoint { x: 0, y: 0 },
                ShapePoint { x: 1000, y: 0 },
                ShapePoint { x: 500, y: 1000 },
            ],
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(50.0).unwrap(),
            placement: None,
            paragraphs: vec![],
            caption: None,
            style: None,
            text_vertical_align: VerticalAlign::Top,
        };
        let ctrl_run = Run::control(ctrl, CharShapeIndex::new(0));
        let sections = vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(vec![ctrl_run], ParaShapeIndex::new(0))],
            PageSettings::a4(),
        )];
        assert!(validate_sections(&sections).is_ok());
    }

    // === Chart validation ===

    fn chart_ctrl(data: crate::chart::ChartData) -> Control {
        Control::Chart {
            chart_type: crate::chart::ChartType::Column,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        }
    }

    fn wrap_ctrl(ctrl: Control) -> Vec<Section> {
        vec![Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(ctrl, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        )]
    }

    #[test]
    fn chart_with_empty_data_rejected() {
        let data = crate::chart::ChartData::category(&["A"], &[]);
        let sections = wrap_ctrl(chart_ctrl(data));
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyChartData { .. })));
    }

    #[test]
    fn chart_zero_width_rejected() {
        let data = crate::chart::ChartData::category(&["A"], &[("S", &[1.0])]);
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Column,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::new(0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Chart", .. })
        ));
    }

    #[test]
    fn valid_chart_accepted() {
        let data = crate::chart::ChartData::category(&["A", "B"], &[("Sales", &[10.0, 20.0])]);
        let sections = wrap_ctrl(chart_ctrl(data));
        assert!(validate_sections(&sections).is_ok());
    }

    #[test]
    fn chart_empty_categories_rejected() {
        let data = crate::chart::ChartData::category(&[], &[("S", &[])]);
        let sections = wrap_ctrl(chart_ctrl(data));
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyCategoryLabels { .. })));
    }

    #[test]
    fn chart_xy_mismatched_lengths_rejected() {
        let data = crate::chart::ChartData::Xy {
            series: vec![crate::chart::XySeries {
                name: "Points".to_string(),
                x_values: vec![1.0, 2.0, 3.0],
                y_values: vec![10.0, 20.0], // mismatched!
            }],
        };
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Scatter,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::MismatchedSeriesLengths { .. })));
    }

    #[test]
    fn chart_xy_matching_lengths_accepted() {
        let data = crate::chart::ChartData::xy(&[("Pts", &[1.0, 2.0], &[3.0, 4.0])]);
        let ctrl = Control::Chart {
            chart_type: crate::chart::ChartType::Scatter,
            data,
            title: None,
            legend: crate::chart::LegendPosition::default(),
            grouping: crate::chart::ChartGrouping::default(),
            width: HwpUnit::from_mm(100.0).unwrap(),
            height: HwpUnit::from_mm(80.0).unwrap(),
            stock_variant: None,
            bar_shape: None,
            scatter_style: None,
            radar_style: None,
            of_pie_type: None,
            explosion: None,
            wireframe: None,
            bubble_3d: None,
            show_markers: None,
        };
        let sections = wrap_ctrl(ctrl);
        assert!(validate_sections(&sections).is_ok());
    }

    // === Equation validation ===

    #[test]
    fn equation_empty_script_rejected() {
        let ctrl = Control::Equation {
            script: String::new(),
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::from_mm(20.0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
            inst_id: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(result, Err(ValidationError::EmptyEquation { .. })));
    }

    #[test]
    fn equation_zero_height_rejected() {
        let ctrl = Control::Equation {
            script: "x^2".to_string(),
            width: HwpUnit::from_mm(50.0).unwrap(),
            height: HwpUnit::new(0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
            inst_id: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Equation", .. })
        ));
    }

    #[test]
    fn equation_zero_width_rejected() {
        let ctrl = Control::Equation {
            script: "x^2".to_string(),
            width: HwpUnit::new(0).unwrap(),
            height: HwpUnit::from_mm(20.0).unwrap(),
            base_line: 71,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".to_string(),
            inst_id: None,
        };
        let sections = wrap_ctrl(ctrl);
        let result = validate_sections(&sections);
        assert!(matches!(
            result,
            Err(ValidationError::InvalidShapeDimension { shape_type: "Equation", .. })
        ));
    }

    #[test]
    fn valid_equation_accepted() {
        let ctrl = Control::equation("{a+b} over {c+d}");
        let sections = wrap_ctrl(ctrl);
        assert!(validate_sections(&sections).is_ok());
    }

    // === Group children ===

    fn group_doc(children: Vec<GroupChild>) -> Vec<Section> {
        let w = HwpUnit::new(1000).unwrap();
        let group =
            Control::Group { children, width: w, height: w, placement: None, inst_id: None };
        let para = Paragraph::with_runs(
            vec![Run::control(group, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        );
        vec![Section::with_paragraphs(vec![para], PageSettings::a4())]
    }

    fn picture(path: &str, w: i32, h: i32) -> GroupChild {
        GroupChild::Picture(crate::image::Image::new(
            path,
            HwpUnit::new(w).unwrap(),
            HwpUnit::new(h).unwrap(),
            crate::image::ImageFormat::Png,
        ))
    }

    #[test]
    fn group_picture_and_shape_children_are_accepted() {
        let w = HwpUnit::new(1000).unwrap();
        let children = vec![
            picture("BinData/image1.png", 10, 10),
            GroupChild::Shape(Control::rect(w, w).unwrap()),
        ];
        assert_eq!(validate_sections(&group_doc(children)), Ok(()));
    }

    /// 이것을 실패시키는 것: Picture 의 빈 경로 검사 삭제.
    #[test]
    fn group_picture_without_a_path_is_rejected() {
        assert!(matches!(
            validate_sections(&group_doc(vec![picture("", 10, 10)])),
            Err(ValidationError::InvalidGroupChild { .. })
        ));
    }

    /// 이것을 실패시키는 것: 크기 검사 삭제, 또는 `<= 0` 을 `== 0` 으로(음수 통과).
    #[test]
    fn group_picture_without_a_size_is_rejected() {
        for (w, h) in [(0, 10), (10, 0), (-5, 10)] {
            assert!(
                matches!(
                    validate_sections(&group_doc(vec![picture("a.png", w, h)])),
                    Err(ValidationError::InvalidShapeDimension { shape_type: "Picture", .. })
                ),
                "{w}x{h}"
            );
        }
    }

    /// 이것을 실패시키는 것: Shape 갈래의 `is_group_child_allowed` 검사 삭제.
    #[test]
    fn group_shape_child_that_is_not_a_drawing_is_rejected() {
        let link = Control::Hyperlink { text: "a".into(), url: "https://a.kr".into() };
        assert!(matches!(
            validate_sections(&group_doc(vec![GroupChild::Shape(link)])),
            Err(ValidationError::InvalidGroupChild { .. })
        ));
    }
}
