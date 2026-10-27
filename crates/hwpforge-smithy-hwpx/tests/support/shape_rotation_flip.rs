#[path = "encoding.rs"]
mod encoding;

use super::*;

pub(super) fn section0(bytes: &[u8]) -> String {
    PackageReader::new(bytes).expect("package").read_section_xml(0).expect("section0.xml")
}

/// `(rotationInfo angle, flip h, flip v, rotMatrix e1..e6)` of every shape's
/// own common block, in document order.
pub(super) fn wire_transforms(xml: &str) -> Vec<(String, String, String, [String; 6])> {
    let attr = |tag: &str, name: &str| -> String {
        let v = tag.split(&format!(" {name}=\"")).nth(1).expect(name);
        v[..v.find('"').unwrap()].to_string()
    };
    let mut out = Vec::new();
    for (start, _) in xml.match_indices("<hp:rotationInfo ") {
        let rest = &xml[start..];
        let ri = &rest[..rest.find("/>").unwrap()];
        let before = &xml[..start];
        let flip_at = before.rfind("<hp:flip ").unwrap();
        let flip = &xml[flip_at..flip_at + xml[flip_at..].find("/>").unwrap()];
        let rot_at = rest.find("<hc:rotMatrix ").unwrap();
        let rot = &rest[rot_at..rot_at + rest[rot_at..].find("/>").unwrap()];
        out.push((
            attr(ri, "angle"),
            attr(flip, "horizontal"),
            attr(flip, "vertical"),
            ["e1", "e2", "e3", "e4", "e5", "e6"].map(|e| attr(rot, e)),
        ));
    }
    out
}

pub(super) fn styles() -> HwpxStyleStore {
    encoding::dotum_styles()
}

pub(super) fn rect(w: i32, h: i32, rotation: f32, flip: Option<Flip>) -> Control {
    let mut c = Control::rect(HwpUnit::new(w).unwrap(), HwpUnit::new(h).unwrap()).unwrap();
    if let Control::Rect { style, .. } = &mut c {
        *style = Some(ShapeStyle { rotation: Some(rotation), flip, ..ShapeStyle::default() });
    }
    c
}

pub(super) fn encode_one(shape: Control) -> (String, Vec<EncodeWarning>) {
    let mut host = Paragraph::new(ParaShapeIndex::new(0));
    host.add_run(Run::control(shape, CharShapeIndex::new(0)));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![host], PageSettings::a4()));
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .expect("encode");
    (section0(&outcome.bytes), outcome.warnings)
}
