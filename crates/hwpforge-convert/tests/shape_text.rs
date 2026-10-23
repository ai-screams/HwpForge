//! HWP5 ellipse and polygon text (§19.5 1a, review r1).
//!
//! `full_report.hwp` section 2 holds an ellipse with the text "BGR!" and a
//! polygon with "주의!", each with a caption ("[그림 2] …", "[그림 3] …").
//! Before, both shapes were written without their text; carrying it then
//! showed that a body shape's caption paragraphs were read as its text.

use std::io::{Cursor, Read};
use std::path::PathBuf;

use hwpforge_convert::{hwp5_to_hwpx_bytes_with_diagnostics, ConvertOptions, ConvertWarning};
use hwpforge_core::control::Control;
use hwpforge_core::run::RunContent;
use hwpforge_core::Paragraph;
use hwpforge_smithy_hwp5::Hwp5Warning;
use hwpforge_smithy_hwpx::HwpxDecoder;

fn fixture(name: &str) -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn section_xml(hwpx: &[u8], index: usize) -> String {
    let mut archive = zip::ZipArchive::new(Cursor::new(hwpx)).expect("zip");
    let mut xml = String::new();
    archive
        .by_name(&format!("Contents/section{index}.xml"))
        .expect("section")
        .read_to_string(&mut xml)
        .unwrap();
    xml
}

/// The text inside each `<hp:{tag}>` element.
fn shape_texts(xml: &str, tag: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find(&format!("<hp:{tag} ")) {
        let end = at + rest[at..].find(&format!("</hp:{tag}>")).expect("end");
        let body = &rest[at..end];
        let text: String =
            body.split("<hp:t>").skip(1).map(|t| t.split('<').next().unwrap_or("")).collect();
        out.push(text);
        rest = &rest[end..];
    }
    out
}

/// The ellipse and polygon keep their own text — and only it, not their
/// captions — in the HWPX and in Core, with and without the layout cache
/// (the shapes' paragraphs take their line-segment hints). The captions are
/// reported, not dropped silently.
///
/// 이것을 실패시키는 것: 타원·다각형에 글을 붙이지 않기 / 캡션 문단을 도형 글에서
/// 떼어내지 않기 / layout hint 수집에서 도형 글을 빼기(캐시 켠 변환 실패) /
/// 캡션 손실 보고를 지우기.
#[test]
fn ellipse_and_polygon_keep_their_text() {
    for carry in [false, true] {
        let (hwpx, warnings) = hwp5_to_hwpx_bytes_with_diagnostics(
            &fixture("mixed/full_report.hwp"),
            ConvertOptions::default().with_carry_layout_cache(carry),
        )
        .unwrap_or_else(|e| panic!("carry={carry}: {e}"));
        let xml = section_xml(&hwpx, 2);
        assert_eq!(shape_texts(&xml, "ellipse"), ["BGR!"], "carry={carry}");
        assert_eq!(shape_texts(&xml, "polygon"), ["주의!"], "carry={carry}");
        assert!(
            warnings.iter().any(|w| matches!(
                w,
                ConvertWarning::Hwp5(Hwp5Warning::DroppedControl { control: "caption", .. })
            )),
            "{warnings:?}"
        );

        let doc = HwpxDecoder::decode(&hwpx).expect("decode");
        let mut texts = Vec::new();
        for para in &doc.document.sections()[2].paragraphs {
            for run in &para.runs {
                if let RunContent::Control(control) = &run.content {
                    if let Control::Ellipse { paragraphs, .. }
                    | Control::Polygon { paragraphs, .. } = control.as_ref()
                    {
                        texts.push(
                            paragraphs.iter().map(Paragraph::text_content).collect::<String>(),
                        );
                    }
                }
            }
        }
        assert_eq!(texts, ["BGR!", "주의!"], "carry={carry}");
    }
}
