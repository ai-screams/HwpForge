//! Caption numbers (`<hp:autoNum numType="PICTURE|TABLE|EQUATION">`,
//! §19.16 J, S12). Before, the decoder dropped them and a re-encode lost
//! "그림 3"'s number. F17 is Hancom's file with two picture captions and one
//! table caption.

use std::io::{Cursor, Read, Write};

use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, Control};
use hwpforge_foundation::NumberFormatType;
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder, HwpxPatcher};

fn fixture(path: &str) -> Vec<u8> {
    let path = format!("{}/../../tests/fixtures/{path}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn section_xml(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/section0.xml").expect("section0").read_to_string(&mut out).unwrap();
    out
}

fn with_section(bytes: &[u8], xml: &str) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        out.start_file(name.as_str(), options).expect("start");
        let body = if name == "Contents/section0.xml" { xml.as_bytes() } else { &data };
        out.write_all(body).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// Every `Control::AutoNumber` in the document, in document order (the
/// JSON of the document holds them wherever they sit — captions included).
fn auto_numbers(bytes: &[u8]) -> (Vec<Control>, Vec<DecodeWarning>) {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let json = serde_json::to_value(&doc.document).unwrap();
    let mut out = Vec::new();
    collect(&json, &mut out);
    (out, doc.warnings)
}

fn collect(v: &serde_json::Value, out: &mut Vec<Control>) {
    match v {
        serde_json::Value::Object(map) => {
            if map.contains_key("AutoNumber") {
                out.push(serde_json::from_value(v.clone()).unwrap());
                return;
            }
            map.values().for_each(|x| collect(x, out));
        }
        serde_json::Value::Array(items) => items.iter().for_each(|x| collect(x, out)),
        _ => {}
    }
}

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

/// F17: the picture captions 3 and 4 and the table caption 7 decode with
/// Hancom's cached numbers and default format, and a re-encode writes the
/// same three `<hp:autoNum>` elements.
///
/// 이것을 실패시키는 것: 디코더가 PICTURE·TABLE 을 버리는 것 / `num` 을 1 로
/// 쓰는 것 / 인코더가 `AutoNumber` 를 건너뛰는 것.
#[test]
fn f17_caption_numbers_round_trip() {
    let original = fixture("images/f17-caption-numbering.hwpx");
    let (numbers, warnings) = auto_numbers(&original);
    let digit = AutoNumberFormat::default();
    assert_eq!(
        numbers,
        [
            Control::AutoNumber { kind: AutoNumberKind::Picture, num: 3, format: digit },
            Control::AutoNumber { kind: AutoNumberKind::Picture, num: 4, format: digit },
            Control::AutoNumber { kind: AutoNumberKind::Table, num: 7, format: digit },
        ]
    );
    assert!(
        !warnings.iter().any(|w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind.starts_with("auto_number"))),
        "{warnings:?}"
    );
    let xml = section_xml(&reencode(&original));
    for (num, kind) in [(3, "PICTURE"), (4, "PICTURE"), (7, "TABLE")] {
        let element = format!(
            r#"<hp:autoNum num="{num}" numType="{kind}"><hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/></hp:autoNum>"#
        );
        assert!(xml.contains(&element), "{element}");
    }
    assert_eq!(auto_numbers(&reencode(&original)).0, numbers);
}

/// A non-default format (Roman capitals in parentheses, superscript)
/// survives decode and re-encode.
///
/// 이것을 실패시키는 것: 접두·접미 문자를 버리는 것 / `supscript` 를 무시하는 것 /
/// 번호 모양을 DIGIT 으로 쓰는 것.
#[test]
fn non_default_format_round_trips() {
    let original = fixture("images/f17-caption-numbering.hwpx");
    let xml = section_xml(&original);
    let default_format =
        r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#;
    let roman = r#"<hp:autoNumFormat type="ROMAN_CAPITAL" userChar="" prefixChar="(" suffixChar=")" supscript="1"/>"#;
    let input = with_section(&original, &xml.replacen(default_format, roman, 1));
    let (numbers, _) = auto_numbers(&input);
    let format = AutoNumberFormat {
        number_type: NumberFormatType::RomanCapital,
        user_char: None,
        prefix: Some('('),
        suffix: Some(')'),
        superscript: true,
    };
    assert_eq!(numbers[0], Control::AutoNumber { kind: AutoNumberKind::Picture, num: 3, format });
    assert!(section_xml(&reencode(&input)).contains(roman));
}

/// A format or kind Core cannot hold is reported lost, not written as the
/// default: an unknown `type`, a two-character prefix, a `supscript` that is
/// not 0/1, an unknown `numType`.
///
/// 이것을 실패시키는 것: 미지 `type` 을 DIGIT 으로 접는 것 / 두 글자 접두를 첫
/// 글자로 자르는 것 / `supscript` 미지값을 0 으로 / 미지 numType 을 조용히 버리는 것.
#[test]
fn unknown_format_or_kind_is_reported_lost() {
    let original = fixture("images/f17-caption-numbering.hwpx");
    let xml = section_xml(&original);
    let default_format =
        r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#;
    for (from, to, kind) in [
        (
            default_format,
            r#"<hp:autoNumFormat type="BOGUS" userChar="" prefixChar="" suffixChar="" supscript="0"/>"#,
            "auto_number_format",
        ),
        (
            default_format,
            r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="((" suffixChar="" supscript="0"/>"#,
            "auto_number_format",
        ),
        (
            default_format,
            r#"<hp:autoNumFormat type="DIGIT" userChar="" prefixChar="" suffixChar="" supscript="2"/>"#,
            "auto_number_format",
        ),
        (r#"numType="PICTURE""#, r#"numType="BOGUS""#, "auto_number"),
    ] {
        let (numbers, warnings) =
            auto_numbers(&with_section(&original, &xml.replacen(from, to, 1)));
        assert_eq!(numbers.len(), 2, "{to}");
        let lost: Vec<&str> = warnings
            .iter()
            .filter_map(|w| match w {
                DecodeWarning::ContentLost { kind, .. } => Some(kind.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(lost, [kind], "{to}");
    }
}

/// A text edit after a caption number goes through the preserving patch
/// path (review r1): the run counter now advances over the number's run as
/// the decoder makes it, so the slot after "그림 3" is found — before, the
/// slots after a caption number were shifted and the edit failed. The
/// numbers stay 3·4·7.
///
/// 이것을 실패시키는 것: patch 의 캡션 번호 전진을 지우기.
#[test]
fn caption_text_after_its_number_is_patched() {
    use hwpforge_core::run::RunContent;
    let base = fixture("images/f17-caption-numbering.hwpx");
    let decoded = HwpxDecoder::decode(&base).expect("decode");
    let original = decoded.document.sections()[0].clone();
    let mut replacement = original.clone();
    let mut edited = false;
    'find: for para in &mut replacement.paragraphs {
        for run in &mut para.runs {
            if let RunContent::Image(img) = &mut run.content {
                if let Some(caption) = img.caption.as_mut() {
                    let runs = &mut caption.paragraphs[0].runs;
                    let after = runs
                        .iter()
                        .position(|r| matches!(&r.content, RunContent::Control(c) if matches!(**c, Control::AutoNumber { .. })))
                        .expect("number")
                        + 1;
                    let RunContent::Text(text) = &mut runs[after].content else {
                        panic!("{:?}", runs[after])
                    };
                    *text = " 조직도".to_string();
                    edited = true;
                    break 'find;
                }
            }
        }
    }
    assert!(edited);
    let preservation =
        HwpxPatcher::export_section_preservation(&base, 0, &original).expect("preservation");
    let patched =
        HwpxPatcher::patch_section_preserving(&base, 0, &replacement, None, Some(&preservation))
            .expect("text edit after a caption number");
    assert!(section_xml(&patched).contains("<hp:t> 조직도</hp:t>"));
    let numbers: Vec<u32> = auto_numbers(&patched)
        .0
        .iter()
        .map(|c| match c {
            Control::AutoNumber { num, .. } => *num,
            _ => unreachable!(),
        })
        .collect();
    assert_eq!(numbers, [3, 4, 7]);
}

/// 이것을 실패시키는 것: pageNum 슬롯 전진을 각주·미주 처리 뒤로 이동하거나 삭제.
/// XML 자식 순서 양쪽에서 실제 Core run 경로로 주석 본문을 보존 편집한다.
#[test]
fn r1_page_number_and_note_in_one_ctrl_preserve_the_same_run_order() {
    use hwpforge_core::run::RunContent;
    let original = fixture("user_samples/sample-field-footnote.hwpx");
    let original_xml = section_xml(&original);
    let page = r#"<hp:pageNum pos="BOTTOM_CENTER" formatType="DIGIT" sideChar="-"/>"#;
    for endnote in [false, true] {
        let note_tag = if endnote { "endNote" } else { "footNote" };
        let xml = if endnote {
            original_xml
                .replace("<hp:footNote ", "<hp:endNote ")
                .replace("</hp:footNote>", "</hp:endNote>")
        } else {
            original_xml.clone()
        };
        for page_first in [false, true] {
            let xml = if page_first {
                xml.replacen(&format!("<hp:{note_tag} "), &format!("{page}<hp:{note_tag} "), 1)
            } else {
                xml.replacen(&format!("</hp:{note_tag}>"), &format!("</hp:{note_tag}>{page}"), 1)
            };
            let base = with_section(&original, &xml);
            let decoded = HwpxDecoder::decode(&base).unwrap();
            let section = decoded.document.sections()[0].clone();
            let mut replacement = section.clone();
            let mut edited = false;
            for paragraph in &mut replacement.paragraphs {
                let Some(page_index) = paragraph.runs.iter().position(|r| matches!(&r.content, RunContent::Control(c) if matches!(**c, Control::PageNumber { .. }))) else { continue; };
                let RunContent::Control(note) = &mut paragraph.runs[page_index + 1].content else {
                    panic!("note follows page run")
                };
                let paragraphs = match &mut **note {
                    Control::Footnote { paragraphs, .. } if !endnote => paragraphs,
                    Control::Endnote { paragraphs, .. } if endnote => paragraphs,
                    other => panic!("wrong run order: {other:?}"),
                };
                let text = paragraphs
                    .iter_mut()
                    .flat_map(|p| &mut p.runs)
                    .find_map(|r| match &mut r.content {
                        RunContent::Text(t) if !t.is_empty() => Some(t),
                        _ => None,
                    })
                    .expect("note text");
                *text = "쪽번호 뒤 주석 수정".to_string();
                edited = true;
            }
            assert!(edited);
            let preservation =
                HwpxPatcher::export_section_preservation(&base, 0, &section).unwrap();
            let patched = HwpxPatcher::patch_section_preserving(
                &base,
                0,
                &replacement,
                None,
                Some(&preservation),
            )
            .unwrap();
            let output = section_xml(&patched);
            assert!(output.contains("쪽번호 뒤 주석 수정"));
            assert!(output.contains(page));
            let reparsed = HwpxDecoder::decode(&patched).unwrap();
            // Editing invalidates layout caches; compare the semantic tree.
            fn without_cache(value: &mut serde_json::Value) {
                match value {
                    serde_json::Value::Object(map) => {
                        map.remove("layout_cache");
                        map.values_mut().for_each(without_cache);
                    }
                    serde_json::Value::Array(items) => items.iter_mut().for_each(without_cache),
                    _ => {}
                }
            }
            let mut expected = serde_json::to_value(&replacement).unwrap();
            let mut actual = serde_json::to_value(&reparsed.document.sections()[0]).unwrap();
            without_cache(&mut expected);
            without_cache(&mut actual);
            assert_eq!(actual["paragraphs"], expected["paragraphs"]);
            let mut before = zip::ZipArchive::new(Cursor::new(&base)).unwrap();
            let mut after = zip::ZipArchive::new(Cursor::new(&patched)).unwrap();
            for i in 0..before.len() {
                let mut entry = before.by_index(i).unwrap();
                let name = entry.name().expect("ZIP entry name").into_owned();
                if name == "Contents/section0.xml" {
                    continue;
                }
                let mut a = Vec::new();
                let mut b = Vec::new();
                entry.read_to_end(&mut a).unwrap();
                after.by_name(&name).unwrap().read_to_end(&mut b).unwrap();
                assert_eq!(a, b, "untouched entry {name}");
            }
        }
    }
}
