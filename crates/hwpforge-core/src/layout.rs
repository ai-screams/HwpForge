//! Line layout cache (줄 조판 캐시) — Hancom 이 문서에 저장하는 조판 결과.
//!
//! HWPX 의 `<hp:linesegarray>`/`<hp:lineseg>` 와 HWP5 의 `PARA_LINE_SEG`
//! (36바이트 레코드) 가 같은 의미의 캐시를 나른다. 이 모듈은 그 캐시를
//! 공유 모델로 승격한 **decode-only** 표현이다:
//!
//! - 디코더(HWPX/HWP5)는 wire 의 캐시를 [`LayoutCache`] 로 승격한다.
//! - 인코더는 기본적으로 캐시를 **방출하지 않는다** (opt-in 전용).
//!   기존 byte-splice 보존(`layout_carry`)·제거(`strip_line_segs`) 불변식은
//!   그대로 유지된다.
//! - 문서 동등성 비교(admission/golden)는 캐시를 정규화(제거)한 사본으로
//!   수행한다 — [`crate::document::Document::strip_layout_caches`] 참조.
//!
//! 필드 이름·타입은 wire 를 그대로 미러링한다 (발명 금지 — KS X 6101
//! `lineseg` 속성명 기준).

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// 한 줄의 조판 결과 (HWPX `<hp:lineseg>` 1개 / HWP5 LINE_SEG 36바이트 1개).
///
/// 모든 좌표·크기 단위는 HWPUNIT (1pt = 100). `vertpos` 는 본문 흐름
/// 기준 상대값이며 쪽 단위로 리셋된다 (표 셀 안에서는 셀 상대값).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LineSeg {
    /// 줄 시작 텍스트 위치 (문단 텍스트의 UTF-16 코드유닛 오프셋).
    pub textpos: u32,
    /// 줄 세로 위치 (컨테이너 상대, 쪽/셀 단위 리셋).
    pub vertpos: i32,
    /// 줄 전체 높이.
    pub vertsize: i32,
    /// 텍스트 부분 높이.
    pub textheight: i32,
    /// 줄 상단에서 베이스라인까지 거리.
    pub baseline: i32,
    /// 줄 간격 (다음 줄과의 간격).
    pub spacing: i32,
    /// 컬럼 기준 가로 시작 위치.
    pub horzpos: i32,
    /// 줄 가로 폭 (컬럼 폭).
    pub horzsize: i32,
    /// 줄 플래그 비트필드 (wire 그대로 보존 — 해석하지 않음).
    pub flags: u32,
}

/// 문단 하나의 줄 조판 캐시 (HWPX `<hp:linesegarray>` 전체).
///
/// [`crate::paragraph::Paragraph::layout_cache`] 로 부착된다.
/// `lines` 는 wire 순서(첫 줄부터)를 그대로 보존한다.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct LayoutCache {
    /// 줄 세그먼트 목록 (wire 순서).
    pub lines: Vec<LineSeg>,
}

impl LayoutCache {
    /// 주어진 줄 세그먼트들로 캐시를 만든다.
    pub fn new(lines: Vec<LineSeg>) -> Self {
        Self { lines }
    }

    /// 줄 수를 반환한다.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// 줄이 하나도 없으면 `true`.
    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(textpos: u32, vertpos: i32) -> LineSeg {
        LineSeg {
            textpos,
            vertpos,
            vertsize: 1000,
            textheight: 1000,
            baseline: 850,
            spacing: 600,
            horzpos: 0,
            horzsize: 48188,
            flags: 0x0060_0000,
        }
    }

    #[test]
    fn lineseg_boundary_values_roundtrip_serde() {
        let s = LineSeg {
            textpos: u32::MAX,
            vertpos: i32::MIN,
            vertsize: i32::MAX,
            textheight: 0,
            baseline: -1,
            spacing: i32::MIN,
            horzpos: i32::MAX,
            horzsize: 0,
            flags: u32::MAX,
        };
        let json = serde_json::to_string(&s).unwrap();
        let back: LineSeg = serde_json::from_str(&json).unwrap();
        assert_eq!(s, back);
    }

    #[test]
    fn layout_cache_default_is_empty() {
        let c = LayoutCache::default();
        assert!(c.is_empty());
        assert_eq!(c.line_count(), 0);
    }

    #[test]
    fn layout_cache_preserves_wire_order() {
        let c = LayoutCache::new(vec![seg(0, 0), seg(70, 1600), seg(122, 3200)]);
        assert_eq!(c.line_count(), 3);
        assert_eq!(c.lines[1].textpos, 70);
        assert_eq!(c.lines[2].vertpos, 3200);
    }

    #[test]
    fn layout_cache_eq_is_structural() {
        let a = LayoutCache::new(vec![seg(0, 0)]);
        let b = LayoutCache::new(vec![seg(0, 0)]);
        let c = LayoutCache::new(vec![seg(0, 16)]);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn paragraph_json_without_cache_field_deserializes() {
        // 구버전 to-json 산출물(layout_cache 필드 없음) 하위호환
        let old_json = r#"{"runs":[],"para_shape_id":0}"#;
        let p: crate::paragraph::Paragraph = serde_json::from_str(old_json).unwrap();
        assert!(p.layout_cache.is_none());
    }

    #[test]
    fn paragraph_json_omits_cache_when_none() {
        let p = crate::paragraph::Paragraph::new(hwpforge_foundation::ParaShapeIndex::new(0));
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("layout_cache"), "None 캐시는 직렬화 생략: {json}");
        let mut cached = p.clone();
        cached.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
        let json2 = serde_json::to_string(&cached).unwrap();
        assert!(json2.contains("layout_cache"));
        let back: crate::paragraph::Paragraph = serde_json::from_str(&json2).unwrap();
        assert_eq!(cached, back);
    }

    // -----------------------------------------------------------------------
    // 순회/정규화 완전성: 모든 문단 컨테이너를 한 문서에 담아 검증한다.
    // -----------------------------------------------------------------------

    mod traversal {
        use super::*;
        use crate::caption::{Caption, CaptionSide};
        use crate::control::Control;
        use crate::document::Document;
        use crate::page::PageSettings;
        use crate::paragraph::Paragraph;
        use crate::run::Run;
        use crate::section::{HeaderFooter, MasterPage, Section};
        use crate::table::{Table, TableCell, TableRow};
        use hwpforge_foundation::{ApplyPageType, CharShapeIndex, HwpUnit, ParaShapeIndex};

        /// 캐시가 박힌 문단 (텍스트로 방문 추적).
        fn cached_para(text: &str) -> Paragraph {
            let mut p = Paragraph::with_runs(
                vec![Run::text(text, CharShapeIndex::new(0))],
                ParaShapeIndex::new(0),
            );
            p.layout_cache = Some(LayoutCache::new(vec![seg(0, 0)]));
            p
        }

        fn one_cell_table(cell_para: Paragraph) -> Table {
            // 표 수준 decode-only 캐시도 부착 — strip 이 문단 캐시와 함께
            // 제거해야 한다 (out/in_margin 은 구조라 남아야 한다).
            Table::new(vec![TableRow::new(vec![TableCell::new(
                vec![cell_para],
                HwpUnit::from_pt(100.0).unwrap(),
            )])])
            .with_layout_cache(crate::table::TableLayoutCache::new(
                Some(HwpUnit::from_pt(56.7).unwrap()),
                true,
            ))
            .with_out_margin(crate::table::TableMargin::default())
        }

        /// 모든 문단 컨테이너를 담은 문서: 본문·표 셀(중첩 표 포함)·표 캡션·
        /// 머리말·꼬리말·바탕쪽·글상자(+캡션)·묶음(재귀)·메모(본문+앵커 run)·
        /// 각주·타원 본문.
        fn document_with_all_containers() -> (Document<crate::document::Draft>, usize) {
            let mut expected = 0usize;

            // 본문 + 중첩 표(셀 문단 안에 또 표) + 표 캡션
            let inner = one_cell_table(cached_para("inner-cell"));
            let mut mid_cell = cached_para("mid-cell");
            mid_cell.add_run(Run::table(inner, CharShapeIndex::new(0)));
            let outer = one_cell_table(mid_cell)
                .with_caption(Caption::new(vec![cached_para("tbl-caption")], CaptionSide::Bottom));
            let mut host = cached_para("tbl-host");
            host.add_run(Run::table(outer, CharShapeIndex::new(0)));
            expected += 4; // host + mid-cell + inner-cell + caption

            // 글상자(+캡션) — 캡션은 variant 필드라 직접 부착
            let mut textbox = Control::text_box(
                vec![cached_para("textbox-body")],
                HwpUnit::from_pt(100.0).unwrap(),
                HwpUnit::from_pt(50.0).unwrap(),
            );
            if let Control::TextBox { caption, .. } = &mut textbox {
                *caption =
                    Some(Caption::new(vec![cached_para("textbox-caption")], CaptionSide::Top));
            }
            let mut tb_host = cached_para("textbox-host");
            tb_host.add_run(Run::control(textbox, CharShapeIndex::new(0)));
            expected += 3; // host + body + caption

            // 묶음(재귀) — 자식 글상자
            let child = Control::text_box(
                vec![cached_para("group-child-body")],
                HwpUnit::from_pt(10.0).unwrap(),
                HwpUnit::from_pt(10.0).unwrap(),
            );
            let group = Control::Group {
                children: vec![child],
                width: HwpUnit::from_pt(10.0).unwrap(),
                height: HwpUnit::from_pt(10.0).unwrap(),
                horz_offset: 0,
                vert_offset: 0,
                inst_id: None,
            };
            let mut group_host = cached_para("group-host");
            group_host.add_run(Run::control(group, CharShapeIndex::new(0)));
            expected += 2; // host + child body

            // 메모: 본문 + 앵커 run 속 표 셀
            let memo = Control::memo_with_anchor(
                vec![cached_para("memo-body")],
                vec![Run::table(
                    one_cell_table(cached_para("memo-anchor-cell")),
                    CharShapeIndex::new(0),
                )],
            );
            let mut memo_host = cached_para("memo-host");
            memo_host.add_run(Run::control(memo, CharShapeIndex::new(0)));
            expected += 3; // host + body + anchor cell

            // 각주 + 타원 본문 (한 문단에 함께)
            let mut note_host = cached_para("note-host");
            note_host.add_run(Run::control(
                Control::footnote(vec![cached_para("footnote-body")]),
                CharShapeIndex::new(0),
            ));
            note_host.add_run(Run::control(
                Control::ellipse_with_text(
                    HwpUnit::from_pt(20.0).unwrap(),
                    HwpUnit::from_pt(20.0).unwrap(),
                    vec![cached_para("ellipse-body")],
                ),
                CharShapeIndex::new(0),
            ));
            expected += 3; // host + footnote + ellipse

            let mut section = Section::with_paragraphs(
                vec![host, tb_host, group_host, memo_host, note_host],
                PageSettings::a4(),
            );
            section.headers.push(HeaderFooter::all_pages(vec![cached_para("header")]));
            section.footers.push(HeaderFooter::all_pages(vec![cached_para("footer")]));
            section.master_pages =
                Some(vec![MasterPage::new(ApplyPageType::Both, vec![cached_para("master")])]);
            expected += 3; // header + footer + master

            let mut doc = Document::new();
            doc.add_section(section);
            (doc, expected)
        }

        #[test]
        fn for_each_paragraph_mut_visits_every_container() {
            let (mut doc, expected) = document_with_all_containers();
            let mut visited = Vec::new();
            doc.for_each_paragraph_mut(|p| visited.push(p.text_content()));
            assert_eq!(visited.len(), expected, "visited: {visited:?}");
            // 대표 중첩 지점들이 실제로 방문됐는지
            for needle in [
                "inner-cell",
                "tbl-caption",
                "textbox-caption",
                "group-child-body",
                "memo-anchor-cell",
                "footnote-body",
                "master",
            ] {
                assert!(visited.iter().any(|t| t == needle), "missing {needle}: {visited:?}");
            }
        }

        #[test]
        fn strip_layout_caches_clears_every_container() {
            let (mut doc, expected) = document_with_all_containers();
            doc.strip_layout_caches();
            let mut remaining = 0;
            let mut total = 0;
            let mut table_caches = 0;
            let mut table_margins = 0;
            doc.for_each_paragraph_mut(|p| {
                total += 1;
                if p.layout_cache.is_some() {
                    remaining += 1;
                }
                for run in &p.runs {
                    if let crate::run::RunContent::Table(t) = &run.content {
                        table_caches += usize::from(t.layout_cache.is_some());
                        table_margins += usize::from(t.out_margin.is_some());
                    }
                }
            });
            assert_eq!(total, expected);
            assert_eq!(remaining, 0);
            // 표 캐시도 제거 (중첩 표 포함 — one_cell_table 이 전부 부착).
            assert_eq!(table_caches, 0, "table layout caches must be stripped");
            // 반면 out_margin 은 구조 — strip 이 건드리면 안 된다.
            assert!(table_margins >= 2, "structural margins must survive strip");
        }

        #[test]
        fn cache_difference_breaks_eq_until_stripped() {
            let (doc_a, _) = document_with_all_containers();
            let (mut doc_b, _) = document_with_all_containers();
            assert_eq!(doc_a, doc_b);
            doc_b.strip_layout_caches();
            assert_ne!(doc_a, doc_b, "derived eq must still see cache differences");
            let mut doc_a2 = doc_a.clone();
            doc_a2.strip_layout_caches();
            assert_eq!(doc_a2, doc_b, "normalized copies must compare equal");
        }
    }
}
