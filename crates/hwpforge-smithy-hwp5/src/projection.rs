//! HWP5 IR → Core document projection.
//!
//! This module converts the decoded HWP5 intermediate representation
//! (parsed records, style tables) into HwpForge Core's `Document<Draft>`
//! structure, bridging the format-specific layer to the format-agnostic core.

use hwpforge_core::document::{Document, Draft};
use hwpforge_core::image::{
    Image, ImageFormat, ImagePlacement, ImageRelativeTo, ImageStore, ImageTextFlow, ImageTextWrap,
};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::{HeaderFooter, Section};
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
use hwpforge_core::Control;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, StyleIndex};

use crate::decoder::section::{
    Hwp5Control, Hwp5ImageControl, Hwp5LineControl, Hwp5Paragraph, Hwp5PolygonControl, Hwp5Table,
    Hwp5TableCell, Hwp5TextBoxControl, SectionResult,
};
use crate::decoder::Hwp5Warning;
use crate::error::Hwp5Result;
use crate::numeric::positive_i32_from_u32;
use crate::schema::section::{
    Hwp5CharShapeRun, Hwp5PageDef, Hwp5ShapeComponentGeometry, Hwp5ShapePoint,
};
use crate::table_cell_vertical_align::{
    core_table_cell_vertical_align, unknown_hwp5_table_cell_vertical_align_raw,
};
use crate::table_page_break::{core_table_page_break, unknown_hwp5_table_page_break_raw};
use crate::{Hwp5JoinedImageAsset, Hwp5JoinedImageAssetPlan};

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

/// Project decoded HWP5 sections into a Core `Document<Draft>`.
///
/// Returns the document and any warnings accumulated during projection.
pub(crate) fn project_to_core(
    sections: Vec<SectionResult>,
) -> Hwp5Result<(Document<Draft>, Vec<Hwp5Warning>)> {
    let (document, _image_store, warnings) = project_to_core_internal(sections, None)?;
    Ok((document, warnings))
}

/// Project decoded HWP5 sections into Core with the current image slice enabled.
pub(crate) fn project_to_core_with_images(
    sections: Vec<SectionResult>,
    image_assets: &Hwp5JoinedImageAssetPlan,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    project_to_core_internal(sections, Some(image_assets))
}

fn project_to_core_internal(
    sections: Vec<SectionResult>,
    image_assets: Option<&Hwp5JoinedImageAssetPlan>,
) -> Hwp5Result<(Document<Draft>, ImageStore, Vec<Hwp5Warning>)> {
    let mut doc = Document::<Draft>::new();
    let mut all_warnings: Vec<Hwp5Warning> = Vec::new();
    let mut projection_images = ProjectionImageState::new(image_assets);

    for section_result in sections {
        // Collect warnings from decoding.
        all_warnings.extend(section_result.warnings);

        // Convert page definition.
        let page_settings = section_result
            .page_def
            .as_ref()
            .map(page_def_to_settings)
            .unwrap_or_else(PageSettings::a4);

        let mut section = Section::new(page_settings);
        let mut header_paragraphs: Vec<Paragraph> = Vec::new();
        let mut footer_paragraphs: Vec<Paragraph> = Vec::new();

        // Project each paragraph.
        for hwp_para in section_result.paragraphs {
            header_paragraphs.extend(collect_header_paragraphs(&hwp_para, &mut projection_images));
            footer_paragraphs.extend(collect_footer_paragraphs(&hwp_para, &mut projection_images));

            let para = project_paragraph_with_images(
                &hwp_para,
                &mut projection_images,
                ImageProjectionContext::Flow,
            );
            section.add_paragraph(para);
        }

        if !header_paragraphs.is_empty() {
            section.header = Some(HeaderFooter::all_pages(header_paragraphs));
        }
        if !footer_paragraphs.is_empty() {
            section.footer = Some(HeaderFooter::all_pages(footer_paragraphs));
        }

        // Ensure every section has at least one paragraph (validation requirement).
        if section.is_empty() {
            section.add_paragraph(Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ));
        }

        doc.add_section(section);
    }

    // Ensure document has at least one section.
    if doc.is_empty() {
        let mut section = Section::new(PageSettings::a4());
        section.add_paragraph(Paragraph::with_runs(
            vec![Run::text("", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        ));
        doc.add_section(section);
    }

    all_warnings.extend(projection_images.warnings);
    Ok((doc, projection_images.image_store, all_warnings))
}

// ---------------------------------------------------------------------------
// Paragraph projection
// ---------------------------------------------------------------------------

struct ProjectionImageState<'a> {
    image_assets: Option<&'a Hwp5JoinedImageAssetPlan>,
    image_store: ImageStore,
    warnings: Vec<Hwp5Warning>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImageProjectionContext {
    Flow,
    TextBox,
}

impl<'a> ProjectionImageState<'a> {
    fn new(image_assets: Option<&'a Hwp5JoinedImageAssetPlan>) -> Self {
        Self { image_assets, image_store: ImageStore::new(), warnings: Vec::new() }
    }

    fn build_image(
        &mut self,
        image: &Hwp5ImageControl,
        context: ImageProjectionContext,
    ) -> Option<Image> {
        let Some(image_assets): Option<&Hwp5JoinedImageAssetPlan> = self.image_assets else {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: "projection_image_assets_unavailable".to_string(),
            });
            return None;
        };
        let Some(asset): Option<&Hwp5JoinedImageAsset> =
            image_assets.asset_for_binary_data_id(image.binary_data_id)
        else {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: format!("missing_image_asset_for_binary_data_id={}", image.binary_data_id),
            });
            return None;
        };
        let resolved_dimensions: ResolvedImageDimensions =
            resolve_image_dimensions(image, &asset.payload);

        if resolved_dimensions.width_hwp <= 0 || resolved_dimensions.height_hwp <= 0 {
            self.warnings.push(Hwp5Warning::DroppedControl {
                control: "image",
                reason: format!(
                    "image_zero_size_projection binary_data_id={} width={} height={}",
                    image.binary_data_id,
                    resolved_dimensions.width_hwp,
                    resolved_dimensions.height_hwp
                ),
            });
            return None;
        }

        self.image_store.insert(asset.payload.storage_name.clone(), asset.bytes.clone());

        Some(
            Image::new(
                asset.payload.package_path.clone(),
                HwpUnit::new(resolved_dimensions.width_hwp).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(resolved_dimensions.height_hwp).unwrap_or(HwpUnit::ZERO),
                core_image_format(&asset.payload.format),
            )
            .with_placement(image_placement_from_geometry(&image.geometry, context)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ResolvedImageDimensions {
    width_hwp: i32,
    height_hwp: i32,
}

fn resolve_image_dimensions(
    image: &Hwp5ImageControl,
    payload: &crate::Hwp5SemanticImagePayload,
) -> ResolvedImageDimensions {
    let control_width_hwp: Option<i32> = positive_i32_from_u32(image.geometry.width);
    let control_height_hwp: Option<i32> = positive_i32_from_u32(image.geometry.height);
    let joined_width_hwp: Option<i32> = payload.width_hwp.filter(|width| *width > 0);
    let joined_height_hwp: Option<i32> = payload.height_hwp.filter(|height| *height > 0);

    let width_hwp: i32 = control_width_hwp
        .or(joined_width_hwp)
        .unwrap_or_else(|| i32::try_from(image.geometry.width).unwrap_or(0));
    let height_hwp: i32 = control_height_hwp
        .or(joined_height_hwp)
        .unwrap_or_else(|| i32::try_from(image.geometry.height).unwrap_or(0));
    ResolvedImageDimensions { width_hwp, height_hwp }
}

fn image_placement_from_geometry(
    geometry: &Hwp5ShapeComponentGeometry,
    context: ImageProjectionContext,
) -> ImagePlacement {
    match context {
        ImageProjectionContext::TextBox => ImagePlacement {
            text_wrap: ImageTextWrap::Square,
            text_flow: ImageTextFlow::BothSides,
            treat_as_char: false,
            flow_with_text: true,
            allow_overlap: false,
            vert_rel_to: ImageRelativeTo::Para,
            horz_rel_to: ImageRelativeTo::Para,
            vert_offset: HwpUnit::new(geometry.y).unwrap_or(HwpUnit::ZERO),
            horz_offset: HwpUnit::new(geometry.x).unwrap_or(HwpUnit::ZERO),
        },
        ImageProjectionContext::Flow if geometry.x != 0 || geometry.y != 0 => ImagePlacement {
            text_wrap: ImageTextWrap::InFrontOfText,
            text_flow: ImageTextFlow::BothSides,
            treat_as_char: false,
            flow_with_text: false,
            allow_overlap: true,
            vert_rel_to: ImageRelativeTo::Paper,
            horz_rel_to: ImageRelativeTo::Paper,
            vert_offset: HwpUnit::new(geometry.y).unwrap_or(HwpUnit::ZERO),
            horz_offset: HwpUnit::new(geometry.x).unwrap_or(HwpUnit::ZERO),
        },
        ImageProjectionContext::Flow => ImagePlacement::legacy_inline_defaults(),
    }
}

fn project_paragraph_with_images(
    hwp_para: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Paragraph {
    let mut runs: Vec<Run> = Vec::new();
    let mut control_iter = hwp_para.controls.iter();
    let mut segment_start_utf16: u32 = 0;
    let mut current_utf16: u32 = 0;

    for ch in hwp_para.text.chars() {
        let char_utf16_len = ch.len_utf16() as u32;
        if ch == '\u{FFFC}' {
            runs.extend(project_text_segment(
                &hwp_para.text,
                &hwp_para.char_shape_runs,
                segment_start_utf16,
                current_utf16,
            ));

            if let Some(control) = control_iter.next() {
                if let Some(run) = project_control_run(control, projection_images, image_context) {
                    runs.push(run);
                }
            }

            current_utf16 += char_utf16_len;
            segment_start_utf16 = current_utf16;
            continue;
        }

        current_utf16 += char_utf16_len;
    }

    runs.extend(project_text_segment(
        &hwp_para.text,
        &hwp_para.char_shape_runs,
        segment_start_utf16,
        current_utf16,
    ));

    for control in control_iter {
        if let Some(run) = project_control_run(control, projection_images, image_context) {
            runs.push(run);
        }
    }

    if runs.is_empty() {
        runs.push(Run::text("", CharShapeIndex::new(0)));
    }

    let mut paragraph =
        Paragraph::with_runs(runs, ParaShapeIndex::new(hwp_para.para_shape_id as usize));
    if hwp_para.style_id > 0 {
        paragraph = paragraph.with_style(StyleIndex::new(hwp_para.style_id as usize));
    }
    paragraph
}

fn collect_header_paragraphs(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
) -> Vec<Paragraph> {
    collect_subtree_paragraphs(paragraph, projection_images, |control| match control {
        Hwp5Control::Header(subtree) => Some(&subtree.paragraphs),
        _ => None,
    })
}

fn collect_footer_paragraphs(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
) -> Vec<Paragraph> {
    collect_subtree_paragraphs(paragraph, projection_images, |control| match control {
        Hwp5Control::Footer(subtree) => Some(&subtree.paragraphs),
        _ => None,
    })
}

fn collect_subtree_paragraphs<F>(
    paragraph: &Hwp5Paragraph,
    projection_images: &mut ProjectionImageState<'_>,
    paragraphs_for_control: F,
) -> Vec<Paragraph>
where
    F: Fn(&Hwp5Control) -> Option<&Vec<Hwp5Paragraph>>,
{
    let mut projected: Vec<Paragraph> = Vec::new();
    for control in &paragraph.controls {
        if let Some(nested_paragraphs) = paragraphs_for_control(control) {
            projected.extend(project_nested_paragraphs(
                nested_paragraphs,
                projection_images,
                ImageProjectionContext::Flow,
            ));
        }
    }
    projected
}

fn project_nested_paragraphs(
    paragraphs: &[Hwp5Paragraph],
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Vec<Paragraph> {
    paragraphs
        .iter()
        .map(|nested| project_paragraph_with_images(nested, projection_images, image_context))
        .collect()
}

// ---------------------------------------------------------------------------
// Text splitting
// ---------------------------------------------------------------------------

fn project_control_run(
    control: &Hwp5Control,
    projection_images: &mut ProjectionImageState<'_>,
    image_context: ImageProjectionContext,
) -> Option<Run> {
    match control {
        Hwp5Control::Table(table) => Some(Run::table(
            build_table_with_images(table, projection_images),
            CharShapeIndex::new(0),
        )),
        Hwp5Control::Image(image) => projection_images
            .build_image(image, image_context)
            .map(|core_image| Run::image(core_image, CharShapeIndex::new(0))),
        Hwp5Control::Line(line) => Some(project_line_run(line)),
        Hwp5Control::Rect(_) => {
            projection_images.warnings.push(Hwp5Warning::DroppedControl {
                control: "rect",
                reason: "pure_rect_projection_requires_core_hwpx_capability".to_string(),
            });
            None
        }
        Hwp5Control::Polygon(polygon) => Some(project_polygon_run(polygon)),
        Hwp5Control::TextBox(textbox) => Some(project_textbox_run(textbox, projection_images)),
        Hwp5Control::Header(_) | Hwp5Control::Footer(_) | Hwp5Control::Unknown { .. } => None,
        Hwp5Control::OleObject(_) => {
            projection_images.warnings.push(Hwp5Warning::DroppedControl {
                control: "ole_object",
                reason: "ole_projection_not_implemented".to_string(),
            });
            None
        }
    }
}

fn project_textbox_run(
    textbox: &Hwp5TextBoxControl,
    projection_images: &mut ProjectionImageState<'_>,
) -> Run {
    let paragraphs = project_nested_paragraphs(
        &textbox.paragraphs,
        projection_images,
        ImageProjectionContext::TextBox,
    );
    Run::control(
        Control::TextBox {
            paragraphs,
            width: hwp_unit_from_u32(textbox.geometry.width),
            height: hwp_unit_from_u32(textbox.geometry.height),
            horz_offset: textbox.geometry.x,
            vert_offset: textbox.geometry.y,
            caption: None,
            style: None,
        },
        CharShapeIndex::new(0),
    )
}

fn project_line_run(line: &Hwp5LineControl) -> Run {
    let projected_start = scale_point_into_geometry(
        line.start,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_end = scale_point_into_geometry(
        line.end,
        line.start.x.min(line.end.x),
        line.start.x.max(line.end.x),
        line.geometry.width,
        100,
        Axis::Horizontal,
    );
    let projected_start_y = scale_point_into_geometry(
        line.start,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );
    let projected_end_y = scale_point_into_geometry(
        line.end,
        line.start.y.min(line.end.y),
        line.start.y.max(line.end.y),
        line.geometry.height,
        100,
        Axis::Vertical,
    );

    let scaled_start =
        hwpforge_core::control::ShapePoint { x: projected_start, y: projected_start_y };
    let scaled_end = hwpforge_core::control::ShapePoint { x: projected_end, y: projected_end_y };
    let mut control = hwpforge_core::control::Control::line(scaled_start, scaled_end)
        .expect("scaled line points remain non-degenerate");
    if let Control::Line { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = line.geometry.x;
        *vert_offset = line.geometry.y;
    }
    Run::control(control, CharShapeIndex::new(0))
}

fn project_polygon_run(polygon: &Hwp5PolygonControl) -> Run {
    let vertices = scale_polygon_points(&polygon.points, &polygon.geometry);
    let mut control =
        hwpforge_core::control::Control::polygon(vertices).expect("fixture polygon is valid");
    if let Control::Polygon { horz_offset, vert_offset, .. } = &mut control {
        *horz_offset = polygon.geometry.x;
        *vert_offset = polygon.geometry.y;
    }
    Run::control(control, CharShapeIndex::new(0))
}

#[derive(Debug, Clone, Copy)]
enum Axis {
    Horizontal,
    Vertical,
}

fn scale_polygon_points(
    points: &[Hwp5ShapePoint],
    geometry: &Hwp5ShapeComponentGeometry,
) -> Vec<hwpforge_core::control::ShapePoint> {
    let min_x = points.iter().map(|point| point.x).min().unwrap_or(0);
    let max_x = points.iter().map(|point| point.x).max().unwrap_or(0);
    let min_y = points.iter().map(|point| point.y).min().unwrap_or(0);
    let max_y = points.iter().map(|point| point.y).max().unwrap_or(0);

    points
        .iter()
        .map(|point| hwpforge_core::control::ShapePoint {
            x: scale_point_into_geometry(*point, min_x, max_x, geometry.width, 1, Axis::Horizontal),
            y: scale_point_into_geometry(*point, min_y, max_y, geometry.height, 1, Axis::Vertical),
        })
        .collect()
}

fn scale_point_into_geometry(
    point: Hwp5ShapePoint,
    raw_min: i32,
    raw_max: i32,
    geometry_span: u32,
    minimum_target_span: i32,
    axis: Axis,
) -> i32 {
    let raw_span = i64::from(raw_max) - i64::from(raw_min);
    let target_span =
        i64::from(i32::try_from(geometry_span).unwrap_or(i32::MAX).max(minimum_target_span));
    if raw_span <= 0 {
        return 0;
    }

    let raw_value = match axis {
        Axis::Horizontal => point.x,
        Axis::Vertical => point.y,
    };
    let relative = i64::from(raw_value) - i64::from(raw_min);
    let scaled = (relative * target_span + (raw_span / 2)) / raw_span;
    i32::try_from(scaled).unwrap_or(i32::MAX)
}

fn project_text_segment(
    text: &str,
    runs: &[Hwp5CharShapeRun],
    start_utf16: u32,
    end_utf16: u32,
) -> Vec<Run> {
    if start_utf16 >= end_utf16 {
        return Vec::new();
    }

    let boundaries = utf16_boundaries(text);
    let start_byte = utf16_offset_to_byte(&boundaries, start_utf16);
    let end_byte = utf16_offset_to_byte(&boundaries, end_utf16);
    if start_byte >= end_byte {
        return Vec::new();
    }

    let segment = &text[start_byte..end_byte];
    let mut segment_runs: Vec<Hwp5CharShapeRun> = Vec::new();
    let active_char_shape_id = char_shape_id_at_position(runs, start_utf16);
    segment_runs.push(Hwp5CharShapeRun { position: 0, char_shape_id: active_char_shape_id });

    for run in runs {
        if run.position > start_utf16 && run.position < end_utf16 {
            segment_runs.push(Hwp5CharShapeRun {
                position: run.position - start_utf16,
                char_shape_id: run.char_shape_id,
            });
        }
    }

    split_text_by_runs(segment, &segment_runs)
}

fn char_shape_id_at_position(runs: &[Hwp5CharShapeRun], position: u32) -> u32 {
    runs.iter()
        .take_while(|run| run.position <= position)
        .last()
        .map(|run| run.char_shape_id)
        .unwrap_or(0)
}

fn core_image_format(format: &crate::Hwp5SemanticImageFormat) -> ImageFormat {
    match format {
        crate::Hwp5SemanticImageFormat::Png => ImageFormat::Png,
        crate::Hwp5SemanticImageFormat::Jpeg => ImageFormat::Jpeg,
        crate::Hwp5SemanticImageFormat::Gif => ImageFormat::Gif,
        crate::Hwp5SemanticImageFormat::Bmp => ImageFormat::Bmp,
        crate::Hwp5SemanticImageFormat::Wmf => ImageFormat::Wmf,
        crate::Hwp5SemanticImageFormat::Emf => ImageFormat::Emf,
        crate::Hwp5SemanticImageFormat::Unknown(value) => ImageFormat::Unknown(value.clone()),
    }
}

fn hwp_unit_from_u32(value: u32) -> HwpUnit {
    i32::try_from(value).ok().and_then(|signed| HwpUnit::new(signed).ok()).unwrap_or(HwpUnit::ZERO)
}

/// Split paragraph text into runs according to `char_shape_runs`.
///
/// Each run entry marks the starting character position (as a UTF-16
/// code-unit index) of a new character shape. For simplicity this
/// implementation treats the positions as Unicode scalar-value indices,
/// which is accurate for all-ASCII or all-Korean text.
fn split_text_by_runs(text: &str, runs: &[Hwp5CharShapeRun]) -> Vec<Run> {
    if text.is_empty() && runs.is_empty() {
        return vec![];
    }
    if runs.is_empty() {
        return vec![Run::text(text, CharShapeIndex::new(0))];
    }

    let boundaries = utf16_boundaries(text);
    let mut result: Vec<Run> = Vec::with_capacity(runs.len());

    for (i, run) in runs.iter().enumerate() {
        let start = utf16_offset_to_byte(&boundaries, run.position);
        let end = if i + 1 < runs.len() {
            utf16_offset_to_byte(&boundaries, runs[i + 1].position)
        } else {
            text.len()
        };

        if start >= text.len() {
            break;
        }
        let end = end.min(text.len());
        let segment = &text[start..end];
        if !segment.is_empty() {
            result.push(Run::text(segment, CharShapeIndex::new(run.char_shape_id as usize)));
        }
    }

    if result.is_empty() {
        result.push(Run::text(text, CharShapeIndex::new(0)));
    }
    result
}

fn utf16_boundaries(text: &str) -> Vec<(u32, usize)> {
    let mut boundaries = Vec::with_capacity(text.chars().count() + 1);
    let mut utf16_offset = 0u32;

    for (byte_idx, ch) in text.char_indices() {
        boundaries.push((utf16_offset, byte_idx));
        utf16_offset += ch.len_utf16() as u32;
    }
    boundaries.push((utf16_offset, text.len()));
    boundaries
}

fn utf16_offset_to_byte(boundaries: &[(u32, usize)], utf16_offset: u32) -> usize {
    match boundaries.binary_search_by_key(&utf16_offset, |(offset, _)| *offset) {
        Ok(idx) => boundaries[idx].1,
        Err(idx) => boundaries
            .get(idx)
            .map(|(_, byte_idx)| *byte_idx)
            .unwrap_or_else(|| boundaries.last().map(|(_, byte_idx)| *byte_idx).unwrap_or(0)),
    }
}

// ---------------------------------------------------------------------------
// Table construction
// ---------------------------------------------------------------------------

/// Build a structurally minimal table with `rows × cols` empty cells.
fn build_empty_table(table: &Hwp5Table, warnings: &mut Vec<Hwp5Warning>) -> Table {
    let row_count = table.rows.max(1) as usize;
    let col_count = table.cols.max(1) as usize;

    let table_rows: Vec<TableRow> = (0..row_count)
        .map(|_| {
            let cells: Vec<TableCell> = (0..col_count)
                .map(|_| {
                    TableCell::new(
                        vec![Paragraph::with_runs(
                            vec![Run::text("", CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        )],
                        HwpUnit::ZERO,
                    )
                })
                .collect();
            TableRow::new(cells)
        })
        .collect();

    let mut core_table = Table::new(table_rows);
    apply_table_projection_metadata(table, &mut core_table, warnings);
    core_table
}

fn build_table_with_images(
    table: &Hwp5Table,
    projection_images: &mut ProjectionImageState<'_>,
) -> Table {
    if table.cells.is_empty() {
        return build_empty_table(table, &mut projection_images.warnings);
    }

    let inferred_rows =
        table.cells.iter().map(|cell| cell.row.saturating_add(cell.row_span)).max().unwrap_or(0);
    let row_count = table.rows.max(inferred_rows).max(1) as usize;

    let mut grouped: Vec<Vec<&Hwp5TableCell>> = vec![Vec::new(); row_count];
    for cell in &table.cells {
        let row_idx = cell.row as usize;
        if row_idx >= grouped.len() {
            grouped.resize(row_idx + 1, Vec::new());
        }
        grouped[row_idx].push(cell);
    }

    let rows = grouped
        .into_iter()
        .map(|mut cells| {
            cells.sort_by_key(|cell| cell.column);
            let row_is_header = projected_row_is_header(&cells, &mut projection_images.warnings);
            let projected = if cells.is_empty() {
                vec![empty_cell()]
            } else {
                cells
                    .iter()
                    .copied()
                    .map(|cell| project_table_cell_with_images(cell, projection_images))
                    .collect()
            };
            let row_height = cells.iter().map(|cell| cell.height).max().unwrap_or(0);
            match HwpUnit::new(row_height) {
                Ok(height) if row_height > 0 => {
                    TableRow::with_height(projected, height).with_header(row_is_header)
                }
                _ => TableRow::new(projected).with_header(row_is_header),
            }
        })
        .collect();

    let mut core_table = Table::new(rows);
    apply_table_projection_metadata(table, &mut core_table, &mut projection_images.warnings);
    core_table
}

fn projected_row_is_header(cells: &[&Hwp5TableCell], warnings: &mut Vec<Hwp5Warning>) -> bool {
    if cells.is_empty() {
        return false;
    }

    let header_count = cells.iter().filter(|cell| cell.is_header).count();
    if header_count == 0 {
        false
    } else if header_count == cells.len() {
        true
    } else {
        warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "table.header_row",
            reason: format!(
                "mixed_hwp5_table_header_cells row={} header_cells={} total_cells={}; defaulting_to=non_header_row",
                cells[0].row,
                header_count,
                cells.len()
            ),
        });
        false
    }
}

fn apply_table_projection_metadata(
    table: &Hwp5Table,
    core_table: &mut Table,
    warnings: &mut Vec<Hwp5Warning>,
) {
    core_table.repeat_header = table.repeat_header;
    core_table.cell_spacing = (table.cell_spacing > 0)
        .then(|| HwpUnit::new(i32::from(table.cell_spacing)))
        .transpose()
        .unwrap_or(None);
    core_table.border_fill_id = table.border_fill_id.map(u32::from);

    match core_table_page_break(table.page_break) {
        Some(page_break) => core_table.page_break = page_break,
        None => warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "table.page_break",
            reason: format!(
                "unknown_hwp5_table_page_break_raw={}; defaulting_to=cell",
                unknown_hwp5_table_page_break_raw(table.page_break)
                    .expect("known table page-break values must not use projection fallback",),
            ),
        }),
    }
}

fn project_table_cell_with_images(
    cell: &Hwp5TableCell,
    projection_images: &mut ProjectionImageState<'_>,
) -> TableCell {
    let width = HwpUnit::new(cell.width).unwrap_or(HwpUnit::ZERO);
    let paragraphs = if cell.paragraphs.is_empty() {
        vec![empty_paragraph()]
    } else {
        cell.paragraphs
            .iter()
            .map(|paragraph| {
                project_paragraph_with_images(
                    paragraph,
                    projection_images,
                    ImageProjectionContext::Flow,
                )
            })
            .collect()
    };

    let mut core_cell =
        TableCell::with_span(paragraphs, width, cell.col_span.max(1), cell.row_span.max(1));
    core_cell.height =
        (cell.height > 0).then(|| HwpUnit::new(cell.height)).transpose().unwrap_or(None);
    core_cell.border_fill_id = cell.border_fill_id.map(u32::from);
    core_cell.margin = Some(TableMargin {
        left: HwpUnit::new(i32::from(cell.margin.left)).unwrap_or(HwpUnit::ZERO),
        right: HwpUnit::new(i32::from(cell.margin.right)).unwrap_or(HwpUnit::ZERO),
        top: HwpUnit::new(i32::from(cell.margin.top)).unwrap_or(HwpUnit::ZERO),
        bottom: HwpUnit::new(i32::from(cell.margin.bottom)).unwrap_or(HwpUnit::ZERO),
    });
    match core_table_cell_vertical_align(cell.vertical_align) {
        Some(vertical_align) => core_cell.vertical_align = Some(vertical_align),
        None => projection_images.warnings.push(Hwp5Warning::ProjectionFallback {
            subject: "table.cell.vertical_align",
            reason: format!(
                "row={} col={} unknown_hwp5_table_cell_vertical_align_raw={}; dropping_vertical_align",
                cell.row,
                cell.column,
                unknown_hwp5_table_cell_vertical_align_raw(cell.vertical_align).expect(
                    "known table cell vertical-align values must not use projection fallback",
                ),
            ),
        }),
    }
    core_cell
}

fn empty_paragraph() -> Paragraph {
    Paragraph::with_runs(vec![Run::text("", CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn empty_cell() -> TableCell {
    TableCell::new(vec![empty_paragraph()], HwpUnit::ZERO)
}

// ---------------------------------------------------------------------------
// PageDef → PageSettings
// ---------------------------------------------------------------------------

/// Convert an `Hwp5PageDef` (raw HWP5 units) into Core `PageSettings`.
///
/// HWP5 page dimensions are already in HwpUnit (720ths of an inch).
/// `HwpUnit::new` rejects values outside ±100,000,000; for the rare case
/// where a malformed file has an out-of-range value, `HwpUnit::ZERO` is
/// used as a safe fallback.
fn page_def_to_settings(pd: &Hwp5PageDef) -> PageSettings {
    let u = |v: u32| HwpUnit::new(v as i32).unwrap_or(HwpUnit::ZERO);
    PageSettings {
        width: u(pd.width),
        height: u(pd.height),
        margin_left: u(pd.margin_left),
        margin_right: u(pd.margin_right),
        margin_top: u(pd.margin_top),
        margin_bottom: u(pd.margin_bottom),
        header_margin: u(pd.header_margin),
        footer_margin: u(pd.footer_margin),
        gutter: u(pd.gutter),
        landscape: pd.landscape,
        ..PageSettings::a4()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    use std::collections::BTreeMap;

    use hwpforge_core::table::TablePageBreak;

    use crate::decoder::section::{
        Hwp5ImageControl, Hwp5LineControl, Hwp5PolygonControl, Hwp5TablePageBreak,
        Hwp5TextBoxControl,
    };
    use crate::Hwp5SemanticImageFormat;

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn make_paragraph(text: &str, para_shape_id: u16, style_id: u8) -> Hwp5Paragraph {
        Hwp5Paragraph {
            text: text.to_string(),
            para_shape_id,
            style_id,
            char_shape_runs: vec![],
            controls: vec![],
        }
    }

    fn _make_paragraph_with_runs(text: &str, runs: Vec<Hwp5CharShapeRun>) -> Hwp5Paragraph {
        Hwp5Paragraph {
            text: text.to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: runs,
            controls: vec![],
        }
    }

    fn make_section(
        paragraphs: Vec<Hwp5Paragraph>,
        page_def: Option<Hwp5PageDef>,
    ) -> SectionResult {
        SectionResult { paragraphs, page_def, warnings: vec![] }
    }

    fn hwp5_char_run(position: u32, char_shape_id: u32) -> Hwp5CharShapeRun {
        Hwp5CharShapeRun { position, char_shape_id }
    }

    fn image_plan<'a>(
        assets: impl IntoIterator<Item = (u16, &'a str, Hwp5SemanticImageFormat, Vec<u8>)>,
    ) -> Hwp5JoinedImageAssetPlan {
        let ordered_assets: Vec<Hwp5JoinedImageAsset> = assets
            .into_iter()
            .map(|(binary_data_id, storage_name, format, bytes)| Hwp5JoinedImageAsset {
                payload: crate::Hwp5SemanticImagePayload {
                    binary_data_id,
                    storage_name: storage_name.to_string(),
                    package_path: format!("BinData/{storage_name}"),
                    format,
                    width_hwp: None,
                    height_hwp: None,
                },
                bytes,
            })
            .collect();
        let assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> = ordered_assets
            .iter()
            .cloned()
            .map(|asset| (asset.payload.binary_data_id, asset))
            .collect();
        Hwp5JoinedImageAssetPlan { ordered_assets, assets_by_binary_data_id }
    }

    fn image_plan_with_dimensions(
        binary_data_id: u16,
        storage_name: &str,
        format: Hwp5SemanticImageFormat,
        width_hwp: Option<i32>,
        height_hwp: Option<i32>,
        bytes: Vec<u8>,
    ) -> Hwp5JoinedImageAssetPlan {
        let asset = Hwp5JoinedImageAsset {
            payload: crate::Hwp5SemanticImagePayload {
                binary_data_id,
                storage_name: storage_name.to_string(),
                package_path: format!("BinData/{storage_name}"),
                format,
                width_hwp,
                height_hwp,
            },
            bytes,
        };
        let assets_by_binary_data_id: BTreeMap<u16, Hwp5JoinedImageAsset> =
            [(binary_data_id, asset.clone())].into_iter().collect();
        Hwp5JoinedImageAssetPlan { ordered_assets: vec![asset], assets_by_binary_data_id }
    }

    // ── project_to_core ───────────────────────────────────────────────────────

    #[test]
    fn empty_sections_produces_default_document() {
        let (doc, warnings) = project_to_core(vec![]).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.section_count(), 1, "empty input must produce 1 fallback section");
        assert_eq!(doc.sections()[0].paragraph_count(), 1);
    }

    #[test]
    fn single_section_with_one_paragraph() {
        let para = make_paragraph("Hello", 3, 0);
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(doc.section_count(), 1);
        let s = &doc.sections()[0];
        assert_eq!(s.paragraph_count(), 1);
        let p = &s.paragraphs[0];
        assert_eq!(p.para_shape_id, ParaShapeIndex::new(3));
        assert_eq!(p.text_content(), "Hello");
    }

    #[test]
    fn style_id_zero_maps_to_none() {
        let para = make_paragraph("text", 0, 0);
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraphs[0].style_id, None);
    }

    #[test]
    fn style_id_nonzero_maps_to_some() {
        let para = make_paragraph("text", 0, 5);
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraphs[0].style_id, Some(StyleIndex::new(5)));
    }

    #[test]
    fn multiple_sections_preserved() {
        let s1 = make_section(vec![make_paragraph("A", 0, 0)], None);
        let s2 = make_section(vec![make_paragraph("B", 0, 0)], None);
        let s3 = make_section(vec![make_paragraph("C", 0, 0)], None);
        let (doc, _) = project_to_core(vec![s1, s2, s3]).unwrap();
        assert_eq!(doc.section_count(), 3);
    }

    #[test]
    fn empty_section_gets_fallback_paragraph() {
        let section = make_section(vec![], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].paragraph_count(), 1);
    }

    #[test]
    fn warnings_are_collected() {
        let warn = Hwp5Warning::UnsupportedTag { tag_id: 0xAB, offset: 0 };
        let section = SectionResult {
            paragraphs: vec![make_paragraph("x", 0, 0)],
            page_def: None,
            warnings: vec![warn],
        };
        let (_, warnings) = project_to_core(vec![section]).unwrap();
        assert_eq!(warnings.len(), 1);
    }

    #[test]
    fn project_to_core_with_images_preserves_inline_order_and_populates_store() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 3_000,
                height: 2_000,
            },
            binary_data_id: 1,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "앞\u{fffc}뒤".to_string(),
                para_shape_id: 3,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![image],
            }],
            None,
        );
        let image_assets = image_plan([(
            1,
            "BIN0001.png",
            Hwp5SemanticImageFormat::Png,
            vec![0x89, 0x50, 0x4E, 0x47],
        )]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.len(), 1);
        assert_eq!(image_store.get("BIN0001.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.runs.len(), 3);
        assert_eq!(paragraph.runs[0].content.as_text(), Some("앞"));
        assert!(paragraph.runs[1].content.is_image());
        assert_eq!(paragraph.runs[2].content.as_text(), Some("뒤"));

        let image = paragraph.runs[1].content.as_image().expect("middle run should be image");
        assert_eq!(image.path, "BinData/BIN0001.png");
        assert_eq!(image.width, HwpUnit::new(3_000).unwrap());
        assert_eq!(image.height, HwpUnit::new(2_000).unwrap());
        let placement = image.placement.as_ref().expect("placement should be attached");
        assert!(placement.treat_as_char);
        assert_eq!(placement.text_wrap, ImageTextWrap::TopAndBottom);
        assert_eq!(placement.horz_rel_to, ImageRelativeTo::Para);
        assert_eq!(placement.vert_rel_to, ImageRelativeTo::Para);
        assert_eq!(document.sections()[0].content_counts().images, 1);
    }

    #[test]
    fn project_to_core_with_images_projects_header_and_footer_subtrees() {
        let header_image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_200,
                height: 800,
            },
            binary_data_id: 7,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}\u{fffc}".to_string(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![
                    Hwp5Control::Header(crate::decoder::section::Hwp5NestedSubtree {
                        ctrl_id: 0x6865_6164,
                        paragraphs: vec![Hwp5Paragraph {
                            text: "\u{fffc}".to_string(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: Vec::new(),
                            controls: vec![header_image],
                        }],
                    }),
                    Hwp5Control::Footer(crate::decoder::section::Hwp5NestedSubtree {
                        ctrl_id: 0x666F_6F74,
                        paragraphs: vec![make_paragraph("꼬리말 테스트", 0, 0)],
                    }),
                ],
            }],
            None,
        );
        let image_assets =
            image_plan([(7, "BIN0007.png", Hwp5SemanticImageFormat::Png, vec![1, 2, 3, 4])]);

        let (document, image_store, _) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        let section = &document.sections()[0];
        let header = section.header.as_ref().expect("header should be projected");
        let footer = section.footer.as_ref().expect("footer should be projected");

        assert_eq!(image_store.get("BIN0007.png"), Some(&[1, 2, 3, 4][..]));
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs.len(), 1);
        assert!(header.paragraphs[0].runs[0].content.is_image());
        assert_eq!(footer.paragraphs[0].text_content(), "꼬리말 테스트");
    }

    #[test]
    fn project_to_core_with_images_projects_textbox_with_nested_image() {
        let nested_image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_500,
                height: 900,
            },
            binary_data_id: 3,
        });
        let textbox = Hwp5Control::TextBox(Hwp5TextBoxControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 50,
                y: 60,
                width: 8_000,
                height: 6_000,
            },
            paragraphs: vec![Hwp5Paragraph {
                text: "앞\u{fffc}뒤".to_string(),
                para_shape_id: 1,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![nested_image],
            }],
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![textbox],
            }],
            None,
        );
        let image_assets =
            image_plan([(3, "BIN0003.png", Hwp5SemanticImageFormat::Png, vec![9, 8, 7])]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0003.png"), Some(&[9, 8, 7][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        assert_eq!(paragraph.runs.len(), 1);
        let textbox_control =
            paragraph.runs[0].content.as_control().expect("textbox should project as control");
        match textbox_control {
            Control::TextBox { paragraphs, width, height, horz_offset, vert_offset, .. } => {
                assert_eq!(width, &HwpUnit::new(8_000).unwrap());
                assert_eq!(height, &HwpUnit::new(6_000).unwrap());
                assert_eq!(*horz_offset, 50);
                assert_eq!(*vert_offset, 60);
                assert_eq!(paragraphs.len(), 1);
                assert_eq!(paragraphs[0].runs.len(), 3);
                assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("앞"));
                let nested_image =
                    paragraphs[0].runs[1].content.as_image().expect("middle run should be image");
                let placement =
                    nested_image.placement.as_ref().expect("textbox image should have placement");
                assert_eq!(placement.text_wrap, ImageTextWrap::Square);
                assert_eq!(placement.text_flow, ImageTextFlow::BothSides);
                assert!(!placement.treat_as_char);
                assert!(placement.flow_with_text);
                assert!(!placement.allow_overlap);
                assert_eq!(placement.horz_rel_to, ImageRelativeTo::Para);
                assert_eq!(placement.vert_rel_to, ImageRelativeTo::Para);
                assert_eq!(paragraphs[0].runs[2].content.as_text(), Some("뒤"));
            }
            other => panic!("expected TextBox control, got {:?}", other),
        }
    }

    #[test]
    fn project_to_core_with_images_warns_when_image_asset_join_is_missing() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 1_000,
                height: 800,
            },
            binary_data_id: 99,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![image],
            }],
            None,
        );

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_plan([])).unwrap();
        assert!(image_store.is_empty());
        assert_eq!(document.sections()[0].paragraphs[0].runs.len(), 1);
        assert_eq!(document.sections()[0].paragraphs[0].text_content(), "");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control, reason }
                if *control == "image"
                    && reason == "missing_image_asset_for_binary_data_id=99"
        )));
    }

    #[test]
    fn project_to_core_with_images_falls_back_to_joined_asset_dimensions() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 5,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![image],
            }],
            None,
        );
        let image_assets = image_plan_with_dimensions(
            5,
            "BIN0005.png",
            Hwp5SemanticImageFormat::Png,
            Some(3_210),
            Some(4_560),
            vec![0x89, 0x50, 0x4E, 0x47],
        );

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(image_store.get("BIN0005.png"), Some(&[0x89, 0x50, 0x4E, 0x47][..]));

        let paragraph = &document.sections()[0].paragraphs[0];
        let image = paragraph.runs[0].content.as_image().expect("run should be image");
        assert_eq!(image.width, HwpUnit::new(3_210).unwrap());
        assert_eq!(image.height, HwpUnit::new(4_560).unwrap());
    }

    #[test]
    fn project_to_core_with_images_drops_zero_sized_image_without_fallback() {
        let image = Hwp5Control::Image(Hwp5ImageControl {
            ctrl_id: 0x6773_6F20,
            geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            binary_data_id: 6,
        });
        let section = make_section(
            vec![Hwp5Paragraph {
                text: "\u{fffc}".to_string(),
                para_shape_id: 0,
                style_id: 0,
                char_shape_runs: Vec::new(),
                controls: vec![image],
            }],
            None,
        );

        let image_assets = image_plan([(6, "BIN0006.png", Hwp5SemanticImageFormat::Png, vec![1])]);

        let (document, image_store, warnings) =
            project_to_core_with_images(vec![section], &image_assets).unwrap();
        assert!(image_store.is_empty());
        assert_eq!(document.sections()[0].paragraphs[0].runs.len(), 1);
        assert_eq!(document.sections()[0].paragraphs[0].text_content(), "");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control, reason }
                if *control == "image"
                    && reason == "image_zero_size_projection binary_data_id=6 width=0 height=0"
        )));
    }

    // ── page_def_to_settings ─────────────────────────────────────────────────

    #[test]
    fn page_def_dimensions_are_preserved() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 5669,
            margin_right: 5669,
            margin_top: 5669,
            margin_bottom: 5669,
            header_margin: 2835,
            footer_margin: 2835,
            gutter: 0,
            landscape: false,
        };
        let ps = page_def_to_settings(&pd);
        assert_eq!(ps.width, HwpUnit::new(59535).unwrap());
        assert_eq!(ps.height, HwpUnit::new(84183).unwrap());
        assert_eq!(ps.margin_left, HwpUnit::new(5669).unwrap());
        assert!(!ps.landscape);
    }

    #[test]
    fn page_def_landscape_flag_propagated() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 0,
            margin_right: 0,
            margin_top: 0,
            margin_bottom: 0,
            header_margin: 0,
            footer_margin: 0,
            gutter: 0,
            landscape: true,
        };
        let ps = page_def_to_settings(&pd);
        assert!(ps.landscape);
    }

    #[test]
    fn section_with_page_def_uses_it() {
        let pd = Hwp5PageDef {
            width: 59535,
            height: 84183,
            margin_left: 5669,
            margin_right: 5669,
            margin_top: 5669,
            margin_bottom: 5669,
            header_margin: 2835,
            footer_margin: 2835,
            gutter: 0,
            landscape: false,
        };
        let section = make_section(vec![make_paragraph("x", 0, 0)], Some(pd));
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].page_settings.width, HwpUnit::new(59535).unwrap());
    }

    #[test]
    fn section_without_page_def_defaults_to_a4() {
        let section = make_section(vec![make_paragraph("x", 0, 0)], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        assert_eq!(doc.sections()[0].page_settings, PageSettings::a4());
    }

    // ── split_text_by_runs ────────────────────────────────────────────────────

    #[test]
    fn split_empty_text_empty_runs() {
        let result = split_text_by_runs("", &[]);
        assert!(result.is_empty());
    }

    #[test]
    fn split_text_no_runs_returns_single_run() {
        let result = split_text_by_runs("Hello", &[]);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(0));
    }

    #[test]
    fn split_single_run_covers_all_text() {
        let runs = vec![hwp5_char_run(0, 7)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(7));
    }

    #[test]
    fn split_two_runs() {
        // "HelloWorld" split at position 5
        let runs = vec![hwp5_char_run(0, 2), hwp5_char_run(5, 3)];
        let result = split_text_by_runs("HelloWorld", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(2));
        assert_eq!(result[1].content.as_text(), Some("World"));
        assert_eq!(result[1].char_shape_id, CharShapeIndex::new(3));
    }

    #[test]
    fn split_run_start_beyond_text_length_ignored() {
        // Run starting at position 100 in a 5-char string → ignored.
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(100, 2)];
        let result = split_text_by_runs("Hello", &runs);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content.as_text(), Some("Hello"));
        assert_eq!(result[0].char_shape_id, CharShapeIndex::new(1));
    }

    #[test]
    fn split_korean_text_by_runs() {
        // "안녕하세요" = 5 chars; split at char 2
        let runs = vec![hwp5_char_run(0, 10), hwp5_char_run(2, 11)];
        let result = split_text_by_runs("안녕하세요", &runs);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].content.as_text(), Some("안녕"));
        assert_eq!(result[1].content.as_text(), Some("하세요"));
    }

    #[test]
    fn split_text_by_utf16_code_units_handles_surrogate_pairs() {
        let runs = vec![hwp5_char_run(0, 1), hwp5_char_run(1, 2), hwp5_char_run(3, 3)];
        let result = split_text_by_runs("A😀B", &runs);
        assert_eq!(result.len(), 3);
        assert_eq!(result[0].content.as_text(), Some("A"));
        assert_eq!(result[1].content.as_text(), Some("😀"));
        assert_eq!(result[2].content.as_text(), Some("B"));
    }

    // ── table controls ────────────────────────────────────────────────────────

    #[test]
    fn table_control_becomes_run_table() {
        let para = Hwp5Paragraph {
            text: String::new(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 2,
                cols: 3,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 120,
                border_fill_id: Some(8),
                cells: vec![],
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        let table_run = p.runs.iter().find(|r| r.content.is_table());
        assert!(table_run.is_some(), "expected a table run");
        let table = table_run.unwrap().content.as_table().unwrap();
        assert_eq!(table.row_count(), 2);
        assert_eq!(table.col_count(), 3);
        assert_eq!(table.page_break, TablePageBreak::None);
        assert!(!table.repeat_header);
        assert_eq!(table.cell_spacing, Some(HwpUnit::new(120).unwrap()));
        assert_eq!(table.border_fill_id, Some(8));
    }

    #[test]
    fn table_cell_text_is_projected() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    column: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    width: 4000,
                    height: 1000,
                    is_header: true,
                    margin: crate::decoder::section::Hwp5TableCellMargin {
                        left: 0,
                        right: 0,
                        top: 0,
                        bottom: 0,
                    },
                    vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                    border_fill_id: Some(3),
                    paragraphs: vec![Hwp5Paragraph {
                        text: "셀".to_string(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        controls: vec![],
                    }],
                }],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        assert_eq!(p.text_content(), "", "control placeholder text should be stripped");

        let table =
            p.runs.iter().find_map(|run| run.content.as_table()).expect("expected table run");
        assert_eq!(table.rows[0].cells[0].paragraphs[0].text_content(), "셀");
        assert_eq!(table.rows[0].cells[0].height, Some(HwpUnit::new(1000).unwrap()));
        assert_eq!(table.rows[0].cells[0].border_fill_id, Some(3));
        assert_eq!(
            table.rows[0].cells[0].margin,
            Some(TableMargin {
                left: HwpUnit::new(0).unwrap(),
                right: HwpUnit::new(0).unwrap(),
                top: HwpUnit::new(0).unwrap(),
                bottom: HwpUnit::new(0).unwrap(),
            })
        );
        assert_eq!(
            table.rows[0].cells[0].vertical_align,
            Some(hwpforge_core::table::TableVerticalAlign::Center)
        );
    }

    #[test]
    fn unknown_table_cell_vertical_align_emits_projection_fallback_warning() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![Hwp5TableCell {
                    column: 0,
                    row: 0,
                    col_span: 1,
                    row_span: 1,
                    width: 4000,
                    height: 1000,
                    is_header: false,
                    margin: crate::decoder::section::Hwp5TableCellMargin {
                        left: 10,
                        right: 20,
                        top: 30,
                        bottom: 40,
                    },
                    vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Unknown(3),
                    border_fill_id: Some(3),
                    paragraphs: vec![Hwp5Paragraph {
                        text: "셀".to_string(),
                        para_shape_id: 0,
                        style_id: 0,
                        char_shape_runs: vec![],
                        controls: vec![],
                    }],
                }],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        let table =
            p.runs.iter().find_map(|run| run.content.as_table()).expect("expected table run");
        assert_eq!(
            table.rows[0].cells[0].margin,
            Some(TableMargin {
                left: HwpUnit::new(10).unwrap(),
                right: HwpUnit::new(20).unwrap(),
                top: HwpUnit::new(30).unwrap(),
                bottom: HwpUnit::new(40).unwrap(),
            })
        );
        assert_eq!(table.rows[0].cells[0].vertical_align, None);
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.cell.vertical_align"
                    && reason
                        == "row=0 col=0 unknown_hwp5_table_cell_vertical_align_raw=3; dropping_vertical_align"
        )));
    }

    #[test]
    fn mixed_table_header_cells_emit_warning_and_do_not_promote_header_row() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Table(Hwp5Table {
                rows: 1,
                cols: 2,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![
                    Hwp5TableCell {
                        column: 0,
                        row: 0,
                        col_span: 1,
                        row_span: 1,
                        width: 4000,
                        height: 1000,
                        is_header: true,
                        margin: crate::decoder::section::Hwp5TableCellMargin {
                            left: 0,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        },
                        vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                        border_fill_id: Some(3),
                        paragraphs: vec![Hwp5Paragraph {
                            text: "head".to_string(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: vec![],
                            controls: vec![],
                        }],
                    },
                    Hwp5TableCell {
                        column: 1,
                        row: 0,
                        col_span: 1,
                        row_span: 1,
                        width: 4000,
                        height: 1000,
                        is_header: false,
                        margin: crate::decoder::section::Hwp5TableCellMargin {
                            left: 0,
                            right: 0,
                            top: 0,
                            bottom: 0,
                        },
                        vertical_align: crate::decoder::section::Hwp5TableCellVerticalAlign::Center,
                        border_fill_id: Some(3),
                        paragraphs: vec![Hwp5Paragraph {
                            text: "body".to_string(),
                            para_shape_id: 0,
                            style_id: 0,
                            char_shape_runs: vec![],
                            controls: vec![],
                        }],
                    },
                ],
            })],
        };

        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let table = doc.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|run| run.content.as_table())
            .expect("expected table run");
        assert!(!table.rows[0].is_header, "mixed header cells must not promote header row");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.header_row"
                    && reason == "mixed_hwp5_table_header_cells row=0 header_cells=1 total_cells=2; defaulting_to=non_header_row"
        )));
    }

    #[test]
    fn line_control_becomes_visible_core_line() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Line(Hwp5LineControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 9_884,
                    y: 11_980,
                    width: 29_360,
                    height: 0,
                },
                start: crate::schema::section::Hwp5ShapePoint { x: 0, y: 0 },
                end: crate::schema::section::Hwp5ShapePoint { x: 100, y: 100 },
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Line { start, end, width, height, horz_offset, vert_offset, .. } => {
                assert_eq!(*start, hwpforge_core::control::ShapePoint { x: 0, y: 0 });
                assert_eq!(*end, hwpforge_core::control::ShapePoint { x: 29_360, y: 100 });
                assert_eq!(*width, HwpUnit::new(29_360).unwrap());
                assert_eq!(*height, HwpUnit::new(100).unwrap());
                assert_eq!(*horz_offset, 9_884);
                assert_eq!(*vert_offset, 11_980);
            }
            other => panic!("expected Line control, got {:?}", other),
        }
    }

    #[test]
    fn polygon_control_becomes_visible_core_polygon() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Polygon(Hwp5PolygonControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 17_804,
                    y: 13_900,
                    width: 12_560,
                    height: 13_040,
                },
                points: vec![
                    crate::schema::section::Hwp5ShapePoint { x: 1_882, y: 0 },
                    crate::schema::section::Hwp5ShapePoint { x: 0, y: 1_405 },
                    crate::schema::section::Hwp5ShapePoint { x: 732, y: 3_675 },
                    crate::schema::section::Hwp5ShapePoint { x: 3_032, y: 3_675 },
                    crate::schema::section::Hwp5ShapePoint { x: 3_765, y: 1_405 },
                    crate::schema::section::Hwp5ShapePoint { x: 1_882, y: 0 },
                ],
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        let control = paragraph.runs[0].content.as_control().expect("expected control run");
        match control {
            Control::Polygon {
                vertices,
                width,
                height,
                horz_offset,
                vert_offset,
                paragraphs,
                ..
            } => {
                assert_eq!(vertices.len(), 6);
                assert_eq!(vertices[0], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(vertices[5], hwpforge_core::control::ShapePoint { x: 6_278, y: 0 });
                assert_eq!(*width, HwpUnit::new(12_560).unwrap());
                assert_eq!(*height, HwpUnit::new(13_040).unwrap());
                assert_eq!(*horz_offset, 17_804);
                assert_eq!(*vert_offset, 13_900);
                assert!(paragraphs.is_empty());
            }
            other => panic!("expected Polygon control, got {:?}", other),
        }
    }

    #[test]
    fn rect_control_emits_projection_warning_and_stays_invisible() {
        let para = Hwp5Paragraph {
            text: "\u{FFFC}".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Rect(crate::decoder::section::Hwp5RectControl {
                ctrl_id: 0x6773_6F20,
                geometry: crate::schema::section::Hwp5ShapeComponentGeometry {
                    x: 13_200,
                    y: 14_280,
                    width: 10_020,
                    height: 8_000,
                },
            })],
        };
        let section = make_section(vec![para], None);
        let (doc, warnings) = project_to_core(vec![section]).unwrap();
        let paragraph = &doc.sections()[0].paragraphs[0];
        assert!(paragraph.runs.iter().all(|run| run.content.is_text()));
        assert_eq!(paragraph.text_content(), "");
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::DroppedControl { control, reason }
                if *control == "rect"
                    && reason == "pure_rect_projection_requires_core_hwpx_capability"
        )));
    }

    #[test]
    fn unknown_control_is_ignored() {
        let para = Hwp5Paragraph {
            text: "text".to_string(),
            para_shape_id: 0,
            style_id: 0,
            char_shape_runs: vec![],
            controls: vec![Hwp5Control::Unknown { ctrl_id: 0xDEAD_BEEF }],
        };
        let section = make_section(vec![para], None);
        let (doc, _) = project_to_core(vec![section]).unwrap();
        let p = &doc.sections()[0].paragraphs[0];
        // Only one text run; no table run.
        assert!(p.runs.iter().all(|r| r.content.is_text()));
        assert_eq!(p.text_content(), "text");
    }

    // ── build_empty_table ─────────────────────────────────────────────────────

    #[test]
    fn build_empty_table_correct_dimensions() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 3,
                cols: 4,
                page_break: Hwp5TablePageBreak::Cell,
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.row_count(), 3);
        assert_eq!(t.col_count(), 4);
        assert_eq!(t.page_break, TablePageBreak::Cell);
        assert!(t.repeat_header);
        assert_eq!(t.cell_spacing, None);
        assert_eq!(t.border_fill_id, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_zero_rows_clamps_to_one() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 0,
                cols: 2,
                page_break: Hwp5TablePageBreak::None,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.row_count(), 1);
        assert_eq!(t.page_break, TablePageBreak::None);
        assert!(!t.repeat_header);
        assert_eq!(t.cell_spacing, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_zero_cols_clamps_to_one() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 2,
                cols: 0,
                page_break: Hwp5TablePageBreak::Table,
                repeat_header: false,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.col_count(), 1);
        assert_eq!(t.page_break, TablePageBreak::Table);
        assert_eq!(t.cell_spacing, None);
        assert!(warnings.is_empty());
    }

    #[test]
    fn build_empty_table_unknown_page_break_emits_projection_fallback_warning() {
        let mut warnings: Vec<Hwp5Warning> = Vec::new();
        let t = build_empty_table(
            &Hwp5Table {
                rows: 1,
                cols: 1,
                page_break: Hwp5TablePageBreak::Unknown(3),
                repeat_header: true,
                cell_spacing: 0,
                border_fill_id: None,
                cells: vec![],
            },
            &mut warnings,
        );
        assert_eq!(t.page_break, TablePageBreak::Cell);
        assert_eq!(warnings.len(), 1);
        assert!(warnings.iter().any(|warning| matches!(
            warning,
            Hwp5Warning::ProjectionFallback { subject, reason }
                if *subject == "table.page_break"
                    && reason == "unknown_hwp5_table_page_break_raw=3; defaulting_to=cell"
        )));
    }
}
