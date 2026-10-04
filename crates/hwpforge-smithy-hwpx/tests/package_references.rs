//! C4: the encoder checks the package it writes — manifest ids two items
//! share, and `binaryItemIDRef` values no image or OLE item resolves. The
//! checks only report; the bytes stay as they are.

use std::io::Read;

use hwpforge_core::control::Control;
use hwpforge_core::image::{Image, ImageStore};
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeOutcome, EncodeWarning, HwpxDecoder, HwpxEncoder};

fn styles() -> HwpxStyleStore {
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬바탕");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    styles
}

fn encode_runs(runs: Vec<Run>, images: &ImageStore) -> EncodeOutcome {
    let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        images,
        EncodeOptions::default(),
    )
    .expect("encode")
}

fn part(bytes: &[u8], name: &str) -> String {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).expect("zip");
    let mut text = String::new();
    zip.by_name(name).expect("part").read_to_string(&mut text).expect("utf-8");
    text
}

fn conflicts(warnings: &[EncodeWarning]) -> Vec<(String, Vec<String>)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::PackageIdConflict { id, hrefs } => Some((id.clone(), hrefs.clone())),
            _ => None,
        })
        .collect()
}

/// An image stored as `header.png` gets the manifest id `header`, which the
/// header part already has. Reported, bytes unchanged (both items written).
///
/// 이것을 실패시키는 것: `encode_with_diagnostics` 에서
/// `manifest_id_conflicts` 호출을 빼는 것.
#[test]
fn an_image_stored_as_header_png_is_a_package_id_conflict() {
    let mut images = ImageStore::new();
    images.insert("header.png", vec![0x89, b'P', b'N', b'G']);
    let outcome = encode_runs(vec![Run::text("본문", CharShapeIndex::new(0))], &images);

    assert_eq!(
        conflicts(&outcome.warnings),
        [(
            "header".to_string(),
            vec!["Contents/header.xml".to_string(), "BinData/header.png".to_string()]
        )]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
    let hpf = part(&outcome.bytes, "Contents/content.hpf");
    assert_eq!(hpf.matches(r#"<opf:item id="header""#).count(), 2, "bytes are not rewritten");
}

/// 이것을 실패시키는 것: 항목이 하나뿐인 id 도 보고하는 것.
#[test]
fn distinct_image_keys_are_not_a_conflict() {
    let mut images = ImageStore::new();
    images.insert("image1.png", vec![0x89, b'P', b'N', b'G']);
    images.insert("image2.jpg", vec![0xFF, 0xD8]);
    let outcome = encode_runs(vec![Run::text("본문", CharShapeIndex::new(0))], &images);
    assert_eq!(conflicts(&outcome.warnings), []);
}

/// A repository fixture under `tests/fixtures/`.
fn fixture(path: &str) -> Vec<u8> {
    let root = concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures/");
    std::fs::read(format!("{root}{path}")).unwrap_or_else(|e| panic!("fixture {path}: {e}"))
}

/// Decodes `bytes` and encodes the result again with `images`.
fn reencode(bytes: &[u8], images: Option<&ImageStore>) -> EncodeOutcome {
    let decoded = HwpxDecoder::decode(bytes).expect("decode");
    HwpxEncoder::encode_with_diagnostics(
        &decoded.document.validate().expect("validate"),
        &decoded.style_store,
        images.unwrap_or(&decoded.image_store),
        EncodeOptions::default(),
    )
    .expect("encode")
}

fn unresolved(warnings: &[EncodeWarning]) -> Vec<(String, String, String)> {
    warnings
        .iter()
        .filter_map(|w| match w {
            EncodeWarning::ImageReferenceUnresolved { part, location, reference } => {
                Some((part.clone(), location.clone(), reference.clone()))
            }
            _ => None,
        })
        .collect()
}

fn triple(part: &str, location: &str, reference: &str) -> (String, String, String) {
    (part.into(), location.into(), reference.into())
}

/// A border fill's image brush lives in the header. Without the decoded
/// images (what `from-json` without `--base` does) its reference resolves to
/// nothing.
///
/// 이것을 실패시키는 것: header part 를 검사 목록에서 빼는 것.
#[test]
fn a_border_fill_image_without_its_asset_is_reported_in_the_header() {
    let outcome =
        reencode(&fixture("tables/table_16c_image_fill_tile.hwpx"), Some(&ImageStore::new()));
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/header.xml", "hc:img@binaryItemIDRef", "image1")]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
}

/// 이것을 실패시키는 것: 이미지 항목을 id 가 아니라 확장자를 포함한 저장소
/// 키(`image1.png`)로 대조하는 것.
#[test]
fn a_border_fill_image_with_its_asset_resolves() {
    let outcome = reencode(&fixture("tables/table_16c_image_fill_tile.hwpx"), None);
    assert_eq!(unresolved(&outcome.warnings), []);
}

/// An embedded chart (the HWP5 carry) writes `<hp:ole binaryItemIDRef="ole1">`
/// and registers `BinData/ole1.ole` as an OLE item, not as an image.
///
/// 이것을 실패시키는 것: 해석 집합에서 OLE 항목을 빼는 것.
#[test]
fn an_embedded_chart_ole_resolves() {
    let chart = Control::EmbeddedChart {
        chart_xml: r#"<?xml version="1.0" encoding="UTF-8"?><c:chartSpace/>"#.to_string(),
        ole_bytes: vec![0xD0, 0xCF, 0x11, 0xE0],
        width: HwpUnit::new(1000).unwrap(),
        height: HwpUnit::new(1000).unwrap(),
        placement: None,
    };
    let outcome =
        encode_runs(vec![Run::control(chart, CharShapeIndex::new(0))], &ImageStore::new());
    assert!(part(&outcome.bytes, "Contents/section0.xml").contains("binaryItemIDRef=\"ole1\""));
    assert!(part(&outcome.bytes, "Contents/content.hpf").contains("media-type=\"application/ole\""));
    assert_eq!(unresolved(&outcome.warnings), []);
}

/// 이것을 실패시키는 것: section part 를 검사 목록에서 빼는 것.
#[test]
fn a_picture_without_its_asset_is_reported_in_its_section() {
    let picture = Image::from_path(
        "BinData/missing.png",
        HwpUnit::new(1000).unwrap(),
        HwpUnit::new(1000).unwrap(),
    );
    let outcome =
        encode_runs(vec![Run::image(picture, CharShapeIndex::new(0))], &ImageStore::new());
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "missing")]
    );
}

/// F6: a picture whose reference names no item (`image2`, written by an
/// older encoder). The reference is reported and kept: Hancom opens such a
/// file without a warning and draws an empty picture box at the picture's
/// size, and an empty reference would make our own decoder drop the picture.
///
/// 이것을 실패시키는 것: 해석되지 않은 참조를 빈 값으로 바꾸거나 그림을 빼는
/// 것 (한컴이 다시 저장할 때 쓰는 형태로의 정규화).
#[test]
fn a_dangling_picture_reference_is_reported_and_kept() {
    let outcome = reencode(&fixture("images/f6-dangling-image.hwpx"), None);
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "image2")]
    );
    let section = part(&outcome.bytes, "Contents/section0.xml");
    assert!(section.contains("binaryItemIDRef=\"image1\""), "the resolved picture stays");
    assert!(section.contains("binaryItemIDRef=\"image2\""), "the dangling reference stays");
}

/// F6, after Hancom saved it again: the lost picture's reference is empty
/// (`binaryItemIDRef=""`), which is not a reference, and `image1` resolves.
/// (The empty-reference picture does not reach the Core document at all —
/// the decoder drops a picture without a reference.)
///
/// 이것을 실패시키는 것: 이미지 항목 id 를 해석 집합에서 빼는 것.
#[test]
fn hancom_resave_of_a_dangling_reference_reports_nothing() {
    let bytes = fixture("images/f6-dangling-image-resaved.hwpx");
    let wire = part(&bytes, "Contents/section0.xml");
    assert!(wire.contains("binaryItemIDRef=\"\""), "Hancom empties the lost reference");

    let outcome = reencode(&bytes, None);
    assert_eq!(unresolved(&outcome.warnings), []);
    assert!(part(&outcome.bytes, "Contents/section0.xml").contains("binaryItemIDRef=\"image1\""));
}
