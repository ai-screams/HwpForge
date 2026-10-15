//! Lossless Markdown for groups (묶음) and their pictures: every field comes
//! back equal, what the grammar cannot hold is refused whole, and the object
//! elements read a closed attribute grammar.

use hwpforge_core::caption::Caption;
use hwpforge_core::control::{Control, GroupChild};
use hwpforge_core::image::{Image, ImageCrop, ImageFormat};
use hwpforge_core::placement::{
    HorzAlign, ObjectPlacement, ObjectRelativeTo, ObjectTextFlow, ObjectTextWrap, VertAlign,
};
use hwpforge_core::table::{Table, TableCell, TableMargin, TableRow};
use hwpforge_core::{Document, ObjectFrame, ObjectId, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex, Size};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn doc(runs: Vec<Run>) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(runs)], PageSettings::a4()));
    d
}

fn encode(runs: Vec<Run>) -> Result<String, MdError> {
    MdEncoder::encode_lossless(&doc(runs).validate().unwrap())
}

fn first_run(d: &Document) -> &Run {
    &d.sections()[0].paragraphs[0].runs[0]
}

/// Every PLACE value away from the inline default, a negative offset and an
/// outer margin.
fn floating(margin: Option<TableMargin>) -> ObjectPlacement {
    let mut p = ObjectPlacement::legacy_inline_defaults();
    p.text_wrap = ObjectTextWrap::InFrontOfText;
    p.text_flow = ObjectTextFlow::LargestOnly;
    p.treat_as_char = false;
    p.flow_with_text = true;
    p.allow_overlap = true;
    p.vert_rel_to = ObjectRelativeTo::Paper;
    p.horz_rel_to = ObjectRelativeTo::Column;
    p.vert_offset = u(-1360);
    p.horz_offset = u(5952);
    p.vert_align = VertAlign::Center;
    p.horz_align = HorzAlign::Right;
    p.affect_line_spacing = true;
    p.hold_anchor_and_so = true;
    p.out_margin = margin;
    p
}

fn picture(path: &str, format: ImageFormat) -> Image {
    Image::new(path, u(6860), u(2388), format)
}

fn group(
    children: Vec<GroupChild>,
    placement: Option<ObjectPlacement>,
    id: Option<u64>,
) -> Control {
    Control::Group {
        children,
        width: u(47509),
        height: u(3721),
        frame: ObjectFrame::new(),
        placement,
        inst_id: id.map(ObjectId::new),
    }
}

/// A group with every field set, pictures with every field set and none
/// set, a nested group, an empty group and an outer margin of zeros (kept
/// apart from no margin) comes back equal, children in z-order.
///
/// 이것을 실패시키는 것: PLACE 방출 삭제 · PLACE 읽기 삭제 · `inst_id` 삭제 ·
/// 중첩 갈래 삭제 · hyperlink 삭제 · `data-format-raw` 삭제(`BIN0001` → `bin0001`) ·
/// offset 부호 없는 파싱 · 바깥 여백·crop·extent 방출 또는 읽기 삭제 ·
/// 자식 순서 뒤집기.
#[test]
fn group_round_trips_every_field() {
    let mut full = picture("BinData/BIN0001", ImageFormat::Unknown("BIN0001".into()));
    full.placement =
        Some(floating(Some(TableMargin { left: u(1), right: u(2), top: u(3), bottom: u(4) })));
    full.inst_id = Some(ObjectId::new(494_346_154));
    full.hyperlink = Some("http://www.korea.kr".into());
    full.crop = Some(ImageCrop { left: u(0), top: u(1113), right: u(7260), bottom: u(3851) });
    full.image_extent = Some((u(22500), u(15000)));
    full.frame = ObjectFrame::new().with_original_size(Size::new(u(7000), u(2400)));
    let bare = picture("BinData/image3.png", ImageFormat::Png);
    let zero_margin = TableMargin { left: u(0), right: u(0), top: u(0), bottom: u(0) };
    let nested = group(
        vec![
            GroupChild::Picture(picture("BinData/image4", ImageFormat::Unknown("image4".into()))),
            GroupChild::Shape(group(Vec::new(), None, None)),
        ],
        Some(floating(Some(zero_margin))),
        Some(7),
    );
    let mut top = group(
        vec![GroupChild::Picture(full), GroupChild::Shape(nested), GroupChild::Picture(bare)],
        Some(floating(None)),
        Some(1),
    );
    if let Control::Group { frame, .. } = &mut top {
        *frame = ObjectFrame::new().with_size_protect(true);
    }
    let run = Run::control(top, CharShapeIndex::new(19));
    let md = encode(vec![run.clone()]).unwrap();
    let back = MdDecoder::decode_lossless(&md).unwrap();
    assert_eq!(*first_run(&back), run, "{md}");
}

/// The spelled values do not depend on the decoder: one table spells them
/// for both sides, so a symmetric misspelling would pass the round trip.
///
/// 이것을 실패시키는 것: 열거형 철자 하나를 바꾸기(양쪽을 함께).
#[test]
fn group_markup_spells_core_names() {
    let mut p = picture("BinData/BIN0001", ImageFormat::Unknown("BIN0001".into()));
    p.placement = Some(floating(None));
    let md = encode(vec![Run::control(
        group(vec![GroupChild::Picture(p)], None, None),
        CharShapeIndex::new(3),
    )])
    .unwrap();
    for expected in [
        r#"<group data-char-shape="3" data-width-unit="47509" data-height-unit="3721""#,
        r#"<img src="BinData/BIN0001" alt="BinData/BIN0001" data-format="bin0001" data-format-raw="BIN0001""#,
        r#"data-text-wrap="InFrontOfText" data-text-flow="LargestOnly" data-treat-as-char="false" data-flow-with-text="true" data-allow-overlap="true" data-vert-rel-to="Paper" data-horz-rel-to="Column" data-vert-offset-unit="-1360" data-horz-offset-unit="5952""#,
        r#"data-vert-align="CENTER" data-horz-align="RIGHT" data-affect-line-spacing="true" data-hold-anchor="true""#,
    ] {
        assert!(md.contains(expected), "{expected}\n{md}");
    }
    assert!(!md.contains("data-out-margin"), "no margin is not written: {md}");
}

/// A shape child, an equation child and a picture caption have no grammar
/// in this wave: the whole document is refused, never a partial group.
///
/// 이것을 실패시키는 것: 도형 자식 거부를 지우고 건너뛰기 · 그림 caption 거부 삭제.
#[test]
fn group_with_what_it_cannot_hold_is_refused() {
    let rect = Control::rect(u(100), u(100)).unwrap();
    let mut captioned = picture("BinData/a.png", ImageFormat::Png);
    captioned.caption = Some(Caption::new(
        vec![para(vec![Run::text("c", CharShapeIndex::new(0))])],
        hwpforge_core::caption::CaptionSide::Bottom,
    ));
    for children in [
        vec![GroupChild::Shape(rect)],
        vec![GroupChild::Shape(group(vec![GroupChild::Shape(Control::equation("a"))], None, None))],
        vec![GroupChild::Picture(captioned)],
    ] {
        let r = encode(vec![Run::control(group(children, None, None), CharShapeIndex::new(0))]);
        assert!(matches!(r, Err(MdError::UnsupportedStructure { .. })), "{r:?}");
    }
}

/// A table caption is written as text only, so an object under it at any
/// depth (here: caption → footnote → picture, and caption → group) refuses
/// the document; a text caption is still written.
///
/// 이것을 실패시키는 것: 표 caption 안 개체 거부 삭제.
#[test]
fn table_caption_object_is_refused() {
    let table = |caption_runs: Vec<Run>| {
        let t = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![para(vec![Run::text("t", CharShapeIndex::new(0))])],
            u(9000),
        )])])
        .with_caption(Caption::new(
            vec![para(caption_runs)],
            hwpforge_core::caption::CaptionSide::Bottom,
        ));
        Run::table(t, CharShapeIndex::new(0))
    };
    let pic = Run::image(picture("BinData/a.png", ImageFormat::Png), CharShapeIndex::new(0));
    let note = Run::control(Control::footnote(vec![para(vec![pic])]), CharShapeIndex::new(0));
    let grp = Run::control(
        group(vec![GroupChild::Picture(picture("BinData/a.png", ImageFormat::Png))], None, None),
        CharShapeIndex::new(0),
    );
    for runs in [vec![note], vec![grp]] {
        assert!(matches!(encode(vec![table(runs)]), Err(MdError::UnsupportedStructure { .. })));
    }
    assert!(encode(vec![table(vec![Run::text("cap", CharShapeIndex::new(0))])]).is_ok());
}

/// Control characters in attribute values come back as written (a raw tab
/// or line break would be read back as a space); U+0000 is refused.
///
/// 이것을 실패시키는 것: `escape_attr` 를 `escape_html` 로(링크 `"a\tb"` → `"a b"`).
#[test]
fn attribute_control_characters_round_trip() {
    for url in ["a\tb", "a\nb", "a\r\nb", "a\u{1}b"] {
        let mut image = picture("BinData/a.png", ImageFormat::Png);
        image.hyperlink = Some(url.into());
        let link = Control::Hyperlink { text: "t".into(), url: url.into() };
        let grp = group(vec![GroupChild::Picture(image.clone())], None, None);
        for run in [
            Run::image(image.clone(), CharShapeIndex::new(0)),
            Run::control(link, CharShapeIndex::new(0)),
            Run::control(grp, CharShapeIndex::new(0)),
        ] {
            let md = encode(vec![run.clone()]).unwrap();
            assert_eq!(*first_run(&MdDecoder::decode_lossless(&md).unwrap()), run, "{url:?}");
        }
    }
    let link = Control::Hyperlink { text: "t".into(), url: "a\u{0}b".into() };
    assert!(matches!(
        encode(vec![Run::control(link, CharShapeIndex::new(0))]),
        Err(MdError::UnsupportedStructure { .. })
    ));
}

const SECTION: &str = r#"<section data-index="0" data-width-mm="210.0" data-height-mm="297.0"><p data-para-shape="0">"#;
const PLACE: &str = r#"data-text-wrap="Square" data-text-flow="BothSides" data-treat-as-char="false" data-flow-with-text="true" data-allow-overlap="false" data-vert-rel-to="Para" data-horz-rel-to="Para" data-vert-offset-unit="0" data-horz-offset-unit="0" data-vert-align="TOP" data-horz-align="LEFT" data-affect-line-spacing="false" data-hold-anchor="false""#;
const IMG: &str = r#"src="BinData/a.png" alt="BinData/a.png" data-format="PNG" data-width-unit="10" data-height-unit="10""#;

fn decode_body(body: &str) -> Result<Document, MdError> {
    let md = encode(vec![Run::text("t", CharShapeIndex::new(0))]).unwrap();
    let frontmatter = &md[..md.find("<section").unwrap()];
    MdDecoder::decode_lossless(&format!("{frontmatter}{SECTION}{body}</p></section>\n"))
}

/// The closed grammar: an unknown attribute, a repeated one, part of PLACE,
/// an unknown enum value, a boolean other than true/false, a char shape on
/// a group child, none on a paragraph child, an outer margin without PLACE,
/// part of the outer margin, the crop or the image extent — each refused. A failing child fails the document (no
/// group returned without it). The full set reads.
///
/// 이것을 실패시키는 것: 허용 밖 속성 무시 · 중복 검사 삭제 · PLACE 일부만 허용 ·
/// 모르는 `data-text-wrap` 를 기본값으로 · 자식 `data-char-shape` 허용 ·
/// 실패한 자식을 건너뛰고 묶음 반환 · 바깥 여백·crop·extent 일부만 허용 ·
/// PLACE 없는 바깥 여백 허용.
#[test]
fn object_attribute_grammar_errors() {
    let grp = |attrs: &str, inner: &str| {
        decode_body(&format!(
            r#"<group data-char-shape="0" data-width-unit="100" data-height-unit="100" {attrs}>{inner}</group>"#
        ))
    };
    let child = |attrs: &str| grp("", &format!("<img {IMG} {attrs} />"));
    let r = grp(PLACE, &format!("<img {IMG} {PLACE} />"));
    assert!(r.is_ok(), "{r:?}");
    assert!(matches!(grp(r#"class="x""#, ""), Err(MdError::LosslessParse { .. })));
    assert!(matches!(child(r#"class="x""#), Err(MdError::LosslessParse { .. })));
    assert!(matches!(
        grp(r#"data-inst-id="1" data-inst-id="1""#, ""),
        Err(MdError::LosslessParse { .. })
    ));
    assert!(matches!(
        child(r#"data-hold-anchor="true""#),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    let partial = PLACE.replace(r#" data-hold-anchor="false""#, "");
    assert!(matches!(
        child(&partial),
        Err(MdError::LosslessMissingAttribute { attribute: "data-hold-anchor", .. })
    ));
    assert!(matches!(
        child(&PLACE.replace("Square", "Wavy")),
        Err(MdError::LosslessInvalidAttribute { attribute: "data-text-wrap", .. })
    ));
    assert!(matches!(
        child(&PLACE.replace(r#"data-flow-with-text="true""#, r#"data-flow-with-text="1""#)),
        Err(MdError::LosslessInvalidAttribute { .. })
    ));
    assert!(matches!(child(r#"data-char-shape="0""#), Err(MdError::LosslessParse { .. })));
    assert!(matches!(
        decode_body(r#"<group data-width-unit="1" data-height-unit="1" />"#),
        Err(MdError::LosslessMissingAttribute { attribute: "data-char-shape", .. })
    ));
    assert!(matches!(
        child(
            r#"data-out-margin-left-unit="0" data-out-margin-right-unit="0" data-out-margin-top-unit="0" data-out-margin-bottom-unit="0""#
        ),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    assert!(matches!(
        child(r#"data-crop-left-unit="0" data-crop-top-unit="0""#),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    assert!(matches!(
        child(r#"data-image-extent-width-unit="10""#),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    assert!(matches!(
        child(&format!(r#"{PLACE} data-out-margin-left-unit="0""#)),
        Err(MdError::LosslessMissingAttribute { .. })
    ));
    assert!(matches!(
        grp("", &format!(r#"<img {IMG} /><img {IMG} data-width-unit="x" />"#)),
        Err(MdError::LosslessParse { .. })
    ));
    assert!(matches!(
        grp(
            "",
            &format!(
                r#"<img {IMG} /><img src="b" data-format="PNG" data-width-unit="x" data-height-unit="1" />"#
            )
        ),
        Err(MdError::LosslessInvalidAttribute { .. })
    ));
    assert!(matches!(grp("", "text"), Err(MdError::LosslessParse { .. })));
}

/// A cell background reads only six ASCII hex digits: `#가나` (six bytes,
/// two characters) is an error, not a panic, and red stays red.
///
/// 이것을 실패시키는 것: ASCII 검사 삭제(`#가나` panic) · 색 R·B 뒤바꿈.
#[test]
fn cell_background_color_is_read_strictly() {
    let body = |bg: &str| {
        decode_body(&format!(
            r#"<table data-char-shape="0"><tr><td data-col-span="1" data-row-span="1" data-width-unit="9000" data-background="{bg}"><p data-para-shape="0"><span data-char-shape="0">t</span></p></td></tr></table>"#
        ))
    };
    let r = body("#가나");
    assert!(matches!(r, Err(MdError::LosslessInvalidAttribute { .. })), "{r:?}");
    let d = body("#FF0000").unwrap();
    let hwpforge_core::RunContent::Table(t) = &first_run(&d).content else { panic!() };
    assert_eq!(
        t.rows[0].cells[0].background,
        Some(hwpforge_foundation::Color::from_rgb(255, 0, 0))
    );
    let blue = Table::new(vec![TableRow::new(vec![TableCell::new(
        vec![para(vec![Run::text("t", CharShapeIndex::new(0))])],
        u(9000),
    )
    .with_background(hwpforge_foundation::Color::from_rgb(0, 0, 255))])]);
    assert!(encode(vec![Run::table(blue, CharShapeIndex::new(0))])
        .unwrap()
        .contains(r##"data-background="#0000FF""##));
}
