//! Characters XML 1.0 cannot represent, arriving through an **edit surface**
//! (fill, patch, insert-para, set-cell, stamp).
//!
//! Every surface removes them from the caller's value at its entry and
//! reports one `XmlForbiddenCharsRemoved` per value — the same treatment the
//! encoder gives a regenerated part (#198). U+001F is not removed: the inline
//! text encoder writes it as `<hp:fwSpace/>`. Only what the caller changed is
//! looked at; a base document that already holds such characters keeps them
//! in every slot the edit leaves alone.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    EncodeWarning, FillError, HwpxDecoder, HwpxEncoder, HwpxFiller, HwpxPatcher,
};

const SECTION0: &str = "Contents/section0.xml";

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

fn field_para(name: &str, display: &str) -> Paragraph {
    let mut para = Paragraph::new(ParaShapeIndex::new(0));
    para.runs.push(Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("hint".to_string()),
            help_text: None,
            name: Some(name.to_string()),
            display_text: display.to_string(),
        },
        CharShapeIndex::new(0),
    ));
    para
}

/// Four paragraphs: a text the dirty variant poisons (`QQQ`), a filled
/// click-here field `f1`, a plain text to edit, and a second field `f2`.
fn clean() -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![
            text_para("keep QQQ"),
            field_para("f1", "old"),
            text_para("edit me"),
            field_para("f2", "QQF"),
        ],
        PageSettings::default(),
    ));
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::default())
        .expect("encode")
}

/// Replaces `from` with `to` in `Contents/section0.xml` — the only way to get
/// a character the encoder refuses into a base package.
fn splice(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut entry = zin.by_index(i).expect("entry");
        let name = entry.name().to_string();
        let method = entry.compression();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == SECTION0 {
            let text = String::from_utf8(data).expect("utf-8");
            assert!(text.contains(from), "{from:?} not in {text}");
            data = text.replace(from, to).into_bytes();
        }
        out.start_file(name, zip::write::SimpleFileOptions::default().compression_method(method))
            .expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// The base with U+0001 already in paragraph 0 and in field `f2`.
fn dirty() -> Vec<u8> {
    splice(&splice(&clean(), "QQQ", "A\u{1}B"), "QQF", "R\u{1}S")
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut entry = zip.by_name(SECTION0).expect("section0");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("utf-8");
    text
}

/// Each paragraph's text as Core reads it; a field shows as `[display]`.
fn texts(bytes: &[u8]) -> Vec<String> {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    decoded.document.sections()[0]
        .paragraphs
        .iter()
        .map(|p| {
            p.runs
                .iter()
                .map(|r| match &r.content {
                    RunContent::Text(t) => t.clone(),
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Field { display_text, .. } => format!("[{display_text}]"),
                        other => format!("{other:?}"),
                    },
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn removed(location: &str, count: usize) -> EncodeWarning {
    EncodeWarning::XmlForbiddenCharsRemoved {
        part: SECTION0.to_string(),
        location: location.to_string(),
        count,
    }
}

#[test]
fn the_dirty_base_keeps_its_characters_through_decode_and_export() {
    // The premise of every "already in the base" test below.
    let bytes = dirty();
    assert_eq!(texts(&bytes), ["keep A\u{1}B", "[old]", "edit me", "[R\u{1}S]"]);
    let section = HwpxDecoder::decode(&bytes).expect("decode").document.sections()[0].clone();
    HwpxPatcher::export_section_preservation(&bytes, 0, &section).expect("preservation");
}

// ── patch ───────────────────────────────────────────────────────────

/// Patches paragraph 2 (`edit me`) of `base` to `text`.
fn patch_para2(
    base: &[u8],
    text: &str,
) -> hwpforge_smithy_hwpx::WithCodecWarnings<hwpforge_smithy_hwpx::SectionPatchOutcome> {
    let mut exported =
        HwpxPatcher::export_section_for_edit(base, 0, false).expect("export").exported;
    let RunContent::Text(slot) = &mut exported.section.paragraphs[2].runs[0].content else {
        panic!("paragraph 2 is plain text");
    };
    *slot = text.to_string();
    HwpxPatcher::patch_exported_section_with_codec_diagnostics(base, 0, &exported).expect("patch")
}

const PARA2_SLOT: &str = "paragraphs[2].runs[0].text";

#[test]
fn patch_removes_a_control_from_the_edited_slot_and_names_the_slot() {
    // 이것을 실패시키는 것: patch 사전 판정에서 `sanitize_edit_text` 대신 원문을 쓰는 것
    // (인라인 escape 가 U+0001 을 그대로 두어 새 fragment 불변식이 patch 를 거부한다).
    let diagnosed = patch_para2(&clean(), "A\u{1}B");

    assert_eq!(texts(&diagnosed.value.bytes)[2], "AB");
    assert_eq!(diagnosed.encode_warnings, vec![removed(PARA2_SLOT, 1)]);
}

#[test]
fn patch_removes_nul_which_the_inline_escape_keeps() {
    // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 좁히는 것 (NUL 이 새 fragment 불변식에 걸려 실패).
    let diagnosed = patch_para2(&clean(), "A\u{0}B");

    assert!(!section_xml(&diagnosed.value.bytes).contains('\u{0}'));
    assert_eq!(texts(&diagnosed.value.bytes)[2], "AB");
    assert_eq!(diagnosed.encode_warnings, vec![removed(PARA2_SLOT, 1)]);
}

#[test]
fn patch_keeps_u001f_as_a_fixed_width_space() {
    // 이것을 실패시키는 것: U+001F 를 판정 집합에 넣는 것.
    let diagnosed = patch_para2(&clean(), "A\u{1F}B");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:fwSpace/>B"));
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn patch_reports_a_control_mixed_with_a_tab() {
    // 이것을 실패시키는 것: patch 사전 판정의 정리 호출을 빼는 것 (새 fragment 불변식이 patch 를 거부한다).
    let diagnosed = patch_para2(&clean(), "A\t\u{1}B");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:tab/>B"));
    assert_eq!(diagnosed.encode_warnings, vec![removed(PARA2_SLOT, 1)]);
}

#[test]
fn patch_writes_an_empty_slot_for_a_value_of_only_controls() {
    // 이것을 실패시키는 것: patch 사전 판정의 정리 호출을 빼는 것 (빈 slot 은 같지만 경고가 사라진다).
    let diagnosed = patch_para2(&clean(), "\u{1}\u{2}");

    assert_eq!(texts(&diagnosed.value.bytes)[2], "");
    assert_eq!(diagnosed.encode_warnings, vec![removed(PARA2_SLOT, 2)]);
}

#[test]
fn patch_keeps_the_bytes_when_the_cleaned_value_equals_the_base() {
    // The base spells the space as a character reference, so re-writing the
    // same text would change the bytes: only the second comparison keeps them.
    // 이것을 실패시키는 것: 정리 뒤 재비교를 빼는 것 (같은 값을 다시 써서 `&#32;` 가 사라진다).
    let base = splice(&clean(), "edit me", "edit&#32;me");
    let diagnosed = patch_para2(&base, "edit me\u{1}");

    assert_eq!(diagnosed.value.bytes, base, "nothing changed once the control is gone");
    assert_eq!(diagnosed.encode_warnings, vec![removed(PARA2_SLOT, 1)]);
}

#[test]
fn patch_leaves_a_neighbouring_slot_with_a_control_alone() {
    // 이것을 실패시키는 것: 검사·정리를 바뀐 slot 대신 section 전체로 넓히는 것.
    let diagnosed = patch_para2(&dirty(), "edited");

    let xml = section_xml(&diagnosed.value.bytes);
    assert!(xml.contains("keep A\u{1}B"), "the untouched slot keeps its bytes");
    assert!(xml.contains("R\u{1}S"), "the untouched field keeps its bytes");
    assert_eq!(texts(&diagnosed.value.bytes)[2], "edited");
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn patch_without_an_edit_returns_the_dirty_base_unchanged() {
    // 이것을 실패시키는 것: 원문 동일 비교보다 정리를 먼저 하는 것
    // (미편집 slot 의 `A\u{1}B` 가 `AB` 로 바뀌어 변경 후보가 된다).
    let base = dirty();
    let exported = HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
    let diagnosed = HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
        .expect("patch");

    assert_eq!(diagnosed.value.bytes, base);
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

/// The base for the older-twin tests: it warns on decode, so a wrapper that
/// dropped the decode channel would be seen.
fn warning_base() -> Vec<u8> {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/stale-line-cache.hwpx");
    std::fs::read(path).expect("fixture")
}

#[test]
fn the_older_patch_twins_return_the_same_bytes_and_drop_only_the_encode_channel() {
    // 이것을 실패시키는 것: 두 옛 patch twin 래퍼(`patch_section_preserving_with_diagnostics`·
    // `patch_exported_section_with_diagnostics`) 중 하나가 `decode_warnings` 를 버리는 것.
    let base = warning_base();
    let mut exported =
        HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
    let slot =
        exported.section.paragraphs.iter_mut().flat_map(|p| p.runs.iter_mut()).find_map(|r| {
            match &mut r.content {
                RunContent::Text(t) if !t.is_empty() => Some(t),
                _ => None,
            }
        });
    slot.expect("a text run").push_str("Z\u{0}");

    let codec = HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
        .expect("codec twin");
    let decode =
        HwpxPatcher::patch_exported_section_with_diagnostics(&base, 0, &exported).expect("twin");
    let plain = HwpxPatcher::patch_exported_section(&base, 0, &exported).expect("plain");
    let lower = HwpxPatcher::patch_section_preserving_with_diagnostics(
        &base,
        0,
        &exported.section,
        None,
        exported.preservation.as_ref(),
    )
    .expect("lower twin");

    assert!(!codec.decode_warnings.is_empty(), "the premise: the base warns on decode");
    assert_eq!(codec.encode_warnings.len(), 1, "{:?}", codec.encode_warnings);
    assert_eq!(codec.value.bytes, plain.bytes);
    assert_eq!(decode.value.bytes, plain.bytes);
    assert_eq!(lower.value, plain.bytes);
    assert_eq!(codec.decode_warnings, decode.warnings);
    assert_eq!(codec.decode_warnings, lower.warnings);
}

// ── fill ────────────────────────────────────────────────────────────

fn fill(
    base: &[u8],
    name: &str,
    value: &str,
) -> Result<hwpforge_smithy_hwpx::WithCodecWarnings<hwpforge_smithy_hwpx::FillOutcome>, FillError> {
    let values = BTreeMap::from([(name.to_string(), value.to_string())]);
    HwpxFiller::fill_with_codec_diagnostics(base, &values)
}

const F1: &str = "field 'f1'";

#[test]
fn fill_removes_a_control_and_names_the_field() {
    // 이것을 실패시키는 것: fill 입구 정리(`sanitize_edit_text`) 대신 원래 값을 쓰는 것.
    let diagnosed = fill(&clean(), "f1", "A\u{1}B").expect("fill");

    assert_eq!(texts(&diagnosed.value.bytes)[1], "[AB]");
    assert_eq!(diagnosed.value.filled[0].name, "f1");
    assert_eq!(diagnosed.encode_warnings, vec![removed(F1, 1)]);
}

#[test]
fn fill_removes_nul() {
    // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 좁히는 것.
    let diagnosed = fill(&clean(), "f1", "A\u{0}B").expect("fill");

    assert!(!section_xml(&diagnosed.value.bytes).contains('\u{0}'));
    assert_eq!(diagnosed.encode_warnings, vec![removed(F1, 1)]);
}

#[test]
fn fill_keeps_u001f_as_a_fixed_width_space() {
    // 이것을 실패시키는 것: U+001F 를 판정 집합에 넣는 것.
    let diagnosed = fill(&clean(), "f1", "A\u{1F}B").expect("fill");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:fwSpace/>B"));
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn fill_reports_a_control_mixed_with_a_tab() {
    // 이것을 실패시키는 것: fill 입구 정리를 빼는 것 (아래 patch 단계가 지우지만 그 경고는 fill 이 버린다).
    let diagnosed = fill(&clean(), "f1", "A\t\u{1}B").expect("fill");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:tab/>B"));
    assert_eq!(diagnosed.encode_warnings, vec![removed(F1, 1)]);
}

#[test]
fn fill_refuses_a_value_of_only_controls_as_empty() {
    // 이것을 실패시키는 것: fill 의 빈 값 검사를 정리 전 값(`value.is_empty()`)으로 되돌리는 것.
    let error = fill(&clean(), "f1", "\u{1}\u{2}").unwrap_err();

    assert!(matches!(&error, FillError::EmptyValue { name } if name == "f1"), "{error:?}");
}

#[test]
fn fill_keeps_the_bytes_when_the_cleaned_value_equals_the_current_one() {
    // 이것을 실패시키는 것: "값이 바뀌었나" 비교를 정리 전 값으로 하는 것.
    let base = clean();
    let diagnosed = fill(&base, "f1", "old\u{1}").expect("fill");

    assert_eq!(diagnosed.value.bytes, base);
    assert!(diagnosed.value.filled.is_empty(), "{:?}", diagnosed.value.filled);
    assert_eq!(diagnosed.encode_warnings, vec![removed(F1, 1)]);
}

#[test]
fn fill_leaves_neighbouring_controls_in_the_base_alone() {
    // 이것을 실패시키는 것: 검사·정리를 채운 필드 대신 section 전체로 넓히는 것.
    let diagnosed = fill(&dirty(), "f1", "new").expect("fill");

    let xml = section_xml(&diagnosed.value.bytes);
    assert!(xml.contains("keep A\u{1}B") && xml.contains("R\u{1}S"), "{xml}");
    assert_eq!(texts(&diagnosed.value.bytes)[1], "[new]");
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn fill_leaves_a_field_alone_when_its_current_value_is_requested_again() {
    // The base already holds `R\u{1}S` in `f2`; asking for exactly that value
    // is no change, so nothing is cleaned, written or reported.
    // 이것을 실패시키는 것: 현재 값과의 원문 비교 전에 정리하는 것 (`RS` 로 다시 쓰고 경고·캐시 무효화).
    let base = dirty();
    let diagnosed = fill(&base, "f2", "R\u{1}S").expect("fill");

    assert_eq!(diagnosed.value.bytes, base);
    assert!(diagnosed.value.filled.is_empty(), "{:?}", diagnosed.value.filled);
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn the_older_fill_twins_return_the_same_outcome() {
    // 이것을 실패시키는 것: `fill_with_diagnostics` 래퍼가 `decode_warnings` 를 버리는 것.
    let base = warning_base();
    let values = BTreeMap::from([("user_email".to_string(), "A\u{0}B".to_string())]);

    let codec = HwpxFiller::fill_with_codec_diagnostics(&base, &values).expect("codec twin");
    let decode = HwpxFiller::fill_with_diagnostics(&base, &values).expect("twin");
    let plain = HwpxFiller::fill(&base, &values).expect("plain");

    assert!(!codec.decode_warnings.is_empty(), "the premise: the base warns on decode");
    assert_eq!(codec.encode_warnings.len(), 1, "{:?}", codec.encode_warnings);
    assert_eq!(section_xml(&codec.value.bytes), section_xml(&plain.bytes));
    assert_eq!(section_xml(&decode.value.bytes), section_xml(&plain.bytes));
    assert_eq!(codec.decode_warnings, decode.warnings);
}

// ── insert-para ─────────────────────────────────────────────────────

fn insert(
    text: &str,
) -> Result<
    hwpforge_smithy_hwpx::WithCodecWarnings<Vec<u8>>,
    hwpforge_smithy_hwpx::StructuralEditError,
> {
    hwpforge_smithy_hwpx::HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
        &clean(),
        hwpforge_smithy_hwpx::ParagraphLocator { section: 0, index: 2 },
        hwpforge_smithy_hwpx::InsertPosition::After,
        &[text.to_string()],
    )
}

const NEW_PARA: &str = "new paragraph 3";

#[test]
fn insert_removes_a_control_and_passes_its_self_verify() {
    // Before this change the self-verify compared the output with the raw
    // text and refused the edit.
    // 이것을 실패시키는 것: 새 문단 구성·expected delta 에 정리 전 원문을 쓰는 것 (`DeltaMismatch`).
    let diagnosed = insert("A\u{1}B").expect("insert");

    assert_eq!(texts(&diagnosed.value)[3], "AB");
    assert_eq!(diagnosed.encode_warnings, vec![removed(NEW_PARA, 1)]);
}

#[test]
fn insert_removes_nul() {
    // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 좁히는 것 (NUL 이 남아 self-verify 가 어긋남).
    let diagnosed = insert("A\u{0}B").expect("insert");

    assert!(!section_xml(&diagnosed.value).contains('\u{0}'));
    assert_eq!(diagnosed.encode_warnings, vec![removed(NEW_PARA, 1)]);
}

#[test]
fn insert_keeps_u001f_as_a_fixed_width_space() {
    // 이것을 실패시키는 것: U+001F 를 판정 집합에 넣는 것.
    let diagnosed = insert("A\u{1F}B").expect("insert");

    assert!(section_xml(&diagnosed.value).contains("A<hp:fwSpace/>B"));
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn insert_reports_a_control_mixed_with_a_tab() {
    // 이것을 실패시키는 것: insert 입구 정리를 빼는 것.
    let diagnosed = insert("A\t\u{1}B").expect("insert");

    assert!(section_xml(&diagnosed.value).contains("A<hp:tab/>B"));
    assert_eq!(diagnosed.encode_warnings, vec![removed(NEW_PARA, 1)]);
}

#[test]
fn insert_writes_an_empty_paragraph_for_a_text_of_only_controls() {
    // 이것을 실패시키는 것: insert 입구 정리를 빼는 것.
    let diagnosed = insert("\u{1}\u{2}").expect("insert");

    assert_eq!(texts(&diagnosed.value)[3], "");
    assert_eq!(diagnosed.encode_warnings, vec![removed(NEW_PARA, 2)]);
}

#[test]
fn the_older_insert_twins_return_the_same_bytes() {
    // 이것을 실패시키는 것: `insert_paragraphs_with_diagnostics` 래퍼가 `decode_warnings` 를 버리는 것.
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = warning_base();
    let anchor = ParagraphLocator { section: 0, index: 2 };
    let texts_in = ["A\u{1}B".to_string()];

    let codec = HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
        &base,
        anchor,
        InsertPosition::After,
        &texts_in,
    )
    .expect("codec twin");
    let decode = HwpxStructuralEditor::insert_paragraphs_with_diagnostics(
        &base,
        anchor,
        InsertPosition::After,
        &texts_in,
    )
    .expect("twin");
    let plain =
        HwpxStructuralEditor::insert_paragraphs(&base, anchor, InsertPosition::After, &texts_in)
            .expect("plain");

    assert!(!codec.decode_warnings.is_empty(), "the premise: the base warns on decode");
    assert_eq!(codec.encode_warnings.len(), 1, "{:?}", codec.encode_warnings);
    assert_eq!(codec.value, plain);
    assert_eq!(decode.value, plain);
    assert_eq!(codec.decode_warnings, decode.warnings);
}

// ── set-cell ────────────────────────────────────────────────────────

fn set_cell(
    text: &str,
) -> hwpforge_smithy_hwpx::WithCodecWarnings<hwpforge_smithy_hwpx::CellEditResult> {
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/tables/merged_grid_form.hwpx");
    let base = std::fs::read(path).expect("fixture");
    let spec = hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            0, 0,
        )),
        text: text.to_string(),
    };
    hwpforge_smithy_hwpx::HwpxCellEditor::set_cells_with_diagnostics(&base, &[spec])
        .expect("set-cell")
}

fn cell00(bytes: &[u8]) -> String {
    let table = hwpforge_smithy_hwpx::HwpxReader::read_table(bytes, 0).expect("table");
    table.cells.iter().find(|c| c.row == 0 && c.col == 0).expect("cell (0, 0)").text.clone()
}

/// The cell clean-up warnings, without whatever the encoder reports about
/// the rest of the fixture.
fn cell_removals(warnings: &[EncodeWarning]) -> Vec<EncodeWarning> {
    warnings
        .iter()
        .filter(|w| matches!(w, EncodeWarning::XmlForbiddenCharsRemoved { .. }))
        .cloned()
        .collect()
}

const CELL00: &str = "table 0 cell (0, 0)";

#[test]
fn set_cell_removes_a_control_and_names_the_cell() {
    // Before this change the encoder's part scan reported it as `hp:t`.
    // 이것을 실패시키는 것: set-cell 입구 정리를 빼는 것 (경고 위치가 `hp:t` 로 돌아간다).
    let diagnosed = set_cell("A\u{1}B");

    assert_eq!(cell00(&diagnosed.value.bytes), "AB");
    assert_eq!(cell_removals(&diagnosed.encode_warnings), vec![removed(CELL00, 1)]);
}

#[test]
fn set_cell_removes_nul() {
    // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 좁히는 것 (경고 위치가 `hp:t` 로 바뀐다).
    let diagnosed = set_cell("A\u{0}B");

    assert!(!section_xml(&diagnosed.value.bytes).contains('\u{0}'));
    assert_eq!(cell_removals(&diagnosed.encode_warnings), vec![removed(CELL00, 1)]);
}

#[test]
fn set_cell_keeps_u001f_as_a_fixed_width_space() {
    // 이것을 실패시키는 것: U+001F 를 판정 집합에 넣는 것.
    let diagnosed = set_cell("A\u{1F}B");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:fwSpace/>B"));
    assert!(
        cell_removals(&diagnosed.encode_warnings).is_empty(),
        "{:?}",
        diagnosed.encode_warnings
    );
}

#[test]
fn set_cell_reports_a_control_mixed_with_a_tab() {
    // The tab sends the text down the mixed-content path, whose escape used to
    // drop the control before the part scan could see it: no warning at all.
    // 이것을 실패시키는 것: set-cell 입구 정리를 빼는 것.
    let diagnosed = set_cell("A\t\u{1}B");

    assert!(section_xml(&diagnosed.value.bytes).contains("A<hp:tab/>B"));
    assert_eq!(cell_removals(&diagnosed.encode_warnings), vec![removed(CELL00, 1)]);
}

#[test]
fn set_cell_clears_the_cell_for_a_text_of_only_controls() {
    // 이것을 실패시키는 것: set-cell 입구 정리를 빼는 것 (`cleared` 가 거짓).
    let diagnosed = set_cell("\u{1}\u{2}");

    assert_eq!(cell00(&diagnosed.value.bytes), "");
    assert!(diagnosed.value.outcome.cells[0].cleared);
    assert_eq!(cell_removals(&diagnosed.encode_warnings), vec![removed(CELL00, 2)]);
}

#[test]
fn set_cell_names_the_part_of_the_section_holding_the_table() {
    // 이것을 실패시키는 것: 경고 part 를 표가 든 section 대신 `section0` 으로 고정하는 것.
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;

    let cell = TableCell::with_span(vec![text_para("x")], HwpUnit::new(8000).unwrap(), 1, 1);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![text_para("first")], PageSettings::default()));
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    let base =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::default())
            .expect("encode");

    let spec = hwpforge_smithy_hwpx::CellSpec {
        table: 0,
        target: hwpforge_smithy_hwpx::CellTarget::At(hwpforge_core::table::grid::GridCoord::new(
            0, 0,
        )),
        text: "A\u{1}B".to_string(),
    };
    let diagnosed =
        hwpforge_smithy_hwpx::HwpxCellEditor::set_cells_with_diagnostics(&base, &[spec])
            .expect("set-cell");

    assert_eq!(
        cell_removals(&diagnosed.encode_warnings),
        vec![EncodeWarning::XmlForbiddenCharsRemoved {
            part: "Contents/section1.xml".to_string(),
            location: CELL00.to_string(),
            count: 1,
        }]
    );
}

// ── stamp ───────────────────────────────────────────────────────────

/// v1: the one `(   )` candidate of the stale-cache fixture, stamped as a
/// field `name` with `hint`.
fn stamp_v1(
    name: &str,
    hint: &str,
) -> Result<
    hwpforge_smithy_hwpx::WithCodecWarnings<hwpforge_smithy_hwpx::stamp::StampResult>,
    hwpforge_smithy_hwpx::stamp::StamperError,
> {
    use hwpforge_smithy_hwpx::stamp::{HwpxStamper, StampAction, StampSpec};
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/stale-line-cache.hwpx");
    let base = std::fs::read(path).expect("fixture");
    let specs: Vec<StampSpec> = HwpxStamper::plan_bytes_v2(&base)
        .expect("plan")
        .text
        .into_iter()
        .map(|c| StampSpec {
            section: c.section,
            path: c.path,
            span: c.span,
            marker: c.marker,
            action: StampAction::Field { name: name.to_string(), hint: Some(hint.to_string()) },
        })
        .collect();
    HwpxStamper::stamp_with_diagnostics(&base, &specs)
}

fn field_hint(bytes: &[u8], name: &str) -> Option<String> {
    HwpxFiller::list_fields(bytes)
        .expect("fields")
        .into_iter()
        .find(|f| f.name.as_deref() == Some(name))
        .expect("stamped field")
        .hint
}

#[test]
fn stamp_removes_a_control_from_a_hint() {
    // Before this change the attribute escape dropped it without a warning.
    // 이것을 실패시키는 것: v1 입구의 `clean_text_hints` 를 빼는 것.
    let diagnosed = stamp_v1("ok", "H\u{1}I").expect("stamp");

    assert_eq!(field_hint(&diagnosed.value.bytes, "ok").as_deref(), Some("HI"));
    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'ok' hint", 1)]);
}

#[test]
fn stamp_removes_nul_from_a_hint() {
    // 이것을 실패시키는 것: 판정 범위를 `'\u{1}'..` 로 좁히는 것 (NUL 이 속성에 남는다).
    let diagnosed = stamp_v1("ok", "H\u{0}I").expect("stamp");

    assert!(!section_xml(&diagnosed.value.bytes).contains('\u{0}'));
    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'ok' hint", 1)]);
}

#[test]
fn stamp_keeps_a_hint_of_only_controls_as_an_empty_hint() {
    // 이것을 실패시키는 것: v1 입구의 `clean_text_hints` 를 빼는 것 (경고가 사라진다).
    let diagnosed = stamp_v1("ok", "\u{1}\u{2}").expect("stamp");

    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'ok' hint", 2)]);
}

#[test]
fn stamp_leaves_a_name_with_a_control_to_fail_closed() {
    // Names are identifiers the manifest re-reads; they are not cleaned.
    // 이것을 실패시키는 것: 이름까지 정리하는 것 (`AB` 로 조용히 이름이 바뀌어 성공한다).
    let error = stamp_v1("A\u{1}B", "hint").unwrap_err();

    assert!(error.to_string().contains("missing from output"), "{error}");
}

/// v2: a one-row table `[성명 | (empty)]`, its empty cell stamped as a
/// field `f` with `hint`.
fn stamp_cell(
    hint: &str,
) -> Result<
    hwpforge_smithy_hwpx::WithCodecWarnings<hwpforge_smithy_hwpx::stamp::StampResultV2>,
    hwpforge_smithy_hwpx::stamp::StamperError,
> {
    use hwpforge_core::table::grid::GridCoord;
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_foundation::HwpUnit;
    use hwpforge_smithy_hwpx::stamp::{
        CellStampAction, CellStampSpec, HwpxStamper, StampRequestV2, STAMP_MAP_VERSION,
    };

    let width = HwpUnit::new(8000).unwrap();
    let row = TableRow::new(vec![
        TableCell::with_span(vec![text_para("성명")], width, 1, 1),
        TableCell::with_span(vec![text_para("")], width, 1, 1),
    ]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(Table::new(vec![row]), CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::default()));
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    let base =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::default())
            .expect("encode");

    let plan = HwpxStamper::plan_bytes_v2(&base).expect("plan");
    let request = StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: plan.source_sha256,
        text: Vec::new(),
        cells: vec![CellStampSpec {
            table: 0,
            at: GridCoord::new(0, 1),
            label: None,
            action: CellStampAction::Field { name: "f".to_string(), hint: hint.to_string() },
        }],
    };
    HwpxStamper::stamp_v2_with_diagnostics(&base, &request)
}

#[test]
fn stamp_v2_removes_a_control_from_a_cell_hint() {
    // 이것을 실패시키는 것: v2 입구의 `clean_cell_hints` 를 빼는 것.
    let diagnosed = stamp_cell("H\u{1}I").expect("stamp");

    assert!(section_xml(&diagnosed.value.bytes).contains("<hp:t>HI</hp:t>"));
    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'f' hint", 1)]);
}

#[test]
fn stamp_v2_keeps_u001f_in_a_cell_hint_as_a_fixed_width_space() {
    // The entry keeps U+001F, so the shown hint has its fixed-width space. The
    // field's `Direction` and `Command` parameters are not text runs and cannot
    // hold it; the section scan removes it there and reports both (R1-5).
    // 이것을 실패시키는 것: U+001F 를 판정 집합에 넣는 것 (입구 경고가 하나 더 붙는다).
    let diagnosed = stamp_cell("H\u{1F}I").expect("stamp");

    assert!(section_xml(&diagnosed.value.bytes).contains("H<hp:fwSpace/>I"));
    assert_eq!(diagnosed.encode_warnings, vec![removed("hp:stringParam", 2)]);
}

#[test]
fn stamp_v2_reports_a_control_mixed_with_a_tab() {
    // 이것을 실패시키는 것: v2 입구의 `clean_cell_hints` 를 빼는 것
    // (section 스캔이 `field 'f' hint` 대신 `hp:stringParam`·`hp:t` 로 보고한다).
    let diagnosed = stamp_cell("H\t\u{1}I").expect("stamp");

    assert!(section_xml(&diagnosed.value.bytes).contains("H<hp:tab/>I"));
    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'f' hint", 1)]);
}

#[test]
fn stamp_v2_refuses_a_cell_hint_of_only_controls_as_blank() {
    // 이것을 실패시키는 것: 정리를 빈 hint 검사 뒤로 미루는 것 (빈 hint 로 성공한다).
    let error = stamp_cell("\u{1}\u{2}").unwrap_err();

    assert!(error.to_string().contains("hint must not be blank"), "{error}");
}

#[test]
fn stamp_v2_removes_a_control_from_a_text_hint() {
    // 이것을 실패시키는 것: v2 입구에서 text spec 의 `clean_text_hints` 를 빼는 것.
    use hwpforge_smithy_hwpx::stamp::{
        CellStampAction, CellStampSpec, HwpxStamper, StampAction, StampRequestV2, StampSpec,
        STAMP_MAP_VERSION,
    };
    let path =
        concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/layout/stale-line-cache.hwpx");
    let base = std::fs::read(path).expect("fixture");
    let plan = HwpxStamper::plan_bytes_v2(&base).expect("plan");
    let request = StampRequestV2 {
        schema_version: STAMP_MAP_VERSION,
        source_sha256: plan.source_sha256,
        text: plan
            .text
            .clone()
            .into_iter()
            .map(|c| StampSpec {
                section: c.section,
                path: c.path,
                span: c.span,
                marker: c.marker,
                action: StampAction::Field {
                    name: "ok".to_string(),
                    hint: Some("H\u{1}I".to_string()),
                },
            })
            .collect(),
        cells: plan
            .cells
            .iter()
            .map(|c| CellStampSpec {
                table: c.table,
                at: c.at,
                label: None,
                action: CellStampAction::Ignore,
            })
            .collect(),
    };

    let diagnosed = HwpxStamper::stamp_v2_with_diagnostics(&base, &request).expect("stamp");

    assert_eq!(field_hint(&diagnosed.value.bytes, "ok").as_deref(), Some("HI"));
    assert_eq!(diagnosed.encode_warnings, vec![removed("field 'ok' hint", 1)]);
}
