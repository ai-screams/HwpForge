//! Baseline 6e6cc1be sibling order, including the table own/descend phases.
#[path = "support/document.rs"]
mod document;
use document::para as p;

use hwpforge_core::caption::{Caption, CaptionSide};
use hwpforge_core::control::{GroupChild, ShapePoint};
use hwpforge_core::image::ImageFormat;
use hwpforge_core::section::{HeaderFooter, MasterPage};
use hwpforge_core::table::{TableCell, TableRow};
use hwpforge_core::{
    Control, Document, Image, ObjectFrame, PageSettings, Paragraph, Run, Section, Table,
};
use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit};
fn control(c: Control) -> Run {
    Run::control(c, CharShapeIndex::new(0))
}
fn picture(id: &str) -> Image {
    Image::new(id, HwpUnit::ZERO, HwpUnit::ZERO, ImageFormat::Png)
}
fn leaf(id: &str) -> Run {
    Run::table(
        Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![p(vec![
                Run::text(id, CharShapeIndex::new(0)),
                Run::image(picture(id), CharShapeIndex::new(0)),
            ])],
            HwpUnit::ZERO,
        )])]),
        CharShapeIndex::new(0),
    )
}
fn siblings(id: &str) -> Vec<Paragraph> {
    (0..2).map(|pi| p((0..2).map(|ri| leaf(&format!("{id}/p{pi}/r{ri}"))).collect())).collect()
}
fn caption(id: &str) -> Caption {
    Caption::new(siblings(id), CaptionSide::Bottom)
}
fn fixture() -> Document {
    let mut table = Table::new(
        (0..2)
            .map(|row| {
                TableRow::new(
                    (0..2)
                        .map(|col| {
                            TableCell::new(siblings(&format!("matrix/{row}/{col}")), HwpUnit::ZERO)
                        })
                        .collect(),
                )
            })
            .collect(),
    );
    table.caption = Some(caption("matrix/caption"));
    let mut runs = vec![Run::table(table, CharShapeIndex::new(0))];
    let mut shapes = [
        Control::text_box(vec![], HwpUnit::ZERO, HwpUnit::ZERO),
        Control::ellipse(HwpUnit::ZERO, HwpUnit::ZERO),
        Control::polygon(vec![
            ShapePoint::new(0, 0),
            ShapePoint::new(100, 0),
            ShapePoint::new(0, 100),
        ])
        .unwrap(),
    ];
    for (i, shape) in shapes.iter_mut().enumerate() {
        match shape {
            Control::TextBox { paragraphs, caption: cap, .. }
            | Control::Ellipse { paragraphs, caption: cap, .. }
            | Control::Polygon { paragraphs, caption: cap, .. } => {
                *paragraphs = siblings(&format!("shape{i}/body"));
                *cap = Some(caption(&format!("shape{i}/caption")));
            }
            _ => unreachable!(),
        }
    }
    runs.extend(shapes.into_iter().map(control));
    runs.push(control(Control::memo_with_anchor(
        siblings("memo/content"),
        vec![leaf("memo/anchor0"), leaf("memo/anchor1")],
    )));
    let mut image = picture("group/picture");
    image.caption = Some(caption("group/picture/caption"));
    runs.push(control(Control::Group {
        children: vec![
            GroupChild::Shape(Control::text_box(
                siblings("group/shape0"),
                HwpUnit::ZERO,
                HwpUnit::ZERO,
            )),
            GroupChild::Picture(image),
            GroupChild::Shape(Control::text_box(
                siblings("group/shape2"),
                HwpUnit::ZERO,
                HwpUnit::ZERO,
            )),
        ],
        width: HwpUnit::ZERO,
        height: HwpUnit::ZERO,
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    }));
    let mut image = picture("run/picture");
    image.caption = Some(caption("run/picture/caption"));
    runs.push(Run::image(image, CharShapeIndex::new(0)));
    // Nest the multi-run root too: every reverse iterator in the engine sees siblings.
    let mut section = Section::with_paragraphs(
        vec![
            p(vec![
                control(Control::text_box(vec![p(runs)], HwpUnit::ZERO, HwpUnit::ZERO)),
                leaf("root/sibling"),
            ]),
            p(vec![leaf("root/second")]),
        ],
        PageSettings::a4(),
    );
    for i in 0..2 {
        section
            .headers
            .push(HeaderFooter::new(siblings(&format!("header{i}")), ApplyPageType::Both));
        section
            .footers
            .push(HeaderFooter::new(siblings(&format!("footer{i}")), ApplyPageType::Both));
    }
    section.master_pages = Some(
        (0..2)
            .map(|i| MasterPage::new(ApplyPageType::Both, siblings(&format!("master{i}"))))
            .collect(),
    );
    let mut d = Document::new();
    d.add_section(section);
    d
}
fn table_id(t: &Table) -> String {
    t.rows[0].cells[0].paragraphs[0].text_content()
}
// 이것을 실패시키는 것: walk.rs 각 .rev() 삭제, shape 본문/캡션 push 교환,
// Memo content/anchor push 교환. 불변·가변 로그를 각각 기준 실행 파일에 고정한다.
#[test]
fn walker_siblings_baseline() {
    let mut d = fixture();
    let mut logs = Vec::new();
    let mut values = Vec::new();
    d.for_each_paragraph(|p| values.push(p.text_content()));
    logs.push(format!("P={values:?}"));
    let mut values = Vec::new();
    d.for_each_paragraph_mut(|p| values.push(p.text_content()));
    logs.push(format!("PM={values:?}"));
    let mut values = Vec::new();
    d.for_each_table(|t| values.push(table_id(t)));
    logs.push(format!("T={values:?}"));
    let mut values = Vec::new();
    d.for_each_table_mut(|t| values.push(table_id(t)));
    logs.push(format!("TM={values:?}"));
    // Each top-level paragraph owns its image traversal; nested paragraphs must not be visited twice.
    let s = &mut d.sections_mut()[0];
    let mut values = Vec::new();
    for p in s
        .paragraphs
        .iter()
        .chain(s.headers.iter().chain(&s.footers).flat_map(|b| &b.paragraphs))
        .chain(s.master_pages.iter().flatten().flat_map(|b| &b.paragraphs))
    {
        p.for_each_image(|i| values.push(i.path.clone()));
    }
    logs.push(format!("I={values:?}"));
    let mut values = Vec::new();
    for p in s
        .paragraphs
        .iter_mut()
        .chain(s.headers.iter_mut().chain(&mut s.footers).flat_map(|b| &mut b.paragraphs))
        .chain(s.master_pages.iter_mut().flatten().flat_map(|b| &mut b.paragraphs))
    {
        p.for_each_image_mut(|i| values.push(i.path.clone()));
    }
    logs.push(format!("IM={values:?}"));
    let log = logs.join("\n") + "\n";
    println!("BASELINE_BEGIN\n{log}BASELINE_END");
    assert_eq!(log, include_str!("walker_siblings_baseline.txt"));
}
