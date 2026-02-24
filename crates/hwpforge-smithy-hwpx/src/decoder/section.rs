//! Parses `Contents/section*.xml` into Core paragraphs and page settings.
//!
//! Converts XML schema types (`HxParagraph`, `HxRun`, `HxText`, `HxTable`,
//! `HxPic`) into Core types (`Paragraph`, `Run`, `RunContent`, `Table`, `Image`).

use std::collections::HashMap;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::{ColumnDef, ColumnLayoutMode, ColumnSettings, ColumnType};
use hwpforge_core::control::{Control, ShapeStyle};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, PageNumber};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    ApplyPageType, CharShapeIndex, HwpUnit, NumberFormatType, PageNumberPosition, ParaShapeIndex,
};
use quick_xml::de::from_str;

use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxCaption, HxChart, HxCtrl, HxEllipse, HxEquation, HxFillBrush, HxFootNote, HxHeaderFooter,
    HxLine, HxLineShape, HxPageNum, HxParagraph, HxPic, HxPolygon, HxRect, HxRun, HxSection,
    HxSubList, HxTable, HxTableCell,
};

/// Maximum nesting depth for tables-within-tables.
///
/// Prevents stack overflow from maliciously crafted HWPX files with
/// deeply nested table structures. 32 levels is far beyond any
/// legitimate document.
const MAX_NESTING_DEPTH: usize = 32;

/// Result of parsing a section XML file.
#[derive(Debug)]
pub struct SectionParseResult {
    /// Paragraphs extracted from the section.
    pub paragraphs: Vec<Paragraph>,
    /// Page settings extracted from `<hp:secPr>`, if present.
    pub page_settings: Option<PageSettings>,
    /// Header extracted from `<hp:ctrl><hp:header>`, if present.
    pub header: Option<HeaderFooter>,
    /// Footer extracted from `<hp:ctrl><hp:footer>`, if present.
    pub footer: Option<HeaderFooter>,
    /// Page number extracted from `<hp:ctrl><hp:pageNum>`, if present.
    pub page_number: Option<PageNumber>,
    /// Multi-column settings extracted from `<hp:ctrl><hp:colPr>`, if present.
    /// `None` means single-column (default).
    pub column_settings: Option<ColumnSettings>,
}

/// Parses a section XML string into paragraphs and optional page settings.
///
/// `section_index` is used only for error messages (e.g. `"Contents/section0.xml"`).
/// `chart_xmls` maps chart file paths (e.g. `"Chart/chart1.xml"`) to their OOXML content.
pub fn parse_section(
    xml: &str,
    section_index: usize,
    chart_xmls: &HashMap<String, String>,
) -> HwpxResult<SectionParseResult> {
    let file_hint = format!("Contents/section{section_index}.xml");
    let section: HxSection = from_str(xml)
        .map_err(|e| HwpxError::XmlParse { file: file_hint, detail: e.to_string() })?;

    let mut page_settings = None;
    let mut header = None;
    let mut footer = None;
    let mut page_number = None;
    let mut column_settings = None;

    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(para_idx, hx_para)| {
            let (mut para, ps) = convert_paragraph(hx_para, para_idx == 0, 0)?;
            if ps.is_some() && page_settings.is_none() {
                page_settings = ps;
            }

            // Extract header/footer/pagenum/column_settings from ctrl elements in runs
            for hx_run in &hx_para.runs {
                for ctrl in &hx_run.ctrls {
                    if column_settings.is_none() {
                        if let Some(cs) = convert_ctrl_column_settings(ctrl) {
                            column_settings = Some(cs);
                        }
                    }
                    if header.is_none() {
                        if let Some(hf) = convert_ctrl_header(ctrl) {
                            header = Some(hf);
                        }
                    }
                    if footer.is_none() {
                        if let Some(hf) = convert_ctrl_footer(ctrl) {
                            footer = Some(hf);
                        }
                    }
                    if page_number.is_none() {
                        if let Some(pn) = convert_ctrl_page_number(ctrl) {
                            page_number = Some(pn);
                        }
                    }
                }

                // Chart runs (from <hp:switch><hp:case><hp:chart>)
                let char_shape_id = CharShapeIndex::new(hx_run.char_pr_id_ref as usize);
                for switch in &hx_run.switches {
                    if let Some(case) = &switch.case {
                        if let Some(hx_chart) = &case.chart {
                            if let Some(run) = decode_chart(hx_chart, char_shape_id, chart_xmls)? {
                                para.runs.push(run);
                            }
                        }
                    }
                }
            }

            Ok(para)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(SectionParseResult {
        paragraphs,
        page_settings,
        header,
        footer,
        page_number,
        column_settings,
    })
}

/// Converts an `HxParagraph` to a Core `Paragraph`.
///
/// Returns the paragraph and optionally extracted page settings
/// (from the first run's `<hp:secPr>`).
fn convert_paragraph(
    hx: &HxParagraph,
    is_first: bool,
    depth: usize,
) -> HwpxResult<(Paragraph, Option<PageSettings>)> {
    let para_shape_id = ParaShapeIndex::new(hx.para_pr_id_ref as usize);
    let mut page_settings = None;

    let mut runs = Vec::new();
    for hx_run in &hx.runs {
        // Extract page settings from secPr in first paragraph
        if is_first && page_settings.is_none() {
            if let Some(sec_pr) = &hx_run.sec_pr {
                page_settings = extract_page_settings(sec_pr);
            }
        }

        let mut converted_runs = convert_run(hx_run, depth)?;
        runs.append(&mut converted_runs);
    }

    // Normalize empty paragraphs: HWPX files from 한글 can have empty paragraphs
    // (blank lines). Core's validate() requires at least 1 run per paragraph.
    if runs.is_empty() {
        runs.push(Run::text("", CharShapeIndex::new(0)));
    }

    let paragraph = Paragraph { runs, para_shape_id, column_break: hx.column_break != 0 };
    Ok((paragraph, page_settings))
}

/// Converts an `HxRun` into one or more Core `Run`s.
///
/// A single HxRun can contain multiple `<hp:t>`, `<hp:tbl>`, `<hp:pic>`,
/// `<hp:ctrl>` (footnote/endnote), and `<hp:rect>` (textbox) elements.
/// Each is converted to a separate Run with the same charPrIDRef.
fn convert_run(hx: &HxRun, depth: usize) -> HwpxResult<Vec<Run>> {
    let char_shape_id = CharShapeIndex::new(hx.char_pr_id_ref as usize);
    let mut runs = Vec::new();

    // Text runs
    for text in &hx.texts {
        if !text.text.is_empty() {
            runs.push(Run { content: RunContent::Text(text.text.clone()), char_shape_id });
        }
    }

    // Table runs
    for table in &hx.tables {
        let core_table = convert_table(table, depth)?;
        runs.push(Run { content: RunContent::Table(Box::new(core_table)), char_shape_id });
    }

    // Image runs
    for pic in &hx.pictures {
        if let Some(image) = convert_picture(pic, depth)? {
            runs.push(Run { content: RunContent::Image(image), char_shape_id });
        }
    }

    // Footnote / Endnote runs (from <hp:ctrl>)
    for ctrl in &hx.ctrls {
        if let Some(run) = decode_footnote(ctrl, char_shape_id, depth)? {
            runs.push(run);
        }
        if let Some(run) = decode_endnote(ctrl, char_shape_id, depth)? {
            runs.push(run);
        }
    }

    // Textbox runs (from <hp:rect>)
    for rect in &hx.rects {
        if let Some(run) = decode_textbox(rect, char_shape_id, depth)? {
            runs.push(run);
        }
    }

    // Line runs (from <hp:line>)
    for line in &hx.lines {
        runs.push(decode_line(line, char_shape_id, depth)?);
    }

    // Ellipse runs (from <hp:ellipse>)
    for ellipse in &hx.ellipses {
        runs.push(decode_ellipse(ellipse, char_shape_id, depth)?);
    }

    // Polygon runs (from <hp:polygon>)
    for polygon in &hx.polygons {
        runs.push(decode_polygon(polygon, char_shape_id, depth)?);
    }

    // Equation runs (from <hp:equation>)
    for equation in &hx.equations {
        runs.push(decode_equation(equation, char_shape_id)?);
    }

    Ok(runs)
}

/// Converts an `HxTable` into a Core `Table`.
fn convert_table(hx: &HxTable, depth: usize) -> HwpxResult<Table> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    let rows = hx
        .rows
        .iter()
        .map(|hx_row| {
            let cells = hx_row
                .cells
                .iter()
                .map(|cell| convert_table_cell(cell, depth))
                .collect::<HwpxResult<Vec<_>>>()?;
            Ok(TableRow { cells, height: None })
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    // Validate declared row count matches actual row count
    if hx.rows.len() != hx.row_cnt as usize {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "Table declared rowCnt={} but contains {} <tr> elements",
                hx.row_cnt,
                hx.rows.len()
            ),
        });
    }

    let caption = hx.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Table { rows, width: None, caption })
}

/// Converts an `HxTableCell` into a Core `TableCell`.
fn convert_table_cell(hx: &HxTableCell, depth: usize) -> HwpxResult<TableCell> {
    let paragraphs = if let Some(sub_list) = &hx.sub_list {
        sub_list
            .paragraphs
            .iter()
            .map(|hx_para| {
                let (para, _) = convert_paragraph(hx_para, false, depth + 1)?;
                Ok(para)
            })
            .collect::<HwpxResult<Vec<_>>>()?
    } else {
        vec![Paragraph::new(ParaShapeIndex::new(0))]
    };

    let (col_span, row_span) =
        hx.cell_span.as_ref().map(|cs| (cs.col_span as u16, cs.row_span as u16)).unwrap_or((1, 1));

    let width =
        hx.cell_sz.as_ref().and_then(|sz| HwpUnit::new(sz.width).ok()).unwrap_or(HwpUnit::ZERO);

    Ok(TableCell { paragraphs, col_span, row_span, width, background: None })
}

/// Converts an `HxPic` into a Core `Image`, if it has a valid image reference.
fn convert_picture(hx: &HxPic, depth: usize) -> HwpxResult<Option<Image>> {
    let img = match hx.img.as_ref() {
        Some(img) if !img.binary_item_id_ref.is_empty() => img,
        _ => return Ok(None),
    };

    let path = format!("BinData/{}", img.binary_item_id_ref);
    let format = guess_image_format(&img.binary_item_id_ref);

    let (width, height) = hx
        .cur_sz
        .as_ref()
        .or(hx.org_sz.as_ref())
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let caption = hx.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Some(Image { path, width, height, format, caption }))
}

// ── Footnote / Endnote / TextBox decoding ────────────────────────

/// Decodes an `HxCtrl`'s footnote into a Core `Run`, if present.
fn decode_footnote(
    ctrl: &HxCtrl,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Option<Run>> {
    let hx = match &ctrl.foot_note {
        Some(note) => note,
        None => return Ok(None),
    };
    let paragraphs = decode_note_paragraphs(hx, depth)?;
    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::Footnote {
            inst_id: hx.inst_id,
            paragraphs,
        })),
        char_shape_id,
    }))
}

/// Decodes an `HxCtrl`'s endnote into a Core `Run`, if present.
fn decode_endnote(
    ctrl: &HxCtrl,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Option<Run>> {
    let hx = match &ctrl.end_note {
        Some(note) => note,
        None => return Ok(None),
    };
    let paragraphs = decode_note_paragraphs(hx, depth)?;
    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::Endnote {
            inst_id: hx.inst_id,
            paragraphs,
        })),
        char_shape_id,
    }))
}

/// Decodes an `HxFootNote` (or `HxEndNote`, same type) sub-list into paragraphs.
fn decode_note_paragraphs(hx: &HxFootNote, depth: usize) -> HwpxResult<Vec<Paragraph>> {
    decode_sublist_paragraphs(&hx.sub_list, depth)
}

/// Decodes an `HxRect`'s draw text into a Core `Run` with `Control::TextBox`, if present.
///
/// Only rects with `<hp:drawText>` are treated as textboxes; rects without
/// text content (pure shapes) are silently skipped.
fn decode_textbox(
    rect: &HxRect,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Option<Run>> {
    let draw_text = match &rect.draw_text {
        Some(dt) => dt,
        None => return Ok(None),
    };

    let paragraphs = decode_sublist_paragraphs(&draw_text.sub_list, depth)?;

    // Extract width/height from sz, falling back to zero
    let (width, height) = rect
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    // Extract offsets from pos (treatAsChar=1 means inline, offsets=0)
    let (horz_offset, vert_offset) =
        rect.pos.as_ref().map(|p| (p.horz_offset, p.vert_offset)).unwrap_or((0, 0));

    let caption = rect.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::TextBox {
            paragraphs,
            width,
            height,
            horz_offset,
            vert_offset,
            caption,
            style: None,
        })),
        char_shape_id,
    }))
}

/// Decodes an `HxLine` into a Core `Run` with `Control::Line`.
fn decode_line(line: &HxLine, char_shape_id: CharShapeIndex, depth: usize) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let start = line
        .start_pt
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let end = line
        .end_pt
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });

    let (width, height) = line
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let caption = line.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Line {
            start,
            end,
            width,
            height,
            caption,
            style: decode_shape_style(&line.line_shape, &line.fill_brush),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxEllipse` into a Core `Run` with `Control::Ellipse`.
fn decode_ellipse(
    ellipse: &HxEllipse,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let center = ellipse
        .center
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let axis1 = ellipse
        .ax1
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });
    let axis2 = ellipse
        .ax2
        .as_ref()
        .map(|p| ShapePoint { x: p.x, y: p.y })
        .unwrap_or(ShapePoint { x: 0, y: 0 });

    let (width, height) = ellipse
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let paragraphs = match &ellipse.draw_text {
        Some(dt) => decode_sublist_paragraphs(&dt.sub_list, depth)?,
        None => Vec::new(),
    };

    let caption = ellipse.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Ellipse {
            center,
            axis1,
            axis2,
            width,
            height,
            horz_offset: 0,
            vert_offset: 0,
            paragraphs,
            caption,
            style: decode_shape_style(&ellipse.line_shape, &ellipse.fill_brush),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxPolygon` into a Core `Run` with `Control::Polygon`.
fn decode_polygon(
    polygon: &HxPolygon,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Run> {
    use hwpforge_core::control::ShapePoint;

    let vertices: Vec<ShapePoint> =
        polygon.points.iter().map(|p| ShapePoint { x: p.x, y: p.y }).collect();

    let (width, height) = polygon
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    let paragraphs = match &polygon.draw_text {
        Some(dt) => decode_sublist_paragraphs(&dt.sub_list, depth)?,
        None => Vec::new(),
    };

    let caption = polygon.caption.as_ref().map(|c| convert_hx_caption(c, depth)).transpose()?;

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Polygon {
            vertices,
            width,
            height,
            paragraphs,
            caption,
            style: decode_shape_style(&polygon.line_shape, &polygon.fill_brush),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxEquation` into a Core `Run` with `Control::Equation`.
///
/// Equations have no shape common block and no recursive sub-content,
/// so no `depth` parameter is needed.
fn decode_equation(eq: &HxEquation, char_shape_id: CharShapeIndex) -> HwpxResult<Run> {
    let script = eq.script.as_ref().map(|s| s.text.clone()).unwrap_or_default();

    let (width, height) = eq
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    Ok(Run {
        content: RunContent::Control(Box::new(Control::Equation {
            script,
            width,
            height,
            base_line: eq.base_line,
            text_color: eq.text_color.clone(),
            font: eq.font.clone(),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxChart` into a Core `Run` with `Control::Chart`.
///
/// Looks up the chart's OOXML XML by `chartIDRef` and parses it into
/// structured chart data. Returns `None` if the chart XML is not found.
fn decode_chart(
    hx: &HxChart,
    char_shape_id: CharShapeIndex,
    chart_xmls: &HashMap<String, String>,
) -> HwpxResult<Option<Run>> {
    let chart_xml = match chart_xmls.get(&hx.chart_id_ref) {
        Some(xml) => xml,
        None => return Ok(None),
    };

    let parsed = super::chart::parse_chart_xml(chart_xml)?;

    let (width, height) = hx
        .sz
        .as_ref()
        .map(|sz| {
            (
                HwpUnit::new(sz.width).unwrap_or(HwpUnit::ZERO),
                HwpUnit::new(sz.height).unwrap_or(HwpUnit::ZERO),
            )
        })
        .unwrap_or((HwpUnit::ZERO, HwpUnit::ZERO));

    Ok(Some(Run {
        content: RunContent::Control(Box::new(Control::Chart {
            chart_type: parsed.chart_type,
            data: parsed.data,
            width,
            height,
            title: parsed.title,
            legend: parsed.legend,
            grouping: parsed.grouping,
        })),
        char_shape_id,
    }))
}

/// Extracts a [`ShapeStyle`] from HWPX shape common elements.
///
/// Maps `HxLineShape` and `HxFillBrush` to Core's `ShapeStyle`.
/// Returns `None` if no style information is present.
fn decode_shape_style(
    line_shape: &Option<HxLineShape>,
    fill_brush: &Option<HxFillBrush>,
) -> Option<ShapeStyle> {
    let fill_color: Option<String> =
        fill_brush.as_ref().map(|fb| &fb.win_brush.face_color).filter(|c| !c.is_empty()).cloned();

    let (line_color, line_width, line_style) = match line_shape.as_ref() {
        None => (None, None, None),
        Some(ls) => (
            if ls.color.is_empty() { None } else { Some(ls.color.clone()) },
            if ls.width == 0 { None } else { Some(ls.width) },
            if ls.style.is_empty() { None } else { Some(ls.style.clone()) },
        ),
    };

    if line_color.is_none() && line_width.is_none() && line_style.is_none() && fill_color.is_none()
    {
        return None;
    }

    Some(ShapeStyle { line_color, fill_color, line_width, line_style })
}

/// Converts paragraphs from an `HxSubList` into Core `Paragraph`s.
///
/// Reuses [`convert_paragraph`] at `depth + 1` to track nesting.
fn decode_sublist_paragraphs(sub_list: &HxSubList, depth: usize) -> HwpxResult<Vec<Paragraph>> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "sublist nesting depth {} exceeds limit of {}",
                depth, MAX_NESTING_DEPTH
            ),
        });
    }
    sub_list
        .paragraphs
        .iter()
        .map(|hx_para| {
            let (para, _) = convert_paragraph(hx_para, false, depth + 1)?;
            Ok(para)
        })
        .collect()
}

/// Converts an `HxCaption` into a Core `Caption`.
///
/// Parses side, gap, optional width, and paragraph content from the schema type.
fn convert_hx_caption(hx: &HxCaption, depth: usize) -> HwpxResult<Caption> {
    let side = match hx.side.as_str() {
        "RIGHT" => CaptionSide::Right,
        "TOP" => CaptionSide::Top,
        "BOTTOM" => CaptionSide::Bottom,
        // LEFT is default per schema default_caption_side()
        _ => CaptionSide::Left,
    };

    let width =
        if hx.width > 0 { Some(HwpUnit::new(hx.width).unwrap_or(HwpUnit::ZERO)) } else { None };

    let gap = HwpUnit::new(hx.gap).unwrap_or(HwpUnit::new(850).unwrap());

    let paragraphs = decode_sublist_paragraphs(&hx.sub_list, depth)?;

    Ok(Caption { side, width, gap, paragraphs })
}

/// Guesses image format from the file reference name.
fn guess_image_format(name: &str) -> ImageFormat {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".png") {
        ImageFormat::Png
    } else if lower.ends_with(".jpg") || lower.ends_with(".jpeg") {
        ImageFormat::Jpeg
    } else if lower.ends_with(".gif") {
        ImageFormat::Gif
    } else if lower.ends_with(".bmp") {
        ImageFormat::Bmp
    } else if lower.ends_with(".wmf") {
        ImageFormat::Wmf
    } else if lower.ends_with(".emf") {
        ImageFormat::Emf
    } else {
        ImageFormat::Unknown(name.to_string())
    }
}

/// Extracts `PageSettings` from an `HxSecPr`.
fn extract_page_settings(sec_pr: &crate::schema::section::HxSecPr) -> Option<PageSettings> {
    let page_pr = sec_pr.page_pr.as_ref()?;

    let width = HwpUnit::new(page_pr.width).unwrap_or_else(|_| {
        // A4 width default
        HwpUnit::new(59528).unwrap_or(HwpUnit::ZERO)
    });
    let height = HwpUnit::new(page_pr.height).unwrap_or_else(|_| {
        // A4 height default
        HwpUnit::new(84188).unwrap_or(HwpUnit::ZERO)
    });

    let m = page_pr.margin.as_ref();
    let margin_left = m.and_then(|m| HwpUnit::new(m.left).ok()).unwrap_or(HwpUnit::ZERO);
    let margin_right = m.and_then(|m| HwpUnit::new(m.right).ok()).unwrap_or(HwpUnit::ZERO);
    let margin_top = m.and_then(|m| HwpUnit::new(m.top).ok()).unwrap_or(HwpUnit::ZERO);
    let margin_bottom = m.and_then(|m| HwpUnit::new(m.bottom).ok()).unwrap_or(HwpUnit::ZERO);
    let header_margin = m.and_then(|m| HwpUnit::new(m.header).ok()).unwrap_or(HwpUnit::ZERO);
    let footer_margin = m.and_then(|m| HwpUnit::new(m.footer).ok()).unwrap_or(HwpUnit::ZERO);

    Some(PageSettings {
        width,
        height,
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        header_margin,
        footer_margin,
    })
}

// ── Ctrl conversion helpers ──────────────────────────────────────

/// Extracts [`ColumnSettings`] from an `HxCtrl`'s `colPr` element, if present.
///
/// Returns `None` if the ctrl has no `colPr` or if `colCount <= 1`
/// (single-column is represented as `column_settings: None` on Section).
fn convert_ctrl_column_settings(ctrl: &HxCtrl) -> Option<ColumnSettings> {
    let col_pr = ctrl.col_pr.as_ref()?;

    // Single column (colCount=0 or 1) → None (default layout)
    if col_pr.col_count <= 1 {
        return None;
    }

    let column_type = match col_pr.col_type.as_str() {
        "PARALLEL" => ColumnType::Parallel,
        _ => ColumnType::Newspaper, // default
    };

    let layout = match col_pr.layout.as_str() {
        "RIGHT" => ColumnLayoutMode::Right,
        "MIRROR" => ColumnLayoutMode::Mirror,
        _ => ColumnLayoutMode::Left, // default
    };

    let columns = if col_pr.same_sz == 1 || col_pr.columns.is_empty() {
        // Equal-width columns: build from col_count + same_gap
        let gap = HwpUnit::new(col_pr.same_gap).unwrap_or(HwpUnit::ZERO);
        (0..col_pr.col_count)
            .map(|i| ColumnDef {
                width: HwpUnit::ZERO, // 한글 calculates actual widths
                gap: if i < col_pr.col_count - 1 { gap } else { HwpUnit::ZERO },
            })
            .collect()
    } else {
        // Variable-width columns: read from <hp:col> children
        col_pr
            .columns
            .iter()
            .map(|c| ColumnDef {
                width: HwpUnit::new(c.width).unwrap_or(HwpUnit::ZERO),
                gap: HwpUnit::new(c.gap).unwrap_or(HwpUnit::ZERO),
            })
            .collect()
    };

    Some(ColumnSettings { column_type, layout_mode: layout, columns })
}

/// Extracts a [`HeaderFooter`] from an `HxCtrl`'s header element, if present.
fn convert_ctrl_header(ctrl: &HxCtrl) -> Option<HeaderFooter> {
    let hx = ctrl.header.as_ref()?;
    Some(convert_header_footer(hx))
}

/// Extracts a [`HeaderFooter`] from an `HxCtrl`'s footer element, if present.
fn convert_ctrl_footer(ctrl: &HxCtrl) -> Option<HeaderFooter> {
    let hx = ctrl.footer.as_ref()?;
    Some(convert_header_footer(hx))
}

/// Converts an `HxHeaderFooter` into a Core [`HeaderFooter`].
fn convert_header_footer(hx: &HxHeaderFooter) -> HeaderFooter {
    let apply_page_type = parse_apply_page_type(&hx.apply_page_type);

    let paragraphs = if let Some(sub_list) = &hx.sub_list {
        sub_list
            .paragraphs
            .iter()
            .filter_map(|hx_para| {
                let (para, _) = convert_paragraph(hx_para, false, 0).ok()?;
                Some(para)
            })
            .collect()
    } else {
        Vec::new()
    };

    HeaderFooter::new(paragraphs, apply_page_type)
}

/// Extracts a [`PageNumber`] from an `HxCtrl`'s page_num element, if present.
fn convert_ctrl_page_number(ctrl: &HxCtrl) -> Option<PageNumber> {
    let hx = ctrl.page_num.as_ref()?;
    Some(convert_page_number(hx))
}

/// Converts an `HxPageNum` into a Core [`PageNumber`].
fn convert_page_number(hx: &HxPageNum) -> PageNumber {
    let position = parse_page_number_position(&hx.pos);
    let number_format = parse_number_format_type(&hx.format_type);
    if hx.side_char.is_empty() {
        PageNumber::new(position, number_format)
    } else {
        PageNumber::with_side_char(position, number_format, hx.side_char.clone())
    }
}

/// Parses an HWPX `applyPageType` string into [`ApplyPageType`].
fn parse_apply_page_type(s: &str) -> ApplyPageType {
    match s {
        "BOTH" | "Both" | "both" => ApplyPageType::Both,
        "EVEN" | "Even" | "even" => ApplyPageType::Even,
        "ODD" | "Odd" | "odd" => ApplyPageType::Odd,
        _ => ApplyPageType::Both,
    }
}

/// Parses an HWPX `pos` string into [`PageNumberPosition`].
fn parse_page_number_position(s: &str) -> PageNumberPosition {
    match s {
        "NONE" => PageNumberPosition::None,
        "TOP_LEFT" => PageNumberPosition::TopLeft,
        "TOP_CENTER" => PageNumberPosition::TopCenter,
        "TOP_RIGHT" => PageNumberPosition::TopRight,
        "BOTTOM_LEFT" => PageNumberPosition::BottomLeft,
        "BOTTOM_CENTER" => PageNumberPosition::BottomCenter,
        "BOTTOM_RIGHT" => PageNumberPosition::BottomRight,
        "OUTSIDE_TOP" => PageNumberPosition::OutsideTop,
        "OUTSIDE_BOTTOM" => PageNumberPosition::OutsideBottom,
        "INSIDE_TOP" => PageNumberPosition::InsideTop,
        "INSIDE_BOTTOM" => PageNumberPosition::InsideBottom,
        _ => PageNumberPosition::TopCenter,
    }
}

/// Parses an HWPX `formatType` string into [`NumberFormatType`].
fn parse_number_format_type(s: &str) -> NumberFormatType {
    match s {
        "DIGIT" => NumberFormatType::Digit,
        "CIRCLED_DIGIT" => NumberFormatType::CircledDigit,
        "ROMAN_CAPITAL" => NumberFormatType::RomanCapital,
        "ROMAN_SMALL" => NumberFormatType::RomanSmall,
        "LATIN_CAPITAL" => NumberFormatType::LatinCapital,
        "LATIN_SMALL" => NumberFormatType::LatinSmall,
        "HANGUL_SYLLABLE" => NumberFormatType::HangulSyllable,
        "HANGUL_JAMO" => NumberFormatType::HangulJamo,
        "HANJA_DIGIT" => NumberFormatType::HanjaDigit,
        _ => NumberFormatType::Digit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Text-only sections ───────────────────────────────────────

    #[test]
    fn parse_empty_section() {
        let xml = r#"<sec></sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.paragraphs.is_empty());
        assert!(result.page_settings.is_none());
    }

    #[test]
    fn parse_single_text_paragraph() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <t>안녕하세요</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert_eq!(result.paragraphs.len(), 1);

        let para = &result.paragraphs[0];
        assert_eq!(para.para_shape_id.get(), 0);
        assert_eq!(para.runs.len(), 1);
        assert_eq!(para.runs[0].content.as_text(), Some("안녕하세요"));
    }

    #[test]
    fn parse_multiple_runs_in_paragraph() {
        let xml = r#"<sec>
            <p paraPrIDRef="1">
                <run charPrIDRef="0"><t>Hello </t></run>
                <run charPrIDRef="1"><t>World</t></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.runs.len(), 2);
        assert_eq!(para.runs[0].char_shape_id.get(), 0);
        assert_eq!(para.runs[0].content.as_text(), Some("Hello "));
        assert_eq!(para.runs[1].char_shape_id.get(), 1);
        assert_eq!(para.runs[1].content.as_text(), Some("World"));
    }

    #[test]
    fn parse_multiple_paragraphs() {
        let xml = r#"<sec>
            <p paraPrIDRef="0"><run charPrIDRef="0"><t>First</t></run></p>
            <p paraPrIDRef="1"><run charPrIDRef="0"><t>Second</t></run></p>
            <p paraPrIDRef="2"><run charPrIDRef="0"><t>Third</t></run></p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert_eq!(result.paragraphs.len(), 3);
        assert_eq!(result.paragraphs[2].para_shape_id.get(), 2);
    }

    #[test]
    fn empty_text_is_normalized_to_empty_run() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0"><t/></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        // Empty paragraphs are normalized to contain a single empty text run
        assert_eq!(result.paragraphs[0].runs.len(), 1);
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some(""));
    }

    // ── Page settings ────────────────────────────────────────────

    #[test]
    fn extract_page_settings_from_first_paragraph() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="4252" footer="4252" gutter="0"
                                    left="8504" right="8504" top="5668" bottom="4252"/>
                        </pagePr>
                    </secPr>
                    <t>Content</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert_eq!(ps.width.as_i32(), 59528);
        assert_eq!(ps.height.as_i32(), 84188);
        assert_eq!(ps.margin_left.as_i32(), 8504);
        assert_eq!(ps.margin_right.as_i32(), 8504);
        assert_eq!(ps.margin_top.as_i32(), 5668);
        assert_eq!(ps.margin_bottom.as_i32(), 4252);
        assert_eq!(ps.header_margin.as_i32(), 4252);
        assert_eq!(ps.footer_margin.as_i32(), 4252);
    }

    #[test]
    fn no_sec_pr_gives_none_page_settings() {
        let xml = r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>Hi</t></run></p></sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.page_settings.is_none());
    }

    // ── Table sections ───────────────────────────────────────────

    #[test]
    fn parse_simple_table() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <tbl rowCnt="2" colCnt="2">
                        <tr>
                            <tc name="A1">
                                <cellSpan rowSpan="1" colSpan="1"/>
                                <cellSz width="5000" height="1000"/>
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell1</t></run></p></subList>
                            </tc>
                            <tc name="B1">
                                <cellSpan rowSpan="1" colSpan="1"/>
                                <cellSz width="5000" height="1000"/>
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell2</t></run></p></subList>
                            </tc>
                        </tr>
                        <tr>
                            <tc name="A2">
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell3</t></run></p></subList>
                            </tc>
                            <tc name="B2">
                                <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Cell4</t></run></p></subList>
                            </tc>
                        </tr>
                    </tbl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let run = &result.paragraphs[0].runs[0];

        match &run.content {
            RunContent::Table(table) => {
                assert_eq!(table.rows.len(), 2);
                assert_eq!(table.rows[0].cells.len(), 2);

                let cell0 = &table.rows[0].cells[0];
                assert_eq!(cell0.col_span, 1);
                assert_eq!(cell0.row_span, 1);
                assert_eq!(cell0.width.as_i32(), 5000);
                assert_eq!(cell0.paragraphs[0].runs[0].content.as_text(), Some("Cell1"),);

                let cell3 = &table.rows[1].cells[0];
                assert_eq!(cell3.paragraphs[0].runs[0].content.as_text(), Some("Cell3"),);
            }
            _ => panic!("expected Table content"),
        }
    }

    #[test]
    fn table_cell_without_sublist_gets_empty_paragraph() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <tbl rowCnt="1" colCnt="1">
                        <tr>
                            <tc name="A1"/>
                        </tr>
                    </tbl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        match &result.paragraphs[0].runs[0].content {
            RunContent::Table(table) => {
                let cell = &table.rows[0].cells[0];
                assert_eq!(cell.paragraphs.len(), 1); // default empty paragraph
            }
            _ => panic!("expected Table"),
        }
    }

    // ── Image sections ───────────────────────────────────────────

    #[test]
    fn parse_picture_with_image() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <pic id="pic1">
                        <img binaryItemIDRef="logo.png" bright="0" contrast="0"/>
                        <curSz width="10000" height="5000"/>
                    </pic>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        match &result.paragraphs[0].runs[0].content {
            RunContent::Image(img) => {
                assert_eq!(img.path, "BinData/logo.png");
                assert_eq!(img.width.as_i32(), 10000);
                assert_eq!(img.height.as_i32(), 5000);
                assert_eq!(img.format, ImageFormat::Png);
            }
            _ => panic!("expected Image"),
        }
    }

    #[test]
    fn picture_without_img_child_is_normalized() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <pic id="pic1"/>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        // Empty paragraphs are normalized to contain a single empty text run
        assert_eq!(result.paragraphs[0].runs.len(), 1);
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some(""));
    }

    // ── Image format guessing ────────────────────────────────────

    #[test]
    fn guess_formats() {
        assert_eq!(guess_image_format("logo.png"), ImageFormat::Png);
        assert_eq!(guess_image_format("photo.jpg"), ImageFormat::Jpeg);
        assert_eq!(guess_image_format("photo.JPEG"), ImageFormat::Jpeg);
        assert_eq!(guess_image_format("anim.gif"), ImageFormat::Gif);
        assert_eq!(guess_image_format("icon.bmp"), ImageFormat::Bmp);
        assert_eq!(guess_image_format("clip.wmf"), ImageFormat::Wmf);
        assert_eq!(guess_image_format("draw.emf"), ImageFormat::Emf);
        assert!(matches!(guess_image_format("unknown"), ImageFormat::Unknown(_)));
    }

    // ── Error cases ──────────────────────────────────────────────

    #[test]
    fn parse_invalid_xml() {
        let err = parse_section("<not-closed", 0, &HashMap::new()).unwrap_err();
        assert!(matches!(err, HwpxError::XmlParse { .. }));
    }

    // ── Nesting depth limit ─────────────────────────────────────

    #[test]
    fn nested_tables_within_limit_succeeds() {
        // 1 level of nesting: table → cell → paragraph (depth 1)
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <tbl rowCnt="1" colCnt="1">
                        <tr><tc name="A1">
                            <subList><p paraPrIDRef="0"><run charPrIDRef="0">
                                <tbl rowCnt="1" colCnt="1">
                                    <tr><tc name="A1">
                                        <subList><p paraPrIDRef="0"><run charPrIDRef="0"><t>Deep</t></run></p></subList>
                                    </tc></tr>
                                </tbl>
                            </run></p></subList>
                        </tc></tr>
                    </tbl>
                </run>
            </p>
        </sec>"#;
        assert!(parse_section(xml, 0, &HashMap::new()).is_ok());
    }

    #[test]
    fn nesting_depth_exceeded_returns_error() {
        use crate::schema::section::HxTable;
        // Directly call convert_table at max depth to trigger the limit
        let hx = HxTable { row_cnt: 0, col_cnt: 0, rows: vec![], ..Default::default() };
        let err = convert_table(&hx, MAX_NESTING_DEPTH).unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("nesting depth"));
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    #[test]
    fn table_row_count_mismatch_returns_error() {
        use crate::schema::section::{HxTable, HxTableRow};
        // Create table with rowCnt=2 but only 1 actual row
        let hx = HxTable {
            row_cnt: 2,
            col_cnt: 1,
            rows: vec![HxTableRow { cells: vec![] }],
            ..Default::default()
        };
        let err = convert_table(&hx, 0).unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("rowCnt=2"));
                assert!(detail.contains("contains 1"));
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    // ── Korean text preservation ─────────────────────────────────

    #[test]
    fn korean_utf8_preservation() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0"><t>우리는 수학을 공부한다.</t></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some("우리는 수학을 공부한다."),);
    }

    // ── Header / Footer / PageNum ctrl parsing ──────────────────

    #[test]
    fn parse_header_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <header id="0" applyPageType="BOTH">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Header Text</t></run>
                                </p>
                            </subList>
                        </header>
                    </ctrl>
                    <t>Body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let header = result.header.expect("should have header");
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("Header Text"));
    }

    #[test]
    fn parse_footer_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <footer id="0" applyPageType="EVEN">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Footer Text</t></run>
                                </p>
                            </subList>
                        </footer>
                    </ctrl>
                    <t>Body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let footer = result.footer.expect("should have footer");
        assert_eq!(footer.apply_page_type, ApplyPageType::Even);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("Footer Text"));
    }

    #[test]
    fn parse_page_number_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="- "/>
                    </ctrl>
                    <t>Body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let pn = result.page_number.expect("should have page number");
        assert_eq!(pn.position, PageNumberPosition::BottomCenter);
        assert_eq!(pn.number_format, NumberFormatType::Digit);
        assert_eq!(pn.side_char, "- ");
    }

    #[test]
    fn parse_header_and_footer_and_pagenum_in_same_section() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <header id="0" applyPageType="BOTH">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>My Header</t></run>
                                </p>
                            </subList>
                        </header>
                    </ctrl>
                    <ctrl>
                        <footer id="0" applyPageType="ODD">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>My Footer</t></run>
                                </p>
                            </subList>
                        </footer>
                    </ctrl>
                    <ctrl>
                        <pageNum pos="TOP_LEFT" formatType="ROMAN_CAPITAL" sideChar=""/>
                    </ctrl>
                    <t>Body text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();

        let header = result.header.expect("should have header");
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("My Header"));

        let footer = result.footer.expect("should have footer");
        assert_eq!(footer.apply_page_type, ApplyPageType::Odd);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("My Footer"));

        let pn = result.page_number.expect("should have page number");
        assert_eq!(pn.position, PageNumberPosition::TopLeft);
        assert_eq!(pn.number_format, NumberFormatType::RomanCapital);
        assert!(pn.side_char.is_empty());
    }

    #[test]
    fn section_without_ctrls_has_no_header_footer_pagenum() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0"><t>Plain text</t></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.header.is_none());
        assert!(result.footer.is_none());
        assert!(result.page_number.is_none());
    }

    // ── Footnote / Endnote / TextBox decoder tests ────────────

    #[test]
    fn parse_footnote_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="2">
                    <ctrl>
                        <footNote instId="42">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Footnote body</t></run>
                                </p>
                            </subList>
                        </footNote>
                    </ctrl>
                    <t>Main text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let para = &result.paragraphs[0];

        // Should have text run + footnote run
        assert!(para.runs.len() >= 2, "expected at least 2 runs, got {}", para.runs.len());

        // Find the footnote run
        let footnote_run =
            para.runs.iter().find(|r| r.content.is_control()).expect("no control run");
        match &footnote_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Footnote { inst_id, paragraphs } => {
                    assert_eq!(*inst_id, Some(42));
                    assert_eq!(paragraphs.len(), 1);
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Footnote body"));
                }
                other => panic!("expected Footnote, got {other:?}"),
            },
            other => panic!("expected Control, got {other:?}"),
        }
        assert_eq!(footnote_run.char_shape_id.get(), 2);
    }

    #[test]
    fn parse_endnote_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <endNote>
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Endnote body</t></run>
                                </p>
                            </subList>
                        </endNote>
                    </ctrl>
                    <t>Main text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let para = &result.paragraphs[0];

        let endnote_run =
            para.runs.iter().find(|r| r.content.is_control()).expect("no control run");
        match &endnote_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Endnote { inst_id, paragraphs } => {
                    assert_eq!(*inst_id, None);
                    assert_eq!(paragraphs.len(), 1);
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Endnote body"));
                }
                other => panic!("expected Endnote, got {other:?}"),
            },
            other => panic!("expected Control, got {other:?}"),
        }
    }

    #[test]
    fn parse_textbox_rect() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="3">
                    <rect id="" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM"
                          textFlow="BOTH_SIDES" lock="0" dropcapstyle="None"
                          href="" groupLevel="0" instid="12345" ratio="0">
                        <sz width="14000" height="8000" widthRelTo="ABSOLUTE" heightRelTo="ABSOLUTE" protect="0"/>
                        <pos treatAsChar="1" affectLSpacing="0" flowWithText="0" allowOverlap="0"
                             holdAnchorAndSO="0" vertRelTo="PARA" horzRelTo="PARA"
                             vertAlign="TOP" horzAlign="LEFT" vertOffset="0" horzOffset="0"/>
                        <drawText lastWidth="13434" name="" editable="0">
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0">
                                    <run charPrIDRef="0"><t>Box content</t></run>
                                </p>
                            </subList>
                        </drawText>
                    </rect>
                    <t>Main text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let para = &result.paragraphs[0];

        let tb_run = para.runs.iter().find(|r| r.content.is_control()).expect("no control run");
        assert_eq!(tb_run.char_shape_id.get(), 3);
        match &tb_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::TextBox {
                    paragraphs,
                    width,
                    height,
                    horz_offset,
                    vert_offset,
                    ..
                } => {
                    assert_eq!(paragraphs.len(), 1);
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Box content"));
                    assert_eq!(width.as_i32(), 14000);
                    assert_eq!(height.as_i32(), 8000);
                    assert_eq!(*horz_offset, 0);
                    assert_eq!(*vert_offset, 0);
                }
                other => panic!("expected TextBox, got {other:?}"),
            },
            other => panic!("expected Control, got {other:?}"),
        }
    }

    #[test]
    fn rect_without_draw_text_is_skipped() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <rect id="" zOrder="0" numberingType="NONE" textWrap="TOP_AND_BOTTOM"
                          textFlow="BOTH_SIDES" lock="0" dropcapstyle="None"
                          href="" groupLevel="0" instid="0" ratio="0"/>
                    <t>Main text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        // The rect without drawText should be skipped, only text run present
        assert_eq!(result.paragraphs[0].runs.len(), 1);
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some("Main text"));
    }

    #[test]
    fn parse_footnote_with_multiple_paragraphs() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <footNote>
                            <subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>Line 1</t></run></p>
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>Line 2</t></run></p>
                            </subList>
                        </footNote>
                    </ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ctrl_run = result.paragraphs[0].runs.iter().find(|r| r.content.is_control()).unwrap();
        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Footnote { paragraphs, .. } => {
                    assert_eq!(paragraphs.len(), 2);
                    assert_eq!(paragraphs[0].runs[0].content.as_text(), Some("Line 1"));
                    assert_eq!(paragraphs[1].runs[0].content.as_text(), Some("Line 2"));
                }
                other => panic!("expected Footnote, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    // ── Multi-column (다단) decoder tests ────────────────────────

    #[test]
    fn parse_two_column_equal_width() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <colPr id="" type="NEWSPAPER" layout="LEFT" colCount="2" sameSz="1" sameGap="1134"/>
                    </ctrl>
                    <t>Two column text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let cs = result.column_settings.expect("should have column_settings");
        assert_eq!(cs.column_type, ColumnType::Newspaper);
        assert_eq!(cs.layout_mode, ColumnLayoutMode::Left);
        assert_eq!(cs.columns.len(), 2);
        assert_eq!(cs.columns[0].gap.as_i32(), 1134);
        assert_eq!(cs.columns[1].gap.as_i32(), 0); // last column has no gap
    }

    #[test]
    fn parse_three_column_variable_width() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <colPr id="" type="NEWSPAPER" layout="LEFT" colCount="3" sameSz="0" sameGap="0">
                            <col width="10000" gap="500"/>
                            <col width="15000" gap="500"/>
                            <col width="10000" gap="0"/>
                        </colPr>
                    </ctrl>
                    <t>Variable columns</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let cs = result.column_settings.expect("should have column_settings");
        assert_eq!(cs.columns.len(), 3);
        assert_eq!(cs.columns[0].width.as_i32(), 10000);
        assert_eq!(cs.columns[0].gap.as_i32(), 500);
        assert_eq!(cs.columns[1].width.as_i32(), 15000);
        assert_eq!(cs.columns[1].gap.as_i32(), 500);
        assert_eq!(cs.columns[2].width.as_i32(), 10000);
        assert_eq!(cs.columns[2].gap.as_i32(), 0);
    }

    #[test]
    fn parse_parallel_column_type() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <colPr id="" type="PARALLEL" layout="MIRROR" colCount="2" sameSz="1" sameGap="850"/>
                    </ctrl>
                    <t>Parallel columns</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let cs = result.column_settings.expect("should have column_settings");
        assert_eq!(cs.column_type, ColumnType::Parallel);
        assert_eq!(cs.layout_mode, ColumnLayoutMode::Mirror);
        assert_eq!(cs.columns.len(), 2);
    }

    #[test]
    fn parse_single_column_colpr_returns_none() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/>
                    </ctrl>
                    <t>Single column</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.column_settings.is_none(), "colCount=1 should produce None");
    }

    #[test]
    fn section_without_colpr_has_no_column_settings() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0"><t>Plain text</t></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.column_settings.is_none());
    }

    #[test]
    fn parse_column_with_right_layout() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <colPr id="" type="NEWSPAPER" layout="RIGHT" colCount="2" sameSz="1" sameGap="567"/>
                    </ctrl>
                    <t>Right layout</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let cs = result.column_settings.unwrap();
        assert_eq!(cs.layout_mode, ColumnLayoutMode::Right);
    }

    // ── Parse helper tests ──────────────────────────────────────

    #[test]
    fn parse_apply_page_type_values() {
        assert_eq!(parse_apply_page_type("BOTH"), ApplyPageType::Both);
        assert_eq!(parse_apply_page_type("EVEN"), ApplyPageType::Even);
        assert_eq!(parse_apply_page_type("ODD"), ApplyPageType::Odd);
        assert_eq!(parse_apply_page_type("Both"), ApplyPageType::Both);
        assert_eq!(parse_apply_page_type("unknown"), ApplyPageType::Both);
    }

    #[test]
    fn parse_page_number_position_values() {
        assert_eq!(parse_page_number_position("NONE"), PageNumberPosition::None);
        assert_eq!(parse_page_number_position("TOP_LEFT"), PageNumberPosition::TopLeft);
        assert_eq!(parse_page_number_position("TOP_CENTER"), PageNumberPosition::TopCenter);
        assert_eq!(parse_page_number_position("TOP_RIGHT"), PageNumberPosition::TopRight);
        assert_eq!(parse_page_number_position("BOTTOM_LEFT"), PageNumberPosition::BottomLeft);
        assert_eq!(parse_page_number_position("BOTTOM_CENTER"), PageNumberPosition::BottomCenter);
        assert_eq!(parse_page_number_position("BOTTOM_RIGHT"), PageNumberPosition::BottomRight);
        assert_eq!(parse_page_number_position("OUTSIDE_TOP"), PageNumberPosition::OutsideTop);
        assert_eq!(parse_page_number_position("OUTSIDE_BOTTOM"), PageNumberPosition::OutsideBottom);
        assert_eq!(parse_page_number_position("INSIDE_TOP"), PageNumberPosition::InsideTop);
        assert_eq!(parse_page_number_position("INSIDE_BOTTOM"), PageNumberPosition::InsideBottom);
        assert_eq!(parse_page_number_position("unknown"), PageNumberPosition::TopCenter);
    }

    #[test]
    fn parse_number_format_type_values() {
        assert_eq!(parse_number_format_type("DIGIT"), NumberFormatType::Digit);
        assert_eq!(parse_number_format_type("CIRCLED_DIGIT"), NumberFormatType::CircledDigit);
        assert_eq!(parse_number_format_type("ROMAN_CAPITAL"), NumberFormatType::RomanCapital);
        assert_eq!(parse_number_format_type("ROMAN_SMALL"), NumberFormatType::RomanSmall);
        assert_eq!(parse_number_format_type("LATIN_CAPITAL"), NumberFormatType::LatinCapital);
        assert_eq!(parse_number_format_type("LATIN_SMALL"), NumberFormatType::LatinSmall);
        assert_eq!(parse_number_format_type("HANGUL_SYLLABLE"), NumberFormatType::HangulSyllable);
        assert_eq!(parse_number_format_type("HANGUL_JAMO"), NumberFormatType::HangulJamo);
        assert_eq!(parse_number_format_type("HANJA_DIGIT"), NumberFormatType::HanjaDigit);
        assert_eq!(parse_number_format_type("unknown"), NumberFormatType::Digit);
    }
}
