//! Parses `Contents/section*.xml` into Core paragraphs and page settings.
//!
//! Converts XML schema types (`HxParagraph`, `HxRun`, `HxText`, `HxTable`,
//! `HxPic`) into Core types (`Paragraph`, `Run`, `RunContent`, `Table`, `Image`).

use std::collections::HashMap;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::column::{ColumnDef, ColumnLayoutMode, ColumnSettings, ColumnType};
use hwpforge_core::control::{Control, DutmalAlign, DutmalPosition};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{HeaderFooter, PageNumber};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{
    ApplyPageType, CharShapeIndex, Color, HwpUnit, PageNumberPosition, ParaShapeIndex, StyleIndex,
    TextDirection,
};
use quick_xml::de::from_str;

use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxCaption, HxChart, HxCompose, HxCtrl, HxDutmal, HxEquation, HxFieldBegin, HxFootNote,
    HxHeaderFooter, HxPageNum, HxParagraph, HxPic, HxRun, HxSection, HxSubList, HxTable,
    HxTableCell,
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
    /// Visibility flags extracted from `<hp:visibility>`, if present.
    pub visibility: Option<hwpforge_core::section::Visibility>,
    /// Line number settings extracted from `<hp:lineNumberShape>`, if present.
    pub line_number_shape: Option<hwpforge_core::section::LineNumberShape>,
    /// Page border fill entries extracted from `<hp:pageBorderFill>`, if present.
    pub page_border_fills: Option<Vec<hwpforge_core::section::PageBorderFillEntry>>,
    /// Master pages extracted from `<masterPage>`, if present.
    pub master_pages: Option<Vec<hwpforge_core::section::MasterPage>>,
    /// Text writing direction extracted from `<hp:secPr textDirection="...">`.
    pub text_direction: TextDirection,
    /// Starting numbers extracted from `<hp:startNum>` in secPr.
    pub begin_num: Option<hwpforge_core::section::BeginNum>,
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
    let mut visibility = None;
    let mut line_number_shape = None;
    let mut page_border_fills = None;
    let mut text_direction = TextDirection::Horizontal;
    let mut begin_num = None;

    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(para_idx, hx_para)| {
            let (mut para, ps) = convert_paragraph(hx_para, para_idx == 0, 0)?;
            if ps.is_some() && page_settings.is_none() {
                page_settings = ps;
            }

            // Extract secPr sub-elements (visibility, lineNumberShape, pageBorderFill,
            // textDirection) from the first paragraph's first run
            if para_idx == 0 {
                for hx_run in &hx_para.runs {
                    if let Some(sec_pr) = &hx_run.sec_pr {
                        if visibility.is_none() {
                            visibility = extract_visibility(sec_pr);
                        }
                        if line_number_shape.is_none() {
                            line_number_shape = extract_line_number_shape(sec_pr);
                        }
                        if page_border_fills.is_none() {
                            page_border_fills = extract_page_border_fills(sec_pr);
                        }
                        if begin_num.is_none() {
                            begin_num = extract_begin_num(sec_pr);
                        }
                        text_direction = TextDirection::from_hwpx_str(&sec_pr.text_direction);
                    }
                }
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
                const MAX_STYLE_INDEX: u32 = 100_000;
                if hx_run.char_pr_id_ref > MAX_STYLE_INDEX {
                    return Err(crate::error::HwpxError::InvalidStructure {
                        detail: format!(
                            "charPrIDRef {} exceeds maximum allowed index {}",
                            hx_run.char_pr_id_ref, MAX_STYLE_INDEX,
                        ),
                    });
                }
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
        visibility,
        line_number_shape,
        page_border_fills,
        master_pages: None,
        text_direction,
        begin_num,
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
    const MAX_STYLE_INDEX: u32 = 100_000;
    if hx.para_pr_id_ref > MAX_STYLE_INDEX {
        return Err(crate::error::HwpxError::InvalidStructure {
            detail: format!(
                "paraPrIDRef {} exceeds maximum allowed index {}",
                hx.para_pr_id_ref, MAX_STYLE_INDEX,
            ),
        });
    }
    let para_shape_id = ParaShapeIndex::new(hx.para_pr_id_ref as usize);
    let mut page_settings = None;

    let mut runs = Vec::new();
    let mut has_title_mark = false;
    for hx_run in &hx.runs {
        // Extract page settings from secPr in first paragraph
        if is_first && page_settings.is_none() {
            if let Some(sec_pr) = &hx_run.sec_pr {
                page_settings = extract_page_settings(sec_pr);
            }
        }

        // Detect titleMark for TOC participation
        if let Some(tm) = &hx_run.title_mark {
            if !tm.ignore {
                has_title_mark = true;
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

    // Best-effort heading level: always decodes as `Some(1)` when a titleMark
    // is present. Exact level inference (1-7) requires mapping `styleIDRef` to
    // style names (e.g. "개요 1".."개요 7"), which is not yet implemented.
    // This limitation is intentional: roundtrip fidelity of the heading level
    // integer is not guaranteed; callers should not rely on the decoded value
    // being the true outline depth from the original document.
    let heading_level = if has_title_mark { Some(1) } else { None };

    let style_id =
        if hx.style_id_ref == 0 { None } else { Some(StyleIndex::new(hx.style_id_ref as usize)) };

    let paragraph = Paragraph {
        runs,
        para_shape_id,
        column_break: hx.column_break != 0,
        page_break: hx.page_break != 0,
        heading_level,
        style_id,
    };
    Ok((paragraph, page_settings))
}

/// Converts an `HxRun` into one or more Core `Run`s.
///
/// A single HxRun can contain multiple `<hp:t>`, `<hp:tbl>`, `<hp:pic>`,
/// `<hp:ctrl>` (footnote/endnote), and `<hp:rect>` (textbox) elements.
/// Each is converted to a separate Run with the same charPrIDRef.
fn convert_run(hx: &HxRun, depth: usize) -> HwpxResult<Vec<Run>> {
    const MAX_STYLE_INDEX: u32 = 100_000;
    if hx.char_pr_id_ref > MAX_STYLE_INDEX {
        return Err(crate::error::HwpxError::InvalidStructure {
            detail: format!(
                "charPrIDRef {} exceeds maximum allowed index {}",
                hx.char_pr_id_ref, MAX_STYLE_INDEX,
            ),
        });
    }
    let char_shape_id = CharShapeIndex::new(hx.char_pr_id_ref as usize);
    let mut runs = Vec::new();

    // Check if this run has a fieldBegin+fieldEnd pair — if so, the text
    // is consumed by the field control and should NOT be emitted separately.
    let has_field_pair = hx.ctrls.iter().any(|c| c.field_begin.is_some())
        && hx.ctrls.iter().any(|c| c.field_end.is_some());

    // Text runs — skip if consumed by field controls
    if !has_field_pair {
        for text in &hx.texts {
            let text_content = text.text();
            if !text_content.is_empty() {
                runs.push(Run { content: RunContent::Text(text_content), char_shape_id });
            }
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

    // Footnote / Endnote / Bookmark / IndexMark / Field runs (from <hp:ctrl>)
    //
    // Field controls use fieldBegin/fieldEnd pairs. We collect fieldBegin ctrls
    // and match them with fieldEnd ctrls to extract the field's text from
    // intervening <hp:t> elements. The text runs between begin and end are
    // consumed as the field's display text.
    let mut field_begin: Option<&HxFieldBegin> = None;
    let mut field_begin_char_shape: CharShapeIndex = char_shape_id;

    for ctrl in &hx.ctrls {
        if let Some(run) = decode_footnote(ctrl, char_shape_id, depth)? {
            runs.push(run);
        }
        if let Some(run) = decode_endnote(ctrl, char_shape_id, depth)? {
            runs.push(run);
        }
        if let Some(run) = decode_bookmark(ctrl, char_shape_id) {
            runs.push(run);
        }
        if let Some(run) = decode_indexmark(ctrl, char_shape_id) {
            runs.push(run);
        }
        // Field begin: remember for pairing with fieldEnd
        if let Some(fb) = &ctrl.field_begin {
            field_begin = Some(fb);
            field_begin_char_shape = char_shape_id;
        }
        // Field end: pair with remembered fieldBegin and emit control
        if ctrl.field_end.is_some() {
            if let Some(fb) = field_begin.take() {
                let field_text = collect_run_text(hx);
                if let Some(run) =
                    decode_field_control(fb, &field_text, field_begin_char_shape, depth)?
                {
                    runs.push(run);
                }
            }
        }
        // AutoNum (inline page number)
        if let Some(an) = &ctrl.auto_num {
            if an.num_type == "PAGE" {
                runs.push(Run {
                    content: RunContent::Control(Box::new(Control::Field {
                        field_type: hwpforge_foundation::FieldType::PageNum,
                        hint_text: None,
                        help_text: None,
                    })),
                    char_shape_id,
                });
            }
        }
    }

    // Handle self-closing fieldBegin without a matching fieldEnd (e.g. bookmark span start)
    if let Some(fb) = field_begin.take() {
        if fb.field_type == "BOOKMARK" {
            runs.push(Run {
                content: RunContent::Control(Box::new(Control::Bookmark {
                    name: fb.name.clone(),
                    bookmark_type: hwpforge_foundation::BookmarkType::SpanStart,
                })),
                char_shape_id: field_begin_char_shape,
            });
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

    // Ellipse and Arc runs (from <hp:ellipse>)
    for ellipse in &hx.ellipses {
        if ellipse.has_arc_pr == 1 {
            runs.push(decode_arc(ellipse, char_shape_id, depth)?);
        } else {
            runs.push(decode_ellipse(ellipse, char_shape_id, depth)?);
        }
    }

    // Polygon runs (from <hp:polygon>)
    for polygon in &hx.polygons {
        runs.push(decode_polygon(polygon, char_shape_id, depth)?);
    }

    // Curve runs (from <hp:curve>)
    for curve in &hx.curves {
        runs.push(decode_curve(curve, char_shape_id, depth)?);
    }

    // ConnectLine runs (from <hp:connectLine>)
    for connect_line in &hx.connect_lines {
        runs.push(decode_connect_line(connect_line, char_shape_id, depth)?);
    }

    // Equation runs (from <hp:equation>)
    for equation in &hx.equations {
        runs.push(decode_equation(equation, char_shape_id)?);
    }

    // Dutmal runs (from <hp:dutmal>)
    for dutmal in &hx.dutmals {
        runs.push(decode_dutmal(dutmal, char_shape_id));
    }

    // Compose runs (from <hp:compose>)
    for compose in &hx.composes {
        runs.push(decode_compose(compose, char_shape_id));
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

/// Decodes an `HxCtrl`'s bookmark into a Core `Run`, if present.
fn decode_bookmark(ctrl: &HxCtrl, char_shape_id: CharShapeIndex) -> Option<Run> {
    let bm = ctrl.bookmark.as_ref()?;
    Some(Run {
        content: RunContent::Control(Box::new(Control::Bookmark {
            name: bm.name.clone(),
            bookmark_type: hwpforge_foundation::BookmarkType::Point,
        })),
        char_shape_id,
    })
}

/// Decodes an `HxCtrl`'s indexmark into a Core `Run`, if present.
fn decode_indexmark(ctrl: &HxCtrl, char_shape_id: CharShapeIndex) -> Option<Run> {
    let im = ctrl.indexmark.as_ref()?;
    Some(Run {
        content: RunContent::Control(Box::new(Control::IndexMark {
            primary: im.first_key.clone(),
            secondary: im.second_key.clone(),
        })),
        char_shape_id,
    })
}

/// Collects all text content from an `HxRun`'s `<hp:t>` elements.
///
/// Used to extract the display text between a fieldBegin and fieldEnd pair.
fn collect_run_text(hx: &HxRun) -> String {
    let mut text = String::new();
    for t in &hx.texts {
        text.push_str(&t.text());
    }
    text
}

/// Extracts named parameter values from an `HxFieldBegin`'s parameters.
fn get_field_param(fb: &HxFieldBegin, name: &str) -> Option<String> {
    let params = fb.parameters.as_ref()?;
    for sp in &params.string_params {
        if sp.name == name {
            return Some(sp.value.clone());
        }
    }
    for ip in &params.integer_params {
        if ip.name == name {
            return Some(ip.value.clone());
        }
    }
    for bp in &params.boolean_params {
        if bp.name == name {
            return Some(bp.value.clone());
        }
    }
    None
}

/// Decodes a fieldBegin into the appropriate Core `Control`, using the run's text as display text.
///
/// Returns `None` for unrecognized field types.
fn decode_field_control(
    fb: &HxFieldBegin,
    text: &str,
    char_shape_id: CharShapeIndex,
    depth: usize,
) -> HwpxResult<Option<Run>> {
    let control = match fb.field_type.as_str() {
        "HYPERLINK" => {
            let url = get_field_param(fb, "Path").unwrap_or_default();
            Control::Hyperlink { text: text.to_string(), url }
        }
        "BOOKMARK" => Control::Bookmark {
            name: fb.name.clone(),
            bookmark_type: hwpforge_foundation::BookmarkType::SpanStart,
        },
        "CLICK_HERE" | "DATE" | "TIME" | "PAGE_NUM" | "DOC_SUMMARY" | "USER_INFO" => {
            let ft = fb.field_type.parse::<hwpforge_foundation::FieldType>().unwrap_or_default();
            Control::Field {
                field_type: ft,
                hint_text: get_field_param(fb, "Direction"),
                help_text: get_field_param(fb, "HelpState"),
            }
        }
        "SUMMERY" => {
            // 한글 uses type="SUMMERY" (typo for Summary) for
            // date/time/author fields. Map to FieldType via Command param.
            let cmd = get_field_param(fb, "Command").unwrap_or_default();
            let ft = match cmd.as_str() {
                "$modifiedtime" => hwpforge_foundation::FieldType::Date,
                "$createtime" => hwpforge_foundation::FieldType::Time,
                "$author" | "$title" => hwpforge_foundation::FieldType::DocSummary,
                "$lastsaveby" => hwpforge_foundation::FieldType::UserInfo,
                _ => hwpforge_foundation::FieldType::DocSummary,
            };
            Control::Field { field_type: ft, hint_text: None, help_text: None }
        }
        "CROSSREF" => {
            let target = get_field_param(fb, "RefPath")
                .map(|p| p.trim_start_matches("?#").to_string())
                .unwrap_or_default();
            let rt = get_field_param(fb, "RefType")
                .and_then(|s| s.parse::<hwpforge_foundation::RefType>().ok())
                .unwrap_or_default();
            let ct = get_field_param(fb, "RefContentType")
                .and_then(|s| s.parse::<hwpforge_foundation::RefContentType>().ok())
                .unwrap_or_default();
            let hl = get_field_param(fb, "RefHyperLink").map(|s| s == "true").unwrap_or(false);
            Control::CrossRef {
                target_name: target,
                ref_type: rt,
                content_type: ct,
                as_hyperlink: hl,
            }
        }
        "MEMO" => {
            // Memo body is in subList inside fieldBegin
            let content = if let Some(sub_list) = &fb.sub_list {
                decode_sublist_paragraphs(sub_list, depth)?
            } else {
                Vec::new()
            };
            Control::Memo { content, author: String::new(), date: String::new() }
        }
        _ => return Ok(None),
    };
    Ok(Some(Run { content: RunContent::Control(Box::new(control)), char_shape_id }))
}

// Shape decode functions (decode_textbox, decode_line, decode_ellipse, decode_polygon)
// are defined in `super::shapes`.
use super::shapes::{
    decode_arc, decode_connect_line, decode_curve, decode_ellipse, decode_line, decode_polygon,
    decode_textbox,
};

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
            text_color: parse_hex_color(&eq.text_color).unwrap_or(Color::BLACK),
            font: eq.font.clone(),
        })),
        char_shape_id,
    })
}

/// Decodes an `HxDutmal` into a Core `Run` with `Control::Dutmal`.
fn decode_dutmal(dutmal: &HxDutmal, char_shape_id: CharShapeIndex) -> Run {
    let position = match dutmal.pos_type.as_str() {
        "BOTTOM" => DutmalPosition::Bottom,
        "RIGHT" => DutmalPosition::Right,
        "LEFT" => DutmalPosition::Left,
        _ => DutmalPosition::Top,
    };
    let align = match dutmal.align.as_str() {
        "LEFT" => DutmalAlign::Left,
        "RIGHT" => DutmalAlign::Right,
        _ => DutmalAlign::Center,
    };
    Run {
        content: RunContent::Control(Box::new(Control::Dutmal {
            main_text: dutmal.main_text.clone(),
            sub_text: dutmal.sub_text.clone(),
            position,
            sz_ratio: dutmal.sz_ratio,
            align,
        })),
        char_shape_id,
    }
}

/// Decodes an `HxCompose` into a Core `Run` with `Control::Compose`.
fn decode_compose(compose: &HxCompose, char_shape_id: CharShapeIndex) -> Run {
    Run {
        content: RunContent::Control(Box::new(Control::Compose {
            compose_text: compose.compose_text.clone(),
            circle_type: compose.circle_type.clone(),
            char_sz: compose.char_sz,
            compose_type: compose.compose_type.clone(),
        })),
        char_shape_id,
    }
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
            bar_shape: parsed.bar_shape,
            explosion: parsed.explosion,
            of_pie_type: parsed.of_pie_type,
            radar_style: parsed.radar_style,
            wireframe: parsed.wireframe,
            bubble_3d: parsed.bubble_3d,
            scatter_style: parsed.scatter_style,
            show_markers: parsed.show_markers,
            stock_variant: parsed.stock_variant,
        })),
        char_shape_id,
    }))
}

/// Parses a `#RRGGBB` hex string into a [`Color`].
pub(crate) fn parse_hex_color(s: &str) -> Option<Color> {
    let s = s.strip_prefix('#').unwrap_or(s);
    if s.len() != 6 {
        return None;
    }
    let r = u8::from_str_radix(&s[0..2], 16).ok()?;
    let g = u8::from_str_radix(&s[2..4], 16).ok()?;
    let b = u8::from_str_radix(&s[4..6], 16).ok()?;
    Some(Color::from_rgb(r, g, b))
}

/// Converts paragraphs from an `HxSubList` into Core `Paragraph`s.
///
/// Reuses [`convert_paragraph`] at `depth + 1` to track nesting.
pub(crate) fn decode_sublist_paragraphs(
    sub_list: &HxSubList,
    depth: usize,
) -> HwpxResult<Vec<Paragraph>> {
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
pub(crate) fn convert_hx_caption(hx: &HxCaption, depth: usize) -> HwpxResult<Caption> {
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
    use hwpforge_foundation::GutterType;

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
    let gutter = m.and_then(|m| HwpUnit::new(m.gutter).ok()).unwrap_or(HwpUnit::ZERO);

    let gutter_type = match page_pr.gutter_type.as_str() {
        "LEFT_RIGHT" => GutterType::LeftRight,
        "TOP_ONLY" => GutterType::TopOnly,
        "TOP_BOTTOM" => GutterType::TopBottom,
        _ => GutterType::LeftOnly,
    };

    // No HWPX attribute exists for mirror_margins.
    let mirror_margins = false;

    // 한글 실제 동작: WIDELY=portrait (세로), NARROWLY=landscape (가로)
    // KS X 6101 스펙과 반대! (gotcha #3: landscape 값 반전)
    let landscape = page_pr.landscape == "NARROWLY";

    Some(PageSettings {
        width,
        height,
        margin_left,
        margin_right,
        margin_top,
        margin_bottom,
        header_margin,
        footer_margin,
        gutter,
        gutter_type,
        mirror_margins,
        landscape,
    })
}

/// Extracts [`Visibility`] from an `HxSecPr`.
fn extract_visibility(
    sec_pr: &crate::schema::section::HxSecPr,
) -> Option<hwpforge_core::section::Visibility> {
    use hwpforge_foundation::ShowMode;

    let hx = sec_pr.visibility.as_ref()?;

    let parse_show_mode = |s: &str| -> ShowMode {
        match s {
            "HIDE_ALL" => ShowMode::HideAll,
            "SHOW_ODD" => ShowMode::ShowOdd,
            "SHOW_EVEN" => ShowMode::ShowEven,
            _ => ShowMode::ShowAll,
        }
    };

    Some(hwpforge_core::section::Visibility {
        hide_first_header: hx.hide_first_header != 0,
        hide_first_footer: hx.hide_first_footer != 0,
        hide_first_master_page: hx.hide_first_master_page != 0,
        hide_first_page_num: hx.hide_first_page_num != 0,
        hide_first_empty_line: hx.hide_first_empty_line != 0,
        show_line_number: hx.show_line_number != 0,
        border: parse_show_mode(&hx.border),
        fill: parse_show_mode(&hx.fill),
    })
}

/// Extracts [`LineNumberShape`] from an `HxSecPr`.
fn extract_line_number_shape(
    sec_pr: &crate::schema::section::HxSecPr,
) -> Option<hwpforge_core::section::LineNumberShape> {
    let hx = sec_pr.line_number_shape.as_ref()?;

    // restart_type: CONTINUOUS=0, PAGE=1, SECTION=2
    let restart_type = match hx.restart_type.as_str() {
        "PAGE" | "1" => 1,
        "SECTION" | "2" => 2,
        _ => 0, // CONTINUOUS
    };

    Some(hwpforge_core::section::LineNumberShape {
        restart_type,
        count_by: hx.count_by,
        distance: HwpUnit::new(hx.distance).unwrap_or(HwpUnit::ZERO),
        start_number: hx.start_number,
    })
}

/// Extracts [`BeginNum`] from an `HxSecPr`'s `<hp:startNum>` element.
fn extract_begin_num(
    sec_pr: &crate::schema::section::HxSecPr,
) -> Option<hwpforge_core::section::BeginNum> {
    let sn = sec_pr.start_num.as_ref()?;
    Some(hwpforge_core::section::BeginNum {
        page: sn.page,
        pic: sn.pic,
        tbl: sn.tbl,
        equation: sn.equation,
        footnote: 1,
        endnote: 1,
    })
}

/// Extracts [`PageBorderFillEntry`] list from an `HxSecPr`.
fn extract_page_border_fills(
    sec_pr: &crate::schema::section::HxSecPr,
) -> Option<Vec<hwpforge_core::section::PageBorderFillEntry>> {
    if sec_pr.page_border_fills.is_empty() {
        return None;
    }

    let entries = sec_pr
        .page_border_fills
        .iter()
        .map(|hx| {
            let offset = hx.offset.as_ref().map_or(
                hwpforge_core::section::PageBorderFillEntry::default().offset,
                |o| {
                    [
                        HwpUnit::new(o.left).unwrap_or(HwpUnit::ZERO),
                        HwpUnit::new(o.right).unwrap_or(HwpUnit::ZERO),
                        HwpUnit::new(o.top).unwrap_or(HwpUnit::ZERO),
                        HwpUnit::new(o.bottom).unwrap_or(HwpUnit::ZERO),
                    ]
                },
            );

            hwpforge_core::section::PageBorderFillEntry {
                apply_type: hx.apply_type.clone(),
                border_fill_id: hx.border_fill_id,
                text_border: if hx.text_border.is_empty() {
                    "PAPER".to_string()
                } else {
                    hx.text_border.clone()
                },
                header_inside: hx.header_inside != 0,
                footer_inside: hx.footer_inside != 0,
                fill_area: if hx.fill_area.is_empty() {
                    "PAPER".to_string()
                } else {
                    hx.fill_area.clone()
                },
                offset,
            }
        })
        .collect();

    Some(entries)
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
    let number_format = super::header::parse_number_format(&hx.format_type);
    if hx.side_char.is_empty() {
        PageNumber::new(position, number_format)
    } else {
        PageNumber::with_decoration(position, number_format, hx.side_char.clone())
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

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_foundation::NumberFormatType;

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
        assert_eq!(pn.decoration, "- ");
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
        assert!(pn.decoration.is_empty());
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
    fn parse_number_format_shared_with_header() {
        use crate::decoder::header::parse_number_format;
        assert_eq!(parse_number_format("DIGIT"), NumberFormatType::Digit);
        assert_eq!(parse_number_format("CIRCLED_DIGIT"), NumberFormatType::CircledDigit);
        assert_eq!(parse_number_format("ROMAN_CAPITAL"), NumberFormatType::RomanCapital);
        assert_eq!(parse_number_format("ROMAN_SMALL"), NumberFormatType::RomanSmall);
        assert_eq!(parse_number_format("LATIN_CAPITAL"), NumberFormatType::LatinCapital);
        assert_eq!(parse_number_format("LATIN_SMALL"), NumberFormatType::LatinSmall);
        assert_eq!(parse_number_format("HANGUL_SYLLABLE"), NumberFormatType::HangulSyllable);
        assert_eq!(parse_number_format("HANGUL_JAMO"), NumberFormatType::HangulJamo);
        assert_eq!(parse_number_format("HANJA_DIGIT"), NumberFormatType::HanjaDigit);
        assert_eq!(
            parse_number_format("CIRCLED_HANGUL_SYLLABLE"),
            NumberFormatType::CircledHangulSyllable
        );
        assert_eq!(parse_number_format("unknown"), NumberFormatType::Digit);
    }

    // ── parse_hex_color ──────────────────────────────────────────

    #[test]
    fn parse_hex_color_with_hash() {
        use hwpforge_foundation::Color;
        let c = parse_hex_color("#FF0000").unwrap();
        assert_eq!(c, Color::from_rgb(255, 0, 0));
    }

    #[test]
    fn parse_hex_color_without_hash() {
        use hwpforge_foundation::Color;
        let c = parse_hex_color("0000FF").unwrap();
        assert_eq!(c, Color::from_rgb(0, 0, 255));
    }

    #[test]
    fn parse_hex_color_black() {
        use hwpforge_foundation::Color;
        let c = parse_hex_color("#000000").unwrap();
        assert_eq!(c, Color::BLACK);
    }

    #[test]
    fn parse_hex_color_invalid_returns_none() {
        assert!(parse_hex_color("GGGGGG").is_none(), "invalid hex must return None");
        assert!(parse_hex_color("#FFFF").is_none(), "short hex must return None");
        assert!(parse_hex_color("").is_none(), "empty string must return None");
    }

    // ── page_break / column_break decoding ──────────────────────

    #[test]
    fn parse_page_break() {
        let xml = r#"<sec>
            <p paraPrIDRef="0" pageBreak="1"><run charPrIDRef="0"><t>break</t></run></p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.paragraphs[0].page_break, "pageBreak=1 must decode as true");
    }

    #[test]
    fn parse_column_break() {
        let xml = r#"<sec>
            <p paraPrIDRef="0" columnBreak="1"><run charPrIDRef="0"><t>col</t></run></p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.paragraphs[0].column_break, "columnBreak=1 must decode as true");
    }

    #[test]
    fn parse_page_break_zero_is_false() {
        let xml = r#"<sec>
            <p paraPrIDRef="0" pageBreak="0"><run charPrIDRef="0"><t>normal</t></run></p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(!result.paragraphs[0].page_break, "pageBreak=0 must decode as false");
    }

    // ── Dutmal decoding ──────────────────────────────────────────

    #[test]
    fn parse_dutmal() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <dutmal posType="TOP" szRatio="50" option="0" styleIDRef="0" align="CENTER">
                        <mainText>漢</mainText>
                        <subText>한</subText>
                    </dutmal>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ctrl_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("expected control run");
        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Dutmal { main_text, sub_text, sz_ratio, .. } => {
                    assert_eq!(main_text, "漢");
                    assert_eq!(sub_text, "한");
                    assert_eq!(*sz_ratio, 50);
                }
                other => panic!("expected Dutmal, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn parse_dutmal_bottom_position() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <dutmal posType="BOTTOM" szRatio="75" option="0" styleIDRef="0" align="RIGHT">
                        <mainText>A</mainText>
                        <subText>a</subText>
                    </dutmal>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ctrl_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("expected dutmal control");
        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Dutmal { position, align, .. } => {
                    use hwpforge_core::control::{DutmalAlign, DutmalPosition};
                    assert_eq!(*position, DutmalPosition::Bottom);
                    assert_eq!(*align, DutmalAlign::Right);
                }
                other => panic!("expected Dutmal, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn parse_dutmal_left_position() {
        use crate::schema::section::HxDutmal;
        use hwpforge_core::control::DutmalPosition;
        use hwpforge_foundation::CharShapeIndex;
        let hx = HxDutmal {
            pos_type: "LEFT".to_string(),
            sz_ratio: 60,
            option: 0,
            style_id_ref: 0,
            align: "LEFT".to_string(),
            main_text: "X".to_string(),
            sub_text: "x".to_string(),
        };
        let run = decode_dutmal(&hx, CharShapeIndex::new(0));
        match &run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Dutmal { position, .. } => {
                    assert_eq!(*position, DutmalPosition::Left);
                }
                _ => panic!("expected Dutmal"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn parse_dutmal_right_position() {
        use crate::schema::section::HxDutmal;
        use hwpforge_core::control::DutmalPosition;
        use hwpforge_foundation::CharShapeIndex;
        let hx = HxDutmal {
            pos_type: "RIGHT".to_string(),
            sz_ratio: 60,
            option: 0,
            style_id_ref: 0,
            align: "CENTER".to_string(),
            main_text: "Y".to_string(),
            sub_text: "y".to_string(),
        };
        let run = decode_dutmal(&hx, CharShapeIndex::new(0));
        match &run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Dutmal { position, .. } => {
                    assert_eq!(*position, DutmalPosition::Right);
                }
                _ => panic!("expected Dutmal"),
            },
            _ => panic!("expected Control"),
        }
    }

    // ── Compose decoding ─────────────────────────────────────────

    #[test]
    fn parse_compose() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <compose circleType="CIRCLE" charSz="100" composeType="COMPOSE" charPrCnt="2" composeText="AB">
                        <charPr prIDRef="4294967295"/>
                        <charPr prIDRef="4294967295"/>
                    </compose>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ctrl_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("expected compose control");
        match &ctrl_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Compose {
                    compose_text,
                    circle_type,
                    char_sz,
                    compose_type,
                } => {
                    assert_eq!(compose_text, "AB");
                    assert_eq!(circle_type, "CIRCLE");
                    assert_eq!(*char_sz, 100);
                    assert_eq!(compose_type, "COMPOSE");
                }
                other => panic!("expected Compose, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    // ── serde field control tests ──────────────────────────────────

    /// Helper: find control runs in a parsed section's paragraphs.
    fn find_controls(result: &SectionParseResult) -> Vec<&hwpforge_core::Control> {
        result
            .paragraphs
            .iter()
            .flat_map(|p| p.runs.iter())
            .filter_map(|r| match &r.content {
                RunContent::Control(ctrl) => Some(ctrl.as_ref()),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn serde_field_autonum_page() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <autoNum num="1" numType="PAGE">
                            <autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>
                        </autoNum>
                    </ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let page_num = controls.iter().find(|c| {
            matches!(c, hwpforge_core::Control::Field { field_type, .. }
                if *field_type == hwpforge_foundation::FieldType::PageNum)
        });
        assert!(page_num.is_some(), "autoNum PAGE must produce Field PageNum control");
    }

    #[test]
    fn serde_field_summery_modifiedtime() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin id="0" type="SUMMERY" name="" editable="1" dirty="0" zorder="-1" fieldid="628321650" metaTag="">
                            <parameters cnt="3" name="">
                                <integerParam name="Prop">8</integerParam>
                                <stringParam name="Command">$modifiedtime</stringParam>
                                <stringParam name="Property">$modifiedtime</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t>2026-03-06</t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="628321650"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let date_ctrl = controls.iter().find(|c| {
            matches!(c, hwpforge_core::Control::Field { field_type, .. }
                if *field_type == hwpforge_foundation::FieldType::Date)
        });
        assert!(date_ctrl.is_some(), "SUMMERY/$modifiedtime must decode as FieldType::Date");
    }

    #[test]
    fn serde_field_summery_createtime() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin id="0" type="SUMMERY" name="" editable="1" dirty="0" zorder="-1" fieldid="628321650" metaTag="">
                            <parameters cnt="3" name="">
                                <integerParam name="Prop">8</integerParam>
                                <stringParam name="Command">$createtime</stringParam>
                                <stringParam name="Property">$createtime</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t> </t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="628321650"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let time_ctrl = controls.iter().find(|c| {
            matches!(c, hwpforge_core::Control::Field { field_type, .. }
                if *field_type == hwpforge_foundation::FieldType::Time)
        });
        assert!(time_ctrl.is_some(), "SUMMERY/$createtime must decode as FieldType::Time");
    }

    #[test]
    fn serde_field_summery_author() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin id="0" type="SUMMERY" name="" editable="1" dirty="0" zorder="-1" fieldid="628321650" metaTag="">
                            <parameters cnt="3" name="">
                                <integerParam name="Prop">8</integerParam>
                                <stringParam name="Command">$author</stringParam>
                                <stringParam name="Property">$author</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t> </t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="628321650"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let doc_ctrl = controls.iter().find(|c| {
            matches!(c, hwpforge_core::Control::Field { field_type, .. }
                if *field_type == hwpforge_foundation::FieldType::DocSummary)
        });
        assert!(doc_ctrl.is_some(), "SUMMERY/$author must decode as FieldType::DocSummary");
    }

    #[test]
    fn serde_field_summery_lastsaveby() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin id="0" type="SUMMERY" name="" editable="1" dirty="0" zorder="-1" fieldid="628321650" metaTag="">
                            <parameters cnt="3" name="">
                                <integerParam name="Prop">8</integerParam>
                                <stringParam name="Command">$lastsaveby</stringParam>
                                <stringParam name="Property">$lastsaveby</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t> </t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="628321650"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let ui_ctrl = controls.iter().find(|c| {
            matches!(c, hwpforge_core::Control::Field { field_type, .. }
                if *field_type == hwpforge_foundation::FieldType::UserInfo)
        });
        assert!(ui_ctrl.is_some(), "SUMMERY/$lastsaveby must decode as FieldType::UserInfo");
    }

    #[test]
    fn serde_field_crossref() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin type="CROSSREF" editable="false" dirty="false" zorder="-1" fieldid="0" name="">
                            <parameters cnt="5" name="">
                                <stringParam name="RefPath">?#mybook</stringParam>
                                <stringParam name="RefType">CURRENT</stringParam>
                                <stringParam name="RefContentType">PAGE_NUMBER</stringParam>
                                <booleanParam name="RefHyperLink">true</booleanParam>
                                <stringParam name="RefOpenType">HYPERLINK_JUMP_DONTCARE</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t>mybook</t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="0"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let crossref =
            controls.iter().find(|c| matches!(c, hwpforge_core::Control::CrossRef { .. }));
        assert!(crossref.is_some(), "CROSSREF field must produce CrossRef control");
        if let Some(hwpforge_core::Control::CrossRef { target_name, as_hyperlink, .. }) = crossref {
            assert_eq!(target_name, "mybook", "target_name must strip ?# prefix");
            assert!(*as_hyperlink, "RefHyperLink=true must decode as as_hyperlink=true");
        }
    }

    #[test]
    fn serde_field_bookmark_self_closing() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin type="BOOKMARK" editable="false" dirty="false" zorder="-1" fieldid="0" name="spanmark"/>
                    </ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let bm = controls.iter().find(|c| {
            matches!(
                c,
                hwpforge_core::Control::Bookmark {
                    bookmark_type: hwpforge_foundation::BookmarkType::SpanStart,
                    ..
                }
            )
        });
        assert!(bm.is_some(), "self-closing BOOKMARK fieldBegin must produce SpanStart");
        if let Some(hwpforge_core::Control::Bookmark { name, .. }) = bm {
            assert_eq!(name, "spanmark");
        }
    }

    #[test]
    fn serde_field_hyperlink() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin type="HYPERLINK" editable="false" dirty="false" zorder="-1" fieldid="0" name="">
                            <parameters cnt="4" name="">
                                <stringParam name="Path">https://example.com</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t>link text</t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="0"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let hyperlink =
            controls.iter().find(|c| matches!(c, hwpforge_core::Control::Hyperlink { .. }));
        assert!(hyperlink.is_some(), "HYPERLINK field must produce Hyperlink control");
        if let Some(hwpforge_core::Control::Hyperlink { url, text, .. }) = hyperlink {
            assert_eq!(url, "https://example.com");
            assert_eq!(text, "link text", "hyperlink text must be captured from run");
        }
    }

    #[test]
    fn serde_field_memo() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin type="MEMO" editable="false" dirty="false" zorder="-1" fieldid="0" name="">
                            <parameters cnt="2" name="">
                                <integerParam name="MemoShapeID">0</integerParam>
                                <stringParam name="MemoType">DEFAULT</stringParam>
                            </parameters>
                            <subList id="" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP"
                                     linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">
                                <p paraPrIDRef="0"><run charPrIDRef="0"><t>memo content</t></run></p>
                            </subList>
                        </fieldBegin>
                    </ctrl>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="0"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        let memo = controls.iter().find(|c| matches!(c, hwpforge_core::Control::Memo { .. }));
        assert!(memo.is_some(), "MEMO field must produce Memo control");
        if let Some(hwpforge_core::Control::Memo { content, .. }) = memo {
            assert_eq!(content.len(), 1, "memo should have 1 paragraph from subList");
            assert_eq!(
                content[0].runs[0].content.as_text(),
                Some("memo content"),
                "memo body text must be decoded from subList"
            );
        }
    }

    #[test]
    fn serde_field_empty_xml_no_controls() {
        let xml =
            r#"<sec><p paraPrIDRef="0"><run charPrIDRef="0"><t>no fields</t></run></p></sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let controls = find_controls(&result);
        assert!(controls.is_empty(), "plain text produces no field controls");
    }

    #[test]
    fn serde_field_stays_in_correct_paragraph() {
        // Field controls should stay in the paragraph they belong to, not get
        // dumped into paragraph 0 like the old extract_field_controls() did.
        let xml = r#"<sec>
            <p paraPrIDRef="0"><run charPrIDRef="0"><t>First paragraph</t></run></p>
            <p paraPrIDRef="1">
                <run charPrIDRef="0">
                    <ctrl>
                        <fieldBegin type="HYPERLINK" editable="false" dirty="false" zorder="-1" fieldid="0" name="">
                            <parameters cnt="1" name="">
                                <stringParam name="Path">https://example.com</stringParam>
                            </parameters>
                        </fieldBegin>
                    </ctrl>
                    <t>link</t>
                    <ctrl><fieldEnd beginIDRef="0" fieldid="0"/></ctrl>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        // First paragraph should have no controls
        let p0_controls: Vec<_> =
            result.paragraphs[0].runs.iter().filter(|r| r.content.is_control()).collect();
        assert!(p0_controls.is_empty(), "first paragraph must have no controls");
        // Second paragraph should have the hyperlink control
        let p1_controls: Vec<_> =
            result.paragraphs[1].runs.iter().filter(|r| r.content.is_control()).collect();
        assert_eq!(p1_controls.len(), 1, "second paragraph must have 1 control");
    }

    // ── Visibility decoding via secPr ────────────────────────────

    #[test]
    fn extract_visibility_from_sec_pr() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <visibility hideFirstHeader="1" hideFirstFooter="0" hideFirstMasterPage="0"
                                    border="HIDE_ALL" fill="SHOW_ODD" hideFirstPageNum="1"
                                    hideFirstEmptyLine="0" showLineNumber="1"/>
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="4252" footer="4252" gutter="0"
                                    left="8504" right="8504" top="5668" bottom="4252"/>
                        </pagePr>
                    </secPr>
                    <t>body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let vis = result.visibility.expect("should have visibility");
        assert!(vis.hide_first_header, "hideFirstHeader=1 must decode as true");
        assert!(!vis.hide_first_footer);
        assert!(vis.hide_first_page_num);
        assert!(vis.show_line_number);
        use hwpforge_foundation::ShowMode;
        assert_eq!(vis.border, ShowMode::HideAll);
        assert_eq!(vis.fill, ShowMode::ShowOdd);
    }

    #[test]
    fn no_visibility_in_sec_pr_gives_none() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="4252" footer="4252" gutter="0"
                                    left="8504" right="8504" top="5668" bottom="4252"/>
                        </pagePr>
                    </secPr>
                    <t>body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert!(result.visibility.is_none(), "absent visibility must give None");
    }

    // ── LineNumberShape decoding ─────────────────────────────────

    #[test]
    fn extract_line_number_shape_from_sec_pr() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <lineNumberShape restartType="PAGE" countBy="5" distance="1000" startNumber="3"/>
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="4252" footer="4252" gutter="0"
                                    left="8504" right="8504" top="5668" bottom="4252"/>
                        </pagePr>
                    </secPr>
                    <t>body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let lns = result.line_number_shape.expect("should have line_number_shape");
        assert_eq!(lns.restart_type, 1, "PAGE restart_type must be 1");
        assert_eq!(lns.count_by, 5);
        assert_eq!(lns.distance.as_i32(), 1000);
        assert_eq!(lns.start_number, 3);
    }

    fn make_empty_sec_pr() -> crate::schema::section::HxSecPr {
        use crate::schema::section::HxSecPr;
        HxSecPr {
            text_direction: String::new(),
            master_page_cnt: 0,
            visibility: None,
            line_number_shape: None,
            page_pr: None,
            page_border_fills: vec![],
            start_num: None,
        }
    }

    #[test]
    fn extract_line_number_shape_section_type() {
        use crate::schema::section::HxLineNumberShape;
        let mut sec_pr = make_empty_sec_pr();
        sec_pr.line_number_shape = Some(HxLineNumberShape {
            restart_type: "SECTION".to_string(),
            count_by: 10,
            distance: 2000,
            start_number: 1,
        });
        let lns = extract_line_number_shape(&sec_pr).unwrap();
        assert_eq!(lns.restart_type, 2, "SECTION restart_type must be 2");
    }

    #[test]
    fn extract_line_number_shape_continuous_type() {
        use crate::schema::section::HxLineNumberShape;
        let mut sec_pr = make_empty_sec_pr();
        sec_pr.line_number_shape = Some(HxLineNumberShape {
            restart_type: "CONTINUOUS".to_string(),
            count_by: 1,
            distance: 0,
            start_number: 0,
        });
        let lns = extract_line_number_shape(&sec_pr).unwrap();
        assert_eq!(lns.restart_type, 0, "CONTINUOUS restart_type must be 0");
    }

    // ── PageBorderFillEntry decoding ─────────────────────────────

    #[test]
    fn extract_page_border_fills_from_sec_pr() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="4252" footer="4252" gutter="0"
                                    left="8504" right="8504" top="5668" bottom="4252"/>
                        </pagePr>
                        <pageBorderFill type="BOTH" borderFillIDRef="2" textBorder="PAPER" headerInside="1" footerInside="0" fillArea="PAGE">
                            <offset left="500" right="600" top="700" bottom="800"/>
                        </pageBorderFill>
                    </secPr>
                    <t>body</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let fills = result.page_border_fills.expect("should have page_border_fills");
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].apply_type, "BOTH");
        assert_eq!(fills[0].border_fill_id, 2);
        assert!(fills[0].header_inside, "headerInside=1 must be true");
        assert!(!fills[0].footer_inside, "footerInside=0 must be false");
        assert_eq!(fills[0].fill_area, "PAGE");
        assert_eq!(fills[0].offset[0].as_i32(), 500); // left
    }

    #[test]
    fn empty_page_border_fills_gives_none() {
        let sec_pr = make_empty_sec_pr();
        assert!(extract_page_border_fills(&sec_pr).is_none(), "empty list must give None");
    }

    // ── Landscape decoding (reversed semantics) ──────────────────

    #[test]
    fn narrowly_decodes_as_landscape_true() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="NARROWLY" width="59528" height="84188">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>landscape</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert!(ps.landscape, "NARROWLY must decode as landscape=true");
    }

    #[test]
    fn widely_decodes_as_landscape_false() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>portrait</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert!(!ps.landscape, "WIDELY must decode as landscape=false");
    }

    // ── Gutter type decoding ─────────────────────────────────────

    #[test]
    fn gutter_type_left_right_decodes() {
        use hwpforge_foundation::GutterType;
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188" gutterType="LEFT_RIGHT">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>gutter</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert_eq!(ps.gutter_type, GutterType::LeftRight);
    }

    #[test]
    fn gutter_type_top_only_decodes() {
        use hwpforge_foundation::GutterType;
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188" gutterType="TOP_ONLY">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>gutter</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert_eq!(ps.gutter_type, GutterType::TopOnly);
    }

    #[test]
    fn gutter_type_top_bottom_decodes() {
        use hwpforge_foundation::GutterType;
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="HORIZONTAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188" gutterType="TOP_BOTTOM">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>gutter</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let ps = result.page_settings.unwrap();
        assert_eq!(ps.gutter_type, GutterType::TopBottom);
    }

    // ── Sublist nesting depth limit ──────────────────────────────

    #[test]
    fn sublist_nesting_depth_exceeded_returns_error() {
        use crate::schema::section::{HxParagraph, HxSubList};
        let sub_list = HxSubList {
            id: String::new(),
            text_direction: "HORIZONTAL".to_string(),
            line_wrap: "BREAK".to_string(),
            vert_align: "TOP".to_string(),
            link_list_id_ref: 0,
            link_list_next_id_ref: 0,
            text_width: 0,
            text_height: 0,
            has_text_ref: 0,
            has_num_ref: 0,
            paragraphs: vec![HxParagraph::default()],
        };
        let err = decode_sublist_paragraphs(&sub_list, MAX_NESTING_DEPTH).unwrap_err();
        match &err {
            crate::error::HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("nesting depth"), "error must mention nesting depth");
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    // ── Bookmark (Point) decoding ────────────────────────────────

    #[test]
    fn parse_bookmark_point_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <bookmark name="bk1"/>
                    </ctrl>
                    <t>text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let bm_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("expected bookmark control run");
        match &bm_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Bookmark { name, bookmark_type } => {
                    assert_eq!(name, "bk1");
                    assert_eq!(*bookmark_type, hwpforge_foundation::BookmarkType::Point);
                }
                other => panic!("expected Bookmark, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    // ── IndexMark decoding ───────────────────────────────────────

    #[test]
    fn parse_indexmark_ctrl() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <ctrl>
                        <indexmark>
                            <firstKey>주항목</firstKey>
                            <secondKey>부항목</secondKey>
                        </indexmark>
                    </ctrl>
                    <t>text</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        let im_run = result.paragraphs[0]
            .runs
            .iter()
            .find(|r| r.content.is_control())
            .expect("expected indexmark control run");
        match &im_run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::IndexMark { primary, secondary } => {
                    assert_eq!(primary, "주항목");
                    assert_eq!(secondary.as_deref(), Some("부항목"));
                }
                other => panic!("expected IndexMark, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    // ── text_direction decoding ──────────────────────────────────

    #[test]
    fn vertical_text_direction_decodes() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <secPr textDirection="VERTICAL">
                        <pagePr landscape="WIDELY" width="59528" height="84188">
                            <margin header="0" footer="0" gutter="0" left="0" right="0" top="0" bottom="0"/>
                        </pagePr>
                    </secPr>
                    <t>세로</t>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0, &HashMap::new()).unwrap();
        assert_eq!(result.text_direction, TextDirection::Vertical);
    }

    // ── Caption decoding ─────────────────────────────────────────

    #[test]
    fn convert_hx_caption_left_side() {
        use crate::schema::section::{HxCaption, HxSubList};
        let hx = HxCaption {
            side: "LEFT".to_string(),
            full_sz: 0,
            width: 5000,
            gap: 850,
            last_width: 10000,
            sub_list: HxSubList {
                id: String::new(),
                text_direction: "HORIZONTAL".to_string(),
                line_wrap: "BREAK".to_string(),
                vert_align: "TOP".to_string(),
                link_list_id_ref: 0,
                link_list_next_id_ref: 0,
                text_width: 0,
                text_height: 0,
                has_text_ref: 0,
                has_num_ref: 0,
                paragraphs: vec![],
            },
        };
        let caption = convert_hx_caption(&hx, 0).unwrap();
        use hwpforge_core::caption::CaptionSide;
        assert_eq!(caption.side, CaptionSide::Left);
        assert_eq!(caption.width.unwrap().as_i32(), 5000);
        assert_eq!(caption.gap.as_i32(), 850);
    }

    #[test]
    fn convert_hx_caption_right_side() {
        use crate::schema::section::{HxCaption, HxSubList};
        let hx = HxCaption {
            side: "RIGHT".to_string(),
            full_sz: 0,
            width: 0, // zero width → None
            gap: 0,
            last_width: 0,
            sub_list: HxSubList {
                id: String::new(),
                text_direction: "HORIZONTAL".to_string(),
                line_wrap: "BREAK".to_string(),
                vert_align: "TOP".to_string(),
                link_list_id_ref: 0,
                link_list_next_id_ref: 0,
                text_width: 0,
                text_height: 0,
                has_text_ref: 0,
                has_num_ref: 0,
                paragraphs: vec![],
            },
        };
        let caption = convert_hx_caption(&hx, 0).unwrap();
        use hwpforge_core::caption::CaptionSide;
        assert_eq!(caption.side, CaptionSide::Right);
        assert!(caption.width.is_none(), "zero width must decode as None");
    }

    #[test]
    fn convert_hx_caption_top_and_bottom() {
        use crate::schema::section::{HxCaption, HxSubList};
        use hwpforge_core::caption::CaptionSide;

        let make_caption = |side: &str| HxCaption {
            side: side.to_string(),
            full_sz: 0,
            width: 0,
            gap: 0,
            last_width: 0,
            sub_list: HxSubList {
                id: String::new(),
                text_direction: "HORIZONTAL".to_string(),
                line_wrap: "BREAK".to_string(),
                vert_align: "TOP".to_string(),
                link_list_id_ref: 0,
                link_list_next_id_ref: 0,
                text_width: 0,
                text_height: 0,
                has_text_ref: 0,
                has_num_ref: 0,
                paragraphs: vec![],
            },
        };

        let top = convert_hx_caption(&make_caption("TOP"), 0).unwrap();
        assert_eq!(top.side, CaptionSide::Top);

        let bottom = convert_hx_caption(&make_caption("BOTTOM"), 0).unwrap();
        assert_eq!(bottom.side, CaptionSide::Bottom);
    }

    // ── Equation decoding ────────────────────────────────────────

    #[test]
    fn parse_equation_from_schema() {
        use crate::schema::section::{HxEquation, HxScript, HxTableSz};
        use hwpforge_foundation::CharShapeIndex;
        let hx = HxEquation {
            sz: Some(HxTableSz {
                width: 10000,
                width_rel_to: "ABSOLUTE".to_string(),
                height: 5000,
                height_rel_to: "ABSOLUTE".to_string(),
                protect: 0,
            }),
            base_line: 80,
            text_color: "#000000".to_string(),
            font: "HCR Batang".to_string(),
            script: Some(HxScript { text: "{x} over {y}".to_string() }),
            ..Default::default()
        };
        let run = decode_equation(&hx, CharShapeIndex::new(0)).unwrap();
        match &run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Equation {
                    script, width, height, base_line, font, ..
                } => {
                    assert_eq!(script, "{x} over {y}");
                    assert_eq!(width.as_i32(), 10000);
                    assert_eq!(height.as_i32(), 5000);
                    assert_eq!(*base_line, 80);
                    assert_eq!(font, "HCR Batang");
                }
                other => panic!("expected Equation, got {other:?}"),
            },
            _ => panic!("expected Control"),
        }
    }

    #[test]
    fn decode_equation_no_sz_uses_zero() {
        use crate::schema::section::HxEquation;
        use hwpforge_foundation::CharShapeIndex;
        let hx = HxEquation {
            sz: None,
            base_line: 0,
            text_color: "#000000".to_string(),
            font: "".to_string(),
            script: None,
            ..Default::default()
        };
        let run = decode_equation(&hx, CharShapeIndex::new(0)).unwrap();
        match &run.content {
            RunContent::Control(ctrl) => match ctrl.as_ref() {
                hwpforge_core::Control::Equation { width, height, script, .. } => {
                    assert_eq!(width.as_i32(), 0);
                    assert_eq!(height.as_i32(), 0);
                    assert_eq!(script, "");
                }
                _ => panic!("expected Equation"),
            },
            _ => panic!("expected Control"),
        }
    }
}
