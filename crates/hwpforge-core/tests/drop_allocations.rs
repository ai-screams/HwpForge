//! Isolated allocator accounting: no production instrumentation or dependency.
use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;

use hwpforge_core::control::{Control, GroupChild};
use hwpforge_core::{Document, ObjectFrame, PageSettings, Paragraph, Run, Section};
use hwpforge_foundation::{CharShapeIndex, HwpUnit, ParaShapeIndex};

#[derive(Clone, Copy, Default, Debug)]
struct Counts {
    live_bytes: isize,
    allocations: usize,
}
thread_local! {
    static COUNTS: Cell<Option<Counts>> = const { Cell::new(None) };
}
struct Counting;
// SAFETY: allocation and deallocation use System unchanged. The thread-local
// Cell only records sizes; it never allocates or dereferences an allocation.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller supplies GlobalAlloc's valid layout.
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            let _ = COUNTS.try_with(|c| {
                if let Some(mut n) = c.get() {
                    n.live_bytes += layout.size() as isize;
                    n.allocations += 1;
                    c.set(Some(n));
                }
            });
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        let _ = COUNTS.try_with(|c| {
            if let Some(mut n) = c.get() {
                n.live_bytes -= layout.size() as isize;
                c.set(Some(n));
            }
        });
        // SAFETY: the pointer/layout pair comes from this System-backed allocator.
        unsafe { System.dealloc(ptr, layout) };
    }
}
#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn group(children: Vec<GroupChild>) -> Control {
    Control::Group {
        children,
        width: HwpUnit::new(100).unwrap(),
        height: HwpUnit::new(100).unwrap(),
        frame: ObjectFrame::new(),
        placement: None,
        inst_id: None,
    }
}

// 이것을 실패시키는 것: text-only runs를 작업 Vec에 예약(할당 0 위반),
// Paragraph runs를 mem::forget(누수).
#[test]
fn text_drop_needs_no_work_allocation() {
    COUNTS.with(|c| c.set(Some(Counts::default())));
    let paragraph = Paragraph::with_runs(
        vec![Run::text("normal text", CharShapeIndex::new(0))],
        ParaShapeIndex::new(0),
    );
    let before = COUNTS.with(|c| c.get().unwrap().allocations);
    drop(paragraph);
    let after = COUNTS.with(|c| c.replace(None).unwrap());
    assert_eq!(after.live_bytes, 0);
    assert_eq!(after.allocations, before);
}

// 이것을 실패시키는 것: 빈 Group도 작업 스택에 예약 (불필요한 할당).
#[test]
fn empty_group_needs_no_work_allocation() {
    COUNTS.with(|c| c.set(Some(Counts::default())));
    drop(group(vec![]));
    let empty = COUNTS.with(|c| c.replace(None).unwrap());
    assert_eq!(empty.live_bytes, 0);
    assert_eq!(empty.allocations, 0);
}

// 이것을 실패시키는 것: Paragraph runs를 mem::forget (정상 문서 누수).
#[test]
fn validated_drop_reclaims_allocations() {
    COUNTS.with(|c| c.set(Some(Counts::default())));
    let mut doc = Document::new();
    doc.add_section(Section::with_paragraphs(
        vec![Paragraph::with_runs(
            vec![Run::text("validated document", CharShapeIndex::new(0))],
            ParaShapeIndex::new(0),
        )],
        PageSettings::a4(),
    ));
    drop(doc.validate().unwrap());
    let normal = COUNTS.with(|c| c.replace(None).unwrap());
    assert_eq!(normal.live_bytes, 0);
}

// 이것을 실패시키는 것: Group children을 mem::forget (누수),
// 각 GroupChild::Shape에 Box::new 추가 (노드별 할당).
#[test]
fn group_drop_reuses_work_and_reclaims_allocations() {
    for depth in [32, 100_000] {
        COUNTS.with(|c| c.set(Some(Counts::default())));
        let mut control = group(vec![]);
        for _ in 0..depth {
            control = group(vec![GroupChild::Shape(control)]);
        }
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![Paragraph::with_runs(
                vec![Run::control(control, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            )],
            PageSettings::a4(),
        ));
        let before = COUNTS.with(|c| c.get().unwrap().allocations);
        drop(doc);
        let after = COUNTS.with(|c| c.replace(None).unwrap());
        assert_eq!(after.live_bytes, 0, "depth={depth}");
        // 이것을 실패시키는 것: 각 GroupChild::Shape에 Box::new 추가.
        assert_eq!(after.allocations - before, 1, "one reusable work Vec, depth={depth}");
    }
}
