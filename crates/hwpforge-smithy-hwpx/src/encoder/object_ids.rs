//! Per-encode object id allocation (W0, design §3.2).
//!
//! The encoder used to draw `id`/`instid` values for objects without an
//! imported id from one process-global counter, so the same document encoded
//! twice in one process — or after another document — came out with
//! different bytes. Now each encode call installs its own allocator
//! ([`IdScope`]): it starts at 1, and it skips every id the document already
//! stores in Core (pictures, tables, equations, groups, text art, footnotes
//! and endnotes), so a drawn id never equals an imported one.
//!
//! Imported ids are written as before, to the attribute their Core field
//! maps to. Two objects sharing one Core id are kept as they are (which one
//! a cross reference means is not ours to decide) and reported as
//! [`super::EncodeWarning::ObjectIdConflict`].

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};

use hwpforge_core::control::{Control, GroupChild};
use hwpforge_core::document::Document;
use hwpforge_core::paragraph::Paragraph;
use hwpforge_core::run::RunContent;
use hwpforge_core::ObjectId;

/// The allocator of one encode call.
struct IdAllocator {
    next: u64,
    reserved: HashSet<u64>,
    /// The Core ids the encoder actually wrote, with the kind of object, in
    /// document order — what the duplicate-id diagnostic counts. A parent
    /// reserves its slot ([`reserve`]) before its children are encoded, so a
    /// slot left `None` is an object written without a Core id.
    written: Vec<Option<(&'static str, u64)>>,
}

thread_local! {
    /// Open encode scopes, innermost last. Encoding never crosses threads.
    static SCOPES: RefCell<Vec<IdAllocator>> = const { RefCell::new(Vec::new()) };
}

/// One encode call's id allocator, open until dropped (also on unwind).
///
/// Not `Send`: the scope lives on the thread whose stack it pushed.
pub(crate) struct IdScope(std::marker::PhantomData<*const ()>);

impl IdScope {
    /// Opens an allocator that skips `reserved`.
    pub(crate) fn enter(reserved: HashSet<u64>) -> Self {
        SCOPES.with(|s| {
            s.borrow_mut().push(IdAllocator { next: 1, reserved, written: Vec::new() });
        });
        Self(std::marker::PhantomData)
    }
}

impl Drop for IdScope {
    fn drop(&mut self) {
        SCOPES.with(|s| {
            s.borrow_mut().pop();
        });
    }
}

/// The next id of the innermost encode scope, skipping reserved ids.
///
/// # Panics
///
/// Outside an encode scope — every encode path opens one, so this is an
/// encoder bug, never input-dependent.
pub(crate) fn next_id() -> u64 {
    SCOPES.with(|s| {
        let mut scopes = s.borrow_mut();
        // Unit tests call the part builders directly, outside any encode
        // call; they get one allocator for the test thread.
        #[cfg(test)]
        if scopes.is_empty() {
            scopes.push(IdAllocator { next: 1, reserved: HashSet::new(), written: Vec::new() });
        }
        #[allow(clippy::expect_used)] // invariant: encode entry points open a scope
        let alloc = scopes.last_mut().expect("object id drawn outside an encode scope");
        loop {
            let id = alloc.next;
            alloc.next += 1;
            if !alloc.reserved.contains(&id) {
                return id;
            }
        }
    })
}

/// The id to write for an object of `kind`: its Core id, recorded as
/// written, else a drawn one. Every place that writes a Core id goes
/// through here (notes through [`record`]), so the duplicate-id diagnostic
/// sees exactly the objects in the output (review W0 r3): an object the
/// encoder leaves out — anything in a memo anchor, a group child equation —
/// is never recorded.
pub(crate) fn core_or_drawn(kind: &'static str, id: Option<ObjectId>) -> String {
    let slot = reserve();
    core_or_drawn_at(slot, kind, id)
}

/// Reserves the diagnostic slot of an object whose children are encoded
/// before its own id is written (tables, pictures, groups), so the record
/// stays in document order (review W2 r1 LOW). `usize::MAX` outside a scope.
pub(crate) fn reserve() -> usize {
    SCOPES.with(|s| match s.borrow_mut().last_mut() {
        Some(alloc) => {
            alloc.written.push(None);
            alloc.written.len() - 1
        }
        None => usize::MAX,
    })
}

/// [`core_or_drawn`] into a slot reserved earlier with [`reserve`].
pub(crate) fn core_or_drawn_at(slot: usize, kind: &'static str, id: Option<ObjectId>) -> String {
    match id {
        Some(id) => {
            SCOPES.with(|s| {
                if let Some(entry) = s.borrow_mut().last_mut().and_then(|a| a.written.get_mut(slot))
                {
                    *entry = Some((kind, id.value()));
                }
            });
            id.to_string()
        }
        None => next_id().to_string(),
    }
}

/// The current end of the diagnostic record — where the records of a part
/// that comes earlier in the output than in the encoding go
/// ([`move_since`]). `usize::MAX` outside a scope.
pub(crate) fn mark() -> usize {
    SCOPES.with(|s| s.borrow().last().map_or(usize::MAX, |a| a.written.len()))
}

/// Moves the records added since `from` to position `to` (`to <= from`, both
/// [`mark`]s of this scope), keeping their order. A table's caption precedes its cells in the output
/// but is encoded after them (review S22 r1 LOW): the ids are still drawn in
/// encoding order, only the diagnostic follows the output.
pub(crate) fn move_since(from: usize, to: usize) {
    SCOPES.with(|s| {
        if let Some(alloc) = s.borrow_mut().last_mut() {
            // A mark taken before the scope existed (`usize::MAX` — part
            // builders called directly in unit tests, where the first drawn
            // id opens the scope) moves nothing.
            // 이것을 실패시키는 것: 이 검사 삭제(표 단위 시험이 범위 밖 색인으로
            // panic).
            if to <= from {
                let moved = alloc.written.len() - from;
                alloc.written[to..].rotate_right(moved);
            }
        }
    });
}

/// Records a Core id the encoder wrote (footnotes and endnotes, whose
/// `instId` has no drawn fallback).
pub(crate) fn record(kind: &'static str, id: u64) {
    SCOPES.with(|s| {
        if let Some(alloc) = s.borrow_mut().last_mut() {
            alloc.written.push(Some((kind, id)));
        }
    });
}

/// The Core ids written so far in the innermost encode scope.
pub(crate) fn written() -> Vec<(&'static str, u64)> {
    SCOPES.with(|s| {
        s.borrow().last().map(|a| a.written.iter().flatten().copied().collect()).unwrap_or_default()
    })
}

/// Every Core object id in the document, in document order, with the kind
/// of object that holds it — the ids the allocator must skip.
///
/// Reaches every place an object is written: body, headers, footers and
/// master pages; cells and table captions; text boxes, shapes, notes and
/// memos; group children (nested groups included); and run pictures'
/// captions, which [`Paragraph::for_each_paragraph`] does not enter. It
/// also takes objects in paragraphs nested in memo anchor runs (a text box
/// in an anchor, say), which the encoder does not write
/// (`build_memo_anchor_xml` keeps the anchor's text): skipping an id too
/// many is harmless.
pub(crate) fn core_object_ids<S>(document: &Document<S>) -> Vec<(&'static str, u64)> {
    let mut out = Vec::new();
    for_each_top_paragraph(document, &mut |p| {
        walk(p, &mut |q| {
            for run in &q.runs {
                match &run.content {
                    RunContent::Table(t) => push(&mut out, "table", t.inst_id),
                    RunContent::Image(img) => push(&mut out, "picture", img.inst_id),
                    RunContent::Control(c) => visit_control(c, &mut out),
                    _ => {}
                }
            }
        });
    });
    out
}

fn for_each_top_paragraph<S>(document: &Document<S>, f: &mut dyn FnMut(&Paragraph)) {
    for section in document.sections() {
        let tops = section
            .paragraphs
            .iter()
            .chain(section.headers.iter().chain(section.footers.iter()).flat_map(|h| &h.paragraphs))
            .chain(section.master_pages.iter().flatten().flat_map(|m| &m.paragraphs));
        for p in tops {
            f(p);
        }
    }
}

/// Every paragraph in `p`, run pictures' captions entered.
fn walk(p: &Paragraph, f: &mut dyn FnMut(&Paragraph)) {
    p.for_each_paragraph(|q| {
        f(q);
        for run in &q.runs {
            if let RunContent::Image(img) = &run.content {
                for cp in img.caption.iter().flat_map(|c| &c.paragraphs) {
                    walk(cp, f);
                }
            }
        }
    });
}

/// A control's own id and, for a group, its children's. The paragraphs
/// inside are reached by the paragraph walk.
fn visit_control(c: &Control, out: &mut Vec<(&'static str, u64)>) {
    match c {
        Control::Equation { inst_id, .. } => push(out, "equation", *inst_id),
        Control::TextArt { inst_id, .. } => push(out, "textart", *inst_id),
        Control::Footnote { inst_id, .. } => push(out, "footnote", *inst_id),
        Control::Endnote { inst_id, .. } => push(out, "endnote", *inst_id),
        Control::Group { inst_id, children, .. } => {
            push(out, "group", *inst_id);
            for child in children {
                match child {
                    GroupChild::Picture(img) => push(out, "picture", img.inst_id),
                    GroupChild::Shape(shape) => visit_control(shape, out),
                    _ => {}
                }
            }
        }
        _ => {}
    }
}

fn push(out: &mut Vec<(&'static str, u64)>, kind: &'static str, id: Option<ObjectId>) {
    if let Some(id) = id {
        out.push((kind, id.value()));
    }
}

/// Ids held by more than one object, with the holders' kinds in document
/// order. An id held only by footnotes and endnotes is left out: Hancom
/// itself saves several notes with one instance id (corpus: three
/// footnotes sharing 6029409 in one document, a footnote and an endnote
/// sharing 1 in `full_report.hwp`), so such a share says nothing about a
/// cross reference. An id a note shares with any other object is kept.
/// Every id is still reserved.
/// 이것을 실패시키는 것: 주석만인 경우 제외 삭제(각주·미주 공유가 경고) · 주석을
/// 집계 전에 빼기(표↔각주·그림↔미주 혼합이 숨는다)
/// (`shared_core_id_is_kept_and_reported`).
pub(crate) fn conflicts(ids: &[(&'static str, u64)]) -> BTreeMap<u64, Vec<&'static str>> {
    let mut by_id: BTreeMap<u64, Vec<&'static str>> = BTreeMap::new();
    for &(kind, id) in ids {
        by_id.entry(id).or_default().push(kind);
    }
    by_id.retain(|_, kinds| {
        kinds.len() > 1 && !kinds.iter().all(|k| *k == "footnote" || *k == "endnote")
    });
    by_id
}

#[cfg(test)]
mod tests {
    use std::io::Read;

    use hwpforge_core::caption::{Caption, CaptionSide};
    use hwpforge_core::control::{Control, GroupChild};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::{Image, ImageFormat, ImageStore};
    use hwpforge_core::paragraph::Paragraph;
    use hwpforge_core::run::Run;
    use hwpforge_core::section::{HeaderFooter, MasterPage, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{ObjectId, PageSettings};
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};

    use super::{next_id, IdScope, SCOPES};
    use crate::style_store::{HwpxCharShape, HwpxParaShape, HwpxStyleStore};
    use crate::HwpxEncoder;
    use std::collections::HashSet;

    fn unit() -> HwpUnit {
        HwpUnit::new(1000).unwrap()
    }

    fn para(runs: Vec<Run>) -> Paragraph {
        Paragraph::with_runs(runs, ParaShapeIndex::new(0))
    }

    fn text() -> Paragraph {
        para(vec![Run::text("x", CharShapeIndex::new(0))])
    }

    fn table(id: Option<u64>) -> Table {
        let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(vec![text()], unit())])]);
        t.inst_id = id.map(ObjectId::new);
        t
    }

    fn holder(t: Table) -> Paragraph {
        para(vec![Run::table(t, CharShapeIndex::new(0))])
    }

    fn picture(id: Option<u64>) -> Image {
        let mut img = Image::new("BinData/a.png", unit(), unit(), ImageFormat::Png);
        img.inst_id = id.map(ObjectId::new);
        img
    }

    /// The package bytes of a one-section document.
    fn encode_bytes(section: Section) -> Vec<u8> {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());
        let mut doc = Document::new();
        doc.add_section(section);
        let mut images = ImageStore::new();
        images.insert("a.png", vec![0x89, b'P', b'N', b'G']);
        HwpxEncoder::encode(&doc.validate().unwrap(), &store, &images).unwrap()
    }

    /// Every XML part of the package, concatenated (for the id checks).
    fn encode(section: Section) -> String {
        let bytes = encode_bytes(section);
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut all = String::new();
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).unwrap();
            if entry.name().ends_with(".xml") {
                entry.read_to_string(&mut all).unwrap();
            }
        }
        all
    }

    /// Object elements whose `id` or `instid` is `value`.
    fn objects_with_id(xml: &str, value: &str) -> usize {
        let tags = ["<hp:tbl ", "<hp:pic ", "<hp:container ", "<hp:equation ", "<hp:rect "];
        let mut n = 0;
        for tag in tags {
            for (at, _) in xml.match_indices(tag) {
                let head = &xml[at..at + xml[at..].find('>').unwrap()];
                n += [" id=", " instid="]
                    .iter()
                    .filter(|a| head.contains(&format!("{a}\"{value}\"")))
                    .count();
            }
        }
        n
    }

    /// W0: an object with Core id 1 in each place an object is written, next
    /// to an id-less table in the body — the allocator never draws 1, so 1
    /// is written once (by its holder).
    /// 이것을 실패시키는 것: 예약 생략 · 예약 순회가 그 자리에 들어가지 않기
    /// (칸·머리말·바탕쪽·각주·글상자·그림 캡션·묶음 자식 각각).
    #[test]
    fn drawn_ids_skip_core_ids() {
        let cs = CharShapeIndex::new(0);
        let with_body = |extra: Vec<Paragraph>| {
            let mut paragraphs = vec![holder(table(None))];
            paragraphs.extend(extra);
            Section::with_paragraphs(paragraphs, PageSettings::a4())
        };
        let mut cases: Vec<(&str, Section)> = Vec::new();
        cases.push(("body", with_body(vec![holder(table(Some(1)))])));
        let cell = Table::new(vec![TableRow::new(vec![TableCell::new(
            vec![holder(table(Some(1)))],
            unit(),
        )])]);
        cases.push(("cell", with_body(vec![holder(cell)])));
        let mut s = with_body(vec![]);
        s.headers.push(HeaderFooter::all_pages(vec![holder(table(Some(1)))]));
        cases.push(("header", s));
        let mut s = with_body(vec![]);
        s.master_pages =
            Some(vec![MasterPage::new(ApplyPageType::Both, vec![holder(table(Some(1)))])]);
        cases.push(("master page", s));
        let note = Control::footnote(vec![holder(table(Some(1)))]);
        cases.push(("footnote", with_body(vec![para(vec![Run::control(note, cs)])])));
        let text_box = Control::text_box(vec![holder(table(Some(1)))], unit(), unit());
        cases.push(("text box", with_body(vec![para(vec![Run::control(text_box, cs)])])));
        let mut pic = picture(None);
        pic.caption = Some(Caption::new(vec![holder(table(Some(1)))], CaptionSide::Bottom));
        cases.push(("picture caption", with_body(vec![para(vec![Run::image(pic, cs)])])));
        let group = Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                GroupChild::Picture(picture(Some(1))),
                GroupChild::Picture(picture(None)),
            ],
            width: unit(),
            height: unit(),
            placement: None,
            inst_id: None,
        };
        cases.push(("group child", with_body(vec![para(vec![Run::control(group, cs)])])));
        for (name, section) in cases {
            let xml = encode(section);
            assert_eq!(objects_with_id(&xml, "1"), 1, "{name}");
        }
    }

    /// W0: the same document encodes to the same bytes again, and after
    /// another document, in one process (nextest runs each test in its own
    /// process, so the repeats are inside this test).
    /// 이것을 실패시키는 것: 프로세스 전역 카운터로 되돌리기.
    #[test]
    fn same_document_same_bytes_in_one_process() {
        let a = || with_drawn_ids(2);
        let b = || with_drawn_ids(5);
        // Whole package bytes (review W0 r1 L1), not only the XML parts.
        let first = encode_bytes(a());
        assert_eq!(encode_bytes(a()), first);
        let _ = encode_bytes(b());
        assert_eq!(encode_bytes(a()), first);
        let other_thread = std::thread::spawn(move || encode_bytes(with_drawn_ids(2)));
        assert_eq!(other_thread.join().unwrap(), first);
    }

    /// Review W0 r1 L2: closing an inner scope resumes the outer one where
    /// it stopped, and a scope closes on unwind too.
    /// 이것을 실패시키는 것: `Drop` 의 `pop` 삭제(바깥이 3 대신 안쪽 2 를 낸다 ·
    /// panic 뒤 깊이가 하나 늘어 있다).
    #[test]
    fn scopes_nest_and_close_on_unwind() {
        let depth = || SCOPES.with(|s| s.borrow().len());
        let base = depth();
        let outer = IdScope::enter(HashSet::new());
        assert_eq!((next_id(), next_id()), (1, 2));
        {
            let _inner = IdScope::enter(HashSet::from([1]));
            assert_eq!(next_id(), 2);
        }
        assert_eq!(next_id(), 3);
        drop(outer);
        assert_eq!(depth(), base);
        let unwound = std::panic::catch_unwind(|| {
            let _scope = IdScope::enter(HashSet::new());
            panic!("inside an encode");
        });
        assert!(unwound.is_err());
        assert_eq!(depth(), base);
    }

    fn with_drawn_ids(n: usize) -> Section {
        Section::with_paragraphs((0..n).map(|_| holder(table(None))).collect(), PageSettings::a4())
    }

    /// S22 r2: with no Core ids, a captioned table draws its own id after
    /// its cells and before its caption's equation, as it always has — the
    /// diagnostic's reordering does not change the ids written.
    /// 이것을 실패시키는 것: 표 id 를 캡션 인코딩 뒤에 뽑기(표 2, 수식 1).
    #[test]
    fn drawn_ids_keep_the_table_before_its_caption() {
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());
        let mut t = table(None);
        t.caption = Some(Caption::new(
            vec![para(vec![Run::control(
                Control::Equation {
                    script: "a".into(),
                    width: unit(),
                    height: unit(),
                    base_line: 85,
                    text_color: hwpforge_foundation::Color::BLACK,
                    font: "HancomEQN".into(),
                    inst_id: None,
                },
                CharShapeIndex::new(0),
            )])],
            CaptionSide::Bottom,
        ));
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(vec![holder(t)], PageSettings::a4()));
        let bytes =
            HwpxEncoder::encode(&doc.validate().unwrap(), &store, &ImageStore::new()).unwrap();
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
        let id_of = |tag: &str| {
            let at = xml.find(tag).unwrap_or_else(|| panic!("{tag}"));
            let head = &xml[at..at + xml[at..].find('>').unwrap()];
            let v = &head[head.find(" id=\"").unwrap() + 5..];
            v[..v.find('"').unwrap()].parse::<u64>().unwrap()
        };
        assert_eq!((id_of("<hp:tbl "), id_of("<hp:equation ")), (1, 2));
    }

    /// W0: two tables with Core id 7 keep it (both written as 7) and give
    /// one warning naming both; a third object with another id adds none,
    /// and neither does a footnote and an endnote sharing 9 — but a note
    /// sharing with a table (10) or a picture (11) does. A table in a memo
    /// anchor (not written) sharing 12 with a body table is not reported.
    /// 이것을 실패시키는 것: 경고 삭제 · 중복 판정을 `> 0` 으로(모든 id 경고).
    #[test]
    fn shared_core_id_is_kept_and_reported() {
        let mut captioned = picture(None);
        captioned.caption = Some(Caption::new(vec![holder(table(Some(12)))], CaptionSide::Bottom));
        let equation = |id: u64| Control::Equation {
            script: "a".into(),
            width: unit(),
            height: unit(),
            base_line: 85,
            text_color: hwpforge_foundation::Color::BLACK,
            font: "HancomEQN".into(),
            inst_id: Some(ObjectId::new(id)),
        };
        let group_with_equation = |id: u64| Control::Group {
            frame: hwpforge_core::ObjectFrame::new(),
            children: vec![
                GroupChild::Picture(picture(None)),
                GroupChild::Shape(Control::Equation {
                    script: "a".into(),
                    width: unit(),
                    height: unit(),
                    base_line: 85,
                    text_color: hwpforge_foundation::Color::BLACK,
                    font: "HancomEQN".into(),
                    inst_id: Some(ObjectId::new(id)),
                }),
            ],
            width: unit(),
            height: unit(),
            placement: None,
            inst_id: None,
        };
        let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
        store.push_char_shape(HwpxCharShape::default());
        store.push_para_shape(HwpxParaShape::default());
        let mut doc = Document::new();
        doc.add_section(Section::with_paragraphs(
            vec![
                holder(table(Some(7))),
                holder(table(Some(7))),
                holder(table(Some(8))),
                para(vec![
                    Run::control(
                        Control::footnote_with_id(9, vec![text()]),
                        CharShapeIndex::new(0),
                    ),
                    Run::control(Control::endnote_with_id(9, vec![text()]), CharShapeIndex::new(0)),
                    Run::control(
                        Control::footnote_with_id(10, vec![text()]),
                        CharShapeIndex::new(0),
                    ),
                    Run::control(
                        Control::endnote_with_id(11, vec![text()]),
                        CharShapeIndex::new(0),
                    ),
                ]),
                holder(table(Some(10))),
                para(vec![Run::image(picture(Some(11)), CharShapeIndex::new(0))]),
                // Review W0 r2·r3 LOW: a memo anchor is written as text only,
                // so the tables in it — one in a text box, one in a picture
                // caption in that text box — share 12 with nothing written.
                holder(table(Some(12))),
                para(vec![Run::control(
                    Control::memo_with_anchor(
                        vec![text()],
                        vec![Run::control(
                            Control::text_box(
                                vec![
                                    holder(table(Some(12))),
                                    para(vec![Run::image(captioned, CharShapeIndex::new(0))]),
                                ],
                                unit(),
                                unit(),
                            ),
                            CharShapeIndex::new(0),
                        )],
                    ),
                    CharShapeIndex::new(0),
                )]),
                // Review W2 r1 LOW: a parent sharing an id with its child is
                // reported in document order (parent first).
                para(vec![Run::control(
                    Control::Group {
                        frame: hwpforge_core::ObjectFrame::new(),
                        children: vec![
                            GroupChild::Picture(picture(Some(21))),
                            GroupChild::Picture(picture(None)),
                        ],
                        width: unit(),
                        height: unit(),
                        placement: None,
                        inst_id: Some(ObjectId::new(21)),
                    },
                    CharShapeIndex::new(0),
                )]),
                holder({
                    let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![para(vec![Run::control(equation(22), CharShapeIndex::new(0))])],
                        unit(),
                    )])]);
                    t.inst_id = Some(ObjectId::new(22));
                    t
                }),
                para(vec![Run::image(
                    {
                        let mut p = picture(Some(23));
                        p.caption =
                            Some(Caption::new(vec![holder(table(Some(23)))], CaptionSide::Bottom));
                        p
                    },
                    CharShapeIndex::new(0),
                )]),
                // Review S22 r1 LOW: a table's caption is written before its
                // cells — a caption equation and a cell footnote sharing 24
                // are reported in that order.
                holder({
                    let mut t = Table::new(vec![TableRow::new(vec![TableCell::new(
                        vec![para(vec![Run::control(
                            Control::footnote_with_id(24, vec![text()]),
                            CharShapeIndex::new(0),
                        )])],
                        unit(),
                    )])]);
                    t.caption = Some(Caption::new(
                        vec![para(vec![Run::control(equation(24), CharShapeIndex::new(0))])],
                        CaptionSide::Bottom,
                    ));
                    t
                }),
                // Review W0 r3 LOW: a group child equation is left out (lost
                // content), so it shares 13 with no written object.
                para(vec![
                    Run::control(
                        Control::footnote_with_id(13, vec![text()]),
                        CharShapeIndex::new(0),
                    ),
                    Run::control(group_with_equation(13), CharShapeIndex::new(0)),
                ]),
            ],
            PageSettings::a4(),
        ));
        let outcome = HwpxEncoder::encode_with_diagnostics(
            &doc.validate().unwrap(),
            &store,
            &ImageStore::new(),
            crate::EncodeOptions::default(),
        )
        .unwrap();
        let outcome_warnings = outcome.warnings.clone();
        let conflicts: Vec<_> = outcome
            .warnings
            .iter()
            .filter(|w| w.code() == "OBJECT_ID_CONFLICT")
            .map(crate::EncodeWarning::message)
            .collect();
        assert_eq!(
            conflicts,
            [
                "2 objects share id 7: table, table",
                "2 objects share id 10: footnote, table",
                "2 objects share id 11: endnote, picture",
                "2 objects share id 21: group, picture",
                "2 objects share id 22: table, equation",
                "2 objects share id 23: picture, table",
                "2 objects share id 24: equation, footnote",
            ]
        );
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(outcome.bytes)).unwrap();
        let mut xml = String::new();
        zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
        assert_eq!(objects_with_id(&xml, "7"), 2);
        assert_eq!(objects_with_id(&xml, "12"), 1, "the anchor's tables are not written");
        // The order the warning follows is the output's: the caption equation
        // comes before the cell's footnote.
        let equation_24 = xml.find(r#"<hp:equation id="24""#).expect("caption equation");
        let footnote_24 = xml.find(r#"instId="24""#).expect("cell footnote");
        assert!(equation_24 < footnote_24);
        assert!(
            outcome_warnings.iter().any(|w| w.code() == "CONTENT_LOST"),
            "the group child equation is lost: {outcome_warnings:?}"
        );
    }

    /// W0: every package entry carries the fixed ZIP epoch time.
    /// 이것을 실패시키는 것: 시각 고정 삭제 — `--features zip/time` 으로 돌릴 때
    /// (그 기능이 꺼진 빌드에서는 zip 기본값이 같은 epoch 라 통과한다).
    #[test]
    fn package_entry_times_are_fixed() {
        let bytes = encode_bytes(with_drawn_ids(1));
        let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        for i in 0..zip.len() {
            let entry = zip.by_index(i).unwrap();
            assert_eq!(entry.last_modified(), Some(zip::DateTime::default()), "{}", entry.name());
        }
    }

    /// W0: a memo without a time gets the epoch, or the time the caller
    /// passes — never the clock, so two encodes match.
    /// 이것을 실패시키는 것: 인코더가 현재 시각을 읽기 · 주입 시각 무시.
    #[test]
    fn memo_time_comes_from_the_caller_not_the_clock() {
        let cs = CharShapeIndex::new(0);
        let memo = Control::memo_with_anchor(vec![text()], vec![Run::text("a", cs)]);
        let section = || {
            Section::with_paragraphs(
                vec![para(vec![Run::control(memo.clone(), cs)])],
                PageSettings::a4(),
            )
        };
        let encode_with = |options: crate::EncodeOptions| {
            let mut store = HwpxStyleStore::with_default_fonts("함초롬바탕");
            store.push_char_shape(HwpxCharShape::default());
            store.push_para_shape(HwpxParaShape::default());
            let mut doc = Document::new();
            doc.add_section(section());
            let bytes = HwpxEncoder::encode_with_options(
                &doc.validate().unwrap(),
                &store,
                &ImageStore::new(),
                options,
            )
            .unwrap();
            let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
            let mut xml = String::new();
            zip.by_name("Contents/section0.xml").unwrap().read_to_string(&mut xml).unwrap();
            xml
        };
        let time = |xml: &str| {
            let tag = r#"<hp:stringParam name="CreateDateTime">"#;
            let at = xml.find(tag).expect("CreateDateTime") + tag.len();
            xml[at..at + xml[at..].find('<').unwrap()].to_string()
        };
        assert_eq!(time(&encode_with(crate::EncodeOptions::default())), "1970-01-01T00:00:00Z");
        let given = crate::EncodeOptions::default().with_memo_create_time(1_780_000_606);
        assert_eq!(time(&encode_with(given)), "2026-05-28T20:36:46Z");
    }
}
