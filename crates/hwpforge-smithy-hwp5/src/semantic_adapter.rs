//! Decoder-output to semantic-IR adapter for the first HWP5 semantic slice.
//!
//! The adapter intentionally stays narrow:
//! - package metadata
//! - DocInfo references
//! - body paragraphs
//! - `table/tc/subList`, `header/subList`, `footer/subList`, `rect/drawText/subList`
//! - explicit image controls backed by `BinData`
//! - unknown control preservation
//!
//! It does **not** re-parse raw records or replace the existing Core
//! projection path.

use std::collections::{BTreeMap, BTreeSet};

use crate::decoder::header::DocInfoResult;
use crate::decoder::section::{
    Hwp5Control, Hwp5ImageControl, Hwp5LineControl, Hwp5NestedSubtree, Hwp5OleObjectControl,
    Hwp5Paragraph, Hwp5PolygonControl, Hwp5RectControl, Hwp5Table, Hwp5TextBoxControl,
    SectionResult,
};
use crate::decoder::DecodedHwp5Intermediate;
use crate::semantic::{
    Hwp5SemanticConfidence, Hwp5SemanticContainerKind, Hwp5SemanticContainerPath,
    Hwp5SemanticControlId, Hwp5SemanticControlKind, Hwp5SemanticControlNode,
    Hwp5SemanticControlPayload, Hwp5SemanticDocInfo, Hwp5SemanticDocument,
    Hwp5SemanticImagePayload, Hwp5SemanticInlineItem, Hwp5SemanticLinePayload,
    Hwp5SemanticNamedStyleRef, Hwp5SemanticOlePayload, Hwp5SemanticPackageMeta,
    Hwp5SemanticPageDefSummary, Hwp5SemanticParagraph, Hwp5SemanticParagraphId,
    Hwp5SemanticPolygonPayload, Hwp5SemanticRectPayload, Hwp5SemanticSection,
    Hwp5SemanticSectionId, Hwp5SemanticShapePoint, Hwp5SemanticTableCellEvidence,
    Hwp5SemanticTableCellMargin, Hwp5SemanticTablePayload,
};
use crate::table_cell_vertical_align::semantic_table_cell_vertical_align;
use crate::table_page_break::semantic_table_page_break;
use crate::{Hwp5BinDataRecordSummary, Hwp5BinDataStream, Hwp5JoinedImageAssetPlan};

#[derive(Debug, Default)]
struct SemanticIdAlloc {
    next_section: usize,
    next_paragraph: usize,
    next_control: usize,
}

impl SemanticIdAlloc {
    fn next_section_id(&mut self) -> Hwp5SemanticSectionId {
        let id = Hwp5SemanticSectionId::new(self.next_section);
        self.next_section += 1;
        id
    }

    fn next_paragraph_id(&mut self) -> Hwp5SemanticParagraphId {
        let id = Hwp5SemanticParagraphId::new(self.next_paragraph);
        self.next_paragraph += 1;
        id
    }

    fn next_control_id(&mut self) -> Hwp5SemanticControlId {
        let id = Hwp5SemanticControlId::new(self.next_control);
        self.next_control += 1;
        id
    }
}

#[derive(Debug, Default)]
struct SectionBuildState {
    paragraphs: Vec<Hwp5SemanticParagraph>,
    controls: Vec<Hwp5SemanticControlNode>,
    next_paragraph_index: usize,
}

#[derive(Debug, Clone)]
struct NestedSubtreeSemanticSpec {
    kind: Hwp5SemanticControlKind,
    subtree_container: Hwp5SemanticContainerKind,
}

#[derive(Debug, Clone, Copy)]
struct SemanticSupport<'a> {
    bin_data_records: &'a [Hwp5BinDataRecordSummary],
    bin_data_streams: &'a [Hwp5BinDataStream],
    image_assets: &'a Hwp5JoinedImageAssetPlan,
}

pub(crate) fn adapt_to_semantic(
    decoded: &DecodedHwp5Intermediate,
    image_assets: &Hwp5JoinedImageAssetPlan,
) -> Hwp5SemanticDocument {
    let mut ids = SemanticIdAlloc::default();
    let support = SemanticSupport {
        bin_data_records: &decoded.bin_data_records,
        bin_data_streams: &decoded.bin_data_streams,
        image_assets,
    };
    let mut document = Hwp5SemanticDocument::new(adapt_package_meta(decoded));
    document.doc_info = adapt_doc_info(&decoded.doc_info);
    document.sections = decoded
        .sections
        .iter()
        .enumerate()
        .map(|(index, section)| adapt_section(index, section, support, &mut ids))
        .collect();
    document
}

fn adapt_package_meta(decoded: &DecodedHwp5Intermediate) -> Hwp5SemanticPackageMeta {
    Hwp5SemanticPackageMeta {
        version: decoded.version.clone(),
        compressed: decoded.compressed,
        package_entries: decoded.package_entries.clone(),
        bin_data_records: decoded.bin_data_records.clone(),
        bin_data_streams: decoded.bin_data_streams.clone(),
    }
}

fn adapt_doc_info(doc_info: &DocInfoResult) -> Hwp5SemanticDocInfo {
    Hwp5SemanticDocInfo {
        font_faces: doc_info.fonts.iter().map(|font| font.face_name.clone()).collect(),
        named_styles: doc_info
            .styles
            .iter()
            .enumerate()
            .filter_map(|(index, style)| {
                let style_id = u16::try_from(index).ok()?;
                Some(Hwp5SemanticNamedStyleRef {
                    style_id: Some(style_id),
                    name: style.name.clone(),
                    para_shape_id: Some(style.para_shape_id),
                    char_shape_id: Some(style.char_shape_id),
                })
            })
            .collect(),
        char_shape_ids: (0..doc_info.char_shapes.len())
            .filter_map(|index| u16::try_from(index).ok())
            .collect(),
        para_shape_ids: (0..doc_info.para_shapes.len())
            .filter_map(|index| u16::try_from(index).ok())
            .collect(),
    }
}

fn adapt_section(
    index: usize,
    section: &SectionResult,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticSection {
    let section_id = ids.next_section_id();
    let mut build: SectionBuildState = SectionBuildState::default();

    for paragraph in &section.paragraphs {
        let _ = adapt_paragraph(
            paragraph,
            Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::Body]),
            None,
            &mut build,
            support,
            ids,
        );
    }

    Hwp5SemanticSection {
        section_id,
        index,
        page_def: section.page_def.as_ref().map(adapt_page_def),
        paragraphs: build.paragraphs,
        controls: build.controls,
    }
}

fn adapt_paragraph(
    paragraph: &Hwp5Paragraph,
    container: Hwp5SemanticContainerPath,
    owner_control_id: Option<Hwp5SemanticControlId>,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticParagraphId {
    let paragraph_id = ids.next_paragraph_id();
    let paragraph_index = build.next_paragraph_index;
    build.next_paragraph_index += 1;

    let paragraph_slot_index = build.paragraphs.len();
    build.paragraphs.push(Hwp5SemanticParagraph {
        paragraph_id,
        paragraph_index,
        container: container.clone(),
        owner_control_id,
        inline_items: Vec::new(),
        text: String::new(),
        style_id: None,
        char_shape_run_count: 0,
        control_ids: Vec::new(),
    });

    let mut inline_items: Vec<Hwp5SemanticInlineItem> = Vec::new();
    let mut control_ids: Vec<Hwp5SemanticControlId> = Vec::new();
    for control in &paragraph.controls {
        let control_id = adapt_control(control, &container, paragraph_id, build, support, ids);
        control_ids.push(control_id);
    }

    populate_inline_items(&paragraph.text, &control_ids, &mut inline_items);

    build.paragraphs[paragraph_slot_index] = Hwp5SemanticParagraph {
        paragraph_id,
        paragraph_index,
        container,
        owner_control_id,
        inline_items,
        text: paragraph.text.clone(),
        style_id: Some(u16::from(paragraph.style_id)),
        char_shape_run_count: paragraph.char_shape_runs.len(),
        control_ids,
    };

    paragraph_id
}

fn adapt_control(
    control: &Hwp5Control,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    match control {
        Hwp5Control::Table(table) => {
            adapt_table_control(table, container, paragraph_id, build, support, ids)
        }
        Hwp5Control::Image(image) => {
            adapt_image_control(image, container, paragraph_id, build, ids, support.image_assets)
        }
        Hwp5Control::Line(line) => adapt_line_control(line, container, paragraph_id, build, ids),
        Hwp5Control::Rect(rect) => adapt_rect_control(rect, container, paragraph_id, build, ids),
        Hwp5Control::Polygon(polygon) => {
            adapt_polygon_control(polygon, container, paragraph_id, build, ids)
        }
        Hwp5Control::OleObject(ole) => {
            adapt_ole_object_control(ole, container, paragraph_id, build, support, ids)
        }
        Hwp5Control::Header(subtree) => adapt_nested_subtree_control(
            subtree,
            container,
            paragraph_id,
            NestedSubtreeSemanticSpec {
                kind: Hwp5SemanticControlKind::Header,
                subtree_container: Hwp5SemanticContainerKind::HeaderSubList,
            },
            build,
            support,
            ids,
        ),
        Hwp5Control::Footer(subtree) => adapt_nested_subtree_control(
            subtree,
            container,
            paragraph_id,
            NestedSubtreeSemanticSpec {
                kind: Hwp5SemanticControlKind::Footer,
                subtree_container: Hwp5SemanticContainerKind::FooterSubList,
            },
            build,
            support,
            ids,
        ),
        Hwp5Control::TextBox(textbox) => {
            adapt_textbox_control(textbox, container, paragraph_id, build, support, ids)
        }
        Hwp5Control::Unknown { ctrl_id } => {
            let node_id = ids.next_control_id();
            let literal = crate::ctrl_id_ascii(*ctrl_id);
            build.controls.push(Hwp5SemanticControlNode {
                node_id,
                kind: Hwp5SemanticControlKind::Unknown(literal.clone()),
                payload: Hwp5SemanticControlPayload::None,
                container: container.clone(),
                literal_ctrl_id: Some(literal),
                anchor_paragraph_id: Some(paragraph_id),
                confidence: Hwp5SemanticConfidence::High,
                notes: vec![format!("raw_ctrl_id=0x{ctrl_id:08X}")],
            });
            node_id
        }
    }
}

fn adapt_ole_object_control(
    ole: &Hwp5OleObjectControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(ole.ctrl_id);
    let joined_record: Option<&Hwp5BinDataRecordSummary> =
        support.bin_data_records.iter().find(|record| record.binary_data_id == ole.binary_data_id);
    let joined_stream_name: Option<&str> = joined_record.and_then(|record| {
        support
            .bin_data_streams
            .iter()
            .find(|stream| stream.name == record.storage_name)
            .map(|stream| stream.name.as_str())
    });

    let (payload, confidence, extra_notes) =
        if let (Some(record), Some(stream_name)) = (joined_record, joined_stream_name) {
            (
                Hwp5SemanticControlPayload::OleObject(Hwp5SemanticOlePayload {
                    binary_data_id: ole.binary_data_id,
                    storage_name: stream_name.to_string(),
                    package_path: format!("BinData/{stream_name}"),
                    extent_width_hwp: Some(ole.extent_width),
                    extent_height_hwp: Some(ole.extent_height),
                }),
                Hwp5SemanticConfidence::High,
                vec![
                    format!("storage_name={stream_name}"),
                    format!("storage_extension={}", record.extension),
                ],
            )
        } else if let Some(record) = joined_record {
            (
                Hwp5SemanticControlPayload::None,
                Hwp5SemanticConfidence::Medium,
                vec![
                    format!("storage_name={}", record.storage_name),
                    format!("storage_extension={}", record.extension),
                    "bin_data_stream_missing".to_string(),
                ],
            )
        } else {
            (
                Hwp5SemanticControlPayload::None,
                Hwp5SemanticConfidence::Medium,
                vec!["ole bin data record missing".to_string()],
            )
        };

    let mut notes: Vec<String> = vec![
        format!("raw_ctrl_id=0x{:08X}", ole.ctrl_id),
        format!("binary_data_id={}", ole.binary_data_id),
        format!("geometry_x={}", ole.geometry.x),
        format!("geometry_y={}", ole.geometry.y),
        format!("geometry_width={}", ole.geometry.width),
        format!("geometry_height={}", ole.geometry.height),
        format!("extent_width={}", ole.extent_width),
        format!("extent_height={}", ole.extent_height),
    ];
    notes.extend(extra_notes);

    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::OleObject,
        payload,
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence,
        notes,
    });
    node_id
}

fn adapt_image_control(
    image: &Hwp5ImageControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    ids: &mut SemanticIdAlloc,
    image_assets: &Hwp5JoinedImageAssetPlan,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(image.ctrl_id);

    if let Some(asset) = image_assets.asset_for_binary_data_id(image.binary_data_id) {
        let mut payload: Hwp5SemanticImagePayload = asset.payload.clone();
        payload.width_hwp = i32::try_from(image.geometry.width).ok();
        payload.height_hwp = i32::try_from(image.geometry.height).ok();

        build.controls.push(Hwp5SemanticControlNode {
            node_id,
            kind: Hwp5SemanticControlKind::Image,
            payload: Hwp5SemanticControlPayload::Image(payload),
            container: container.clone(),
            literal_ctrl_id: Some(literal),
            anchor_paragraph_id: Some(paragraph_id),
            confidence: Hwp5SemanticConfidence::High,
            notes: vec![
                format!("raw_ctrl_id=0x{:08X}", image.ctrl_id),
                format!("binary_data_id={}", image.binary_data_id),
            ],
        });
        return node_id;
    }

    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::Unknown(literal.clone()),
        payload: Hwp5SemanticControlPayload::None,
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::Medium,
        notes: vec![
            format!("raw_ctrl_id=0x{:08X}", image.ctrl_id),
            format!("binary_data_id={} had no joined image asset", image.binary_data_id),
        ],
    });
    node_id
}

fn adapt_line_control(
    line: &Hwp5LineControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(line.ctrl_id);
    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::Line,
        payload: Hwp5SemanticControlPayload::Line(Hwp5SemanticLinePayload {
            x_hwp: line.geometry.x,
            y_hwp: line.geometry.y,
            width_hwp: line.geometry.width,
            height_hwp: line.geometry.height,
            start: Hwp5SemanticShapePoint { x: line.start.x, y: line.start.y },
            end: Hwp5SemanticShapePoint { x: line.end.x, y: line.end.y },
        }),
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::High,
        notes: vec![format!("raw_ctrl_id=0x{:08X}", line.ctrl_id)],
    });
    node_id
}

fn adapt_rect_control(
    rect: &Hwp5RectControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(rect.ctrl_id);
    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::Rect,
        payload: Hwp5SemanticControlPayload::Rect(Hwp5SemanticRectPayload {
            x_hwp: rect.geometry.x,
            y_hwp: rect.geometry.y,
            width_hwp: rect.geometry.width,
            height_hwp: rect.geometry.height,
        }),
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::High,
        notes: vec![
            format!("raw_ctrl_id=0x{:08X}", rect.ctrl_id),
            "projection_unsupported=pure_rect_requires_core_hwpx_capability".to_string(),
        ],
    });
    node_id
}

fn adapt_polygon_control(
    polygon: &Hwp5PolygonControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(polygon.ctrl_id);
    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::Polygon,
        payload: Hwp5SemanticControlPayload::Polygon(Hwp5SemanticPolygonPayload {
            x_hwp: polygon.geometry.x,
            y_hwp: polygon.geometry.y,
            width_hwp: polygon.geometry.width,
            height_hwp: polygon.geometry.height,
            points: polygon
                .points
                .iter()
                .map(|point| Hwp5SemanticShapePoint { x: point.x, y: point.y })
                .collect(),
        }),
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::High,
        notes: vec![format!("raw_ctrl_id=0x{:08X}", polygon.ctrl_id)],
    });
    node_id
}

fn adapt_nested_subtree_control(
    subtree: &Hwp5NestedSubtree,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    spec: NestedSubtreeSemanticSpec,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let literal = crate::ctrl_id_ascii(subtree.ctrl_id);
    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: spec.kind.clone(),
        payload: Hwp5SemanticControlPayload::None,
        container: container.clone(),
        literal_ctrl_id: Some(literal),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::High,
        notes: vec![format!("raw_ctrl_id=0x{:08X}", subtree.ctrl_id)],
    });

    let nested_container = Hwp5SemanticContainerPath::new(vec![spec.subtree_container]);
    for paragraph in &subtree.paragraphs {
        let _ = adapt_paragraph(
            paragraph,
            nested_container.clone(),
            Some(node_id),
            build,
            support,
            ids,
        );
    }

    node_id
}

fn adapt_textbox_control(
    textbox: &Hwp5TextBoxControl,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let subtree =
        Hwp5NestedSubtree { ctrl_id: textbox.ctrl_id, paragraphs: textbox.paragraphs.clone() };

    adapt_nested_subtree_control(
        &subtree,
        container,
        paragraph_id,
        NestedSubtreeSemanticSpec {
            kind: Hwp5SemanticControlKind::TextBox,
            subtree_container: Hwp5SemanticContainerKind::TextBoxSubList,
        },
        build,
        support,
        ids,
    )
}

fn adapt_table_control(
    table: &Hwp5Table,
    container: &Hwp5SemanticContainerPath,
    paragraph_id: Hwp5SemanticParagraphId,
    build: &mut SectionBuildState,
    support: SemanticSupport<'_>,
    ids: &mut SemanticIdAlloc,
) -> Hwp5SemanticControlId {
    let node_id = ids.next_control_id();
    let structural_width_hwp: Option<i32> = structural_table_width_hwp(table);
    let cell_evidence: Vec<Hwp5SemanticTableCellEvidence> = table
        .cells
        .iter()
        .map(|cell| Hwp5SemanticTableCellEvidence {
            column: cell.column,
            row: cell.row,
            col_span: cell.col_span,
            row_span: cell.row_span,
            is_header: cell.is_header,
            border_fill_id: cell.border_fill_id,
            height_hwp: (cell.height > 0).then_some(cell.height),
            width_hwp: (cell.width > 0).then_some(cell.width),
            margin_hwp: Hwp5SemanticTableCellMargin {
                left_hwp: cell.margin.left,
                right_hwp: cell.margin.right,
                top_hwp: cell.margin.top,
                bottom_hwp: cell.margin.bottom,
            },
            vertical_align: semantic_table_cell_vertical_align(cell.vertical_align),
        })
        .collect();
    let distinct_cell_border_fill_ids: Vec<u16> = table
        .cells
        .iter()
        .filter_map(|cell| cell.border_fill_id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let distinct_cell_heights_hwp: Vec<i32> = table
        .cells
        .iter()
        .map(|cell| cell.height)
        .filter(|height| *height > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let distinct_cell_widths_hwp: Vec<i32> = table
        .cells
        .iter()
        .map(|cell| cell.width)
        .filter(|width| *width > 0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let header_row_count: u16 = structural_header_row_count(table);
    let row_max_cell_heights_hwp: Vec<i32> = structural_row_max_cell_heights_hwp(table);
    let mut notes: Vec<String> = vec![
        format!("rows={} cols={} cells={}", table.rows, table.cols, table.cells.len()),
        format!("page_break={}", semantic_table_page_break(table.page_break).audit_key()),
        format!("repeat_header={}", table.repeat_header),
        format!("cell_spacing_hwp={}", table.cell_spacing),
    ];
    if let Some(border_fill_id) = table.border_fill_id {
        notes.push(format!("table_border_fill_id={border_fill_id}"));
    }
    if !distinct_cell_border_fill_ids.is_empty() {
        let joined =
            distinct_cell_border_fill_ids.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("cell_border_fill_ids={joined}"));
    }
    if !distinct_cell_heights_hwp.is_empty() {
        let joined =
            distinct_cell_heights_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("cell_heights_hwp={joined}"));
    }
    if !distinct_cell_widths_hwp.is_empty() {
        let joined =
            distinct_cell_widths_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("cell_widths_hwp={joined}"));
    }
    if let Some(width_hwp) = structural_width_hwp {
        notes.push(format!("table_structural_width_hwp={width_hwp}"));
    }
    if !row_max_cell_heights_hwp.is_empty() {
        let joined =
            row_max_cell_heights_hwp.iter().map(i32::to_string).collect::<Vec<_>>().join(",");
        notes.push(format!("row_max_cell_heights_hwp={joined}"));
    }
    if header_row_count > 0 {
        notes.push(format!("header_rows={header_row_count}"));
    }
    let distinct_vertical_aligns: Vec<String> = cell_evidence
        .iter()
        .map(|cell| cell.vertical_align.audit_key())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if !distinct_vertical_aligns.is_empty() {
        notes.push(format!("cell_vertical_aligns={}", distinct_vertical_aligns.join(",")));
    }
    if cell_evidence.iter().any(|cell| {
        cell.margin_hwp.left_hwp != 0
            || cell.margin_hwp.right_hwp != 0
            || cell.margin_hwp.top_hwp != 0
            || cell.margin_hwp.bottom_hwp != 0
    }) {
        notes.push("cell_margins_hwp=present".to_string());
    }
    build.controls.push(Hwp5SemanticControlNode {
        node_id,
        kind: Hwp5SemanticControlKind::Table,
        payload: Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
            rows: table.rows,
            cols: table.cols,
            cell_count: table.cells.len(),
            page_break: semantic_table_page_break(table.page_break),
            repeat_header: table.repeat_header,
            header_row_count,
            cell_spacing_hwp: table.cell_spacing,
            border_fill_id: table.border_fill_id,
            distinct_cell_border_fill_ids,
            distinct_cell_heights_hwp,
            distinct_cell_widths_hwp,
            structural_width_hwp,
            row_max_cell_heights_hwp,
            cells: cell_evidence,
        }),
        container: container.clone(),
        literal_ctrl_id: Some("tbl ".to_string()),
        anchor_paragraph_id: Some(paragraph_id),
        confidence: Hwp5SemanticConfidence::High,
        notes,
    });

    let cell_container =
        Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::TableCellSubList]);
    for cell in &table.cells {
        for paragraph in &cell.paragraphs {
            let _ = adapt_paragraph(
                paragraph,
                cell_container.clone(),
                Some(node_id),
                build,
                support,
                ids,
            );
        }
    }

    node_id
}

fn structural_table_width_hwp(table: &Hwp5Table) -> Option<i32> {
    let first_row: u16 = table.cells.iter().map(|cell| cell.row).min()?;
    let width_hwp: i32 = table
        .cells
        .iter()
        .filter(|cell| cell.row == first_row)
        .map(|cell| cell.width)
        .filter(|width| *width > 0)
        .sum();
    (width_hwp > 0).then_some(width_hwp)
}

fn structural_header_row_count(table: &Hwp5Table) -> u16 {
    let mut row_flags: BTreeMap<u16, (usize, usize)> = BTreeMap::new();
    for cell in &table.cells {
        let entry = row_flags.entry(cell.row).or_insert((0, 0));
        entry.0 += 1;
        if cell.is_header {
            entry.1 += 1;
        }
    }

    row_flags
        .into_iter()
        .take_while(|(_, (cells, header_cells))| *cells > 0 && *cells == *header_cells)
        .count() as u16
}

fn structural_row_max_cell_heights_hwp(table: &Hwp5Table) -> Vec<i32> {
    let mut row_maxima: BTreeMap<u16, i32> = BTreeMap::new();
    for cell in &table.cells {
        if cell.height <= 0 {
            continue;
        }
        row_maxima
            .entry(cell.row)
            .and_modify(|max_height| *max_height = (*max_height).max(cell.height))
            .or_insert(cell.height);
    }
    row_maxima.into_values().collect()
}

fn adapt_page_def(page_def: &crate::schema::section::Hwp5PageDef) -> Hwp5SemanticPageDefSummary {
    Hwp5SemanticPageDefSummary {
        width: page_def.width,
        height: page_def.height,
        margin_left: page_def.margin_left,
        margin_right: page_def.margin_right,
        margin_top: page_def.margin_top,
        margin_bottom: page_def.margin_bottom,
        header_margin: page_def.header_margin,
        footer_margin: page_def.footer_margin,
        gutter: page_def.gutter,
        landscape: page_def.landscape,
    }
}

fn populate_inline_items(
    text: &str,
    control_ids: &[Hwp5SemanticControlId],
    inline_items: &mut Vec<Hwp5SemanticInlineItem>,
) {
    let placeholder: char = '\u{FFFC}';
    let mut remaining_controls = control_ids.iter().copied();
    let mut buffer = String::new();

    for ch in text.chars() {
        buffer.push(ch);
        if ch == placeholder {
            if !buffer.is_empty() {
                inline_items
                    .push(Hwp5SemanticInlineItem::Text { text: std::mem::take(&mut buffer) });
            }

            if let Some(control_id) = remaining_controls.next() {
                inline_items.push(Hwp5SemanticInlineItem::Control { control_id });
            }
        }
    }

    if !buffer.is_empty() {
        inline_items.push(Hwp5SemanticInlineItem::Text { text: buffer });
    }

    for control_id in remaining_controls {
        inline_items.push(Hwp5SemanticInlineItem::Control { control_id });
    }

    if inline_items.is_empty() && text.is_empty() {
        return;
    }

    if inline_items.iter().all(|item| matches!(item, Hwp5SemanticInlineItem::Control { .. })) {
        inline_items.insert(0, Hwp5SemanticInlineItem::Text { text: String::new() });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use crate::build_hwp5_semantic_file;
    use crate::decoder::header::DocInfoResult;
    use crate::decoder::section::{
        Hwp5ImageControl, Hwp5TableCell, Hwp5TableCellMargin, Hwp5TableCellVerticalAlign,
        Hwp5TablePageBreak, Hwp5TextBoxControl, SectionResult,
    };
    use crate::schema::section::Hwp5PageDef;
    use crate::Hwp5JoinedImageAsset;
    use crate::Hwp5SemanticImageFormat;
    use crate::Hwp5SemanticTablePageBreak;

    fn empty_doc_info() -> DocInfoResult {
        DocInfoResult {
            id_mappings: None,
            fonts: Vec::new(),
            char_shapes: Vec::new(),
            para_shapes: Vec::new(),
            tab_defs: Vec::new(),
            styles: Vec::new(),
            border_fills: Vec::new(),
            warnings: Vec::new(),
        }
    }

    fn fixture_path(name: &str) -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name)
    }

    fn semantic_fixture(name: &str) -> Option<Hwp5SemanticDocument> {
        let path = fixture_path(name);
        if !path.exists() {
            return None;
        }

        Some(build_hwp5_semantic_file(&path).expect("semantic build should succeed"))
    }

    fn container_path(kind: Hwp5SemanticContainerKind) -> Hwp5SemanticContainerPath {
        Hwp5SemanticContainerPath::new(vec![kind])
    }

    fn empty_image_plan() -> Hwp5JoinedImageAssetPlan {
        Hwp5JoinedImageAssetPlan {
            ordered_assets: Vec::new(),
            assets_by_binary_data_id: BTreeMap::new(),
        }
    }

    fn image_plan<'a>(
        assets: impl IntoIterator<Item = (u16, &'a str, Hwp5SemanticImageFormat)>,
    ) -> Hwp5JoinedImageAssetPlan {
        let ordered_assets: Vec<Hwp5JoinedImageAsset> = assets
            .into_iter()
            .map(|(binary_data_id, storage_name, format)| Hwp5JoinedImageAsset {
                payload: Hwp5SemanticImagePayload {
                    binary_data_id,
                    storage_name: storage_name.to_string(),
                    package_path: format!("BinData/{storage_name}"),
                    format,
                    width_hwp: None,
                    height_hwp: None,
                },
                bytes: vec![binary_data_id as u8],
            })
            .collect();
        let assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> = ordered_assets
            .iter()
            .cloned()
            .map(|asset| (asset.payload.binary_data_id, asset))
            .collect();
        Hwp5JoinedImageAssetPlan { ordered_assets, assets_by_binary_data_id }
    }

    fn image_controls_in_container(
        semantic: &Hwp5SemanticDocument,
        kind: Hwp5SemanticContainerKind,
    ) -> Vec<&Hwp5SemanticControlNode> {
        let path = container_path(kind);
        semantic
            .sections
            .iter()
            .flat_map(|section| section.controls.iter())
            .filter(|control| {
                control.kind == Hwp5SemanticControlKind::Image
                    && control.container == path
                    && control.literal_ctrl_id.as_deref() == Some("gso ")
            })
            .collect()
    }

    fn ole_controls_in_container(
        semantic: &Hwp5SemanticDocument,
        kind: Hwp5SemanticContainerKind,
    ) -> Vec<&Hwp5SemanticControlNode> {
        let path = container_path(kind);
        semantic
            .sections
            .iter()
            .flat_map(|section| section.controls.iter())
            .filter(|control| {
                control.kind == Hwp5SemanticControlKind::OleObject
                    && control.container == path
                    && control.literal_ctrl_id.as_deref() == Some("gso ")
            })
            .collect()
    }

    fn package_bin_storage_names(semantic: &Hwp5SemanticDocument) -> Vec<String> {
        let mut storage_names: Vec<String> = semantic
            .package_meta
            .bin_data_records
            .iter()
            .map(|record| record.storage_name.clone())
            .collect();
        storage_names.sort();
        storage_names
    }

    fn package_bin_stream_names(semantic: &Hwp5SemanticDocument) -> Vec<String> {
        let mut stream_names: Vec<String> = semantic
            .package_meta
            .bin_data_streams
            .iter()
            .map(|stream| stream.name.clone())
            .collect();
        stream_names.sort();
        stream_names
    }

    fn assert_chart_fixture_ole_evidence(semantic: &Hwp5SemanticDocument) {
        assert!(semantic.graph_is_coherent());
        assert!(
            semantic
                .sections
                .iter()
                .flat_map(|section| section.controls.iter())
                .all(|control| control.kind != Hwp5SemanticControlKind::Chart),
            "chart discovery v1 must preserve OLE evidence without emitting Chart semantic nodes"
        );

        let body_ole_controls =
            ole_controls_in_container(semantic, Hwp5SemanticContainerKind::Body);
        assert_eq!(body_ole_controls.len(), 1);
        assert_eq!(package_bin_storage_names(semantic), vec!["BIN0001.OLE".to_string()]);
        assert_eq!(package_bin_stream_names(semantic), vec!["BIN0001.OLE".to_string()]);
        assert_eq!(body_ole_controls[0].confidence, Hwp5SemanticConfidence::High);
        assert!(matches!(
            body_ole_controls[0].payload,
            Hwp5SemanticControlPayload::OleObject(Hwp5SemanticOlePayload {
                binary_data_id: 1,
                ref storage_name,
                ref package_path,
                ..
            }) if storage_name == "BIN0001.OLE" && package_path == "BinData/BIN0001.OLE"
        ));
        assert!(
            body_ole_controls[0].notes.iter().any(|note| note == "raw_ctrl_id=0x67736F20"),
            "OLE notes must preserve the raw gso control identifier"
        );
        assert!(
            body_ole_controls[0].notes.iter().any(|note| note == "binary_data_id=1"),
            "OLE notes must preserve the joined BinData identifier"
        );
        assert!(
            body_ole_controls[0].notes.iter().any(|note| note == "storage_name=BIN0001.OLE"),
            "OLE notes must preserve the joined storage name as factual evidence"
        );
        assert!(
            body_ole_controls[0].notes.iter().any(|note| note == "storage_extension=OLE"),
            "OLE notes must preserve the storage extension as factual evidence"
        );
    }

    #[test]
    fn adapter_preserves_body_table_and_unknown_controls() {
        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: true,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
            doc_info: empty_doc_info(),
            sections: vec![SectionResult {
                paragraphs: vec![Hwp5Paragraph {
                    text: "\u{fffc}본문".to_string(),
                    para_shape_id: 2,
                    style_id: 1,
                    char_shape_runs: Vec::new(),
                    controls: vec![
                        Hwp5Control::Table(Hwp5Table {
                            rows: 1,
                            cols: 1,
                            page_break: Hwp5TablePageBreak::Cell,
                            repeat_header: true,
                            cell_spacing: 120,
                            border_fill_id: Some(8),
                            cells: vec![Hwp5TableCell {
                                column: 0,
                                row: 0,
                                col_span: 1,
                                row_span: 1,
                                width: 1000,
                                height: 1000,
                                is_header: true,
                                margin: Hwp5TableCellMargin {
                                    left: 15,
                                    right: 20,
                                    top: 10,
                                    bottom: 5,
                                },
                                vertical_align: Hwp5TableCellVerticalAlign::Bottom,
                                border_fill_id: Some(3),
                                paragraphs: vec![Hwp5Paragraph {
                                    text: "cell".to_string(),
                                    para_shape_id: 4,
                                    style_id: 0,
                                    char_shape_runs: Vec::new(),
                                    controls: vec![Hwp5Control::Unknown { ctrl_id: 0x6865_6164 }],
                                }],
                            }],
                        }),
                        Hwp5Control::Unknown { ctrl_id: 0x666F_6F74 },
                    ],
                }],
                page_def: Some(Hwp5PageDef {
                    width: 12_300,
                    height: 10_000,
                    margin_left: 100,
                    margin_right: 200,
                    margin_top: 300,
                    margin_bottom: 400,
                    header_margin: 500,
                    footer_margin: 600,
                    gutter: 700,
                    landscape: true,
                }),
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(&decoded, &empty_image_plan());
        assert_eq!(semantic.sections.len(), 1);
        assert!(semantic.graph_is_coherent());

        let section = &semantic.sections[0];
        assert_eq!(section.page_def.as_ref().map(|page_def| page_def.landscape), Some(true));
        assert_eq!(section.paragraphs.len(), 2);
        assert_eq!(
            section.paragraphs[0].container,
            Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::Body])
        );
        assert_eq!(
            section.paragraphs[1].container,
            Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::TableCellSubList])
        );
        assert_eq!(section.paragraphs[1].owner_control_id, Some(section.controls[0].node_id));
        assert_eq!(
            section.paragraphs[0].inline_items,
            vec![
                Hwp5SemanticInlineItem::Text { text: "\u{fffc}".to_string() },
                Hwp5SemanticInlineItem::Control {
                    control_id: section.paragraphs[0].control_ids[0],
                },
                Hwp5SemanticInlineItem::Text { text: "본문".to_string() },
                Hwp5SemanticInlineItem::Control {
                    control_id: section.paragraphs[0].control_ids[1],
                },
            ]
        );
        assert_eq!(section.controls.len(), 3);
        assert!(section.controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Table
                && control.payload
                    == Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
                        rows: 1,
                        cols: 1,
                        cell_count: 1,
                        page_break: Hwp5SemanticTablePageBreak::Cell,
                        repeat_header: true,
                        header_row_count: 1,
                        cell_spacing_hwp: 120,
                        border_fill_id: Some(8),
                        distinct_cell_border_fill_ids: vec![3],
                        distinct_cell_heights_hwp: vec![1000],
                        distinct_cell_widths_hwp: vec![1000],
                        structural_width_hwp: Some(1000),
                        row_max_cell_heights_hwp: vec![1000],
                        cells: vec![crate::semantic::Hwp5SemanticTableCellEvidence {
                            column: 0,
                            row: 0,
                            col_span: 1,
                            row_span: 1,
                            is_header: true,
                            border_fill_id: Some(3),
                            height_hwp: Some(1000),
                            width_hwp: Some(1000),
                            margin_hwp: crate::semantic::Hwp5SemanticTableCellMargin {
                                left_hwp: 15,
                                right_hwp: 20,
                                top_hwp: 10,
                                bottom_hwp: 5,
                            },
                            vertical_align:
                                crate::semantic::Hwp5SemanticTableCellVerticalAlign::Bottom,
                        }],
                    })
        }));
        let table_control = section
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::Table)
            .expect("table control should exist");
        assert!(table_control.notes.iter().any(|note| note == "page_break=cell"));
        assert!(table_control.notes.iter().any(|note| note == "repeat_header=true"));
        assert!(table_control.notes.iter().any(|note| note == "cell_spacing_hwp=120"));
        assert!(table_control.notes.iter().any(|note| note == "table_border_fill_id=8"));
        assert!(table_control.notes.iter().any(|note| note == "cell_border_fill_ids=3"));
        assert!(table_control.notes.iter().any(|note| note == "cell_heights_hwp=1000"));
        assert!(table_control.notes.iter().any(|note| note == "cell_widths_hwp=1000"));
        assert!(table_control.notes.iter().any(|note| note == "table_structural_width_hwp=1000"));
        assert!(table_control.notes.iter().any(|note| note == "row_max_cell_heights_hwp=1000"));
        assert!(section.controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Unknown("foot".to_string())
                && control.literal_ctrl_id.as_deref() == Some("foot")
                && control.notes.iter().any(|note| note == "raw_ctrl_id=0x666F6F74")
        }));
        assert!(section.controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Unknown("head".to_string())
                && control.container
                    == Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::TableCellSubList,
                    ])
        }));
    }

    #[test]
    fn adapter_maps_doc_info_refs_into_semantic_doc_info() {
        let doc_info = DocInfoResult {
            id_mappings: None,
            fonts: vec![crate::schema::header::Hwp5RawFaceName {
                property: 0,
                face_name: "바탕".to_string(),
                alternate_font_type: None,
                alternate_font_name: None,
                panose1: None,
                default_font_name: None,
            }],
            char_shapes: vec![crate::schema::header::Hwp5RawCharShape::parse(&[0u8; 68]).unwrap()],
            para_shapes: vec![crate::schema::header::Hwp5RawParaShape::parse(&[0u8; 42]).unwrap()],
            tab_defs: Vec::new(),
            styles: vec![crate::schema::header::Hwp5RawStyle {
                name: "본문".to_string(),
                english_name: "Body".to_string(),
                kind: 0,
                next_style_id: 0,
                lang_id: 0,
                para_shape_id: 7,
                char_shape_id: 9,
                lock_form: 0,
            }],
            border_fills: Vec::new(),
            warnings: Vec::new(),
        };

        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: false,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
            doc_info,
            sections: vec![SectionResult {
                paragraphs: Vec::new(),
                page_def: None,
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(&decoded, &empty_image_plan());
        assert_eq!(semantic.doc_info.font_faces, vec!["바탕".to_string()]);
        assert_eq!(semantic.doc_info.char_shape_ids, vec![0]);
        assert_eq!(semantic.doc_info.para_shape_ids, vec![0]);
        assert_eq!(semantic.doc_info.named_styles.len(), 1);
        assert_eq!(semantic.doc_info.named_styles[0].style_id, Some(0));
        assert_eq!(semantic.doc_info.named_styles[0].para_shape_id, Some(7));
        assert_eq!(semantic.doc_info.named_styles[0].char_shape_id, Some(9));
    }

    #[test]
    fn adapter_keeps_inline_summary_and_control_inventory_coherent() {
        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: false,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
            doc_info: empty_doc_info(),
            sections: vec![SectionResult {
                paragraphs: vec![Hwp5Paragraph {
                    text: "앞\u{fffc}뒤".to_string(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    controls: vec![Hwp5Control::Unknown { ctrl_id: 0x6865_6164 }],
                }],
                page_def: None,
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(&decoded, &empty_image_plan());
        let paragraph = &semantic.sections[0].paragraphs[0];
        assert_eq!(paragraph.inline_text_summary(), paragraph.text);
        assert_eq!(paragraph.inline_control_ids(), paragraph.control_ids);
        assert_eq!(paragraph.owner_control_id, None);
    }

    #[test]
    fn adapter_maps_paragraph_local_image_control_into_semantic_image_payload() {
        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: false,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
            doc_info: empty_doc_info(),
            sections: vec![SectionResult {
                paragraphs: vec![Hwp5Paragraph {
                    text: "앞\u{fffc}뒤".to_string(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    controls: vec![Hwp5Control::Image(Hwp5ImageControl {
                        ctrl_id: 0x6773_6F20,
                        geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                            x: 0,
                            y: 0,
                            width: 3_210,
                            height: 4_560,
                        },
                        binary_data_id: 1,
                    })],
                }],
                page_def: None,
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(
            &decoded,
            &image_plan([(1, "BIN0001.png", Hwp5SemanticImageFormat::Png)]),
        );
        let paragraph = &semantic.sections[0].paragraphs[0];
        let image_control = semantic.sections[0]
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::Image)
            .expect("semantic image control should exist");
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).len(),
            1
        );

        assert_eq!(
            paragraph.inline_items,
            vec![
                Hwp5SemanticInlineItem::Text { text: "앞\u{fffc}".to_string() },
                Hwp5SemanticInlineItem::Control { control_id: image_control.node_id },
                Hwp5SemanticInlineItem::Text { text: "뒤".to_string() },
            ]
        );
        assert_eq!(
            image_control.payload,
            Hwp5SemanticControlPayload::Image(Hwp5SemanticImagePayload {
                binary_data_id: 1,
                storage_name: "BIN0001.png".to_string(),
                package_path: "BinData/BIN0001.png".to_string(),
                format: Hwp5SemanticImageFormat::Png,
                width_hwp: Some(3_210),
                height_hwp: Some(4_560),
            })
        );
    }

    #[test]
    fn adapter_demotes_ole_object_when_bin_data_stream_is_missing() {
        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: false,
            package_entries: Vec::new(),
            bin_data_records: vec![crate::Hwp5BinDataRecordSummary {
                binary_data_id: 1,
                storage_name: "BIN0001.OLE".to_string(),
                extension: "OLE".to_string(),
                data_type: "Embedding".to_string(),
                compression: "Default".to_string(),
                should_decompress: false,
            }],
            bin_data_streams: Vec::new(),
            doc_info: empty_doc_info(),
            sections: vec![SectionResult {
                paragraphs: vec![Hwp5Paragraph {
                    text: "\u{fffc}".to_string(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    controls: vec![Hwp5Control::OleObject(Hwp5OleObjectControl {
                        ctrl_id: 0x6773_6F20,
                        geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                            x: 10,
                            y: 20,
                            width: 5_000,
                            height: 6_000,
                        },
                        binary_data_id: 1,
                        extent_width: 9_100,
                        extent_height: 8_200,
                    })],
                }],
                page_def: None,
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(&decoded, &empty_image_plan());
        assert!(semantic.graph_is_coherent());

        let ole_control = semantic.sections[0]
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::OleObject)
            .expect("semantic ole control should exist");
        assert_eq!(ole_control.confidence, Hwp5SemanticConfidence::Medium);
        assert_eq!(ole_control.payload, Hwp5SemanticControlPayload::None);
        assert!(ole_control.notes.iter().any(|note| note == "bin_data_stream_missing"));
    }

    #[test]
    fn adapter_reconstructs_textbox_subtree_with_owner_linkage_in_synthetic_input() {
        let decoded = DecodedHwp5Intermediate {
            version: "5.1.1.0".to_string(),
            compressed: false,
            package_entries: Vec::new(),
            bin_data_records: Vec::new(),
            bin_data_streams: Vec::new(),
            doc_info: empty_doc_info(),
            sections: vec![SectionResult {
                paragraphs: vec![Hwp5Paragraph {
                    text: "\u{fffc}".to_string(),
                    para_shape_id: 0,
                    style_id: 0,
                    char_shape_runs: Vec::new(),
                    controls: vec![Hwp5Control::TextBox(Hwp5TextBoxControl {
                        ctrl_id: 0x6773_6F20,
                        geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                            x: 10,
                            y: 20,
                            width: 8_000,
                            height: 6_000,
                        },
                        paragraphs: vec![Hwp5Paragraph {
                            text: "글상자 시작.\u{fffc}글상자 끝.".to_string(),
                            para_shape_id: 1,
                            style_id: 0,
                            char_shape_runs: Vec::new(),
                            controls: vec![Hwp5Control::Unknown { ctrl_id: 0x6773_6F20 }],
                        }],
                    })],
                }],
                page_def: None,
                warnings: Vec::new(),
            }],
            warnings: Vec::new(),
        };

        let semantic = adapt_to_semantic(&decoded, &empty_image_plan());
        assert!(semantic.graph_is_coherent());

        let section = &semantic.sections[0];
        let textbox_control = section
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::TextBox)
            .expect("textbox control should exist");
        let textbox_paragraph = section
            .paragraphs
            .iter()
            .find(|paragraph| {
                paragraph.container
                    == Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::TextBoxSubList,
                    ])
            })
            .expect("textbox subtree paragraph should exist");
        assert_eq!(textbox_paragraph.owner_control_id, Some(textbox_control.node_id));
        assert_eq!(textbox_paragraph.inline_control_ids().len(), 1);
        assert!(textbox_paragraph.text.contains("글상자 시작."));
        assert!(textbox_paragraph.text.contains("글상자 끝."));
    }

    #[test]
    fn fixture_hwp5_00_semantic_slice_is_body_only_and_coherent() {
        let path = fixture_path("hwp5_00.hwp");
        if !path.exists() {
            return;
        }

        let semantic = build_hwp5_semantic_file(&path).expect("semantic build should succeed");
        assert!(semantic.graph_is_coherent());
        assert_eq!(semantic.sections.len(), 1);
        assert!(semantic.sections[0].paragraphs.iter().all(|paragraph| paragraph.container
            == Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::Body])));

        let snapshot = semantic.parser_audit_snapshot();
        assert_eq!(
            snapshot.paragraph_container_counts,
            vec![crate::semantic::Hwp5ParserAuditContainerCount {
                kind: Hwp5SemanticContainerKind::Body,
                count: semantic.sections[0].paragraphs.len(),
            }]
        );
    }

    #[test]
    fn fixture_hwp5_01_semantic_slice_preserves_table_cell_sublist() {
        let path = fixture_path("hwp5_01.hwp");
        if !path.exists() {
            return;
        }

        let semantic = build_hwp5_semantic_file(&path).expect("semantic build should succeed");
        assert!(semantic.graph_is_coherent());

        let first_section = &semantic.sections[0];
        assert!(first_section.controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Table
                && matches!(
                    control.payload,
                    Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
                        rows: _,
                        cols: _,
                        cell_count,
                        page_break: _,
                        repeat_header: _,
                        header_row_count: _,
                        cell_spacing_hwp: _,
                        border_fill_id: _,
                        distinct_cell_border_fill_ids: _,
                        distinct_cell_heights_hwp: _,
                        distinct_cell_widths_hwp: _,
                        structural_width_hwp: _,
                        row_max_cell_heights_hwp: _,
                        cells: _,
                    }) if cell_count > 0
                )
        }));
        assert!(first_section.paragraphs.iter().any(|paragraph| {
            paragraph.container
                == Hwp5SemanticContainerPath::new(vec![Hwp5SemanticContainerKind::TableCellSubList])
        }));

        let snapshot = semantic.parser_audit_snapshot();
        assert!(snapshot.paragraph_container_counts.iter().any(|entry| {
            entry.kind == Hwp5SemanticContainerKind::TableCellSubList && entry.count > 0
        }));
        assert!(snapshot.container_owner_counts.iter().any(|entry| {
            entry.container == Hwp5SemanticContainerKind::TableCellSubList
                && entry.owner_kind == Hwp5SemanticControlKind::Table
                && entry.count > 0
        }));
        assert!(snapshot.container_control_counts.iter().any(|entry| {
            entry.container == Hwp5SemanticContainerKind::Body
                && entry.kind == Hwp5SemanticControlKind::Table
                && entry.count > 0
        }));
    }

    #[test]
    fn fixture_table_06_repeat_header_semantic_slice_preserves_table_flags() {
        let Some(semantic) = semantic_fixture("table_06_repeat_header_row.hwp") else {
            return;
        };

        let table_control = semantic.sections[0]
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::Table)
            .expect("table control should exist");
        assert!(matches!(
            table_control.payload,
            Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
                page_break: Hwp5SemanticTablePageBreak::Cell,
                repeat_header: true,
                ..
            })
        ));
    }

    #[test]
    fn fixture_table_06b_no_repeat_header_semantic_slice_preserves_table_flags() {
        let Some(semantic) = semantic_fixture("table_06b_no_repeat_header_row.hwp") else {
            return;
        };

        let table_control = semantic.sections[0]
            .controls
            .iter()
            .find(|control| control.kind == Hwp5SemanticControlKind::Table)
            .expect("table control should exist");
        assert!(matches!(
            table_control.payload,
            Hwp5SemanticControlPayload::Table(Hwp5SemanticTablePayload {
                page_break: Hwp5SemanticTablePageBreak::Cell,
                repeat_header: false,
                ..
            })
        ));
    }

    #[test]
    fn fixture_table_06c_multi_page_semantic_slice_preserves_header_rows() {
        let Some(semantic) = semantic_fixture("table_06c_repeat_header_multi_page.hwp") else {
            return;
        };

        let table_payload = semantic.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table control should exist");
        assert_eq!(table_payload.header_row_count, 1);
        assert!(table_payload.repeat_header);
        assert!(table_payload.cells.iter().filter(|cell| cell.is_header).count() >= 3);
    }

    #[test]
    fn fixture_table_06d_multi_page_semantic_slice_preserves_title_row_without_repeat() {
        let Some(semantic) = semantic_fixture("table_06d_no_repeat_header_multi_page.hwp") else {
            return;
        };

        let table_payload = semantic.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table control should exist");
        assert_eq!(table_payload.header_row_count, 1);
        assert!(!table_payload.repeat_header);
        assert_eq!(table_payload.cells.iter().filter(|cell| cell.is_header).count(), 3);
    }

    #[test]
    fn fixture_table_09_page_break_semantic_slice_preserves_all_modes() {
        let Some(table_mode) = semantic_fixture("table_09a_page_break_cell.hwp") else {
            return;
        };
        let Some(none_mode) = semantic_fixture("table_09c_page_break_none.hwp") else {
            return;
        };
        let Some(cell_mode) = semantic_fixture("table_09d_page_break_cell_explicit.hwp") else {
            return;
        };

        let table_payload = table_mode.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table mode payload should exist");
        let none_payload = none_mode.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("none mode payload should exist");
        let cell_payload = cell_mode.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("cell mode payload should exist");

        assert_eq!(table_payload.page_break, Hwp5SemanticTablePageBreak::Table);
        assert_eq!(none_payload.page_break, Hwp5SemanticTablePageBreak::None);
        assert_eq!(cell_payload.page_break, Hwp5SemanticTablePageBreak::Cell);
    }

    #[test]
    fn fixture_table_03_border_fill_semantic_slice_preserves_border_fill_evidence() {
        let Some(semantic) = semantic_fixture("table_03_border_fill_variants.hwp") else {
            return;
        };

        let payload = semantic.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table payload should exist");
        assert!(!payload.distinct_cell_border_fill_ids.is_empty());
    }

    #[test]
    fn fixture_table_04_semantic_slice_preserves_cell_vertical_align_evidence() {
        let Some(semantic) = semantic_fixture("table_04_vertical_align.hwp") else {
            return;
        };

        let payload = semantic.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table payload should exist");

        let vertical_aligns: Vec<_> =
            payload.cells.iter().map(|cell| cell.vertical_align.audit_key()).collect();
        assert_eq!(vertical_aligns, vec!["top", "center", "bottom"]);
    }

    #[test]
    fn fixture_table_05_semantic_slice_preserves_cell_margin_evidence() {
        let Some(semantic) = semantic_fixture("table_05_cell_margin_padding.hwp") else {
            return;
        };

        let payload = semantic.sections[0]
            .controls
            .iter()
            .find_map(|control| match &control.payload {
                Hwp5SemanticControlPayload::Table(payload) => Some(payload),
                _ => None,
            })
            .expect("table payload should exist");

        assert_eq!(payload.cells.len(), 2);
        assert_eq!(
            payload.cells[1].margin_hwp,
            crate::semantic::Hwp5SemanticTableCellMargin {
                left_hwp: 4251,
                right_hwp: 5669,
                top_hwp: 2834,
                bottom_hwp: 1417,
            }
        );
    }

    #[test]
    fn fixture_hwp5_04_semantic_slice_preserves_page_def_summary() {
        let path = fixture_path("hwp5_04.hwp");
        if !path.exists() {
            return;
        }

        let semantic = build_hwp5_semantic_file(&path).expect("semantic build should succeed");
        assert!(semantic.graph_is_coherent());
        assert_eq!(semantic.sections.len(), 2);
        assert_eq!(
            semantic.sections[0].page_def.as_ref().map(|page_def| page_def.landscape),
            Some(true)
        );
        assert_eq!(
            semantic.sections[1].page_def.as_ref().map(|page_def| page_def.landscape),
            Some(false)
        );
    }

    #[test]
    fn fixture_img_01_semantic_slice_keeps_single_body_image_anchor_and_bin_data() {
        let Some(semantic) = semantic_fixture("img_01_single_png_inline.hwp") else {
            return;
        };

        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(package_bin_stream_names(&semantic), vec!["BIN0001.png".to_string()]);
        let body_images = image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body);
        assert_eq!(body_images.len(), 1);
        assert!(matches!(
            body_images[0].payload,
            Hwp5SemanticControlPayload::Image(Hwp5SemanticImagePayload {
                binary_data_id: 1,
                width_hwp: Some(_),
                height_hwp: Some(_),
                ..
            })
        ));
    }

    #[test]
    fn fixture_img_03_semantic_slice_keeps_two_body_image_anchors_and_distinct_bin_data() {
        let Some(semantic) = semantic_fixture("img_03_two_images_png_jpg.hwp") else {
            return;
        };

        assert!(semantic.graph_is_coherent());
        assert_eq!(
            package_bin_storage_names(&semantic),
            vec!["BIN0001.png".to_string(), "BIN0002.jpeg".to_string()]
        );
        assert_eq!(
            package_bin_stream_names(&semantic),
            vec!["BIN0001.png".to_string(), "BIN0002.jpeg".to_string()]
        );
        let body_images = image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body);
        assert_eq!(body_images.len(), 2);
    }

    #[test]
    fn fixture_mixed_02a_real_semantic_slice_reconstructs_header_and_footer_subtrees() {
        let Some(semantic) = semantic_fixture("mixed_02a_header_image_footer_text_real.hwp") else {
            return;
        };
        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);

        let controls: Vec<&Hwp5SemanticControlNode> =
            semantic.sections.iter().flat_map(|section| section.controls.iter()).collect();
        assert!(controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Header
                && control.literal_ctrl_id.as_deref() == Some("head")
        }));
        assert!(controls.iter().any(|control| {
            control.kind == Hwp5SemanticControlKind::Footer
                && control.literal_ctrl_id.as_deref() == Some("foot")
        }));

        let header_paragraphs: Vec<&Hwp5SemanticParagraph> = semantic
            .sections
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .filter(|paragraph| {
                paragraph.container
                    == Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::HeaderSubList,
                    ])
            })
            .collect();
        assert!(!header_paragraphs.is_empty());
        assert!(header_paragraphs.iter().all(|paragraph| paragraph.owner_control_id.is_some()));
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::HeaderSubList).len(),
            1
        );
        assert!(image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).is_empty());

        let footer_paragraphs: Vec<&Hwp5SemanticParagraph> = semantic
            .sections
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .filter(|paragraph| {
                paragraph.container
                    == Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::FooterSubList,
                    ])
            })
            .collect();
        assert!(!footer_paragraphs.is_empty());
        assert!(footer_paragraphs.iter().any(|paragraph| paragraph.text.contains("꼬리말 테스트")));
    }

    #[test]
    fn fixture_mixed_02b_real_semantic_slice_reconstructs_textbox_subtree() {
        let Some(semantic) = semantic_fixture("mixed_02b_textbox_with_image_real.hwp") else {
            return;
        };
        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);

        let textbox_controls: Vec<&Hwp5SemanticControlNode> = semantic
            .sections
            .iter()
            .flat_map(|section| section.controls.iter())
            .filter(|control| {
                control.kind == Hwp5SemanticControlKind::TextBox
                    && control.literal_ctrl_id.as_deref() == Some("gso ")
            })
            .collect();
        assert!(
            !textbox_controls.is_empty(),
            "textbox fixture should reconstruct textbox controls once decoder subtree capture lands"
        );

        let textbox_paragraphs: Vec<&Hwp5SemanticParagraph> = semantic
            .sections
            .iter()
            .flat_map(|section| section.paragraphs.iter())
            .filter(|paragraph| {
                paragraph.container
                    == Hwp5SemanticContainerPath::new(vec![
                        Hwp5SemanticContainerKind::TextBoxSubList,
                    ])
            })
            .collect();
        if !textbox_paragraphs.is_empty() {
            assert!(textbox_paragraphs
                .iter()
                .all(|paragraph| paragraph.owner_control_id.is_some()));
            assert!(textbox_paragraphs.iter().any(|paragraph| {
                paragraph.text.contains("글상자 시작.")
                    && paragraph.text.contains("글상자 끝.")
                    && !paragraph.inline_control_ids().is_empty()
            }));

            let snapshot = semantic.parser_audit_snapshot();
            assert!(snapshot.container_owner_counts.iter().any(|entry| {
                entry.container == Hwp5SemanticContainerKind::TextBoxSubList
                    && entry.owner_kind == Hwp5SemanticControlKind::TextBox
                    && entry.count > 0
            }));
        }
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::TextBoxSubList).len(),
            1
        );
    }

    #[test]
    fn fixture_img_05_semantic_slice_keeps_nested_gso_inside_table_cell_owner_container() {
        let Some(semantic) = semantic_fixture("img_05_image_in_table_cell.hwp") else {
            return;
        };
        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert!(image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).is_empty());
        assert!(
            !image_controls_in_container(&semantic, Hwp5SemanticContainerKind::TableCellSubList)
                .is_empty(),
            "image-in-table-cell fixture should keep nested image controls under the table-cell subtree container"
        );
    }

    #[test]
    fn fixture_floating_image_semantic_slice_keeps_single_body_image_anchor_and_bin_data() {
        let Some(semantic) = semantic_fixture("floating_image_not_treat_as_char.hwp") else {
            return;
        };

        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).len(),
            1
        );
    }

    #[test]
    fn fixture_two_same_image_refs_semantic_slice_keeps_two_body_anchors_with_one_binary() {
        let Some(semantic) = semantic_fixture("two_same_image_refs_different_places.hwp") else {
            return;
        };

        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(package_bin_stream_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).len(),
            2
        );
    }

    #[test]
    fn fixture_real_crop_semantic_slice_keeps_two_body_anchors_with_one_binary() {
        let Some(semantic) = semantic_fixture("real_crop_vs_original_two_objects.hwp") else {
            return;
        };

        assert!(semantic.graph_is_coherent());
        assert_eq!(package_bin_storage_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(package_bin_stream_names(&semantic), vec!["BIN0001.png".to_string()]);
        assert_eq!(
            image_controls_in_container(&semantic, Hwp5SemanticContainerKind::Body).len(),
            2
        );
    }

    #[test]
    fn fixture_chart_01_semantic_slice_preserves_ole_backed_gso_evidence() {
        let Some(semantic) = semantic_fixture("chart_01_single_column.hwp") else {
            return;
        };

        assert_chart_fixture_ole_evidence(&semantic);
    }

    #[test]
    fn fixture_chart_02_semantic_slice_preserves_ole_backed_gso_evidence() {
        let Some(semantic) = semantic_fixture("chart_02_single_pie.hwp") else {
            return;
        };

        assert_chart_fixture_ole_evidence(&semantic);
    }

    #[test]
    fn fixture_chart_03_semantic_slice_preserves_ole_backed_gso_evidence() {
        let Some(semantic) = semantic_fixture("chart_03_line_or_scatter.hwp") else {
            return;
        };

        assert_chart_fixture_ole_evidence(&semantic);
    }
}
