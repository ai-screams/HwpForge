//! Text box inner margin (§19.22 D4, S16).
//!
//! Hancom writes a drawn shape's text inset on `<hp:drawText><hp:textMargin>`
//! (911 text boxes in 274 corpus documents differ from 283). The decoder did
//! not read it and the encoder wrote 283 on every side. Inputs are repo
//! fixtures with one margin rewritten.

use std::io::{Cursor, Read, Write};

use hwpforge_core::control::Control;
use hwpforge_core::run::RunContent;
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

const DEFAULT: &str = r#"<hp:textMargin left="283" right="283" top="283" bottom="283"/>"#;
const ODD: &str = r#"<hp:textMargin left="850" right="700" top="566" bottom="400"/>"#;

fn text_box_margins(bytes: &[u8]) -> Vec<Option<[i32; 4]>> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let mut out = Vec::new();
    for p in &doc.document.sections()[0].paragraphs {
        for run in &p.runs {
            if let RunContent::Control(c) = &run.content {
                if let Control::TextBox { text_margin, .. } = c.as_ref() {
                    out.push(text_margin.map(|m| {
                        [m.left.as_i32(), m.right.as_i32(), m.top.as_i32(), m.bottom.as_i32()]
                    }));
                }
            }
        }
    }
    out
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

/// A text box keeps its own (asymmetric) margin through decode and
/// re-encode; the others keep 283, carried as the wire value.
///
/// 이것을 실패시키는 것: 디코더가 읽지 않기 / 인코더 상수 283 / 왼·오, 위·아래
/// 맞바꿈(값이 모두 달라 드러남).
#[test]
fn shape_text_margins_round_trip() {
    let source = fixture("shapes/textbox.hwpx");
    let xml = section_xml(&source);
    assert_eq!(xml.matches(DEFAULT).count(), 4, "fixture margins");
    let input = with_section(&source, &xml.replacen(DEFAULT, ODD, 1));
    let margins = text_box_margins(&input);
    assert_eq!(margins[0], Some([850, 700, 566, 400]));
    assert!(margins[1..].iter().all(|m| *m == Some([283; 4])), "{margins:?}");
    let out = section_xml(&reencode(&input));
    assert_eq!(out.matches(ODD).count(), 1, "{out}");
    assert_eq!(out.matches(DEFAULT).count(), 3);
}

/// An ellipse's text margin other than 283 is reported (Core has no place for
/// it), not refused; 283 is not reported.
///
/// 이것을 실패시키는 것: 이 보고를 지우기.
#[test]
fn ellipse_text_margin_is_reported() {
    let source = fixture("shapes/group-native.hwpx");
    let xml = section_xml(&source);
    let at = xml.find("<hp:ellipse ").expect("ellipse");
    let rel = xml[at..].find(DEFAULT).expect("ellipse margin");
    let edited = format!("{}{}{}", &xml[..at + rel], ODD, &xml[at + rel + DEFAULT.len()..]);
    let reported = |bytes: &[u8]| {
        HwpxDecoder::decode(bytes).expect("decode").warnings.iter().any(|w| {
            matches!(
                w,
                DecodeWarning::UnknownEnumValue { attribute: "hp:drawText/hp:textMargin", .. }
            )
        })
    };
    assert!(reported(&with_section(&source, &edited)));
    assert!(!reported(&source));
}

/// A polygon's text margin other than 283 is reported too: a polygon with
/// text, encoded, then given a margin on the wire.
///
/// 이것을 실패시키는 것: 다각형 쪽 보고를 지우기.
#[test]
fn polygon_text_margin_is_reported() {
    use hwpforge_core::control::ShapePoint;
    use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
    let mut polygon = Control::polygon(vec![
        ShapePoint::new(0, 0),
        ShapePoint::new(4000, 0),
        ShapePoint::new(2000, 3000),
    ])
    .expect("polygon");
    if let Control::Polygon { paragraphs, .. } = &mut polygon {
        *paragraphs = vec![Paragraph::with_runs(
            vec![Run::text("다각형", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )];
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(polygon, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &{
            // 이것을 실패시키는 것: 합성 fixture의 원본 charPr/paraPr 0 정의 누락.
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(hwpforge_smithy_hwpx::HwpxCharShape::default());
            store.push_para_shape(hwpforge_smithy_hwpx::HwpxParaShape::default());
            store
        },
        &Default::default(),
    )
    .expect("encode");
    let xml = section_xml(&bytes);
    let at = xml.find("<hp:polygon ").expect("polygon");
    let end = at + xml[at..].find("</hp:subList>").expect("sublist") + "</hp:subList>".len();
    let with =
        |margin: &str| with_section(&bytes, &format!("{}{}{}", &xml[..end], margin, &xml[end..]));
    let reported = |bytes: &[u8]| {
        HwpxDecoder::decode(bytes).expect("decode").warnings.iter().any(|w| {
            matches!(
                w,
                DecodeWarning::UnknownEnumValue { attribute: "hp:drawText/hp:textMargin", .. }
            )
        })
    };
    assert!(reported(&with(ODD)));
    assert!(!reported(&with(DEFAULT)));
}

/// A text box built without a margin (old JSON, the API's constructor)
/// writes 283 on every side, as before the field existed.
///
/// 이것을 실패시키는 것: `None` 을 0 으로 쓰기.
#[test]
fn text_box_without_a_margin_writes_283() {
    use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section};
    use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
    use hwpforge_smithy_hwpx::style_store::HwpxStyleStore;
    let unit = HwpUnit::new(3000).unwrap();
    let inner =
        Paragraph::with_runs(vec![Run::text("글", CharShapeIndex::new(0))], ParaShapeIndex::new(0));
    let tb = Control::text_box(vec![inner], unit, unit);
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::control(tb, CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(
        &doc.validate().expect("validate"),
        &{
            // 이것을 실패시키는 것: 합성 fixture의 원본 charPr/paraPr 0 정의 누락.
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(hwpforge_smithy_hwpx::HwpxCharShape::default());
            store.push_para_shape(hwpforge_smithy_hwpx::HwpxParaShape::default());
            store
        },
        &Default::default(),
    )
    .expect("encode");
    assert_eq!(section_xml(&bytes).matches(DEFAULT).count(), 1);
}

/// A text box margin with a side outside the HWPUNIT range is refused, not
/// read as "no margin" (which re-encodes as 283 and skips the PDF and
/// Markdown refusals).
///
/// 이것을 실패시키는 것: 범위 밖 한 변을 `None` 으로 접기.
#[test]
fn out_of_range_text_margin_is_refused() {
    let source = fixture("shapes/textbox.hwpx");
    let xml = section_xml(&source);
    let bad = r#"<hp:textMargin left="283" right="283" top="283" bottom="2000000000"/>"#;
    let input = with_section(&source, &xml.replacen(DEFAULT, bad, 1));
    assert!(HwpxDecoder::decode(&input).is_err());
}
