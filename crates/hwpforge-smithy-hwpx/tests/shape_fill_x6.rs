//! Shape fills against Hancom's x6 fixture (§19.18): five rectangles, top to
//! bottom — F1 colour + hatch, F2 colour, F3 linear gradient, F4 conical
//! gradient with centre (30, 20), 100 steps, step centre 70, F5 picture fill.

use std::io::{Cursor, Read};

use hwpforge_core::control::{Control, Fill, ShapeStyle};
use hwpforge_core::image::ImageStore;
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, Color, GradientType, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder};

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

/// The `<hc:fillBrush>…</hc:fillBrush>` of each shape, in order.
fn fill_brushes(xml: &str) -> Vec<String> {
    xml.match_indices("<hc:fillBrush>")
        .map(|(at, _)| {
            let end = at + xml[at..].find("</hc:fillBrush>").expect("end");
            xml[at..end].to_string()
        })
        .collect()
}

fn shape_fills(doc: &Document) -> Vec<Option<Fill>> {
    let mut out = Vec::new();
    doc.sections()[0].for_each_paragraph(|p| {
        for run in &p.runs {
            if let RunContent::Control(control) = &run.content {
                if let Control::Rect { style, .. } | Control::TextBox { style, .. } =
                    control.as_ref()
                {
                    out.push(style.as_ref().and_then(|s| s.fill.clone()));
                }
            }
        }
    });
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

/// S13: the decoder keeps the wire centre, steps and step centre as `Some`
/// (F3's are the encoder defaults and still `Some`), and re-encoding writes
/// them back. 이것을 실패시키는 것: 기본값과 같은 값을 `None` 으로 접기(F3) /
/// 인코더가 S13 값을 무시하고 고정값을 쓰기(F4 `centerX="50"`).
#[test]
fn gradient_center_and_steps_round_trip() {
    let source = fixture("shapes/x6-fill.hwpx");
    let doc = HwpxDecoder::decode(&source).expect("decode");
    let gradients: Vec<_> = shape_fills(&doc.document)
        .into_iter()
        .filter_map(|f| match f {
            Some(Fill::Gradient { center_x, center_y, step, step_center, .. }) => {
                Some((center_x, center_y, step, step_center))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        gradients,
        [(Some(0), Some(0), Some(255), Some(50)), (Some(30), Some(20), Some(100), Some(70))]
    );
    let ours = fill_brushes(&section_xml(&reencode(&source)));
    let hancom = fill_brushes(&section_xml(&source));
    let gradation = |b: &Vec<String>| -> Vec<String> {
        b.iter()
            .filter_map(|x| {
                let at = x.find("<hc:gradation ")?;
                Some(x[at..at + x[at..].find('>').unwrap()].to_string())
            })
            .collect()
    };
    assert_eq!(gradation(&hancom).len(), 2);
    assert_eq!(gradation(&ours), gradation(&hancom));
}

fn gradient_doc(gradient_type: GradientType) -> Vec<u8> {
    let style = ShapeStyle {
        fill: Some(Fill::Gradient {
            gradient_type,
            angle: 0,
            colors: vec![(Color::from_rgb(255, 0, 0), 0), (Color::from_rgb(0, 0, 255), 100)],
            center_x: None,
            center_y: None,
            step: None,
            step_center: None,
        }),
        ..ShapeStyle::default()
    };
    let rect = Control::Rect {
        width: HwpUnit::new(1000).unwrap(),
        height: HwpUnit::new(1000).unwrap(),
        placement: None,
        caption: None,
        style: Some(style),
    };
    let para = Paragraph::with_runs(
        vec![Run::control(rect, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let mut styles = HwpxStyleStore::new();
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
        .expect("encode")
}

/// S13 `None` = what the encoder always wrote: LINEAR (0, 0), the other
/// kinds (50, 50), 255 steps, step centre 50 — old JSON and API gradients
/// keep their bytes. 이것을 실패시키는 것: `None` 을 0 으로 쓰기(RADIAL 이 0,0).
#[test]
fn gradient_without_center_keeps_the_per_kind_default() {
    let attrs = |bytes: Vec<u8>| {
        let xml = section_xml(&bytes);
        let at = xml.find("<hc:gradation ").expect("gradation");
        xml[at..at + xml[at..].find('>').unwrap()].to_string()
    };
    let radial = attrs(gradient_doc(GradientType::Radial));
    assert!(
        radial.contains(r#"centerX="50" centerY="50" step="255""#)
            && radial.contains(r#"stepCenter="50""#),
        "{radial}"
    );
    let linear = attrs(gradient_doc(GradientType::Linear));
    assert!(linear.contains(r#"centerX="0" centerY="0""#), "{linear}");
}
