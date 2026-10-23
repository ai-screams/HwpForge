//! Mixed title cells in purpose-made Hancom twins.
use hwpforge_core::{Document, Draft, Table};
use hwpforge_smithy_hwp5::Hwp5Decoder;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

fn first_table(doc: &Document<Draft>) -> &Table {
    doc.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_table())
        .next()
        .expect("table 1")
}

mod wire {
    include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/support/hancom.rs"));
}
use wire::wire_headers;

fn assert_mixed(table: &Table, expected: &[u32]) {
    assert_eq!(
        table.rows[0].cells.iter().map(|c| c.header).collect::<Vec<_>>(),
        expected.iter().map(|&v| Some(v != 0)).collect::<Vec<_>>()
    );
    assert!(!table.rows[0].is_header);
}

/// 이것을 실패시키는 것: 혼합 행 거부 / 제목 값의 행 단위 복제 / HWP5 bit 18 오독.
#[test]
fn cell_header_hancom_twins_roundtrip() {
    let hwpx = include_bytes!("../../../tests/fixtures/tables/f1-cell-headers.hwpx");
    let expected = wire_headers(hwpx);
    // f1 Hancom hp:tr[1]/hp:tc@header: mixed individual cells.
    assert_eq!(expected, [0, 0, 1, 0]);
    let source = HwpxDecoder::decode(hwpx).expect("Hancom HWPX must open");
    assert_mixed(first_table(&source.document), &expected);
    let encoded = HwpxEncoder::encode(
        &source.document.validate().unwrap(),
        &source.style_store,
        &source.image_store,
    )
    .unwrap();
    assert_eq!(wire_headers(&encoded), expected);
    assert_mixed(first_table(&HwpxDecoder::decode(&encoded).unwrap().document), &expected);

    let hwp = include_bytes!("../../../tests/fixtures/tables/f1-cell-headers.hwp");
    let source = Hwp5Decoder::decode(hwp).unwrap();
    assert_mixed(first_table(&source.document), &expected);
    assert!(!source.warnings.iter().any(|w| format!("{w:?}").contains("table.header_row")));
    let (encoded, _) = hwpforge_convert::hwp5_to_hwpx_bytes(hwp).unwrap();
    assert_eq!(wire_headers(&encoded), expected);
    assert_mixed(first_table(&HwpxDecoder::decode(&encoded).unwrap().document), &expected);
}
