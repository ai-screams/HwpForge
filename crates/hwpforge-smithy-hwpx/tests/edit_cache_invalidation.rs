//! Cell cache invalidation: a set-cell edit drops the line caches it makes
//! stale — the edited cell paragraph, the table's host paragraph and the
//! body paragraph after the host — and keeps the rest.

#[path = "support/document.rs"]
mod document;
use document::text_para as para;
#[path = "support/package.rs"]
mod package;
use package::section_xml_unchecked as section_xml;
use std::io::{Cursor, Read, Write};

use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{ImageStore, PageSettings};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    style_store_for_preset, CellSpec, CellTarget, HwpxCellEditor, HwpxEncoder,
};

fn with_section_xml(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().expect("ZIP entry name").to_string();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).unwrap();
        if name == "Contents/section0.xml" {
            data = xml.as_bytes().to_vec();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        out.write_all(&data).unwrap();
    }
    out.finish().unwrap().into_inner()
}

/// A cache marked `9000 + k` before the `k`-th `</hp:p>` (close order).
fn add_caches(xml: &str) -> String {
    let mut out = String::new();
    for (k, piece) in xml.split("</hp:p>").enumerate() {
        if k > 0 {
            out.push_str("</hp:p>");
        }
        out.push_str(piece);
        if xml.matches("</hp:p>").count() > k {
            out.push_str(&format!(
                "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" \
                 textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                 horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray>",
                9000 + k
            ));
        }
    }
    out
}

fn caches(xml: &str) -> Vec<usize> {
    xml.match_indices("vertpos=\"9")
        .map(|(at, _)| xml[at + 9..at + 13].parse::<usize>().unwrap() - 9000)
        .collect()
}

/// 이것을 실패시키는 것: `layout_carry` 의 흐름 꼬리 삭제(뒤 문단 9004 가 원래
/// 캐시를 다시 받는다).
#[test]
fn set_cell_drops_the_flow_tail_and_keeps_the_rest() {
    let unit = HwpUnit::new(4000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![para("a")], unit),
        TableCell::new(vec![para("b")], unit),
    ])]);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            para("앞"),
            Paragraph::with_runs(
                vec![Run::table(table, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
            para("뒤"),
        ],
        PageSettings::a4(),
    ));
    let store = style_store_for_preset("default").unwrap();
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    // close order: 앞 9000, a 9001, b 9002, host 9003, 뒤 9004.
    let bytes = with_section_xml(&bytes, &add_caches(&section_xml(&bytes)));
    assert_eq!(caches(&section_xml(&bytes)), [0, 1, 2, 3, 4]);
    let spec = CellSpec {
        table: 0,
        target: CellTarget::At(GridCoord::new(0, 0)),
        text: "여러 줄로 길어진 값 여러 줄로 길어진 값 여러 줄로 길어진 값".to_string(),
    };
    let edited = HwpxCellEditor::set_cells(&bytes, &[spec]).expect("set_cells");
    // The other cell (its own flow) and the paragraph before keep theirs.
    assert_eq!(caches(&section_xml(&edited.bytes)), [0, 2]);
}

fn table(text: &str) -> Table {
    let unit = HwpUnit::new(4000).unwrap();
    Table::new(vec![TableRow::new(vec![TableCell::new(vec![para(text)], unit)])])
}

/// A first paragraph holding the section controls, a table and twenty
/// letters, then a paragraph with a second table; the first paragraph's
/// cache is replaced by `lines` (its `textpos` values) and, when
/// `omit_col_pr`, its `colPr` control is removed, as an input may omit it.
fn first_paragraph_cached(lines: &[u32], omit_col_pr: bool) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            Paragraph::with_runs(
                vec![
                    Run::table(table("t0"), CharShapeIndex::new(0)),
                    Run::text("ABCDEFGHIJKLMNOPQRST", CharShapeIndex::new(0)),
                ],
                ParaShapeIndex::new(0),
            ),
            Paragraph::with_runs(
                vec![Run::table(table("t1"), CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            ),
        ],
        PageSettings::a4(),
    ));
    let store = style_store_for_preset("default").unwrap();
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
    let xml = section_xml(&bytes);
    let xml = if omit_col_pr {
        let col_start = xml.find("<hp:ctrl><hp:colPr").expect("colPr control");
        let col_end = col_start + xml[col_start..].find("</hp:ctrl>").unwrap() + "</hp:ctrl>".len();
        format!("{}{}", &xml[..col_start], &xml[col_end..])
    } else {
        xml
    };
    // The first paragraph is the last to close at body level before the
    // second: put its cache before the `</hp:p>` that follows the text.
    let text_at = xml.find("ABCDEFGHIJKLMNOPQRST").unwrap();
    let close = text_at + xml[text_at..].find("</hp:p>").unwrap();
    let segs: String = lines
        .iter()
        .enumerate()
        .map(|(i, pos)| {
            format!(
                "<hp:lineseg textpos=\"{pos}\" vertpos=\"{}\" vertsize=\"1000\" \
                 textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                 horzsize=\"42520\" flags=\"393216\"/>",
                i * 1600
            )
        })
        .collect();
    let xml =
        format!("{}<hp:linesegarray>{segs}</hp:linesegarray>{}", &xml[..close], &xml[close..]);
    with_section_xml(&bytes, &xml)
}

fn first_cache(bytes: &[u8]) -> Option<hwpforge_core::layout::LayoutCache> {
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(bytes).unwrap();
    decoded.document.sections()[0].paragraphs[0].layout_cache.clone()
}

fn edit_second_table(bytes: &[u8]) -> Vec<u8> {
    let spec = CellSpec {
        table: 1,
        target: CellTarget::At(GridCoord::new(0, 0)),
        text: "바뀐 칸".to_string(),
    };
    HwpxCellEditor::set_cells(bytes, &[spec]).expect("set_cells").bytes
}

/// The input omits `colPr`, so its line start 26 is Core
/// position 10 (section controls and the table before the text); the
/// re-encoder writes `colPr`, and carrying the original cache XML moved that
/// line to 2 with no warning. The carried cache now re-decodes to the
/// input's own Core cache.
/// 이것을 실패시키는 것: 캐시를 원본 XML 에서 가져오기(재디코드가 [0,2]).
#[test]
fn an_untouched_paragraph_keeps_its_line_starts_when_the_encoder_adds_a_token() {
    let bytes = first_paragraph_cached(&[0, 26], true);
    let before = first_cache(&bytes).expect("the input cache decodes");
    let starts: Vec<u32> = before.lines.iter().map(|s| s.textpos).collect();
    assert_eq!(starts, [0, 10]);
    let edited = edit_second_table(&bytes);
    assert_eq!(first_cache(&edited), Some(before));
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&edited).unwrap();
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
}

/// A cache the decoder dropped (a line starting at 20, inside the table's
/// span after the section and column controls) is not in Core, so the edit
/// does not bring it back from the input.
/// 이것을 실패시키는 것: 캐시를 원본 XML 에서 가져오기(버린 캐시가 되살아난다).
#[test]
fn a_cache_the_decoder_dropped_stays_dropped() {
    let bytes = first_paragraph_cached(&[0, 20], false);
    assert_eq!(first_cache(&bytes), None);
    let edited = edit_second_table(&bytes);
    let xml = section_xml(&edited);
    assert!(!xml[..xml.find("바뀐 칸").unwrap()].contains("<hp:linesegarray"));
    assert_eq!(first_cache(&edited), None);
}

/// A second line starting at wire 16 — after the
/// section definition and the table, before the text — is Core position 0
/// with the boundary "after a section definition and a table". The
/// re-encoder writes a column definition between those two; the line still
/// starts after the table, so the untouched paragraph keeps the input's
/// cache exactly.
/// 이것을 실패시키는 것: 경계가 단 정의를 건너뛰지 않기(캐시가 무경고로 사라진다).
#[test]
fn an_untouched_paragraph_keeps_a_line_start_past_a_column_definition() {
    let bytes = first_paragraph_cached(&[0, 16], true);
    let before = first_cache(&bytes).expect("the input cache decodes");
    assert!(before.lines[1].boundary.is_some(), "{before:?}");
    let edited = edit_second_table(&bytes);
    // The re-decoded boundary also names the column definition the output
    // now holds; otherwise the cache is the input's.
    let without_col = |mut cache: hwpforge_core::layout::LayoutCache| {
        for line in &mut cache.lines {
            if let Some(b) = line.boundary.as_mut() {
                b.skip.retain(|k| *k != hwpforge_core::layout::WireTokenKind::ColumnDef);
            }
        }
        cache
    };
    let after = first_cache(&edited).expect("the untouched paragraph keeps a cache");
    assert_eq!(without_col(after), before);
    let decoded = hwpforge_smithy_hwpx::HwpxDecoder::decode(&edited).unwrap();
    assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
}
