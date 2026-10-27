#[path = "support/encoding.rs"]
mod encoding;
use encoding::batang_styles as styles;
#[path = "support/package_references.rs"]
mod shared;
use shared::*;

use std::io::Read;

use hwpforge_core::control::Control;
use hwpforge_core::image::{Image, ImageStore};
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{EncodeOptions, EncodeOutcome, EncodeWarning, HwpxDecoder, HwpxEncoder};

fn try_encode_runs(
    runs: Vec<Run>,
    images: &ImageStore,
) -> hwpforge_smithy_hwpx::error::HwpxResult<EncodeOutcome> {
    let para = Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    HwpxEncoder::encode_with_diagnostics(
        &doc.validate().expect("validate"),
        &styles(),
        images,
        EncodeOptions::default(),
    )
}

fn encode_runs(runs: Vec<Run>, images: &ImageStore) -> EncodeOutcome {
    try_encode_runs(runs, images).expect("encode")
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
        frame: hwpforge_core::ObjectFrame::new(),
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
    assert!(
        zip.file_names()
            .map(|name| name.expect("ZIP entry name").into_owned())
            .any(|n| n == "BinData/a\u{1}.png"),
        "premise: the entry keeps U+0001"
    );
    let hpf = part(&outcome.bytes, "Contents/content.hpf");
    assert!(hpf.contains(r#"href="BinData/a.png""#), "premise: the href lost it: {hpf}");
    assert_eq!(
        unresolved(&outcome.warnings),
        [triple("Contents/section0.xml", "hc:img@binaryItemIDRef", "a")]
    );
    assert!(outcome.warnings.iter().any(EncodeWarning::is_semantic_loss));
}

/// A key that used to be cleaned to another asset's href is now rejected.
/// 이것을 실패시키는 것: 빈 성분/절대 자산 경로를 다시 허용하거나 삭제식 정규화 복원.
#[test]
fn zip9_fix1_picture_with_absolute_asset_key_is_rejected() {
    let mut images = ImageStore::new();
    images.insert("a\u{0}.png", vec![0x89, b'P', b'N', b'G', 1]);
    images.insert("/a.png", vec![0x89, b'P', b'N', b'G', 2]);
    let size = |v| HwpUnit::new(v).unwrap();
    let picture = Image::from_path("BinData/a\u{0}.png", size(1000), size(1000));
    let result = try_encode_runs(vec![Run::image(picture, CharShapeIndex::new(0))], &images);
    assert!(matches!(result, Err(hwpforge_smithy_hwpx::HwpxError::InvalidStructure { .. })));
}

/// 이것을 실패시키는 것: 그룹 그림 경로에서 공통 파트 이름 거부를 우회하기.
#[test]
fn zip9_fix1_group_picture_with_absolute_asset_key_is_rejected() {
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
        frame: hwpforge_core::ObjectFrame::new(),
        children: vec![GroupChild::Picture(picture), GroupChild::Shape(rect)],
        width: size(1000),
        height: size(1000),
        placement: None,
        inst_id: None,
    };
    let result = try_encode_runs(vec![Run::control(group, CharShapeIndex::new(0))], &images);
    assert!(matches!(result, Err(hwpforge_smithy_hwpx::HwpxError::InvalidStructure { .. })));
}

/// a picture whose reference names no item (`image2`, written by an
/// older encoder). The reference is reported and kept: Hancom opens such a
/// file without a warning and draws an empty picture box at the picture's
/// size. Keep the nonempty reference as the key for a later asset repair.
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

/// 이것을 실패시키는 것: 빈 참조 버리기 / 빈 경로 대신 BinData/ 저장 /
/// decode_picture_frame 호출 제거 / decode_image_placement 기본값화.
#[test]
fn assetless_body_picture_keeps_frame_attributes() {
    use hwpforge_core::image::ImageCrop;
    use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
    use hwpforge_core::table::TableMargin;
    use hwpforge_core::RunContent;
    use hwpforge_foundation::Size;

    let u = |v| HwpUnit::new(v).unwrap();
    for treat_as_char in [false, true] {
        let mut image = Image::from_path("", u(1200), u(700));
        image.crop = Some(ImageCrop { left: u(10), top: u(20), right: u(1900), bottom: u(1000) });
        image.image_extent = Some((u(2000), u(1100)));
        image.frame.original_size = Some(Size::new(u(2400), u(1400)));
        let mut placement = ObjectPlacement::legacy_inline_defaults();
        placement.treat_as_char = treat_as_char;
        placement.text_wrap = ObjectTextWrap::Square;
        placement.vert_rel_to = ObjectRelativeTo::Page;
        placement.vert_offset = u(123);
        placement.horz_offset = u(456);
        placement.out_margin =
            Some(TableMargin { left: u(1), right: u(2), top: u(3), bottom: u(4) });
        image.placement = Some(placement);
        let source =
            encode_runs(vec![Run::image(image, CharShapeIndex::new(0))], &ImageStore::new());
        let decoded = HwpxDecoder::decode(&source.bytes).unwrap();
        let image = decoded.document.sections()[0].paragraphs[0]
            .runs
            .iter()
            .find_map(|r| if let RunContent::Image(i) = &r.content { Some(i) } else { None })
            .expect("assetless frame");
        assert!(image.path.is_empty(), "no fake BinData key");
        assert!(decoded.image_store.is_empty());
        let output = reencode(&source.bytes, None);
        assert!(!output.warnings.iter().any(EncodeWarning::is_semantic_loss));
        let before = part(&source.bytes, "Contents/section0.xml");
        let after = part(&output.bytes, "Contents/section0.xml");
        for tag in ["hp:sz", "hp:pos", "hp:imgClip", "hp:imgDim", "hp:orgSz", "hp:outMargin"] {
            let element = |xml: &str| {
                let start = xml.find(&format!("<{tag} ")).unwrap();
                xml[start..start + xml[start..].find("/>").unwrap() + 2].to_string()
            };
            assert_eq!(element(&before), element(&after), "{tag}, treatAsChar={treat_as_char}");
        }
        assert!(after.contains(r#"textWrap="SQUARE""#));
    }
}
