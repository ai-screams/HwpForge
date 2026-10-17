//! Regression tests for Core indices versus original package part names.
use std::collections::BTreeMap;

use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::{Document, PageSettings, Paragraph, Section};
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};

use crate::patch::RawPackage;
use crate::*;

const FIRST: &str = "Contents/section1.xml";
const SECOND: &str = "Contents/section0.xml";
const CACHE: &str = r#"<hp:linesegarray><hp:lineseg textpos="0" vertpos="9000" vertsize="1000" textheight="1000" baseline="850" spacing="600" horzpos="0" horzsize="42520" flags="393216"/></hp:linesegarray>"#;

fn para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn reversed(paragraphs: Vec<Paragraph>) -> Vec<u8> {
    let mut doc = Document::new();
    for _ in 0..2 {
        doc.add_section(Section::with_paragraphs(paragraphs.clone(), PageSettings::a4()));
    }
    let bytes = HwpxEncoder::encode(
        &doc.validate().unwrap(),
        &style_store_for_preset("default").unwrap(),
        &ImageStore::new(),
    )
    .unwrap();
    let mut pkg = RawPackage::read(&bytes).unwrap();
    let xml = pkg.read_text_entry(SECOND).unwrap().replace("</hp:p>", &format!("{CACHE}</hp:p>"));
    // Identical wire bodies hide wrong-file edits from text/hash/slot comparisons.
    pkg.replace_text_entry(SECOND, xml.clone());
    pkg.replace_text_entry(FIRST, xml);
    let hpf = pkg.read_text_entry("Contents/content.hpf").unwrap();
    let (before, spine) = hpf.split_once("<opf:spine>").unwrap();
    pkg.replace_text_entry(
        "Contents/content.hpf",
        format!(
            "{before}<opf:spine>{}",
            spine
                .replace("section0", "SWAP")
                .replace("section1", "section0")
                .replace("SWAP", "section1")
        ),
    );
    let bytes = pkg.write().unwrap();
    let pkg = RawPackage::read(&bytes).unwrap();
    assert_eq!(pkg.order.sections, [FIRST, SECOND]);
    assert!(pkg.order.problems.is_empty(), "{:?}", pkg.order.problems);
    bytes
}

fn texts(bytes: &[u8]) -> Vec<Vec<String>> {
    HwpxDecoder::decode(bytes)
        .unwrap()
        .document
        .sections()
        .iter()
        .map(|s| s.paragraphs.iter().map(Paragraph::text_content).collect())
        .collect()
}

fn only_first_part_changed(base: &[u8], output: &[u8]) {
    let before = RawPackage::read(base).unwrap();
    let after = RawPackage::read(output).unwrap();
    assert_eq!(before.entry_paths().collect::<Vec<_>>(), after.entry_paths().collect::<Vec<_>>());
    for path in before.entry_paths() {
        if path == FIRST {
            assert_ne!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap());
        } else {
            assert_eq!(before.read_entry(path).unwrap(), after.read_entry(path).unwrap(), "{path}");
        }
    }
}

// 이것을 실패시키는 것: export 또는 patch의 읽기/쓰기 경로를 구역 번호로 만들기.
#[test]
fn reversed_spine_export_patch_changes_only_section_zero() {
    let base = reversed(vec![para("old"), para("keep")]);
    let export = HwpxPatcher::export_section_for_edit(&base, 0, true).unwrap();
    assert!(export.warning.is_none());
    let mut exported = export.exported;
    assert_eq!(exported.preservation.as_ref().unwrap().section_path, FIRST);
    // Exercise the public JSON payload as well as the section workflow.
    exported = serde_json::from_str(&serde_json::to_string(&exported).unwrap()).unwrap();
    exported.section.paragraphs[0].runs[0].content = RunContent::Text("new".into());
    let output = HwpxPatcher::patch_exported_section(&base, 0, &exported).unwrap().bytes;
    assert_eq!(texts(&output), [vec!["new", "keep"], vec!["old", "keep"]]);
    only_first_part_changed(&base, &output);
}

// 이것을 실패시키는 것: preservation.section_path 검사 삭제 — 동일 본문/해시로는 다른 파트를 구분 못 함.
#[test]
fn reversed_spine_rejects_metadata_for_the_other_part() {
    let base = reversed(vec![para("old")]);
    let mut exported = HwpxPatcher::export_section_for_edit(&base, 0, false).unwrap().exported;
    exported.preservation.as_mut().unwrap().section_path = SECOND.into();
    exported.section.paragraphs[0].runs[0].content = RunContent::Text("new".into());
    let error = HwpxPatcher::patch_exported_section(&base, 0, &exported).unwrap_err();
    assert!(error.to_string().contains("metadata targets"), "{error}");
}

// 이것을 실패시키는 것: 범위 밖 구역 인덱스를 첫 구역으로 폴백하기.
#[test]
fn reversed_spine_out_of_range_has_no_file_fallback() {
    let base = reversed(vec![para("old")]);
    let pkg = RawPackage::read(&base).unwrap();
    assert!(matches!(pkg.section_path(2), Err(HwpxError::InvalidStructure { .. })));
    let section = HwpxDecoder::decode(&base).unwrap().document.sections()[0].clone();
    assert!(HwpxPatcher::export_section_preservation(&base, 2, &section).is_err());
}

// 이것을 실패시키는 것: fill 캐시 제거/경고 경로를 숫자 파일명으로 되돌리기.
#[test]
fn reversed_spine_fill_changes_only_section_zero() {
    let field = Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("old".into()),
            help_text: None,
            name: Some("target".into()),
            display_text: "old".into(),
        },
        CharShapeIndex::new(0),
    );
    let base =
        reversed(vec![Paragraph::with_runs(vec![field], ParaShapeIndex::new(0)), para("keep")]);
    let mut pkg = RawPackage::read(&base).unwrap();
    // Same displayed body in both sections; names must be unique for the fill API.
    let xml = pkg.read_text_entry(SECOND).unwrap().replace("target", "other");
    pkg.replace_text_entry(SECOND, xml);
    let base = pkg.write().unwrap();
    let output = HwpxFiller::fill_with_codec_diagnostics(
        &base,
        &BTreeMap::from([("target".into(), "n\u{1}ew".into())]),
    )
    .unwrap();
    assert_eq!(output.value.filled[0].section, 0);
    let fields = HwpxFiller::list_fields(&output.value.bytes).unwrap();
    assert_eq!(
        fields.iter().map(|f| (f.section, f.current.as_str())).collect::<Vec<_>>(),
        [(0, "new"), (1, "old")]
    );
    assert_removed_part(&output.encode_warnings);
    only_first_part_changed(&base, &output.value.bytes);
    let pkg = RawPackage::read(&output.value.bytes).unwrap();
    assert!(!pkg.read_text_entry(FIRST).unwrap().contains("linesegarray"));
    assert!(pkg.read_text_entry(SECOND).unwrap().contains("linesegarray"));
}

// 이것을 실패시키는 것: 구조 삭제의 대상 경로를 숫자 파일명으로 되돌리기.
#[test]
fn reversed_spine_delete_changes_only_section_zero() {
    let base = reversed(vec![para("old"), para("keep")]);
    let output = HwpxStructuralEditor::delete_paragraphs(
        &base,
        &[ParagraphLocator { section: 0, index: 1 }],
    )
    .unwrap();
    assert_eq!(texts(&output), [vec!["old"], vec!["old", "keep"]]);
    only_first_part_changed(&base, &output);
}

// 이것을 실패시키는 것: 삽입 대상 경로를 숫자로 만들거나 임시 인코딩 파트에 원본 파트 경로를 재사용하기.
#[test]
fn reversed_spine_insert_resolves_base_and_encoded_parts_separately() {
    let base = reversed(vec![para("old"), para("keep")]);
    let output = HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
        &base,
        ParagraphLocator { section: 0, index: 0 },
        InsertPosition::After,
        &["n\u{1}ew".into()],
    )
    .unwrap();
    assert_eq!(texts(&output.value), [vec!["old", "new", "keep"], vec!["old", "keep"]]);
    assert_removed_part(&output.encode_warnings);
    only_first_part_changed(&base, &output.value);
}

fn assert_removed_part(warnings: &[EncodeWarning]) {
    let parts: Vec<_> = warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::XmlForbiddenCharsRemoved { part, .. } => Some(part.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(parts, [FIRST]);
}

// 이것을 실패시키는 것: XML 파서의 file을 Core 인덱스로 만들기.
#[test]
fn reversed_spine_xml_error_names_the_actual_part() {
    let mut pkg = RawPackage::read(&reversed(vec![para("old")])).unwrap();
    pkg.replace_text_entry(FIRST, "<hs:sec".into());
    let error = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap_err();
    assert!(matches!(error, HwpxError::XmlParse { ref file, .. } if file == FIRST), "{error:?}");
}

// 이것을 실패시키는 것: DecodeCtx.part를 Core 인덱스로 만들기.
#[test]
fn reversed_spine_content_loss_names_the_actual_part_and_core_index() {
    let mut pkg = RawPackage::read(&reversed(vec![para("old")])).unwrap();
    // Open CLICK_HERE fields now retain their body. Use an unsupported kind
    // to exercise actual content loss without weakening the part/path checks.
    let xml = pkg.read_text_entry(FIRST).unwrap().replace("<hp:t>old</hp:t>",
        r#"<hp:ctrl><hp:fieldBegin id="1" type="UNSUPPORTED" name="lost"/></hp:ctrl><hp:t>old</hp:t>"#);
    pkg.replace_text_entry(FIRST, xml);
    let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
    let warning = decoded
        .warnings
        .iter()
        .find(|w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind == "field_body"))
        .unwrap();
    assert_eq!(warning.part(), Some(FIRST));
    assert!(warning.location().unwrap().contains("section[0]"));
}

#[test]
fn reversed_spine_unclosed_field_retains_body_and_source_part() {
    let mut pkg = RawPackage::read(&reversed(vec![para("old")])).unwrap();
    let xml = pkg.read_text_entry(FIRST).unwrap().replace("<hp:t>old</hp:t>",
        r#"<hp:ctrl><hp:fieldBegin id="1" type="CLICK_HERE" name="kept"/></hp:ctrl><hp:t>old</hp:t>"#);
    pkg.replace_text_entry(FIRST, xml);
    let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
    let warning = decoded
        .warnings
        .iter()
        .find(|w| matches!(w, DecodeWarning::UnclosedFieldBoundary { .. }))
        .unwrap();
    assert_eq!(warning.part(), Some(FIRST));
    assert!(!decoded.warnings.iter().any(DecodeWarning::is_content_loss));
    assert_eq!(decoded.document.sections()[0].paragraphs[0].text_content(), "old");
    let boundary = decoded
        .source_map(0)
        .unwrap()
        .nodes()
        .find(|n| n.kind() == hwpforge_core::node_path::NodeKind::FieldBoundary)
        .unwrap();
    assert_eq!(boundary.part(), FIRST);
    assert_eq!(boundary.path().section(), 0);
    HwpxDecoder::verify_document_source(&decoded.document).unwrap();
    decoded.document.validate().unwrap();
}

fn table_base() -> Vec<u8> {
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    let width = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![para("name")], width),
        TableCell::new(vec![para("")], width),
    ])]);
    reversed(vec![Paragraph::with_runs(
        vec![Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )])
}

// 이것을 실패시키는 것: cell-edit 경고의 part를 구역 번호로 만들기.
#[test]
fn reversed_spine_cell_edit_warning_names_the_source_part() {
    let base = table_base();
    let output = HwpxCellEditor::set_cells_with_diagnostics(
        &base,
        &[CellSpec {
            table: 0,
            target: CellTarget::At(hwpforge_core::table::grid::GridCoord::new(0, 1)),
            text: "n\u{1}ew".into(),
        }],
    )
    .unwrap();
    assert_removed_part(&output.encode_warnings);
}

// 이것을 실패시키는 것: stamp 텍스트 힌트 경고의 part를 구역 번호로 만들기.
#[test]
fn reversed_spine_stamp_text_warning_names_the_source_part() {
    use crate::stamp::{HwpxStamper, StampAction, StampSpec};
    let base = reversed(vec![para("name (   )")]);
    let specs: Vec<_> = HwpxStamper::plan_bytes_v2(&base)
        .unwrap()
        .text
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: if c.section == 0 {
                StampAction::Field { name: "target".into(), hint: Some("n\u{1}ew".into()) }
            } else {
                StampAction::Ignore
            },
        })
        .collect();
    let output = HwpxStamper::stamp_with_diagnostics(&base, &specs).unwrap();
    assert_removed_part(&output.encode_warnings);
}

// 이것을 실패시키는 것: stamp 셀 힌트 경고의 part를 구역 번호로 만들기.
#[test]
fn reversed_spine_stamp_cell_warning_names_the_source_part() {
    use crate::stamp::{
        CellStampAction, CellStampSpec, HwpxStamper, StampRequestV2, STAMP_MAP_VERSION,
    };
    let base = table_base();
    let plan = HwpxStamper::plan_bytes_v2(&base).unwrap();
    let request = StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: plan.source_sha256,
        text: vec![],
        cells: vec![
            CellStampSpec {
                table: 0,
                at: hwpforge_core::table::grid::GridCoord::new(0, 1),
                label: None,
                action: CellStampAction::Field { name: "target".into(), hint: "n\u{1}ew".into() },
            },
            CellStampSpec {
                table: 1,
                at: hwpforge_core::table::grid::GridCoord::new(0, 1),
                label: None,
                action: CellStampAction::Ignore,
            },
        ],
    };
    let output = HwpxStamper::stamp_v2_with_diagnostics(&base, &request).unwrap();
    assert_removed_part(&output.encode_warnings);
}

// 이것을 실패시키는 것: 후처리 cellzone 손실 경고의 part를 구역 번호로 만들기.
#[test]
fn reversed_spine_cell_zone_warning_names_the_actual_part() {
    let mut pkg = RawPackage::read(&table_base()).unwrap();
    let xml = pkg.read_text_entry(FIRST).unwrap().replace("</hp:tbl>",
        r#"<hp:cellzoneList><hp:cellzone startRowAddr="0" startColAddr="0" endRowAddr="0" endColAddr="0" borderFillIDRef="999"/></hp:cellzoneList></hp:tbl>"#);
    pkg.replace_text_entry(FIRST, xml);
    let decoded = HwpxDecoder::decode(&pkg.write().unwrap()).unwrap();
    let warning = decoded
        .warnings
        .iter()
        .find(|w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind == "cellzone"))
        .unwrap();
    assert_eq!(warning.part(), Some(FIRST));
}
