//! XML schema types for `section*.xml` (hp:, hs: namespaces).
//!
//! Maps the `<hs:sec>` element tree into Rust structs via serde.
//! Unknown elements (shapes, controls, line segments) are silently
//! ignored for Phase 3 — we extract text, tables, images only.
//!
//! Fields are used by serde deserialization even if not directly accessed.
#![allow(dead_code)]

use serde::Deserialize;

// ── Section root ──────────────────────────────────────────────────

/// `<hs:sec>` — root element of section*.xml.
#[derive(Debug, Deserialize)]
#[serde(rename = "sec")]
pub struct HxSection {
    #[serde(rename = "p", default)]
    pub paragraphs: Vec<HxParagraph>,
}

// ── Paragraph ─────────────────────────────────────────────────────

/// `<hp:p id="..." paraPrIDRef="3" styleIDRef="0" ...>`.
#[derive(Debug, Deserialize)]
pub struct HxParagraph {
    #[serde(rename = "@id", default)]
    pub id: String,
    #[serde(rename = "@paraPrIDRef", default)]
    pub para_pr_id_ref: u32,
    #[serde(rename = "@styleIDRef", default)]
    pub style_id_ref: u32,
    #[serde(rename = "@pageBreak", default)]
    pub page_break: u32,
    #[serde(rename = "@columnBreak", default)]
    pub column_break: u32,
    #[serde(rename = "@merged", default)]
    pub merged: u32,

    #[serde(rename = "run", default)]
    pub runs: Vec<HxRun>,
    // <hp:linesegarray> — silently ignored
}

// ── Run ───────────────────────────────────────────────────────────

/// `<hp:run charPrIDRef="0">`.
///
/// A run can contain multiple mixed children:
/// `<hp:secPr>`, `<hp:ctrl>`, `<hp:t>`, `<hp:tbl>`, `<hp:pic>`,
/// `<hp:rect>`, `<hp:ellipse>`, etc.
///
/// Phase 3 extracts text, tables, images, and secPr; everything else
/// is silently skipped by serde (no `deny_unknown_fields`).
#[derive(Debug, Deserialize)]
pub struct HxRun {
    #[serde(rename = "@charPrIDRef", default)]
    pub char_pr_id_ref: u32,

    /// Section properties (typically in the first run of the first paragraph).
    #[serde(rename = "secPr", default)]
    pub sec_pr: Option<HxSecPr>,

    /// All `<hp:t>` elements in this run (may be multiple).
    #[serde(rename = "t", default)]
    pub texts: Vec<HxText>,

    /// All `<hp:tbl>` elements in this run.
    #[serde(rename = "tbl", default)]
    pub tables: Vec<HxTable>,

    /// All `<hp:pic>` elements in this run.
    #[serde(rename = "pic", default)]
    pub pictures: Vec<HxPic>,

    // hp:ctrl, hp:rect, hp:ellipse, etc. — ignored
}

// ── Text ──────────────────────────────────────────────────────────

/// `<hp:t>수학</hp:t>` or `<hp:t/>` (empty).
#[derive(Debug, Deserialize)]
pub struct HxText {
    #[serde(rename = "$text", default)]
    pub text: String,
}

// ── Section Properties ────────────────────────────────────────────

/// `<hp:secPr>` — section settings, embedded in the first paragraph.
#[derive(Debug, Deserialize)]
pub struct HxSecPr {
    #[serde(rename = "@textDirection", default)]
    pub text_direction: String,

    #[serde(rename = "pagePr", default)]
    pub page_pr: Option<HxPagePr>,
    // grid, startNum, visibility, lineNumberShape, footNotePr,
    // endNotePr, pageBorderFill — ignored (Phase 3)
}

/// `<hp:pagePr landscape="WIDELY" width="59528" height="84188">`.
#[derive(Debug, Deserialize)]
pub struct HxPagePr {
    #[serde(rename = "@landscape", default)]
    pub landscape: String,
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
    #[serde(rename = "@gutterType", default)]
    pub gutter_type: String,

    #[serde(rename = "margin", default)]
    pub margin: Option<HxPageMargin>,
}

/// `<hp:margin header="4252" footer="4252" gutter="0" left="8504" ...>`.
#[derive(Debug, Deserialize, Default)]
pub struct HxPageMargin {
    #[serde(rename = "@header", default)]
    pub header: i32,
    #[serde(rename = "@footer", default)]
    pub footer: i32,
    #[serde(rename = "@gutter", default)]
    pub gutter: i32,
    #[serde(rename = "@left", default)]
    pub left: i32,
    #[serde(rename = "@right", default)]
    pub right: i32,
    #[serde(rename = "@top", default)]
    pub top: i32,
    #[serde(rename = "@bottom", default)]
    pub bottom: i32,
}

// ── Table ─────────────────────────────────────────────────────────

/// `<hp:tbl ... rowCnt="3" colCnt="2">`.
#[derive(Debug, Deserialize)]
pub struct HxTable {
    #[serde(rename = "@rowCnt", default)]
    pub row_cnt: u32,
    #[serde(rename = "@colCnt", default)]
    pub col_cnt: u32,

    #[serde(rename = "tr", default)]
    pub rows: Vec<HxTableRow>,
    // hp:caption, hp:tblPr — ignored
}

/// `<hp:tr>`.
#[derive(Debug, Deserialize)]
pub struct HxTableRow {
    #[serde(rename = "tc", default)]
    pub cells: Vec<HxTableCell>,
}

/// `<hp:tc>`.
#[derive(Debug, Deserialize)]
pub struct HxTableCell {
    #[serde(rename = "@name", default)]
    pub name: String,

    #[serde(rename = "cellSpan", default)]
    pub cell_span: Option<HxCellSpan>,
    #[serde(rename = "cellSz", default)]
    pub cell_sz: Option<HxCellSz>,
    #[serde(rename = "subList", default)]
    pub sub_list: Option<HxSubList>,
    // hp:cellAddr, hp:cellMargin, hh:borderFill — ignored
}

/// `<hp:cellSpan rowSpan="1" colSpan="1"/>`.
#[derive(Debug, Deserialize)]
pub struct HxCellSpan {
    #[serde(rename = "@rowSpan", default = "default_one")]
    pub row_span: u32,
    #[serde(rename = "@colSpan", default = "default_one")]
    pub col_span: u32,
}

fn default_one() -> u32 {
    1
}

/// `<hp:cellSz width="..." height="..."/>`.
#[derive(Debug, Deserialize)]
pub struct HxCellSz {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
}

/// `<hp:subList>` — container for paragraphs inside a table cell.
#[derive(Debug, Deserialize)]
pub struct HxSubList {
    #[serde(rename = "p", default)]
    pub paragraphs: Vec<HxParagraph>,
}

// ── Picture / Image ───────────────────────────────────────────────

/// `<hp:pic>` — image container.
#[derive(Debug, Deserialize)]
pub struct HxPic {
    #[serde(rename = "@id", default)]
    pub id: String,

    #[serde(rename = "img", default)]
    pub img: Option<HxImg>,
    #[serde(rename = "orgSz", default)]
    pub org_sz: Option<HxSizeAttr>,
    #[serde(rename = "curSz", default)]
    pub cur_sz: Option<HxSizeAttr>,
    // lineShape, fillBrush, shadow, pos, sz — ignored
}

/// `<hp:img binaryItemIDRef="image1" bright="0" contrast="0" .../>` or
/// `<hc:img binaryItemIDRef="..."/>`.
#[derive(Debug, Deserialize)]
pub struct HxImg {
    #[serde(rename = "@binaryItemIDRef", default)]
    pub binary_item_id_ref: String,
    #[serde(rename = "@bright", default)]
    pub bright: i32,
    #[serde(rename = "@contrast", default)]
    pub contrast: i32,
}

/// Generic width/height attribute pair used in `<hp:orgSz>`, `<hp:curSz>`.
#[derive(Debug, Deserialize)]
pub struct HxSizeAttr {
    #[serde(rename = "@width", default)]
    pub width: i32,
    #[serde(rename = "@height", default)]
    pub height: i32,
}

// ── Tests ─────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_section(xml: &str) -> HxSection {
        quick_xml::de::from_str(xml).expect("failed to parse HxSection")
    }

    #[test]
    fn parse_minimal_section() {
        let xml = r#"<hs:sec></hs:sec>"#;
        let sec = parse_section(xml);
        assert!(sec.paragraphs.is_empty());
    }

    #[test]
    fn parse_single_text_paragraph() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0" pageBreak="0" columnBreak="0" merged="0">
            <hp:run charPrIDRef="0">
              <hp:t>안녕하세요</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs.len(), 1);
        let p = &sec.paragraphs[0];
        assert_eq!(p.para_pr_id_ref, 3);
        assert_eq!(p.style_id_ref, 0);
        assert_eq!(p.runs.len(), 1);
        assert_eq!(p.runs[0].char_pr_id_ref, 0);
        assert_eq!(p.runs[0].texts.len(), 1);
        assert_eq!(p.runs[0].texts[0].text, "안녕하세요");
    }

    #[test]
    fn parse_multiple_text_runs() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t>Hello</hp:t>
            </hp:run>
            <hp:run charPrIDRef="7">
              <hp:t>World</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let p = &sec.paragraphs[0];
        assert_eq!(p.runs.len(), 2);
        assert_eq!(p.runs[0].texts[0].text, "Hello");
        assert_eq!(p.runs[1].char_pr_id_ref, 7);
        assert_eq!(p.runs[1].texts[0].text, "World");
    }

    #[test]
    fn parse_empty_text_element() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t/>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "");
    }

    #[test]
    fn parse_sec_pr_with_page_settings() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="3" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:secPr textDirection="HORIZONTAL">
                <hp:pagePr landscape="WIDELY" width="59528" height="84188" gutterType="LEFT_ONLY">
                  <hp:margin header="4252" footer="4252" gutter="0" left="8504" right="8504" top="5668" bottom="4252"/>
                </hp:pagePr>
              </hp:secPr>
              <hp:t>text</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let run = &sec.paragraphs[0].runs[0];
        let sec_pr = run.sec_pr.as_ref().unwrap();
        let page_pr = sec_pr.page_pr.as_ref().unwrap();
        assert_eq!(page_pr.width, 59528);
        assert_eq!(page_pr.height, 84188);
        assert_eq!(page_pr.landscape, "WIDELY");
        let margin = page_pr.margin.as_ref().unwrap();
        assert_eq!(margin.left, 8504);
        assert_eq!(margin.right, 8504);
        assert_eq!(margin.top, 5668);
        assert_eq!(margin.bottom, 4252);
        assert_eq!(margin.header, 4252);
        assert_eq!(margin.footer, 4252);
    }

    #[test]
    fn parse_table_basic() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:tbl rowCnt="2" colCnt="2">
                <hp:tr>
                  <hp:tc name="A1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:cellSz width="1000" height="500"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 1</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                  <hp:tc name="B1">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:cellSz width="1000" height="500"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 2</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
                <hp:tr>
                  <hp:tc name="A2">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 3</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                  <hp:tc name="B2">
                    <hp:cellSpan rowSpan="1" colSpan="1"/>
                    <hp:subList>
                      <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
                        <hp:run charPrIDRef="0">
                          <hp:t>Cell 4</hp:t>
                        </hp:run>
                      </hp:p>
                    </hp:subList>
                  </hp:tc>
                </hp:tr>
              </hp:tbl>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let tbl = &sec.paragraphs[0].runs[0].tables[0];
        assert_eq!(tbl.row_cnt, 2);
        assert_eq!(tbl.col_cnt, 2);
        assert_eq!(tbl.rows.len(), 2);
        assert_eq!(tbl.rows[0].cells.len(), 2);
        let cell0 = &tbl.rows[0].cells[0];
        assert_eq!(cell0.name, "A1");
        let text = &cell0.sub_list.as_ref().unwrap().paragraphs[0].runs[0].texts[0].text;
        assert_eq!(text, "Cell 1");
    }

    #[test]
    fn parse_picture() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:pic id="123">
                <hp:img binaryItemIDRef="image1.jpg"/>
                <hp:orgSz width="5000" height="3000"/>
              </hp:pic>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let pic = &sec.paragraphs[0].runs[0].pictures[0];
        let img = pic.img.as_ref().unwrap();
        assert_eq!(img.binary_item_id_ref, "image1.jpg");
        let org = pic.org_sz.as_ref().unwrap();
        assert_eq!(org.width, 5000);
        assert_eq!(org.height, 3000);
    }

    #[test]
    fn unknown_elements_in_run_are_skipped() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:ctrl>
                <hp:colPr id="" type="NEWSPAPER" layout="LEFT" colCount="1"/>
              </hp:ctrl>
              <hp:t>text after ctrl</hp:t>
            </hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        let run = &sec.paragraphs[0].runs[0];
        assert_eq!(run.texts[0].text, "text after ctrl");
    }

    #[test]
    fn linesegarray_is_ignored() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0">
              <hp:t>text</hp:t>
            </hp:run>
            <hp:linesegarray>
              <hp:lineseg textpos="0" vertpos="0" vertsize="1000"/>
            </hp:linesegarray>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "text");
    }

    #[test]
    fn multiple_paragraphs() {
        let xml = r#"
        <hs:sec>
          <hp:p id="0" paraPrIDRef="0" styleIDRef="0">
            <hp:run charPrIDRef="0"><hp:t>First</hp:t></hp:run>
          </hp:p>
          <hp:p id="1" paraPrIDRef="1" styleIDRef="0">
            <hp:run charPrIDRef="1"><hp:t>Second</hp:t></hp:run>
          </hp:p>
          <hp:p id="2" paraPrIDRef="2" styleIDRef="0">
            <hp:run charPrIDRef="0"><hp:t>Third</hp:t></hp:run>
          </hp:p>
        </hs:sec>"#;
        let sec = parse_section(xml);
        assert_eq!(sec.paragraphs.len(), 3);
        assert_eq!(sec.paragraphs[0].runs[0].texts[0].text, "First");
        assert_eq!(sec.paragraphs[1].runs[0].texts[0].text, "Second");
        assert_eq!(sec.paragraphs[2].runs[0].texts[0].text, "Third");
    }
}
