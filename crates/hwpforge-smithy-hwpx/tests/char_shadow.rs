//! Character shadow (`<hh:shadow>`) through the HWPX codec (S21).
//!
//! The fixture is a new Hancom (macOS 12.30) document whose body "가나다"
//! uses three shadows: DROP at the default color and offsets, DROP at
//! (5, 5), and CONTINUOUS `#123456` at (−3, 4). Its other seven `charPr`
//! carry Hancom's new-document value, NONE `#C0C0C0` 10 10.

use std::io::{Cursor, Read, Write};

use hwpforge_core::image::ImageStore;
use hwpforge_core::run::Run;
use hwpforge_core::section::Section;
use hwpforge_core::{Document, PageSettings, Paragraph};
use hwpforge_foundation::{CharShadow, CharShapeIndex, ParaShapeIndex, ShadowType};
use hwpforge_smithy_hwpx::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
use hwpforge_smithy_hwpx::{DecodeWarning, HwpxDecoder, HwpxEncoder};

fn fixture() -> Vec<u8> {
    let path = format!(
        "{}/../../tests/fixtures/user_samples/sample-char-shadow.hwpx",
        env!("CARGO_MANIFEST_DIR")
    );
    std::fs::read(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
}

fn header(bytes: &[u8]) -> String {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = String::new();
    zip.by_name("Contents/header.xml").expect("header").read_to_string(&mut out).unwrap();
    out
}

/// `bytes` with `Contents/header.xml` passed through `edit`.
fn with_header(bytes: &[u8], edit: impl Fn(&str) -> String) -> Vec<u8> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("zip");
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for i in 0..zip.len() {
        let mut entry = zip.by_index(i).expect("entry");
        let name = entry.name().expect("ZIP entry name").to_string();
        let options =
            zip::write::SimpleFileOptions::default().compression_method(entry.compression());
        let mut data = Vec::new();
        entry.read_to_end(&mut data).expect("read");
        if name == "Contents/header.xml" {
            data = edit(&String::from_utf8(data).expect("utf-8")).into_bytes();
        }
        out.start_file(name.as_str(), options).expect("start");
        out.write_all(&data).expect("write");
    }
    out.finish().expect("finish").into_inner()
}

/// Every `<hh:shadow …/>` element, in document order.
fn shadows(header: &str) -> Vec<String> {
    header
        .match_indices("<hh:shadow ")
        .map(|(at, _)| header[at..at + header[at..].find("/>").expect("end") + 2].to_string())
        .collect()
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

fn shadow_losses(warnings: &[DecodeWarning]) -> Vec<&DecodeWarning> {
    warnings
        .iter()
        .filter(|w| matches!(w, DecodeWarning::ContentLost { kind, .. } if kind == "char_shadow"))
        .collect()
}

/// `bytes` with the first `charPr`'s `<hh:shadow>` attribute `name` set to `value`.
fn with_first_shadow_attr(bytes: &[u8], name: &str, value: &str) -> Vec<u8> {
    with_header(bytes, |h| {
        let at = h.find("<hh:shadow ").expect("shadow");
        let start = at + h[at..].find(&format!(" {name}=\"")).expect("attr") + name.len() + 3;
        let end = start + h[start..].find('"').expect("quote");
        format!("{}{value}{}", &h[..start], &h[end..])
    })
}

fn first_shadow(bytes: &[u8]) -> (CharShadow, Vec<DecodeWarning>) {
    let doc = HwpxDecoder::decode(bytes).expect("decode");
    let s = doc.style_store.char_shape(CharShapeIndex::new(0)).expect("charPr 0").shadow;
    (s, doc.warnings)
}

/// The fixture's ten shadows survive a full decode → encode byte for byte,
/// and charPr 9 decodes to its values.
///
/// 이것을 실패시키는 것: 인코더의 `#B2B2B2`·10·10 하드코딩 복귀, 디코더가 RGB 정수를
/// `from_raw` 로 읽는 것(BGR 반전), `CONTINUOUS` 를 무음 NONE 으로 읽는 것,
/// 음수 간격을 u32 감김(`4294967293`)으로 쓰는 것.
#[test]
fn hancom_shadows_survive_a_reencode() {
    let source = fixture();
    let before = shadows(&header(&source));
    assert_eq!(before.len(), 10);
    assert_eq!(
        before[9],
        r##"<hh:shadow type="CONTINUOUS" color="#123456" offsetX="-3" offsetY="4"/>"##
    );

    let doc = HwpxDecoder::decode(&source).expect("decode");
    assert!(shadow_losses(&doc.warnings).is_empty(), "{:?}", doc.warnings);
    let nine = doc.style_store.char_shape(CharShapeIndex::new(9)).unwrap().shadow;
    assert_eq!(nine.kind(), ShadowType::Continuous);
    let c = nine.color().expect("color");
    assert_eq!((c.red(), c.green(), c.blue()), (0x12, 0x34, 0x56));
    assert_eq!((nine.offset_x().get(), nine.offset_y().get()), (-3, 4));

    assert_eq!(shadows(&header(&reencode(&source))), before);
}

/// `color="none"` reads as no color and writes back as `none`.
///
/// 이것을 실패시키는 것: `none` 을 검정·DEFAULT 색으로 읽는 것.
#[test]
fn color_none_round_trips() {
    let source = with_first_shadow_attr(&fixture(), "color", "none");
    let (shadow, warnings) = first_shadow(&source);
    assert_eq!(shadow.color(), None);
    assert!(shadow_losses(&warnings).is_empty(), "{warnings:?}");
    assert!(shadows(&header(&reencode(&source)))[0].contains(r#"color="none""#));
}

/// A color or type the store cannot hold is reported as lost content
/// (the editors refuse it), and the fallback is the default color / NONE.
///
/// 이것을 실패시키는 것: 무음 폴백(경고 삭제), `UnknownEnumValue` 로 분류하는 것.
#[test]
fn unreadable_color_or_type_is_content_lost() {
    let (shadow, warnings) = first_shadow(&with_first_shadow_attr(&fixture(), "color", "#12345"));
    assert_eq!(shadow.color(), CharShadow::DEFAULT.color());
    let lost = shadow_losses(&warnings);
    assert_eq!(lost.len(), 1, "{warnings:?}");
    assert!(lost[0].is_content_loss());

    let (shadow, warnings) = first_shadow(&with_first_shadow_attr(&fixture(), "type", "SPARKLE"));
    assert_eq!(shadow.kind(), ShadowType::None);
    let lost = shadow_losses(&warnings);
    assert_eq!(lost.len(), 1, "{warnings:?}");
    assert!(lost[0].is_content_loss());
}

/// Offsets outside −100..=100 clamp and are reported; a u32-wrapped
/// negative in range is read as the negative, without a report.
///
/// 이것을 실패시키는 것: 자르기 전에 좁히기(300 → 44), 자르기·경고 삭제,
/// `deser_i32_or_u32` 를 거치지 않는 것(감긴 값이 범위 밖으로 잘림).
#[test]
fn out_of_range_offsets_clamp_and_are_reported() {
    for (raw, want) in [("300", 100), ("-150", -100)] {
        let (shadow, warnings) = first_shadow(&with_first_shadow_attr(&fixture(), "offsetX", raw));
        assert_eq!(shadow.offset_x().get(), want, "{raw}");
        assert_eq!(shadow_losses(&warnings).len(), 1, "{raw}: {warnings:?}");
    }
    let (shadow, warnings) =
        first_shadow(&with_first_shadow_attr(&fixture(), "offsetX", "4294967291"));
    assert_eq!(shadow.offset_x().get(), -5);
    assert!(shadow_losses(&warnings).is_empty(), "{warnings:?}");
}

/// Older Hancom versions omit `<hh:shadow>` for kind 0: the default, no report.
///
/// 이것을 실패시키는 것: 요소 없음을 손실로 보고하는 것, 다른 값으로 채우는 것.
#[test]
fn missing_shadow_element_is_the_default() {
    let source = with_header(&fixture(), |h| {
        let at = h.find("<hh:shadow ").expect("shadow");
        let end = at + h[at..].find("/>").expect("end") + 2;
        format!("{}{}", &h[..at], &h[end..])
    });
    let (shadow, warnings) = first_shadow(&source);
    assert_eq!(shadow, CharShadow::DEFAULT);
    assert!(shadow_losses(&warnings).is_empty(), "{warnings:?}");
    assert!(!warnings.iter().any(DecodeWarning::is_content_loss), "{warnings:?}");
}

/// A document built in code writes the new-document default for every
/// `charPr` — the value Hancom itself writes, NONE `#C0C0C0` 10 10.
///
/// 이것을 실패시키는 것: 인코더가 DEFAULT 대신 리터럴(옛 `#B2B2B2`)을 쓰는 것,
/// `HwpxCharShape::default()` 가 DEFAULT 아닌 값을 갖는 것.
#[test]
fn a_new_document_writes_the_hancom_default_shadow() {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let mut styles = HwpxStyleStore::with_default_fonts("함초롬돋움");
    styles.push_char_shape(HwpxCharShape::default());
    styles.push_para_shape(HwpxParaShape::default());
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let all = shadows(&header(&bytes));
    assert!(!all.is_empty());
    for s in all {
        assert_eq!(s, r##"<hh:shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>"##);
    }
}

/// A Blueprint document: the built-in template with `shadow: drop` on
/// heading1 (kind only). Every `charPr` the registry writes is Hancom's
/// new-document value, and the heading's shadow takes the default color and
/// offsets — the visible change from `#B2B2B2` that the release notes name.
///
/// 이것을 실패시키는 것: 기본 charPr 묶음(`HwpxStyleStore` 의 Hancom 기본 글자 모양)을
/// DEFAULT 아닌 값으로 두는 것, `PartialCharShadow::resolve` 의 기본 색을 바꾸는 것.
#[test]
fn a_blueprint_document_writes_the_default_shadow_and_a_kind_only_drop() {
    use hwpforge_blueprint::builtins::builtin_default;
    use hwpforge_blueprint::registry::StyleRegistry;
    use hwpforge_blueprint::style::PartialCharShadow;

    let mut template = builtin_default().unwrap();
    let mut drop = PartialCharShadow::default();
    drop.kind = Some(ShadowType::Drop);
    template.styles["heading1"].char_shape.as_mut().expect("heading1 char shape").shadow =
        Some(drop);
    let registry = StyleRegistry::from_template(&template).unwrap();
    let styles = HwpxStyleStore::from_registry(&registry).unwrap();

    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("가", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    let bytes = HwpxEncoder::encode(&doc.validate().unwrap(), &styles, &ImageStore::new()).unwrap();
    let all = shadows(&header(&bytes));
    let (drops, rest): (Vec<_>, Vec<_>) = all.iter().partition(|s| s.contains("DROP"));
    assert_eq!(drops, [r##"<hh:shadow type="DROP" color="#C0C0C0" offsetX="10" offsetY="10"/>"##]);
    assert!(rest.len() >= 7, "{all:?}");
    for s in rest {
        assert_eq!(s, r##"<hh:shadow type="NONE" color="#C0C0C0" offsetX="10" offsetY="10"/>"##);
    }
}
