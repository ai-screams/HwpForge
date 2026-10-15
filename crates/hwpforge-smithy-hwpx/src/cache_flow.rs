//! Which paragraph line caches a text change makes stale (W2, design 3.5),
//! shared by the two preserve-first text editors: `fill` and `patch`.
//!
//! A paragraph's `<hp:linesegarray>` places its lines by character position
//! and stacks them by vertical position. A changed paragraph's own cache is
//! stale; so is every later paragraph of the same flow (their vertical
//! positions follow it); so is the paragraph hosting that flow (a cell or
//! text box that grows moves its host's lines) and, the same way, its own
//! flow tail and host chain, up to the section body.

use std::ops::Range;

use crate::error::{HwpxError, HwpxResult};

/// The caches a set of changed paragraphs makes stale: for each, from that
/// paragraph to the end of its list, then from the list's host paragraph to
/// the end of the host's list, up to the section body. Each list keeps only
/// its earliest invalidation point, so every paragraph is examined once.
///
/// `caches[p]` are paragraph `p`'s own cache spans, `para_list[p]` the list
/// it sits in, `list_host[l]` the paragraph hosting list `l` (`None` for the
/// section body). `tick` counts the work (chain steps and paragraphs
/// examined) for the linearity tests.
///
/// 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · host 사슬 생략 · 목록 처음부터
/// (fill: `cell_fill_drops_the_flow_tail_and_the_host_chain`,
/// `each_container_drops_its_flow_tail_and_host_chain`; patch:
/// `patch_drops_the_changed_flow_tail_and_host_chain`).
pub(crate) fn stale_caches(
    caches: &[Vec<Range<usize>>],
    para_list: &[usize],
    list_host: &[Option<usize>],
    paragraphs: impl IntoIterator<Item = usize>,
    tick: &mut impl FnMut(),
) -> Vec<Range<usize>> {
    let mut from = vec![usize::MAX; list_host.len()];
    for paragraph in paragraphs {
        let mut at = Some(paragraph);
        while let Some(q) = at {
            tick();
            self::tick();
            let list = para_list[q];
            from[list] = from[list].min(q);
            at = list_host[list];
        }
    }
    (0..caches.len())
        .filter(|&j| {
            tick();
            self::tick();
            j >= from[para_list[j]]
        })
        .flat_map(|j| caches[j].iter().cloned())
        .collect()
}

/// Every `<hp:p>` of a section XML in start order, whatever container holds
/// it: its byte span, its own cache spans, the list it sits in (the element
/// whose direct children the paragraphs are) and the paragraph hosting that
/// list. Element names are compared without their prefix, as the decoder
/// does.
#[derive(Debug, Default)]
pub(crate) struct ParagraphFlow {
    spans: Vec<Range<usize>>,
    caches: Vec<Vec<Range<usize>>>,
    para_list: Vec<usize>,
    list_host: Vec<Option<usize>>,
    /// The paragraph each paragraph sits inside (`None` in the section
    /// body) — unlike `list_host`, also across a header or footer.
    parent: Vec<Option<usize>>,
}

impl ParagraphFlow {
    /// Scans `xml` (a section part).
    ///
    /// # Errors
    ///
    /// [`HwpxError::XmlParse`] when the XML cannot be read.
    pub(crate) fn scan(xml: &str) -> HwpxResult<Self> {
        use crate::patch::{buffer_position, event_start};
        use quick_xml::events::Event;
        use quick_xml::Reader;

        let mut flow = Self::default();
        let mut reader = Reader::from_str(xml);
        reader.config_mut().trim_text(false);
        let mut buf = Vec::new();
        let mut stack: Vec<Open> = Vec::new();
        loop {
            let event = reader.read_event_into(&mut buf).map_err(|e| HwpxError::XmlParse {
                file: "section.xml".into(),
                detail: e.to_string(),
            })?;
            let end = buffer_position(&reader)?;
            let (start, is_empty) = match &event {
                Event::Start(start) => (start, false),
                Event::Empty(start) => (start, true),
                Event::End(_) => {
                    flow.close(stack.pop(), end);
                    buf.clear();
                    continue;
                }
                Event::Eof => break,
                _ => {
                    buf.clear();
                    continue;
                }
            };
            let begin = event_start(end, start.len(), is_empty)?;
            let local = start.local_name();
            let mut open = Open {
                paragraph: None,
                list: None,
                cache: None,
                page_level: matches!(local.as_ref(), "header" | "footer"),
            };
            match local.as_ref() {
                "p" => {
                    let parent = stack.iter().rev().find_map(|o| o.paragraph);
                    // A header or footer is laid out apart from the body: its
                    // text does not move the lines of the paragraph holding
                    // its control, so its flow has no host.
                    // 이것을 실패시키는 것: 머리말·꼬리말도 host 사슬로 잇기
                    // (`patch_drops_the_changed_flow_tail_and_host_chain` 의
                    // 머리말 편집이 본문 캐시를 모두 지운다).
                    let host =
                        stack.iter().rev().take_while(|o| !o.page_level).find_map(|o| o.paragraph);
                    let list = match stack.last_mut() {
                        Some(parent) => match parent.list {
                            Some(list) => list,
                            None => {
                                flow.list_host.push(host);
                                parent.list = Some(flow.list_host.len() - 1);
                                flow.list_host.len() - 1
                            }
                        },
                        None => {
                            flow.list_host.push(None);
                            flow.list_host.len() - 1
                        }
                    };
                    flow.spans.push(begin..end);
                    flow.caches.push(Vec::new());
                    flow.para_list.push(list);
                    flow.parent.push(parent);
                    open.paragraph = Some(flow.spans.len() - 1);
                }
                "linesegarray" => {
                    open.cache = stack.last().and_then(|o| o.paragraph).map(|p| (p, begin));
                }
                _ => {}
            }
            if is_empty {
                flow.close(Some(open), end);
            } else {
                stack.push(open);
            }
            buf.clear();
        }
        Ok(flow)
    }

    /// Ends the element `open`, which closed at byte `end`.
    fn close(&mut self, open: Option<Open>, end: usize) {
        let Some(open) = open else {
            return;
        };
        if let Some(paragraph) = open.paragraph {
            self.spans[paragraph].end = end;
        }
        if let Some((paragraph, start)) = open.cache {
            self.caches[paragraph].push(start..end);
        }
    }

    /// The innermost paragraph whose span holds byte `offset`.
    ///
    /// The last paragraph to start at or before `offset` is either it or
    /// nested in it (every paragraph still open at `offset` started before
    /// that one and so encloses it), so the walk only climbs parents.
    pub(crate) fn paragraph_at(&self, offset: usize) -> Option<usize> {
        let last = self.spans.partition_point(|span| span.start <= offset).checked_sub(1)?;
        let mut at = Some(last);
        while let Some(p) = at {
            if self.spans[p].contains(&offset) {
                return Some(p);
            }
            at = self.parent[p];
        }
        None
    }

    /// The caches the changed `paragraphs` make stale ([`stale_caches`]).
    pub(crate) fn stale_caches(
        &self,
        paragraphs: impl IntoIterator<Item = usize>,
    ) -> Vec<Range<usize>> {
        stale_caches(&self.caches, &self.para_list, &self.list_host, paragraphs, &mut || {})
    }
}

/// One open element of [`ParagraphFlow::scan`].
struct Open {
    /// This element's index when it is a paragraph.
    paragraph: Option<usize>,
    /// The list its direct paragraph children form, made on the first.
    list: Option<usize>,
    /// The paragraph and start of a `linesegarray` directly under it.
    cache: Option<(usize, usize)>,
    /// A header or footer: the flows inside it have no host.
    page_level: bool,
}

/// Counts one unit of work, for the tests that pin a path linear in its
/// input (branch review B-H2, B-M1); nothing outside tests.
#[inline]
pub(crate) fn tick() {
    tick_n(1);
}

/// Counts `n` units of work (see [`tick`]).
#[inline]
#[cfg_attr(not(test), allow(unused_variables))]
pub(crate) fn tick_n(n: usize) {
    #[cfg(test)]
    WORK.with(|w| w.set(w.get() + n));
}

#[cfg(test)]
thread_local! {
    /// Work units counted by [`tick`] on this thread.
    pub(crate) static WORK: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Writes `xml` with each `(range, replacement)` applied, in one pass.
///
/// # Errors
///
/// [`HwpxError::InvalidStructure`] when two ranges overlap or one falls
/// outside `xml` — the edits would not mean what they say.
pub(crate) fn splice(
    xml: &str,
    mut replacements: Vec<(Range<usize>, String)>,
) -> HwpxResult<String> {
    replacements.sort_unstable_by_key(|(range, _)| (range.start, range.end));
    // One unit per KiB written, so writing the part once per range shows.
    tick_n(xml.len() / 1024);
    let mut out = String::with_capacity(xml.len());
    let mut cursor = 0;
    for (range, text) in replacements {
        tick();
        if range.start < cursor
            || range.end < range.start
            || !xml.is_char_boundary(range.start)
            || !xml.is_char_boundary(range.end)
        {
            return Err(HwpxError::InvalidStructure {
                detail: format!("section edit spans overlap or leave the part at {range:?}"),
            });
        }
        out.push_str(&xml[cursor..range.start]);
        out.push_str(&text);
        cursor = range.end;
    }
    out.push_str(&xml[cursor..]);
    Ok(out)
}

#[cfg(test)]
mod tests {
    //! Cache marker: [`add_caches`] puts a cache with `vertpos` = 9000 + (the
    //! order the paragraph closes in) before every `</hp:p>`; inner paragraphs
    //! close first, so each number names one paragraph.

    use crate::patch::RawPackage;
    use crate::{HwpxDecoder, HwpxPatcher};
    use hwpforge_core::document::Document;
    use hwpforge_core::image::ImageStore;
    use hwpforge_core::run::{Run, RunContent};
    use hwpforge_core::section::{HeaderFooter, Section};
    use hwpforge_core::table::{Table, TableCell, TableRow};
    use hwpforge_core::{PageSettings, Paragraph};
    use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};

    fn para(text: &str) -> Paragraph {
        Paragraph::with_runs(vec![Run::text(text, CharShapeIndex::new(0))], ParaShapeIndex::new(0))
    }

    fn encode(section: Section) -> Vec<u8> {
        let mut doc = Document::new();
        doc.add_section(section);
        let styles = crate::style_store_for_preset("default").expect("preset");
        crate::HwpxEncoder::encode(&doc.validate().expect("validate"), &styles, &ImageStore::new())
            .expect("encode")
    }

    fn section_xml(bytes: &[u8]) -> String {
        RawPackage::read(bytes)
            .expect("read")
            .read_text_entry("Contents/section0.xml")
            .expect("xml")
    }

    fn with_section_xml(bytes: &[u8], xml: String) -> Vec<u8> {
        let mut pkg = RawPackage::read(bytes).expect("read");
        pkg.replace_text_entry("Contents/section0.xml", xml);
        pkg.write().expect("write")
    }

    fn add_caches(xml: &str) -> String {
        let closes = xml.matches("</hp:p>").count();
        let mut out = String::with_capacity(xml.len());
        for (k, piece) in xml.split("</hp:p>").enumerate() {
            if k > 0 {
                out.push_str("</hp:p>");
            }
            out.push_str(piece);
            if k < closes {
                out.push_str(&format!(
                    "<hp:linesegarray><hp:lineseg textpos=\"0\" vertpos=\"{}\" vertsize=\"1000\" \
                     textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
                     horzsize=\"42520\" flags=\"393216\"/></hp:linesegarray>",
                    9000 + k
                ));
            }
        }
        out
    }

    fn caches(xml: &str) -> Vec<usize> {
        (0..64).filter(|k| xml.contains(&format!("vertpos=\"{}\"", 9000 + k))).collect()
    }

    /// Patches section 0 of `bytes` after `edit` changes its decoded section.
    fn patch(bytes: &[u8], edit: impl FnOnce(&mut Section)) -> Vec<u8> {
        let decoded = HwpxDecoder::decode(bytes).expect("decode");
        let base = decoded.document.sections()[0].clone();
        let preservation =
            HwpxPatcher::export_section_preservation(bytes, 0, &base).expect("preservation");
        let mut replacement = base;
        edit(&mut replacement);
        HwpxPatcher::patch_section_preserving(bytes, 0, &replacement, None, Some(&preservation))
            .expect("patch")
    }

    fn set_text(paragraph: &mut Paragraph, text: &str) {
        paragraph.runs[0].content = RunContent::Text(text.into());
    }

    /// The review's counterexample: a body paragraph "ABCDEFGHIJKLMNOPQRST"
    /// with a line at 10, patched to "A", kept `textpos="10"` and no warning;
    /// the re-decode then dropped the cache as out of range.
    /// 이것을 실패시키는 것: patch 의 캐시 제거 삭제.
    #[test]
    fn a_shortened_patch_drops_the_stale_line_start() {
        let bytes = encode(Section::with_paragraphs(
            vec![para("앞"), para("ABCDEFGHIJKLMNOPQRST"), para("뒤")],
            PageSettings::a4(),
        ));
        // Close order: 앞 0, the long paragraph 1, 뒤 2. The long one gets a
        // second line at 10 (the first paragraph also holds the section
        // controls, so it is not used).
        let xml = add_caches(&section_xml(&bytes));
        let at = xml.match_indices("</hp:linesegarray>").nth(1).expect("second cache").0;
        let xml = format!(
            "{}<hp:lineseg textpos=\"10\" vertpos=\"1600\" vertsize=\"1000\" \
             textheight=\"1000\" baseline=\"850\" spacing=\"600\" horzpos=\"0\" \
             horzsize=\"42520\" flags=\"393216\"/>{}",
            &xml[..at],
            &xml[at..]
        );
        let bytes = with_section_xml(&bytes, xml);
        let warnings = HwpxDecoder::decode(&bytes).expect("decode").warnings;
        assert!(warnings.is_empty(), "{warnings:?}");
        let out = patch(&bytes, |s| set_text(&mut s.paragraphs[1], "A"));
        let out_xml = section_xml(&out);
        assert!(!out_xml.contains("textpos=\"10\""), "{out_xml}");
        assert_eq!(caches(&out_xml), vec![0], "the flow tail goes too");
        let decoded = HwpxDecoder::decode(&out).expect("decode");
        assert!(decoded.warnings.is_empty(), "{:?}", decoded.warnings);
    }

    /// A cell edit drops its paragraph, the cell's later paragraph, the host
    /// paragraph and the body paragraph after it. A header edit drops its own
    /// flow only: a header is laid out apart from the body, so the paragraph
    /// holding its control keeps its lines.
    /// 이것을 실패시키는 것: 꼬리 대신 문단 하나만 · host 사슬 생략 · 목록
    /// 처음부터 · 바뀐 글자의 문단을 바깥 문단으로 잡기.
    #[test]
    fn patch_drops_the_changed_flow_tail_and_host_chain() {
        let width = HwpUnit::new(4000).expect("width");
        let cell = TableCell::new(vec![para("c0"), para("mid"), para("c2")], width);
        let mut host = Paragraph::new(ParaShapeIndex::new(0));
        host.add_run(Run::table(
            Table::new(vec![TableRow::new(vec![cell])]),
            CharShapeIndex::new(0),
        ));
        let mut section =
            Section::with_paragraphs(vec![para("앞"), host, para("뒤")], PageSettings::a4());
        section.headers.push(HeaderFooter::new(vec![para("h0"), para("h1")], ApplyPageType::Both));
        let bytes = encode(section);
        let xml = section_xml(&bytes);
        let bytes = with_section_xml(&bytes, add_caches(&xml));
        let all = caches(&section_xml(&bytes));
        assert_eq!(all.len(), 8, "{all:?}");

        // Close order: h0 0, h1 1, 앞 2, c0 3, mid 4, c2 5, host 6, 뒤 7.
        let cell_edit = |s: &mut Section| {
            let RunContent::Table(table) = &mut s.paragraphs[1].runs[0].content else {
                panic!("table host");
            };
            set_text(&mut table.rows[0].cells[0].paragraphs[1], "많이 길어진 칸");
        };
        assert_eq!(caches(&section_xml(&patch(&bytes, cell_edit))), vec![0, 1, 2, 3]);
        let header_edit = |s: &mut Section| {
            set_text(&mut s.headers[0].paragraphs[0], "H");
        };
        assert_eq!(caches(&section_xml(&patch(&bytes, header_edit))), vec![2, 3, 4, 5, 6, 7]);
        // The body text after the header control belongs to the body
        // paragraph, found through its parent, not the header's flow.
        // 이것을 실패시키는 것: `paragraph_at` 이 부모 대신 host 사슬을 타기
        // (머리말 목록의 host 가 없어 문단을 못 찾는다).
        let body_edit = |s: &mut Section| set_text(&mut s.paragraphs[0], "앞앞");
        assert_eq!(caches(&section_xml(&patch(&bytes, body_edit))), vec![0, 1, 3, 4, 5]);
    }

    /// 이것을 실패시키는 것: 겹침 검사 삭제(뒤 교체가 앞 교체 안을 다시 쓴다).
    #[test]
    fn splice_refuses_overlapping_spans() {
        let ok = super::splice("abcdef", vec![(3..4, "Y".into()), (1..2, "X".into())]);
        assert_eq!(ok.expect("disjoint"), "aXcYef");
        assert!(super::splice("abcdef", vec![(1..3, "X".into()), (2..4, "Y".into())]).is_err());
        assert!(super::splice("abc", vec![(2..9, String::new())]).is_err());
    }
}
