//! Issue #198: characters XML 1.0 forbids (C0 controls other than TAB, LF and
//! CR, and U+FFFE/U+FFFF) must not reach a serde-written part, and their
//! removal must be reported.
//!
//! The three inputs are the ones the issue reproduced through `from-json`: a
//! style `engName` with U+0001, a font face with U+0002 and body text with
//! U+0001. They go through the public encode path, and the test reads the
//! parts back out of the ZIP.

use std::collections::BTreeMap;
use std::io::{Read, Write};

use hwpforge_core::chart::{ChartData, ChartType};
use hwpforge_core::control::Control;
use hwpforge_core::image::ImageStore;
use hwpforge_core::metadata::Metadata;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::{MasterPage, Section};
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{ApplyPageType, CharShapeIndex, FieldType, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyle, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning, HwpxEncoder, HwpxFiller, HwpxPatcher};

/// Encodes one paragraph of `body` with one style and one font face.
fn encode(face: &str, eng_name: &str, body: &str) -> (Vec<(String, String)>, Vec<EncodeWarning>) {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![text_para(body)], PageSettings::a4()));
    encode_with(doc, face, eng_name)
}

fn text_para(text: &str) -> Paragraph {
    Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
}

/// Encodes `doc` with one style and one font face.
fn encode_with(
    doc: Document,
    face: &str,
    eng_name: &str,
) -> (Vec<(String, String)>, Vec<EncodeWarning>) {
    let mut styles = HwpxStyleStore::with_default_fonts(face);
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles.push_style(HwpxStyle::new(0, "PARA", "바탕글", eng_name, 0, 0, 0, 1042, 0));

    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .expect("encode");
    (xml_parts(&outcome.bytes), outcome.warnings)
}

/// Encodes a one-section document whose only paragraph is `para`.
fn encode_para(para: Paragraph) -> (Vec<(String, String)>, Vec<EncodeWarning>) {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    encode_with(doc, "함초롬돋움", "Normal")
}

/// No part of the package holds a character XML 1.0 forbids.
fn assert_no_forbidden_char(parts: &[(String, String)]) {
    for (name, text) in parts {
        let bad: Vec<char> = text.chars().filter(|&c| is_forbidden(c)).collect();
        assert!(bad.is_empty(), "{name} still holds {bad:?}");
    }
}

/// Every XML part of the package (`*.xml`, `*.hpf`), by entry name.
fn xml_parts(bytes: &[u8]) -> Vec<(String, String)> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut parts = Vec::new();
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().to_string();
        if name.ends_with(".xml") || name.ends_with(".hpf") {
            let mut text = String::new();
            entry.read_to_string(&mut text).expect("utf-8 part");
            parts.push((name, text));
        }
    }
    parts
}

fn part<'a>(parts: &'a [(String, String)], name: &str) -> &'a str {
    &parts.iter().find(|(n, _)| n == name).unwrap_or_else(|| panic!("no part {name}")).1
}

/// The XML 1.0 `Char` production, negated.
fn is_forbidden(c: char) -> bool {
    matches!(c, '\u{0}'..='\u{8}' | '\u{B}' | '\u{C}' | '\u{E}'..='\u{1F}' | '\u{FFFE}' | '\u{FFFF}')
}

/// `(part, location, count)` of every removal warning, in warning order.
fn removals(warnings: &[EncodeWarning]) -> Vec<(String, String, usize)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::XmlForbiddenCharsRemoved { part, location, count } => {
                Some((part.clone(), location.clone(), *count))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn the_three_issue_inputs_leave_no_forbidden_character_in_any_part() {
    // 이것을 실패시키는 것: `encode_with_diagnostics`(header) 나
    // `encode_section_with_note_counters`(section) 의 `strip_xml_forbidden_chars` 호출을 빼는 것.
    let (parts, _) = encode("함초롬\u{2}돋움", "Nor\u{1}mal", "BO\u{1}DY");

    for (name, text) in &parts {
        let bad: Vec<char> = text.chars().filter(|&c| is_forbidden(c)).collect();
        assert!(bad.is_empty(), "{name} still holds {bad:?}");
    }
    let header = part(&parts, "Contents/header.xml");
    assert!(header.contains(r#"engName="Normal""#), "{header}");
    assert!(header.contains(r#"face="함초롬돋움""#), "{header}");
    assert!(part(&parts, "Contents/section0.xml").contains("<hp:t>BODY</hp:t>"));
}

#[test]
fn each_removal_is_reported_with_its_part_and_location() {
    // 이것을 실패시키는 것: 글꼴 이름 사전 집계(`face_removed`)를 0 으로 두는 것,
    // part 스캔 호출 하나를 빼는 것, 또는 `tally_removed` 의 `count == 0` 가드를 빼는 것.
    let (_, warnings) = encode("함초롬\u{2}돋움", "Nor\u{1}mal", "BO\u{1}DY");

    // `with_default_fonts` registers the face once per language group: seven
    // fonts, one U+0002 each.
    assert_eq!(
        removals(&warnings),
        vec![
            ("Contents/header.xml".to_string(), "hh:font@face".to_string(), 7),
            ("Contents/header.xml".to_string(), "hh:style@engName".to_string(), 1),
            ("Contents/section0.xml".to_string(), "hp:t".to_string(), 1),
        ]
    );
}

#[test]
fn removal_warnings_are_not_semantic_loss() {
    // 이것을 실패시키는 것: `is_semantic_loss` 에서 `XmlForbiddenCharsRemoved` 를 `true` 로 분류하는 것.
    let (_, warnings) = encode("함초롬\u{2}돋움", "Nor\u{1}mal", "BO\u{1}DY");

    assert!(!removals(&warnings).is_empty());
    assert!(!warnings.iter().any(EncodeWarning::is_semantic_loss), "{warnings:?}");
}

#[test]
fn tab_lf_cr_are_not_removed_and_not_reported() {
    // 이것을 실패시키는 것: `is_xml_forbidden_char` 범위에 CR 을 넣는 것 (`'\u{D}'..='\u{1F}'`).
    // TAB·LF 는 본문에서 `<hp:tab/>`·`<hp:lineBreak/>` 가 되고 속성에서는 공백이
    // 되므로(#208) 조립된 part 에 글자 그대로 남지 않는다 — 그 둘의 범위 변이는
    // `wire_xml` 단위 테스트가 잡는다.
    let (parts, warnings) = encode("함초롬돋움", "A\tB\nC\rD", "a\tb\nc\rd");

    assert!(removals(&warnings).is_empty(), "{warnings:?}");
    let section = part(&parts, "Contents/section0.xml");
    assert!(section.contains("<hp:t>a<hp:tab/>b<hp:lineBreak/>c\rd</hp:t>"), "{section}");
    assert!(part(&parts, "Contents/header.xml").contains(r#"engName="A B C D""#));
}

#[test]
fn a_clean_document_raises_no_warning() {
    // 이것을 실패시키는 것: `tally_removed` 의 `count == 0` 가드를 빼는 것 (빈 글꼴 집계가 경고가 된다).
    let (_, warnings) = encode("함초롬돋움", "Normal", "BODY");

    assert!(warnings.is_empty(), "{warnings:?}");
}

// ── parts outside the header and section scans: content.hpf, master pages, charts ──
//
// U+0000 is the probe: the hand-built escape that writes these parts never
// removed it, so before these parts were scanned it reached the part.

#[test]
fn a_chart_part_loses_nul_and_reports_it() {
    // 이것을 실패시키는 것: `PackageWriter::write_hwpx_with_content_hpf` 의 차트 `scan_part` 호출을 빼는 것.
    let mut chart = Control::chart(
        ChartType::Column,
        ChartData::category(&["C\u{0}1"], &[("S\u{0}1", &[1.0])]),
    );
    if let Control::Chart { title, .. } = &mut chart {
        *title = Some("T\u{0}1".to_string());
    }
    let para = Paragraph::with_runs(
        vec![Run::control(chart, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let (parts, warnings) = encode_para(para);

    assert_no_forbidden_char(&parts);
    let chart = part(&parts, "Chart/chart1.xml");
    assert!(chart.contains("<a:t>T1</a:t>"), "{chart}");
    assert!(chart.contains("<c:v>S1</c:v>"), "{chart}");
    assert!(chart.contains("<c:v>C1</c:v>"), "{chart}");
    assert_eq!(
        removals(&warnings),
        vec![
            ("Chart/chart1.xml".to_string(), "a:t".to_string(), 1),
            ("Chart/chart1.xml".to_string(), "c:v".to_string(), 2),
        ]
    );
}

#[test]
fn content_hpf_loses_nul_and_reports_it() {
    // 이것을 실패시키는 것: 인코더가 `render_content_hpf` 직후에 하는 `content.hpf` 스캔을 빼는 것.
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![text_para("body")], PageSettings::a4()));
    doc.set_metadata(Metadata::new().with_title("T\u{0}1").with_author("A\u{0}1"));
    let (parts, warnings) = encode_with(doc, "함초롬돋움", "Normal");

    assert_no_forbidden_char(&parts);
    let hpf = part(&parts, "Contents/content.hpf");
    assert!(hpf.contains("<opf:title>T1</opf:title>"), "{hpf}");
    assert!(hpf.contains(r#"<opf:meta name="creator" content="text">A1</opf:meta>"#), "{hpf}");
    assert_eq!(
        removals(&warnings),
        vec![
            ("Contents/content.hpf".to_string(), "opf:title".to_string(), 1),
            ("Contents/content.hpf".to_string(), "opf:meta".to_string(), 1),
        ]
    );
}

#[test]
fn a_master_page_loses_nul_and_reports_it() {
    // 이것을 실패시키는 것: 바탕쪽 조립(`encoder/section/header_footer.rs`)의 금지 문자 스캔을 빼는 것.
    // 경고 순서가 아니라 포함으로 단언한다 — 바탕쪽을 일반 문단 인코더로 쓰는 쪽에서는
    // 같은 경고가 section 경고들 사이에서 나온다.
    let mut section = Section::with_paragraphs(vec![text_para("body")], PageSettings::a4());
    section.master_pages =
        Some(vec![MasterPage::new(ApplyPageType::Both, vec![text_para("M\u{0}1")])]);
    let mut doc = Document::new();
    doc.add_section(section);
    let (parts, warnings) = encode_with(doc, "함초롬돋움", "Normal");

    assert_no_forbidden_char(&parts);
    assert!(part(&parts, "Contents/masterpage0.xml").contains(">M1</hp:t>"));
    assert!(
        removals(&warnings).contains(&(
            "Contents/masterpage0.xml".to_string(),
            "hp:t".to_string(),
            1
        )),
        "{warnings:?}"
    );
}

// ── preserve-first edits leave the parts they do not touch alone ────
//
// fill and patch rewrite one section through `RawPackage`. Removing forbidden
// characters there is the entry clean-up's job, for the caller's values only;
// a part the edit does not touch keeps its bytes, characters included.

/// Two sections: a click-here field `f` and a plain text in section 0, and
/// a text holding U+0001 in section 1.
fn two_section_base() -> Vec<u8> {
    let mut field = Paragraph::new(ParaShapeIndex::new(0));
    field.runs.push(Run::control(
        Control::Field {
            field_type: FieldType::ClickHere,
            hint_text: Some("hint".to_string()),
            help_text: None,
            name: Some("f".to_string()),
            display_text: "old".to_string(),
        },
        CharShapeIndex::new(0),
    ));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![field, text_para("edit me")],
        PageSettings::a4(),
    ));
    doc.add_section(Section::with_paragraphs(vec![text_para("keep QQQ")], PageSettings::a4()));
    let (bytes, _) = encode_bytes(doc);
    splice_part(&bytes, "Contents/section1.xml", "QQQ", "A\u{1}B")
}

fn encode_bytes(doc: Document) -> (Vec<u8>, Vec<EncodeWarning>) {
    let styles = hwpforge_smithy_hwpx::style_store_for_preset("default").expect("preset");
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles,
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .expect("encode");
    (outcome.bytes, outcome.warnings)
}

/// Replaces `from` with `to` in the part `name` — the only way to get a
/// character the encoder refuses into a base package.
fn splice_part(bytes: &[u8], name: &str, from: &str, to: &str) -> Vec<u8> {
    let mut zin = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for i in 0..zin.len() {
        let mut entry = zin.by_index(i).expect("entry");
        let entry_name = entry.name().to_string();
        let method = entry.compression();
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if entry_name == name {
            let text = String::from_utf8(data).expect("utf-8");
            assert!(text.contains(from), "{from:?} not in {text}");
            data = text.replace(from, to).into_bytes();
        }
        out.start_file(
            entry_name,
            zip::write::SimpleFileOptions::default().compression_method(method),
        )
        .expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

fn raw_part(bytes: &[u8], name: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut entry = zip.by_name(name).expect("part");
    let mut data = Vec::new();
    entry.read_to_end(&mut data).expect("read");
    data
}

#[test]
fn patch_keeps_a_forbidden_character_in_a_section_it_does_not_edit() {
    // 이것을 실패시키는 것: `RawPackage::write` 가 쓰는 part 를 `strip_xml_forbidden_chars` 로 훑는 것.
    let base = two_section_base();
    let mut exported =
        HwpxPatcher::export_section_for_edit(&base, 0, false).expect("export").exported;
    let RunContent::Text(slot) = &mut exported.section.paragraphs[1].runs[0].content else {
        panic!("paragraph 1 is plain text");
    };
    *slot = "edited".to_string();
    let diagnosed = HwpxPatcher::patch_exported_section_with_codec_diagnostics(&base, 0, &exported)
        .expect("patch");

    assert!(String::from_utf8(raw_part(&diagnosed.value.bytes, "Contents/section0.xml"))
        .expect("utf-8")
        .contains(">edited<"));
    assert_eq!(
        raw_part(&diagnosed.value.bytes, "Contents/section1.xml"),
        raw_part(&base, "Contents/section1.xml")
    );
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}

#[test]
fn fill_keeps_a_forbidden_character_in_a_section_it_does_not_edit() {
    // 이것을 실패시키는 것: `RawPackage::write` 가 쓰는 part 를 `strip_xml_forbidden_chars` 로 훑는 것.
    let base = two_section_base();
    let values = BTreeMap::from([("f".to_string(), "new".to_string())]);
    let diagnosed = HwpxFiller::fill_with_codec_diagnostics(&base, &values).expect("fill");

    assert_eq!(diagnosed.value.filled.len(), 1);
    assert_eq!(
        raw_part(&diagnosed.value.bytes, "Contents/section1.xml"),
        raw_part(&base, "Contents/section1.xml")
    );
    assert!(diagnosed.encode_warnings.is_empty(), "{:?}", diagnosed.encode_warnings);
}
