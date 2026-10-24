//! Lossless Markdown for body pictures: placement, instance id, crop,
//! image extent and raw format come back equal, and the picture caption is a
//! `<caption>` child whose content is an exact subtree.

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::Control;
use hwpforge_core::image::{Image, ImageCrop, ImageFormat};
use hwpforge_core::placement::{ObjectPlacement, ObjectRelativeTo, ObjectTextWrap};
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
use hwpforge_core::{Document, ObjectId, PageSettings, Paragraph, Run, RunContent, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn text(t: &str) -> Run {
    Run::text(t, CharShapeIndex::new(0))
}

fn encode(runs: Vec<Run>) -> Result<String, MdError> {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    MdEncoder::encode_lossless(&d.validate().unwrap())
}

fn round_trip(run: Run) -> Run {
    let md = encode(vec![run]).unwrap();
    MdDecoder::decode_lossless(&md).unwrap().sections()[0].paragraphs[0].runs[0].clone()
}

fn picture() -> Image {
    Image::new("BinData/BIN0002", u(6860), u(2388), ImageFormat::Unknown("BIN0002".into()))
}

/// 기존 동작 고정: lossless는 이 변경 전부터 빈 경로와 crop을 왕복 보존한다.
#[test]
fn assetless_picture_round_trips_losslessly() {
    let mut image = picture();
    image.path.clear();
    image.crop = Some(ImageCrop { left: u(1), top: u(2), right: u(30), bottom: u(40) });
    let run = Run::image(image, CharShapeIndex::new(0));
    assert_eq!(round_trip(run.clone()), run);
}

fn with_caption(paragraphs: Vec<Paragraph>) -> Run {
    let mut image = picture();
    image.caption = Some(Caption::new(paragraphs, CaptionSide::Bottom));
    Run::image(image, CharShapeIndex::new(4))
}

/// A body picture with every field set comes back equal: a floating
/// placement with a negative offset and an outer margin, an instance id, a
/// crop, an image extent, a link and an unknown format's raw spelling.
///
/// 이것을 실패시키는 것: 본문 그림 PLACE 방출 삭제 · 디코더가 본문에서 PLACE 무시 ·
/// `inst_id`·crop·extent·`data-format-raw` 방출 삭제.
#[test]
fn body_picture_round_trips_every_field() {
    let mut p = ObjectPlacement::legacy_inline_defaults();
    p.text_wrap = ObjectTextWrap::Square;
    p.treat_as_char = false;
    p.vert_rel_to = ObjectRelativeTo::Page;
    p.vert_offset = u(-120);
    p.out_margin = Some(TableMargin { left: u(0), right: u(0), top: u(0), bottom: u(140) });
    let mut image = picture();
    image.placement = Some(p);
    image.inst_id = Some(ObjectId::new(42));
    image.crop = Some(ImageCrop { left: u(0), top: u(0), right: u(7474), bottom: u(15000) });
    image.image_extent = Some((u(22500), u(15000)));
    image.hyperlink = Some("http://a.kr".into());
    let run = Run::image(image, CharShapeIndex::new(9));
    assert_eq!(round_trip(run.clone()), run);
}

/// The caption comes back with its side, gap, width — none kept apart from
/// zero —, full-size flag, saved width and paragraphs (a picture inside the
/// caption included); an empty caption is an empty element.
///
/// 이것을 실패시키는 것: caption `data-side` 기본값화 · caption `width` 없음과 0 혼동 ·
/// caption 문단 버림 · full-size·saved width 방출 삭제.
#[test]
fn picture_caption_round_trips() {
    let inner = Run::image(picture(), CharShapeIndex::new(0));
    for (side, width) in
        [(CaptionSide::Left, None), (CaptionSide::Top, Some(0)), (CaptionSide::Right, Some(9000))]
    {
        let mut caption = Caption::new(
            vec![para(vec![text("그림 1"), inner.clone()]), para(vec![text("둘째")])],
            side,
        )
        .with_gap(u(567))
        .with_full_size(true)
        .with_last_width(Some(u(30000)));
        if let Some(w) = width {
            caption = caption.with_width(u(w));
        }
        let mut image = picture();
        image.caption = Some(caption);
        let run = Run::image(image, CharShapeIndex::new(4));
        assert_eq!(round_trip(run.clone()), run, "{side:?} {width:?}");
    }
    let empty = with_caption(Vec::new());
    let md = encode(vec![empty.clone()]).unwrap();
    assert!(md.contains("<caption data-side=\"Bottom\"") && md.contains(" /></img>"), "{md}");
    assert_eq!(round_trip(empty.clone()), empty);
}

/// The caption is an exact subtree: every list the decoder would fill in —
/// a paragraph without runs, a table without rows, a cell or a note without
/// paragraphs — refuses the document, at any depth.
///
/// 이것을 실패시키는 것: `runs: []` 거부 · 빈 행 표 거부 · 빈 칸 거부 · 빈 각주 거부 ·
/// 표 칸 안으로 내려가기 각각 삭제.
#[test]
fn picture_caption_admission() {
    let cell = |paragraphs: Vec<Paragraph>| TableCell::new(paragraphs, u(3000));
    let table = |rows: Vec<TableRow>| Run::table(Table::new(rows), CharShapeIndex::new(0));
    let cases: Vec<(&str, Vec<Paragraph>)> = vec![
        ("no runs", vec![Paragraph::with_runs(Vec::new(), ParaShapeIndex::new(0))]),
        ("no rows", vec![para(vec![table(Vec::new())])]),
        (
            "empty note in a cell",
            vec![para(vec![table(vec![TableRow::new(vec![cell(vec![para(vec![
                Run::control(Control::footnote(Vec::new()), CharShapeIndex::new(0)),
            ])])])])])],
        ),
        ("empty cell", vec![para(vec![table(vec![TableRow::new(vec![cell(Vec::new())])])])]),
        (
            "empty note",
            vec![para(vec![Run::control(Control::footnote(Vec::new()), CharShapeIndex::new(0))])],
        ),
    ];
    for (name, paragraphs) in cases {
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(
            vec![para(vec![with_caption(paragraphs)])],
            PageSettings::a4(),
        ));
        // Core validation may refuse some of these shapes before the encoder;
        // when it does not, the encoder must.
        if let Ok(v) = d.validate() {
            assert!(
                matches!(MdEncoder::encode_lossless(&v), Err(MdError::UnsupportedStructure { .. })),
                "{name}"
            );
        }
    }
    // A note, a table and a group-free picture inside the caption are fine.
    let ok = vec![para(vec![
        Run::control(Control::footnote(vec![para(vec![text("n")])]), CharShapeIndex::new(0)),
        table(vec![TableRow::new(vec![cell(vec![para(vec![text("c")])])])]),
    ])];
    assert!(encode(vec![with_caption(ok)]).is_ok());
}

const SECTION: &str = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0">"#;
const IMG: &str = r#"data-char-shape="0" src="a" alt="a" data-format="PNG" data-width-unit="10" data-height-unit="10""#;

fn decode_body(body: &str) -> Result<Document, MdError> {
    let md = encode(vec![text("t")]).unwrap();
    let frontmatter = &md[..md.find("<section").unwrap()];
    MdDecoder::decode_lossless(&format!("{frontmatter}{SECTION}{body}</p></section>\n"))
}

/// The caption grammar: side and gap are required, an unknown side is
/// invalid, a width given only in millimetres is missing its unit, a second
/// caption, text beside it and a caption outside `<img>` are refused.
///
/// 이것을 실패시키는 것: caption 을 둘 허용 · `data-side` 기본값화 · `img` 밖 caption 허용.
#[test]
fn picture_caption_grammar_errors() {
    let img = |inner: &str| decode_body(&format!("<img {IMG}>{inner}</img>"));
    let cap = r#"<caption data-side="Top" data-gap-unit="0" />"#;
    assert!(img(cap).is_ok());
    assert!(matches!(
        img(r#"<caption data-gap-unit="0" />"#),
        Err(MdError::LosslessMissingAttribute { attribute: "data-side", .. })
    ));
    assert!(matches!(
        img(r#"<caption data-side="Top" />"#),
        Err(MdError::LosslessMissingAttribute { attribute: "data-gap-unit", .. })
    ));
    assert!(matches!(
        img(r#"<caption data-side="Middle" data-gap-unit="0" />"#),
        Err(MdError::LosslessInvalidAttribute { attribute: "data-side", .. })
    ));
    assert!(matches!(
        img(r#"<caption data-side="Top" data-gap-unit="0" data-width-mm="1.00" />"#),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    assert!(matches!(img(&format!("{cap}{cap}")), Err(MdError::LosslessParse { .. })));
    assert!(matches!(img("words"), Err(MdError::LosslessParse { .. })));
    assert!(matches!(decode_body(cap), Err(MdError::LosslessParse { .. })));
    let d = img(r#"<caption data-side="Left" data-gap-unit="5" data-width-unit="0"><p data-para-shape="0"><span data-char-shape="0">c</span></p></caption>"#).unwrap();
    let RunContent::Image(i) = &d.sections()[0].paragraphs[0].runs[0].content else { panic!() };
    let c = i.caption.as_ref().unwrap();
    assert_eq!(
        (c.side, c.gap, c.width, c.paragraphs.len()),
        (CaptionSide::Left, u(5), Some(u(0)), 1)
    );
}
