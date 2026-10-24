use hwpforge_core::control::{FieldBoundary, FieldStart, GroupChild};
use hwpforge_core::section::{HeaderFooter, MasterPage};
use hwpforge_core::table::{Table, TableCell, TableRow};
use hwpforge_core::{Control, Document, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};
fn text(s: &str) -> Run {
    Run::text(s, CharShapeIndex::new(0))
}
fn control(c: Control) -> Run {
    Run::control(c, CharShapeIndex::new(0))
}
fn para(runs: Vec<Run>) -> Paragraph {
    Paragraph::with_runs(runs, ParaShapeIndex::new(0))
}
fn start(id: &str, kind: &str) -> Run {
    let mut s = FieldStart::default();
    s.id = id.into();
    s.kind = kind.into();
    s.name = format!("name-{id}");
    control(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) })
}
fn end(id: &str) -> Run {
    control(Control::FieldBoundary {
        boundary: FieldBoundary::End { begin_id: id.into(), field_id: String::new() },
    })
}
fn boxed(ps: Vec<Paragraph>) -> Run {
    control(Control::text_box(ps, HwpUnit::ZERO, HwpUnit::ZERO))
}
fn table() -> Run {
    Run::table(
        Table::new(vec![TableRow::new(vec![
            TableCell::new(vec![para(vec![text("Name")])], HwpUnit::ZERO),
            TableCell::new(vec![para(vec![text("")])], HwpUnit::ZERO),
            TableCell::new(
                vec![para(vec![start("cell", "CLICK_HERE"), text("@ "), end("cell")])],
                HwpUnit::ZERO,
            ),
        ])]),
        CharShapeIndex::new(0),
    )
}
pub fn fixtures() -> Vec<(&'static str, Document)> {
    let make = |ps| {
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(ps, PageSettings::a4()));
        d
    };
    let mut plain = make(vec![
        para(vec![text("@ "), boxed(vec![para(vec![]), para(vec![text("@ ")])]), text("tail")]),
        para(vec![]),
    ]);
    plain.sections_mut()[0]
        .headers
        .push(HeaderFooter::new(vec![para(vec![text("single")])], ApplyPageType::Both));
    let nested = make(vec![
        para(vec![
            start("outer", "FORMULA"),
            boxed(vec![
                para(vec![]),
                para(vec![text("protected plain")]),
                para(vec![start("inner", "CLICK_HERE"), text("value"), end("inner")]),
                para(vec![boxed(vec![para(vec![text("nested plain")])])]),
            ]),
            table(),
            end("outer"),
            text("@ "),
        ]),
        para(vec![table()]),
    ]);
    let memo = make(vec![para(vec![
        control(Control::memo_with_anchor(
            vec![para(vec![text("memo content")])],
            vec![
                text("@ "),
                start("anchor", "CLICK_HERE"),
                text("anchor value"),
                end("anchor"),
                control(Control::memo_with_anchor(vec![], vec![text("nested anchor")])),
            ],
        )),
        text("@ "),
    ])]);
    let malformed = make(vec![para(vec![
        end("missing"),
        start("duplicate", "CLICK_HERE"),
        text("x"),
        start("duplicate", "FORMULA"),
    ])]);
    let mut bands = make(vec![para(vec![
        control(Control::field("hint")),
        control(Control::bookmark("mark")),
        control(Control::hyperlink("link", "https://example.org")),
    ])]);
    for i in 0..2 {
        bands.sections_mut()[0].headers.push(HeaderFooter::new(
            vec![para(vec![
                start(&format!("h{i}"), "CLICK_HERE"),
                text("@ "),
                end(&format!("h{i}")),
            ])],
            ApplyPageType::Both,
        ));
        bands.sections_mut()[0]
            .footers
            .push(HeaderFooter::new(vec![para(vec![text("footer @ ")])], ApplyPageType::Both));
    }
    bands.sections_mut()[0].master_pages = Some(vec![MasterPage::new(
        ApplyPageType::Both,
        vec![para(vec![start("master", "CLICK_HERE"), text("master"), end("master")])],
    )]);
    bands.add_section(Section::with_paragraphs(
        vec![para(vec![text("section two @ ")])],
        PageSettings::a4(),
    ));
    let group = make(vec![para(vec![
        control(Control::Group {
            children: vec![
                GroupChild::Shape(Control::text_box(
                    vec![para(vec![text("@ ")])],
                    HwpUnit::ZERO,
                    HwpUnit::ZERO,
                )),
                GroupChild::Shape(Control::footnote(vec![para(vec![
                    start("note", "CLICK_HERE"),
                    text("note"),
                    end("note"),
                ])])),
            ],
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            inst_id: None,
        }),
        text("after group"),
    ])]);
    let mut p = para(vec![start("deep", "CLICK_HERE"), text("@ "), end("deep"), table()]);
    for _ in 0..33 {
        p = para(vec![boxed(vec![p])]);
    }
    let mut cases = vec![
        ("plain", plain),
        ("nested", nested),
        ("memo", memo),
        ("malformed", malformed),
        ("bands", bands),
        ("group", group),
        ("depth33", make(vec![p])),
    ];
    cases.extend(review_fixtures());
    cases
}

fn start_ann(id: &str, kind: &str, ann: Option<Vec<Paragraph>>) -> Run {
    let mut s = FieldStart::default();
    s.id = id.into();
    s.kind = kind.into();
    s.name = format!("name-{id}");
    s.annotation = ann;
    control(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) })
}

fn review_fixtures() -> Vec<(&'static str, Document)> {
    let make = |ps| {
        let mut d = Document::new();
        d.add_section(Section::with_paragraphs(ps, PageSettings::a4()));
        d
    };
    let span = make(vec![
        para(vec![
            text("pre @ "),
            start("o", "FORMULA"),
            boxed(vec![para(vec![text("a @ ")]), para(vec![])]),
            text("mid"),
        ]),
        para(vec![boxed(vec![para(vec![boxed(vec![para(vec![text("deep @ ")])])])])]),
        para(vec![
            boxed(vec![para(vec![text("x")])]),
            end("o"),
            boxed(vec![para(vec![text("after @ ")])]),
        ]),
    ]);
    let ann = make(vec![para(vec![
        start("o", "FORMULA"),
        start_ann(
            "i",
            "CLICK_HERE",
            Some(vec![para(vec![]), para(vec![boxed(vec![para(vec![text("ann @ ")])])])]),
        ),
        text("v"),
        end("i"),
        end("o"),
        start_ann(
            "free",
            "FORMULA",
            Some(vec![para(vec![boxed(vec![para(vec![text("free ann")])])])]),
        ),
        end("free"),
    ])]);
    let memo_in_body = make(vec![para(vec![
        start("o", "FORMULA"),
        control(Control::memo_with_anchor(
            vec![para(vec![boxed(vec![para(vec![text("mc @ ")])])])],
            vec![text("plain anchor @ "), boxed(vec![para(vec![text("in anchor box")])])],
        )),
        end("o"),
        control(Control::memo_with_anchor(vec![], vec![])),
    ])]);
    let three = make(vec![para(vec![
        start("o", "FORMULA"),
        boxed(vec![para(vec![boxed(vec![para(vec![
            start("i", "CLICK_HERE"),
            text("@ "),
            end("i"),
            boxed(vec![para(vec![]), para(vec![text("leaf @ ")])]),
        ])])])]),
        end("o"),
        boxed(vec![para(vec![boxed(vec![para(vec![
            control(Control::bookmark("bm")),
            boxed(vec![para(vec![text("under bm")])]),
        ])])])]),
    ])]);
    let group_body = make(vec![para(vec![
        start("o", "FORMULA"),
        control(Control::Group {
            children: vec![
                GroupChild::Shape(Control::text_box(
                    vec![para(vec![text("g @ ")])],
                    HwpUnit::ZERO,
                    HwpUnit::ZERO,
                )),
                GroupChild::Shape(Control::Group {
                    children: vec![GroupChild::Shape(Control::text_box(
                        vec![para(vec![]), para(vec![text("gg")])],
                        HwpUnit::ZERO,
                        HwpUnit::ZERO,
                    ))],
                    width: HwpUnit::ZERO,
                    height: HwpUnit::ZERO,
                    frame: hwpforge_core::ObjectFrame::new(),
                    placement: None,
                    inst_id: None,
                }),
            ],
            width: HwpUnit::ZERO,
            height: HwpUnit::ZERO,
            frame: hwpforge_core::ObjectFrame::new(),
            placement: None,
            inst_id: None,
        }),
        end("o"),
    ])]);
    vec![
        ("span", span),
        ("ann", ann),
        ("memo_in_body", memo_in_body),
        ("three", three),
        ("group_body", group_body),
    ]
}
