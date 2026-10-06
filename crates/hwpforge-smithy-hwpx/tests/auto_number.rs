//! Caption numbers (`<hp:autoNum numType="PICTURE|TABLE|EQUATION">`,
//! §19.16 J, S12). Before, the decoder dropped them and a re-encode lost
//! "그림 3"'s number. F17 is Hancom's file with two picture captions and one
//! table caption.

use std::io::{Cursor, Read, Write};

use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, Control};
use hwpforge_foundation::NumberFormatType;
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder};

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
        let name = entry.name().to_string();
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
