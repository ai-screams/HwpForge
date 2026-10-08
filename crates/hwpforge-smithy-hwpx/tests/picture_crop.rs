//! Picture crop, image extent and original box (§19.20 H-v3, S8).
//!
//! The encoder used to write every picture's `orgSz`, `imgRect`, `imgClip`
//! and `imgDim` from its display size, which drops a crop (the whole image
//! shows, shrunk) and the original box. Inputs are repo fixtures and Core
//! pictures built through the API.

use std::io::{Cursor, Read};

use hwpforge_core::image::{Image, ImageCrop, ImageFormat, ImageStore};
use hwpforge_core::run::{Run, RunContent};
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};
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

fn reencode(bytes: &[u8]) -> Vec<u8> {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode(
        &doc.document.validate().expect("validate"),
        &doc.style_store,
        &doc.image_store,
    )
    .expect("encode")
}

/// Each picture's frame elements, in document order: `orgSz`, `imgClip`,
/// `imgDim` (or `-`), and the `scaMatrix` `e1`/`e5` as numbers.
fn frames(xml: &str) -> Vec<(String, String, String, (f64, f64))> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(at) = rest.find("<hp:pic ") {
        let end = at + rest[at..].find("</hp:pic>").expect("pic end");
        let pic = &rest[at..end];
        let element = |tag: &str| {
            pic.find(&format!("<{tag} "))
                .map(|p| pic[p..p + pic[p..].find("/>").unwrap() + 2].to_string())
                .unwrap_or_else(|| "-".to_string())
        };
        let sca = element("hc:scaMatrix");
        let num = |name: &str| attr(&sca, name).parse::<f64>().unwrap();
        out.push((
            element("hp:orgSz"),
            element("hp:imgClip"),
            element("hp:imgDim"),
            (num("e1"), num("e5")),
        ));
        rest = &rest[end..];
    }
    out
}

fn attr<'a>(tag: &'a str, name: &str) -> &'a str {
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle).unwrap_or_else(|| panic!("{name} in {tag}")) + needle.len();
    &tag[at..at + tag[at..].find('"').unwrap()]
}

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

/// Hancom's cropped picture and its uncropped twin (each scaled from a
/// 275220 x 153600 original box) re-encode with the same `orgSz`,
/// `imgClip`, `imgDim` and scale, and decode to the crop in the image's
/// own coordinates.
///
/// 이것을 실패시키는 것: 디코더가 `crop`·`image_extent`·`frame.original_size` 중 하나를
/// 버리는 것 / 인코더가 표시 크기로 되돌려 쓰는 것 / 배율 행렬을 항등으로 두는 것.
#[test]
fn hancom_crop_and_original_box_round_trip() {
    let original = fixture("images/real_crop_vs_original_two_objects.hwpx");
    let doc = HwpxDecoder::decode(&original).expect("decode");
    let images: Vec<&Image> = doc.document.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| match &r.content {
            RunContent::Image(img) => Some(img),
            _ => None,
        })
        .collect();
    assert_eq!(images.len(), 2);
    assert_eq!(images[0].crop, None);
    assert_eq!(images[0].image_extent, Some((u(206_400), u(115_200))));
    assert_eq!(images[0].frame.original_size, Some(Size::new(u(275_220), u(153_600))));
    assert_eq!(
        images[1].crop,
        Some(ImageCrop { left: u(54_464), top: u(21_128), right: u(151_936), bottom: u(115_200) })
    );
    assert_eq!(images[1].image_extent, Some((u(206_400), u(115_200))));

    let before = frames(&section_xml(&original));
    let after = frames(&section_xml(&reencode(&original)));
    assert_eq!(before.len(), 2);
    for (b, a) in before.iter().zip(&after) {
        assert_eq!((&b.0, &b.1, &b.2), (&a.0, &a.1, &a.2));
        assert!((b.3 .0 - a.3 .0).abs() < 1e-6 && (b.3 .1 - a.3 .1).abs() < 1e-6, "{b:?} {a:?}");
    }
}

fn encode_picture(image: Image) -> String {
    let para = Paragraph::with_runs(
        vec![Run::image(image, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    let mut styles = HwpxStyleStore::new();
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let mut store = ImageStore::new();
    store.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &store).expect("encode");
    section_xml(&bytes)
}

fn picture() -> Image {
    Image::new("BinData/image1.png", u(10_625), u(3_612), ImageFormat::Png)
}

/// What the encoder writes for each combination of the three fields:
/// none set = the display size everywhere (today's bytes); an extent alone
/// = the whole extent clipped; a crop with its extent = both as given; a
/// crop without an extent = no `imgDim` (as Hancom writes such a picture);
/// an original box = `orgSz`/`imgRect` from it and the display / original
/// scale, 6 decimals — mnd1's image1 (10625 x 3612 from 84180 x 59580) is
/// Hancom's `0.126218` / `0.060624`.
///
/// 이것을 실패시키는 것: 자르기만 있을 때 `imgDim` 을 쓰는 것 / 범위를 무시하고
/// 표시 크기로 자르기 전체를 쓰는 것 / `imgRect` 를 표시 크기로 두는 것 / 배율을
/// original / display 로 뒤집는 것.
#[test]
fn encoder_writes_each_frame_state() {
    let none = encode_picture(picture());
    let f = &frames(&none)[0];
    assert_eq!(f.0, r#"<hp:orgSz width="10625" height="3612"/>"#);
    assert_eq!(f.1, r#"<hp:imgClip left="0" right="10625" top="0" bottom="3612"/>"#);
    assert_eq!(f.2, r#"<hp:imgDim dimwidth="10625" dimheight="3612"/>"#);
    assert_eq!(f.3, (1.0, 1.0));

    let mut extent = picture();
    extent.image_extent = Some((u(263_100), u(186_060)));
    let f = &frames(&encode_picture(extent.clone()))[0];
    assert_eq!(f.1, r#"<hp:imgClip left="0" right="263100" top="0" bottom="186060"/>"#);
    assert_eq!(f.2, r#"<hp:imgDim dimwidth="263100" dimheight="186060"/>"#);

    let crop = ImageCrop { left: u(42_962), top: u(55_462), right: u(222_595), bottom: u(138_334) };
    let mut cropped = extent;
    cropped.crop = Some(crop);
    let f = &frames(&encode_picture(cropped))[0];
    assert_eq!(f.1, r#"<hp:imgClip left="42962" right="222595" top="55462" bottom="138334"/>"#);
    assert_eq!(f.2, r#"<hp:imgDim dimwidth="263100" dimheight="186060"/>"#);

    let mut no_extent = picture();
    no_extent.crop = Some(crop);
    let f = &frames(&encode_picture(no_extent))[0];
    assert_eq!(f.1, r#"<hp:imgClip left="42962" right="222595" top="55462" bottom="138334"/>"#);
    assert_eq!(f.2, "-");

    let mut scaled = picture();
    scaled.frame.original_size = Some(Size::new(u(84_180), u(59_580)));
    let xml = encode_picture(scaled);
    let f = &frames(&xml)[0];
    assert_eq!(f.0, r#"<hp:orgSz width="84180" height="59580"/>"#);
    assert!(xml.contains(r#"<hc:pt2 x="84180" y="59580"/>"#), "{xml}");
    assert!(
        xml.contains(r#"<hc:scaMatrix e1="0.126218" e2="0" e3="0" e4="0" e5="0.060624" e6="0"/>"#),
        "{xml}"
    );
    assert!(xml.contains(r#"<hp:curSz width="10625" height="3612"/>"#), "{xml}");
}
