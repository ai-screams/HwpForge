//! W3c 표 배치소스 — **검증된 프로파일**의 표만 캐시 재생한다.
//!
//! 셀 텍스트·행높이·열폭은 캐시/wire 재생이지만, 분할 표의 **중간 쪽 경계는
//! 캐시에 신호가 없어 계산**한다 (게이트2 C1 실측 — 셀 lineseg 는 전부
//! 셀-상대 v=0, host lineseg 는 1개). 그래서:
//!
//! - 분할이 일어나면 [`PdfWarning::TablePaginationComputed`] 를 **항상** 방출
//! - 계산 결과를 캐시 앵커로 **이중 검산** (fatal): ① 첫 조각 높이 ==
//!   재저장 sz(있을 때) ② 표 다음 문단의 캐시 v == 마지막 조각 종료 +
//!   outMargin.bottom (rules-pagespan3 실측 62102 = 283+48×1282+283, 0 오차)
//!
//! 프로파일 밖(중첩 표·caption·비기본 pos·cellSpacing≠0·분할 경계 교차
//! rowspan·본문보다 큰 행·셀 캐시 결손·열폭 미유일)은 전부 fail-closed.

use hwpforge_core::table::grid::{covered_area, GridCell, TableGrid, MAX_GRID_POSITIONS};
use hwpforge_core::table::{Table, TableCell, TableMargin, TablePageBreak, TableVerticalAlign};
use hwpforge_core::{BorderLineKind, FillKind};
use hwpforge_foundation::Alignment;

use super::{
    run_utf16_spans, slice_line_runs, validate_textpos, LaidBorder, LaidLine, LaidRect, PageLayout,
};
use crate::text::align::LineBox;
use crate::{PdfError, PdfInput, PdfResult, PdfWarning};

/// 분할 조각 하단 예약 (HWPUNIT) — wire `outMargin.bottom` 에 더해지는
/// 고정 여유는 실측상 불요 (rules-pagespan3: 예약 = outMargin.bottom 만으로
/// 44/49/48 분할 정확 재현). blank-HPC 게이트가 반증하면 여기서 재보정.
const SPLIT_BOTTOM_EXTRA: i32 = 0;

/// 재귀 중첩 표 깊이 상한 — 디코더 캡(32)과 무관하게 API 저작 문서 방어.
const MAX_TABLE_DEPTH: usize = 4;

/// 분할 표 첫 조각: 재저장 sz(한컴 실측)와 계산 절단의 허용 편차 (실측 Δ101HU).
const SZ_CUT_TOLERANCE: i32 = 300;

/// 섹션 본문 기하 (HWPUNIT).
pub(crate) struct SectionGeom {
    /// body 상단 오프셋 (margin_top + header_margin).
    pub body_top: i32,
    /// body 좌변 오프셋 (margin_left).
    pub body_left: i32,
    /// body 세로 높이 (쪽 높이 − 상하 여백/머리말/꼬리말).
    pub body_height: i32,
}

/// 표 재생 결과.
pub(crate) struct TableReplayOutcome {
    /// 표 다음 문단의 캐시 v 기대값 (마지막 조각이 놓인 쪽 기준).
    pub expected_next_v: i32,
    /// 앵커 허용 슬랙 — 다음 문단의 문단-위-간격(캐시 밖 속성)은 기대값에
    /// 더해질 수 있다 (blank-HPC 실측 +214HU). 페이지네이션 오류는 행
    /// 단위로 어긋나므로 "마지막 조각 최소 행높이 미만의 양의 편차"만
    /// 허용하면 오류 포착력은 유지된다.
    pub anchor_slack: i32,
    /// 쪽 경계 분할 여부 — 분할 표는 계산 페이지네이션으로만 흐름 전진 가능
    /// (캐시가 연속 조각을 표현하지 못함).
    pub split: bool,
    /// 계산된 표 총높이 (Σ행높이, outMargin 미포함) — 미분할 표의 흐름 모델
    /// 판별용: host lineseg vertsize ≥ 이 값이면 글자취급(인라인) 표라
    /// host 줄이 흐름 소비를 담고(기재부 corpus 실측), 미만이면 앵커형이라
    /// 계산치(`expected_next_v`)가 흐름이다 (rules-table 실측).
    pub total_height: i32,
}

/// 표 host 문단 하나를 재생한다. 분할 시 `pages` 에 새 쪽을 만든다.
#[allow(clippy::too_many_arguments)] // 배치 컨텍스트 전달 — 구조체화는 W3d 정리 후보
pub(crate) fn replay_table(
    input: &PdfInput<'_>,
    table: &Table,
    location: &str,
    geom: &SectionGeom,
    host_v: i32,
    host_h: i32,
    new_page: &dyn Fn(&mut Vec<PageLayout>),
    pages: &mut Vec<PageLayout>,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<TableReplayOutcome> {
    let reject = |kind: &'static str| {
        Err(PdfError::UnsupportedContent { kind, location: location.to_string() })
    };

    // ── admission: 검증된 프로파일 밖 = 거부 ─────────────────────
    if table.caption.is_some() {
        return reject("table caption");
    }
    let Some(tlc) = table.layout_cache else {
        return Err(PdfError::MissingLayoutCache { count: 1, first: location.to_string() });
    };
    if !tlc.default_flow_pos {
        return reject("non-default table position");
    }
    if table.cell_spacing.is_some_and(|s| s.as_i32() != 0) {
        return reject("nonzero table cellSpacing");
    }
    scan_cell_contents(table, location)?;

    // ── 격자 (strict) + 열폭 제약 풀이 ───────────────────────────
    if covered_area(table) > MAX_GRID_POSITIONS {
        return reject("table grid too large");
    }
    let grid = match TableGrid::from_table(table) {
        Ok(grid) => grid,
        Err(_) => return reject("malformed table grid"),
    };
    let (row_count, col_count) = grid.dimensions();
    let (row_count, col_count) = (row_count as usize, col_count as usize);
    let anchors: Vec<&GridCell> = grid.iter_anchors().collect();
    let col_x = solve_column_offsets(table, &anchors, col_count, location)?;

    // ── 행높이 R1' (게이트2 C3: span=1 만 기여, span>1 은 합 제약) ──
    let depth = 0usize; // top-level 표 — 중첩은 emit_row 재귀에서 +1
    let row_heights =
        compute_row_heights(input, table, &anchors, row_count, location, depth, warnings)?;

    // ── 제목행 반복 블록 ─────────────────────────────────────────
    let header_rows = table.rows.iter().take_while(|r| r.is_header).count();
    let repeat = table.repeat_header && header_rows > 0;
    for anchor in &anchors {
        let (r, s) = (anchor.anchor.row as usize, anchor.row_span as usize);
        if s > 1 && r < header_rows && r + s > header_rows {
            return reject("rowspan crossing header boundary");
        }
    }
    let header_height: i32 = row_heights[..header_rows].iter().sum();

    // ── 쪽걸침 분할 (계산 — 규칙 §3.3 + 예약 = outMargin.bottom) ──
    let om = table.out_margin.unwrap_or_default();
    let (om_top, om_bottom, om_left) = (om.top.as_i32(), om.bottom.as_i32(), om.left.as_i32());
    let capacity_end = geom.body_height - om_bottom - SPLIT_BOTTOM_EXTRA;

    // ── 조각 = 표 콘텐츠 y-창 [y0, y1) (0 = 행0 상단) ────────────
    let row_tops: Vec<i32> = {
        let mut acc = 0;
        let mut v = Vec::with_capacity(row_count + 1);
        v.push(0);
        for &h in &row_heights {
            acc += h;
            v.push(acc);
        }
        v
    };
    let total_height = row_tops[row_count];
    // 행 경계 절단 허용 여부 (TABLE 모드에서만 병합 블록 원자 — CELL 모드는
    // 병합 셀도 관통 절단한다: blank-HPC r3..r10 병합 실측).
    let mut break_ok = vec![true; row_count + 1];
    for a in &anchors {
        let (ar, sp) = (a.anchor.row as usize, a.row_span as usize);
        break_ok[(ar + 1)..(ar + sp).min(row_count)].fill(false);
    }
    // CELL 내부 절단: 줄 걸침은 줄 상단으로 스냅 (텍스트 줄은 원자).
    let snap_cut = |mut c: i32| -> i32 {
        loop {
            let mut snapped = c;
            for a in &anchors {
                let cell = cell_of(table, a);
                let m = effective_margin(table, cell);
                let base = row_tops[a.anchor.row as usize] + m.top.as_i32();
                for para in &cell.paragraphs {
                    let Some(cache) = para.layout_cache.as_ref().filter(|cc| !cc.is_empty()) else {
                        continue;
                    };
                    for seg in &cache.lines {
                        let line_top = base + seg.vertpos;
                        if line_top < snapped && snapped < line_top + seg.vertsize {
                            snapped = line_top;
                        }
                    }
                }
            }
            if snapped == c {
                return c;
            }
            c = snapped;
        }
    };
    struct Frag {
        y0: i32,
        y1: i32,
        top_offset: i32,
    }
    let continuation_top = om_top + if repeat { header_height } else { 0 };
    let mut frags: Vec<Frag> = Vec::new();
    let mut y0 = 0i32;
    let mut top = host_v + om_top;
    loop {
        let avail = capacity_end - top;
        if total_height - y0 <= avail {
            frags.push(Frag { y0, y1: total_height, top_offset: top });
            break;
        }
        if avail <= 0 {
            return reject("table fragment has no room on page");
        }
        let limit = y0 + avail;
        let cut = match table.page_break {
            TablePageBreak::None => {
                return reject("unsplittable table taller than page body");
            }
            TablePageBreak::Table => {
                let mut best = None;
                for (ri, &t) in row_tops.iter().enumerate() {
                    if t > y0 && t <= limit && break_ok[ri] {
                        best = Some(t);
                    }
                }
                match best {
                    Some(c) => c,
                    None => return reject("merged row block taller than page body"),
                }
            }
            TablePageBreak::Cell => {
                // 남은 공간을 채우는 임의 절단 (한컴 실측 — r10 행 내부 분할).
                let c = snap_cut(limit);
                if c <= y0 {
                    return reject("table line taller than page body");
                }
                c
            }
        };
        frags.push(Frag { y0, y1: cut, top_offset: top });
        y0 = cut;
        top = continuation_top;
    }

    if frags.len() > 1 {
        // 분할 표 재저장 sz = 한컴의 **첫 조각 높이 실측치** (규칙 §3.3) —
        // 캐시 우선 원칙대로 첫 절단을 그 값으로 교정한다 (blank-HPC 실측
        // Δ101HU 급 미세차). 크게 어긋나면 모델 모순 = fatal.
        if let Some(sz) = tlc.saved_sz_height {
            let saved = sz.as_i32();
            let computed = frags[0].y1;
            if (saved - computed).abs() > SZ_CUT_TOLERANCE {
                return Err(PdfError::InvalidCache {
                    detail: format!(
                        "{location}: saved first-fragment height {saved} vs computed \
                         {computed} (beyond ±{SZ_CUT_TOLERANCE} — pagination model mismatch; \
                         host_v={host_v} capacity_end={capacity_end} heights={row_heights:?})"
                    ),
                });
            }
            if saved != computed && saved > frags[0].y0 && saved < frags[1].y1 {
                frags[0].y1 = saved;
                frags[1].y0 = saved;
            }
        }
        warnings.push(PdfWarning::TablePaginationComputed { location: location.to_string() });
    } else if let Some(sz) = tlc.saved_sz_height {
        // 비분할: Σ행높이 == sz 검산 (규칙 §6).
        if sz.as_i32() != total_height {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: computed table height {total_height} != saved sz height {} \
                     (R1 mismatch; heights={row_heights:?})",
                    sz.as_i32()
                ),
            });
        }
    }

    // ── 방출: 조각 창과 교차하는 앵커를 창-절단으로 그린다 ────────
    let table_x = geom.body_left + host_h + om_left;
    for (fi, frag) in frags.iter().enumerate() {
        if fi > 0 {
            new_page(pages);
        }
        let loc = if fi == 0 { location.to_string() } else { format!("{location}/frag{fi}") };
        if fi > 0 && repeat {
            // 연속 조각 상단 반복 제목행 (원본 셀-상대 캐시, 절대좌표 재계산).
            for a in anchors.iter().filter(|a| (a.anchor.row as usize) < header_rows) {
                let a_top = row_tops[a.anchor.row as usize];
                emit_anchor_clipped(
                    input,
                    table,
                    a,
                    &col_x,
                    &row_heights,
                    table_x,
                    geom.body_top + om_top + a_top,
                    None,
                    &format!("{location}/rep{fi}"),
                    depth,
                    pages,
                    warnings,
                )?;
            }
        }
        for a in &anchors {
            let ar = a.anchor.row as usize;
            let a_top = row_tops[ar];
            let a_bot = a_top
                + row_heights[ar..(ar + a.row_span as usize).min(row_count)].iter().sum::<i32>();
            let w0 = frag.y0.max(a_top);
            let w1 = frag.y1.min(a_bot);
            if w0 >= w1 {
                continue;
            }
            let clip =
                if w0 == a_top && w1 == a_bot { None } else { Some((w0 - a_top, w1 - a_top)) };
            emit_anchor_clipped(
                input,
                table,
                a,
                &col_x,
                &row_heights,
                table_x,
                geom.body_top + frag.top_offset + (w0 - frag.y0),
                clip,
                &loc,
                depth,
                pages,
                warnings,
            )?;
        }
    }

    // ── 앵커: 다음 문단 v 기대값 ─────────────────────────────────
    let last = frags.last().ok_or_else(|| PdfError::InternalInvariant {
        detail: format!("{location}: table replay produced no fragments"),
    })?;
    let expected_next_v = last.top_offset + (last.y1 - last.y0) + om_bottom;
    let anchor_slack = (0..row_count)
        .filter(|&ri| row_tops[ri] < last.y1 && row_tops[ri + 1] > last.y0)
        .map(|ri| row_heights[ri])
        .min()
        .unwrap_or(1)
        .max(1);
    Ok(TableReplayOutcome {
        expected_next_v,
        anchor_slack,
        split: frags.len() > 1,
        total_height: row_heights.iter().sum(),
    })
}

/// 앵커 제약(`x[c+span] − x[c] = width`)으로 열 경계 오프셋을 푼다.
/// 미유일/모순 = 거부 (게이트2 H2).
fn solve_column_offsets(
    table: &Table,
    anchors: &[&GridCell],
    col_count: usize,
    location: &str,
) -> PdfResult<Vec<i32>> {
    let mut x: Vec<Option<i64>> = vec![None; col_count + 1];
    x[0] = Some(0);
    let mut changed = true;
    while changed {
        changed = false;
        for a in anchors {
            let (c, s) = (a.anchor.col as usize, a.col_span as usize);
            let w = i64::from(cell_of(table, a).width.as_i32());
            match (x[c], x[c + s]) {
                (Some(lo), None) => {
                    x[c + s] = Some(lo + w);
                    changed = true;
                }
                (None, Some(hi)) => {
                    x[c] = Some(hi - w);
                    changed = true;
                }
                (Some(lo), Some(hi)) if hi - lo != w => {
                    return Err(PdfError::UnsupportedContent {
                        kind: "inconsistent table column widths",
                        location: location.to_string(),
                    });
                }
                _ => {}
            }
        }
    }
    if x.iter().any(Option::is_none) {
        return Err(PdfError::UnsupportedContent {
            kind: "ambiguous table column widths",
            location: location.to_string(),
        });
    }
    Ok(x.into_iter().flatten().map(|v| v as i32).collect())
}

fn cell_of<'t>(table: &'t Table, anchor: &GridCell) -> &'t TableCell {
    &table.rows[anchor.row_idx].cells[anchor.cell_idx]
}

fn effective_margin(table: &Table, cell: &TableCell) -> TableMargin {
    cell.margin.or(table.in_margin).unwrap_or_default()
}

/// 셀 콘텐츠 세로 범위 = 전 문단 max(마지막 lineseg.v + vertsize).
/// (다문단 셀 v 는 셀 내 연속 누적 — blank-HPC 56셀 실측.)
pub(crate) fn cell_content_extent(
    input: &PdfInput<'_>,
    cell: &TableCell,
    location: &str,
    depth: usize,
) -> PdfResult<Option<i32>> {
    let mut extent: Option<i32> = None;
    for para in &cell.paragraphs {
        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            return Ok(None);
        };
        // W3 w3 (§7 r2 fold-in): 마지막 줄 단독이 아니라 **전 줄의
        // checked max-bottom** — 앞줄의 큰 이미지 bottom 이 마지막 줄을
        // 넘는 stale/overlap 캐시에서 행높이 누락을 막는다 (정상 캐시는
        // last==max — 기존 fixture 정적 대조 전수 동치 확인).
        let mut e = cache
            .lines
            .iter()
            .map(|seg| {
                seg.vertpos.checked_add(seg.vertsize).ok_or_else(|| PdfError::InvalidCache {
                    detail: format!("{location}: cell line bottom overflows i32"),
                })
            })
            .collect::<PdfResult<Vec<_>>>()?
            .into_iter()
            .max()
            .ok_or_else(|| PdfError::InternalInvariant {
                detail: format!("{location}: non-empty cache has no lines"),
            })?;
        // 중첩 표 host 문단: lineseg 는 한 줄 높이만 알므로 표 흐름 소비
        // (host.v + om.top + Σ행높이 + om.bottom, R5)를 별도 가산한다.
        for run in &para.runs {
            if let hwpforge_core::run::RunContent::Table(nested) = &run.content {
                let om = nested.out_margin.unwrap_or_default();
                let h = flat_table_height(input, nested, location, depth + 1)?;
                e = e.max(cache.lines[0].vertpos + om.top.as_i32() + h + om.bottom.as_i32());
            }
        }
        extent = Some(extent.map_or(e, |cur| cur.max(e)));
    }
    Ok(extent)
}

/// 셀 콘텐츠 admission: 중첩 표는 허용(재귀 배치), 그 밖의 비텍스트 run 은
/// 자동 행높이 재현 불가로 거부 (게이트2 M4).
fn scan_cell_contents(table: &Table, location: &str) -> PdfResult<()> {
    for row in &table.rows {
        for cell in &row.cells {
            for para in &cell.paragraphs {
                // W3 (§7 v2 D1): 3상태 — 글자취급 인라인 이미지는 통과,
                // `[Table+Image]` 혼합은 명시 거부 (hosted-table replay 가
                // 문단을 통째로 skip 해 이미지가 무음 폐기되므로 — r2 #2).
                let mut has_table = false;
                let mut has_text = false;
                let mut has_inline_image = false;
                for run in &para.runs {
                    match &run.content {
                        hwpforge_core::run::RunContent::Table(_) => has_table = true,
                        content if crate::source::is_admitted_inline_image(content) => {
                            has_inline_image = true;
                        }
                        content => match content.plain_text() {
                            Some(t) => has_text |= !t.trim().is_empty(),
                            None => {
                                return Err(PdfError::UnsupportedContent {
                                    kind: "non-text content in table cell",
                                    location: location.to_string(),
                                });
                            }
                        },
                    }
                }
                if has_table && has_text {
                    return Err(PdfError::UnsupportedContent {
                        kind: "table mixed with visible text",
                        location: location.to_string(),
                    });
                }
                if has_table && has_inline_image {
                    return Err(PdfError::UnsupportedContent {
                        kind: "table mixed with inline image",
                        location: location.to_string(),
                    });
                }
            }
        }
    }
    Ok(())
}

/// 중첩 표(행 내부 — 분할 없음)의 총높이 = Σ R1' 행높이.
fn flat_table_height(
    input: &PdfInput<'_>,
    table: &Table,
    location: &str,
    depth: usize,
) -> PdfResult<i32> {
    if depth > MAX_TABLE_DEPTH {
        return Err(PdfError::UnsupportedContent {
            kind: "table nesting too deep",
            location: location.to_string(),
        });
    }
    let grid = TableGrid::from_table(table).map_err(|_| PdfError::UnsupportedContent {
        kind: "malformed table grid",
        location: location.to_string(),
    })?;
    let anchors: Vec<&GridCell> = grid.iter_anchors().collect();
    let rows = grid.dimensions().0 as usize;
    let mut scratch = Vec::new();
    let heights = compute_row_heights(input, table, &anchors, rows, location, depth, &mut scratch)?;
    Ok(heights.iter().sum())
}

/// 중첩 표를 고정 원점에 배치한다 (분할 없음 — 행 내부는 쪽 경계를 못 넘는다).
#[allow(clippy::too_many_arguments)]
fn place_table_flat(
    input: &PdfInput<'_>,
    table: &Table,
    location: &str,
    origin_x: i32,
    origin_y: i32,
    depth: usize,
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<i32> {
    let reject = |kind: &'static str| {
        Err(PdfError::UnsupportedContent { kind, location: location.to_string() })
    };
    if depth > MAX_TABLE_DEPTH {
        return reject("table nesting too deep");
    }
    if table.caption.is_some() {
        return reject("table caption");
    }
    let Some(tlc) = table.layout_cache else {
        return Err(PdfError::MissingLayoutCache { count: 1, first: location.to_string() });
    };
    if !tlc.default_flow_pos {
        return reject("non-default table position");
    }
    if table.cell_spacing.is_some_and(|s| s.as_i32() != 0) {
        return reject("nonzero table cellSpacing");
    }
    scan_cell_contents(table, location)?;
    let grid = match TableGrid::from_table(table) {
        Ok(grid) => grid,
        Err(_) => return reject("malformed table grid"),
    };
    let (row_count, col_count) = grid.dimensions();
    let anchors: Vec<&GridCell> = grid.iter_anchors().collect();
    let col_x = solve_column_offsets(table, &anchors, col_count as usize, location)?;
    let heights =
        compute_row_heights(input, table, &anchors, row_count as usize, location, depth, warnings)?;
    let total: i32 = heights.iter().sum();
    // 비분할 검산: Σ행높이 == 재저장 sz (있을 때).
    if let Some(sz) = tlc.saved_sz_height {
        if sz.as_i32() != total {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: nested table height {total} != saved sz {}",
                    sz.as_i32()
                ),
            });
        }
    }
    let mut y = origin_y;
    for (r, &h) in heights.iter().enumerate() {
        emit_row(
            input, table, &anchors, r, &col_x, &heights, origin_x, y, location, depth, pages,
            warnings,
        )?;
        y += h;
    }
    Ok(total)
}

/// R1' 행높이: span=1 셀만 행 최소높이에 기여, span>1 은 합 제약 검사.
fn compute_row_heights(
    input: &PdfInput<'_>,
    table: &Table,
    anchors: &[&GridCell],
    row_count: usize,
    location: &str,
    depth: usize,
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Vec<i32>> {
    let mut heights = vec![0i32; row_count];
    for a in anchors {
        if a.row_span != 1 {
            continue;
        }
        let cell = cell_of(table, a);
        let m = effective_margin(table, cell);
        let Some(extent) = cell_content_extent(input, cell, location, depth)? else {
            // 셀 캐시 결손 = 자동 행높이 재현 불가 (게이트2 C4 — 표 단위 fatal).
            return Err(PdfError::MissingLayoutCache {
                count: 1,
                first: format!("{location}/r{}c{}", a.anchor.row, a.anchor.col),
            });
        };
        let auto = extent + m.top.as_i32() + m.bottom.as_i32();
        let min = cell.height.map_or(0, |h| h.as_i32());
        let r = a.anchor.row as usize;
        heights[r] = heights[r].max(auto.max(min));
    }
    // span>1 합 제약 + deficit 배분: 부족분은 **span 의 마지막 행이 전부
    // 흡수**한다 (rules-rowspan-deficit 실측 2026-08-07 — 표A 명시높이
    // 25000·표B 콘텐츠 30082 모두 행0..n-1 최소 유지 + 마지막 행 몰빵,
    // PDF 행 간격 12.8pt 균일 + 재저장 sz 체크섬 정합). 겹치는 스팬은
    // end-row 오름차순 fixpoint — blank-HPC 표별 앵커 검산이 재검증한다.
    let mut spans: Vec<(&&GridCell, usize, usize)> = anchors
        .iter()
        .filter(|a| a.row_span > 1)
        .map(|a| {
            let r = a.anchor.row as usize;
            let end = (r + a.row_span as usize).min(row_count);
            (a, r, end)
        })
        .collect();
    spans.sort_by_key(|&(_, r, end)| (end, end - r));
    let mut changed = true;
    let mut redistributed = false;
    while changed {
        changed = false;
        for &(a, r, end) in &spans {
            let cell = cell_of(table, a);
            let m = effective_margin(table, cell);
            // C4: 병합 셀도 캐시 결손 = 표 단위 fatal (span-1 과 동일 —
            // 무음 0 처리하면 콘텐츠가 조용히 사라진다, 독립리뷰 H1).
            let Some(extent) = cell_content_extent(input, cell, location, depth)? else {
                return Err(PdfError::MissingLayoutCache {
                    count: 1,
                    first: format!("{location}/r{r}c{}", a.anchor.col),
                });
            };
            let need = cell
                .height
                .map_or(0, |h| h.as_i32())
                .max(extent + m.top.as_i32() + m.bottom.as_i32());
            let have: i32 = heights[r..end].iter().sum();
            if have < need {
                heights[end - 1] += need - have;
                changed = true;
                redistributed = true;
            }
        }
    }
    if redistributed {
        // 배분 규칙(마지막 스팬 행 몰빵)은 실측 1종 + blank-HPC 괘선 대조로
        // 확정 — 그래도 내부 기하가 검산 사각이므로 표면화한다 (독립리뷰 M1).
        warnings.push(PdfWarning::TableDeficitDistributed { location: location.to_string() });
    }
    // 배분 후에도 0 인 행 = 어떤 셀도 높이를 결정 못 함 → 재현 불가.
    for (r, h) in heights.iter().enumerate() {
        if *h <= 0 {
            return Err(PdfError::UnsupportedContent {
                kind: "row height indeterminate (no span-1 cell)",
                location: format!("{location}/r{r}"),
            });
        }
    }
    Ok(heights)
}

/// 한 (논리) 행의 앵커 셀들을 방출한다 — 배경 → 괘선 → 텍스트.
#[allow(clippy::too_many_arguments)]
fn emit_row(
    input: &PdfInput<'_>,
    table: &Table,
    anchors: &[&GridCell],
    row: usize,
    col_x: &[i32],
    row_heights: &[i32],
    table_x: i32,
    row_y: i32,
    location: &str,
    depth: usize,
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    emit_row_clipped(
        input,
        table,
        anchors,
        row,
        col_x,
        row_heights,
        table_x,
        row_y,
        None,
        location,
        depth,
        pages,
        warnings,
    )
}

/// [`emit_row`] 의 절단 판 — `clip = Some((from, to))` 이면 행-로컬 y 창
/// `[from, to)` 만 그린다 (CELL 모드 행 내부 분할 — blank-HPC r10 실측).
#[allow(clippy::too_many_arguments)]
fn emit_row_clipped(
    input: &PdfInput<'_>,
    table: &Table,
    anchors: &[&GridCell],
    row: usize,
    col_x: &[i32],
    row_heights: &[i32],
    table_x: i32,
    row_y: i32,
    clip: Option<(i32, i32)>,
    location: &str,
    depth: usize,
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    for a in anchors.iter().filter(|a| a.anchor.row as usize == row) {
        emit_anchor_clipped(
            input,
            table,
            a,
            col_x,
            row_heights,
            table_x,
            row_y,
            clip,
            location,
            depth,
            pages,
            warnings,
        )?;
    }
    Ok(())
}

/// 앵커 셀 하나를 (필요 시 절단 창으로) 방출한다 — 배경 → 괘선 → 텍스트.
///
/// `clip = Some((from, to))` 는 **앵커-로컬** y 창 (0 = 앵커 셀 상단):
/// 병합 셀이 쪽 경계를 가로지르면 조각별로 다른 창이 들어온다.
#[allow(clippy::too_many_arguments)]
fn emit_anchor_clipped(
    input: &PdfInput<'_>,
    table: &Table,
    a: &GridCell,
    col_x: &[i32],
    row_heights: &[i32],
    table_x: i32,
    row_y: i32,
    clip: Option<(i32, i32)>,
    location: &str,
    depth: usize,
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    {
        let row = a.anchor.row as usize;
        let cell = cell_of(table, a);
        let (c, s_col, s_row) = (a.anchor.col as usize, a.col_span as usize, a.row_span as usize);
        let cell_loc = format!("{location}/t0r{row}c{c}");
        let x0 = table_x + col_x[c];
        let w = col_x[c + s_col] - col_x[c];
        let full_h: i32 = row_heights[row..(row + s_row).min(row_heights.len())].iter().sum();
        let (clip_from, clip_to) = clip.unwrap_or((0, full_h));
        let h = clip_to - clip_from;

        // 배경 (FillKind — 없음/미지원 구분, 게이트2 M2).
        if let Some(id) = cell.border_fill_id.or(table.border_fill_id) {
            match input.styles.border_fill_face(id) {
                Some(FillKind::Solid(color)) => {
                    let page = pages.last_mut().ok_or_else(|| PdfError::InternalInvariant {
                        detail: format!("{cell_loc}: no current page in table emit"),
                    })?;
                    page.rects.push(LaidRect {
                        location: cell_loc.clone(),
                        x: x0,
                        y: row_y,
                        width: w,
                        height: h,
                        color,
                    });
                }
                Some(FillKind::Unsupported) => {
                    warnings.push(PdfWarning::UnsupportedTableStyle {
                        location: cell_loc.clone(),
                        what: "cell fill",
                    });
                }
                Some(FillKind::None) | None | Some(_) => {}
            }
            // 괘선 4변 (Solid 만 — Other 는 경고 후 생략).
            if let Some(lines) = input.styles.border_fill_lines(id) {
                let edges = [
                    (lines.left, (x0, row_y), (x0, row_y + h)),
                    (lines.right, (x0 + w, row_y), (x0 + w, row_y + h)),
                    (lines.top, (x0, row_y), (x0 + w, row_y)),
                    (lines.bottom, (x0, row_y + h), (x0 + w, row_y + h)),
                ];
                for (line, from, to) in edges {
                    match line.kind {
                        BorderLineKind::Solid => {
                            let page =
                                pages.last_mut().ok_or_else(|| PdfError::InternalInvariant {
                                    detail: format!("{cell_loc}: no current page in table emit"),
                                })?;
                            page.borders.push(LaidBorder {
                                location: cell_loc.clone(),
                                from,
                                to,
                                width: line.width.as_i32(),
                                color: line.color,
                            });
                        }
                        BorderLineKind::None => {}
                        _ => {
                            warnings.push(PdfWarning::UnsupportedTableStyle {
                                location: cell_loc.clone(),
                                what: "border line style",
                            });
                        }
                    }
                }
            }
        }

        // 셀 텍스트 (셀-상대 캐시 → 페이지 절대 재배치).
        let m = effective_margin(table, cell);
        let extent = cell_content_extent(input, cell, location, depth)?.unwrap_or(0);
        // 세로정렬은 행 전체 높이 기준 — 절단 창은 그 위에 씌운다.
        let inner = full_h - m.top.as_i32() - m.bottom.as_i32();
        let valign_shift = match cell.vertical_align.unwrap_or(TableVerticalAlign::Top) {
            TableVerticalAlign::Top => 0,
            TableVerticalAlign::Center => ((inner - extent) / 2).max(0),
            TableVerticalAlign::Bottom => (inner - extent).max(0),
        };
        let content_x = x0 + m.left.as_i32();
        let content_y = row_y - clip_from + m.top.as_i32() + valign_shift;
        for (pi, para) in cell.paragraphs.iter().enumerate() {
            let Some(cache) = para.layout_cache.as_ref().filter(|cc| !cc.is_empty()) else {
                continue; // compute_row_heights 가 전 셀(span 포함) fatal 처리 — 방어적 스킵.
            };
            // 중첩 표 host 문단: 셀 원점 기준으로 재귀 배치 (행 내부라 분할 없음).
            let mut hosted_table = false;
            for run in &para.runs {
                if let hwpforge_core::run::RunContent::Table(nested) = &run.content {
                    hosted_table = true;
                    if clip.is_some() {
                        continue; // 분할 창에서 중첩 표 없음 (splittable 가드) — 방어
                    }
                    let host_seg = &cache.lines[0];
                    let nom = nested.out_margin.unwrap_or_default();
                    place_table_flat(
                        input,
                        nested,
                        &format!("{cell_loc}/p{pi}/tbl"),
                        content_x + host_seg.horzpos + nom.left.as_i32(),
                        content_y + host_seg.vertpos + nom.top.as_i32(),
                        depth + 1,
                        pages,
                        warnings,
                    )?;
                }
            }
            if hosted_table {
                continue; // scan_cell_contents 가 표+가시 텍스트 혼재를 이미 거부.
            }
            let text = para.text_content();
            let utf16: Vec<u16> = text.encode_utf16().collect();
            let para_loc = format!("{cell_loc}/p{pi}");
            // W3 (§7 v2): admitted 이미지 보유 문단은 **빈 텍스트 continue
            // 앞에서** helper 경로로 분기 (image-only 셀 도달 — r2 #4).
            let has_admitted_images =
                para.runs.iter().any(|r| crate::source::is_admitted_inline_image(&r.content));
            let line_atoms_override = if has_admitted_images {
                let atoms = crate::source::build_inline_image_line_atoms(para, cache, &para_loc)?;
                for (li, line) in atoms.iter().enumerate() {
                    for atom in line {
                        if let crate::source::LineAtom::Image(img) = atom {
                            let seg = &cache.lines[li];
                            if !crate::source::line_matches_image_height(seg, img.height) {
                                return Err(PdfError::InvalidCache {
                                    detail: format!(
                                        "{para_loc}/l{li}: image height {} != line vertsize {} / \
                                         textheight {} — unmeasured height profile",
                                        img.height, seg.vertsize, seg.textheight
                                    ),
                                });
                            }
                        }
                    }
                }
                Some(atoms)
            } else {
                None
            };
            if utf16.is_empty() && line_atoms_override.is_none() {
                // 빈 문단 — 기하(extent)만 행높이에 기여, 그릴 글리프 없음.
                // 한컴 native 는 텍스트 삭제 후 stale textpos 가 남은 캐시를
                // 쓰기도 한다 (blank-HPC r10c4 실측: <t/> + textpos=49).
                continue;
            }
            validate_textpos(cache, utf16.len(), &para_loc)?;
            let run_spans = if line_atoms_override.is_none() {
                run_utf16_spans(para, warnings, &para_loc)
            } else {
                Vec::new()
            };
            let alignment =
                input.styles.para_alignment(para.para_shape_id).unwrap_or(Alignment::Left);
            let line_count = cache.lines.len();
            for (li, seg) in cache.lines.iter().enumerate() {
                let line_top = m.top.as_i32() + valign_shift + seg.vertpos;
                if line_top < clip_from || line_top >= clip_to {
                    continue; // 절단 창 밖 줄 — 줄은 상단 기준으로 한 조각에 배정
                }
                // W3 w3 (§7 r2 fold-in): **내부 절단선**(from>0·to<full_h —
                // 셀 외곽은 절단선 아님)이 이미지 줄을 strict 관통하면
                // 잘림/넘침 (이미지에 clip IR 없음 — W5/W6) → fail-closed.
                // boundary touch 는 허용.
                if line_atoms_override.is_some() {
                    let line_bottom = line_top.checked_add(seg.vertsize).ok_or_else(|| {
                        PdfError::InvalidCache {
                            detail: format!("{para_loc}/l{li}: line bottom overflows i32"),
                        }
                    })?;
                    let has_image = matches!(
                        &line_atoms_override,
                        Some(per) if per[li]
                            .iter()
                            .any(|a| matches!(a, crate::source::LineAtom::Image(_)))
                    );
                    let crosses_from =
                        clip_from > 0 && line_top < clip_from && clip_from < line_bottom;
                    let crosses_to =
                        clip_to < full_h && line_top < clip_to && clip_to < line_bottom;
                    if has_image && (crosses_from || crosses_to) {
                        return Err(PdfError::InvalidCache {
                            detail: format!(
                                "{para_loc}/l{li}: split boundary intersects an image \
                                 line — image clipping is not renderable before W5"
                            ),
                        });
                    }
                }
                let start = seg.textpos as usize;
                let end = cache.lines.get(li + 1).map_or(utf16.len(), |n| n.textpos as usize);
                let line_atoms = match &line_atoms_override {
                    Some(per_line) => per_line[li].clone(),
                    None => slice_line_runs(&utf16, &run_spans, start, end)
                        .into_iter()
                        .map(crate::source::LineAtom::Text)
                        .collect(),
                };
                let page = pages.last_mut().ok_or_else(|| PdfError::InternalInvariant {
                    detail: format!("{para_loc}: no current page in table emit"),
                })?;
                page.lines.push(LaidLine {
                    location: format!("{para_loc}/l{li}"),
                    atoms: line_atoms,
                    top_y: content_y + seg.vertpos,
                    baseline_y: content_y + seg.vertpos + seg.baseline,
                    line_box: LineBox { horzpos: content_x + seg.horzpos, horzsize: seg.horzsize },
                    is_last_line: li + 1 == line_count,
                    alignment,
                });
            }
        }
    }
    Ok(())
}
