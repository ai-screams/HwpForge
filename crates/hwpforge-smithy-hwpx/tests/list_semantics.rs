//! Paragraph list semantics through a written and read-back HWPX (#296):
//! the read surface classifies a paragraph by the shared typed lookup, and
//! a list reference the shared semantics cannot carry stays on the wire.

#[path = "support/encoding.rs"]
mod encoding;
#[path = "support/package.rs"]
mod package;
#[path = "support/xml.rs"]
mod xml;

use hwpforge_core::image::ImageStore;
use hwpforge_core::{
    BulletDef, Document, NumberingDef, PageSettings, ParaHead, Paragraph, ParagraphListRef, Run,
    Section, StyleLookup,
};
use hwpforge_foundation::{
    BulletIndex, CharShapeIndex, HeadingType, NumberFormatType, ParaShapeIndex,
};
use hwpforge_smithy_hwpx::style_store::HwpxParaShape;
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxEncoder, HwpxReader, ParaKindView};

fn head(checkable: bool) -> ParaHead {
    ParaHead {
        start: 0,
        level: 1,
        num_format: NumberFormatType::Digit,
        text: String::new(),
        checkable,
    }
}

/// A document of one paragraph per given list wire triple (type, idRef,
/// level, checked), after the store's default paragraph shape 0.
fn package(lists: &[(HeadingType, u32, u32, bool)]) -> Vec<u8> {
    let mut store = encoding::batang_styles();
    store.push_numbering(NumberingDef::default_outline());
    store.push_bullet(BulletDef {
        id: 1,
        bullet_char: "•".into(),
        checked_char: None,
        use_image: false,
        para_head: head(false),
    });
    store.push_bullet(BulletDef {
        id: 2,
        bullet_char: "☐".into(),
        checked_char: Some("☑".into()),
        use_image: false,
        para_head: head(true),
    });
    let mut paragraphs = Vec::new();
    for (i, &(heading_type, heading_id_ref, heading_level, checked)) in lists.iter().enumerate() {
        let mut shape = HwpxParaShape::default();
        shape.heading_type = heading_type;
        shape.heading_id_ref = heading_id_ref;
        shape.heading_level = heading_level;
        shape.checked = checked;
        store.push_para_shape(shape);
        paragraphs.push(Paragraph::with_runs(
            vec![Run::text(format!("p{i}"), CharShapeIndex::new(0))],
            ParaShapeIndex::new(i + 1),
        ));
    }
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    HwpxEncoder::encode(&doc.validate().expect("validate"), &store, &ImageStore::new())
        .expect("encode")
}

/// The read surface reports each list paragraph's kind, level and checked
/// state from its wire triple: a NUMBER, a plain BULLET (its `checked`
/// is no checkbox), a checkable BULLET checked and not, an OUTLINE (a
/// heading at its depth).
/// 이것을 실패시키는 것: 저장소 조회에서 위치(`idRef − 1`) 대신 `idRef` 로 글머리표 찾기.
#[test]
fn read_reports_list_kind_level_and_checked_state() {
    let bytes = package(&[
        (HeadingType::Number, 1, 2, false),
        (HeadingType::Bullet, 1, 1, true),
        (HeadingType::Bullet, 2, 0, true),
        (HeadingType::Bullet, 2, 3, false),
        (HeadingType::Outline, 0, 1, false),
    ]);
    let view = HwpxReader::read_paragraphs(&bytes, 0, None).expect("read");
    let kinds: Vec<_> = view.paragraphs.iter().map(|p| p.kind).collect();
    let list = |numbered, level, checked| ParaKindView::List { numbered, level, checked };
    assert_eq!(
        kinds,
        [
            list(true, 2, None),
            list(false, 1, None),
            list(false, 0, Some(true)),
            list(false, 3, Some(false)),
            ParaKindView::Heading { level: 2 },
        ]
    );
}

/// A BULLET with no definition (`idRef` 0) reads as no list, and decode,
/// encode and decode again keep its wire triple: `type`, `idRef`, `level`
/// and `paraPr@checked` are written as they were read.
/// 이것을 실패시키는 것: 다시 쓸 때 정의 없는 참조의 `checked` 지우기.
#[test]
fn unreadable_list_reference_is_kept_on_the_wire() {
    let bytes = package(&[(HeadingType::Bullet, 0, 1, true)]);
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    assert_eq!(decoded.style_store.para_list(ParaShapeIndex::new(1)), None);
    let again = HwpxEncoder::encode(
        &decoded.document.validate().expect("validate"),
        &decoded.style_store,
        &decoded.image_store,
    )
    .expect("encode again");
    let header = xml::Element::parse(&package::part(&again, "Contents/header.xml"));
    let para = header
        .find("hh:paraProperties")
        .children
        .iter()
        .find(|p| p.attr("id") == Some("1"))
        .expect("paraPr 1");
    let heading = para.find("hh:heading");
    assert_eq!(
        (heading.attr("type"), heading.attr("idRef"), heading.attr("level"), para.attr("checked")),
        (Some("BULLET"), Some("0"), Some("1"), Some("1"))
    );
    let read_back = HwpxDecoder::decode(&again).expect("decode again");
    let shape = read_back.style_store.para_shape(ParaShapeIndex::new(1)).expect("shape");
    assert_eq!(
        (shape.heading_type, shape.heading_id_ref, shape.heading_level, shape.checked),
        (HeadingType::Bullet, 0, 1, true)
    );
    // A valid reference beside it still reads.
    let bytes = package(&[(HeadingType::Bullet, 0, 1, true), (HeadingType::Bullet, 2, 0, true)]);
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    assert_eq!(
        decoded.style_store.para_list(ParaShapeIndex::new(2)),
        Some(ParagraphListRef::CheckBullet {
            bullet_id: BulletIndex::new(1),
            level: 0,
            checked: true
        })
    );
}

/// The `list_semantics` losses of a decode, by reason.
fn list_semantics_losses(decoded: &hwpforge_smithy_hwpx::HwpxDocument) -> Vec<String> {
    decoded
        .warnings
        .iter()
        .filter_map(|w| match w {
            hwpforge_smithy_hwpx::DecodeWarning::ContentLost { kind, reason, .. }
                if kind == "list_semantics" =>
            {
                Some(reason.clone())
            }
            _ => None,
        })
        .collect()
}

/// An OUTLINE level the shared semantics cannot carry (10) is reported
/// once and reads as a body paragraph, not as a heading clamped to H6.
/// 이것을 실패시키는 것: 제목 수준을 원래 `heading_level` 에서 직접 읽기.
#[test]
fn invalid_outline_level_reads_as_body() {
    let bytes =
        package(&[(HeadingType::Outline, 0, 10, false), (HeadingType::Outline, 0, 9, false)]);
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    let losses = list_semantics_losses(&decoded);
    assert_eq!(losses.len(), 1, "{losses:?}");
    assert!(losses[0].starts_with("paraPr 1 heading OUTLINE idRef 0 level 10"), "{}", losses[0]);
    let view = HwpxReader::read_paragraphs(&bytes, 0, None).expect("read");
    let kinds: Vec<_> = view.paragraphs.iter().map(|p| p.kind).collect();
    assert_eq!(kinds, [ParaKindView::Body, ParaKindView::Heading { level: 6 }]);
}

/// A heading type the decoder does not know is reported once with its raw
/// type and paraPr, reads as no list, and is written back as NONE.
/// 이것을 실패시키는 것: 미지 type 보고 삭제.
#[test]
fn unknown_heading_type_is_reported_and_written_as_none() {
    let bytes = package::splice_part(
        &package(&[(HeadingType::Bullet, 1, 1, false)]),
        "Contents/header.xml",
        r#"type="BULLET""#,
        r#"type="MYSTERY""#,
    );
    let decoded = HwpxDecoder::decode(&bytes).expect("decode");
    assert_eq!(
        list_semantics_losses(&decoded),
        ["paraPr 1 heading MYSTERY idRef 1 level 1: unknown heading type; it is read as NONE \
          and re-encoded as NONE"]
    );
    let view = HwpxReader::read_paragraphs(&bytes, 0, None).expect("read");
    assert_eq!(view.paragraphs[0].kind, ParaKindView::Body);
    let again = HwpxEncoder::encode(
        &decoded.document.validate().expect("validate"),
        &decoded.style_store,
        &decoded.image_store,
    )
    .expect("encode again");
    let header = xml::Element::parse(&package::part(&again, "Contents/header.xml"));
    let para = header
        .find("hh:paraProperties")
        .children
        .iter()
        .find(|p| p.attr("id") == Some("1"))
        .expect("paraPr 1");
    assert_eq!(para.find("hh:heading").attr("type"), Some("NONE"));
}
