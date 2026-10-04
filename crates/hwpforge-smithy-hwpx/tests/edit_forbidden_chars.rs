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
    // (인라인 escape 가 U+0001 을 무경고로 지워 글자는 같고 경고만 사라진다).
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
    // 이것을 실패시키는 것: patch 사전 판정의 정리 호출을 빼는 것 (탭 경로 escape 가 무경고로 지움).
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

#[test]
fn the_older_patch_twins_return_the_same_bytes_and_drop_only_the_encode_channel() {
    // 이것을 실패시키는 것: 래퍼가 정리 전 원문으로 따로 패치하는 것.
    let base = clean();
    let mut exported =
        HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
    let RunContent::Text(slot) = &mut exported.section.paragraphs[2].runs[0].content else {
        panic!("paragraph 2 is plain text");
    };
    *slot = "A\u{0}B".to_string();

    let codec = HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
        .expect("codec twin");
    let decode =
        HwpxPatcher::patch_exported_section_with_diagnostics(&base, 0, &exported).expect("twin");
    let plain = HwpxPatcher::patch_exported_section(&base, 0, &exported).expect("plain");

    assert_eq!(codec.value.bytes, plain.bytes);
    assert_eq!(decode.value.bytes, plain.bytes);
    assert_eq!(codec.decode_warnings, decode.warnings);
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
    // 이것을 실패시키는 것: fill 입구 정리를 빼는 것 (탭 경로 escape 가 무경고로 지움).
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
fn the_older_fill_twins_return_the_same_outcome() {
    // 이것을 실패시키는 것: 래퍼가 정리 전 원래 값으로 따로 채우는 것.
    let base = clean();
    let values = BTreeMap::from([("f1".to_string(), "A\u{0}B".to_string())]);

    let codec = HwpxFiller::fill_with_codec_diagnostics(&base, &values).expect("codec twin");
    let decode = HwpxFiller::fill_with_diagnostics(&base, &values).expect("twin");
    let plain = HwpxFiller::fill(&base, &values).expect("plain");

    assert_eq!(texts(&codec.value.bytes), texts(&plain.bytes));
    assert_eq!(texts(&decode.value.bytes), texts(&plain.bytes));
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
    // 이것을 실패시키는 것: 래퍼가 정리 전 원문으로 따로 삽입하는 것.
    use hwpforge_smithy_hwpx::{HwpxStructuralEditor, InsertPosition, ParagraphLocator};
    let base = clean();
    let anchor = ParagraphLocator { section: 0, index: 2 };
    let texts_in = ["A\u{1}B".to_string()];

    let codec = HwpxStructuralEditor::insert_paragraphs_with_codec_diagnostics(
        &base,
        anchor,
        InsertPosition::After,
        &texts_in,
    )
    .expect("codec twin");
    let plain =
        HwpxStructuralEditor::insert_paragraphs(&base, anchor, InsertPosition::After, &texts_in)
            .expect("plain");

    assert_eq!(codec.value, plain);
}
