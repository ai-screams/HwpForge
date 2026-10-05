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

/// The ids that conflict are the ones the written `content.hpf` holds. A key
/// with a character XML cannot carry (U+0001) loses it on the way out, so
/// `head\u{1}er.png` is written as id `header` — the same id as the header
/// part — although the two keys differ inside the encoder.
///
/// 이것을 실패시키는 것: 겹침을 써진 `content.hpf` 를 읽은 값이 아니라 인코더
/// 안의 manifest 항목 id 로 판정하는 것.
#[test]
fn ids_are_compared_as_the_written_content_hpf_reads() {
    let mut images = ImageStore::new();
    images.insert("head\u{1}er.png", vec![0x89, b'P', b'N', b'G']);
    images.insert("he&#97;der.jpg", vec![0xFF, 0xD8]);
    let outcome = encode_runs(vec![Run::text("본문", CharShapeIndex::new(0))], &images);

    let ids = written_ids(&part(&outcome.bytes, "Contents/content.hpf"));
    assert_eq!(ids.iter().filter(|id| *id == "header").count(), 2, "{ids:?}");
    assert!(
        ids.contains(&"he&#97;der".to_string()),
        "an escaped key reads back as itself: {ids:?}"
    );
    assert_eq!(
        conflicts(&outcome.warnings),
        [(
            "header".to_string(),
            vec!["Contents/header.xml".to_string(), "BinData/header.png".to_string()]
        )]
    );
}

/// `opf:item@id` values of a `content.hpf`, read with an XML reader.
fn written_ids(hpf: &str) -> Vec<String> {
    let mut reader = quick_xml::Reader::from_str(hpf);
    let mut ids = Vec::new();
    loop {
        match reader.read_event().expect("well-formed content.hpf") {
            quick_xml::events::Event::Empty(e) | quick_xml::events::Event::Start(e)
                if e.name().as_ref() == "opf:item" =>
            {
                for attr in e.attributes() {
                    let attr = attr.expect("attribute");
                    if attr.key.as_ref() == "id" {
                        ids.push(
                            attr.normalized_value(quick_xml::XmlVersion::Explicit1_0)
                                .expect("value")
                                .into_owned(),
                        );
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
    }
    ids
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

/// A picture inside a group writes its `binaryItemIDRef` in the group's
/// `<hp:container>`; a missing asset is reported there as for a picture in a
/// run.
///
/// 이것을 실패시키는 것: 묶음 그림 writer 가 참조를 빈 값으로 쓰는 것(빈 값은
/// 참조로 보지 않아 경고가 사라진다), 또는 section part 를 검사 목록에서 빼는 것.
#[test]
fn a_group_picture_without_its_asset_is_reported_in_its_section() {
    use hwpforge_core::control::{GroupChild, ShapeStyle};
    let size = |v| HwpUnit::new(v).unwrap();
    let picture = Image::from_path("BinData/missing.png", size(1000), size(1000));
    let mut rect = Control::rect(size(1000), size(1000)).unwrap();
    if let Control::Rect { style, .. } = &mut rect {
        *style = Some(ShapeStyle::default());
    }
    let group = Control::Group {
        children: vec![GroupChild::Picture(picture), GroupChild::Shape(rect)],
        width: size(1000),
        height: size(1000),
        placement: None,
        inst_id: None,
    };
    let outcome =
        encode_runs(vec![Run::control(group, CharShapeIndex::new(0))], &ImageStore::new());
    let section = part(&outcome.bytes, "Contents/section0.xml");
    assert!(section.contains("<hp:container"), "premise: the picture is written in a group");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "missing")]
    );
}

/// A picture on a master page is written in that master page's part, which
/// the check reads too.
///
/// 이것을 실패시키는 것: 바탕쪽 part 를 검사 목록에서 빼는 것.
#[test]
fn a_master_page_picture_without_its_asset_is_reported_in_its_part() {
    use hwpforge_core::section::MasterPage;
    use hwpforge_foundation::ApplyPageType;
    let size = |v| HwpUnit::new(v).unwrap();
    let picture = Image::from_path("BinData/missing.png", size(1000), size(1000));
    let master = Paragraph::with_runs(
        vec![Run::image(picture, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let body = Paragraph::with_runs(
        vec![Run::text("본문", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let mut section = Section::with_paragraphs(vec![body], PageSettings::a4());
    section.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, vec![master])]);
    let mut doc = Document::new();
    doc.add_section(section);
    let outcome = HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        &ImageStore::new(),
        EncodeOptions::default(),
    )
    .expect("encode");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/masterpage0.xml", "hc:img@binaryItemIDRef", "missing")]
    );
}

/// An image key with a character XML cannot carry keeps it in the ZIP entry
/// name, but the scanned `content.hpf` writes the item's `href` without it
/// (`BinData/a.png` for the entry `BinData/a\u{1}.png`). The item id still
/// matches the picture's reference, yet a reader following the `href` finds
/// no entry: the picture is reported as unresolved, and the entry name is
/// not cleaned to match (that would rename the asset without a word).
///
/// 이것을 실패시키는 것: 해석 집합을 실제로 쓴 ZIP 항목 이름과 대조하지 않는 것
/// (manifest id 만 보면 `a` 가 있어 경고가 사라진다).
#[test]
fn an_item_whose_href_is_not_the_written_entry_is_unresolved() {
    let mut images = ImageStore::new();
    images.insert("a\u{1}.png", vec![0x89, b'P', b'N', b'G']);
    let picture = Image::from_path(
        "BinData/a\u{1}.png",
        HwpUnit::new(1000).unwrap(),
        HwpUnit::new(1000).unwrap(),
    );
    let outcome = encode_runs(vec![Run::image(picture, CharShapeIndex::new(0))], &images);

    let zip = zip::ZipArchive::new(std::io::Cursor::new(&outcome.bytes)).expect("zip");
    assert!(zip.file_names().any(|n| n == "BinData/a\u{1}.png"), "premise: the entry keeps U+0001");
    let hpf = part(&outcome.bytes, "Contents/content.hpf");
    assert!(hpf.contains(r#"href="BinData/a.png""#), "premise: the href lost it: {hpf}");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "a")]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
}

/// Two different images stored as `a\u{0}.png` and `/a.png`: once
/// `content.hpf` is scanned, the first item reads `id="a"
/// href="BinData/a.png"`, and the second asset's entry name is cleaned to
/// exactly `BinData/a.png`. The ids (`a`, `/a`) do not collide, so a reader
/// following the first picture's `href` opens the other picture. The item is
/// compared with the entry of its own asset, so the picture is reported.
///
/// 이것을 실패시키는 것: 항목별 대조 대신 쓴 ZIP 항목 이름 전체 집합에 `href` 가
/// 있는지만 보는 것(두 번째 자산이 그 이름을 차지해 경고가 사라진다).
#[test]
fn an_href_naming_another_assets_entry_is_unresolved() {
    let mut images = ImageStore::new();
    images.insert("a\u{0}.png", vec![0x89, b'P', b'N', b'G', 1]);
    images.insert("/a.png", vec![0x89, b'P', b'N', b'G', 2]);
    let size = |v| HwpUnit::new(v).unwrap();
    let picture = Image::from_path("BinData/a\u{0}.png", size(1000), size(1000));
    let outcome = encode_runs(vec![Run::image(picture, CharShapeIndex::new(0))], &images);

    let zip = zip::ZipArchive::new(std::io::Cursor::new(&outcome.bytes)).expect("zip");
    assert!(zip.file_names().any(|n| n == "BinData/a.png"), "premise: `/a.png` owns the name");
    let hpf = part(&outcome.bytes, "Contents/content.hpf");
    assert!(
        hpf.contains(r#"<opf:item id="a" href="BinData/a.png""#),
        "premise: the first item's href is that name: {hpf}"
    );
    assert!(conflicts(&outcome.warnings).is_empty(), "premise: the ids do not collide");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "a")]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
}

/// The same pair of keys with the picture inside a group: the group's
/// `<hp:container>` reference is checked against its own asset's entry too.
///
/// 이것을 실패시키는 것: 위와 같다(전체 이름 집합으로 판정).
#[test]
fn a_group_picture_whose_href_names_another_assets_entry_is_unresolved() {
    use hwpforge_core::control::{GroupChild, ShapeStyle};
    let mut images = ImageStore::new();
    images.insert("a\u{0}.png", vec![0x89, b'P', b'N', b'G', 1]);
    images.insert("/a.png", vec![0x89, b'P', b'N', b'G', 2]);
    let size = |v| HwpUnit::new(v).unwrap();
    let picture = Image::from_path("BinData/a\u{0}.png", size(1000), size(1000));
    let mut rect = Control::rect(size(1000), size(1000)).unwrap();
    if let Control::Rect { style, .. } = &mut rect {
        *style = Some(ShapeStyle::default());
    }
    let group = Control::Group {
        children: vec![GroupChild::Picture(picture), GroupChild::Shape(rect)],
        width: size(1000),
        height: size(1000),
        placement: None,
        inst_id: None,
    };
    let outcome = encode_runs(vec![Run::control(group, CharShapeIndex::new(0))], &images);
    let section = part(&outcome.bytes, "Contents/section0.xml");
    assert!(section.contains("<hp:container"), "premise: the picture is written in a group");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "a")]
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
