#[path = "support/picture_crop.rs"]
mod shared;
use shared::*;

use std::io::{Cursor, Read};

use hwpforge_core::image::{Image, ImageCrop, ImageFormat, ImageStore};
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::HwpxEncoder;

fn encode_picture(image: Image) -> String {
    let para = Paragraph::with_runs(
        vec![Run::image(image, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    // 이것을 실패시키는 것: 합성 fixture에서 참조하는 원본 글꼴 정의 누락.
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let mut store = ImageStore::new();
    store.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
    let bytes =
        HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &store).expect("encode");
    section_xml(&bytes)
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

fn picture() -> Image {
    Image::new("BinData/image1.png", u(10_625), u(3_612), ImageFormat::Png)
}
