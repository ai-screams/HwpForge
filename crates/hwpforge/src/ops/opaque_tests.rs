//! Ops-level save contract; synthetic HWPX keeps the tests independent of corpus paths.
use super::*;
use hwpforge_core::image::ImageStore;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::zip_package::open_package;
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::stamp::{StampMap, StampRequestV2, STAMP_MAP_VERSION};
use hwpforge_smithy_hwpx::{style_store_for_preset, HwpxDecoder, HwpxEncoder};
use serde_json::{json, Value};
use std::cell::Cell;
use std::io::{Cursor, Write};

// Only observes wiring in test builds. Preserving writers already produce safe
// bytes, so removing their final gate has no observable payload difference.
thread_local! {
    pub(super) static GATE_CALLS: Cell<usize> = const { Cell::new(0) };
}

fn paragraph(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn base() -> Vec<u8> {
    let field = Paragraph::with_runs(
        vec![Run::control(
            Control::Field {
                field_type: FieldType::ClickHere,
                hint_text: Some("hint".into()),
                help_text: None,
                name: Some("field".into()),
                display_text: String::new(),
            },
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    let table = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![paragraph("cell")],
        HwpUnit::new(8000).unwrap(),
    )])]);
    let host = Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![paragraph("alpha"), paragraph("delete me"), field, host],
        PageSettings::a4(),
    ));
    HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap()
}

fn rewrite(base: &[u8], entries: &[(&str, &[u8])], remove: &[&str]) -> Vec<u8> {
    let mut input = open_package(base, Default::default()).unwrap();
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for index in 0..input.entries().len() {
        let name = input.entries()[index].path.clone();
        if remove.contains(&name.as_str()) || entries.iter().any(|(n, _)| *n == name) {
            continue;
        }
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(&input.read(index).unwrap()).unwrap();
    }
    for (name, payload) in entries {
        zip.start_file(*name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(payload).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

fn shapes() -> [Vec<(&'static str, &'static [u8])>; 4] {
    [
        vec![("Unused/./a.bin", b"x")],
        vec![("Unused", b"x"), ("Unused/", b"")],
        vec![("Unused/", b"x")],
        vec![("Unused/./", b"")],
    ]
}

fn assert_carried(input: &[u8], output: &[u8]) {
    let mut before = open_package(input, Default::default()).unwrap();
    let mut after = open_package(output, Default::default()).unwrap();
    for i in 0..before.entries().len() {
        if before.is_opaque(i) {
            let name = &before.entries()[i].path;
            let j = after.entries().iter().position(|e| e.path == *name).expect("same name");
            assert_eq!(before.is_opaque(i), after.is_opaque(j), "{name}");
            assert_eq!(before.read(i).unwrap(), after.read(j).unwrap());
        }
    }
    HwpxDecoder::decode(output).unwrap();
}

fn legacy(value: &mut Value) {
    value.as_object_mut().unwrap().remove("format_version");
    for para in value["styles"]["para_shapes"].as_array_mut().unwrap() {
        for key in ["margin_left", "margin_right", "indent", "spacing_before", "spacing_after"] {
            let measure = &para[key];
            para[key] = json!(
                measure["value"].as_i64().unwrap() * 2 + i64::from(measure["unit"] == "char")
            );
        }
    }
}

fn patch_bytes(input: &[u8], old: bool) -> Result<Vec<u8>, OpsError> {
    let mut value = export_section(input, &Default::default()).unwrap().section;
    value["section"]["paragraphs"][0]["runs"][0]["content"]["Text"] = json!("edited");
    if old {
        legacy(&mut value);
    }
    patch(input, &PatchOptions::default().with_patch(value.to_string())).map(|out| out.bytes)
}

fn json_bytes(input: &[u8], old: bool) -> Result<Vec<u8>, OpsError> {
    let mut value = to_json(input, &Default::default()).unwrap().document;
    value["document"]["sections"][0]["paragraphs"][0]["runs"][0]["content"]["Text"] =
        json!("edited");
    if old {
        legacy(&mut value);
    }
    from_json(&value.to_string(), &FromJsonOptions::default().with_base(input)).map(|out| out.bytes)
}

fn check_regenerating(op: impl Fn(&[u8]) -> Result<Vec<u8>, OpsError>) {
    let normal = base();
    op(&normal).unwrap();
    for shape in shapes() {
        let input = rewrite(&normal, &shape, &[]);
        let error = op(&input).expect_err("opaque loss must refuse output");
        assert_eq!(error.code(), OpsCode::EncodeFailed);
        assert!(
            matches!(&error, OpsError::Encode(HwpxError::InvalidStructure { detail }) if detail.contains("opaque entry")),
            "{error}"
        );
    }
}

// 이것을 실패시키는 것: patch 최종 게이트 호출 제거 (구형 일반 재생성 우회).
#[test]
fn w2_gate_legacy_patch() {
    check_regenerating(|b| patch_bytes(b, true));
}

// 이것을 실패시키는 것: from_json+base 최종 게이트 호출 제거 (구형 분기).
#[test]
fn w2_gate_legacy_from_json() {
    check_regenerating(|b| json_bytes(b, true));
}

// 이것을 실패시키는 것: restyle 최종 게이트 호출 제거.
#[test]
fn w2_gate_restyle() {
    check_regenerating(|b| {
        restyle(b, &RestyleOptions::default().with_preset("default")).map(|o| o.bytes)
    });
}

fn check_preserving(op: impl Fn(&[u8]) -> Result<Vec<u8>, OpsError>) {
    let normal = base();
    for shape in shapes() {
        let input = rewrite(&normal, &shape, &[]);
        GATE_CALLS.set(0);
        let output = op(&input).unwrap();
        assert_eq!(GATE_CALLS.get(), 1, "final gate called exactly once");
        assert_carried(&input, &output);
    }
}

// 이것을 실패시키는 것: 보존 patch 최종 게이트 호출 제거; opaque 이름·payload·판정 손실.
// 게이트가 정상 항목까지 바이트 비교 대상으로 삼아도 section 편집이 거부되어 실패한다.
#[test]
fn w2_gate_preserving_patch() {
    check_preserving(|b| patch_bytes(b, false));
}

// 이것을 실패시키는 것: from_json+base 신형 편집 encoder 분기의 최종 게이트 호출 제거.
#[test]
fn w2_gate_edit_encoder() {
    check_preserving(|b| json_bytes(b, false));
}

// 이것을 실패시키는 것: fill 최종 게이트 호출 제거; 보존 출력의 opaque 손실.
#[test]
fn w2_gate_fill() {
    check_preserving(|b| {
        fill(b, &[("field".into(), "filled".into())], &Default::default()).map(|o| o.bytes)
    });
}

// 이것을 실패시키는 것: insert_para 최종 게이트 호출 제거.
#[test]
fn w2_gate_insert_para() {
    check_admission(|b| {
        insert_para(b, &InsertParaOptions::default().with_text(vec!["inserted".into()]))
            .map(|o| o.bytes)
    });
}

// 이것을 실패시키는 것: delete_para 최종 게이트 호출 제거.
#[test]
fn w2_gate_delete_para() {
    check_admission(|b| {
        delete_para(b, &DeleteParaOptions::default().with_indexes(vec![1])).map(|o| o.bytes)
    });
}

fn check_admission(op: impl Fn(&[u8]) -> Result<Vec<u8>, OpsError>) {
    let normal = base();
    GATE_CALLS.set(0);
    let output = op(&normal).unwrap();
    assert_eq!(GATE_CALLS.get(), 1, "successful regeneration must reach final gate");
    assert_carried(&normal, &output);
    for shape in shapes() {
        let input = rewrite(&normal, &shape, &[]);
        // Existing admission rejects these uncarried inputs before regeneration.
        // Do not turn an early unrelated error into a passing preservation test.
        let error = op(&input).unwrap_err();
        assert_eq!(error.code(), OpsCode::InputEntriesNotCarried, "{error}");
    }
}

// 이것을 실패시키는 것: stamp 최종 게이트 호출 제거 (v1·v2 합류 뒤).
#[test]
fn w2_gate_stamp() {
    check_admission(|b| stamp(b, &StampMap::Legacy(vec![]), &Default::default()).map(|o| o.bytes));
    check_admission(|b| {
        let request = StampMap::V2(StampRequestV2 {
            schema_version: STAMP_MAP_VERSION,
            source_sha256: stamp_plan(b).unwrap().plan.source_sha256,
            text: vec![],
            cells: vec![],
        });
        stamp(b, &request, &Default::default()).map(|o| o.bytes)
    });
}

// 이것을 실패시키는 것: set_cell 최종 게이트 호출 제거.
#[test]
fn w2_gate_cell_edit() {
    check_admission(|b| {
        set_cell(
            b,
            &SetCellOptions::default().with_table(0).with_at("0,0").with_text("edited cell"),
        )
        .map(|o| o.bytes)
    });
}

// 이것을 실패시키는 것: 게이트의 바이트 비교 생략 (이름·판정은 같은 nonempty 폴더).
#[test]
fn w2_gate_changed_payload() {
    let input = rewrite(&base(), &[("Unused/", b"x")], &[]);
    let output = rewrite(&input, &[("Unused/", b"y")], &[]);
    ensure_opaque_carried(&input, &input).unwrap();
    let error = ensure_opaque_carried(&input, &output).unwrap_err();
    assert!(error.to_string().contains("payload changed"), "{error}");
}

// 이것을 실패시키는 것: 재개방 is_opaque 비교 생략. 실제 payload는 같지만
// 입력은 빈 폴더의 비압축 크기를 1로 광고한다. 출력이 실제 크기 0으로 쓰면
// metadata 판정만 정상으로 바뀐다. ZIP reader는 상한 초과는 막지만 짧은 payload는 허용한다.
#[test]
fn w2_gate_changed_classification() {
    let output = rewrite(&base(), &[("Unused/", b"")], &[]);
    let mut input = output.clone();
    // This is the last appended entry; alter only its advertised size in both
    // headers, retaining the compressed stream and its CRC. Never parse ZIP
    // metadata this way in production: the gate uses core's classification.
    let central = input.windows(4).rposition(|w| w == b"PK\x01\x02").unwrap();
    let local = u32::from_le_bytes(input[central + 42..central + 46].try_into().unwrap()) as usize;
    input[central + 24..central + 28].copy_from_slice(&1u32.to_le_bytes());
    input[local + 22..local + 26].copy_from_slice(&1u32.to_le_bytes());
    let mut before = open_package(&input, Default::default()).unwrap();
    let mut after = open_package(&output, Default::default()).unwrap();
    assert_eq!(before.entries().len(), after.entries().len());
    for i in 0..before.entries().len() {
        assert_eq!(before.entries()[i].path, after.entries()[i].path);
        assert_eq!(before.read(i).unwrap(), after.read(i).unwrap());
    }
    let i = before.entries().iter().position(|e| e.path == "Unused/").unwrap();
    assert!(before.is_opaque(i));
    assert!(!after.is_opaque(i));
    let error = ensure_opaque_carried(&input, &output).unwrap_err();
    assert!(error.to_string().contains("opaque entry 'Unused/'"), "{error}");
}

// 이것을 실패시키는 것: 이름 검사 생략 또는 정상 입력에서도 출력 재개방하기.
#[test]
fn w2_gate_missing_and_no_opaque_fast_path() {
    let normal = base();
    ensure_opaque_carried(&normal, b"not a ZIP").unwrap();
    for shape in shapes() {
        let input = rewrite(&normal, &shape, &[]);
        assert!(ensure_opaque_carried(&input, &normal).is_err());
        assert!(ensure_opaque_carried(&input, b"not a ZIP").is_err());
        ensure_opaque_carried(&input, &input).unwrap();
    }
}
