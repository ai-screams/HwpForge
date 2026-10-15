//! The lossless view limits how deep containers nest (tables, groups,
//! pictures, captions, shapes, notes), in both directions, before building
//! or walking the tree — so a deep input is an error, not a stack overflow.

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{Control, GroupChild};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, ObjectFrame, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_md::{MdDecoder, MdEncoder, MdError};

/// The limit (`MAX_NESTING_DEPTH`), the same as the HWPX reader's.
const LIMIT: usize = 32;

fn u(v: i32) -> HwpUnit {
    HwpUnit::new(v).unwrap()
}

fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}

fn encode(run: Run) -> Result<String, MdError> {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(vec![para(vec![run])], PageSettings::a4()));
    MdEncoder::encode_lossless(&d.validate().unwrap())
}

/// `n` groups, one inside the other, around a picture.
fn groups(n: usize) -> Control {
    let mut c = Control::Group {
        children: vec![GroupChild::Picture(Image::new("a", u(10), u(10), ImageFormat::Png))],
        width: u(10),
        height: u(10),
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    for _ in 1..n {
        c = Control::Group {
            children: vec![GroupChild::Shape(c)],
            width: u(10),
            height: u(10),
            frame: ObjectFrame::new(),
            placement: None,
            inst_id: None,
        };
    }
    c
}

/// `n` times picture → caption → table → cell, the review's path (each
/// round is three containers: picture, caption, table).
fn caption_chain(n: usize) -> Run {
    let mut run = Run::text("x", CharShapeIndex::new(0));
    for _ in 0..n {
        let table =
            Table::new(vec![TableRow::new(vec![TableCell::new(vec![para(vec![run])], u(100))])]);
        let mut image = Image::new("a", u(10), u(10), ImageFormat::Png);
        image.caption = Some(Caption::new(
            vec![para(vec![Run::table(table, CharShapeIndex::new(0))])],
            CaptionSide::Bottom,
        ));
        run = Run::image(image, CharShapeIndex::new(0));
    }
    run
}

/// `n` text boxes, one in the other's text.
fn boxes(n: usize) -> Run {
    boxes_around(n, Run::text("x", CharShapeIndex::new(0)))
}

/// `n` text boxes around `run`.
fn boxes_around(n: usize, mut run: Run) -> Run {
    for _ in 0..n {
        run = Run::control(
            Control::text_box(vec![para(vec![run])], u(10), u(10)),
            CharShapeIndex::new(0),
        );
    }
    run
}

/// Encoder: depth 32 is written and read back; 33 is refused — for nested
/// groups (the group plus its picture are the containers), for the
/// picture → caption → table path and for text boxes in text boxes.
///
/// 이것을 실패시키는 것: 인코더의 깊이 검사 삭제 · 한도 비교 하나 어긋남(31·33) ·
/// 빈 묶음·선의 자기 깊이 검사 각각 삭제.
#[test]
fn encoder_limits_nesting_depth() {
    // groups(n) is n groups and one picture: n + 1 containers.
    let ok = Run::control(groups(LIMIT - 1), CharShapeIndex::new(0));
    let md = encode(ok.clone()).unwrap();
    assert_eq!(MdDecoder::decode_lossless(&md).unwrap().sections()[0].paragraphs[0].runs[0], ok);
    let deep = Run::control(groups(LIMIT), CharShapeIndex::new(0));
    assert!(matches!(encode(deep), Err(MdError::UnsupportedStructure { .. })));
    // caption_chain(10) = 30 containers, (11) = 33.
    let md = encode(caption_chain(10)).unwrap();
    assert!(MdDecoder::decode_lossless(&md).is_ok());
    assert!(matches!(encode(caption_chain(11)), Err(MdError::UnsupportedStructure { .. })));
    assert!(encode(boxes(LIMIT)).is_ok());
    assert!(matches!(encode(boxes(LIMIT + 1)), Err(MdError::UnsupportedStructure { .. })));
    // A container holding nothing still counts: an empty group, a line, a
    // table at depth 33 (each written as an element the decoder counts).
    let empty_group = Control::Group {
        children: Vec::new(),
        width: u(10),
        height: u(10),
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    };
    let line = Control::line(
        hwpforge_core::control::ShapePoint::new(0, 0),
        hwpforge_core::control::ShapePoint::new(10, 0),
    )
    .unwrap();
    let table = || {
        Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![para(vec![Run::text("t", CharShapeIndex::new(0))])],
                u(100),
            )])]),
            CharShapeIndex::new(0),
        )
    };
    for (name, leaf) in [
        ("group", Run::control(empty_group, CharShapeIndex::new(0))),
        ("line", Run::control(line, CharShapeIndex::new(0))),
        ("table", table()),
    ] {
        assert!(encode(boxes_around(LIMIT - 1, leaf.clone())).is_ok(), "{name}");
        assert!(
            matches!(encode(boxes_around(LIMIT, leaf)), Err(MdError::UnsupportedStructure { .. })),
            "{name}"
        );
    }
}

fn frontmatter() -> String {
    let md = encode(Run::text("t", CharShapeIndex::new(0))).unwrap();
    md[..md.find("<section").unwrap()].to_string()
}

fn decode(body: &str) -> Result<Document, MdError> {
    MdDecoder::decode_lossless(&format!(
        "{}<section data-index=\"0\" data-width-mm=\"210.0\" data-height-mm=\"297.0\"><p data-para-shape=\"0\">{body}</p></section>\n",
        frontmatter()
    ))
}

const BOX: &str = r#"data-width-unit="10" data-height-unit="10""#;

/// `n` nested `<group>`s around an `<img/>` (n + 1 containers), then a bad
/// sibling.
fn group_markup(n: usize, tail: &str) -> String {
    let mut s = format!(r#"<group data-char-shape="0" {BOX}>"#);
    for _ in 1..n {
        s.push_str(&format!("<group {BOX}>"));
    }
    s.push_str(&format!(r#"<img src="a" alt="a" data-format="PNG" {BOX} />"#));
    for _ in 0..n {
        s.push_str("</group>");
    }
    s.push_str(tail);
    s
}

/// Decoder: 32 containers read, 33 refused before the tree is built — also
/// a hundred thousand of them followed by a bad sibling (which used to
/// build the whole tree and could overflow the stack dropping it), and the
/// picture → caption → table → cell path.
///
/// 이것을 실패시키는 것: 디코더의 깊이 검사 삭제(10 만 단계에서 스택 오버플로) ·
/// 한도 비교를 `>` 로 · 묶음만 세기(캡션 경로가 빠져나감).
#[test]
fn decoder_limits_nesting_depth() {
    assert!(decode(&group_markup(LIMIT - 1, "")).is_ok());
    assert!(matches!(decode(&group_markup(LIMIT, "")), Err(MdError::LosslessParse { .. })));
    assert!(matches!(
        decode(&group_markup(100_000, "<bogus/>")),
        Err(MdError::LosslessParse { detail }) if detail.contains("deeper")
    ));
    let chain = |n: usize| {
        let open = r#"<img data-char-shape="0" src="a" alt="a" data-format="PNG" data-width-unit="10" data-height-unit="10"><caption data-side="Bottom" data-gap-unit="0"><p data-para-shape="0"><table data-char-shape="0"><tr><td data-col-span="1" data-row-span="1" data-width-unit="100"><p data-para-shape="0">"#;
        let close = "</p></td></tr></table></p></caption></img>";
        format!("{}<span data-char-shape=\"0\">x</span>{}<bogus/>", open.repeat(n), close.repeat(n))
    };
    // 10 rounds = 30 containers: only the bad sibling fails.
    assert!(
        matches!(decode(&chain(10)), Err(MdError::LosslessParse { detail }) if detail.contains("bogus"))
    );
    assert!(
        matches!(decode(&chain(11)), Err(MdError::LosslessParse { detail }) if detail.contains("deeper"))
    );
    assert!(
        matches!(decode(&chain(50_000)), Err(MdError::LosslessParse { detail }) if detail.contains("deeper"))
    );
}
