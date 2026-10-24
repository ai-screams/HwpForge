//! A Core CR (`\r`) must reach the wire as `&#13;` on every `<hp:t>` writer.
//!
//! XML parsers normalise a raw CR to LF. The serde run path always wrote the
//! character reference; the manual inline path (`encode_inline_text_xml`)
//! wrote the raw byte, so a run that also held LF/TAB/NBSP/fwSpace, an
//! `InlineText` plain segment, a field display body or a patched slot came
//! back as `\n` (#207).

use std::io::{Read, Write};

use hwpforge_core::control::Control;
use hwpforge_core::document::Document;
use hwpforge_core::image::ImageStore;
use hwpforge_core::inline::{InlineSegment, InlineTabAttr, InlineText};
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::table::grid::GridCoord;
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::PageSettings;
use hwpforge_foundation::{CharShapeIndex, FieldType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{
    CellSpec, CellTarget, HwpxCellEditor, HwpxDecoder, HwpxEncoder, HwpxPatcher,
};

const SECTION0: &str = "Contents/section0.xml";

fn run_para(run: Run) -> Paragraph {
    Paragraph::with_runs(vec![run], ParaShapeIndex::new(0))
}

fn text_para(text: &str) -> Paragraph {
    run_para(Run::text(text, CharShapeIndex::new(0)))
}

fn encode(paragraphs: Vec<Paragraph>) -> Vec<u8> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::default()));
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::default())
        .expect("encode")
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut entry = zip.by_name(SECTION0).expect("section0");
    let mut text = String::new();
    entry.read_to_string(&mut text).expect("utf-8");
    text
}

/// Replaces `from` with `to` in section0 (the way to hand-write a base).
fn splice(bytes: &[u8], from: &str, to: &str) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut entry = zin.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
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

/// Every run of the first section as Core reads it, flattened to text; a
/// field shows its display text.
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
                    RunContent::InlineText(it) => it.plain_text(),
                    RunContent::Control(c) => match c.as_ref() {
                        Control::Field { display_text, .. } => display_text.clone(),
                        Control::Hyperlink { text, .. } => text.clone(),
                        other => format!("{other:?}"),
                    },
                    other => format!("{other:?}"),
                })
                .collect()
        })
        .collect()
}

fn assert_wire_has_reference_only(bytes: &[u8]) {
    let xml = section_xml(bytes);
    assert!(xml.contains("&#13;"), "{xml}");
    assert!(!xml.contains('\r'), "a raw CR reached the section: {xml:?}");
}

#[test]
fn a_cr_mixed_with_tab_lf_nbsp_or_fwspace_is_written_as_a_reference() {
    // 이것을 실패시키는 것: `encode_inline_text_xml` 의 `'\r'` 갈래를 빼는 것
    // (수동 경로가 CR 을 그대로 써서 LF 로 되읽힌다).
    let samples = ["가\r나\t다", "가\r나\n다", "가\r나\u{A0}다", "가\r나\u{1F}다"];
    let bytes = encode(samples.iter().map(|s| text_para(s)).collect());

    assert_wire_has_reference_only(&bytes);
    assert_eq!(texts(&bytes), samples);
}

#[test]
fn a_cr_in_an_inline_text_plain_segment_is_written_as_a_reference() {
    // 이것을 실패시키는 것: `build_inline_text_element_xml` 의 Plain 갈래가
    // `encode_inline_text_xml` 대신 CR 을 그대로 쓰는 `escape_xml` 을 부르는 것.
    let tab = InlineTabAttr { width: HwpUnit::new(1000).unwrap(), leader: 1, tab_type: 1 };
    let inline = InlineText::from_segments([
        InlineSegment::Plain("가\r나".to_string()),
        InlineSegment::Tab(tab),
        InlineSegment::Plain("다".to_string()),
    ]);
    let bytes = encode(vec![run_para(Run {
        content: RunContent::InlineText(inline),
        char_shape_id: CharShapeIndex::new(0),
    })]);

    assert_wire_has_reference_only(&bytes);
    assert_eq!(texts(&bytes), ["가\r나\t다"]);
}

#[test]
fn a_cr_in_a_field_display_body_is_written_as_a_reference() {
    // 이것을 실패시키는 것: 필드 표시 본문의 `build_text_element_xml` 호출을
    // CR 을 그대로 쓰는 직접 이스케이프로 바꾸는 것.
    let click_here = Control::Field {
        field_type: FieldType::ClickHere,
        hint_text: Some("힌트".to_string()),
        help_text: None,
        name: Some("f".to_string()),
        display_text: "가\r나".to_string(),
    };
    let hyperlink =
        Control::Hyperlink { text: "가\r나".to_string(), url: "https://example.com".to_string() };
    for control in [click_here, hyperlink] {
        let mut para = Paragraph::new(ParaShapeIndex::new(0));
        para.runs.push(Run::control(control.clone(), CharShapeIndex::new(0)));
        let bytes = encode(vec![para]);

        assert_wire_has_reference_only(&bytes);
        assert_eq!(texts(&bytes), ["가\r나"], "{control:?}");
    }
}

/// Patches paragraph 1 (`edit me`) of `base` to `text`.
fn patch_para1(base: &[u8], text: &str) -> Vec<u8> {
    let mut exported =
        HwpxPatcher::export_section_for_edit(base, 0, false).expect("export").exported;
    let RunContent::Text(slot) = &mut exported.section.paragraphs[1].runs[0].content else {
        panic!("paragraph 1 is plain text");
    };
    *slot = text.to_string();
    HwpxPatcher::patch_exported_section_with_codec_diagnostics(base, 0, &exported)
        .expect("patch")
        .value
        .bytes
}

#[test]
fn a_patched_slot_with_a_cr_keeps_it_and_an_untouched_reference_keeps_its_bytes() {
    // 이것을 실패시키는 것: `encode_inline_text_xml` 의 `'\r'` 갈래를 빼는 것
    // (편집된 slot 이 raw CR 로 쓰여 `\n` 으로 되읽힌다). 미편집 slot 의
    // 바이트 보존은 patch 가 그 slot 을 다시 쓰는 것을 잡는다.
    let base = splice(&encode(vec![text_para("keep"), text_para("edit me")]), "keep", "가&#13;나");

    let noop = {
        let exported =
            HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
        HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
            .expect("patch")
            .value
            .bytes
    };
    assert_eq!(noop, base, "a no-op patch is byte-identical");

    let patched = patch_para1(&base, "다\r라\t마");
    assert_wire_has_reference_only(&patched);
    assert!(section_xml(&patched).contains("가&#13;나"), "the untouched slot keeps its bytes");
    assert_eq!(texts(&patched), ["가\r나", "다\r라\t마"]);
}

#[test]
fn set_cell_accepts_a_document_holding_a_cr_beside_a_tab() {
    // 이것을 실패시키는 것: `encode_inline_text_xml` 의 `'\r'` 갈래를 빼는 것
    // (decode → encode → decode 가 `\r` 을 `\n` 으로 바꿔 admission 이 문서를 거부한다).
    let width = HwpUnit::new(8000).unwrap();
    let table = Table::new(vec![TableRow::new(vec![
        TableCell::new(vec![text_para("성명")], width),
        TableCell::new(vec![text_para("")], width),
    ])]);
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::table(table, CharShapeIndex::new(0)));
    let base = splice(&encode(vec![text_para("ZZZ"), host]), "ZZZ", "가&#13;나<hp:tab/>다");
    assert_eq!(texts(&base)[0], "가\r나\t다");

    let spec =
        CellSpec { table: 0, target: CellTarget::At(GridCoord::new(0, 1)), text: "라".into() };
    let edited = HwpxCellEditor::set_cells(&base, &[spec]).expect("admission accepts the base");

    let after = texts(&edited.bytes);
    assert_eq!(after[0], "가\r나\t다");
}
