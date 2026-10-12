//! HWP5 layout-hint capture.
//!
//! HWP5 paragraphs carry a `ParaLineSeg` (tag `0x45`) layout cache and tables
//! carry a derivable row-height hint. These are format-local rendering caches —
//! they must not leak into Core — but HWP5 → HWPX conversion preserves them as
//! fidelity hints so Hancom Office can open the converted file without flagging
//! a "low-security recovery".
//!
//! This module captures those hints from the decoded HWP5 section tree into a
//! neutral [`SectionLayoutHints`] value. Applying the captured hints onto HWPX
//! bytes is an HWPX concern and lives in the `hwpforge-convert` orchestrator.

use std::collections::VecDeque;

use crate::decoder::section::{Hwp5Control, Hwp5Paragraph, Hwp5Table, SectionResult};
use crate::schema::section::Hwp5ParaLineSeg;

/// Per-section captured layout hints (paragraph line segments + table heights).
///
/// Both queues hold the body's hints, then the headers', then the footers'
/// (each scope in the order the HWPX encoder emits it). The encoder writes
/// a header/footer into the paragraph it is anchored to, so in the XML its
/// elements interleave with the body's; a consumer splits each queue by the
/// number of elements of each scope it finds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SectionLayoutHints {
    /// Paragraph layout hints (body, then headers, then footers).
    pub paragraphs: VecDeque<ParagraphLayoutHint>,
    /// Table layout hints (body, then headers, then footers).
    pub tables: VecDeque<TableLayoutHint>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ScopeLayoutHints {
    paragraphs: VecDeque<ParagraphLayoutHint>,
    tables: VecDeque<TableLayoutHint>,
}

/// Captured line-segment layout cache for a single paragraph.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParagraphLayoutHint {
    /// Line segments parsed from the HWP5 `ParaLineSeg` record (may be empty
    /// when the source paragraph carried no such record).
    pub line_segments: Vec<Hwp5ParaLineSeg>,
}

/// Captured derived height hint for a single table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableLayoutHint {
    /// Derived total table height in HWPUNIT, or `None` when it cannot be
    /// reliably computed (for example, presence of row spans).
    pub height: Option<i32>,
}

/// Captures per-section layout hints from the decoded HWP5 section tree.
///
/// Crate-internal: the captured hints reach output-format orchestrators through
/// the `Hwp5Decoded::layout_hints` bundle rather than this entry point.
pub(crate) fn capture_layout_hints(sections: &[SectionResult]) -> Vec<SectionLayoutHints> {
    sections.iter().map(collect_section_layout_hints).collect()
}

impl SectionLayoutHints {
    /// Whether this section carries any actionable layout hint.
    pub fn has_payload(&self) -> bool {
        self.paragraphs.iter().any(|hint| !hint.line_segments.is_empty()) || !self.tables.is_empty()
    }
}

fn collect_section_layout_hints(section: &SectionResult) -> SectionLayoutHints {
    let mut body = ScopeLayoutHints::default();
    let mut header = ScopeLayoutHints::default();
    let mut footer = ScopeLayoutHints::default();

    for paragraph in &section.paragraphs {
        collect_flow_paragraph_layout_hints(paragraph, &mut body, &mut header, &mut footer);
    }

    body.append(&mut header);
    body.append(&mut footer);
    SectionLayoutHints { paragraphs: body.paragraphs, tables: body.tables }
}

impl ScopeLayoutHints {
    fn push_paragraph(&mut self, paragraph: &Hwp5Paragraph) {
        self.paragraphs
            .push_back(ParagraphLayoutHint { line_segments: paragraph.line_segments.clone() });
    }

    fn push_table(&mut self, table: &Hwp5Table) {
        self.tables.push_back(TableLayoutHint { height: derive_table_height(table) });
    }

    fn append(&mut self, other: &mut Self) {
        self.paragraphs.append(&mut other.paragraphs);
        self.tables.append(&mut other.tables);
    }
}

fn collect_flow_paragraph_layout_hints(
    paragraph: &Hwp5Paragraph,
    body: &mut ScopeLayoutHints,
    header: &mut ScopeLayoutHints,
    footer: &mut ScopeLayoutHints,
) {
    body.push_paragraph(paragraph);

    for control in &paragraph.controls {
        match control {
            Hwp5Control::Table(table) => {
                collect_flow_table_layout_hints(table, body, header, footer)
            }
            Hwp5Control::Header(subtree) => collect_scope_paragraphs(&subtree.paragraphs, header),
            Hwp5Control::Footer(subtree) => collect_scope_paragraphs(&subtree.paragraphs, footer),
            Hwp5Control::Footnote(subtree) | Hwp5Control::Endnote(subtree) => {
                collect_scope_paragraphs(&subtree.paragraphs, body);
            }
            Hwp5Control::TextBox(textbox) => collect_scope_paragraphs(&textbox.paragraphs, body),
            // 이것을 실패시키는 것: 이 갈래를 지우기(타원 글 hint 가 빠져 캐시 켠
            // 변환이 hint 부족으로 실패 — `ellipse_and_polygon_keep_their_text`).
            Hwp5Control::Ellipse(_) | Hwp5Control::Polygon(_) => {
                collect_scope_paragraphs(crate::projection::written_shape_text(control), body);
            }
            // Memo body paragraphs are emitted as `<hp:subList>` children of
            // the body paragraph in HWPX, so the patcher consumes layout
            // hints for them from the body scope — same pattern as
            // Footnote/Endnote.
            Hwp5Control::Memo(memo) => collect_scope_paragraphs(&memo.paragraphs, body),
            // Group children with text (rect/ellipse drawText) carry their
            // own paragraphs that need linesegarray hints, mirroring TextBox.
            Hwp5Control::Group(group) => collect_group_child_layout_hints(group, body),
            Hwp5Control::Image(_)
            | Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Arc(_)
            | Hwp5Control::Curve(_)
            | Hwp5Control::TextArt(_)
            | Hwp5Control::ConnectLine(_)
            | Hwp5Control::Equation(_)
            | Hwp5Control::Dutmal(_)
            | Hwp5Control::Compose(_)
            | Hwp5Control::IndexMark(_)
            | Hwp5Control::ClickHere(_)
            | Hwp5Control::SummaryField(_)
            | Hwp5Control::DateCodeField(_)
            | Hwp5Control::PathField(_)
            | Hwp5Control::CrossRef(_)
            | Hwp5Control::InlinePageNumber(_)
            | Hwp5Control::NewNumber(_)
            | Hwp5Control::PageHiding(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Unknown { .. } => {}
        }
    }
}

/// A body table: its cells are flow paragraphs too, so a header/footer in a
/// cell (lifted into the section by projection, §19.7 F) leaves its hints
/// in the header/footer scope rather than the body.
///
/// 이것을 실패시키는 것: 칸 문단을 `collect_scope_paragraphs(.., body)` 로
/// 되돌리기 — 칸 안 꼬리말 문단의 hint 가 사라져 문단 hint 수가 모자란다
/// (`cell_footer_converts_with_and_without_the_layout_cache`).
fn collect_flow_table_layout_hints(
    table: &Hwp5Table,
    body: &mut ScopeLayoutHints,
    header: &mut ScopeLayoutHints,
    footer: &mut ScopeLayoutHints,
) {
    body.push_table(table);
    for cell in &table.cells {
        for paragraph in &cell.paragraphs {
            collect_flow_paragraph_layout_hints(paragraph, body, header, footer);
        }
    }
}

/// Collects layout hints from the group children whose text is written:
/// the group projection's own plan
/// ([`written_group_text`](crate::projection::written_group_text)) decides,
/// so a child whose text is dropped (a polygon's, an arc's), a child the
/// fold drops (an axis collapsed, out of range) and every child of a nested
/// group that is dropped leave no hint behind — a leftover hint fails the
/// whole conversion ("left unconsumed hints").
fn collect_group_child_layout_hints(
    group: &crate::decoder::section::Hwp5GroupControl,
    scope: &mut ScopeLayoutHints,
) {
    for paragraphs in crate::projection::written_group_text(group) {
        collect_scope_paragraphs(paragraphs, scope);
    }
}

fn collect_scope_paragraphs(paragraphs: &[Hwp5Paragraph], scope: &mut ScopeLayoutHints) {
    for paragraph in paragraphs {
        collect_scope_paragraph_layout_hints(paragraph, scope);
    }
}

fn collect_scope_paragraph_layout_hints(paragraph: &Hwp5Paragraph, scope: &mut ScopeLayoutHints) {
    scope.push_paragraph(paragraph);

    for control in &paragraph.controls {
        match control {
            Hwp5Control::Table(table) => collect_table_layout_hints(table, scope),
            Hwp5Control::TextBox(textbox) => collect_scope_paragraphs(&textbox.paragraphs, scope),
            Hwp5Control::Ellipse(_) | Hwp5Control::Polygon(_) => {
                collect_scope_paragraphs(crate::projection::written_shape_text(control), scope);
            }
            Hwp5Control::Footnote(subtree) | Hwp5Control::Endnote(subtree) => {
                collect_scope_paragraphs(&subtree.paragraphs, scope);
            }
            Hwp5Control::Memo(memo) => collect_scope_paragraphs(&memo.paragraphs, scope),
            Hwp5Control::Group(group) => collect_group_child_layout_hints(group, scope),
            Hwp5Control::Header(_)
            | Hwp5Control::Footer(_)
            | Hwp5Control::Image(_)
            | Hwp5Control::Line(_)
            | Hwp5Control::Rect(_)
            | Hwp5Control::Arc(_)
            | Hwp5Control::Curve(_)
            | Hwp5Control::TextArt(_)
            | Hwp5Control::ConnectLine(_)
            | Hwp5Control::Equation(_)
            | Hwp5Control::Dutmal(_)
            | Hwp5Control::Compose(_)
            | Hwp5Control::IndexMark(_)
            | Hwp5Control::ClickHere(_)
            | Hwp5Control::SummaryField(_)
            | Hwp5Control::DateCodeField(_)
            | Hwp5Control::PathField(_)
            | Hwp5Control::CrossRef(_)
            | Hwp5Control::InlinePageNumber(_)
            | Hwp5Control::NewNumber(_)
            | Hwp5Control::PageHiding(_)
            | Hwp5Control::OleObject(_)
            | Hwp5Control::Unknown { .. } => {}
        }
    }
}

fn collect_table_layout_hints(table: &Hwp5Table, scope: &mut ScopeLayoutHints) {
    scope.push_table(table);
    for cell in &table.cells {
        collect_scope_paragraphs(&cell.paragraphs, scope);
    }
}

/// The table's `sz height`: the height Hancom saved in the table's own
/// `CtrlHeader` (Hancom twins: 5,130 of 5,131 tables), else the old
/// derivation from row heights.
///
/// 이것을 실패시키는 것: `CtrlHeader` 높이를 건너뛰기
/// (`table_height_hint_is_the_ctrl_header_height`).
fn derive_table_height(table: &Hwp5Table) -> Option<i32> {
    if let Some(height) = table
        .ctrl
        .as_ref()
        .and_then(|ctrl| i32::try_from(ctrl.geometry.height).ok())
        .filter(|&h| h > 0)
    {
        return Some(height);
    }
    if table.rows == 0 || table.cells.iter().any(|cell| cell.row_span != 1) {
        return None;
    }

    let mut row_heights = vec![0i32; table.rows as usize];
    for cell in &table.cells {
        let row = usize::from(cell.row);
        if row >= row_heights.len() {
            continue;
        }
        let content_height = cell.paragraphs.iter().map(paragraph_layout_height).max().unwrap_or(0);
        let margin_height =
            i32::from(cell.margin.top).saturating_add(i32::from(cell.margin.bottom));
        let hinted_height = content_height.saturating_add(margin_height);
        row_heights[row] = row_heights[row].max(hinted_height.max(cell.height.max(0)));
    }

    let cell_spacing = i32::from(table.cell_spacing.max(0));
    let total_height = row_heights
        .into_iter()
        .sum::<i32>()
        .saturating_add(cell_spacing.saturating_mul(i32::from(table.rows.saturating_sub(1))));
    (total_height > 0).then_some(total_height)
}

fn paragraph_layout_height(paragraph: &Hwp5Paragraph) -> i32 {
    paragraph
        .line_segments
        .iter()
        .map(|segment| segment.vertical_position.saturating_add(segment.line_height))
        .max()
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::decoder::section::Hwp5NestedSubtree;

    fn paragraph(
        text: &str,
        line_segments: Vec<Hwp5ParaLineSeg>,
        controls: Vec<Hwp5Control>,
    ) -> Hwp5Paragraph {
        Hwp5Paragraph {
            end_char_shape: None,
            silent_wires: Vec::new(),
            page_break: false,
            column_break: false,
            text: text.into(),
            text_segments: Vec::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: Vec::new(),
            line_segments,
            controls,
        }
    }

    fn line_segment(
        text_start_position: u32,
        vertical_position: i32,
        line_height: i32,
    ) -> Hwp5ParaLineSeg {
        Hwp5ParaLineSeg {
            text_start_position,
            vertical_position,
            line_height,
            text_height: 1000,
            baseline_distance: 850,
            line_spacing: 600,
            column_start_position: 0,
            segment_width: 20272,
            tag: 393216,
        }
    }

    /// A-T4: the table height hint is the table `CtrlHeader` height (법무부
    /// 본문 표 43643, not the cell-height derived 102508).
    /// 이것을 실패시키는 것: `CtrlHeader` 높이를 건너뛰기.
    #[test]
    fn table_height_hint_is_the_ctrl_header_height() {
        let mut table = Hwp5Table {
            inner_margin: None,
            no_adjust: false,
            rows: 0,
            cols: 0,
            page_break: crate::decoder::section::Hwp5TablePageBreak::Cell,
            repeat_header: false,
            cell_spacing: 0,
            border_fill_id: None,
            cells: vec![],
            instance_id: 0,
            ctrl: None,
        };
        assert_eq!(derive_table_height(&table), None);
        table.ctrl = Some(crate::decoder::section::Hwp5TableCtrl {
            outer_margin: None,
            properties: 0,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                outer_margin: [0; 4],
                x: 0,
                y: 0,
                width: 50_737,
                height: 43_643,
            },
        });
        assert_eq!(derive_table_height(&table), Some(43_643));
    }

    #[test]
    fn derive_table_height_uses_line_segments_and_cell_margin() {
        let table = Hwp5Table {
            inner_margin: None,
            no_adjust: false,
            rows: 1,
            cols: 1,
            page_break: crate::decoder::section::Hwp5TablePageBreak::Cell,
            repeat_header: false,
            cell_spacing: 0,
            border_fill_id: None,
            cells: vec![crate::decoder::section::Hwp5TableCell {
                has_margin: true,
                line_wrap: 0,
                column: 0,
                row: 0,
                col_span: 1,
                row_span: 1,
                width: 1000,
                height: 0,
                margin: crate::decoder::section::Hwp5TableCellMargin {
                    left: 0,
                    right: 0,
                    top: 141,
                    bottom: 141,
                },
                vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                is_header: false,
                border_fill_id: None,
                paragraphs: vec![Hwp5Paragraph {
                    end_char_shape: None,
                    silent_wires: Vec::new(),
                    page_break: false,
                    column_break: false,
                    text: "cell".into(),
                    text_segments: Vec::new(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    line_segments: vec![
                        line_segment(0, 0, 1000),
                        line_segment(20, 1600, 1000),
                        line_segment(48, 3200, 1000),
                    ],
                    controls: Vec::new(),
                }],
            }],
            instance_id: 0,
            ctrl: None,
        };

        assert_eq!(derive_table_height(&table), Some(4482));
    }

    /// The group projection keeps the text of a rect child and drops that of
    /// a polygon child (reported lost); only the kept text's paragraph gets
    /// a hint, else the patcher is left with an unconsumed one.
    ///
    /// 이것을 실패시키는 것: 글자를 버리는 자식(다각형)의 문단도 힌트로 모으는 것,
    /// 행렬열을 못 읽어 버리는 자식의 문단을 모으는 것.
    #[test]
    fn group_hints_follow_the_text_the_projection_keeps() {
        use crate::decoder::section::{
            Hwp5GroupChild, Hwp5GroupControl, Hwp5PolygonControl, Hwp5RectControl,
        };
        use crate::schema::section::{Hwp5ShapeComponentGeometry, Hwp5ShapeFill, Hwp5ShapeRender};
        let geo =
            Hwp5ShapeComponentGeometry { x: 0, y: 0, width: 0, height: 0, outer_margin: [0; 4] };
        let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let render = Ok(Hwp5ShapeRender {
            initial: (100, 100),
            trans: id,
            pairs: vec![(id, id)],
            fill: Hwp5ShapeFill::Plain,
        });
        let child = |control, render, start| Hwp5GroupChild {
            control,
            paragraphs: vec![paragraph("t", vec![line_segment(start, 0, 1000)], Vec::new())],
            list_header_properties: None,
            text_margin: None,
            render,
            nested_controls: Vec::new(),
        };
        let rect = || {
            Hwp5Control::Rect(Hwp5RectControl {
                original_size: None,
                look: None,
                ctrl_id: 0,
                geometry: geo.clone(),
                ctrl_properties: 0,
            })
        };
        let group = Hwp5GroupControl {
            ctrl_id: 0,
            geometry: geo.clone(),
            children: vec![
                child(rect(), render.clone(), 1),
                child(
                    Hwp5Control::Polygon(Hwp5PolygonControl {
                        original_size: None,
                        look: None,
                        ctrl_id: 0,
                        paragraphs: Vec::new(),
                        list_header_properties: None,
                        text_margin: None,
                        geometry: geo.clone(),
                        points: Vec::new(),
                        ctrl_properties: 0,
                    }),
                    render.clone(),
                    2,
                ),
                child(rect(), Err("cut short".into()), 3),
            ],
            render,
            instance_id: 0,
            has_link: false,
            ctrl_properties: 0,
        };
        let mut scope = ScopeLayoutHints::default();
        collect_group_child_layout_hints(&group, &mut scope);
        let starts: Vec<u32> =
            scope.paragraphs.iter().map(|h| h.line_segments[0].text_start_position).collect();
        assert_eq!(starts, [1]);
    }

    /// A rect with text that the fold drops (its x scale is 0) and a nested
    /// group dropped because its only child collapses leave no hint; the
    /// kept rect's text does.
    ///
    /// 이것을 실패시키는 것: 힌트를 투영 계획이 아니라 자식 종류·행렬열 읽힘만으로
    /// 고르는 것(접기에서 버려진 글상자의 힌트가 남는다).
    #[test]
    fn group_hints_skip_text_the_fold_drops() {
        use crate::decoder::section::{Hwp5GroupChild, Hwp5GroupControl, Hwp5RectControl};
        use crate::schema::section::{Hwp5ShapeComponentGeometry, Hwp5ShapeFill, Hwp5ShapeRender};
        let geo =
            Hwp5ShapeComponentGeometry { x: 0, y: 0, width: 0, height: 0, outer_margin: [0; 4] };
        let id = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let flat = [0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let render = |sca| {
            Ok(Hwp5ShapeRender {
                initial: (100, 100),
                trans: id,
                pairs: vec![(sca, id)],
                fill: Hwp5ShapeFill::Plain,
            })
        };
        let rect = |sca, start| Hwp5GroupChild {
            control: Hwp5Control::Rect(Hwp5RectControl {
                original_size: None,
                look: None,
                ctrl_id: 0,
                geometry: geo.clone(),
                ctrl_properties: 0,
            }),
            paragraphs: vec![paragraph("t", vec![line_segment(start, 0, 1000)], Vec::new())],
            list_header_properties: None,
            text_margin: None,
            render: render(sca),
            nested_controls: Vec::new(),
        };
        let nested = Hwp5GroupChild {
            control: Hwp5Control::Group(Hwp5GroupControl {
                ctrl_id: 0,
                geometry: geo.clone(),
                children: vec![rect(flat, 3)],
                render: render(id),
                instance_id: 0,
                has_link: false,
                ctrl_properties: 0,
            }),
            paragraphs: Vec::new(),
            list_header_properties: None,
            text_margin: None,
            render: render(id),
            nested_controls: Vec::new(),
        };
        let group = Hwp5GroupControl {
            ctrl_id: 0,
            geometry: geo.clone(),
            children: vec![rect(id, 1), rect(flat, 2), nested],
            render: render(id),
            instance_id: 0,
            has_link: false,
            ctrl_properties: 0,
        };
        let mut scope = ScopeLayoutHints::default();
        collect_group_child_layout_hints(&group, &mut scope);
        let starts: Vec<u32> =
            scope.paragraphs.iter().map(|h| h.line_segments[0].text_start_position).collect();
        assert_eq!(starts, [1]);
    }

    #[test]
    fn collect_section_layout_hints_orders_body_before_header_and_footer() {
        let section = SectionResult {
            paragraphs: vec![
                paragraph(
                    "body-a",
                    vec![line_segment(10, 0, 1000)],
                    vec![Hwp5Control::Header(Hwp5NestedSubtree {
                        ctrl_id: 0x6865_6164,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![paragraph(
                            "header",
                            vec![line_segment(30, 0, 1000)],
                            Vec::new(),
                        )],
                    })],
                ),
                paragraph(
                    "body-b",
                    vec![line_segment(20, 0, 1000)],
                    vec![Hwp5Control::Footer(Hwp5NestedSubtree {
                        ctrl_id: 0x666F_6F74,
                        properties_raw: 0,
                        instance_id: 0,
                        paragraphs: vec![paragraph(
                            "footer",
                            vec![line_segment(40, 0, 1000)],
                            Vec::new(),
                        )],
                    })],
                ),
            ],
            page_def: None,
            section_def_properties: None,
            section_def_start_numbers: None,
            page_border_fills: Vec::new(),
            column_def: None,
            warnings: Vec::new(),
        };

        let hints = collect_section_layout_hints(&section);
        let order = hints
            .paragraphs
            .iter()
            .map(|hint| hint.line_segments[0].text_start_position)
            .collect::<Vec<_>>();

        assert_eq!(order, vec![10, 20, 30, 40]);
    }
}
