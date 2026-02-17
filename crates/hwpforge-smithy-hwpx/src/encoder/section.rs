//! Encodes a Core [`Section`] into HWPX section XML.
//!
//! This is the reverse of [`crate::decoder::section`]: it converts Core types
//! (`Section`, `Paragraph`, `Run`, `Table`, `Image`) into schema types
//! (`HxSection`, `HxParagraph`, `HxRun`, etc.), serializes them with
//! `quick_xml::se::to_string`, and wraps the result in an xmlns-qualified
//! `<hs:sec>` root element.
//!
//! # SecPr Injection
//!
//! HWPX encodes page settings (`<hp:secPr>`) inside the **first run** of
//! the **first paragraph**, not at the section level. This module reproduces
//! that quirk so the output is compatible with the Hancom HWP editor.

use hwpforge_core::image::Image;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;

use crate::encoder::package::XMLNS_DECLS;
use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxCellAddr, HxCellSpan, HxCellSz, HxImg, HxLineSeg, HxLineSegArray, HxPageMargin, HxPagePr,
    HxParagraph, HxPic, HxRun, HxSecPr, HxSection, HxSizeAttr, HxSubList, HxTable, HxTableCell,
    HxTableMargin, HxTablePos, HxTableRow, HxTableSz, HxText,
};

/// Maximum nesting depth for tables-within-tables.
///
/// Mirrors the decoder's limit. Prevents stack overflow from deeply nested
/// table structures (e.g. a table cell containing another table, ad infinitum).
const MAX_NESTING_DEPTH: usize = 32;

/// Encodes a Core [`Section`] into a complete HWPX section XML string.
///
/// The returned string is a well-formed XML document with `<?xml ...?>`
/// declaration and an `<hs:sec>` root element carrying all required
/// namespace declarations.
///
/// `_section_index` is reserved for future use (e.g. error messages) but
/// currently unused.
///
/// # Errors
///
/// Returns [`HwpxError::XmlSerialize`] if quick-xml serialization fails,
/// or [`HwpxError::InvalidStructure`] if table nesting exceeds the limit.
pub(crate) fn encode_section(section: &Section, _section_index: usize) -> HwpxResult<String> {
    let hx_section = build_section(section)?;
    let inner_xml = quick_xml::se::to_string(&hx_section)
        .map_err(|e| HwpxError::XmlSerialize { detail: e.to_string() })?;

    // quick_xml produces `<sec>...</sec>` (from the serde rename).
    // We need `<hs:sec xmlns:...>...</hs:sec>`, so strip the outer
    // element and wrap with our template.
    let inner_content = strip_root_element(&inner_xml);

    // Enrich <hp:secPr> with sub-elements required by 한글 (grid,
    // startNum, visibility, footnote/endnote, pageBorderFill).
    let mut enriched = enrich_sec_pr(inner_content);

    // Inject header/footer/page number controls after colPr
    inject_header_footer_pagenum(&mut enriched, section);

    Ok(wrap_section_xml(&enriched))
}

/// Wraps inner XML content in an `<hs:sec>` element with all xmlns declarations.
fn wrap_section_xml(inner_xml: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8" standalone="yes" ?><hs:sec{xmlns}>{inner_xml}</hs:sec>"#,
        xmlns = XMLNS_DECLS,
    )
}

/// Strips the outermost element from a serialized XML string, keeping inner content.
///
/// Input: `<sec><hp:p ...>...</hp:p></sec>` produces `<hp:p ...>...</hp:p>`.
/// Input: `<sec/>` (self-closing, empty) produces `""`.
fn strip_root_element(xml: &str) -> &str {
    // Self-closing element: <sec/> or <sec />
    if xml.ends_with("/>") {
        return "";
    }
    // Find first '>' after opening tag
    let start = match xml.find('>') {
        Some(i) => i + 1,
        None => return xml,
    };
    // Find last '</'
    let end = xml.rfind("</").unwrap_or(xml.len());
    &xml[start..end]
}

/// Builds an `HxSection` from a Core `Section`.
fn build_section(section: &Section) -> HwpxResult<HxSection> {
    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| {
            let inject_sec_pr = idx == 0;
            let page_settings = if inject_sec_pr { Some(&section.page_settings) } else { None };
            build_paragraph(para, inject_sec_pr, page_settings, idx, 0)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxSection { paragraphs })
}

/// Builds an `HxParagraph` from a Core `Paragraph`.
///
/// When `inject_sec_pr` is true (first paragraph of the section), page
/// settings are embedded in the first run's `<hp:secPr>`.
/// `depth` tracks table nesting level for overflow prevention.
fn build_paragraph(
    para: &Paragraph,
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    para_idx: usize,
    depth: usize,
) -> HwpxResult<HxParagraph> {
    let runs = build_runs(&para.runs, inject_sec_pr, page_settings, depth)?;

    // Build a placeholder linesegarray with approximate values.
    // 한글 will recalculate this on open, but having a placeholder
    // improves initial rendering.
    let horzsize = page_settings
        .map(|ps| ps.width.as_i32() - ps.margin_left.as_i32() - ps.margin_right.as_i32())
        .unwrap_or(DEFAULT_HORZ_SIZE);
    let linesegarray = Some(build_linesegarray(horzsize));

    Ok(HxParagraph {
        id: format!("{para_idx}"),
        para_pr_id_ref: para.para_shape_id.get() as u32,
        style_id_ref: 0,
        page_break: 0,
        column_break: 0,
        merged: 0,
        runs,
        linesegarray,
    })
}

/// Builds `Vec<HxRun>` from Core runs.
///
/// Each Core `Run` maps to exactly one `HxRun`. `RunContent::Control`
/// variants are silently skipped (no XML output for controls in Phase 4).
///
/// If `inject_sec_pr` is true and `page_settings` is `Some`, the first
/// run gets `<hp:secPr>` attached.
fn build_runs(
    runs: &[Run],
    inject_sec_pr: bool,
    page_settings: Option<&PageSettings>,
    depth: usize,
) -> HwpxResult<Vec<HxRun>> {
    let mut result = Vec::new();
    let mut sec_pr_injected = false;

    for run in runs {
        // Skip Control runs entirely
        if run.content.is_control() {
            continue;
        }

        let sec_pr = if inject_sec_pr && !sec_pr_injected {
            sec_pr_injected = true;
            page_settings.map(build_sec_pr)
        } else {
            None
        };

        let char_pr_id_ref = run.char_shape_id.get() as u32;

        let mut texts = Vec::new();
        let mut tables = Vec::new();
        let mut pictures = Vec::new();

        match &run.content {
            RunContent::Text(s) => {
                texts.push(HxText { text: s.clone() });
            }
            RunContent::Table(t) => {
                tables.push(build_table(t, depth)?);
            }
            RunContent::Image(img) => {
                pictures.push(build_picture(img));
            }
            RunContent::Control(_) => unreachable!("Controls filtered above"),
            _ => {
                // Future RunContent variants are silently skipped
                // (non_exhaustive enum)
            }
        }

        result.push(HxRun { char_pr_id_ref, sec_pr, texts, tables, pictures, ctrls: Vec::new() });
    }

    // If we need to inject secPr but there were no non-control runs,
    // create a synthetic empty run to carry it.
    if inject_sec_pr && !sec_pr_injected {
        if let Some(ps) = page_settings {
            result.insert(
                0,
                HxRun {
                    char_pr_id_ref: 0,
                    sec_pr: Some(build_sec_pr(ps)),
                    texts: Vec::new(),
                    tables: Vec::new(),
                    pictures: Vec::new(),
                    ctrls: Vec::new(),
                },
            );
        }
    }

    Ok(result)
}

/// Builds `HxSecPr` from Core `PageSettings`.
fn build_sec_pr(ps: &PageSettings) -> HxSecPr {
    HxSecPr {
        text_direction: "HORIZONTAL".to_string(),
        page_pr: Some(HxPagePr {
            landscape: "WIDELY".to_string(),
            width: ps.width.as_i32(),
            height: ps.height.as_i32(),
            gutter_type: "LEFT_ONLY".to_string(),
            margin: Some(HxPageMargin {
                header: ps.header_margin.as_i32(),
                footer: ps.footer_margin.as_i32(),
                gutter: 0,
                left: ps.margin_left.as_i32(),
                right: ps.margin_right.as_i32(),
                top: ps.margin_top.as_i32(),
                bottom: ps.margin_bottom.as_i32(),
            }),
        }),
    }
}

/// Default inner cell margin (left/right: 510 ≈ 1.8mm, top/bottom: 141 ≈ 0.5mm).
const DEFAULT_CELL_MARGIN: HxTableMargin =
    HxTableMargin { left: 510, right: 510, top: 141, bottom: 141 };

/// Default outer table margin (283 ≈ 1mm on all sides).
const DEFAULT_OUT_MARGIN: HxTableMargin =
    HxTableMargin { left: 283, right: 283, top: 283, bottom: 283 };

/// `borderFillIDRef` for table cells (matches header.xml borderFill id=3).
const TABLE_BORDER_FILL_ID: u32 = 3;

/// Builds `HxTable` from a Core `Table`.
///
/// Populates all attributes and sub-elements required by 한글:
/// `hp:sz`, `hp:pos`, `hp:outMargin`, `hp:inMargin`, plus full
/// attribute set on `<hp:tbl>`.
///
/// # Errors
///
/// Returns [`HwpxError::InvalidStructure`] if nesting depth exceeds
/// [`MAX_NESTING_DEPTH`].
fn build_table(table: &Table, depth: usize) -> HwpxResult<HxTable> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!("table nesting depth {} exceeds limit of {}", depth, MAX_NESTING_DEPTH,),
        });
    }

    let col_cnt = table.rows.iter().map(|r| r.cells.len()).max().unwrap_or(0) as u32;

    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(row_idx, row)| build_table_row(row, row_idx as u32, depth))
        .collect::<HwpxResult<Vec<_>>>()?;

    // Table width: use explicit width or sum of first row's cell widths
    let table_width = table.width.map(|w| w.as_i32()).unwrap_or_else(|| {
        table
            .rows
            .first()
            .map_or(DEFAULT_HORZ_SIZE, |r| r.cells.iter().map(|c| c.width.as_i32()).sum())
    });

    Ok(HxTable {
        id: String::new(),
        z_order: 0,
        numbering_type: "TABLE".to_string(),
        text_wrap: "TOP_AND_BOTTOM".to_string(),
        text_flow: "BOTH_SIDES".to_string(),
        lock: 0,
        dropcap_style: "None".to_string(),
        page_break: "CELL".to_string(),
        repeat_header: 1,
        row_cnt: table.rows.len() as u32,
        col_cnt,
        cell_spacing: 0,
        border_fill_id_ref: TABLE_BORDER_FILL_ID,
        no_adjust: 0,
        sz: Some(HxTableSz {
            width: table_width,
            width_rel_to: "ABSOLUTE".to_string(),
            height: 0,
            height_rel_to: "ABSOLUTE".to_string(),
            protect: 0,
        }),
        pos: Some(HxTablePos {
            treat_as_char: 0,
            affect_l_spacing: 0,
            flow_with_text: 1,
            allow_overlap: 0,
            hold_anchor_and_so: 0,
            vert_rel_to: "PARA".to_string(),
            horz_rel_to: "COLUMN".to_string(),
            vert_align: "TOP".to_string(),
            horz_align: "LEFT".to_string(),
            vert_offset: 0,
            horz_offset: 0,
        }),
        out_margin: Some(DEFAULT_OUT_MARGIN),
        in_margin: Some(DEFAULT_CELL_MARGIN),
        rows,
    })
}

/// Builds `HxTableRow` from a Core `TableRow`.
fn build_table_row(row: &TableRow, row_idx: u32, depth: usize) -> HwpxResult<HxTableRow> {
    let cells = row
        .cells
        .iter()
        .enumerate()
        .map(|(col_idx, cell)| build_table_cell(cell, col_idx as u32, row_idx, depth))
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxTableRow { cells })
}

/// Builds `HxTableCell` from a Core `TableCell`.
///
/// Cell paragraphs are built recursively at `depth + 1` to track nesting.
/// `col_idx` and `row_idx` are used to populate `<hp:cellAddr>`.
fn build_table_cell(
    cell: &TableCell,
    col_idx: u32,
    row_idx: u32,
    depth: usize,
) -> HwpxResult<HxTableCell> {
    let paragraphs = cell
        .paragraphs
        .iter()
        .enumerate()
        .map(|(idx, para)| build_paragraph(para, false, None, idx, depth + 1))
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(HxTableCell {
        name: String::new(),
        header: 0,
        has_margin: 0,
        protect: 0,
        editable: 0,
        dirty: 0,
        border_fill_id_ref: TABLE_BORDER_FILL_ID,
        sub_list: Some(HxSubList {
            id: String::new(),
            text_direction: "HORIZONTAL".to_string(),
            line_wrap: "BREAK".to_string(),
            vert_align: "CENTER".to_string(),
            link_list_id_ref: 0,
            link_list_next_id_ref: 0,
            text_width: 0,
            text_height: 0,
            has_text_ref: 0,
            has_num_ref: 0,
            paragraphs,
        }),
        cell_addr: Some(HxCellAddr { col_addr: col_idx, row_addr: row_idx }),
        cell_span: Some(HxCellSpan {
            col_span: cell.col_span as u32,
            row_span: cell.row_span as u32,
        }),
        cell_sz: Some(HxCellSz { width: cell.width.as_i32(), height: 0 }),
        cell_margin: Some(DEFAULT_CELL_MARGIN),
    })
}

/// Builds `HxPic` from a Core `Image`.
///
/// The `BinData/` prefix and file extension are stripped from the path
/// to produce the `binaryItemIDRef` attribute value. For example,
/// `"BinData/image1.png"` becomes `"image1"`. This matches 한글's
/// convention where `binaryItemIDRef` is a logical name without extension.
fn build_picture(img: &Image) -> HxPic {
    let without_prefix = img.path.strip_prefix("BinData/").unwrap_or(&img.path);
    // Strip extension: "image1.png" → "image1"
    let binary_ref = match without_prefix.rfind('.') {
        Some(dot) => &without_prefix[..dot],
        None => without_prefix,
    };

    HxPic {
        id: String::new(),
        img: Some(HxImg { binary_item_id_ref: binary_ref.to_string(), bright: 0, contrast: 0 }),
        org_sz: None,
        cur_sz: Some(HxSizeAttr { width: img.width.as_i32(), height: img.height.as_i32() }),
    }
}

// ── Linesegarray placeholder ─────────────────────────────────────

/// Default horizontal size for A4 with 30mm margins (59528 - 8504 - 8504).
const DEFAULT_HORZ_SIZE: i32 = 42520;

/// Default char height in HWPUNIT (10pt = 1000).
const DEFAULT_CHAR_HEIGHT: i32 = 1000;

/// Builds a minimal placeholder `HxLineSegArray` with one line segment.
///
/// Uses fixed defaults for char height (10pt) and calculates baseline
/// and spacing from standard ratios. 한글 recalculates these on open,
/// so exact values are not critical.
fn build_linesegarray(horzsize: i32) -> HxLineSegArray {
    let vertsize = DEFAULT_CHAR_HEIGHT;
    let baseline = vertsize * 85 / 100;
    let spacing = 600; // default for ~160% line spacing
    HxLineSegArray {
        items: vec![HxLineSeg {
            textpos: 0,
            vertpos: 0,
            vertsize,
            textheight: vertsize,
            baseline,
            spacing,
            horzpos: 0,
            horzsize,
            flags: 393216,
        }],
    }
}

// ── 한글 compatibility: secPr enrichment ────────────────────────

/// Enriched `<hp:secPr>` opening tag with all attributes 한글 expects.
const SEC_PR_OPEN_ENRICHED: &str = concat!(
    r#"<hp:secPr id="" textDirection="HORIZONTAL" "#,
    r#"spaceColumns="1134" tabStop="8000" tabStopVal="4000" tabStopUnit="HWPUNIT" "#,
    r#"outlineShapeIDRef="1" memoShapeIDRef="0" textVerticalWidthHead="0" masterPageCnt="0">"#,
);

/// Sub-elements inserted before `<hp:pagePr>` inside secPr.
const SEC_PR_PRE_ELEMENTS: &str = concat!(
    r#"<hp:grid lineGrid="0" charGrid="0" wonggojiFormat="0"/>"#,
    r#"<hp:startNum pageStartsOn="BOTH" page="0" pic="0" tbl="0" equation="0"/>"#,
    r#"<hp:visibility hideFirstHeader="0" hideFirstFooter="0" hideFirstMasterPage="0" "#,
    r#"border="SHOW_ALL" fill="SHOW_ALL" hideFirstPageNum="0" hideFirstEmptyLine="0" showLineNumber="0"/>"#,
    r#"<hp:lineNumberShape restartType="0" countBy="0" distance="0" startNumber="0"/>"#,
);

/// Sub-elements inserted after `</hp:pagePr>` and before `</hp:secPr>`.
const SEC_PR_POST_ELEMENTS: &str = concat!(
    r#"<hp:footNotePr>"#,
    r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    r##"<hp:noteLine length="-1" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    r#"<hp:noteSpacing betweenNotes="283" belowLine="567" aboveLine="850"/>"#,
    r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
    r#"<hp:placement place="EACH_COLUMN" beneathText="0"/>"#,
    r#"</hp:footNotePr>"#,
    r#"<hp:endNotePr>"#,
    r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar=")" supscript="0"/>"#,
    r##"<hp:noteLine length="14692344" type="SOLID" width="0.12 mm" color="#000000"/>"##,
    r#"<hp:noteSpacing betweenNotes="0" belowLine="567" aboveLine="850"/>"#,
    r#"<hp:numbering type="CONTINUOUS" newNum="1"/>"#,
    r#"<hp:placement place="END_OF_DOCUMENT" beneathText="0"/>"#,
    r#"</hp:endNotePr>"#,
    r#"<hp:pageBorderFill type="BOTH" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
    r#"<hp:pageBorderFill type="EVEN" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
    r#"<hp:pageBorderFill type="ODD" borderFillIDRef="1" textBorder="PAPER" headerInside="0" footerInside="0" fillArea="PAPER">"#,
    r#"<hp:offset left="1417" right="1417" top="1417" bottom="1417"/>"#,
    r#"</hp:pageBorderFill>"#,
);

/// Column properties injected after `</hp:secPr>` inside the first run.
///
/// Single-column default layout matching 한글's standard output.
const COL_PR_XML: &str = concat!(
    r#"<hp:ctrl>"#,
    r#"<hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1" sameSz="1" sameGap="0"/>"#,
    r#"</hp:ctrl>"#,
);

/// Enriches the minimal `<hp:secPr>` output with sub-elements required
/// by 한글 for proper rendering.
///
/// Replaces the opening tag with an enriched version carrying all expected
/// attributes, inserts grid/visibility elements before `<hp:pagePr>`,
/// appends footnote/endnote/pageBorderFill after `</hp:pagePr>`,
/// and injects `<hp:ctrl><hp:colPr>` after the closing `</hp:secPr>`.
fn enrich_sec_pr(xml: &str) -> String {
    let minimal_open = r#"<hp:secPr textDirection="HORIZONTAL">"#;

    // If no secPr to enrich, return as-is
    if !xml.contains(minimal_open) {
        return xml.to_string();
    }

    let mut result =
        xml.replacen(minimal_open, &format!("{SEC_PR_OPEN_ENRICHED}{SEC_PR_PRE_ELEMENTS}"), 1);

    // Insert post-elements before the first </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        result.insert_str(pos, SEC_PR_POST_ELEMENTS);
    }

    // Insert colPr after </hp:secPr>
    if let Some(pos) = result.find("</hp:secPr>") {
        let insert_pos = pos + "</hp:secPr>".len();
        result.insert_str(insert_pos, COL_PR_XML);
    }

    result
}

// ── Header/Footer/PageNumber injection ──────────────────────────

/// Injects header, footer, and page number `<hp:ctrl>` blocks into
/// the section XML after the colPr ctrl (in the first run).
///
/// In real HWPX from 한글, these appear as:
/// - `<hp:ctrl><hp:header><hp:p>...</hp:p></hp:header></hp:ctrl>`
/// - `<hp:ctrl><hp:footer><hp:p>...</hp:p></hp:footer></hp:ctrl>`
/// - `<hp:ctrl><hp:autoNum numType="PAGE" ...></hp:ctrl>`
fn inject_header_footer_pagenum(xml: &mut String, section: &Section) {
    // Find insertion point: after the last </hp:ctrl> that contains colPr
    // (or after </hp:secPr> if no colPr).
    // We inject after the colPr ctrl block.
    let insert_pos = find_ctrl_injection_point(xml);
    if insert_pos == 0 {
        return; // no suitable injection point found
    }

    let mut injection = String::new();

    // Header
    if let Some(ref header) = section.header {
        injection.push_str(&build_header_xml(header, "header"));
    }

    // Footer
    if let Some(ref footer) = section.footer {
        injection.push_str(&build_header_xml(footer, "footer"));
    }

    // Page number
    if let Some(ref page_number) = section.page_number {
        injection.push_str(&build_page_number_xml(page_number));
    }

    if !injection.is_empty() {
        xml.insert_str(insert_pos, &injection);
    }
}

/// Finds the insertion point for header/footer/pagenum ctrl blocks.
///
/// Returns the byte offset after the colPr `</hp:ctrl>` block.
/// Falls back to after `</hp:secPr>` if no colPr is found.
fn find_ctrl_injection_point(xml: &str) -> usize {
    // Look for colPr ctrl: find "</hp:colPr>" and then the next "</hp:ctrl>"
    if let Some(col_pr_pos) = xml.find("</hp:colPr>") {
        if let Some(ctrl_close) = xml[col_pr_pos..].find("</hp:ctrl>") {
            return col_pr_pos + ctrl_close + "</hp:ctrl>".len();
        }
    }
    // Fallback: after </hp:secPr>
    if let Some(sec_pr_end) = xml.find("</hp:secPr>") {
        return sec_pr_end + "</hp:secPr>".len();
    }
    0
}

/// Builds `<hp:ctrl><hp:header>` or `<hp:ctrl><hp:footer>` XML.
///
/// `tag_name` should be `"header"` or `"footer"`.
fn build_header_xml(hf: &hwpforge_core::section::HeaderFooter, tag_name: &str) -> String {
    use std::fmt::Write as _;

    let apply_page = match hf.apply_page_type {
        hwpforge_foundation::ApplyPageType::Both => "BOTH",
        hwpforge_foundation::ApplyPageType::Even => "EVEN",
        hwpforge_foundation::ApplyPageType::Odd => "ODD",
        _ => "BOTH",
    };

    let mut xml = String::new();
    write!(xml, r#"<hp:ctrl><hp:{tag_name} applyPageType="{apply_page}" createItemType="0">"#,)
        .expect("write to String is infallible");

    // Wrap paragraphs in <hp:subList> (required by HWPX schema)
    xml.push_str(
        r#"<hp:subList id="0" textDirection="HORIZONTAL" lineWrap="BREAK" vertAlign="TOP" linkListIDRef="0" linkListNextIDRef="0" textWidth="0" textHeight="0">"#,
    );

    // Encode each paragraph in the header/footer
    for (idx, para) in hf.paragraphs.iter().enumerate() {
        write!(
            xml,
            r#"<hp:p id="{idx}" paraPrIDRef="{}" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">"#,
            para.para_shape_id.get(),
        )
        .expect("write to String is infallible");

        for run in &para.runs {
            if let hwpforge_core::run::RunContent::Text(text) = &run.content {
                write!(
                    xml,
                    r#"<hp:run charPrIDRef="{}"><hp:t>{}</hp:t></hp:run>"#,
                    run.char_shape_id.get(),
                    escape_xml(text),
                )
                .expect("write to String is infallible");
            }
        }

        xml.push_str("</hp:p>");
    }

    xml.push_str("</hp:subList>");
    write!(xml, "</hp:{tag_name}></hp:ctrl>").expect("write to String is infallible");
    xml
}

/// Builds `<hp:ctrl><hp:autoNum>` XML for page numbers.
fn build_page_number_xml(pn: &hwpforge_core::section::PageNumber) -> String {
    use std::fmt::Write as _;

    let num_type = match pn.number_format {
        hwpforge_foundation::NumberFormatType::Digit => "PAGE",
        hwpforge_foundation::NumberFormatType::CircledDigit => "PAGE",
        hwpforge_foundation::NumberFormatType::RomanCapital => "PAGE",
        hwpforge_foundation::NumberFormatType::RomanSmall => "PAGE",
        _ => "PAGE",
    };

    let mut xml = String::new();
    write!(
        xml,
        r#"<hp:ctrl><hp:autoNum numType="{num_type}" sideChar="{side_char}"/></hp:ctrl>"#,
        side_char = escape_xml(&pn.side_char),
    )
    .expect("write to String is infallible");
    xml
}

/// Escapes XML special characters in text content.
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;
    use hwpforge_core::image::ImageFormat;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

    /// Helper: build a simple text paragraph.
    fn text_paragraph(text: &str, para_shape: usize, char_shape: usize) -> Paragraph {
        Paragraph::with_runs(
            vec![Run::text(text, CharShapeIndex::new(char_shape))],
            ParaShapeIndex::new(para_shape),
        )
    }

    /// Helper: build a section with one text paragraph.
    fn simple_section(text: &str) -> Section {
        Section::with_paragraphs(vec![text_paragraph(text, 0, 0)], PageSettings::a4())
    }

    // ── Test 1: Single text paragraph ────────────────────────────

    #[test]
    fn encode_single_text_paragraph() {
        let section = simple_section("텍스트");
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<?xml version="), "missing XML declaration");
        assert!(xml.contains("<hs:sec"), "missing <hs:sec> root");
        assert!(xml.contains("</hs:sec>"), "missing </hs:sec> close");
        assert!(xml.contains("<hp:p "), "missing <hp:p>");

        // Verify Gap 6: colPr is injected after </hp:secPr>
        assert!(xml.contains("<hp:ctrl>"), "missing <hp:ctrl>");
        assert!(
            xml.contains("<hp:colPr id=\"\" type=\"NEWSPAPER\" layout=\"LEFT\" colCount=\"1\""),
            "missing colPr with correct attributes"
        );
        assert!(xml.contains("sameSz=\"1\" sameGap=\"0\""), "colPr missing sameSz/sameGap");

        // Verify colPr appears AFTER </hp:secPr> and BEFORE <hp:t>
        let sec_pr_end = xml.find("</hp:secPr>").expect("secPr must be present");
        let col_pr_pos = xml.find("<hp:colPr").expect("colPr must be present");
        assert!(col_pr_pos > sec_pr_end, "colPr must come after </hp:secPr>");
        assert!(xml.contains("<hp:run "), "missing <hp:run>");
        assert!(xml.contains("<hp:t>텍스트</hp:t>"), "missing text content");
        assert!(xml.contains(r#"xmlns:hp="#), "missing xmlns:hp namespace");
    }

    // ── Test 2: Section roundtrip via decoder ────────────────────

    #[test]
    fn encode_section_roundtrip() {
        let section = simple_section("안녕하세요 round-trip test");
        let xml = encode_section(&section, 0).unwrap();

        // Parse back with the decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        assert_eq!(result.paragraphs.len(), 1);
        assert_eq!(
            result.paragraphs[0].runs[0].content.as_text(),
            Some("안녕하세요 round-trip test"),
        );
        assert_eq!(result.paragraphs[0].para_shape_id.get(), 0);
    }

    // ── Test 3: SecPr injection ──────────────────────────────────

    #[test]
    fn sec_pr_injection() {
        let ps = PageSettings {
            width: HwpUnit::new(59528).unwrap(),
            height: HwpUnit::new(84188).unwrap(),
            margin_left: HwpUnit::new(8504).unwrap(),
            margin_right: HwpUnit::new(8504).unwrap(),
            margin_top: HwpUnit::new(5668).unwrap(),
            margin_bottom: HwpUnit::new(4252).unwrap(),
            header_margin: HwpUnit::new(4252).unwrap(),
            footer_margin: HwpUnit::new(4252).unwrap(),
        };
        let section = Section::with_paragraphs(vec![text_paragraph("Content", 0, 0)], ps);
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hp:secPr"), "missing secPr");
        assert!(xml.contains(r#"textDirection="HORIZONTAL""#), "missing textDirection");
        assert!(xml.contains(r#"width="59528""#), "missing width");
        assert!(xml.contains(r#"height="84188""#), "missing height");
        assert!(xml.contains(r#"left="8504""#), "missing left margin");

        // Roundtrip the page settings through the decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let decoded_ps = result.page_settings.unwrap();
        assert_eq!(decoded_ps.width.as_i32(), 59528);
        assert_eq!(decoded_ps.height.as_i32(), 84188);
        assert_eq!(decoded_ps.margin_left.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_right.as_i32(), 8504);
        assert_eq!(decoded_ps.margin_top.as_i32(), 5668);
        assert_eq!(decoded_ps.margin_bottom.as_i32(), 4252);
        assert_eq!(decoded_ps.header_margin.as_i32(), 4252);
        assert_eq!(decoded_ps.footer_margin.as_i32(), 4252);
    }

    // ── Test 4: Table encoding ───────────────────────────────────

    #[test]
    fn table_encoding() {
        let cell1 =
            TableCell::new(vec![text_paragraph("Cell1", 0, 0)], HwpUnit::new(5000).unwrap());
        let cell2 =
            TableCell::new(vec![text_paragraph("Cell2", 0, 0)], HwpUnit::new(5000).unwrap());
        let row = TableRow { cells: vec![cell1, cell2], height: None };
        let table = Table::new(vec![row]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains(r#"rowCnt="1""#), "missing rowCnt");
        assert!(xml.contains(r#"colCnt="2""#), "missing colCnt");
        assert!(xml.contains("<hp:t>Cell1</hp:t>"), "missing Cell1 text");
        assert!(xml.contains("<hp:t>Cell2</hp:t>"), "missing Cell2 text");
    }

    // ── Test 5: Image encoding ───────────────────────────────────

    #[test]
    fn image_encoding() {
        let img = Image::new(
            "BinData/logo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        );
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(img, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();

        assert!(
            xml.contains(r#"binaryItemIDRef="logo""#),
            "missing binaryItemIDRef (should strip BinData/ prefix and extension)"
        );
        assert!(xml.contains(r#"width="10000""#), "missing image width");
        assert!(xml.contains(r#"height="5000""#), "missing image height");
    }

    // ── Test 6: Multiple paragraphs ──────────────────────────────

    #[test]
    fn multi_paragraph() {
        let section = Section::with_paragraphs(
            vec![
                text_paragraph("First", 0, 0),
                text_paragraph("Second", 1, 0),
                text_paragraph("Third", 2, 0),
            ],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hp:t>First</hp:t>"), "missing First");
        assert!(xml.contains("<hp:t>Second</hp:t>"), "missing Second");
        assert!(xml.contains("<hp:t>Third</hp:t>"), "missing Third");

        // secPr should only appear once (in the first paragraph)
        let sec_pr_count = xml.matches("<hp:secPr").count();
        assert_eq!(sec_pr_count, 1, "secPr should appear exactly once, in first paragraph");
    }

    // ── Test 7: Nested table ─────────────────────────────────────

    #[test]
    fn nested_table() {
        // Inner table
        let inner_cell =
            TableCell::new(vec![text_paragraph("Deep", 0, 0)], HwpUnit::new(3000).unwrap());
        let inner_table = Table::new(vec![TableRow { cells: vec![inner_cell], height: None }]);

        // Outer table: cell contains a paragraph with the inner table
        let outer_cell = TableCell {
            paragraphs: vec![Paragraph::with_runs(
                vec![Run::table(inner_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            col_span: 1,
            row_span: 1,
            width: HwpUnit::new(8000).unwrap(),
            background: None,
        };
        let outer_table = Table::new(vec![TableRow { cells: vec![outer_cell], height: None }]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(outer_table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );

        // Should succeed within nesting limit
        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:t>Deep</hp:t>"), "missing nested text");
    }

    // ── Test 8: Control is skipped ───────────────────────────────

    #[test]
    fn control_skipped() {
        use hwpforge_core::control::Control;

        let ctrl =
            Control::Hyperlink { text: "link".to_string(), url: "https://example.com".to_string() };
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![
                    Run::text("before", CharShapeIndex::new(0)),
                    Run::control(ctrl, CharShapeIndex::new(0)),
                    Run::text("after", CharShapeIndex::new(0)),
                ],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hp:t>before</hp:t>"), "missing 'before' text");
        assert!(xml.contains("<hp:t>after</hp:t>"), "missing 'after' text");
        // Control content should not appear
        assert!(!xml.contains("example.com"), "control content should not appear in XML");
    }

    // ── Test 9: Empty text produces valid XML ────────────────────

    #[test]
    fn empty_text_produces_valid_xml() {
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::text("", CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();

        // Should parse without error
        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    // ── Test 10: Korean text preservation ────────────────────────

    #[test]
    fn korean_text_preservation() {
        let korean = "우리는 수학을 공부한다.";
        let section = simple_section(korean);
        let xml = encode_section(&section, 0).unwrap();

        // Roundtrip through decoder
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        assert_eq!(result.paragraphs[0].runs[0].content.as_text(), Some(korean),);
    }

    // ── Additional edge cases ────────────────────────────────────

    #[test]
    fn empty_section_produces_valid_xml() {
        let section = Section::new(PageSettings::a4());
        let xml = encode_section(&section, 0).unwrap();

        assert!(xml.contains("<hs:sec"), "missing root element");
        assert!(xml.contains("</hs:sec>"), "missing close tag");
    }

    #[test]
    fn strip_root_element_basic() {
        let xml = "<sec><hp:p>inner</hp:p></sec>";
        assert_eq!(strip_root_element(xml), "<hp:p>inner</hp:p>");
    }

    #[test]
    fn strip_root_element_self_closing() {
        assert_eq!(strip_root_element("<sec/>"), "");
    }

    #[test]
    fn strip_root_element_with_attributes() {
        let xml = r#"<sec attr="val"><hp:p>x</hp:p></sec>"#;
        assert_eq!(strip_root_element(xml), "<hp:p>x</hp:p>");
    }

    #[test]
    fn nesting_depth_exceeded() {
        let hx_table = Table::new(vec![]);
        let err = build_table(&hx_table, MAX_NESTING_DEPTH).unwrap_err();
        match &err {
            HwpxError::InvalidStructure { detail } => {
                assert!(detail.contains("nesting depth"));
            }
            _ => panic!("expected InvalidStructure, got: {err:?}"),
        }
    }

    #[test]
    fn image_without_bindata_prefix() {
        let img = Image::new(
            "image.jpg",
            HwpUnit::new(1000).unwrap(),
            HwpUnit::new(500).unwrap(),
            ImageFormat::Jpeg,
        );
        let hx = build_picture(&img);
        assert_eq!(
            hx.img.unwrap().binary_item_id_ref,
            "image",
            "path without BinData/ prefix should strip extension only"
        );
    }

    #[test]
    fn paragraph_shape_id_preserved_in_roundtrip() {
        let section = Section::with_paragraphs(
            vec![text_paragraph("p0", 3, 5), text_paragraph("p1", 7, 2)],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

        assert_eq!(result.paragraphs[0].para_shape_id.get(), 3);
        assert_eq!(result.paragraphs[0].runs[0].char_shape_id.get(), 5);
        assert_eq!(result.paragraphs[1].para_shape_id.get(), 7);
        assert_eq!(result.paragraphs[1].runs[0].char_shape_id.get(), 2);
    }

    #[test]
    fn table_roundtrip_via_decoder() {
        let cell = TableCell::new(vec![text_paragraph("Hello", 0, 0)], HwpUnit::new(5000).unwrap());
        let table = Table::new(vec![TableRow { cells: vec![cell], height: None }]);

        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

        let decoded_table = result.paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(decoded_table.rows.len(), 1);
        assert_eq!(decoded_table.rows[0].cells.len(), 1);
        assert_eq!(
            decoded_table.rows[0].cells[0].paragraphs[0].runs[0].content.as_text(),
            Some("Hello"),
        );
        assert_eq!(decoded_table.rows[0].cells[0].width.as_i32(), 5000);
    }

    #[test]
    fn image_roundtrip_via_decoder() {
        let img = Image::new(
            "BinData/photo.png",
            HwpUnit::new(10000).unwrap(),
            HwpUnit::new(5000).unwrap(),
            ImageFormat::Png,
        );
        let section = Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::image(img, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        );
        let xml = encode_section(&section, 0).unwrap();
        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();

        let decoded_img = result.paragraphs[0].runs[0].content.as_image().unwrap();
        // binaryItemIDRef is "photo" (extension stripped by encoder),
        // so decoder reconstructs path as "BinData/photo"
        assert_eq!(decoded_img.path, "BinData/photo");
        assert_eq!(decoded_img.width.as_i32(), 10000);
        assert_eq!(decoded_img.height.as_i32(), 5000);
    }

    // ── Header / Footer / PageNum encoder roundtrip ─────────────

    #[test]
    fn header_roundtrip_via_decoder() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body text");
        section.header = Some(HeaderFooter::new(
            vec![text_paragraph("Header Content", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:header"), "XML should contain header element");
        assert!(xml.contains("Header Content"), "XML should contain header text");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let header = result.header.expect("decoded section should have header");
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(header.paragraphs.len(), 1);
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("Header Content"));
    }

    #[test]
    fn footer_roundtrip_via_decoder() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body text");
        section.footer = Some(HeaderFooter::new(
            vec![text_paragraph("Footer Content", 0, 0)],
            ApplyPageType::Even,
        ));

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:footer"), "XML should contain footer element");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let footer = result.footer.expect("decoded section should have footer");
        assert_eq!(footer.apply_page_type, ApplyPageType::Even);
        assert_eq!(footer.paragraphs.len(), 1);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("Footer Content"));
    }

    #[test]
    fn page_number_roundtrip_via_decoder() {
        use hwpforge_core::section::PageNumber;
        use hwpforge_foundation::{NumberFormatType, PageNumberPosition};

        let mut section = simple_section("Body text");
        section.page_number = Some(PageNumber::with_side_char(
            PageNumberPosition::BottomCenter,
            NumberFormatType::Digit,
            "- ".to_string(),
        ));

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("<hp:autoNum"), "XML should contain autoNum element");
        assert!(xml.contains("sideChar=\"- \""), "XML should contain side char");

        // NOTE: The encoder outputs <hp:autoNum> but the schema decodes <hp:pageNum>.
        // The autoNum element has different structure than pageNum, so full roundtrip
        // of page number through encode→decode is only partially possible.
        // This test validates the encoder output contains the right data.
    }

    #[test]
    fn header_and_footer_together_roundtrip() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Main body");
        section.header =
            Some(HeaderFooter::new(vec![text_paragraph("My Header", 0, 0)], ApplyPageType::Both));
        section.footer =
            Some(HeaderFooter::new(vec![text_paragraph("My Footer", 0, 0)], ApplyPageType::Odd));

        let xml = encode_section(&section, 0).unwrap();

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let header = result.header.expect("should have header");
        let footer = result.footer.expect("should have footer");

        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("My Header"));
        assert_eq!(header.apply_page_type, ApplyPageType::Both);
        assert_eq!(footer.paragraphs[0].runs[0].content.as_text(), Some("My Footer"));
        assert_eq!(footer.apply_page_type, ApplyPageType::Odd);
    }

    #[test]
    fn xml_special_chars_escaped_in_header() {
        use hwpforge_core::section::HeaderFooter;
        use hwpforge_foundation::ApplyPageType;

        let mut section = simple_section("Body");
        section.header = Some(HeaderFooter::new(
            vec![text_paragraph("A & B < C > D", 0, 0)],
            ApplyPageType::Both,
        ));

        let xml = encode_section(&section, 0).unwrap();
        assert!(xml.contains("A &amp; B &lt; C &gt; D"), "special chars must be escaped");

        let result = crate::decoder::section::parse_section(&xml, 0).unwrap();
        let header = result.header.expect("should have header");
        assert_eq!(header.paragraphs[0].runs[0].content.as_text(), Some("A & B < C > D"),);
    }
}
