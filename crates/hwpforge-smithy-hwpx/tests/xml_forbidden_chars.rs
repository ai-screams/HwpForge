//! Issue #198: characters XML 1.0 forbids (C0 controls other than TAB, LF and
//! CR, and U+FFFE/U+FFFF) must not reach a serde-written part, and their
//! removal must be reported.
//!
//! The three inputs are the ones the issue reproduced through `from-json`: a
//! style `engName` with U+0001, a font face with U+0002 and body text with
//! U+0001. They go through the public encode path, and the test reads the
//! parts back out of the ZIP.

use std::io::Read;

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyle, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeWarning, HwpxEncoder};

/// Encodes one paragraph of `body` with one style and one font face.
fn encode(face: &str, eng_name: &str, body: &str) -> (Vec<(String, String)>, Vec<EncodeWarning>) {
    let para =
        Paragraph::with_runs(vec![Run::text(body, CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));

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
