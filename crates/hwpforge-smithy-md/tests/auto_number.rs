//! Caption numbers in Markdown (§19.16 J, P19): the lossy and styled views
//! write the number Hancom cached between its prefix and suffix; the
//! lossless grammar has no place for it and refuses the document rather
//! than dropping the number.

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{AutoNumberFormat, AutoNumberKind, Control};
use hwpforge_core::image::{Image, ImageFormat};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Document, PageSettings, Paragraph, Run, Section, StyleLookup, Validated};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};
use hwpforge_smithy_hwpx::{HwpxDecoder, HwpxStyleLookup};
use hwpforge_smithy_md::{MdEncoder, MdError};

struct PlainStyles;

impl StyleLookup for PlainStyles {}

fn caption(format: AutoNumberFormat) -> Document<Validated> {
    let number = Control::AutoNumber { kind: AutoNumberKind::Picture, num: 3, format };
    let para = Paragraph::with_runs(
        vec![
            Run::text("그림 ", CharShapeIndex::new(0)),
            Run::control(number, CharShapeIndex::new(0)),
            Run::text(" 조직도", CharShapeIndex::new(0)),
        ],
        ParaShapeIndex::new(0),
    );
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(vec![para], PageSettings::a4()));
    doc.validate().expect("validate")
}

/// "그림 3 조직도" in both readable views; "(3)" with a prefix and suffix;
/// the lossless encoder refuses.
///
/// 이것을 실패시키는 것: lossy·styled 의 `AutoNumber` 갈래를 지우는 것(번호가
/// 사라짐) / 접두·접미를 버리는 것 / lossless 가 번호를 조용히 버리는 것.
#[test]
fn caption_number_is_written_or_refused() {
    let plain = caption(AutoNumberFormat::default());
    assert!(MdEncoder::encode_lossy(&plain).unwrap().contains("그림 3 조직도"));
    assert!(MdEncoder::encode_styled(&plain, &PlainStyles).markdown.contains("그림 3 조직도"));
    let wrapped = caption(AutoNumberFormat {
        prefix: Some('('),
        suffix: Some(')'),
        ..AutoNumberFormat::default()
    });
    assert!(MdEncoder::encode_lossy(&wrapped).unwrap().contains("그림 (3) 조직도"));
    assert!(MdEncoder::encode_styled(&wrapped, &PlainStyles).markdown.contains("그림 (3) 조직도"));
    assert!(matches!(
        MdEncoder::encode_lossless(&plain),
        Err(MdError::UnsupportedStructure { .. })
    ));
}

/// F17's numbers all sit in captions: both readable views write the
/// captions after their picture or table, so "그림 3", "그림 4" and "표 7"
/// reach the Markdown; the lossless encoder refuses the document (review
/// r1: the captions were not written at all, and lossless dropped the
/// numbers without a word).
///
/// 이것을 실패시키는 것: 두 보기의 캡션 출력을 지우기 / lossless 의 캡션 검사를
/// 지우기(그림 캡션은 Core 문단 순회에 없다).
#[test]
fn f17_caption_numbers_reach_the_readable_views() {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/images/f17-caption-numbering.hwpx"
    );
    let decoded = HwpxDecoder::decode(&std::fs::read(path).expect("F17")).expect("decode");
    let doc = decoded.document.validate().expect("validate");
    let lookup = HwpxStyleLookup::new(&decoded.style_store, &decoded.image_store);
    let lossy = MdEncoder::encode_lossy(&doc).expect("lossy");
    let styled = MdEncoder::encode_styled(&doc, &lookup).markdown;
    for view in [&lossy, &styled] {
        for number in ["그림 3", "그림 4", "표 7"] {
            assert!(view.contains(number), "{number}: {view}");
        }
    }
    assert!(matches!(MdEncoder::encode_lossless(&doc), Err(MdError::UnsupportedStructure { .. })));
}

/// A `*` prefix and suffix stay characters in the styled view, not an
/// emphasis around the number.
///
/// 이것을 실패시키는 것: styled 의 캡션 번호 이스케이프를 지우기(`*3*`).
#[test]
fn caption_number_prefix_is_escaped() {
    let starred = caption(AutoNumberFormat {
        prefix: Some('*'),
        suffix: Some('*'),
        ..AutoNumberFormat::default()
    });
    let md = MdEncoder::encode_styled(&starred, &PlainStyles).markdown;
    assert!(md.contains(r"\*3\*"), "{md}");
}

fn numbered_caption(label: &str, kind: AutoNumberKind, num: u32) -> Caption {
    let number = Control::AutoNumber { kind, num, format: AutoNumberFormat::default() };
    Caption::new(
        vec![Paragraph::with_runs(
            vec![
                Run::text(label, CharShapeIndex::new(0)),
                Run::control(number, CharShapeIndex::new(0)),
            ],
            ParaShapeIndex::new(0),
        )],
        CaptionSide::Bottom,
    )
}

fn picture(caption: Option<Caption>) -> Image {
    let size = HwpUnit::new(1000).unwrap();
    let image = Image::new("BinData/a.png", size, size, ImageFormat::Png);
    match caption {
        Some(caption) => image.with_caption(caption),
        None => image,
    }
}

fn doc_of(paragraphs: Vec<Paragraph>) -> Document<Validated> {
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(paragraphs, PageSettings::a4()));
    doc.validate().expect("validate")
}

/// A picture alone in its paragraph and a table after text — the two
/// placements F17 does not have — write their caption numbers in both
/// readable views; a picture caption's number alone makes lossless refuse,
/// and the same picture without a caption does not.
///
/// 이것을 실패시키는 것: 단독 그림의 캡션 출력을 지우기 / 글 뒤 표의 캡션
/// 출력을 지우기(두 보기 각각) / lossless 의 그림 캡션 검사를 지우기.
#[test]
fn lone_picture_and_inline_table_captions_are_written() {
    let lone = Paragraph::with_runs(
        vec![Run::image(
            picture(Some(numbered_caption("그림 ", AutoNumberKind::Picture, 1))),
            CharShapeIndex::new(0),
        )],
        ParaShapeIndex::new(0),
    );
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("칸", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        HwpUnit::new(1000).unwrap(),
    );
    let table = Table::new(vec![TableRow::new(vec![cell])]).with_caption(numbered_caption(
        "표 ",
        AutoNumberKind::Table,
        2,
    ));
    let after_text = Paragraph::with_runs(
        vec![Run::text("앞", CharShapeIndex::new(0)), Run::table(table, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let doc = doc_of(vec![lone.clone(), after_text]);
    let lossy = MdEncoder::encode_lossy(&doc).expect("lossy");
    let styled = MdEncoder::encode_styled(&doc, &PlainStyles).markdown;
    for view in [&lossy, &styled] {
        for number in ["그림 1", "표 2"] {
            assert!(view.contains(number), "{number}: {view}");
        }
    }

    assert!(matches!(
        MdEncoder::encode_lossless(&doc_of(vec![lone])),
        Err(MdError::UnsupportedStructure { .. })
    ));
    let plain = Paragraph::with_runs(
        vec![Run::image(picture(None), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    MdEncoder::encode_lossless(&doc_of(vec![plain])).expect("uncaptioned picture");
}

/// Every picture's bytes are known (so caption pictures can be checked).
struct PictureStyles;

impl StyleLookup for PictureStyles {
    fn image_data(&self, key: &str) -> Option<&[u8]> {
        key.contains("inner").then_some(b"inner-bytes".as_slice())
    }
}

/// In a merged-cell table (the HTML path) a picture's and a nested table's
/// caption numbers are written; and a picture inside a caption keeps its
/// bytes next to its link (review r2: the HTML path wrote no captions, and
/// caption pictures were linked without their bytes).
///
/// 이것을 실패시키는 것: HTML 칸의 그림 캡션 / 표 캡션 출력을 지우기 / 캡션
/// 그림의 바이트를 버리기.
#[test]
fn caption_pictures_and_html_table_captions_are_kept() {
    let unit = HwpUnit::new(1000).unwrap();
    let cell = |runs: Vec<Run>| {
        TableCell::new(vec![Paragraph::with_runs(runs, ParaShapeIndex::new(0))], unit)
    };
    let inner_table =
        Table::new(vec![TableRow::new(vec![cell(vec![Run::text("안", CharShapeIndex::new(0))])])])
            .with_caption(numbered_caption("표 ", AutoNumberKind::Table, 6));
    let mut merged = cell(vec![
        Run::image(
            picture(Some(numbered_caption("그림 ", AutoNumberKind::Picture, 5))),
            CharShapeIndex::new(0),
        ),
        Run::table(inner_table, CharShapeIndex::new(0)),
    ]);
    merged.col_span = 2;
    let outer = Table::new(vec![
        TableRow::new(vec![merged]),
        TableRow::new(vec![
            cell(vec![Run::text("a", CharShapeIndex::new(0))]),
            cell(vec![Run::text("b", CharShapeIndex::new(0))]),
        ]),
    ]);
    let doc = doc_of(vec![Paragraph::with_runs(
        vec![Run::table(outer, CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    )]);
    let styled = MdEncoder::encode_styled(&doc, &PlainStyles).markdown;
    assert!(styled.contains("<table>"), "{styled}");
    for number in ["그림 5", "표 6"] {
        assert!(styled.contains(number), "{number}: {styled}");
    }

    // A caption holding the picture `inner{n}.png`, in each place a caption
    // is written: a lone picture, a picture after text, a lone table, a
    // table after text, and a picture and a table in an HTML cell.
    let with_inner = |n: u32| {
        let inner = Image::new(format!("BinData/inner{n}.png"), unit, unit, ImageFormat::Png);
        Caption::new(
            vec![Paragraph::with_runs(
                vec![Run::image(inner, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            CaptionSide::Bottom,
        )
    };
    let text = || Run::text("앞", CharShapeIndex::new(0));
    let pic = |n| Run::image(picture(None).with_caption(with_inner(n)), CharShapeIndex::new(0));
    let tbl = |n| {
        Run::table(
            Table::new(vec![TableRow::new(vec![cell(vec![text()])])]).with_caption(with_inner(n)),
            CharShapeIndex::new(0),
        )
    };
    let para = |runs| Paragraph::with_runs(runs, ParaShapeIndex::new(0));
    let mut html_cell = cell(vec![pic(5), tbl(6)]);
    html_cell.col_span = 2;
    let html = Table::new(vec![
        TableRow::new(vec![html_cell]),
        TableRow::new(vec![cell(vec![text()]), cell(vec![text()])]),
    ]);
    let doc = doc_of(vec![
        para(vec![pic(1)]),
        para(vec![text(), pic(2)]),
        para(vec![tbl(3)]),
        para(vec![text(), tbl(4)]),
        para(vec![Run::table(html, CharShapeIndex::new(0))]),
    ]);
    let out = MdEncoder::encode_styled(&doc, &PictureStyles);
    for n in 1..=6 {
        let key = format!("images/inner{n}.png");
        assert!(out.markdown.contains(&key), "{key}: {}", out.markdown);
        assert_eq!(
            out.images.get(&key).map(Vec::as_slice),
            Some(b"inner-bytes".as_slice()),
            "{key}"
        );
    }
}

/// §19.22 D5: a table paragraph with an end-mark char shape is still a
/// table block in both readable views (the end mark is not a run).
#[test]
fn table_paragraph_with_an_end_mark_stays_a_table_block() {
    let unit = HwpUnit::new(1000).unwrap();
    let cell = TableCell::new(
        vec![Paragraph::with_runs(
            vec![Run::text("칸", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        unit,
    );
    let mut host = Paragraph::with_runs(
        vec![Run::table(Table::new(vec![TableRow::new(vec![cell])]), CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    host.end_char_shape = Some(CharShapeIndex::new(4));
    let doc = doc_of(vec![host]);
    let lossy = MdEncoder::encode_lossy(&doc).expect("lossy");
    let styled = MdEncoder::encode_styled(&doc, &PlainStyles).markdown;
    for view in [&lossy, &styled] {
        assert!(view.trim_start().starts_with('|'), "{view}");
    }
}
