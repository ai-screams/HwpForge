//! Cell title markers across serialization, editing and readable projections.
#![cfg(feature = "ops-hwpx")]

use hwpforge::core::image::ImageStore;
use hwpforge::core::table::grid::GridCoord;
use hwpforge::core::{
    Document, Draft, PageSettings, Paragraph, Run, Section, Table, TableCell, TableRow,
};
use hwpforge::foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge::hwpx::stamp::{
    CellStampAction, CellStampSpec, StampMap, StampRequestV2, STAMP_MAP_VERSION,
};
use hwpforge::hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge::hwpx::{HwpxDecoder, HwpxEncoder};
use hwpforge::ops::edit::{set_cell, SetCellOptions};
use hwpforge::ops::stamp::{stamp, stamp_plan, StampOptions};

fn document(markers: &[Vec<Option<bool>>], flags: &[bool]) -> Document<Draft> {
    let rows = markers
        .iter()
        .zip(flags)
        .map(|(markers, flag)| {
            TableRow::new(
                markers
                    .iter()
                    .enumerate()
                    .map(|(i, marker)| {
                        let text = if i == 0 { "성명" } else { "" };
                        let para = Paragraph::with_runs(
                            vec![Run::text(text, CharShapeIndex::new(0))],
                            ParaShapeIndex::new(0),
                        );
                        TableCell::new(vec![para], HwpUnit::new(8000).unwrap()).with_header(*marker)
                    })
                    .collect(),
            )
            .with_header(*flag)
        })
        .collect();
    let table = Table::new(rows).with_repeat_header(true);
    let host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    doc
}

fn encode(doc: Document<Draft>) -> Vec<u8> {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap()
}

fn markers(bytes: &[u8]) -> Vec<Vec<Option<bool>>> {
    let decoded = HwpxDecoder::decode(bytes).unwrap();
    let table = decoded.document.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap();
    table.rows.iter().map(|r| r.cells.iter().map(|c| c.header).collect()).collect()
}

/// 이것을 실패시키는 것: encoder의 칸 우선 제거 / decoder 칸 값 누락 /
/// 혼합·비선두 거부 복원 / None의 행 fallback 제거 / all→any 파생.
#[test]
fn cell_header_codec_matrix() {
    let inputs = vec![
        vec![None, None],
        vec![Some(true), Some(true)],
        vec![Some(false), Some(false)],
        vec![Some(false), Some(true)],
        vec![Some(true), Some(true)],
    ];
    let bytes = encode(document(&inputs, &[true, true, false, false, false]));
    let expected = vec![
        vec![Some(true), Some(true)],
        vec![Some(true), Some(true)],
        vec![Some(false), Some(false)],
        vec![Some(false), Some(true)],
        vec![Some(true), Some(true)],
    ];
    assert_eq!(markers(&bytes), expected);
    let decoded = HwpxDecoder::decode(&bytes).unwrap();
    let table = decoded.document.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap();
    assert_eq!(
        table.rows.iter().map(|r| r.is_header).collect::<Vec<_>>(),
        [true, true, false, false, false]
    );
    let encoded = HwpxEncoder::encode(
        &decoded.document.validate().unwrap(),
        &decoded.style_store,
        &decoded.image_store,
    )
    .unwrap();
    assert_eq!(markers(&encoded), expected);
    // Explicit false overrides even a manually supplied true row flag.
    assert_eq!(
        markers(&encode(document(&[vec![Some(false), Some(true)]], &[true]))),
        vec![vec![Some(false), Some(true)]]
    );
    assert_eq!(
        markers(&encode(document(&[vec![None, None]], &[false]))),
        vec![vec![Some(false), Some(false)]]
    );
}

/// 이것을 실패시키는 것: set-cell 또는 stamp가 편집한/이웃 칸의 header를 초기화.
#[test]
fn cell_header_survives_set_cell_and_cell_stamp() {
    for input in [vec![Some(false), Some(true)], vec![Some(true), Some(false)]] {
        let expected = vec![input];
        let bytes = encode(document(&expected, &[false]));
        let edited = set_cell(
            &bytes,
            &SetCellOptions::default().with_table(0).with_at("0,1").with_text("홍길동"),
        )
        .unwrap();
        assert_eq!(markers(&edited.bytes), expected);
        let edited_doc = HwpxDecoder::decode(&edited.bytes).unwrap();
        let edited_table =
            edited_doc.document.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(edited_table.rows[0].cells[1].paragraphs[0].text_content(), "홍길동");
        let plan = stamp_plan(&bytes).unwrap().plan;
        let request = StampMap::V2(StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: plan.source_sha256,
            text: vec![],
            cells: vec![CellStampSpec {
                table: 0,
                at: GridCoord::new(0, 1),
                label: None,
                action: CellStampAction::Field {
                    name: "성명".into(), hint: "성명 입력".into()
                },
            }],
        });
        let stamped = stamp(&bytes, &request, &StampOptions::default()).unwrap();
        assert_eq!(markers(&stamped.bytes), expected);
        assert_eq!(stamped.stamped_cells.len(), 1);
        assert_eq!(
            hwpforge::hwpx::HwpxFiller::list_fields(&stamped.bytes).unwrap()[0].name.as_deref(),
            Some("성명")
        );
    }
}

/// 이것을 실패시키는 것: MD가 제목 칸으로 행을 재정렬하거나 일부 칸을 숨기기.
#[cfg(feature = "md")]
#[test]
fn cell_header_does_not_change_readable_markdown() {
    use hwpforge::hwpx::HwpxStyleLookup;
    use hwpforge::md::MdEncoder;
    let output = |markers: &[Vec<Option<bool>>], flags: &[bool]| {
        let mut source = document(markers, flags);
        let hwpforge::core::run::RunContent::Table(table) =
            &mut source.sections_mut()[0].paragraphs[0].runs[0].content
        else {
            panic!("table")
        };
        // A nonempty later cell detects truncation even when GFM pads the
        // resulting row back to the original header width.
        table.rows[1].cells[1].paragraphs[0].runs[0] =
            Run::text("보존할 본문", CharShapeIndex::new(0));
        let bytes = encode(source);
        let d = HwpxDecoder::decode(&bytes).unwrap();
        let lookup = HwpxStyleLookup::new(&d.style_store, &d.image_store);
        let doc = d.document.validate().unwrap();
        (MdEncoder::encode_lossy(&doc).unwrap(), MdEncoder::encode_styled(&doc, &lookup).markdown)
    };
    let plain = output(&[vec![None, None], vec![None, None]], &[false, false]);
    for first in [
        vec![Some(false), Some(true)],
        vec![Some(true), Some(true)],
        vec![Some(false), Some(false)],
    ] {
        assert_eq!(output(&[first, vec![Some(true), Some(true)]], &[false, false]), plain);
    }
}

/// 이것을 실패시키는 것: lossless MD의 칸/행 속성 방출·읽기 누락 /
/// Some(false)와 None 혼동 / 행 flag를 칸 값으로 덮어쓰기.
#[cfg(feature = "md")]
#[test]
fn cell_header_lossless_markdown_preserves_explicit_and_inherited() {
    use hwpforge::md::{MdDecoder, MdEncoder};
    for (markers, flag, false_count, true_count, row_count) in [
        (vec![None, Some(false), Some(true)], false, 1, 1, 0),
        (vec![None, Some(false), Some(true)], true, 1, 2, 0),
        (vec![None, Some(true)], true, 0, 1, 1),
        (vec![Some(true), Some(true)], false, 0, 2, 1),
    ] {
        // 이것을 실패시키는 것: 정규화된 행/칸 대신 오래된 행 flag 직렬화,
        // 바뀌지 않은 행의 None까지 구체화, 구체화한 true 손실.
        let d = document(&[markers], &[flag]).validate().unwrap();
        let original =
            d.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap().rows.clone();
        let md = MdEncoder::encode_lossless(&d).unwrap();
        assert_eq!(md.matches("data-header=\"false\"").count(), false_count, "{md}");
        assert_eq!(md.matches("data-header=\"true\"").count(), true_count, "{md}");
        assert_eq!(md.matches("data-is-header=\"true\"").count(), row_count, "{md}");
        let decoded = MdDecoder::decode_lossless(&md).unwrap();
        let table = decoded.sections()[0].paragraphs[0].runs[0].content.as_table().unwrap();
        assert_eq!(table.rows, original);
    }
}

/// 이것을 실패시키는 것: lossless 제목 속성의 잘못된 불리언 허용 / 중복 허용.
#[cfg(feature = "md")]
#[test]
fn cell_header_lossless_markdown_rejects_invalid_markers() {
    use hwpforge::md::{MdDecoder, MdEncoder};
    let md =
        MdEncoder::encode_lossless(&document(&[vec![Some(true)]], &[true]).validate().unwrap())
            .unwrap();
    for (element, attr) in [("td", "data-header"), ("tr", "data-is-header")] {
        let valid = format!("{attr}=\"true\"");
        for bad in [format!("{attr}=\"1\""), format!("{attr}=\"wrong\"")] {
            let err = MdDecoder::decode_lossless(&md.replace(&valid, &bad)).unwrap_err();
            assert!(
                matches!(err, hwpforge::md::MdError::LosslessInvalidAttribute { attribute, .. } if attribute == attr),
                "{bad}"
            );
        }
        let duplicate = md.replace(&valid, &format!("{valid} {valid}"));
        let err = MdDecoder::decode_lossless(&duplicate).unwrap_err();
        assert!(
            matches!(err, hwpforge::md::MdError::LosslessParse { detail } if detail == format!("<{element}> repeats {attr}"))
        );
    }
}
