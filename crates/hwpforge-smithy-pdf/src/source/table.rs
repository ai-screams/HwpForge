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
use hwpforge_core::table::{Table, TableCell, TableMargin, TableVerticalAlign};
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
    let row_heights = compute_row_heights(input, table, &anchors, row_count, location, warnings)?;

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

    struct Frag {
        start_row: usize,
        end_row: usize,
        top_offset: i32, // body 기준 v
    }
    // 분할점은 rowspan 내부에 못 떨어진다 — 병합 블록은 원자 (한컴 CELL
    // 분할도 행 경계·병합 경계만 자른다). r 앞에서 끊어도 되는지 미리 계산.
    let mut break_ok = vec![true; row_count + 1];
    for a in &anchors {
        let (ar, s) = (a.anchor.row as usize, a.row_span as usize);
        break_ok[(ar + 1)..(ar + s).min(row_count)].fill(false);
    }
    let mut frags: Vec<Frag> = Vec::new();
    let mut cursor = host_v + om_top;
    let mut frag_start = 0usize;
    let mut r = 0usize;
    let mut y = cursor;
    while r < row_count {
        let h = row_heights[r];
        if y + h > capacity_end && r > frag_start {
            // 경계를 rowspan-안전 지점으로 되돌린다 (병합 블록 통째 이월).
            let mut cut = r;
            while cut > frag_start && !break_ok[cut] {
                cut -= 1;
            }
            if cut == frag_start {
                return reject("merged row block taller than page body");
            }
            frags.push(Frag { start_row: frag_start, end_row: cut, top_offset: cursor });
            frag_start = cut;
            cursor = om_top + if repeat { header_height } else { 0 };
            y = cursor + row_heights[cut..r].iter().sum::<i32>();
            // r 는 그대로 — 이월된 행들 높이만 새 조각 기준으로 재적산.
        }
        if y + h > capacity_end {
            return reject("table row taller than page body");
        }
        y += h;
        r += 1;
    }
    frags.push(Frag { start_row: frag_start, end_row: row_count, top_offset: cursor });

    if frags.len() > 1 {
        for anchor in &anchors {
            let (r, s) = (anchor.anchor.row as usize, anchor.row_span as usize);
            if s > 1 {
                let within_one = frags.iter().any(|f| r >= f.start_row && r + s <= f.end_row);
                if !within_one {
                    return reject("rowspan across page boundary");
                }
            }
        }
        warnings.push(PdfWarning::TablePaginationComputed { location: location.to_string() });
    }

    // ── 검산 ① 첫 조각 높이 == 재저장 sz (있을 때, 규칙 §3.3 함정 반영) ──
    let frag0_height: i32 = row_heights[frags[0].start_row..frags[0].end_row].iter().sum();
    if let Some(sz) = tlc.saved_sz_height {
        if sz.as_i32() != frag0_height {
            return Err(PdfError::InvalidCache {
                detail: format!(
                    "{location}: computed first-fragment height {frag0_height} != saved sz \
                     height {} (pagination or R1 mismatch)",
                    sz.as_i32()
                ),
            });
        }
    }

    // ── 방출 (조각별 절대좌표 재계산 — 게이트2 H4) ───────────────
    let table_x = geom.body_left + host_h + om_left;
    for (fi, frag) in frags.iter().enumerate() {
        if fi > 0 {
            new_page(pages);
        }
        let mut row_y = geom.body_top + frag.top_offset;
        if fi > 0 && repeat {
            // 연속 조각 상단 제목행 (원본 셀-상대 캐시 재생, 절대좌표만 재계산).
            let mut header_y = geom.body_top + om_top;
            for hr in 0..header_rows {
                emit_row(
                    input,
                    table,
                    &anchors,
                    hr,
                    &col_x,
                    &row_heights,
                    table_x,
                    header_y,
                    &format!("{location}/rep{fi}"),
                    pages,
                    warnings,
                )?;
                header_y += row_heights[hr];
            }
        }
        for r in frag.start_row..frag.end_row {
            let loc = if fi == 0 { location.to_string() } else { format!("{location}/frag{fi}") };
            emit_row(
                input,
                table,
                &anchors,
                r,
                &col_x,
                &row_heights,
                table_x,
                row_y,
                &loc,
                pages,
                warnings,
            )?;
            row_y += row_heights[r];
        }
    }

    // ── 검산 ② 앵커 준비: 다음 문단 v 기대값 ─────────────────────
    // 연속 조각의 top_offset 은 (반복 제목행 포함) 데이터 시작 위치라
    // 마지막 조각 데이터 높이만 더하면 표 하단이 된다.
    let last = frags.last().expect("at least one fragment");
    let last_height: i32 = row_heights[last.start_row..last.end_row].iter().sum();
    let expected_next_v = last.top_offset + last_height + om_bottom;
    let anchor_slack =
        row_heights[last.start_row..last.end_row].iter().copied().min().unwrap_or(1).max(1);
    Ok(TableReplayOutcome { expected_next_v, anchor_slack })
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
    Ok(x.into_iter().map(|v| v.expect("checked") as i32).collect())
}

fn cell_of<'t>(table: &'t Table, anchor: &GridCell) -> &'t TableCell {
    &table.rows[anchor.row_idx].cells[anchor.cell_idx]
}

fn effective_margin(table: &Table, cell: &TableCell) -> TableMargin {
    cell.margin.or(table.in_margin).unwrap_or_default()
}

/// 셀 콘텐츠 세로 범위 = 전 문단 max(마지막 lineseg.v + vertsize).
/// (다문단 셀 v 는 셀 내 연속 누적 — blank-HPC 56셀 실측.)
fn cell_content_extent(
    input: &PdfInput<'_>,
    cell: &TableCell,
    location: &str,
) -> PdfResult<Option<i32>> {
    let mut extent: Option<i32> = None;
    for para in &cell.paragraphs {
        let Some(cache) = para.layout_cache.as_ref().filter(|c| !c.is_empty()) else {
            return Ok(None);
        };
        let last = cache.lines.last().expect("non-empty cache");
        let mut e = last.vertpos + last.vertsize;
        // 중첩 표 host 문단: lineseg 는 한 줄 높이만 알므로 표 흐름 소비
        // (host.v + om.top + Σ행높이 + om.bottom, R5)를 별도 가산한다.
        for run in &para.runs {
            if let hwpforge_core::run::RunContent::Table(nested) = &run.content {
                let om = nested.out_margin.unwrap_or_default();
                let h = flat_table_height(input, nested, location)?;
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
                let mut has_table = false;
                let mut has_text = false;
                for run in &para.runs {
                    match &run.content {
                        hwpforge_core::run::RunContent::Table(_) => has_table = true,
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
            }
        }
    }
    Ok(())
}

/// 중첩 표(행 내부 — 분할 없음)의 총높이 = Σ R1' 행높이.
fn flat_table_height(input: &PdfInput<'_>, table: &Table, location: &str) -> PdfResult<i32> {
    let grid = TableGrid::from_table(table).map_err(|_| PdfError::UnsupportedContent {
        kind: "malformed table grid",
        location: location.to_string(),
    })?;
    let anchors: Vec<&GridCell> = grid.iter_anchors().collect();
    let rows = grid.dimensions().0 as usize;
    let mut scratch = Vec::new();
    let heights = compute_row_heights(input, table, &anchors, rows, location, &mut scratch)?;
    Ok(heights.iter().sum())
}

/// 중첩 표를 고정 원점에 배치한다 (분할 없음 — 행 내부는 쪽 경계를 못 넘는다).
fn place_table_flat(
    input: &PdfInput<'_>,
    table: &Table,
    location: &str,
    origin_x: i32,
    origin_y: i32,
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<i32> {
    let reject = |kind: &'static str| {
        Err(PdfError::UnsupportedContent { kind, location: location.to_string() })
    };
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
        compute_row_heights(input, table, &anchors, row_count as usize, location, warnings)?;
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
            input, table, &anchors, r, &col_x, &heights, origin_x, y, location, pages, warnings,
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
    _warnings: &mut Vec<PdfWarning>,
) -> PdfResult<Vec<i32>> {
    let mut heights = vec![0i32; row_count];
    for a in anchors {
        if a.row_span != 1 {
            continue;
        }
        let cell = cell_of(table, a);
        let m = effective_margin(table, cell);
        let Some(extent) = cell_content_extent(input, cell, location)? else {
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
    while changed {
        changed = false;
        for &(a, r, end) in &spans {
            let cell = cell_of(table, a);
            let m = effective_margin(table, cell);
            let need = cell.height.map_or(0, |h| h.as_i32()).max(
                cell_content_extent(input, cell, location)?
                    .map_or(0, |e| e + m.top.as_i32() + m.bottom.as_i32()),
            );
            let have: i32 = heights[r..end].iter().sum();
            if have < need {
                heights[end - 1] += need - have;
                changed = true;
            }
        }
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
    pages: &mut [PageLayout],
    warnings: &mut Vec<PdfWarning>,
) -> PdfResult<()> {
    for a in anchors.iter().filter(|a| a.anchor.row as usize == row) {
        let cell = cell_of(table, a);
        let (c, s_col, s_row) = (a.anchor.col as usize, a.col_span as usize, a.row_span as usize);
        let cell_loc = format!("{location}/t0r{row}c{c}");
        let x0 = table_x + col_x[c];
        let w = col_x[c + s_col] - col_x[c];
        let h: i32 = row_heights[row..(row + s_row).min(row_heights.len())].iter().sum();

        // 배경 (FillKind — 없음/미지원 구분, 게이트2 M2).
        if let Some(id) = cell.border_fill_id {
            match input.styles.border_fill_face(id) {
                Some(FillKind::Solid(color)) => {
                    let page = pages.last_mut().expect("page exists");
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
                            let page = pages.last_mut().expect("page exists");
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
        let extent = cell_content_extent(input, cell, location)?.unwrap_or(0);
        let inner = h - m.top.as_i32() - m.bottom.as_i32();
        let valign_shift = match cell.vertical_align.unwrap_or(TableVerticalAlign::Top) {
            TableVerticalAlign::Top => 0,
            TableVerticalAlign::Center => ((inner - extent) / 2).max(0),
            TableVerticalAlign::Bottom => (inner - extent).max(0),
        };
        let content_x = x0 + m.left.as_i32();
        let content_y = row_y + m.top.as_i32() + valign_shift;
        for (pi, para) in cell.paragraphs.iter().enumerate() {
            let Some(cache) = para.layout_cache.as_ref().filter(|cc| !cc.is_empty()) else {
                continue; // compute_row_heights 가 이미 fatal 처리 — 방어적 스킵.
            };
            // 중첩 표 host 문단: 셀 원점 기준으로 재귀 배치 (행 내부라 분할 없음).
            let mut hosted_table = false;
            for run in &para.runs {
                if let hwpforge_core::run::RunContent::Table(nested) = &run.content {
                    hosted_table = true;
                    let host_seg = &cache.lines[0];
                    let nom = nested.out_margin.unwrap_or_default();
                    place_table_flat(
                        input,
                        nested,
                        &format!("{cell_loc}/p{pi}/tbl"),
                        content_x + host_seg.horzpos + nom.left.as_i32(),
                        content_y + host_seg.vertpos + nom.top.as_i32(),
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
            if utf16.is_empty() {
                // 빈 문단 — 기하(extent)만 행높이에 기여, 그릴 글리프 없음.
                // 한컴 native 는 텍스트 삭제 후 stale textpos 가 남은 캐시를
                // 쓰기도 한다 (blank-HPC r10c4 실측: <t/> + textpos=49).
                continue;
            }
            let para_loc = format!("{cell_loc}/p{pi}");
            validate_textpos(cache, utf16.len(), &para_loc)?;
            let run_spans = run_utf16_spans(para, warnings, &para_loc);
            let alignment =
                input.styles.para_alignment(para.para_shape_id).unwrap_or(Alignment::Left);
            let line_count = cache.lines.len();
            for (li, seg) in cache.lines.iter().enumerate() {
                let start = seg.textpos as usize;
                let end = cache.lines.get(li + 1).map_or(utf16.len(), |n| n.textpos as usize);
                let runs = slice_line_runs(&utf16, &run_spans, start, end);
                let page = pages.last_mut().expect("page exists");
                page.lines.push(LaidLine {
                    location: format!("{para_loc}/l{li}"),
                    runs,
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
