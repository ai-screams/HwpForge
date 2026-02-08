//! Parses `Contents/section*.xml` into Core paragraphs and page settings.
//!
//! Converts XML schema types (`HxParagraph`, `HxRun`, `HxText`, `HxTable`,
//! `HxPic`) into Core types (`Paragraph`, `Run`, `RunContent`, `Table`, `Image`).

use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use quick_xml::de::from_str;

use crate::error::{HwpxError, HwpxResult};
use crate::schema::section::{
    HxParagraph, HxPic, HxRun, HxSection, HxTable, HxTableCell,
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
}

/// Parses a section XML string into paragraphs and optional page settings.
///
/// `section_index` is used only for error messages (e.g. `"Contents/section0.xml"`).
pub fn parse_section(xml: &str, section_index: usize) -> HwpxResult<SectionParseResult> {
    let file_hint = format!("Contents/section{section_index}.xml");
    let section: HxSection = from_str(xml).map_err(|e| HwpxError::XmlParse {
        file: file_hint,
        detail: e.to_string(),
    })?;

    let mut page_settings = None;

    let paragraphs = section
        .paragraphs
        .iter()
        .enumerate()
        .map(|(para_idx, hx_para)| {
            let (para, ps) = convert_paragraph(hx_para, para_idx == 0, 0)?;
            if ps.is_some() && page_settings.is_none() {
                page_settings = ps;
            }
            Ok(para)
        })
        .collect::<HwpxResult<Vec<_>>>()?;

    Ok(SectionParseResult { paragraphs, page_settings })
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

    let paragraph = Paragraph { runs, para_shape_id };
    Ok((paragraph, page_settings))
}

/// Converts an `HxRun` into one or more Core `Run`s.
///
/// A single HxRun can contain multiple `<hp:t>`, `<hp:tbl>`, and `<hp:pic>`
/// elements. Each is converted to a separate Run with the same charPrIDRef.
fn convert_run(hx: &HxRun, depth: usize) -> HwpxResult<Vec<Run>> {
    let char_shape_id = CharShapeIndex::new(hx.char_pr_id_ref as usize);
    let mut runs = Vec::new();

    // Text runs
    for text in &hx.texts {
        if !text.text.is_empty() {
            runs.push(Run {
                content: RunContent::Text(text.text.clone()),
                char_shape_id,
            });
        }
    }

    // Table runs
    for table in &hx.tables {
        let core_table = convert_table(table, depth)?;
        runs.push(Run {
            content: RunContent::Table(Box::new(core_table)),
            char_shape_id,
        });
    }

    // Image runs
    for pic in &hx.pictures {
        if let Some(image) = convert_picture(pic) {
            runs.push(Run {
                content: RunContent::Image(image),
                char_shape_id,
            });
        }
    }

    Ok(runs)
}

/// Converts an `HxTable` into a Core `Table`.
fn convert_table(hx: &HxTable, depth: usize) -> HwpxResult<Table> {
    if depth >= MAX_NESTING_DEPTH {
        return Err(HwpxError::InvalidStructure {
            detail: format!(
                "table nesting depth {} exceeds limit of {}",
                depth, MAX_NESTING_DEPTH,
            ),
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

    Ok(Table { rows, width: None, caption: None })
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

    let (col_span, row_span) = hx
        .cell_span
        .as_ref()
        .map(|cs| (cs.col_span as u16, cs.row_span as u16))
        .unwrap_or((1, 1));

    let width = hx
        .cell_sz
        .as_ref()
        .and_then(|sz| HwpUnit::new(sz.width).ok())
        .unwrap_or(HwpUnit::ZERO);

    Ok(TableCell {
        paragraphs,
        col_span,
        row_span,
        width,
        background: None,
    })
}

/// Converts an `HxPic` into a Core `Image`, if it has a valid image reference.
fn convert_picture(hx: &HxPic) -> Option<Image> {
    let img = hx.img.as_ref()?;
    if img.binary_item_id_ref.is_empty() {
        return None;
    }

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

    Some(Image { path, width, height, format })
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
fn extract_page_settings(
    sec_pr: &crate::schema::section::HxSecPr,
) -> Option<PageSettings> {
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Text-only sections ───────────────────────────────────────

    #[test]
    fn parse_empty_section() {
        let xml = r#"<sec></sec>"#;
        let result = parse_section(xml, 0).unwrap();
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
        let result = parse_section(xml, 0).unwrap();
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
        let result = parse_section(xml, 0).unwrap();
        let para = &result.paragraphs[0];
        assert_eq!(para.runs.len(), 2);
        assert_eq!(para.runs[0].char_shape_id.get(), 0);
        assert_eq!(para.runs[0].content.as_text(), Some("Hello"));
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
        let result = parse_section(xml, 0).unwrap();
        assert_eq!(result.paragraphs.len(), 3);
        assert_eq!(result.paragraphs[2].para_shape_id.get(), 2);
    }

    #[test]
    fn empty_text_is_skipped() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0"><t/></run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0).unwrap();
        assert!(result.paragraphs[0].runs.is_empty());
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
        let result = parse_section(xml, 0).unwrap();
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
        let result = parse_section(xml, 0).unwrap();
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
        let result = parse_section(xml, 0).unwrap();
        let run = &result.paragraphs[0].runs[0];

        match &run.content {
            RunContent::Table(table) => {
                assert_eq!(table.rows.len(), 2);
                assert_eq!(table.rows[0].cells.len(), 2);

                let cell0 = &table.rows[0].cells[0];
                assert_eq!(cell0.col_span, 1);
                assert_eq!(cell0.row_span, 1);
                assert_eq!(cell0.width.as_i32(), 5000);
                assert_eq!(
                    cell0.paragraphs[0].runs[0].content.as_text(),
                    Some("Cell1"),
                );

                let cell3 = &table.rows[1].cells[0];
                assert_eq!(
                    cell3.paragraphs[0].runs[0].content.as_text(),
                    Some("Cell3"),
                );
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
        let result = parse_section(xml, 0).unwrap();
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
        let result = parse_section(xml, 0).unwrap();
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
    fn picture_without_img_child_is_skipped() {
        let xml = r#"<sec>
            <p paraPrIDRef="0">
                <run charPrIDRef="0">
                    <pic id="pic1"/>
                </run>
            </p>
        </sec>"#;
        let result = parse_section(xml, 0).unwrap();
        assert!(result.paragraphs[0].runs.is_empty());
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
        assert!(matches!(
            guess_image_format("unknown"),
            ImageFormat::Unknown(_)
        ));
    }

    // ── Error cases ──────────────────────────────────────────────

    #[test]
    fn parse_invalid_xml() {
        let err = parse_section("<not-closed", 0).unwrap_err();
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
        assert!(parse_section(xml, 0).is_ok());
    }

    #[test]
    fn nesting_depth_exceeded_returns_error() {
        use crate::schema::section::HxTable;
        // Directly call convert_table at max depth to trigger the limit
        let hx = HxTable { row_cnt: 0, col_cnt: 0, rows: vec![] };
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
        let result = parse_section(xml, 0).unwrap();
        assert_eq!(
            result.paragraphs[0].runs[0].content.as_text(),
            Some("우리는 수학을 공부한다."),
        );
    }
}
