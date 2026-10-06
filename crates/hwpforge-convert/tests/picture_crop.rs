//! HWP5 picture crop, image extent and original box (§19.20 H-v3, S8),
//! against Hancom's HWPX save of the same document.

use std::path::PathBuf;

use hwpforge_core::image::Image;
use hwpforge_core::run::RunContent;
use hwpforge_smithy_hwp5::decode_hwp5_with_images;
use hwpforge_smithy_hwpx::HwpxDecoder;

fn fixture(name: &str) -> Vec<u8> {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/images").join(name);
    std::fs::read(&path).unwrap_or_else(|err| panic!("read {}: {err}", path.display()))
}

fn pictures(doc: &hwpforge_core::document::Document<hwpforge_core::document::Draft>) -> Vec<Image> {
    doc.sections()[0]
        .paragraphs
        .iter()
        .flat_map(|p| &p.runs)
        .filter_map(|r| match &r.content {
            RunContent::Image(img) => Some(img.clone()),
            _ => None,
        })
        .collect()
}

/// The cropped picture and its uncropped twin (bytes 44–60 crop, 82–90
/// image size, the component's initial box) carry the same crop, image
/// extent and original box as Hancom's HWPX save gives the HWPX decoder.
///
/// 이것을 실패시키는 것: HWP5 투영이 자르기·범위·원래 크기 중 하나를 버리는 것 /
/// `ShapeComponent` 초기 크기 대신 표시 크기를 쓰는 것.
#[test]
fn hwp5_crop_matches_the_hancom_hwpx_twin() {
    let ours = decode_hwp5_with_images(&fixture("real_crop_vs_original_two_objects.hwp")).unwrap();
    let twin = HwpxDecoder::decode(&fixture("real_crop_vs_original_two_objects.hwpx")).unwrap();
    let (a, b) = (pictures(&ours.document), pictures(&twin.document));
    assert_eq!(a.len(), 2);
    for (x, y) in a.iter().zip(&b) {
        assert_eq!(
            (x.crop, x.image_extent, x.original_size),
            (y.crop, y.image_extent, y.original_size)
        );
    }
    assert!(a[1].crop.is_some() && a[0].original_size.is_some(), "{a:?}");
}
