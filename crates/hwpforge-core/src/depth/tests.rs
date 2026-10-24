use super::*;
use crate::caption::{Caption, CaptionSide};
use crate::control::{FieldStart, ShapePoint};
use crate::image::ImageFormat;
use crate::section::{HeaderFooter, MasterPage};
use crate::table::{TableCell, TableRow};
use crate::{Document, ObjectFrame, PageSettings};
use hwpforge_foundation::{ApplyPageType, ArcType, CharShapeIndex, HwpUnit, ParaShapeIndex};

fn u() -> HwpUnit {
    HwpUnit::new(100).unwrap()
}
fn text() -> Run {
    Run::text("leaf", CharShapeIndex::new(0))
}
fn ps(r: Run) -> Vec<Paragraph> {
    vec![Paragraph::with_runs(vec![r], ParaShapeIndex::new(0))]
}
fn ctrl(c: Control) -> Run {
    Run::control(c, CharShapeIndex::new(0))
}
fn group(children: Vec<GroupChild>) -> Control {
    Control::Group {
        children,
        width: u(),
        height: u(),
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    }
}
fn document(r: Run) -> Document {
    let mut d = Document::new();
    d.add_section(Section::with_paragraphs(ps(r), PageSettings::a4()));
    d
}
fn boxes(mut r: Run, n: usize) -> Run {
    for _ in 0..n {
        r = ctrl(Control::text_box(ps(r), u(), u()));
    }
    r
}
fn cap(r: Run) -> Caption {
    Caption::new(ps(r), CaptionSide::Bottom)
}

// Exhaustive ownership-edge fixtures: cost and independent node counts below
// lock measurement and cleanup to the same tree (including siblings).
const EDGES: &[(&str, usize, usize)] = &[
    ("table cell", 1, 3),
    ("table caption", 1, 3),
    ("image caption", 2, 3),
    ("textbox body", 1, 3),
    ("ellipse body", 1, 3),
    ("polygon body", 1, 3),
    ("textbox caption", 2, 3),
    ("ellipse caption", 2, 3),
    ("polygon caption", 2, 3),
    ("line caption", 2, 3),
    ("rect caption", 2, 3),
    ("arc caption", 2, 3),
    ("curve caption", 2, 3),
    ("connect caption", 2, 3),
    ("footnote", 1, 3),
    ("endnote", 1, 3),
    ("memo content", 1, 3),
    ("memo anchor", 1, 2),
    ("field annotation", 1, 3),
    ("group shape", 2, 4),
    ("group picture", 3, 4),
];
fn wrap(edge: &str, r: Run) -> Run {
    match edge {
        "table cell" => Run::table(
            Table::new(vec![TableRow::new(vec![TableCell::new(ps(r), u())])]),
            CharShapeIndex::new(0),
        ),
        "table caption" => {
            let mut t = Table::new(vec![]);
            t.caption = Some(cap(r));
            Run::table(t, CharShapeIndex::new(0))
        }
        "image caption" | "group picture" => {
            let mut i = Image::new("a", u(), u(), ImageFormat::Png);
            i.caption = Some(cap(r));
            if edge == "group picture" {
                ctrl(group(vec![GroupChild::Picture(i)]))
            } else {
                Run::image(i, CharShapeIndex::new(0))
            }
        }
        "textbox body" => ctrl(Control::text_box(ps(r), u(), u())),
        "ellipse body" => ctrl(Control::ellipse_with_text(u(), u(), ps(r))),
        "polygon body" => {
            let mut c = polygon();
            if let Control::Polygon { paragraphs, .. } = &mut c {
                *paragraphs = ps(r);
            }
            ctrl(c)
        }
        "footnote" => ctrl(Control::footnote(ps(r))),
        "endnote" => ctrl(Control::endnote(ps(r))),
        "memo content" => ctrl(Control::memo(ps(r))),
        "memo anchor" => ctrl(Control::memo_with_anchor(vec![], vec![r])),
        "field annotation" => {
            let s = FieldStart { annotation: Some(ps(r)), ..FieldStart::default() };
            ctrl(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::new(s)) })
        }
        "group shape" => ctrl(group(vec![GroupChild::Shape(Control::text_box(ps(r), u(), u()))])),
        other => {
            let a = ShapePoint::new(0, 0);
            let b = ShapePoint::new(100, 100);
            let mut c = match other {
                "textbox caption" => Control::text_box(vec![], u(), u()),
                "ellipse caption" => Control::ellipse(u(), u()),
                "polygon caption" => polygon(),
                "line caption" => Control::line(a, b).unwrap(),
                "rect caption" => Control::rect(u(), u()).unwrap(),
                "arc caption" => Control::arc(ArcType::default(), u(), u()),
                "curve caption" => Control::curve(vec![a, b]).unwrap(),
                "connect caption" => Control::connect_line(a, b).unwrap(),
                _ => panic!("unknown fixture"),
            };
            match &mut c {
                Control::TextBox { caption, .. }
                | Control::Ellipse { caption, .. }
                | Control::Polygon { caption, .. }
                | Control::Line { caption, .. }
                | Control::Rect { caption, .. }
                | Control::Arc { caption, .. }
                | Control::Curve { caption, .. }
                | Control::ConnectLine { caption, .. } => *caption = Some(cap(r)),
                _ => unreachable!(),
            }
            ctrl(c)
        }
    }
}
fn polygon() -> Control {
    Control::polygon(vec![ShapePoint::new(0, 0), ShapePoint::new(100, 0), ShapePoint::new(0, 100)])
        .unwrap()
}

// 이것을 실패시키는 것: LIMIT 31/33, Object/Caption 비용 0,
// 각 소유 간선 예약 삭제(측정/해체 각각), 형제 깊이 합산.
// 이것을 실패시키는 것: Run의 Table 자식 떼기 삭제(W1 노드 대조).
#[test]
fn every_edge_boundaries_and_cleanup() {
    for &(edge, cost, nodes) in EDGES {
        for depth in [31, 32, 33] {
            let mut d = document(wrap(edge, boxes(text(), depth - cost)));
            let measured = measure(d.sections());
            assert_eq!(measured.depth, depth, "{edge}");
            assert_eq!(measured.nodes, 2 + nodes + 3 * (depth - cost), "{edge}");
            assert_eq!(d.sections()[0].check_nesting_depth().is_ok(), depth <= 32, "{edge}");
            assert_eq!(dismantle(d.sections_mut()), measured.nodes, "{edge}");
            assert_eq!(measure(d.sections()).nodes, 0, "{edge}");
        }
    }
    let mut d = document(boxes(text(), 32));
    d.sections_mut()[0].paragraphs.extend(ps(boxes(text(), 31)));
    assert_eq!(measure(d.sections()).depth, 32);
    let n = measure(d.sections()).nodes;
    assert_eq!(dismantle(d.sections_mut()), n);
}

// 이것을 실패시키는 것: band 루트 예약 삭제 또는 루트 비용 1,
// 빈 컨테이너 자기 비용 삭제, FieldBoundary::End 에 비용 추가.
#[test]
fn roots_empty_objects_and_error_location() {
    for root in 0..4 {
        for depth in [31, 32, 33] {
            let mut d = document(text());
            d.add_section(Section::with_paragraphs(ps(text()), PageSettings::a4()));
            let s = &mut d.sections_mut()[1];
            let mut paragraphs = ps(text());
            paragraphs.extend(ps(boxes(text(), depth)));
            match root {
                0 => s.paragraphs = paragraphs,
                1 => s.headers.push(HeaderFooter::new(paragraphs, ApplyPageType::Both)),
                2 => s.footers.push(HeaderFooter::new(paragraphs, ApplyPageType::Both)),
                _ => s.master_pages = Some(vec![MasterPage::new(ApplyPageType::Both, paragraphs)]),
            }
            let m = measure(d.sections());
            assert_eq!(m.depth, depth);
            assert_eq!(m.section, 1);
            assert_eq!(m.paragraph, 1);
            assert_eq!(check(d.sections()).is_ok(), depth <= 32);
            if depth == 33 {
                let CoreError::InvalidStructure { context, reason } =
                    check(d.sections()).unwrap_err()
                else {
                    panic!("wrong error")
                };
                assert_eq!(context, "nesting_depth");
                for part in ["maximum 33", "limit 32", "section=1", "paragraph=1", "root="] {
                    assert!(reason.contains(part));
                }
            }
            assert_eq!(dismantle(d.sections_mut()), m.nodes);
        }
    }
    let mut empty = Document::new();
    assert_eq!(measure(empty.sections()).depth, 0);
    assert!(check(empty.sections()).is_ok());
    assert_eq!(dismantle(empty.sections_mut()), 0);
    assert!(empty.validate().is_err());
    let objects = vec![
        ctrl(group(vec![])),
        ctrl(Control::memo(vec![])),
        ctrl(Control::text_box(vec![], u(), u())),
        ctrl(Control::footnote(vec![])),
        ctrl(Control::endnote(vec![])),
        ctrl(Control::FieldBoundary { boundary: FieldBoundary::Start(Box::default()) }),
        Run::table(Table::new(vec![]), CharShapeIndex::new(0)),
        Run::image(Image::new("a", u(), u(), ImageFormat::Png), CharShapeIndex::new(0)),
        ctrl(Control::line(ShapePoint::new(0, 0), ShapePoint::new(1, 1)).unwrap()),
    ];
    for r in objects {
        let mut d = document(r);
        assert_eq!(measure(d.sections()).depth, 1);
        dismantle(d.sections_mut());
    }
    let d = document(ctrl(Control::FieldBoundary {
        boundary: FieldBoundary::End { begin_id: String::new(), field_id: String::new() },
    }));
    assert_eq!(measure(d.sections()).depth, 0);
}

// 이것을 실패시키는 것: validate 선행 검사 삭제/정규화 뒤로 이동,
// Group/Memo/TextBox 공유 해체 간선 삭제. 명시 해체 계수는 위 대조 시험에서 검증.
#[test]
fn consuming_deep_validation_small_stack() {
    small_stack_child(
        "depth::tests::consuming_deep_validation_small_stack",
        "HWPFORGE_DEPTH_CORE_CHILD",
        2 * 1024 * 1024,
        || {
            let reject = |d: Document| {
                let CoreError::InvalidStructure { context, reason } = d.validate().unwrap_err()
                else {
                    panic!("wrong error")
                };
                assert_eq!(context, "nesting_depth");
                assert!(reason.contains("maximum 100000"), "{reason}");
            };
            let mut c = group(vec![]);
            for _ in 1..100_000 {
                c = group(vec![GroupChild::Shape(c)]);
            }
            reject(document(ctrl(c)));
            let mut r = text();
            for _ in 0..100_000 {
                r = ctrl(Control::memo_with_anchor(vec![], vec![r]));
            }
            reject(document(r));
            reject(document(boxes(text(), 100_000)));
        },
    );
}

// 이것을 실패시키는 것: Paragraph 또는 Control Drop 본문 비우기,
// 간선 분리 대신 자식 Vec를 바로 drop하기 (자식 프로세스 stack overflow).
#[test]
fn automatic_deep_drop_small_stack() {
    small_stack_child(
        "depth::tests::automatic_deep_drop_small_stack",
        "HWPFORGE_DROP_CORE_CHILD",
        512 * 1024,
        || {
            let mut c = group(vec![]);
            for _ in 1..100_000 {
                c = group(vec![GroupChild::Shape(c)]);
            }
            drop(document(ctrl(c)));
            let mut c = group(vec![]);
            for _ in 1..100_000 {
                c = group(vec![GroupChild::Shape(c)]);
            }
            drop(c);
            let mut r = text();
            for _ in 0..100_000 {
                r = ctrl(Control::memo_with_anchor(vec![], vec![r]));
            }
            drop(document(r));
            drop(document(boxes(text(), 100_000)));
            // Reuse W1's exhaustive edge inventory, including both caption
            // routes and paragraph-free memo anchors. No validation/clone.
            for &(edge, _, _) in EDGES {
                let mut r = text();
                for _ in 0..100_000 {
                    r = wrap(edge, r);
                }
                drop(document(r));
            }
            let mut r = text();
            for i in 0..100_000 {
                r = wrap(EDGES[i % EDGES.len()].0, r);
            }
            drop(document(r));
        },
    );
}

// 이것을 실패시키는 것: Paragraph Drop 비우기; 루트 종류와 무관하게
// image-caption 사슬이 첫 Paragraph에서 반복 해체되지 못해 abort한다.
#[test]
fn independent_roots_drop_small_stack() {
    small_stack_child(
        "depth::tests::independent_roots_drop_small_stack",
        "HWPFORGE_DROP_ROOT_CHILD",
        512 * 1024,
        || {
            let deep = || {
                let mut r = text();
                for _ in 0..100_000 {
                    r = wrap("image caption", r);
                }
                r
            };
            drop(Paragraph::with_runs(vec![deep()], ParaShapeIndex::new(0)));
            drop(Control::memo_with_anchor(vec![], vec![deep()]));
            drop(deep());
            drop(deep().content);
            drop(Table::new(vec![TableRow::new(vec![TableCell::new(ps(deep()), u())])]));
            drop(TableCell::new(ps(deep()), u()));
            drop(cap(deep()));
            let mut image = Image::new("root", u(), u(), ImageFormat::Png);
            image.caption = Some(cap(deep()));
            drop(image);
            drop(GroupChild::Shape(Control::memo(ps(deep()))));
            drop(FieldStart { annotation: Some(ps(deep())), ..FieldStart::default() });
            drop(HeaderFooter::new(ps(deep()), ApplyPageType::Both));
            drop(MasterPage::new(ApplyPageType::Both, ps(deep())));
            drop(Section::with_paragraphs(ps(deep()), PageSettings::a4()));
            let mut d = document(text());
            d.sections_mut()[0].headers.push(HeaderFooter::new(ps(deep()), ApplyPageType::Both));
            d.sections_mut()[0].footers.push(HeaderFooter::new(ps(deep()), ApplyPageType::Both));
            d.sections_mut()[0].master_pages =
                Some(vec![MasterPage::new(ApplyPageType::Both, ps(deep()))]);
            drop(d);
        },
    );
}

// 이것을 실패시키는 것: Paragraph/Control Drop 본문 비우기 → 패닉 해제 중
// stack overflow, 콜백 panic 삭제 → catch_unwind 및 해제 시점 단언 실패.
#[test]
fn panic_deep_drop_small_stack() {
    small_stack_child(
        "depth::tests::panic_deep_drop_small_stack",
        "HWPFORGE_PANIC_DROP_CORE_CHILD",
        512 * 1024,
        || {
            use std::sync::atomic::{AtomicBool, Ordering};
            use std::sync::Arc;

            // Document drops format_source after its owned sections. Record that
            // the document finished dropping while the callback was unwinding.
            struct DroppedDuringPanic(Arc<AtomicBool>);
            impl Drop for DroppedDuringPanic {
                fn drop(&mut self) {
                    self.0.store(std::thread::panicking(), Ordering::SeqCst);
                }
            }

            for control_chain in [false, true] {
                let mut d = if control_chain {
                    let mut c = group(vec![]);
                    for _ in 1..100_000 {
                        c = group(vec![GroupChild::Shape(c)]);
                    }
                    document(ctrl(c))
                } else {
                    let mut r = text();
                    for _ in 0..100_000 {
                        r = wrap("image caption", r);
                    }
                    document(r)
                };
                let dropped = Arc::new(AtomicBool::new(false));
                d.set_format_source(Arc::new(DroppedDuringPanic(Arc::clone(&dropped))));
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(move || {
                    // The current walker is recursive (W3). Panic at the first
                    // paragraph, before descending, while this scope owns d.
                    d.for_each_paragraph_mut(|paragraph| {
                        // Also unwind an owned run removed by a user callback:
                        // its Control must drop without a Paragraph parent.
                        let _owned_run = control_chain.then(|| paragraph.runs.pop().unwrap());
                        panic!("callback panic");
                    });
                }));
                assert_eq!(result.unwrap_err().downcast_ref::<&str>(), Some(&"callback panic"));
                assert!(dropped.load(Ordering::SeqCst), "document must drop during unwinding");
            }
        },
    );
}

// 이것을 실패시키는 것: 자식 실행의 --exact 필터를 없는 이름으로 변경.
// 이것을 실패시키는 것: 성공 자식 stdout을 `test result: ok. 11 passed`로 변조.
// 정상 exit만 검사하면 0 tests 실행도 통과하므로 실행 수를 함께 확인한다.
fn small_stack_child(name: &str, flag: &str, stack_size: usize, body: fn()) {
    if std::env::var_os(flag).is_some() {
        std::thread::Builder::new().stack_size(stack_size).spawn(body).unwrap().join().unwrap();
    } else {
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", name, "--nocapture"])
            .env(flag, "1")
            .output()
            .unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("test result: ok. 1 passed"),
            "child must execute exactly one test: {}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}
