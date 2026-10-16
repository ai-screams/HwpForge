//! Hancom's actual mixed title cells, with both source formats checked.
//! Fixtures are byte-for-byte copies of r4-mixedhdr-hancom.hwpx and the
//! source public document.
use hwpforge_core::{Document, Draft, Table};
use hwpforge_smithy_hwp5::Hwp5Decoder;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};
use std::io::{Cursor, Read};

fn fifteenth_table(doc: &Document<Draft>) -> &Table {
    doc.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| r.content.as_table())
        .nth(14)
        .expect("table 15")
}

// Independent wire oracle: (//hp:tbl)[15]/hp:tr[1]/hp:tc/@header.
fn wire_headers(bytes: &[u8]) -> Vec<u32> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
    let mut xml = String::new();
    zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
    let mut reader = quick_xml::Reader::from_str(&xml);
    let (mut tables, mut depth, mut target, mut first_row) = (0, 0_i32, None, false);
    let mut flags = Vec::new();
    loop {
        use quick_xml::events::Event;
        let event = reader.read_event().unwrap();
        let empty = matches!(event, Event::Empty(_));
        match event {
            Event::Start(e) | Event::Empty(e) => {
                if e.local_name().as_ref() == "tbl" {
                    tables += 1;
                    if tables == 15 {
                        target = Some(depth);
                    }
                }
                if target == Some(depth - 1) && e.local_name().as_ref() == "tr" {
                    first_row = true;
                }
                if first_row && target == Some(depth - 2) && e.local_name().as_ref() == "tc" {
                    let a = e
                        .attributes()
                        .map(Result::unwrap)
                        .find(|a| a.key.as_ref() == "header")
                        .unwrap();
                    flags.push(a.value.parse().unwrap());
                }
                if !empty {
                    depth += 1;
                }
            }
            Event::End(e) => {
                depth -= 1;
                if first_row && target == Some(depth - 1) && e.local_name().as_ref() == "tr" {
                    return flags;
                }
            }
            Event::Eof => panic!("table 15 first row missing"),
            _ => {}
        }
    }
}

fn assert_mixed(table: &Table) {
    assert_eq!(
        table.rows[0].cells.iter().map(|c| c.header).collect::<Vec<_>>(),
        [Some(false), Some(false), Some(true), Some(false)]
    );
    assert!(!table.rows[0].is_header);
}

/// 이것을 실패시키는 것: 혼합 행 거부 / 제목 값의 행 단위 복제 / HWP5 bit 18 오독.
#[test]
fn cell_header_hancom_mixedhdr_twins_roundtrip() {
    let hwpx = include_bytes!("../../../tests/fixtures/tables/r4-mixedhdr-hancom.hwpx");
    assert_eq!(wire_headers(hwpx), [0, 0, 1, 0]);
    let source = HwpxDecoder::decode(hwpx).expect("Hancom HWPX must open");
    assert_mixed(fifteenth_table(&source.document));
    let encoded = HwpxEncoder::encode(
        &source.document.validate().unwrap(),
        &source.style_store,
        &source.image_store,
    )
    .unwrap();
    assert_eq!(wire_headers(&encoded), [0, 0, 1, 0]);
    assert_mixed(fifteenth_table(&HwpxDecoder::decode(&encoded).unwrap().document));

    let hwp = include_bytes!("../../../tests/fixtures/tables/mixedhdr-original.hwp");
    let source = Hwp5Decoder::decode(hwp).unwrap();
    assert_mixed(fifteenth_table(&source.document));
    assert!(!source.warnings.iter().any(|w| format!("{w:?}").contains("table.header_row")));
    let (encoded, _) = hwpforge_convert::hwp5_to_hwpx_bytes(hwp).unwrap();
    assert_eq!(wire_headers(&encoded), [0, 0, 1, 0]);
    assert_mixed(fifteenth_table(&HwpxDecoder::decode(&encoded).unwrap().document));
}
