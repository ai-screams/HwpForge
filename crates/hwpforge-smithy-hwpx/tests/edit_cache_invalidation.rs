//! W2 (design 3.5 test 2): a set-cell edit drops the line caches it makes
//! stale — the edited cell paragraph, the table's host paragraph and the
//! body paragraph after the host — and keeps the rest.

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

fn para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    xml
}

fn with_section_xml(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).unwrap();
        let name = entry.name().to_string();
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
