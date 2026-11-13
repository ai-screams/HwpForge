//! Re-encoded parts keep the caches of their own reading-order sections.
#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

use hwpforge_core::layout::{LayoutCache, LineSeg};
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::{
    Document, ImageStore, PageSettings, Paragraph, Run, RunContent, Section, Table, TableCell,
    TableRow,
};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::stamp::{HwpxStamper, StampAction, StampRequestV2, StampSpec};
use hwpforge_smithy_hwpx::{
    CellSpec, CellTarget, EncodeOptions, HwpxCellEditor, HwpxDecoder, HwpxEncoder,
};

fn fixture(order: &[usize], mask: usize) -> Vec<u8> {
    let mut doc = Document::new();
    for file in 0..order.len() {
        let para = |text: &str| {
            let mut p = Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            if mask & (1 << file) != 0 {
                let mut line = LineSeg::default();
                line.vertpos = 1000 + file as i32;
                line.horzsize = 40000;
                line.vertsize = 1000;
                line.textheight = 1000;
                line.baseline = 850;
                p.layout_cache = Some(LayoutCache::new(vec![line]));
            }
            p
        };
        let table = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![para("가나")],
            HwpUnit::new(8000).unwrap(),
        )])]);
        let mut host = para(if file == 0 { "성명: (   )" } else { "앞" });
        host.runs.push(Run::table(table, CharShapeIndex::new(0)));
        doc.add_section(Section::with_paragraphs(vec![host, para("뒤")], PageSettings::a4()));
    }
    let bytes = HwpxEncoder::encode_with_options(
        &doc.validate().unwrap(),
        &hwpforge_smithy_hwpx::style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
        EncodeOptions::default().with_emit_layout_cache(true),
    )
    .unwrap();
    let content = package::part(&bytes, "Contents/content.hpf");
    let (_, spine) = content.split_once("<opf:spine>").unwrap();
    let (spine, _) = spine.split_once("</opf:spine>").unwrap();
    let reordered = order
        .iter()
        .map(|i| format!("<opf:itemref idref=\"section{i}\" linear=\"yes\"/>"))
        .collect::<String>();
    package::splice_part(&bytes, "Contents/content.hpf", spine, &reordered)
}

fn check_matrix(preserving: bool, edit: impl Fn(&[u8], usize) -> Vec<u8>) {
    for order in [&[1, 0][..], &[2, 0, 1][..], &[1, 2, 0][..]] {
        for mask in 0..(1 << order.len()) {
            let input = fixture(order, mask);
            let edited_section = order.iter().position(|&file| file == 0).unwrap();
            let output = edit(&input, edited_section);
            for (section, &file) in order.iter().enumerate().filter(|(_, file)| **file != 0) {
                let tree = xml::Element::parse(&package::part(
                    &output,
                    &format!("Contents/section{}.xml", if preserving { file } else { section }),
                ));
                assert_eq!(tree.children[1].find("hp:t").text, "뒤");
                let lines = tree.descendants("hp:lineseg");
                let expected = if mask & (1 << file) != 0 { 3 } else { 0 };
                assert_eq!(
                    lines.len(),
                    expected,
                    "order={order:?}, mask={mask}, section={section}"
                );
                let expected_vertpos = ["1000", "1001", "1002"][file];
                for line in lines {
                    assert_eq!(line.attr("vertpos"), Some(expected_vertpos));
                }
            }
        }
    }
}

// 이것을 실패시키는 것: carry 원본을 출력 파일명으로 조회, source를 원본 경로로 조회.
// 미편집 구역의 본문은 서로 같으므로 텍스트 차이에 기대지 않고,
// 캐시 없는 구역은 0개, 있는 구역은 자기 vertpos를 단언한다.
#[test]
fn set_cell_keeps_caches_in_reading_order_without_cross_section_transfer() {
    check_matrix(true, |input, section| {
        let result = HwpxCellEditor::set_cells_with_diagnostics(
            input,
            &[CellSpec {
                table: section,
                target: CellTarget::At(GridCoord::new(0, 0)),
                text: "다".into(),
            }],
        )
        .unwrap();
        let decoded = HwpxDecoder::decode(&result.value.bytes).unwrap();
        let RunContent::Table(table) =
            &decoded.document.sections()[section].paragraphs[0].runs[1].content
        else {
            panic!("table")
        };
        assert_eq!(table.rows[0].cells[0].paragraphs[0].text_content(), "다");
        encoding::assert_cell_patch(input, &result.value.bytes, section, 0, "다");
        result.value.bytes
    });
}

fn specs(input: &[u8]) -> Vec<StampSpec> {
    let candidates = HwpxStamper::plan_bytes(input).unwrap();
    assert_eq!(candidates.len(), 1);
    candidates
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: "성명".into(), hint: Some("이름".into()) },
        })
        .collect()
}

// 이것을 실패시키는 것: carry 원본을 출력 파일명으로 조회하거나 legacy stamp가 원본 대신 캐시 없는 baseline을 carry에 전달.
#[test]
fn stamp_keeps_caches_in_reading_order_without_cross_section_transfer() {
    check_matrix(false, |input, _| {
        let result = HwpxStamper::stamp_with_diagnostics(input, &specs(input)).unwrap();
        assert_eq!(
            hwpforge_smithy_hwpx::HwpxFiller::list_fields(&result.value.bytes).unwrap().len(),
            1
        );
        result.value.bytes
    });
}

// 이것을 실패시키는 것: carry 원본을 출력 파일명으로 조회하거나 v2 stamp가 원본 대신 캐시 없는 baseline을 carry에 전달.
#[test]
fn stamp_v2_keeps_caches_in_reading_order_without_cross_section_transfer() {
    check_matrix(false, |input, _| {
        let plan = HwpxStamper::plan_bytes_v2(input).unwrap();
        let request = StampRequestV2 {
            schema_version: 2,
            source_sha256: plan.source_sha256,
            text: specs(input),
            cells: vec![],
        };
        let result = HwpxStamper::stamp_v2_with_diagnostics(input, &request).unwrap();
        assert_eq!(
            hwpforge_smithy_hwpx::HwpxFiller::list_fields(&result.value.bytes).unwrap().len(),
            1
        );
        result.value.bytes
    });
}
